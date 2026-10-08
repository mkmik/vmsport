//! greet: GREET in Rust. The same command table as examples/greet/greet.c,
//! and the same output.

use libvms::cli;
use vms_cond::Cond;

const TABLES: &str = include_str!("../../../../examples/greet/GREET.CLD");
const ENTITIES: [&str; 9] = [
    "NAMES",
    "COUNT",
    "LOUD",
    "STYLE",
    "STYLE.PLAIN",
    "STYLE.FRAME",
    "STYLE.WHISPER",
    "SIGN",
    "SYMBOL",
];

fn on(name: &str) -> bool {
    cli::present(name).is_success()
}

fn value(name: &str) -> String {
    cli::get_value(name).map(|v| v.0).unwrap_or_default()
}

fn main() {
    let st = cli::dcl_parse(None, TABLES);
    if !st.is_success() {
        cli::exit(Cond(st.0 | 0x1000_0000));
    }
    for e in ENTITIES {
        cli::put_output(&format!("{e} {:08X}", cli::present(e).0));
    }
    let mut names = Vec::new();
    while let Ok((v, st)) = cli::get_value("NAMES") {
        cli::put_output(&format!("  value {v} {:08X}", st.0));
        names.push(v);
    }
    cli::put_output(&format!("foreign [{}]", cli::get_foreign(None)));

    let count: usize = if on("COUNT") {
        value("COUNT").parse().unwrap_or(0)
    } else {
        1
    };
    let frame: usize = if on("STYLE.FRAME") {
        value("STYLE.FRAME").parse().unwrap_or(0)
    } else {
        0
    };
    let sign = if on("SIGN") {
        value("SIGN")
    } else {
        String::new()
    };
    let stars = "*".repeat(frame.min(63));
    for n in &names {
        // NOBODY: a shared message with an FAO argument, %SYSTEM-E-OPENIN.
        if n == "NOBODY" {
            cli::signal(&[(Cond(0x109A), vec![vms_fao::Arg::Str(n)])]);
            continue;
        }
        for _ in 0..count {
            let mut line = format!("Hello, {n}!{}", if on("LOUD") { "!" } else { "" });
            if on("STYLE.WHISPER") {
                line = line.to_lowercase();
            }
            if frame > 0 {
                line = format!("{stars} {line} {stars}");
            }
            if !sign.is_empty() {
                line = format!("{line} -- {sign}");
            }
            cli::put_output(&line);
        }
    }
    if on("SYMBOL") {
        let sym = value("SYMBOL");
        let num = (names.len() * count).to_string();
        let st = cli::set_symbol(&sym, &num, false);
        cli::put_output(&format!("set {sym} = {num} {:08X}", st.0));
    }
    cli::exit(Cond(1));
}
