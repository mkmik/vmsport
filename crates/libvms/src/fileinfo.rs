//! What the utilities show about a file beyond its records: the header
//! ANALYZE/RMS_FILE reports, F$FILE_ATTRIBUTES' items, the prologue counts
//! DIRECTORY/FULL gives. Where the host keeps nothing VMS would show, the
//! values are stand-ins: file ID `(inode,1,0)`, owner `[gid,uid]`, revision
//! 1, a cluster of 16 blocks (what VMS used on the volumes in fixtures)
//! that sequential files' allocations are rounded up to.

use crate::{Session, files, rms::HostBlocks, sys};
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use vms_cond::Cond;
use vms_rms::analyze::Header;
use vms_rms::{BLOCK, Blocks, Fab, Org, Rfm};

/// The cluster size the reports show.
pub const CLUSTER: u32 = 16;

/// What a relative or indexed file's prologue says.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Prologue {
    /// Prologue version: 1 relative, 3 indexed.
    pub version: u16,
    pub keys: u32,
    pub areas: u32,
    /// A relative file's maximum record number.
    pub mrn: u32,
}

/// The prologue of `path`, if it is a relative or indexed file with one.
pub fn prologue(path: &Path, fab: &Fab) -> Option<Prologue> {
    let mut b = HostBlocks::new(std::fs::File::open(path).ok()?).ok()?;
    match fab.org {
        Org::Seq => None,
        Org::Rel => {
            let p = vms_rms::rel::Prologue::read(&mut b).ok()?;
            Some(Prologue {
                version: 1,
                mrn: p.mrn,
                ..Prologue::default()
            })
        }
        Org::Idx => {
            // ponytail: VBN 1's few fields read here; vms_rms::idx's
            // prologue when DIRECTORY needs more of it.
            let mut blk = [0u8; BLOCK];
            b.read(1, &mut blk).ok()?;
            let at = |b: &[u8], o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
            // Key descriptors chain from VBN 1 offset 0: next VBN, offset.
            let (mut keys, mut cur, mut off) = (1, blk, 0usize);
            while at(&cur, off) != 0 && keys < 255 {
                let next = at(&cur, off);
                off = u16::from_le_bytes([cur[off + 4], cur[off + 5]]) as usize;
                b.read(next, &mut cur).ok()?;
                keys += 1;
            }
            Some(Prologue {
                version: u16::from_le_bytes([blk[0x74], blk[0x75]]),
                keys,
                areas: blk[0x67] as u32,
                mrn: 0,
            })
        }
    }
}

/// A sequential file's records and data bytes (its length hint), and its
/// longest record.
pub fn records(path: &Path, fab: &Fab) -> Option<(u64, u64, u16)> {
    let recs = vms_rms::decode(fab, &std::fs::read(path).ok()?).ok()?;
    let longest = recs.iter().map(|r| r.data.len()).max().unwrap_or(0) as u16;
    Some((
        recs.len() as u64,
        recs.iter()
            .map(|r| (r.control.len() + r.data.len()) as u64)
            .sum(),
        longest,
    ))
}

/// The file's record attributes, a sequential file's longest record
/// counted (RMS keeps it in the header; the host's writers don't).
pub fn fab(path: &Path) -> Fab {
    let fab = files::fab(path);
    match fab.org {
        Org::Seq => Fab {
            lrl: records(path, &fab).map_or(0, |r| r.2).max(fab.lrl),
            ..fab
        },
        _ => fab,
    }
}

/// Host permission bits as VMS protection (a set bit denies): system may
/// do anything, D goes with W, a directory can't be deleted through it.
pub fn protection(mode: u32, dir: bool) -> u16 {
    let class = |bits: u32| -> u16 {
        let mut deny = 0xF;
        if bits & 4 != 0 {
            deny &= !1;
        }
        if bits & 2 != 0 {
            deny &= !(2 | if dir { 0 } else { 8 });
        }
        if bits & 1 != 0 {
            deny &= !4;
        }
        deny
    };
    let system = if dir { 8 } else { 0 };
    system | class(mode >> 6 & 7) << 4 | class(mode >> 3 & 7) << 8 | class(mode & 7) << 12
}

/// Blocks allocated to a file: what it holds (a directory, one block), in
/// whole clusters.
pub fn allocation(path: &Path) -> u32 {
    let m = std::fs::metadata(path).ok();
    let blocks = match &m {
        Some(m) if m.is_dir() => 1,
        Some(m) => m.len().div_ceil(BLOCK as u64) as u32,
        None => 0,
    };
    blocks.div_ceil(CLUSTER) * CLUSTER
}

/// What ANALYZE/RMS_FILE reports from the file header; `spec` as shown.
pub fn header(path: &Path, spec: &str, fab: &Fab) -> Header {
    let m = std::fs::metadata(path).ok();
    let time = |t: Option<std::time::SystemTime>| t.map_or(0, sys::vms_time);
    let revised = time(m.as_ref().and_then(|m| m.modified().ok()));
    let created = m
        .as_ref()
        .and_then(|m| m.created().ok())
        .map_or(revised, sys::vms_time);
    let len = m.as_ref().map_or(0, |m| m.len());
    let allocated = allocation(path);
    let hint = match (fab.org, fab.rfm) {
        (Org::Seq, Rfm::Var | Rfm::Vfc) => records(path, fab).map(|(r, b, _)| (r, b)),
        _ => None,
    };
    Header {
        spec: spec.to_string(),
        fid: [m.as_ref().map_or(0, |m| m.ino() as u32), 1, 0],
        owner: m.as_ref().map_or(0, |m| m.gid() << 16 | m.uid() & 0xFFFF),
        protection: protection(m.as_ref().map_or(0, |m| m.mode()), false),
        created: vms_time::asctim(created, false),
        revised: vms_time::asctim(revised, false),
        revision: 1,
        expires: None,
        backup: None,
        allocated,
        cluster: CLUSTER,
        eof: len,
        contiguous: false,
        best_try_contiguous: false,
        length_hint: hint,
    }
}

