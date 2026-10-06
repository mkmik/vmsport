//! Lexical functions (`F$...`).

use crate::expr::{self, Expr, Value};
use crate::{Dcl, DclError};
use vms_cond::Cond;
use vms_filespec::FileSpec;
use vms_msg::Flags;

fn arity(args: &[Expr], min: usize, max: usize) -> Result<(), DclError> {
    if args.len() < min || args.len() > max || args[..min].contains(&Expr::Missing) {
        return Err(DclError::new("SYMDEL"));
    }
    Ok(())
}

fn s(v: String) -> Value {
    Value::Str(v)
}

/// `TRUE` / `FALSE`, as several lexicals answer.
fn tf(b: bool) -> Value {
    s(if b { "TRUE" } else { "FALSE" }.to_string())
}

pub(crate) fn call(d: &mut Dcl, name: &str, args: &[Expr]) -> Result<Value, DclError> {
    let mut ev = |i: usize| -> Result<Value, DclError> {
        match args.get(i) {
            Some(a) => expr::eval(a, d),
            None => Ok(Value::Str(String::new())),
        }
    };
    Ok(match name {
        "F$LENGTH" => {
            arity(args, 1, 1)?;
            Value::Int(ev(0)?.to_str().chars().count() as i32)
        }
        "F$LOCATE" => {
            arity(args, 2, 2)?;
            let (sub, st) = (ev(0)?.to_str(), ev(1)?.to_str());
            let pos = st
                .find(&sub)
                .map_or(st.chars().count(), |b| st[..b].chars().count());
            Value::Int(pos as i32)
        }
        "F$EXTRACT" => {
            arity(args, 3, 3)?;
            let (start, len, st) = (ev(0)?.to_int(), ev(1)?.to_int(), ev(2)?.to_str());
            if start < 0 || len < 0 {
                s(String::new())
            } else {
                s(st.chars().skip(start as usize).take(len as usize).collect())
            }
        }
        "F$ELEMENT" => {
            arity(args, 3, 3)?;
            let (n, delim, st) = (ev(0)?.to_int(), ev(1)?.to_str(), ev(2)?.to_str());
            let dc = delim
                .chars()
                .next()
                .ok_or_else(|| DclError::new("SYMDEL"))?;
            match usize::try_from(n).ok().and_then(|n| st.split(dc).nth(n)) {
                Some(e) => s(e.to_string()),
                None => s(dc.to_string()),
            }
        }
        "F$EDIT" => {
            arity(args, 2, 2)?;
            let (st, list) = (ev(0)?.to_str(), ev(1)?.to_str().to_ascii_uppercase());
            s(edit(&st, &list)?)
        }
        "F$INTEGER" => {
            arity(args, 1, 1)?;
            Value::Int(ev(0)?.to_int())
        }
        "F$STRING" => {
            arity(args, 1, 1)?;
            s(ev(0)?.to_str())
        }
        "F$TYPE" => {
            arity(args, 1, 1)?;
            let Expr::Sym(n) = &args[0] else {
                return Err(DclError::new("SYMDEL"));
            };
            s(match d.symbol(n) {
                Some(Value::Int(_)) => "INTEGER".into(),
                Some(Value::Str(v)) if expr::parse_int(&v).is_some() => "INTEGER".into(),
                Some(Value::Str(_)) => "STRING".into(),
                None => String::new(),
            })
        }
        "F$CVUI" | "F$CVSI" => {
            arity(args, 3, 3)?;
            let (pos, size, st) = (ev(0)?.to_int(), ev(1)?.to_int(), ev(2)?.to_str());
            if !(0..=32).contains(&size) || pos < 0 || (pos + size) as usize > st.len() * 8 {
                return Err(DclError::new("INVRANGE"));
            }
            let bytes: Vec<u8> = st.chars().map(|c| c as u32 as u8).collect();
            let mut v: u64 = 0;
            for i in 0..size as usize {
                let bit = pos as usize + i;
                v |= u64::from((bytes[bit / 8] >> (bit % 8)) & 1) << i;
            }
            let v = if name == "F$CVSI" && size > 0 && v >> (size - 1) & 1 == 1 {
                (v as i64 - (1i64 << size)) as i32
            } else {
                v as i32
            };
            Value::Int(v)
        }
        "F$FAO" => {
            if args.is_empty() || args[0] == Expr::Missing {
                return Err(DclError::new("SYMDEL"));
            }
            let ctl = ev(0)?.to_str();
            let vals: Vec<Value> = (1..args.len()).map(&mut ev).collect::<Result<_, _>>()?;
            let fa: Vec<vms_fao::Arg> = vals
                .iter()
                .map(|v| match v {
                    Value::Int(n) => vms_fao::Arg::Num(*n as i64),
                    Value::Str(t) => vms_fao::Arg::Str(t),
                })
                .collect();
            s(vms_fao::fao(&ctl, &fa).map_err(|_| DclError::status(Cond(0x14)))?)
        }
        "F$MESSAGE" => {
            arity(args, 1, 2)?;
            let code = Cond(ev(0)?.to_int() as u32);
            let flags = match args.get(1) {
                Some(Expr::Missing) | None => Flags::ALL,
                Some(_) => {
                    let list = ev(1)?.to_str().to_ascii_uppercase();
                    let has = |k: &str| {
                        list.split(',')
                            .any(|p| !p.is_empty() && k.starts_with(p.trim()))
                    };
                    Flags {
                        text: has("TEXT"),
                        ident: has("IDENT"),
                        severity: has("SEVERITY"),
                        facility: has("FACILITY"),
                    }
                }
            };
            s(d.catalog.get_msg(code, flags))
        }
        "F$ENVIRONMENT" => {
            arity(args, 1, 1)?;
            let item = ev(0)?.to_str().to_ascii_uppercase();
            environment(d, &item)?
        }
        "F$VERIFY" => {
            let old = Value::Int(i32::from(d.verify));
            if let Some(a) = args.first().filter(|a| **a != Expr::Missing) {
                d.verify = expr::eval(a, d)?.is_true();
            }
            old
        }
        "F$MODE" => s("INTERACTIVE".into()),
        "F$TIME" => s(vms_time::asctim(d.host.now(), false)),
        "F$CVTIME" => {
            arity(args, 0, 3)?;
            let opt = |i: usize,
                       ev: &mut dyn FnMut(usize) -> Result<Value, DclError>|
             -> Result<Option<String>, DclError> {
                match args.get(i) {
                    None | Some(Expr::Missing) => Ok(None),
                    Some(_) => Ok(Some(ev(i)?.to_str())),
                }
            };
            let (input, format, field) = (opt(0, &mut ev)?, opt(1, &mut ev)?, opt(2, &mut ev)?);
            let now = d.host.now();
            vms_time::cvtime(input.as_deref(), format.as_deref(), field.as_deref(), now)
                .map(s)
                .map_err(|e| DclError {
                    code: e.code,
                    ident: e.ident,
                    token: e.token,
                })?
        }
        "F$PARSE" => {
            arity(args, 1, 5)?;
            let spec = ev(0)?.to_str();
            let (default, related) = (ev(1)?.to_str(), ev(2)?.to_str());
            let field = ev(3)?.to_str().to_ascii_uppercase();
            let syntax_only = "SYNTAX_ONLY"
                .starts_with(ev(4)?.to_str().to_ascii_uppercase().as_str())
                && !ev(4)?.to_str().is_empty();
            match d
                .host
                .parse(&spec, &default, &related, syntax_only, field.is_empty())
            {
                None => s(String::new()),
                Some(full) if field.is_empty() => s(full),
                Some(full) => s(parse_field(&full, &field)?),
            }
        }
        "F$SEARCH" => {
            arity(args, 1, 2)?;
            let spec = ev(0)?.to_str();
            let stream = ev(1)?.to_int() as u32;
            s(d.host.search(&spec, stream).unwrap_or_default())
        }
        "F$TRNLNM" | "F$LOGICAL" => {
            arity(args, 1, 6)?;
            let n = ev(0)?.to_str();
            let table = ev(1)?.to_str();
            let index = ev(2)?.to_int().max(0) as u32;
            let item = ev(5)?.to_str().to_ascii_uppercase();
            let table = if table.is_empty() {
                "LNM$DCL_LOGICAL".to_string()
            } else {
                table
            };
            let item = if item.is_empty() {
                "VALUE".to_string()
            } else {
                item
            };
            s(d.host.trnlnm(&n, &table, index, &item).unwrap_or_default())
        }
        "F$DIRECTORY" => {
            let def = d.host.default_directory();
            s(def
                .find('[')
                .map(|i| def[i..].to_string())
                .unwrap_or_default())
        }
        "F$GETJPI" => {
            arity(args, 2, 2)?;
            let item = ev(1)?.to_str().to_ascii_uppercase();
            s(d.host.info(&item).unwrap_or_default())
        }
        "F$GETSYI" => {
            arity(args, 1, 3)?;
            let item = ev(0)?.to_str().to_ascii_uppercase();
            s(d.host.info(&item).unwrap_or_default())
        }
        "F$USER" => s(d.host.info("UIC").unwrap_or_default()),
        "F$PID" => s(d.host.info("PID").unwrap_or_default()),
        "F$UNIQUE" => s(format!("{:016X}", d.host.now())),
        "F$MATCH_WILD" => {
            arity(args, 2, 2)?;
            let (c, p) = (
                ev(0)?.to_str().to_ascii_uppercase(),
                ev(1)?.to_str().to_ascii_uppercase(),
            );
            tf(wild(
                &c.chars().collect::<Vec<_>>(),
                &p.chars().collect::<Vec<_>>(),
            ))
        }
        _ => return Err(DclError::with("IVFNAM", &format!("{name}("))),
    })
}

