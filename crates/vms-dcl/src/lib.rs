//! DCL, the command language: symbols, expressions, procedures, control
//! flow, and verbs dispatched through the command tables, either to DCL
//! itself (ROUTINE) or to images (IMAGE).

mod builtins;
pub mod expr;
pub mod host;
mod lexicals;
pub mod real;

use expr::Value;
pub use host::{Host, Mode, RecordFile, Table};
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;
use vms_cld::Tables;
use vms_cond::Cond;
use vms_msg::{Catalog, Flags};

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
    labels: HashMap<String, usize>,
}

impl Proc {
    fn new(spec: String, records: Vec<String>) -> Proc {
        // Join continuations (a trailing `-`), keep lines that start a command.
        let mut lines: Vec<String> = Vec::new();
        let mut cont = false;
        for r in records {
            let text = if cont {
                r.clone()
            } else {
                match r.trim_start().strip_prefix('$') {
                    Some(t) => t.to_string(),
                    None => continue, // data lines: ignored
                }
            };
            let t = text.trim_end();
            let (body, more) = match t.strip_suffix('-') {
                Some(b) if !in_quotes(b) => (b.to_string(), true),
                _ => (t.to_string(), false),
            };
            if cont {
                lines.last_mut().unwrap().push_str(&body);
            } else {
                lines.push(body);
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
        for l in m.lines().map(str::to_string).collect::<Vec<_>>() {
            self.print(&l);
        }
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
            f.pc += 1;
            if self.verify && self.top().active() {
                self.print(&format!("$ {line}"));
            }
            self.step(&line);
        }
        if let Some((status, _)) = self.exiting.take() {
            self.status = status;
        }
    }

    /// Leaves `levels` procedure levels with `status`. A procedure (not a
    /// CALL) that fails shows its status, unless it was shown already.
    fn unwind(&mut self, status: Cond, levels: usize) {
        for _ in 0..levels.min(self.frames.len() - 1) {
            let f = self.frames.pop().unwrap();
            if f.output != 0 && !self.frames.iter().any(|g| g.output == f.output) {
                self.outputs.truncate(f.output);
            }
        }
        self.status = status;
        if !status.is_success() && !status.inhibit_msg() && !self.frames.is_empty() {
            let m = self.message(status);
            for l in m.lines().map(str::to_string).collect::<Vec<_>>() {
                self.print(&l);
            }
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
            Ok(Some(st)) => self.after(st),
            Ok(None) => {}
            Err(e) => {
                if e.ident.is_empty() {
                    let m = self.message(e.code);
                    self.print(&m);
                } else {
                    self.report(&e);
                }
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
            None => self.exiting = Some((Cond(status.0 | 0x1000_0000), 1)),
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
        let cs: Vec<char> = line.chars().collect();
        let mut out = String::new();
        let mut quoted = false;
        let mut i = 0;
        while i < cs.len() {
            let c = cs[i];
            if c == '"' {
                quoted = !quoted;
            }
            let start = if !quoted && c == '\'' {
                Some(i + 1)
            } else if quoted && c == '\'' && cs.get(i + 1) == Some(&'\'') {
                Some(i + 2)
            } else {
                None
            };
            if let Some(s) = start
                && let Some(len) = cs[s..].iter().position(|&c| c == '\'')
            {
                let inner: String = cs[s..s + len].iter().collect();
                let up = upcase_outside_quotes(&inner);
                if let Ok(e) = expr::parse(&up)
                    && let Ok(v) = expr::eval(&e, self)
                {
                    out.push_str(&v.to_str());
                }
                i = s + len + 1;
                continue;
            }
            out.push(c);
            i += 1;
        }
        Ok(strip_comment(&out))
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
                args.extend(split_args(rest));
                return Ok(Some(self.host.run_foreign(&path, &args)));
            }
            let expanded = format!("{v}{rest}");
            return self.command_line(&expanded);
        }
        self.command_line(line)
    }

    /// Whether `word` names one of DCL's own verbs exactly (a symbol of the
    /// same name doesn't replace those: `ELSE`, `ENDIF`...).
    fn is_dcl_verb(&self, word: &str) -> bool {
        matches!(word, "IF" | "THEN" | "ELSE" | "ENDIF" | "SET" | "SHOW")
    }

    fn command_line(&mut self, line: &str) -> Result<Option<Cond>, DclError> {
        let r = vms_cld::parse(&self.tables, line)?;
        match (r.routine.clone(), r.image.clone()) {
            (Some(routine), _) => self.routine(&routine, r),
            (None, Some(image)) => {
                let cld = self.verb_tables(&r.verb);
                let out = self.outputs[self.frames.last().map_or(0, |f| f.output)].host_file();
                // The image parses the command itself: the line as typed, since
                // $LINE has lost the quotes around untyped values.
                let st = self.host.run_image(&image, &cld, line, out);
                // A failure the image didn't show itself, DCL shows.
                if !st.is_success() && !st.inhibit_msg() {
                    let m = self.message(st);
                    for l in m.lines().map(str::to_string).collect::<Vec<_>>() {
                        self.print(&l);
                    }
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
