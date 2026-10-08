//! The utilities against OpenVMS: fixtures/utils/UTILS.COM run through dcl
//! as fixtures/vms/record.py ran it (DKA200 a device, the cases in
//! DKA200:[T.UTILS.W]), compared case by case with recorded/utils.log.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/utils")
}

/// Cases that can't match, and why.
const SKIP: &[(&str, &str)] = &[
    // Host file IDs, owners, protections and allocation; DCL's OPEN/WRITE
    // makes VFC files on VMS and stream_LF ones here.
    ("DIRECTORY/FULL B.DAT", "file system details"),
    // Counts the blocks of UTILS.COM and UTILS.LOG, which differ.
    (
        "DIRECTORY/SIZE/GRAND_TOTAL [-],[]",
        "sizes of the procedure and its log",
    ),
    // VMS said nothing and returned success for a missing output directory.
    (
        "COPY/LOG C.LIS [.NOSUCH]",
        "VMS ignored a missing output directory",
    ),
];

/// Dates (` 7-OCT-2026 01:07:02.41`) and the blocks DELETE and PURGE report
/// (allocation: VMS clusters versus host blocks) are masked.
fn mask(s: &str) -> String {
    s.lines().map(mask_line).collect::<Vec<_>>().join("\n")
}

fn mask_line(l: &str) -> String {
    let mut out = String::new();
    let cs: Vec<char> = l.chars().collect();
    let mut i = 0;
    let shape = "Dd-MMM-dddd dd:dd:dd.dd";
    while i < cs.len() {
        let fits = i + shape.len() <= cs.len()
            && shape.chars().zip(&cs[i..]).all(|(p, &c)| match p {
                'D' => c == ' ' || c.is_ascii_digit(),
                'd' => c.is_ascii_digit(),
                'M' => c.is_ascii_uppercase(),
                p => p == c,
            });
        if fits {
            out += "<date-time>            ";
            i += shape.len();
        } else {
            out.push(cs[i]);
            i += 1;
        }
    }
    if let Some(i) = out.rfind(" deleted (") {
        out.truncate(i);
        out += " deleted (n blocks)";
    }
    out
}

fn cases(log: &str) -> Vec<(String, String)> {
    log.split("@@ ")
        .skip(1)
        .map(|b| {
            let (head, body) = b.split_once('\n').unwrap_or((b, ""));
            (head.to_string(), body.to_string())
        })
        .collect()
}

#[test]
fn recorded_utilities() {
    for p in ["vms-dcl", "vmsportd"] {
        let st = Command::new(env!("CARGO"))
            .args(["build", "-q", "-p", p])
            .status()
            .unwrap();
        assert!(st.success());
    }
    let dcl = Path::new(env!("CARGO_BIN_EXE_directory")).with_file_name("dcl");
    let tmp = std::env::temp_dir().join(format!("vpt-utils-{}", std::process::id()));
    let dir = tmp.join("T/UTILS");
    std::fs::create_dir_all(dir.join("W")).unwrap();
    std::fs::copy(fixtures().join("UTILS.COM"), dir.join("UTILS.COM")).unwrap();
    std::fs::write(
        tmp.join("T/RECORD.COM"),
        "$ SET NOON\n$ SET DEFAULT DKA200:[T.UTILS]\n$ @UTILS.COM/OUTPUT=UTILS.LOG\n",
    )
    .unwrap();

    // A short run directory: socket paths are limited to about 100 bytes.
    let run = PathBuf::from(format!("/tmp/vpt-u{}", std::process::id()));
    let root = vmsportd::host_dir(&tmp, true);
    let out = Command::new(&dcl)
        .args([
            "-c",
            &format!("DEFINE/TRANSLATION=CONCEALED DKA200 {root}"),
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
    print!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let want = std::fs::read_to_string(fixtures().join("recorded/utils.log")).unwrap();
    let got = std::fs::read_to_string(dir.join("UTILS.LOG;1")).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&tmp);
    let got = cases(&got);
    let mut failures = Vec::new();
    let mut matched = 0;
    for (i, (case, body)) in cases(&want).into_iter().enumerate() {
        if SKIP.iter().any(|s| s.0 == case) {
            continue;
        }
        let mine = got
            .get(i)
            .filter(|g| g.0 == case)
            .map_or("<missing>\n".into(), |g| g.1.clone());
        if mask(&body) == mask(&mine) {
            matched += 1;
        } else {
            failures.push(format!("@@ {case}\n--- VMS\n{body}--- vmsport\n{mine}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{matched} match, {} differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
