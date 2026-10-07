//! EDIT/EDT: vmsport's EDT (crates/vms-edt). Line mode at the `*` prompt
//! (no prompt when commands come from a procedure's data lines), keypad
//! mode on a terminal (CHANGE, or SET MODE CHANGE in the command file); a
//! command file (/COMMAND, else EDTINI.EDT in the default directory); a
//! journal of what was typed (/JOURNAL, NAME.JOU), replayed by /RECOVER;
//! EXIT writes a new version of the file, in its own record format.

use libvms::Session;
use libvms::term::{self, Key};
use std::collections::VecDeque;
use std::io::{IsTerminal, Read, Write};
use std::path::PathBuf;
use vms_cond::Cond;
use vms_edt::keypad::{After, Keypad, Screen};
use vms_edt::{Edt, Files, Flow};
use vms_fao::Arg;
use vms_filespec::FileSpec;
use vms_rms::{Fab, Record};
use vms_utils::{Util, inhibit, shr};

const EDT: u32 = 133;
/// EDT-W-INPFILNEX, as VMS returned it (fixtures/edt/recorded/run2.log).
const INPFILNEX: Cond = Cond(0x0085_8148);

/// Files through the session: read as records, written as new versions in
/// the edited file's record format.
struct Host<'a> {
    s: &'a Session,
    fab: Fab,
}

impl Files for Host<'_> {
    fn read(&mut self, spec: &str) -> Result<Vec<String>, String> {
        let p = self
            .s
            .parse(spec, "", "")
            .map_err(|_| format!("Error opening {spec}"))?;
        let (path, _) = self
            .s
            .find(&p)
            .map_err(|_| "Input file does not exist".to_string())?;
        read_lines(&path).map_err(|_| format!("Error reading {}", p.expanded()))
    }

    fn write(&mut self, spec: &str, lines: &[String]) -> Result<String, String> {
        let p = self
            .s
            .parse(spec, "", "")
            .map_err(|_| format!("Error opening {spec}"))?;
        let (path, shown) = self
            .s
            .new_version(&FileSpec {
                version: None,
                ..p.clone()
            })
            .map_err(|_| format!("Error opening {} as output", p.expanded()))?;
        let r = if self.fab == Fab::default() {
            let mut text = lines.join("\n");
            if !lines.is_empty() {
                text.push('\n');
            }
            std::fs::write(&path, text).map_err(libvms::files::io_status)
        } else {
            libvms::files::Writer::create(&path, self.fab).and_then(|mut w| {
                lines
                    .iter()
                    .try_for_each(|l| w.put(&Record::new(l.as_bytes().to_vec())))
            })
        };
        r.map_err(|_| format!("Error writing {}", shown.expanded()))?;
        Ok(shown.expanded())
    }
}

/// A file's lines: a plain (stream_LF) file's text lines, else its records.
fn read_lines(path: &std::path::Path) -> Result<Vec<String>, Cond> {
    if libvms::files::fab(path) == Fab::default() {
        let text = std::fs::read(path).map_err(libvms::files::io_status)?;
        let text = String::from_utf8_lossy(&text);
        let mut lines: Vec<String> = text.split('\n').map(String::from).collect();
        if text.ends_with('\n') || text.is_empty() {
            lines.pop();
        }
        return Ok(lines);
    }
    let mut r = libvms::files::Reader::open(path)?;
    Ok(std::iter::from_fn(|| r.get())
        .map(|rec| String::from_utf8_lossy(&rec.data).into_owned())
        .collect())
}

/// What is typed: a journal being replayed first, then the terminal (or
/// SYS$INPUT), a byte at a time, each byte kept in the journal.
struct Input {
    replay: VecDeque<u8>,
    journal: Option<(PathBuf, std::fs::File)>,
}

impl Input {
    fn replaying(&self) -> bool {
        !self.replay.is_empty()
    }

    fn byte(&mut self) -> Option<u8> {
        let b = match self.replay.pop_front() {
            Some(b) => b,
            None => {
                let mut b = [0u8];
                loop {
                    match std::io::stdin().read(&mut b) {
                        Ok(1) => break b[0],
                        // Raw mode's reads time out: wait on.
                        Ok(0) if std::io::stdin().is_terminal() && raw_now() => continue,
                        _ => return None,
                    }
                }
            }
        };
        if let Some((_, j)) = &mut self.journal {
            let _ = j.write_all(&[b]);
        }
        Some(b)
    }

    fn line(&mut self) -> Option<String> {
        let mut buf = Vec::new();
        loop {
            match self.byte() {
                Some(b'\n') => break,
                Some(b) => buf.push(b),
                None if buf.is_empty() => return None,
                None => break,
            }
        }
        Some(
            String::from_utf8_lossy(&buf)
                .trim_end_matches('\r')
                .to_string(),
        )
    }
}

