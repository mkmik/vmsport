//! The verbs DCL does itself (ROUTINE in DCL.CLD).

use crate::expr::{self, Value};
use crate::{Dcl, DclError, Frame, If, Mode, NORMAL, Table, split_args, split_then, unquote};
use vms_cld::{ParseResult, status};
use vms_cond::Cond;

/// RMS$_NORMAL: what WRITE and READ (RMS $PUT, $GET) leave in $STATUS.
const RMS_NORMAL: Cond = Cond(0x0001_0001);

/// RMS$_EOF, which READ returns at end of file.
const RMS_EOF: Cond = Cond(0x0001_827A);

type R = Result<Option<Cond>, DclError>;

fn value(r: &mut ParseResult, name: &str) -> Option<String> {
    r.get_value(name).ok().map(|v| v.0)
}

fn values(r: &mut ParseResult, name: &str) -> Vec<String> {
    std::iter::from_fn(|| r.get_value(name).ok().map(|v| v.0)).collect()
}

fn present(r: &ParseResult, name: &str) -> bool {
    matches!(
        r.present(name),
        status::PRESENT | status::DEFAULTED | status::LOCPRES
    )
}

/// The logical name table qualifiers ask for (process by default).
fn table(r: &mut ParseResult) -> Table {
    if let Some(t) = value(r, "TABLE") {
        return Table::Named(t);
    }
    for (q, t) in [
        ("JOB", Table::Job),
        ("GROUP", Table::Group),
        ("SYSTEM", Table::System),
    ] {
        if present(r, q) {
            return t;
        }
    }
    Table::Process
}

