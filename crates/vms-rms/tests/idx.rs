//! Indexed files made on OpenVMS (fixtures/rms, fixtures/idx) read here as
//! VMS read them: every key's order as CONVERT/KEY=n dumped it, and DCL's
//! keyed READs (fixtures/idx/recorded/reads.log).

use vms_rms::Rfa;
use vms_rms::idx::{File, Match};

fn fixture(path: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(path)
}

/// A number field of `name`'s entry in an ods-manifest.json.
fn field(manifest: &str, name: &str, key: &str) -> usize {
    let entry = &manifest[manifest
        .find(&format!("\"name\": \"{name}\""))
        .unwrap_or_else(|| panic!("{name}"))..];
    let v = &entry[entry.find(&format!("\"{key}\": ")).unwrap() + key.len() + 4..];
    v[..v.find([',', '\n']).unwrap()].trim().parse().unwrap()
}

/// An indexed file of fixtures/AREA/recorded, with its record format.
fn open(area: &str, name: &str) -> File<Vec<u8>> {
    let m =
        std::fs::read_to_string(fixture(&format!("{area}/recorded/ods-manifest.json"))).unwrap();
    let rtype = field(&m, name, "rtype");
    assert_eq!(rtype >> 4, 2, "{name} is indexed");
    let bytes = std::fs::read(fixture(&format!("{area}/recorded/{name}"))).unwrap();
    File::new(bytes, rtype & 15 == 1, field(&m, name, "maxrec") as u16)
}

/// The records of a VAR sequential file CONVERT wrote, up to its end mark.
fn seq(path: &str) -> Vec<Vec<u8>> {
    let b = std::fs::read(fixture(path)).unwrap();
    let mut out = Vec::new();
    let mut i = 0;
    loop {
        let n = u16::from_le_bytes([b[i], b[i + 1]]);
        if n == 0xffff {
            return out;
        }
        out.push(b[i + 2..i + 2 + n as usize].to_vec());
        i += 2 + n as usize + n as usize % 2;
    }
}

/// Key `key`'s records in order, by first/next.
fn all(f: &mut File<Vec<u8>>, key: u8) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut r = f.first(key);
    while let Ok(hit) = r {
        assert_eq!(f.get_rfa(hit.rfa).unwrap(), hit.record);
        out.push(hit.record);
        r = f.next(&hit.at);
    }
    assert_eq!(r.unwrap_err(), vms_rms::status::EOF);
    out
}

#[test]
fn every_key_order_as_convert_dumped_it() {
    for (name, keys) in [
        ("RB", 3),
        ("DEL", 3),
        ("PD", 1),
        ("KS", 1),
        ("NC", 1),
        ("TY", 8),
        ("ML", 0),
    ] {
        let mut f = open("idx", &format!("{name}.DAT"));
        for k in 0..=keys {
            let want = seq(&format!("idx/recorded/{name}{k}.SEQ"));
            assert_eq!(all(&mut f, k), want, "{name} key {k}");
        }
    }
}

/// The records rms.com wrote: 150 of them, three deleted, one changed.
fn idx_records() -> Vec<Vec<u8>> {
    let cities = ["ROME", "PARIS", "ZAGREB", "OSLO", "LIMA", "TOKYO", "QUITO"];
    let mut out: Vec<Vec<u8>> = (1..=150)
        .map(|n| {
            let k = n * 37 % 151;
            let city = if k == 20 { "NEWCITY" } else { cities[k % 7] };
            format!(
                "ID{k:06}{city:<10}..{:04}......{:04}......N{:03}....................",
                k / 10,
                k % 3,
                k / 2
            )
            .into_bytes()
        })
        .filter(|r| ![&b"ID000010"[..], b"ID000011", b"ID000100"].contains(&&r[..8]))
        .collect();
    out.sort();
    out
}

#[test]
fn rms_fixtures_read() {
    let mut f = open("rms", "IDX.DAT");
    assert_eq!(all(&mut f, 0), idx_records());
    for k in 1..=3 {
        let mut got = all(&mut f, k);
        got.sort();
        assert_eq!(got, idx_records(), "key {k}");
    }
    let mut f = open("rms", "IDXVAR.DAT");
    let want: Vec<Vec<u8>> = (1..=60)
        .map(|k| format!("K{k:05}{}", "v".repeat(k * 2)).into_bytes())
        .collect();
    assert_eq!(all(&mut f, 0), want);
}

#[test]
fn keyed_reads_as_dcl_did() {
    let log = std::fs::read_to_string(fixture("idx/recorded/reads.log")).unwrap();
    let mut lines = log
        .lines()
        .filter(|l| l.starts_with('+') || l.starts_with('%'));
    let mut want = || {
        let l = lines.next().unwrap();
        match l.strip_prefix("+ ") {
            Some(r) => Ok(r.as_bytes().to_vec()),
            None if l.contains("-RNF,") => Err(vms_rms::status::RNF),
            None => Err(vms_rms::status::EOF),
        }
    };
    let mut f = open("idx", "RB.DAT");
    let mut at = None;
    let mut read = |f: &mut File<Vec<u8>>, key: Option<(u8, &str, Match)>| {
        let r = match key {
            Some((k, v, m)) => f.get(k, v.as_bytes(), m),
            None => match &at {
                Some(c) => f.next(c),
                None => f.first(0),
            },
        };
        if let Ok(hit) = &r {
            at = Some(hit.at.clone());
        }
        r.map(|h| h.record)
    };
    use Match::*;
    let steps: Vec<Option<(u8, &str, Match)>> = vec![
        None,
        Some((0, "ID000050", Eq)),
        None,
        Some((0, "ID00005", Ge)),
        Some((0, "ID00005", Eq)),
        Some((0, "ID00005", Gt)),
        Some((0, "ID000010", Eq)),
        Some((0, "ID000010", Ge)),
        Some((0, "ID000010", Gt)),
        Some((0, "ID000149", Gt)),
        None,
        Some((0, "ID000151", Ge)),
        Some((1, "OSLO", Eq)),
        None,
        None,
        None,
        Some((1, "P", Ge)),
        Some((1, "P", Gt)),
        Some((1, "NEWCITY", Eq)),
        Some((1, "ZAGREB", Eq)),
    ];
    for s in steps {
        assert_eq!(read(&mut f, s), want(), "{s:?}");
    }
    for _ in 0..22 {
        assert_eq!(read(&mut f, None), want());
    }
    for s in [
        Some((2, "0001", Eq)),
        None,
        Some((3, "N050", Eq)),
        None,
        None,
    ] {
        assert_eq!(read(&mut f, s), want(), "{s:?}");
    }
    // KS: a descending key; then DCL's READ/KEY="A0" (still key 1).
    let mut f = open("idx", "KS.DAT");
    for s in [
        (1, "P", Ge),
        (1, "P", Gt),
        (1, "ZZ", Ge),
        (1, "A", Ge),
        (1, "A0", Eq),
    ] {
        assert_eq!(
            f.get(s.0, s.1.as_bytes(), s.2).map(|h| h.record),
            want(),
            "{s:?}"
        );
    }
    let mut f = open("idx", "PD.DAT");
    for s in [(0, "AAA", Eq), (0, "BBB", Eq), (0, "AAA", Gt)] {
        assert_eq!(
            f.get(s.0, s.1.as_bytes(), s.2).map(|h| h.record),
            want(),
            "{s:?}"
        );
    }
    let _ = Rfa::default();
}
