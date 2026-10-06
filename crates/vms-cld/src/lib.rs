//! Command tables: the CLD language (`SET COMMAND`, CDU), and parsing DCL
//! command lines against the tables the way DCL does before it runs an
//! image (`CLI$DCL_PARSE`), with `CLI$PRESENT` and `CLI$GET_VALUE` on the
//! result.
//!
//! Parse results, statuses and errors follow what VMS does; see
//! fixtures/cld/recorded.

mod cld;
mod parse;

pub use cld::compile;
pub use parse::{Error, ParseResult, parse};

use vms_cond::Cond;

/// What one or more `.CLD` files define. Compiled tables are this, written
/// back out as canonical CLD text by [`Tables::to_cld`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Tables {
    pub module: Option<String>,
    pub ident: Option<String>,
    pub verbs: Vec<Syntax>,
    pub syntaxes: Vec<Syntax>,
    pub types: Vec<Type>,
}

/// `DEFINE VERB` or `DEFINE SYNTAX`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Syntax {
    pub name: String,
    pub image: Option<String>,
    pub routine: Option<String>,
    pub synonyms: Vec<String>,
    pub params: Vec<Entity>,
    pub quals: Vec<Entity>,
    pub disallows: Vec<Expr>,
    /// `NOPARAMETERS`, `NOQUALIFIERS`, `NODISALLOWS`.
    pub noparams: bool,
    pub noquals: bool,
    pub nodisallows: bool,
}

/// `DEFINE TYPE`: a list of keywords.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Type {
    pub name: String,
    pub keywords: Vec<Entity>,
}

/// A parameter, qualifier or keyword.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Entity {
    pub name: String,
    pub label: Option<String>,
    pub prompt: Option<String>,
    /// `DEFAULT`: present unless negated.
    pub default: bool,
    /// `NEGATABLE` / `NONNEGATABLE`; qualifiers default to negatable,
    /// keywords to not.
    pub negatable: Option<bool>,
    pub batch: bool,
    pub placement: Placement,
    pub syntax: Option<String>,
    pub value: Option<Value>,
}

impl Entity {
    /// The name `CLI$PRESENT` and `CLI$GET_VALUE` know it by.
    pub fn label(&self) -> &str {
        self.label.as_deref().unwrap_or(&self.name)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Placement {
    #[default]
    Global,
    Local,
    Positional,
}

/// `VALUE (...)`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Value {
    pub required: bool,
    pub list: bool,
    /// `CONCATENATE` / `NOCONCATENATE`; parameters default to concatenate.
    pub concatenate: Option<bool>,
    pub default: Option<String>,
    /// `$FILE`, `$NUMBER`, ... or a `DEFINE TYPE` name.
    pub typ: Option<String>,
}

/// A `DISALLOW` expression.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// An entity, by label path: `PAGE.NONE`.
    Entity(Vec<String>),
    /// `NEG entity`: explicitly negated.
    Neg(Vec<String>),
    Not(Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    /// `ANY2(...)`: at least two of them.
    Any2(Vec<Expr>),
}

/// CLI statuses (`CLI$_x`).
pub mod status {
    use super::Cond;
    pub const NORMAL: Cond = Cond(1);
    pub const PRESENT: Cond = Cond(0x3FD19);
    pub const DEFAULTED: Cond = Cond(0x3FD21);
    pub const CONCAT: Cond = Cond(0x3FD29);
    pub const LOCPRES: Cond = Cond(0x3FD31);
    pub const COMMA: Cond = Cond(0x3FD39);
    pub const ABSENT: Cond = Cond(0x381F0);
    pub const NEGATED: Cond = Cond(0x381F8);
    pub const LOCNEG: Cond = Cond(0x38230);
    /// What VMS returns for an entity the current syntax doesn't define
    /// (signalled, shown as `%DCL-F-SYNTAX`).
    pub const UNDEFINED: Cond = Cond(0x310FC);
}

impl Tables {
    pub fn verb(&self, name: &str) -> Option<&Syntax> {
        self.verbs.iter().find(|v| v.name == name)
    }

    pub fn syntax(&self, name: &str) -> Option<&Syntax> {
        self.syntaxes.iter().find(|v| v.name == name)
    }

    pub fn typ(&self, name: &str) -> Option<&Type> {
        self.types.iter().find(|t| t.name == name)
    }

    /// `SET COMMAND`: `other`'s verbs, syntaxes and types replace those
    /// with the same names, the rest are added.
    pub fn merge(&mut self, other: Tables) {
        fn put<T, F: Fn(&T) -> &str>(into: &mut Vec<T>, from: Vec<T>, name: F) {
            for x in from {
                match into.iter().position(|y| name(y) == name(&x)) {
                    Some(i) => into[i] = x,
                    None => into.push(x),
                }
            }
        }
        put(&mut self.verbs, other.verbs, |s| &s.name);
        put(&mut self.syntaxes, other.syntaxes, |s| &s.name);
        put(&mut self.types, other.types, |t| &t.name);
    }

    /// Canonical CLD text; [`compile`] reads it back to the same tables.
    pub fn to_cld(&self) -> String {
        cld::emit(self)
    }

    /// The tables as C, what SET COMMAND/OBJECT gives on VMS: the CLD text
    /// in `const char NAME[]`, for `cli$dcl_parse(0, &NAME)`.
    pub fn to_c(&self, name: &str) -> String {
        let mut out = format!(
            "/* {name}: command tables for cli$dcl_parse, made by vmsport cdu. */\nconst char {name}[] =\n"
        );
        for line in self.to_cld().lines() {
            out.push_str("\t\"");
            for b in line.bytes() {
                match b {
                    b'"' | b'\\' => out.extend(['\\', b as char]),
                    b'\t' => out.push_str("\\t"),
                    0x20..=0x7E => out.push(b as char),
                    _ => out.push_str(&format!("\\{b:03o}")),
                }
            }
            out.push_str("\\n\"\n");
        }
        out.push_str(";\n");
        out
    }
}
