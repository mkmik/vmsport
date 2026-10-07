//! DCL, the command language: symbols, expressions, procedures, control
//! flow, and verbs dispatched through the command tables, either to DCL
//! itself (ROUTINE) or to images (IMAGE).

mod builtins;
pub mod expr;
pub mod host;
mod lexicals;
pub mod lineedit;
pub mod real;
mod spawn;

use expr::Value;
pub use host::{Change, Child, Host, Launch, Mode, RecordFile, Table};
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;
use vms_cld::Tables;
use vms_cond::Cond;
use vms_msg::{Catalog, Flags};

/// Procedure levels (@ and CALL) DCL allows.
const MAX_DEPTH: usize = 32;

/// `CLI$_NORMAL`: what a DCL command that worked leaves in `$STATUS`.
pub const NORMAL: Cond = Cond(0x0003_0001);

/// The CLI codes DCL's own errors use (fixtures/msg/recorded/sysmsg.log).
const CLI: &[(&str, u32)] = &[
    ("ABVERB", 0x38008),
    ("EXPSYN", 0x38038),
    ("INSFPRM", 0x38048),
    ("IVCHAR", 0x38050),
    ("IVKEYW", 0x38060),
    ("IVVERB", 0x38090),
    ("MAXPARM", 0x38098),
    ("NOCOMD", 0x380B0),
    ("ONERR", 0x380F8),
    ("SYMDEL", 0x38130),
    ("UNDSYM", 0x38140),
    ("USGOTO", 0x38148),
    ("BADRET", 0x38198),
    ("UNDFIL", 0x38188),
    ("FILOPN", 0x38190),
    ("IVFNAM", 0x381C0),
    ("BADBLK", 0x381D8),
    ("NOTHEN", 0x38210),
    ("INVRANGE", 0x38228),
    ("IVQUAL", 0x38240),
    ("IVATIME", 0x38290),
    ("IVDTIME", 0x38298),
    ("USCALL", 0x382D8),
    ("USGOSUB", 0x382E0),
    ("IVVALU", 0x38088),
    ("STKOVF", 0x38128),
    ("SKPDAT", 0x38120),
];

/// A failed command: a status, and the element DCL shows as ` \TOKEN\`.
#[derive(Debug, Clone, PartialEq)]
pub struct DclError {
    pub code: Cond,
    pub ident: &'static str,
    pub token: Option<String>,
}

impl DclError {
    pub fn new(ident: &'static str) -> Self {
        let code = CLI.iter().find(|c| c.0 == ident).map_or(0x38038, |c| c.1);
        DclError {
            code: Cond(code),
            ident,
            token: None,
        }
    }

    pub fn with(ident: &'static str, token: &str) -> Self {
        DclError {
            token: Some(token.to_string()),
            ..Self::new(ident)
        }
    }

    pub fn status(code: Cond) -> Self {
        DclError {
            code,
            ident: "",
            token: None,
        }
    }
}

impl From<vms_cld::Error> for DclError {
    fn from(e: vms_cld::Error) -> Self {
        DclError {
            code: e.code,
            ident: e.ident,
            token: e.token,
        }
    }
}

/// Symbols of one level: exact names, plus abbreviations (`ab*c`).
#[derive(Debug, Default, Clone)]
pub struct Symbols {
    map: BTreeMap<String, Value>,
    /// Full name and the shortest abbreviation's length.
    abbrev: Vec<(String, usize)>,
}

impl Symbols {
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.map.get(name).or_else(|| {
            let (full, _) = self
                .abbrev
                .iter()
                .find(|(f, min)| name.len() >= *min && f.starts_with(name))?;
            self.map.get(full)
        })
    }

    /// `name` may hold a `*` marking the shortest abbreviation.
    pub fn set(&mut self, name: &str, v: Value) {
        let full = name.replace('*', "");
        if let Some(min) = name.find('*') {
            self.abbrev.retain(|(f, _)| *f != full);
            self.abbrev.push((full.clone(), min));
        }
        self.map.insert(full, v);
    }

    pub fn delete(&mut self, name: &str) -> bool {
        let full = self
            .abbrev
            .iter()
            .find(|(f, min)| name.len() >= *min && f.starts_with(name))
            .map(|(f, _)| f.clone());
        let key = full.unwrap_or_else(|| name.to_string());
        self.abbrev.retain(|(f, _)| *f != key);
        self.map.remove(&key).is_some()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &Value)> {
        self.map.iter()
    }
}

/// A command procedure's text.
struct Proc {
    spec: String,
    lines: Vec<String>,
    /// Each command's lines as they are in the file (for SET VERIFY).
    records: Vec<Vec<String>>,
    /// The data lines (not starting with `$`) after each command: its
    /// image's SYS$INPUT.
    data: Vec<Vec<String>>,
    labels: HashMap<String, usize>,
}

