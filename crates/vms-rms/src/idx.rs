//! Indexed files, prologue 3, on a [`Blocks`] store, laid out byte for
//! byte as RMS lays them (fixtures/rms, fixtures/idx, fixtures/idxw).
//!
//! The prologue (VBN 1 on) holds a descriptor per key and per area. Each
//! key has a tree of index buckets over a chain of data buckets: for the
//! primary key the records themselves, for the others SIDRs (a key value
//! and the RFAs of the records that have it). A record's RFA is where it
//! was put; when a split moves it, an RRV left there points to where it
//! went. Duplicates that outgrow a bucket go on in continuation buckets,
//! which the index does not point to. Every call reads what it needs and
//! keeps nothing, so the host can share a file by locking around calls.

use crate::{Blocks, Design, KeyDesc, KeyType, Rfa, Segment, status};
use std::cmp::Ordering;
use vms_cond::Cond;

pub mod analyze;

const BLK: usize = 512;

/// Bucket header (BKT$): check byte, key of reference, VBN sample, free
/// space, next record ID, next bucket, level, flags; then the records. The
/// last byte repeats the check byte, which counts the bucket's writes.
const HDR: usize = 14;
const LASTBKT: u8 = 1;
const ROOTBKT: u8 = 2;
/// Index buckets end with the offset of the last free byte below the
/// pointers, a spare byte and the check byte.
const IDX_TAIL: usize = 4;

/// Record control bits (IRC$): pointer size - 2, deleted, an RRV, no
/// pointer (a deleted SIDR element cut to this byte), the first element of
/// a key value's SIDRs.
const DELETED: u8 = 4;
const RRV: u8 = 8;
const NOPTR: u8 = 0x10;
const FIRST: u8 = 0x80;

/// Key descriptor flags (KEY$).
const DUPKEYS: u8 = 1;
const CHGKEYS: u8 = 2;
const NULKEYS: u8 = 4;
const IDX_COMPR: u8 = 8;
const INITIDX: u8 = 0x10;
const KEY_COMPR: u8 = 0x40;
const REC_COMPR: u8 = 0x80;

/// Key descriptors are this long, five to a block after VBN 1's. The
/// fixed prologue is in VBN 1 after key 0's; area descriptors follow the
/// key blocks, eight to a block.
const KEY_SIZE: usize = 0x66;
const KEYS_PER_BLOCK: usize = 5;
const PLG_AVBN: usize = 0x66;
const PLG_AMAX: usize = 0x67;
const PLG_VER: usize = 0x74;
const AREA_SIZE: usize = 64;

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

fn put16(b: &mut [u8], at: usize, v: u16) {
    b[at..at + 2].copy_from_slice(&v.to_le_bytes());
}

fn put32(b: &mut [u8], at: usize, v: u32) {
    b[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

/// A little-endian number of `b.len()` bytes (pointers are 2 to 4).
fn uint(b: &[u8]) -> u32 {
    b.iter().rev().fold(0, |v, &x| v << 8 | x as u32)
}

/// The bytes a pointer to `vbn` takes.
fn ptr_size(vbn: u32) -> usize {
    match vbn {
        0..=0xffff => 2,
        0x1_0000..=0xff_ffff => 3,
        _ => 4,
    }
}

/// A key as the prologue describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Key {
    pub desc: KeyDesc,
    /// No record has had the key yet: no buckets.
    pub empty: bool,
    pub root: u32,
    pub root_level: u8,
    pub index_bucket: u8,
    pub data_bucket: u8,
    /// The first data bucket.
    pub first: u32,
    /// The shortest record that has the key.
    pub min_size: u16,
    /// Where the descriptor is: VBN and offset.
    at: (u32, usize),
}

impl Key {
    fn len(&self) -> usize {
        self.desc.length()
    }

    /// Whether `record` has the key: long enough, and not the null value.
    fn in_record(&self, record: &[u8]) -> bool {
        record.len() >= self.min_size as usize
            && !(self.desc.null_key
                && self
                    .desc
                    .extract(record)
                    .iter()
                    .all(|&b| b == self.desc.null_value))
    }
}

/// An area as the prologue describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AreaDesc {
    pub bucket_size: u8,
    /// First bucket of the reclaimed list.
    pub reclaimed: u32,
    /// The current extent: first VBN, blocks, blocks used, next free VBN.
    pub start: u32,
    pub blocks: u32,
    pub used: u32,
    pub next: u32,
    /// An extent waiting to be used: first VBN and blocks.
    pub next_start: u32,
    pub next_blocks: u32,
    pub extension: u16,
    pub total: u32,
    at: (u32, usize),
}

/// The prologue: its blocks as read, and what they say.
#[derive(Debug, Clone)]
pub struct Prologue {
    pub keys: Vec<Key>,
    pub areas: Vec<AreaDesc>,
    /// VBN, bytes, changed.
    blocks: Vec<(u32, Vec<u8>, bool)>,
}

impl Prologue {
    fn block(&mut self, vbn: u32) -> &mut Vec<u8> {
        let b = self.blocks.iter_mut().find(|b| b.0 == vbn).unwrap();
        b.2 = true;
        &mut b.1
    }

    /// Puts key `i`'s tree back in its descriptor.
    fn store_key(&mut self, i: usize) {
        let k = self.keys[i].clone();
        let b = &mut self.block(k.at.0)[k.at.1..k.at.1 + KEY_SIZE];
        b[0x09] = k.root_level;
        put32(b, 0x0c, k.root);
        put32(b, 0x54, k.first);
        b[0x10] = b[0x10] & !INITIDX | if k.empty { INITIDX } else { 0 };
    }

    fn store_area(&mut self, i: usize) {
        let a = self.areas[i].clone();
        let b = &mut self.block(a.at.0)[a.at.1..a.at.1 + AREA_SIZE];
        put32(b, 0x08, a.reclaimed);
        put32(b, 0x0c, a.start);
        put32(b, 0x10, a.blocks);
        put32(b, 0x14, a.used);
        put32(b, 0x18, a.next);
        put32(b, 0x1c, a.next_start);
        put32(b, 0x20, a.next_blocks);
        put32(b, 0x32, a.total);
    }
}

fn key_type(t: u8) -> (KeyType, bool) {
    let typ = match t & 0x1f {
        1 => KeyType::Int2,
        2 => KeyType::Bin2,
        3 => KeyType::Int4,
        4 => KeyType::Bin4,
        5 => KeyType::Decimal,
        6 => KeyType::Int8,
        7 => KeyType::Bin8,
        8 => KeyType::Collated,
        _ => KeyType::String,
    };
    (typ, t & 0x20 != 0)
}

fn type_code(typ: KeyType, descending: bool) -> u8 {
    let t = match typ {
        KeyType::String => 0,
        KeyType::Int2 => 1,
        KeyType::Bin2 => 2,
        KeyType::Int4 => 3,
        KeyType::Bin4 => 4,
        KeyType::Decimal => 5,
        KeyType::Int8 => 6,
        KeyType::Bin8 => 7,
        KeyType::Collated => 8,
    };
    t | if descending { 0x20 } else { 0 }
}

fn parse_key(b: &[u8], at: (u32, usize)) -> Key {
    let flags = b[0x10];
    let (typ, descending) = key_type(b[0x11]);
    let segments = (0..b[0x12] as usize)
        .map(|i| Segment {
            position: u16_at(b, 0x1c + 2 * i),
            length: b[0x2c + i] as u16,
        })
        .collect();
    let name = String::from_utf8_lossy(&b[0x34..0x54])
        .trim_end_matches([' ', '\0'])
        .to_string();
    Key {
        desc: KeyDesc {
            number: b[0x15],
            name,
            typ,
            descending,
            segments,
            duplicates: flags & DUPKEYS != 0,
            changes: flags & CHGKEYS != 0,
            null_key: flags & NULKEYS != 0,
            null_value: b[0x13],
            data_area: b[0x08],
            index_area: b[0x06],
            level1_index_area: b[0x07],
            data_fill: u16_at(b, 0x1a),
            index_fill: u16_at(b, 0x18),
            data_key_compression: flags & KEY_COMPR != 0,
            data_record_compression: flags & REC_COMPR != 0,
            index_compression: flags & IDX_COMPR != 0,
        },
        empty: flags & INITIDX != 0,
        root: u32_at(b, 0x0c),
        root_level: b[0x09],
        index_bucket: b[0x0a],
        data_bucket: b[0x0b],
        first: u32_at(b, 0x54),
        min_size: u16_at(b, 0x16),
        at,
    }
}

