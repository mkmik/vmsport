//! Parsing a command line against the tables, and `CLI$PRESENT` /
//! `CLI$GET_VALUE` on the result.

use crate::{Entity, Expr, Placement, Syntax, Tables, status};
use std::collections::HashMap;
use vms_cond::Cond;

/// A command line DCL rejects: `%DCL-W-IVQUAL, text` and ` \TOKEN\`.
#[derive(Debug, Clone, PartialEq)]
pub struct Error {
    pub code: Cond,
    pub ident: &'static str,
    pub text: &'static str,
    pub token: Option<String>,
    /// For a missing required parameter (`INSFPRM`): the prompt DCL would
    /// show in a terminal session instead of failing.
    pub prompt: Option<String>,
}

const ERRORS: &[(&str, u32, &str)] = &[
    (
        "ABVERB",
        0x38008,
        "ambiguous command verb - supply more characters",
    ),
    (
        "ABKEYW",
        0x38010,
        "ambiguous qualifier or keyword - supply more characters",
    ),
    (
        "INSFPRM",
        0x38048,
        "missing command parameters - supply all required parameters",
    ),
    (
        "IVKEYW",
        0x38060,
        "unrecognized keyword - check validity and spelling",
    ),
    (
        "IVQLOC",
        0x38078,
        "invalid qualifier location - place after a parameter",
    ),
    (
        "IVVERB",
        0x38090,
        "unrecognized command verb - check validity and spelling",
    ),
    (
        "MAXPARM",
        0x38098,
        "too many parameters - reenter command with fewer parameters",
    ),
    (
        "NOCCAT",
        0x380A8,
        "parameter concatenation not allowed - check use of plus (+)",
    ),
    (
        "NOCOMD",
        0x380B0,
        "no command on line - reenter with alphabetic first character",
    ),
    (
        "NOKEYW",
        0x380B8,
        "qualifier name is missing - append the name to the slash",
    ),
    (
        "NOLIST",
        0x380C0,
        "list of parameter values not allowed - check use of comma (,)",
    ),
    (
        "NOVALU",
        0x380D0,
        "value not allowed - remove value specification",
    ),
    (
        "NOTNEG",
        0x380D8,
        "qualifier or keyword not negatable - remove \"NO\" or omit",
    ),
    (
        "NULFIL",
        0x380E0,
        "missing or invalid file specification - respecify",
    ),
    (
        "NUMBER",
        0x380E8,
        "invalid numeric value - supply an integer",
    ),
    (
        "PARMDEL",
        0x38110,
        "invalid parameter delimiter - check use of special characters",
    ),
    (
        "VALREQ",
        0x38150,
        "missing qualifier or keyword value - supply all required values",
    ),
    (
        "ONEVAL",
        0x38158,
        "list of values not allowed - check use of comma (,)",
    ),
    (
        "IVQUAL",
        0x38240,
        "unrecognized qualifier - check validity, spelling, and placement",
    ),
    (
        "NOPAREN",
        0x38288,
        "value improperly delimited - supply parenthesis",
    ),
    (
        "CONFLICT",
        0x38258,
        "illegal combination of command elements - check documentation",
    ),
];

fn err(ident: &'static str, token: Option<String>) -> Error {
    let &(ident, code, text) = ERRORS
        .iter()
        .find(|e| e.0 == ident)
        .expect("known CLI error");
    Error {
        code: Cond(code),
        ident,
        text,
        token,
        prompt: None,
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "%DCL-{}-{}, {}",
            self.code.severity_letter(),
            self.ident,
            self.text
        )?;
        if let Some(t) = &self.token {
            write!(f, "\n \\{t}\\")?;
        }
        Ok(())
    }
}

impl std::error::Error for Error {}

/// A value with the status `CLI$GET_VALUE` returns with it.
#[derive(Debug, Clone, PartialEq)]
struct Val {
    text: String,
    status: Cond,
}

/// A local or positional qualifier after a parameter value.
#[derive(Debug, Clone, PartialEq)]
struct Local {
    qual: usize,
    negated: bool,
    values: Vec<Val>,
}

#[derive(Debug, Clone, PartialEq)]
struct PVal {
    val: Val,
    locals: Vec<Local>,
}

