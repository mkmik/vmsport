//! SORT, MERGE and CONVERT say and make what they did on VMS
//! (fixtures/sort): SORT.COM and CONVERT.COM run here as record.py ran them
//! there, their logs compared section by section, their output files byte
//! for byte up to the end of file, with their record attributes.

use std::path::{Path, PathBuf};
use std::process::Command;
use vms_rms::{Fab, Record, Rfm};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sort")
}

/// Sections that differ: VMS drops duplicates inside its sort tree,
/// keeping others; it sizes the tree otherwise for decimal keys.
const KNOWN: &[&str] = &["SORT noduplicates", "SORT decimal"];

/// Which of two records with equal keys CONVERT keeps in a file that
/// takes no duplicates: the order its sort leaves them in (C13, C14).
fn mask_duplicates(s: &str) -> String {
    [
        "apple   0100 a",
        "apple   0007 d",
        "banana  0012 b",
        "banana  0012 f",
    ]
    .iter()
    .fold(s.to_string(), |s, r| s.replace(r, &r[..8]))
}

/// `@@ name` sections of a log.
fn sections(log: &str) -> Vec<(String, String)> {
    log.split("@@ ")
        .filter_map(|s| s.split_once('\n'))
        .map(|(n, b)| (n.trim().to_string(), b.to_string()))
        .collect()
}

/// Statistics a run can't repeat: memory, I/O, times.
fn mask(s: &str) -> String {
    let fields = [
        "Working set:",
        "Virtual memory:",
        "Direct I/O:",
        "Buffered I/O:",
        "Page faults:",
        "Elapsed time:",
        "Elapsed CPU:",
        "Buffered I/O Count:",
        "Direct I/O Count:",
        "Page Faults:",
        "Elapsed Time:",
        "CPU Time:",
    ];
    let mut out = s.to_string();
    for f in fields {
        let mut done = 0;
        while let Some(i) = out[done..].find(f) {
            // The value and the blanks that align it.
            let start = done + i + f.len();
            let end = out[start..]
                .find(|c: char| !(c.is_ascii_digit() || " \t:.".contains(c)))
                .map_or(out.len(), |e| start + e);
            out.replace_range(start..end, " *");
            done = start;
        }
    }
    out.lines()
        .filter(|l| !l.starts_with("%DCL-I-SUPERSEDE"))
        .map(|l| format!("{}\n", l.trim_end()))
        .collect()
}