fn parse_area(b: &[u8], at: (u32, usize)) -> AreaDesc {
    AreaDesc {
        bucket_size: b[3],
        reclaimed: u32_at(b, 0x08),
        start: u32_at(b, 0x0c),
        blocks: u32_at(b, 0x10),
        used: u32_at(b, 0x14),
        next: u32_at(b, 0x18),
        next_start: u32_at(b, 0x1c),
        next_blocks: u32_at(b, 0x20),
        extension: u16_at(b, 0x24),
        total: u32_at(b, 0x32),
        at,
    }
}

/// A prologue block's check: the sum of its other words.
fn checksum(b: &[u8]) -> u16 {
    (0..255).fold(0u16, |s, i| s.wrapping_add(u16_at(b, 2 * i)))
}

/// How a keyed lookup matches the key it is given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Match {
    Eq,
    Ge,
    Gt,
}

/// Compares key values `a` and `b` of key `k` in its order; a shorter `b`
/// is a generic (leading part) string key, compared on its length.
pub fn compare(k: &KeyDesc, a: &[u8], b: &[u8]) -> Ordering {
    let n = b.len().min(a.len());
    let (a, b) = (&a[..n], &b[..n]);
    let o = match k.typ {
        KeyType::String | KeyType::Collated => a.cmp(b),
        KeyType::Int2 | KeyType::Int4 | KeyType::Int8 => int(a).cmp(&int(b)),
        KeyType::Bin2 | KeyType::Bin4 | KeyType::Bin8 => uint64(a).cmp(&uint64(b)),
        KeyType::Decimal => packed(a).cmp(&packed(b)),
    };
    if k.descending { o.reverse() } else { o }
}

fn uint64(b: &[u8]) -> u64 {
    b.iter().rev().fold(0, |v, &x| v << 8 | x as u64)
}

fn int(b: &[u8]) -> i64 {
    let bits = 64 - 8 * b.len() as u32;
    ((uint64(b) << bits) as i64) >> bits
}

/// A packed decimal: a digit a nibble, the sign in the last (B, D minus).
fn packed(b: &[u8]) -> i128 {
    let mut v: i128 = 0;
    for (i, &x) in b.iter().enumerate() {
        v = v * 10 + (x >> 4) as i128;
        if i + 1 < b.len() {
            v = v * 10 + (x & 15) as i128;
        } else if matches!(x & 15, 0xb | 0xd) {
            v = -v;
        }
    }
    v
}

/// The value after every other in key `k`'s order: the last index entry's.
fn high_key(k: &KeyDesc) -> Vec<u8> {
    let n = k.length();
    let mut v = match (k.typ, k.descending) {
        (
            KeyType::String | KeyType::Collated | KeyType::Bin2 | KeyType::Bin4 | KeyType::Bin8,
            d,
        ) => {
            vec![if d { 0 } else { 0xff }; n]
        }
        (KeyType::Int2 | KeyType::Int4 | KeyType::Int8, d) => {
            let mut v = vec![if d { 0 } else { 0xff }; n];
            v[n - 1] = if d { 0x80 } else { 0x7f };
            v
        }
        (KeyType::Decimal, d) => {
            let mut v = vec![0x99; n];
            v[n - 1] = if d { 0x9d } else { 0x9c };
            v
        }
    };
    v.truncate(n);
    v
}

/// A key value as compressed in a bucket: its length, the bytes it shares
/// with the one before, and the rest, the last byte repeated to the end
/// left off.
fn expand(b: &[u8], prev: &[u8], len: usize) -> Result<(Vec<u8>, usize), Cond> {
    let (n, front) = (*b.first().ok_or(status::BUG)? as usize, b[1] as usize);
    let mut k = prev[..front.min(prev.len())].to_vec();
    k.extend_from_slice(b.get(2..2 + n).ok_or(status::BUG)?);
    let last = k.last().copied().unwrap_or(0);
    k.resize(len, last);
    Ok((k, 2 + n))
}

/// `key` compressed after `prev`: what it shares with `prev` off the
/// front, then a repeated last byte off the end.
fn squeeze(key: &[u8], prev: Option<&[u8]>) -> Vec<u8> {
    let front = prev.map_or(0, |p| p.iter().zip(key).take_while(|(a, b)| a == b).count());
    let front = front.min(key.len() - 1);
    let mut end = key.len();
    while end > front + 1 && key[end - 2] == key[end - 1] {
        end -= 1;
    }
    let mut out = vec![(end - front) as u8, front as u8];
    out.extend_from_slice(&key[front..end]);
    out
}

/// Record compression: segments of a length word, the bytes, and a count
/// of times the last byte repeats. RMS takes a run only where it fills a
/// longword aligned on the segment's start and a byte follows that
/// longword, and the run is five long or more (fixtures/idxw CR.DAT).
fn compress(r: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut seg = 0;
    let mut a = 0;
    while a + 4 < r.len() {
        let c = r[a];
        if r[a..a + 4].iter().all(|&x| x == c) {
            let mut s = a;
            while s > seg && r[s - 1] == c {
                s -= 1;
            }
            let mut e = a + 4;
            while e < r.len() && r[e] == c && e - s <= 255 {
                e += 1;
            }
            if e - s >= 5 {
                out.extend_from_slice(&((s + 1 - seg) as u16).to_le_bytes());
                out.extend_from_slice(&r[seg..=s]);
                out.push((e - s - 1) as u8);
                seg = e;
                a = e;
                continue;
            }
        }
        a += 4;
    }
    if seg < r.len() {
        out.extend_from_slice(&((r.len() - seg) as u16).to_le_bytes());
        out.extend_from_slice(&r[seg..]);
        out.push(0);
    }
    out
}

fn decompress(b: &[u8]) -> Result<Vec<u8>, Cond> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < b.len() {
        let n = u16_at(b.get(..at + 2).ok_or(status::BUG)?, at) as usize;
        let seg = b.get(at + 2..at + 2 + n).ok_or(status::BUG)?;
        out.extend_from_slice(seg);
        let rep = *b.get(at + 2 + n).ok_or(status::BUG)? as usize;
        let last = seg.last().copied().unwrap_or(0);
        out.resize(out.len() + rep, last);
        at += 3 + n;
    }
    Ok(out)
}

/// The record without its key's bytes.
fn rest_of(d: &KeyDesc, record: &[u8]) -> Vec<u8> {
    let mut keep = vec![true; record.len()];
    for s in &d.segments {
        for k in keep
            .iter_mut()
            .skip(s.position as usize)
            .take(s.length as usize)
        {
            *k = false;
        }
    }
    record
        .iter()
        .zip(keep)
        .filter_map(|(&b, k)| k.then_some(b))
        .collect()
}

/// The record from its key and the rest: the key's segments go back where
/// they were taken from.
fn insert_key(d: &KeyDesc, key: &[u8], rest: &[u8]) -> Vec<u8> {
    let mut segs: Vec<(usize, &[u8])> = Vec::new();
    let mut at = 0;
    for s in &d.segments {
        segs.push((s.position as usize, &key[at..at + s.length as usize]));
        at += s.length as usize;
    }
    segs.sort_by_key(|s| s.0);
    let mut out = Vec::new();
    let mut rest = rest;
    for (pos, bytes) in segs {
        let take = pos.saturating_sub(out.len()).min(rest.len());
        out.extend_from_slice(&rest[..take]);
        rest = &rest[take..];
        out.extend_from_slice(bytes);
    }
    out.extend_from_slice(rest);
    out
}

/// A bucket as read: its VBN and bytes.
#[derive(Debug, Clone)]
struct Bucket {
    vbn: u32,
    b: Vec<u8>,
}

impl Bucket {
    fn fresh(vbn: u32, blocks: u8, key: u8, level: u8, flags: u8) -> Bucket {
        let mut b = vec![0; blocks as usize * BLK];
        b[1] = key;
        put16(&mut b, 2, vbn as u16);
        put16(&mut b, 4, HDR as u16);
        put16(&mut b, 6, 1);
        put32(&mut b, 8, vbn);
        b[12] = level;
        b[13] = flags;
        Bucket { vbn, b }
    }
    fn free(&self) -> usize {
        u16_at(&self.b, 4) as usize
    }
    fn next_id(&self) -> u16 {
        u16_at(&self.b, 6)
    }
    fn set_next_id(&mut self, id: u16) {
        put16(&mut self.b, 6, id)
    }
    fn next(&self) -> u32 {
        u32_at(&self.b, 8)
    }
    fn set_next(&mut self, vbn: u32) {
        put32(&mut self.b, 8, vbn)
    }
    fn level(&self) -> u8 {
        self.b[12]
    }
    fn last(&self) -> bool {
        self.b[13] & LASTBKT != 0
    }
    fn set_flag(&mut self, flag: u8, on: bool) {
        self.b[13] = self.b[13] & !flag | if on { flag } else { 0 };
    }
    fn ptr_size(&self) -> usize {
        2 + (self.b[13] >> 3 & 3) as usize
    }
    /// Lays `body` out after the header; false if it doesn't fit.
    fn fill(&mut self, body: &[u8]) -> bool {
        let free = HDR + body.len();
        if free > self.b.len() - 2 {
            return false;
        }
        self.b[HDR..free].copy_from_slice(body);
        put16(&mut self.b, 4, free as u16);
        true
    }
}

