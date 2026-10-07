//! MOUNT and DISMOUNT of a Files-11 image: its files staged out with their
//! attributes, and what changed, came or went written back.

use libvms::mount;
use ods_image::{Conversion, Image, InitParams, Level, RecordAttrs};
use std::path::Path;

#[test]
fn round_trip() {
    let run = format!("/tmp/vpt-mnt-{}", std::process::id());
    // SAFETY: the only test in this program, before any thread starts.
    unsafe { std::env::set_var("VMSPORT_RUN", &run) };
    let tmp = std::env::temp_dir().join(format!("vpt-mount-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let path = tmp.join("v.img");
    let idx = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/rms/recorded/IDX.DAT"),
    )
    .unwrap();
    {
        let p = InitParams {
            label: b"TESTVOL".to_vec(),
            level: Level::Ods5,
            ..InitParams::default()
        };
        let mut img = Image::create(&path, 4000, &p).unwrap();
        img.mkdir("[T]").unwrap();
        img.copy_in(
            &mut &b"one\ntwo\n"[..],
            "[T]A.TXT",
            Conversion::LinesToRecords,
            None,
            None,
        )
        .unwrap();
        img.copy_in(
            &mut &b"bye\n"[..],
            "[T]GONE.TXT",
            Conversion::LinesToRecords,
            None,
            None,
        )
        .unwrap();
        let r = RecordAttrs {
            rtype: 0x21,
            rattrib: 2,
            rsize: 64,
            maxrec: 64,
            bktsize: 1,
            ..RecordAttrs::default()
        };
        img.copy_in(
            &mut &idx[..],
            "[T]IDX.DAT",
            Conversion::Binary,
            None,
            Some(r),
        )
        .unwrap();
        img.flush().unwrap();
    }

    assert_eq!(mount::mount(&path, "_dka100:", true).unwrap(), "TESTVOL");
    assert_eq!(
        mount::mount(&path, "DKA200", true).unwrap_err().0,
        mount::DEVMOUNT
    );
    assert_eq!(
        mount::mount(&path, "DKA100", true).unwrap_err().0,
        mount::DEVMOUNT
    );
    let root = mount::root("DKA100:").unwrap();
    let t = root.join("T");
    assert_eq!(
        std::fs::read(t.join("A.TXT;1")).unwrap(),
        b"\x03\x00one\x00\x03\x00two\x00"
    );
    assert_eq!(
        libvms::files::fab(&t.join("A.TXT;1")).to_string(),
        "org=seq rfm=var rat=cr mrs=0 lrl=3 fsz=0 bks=0"
    );
    // An indexed file comes out whole, past its end of file.
    let staged = std::fs::read(t.join("IDX.DAT;1")).unwrap();
    assert!(staged.len() >= idx.len() && staged.starts_with(&idx));
    assert!(!root.join("INDEXF.SYS;1").exists());

    std::fs::write(t.join("A.TXT;2"), b"\x05\x00three\x00").unwrap();
    libvms::sys::set_xattr(
        &t.join("A.TXT;2"),
        "vms.fab",
        b"org=seq rfm=var rat=cr mrs=0 lrl=5 fsz=0 bks=0",
    )
    .unwrap();
    std::fs::remove_file(t.join("GONE.TXT;1")).unwrap();
    std::fs::create_dir(t.join("NEW")).unwrap();
    std::fs::write(t.join("NEW/N.DAT;1"), b"raw").unwrap();
    let mut changed = staged.clone();
    changed[600] ^= 0xFF;
    std::fs::write(t.join("IDX.DAT;1"), &changed).unwrap();
    mount::dismount("DKA100").unwrap();
    assert!(mount::root("DKA100").is_none());
    assert_eq!(mount::dismount("DKA100").unwrap_err().0, mount::DEVNOTMOUNT);

    let mut img = Image::open(&path, ods_image::Mode::ReadOnly).unwrap();
    let read = |img: &mut Image, spec: &str| {
        let fid = img.lookup(spec).unwrap();
        let mut v = Vec::new();
        img.copy_out(fid, &mut v, Conversion::Binary).unwrap();
        (v, img.attributes(fid).unwrap())
    };
    assert_eq!(
        read(&mut img, "[T]A.TXT;1").0,
        b"\x03\x00one\x00\x03\x00two\x00"
    );
    let (a2, attrs) = read(&mut img, "[T]A.TXT;2");
    assert_eq!(
        (a2.as_slice(), attrs.record.rtype, attrs.record.rsize),
        (&b"\x05\x00three\x00"[..], 2, 5)
    );
    assert!(img.lookup("[T]GONE.TXT;1").is_err());
    assert_eq!(read(&mut img, "[T.NEW]N.DAT;1").0, b"raw");
    let (i, attrs) = read(&mut img, "[T]IDX.DAT;1");
    assert_eq!(
        (i, attrs.record.rtype, attrs.record.bktsize),
        (changed, 0x21, 1)
    );
    assert_eq!(img.verify().unwrap().findings, vec![]);
    drop(img);
    // /NOWRITE: nothing goes back.
    mount::mount(&path, "DKA100", false).unwrap();
    std::fs::remove_file(mount::root("DKA100").unwrap().join("T/A.TXT;1")).unwrap();
    mount::dismount("DKA100").unwrap();
    let mut img = Image::open(&path, ods_image::Mode::ReadOnly).unwrap();
    assert!(img.lookup("[T]A.TXT;1").is_ok());
    drop(img);
    let _ = std::fs::remove_dir_all(&tmp);
    let _ = std::fs::remove_dir_all(&run);
}
