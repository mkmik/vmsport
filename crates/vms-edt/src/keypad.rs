//! Keypad (change) mode: EDT's VT100 keypad, the arrow and control keys,
//! and the screen as EDT draws it, kept as a model ([`Screen`]) that the
//! program puts on the terminal. From EDT's documented keypad; VMS can't be
//! recorded at a console here (fixtures/edt/README).
//!
//! ```text
//!  PF1 GOLD      PF2 HELP       PF3 FNDNXT      PF4 DEL L
//!                               (gold) FIND     (gold) UND L
//!  7 PAGE        8 SECT         9 APPEND        - DEL W
//!  (gold) COMMAND (gold) FILL   (gold) REPLACE  (gold) UND W
//!  4 ADVANCE     5 BACKUP       6 CUT           , DEL C
//!  (gold) BOTTOM (gold) TOP     (gold) PASTE    (gold) UND C
//!  1 WORD        2 EOL          3 CHAR          ENTER
//!  (gold) CHNGCASE (gold) DEL EOL (gold) SPECINS (gold) SUBS
//!  0 LINE                       . SELECT
//!  (gold) OPEN LINE             (gold) RESET
//! ```

use crate::{Cursor, Edt, Files, Flow, ONE};
use libvms::term::Key;

/// A key DEFINE KEY can name: GOLD or not, and the key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Defined {
    pub gold: bool,
    pub key: Key,
}

impl Defined {
    /// `[GOLD] CONTROL x` or `[GOLD] FUNCTION n`: keypad 0-9 are 0-9, 10
    /// is the period, 11 the comma, 12 the minus, 13 ENTER, 14-17 the
    /// arrows (up, down, right, left), 18-21 PF1-PF4.
    pub fn parse(c: &mut Cursor) -> Option<Defined> {
        let mut gold = false;
        let mut w = c.word();
        if w == "GOLD" {
            gold = true;
            w = c.word();
        }
        let key = match w.as_str() {
            "CONTROL" => {
                c.ws();
                let ch = c.peek()?.to_ascii_uppercase();
                c.i += 1;
                if !ch.is_ascii_alphabetic() {
                    return None;
                }
                Key::Ctrl(ch)
            }
            "FUNCTION" => {
                let n = c.number()?;
                match n {
                    0..=9 => Key::Kp((b'0' + n as u8) as char),
                    10 => Key::Kp('.'),
                    11 => Key::Kp(','),
                    12 => Key::Kp('-'),
                    13 => Key::KpEnter,
                    14 => Key::Up,
                    15 => Key::Down,
                    16 => Key::Right,
                    17 => Key::Left,
                    18..=21 => Key::Pf((n - 17) as u8),
                    _ => return None,
                }
            }
            _ => return None,
        };
        // GOLD itself can't be redefined.
        (key != Key::Pf(1)).then_some(Defined { gold, key })
    }
}

/// The screen: rows of text, which columns of each are selected (shown in
/// reverse video), and the cursor (row, column).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Screen {
    pub rows: Vec<String>,
    pub reverse: Vec<Option<(usize, usize)>>,
    pub cursor: (usize, usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Prompt {
    Command(String),
    Search(String),
}

/// What a key did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum After {
    Stay,
    /// Ctrl/Z: back to line mode.
    LineMode,
    /// EXIT or QUIT from the COMMAND prompt.
    Done(Flow),
}

/// Keypad mode's state.
#[derive(Debug, Default)]
pub struct Keypad {
    gold: bool,
    count: Option<usize>,
    /// Digits typed after GOLD go on making the count.
    counting: bool,
    backup: bool,
    select: Option<(usize, usize)>,
    und_char: String,
    und_word: String,
    und_line: String,
    prompt: Option<Prompt>,
    pub message: String,
    /// The first buffer line on the screen.
    top: usize,
    /// The display column up and down arrows keep to.
    goal: Option<usize>,
    help: bool,
}

const SECT: usize = 16;

impl Keypad {
    pub fn new() -> Keypad {
        Keypad::default()
    }