/// An index entry: the highest key in the bucket below, and its VBN.
#[derive(Debug, Clone, PartialEq, Eq)]
struct IndexEntry {
    key: Vec<u8>,
    vbn: u32,
}

/// A record of a primary data bucket: a data record (perhaps a deleted
/// one, its key kept) or an RRV.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Rec {
    ctl: u8,
    id: u16,
    /// A data record: where it was put, its RFA. An RRV: where it went.
    rrv: Rfa,
    key: Vec<u8>,
    /// What is stored after the key: the rest of the record.
    tail: Vec<u8>,
}

impl Rec {
    fn live(&self) -> bool {
        self.ctl & (DELETED | RRV) == 0
    }
}

/// One element of a SIDR: the RFA of a record with the key value.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Elem {
    ctl: u8,
    rfa: Rfa,
}

impl Elem {
    fn new(rfa: Rfa, first: bool) -> Elem {
        let ctl = (ptr_size(rfa.vbn) - 2) as u8 | if first { FIRST } else { 0 };
        Elem { ctl, rfa }
    }
    fn live(&self) -> bool {
        self.ctl & DELETED == 0
    }
}

/// A secondary index data record: a key value and its records.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Sidr {
    key: Vec<u8>,
    elems: Vec<Elem>,
}

/// An indexed file: its blocks, and the record format from its FAB.
pub struct File<B> {
    pub blocks: B,
    /// FIX records are `mrs` long; VAR ones at most that (0: no limit).
    pub fixed: bool,
    pub mrs: u16,
    /// Allocations are rounded up to this many blocks: an ODS volume's
    /// cluster size, 1 on the host.
    pub cluster: u32,
}

/// A record found: its RFA, its bytes, and where it is in the key's order
/// (for [`File::next`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub rfa: Rfa,
    pub record: Vec<u8>,
    pub at: Cursor,
}

/// A place in a key's order: after the record with this key value and
/// RFA, the `dup`th of those with the value. It finds its place again
/// after other calls changed the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cursor {
    pub key: u8,
    value: Vec<u8>,
    rfa: Rfa,
    dup: usize,
}

/// One step of a key's order: the value and the record's RFA, `None` for
/// a deleted SIDR element (kept so others' places stay put).
#[derive(Debug, Clone)]
struct Entry {
    value: Vec<u8>,
    rfa: Option<Rfa>,
}

impl<B: Blocks> File<B> {
    pub fn new(blocks: B, fixed: bool, mrs: u16) -> File<B> {
        File {
            blocks,
            fixed,
            mrs,
            cluster: 1,
        }
    }

    fn read(&mut self, vbn: u32, blocks: u8) -> Result<Bucket, Cond> {
        let mut b = vec![0; blocks.max(1) as usize * BLK];
        self.blocks.read(vbn, &mut b)?;
        Ok(Bucket { vbn, b })
    }

    /// Writes a changed bucket, counting the write in its check byte.
    fn write(&mut self, b: &mut Bucket) -> Result<(), Cond> {
        let c = b.b[0].wrapping_add(1);
        let n = b.b.len();
        b.b[0] = c;
        b.b[n - 1] = c;
        self.blocks.write(b.vbn, &b.b)
    }

    /// The prologue: key descriptors from VBN 1, area descriptors.
    pub fn prologue(&mut self) -> Result<Prologue, Cond> {
        let mut p = Prologue {
            keys: Vec::new(),
            areas: Vec::new(),
            blocks: Vec::new(),
        };
        let get = |f: &mut Self, p: &mut Prologue, vbn: u32| -> Result<Vec<u8>, Cond> {
            if let Some((_, b, _)) = p.blocks.iter().find(|b| b.0 == vbn) {
                return Ok(b.clone());
            }
            let b = f.read(vbn, 1)?.b;
            if checksum(&b) != u16_at(&b, 510) || vbn == 1 && u16_at(&b, PLG_VER) != 3 {
                return Err(status::PLG);
            }
            p.blocks.push((vbn, b.clone(), false));
            Ok(b)
        };
        let first = get(self, &mut p, 1)?;
        let (mut vbn, mut off) = (1, 0);
        loop {
            let b = get(self, &mut p, vbn)?;
            let k = parse_key(&b[off..off + KEY_SIZE], (vbn, off));
            let (nv, no) = (u32_at(&b, off), u16_at(&b, off + 4) as usize);
            p.keys.push(k);
            if nv == 0 || p.keys.len() > 255 || no + KEY_SIZE > BLK {
                break;
            }
            (vbn, off) = (nv, no);
        }
        for i in 0..first[PLG_AMAX] as usize {
            let vbn = first[PLG_AVBN] as u32 + (i / 8) as u32;
            let off = i % 8 * AREA_SIZE;
            let b = get(self, &mut p, vbn)?;
            p.areas
                .push(parse_area(&b[off..off + AREA_SIZE], (vbn, off)));
        }
        Ok(p)
    }

    /// Writes back the prologue blocks a change touched.
    fn save(&mut self, p: &mut Prologue) -> Result<(), Cond> {
        for (vbn, b, changed) in p.blocks.iter_mut().filter(|b| b.2) {
            let c = checksum(b);
            put16(b, 510, c);
            self.blocks.write(*vbn, b)?;
            *changed = false;
        }
        Ok(())
    }

    /// Key `n`'s descriptor.
    fn key(&mut self, n: u8) -> Result<Key, Cond> {
        self.prologue()?
            .keys
            .into_iter()
            .find(|k| k.desc.number == n)
            .ok_or(status::KRF)
    }

    // ---- buckets and what is in them ----

    fn index_entries(k: &Key, b: &Bucket) -> Result<Vec<IndexEntry>, Cond> {
        let len = k.len();
        let mut keys: Vec<Vec<u8>> = Vec::new();
        let mut at = HDR;
        while at < b.free() {
            let key = if k.desc.index_compression {
                let prev = keys.last().map_or(&[][..], |k| k);
                let (key, n) = expand(&b.b[at..], prev, len)?;
                at += n;
                key
            } else {
                at += len;
                b.b.get(at - len..at).ok_or(status::BUG)?.to_vec()
            };
            keys.push(key);
        }
        let ptr = b.ptr_size();
        let mut p = b.b.len() - IDX_TAIL;
        keys.into_iter()
            .map(|key| {
                p = p.checked_sub(ptr).ok_or(status::BUG)?;
                Ok(IndexEntry {
                    key,
                    vbn: uint(&b.b[p..p + ptr]),
                })
            })
            .collect()
    }

    /// Lays index entries out in `b`; false if they don't fit.
    fn put_index(k: &Key, b: &mut Bucket, es: &[IndexEntry]) -> bool {
        let ptr = es.iter().map(|e| ptr_size(e.vbn)).max().unwrap_or(2);
        let mut body = Vec::new();
        let mut prev: Option<&[u8]> = None;
        for e in es {
            if k.desc.index_compression {
                body.extend(squeeze(&e.key, prev));
            } else {
                body.extend_from_slice(&e.key);
            }
            prev = Some(&e.key);
        }
        let n = b.b.len();
        let low = n - IDX_TAIL - ptr * es.len();
        if HDR + body.len() > low {
            return false;
        }
        b.b[HDR..HDR + body.len()].copy_from_slice(&body);
        put16(&mut b.b, 4, (HDR + body.len()) as u16);
        b.b[13] = b.b[13] & !0x18 | ((ptr - 2) as u8) << 3;
        let mut p = n - IDX_TAIL;
        for e in es {
            p -= ptr;
            b.b[p..p + ptr].copy_from_slice(&e.vbn.to_le_bytes()[..ptr]);
        }
        put16(&mut b.b, n - 4, (low - 1) as u16);
        b.b[n - 2] = 0;
        true
    }

