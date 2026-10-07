//! The keypad editors on a real terminal (a pty): raw mode holds, so
//! cursor keys and the keypad arrive as keys, not text; F16 (ESC [29~)
//! is GOLD in EDT's keypad.

use std::io::{Read, Write};
use std::os::fd::FromRawFd;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

/// Runs `program args` on a pty in `dir`, typing `keys` a chunk at a time.
fn on_pty(dir: &Path, run: &Path, program: &Path, args: &[&str], keys: &[&[u8]]) {
    let (mut m, mut s) = (0, 0);
    // SAFETY: openpty fills in two descriptors we then own.
    let r = unsafe {
        libc::openpty(
            &mut m,
            &mut s,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    assert_eq!(r, 0);
    // SAFETY: fresh descriptors from openpty.
    let (mut master, slave) =
        unsafe { (std::fs::File::from_raw_fd(m), std::fs::File::from_raw_fd(s)) };
    let mut child = Command::new(program)
        .args(args)
        .current_dir(dir)
        .env("VMSPORT_RUN", run)
        .env("TERM", "vt220")
        .stdin(Stdio::from(slave.try_clone().unwrap()))
        .stdout(Stdio::from(slave.try_clone().unwrap()))
        .stderr(Stdio::from(slave))
        .spawn()
        .unwrap();
    // Drain the screen so the editor never blocks writing it.
    let mut reader = master.try_clone().unwrap();
    std::thread::spawn(move || {
        let mut b = [0u8; 4096];
        while reader.read(&mut b).is_ok_and(|n| n > 0) {}
    });
    std::thread::sleep(Duration::from_millis(1500));
    for k in keys {
        master.write_all(k).unwrap();
        std::thread::sleep(Duration::from_millis(300));
    }
    for _ in 0..50 {
        if child.try_wait().unwrap().is_some() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = child.kill();
    panic!("{} didn't finish", program.display());
}

#[test]
fn keypad_editors_on_a_terminal() {
    let st = Command::new(env!("CARGO"))
        .args(["build", "-q", "-p", "vmsportd"])
        .status()
        .unwrap();
    assert!(st.success());
    let tmp = std::env::temp_dir().join(format!("vpt-pty-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let run = PathBuf::from(format!("/tmp/vpt-pt{}", std::process::id()));
    let bin = |name: &str| Path::new(env!("CARGO_BIN_EXE_tpu")).with_file_name(name);

    // EVE: two rights, a letter, Ctrl/Z.
    std::fs::write(tmp.join("e.txt"), "abc\n").unwrap();
    on_pty(
        &tmp,
        &run,
        &bin("tpu"),
        &["e.txt"],
        &[b"\x1b[C", b"\x1b[C", b"X", b"\x1a"],
    );
    assert_eq!(
        std::fs::read_to_string(tmp.join("e.txt;2")).unwrap(),
        "abXc\n"
    );

    // EDT: CHANGE, down, right, a letter, F16 (GOLD) KP7 (COMMAND), EXIT.
    std::fs::write(tmp.join("d.txt"), "abc\ndef\n").unwrap();
    let keys: &[&[u8]] = &[
        b"CHANGE\r",
        b"\x1b[B",
        b"\x1b[C",
        b"Y",
        b"\x1b[29~",
        b"\x1bOw",
        b"EXIT\r",
    ];
    on_pty(&tmp, &run, &bin("edt"), &["d.txt"], keys);
    assert_eq!(
        std::fs::read_to_string(tmp.join("d.txt;2")).unwrap(),
        "abc\ndYef\n"
    );

    if let Ok(c) = vmsportd::Client::connect_in(&run, Path::new("/nonexistent")) {
        let _ = c.stop();
    }
    let _ = std::fs::remove_dir_all(&run);
    let _ = std::fs::remove_dir_all(&tmp);
}
