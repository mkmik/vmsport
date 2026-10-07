//! What SORT, MERGE and CONVERT share: keys as /KEY gives them, comparing
//! records by them, and the order VMS's sort leaves records with equal keys
//! in (fixtures/sort).

use std::cmp::Ordering;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Typ {
    Character,
    Binary {
        signed: bool,
    },
    /// Zoned decimal: one digit a byte, the sign overpunched on the last
    /// digit, or (leading, separate) where the qualifiers put it.
    Decimal {
        leading: bool,
        separate: bool,
    },
    Packed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Key {
    /// Byte offset in the record (POSITION less one).
    pub pos: usize,
    /// SIZE: bytes, or digits for decimal keys.
    pub size: usize,
    pub typ: Typ,
    pub descending: bool,
}

/// A key's value in one record.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Value {
    Bytes(Vec<u8>),
    Number(i128),
}

impl Key {
    /// The bytes the key spans.
    pub fn bytes(&self) -> usize {
        match self.typ {
            Typ::Decimal { separate: true, .. } => self.size + 1,
            Typ::Packed => self.size / 2 + 1,
            _ => self.size,
        }
    }

    /// The key's value; `None` for a numeric key the record is too short
    /// to hold (VMS drops the record: SORT-W-BAD_SRL). A character key
    /// past the end of the record is padded with NULs.
    pub fn value(&self, rec: &[u8]) -> Option<Value> {
        let end = self.pos + self.bytes();
        if self.typ == Typ::Character {
            let mut v = rec
                .get(self.pos..end.min(rec.len()))
                .unwrap_or(&[])
                .to_vec();
            v.resize(self.bytes(), 0);
            return Some(Value::Bytes(v));
        }
        let b = rec.get(self.pos..end)?;
        let digit = |c: u8| match c {
            b'0'..=b'9' => (c - b'0') as i128,
            // An overpunched digit.
            0x70..=0x79 => (c & 15) as i128,
            _ => 0,
        };
        let n = match self.typ {
            Typ::Binary { signed } => {
                let mut v = [0u8; 16];
                v[..b.len()].copy_from_slice(b);
                let fill = if signed && b.last().is_some_and(|x| x & 0x80 != 0) {
                    0xFF
                } else {
                    0
                };
                v[b.len()..].fill(fill);
                i128::from_le_bytes(v)
            }
            Typ::Decimal { leading, separate } => {
                let (digits, sign) = match (leading, separate) {
                    (true, true) => (&b[1..], b[0]),
                    (false, true) => (&b[..b.len() - 1], b[b.len() - 1]),
                    (true, false) => (b, b[0]),
                    (false, false) => (b, b[b.len() - 1]),
                };
                let v = digits.iter().fold(0, |v, c| v * 10 + digit(*c));
                let negative = if separate {
                    sign == b'-'
                } else {
                    sign & 0xF0 == 0x70
                };
                if negative { -v } else { v }
            }
            Typ::Packed => {
                let mut v = 0i128;
                for (i, c) in b.iter().enumerate() {
                    v = v * 10 + (c >> 4) as i128;
                    if i + 1 < b.len() {
                        v = v * 10 + (c & 15) as i128;
                    }
                }
                if b[b.len() - 1] & 15 == 0xD { -v } else { v }
            }
            Typ::Character => unreachable!(),
        };
        Some(Value::Number(n))
    }
}

/// Records' key values, in key order; no keys is the whole record.
pub fn values(keys: &[Key], rec: &[u8]) -> Option<Vec<Value>> {
    if keys.is_empty() {
        return Some(vec![Value::Bytes(rec.to_vec())]);
    }
    keys.iter().map(|k| k.value(rec)).collect()
}

/// Compares two records' key values.
pub fn compare(keys: &[Key], a: &[Value], b: &[Value]) -> Ordering {
    for (i, (x, y)) in a.iter().zip(b).enumerate() {
        let o = x.cmp(y);
        let o = if keys.get(i).is_some_and(|k| k.descending) {
            o.reverse()
        } else {
            o
        };
        if o != Ordering::Equal {
            return o;
        }
    }
    Ordering::Equal
}

