//! EDT, VMS's line and keypad editor, without the terminal and the files:
//! buffers of numbered lines, line mode's commands with their output as
//! EDT prints it (fixtures/edt), and keypad mode ([`keypad`]).

pub mod keypad;

/// Line numbers in 1/100000ths: lines inserted between 1 and 2 are 1.1,
/// 1.2 ..., and between 1.1 and 1.2, 1.11 ..., to five decimals.
pub const ONE: u64 = 100_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub num: u64,
    pub text: String,
}

/// A text buffer and where its current line is (`lines.len()`: the end of
/// buffer, [EOB]); `col` is the position in that line, for SUBSTITUTE NEXT
/// and keypad mode.
#[derive(Debug, Clone, Default)]
pub struct Buffer {
    pub name: String,
    pub lines: Vec<Line>,
    pub cur: usize,
    pub col: usize,
}

impl Buffer {
    fn new(name: &str) -> Buffer {
        Buffer {
            name: name.into(),
            ..Buffer::default()
        }
    }

    /// The buffer of `text`, numbered from 1.
    pub fn of(name: &str, text: Vec<String>) -> Buffer {
        Buffer {
            lines: text
                .into_iter()
                .zip(1..)
                .map(|(text, n)| Line { num: n * ONE, text })
                .collect(),
            ..Buffer::new(name)
        }
    }

    pub fn texts(&self) -> Vec<String> {
        self.lines.iter().map(|l| l.text.clone()).collect()
    }

    /// The numbers for `k` lines inserted before line `at`: the largest of
    /// 1, 0.1, 0.01 ... steps that fits before the next line.
    fn numbers(&self, at: usize, k: usize) -> Vec<u64> {
        let p = if at == 0 { 0 } else { self.lines[at - 1].num };
        let n = self.lines.get(at).map(|l| l.num);
        let mut inc = ONE;
        while inc > 1 && n.is_some_and(|n| p + k as u64 * inc >= n) {
            inc /= 10;
        }
        if n.is_some_and(|n| p + k as u64 * inc >= n) {
            // ponytail: no room even in 0.00001 steps; EDT asks for
            // RESEQUENCE, this renumbers what follows.
            return (1..=k as u64).map(|i| p + i).collect();
        }
        (1..=k as u64).map(|i| p + i * inc).collect()
    }

    /// Inserts `texts` before line `at` as one block.
    pub fn insert(&mut self, at: usize, texts: Vec<String>) {
        let nums = self.numbers(at, texts.len());
        let fits = self
            .lines
            .get(at)
            .is_none_or(|l| nums.last().is_none_or(|n| *n < l.num));
        let new: Vec<Line> = nums
            .into_iter()
            .zip(texts)
            .map(|(num, text)| Line { num, text })
            .collect();
        let k = new.len();
        self.lines.splice(at..at, new);
        if !fits {
            self.resequence(ONE, ONE);
        }
        if self.cur >= at {
            self.cur += k;
        }
    }

    pub fn resequence(&mut self, init: u64, inc: u64) {
        for (i, l) in self.lines.iter_mut().enumerate() {
            l.num = init + i as u64 * inc;
        }
    }
}

/// A line number as EDT shows it: `    2`, `    2.1`.
pub fn number(num: u64) -> String {
    let (int, frac) = (num / ONE, num % ONE);
    let mut s = format!("{int:>5}");
    if frac > 0 {
        let f = format!("{frac:05}");
        s.push('.');
        s.push_str(f.trim_end_matches('0'));
    }
    s
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Case {
    None,
    Upper,
    Lower,
}

/// What SET changes.
#[derive(Debug, Clone)]
pub struct Settings {
    pub numbers: bool,
    pub case: Case,
    /// SET SEARCH EXACT: case matters.
    pub exact: bool,
    /// SET SEARCH END: a search leaves the cursor after the string.
    pub search_end: bool,
    pub bounded: bool,
    pub tab: Option<u16>,
    pub truncate: bool,
    pub verify: bool,
    pub wrap: Option<u16>,
    pub quiet: bool,
    pub lines: u16,
    pub screen: u16,
    pub cursor: (u16, u16),
    pub change_mode: bool,
    pub keypad: bool,
    pub word_delimiter: bool,
    /// What [EOB] says (SET TEXT END).
    pub eob: String,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            numbers: true,
            case: Case::None,
            exact: false,
            search_end: false,
            bounded: false,
            tab: None,
            truncate: true,
            verify: false,
            wrap: None,
            quiet: false,
            lines: 22,
            screen: 80,
            cursor: (7, 14),
            change_mode: false,
            keypad: true,
            word_delimiter: true,
            eob: "[EOB]".into(),
        }
    }
}

/// Files, as the host has them.
pub trait Files {
    /// A file's lines, or why not (the message EDT shows).
    fn read(&mut self, spec: &str) -> Result<Vec<String>, String>;
    /// Writes a new version of `spec`; returns its full specification.
    fn write(&mut self, spec: &str, lines: &[String]) -> Result<String, String>;
}

/// What the program does after a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    Go,
    /// Keypad mode (CHANGE, or SET MODE CHANGE at the start).
    Change,
    /// EXIT wrote the file; `save`: keep the journal.
    Exit {
        save: bool,
    },
    Quit {
        save: bool,
    },
}

/// An error at a place in the command line (the caret EDT draws under it).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Err {
    at: Option<usize>,
    msg: String,
}

fn err(at: usize, msg: &str) -> Err {
    Err {
        at: Some(at),
        msg: msg.into(),
    }
}

/// The search failed: EDT says so and shows the current line.
const NOT_FOUND: &str = "String was not found";

pub struct Edt {
    pub buffers: Vec<Buffer>,
    /// The current buffer.
    pub cur: usize,
    pub set: Settings,
    out: Vec<String>,
    /// SHOW FILES: as typed; `output` None for /READ_ONLY.
    pub input: String,
    pub output: Option<String>,
    /// SUBSTITUTE's last strings, for SUBSTITUTE NEXT.
    subs: Option<(String, String)>,
    /// The last search string (keypad FIND, FNDNXT).
    pub search: String,
    /// While a journal is replayed, counts and the like aren't said.
    pub replaying: bool,
    /// Lines being inserted before the current line (INSERT without text).
    inserting: bool,
    /// Keypad mode is possible (SYS$INPUT is a terminal).
    pub terminal: bool,
    /// Key definitions (DEFINE KEY), for keypad mode.
    pub keys: Vec<(keypad::Defined, String)>,
    /// The paste buffer's text when its last line has no end (a cut
    /// within a line).
    pub paste_open: Option<String>,
    /// Commands come from a command file: a bad one is shown above its
    /// caret (and all of them with SET VERIFY).
    pub from_file: bool,
}

