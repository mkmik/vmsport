//! M3's acceptance: indexed files made on VMS (fixtures/accept) go into a
//! Files-11 image, vmsport MOUNTs it and runs UPDATE.COM on them through
//! DCL (puts that split buckets, updates of alternate keys, deletes), and
//! DISMOUNTs it. That image went back to VMS (fixtures/acceptback), whose
//! ANALYZE/RMS_FILE/CHECK found it sound and whose reads match ours.
//!
//! The image is rebuilt here each time; the files in it must be the ones
//! VMS checked, byte for byte. VMSPORT_ACCEPT_WRITE=1 writes it to
//! fixtures/acceptback/VOLUME.IMG.gz instead, for recording.

use ods_image::{Conversion, Image, InitParams, Level, RecordAttrs};
use std::path::{Path, PathBuf};
use std::process::Command;

fn fixtures(p: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(p)
}

/// A file's record attributes as fixtures/accept recorded them.
fn attrs(name: &str) -> RecordAttrs {
    let m = std::fs::read_to_string(fixtures("accept/recorded/ods-manifest.json")).unwrap();
    let e = m
        .split("\"name\": \"")
        .find(|e| e.starts_with(&format!("{name}\"")))
        .unwrap();
    let num = |k: &str| -> u64 {
        let v = &e[e.find(&format!("\"{k}\": ")).unwrap() + k.len() + 4..];
        v[..v.find([',', '\n']).unwrap()].trim().parse().unwrap()
    };
    RecordAttrs {
        rtype: num("rtype") as u8,
        rattrib: num("rattrib") as u8,
        rsize: num("rsize") as u16,
        bktsize: num("bktsize") as u8,
        vfcsize: num("vfcsize") as u8,
        maxrec: num("maxrec") as u16,
        defext: num("defext") as u16,
        ..RecordAttrs::default()
    }
}

/// A relative or indexed file's blocks, all of its allocation.
fn blocks(img: &mut Image, spec: &str) -> Vec<u8> {
    let fid = img.lookup(spec).unwrap();
    let hiblk = img.attributes(fid).unwrap().record.hiblk as usize;
    let mut v = Vec::new();
    for (lbn, n) in img.extents(fid).unwrap() {
        for b in lbn..lbn + n {
            v.extend(img.read_block(b).unwrap());
        }
    }
    v.truncate(hiblk * 512);
    v
}

/// The `@@` sections of a log that READS.COM wrote, each up to the status
/// it ends with.
fn reads(log: &str) -> Vec<String> {
    log.split("@@ ")
        .skip(1)
        .map(|s| {
            let end = s.find("  $STATUS").map_or(s.len(), |i| {
                i + s[i..].find('\n').map_or(s.len() - i, |n| n + 1)
            });
            s[..end].to_string()
        })
        .collect()
}

#[test]

