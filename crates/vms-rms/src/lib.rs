//! RMS record attributes and sequential record formats.
//!
//! Pure: bytes in, records out, and back, laid out as VMS lays them on disk
//! (see fixtures/rms and fixtures/rmsblk). [`Text`] shows records the way a
//! terminal shows them after TYPE, as Unix text.

use std::fmt;

/// The extended attribute a host file keeps its [`Fab`] in.
pub const XATTR: &str = "vms.fab";

/// Disk block size: records of files with [`rat::BLK`] don't cross it.
pub const BLOCK: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Org {
    Seq,
    Rel,
    Idx,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rfm {
    Udf,
    Fix,
    Var,
    Vfc,
    Stm,
    Stmlf,
    Stmcr,
}

/// Record attribute bits (`FAB$V_FTN` ...).
pub mod rat {
    pub const FTN: u8 = 1;
    pub const CR: u8 = 2;
    pub const PRN: u8 = 4;
    pub const BLK: u8 = 8;
}

/// What RMS knows about a file's records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fab {
    pub org: Org,
    pub rfm: Rfm,
    pub rat: u8,
    /// Maximum record size; 0 is no limit. FIX records are this long.
    pub mrs: u16,
    /// Longest record in the file, as RMS keeps it.
    pub lrl: u16,
    /// VFC control area size.
    pub fsz: u8,
    /// Bucket size, for relative and indexed files.
    pub bks: u8,
}

