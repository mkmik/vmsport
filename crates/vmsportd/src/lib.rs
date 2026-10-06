//! vmsportd: the per-user daemon that holds the logical name tables VMS
//! shares between processes (job, group, system), and its client.
//!
//! The socket is `vmsportd.sock` in `$VMSPORT_RUN`, or in
//! `${TMPDIR:-/tmp}/vmsport-<uid>/`. A client starts the daemon on first
//! use. The protocol is one request line and a response ending with an
//! empty line; fields are tab-separated, with `\\`, `\t` and `\n` escaped:
//!
//! ```text
//! table   NAME                      -> T, then L <logical>... | nothing: no such table
//! define  TABLE <logical>           -> ok %Xcond | err %Xcond
//! deassign TABLE NAME MODE          -> ok %Xcond | err %Xcond
//! job     ID GID                    -> ok JOBTABLE GROUPTABLE NEW(0/1)
//! stop                              -> ok, and the daemon exits
//! ```
//!
//! A logical is `NAME MODE FLAGS EQUIV...`: MODE 0 (kernel) to 3 (user),
//! FLAGS some of `a` (no_alias), `c` (confine), `t` (table); each EQUIV
//! `FLAGS:text` with `c` (concealed) and `t` (terminal).

use std::cell::RefCell;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::fs::DirBuilderExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use vms_cond::Cond;
use vms_lnm::{Equiv, Logical, Mode, Shared, Table};

unsafe extern "C" {
    fn getuid() -> u32;
    fn getgid() -> u32;
}

/// `SS$_ABORT`: what a request gets when the daemon can't be reached.
const SS_ABORT: Cond = Cond(0x2C);

/// The run directory: `$VMSPORT_RUN`, else `${TMPDIR:-/tmp}/vmsport-<uid>`.
pub fn run_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("VMSPORT_RUN") {
        return d.into();
    }
    let tmp = std::env::var_os("TMPDIR").map_or_else(|| PathBuf::from("/tmp"), PathBuf::from);
    tmp.join(format!("vmsport-{}", unsafe { getuid() }))
}

/// `$VMSPORT`, else the source tree this was built from.
pub fn vmsport() -> PathBuf {
    std::env::var_os("VMSPORT").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."),
        PathBuf::from,
    )
}

/// A host directory as a VMS spec on the `HOST:` device; `rooted` gives
/// the `HOST:[A.B.]` form for a concealed device.
pub fn host_dir(path: &Path, rooted: bool) -> String {
    let parts: Vec<String> = path
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => Some(vms_filespec::escape(&s.to_string_lossy())),
            _ => None,
        })
        .collect();
    match (parts.is_empty(), rooted) {
        (true, _) => "HOST:[000000]".into(),
        (false, true) => format!("HOST:[{}.]", parts.join(".")),
        (false, false) => format!("HOST:[{}]", parts.join(".")),
    }
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
}

fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut cs = s.chars();
    while let Some(c) = cs.next() {
        out.push(match (c, c == '\\') {
            (_, true) => match cs.next() {
                Some('t') => '\t',
                Some('n') => '\n',
                Some(c) => c,
                None => '\\',
            },
            (c, false) => c,
        });
    }
    out
}

fn mode(n: &str) -> Mode {
    [Mode::Kernel, Mode::Executive, Mode::Supervisor, Mode::User]
        [n.parse::<usize>().unwrap_or(2).min(3)]
}

fn encode(l: &Logical) -> String {
    let flags: String = [(l.no_alias, 'a'), (l.confine, 'c'), (l.table, 't')]
        .iter()
        .filter(|f| f.0)
        .map(|f| f.1)
        .collect();
    let mut s = format!("{}\t{}\t{flags}", escape(&l.name), l.mode as usize);
    for e in &l.equivs {
        let f: String = [(e.concealed, 'c'), (e.terminal, 't')]
            .iter()
            .filter(|f| f.0)
            .map(|f| f.1)
            .collect();
        s += &format!("\t{f}:{}", escape(&e.text));
    }
    s
}

fn decode(fields: &[&str]) -> Option<Logical> {
    let [name, m, flags, equivs @ ..] = fields else {
        return None;
    };
    Some(Logical {
        name: unescape(name),
        mode: mode(m),
        no_alias: flags.contains('a'),
        confine: flags.contains('c'),
        table: flags.contains('t'),
        equivs: equivs
            .iter()
            .map(|e| {
                let (f, text) = e.split_once(':').unwrap_or(("", e));
                Equiv {
                    text: unescape(text),
                    concealed: f.contains('c'),
                    terminal: f.contains('t'),
                }
            })
            .collect(),
    })
}

fn status(r: Result<Cond, Cond>) -> String {
    match r {
        Ok(c) => format!("ok\t%X{:08X}", c.0),
        Err(c) => format!("err\t%X{:08X}", c.0),
    }
}

