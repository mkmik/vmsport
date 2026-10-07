//! RMS on the host: files of every organization as record streams, their
//! blocks in host files, shared between processes through vmsportd's lock
//! manager (docs/design/m3.md).

use crate::fileinfo::CLUSTER;
use crate::files::{self, io_status};
use std::os::unix::fs::{FileExt, MetadataExt};
use std::path::{Path, PathBuf};
use vms_cond::Cond;
use vms_rms::idx::{self, Cursor, Found};
use vms_rms::rel::Rel;
use vms_rms::{BLOCK, Blocks, Design, Fab, Org, Record, Rfa, Rfm, status};
use vmsportd::Client;
use vmsportd::locks::{self, Mode};

/// A host file as VMS blocks: VBN n is bytes `(n-1)*512 .. n*512`.
pub struct HostBlocks {
    pub file: std::fs::File,
    blocks: u32,
}

impl HostBlocks {
    pub fn new(file: std::fs::File) -> Result<HostBlocks, Cond> {
        let len = file.metadata().map_err(io_status)?.len();
        Ok(HostBlocks {
            file,
            blocks: len.div_ceil(BLOCK as u64) as u32,
        })
    }
}

impl Blocks for HostBlocks {
    fn read(&mut self, vbn: u32, buf: &mut [u8]) -> Result<(), Cond> {
        let at = (vbn as u64 - 1) * BLOCK as u64;
        // Past the end of the host file (a short last block) reads zeros.
        buf.fill(0);
        let mut done = 0;
        while done < buf.len() {
            match self.file.read_at(&mut buf[done..], at + done as u64) {
                Ok(0) => break,
                Ok(n) => done += n,
                Err(_) => return Err(status::RER),
            }
        }
        Ok(())
    }

    fn write(&mut self, vbn: u32, buf: &[u8]) -> Result<(), Cond> {
        let at = (vbn as u64 - 1) * BLOCK as u64;
        self.file.write_all_at(buf, at).map_err(|_| status::WER)?;
        self.blocks = self.blocks.max(vbn - 1 + buf.len().div_ceil(BLOCK) as u32);
        Ok(())
    }

    fn allocated(&self) -> u32 {
        self.blocks
    }

    fn grow(&mut self, n: u32) -> Result<u32, Cond> {
        let first = self.blocks + 1;
        self.file
            .set_len((self.blocks + n) as u64 * BLOCK as u64)
            .map_err(|_| status::EXT)?;
        self.blocks += n;
        Ok(first)
    }
}

/// FAB$B_FAC and FAB$B_SHR bits.
pub mod fab {
    pub const PUT: u8 = 1;
    pub const GET: u8 = 2;
    pub const DEL: u8 = 4;
    pub const UPD: u8 = 8;
    /// SHR only: no sharing at all.
    pub const NIL: u8 = 0x20;
}

/// The file lock an open takes for its access and sharing: others hold
/// theirs in compatible modes exactly when each one's sharing allows the
/// other's access.
pub fn file_lock_mode(fac: u8, shr: u8) -> Mode {
    let writes = fab::PUT | fab::DEL | fab::UPD;
    let (writer, shares_writes) = (fac & writes != 0, shr & writes != 0);
    match () {
        _ if shr & fab::NIL != 0 || shr & (writes | fab::GET) == 0 => Mode::EX,
        _ if shares_writes => {
            if writer {
                Mode::CW
            } else {
                Mode::CR
            }
        }
        _ if writer => Mode::PW,
        _ => Mode::PR,
    }
}

/// How a $GET or $FIND finds its record.
#[derive(Debug, Clone, Copy)]
pub enum At<'a> {
    /// The next record (RAB$C_SEQ), in the order of the key the last keyed
    /// access used.
    Next,
    /// By key (RAB$C_KEY): key of reference, value, match. A relative
    /// file's key is its record number, 4 bytes.
    Key(u8, &'a [u8], Match),
    /// By RFA (RAB$C_RFA).
    Rfa(Rfa),
}

/// How a key value matches: RAB$V_KGE, RAB$V_KGT, and VMS 8.4's reverse
/// searches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Match {
    Eq,
    Ge,
    Gt,
    Le,
    Lt,
}

/// The RAB$L_ROP options record operations take.
#[derive(Debug, Clone, Copy, Default)]
pub struct Rop {
    /// RAB$V_NLK: don't lock the record.
    pub nolock: bool,
    /// RAB$V_WAT: wait for a locked record rather than fail with RLK.
    pub wait: bool,
}

