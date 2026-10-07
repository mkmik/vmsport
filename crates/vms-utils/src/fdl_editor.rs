//! The FDL editor's interactive part (EDIT/FDL at a terminal): its main
//! menu (ADD, DELETE, EXIT, HELP, INVOKE, MODIFY, QUIT, SET, VIEW), its
//! questions and scripts, as OpenVMS 8.4's says them (fixtures/edf).
//!
//! The dialogue is pure: a [`Console`] asks and says, so the recorded
//! sessions replay in tests. The image (bin/edf.rs) reads and writes the
//! files.

mod scripts;

use vms_help::Help;
use vms_rms::fdl::{Fdl, Section};

/// The terminal, as the editor uses it.
pub trait Console {
    /// Shows `prompt` and returns the line typed; `None` for Ctrl/Z.
    fn ask(&mut self, prompt: &str) -> Option<String>;
    /// Writes `text` as it is.
    fn say(&mut self, text: &str);
}

/// How a session ended.
#[derive(Debug, PartialEq, Eq)]
pub enum Ending {
    /// EXIT (or Ctrl/Z at the main menu): write this.
    Exit(Fdl),
    /// EXIT with nothing defined: nothing written.
    Empty,
    Quit,
}

/// What the editor works on.
pub struct Editor<'a> {
    pub fdl: Fdl,
    /// The FDL editor's help library (SYS$HELP:EDFHELP).
    pub help: &'a Help,
    /// IDENT's text: `FDL_VERSION 02\t" 7-OCT-2026 17:52:37  OpenVMS FDL Editor"`.
    pub ident: String,
    /// /ANALYSIS, read.
    pub analysis: Option<Fdl>,
}

/// What a question takes.
#[derive(Clone)]
pub enum Takes {
    /// A number from `lo` to `hi`, the range shown as `shown`: `(0-2Giga)`.
    Number {
        lo: u64,
        hi: u64,
        shown: String,
    },
    /// One of `words`, listed above the question as `list` (its lines).
    Keyword {
        words: &'static [&'static str],
        list: &'static str,
    },
    YesNo,
    /// Text of 1 to `max` characters.
    Text {
        max: usize,
    },
}

/// An answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    Number(u64),
    Word(&'static str),
    Yes(bool),
    Text(String),
}

/// A question: its text (its last line beside what it takes), what it
/// takes, the default (shown, and what it is), and the lines that explain
/// it when an answer isn't taken.
#[derive(Clone)]
pub struct Q {
    pub text: String,
    pub takes: Takes,
    pub default: Option<(String, Answer)>,
    pub explain: &'static str,
    /// What follows the default: `\t: ` mostly.
    pub sep: &'static str,
}

impl Q {
    pub fn new(text: &str, takes: Takes) -> Q {
        Q {
            text: text.into(),
            takes,
            default: None,
            explain: "",
            sep: "\t: ",
        }
    }

    pub fn number(text: &str, lo: u64, hi: u64) -> Q {
        let shown = |n: u64| match n {
            2147483647 => "2Giga".to_string(),
            4294967295 => "4Giga".to_string(),
            n => n.to_string(),
        };
        let shown = format!("({}-{})", shown(lo), shown(hi));
        Q::new(text, Takes::Number { lo, hi, shown })
    }

    pub fn or(mut self, shown: &str, a: Answer) -> Q {
        self.default = Some((shown.into(), a));
        self
    }

    pub fn explained(mut self, lines: &'static str) -> Q {
        self.explain = lines;
        self
    }

