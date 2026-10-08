//! DCL expressions: values, parsing and evaluation.
//!
//! Precedence, lowest first: `.OR.`, `.AND.`, `.NOT.`, comparisons,
//! `+ -`, `* /`, unary `+ -`. Integers are 32 bits and wrap.

use crate::DclError;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i32),
    Str(String),
}

/// A number as DCL reads one: decimal, or `%X`, `%O`, `%D` with a radix.
pub fn parse_int(s: &str) -> Option<i32> {
    let s = s.trim();
    let (neg, s) = match s.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    let u = s.to_ascii_uppercase();
    let (radix, d) = match u.get(..2) {
        Some("%X") => (16, &u[2..]),
        Some("%O") => (8, &u[2..]),
        Some("%D") => (10, &u[2..]),
        _ => (10, &u[..]),
    };
    if d.is_empty() {
        return None;
    }
    let v = u32::from_str_radix(d, radix).ok()? as i32;
    Some(if neg { v.wrapping_neg() } else { v })
}

impl Value {
    /// Strings: a number if they are one, else 1 if they start with T or Y.
    pub fn to_int(&self) -> i32 {
        match self {
            Value::Int(n) => *n,
            Value::Str(s) => {
                parse_int(s).unwrap_or_else(|| i32::from(s.starts_with(['T', 't', 'Y', 'y'])))
            }
        }
    }

    pub fn to_str(&self) -> String {
        match self {
            Value::Int(n) => n.to_string(),
            Value::Str(s) => s.clone(),
        }
    }