/// The order VMS's sort puts `n` records in: replacement selection
/// (Knuth's Algorithm R, TAOCP 5.4.1) over a tree of `p` nodes, which
/// leaves records with equal keys in an order of its own (/STABLE asks for
/// the input's). With `n` past `p` it would make several runs; the tree is
/// grown to hold them all instead (ponytail: VMS merges runs from work
/// files there, so equal keys may come out in another order).
pub fn select(n: usize, p: usize, cmp: impl Fn(usize, usize) -> Ordering) -> Vec<usize> {
    let p = p.max(n).max(2);
    let mut rec: Vec<Option<usize>> = vec![None; p];
    let mut rn = vec![0u32; p];
    let mut loser: Vec<usize> = (0..p).collect();
    let (mut rmax, mut rc, mut q, mut rq) = (0u32, 0u32, 0usize, 0u32);
    let mut last: Option<usize> = None;
    let mut next = 0;
    let mut out = Vec::with_capacity(n);
    loop {
        // R2: the end of a run, or of everything.
        if rq != rc {
            if rq > rmax {
                return out;
            }
            rc = rq;
        }
        // R3: out with the winner.
        if rq != 0 {
            let r = rec[q].unwrap();
            out.push(r);
            last = Some(r);
        }
        // R4: in with the next record, into the winner's place.
        if next < n {
            rec[q] = Some(next);
            if last.is_none_or(|l| cmp(next, l) == Ordering::Less) {
                rq += 1;
                rmax = rmax.max(rq);
            }
            next += 1;
        } else {
            rq = rmax + 1;
        }
        // R5-R7: up the tree, leaving the loser of each match.
        let mut t = (p + q) / 2;
        loop {
            let l = loser[t];
            let wins = rn[l] < rq
                || rn[l] == rq
                    && matches!((rec[l], rec[q]), (Some(a), Some(b)) if cmp(a, b) == Ordering::Less);
            if wins {
                loser[t] = q;
                rn[q] = rq;
                rq = rn[l];
                q = l;
            }
            if t == 1 {
                break;
            }
            t /= 2;
        }
    }
}

/// The tree VMS's sort builds for an input of `blocks` blocks of records
/// at most `lrl` long.
///
/// ponytail: VMS sizes it from its memory plan, which isn't known here;
/// this matches what it chose for one-block files and some larger ones
/// (fixtures/sort/recorded/STATS.log), so /STABLE is the way to VMS's
/// order of equal keys in general.
pub fn tree_size(blocks: u64, lrl: usize) -> usize {
    2 * (blocks.max(1) as usize * 512).div_ceil(lrl + 2) + 2
}

#[cfg(test)]
mod tests {
    use super::*;

    /// VMS's order of ten records whose keys are all equal but the last
    /// (fixtures/sort: SORT/KEY=(POSITION:30,SIZE:4) IN.TXT).
    #[test]
    fn equal_keys_as_vms() {
        let keys = [0, 0, 0, 0, 0, 0, 0, 0, 0, 1];
        let out = select(10, 34, |a, b| keys[a].cmp(&keys[b]));
        assert_eq!(out, [0, 4, 6, 5, 8, 7, 1, 3, 2, 9]);
        assert_eq!(tree_size(1, 32), 34);
    }

    #[test]
    fn key_values() {
        let k = |typ, pos, size| Key {
            pos,
            size,
            typ,
            descending: false,
        };
        let dec = Typ::Decimal {
            leading: false,
            separate: false,
        };
        assert_eq!(k(dec, 0, 5).value(b"-0045"), Some(Value::Number(45)));
        assert_eq!(k(dec, 0, 2).value(b"1u"), Some(Value::Number(-15)));
        assert_eq!(
            k(Typ::Binary { signed: true }, 0, 2).value(b"\xFF\xFF"),
            Some(Value::Number(-1))
        );
        assert_eq!(
            k(Typ::Binary { signed: false }, 0, 2).value(b"\xFF\xFF"),
            Some(Value::Number(65535))
        );
        assert_eq!(
            k(Typ::Character, 1, 4).value(b"ab"),
            Some(Value::Bytes(b"b\0\0\0".to_vec()))
        );
        assert_eq!(k(dec, 1, 4).value(b"ab"), None);
        let sep = Typ::Decimal {
            leading: true,
            separate: true,
        };
        assert_eq!(k(sep, 0, 3).value(b"-012"), Some(Value::Number(-12)));
        assert_eq!(
            k(Typ::Packed, 0, 3).value(b"\x12\x3D"),
            Some(Value::Number(-123))
        );
    }
}
