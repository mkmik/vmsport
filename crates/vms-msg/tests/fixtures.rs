//! Checks vms-msg against what VMS made of fixtures/msg/TESTMSG.MSG: the
//! MESSAGE listing (codes) and F$MESSAGE / DCL output (texts).

use vms_cond::Cond;
use vms_msg::{Catalog, Flags, MessageFile, compile};

fn fixture(name: &str) -> String {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/msg")
        .join(name);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// The few system messages the recording shows, as a system file would
/// have them.
const SYSTEM: &str = "
    .FACILITY SYSTEM,0 /SYSTEM
    .SEVERITY SUCCESS
    .BASE 0
    NORMAL   <normal successful completion>
    ACCVIO   <access violation, reason mask=!XB, virtual address=!XH, PC=!XH, PS=!XL> /FATAL /FAO=4
    BADPARAM <bad parameter value> /FATAL
    .BASE 5
    ABORT    <abort> /FATAL
    .BASE 202
    CONTROLC <operation completed under CTRL/C>
    .FACILITY RMS,1 /SYSTEM
    .BASE 82
    FNF      <file not found> /ERROR
    .FACILITY CLI,3 /SYSTEM
    .BASE 72
    IVQUAL   <unrecognized qualifier - check validity, spelling, and placement> /WARNING
    .FACILITY IMGACT,77 /SYSTEM
";

fn catalog() -> Catalog {
    let mut cat = Catalog::default();
    cat.add_system(compile(SYSTEM).unwrap());
    let test = compile(&fixture("TESTMSG.MSG")).unwrap();
    // Through the compiled form, as SET MESSAGE would load it.
    cat.add_process(MessageFile::from_text(&test.to_text()).unwrap());
    cat
}

fn hex(s: &str) -> Cond {
    Cond(u32::from_str_radix(s, 16).unwrap())
}

#[test]
fn codes_match_the_listing() {
    let f = compile(&fixture("TESTMSG.MSG")).unwrap();
    let mut checked = 0;
    for line in fixture("recorded/TESTMSG.LIS").lines() {
        // "                         0CD28009     8 NORMAL		<...>"
        let w: Vec<&str> = line.split_whitespace().collect();
        let [code, _line, name, ..] = w[..] else {
            continue;
        };
        if code.len() != 8 || !code.chars().all(|c| c.is_ascii_hexdigit()) {
            continue;
        }
        let want = u32::from_str_radix(code, 16).unwrap();
        if name == ".FACILITY" {
            let fname = w[3].split(',').next().unwrap();
            let fac = f.facilities.iter().find(|x| x.name == fname).unwrap();
            assert_eq!(fac.number as u32, want, "{line}");
        } else {
            let m = f
                .messages
                .iter()
                .find(|m| m.name == name)
                .unwrap_or_else(|| panic!("{name}"));
            assert_eq!(m.code.0, want, "{line}");
        }
        checked += 1;
    }
    assert_eq!(checked, 17);
    // Symbols, as MESSAGE/SDL wrote them.
    let sym = |n: &str| {
        f.symbols
            .iter()
            .find(|s| s.0 == n)
            .unwrap_or_else(|| panic!("{n}"))
            .1
    };
    assert_eq!(sym("VPT__NORMAL"), 0x0CD28009);
    assert_eq!(sym("OTHER$_FIVE"), 0x004D802A);
    assert_eq!(sym("VPT__TWICE"), 510);
    assert_eq!(sym("VPT$_FACILITY"), 3282);
}

#[test]
fn f_message() {
    let cat = catalog();
    let log = fixture("recorded/msg.log");
    let mut checked = 0;
    for line in log.lines() {
        let Some((kind, rest)) = line.split_once(' ') else {
            continue;
        };
        match kind {
            "msg" => {
                let (code, text) = rest.split_once(' ').unwrap();
                let code = hex(code);
                // Shared messages beyond the few in SYSTEM, and VMS's own IMGACT
                // messages (facility 77, like OTHER), need the real system file.
                if (!code.is_fac_specific() && code.msg_no() > 2) || text.starts_with("%IMGACT-") {
                    continue;
                }
                assert_eq!(cat.get_msg(code, Flags::ALL), text, "{line}");
            }
            "sev" => {
                let (sev, text) = rest.split_once(' ').unwrap();
                let code = Cond(0x0CD28010 + sev.parse::<u32>().unwrap());
                assert_eq!(cat.get_msg(code, Flags::ALL), text, "{line}");
            }
            "none" if !rest.contains("MNSS") => {
                let code = hex(rest.rsplit(' ').next().unwrap());
                assert_eq!(cat.get_msg(code, Flags::ALL), rest, "{line}");
            }
            "sys" => {
                let code = [
                    (1, "NORMAL"),
                    (0x2C, "ABORT"),
                    (0x651, "CONTROLC"),
                    (0x18292, "FNF"),
                    (0x38240, "IVQUAL"),
                ]
                .iter()
                .find(|(_, n)| rest.contains(&format!("-{n},")))
                .unwrap()
                .0;
                assert_eq!(cat.get_msg(Cond(code), Flags::ALL), rest, "{line}");
            }
            _ => continue,
        }
        checked += 1;
    }
    assert!(checked > 30, "{checked}");

    // F$MESSAGE(code, components)
    let comps: Vec<&str> = log
        .lines()
        .filter_map(|l| l.strip_prefix("comp "))
        .collect();
    let code = Cond(0x0CD28010);
    let f = |t, i, s, fa| {
        cat.get_msg(
            code,
            Flags {
                text: t,
                ident: i,
                severity: s,
                facility: fa,
            },
        )
    };
    assert_eq!(
        comps,
        [
            f(true, false, false, false),
            f(false, true, false, false),
            f(false, false, true, false),
            f(false, false, false, true),
            f(false, true, false, true),
            f(true, false, true, false)
        ]
    );
}

#[test]
fn dcl_shows_exit_status() {
    let cat = catalog();
    let log = fixture("recorded/msg.log");
    let lines: Vec<&str> = log
        .lines()
        .skip_while(|l| !l.starts_with("comp "))
        .collect();
    let mut shown: Option<&str> = None;
    let mut checked = 0;
    for line in &lines {
        if let Some(code) = line.strip_prefix("status ") {
            let code = hex(code);
            // DCL shows failures, unless the inhibit bit says it was shown already.
            let expect = (!code.is_success() && !code.inhibit_msg())
                .then(|| cat.put_msg(&[(code, vec![])], Flags::ALL)[0].clone());
            assert_eq!(shown.map(str::to_string), expect, "status {code}");
            shown = None;
            checked += 1;
        } else if line.starts_with('%') {
            shown = Some(line);
        }
    }
    assert_eq!(checked, 7);

    // SET MESSAGE /NOFACILITY etc., the flags accumulating.
    let tail = &lines[lines.len() - 4..];
    let abort = Cond(0x2C);
    let mut fl = Flags::ALL;
    fl.facility = false;
    assert_eq!(tail[0], cat.get_msg(abort, fl));
    (fl.ident, fl.facility) = (false, true);
    assert_eq!(tail[1], cat.get_msg(abort, fl));
    (fl.severity, fl.ident) = (false, true);
    assert_eq!(tail[2], cat.get_msg(abort, fl));
    (fl.text, fl.severity) = (false, true);
    assert_eq!(tail[3], cat.get_msg(abort, fl));
}