impl Proc {
    fn new(spec: String, records: Vec<String>) -> Proc {
        // Join continuations (a trailing `-`), keep lines that start a command.
        let mut lines: Vec<String> = Vec::new();
        let mut raw: Vec<Vec<String>> = Vec::new();
        let mut data: Vec<Vec<String>> = Vec::new();
        let mut cont = false;
        for r in records {
            let text = if cont {
                r.clone()
            } else {
                match r.trim_start().strip_prefix('$') {
                    Some(t) => t.to_string(),
                    None => {
                        if let Some(d) = data.last_mut() {
                            d.push(r);
                        }
                        continue;
                    }
                }
            };
            let t = text.trim_end();
            let (body, more) = match t.strip_suffix('-') {
                Some(b) if !in_quotes(b) => (b.to_string(), true),
                _ => (t.to_string(), false),
            };
            if cont {
                lines.last_mut().unwrap().push_str(&body);
                raw.last_mut().unwrap().push(r.clone());
            } else {
                lines.push(body);
                raw.push(vec![r.trim_start().to_string()]);
                data.push(Vec::new());
            }
            cont = more;
        }
        let mut labels = HashMap::new();
        for (i, l) in lines.iter().enumerate() {
            if let Some((label, _)) = split_label(l) {
                labels.entry(label.to_ascii_uppercase()).or_insert(i);
            }
        }
        Proc {
            spec,
            lines,
            records: raw,
            data,
            labels,
        }
    }
}

fn in_quotes(s: &str) -> bool {
    s.chars().filter(|&c| c == '"').count() % 2 == 1
}

/// `label: rest` at the start of a command line.
fn split_label(line: &str) -> Option<(&str, &str)> {
    let t = line.trim_start();
    let end = t.find(|c: char| !(c.is_alphanumeric() || c == '$' || c == '_'))?;
    if end == 0 || !t[end..].starts_with(':') || t[end + 1..].starts_with(['=', ':']) {
        return None;
    }
    Some((&t[..end], &t[end + 1..]))
}

#[derive(Debug, Clone)]
struct If {
    cond: bool,
    in_else: bool,
    /// The IF itself is in an executed part.
    live: bool,
    /// `IF expr` without THEN on its line: the next command must be THEN.
    awaiting_then: bool,
}

struct Frame {
    proc_: Option<Rc<Proc>>,
    pc: usize,
    locals: Symbols,
    /// SET ON / SET NOON.
    on: bool,
    /// ON threshold (1 warning, 2 error, 3 severe) and command; `None` is EXIT.
    on_level: u8,
    on_action: Option<String>,
    gosubs: Vec<usize>,
    ifs: Vec<If>,
    /// A CALL frame: the line of its ENDSUBROUTINE.
    sub_end: Option<usize>,
    /// Index into `Dcl::outputs`.
    output: usize,
    /// The data lines after the command running now, not yet read.
    pending: std::collections::VecDeque<String>,
}

impl Frame {
    fn new(proc_: Option<Rc<Proc>>, output: usize) -> Frame {
        Frame {
            proc_,
            pc: 0,
            locals: Symbols::default(),
            on: true,
            on_level: 2,
            on_action: None,
            gosubs: Vec::new(),
            ifs: Vec::new(),
            sub_end: None,
            output,
            pending: Default::default(),
        }
    }

    fn active(&self) -> bool {
        self.ifs
            .last()
            .is_none_or(|i| i.live && i.cond != i.in_else)
    }
}

pub struct Dcl {
    pub host: Box<dyn Host>,
    pub globals: Symbols,
    pub tables: Tables,
    pub catalog: Catalog,
    pub msg_flags: Flags,
    pub status: Cond,
    pub verify: bool,
    /// LOGOUT: the session should end.
    pub logged_out: bool,
    frames: Vec<Frame>,
    /// SYS$OUTPUT of each level that redirected it; 0 is the terminal.
    outputs: Vec<Box<dyn RecordFile>>,
    /// Files OPENed by logical name.
    files: HashMap<String, Box<dyn RecordFile>>,
    /// EXIT or STOP in progress: the status, and how many frames to pop.
    exiting: Option<(Cond, usize)>,
    /// $STATUS's message was shown already (by DCL or an image).
    shown: bool,
    /// The command just run showed its status itself.
    pub(crate) just_shown: bool,
    /// SYS$ERROR, and whether it is the same file as SYS$OUTPUT started as.
    errors: Box<dyn RecordFile>,
    errors_same: bool,
    /// This process's name, and how many subprocesses it has spawned.
    pub process_name: String,
    spawned: u32,
}

