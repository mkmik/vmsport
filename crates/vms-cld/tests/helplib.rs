//! Every verb, keyword and qualifier in sys/SYSLIB/DCLTABLES has help in
//! sys/SYSHLP/HELPLIB.HLP.

use std::collections::HashSet;
use std::path::Path;

fn sys(p: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../sys")
        .join(p)
}

/// The keys of a .HLP file, as paths: `1 SET`, `2 COMMAND`, and the
/// `/DELETE` under `3 Qualifiers` is SET COMMAND QUALIFIERS /DELETE.
fn keys(src: &str) -> HashSet<Vec<String>> {
    let mut out = HashSet::new();
    let mut path: Vec<String> = Vec::new();
    let mut level = 0;
    for line in src.lines() {
        let digits = line.chars().take_while(char::is_ascii_digit).count();
        let (depth, name) = if digits > 0 && line[digits..].starts_with(' ') {
            level = line[..digits].parse().unwrap();
            (level, &line[digits + 1..])
        } else if line.starts_with('/') {
            (level + 1, line)
        } else {
            continue;
        };
        let name = name
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_ascii_uppercase();
        path.truncate(depth - 1);
        path.push(name);
        out.insert(path.clone());
    }
    out
}

#[test]
fn every_verb_and_qualifier_has_help() {
    let mut cld = String::new();
    for f in std::fs::read_dir(sys("SYSLIB/DCLTABLES"))
        .unwrap()
        .flatten()
    {
        cld += &std::fs::read_to_string(f.path()).unwrap();
        cld.push('\n');
    }
    let t = vms_cld::compile(&cld).unwrap();
    let k = keys(&std::fs::read_to_string(sys("SYSHLP/HELPLIB.HLP")).unwrap());
    let p = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    let mut missing = Vec::new();
    let mut wanted = Vec::new();
    let mut want = |path: Vec<String>| wanted.push(path);
    for v in &t.verbs {
        // A verb is a topic, or a subtopic of one (THEN under IF).
        if !k
            .iter()
            .any(|key| key.last() == Some(&v.name) && key.len() <= 2)
        {
            missing.push(v.name.clone());
        }
        for q in &v.quals {
            want(p(&[&v.name, "QUALIFIERS", &format!("/{}", q.name)]));
            // A qualifier that switches syntax is a subtopic with its own.
            if let Some(s) = q.syntax.as_deref().and_then(|s| t.syntax(s)) {
                for q2 in s.quals.iter().filter(|q2| q2.name != q.name) {
                    want(p(&[
                        &v.name,
                        &format!("/{}", q.name),
                        "QUALIFIERS",
                        &format!("/{}", q2.name),
                    ]));
                }
            }
        }
        // SET and SHOW: each keyword a subtopic, with its syntax's qualifiers.
        for param in &v.params {
            let Some(ty) = param
                .value
                .as_ref()
                .and_then(|x| x.typ.as_deref())
                .and_then(|ty| t.typ(ty))
            else {
                continue;
            };
            for kw in &ty.keywords {
                want(p(&[&v.name, &kw.name]));
                if let Some(s) = kw.syntax.as_deref().and_then(|s| t.syntax(s)) {
                    for q in &s.quals {
                        want(p(&[
                            &v.name,
                            &kw.name,
                            "QUALIFIERS",
                            &format!("/{}", q.name),
                        ]));
                    }
                }
            }
        }
    }
    missing.extend(
        wanted
            .into_iter()
            .filter(|w| !k.contains(w))
            .map(|w| w.join(" ")),
    );
    assert!(missing.is_empty(), "no help for:\n{}", missing.join("\n"));
}
