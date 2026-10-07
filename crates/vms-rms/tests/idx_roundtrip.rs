//! Indexed files written here, judged by VMS (fixtures/idxrt): made here
//! the same way every time, sent to VMS, which found no errors in them with
//! ANALYZE/RMS_FILE/CHECK, read them in every key's order (CONVERT/KEY=n)
//! and by key (DCL READ/KEY), and went on writing to one of them.
//!
//! IDX_WRITE_FIXTURES=1 writes the files into fixtures/idxrt for a new
//! recording (fixtures/vms/record.py idxrt).

mod common;

use common::*;
use vms_rms::idx::{File, Match};

/// A file made here from a recorded file's design, on the host (no
/// clusters).
fn host(like: &str, allocs: &[u32]) -> File<Vec<u8>> {
    File::create(Vec::new(), &design("idx", like, allocs), 1).unwrap()
}

fn put(f: &mut File<Vec<u8>>, r: impl AsRef<[u8]>) {
    f.put(r.as_ref()).unwrap();
}

fn delete(f: &mut File<Vec<u8>>, key: &str) {
    let hit = f.get(0, key.as_bytes(), Match::Eq).unwrap();
    f.delete(hit.rfa).unwrap();
}

fn update(f: &mut File<Vec<u8>>, key: &str, change: impl Fn(&[u8]) -> Vec<u8>) {
    let hit = f.get(0, key.as_bytes(), Match::Eq).unwrap();
    let new = change(&hit.record);
    f.update(hit.rfa, &new).unwrap();
}

/// IDX.FDL's design: 600 records out of order, every 13th deleted, every
/// 11th moved to another city and number. Splits at both index levels,
/// extensions of 12 blocks.
fn rt1() -> File<Vec<u8>> {
    let mut f = host("E1.DAT", &[30]);
    for n in 1..=600 {
        put(&mut f, idx_rec(n * 97 % 601));
    }
    for k in (13..=600).step_by(13) {
        delete(&mut f, &format!("ID{k:06}"));
    }
    for k in (11..=600).step_by(11).filter(|k| k % 13 != 0) {
        update(&mut f, &format!("ID{k:06}"), |r| {
            let num = format!("M{:03}", k % 1000);
            [&r[..8], b"MOVED     ", &r[18..40], num.as_bytes(), &r[44..]].concat()
        });
    }
    f
}

/// What VMS did to RT1 (RT1V): 100 more records, 20 deleted.
fn rt1v(f: &mut File<Vec<u8>>) {
    for n in 1..=100 {
        put(f, idx_rec(601 + n * 37 % 101));
    }
    for k in (2..=40).step_by(2).filter(|k| k % 13 != 0) {
        delete(f, &format!("ID{k:06}"));
    }
}

/// KS.FDL's design: two areas, a segmented primary key away from the
/// front, a descending alternate key with a null value and long runs of
/// duplicates (continuation buckets).
fn rt2() -> File<Vec<u8>> {
    let mut f = host("E4.DAT", &[8, 8]);
    let rec = |r: usize, city: &str| {
        let mut rec = format!(
            "XXB{:02}-----A{r:03}......{city}{}",
            r % 100,
            "v".repeat(r % 30)
        );
        if r.is_multiple_of(8) {
            rec.truncate(18);
        }
        rec
    };
    let city = |r: usize| {
        if r.is_multiple_of(10) {
            "      "
        } else {
            ["ROME  ", "PARIS "][r % 2]
        }
    };
    for n in 1..=900 {
        let r = n * 367 % 901;
        put(&mut f, rec(r, city(r)));
    }
    for r in (17..=900).step_by(17) {
        delete(&mut f, &format!("A{r:03}B{:02}", r % 100));
    }
    for r in (19..=900).step_by(19).filter(|r| r % 17 != 0 && r % 8 != 0) {
        let other = if city(r) == "ROME  " {
            "PARIS "
        } else {
            "ROME  "
        };
        update(&mut f, &format!("A{r:03}B{:02}", r % 100), |_| {
            format!("{}+", rec(r, other)).into_bytes()
        });
    }
    f
}

