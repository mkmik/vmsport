//! Parses every command line in fixtures/cld/cases.txt against VPTEST.CLD
//! and prints what CLIDUMP.MAR printed on VMS, to compare with
//! fixtures/cld/recorded/cld.log.

use vms_cld::{Tables, compile, parse, status};

fn fixture(name: &str) -> String {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/cld")
        .join(name);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// CLIDUMP's NAMES list: (is a parameter, name).
const NAMES: &[(bool, &str)] = &[
    (false, "$VERB"),
    (false, "$LINE"),
    (true, "INPUT"),
    (true, "OUTPUT"),
    (true, "TARGET"),
    (true, "TEXT"),
    (true, "FILES"),
    (true, "TITLE"),
    (false, "LOG"),
    (false, "CONFIRM"),
    (false, "COUNT"),
    (false, "NAME"),
    (false, "LIST"),
    (false, "STYLE"),
    (false, "STYLE.BRIEF"),
    (false, "STYLE.FULL"),
    (false, "STYLE.COLUMNS"),
    (false, "STYLE.HEADER"),
    (false, "OUT"),
    (false, "SINCE"),
    (false, "BEFORE"),
    (false, "EXCLUDE"),
    (false, "BINARY"),
    (false, "DELETE"),
    (false, "ALL"),
    (false, "WIDTH"),
    (false, "PAGE"),
    (false, "PAGE.NONE"),
    (false, "PAGE.SIZE"),
];
const LOCALS: &[&str] = &["EXCLUDE", "BINARY", "LOG"];

/// What CLIDUMP prints for `line`, or DCL's error.
fn dump(tables: &Tables, line: &str) -> String {
    let mut r = match parse(tables, line) {
        Ok(r) => r,
        Err(e) => return format!("{e}\n"),
    };
    let mut out = String::new();
    for (_, n) in NAMES {
        out += &format!("{n} present={:08X}\n", r.present(n).0);
    }
    for (param, n) in NAMES {
        for _ in 0..16 {
            match r.get_value(n) {
                Ok((v, st)) => {
                    out += &format!("{n} value=\"{v}\" status={:08X}\n", st.0);
                    if *param {
                        for l in LOCALS {
                            out += &format!("{n}   local {l}={:08X}\n", r.present(l).0);
                        }
                    }
                }
                Err(st) => {
                    out += &format!("{n} end={:08X}\n", st.0);
                    break;
                }
            }
        }
    }
    out
}

#[test]
fn recorded_parses() {
    let tables = compile(&fixture("VPTEST.CLD")).unwrap();
    let log = fixture("recorded/cld.log");
    let mut failures = Vec::new();
    let mut checked = 0;
    for block in log.split("@@ ").skip(1) {
        let (line, expect) = block.split_once('\n').unwrap();
        // VMS turns /SINCE=YESTERDAY into today's date minus one.
        if line.contains("YESTERDAY") {
            continue;
        }
        let got = dump(&tables, line);
        if got != expect {
            failures.push(format!("@@ {line}\n--- VMS\n{expect}--- vmsport\n{got}"));
        }
        checked += 1;
    }
    assert!(
        failures.is_empty(),
        "{} of {checked} differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert!(checked > 80);
}

#[test]
fn tables_round_trip_through_cld() {
    let tables = compile(&fixture("VPTEST.CLD")).unwrap();
    assert_eq!(compile(&tables.to_cld()).unwrap(), tables);
}

#[test]
fn statuses() {
    let tables = compile(&fixture("VPTEST.CLD")).unwrap();
    let r = parse(&tables, "VPTEST/LOG A").unwrap();
    assert_eq!(r.present("log"), status::PRESENT);
    assert_eq!(r.verb, "VPTEST");
    assert_eq!(r.image.as_deref(), Some("DKA200:[T.CLD]CLIDUMP.EXE"));
    let e = parse(&tables, "VPTEST").unwrap_err();
    assert_eq!((e.ident, e.prompt.as_deref()), ("INSFPRM", Some("From")));
}