    /// The records and RRVs of a primary data bucket.
    fn recs(&self, k: &Key, b: &Bucket) -> Result<(Vec<Rec>, Vec<Rec>), Cond> {
        let d = &k.desc;
        let sized = !self.fixed || d.data_key_compression || d.data_record_compression;
        let (mut recs, mut rrvs) = (Vec::new(), Vec::new());
        let mut at = HDR;
        while at < b.free() {
            let x = b.b.get(at..).ok_or(status::BUG)?;
            let (ctl, id) = (x[0], u16_at(x, 1));
            let ptr = 2 + (ctl & 3) as usize;
            let rrv = Rfa {
                id: u16_at(x, 3),
                vbn: uint(&x[5..5 + ptr]),
            };
            let mut len = 5 + ptr;
            let mut r = Rec {
                ctl,
                id,
                rrv,
                key: Vec::new(),
                tail: Vec::new(),
            };
            if ctl & RRV != 0 {
                rrvs.push(r);
            } else {
                let size = if sized {
                    len += 2;
                    u16_at(x, len - 2) as usize
                } else if ctl & DELETED != 0 {
                    k.len()
                } else {
                    self.mrs as usize
                };
                let body = x.get(len..len + size).ok_or(status::BUG)?;
                len += size;
                let (key, n) = if d.data_key_compression {
                    let prev = recs.last().map_or(&[][..], |r: &Rec| &r.key);
                    expand(body, prev, k.len())?
                } else {
                    (body.get(..k.len()).ok_or(status::BUG)?.to_vec(), k.len())
                };
                r.key = key;
                r.tail = body[n..].to_vec();
                recs.push(r);
            }
            at += len;
        }
        Ok((recs, rrvs))
    }

    /// A data record's bytes.
    fn record(&self, k: &Key, r: &Rec) -> Result<Vec<u8>, Cond> {
        let rest = if k.desc.data_record_compression {
            decompress(&r.tail)?
        } else {
            r.tail.clone()
        };
        Ok(insert_key(&k.desc, &r.key, &rest))
    }

    /// A record's key and what is stored after it.
    fn pack(&self, k: &Key, record: &[u8]) -> Rec {
        let d = &k.desc;
        let rest = rest_of(d, record);
        Rec {
            ctl: 2,
            id: 0,
            rrv: Rfa::default(),
            key: d.extract(record),
            tail: if d.data_record_compression {
                compress(&rest)
            } else {
                rest
            },
        }
    }

    /// Lays records and RRVs out in `b`; false if they don't fit.
    fn put_recs(&self, k: &Key, b: &mut Bucket, recs: &[Rec], rrvs: &[Rec]) -> bool {
        let d = &k.desc;
        let sized = !self.fixed || d.data_key_compression || d.data_record_compression;
        let mut body = Vec::new();
        let mut prev: Option<&[u8]> = None;
        for r in recs.iter().chain(rrvs) {
            body.push(r.ctl);
            body.extend_from_slice(&r.id.to_le_bytes());
            body.extend_from_slice(&r.rrv.id.to_le_bytes());
            body.extend_from_slice(&r.rrv.vbn.to_le_bytes()[..2 + (r.ctl & 3) as usize]);
            if r.ctl & RRV != 0 {
                continue;
            }
            let mut v = if d.data_key_compression {
                squeeze(&r.key, prev)
            } else {
                r.key.clone()
            };
            v.extend_from_slice(&r.tail);
            if sized {
                body.extend_from_slice(&(v.len() as u16).to_le_bytes());
            }
            body.extend(v);
            prev = Some(&r.key);
        }
        b.fill(&body)
    }

    /// The SIDRs of bucket `b`.
    fn sidrs(k: &Key, b: &Bucket) -> Result<Vec<Sidr>, Cond> {
        let mut out: Vec<Sidr> = Vec::new();
        let mut at = HDR;
        while at < b.free() {
            let size = u16_at(&b.b, at) as usize;
            let body = b.b.get(at + 2..at + 2 + size).ok_or(status::BUG)?;
            let (key, mut p) = if k.desc.data_key_compression {
                let prev = out.last().map_or(&[][..], |s| &s.key);
                expand(body, prev, k.len())?
            } else {
                (body.get(..k.len()).ok_or(status::BUG)?.to_vec(), k.len())
            };
            let mut elems = Vec::new();
            while p < body.len() {
                let ctl = body[p];
                if ctl & NOPTR != 0 {
                    elems.push(Elem {
                        ctl,
                        rfa: Rfa::default(),
                    });
                    p += 1;
                    continue;
                }
                let ptr = 2 + (ctl & 3) as usize;
                let x = body.get(p..p + 3 + ptr).ok_or(status::BUG)?;
                elems.push(Elem {
                    ctl,
                    rfa: Rfa {
                        id: u16_at(x, 1),
                        vbn: uint(&x[3..]),
                    },
                });
                p += 3 + ptr;
            }
            out.push(Sidr { key, elems });
            at += 2 + size;
        }
        Ok(out)
    }

    fn put_sidrs(k: &Key, b: &mut Bucket, sidrs: &[Sidr]) -> bool {
        let mut body = Vec::new();
        let mut prev: Option<&[u8]> = None;
        for s in sidrs {
            let mut v = if k.desc.data_key_compression {
                squeeze(&s.key, prev)
            } else {
                s.key.clone()
            };
            for e in &s.elems {
                v.push(e.ctl);
                if e.ctl & NOPTR == 0 {
                    v.extend_from_slice(&e.rfa.id.to_le_bytes());
                    v.extend_from_slice(&e.rfa.vbn.to_le_bytes()[..2 + (e.ctl & 3) as usize]);
                }
            }
            body.extend_from_slice(&(v.len() as u16).to_le_bytes());
            body.extend(v);
            prev = Some(&s.key);
        }
        b.fill(&body)
    }

    // ---- reading ----

    /// The index buckets from the root down to the data bucket where the
    /// records of key `k` matching `value` start: each bucket's VBN and
    /// the entry taken. Then that data bucket's VBN.
    fn descend(
        &mut self,
        k: &Key,
        value: &[u8],
        m: Match,
    ) -> Result<(Vec<(u32, usize)>, u32), Cond> {
        let (mut vbn, mut level) = (k.root, k.root_level);
        let mut path = Vec::new();
        while level > 0 {
            let b = self.read(vbn, k.index_bucket)?;
            let es = Self::index_entries(k, &b)?;
            let i = es
                .iter()
                .position(|e| {
                    let o = compare(&k.desc, &e.key, value);
                    o == Ordering::Greater || o == Ordering::Equal && m != Match::Gt
                })
                .unwrap_or(es.len().checked_sub(1).ok_or(status::BUG)?);
            path.push((vbn, i));
            vbn = es[i].vbn;
            level -= 1;
        }
        Ok((path, vbn))
    }

    /// The entries of key `k` in data bucket `b`, in order.
    fn entries(&self, k: &Key, b: &Bucket) -> Result<Vec<Entry>, Cond> {
        Ok(if k.desc.number == 0 {
            self.recs(k, b)?
                .0
                .into_iter()
                .filter(|r| r.live())
                .map(|r| Entry {
                    value: r.key,
                    rfa: Some(r.rrv),
                })
                .collect()
        } else {
            Self::sidrs(k, b)?
                .into_iter()
                .flat_map(|s| {
                    s.elems.into_iter().map(move |e| Entry {
                        value: s.key.clone(),
                        rfa: e.live().then_some(e.rfa),
                    })
                })
                .collect()
        })
    }

    /// Key `k`'s entries from the bucket where `value` is (by `m`), in
    /// order: calls `f` with each until it says stop.
    fn walk(
        &mut self,
        k: &Key,
        value: &[u8],
        m: Match,
        mut f: impl FnMut(&Entry) -> bool,
    ) -> Result<(), Cond> {
        if k.empty {
            return Ok(());
        }
        let mut vbn = self.descend(k, value, m)?.1;
        loop {
            let b = self.read(vbn, k.data_bucket)?;
            for e in self.entries(k, &b)? {
                if !f(&e) {
                    return Ok(());
                }
            }
            if b.last() {
                return Ok(());
            }
            vbn = b.next();
        }
    }

    /// Where the record with RFA `rfa` is: its bucket, following an RRV,
    /// and its place in the bucket's records.
    fn locate(&mut self, k: &Key, rfa: Rfa) -> Result<(Bucket, usize), Cond> {
        let mut at = rfa;
        for _ in 0..2 {
            if at.vbn == 0 || at.vbn > self.blocks.allocated() {
                return Err(status::RFA);
            }
            let b = self.read(at.vbn, k.data_bucket)?;
            if b.level() != 0 || b.b[1] != 0 {
                return Err(status::RFA);
            }
            let (recs, rrvs) = self.recs(k, &b)?;
            if let Some(i) = recs.iter().position(|r| r.id == at.id) {
                if !recs[i].live() || recs[i].rrv != rfa {
                    return Err(status::RNF);
                }
                return Ok((b, i));
            }
            let r = rrvs.iter().find(|r| r.id == at.id).ok_or(status::RNF)?;
            if r.ctl & DELETED != 0 {
                return Err(status::RNF);
            }
            at = r.rrv;
        }
        Err(status::RNF)
    }