/// `*` and `%` wildcards.
fn wild(c: &[char], p: &[char]) -> bool {
    match p.first() {
        None => c.is_empty(),
        Some('*') => (0..=c.len()).any(|i| wild(&c[i..], &p[1..])),
        Some('%') => !c.is_empty() && wild(&c[1..], &p[1..]),
        Some(x) => c.first() == Some(x) && wild(&c[1..], &p[1..]),
    }
}

fn edit(st: &str, list: &str) -> Result<String, DclError> {
    let mut out = st.to_string();
    let words: Vec<&str> = list.split(',').map(str::trim).collect();
    let has = |k: &str| words.iter().any(|w| !w.is_empty() && k.starts_with(w));
    for w in &words {
        if ![
            "COLLAPSE",
            "COMPRESS",
            "LOWERCASE",
            "TRIM",
            "UNCOMMENT",
            "UPCASE",
        ]
        .iter()
        .any(|k| !w.is_empty() && k.starts_with(w))
        {
            return Err(DclError::with("IVKEYW", w));
        }
    }
    // Text in quotes is left alone, except by TRIM and UNCOMMENT.
    let outside = |s: &str, f: &dyn Fn(&str) -> String| -> String {
        let mut res = String::new();
        for (i, part) in s.split('"').enumerate() {
            if i > 0 {
                res.push('"');
            }
            res.push_str(&if i % 2 == 0 {
                f(part)
            } else {
                part.to_string()
            });
        }
        res
    };
    if has("UNCOMMENT") {
        let mut q = false;
        if let Some(i) = out.char_indices().find(|&(_, c)| {
            if c == '"' {
                q = !q;
            }
            c == '!' && !q
        }) {
            out.truncate(i.0);
        }
    }
    if has("COLLAPSE") {
        out = outside(&out, &|p| {
            p.chars().filter(|c| !matches!(c, ' ' | '\t')).collect()
        });
    }
    if has("COMPRESS") {
        out = outside(&out, &|p| {
            let mut r = String::new();
            for c in p.chars() {
                let c = if c == '\t' { ' ' } else { c };
                if !(c == ' ' && r.ends_with(' ')) {
                    r.push(c);
                }
            }
            r
        });
    }
    if has("TRIM") {
        out = out.trim_matches([' ', '\t']).to_string();
    }
    if has("UPCASE") {
        out = outside(&out, &|p| p.to_uppercase());
    }
    if has("LOWERCASE") {
        out = outside(&out, &|p| p.to_lowercase());
    }
    Ok(out)
}

