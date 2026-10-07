//! DCL procedures recorded on OpenVMS give the same output here: M1's
//! acceptance test (fixtures/dcl) and SPAWN and PIPE (fixtures/spawn).
//!
//! They run as fixtures/vms/record.py ran them: DKA200 is a device (here a
//! concealed rooted logical over a temporary directory), and a top-level
//! procedure runs each one with /OUTPUT from DKA200:[T.AREA].

use std::path::{Path, PathBuf};
use std::process::Command;

fn fixtures(area: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(area)
}

/// Runs `procs` of fixtures/AREA and returns the differences from what VMS
/// gave, after `mask` on both sides.
fn run_area(area: &str, procs: &[&str], mask: fn(&str) -> String) -> Vec<String> {
    let tag = format!("{}{}", &area[..1], std::process::id());
    let tmp = std::env::temp_dir().join(format!("vpt-{area}-{}", std::process::id()));
    let dir = tmp.join("T").join(area.to_uppercase());
    std::fs::create_dir_all(&dir).unwrap();
    for f in std::fs::read_dir(fixtures(area)).unwrap().flatten() {
        if f.path().extension().is_some_and(|e| e == "COM") {
            std::fs::copy(f.path(), dir.join(f.file_name())).unwrap();
        }
    }
    let mut record = format!(
        "$ SET NOON\n$ SET DEFAULT DKA200:[T.{}]\n",
        area.to_uppercase()
    );
    for p in procs {
        record += &format!("$ @{p}.COM/OUTPUT={p}.LOG\n");
    }
    std::fs::write(tmp.join("T/RECORD.COM"), record).unwrap();

    // vmsportd and the utilities, built next to dcl.
    let status = Command::new(env!("CARGO"))
        .args(["build", "-q", "-p", "vmsportd", "-p", "vms-utils"])
        .status()
        .unwrap();
    assert!(status.success());
    // A short run directory: socket paths are limited to about 100 bytes.
    let run = PathBuf::from(format!("/tmp/vpt-{tag}"));
    let root = vmsportd::host_dir(&tmp, true);
    // DKA100 was the input volume; LEXICALS parses names on it.
    std::fs::create_dir_all(tmp.join("DKA100")).unwrap();
    let dka100 = vmsportd::host_dir(&tmp.join("DKA100"), true);
    let out = Command::new(env!("CARGO_BIN_EXE_dcl"))
        .args([
            "-c",
            &format!("DEFINE/TRANSLATION=CONCEALED DKA200 {root}"),
            &format!("DEFINE/TRANSLATION=CONCEALED DKA100 {dka100}"),
            "@DKA200:[T]RECORD.COM",
        ])
        .env("VMSPORT_RUN", &run)
        .env("VMSPORT_JOB", format!("{:X}", std::process::id()))
        .output()
        .unwrap();
    if let Ok(c) = vmsportd::Client::connect_in(&run, Path::new("/nonexistent")) {
        let _ = c.stop();
    }
    let _ = std::fs::remove_dir_all(&run);
    println!("{}", String::from_utf8_lossy(&out.stdout));

    let mut failures = Vec::new();
    for p in procs {
        let want =
            std::fs::read_to_string(fixtures(area).join(format!("recorded/{p}.log"))).unwrap();
        let got = std::fs::read_to_string(dir.join(format!("{p}.LOG;1")))
            .unwrap_or_else(|e| format!("<{e}>"));
        let (want, got) = (mask(&want), mask(&got));
        if got != want {
            failures.push(format!("{p}:\n{}", diff(&want, &got)));
        }
    }
    let _ = std::fs::remove_dir_all(&tmp);
    failures
}

#[test]
fn recorded_procedures() {
    let procs = [
        "SYMBOLS", "LEXICALS", "CONTROL", "ERRORS", "FILES", "BLOCKS", "VERIFY", "LOGICALS",
        "SUBST",
    ];
    let failures = run_area("dcl", &procs, str::to_string);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Process names differ from run to run (SYSTEM_38089 on VMS).
fn mask_processes(s: &str) -> String {
    s.lines()
        .map(|l| match l.find("process ") {
            Some(i) if l.starts_with("%DCL-S-") => {
                let rest = &l[i + 8..];
                let end = rest.find(' ').unwrap_or(rest.len());
                format!("{}process *{}\n", &l[..i], &rest[end..])
            }
            _ => format!("{l}\n"),
        })
        .collect()
}

#[test]
fn recorded_spawn_and_pipe() {
    let failures = run_area("spawn", &["SPAWN", "PIPE"], mask_processes);
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

/// DIRECTORY/FULL's dates, file IDs and owners are the run's.
fn mask_full(s: &str) -> String {
    s.lines()
        .map(|l| {
            let dated = ["Created:", "Modified:", "Accessed:", "Attr Mod:", "Data Mod:"];
            match () {
                _ if dated.iter().any(|d| l.starts_with(d)) => format!("{}\n", &l[..10]),
                _ if l.contains("File ID:") => format!("{}\n", &l[..l.find("File ID:").unwrap()]),
                _ if l.starts_with("Size:") => format!("{}\n", &l[..l.find("Owner:").unwrap_or(l.len())]),
                _ => format!("{l}\n"),
            }
        })
        .collect()
}

#[test]
#[ignore = "needs CREATE/FDL and indexed files"]
fn recorded_keyed_io() {
    let failures = run_area("dclrms", &["DCLRMS"], mask_full);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
