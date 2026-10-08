//! MOUNT and DISMOUNT through DCL say and do what VMS did
//! (fixtures/mount/recorded/mount.log), on an image with the volume VMS
//! had: VPTIN, with [T]DCLRMS.DIR, MOUNT.DIR and RECORD.COM.

use ods_image::{Conversion, Image, InitParams, Level};
use std::path::{Path, PathBuf};
use std::process::Command;

/// The recorded `@@ mount` section, up to MOUNT of a device that isn't
/// there (vmsport's devices are images: that one has no counterpart).
fn recorded() -> String {
    let log = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/mount/recorded/mount.log"),
    )
    .unwrap();
    let start = log.find("@@ mount\n").unwrap();
    let end = log.find("%MOUNT-F-NOSUCHDEV").unwrap();
    log[start..end].to_string()
}

/// The job table's name varies.
fn mask(s: &str) -> String {
    s.split("(LNM$JOB_")
        .enumerate()
        .map(|(i, p)| {
            if i == 0 {
                p.to_string()
            } else {
                format!("(LNM$JOB_x{}", &p[p.find(')').unwrap()..])
            }
        })
        .collect()
}

#[test]
fn mount_through_dcl() {
    for p in ["vms-dcl", "vmsportd"] {
        let st = Command::new(env!("CARGO"))
            .args(["build", "-q", "-p", p])
            .status()
            .unwrap();
        assert!(st.success());
    }
    let dcl = Path::new(env!("CARGO_BIN_EXE_mount")).with_file_name("dcl");
    let tmp = std::env::temp_dir().join(format!("vpt-mountdcl-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    {
        let p = InitParams {
            label: b"VPTIN".to_vec(),
            level: Level::Ods5,
            ..InitParams::default()
        };
        let mut img = Image::create(tmp.join("V.IMG"), 20000, &p).unwrap();
        img.mkdir("[T]").unwrap();
        img.mkdir("[T.DCLRMS]").unwrap();
        img.mkdir("[T.MOUNT]").unwrap();
        img.copy_in(
            &mut &b"$ EXIT\n"[..],
            "[T]RECORD.COM",
            Conversion::LinesToRecords,
            None,
            None,
        )
        .unwrap();
        img.flush().unwrap();
    }
    let proc_ = "$ SET NOON\n\
        $ WRITE SYS$OUTPUT \"@@ mount\"\n\
        $ MOUNT/OVERRIDE=IDENTIFICATION V.IMG DKA100:\n\
        $ SHOW SYMBOL $STATUS\n\
        $ SHOW LOGICAL DISK$VPTIN\n\
        $ MOUNT/OVERRIDE=IDENTIFICATION V.IMG DKA100:\n\
        $ SHOW SYMBOL $STATUS\n\
        $ MOUNT V.IMG DKA100: VPTIN\n\
        $ SHOW SYMBOL $STATUS\n\
        $ DISMOUNT DKA100:\n\
        $ MOUNT V.IMG DKA100: WRONG\n\
        $ SHOW SYMBOL $STATUS\n\
        $ MOUNT V.IMG DKA100: VPTIN\n\
        $ SHOW SYMBOL $STATUS\n\
        $ SHOW LOGICAL DISK$VPTIN\n\
        $ DISMOUNT DKA100:\n\
        $ MOUNT V.IMG DKA100: VPTIN MYDISK\n\
        $ SHOW SYMBOL $STATUS\n\
        $ SHOW LOGICAL MYDISK\n\
        $ DIRECTORY MYDISK:[T]\n\
        $ DISMOUNT DKA100:\n\
        $ SHOW LOGICAL MYDISK\n";
    std::fs::write(tmp.join("M.COM"), proc_).unwrap();
    let run = PathBuf::from(format!("/tmp/vpt-mt{}", std::process::id()));
    let out = Command::new(&dcl)
        .arg(tmp.join("M.COM"))
        .current_dir(&tmp)
        .env("VMSPORT_RUN", &run)
        .env("VMSPORT_JOB", "4D4F")
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    if let Ok(c) = vmsportd::Client::connect_in(&run, Path::new("/nonexistent")) {
        let _ = c.stop();
    }
    let _ = std::fs::remove_dir_all(&run);
    let _ = std::fs::remove_dir_all(&tmp);
    let got =
        String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    assert_eq!(mask(&got), mask(&recorded()));
}