impl Edt {
    /// EDT on `text`, the file named `input` (`None`: it doesn't exist).
    pub fn new(input: &str, text: Option<Vec<String>>) -> Edt {
        let mut e = Edt {
            buffers: vec![Buffer::new("MAIN"), Buffer::new("PASTE")],
            cur: 0,
            set: Settings::default(),
            out: Vec::new(),
            input: input.into(),
            output: Some(input.into()),
            subs: None,
            search: String::new(),
            replaying: false,
            inserting: false,
            terminal: false,
            keys: Vec::new(),
            paste_open: None,
            from_file: false,
        };
        match text {
            Some(t) => e.buffers[0] = Buffer::of("MAIN", t),
            None => e.say("Input file does not exist"),
        }
        e
    }

    /// What was printed since the last call.
    pub fn take(&mut self) -> Vec<String> {
        std::mem::take(&mut self.out)
    }

    pub fn print(&mut self, s: impl Into<String>) {
        self.out.push(s.into());
    }

    /// A message: not while a journal is replayed.
    fn say(&mut self, s: impl Into<String>) {
        if !self.replaying {
            self.out.push(s.into());
        }
    }

    pub fn buf(&self) -> &Buffer {
        &self.buffers[self.cur]
    }

    pub fn buf_mut(&mut self) -> &mut Buffer {
        &mut self.buffers[self.cur]
    }

    /// Line `i` of buffer `b` as TYPE shows it.
    fn shown(&self, b: usize, i: usize) -> String {
        let buf = &self.buffers[b];
        let Some(l) = buf.lines.get(i) else {
            return self.set.eob.clone();
        };
        let text = match self.set.case {
            Case::None => l.text.clone(),
            Case::Upper => flag(&l.text, char::is_uppercase),
            Case::Lower => flag(&l.text, char::is_lowercase),
        };
        match self.set.numbers {
            true => format!("{:<9}   {text}", number(l.num)),
            false => text,
        }
    }

    pub fn type_current(&mut self) {
        let s = self.shown(self.cur, self.buf().cur);
        self.print(s);
    }

    /// Runs a line-mode command (or, during INSERT, takes a line of text).
    pub fn command(&mut self, line: &str, files: &mut dyn Files) -> Flow {
        if self.inserting {
            let (text, end) = match line.find('\x1a') {
                Some(i) => (&line[..i], true),
                None => (line, false),
            };
            if !end || !text.is_empty() {
                let b = self.buf_mut();
                let at = b.cur;
                b.insert(at, vec![text.into()]);
            }
            if end {
                self.inserting = false;
                self.type_current();
            }
            return Flow::Go;
        }
        if self.from_file && self.set.verify {
            self.print(format!(" {line}"));
        }
        match self.run(line, files) {
            Ok(f) => f,
            Err(e) => {
                if self.from_file && !self.set.verify {
                    self.print(format!(" {line}"));
                }
                if let Some(at) = e.at {
                    self.print(format!("{}^", " ".repeat(at + 1)));
                }
                let current = e.msg == NOT_FOUND;
                self.print(e.msg);
                if current {
                    self.type_current();
                }
                Flow::Go
            }
        }
    }

    /// The end of input (Ctrl/Z or end of file): an insert ends; at the
    /// prompt, EDT quits.
    pub fn end_of_input(&mut self) -> Flow {
        if self.inserting {
            self.inserting = false;
            self.type_current();
        }
        self.print("");
        Flow::Quit { save: false }
    }