    /// The prompt: the keyword list, the text, what it takes, the default.
    fn prompt(&self, again: bool) -> String {
        let default = self.default.as_ref().map_or("-", |d| d.0.as_str());
        let mut out = String::new();
        if !again {
            out.push('\n');
        }
        if let Takes::Keyword { list, .. } = &self.takes {
            out += list;
        }
        let choices = match &self.takes {
            Takes::Number { shown, .. } => shown.clone(),
            Takes::Keyword { .. } => "(Keyword)".into(),
            Takes::YesNo => "(Yes/No)".into(),
            Takes::Text { max } => format!("(1-{max} chars)"),
        };
        if let Takes::Text { .. } = self.takes {
            let default = self.default.as_ref().map_or("null", |d| d.0.as_str());
            return out + &question_text(&self.text) + &format!("{choices}[{default}]\n\t: ");
        }
        // A long range and default are followed by " : ", not a tab.
        let field = format!("{choices}[{default}]");
        let sep = match field.len() >= 16 && !self.text.contains('\n') {
            true => " : ",
            false => self.sep,
        };
        out + &question_text(&self.text) + &field + sep
    }
}

/// A question's text at column 8, its last line padded with tabs to
/// column 40 unless it ends in its own tabs.
fn question_text(text: &str) -> String {
    let mut lines: Vec<&str> = text.split('\n').collect();
    let last = lines.pop().unwrap_or("");
    let mut out: String = lines.iter().map(|l| format!("\t{l}\n")).collect();
    out += &format!("\t{last}");
    if !last.ends_with('\t') {
        out += &"\t".repeat((40 - (8 + last.len()).min(39)).div_ceil(8));
    }
    out
}

impl Editor<'_> {
    /// Asks `q` until it is answered as it takes; `None` for Ctrl/Z.
    pub fn ask(&self, c: &mut impl Console, q: &Q) -> Option<Answer> {
        let mut again = false;
        loop {
            let typed = c.ask(&q.prompt(again))?;
            let t = typed.trim();
            again = true;
            if t.is_empty() {
                if let Some((_, a)) = &q.default {
                    return Some(a.clone());
                }
                c.say("\n\t You must provide an answer here (or ^Z for Main Menu).\n");
                c.say(q.explain);
                continue;
            }
            let a = match &q.takes {
                Takes::Number { lo, hi, .. } => t
                    .parse::<u64>()
                    .ok()
                    .filter(|n| (lo..=hi).contains(&n))
                    .map(Answer::Number),
                Takes::Keyword { words, .. } => keyword(t, words).map(Answer::Word),
                Takes::YesNo => keyword(t, &["YES", "NO"]).map(|w| Answer::Yes(w == "YES")),
                Takes::Text { max } => (t.len() <= *max).then(|| Answer::Text(t.to_string())),
            };
            if let Some(a) = a {
                return Some(a);
            }
            let why = match q.takes {
                Takes::Keyword { .. } => "contains a syntax error",
                _ => "is not appropriate in this context",
            };
            c.say(&format!("\n\t \"{}\" {why}. \n", t.to_ascii_uppercase()));
            c.say(q.explain);
        }
    }

    /// Waits for Return; `None` for Ctrl/Z.
    pub fn press_return(&self, c: &mut impl Console, prompt: &str) -> Option<()> {
        c.ask(prompt).map(drop)
    }
}

/// How a session starts.
pub struct Start<'a> {
    /// The definition file's text, if it exists.
    pub text: Option<&'a str>,
    /// Its name as $PARSE gives it, for "will be created".
    pub shown: &'a str,
    /// /SCRIPT: run first.
    pub script: Option<&'a str>,
    /// /ANALYSIS, read.
    pub analysis: Option<Fdl>,
    /// The time IDENT gives: ` 7-OCT-2026 17:52:37`.
    pub now: &'a str,
}

/// A session: the definition read (or a new one), /SCRIPT's script, then
/// the main menu.
pub fn session(c: &mut impl Console, help: &Help, s: Start) -> Ending {
    c.say("\t\t\tParsing Definition File\n");
    let fdl = match s.text {
        Some(t) => {
            let f = normalized(vms_rms::fdl::parse(t).unwrap_or_default());
            c.say("\t\t\tDefinition Parse Complete\n");
            f
        }
        None => {
            c.say(&format!("\n\n\n\t{} will be created.\n", s.shown));
            Fdl::default()
        }
    };
    let mut e = Editor {
        fdl,
        help,
        ident: format!("FDL_VERSION 02\t\"{}  OpenVMS FDL Editor\"", s.now),
        analysis: s.analysis,
    };
    if let Some(name) = s.script {
        let script = keyword(name, &scripts::SCRIPTS).unwrap_or("");
        c.say(&format!("\t\t\t {} Script \n", scripts::title(script)));
        e.script(c, script);
    }
    e.run(c)
}