#[derive(Debug, Clone, Default, PartialEq)]
struct KwState {
    /// Some(true) present, Some(false) negated.
    given: Option<bool>,
    defaulted: bool,
    values: Vec<Val>,
}

#[derive(Debug, Clone, Default, PartialEq)]
struct QualState {
    given: Option<bool>,
    defaulted: bool,
    values: Vec<Val>,
    keywords: Vec<KwState>,
    /// What was typed (`NOCONFIRM`) and where, for CONFLICT.
    typed: Option<(usize, String)>,
}

/// What `CLI$DCL_PARSE` leaves for the image.
#[derive(Debug, Clone, PartialEq)]
pub struct ParseResult {
    /// The verb's full name.
    pub verb: String,
    pub image: Option<String>,
    pub routine: Option<String>,
    /// `$LINE`: the command as DCL rebuilt it.
    pub line: String,
    syntax: Syntax,
    /// Keywords of each qualifier's type, by qualifier index.
    kw_defs: Vec<Vec<Entity>>,
    params: Vec<Vec<PVal>>,
    quals: Vec<QualState>,
    cursors: HashMap<String, usize>,
    /// The parameter value last returned by `get_value`: local qualifiers
    /// answer for it.
    ctx: Option<(usize, usize)>,
}

/// Parses `line` against `tables`.
pub fn parse(tables: &Tables, line: &str) -> Result<ParseResult, Error> {
    let mut c = Cursor {
        cs: line.trim().chars().collect(),
        i: 0,
    };
    let typed_verb = c.name();
    if typed_verb.is_empty() {
        return Err(err("NOCOMD", None));
    }
    let verb = find_verb(tables, &typed_verb)?;
    let start = c.i;
    let mut syntax = verb.clone();
    let mut skip = None;
    let mut name = verb.name.clone();
    // A qualifier or keyword with SYNTAX= starts the parse over in that
    // syntax; a qualifier that switched is left out of the new parse.
    for _ in 0..8 {
        c.i = start;
        match Parser::new(tables, verb, &syntax, &name)?.run(&mut c, &typed_verb, skip)? {
            Run::Done(r) => return Ok(*r),
            Run::Switch(to, at) => {
                let s = tables.syntax(&to).expect("syntax checked at compile time");
                syntax = effective(verb, s);
                name = to;
                skip = at;
            }
        }
    }
    Err(err("IVQUAL", None))
}

/// The verb `typed` names: exactly, or by its first four characters (or
/// fewer, if unique), as DCL finds verbs.
pub fn find_verb<'t>(tables: &'t Tables, typed: &str) -> Result<&'t Syntax, Error> {
    let t = typed.to_ascii_uppercase();
    let names = || {
        tables.verbs.iter().flat_map(|v| {
            std::iter::once(&v.name)
                .chain(&v.synonyms)
                .map(move |n| (n, v))
        })
    };
    if let Some((_, v)) = names().find(|(n, _)| **n == t) {
        return Ok(v);
    }
    // Four characters are significant; fewer will do if they are unique.
    let key = &t[..t.len().min(4)];
    let mut hits: Vec<&Syntax> = names()
        .filter(|(n, _)| n.starts_with(key))
        .map(|(_, v)| v)
        .collect();
    hits.sort_by(|a, b| a.name.cmp(&b.name));
    hits.dedup_by(|a, b| a.name == b.name);
    match hits[..] {
        [v] => Ok(v),
        [] => Err(err("IVVERB", Some(t))),
        _ => Err(err("ABVERB", Some(t))),
    }
}

/// A syntax gets its verb's parameters, qualifiers and disallows when it
/// defines none of its own (and doesn't say NO...).
fn effective(verb: &Syntax, s: &Syntax) -> Syntax {
    let mut e = s.clone();
    e.name = verb.name.clone();
    if e.params.is_empty() && !e.noparams {
        e.params = verb.params.clone();
    }
    if e.quals.is_empty() && !e.noquals {
        e.quals = verb.quals.clone();
    }
    if e.disallows.is_empty() && !e.nodisallows {
        e.disallows = verb.disallows.clone();
    }
    e.image = e.image.or(verb.image.clone());
    e.routine = e.routine.or(verb.routine.clone());
    e
}

struct Cursor {
    cs: Vec<char>,
    i: usize,
}