/// No attribute: a plain Unix text file.
impl Default for Fab {
    fn default() -> Fab {
        Fab {
            org: Org::Seq,
            rfm: Rfm::Stmlf,
            rat: rat::CR,
            mrs: 0,
            lrl: 0,
            fsz: 0,
            bks: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub &'static str);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for Error {}

const ORGS: [(Org, &str); 3] = [(Org::Seq, "seq"), (Org::Rel, "rel"), (Org::Idx, "idx")];
const RFMS: [(Rfm, &str); 7] = [
    (Rfm::Udf, "udf"),
    (Rfm::Fix, "fix"),
    (Rfm::Var, "var"),
    (Rfm::Vfc, "vfc"),
    (Rfm::Stm, "stm"),
    (Rfm::Stmlf, "stmlf"),
    (Rfm::Stmcr, "stmcr"),
];
const RATS: [(u8, &str); 4] = [
    (rat::FTN, "ftn"),
    (rat::CR, "cr"),
    (rat::PRN, "prn"),
    (rat::BLK, "blk"),
];

/// The `vms.fab` text: `org=seq rfm=var rat=cr,blk mrs=0 lrl=21 fsz=0 bks=0`.
impl fmt::Display for Fab {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let org = ORGS.iter().find(|o| o.0 == self.org).unwrap().1;
        let rfm = RFMS.iter().find(|r| r.0 == self.rfm).unwrap().1;
        let rats: Vec<&str> = RATS
            .iter()
            .filter(|r| self.rat & r.0 != 0)
            .map(|r| r.1)
            .collect();
        let rats = if rats.is_empty() {
            "none".to_string()
        } else {
            rats.join(",")
        };
        write!(
            f,
            "org={org} rfm={rfm} rat={rats} mrs={} lrl={} fsz={} bks={}",
            self.mrs, self.lrl, self.fsz, self.bks
        )
    }
}

/// Reads the `vms.fab` text. Missing keys keep their defaults and unknown
/// keys are ignored, so newer files still read.
impl std::str::FromStr for Fab {
    type Err = Error;

    fn from_str(s: &str) -> Result<Fab, Error> {
        let mut fab = Fab::default();
        for kv in s.split_whitespace() {
            let (k, v) = kv.split_once('=').ok_or(Error("invalid vms.fab"))?;
            let num = || v.parse().map_err(|_| Error("invalid vms.fab number"));
            match k {
                "org" => {
                    fab.org = ORGS
                        .iter()
                        .find(|o| o.1 == v)
                        .ok_or(Error("invalid org"))?
                        .0
                }
                "rfm" => {
                    fab.rfm = RFMS
                        .iter()
                        .find(|r| r.1 == v)
                        .ok_or(Error("invalid rfm"))?
                        .0
                }
                "rat" => {
                    fab.rat = 0;
                    for r in v.split(',').filter(|r| *r != "none") {
                        fab.rat |= RATS
                            .iter()
                            .find(|x| x.1 == r)
                            .ok_or(Error("invalid rat"))?
                            .0;
                    }
                }
                "mrs" => fab.mrs = num()?,
                "lrl" => fab.lrl = num()?,
                "fsz" => fab.fsz = num()? as u8,
                "bks" => fab.bks = num()? as u8,
                _ => {}
            }
        }
        Ok(fab)
    }
}

/// A record: the VFC control area (empty for other formats) and the data.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Record {
    pub control: Vec<u8>,
    pub data: Vec<u8>,
}

impl Record {
    pub fn new(data: impl Into<Vec<u8>>) -> Record {
        Record {
            control: Vec::new(),
            data: data.into(),
        }
    }
}

fn blk(fab: &Fab) -> bool {
    fab.rat & rat::BLK != 0
}

/// Bytes a record takes in the file, without block filling.
fn size(fab: &Fab, rec: &Record) -> usize {
    let n = rec.control.len() + rec.data.len();
    match fab.rfm {
        Rfm::Var | Rfm::Vfc => 2 + n + n % 2,
        Rfm::Fix => n + n % 2,
        Rfm::Stm => n + 2,
        Rfm::Stmlf | Rfm::Stmcr => n + 1,
        Rfm::Udf => n,
    }
}

/// Splits a sequential file's bytes (up to its end of file) into records.
pub fn decode(fab: &Fab, bytes: &[u8]) -> Result<Vec<Record>, Error> {
    let mut out = Vec::new();
    match fab.rfm {
        Rfm::Udf => {
            if !bytes.is_empty() {
                out.push(Record::new(bytes));
            }
        }
        Rfm::Stmlf | Rfm::Stmcr | Rfm::Stm => {
            let term = if fab.rfm == Rfm::Stmcr { b'\r' } else { b'\n' };
            let mut lines: Vec<&[u8]> = bytes.split(|&b| b == term).collect();
            if lines.last().is_some_and(|l| l.is_empty()) {
                lines.pop(); // the terminator of the last record
            }
            for l in lines {
                // ponytail: STM ends records at CR LF or LF; FF and VT, which
                // RMS also takes as terminators, stay in the data.
                let l = if fab.rfm == Rfm::Stm {
                    l.strip_suffix(b"\r").unwrap_or(l)
                } else {
                    l
                };
                out.push(Record::new(l));
            }
        }
        Rfm::Fix => {
            let n = fab.mrs as usize;
            if n == 0 {
                return Err(Error("fixed-length records with no size"));
            }
            let step = n + n % 2;
            let mut i = 0;
            while i < bytes.len() {
                if blk(fab) && i % BLOCK + step > BLOCK {
                    i = i.next_multiple_of(BLOCK);
                    continue;
                }
                let rec = bytes.get(i..i + n).ok_or(Error("truncated record"))?;
                out.push(Record::new(rec));
                i += step;
            }
        }
        Rfm::Var | Rfm::Vfc => {
            let fsz = if fab.rfm == Rfm::Vfc {
                fab.fsz as usize
            } else {
                0
            };
            let mut i = 0;
            while i < bytes.len() {
                if blk(fab) && BLOCK - i % BLOCK < 2 {
                    i = i.next_multiple_of(BLOCK);
                    continue;
                }
                let len = u16::from_le_bytes([
                    bytes[i],
                    *bytes.get(i + 1).ok_or(Error("truncated record"))?,
                ]);
                if len == 0xffff {
                    // Nothing more in this block.
                    i = (i + 1).next_multiple_of(BLOCK);
                    continue;
                }
                let len = len as usize;
                let rec = bytes
                    .get(i + 2..i + 2 + len)
                    .ok_or(Error("truncated record"))?;
                if len < fsz {
                    return Err(Error("record shorter than its control area"));
                }
                out.push(Record {
                    control: rec[..fsz].to_vec(),
                    data: rec[fsz..].to_vec(),
                });
                i += 2 + len + len % 2;
            }
        }
    }
    Ok(out)
}

/// The bytes of one record written at file offset `offset` (which matters
/// only with [`rat::BLK`]: a record that doesn't fit in what is left of the
/// block starts the next one).
pub fn encode_record(fab: &Fab, offset: usize, rec: &Record) -> Result<Vec<u8>, Error> {
    let fsz = if fab.rfm == Rfm::Vfc {
        fab.fsz as usize
    } else {
        0
    };
    if rec.control.len() != fsz {
        return Err(Error("control area size differs from the file's"));
    }
    let n = rec.data.len() + fsz;
    if fab.rfm == Rfm::Fix && n != fab.mrs as usize {
        return Err(Error("fixed-length record of the wrong size"));
    }
    if (fab.mrs > 0 && rec.data.len() > fab.mrs as usize) || n > 32767 {
        return Err(Error("record too long"));
    }
    let mut out = Vec::new();
    let need = size(fab, rec);
    if blk(fab) && matches!(fab.rfm, Rfm::Fix | Rfm::Var | Rfm::Vfc) {
        if need > BLOCK {
            return Err(Error("record too long for a block"));
        }
        let left = BLOCK - offset % BLOCK;
        if need > left {
            // VAR and VFC mark the rest of the block unused with -1.
            if fab.rfm != Rfm::Fix && left >= 2 {
                out.extend_from_slice(&[0xff, 0xff]);
            }
            out.resize(left, 0);
        }
    }
    match fab.rfm {
        Rfm::Var | Rfm::Vfc => out.extend_from_slice(&(n as u16).to_le_bytes()),
        _ => {}
    }
    out.extend_from_slice(&rec.control);
    out.extend_from_slice(&rec.data);
    match fab.rfm {
        Rfm::Var | Rfm::Vfc | Rfm::Fix if n % 2 == 1 => out.push(0),
        Rfm::Stm => out.extend_from_slice(b"\r\n"),
        Rfm::Stmlf => out.push(b'\n'),
        Rfm::Stmcr => out.push(b'\r'),
        _ => {}
    }
    Ok(out)
}

/// A whole file's bytes.
pub fn encode(fab: &Fab, records: &[Record]) -> Result<Vec<u8>, Error> {
    let mut out = Vec::new();
    for r in records {
        let b = encode_record(fab, out.len(), r)?;
        out.extend(b);
    }
    Ok(out)
}

/// Shows records as Unix text, the way a VMS terminal shows them after
/// TYPE: carriage control (CR, FORTRAN, print/VFC) becomes line ends, blank
/// lines, form feeds and overprinting. VMS moves to a new line *before* a
/// record and Unix ends lines *after*; the first new line belongs to the
/// prompt, and [`finish`](Self::finish) ends the last line.
#[derive(Debug, Default)]
pub struct Text {
    started: bool,
    /// Characters on the current line, not ended yet.
    open: bool,
    /// A carriage return that only shows if more text follows on the line.
    cr: bool,
}

/// What a terminal does for one carriage-control step.
enum Act {
    Lf,
    Cr,
    Char(u8),
}

/// Print-file (PRN) control byte: new lines, or one control character.
fn prn(b: u8, acts: &mut Vec<Act>) {
    match b {
        0 => {}
        1..=0x7f => (0..b).for_each(|_| acts.extend([Act::Cr, Act::Lf])),
        0x80..=0x9f => acts.push(match b & 0x1f {
            b'\n' => Act::Lf,
            b'\r' => Act::Cr,
            c => Act::Char(c),
        }),
        0xa0..=0xbf => acts.push(Act::Char(b - 0x20)), // C1 control: 0x80-0x9f
        _ => {}                                        // printer VFU channels
    }
}

impl Text {
    pub fn new() -> Text {
        Text::default()
    }