/// What the organization keeps between calls: the file, and where the
/// next sequential record is.
enum Kind {
    /// ponytail: a sequential file is read whole at open and only appended
    /// to; $UPDATE in place when something needs it.
    Seq {
        records: Vec<Record>,
        next: usize,
        out: Option<files::Writer>,
    },
    Rel {
        rel: Rel,
        b: HostBlocks,
        next: u32,
    },
    Idx {
        f: idx::File<HostBlocks>,
        /// The key sequential $GETs follow (the last keyed access's), and
        /// the record they are past.
        key: u8,
        next: Option<Cursor>,
    },
}

/// Where the next sequential record is, kept to go back to.
#[derive(Clone)]
enum Mark {
    Seq(usize),
    Rel(u32),
    Idx(u8, Option<Cursor>),
}

impl Kind {
    fn mark(&self) -> Mark {
        match self {
            Kind::Seq { next, .. } => Mark::Seq(*next),
            Kind::Rel { next, .. } => Mark::Rel(*next),
            Kind::Idx { key, next, .. } => Mark::Idx(*key, next.clone()),
        }
    }

    fn reset(&mut self, m: Mark) {
        match (self, m) {
            (Kind::Seq { next, .. }, Mark::Seq(n)) => *next = n,
            (Kind::Rel { next, .. }, Mark::Rel(n)) => *next = n,
            (Kind::Idx { key, next, .. }, Mark::Idx(k, n)) => (*key, *next) = (k, n),
            _ => {}
        }
    }
}

/// This open's connection to the lock manager: its locks go when the file
/// closes, or the process ends.
struct Locks {
    client: Client,
    /// Resource names start with the host file's identity.
    id: String,
    /// Others may change the file: operations take the structure lock and
    /// records are locked.
    shared: bool,
    record: Option<u32>,
}

impl Locks {
    /// The file lock for `fac` access sharing `shr`: RMS$_FLK if another
    /// open's access or sharing is incompatible.
    fn take(path: &Path, fac: u8, shr: u8) -> Result<Locks, Cond> {
        let md = std::fs::metadata(path).map_err(io_status)?;
        let id = format!("RMS${:X}.{:X}", md.dev(), md.ino());
        let client = Client::connect().map_err(|_| status::RER)?;
        client
            .enq(&id, file_lock_mode(fac, shr), true)
            .map_err(|c| {
                if c == locks::NOTQUEUED {
                    status::FLK
                } else {
                    c
                }
            })?;
        let writes = fab::PUT | fab::DEL | fab::UPD;
        let shared =
            shr & fab::NIL == 0 && shr & (writes | fab::GET) != 0 && (fac | shr) & writes != 0;
        Ok(Locks {
            client,
            id,
            shared,
            record: None,
        })
    }
}

/// An open file and its record stream (one per open; ponytail: RAB$V_MSE's
/// several streams when something needs them).
pub struct File {
    pub path: PathBuf,
    pub fab: Fab,
    kind: Kind,
    /// The record $UPDATE and $DELETE act on.
    current: Option<Rfa>,
    locks: Locks,
}

const WRITES: u8 = fab::PUT | fab::DEL | fab::UPD;

impl File {
    /// $OPEN and $CONNECT: an existing file for `fac` access, sharing
    /// `shr`.
    pub fn open(path: &Path, fac: u8, shr: u8) -> Result<File, Cond> {
        let fab = files::fab(path);
        // Read-only access shares reads unless told otherwise, as in RMS.
        let shr = if shr == 0 && fac & WRITES == 0 {
            fab::GET
        } else {
            shr
        };
        let locks = Locks::take(path, fac, shr)?;
        let kind = match fab.org {
            Org::Seq => {
                let mut r = files::Reader::open(path)?;
                let out = match fac & fab::PUT {
                    0 => None,
                    _ => Some(files::Writer::append(path)?),
                };
                Kind::Seq {
                    records: std::iter::from_fn(|| r.get()).collect(),
                    next: 0,
                    out,
                }
            }
            Org::Rel => Kind::Rel {
                rel: Rel::new(fab)?,
                b: HostBlocks::new(host_file(path, fac & WRITES != 0, false)?)?,
                next: 1,
            },
            Org::Idx => {
                let b = HostBlocks::new(host_file(path, fac & WRITES != 0, false)?)?;
                let mut f = idx::File::new(b, fab.rfm == Rfm::Fix, fab.mrs);
                // Extensions come in clusters, as on a VMS disk.
                f.cluster = CLUSTER;
                Kind::Idx {
                    f,
                    key: 0,
                    next: None,
                }
            }
        };
        Ok(File {
            path: path.to_path_buf(),
            fab,
            kind,
            current: None,
            locks,
        })
    }

