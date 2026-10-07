//! DISMOUNT: writes a mounted image's changes back (libvms::mount) and
//! deassigns the logical name MOUNT defined for the device. Quiet when it
//! works; otherwise its status says why, for DCL to show.

use libvms::mount;
use vms_cond::Cond;
use vms_utils::Util;

const DEVNOTMOUNT: Cond = Cond(0x7C);
const NOSUCHDEV: Cond = Cond(0x908);

fn main() {
    let mut u = Util::new(
        include_str!("../../../../sys/SYSLIB/DCLTABLES/DISMOUNT.CLD"),
        0,
    );
    let device = u.value("DEVICE").unwrap_or_default();
    let dev = match mount::device_name(&device) {
        Ok(d) if d == libvms::HOST_DEVICE => u.exit(DEVNOTMOUNT),
        Ok(d) if mount::mounted(&d).is_some() => d,
        _ => u.exit(NOSUCHDEV),
    };
    if let Err((c, what)) = mount::dismount(&dev) {
        u.msg(&[(c, vec![])]);
        eprintln!("-DISMOUNT-I-IMAGE, {what}");
        u.exit(Cond(c.0 | 0x1000_0000));
    }
    // The job's logical names for the device go with it.
    let target = format!("{dev}:");
    let names = &u.img.session.names;
    let job = names.tables("LNM$JOB", vms_lnm::Mode::User);
    let names: Vec<String> = job
        .first()
        .and_then(|t| names.get(t))
        .map(|t| {
            t.logicals
                .iter()
                .filter(|l| {
                    l.equivs
                        .first()
                        .is_some_and(|e| e.text.eq_ignore_ascii_case(&target))
                })
                .map(|l| l.name.clone())
                .collect()
        })
        .unwrap_or_default();
    for n in names {
        let _ = u.img.session.deassign("LNM$JOB", &n);
    }
    u.exit(Cond(1));
}