impl Cursor {
    fn peek(&self) -> Option<char> {
        self.cs.get(self.i).copied()
    }

    fn ws(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.i += 1;
        }
    }

    fn eat(&mut self, c: char) -> bool {
        self.ws();
        if self.peek() == Some(c) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    /// Letters, digits, `$`, `_`, upcased.
    fn name(&mut self) -> String {
        self.ws();
        let mut s = String::new();
        while let Some(c) = self
            .peek()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '$' || *c == '_')
        {
            s.push(c.to_ascii_uppercase());
            self.i += 1;
        }
        s
    }

    /// One value element: quoted pieces and plain characters up to a
    /// delimiter (`stop`, whitespace, `/`, `,`, `+`, `)`); `,` and `/`
    /// inside `[]` or `<>` don't end it. Returns (as typed with quotes,
    /// unquoted), plain parts upcased.
    fn element(&mut self, stop: &str) -> (String, String) {
        self.ws();
        let (mut raw, mut plain) = (String::new(), String::new());
        let mut depth = 0;
        while let Some(c) = self.peek() {
            if c == '"' {
                self.i += 1;
                raw.push('"');
                while let Some(c) = self.peek() {
                    self.i += 1;
                    if c == '"' {
                        if self.peek() == Some('"') {
                            self.i += 1;
                            raw.push_str("\"\"");
                            plain.push('"');
                            continue;
                        }
                        break;
                    }
                    raw.push(c);
                    plain.push(c);
                }
                raw.push('"');
                continue;
            }
            match c {
                '[' | '<' => depth += 1,
                ']' | '>' => depth -= 1,
                _ => {}
            }
            if depth <= 0 && (c.is_whitespace() || "/,+)".contains(c) || stop.contains(c)) {
                break;
            }
            raw.push(c.to_ascii_uppercase());
            plain.push(c.to_ascii_uppercase());
            self.i += 1;
        }
        (raw, plain)
    }
}

enum Run {
    Done(Box<ParseResult>),
    Switch(String, Option<usize>),
}

struct Parser {
    s: Syntax,
    /// The syntax's own name (`s.name` is the verb's).
    syntax_name: String,
    kw_defs: Vec<Vec<Entity>>,
    /// Keywords of each parameter's type.
    param_kws: Vec<Vec<Entity>>,
    params: Vec<Vec<PVal>>,
    quals: Vec<QualState>,
    /// Pieces of $LINE.
    line: String,
    order: usize,
}

/// Unique-prefix lookup by name; exact matches win.
fn lookup<'e>(list: &'e [Entity], name: &str) -> Result<Option<(usize, &'e Entity)>, ()> {
    if let Some(hit) = list.iter().enumerate().find(|(_, e)| e.name == name) {
        return Ok(Some(hit));
    }
    let hits: Vec<_> = list
        .iter()
        .enumerate()
        .filter(|(_, e)| e.name.starts_with(name))
        .collect();
    match hits[..] {
        [] => Ok(None),
        [h] => Ok(Some(h)),
        _ => Err(()),
    }
}

/// Resolves `name` or `NOname`: (index, negated).
fn resolve(list: &[Entity], name: &str) -> Result<(usize, bool), Error> {
    if name.is_empty() {
        return Err(err("NOKEYW", None));
    }
    let amb = || err("ABKEYW", Some(name.to_string()));
    if let Some((i, _)) = lookup(list, name).map_err(|_| amb())? {
        return Ok((i, false));
    }
    if let Some(rest) = name.strip_prefix("NO").filter(|r| !r.is_empty())
        && let Some((i, _)) = lookup(list, rest).map_err(|_| amb())?
    {
        return Ok((i, true));
    }
    Err(err("IVQUAL", Some(name.to_string())))
}

fn number(s: &str) -> Option<i64> {
    let (radix, d) = match s.get(..2) {
        Some("%X") => (16, &s[2..]),
        Some("%O") => (8, &s[2..]),
        Some("%D") => (10, &s[2..]),
        _ => (10, s),
    };
    i64::from_str_radix(d, radix).ok()
}

