//! EVE, the editor EDIT runs on VMS (EDIT/TPU), as a user sees it: its
//! screen, its keys, its commands and its messages, from what OpenVMS 8.4
//! did (fixtures/eve). Pure: files, DCL and subprocesses come through
//! [`Host`]; keys come in as [`Key`]s and the screen goes out as a
//! [`Grid`], so tests drive it without a terminal.
//!
//! ponytail: no TPU language (its programs, sections, the TPU and EXTEND
//! commands), no box editing, no WPS keypad; each says so when asked.

pub mod screen;
pub mod text;

pub use libvms::term::Key;
use screen::{Attr, Grid};
use std::collections::HashMap;
use text::{Pos, Text};

/// What EVE needs from the system around it.
pub trait Host {
    /// A file's lines and its full spec; Err with the spec it looked for.
    fn read(&mut self, spec: &str) -> Result<(Vec<String>, String), String>;
    /// `lines` as a new version of `spec`: the spec written, and whether
    /// the file's record format had to change; Err with a message.
    fn write(&mut self, spec: &str, lines: &[String]) -> Result<(String, bool), String>;
    /// What a DCL command says.
    fn dcl(&mut self, command: &str) -> Vec<String>;
    /// A DCL subprocess on the terminal until it logs out (SPAWN), or a
    /// command run there.
    fn spawn(&mut self, command: &str);
}

pub struct Buffer {
    pub name: String,
    pub text: Text,
    /// Where the cursor was when the buffer was last in a window.
    pub pos: Pos,
    pub modified: bool,
    /// The file it was read from, and goes back to.
    pub file: Option<String>,
    /// EXIT writes it ("Write"; else "Read-only").
    pub write: bool,
    pub modifiable: bool,
    /// EVE's own (DCL, $CHOICES$, HELP...): not written, not listed.
    pub system: bool,
}

impl Buffer {
    fn new(name: &str, lines: &[String]) -> Buffer {
        Buffer {
            name: name.to_string(),
            text: Text::from_lines(lines),
            pos: (0, 0),
            modified: false,
            file: None,
            write: true,
            modifiable: true,
            system: false,
        }
    }
}

#[derive(Debug, Clone)]
struct Window {
    buffer: usize,
    /// The first line shown.
    top: usize,
    pos: Pos,
    /// The cursor's column when it is off the text (EVE's free cursor:
    /// past a line's end, inside a tab, below the end of the buffer, where
    /// pos's line may be); typing there fills it in first.
    col: Option<usize>,
    /// First screen row, and how many rows of text (its status line is
    /// the row after them).
    row: usize,
    height: usize,
}

/// What a prompt's answer is for.
#[derive(Debug, Clone)]
enum Ask {
    Command,
    /// A command's missing argument: the command line so far.
    Arg(String),
    Find {
        forward: bool,
        wildcard: bool,
    },
    /// Replace's question at an occurrence.
    Replace(Replace),
    /// Its last question: go back for those before where it started?
    ReplaceBack(Replace),
    QuitAnyway,
    /// EXIT: a file for a buffer that has none.
    ExitFile(usize),
    /// EXIT: whether to write another modified buffer.
    ExitWrite(usize),
    /// The key for DEFINE KEY (the command), or for what LEARN learned.
    DefineKey(String),
    LearnKey(Vec<Key>),
    Help,
}

#[derive(Debug, Clone)]
struct Replace {
    old: String,
    new: String,
    count: usize,
    all: bool,
}

#[derive(Debug, Clone)]
struct Prompt {
    label: String,
    text: Vec<char>,
    ask: Ask,
}

/// How the session ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Done {
    Exit,
    Quit,
}

/// TPU's statuses at its end (fixtures/eve: EXIT and QUIT).
pub const EXIT_STATUS: u32 = 0x13F2_AF01;
pub const QUIT_STATUS: u32 = 0x13F2_AF59;

/// How the session starts: TPU's qualifiers.
#[derive(Debug, Clone)]
pub struct Start {
    pub file: String,
    pub create: bool,
    /// EXIT writes the buffer (/WRITE; /READ_ONLY is /NOWRITE/NOMODIFY).
    pub write: bool,
    pub modify: bool,
    pub output: Option<String>,
    /// (line, column), 1-based.
    pub start_position: Option<(usize, usize)>,
    /// EVE commands, one per line, and the file they came from; or the
    /// name of the one that isn't there.
    pub init: Option<Result<(Vec<String>, String), String>>,
}

impl Default for Start {
    fn default() -> Start {
        Start {
            file: String::new(),
            create: true,
            write: true,
            modify: true,
            output: None,
            start_position: None,
            init: None,
        }
    }
}

pub struct Editor {
    rows: usize,
    cols: usize,
    pub buffers: Vec<Buffer>,
    windows: Vec<Window>,
    current: usize,
    overstrike: bool,
    pub reverse: bool,
    message: String,
    prompt: Option<Prompt>,
    select: Option<Pos>,
    /// What INSERT HERE inserts.
    paste: String,
    /// What RESTORE puts back: the last text erased.
    erased: String,
    find: String,
    /// The text FIND last matched, shown bold.
    found: Option<(usize, Pos, Pos)>,
    marks: HashMap<String, (usize, Pos)>,
    keys: Vec<(Key, Binding)>,
    edt: bool,
    gold: bool,
    learning: Option<Vec<Key>>,
    recall: Vec<String>,
    repeat: Option<usize>,
    left: usize,
    right: usize,
    wrap: bool,
    tabs: usize,
    /// Windows before a DCL, choices or buffer list window opened.
    choices: Option<usize>,
    dcl_started: bool,
    host: Box<dyn Host>,
    /// No screen: messages go here, a line each, for the caller to print.
    pub nodisplay: Option<Vec<String>>,
    pub done: Option<Done>,
    /// A question just answered, as it stays on the screen when the
    /// answer ends the session.
    answered: Option<String>,
}

#[derive(Debug, Clone)]
enum Binding {
    Command(String),
    Keys(Vec<Key>),
}

const DO: Key = Key::F(16);
const HELP: Key = Key::F(15);

/// EVE's commands in the order it lists them, by first letter (choices
/// for "b", "c" ...: fixtures/eve). Those after `|` it doesn't list, but
/// takes as one-word abbreviations.
const COMMANDS: &[&str] = &[
    "Attach",
    "Bottom",
    "Buffer",
    "Box paste overstrike",
    "Box cut overstrike",
    "Box paste",
    "Box paste insert",
    "Box cut insert",
    "Box select",
    "Box cut",
    "Box copy",
    "Center line",
    "Capitalize word",
    "Cut",
    "Copy",
    "Change mode",
    "Change direction",
    "Convert tabs",
    "Delete window",
    "Do",
    "Delete",
    "Define",
    "Dcl",
    "Define key",
    "Delete buffer",
    "Define menu entry",
    "End of line",
    "Extend all",
    "Exit",
    "Enlarge window",
    "Extend this",
    "Erase start of line",
    "Erase word",
    "Extend eve",
    "Erase line",
    "Erase previous word",
    "Extend",
    "Extend tpu",
    "Erase character",
    "Fill range",
    "Find selected",
    "Find",
    "Fill paragraph",
    "Find next",
    "Forward",
    "Fill",
    "Get file",
    "Go to",
    "Help",
    "Include file",
    "Insert here",
    "Insert mode",
    "Insert page break",
    "Lowercase word",
    "Learn",
    "Line",
    "Mark",
    "Move by word",
    "Move by line",
    "Move down",
    "Move right",
    "Move left",
    "Move up",
    "Move by page",
    "New",
    "Next window",
    "Next buffer",
    "Next screen",
    "One window",
    "Other window",
    "Open",
    "Overstrike mode",
    "Paginate",
    "Paste",
    "Previous window",
    "Previous buffer",
    "Previous screen",
    "Quit",
    "Quote",
    "Restore sentence",
    "Restore line",
    "Replace",
    "Restore",
    "Restore word",
    "Restore selection",
    "Refresh",
    "Reset",
    "Restore character",
    "Recover buffer",
    "Recover buffer all",
    "Recall",
    "Recover",
    "Remove",
    "Restore box selection",
    "Return",
    "Reverse",
    "Repeat",
    "Remember",
    "Set noclipboard",
    "Set clipboard",
    "Set function keys nomotif",
    "Set function keys motif",
    "Shift right",
    "Shift left",
    "Set find nowhitespace",
    "Set find whitespace",
    "Start of line",
    "Save file",
    "Set scroll off",
    "Select all",
    "Set scroll on",
    "Set scroll smooth",
    "Set width",
    "Store text",
    "Select",
    "Shrink window",
    "Split window",
    "Set scroll jump",
    "Set scroll margins",
    "Set wildcard vms",
    "Show wildcards",
    "Save file as",
    "Show buffers",
    "Set wildcard ultrix",
    "Set backup control string",
    "Set nojournaling",
    "Set journaling",
    "Set keypad numeric",
    "Set nojournaling all",
    "Set journaling all",
    "Set keypad edt",
    "Set nofile backup",
    "Set file backup",
    "Set keypad vt100",
    "Set left margin",
    "Set right margin",
    "Set tabs",
    "Set wrap",
    "Set nowrap",
    "Set mode insert",
    "Set mode overstrike",
    "Set cursor free",
    "Set cursor bound",
    "Shift",
    "Show file backup",
    "Shell",
    "Show key",
    "Show",
    "Show system buffers",
    "Show defaults buffer",
    "Show summary",
    "Spawn",
    "Top",
    "Two windows",
    "Tab",
    "Tpu",
    "Uppercase word",
    "Undefine key",
    "Wildcard find",
    "Write file",
    "What line",
    "|",
    "Get",
    "Undefine",
];

