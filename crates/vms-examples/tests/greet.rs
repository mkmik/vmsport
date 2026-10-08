//! M2's acceptance test: a utility in Rust (greet) and one in C
//! (examples/greet/greet.c) get the same typed qualifiers from the same
//! CLD, run by DCL (SET COMMAND, the image named by GREET_IMAGE) and from a
//! Unix shell, and the symbol the program sets arrives in DCL.

use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    std::fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")).unwrap()
}

/// target/debug: where this test, dcl and libvms.dylib are.
fn target() -> PathBuf {
    std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn run(cmd: &mut Command) -> String {
    let out = cmd.output().unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr)
}

/// What both programs print for the first DCL session.
const EXPECTED: &str = r#"NAMES 0003FD19
COUNT 0003FD19
LOUD 000381F0
STYLE 0003FD19
STYLE.PLAIN 000381F0
STYLE.FRAME 0003FD19
STYLE.WHISPER 000381F0
SIGN 000381F0
SYMBOL 0003FD19
  value ANN 0003FD39
  value Bo b 00000001
foreign [/COUNT/SYMBOL=GREETED/STYLE=(FRAME=2) ann,"Bo b"]
** Hello, ANN! **
** Hello, ANN! **
** Hello, Bo b! **
** Hello, Bo b! **
set GREETED = 4 00000001
  GREETED = "4"
"#;

#[test]
fn rust_and_c_share_a_command_table() {
    let status = Command::new(env!("CARGO"))
        .current_dir(root())
        .args([
            "build",
            "-q",
            "-p",
            "vmsportd",
            "-p",
            "vms-dcl",
            "-p",
            "vms-c",
            "-p",
            "vms-examples",
            "-p",
            "vmsport",
        ])
        .status()
        .unwrap();
    assert!(status.success());
    let (t, ex) = (target(), root().join("examples/greet"));
    let tmp = std::env::temp_dir().join(format!("vpt-greet-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let tmp = std::fs::canonicalize(tmp).unwrap();

    // The C program: its table from vmsport cdu, linked with libvms.
    let tables = tmp.join("greet_tables.c");
    let cdu = Command::new(t.join("vmsport"))
        .arg("cdu")
        .arg(ex.join("GREET.CLD"))
        .arg("-o")
        .arg(&tables)
        .status();
    assert!(cdu.unwrap().success());
    let greet_c = tmp.join("greet_c");
    let cc = Command::new("cc")
        .args(["-Wall", "-Werror", "-I"])
        .arg(root().join("include"))
        .arg(ex.join("greet.c"))
        .arg(&tables)
        .arg("-L")
        .arg(&t)
        .arg("-lvms")
        .arg(format!("-Wl,-rpath,{}", t.display()))
        .arg("-o")
        .arg(&greet_c)
        .output()
        .unwrap();
    assert!(
        cc.status.success(),
        "{}",
        String::from_utf8_lossy(&cc.stderr)
    );
    let programs = [t.join("greet"), greet_c];

    // From DCL. A short run directory: socket paths are limited to ~100 bytes.
    let rundir = PathBuf::from(format!("/tmp/vpt-g{}", std::process::id()));
    let spec = |p: &Path| {
        let (dir, name) = (
            vmsportd::host_dir(p.parent().unwrap(), false),
            p.file_name().unwrap().to_string_lossy(),
        );
        format!("{dir}{name}{}", if name.contains('.') { "" } else { "." })
    };
    let dcl = |image: &Path, commands: &[&str]| {
        let mut c = Command::new(t.join("dcl"));
        c.env("VMSPORT_RUN", &rundir).arg("-c");
        c.arg(format!("SET COMMAND {}", spec(&ex.join("GREET.CLD"))));
        c.arg(format!("DEFINE GREET_IMAGE {}", spec(image)));
        c.args(commands);
        run(&mut c)
    };
    let sessions: [&[&str]; 4] = [
        &[
            r#"GREET/COUNT/SYMBOL=GREETED/STYLE=(FRAME=2) ann,"Bo b""#,
            "SHOW SYMBOL GREETED",
        ],
        &[
            r#"GREET/LOUD/SIGN="with love" carol"#,
            "SHOW SYMBOL $STATUS",
        ],
        &[
            "GREET/LOUD/STYLE=WHISPER x",
            "GREET/STYLE=FRAME y",
            "GREET/COUNT=Z y",
        ],
        &["GREET nobody,zoe", "SHOW SYMBOL $STATUS"],
    ];
    let mut failures = Vec::new();
    for s in sessions {
        let (rs, c) = (dcl(&programs[0], s), dcl(&programs[1], s));
        if rs != c {
            failures.push(format!("DCL {s:?}\n--- Rust\n{rs}--- C\n{c}"));
        }
    }
    let first = dcl(&programs[1], sessions[0]);
    if first != EXPECTED {
        failures.push(format!(
            "DCL {:?}\n--- expected\n{EXPECTED}--- got\n{first}",
            sessions[0]
        ));
    }
    let signalled = dcl(&programs[1], sessions[3]);
    if !signalled.contains("%SYSTEM-E-OPENIN, error opening NOBODY as input\nHello, ZOE!\n") {
        failures.push(format!("lib$signal:\n{signalled}"));
    }

    // From a Unix shell: the argv, parsed with each program's own table.
    for argv in [
        &["alice,bob", "/count=2"][..],
        &["/style=(frame=3)", "dave", "/sign=with love"],
        &["/count=x", "zed"],
    ] {
        let (rs, c) = (
            run(Command::new(&programs[0]).args(argv)),
            run(Command::new(&programs[1]).args(argv)),
        );
        if rs != c {
            failures.push(format!("shell {argv:?}\n--- Rust\n{rs}--- C\n{c}"));
        }
    }

    if let Ok(c) = vmsportd::Client::connect_in(&rundir, Path::new("/nonexistent")) {
        let _ = c.stop();
    }
    let _ = std::fs::remove_dir_all(&rundir);
    let _ = std::fs::remove_dir_all(&tmp);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
