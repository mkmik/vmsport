//! The Command Definition Language: reading `.CLD` text, and writing
//! tables back out as canonical CLD.

use crate::{Entity, Expr, Placement, Syntax, Tables, Type, Value};
use std::fmt::Write;

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Word(String),
    Str(String),
    P(char),
}

/// Tokens with their line numbers. Words are upcased; `!` starts a comment.
fn lex(src: &str) -> Result<Vec<(Tok, usize)>, String> {
    let mut out = Vec::new();
    for (n, line) in src.lines().enumerate() {
        let n = n + 1;
        let mut cs = line.chars().peekable();
        while let Some(c) = cs.next() {
            match c {
                '!' => break,
                c if c.is_whitespace() => {}
                '"' => {
                    let mut s = String::new();
                    loop {
                        match cs.next() {
                            Some('"') if cs.peek() == Some(&'"') => {
                                cs.next();
                                s.push('"');
                            }
                            Some('"') => break,
                            Some(c) => s.push(c),
                            None => return Err(format!("line {n}: unterminated string")),
                        }
                    }
                    out.push((Tok::Str(s), n));
                }
                ',' | '=' | '(' | ')' | '<' | '>' => out.push((Tok::P(c), n)),
                _ => {
                    let mut w = String::from(c);
                    while let Some(&c) = cs.peek() {
                        if c.is_whitespace() || ",=()<>!\"".contains(c) {
                            break;
                        }
                        w.push(c);
                        cs.next();
                    }
                    out.push((Tok::Word(w.to_ascii_uppercase()), n));
                }
            }
        }
    }
    Ok(out)
}

struct P {
    toks: Vec<(Tok, usize)>,
    i: usize,
}

type R<T> = Result<T, String>;

impl P {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.i).map(|t| &t.0)
    }

    fn line(&self) -> usize {
        self.toks
            .get(self.i)
            .or(self.toks.last())
            .map_or(0, |t| t.1)
    }

    fn err<T>(&self, msg: &str) -> R<T> {
        Err(format!("line {}: {msg}", self.line()))
    }

    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.i).map(|t| t.0.clone());
        self.i += 1;
        t
    }

    fn is_word(&self, w: &str) -> bool {
        matches!(self.peek(), Some(Tok::Word(x)) if x == w)
    }

    fn eat(&mut self, p: char) -> bool {
        if self.peek() == Some(&Tok::P(p)) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, p: char) -> R<()> {
        if self.eat(p) {
            Ok(())
        } else {
            self.err(&format!("expected {p}"))
        }
    }

    fn word(&mut self) -> R<String> {
        match self.next() {
            Some(Tok::Word(w)) => Ok(w),
            _ => {
                self.i -= 1;
                self.err("expected a name")
            }
        }
    }

    /// A name or a quoted string.
    fn text(&mut self) -> R<String> {
        match self.next() {
            Some(Tok::Word(w) | Tok::Str(w)) => Ok(w),
            _ => {
                self.i -= 1;
                self.err("expected a name or string")
            }
        }
    }
}

/// Compiles `.CLD` source into tables.
pub fn compile(src: &str) -> Result<Tables, String> {
    let mut p = P {
        toks: lex(src)?,
        i: 0,
    };
    let mut t = Tables::default();
    while let Some(tok) = p.next() {
        let Tok::Word(w) = tok else {
            return p.err("expected a statement");
        };
        match w.as_str() {
            "MODULE" => t.module = Some(p.word()?),
            "IDENT" => t.ident = Some(p.text()?),
            "DEFINE" => match p.word()?.as_str() {
                "VERB" => t.verbs.push(syntax(&mut p, true)?),
                "SYNTAX" => t.syntaxes.push(syntax(&mut p, false)?),
                "TYPE" => t.types.push(typ(&mut p)?),
                _ => return p.err("expected VERB, SYNTAX or TYPE"),
            },
            _ => {
                p.i -= 1;
                return p.err(&format!("unknown statement {w}"));
            }
        }
    }
    check(&t)?;
    Ok(t)
}

const STATEMENTS: [&str; 3] = ["MODULE", "IDENT", "DEFINE"];