    /// The record with RFA `rfa`.
    pub fn get_rfa(&mut self, rfa: Rfa) -> Result<Vec<u8>, Cond> {
        let k = self.key(0)?;
        let (b, i) = self.locate(&k, rfa)?;
        let recs = self.recs(&k, &b)?.0;
        self.record(&k, &recs[i])
    }

    /// The first record of key `key` whose value matches `value` by `m`.
    pub fn get(&mut self, key: u8, value: &[u8], m: Match) -> Result<Found, Cond> {
        let k = self.key(key)?;
        if value.is_empty() || value.len() > k.len() {
            return Err(status::KSZ);
        }
        let mut hit = None;
        let mut run: (Vec<u8>, usize) = (Vec::new(), 0);
        self.walk(&k, value, m, |e| {
            if run.0 != e.value {
                run = (e.value.clone(), 0);
            }
            let o = compare(&k.desc, &e.value, value);
            let ok = match m {
                Match::Eq | Match::Ge => o != Ordering::Less,
                Match::Gt => o == Ordering::Greater,
            };
            if ok && e.rfa.is_some() {
                hit = Some((e.clone(), run.1, o));
                return false;
            }
            run.1 += 1;
            true
        })?;
        let (e, dup, o) = hit.ok_or(status::RNF)?;
        if m == Match::Eq && o != Ordering::Equal {
            return Err(status::RNF);
        }
        self.found(key, e, dup)
    }

    fn found(&mut self, key: u8, e: Entry, dup: usize) -> Result<Found, Cond> {
        let rfa = e.rfa.unwrap();
        Ok(Found {
            rfa,
            record: self.get_rfa(rfa)?,
            at: Cursor {
                key,
                value: e.value,
                rfa,
                dup,
            },
        })
    }

    /// The first record in key `key`'s order.
    pub fn first(&mut self, key: u8) -> Result<Found, Cond> {
        let k = self.key(key)?;
        if k.empty {
            return Err(status::EOF);
        }
        let mut vbn = k.first;
        let mut run: (Vec<u8>, usize) = (Vec::new(), 0);
        loop {
            let b = self.read(vbn, k.data_bucket)?;
            for e in self.entries(&k, &b)? {
                if run.0 != e.value {
                    run = (e.value.clone(), 0);
                }
                if e.rfa.is_some() {
                    return self.found(key, e, run.1);
                }
                run.1 += 1;
            }
            if b.last() {
                return Err(status::EOF);
            }
            vbn = b.next();
        }
    }

    /// The record after `at` in its key's order.
    pub fn next(&mut self, at: &Cursor) -> Result<Found, Cond> {
        let k = self.key(at.key)?;
        // The entries with the cursor's value; then the first live one
        // after them, and how many dead ones with its value precede it.
        let mut same: Vec<Option<Rfa>> = Vec::new();
        let mut after = None;
        let mut run: (Vec<u8>, usize) = (Vec::new(), 0);
        self.walk(&k, &at.value, Match::Ge, |e| {
            match compare(&k.desc, &e.value, &at.value) {
                Ordering::Less => {}
                Ordering::Equal => same.push(e.rfa),
                Ordering::Greater => {
                    if run.0 != e.value {
                        run = (e.value.clone(), 0);
                    }
                    if e.rfa.is_some() {
                        after = Some((e.clone(), run.1));
                        return false;
                    }
                    run.1 += 1;
                }
            }
            true
        })?;
        let from = match same.iter().position(|r| *r == Some(at.rfa)) {
            Some(i) => i + 1,
            None if same.get(at.dup).is_some_and(|r| r.is_none()) => at.dup + 1,
            None => at.dup,
        };
        if let Some(i) = (from..same.len()).find(|&i| same[i].is_some()) {
            let e = Entry {
                value: at.value.clone(),
                rfa: same[i],
            };
            return self.found(at.key, e, i);
        }
        let (e, dup) = after.ok_or(status::EOF)?;
        self.found(at.key, e, dup)
    }

    // ---- writing ----

    /// A new bucket of `blocks` blocks from area `area`: the current
    /// extent's next, or a new extent of the area's extension.
    fn alloc(&mut self, p: &mut Prologue, area: u8) -> Result<u32, Cond> {
        let i = area as usize;
        let a = p.areas.get_mut(i).ok_or(status::BUG)?;
        let bks = a.bucket_size.max(1) as u32;
        if a.used + bks > a.blocks {
            let want = (a.extension as u32).max(bks).next_multiple_of(bks);
            let want = want.next_multiple_of(self.cluster.max(1));
            let first = self.blocks.grow(want)?;
            a.start = first;
            a.blocks = want;
            a.used = 0;
            a.next = first;
            a.total += want;
        }
        let vbn = a.next;
        a.next += bks;
        a.used += bks;
        p.store_area(i);
        Ok(vbn)
    }

    /// Gives key `ki` its first data bucket and root.
    fn start_key(&mut self, p: &mut Prologue, ki: usize) -> Result<(), Cond> {
        let k = p.keys[ki].clone();
        let data = self.alloc(p, k.desc.data_area)?;
        let root = self.alloc(p, k.desc.level1_index_area)?;
        let d = Bucket::fresh(data, k.data_bucket, k.desc.number, 0, LASTBKT);
        let mut r = Bucket::fresh(root, k.index_bucket, k.desc.number, 1, LASTBKT | ROOTBKT);
        let es = [IndexEntry {
            key: high_key(&k.desc),
            vbn: data,
        }];
        Self::put_index(&k, &mut r, &es);
        self.blocks.write(data, &d.b)?;
        self.blocks.write(root, &r.b)?;
        let k = &mut p.keys[ki];
        k.empty = false;
        k.root = root;
        k.root_level = 1;
        k.first = data;
        p.store_key(ki);
        Ok(())
    }

    /// Writes `record`; returns its RFA, and OK_DUP if a key it has was
    /// already some other record's.
    pub fn put(&mut self, record: &[u8]) -> Result<(Rfa, Cond), Cond> {
        let mut p = self.prologue()?;
        self.check_size(&p.keys[0], record)?;
        for k in &p.keys[1..] {
            if !k.desc.duplicates && k.in_record(record) && !k.empty {
                let v = k.desc.extract(record);
                if self.sidr_has(k, &v)? {
                    return Err(status::DUP);
                }
            }
        }
        if p.keys[0].empty {
            self.start_key(&mut p, 0)?;
        }
        let (rfa, mut dup) = self.put_primary(&mut p, record)?;
        for ki in 1..p.keys.len() {
            if p.keys[ki].in_record(record) {
                let v = p.keys[ki].desc.extract(record);
                dup |= self.sidr_add(&mut p, ki, &v, rfa)?;
            }
        }
        self.save(&mut p)?;
        Ok((rfa, if dup { status::OK_DUP } else { status::SUC }))
    }

    fn check_size(&self, k0: &Key, record: &[u8]) -> Result<(), Cond> {
        let bad = if self.fixed {
            record.len() != self.mrs as usize
        } else {
            self.mrs != 0 && record.len() > self.mrs as usize
        };
        if bad || record.len() < k0.min_size as usize {
            return Err(status::RSZ);
        }
        Ok(())
    }

    /// Whether key `k` has a live SIDR element with value `v`.
    fn sidr_has(&mut self, k: &Key, v: &[u8]) -> Result<bool, Cond> {
        let mut has = false;
        self.walk(k, v, Match::Ge, |e| match compare(&k.desc, &e.value, v) {
            Ordering::Less => true,
            Ordering::Equal => {
                has |= e.rfa.is_some();
                !has
            }
            Ordering::Greater => false,
        })?;
        Ok(has)
    }

    /// The last bucket of a run of continuation buckets for `key` that
    /// starts at `b`.
    fn chain_end(&mut self, k: &Key, mut b: Bucket, key: &[u8]) -> Result<Bucket, Cond> {
        loop {
            let last = if k.desc.number == 0 {
                self.recs(k, &b)?.0.last().map(|r| r.key.clone())
            } else {
                Self::sidrs(k, &b)?.last().map(|s| s.key.clone())
            };
            if b.last() || last.as_deref() != Some(key) {
                return Ok(b);
            }
            let n = self.read(b.next(), k.data_bucket)?;
            let first = if k.desc.number == 0 {
                self.recs(k, &n)?.0.first().map(|r| r.key.clone())
            } else {
                Self::sidrs(k, &n)?.first().map(|s| s.key.clone())
            };
            if first.as_deref() != Some(key) {
                return Ok(b);
            }
            b = n;
        }
    }

