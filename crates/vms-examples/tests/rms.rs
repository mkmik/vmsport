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
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(got, RMSDEMO);
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
