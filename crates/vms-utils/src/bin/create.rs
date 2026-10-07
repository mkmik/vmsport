//! CREATE: a file of the records read from SYS$INPUT (in a procedure, its
//! data lines), directories (/DIRECTORY), or an empty file of any
//! organization as an FDL describes it (/FDL; the messages are FDL's).
//! What it says and returns is what VMS did (fixtures/fdlutil).

use libvms::fileinfo::CLUSTER;
use libvms::files::{Writer, io_status};
use libvms::rms::HostBlocks;
use std::io::BufRead;
use vms_cond::Cond;
use vms_fao::Arg;
use vms_filespec::FileSpec;
use vms_rms::{Blocks, Fab, Org, Record, Rfm, fdl, rat};
use vms_utils::{E, I, NOSUCHFILE, Util, inhibit, shr, texts};

const CREATE: u32 = 145;
const CREATED: u32 = 526;
const EXISTS: u32 = 594;
const DIRNOTCRE: u32 = 600;
/// FDL's messages (sys/SYSMSG/FDL.MSG).
const FDL_CREATE: Cond = Cond(0x00B4_806C);
const OPENFDL: Cond = Cond(0x00B4_808C);
const FDL_CREATED: Cond = Cond(0x00B4_8323);
const UNQUAKW: Cond = Cond(0x00B4_8328);
const WARNING: Cond = Cond(0x00B4_8330);
const UNPRIKW: Cond = Cond(0x00B4_80A2);
const FDLERROR: Cond = Cond(0x00B4_8342);
/// LIB-F-INVFILSPE (sys/SYSMSG/LIB.MSG).
const INVFILSPE: Cond = Cond(0x0015_9F44);
const RMS_NORMAL: Cond = Cond(0x0001_0001);

fn main() {
    let u = Util::new(
        include_str!("../../../../sys/SYSLIB/DCLTABLES/CREATE.CLD"),
        CREATE,
    );
    if u.present("FDL") {
        from_fdl(u)
    } else if !u.present("FILE") {
        // Only /FDL may leave the file out (DCL would have asked for one).
        let c = Cond(0x38048);
        u.msg(&[(c, vec![])]);
        u.exit(inhibit(c))
    } else if u.present("DIRECTORY") {
        directories(u)
    } else {
        files(u)
    }
}

/// SYS$INPUT's records, up to its end.
fn input() -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    for line in std::io::stdin().lock().split(b'\n') {
        let Ok(mut l) = line else { break };
        if l.last() == Some(&b'\r') {
            l.pop();
        }
        out.push(l);
    }
    out
}

/// `dev:[dir]` of a spec.
fn dir_text(s: &FileSpec) -> String {
    format!(
        "{}:{}",
        s.device.as_deref().unwrap_or(""),
        s.directory.clone().unwrap_or_default()
    )
}

/// Variable length records with carriage return control; the first file
/// gets SYS$INPUT's records, the others none.
fn files(mut u: Util) -> ! {
    let log = u.present("LOG");
    let mut status = Cond(1);
    let mut related = String::new();
    let mut first = true;
    for text in texts(&u.values("FILE")) {
        let made = u.img.session.parse(&text, "", &related).map(|spec| {
            related = dir_text(&spec);
            (u.img.session.new_version(&spec), spec)
        });
        let fab = Fab {
            rfm: Rfm::Var,
            rat: rat::CR,
            ..Fab::default()
        };
        let (path, shown) = match made {
            Ok((Ok(p), _)) => p,
            Ok((Err(e), spec)) => {
                status = fail_out(&u, &spec.expanded(), e);
                continue;
            }
            Err(e) => {
                status = fail_out(&u, &text, e);
                continue;
            }
        };
        let records = if std::mem::take(&mut first) {
            input()
        } else {
            Vec::new()
        };
        let mut w = match Writer::create(&path, fab) {
            Ok(w) => w,
            Err(e) => {
                status = fail_out(&u, &shown.expanded(), e);
                continue;
            }
        };
        let mut longest = 0;
        for r in &records {
            longest = longest.max(r.len());
            if let Err(e) = w.put(&Record::new(r.clone())) {
                status = fail_out(&u, &shown.expanded(), e);
                break;
            }
        }
        let fab = Fab {
            lrl: longest as u16,
            ..fab
        };
        let _ = libvms::sys::set_xattr(&path, vms_rms::XATTR, fab.to_string().as_bytes());
        if log {
            let c = u.shared(CREATED, I);
            u.msg(&[(c, vec![Arg::Str(&shown.expanded())])]);
            if status.is_success() {
                status = inhibit(c);
            }
        }
    }
    u.exit(status)
}

/// %CREATE-E-OPENOUT and why; the status to return.
fn fail_out(u: &Util, what: &str, e: Cond) -> Cond {
    let open = u.shared(shr::OPENOUT, E);
    let mut msgs = vec![(open, vec![Arg::Str(what)]), (e, vec![])];
    if e == libvms::status::DNF {
        msgs.push((NOSUCHFILE, vec![]));
    }
    u.msg(&msgs);
    inhibit(open)
}