    /// Puts a record in the primary data buckets.
    fn put_primary(&mut self, p: &mut Prologue, record: &[u8]) -> Result<(Rfa, bool), Cond> {
        let k = p.keys[0].clone();
        let mut new = self.pack(&k, record);
        let (path, vbn) = self.descend(&k, &new.key, Match::Ge)?;
        let b = self.read(vbn, k.data_bucket)?;
        let mut b = self.chain_end(&k, b, &new.key)?;
        let (mut recs, rrvs) = self.recs(&k, &b)?;
        let pos = recs
            .iter()
            .position(|r| compare(&k.desc, &r.key, &new.key) == Ordering::Greater)
            .unwrap_or(recs.len());
        let dup = recs[..pos]
            .iter()
            .any(|r| r.live() && compare(&k.desc, &r.key, &new.key) == Ordering::Equal);
        if dup && !k.desc.duplicates {
            return Err(status::DUP);
        }
        let id = b.next_id();
        new.id = id;
        new.rrv = Rfa { vbn: b.vbn, id };
        // A run of puts at the bucket's end (its last record the last put)
        // splits off the new record alone; so does a bucket of nothing but
        // this key, into a continuation bucket.
        let seq = pos == recs.len() && recs.last().is_some_and(|r| r.id.wrapping_add(1) == id);
        let all_same = recs
            .iter()
            .all(|r| compare(&k.desc, &r.key, &new.key) == Ordering::Equal);
        recs.insert(pos, new.clone());
        b.set_next_id(id.wrapping_add(1));
        let mut try_b = b.clone();
        if self.put_recs(&k, &mut try_b, &recs, &rrvs) {
            self.write(&mut try_b)?;
            return Ok((new.rrv, dup));
        }
        let at = if seq || all_same {
            pos
        } else {
            self.split_at(&k, b.vbn, &recs, &rrvs, Some(pos))
        };
        if pos >= at {
            // It goes to the new bucket, taking no ID here.
            b.set_next_id(id);
        }
        let rfa = self.split_data(p, &k, path, b, recs, rrvs, at, Some(pos), all_same)?;
        Ok((rfa.unwrap_or(new.rrv), dup))
    }

    /// Where the records of bucket `vbn`, with these RRVs, split; `new`
    /// is the one being put.
    fn split_at(&self, k: &Key, vbn: u32, recs: &[Rec], rrvs: &[Rec], new: Option<usize>) -> usize {
        let born: Vec<bool> = recs
            .iter()
            .enumerate()
            .map(|(i, r)| Some(i) != new && r.live() && r.rrv.vbn == vbn)
            .collect();
        split_point(&self.sizes(k, recs), rrvs.len(), &born, recs, &k.desc)
    }

    /// The bytes each record takes laid out in a bucket after the one
    /// before.
    fn sizes(&self, k: &Key, recs: &[Rec]) -> Vec<usize> {
        let mut out = Vec::new();
        let mut b = Bucket::fresh(0, 64, 0, 0, 0);
        let mut last = HDR;
        for i in 0..recs.len() {
            self.put_recs(k, &mut b, &recs[..=i], &[]);
            out.push(b.free() - last);
            last = b.free();
        }
        out
    }

    /// Splits primary data bucket `b` at record `at`: `recs` from `at` on
    /// go to a new bucket after it, with new IDs, RRVs pointing there.
    /// Record `new` is the one being put, which leaves no RRV. Returns its
    /// RFA when it moved.
    #[allow(clippy::too_many_arguments)]
    fn split_data(
        &mut self,
        p: &mut Prologue,
        k: &Key,
        path: Vec<(u32, usize)>,
        mut b: Bucket,
        mut recs: Vec<Rec>,
        mut rrvs: Vec<Rec>,
        at: usize,
        new: Option<usize>,
        continuation: bool,
    ) -> Result<Option<Rfa>, Cond> {
        let right = recs.split_off(at);
        let nv = self.alloc(p, k.desc.data_area)?;
        let mut n = Bucket::fresh(nv, k.data_bucket, 0, 0, if b.last() { LASTBKT } else { 0 });
        n.set_next(b.next());
        b.set_next(nv);
        b.set_flag(LASTBKT, false);
        let mut moved = Vec::new();
        let mut rfa = None;
        let mut elsewhere = Vec::new();
        for (i, mut r) in right.into_iter().enumerate() {
            let to = Rfa {
                vbn: nv,
                id: i as u16 + 1,
            };
            if new == Some(at + i) {
                r.rrv = to;
                rfa = Some(to);
            } else if r.live() && r.rrv.vbn == b.vbn {
                rrvs.push(Rec {
                    ctl: RRV | 2,
                    id: r.rrv.id,
                    rrv: to,
                    key: Vec::new(),
                    tail: Vec::new(),
                });
            } else if r.live() {
                elsewhere.push((r.rrv, to));
            }
            r.id = to.id;
            moved.push(r);
        }
        n.set_next_id(moved.len() as u16 + 1);
        if !self.put_recs(k, &mut b, &recs, &rrvs) || !self.put_recs(k, &mut n, &moved, &[]) {
            return Err(status::BUG);
        }
        self.write(&mut b)?;
        self.write(&mut n)?;
        self.point_rrvs(k, elsewhere)?;
        if !continuation {
            let high = recs.last().ok_or(status::BUG)?.key.clone();
            self.index_add(p, 0, path, high, b.vbn, nv)?;
        }
        Ok(rfa)
    }

    /// Points the RRVs of records that moved again to where they are now,
    /// a write for each bucket the RRVs are in.
    fn point_rrvs(&mut self, k: &Key, mut moves: Vec<(Rfa, Rfa)>) -> Result<(), Cond> {
        moves.sort_by_key(|m| m.0.vbn);
        for group in moves.chunk_by(|a, b| a.0.vbn == b.0.vbn) {
            let mut b = self.read(group[0].0.vbn, k.data_bucket)?;
            let (recs, mut rrvs) = self.recs(k, &b)?;
            for (at, to) in group {
                if let Some(r) = rrvs.iter_mut().find(|r| r.id == at.id) {
                    r.rrv = *to;
                }
            }
            self.put_recs(k, &mut b, &recs, &rrvs);
            self.write(&mut b)?;
        }
        Ok(())
    }

    /// After a split of the bucket at the end of `path` into `left` and
    /// `right`, the index takes an entry for `left` (its highest key
    /// `high`), the entry that was the bucket's now pointing to `right`.
    fn index_add(
        &mut self,
        p: &mut Prologue,
        ki: usize,
        mut path: Vec<(u32, usize)>,
        high: Vec<u8>,
        left: u32,
        right: u32,
    ) -> Result<(), Cond> {
        let k = p.keys[ki].clone();
        let Some((vbn, i)) = path.pop() else {
            // The root split: a new root over the two.
            let root = self.alloc(p, k.desc.index_area)?;
            let level = k.root_level + 1;
            let mut r = Bucket::fresh(
                root,
                k.index_bucket,
                k.desc.number,
                level,
                LASTBKT | ROOTBKT,
            );
            let es = [
                IndexEntry {
                    key: high,
                    vbn: left,
                },
                IndexEntry {
                    key: high_key(&k.desc),
                    vbn: right,
                },
            ];
            Self::put_index(&k, &mut r, &es);
            self.write(&mut r)?;
            let k = &mut p.keys[ki];
            k.root = root;
            k.root_level = level;
            p.store_key(ki);
            return Ok(());
        };
        let mut b = self.read(vbn, k.index_bucket)?;
        let mut es = Self::index_entries(&k, &b)?;
        es[i].vbn = right;
        es.insert(
            i,
            IndexEntry {
                key: high,
                vbn: left,
            },
        );
        if Self::put_index(&k, &mut b, &es) {
            return self.write(&mut b);
        }
        // Split this index bucket too.
        let sizes: Vec<usize> = es
            .iter()
            .map(|e| {
                ptr_size(e.vbn)
                    + if k.desc.index_compression {
                        squeeze(&e.key, None).len()
                    } else {
                        k.len()
                    }
            })
            .collect();
        let at = balance(&sizes);
        let moved = es.split_off(at);
        let area = if b.level() == 1 {
            k.desc.level1_index_area
        } else {
            k.desc.index_area
        };
        let nv = self.alloc(p, area)?;
        let mut n = Bucket::fresh(
            nv,
            k.index_bucket,
            k.desc.number,
            b.level(),
            b.b[13] & LASTBKT,
        );
        n.set_next(b.next());
        b.set_next(nv);
        b.set_flag(LASTBKT | ROOTBKT, false);
        if !Self::put_index(&k, &mut b, &es) || !Self::put_index(&k, &mut n, &moved) {
            return Err(status::BUG);
        }
        self.write(&mut b)?;
        self.write(&mut n)?;
        let high = es.last().unwrap().key.clone();
        self.index_add(p, ki, path, high, vbn, nv)
    }

