//! RMS and the lock manager from C (examples/rms): programs written as for
//! VMS, built against $VMSPORT/include and libvms, run from a Unix shell.

use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    std::fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")).unwrap()
}

/// target/debug: where this test, vmsportd and libvms.dylib are.
fn target() -> PathBuf {
    std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

/// Builds examples/rms/NAME.c and runs it in `dir`: its output.
pub fn run_example(name: &str, dir: &Path, run: &Path) -> String {
    let status = Command::new(env!("CARGO"))
        .current_dir(root())
        .args(["build", "-q", "-p", "vmsportd", "-p", "vms-c"])
        .status()
        .unwrap();
    assert!(status.success());
    let t = target();
    let exe = dir.join(name);
    let cc = Command::new("cc")
        .args(["-Wall", "-Werror", "-I"])
        .arg(root().join("include"))
        .arg(root().join(format!("examples/rms/{name}.c")))
        .arg("-L")
        .arg(&t)
        .arg("-lvms")
        .arg(format!("-Wl,-rpath,{}", t.display()))
        .arg("-o")
        .arg(&exe)
        .output()
        .unwrap();
    assert!(
        cc.status.success(),
        "{}",
        String::from_utf8_lossy(&cc.stderr)
    );
    let out = Command::new(&exe)
        .current_dir(dir)
        .env("VMSPORT_RUN", run)
        .env("VMSPORTD", t.join("vmsportd"))
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr)
}

fn stop(run: &Path) {
    if let Ok(c) = vmsportd::Client::connect_in(run, Path::new("/nonexistent")) {
        let _ = c.stop();
    }
    let _ = std::fs::remove_dir_all(run);
}