/// Turns a typed element into the value an image sees, by type.
fn typed_value(typ: Option<&str>, raw: &str, plain: &str) -> Result<String, Error> {
    match typ {
        Some("$FILE" | "$INFILE" | "$OUTFILE" | "$OUTLOG" | "$QUOTED_STRING") => {
            Ok(raw.to_string())
        }
        Some("$NUMBER") => number(plain)
            .map(|n| n.to_string())
            .ok_or_else(|| err("NUMBER", Some(raw.to_string()))),
        // ponytail: $DATETIME / $DELTATIME are passed as typed; VMS converts
        // them to absolute times, which needs the time services (M1).
        _ => Ok(plain.to_string()),
    }
}

impl Parser {
    fn new(tables: &Tables, verb: &Syntax, s: &Syntax, syntax_name: &str) -> Result<Self, Error> {
        let s = if s.name == verb.name {
            s.clone()
        } else {
            effective(verb, s)
        };
        let keywords = |e: &Entity| {
            e.value
                .as_ref()
                .and_then(|v| v.typ.as_deref())
                .and_then(|t| tables.typ(t))
                .map(|t| t.keywords.clone())
                .unwrap_or_default()
        };
        let kw_defs = s.quals.iter().map(keywords).collect::<Vec<_>>();
        let param_kws = s.params.iter().map(keywords).collect();
        let quals = kw_defs
            .iter()
            .map(|k| QualState {
                keywords: vec![KwState::default(); k.len()],
                ..Default::default()
            })
            .collect();
        Ok(Parser {
            syntax_name: syntax_name.to_string(),
            param_kws,
            params: vec![Vec::new(); s.params.len()],
            quals,
            kw_defs,
            s,
            line: String::new(),
            order: 0,
        })
    }

    fn run(mut self, c: &mut Cursor, typed_verb: &str, skip: Option<usize>) -> Result<Run, Error> {
        self.line = typed_verb.to_string();
        let mut param = 0; // next parameter
        let mut ctx: Option<(usize, usize)> = None;
        loop {
            c.ws();
            let Some(ch) = c.peek() else { break };
            if ch == '/' {
                let at = c.i;
                c.i += 1;
                if let Some(sw) = self.qualifier(c, ctx, skip == Some(at))? {
                    return Ok(Run::Switch(sw, Some(at)));
                }
                continue;
            }
            if param >= self.s.params.len() {
                // VMS names the extra parameter by its leading name characters.
                let name = c.name();
                let token = if name.is_empty() {
                    c.element("").0
                } else {
                    name
                };
                return Err(err("MAXPARM", Some(token)));
            }
            let p = self.s.params[param].clone();
            let typ = p.value.as_ref().and_then(|v| v.typ.clone());
            if typ.as_deref() == Some("$REST_OF_LINE") {
                let rest = rest_of_line(&c.cs[c.i..]);
                c.i = c.cs.len();
                self.line.push(' ');
                self.line.push_str(&rest);
                self.params[param].push(PVal {
                    val: Val {
                        text: rest,
                        status: status::NORMAL,
                    },
                    locals: Vec::new(),
                });
                param += 1;
                continue;
            }
            let v = p.value.clone().unwrap_or_default();
            self.line.push(' ');
            loop {
                let (raw, plain) = c.element("");
                if raw.is_empty() {
                    return Err(match c.peek() {
                        Some(ch) if !",+".contains(ch) => err("PARMDEL", Some(ch.to_string())),
                        _ => err("NULFIL", None),
                    });
                }
                let text = typed_value(typ.as_deref(), &raw, &plain)?;
                // A keyword parameter: it must name a keyword, which may
                // switch syntax.
                if !self.param_kws[param].is_empty() {
                    let defs = &self.param_kws[param];
                    let (ki, negated) = resolve(defs, &plain).map_err(|e| {
                        if e.ident == "IVQUAL" {
                            err("IVKEYW", e.token)
                        } else {
                            e
                        }
                    })?;
                    if negated && defs[ki].negatable != Some(true) {
                        return Err(err("NOTNEG", Some(plain)));
                    }
                    if let Some(sw) = defs[ki].syntax.clone().filter(|sw| *sw != self.syntax_name) {
                        return Ok(Run::Switch(sw, None));
                    }
                }
                self.line.push_str(&text);
                let idx = self.params[param].len();
                self.params[param].push(PVal {
                    val: Val {
                        text,
                        status: status::NORMAL,
                    },
                    locals: Vec::new(),
                });
                ctx = Some((param, idx));
                // Qualifiers right after the value are local to it.
                c.ws();
                while c.peek() == Some('/') {
                    let at = c.i;
                    c.i += 1;
                    if let Some(sw) = self.qualifier(c, ctx, skip == Some(at))? {
                        return Ok(Run::Switch(sw, Some(at)));
                    }
                    c.ws();
                }
                let sep = match c.peek() {
                    Some(',') => {
                        if !v.list {
                            return Err(err("NOLIST", None));
                        }
                        status::COMMA
                    }
                    Some('+') => {
                        if v.concatenate == Some(false) {
                            return Err(err("NOCCAT", None));
                        }
                        status::CONCAT
                    }
                    _ => break,
                };
                c.i += 1;
                self.line.push(if sep == status::COMMA { ',' } else { '+' });
                self.params[param][idx].val.status = sep;
            }
            param += 1;
        }
        self.finish().map(|r| Run::Done(Box::new(r)))
    }

