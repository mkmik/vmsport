//! DELETE: deletes files. Each spec needs a version (`;`, `;n`, `;*`).
//! (DELETE/SYMBOL is DCL's own.)

use libvms::files;
use vms_fao::Arg;
use vms_utils::{DONE, E, I, Util, W, inhibit, shr, texts};

const DELETE: u32 = 147;

fn main() {
    let mut u = Util::new(
        include_str!("../../../../sys/SYSLIB/DCLTABLES/DELETE.CLD"),
        DELETE,
    );
    let items = texts(&u.values("INPUT"));
    let log = u.present("LOG");
    let mut status = DONE;
    let (mut count, mut blocks) = (0, 0);
    for item in u.expand(&items, "") {
        if item.typed.version.is_none() {
            let delver = u.shared(shr::DELVER, E);
            u.msg(&[(delver, vec![])]);
            status = inhibit(delver);
            continue;
        }
        let files = match item.files {
            Ok(f) if !f.is_empty() => f,
            r => {
                let e = r.err().unwrap_or(libvms::status::FNF);
                let fail = u.shared(shr::SEARCHFAIL, W);
                u.msg(&[(fail, vec![Arg::Str(&item.spec.expanded())]), (e, vec![])]);
                status = inhibit(fail);
                continue;
            }
        };
        for (path, spec) in files {
            let alloc = files::info(&path).map_or(0, |i| i.allocated);
            if let Err(e) = files::delete(&path) {
                u.msg(&[
                    (
                        u.shared(shr::FILDEL, E),
                        vec![Arg::Str(&spec.expanded()), Arg::Num(0)],
                    ),
                    (e, vec![]),
                ]);
                status = inhibit(e);
                continue;
            }
            count += 1;
            blocks += alloc;
            if log {
                u.msg(&[(
                    u.shared(shr::FILDEL, I),
                    vec![Arg::Str(&spec.expanded()), Arg::Num(alloc as i64)],
                )]);
            }
        }
    }
    if log && count > 1 {
        u.msg(&[(
            u.shared(shr::TOTAL, I),
            vec![Arg::Num(count), Arg::Num(blocks as i64)],
        )]);
    }
    u.exit(status);
}