/// The order EDF keeps sections in.
const ORDER: [&str; 15] = [
    "TITLE",
    "IDENT",
    "SYSTEM",
    "FILE",
    "DATE",
    "RECORD",
    "ACCESS",
    "SHARING",
    "CONNECT",
    "NETWORK",
    "JOURNALING",
    "AREA",
    "KEY",
    "ANALYSIS_OF_AREA",
    "ANALYSIS_OF_KEY",
];

/// A definition as EDF holds it: sections in its order (numbered ones by
/// number), attributes by name.
pub fn normalized(mut f: Fdl) -> Fdl {
    for s in &mut f.sections {
        s.attrs.retain(|(k, _)| k != "!");
        s.attrs.sort_by(|a, b| a.0.cmp(&b.0));
    }
    f.sections.sort_by_key(|s| {
        (
            ORDER
                .iter()
                .position(|o| *o == s.name)
                .unwrap_or(ORDER.len()),
            s.value.trim().parse::<u32>().unwrap_or(0),
        )
    });
    f
}

/// The main menu's functions, as the prompt lists them.
const FUNCTIONS: [&str; 9] = [
    "ADD", "DELETE", "EXIT", "HELP", "INVOKE", "MODIFY", "QUIT", "SET", "VIEW",
];

/// A question at column 8 with its choices at 40, as EDF lays them out:
/// `\tMain Editor Function\t\t(Keyword)[Help]\t: `.
pub fn question(text: &str, choices: &str, default: &str) -> String {
    question_text(text) + &format!("{choices}[{default}]\t: ")
}

/// A keyword typed for one of `words`: unique abbreviations allowed.
pub fn keyword<'w>(typed: &str, words: &[&'w str]) -> Option<&'w str> {
    let t = typed.trim().to_ascii_uppercase();
    if t.is_empty() {
        return None;
    }
    let hits: Vec<&&str> = words.iter().filter(|w| w.starts_with(&t)).collect();
    match hits[..] {
        [w] => Some(w),
        _ => words.iter().find(|w| **w == t).copied(),
    }
}

