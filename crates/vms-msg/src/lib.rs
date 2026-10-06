//! Message files: the `.MSG` language (MESSAGE utility), compiled message
//! files, and `$GETMSG` / `$PUTMSG` formatting.
//!
//! Codes come out as the VMS MESSAGE utility assigns them (see
//! fixtures/msg/recorded): numbers start at 1 in each facility, `.BASE`
//! moves them, the facility-specific bit is set (except in facility 0, the
//! shared system messages), and a facility that isn't `/SYSTEM` gets the
//! customer bit.

use std::fmt::Write;
use vms_cond::{Cond, Severity};
use vms_fao::Arg;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facility {
    pub name: String,
    /// 12 bits, including the customer bit.
    pub number: u16,
    /// Symbol prefix, `NAME$_` unless `/PREFIX` says otherwise.
    pub prefix: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub code: Cond,
    /// The name, as in `VPT$_NOFILES`, without the prefix.
    pub name: String,
    /// `/IDENTIFICATION`, otherwise the name.
    pub ident: String,
    pub fao_count: u8,
    pub user_value: u32,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MessageFile {
    pub title: String,
    pub ident: String,
    pub facilities: Vec<Facility>,
    pub messages: Vec<Message>,
    /// Symbols for headers: messages (prefix + name) and `.LITERAL`s.
    pub symbols: Vec<(String, i64)>,
}

/// A compile error: source line (1-based) and text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diag {
    pub line: usize,
    pub text: String,
}

impl std::fmt::Display for Diag {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "line {}: {}", self.line, self.text)
    }
}

/// Splits a line into tokens: names/numbers, `/`, `=`, `,`, and text in
/// `<...>` or `"..."` (as `Text`). A `!` outside text starts a comment.
#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Word(String),
    Text(String),
    P(char),
}

fn lex(line: &str) -> Result<Vec<Tok>, String> {
    let mut out = Vec::new();
    let mut cs = line.chars().peekable();
    while let Some(c) = cs.next() {
        match c {
            '!' => break,
            c if c.is_whitespace() => {}
            '<' | '"' => {
                let close = if c == '<' { '>' } else { '"' };
                let mut t = String::new();
                loop {
                    match cs.next() {
                        Some(c) if c == close => break,
                        Some(c) => t.push(c),
                        None => return Err("unterminated message text".into()),
                    }
                }
                out.push(Tok::Text(t));
            }
            '\'' => {
                let t: String = cs.by_ref().take_while(|&c| c != '\'').collect();
                out.push(Tok::Text(t));
            }
            '/' | '=' | ',' | '(' | ')' | '+' | '-' | '*' => out.push(Tok::P(c)),
            _ => {
                let mut w = String::from(c);
                while let Some(&c) = cs.peek() {
                    if c.is_alphanumeric() || matches!(c, '$' | '_' | '.' | '^') {
                        w.push(c);
                        cs.next();
                    } else {
                        break;
                    }
                }
                out.push(Tok::Word(w));
            }
        }
    }
    Ok(out)
}

/// Unique-prefix match of `word` against `options` (case-insensitive).
fn keyword<'a>(word: &str, options: &[&'a str]) -> Option<&'a str> {
    let w = word.to_ascii_uppercase();
    if let Some(exact) = options.iter().find(|o| **o == w) {
        return Some(exact);
    }
    let mut hits = options.iter().filter(|o| o.starts_with(&w));
    match (hits.next(), hits.next()) {
        (Some(o), None) => Some(o),
        _ => None,
    }
}

fn number(w: &str) -> Option<i64> {
    let u = w.to_ascii_uppercase();
    let (radix, digits) = match u.get(..2) {
        Some("^X") => (16, &u[2..]),
        Some("^O") => (8, &u[2..]),
        Some("^D") => (10, &u[2..]),
        _ => (10, &u[..]),
    };
    i64::from_str_radix(digits, radix).ok()
}

fn severity(word: &str) -> Option<Severity> {
    Some(
        match keyword(
            word,
            &[
                "SUCCESS",
                "INFORMATIONAL",
                "WARNING",
                "ERROR",
                "SEVERE",
                "FATAL",
            ],
        )? {
            "SUCCESS" => Severity::Success,
            "INFORMATIONAL" => Severity::Info,
            "WARNING" => Severity::Warning,
            "ERROR" => Severity::Error,
            _ => Severity::Severe,
        },
    )
}

