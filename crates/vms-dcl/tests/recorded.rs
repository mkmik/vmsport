//! M1's acceptance test: the DCL procedures in fixtures/dcl give the output
//! OpenVMS gave (fixtures/dcl/recorded).
//!
//! It runs them as fixtures/vms/record.py did: DKA200 is a device (here a
//! concealed rooted logical over a temporary directory), and a top-level
//! procedure runs each one with /OUTPUT from DKA200:[T.DCL].

use std::path::{Path, PathBuf};
use std::process::Command;

const PROCS: [&str; 4] = ["SYMBOLS", "LEXICALS", "CONTROL", "ERRORS"];

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/dcl")
}

/// vmsportd, built next to dcl, in a run directory of our own.
fn daemon_env(cmd: &mut Command, run: &Path) {
    let status = Command::new(env!("CARGO"))
        .args(["build", "-q", "-p", "vmsportd"])
        .status()
        .unwrap();
    assert!(status.success());
    cmd.env("VMSPORT_RUN", run)
        .env("VMSPORT_JOB", format!("{:X}", std::process::id()));
}

#[test]
fn recorded_procedures() {
    let tmp = std::env::temp_dir().join(format!("vpt-dcl-{}", std::process::id()));
    let dir = tmp.join("T/DCL");
    std::fs::create_dir_all(&dir).unwrap();
    for f in std::fs::read_dir(fixtures()).unwrap().flatten() {
        if f.path().extension().is_some_and(|e| e == "COM") {
            std::fs::copy(f.path(), dir.join(f.file_name())).unwrap();
        }
    }
    let mut record = String::from("$ SET NOON\n$ SET DEFAULT DKA200:[T.DCL]\n");
    for p in PROCS {
        record += &format!("$ @{p}.COM/OUTPUT={p}.LOG\n");
    }
    std::fs::write(tmp.join("T/RECORD.COM"), record).unwrap();

    // A short run directory: socket paths are limited to about 100 bytes.
    let run = PathBuf::from(format!("/tmp/vpt-t{}", std::process::id()));
    let root = vmsportd::host_dir(&tmp, true);
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_dcl"));
    // DKA100 was the input volume; LEXICALS parses names on it.
    std::fs::create_dir_all(tmp.join("DKA100")).unwrap();
    let dka100 = vmsportd::host_dir(&tmp.join("DKA100"), true);
    cmd.args([
        "-c",
        &format!("DEFINE/TRANSLATION=CONCEALED DKA200 {root}"),
        &format!("DEFINE/TRANSLATION=CONCEALED DKA100 {dka100}"),
        "@DKA200:[T]RECORD.COM",
    ]);
    daemon_env(&mut cmd, &run);
    let out = cmd.output().unwrap();
    // Stop the daemon.
    if let Ok(c) = vmsportd::Client::connect_in(&run, Path::new("/nonexistent")) {
        let _ = c.stop();
    }
    let _ = std::fs::remove_dir_all(&run);
    println!("{}", String::from_utf8_lossy(&out.stdout));

    let mut failures = Vec::new();
    for p in PROCS {
        let want = std::fs::read_to_string(fixtures().join(format!("recorded/{p}.log"))).unwrap();
        let got = std::fs::read_to_string(dir.join(format!("{p}.LOG;1")))
            .unwrap_or_else(|e| format!("<{e}>"));
        if got != want {
            failures.push(format!("{p}:\n{}", diff(&want, &got)));
        }
    }
    let _ = std::fs::remove_dir_all(&tmp);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Line differences, VMS first.
fn diff(want: &str, got: &str) -> String {
    let (w, g): (Vec<_>, Vec<_>) = (want.lines().collect(), got.lines().collect());
    let mut out = String::new();
    for i in 0..w.len().max(g.len()) {
        let (a, b) = (
            w.get(i).copied().unwrap_or("<none>"),
            g.get(i).copied().unwrap_or("<none>"),
        );
        if a != b {
            out += &format!("  {i:3} VMS     {a}\n  {i:3} vmsport {b}\n");
        }
    }
    out
}
