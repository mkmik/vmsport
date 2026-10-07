//! ANALYZE/RMS_FILE: the /CHECK report of each file (to SYS$OUTPUT, or
//! /OUTPUT's file, default type .ANL), or the /FDL description of the
//! first (to name.FDL, or /OUTPUT's). What it says and returns is what VMS
//! did (fixtures/fdlutil, fixtures/fdlutil2).

use libvms::fileinfo;
use libvms::rms::HostBlocks;
use std::io::Write;
use vms_cond::Cond;
use vms_fao::Arg;
use vms_filespec::FileSpec;
use vms_rms::{Fab, Rfm, analyze, rat};
use vms_utils::{E, Util, inhibit, shr, texts};

const ANALYZE: u32 = 177;
/// ANLRMS's (sys/SYSMSG/ANALYZE.MSG).
const OK: Cond = Cond(0x003B_8009);
const ERRORS: Cond = Cond(0x003B_8103);
const BADFILE: Cond = Cond(0x003B_88B2);
const RMSONLY: Cond = Cond(0x00B1_9F42);

fn main() {
    let mut u = Util::new(
        include_str!("../../../../sys/SYSLIB/DCLTABLES/ANALYZE.CLD"),
        ANALYZE,
    );
    if !u.present("RMS_FILE") {
        u.msg(&[(RMSONLY, vec![])]);
        u.exit(inhibit(RMSONLY));
    }
    let want_fdl = u.present("FDL");
    let output = u.value("OUTPUT").filter(|o| !o.is_empty());
    let command = u.img.command.line.clone();
    let now = vms_time::asctim(libvms::sys::now(), false);
    let items = texts(&u.values("FILE"));
    let mut files = Vec::new();
    let mut status = inhibit(if output.is_some() && !want_fdl {
        ERRORS
    } else {
        OK
    });
    // The files in order, and the specs that found none.
    for item in u.expand(&items, "") {
        match item.files {
            Ok(f) if !f.is_empty() => files.extend(f.into_iter().map(Ok)),
            r => files.push(Err((
                item.spec.expanded(),
                r.err().unwrap_or(libvms::status::FNF),
            ))),
        }
        // /FDL describes one file.
        if want_fdl && files.iter().any(|f| f.is_ok()) {
            break;
        }
    }
    if want_fdl {
        for (what, e) in files.iter().filter_map(|f| f.as_ref().err()) {
            status = openin(&u, what, *e);
        }
        let Some((path, spec)) = files.iter().find_map(|f| f.as_ref().ok()) else {
            u.exit(status)
        };
        let shown = spec.expanded();
        let r = open(path, &shown).and_then(|(fab, mut b, h)| analyze::fdl(&fab, &h, &mut b, &now));
        let text = match r {
            Ok(f) => f.to_string(),
            Err(e) => {
                let st = openin(&u, &shown, e);
                u.exit(st)
            }
        };
        let target = output.unwrap_or_else(|| spec.name.clone());
        if let Err(e) = write_to(&u, &target, ".FDL", &text) {
            u.exit(e);
        }
        u.exit(status);
    }
    let mut out = String::new();
    for f in &files {
        let (path, spec) = match f {
            Ok(f) => f,
            Err((what, e)) => {
                status = openin(&u, what, *e);
                continue;
            }
        };
        let shown = spec.expanded();
        let r = open(path, &shown)
            .and_then(|(fab, mut b, h)| analyze::check(&fab, &h, &mut b, &now, &command));
        let (text, errors) = match r {
            Ok(r) => r,
            Err(e) => {
                status = openin(&u, &shown, e);
                continue;
            }
        };
        if errors > 0 {
            status = inhibit(BADFILE);
        }
        if output.is_none() {
            print!("{text}");
            continue;
        }
        // To a file the report goes whole; to SYS$OUTPUT the message comes
        // before its last lines, the command.
        let (body, tail) = text.split_at(text.rfind("\n\n\n").map_or(text.len(), |i| i + 1));
        let message = (ERRORS, vec![Arg::Str(&shown), Arg::Num(errors as i64)]);
        if is_terminal(output.as_deref()) {
            print!("{body}");
            let _ = std::io::stdout().flush();
            u.msg(&[message]);
            print!("{tail}");
        } else {
            out.push_str(&text);
            u.msg(&[message]);
        }
    }
    if let Some(o) = output.as_deref().filter(|o| !is_terminal(Some(o)))
        && let Err(e) = write_to(&u, o, ".ANL", &out)
    {
        u.exit(e);
    }
    u.exit(status)
}

fn is_terminal(output: Option<&str>) -> bool {
    output.is_some_and(|o| {
        let o = o.trim_end_matches(':').to_ascii_uppercase();
        o == "SYS$OUTPUT" || o == "TT"
    })
}

/// The file's attributes, its blocks and its header. A directory is a
/// sequential file of 512-byte records, one block long (whose blocks the
/// reports don't read).
fn open(path: &std::path::Path, shown: &str) -> Result<(Fab, HostBlocks, analyze::Header), Cond> {
    let b = HostBlocks::new(std::fs::File::open(path).map_err(libvms::files::io_status)?)?;
    if path.is_dir() {
        let fab = Fab {
            rfm: Rfm::Var,
            rat: rat::BLK,
            mrs: 512,
            lrl: 512,
            ..Fab::default()
        };
        let h = analyze::Header {
            revision: 0,
            eof: 512,
            contiguous: true,
            protection: fileinfo::protection(
                std::os::unix::fs::PermissionsExt::mode(
                    &std::fs::metadata(path).map(|m| m.permissions()).unwrap(),
                ),
                true,
            ),
            ..fileinfo::header(path, shown, &fab)
        };
        return Ok((fab, b, h));
    }
    let fab = fileinfo::fab(path);
    let h = fileinfo::header(path, shown, &fab);
    Ok((fab, b, h))
}

/// %ANALYZE-E-OPENIN for a file it couldn't read; the status it leaves.
fn openin(u: &Util, what: &str, e: Cond) -> Cond {
    let open = u.shared(shr::OPENIN, E);
    u.msg(&[(open, vec![Arg::Str(what)]), (e, vec![])]);
    inhibit(open)
}

/// `text` to `spec` (default type `typ`) in the default directory, or to
/// SYS$OUTPUT. Returns the status to end with when it can't.
fn write_to(u: &Util, spec: &str, typ: &str, text: &str) -> Result<(), Cond> {
    if is_terminal(Some(spec)) {
        print!("{text}");
        return Ok(());
    }
    let s = &u.img.session;
    let made = s.parse(spec, typ, "").and_then(|sp| {
        s.new_version(&FileSpec {
            version: None,
            ..sp
        })
    });
    let r = made.and_then(|(path, _)| {
        let mut w = libvms::files::Writer::create(
            &path,
            Fab {
                rfm: Rfm::Var,
                rat: rat::CR,
                ..Fab::default()
            },
        )?;
        for l in text.lines() {
            w.put(&vms_rms::Record::new(l.as_bytes().to_vec()))?;
        }
        Ok(())
    });
    r.map_err(|e| {
        let open = u.shared(shr::OPENOUT, E);
        u.msg(&[(open, vec![Arg::Str(spec)]), (e, vec![])]);
        inhibit(open)
    })
}
