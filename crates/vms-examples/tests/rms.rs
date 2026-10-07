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
    let files: Vec<(&str, Vec<u8>, String)> =
        [("DEMO.REL", "DEMO.REL;1"), ("DEMO.SEQ", "DEMO.SEQ;1")]
            .iter()
            .map(|(name, host)| {
                let p = dir.join(host);
                (*name, std::fs::read(&p).unwrap(), entry(name, &p))
            })
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
    for name in ["DEMO.REL", "DEMO.SEQ"] {
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
    assert_eq!(read("DEMO.REL"), ["first", "SECOND", "third"]);
    assert_eq!(read("DEMO.SEQ"), ["one line", "and another"]);
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
const RMSDEMO: &str = "open, bad FAB          0001850C
create relative        00010001
connect                00010001
put                    00010001
put                    00010001
put                    00010001
put key 10             00010001
put key 10 again       000182A2
put key 10 UIF         00010001
close                  00010001
open relative          00010001
  org 10 rfm 2 mrs 40 mrn 100 bks 1 lrl 40 ebk 3
connect                00010001
get                    00010001
  [first] rfa 1.0
get                    00010001
  [second] rfa 2.0
get                    00010001
  [third] rfa 3.0
get                    00010001
  [tenth, again] rfa 10.0
get                    0001827A
get key 2              00010001
  [second] rfa 2.0
update                 00010001
get key GE 4           00010001
  [tenth, again] rfa 10.0
delete                 00010001
delete again           000184B4
get key 4              000182B2
get rfa 2              00010001
  [SECOND] rfa 2.0
get, short buffer      000181A8
  [SEC] rfa 2.0
rewind                 00010001
get                    00010001
  [first] rfa 1.0
get                    00010001
  [SECOND] rfa 2.0
get                    00010001
  [third] rfa 3.0
get                    0001827A
disconnect             00010001
get, disconnected      00018584
close                  00010001
close again            00018564
create sequential      00010001
connect                00010001
put                    00010001
put                    00010001
close                  00010001
open sequential        00010001
  org 00 rfm 2 rat 2
connect                00010001
get                    00010001
  [one line] rfa 1.0
get                    00010001
  [and another] rfa 2.0
get                    0001827A
close                  00010001
create again           00010001
close                  00010001
create SUP             00010631
close                  00010001
create CIF             00010001
close                  00010001
create CIF, new        00010619
close                  00010001
erase                  00010001
erase again            00018292
parse                  00010001
  fnb 0000011F name [DEMO.*;*]
search                 00010001
  DEMO.REL;1
search                 00010001
  DEMO.SEQ;2
search                 00010001
  DEMO.SEQ;1
search                 000182CA
parse                  00010001
search                 00018292
";