/// TY.FDL's design: a key of every type, 300 records, every 9th deleted.
fn rt3() -> File<Vec<u8>> {
    let mut f = host("E3.DAT", &[16]);
    for n in 1..=300 {
        put(&mut f, ty_rec(n * 67 % 307));
    }
    for n in (9..=300).step_by(9) {
        delete(&mut f, &format!("R{:03}", n * 67 % 307));
    }
    f
}

/// ML.FDL's design: 100-byte keys, nothing compressed, a deep index.
fn rt4() -> File<Vec<u8>> {
    let mut f = host("E5.DAT", &[64]);
    for n in 1..=200 {
        let k = n * 37 % 211;
        put(
            &mut f,
            format!("K{k:05}{}{}", "=".repeat(94), ".".repeat(100)),
        );
    }
    f
}

/// PDUP.FDL's design: duplicate primary keys over continuation buckets.
fn rt5() -> File<Vec<u8>> {
    let mut f = host("E7.DAT", &[4]);
    for i in 1..=300 {
        let key = ["AAA", "BBB", "CCC", "DDD", "EEE"][i * 7 % 5];
        put(&mut f, format!("{key} {i:04} {:04}{:04}", i * 3, i * 7));
    }
    for key in ["AAA", "CCC", "EEE"] {
        delete(&mut f, key);
    }
    f
}

/// KS.FDL's design, empty.
fn rt6() -> File<Vec<u8>> {
    host("E4.DAT", &[8, 8])
}

/// IDXVAR.FDL's design, one record.
fn rt7() -> File<Vec<u8>> {
    let mut f = host("E2.DAT", &[12]);
    put(&mut f, "K00001vv");
    f
}

/// The files, their record attributes for record.py, and the number of
/// their last key.
fn files() -> Vec<(&'static str, File<Vec<u8>>, &'static str, u8)> {
    vec![
        (
            "RT1",
            rt1(),
            "org=idx rfm=fix rat=cr mrs=64 lrl=64 bks=1",
            3,
        ),
        ("RT2", rt2(), "org=idx rfm=var rat=cr mrs=80 bks=2", 1),
        (
            "RT3",
            rt3(),
            "org=idx rfm=fix rat=none mrs=40 lrl=40 bks=1",
            8,
        ),
        (
            "RT4",
            rt4(),
            "org=idx rfm=fix rat=cr mrs=200 lrl=200 bks=1",
            0,
        ),
        ("RT5", rt5(), "org=idx rfm=var rat=cr mrs=40 bks=1", 1),
        ("RT6", rt6(), "org=idx rfm=var rat=cr mrs=80 bks=2", 1),
        ("RT7", rt7(), "org=idx rfm=var rat=cr mrs=200 bks=2", 0),
    ]
}

/// The first 16 bytes of each record in key `key`'s order.
fn order(f: &mut File<Vec<u8>>, key: u8) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut r = f.first(key);
    while let Ok(hit) = r {
        out.push(hit.record[..hit.record.len().min(16)].to_vec());
        r = f.next(&hit.at);
    }
    out
}

fn recorded_rt(name: &str) -> Option<Vec<u8>> {
    std::fs::read(fixture(&format!("idxrt/recorded/{name}"))).ok()
}

#[test]
fn files_are_the_ones_vms_checked() {
    for (name, f, attrs, _) in files() {
        let path = fixture(&format!("idxrt/{name}.DAT"));
        if std::env::var("IDX_WRITE_FIXTURES").is_ok() {
            std::fs::write(&path, &f.blocks).unwrap();
            eprintln!("binary {name}.DAT {attrs}");
        }
        assert!(std::fs::read(&path).unwrap() == f.blocks, "{name} changed");
    }
}

