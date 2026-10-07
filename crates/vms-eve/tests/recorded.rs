//! The sessions recorded on OpenVMS 8.4 (fixtures/eve): the same keys
//! here leave the same screen, cursor and status. Files are fixtures/eve/
//! files, as version 1 of SYS$SYSDEVICE:[EVE] like there.
//!
//! EVE_SESSION=name runs just that one and prints both screens.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use vms_eve::{Done, Editor, Host, Key, Start};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/eve")
}

const DIR: &str = "SYS$SYSDEVICE:[EVE]";

/// Each file's versions.
type Dir = HashMap<String, Vec<Vec<String>>>;

/// The directory as the session found it, shared with the test.
pub struct Files {
    pub files: Rc<RefCell<Dir>>,
}

impl Host for Files {
    fn read(&mut self, spec: &str) -> Result<(Vec<String>, String), String> {
        let name = spec.to_uppercase();
        match self.files.borrow().get(&name) {
            Some(v) if !v.is_empty() => Ok((
                v.last().unwrap().clone(),
                format!("{DIR}{name};{}", v.len()),
            )),
            _ => Err(format!("{DIR}{name};")),
        }
    }

    fn write(&mut self, spec: &str, lines: &[String]) -> Result<(String, bool), String> {
        let name = spec
            .trim_start_matches(DIR)
            .split(';')
            .next()
            .unwrap()
            .to_uppercase();
        let mut files = self.files.borrow_mut();
        let v = files.entry(name.clone()).or_default();
        v.push(lines.to_vec());
        // L.TXT is DCL's (OPEN/WRITE): VFC, which EVE doesn't write.
        Ok((format!("{DIR}{name};{}", v.len()), name == "L.TXT"))
    }

    fn dcl(&mut self, _command: &str) -> Vec<String> {
        vec![format!("  {DIR}")]
    }

    fn spawn(&mut self, _command: &str) {}
}

pub fn files() -> Files {
    let mut files = HashMap::new();
    for e in std::fs::read_dir(fixtures().join("files")).unwrap() {
        let p = e.unwrap().path();
        let text = std::fs::read_to_string(&p).unwrap();
        let name = p.file_name().unwrap().to_string_lossy().to_uppercase();
        files.insert(name, vec![text.lines().map(str::to_string).collect()]);
    }
    Files {
        files: Rc::new(RefCell::new(files)),
    }
}