/// The commands that match `typed` (its words abbreviate theirs), and the
/// words left over as its arguments; one of them, or the choices.
fn lookup(typed: &str) -> Result<(&'static str, String), Vec<&'static str>> {
    let words: Vec<String> = typed.split_whitespace().map(str::to_lowercase).collect();
    let listed = COMMANDS.iter().position(|c| *c == "|").unwrap();
    let mut best: Vec<(&'static str, usize)> = Vec::new();
    for (i, c) in COMMANDS.iter().enumerate() {
        if *c == "|" {
            continue;
        }
        let cw: Vec<String> = c.split(' ').map(str::to_lowercase).collect();
        let n = cw.len().min(words.len());
        if n == 0 || !(0..n).all(|k| cw[k].starts_with(&words[k])) {
            continue;
        }
        // The typed words a command takes must all be there, unless they
        // abbreviate its name (TWO for TWO WINDOWS).
        if words.len() < cw.len() && i >= listed {
            continue;
        }
        best.push((c, n));
    }
    let most = best.iter().map(|b| b.1).max().unwrap_or(0);
    best.retain(|b| b.1 == most);
    let rest = |n: usize| words[n..].join(" ");
    let original_rest = |n: usize| -> String {
        let mut s = typed.trim_start();
        for _ in 0..n {
            s = s.trim_start();
            s = s.find(char::is_whitespace).map_or("", |i| &s[i..]);
        }
        s.trim().to_string()
    };
    let _ = rest;
    let exact = best.iter().find(|b| {
        b.0.split(' ')
            .map(str::to_lowercase)
            .eq(words[..b.1].iter().cloned())
    });
    let pick = match (exact, best.len()) {
        (Some(e), _) => Some(*e),
        (None, 1) => Some(best[0]),
        _ if words.len() == 1 => {
            let one: Vec<_> = best.iter().filter(|b| !b.0.contains(' ')).collect();
            (one.len() == 1).then(|| *one[0])
        }
        _ => None,
    };
    match pick {
        Some((c, n)) => {
            let c = match c {
                "Get" | "Open" => "Get file",
                "Undefine" => "Undefine key",
                c => c,
            };
            Ok((c, original_rest(n)))
        }
        None => Err(best
            .into_iter()
            .map(|b| b.0)
            .filter(|c| COMMANDS[..listed].contains(c))
            .collect()),
    }
}

fn plural(n: usize, what: &str) -> String {
    format!("{n} {what}{}", if n == 1 { "" } else { "s" })
}

/// A buffer's name from a file spec: NAME.TYPE.
fn buffer_name(spec: &str) -> String {
    let file = spec.rsplit([']', '>', ':']).next().unwrap_or(spec);
    file.split(';').next().unwrap_or(file).to_uppercase()
}

impl Editor {
    pub fn new(rows: usize, cols: usize, host: Box<dyn Host>) -> Editor {
        Editor {
            rows,
            cols,
            buffers: Vec::new(),
            windows: Vec::new(),
            current: 0,
            overstrike: false,
            reverse: false,
            message: String::new(),
            prompt: None,
            select: None,
            paste: String::new(),
            erased: String::new(),
            find: String::new(),
            found: None,
            marks: HashMap::new(),
            keys: Vec::new(),
            edt: false,
            gold: false,
            learning: None,
            recall: Vec::new(),
            repeat: None,
            left: 1,
            right: cols - 1,
            wrap: false,
            tabs: 8,
            choices: None,
            dcl_started: false,
            host,
            nodisplay: None,
            done: None,
            answered: None,
        }
    }

    /// Reads the file and runs the initialization commands; with no
    /// display, then exits as EVE does.
    pub fn start(&mut self, s: &Start) {
        let name = buffer_name(&s.file);
        match self.host.read(&s.file) {
            Ok((lines, spec)) => {
                let mut b = Buffer::new(&name, &lines);
                b.file = Some(spec.clone());
                self.buffers.push(b);
                self.msg(&format!(
                    "{} read from file {spec}",
                    plural(lines.len(), "line")
                ));
            }
            Err(spec) => {
                let mut b = Buffer::new(&name, &[]);
                b.file = Some(spec.clone());
                self.buffers.push(b);
                self.msg(&format!("Editing new file.  Could not find: {spec}"));
                if !s.create {
                    self.msg(&format!(
                        "Input file or files do not exist: {}",
                        s.file.to_uppercase()
                    ));
                    self.done = Some(Done::Exit);
                    return;
                }
            }
        }
        if let Some(o) = &s.output {
            self.buffers[0].file = Some(o.clone());
        }
        self.buffers[0].write = s.write;
        self.buffers[0].modifiable = s.modify;
        self.windows = vec![Window {
            buffer: 0,
            top: 0,
            pos: (0, 0),
            col: None,
            row: 0,
            height: self.rows - 3,
        }];
        if let Some((l, c)) = s.start_position {
            let line = (l.max(1) - 1).min(self.buf().text.lines.len());
            self.set_pos((line, 0));
            self.win_mut().col = Some(c.max(1) - 1);
        }
        if let Some(Err(name)) = &s.init {
            self.msg(&format!("No initialization file matching: {name}"));
        }
        if let Some(Ok((cmds, spec))) = &s.init {
            self.msg(&format!(
                "Executing commands in initialization file: {spec}"
            ));
            for c in cmds {
                if c.trim().is_empty() || c.trim_start().starts_with('!') {
                    continue;
                }
                if !self.run_init(c) {
                    self.msg(&format!(
                        "Did not finish executing initialization file: {spec}"
                    ));
                    break;
                }
                if self.done.is_some() {
                    return;
                }
            }
        }
        if self.nodisplay.is_some() && self.done.is_none() {
            self.exit();
        }
    }

    fn run_init(&mut self, c: &str) -> bool {
        match lookup(c) {
            Ok((cmd, args)) => {
                self.command(cmd, &args);
                true
            }
            Err(choices) => {
                let what = if choices.is_empty() {
                    "Don't understand initialization file command"
                } else {
                    "Ambiguous initialization file command"
                };
                self.msg(&format!("{what}: {}", c.trim()));
                false
            }
        }
    }

    // Messages, buffers, windows.

    fn msg(&mut self, m: &str) {
        self.message = m.to_string();
        if let Some(out) = &mut self.nodisplay {
            out.push(m.to_string());
        }
    }

    fn win(&self) -> &Window {
        &self.windows[self.current]
    }

    fn win_mut(&mut self) -> &mut Window {
        &mut self.windows[self.current]
    }

    fn buf(&self) -> &Buffer {
        &self.buffers[self.win().buffer]
    }

    fn buf_mut(&mut self) -> &mut Buffer {
        let b = self.win().buffer;
        &mut self.buffers[b]
    }

    /// Where editing happens: the character under the cursor, the end of
    /// its line past it, the end of the buffer below that.
    fn pos(&self) -> Pos {
        let w = self.win();
        let n = self.buf().text.lines.len();
        match w.col {
            _ if w.pos.0 >= n => (n, 0),
            Some(c) => (w.pos.0, self.char_at_col(w.pos.0, c)),
            None => w.pos,
        }
    }

    fn set_pos(&mut self, p: Pos) {
        let w = self.win_mut();
        w.pos = p;
        w.col = None;
        self.scroll();
    }

    /// The free cursor's place made real, to type there: lines down to
    /// it, blanks out to it, a tab it is inside split at it.
    fn fill(&mut self) -> Pos {
        let w = self.win().clone();
        let Some(col) = w.col else {
            return self.pos();
        };
        let l = w.pos.0;
        let t = &mut self.buf_mut().text;
        while t.lines.len() < l || col > 0 && t.lines.len() == l {
            t.lines.push(Vec::new());
        }
        let c = self.char_at_col(l, col);
        let at = self.display_col((l, c));
        let n = col.saturating_sub(at);
        let t = &mut self.buf_mut().text;
        if l < t.lines.len() {
            t.lines[l].splice(c..c, std::iter::repeat_n(' ', n));
        }
        self.set_pos((l, c + n));
        (l, c + n)
    }

    /// Whether the buffer may change; if not, says so.
    fn can_change(&mut self, typed: bool) -> bool {
        if self.buf().modifiable {
            return true;
        }
        let name = self.buf().name.clone();
        self.msg(&if typed {
            format!("Attempt to change unmodifiable buffer {name}")
        } else {
            format!("Attempt to change unmodifiable buffer: {name}")
        });
        false
    }

    /// After an edit: the buffer is modified, and the other windows on it
    /// keep their places in the text.
    fn changed(&mut self) {
        self.buf_mut().modified = true;
        self.found = None;
        let b = self.win().buffer;
        let edits = std::mem::take(&mut self.buffers[b].text.edits);
        for (i, w) in self.windows.iter_mut().enumerate() {
            if i != self.current && w.buffer == b {
                w.pos = edits.iter().fold(w.pos, |q, e| e.moved(q));
            }
        }
    }

    /// Keeps the cursor's line in its window, scrolling as little as
    /// that takes.
    fn scroll(&mut self) {
        let w = self.win_mut();
        if w.pos.0 < w.top {
            w.top = w.pos.0;
        } else if w.pos.0 >= w.top + w.height {
            w.top = w.pos.0 + 1 - w.height;
        }
    }

    /// A jump (FIND, LINE, GO TO, BOTTOM) to a line off the window: it
    /// goes to the middle, the end of the buffer no higher than the
    /// window's last row.
    fn jump(&mut self, p: Pos) {
        let (top, h) = (self.win().top, self.win().height);
        let lines = self.buf().text.lines.len();
        let w = self.win_mut();
        w.pos = p;
        w.col = None;
        if p.0 < top || p.0 >= top + h {
            let last_top = (lines + 1).saturating_sub(h);
            w.top = p.0.saturating_sub(h / 2).min(last_top);
        }
        self.scroll();
    }

    /// Lays the windows out over the rows above the prompt and message
    /// rows: equal parts, the last taking what is left.
    fn layout(&mut self) {
        let n = self.windows.len();
        let total = self.rows - 2;
        let each = total / n;
        let mut row = 0;
        for (i, w) in self.windows.iter_mut().enumerate() {
            let size = if i == n - 1 { total - row } else { each };
            w.row = row;
            w.height = size - 1;
            row += size;
        }
        for i in 0..n {
            let keep = self.current;
            self.current = i;
            self.scroll();
            self.current = keep;
        }
    }

    /// Shows buffer `b` in the current window.
    fn show(&mut self, b: usize) {
        let pos = self.pos();
        let old = self.win().buffer;
        self.buffers[old].pos = pos;
        let w = self.win_mut();
        w.buffer = b;
        w.top = 0;
        let p = self.buffers[b].pos;
        self.set_pos(p);
    }

    fn find_buffer(&self, name: &str) -> Option<usize> {
        self.buffers
            .iter()
            .position(|b| b.name.eq_ignore_ascii_case(name))
    }

    fn system_buffer(&mut self, name: &str, lines: &[String]) -> usize {
        let b = match self.find_buffer(name) {
            Some(b) => {
                self.buffers[b].text = Text::from_lines(lines);
                b
            }
            None => {
                let mut nb = Buffer::new(name, lines);
                nb.system = true;
                nb.write = false;
                self.buffers.push(nb);
                self.buffers.len() - 1
            }
        };
        self.buffers[b].pos = (0, 0);
        b
    }

    /// A buffer in a window at the bottom (choices, DCL): the windows
    /// split as TWO WINDOWS does, or the choices' five rows.
    fn bottom_window(&mut self, b: usize, height: usize) {
        self.close_choices();
        let keep = self.current;
        self.choices = Some(keep);
        let total = self.rows - 2;
        let last = self.windows.len() - 1;
        let mut w = self.windows[last].clone();
        w.buffer = b;
        w.top = 0;
        w.pos = (0, 0);
        w.col = None;
        self.windows.push(w);
        if height == 0 {
            self.layout();
        } else {
            let n = self.windows.len();
            let above = total - height - 1;
            let each = above / (n - 1);
            let mut row = 0;
            for (i, w) in self.windows.iter_mut().enumerate() {
                let size = if i == n - 1 {
                    height + 1
                } else if i == n - 2 {
                    above - row
                } else {
                    each
                };
                w.row = row;
                w.height = size - 1;
                row += size;
            }
        }
        self.current = keep;
        self.scroll();
    }

    /// Closes the choices window, if one is open.
    fn close_choices(&mut self) {
        if let Some(keep) = self.choices.take()
            && self.windows.len() > 1
        {
            self.windows.pop();
            self.current = keep.min(self.windows.len() - 1);
            self.layout();
        }
    }

    // Keys.

    /// A key typed.
    pub fn key(&mut self, k: Key) {
        self.answered = None;
        if let Some(l) = &mut self.learning
            && k != Key::Ctrl('R')
        {
            l.push(k.clone());
        }
        if self.prompt.is_some() {
            self.prompt_key(k);
            return;
        }
        if let Some(n) = self.repeat.take() {
            for _ in 0..n {
                self.key_once(k.clone());
            }
            return;
        }
        self.key_once(k);
    }

    fn key_once(&mut self, k: Key) {
        if let Some((_, b)) = self.keys.iter().find(|b| b.0 == k).cloned() {
            match b {
                Binding::Command(c) => self.run(&c),
                Binding::Keys(ks) => {
                    for k in ks {
                        self.key(k);
                    }
                }
            }
            return;
        }
        if self.edt && self.edt_key(&k) {
            return;
        }
        match k {
            Key::Char(c) => self.type_char(c),
            Key::Tab => self.type_char('\t'),
            Key::Return | Key::KpEnter => self.return_key(),
            Key::Delete => self.command("Delete", ""),
            Key::Up => self.vertical(-1),
            Key::Down => self.vertical(1),
            Key::Left => self.horizontal(false),
            Key::Right => self.horizontal(true),
            Key::Find => self.command("Find", ""),
            Key::InsertHere => self.command("Insert here", ""),
            Key::Remove => self.command("Remove", ""),
            Key::Select => self.command("Select", ""),
            Key::PrevScreen => self.command("Previous screen", ""),
            Key::NextScreen => self.command("Next screen", ""),
            DO => self.ask("Command: ", Ask::Command),
            HELP => self.help(),
            Key::F(10) | Key::Ctrl('Z') => self.exit(),
            Key::F(11) => self.command("Change direction", ""),
            Key::F(12) => self.command("Move by line", ""),
            Key::Ctrl('H') | Key::Backspace => self.command("Start of line", ""),
            Key::F(13) | Key::Ctrl('J') => self.command("Erase word", ""),
            Key::F(14) | Key::Ctrl('A') => self.command("Change mode", ""),
            Key::Ctrl('B') => self.ask("Command: ", Ask::Command),
            Key::Ctrl('E') => self.command("End of line", ""),
            Key::Ctrl('L') => self.command("Insert page break", ""),
            Key::Ctrl('R') => self.command("Remember", ""),
            Key::Ctrl('U') => self.command("Erase start of line", ""),
            Key::Ctrl('W') => {}
            _ => {}
        }
    }

    /// The EDT keypad (SET KEYPAD EDT): GOLD is PF1, and F16 (ESC [29~) for
    /// keyboards without PF1 (KP7 is Do here).
    fn edt_key(&mut self, k: &Key) -> bool {
        let gold = std::mem::take(&mut self.gold);
        let c = match (k, gold) {
            (Key::Pf(1) | Key::F(16), _) => {
                self.gold = true;
                return true;
            }
            (Key::Pf(2), _) => {
                return {
                    self.help();
                    true
                };
            }
            (Key::Pf(3), false) => "Find next",
            (Key::Pf(3), true) => "Find",
            (Key::Pf(4), false) => "Erase line",
            (Key::Kp('0'), false) => "Move by line",
            (Key::Kp('1'), false) => "Move by word",
            (Key::Kp('2'), false) => "End of line",
            (Key::Kp('3'), false) => "Move right",
            (Key::Kp('4'), false) => "Forward",
            (Key::Kp('5'), false) => "Reverse",
            (Key::Kp('6'), false) => "Remove",
            (Key::Kp('6'), true) => "Insert here",
            (Key::Kp('7'), false) => {
                self.ask("Command: ", Ask::Command);
                return true;
            }
            (Key::Kp('8'), false) => "Next screen",
            (Key::Kp('9'), false) => "Erase word",
            (Key::Kp('-'), false) => "Erase word",
            (Key::Kp(','), false) => "Erase character",
            (Key::Kp('.'), false) => "Select",
            (Key::Kp('4'), true) => "Bottom",
            (Key::Kp('5'), true) => "Top",
            _ => return false,
        };
        let edt_line = c == "Erase line";
        if edt_line {
            // EDT's DEL L: from the cursor through the line's end.
            self.command("Erase line", "");
        } else {
            self.command(c, "");
        }
        true
    }

    fn type_char(&mut self, c: char) {
        if !self.can_change(true) {
            return;
        }
        let p = self.fill();
        let over = self.overstrike;
        let p = self.buf_mut().text.type_char(p, c, over);
        self.changed();
        self.set_pos(p);
        if self.wrap && c == ' ' {
            self.wrap_line();
        } else if self.wrap {
            let col = self.display_col(self.pos());
            if col > self.right {
                self.wrap_line();
            }
        }
    }

    /// SET WRAP: a word typed past the right margin starts the next line.
    fn wrap_line(&mut self) {
        let (l, c) = self.pos();
        let line: Vec<char> = self.buf().text.line(l).to_vec();
        if self.display_col((l, line.len())) <= self.right {
            return;
        }
        // Break before the last word that ends past the margin.
        let mut cut = line.len();
        while cut > 0 && line[cut - 1] != ' ' {
            cut -= 1;
        }
        if cut == 0 {
            return;
        }
        let mut start = cut;
        while start > 0 && line[start - 1] == ' ' {
            start -= 1;
        }
        let indent = " ".repeat(self.left - 1);
        let t = &mut self.buf_mut().text;
        let word: String = line[cut..].iter().collect();
        t.lines[l].truncate(start);
        t.lines
            .insert(l + 1, format!("{indent}{word}").chars().collect());
        let col = c.saturating_sub(cut) + indent.len();
        self.set_pos((l + 1, col));
    }

    fn return_key(&mut self) {
        if !self.can_change(true) {
            return;
        }
        let p = self.pos();
        let p = self.buf_mut().text.split(p);
        let indent = " ".repeat(self.left - 1);
        let p = self.buf_mut().text.insert_str(p, &indent);
        self.changed();
        self.set_pos(p);
    }

    /// The cursor's screen column.
    fn cursor_col(&self) -> usize {
        let w = self.win();
        w.col.unwrap_or_else(|| self.display_col(w.pos))
    }

    /// Up and down keep the column, whatever the line has there; down
    /// goes on below the end of the buffer to the window's last row.
    fn vertical(&mut self, d: isize) {
        let col = self.cursor_col();
        let w = self.win();
        let Some(to) = w.pos.0.checked_add_signed(d) else {
            return;
        };
        if to > self.buf().text.lines.len() && to >= w.top + w.height {
            return;
        }
        let w = self.win_mut();
        w.pos.0 = to;
        w.col = Some(col);
        self.scroll();
    }

    /// Left and right go a column, not past the line's ends to another
    /// line: from the left edge nowhere, to the right up to the screen's.
    fn horizontal(&mut self, right: bool) {
        let col = self.cursor_col();
        let col = if right {
            (col + 1).min(self.cols - 1)
        } else {
            col.saturating_sub(1)
        };
        self.win_mut().col = Some(col);
    }

    /// The screen column (0-based) of a position, tabs expanded.
    fn display_col(&self, p: Pos) -> usize {
        let line = self.buf().text.line(p.0);
        let mut col = 0;
        for &c in line.iter().take(p.1) {
            col = if c == '\t' {
                (col / self.tabs + 1) * self.tabs
            } else {
                col + 1
            };
        }
        col + p.1.saturating_sub(line.len())
    }

    /// The character at or before screen column `col` on line `l`.
    fn char_at_col(&self, l: usize, col: usize) -> usize {
        let line = self.buf().text.line(l);
        let mut c = 0;
        for (i, &ch) in line.iter().enumerate() {
            let next = if ch == '\t' {
                (c / self.tabs + 1) * self.tabs
            } else {
                c + 1
            };
            if next > col {
                return i;
            }
            c = next;
        }
        line.len()
    }

    // Prompts.

    fn ask(&mut self, label: &str, ask: Ask) {
        if self.nodisplay.is_some() {
            self.msg("Feature requires a terminal");
            return;
        }
        self.prompt = Some(Prompt {
            label: label.to_string(),
            text: Vec::new(),
            ask,
        });
        if matches!(self.prompt.as_ref().unwrap().ask, Ask::Command) {
            self.close_choices();
        }
    }

    fn prompt_key(&mut self, k: Key) {
        let p = self.prompt.as_mut().unwrap();
        match &p.ask {
            Ask::DefineKey(_) | Ask::LearnKey(_) => {
                let ask = self.prompt.take().unwrap().ask;
                self.bind(k, ask);
                return;
            }
            Ask::Help => {
                self.prompt = None;
                if k != Key::Return {
                    self.help_on(&k);
                } else {
                    self.close_choices();
                }
                return;
            }
            _ => {}
        }
        match k {
            Key::Char(c) => p.text.push(c),
            Key::Delete | Key::Backspace => {
                p.text.pop();
            }
            Key::Ctrl('U') => p.text.clear(),
            Key::Ctrl('B') | Key::Up if matches!(p.ask, Ask::Command) => {
                let i = self.recall.len().saturating_sub(1);
                if let Some(r) = self.recall.get(i) {
                    p.text = r.chars().collect();
                }
            }
            Key::Return | Key::KpEnter => {
                let p = self.prompt.take().unwrap();
                let text: String = p.text.iter().collect();
                self.answer(p, text);
            }
            // Ctrl/Z at a question is EXIT, the question left as it was.
            Key::Ctrl('Z') => {
                let p = self.prompt.take().unwrap();
                self.answered = Some(format!("{}{}", p.label, p.text.iter().collect::<String>()));
                self.exit();
            }
            Key::Ctrl('C') => {
                let p = self.prompt.take().unwrap();
                self.answer(p, String::new());
            }
            _ => {}
        }
    }

    fn answer(&mut self, p: Prompt, text: String) {
        if !matches!(p.ask, Ask::Command) {
            self.answered = Some(format!("{}{text}", p.label));
        }
        match p.ask {
            Ask::Command => {
                // Return at the Do prompt clears the message line.
                self.message.clear();
                self.close_choices();
                if !text.trim().is_empty() {
                    self.recall.push(text.clone());
                }
                self.run(&text);
            }
            Ask::Arg(line) => {
                if text.trim().is_empty() {
                    self.no_arg(&line);
                } else {
                    self.run(&format!("{line} {text}"));
                }
            }
            Ask::Find { forward, wildcard } => self.find(&text, forward, wildcard),
            Ask::Replace(r) => self.replace_answer(r, &text),
            Ask::ReplaceBack(r) => {
                if text.to_lowercase().starts_with('y') {
                    self.replace_next(r, (0, 0));
                } else {
                    self.replaced(r.count);
                }
            }
            Ask::QuitAnyway => {
                if text.is_empty() || text.to_lowercase().starts_with('y') {
                    self.done = Some(Done::Quit);
                }
            }
            Ask::ExitFile(b) => {
                if !text.is_empty() {
                    self.write_buffer(b, &text);
                }
                self.exit();
            }
            Ask::ExitWrite(b) => {
                if text.to_lowercase().starts_with('y') {
                    match self.buffers[b].file.clone() {
                        Some(f) => {
                            self.write_buffer(b, &f);
                        }
                        None => {
                            let name = self.buffers[b].name.clone();
                            return self.ask(
                                &format!(
                                    "Type filename for buffer {name} (press RETURN to not write it): "
                                ),
                                Ask::ExitFile(b),
                            );
                        }
                    }
                }
                self.exit();
            }
            Ask::DefineKey(_) | Ask::LearnKey(_) | Ask::Help => {}
        }
    }

    /// What a prompt answered with nothing does.
    fn no_arg(&mut self, line: &str) {
        let m = match lookup(line).map(|c| c.0) {
            Ok("Get file" | "Include file") => "No file specified.",
            Ok("Mark") => "Current position not marked.",
            Ok("Go to") => "No mark specified.",
            Ok("Buffer") => "No buffer specified.",
            _ => "",
        };
        self.msg(m);
    }

    /// A command line from the Do prompt, or a key bound to one.
    pub fn run(&mut self, line: &str) {
        let line = line.trim();
        if line.is_empty() {
            return;
        }
        match lookup(line) {
            Ok((cmd, args)) => self.command(cmd, &args),
            Err(choices) if choices.is_empty() => {
                self.msg(&format!("Don't understand command: {line}"));
            }
            Err(choices) => {
                self.msg(&format!("Ambiguous command name: {line}"));
                self.show_choices(&choices);
                // The Do prompt again, with what was typed, to finish it.
                if self.nodisplay.is_none() {
                    self.prompt = Some(Prompt {
                        label: "Command: ".into(),
                        text: line.chars().collect(),
                        ask: Ask::Command,
                    });
                }
            }
        }
    }

    fn show_choices(&mut self, choices: &[&str]) {
        let longest = choices.iter().map(|c| c.len()).max().unwrap_or(0);
        let ncols = (78 / (longest + 2)).min(choices.len()).max(1);
        let width = 78 / ncols;
        let lines: Vec<String> = choices
            .chunks(ncols)
            .map(|row| {
                let mut s = String::from(" ");
                for c in row {
                    s.push_str(&format!("{c:<width$}"));
                }
                s.trim_end().to_string()
            })
            .collect();
        let b = self.system_buffer("$CHOICES$", &lines);
        self.bottom_window(b, 5);
    }

    /// Asks for a command's argument, or runs it with what it has.
    fn need(&mut self, line: &str, label: &str, args: &str) -> bool {
        if args.is_empty() {
            self.ask(label, Ask::Arg(line.to_string()));
            return false;
        }
        true
    }

    // Commands.

    fn command(&mut self, cmd: &str, args: &str) {
        let args = args.trim();
        let num = |s: &str| s.parse::<usize>().ok();
        match cmd {
            "Top" => {
                if self.pos() == (0, 0) {
                    self.msg("You are already at the top of the buffer.");
                }
                self.jump((0, 0));
            }
            "Bottom" => {
                let end = (self.buf().text.lines.len(), 0);
                if self.pos() == end {
                    self.msg("You are already at the bottom of the buffer.");
                }
                self.jump(end);
            }
            "Line" => {
                if !self.need("line", "Line number: ", args) {
                    return;
                }
                let Some(n) = num(args) else {
                    self.msg(&format!("Don't understand line number: {args}"));
                    return;
                };
                let lines = self.buf().text.lines.len();
                if n > lines {
                    self.msg(&format!(
                        "Buffer has only {}.  (Now going to End of Buffer).",
                        plural(lines, "line")
                    ));
                    self.jump((lines, 0));
                } else {
                    self.set_pos((n.max(1) - 1, 0));
                }
            }
            "What line" => {
                let (l, _) = self.pos();
                let n = self.buf().text.lines.len().max(1);
                self.msg(&format!(
                    "You are on line {} out of {n} ({}%).",
                    l + 1,
                    (l + 1) * 100 / n
                ));
            }
            "Move up" => self.vertical(-1),
            "Move down" => self.vertical(1),
            "Move left" => self.horizontal(false),
            "Move right" => self.horizontal(true),
            "Start of line" => {
                let l = self.pos().0;
                self.set_pos((l, 0));
            }
            "End of line" => {
                let l = self.pos().0;
                let n = self.buf().text.line(l).len();
                self.set_pos((l, n));
            }
            "Move by line" => {
                let (l, c) = self.pos();
                let lines = self.buf().text.lines.len();
                let p = if self.reverse {
                    if c > 0 {
                        (l, 0)
                    } else {
                        (l.saturating_sub(1), 0)
                    }
                } else {
                    let n = self.buf().text.line(l).len();
                    if c < n || l >= lines {
                        (l, n)
                    } else {
                        (l + 1, self.buf().text.line(l + 1).len())
                    }
                };
                self.set_pos(p);
            }
            "Move by word" => {
                let p = self.buf().text.word_move(self.pos(), !self.reverse);
                self.set_pos(p);
            }
            "Next screen" | "Previous screen" | "Move by page" => {
                let h = self.win().height;
                let step = h.saturating_sub(2).max(1);
                let (l, _) = self.pos();
                let lines = self.buf().text.lines.len();
                let to = if cmd == "Previous screen" || cmd == "Move by page" && self.reverse {
                    l.saturating_sub(step)
                } else {
                    (l + step).min(lines)
                };
                // The end of the buffer no higher than the last row.
                let last_top = (lines + 1).saturating_sub(h);
                self.set_pos((to, 0));
                self.win_mut().top = to.min(last_top);
            }
            "Forward" => self.reverse = false,
            "Reverse" => self.reverse = true,
            "Change direction" => self.reverse = !self.reverse,
            "Change mode" => self.overstrike = !self.overstrike,
            "Insert mode" | "Set mode insert" => self.overstrike = false,
            "Overstrike mode" | "Set mode overstrike" => self.overstrike = true,
            "Find" | "Find next" | "Wildcard find" => {
                let wildcard = cmd == "Wildcard find";
                let forward = !self.reverse;
                if args.is_empty() && cmd == "Find next" {
                    let f = self.find.clone();
                    self.find(&f, forward, false);
                } else if args.is_empty() {
                    let label = match (forward, wildcard) {
                        (true, false) => "Forward Find: ",
                        (false, false) => "Reverse Find: ",
                        (true, true) => "Forward Wildcard Find: ",
                        (false, true) => "Reverse Wildcard Find: ",
                    };
                    self.message.clear();
                    self.ask(label, Ask::Find { forward, wildcard });
                } else {
                    self.find(&unquote(args), forward, wildcard);
                }
            }
            "Replace" => {
                let parts = replace_args(args);
                match parts.len() {
                    0 => self.ask("Old String: ", Ask::Arg("replace".into())),
                    1 => self.ask(
                        "New String: ",
                        Ask::Arg(format!("replace \"{}\"", parts[0])),
                    ),
                    2 => {
                        let r = Replace {
                            old: parts[0].clone(),
                            new: parts[1].clone(),
                            count: 0,
                            all: false,
                        };
                        let p = self.pos();
                        self.replace_next(r, p);
                    }
                    _ => self.msg("Replace takes only 2 arguments."),
                }
            }
            "Select" => {
                if self.select.take().is_some() {
                    self.msg("Selection canceled.");
                } else {
                    self.select = Some(self.pos());
                    self.msg("Move the text cursor to select text.");
                }
            }
            "Select all" => {
                self.select = Some((0, 0));
                let end = (self.buf().text.lines.len(), 0);
                self.set_pos(end);
            }
            "Remove" | "Cut" => {
                if let Some((a, b)) = self.selection() {
                    if !self.can_change(false) {
                        return;
                    }
                    self.paste = self.buf_mut().text.delete(a, b);
                    self.select = None;
                    self.changed();
                    self.set_pos(a);
                } else {
                    self.msg("No selection active.");
                }
            }
            "Store text" | "Copy" => {
                if let Some((a, b)) = self.selection() {
                    self.paste = self.buf().text.get(a, b);
                    self.select = None;
                } else {
                    self.msg("No selection active.");
                }
            }
            "Insert here" | "Paste" => {
                if !self.can_change(false) {
                    return;
                }
                let p = self.fill();
                let s = self.paste.clone();
                let p = self.buf_mut().text.insert_str(p, &s);
                self.changed();
                self.set_pos(p);
            }
            "Erase character" => {
                if !self.can_change(false) {
                    return;
                }
                let p = self.pos();
                let n = self.buf().text.next(p);
                self.erased = self.buf_mut().text.delete(p, n);
                self.changed();
            }
            "Delete" => {
                // Past the end of the line it only moves back.
                let (l, col) = (self.win().pos.0, self.cursor_col());
                let end = self.buf().text.line(l).len();
                if self.win().col.is_some() && col > self.display_col((l, end)) {
                    return self.horizontal(false);
                }
                if !self.can_change(true) {
                    return;
                }
                let p = self.pos();
                if p == (0, 0) {
                    return;
                }
                let a = self.buf().text.prev(p);
                self.erased = self.buf_mut().text.delete(a, p);
                self.changed();
                self.set_pos(a);
            }
            "Erase word" => {
                if !self.can_change(false) {
                    return;
                }
                let p = self.pos();
                let (a, b) = self.buf().text.word_extent(p);
                let at_end = p.1 >= self.buf().text.line(p.0).len();
                if at_end && p.0 + 1 >= self.buf().text.lines.len() {
                    return;
                }
                self.erased = self.buf_mut().text.delete(a, b);
                // Words joined across the line break keep a blank between.
                let t = &self.buf().text;
                if at_end && a.1 > 0 && a.1 < t.line(a.0).len() {
                    self.buf_mut().text.type_char(a, ' ', false);
                }
                self.changed();
                self.set_pos(a);
            }
            "Erase previous word" => {
                if !self.can_change(false) {
                    return;
                }
                // To the start of the word the cursor is in, or else of
                // the one before, then ERASE WORD.
                let p = self.pos();
                let t = &self.buf().text;
                let (a, _) = t.word_at(p);
                let to = if a < p.1 {
                    (p.0, a)
                } else {
                    t.word_move(p, false)
                };
                self.set_pos(to);
                self.command("Erase word", "");
            }
            "Erase line" => {
                if !self.can_change(false) {
                    return;
                }
                let p = self.pos();
                if p.0 >= self.buf().text.lines.len() {
                    return;
                }
                self.erased = self.buf_mut().text.delete(p, (p.0 + 1, 0));
                self.changed();
                self.set_pos(p);
            }
            "Erase start of line" => {
                if !self.can_change(false) {
                    return;
                }
                let (l, c) = self.pos();
                self.erased = self.buf_mut().text.delete((l, 0), (l, c));
                self.changed();
                self.set_pos((l, 0));
            }
            "Restore" | "Restore word" | "Restore line" | "Restore character"
            | "Restore selection" | "Restore sentence" => {
                if !self.can_change(false) {
                    return;
                }
                let p = self.pos();
                let s = self.erased.clone();
                self.buf_mut().text.insert_str(p, &s);
                self.changed();
            }
            "Uppercase word" | "Lowercase word" | "Capitalize word" => {
                if !self.can_change(false) {
                    return;
                }
                let (l, c) = self.pos();
                if l == self.buf().text.lines.len() {
                    return;
                }
                let (a, b) = self.buf().text.word_at((l, c));
                let line = &mut self.buf_mut().text.lines[l];
                for (i, ch) in line[a..b].iter_mut().enumerate() {
                    let up = match cmd {
                        "Uppercase word" => true,
                        "Lowercase word" => false,
                        _ => i == 0,
                    };
                    *ch = if up {
                        ch.to_uppercase().next().unwrap_or(*ch)
                    } else {
                        ch.to_lowercase().next().unwrap_or(*ch)
                    };
                }
                self.changed();
                let next = self.buf().text.word_move((l, b), true);
                self.set_pos(if b == a { (l, c) } else { next });
            }
            "Center line" => {
                if !self.can_change(false) {
                    return;
                }
                let l = self.pos().0;
                if l >= self.buf().text.lines.len() {
                    return;
                }
                let s: String = self.buf().text.string(l).trim().to_string();
                // As VMS centers (fixtures/eve: center, center_odd,
                // center_margins): a column left of the middle.
                let width = self.right - self.left + 1;
                let pad =
                    (self.left + width.saturating_sub(s.chars().count()) / 2).saturating_sub(2);
                self.buf_mut().text.lines[l] = format!("{}{s}", " ".repeat(pad)).chars().collect();
                self.changed();
                self.set_pos((l, pad));
            }
            "Fill" | "Fill paragraph" | "Fill range" => {
                if !self.can_change(false) {
                    return;
                }
                let (a, b) = match (cmd, self.selection()) {
                    ("Fill range" | "Fill", Some((a, b))) => {
                        self.select = None;
                        (a.0, if b.1 == 0 && b.0 > a.0 { b.0 - 1 } else { b.0 })
                    }
                    _ => {
                        let t = &self.buf().text;
                        let mut l = self.pos().0.min(t.lines.len().saturating_sub(1));
                        if t.lines.is_empty() {
                            return;
                        }
                        // At the end of the buffer: the last paragraph.
                        while l > 0 && t.string(l).trim().is_empty() {
                            l -= 1;
                        }
                        let mut a = l;
                        while a > 0 && !t.string(a - 1).trim().is_empty() {
                            a -= 1;
                        }
                        let mut b = l;
                        while b + 1 < t.lines.len() && !t.string(b + 1).trim().is_empty() {
                            b += 1;
                        }
                        (a, b)
                    }
                };
                let (left, right) = (self.left, self.right);
                let before = self.buf().text.lines.len();
                self.buf_mut().text.fill(a, b, left, right);
                let after = self.buf().text.lines.len();
                self.changed();
                let end = b + after - before;
                let n = self.buf().text.line(end).len();
                self.set_pos((end, n));
            }
            "Set left margin" => {
                if !self.need("set left margin", "Left margin: ", args) {
                    return;
                }
                match num(args) {
                    Some(n) if n >= 1 && n < self.right => {
                        self.left = n;
                        self.msg(&format!("Left margin set to: {n}"));
                    }
                    _ => self.msg(&format!(
                        "Left margin must be between 1 and {}.",
                        self.right - 1
                    )),
                }
            }
            "Set right margin" => {
                if !self.need("set right margin", "Right margin: ", args) {
                    return;
                }
                match num(args) {
                    Some(n) if n > self.left => {
                        self.right = n;
                        self.msg(&format!("Right margin set to: {n}"));
                    }
                    _ => self.msg(&format!(
                        "Right margin must be greater than left margin {}.",
                        self.left
                    )),
                }
            }
            "Set wrap" => self.wrap = true,
            "Set nowrap" => self.wrap = false,
            "Set tabs" => {
                let w: Vec<&str> = args.split_whitespace().collect();
                match w.as_slice() {
                    [e, n]
                        if "every".starts_with(&e.to_lowercase())
                            && num(n).is_some_and(|n| n > 0) =>
                    {
                        self.tabs = num(n).unwrap()
                    }
                    _ => self.msg("Specify SET TABS EVERY n."),
                }
            }
            "Tab" => self.type_char('\t'),
            "Return" => self.return_key(),
            "Insert page break" => {
                if self.can_change(false) {
                    self.type_char('\u{c}');
                    self.return_key();
                }
            }
            "Set keypad edt" => {
                self.edt = true;
                self.msg("The EDT keypad is set.");
            }
            "Set keypad numeric" | "Set keypad vt100" => self.edt = false,
            "Mark" => {
                if !self.need("mark", "Mark name: ", args) {
                    return;
                }
                let b = self.win().buffer;
                self.marks.insert(args.to_lowercase(), (b, self.pos()));
                self.msg(&format!("Current position marked as {args}."));
            }
            "Go to" => {
                if !self.need("go to", "Go to mark: ", args) {
                    return;
                }
                match self.marks.get(&args.to_lowercase()).copied() {
                    Some((b, p)) => {
                        if b != self.win().buffer {
                            self.show(b);
                        }
                        let p = (p.0.min(self.buf().text.lines.len()), p.1);
                        self.jump(p);
                    }
                    None => self.msg(&format!("No mark named {args}.")),
                }
            }
            "Two windows" | "Split window" => {
                let n = if cmd == "Two windows" {
                    2
                } else {
                    num(args).unwrap_or(2)
                };
                self.close_choices();
                if self.windows.len() > 1 && cmd == "Two windows" {
                    self.msg("Already two windows.");
                    return;
                }
                let w = self.win().clone();
                for _ in 1..n.max(2) {
                    self.windows.insert(self.current + 1, w.clone());
                }
                self.layout();
                self.current += 1;
                self.scroll();
            }
            "One window" => {
                if self.windows.len() == 1 {
                    self.msg("Already using only one window.");
                    return;
                }
                self.choices = None;
                let w = self.win().clone();
                self.windows = vec![w];
                self.current = 0;
                self.layout();
            }
            "Delete window" => {
                if self.windows.len() == 1 {
                    self.msg("You can't delete the only window.");
                    return;
                }
                self.windows.remove(self.current);
                self.current = self.current.min(self.windows.len() - 1);
                self.layout();
            }
            "Other window" | "Next window" => {
                if self.windows.len() == 1 {
                    self.msg("There is only one window.");
                    return;
                }
                self.current = (self.current + 1) % self.windows.len();
            }
            "Previous window" => {
                if self.windows.len() == 1 {
                    self.msg("There is only one window.");
                    return;
                }
                self.current = (self.current + self.windows.len() - 1) % self.windows.len();
            }
            "Enlarge window" | "Shrink window" => {
                self.msg(&format!("Not available in vmsport: {}", cmd.to_uppercase()))
            }
            "Buffer" => {
                if !self.need("buffer", "Buffer name: ", args) {
                    return;
                }
                let b = match self.find_buffer(args) {
                    Some(b) if b == self.win().buffer => {
                        self.msg(&format!(
                            "You are already in buffer: {}",
                            self.buffers[b].name
                        ));
                        return;
                    }
                    Some(b) => b,
                    None => {
                        let mut nb = Buffer::new(&args.to_uppercase(), &[]);
                        nb.write = true;
                        self.buffers.push(nb);
                        self.buffers.len() - 1
                    }
                };
                self.show(b);
            }
            "New" => {
                let name = (1..)
                    .map(|i| {
                        if i == 1 {
                            "MAIN".to_string()
                        } else {
                            format!("MAIN_{i}")
                        }
                    })
                    .find(|n| self.find_buffer(n).is_none())
                    .unwrap();
                self.buffers.push(Buffer::new(&name, &[]));
                let b = self.buffers.len() - 1;
                self.show(b);
            }
            "Next buffer" | "Previous buffer" => {
                let user: Vec<usize> = (0..self.buffers.len())
                    .filter(|b| !self.buffers[*b].system)
                    .collect();
                let at = user
                    .iter()
                    .position(|b| *b == self.win().buffer)
                    .unwrap_or(0);
                let to = if cmd == "Next buffer" {
                    user[(at + 1) % user.len()]
                } else {
                    user[(at + user.len() - 1) % user.len()]
                };
                self.show(to);
            }
            "Delete buffer" => {
                let name = if args.is_empty() {
                    self.buf().name.clone()
                } else {
                    args.to_string()
                };
                let Some(b) = self.find_buffer(&name) else {
                    self.msg(&format!("No such buffer: {name}"));
                    return;
                };
                let user = self.buffers.iter().filter(|b| !b.system).count();
                if user <= 1 {
                    self.msg("You can't delete the only buffer.");
                    return;
                }
                let name = self.buffers[b].name.clone();
                self.buffers.remove(b);
                for w in &mut self.windows {
                    if w.buffer == b {
                        w.buffer = 0;
                        w.pos = (0, 0);
                        w.col = None;
                        w.top = 0;
                    } else if w.buffer > b {
                        w.buffer -= 1;
                    }
                }
                self.msg(&format!("Deleted buffer: {name}"));
            }
            "Show buffers" => self.show_buffers(),
            "Get file" | "Include file" => {
                let line = if cmd == "Get file" {
                    "get file"
                } else {
                    "include file"
                };
                let label = if cmd == "Get file" {
                    "File to get: "
                } else {
                    "File to include: "
                };
                if !self.need(line, label, args) {
                    return;
                }
                self.get_file(cmd == "Get file", args);
            }
            "Write file" | "Save file" | "Save file as" => {
                let b = self.win().buffer;
                let spec = if args.is_empty() {
                    self.buffers[b].file.clone()
                } else {
                    Some(args.to_string())
                };
                match spec {
                    Some(s) => {
                        if self.write_buffer(b, &s) && (cmd != "Write file" || args.is_empty()) {
                            self.buffers[b].modified = false;
                        }
                    }
                    None => self.ask("File to write: ", Ask::Arg("write file".into())),
                }
            }
            "Exit" => self.exit(),
            "Quit" => {
                let modified = self
                    .buffers
                    .iter()
                    .any(|b| b.modified && b.write && !b.system);
                if modified && self.nodisplay.is_none() {
                    self.ask(
                        "Buffer modifications will not be saved, continue quitting [Yes]? ",
                        Ask::QuitAnyway,
                    );
                } else {
                    self.message.clear();
                    self.done = Some(Done::Quit);
                }
            }
            "Define key" => {
                if !self.need("define key", "Command to define: ", args) {
                    return;
                }
                self.ask(
                    "Press the key you want to define: ",
                    Ask::DefineKey(args.to_string()),
                );
            }
            "Undefine key" => self.msg("Cannot undefine a key that is not in the user key map."),
            "Learn" => {
                self.learning = Some(Vec::new());
                self.msg(
                    "Press keystrokes to be learned.  Press Ctrl/R to remember these keystrokes.",
                );
            }
            "Remember" => match self.learning.take() {
                Some(keys) => self.ask(
                    "Press the key you want to use to do what was just learned: ",
                    Ask::LearnKey(keys),
                ),
                None => self.msg("Not currently learning."),
            },
            "Repeat" => {
                if !self.need("repeat", "Number of times to repeat: ", args) {
                    return;
                }
                match num(args) {
                    Some(n) => self.repeat = Some(n),
                    None => self.msg(&format!("Don't understand repeat count: {args}")),
                }
            }
            "Recall" => self.ask("Command: ", Ask::Command),
            "Refresh" => {}
            "Dcl" => {
                if !self.need("dcl", "DCL command: ", args) {
                    return;
                }
                self.dcl(args);
            }
            "Spawn" | "Shell" => {
                self.host.spawn(args);
            }
            "Attach" => self.msg("You are not running editor in a subprocess."),
            "Help" => self.help(),
            "Do" => self.ask("Command: ", Ask::Command),
            "Tpu" | "Extend" | "Extend tpu" | "Extend eve" | "Extend all" | "Extend this"
            | "Define" => self.msg(&format!("vmsport's EVE has no TPU: {}", cmd.to_uppercase())),
            c => self.msg(&format!("Not available in vmsport: {}", c.to_uppercase())),
        }
    }

    fn selection(&self) -> Option<(Pos, Pos)> {
        let s = self.select?;
        let p = self.pos();
        Some(if s <= p { (s, p) } else { (p, s) })
    }

    fn find(&mut self, what: &str, forward: bool, wildcard: bool) {
        let what = if what.is_empty() {
            self.find.clone()
        } else {
            what.to_string()
        };
        if what.is_empty() {
            self.msg("Nothing to find.");
            return;
        }
        self.find = what.clone();
        let p = self.pos();
        let t = &self.buf().text;
        // Not where the last find left the cursor.
        let from = if forward { t.next(p) } else { t.prev(p) };
        let from = if self.found.is_some_and(|f| f.1 == p) {
            from
        } else {
            p
        };
        let hit = if wildcard {
            t.wildcard(from, &what, forward)
        } else {
            t.find(from, &what, forward)
        };
        match hit {
            Some((a, b)) => {
                self.found = Some((self.win().buffer, a, b));
                self.jump(a);
                self.msg("");
            }
            None => self.msg(&format!("Could not find: {what}")),
        }
    }

    fn replace_next(&mut self, r: Replace, from: Pos) {
        let hit = self.buf().text.find(from, &r.old, true);
        match hit {
            Some((a, b)) => {
                self.found = Some((self.win().buffer, a, b));
                self.jump(a);
                if r.all {
                    self.replace_at(r, a, b);
                } else {
                    self.ask(
                        "Replace? Type Yes, No, All, Last, or Quit: ",
                        Ask::Replace(r),
                    );
                }
            }
            None => {
                let before = self.buf().text.find((0, 0), &r.old, true);
                if before.is_some() {
                    self.ask(
                        "Found in reverse direction (may have already replaced).  Go there [N]? ",
                        Ask::ReplaceBack(r),
                    );
                } else {
                    self.replaced(r.count);
                }
            }
        }
    }

    fn replace_at(&mut self, mut r: Replace, a: Pos, b: Pos) {
        if !self.can_change(false) {
            return;
        }
        self.buf_mut().text.delete(a, b);
        let end = self.buf_mut().text.insert_str(a, &r.new);
        self.changed();
        r.count += 1;
        self.set_pos(end);
        self.replace_next(r, end);
    }

    fn replace_answer(&mut self, mut r: Replace, text: &str) {
        let Some((_, a, b)) = self.found else {
            return;
        };
        match text.to_lowercase().chars().next() {
            Some('y') => self.replace_at(r, a, b),
            Some('n') => {
                let n = self.buf().text.next(a);
                self.replace_next(r, n);
            }
            Some('a') => {
                self.msg(&format!("Replacing all occurrences of: {}", r.old));
                r.all = true;
                self.replace_at(r, a, b);
            }
            Some('l') => {
                self.buf_mut().text.delete(a, b);
                let end = self.buf_mut().text.insert_str(a, &r.new);
                self.changed();
                self.set_pos(end);
                self.replaced(r.count + 1);
            }
            _ => self.replaced(r.count),
        }
    }

    fn replaced(&mut self, n: usize) {
        self.found = None;
        self.msg(&format!("Replaced {}.", plural(n, "occurrence")));
    }

    fn get_file(&mut self, get: bool, spec: &str) {
        if get {
            let name = buffer_name(spec);
            if let Some(b) = self.find_buffer(&name) {
                if b == self.win().buffer {
                    let shown = self.buffers[b].file.clone().unwrap_or(name);
                    self.msg(&format!("You are already editing file: {shown}"));
                }
                self.show(b);
                return;
            }
        }
        match self.host.read(spec) {
            Ok((lines, full)) => {
                if get {
                    let mut b = Buffer::new(&buffer_name(spec), &lines);
                    b.file = Some(full.clone());
                    self.buffers.push(b);
                    let n = self.buffers.len() - 1;
                    self.show(n);
                } else {
                    if !self.can_change(false) {
                        return;
                    }
                    let p = self.pos();
                    let line_start = (p.0, 0);
                    let text: String = lines.iter().map(|l| format!("{l}\n")).collect();
                    self.buf_mut().text.insert_str(line_start, &text);
                    self.changed();
                }
                self.msg(&format!(
                    "{} read from file {full}",
                    plural(lines.len(), "line")
                ));
            }
            Err(full) => {
                if get {
                    let mut b = Buffer::new(&buffer_name(spec), &[]);
                    b.file = Some(full.clone());
                    self.buffers.push(b);
                    let n = self.buffers.len() - 1;
                    self.show(n);
                    self.msg(&format!("Editing new file.  Could not find: {full}"));
                } else {
                    self.msg(&format!("Could not include file: {full}"));
                }
            }
        }
    }

    fn write_buffer(&mut self, b: usize, spec: &str) -> bool {
        let lines: Vec<String> = (0..self.buffers[b].text.lines.len())
            .map(|i| self.buffers[b].text.string(i))
            .collect();
        match self.host.write(spec, &lines) {
            Ok((full, converted)) => {
                if converted {
                    self.msg("File format is being converted to a supported type");
                }
                self.msg(&format!(
                    "{} written to file {full}",
                    plural(lines.len(), "line")
                ));
                self.buffers[b].file = Some(full);
                true
            }
            Err(e) => {
                self.msg(&e);
                false
            }
        }
    }

    /// EXIT: each modified buffer written, then the end.
    fn exit(&mut self) {
        // The current buffer first; others are asked about.
        let cur = self.win().buffer;
        let mut order: Vec<usize> = (0..self.buffers.len()).collect();
        order.sort_by_key(|&b| b != cur);
        for b in order {
            let buf = &self.buffers[b];
            if !buf.modified || !buf.write || buf.system {
                continue;
            }
            let (name, file) = (buf.name.clone(), buf.file.clone());
            self.buffers[b].modified = false;
            if self.nodisplay.is_none() && b != cur {
                return self.ask(&format!("Write Buffer {name}? "), Ask::ExitWrite(b));
            }
            match file {
                Some(f) => {
                    if !self.write_buffer(b, &f) {
                        self.buffers[b].modified = true;
                        return;
                    }
                }
                None if self.nodisplay.is_some() => {}
                None => {
                    return self.ask(
                        &format!(
                            "Type filename for buffer {name} (press RETURN to not write it): "
                        ),
                        Ask::ExitFile(b),
                    );
                }
            }
        }
        // Said at the end of a procedure; on the screen it isn't seen.
        if self.dcl_started && self.nodisplay.is_some() {
            self.msg("Subprocess terminated");
        }
        self.done = Some(Done::Exit);
    }

    fn dcl(&mut self, command: &str) {
        let mut lines = vec![String::new(), command.to_string()];
        lines.extend(self.host.dcl(command));
        let b = match self.find_buffer("DCL") {
            Some(b) => {
                let t = &mut self.buffers[b].text;
                for l in &lines {
                    t.lines.push(l.chars().collect());
                }
                b
            }
            None => self.system_buffer("DCL", &lines),
        };
        if !self.windows.iter().any(|w| w.buffer == b) {
            self.bottom_window(b, 0);
            self.choices = None;
        }
        let end = self.buffers[b].text.lines.len();
        if let Some(w) = self.windows.iter_mut().find(|w| w.buffer == b) {
            w.pos = (end, 0);
            if w.pos.0 >= w.top + w.height {
                w.top = w.pos.0 + 1 - w.height;
            }
        }
        if !self.dcl_started {
            self.dcl_started = true;
            self.msg("Subprocess activated");
        }
    }

    fn show_buffers(&mut self) {
        let mut lines = vec![
            " Buffer name                   Lines  Attributes".to_string(),
            String::new(),
        ];
        for b in self.buffers.iter().filter(|b| !b.system) {
            let mut attrs = Vec::new();
            if b.modified {
                attrs.push("Modified");
            }
            if !b.write {
                attrs.push("No write");
            }
            lines.push(
                format!(
                    "  {:<30}{:>4}  {}",
                    b.name,
                    b.text.lines.len(),
                    attrs.join(", ")
                )
                .trim_end()
                .to_string(),
            );
        }
        let b = self.system_buffer("BUFFER LIST", &lines);
        self.buffers[b].pos = (2, 2);
        self.show(b);
    }

    fn bind(&mut self, k: Key, ask: Ask) {
        match ask {
            Ask::DefineKey(c) => {
                self.keys.retain(|b| b.0 != k);
                self.keys.push((k, Binding::Command(c)));
                self.msg("Key defined.");
            }
            Ask::LearnKey(keys) => {
                self.keys.retain(|b| b.0 != k);
                self.keys.push((k, Binding::Keys(keys)));
                self.msg("Key sequence remembered.");
            }
            _ => {}
        }
    }

    /// HELP: vmsport's own keypad diagram and words (not EVE's text).
    fn help(&mut self) {
        let lines: Vec<String> = HELP_TEXT.lines().map(str::to_string).collect();
        let b = self.system_buffer("HELP", &lines);
        self.bottom_window(b, self.rows - 4);
        self.ask(
            "Press the key that you want help on (RETURN to exit help): ",
            Ask::Help,
        );
    }

    fn help_on(&mut self, k: &Key) {
        let what = match *k {
            DO => "Do: type an EVE command and press Return.",
            Key::Find => "Find: searches for text, in the current direction.",
            Key::InsertHere => "Insert Here: inserts what Remove or Store Text took.",
            Key::Remove => "Remove: cuts the selected text.",
            Key::Select => "Select: starts (or cancels) a selection at the cursor.",
            Key::PrevScreen | Key::NextScreen => "Prev/Next Screen: moves a window's worth.",
            Key::F(10) | Key::Ctrl('Z') => "Exit: writes what changed and ends the session.",
            Key::F(11) => "F11: changes the direction, Forward or Reverse.",
            Key::F(12) => "F12: to the start or end of the line, by direction.",
            Key::F(13) => "F13: erases a word.",
            Key::F(14) => "F14: changes the mode, Insert or Overstrike.",
            _ => "No help for that key.",
        };
        self.msg(what);
        self.ask(
            "Press the key that you want help on (RETURN to exit help): ",
            Ask::Help,
        );
    }

    // The screen.

    /// The screen as it is now.
    pub fn grid(&self) -> Grid {
        let mut g = Grid::new(self.rows, self.cols);
        for (i, w) in self.windows.iter().enumerate() {
            self.draw_window(&mut g, w, i == self.current);
        }
        let prompt_row = self.rows - 2;
        if let Some(p) = &self.prompt {
            let a = if matches!(p.ask, Ask::Command) {
                Attr::Normal
            } else {
                Attr::Reverse
            };
            g.put(prompt_row, 0, &p.label, a);
            let text: String = p.text.iter().collect();
            g.put(prompt_row, p.label.chars().count(), &text, Attr::Normal);
            g.cursor = (
                prompt_row,
                (p.label.chars().count() + p.text.len()).min(self.cols - 1),
            );
        }
        if let Some(a) = self.answered.as_ref().filter(|_| self.done.is_some()) {
            g.put(prompt_row, 0, a, Attr::Normal);
        }
        g.put(self.rows - 1, 0, &self.message, Attr::Normal);
        if self.done.is_some() {
            g.cursor = (self.rows - 1, 0);
        }
        g
    }

    fn draw_window(&self, g: &mut Grid, w: &Window, current: bool) {
        let b = &self.buffers[w.buffer];
        let sel = if current { self.selection() } else { None };
        for r in 0..w.height {
            let l = w.top + r;
            let row = w.row + r;
            // EVE's own buffers have no end-of-file line.
            if l == b.text.lines.len() && !b.system {
                g.put(row, 0, "[End of file]", Attr::Normal);
                continue;
            }
            if l > b.text.lines.len() {
                continue;
            }
            let mut col = 0;
            let line = b.text.line(l);
            for (i, &c) in line.iter().enumerate() {
                let attr = match (sel, self.found) {
                    (Some((a, z)), _) if (l, i) >= a && (l, i) < z => Attr::Reverse,
                    (_, Some((fb, a, z))) if fb == w.buffer && (l, i) >= a && (l, i) < z => {
                        Attr::Bold
                    }
                    _ => Attr::Normal,
                };
                let shown: String = match c {
                    '\t' => " ".repeat((col / self.tabs + 1) * self.tabs - col),
                    // FF, CR, LF, VT as their symbols.
                    '\u{b}'..='\r' => char::from_u32(0x2400 + c as u32).unwrap().to_string(),
                    c if (c as u32) < 32 => format!("^{}", (c as u8 + 64) as char),
                    c => c.to_string(),
                };
                for ch in shown.chars() {
                    if col >= self.cols - 1
                        && (col > self.cols - 1 || i + 1 < line.len() || shown.len() > 1)
                    {
                        // A line going on past the edge ends in a diamond.
                        g.put(row, self.cols - 1, "\u{25c6}", Attr::Normal);
                        col = usize::MAX / 2;
                        break;
                    }
                    g.put(row, col, &ch.to_string(), attr);
                    col += 1;
                }
                if col == usize::MAX / 2 {
                    break;
                }
            }
        }
        // The status line.
        let status_row = w.row + w.height;
        let status = self.status(b, current);
        g.put(
            status_row,
            0,
            &format!("{status:<width$}", width = self.cols),
            Attr::Reverse,
        );
        if current {
            let c = self.cursor_col().min(self.cols - 1);
            g.cursor = (w.row + w.pos.0 - w.top, c);
        }
    }

    fn status(&self, b: &Buffer, _current: bool) -> String {
        let left = format!(" Buffer: {}", b.name);
        let right = match b.name.as_str() {
            "$CHOICES$" if b.text.lines.len() > 5 => {
                "To see more, use: | Prev Screen | Next Screen".to_string()
            }
            "$CHOICES$" | "HELP" => String::new(),
            "BUFFER LIST" => "Use SELECT to view or REMOVE to delete buffers ".to_string(),
            _ => format!(
                "| {} | {} | {} ",
                if b.write { "Write" } else { "Read-only" },
                if !b.modifiable {
                    "Unmodifiable"
                } else if self.overstrike {
                    "Overstrike"
                } else {
                    "Insert"
                },
                if self.reverse { "Reverse" } else { "Forward" }
            ),
        };
        let width = self.cols.saturating_sub(right.len());
        let left = if b.name == "BUFFER LIST" {
            format!("{left:<33}")
        } else {
            format!("{left:<width$}")
        };
        format!("{left}{right}")
    }
}

/// Arguments as EVE takes them: words, or "quoted strings".
fn split_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cs = s.chars().peekable();
    while let Some(&c) = cs.peek() {
        if c.is_whitespace() {
            cs.next();
            continue;
        }
        let mut w = String::new();
        if c == '"' {
            cs.next();
            while let Some(c) = cs.next() {
                if c == '"' {
                    if cs.peek() == Some(&'"') {
                        cs.next();
                    } else {
                        break;
                    }
                }
                w.push(c);
            }
        } else {
            while let Some(&c) = cs.peek() {
                if c.is_whitespace() {
                    break;
                }
                w.push(c);
                cs.next();
            }
        }
        out.push(w);
    }
    out
}

/// REPLACE's two strings: quoted, or the first word and then the rest.
fn replace_args(s: &str) -> Vec<String> {
    let s = s.trim();
    if s.starts_with('"') {
        return split_args(s);
    }
    match s.split_once(char::is_whitespace) {
        Some((a, b)) if !b.trim().starts_with('"') => vec![a.to_string(), b.trim().to_string()],
        Some((a, b)) => [vec![a.to_string()], split_args(b)].concat(),
        None if s.is_empty() => Vec::new(),
        None => vec![s.to_string()],
    }
}

fn unquote(s: &str) -> String {
    split_args(s).join(" ")
}

const HELP_TEXT: &str = "
    vmsport's EVE: the keys

    F10 or Ctrl/Z  exit, writing what changed      Do (F16)  an EVE command
    F11  Forward / Reverse                         Help (F15) this
    F12  to the start or end of the line
    F13  erase a word                              Find    Insert Here  Remove
    F14  Insert / Overstrike                       Select  Prev Screen  Next Screen

    Ctrl/A  Insert / Overstrike   Ctrl/E  end of line    Ctrl/H  start of line
    Ctrl/J  erase a word          Ctrl/U  erase to the line's start
    Ctrl/R  remember what LEARN learned            Ctrl/W  redraw

    Commands at Do: TOP, BOTTOM, LINE n, FIND, REPLACE, SELECT, REMOVE,
    INSERT HERE, ERASE WORD, FILL, CENTER LINE, TWO WINDOWS, BUFFER,
    GET FILE, INCLUDE FILE, WRITE FILE, DCL, LEARN, DEFINE KEY, EXIT, QUIT.
    An abbreviation will do; when it fits several, EVE lists them.
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abbreviations_as_eve_takes_them() {
        assert_eq!(lookup("bot").unwrap().0, "Bottom");
        assert_eq!(lookup("line 2").unwrap(), ("Line", "2".to_string()));
        assert_eq!(lookup("n").unwrap().0, "New");
        assert_eq!(lookup("ne").unwrap().0, "New");
        assert_eq!(lookup("g").unwrap().0, "Get file");
        assert_eq!(lookup("up").unwrap().0, "Uppercase word");
        assert_eq!(
            lookup("set right margin 20").unwrap(),
            ("Set right margin", "20".to_string())
        );
        assert_eq!(
            lookup("replace line row").unwrap(),
            ("Replace", "line row".to_string())
        );
        assert_eq!(lookup("q").unwrap_err(), ["Quit", "Quote"]);
        assert_eq!(lookup("x").unwrap_err(), Vec::<&str>::new());
        assert_eq!(lookup("two").unwrap().0, "Two windows");
        assert_eq!(lookup("erase w").unwrap().0, "Erase word");
    }
}