impl Dcl {
    pub(crate) fn routine(&mut self, name: &str, mut r: ParseResult) -> R {
        match name {
            "IF" => self.if_(&value(&mut r, "P1").unwrap_or_default()),
            "THEN" => {
                match self.top().ifs.last_mut() {
                    Some(i) if i.awaiting_then => i.awaiting_then = false,
                    _ => return Err(DclError::new("NOTHEN")),
                }
                Ok(None)
            }
            "ELSE" => {
                let i = self
                    .top()
                    .ifs
                    .last_mut()
                    .ok_or_else(|| DclError::new("NOTHEN"))?;
                i.in_else = true;
                Ok(None)
            }
            "ENDIF" => {
                self.top()
                    .ifs
                    .pop()
                    .ok_or_else(|| DclError::new("NOTHEN"))?;
                Ok(None)
            }
            "GOTO" => {
                let label = value(&mut r, "LABEL").unwrap_or_default();
                let pc = self
                    .label(&label)
                    .ok_or_else(|| DclError::with("USGOTO", &label))?;
                let f = self.top();
                f.pc = pc;
                f.ifs.clear();
                Ok(None)
            }
            "GOSUB" => {
                let label = value(&mut r, "LABEL").unwrap_or_default();
                let pc = self
                    .label(&label)
                    .ok_or_else(|| DclError::with("USGOSUB", &label))?;
                let f = self.top();
                f.gosubs.push(f.pc);
                f.pc = pc;
                Ok(None)
            }
            "RETURN" => {
                // Without a GOSUB, VMS says nothing and marks $STATUS shown.
                let Some(pc) = self.top().gosubs.pop() else {
                    return Ok(Some(Cond(self.status.0 | 0x1000_0000)));
                };
                self.top().pc = pc;
                match value(&mut r, "STATUS").filter(|s| !s.is_empty()) {
                    Some(s) => Ok(Some(Cond(self.evaluate(&s)?.to_int() as u32))),
                    None => Ok(None),
                }
            }
            "EXIT" => {
                let st = match value(&mut r, "STATUS").filter(|s| !s.is_empty()) {
                    Some(s) => {
                        self.shown = false;
                        Cond(self.evaluate(&s)?.to_int() as u32)
                    }
                    None => self.status,
                };
                if self.frames.len() > 1 {
                    self.exiting = Some((st, 1));
                    Ok(None)
                } else {
                    Ok(Some(st))
                }
            }
            "STOP" => {
                self.exiting = Some((Cond(0x2C), usize::MAX)); // SS$_ABORT
                Ok(None)
            }
            "LOGOUT" => {
                self.logged_out = true;
                self.exiting = Some((Cond(1), usize::MAX));
                Ok(None)
            }
            "CALL" => self.call(&value(&mut r, "ARGS").unwrap_or_default()),
            "SUBROUTINE" => Ok(None),
            "ENDSUBROUTINE" => {
                if self.top().sub_end.is_some() {
                    self.exiting = Some((NORMAL, 1));
                }
                Ok(None)
            }
            "ON" => self.on(&value(&mut r, "ACTION").unwrap_or_default()),
            // CONTINUE leaves $STATUS alone (ON WARNING THEN CONTINUE).
            "CONTINUE" => Ok(None),
            "WRITE" => self.write(&mut r),
            "READ" => self.read(&mut r),
            "OPEN" => self.open(&mut r),
            "CLOSE" => {
                let l = value(&mut r, "LOGICAL").unwrap_or_default();
                self.files
                    .remove(&l)
                    .ok_or_else(|| DclError::new("UNDFIL"))?;
                Ok(Some(NORMAL))
            }
            "INQUIRE" => {
                let sym = value(&mut r, "SYMBOL").unwrap_or_default();
                let prompt = value(&mut r, "PROMPT")
                    .map(|p| unquote(&p))
                    .unwrap_or_else(|| sym.clone());
                let punct = if present(&r, "PUNCTUATION") { ": " } else { "" };
                let line = self
                    .host
                    .read_terminal(&format!("{prompt}{punct}"))
                    .unwrap_or_default();
                let v = Value::Str(crate::string_assignment(&line));
                if present(&r, "GLOBAL") {
                    self.globals.set(&sym, v);
                } else {
                    self.top().locals.set(&sym, v);
                }
                Ok(Some(NORMAL))
            }
            "DEFINE" | "ASSIGN" => {
                let name = value(&mut r, "LOGICAL").unwrap_or_default();
                let equivs = values(&mut r, "EQUIVALENCE");
                let attrs = values(&mut r, "TRANSLATION_ATTRIBUTES");
                let t = table(&mut r);
                let st = self
                    .host
                    .define(&name, &equivs, &t, &attrs)
                    .map_err(DclError::status)?;
                if st != Cond(1) && present(&r, "LOG") {
                    let m = self
                        .catalog
                        .put_msg(&[(st, vec![vms_fao::Arg::Str(&name)])], self.msg_flags);
                    for l in m {
                        self.print(&l.replacen("%SYSTEM-", "%DCL-", 1));
                    }
                }
                Ok(Some(NORMAL))
            }
            "DEASSIGN" => {
                let name = value(&mut r, "LOGICAL");
                let t = table(&mut r);
                if name.is_none() && !present(&r, "ALL") {
                    return Err(DclError::new("INSFPRM"));
                }
                self.host
                    .deassign(name.as_deref(), &t)
                    .map_err(DclError::status)?;
                Ok(Some(NORMAL))
            }
            "SET" => {
                let opt = value(&mut r, "OPTION").unwrap_or_default();
                let (neg, kw) = match opt.strip_prefix("NO") {
                    Some(k) if "VERIFY".starts_with(k) || k == "ON" => (true, k.to_string()),
                    _ => (false, opt),
                };
                if kw == "ON" {
                    self.top().on = !neg;
                } else if "VERIFY".starts_with(&kw) {
                    self.verify = !neg;
                }
                Ok(Some(NORMAL))
            }
            "SET_DEFAULT" => {
                let d = value(&mut r, "DIRECTORY").unwrap_or_default();
                self.host.set_default(&d).map_err(DclError::status)?;
                Ok(Some(NORMAL))
            }
            "SET_MESSAGE" => self.set_message(&mut r),
            "SET_COMMAND" => {
                for f in values(&mut r, "FILE") {
                    let text = self.read_file(&f, ".CLD")?;
                    let t = vms_cld::compile(&text).map_err(|e| {
                        self.print(&format!("%CDU-E-SYNTAX, {e}"));
                        DclError::status(Cond(0x0017_8012))
                    })?;
                    self.tables.merge(t);
                }
                Ok(Some(NORMAL))
            }
            "SET_SYMBOL" => Ok(Some(NORMAL)),
            "SHOW" => {
                let opt = value(&mut r, "OPTION").unwrap_or_default();
                if "DEFAULT".starts_with(&opt) {
                    let d = self.host.default_directory();
                    self.print(&format!("  {d}"));
                } else if "TIME".starts_with(&opt) {
                    let t = vms_time::asctim(self.host.now(), false);
                    self.print(&format!("  {}", &t[..20]));
                } else if "STATUS".starts_with(&opt) {
                    let st = format!("  Status on {}", vms_time::asctim(self.host.now(), false));
                    self.print(&st);
                }
                Ok(Some(NORMAL))
            }
            "SHOW_SYMBOL" => self.show_symbol(&mut r),
            "SHOW_LOGICAL" | "SHOW_TRANSLATION" => {
                let names = values(&mut r, "NAME");
                let tables = if present(&r, "ALL")
                    || name == "SHOW_TRANSLATION" && value(&mut r, "TABLE").is_none()
                {
                    Vec::new()
                } else {
                    let mut t = Vec::new();
                    for (q, tb) in [
                        ("PROCESS", Table::Process),
                        ("JOB", Table::Job),
                        ("GROUP", Table::Group),
                        ("SYSTEM", Table::System),
                    ] {
                        if present(&r, q) {
                            t.push(tb);
                        }
                    }
                    t.extend(values(&mut r, "TABLE").into_iter().map(Table::Named));
                    t
                };
                let lines = self.host.show_logical(&names, &tables, present(&r, "FULL"));
                match lines {
                    Ok(lines) => {
                        for l in lines {
                            self.print(&l);
                        }
                        Ok(Some(Cond(1)))
                    }
                    Err(st) => Err(DclError::status(st)),
                }
            }
            "DELETE_SYMBOL" => {
                let name = value(&mut r, "SYMBOL")
                    .or_else(|| value(&mut r, "P1"))
                    .unwrap_or_default();
                let all = present(&r, "ALL");
                let global = present(&r, "GLOBAL");
                if all {
                    if global {
                        self.globals = Default::default();
                    } else {
                        self.top().locals = Default::default();
                    }
                    return Ok(Some(NORMAL));
                }
                let found = if global {
                    self.globals.delete(&name)
                } else {
                    self.top().locals.delete(&name)
                };
                if !found {
                    return Err(DclError::with("UNDSYM", &name));
                }
                Ok(Some(NORMAL))
            }
            _ => Err(DclError::with("IVVERB", name)),
        }
    }