impl Dcl {
    pub fn new(mut host: Box<dyn Host>) -> Dcl {
        let mut tables = Tables::default();
        match vms_cld::compile(&host.dcl_tables()) {
            Ok(t) => tables.merge(t),
            Err(e) => eprintln!("%DCL-F-TABLES, bad command tables: {e}"),
        }
        let mut catalog = Catalog::default();
        match vms_msg::compile(&host.system_messages()) {
            Ok(m) => catalog.add_system(m),
            Err(e) => eprintln!("%DCL-F-MESSAGES, bad system messages: {:?}", e.first()),
        }
        let terminal = host.terminal_output();
        let (errors, errors_same) = host.error_output();
        let process_name = std::env::var("VMSPORT_PROCESS")
            .ok()
            .or_else(|| host.info("USERNAME"))
            .unwrap_or_else(|| "DCL".into());
        Dcl {
            host,
            globals: Symbols::default(),
            tables,
            catalog,
            msg_flags: Flags::ALL,
            status: Cond(1),
            verify: false,
            logged_out: false,
            frames: vec![Frame::new(None, 0)],
            outputs: vec![terminal],
            files: HashMap::new(),
            exiting: None,
            shown: false,
            just_shown: false,
            errors,
            errors_same,
            process_name,
            spawned: 0,
        }
    }

    /// Defines a symbol at the current level (local) or globally, as a
    /// subprocess gets them from its parent.
    pub fn set_symbol(&mut self, name: &str, v: Value, global: bool) {
        if global {
            self.globals.set(name, v);
        } else {
            self.top().locals.set(name, v);
        }
    }

    fn top(&mut self) -> &mut Frame {
        self.frames.last_mut().unwrap()
    }

    /// Procedure levels: `@` and CALL.
    pub fn depth(&self) -> usize {
        self.frames.len() - 1
    }

    pub fn symbol(&self, name: &str) -> Option<Value> {
        match name {
            "$STATUS" => return Some(Value::Str(format!("%X{:08X}", self.status.0))),
            "$SEVERITY" => return Some(Value::Str((self.status.0 & 7).to_string())),
            _ => {}
        }
        for f in self.frames.iter().rev() {
            if let Some(v) = f.locals.get(name) {
                return Some(v.clone());
            }
            // Locals of outer procedure levels are visible too.
        }
        self.globals.get(name).cloned()
    }

    /// Writes a line to the current SYS$OUTPUT.
    pub fn print(&mut self, line: &str) {
        let i = self.frames.last().map_or(0, |f| f.output);
        let _ = self.outputs[i].write(line);
    }

    /// Shows a message: on SYS$OUTPUT, and on SYS$ERROR too when that is
    /// somewhere else.
    pub fn show(&mut self, text: &str) {
        for l in text.lines() {
            self.print(l);
        }
        let redirected = self.frames.last().is_some_and(|f| f.output != 0);
        if redirected || !self.errors_same {
            for l in text.lines() {
                let _ = self.errors.write(l);
            }
        }
    }

    /// The message for `code`, as DCL shows it.
    pub fn message(&self, code: Cond) -> String {
        let m = self.catalog.get_msg(code, self.msg_flags);
        // DCL shows the CLI facility's errors as its own.
        match m.strip_prefix("%CLI-") {
            Some(rest) => format!("%DCL-{rest}"),
            None => m,
        }
    }

    pub fn report(&mut self, e: &DclError) {
        let mut m = self.message(e.code);
        if let Some(t) = &e.token {
            m.push_str(&format!("\n \\{t}\\"));
        }
        self.show(&m);
    }

    /// Runs one command at the interactive level (or `dcl -c`).
    pub fn command(&mut self, line: &str) -> Cond {
        let line = line.trim_start().strip_prefix('$').unwrap_or(line);
        self.step(line);
        self.run();
        self.status
    }

    /// Runs `@spec params`, like typing it.
    pub fn execute(&mut self, spec: &str, params: &[String]) -> Cond {
        let mut line = format!("@{spec}");
        for p in params {
            line.push_str(&format!(" \"{}\"", p.replace('"', "\"\"")));
        }
        self.command(&line)
    }

    /// Runs procedure frames until they all return to the level we
    /// started at.
    fn run(&mut self) {
        while self.frames.len() > 1 {
            if let Some((status, levels)) = self.exiting.take() {
                self.unwind(status, levels);
                continue;
            }
            let f = self.top();
            let p = f.proc_.clone().unwrap();
            if f.pc >= p.lines.len() {
                let st = self.status;
                self.unwind(st, 1);
                continue;
            }
            let line = p.lines[f.pc].clone();
            let records = p.records[f.pc].clone();
            f.pending = p.data[f.pc].iter().cloned().collect();
            f.pc += 1;
            let level = self.frames.len();
            // Verify shows the lines as in the file, after apostrophe
            // substitution, if it was on when they were read.
            if self.verify && self.top().active() {
                for r in records {
                    let shown = self.substitute_only(&r);
                    self.print(&shown);
                }
            }
            self.step(&line);
            // Data lines nobody read.
            if let Some(f) = self.frames.get_mut(level - 1)
                && !std::mem::take(&mut f.pending).is_empty()
                && self.exiting.is_none()
            {
                let e = DclError::new("SKPDAT");
                self.report(&e);
                self.shown = true;
                self.after(e.code);
            }
        }
        if let Some((status, _)) = self.exiting.take() {
            self.status = status;
        }
    }