fn indexed_files_from_vms_through_an_image() {
    for p in ["vms-dcl", "vmsportd"] {
        let st = Command::new(env!("CARGO"))
            .args(["build", "-q", "-p", p])
            .status()
            .unwrap();
        assert!(st.success());
    }
    let dcl = Path::new(env!("CARGO_BIN_EXE_mount")).with_file_name("dcl");
    let tmp = std::env::temp_dir().join(format!("vpt-accept-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let image = tmp.join("VOLUME.IMG");
    {
        let p = InitParams {
            label: b"VPTIN".to_vec(),
            level: Level::Ods5,
            ..InitParams::default()
        };
        let mut img = Image::create(&image, 20000, &p).unwrap();
        img.mkdir("[T]").unwrap();
        img.mkdir("[T.ACCEPTBACK]").unwrap();
        for f in ["ORDERS.DAT", "PARTS.DAT"] {
            let data = std::fs::read(fixtures("accept/recorded").join(f)).unwrap();
            let spec = format!("[T.ACCEPTBACK]{f};1");
            img.copy_in(
                &mut &data[..],
                &spec,
                Conversion::Binary,
                Some(data.len() as u64),
                Some(attrs(f)),
            )
            .unwrap();
        }
        img.flush().unwrap();
    }
    for f in ["RECORDS.COM", "UPDATE.COM", "READS.COM"] {
        std::fs::copy(fixtures("accept").join(f), tmp.join(f)).unwrap();
    }
    let on = "DKA100:[T.ACCEPTBACK]";
    let proc_ = format!(
        "$ SET NOON\n\
         $ MOUNT VOLUME.IMG DKA100: VPTIN\n\
         $ @UPDATE {on}ORDERS.DAT {on}PARTS.DAT\n\
         $ @READS {on}ORDERS.DAT 3\n\
         $ @READS {on}PARTS.DAT 2\n\
         $ DISMOUNT DKA100:\n"
    );
    std::fs::write(tmp.join("A.COM"), proc_).unwrap();
    let run = PathBuf::from(format!("/tmp/vpt-ac{}", std::process::id()));
    let out = Command::new(&dcl)
        .arg(tmp.join("A.COM"))
        .current_dir(&tmp)
        .env("VMSPORT_RUN", &run)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    if let Ok(c) = vmsportd::Client::connect_in(&run, Path::new("/nonexistent")) {
        let _ = c.stop();
    }
    let _ = std::fs::remove_dir_all(&run);
    let got = String::from_utf8_lossy(&out.stdout).to_string();
    let said = got.clone() + &String::from_utf8_lossy(&out.stderr);
    assert!(!said.contains("-E-") && !said.contains("-F-"), "{said}");

    // What we read after our updates is what VMS read after its own.
    let vms = std::fs::read_to_string(fixtures("accept/recorded/make.log")).unwrap();
    let mask = |s: &str| {
        s.replace(&format!("{on}ORDERS.DAT"), "ORDERS")
            .replace(&format!("{on}PARTS.DAT"), "PARTS")
    };
    let vms_mask = |s: &str| {
        s.replace("ORDERSU.DAT", "ORDERS")
            .replace("PARTSU.DAT", "PARTS")
    };
    let ours: Vec<String> = reads(&got).iter().map(|s| mask(s)).collect();
    let theirs: Vec<String> = reads(&vms).iter().map(|s| vms_mask(s)).collect();
    assert_eq!(ours.len(), 5);
    for (o, t) in ours.iter().zip(&theirs) {
        assert_eq!(o, t);
    }
    // VMS read the same from the image we sent it.
    if let Ok(back) = std::fs::read_to_string(fixtures("acceptback/recorded/check.log")) {
        let theirs: Vec<String> = reads(&back)
            .iter()
            .filter(|s| s.contains(" key "))
            .map(|s| mask(s))
            .collect();
        assert_eq!(ours, theirs);
    }

    let mut img = Image::open(&image, ods_image::Mode::ReadOnly).unwrap();
    assert_eq!(img.verify().unwrap().findings, vec![]);
    let ours: Vec<Vec<u8>> = ["ORDERS.DAT;1", "PARTS.DAT;1"]
        .iter()
        .map(|f| blocks(&mut img, &format!("[T.ACCEPTBACK]{f}")))
        .collect();
    drop(img);
    let back = fixtures("acceptback/VOLUME.IMG.gz");
    if std::env::var_os("VMSPORT_ACCEPT_WRITE").is_some() {
        let gz = Command::new("gzip")
            .args(["-9", "-n", "-c"])
            .arg(&image)
            .output()
            .unwrap();
        std::fs::write(&back, gz.stdout).unwrap();
    } else {
        // The files VMS checked are the ones we make now.
        let raw = Command::new("gzip")
            .args(["-d", "-c"])
            .arg(&back)
            .output()
            .unwrap();
        let sent = tmp.join("SENT.IMG");
        std::fs::write(&sent, raw.stdout).unwrap();
        let mut img = Image::open(&sent, ods_image::Mode::ReadOnly).unwrap();
        for (f, o) in ["ORDERS.DAT;1", "PARTS.DAT;1"].iter().zip(&ours) {
            assert!(
                blocks(&mut img, &format!("[T.ACCEPTBACK]{f}")) == *o,
                "{f} differs from what VMS checked"
            );
        }
        // And VMS found them sound.
        let log = std::fs::read_to_string(fixtures("acceptback/recorded/check.log")).unwrap();
        for f in ["ORDERS", "PARTS"] {
            let chk =
                std::fs::read_to_string(fixtures(&format!("acceptback/recorded/{f}.CHK"))).unwrap();
            assert!(
                chk.contains("The analysis uncovered NO errors."),
                "{f}.CHK:\n{chk}"
            );
        }
        assert!(!log.contains("-F-") && !log.contains("-E-"), "{log}");
    }
    let _ = std::fs::remove_dir_all(&tmp);
}
