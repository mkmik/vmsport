//! The FDL editor's sessions as OpenVMS 8.4 had them (fixtures/edf):
//! each replays through vms_utils::fdl_editor, the recorded transcript
//! saying what was typed at each question and what must come out.

use std::path::{Path, PathBuf};
use vms_utils::fdl_editor::{self, Console, Ending, Start};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/edf")
}

/// The console VMS's log was: what the editor says goes on the end of
/// `got` (tabs as the terminal shows them) and must be how `want` goes
/// on; what was typed after a question is the answer.
struct Replay<'a> {
    want: &'a str,
    got: String,
    col: usize,
}

impl Replay<'_> {
    fn put(&mut self, s: &str) {
        for ch in s.chars() {
            match ch {
                '\t' => {
                    let n = 8 - self.col % 8;
                    self.got.extend(std::iter::repeat_n(' ', n));
                    self.col += n;
                }
                '\n' => {
                    self.got.push('\n');
                    self.col = 0;
                }
                c => {
                    self.got.push(c);
                    self.col += 1;
                }
            }
        }
        self.check();
    }

    fn check(&self) {
        if !self.want.starts_with(&self.got) {
            let at = self
                .want
                .char_indices()
                .zip(self.got.chars())
                .find(|((_, a), b)| a != b)
                .map_or(self.got.len().min(self.want.len()), |((i, _), _)| i);
            let from = self.want[..at].rfind('\n').map_or(0, |i| i + 1);
            panic!(
                "differs at {at}:\n--- VMS\n{}\n--- vmsport\n{}",
                &self.want[from..(at + 200).min(self.want.len())],
                &self.got[from..]
            );
        }
    }
}

impl Console for Replay<'_> {
    fn ask(&mut self, prompt: &str) -> Option<String> {
        self.put(prompt);
        let rest = &self.want[self.got.len()..];
        let typed = rest.split('\n').next().unwrap_or("").to_string();
        self.got += &typed;
        self.got.push('\n');
        self.col = 0;
        (typed != "*EXIT*").then_some(typed)
    }

    fn say(&mut self, text: &str) {
        self.put(text);
    }
}

/// The sessions of a recorded log: what came after each `$ EDIT/FDL...`
/// line up to DCL's next prompt, with the command.
fn sessions(log: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for chunk in log.split("\n$ EDIT/FDL").skip(1) {
        let (cmd, rest) = chunk.split_once('\n').unwrap();
        let end = rest.find("\n$ ").map_or(rest.len(), |i| i + 1);
        out.push((cmd.to_string(), rest[..end].to_string()));
    }
    out
}

/// The time the session's IDENT gave.
fn now(text: &str) -> String {
    text.split("\" ")
        .nth(1)
        .and_then(|t| t.get(..20))
        .map_or(String::new(), |t| format!(" {}", &t[..19]))
}

fn help() -> vms_help::Help {
    let text = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../sys/SYSHLP/EDFHELP.HLP"),
    )
    .unwrap();
    vms_help::Help {
        libraries: vec![vms_help::parse(&text)],
        width: 132,
        instructions: false,
    }
}

/// Replays `session` of `log`, the definition `file` (None: new) given.
fn replay(log: &str, cmd: &str, file: Option<&str>) {
    let all = sessions(&std::fs::read_to_string(fixtures().join(log)).unwrap());
    let (_, want) = all
        .iter()
        .find(|(c, _)| c == cmd)
        .unwrap_or_else(|| panic!("{cmd} not in {log}"));
    let mut r = Replay {
        want,
        got: String::new(),
        col: 0,
    };
    let shown = want
        .lines()
        .find_map(|l| l.trim().strip_suffix(" will be created."))
        .unwrap_or("");
    // The file written, if any, says the session's IDENT.
    let written = want
        .trim_end()
        .rsplit('\n')
        .next()
        .filter(|l| l.ends_with(" lines"))
        .map(|l| l.split_whitespace().next().unwrap().to_string());
    let now = match &written {
        Some(name) => now(&log_file(log, name)),
        None => now(want),
    };
    let script = cmd
        .split('/')
        .find_map(|q| q.strip_prefix("SCRIPT="))
        .map(|q| q.split_whitespace().next().unwrap_or(""));
    let ending = fdl_editor::session(
        &mut r,
        &help(),
        Start {
            text: file,
            shown,
            script,
            analysis: None,
            now: &now,
        },
    );
    // What the image says once it has written the file.
    if let Ending::Exit(f) = &ending {
        let text = vms_rms::edf::text(f);
        let rest = &want[r.got.len()..];
        assert!(
            rest.ends_with(&format!("  {} lines\n", text.lines().count())),
            "{rest:?}"
        );
        // What was written, as TYPE showed it.
        let name = written.unwrap();
        assert_eq!(expand(&text), log_file(log, &name), "{name}");
    } else {
        assert_eq!(&want[r.got.len()..], "", "after {ending:?}");
    }
}

/// What `TYPE` showed of file `name` (`DEV:[DIR]X.FDL;2`) in `log`.
fn log_file(log: &str, name: &str) -> String {
    let text = std::fs::read_to_string(fixtures().join(log)).unwrap();
    let at = text
        .find(&format!("\n{name}\n \n"))
        .unwrap_or_else(|| panic!("{name} not typed in {log}"));
    let body = &text[at + name.len() + 4..];
    let end = [body.find("\n \n"), body.find("\n$ ")]
        .into_iter()
        .flatten()
        .min()
        .unwrap_or(body.len());
    body[..end + 1].to_string()
}

/// Tabs as the console showed them, 8 columns apart.
fn expand(s: &str) -> String {
    let mut out = String::new();
    for line in s.split_inclusive('\n') {
        let mut col = 0;
        for ch in line.chars() {
            if ch == '\t' {
                let n = 8 - col % 8;
                out.extend(std::iter::repeat_n(' ', n));
                col += n;
            } else {
                out.push(ch);
                col += 1;
            }
        }
    }
    out
}

#[test]
fn sequential_and_relative_scripts() {
    for cmd in [
        "/SCRIPT=SEQUENTIAL S1.FDL",
        "/SCRIPT=SEQUENTIAL S2.FDL",
        "/SCRIPT=RELATIVE R1.FDL",
        "/SCRIPT=RELATIVE R2.FDL",
    ] {
        replay("recorded/seqrel.log", cmd, None);
    }
    replay(
        "recorded/seqrel.log",
        " SMALL.FDL",
        Some("FILE\n  ORGANIZATION sequential\n"),
    );
}

#[test]
fn new_file_viewed_and_left_empty() {
    replay("recorded/probe3.log", " NEW.FDL", None);
}
