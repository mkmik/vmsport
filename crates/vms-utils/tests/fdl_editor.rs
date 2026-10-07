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
/// on; what was typed after a question is the answer. HELP's text is in
/// vmsport's own words, so in a help session only its prompts must come
/// as VMS's did: what differs is let be until the next one.
struct Replay<'a> {
    want: &'a str,
    got: String,
    col: usize,
    /// Where `got` went wrong, and how far it was right.
    wrong: Option<String>,
    right: usize,
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
        if self.wrong.is_none() {
            if self.want.starts_with(&self.got) {
                self.right = self.got.len();
            } else {
                self.wrong = Some(self.diff());
            }
        }
    }

    fn diff(&self) -> String {
        {
            let at = self
                .want
                .char_indices()
                .zip(self.got.chars())
                .find(|((_, a), b)| a != b)
                .map_or(self.got.len().min(self.want.len()), |((i, _), _)| i);
            let from = self.want[..at].rfind('\n').map_or(0, |i| i + 1);
            format!(
                "differs at {at}:\n--- VMS\n{}\n--- vmsport\n{}",
                &self.want[from..(at + 200).min(self.want.len())],
                &self.got[from..]
            )
        }
    }
}

impl Console for Replay<'_> {
    fn ask(&mut self, prompt: &str) -> Option<String> {
        let last = prompt.rsplit('\n').next().unwrap();
        if let Some(why) = self.wrong.take() {
            // Back in step at a help prompt, as VMS asked it.
            let at = last
                .ends_with("opic? ")
                .then(|| self.want[self.right..].find(&format!("\n{last}")))
                .flatten()
                .unwrap_or_else(|| panic!("{why}"));
            self.got = self.want[..self.right + at + 1].to_string();
            self.col = 0;
            self.put(last);
        } else {
            self.put(prompt);
        }
        if let Some(why) = &self.wrong {
            panic!("{why}");
        }
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

    /// No file but the analysis is there; what the image says of the
    /// one it can't open (its messages) is taken as VMS's.
    fn read(&mut self, _spec: &str) -> Option<String> {
        let rest = &self.want[self.got.len()..];
        let n: usize = rest
            .split_inclusive('\n')
            .take_while(|l| l.starts_with('%') || l.starts_with('-'))
            .map(str::len)
            .sum();
        self.got += &rest[..n];
        self.col = 0;
        None
    }
}

