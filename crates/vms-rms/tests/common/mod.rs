//! What the indexed-file tests share: fixtures, files made from the
//! designs of recorded ones, the records the VMS procedures wrote.

#![allow(dead_code)]

use vms_rms::idx::{File, Match};
use vms_rms::{Design, Fab, Org, Rfm};

pub fn fixture(path: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(path)
}

pub fn field(manifest: &str, name: &str, key: &str) -> usize {
    let entry = &manifest[manifest
        .find(&format!("\"name\": \"{name}\""))
        .unwrap_or_else(|| panic!("{name}"))..];
    let v = &entry[entry.find(&format!("\"{key}\": ")).unwrap() + key.len() + 4..];
    v[..v.find([',', '\n']).unwrap()].trim().parse().unwrap()
}

pub fn fab(area: &str, name: &str) -> Fab {
    let m =
        std::fs::read_to_string(fixture(&format!("{area}/recorded/ods-manifest.json"))).unwrap();
    let rtype = field(&m, name, "rtype");
    Fab {
        org: Org::Idx,
        rfm: if rtype & 15 == 1 { Rfm::Fix } else { Rfm::Var },
        rat: field(&m, name, "rattrib") as u8,
        mrs: field(&m, name, "maxrec") as u16,
        lrl: 0,
        fsz: 0,
        bks: field(&m, name, "bktsize") as u8,
    }
}

pub fn recorded(area: &str, name: &str) -> Vec<u8> {
    std::fs::read(fixture(&format!("{area}/recorded/{name}"))).unwrap()
}

/// The design of `like` (a recorded file), its areas `allocs` blocks.
pub fn design(area: &str, like: &str, allocs: &[u32]) -> Design {
    let mut f = File::new(recorded(area, like), false, 0);
    let mut d = f.design(fab(area, like)).unwrap();
    for (a, &n) in d.areas.iter_mut().zip(allocs) {
        a.allocation = n;
    }
    d
}

/// An empty file made from the design of `like`, as on VMS's volume
/// (cluster 16).
pub fn empty(area: &str, like: &str, allocs: &[u32]) -> File<Vec<u8>> {
    File::create(Vec::new(), &design(area, like, allocs), 16).unwrap()
}

/// `file` with what RMS leaves beyond a bucket's free space zeroed: in
/// data buckets after the records, in index buckets between the keys and
/// the pointers. RMS moves bytes about in its buffer and leaves old copies
/// there, which mean nothing.
pub fn scrub(mut file: Vec<u8>) -> Vec<u8> {
    let n = file.len() / 512;
    for v in 1..=n {
        let at = (v - 1) * 512;
        let b = &file[at..];
        if u16::from_le_bytes([b[2], b[3]]) != v as u16 || b[12] > 32 {
            continue;
        }
        let Some(len) = [512, 1024, 1536, 2048]
            .into_iter()
            .find(|&l| at + l <= file.len() && file[at + l - 1] == b[0])
        else {
            continue;
        };
        let b = &mut file[at..at + len];
        let free = u16::from_le_bytes([b[4], b[5]]) as usize;
        let end = if b[12] == 0 {
            len - 1
        } else {
            u16::from_le_bytes([b[len - 4], b[len - 3]]) as usize + 1
        };
        if free <= end && end <= len {
            b[free..end].fill(0);
        }
    }
    file
}

/// The VBNs where `got` and `want` differ.
pub fn diff(got: &[u8], want: &[u8]) -> Vec<usize> {
    let (got, want) = (&scrub(got.to_vec())[..], &scrub(want.to_vec())[..]);
    let n = got.len().max(want.len()) / 512;
    (0..n)
        .filter(|&i| got.get(i * 512..(i + 1) * 512) != want.get(i * 512..(i + 1) * 512))
        .map(|i| i + 1)
        .collect()
}

pub fn put(f: &mut File<Vec<u8>>, r: impl AsRef<[u8]>) {
    f.put(r.as_ref()).unwrap();
}

pub fn delete(f: &mut File<Vec<u8>>, key: &str) {
    let hit = f.get(0, key.as_bytes(), Match::Eq).unwrap();
    f.delete(hit.rfa).unwrap();
}

/// READ/KEY then WRITE/UPDATE with what `change` makes of the record.
pub fn update(f: &mut File<Vec<u8>>, key: &str, change: impl Fn(&[u8]) -> Vec<u8>) {
    let hit = f.get(0, key.as_bytes(), Match::Eq).unwrap();
    let new = change(&hit.record);
    f.update(hit.rfa, &new).unwrap();
}

pub fn idx_rec(k: usize) -> String {
    let cities = ["ROME", "PARIS", "ZAGREB", "OSLO", "LIMA", "TOKYO", "QUITO"];
    format!(
        "ID{k:06}{:<10}..{:04}......{:04}......N{:03}....................",
        cities[k % 7],
        k / 10,
        k % 3,
        k / 2
    )
}

pub fn cat(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

pub fn v(n: usize) -> String {
    "v".repeat(n)
}

/// TY.DAT's record `r`: a key of every type.
pub fn ty_rec(r: usize) -> Vec<u8> {
    let mut rec = format!("R{r:03}{}", "x".repeat(36)).into_bytes();
    let ri = r as i64;
    rec[4..6].copy_from_slice(&((ri % 9 - 4) as i16).to_le_bytes());
    rec[6..10].copy_from_slice(&((ri % 11 * 1000003 - 5000000) as i32).to_le_bytes());
    rec[10..14].copy_from_slice(&((ri * 1234567) as u32).to_le_bytes());
    rec[14..18].copy_from_slice(&((ri % 3 - 1) as i32).to_le_bytes());
    rec[18..20].copy_from_slice(&((r % 7 * 9000) as u16).to_le_bytes());
    rec[20..24].copy_from_slice(&((r * 7) as u32).to_le_bytes());
    let hi: u32 = if r % 2 == 1 { 0x8000_0000 } else { 0 };
    rec[24..28].copy_from_slice(&hi.to_le_bytes());
    let d = ri % 13 * 111 - 600;
    let s = if d < 0 { 13 } else { 12 };
    let d = d.unsigned_abs() as u32;
    rec[28] = ((d / 10000) * 16 + d / 1000 % 10) as u8;
    rec[29] = ((d / 100 % 10) * 16 + d / 10 % 10) as u8;
    rec[30] = ((d % 10) * 16 + s) as u8;
    rec[31..35].copy_from_slice(format!("S{:03}", r % 6).as_bytes());
    rec[35..39].copy_from_slice(&((ri % 5 - 2) as i32).to_le_bytes());
    rec
}

/// The records of a VAR sequential file CONVERT wrote, up to its end mark.
pub fn seq(path: &str) -> Vec<Vec<u8>> {
    let b = std::fs::read(fixture(path)).unwrap();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < b.len() {
        let n = u16::from_le_bytes([b[i], b[i + 1]]);
        if n == 0xffff {
            return out;
        }
        out.push(b[i + 2..i + 2 + n as usize].to_vec());
        i += 2 + n as usize + n as usize % 2;
    }
    out
}