#[test]
fn vms_found_no_errors() {
    for (name, ..) in files() {
        let Some(chk) = recorded_rt(&format!("{name}.CHK")) else {
            continue;
        };
        let chk = String::from_utf8_lossy(&chk);
        assert!(
            chk.contains("The analysis uncovered NO errors."),
            "{name}:\n{chk}"
        );
    }
}

#[test]
fn vms_read_them_in_every_key_order() {
    for (name, mut f, _, last) in files() {
        for k in 0..=last {
            let Some(_) = recorded_rt(&format!("{name}{k}.SEQ")) else {
                continue;
            };
            let want = seq(&format!("idxrt/recorded/{name}{k}.SEQ"));
            assert_eq!(order(&mut f, k), want, "{name} key {k}");
        }
    }
}

#[test]
fn vms_went_on_writing_and_so_do_we() {
    let Some(want) = recorded_rt("RT1V.DAT") else {
        return;
    };
    let mut f = rt1();
    rt1v(&mut f);
    // VMS's file reads as ours does, in every key's order...
    let mut v = File::new(want.clone(), true, 64);
    for k in 0..=3 {
        assert_eq!(order(&mut v, k), order(&mut f, k), "key {k}");
    }
    let chk = String::from_utf8_lossy(&recorded_rt("RT1V.CHK").unwrap()).to_string();
    assert!(chk.contains("The analysis uncovered NO errors."), "{chk}");
}

/// Each key's order here is its values' order, and holds every record
/// that has the key, each once.
#[test]
fn every_order_is_whole_and_sorted() {
    for (name, mut f, _, last) in files() {
        let p = f.prologue().unwrap();
        let mut all = Vec::new();
        let mut r = f.first(0);
        while let Ok(hit) = r {
            all.push(hit.record.clone());
            r = f.next(&hit.at);
        }
        for k in 0..=last {
            let d = &p.keys[k as usize].desc;
            let mut got = Vec::new();
            let mut r = f.first(k);
            while let Ok(hit) = r {
                got.push(hit.record.clone());
                r = f.next(&hit.at);
            }
            for w in got.windows(2) {
                let o = vms_rms::idx::compare(d, &d.extract(&w[0]), &d.extract(&w[1]));
                assert!(
                    o.is_le() && (d.duplicates || o.is_lt()),
                    "{name} key {k} out of order"
                );
            }
            let min = d
                .segments
                .iter()
                .map(|s| (s.position + s.length) as usize)
                .max()
                .unwrap();
            let mut want: Vec<_> = all
                .iter()
                .filter(|r| {
                    r.len() >= min
                        && !(d.null_key && d.extract(r).iter().all(|&b| b == d.null_value))
                })
                .cloned()
                .collect();
            got.sort();
            want.sort();
            assert_eq!(got.len(), want.len(), "{name} key {k}");
            assert!(got == want, "{name} key {k}");
        }
    }
}

/// Our audit finds nothing wrong in these files, nor in VMS's.
#[test]
fn audits_find_nothing() {
    let mut all: Vec<(String, File<Vec<u8>>)> = files()
        .into_iter()
        .map(|(n, f, ..)| (n.to_string(), f))
        .collect();
    for (area, names) in [
        (
            "idx",
            &["RB", "AS", "DS", "DEL", "PD", "KS", "NC", "TY", "ML"][..],
        ),
        (
            "idxw",
            &["SC", "MC", "DM1", "DM2", "SV1", "UP", "CR", "NK", "FK"][..],
        ),
    ] {
        for n in names {
            let name = format!("{n}.DAT");
            let fab = fab(area, &name);
            all.push((
                format!("{area}/{n}"),
                File::new(recorded(area, &name), fab.rfm == vms_rms::Rfm::Fix, fab.mrs),
            ));
        }
    }
    for (name, mut f) in all {
        let keys = f.prologue().unwrap().keys.len();
        for k in 0..keys {
            let (errs, _) = f.audit(k).unwrap();
            assert!(errs.is_empty(), "{name} key {k}: {errs:?}");
        }
    }
}