    /// After the `/`. Returns a syntax to switch to, if the qualifier says so.
    fn qualifier(
        &mut self,
        c: &mut Cursor,
        ctx: Option<(usize, usize)>,
        skipped: bool,
    ) -> Result<Option<String>, Error> {
        let name = c.name();
        if skipped {
            // The qualifier that switched syntax: still in $LINE, gone from
            // the new parse.
            let start = c.i;
            if c.eat('=') || c.eat(':') {
                if c.eat('(') {
                    while c.peek().is_some_and(|ch| ch != ')') {
                        c.i += 1;
                    }
                    c.eat(')');
                } else {
                    c.element("");
                }
            }
            let value: String = c.cs[start..c.i].iter().collect();
            self.line
                .push_str(&format!("/{name}{}", value.trim().to_ascii_uppercase()));
            return Ok(None);
        }
        let (qi, negated) = resolve(&self.s.quals, &name)?;
        let q = self.s.quals[qi].clone();
        if negated && q.negatable == Some(false) {
            return Err(err("NOTNEG", Some(name)));
        }
        let mut values = None;
        let mut typed_line = format!("/{name}");
        // DCL takes a colon for the equals sign: /KEY=(POSITION:9,SIZE:4).
        if c.eat('=') || c.eat(':') {
            let Some(v) = q.value.as_ref().filter(|_| !negated) else {
                return Err(err("NOVALU", Some(format!("{name}="))));
            };
            let (vals, text) = self.qual_value(c, qi, v.list, v.typ.as_deref(), false)?;
            typed_line.push('=');
            typed_line.push_str(&text);
            values = Some(vals);
        } else if q.value.as_ref().is_some_and(|v| v.required) && !negated {
            return Err(err("VALREQ", Some(name)));
        }
        if q.placement == Placement::Local && ctx.is_none() {
            return Err(err("IVQLOC", Some(name)));
        }
        if let Some(sw) = &q.syntax {
            return Ok(Some(sw.clone()));
        }
        self.line.push_str(&typed_line);
        // Defaults for a qualifier given without a value.
        if values.is_none()
            && !negated
            && let Some(d) = q.value.as_ref().and_then(|v| v.default.clone())
        {
            let mut dc = Cursor {
                cs: d.chars().collect(),
                i: 0,
            };
            let v = q.value.as_ref().unwrap();
            values = Some(
                self.qual_value(&mut dc, qi, v.list, v.typ.as_deref(), true)?
                    .0,
            );
        }
        // Local without a parameter was refused above.
        let local = if q.placement == Placement::Global {
            None
        } else {
            ctx
        };
        match local {
            Some((p, v)) => {
                let locals = &mut self.params[p][v].locals;
                locals.retain(|l| l.qual != qi);
                locals.push(Local {
                    qual: qi,
                    negated,
                    values: values.unwrap_or_default(),
                });
            }
            None => {
                let st = &mut self.quals[qi];
                st.given = Some(!negated);
                st.typed = Some((self.order, name));
                st.values = values.unwrap_or_default();
            }
        }
        self.order += 1;
        Ok(None)
    }

