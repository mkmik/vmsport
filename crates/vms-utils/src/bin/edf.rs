//! EDIT/FDL, the FDL editor's non-interactive part: with /NOINTERACTIVE
//! and /ANALYSIS=fdl it writes the input FDL's design optimized for the
//! data the analysis (ANALYZE/RMS_FILE/FDL's) describes (vms_rms::edf), to
//! /OUTPUT or a new version of the input. Only indexed designs change; for
//! others it writes nothing. What it says and returns is what VMS's FDL
//! editor did (fixtures/edf/recorded/run1.log).
//!
//! ponytail: no text editors (EDIT without /FDL) and no interactive FDL
//! editor; each says so.

use vms_cond::Cond;
use vms_fao::Arg;
use vms_filespec::FileSpec;
use vms_rms::edf::{self, Outcome};
use vms_rms::{Fab, Record, Rfm, fdl, rat};
use vms_utils::{E, Util, inhibit, shr};

/// FDL's messages (sys/SYSMSG/FDL.MSG).
const OPENFDL: Cond = Cond(0x00B4_808C);
const UNPRIKW: Cond = Cond(0x00B4_80A2);
const UNQUAKW: Cond = Cond(0x00B4_8328);
const NOEDITOR: Cond = Cond(0x00B4_834C);
const NOINTER: Cond = Cond(0x00B4_8354);
/// STR-F-ILLSTRCLA: what VMS says of an analysis it can't parse.
const ILLSTRCLA: Cond = Cond(0x0024_8054);
const FDL: u32 = 180;

fn main() {
    let mut u = Util::new(
        include_str!("../../../../sys/SYSLIB/DCLTABLES/EDIT.CLD"),
        FDL,
    );
    let fail = |u: Util, c: Cond| -> ! {
        u.msg(&[(c, vec![])]);
        u.exit(inhibit(c))
    };
    // /FDL switches to EDIT_FDL's syntax, which isn't kept; only that
    // syntax has /INTERACTIVE.
    if u.img.command.present("INTERACTIVE") == vms_cld::status::UNDEFINED {
        fail(u, NOEDITOR);
    }
    if u.present("INTERACTIVE") {
        fail(u, NOINTER);
    }
    let input_spec = u.value("FILE").unwrap_or_default();
    let analysis_spec = u.value("ANALYSIS").unwrap_or_default();
    let granularity = u
        .value("GRANULARITY")
        .and_then(|g| g.parse().ok())
        .unwrap_or(3);
    let output = u.value("OUTPUT").filter(|o| !o.is_empty());
    // Without an analysis there is nothing to do, as on VMS.
    if analysis_spec.is_empty() {
        let st = read(&u, &input_spec).map_or_else(|st| st, |_| Cond(1));
        u.exit(st);
    }
    let files = read(&u, &input_spec)
        .and_then(|i| Ok((i, read(&u, &analysis_spec)?)))
        .and_then(|(i, a)| Ok((parsed(&u, &i, None)?, parsed(&u, &a, Some(ILLSTRCLA))?)));
    let (input, analysis) = match files {
        Ok(f) => f,
        Err(st) => u.exit(st),
    };
    let now = vms_time::asctim(libvms::sys::now(), false);
    match edf::optimize(&input, &analysis, granularity, &now[..now.len().min(20)]) {
        Outcome::Fdl(f) => {
            let spec = output.unwrap_or(input_spec);
            if let Err(st) = write(&u, &spec, &edf::text(&f)) {
                u.exit(st);
            }
        }
        Outcome::NotIndexed => println!(" The current file organization is not Indexed. "),
        Outcome::Nothing => {}
    }
    u.exit(Cond(1));
}

/// An FDL file's text (default type .FDL), or FDL-F-OPENFDL's status.
fn read(u: &Util, spec: &str) -> Result<String, Cond> {
    let s = &u.img.session;
    let parsed = s.parse(spec, ".FDL", "");
    let shown = parsed.as_ref().map_or(spec.to_string(), |p| p.expanded());
    match parsed
        .and_then(|p| s.find(&p))
        .and_then(|(path, _)| libvms::files::Reader::open(&path))
    {
        Ok(mut r) => Ok(std::iter::from_fn(|| r.get())
            .map(|rec| String::from_utf8_lossy(&rec.data).into_owned() + "\n")
            .collect()),
        Err(e) => {
            u.msg(&[(OPENFDL, vec![Arg::Str(&shown)]), (e, vec![])]);
            Err(inhibit(OPENFDL))
        }
    }
}

/// The FDL in `text`, or its problems shown and the status to end with:
/// an unknown keyword's, or `then` said after it (a bad value only warns).
fn parsed(u: &Util, text: &str, then: Option<Cond>) -> Result<fdl::Fdl, Cond> {
    let problems = fdl::check(text);
    let mut fatal = false;
    for p in &problems {
        let (c, n, w) = match p {
            fdl::Problem::Primary(n, w) => (UNPRIKW, n, w),
            fdl::Problem::Value(n, w) => (UNQUAKW, n, w),
        };
        fatal |= c == UNPRIKW;
        u.msg(&[(c, vec![Arg::Num(*n as i64), Arg::Str(w)])]);
    }
    if fatal {
        if let Some(c) = then {
            u.msg(&[(c, vec![])]);
        }
        return Err(inhibit(then.unwrap_or(UNPRIKW)));
    }
    Ok(fdl::parse(text).unwrap_or_default())
}

/// `text` as a new version of `spec` (default type .FDL), VAR records.
fn write(u: &Util, spec: &str, text: &str) -> Result<(), Cond> {
    let s = &u.img.session;
    let made = s.parse(spec, ".FDL", "").and_then(|sp| {
        s.new_version(&FileSpec {
            version: None,
            ..sp
        })
    });
    let r = made.and_then(|(path, _)| {
        let fab = Fab {
            rfm: Rfm::Var,
            rat: rat::CR,
            ..Fab::default()
        };
        let mut w = libvms::files::Writer::create(&path, fab)?;
        text.lines()
            .try_for_each(|l| w.put(&Record::new(l.as_bytes().to_vec())))
    });
    r.map_err(|e| {
        let open = u.shared(shr::OPENOUT, E);
        u.msg(&[(open, vec![Arg::Str(spec)]), (e, vec![])]);
        inhibit(open)
    })
}
