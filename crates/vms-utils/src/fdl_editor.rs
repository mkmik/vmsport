//! The FDL editor's interactive part (EDIT/FDL at a terminal): its main
//! menu (ADD, DELETE, EXIT, HELP, INVOKE, MODIFY, QUIT, SET, VIEW), its
//! questions and scripts, as OpenVMS 8.4's says them (fixtures/edf).
//!
//! The dialogue is pure: a [`Console`] asks and says, so the recorded
//! sessions replay in tests. The image (bin/edf.rs) reads and writes the
//! files.

mod functions;
mod indexed;
mod scripts;

use vms_help::Help;
use vms_rms::fdl::{Fdl, Section};

/// The terminal, as the editor uses it.
pub trait Console {
    /// Shows `prompt` and returns the line typed; `None` for Ctrl/Z.
    fn ask(&mut self, prompt: &str) -> Option<String>;
    /// Writes `text` as it is.
    fn say(&mut self, text: &str);
    /// The text of file `spec` (SET ANALYSIS): `None` if it can't be read.
    fn read(&mut self, _spec: &str) -> Option<String> {
        None
    }
}

/// How a session ended.
#[derive(Debug, PartialEq, Eq)]
pub enum Ending {
    /// EXIT (or Ctrl/Z at the main menu): write this, to SET OUTPUT's
    /// file if it was given.
    Exit(Fdl, Option<String>),
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
    /// SET OUTPUT.
    pub output: Option<String>,
    /// SET PROMPTING FULL: menus as tables.
    pub full: bool,
    /// SET DISPLAY, EMPHASIS, GRANULARITY, NUMBER_KEYS, RESPONSES.
    pub graph: &'static str,
    pub emphasis: &'static str,
    pub granularity: &'static str,
    pub keys: u64,
    pub automatic: bool,
    /// In a script, where automatic responses apply.
    pub scripting: bool,
    /// The primary attribute last named (ADD, MODIFY, DELETE's default).
    pub primary: String,
    /// Blocks a design's AREA 0 gets on EXIT, for the prologue.
    pub prologue: u32,
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
    /// A date and time: `?` is no help here.
    Date,
    /// What the asker parses: shown as `shown` (`(Keyword)`), `list` above.
    Other {
        shown: &'static str,
        list: &'static str,
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
/// takes, the default (shown, and what it is), the lines that explain it
/// when an answer isn't taken (or `?` is typed), and for a menu the table
/// shown instead of them (and of the list, with full prompting).
#[derive(Clone)]
pub struct Q {
    pub text: String,
    pub takes: Takes,
    pub default: Option<(String, Answer)>,
    pub explain: String,
    pub table: Option<String>,
    /// What follows the default when it isn't `\t: ` (or ` : ` after a
    /// long one).
    pub sep: Option<&'static str>,
    /// Whether a blank line comes first.
    pub lead: bool,
}

/// What VMS says of an answer left out.
const MUST: &str = "\n\t You must provide an answer here (or ^Z for Main Menu).\n";

/// What VMS says of `word` where it can't be parsed.
pub fn syntax(word: &str) -> String {
    format!(
        "\n\t \"{}\" contains a syntax error. \n",
        word.to_ascii_uppercase()
    )
}

/// What VMS says of `word` where it parses but doesn't fit.
pub fn inappropriate(word: &str) -> String {
    format!(
        "\n\t \"{}\" is not appropriate in this context. \n",
        word.to_ascii_uppercase()
    )
}

impl Q {
    pub fn new(text: &str, takes: Takes) -> Q {
        Q {
            text: text.into(),
            takes,
            default: None,
            explain: String::new(),
            table: None,
            sep: None,
            lead: true,
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

    pub fn explained(mut self, lines: &str) -> Q {
        self.explain = lines.into();
        self
    }

    pub fn tabled(mut self, table: String) -> Q {
        self.table = Some(table);
        self
    }

    /// The prompt: the list (or table), the text, what it takes, the
    /// default; `again` after an answer that wasn't taken.
    fn prompt(&self, again: bool, full: bool) -> String {
        let default = self.default.as_ref().map_or("-", |d| d.0.as_str());
        let mut out = String::new();
        if self.lead && !again {
            out.push('\n');
        }
        let list = match &self.takes {
            Takes::Keyword { list, .. } | Takes::Other { list, .. } => list,
            Takes::Date => "\t(dd-mmm-yyyy hh:mm:ss.cc)\n",
            _ => "",
        };
        match (&self.table, again) {
            (Some(table), false) if full => out += table,
            (Some(_), true) => {}
            _ => out += list,
        }
        let choices = match &self.takes {
            Takes::Number { shown, .. } => shown.clone(),
            Takes::Keyword { .. } => "(Keyword)".into(),
            Takes::Other { shown, .. } => shown.to_string(),
            Takes::Date => "(Date-str)".into(),
            Takes::YesNo => "(Yes/No)".into(),
            Takes::Text { max } => format!("(1-{max} chars)"),
        };
        if let Takes::Text { .. } = self.takes {
            let default = self.default.as_ref().map_or("null", |d| d.0.as_str());
            return out + &question_text(&self.text) + &format!("{choices}[{default}]\n\t: ");
        }
        // A long range and default are followed by " : ", not a tab.
        let field = format!("{choices}[{default}]");
        let sep = self
            .sep
            .unwrap_or(if field.len() >= 16 { " : " } else { "\t: " });
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
        let col = last.chars().fold(
            8,
            |col, ch| if ch == '\t' { col / 8 * 8 + 8 } else { col + 1 },
        );
        out += &"\t".repeat(40usize.saturating_sub(col).div_ceil(8));
    }
    out
}

impl Editor<'_> {
    /// Asks `q` until it is answered as it takes; `None` for Ctrl/Z.
    pub fn ask(&self, c: &mut impl Console, q: &Q) -> Option<Answer> {
        self.ask_as(c, q, |t| {
            let Some(t) = t else {
                return Ok(q.default.as_ref().unwrap().1.clone());
            };
            let a = match &q.takes {
                Takes::Number { lo, hi, .. } => t
                    .parse::<u64>()
                    .ok()
                    .filter(|n| (lo..=hi).contains(&n))
                    .map(Answer::Number),
                Takes::Keyword { words, .. } => keyword(t, words).map(Answer::Word),
                Takes::YesNo => keyword(t, &["YES", "NO"]).map(|w| Answer::Yes(w == "YES")),
                Takes::Text { max } => (t.len() <= *max).then(|| Answer::Text(t.to_string())),
                Takes::Date => vms_time::bintim(t, 0)
                    .ok()
                    .map(|_| Answer::Text(t.to_string())),
                Takes::Other { .. } => None,
            };
            a.ok_or_else(|| match q.takes {
                Takes::Keyword { .. } | Takes::YesNo => syntax(token(t)),
                Takes::Date => syntax(t),
                _ => inappropriate(t),
            })
        })
    }

    /// Asks `q` until `parse` takes the answer (`None`: the default) or
    /// says what is wrong with it; `None` for Ctrl/Z.
    pub fn ask_as<A>(
        &self,
        c: &mut impl Console,
        q: &Q,
        parse: impl Fn(Option<&str>) -> Result<A, String>,
    ) -> Option<A> {
        let mut again = false;
        loop {
            if let Ok(a) = self.ask_once(c, q, again, &parse)? {
                return Some(a);
            }
            again = true;
        }
    }

    /// Asks `q` once: `Err` when the answer isn't taken (what is wrong
    /// with it said); `None` for Ctrl/Z. With automatic responses a
    /// question with a default isn't asked.
    pub fn ask_once<A>(
        &self,
        c: &mut impl Console,
        q: &Q,
        again: bool,
        parse: &impl Fn(Option<&str>) -> Result<A, String>,
    ) -> Option<Result<A, ()>> {
        if self.automatic
            && self.scripting
            && q.default.is_some()
            && let Ok(a) = parse(None)
        {
            return Some(Ok(a));
        }
        let typed = c.ask(&q.prompt(again, self.full))?;
        let t = typed.trim();
        let wrong = match t {
            "" if q.default.is_none() => MUST.to_string(),
            "" => match parse(None) {
                Ok(a) => return Some(Ok(a)),
                Err(e) => e,
            },
            "?" if !matches!(q.takes, Takes::Text { .. } | Takes::Date) => "\n".to_string(),
            t => match parse(Some(t)) {
                Ok(a) => return Some(Ok(a)),
                Err(e) => e,
            },
        };
        c.say(&wrong);
        c.say(q.table.as_deref().unwrap_or(&q.explain));
        Some(Err(()))
    }

    /// Waits for Return (anything else is an error); `None` for Ctrl/Z.
    pub fn press_return(&self, c: &mut impl Console, prompt: &str) -> Option<()> {
        loop {
            let typed = c.ask(&format!("\n{prompt}"))?;
            match typed.trim() {
                "" => return Some(()),
                t => c.say(&syntax(token(t))),
            }
        }
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
        output: None,
        full: false,
        graph: "LINE",
        emphasis: "FLATTER_FILES",
        granularity: "THREE",
        keys: 1,
        automatic: false,
        scripting: false,
        primary: "FILE".into(),
        prologue: 0,
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
        match s.name.as_str() {
            "FILE" => s.attrs.sort_by_key(|a| vms_rms::edf::file_order(&a.0)),
            _ => s.attrs.sort_by(|a, b| a.0.cmp(&b.0)),
        }
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

/// What an answer starts with: letters, digits, `_` and `$`.
pub fn token(t: &str) -> &str {
    let t = t.trim_start();
    let end = t
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '$'))
        .unwrap_or(t.len());
    &t[..end]
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
        let mut q = Q::new(
            "Main Editor Function",
            Takes::Other {
                shown: "(Keyword)",
                list: "\t(Add Delete Exit Help Invoke Modify Quit Set View)\n",
            },
        )
        .or("Help", Answer::Word("HELP"))
        .tabled(menu_table());
        q.lead = false;
        loop {
            let f = self.ask_as(c, &q, |t| {
                let Some(t) = t else { return Ok("HELP") };
                // A function and nothing after it.
                let word = token(t);
                match keyword(word, &FUNCTIONS) {
                    Some(_) if !t[word.len()..].trim().is_empty() => Err(syntax("")),
                    Some(f) => Ok(f),
                    None => Err(syntax(word)),
                }
            });
            match f {
                None => return self.exit(c),
                Some("VIEW") => self.view(c),
                Some("HELP") => self.help(c),
                Some("EXIT") => {
                    c.say("\n");
                    return self.exit(c);
                }
                Some("QUIT") => {
                    c.say("\n");
                    return Ending::Quit;
                }
                Some("INVOKE") => self.invoke(c),
                Some("SET") => self.set(c),
                Some("ADD") => self.add(c),
                Some("MODIFY") => self.modify(c),
                Some(_) => self.delete(c),
            }
        }
    }

    /// The definition, indented a tab, as EXIT would write it; a blank
    /// line after it when that leaves room for the menu on the screen.
    /// ponytail: a 24-line screen assumed, as the recordings had.
    fn view(&self, c: &mut impl Console) {
        let text = vms_rms::edf::text(&self.with_ident());
        c.say("\n");
        c.say(&indented(&text));
        if (text.lines().count() + 1) % 24 <= 21 {
            c.say("\n");
        }
    }

    /// The definition with the session's IDENT first.
    pub fn with_ident(&self) -> Fdl {
        let mut f = self.fdl.clone();
        f.sections.retain(|s| s.name != "IDENT");
        let at = f.sections.iter().take_while(|s| s.name == "TITLE").count();
        f.sections.insert(at, Section::new("IDENT", &self.ident));
        f
    }

    fn exit(&self, c: &mut impl Console) -> Ending {
        if self.fdl.sections.iter().all(|s| s.name == "IDENT") {
            c.say("\n\t\x07Output not created - Current FDL Definition empty.\n");
            return Ending::Empty;
        }
        let mut f = self.with_ident();
        if let Some(a) = f
            .sections
            .iter_mut()
            .find(|s| s.name == "AREA" && s.value.trim() == "0")
            && let Some(alloc) = a.get("ALLOCATION").and_then(|v| v.parse::<u32>().ok())
        {
            a.set("ALLOCATION", alloc + self.prologue);
        }
        Ending::Exit(f, self.output.clone())
    }

    fn help(&self, c: &mut impl Console) {
        let mut out = vms_help::Out::default();
        out.lines.push(String::new());
        self.help.session(&mut out, &[], true, &mut |q, out| {
            out.flush();
            for l in out.take() {
                c.say(&format!("{l}\n"));
            }
            c.ask(q)
        });
        for l in out.take() {
            c.say(&format!("{l}\n"));
        }
    }
}

/// FDL text as the editor shows it, each line a tab in.
pub fn indented(text: &str) -> String {
    text.lines()
        .map(|l| match l {
            "" => "\n".to_string(),
            l => format!("\t{l}\n"),
        })
        .collect()
}

/// The main menu as a table: what each function does.
fn menu_table() -> String {
    let mut out = String::from("\t\t\t OpenVMS FDL Editor\n\n");
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
        out += &format!("\t{f:<8}{what}\n");
    }
    out + "\n"
}

/// The plot a design shows: index depth (rows 9 down to 1, `*` above) at
/// each bucket size 1 to 32 (`None`: such buckets don't hold the records).
pub fn plot(depths: &[Option<u32>; 32]) -> String {
    let mut out = String::new();
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
                line.push(digit(d));
            }
        }
        out += &line;
        out.push('\n');
    }
    out + &axis()
}