    pub(crate) fn label(&self, label: &str) -> Option<usize> {
        let f = self.frames.last()?;
        f.proc_
            .as_ref()?
            .labels
            .get(&label.to_ascii_uppercase())
            .copied()
    }

    fn if_(&mut self, rest: &str) -> R {
        let live = self.top().active();
        match split_then(rest) {
            Some((e, cmd)) => {
                let cond = self.evaluate(&e)?.is_true();
                if !cmd.trim().is_empty() {
                    return if cond {
                        self.dispatch(cmd.trim())
                    } else {
                        Ok(None)
                    };
                }
                self.top().ifs.push(If {
                    cond,
                    in_else: false,
                    live,
                    awaiting_then: false,
                });
            }
            None => {
                let cond = self.evaluate(rest)?.is_true();
                self.top().ifs.push(If {
                    cond,
                    in_else: false,
                    live,
                    awaiting_then: true,
                });
            }
        }
        Ok(None)
    }

    fn call(&mut self, rest: &str) -> R {
        if self.depth() >= crate::MAX_DEPTH {
            return Err(DclError::new("STKOVF"));
        }
        let mut args = split_args(rest);
        let label = args.remove(0).to_ascii_uppercase();
        let start = self
            .label(&label)
            .ok_or_else(|| DclError::with("USCALL", &label))?;
        let p = self.top().proc_.clone().unwrap();
        // The matching ENDSUBROUTINE.
        let mut depth = 0;
        let mut end = start + 1;
        while end < p.lines.len() {
            let l = crate::split_label(&p.lines[end]).map_or(p.lines[end].as_str(), |(_, r)| r);
            match crate::first_word(l).as_str() {
                "SUBROUTINE" => depth += 1,
                "ENDSUBROUTINE" if depth == 0 => break,
                "ENDSUBROUTINE" => depth -= 1,
                _ => {}
            }
            end += 1;
        }
        let out = self.top().output;
        let mut f = Frame::new(Some(p), out);
        f.pc = start + 1;
        f.sub_end = Some(end);
        for (i, a) in (1..=8).zip(
            args.iter()
                .map(|a| unquote(a))
                .chain(std::iter::repeat(String::new())),
        ) {
            f.locals.set(&format!("P{i}"), Value::Str(a));
        }
        self.frames.push(f);
        self.status = NORMAL;
        Ok(None)
    }

