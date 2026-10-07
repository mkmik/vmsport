//! MOUNT: a Files-11 image file as a disk device (libvms::mount), its label
//! checked unless /OVERRIDE=IDENTIFICATION, and DISK$label (or the logical
//! name given) defined in the job table for the device.

use libvms::mount;
use vms_cond::Cond;
use vms_fao::Arg;
use vms_lnm::Logical;
use vms_utils::{E, Util, inhibit, shr};

const MOUNT: u32 = 114;
const DEVMOUNT: Cond = Cond(0x0072_006C);
const INCVOLLABEL: Cond = Cond(0x0072_010C);
const IVDEVNAM: Cond = Cond(0x0072_0144);
const MOUNTED: Cond = Cond(0x0072_800B);
const VOLIDENT: Cond = Cond(0x0072_8013);

fn main() {
    let mut u = Util::new(
        include_str!("../../../../sys/SYSLIB/DCLTABLES/MOUNT.CLD"),
        MOUNT,
    );
    let image = u.value("IMAGE").unwrap_or_default();
    let device = u.value("DEVICE").unwrap_or_default();
    let label = u.value("VOLUME_LABEL");
    let logical = u.value("LOGICAL_NAME");
    let check = !u
        .values("OVERRIDE")
        .iter()
        .any(|(v, _)| "IDENTIFICATION".starts_with(&v.to_ascii_uppercase()));
    let Ok(dev) = mount::device_name(&device) else {
        u.msg(&[(IVDEVNAM, vec![])]);
        u.exit(inhibit(IVDEVNAM));
    };
    if dev == libvms::HOST_DEVICE {
        u.msg(&[(DEVMOUNT, vec![])]);
        u.exit(inhibit(DEVMOUNT));
    }
    let path = match u
        .img
        .session
        .parse(&image, "", "")
        .and_then(|s| u.img.session.find(&s))
    {
        Ok((p, _)) => p,
        Err(e) => {
            u.msg(&[
                (u.shared(shr::OPENIN, E), vec![Arg::Str(&image)]),
                (e, vec![]),
            ]);
            u.exit(inhibit(e));
        }
    };
    let failed = |u: Util, (c, what): mount::Error| -> ! {
        if c == mount::DEVMOUNT {
            u.msg(&[(DEVMOUNT, vec![])]);
            u.exit(inhibit(DEVMOUNT));
        }
        u.msg(&[
            (u.shared(shr::OPENIN, E), vec![Arg::Str(&what)]),
            (c, vec![]),
        ]);
        u.exit(inhibit(c));
    };
    let vol = match mount::volume(&path) {
        Ok(v) => v,
        Err(e) => failed(u, e),
    };
    if label.is_some_and(|l| check && !l.eq_ignore_ascii_case(&vol.label)) {
        let pad = |s: &str| format!("{s:<12}");
        u.msg(&[
            (INCVOLLABEL, vec![]),
            (
                VOLIDENT,
                vec![
                    Arg::Str(&pad(&vol.label)),
                    Arg::Str(&pad(&vol.owner)),
                    Arg::Str(&pad(&vol.format)),
                ],
            ),
        ]);
        u.exit(inhibit(INCVOLLABEL));
    }
    if let Err(e) = mount::mount(&path, &dev) {
        failed(u, e);
    }
    let name = logical.unwrap_or_else(|| format!("DISK${}", vol.label));
    let name = name.trim_end_matches(':').to_ascii_uppercase();
    // Concealed and terminal, so specs show the name and not the device.
    let mut l = Logical::new(&name, &[&format!("{dev}:")]);
    l.equivs[0].concealed = true;
    l.equivs[0].terminal = true;
    let _ = u.img.session.define("LNM$JOB", l);
    u.msg(&[(
        MOUNTED,
        vec![Arg::Str(&vol.label), Arg::Str(&format!("_{dev}:"))],
    )]);
    u.exit(Cond(1));
}