fn parse_field(full: &str, field: &str) -> Result<String, DclError> {
    let f: FileSpec = full.parse().map_err(|_| DclError::new("SYMDEL"))?;
    const FIELDS: [&str; 6] = ["NODE", "DEVICE", "DIRECTORY", "NAME", "TYPE", "VERSION"];
    let k = FIELDS
        .iter()
        .find(|k| k.starts_with(field))
        .ok_or_else(|| DclError::with("IVKEYW", field))?;
    Ok(match *k {
        "NODE" => f.node.map(|n| format!("{n}::")).unwrap_or_default(),
        "DEVICE" => f.device.map(|n| format!("{n}:")).unwrap_or_default(),
        "DIRECTORY" => f.directory.map(|d| d.to_string()).unwrap_or_default(),
        "NAME" => f.name,
        "TYPE" => format!(".{}", f.typ.unwrap_or_default()),
        _ => format!(";{}", f.version.map(|v| v.to_string()).unwrap_or_default()),
    })
}

fn environment(d: &mut Dcl, item: &str) -> Result<Value, DclError> {
    let f = d.frames.last().unwrap();
    Ok(match item {
        "DEPTH" => Value::Int(d.depth() as i32),
        "PROCEDURE" => s(f.proc_.as_ref().map(|p| p.spec.clone()).unwrap_or_default()),
        "DEFAULT" => s(d.host.default_directory()),
        "VERIFY_PROCEDURE" | "VERIFY_IMAGE" => tf(d.verify),
        "INTERACTIVE" => tf(true),
        "CAPTIVE" => tf(false),
        "MAX_DEPTH" => Value::Int(32),
        "PROMPT" => s("$ ".into()),
        "ON_SEVERITY" => s(if !f.on {
            "NONE".into()
        } else {
            ["WARNING", "ERROR", "SEVERE_ERROR"][f.on_level as usize - 1].into()
        }),
        "ON_CONTROL_Y" => tf(false),
        "MESSAGE" => {
            let fl = d.msg_flags;
            let mut m = String::new();
            for (on, k) in [
                (fl.facility, "FACILITY"),
                (fl.ident, "IDENTIFICATION"),
                (fl.severity, "SEVERITY"),
                (fl.text, "TEXT"),
            ] {
                m.push_str(&format!("/{}{k}", if on { "" } else { "NO" }));
            }
            s(m)
        }
        "SYMBOL_SCOPE" => s("/LOCAL/GLOBAL".into()),
        _ => return Err(DclError::with("IVKEYW", item)),
    })
}