    fn on(&mut self, rest: &str) -> R {
        let (cond, cmd) = split_then(rest).ok_or_else(|| DclError::new("NOTHEN"))?;
        let cond = cond.trim().to_ascii_uppercase();
        let level = [
            ("WARNING", 1),
            ("ERROR", 2),
            ("SEVERE_ERROR", 3),
            ("CONTROL_Y", 0),
        ]
        .iter()
        .find(|(k, _)| !cond.is_empty() && k.starts_with(&cond))
        .ok_or_else(|| DclError::with("ONERR", &cond))?
        .1;
        if level > 0 {
            let f = self.top();
            f.on_level = level;
            f.on_action = Some(cmd.trim().to_string());
        }
        Ok(Some(NORMAL))
    }

    fn write(&mut self, r: &mut ParseResult) -> R {
        let logical = value(r, "LOGICAL").unwrap_or_default();
        let text = value(r, "EXPRESSION").unwrap_or_default();
        let items = expr::parse_list(&self.ampersand(&text))?;
        let mut line = String::new();
        for i in &items {
            line.push_str(&expr::eval(i, self)?.to_str());
        }
        match logical.as_str() {
            "SYS$OUTPUT" | "SYS$ERROR" | "TT" => self.print(&line),
            _ => {
                let f = self
                    .files
                    .get_mut(&logical)
                    .ok_or_else(|| DclError::new("UNDFIL"))?;
                f.write(&line).map_err(DclError::status)?;
            }
        }
        Ok(Some(RMS_NORMAL))
    }

    fn read(&mut self, r: &mut ParseResult) -> R {
        let logical = value(r, "LOGICAL").unwrap_or_default();
        let sym = value(r, "SYMBOL").unwrap_or_default();
        let rec = match logical.as_str() {
            "SYS$INPUT" | "SYS$COMMAND" | "TT" => {
                let prompt = value(r, "PROMPT").unwrap_or_default();
                self.host.read_terminal(&prompt)
            }
            _ => {
                let f = self
                    .files
                    .get_mut(&logical)
                    .ok_or_else(|| DclError::new("UNDFIL"))?;
                f.read().map_err(DclError::status)?
            }
        };
        match rec {
            Some(rec) => {
                self.top().locals.set(&sym, Value::Str(rec));
                Ok(Some(RMS_NORMAL))
            }
            None => match value(r, "END_OF_FILE") {
                Some(label) => {
                    let pc = self
                        .label(&label)
                        .ok_or_else(|| DclError::with("USGOTO", &label))?;
                    self.top().pc = pc;
                    self.status = RMS_EOF;
                    Ok(None)
                }
                None => Err(DclError::status(RMS_EOF)),
            },
        }
    }

