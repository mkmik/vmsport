//! The CLI$, LIB$ and SYS$ routines an image calls about its command and
//! its process, global to the process as on VMS: CLI$DCL_PARSE,
//! CLI$PRESENT, CLI$GET_VALUE, LIB$GET_FOREIGN, DCL's symbols, messages,
//! SYS$EXIT. The C ABI (crates/vms-c) is a thin layer over these.
//!
//! Run by DCL (a verb's IMAGE), an image has its command parsed already:
//! [`present`] and [`get_value`] work without [`dcl_parse`]. Run from a Unix
//! shell, `dcl_parse(None, tables)` parses the table's verb and the argv.

use crate::image::{Context, Value, message, shell_line};
use std::sync::Mutex;
use vms_cld::ParseResult;
use vms_cond::Cond;

/// Statuses these routines return (fixtures/cabi/recorded/codes.log).
pub mod status {
    use vms_cond::Cond;
    pub const SS_NORMAL: Cond = Cond(1);
    pub const CLI_NORMAL: Cond = Cond(196609);
    /// No command to ask about.
    pub const CLI_INVREQTYP: Cond = Cond(231458);
    pub const CLI_INVTAB: Cond = Cond(231714);
    pub const LIB_STRTRU: Cond = Cond(1409041);
    pub const LIB_INVARG: Cond = Cond(1409588);
    pub const LIB_NOSUCHSYM: Cond = Cond(1409892);
    pub const LIB_NOCLI: Cond = Cond(1409916);
    pub const LIB_INVSYMNAM: Cond = Cond(1409932);
}

struct State {
    started: bool,
    ctx: Option<Context>,
    command: Option<ParseResult>,
    catalog: Option<vms_msg::Catalog>,
}

static STATE: Mutex<State> = Mutex::new(State {
    started: false,
    ctx: None,
    command: None,
    catalog: None,
});

/// The process's state, DCL's context taken (and its command parsed) the
/// first time.
fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut s = STATE.lock().unwrap_or_else(|e| e.into_inner());
    if !s.started {
        s.started = true;
        s.ctx = Context::get();
        if let Some(c) = s.ctx.as_ref().filter(|c| !c.tables.is_empty()) {
            s.command = vms_cld::compile(&c.tables)
                .ok()
                .and_then(|t| vms_cld::parse(&t, &c.line).ok());
        }
    }
    f(&mut s)
}

/// CLI$DCL_PARSE: parses `command` (verb included) with `tables` (CLD text)
/// for [`present`] and [`get_value`]. Without a command: the one DCL ran
/// us with, or the table's first verb and the argv. A command that doesn't
/// parse is shown, as DCL would, and its status returned.
pub fn dcl_parse(command: Option<&str>, tables: &str) -> Cond {
    let Ok(t) = vms_cld::compile(tables) else {
        return status::CLI_INVTAB;
    };
    with(|s| {
        let line = match (command, &s.ctx) {
            (Some(c), _) => c.to_string(),
            (None, Some(c)) if !c.tables.is_empty() => c.line.clone(),
            (None, Some(c)) => format!(
                "{} {}",
                t.verbs.first().map_or("", |v| v.name.as_str()),
                c.line
            ),
            (None, None) => shell_line(&t),
        };
        match vms_cld::parse(&t, &line) {
            Ok(r) => {
                s.command = Some(r);
                status::CLI_NORMAL
            }
            Err(e) => {
                message(&e.to_string());
                e.code
            }
        }
    })
}

/// CLI$PRESENT.
pub fn present(name: &str) -> Cond {
    with(|s| {
        s.command
            .as_ref()
            .map_or(status::CLI_INVREQTYP, |c| c.present(name))
    })
}

/// CLI$GET_VALUE: the next value and its status (COMMA, CONCAT, NORMAL),
/// or the status that there is none.
pub fn get_value(name: &str) -> Result<(String, Cond), Cond> {
    with(|s| {
        s.command
            .as_mut()
            .ok_or(status::CLI_INVREQTYP)?
            .get_value(name)
    })
}

/// LIB$GET_FOREIGN: a foreign command's parameters as DCL processed them,
/// or a verb's (the command without its verb); from a Unix shell, the
/// argv. If there are none and `prompt` is given, a line from the terminal.
pub fn get_foreign(prompt: Option<&str>) -> String {
    let line = with(|s| match &s.ctx {
        Some(c) if c.tables.is_empty() => c.line.clone(),
        Some(c) => {
            let verb = c
                .line
                .find(|ch: char| !(ch.is_alphanumeric() || ch == '$' || ch == '_'));
            verb.map_or(String::new(), |i| c.line[i..].trim().to_string())
        }
        None => std::env::args().skip(1).collect::<Vec<_>>().join(" "),
    });
    match prompt {
        Some(p) if line.is_empty() => {
            use std::io::Write;
            print!("{p}");
            let _ = std::io::stdout().flush();
            let mut l = String::new();
            let _ = std::io::stdin().read_line(&mut l);
            l.trim_end_matches(['\n', '\r']).to_string()
        }
        _ => line,
    }
}