    /// One key.
    pub fn key(&mut self, e: &mut Edt, k: Key, files: &mut dyn Files) -> After {
        self.message.clear();
        if self.help {
            self.help = false;
            return After::Stay;
        }
        if let Some(p) = self.prompt.take() {
            return self.prompted(e, p, k, files);
        }
        // A defined key does what its definition says.
        let def = e
            .keys
            .iter()
            .find(|(d, _)| d.gold == self.gold && d.key == k)
            .map(|d| d.1.clone());
        if let Some(def) = def {
            self.gold = false;
            return self.nokeypad(e, &def, files);
        }
        let gold = std::mem::take(&mut self.gold);
        if !matches!(k, Key::Up | Key::Down) {
            self.goal = None;
        }
        match (gold, &k) {
            (_, Key::Pf(1)) => {
                self.gold = true;
                return After::Stay;
            }
            // GOLD and digits: a count for the next key (itself GOLDed or
            // not); the digits after the first need no GOLD.
            (g, Key::Char(d)) if d.is_ascii_digit() && (g || self.counting) => {
                let n = self.count.unwrap_or(0) * 10 + d.to_digit(10).unwrap() as usize;
                self.count = Some(n);
                self.counting = true;
                self.message = format!("Repeat: {n}");
                return After::Stay;
            }
            _ => {}
        }
        self.counting = false;
        let n = self.count.take().unwrap_or(1).max(1);
        let times = if gold && matches!(k, Key::Kp('3')) {
            1
        } else {
            n
        };
        for _ in 0..times {
            match self.act(e, gold, &k, n) {
                After::Stay => {}
                other => return other,
            }
        }
        self.keep_visible(e);
        After::Stay
    }