impl Read for Input {
    fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
        match (b.is_empty(), self.byte()) {
            (true, _) | (_, None) => Ok(0),
            (_, Some(x)) => {
                b[0] = x;
                Ok(1)
            }
        }
    }
}

static RAW: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn raw_now() -> bool {
    RAW.load(std::sync::atomic::Ordering::Relaxed)
}

fn main() {
    let mut u = Util::new(
        include_str!("../../../../sys/SYSLIB/DCLTABLES/EDIT.CLD"),
        EDT,
    );
    let spec = u.value("FILE").unwrap_or_default();
    let read_only = u.present("READ_ONLY");
    let output = match u.img.command.present("OUTPUT") {
        vms_cld::status::NEGATED => None,
        _ if read_only => None,
        _ => Some(
            u.value("OUTPUT")
                .filter(|o| !o.is_empty())
                .unwrap_or(spec.clone()),
        ),
    };
    let journaled = u.present("JOURNAL") && !read_only;
    let journal_spec = u.value("JOURNAL").filter(|j| !j.is_empty());
    let recover = u.present("RECOVER");
    let command = match u.img.command.present("COMMAND") {
        vms_cld::status::NEGATED => None,
        _ => Some(u.value("COMMAND").filter(|c| !c.is_empty())),
    };
    let create = u.present("CREATE");

    let s = &u.img.session;
    let parsed = s.parse(&spec, "", "").ok();
    let found = parsed.as_ref().and_then(|p| s.find(p).ok());
    let fab = found
        .as_ref()
        .map_or(Fab::default(), |(p, _)| libvms::files::fab(p));
    let text = match &found {
        Some((p, shown)) => match read_lines(p) {
            Ok(t) => Some(t),
            Err(e) => {
                let open = u.shared(shr::OPENIN, 4);
                u.msg(&[(open, vec![Arg::Str(&shown.expanded())]), (e, vec![])]);
                u.exit(inhibit(open));
            }
        },
        None => None,
    };
    if text.is_none() && !create {
        println!("Input file does not exist");
        u.exit(INPFILNEX);
    }
    let mut e = Edt::new(&spec, text);
    e.output = output;
    e.terminal = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    let mut files = Host { s, fab };

    // The journal: NAME.JOU in the default directory.
    let jspec = journal_spec.unwrap_or_else(|| {
        let name = parsed.as_ref().map_or("EDT".into(), |p| p.name.clone());
        format!("{name}.JOU")
    });
    let jpath = s
        .parse(&jspec, ".JOU", "")
        .ok()
        .and_then(|p| s.new_version(&FileSpec { version: None, ..p }).ok())
        .map(|(p, _)| p);
    let mut input = Input {
        replay: VecDeque::new(),
        journal: None,
    };
    if recover {
        let old = s
            .parse(&jspec, ".JOU", "")
            .ok()
            .and_then(|p| s.find(&p).ok());
        match old {
            Some((p, _)) => {
                input.replay = std::fs::read(&p).unwrap_or_default().into();
                let _ = std::fs::remove_file(&p);
            }
            None => {
                let open = u.shared(shr::OPENIN, 4);
                u.msg(&[
                    (open, vec![Arg::Str(&jspec)]),
                    (libvms::status::FNF, vec![]),
                ]);
                u.exit(inhibit(open));
            }
        }
    }
    if journaled
        && let Some(p) = jpath
        && let Ok(f) = std::fs::File::create(&p)
    {
        input.journal = Some((p, f));
    }

    // The command file.
    let cmd_file = match &command {
        Some(Some(c)) => Some((c.clone(), true)),
        Some(None) => Some(("EDTINI".to_string(), false)),
        None => None,
    };
    let mut out = std::io::stdout();
    if let Some((c, explicit)) = cmd_file {
        let lines = s
            .parse(&c, ".EDT", "")
            .ok()
            .and_then(|p| s.find(&p).ok())
            .map(|(p, _)| read_lines(&p));
        match lines {
            Some(Ok(lines)) => {
                e.from_file = true;
                for l in lines {
                    let f = e.command(&l, &mut files);
                    show(&mut e, &mut out);
                    if let Flow::Exit { .. } | Flow::Quit { .. } = f {
                        finish(&u, &mut input, f);
                    }
                }
                e.from_file = false;
            }
            _ if explicit => {
                let open = u.shared(shr::OPENIN, 4);
                let shown = s.parse(&c, ".EDT", "").map_or(c.clone(), |p| p.expanded());
                u.msg(&[
                    (open, vec![Arg::Str(&shown)]),
                    (libvms::status::FNF, vec![]),
                ]);
                u.exit(inhibit(open));
            }
            _ => {}
        }
    }

    // A journal replayed: what it typed, again, without EDT's counts; its
    // EXIT or QUIT ended that session, not this one.
    if input.replaying() {
        e.replaying = true;
        while input.replaying() {
            let Some(l) = input.line() else { break };
            e.print(l.clone());
            let ended = e.command(&l, &mut files);
            show(&mut e, &mut out);
            if ended == Flow::Change {
                // ponytail: keypad keys in a journal replay in keypad mode
                // only when the journal switched to it.
                let f = keypad_mode(&mut e, &mut input, &mut files, &mut out);
                if let Some(f) = f {
                    let _ = f;
                }
            }
        }
        e.replaying = false;
    }
    e.type_current();
    show(&mut e, &mut out);

    let mut flow = if e.set.change_mode && e.terminal {
        Flow::Change
    } else {
        Flow::Go
    };
    loop {
        if flow == Flow::Change {
            match keypad_mode(&mut e, &mut input, &mut files, &mut out) {
                Some(f) => finish(&u, &mut input, f),
                None => flow = Flow::Go,
            }
            continue;
        }
        if e.terminal {
            let _ = write!(out, "*");
            let _ = out.flush();
        }
        let line = input.line();
        flow = match line {
            None => e.end_of_input(),
            // ponytail: Ctrl/Z at the prompt is taken as nothing.
            Some(l) if l.contains('\x1a') && !e.terminal => e.end_of_input(),
            Some(l) => e.command(&l, &mut files),
        };
        show(&mut e, &mut out);
        if let Flow::Exit { .. } | Flow::Quit { .. } = flow {
            finish(&u, &mut input, flow);
        }
    }
}

