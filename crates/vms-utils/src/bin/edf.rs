//! EDIT/FDL, the FDL editor. At a terminal it asks (vms_utils::fdl_editor);
//! with /NOINTERACTIVE and /ANALYSIS=fdl it writes the input FDL's design
//! optimized for the data the analysis (ANALYZE/RMS_FILE/FDL's) describes
//! (vms_rms::edf), to /OUTPUT or a new version of the input. Only indexed
//! designs change; for others it writes nothing. What it says and returns
//! is what VMS's FDL editor did (fixtures/edf).

use std::io::{IsTerminal, Write};
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
/// EDF-F-DEVCLASS: the editor's questions want a terminal.
const DEVCLASS: Cond = Cond(0x00B3_800C);
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
    if u.present("INTERACTIVE") {
        if !std::io::stdin().is_terminal() {
            fail(u, DEVCLASS);
        }
        interactive(u);
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

/// The editor at the terminal: the definition (a new one if the file isn't
/// there), /SCRIPT, /ANALYSIS; EXIT writes /OUTPUT or a new version.
fn interactive(mut u: Util) -> ! {
    let input_spec = u.value("FILE").unwrap_or_default();
    let output = u.value("OUTPUT").filter(|o| !o.is_empty());
    let script = u.value("SCRIPT");
    let analysis = match u.value("ANALYSIS").filter(|a| !a.is_empty()) {
        Some(a) => match read(&u, &a).and_then(|t| parsed(&u, &t, Some(ILLSTRCLA))) {
            Ok(f) => Some(f),
            Err(st) => u.exit(st),
        },
        None => None,
    };
    let s = &u.img.session;
    let spec = s.parse(&input_spec, ".FDL", "");
    let shown = spec.as_ref().map_or(input_spec.clone(), |p| p.expanded());
    let text = spec
        .and_then(|p| s.find(&p))
        .and_then(|(path, _)| libvms::files::Reader::open(&path))
        .map(|mut r| {
            std::iter::from_fn(|| r.get())
                .map(|rec| String::from_utf8_lossy(&rec.data).into_owned() + "\n")
                .collect::<String>()
        })
        .ok();
    let help = libvms::help::library(s, "SYS$HELP:EDFHELP").unwrap_or_default();
    let help = vms_help::Help {
        libraries: vec![help],
        width: libvms::help::width(),
        instructions: false,
    };
    let now = vms_time::asctim(libvms::sys::now(), false);
    let emphasis = u.value("EMPHASIS");
    let granularity = u.value("GRANULARITY");
    let ending = vms_utils::fdl_editor::session(
        &mut Tty(&u),
        &help,
        vms_utils::fdl_editor::Start {
            text: text.as_deref(),
            shown: &shown,
            script: script.as_deref(),
            analysis,
            now: &now[..now.len().min(20)],
            emphasis: emphasis.as_deref(),
            granularity: granularity.as_deref(),
        },
    );
    if let vms_utils::fdl_editor::Ending::Exit(f, set) = ending {
        let text = edf::text(&f);
        match write(&u, &set.or(output).unwrap_or(input_spec), &text) {
            Ok(written) => println!("\n{written}  {} lines", text.lines().count()),
            Err(st) => u.exit(st),
        }
    }
    u.exit(Cond(1));
}

/// The terminal: answers read raw, so that Ctrl/Z ends one as on VMS
/// (echoed `*EXIT*`) rather than stopping the program.
struct Tty<'a>(&'a Util);

impl vms_utils::fdl_editor::Console for Tty<'_> {
    fn read(&mut self, spec: &str) -> Option<String> {
        read(self.0, spec).ok()
    }

    fn say(&mut self, text: &str) {
        print!("{text}");
        let _ = std::io::stdout().flush();
    }

    fn ask(&mut self, prompt: &str) -> Option<String> {
        use libvms::term::Key;
        self.say(prompt);
        let _raw = libvms::term::Raw::plain().ok()?;
        let mut line = String::new();
        let mut out = std::io::stdout();
        for k in libvms::term::keys() {
            match k {
                Key::Return | Key::KpEnter => {
                    let _ = write!(out, "\r\n");
                    return Some(line);
                }
                Key::Ctrl('Z' | 'C' | 'Y') => {
                    let _ = write!(out, "*EXIT*\r\n");
                    return None;
                }
                Key::Ctrl('U') => {
                    let _ = write!(out, "{}", "\x08 \x08".repeat(line.chars().count()));
                    line.clear();
                }
                Key::Delete | Key::Backspace => {
                    if line.pop().is_some() {
                        let _ = write!(out, "\x08 \x08");
                    }
                }
                Key::Char(ch) => {
                    line.push(ch);
                    let _ = write!(out, "{ch}");
                }
                _ => {}
            }
            let _ = out.flush();
        }
        None
    }
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

/// `text` as a new version of `spec` (default type .FDL), VAR records;
/// the name it got.
fn write(u: &Util, spec: &str, text: &str) -> Result<String, Cond> {
    let s = &u.img.session;
    let made = s.parse(spec, ".FDL", "").and_then(|sp| {
        s.new_version(&FileSpec {
            version: None,
            ..sp
        })
    });
    let shown = made.as_ref().map_or(String::new(), |m| m.1.expanded());
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
    r.map(|()| shown).map_err(|e| {
        let open = u.shared(shr::OPENOUT, E);
        u.msg(&[(open, vec![Arg::Str(spec)]), (e, vec![])]);
        inhibit(open)
    })
}