/// A depth as the plots show it: `*` from 10 on.
fn digit(d: u32) -> char {
    char::from_digit(d, 10).unwrap_or('*')
}

/// The plots' bucket-size axis and its labels.
fn axis() -> String {
    let mut axis = String::from("              +-");
    for b in 1..=32 {
        axis.push(' ');
        axis.push(if [1, 5, 10, 15, 20, 25, 30, 32].contains(&b) {
            '+'
        } else {
            '-'
        });
    }
    axis + "\n                 1       5        10        15        20        25        30  32\n                               Bucket Size (number of blocks)\n"
}

/// A surface plot: for each of 13 rows of a quantity (`rows`, the top
/// first, labelled every other one, `names` down the side), the depth at
/// each bucket size, `\` before the flattest and the third suggestion.
pub fn surface(rows: &[(u64, [Option<u32>; 32], u32, u32)], names: &[(usize, &str)]) -> String {
    let mut out = String::new();
    for (i, (v, depths, flat, third)) in rows.iter().enumerate() {
        let name = names.iter().find(|n| n.0 == i).map_or("", |n| n.1);
        let mut line = match i % 2 {
            0 => format!("{v:>14}|"),
            _ => format!("{name:<14}|"),
        };
        line.push(' ');
        for (b, d) in (1..).zip(depths) {
            line.push(if b == *flat || b == *third { '\\' } else { ' ' });
            line.push(d.map_or(' ', digit));
        }
        out += line.trim_end();
        out.push('\n');
    }
    out + &axis()
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
        let want = "             *|\n             9|\n             8|\nIndex        7|\n             6|\nDepth        5|\n             4|\n             3|\n             2|  2 2\n             1|      1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1\n              +- + - - - + - - - - + - - - - + - - - - + - - - - + - - - - + - +\n                 1       5        10        15        20        25        30  32\n                               Bucket Size (number of blocks)\n";
        assert_eq!(plot(&d), want);
    }
}