    /// Adds `rfa` to key `ki`'s SIDR for `value`; true if the value had
    /// other records.
    fn sidr_add(
        &mut self,
        p: &mut Prologue,
        ki: usize,
        value: &[u8],
        rfa: Rfa,
    ) -> Result<bool, Cond> {
        if p.keys[ki].empty {
            self.start_key(p, ki)?;
        }
        let k = p.keys[ki].clone();
        let (path, vbn) = self.descend(&k, value, Match::Ge)?;
        let b = self.read(vbn, k.data_bucket)?;
        let mut b = self.chain_end(&k, b, value)?;
        let mut sidrs = Self::sidrs(&k, &b)?;
        let at = sidrs
            .iter()
            .position(|s| compare(&k.desc, &s.key, value) != Ordering::Less)
            .unwrap_or(sidrs.len());
        let found = sidrs
            .get(at)
            .is_some_and(|s| compare(&k.desc, &s.key, value) == Ordering::Equal);
        // The last SIDR of the value's run: continuations follow.
        let at = if found {
            (at..sidrs.len())
                .take_while(|&i| compare(&k.desc, &sidrs[i].key, value) == Ordering::Equal)
                .last()
                .unwrap()
        } else {
            at
        };
        let dup = found && sidrs[at].elems.iter().any(|e| e.live());
        if found {
            sidrs[at].elems.push(Elem::new(rfa, false));
        } else {
            sidrs.insert(
                at,
                Sidr {
                    key: value.to_vec(),
                    elems: vec![Elem::new(rfa, true)],
                },
            );
        }
        let mut try_b = b.clone();
        if Self::put_sidrs(&k, &mut try_b, &sidrs) {
            self.write(&mut try_b)?;
            return Ok(dup);
        }
        if found && sidrs.len() == 1 {
            // A bucket of this value alone: it goes on in a continuation.
            sidrs[at].elems.pop();
            let cont = vec![Sidr {
                key: value.to_vec(),
                elems: vec![Elem::new(rfa, false)],
            }];
            self.split_sidrs(p, &k, path, &mut b, sidrs, cont, true)?;
            return Ok(dup);
        }
        let sizes: Vec<usize> = sidrs
            .iter()
            .map(|s| {
                let mut one = Bucket::fresh(0, 8, 0, 0, 0);
                Self::put_sidrs(&k, &mut one, std::slice::from_ref(s));
                one.free() - HDR
            })
            .collect();
        // A new value at the end goes to the new bucket alone; a value that
        // grew sends the last SIDR there.
        let cut = if !found && at + 1 == sidrs.len() {
            at
        } else if found {
            sidrs.len() - 1
        } else {
            balance(&sizes)
        };
        let right = sidrs.split_off(cut);
        self.split_sidrs(p, &k, path, &mut b, sidrs, right, false)?;
        Ok(dup)
    }

    #[allow(clippy::too_many_arguments)]
    fn split_sidrs(
        &mut self,
        p: &mut Prologue,
        k: &Key,
        path: Vec<(u32, usize)>,
        b: &mut Bucket,
        left: Vec<Sidr>,
        right: Vec<Sidr>,
        continuation: bool,
    ) -> Result<(), Cond> {
        let nv = self.alloc(p, k.desc.data_area)?;
        let mut n = Bucket::fresh(
            nv,
            k.data_bucket,
            k.desc.number,
            0,
            if b.last() { LASTBKT } else { 0 },
        );
        n.set_next(b.next());
        b.set_next(nv);
        b.set_flag(LASTBKT, false);
        if !Self::put_sidrs(k, b, &left) || !Self::put_sidrs(k, &mut n, &right) {
            return Err(status::BUG);
        }
        self.write(b)?;
        self.write(&mut n)?;
        if !continuation {
            let high = left.last().ok_or(status::BUG)?.key.clone();
            let ki = p
                .keys
                .iter()
                .position(|x| x.desc.number == k.desc.number)
                .unwrap();
            self.index_add(p, ki, path, high, b.vbn, nv)?;
        }
        Ok(())
    }

    /// Takes `rfa` out of key `k`'s SIDR for `value`: a SIDR's first
    /// element keeps its pointer, marked deleted; others shrink to their
    /// control byte.
    fn sidr_remove(&mut self, k: &Key, value: &[u8], rfa: Rfa) -> Result<(), Cond> {
        let mut vbn = self.descend(k, value, Match::Ge)?.1;
        loop {
            let mut b = self.read(vbn, k.data_bucket)?;
            let mut sidrs = Self::sidrs(k, &b)?;
            let hit = sidrs.iter().enumerate().find_map(|(i, s)| {
                (s.key == value)
                    .then(|| s.elems.iter().position(|e| e.live() && e.rfa == rfa))
                    .flatten()
                    .map(|j| (i, j))
            });
            if let Some((i, j)) = hit {
                let e = &mut sidrs[i].elems[j];
                if j == 0 {
                    e.ctl |= DELETED;
                } else {
                    e.ctl = DELETED | NOPTR;
                }
                // A SIDR with no record left goes, unless others with its
                // value go on from it or lead to it.
                if !sidrs[i].elems.iter().any(|e| e.live()) {
                    let after = i + 1 == sidrs.len() && !b.last() && {
                        let n = self.read(b.next(), k.data_bucket)?;
                        Self::sidrs(k, &n)?.first().is_some_and(|s| s.key == value)
                    };
                    let before = i == 0 && self.descend(k, value, Match::Ge)?.1 != vbn;
                    if !after && !before {
                        sidrs.remove(i);
                    }
                }
                Self::put_sidrs(k, &mut b, &sidrs);
                return self.write(&mut b);
            }
            if b.last()
                || sidrs
                    .last()
                    .is_some_and(|s| compare(&k.desc, &s.key, value) == Ordering::Greater)
            {
                return Err(status::BUG);
            }
            vbn = b.next();
        }
    }

    /// Deletes the record with RFA `rfa`. Its SIDR elements go; the record
    /// goes too, but its key stays as a deleted record when it is the
    /// highest in its bucket, which the index names.
    pub fn delete(&mut self, rfa: Rfa) -> Result<(), Cond> {
        let mut p = self.prologue()?;
        let k = p.keys[0].clone();
        let (b, i) = self.locate(&k, rfa)?;
        let (recs, _) = self.recs(&k, &b)?;
        let record = self.record(&k, &recs[i])?;
        for kn in p.keys[1..].iter().cloned() {
            if kn.in_record(&record) && !kn.empty {
                self.sidr_remove(&kn, &kn.desc.extract(&record), rfa)?;
            }
        }
        self.remove_primary(&k, b.vbn, i)?;
        if rfa.vbn != b.vbn {
            // Its RRV goes too.
            let mut o = self.read(rfa.vbn, k.data_bucket)?;
            let (recs, mut rrvs) = self.recs(&k, &o)?;
            rrvs.retain(|r| r.id != rfa.id);
            self.put_recs(&k, &mut o, &recs, &rrvs);
            self.write(&mut o)?;
        }
        self.save(&mut p)
    }

    fn remove_primary(&mut self, k: &Key, vbn: u32, i: usize) -> Result<(), Cond> {
        let mut b = self.read(vbn, k.data_bucket)?;
        let (mut recs, rrvs) = self.recs(k, &b)?;
        if recs.last().is_some_and(|l| l.key == recs[i].key) {
            recs[i].ctl |= DELETED;
            recs[i].tail.clear();
        } else {
            recs.remove(i);
        }
        self.put_recs(k, &mut b, &recs, &rrvs);
        self.write(&mut b)
    }

    /// Replaces the record with RFA `rfa` by `record`, its primary key the
    /// same; keys that can change may.
    pub fn update(&mut self, rfa: Rfa, record: &[u8]) -> Result<Cond, Cond> {
        let mut p = self.prologue()?;
        let k = p.keys[0].clone();
        self.check_size(&k, record)?;
        let (b, i) = self.locate(&k, rfa)?;
        let (recs, _) = self.recs(&k, &b)?;
        let old = self.record(&k, &recs[i])?;
        if k.desc.extract(&old) != k.desc.extract(record) {
            return Err(status::CHG);
        }
        let mut changed = Vec::new();
        for (ki, kn) in p.keys.iter().enumerate().skip(1) {
            let was = kn.in_record(&old).then(|| kn.desc.extract(&old));
            let is = kn.in_record(record).then(|| kn.desc.extract(record));
            if was != is {
                if !kn.desc.changes {
                    return Err(status::CHG);
                }
                changed.push((ki, was, is));
            }
        }
        for (ki, _, is) in &changed {
            let kn = &p.keys[*ki];
            if let Some(v) = is
                && !kn.desc.duplicates
                && !kn.empty
                && self.sidr_has(&kn.clone(), v)?
            {
                return Err(status::DUP);
            }
        }
        self.replace_primary(&mut p, &k, b.vbn, i, record)?;
        let mut dup = false;
        for (ki, was, is) in changed {
            let kn = p.keys[ki].clone();
            if let Some(v) = was {
                self.sidr_remove(&kn, &v, rfa)?;
            }
            if let Some(v) = is {
                dup |= self.sidr_add(&mut p, ki, &v, rfa)?;
            }
        }
        self.save(&mut p)?;
        Ok(if dup { status::OK_DUP } else { status::SUC })
    }