    /// $CREATE and $CONNECT: a new file as `d` designs it.
    pub fn create(path: &Path, d: &Design, fac: u8, shr: u8) -> Result<File, Cond> {
        let (fab, kind) = match d.fab.org {
            Org::Seq => {
                let w = files::Writer::create(path, d.fab)?;
                let kind = Kind::Seq {
                    records: Vec::new(),
                    next: 0,
                    out: Some(w),
                };
                (d.fab, kind)
            }
            Org::Rel => {
                let mut b = HostBlocks::new(host_file(path, true, true)?)?;
                // Allocations come in clusters, as on a VMS disk.
                let alloc = d.areas.first().map_or(0, |a| a.allocation).max(1);
                b.grow(alloc.div_ceil(CLUSTER) * CLUSTER)?;
                let rel = vms_rms::rel::create(&mut b, d)?;
                crate::sys::set_xattr(path, vms_rms::XATTR, rel.fab.to_string().as_bytes())
                    .map_err(|_| status::CRE)?;
                (rel.fab, Kind::Rel { rel, b, next: 1 })
            }
            Org::Idx => {
                let b = HostBlocks::new(host_file(path, true, true)?)?;
                let mut f = idx::File::create(b, d, CLUSTER)?;
                // What RMS keeps: the largest bucket, area 0's extension,
                // the record size of fixed records.
                let areas = f.prologue()?.areas;
                let fab = Fab {
                    bks: areas.iter().map(|a| a.bucket_size).max().unwrap_or(1),
                    deq: match d.fab.deq {
                        0 => areas.first().map_or(0, |a| a.extension),
                        n => n,
                    },
                    lrl: if d.fab.rfm == Rfm::Fix { d.fab.mrs } else { 0 },
                    ..d.fab
                };
                crate::sys::set_xattr(path, vms_rms::XATTR, fab.to_string().as_bytes())
                    .map_err(|_| status::CRE)?;
                let kind = Kind::Idx {
                    f,
                    key: 0,
                    next: None,
                };
                (fab, kind)
            }
        };
        let locks = Locks::take(path, fac | fab::PUT, shr)?;
        Ok(File {
            path: path.to_path_buf(),
            fab,
            kind,
            current: None,
            locks,
        })
    }

    /// $CONNECT with RAB$V_EOF: sequential $PUTs go at the end.
    pub fn to_end(&mut self) -> Result<(), Cond> {
        let Locks {
            client, id, shared, ..
        } = &self.locks;
        let held = structure(client, id, *shared)?;
        let r = match &mut self.kind {
            Kind::Seq { records, next, .. } => {
                *next = records.len();
                Ok(())
            }
            Kind::Rel { rel, b, next } => rel.last(b).map(|n| *next = n + 1),
            Kind::Idx { .. } => Ok(()),
        };
        release(client, held);
        r
    }

    /// $GET: the record, locked for this stream when others may change it.
    pub fn get(&mut self, at: At, rop: Rop) -> Result<Record, Cond> {
        self.find(at, rop).map(|(_, r)| r)
    }

    /// $FIND (and $GET): the record's RFA and the record.
    pub fn find(&mut self, at: At, rop: Rop) -> Result<(Rfa, Record), Cond> {
        // A stream's next operation frees the record it locked.
        self.unlock();
        self.current = None;
        let mark = self.kind.mark();
        let (rfa, rec) = self.op(|k| locate(k, at))?;
        if !self.locks.shared || rop.nolock {
            self.current = Some(rfa);
            return Ok((rfa, rec));
        }
        // A record another stream holds isn't passed by.
        if let Err(e) = self.lock(rfa, rop) {
            self.kind.reset(mark);
            return Err(e);
        }
        // It may have changed before the lock was ours.
        let rec = self.op(|k| locate(k, At::Rfa(rfa)).map(|r| r.1))?;
        self.current = Some(rfa);
        Ok((rfa, rec))
    }

    /// $PUT: sequentially, or (a relative file) as record number `key`.
    /// RMS$_OK_DUP when an indexed file's record repeats a key.
    pub fn put(&mut self, rec: &Record, key: Option<u32>) -> Result<Cond, Cond> {
        self.unlock();
        let normal = |r: Result<(), Cond>| r.map(|()| status::NORMAL);
        self.op(|k| match k {
            Kind::Seq { out, .. } => normal(out.as_mut().ok_or(status::FAC)?.put(rec)),
            Kind::Rel { rel, b, next } => normal(match key {
                Some(n) => rel.put(b, n, rec),
                None => rel.put(b, *next, rec).map(|()| *next += 1),
            }),
            Kind::Idx { f, .. } => f.put(&rec.data).map(|(_, st)| st),
        })
    }

