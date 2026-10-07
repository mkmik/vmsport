//! EVE through DCL: EDIT/TPU/NODISPLAY with an initialization file edits
//! a file from a procedure; plain EDIT takes keys (here piped, no
//! terminal) and saves a new version; /JOURNAL keeps the keys and
//! /RECOVER types them again.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn dcl(dir: &Path, run: &Path, cmd: &str, keys: &[u8]) -> String {
    use std::io::Write;
    let dcl = Path::new(env!("CARGO_BIN_EXE_tpu")).with_file_name("dcl");
    let mut c = Command::new(dcl)
        .args(["-c", cmd])
        .current_dir(dir)
        .env("VMSPORT_RUN", run)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    c.stdin.take().unwrap().write_all(keys).unwrap();
    String::from_utf8_lossy(&c.wait_with_output().unwrap().stdout).into_owned()
}

#[test]
fn eve_through_dcl() {
    for p in ["vms-dcl", "vmsportd"] {
        let st = Command::new(env!("CARGO"))
            .args(["build", "-q", "-p", p])
            .status()
            .unwrap();
        assert!(st.success());
    }
    let tmp = std::env::temp_dir().join(format!("vpt-eve-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(tmp.join("T.TXT"), "line one\nline two\nthe third line\n").unwrap();
    std::fs::write(
        tmp.join("I.EVE"),
        "top\nfind \"two\"\nerase line\nwrite file\nexit\n",
    )
    .unwrap();
    let run = PathBuf::from(format!("/tmp/vpt-ev{}", std::process::id()));
    let read = |n: &str| std::fs::read_to_string(tmp.join(n)).unwrap();

    // As VMS did it (fixtures/eve/recorded/batch.log, init_exit).
    let out = dcl(
        &tmp,
        &run,
        "EDIT/TPU/NODISPLAY/INITIALIZATION=I.EVE T.TXT",
        b"",
    );
    let said: Vec<&str> = out
        .lines()
        .map(|l| l.split(" HOST:").next().unwrap())
        .collect();
    assert_eq!(
        said,
        [
            "3 lines read from file",
            "Executing commands in initialization file:",
            "You are already at the top of the buffer.",
            "",
            "2 lines written to file"
        ]
    );
    assert_eq!(read("T.TXT;2"), "line one\nline the third line\n");

    // Keys typed: text, a move, Ctrl/Z.
    dcl(&tmp, &run, "EDIT T.TXT", b"Hello\x1bOBWorld\x1a");
    assert_eq!(read("T.TXT;3"), "Helloline one\nline Worldthe third line\n");

    // A session that never ended: its journal brings its keys back.
    dcl(&tmp, &run, "EDIT/JOURNAL T.TXT", b"XY");
    assert_eq!(read("T.TJL;1"), "XY");
    assert!(!tmp.join("T.TXT;4").exists());
    dcl(&tmp, &run, "EDIT/RECOVER T.TXT", b"Z\x1a");
    assert_eq!(
        read("T.TXT;4"),
        "XYZHelloline one\nline Worldthe third line\n"
    );
    assert!(!tmp.join("T.TJL;1").exists());

    if let Ok(c) = vmsportd::Client::connect_in(&run, Path::new("/nonexistent")) {
        let _ = c.stop();
    }
    let _ = std::fs::remove_dir_all(&run);
    let _ = std::fs::remove_dir_all(&tmp);
}