    fn act(&mut self, a: &Act, out: &mut Vec<u8>) {
        match a {
            Act::Lf => {
                if self.started || self.open {
                    out.push(b'\n');
                }
                self.open = false;
            }
            Act::Cr => {
                self.cr = true;
                return;
            }
            Act::Char(c) => {
                // A form feed starts a new page, at the start of a line.
                if *c == 0x0c && self.open {
                    out.push(b'\n');
                    self.open = false;
                }
                out.push(*c);
            }
        }
        self.cr = false;
        self.started = true;
    }

    fn data(&mut self, d: &[u8], out: &mut Vec<u8>) {
        if d.is_empty() {
            return;
        }
        if self.cr && self.open {
            out.push(b'\r');
        }
        out.extend_from_slice(d);
        self.cr = false;
        self.open = true;
        self.started = true;
    }

    /// The text for one record.
    pub fn record(&mut self, fab: &Fab, rec: &Record) -> Vec<u8> {
        let (mut before, mut after) = (Vec::new(), Vec::new());
        let mut data = &rec.data[..];
        if fab.rat & rat::FTN != 0 {
            let (first, rest) = data.split_first().map_or((b' ', &[][..]), |(f, r)| (*f, r));
            data = rest;
            match first {
                b'0' => before.extend([Act::Lf, Act::Lf]),
                b'1' => before.push(Act::Char(0x0c)),
                b'+' => {}
                b'$' => before.push(Act::Lf),
                0 => {}
                _ => before.push(Act::Lf),
            }
            if !matches!(first, b'$' | 0) {
                after.push(Act::Cr);
            }
        } else if fab.rat & rat::PRN != 0 && fab.rfm == Rfm::Vfc && rec.control.len() >= 2 {
            prn(rec.control[0], &mut before);
            prn(rec.control[1], &mut after);
        } else {
            // CR, and no carriage control: TYPE shows a line per record.
            before.push(Act::Lf);
            after.push(Act::Cr);
        }
        let mut out = Vec::new();
        before.iter().for_each(|a| self.act(a, &mut out));
        self.data(data, &mut out);
        after.iter().for_each(|a| self.act(a, &mut out));
        out
    }