fn syntax(p: &mut P, verb: bool) -> R<Syntax> {
    let mut s = Syntax {
        name: p.word()?,
        ..Default::default()
    };
    while let Some(Tok::Word(w)) = p.peek() {
        if STATEMENTS.contains(&w.as_str()) {
            break;
        }
        let w = p.word()?;
        match w.as_str() {
            "IMAGE" => s.image = Some(p.text()?),
            "ROUTINE" | "CLIROUTINE" => s.routine = Some(p.word()?),
            "SYNONYM" if verb => s.synonyms.push(p.word()?),
            "PARAMETER" => {
                let e = entity(p, false)?;
                if !matches!(
                    e.name.as_str(),
                    "P1" | "P2" | "P3" | "P4" | "P5" | "P6" | "P7" | "P8"
                ) {
                    return p.err("parameter names are P1 to P8");
                }
                s.params.push(e);
            }
            "QUALIFIER" => s.quals.push(entity(p, false)?),
            "DISALLOW" => s.disallows.push(or(p)?),
            "NOPARAMETERS" => s.noparams = true,
            "NOQUALIFIERS" => s.noquals = true,
            "NODISALLOWS" => s.nodisallows = true,
            _ => {
                p.i -= 1;
                return p.err(&format!("unknown clause {w}"));
            }
        }
    }
    Ok(s)
}

fn typ(p: &mut P) -> R<Type> {
    let mut t = Type {
        name: p.word()?,
        ..Default::default()
    };
    while p.is_word("KEYWORD") {
        p.next();
        t.keywords.push(entity(p, true)?);
    }
    Ok(t)
}

/// `name, clause, clause...`
fn entity(p: &mut P, keyword: bool) -> R<Entity> {
    let mut e = Entity {
        name: p.word()?,
        ..Default::default()
    };
    while p.eat(',') {
        let w = p.word()?;
        match w.as_str() {
            "LABEL" => {
                p.expect('=')?;
                e.label = Some(p.word()?);
            }
            "PROMPT" => {
                p.expect('=')?;
                e.prompt = Some(p.text()?);
            }
            "DEFAULT" => e.default = true,
            "NEGATABLE" => e.negatable = Some(true),
            "NONNEGATABLE" => e.negatable = Some(false),
            "BATCH" if !keyword => e.batch = true,
            "PLACEMENT" if !keyword => {
                p.expect('=')?;
                e.placement = match p.word()?.as_str() {
                    "GLOBAL" => Placement::Global,
                    "LOCAL" => Placement::Local,
                    "POSITIONAL" => Placement::Positional,
                    _ => return p.err("expected GLOBAL, LOCAL or POSITIONAL"),
                };
            }
            "SYNTAX" => {
                p.expect('=')?;
                e.syntax = Some(p.word()?);
            }
            "VALUE" => e.value = Some(value(p)?),
            _ => {
                p.i -= 1;
                return p.err(&format!("unknown clause {w}"));
            }
        }
    }
    Ok(e)
}

fn value(p: &mut P) -> R<Value> {
    let mut v = Value::default();
    if !p.eat('(') {
        return Ok(v);
    }
    loop {
        let w = p.word()?;
        match w.as_str() {
            "REQUIRED" => v.required = true,
            "LIST" => v.list = true,
            "CONCATENATE" => v.concatenate = Some(true),
            "NOCONCATENATE" => v.concatenate = Some(false),
            "IMPCAT" => {} // implicit concatenation: not supported, harmless to accept
            "DEFAULT" => {
                p.expect('=')?;
                v.default = Some(p.text()?);
            }
            "TYPE" => {
                p.expect('=')?;
                v.typ = Some(p.word()?);
            }
            _ => {
                p.i -= 1;
                return p.err(&format!("unknown VALUE clause {w}"));
            }
        }
        if !p.eat(',') {
            break;
        }
    }
    p.expect(')')?;
    Ok(v)
}

// DISALLOW expressions: OR binds looser than AND.
fn or(p: &mut P) -> R<Expr> {
    let mut e = and(p)?;
    while p.is_word("OR") {
        p.next();
        e = Expr::Or(Box::new(e), Box::new(and(p)?));
    }
    Ok(e)
}

fn and(p: &mut P) -> R<Expr> {
    let mut e = unary(p)?;
    while p.is_word("AND") {
        p.next();
        e = Expr::And(Box::new(e), Box::new(unary(p)?));
    }
    Ok(e)
}

fn unary(p: &mut P) -> R<Expr> {
    if p.eat('(') {
        let e = or(p)?;
        p.expect(')')?;
        return Ok(e);
    }
    let w = p.word()?;
    Ok(match w.as_str() {
        "NOT" => Expr::Not(Box::new(unary(p)?)),
        "NEG" => Expr::Neg(path(p_word(p)?)),
        "ANY2" => {
            p.expect('(')?;
            let mut list = vec![or(p)?];
            while p.eat(',') {
                list.push(or(p)?);
            }
            p.expect(')')?;
            Expr::Any2(list)
        }
        _ => Expr::Entity(path(w)),
    })
}

fn p_word(p: &mut P) -> R<String> {
    if p.eat('<') {
        let w = p.word()?;
        p.expect('>')?;
        Ok(w)
    } else {
        p.word()
    }
}

fn path(w: String) -> Vec<String> {
    w.split('.').map(str::to_string).collect()
}