    /// Leaves `levels` procedure levels with `status`. A failure status is
    /// shown here, unless it was shown already.
    fn unwind(&mut self, status: Cond, levels: usize) {
        for _ in 0..levels.min(self.frames.len() - 1) {
            let f = self.frames.pop().unwrap();
            if f.output != 0 && !self.frames.iter().any(|g| g.output == f.output) {
                self.outputs.truncate(f.output);
            }
        }
        self.status = status;
        if !status.is_success() && !status.inhibit_msg() && !self.shown {
            self.shown = true;
            let m = self.message(status);
            self.show(&m);
        }
        self.after(status);
    }

    /// One command line (without its `$`).
    fn step(&mut self, raw: &str) {
        let mut line = raw;
        if let Some((_, rest)) = split_label(line) {
            line = rest;
        }
        let line = line.trim();
        if !self.top().active() || self.top().ifs.last().is_some_and(|i| i.awaiting_then) {
            self.skip(line);
            return;
        }
        if first_word(line) == "SUBROUTINE" && self.top().sub_end.is_none() {
            // Not called: step over the subroutine.
            self.skip_subroutine();
            return;
        }
        let result = self.substitute(line).and_then(|l| self.dispatch(&l));
        match result {
            Ok(Some(st)) => {
                self.shown = st.inhibit_msg() || std::mem::take(&mut self.just_shown);
                self.after(st);
            }
            Ok(None) => {}
            Err(e) => {
                if e.ident.is_empty() {
                    let m = self.message(e.code);
                    self.show(&m);
                } else {
                    self.report(&e);
                }
                self.shown = true;
                self.after(e.code);
            }
        }
    }

    /// Sets $STATUS and takes the ON action if the severity calls for it.
    fn after(&mut self, status: Cond) {
        self.status = status;
        if self.frames.len() < 2 || status.is_success() || self.exiting.is_some() {
            return;
        }
        let level = match status.0 & 7 {
            0 => 1,
            2 => 2,
            _ => 3,
        };
        let f = self.top();
        if !f.on || level < f.on_level {
            return;
        }
        match f.on_action.take() {
            Some(cmd) => {
                f.on_level = 2;
                self.step(&cmd);
            }
            None => self.exiting = Some((status, 1)),
        }
    }

    /// IF/THEN/ELSE/ENDIF bookkeeping in a part that doesn't run.
    fn skip(&mut self, line: &str) {
        let f = self.top();
        match first_word(line).as_str() {
            "IF" => {
                if !has_then_command(line) {
                    let awaiting = !line.to_ascii_uppercase().contains("THEN");
                    f.ifs.push(If {
                        cond: false,
                        in_else: false,
                        live: false,
                        awaiting_then: awaiting,
                    });
                }
            }
            "THEN" => {
                if let Some(i) = f.ifs.last_mut() {
                    i.awaiting_then = false;
                }
            }
            "ELSE" => {
                if let Some(i) = f.ifs.last_mut() {
                    i.in_else = true;
                }
            }
            "ENDIF" => {
                f.ifs.pop();
            }
            _ => {}
        }
    }

    fn skip_subroutine(&mut self) {
        let p = self.top().proc_.clone().unwrap();
        let mut depth = 0;
        let mut pc = self.top().pc;
        while pc < p.lines.len() {
            let l = split_label(&p.lines[pc]).map_or(p.lines[pc].as_str(), |(_, r)| r);
            match first_word(l).as_str() {
                "SUBROUTINE" => depth += 1,
                "ENDSUBROUTINE" if depth == 0 => break,
                "ENDSUBROUTINE" => depth -= 1,
                _ => {}
            }
            pc += 1;
        }
        self.top().pc = pc + 1;
    }

    /// Apostrophe substitution: `'expr'` outside quotes, `''expr'` inside.
    fn substitute(&mut self, line: &str) -> Result<String, DclError> {
        let out = self.substitute_only(line);
        Ok(strip_comment(&out))
    }

    /// Apostrophe substitution alone, comments kept.
    fn substitute_only(&mut self, line: &str) -> String {
        let cs: Vec<char> = line.chars().collect();
        let mut out = String::new();
        let mut quoted = false;
        let mut i = 0;
        while i < cs.len() {
            let c = cs[i];
            if c == '"' {
                quoted = !quoted;
            }
            let start = match (c, quoted) {
                ('\'', false) => Some(i + 1),
                ('\'', true) if cs.get(i + 1) == Some(&'\'') => Some(i + 2),
                _ => None,
            };
            if let Some(s) = start
                && let Some((text, end)) = self.substitution(&cs, s)
            {
                out.push_str(&text);
                i = if cs.get(end) == Some(&'\'') {
                    end + 1
                } else {
                    end
                };
                continue;
            }
            out.push(c);
            i += 1;
        }
        out
    }