/// `[g,m]`, octal.
pub fn uic(owner: u32) -> String {
    format!("[{:o},{:o}]", owner >> 16, owner & 0xFFFF)
}

/// `SYSTEM=RWED, OWNER=RWED, GROUP=RE, WORLD`, as F$FILE_ATTRIBUTES gives it.
fn pro(p: u16) -> String {
    ["SYSTEM", "OWNER", "GROUP", "WORLD"]
        .iter()
        .enumerate()
        .map(|(c, name)| {
            let can: String = "RWED"
                .chars()
                .enumerate()
                .filter(|(i, _)| p >> (4 * c + i) & 1 == 0)
                .map(|(_, ch)| ch)
                .collect();
            if can.is_empty() {
                name.to_string()
            } else {
                format!("{name}={can}")
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn tf(b: bool) -> String {
    if b { "TRUE" } else { "FALSE" }.into()
}

/// SS$_NOSUCHFILE: F$FILE_ATTRIBUTES of a file that isn't there.
pub const NOSUCHFILE: Cond = Cond(0x910);
/// CLI$_IVKEYW: an item it doesn't know.
pub const IVKEYW: Cond = Cond(0x38060);
/// DCL-E-ILLFILEVER: KNOWN of a spec with a version.
pub const ILLFILEVER: Cond = Cond(0x3898A);

/// F$FILE_ATTRIBUTES: `item` of the file `spec` names.
pub fn attribute(s: &Session, spec: &str, item: &str) -> Result<String, Cond> {
    let item = item.trim().to_ascii_uppercase();
    let parsed = s.parse(spec, "", "").map_err(|_| NOSUCHFILE)?;
    if item == "KNOWN" && parsed.version.is_some() {
        return Err(ILLFILEVER);
    }
    let (path, _) = s
        .search_all(&parsed)
        .ok()
        .and_then(|f| f.into_iter().next())
        .ok_or(NOSUCHFILE)?;
    let m = std::fs::metadata(&path).map_err(|_| NOSUCHFILE)?;
    let dir = m.is_dir();
    // A directory file, as VMS describes one.
    let fab = if dir {
        Fab {
            rfm: Rfm::Var,
            rat: vms_rms::rat::BLK,
            mrs: 512,
            lrl: 512,
            ..Fab::default()
        }
    } else {
        fab(&path)
    };
    let p = prologue(&path, &fab).unwrap_or_default();
    let len = if dir { 512 } else { m.len() };
    let seq = fab.org == Org::Seq;
    let blocks = len.div_ceil(BLOCK as u64);
    let n = |v: u64| v.to_string();
    Ok(match item.as_str() {
        "ALQ" => n(allocation(&path) as u64),
        "BKS" => n(fab.bks as u64),
        "BLS" => n(if seq { 512 } else { 0 }),
        "CTG" | "DIRECTORY" => tf(dir),
        "DEQ" => n(fab.deq as u64),
        "EOF" => n(blocks),
        "FFB" => n(if seq && !dir { len % BLOCK as u64 } else { 0 }),
        "FSZ" => n(fab.fsz as u64),
        "GBC" | "GBC32" => "0".into(),
        "GBCFLAGS" => "NONE".into(),
        "KNOWN" | "RCK" | "WCK" | "ERASE" | "NOBACKUP" | "LOCKED" | "CBT" => tf(false),
        "LRL" => n(fab.lrl as u64),
        // What VMS gave for every file.
        "MBM" => "4".into(),
        "MRN" => n(p.mrn as u64),
        "MRS" => n(fab.mrs as u64),
        "NOA" => n(p.areas as u64),
        "NOK" => n(p.keys as u64),
        "ORG" => ["SEQ", "REL", "IDX"][fab.org as usize].into(),
        "PRO" => pro(protection(m.mode(), dir)),
        "PVN" => n(if fab.org == Org::Idx {
            p.version as u64
        } else {
            0
        }),
        "RAT" => [
            (vms_rms::rat::CR, "CR"),
            (vms_rms::rat::FTN, "FTN"),
            (vms_rms::rat::PRN, "PRN"),
        ]
        .iter()
        .find(|(r, _)| fab.rat & r != 0)
        .map_or("", |(_, s)| s)
        .into(),
        "RFM" => ["UDF", "FIX", "VAR", "VFC", "STM", "STMLF", "STMCR"][fab.rfm as usize].into(),
        "UIC" => uic(m.gid() << 16 | m.uid() & 0xFFFF),
        "VERLIMIT" => "32767".into(),
        "FILE_LENGTH_HINT" => match (seq && !dir).then(|| records(&path, &fab)).flatten() {
            Some((r, b, _)) => format!("({r},{b})"),
            None => "(-1,-1)".into(),
        },
        "CDT" => vms_time::asctim(m.created().map_or(0, sys::vms_time), false),
        "RDT" => vms_time::asctim(m.modified().map_or(0, sys::vms_time), false),
        _ => return Err(IVKEYW),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protections() {
        // rw-r--r--: owner RWD (W lets it delete), group and world R.
        assert_eq!(
            pro(protection(0o644, false)),
            "SYSTEM=RWED, OWNER=RWD, GROUP=R, WORLD=R"
        );
        assert_eq!(
            pro(protection(0o750, true)),
            "SYSTEM=RWE, OWNER=RWE, GROUP=RE, WORLD"
        );
    }
}