    fn act(&mut self, e: &mut Edt, gold: bool, k: &Key, n: usize) -> After {
        let fwd = !self.backup;
        match (gold, k) {
            (false, Key::Pf(2)) | (true, Key::Pf(2)) => self.help = true,
            (false, Key::Pf(3)) => self.find_next(e),
            (true, Key::Pf(3)) => self.prompt = Some(Prompt::Search(String::new())),
            (false, Key::Pf(4)) => {
                let (l, c) = pos(e);
                let end = if l < e.buf().lines.len() {
                    (l + 1, 0)
                } else {
                    (l, c)
                };
                self.und_line = cut(e, (l, c), end);
            }
            (true, Key::Pf(4)) => {
                let t = self.und_line.clone();
                insert_text(e, &t);
            }
            (false, Key::Kp('7')) => move_page(e, fwd),
            // COMMAND; also on Do (F16, ESC [29~), which EDT leaves free.
            (true, Key::Kp('7')) | (_, Key::F(16)) => {
                self.prompt = Some(Prompt::Command(String::new()))
            }
            (false, Key::Kp('8')) => move_lines(e, fwd, SECT),
            (true, Key::Kp('8')) => {
                if let Some((s, _)) = self.range(e) {
                    let (from, to) = (s.0, pos(e).0.max(s.0));
                    let idx: Vec<usize> = (from.min(to)..=from.max(to)).collect();
                    let width = e.set.wrap.map_or(e.set.screen as usize - 1, |w| w as usize);
                    crate::fill(e.buf_mut(), &idx, width);
                    self.select = None;
                }
            }
            (false, Key::Kp('9')) => {
                if let Some((s, t)) = self.range(e) {
                    let text = cut(e, s, t);
                    let mut p = paste_text(e);
                    p.push_str(&text);
                    set_paste(e, &p);
                    self.select = None;
                }
            }
            (true, Key::Kp('9')) => {
                if let Some((s, t)) = self.range(e) {
                    cut(e, s, t);
                    self.select = None;
                    let p = paste_text(e);
                    insert_text(e, &p);
                }
            }
            (false, Key::Kp('-')) => {
                let (s, t) = (pos(e), word_end(e));
                self.und_word = cut(e, s, t);
            }
            (true, Key::Kp('-')) => {
                let t = self.und_word.clone();
                insert_text(e, &t);
            }
            (false, Key::Kp('4')) => self.backup = false,
            (false, Key::Kp('5')) => self.backup = true,
            (true, Key::Kp('4')) => {
                let b = e.buf_mut();
                (b.cur, b.col) = (b.lines.len(), 0);
            }
            (true, Key::Kp('5')) => {
                let b = e.buf_mut();
                (b.cur, b.col) = (0, 0);
            }
            (false, Key::Kp('6')) => match self.range(e) {
                Some((s, t)) => {
                    let text = cut(e, s, t);
                    set_paste(e, &text);
                    self.select = None;
                }
                None => self.message = "No select range active".into(),
            },
            (true, Key::Kp('6')) => {
                let p = paste_text(e);
                insert_text(e, &p);
            }
            (false, Key::Kp(',')) => {
                let p = pos(e);
                let t = step_char(e, p, true);
                self.und_char = cut(e, p, t);
            }
            (true, Key::Kp(',')) => {
                let t = self.und_char.clone();
                insert_text(e, &t);
            }
            (false, Key::Kp('1')) => move_word(e, fwd),
            (true, Key::Kp('1')) => self.change_case(e, fwd),
            (false, Key::Kp('2')) => move_eol(e, fwd),
            (true, Key::Kp('2')) => {
                let (l, c) = pos(e);
                if let Some(line) = e.buf().lines.get(l) {
                    let end = (l, line.text.len());
                    self.und_line = cut(e, (l, c), end);
                }
            }
            (false, Key::Kp('3')) => {
                let t = step_char(e, pos(e), fwd);
                set_pos(e, t);
            }
            (true, Key::Kp('3')) => {
                if let Some(ch) = char::from_u32(n as u32) {
                    insert_text(e, &ch.to_string());
                }
            }
            (false, Key::Kp('0')) => move_line(e, fwd),
            (true, Key::Kp('0')) => {
                insert_text(e, "\n");
                let t = step_char(e, pos(e), false);
                set_pos(e, t);
            }
            (false, Key::Kp('.')) => match self.select {
                Some(_) => self.message = "Select range is already active".into(),
                None => self.select = Some(pos(e)),
            },
            (true, Key::Kp('.')) => {
                self.select = None;
                self.backup = false;
            }
            (true, Key::KpEnter) => self.subs(e),
            (_, Key::KpEnter) => {}
            (_, Key::Up | Key::Down) => {
                let (l, c) = pos(e);
                let goal = *self
                    .goal
                    .get_or_insert_with(|| col_of(e.buf().lines.get(l).map_or("", |x| &x.text), c));
                let n = e.buf().lines.len();
                let to = if *k == Key::Up {
                    l.saturating_sub(1)
                } else {
                    (l + 1).min(n)
                };
                let text = e
                    .buf()
                    .lines
                    .get(to)
                    .map_or(String::new(), |x| x.text.clone());
                set_pos(e, (to, byte_at(&text, goal)));
            }
            (_, Key::Right) => {
                let t = step_char(e, pos(e), true);
                set_pos(e, t);
            }
            (_, Key::Left) => {
                let t = step_char(e, pos(e), false);
                set_pos(e, t);
            }
            (_, Key::Return) => insert_text(e, "\n"),
            (_, Key::Tab) => match e.set.tab {
                Some(t) if pos(e).1 == 0 => {
                    let s = crate::indent("", (e.set.tab_level * t as usize) as i64);
                    insert_text(e, &s);
                }
                _ => insert_text(e, "\t"),
            },
            (_, Key::Delete) => {
                let p = pos(e);
                let s = step_char(e, p, false);
                self.und_char = cut(e, s, p);
            }
            (_, Key::Backspace) => {
                let (l, c) = pos(e);
                set_pos(e, if c > 0 || l == 0 { (l, 0) } else { (l - 1, 0) });
            }
            (_, Key::Ctrl('J')) => {
                let p = pos(e);
                let s = word_start(e);
                self.und_word = cut(e, s, p);
            }
            (_, Key::Ctrl('U')) => {
                let (l, c) = pos(e);
                self.und_line = cut(e, (l, 0), (l, c));
            }
            (_, Key::Ctrl('L')) => insert_text(e, "\x0c"),
            (_, Key::Ctrl('W') | Key::Ctrl('R')) => {}
            (_, Key::Ctrl('Z')) => return After::LineMode,
            (_, Key::Ctrl('D')) => e.set.tab_level = e.set.tab_level.saturating_sub(1),
            (_, Key::Ctrl('E')) => e.set.tab_level += 1,
            (_, Key::Ctrl('A')) => {
                let (l, _) = pos(e);
                let text = e
                    .buf()
                    .lines
                    .get(l)
                    .map_or(String::new(), |x| x.text.clone());
                let lead = col_of(&text, text.len() - text.trim_start().len());
                e.set.tab_level = lead / e.set.tab.unwrap_or(8).max(1) as usize;
            }
            (_, Key::Ctrl('T')) => {
                if let Some((s, t)) = self.range(e) {
                    let tab = e.set.tab.unwrap_or(8) as i64;
                    let b = e.buf_mut();
                    for i in s.0..=t.0.min(b.lines.len().saturating_sub(1)) {
                        b.lines[i].text = crate::indent(&b.lines[i].text, n as i64 * tab);
                    }
                    self.select = None;
                }
            }
            (_, Key::Char(ch)) => insert_text(e, &ch.to_string()),
            _ => {}
        }
        After::Stay
    }