    /// What `'` at `s` substitutes: a symbol's value ("" if undefined), or a
    /// lexical function's result. Returns the text and where it ended.
    fn substitution(&mut self, cs: &[char], s: usize) -> Option<(String, usize)> {
        let mut e = s;
        while e < cs.len() && (cs[e].is_alphanumeric() || cs[e] == '$' || cs[e] == '_') {
            e += 1;
        }
        if e == s {
            return None;
        }
        let name: String = cs[s..e].iter().collect::<String>().to_ascii_uppercase();
        if name.starts_with("F$") && cs.get(e) == Some(&'(') {
            // To the matching parenthesis, strings included.
            let (mut depth, mut q) = (0, false);
            while e < cs.len() {
                match cs[e] {
                    '"' => q = !q,
                    '(' if !q => depth += 1,
                    ')' if !q => {
                        depth -= 1;
                        if depth == 0 {
                            e += 1;
                            break;
                        }
                    }
                    _ => {}
                }
                e += 1;
            }
            let call: String = cs[s..e].iter().collect();
            let v = self.evaluate(&call).map(|v| v.to_str()).unwrap_or_default();
            return Some((v, e));
        }
        Some((
            self.symbol(&name).map(|v| v.to_str()).unwrap_or_default(),
            e,
        ))
    }