/// The system names vmsportd starts with, under `$VMSPORT/sys`.
fn system_names(tables: &mut Vec<Table>) {
    let sys = vmsport().join("sys");
    let sys = sys.canonicalize().unwrap_or(sys);
    let root = Equiv {
        concealed: true,
        terminal: true,
        ..Equiv::new(host_dir(&sys, true))
    };
    let exec = |name: &str, equivs: Vec<Equiv>| Logical {
        mode: Mode::Executive,
        equivs,
        ..Logical::new(name, &[])
    };
    let mut defs = vec![exec("SYS$SYSROOT", vec![root])];
    for (name, dir) in [
        ("SYS$SYSTEM", "SYSEXE"),
        ("SYS$MESSAGE", "SYSMSG"),
        ("SYS$LIBRARY", "SYSLIB"),
        ("SYS$HELP", "SYSHLP"),
    ] {
        defs.push(exec(name, vec![Equiv::new(format!("SYS$SYSROOT:[{dir}]"))]));
    }
    for l in defs {
        tables.define(vms_lnm::SYSTEM_TABLE, l).unwrap();
    }
}

/// One request against the shared tables. `None` for `stop`.
fn handle(tables: &Mutex<Vec<Table>>, line: &str) -> Option<String> {
    let f: Vec<&str> = line.split('\t').collect();
    let mut t = tables.lock().unwrap();
    Some(match f[..] {
        ["table", name] => {
            let tab = t.table(&unescape(name));
            let lines = |tab: Table| {
                tab.logicals
                    .iter()
                    .map(|l| format!("L\t{}\n", encode(l)))
                    .collect::<String>()
            };
            tab.map(|tab| format!("T\n{}", lines(tab)))
                .unwrap_or_default()
        }
        ["define", table, ref l @ ..] => match decode(l) {
            Some(l) => status(t.define(&unescape(table), l)) + "\n",
            None => "err\t%X00000014\n".into(),
        },
        ["deassign", table, name, m] => {
            status(t.deassign(&unescape(table), &unescape(name), mode(m))) + "\n"
        }
        ["job", id, gid] => {
            let job = vms_lnm::job_table(u32::from_str_radix(id, 16).unwrap_or(0));
            let group = vms_lnm::group_table(gid.parse().unwrap_or(0));
            let new = !t.iter().any(|x| x.name == job);
            vms_lnm::add_table(&mut t, &job);
            vms_lnm::add_table(&mut t, &group);
            format!("ok\t{job}\t{group}\t{}\n", new as u8)
        }
        ["stop"] => return None,
        _ => "err\t%X00000014\n".into(), // SS$_BADPARAM
    })
}

/// Runs the daemon in `dir` until a `stop`. Returns at once if another
/// daemon already holds the directory.
pub fn serve(dir: &Path) -> io::Result<()> {
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    let lock = std::fs::File::create(dir.join("vmsportd.lock"))?;
    if lock.try_lock().is_err() {
        return Ok(());
    }
    let sock = dir.join("vmsportd.sock");
    let _ = std::fs::remove_file(&sock);
    let listener = UnixListener::bind(&sock)?;
    let mut tables = vms_lnm::system_tables();
    system_names(&mut tables);
    let tables = Arc::new(Mutex::new(tables));
    for conn in listener.incoming() {
        let Ok(conn) = conn else { continue };
        let tables = tables.clone();
        let sock = sock.clone();
        std::thread::spawn(move || {
            let mut w = conn.try_clone().unwrap();
            for line in BufReader::new(conn).lines() {
                let Ok(line) = line else { return };
                match handle(&tables, &line) {
                    Some(resp) => {
                        if w.write_all(format!("{resp}\n").as_bytes()).is_err() {
                            return;
                        }
                    }
                    None => {
                        let _ = std::fs::remove_file(&sock);
                        let _ = w.write_all(b"ok\n\n");
                        std::process::exit(0);
                    }
                }
            }
        });
    }
    drop(lock);
    Ok(())
}

/// A connection to vmsportd; as [`Shared`], the job, group and system
/// tables for a [`vms_lnm::Names`].
pub struct Client {
    conn: RefCell<(BufReader<UnixStream>, UnixStream)>,
}