/// Each directory of the list, with the ones above it; one that is there
/// already says so, /LOG or not.
fn directories(mut u: Util) -> ! {
    let log = u.present("LOG");
    let (mut exists, mut failed, mut created) = (false, false, false);
    for text in texts(&u.values("FILE")) {
        let not_created = |u: &Util, e: Cond| {
            u.msg(&[(u.shared(DIRNOTCRE, E), vec![Arg::Str(&text)]), (e, vec![])])
        };
        let spec = match u.img.session.parse(&text, "", "") {
            Ok(s) if s.name.is_empty() && s.typ.is_none() => s,
            Ok(_) => {
                not_created(&u, INVFILSPE);
                failed = true;
                continue;
            }
            Err(e) => {
                not_created(&u, e);
                failed = true;
                continue;
            }
        };
        let (shown, path) = match u.img.session.locate(&spec) {
            Ok(mut v) if !v.is_empty() => v.remove(0),
            Ok(_) => (spec, Default::default()),
            Err(e) => {
                not_created(&u, e);
                failed = true;
                continue;
            }
        };
        if path.is_dir() {
            u.msg(&[(u.shared(EXISTS, I), vec![Arg::Str(&text)])]);
            exists = true;
            continue;
        }
        if let Err(e) = std::fs::create_dir_all(&path) {
            not_created(&u, io_status(e));
            failed = true;
            continue;
        }
        created = true;
        if log {
            u.msg(&[(u.shared(CREATED, I), vec![Arg::Str(&dir_text(&shown))])]);
        }
    }
    let st = match () {
        _ if failed => inhibit(u.shared(DIRNOTCRE, E)),
        _ if exists => inhibit(u.shared(EXISTS, I)),
        _ if created && log => inhibit(u.shared(CREATED, I)),
        _ => Cond(1),
    };
    u.exit(st)
}

/// An empty file as the FDL says: sequential, relative (all of its
/// allocation formatted, rounded up to the cluster) or indexed.
fn from_fdl(mut u: Util) -> ! {
    let log = u.present("LOG");
    let source = u.value("FDL").unwrap_or_default();
    let file = u.value("FILE");
    let text: String = if source
        .trim()
        .trim_end_matches(':')
        .eq_ignore_ascii_case("SYS$INPUT")
    {
        input()
            .into_iter()
            .map(|l| String::from_utf8_lossy(&l).into_owned() + "\n")
            .collect()
    } else {
        let s = &u.img.session;
        let spec = s.parse(&source, ".FDL", "");
        let shown = spec.as_ref().map_or(source.clone(), |s| s.expanded());
        match spec
            .and_then(|sp| s.find(&sp))
            .and_then(|(p, _)| libvms::files::Reader::open(&p))
        {
            Ok(mut r) => std::iter::from_fn(|| r.get())
                .map(|rec| String::from_utf8_lossy(&rec.data).into_owned() + "\n")
                .collect(),
            Err(e) => {
                u.msg(&[(OPENFDL, vec![Arg::Str(&shown)]), (e, vec![])]);
                u.exit(inhibit(OPENFDL));
            }
        }
    };
    let parsed = fdl::parse(&text).unwrap_or_default();
    // The file parameter, else the FDL's NAME.
    let name = file.unwrap_or_else(|| {
        let n = parsed
            .section("FILE", "")
            .and_then(|f| f.get("NAME"))
            .unwrap_or("");
        n.trim_matches('"').to_string()
    });
    let problems = fdl::check(&text);
    for p in &problems {
        let (c, n, w) = match p {
            fdl::Problem::Primary(n, w) => (UNPRIKW, n, w),
            fdl::Problem::Value(n, w) => (UNQUAKW, n, w),
        };
        u.msg(&[(c, vec![Arg::Num(*n as i64), Arg::Str(w)])]);
    }
    let design = match fdl::to_design(&parsed) {
        Ok(d) if problems.is_empty() => d,
        _ => {
            let why = match problems.first() {
                Some(fdl::Problem::Value(..)) => WARNING,
                _ => FDLERROR,
            };
            u.msg(&[(FDL_CREATE, vec![Arg::Str(&name)]), (why, vec![])]);
            u.exit(inhibit(FDL_CREATE));
        }
    };
    let s = &u.img.session;
    let spec = match s.parse(&name, "", "") {
        Ok(sp) => sp,
        Err(e) => {
            u.msg(&[(FDL_CREATE, vec![Arg::Str(&name)]), (e, vec![])]);
            u.exit(inhibit(FDL_CREATE));
        }
    };
    let made = s.new_version(&spec).and_then(|(path, shown)| {
        make(&path, &design).inspect_err(|_| {
            let _ = std::fs::remove_file(&path);
        })?;
        Ok(shown)
    });
    match made {
        Ok(shown) => {
            if log {
                u.msg(&[(FDL_CREATED, vec![Arg::Str(&shown.expanded())])]);
            }
            u.exit(RMS_NORMAL)
        }
        Err(e) => {
            u.msg(&[(FDL_CREATE, vec![Arg::Str(&spec.expanded())]), (e, vec![])]);
            u.exit(inhibit(FDL_CREATE))
        }
    }
}

/// The file at `path`, new, as `d` describes it.
fn make(path: &std::path::Path, d: &vms_rms::Design) -> Result<(), Cond> {
    match d.fab.org {
        Org::Seq => Writer::create(path, d.fab).map(drop),
        Org::Rel => {
            let f = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .open(path)
                .map_err(io_status)?;
            let mut b = HostBlocks::new(f)?;
            let alloc = d.areas.first().map_or(0, |a| a.allocation).max(1);
            b.grow(alloc.div_ceil(CLUSTER) * CLUSTER)?;
            let rel = vms_rms::rel::create(&mut b, d)?;
            libvms::sys::set_xattr(path, vms_rms::XATTR, rel.fab.to_string().as_bytes())
                .map_err(|_| libvms::status::CRE)
        }
        // ponytail: indexed files once vms_rms::idx can make them.
        Org::Idx => Err(vms_rms::status::ORG),
    }
}