    /// Puts `record` in place of the `i`th record of bucket `vbn`; when it
    /// no longer fits, the bucket splits around it.
    fn replace_primary(
        &mut self,
        p: &mut Prologue,
        k: &Key,
        vbn: u32,
        i: usize,
        record: &[u8],
    ) -> Result<(), Cond> {
        let b = self.read(vbn, k.data_bucket)?;
        let (mut recs, rrvs) = self.recs(k, &b)?;
        let new = self.pack(k, record);
        recs[i].tail = new.tail;
        let mut try_b = b.clone();
        if self.put_recs(k, &mut try_b, &recs, &rrvs) {
            return self.write(&mut try_b);
        }
        let path = self.descend(k, &recs[i].key, Match::Ge)?.0;
        let at = self.split_at(k, vbn, &recs, &rrvs, None);
        self.split_data(p, k, path, b, recs, rrvs, at, None, false)?;
        Ok(())
    }

    /// What the file was made from: its areas and keys as the prologue
    /// has them, with `fab`.
    pub fn design(&mut self, fab: crate::Fab) -> Result<Design, Cond> {
        let p = self.prologue()?;
        Ok(Design {
            fab,
            max_record_number: 0,
            prologue: 3,
            areas: p
                .areas
                .iter()
                .enumerate()
                .map(|(i, a)| crate::Area {
                    number: i as u8,
                    allocation: a.total,
                    bucket_size: a.bucket_size,
                    extension: a.extension,
                    contiguous: false,
                    best_try_contiguous: false,
                })
                .collect(),
            keys: p.keys.iter().map(|k| k.desc.clone()).collect(),
        })
    }

    /// Makes an empty indexed file as `design` says, on `blocks`.
    pub fn create(blocks: B, design: &Design, cluster: u32) -> Result<File<B>, Cond> {
        let fab = &design.fab;
        let mut f = File {
            blocks,
            fixed: fab.rfm == crate::Rfm::Fix,
            mrs: fab.mrs,
            cluster: cluster.max(1),
        };
        let nkeys = design.keys.len();
        if nkeys == 0 || design.areas.is_empty() {
            return Err(status::IDX);
        }
        let key_blocks = 1 + (nkeys - 1).div_ceil(KEYS_PER_BLOCK) as u32;
        let area_vbn = key_blocks + 1;
        let plg = key_blocks + design.areas.len().div_ceil(8) as u32;
        let mut img = vec![0u8; plg as usize * BLK];
        let where_key = |i: usize| -> (u32, usize) {
            if i == 0 {
                (1, 0)
            } else {
                (
                    2 + ((i - 1) / KEYS_PER_BLOCK) as u32,
                    (i - 1) % KEYS_PER_BLOCK * KEY_SIZE,
                )
            }
        };
        let area_bks = |a: u8| {
            design
                .areas
                .iter()
                .find(|x| x.number == a)
                .map_or(1, |x| x.bucket_size.max(1))
        };
        for (i, d) in design.keys.iter().enumerate() {
            let (vbn, off) = where_key(i);
            let b = &mut img[(vbn as usize - 1) * BLK + off..][..KEY_SIZE];
            if i + 1 < nkeys {
                let (nv, no) = where_key(i + 1);
                put32(b, 0, nv);
                put16(b, 4, no as u16);
            }
            b[0x06] = d.index_area;
            b[0x07] = d.level1_index_area;
            b[0x08] = d.data_area;
            b[0x0a] = area_bks(d.index_area);
            b[0x0b] = area_bks(d.data_area);
            // Before a first record, the areas again (as RMS leaves them).
            b[0x0c] = d.data_area;
            b[0x0d] = d.index_area;
            b[0x0e] = d.level1_index_area;
            // Short keys and others than strings are not compressed.
            let squeezable = d.typ == KeyType::String && d.length() >= 6;
            let mut flags = INITIDX;
            for (on, bit) in [
                (d.duplicates, DUPKEYS),
                (d.changes && i > 0, CHGKEYS),
                (d.null_key, NULKEYS),
                (d.index_compression && squeezable, IDX_COMPR),
                (d.data_key_compression && squeezable, KEY_COMPR),
                (d.data_record_compression && i == 0, REC_COMPR),
            ] {
                if on {
                    flags |= bit;
                }
            }
            b[0x10] = flags;
            let code = type_code(d.typ, d.descending);
            b[0x11] = code;
            b[0x12] = d.segments.len() as u8;
            b[0x13] = d.null_value;
            b[0x14] = d.length() as u8;
            b[0x15] = i as u8;
            let min = d
                .segments
                .iter()
                .map(|s| s.position + s.length)
                .max()
                .unwrap_or(0);
            put16(b, 0x16, min);
            let fill = |pct_bytes: u16, a: u8| {
                if pct_bytes == 0 {
                    area_bks(a) as u16 * BLK as u16
                } else {
                    pct_bytes
                }
            };
            put16(b, 0x18, fill(d.index_fill, d.index_area));
            put16(b, 0x1a, fill(d.data_fill, d.data_area));
            for (j, s) in d.segments.iter().enumerate().take(8) {
                put16(b, 0x1c + 2 * j, s.position);
                b[0x2c + j] = s.length as u8;
                b[0x58 + j] = code;
            }
            let name = d.name.as_bytes();
            if !name.is_empty() {
                b[0x34..0x54].fill(b' ');
                let n = name.len().min(32);
                b[0x34..0x34 + n].copy_from_slice(&name[..n]);
            }
        }
        img[PLG_AVBN] = area_vbn as u8;
        img[PLG_AMAX] = design.areas.len() as u8;
        put16(&mut img, PLG_VER, 3);
        let mut next = 1;
        for (i, a) in design.areas.iter().enumerate() {
            let at = (area_vbn as usize - 1 + i / 8) * BLK + i % 8 * AREA_SIZE;
            let b = &mut img[at..at + AREA_SIZE];
            let n = a.allocation.max(1).next_multiple_of(f.cluster);
            b[2] = a.number;
            b[3] = a.bucket_size.max(1);
            put32(b, 0x0c, next);
            put32(b, 0x10, n);
            let used = if i == 0 { plg } else { 0 };
            put32(b, 0x14, used);
            put32(b, 0x18, next + used);
            put16(b, 0x24, a.extension);
            put32(b, 0x32, n);
            next += n;
        }
        let have = f.blocks.allocated();
        if have < next - 1 {
            f.blocks.grow(next - 1 - have)?;
        }
        for vbn in 1..=plg {
            let b = &mut img[(vbn as usize - 1) * BLK..vbn as usize * BLK];
            let c = checksum(b);
            put16(b, 510, c);
            f.blocks.write(vbn, b)?;
        }
        Ok(f)
    }
}

/// Where a bucket of records of these sizes splits: the first cut where
/// the old bucket (the records it keeps, half of the `rrvs` it has, half
/// of the RRVs the records `born` there leave when they go, and 50 bytes)
/// holds as much as the new one; then back to the nearest cut before it
/// between two key values. (Fits every split of fixtures/idx, idxw, idxv
/// and fixtures/accept's PARTS that we could follow; see OTHER_SPLITS in
/// tests/idx_write.rs for the files it doesn't.)
fn split_point(sizes: &[usize], rrvs: usize, born: &[bool], recs: &[Rec], k: &KeyDesc) -> usize {
    let total: usize = sizes.iter().sum();
    let mut left = 0;
    let mut cut = sizes.len() - 1;
    for j in 1..sizes.len() {
        left += sizes[j - 1];
        let leaving = born[j..].iter().filter(|&&b| b).count();
        if 2 * left + 9 * (rrvs + leaving) + 100 >= 2 * (total - left) {
            cut = j;
            break;
        }
    }
    let between = |j: usize| compare(k, &recs[j - 1].key, &recs[j].key) != Ordering::Equal;
    (1..=cut)
        .rev()
        .find(|&j| between(j))
        .or_else(|| (cut + 1..sizes.len()).find(|&j| between(j)))
        .unwrap_or(cut)
}

/// The most even cut of items of these sizes.
fn balance(sizes: &[usize]) -> usize {
    let total: usize = sizes.iter().sum();
    let mut best = (usize::MAX, 1);
    let mut left = 0;
    for j in 1..sizes.len() {
        left += sizes[j - 1];
        let d = left.abs_diff(total - left);
        if d < best.0 {
            best = (d, j);
        }
    }
    best.1
}