    /// A qualifier value: one element or a parenthesized list. Returns the
    /// values and their $LINE text.
    fn qual_value(
        &mut self,
        c: &mut Cursor,
        qi: usize,
        list: bool,
        typ: Option<&str>,
        defaulted: bool,
    ) -> Result<(Vec<Val>, String), Error> {
        c.ws();
        let paren = c.eat('(');
        let mut vals: Vec<Val> = Vec::new();
        let mut texts = Vec::new();
        loop {
            let start = c.i;
            let keywords = !self.kw_defs[qi].is_empty();
            let (raw, text) = if keywords {
                self.keyword(c, qi, defaulted)?
            } else {
                let (raw, plain) = c.element("");
                (raw.clone(), typed_value(typ, &raw, &plain)?)
            };
            if raw.is_empty() {
                return Err(err("VALREQ", None));
            }
            if !vals.is_empty() && !list {
                // VMS shows the rest of the list from the extra value on.
                let mut end = c.i;
                while end < c.cs.len() && c.cs[end] != ')' {
                    end += 1;
                }
                let tail: String = c.cs[start..(end + 1).min(c.cs.len())].iter().collect();
                return Err(err("ONEVAL", Some(tail.trim().to_ascii_uppercase())));
            }
            vals.push(Val {
                text: if keywords { raw } else { text.clone() },
                status: status::NORMAL,
            });
            texts.push(text);
            if !(paren && c.eat(',')) {
                break;
            }
            vals.last_mut().unwrap().status = status::COMMA;
        }
        if paren && !c.eat(')') {
            return Err(err("NOPAREN", None));
        }
        let text = if paren {
            format!("({})", texts.join(","))
        } else {
            texts.join(",")
        };
        Ok((vals, text))
    }

    /// One keyword element, `[NO]KEY[=value]`. Returns (as typed, $LINE text).
    fn keyword(
        &mut self,
        c: &mut Cursor,
        qi: usize,
        defaulted: bool,
    ) -> Result<(String, String), Error> {
        let start = c.i;
        let name = c.name();
        if name.is_empty() {
            return Ok((String::new(), String::new()));
        }
        let defs = &self.kw_defs[qi];
        let (ki, negated) = resolve(defs, &name).map_err(|e| {
            if e.ident == "IVQUAL" {
                err("IVKEYW", e.token)
            } else {
                e
            }
        })?;
        let k = defs[ki].clone();
        if negated && k.negatable != Some(true) {
            return Err(err("NOTNEG", Some(name)));
        }
        let mut values = Vec::new();
        if c.eat('=') || c.eat(':') {
            let Some(v) = k.value.as_ref().filter(|_| !negated) else {
                return Err(err("NOVALU", Some(format!("{name}="))));
            };
            values = self.sub_value(c, v.list, v.typ.as_deref())?;
        } else if k.value.as_ref().is_some_and(|v| v.required) && !negated {
            return Err(err("VALREQ", Some(name)));
        }
        let typed: String = c.cs[start..c.i]
            .iter()
            .collect::<String>()
            .trim()
            .to_ascii_uppercase();
        let st = &mut self.quals[qi].keywords[ki];
        st.given = Some(!negated);
        st.defaulted = defaulted;
        st.values = values;
        Ok((typed.clone(), typed))
    }

    /// A keyword's own value: one element or a parenthesized list.
    fn sub_value(
        &mut self,
        c: &mut Cursor,
        list: bool,
        typ: Option<&str>,
    ) -> Result<Vec<Val>, Error> {
        let paren = c.eat('(');
        let mut vals: Vec<Val> = Vec::new();
        loop {
            let (raw, plain) = c.element("");
            if raw.is_empty() {
                return Err(err("VALREQ", None));
            }
            if !vals.is_empty() && !list {
                return Err(err("ONEVAL", Some(raw)));
            }
            vals.push(Val {
                text: typed_value(typ, &raw, &plain)?,
                status: status::NORMAL,
            });
            if !(paren && c.eat(',')) {
                break;
            }
            vals.last_mut().unwrap().status = status::COMMA;
        }
        if paren && !c.eat(')') {
            return Err(err("NOPAREN", None));
        }
        Ok(vals)
    }

