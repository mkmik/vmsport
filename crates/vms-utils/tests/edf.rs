//! EDIT/FDL through DCL says, returns and writes what VMS's FDL editor did
//! (fixtures/edf/recorded/run1.log): the statuses of optimizing each
//! organization, the errors, and the designs written.
//!
//! What differs by system is masked: the device and directory in
//! FDL-F-OPENFDL (VMS's search list made its reason DNF, ours is FNF), the
//! date in IDENT, and the register dump VMS's EDF printed when the
//! analysis file was missing (vmsport ends with the message alone).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/edf")
}

/// `@@ name` sections of a log, from its `@RUN` (or the start).
fn sections(log: &str) -> HashMap<String, String> {
    let log = log.find("$ @RUN").map_or(log, |i| &log[i..]);
    let mut out = HashMap::new();
    let mut cur: Option<String> = None;
    for l in log.lines() {
        if let Some(h) = l.strip_prefix("@@ ") {
            cur = Some(h.trim().to_string());
            out.insert(h.trim().to_string(), String::new());
        } else if let Some(h) = &cur {
            let body = out.get_mut(h).unwrap();
            body.push_str(l);
            body.push('\n');
        }
    }
    out
}

trait PopBlank {
    fn pop_if_blank(&mut self);
}

impl PopBlank for String {
    fn pop_if_blank(&mut self) {
        if self.ends_with("\n\n") || self == "\n" {
            self.pop();
        }
    }
}

fn mask(s: &str) -> String {
    let mut out = String::new();
    let mut dump = false;
    for l in s.lines() {
        if l.starts_with("  Improperly handled condition") {
            dump = true;
            out.pop_if_blank();
        }
        // VMS's procedure listed its directory next.
        if l.starts_with("Directory ") {
            out.pop_if_blank();
            break;
        }
        if dump {
            dump = !l.trim_start().starts_with("SP  =");
            continue;
        }
        let l = match l.find("error opening ") {
            Some(i) => format!(
                "{}error opening {}",
                &l[..i],
                &l[l.rfind(']').map_or(i + 14, |j| j + 1)..]
            ),
            None if l.starts_with("-RMS-E-") || l.starts_with("-SYSTEM-W-NOSUCHFILE") => continue,
            None => l.to_string(),
        };
        let l = match (l.starts_with("IDENT"), l.find('"')) {
            (true, Some(i)) => format!("{}\"date{}", &l[..i], &l[l.find("  OpenVMS").unwrap()..]),
            _ => l,
        };
        out += &l;
        out.push('\n');
    }
    out
}

