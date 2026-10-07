//! HELP for the HELP utility, DCL and programs: the help libraries, the
//! output width, and the /HELP qualifier any command takes in vmsport.

use crate::Session;
use vms_cld::{Syntax, Tables};
use vms_cond::Cond;
use vms_help::{Help, Library, Out};

/// HELP-E-OPENIN, a shared message in the HELP facility.
pub const OPENIN: Cond = Cond(0x0076_109A);

/// A help library: `spec`, defaulting to SYS$HELP: and type .HLP.
pub fn library(s: &Session, spec: &str) -> Result<Library, (Cond, String)> {
    let parsed = s
        .parse(spec, "SYS$HELP:.HLP", "")
        .map_err(|c| (c, spec.to_string()))?;
    let (path, _) = s.find(&parsed).map_err(|c| (c, parsed.expanded()))?;
    let text = std::fs::read(&path).map_err(|e| (crate::files::io_status(e), parsed.expanded()))?;
    Ok(vms_help::parse(&String::from_utf8_lossy(&text)))
}

/// The libraries HELP searches: `main` (SYS$HELP:HELPLIB), then the user
/// libraries HLP$LIBRARY, HLP$LIBRARY_1, ... as far as they are defined.
pub fn libraries(s: &Session, main: Option<&str>) -> Result<Vec<Library>, (Cond, String)> {
    let mut out = vec![library(s, main.unwrap_or("HELPLIB"))?];
    for n in 0.. {
        let name = if n == 0 {
            "HLP$LIBRARY".to_string()
        } else {
            format!("HLP$LIBRARY_{n}")
        };
        if s.names
            .translate(&name, "LNM$FILE_DEV", vms_lnm::Mode::User, false)
            .is_none()
        {
            break;
        }
        if let Ok(l) = library(s, &name) {
            out.push(l);
        }
    }
    Ok(out)
}

/// The width HELP lays lists out to: $COLUMNS, the terminal's, or 80.
pub fn width() -> usize {
    if let Some(n) = std::env::var("COLUMNS").ok().and_then(|c| c.parse().ok()) {
        return n;
    }
    // SAFETY: TIOCGWINSZ fills a winsize.
    unsafe {
        let mut ws: libc::winsize = std::mem::zeroed();
        if libc::ioctl(1, libc::TIOCGWINSZ, &mut ws) == 0 && ws.ws_col > 0 {
            return ws.ws_col as usize;
        }
    }
    80
}

/// `line` without a `/HELP` qualifier, if it has one (outside quotes).
fn strip_help(line: &str) -> Option<String> {
    let up = line.to_ascii_uppercase();
    let b = up.as_bytes();
    let mut quoted = false;
    for i in 0..b.len() {
        if b[i] == b'"' {
            quoted = !quoted;
        }
        if !quoted
            && up[i..].starts_with("/HELP")
            && b.get(i + 5)
                .is_none_or(|c| *c == b'/' || c.is_ascii_whitespace())
        {
            return Some(format!("{}{}", &line[..i], &line[i + 5..]));
        }
    }
    None
}

/// Unique-prefix lookup of a typed name among `names`.
fn resolve<'a>(names: impl Iterator<Item = &'a str>, typed: &str) -> Option<&'a str> {
    let t = typed.to_ascii_uppercase();
    let all: Vec<&str> = names.collect();
    if let Some(n) = all.iter().find(|n| **n == t) {
        return Some(n);
    }
    let hits: Vec<&&str> = all.iter().filter(|n| n.starts_with(&t)).collect();
    (hits.len() == 1).then(|| *hits[0])
}