    /// $UPDATE of the current record.
    pub fn update(&mut self, rec: &Record) -> Result<Cond, Cond> {
        let rfa = self.current.ok_or(status::CUR)?;
        let r = self.op(|k| match k {
            Kind::Seq { .. } => Err(status::IOP),
            Kind::Rel { rel, b, .. } => rel.update(b, rfa.vbn, rec).map(|()| status::NORMAL),
            Kind::Idx { f, .. } => f.update(rfa, &rec.data),
        });
        self.unlock();
        r
    }

    /// $DELETE of the current record.
    pub fn delete(&mut self) -> Result<Cond, Cond> {
        let rfa = self.current.take().ok_or(status::CUR)?;
        let r = self.op(|k| match k {
            Kind::Seq { .. } => Err(status::IOP),
            Kind::Rel { rel, b, .. } => rel.delete(b, rfa.vbn),
            Kind::Idx { f, .. } => f.delete(rfa),
        });
        self.unlock();
        r.map(|()| status::NORMAL)
    }

    /// $REWIND: the next record is the first again, in key `krf`'s
    /// order for an indexed file.
    pub fn rewind(&mut self, krf: u8) {
        self.unlock();
        self.current = None;
        match &mut self.kind {
            Kind::Seq { next, .. } => *next = 0,
            Kind::Rel { next, .. } => *next = 1,
            Kind::Idx { key, next, .. } => (*key, *next) = (krf, None),
        }
    }

    /// `f` on the file, under the structure lock when it is shared.
    fn op<T>(&mut self, f: impl FnOnce(&mut Kind) -> Result<T, Cond>) -> Result<T, Cond> {
        let Locks {
            client, id, shared, ..
        } = &self.locks;
        let held = structure(client, id, *shared)?;
        let r = f(&mut self.kind);
        release(client, held);
        r
    }

    fn lock(&mut self, rfa: Rfa, rop: Rop) -> Result<(), Cond> {
        let l = &mut self.locks;
        let name = format!("{}R{}.{}", l.id, rfa.vbn, rfa.id);
        loop {
            match l.client.enq(&name, Mode::EX, true) {
                Ok((id, _)) => {
                    l.record = Some(id);
                    return Ok(());
                }
                // ponytail: waiting polls, so no wait holds the structure lock.
                Err(c) if c == locks::NOTQUEUED && rop.wait => {
                    std::thread::sleep(std::time::Duration::from_millis(10))
                }
                Err(c) if c == locks::NOTQUEUED => return Err(status::RLK),
                Err(c) => return Err(c),
            }
        }
    }

    /// $FREE: the record lock goes.
    pub fn unlock(&mut self) {
        if let Some(id) = self.locks.record.take() {
            let _ = self.locks.client.deq(id);
        }
    }

    /// What the file is: its FAB, and an indexed file's areas and keys as
    /// its prologue has them ($DISPLAY's XABKEYs and XABALLs).
    pub fn design(&mut self) -> Result<Design, Cond> {
        let fab = self.fab;
        self.op(|k| match k {
            Kind::Idx { f, .. } => f.design(fab),
            _ => Ok(Design {
                fab,
                ..Design::default()
            }),
        })
    }
}

fn structure(client: &Client, id: &str, shared: bool) -> Result<Option<u32>, Cond> {
    if !shared {
        return Ok(None);
    }
    client
        .enq(&format!("{id}S"), Mode::EX, false)
        .map(|(id, _)| Some(id))
}

fn release(client: &Client, held: Option<u32>) {
    if let Some(id) = held {
        let _ = client.deq(id);
    }
}

fn host_file(path: &Path, write: bool, new: bool) -> Result<std::fs::File, Cond> {
    std::fs::OpenOptions::new()
        .read(true)
        .write(write)
        .create_new(new)
        .open(path)
        .map_err(io_status)
}

/// A relative file's key: its record number, 4 bytes.
fn record_number(key: &[u8]) -> Result<u32, Cond> {
    let k: [u8; 4] = key.try_into().map_err(|_| status::KSZ)?;
    Ok(u32::from_le_bytes(k))
}

