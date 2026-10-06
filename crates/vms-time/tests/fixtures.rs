//! Checks vms-time against F$CVTIME results recorded on OpenVMS
//! (fixtures/time: cases.txt and recorded/time.log).

use vms_time::{bintim, cvtime};

fn fixture(name: &str) -> String {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/time")
        .join(name);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// A DCL argument list of strings: an empty argument is `None`.
fn args(s: &str) -> Vec<Option<String>> {
    let mut out = vec![None];
    let mut quoted = false;
    for c in s.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                out.last_mut().unwrap().get_or_insert_with(String::new);
            }
            ',' if !quoted => out.push(None),
            ' ' if !quoted => {}
            c => out
                .last_mut()
                .unwrap()
                .get_or_insert_with(String::new)
                .push(c),
        }
    }
    out
}

#[test]
fn recorded_f_cvtime() {
    let cases = fixture("cases.txt");
    let log = fixture("recorded/time.log");
    let mut results = Vec::new();
    for block in log.split("@@ ").skip(1) {
        let (n, body) = block.split_once('\n').unwrap();
        results.push((n.to_string(), body.trim_end().to_string()));
    }
    // F$TIME() just before the cases.
    let now = results[0].1.trim_matches(['[', ']']).to_string();
    let now = bintim(&now, 0).unwrap();
    let mut failures = Vec::new();
    let mut checked = 0;
    for (n, expect) in &results[1..results.len() - 1] {
        let case = cases.lines().nth(n.parse::<usize>().unwrap() - 1).unwrap();
        let a = args(case);
        let get = |i: usize| a.get(i).cloned().flatten();
        let (input, format, field) = (get(0), get(1), get(2));
        // The current time of day moved on while VMS ran the cases.
        if input.as_deref() == Some("")
            && field.as_deref() != Some("DATE")
            && format.as_deref() != Some("DELTA")
        {
            continue;
        }
        let got = match cvtime(input.as_deref(), format.as_deref(), field.as_deref(), now) {
            Ok(s) => format!("[{s}]"),
            Err(e) => e.to_string(),
        };
        checked += 1;
        if got != *expect {
            failures.push(format!(
                "{n} {case}\n  VMS     {expect:?}\n  vmsport {got:?}"
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert_eq!(checked, 221);
}

#[test]
fn lexicals_log() {
    // The F$CVTIME lines of fixtures/dcl/LEXICALS.COM.
    let now = bintim("7-OCT-2026 00:00", 0).unwrap();
    assert_eq!(
        cvtime(
            Some("1-JAN-2000 10:20:30.40"),
            Some("COMPARISON"),
            None,
            now
        )
        .unwrap(),
        "2000-01-01 10:20:30.40"
    );
    assert_eq!(
        cvtime(
            Some("1-JAN-2000 10:20:30.40"),
            Some("ABSOLUTE"),
            Some("WEEKDAY"),
            now
        )
        .unwrap(),
        "Saturday"
    );
    assert_eq!(
        cvtime(Some("29-FEB-2000"), Some("ABSOLUTE"), Some("DATE"), now).unwrap(),
        "29-FEB-2000"
    );
    let e = cvtime(Some("1-JAN-2000"), Some("DELTA"), None, now).unwrap_err();
    assert_eq!(
        e.to_string(),
        "%DCL-W-IVDTIME, invalid delta time - use DDDD-HH:MM:SS.CC format\n \\1-JAN-2000\\"
    );
}