/// A valid DCL symbol name: a letter, `$` or `_`, then those and digits.
fn symbol_name(name: &str) -> Option<String> {
    let up = name.trim().to_ascii_uppercase();
    let mut cs = up.chars();
    let ok = cs
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '$' || c == '_')
        && cs.all(|c| c.is_ascii_alphanumeric() || c == '$' || c == '_')
        && up.len() <= 255;
    ok.then_some(up)
}

/// LIB$GET_SYMBOL: a symbol's value as text, and whether it is global (a
/// local one hides a global one).
pub fn get_symbol(name: &str) -> Result<(String, bool), Cond> {
    let name = symbol_name(name).ok_or(status::LIB_INVSYMNAM)?;
    with(|s| {
        let c = s.ctx.as_ref().ok_or(status::LIB_NOCLI)?;
        let sym = c.get_symbol(&name).ok_or(status::LIB_NOSUCHSYM)?;
        let v = match &sym.value {
            Value::Int(n) => n.to_string(),
            Value::Str(t) => t.clone(),
        };
        Ok((v, sym.global))
    })
}

/// LIB$SET_SYMBOL: a string symbol, local or global, in DCL once the image
/// ends.
pub fn set_symbol(name: &str, value: &str, global: bool) -> Cond {
    let Some(name) = symbol_name(name) else {
        return status::LIB_INVSYMNAM;
    };
    with(|s| match s.ctx.as_mut() {
        Some(c) => {
            c.set_symbol(&name, Value::Str(value.to_string()), global);
            status::SS_NORMAL
        }
        None => status::LIB_NOCLI,
    })
}

/// LIB$DELETE_SYMBOL.
pub fn delete_symbol(name: &str, global: bool) -> Cond {
    let Some(name) = symbol_name(name) else {
        return status::LIB_INVSYMNAM;
    };
    with(
        |s| match s.ctx.as_mut().map(|c| c.delete_symbol(&name, global)) {
            Some(true) => status::SS_NORMAL,
            Some(false) => status::LIB_NOSUCHSYM,
            None => status::LIB_NOCLI,
        },
    )
}

/// LIB$PUT_OUTPUT: a line to SYS$OUTPUT.
pub fn put_output(line: &str) -> Cond {
    println!("{line}");
    status::SS_NORMAL
}

/// SYS$GETMSG: the text for `code`; `flags` as $GETMSG takes them (0:
/// every part).
pub fn getmsg(code: Cond, flags: u32) -> String {
    let flags = if flags == 0 {
        vms_msg::Flags::ALL
    } else {
        vms_msg::Flags::from_mask(flags)
    };
    with(|s| catalog(s).get_msg(code, flags))
}

fn catalog(s: &mut State) -> &vms_msg::Catalog {
    s.catalog.get_or_insert_with(|| {
        let mut c = vms_msg::Catalog::default();
        if let Ok(m) = vms_msg::compile(&crate::system_messages()) {
            c.add_system(m);
        }
        c
    })
}

/// LIB$SIGNAL with the default handler: shows the conditions (each with
/// its FAO arguments) as $PUTMSG does, and for a severe one ends the image
/// with its status, marked shown. Otherwise returns SS$_NORMAL.
pub fn signal(conds: &[(Cond, Vec<vms_fao::Arg>)]) -> Cond {
    let text = with(|s| catalog(s).put_msg(conds, vms_msg::Flags::ALL).join("\n"));
    message(&text);
    match conds.first() {
        Some((c, _)) if c.0 & 7 == 4 => exit(Cond(c.0 | 0x1000_0000)),
        _ => status::SS_NORMAL,
    }
}

/// LIB$STOP: as [`signal`], and the image ends whatever the severity.
pub fn stop(conds: &[(Cond, Vec<vms_fao::Arg>)]) -> ! {
    let text = with(|s| catalog(s).put_msg(conds, vms_msg::Flags::ALL).join("\n"));
    message(&text);
    exit(Cond(conds.first().map_or(0x2C, |c| c.0.0) | 0x1000_0000))
}

/// SYS$EXIT: ends the image, telling DCL its status and the symbols it set.
pub fn exit(st: Cond) -> ! {
    let ctx = with(|s| s.ctx.take());
    if let Some(c) = ctx {
        c.finish(st);
    }
    std::process::exit(if st.is_success() { 0 } else { 1 });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_a_shell() {
        // No DCL context in a test: CLI calls need a parse, symbols a CLI.
        assert_eq!(present("X"), status::CLI_INVREQTYP);
        let cld = "DEFINE VERB T\n QUALIFIER LOG\n PARAMETER P1, VALUE(LIST)\n";
        assert_eq!(dcl_parse(Some("T/LOG a,b"), cld), status::CLI_NORMAL);
        assert_eq!(present("LOG"), vms_cld::status::PRESENT);
        assert_eq!(
            get_value("P1"),
            Ok(("A".to_string(), vms_cld::status::COMMA))
        );
        assert_eq!(set_symbol("X", "1", false), status::LIB_NOCLI);
        assert_eq!(set_symbol("1X", "1", false), status::LIB_INVSYMNAM);
        assert_eq!(dcl_parse(Some("T"), "DEFINE BOGUS"), status::CLI_INVTAB);
    }
}
