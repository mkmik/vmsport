//! SEARCH: lines of files that hold strings. With several files or a
//! wildcard, each file with matches gets a heading.

use libvms::files::Reader;
use std::io::Write;
use vms_cond::Cond;
use vms_fao::Arg;
use vms_utils::{Util, W, shr, texts};

/// SEARCH's own messages (sys/SYSMSG/SEARCH.MSG): facility 215 with the
/// customer bit, facility-specific.
fn own(msgno: u32, sev: u32) -> Cond {
    Cond(0x08D7_8000 | msgno << 3 | sev)
}
const NOFILE: Cond = Cond(0x08D7_804A);
const NOMATCHES: Cond = Cond(0x08D7_8053);

fn main() {
    let mut u = Util::new(
        include_str!("../../../../sys/SYSLIB/DCLTABLES/SEARCH.CLD"),
        215,
    );
    let items = texts(&u.values("INPUT"));
    let strings = texts(&u.values("STRINGS"));
    let exact = u.present("EXACT");
    let numbers = u.present("NUMBERS");
    let log = u.present("LOG");
    let mode = u.value("MATCH").unwrap_or_else(|| "OR".into());
    let (before, after) = if u.present("WINDOW") {
        let w: Vec<usize> = texts(&u.values("WINDOW"))
            .iter()
            .filter_map(|v| v.parse().ok())
            .collect();
        match w[..] {
            [] => (2, 2),
            [n] => (
                n.saturating_sub(1) / 2,
                n.saturating_sub(1) - n.saturating_sub(1) / 2,
            ),
            [b, a, ..] => (b, a),
        }
    } else {
        (0, 0)
    };
    let fold = |s: &str| {
        if exact {
            s.to_string()
        } else {
            s.to_uppercase()
        }
    };
    let wanted: Vec<String> = strings.iter().map(|s| fold(s)).collect();
    let matches = |line: &str| {
        let l = fold(line);
        let hits = wanted.iter().filter(|s| l.contains(s.as_str())).count();
        match &mode[..] {
            m if "AND".starts_with(m) && m.len() > 1 => hits == wanted.len(),
            m if "NAND".starts_with(m) && m.len() > 1 => hits < wanted.len(),
            m if "NOR".starts_with(m) && m.len() > 1 => hits == 0,
            _ => hits > 0,
        }
    };

    let expanded = u.expand(&items, "");
    let heading = u.present("HEADING") && (expanded.len() > 1 || expanded.iter().any(|i| i.wild()));
    let (mut searched, mut total) = (0, 0);
    for item in expanded {
        let files = match item.files {
            Ok(f) if !f.is_empty() => f,
            r => {
                let e = r.err().unwrap_or(libvms::status::FNF);
                u.msg(&[
                    (
                        u.shared(shr::OPENIN, W),
                        vec![Arg::Str(&item.spec.expanded())],
                    ),
                    (e, vec![]),
                ]);
                continue;
            }
        };
        for (path, spec) in files {
            let name = spec.expanded();
            let mut r = match Reader::open(&path) {
                Ok(r) => r,
                Err(e) => {
                    u.msg(&[
                        (u.shared(shr::OPENIN, W), vec![Arg::Str(&name)]),
                        (e, vec![]),
                    ]);
                    continue;
                }
            };
            searched += 1;
            let lines: Vec<String> = std::iter::from_fn(|| r.get())
                .map(|rec| String::from_utf8_lossy(&rec.data).into_owned())
                .collect();
            if lines.is_empty() {
                u.msg(&[(own(11, 3), vec![Arg::Str(&name)])]);
                continue;
            }
            let hits: Vec<usize> = (0..lines.len()).filter(|&i| matches(&lines[i])).collect();
            total += hits.len();
            if !hits.is_empty() {
                let mut out = std::io::stdout().lock();
                if heading {
                    let _ = write!(out, "\n{}\n{name}\n\n", "*".repeat(30));
                }
                // Windows around the matches, merged where they touch.
                let mut ranges: Vec<(usize, usize)> = Vec::new();
                for &h in &hits {
                    let (s, e) = (h.saturating_sub(before), (h + after).min(lines.len() - 1));
                    match ranges.last_mut() {
                        Some(last) if s <= last.1 + 1 => last.1 = last.1.max(e),
                        _ => ranges.push((s, e)),
                    }
                }
                for (k, (s, e)) in ranges.iter().enumerate() {
                    if k > 0 && before + after > 0 {
                        let _ = writeln!(out, "{}", "*".repeat(15));
                    }
                    for (i, line) in lines.iter().enumerate().take(*e + 1).skip(*s) {
                        if numbers {
                            let _ = write!(out, "{:6}\t", i + 1);
                        }
                        let _ = writeln!(out, "{line}");
                    }
                }
                let _ = out.flush();
            }
            if log {
                let n = Arg::Num(lines.len() as i64);
                if hits.is_empty() {
                    u.msg(&[(own(13, 1), vec![Arg::Str(&name), n])]);
                } else {
                    let es = if hits.len() == 1 { "" } else { "es" };
                    u.msg(&[(
                        own(12, 1),
                        vec![
                            Arg::Str(&name),
                            n,
                            Arg::Num(hits.len() as i64),
                            Arg::Str(es),
                        ],
                    )]);
                }
            }
        }
    }
    let status = if searched == 0 {
        NOFILE // shown by DCL
    } else if total == 0 {
        u.msg(&[(NOMATCHES, vec![])]);
        NOMATCHES
    } else {
        Cond(1)
    };
    u.exit(status);
}
