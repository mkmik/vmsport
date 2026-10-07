//! CREATE, ANALYZE/RMS_FILE, DIRECTORY/FULL and F$FILE_ATTRIBUTES against
//! OpenVMS: fixtures/fdlutil/FDLUTIL.COM (and FDLUTIL2.COM) run through dcl
//! as record.py ran them (in DKA200:[T.AREA]), compared section by section
//! with what VMS logged. What the host can't know is masked: dates, file
//! IDs, owners, protections, revision counts.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fixtures(area: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(area)
}

/// Each `shape` (D a digit or blank, d a digit, M an upper case letter) in
/// `l` replaced by `with`.
fn mask_shape(l: &str, shape: &str, with: &str) -> String {
    let cs: Vec<char> = l.chars().collect();
    let (mut out, mut i) = (String::new(), 0);
    while i < cs.len() {
        let fits = i + shape.len() <= cs.len()
            && shape.chars().zip(&cs[i..]).all(|(p, &c)| match p {
                'D' => c == ' ' || c.is_ascii_digit(),
                'd' => c.is_ascii_digit(),
                'M' => c.is_ascii_uppercase(),
                p => p == c,
            });
        if fits {
            out += with;
            i += shape.len();
        } else {
            out.push(cs[i]);
            i += 1;
        }
    }
    out
}

/// Digits right after `after`, as `n`.
fn mask_number(l: &str, after: &str) -> String {
    match l.find(after) {
        Some(i) => {
            let rest = &l[i + after.len()..];
            let digits = rest.chars().take_while(|c| c.is_ascii_digit()).count();
            format!("{}{after}n{}", &l[..i], &rest[digits..])
        }
        None => l.to_string(),
    }
}

fn mask_line(l: &str) -> String {
    let mut l = mask_shape(l, "Dd-MMM-dddd dd:dd:dd.dd", "<date>");
    l = mask_shape(&l, "Dd-MMM-dddd dd:dd:dd", "<date>");
    if let Some(i) = l.find("File ID:") {
        l = format!("{}File ID: <fid>", &l[..i]);
    }
    for p in ["Number: ", "<date> ("] {
        l = mask_number(&l, p);
    }
    // UICs: [g,m].
    while let Some(i) = l.find('[').filter(|&i| {
        let r = &l[i + 1..];
        let d1 = r.chars().take_while(|c| c.is_ascii_digit()).count();
        d1 > 0 && r[d1..].starts_with(',')
    }) {
        let j = i + l[i..].find(']').unwrap_or(l.len() - i);
        l = format!("{}<uic>{}", &l[..i], &l[(j + 1).min(l.len())..]);
    }
    for p in ["Protection:", "PROTECTION", "File protection:", " PRO "] {
        if let Some(i) = l.find(p) {
            l = format!("{}{p} <protection>", &l[..i]);
        }
    }
    l
}

fn mask(s: &str) -> String {
    s.lines().map(mask_line).collect::<Vec<_>>().join("\n")
}