    /// Runs a command line after substitution. `Ok(None)`: the command set
    /// $STATUS itself (control flow).
    fn dispatch(&mut self, line: &str) -> Result<Option<Cond>, DclError> {
        if line.is_empty() {
            return Ok(None);
        }
        if let Some(r) = self.assignment(line)? {
            return Ok(Some(r));
        }
        if let Some(rest) = line.strip_prefix('@') {
            return self.at(rest).map(|_| None);
        }
        // A symbol for the verb: its value replaces it (`DIR :== DIRECTORY/SIZE`),
        // and `$path` makes a foreign command.
        let word: String = line
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '$' || *c == '_')
            .collect();
        let up = word.to_ascii_uppercase();
        if !up.is_empty()
            && let Some(Value::Str(v)) = self.symbol(&up)
            && !self.is_dcl_verb(&up)
        {
            let rest = &line[word.len()..];
            if let Some(path) = v.trim_start().strip_prefix('$') {
                let mut args = split_args(path);
                let path = args.remove(0);
                let head = args.join(" ");
                args.extend(split_args(rest));
                return Ok(Some(self.foreign(&path, &format!("{head}{rest}"), &args)));
            }
            let expanded = format!("{v}{rest}");
            return self.command_line(&expanded);
        }
        self.command_line(line)
    }

    /// A foreign command: Unix `args`, and `rest` (DCL-processed) for
    /// LIB$GET_FOREIGN. A failure it didn't show, DCL shows.
    fn foreign(&mut self, path: &str, rest: &str, args: &[String]) -> Cond {
        let line = foreign_line(rest);
        let out = self.outputs[self.frames.last().map_or(0, |f| f.output)].host_file();
        let st = self.child(Child::Foreign(path), "", &line, args, out);
        if !st.is_success() && !st.inhibit_msg() {
            let m = self.message(st);
            self.show(&m);
            self.just_shown = true;
        }
        st
    }

    /// Runs an image or foreign command with DCL's context and applies the
    /// symbols and process logical names it changed.
    fn child(
        &mut self,
        what: Child,
        tables: &str,
        line: &str,
        args: &[String],
        out: Option<std::fs::File>,
    ) -> Cond {
        let symbols = self.visible_symbols();
        // The procedure's data lines after the command are the image's
        // SYS$INPUT, through a pipe we keep one end of to see what is left.
        let data: Vec<String> = self.top().pending.iter().cloned().collect();
        let mut feed = None;
        let mut stdin = None;
        if !data.is_empty()
            && let Ok((r, mut w)) = std::io::pipe()
            && let Ok(check) = r.try_clone()
        {
            stdin = Some(std::process::Stdio::from(r));
            let text: String = data.iter().map(|l| format!("{l}\n")).collect();
            let writer = std::thread::spawn(move || {
                use std::io::Write;
                let _ = w.write_all(text.as_bytes());
            });
            feed = Some((check, writer));
        }
        let launch = Launch {
            symbols: &symbols,
            tables,
            line,
            args,
            stdin,
            stdout: out.map(std::process::Stdio::from),
            ..Default::default()
        };
        let st = match self.host.start(what, launch) {
            Ok(c) => {
                let outcome = c.wait();
                self.apply(&outcome.changes);
                outcome.status
            }
            Err(st) => st,
        };
        if let Some((check, writer)) = feed {
            if unread(&check) == 0 {
                self.top().pending.clear();
            }
            drop(check);
            let _ = writer.join();
        }
        st
    }

    /// What a child changed: symbols here, process logical names by the host.
    fn apply(&mut self, changes: &[Change]) {
        for c in changes {
            match c {
                Change::Set(sym) => {
                    let v = match &sym.value {
                        libvms::image::Value::Int(n) => Value::Int(*n),
                        libvms::image::Value::Str(s) => Value::Str(s.clone()),
                    };
                    if sym.global {
                        self.globals.set(&sym.name, v);
                    } else {
                        self.top().locals.set(&sym.name, v);
                    }
                }
                Change::Delete { name, global } => {
                    if *global {
                        self.globals.delete(name);
                    } else {
                        self.top().locals.delete(name);
                    }
                }
                _ => {}
            }
        }
        self.host.apply(changes);
    }

    /// The symbols a child sees: locals of every level (inner ones win),
    /// and globals.
    pub fn visible_symbols(&self) -> Vec<libvms::image::Symbol> {
        let conv = |v: &Value| match v {
            Value::Int(n) => libvms::image::Value::Int(*n),
            Value::Str(s) => libvms::image::Value::Str(s.clone()),
        };
        let mut locals: BTreeMap<String, Value> = BTreeMap::new();
        for f in &self.frames {
            for (k, v) in f.locals.iter() {
                locals.insert(k.clone(), v.clone());
            }
        }
        let mut out: Vec<libvms::image::Symbol> = locals
            .iter()
            .map(|(k, v)| libvms::image::Symbol {
                name: k.clone(),
                global: false,
                value: conv(v),
            })
            .collect();
        out.extend(self.globals.iter().map(|(k, v)| libvms::image::Symbol {
            name: k.clone(),
            global: true,
            value: conv(v),
        }));
        out
    }

    /// Whether `word` names one of DCL's own verbs exactly (a symbol of the
    /// same name doesn't replace those: `ELSE`, `ENDIF`...).
    fn is_dcl_verb(&self, word: &str) -> bool {
        matches!(word, "IF" | "THEN" | "ELSE" | "ENDIF" | "SET" | "SHOW")
    }

    fn command_line(&mut self, line: &str) -> Result<Option<Cond>, DclError> {
        // Any command with /HELP shows its help instead.
        if let Some(lines) = self.host.command_help(&self.tables, line) {
            for l in lines {
                self.print(&l);
            }
            return Ok(Some(NORMAL));
        }
        let r = match vms_cld::parse(&self.tables, line) {
            Ok(r) => r,
            // Not a verb: DCL$PATH may have a procedure or program for it.
            Err(e) if e.ident == "IVVERB" => {
                let word: String = line
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '$' || *c == '_' || *c == '-')
                    .collect();
                let rest = &line[word.len()..];
                match self.host.dcl_path(&word.to_ascii_lowercase()) {
                    Some((spec, true)) => return self.at(&format!("{spec}{rest}")).map(|_| None),
                    Some((path, false)) => {
                        return Ok(Some(self.foreign(&path, rest, &split_args(rest))));
                    }
                    None => return Err(e.into()),
                }
            }
            Err(e) => return Err(e.into()),
        };
        match (r.routine.clone(), r.image.clone()) {
            (Some(routine), _) => self.routine(&routine, r),
            (None, Some(image)) => {
                let cld = self.verb_tables(&r.verb);
                let out = self.outputs[self.frames.last().map_or(0, |f| f.output)].host_file();
                // The image parses the command itself: the line as typed, since
                // $LINE has lost the quotes around untyped values.
                // A plain Unix program as IMAGE sees the parameters as argv.
                let verb_len = line
                    .find(|c: char| !(c.is_alphanumeric() || c == '$' || c == '_'))
                    .unwrap_or(line.len());
                let args = split_args(&line[verb_len..]);
                let st = self.child(Child::Image(&image), &cld, line, &args, out);
                // A failure the image didn't show itself, DCL shows.
                if !st.is_success() && !st.inhibit_msg() {
                    let m = self.message(st);
                    self.show(&m);
                    self.just_shown = true;
                }
                Ok(Some(st))
            }
            _ => Err(DclError::with("IVVERB", &r.verb)),
        }
    }

    /// The CLD an image needs to parse its command: its verb, the syntaxes
    /// and types.
    fn verb_tables(&self, verb: &str) -> String {
        let t = Tables {
            module: None,
            ident: None,
            verbs: self
                .tables
                .verbs
                .iter()
                .filter(|v| v.name == verb)
                .cloned()
                .collect(),
            syntaxes: self.tables.syntaxes.clone(),
            types: self.tables.types.clone(),
        };
        t.to_cld()
    }

    /// `name = expr`, `name == expr`, `name := text`, `name :== text`,
    /// `name[pos,size] = expr`, `name[pos,size] := text`.
    fn assignment(&mut self, line: &str) -> Result<Option<Cond>, DclError> {
        let cs: Vec<char> = line.chars().collect();
        let mut i = 0;
        while i < cs.len() && (cs[i].is_alphanumeric() || "$_*".contains(cs[i])) {
            i += 1;
        }
        if i == 0 || !(cs[0].is_alphabetic() || cs[0] == '$' || cs[0] == '_') {
            return Ok(None);
        }
        let name: String = cs[..i].iter().collect::<String>().to_ascii_uppercase();
        let mut j = i;
        while j < cs.len() && cs[j].is_whitespace() {
            j += 1;
        }
        let mut field = None;
        if cs.get(j) == Some(&'[') {
            let end = cs[j..]
                .iter()
                .position(|&c| c == ']')
                .ok_or_else(|| DclError::new("SYMDEL"))?
                + j;
            field = Some(cs[j + 1..end].iter().collect::<String>());
            j = end + 1;
            while j < cs.len() && cs[j].is_whitespace() {
                j += 1;
            }
        }
        let rest: String = cs[j..].iter().collect();
        let (string, global, value) = if let Some(v) = rest.strip_prefix(":==") {
            (true, true, v)
        } else if let Some(v) = rest.strip_prefix(":=") {
            (true, false, v)
        } else if let Some(v) = rest.strip_prefix("==") {
            (false, true, v)
        } else if let Some(v) = rest.strip_prefix('=') {
            (false, false, v)
        } else {
            return Ok(None);
        };
        let v = if string {
            Value::Str(string_assignment(value))
        } else {
            self.evaluate(value)?
        };
        let v = match field {
            Some(f) => self.field(&name, &f, v, string)?,
            None => v,
        };
        if global {
            self.globals.set(&name, v);
        } else {
            self.top().locals.set(&name, v);
        }
        Ok(Some(NORMAL))
    }

    /// `name[pos,size]`: bits of an integer, or characters of a string.
    fn field(&mut self, name: &str, f: &str, v: Value, string: bool) -> Result<Value, DclError> {
        let (p, s) = f.split_once(',').ok_or_else(|| DclError::new("SYMDEL"))?;
        let (pos, size) = (self.evaluate(p)?.to_int(), self.evaluate(s)?.to_int());
        if pos < 0 || size < 0 {
            return Err(DclError::new("INVRANGE"));
        }
        let old = self.symbol(&name.replace('*', ""));
        if string {
            let mut chars: Vec<char> = old
                .map(|o| o.to_str())
                .unwrap_or_default()
                .chars()
                .collect();
            let (pos, size) = (pos as usize, size as usize);
            if chars.len() < pos + size {
                chars.resize(pos + size, ' ');
            }
            let new: Vec<char> = format!("{:<size$.size$}", v.to_str()).chars().collect();
            chars.splice(pos..pos + size, new);
            return Ok(Value::Str(chars.into_iter().collect()));
        }
        if pos + size > 32 {
            return Err(DclError::new("INVRANGE"));
        }
        let mask = if size == 32 {
            u32::MAX
        } else {
            ((1u32 << size) - 1) << pos
        };
        let old = old.map_or(0, |o| o.to_int()) as u32;
        Ok(Value::Int(
            ((old & !mask) | (((v.to_int() as u32) << pos) & mask)) as i32,
        ))
    }

    /// Evaluates an expression as DCL reads it: upcased outside quotes,
    /// then `&` substitution.
    pub fn evaluate(&mut self, text: &str) -> Result<Value, DclError> {
        let t = self.ampersand(&upcase_outside_quotes(text));
        let e = expr::parse(&t)?;
        expr::eval(&e, self)
    }

    fn ampersand(&self, s: &str) -> String {
        let mut out = String::new();
        let mut quoted = false;
        let mut cs = s.chars().peekable();
        while let Some(c) = cs.next() {
            if c == '"' {
                quoted = !quoted;
            }
            if c == '&' && !quoted {
                let mut name = String::new();
                while let Some(&n) = cs
                    .peek()
                    .filter(|n| n.is_alphanumeric() || **n == '$' || **n == '_')
                {
                    name.push(n);
                    cs.next();
                }
                out.push_str(&self.symbol(&name).map(|v| v.to_str()).unwrap_or_default());
                continue;
            }
            out.push(c);
        }
        out
    }

    /// `@file[/OUTPUT=spec] params`.
    fn at(&mut self, rest: &str) -> Result<(), DclError> {
        let mut args = split_args(rest);
        if args.is_empty() {
            return Err(DclError::new("INSFPRM"));
        }
        let mut spec = args.remove(0);
        let mut output = None;
        if let Some(i) = spec.find('/') {
            let quals = spec[i..].to_string();
            spec.truncate(i);
            for q in quals.split('/').filter(|q| !q.is_empty()) {
                let (k, v) = q.split_once('=').unwrap_or((q, ""));
                if "OUTPUT".starts_with(&k.to_ascii_uppercase()) && !k.is_empty() {
                    output = Some(v.to_string());
                } else {
                    return Err(DclError::with("IVQUAL", &k.to_ascii_uppercase()));
                }
            }
        }
        if self.depth() >= MAX_DEPTH {
            return Err(DclError::new("STKOVF"));
        }
        let (mut file, full) = self
            .host
            .open(&spec, ".COM", Mode::Read)
            .map_err(DclError::status)?;
        let mut records = Vec::new();
        while let Some(r) = file.read().map_err(DclError::status)? {
            records.push(r);
        }
        let out = match output {
            Some(o) => {
                let (f, _) = self
                    .host
                    .open(&o, ".LIS", Mode::Write)
                    .map_err(DclError::status)?;
                self.outputs.push(f);
                self.outputs.len() - 1
            }
            None => self.top().output,
        };
        let mut frame = Frame::new(Some(Rc::new(Proc::new(full, records))), out);
        for (i, p) in (1..=8).zip(
            args.iter()
                .map(|a| unquote(a))
                .chain(std::iter::repeat(String::new())),
        ) {
            frame.locals.set(&format!("P{i}"), Value::Str(p));
        }
        self.frames.push(frame);
        self.status = NORMAL;
        Ok(())
    }
}