    fn run(&mut self, line: &str, files: &mut dyn Files) -> Result<Flow, Err> {
        let mut c = Cursor::new(line);
        c.ws();
        if c.done() {
            // Return alone: the next line.
            let b = self.buf_mut();
            b.cur = (b.cur + 1).min(b.lines.len());
            self.type_current();
            return Ok(Flow::Go);
        }
        let start = c.i;
        let word = c.word();
        // A range alone types it.
        if word.is_empty() || word.chars().all(|ch| ch.is_ascii_digit()) || is_range_word(&word) {
            c.i = start;
            return self.type_cmd(&mut c);
        }
        let cmd = COMMANDS
            .iter()
            .find(|(name, min)| word.len() >= *min && name.starts_with(&word))
            .map(|(n, _)| *n)
            .ok_or(err(start, "Unrecognized command"))?;
        match cmd {
            "TYPE" => self.type_cmd(&mut c),
            "DELETE" => self.delete(&mut c),
            "INSERT" => self.insert(&mut c, false),
            "REPLACE" => self.insert(&mut c, true),
            "FIND" => {
                let (b, idx) = self.range(&mut c, Dflt::Dot)?;
                c.end()?;
                self.cur = b;
                let buf = self.buf_mut();
                buf.cur = idx.first().copied().unwrap_or(buf.cur);
                buf.col = 0;
                Ok(Flow::Go)
            }
            "SUBSTITUTE" => self.substitute(&mut c),
            "COPY" => self.copy(&mut c, false),
            "MOVE" => self.copy(&mut c, true),
            "RESEQUENCE" => {
                let (b, idx) = self.range(&mut c, Dflt::Whole)?;
                let (init, inc) = sequence(&mut c)?;
                c.end()?;
                let n = idx
                    .iter()
                    .filter(|i| **i < self.buffers[b].lines.len())
                    .count();
                let buf = &mut self.buffers[b];
                if n == buf.lines.len() {
                    buf.resequence(init, inc);
                } else {
                    let len = buf.lines.len();
                    for (k, i) in idx.iter().filter(|i| **i < len).enumerate() {
                        buf.lines[*i].num = init + k as u64 * inc;
                    }
                }
                self.say(count(n, "line", "resequenced"));
                Ok(Flow::Go)
            }
            "WRITE" | "PRINT" => {
                let spec = c.file().ok_or(err(c.i, "File specification required"))?;
                let (b, idx) = self.range(&mut c, Dflt::Whole)?;
                c.end()?;
                let buf = &self.buffers[b];
                let lines: Vec<&Line> = idx.iter().filter_map(|i| buf.lines.get(*i)).collect();
                if cmd == "PRINT" {
                    let mut text: Vec<String> = lines
                        .iter()
                        .map(|l| format!("{:<9}   {}", number(l.num), l.text))
                        .collect();
                    // ponytail: one page; a form feed starts it.
                    if let Some(first) = text.first_mut() {
                        first.insert(0, '\x0c');
                    }
                    files
                        .write(&spec, &text)
                        .map_err(|m| Err { at: None, msg: m })?;
                } else {
                    let text: Vec<String> = lines.iter().map(|l| l.text.clone()).collect();
                    let shown = files
                        .write(&spec, &text)
                        .map_err(|m| Err { at: None, msg: m })?;
                    self.say(format!(
                        "{shown} {}",
                        count(text.len(), "line", "").trim_end()
                    ));
                }
                Ok(Flow::Go)
            }
            "INCLUDE" => {
                let spec = c.file().ok_or(err(c.i, "File specification required"))?;
                let (b, idx) = self.range(&mut c, Dflt::Dot)?;
                c.end()?;
                let text = files.read(&spec).map_err(|m| Err { at: None, msg: m })?;
                let at = idx.first().copied().unwrap_or(self.buffers[b].cur);
                self.buffers[b].insert(at, text);
                Ok(Flow::Go)
            }
            "CLEAR" => {
                c.ws();
                let at = c.i;
                let name = c.word();
                if name.is_empty() {
                    return Err(err(at, "Invalid buffer name"));
                }
                c.end()?;
                let b = self.buffer(&name);
                self.buffers[b].lines.clear();
                self.buffers[b].cur = 0;
                Ok(Flow::Go)
            }
            "EXIT" | "QUIT" => {
                let mut save = false;
                let mut spec = None;
                loop {
                    c.ws();
                    if c.eat('/') {
                        let at = c.i;
                        let q = c.word();
                        match q.as_str() {
                            "SAVE" | "SAV" => save = true,
                            "SEQUENCE" | "SEQ" if cmd == "EXIT" => {
                                sequence_values(&mut c)?;
                            }
                            _ => return Err(err(at, "Unrecognized command option")),
                        }
                    } else if cmd == "EXIT" && spec.is_none() && !c.done() {
                        spec = c.file();
                    } else {
                        break;
                    }
                }
                c.end()?;
                if cmd == "QUIT" {
                    self.print("");
                    return Ok(Flow::Quit { save });
                }
                let Some(spec) = spec.or(self.output.clone()) else {
                    return Err(Err {
                        at: None,
                        msg: "File specification required".into(),
                    });
                };
                let text = self.buffers[0].texts();
                let shown = files
                    .write(&spec, &text)
                    .map_err(|m| Err { at: None, msg: m })?;
                let n = match text.len() {
                    0 => "No lines".to_string(),
                    1 => "1 line".to_string(),
                    n => format!("{n} lines"),
                };
                self.print(format!("{shown} {n}"));
                Ok(Flow::Exit { save })
            }
            "SET" => self.set_cmd(&mut c),
            "SHOW" => self.show(&mut c),
            "FILL" => {
                let (b, idx) = self.range(&mut c, Dflt::Dot)?;
                c.end()?;
                let width = self.set.wrap.unwrap_or(self.set.screen) as usize;
                fill(&mut self.buffers[b], &idx, width);
                Ok(Flow::Go)
            }
            "TAB" => {
                c.ws();
                let at = c.i;
                if c.word() != "ADJUST" {
                    return Err(err(at, "Unrecognized command"));
                }
                c.ws();
                let neg = c.eat('-');
                let n = c.number().unwrap_or(0) as i64 * if neg { -1 } else { 1 };
                let (b, idx) = self.range(&mut c, Dflt::Dot)?;
                c.end()?;
                let tab = self.set.tab.unwrap_or(8) as i64;
                for i in idx {
                    if let Some(l) = self.buffers[b].lines.get_mut(i) {
                        l.text = indent(&l.text, n * tab);
                    }
                }
                Ok(Flow::Go)
            }
            "CHANGE" => {
                let (b, idx) = self.range(&mut c, Dflt::Dot)?;
                c.end()?;
                if !self.terminal {
                    return Err(Err {
                        at: None,
                        msg: "Change mode can be entered only from a terminal".into(),
                    });
                }
                self.cur = b;
                let buf = self.buf_mut();
                buf.cur = idx.first().copied().unwrap_or(buf.cur);
                buf.col = 0;
                Ok(Flow::Change)
            }
            "HELP" => {
                for l in HELP.lines() {
                    self.print(l);
                }
                Ok(Flow::Go)
            }
            "DEFINE" => self.define(&mut c),
            _ => Err(err(start, "Unrecognized command")),
        }
    }

    /// The buffer named `name` (created if new).
    pub fn buffer(&mut self, name: &str) -> usize {
        let name = name.to_ascii_uppercase();
        if let Some(i) = self.buffers.iter().position(|b| b.name == name) {
            return i;
        }
        self.buffers.push(Buffer::new(&name));
        self.buffers.len() - 1
    }

    fn type_cmd(&mut self, c: &mut Cursor) -> Result<Flow, Err> {
        let (b, idx) = self.range(c, Dflt::Dot)?;
        c.end()?;
        self.cur = b;
        for i in &idx {
            let s = self.shown(b, *i);
            self.print(s);
        }
        if let Some(first) = idx.first() {
            self.buffers[b].cur = *first;
            self.buffers[b].col = 0;
        }
        Ok(Flow::Go)
    }

    fn delete(&mut self, c: &mut Cursor) -> Result<Flow, Err> {
        let (b, idx) = self.range(c, Dflt::Dot)?;
        c.end()?;
        self.cur = b;
        let buf = &mut self.buffers[b];
        let gone: Vec<usize> = idx
            .iter()
            .copied()
            .filter(|i| *i < buf.lines.len())
            .collect();
        let after = gone
            .last()
            .map_or(idx.first().copied(), |l| Some(l + 1 - gone.len()));
        for i in gone.iter().rev() {
            buf.lines.remove(*i);
        }
        if let Some(a) = after {
            buf.cur = a;
        }
        buf.col = 0;
        let n = gone.len();
        self.say(match n {
            0 => "No lines deleted".into(),
            _ => count(n, "line", "deleted"),
        });
        self.type_current();
        Ok(Flow::Go)
    }

