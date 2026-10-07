//! vmsportd with real sockets: started on first use, shared between
//! clients, raced by concurrent ones.

use std::path::{Path, PathBuf};
use vms_lnm::{Logical, Mode, Names, Shared};
use vmsportd::Client;

/// A fresh run directory; short, as socket paths are limited.
fn dir(name: &str) -> PathBuf {
    let d = PathBuf::from(format!("/tmp/vpt-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn connect(d: &Path) -> Client {
    Client::connect_in(d, Path::new(env!("CARGO_BIN_EXE_vmsportd"))).unwrap()
}

#[test]
fn shared_tables() {
    let d = dir("shared");
    let mut a = connect(&d); // starts the daemon
    let (job, group) = a.job(0x1234).unwrap();
    assert_eq!(
        (job.as_str(), group.as_str()),
        ("LNM$JOB_00001234", group.as_str())
    );
    let mut na = Names::new(a, &job, &group);
    na.define("LNM$JOB", Logical::new("VPT_J", &["job value"]))
        .unwrap();
    na.define("LNM$PROCESS", Logical::new("VPT_P", &["private"]))
        .unwrap();

    // Another process of the same job.
    let mut b = connect(&d);
    assert_eq!(b.job(0x1234).unwrap(), (job.clone(), group.clone()));
    let nb = Names::new(b, &job, &group);
    let t = |n: &Names<Client>, name: &str| n.translate(name, "LNM$FILE_DEV", Mode::User, false);
    assert_eq!(t(&nb, "VPT_J").unwrap().logical.equivs[0].text, "job value");
    assert!(t(&nb, "VPT_P").is_none());

    // What the daemon and the job start with.
    let home = vmsportd::host_dir(Path::new(&std::env::var("HOME").unwrap()), false);
    assert_eq!(t(&nb, "SYS$LOGIN").unwrap().logical.equivs[0].text, home);
    assert_eq!(t(&nb, "SYS$SYSTEM").unwrap().table, vms_lnm::SYSTEM_TABLE);
    let r = nb
        .resolve(&"SYS$SYSTEM:DIRECTORY.EXE".parse().unwrap())
        .unwrap()
        .remove(0);
    assert_eq!(r.display.to_string(), "SYS$SYSROOT:[SYSEXE]DIRECTORY.EXE");
    assert_eq!(r.physical.device.as_deref(), Some("HOST"));
    assert!(
        r.path()
            .ends_with(&["sys".to_string(), "SYSEXE".to_string()])
    );

    // SHOW LOGICAL through the daemon.
    let s = vms_lnm::show(&nb, Some("VPT_J"), "LNM$DCL_LOGICAL", false);
    assert_eq!(s, "   \"VPT_J\" = \"job value\" (LNM$JOB_00001234)\n");
    assert_eq!(
        na.deassign("LNM$JOB", "VPT_J", Mode::Supervisor),
        Ok(vms_lnm::SS_NORMAL)
    );
    assert!(t(&nb, "VPT_J").is_none());
    nb.shared.stop().unwrap();
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn concurrent_clients_start_one_daemon() {
    let d = dir("race");
    let threads: Vec<_> = (0..8)
        .map(|i| {
            let d = d.clone();
            std::thread::spawn(move || {
                let mut c = connect(&d);
                c.define(
                    vms_lnm::SYSTEM_TABLE,
                    Logical::new(format!("VPT_{i}"), &["x"]),
                )
                .unwrap();
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    let c = connect(&d);
    let sys = c.table(vms_lnm::SYSTEM_TABLE).unwrap();
    for i in 0..8 {
        assert!(
            sys.logicals.iter().any(|l| l.name == format!("VPT_{i}")),
            "VPT_{i}"
        );
    }
    assert!(c.table("NOSUCH").is_none());
    c.stop().unwrap();
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn locks_between_processes() {
    use vmsportd::locks::{self, Mode::*};
    let d = dir("locks");
    let a = connect(&d);
    let b = connect(&d);
    let (_, st) = a.enq("VPT$FILE", PR, false).unwrap();
    assert_eq!(st, locks::SYNCH);
    assert_eq!(b.enq("VPT$FILE", EX, true), Err(locks::NOTQUEUED));
    let (rb, _) = b.enq("VPT$FILE", CR, false).unwrap();
    assert_eq!(b.convert(rb, PW, true), Err(locks::NOTQUEUED));
    // b waits for EX; a going away lets it through.
    let waiter = std::thread::spawn(move || {
        let st = b.convert(rb, EX, false);
        (b, st)
    });
    std::thread::sleep(std::time::Duration::from_millis(50));
    drop(a);
    let (b, st) = waiter.join().unwrap();
    assert_eq!(st, Ok(locks::NORMAL));
    assert_eq!(b.deq(rb), Ok(locks::NORMAL));
    assert_eq!(b.deq(rb), Err(locks::IVLOCKID));
    b.stop().unwrap();
}
