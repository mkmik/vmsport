//! Indexed files written here as VMS wrote them: each file of
//! fixtures/idx and fixtures/idxw made again from its empty design by the
//! puts, updates and deletes its BUILD.COM did, compared block by block
//! with what VMS made (cluster 16, as on VMS's volume).

mod common;

use common::*;
use vms_rms::idx::{File, Match};
use vms_rms::status;

/// Every file, made again: name, the blocks that differ, and whether it
/// holds the same records in every key's order as VMS's.
fn rebuild() -> Vec<(String, Vec<usize>, bool)> {
    let mut out = Vec::new();
    let mut check = |area: &str, name: &str, mut f: File<Vec<u8>>| {
        let want = recorded(area, &format!("{name}.DAT"));
        if let Ok(dir) = std::env::var("IDX_DUMP") {
            std::fs::write(format!("{dir}/{name}.DAT"), &f.blocks).unwrap();
        }
        let mut v = File::new(want.clone(), f.fixed, f.mrs);
        let keys = v.prologue().unwrap().keys.len() as u8;
        let same = (0..keys).all(|k| order(&mut f, k) == order(&mut v, k));
        out.push((format!("{area}/{name}"), diff(&f.blocks, &want), same));
    };
    // fixtures/idx
    let idx = |allocs: &[u32]| empty("idx", "E1.DAT", allocs);
    for (name, like, allocs) in [
        ("E1", "E1.DAT", &[30][..]),
        ("E2", "E2.DAT", &[12]),
        ("E3", "E3.DAT", &[16]),
        ("E4", "E4.DAT", &[8, 8]),
        ("E5", "E5.DAT", &[64]),
        ("E6", "E6.DAT", &[8]),
        ("E7", "E7.DAT", &[4]),
    ] {
        check("idx", name, empty("idx", like, allocs));
    }
    let mut f = idx(&[30]);
    put(&mut f, idx_rec(37));
    check("idx", "P1", f);
    let mut f = idx(&[30]);
    put(&mut f, idx_rec(37));
    put(&mut f, idx_rec(74));
    check("idx", "P2", f);
    let mut f = idx(&[30]);
    for n in 1..=150 {
        put(&mut f, idx_rec(n * 37 % 151));
    }
    for k in ["ID000010", "ID000011", "ID000100"] {
        delete(&mut f, k);
    }
    update(&mut f, "ID000020", |r| {
        cat(&[&r[..8], b"NEWCITY   ", &r[18..]])
    });
    check("idx", "RB", f);
    let mut f = empty("idx", "E2.DAT", &[12]);
    for k in 1..=60 {
        put(&mut f, format!("K{k:05}{}", v(k * 2)));
    }
    check("idx", "AS", f);
    let mut f = empty("idx", "E2.DAT", &[12]);
    for k in (1..=60).rev() {
        put(&mut f, format!("K{k:05}{}", v(k * 2)));
    }
    check("idx", "DS", f);
    let mut f = idx(&[30]);
    for k in 1..=30 {
        put(&mut f, idx_rec(k));
    }
    for k in [7, 21, 28, 4, 11, 18, 25] {
        delete(&mut f, &format!("ID{k:06}"));
    }
    put(&mut f, idx_rec(4));
    update(&mut f, "ID000005", |r| {
        cat(&[&r[..8], b"NEWCITY   ", &r[18..40], b"N999", &r[44..]])
    });
    check("idx", "DEL", f);
    let mut f = empty("idx", "E7.DAT", &[4]);
    for i in 1..=60 {
        let key = ["AAA", "BBB", "AAA", "CCC"][i % 4];
        put(
            &mut f,
            format!(
                "{key} {i:04} {:04}{:04}{:04}{:04}{:04}",
                i * 3,
                i * 7,
                i * 11,
                i * 13,
                i * 17
            ),
        );
    }
    delete(&mut f, "AAA");
    update(&mut f, "BBB", |r| r.to_vec());
    update(&mut f, "CCC", |r| cat(&[r, b"-longer"]));
    check("idx", "PD", f);
    let mut f = empty("idx", "E4.DAT", &[8, 8]);
    for n in 1..=40 {
        let r = n * 17 % 41;
        let c = ["ROME  ", "PARIS ", "      ", "OSLO  ", "LIMA  "][r % 5];
        let mut rec = format!("XXB{:02}-----A{r:03}......{c}{}", r % 10, v(r % 30)).into_bytes();
        if r % 8 == 0 {
            rec.truncate(18);
        }
        put(&mut f, rec);
    }
    update(&mut f, "A007B07", |r| cat(&[r, b"longer"]));
    update(&mut f, "A009B09", |r| cat(&[&r[..20], b"ZAGREB", &r[26..]]));
    delete(&mut f, "A011B01");
    check("idx", "KS", f);
    let mut f = empty("idx", "E6.DAT", &[8]);
    for n in 1..=30 {
        let r = n * 7 % 31;
        put(
            &mut f,
            format!("abcK{r:04}xxC{:03}zzzzzzzzzzzz{}", r % 4, v(r)),
        );
    }
    check("idx", "NC", f);
    let mut f = empty("idx", "E3.DAT", &[16]);
    for n in 1..=40 {
        put(&mut f, ty_rec(n * 17 % 41));
    }
    check("idx", "TY", f);
    let mut f = empty("idx", "E5.DAT", &[64]);
    for n in 1..=300 {
        let k = n * 37 % 307;
        put(
            &mut f,
            format!("K{k:05}{}{}", "=".repeat(94), ".".repeat(100)),
        );
    }
    check("idx", "ML", f);

    // fixtures/idxw
    let w = |like: &str, allocs: &[u32]| empty("idxw", like, allocs);
    let dash = "-".repeat(56);
    for p in 0..8 {
        let mut f = w("SA0.DAT", &[16]);
        for i in 1..=7 {
            put(&mut f, format!("K{i:02}0{dash}"));
        }
        put(&mut f, format!("K{p:02}5{dash}"));
        check("idxw", &format!("SA{p}"), f);
        let mut f = w("SA0.DAT", &[16]);
        for i in (1..=7).rev() {
            put(&mut f, format!("K{i:02}0{dash}"));
        }
        put(&mut f, format!("K{p:02}5{dash}"));
        check("idxw", &format!("SD{p}"), f);
    }
    let mut f = w("SV1.DAT", &[16]);
    put(&mut f, format!("K010{}", "a".repeat(196)));
    for i in 2..=7 {
        put(&mut f, format!("K{i:02}0{}", "b".repeat(26)));
    }
    put(&mut f, format!("K045{}", "c".repeat(46)));
    check("idxw", "SV1", f);
    let mut f = w("SV1.DAT", &[16]);
    for i in 1..=6 {
        put(&mut f, format!("K{i:02}0{}", "b".repeat(26)));
    }
    put(&mut f, format!("K070{}", "a".repeat(196)));
    put(&mut f, format!("K035{}", "c".repeat(46)));
    check("idxw", "SV2", f);
    let mut f = w("CR.DAT", &[32]);
    let mut i = 0;
    let mut cr = |f: &mut File<Vec<u8>>, body: String| {
        i += 1;
        put(f, format!("K{i:03}{body}"));
    };
    for l in 2..=9 {
        cr(&mut f, v(l));
    }
    for q in 1..=7 {
        for l in 3..=9 {
            cr(&mut f, format!("{}{}", &"abcdefg"[..q], v(l)));
        }
    }
    for q in 0..=3 {
        for l in 3..=8 {
            cr(&mut f, format!("{}{}xyz", &"abc"[..q], v(l)));
            cr(&mut f, format!("zzzzzzzzz{}{}", &"abc"[..q], v(l)));
        }
    }
    check("idxw", "CR", f);
    let mut f = w("NK.DAT", &[16]);
    for n in 1..=22 {
        let r = n * 7 % 23;
        put(&mut f, format!("hdr00K{r:03}---C{:03}{}", r % 3, v(r)));
    }
    assert_eq!(f.put(b"hdr").unwrap_err(), status::RSZ);
    assert_eq!(
        f.put(format!("hdr00K999---C000{}", v(80)).as_bytes())
            .unwrap_err(),
        status::RSZ
    );
    check("idxw", "NK", f);
    let mut f = w("FK.DAT", &[16]);
    for n in 1..=22 {
        let r = n * 7 % 23;
        put(&mut f, format!("hdr00K{r:03}-----------"));
    }
    check("idxw", "FK", f);
    let mut f = w("SC.DAT", &[16]);
    for n in 1..=150 {
        let r = n * 7 % 151;
        put(&mut f, format!("R{r:04}-SAME-U{r:04}----"));
    }
    delete(&mut f, "R0007");
    delete(&mut f, "R0100");
    assert_eq!(f.put(b"R9999-SAME-U0005----").unwrap_err(), status::DUP);
    let hit = f.get(0, b"R0005", Match::Eq).unwrap();
    assert_eq!(
        f.update(hit.rfa, b"R0005-SAME-U9999----").unwrap_err(),
        status::CHG
    );
    let hit = f.get(0, b"R0006", Match::Eq).unwrap();
    assert_eq!(
        f.update(hit.rfa, b"R9998-SAME-U0006----").unwrap_err(),
        status::CHG
    );
    check("idxw", "SC", f);
    for d in 1..=5 {
        let mut f = w("SA0.DAT", &[16]);
        for i in 1..=7 {
            put(&mut f, format!("K{i:02}0{dash}"));
        }
        if d <= 2 {
            put(&mut f, format!("K035{dash}"));
        }
        match d {
            1 => delete(&mut f, "K060"),
            2 => delete(&mut f, "K070"),
            3 => {
                delete(&mut f, "K070");
                put(&mut f, format!("K070{dash}"));
            }
            4 => {
                delete(&mut f, "K070");
                put(&mut f, format!("K080{dash}"));
            }
            _ => {
                for i in 1..=7 {
                    delete(&mut f, &format!("K{i:02}0"));
                }
                put(&mut f, format!("K040{dash}"));
            }
        }
        check("idxw", &format!("DM{d}"), f);
    }
    let mut f = w("SV1.DAT", &[16]);
    for i in 1..=7 {
        put(&mut f, format!("K{i:02}0{}", "b".repeat(56)));
    }
    update(&mut f, "K050", |_| b"K050short".to_vec());
    check("idxw", "UP", f);
    // fixtures/idxv: what idx_roundtrip.rs does to RT1, RT2, RT5, done by
    // VMS to files of its own.
    let mut f = empty("idx", "E1.DAT", &[30]);
    for n in 1..=600 {
        put(&mut f, idx_rec(n * 97 % 601));
    }
    for k in (13..=600).step_by(13) {
        delete(&mut f, &format!("ID{k:06}"));
    }
    for k in (11..=600).step_by(11).filter(|k| k % 13 != 0) {
        update(&mut f, &format!("ID{k:06}"), |r| {
            [
                &r[..8],
                b"MOVED     ",
                &r[18..40],
                format!("M{k:03}").as_bytes(),
                &r[44..],
            ]
            .concat()
        });
    }
    check("idxv", "V1", f);
    let mut f = empty("idx", "E4.DAT", &[8, 8]);
    let ks = |r: usize, city: &str| {
        let mut rec = format!("XXB{:02}-----A{r:03}......{city}{}", r % 100, v(r % 30));
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
        put(&mut f, ks(r, city(r)));
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
            format!("{}+", ks(r, other)).into_bytes()
        });
    }
    check("idxv", "V2", f);
    let mut f = empty("idx", "E7.DAT", &[4]);
    for i in 1..=300 {
        let key = ["AAA", "BBB", "CCC", "DDD", "EEE"][i * 7 % 5];
        put(&mut f, format!("{key} {i:04} {:04}{:04}", i * 3, i * 7));
    }
    for key in ["AAA", "CCC", "EEE"] {
        delete(&mut f, key);
    }
    check("idxv", "V5", f);
    let mut f = w("MC.DAT", &[64]);
    for n in 1..=300 {
        let k = n * 37 % 307;
        let s = format!("{k:05}").repeat(8);
        put(
            &mut f,
            format!("{s}{:<20}", ["alpha", "beta", "gamma", "delta"][k % 4]),
        );
    }
    check("idxw", "MC", f);
    out
}

/// Key `key`'s records, in order.
fn order(f: &mut File<Vec<u8>>, key: u8) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut r = f.first(key);
    while let Ok(hit) = r {
        out.push(hit.record);
        r = f.next(&hit.at);
    }
    out
}

/// Where VMS split a bucket at another record than we do (see
/// `split_point`): the files hold the same records, laid out otherwise.
const OTHER_SPLITS: [&str; 8] = [
    "idx/RB", "idx/KS", "idx/ML", "idxw/SC", "idxw/MC", "idxv/V1", "idxv/V2", "idxv/V5",
];

#[test]
fn files_written_as_vms_wrote_them() {
    let got = rebuild();
    for (name, d, same) in &got {
        eprintln!("{name:12} {same} {d:?}");
        assert!(same, "{name} reads otherwise than VMS's");
        assert_eq!(
            d.is_empty(),
            !OTHER_SPLITS.contains(&name.as_str()),
            "{name}: {d:?}"
        );
    }
}