/// `/NAME[=value]` qualifiers from `toks`, until a token that isn't `/`.
fn qualifiers(toks: &mut &[Tok]) -> Result<Vec<(String, Option<String>)>, String> {
    let mut out = Vec::new();
    while let [Tok::P('/'), Tok::Word(q), rest @ ..] = *toks {
        *toks = rest;
        let value = match *toks {
            [Tok::P('='), Tok::Word(v) | Tok::Text(v), rest @ ..] => {
                *toks = rest;
                Some(v.clone())
            }
            _ => None,
        };
        out.push((q.to_ascii_uppercase(), value));
    }
    Ok(out)
}

/// Evaluates `.LITERAL` expressions: numbers, symbols, `+ - * /` with the
/// usual precedence, and parentheses.
fn expr(toks: &[Tok], syms: &[(String, i64)]) -> Result<i64, String> {
    fn atom(t: &mut &[Tok], syms: &[(String, i64)]) -> Result<i64, String> {
        match *t {
            [Tok::P('-'), rest @ ..] => {
                *t = rest;
                Ok(-atom(t, syms)?)
            }
            [Tok::P('('), rest @ ..] => {
                *t = rest;
                let v = sum(t, syms)?;
                match *t {
                    [Tok::P(')'), rest @ ..] => {
                        *t = rest;
                        Ok(v)
                    }
                    _ => Err("missing )".into()),
                }
            }
            [Tok::Word(w), rest @ ..] => {
                *t = rest;
                number(w)
                    .or_else(|| {
                        syms.iter()
                            .find(|(n, _)| n.eq_ignore_ascii_case(w))
                            .map(|s| s.1)
                    })
                    .ok_or_else(|| format!("undefined symbol {w}"))
            }
            _ => Err("invalid expression".into()),
        }
    }
    fn product(t: &mut &[Tok], syms: &[(String, i64)]) -> Result<i64, String> {
        let mut v = atom(t, syms)?;
        while let [Tok::P(op @ ('*' | '/')), rest @ ..] = *t {
            *t = rest;
            let r = atom(t, syms)?;
            v = if *op == '*' {
                v * r
            } else {
                v.checked_div(r).ok_or("division by zero")?
            };
        }
        Ok(v)
    }
    fn sum(t: &mut &[Tok], syms: &[(String, i64)]) -> Result<i64, String> {
        let mut v = product(t, syms)?;
        while let [Tok::P(op @ ('+' | '-')), rest @ ..] = *t {
            *t = rest;
            let r = product(t, syms)?;
            v = if *op == '+' { v + r } else { v - r };
        }
        Ok(v)
    }
    let mut t = toks;
    let v = sum(&mut t, syms)?;
    if !t.is_empty() {
        return Err("invalid expression".into());
    }
    Ok(v)
}

/// Compiles `.MSG` source.
pub fn compile(src: &str) -> Result<MessageFile, Vec<Diag>> {
    let mut f = MessageFile::default();
    let mut diags = Vec::new();
    let mut sev = Severity::Warning;
    let mut next = 1i64;
    for (i, line) in src.lines().enumerate() {
        if let Err(text) = statement(line, &mut f, &mut sev, &mut next) {
            diags.push(Diag { line: i + 1, text });
        }
    }
    if diags.is_empty() { Ok(f) } else { Err(diags) }
}