#[test]
fn lock_manager_from_c() {
    let dir = std::env::temp_dir().join(format!("vpt-lockdemo-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let run = PathBuf::from(format!("/tmp/vpt-lk{}", std::process::id()));
    let got = run_example("lockdemo", &dir, &run);
    stop(&run);
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        got,
        "parent EX: 00000001 lksb 0001
parent EX again, NOQUEUE: 000009B8 (SS$_NOTQUEUED 000009B8)
child EX, NOQUEUE: 000009B8
child EX, waited: 00000001 lksb 0001
parent converted to NL: 00000001; child exited 0
parent back to EX, NOQUEUE SYNCSTS: 00000689
value block: 00000014
deq: 00000001
deq again: 00002124
deq all: 00000001
"
    );
}

#[test]
fn rms_from_c() {
    let dir = std::env::temp_dir().join(format!("vpt-rmsdemo-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let run = PathBuf::from(format!("/tmp/vpt-rd{}", std::process::id()));
    let got = run_example("rmsdemo", &dir, &run);
    stop(&run);
    assert_eq!(got, RMSDEMO);
    let files: Vec<(&str, Vec<u8>, String)> = [
        ("DEMO.REL", "DEMO.REL;1"),
        ("DEMO.IDX", "DEMO.IDX;1"),
        ("DEMO.SEQ", "DEMO.SEQ;1"),
    ]
    .iter()
    .map(|(name, host)| {
        let p = dir.join(host);
        (*name, std::fs::read(&p).unwrap(), entry(name, &p))
    })
    .collect();
    // What vmsport reads in them, for VMS's DCL READ to agree with.
    let reads: Vec<Vec<String>> = files
        .iter()
        .map(|(name, _, _)| records(&dir.join(format!("{name};1"))))
        .collect();
    let _ = std::fs::remove_dir_all(&dir);
    let manifest = format!(
        "{{\"volume\": \"VPTIN\", \"structure_level\": 5, \"root\": \"[T]\", \"entries\": [\n{}\n]}}\n",
        files
            .iter()
            .map(|f| f.2.clone())
            .collect::<Vec<_>>()
            .join(",\n")
    );
    let ours = root().join("fixtures/cabiback/ours");
    if std::env::var_os("VMSPORT_WRITE_FIXTURES").is_some() {
        std::fs::create_dir_all(&ours).unwrap();
        for (name, b, _) in &files {
            std::fs::write(ours.join(name), b).unwrap();
        }
        std::fs::write(ours.join("ods-manifest.json"), manifest).unwrap();
        return;
    }
    // What we wrote is what VMS checked and read.
    assert_eq!(
        std::fs::read_to_string(ours.join("ods-manifest.json")).unwrap(),
        manifest
    );
    for (name, b, _) in &files {
        assert!(std::fs::read(ours.join(name)).unwrap() == *b, "{name}");
    }
    let log =
        std::fs::read_to_string(root().join("fixtures/cabiback/recorded/CABIBACK.log")).unwrap();
    for (name, _, _) in &files {
        let check = log
            .split("@@ ")
            .find(|b| b.starts_with(&format!("check {name}")))
            .unwrap();
        assert!(
            check.contains("The analysis uncovered NO errors."),
            "{check}"
        );
    }
    let read = |name: &str| -> Vec<String> {
        let b = log
            .split("@@ ")
            .find(|b| b.starts_with(&format!("read {name}")))
            .unwrap();
        b.lines()
            .filter_map(|l| Some(l.strip_prefix('[')?.strip_suffix(']')?.to_string()))
            .collect()
    };
    for ((name, _, _), ours) in files.iter().zip(&reads) {
        assert_eq!(&read(name), ours, "{name}");
    }
    assert_eq!(reads[0], ["first", "SECOND", "third"]);
    assert_eq!(reads[2], ["one line", "and another"]);
}

/// A file's ods-manifest.json entry: its record attributes from vms.fab,
/// fixed dates and owner, for `ods import`.
fn entry(name: &str, path: &Path) -> String {
    let f = libvms::files::fab(path);
    // 7-OCT-2026 00:28:23.27, as in fixtures/rmsback.
    let t = 52980497032782765u64;
    format!(
        "  {{\"path\": \"{name}\", \"directory\": false, \"name\": \"{name}\", \"version\": 1, \
         \"bytes\": {}, \"rtype\": {}, \"rattrib\": {}, \"rsize\": {}, \"bktsize\": {}, \
         \"vfcsize\": {}, \"maxrec\": {}, \"defext\": {}, \"gbc\": 0, \
         \"reserved\": [0, 0, 0, 0, 0, 0, 0, 0], \"version_limit\": 0, \"filechar\": 0, \
         \"owner\": 65540, \"protection\": 64000, \"revision\": 1, \"created\": {t}, \
         \"revised\": {t}, \"expires\": 0, \"backup\": 0, \"accessed\": {t}, \"attr_changed\": {t}}}",
        std::fs::metadata(path).unwrap().len(),
        (f.org as u8) << 4 | f.rfm as u8,
        f.rat,
        f.lrl,
        f.bks,
        f.fsz,
        f.mrs,
        f.deq
    )
}

/// What rmsdemo says.
const RMSDEMO: &str = "open, bad FAB          0001850C\ncreate relative        00010001\nconnect                00010001\nput                    00010001\nput                    00010001\nput                    00010001\nput key 10             00010001\nput key 10 again       000182A2\nput key 10 UIF         00010001\nclose                  00010001\nopen relative          00010001\n  org 10 rfm 2 mrs 40 mrn 100 bks 1 lrl 40 ebk 17\nconnect                00010001\nget                    00010001\n  [first] rfa 1.0\nget                    00010001\n  [second] rfa 2.0\nget                    00010001\n  [third] rfa 3.0\nget                    00010001\n  [tenth, again] rfa 10.0\nget                    0001827A\nget key 2              00010001\n  [second] rfa 2.0\nupdate                 00010001\nget key GE 4           00010001\n  [tenth, again] rfa 10.0\ndelete                 00010001\ndelete again           000184B4\nget key 4              000182B2\nget rfa 2              00010001\n  [SECOND] rfa 2.0\nget, short buffer      000181A8\n  [SEC] rfa 2.0\nrewind                 00010001\nget                    00010001\n  [first] rfa 1.0\nget                    00010001\n  [SECOND] rfa 2.0\nget                    00010001\n  [third] rfa 3.0\nget                    0001827A\ndisconnect             00010001\nget, disconnected      00018584\nclose                  00010001\nclose again            00018564\ncreate indexed         00010001\nconnect                00010001\nput 100 records\nput duplicate ID       000184EC\nput duplicate name     00018011\nclose                  00010001\nopen indexed           00010001\n  org 20 rfm 1 mrs 30 bks 2 nok 2 noa 2 pvn 3\n  key 1 [BY_NAME] pos 4 siz 10 flg 83 dtp 0 dan 1 ian 1 dbs 1 ibs 1 tks 10\nconnect                00010001\nget                    00010001\n  [0001BANANA    a note          ] rfa 4.44\nget                    00010001\n  [0002GRAPE     a note          ] rfa 4.35\nget                    00010001\n  [0003ELDER     a note          ] rfa 4.11\n  101 records by ID\nget key 0050           00010001\n  [0050BANANA    a note          ] rfa 4.15\nget key GT 0100        00010001\nget key GE 0100        00010001\n  [0100CHERRY    a note          ] rfa 8.16\nget key 005 (generic)  00010001\n  [0050BANANA    a note          ] rfa 4.15\nget name CH (generic)  00010001\n  [0074CHERRY    a note          ] rfa 4.2\nget next by name       00010001\n  [0030CHERRY    a note          ] rfa 4.9\nget name GT DATE       00010001\n  [0047ELDER     a note          ] rfa 4.4\nget name ZEBRA         000182B2\nget key 5              0001859C\nget rfa of 0050        00010001\n  [0050BANANA    a note          ] rfa 4.15\nupdate name            00010001\nget rfa of 0050        00010001\nupdate ID              0001849C\nget name ZEBRA         00010001\n  [0050ZEBRA     renamed         ] rfa 4.15\nfind 0001              00010001\ndelete                 00010001\nget 0001               000182B2\nrewind                 00010001\nget first              00010001\n  [0002GRAPE     a note          ] rfa 4.35\nclose                  00010001\ncreate sequential      00010001\nconnect                00010001\nput                    00010001\nput                    00010001\nclose                  00010001\nopen sequential        00010001\n  org 00 rfm 2 rat 2\nconnect                00010001\nget                    00010001\n  [one line] rfa 1.0\nget                    00010001\n  [and another] rfa 2.0\nget                    0001827A\nclose                  00010001\ncreate again           00010001\nclose                  00010001\ncreate SUP             00010631\nclose                  00010001\ncreate CIF             00010001\nclose                  00010001\ncreate CIF, new        00010619\nclose                  00010001\nerase                  00010001\nerase again            00018292\nparse                  00010001\n  fnb 0000011F name [DEMO.*;*]\nsearch                 00010001\n  DEMO.IDX;1\nsearch                 00010001\n  DEMO.REL;1\nsearch                 00010001\n  DEMO.SEQ;2\nsearch                 00010001\n  DEMO.SEQ;1\nsearch                 000182CA\nparse                  00010001\nsearch                 00018292\n";

/// A file's records in order (an indexed file's by its primary key).
fn records(path: &Path) -> Vec<String> {
    use vms_rms::{Org, Rfm, idx, rel::Rel};
    let fab = libvms::files::fab(path);
    let mut b = std::fs::read(path).unwrap();
    let data: Vec<Vec<u8>> = match fab.org {
        Org::Seq => vms_rms::decode(&fab, &b)
            .unwrap()
            .into_iter()
            .map(|r| r.data)
            .collect(),
        Org::Rel => {
            let rel = Rel::new(fab).unwrap();
            let (mut n, mut out) = (0, Vec::new());
            while let Ok((m, r)) = rel.next(&mut b, n) {
                out.push(r.data);
                n = m;
            }
            out
        }
        Org::Idx => {
            let mut f = idx::File::new(b, fab.rfm == Rfm::Fix, fab.mrs);
            let mut out = Vec::new();
            let mut cur = f.first(0);
            while let Ok(found) = cur {
                out.push(found.record.clone());
                cur = f.next(&found.at);
            }
            out
        }
    };
    data.into_iter()
        .map(|d| String::from_utf8(d).unwrap())
        .collect()
}
