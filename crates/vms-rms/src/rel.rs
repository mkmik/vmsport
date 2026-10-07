//! Relative files: VBN 1 is the prologue, then buckets of `bks` blocks hold
//! one fixed-size cell per record number from 1, none crossing a bucket, no
//! bucket header (fixtures/rms/recorded/REL.DAT, fixtures/rmsrel).
//!
//! A cell is a control byte, a length word for VAR and VFC records (control
//! area and data), then room for the largest record: FSZ + MRS bytes. The
//! control byte is 0 in a cell never written, [`PRESENT`] for a record and
//! [`PRESENT`] | [`DELETED`] for a deleted one, whose bytes stay. A shorter
//! record put over a longer one leaves its tail, as RMS does. Record
//! numbers are what an [`Rfa`](crate::Rfa) of a relative file holds in `vbn`.

use crate::{BLOCK, Blocks, Design, Fab, Org, Record, Rfm, status};
use vms_cond::Cond;

/// Control byte: the cell holds a record.
pub const PRESENT: u8 = 8;
/// Control byte: ... that was deleted.
pub const DELETED: u8 = 4;

/// The highest record number when none is set (FDL MAX_RECORD_NUMBER 0).
pub const NO_MRN: u32 = 0x7fff_ffff;

/// The prologue, VBN 1 (version 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Prologue {
    /// First data bucket: 2.
    pub dvbn: u32,
    /// The highest record number allowed.
    pub mrn: u32,
    /// One past the formatted blocks; RMS formats all it allocates.
    pub eof: u32,
}

impl Prologue {
    pub fn read(b: &mut impl Blocks) -> Result<Prologue, Cond> {
        let mut blk = [0; BLOCK];
        b.read(1, &mut blk)?;
        let l = |at: usize| u32::from_le_bytes(blk[at..at + 4].try_into().unwrap());
        if blk[0x74..0x76] != [1, 0] || blk[0x1fe..] != checksum(&blk).to_le_bytes() {
            return Err(status::PLG);
        }
        Ok(Prologue {
            dvbn: l(0x68),
            mrn: l(0x6c),
            eof: l(0x70),
        })
    }

    fn write(&self, b: &mut impl Blocks) -> Result<(), Cond> {
        let mut blk = [0; BLOCK];
        blk[0x68..0x6c].copy_from_slice(&self.dvbn.to_le_bytes());
        blk[0x6c..0x70].copy_from_slice(&self.mrn.to_le_bytes());
        blk[0x70..0x74].copy_from_slice(&self.eof.to_le_bytes());
        blk[0x74] = 1;
        let sum = checksum(&blk);
        blk[0x1fe..].copy_from_slice(&sum.to_le_bytes());
        b.write(1, &blk)
    }
}

/// The sum of the words before it.
fn checksum(blk: &[u8]) -> u16 {
    blk[..0x1fe].chunks(2).fold(0, |s: u16, w| {
        s.wrapping_add(u16::from_le_bytes([w[0], w[1]]))
    })
}

/// A cell's size: control byte, length word, room for the largest record.
pub(crate) fn cell_size(fab: &Fab) -> usize {
    match fab.rfm {
        Rfm::Fix => 1 + fab.mrs as usize,
        Rfm::Vfc => 3 + fab.fsz as usize + fab.mrs as usize,
        _ => 3 + fab.mrs as usize,
    }
}

/// A relative file's geometry, from its FAB. What is in the file (its
/// prologue and cells) is read at each call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rel {
    pub fab: Fab,
    cell: usize,
    per_bucket: u32,
}