    /// INSERT [range][;text], REPLACE [range][;text].
    fn insert(&mut self, c: &mut Cursor, replace: bool) -> Result<Flow, Err> {
        let (b, idx) = self.range(c, Dflt::Dot)?;
        c.ws();
        let text = match c.eat(';') {
            true => Some(c.rest()),
            false => {
                c.end()?;
                None
            }
        };
        self.cur = b;
        let buf = &mut self.buffers[b];
        let mut at = idx.first().copied().unwrap_or(buf.cur);
        if replace {
            let gone: Vec<usize> = idx
                .iter()
                .copied()
                .filter(|i| *i < buf.lines.len())
                .collect();
            for i in gone.iter().rev() {
                buf.lines.remove(*i);
            }
            at = gone.first().copied().unwrap_or(at);
            let n = gone.len();
            self.say(match n {
                0 => "No lines deleted".into(),
                _ => count(n, "line", "deleted"),
            });
        }
        let buf = &mut self.buffers[b];
        buf.cur = at;
        match text {
            Some(t) => {
                buf.insert(at, vec![t]);
                self.type_current();
            }
            None => self.inserting = true,
        }
        Ok(Flow::Go)
    }

    fn copy(&mut self, c: &mut Cursor, mv: bool) -> Result<Flow, Err> {
        let (from, idx) = self.range(c, Dflt::Dot)?;
        c.ws();
        let at = c.i;
        if c.word() != "TO" {
            return Err(err(at, "TO expected"));
        }
        let (to, dest) = self.range(c, Dflt::Dot)?;
        c.end()?;
        let fb = &self.buffers[from];
        let src: Vec<usize> = idx.into_iter().filter(|i| *i < fb.lines.len()).collect();
        let texts: Vec<String> = src.iter().map(|i| fb.lines[*i].text.clone()).collect();
        let mut at = dest.first().copied().unwrap_or(self.buffers[to].cur);
        if mv {
            let fb = &mut self.buffers[from];
            for i in src.iter().rev() {
                fb.lines.remove(*i);
                if from == to && *i < at {
                    at -= 1;
                }
                if fb.cur > *i {
                    fb.cur -= 1;
                }
            }
        }
        let n = texts.len();
        let tb = &mut self.buffers[to];
        tb.insert(at, texts);
        tb.cur = at + n;
        let verb = if mv { "moved" } else { "copied" };
        self.say(match n {
            0 => format!("No lines {verb}"),
            _ => count(n, "line", verb),
        });
        Ok(Flow::Go)
    }

    fn substitute(&mut self, c: &mut Cursor) -> Result<Flow, Err> {
        c.ws();
        let at = c.i;
        // SUBSTITUTE NEXT [/old/new/].
        let save = c.i;
        if c.word() == "NEXT" {
            c.ws();
            if !c.done() {
                self.subs = Some(c.strings()?);
            }
            c.end()?;
            return self.substitute_next();
        }
        c.i = save;
        let (old, new) = c.strings().map_err(|_| err(at, "Unrecognized command"))?;
        let (b, idx) = self.range(c, Dflt::Dot)?;
        let (mut brief, mut notype) = (false, false);
        loop {
            c.ws();
            if !c.eat('/') {
                break;
            }
            let qa = c.i;
            match c.word().as_str() {
                "BRIEF" | "BRI" => {
                    brief = true;
                    if c.eat(':') {
                        c.number();
                    }
                }
                "NOTYPE" | "NOT" => notype = true,
                "QUERY" | "QUE" => {}
                _ => return Err(err(qa, "Unrecognized command option")),
            }
        }
        let _ = brief;
        c.end()?;
        self.subs = Some((old.clone(), new.clone()));
        self.cur = b;
        let mut total = 0;
        for i in idx {
            let Some(l) = self.buffers[b].lines.get_mut(i) else {
                continue;
            };
            let (text, n) = replace_all(&l.text, &old, &new, self.set.exact);
            if n > 0 {
                l.text = text;
                total += n;
                self.buffers[b].cur = i;
                if !notype {
                    let s = self.shown(b, i);
                    self.print(s);
                }
            }
        }
        self.say(match total {
            0 => "No substitutions".into(),
            1 => "1 substitution".into(),
            n => format!("{n} substitutions"),
        });
        Ok(Flow::Go)
    }

    /// The next occurrence of SUBSTITUTE's string from the cursor, changed.
    fn substitute_next(&mut self) -> Result<Flow, Err> {
        let (old, new) = self.subs.clone().ok_or(Err {
            at: None,
            msg: "No previous substitute".into(),
        })?;
        let exact = self.set.exact;
        let b = self.buf();
        let mut found = None;
        for i in b.cur..b.lines.len() {
            let from = if i == b.cur { b.col } else { 0 };
            if let Some(p) = find_from(&b.lines[i].text, &old, from, exact) {
                found = Some((i, p));
                break;
            }
        }
        let Some((i, p)) = found else {
            return Err(Err {
                at: None,
                msg: NOT_FOUND.into(),
            });
        };
        let b = self.buf_mut();
        b.lines[i].text.replace_range(p..p + old.len(), &new);
        b.cur = i;
        b.col = p + new.len();
        self.type_current();
        Ok(Flow::Go)
    }