/// Names that other definitions refer to must exist.
fn check(t: &Tables) -> R<()> {
    let builtin = |n: &str| n.starts_with('$');
    for s in t.verbs.iter().chain(&t.syntaxes) {
        for e in s.params.iter().chain(&s.quals) {
            if let Some(syn) = &e.syntax
                && t.syntax(syn).is_none()
            {
                return Err(format!("{}: undefined syntax {syn}", s.name));
            }
            if let Some(ty) = e.value.as_ref().and_then(|v| v.typ.as_deref())
                && !builtin(ty)
                && t.typ(ty).is_none()
            {
                return Err(format!("{}: undefined type {ty}", s.name));
            }
        }
    }
    Ok(())
}

fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

fn emit_entity(out: &mut String, kind: &str, e: &Entity) {
    write!(out, "\t{kind} {}", e.name).unwrap();
    if let Some(l) = &e.label {
        write!(out, ", LABEL={l}").unwrap();
    }
    if let Some(pr) = &e.prompt {
        write!(out, ", PROMPT={}", quote(pr)).unwrap();
    }
    if e.default {
        out.push_str(", DEFAULT");
    }
    match e.negatable {
        Some(true) => out.push_str(", NEGATABLE"),
        Some(false) => out.push_str(", NONNEGATABLE"),
        None => {}
    }
    if e.batch {
        out.push_str(", BATCH");
    }
    match e.placement {
        Placement::Global => {}
        Placement::Local => out.push_str(", PLACEMENT=LOCAL"),
        Placement::Positional => out.push_str(", PLACEMENT=POSITIONAL"),
    }
    if let Some(s) = &e.syntax {
        write!(out, ", SYNTAX={s}").unwrap();
    }
    if let Some(v) = &e.value {
        let mut parts = Vec::new();
        if v.required {
            parts.push("REQUIRED".to_string());
        }
        if v.list {
            parts.push("LIST".to_string());
        }
        match v.concatenate {
            Some(true) => parts.push("CONCATENATE".to_string()),
            Some(false) => parts.push("NOCONCATENATE".to_string()),
            None => {}
        }
        if let Some(d) = &v.default {
            parts.push(format!("DEFAULT={}", quote(d)));
        }
        if let Some(ty) = &v.typ {
            parts.push(format!("TYPE={ty}"));
        }
        if parts.is_empty() {
            out.push_str(", VALUE");
        } else {
            write!(out, ", VALUE({})", parts.join(", ")).unwrap();
        }
    }
    out.push('\n');
}

fn emit_expr(e: &Expr) -> String {
    match e {
        Expr::Entity(p) => p.join("."),
        Expr::Neg(p) => format!("NEG {}", p.join(".")),
        Expr::Not(e) => format!("NOT ({})", emit_expr(e)),
        Expr::And(a, b) => format!("({} AND {})", emit_expr(a), emit_expr(b)),
        Expr::Or(a, b) => format!("({} OR {})", emit_expr(a), emit_expr(b)),
        Expr::Any2(l) => format!(
            "ANY2({})",
            l.iter().map(emit_expr).collect::<Vec<_>>().join(", ")
        ),
    }
}

pub(crate) fn emit(t: &Tables) -> String {
    let mut out = String::new();
    if let Some(m) = &t.module {
        writeln!(out, "MODULE {m}").unwrap();
    }
    if let Some(i) = &t.ident {
        writeln!(out, "IDENT {}", quote(i)).unwrap();
    }
    for (kind, list) in [("VERB", &t.verbs), ("SYNTAX", &t.syntaxes)] {
        for s in list {
            writeln!(out, "\nDEFINE {kind} {}", s.name).unwrap();
            for syn in &s.synonyms {
                writeln!(out, "\tSYNONYM {syn}").unwrap();
            }
            if let Some(i) = &s.image {
                writeln!(out, "\tIMAGE {}", quote(i)).unwrap();
            }
            if let Some(r) = &s.routine {
                writeln!(out, "\tROUTINE {r}").unwrap();
            }
            for (flag, word) in [
                (s.noparams, "NOPARAMETERS"),
                (s.noquals, "NOQUALIFIERS"),
                (s.nodisallows, "NODISALLOWS"),
            ] {
                if flag {
                    writeln!(out, "\t{word}").unwrap();
                }
            }
            for e in &s.params {
                emit_entity(&mut out, "PARAMETER", e);
            }
            for e in &s.quals {
                emit_entity(&mut out, "QUALIFIER", e);
            }
            for d in &s.disallows {
                writeln!(out, "\tDISALLOW {}", emit_expr(d)).unwrap();
            }
        }
    }
    for ty in &t.types {
        writeln!(out, "\nDEFINE TYPE {}", ty.name).unwrap();
        for k in &ty.keywords {
            emit_entity(&mut out, "KEYWORD", k);
        }
    }
    out
}