/// VMS's console with the blank lines after a design's plot, as many as
/// its screen handling happened to write (3 to 25), made 3.
fn screens(log: &str) -> String {
    let mut out = String::new();
    let mut blank = 0;
    for l in log.split_inclusive('\n') {
        if l == "\n" {
            blank += 1;
            continue;
        }
        let keep = if blank > 3 && out.ends_with("Bucket Size (number of blocks)\n") {
            3
        } else {
            blank
        };
        out.extend(std::iter::repeat_n('\n', keep));
        blank = 0;
        out += l;
    }
    out + &"\n".repeat(blank)
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

/// Replays the session of `log` that `cmd` started, the definition
/// `file` (None: new) given.
fn replay(log: &str, cmd: &str, file: Option<&str>) {
    replay_nth(log, cmd, 0, file)
}

/// Replays the `n`th session `cmd` started.
fn replay_nth(log: &str, cmd: &str, n: usize, file: Option<&str>) {
    replay_full(log, cmd, n, file, None, true)
}

/// Replays the `n`th session `cmd` started, with the definition `file`
/// and the analysis `analysis` given; and, if `check`, checks the file
/// it wrote.
fn replay_full(
    log: &str,
    cmd: &str,
    n: usize,
    file: Option<&str>,
    analysis: Option<&str>,
    check: bool,
) {
    let all = sessions(&screens(
        &std::fs::read_to_string(fixtures().join(log)).unwrap(),
    ));
    let (_, want) = all
        .iter()
        .filter(|(c, _)| c == cmd)
        .nth(n)
        .unwrap_or_else(|| panic!("{cmd} not in {log}"));
    let mut r = Replay {
        want,
        got: String::new(),
        col: 0,
        wrong: None,
        right: 0,
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
            analysis: analysis.map(|a| vms_rms::fdl::parse(a).unwrap()),
            now: &now,
            emphasis: None,
            granularity: None,
        },
    );
    if let Some(why) = &r.wrong {
        panic!("{why}");
    }
    // What the image says once it has written the file.
    if !check {
        return;
    }
    if let Ending::Exit(f, _) = &ending {
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
    // TYPE of several versions heads each with its name; of one, not.
    let file = name.rsplit(']').next().unwrap().split(';').next().unwrap();
    let body = match text.find(&format!("\n{name}\n \n")) {
        Some(at) => &text[at + name.len() + 4..],
        None => {
            let typed = format!("\n$ TYPE {file};0\n");
            let at = text
                .find(&typed)
                .unwrap_or_else(|| panic!("{name} not typed in {log}"));
            &text[at + typed.len()..]
        }
    };
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

#[test]
fn scripts_by_record_format() {
    for f in [
        "S3", "S4", "S5", "S6", "S7", "S8", "S9", "R3", "R4", "R5", "R6", "R7", "R8", "R9",
    ] {
        let script = if f.starts_with('S') {
            "SEQUENTIAL"
        } else {
            "RELATIVE"
        };
        replay(
            "recorded/seqrel2.log",
            &format!("/SCRIPT={script} {f}.FDL"),
            None,
        );
    }
}

/// The indexed definition the menu recordings edit (fixtures/edf/menus.dcl).
const IDX: &str = "FILE
  ORGANIZATION indexed
RECORD
  FORMAT fixed
  SIZE 64
KEY 0
  SEG0_LENGTH 8
  SEG0_POSITION 0
  DUPLICATES no
  CHANGES no
KEY 1
  SEG0_LENGTH 10
  SEG0_POSITION 8
  DUPLICATES yes
  CHANGES yes
";

#[test]
fn view_add_modify_delete() {
    let log = "recorded/menus.log";
    replay(log, " SMALL.FDL", Some("FILE\n  ORGANIZATION sequential\n"));
    for m in ["M1", "M2", "M3"] {
        replay(log, &format!(" {m}.FDL"), Some(IDX));
    }
}

#[test]
fn set_quit_and_output() {
    let log = "recorded/menus.log";
    replay_nth(log, " M4.FDL", 0, Some(IDX));
    replay_nth(log, " M4.FDL", 1, Some(IDX));
    replay(log, "/OUTPUT=OUT.FDL M4.FDL", Some(IDX));
}

#[test]
fn add_tables_and_values() {
    let log = "recorded/menus2.log";
    for m in ["M5", "M6", "M7"] {
        replay(log, &format!(" {m}.FDL"), Some(IDX));
    }
    replay(log, " M8.FDL", Some("FILE\n  ORGANIZATION sequential\n"));
}

#[test]
fn indexed_script() {
    for i in 1..=6 {
        replay(
            "recorded/indexed.log",
            &format!("/SCRIPT=INDEXED I{i}.FDL"),
            None,
        );
    }
}

/// What ANALYZE/RMS_FILE/FDL said of others.dcl's IDX.DAT, as far as the
/// designs read it (the log has its figures, not the file).
const IDXA: &str = "FILE
  ORGANIZATION indexed
  CLUSTER_SIZE 16
ANALYSIS_OF_KEY 0
  DATA_KEY_COMPRESSION 56
  DATA_RECORD_COMPRESSION 73
  DATA_RECORD_COUNT 1000
  INDEX_COMPRESSION 0
  MEAN_DATA_LENGTH 64
ANALYSIS_OF_KEY 1
  DATA_KEY_COMPRESSION 56
  DATA_RECORD_COUNT 50
  DUPLICATES_PER_SIDR 19
";

#[test]
fn optimize_add_key_touchup_delete_key() {
    let log = "recorded/others.log";
    replay_full(
        log,
        "/SCRIPT=OPTIMIZE/ANALYSIS=IDXA.FDL O1.FDL",
        0,
        Some(IDX),
        Some(IDXA),
        true,
    );
    replay(log, "/SCRIPT=DELETE_KEY D1.FDL", Some(IDX));
    replay(log, " V1.FDL", Some(IDX));
    // VMS leaves areas behind the files these wrote: AREA 1 of
    // BUCKET_SIZE 12 and the like, which vmsport doesn't.
    replay_full(log, "/SCRIPT=OPTIMIZE O2.FDL", 0, Some(IDX), None, false);
    replay_full(log, "/SCRIPT=ADD_KEY A1.FDL", 0, Some(IDX), None, false);
    replay_full(log, "/SCRIPT=TOUCHUP T1.FDL", 0, Some(IDX), None, false);
}

#[test]
fn first_probe() {
    let log = "recorded/probe3.log";
    replay(log, " IDX.FDL", Some(IDX));
    for cmd in [
        "/SCRIPT=INDEXED S1.FDL",
        "/SCRIPT=SEQUENTIAL S2.FDL",
        "/SCRIPT=RELATIVE S3.FDL",
    ] {
        replay(log, cmd, None);
    }
    replay(log, "/SCRIPT=DELETE_KEY DK.FDL", Some(IDX));
    // Scripts left with Ctrl/Z: VMS writes the areas they had begun.
    let opt = "/SCRIPT=OPTIMIZE/ANALYSIS=IDXA.FDL OPT.FDL";
    replay_full(log, opt, 0, Some(IDX), Some(IDXA), false);
    for cmd in ["/SCRIPT=ADD_KEY AK.FDL", "/SCRIPT=TOUCHUP TU.FDL"] {
        replay_full(log, cmd, 0, Some(IDX), None, false);
    }
}