    fn set_cmd(&mut self, c: &mut Cursor) -> Result<Flow, Err> {
        c.ws();
        let at = c.i;
        let what = c.word();
        let bad = err(at, "Invalid parameter for SET or SHOW");
        let s = &mut self.set;
        let on = !what.starts_with("NO");
        let base = what.strip_prefix("NO").unwrap_or(&what);
        let is = |name: &str, min: usize| base.len() >= min && name.starts_with(base);
        if is("NUMBERS", 3) {
            s.numbers = on;
        } else if is("QUIET", 1) {
            s.quiet = on;
        } else if is("VERIFY", 1) {
            s.verify = on;
        } else if is("TRUNCATE", 2) {
            s.truncate = on;
        } else if is("KEYPAD", 1) {
            s.keypad = on;
        } else if is("TAB", 2) {
            s.tab = match on {
                true => Some(c.number().ok_or(err(c.i, "Numeric value required"))? as u16),
                false => None,
            };
        } else if is("WRAP", 2) {
            s.wrap = match on {
                true => Some(c.number().ok_or(err(c.i, "Numeric value required"))? as u16),
                false => None,
            };
        } else if !on {
            return Err(bad);
        } else if is("CASE", 2) {
            c.ws();
            let wa = c.i;
            s.case = match c.word().as_str() {
                "UPPER" => Case::Upper,
                "LOWER" => Case::Lower,
                "NONE" => Case::None,
                _ => return Err(err(wa, "Invalid parameter for SET or SHOW")),
            };
        } else if is("SEARCH", 2) {
            c.ws();
            let wa = c.i;
            match c.word().as_str() {
                "GENERAL" => s.exact = false,
                "EXACT" => s.exact = true,
                "BEGIN" => s.search_end = false,
                "END" => s.search_end = true,
                "BOUNDED" => s.bounded = true,
                "UNBOUNDED" => s.bounded = false,
                _ => return Err(err(wa, "Invalid parameter for SET or SHOW")),
            }
        } else if is("LINES", 1) {
            let n = c.number().ok_or(err(c.i, "Numeric value required"))? as u16;
            if !(1..=255).contains(&n) {
                return Err(Err {
                    at: None,
                    msg: "Numeric value illegal".into(),
                });
            }
            s.lines = n;
            s.cursor.1 = s.cursor.1.min(n - 1);
            s.cursor.0 = s.cursor.0.min(s.cursor.1);
        } else if is("SCREEN", 2) {
            s.screen = c.number().ok_or(err(c.i, "Numeric value required"))? as u16;
        } else if is("CURSOR", 2) {
            let top = c.number().ok_or(err(c.i, "Numeric value required"))? as u16;
            c.ws();
            c.eat(':');
            let bottom = c.number().ok_or(err(c.i, "Numeric value required"))? as u16;
            if top > bottom || bottom >= s.lines {
                return Err(Err {
                    at: None,
                    msg: "Numeric value illegal".into(),
                });
            }
            s.cursor = (top, bottom);
        } else if is("MODE", 1) {
            c.ws();
            let wa = c.i;
            match c.word().as_str() {
                "CHANGE" | "C" => s.change_mode = true,
                "LINE" | "L" => s.change_mode = false,
                _ => return Err(err(wa, "Invalid parameter for SET or SHOW")),
            }
        } else if is("ENTITY", 2) {
            c.ws();
            let ea = c.i;
            if !matches!(
                c.word().as_str(),
                "WORD" | "SENTENCE" | "PARAGRAPH" | "PAGE"
            ) {
                return Err(err(ea, "Entity must be WORD, SENTENCE, PARAGRAPH or PAGE"));
            }
            // ponytail: the delimiters are taken, not used.
            c.string().ok_or(err(c.i, "Quoted string required"))?;
        } else if is("WORD", 1) {
            c.ws();
            let wa = c.i;
            s.word_delimiter = match c.word().as_str() {
                "DELIMITER" => true,
                "NODELIMITER" => false,
                _ => return Err(err(wa, "Invalid parameter for SET or SHOW")),
            };
        } else if is("TEXT", 2) {
            c.ws();
            let wa = c.i;
            let which = c.word();
            let t = c.string().ok_or(err(c.i, "Quoted string required"))?;
            match which.as_str() {
                "END" => s.eob = t,
                "PAGE" => {}
                _ => return Err(err(wa, "Invalid parameter for SET or SHOW")),
            }
        } else {
            return Err(bad);
        }
        c.end()?;
        if what.starts_with("MODE") && self.set.change_mode && self.terminal {
            return Ok(Flow::Change);
        }
        Ok(Flow::Go)
    }

    fn show(&mut self, c: &mut Cursor) -> Result<Flow, Err> {
        c.ws();
        let at = c.i;
        let what = c.word();
        let is = |name: &str, min: usize| what.len() >= min && name.starts_with(&what);
        let s = &self.set;
        let yes = |on: bool, w: &str| if on { w.to_string() } else { format!("no{w}") };
        let lines: Vec<String> = if is("CASE", 2) {
            vec![
                match s.case {
                    Case::None => "None",
                    Case::Upper => "Upper",
                    Case::Lower => "Lower",
                }
                .into(),
            ]
        } else if is("CURSOR", 2) {
            vec![format!("{}:{}", s.cursor.0, s.cursor.1)]
        } else if is("ENTITY", 2) {
            c.ws();
            return Err(err(c.i, "Entity must be WORD, SENTENCE, PARAGRAPH or PAGE"));
        } else if is("LINES", 1) {
            vec![s.lines.to_string()]
        } else if is("MODE", 1) {
            vec![if s.change_mode { "Change" } else { "Line" }.into()]
        } else if is("NUMBERS", 3) {
            vec![yes(s.numbers, "numbers")]
        } else if is("QUIET", 1) {
            vec![yes(s.quiet, "quiet")]
        } else if is("SCREEN", 2) {
            vec![s.screen.to_string()]
        } else if is("SEARCH", 2) {
            vec![format!(
                "{} {} {}",
                if s.exact { "exact" } else { "general" },
                if s.search_end { "end" } else { "begin" },
                if s.bounded { "bounded" } else { "unbounded" }
            )]
        } else if is("TAB", 2) {
            vec![s.tab.map_or("notab".into(), |t| format!("tab {t}"))]
        } else if is("TRUNCATE", 2) {
            vec![yes(s.truncate, "truncate")]
        } else if is("VERIFY", 1) {
            vec![yes(s.verify, "verify")]
        } else if is("WORD", 2) {
            vec![
                if s.word_delimiter {
                    "delimiter "
                } else {
                    "nodelimiter "
                }
                .into(),
            ]
        } else if is("WRAP", 2) {
            vec![s.wrap.map_or("nowrap".into(), |w| w.to_string())]
        } else if is("VERSION", 1) {
            vec![format!("V3.12-04 (vmsport {})", env!("CARGO_PKG_VERSION"))]
        } else if is("FILES", 1) {
            vec![
                format!("Input  File: {}", self.input),
                format!("Output File: {}", self.output.clone().unwrap_or_default()),
            ]
        } else if what.len() >= 4 && "KEYPAD".starts_with(&what) {
            vec![yes(s.keypad, "keypad")]
        } else if what == "KEY" {
            c.ws();
            if c.done() {
                return Err(err(c.i, "That key is not definable"));
            }
            let ka = c.i;
            let key = keypad::Defined::parse(c).ok_or(err(ka, "That key is not definable"))?;
            let def = self.keys.iter().find(|(k, _)| *k == key);
            vec![def.map_or("Key is not defined".into(), |d| d.1.clone())]
        } else if is("BUFFER", 1) {
            // The newest buffer first, MAIN and PASTE last.
            let order = (2..self.buffers.len()).rev().chain([0, 1]);
            order
                .map(|i| (i, &self.buffers[i]))
                .map(|(i, b)| {
                    let mark = if i == self.cur { '=' } else { ' ' };
                    let n = match b.lines.len() {
                        0 => "No".into(),
                        n => n.to_string(),
                    };
                    format!("{mark}{}\t{n}\tlines", b.name)
                })
                .collect()
        } else {
            return Err(err(at, "Invalid parameter for SET or SHOW"));
        };
        c.end()?;
        for l in lines {
            self.print(l);
        }
        Ok(Flow::Go)
    }