impl expr::Env for Dcl {
    fn symbol(&self, name: &str) -> Option<Value> {
        Dcl::symbol(self, name)
    }

    fn lexical(&mut self, name: &str, args: &[expr::Expr]) -> Result<Value, DclError> {
        lexicals::call(self, name, args)
    }
}

/// The first word of a command, upcased.
fn first_word(line: &str) -> String {
    line.trim_start()
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '$' || *c == '_')
        .collect::<String>()
        .to_ascii_uppercase()
}

/// `IF expr THEN command` (as opposed to a block IF).
fn has_then_command(line: &str) -> bool {
    split_then(line).is_some_and(|(_, cmd)| !cmd.trim().is_empty())
}

/// Splits `expr THEN rest` at the first THEN outside quotes.
fn split_then(s: &str) -> Option<(String, String)> {
    let up = upcase_outside_quotes(s);
    let mut quoted = false;
    let b = up.as_bytes();
    for i in 0..b.len() {
        if b[i] == b'"' {
            quoted = !quoted;
        }
        if !quoted
            && up[i..].starts_with("THEN")
            && (i == 0 || !b[i - 1].is_ascii_alphanumeric())
            && b.get(i + 4)
                .is_none_or(|c| !c.is_ascii_alphanumeric() && *c != b'$' && *c != b'_')
        {
            return Some((s[..i].to_string(), s[i + 4..].to_string()));
        }
    }
    None
}