    fn finish(mut self) -> Result<ParseResult, Error> {
        for (i, p) in self.s.params.iter().enumerate() {
            let v = p.value.clone().unwrap_or_default();
            if self.params[i].is_empty() {
                if v.required {
                    let mut e = err("INSFPRM", None);
                    e.prompt = Some(p.prompt.clone().unwrap_or_else(|| p.label().to_string()));
                    return Err(e);
                }
                if let Some(d) = v.default {
                    self.params[i].push(PVal {
                        val: Val {
                            text: d,
                            status: status::DEFAULTED,
                        },
                        locals: Vec::new(),
                    });
                }
            }
        }
        for (qi, q) in self.s.quals.iter().enumerate() {
            let st = &mut self.quals[qi];
            if st.given.is_none() && q.default {
                st.defaulted = true;
            }
            // Keywords marked DEFAULT, when the qualifier names no keyword.
            if st.given.is_some() && st.keywords.iter().all(|k| k.given.is_none() || k.defaulted) {
                for (k, def) in st.keywords.iter_mut().zip(&self.kw_defs[qi]) {
                    if def.default {
                        k.defaulted = true;
                    }
                }
            }
            if st.given == Some(false) {
                for k in &mut st.keywords {
                    k.given = None;
                    k.values.clear();
                }
            }
        }
        for d in self.s.disallows.clone() {
            if self.eval(&d) {
                let mut names = Vec::new();
                refs(&d, &mut names);
                // The first of the qualifiers involved, as typed.
                let token = names
                    .iter()
                    .filter_map(|n| self.qual_index(n))
                    .filter_map(|qi| self.quals[qi].typed.clone())
                    .min()
                    .map(|t| t.1);
                return Err(err("CONFLICT", token));
            }
        }
        Ok(ParseResult {
            verb: self.s.name.clone(),
            image: self.s.image.clone(),
            routine: self.s.routine.clone(),
            line: self.line,
            syntax: self.s,
            kw_defs: self.kw_defs,
            params: self.params,
            quals: self.quals,
            cursors: HashMap::new(),
            ctx: None,
        })
    }

    fn qual_index(&self, label: &str) -> Option<usize> {
        self.s.quals.iter().position(|q| q.label() == label)
    }

    /// Present, defaulted, or present after some parameter value.
    fn qual_true(&self, qi: usize) -> bool {
        let st = &self.quals[qi];
        st.given == Some(true)
            || st.defaulted
            || self
                .params
                .iter()
                .flatten()
                .any(|v| v.locals.iter().any(|l| l.qual == qi && !l.negated))
    }

    fn qual_negated(&self, qi: usize) -> bool {
        self.quals[qi].given == Some(false)
            || self
                .params
                .iter()
                .flatten()
                .any(|v| v.locals.iter().any(|l| l.qual == qi && l.negated))
    }

    fn entity(&self, path: &[String], neg: bool) -> bool {
        if let Some(pi) = self.s.params.iter().position(|p| p.label() == path[0]) {
            return !neg && !self.params[pi].is_empty();
        }
        let Some(qi) = self.qual_index(&path[0]) else {
            return false;
        };
        match path.get(1) {
            None if neg => self.qual_negated(qi),
            None => self.qual_true(qi),
            Some(k) => {
                let Some(ki) = self.kw_defs[qi].iter().position(|e| e.label() == k) else {
                    return false;
                };
                let ks = &self.quals[qi].keywords[ki];
                if neg {
                    ks.given == Some(false)
                } else {
                    ks.given == Some(true) || ks.defaulted
                }
            }
        }
    }

    fn eval(&self, e: &Expr) -> bool {
        match e {
            Expr::Entity(p) => self.entity(p, false),
            Expr::Neg(p) => self.entity(p, true),
            Expr::Not(e) => !self.eval(e),
            Expr::And(a, b) => self.eval(a) && self.eval(b),
            Expr::Or(a, b) => self.eval(a) || self.eval(b),
            Expr::Any2(l) => l.iter().filter(|e| self.eval(e)).count() >= 2,
        }
    }
}

fn refs(e: &Expr, out: &mut Vec<String>) {
    match e {
        Expr::Entity(p) | Expr::Neg(p) => out.push(p[0].clone()),
        Expr::Not(e) => refs(e, out),
        Expr::And(a, b) | Expr::Or(a, b) => {
            refs(a, out);
            refs(b, out);
        }
        Expr::Any2(l) => l.iter().for_each(|e| refs(e, out)),
    }
}