    /// DEFINE KEY [GOLD] {CONTROL x | FUNCTION n | key} AS "definition".
    /// DEFINE MACRO name makes a buffer to hold commands.
    fn define(&mut self, c: &mut Cursor) -> Result<Flow, Err> {
        c.ws();
        let at = c.i;
        match c.word().as_str() {
            "KEY" | "K" => {
                c.ws();
                let ka = c.i;
                let key = keypad::Defined::parse(c).ok_or(err(ka, "That key is not definable"))?;
                c.ws();
                let aa = c.i;
                if c.word() != "AS" {
                    return Err(err(aa, "AS expected"));
                }
                let def = c.string().ok_or(err(c.i, "Quoted string required"))?;
                c.end()?;
                self.keys.retain(|(k, _)| *k != key);
                self.keys.push((key, def));
                Ok(Flow::Go)
            }
            "MACRO" | "M" => {
                c.ws();
                let name = c.word();
                if name.is_empty() {
                    return Err(err(c.i, "Invalid buffer name"));
                }
                c.end()?;
                self.buffer(&name);
                Ok(Flow::Go)
            }
            _ => Err(err(at, "Unrecognized command")),
        }
    }

    /// A range's buffer and lines (indices; the buffer's length is [EOB]).
    fn range(&mut self, c: &mut Cursor, dflt: Dflt) -> Result<(usize, Vec<usize>), Err> {
        c.ws();
        let mut b = self.cur;
        if c.peek() == Some('=') {
            let at = c.i;
            c.i += 1;
            let name = c.word();
            if name.is_empty() {
                return Err(err(at, "Invalid buffer name"));
            }
            b = self.buffer(&name);
            c.ws();
        }
        let mut all = Vec::new();
        loop {
            let part = self.range_part(c, b, dflt)?;
            all.extend(part);
            c.ws();
            if !c.eat(',') {
                break;
            }
        }
        Ok((b, all))
    }

    fn range_part(&mut self, c: &mut Cursor, b: usize, dflt: Dflt) -> Result<Vec<usize>, Err> {
        c.ws();
        let save = c.i;
        let word = c.word();
        let buf = &self.buffers[b];
        let (n, cur) = (buf.lines.len(), buf.cur);
        let mut idx: Vec<usize> = match word.as_str() {
            "BEFORE" | "BEF" => (0..cur).collect(),
            "REST" | "RES" => (cur..=n).collect(),
            "WHOLE" | "W" | "WH" | "WHO" | "WHOL" => (0..=n).collect(),
            "ALL" => {
                c.i = save;
                (0..n).collect()
            }
            _ => {
                c.i = save;
                if self.at_range_end(c) {
                    match dflt {
                        Dflt::Dot => vec![cur],
                        Dflt::Whole => (0..=n).collect(),
                    }
                } else {
                    let first = self.element(c, b, false)?;
                    c.ws();
                    let s2 = c.i;
                    let w = c.word();
                    if c.eat(':') || w == "THRU" || w == "THR" || w == "T" {
                        let last = self.element(c, b, true)?;
                        if last < first {
                            vec![first]
                        } else {
                            (first..=last).collect()
                        }
                    } else if w == "FOR" || w == "F" {
                        c.ws();
                        let k = c.number().ok_or(err(c.i, "Numeric value required"))? as usize;
                        (first..(first + k).min(n)).collect::<Vec<_>>()
                    } else {
                        c.i = s2;
                        vec![first]
                    }
                }
            }
        };
        // [range] ALL "string".
        c.ws();
        let s3 = c.i;
        if c.word() == "ALL" {
            let at = c.i;
            let s = c.string().ok_or(err(at, "Quoted string required"))?;
            let exact = self.set.exact;
            let buf = &self.buffers[b];
            idx.retain(|i| {
                buf.lines
                    .get(*i)
                    .is_some_and(|l| find_from(&l.text, &s, 0, exact).is_some())
            });
            if idx.is_empty() {
                return Err(Err {
                    at: None,
                    msg: NOT_FOUND.into(),
                });
            }
        } else {
            c.i = s3;
        }
        Ok(idx)
    }

    fn at_range_end(&self, c: &mut Cursor) -> bool {
        c.ws();
        let save = c.i;
        let w = c.word();
        c.i = save;
        c.done() || matches!(c.peek(), Some(';' | '/' | ',')) || w == "TO"
    }