fn statement(
    line: &str,
    f: &mut MessageFile,
    sev: &mut Severity,
    next: &mut i64,
) -> Result<(), String> {
    let toks = lex(line)?;
    let Some(Tok::Word(first)) = toks.first() else {
        return if toks.is_empty() {
            Ok(())
        } else {
            Err("invalid statement".into())
        };
    };
    let mut rest = &toks[1..];
    if let Some(directive) = first.strip_prefix('.') {
        let directive = keyword(
            directive,
            &[
                "TITLE", "IDENT", "FACILITY", "SEVERITY", "BASE", "LITERAL", "END", "PAGE",
            ],
        )
        .ok_or_else(|| format!("unknown directive {first}"))?;
        match directive {
            "TITLE" => {
                f.title = rest
                    .iter()
                    .map(|t| match t {
                        Tok::Word(w) | Tok::Text(w) => w.clone(),
                        Tok::P(c) => c.to_string(),
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            }
            "IDENT" => match rest {
                [Tok::Text(s) | Tok::Word(s)] => f.ident = s.clone(),
                _ => return Err("invalid .IDENT".into()),
            },
            "FACILITY" => {
                let mut quals = qualifiers(&mut rest)?;
                let [Tok::Word(name), r @ ..] = rest else {
                    return Err("missing facility name".into());
                };
                rest = r;
                if let [Tok::P(','), r @ ..] = rest {
                    rest = r;
                }
                let [Tok::Word(num), r @ ..] = rest else {
                    return Err("missing facility number".into());
                };
                rest = r;
                quals.extend(qualifiers(&mut rest)?);
                if !rest.is_empty() {
                    return Err("invalid .FACILITY".into());
                }
                let number = number(num)
                    .filter(|n| (0..=2047).contains(n))
                    .ok_or("illegal qualifier value")? as u16;
                let mut system = false;
                let mut prefix = format!("{}$_", name.to_ascii_uppercase());
                for (q, v) in quals {
                    match keyword(&q, &["PREFIX", "SYSTEM", "SHARED"]) {
                        Some("PREFIX") => {
                            prefix = v.ok_or("missing /PREFIX value")?.to_ascii_uppercase()
                        }
                        Some("SYSTEM") => system = true,
                        Some("SHARED") => {}
                        _ => return Err(format!("unknown qualifier /{q}")),
                    }
                }
                let number = if system { number } else { number | 0x800 };
                f.facilities.push(Facility {
                    name: name.to_ascii_uppercase(),
                    number,
                    prefix,
                });
                f.symbols.push((
                    format!("{}$_FACILITY", name.to_ascii_uppercase()),
                    number as i64,
                ));
                *next = 1;
            }
            "SEVERITY" => match rest {
                [Tok::Word(w)] => *sev = severity(w).ok_or("invalid severity")?,
                _ => return Err("invalid .SEVERITY".into()),
            },
            "BASE" => {
                *next = match rest {
                    [Tok::Word(w)] => number(w)
                        .filter(|n| (0..=4095).contains(n))
                        .ok_or("invalid .BASE")?,
                    _ => return Err("invalid .BASE".into()),
                }
            }
            "LITERAL" => {
                for def in rest.split(|t| *t == Tok::P(',')) {
                    let [Tok::Word(name), Tok::P('='), e @ ..] = def else {
                        return Err("invalid .LITERAL".into());
                    };
                    let v = expr(e, &f.symbols)?;
                    f.symbols.push((name.to_ascii_uppercase(), v));
                }
            }
            _ => {} // END, PAGE
        }
        return Ok(());
    }

    // NAME [/qual...] <text> [/qual...]
    let fac = f
        .facilities
        .last()
        .ok_or("constant outside scope of facility")?;
    let mut quals = qualifiers(&mut rest)?;
    let [Tok::Text(text), r @ ..] = rest else {
        return Err("missing message text".into());
    };
    rest = r;
    quals.extend(qualifiers(&mut rest)?);
    if !rest.is_empty() {
        return Err("invalid message definition".into());
    }
    let name = first.to_ascii_uppercase();
    let (mut msev, mut ident, mut fao_count, mut user_value) = (*sev, name.clone(), 0u8, 0u32);
    for (q, v) in quals {
        if let Some(s) = severity(&q) {
            msev = s;
            continue;
        }
        let v = v.ok_or_else(|| format!("missing /{q} value"));
        match keyword(&q, &["FAO_COUNT", "IDENTIFICATION", "USER_VALUE"]) {
            Some("FAO_COUNT") => {
                fao_count = number(&v?)
                    .and_then(|n| u8::try_from(n).ok())
                    .ok_or("invalid /FAO_COUNT")?
            }
            Some("IDENTIFICATION") => ident = v?.to_ascii_uppercase(),
            Some("USER_VALUE") => {
                user_value = number(&v?)
                    .and_then(|n| u32::try_from(n).ok())
                    .ok_or("invalid /USER_VALUE")?
            }
            _ => return Err(format!("unknown qualifier /{q}")),
        }
    }
    if *next > 4095 {
        return Err("message number too large".into());
    }
    // Facility 0 holds the shared system messages, which have no
    // facility-specific bit (SS$_ABORT is %X2C).
    let fac_sp = if fac.number == 0 { 0 } else { 0x8000 };
    let code = Cond(((fac.number as u32) << 16) | fac_sp | ((*next as u32) << 3) | msev as u32);
    *next += 1;
    f.symbols
        .push((format!("{}{name}", fac.prefix), code.0 as i64));
    f.messages.push(Message {
        code,
        name,
        ident,
        fao_count,
        user_value,
        text: text.clone(),
    });
    Ok(())
}

const MAGIC: &str = "vmsport-messages 1";

impl MessageFile {
    /// The compiled form `SET MESSAGE` loads: one line per facility and
    /// message, tab-separated, the text last.
    pub fn to_text(&self) -> String {
        let mut s = format!("{MAGIC}\n");
        for fac in &self.facilities {
            writeln!(s, "facility\t{}\t{}", fac.number, fac.name).unwrap();
        }
        for m in &self.messages {
            writeln!(
                s,
                "message\t{:08X}\t{}\t{}\t{}\t{}",
                m.code.0, m.ident, m.fao_count, m.user_value, m.text
            )
            .unwrap();
        }
        s
    }

    /// Reads [`to_text`](Self::to_text) output. Names and symbols are not
    /// kept there, so they come back empty.
    pub fn from_text(s: &str) -> Result<MessageFile, String> {
        let mut lines = s.lines();
        if lines.next() != Some(MAGIC) {
            return Err("not a vmsport message file".into());
        }
        let mut f = MessageFile::default();
        for line in lines {
            let bad = || format!("bad message file line: {line}");
            match line.split_once('\t') {
                Some(("facility", rest)) => {
                    let (num, name) = rest.split_once('\t').ok_or_else(bad)?;
                    let number = num.parse().map_err(|_| bad())?;
                    f.facilities.push(Facility {
                        name: name.into(),
                        number,
                        prefix: String::new(),
                    });
                }
                Some(("message", rest)) => {
                    let p: Vec<&str> = rest.splitn(5, '\t').collect();
                    let [code, ident, fao, user, text] = p[..] else {
                        return Err(bad());
                    };
                    f.messages.push(Message {
                        code: Cond(u32::from_str_radix(code, 16).map_err(|_| bad())?),
                        name: String::new(),
                        ident: ident.into(),
                        fao_count: fao.parse().map_err(|_| bad())?,
                        user_value: user.parse().map_err(|_| bad())?,
                        text: text.into(),
                    });
                }
                _ => return Err(bad()),
            }
        }
        Ok(f)
    }

    /// C `#define`s for the message codes and literals.
    pub fn c_header(&self) -> String {
        let mut s = format!(
            "/* {} {} -- generated by vmsport from a .MSG file */\n",
            self.title, self.ident
        );
        for (name, v) in &self.symbols {
            if self.messages.iter().any(|m| m.code.0 as i64 == *v) {
                writeln!(s, "#define {name} 0x{v:08X}").unwrap();
            } else {
                writeln!(s, "#define {name} {v}").unwrap();
            }
        }
        s
    }
}

/// `$GETMSG` / `SET MESSAGE` flags: which parts of `%FAC-S-IDENT, text`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Flags {
    pub text: bool,
    pub ident: bool,
    pub severity: bool,
    pub facility: bool,
}

impl Flags {
    pub const ALL: Flags = Flags {
        text: true,
        ident: true,
        severity: true,
        facility: true,
    };

    /// The `$GETMSG` bit mask: 1 text, 2 ident, 4 severity, 8 facility.
    pub fn from_mask(m: u32) -> Flags {
        Flags {
            text: m & 1 != 0,
            ident: m & 2 != 0,
            severity: m & 4 != 0,
            facility: m & 8 != 0,
        }
    }
}

/// Message files in search order: process (`SET MESSAGE`) files, then the
/// system's.
#[derive(Debug, Clone, Default)]
pub struct Catalog {
    files: Vec<(MessageFile, bool)>,
}

impl Catalog {
    /// A system file: its facility names also name codes it has no
    /// message for (`%SYSTEM-W-NOMSG`), and its facility 0 messages are
    /// the shared ones that codes without the facility-specific bit use.
    pub fn add_system(&mut self, f: MessageFile) {
        self.files.push((f, true));
    }

    /// `SET MESSAGE file`: searched before every file added earlier.
    pub fn add_process(&mut self, f: MessageFile) {
        self.files.insert(0, (f, false));
    }

    fn system_facility(&self, number: u16) -> Option<&str> {
        self.files
            .iter()
            .filter(|(_, sys)| *sys)
            .flat_map(|(f, _)| &f.facilities)
            .find(|fac| fac.number == number)
            .map(|fac| fac.name.as_str())
    }

    /// The message for `code` and its facility name, ignoring severity and
    /// control bits.
    pub fn lookup(&self, code: Cond) -> Option<(&str, &Message)> {
        if code.0 == 0 {
            return None; // VMS has no message for status 0
        }
        if code.is_fac_specific() {
            return self.files.iter().find_map(|(f, _)| {
                let m = f.messages.iter().find(|m| m.code.matches(code))?;
                let fac = f
                    .facilities
                    .iter()
                    .find(|fac| fac.number == code.facility())?;
                Some((fac.name.as_str(), m))
            });
        }
        // Shared message: the system's facility 0 message with this number,
        // named after the code's own facility.
        let msg = self
            .files
            .iter()
            .filter(|(_, sys)| *sys)
            .flat_map(|(f, _)| &f.messages)
            .find(|m| {
                m.code.facility() == 0
                    && !m.code.is_fac_specific()
                    && m.code.msg_no() == code.msg_no()
            })?;
        Some((
            self.system_facility(code.facility()).unwrap_or("NONAME"),
            msg,
        ))
    }

    /// `$GETMSG`: the unformatted message, FAO directives left in.
    pub fn get_msg(&self, code: Cond, flags: Flags) -> String {
        match self.lookup(code) {
            Some((fac, m)) => compose(fac, code.severity_letter(), &m.ident, &m.text, flags),
            None => {
                let fac = if code.0 == 0 {
                    "NONAME"
                } else {
                    self.system_facility(code.facility()).unwrap_or("NONAME")
                };
                compose(
                    fac,
                    code.severity_letter(),
                    "NOMSG",
                    &format!("Message number {:08X}", code.0),
                    flags,
                )
            }
        }
    }

    /// `$PUTMSG`: one line per condition, `%` then `-`, FAO arguments
    /// filled in. With no arguments the text is left as it is, as DCL shows
    /// an exit status.
    pub fn put_msg(&self, conds: &[(Cond, Vec<Arg>)], flags: Flags) -> Vec<String> {
        conds
            .iter()
            .enumerate()
            .map(|(i, (code, args))| {
                let mut s = self.get_msg(*code, flags);
                if !args.is_empty() {
                    s = vms_fao::fao(&s, args).unwrap_or(s);
                }
                if i > 0 && s.starts_with('%') {
                    s.replace_range(..1, "-");
                }
                s
            })
            .collect()
    }
}

fn compose(fac: &str, sev: char, ident: &str, text: &str, flags: Flags) -> String {
    let sev = sev.to_string();
    let parts: Vec<&str> = [
        (flags.facility, fac),
        (flags.severity, &sev),
        (flags.ident, ident),
    ]
    .into_iter()
    .filter_map(|(on, p)| on.then_some(p))
    .collect();
    let prefix = if parts.is_empty() {
        String::new()
    } else {
        format!("%{}", parts.join("-"))
    };
    match (prefix.is_empty(), flags.text) {
        (false, true) => format!("{prefix}, {text}"),
        (false, false) => prefix,
        (true, true) => text.to_string(),
        (true, false) => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = "
        .FACILITY  TST,5 /PREFIX=TST$_
        .SEVERITY  ERROR
        OOPS       <oops: !AS> /FAO=1
        .LITERAL   TST$K_MAX = (2 + 3) * 4, TST$K_ONE=1
    ";

    #[test]
    fn compile_and_format() {
        let f = compile(SRC).unwrap();
        let m = &f.messages[0];
        assert_eq!(m.code, Cond(0x0805_800A));
        assert_eq!(f.symbols.iter().find(|s| s.0 == "TST$K_MAX").unwrap().1, 20);
        let mut cat = Catalog::default();
        cat.add_process(MessageFile::from_text(&f.to_text()).unwrap());
        let two = cat.put_msg(
            &[(m.code, vec![Arg::Str("x")]), (m.code, vec![])],
            Flags::ALL,
        );
        assert_eq!(two, ["%TST-E-OOPS, oops: x", "-TST-E-OOPS, oops: !AS"]);
        assert!(
            cat.get_msg(Cond(0x0805_8012), Flags::ALL)
                .ends_with("-NOMSG, Message number 08058012")
        );
        assert!(f.c_header().contains("#define TST$_OOPS 0x0805800A"));
    }

    #[test]
    fn errors_name_the_line() {
        let e = compile(".FACILITY X,9999\nNAME <t>\n.BOGUS\n").unwrap_err();
        assert_eq!(e.iter().map(|d| d.line).collect::<Vec<_>>(), [1, 2, 3]);
    }
}