/// Makes a relative file on an empty store: its FAB (bucket size and
/// longest record filled in as RMS does) and the prologue, all of the
/// allocation formatted.
pub fn create(b: &mut impl Blocks, d: &Design) -> Result<Rel, Cond> {
    let mut fab = Fab {
        org: Org::Rel,
        lrl: d.fab.mrs,
        ..d.fab
    };
    if fab.mrs == 0 {
        return Err(status::MRS);
    }
    if fab.bks == 0 {
        fab.bks = cell_size(&fab).div_ceil(BLOCK) as u8;
    }
    let rel = Rel::new(fab)?;
    let want = d
        .areas
        .first()
        .map_or(0, |a| a.allocation)
        .max(1 + fab.bks as u32);
    if b.allocated() < want {
        b.grow(want - b.allocated())?;
    }
    Prologue {
        dvbn: 2,
        mrn: if d.max_record_number == 0 {
            NO_MRN
        } else {
            d.max_record_number
        },
        eof: b.allocated() + 1,
    }
    .write(b)?;
    Ok(rel)
}

impl Rel {
    pub fn new(fab: Fab) -> Result<Rel, Cond> {
        if fab.org != Org::Rel {
            return Err(status::ORG);
        }
        if !matches!(fab.rfm, Rfm::Fix | Rfm::Var | Rfm::Vfc) {
            return Err(status::RFM);
        }
        if fab.mrs == 0 {
            return Err(status::MRS);
        }
        let cell = cell_size(&fab);
        if cell > fab.bks as usize * BLOCK {
            return Err(status::BKS);
        }
        Ok(Rel {
            fab,
            cell,
            per_bucket: (fab.bks as usize * BLOCK / cell) as u32,
        })
    }

    /// The bucket holding record `n` and the cell's offset in it.
    fn locate(&self, p: &Prologue, n: u32) -> (u32, usize) {
        let i = n - 1;
        (
            p.dvbn + i / self.per_bucket * self.fab.bks as u32,
            (i % self.per_bucket) as usize * self.cell,
        )
    }

    /// Whether the bucket at `vbn` is inside the formatted blocks.
    fn formatted(&self, p: &Prologue, vbn: u32) -> bool {
        vbn + self.fab.bks as u32 <= p.eof
    }

    fn bucket(&self, b: &mut impl Blocks, vbn: u32) -> Result<Vec<u8>, Cond> {
        let mut buf = vec![0; self.fab.bks as usize * BLOCK];
        b.read(vbn, &mut buf)?;
        Ok(buf)
    }

    /// The prologue, with `n` checked against it.
    fn prologue(&self, b: &mut impl Blocks, n: u32) -> Result<Prologue, Cond> {
        let p = Prologue::read(b)?;
        match n {
            0 => Err(status::KEY),
            n if n > p.mrn => Err(status::MRN),
            _ => Ok(p),
        }
    }

    /// The record in a cell, if it holds one.
    fn record(&self, cell: &[u8]) -> Result<Option<Record>, Cond> {
        if cell[0] & (PRESENT | DELETED) != PRESENT {
            return Ok(None);
        }
        if self.fab.rfm == Rfm::Fix {
            return Ok(Some(Record::new(&cell[1..])));
        }
        let len = u16::from_le_bytes([cell[1], cell[2]]) as usize;
        let fsz = if self.fab.rfm == Rfm::Vfc {
            self.fab.fsz as usize
        } else {
            0
        };
        let rec = cell.get(3..3 + len).filter(|_| len >= fsz);
        let rec = rec.ok_or(status::IRC)?;
        Ok(Some(Record {
            control: rec[..fsz].to_vec(),
            data: rec[fsz..].to_vec(),
        }))
    }

    /// A record's cell bytes, up to its end.
    fn encode(&self, rec: &Record) -> Result<Vec<u8>, Cond> {
        let (mrs, fsz) = (self.fab.mrs as usize, self.fab.fsz as usize);
        let fsz = if self.fab.rfm == Rfm::Vfc { fsz } else { 0 };
        let bad = match self.fab.rfm {
            Rfm::Fix => rec.data.len() != mrs,
            _ => rec.data.len() > mrs,
        };
        if bad || rec.control.len() != fsz {
            return Err(status::RSZ);
        }
        let mut c = vec![PRESENT];
        if self.fab.rfm != Rfm::Fix {
            c.extend_from_slice(&((fsz + rec.data.len()) as u16).to_le_bytes());
        }
        c.extend_from_slice(&rec.control);
        c.extend_from_slice(&rec.data);
        Ok(c)
    }

