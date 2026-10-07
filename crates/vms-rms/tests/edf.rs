//! EDIT/FDL/NOINTERACTIVE/ANALYSIS's designs, as the FDL editor of
//! OpenVMS 8.4 made them for the corpus in fixtures/edf: each case's input
//! FDL (typed by its runN.dcl), the analysis it was given and the FDL it
//! wrote (both TYPEd in recorded/runN.log) must come out the same here.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use vms_rms::edf::{self, Outcome};
use vms_rms::fdl;

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/edf")
}

/// The files a command file types: `CREATE name`, its lines, `@@CTRLZ`.
fn typed(dcl: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut lines = dcl.lines();
    while let Some(l) = lines.next() {
        if let Some(name) = l.strip_prefix("CREATE ").filter(|n| !n.starts_with('/')) {
            let text: Vec<&str> = lines.by_ref().take_while(|l| *l != "@@CTRLZ").collect();
            out.insert(name.to_string(), text.join("\n") + "\n");
        }
    }
    out
}

/// What a procedure TYPEd between `@@ header` lines, from its `@RUN` on;
/// messages that come after a TYPE (a `%` line and what follows) cut off.
fn sections(log: &str) -> Vec<(String, String)> {
    let log = &log[log.find("$ @RUN").unwrap()..];
    let mut out = Vec::new();
    let mut cur: Option<(String, String, bool)> = None;
    for l in log.lines() {
        if let Some(h) = l.strip_prefix("@@ ") {
            out.extend(cur.take().map(|(h, b, _)| (h, b)));
            cur = Some((h.to_string(), String::new(), false));
        } else if let Some((_, body, cut)) = cur.as_mut() {
            *cut |= l.starts_with('%');
            if !*cut {
                body.push_str(l);
                body.push('\n');
            }
        }
    }
    out.extend(cur.map(|(h, b, _)| (h, b)));
    out
}

/// The date the output was written, for its IDENT.
fn when(output: &str) -> &str {
    let s = &output[output.find('"').unwrap() + 1..];
    &s[..s.find("  OpenVMS").unwrap()]
}

fn check(name: &str, input: &str, analysis: &str, quals: &str, want: &str) -> Option<String> {
    let granularity = quals
        .split('/')
        .find_map(|q| q.strip_prefix("GRANULARITY="))
        .map_or(3, |g| g.parse().unwrap());
    let got = match edf::optimize(
        &fdl::parse(input).unwrap(),
        &fdl::parse(analysis).unwrap(),
        granularity,
        when(want),
    ) {
        Outcome::Fdl(f) => edf::text(&f),
        o => format!("{o:?}\n"),
    };
    (got != want).then(|| format!("{name}:\n--- VMS\n{want}--- vmsport\n{got}"))
}

#[test]
fn corpus() {
    let mut failures = Vec::new();
    let mut count = 0;
    // Run 1: files of every organization, analyzed for real.
    let files = typed(&std::fs::read_to_string(dir().join("run1.dcl")).unwrap());
    let typed1: HashMap<String, String> =
        sections(&std::fs::read_to_string(dir().join("recorded/run1.log")).unwrap())
            .into_iter()
            .collect();
    for (out, input, analysis, quals) in [
        ("IDXO.FDL", "IDX.FDL", "IDXA.FDL", ""),
        ("IDSO.FDL", "IDX.FDL", "IDSA.FDL", ""),
        ("IDVO.FDL", "IDV.FDL", "IDVA.FDL", ""),
        (
            "IDXSB.FDL",
            "IDX.FDL",
            "IDXA.FDL",
            "/EMPHASIS=SMALLER_BUFFERS",
        ),
        ("IDXG4.FDL", "IDX.FDL", "IDXA.FDL", "/GRANULARITY=4"),
        ("IDXSELF.FDL", "IDXA.FDL", "IDXA.FDL", ""),
    ] {
        let input = files.get(input).unwrap_or_else(|| &typed1[input]);
        count += 1;
        failures.extend(check(out, input, &typed1[analysis], quals, &typed1[out]));
    }
    // Then the sweeps: synthetic analyses, the cases' outputs.
    for run in ["run2", "run3", "run4a", "run4b", "run4c", "run4d", "run5"] {
        let Ok(log) = std::fs::read_to_string(dir().join(format!("recorded/{run}.log"))) else {
            continue;
        };
        let files = typed(&std::fs::read_to_string(dir().join(format!("{run}.dcl"))).unwrap());
        let secs = sections(&log);
        for (i, (head, analysis)) in secs.iter().enumerate() {
            let w: Vec<&str> = head.split(' ').collect();
            let [name, input, quals, status] = w[..] else {
                continue;
            };
            let Some((_, output)) = secs
                .get(i + 1)
                .filter(|(h, _)| *h == format!("{name} output"))
            else {
                continue;
            };
            if status != "00000001" || output.is_empty() {
                continue;
            }
            count += 1;
            failures.extend(check(
                &format!("{run} {name}"),
                &files[input],
                analysis,
                quals,
                output,
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {count} differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
    // Every case with an output: none lost to the log's parsing.
    assert_eq!(count, 219);
}
