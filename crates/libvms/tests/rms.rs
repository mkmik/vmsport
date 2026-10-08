//! Record streams on host files, shared through vmsportd: what two opens
//! of one file may do together, record locks, and a relative file's
//! records through puts, updates and deletes.

use libvms::rms::{At, File, Match, Rop, fab::*};
use std::path::{Path, PathBuf};
use std::process::Command;
use vms_rms::{Design, Fab, Org, Record, Rfm, status};

#[test]
fn relative_shared() {
    let st = Command::new(env!("CARGO"))
        .args(["build", "-q", "-p", "vmsportd"])
        .status()
        .unwrap();
    assert!(st.success());
    let target = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/vmsportd");
    let run = PathBuf::from(format!("/tmp/vpt-rms{}", std::process::id()));
    // SAFETY: the only test in this program, before any thread starts.
    unsafe {
        std::env::set_var("VMSPORT_RUN", &run);
        std::env::set_var("VMSPORTD", &target);
    }
    let tmp = std::env::temp_dir().join(format!("vpt-rms-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let path = tmp.join("REL.DAT;1");
    let d = Design {
        fab: Fab {
            org: Org::Rel,
            rfm: Rfm::Var,
            mrs: 40,
            ..Fab::default()
        },
        ..Design::default()
    };
    let rec = |s: &str| Record::new(s.as_bytes().to_vec());
    let text = |r: Record| String::from_utf8(r.data).unwrap();
    {
        let mut f = File::create(&path, &d, PUT | GET, 0).unwrap();
        for s in ["first", "second", "third"] {
            f.put(&rec(s), None).unwrap();
        }
        // Not shared: nobody else gets in.
        assert_eq!(File::open(&path, GET, GET).err(), Some(status::FLK));
    }
    let all = PUT | GET | UPD | DEL;
    let mut a = File::open(&path, all, all).unwrap();
    let mut b = File::open(&path, all, all).unwrap();
    // A reader that won't share writes can't come in with writers there.
    assert_eq!(File::open(&path, GET, GET).err(), Some(status::FLK));
    let two = 2u32.to_le_bytes();
    let key = At::Key(0, &two, Match::Eq);
    assert_eq!(text(a.get(key, Rop::default()).unwrap()), "second");
    assert_eq!(b.get(key, Rop::default()).err(), Some(status::RLK));
    let nolock = Rop {
        nolock: true,
        ..Rop::default()
    };
    assert_eq!(text(b.get(key, nolock).unwrap()), "second");
    // A's update frees the record; B sees the change.
    a.update(&rec("second, changed")).unwrap();
    assert_eq!(text(b.get(key, Rop::default()).unwrap()), "second, changed");
    b.delete().unwrap();
    assert_eq!(a.get(key, Rop::default()).err(), Some(status::RNF));
    assert_eq!(a.update(&rec("x")).err(), Some(status::CUR));
    // Sequentially, past the deleted cell; keys of the wrong size.
    a.rewind(0);
    let next = |f: &mut File| f.get(At::Next, Rop::default()).map(text);
    assert_eq!(next(&mut a).unwrap(), "first");
    assert_eq!(next(&mut a).unwrap(), "third");
    assert_eq!(next(&mut a).err(), Some(status::EOF));
    assert_eq!(
        a.get(At::Key(0, b"2", Match::Eq), Rop::default()).err(),
        Some(status::KSZ)
    );
    assert_eq!(
        text(a.get(At::Key(0, &two, Match::Ge), Rop::default()).unwrap()),
        "third"
    );
    assert_eq!(
        text(a.get(At::Key(0, &two, Match::Le), Rop::default()).unwrap()),
        "first"
    );
    // At the end; and by record number.
    a.to_end().unwrap();
    a.put(&rec("fourth"), None).unwrap();
    assert_eq!(a.put(&rec("again"), Some(4)).err(), Some(status::REX));
    a.put(&rec("tenth"), Some(10)).unwrap();
    drop((a, b));
    let mut r = File::open(&path, GET, 0).unwrap();
    let got: Vec<String> = std::iter::from_fn(|| next(&mut r).ok()).collect();
    assert_eq!(got, ["first", "third", "fourth", "tenth"]);
    drop(r);

    if let Ok(c) = vmsportd::Client::connect_in(&run, Path::new("/nonexistent")) {
        let _ = c.stop();
    }
    let _ = std::fs::remove_dir_all(&run);
    let _ = std::fs::remove_dir_all(&tmp);
}