impl Editor<'_> {
    /// The main menu, until EXIT, QUIT or Ctrl/Z.
    pub fn run(&mut self, c: &mut impl Console) -> Ending {
        let menu = "\t(Add Delete Exit Help Invoke Modify Quit Set View)\n";
        let mut show_menu = true;
        loop {
            if show_menu {
                c.say(menu);
            }
            show_menu = true;
            let q = question("Main Editor Function", "(Keyword)", "Help");
            let Some(answer) = c.ask(&q) else {
                return self.exit(c);
            };
            // The keyword: letters and digits up to anything else.
            let word = answer.trim_start();
            let end = word
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '$'))
                .unwrap_or(word.len());
            let word = &word[..end];
            let f = if word.is_empty() {
                Some("HELP")
            } else {
                keyword(word, &FUNCTIONS)
            };
            match f {
                Some("VIEW") => self.view(c),
                Some("HELP") => self.help(c),
                Some("EXIT") => return self.exit(c),
                Some("QUIT") => return Ending::Quit,
                Some("INVOKE") => self.invoke(c),
                _ => {
                    self.syntax_error(c, word);
                    show_menu = false;
                }
            }
        }
    }

    /// The definition, indented a tab, as EXIT would write it.
    fn view(&self, c: &mut impl Console) {
        c.say("\n");
        for l in vms_rms::edf::text(&self.with_ident()).lines() {
            c.say(&if l.is_empty() {
                "\n".to_string()
            } else {
                format!("\t{l}\n")
            });
        }
        c.say("\n");
    }

    /// The definition with the session's IDENT first.
    pub fn with_ident(&self) -> Fdl {
        let mut f = self.fdl.clone();
        f.sections.retain(|s| s.name != "IDENT");
        f.sections.insert(0, Section::new("IDENT", &self.ident));
        f
    }

    fn exit(&self, c: &mut impl Console) -> Ending {
        if self.fdl.sections.iter().all(|s| s.name == "IDENT") {
            c.say("\n\n\t\x07Output not created - Current FDL Definition empty.\n");
            return Ending::Empty;
        }
        Ending::Exit(self.with_ident())
    }

    fn help(&self, c: &mut impl Console) {
        let mut out = vms_help::Out::default();
        out.lines.push(String::new());
        self.help.session(&mut out, &[], true, &mut |q, out| {
            for l in out.take() {
                c.say(&format!("{l}\n"));
            }
            c.ask(q)
        });
        for l in out.take() {
            c.say(&format!("{l}\n"));
        }
    }

    fn syntax_error(&self, c: &mut impl Console, word: &str) {
        c.say(&format!(
            "\n\t \"{}\" contains a syntax error. \n",
            word.to_ascii_uppercase()
        ));
        c.say("\t\t\t OpenVMS FDL Editor\n\n");
        for (f, what) in [
            ("Add", "to insert one line into the FDL definition"),
            ("Delete", "to remove one line from the FDL definition"),
            (
                "Exit",
                "to leave the FDL Editor after creating the FDL file",
            ),
            ("Help", "to obtain information about the FDL Editor"),
            ("Invoke", "to initiate a script of related questions"),
            ("Modify", "to change an existing line in the FDL definition"),
            ("Quit", "to abort the FDL Editor with no FDL file creation"),
            ("Set", "to specify FDL Editor characteristics"),
            ("View", "to display the current FDL Definition"),
        ] {
            c.say(&format!("\t{f:<8}{what}\n"));
        }
        c.say("\n");
    }
}

/// The plot a design shows: index depth (rows 9 down to 1, `*` above) at
/// each bucket size 1 to 32 (`None`: such buckets don't hold the records).
pub fn plot(depths: &[Option<u32>; 32]) -> String {
    let mut out = String::from("\n\n");
    for row in (1..=10u32).rev() {
        let label = match row {
            10 => '*',
            r => char::from_digit(r, 10).unwrap(),
        };
        let name = match row {
            7 => "Index",
            5 => "Depth",
            _ => "",
        };
        let mut line = format!("{name:<13}{label}|");
        for (i, d) in depths.iter().enumerate() {
            if let Some(d) = d.filter(|d| (*d).min(10) == row) {
                let col = 15 + 2 * (i + 1);
                line.extend(std::iter::repeat_n(' ', col - line.len()));
                line.push(if d >= 10 {
                    '*'
                } else {
                    char::from_digit(d, 10).unwrap()
                });
            }
        }
        out += &line;
        out.push('\n');
    }
    let mut axis = String::from("              +-");
    for b in 1..=32 {
        axis.push(' ');
        axis.push(if [1, 5, 10, 15, 20, 25, 30, 32].contains(&b) {
            '+'
        } else {
            '-'
        });
    }
    out += &axis;
    out += "\n                 1       5        10        15        20        25        30  32\n";
    out += "                               Bucket Size (number of blocks)\n\n\n";
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plots_as_vms_does() {
        // fixtures/edf: the indexed script's 1000 records of 64 bytes.
        let mut d = [Some(1); 32];
        d[0] = Some(2);
        d[1] = Some(2);
        let want = "\n\n             *|\n             9|\n             8|\nIndex        7|\n             6|\nDepth        5|\n             4|\n             3|\n             2|  2 2\n             1|      1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1\n              +- + - - - + - - - - + - - - - + - - - - + - - - - + - - - - + - +\n                 1       5        10        15        20        25        30  32\n                               Bucket Size (number of blocks)\n\n\n";
        assert_eq!(plot(&d), want);
    }
}
