//! EDIT/TPU, VMS's default EDIT: EVE (crates/vms-eve) on the terminal,
//! or without one (/NODISPLAY: the initialization commands, then EXIT).
//! Files come and go through libvms, in their record formats; DCL
//! commands run through vmsport's dcl; /JOURNAL keeps the keys typed, for
//! /RECOVER to type them again.
//!
//! ponytail: TPU's keystroke journal, not EVE's buffer-change journals;
//! TPU programs (/COMMAND, /SECTION) are not run.

use libvms::term;
use std::cell::RefCell;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::rc::Rc;
use vms_cld::status as cli;
use vms_cond::Cond;
use vms_eve::{Done, EXIT_STATUS, Editor, Host, QUIT_STATUS, Start, screen};
use vms_fao::Arg;
use vms_filespec::FileSpec;
use vms_rms::{Fab, Record, Rfm, rat};
use vms_utils::{E, Util, inhibit, shr};

/// TPU's messages (facility 1010).
const TPU: u32 = 1010;

/// The terminal's state a subprocess has to leave as it found it.
struct Term {
    raw: Option<term::Raw>,
    redraw: bool,
}

struct Files {
    /// The image's, shared with main(): its session finds the files.
    util: Rc<RefCell<Util>>,
    term: Rc<RefCell<Term>>,
}

impl Files {
    fn parsed(&self, spec: &str) -> Result<FileSpec, String> {
        let u = self.util.borrow();
        u.img
            .session
            .parse(spec, "", "")
            .map_err(|_| spec.to_uppercase())
    }

    fn run_dcl(&self, command: &str, capture: bool) -> Vec<String> {
        let dcl = std::env::current_exe()
            .ok()
            .and_then(|e| Some(e.parent()?.join("dcl")))
            .unwrap_or_else(|| PathBuf::from("dcl"));
        let mut c = std::process::Command::new(dcl);
        if !command.is_empty() {
            c.args(["-c", command]);
        }
        if !capture {
            let _ = c.status();
            return Vec::new();
        }
        match c.stdin(std::process::Stdio::null()).output() {
            Ok(o) => String::from_utf8_lossy(&[o.stdout, o.stderr].concat())
                .lines()
                .map(str::to_string)
                .collect(),
            Err(e) => vec![e.to_string()],
        }
    }
}

impl Host for Files {
    fn read(&mut self, spec: &str) -> Result<(Vec<String>, String), String> {
        let parsed = self.parsed(spec)?;
        let u = self.util.borrow();
        let (path, shown) = u.img.session.find(&parsed).map_err(|_| {
            let mut s = parsed.clone();
            s.version = None;
            s.expanded()
        })?;
        let fab = libvms::files::fab(&path);
        let lines = if fab == Fab::default() {
            let bytes = std::fs::read(&path).map_err(|_| shown.expanded())?;
            String::from_utf8_lossy(&bytes)
                .lines()
                .map(str::to_string)
                .collect()
        } else {
            let mut r = libvms::files::Reader::open(&path).map_err(|_| shown.expanded())?;
            std::iter::from_fn(|| r.get())
                .map(|rec| String::from_utf8_lossy(&rec.data).into_owned())
                .collect()
        };
        Ok((lines, shown.expanded()))
    }

    fn write(&mut self, spec: &str, lines: &[String]) -> Result<(String, bool), String> {
        let mut parsed = self.parsed(spec)?;
        parsed.version = None;
        // The format of the file it replaces, if EVE can write it.
        let u = self.util.borrow();
        let session = &u.img.session;
        let old = session
            .find(&parsed)
            .ok()
            .map(|(p, _)| libvms::files::fab(&p));
        let (fab, converted) = match old {
            Some(f) if f.org == vms_rms::Org::Seq && f.rfm != Rfm::Vfc => (f, false),
            Some(_) => (
                Fab {
                    rfm: Rfm::Var,
                    rat: rat::CR,
                    ..Fab::default()
                },
                true,
            ),
            None => (Fab::default(), false),
        };
        let (path, shown) = session
            .new_version(&parsed)
            .map_err(|_| format!("Error writing file: {}", parsed.expanded()))?;
        let written = if fab == Fab::default() {
            let text: String = lines.iter().map(|l| format!("{l}\n")).collect();
            std::fs::write(&path, text).map_err(libvms::files::io_status)
        } else {
            libvms::files::Writer::create(&path, fab).and_then(|mut w| {
                lines
                    .iter()
                    .try_for_each(|l| w.put(&Record::new(l.as_bytes().to_vec())))
            })
        };
        written.map_err(|_| format!("Error writing file: {}", shown.expanded()))?;
        Ok((shown.expanded(), converted))
    }

