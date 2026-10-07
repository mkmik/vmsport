//! HELP through DCL and the help image gives what VMS gave
//! (fixtures/help/recorded/help.log): a topic, and a prompting session
//! fed by the procedure's data lines, the unread one left to SKPDAT.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/help")
}

/// The recorded lines of `@@ label`.
fn recorded(label: &str) -> String {
    let log = std::fs::read_to_string(fixtures().join("recorded/help.log")).unwrap();
    let block = log
        .split("@@ ")
        .find(|b| b.starts_with(&format!("{label}\n")))
        .unwrap();
    block.split_once('\n').unwrap().1.to_string()
}

#[test]
fn help_through_dcl() {
    for p in ["vms-dcl", "vmsportd"] {
        let st = Command::new(env!("CARGO"))
            .args(["build", "-q", "-p", p])
            .status()
            .unwrap();
        assert!(st.success());
    }
    let dcl = Path::new(env!("CARGO_BIN_EXE_help")).with_file_name("dcl");
    let tmp = std::env::temp_dir().join(format!("vpt-help-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::copy(fixtures().join("TEST.HLP"), tmp.join("TEST.HLP")).unwrap();
    let lib = format!("{}TEST.HLP", vmsportd::host_dir(&tmp, false));
    // As recorded: data lines answer the prompts; one is left over.
    let proc_ = format!(
        "$ SET NOON\n\
         $ WRITE SYS$OUTPUT \"@@ topic\"\n\
         $ HELP/LIBRARY={lib}/NOPROMPT ALPHA\n\
         $ WRITE SYS$OUTPUT \"status \", F$FAO(\"!XL\", F$INTEGER($STATUS))\n\
         $ WRITE SYS$OUTPUT \"@@ prompting\"\n\
         $ HELP/LIBRARY={lib}/PROMPT\n\
         ALPHA\nSUBTOPIC_ONE\n?\nDEEPER\n\n\nNOSUCH\nDELTA\nONE\n\n\n\n\
         $ WRITE SYS$OUTPUT \"status \", F$FAO(\"!XL\", F$INTEGER($STATUS))\n"
    );
    std::fs::write(tmp.join("H.COM"), proc_).unwrap();
    let run = PathBuf::from(format!("/tmp/vpt-h{}", std::process::id()));
    let out = Command::new(&dcl)
        .arg(tmp.join("H.COM"))
        .current_dir(&tmp)
        .env("VMSPORT_RUN", &run)
        .env("COLUMNS", "132")
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    if let Ok(c) = vmsportd::Client::connect_in(&run, Path::new("/nonexistent")) {
        let _ = c.stop();
    }
    let _ = std::fs::remove_dir_all(&run);
    let _ = std::fs::remove_dir_all(&tmp);
    let got = String::from_utf8_lossy(&out.stdout).to_string();
    let want = format!(
        "@@ topic\n{}@@ prompting\n{}",
        recorded("topic"),
        recorded("prompting")
    );
    assert_eq!(got, want);
}