    pub fn is_true(&self) -> bool {
        self.to_int() & 1 != 0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Int(i32),
    Str(String),
    Sym(String),
    /// `F$NAME(args)`; `Missing` for an empty argument.
    Lex(String, Vec<Expr>),
    Missing,
    Neg(Box<Expr>),
    Not(Box<Expr>),
    Bin(Op, Box<Expr>, Box<Expr>),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Op {
    Or,
    And,
    Cmp(&'static str), // EQ NE LT LE GT GE, with S for strings
    Add,
    Sub,
    Mul,
    Div,
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Int(i32),
    Str(String),
    Name(String),
    Dot(String),
    P(char),
}

fn tokenize(s: &str) -> Result<Vec<Tok>, DclError> {
    let cs: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        match c {
            c if c.is_whitespace() => i += 1,
            '"' => {
                // A string runs to its closing quote, or the end of the line.
                let mut t = String::new();
                i += 1;
                while i < cs.len() {
                    if cs[i] == '"' {
                        if cs.get(i + 1) == Some(&'"') {
                            t.push('"');
                            i += 2;
                            continue;
                        }
                        i += 1;
                        break;
                    }
                    t.push(cs[i]);
                    i += 1;
                }
                out.push(Tok::Str(t));
            }
            '.' if cs.get(i + 1).is_some_and(|c| c.is_ascii_alphabetic()) => {
                let end = cs[i + 1..]
                    .iter()
                    .position(|&c| c == '.')
                    .ok_or_else(|| DclError::new("EXPSYN"))?;
                let op: String = cs[i + 1..i + 1 + end]
                    .iter()
                    .collect::<String>()
                    .to_ascii_uppercase();
                out.push(Tok::Dot(op));
                i += end + 2;
            }
            '%' | '0'..='9' => {
                let start = i;
                i += 1;
                while i < cs.len() && cs[i].is_ascii_alphanumeric() {
                    i += 1;
                }
                let t: String = cs[start..i].iter().collect();
                out.push(Tok::Int(
                    parse_int(&t).ok_or_else(|| DclError::with("IVCHAR", &t))?,
                ));
            }
            c if c.is_alphanumeric() || c == '$' || c == '_' => {
                let start = i;
                while i < cs.len() && (cs[i].is_alphanumeric() || cs[i] == '$' || cs[i] == '_') {
                    i += 1;
                }
                out.push(Tok::Name(cs[start..i].iter().collect()));
            }
            '+' | '-' | '*' | '/' | '(' | ')' | ',' => {
                out.push(Tok::P(c));
                i += 1;
            }
            _ => return Err(DclError::with("SYMDEL", &c.to_string())),
        }
    }
    Ok(out)
}

/// The lexical functions DCL knows (checked at parse time: `IVFNAM`).
pub const LEXICALS: &[&str] = &[
    "F$CVSI",
    "F$CVTIME",
    "F$CVUI",
    "F$DIRECTORY",
    "F$EDIT",
    "F$ELEMENT",
    "F$ENVIRONMENT",
    "F$EXTRACT",
    "F$FAO",
    "F$FILE_ATTRIBUTES",
    "F$GETJPI",
    "F$GETSYI",
    "F$IDENTIFIER",
    "F$INTEGER",
    "F$LENGTH",
    "F$LOCATE",
    "F$MESSAGE",
    "F$MODE",
    "F$PARSE",
    "F$PID",
    "F$SEARCH",
    "F$STRING",
    "F$TIME",
    "F$TRNLNM",
    "F$TYPE",
    "F$USER",
    "F$VERIFY",
    "F$LOGICAL",
    "F$PRIVILEGE",
    "F$CONTEXT",
    "F$GETDVI",
    "F$CSID",
    "F$UNIQUE",
    "F$MATCH_WILD",
    "F$READLINE",
    "F$DELTA_TIME",
];

struct Parser {
    t: Vec<Tok>,
    i: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.t.get(self.i)
    }

    fn eat(&mut self, t: &Tok) -> bool {
        if self.peek() == Some(t) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn dot(&mut self, names: &[&'static str]) -> Option<&'static str> {
        if let Some(Tok::Dot(d)) = self.peek()
            && let Some(n) = names.iter().find(|n| **n == d)
        {
            self.i += 1;
            return Some(n);
        }
        None
    }

    fn or(&mut self) -> Result<Expr, DclError> {
        let mut e = self.and()?;
        while self.dot(&["OR"]).is_some() {
            e = Expr::Bin(Op::Or, Box::new(e), Box::new(self.and()?));
        }
        Ok(e)
    }

    fn and(&mut self) -> Result<Expr, DclError> {
        let mut e = self.not()?;
        while self.dot(&["AND"]).is_some() {
            e = Expr::Bin(Op::And, Box::new(e), Box::new(self.not()?));
        }
        Ok(e)
    }

    fn not(&mut self) -> Result<Expr, DclError> {
        if self.dot(&["NOT"]).is_some() {
            return Ok(Expr::Not(Box::new(self.not()?)));
        }
        self.cmp()
    }

    fn cmp(&mut self) -> Result<Expr, DclError> {
        let mut e = self.sum()?;
        const CMP: [&str; 12] = [
            "EQ", "NE", "LT", "LE", "GT", "GE", "EQS", "NES", "LTS", "LES", "GTS", "GES",
        ];
        while let Some(op) = self.dot(&CMP) {
            e = Expr::Bin(Op::Cmp(op), Box::new(e), Box::new(self.sum()?));
        }
        Ok(e)
    }

    fn sum(&mut self) -> Result<Expr, DclError> {
        let mut e = self.product()?;
        loop {
            let op = if self.eat(&Tok::P('+')) {
                Op::Add
            } else if self.eat(&Tok::P('-')) {
                Op::Sub
            } else {
                return Ok(e);
            };
            e = Expr::Bin(op, Box::new(e), Box::new(self.product()?));
        }
    }

    fn product(&mut self) -> Result<Expr, DclError> {
        let mut e = self.unary()?;
        loop {
            let op = if self.eat(&Tok::P('*')) {
                Op::Mul
            } else if self.eat(&Tok::P('/')) {
                Op::Div
            } else {
                return Ok(e);
            };
            e = Expr::Bin(op, Box::new(e), Box::new(self.unary()?));
        }
    }

    fn unary(&mut self) -> Result<Expr, DclError> {
        if self.eat(&Tok::P('-')) {
            return Ok(Expr::Neg(Box::new(self.unary()?)));
        }
        if self.eat(&Tok::P('+')) {
            return self.unary();
        }
        match self.t.get(self.i).cloned() {
            Some(Tok::Int(n)) => {
                self.i += 1;
                Ok(Expr::Int(n))
            }
            Some(Tok::Str(s)) => {
                self.i += 1;
                Ok(Expr::Str(s))
            }
            Some(Tok::P('(')) => {
                self.i += 1;
                let e = self.or()?;
                if !self.eat(&Tok::P(')')) {
                    return Err(DclError::new("EXPSYN"));
                }
                Ok(e)
            }
            Some(Tok::Name(n)) => {
                self.i += 1;
                if self.peek() == Some(&Tok::P('(')) && n.to_ascii_uppercase().starts_with("F$") {
                    let up = n.to_ascii_uppercase();
                    if !LEXICALS.contains(&up.as_str()) {
                        return Err(DclError::with("IVFNAM", &format!("{up}(")));
                    }
                    self.i += 1;
                    let mut args = Vec::new();
                    if !self.eat(&Tok::P(')')) {
                        loop {
                            let a = match self.peek() {
                                Some(Tok::P(',' | ')')) => Expr::Missing,
                                _ => self.or()?,
                            };
                            args.push(a);
                            if self.eat(&Tok::P(')')) {
                                break;
                            }
                            if !self.eat(&Tok::P(',')) {
                                return Err(DclError::new("SYMDEL"));
                            }
                        }
                    }
                    return Ok(Expr::Lex(up, args));
                }
                Ok(Expr::Sym(n))
            }
            _ => Err(DclError::new("EXPSYN")),
        }
    }
}

/// Parses a whole expression.
pub fn parse(s: &str) -> Result<Expr, DclError> {
    let mut p = Parser {
        t: tokenize(s)?,
        i: 0,
    };
    let e = p.or()?;
    if p.i != p.t.len() {
        return Err(DclError::new("SYMDEL"));
    }
    Ok(e)
}

/// Parses a comma-separated list of expressions (WRITE's items).
pub fn parse_list(s: &str) -> Result<Vec<Expr>, DclError> {
    let mut p = Parser {
        t: tokenize(s)?,
        i: 0,
    };
    let mut out = vec![p.or()?];
    while p.eat(&Tok::P(',')) {
        out.push(p.or()?);
    }
    if p.i != p.t.len() {
        return Err(DclError::new("SYMDEL"));
    }
    Ok(out)
}

/// What evaluation needs from DCL: symbols and lexical functions.
pub trait Env {
    fn symbol(&self, name: &str) -> Option<Value>;
    fn lexical(&mut self, name: &str, args: &[Expr]) -> Result<Value, DclError>;
}

pub fn eval(e: &Expr, env: &mut dyn Env) -> Result<Value, DclError> {
    Ok(match e {
        Expr::Int(n) => Value::Int(*n),
        Expr::Str(s) => Value::Str(s.clone()),
        Expr::Missing => Value::Str(String::new()),
        Expr::Sym(n) => env.symbol(n).ok_or_else(|| DclError::with("UNDSYM", n))?,
        Expr::Lex(n, args) => env.lexical(n, args)?,
        Expr::Neg(e) => Value::Int(eval(e, env)?.to_int().wrapping_neg()),
        Expr::Not(e) => Value::Int(!eval(e, env)?.to_int()),
        Expr::Bin(op, a, b) => {
            let (a, b) = (eval(a, env)?, eval(b, env)?);
            binary(*op, a, b)
        }
    })
}

fn binary(op: Op, a: Value, b: Value) -> Value {
    use Value::*;
    let (x, y) = (a.to_int(), b.to_int());
    match op {
        Op::Or => Int(x | y),
        Op::And => Int(x & y),
        Op::Add => match (a, b) {
            (Str(s), Str(t)) => Str(s + &t),
            _ => Int(x.wrapping_add(y)),
        },
        Op::Sub => match (a, b) {
            (Str(s), Str(t)) => Str(match s.find(&t) {
                Some(i) if !t.is_empty() => format!("{}{}", &s[..i], &s[i + t.len()..]),
                _ => s,
            }),
            _ => Int(x.wrapping_sub(y)),
        },
        Op::Mul => Int(x.wrapping_mul(y)),
        Op::Div => Int(if y == 0 { 0 } else { x.wrapping_div(y) }),
        Op::Cmp(c) => {
            let r = if c.ends_with('S') {
                let (s, t) = (a.to_str(), b.to_str());
                match c {
                    "EQS" => s == t,
                    "NES" => s != t,
                    "LTS" => s < t,
                    "LES" => s <= t,
                    "GTS" => s > t,
                    _ => s >= t,
                }
            } else {
                match c {
                    "EQ" => x == y,
                    "NE" => x != y,
                    "LT" => x < y,
                    "LE" => x <= y,
                    "GT" => x > y,
                    _ => x >= y,
                }
            };
            Int(i32::from(r))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct E;
    impl Env for E {
        fn symbol(&self, n: &str) -> Option<Value> {
            (n == "A").then_some(Value::Int(7))
        }
        fn lexical(&mut self, _: &str, _: &[Expr]) -> Result<Value, DclError> {
            Ok(Value::Int(0))
        }
    }

    fn ev(s: &str) -> String {
        eval(&parse(s).unwrap(), &mut E).unwrap().to_str()
    }

    #[test]
    fn arithmetic_and_strings() {
        assert_eq!(ev("1 + 2 * 3"), "7");
        assert_eq!(ev("-A / 2"), "-3");
        assert_eq!(ev("2147483647 + 1"), "-2147483648");
        assert_eq!(ev("7 .AND. 3"), "3");
        assert_eq!(ev(".NOT. 1"), "-2");
        assert_eq!(ev("\"abcabc\" - \"b\""), "acabc");
        assert_eq!(ev("\"3\" + 4"), "7");
        assert_eq!(ev("\"10\" .LT. \"9\""), "0");
        assert_eq!(ev("\"10\" .LTS. \"9\""), "1");
        assert_eq!(ev("\"YES\" .EQ. 1"), "1");
        assert_eq!(ev("%X10 + %O17"), "31");
        assert_eq!(ev("\"unterminated"), "unterminated");
    }

    #[test]
    fn errors() {
        assert_eq!(
            parse("F$NOSUCH(1)").unwrap_err().token.as_deref(),
            Some("F$NOSUCH(")
        );
        let e = eval(&parse("x").unwrap(), &mut E).unwrap_err();
        assert_eq!((e.ident, e.token.as_deref()), ("UNDSYM", Some("x")));
    }
}