#[test]
fn edit_fdl_through_dcl() {
    for p in ["vms-dcl", "vmsportd"] {
        let st = Command::new(env!("CARGO"))
            .args(["build", "-q", "-p", p])
            .status()
            .unwrap();
        assert!(st.success());
    }
    let dcl = Path::new(env!("CARGO_BIN_EXE_edf")).with_file_name("dcl");
    let tmp = std::env::temp_dir().join(format!("vpt-edf-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    // The inputs as run1.dcl typed them, the analyses as VMS made them.
    let dcl_text = std::fs::read_to_string(fixtures().join("run1.dcl")).unwrap();
    let mut lines = dcl_text.lines();
    while let Some(l) = lines.next() {
        if let Some(name) = l.strip_prefix("CREATE ").filter(|n| n.ends_with(".FDL")) {
            let text: Vec<&str> = lines.by_ref().take_while(|l| *l != "@@CTRLZ").collect();
            std::fs::write(tmp.join(name), text.join("\n") + "\n").unwrap();
        }
    }
    let vms = sections(&std::fs::read_to_string(fixtures().join("recorded/run1.log")).unwrap());
    for a in ["SEQA.FDL", "RELA.FDL", "IDXA.FDL", "IDSA.FDL", "IDVA.FDL"] {
        std::fs::write(tmp.join(a), &vms[a]).unwrap();
    }
    // The procedure's EDIT/FDL lines, as run1.dcl ran them.
    let edits: Vec<&str> = dcl_text
        .lines()
        .filter(|l| {
            l.starts_with("$ EDIT/FDL") || l.starts_with("$ SHOW SYMBOL") || l.contains("\"@@ ")
        })
        .skip_while(|l| !l.contains("@@ statuses"))
        .take_while(|l| !l.contains("@@ end"))
        .collect();
    let outputs = [
        "IDXO.FDL",
        "IDSO.FDL",
        "IDVO.FDL",
        "IDXSB.FDL",
        "IDXG4.FDL",
        "IDXSELF.FDL",
    ];
    let mut proc_ = String::from("$ SET NOON\n");
    for l in &edits {
        proc_ += l;
        proc_.push('\n');
    }
    for o in outputs {
        proc_ += &format!("$ WRITE SYS$OUTPUT \"@@ {o}\"\n$ TYPE {o}\n");
    }
    std::fs::write(tmp.join("E.COM"), proc_).unwrap();
    let run = PathBuf::from(format!("/tmp/vpt-ed{}", std::process::id()));
    let out = Command::new(&dcl)
        .arg(tmp.join("E.COM"))
        .current_dir(&tmp)
        .env("VMSPORT_RUN", &run)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    if let Ok(c) = vmsportd::Client::connect_in(&run, Path::new("/nonexistent")) {
        let _ = c.stop();
    }
    let _ = std::fs::remove_dir_all(&run);
    // DCL's and the program's messages go to SYS$OUTPUT as well.
    let got = sections(&String::from_utf8_lossy(&out.stdout));
    // Only indexed designs are written.
    for f in [
        "SEQO.FDL", "RELO.FDL", "E1.FDL", "E5.FDL", "E9.FDL", "E10.FDL",
    ] {
        assert!(!tmp.join(format!("{f};1")).exists(), "{f} was written");
    }
    let _ = std::fs::remove_dir_all(&tmp);
    for s in ["statuses", "errors"].into_iter().chain(outputs) {
        assert_eq!(mask(&got[s]), mask(&vms[s]), "{s}");
    }
}

/// EDIT of a text file: the host's editor (here a script adding a line)
/// makes a new version named as the file's others are; nothing written
/// when nothing changed, nor with /READ_ONLY; a VAR file comes back VAR.
#[test]
fn edit_text_through_dcl() {
    for p in ["vms-dcl", "vmsportd"] {
        let st = Command::new(env!("CARGO"))
            .args(["build", "-q", "-p", p])
            .status()
            .unwrap();
        assert!(st.success());
    }
    let dcl = Path::new(env!("CARGO_BIN_EXE_edf")).with_file_name("dcl");
    let tmp = std::env::temp_dir().join(format!("vpt-edit-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(tmp.join("notes.txt"), "one\n").unwrap();
    std::fs::write(tmp.join("ADD.SH"), "echo added >> \"$1\"\n").unwrap();
    let var = vms_rms::Fab {
        rfm: vms_rms::Rfm::Var,
        ..vms_rms::Fab::default()
    };
    let mut w = libvms::files::Writer::create(&tmp.join("V.DAT;1"), var).unwrap();
    w.put(&vms_rms::Record::new(b"x".to_vec())).unwrap();
    drop(w);
    let run = PathBuf::from(format!("/tmp/vpt-et{}", std::process::id()));
    let edit = |editor: &str, cmd: &str| {
        let st = Command::new(&dcl)
            .args(["-c", cmd])
            .current_dir(&tmp)
            .env("VMSPORT_RUN", &run)
            .env("EDITOR", editor)
            .env_remove("VISUAL")
            .stdin(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(st.success(), "{cmd}");
    };
    let add = format!("sh {}", tmp.join("ADD.SH").display());
    edit(&add, "EDIT NOTES.TXT");
    edit("true", "EDIT NOTES.TXT");
    edit(&add, "EDIT/READ_ONLY NOTES.TXT");
    edit("true", "EDIT NEW.TXT");
    edit(&add, "EDIT V.DAT");
    let mut names: Vec<String> = std::fs::read_dir(&tmp)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(
        names,
        ["ADD.SH", "V.DAT;1", "V.DAT;2", "notes.txt", "notes.txt;2"]
    );
    let mut r = libvms::files::Reader::open(&tmp.join("V.DAT;2")).unwrap();
    assert_eq!(r.fab, var);
    let recs: Vec<Vec<u8>> = std::iter::from_fn(|| r.get().map(|r| r.data)).collect();
    assert_eq!(recs, [b"x".to_vec(), b"added".to_vec()]);
    assert_eq!(
        std::fs::read_to_string(tmp.join("notes.txt;2")).unwrap(),
        "one\nadded\n"
    );
    if let Ok(c) = vmsportd::Client::connect_in(&run, Path::new("/nonexistent")) {
        let _ = c.stop();
    }
    let _ = std::fs::remove_dir_all(&run);
    let _ = std::fs::remove_dir_all(&tmp);
}