/// `$REST_OF_LINE`: upcased and with runs of blanks squeezed, outside quotes.
fn rest_of_line(cs: &[char]) -> String {
    let mut out = String::new();
    let mut quoted = false;
    for &c in cs {
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
    out.trim().to_string()
}

impl ParseResult {
    fn param_index(&self, label: &str) -> Option<usize> {
        self.syntax.params.iter().position(|p| p.label() == label)
    }

    fn qual_index(&self, label: &str) -> Option<usize> {
        self.syntax.quals.iter().position(|q| q.label() == label)
    }

    /// The local or positional occurrence of qualifier `qi` for the
    /// parameter value last returned.
    fn local(&self, qi: usize) -> Option<&Local> {
        let (p, v) = self.ctx?;
        self.params[p][v].locals.iter().find(|l| l.qual == qi)
    }

    /// `CLI$PRESENT`.
    pub fn present(&self, name: &str) -> Cond {
        let name = name.to_ascii_uppercase();
        let path: Vec<&str> = name.split('.').collect();
        if matches!(path[..], ["$VERB"] | ["$LINE"]) {
            return status::PRESENT;
        }
        if let (Some(pi), [_]) = (self.param_index(path[0]), &path[..]) {
            return match self.params[pi].first() {
                Some(v) if v.val.status == status::DEFAULTED => status::DEFAULTED,
                Some(_) => status::PRESENT,
                None => status::ABSENT,
            };
        }
        let Some(qi) = self.qual_index(path[0]) else {
            return status::UNDEFINED;
        };
        let st = &self.quals[qi];
        if let Some(k) = path.get(1) {
            let Some(ki) = self.kw_defs[qi].iter().position(|e| e.label() == *k) else {
                return status::UNDEFINED;
            };
            let ks = &st.keywords[ki];
            return match (ks.given, ks.defaulted) {
                (_, true) => status::DEFAULTED,
                (Some(true), _) => status::PRESENT,
                (Some(false), _) => status::NEGATED,
                (None, _) => status::ABSENT,
            };
        }
        if self.syntax.quals[qi].placement != Placement::Global
            && let Some(l) = self.local(qi)
        {
            return if l.negated {
                status::LOCNEG
            } else {
                status::LOCPRES
            };
        }
        match (st.given, st.defaulted) {
            (Some(true), _) => status::PRESENT,
            (Some(false), _) => status::NEGATED,
            (None, true) => status::DEFAULTED,
            (None, false) => status::ABSENT,
        }
    }

    /// `CLI$GET_VALUE`: the next value and its status (`COMMA`, `CONCAT`
    /// or `NORMAL` for the last), or `Err(ABSENT)` when there are no more.
    pub fn get_value(&mut self, name: &str) -> Result<(String, Cond), Cond> {
        let name = name.to_ascii_uppercase();
        match name.as_str() {
            "$VERB" => return Ok((self.verb.chars().take(4).collect(), status::NORMAL)),
            "$LINE" => return Ok((self.line.clone(), status::NORMAL)),
            _ => {}
        }
        let path: Vec<&str> = name.split('.').collect();
        let cursor = *self.cursors.get(&name).unwrap_or(&0);
        let values: Vec<Val> = if let (Some(pi), [_]) = (self.param_index(path[0]), &path[..]) {
            let Some(v) = self.params[pi].get(cursor) else {
                self.ctx = None;
                return Err(status::ABSENT);
            };
            self.ctx = Some((pi, cursor));
            self.cursors.insert(name, cursor + 1);
            return Ok((v.val.text.clone(), v.val.status));
        } else {
            let Some(qi) = self.qual_index(path[0]) else {
                return Err(status::UNDEFINED);
            };
            match path.get(1) {
                Some(k) => {
                    let Some(ki) = self.kw_defs[qi].iter().position(|e| e.label() == *k) else {
                        return Err(status::UNDEFINED);
                    };
                    self.quals[qi].keywords[ki].values.clone()
                }
                None => match self.local(qi) {
                    Some(l) if !l.values.is_empty() => l.values.clone(),
                    _ => self.quals[qi].values.clone(),
                },
            }
        };
        let v = values.get(cursor).ok_or(status::ABSENT)?;
        self.cursors.insert(name, cursor + 1);
        Ok((v.text.clone(), v.status))
    }
}