    fn open(&mut self, r: &mut ParseResult) -> R {
        let logical = value(r, "LOGICAL").unwrap_or_default();
        let spec = unquote(&value(r, "FILE").unwrap_or_default());
        let (read, write, append) = (
            present(r, "READ"),
            present(r, "WRITE"),
            present(r, "APPEND"),
        );
        let mode = match (read, write, append) {
            (_, _, true) => Mode::Append,
            (true, true, _) => Mode::ReadWrite,
            (false, true, _) => Mode::Write,
            _ => Mode::Read,
        };
        match self.host.open(&spec, "", mode) {
            Ok((f, _)) => {
                self.files.insert(logical, f);
                Ok(Some(NORMAL))
            }
            Err(st) => {
                // The RMS status, marked shown, whether DCL shows it or takes
                // /ERROR.
                let shown = Cond(st.0 | 0x1000_0000);
                if let Some(label) = value(r, "ERROR") {
                    let pc = self
                        .label(&label)
                        .ok_or_else(|| DclError::with("USGOTO", &label))?;
                    self.top().pc = pc;
                    self.status = shown;
                    return Ok(None);
                }
                // %DCL-E-OPENIN (or OPENOUT), then the reason.
                let full = self.host.parse(&spec, "", "", false).unwrap_or(spec);
                let what = if mode == Mode::Read {
                    0x0003_109A
                } else {
                    0x0003_10A2
                };
                let lines = self.catalog.put_msg(
                    &[(Cond(what), vec![vms_fao::Arg::Str(&full)]), (st, vec![])],
                    self.msg_flags,
                );
                for l in lines {
                    self.print(&l.replacen("%CLI-", "%DCL-", 1));
                }
                Ok(Some(shown))
            }
        }
    }

    pub(crate) fn read_file(&mut self, spec: &str, default: &str) -> Result<String, DclError> {
        let (mut f, _) = self
            .host
            .open(spec, default, Mode::Read)
            .map_err(DclError::status)?;
        let mut text = String::new();
        while let Some(rec) = f.read().map_err(DclError::status)? {
            text.push_str(&rec);
            text.push('\n');
        }
        Ok(text)
    }

    fn set_message(&mut self, r: &mut ParseResult) -> R {
        if let Some(f) = value(r, "FILE") {
            let text = self.read_file(&unquote(&f), ".MSG")?;
            let file = vms_msg::MessageFile::from_text(&text)
                .or_else(|_| vms_msg::compile(&text).map_err(|e| format!("{e:?}")))
                .map_err(|_| DclError::status(Cond(0x0001_8292)))?;
            self.catalog.add_process(file);
        }
        for (q, flag) in [
            ("FACILITY", &mut self.msg_flags.facility),
            ("IDENTIFICATION", &mut self.msg_flags.ident),
            ("SEVERITY", &mut self.msg_flags.severity),
            ("TEXT", &mut self.msg_flags.text),
        ] {
            match r.present(q) {
                status::PRESENT => *flag = true,
                status::NEGATED => *flag = false,
                _ => {}
            }
        }
        Ok(Some(NORMAL))
    }

    fn show_symbol(&mut self, r: &mut ParseResult) -> R {
        let name = value(r, "NAME");
        let (local, global, all) = (present(r, "LOCAL"), present(r, "GLOBAL"), present(r, "ALL"));
        let show = |n: &str, v: &Value, g: bool| {
            let eq = if g { "==" } else { "=" };
            match v {
                Value::Int(i) => format!(
                    "  {n} {eq} {i}   Hex = {:08X}  Octal = {:011o}",
                    *i as u32, *i as u32
                ),
                Value::Str(s) => format!("  {n} {eq} \"{}\"", s.replace('"', "\"\"")),
            }
        };
        let mut lines = Vec::new();
        match name {
            Some(n) if !all => {
                let found = if global {
                    self.globals.get(&n).map(|v| (v.clone(), true))
                } else if local {
                    self.frames
                        .last()
                        .unwrap()
                        .locals
                        .get(&n)
                        .map(|v| (v.clone(), false))
                } else {
                    self.frames
                        .iter()
                        .rev()
                        .find_map(|f| f.locals.get(&n).map(|v| (v.clone(), false)))
                        .or_else(|| self.globals.get(&n).map(|v| (v.clone(), true)))
                };
                let (v, g) = found.ok_or_else(|| DclError::new("UNDSYM"))?;
                lines.push(show(&n, &v, g));
            }
            _ => {
                if !global {
                    let locals: Vec<_> = self
                        .top()
                        .locals
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect();
                    lines.extend(locals.iter().map(|(k, v)| show(k, v, false)));
                }
                if !local {
                    lines.extend(self.globals.iter().map(|(k, v)| show(k, v, true)));
                }
            }
        }
        for l in lines {
            self.print(&l);
        }
        Ok(Some(NORMAL))
    }
}