    /// One place: a number, ., BEGIN, END, "string", -"string", +n, -n,
    /// with + and - offsets. `last`: a number names the last line at or
    /// before it (the end of a range), not the first at or after it.
    fn element(&mut self, c: &mut Cursor, b: usize, last: bool) -> Result<usize, Err> {
        c.ws();
        let at = c.i;
        let exact = self.set.exact;
        let buf = &self.buffers[b];
        let (n, cur) = (buf.lines.len(), buf.cur);
        let mut i = match c.peek() {
            Some('"' | '\'') => {
                let s = c.string().ok_or(err(at, "Quoted string required"))?;
                (cur..n)
                    .find(|i| find_from(&buf.lines[*i].text, &s, 0, exact).is_some())
                    .ok_or(Err {
                        at: None,
                        msg: NOT_FOUND.into(),
                    })?
            }
            Some('-') if matches!(c.peek_at(1), Some('"' | '\'')) => {
                c.i += 1;
                let s = c.string().ok_or(err(at, "Quoted string required"))?;
                (0..=cur.min(n.saturating_sub(1)))
                    .rev()
                    .find(|i| find_from(&buf.lines[*i].text, &s, 0, exact).is_some())
                    .ok_or(Err {
                        at: None,
                        msg: NOT_FOUND.into(),
                    })?
            }
            Some('+' | '-') => cur,
            Some('.') => {
                c.i += 1;
                cur
            }
            Some(d) if d.is_ascii_digit() => {
                let num = c.line_number().ok_or(err(at, "Invalid line number"))?;
                match last {
                    false => buf.lines.iter().position(|l| l.num >= num).unwrap_or(n),
                    true => buf.lines.iter().rposition(|l| l.num <= num).unwrap_or(0),
                }
            }
            _ => match c.word().as_str() {
                "BEGIN" | "B" | "BE" | "BEG" | "BEGI" => 0,
                "END" | "E" | "EN" => n,
                "" => cur,
                _ => return Err(err(at, "Invalid range specification")),
            },
        };
        loop {
            c.ws();
            match c.peek() {
                Some('+') => {
                    c.i += 1;
                    i = (i + c.number().unwrap_or(1) as usize).min(n);
                }
                Some('-') if !matches!(c.peek_at(1), Some('"' | '\'')) => {
                    c.i += 1;
                    i = i.saturating_sub(c.number().unwrap_or(1) as usize);
                }
                _ => break,
            }
        }
        Ok(i)
    }
}

#[derive(Clone, Copy)]
enum Dflt {
    Dot,
    Whole,
}

/// Commands and the shortest abbreviation EDT takes.
const COMMANDS: &[(&str, usize)] = &[
    ("CHANGE", 1),
    ("CLEAR", 3),
    ("COPY", 2),
    ("DEFINE", 3),
    ("DELETE", 1),
    ("EXIT", 2),
    ("FILL", 3),
    ("FIND", 1),
    ("HELP", 1),
    ("INCLUDE", 3),
    ("INSERT", 1),
    ("MOVE", 1),
    ("PRINT", 2),
    ("QUIT", 4),
    ("REPLACE", 1),
    ("RESEQUENCE", 3),
    ("SET", 2),
    ("SHOW", 2),
    ("SUBSTITUTE", 1),
    ("TAB", 2),
    ("TYPE", 1),
    ("WRITE", 2),
];

fn is_range_word(w: &str) -> bool {
    matches!(w, "BEGIN" | "END" | "BEFORE" | "REST" | "WHOLE" | "ALL")
}

const HELP: &str = "\
EDT in line mode takes commands at the * prompt; a range alone types it.
Ranges: n, ., BEGIN, END, \"string\", -\"string\", +n, -n, r:r, r THRU r,
r FOR n, BEFORE, REST, WHOLE, ALL \"string\", =buffer, and lists r,r.
Commands: TYPE, INSERT [;text], REPLACE, DELETE, COPY r TO r, MOVE r TO r,
SUBSTITUTE/old/new/ [r], SUBSTITUTE NEXT, FIND, FILL, TAB ADJUST n,
RESEQUENCE, INCLUDE file, WRITE file, PRINT file, CLEAR buffer, SET, SHOW,
DEFINE KEY, CHANGE (keypad mode), EXIT [file] [/SAVE], QUIT [/SAVE].";

/// "1 line deleted", "3 lines resequenced".
fn count(n: usize, what: &str, verb: &str) -> String {
    let s = if n == 1 { "" } else { "s" };
    format!("{n} {what}{s} {verb}")
}

/// Each letter `is` picks, flagged with an apostrophe (SET CASE).
fn flag(s: &str, is: fn(char) -> bool) -> String {
    s.chars()
        .flat_map(|c| if is(c) { vec!['\'', c] } else { vec![c] })
        .collect()
}

/// Where `pat` is in `s` at or after byte `from` (case-blind unless exact).
pub fn find_from(s: &str, pat: &str, from: usize, exact: bool) -> Option<usize> {
    if pat.is_empty() || from > s.len() {
        return None;
    }
    if exact {
        return s[from..].find(pat).map(|p| p + from);
    }
    let (hs, hp) = (s.to_ascii_lowercase(), pat.to_ascii_lowercase());
    hs.get(from..)?.find(&hp).map(|p| p + from)
}

fn replace_all(s: &str, old: &str, new: &str, exact: bool) -> (String, usize) {
    let mut out = String::new();
    let (mut i, mut n) = (0, 0);
    while let Some(p) = find_from(s, old, i, exact) {
        out.push_str(&s[i..p]);
        out.push_str(new);
        i = p + old.len();
        n += 1;
    }
    out.push_str(&s[i..]);
    (out, n)
}

/// RESEQUENCE's /SEQUENCE[:init[:incr]].
fn sequence(c: &mut Cursor) -> Result<(u64, u64), Err> {
    c.ws();
    if !c.eat('/') {
        return Ok((ONE, ONE));
    }
    let at = c.i;
    if !matches!(c.word().as_str(), "SEQUENCE" | "SEQ") {
        return Err(err(at, "Unrecognized command option"));
    }
    sequence_values(c)
}

fn sequence_values(c: &mut Cursor) -> Result<(u64, u64), Err> {
    let mut v = (ONE, ONE);
    if c.eat(':') {
        v.0 = c.line_number().ok_or(err(c.i, "Numeric value required"))?;
        if c.eat(':') {
            v.1 = c.line_number().ok_or(err(c.i, "Numeric value required"))?;
        }
    }
    Ok(v)
}

