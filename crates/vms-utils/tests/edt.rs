//! EDIT/EDT in line mode, from a procedure's data lines, says and writes
//! what VMS's EDT did (fixtures/edt: each runN.dcl was typed at an
//! installed system's console; recorded/runN.log is what it printed).
//!
//! The console expanded tabs, wrapped at 80 columns and turned form feeds
//! into blank lines; vmsport's output gets the same treatment here.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/edt")
}

/// The files a .dcl CREATEs (name, text), the procedure T.COM among them,
/// and the directory it works in.
fn files_of(dcl: &str) -> (Vec<(String, String)>, String) {
    let lines: Vec<&str> = dcl.lines().collect();
    let mut out = Vec::new();
    let mut dir = String::new();
    let mut i = 0;
    while i < lines.len() {
        if let Some(d) = lines[i].strip_prefix("SET DEFAULT ") {
            dir = d.to_string();
        }
        if let Some(name) = lines[i]
            .strip_prefix("CREATE ")
            .filter(|n| !n.starts_with('/'))
        {
            let end = lines[i..].iter().position(|l| *l == "@@CTRLZ").unwrap() + i;
            let mut text = lines[i + 1..end].join("\n");
            text.push('\n');
            out.push((name.to_string(), text));
            i = end;
        }
        i += 1;
    }
    (out, dir)
}

/// As the console showed it: tabs expanded, lines wrapped at 80, a form
/// feed a blank line; and runs of blank lines as one.
fn console(s: &str, host_dir: &str, vms_dir: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for l in s.lines() {
        let l = l.replace(host_dir, vms_dir);
        let (ff, l) = match l.strip_prefix('\x0c') {
            Some(rest) => (true, rest.to_string()),
            None => (false, l),
        };
        if ff {
            out.push(String::new());
        }
        let mut col = String::new();
        for ch in l.chars() {
            if ch == '\t' {
                let n = 8 - col.chars().count() % 8;
                col.push_str(&" ".repeat(n));
            } else {
                col.push(ch);
            }
        }
        let chars: Vec<char> = col.chars().collect();
        if chars.is_empty() {
            out.push(String::new());
        }
        for chunk in chars.chunks(80) {
            out.push(chunk.iter().collect());
        }
    }
    squeeze(out)
}

fn squeeze(lines: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for l in lines {
        if l.is_empty() && out.last().is_some_and(|p| p.is_empty()) {
            continue;
        }
        out.push(l);
    }
    while out
        .last()
        .is_some_and(|l| l.trim_end().is_empty() || l.trim_end() == "$")
    {
        out.pop();
    }
    out
}

/// What VMS printed from `$ @T` on; its version banner masked.
fn recorded(log: &str) -> Vec<String> {
    let lines: Vec<String> = log
        .lines()
        .skip_while(|l| *l != "$ @T")
        .skip(1)
        .map(|l| version(l.to_string()))
        .collect();
    squeeze(lines)
}

fn version(l: String) -> String {
    match l.starts_with("V3.12-04") {
        true => "V3.12-04 <version>".into(),
        false => l,
    }
}

#[test]
fn line_mode_as_vms() {
    for p in ["vms-dcl", "vmsportd"] {
        let st = Command::new(env!("CARGO"))
            .args(["build", "-q", "-p", p])
            .status()
            .unwrap();
        assert!(st.success());
    }
    let dcl = Path::new(env!("CARGO_BIN_EXE_edt")).with_file_name("dcl");
    let mut failures = Vec::new();
    for run in ["run2", "run3"] {
        let Ok(log) = std::fs::read_to_string(fixtures().join(format!("recorded/{run}.log")))
        else {
            continue;
        };
        let (files, vms_dir) =
            files_of(&std::fs::read_to_string(fixtures().join(format!("{run}.dcl"))).unwrap());
        let tmp = std::env::temp_dir().join(format!("vpt-edt-{run}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        for (name, text) in &files {
            std::fs::write(tmp.join(name), text).unwrap();
        }
        let out_path =
            std::env::temp_dir().join(format!("vpt-edt-{run}-{}.log", std::process::id()));
        let out = std::fs::File::create(&out_path).unwrap();
        let rundir = PathBuf::from(format!("/tmp/vpt-et{}{run}", std::process::id()));
        let st = Command::new(&dcl)
            .arg("T.COM")
            .current_dir(&tmp)
            .env("VMSPORT_RUN", &rundir)
            .stdin(Stdio::null())
            .stdout(out.try_clone().unwrap())
            .stderr(out)
            .status()
            .unwrap();
        assert!(st.success() || st.code().is_some());
        if let Ok(c) = vmsportd::Client::connect_in(&rundir, Path::new("/nonexistent")) {
            let _ = c.stop();
        }
        let _ = std::fs::remove_dir_all(&rundir);
        let ours = std::fs::read_to_string(&out_path).unwrap();
        let host_dir = vmsportd::host_dir(&std::fs::canonicalize(&tmp).unwrap(), false);
        let got: Vec<String> = console(&ours, &host_dir, &vms_dir)
            .into_iter()
            .map(version)
            .collect();
        let want = recorded(&log);
        if got != want {
            let n = got.iter().zip(&want).take_while(|(a, b)| a == b).count();
            failures.push(format!(
                "{run}: first difference at line {n}:\n  VMS     {:?}\n  vmsport {:?}",
                want.get(n),
                got.get(n)
            ));
        }
        let _ = std::fs::remove_dir_all(&tmp);
        let _ = std::fs::remove_file(&out_path);
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
