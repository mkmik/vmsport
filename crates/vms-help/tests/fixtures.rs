//! HELP over fixtures/help/TEST.HLP gives what VMS HELP gave over the same
//! library (fixtures/help/recorded/help.log), at the recording's width 132.

use vms_help::{Help, Out, parse, words};

fn fixture(name: &str) -> String {
    std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/help")
            .join(name),
    )
    .unwrap()
}

/// The recorded output of each `@@ label`.
fn recorded() -> Vec<(String, Vec<String>)> {
    let log = fixture("recorded/help.log");
    log.split("@@ ")
        .skip(1)
        .map(|b| {
            let (label, rest) = b.split_once('\n').unwrap();
            (
                label.to_string(),
                rest.lines().map(str::to_string).collect(),
            )
        })
        .collect()
}

fn help(instructions: bool) -> Help {
    Help {
        libraries: vec![parse(&fixture("TEST.HLP"))],
        width: 132,
        instructions,
    }
}

/// A session; with `answers`, prompting with them as input.
fn session(h: &Help, topic: &str, answers: Option<&[&str]>) -> Vec<String> {
    let mut out = Out::default();
    let mut left: Vec<String> = answers
        .unwrap_or(&[])
        .iter()
        .map(|s| s.to_string())
        .collect();
    left.reverse();
    h.session(&mut out, &words(topic), answers.is_some(), &mut |_, _| {
        left.pop()
    });
    out.lines
}

#[test]
fn recorded_help() {
    let script = fixture("help.com");
    let topics: Vec<(String, String)> = script
        .lines()
        .filter_map(|l| l.strip_prefix("$ CALL h "))
        .map(|l| {
            let parts: Vec<&str> = l.split('"').collect();
            (parts[1].to_string(), parts[3].to_string())
        })
        .collect();
    let mut failures = Vec::new();
    let mut checked = 0;
    for (label, want) in recorded() {
        let got: Vec<String> = match label.as_str() {
            "noinstructions" => session(&help(false), "", None),
            "prompting" => {
                let answers = [
                    "ALPHA",
                    "SUBTOPIC_ONE",
                    "?",
                    "DEEPER",
                    "",
                    "",
                    "NOSUCH",
                    "DELTA",
                    "ONE",
                    "",
                    "",
                    "",
                ];
                session(&help(true), "", Some(&answers))
            }
            "prompting from a topic" => session(&help(true), "BETA", Some(&["GAMMA", ""])),
            // HELP/OUTPUT: what TYPE showed of the file.
            "output file" => session(&help(true), "BETA", None),
            "no library" => continue,
            l => match topics.iter().find(|t| t.0 == l) {
                Some((_, topic)) => session(&help(true), topic, None),
                None => panic!("no case for {l}"),
            },
        };
        // DCL's lines after HELP: its status, and SKPDAT for unread data.
        let want: Vec<String> = want
            .into_iter()
            .take_while(|l| !l.starts_with("status ") && !l.starts_with("%DCL-W-SKPDAT"))
            .collect();
        if got != want {
            failures.push(format!(
                "@@ {label}\n--- VMS\n{}\n--- vmsport\n{}",
                want.join("|\n"),
                got.join("|\n")
            ));
        }
        checked += 1;
    }
    assert!(
        failures.is_empty(),
        "{} of {checked} differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert!(checked > 30);
}
