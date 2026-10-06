//! Checks vms-fao against F$FAO results recorded on OpenVMS
//! (fixtures/fao: cases.txt and recorded/fao.log).

use vms_fao::{Arg, Error, fao};

fn fixtures() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/fao")
}

/// Splits a DCL argument list: "strings" (with "" for a quote) and integers
/// (decimal or %X hex).
fn dcl_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cs = s.chars().peekable();
    while let Some(c) = cs.next() {
        match c {
            ' ' | ',' => {}
            '"' => {
                let mut v = String::from("\"");
                loop {
                    match cs.next().unwrap() {
                        '"' if cs.peek() == Some(&'"') => {
                            cs.next();
                            v.push('"');
                        }
                        '"' => break,
                        c => v.push(c),
                    }
                }
                out.push(v);
            }
            c => {
                let mut v = String::from(c);
                while cs.peek().is_some_and(|&c| c != ',') {
                    v.push(cs.next().unwrap());
                }
                out.push(v.trim().to_string());
            }
        }
    }
    out
}

fn num(s: &str) -> i64 {
    match s.strip_prefix("%X") {
        Some(h) => i64::from_str_radix(h, 16).unwrap(),
        None => s.parse().unwrap(),
    }
}

#[test]
fn recorded_f_fao() {
    let cases = std::fs::read_to_string(fixtures().join("cases.txt")).unwrap();
    let log = std::fs::read(fixtures().join("recorded/fao.log")).unwrap();
    let log: String = log.iter().map(|&b| b as char).collect(); // Latin-1
    let mut results = std::collections::HashMap::new();
    let mut parts = log.split("@@ ").skip(1);
    for p in parts.by_ref() {
        let (n, body) = p.split_once('\n').unwrap();
        results.insert(
            n.parse::<usize>().unwrap(),
            body.strip_suffix('\n').unwrap_or(body).to_string(),
        );
    }

    let mut checked = 0;
    for (i, case) in cases.lines().enumerate() {
        let n = i + 1;
        let expect = &results[&n];
        let args = dcl_args(case);
        let ctl = &args[0][1..];
        // Skipped: mistyped arguments (VMS prints garbage or ACCVIOs) and the
        // current time.
        if ctl.contains("%T") || ctl.contains("%D") || matches!(n, 52 | 53) {
            continue;
        }
        let vals: Vec<String> = args[1..].to_vec();
        let fargs: Vec<Arg> = vals
            .iter()
            .map(|v| match v.strip_prefix('"') {
                Some(s) => Arg::Str(s),
                None => Arg::Num(num(v)),
            })
            .collect();
        match fao(ctl, &fargs) {
            Ok(s) => assert_eq!(format!("[{s}]"), *expect, "case {n}: {case}"),
            Err(Error::BadParam) => {
                assert!(expect.contains("BADPARAM"), "case {n}: {case}: {expect}")
            }
            Err(e) => panic!("case {n}: {case}: {e}, VMS gave {expect}"),
        }
        checked += 1;
    }
    assert!(checked > 55, "only {checked} cases checked");
}
