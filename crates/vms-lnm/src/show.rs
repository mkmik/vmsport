//! What SHOW LOGICAL prints and F$TRNLNM returns.

use crate::{Found, Logical, MAX_DEPTH, Mode, Names, Shared};
use std::fmt::Write;

/// `[super,confine,no_alias,table]`
fn name_attrs(l: &Logical) -> String {
    let mut a = vec![l.mode.short()];
    a.extend(
        [
            (l.confine, "confine"),
            (l.no_alias, "no_alias"),
            (l.table, "table"),
        ]
        .iter()
        .filter(|x| x.0)
        .map(|x| x.1),
    );
    format!(" [{}]", a.join(","))
}

/// The values to show: a table shows as one terminal "".
fn values(l: &Logical) -> Vec<(String, bool, bool)> {
    if l.table {
        return vec![(String::new(), false, true)];
    }
    l.equivs
        .iter()
        .map(|e| (e.text.clone(), e.concealed, e.terminal))
        .collect()
}

fn value_attrs(concealed: bool, terminal: bool) -> String {
    let a: Vec<&str> = [(concealed, "concealed"), (terminal, "terminal")]
        .iter()
        .filter(|x| x.0)
        .map(|x| x.1)
        .collect();
    if a.is_empty() {
        String::new()
    } else {
        format!(" [{}]", a.join(","))
    }
}

/// One logical name: `"NAME" [attrs] = "value" [attrs] (TABLE)` and the
/// rest of a search list, each value on its own line after a tab.
fn entry(out: &mut String, l: &Logical, table: Option<&str>, full: bool) {
    write!(out, "\"{}\"", l.name).unwrap();
    if full {
        out.push_str(&name_attrs(l));
    }
    for (i, (v, c, t)) in values(l).iter().enumerate() {
        if i > 0 {
            out.push_str("\n\t");
        } else {
            out.push(' ');
        }
        write!(out, "= \"{v}\"").unwrap();
        if full {
            out.push_str(&value_attrs(*c, *t));
        }
        if i == 0
            && let Some(t) = table
        {
            write!(out, " ({t})").unwrap();
        }
    }
    out.push('\n');
}

/// `*` and `%` wildcards.
fn matches(pat: &[u8], s: &[u8]) -> bool {
    match (pat.first(), s.first()) {
        (None, None) => true,
        (Some(b'*'), _) => matches(&pat[1..], s) || (!s.is_empty() && matches(pat, &s[1..])),
        (Some(b'%'), Some(_)) => matches(&pat[1..], &s[1..]),
        (Some(p), Some(c)) if p == c => matches(&pat[1..], &s[1..]),
        _ => false,
    }
}

/// SHOW LOGICAL [name] [/TABLE=table] [/FULL]. With a name, every table in
/// `table`'s search list that has it, each followed by what its values
/// translate to (`1  "..."`, `2  "..."`). Without one, or with wildcards,
/// a listing per table.
pub fn show<S: Shared>(names: &Names<S>, name: Option<&str>, table: &str, full: bool) -> String {
    let mut out = String::new();
    let tables = names.tables(table, Mode::User);
    match name.filter(|n| !n.contains(['*', '%'])) {
        Some(n) => {
            for t in &tables {
                if let Some(l) = names
                    .get(t)
                    .and_then(|tab| tab.find(n, Mode::User, false).cloned())
                {
                    out.push_str("   ");
                    entry(&mut out, &l, Some(t), full);
                    iterate(names, &l, table, full, 1, &mut out);
                }
            }
            if out.is_empty() {
                writeln!(out, "%SHOW-S-NOTRAN, no translation for logical name {n}").unwrap();
            }
        }
        None => {
            let pat = name.unwrap_or("*").as_bytes();
            for t in &tables {
                write!(out, "\n({t})\n").unwrap();
                let Some(tab) = names.get(t) else { continue };
                let mut ls: Vec<&Logical> = tab
                    .logicals
                    .iter()
                    .filter(|l| matches(pat, l.name.as_bytes()))
                    .collect();
                ls.sort_by(|a, b| a.name.cmp(&b.name));
                if !ls.is_empty() {
                    out.push('\n');
                }
                for l in ls {
                    out.push_str("  ");
                    entry(&mut out, l, None, full);
                }
            }
        }
    }
    out
}

/// The translations of `l`'s values that aren't terminal, depth first.
fn iterate<S: Shared>(
    names: &Names<S>,
    l: &Logical,
    table: &str,
    full: bool,
    level: usize,
    out: &mut String,
) {
    if level > MAX_DEPTH || l.table {
        return;
    }
    for e in l.equivs.iter().filter(|e| !e.terminal) {
        if let Some(Found { table: t, logical }) =
            names.translate(&e.text, table, Mode::User, false)
        {
            write!(out, "{level:<3}").unwrap();
            entry(out, &logical, Some(&t), full);
            iterate(names, &logical, table, full, level + 1, out);
        }
    }
}

/// An F$TRNLNM item for what was found (`None`: nothing was, and every
/// item is ""). Unknown items give `None`.
pub fn item(found: Option<&Found>, index: usize, item: &str) -> Option<String> {
    let bool = |b: bool| if b { "TRUE" } else { "FALSE" }.to_string();
    let item = item.to_ascii_uppercase();
    let known = [
        "VALUE",
        "LENGTH",
        "MAX_INDEX",
        "TABLE",
        "TABLE_NAME",
        "TERMINAL",
        "CONCEALED",
        "CONFINE",
        "NO_ALIAS",
        "CRELOG",
        "ACCESS_MODE",
    ];
    if !known.contains(&item.as_str()) {
        return None;
    }
    let Some(Found { table, logical: l }) = found else {
        return Some(String::new());
    };
    let e = l.equivs.get(index);
    Some(match item.as_str() {
        "VALUE" => e.map(|e| e.text.clone()).unwrap_or_default(),
        "LENGTH" => e.map(|e| e.text.len().to_string()).unwrap_or_default(),
        "MAX_INDEX" => l.equivs.len().saturating_sub(1).to_string(),
        "TABLE" => bool(l.table),
        "TABLE_NAME" => table.clone(),
        "TERMINAL" => e.map(|e| bool(e.terminal)).unwrap_or_default(),
        "CONCEALED" => e.map(|e| bool(e.concealed)).unwrap_or_default(),
        "CONFINE" => bool(l.confine),
        "NO_ALIAS" => bool(l.no_alias),
        "CRELOG" => bool(false),
        _ => l.mode.long().to_string(),
    })
}