/// FILL: the range's words in lines of at most `width` columns, each new
/// line put in before the first old line still there, an old line
/// deleted once its last word is out (how EDT numbers them: 0.1 ... 1.4).
fn fill(b: &mut Buffer, idx: &[usize], width: usize) {
    let lines: Vec<usize> = idx.iter().copied().filter(|i| *i < b.lines.len()).collect();
    if lines.is_empty() {
        return;
    }
    // Words, each with the old line it is in (its index in `lines`); an
    // old line is done with the last of its words (a blank one, with the
    // word before it).
    let mut words: Vec<(String, usize)> = Vec::new();
    let mut done_at: Vec<usize> = Vec::new();
    for (k, i) in lines.iter().enumerate() {
        for w in b.lines[*i].text.split_whitespace() {
            words.push((w.to_string(), k));
        }
        done_at.push(words.len());
    }
    // The text ends with the space after its last line.
    // Each new line, with how many words are out after it.
    let mut out: Vec<(String, usize)> = Vec::new();
    let mut cur = String::new();
    for (n, (w, _)) in words.iter().enumerate() {
        if !cur.is_empty() && cur.len() + 1 + w.len() > width {
            out.push((cur, n));
            cur = String::new();
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(w);
    }
    if !cur.is_empty() {
        if cur.len() < width {
            cur.push(' ');
        }
        out.push((cur, words.len()));
    }
    // Old lines by identity: indices move as lines come and go.
    let ids: Vec<u64> = lines.iter().map(|i| b.lines[*i].num).collect();
    let mut gone = 0;
    for (text, out_words) in out {
        let at = b
            .lines
            .iter()
            .position(|l| ids[gone..].contains(&l.num))
            .unwrap_or_else(|| {
                let after = ids.last().copied().unwrap_or(0);
                b.lines
                    .iter()
                    .position(|l| l.num > after)
                    .unwrap_or(b.lines.len())
            });
        b.insert(at, vec![text]);
        while gone < ids.len() && done_at[gone] <= out_words {
            if let Some(p) = b.lines.iter().position(|l| l.num == ids[gone]) {
                b.lines.remove(p);
            }
            gone += 1;
        }
    }
    b.cur = b.cur.min(b.lines.len());
}

/// `s` with its indentation moved by `cols` columns, in tabs and spaces.
fn indent(s: &str, cols: i64) -> String {
    let body = s.trim_start_matches([' ', '\t']);
    let mut col = 0i64;
    for ch in s[..s.len() - body.len()].chars() {
        col = if ch == '\t' {
            (col / 8 + 1) * 8
        } else {
            col + 1
        };
    }
    let col = (col + cols).max(0) as usize;
    format!("{}{}{body}", "\t".repeat(col / 8), " ".repeat(col % 8))
}

/// A command line being read.
pub struct Cursor {
    s: Vec<char>,
    pub i: usize,
}

impl Cursor {
    pub fn new(s: &str) -> Cursor {
        Cursor {
            s: s.chars().collect(),
            i: 0,
        }
    }

    fn ws(&mut self) {
        while self.peek().is_some_and(|c| c == ' ' || c == '\t') {
            self.i += 1;
        }
    }

    fn peek(&self) -> Option<char> {
        self.s.get(self.i).copied()
    }

    fn peek_at(&self, k: usize) -> Option<char> {
        self.s.get(self.i + k).copied()
    }

    fn done(&self) -> bool {
        self.i >= self.s.len()
    }

    fn eat(&mut self, c: char) -> bool {
        let ok = self.peek() == Some(c);
        if ok {
            self.i += 1;
        }
        ok
    }

    /// Letters (and $ _ digits after the first), upcased.
    pub fn word(&mut self) -> String {
        self.ws();
        let mut w = String::new();
        while let Some(c) = self.peek() {
            if c.is_ascii_alphabetic()
                || !w.is_empty() && (c.is_ascii_digit() || c == '$' || c == '_')
            {
                w.push(c.to_ascii_uppercase());
                self.i += 1;
            } else {
                break;
            }
        }
        w
    }

    fn number(&mut self) -> Option<u64> {
        self.ws();
        let start = self.i;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.i += 1;
        }
        (self.i > start).then(|| {
            self.s[start..self.i]
                .iter()
                .collect::<String>()
                .parse()
                .ok()
        })?
    }

    /// A line number: digits, maybe a point and up to five decimals.
    fn line_number(&mut self) -> Option<u64> {
        let int = self.number()?;
        let mut frac = 0;
        if self.peek() == Some('.') && self.peek_at(1).is_some_and(|c| c.is_ascii_digit()) {
            self.i += 1;
            let mut k = 0;
            while let Some(d) = self.peek().and_then(|c| c.to_digit(10)) {
                if k < 5 {
                    frac = frac * 10 + d as u64;
                    k += 1;
                }
                self.i += 1;
            }
            frac *= 10u64.pow(5 - k);
        }
        Some(int * ONE + frac)
    }

    /// A string in matching quotes, " or '.
    fn string(&mut self) -> Option<String> {
        self.ws();
        let q = self.peek().filter(|c| *c == '"' || *c == '\'')?;
        self.i += 1;
        let start = self.i;
        while self.peek().is_some_and(|c| c != q) {
            self.i += 1;
        }
        let s: String = self.s[start..self.i].iter().collect();
        // An unterminated string runs to the end of the line.
        self.eat(q);
        Some(s)
    }

    /// /old/new/ with any delimiter.
    fn strings(&mut self) -> Result<(String, String), Err> {
        self.ws();
        let at = self.i;
        let d = self
            .peek()
            .filter(|c| !c.is_alphanumeric() && *c != ' ')
            .ok_or(err(at, "Unrecognized command"))?;
        self.i += 1;
        let part = |c: &mut Cursor| {
            let start = c.i;
            while c.peek().is_some_and(|ch| ch != d) {
                c.i += 1;
            }
            let s: String = c.s[start..c.i].iter().collect();
            c.eat(d);
            s
        };
        let old = part(self);
        let new = part(self);
        Ok((old, new))
    }

    /// A file specification: up to a blank, / or the end.
    fn file(&mut self) -> Option<String> {
        self.ws();
        let start = self.i;
        let mut quoted = false;
        while let Some(c) = self.peek() {
            if c == '"' {
                quoted = !quoted;
            }
            if !quoted && (c == ' ' || c == '/' && self.i > start) {
                break;
            }
            self.i += 1;
        }
        (self.i > start).then(|| self.s[start..self.i].iter().collect())
    }

    fn rest(&mut self) -> String {
        let s: String = self.s[self.i..].iter().collect();
        self.i = self.s.len();
        s
    }

    fn end(&mut self) -> Result<(), Err> {
        self.ws();
        match self.done() {
            true => Ok(()),
            false => Err(err(self.i, "Unexpected characters after end of command")),
        }
    }
}