fn show(e: &mut Edt, out: &mut impl Write) {
    for l in e.take() {
        let _ = writeln!(out, "{l}");
    }
    let _ = out.flush();
}

/// The journal goes, unless /SAVE kept it; the image ends.
fn finish(u: &Util, input: &mut Input, f: Flow) -> ! {
    let save = matches!(f, Flow::Exit { save: true } | Flow::Quit { save: true });
    if let Some((p, file)) = input.journal.take() {
        drop(file);
        if !save {
            let _ = std::fs::remove_file(p);
        }
    }
    let _ = u;
    std::process::exit(0);
}

/// Keypad mode until Ctrl/Z (None: back to line mode) or an EXIT or QUIT
/// at the COMMAND prompt.
fn keypad_mode(
    e: &mut Edt,
    input: &mut Input,
    files: &mut dyn Files,
    out: &mut std::io::Stdout,
) -> Option<Flow> {
    let _raw = term::Raw::new().ok();
    RAW.store(true, std::sync::atomic::Ordering::Relaxed);
    let mut k = Keypad::new();
    let mut last: Option<Screen> = None;
    let mut keys = term::Keys::new(&mut *input);
    let result = loop {
        let (rows, cols) = term::size();
        let s = k.screen(e, rows, cols);
        draw(out, &s, last.as_ref());
        last = Some(s);
        let Some(key) = keys.next() else {
            break Some(Flow::Quit { save: false });
        };
        if key == Key::Ctrl('W') {
            last = None;
        }
        match k.key(e, key, files) {
            After::Stay => {}
            After::LineMode => break None,
            After::Done(f) => break Some(f),
        }
    };
    RAW.store(false, std::sync::atomic::Ordering::Relaxed);
    let (rows, _) = term::size();
    let _ = write!(out, "\x1b[{rows};1H\x1b[K\r\n");
    let _ = out.flush();
    let said = e.take();
    for l in said {
        let _ = writeln!(out, "{l}");
    }
    result
}

/// Puts `s` on the terminal: the rows that changed since `last` (all of
/// them without one), then the cursor.
fn draw(out: &mut impl Write, s: &Screen, last: Option<&Screen>) {
    let mut buf = String::new();
    if last.is_none() {
        buf.push_str("\x1b[H\x1b[J");
    }
    for (r, row) in s.rows.iter().enumerate() {
        let rev = s.reverse.get(r).copied().flatten();
        let same = last.is_some_and(|l| {
            l.rows.get(r) == Some(row) && l.reverse.get(r).copied().flatten() == rev
        });
        if same {
            continue;
        }
        buf.push_str(&format!("\x1b[{};1H", r + 1));
        match rev {
            Some((a, z)) => {
                let chars: Vec<char> = row.chars().collect();
                let a = a.min(chars.len());
                let z = z.min(chars.len().max(a));
                let pre: String = chars[..a].iter().collect();
                let mid: String = chars[a..z].iter().collect();
                let post: String = chars[z..].iter().collect();
                buf.push_str(&format!("{pre}\x1b[7m{mid}\x1b[m{post}"));
            }
            None => buf.push_str(row),
        }
        buf.push_str("\x1b[K");
    }
    buf.push_str(&format!("\x1b[{};{}H", s.cursor.0 + 1, s.cursor.1 + 1));
    let _ = out.write_all(buf.as_bytes());
    let _ = out.flush();
}