#[test]
fn sort_and_convert_as_vms() {
    let st = Command::new(env!("CARGO"))
        .args(["build", "-q", "-p", "vms-dcl", "-p", "vmsportd"])
        .status()
        .unwrap();
    assert!(st.success());
    let dcl = Path::new(env!("CARGO_BIN_EXE_sort")).with_file_name("dcl");
    let tmp = std::env::temp_dir().join(format!("vpt-sort-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let dir = tmp.join("T/SORT");
    std::fs::create_dir_all(&dir).unwrap();
    // The inputs as record.py made them: variable records, CR.
    for f in [
        "IN.TXT",
        "B.TXT",
        "NUM.TXT",
        "FIX20.FDL",
        "REL.FDL",
        "SEQ.FDL",
        "STM.FDL",
        "IDX.FDL",
        "IDXND.FDL",
    ] {
        let text = std::fs::read_to_string(fixtures().join(f)).unwrap();
        let recs: Vec<Record> = text
            .lines()
            .map(|l| Record::new(l.as_bytes().to_vec()))
            .collect();
        let lrl = recs.iter().map(|r| r.data.len()).max().unwrap_or(0) as u16;
        let fab = Fab {
            rfm: Rfm::Var,
            lrl,
            ..Fab::default()
        };
        let path = dir.join(format!("{f};1"));
        std::fs::write(&path, vms_rms::encode(&fab, &recs).unwrap()).unwrap();
        libvms::sys::set_xattr(&path, vms_rms::XATTR, fab.to_string().as_bytes()).unwrap();
    }
    for f in ["SORT.COM", "CONVERT.COM"] {
        std::fs::copy(fixtures().join(f), dir.join(f)).unwrap();
    }
    std::fs::write(
        tmp.join("T/RECORD.COM"),
        "$ SET NOON\n$ SET DEFAULT DKA200:[T.SORT]\n$ @SORT.COM/OUTPUT=SORT.LOG\n$ @CONVERT.COM/OUTPUT=CONVERT.LOG\n",
    )
    .unwrap();
    let run = PathBuf::from(format!("/tmp/vpt-st{}", std::process::id()));
    let root = vmsportd::host_dir(&tmp, true);
    let out = Command::new(&dcl)
        .args([
            "-c",
            &format!("DEFINE/TRANSLATION=CONCEALED DKA200 {root}"),
            "@DKA200:[T]RECORD.COM",
        ])
        .env("VMSPORT_RUN", &run)
        .env("VMSPORT_JOB", format!("{:X}", std::process::id()))
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    if let Ok(c) = vmsportd::Client::connect_in(&run, Path::new("/nonexistent")) {
        let _ = c.stop();
    }
    let _ = std::fs::remove_dir_all(&run);
    println!("{}", String::from_utf8_lossy(&out.stderr));

    let mut failures = Vec::new();
    for name in ["SORT", "CONVERT"] {
        let want = std::fs::read(fixtures().join(format!("recorded/{name}.log"))).unwrap();
        let got = std::fs::read(dir.join(format!("{name}.LOG;1"))).unwrap_or_default();
        let (want, got) = (
            String::from_utf8_lossy(&want),
            String::from_utf8_lossy(&got),
        );
        let (want, got) = (sections(&mask(&want)), sections(&mask(&got)));
        let fix = |n: &str, t: &str| match n {
            "indexed" if name == "CONVERT" => mask_duplicates(t),
            _ => t.to_string(),
        };
        assert_eq!(
            want.iter().map(|s| &s.0).collect::<Vec<_>>(),
            got.iter().map(|s| &s.0).collect::<Vec<_>>(),
            "{name} sections"
        );
        for ((n, w), (_, g)) in want.iter().zip(&got) {
            let (w, g) = (fix(n, w), fix(n, g));
            if w != g && !KNOWN.contains(&format!("{name} {n}").as_str()) {
                failures.push(format!("{name} @@ {n}:\n--- VMS\n{w}--- vmsport\n{g}"));
            }
        }
    }

    // The files, as far as the end of file (relative files: whole).
    let manifest: serde_like::Manifest =
        serde_like::read(&fixtures().join("recorded/ods-manifest.json"));
    for name in [
        "S1.DAT", "S14.DAT", "S15.DAT", "S16.DAT", "S17.DAT", "S18.DAT", "S19.DAT", "S20.DAT",
        "M3.DAT", "C1.DAT", "C3.DAT", "C4.DAT", "C5.EXC", "C7.DAT", "C8.DAT", "C9.DAT", "C17.DAT",
        "C18.DAT", "C22.DAT",
    ] {
        let e = manifest.entry(name);
        let want = std::fs::read(fixtures().join(format!("recorded/{name}"))).unwrap();
        let path = dir.join(format!("{name};1"));
        let got = std::fs::read(&path).unwrap_or_default();
        let n = if e.rtype >> 4 == 0 {
            e.bytes
        } else {
            want.len()
        };
        if got.get(..n) != want.get(..n) {
            failures.push(format!("{name}: blocks differ"));
        }
        let fab = libvms::files::fab(&path);
        let ours = (
            fab.rfm as u8 | (fab.org as u8) << 4,
            fab.rat,
            fab.lrl,
            fab.mrs,
            fab.fsz,
            fab.bks,
        );
        let vms = (e.rtype, e.rattrib, e.rsize, e.maxrec, e.vfcsize, e.bktsize);
        if ours != vms {
            failures.push(format!("{name}: attributes {ours:?}, VMS {vms:?}"));
        }
    }
    // Indexed files: VMS's records in every key's order, and its prologue
    // byte for byte (ponytail: the buckets differ where vms_rms::idx lays
    // them out otherwise: fast loads, duplicate keys, fixed records'
    // compression). C13 keeps the other duplicates (see above).
    const RFMS: [Rfm; 7] = [
        Rfm::Udf,
        Rfm::Fix,
        Rfm::Var,
        Rfm::Vfc,
        Rfm::Stm,
        Rfm::Stmlf,
        Rfm::Stmcr,
    ];
    for name in [
        "C10", "C11", "C12", "C13", "C14", "C15", "C16", "C19", "C21", "S21",
    ] {
        // The file VMS checked (fixtures/sortback).
        let ours = dir.join(format!("{name}.DAT;1"));
        let got = std::fs::read(&ours).unwrap_or_default();
        let back = fixtures().join(format!("../sortback/ours/{name}.DAT"));
        if std::fs::read(&back).ok().as_ref() != Some(&got) {
            failures.push(format!(
                "{name}: not the file VMS checked (fixtures/sortback)"
            ));
        }
        // VMS's own (not recorded for C14).
        if name == "C14" {
            continue;
        }
        let e = manifest.entry(&format!("{name}.DAT"));
        let vms = tmp.join(format!("VMS{name}.DAT;1"));
        let want = std::fs::read(fixtures().join(format!("recorded/{name}.DAT"))).unwrap();
        std::fs::write(&vms, &want).unwrap();
        let fab = Fab {
            org: vms_rms::Org::Idx,
            rfm: RFMS[e.rtype as usize & 15],
            rat: e.rattrib,
            mrs: e.maxrec,
            lrl: e.rsize,
            fsz: e.vfcsize,
            bks: e.bktsize,
            deq: 0,
        };
        libvms::sys::set_xattr(&vms, vms_rms::XATTR, fab.to_string().as_bytes()).unwrap();
        // C13's records are compared by their keys alone.
        let n = if name == "C13" { 8 } else { usize::MAX };
        let cut = |r: Vec<Vec<u8>>| -> Vec<Vec<u8>> {
            r.into_iter()
                .map(|r| r[..n.min(r.len())].to_vec())
                .collect()
        };
        for key in 0..2 {
            if cut(records(&ours, key)) != cut(records(&vms, key)) {
                failures.push(format!("{name}: records by key {key} differ"));
            }
        }
        if name != "C13" && got.get(..1536) != want.get(..1536) {
            failures.push(format!("{name}: prologue differs"));
        }
    }
    let _ = std::fs::remove_dir_all(&tmp);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// What VMS made of the indexed files SORT and CONVERT made here
/// (fixtures/sortback, which `sort_and_convert_as_vms` checks they still
/// are): ANALYZE/RMS_FILE/CHECK found no errors, and DCL read the records
/// vmsport reads, by every key.
#[test]
fn vms_checks_our_indexed_files() {
    let back = fixtures().join("../sortback");
    let log = std::fs::read(back.join("recorded/SORTBACK.log")).unwrap();
    let log = String::from_utf8_lossy(&log);
    for (name, body) in log.split("@@ ").skip(1).filter_map(|s| s.split_once('\n')) {
        if name == "end" {
            continue;
        }
        let chk = std::fs::read_to_string(back.join(format!("recorded/{name}.CHK"))).unwrap();
        assert!(chk.contains("The analysis uncovered NO errors."), "{name}");
        let path =
            std::env::temp_dir().join(format!("vpt-back-{}-{name}.DAT;1", std::process::id()));
        std::fs::copy(back.join(format!("ours/{name}.DAT")), &path).unwrap();
        let attrs = std::fs::read_to_string(back.join("ours/ods-manifest.json")).unwrap();
        let e = serde_like::Manifest::of(attrs).entry(&format!("{name}.DAT"));
        let rfm = [Rfm::Udf, Rfm::Fix, Rfm::Var, Rfm::Vfc][e.rtype as usize & 15];
        let fab = Fab {
            org: vms_rms::Org::Idx,
            rfm,
            rat: e.rattrib,
            mrs: e.maxrec,
            lrl: e.rsize,
            bks: e.bktsize,
            ..Fab::default()
        };
        libvms::sys::set_xattr(&path, vms_rms::XATTR, fab.to_string().as_bytes()).unwrap();
        for (k, keyed) in body.split("@ key ").skip(1).enumerate() {
            let vms: Vec<&str> = keyed
                .lines()
                .skip(1)
                .filter(|l| !l.starts_with('%'))
                .collect();
            // READ/KEY=" "/MATCH=GE starts past keys below a blank (the
            // keys at 0 and 8 of IDX.FDL and IDXND.FDL).
            let at = [0, 8][k];
            let ours: Vec<String> = records(&path, k as u8)
                .iter()
                .filter(|r| r.get(at).is_none_or(|b| *b >= b' '))
                .map(|r| String::from_utf8_lossy(r).into_owned())
                .collect();
            assert_eq!(ours, vms, "{name} by key {k}");
        }
        let _ = std::fs::remove_file(&path);
    }
}

/// An indexed file's records in the order of `key` (none if it has no
/// such key).
fn records(path: &Path, key: u8) -> Vec<Vec<u8>> {
    let fab = libvms::files::fab(path);
    let b = libvms::rms::HostBlocks::new(std::fs::File::open(path).unwrap()).unwrap();
    let mut f = vms_rms::idx::File::new(b, fab.rfm == Rfm::Fix, fab.mrs);
    let mut out = Vec::new();
    let mut at = f.first(key);
    while let Ok(found) = at {
        out.push(found.record.clone());
        at = f.next(&found.at);
    }
    out
}

/// The orders VMS's sort gave equal keys (fixtures/sort/recorded/S*.txt),
/// from replacement selection over the tree size it reported.
#[test]
fn equal_keys_in_vms_order() {
    let gen_ = |n: usize, x: usize| -> Vec<String> {
        (0..n)
            .map(|i| format!("{i:05} {:02} {}", i % 7, "x".repeat(x)))
            .collect()
    };
    let key = |r: &String| r.as_bytes()[6..8].to_vec();
    let cases = [
        (10, 23, 34, "SG10"),
        (20, 23, 68, "SG20"),
        (34, 23, 92, "SG34"),
        (35, 23, 92, "SG35"),
        (100, 23, 244, "SG100"),
        (300, 23, 674, "SG300"),
        (1000, 23, 2140, "SG1K"),
        (100, 71, 216, "SH100"),
        (100, 1, 276, "SK100"),
        (3, 23, 34, "SG3"),
        (1, 23, 34, "SG1"),
    ];
    for (n, x, p, f) in cases {
        let recs = gen_(n, x);
        let order = vms_utils::sort::select(n, p, |a, b| key(&recs[a]).cmp(&key(&recs[b])));
        let got: Vec<&String> = order.iter().map(|&i| &recs[i]).collect();
        let want = std::fs::read_to_string(fixtures().join(format!("recorded/{f}.txt"))).unwrap();
        assert_eq!(got, want.lines().collect::<Vec<_>>(), "{f}");
    }
}

/// Just enough of ods-manifest.json, without a JSON library.
mod serde_like {
    use std::path::Path;

    pub struct Entry {
        pub bytes: usize,
        pub rtype: u8,
        pub rattrib: u8,
        pub rsize: u16,
        pub maxrec: u16,
        pub vfcsize: u8,
        pub bktsize: u8,
    }

    pub struct Manifest(String);

    pub fn read(p: &Path) -> Manifest {
        Manifest(std::fs::read_to_string(p).unwrap())
    }

    impl Manifest {
        pub fn of(text: String) -> Manifest {
            Manifest(text)
        }

        pub fn entry(&self, name: &str) -> Entry {
            let at = self
                .0
                .find(&format!("\"name\": \"{name}\""))
                .unwrap_or_else(|| panic!("{name}"));
            let start = self.0[..at].rfind('{').unwrap();
            let end = at + self.0[at..].find('}').unwrap();
            let obj = &self.0[start..end];
            let num = |k: &str| -> usize {
                let i = obj.find(&format!("\"{k}\":")).unwrap() + k.len() + 3;
                obj[i..]
                    .trim_start()
                    .split(|c: char| !c.is_ascii_digit())
                    .next()
                    .unwrap()
                    .parse()
                    .unwrap()
            };
            Entry {
                bytes: num("bytes"),
                rtype: num("rtype") as u8,
                rattrib: num("rattrib") as u8,
                rsize: num("rsize") as u16,
                maxrec: num("maxrec") as u16,
                vfcsize: num("vfcsize") as u8,
                bktsize: num("bktsize") as u8,
            }
        }
    }
}