    fn dcl(&mut self, command: &str) -> Vec<String> {
        self.run_dcl(command, true)
    }

    fn spawn(&mut self, command: &str) {
        let mut t = self.term.borrow_mut();
        let had = t.raw.take().is_some();
        print!("{}\x1b[24;1H\r\n", screen::END);
        let _ = std::io::stdout().flush();
        drop(t);
        self.run_dcl(command, false);
        let mut t = self.term.borrow_mut();
        if had {
            t.raw = term::Raw::new().ok();
            print!("{}", screen::START);
        }
        t.redraw = true;
    }
}

/// Keys from the terminal, each byte also kept in the journal.
struct Journaled {
    journal: Option<std::fs::File>,
}

impl Read for Journaled {
    fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
        loop {
            let n = std::io::stdin().read(b)?;
            // SAFETY: a plain query on fd 0.
            if n == 0 && unsafe { libc::isatty(0) } == 1 {
                continue;
            }
            if let Some(j) = &mut self.journal {
                j.write_all(&b[..n])?;
            }
            return Ok(n);
        }
    }
}

fn main() {
    let mut u = Util::new(
        include_str!("../../../../sys/SYSLIB/DCLTABLES/EDIT.CLD"),
        TPU,
    );
    let file = u.value("FILE").unwrap_or_default();
    let negated = |u: &Util, q: &str| u.img.command.present(q) == cli::NEGATED;
    let display = !negated(&u, "DISPLAY");
    let read_only = u.present("READ_ONLY");
    let mut start = Start {
        file: file.clone(),
        create: !negated(&u, "CREATE"),
        write: !read_only && !negated(&u, "WRITE"),
        modify: !read_only && !negated(&u, "MODIFY"),
        output: u.value("OUTPUT").filter(|o| !o.is_empty()),
        ..Start::default()
    };
    if u.present("START_POSITION") {
        let n: Vec<usize> = u
            .values("START_POSITION")
            .iter()
            .filter_map(|v| v.0.parse().ok())
            .collect();
        start.start_position = Some((
            n.first().copied().unwrap_or(1),
            n.get(1).copied().unwrap_or(1),
        ));
    }
    let value = |u: &mut Util, q: &str| u.value(q).filter(|v| !v.is_empty());
    let tpu_files = [
        ("COMMAND", value(&mut u, "COMMAND")),
        ("SECTION", value(&mut u, "SECTION")),
    ];
    let init_q = value(&mut u, "INITIALIZATION");
    let journal_q = value(&mut u, "JOURNAL");
    let session = &u.img.session;
    // /COMMAND's TPU program: there or not, it isn't run.
    let mut notes = Vec::new();
    for (q, spec) in tpu_files {
        let Some(spec) = spec else {
            continue;
        };
        let default = if q == "COMMAND" {
            ".TPU"
        } else {
            ".TPU$SECTION"
        };
        match session
            .parse(&spec, default, "")
            .and_then(|p| session.find(&p).map(|f| f.1))
        {
            Ok(found) => notes.push(format!(
                "vmsport's EVE runs no TPU programs: {}",
                found.expanded()
            )),
            Err(e) => {
                let shown = session
                    .parse(&spec, default, "")
                    .map_or(spec.clone(), |p| p.expanded());
                u.msg(&[
                    (u.shared(shr::OPENIN, E), vec![Arg::Str(&shown)]),
                    (e, vec![]),
                ]);
                u.exit(inhibit(Cond(0x03F2_ED7C)));
            }
        }
    }
    // /INITIALIZATION, else EVE$INIT.
    let init = match init_q {
        Some(v) => Some(v),
        None if negated(&u, "INITIALIZATION") => None,
        None => session
            .names
            .translate("EVE$INIT", "LNM$FILE_DEV", vms_lnm::Mode::User, false)
            .map(|_| "EVE$INIT".to_string()),
    };
    start.init = init.map(|name| {
        session
            .parse(&name, ".EVE", "")
            .and_then(|p| session.find(&p))
            .and_then(|(path, shown)| {
                let text = std::fs::read(path).map_err(libvms::files::io_status)?;
                Ok((
                    String::from_utf8_lossy(&text)
                        .lines()
                        .map(str::to_string)
                        .collect(),
                    shown.expanded(),
                ))
            })
            .map_err(|_| name.to_uppercase())
    });
    // The keystroke journal, and what /RECOVER types again from it.
    let journal_name = journal_q.or_else(|| {
        (u.present("JOURNAL") || u.present("RECOVER")).then(|| {
            let stem = file.rsplit([']', ':']).next().unwrap_or(&file);
            format!("{}.TJL", stem.split(['.', ';']).next().unwrap_or(stem))
        })
    });
    let journal_path = journal_name.as_ref().and_then(|j| {
        let p = session.parse(j, ".TJL", "").ok()?;
        session
            .find(&p)
            .map(|f| f.0)
            .or_else(|_| session.new_version(&p).map(|f| f.0))
            .ok()
    });
    let recover = u.present("RECOVER");
    let term = Rc::new(RefCell::new(Term {
        raw: None,
        redraw: false,
    }));
    let u = Rc::new(RefCell::new(u));
    let host = Files {
        util: u.clone(),
        term: term.clone(),
    };
    let exit = move |ed: Editor, u: Rc<RefCell<Util>>| -> ! {
        let st = match ed.done {
            Some(Done::Quit) => QUIT_STATUS,
            _ => EXIT_STATUS,
        };
        drop(ed);
        let u = Rc::try_unwrap(u)
            .ok()
            .expect("the editor let go")
            .into_inner();
        u.exit(Cond(st))
    };
    let (rows, cols) = term::size();
    let mut ed = Editor::new(rows, cols, Box::new(host));
    if !display {
        ed.nodisplay = Some(Vec::new());
        for n in notes {
            println!("{n}");
        }
        ed.start(&start);
        for m in ed.nodisplay.take().unwrap_or_default() {
            println!("{m}");
        }
        exit(ed, u);
    }
    let recovered = match (&journal_path, recover) {
        (Some(p), true) => std::fs::read(p).unwrap_or_default(),
        _ => Vec::new(),
    };
    let journal = journal_path.as_ref().and_then(|p| {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(p)
            .ok()
    });
    term.borrow_mut().raw = term::Raw::new().ok();
    print!("{}", screen::START);
    ed.start(&start);
    for n in notes {
        ed.run(&format!("! {n}"));
    }
    for k in term::Keys::new(&recovered[..]) {
        if ed.done.is_some() {
            break;
        }
        ed.key(k);
    }
    let mut keys = term::Keys::new(Journaled { journal });
    let mut old: Option<screen::Grid> = None;
    loop {
        let g = ed.grid();
        if std::mem::take(&mut term.borrow_mut().redraw) {
            old = None;
        }
        print!("{}", g.draw(old.as_ref()));
        let _ = std::io::stdout().flush();
        old = Some(g);
        if ed.done.is_some() {
            break;
        }
        let Some(k) = keys.next() else {
            break;
        };
        if k == term::Key::Ctrl('W') {
            old = None;
        }
        ed.key(k);
    }
    term.borrow_mut().raw = None;
    print!("{}\x1b[{rows};1H\r\n", screen::END);
    let _ = std::io::stdout().flush();
    if let Some(p) = journal_path.filter(|_| ed.done.is_some()) {
        let _ = std::fs::remove_file(p);
    }
    exit(ed, u);
}