/// The record `at` names, and the next record moved past it.
fn locate(k: &mut Kind, at: At) -> Result<(Rfa, Record), Cond> {
    match k {
        Kind::Seq { records, next, .. } => {
            let i = match at {
                At::Next => *next,
                At::Rfa(r) => r.vbn as usize - 1,
                At::Key(..) => return Err(status::RAC),
            };
            let rec = records.get(i).ok_or(match at {
                At::Next => status::EOF,
                _ => status::RNF,
            })?;
            *next = i + 1;
            Ok((
                Rfa {
                    vbn: i as u32 + 1,
                    id: 0,
                },
                rec.clone(),
            ))
        }
        Kind::Rel { rel, b, next } => {
            let (n, rec) = match at {
                At::Next => rel.next(b, *next - 1)?,
                At::Rfa(r) => (r.vbn, rel.get(b, r.vbn)?),
                At::Key(_, key, m) => {
                    let n = record_number(key)?;
                    match m {
                        Match::Eq => (n, rel.get(b, n)?),
                        Match::Ge => rel.next(b, n.saturating_sub(1))?,
                        Match::Gt => rel.next(b, n)?,
                        // ponytail: reverse searches look back cell by cell.
                        Match::Le | Match::Lt => {
                            let top = if m == Match::Le {
                                n
                            } else {
                                n.saturating_sub(1)
                            };
                            (1..=top)
                                .rev()
                                .find_map(|i| rel.get(b, i).ok().map(|r| (i, r)))
                                .ok_or(status::RNF)?
                        }
                    }
                }
            };
            *next = n + 1;
            Ok((Rfa { vbn: n, id: 0 }, rec))
        }
        Kind::Idx { f, key, next } => {
            let found = match at {
                At::Next => match next {
                    Some(c) => f.next(c)?,
                    None => f.first(*key)?,
                },
                At::Rfa(r) => return Ok((r, Record::new(f.get_rfa(r)?))),
                At::Key(krf, value, m) => {
                    let found = match m {
                        Match::Eq => f.get(krf, value, idx::Match::Eq)?,
                        Match::Ge => f.get(krf, value, idx::Match::Ge)?,
                        Match::Gt => f.get(krf, value, idx::Match::Gt)?,
                        Match::Le | Match::Lt => before(f, krf, value, m == Match::Le)?,
                    };
                    *key = krf;
                    found
                }
            };
            *next = Some(found.at.clone());
            Ok((found.rfa, Record::new(found.record)))
        }
    }
}

/// The last record of key `krf` before `value`, or at it if `or_equal`.
/// ponytail: reads the key's order from its start.
fn before(
    f: &mut idx::File<HostBlocks>,
    krf: u8,
    value: &[u8],
    or_equal: bool,
) -> Result<Found, Cond> {
    let desc = f
        .prologue()?
        .keys
        .get(krf as usize)
        .ok_or(status::KRF)?
        .desc
        .clone();
    if value.is_empty() || value.len() > desc.length() {
        return Err(status::KSZ);
    }
    let mut last = None;
    let mut cur = f.first(krf);
    while let Ok(found) = cur {
        let o = idx::compare(&desc, &desc.extract(&found.record), value);
        if o == std::cmp::Ordering::Greater || o == std::cmp::Ordering::Equal && !or_equal {
            break;
        }
        cur = f.next(&found.at);
        last = Some(found);
    }
    last.ok_or(status::RNF)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fab::*;

    #[test]
    fn sharing() {
        let all = PUT | GET | DEL | UPD;
        let m = |fac, shr| file_lock_mode(fac, shr);
        let ok = |a: (u8, u8), b: (u8, u8)| m(a.0, a.1).compatible(m(b.0, b.1));
        // Readers sharing reads; writers sharing everything.
        assert!(ok((GET, GET), (GET, GET)));
        assert!(ok((GET | PUT, all), (GET | PUT, all)));
        // A writer that shares reads, with readers that allow writers.
        assert!(ok((GET | PUT, GET), (GET, all)));
        assert!(!ok((GET | PUT, GET), (GET, GET)));
        assert!(!ok((GET | PUT, GET), (GET | PUT, GET)));
        // No sharing.
        assert!(!ok((GET, NIL), (GET, all)));
        assert!(!ok((GET, 0), (GET, GET)));
    }

    #[test]
    fn blocks() {
        let path = std::env::temp_dir().join(format!("vpt-hb-{}", std::process::id()));
        let f = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)
            .unwrap();
        let mut b = HostBlocks::new(f).unwrap();
        assert_eq!(b.grow(2).unwrap(), 1);
        b.write(2, &[7; 512]).unwrap();
        let mut buf = [0; 1024];
        b.read(1, &mut buf).unwrap();
        assert_eq!((buf[0], buf[512], b.allocated()), (0, 7, 2));
        std::fs::remove_file(&path).unwrap();
    }
}