    /// $GET (or $FIND) by record number.
    pub fn get(&self, b: &mut impl Blocks, n: u32) -> Result<Record, Cond> {
        let p = self.prologue(b, n)?;
        let (vbn, at) = self.locate(&p, n);
        if !self.formatted(&p, vbn) {
            return Err(status::RNF);
        }
        let bucket = self.bucket(b, vbn)?;
        self.record(&bucket[at..at + self.cell])?.ok_or(status::RNF)
    }

    /// Sequential $GET: the first record after record number `after` (0
    /// to start), past empty and deleted cells.
    pub fn next(&self, b: &mut impl Blocks, after: u32) -> Result<(u32, Record), Cond> {
        let p = Prologue::read(b)?;
        let mut n = after + 1;
        while n <= p.mrn {
            let (vbn, mut at) = self.locate(&p, n);
            if !self.formatted(&p, vbn) {
                break;
            }
            let bucket = self.bucket(b, vbn)?;
            while at + self.cell <= bucket.len() && n <= p.mrn {
                if let Some(r) = self.record(&bucket[at..at + self.cell])? {
                    return Ok((n, r));
                }
                at += self.cell;
                n += 1;
            }
        }
        Err(status::EOF)
    }

    /// The highest record number whose cell was ever written, deleted or
    /// not; 0 if none was.
    pub fn last(&self, b: &mut impl Blocks) -> Result<u32, Cond> {
        let p = Prologue::read(b)?;
        let buckets = p.eof.saturating_sub(p.dvbn) / self.fab.bks as u32;
        for i in (0..buckets).rev() {
            let bucket = self.bucket(b, p.dvbn + i * self.fab.bks as u32)?;
            if let Some(c) = (0..self.per_bucket)
                .rev()
                .find(|c| bucket[*c as usize * self.cell] != 0)
            {
                return Ok(i * self.per_bucket + c + 1);
            }
        }
        Ok(0)
    }

    /// $PUT of record number `n`: RMS$_REX if it holds a record. Extends
    /// the file (by at least the default extension) to reach it.
    pub fn put(&self, b: &mut impl Blocks, n: u32, rec: &Record) -> Result<(), Cond> {
        let cell = self.encode(rec)?;
        let p = self.prologue(b, n)?;
        let (vbn, at) = self.locate(&p, n);
        if !self.formatted(&p, vbn) {
            self.extend(b, p, vbn)?;
        }
        let mut bucket = self.bucket(b, vbn)?;
        if bucket[at] & (PRESENT | DELETED) == PRESENT {
            return Err(status::REX);
        }
        bucket[at..at + cell.len()].copy_from_slice(&cell);
        b.write(vbn, &bucket)
    }

    /// Sequential $PUT after OPEN/APPEND: the record goes after the last
    /// cell ever written (a deleted last record isn't reused). Returns its
    /// record number.
    pub fn append(&self, b: &mut impl Blocks, rec: &Record) -> Result<u32, Cond> {
        let n = self.last(b)? + 1;
        self.put(b, n, rec)?;
        Ok(n)
    }

    /// $UPDATE of record number `n`.
    pub fn update(&self, b: &mut impl Blocks, n: u32, rec: &Record) -> Result<(), Cond> {
        let cell = self.encode(rec)?;
        self.change(b, n, |c| c[..cell.len()].copy_from_slice(&cell))
    }

    /// $DELETE of record number `n`.
    pub fn delete(&self, b: &mut impl Blocks, n: u32) -> Result<(), Cond> {
        self.change(b, n, |c| c[0] |= DELETED)
    }

