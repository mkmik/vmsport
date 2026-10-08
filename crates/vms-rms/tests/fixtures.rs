//! Checks vms-rms against sequential files made on OpenVMS: fixtures/rms
//! (one file per record format) and fixtures/rmsblk (records meeting block
//! boundaries, FORTRAN carriage control).

use vms_rms::{Fab, Org, Record, Rfm, Text, decode, encode};

fn fixture(path: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(path)
}

fn read(path: &str) -> String {
    std::fs::read_to_string(fixture(path)).unwrap()
}

/// A number field of `name`'s entry in an ods-manifest.json.
fn field(manifest: &str, name: &str, key: &str) -> usize {
    let entry = &manifest[manifest
        .find(&format!("\"name\": \"{name}\""))
        .unwrap_or_else(|| panic!("{name}"))..];
    let v = &entry[entry.find(&format!("\"{key}\": ")).unwrap() + key.len() + 4..];
    v[..v.find([',', '\n']).unwrap()].trim().parse().unwrap()
}

/// The file's attributes and its bytes up to the end of file, as the file
/// header says.
fn file(area: &str, name: &str) -> (Fab, Vec<u8>) {
    let m = read(&format!("{area}/recorded/ods-manifest.json"));
    let f = |k| field(&m, name, k);
    let rtype = f("rtype");
    let fab = Fab {
        org: [Org::Seq, Org::Rel, Org::Idx][rtype >> 4],
        rfm: [
            Rfm::Udf,
            Rfm::Fix,
            Rfm::Var,
            Rfm::Vfc,
            Rfm::Stm,
            Rfm::Stmlf,
            Rfm::Stmcr,
        ][rtype & 15],
        rat: f("rattrib") as u8,
        mrs: f("maxrec") as u16,
        lrl: f("rsize") as u16,
        fsz: f("vfcsize") as u8,
        bks: f("bktsize") as u8,
        deq: f("defext") as u16,
    };
    let mut bytes = std::fs::read(fixture(&format!("{area}/recorded/{name}"))).unwrap();
    bytes.truncate(f("bytes"));
    (fab, bytes)
}

/// `bytes` with the pad byte of odd-length VAR and VFC records and the filler
/// after an end-of-block mark (-1) zeroed: RMS leaves whatever was in its
/// buffer there, vms-rms writes zeros.
fn unfilled(fab: &Fab, mut bytes: Vec<u8>) -> Vec<u8> {
    if !matches!(fab.rfm, Rfm::Var | Rfm::Vfc) {
        return bytes;
    }
    let mut i = 0;
    while i + 1 < bytes.len() {
        let n = u16::from_le_bytes([bytes[i], bytes[i + 1]]) as usize;
        if n == 0xffff {
            let end = (i + 2).next_multiple_of(512).min(bytes.len());
            bytes[i + 2..end].fill(0);
            i = end;
        } else {
            if n % 2 == 1 && i + 2 + n < bytes.len() {
                bytes[i + 2 + n] = 0;
            }
            i += 2 + n + n % 2;
        }
    }
    bytes
}

fn text(fab: &Fab, recs: &[Record]) -> String {
    let mut t = Text::new();
    let mut out: Vec<u8> = recs.iter().flat_map(|r| t.record(fab, r)).collect();
    out.extend(t.finish());
    String::from_utf8(out).unwrap()
}

/// Decodes `name`, checks the records and their longest length, and that
/// encoding them gives the file back byte for byte.
fn check(area: &str, name: &str, want: &[String]) -> (Fab, Vec<Record>) {
    let (fab, bytes) = file(area, name);
    let recs = decode(&fab, &bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
    let data: Vec<String> = recs
        .iter()
        .map(|r| String::from_utf8(r.data.clone()).unwrap())
        .collect();
    assert_eq!(data, want, "{name}");
    assert_eq!(
        recs.iter().map(|r| r.data.len()).max().unwrap_or(0),
        fab.lrl as usize,
        "{name} longest record"
    );
    assert_eq!(
        encode(&fab, &recs).unwrap(),
        unfilled(&fab, bytes),
        "{name} encoded"
    );
    (fab, recs)
}

#[test]
fn every_sequential_format() {
    // What rms.com wrote.
    let com = read("rms/rms.com");
    let lines = com
        .split("$ lines = \"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap();
    let lines: Vec<String> = lines.split('|').map(String::from).collect();
    let fixed: Vec<String> = ["thirteen char", "0123456789abc", "             "]
        .map(String::from)
        .into();

    // TYPE of all six files: the first, then " ", the file name and " "
    // before each of the others.
    let log = read("rms/recorded/rms.log");
    let typed = log
        .rsplit("%ANLRMS-I-ERRORS")
        .next()
        .unwrap()
        .split_once('\n')
        .unwrap()
        .1;
    let typed = &typed[..typed.find("relative record 1.").unwrap()];
    let mut shown = typed
        .split("\n \nDKA200:[T]")
        .map(|s| match s.split_once(";1\n \n") {
            Some((_, rest)) => rest.to_string(),
            None => s.to_string(),
        });
    let first = shown.next().unwrap() + "\n";
    let mut shown: Vec<String> = std::iter::once(first)
        .chain(shown.map(|s| s + "\n"))
        .collect();
    shown.last_mut().unwrap().pop();

    for (i, name) in [
        "SEQVAR", "SEQVFC", "SEQSTM", "SEQLFSTM", "SEQCRSTM", "SEQFIX",
    ]
    .iter()
    .enumerate()
    {
        let want = if *name == "SEQFIX" { &fixed } else { &lines };
        let (fab, recs) = check("rms", &format!("{name}.DAT"), want);
        if fab.rfm == Rfm::Vfc {
            // DCL's WRITE: one new line before, a carriage return after.
            assert!(recs.iter().all(|r| r.control == [1, 0x8d]));
        }
        assert_eq!(text(&fab, &recs), shown[i], "{name} as TYPE shows it");
    }
}

#[test]
fn block_boundaries() {
    let n = |count: usize, f: &dyn Fn(usize) -> String| (1..=count).map(f).collect::<Vec<_>>();
    let mut var = n(25, &|i| format!("{i:02}{}", "-".repeat(48)));
    var.extend(["v".repeat(509), "w".repeat(510), "after".into()]);
    check("rmsblk", "VARBLK.DAT", &var);
    check(
        "rmsblk",
        "FIXBLK.DAT",
        &n(12, &|i| format!("{i:03}{}", "=".repeat(98))),
    );
    check(
        "rmsblk",
        "VFCBLK.DAT",
        &n(20, &|i| format!("{i:02}{}", "~".repeat(59))),
    );
    let long = [
        "a".repeat(700),
        "b".repeat(300),
        "c".repeat(1100),
        "end".into(),
    ];
    check("rmsblk", "VARLONG.DAT", &long);
}

#[test]
fn fortran_carriage_control() {
    let recs: Vec<String> = [
        " single", "0double", "1page", "+over", "$prompt", "xother", "",
    ]
    .map(String::from)
    .into();
    let (fab, recs) = check("rmsblk", "FTN.DAT", &recs);
    assert_eq!(
        text(&fab, &recs),
        "single\n\ndouble\n\x0cpage\rover\nprompt\nother\n"
    );
}
