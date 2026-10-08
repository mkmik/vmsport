//! COPY: copies files record by record, keeping their attributes. Several
//! inputs to one output that has no wildcard are concatenated, as with `+`.

use libvms::files::{Reader, Writer};
use std::os::unix::fs::FileExt;
use vms_cond::Cond;
use vms_fao::Arg;
use vms_filespec::FileSpec;
use vms_utils::{E, S, Util, inhibit, shr};

fn main() {
    let mut u = Util::new(
        include_str!("../../../../sys/SYSLIB/DCLTABLES/COPY.CLD"),
        103,
    );
    let inputs = u.values("INPUT");
    let output = u.value("OUTPUT").unwrap_or_default();
    let log = u.present("LOG");
    let out = match u.img.session.parse(&output, "", "") {
        Ok(o) => o,
        Err(e) => {
            u.msg(&[
                (u.shared(shr::OPENOUT, E), vec![Arg::Str(&output)]),
                (e, vec![]),
            ]);
            u.exit(inhibit(e));
        }
    };
    // Wildcards or missing fields in the output come from each input.
    let wild_out =
        out.name.contains(['*', '%']) || out.typ.as_deref().is_some_and(|t| t.contains(['*', '%']));
    let texts: Vec<String> = inputs.iter().map(|i| i.0.clone()).collect();
    let mut files = Vec::new();
    let mut status = Cond(1);
    for item in u.expand(&texts, "") {
        match item.files {
            Ok(f) if !f.is_empty() => files.extend(f),
            r => {
                let e = r.err().unwrap_or(libvms::status::FNF);
                let open = u.shared(shr::OPENIN, E);
                u.msg(&[(open, vec![Arg::Str(&item.spec.expanded())]), (e, vec![])]);
                status = inhibit(open);
            }
        }
    }
    let concatenate = !wild_out && files.len() > 1;
    let mut current: Option<(Writer, FileSpec)> = None;
    let mut created = 0;
    for (path, spec) in &files {
        let mut r = match Reader::open(path) {
            Ok(r) => r,
            Err(e) => {
                u.msg(&[
                    (u.shared(shr::OPENIN, E), vec![Arg::Str(&spec.expanded())]),
                    (e, vec![]),
                ]);
                status = inhibit(e);
                continue;
            }
        };
        let appending = concatenate && current.is_some();
        if !appending {
            let mut target = out.clone();
            if target.name.is_empty() || target.name.contains(['*', '%']) {
                target.name = spec.name.clone();
            }
            if target.typ.as_deref().is_none_or(|t| t.contains(['*', '%'])) {
                target.typ = spec.typ.clone();
            }
            target.version = None;
            let opened = u
                .img
                .session
                .new_version(&target)
                .and_then(|(p, shown)| Ok((Writer::create(&p, r.fab)?, shown)));
            match opened {
                Ok(w) => current = Some(w),
                Err(e) => {
                    let open = u.shared(shr::OPENOUT, E);
                    u.msg(&[(open, vec![Arg::Str(&target.expanded())]), (e, vec![])]);
                    status = inhibit(open);
                    continue;
                }
            }
            created += 1;
        }
        let (w, shown) = current.as_mut().unwrap();
        let mut records = 0;
        // A relative or indexed file is copied as it is, block for block.
        if !appending && r.fab.org != vms_rms::Org::Seq {
            let copied = std::fs::read(path)
                .and_then(|b| w.file().write_all_at(&b, 0))
                .map_err(libvms::files::io_status);
            if let Err(e) = copied {
                status = e;
            }
            while r.get().is_some() {}
        }
        while let Some(rec) = r.get() {
            if let Err(e) = w.put(&rec) {
                status = e;
                break;
            }
            records += 1;
        }
        if log {
            let (from, to) = (spec.expanded(), shown.expanded());
            let blocks = w.file().metadata().map_or(0, |m| m.len().div_ceil(512));
            let m = if appending {
                (u.shared(shr::APPENDED_RECORDS, S), records)
            } else if blocks == 0 {
                (u.shared(shr::COPIED_RECORDS, S), records)
            } else {
                (u.shared(shr::COPIED_BLOCKS, S), blocks)
            };
            u.msg(&[(
                m.0,
                vec![Arg::Str(&from), Arg::Str(&to), Arg::Num(m.1 as i64)],
            )]);
        }
    }
    if log && created > 1 {
        u.msg(&[(u.shared(shr::NEWFILES, S), vec![Arg::Num(created)])]);
    }
    u.exit(status);
}
