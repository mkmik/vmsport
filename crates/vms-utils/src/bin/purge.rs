//! PURGE: deletes all but the newest versions of files (/KEEP=n of them).

use libvms::files;
use vms_cond::Cond;
use vms_fao::Arg;
use vms_utils::{DONE, I, Util, W, inhibit, shr, texts};

/// PURGE's messages say PURGE; its statuses are DELETE's, as on VMS (it is
/// DELETE.EXE there).
const PURGE: u32 = 148;
const DELETE: u32 = 147;

fn main() {
    let mut u = Util::new(
        include_str!("../../../../sys/SYSLIB/DCLTABLES/PURGE.CLD"),
        PURGE,
    );
    let mut items = texts(&u.values("INPUT"));
    if items.is_empty() {
        items.push(String::new());
    }
    let keep: usize = u
        .value("KEEP")
        .and_then(|k| k.parse().ok())
        .unwrap_or(1)
        .max(1);
    let log = u.present("LOG");
    let mut status = DONE;
    let (mut count, mut blocks) = (0, 0);
    for item in u.expand(&items, "*.*;*") {
        let files = match item.files {
            Ok(f) if !f.is_empty() => f,
            r => {
                let e = r.err().unwrap_or(libvms::status::FNF);
                u.msg(&[
                    (
                        u.shared(shr::SEARCHFAIL, W),
                        vec![Arg::Str(&item.spec.expanded())],
                    ),
                    (e, vec![]),
                ]);
                status = inhibit(Cond(DELETE << 16 | shr::SEARCHFAIL << 3 | W));
                continue;
            }
        };
        // Newest first within each name: skip the first `keep`.
        let mut seen: Option<String> = None;
        let mut n = 0;
        for (path, spec) in files {
            let key = format!("{}.{}", spec.name, spec.typ.as_deref().unwrap_or("")).to_uppercase();
            n = if seen.as_deref() == Some(&key) {
                n + 1
            } else {
                0
            };
            seen = Some(key);
            if n < keep {
                continue;
            }
            let alloc = files::info(&path).map_or(0, |i| i.allocated);
            if let Err(e) = files::delete(&path) {
                u.msg(&[(e, vec![])]);
                status = inhibit(e);
                continue;
            }
            count += 1;
            blocks += alloc;
            if log {
                u.msg(&[(
                    u.shared(shr::FILPURG, I),
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
