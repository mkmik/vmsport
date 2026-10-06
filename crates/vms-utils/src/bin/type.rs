//! TYPE: copies files to SYS$OUTPUT, their carriage control turned into
//! text. A file from a wildcard, or after the first, gets its name first.

use libvms::files::Reader;
use std::io::Write;
use vms_fao::Arg;
use vms_utils::{DONE, E, Util, W, inhibit, shr, texts};

fn main() {
    let mut u = Util::new(
        include_str!("../../../../sys/SYSLIB/DCLTABLES/TYPE.CLD"),
        149,
    );
    let items = texts(&u.values("INPUT"));
    let mut status = DONE;
    let mut n = 0;
    for item in u.expand(&items, ".LIS") {
        let wild = item.wild();
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
            let mut out = std::io::stdout().lock();
            if wild || n > 0 {
                let _ = write!(out, " \n{}\n \n", spec.expanded());
            }
            n += 1;
            let mut r = match Reader::open(&path) {
                Ok(r) => r,
                Err(e) => {
                    drop(out);
                    u.msg(&[
                        (u.shared(shr::OPENIN, E), vec![Arg::Str(&spec.expanded())]),
                        (e, vec![]),
                    ]);
                    status = inhibit(e);
                    continue;
                }
            };
            let mut text = vms_rms::Text::new();
            while let Some(rec) = r.get() {
                let _ = out.write_all(&text.record(&r.fab, &rec));
            }
            let _ = out.write_all(&text.finish());
            let _ = out.flush();
        }
    }
    u.exit(status);
}