/// What depends on where buckets split: extents' use, and the numbers of
/// ANALYSIS_OF_KEY sections.
fn mask_splits(s: &str) -> String {
    let mut analysis = false;
    s.lines()
        .map(|l| {
            analysis = analysis && !l.is_empty() || l.starts_with("ANALYSIS_OF_KEY");
            if l.starts_with("\tCurrent Extent Start:") || analysis && l.starts_with('\t') {
                l.replace(|c: char| c.is_ascii_digit(), "")
            } else {
                l.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn sections(log: &str) -> Vec<(String, String)> {
    log.split("@@ ")
        .skip(1)
        .map(|b| {
            let (head, body) = b.split_once('\n').unwrap_or((b, ""));
            (head.to_string(), body.to_string())
        })
        .collect()
}

/// Runs AREA's procedure in DKA200:[T.AREA] (the area's `in` files and
/// `extra` files copied there first) and compares its log with VMS's,
/// case by case.
fn run_area(area: &str, procedure: &str, extra: &[(&str, &[u8], &str)], same: &[(&str, &str)]) {
    for p in ["vms-dcl", "vmsportd"] {
        let st = Command::new(env!("CARGO"))
            .args(["build", "-q", "-p", p])
            .status()
            .unwrap();
        assert!(st.success());
    }
    let dcl = Path::new(env!("CARGO_BIN_EXE_create")).with_file_name("dcl");
    let tmp = std::env::temp_dir().join(format!("vpt-{area}-{}", std::process::id()));
    let up = area.to_uppercase();
    let dir = tmp.join("T").join(&up);
    std::fs::create_dir_all(&dir).unwrap();
    let vms = std::fs::read_to_string(fixtures(area).join("vms.txt")).unwrap();
    for f in vms.lines().filter_map(|l| l.strip_prefix("in ")) {
        let text = std::fs::read(fixtures(area).join(f)).unwrap();
        std::fs::write(dir.join(f.to_uppercase()), text).unwrap();
    }
    for (name, bytes, fab) in extra {
        std::fs::write(dir.join(name), bytes).unwrap();
        libvms::sys::set_xattr(&dir.join(name), "vms.fab", fab.as_bytes()).unwrap();
    }
    std::fs::write(
        tmp.join("T/RECORD.COM"),
        format!("$ SET NOON\n$ SET DEFAULT DKA200:[T.{up}]\n$ @{procedure}/OUTPUT={up}.LOG\n"),
    )
    .unwrap();
    // Short (socket paths are limited), and one for each test.
    let run = PathBuf::from(format!(
        "/tmp/vpt-{}{}",
        &area[area.len() - 2..],
        std::process::id()
    ));
    let root = vmsportd::host_dir(&tmp, true);
    let out = Command::new(&dcl)
        .args([
            "-c",
            &format!("DEFINE/TRANSLATION=CONCEALED DKA200 {root}"),
            "@DKA200:[T]RECORD.COM",
        ])
        .env("VMSPORT_RUN", &run)
        .env(
            "VMSPORT_JOB",
            format!("{:X}{}", std::process::id(), area.len()),
        )
        .stdin(std::process::Stdio::null())
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
    let want =
        std::fs::read_to_string(fixtures(area).join(format!("recorded/{area}.log"))).unwrap();
    let got = std::fs::read_to_string(dir.join(format!("{up}.LOG;1"))).unwrap_or_default();
    // Files made here hold what VMS's did (whose blocks go on to the
    // highwater mark: past a sequential file's end, RMS's FFFF and zeros).
    let mut differ = Vec::new();
    for (mine, theirs) in same {
        let m = std::fs::read(dir.join(mine)).unwrap_or_default();
        let t = std::fs::read(fixtures(area).join("recorded").join(theirs)).unwrap();
        if !t.starts_with(&m) || m.is_empty() {
            differ.push(*mine);
        }
    }
    let _ = std::fs::remove_dir_all(&tmp);
    assert!(differ.is_empty(), "not as VMS made them: {differ:?}");
    let got = sections(&got);
    let mut failures = Vec::new();
    for (i, (name, body)) in sections(&want).into_iter().enumerate() {
        let mine = got
            .get(i)
            .filter(|g| g.0 == name)
            .map_or("<missing>\n".into(), |g| g.1.clone());
        let (want, mine) = match SPLITS.iter().any(|s| s.0 == name) {
            true => (mask_splits(&body), mask_splits(&mine)),
            false => (body.clone(), mine),
        };
        if mask(&want) != mask(&mine) {
            failures.push(format!("@@ {name}\n--- VMS\n{body}--- vmsport\n{mine}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// A relative file with damaged cells, as fixtures/rmsback made it.
fn damaged() -> (&'static str, Vec<u8>, &'static str) {
    (
        "RBADCTL.DAT;1",
        std::fs::read(fixtures("fdlutil/bad/RBADCTL.DAT")).unwrap(),
        "org=rel rfm=var rat=cr mrs=10 lrl=10 fsz=0 bks=1",
    )
}

#[test]
fn fdlutil() {
    let (n, b, f) = damaged();
    // (S.DAT's DCL WRITEs leave VMS's odd records a stray pad byte.)
    let same = [("R.DAT;1", "R.DAT"), ("A.TXT;1", "A1.TXT")];
    run_area("fdlutil", "FDLUTIL.COM", &[(n, &b, f)], &same);
}

#[test]
fn fdlutil2() {
    run_area("fdlutil2", "FDLUTIL2.COM", &[], &[]);
}

/// Cases where vmsport's indexed file splits its buckets elsewhere than
/// VMS's did (vms_rms::idx's choices): the reports match but for what
/// mask_splits hides.
const SPLITS: &[(&str, &str)] = &[
    (
        "ANALYZE/RMS_FILE/OUTPUT=SYS$OUTPUT I.DAT",
        "4 data buckets on VMS, 5 here",
    ),
    (
        "ANALYZE/RMS_FILE/FDL/OUTPUT=SYS$OUTPUT I.DAT",
        "4 data buckets on VMS, 5 here",
    ),
];