    /// A key at a prompt: the text typed, then ENTER (or Return); for a
    /// search, ADVANCE and BACKUP end it too and set the direction.
    fn prompted(&mut self, e: &mut Edt, p: Prompt, k: Key, files: &mut dyn Files) -> After {
        let (mut text, cmd) = match &p {
            Prompt::Command(t) => (t.clone(), true),
            Prompt::Search(t) => (t.clone(), false),
        };
        let done = match k {
            Key::Char(c) => {
                text.push(c);
                false
            }
            Key::Delete => {
                text.pop();
                false
            }
            Key::Ctrl('U') => {
                text.clear();
                false
            }
            Key::Ctrl('Z') | Key::Ctrl('C') => return After::Stay,
            Key::Kp('4') if !cmd => {
                self.backup = false;
                true
            }
            Key::Kp('5') if !cmd => {
                self.backup = true;
                true
            }
            Key::KpEnter | Key::Return => true,
            _ => false,
        };
        if !done {
            self.prompt = Some(if cmd {
                Prompt::Command(text)
            } else {
                Prompt::Search(text)
            });
            return After::Stay;
        }
        if !cmd {
            if !text.is_empty() {
                e.search = text;
            }
            self.find_next(e);
            self.keep_visible(e);
            return After::Stay;
        }
        let flow = e.command(&text, files);
        let said = e.take();
        self.message = said.last().cloned().unwrap_or_default();
        self.keep_visible(e);
        match flow {
            Flow::Go | Flow::Change => After::Stay,
            f => After::Done(f),
        }
    }

    /// The select range (start, end) in order, or None (and a message).
    fn range(&mut self, e: &Edt) -> Option<((usize, usize), (usize, usize))> {
        let s = self.select?;
        let p = pos(e);
        Some(if s <= p { (s, p) } else { (p, s) })
    }

    fn find_next(&mut self, e: &mut Edt) {
        if e.search.is_empty() {
            self.message = "No search string".into();
            return;
        }
        let (l, c) = pos(e);
        let b = e.buf();
        let exact = e.set.exact;
        let pat = e.search.clone();
        let hit = if self.backup {
            (0..=l.min(b.lines.len().saturating_sub(1)))
                .rev()
                .find_map(|i| {
                    let t = &b.lines.get(i)?.text;
                    let lim = if i == l { c } else { t.len() };
                    let mut last = None;
                    let mut from = 0;
                    while let Some(p) = crate::find_from(t, &pat, from, exact) {
                        if p >= lim {
                            break;
                        }
                        last = Some(p);
                        from = p + 1;
                    }
                    last.map(|p| (i, p))
                })
        } else {
            (l..b.lines.len()).find_map(|i| {
                let from = if i == l { c + 1 } else { 0 };
                crate::find_from(&b.lines[i].text, &pat, from, exact).map(|p| (i, p))
            })
        };
        match hit {
            Some((i, p)) => {
                let p = if e.set.search_end { p + pat.len() } else { p };
                set_pos(e, (i, p));
            }
            None => self.message = crate::NOT_FOUND.into(),
        }
    }

    /// SUBS: the search string at the cursor becomes the paste buffer's
    /// text, and the search goes on.
    fn subs(&mut self, e: &mut Edt) {
        let (l, c) = pos(e);
        let pat = e.search.clone();
        let here = e
            .buf()
            .lines
            .get(l)
            .is_some_and(|x| crate::find_from(&x.text, &pat, c, e.set.exact) == Some(c));
        if pat.is_empty() || !here {
            self.message = "No select range active".into();
            return;
        }
        cut(e, (l, c), (l, c + pat.len()));
        let p = paste_text(e);
        insert_text(e, &p);
        self.find_next(e);
    }

    fn change_case(&mut self, e: &mut Edt, fwd: bool) {
        let swap = |s: &str| -> String {
            s.chars()
                .map(|c| match c.is_uppercase() {
                    true => c.to_lowercase().next().unwrap_or(c),
                    false => c.to_uppercase().next().unwrap_or(c),
                })
                .collect()
        };
        match self.range(e) {
            Some((s, t)) => {
                let text = cut(e, s, t);
                insert_text(e, &swap(&text));
                set_pos(e, s);
                self.select = None;
            }
            None => {
                let p = pos(e);
                let q = step_char(e, p, true);
                if q != p && q.0 == p.0 {
                    let text = cut(e, p, q);
                    insert_text(e, &swap(&text));
                    if !fwd {
                        let back = step_char(e, p, false);
                        set_pos(e, back);
                    }
                }
            }
        }
    }