/// Python escapes as the bytes typed.
fn unescape(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 1 < b.len() {
            match b[i + 1] {
                b'x' => {
                    out.push(u8::from_str_radix(&s[i + 2..i + 4], 16).unwrap());
                    i += 4;
                    continue;
                }
                b'r' => out.push(b'\r'),
                b't' => out.push(b'\t'),
                b'n' => out.push(b'\n'),
                c => out.push(c),
            }
            i += 2;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

/// What EDIT's qualifiers ask.
fn start(cmd: &str) -> Start {
    let (quals, file) = cmd.split_once(' ').unwrap();
    let mut s = Start {
        file: file.to_string(),
        ..Start::default()
    };
    for q in quals.split('/').skip(1) {
        let q = q.to_uppercase();
        if q == "READ_ONLY" {
            (s.write, s.modify) = (false, false);
        } else if q == "NOCREATE" {
            s.create = false;
        } else if let Some(p) = q.strip_prefix("START_POSITION=") {
            let n: Vec<usize> = p
                .trim_matches(['(', ')'])
                .split(',')
                .map(|n| n.parse().unwrap())
                .collect();
            s.start_position = Some((n[0], n[1]));
        }
    }
    s
}

/// A screen as screens.py writes it: the rows with text, and the cursor.
fn dump(e: &Editor) -> String {
    let g = e.grid();
    let mut out = String::new();
    for r in 0..g.rows() {
        let t = g.text(r);
        if !t.trim().is_empty() {
            out.push_str(&format!("{:2}|{t}\n", r + 1));
        }
    }
    out + &format!("cursor {},{}\n", g.cursor.0 + 1, g.cursor.1 + 1)
}

#[test]
fn sessions_as_vms_left_them() {
    let recorded = std::fs::read_to_string(fixtures().join("recorded/screens.txt")).unwrap();
    let mut screens = HashMap::new();
    for block in recorded.split("@@ ").skip(1) {
        let (name, rest) = block.split_once('\n').unwrap();
        let (status, screen) = rest.split_once('\n').unwrap();
        screens.insert(
            name.to_string(),
            (
                status.trim_start_matches("status ").to_string(),
                screen.to_string(),
            ),
        );
    }
    let only = std::env::var("EVE_SESSION").ok();
    let sessions = std::fs::read_to_string(fixtures().join("sessions.txt")).unwrap();
    let mut failed = Vec::new();
    let mut n = 0;
    for line in sessions
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
    {
        let f: Vec<&str> = line.split('\t').collect();
        let (name, cmd, keys) = (f[0], f[1], f[2]);
        if only.as_deref().is_some_and(|o| o != name) {
            continue;
        }
        let Some((status, want)) = screens.get(name) else {
            continue;
        };
        n += 1;
        let mut e = Editor::new(24, 80, Box::new(files()));
        e.start(&start(cmd));
        for k in libvms::term::Keys::new(&unescape(keys)[..]) {
            if e.done.is_some() {
                break;
            }
            e.key(k);
        }
        // record.py's Ctrl/Z for a session left at a question.
        for _ in 0..4 {
            if e.done.is_none() {
                e.key(Key::Ctrl('Z'));
            }
        }
        let got = dump(&e);
        let got_status = match e.done {
            Some(Done::Exit) => "%X13F2AF01",
            Some(Done::Quit) => "%X13F2AF59",
            None => "running",
        };
        if &got != want || got_status != status {
            failed.push(name.to_string());
            if only.is_some() || std::env::var("EVE_SHOW").is_ok() {
                println!("== {name}\n-- VMS ({status})\n{want}-- vmsport ({got_status})\n{got}");
            }
        }
    }
    assert!(n > 0);
    assert!(
        failed.is_empty(),
        "{} of {n} differ: {}",
        failed.len(),
        failed.join(" ")
    );
}

/// The NODISPLAY cases (fixtures/eve/batch.txt): what EVE said, its
/// status, and the files it left, as VMS.
#[test]
fn batch_as_vms_did_it() {
    let log = std::fs::read_to_string(fixtures().join("recorded/batch.log")).unwrap();
    let cases = std::fs::read_to_string(fixtures().join("batch.txt")).unwrap();
    let mut failed = Vec::new();
    for (i, line) in cases
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
        .enumerate()
    {
        let f: Vec<&str> = line.split('\t').collect();
        let (name, cmd, init, show) = (f[0], f[1], f[2], f[3]);
        let block = log
            .split(&format!("@@ {name}\n"))
            .nth(1)
            .unwrap()
            .split("\n@@ ")
            .next()
            .unwrap();
        let (said, rest) = block.split_once("  $STATUS == ").unwrap();
        let (status, after) = rest.split_once('\n').unwrap_or((rest, ""));
        let cmd = cmd.replace("I.EVE", &format!("I{i}.EVE"));
        let (quals, file) = cmd.split_once(' ').unwrap();
        let mut s = vms_eve::Start {
            file: file.to_string(),
            ..vms_eve::Start::default()
        };
        let host = files();
        for q in quals.split('/').skip(1) {
            match q.split_once('=') {
                Some(("INITIALIZATION", f)) if f.starts_with('I') => {
                    s.init = Some(Ok((
                        init.split(" | ").map(str::to_string).collect(),
                        format!("{DIR}{f};1"),
                    )))
                }
                Some(("INITIALIZATION", f)) => s.init = Some(Err(f.to_string())),
                Some(("OUTPUT", f)) => s.output = Some(f.to_string()),
                Some(("COMMAND", _)) => {
                    // vmsport's tpu says this itself (crates/vms-utils/src/bin/tpu.rs).
                    continue;
                }
                None if q == "READ_ONLY" => (s.write, s.modify) = (false, false),
                None if q == "NOCREATE" => s.create = false,
                _ => {}
            }
        }
        if !cmd.contains("/INITIALIZATION") && init != "-" {
            s.init = Some(Ok((
                init.split(" | ").map(str::to_string).collect(),
                format!("{DIR}I{i}.EVE;1"),
            )));
        }
        if cmd.contains("/COMMAND") {
            continue;
        }
        host.files.borrow_mut().remove("NEW.TXT");
        let dir = host.files.clone();
        let mut e = Editor::new(24, 80, Box::new(host));
        e.nodisplay = Some(Vec::new());
        e.start(&s);
        let got = e
            .nodisplay
            .take()
            .unwrap()
            .iter()
            .map(|m| format!("{m}\n"))
            .collect::<String>();
        let got_status = format!(
            "\"{}\"",
            match e.done {
                Some(Done::Quit) => "%X13F2AF59",
                _ => "%X13F2AF01",
            }
        );
        // The newest version of the file TYPE showed, as VMS left it.
        let files = dir.borrow();
        let typed = after
            .lines()
            .filter(|l| !l.starts_with(DIR) && l.trim() != "")
            .collect::<Vec<_>>();
        let wanted_file = (show != "-").then(|| {
            let name = show.split(';').next().unwrap();
            files
                .get(name)
                .and_then(|v| v.last())
                .cloned()
                .unwrap_or_default()
        });
        let file_ok = match wanted_file {
            Some(lines) => {
                let n = lines.iter().filter(|l| !l.is_empty()).count();
                typed
                    .iter()
                    .take(n)
                    .map(|l| l.to_string())
                    .eq(lines.into_iter().filter(|l| !l.is_empty()))
            }
            None => true,
        };
        if got != said || got_status != status || !file_ok {
            println!(
                "== {name}\n-- VMS {status}\n{said}{}\n-- vmsport {got_status}\n{got}{:?}",
                typed.join("\n"),
                files.get(show.split(';').next().unwrap())
            );
            failed.push(name);
        }
    }
    assert!(failed.is_empty(), "differ: {}", failed.join(" "));
}