    /// Ends the last line.
    pub fn finish(&mut self) -> Vec<u8> {
        if self.open {
            self.open = false;
            vec![b'\n']
        } else {
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(fab: &Fab, recs: &[Record]) -> String {
        let mut t = Text::new();
        let mut out: Vec<u8> = recs.iter().flat_map(|r| t.record(fab, r)).collect();
        out.extend(t.finish());
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn xattr_round_trip() {
        let fab = Fab {
            rfm: Rfm::Vfc,
            rat: rat::PRN | rat::BLK,
            fsz: 2,
            lrl: 21,
            ..Fab::default()
        };
        assert_eq!(
            fab.to_string(),
            "org=seq rfm=vfc rat=prn,blk mrs=0 lrl=21 fsz=2 bks=0"
        );
        assert_eq!(fab.to_string().parse::<Fab>().unwrap(), fab);
        assert_eq!(
            "rfm=fix mrs=13 rat=none future=1"
                .parse::<Fab>()
                .unwrap()
                .rat,
            0
        );
        assert!("rfm=bogus".parse::<Fab>().is_err());
    }

    #[test]
    fn streams() {
        let lf = Fab::default();
        let recs = decode(&lf, b"a\n\nb").unwrap();
        assert_eq!(recs, [Record::new("a"), Record::new(""), Record::new("b")]);
        assert_eq!(encode(&lf, &recs).unwrap(), b"a\n\nb\n");
        let stm = Fab {
            rfm: Rfm::Stm,
            ..lf
        };
        assert_eq!(
            decode(&stm, b"a\r\nb\n").unwrap(),
            [Record::new("a"), Record::new("b")]
        );
        assert_eq!(decode(&lf, b"").unwrap(), []);
    }

    #[test]
    fn errors() {
        let fix = Fab {
            rfm: Rfm::Fix,
            mrs: 4,
            ..Fab::default()
        };
        assert!(encode_record(&fix, 0, &Record::new("abc")).is_err());
        let var = Fab {
            rfm: Rfm::Var,
            ..Fab::default()
        };
        assert!(decode(&var, &[5, 0, b'a']).is_err());
    }

    #[test]
    fn fortran_and_print_control() {
        let ftn = Fab {
            rfm: Rfm::Var,
            rat: rat::FTN,
            ..Fab::default()
        };
        let recs: Vec<Record> = [" one", "0two", "1three", "+over", "$ask", " end"]
            .map(Record::new)
            .into();
        assert_eq!(text(&ftn, &recs), "one\n\ntwo\n\x0cthree\rover\nask\nend\n");
        let prn = Fab {
            rfm: Rfm::Vfc,
            rat: rat::PRN,
            fsz: 2,
            ..Fab::default()
        };
        let r = |c: [u8; 2], d: &str| Record {
            control: c.to_vec(),
            data: d.into(),
        };
        // One new line before and CR after is a plain line; three before
        // leave two blank lines; none before prints over the line.
        assert_eq!(
            text(
                &prn,
                &[r([1, 0x8d], "a"), r([3, 0x8d], "b"), r([0, 0x8c], "c")]
            ),
            "a\n\n\nb\rc\n\x0c"
        );
    }
}