    /// A DEFINE KEY definition: EDT's nokeypad commands, each ended by a
    /// period. ponytail: the entities and commands keypad mode uses; not
    /// the whole nokeypad language (SN, macros in definitions ...).
    fn nokeypad(&mut self, e: &mut Edt, def: &str, files: &mut dyn Files) -> After {
        let up = def.to_ascii_uppercase();
        let mut i = 0;
        let s: Vec<char> = def.chars().collect();
        let u: Vec<char> = up.chars().collect();
        while i < s.len() {
            // I text ^Z: insert.
            if u[i] == 'I' {
                let end = (i + 1..s.len())
                    .find(|j| s[*j] == '^' && u.get(j + 1) == Some(&'Z'))
                    .unwrap_or(s.len());
                let text: String = s[i + 1..end].iter().collect();
                insert_text(e, &text);
                i = (end + 2).min(s.len());
                if s.get(i) == Some(&'.') {
                    i += 1;
                }
                continue;
            }
            let end = (i..s.len()).find(|j| s[*j] == '.').unwrap_or(s.len());
            let cmd: String = u[i..end].iter().collect();
            i = end + 1;
            let cmd = cmd.trim();
            if let Some(rest) = cmd.strip_prefix("EXT ") {
                let f = e.command(rest, files);
                let said = e.take();
                self.message = said.last().cloned().unwrap_or_default();
                if matches!(f, Flow::Exit { .. } | Flow::Quit { .. }) {
                    return After::Done(f);
                }
                continue;
            }
            let k = match cmd {
                "ADV" => Some((false, Key::Kp('4'))),
                "BACK" => Some((false, Key::Kp('5'))),
                "SEL" => Some((false, Key::Kp('.'))),
                "RESET" => Some((true, Key::Kp('.'))),
                "CUTSR" => Some((false, Key::Kp('6'))),
                "APPENDSR" => Some((false, Key::Kp('9'))),
                "PASTE" => Some((true, Key::Kp('6'))),
                "UNDC" => Some((true, Key::Kp(','))),
                "UNDW" => Some((true, Key::Kp('-'))),
                "UNDL" => Some((true, Key::Pf(4))),
                "REF" => Some((false, Key::Ctrl('W'))),
                "^Z" | "EXIT" => return After::LineMode,
                _ => None,
            };
            if let Some((g, k)) = k {
                self.act(e, g, &k, 1);
                continue;
            }
            // [D][+|-][n]entity.
            let (del, rest) = match cmd.strip_prefix('D') {
                Some(r) if !r.starts_with("EL") || r.starts_with('+') || r.starts_with('-') => {
                    (true, r)
                }
                _ => (false, cmd),
            };
            let (fwd, rest) = match rest.chars().next() {
                Some('+') => (true, &rest[1..]),
                Some('-') => (false, &rest[1..]),
                _ => (!self.backup, rest),
            };
            let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            let times: usize = digits.parse().unwrap_or(1);
            let ent = &rest[digits.len()..];
            for _ in 0..times {
                let from = pos(e);
                match ent {
                    "C" => {
                        let t = step_char(e, from, fwd);
                        set_pos(e, t);
                    }
                    "W" => move_word(e, fwd),
                    "L" => move_line(e, fwd),
                    "EL" => move_eol(e, fwd),
                    "BL" => set_pos(e, (from.0, 0)),
                    "PAGE" => move_page(e, fwd),
                    "SR" => {
                        if let Some((s, t)) = self.range(e) {
                            set_pos(e, if fwd { t } else { s });
                        }
                    }
                    _ => {
                        self.message = "Unrecognized command".into();
                        return After::Stay;
                    }
                }
                if del {
                    let to = pos(e);
                    let (a, b) = if from <= to { (from, to) } else { (to, from) };
                    cut(e, a, b);
                    set_pos(e, a);
                }
            }
        }
        self.keep_visible(e);
        After::Stay
    }

    /// Scrolls so the cursor line stays between SET CURSOR's rows.
    fn keep_visible(&mut self, e: &Edt) {
        let l = e.buf().cur;
        let (ct, cb) = (e.set.cursor.0 as usize, e.set.cursor.1 as usize);
        let rows = e.set.lines as usize;
        if l < self.top + ct.min(l) {
            self.top = l.saturating_sub(ct);
        }
        if l >= self.top + cb.max(1) && l + 1 > self.top + rows.min(cb + 1) {
            self.top = l + 1 - cb.min(rows).max(1);
        }
        if l < self.top {
            self.top = l;
        }
        if l >= self.top + rows {
            self.top = l + 1 - rows;
        }
    }