/// The help a command with `/HELP` asks for: the verb's topic (and the
/// keyword's, as in `SET DEFAULT/HELP`, and the qualifiers' typed with it),
/// or, when no library has the verb, what its command table takes. `None`
/// if the line has no /HELP, or its verb defines a HELP qualifier itself.
pub fn for_command(s: &Session, tables: &Tables, line: &str) -> Option<Vec<String>> {
    let rest = strip_help(line)?;
    let rest = rest.trim();
    let word: String = rest
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '$' || *c == '_')
        .collect();
    let verb = vms_cld::find_verb(tables, &word).ok()?;
    if verb.quals.iter().any(|q| q.name == "HELP") {
        return None;
    }
    let mut topic = vec![verb.name.clone()];
    let mut after = rest[word.len()..].to_string();
    // A keyword parameter: SET DEFAULT, SHOW SYMBOL.
    let kw_type = verb
        .params
        .first()
        .and_then(|p| p.value.as_ref()?.typ.as_deref())
        .and_then(|t| tables.typ(t));
    if let Some(ty) = kw_type {
        let next: String = after
            .trim_start()
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        let bare = next.to_ascii_uppercase();
        let bare = bare
            .strip_prefix("NO")
            .filter(|b| resolve(ty.keywords.iter().map(|k| k.name.as_str()), b).is_some())
            .unwrap_or(&bare);
        if let Some(k) = resolve(ty.keywords.iter().map(|k| k.name.as_str()), bare) {
            topic.push(k.to_string());
            after = after.trim_start()[next.len()..].to_string();
        }
    }
    let quals: Vec<String> = after
        .split('/')
        .skip(1)
        .filter_map(|q| {
            let name: String = q
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            let name = name.to_ascii_uppercase();
            let bare = name
                .strip_prefix("NO")
                .filter(|b| resolve(verb.quals.iter().map(|q| q.name.as_str()), b).is_some())
                .unwrap_or(&name);
            resolve(verb.quals.iter().map(|q| q.name.as_str()), bare).map(|n| format!("/{n}"))
        })
        .collect();
    let help = Help {
        libraries: libraries(s, None).unwrap_or_default(),
        width: width(),
        instructions: true,
    };
    let mut tries = vec![
        topic.iter().cloned().chain(quals).collect::<Vec<_>>(),
        topic.clone(),
    ];
    tries.dedup();
    for words in tries {
        let mut out = Out::default();
        if !help.lookup(&mut out, &[], &words).is_empty() {
            out.flush();
            return Some(out.take());
        }
    }
    Some(usage(verb, tables))
}

/// What a verb's command table takes, as a help page: for verbs no
/// library documents (SET COMMAND's).
pub fn usage(verb: &Syntax, tables: &Tables) -> Vec<String> {
    let mut out = vec![String::new(), verb.name.clone(), String::new()];
    out.push(format!(
        "   No help library has {}; this is what its command table takes.",
        verb.name
    ));
    out.push(String::new());
    let mut format = format!("     {}", verb.name);
    if !verb.quals.is_empty() {
        format.push_str(" [/qualifier...]");
    }
    for p in &verb.params {
        let v = p.value.clone().unwrap_or_default();
        let mut s = p.label().to_ascii_lowercase();
        if v.list {
            s.push_str("[,...]");
        }
        format.push(' ');
        format.push_str(&if v.required { s } else { format!("[{s}]") });
    }
    out.extend([
        "   Format".to_string(),
        String::new(),
        format,
        String::new(),
    ]);
    let what = |v: &vms_cld::Value| {
        let mut w = Vec::new();
        if v.required {
            w.push("required".to_string());
        }
        if v.list {
            w.push("a list".to_string());
        }
        if let Some(t) = &v.typ {
            match tables.typ(t) {
                Some(ty) => {
                    let ks: Vec<String> = ty
                        .keywords
                        .iter()
                        .map(|k| {
                            let neg = if k.negatable == Some(true) {
                                "[NO]"
                            } else {
                                ""
                            };
                            let val = if k.value.is_some() { "=value" } else { "" };
                            format!("{neg}{}{val}", k.name)
                        })
                        .collect();
                    w.push(format!("keywords {}", ks.join(", ")));
                }
                None => w.push(t.clone()),
            }
        }
        if let Some(d) = &v.default {
            w.push(format!("default {d}"));
        }
        w.join(", ")
    };
    if !verb.params.is_empty() {
        out.extend(["   Parameters".to_string(), String::new()]);
        for p in &verb.params {
            let v = p.value.clone().unwrap_or_default();
            out.push(
                format!("     {} ({})  {}", p.name, p.label(), what(&v))
                    .trim_end()
                    .to_string(),
            );
        }
        out.push(String::new());
    }
    if !verb.quals.is_empty() {
        out.extend(["   Qualifiers".to_string(), String::new()]);
        for q in &verb.quals {
            let neg = if q.negatable != Some(false) {
                "[NO]"
            } else {
                ""
            };
            let mut s = format!("     /{neg}{}", q.name);
            if let Some(v) = &q.value {
                s.push_str(if v.required { "=value" } else { "[=value]" });
                // "=value" says it is required.
                let w = what(&vms_cld::Value {
                    required: false,
                    ..v.clone()
                });
                if !w.is_empty() {
                    s.push_str(&format!("  ({w})"));
                }
            }
            if q.default {
                s.push_str("  present by default");
            }
            out.push(s);
        }
        out.push(String::new());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_help() {
        assert_eq!(strip_help("DIRECTORY/HELP").as_deref(), Some("DIRECTORY"));
        assert_eq!(
            strip_help("set default/help [x]").as_deref(),
            Some("set default [x]")
        );
        assert_eq!(strip_help("WRITE SYS$OUTPUT \"/HELP\""), None);
        assert_eq!(strip_help("DIRECTORY/HELPER"), None);
    }
}