    /// Changes the cell of record `n`, which must hold one.
    fn change(&self, b: &mut impl Blocks, n: u32, f: impl FnOnce(&mut [u8])) -> Result<(), Cond> {
        let p = self.prologue(b, n)?;
        let (vbn, at) = self.locate(&p, n);
        if !self.formatted(&p, vbn) {
            return Err(status::RNF);
        }
        let mut bucket = self.bucket(b, vbn)?;
        if bucket[at] & (PRESENT | DELETED) != PRESENT {
            return Err(status::RNF);
        }
        f(&mut bucket[at..at + self.cell]);
        b.write(vbn, &bucket)
    }

    /// Formats blocks up to the bucket at `vbn`, allocating what is missing
    /// and at least the default extension.
    fn extend(&self, b: &mut impl Blocks, mut p: Prologue, vbn: u32) -> Result<(), Cond> {
        let end = vbn + self.fab.bks as u32;
        let have = b.allocated() + 1;
        // Allocated but not formatted: zero it.
        for v in p.eof..have {
            b.write(v, &[0; BLOCK])?;
        }
        if end > have {
            b.grow((end - have).max(self.fab.deq as u32))?;
        }
        p.eof = b.allocated() + 1;
        p.write(b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn var(mrs: u16) -> Design {
        Design {
            fab: Fab {
                rfm: Rfm::Var,
                mrs,
                ..Fab::default()
            },
            ..Design::default()
        }
    }

    #[test]
    fn put_get_delete() {
        let mut b = Vec::new();
        let rel = create(&mut b, &var(40)).unwrap();
        assert_eq!((rel.fab.bks, rel.fab.lrl, b.len()), (1, 40, 2 * BLOCK));
        let r = |s: &str| Record::new(s);
        assert_eq!(rel.get(&mut b, 1), Err(status::RNF));
        assert_eq!(rel.get(&mut b, 0), Err(status::KEY));
        assert_eq!(rel.next(&mut b, 0), Err(status::EOF));
        assert_eq!(rel.append(&mut b, &r("one")), Ok(1));
        // Record 30 is in the third bucket: the file grows to hold it.
        rel.put(&mut b, 30, &r("thirty")).unwrap();
        assert_eq!(b.len(), 4 * BLOCK);
        assert_eq!(Prologue::read(&mut b).unwrap().eof, 5);
        assert_eq!(rel.put(&mut b, 30, &r("again")), Err(status::REX));
        assert_eq!(rel.put(&mut b, 2, &r(&"x".repeat(41))), Err(status::RSZ));
        assert_eq!(rel.next(&mut b, 1), Ok((30, r("thirty"))));
        rel.update(&mut b, 30, &r("30")).unwrap();
        assert_eq!(rel.get(&mut b, 30), Ok(r("30")));
        rel.delete(&mut b, 30).unwrap();
        assert_eq!(rel.get(&mut b, 30), Err(status::RNF));
        assert_eq!(rel.delete(&mut b, 30), Err(status::RNF));
        assert_eq!(rel.next(&mut b, 1), Err(status::EOF));
        // A deleted cell takes a new record, but appends go after it.
        assert_eq!(rel.append(&mut b, &r("31")), Ok(31));
        rel.put(&mut b, 30, &r("new 30")).unwrap();
        assert_eq!(rel.get(&mut b, 30), Ok(r("new 30")));
    }

    #[test]
    fn mrn() {
        let mut b = Vec::new();
        let d = Design {
            max_record_number: 3,
            ..var(10)
        };
        let rel = create(&mut b, &d).unwrap();
        for _ in 0..3 {
            rel.append(&mut b, &Record::new("r")).unwrap();
        }
        assert_eq!(rel.append(&mut b, &Record::new("r")), Err(status::MRN));
        assert_eq!(rel.next(&mut b, 3), Err(status::EOF));
    }
}