    /// The screen, `rows` by `cols`: SET LINES of text, then the message
    /// or prompt on the last row.
    pub fn screen(&mut self, e: &Edt, rows: usize, cols: usize) -> Screen {
        if self.help {
            let mut s = Screen::default();
            for l in HELP.lines().take(rows) {
                s.rows.push(l.chars().take(cols).collect());
            }
            s.rows.resize(rows, String::new());
            s.reverse = vec![None; rows];
            s.cursor = (rows - 1, 0);
            return s;
        }
        self.keep_visible(e);
        let text_rows = (e.set.lines as usize).min(rows.saturating_sub(1)).max(1);
        let b = e.buf();
        let sel = self.select.map(|s| {
            let p = (b.cur, b.col);
            if s <= p { (s, p) } else { (p, s) }
        });
        let mut s = Screen::default();
        let mut cursor = (0, 0);
        for r in 0..text_rows {
            let i = self.top + r;
            let (row, rev) = match b.lines.get(i) {
                Some(l) => {
                    let shown = expand(&l.text);
                    let rev = sel.and_then(|((sl, sc), (el, ec))| {
                        if i < sl || i > el {
                            return None;
                        }
                        let a = if i == sl { col_of(&l.text, sc) } else { 0 };
                        let z = if i == el {
                            col_of(&l.text, ec)
                        } else {
                            shown.chars().count() + 1
                        };
                        Some((a, z))
                    });
                    (shown, rev)
                }
                None if i == b.lines.len() => (e.set.eob.clone(), None),
                None => (String::new(), None),
            };
            let row: String = row.chars().take(cols).collect();
            if i == b.cur {
                let text = b.lines.get(i).map_or("", |l| l.text.as_str());
                cursor = (
                    r,
                    col_of(text, b.col.min(text.len())).min(cols.saturating_sub(1)),
                );
            }
            s.rows.push(row);
            s.reverse.push(rev);
        }
        while s.rows.len() < rows - 1 {
            s.rows.push(String::new());
            s.reverse.push(None);
        }
        let bottom = match &self.prompt {
            Some(Prompt::Command(t)) => format!("Command: {t}"),
            Some(Prompt::Search(t)) => format!("Search for: {t}"),
            None => self.message.clone(),
        };
        if self.prompt.is_some() {
            cursor = (rows - 1, bottom.chars().count());
        }
        s.rows.push(bottom.chars().take(cols).collect());
        s.reverse.push(None);
        s.cursor = cursor;
        s
    }
}

const HELP: &str = "\
                          EDT keypad (vmsport)

  PF1 GOLD        PF2 HELP        PF3 FNDNXT      PF4 DEL L
                                  gold: FIND      gold: UND L
  7 PAGE          8 SECT          9 APPEND        - DEL W
  gold: COMMAND   gold: FILL      gold: REPLACE   gold: UND W
  4 ADVANCE       5 BACKUP        6 CUT           , DEL C
  gold: BOTTOM    gold: TOP       gold: PASTE     gold: UND C
  1 WORD          2 EOL           3 CHAR          ENTER
  gold: CHNGCASE  gold: DEL EOL   gold: SPECINS   gold: SUBS
  0 LINE                          . SELECT
  gold: OPEN LINE                 gold: RESET

  Arrows move; DELETE deletes the character before the cursor;
  Ctrl/H to the start of the line; Ctrl/J deletes a word back;
  Ctrl/U to the start of the line; Ctrl/W redraws; Ctrl/Z: line mode.
  GOLD and digits repeat the next key.

  Press any key to go on.";

fn pos(e: &Edt) -> (usize, usize) {
    let b = e.buf();
    let len = b.lines.get(b.cur).map_or(0, |l| l.text.len());
    (b.cur, b.col.min(len))
}

fn set_pos(e: &mut Edt, p: (usize, usize)) {
    let b = e.buf_mut();
    b.cur = p.0.min(b.lines.len());
    b.col = p.1;
}

/// The display column of byte `c` in `s` (tabs to every 8).
fn col_of(s: &str, c: usize) -> usize {
    let mut col = 0;
    for ch in s[..c.min(s.len())].chars() {
        col = if ch == '\t' {
            (col / 8 + 1) * 8
        } else {
            col + 1
        };
    }
    col
}