impl Client {
    /// Connects to the daemon in [`run_dir`], starting it if needed: the
    /// `vmsportd` next to this program, or `$VMSPORTD`, or on `$PATH`.
    pub fn connect() -> io::Result<Client> {
        let daemon = std::env::var_os("VMSPORTD")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                let sibling = std::env::current_exe()
                    .ok()
                    .and_then(|e| Some(e.parent()?.join("vmsportd")));
                sibling
                    .filter(|p| p.exists())
                    .unwrap_or_else(|| "vmsportd".into())
            });
        Client::connect_in(&run_dir(), &daemon)
    }

    /// Connects to the daemon in `dir`, starting `daemon` there if needed.
    pub fn connect_in(dir: &Path, daemon: &Path) -> io::Result<Client> {
        let sock = dir.join("vmsportd.sock");
        let mut started = false;
        for _ in 0..300 {
            match UnixStream::connect(&sock) {
                Ok(s) => {
                    return Ok(Client {
                        conn: RefCell::new((BufReader::new(s.try_clone()?), s)),
                    });
                }
                Err(_) if !started => {
                    started = true;
                    let mut child = std::process::Command::new(daemon)
                        .arg(dir)
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .process_group(0)
                        .spawn()?;
                    // Reap it whenever it exits, so it isn't left a zombie.
                    std::thread::spawn(move || child.wait());
                }
                Err(_) => std::thread::sleep(Duration::from_millis(10)),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            format!("vmsportd did not start in {}", dir.display()),
        ))
    }

    fn request(&self, line: &str) -> io::Result<Vec<String>> {
        let (r, w) = &mut *self.conn.borrow_mut();
        w.write_all(format!("{line}\n").as_bytes())?;
        let mut out = Vec::new();
        loop {
            let mut l = String::new();
            if r.read_line(&mut l)? == 0 {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
            let l = l.trim_end_matches('\n');
            if l.is_empty() {
                return Ok(out);
            }
            out.push(l.to_string());
        }
    }

    fn status(&self, line: &str) -> Result<Cond, Cond> {
        let resp = self.request(line).map_err(|_| SS_ABORT)?;
        let (kind, code) = resp
            .first()
            .and_then(|l| l.split_once('\t'))
            .ok_or(SS_ABORT)?;
        let c = Cond(u32::from_str_radix(code.trim_start_matches("%X"), 16).map_err(|_| SS_ABORT)?);
        if kind == "ok" { Ok(c) } else { Err(c) }
    }

    /// Joins job `id` (DCL's session id) and the user's group: makes their
    /// tables if they aren't there and, for a new job, its SYS$LOGIN,
    /// SYS$SCRATCH and SYS$LOGIN_DEVICE from `$HOME`. Returns the job and
    /// group table names for [`vms_lnm::Names::new`].
    pub fn job(&mut self, id: u32) -> io::Result<(String, String)> {
        let resp = self.request(&format!("job\t{id:X}\t{}", unsafe { getgid() }))?;
        let f: Vec<&str> = resp
            .first()
            .map(|l| l.split('\t').collect())
            .unwrap_or_default();
        let ["ok", job, group, new] = f[..] else {
            return Err(io::Error::other(format!("vmsportd: {resp:?}")));
        };
        let (job, group) = (job.to_string(), group.to_string());
        if new == "1" {
            let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from);
            let login = host_dir(&home, false);
            for (name, value) in [
                ("SYS$LOGIN", login.as_str()),
                ("SYS$SCRATCH", login.as_str()),
                ("SYS$LOGIN_DEVICE", "HOST:"),
            ] {
                let l = Logical {
                    mode: Mode::Executive,
                    ..Logical::new(name, &[value])
                };
                self.define(&job, l)
                    .map_err(|c| io::Error::other(format!("vmsportd: %X{:08X}", c.0)))?;
            }
        }
        Ok((job, group))
    }

    /// Asks the daemon to exit.
    pub fn stop(self) -> io::Result<()> {
        self.request("stop").map(drop)
    }
}

impl Shared for Client {
    fn table(&self, name: &str) -> Option<Table> {
        let resp = self.request(&format!("table\t{}", escape(name))).ok()?;
        let logicals = resp
            .get(1..)?
            .iter()
            .filter_map(|l| decode(&l.split('\t').collect::<Vec<_>>()[1..]))
            .collect();
        Some(Table {
            name: name.to_string(),
            logicals,
        })
    }

    fn define(&mut self, table: &str, l: Logical) -> Result<Cond, Cond> {
        self.status(&format!("define\t{}\t{}", escape(table), encode(&l)))
    }

    fn deassign(&mut self, table: &str, name: &str, m: Mode) -> Result<Cond, Cond> {
        self.status(&format!(
            "deassign\t{}\t{}\t{}",
            escape(table),
            escape(name),
            m as usize
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_format() {
        let l = Logical {
            no_alias: true,
            equivs: vec![
                Equiv {
                    concealed: true,
                    ..Equiv::new("A:[B.]\twith tab")
                },
                Equiv::new("x\\y"),
            ],
            ..Logical::new("N\nM", &[])
        };
        let line = encode(&l);
        assert!(!line.contains('\n'));
        assert_eq!(decode(&line.split('\t').collect::<Vec<_>>()), Some(l));
        assert_eq!(
            host_dir(Path::new("/Users/mkm/.claude"), true),
            "HOST:[Users.mkm.^.claude.]"
        );
        assert_eq!(host_dir(Path::new("/"), false), "HOST:[000000]");
    }
}