pub fn upcase_outside_quotes(s: &str) -> String {
    let mut quoted = false;
    s.chars()
        .map(|c| {
            if c == '"' {
                quoted = !quoted;
            }
            if quoted { c } else { c.to_ascii_uppercase() }
        })
        .collect()
}

/// Drops a `!` comment outside quotes.
fn strip_comment(s: &str) -> String {
    let mut quoted = false;
    for (i, c) in s.char_indices() {
        match c {
            '"' => quoted = !quoted,
            '!' if !quoted => return s[..i].trim_end().to_string(),
            _ => {}
        }
    }
    s.trim_end().to_string()
}

/// Bytes waiting in a pipe.
fn unread(r: &std::io::PipeReader) -> usize {
    use std::os::fd::AsRawFd;
    let mut n: libc::c_int = 0;
    // SAFETY: FIONREAD writes an int.
    let rc = unsafe { libc::ioctl(r.as_raw_fd(), libc::FIONREAD, &mut n) };
    if rc < 0 { 0 } else { n as usize }
}

/// A foreign command's parameters as LIB$GET_FOREIGN returns them: upcased
/// and squeezed outside quotes, quotes kept.
fn foreign_line(s: &str) -> String {
    let mut out = String::new();
    let mut quoted = false;
    for c in s.trim().chars() {
        if c == '"' {
            quoted = !quoted;
        }
        if !quoted && c.is_whitespace() {
            if !out.ends_with(' ') {
                out.push(' ');
            }
        } else {
            out.push(if quoted { c } else { c.to_ascii_uppercase() });
        }
    }
    out
}

/// `:=` text: upcased and squeezed outside quotes, quotes removed.
fn string_assignment(s: &str) -> String {
    let mut out = String::new();
    let mut quoted = false;
    let mut cs = s.trim().chars().peekable();
    while let Some(c) = cs.next() {
        if c == '"' {
            if quoted && cs.peek() == Some(&'"') {
                cs.next();
                out.push('"');
                continue;
            }
            quoted = !quoted;
            continue;
        }
        if quoted {
            out.push(c);
        } else if c.is_whitespace() {
            if !out.ends_with(' ') {
                out.push(' ');
            }
        } else {
            out.push(c.to_ascii_uppercase());
        }
    }
    out
}

/// Parameters separated by blanks; quoted ones may hold blanks.
fn split_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    for c in s.chars() {
        if c == '"' {
            quoted = !quoted;
        }
        if c.is_whitespace() && !quoted {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            continue;
        }
        cur.push(c);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// A parameter as a procedure sees it: upcased unless quoted, quotes gone.
fn unquote(s: &str) -> String {
    string_assignment(s)
}