/// The byte in `s` at display column `col` (or its end).
fn byte_at(s: &str, col: usize) -> usize {
    let mut c = 0;
    for (i, ch) in s.char_indices() {
        let next = if ch == '\t' { (c / 8 + 1) * 8 } else { c + 1 };
        if next > col {
            return i;
        }
        c = next;
    }
    s.len()
}

fn expand(s: &str) -> String {
    let mut out = String::new();
    for ch in s.chars() {
        match ch {
            '\t' => {
                let n = 8 - out.chars().count() % 8;
                out.push_str(&" ".repeat(n));
            }
            '\x0c' => out.push_str("<FF>"),
            c if (c as u32) < 32 => out.push_str(&format!("<{}>", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// One character on, or back; across line ends.
fn step_char(e: &Edt, (l, c): (usize, usize), fwd: bool) -> (usize, usize) {
    let b = e.buf();
    let text = b.lines.get(l).map_or("", |x| x.text.as_str());
    if fwd {
        match text[c..].chars().next() {
            Some(ch) => (l, c + ch.len_utf8()),
            None if l < b.lines.len() => (l + 1, 0),
            None => (l, c),
        }
    } else if c > 0 {
        let ch = text[..c].chars().last().unwrap();
        (l, c - ch.len_utf8())
    } else if l > 0 {
        (l - 1, b.lines[l - 1].text.len())
    } else {
        (l, c)
    }
}

fn is_word(c: char) -> bool {
    !c.is_whitespace()
}

/// Where the word after the cursor starts (WORD advancing, DEL W).
fn word_end(e: &Edt) -> (usize, usize) {
    let (l, c) = pos(e);
    let b = e.buf();
    let Some(line) = b.lines.get(l) else {
        return (l, c);
    };
    let t = &line.text;
    let rest: Vec<(usize, char)> = t[c..].char_indices().map(|(k, ch)| (k + c, ch)).collect();
    if rest.is_empty() {
        return (l + 1, 0);
    }
    let mut it = rest.iter().peekable();
    if it.peek().is_some_and(|(_, ch)| is_word(*ch)) {
        while it.peek().is_some_and(|(_, ch)| is_word(*ch)) {
            it.next();
        }
    }
    while it.peek().is_some_and(|(_, ch)| !is_word(*ch)) {
        it.next();
    }
    if let Some((k, _)) = it.peek() {
        return (l, *k);
    }
    (l, t.len())
}

/// Where the word before the cursor starts (WORD backing up, Ctrl/J).
fn word_start(e: &Edt) -> (usize, usize) {
    let (l, c) = pos(e);
    if c == 0 {
        return match l {
            0 => (0, 0),
            _ => (l - 1, e.buf().lines[l - 1].text.len()),
        };
    }
    let t = &e.buf().lines[l].text;
    let before: Vec<(usize, char)> = t[..c].char_indices().collect();
    let mut k = before.len();
    while k > 0 && !is_word(before[k - 1].1) {
        k -= 1;
    }
    while k > 0 && is_word(before[k - 1].1) {
        k -= 1;
    }
    (l, before.get(k).map_or(0, |x| x.0))
}

fn move_word(e: &mut Edt, fwd: bool) {
    let p = if fwd { word_end(e) } else { word_start(e) };
    set_pos(e, p);
}

/// LINE: the start of the next line; backing up, of this one (or the
/// one before, at a start).
fn move_line(e: &mut Edt, fwd: bool) {
    let (l, c) = pos(e);
    let n = e.buf().lines.len();
    set_pos(
        e,
        match fwd {
            true => ((l + 1).min(n), 0),
            false if c > 0 => (l, 0),
            false => (l.saturating_sub(1), 0),
        },
    );
}

/// EOL: the end of this line (or the next, at an end); backing up, of the
/// line before.
fn move_eol(e: &mut Edt, fwd: bool) {
    let (l, c) = pos(e);
    let b = e.buf();
    let len = |i: usize| b.lines.get(i).map_or(0, |x| x.text.len());
    let to = match fwd {
        true if c < len(l) => (l, len(l)),
        true if l < b.lines.len() => ((l + 1).min(b.lines.len()), len(l + 1)),
        true => (l, c),
        false if l > 0 => (l - 1, len(l - 1)),
        false => (0, 0),
    };
    set_pos(e, to);
}

fn move_lines(e: &mut Edt, fwd: bool, n: usize) {
    let (l, _) = pos(e);
    let last = e.buf().lines.len();
    set_pos(
        e,
        (
            if fwd {
                (l + n).min(last)
            } else {
                l.saturating_sub(n)
            },
            0,
        ),
    );
}

/// PAGE: the next line starting with a form feed (or the buffer's end).
fn move_page(e: &mut Edt, fwd: bool) {
    let (l, _) = pos(e);
    let b = e.buf();
    let ff = |i: &usize| b.lines[*i].text.starts_with('\x0c');
    let to = match fwd {
        true => (l + 1..b.lines.len()).find(ff).unwrap_or(b.lines.len()),
        false => (0..l).rev().find(ff).unwrap_or(0),
    };
    set_pos(e, (to, 0));
}

/// Removes the text from `a` to `b` (positions, `a` first) and returns it
/// ("\n" for each line end); the cursor goes to `a`.
fn cut(e: &mut Edt, a: (usize, usize), b: (usize, usize)) -> String {
    let buf = e.buf_mut();
    let n = buf.lines.len();
    let (a, b) = ((a.0.min(n), a.1), (b.0.min(n), b.1));
    if a >= b {
        return String::new();
    }
    let text_of =
        |buf: &crate::Buffer, i: usize| buf.lines.get(i).map_or(String::new(), |l| l.text.clone());
    let first = text_of(buf, a.0);
    let (ac, bc) = (a.1.min(first.len()), b.1);
    let mut out = String::new();
    if a.0 == b.0 {
        out.push_str(&first[ac..bc.min(first.len())]);
        if a.0 < n {
            buf.lines[a.0]
                .text
                .replace_range(ac..bc.min(first.len()), "");
        }
    } else {
        out.push_str(&first[ac..]);
        out.push('\n');
        for i in a.0 + 1..b.0 {
            out.push_str(&text_of(buf, i));
            out.push('\n');
        }
        let last = text_of(buf, b.0);
        let bc = bc.min(last.len());
        out.push_str(&last[..bc]);
        let joined = format!("{}{}", &first[..ac], &last[bc..]);
        let end = if b.0 < n { b.0 } else { n - 1 };
        buf.lines.drain(a.0 + 1..=end);
        if a.0 < buf.lines.len() {
            buf.lines[a.0].text = joined;
        }
    }
    buf.cur = a.0.min(buf.lines.len());
    buf.col = ac;
    out
}

/// Puts `text` in at the cursor ("\n" breaks lines); the cursor ends
/// after it.
fn insert_text(e: &mut Edt, text: &str) {
    let (l, c) = pos(e);
    let buf = e.buf_mut();
    let parts: Vec<&str> = text.split('\n').collect();
    if l == buf.lines.len() {
        // At [EOB]: whole new lines.
        let mut lines: Vec<String> = parts.iter().map(|s| s.to_string()).collect();
        let open = lines.last().is_some_and(|s| !s.is_empty());
        if !open {
            lines.pop();
        }
        let k = lines.len();
        buf.insert(l, lines);
        if open {
            (buf.cur, buf.col) = (l + k - 1, parts.last().unwrap().len());
        } else {
            (buf.cur, buf.col) = (l + k, 0);
        }
        return;
    }
    let line = buf.lines[l].text.clone();
    let (head, tail) = line.split_at(c.min(line.len()));
    if parts.len() == 1 {
        buf.lines[l].text = format!("{head}{text}{tail}");
        buf.col = c + text.len();
        return;
    }
    buf.lines[l].text = format!("{head}{}", parts[0]);
    let mut news: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();
    let last_len = news.last().unwrap().len();
    news.last_mut().unwrap().push_str(tail);
    let k = news.len();
    buf.cur = l + 1;
    buf.insert(l + 1, news);
    (buf.cur, buf.col) = (l + k, last_len);
}

fn paste_text(e: &mut Edt) -> String {
    let p = e.buffer("PASTE");
    let lines = &e.buffers[p].lines;
    let mut s = lines
        .iter()
        .map(|l| l.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    if e.paste_open != Some(s.clone()) && !lines.is_empty() {
        s.push('\n');
    }
    s
}

/// The paste buffer holds `text` (its lines; a last line without its end
/// is remembered as open).
fn set_paste(e: &mut Edt, text: &str) {
    let p = e.buffer("PASTE");
    let mut parts: Vec<String> = text.split('\n').map(String::from).collect();
    let open = !text.ends_with('\n');
    if !open {
        parts.pop();
    }
    e.buffers[p].lines = parts
        .into_iter()
        .zip(1..)
        .map(|(text, n)| crate::Line { num: n * ONE, text })
        .collect();
    e.buffers[p].cur = 0;
    e.paste_open = open.then(|| text.to_string());
}
