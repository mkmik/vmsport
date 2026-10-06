//! Running images and subprocesses: DCL hands a child its context (default
//! directory, process logical names, symbols, command tables, command line)
//! on an inherited socket, `VMSPORT_CONTEXT=<fd>`. An image sends back the
//! symbols and process logical names it changed, then its 32-bit status;
//! a subprocess (SPAWN, PIPE) just its status. See docs/design/m1.md.

use crate::{Session, status};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use vms_cond::Cond;
use vms_lnm::Logical;

/// A DCL symbol's value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i32),
    Str(String),
}

/// A symbol as it travels: name, global, value.
#[derive(Debug, Clone, PartialEq)]
pub struct Symbol {
    pub name: String,
    pub global: bool,
    pub value: Value,
}

/// What an image changed in DCL's process, in order.
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    Set(Symbol),
    Delete { name: String, global: bool },
    Define(Logical),
    Deassign(String),
}

/// What DCL gives a child.
#[derive(Default)]
pub struct Launch<'a> {
    pub default: &'a str,
    pub process: &'a [Logical],
    pub symbols: &'a [Symbol],
    /// CLD text for an image; empty for a DCL subprocess.
    pub tables: &'a str,
    pub line: &'a str,
    /// `spawn`: the child is DCL; `line` is its command (empty: it reads
    /// commands from its input).
    pub spawn: bool,
    pub stdin: Option<Stdio>,
    pub stdout: Option<Stdio>,
    pub stderr: Option<Stdio>,
    pub args: &'a [String],
    /// SPAWN/NOLOGICAL_NAMES: no process logical names.
    pub no_logicals: bool,
}

/// How a child ended.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub status: Cond,
    pub changes: Vec<Change>,
}

/// A child that is running.
pub struct Child {
    child: std::process::Child,
    ours: UnixStream,
    writer: std::thread::JoinHandle<()>,
}

/// Without a status line, a Unix exit code n stands for this plus n*8: an
/// error in the C run-time's facility, already shown.
const C_EXIT: u32 = 0x1035_A002;

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
}

fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut cs = s.chars();
    while let Some(c) = cs.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match cs.next() {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some(c) => out.push(c),
            None => {}
        }
    }
    out
}

fn scope(global: bool) -> &'static str {
    if global { "G" } else { "L" }
}

/// `L|G  I|S  name  value`
fn encode_symbol(s: &Symbol) -> String {
    let (t, v) = match &s.value {
        Value::Int(n) => ("I", n.to_string()),
        Value::Str(v) => ("S", escape(v)),
    };
    format!("{}\t{t}\t{}\t{v}", scope(s.global), escape(&s.name))
}

fn decode_symbol(f: &[&str]) -> Option<Symbol> {
    let [g, t, name, v] = f else { return None };
    Some(Symbol {
        name: unescape(name),
        global: *g == "G",
        value: if *t == "I" {
            Value::Int(v.parse().ok()?)
        } else {
            Value::Str(unescape(v))
        },
    })
}

/// Starts `program` with `l`'s context.
pub fn start(program: &Path, l: Launch) -> Result<Child, Cond> {
    let (ours, theirs) = UnixStream::pair().map_err(|_| Cond(0x2C))?;
    let fd = theirs.as_raw_fd();
    let mut cmd = Command::new(program);
    cmd.args(l.args)
        .env("VMSPORT_CONTEXT", fd.to_string())
        .env("VMSPORT_JOB", format!("{:X}", crate::job_id()));
    if let Some(s) = l.stdin {
        cmd.stdin(s);
    }
    if let Some(s) = l.stdout {
        cmd.stdout(s);
    }
    if let Some(s) = l.stderr {
        cmd.stderr(s);
    }
    // SAFETY: only fcntl between fork and exec.
    unsafe {
        cmd.pre_exec(move || {
            if libc::fcntl(fd, libc::F_SETFD, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let child = cmd.spawn().map_err(|_| status::FNF)?;
    drop(theirs);
    let mut msg = format!("vmsport-context\t1\ndefault\t{}\n", l.default);
    for lg in l.process {
        msg += &format!("lnm\t{}\n", vmsportd::encode(lg));
    }
    for s in l.symbols {
        msg += &format!("symbol\t{}\n", encode_symbol(s));
    }
    if l.spawn {
        msg += "spawn\t1\n";
    }
    msg += &format!(
        "tables\t{}\n{}line\t{}\n\n",
        l.tables.len(),
        l.tables,
        escape(l.line)
    );
    let mut w = ours.try_clone().map_err(|_| Cond(0x2C))?;
    // A writer thread, so a child that never reads can't block us.
    let writer = std::thread::spawn(move || {
        let _ = w.write_all(msg.as_bytes());
    });
    Ok(Child {
        child,
        ours,
        writer,
    })
}

impl Child {
    /// Waits for the child: its status and the changes it sent.
    pub fn wait(mut self) -> Outcome {
        let exit = self.child.wait();
        let _ = self.writer.join();
        let _ = self.ours.shutdown(std::net::Shutdown::Write);
        let mut reply = String::new();
        let _ = BufReader::new(&self.ours).read_to_string(&mut reply);
        let mut out = Outcome {
            status: status::NORMAL,
            changes: Vec::new(),
        };
        let mut st = None;
        for line in reply.lines() {
            let f: Vec<&str> = line.split('\t').collect();
            match f[..] {
                ["status", v] => {
                    st = u32::from_str_radix(v.trim_start_matches("%X"), 16)
                        .ok()
                        .map(Cond)
                }
                ["symbol", "set", ref rest @ ..] => {
                    out.changes.extend(decode_symbol(rest).map(Change::Set))
                }
                ["symbol", "del", g, name] => out.changes.push(Change::Delete {
                    name: unescape(name),
                    global: g == "G",
                }),
                ["lnm", "set", ref rest @ ..] => out
                    .changes
                    .extend(vmsportd::decode(rest).map(Change::Define)),
                ["lnm", "del", name] => out.changes.push(Change::Deassign(unescape(name))),
                _ => {}
            }
        }
        out.status = st.unwrap_or_else(|| match exit.ok().and_then(|e| e.code()) {
            Some(0) => status::NORMAL,
            Some(n) => Cond(C_EXIT + (n as u32) * 8),
            None => Cond(0x2C), // killed: SS$_ABORT
        });
        out
    }
}

/// Runs `program` with `l`'s context and waits for it.
pub fn run(program: &Path, l: Launch) -> Outcome {
    match start(program, l) {
        Ok(c) => c.wait(),
        Err(st) => Outcome {
            status: st,
            changes: Vec::new(),
        },
    }
}

/// What a child gets from DCL.
pub struct Context {
    pub default: String,
    pub process: Vec<Logical>,
    pub symbols: Vec<Symbol>,
    pub tables: String,
    pub line: String,
    /// A DCL subprocess (SPAWN, PIPE), not an image.
    pub spawn: bool,
    changes: Vec<Change>,
    stream: UnixStream,
}

impl Context {
    /// The context DCL passed, if it ran us.
    pub fn get() -> Option<Context> {
        let fd: i32 = std::env::var("VMSPORT_CONTEXT").ok()?.parse().ok()?;
        // SAFETY: DCL left this descriptor open for us; we own it from here.
        unsafe { std::env::remove_var("VMSPORT_CONTEXT") };
        let stream = unsafe { <UnixStream as std::os::fd::FromRawFd>::from_raw_fd(fd) };
        let mut r = BufReader::new(stream.try_clone().ok()?);
        let mut ctx = Context {
            default: String::new(),
            process: Vec::new(),
            symbols: Vec::new(),
            tables: String::new(),
            line: String::new(),
            spawn: false,
            changes: Vec::new(),
            stream,
        };
        let mut line = String::new();
        loop {
            line.clear();
            if r.read_line(&mut line).ok()? == 0 {
                break;
            }
            let l = line.trim_end_matches('\n');
            if l.is_empty() {
                break;
            }
            let f: Vec<&str> = l.split('\t').collect();
            match f[..] {
                ["default", v] => ctx.default = v.to_string(),
                ["lnm", ref rest @ ..] => ctx.process.extend(vmsportd::decode(rest)),
                ["symbol", ref rest @ ..] => ctx.symbols.extend(decode_symbol(rest)),
                ["spawn", _] => ctx.spawn = true,
                ["tables", n] => {
                    let mut buf = vec![0; n.parse().ok()?];
                    r.read_exact(&mut buf).ok()?;
                    ctx.tables = String::from_utf8(buf).ok()?;
                }
                ["line", v] => ctx.line = unescape(v),
                _ => {}
            }
        }
        Some(ctx)
    }

    /// LIB$GET_SYMBOL: a local symbol, else a global one.
    pub fn get_symbol(&self, name: &str) -> Option<&Symbol> {
        let name = name.to_ascii_uppercase();
        let find = |global| {
            self.symbols
                .iter()
                .find(|s| s.name == name && s.global == global)
        };
        find(false).or_else(|| find(true))
    }

    /// LIB$SET_SYMBOL: sets it here and in DCL when the image ends.
    pub fn set_symbol(&mut self, name: &str, value: Value, global: bool) {
        let s = Symbol {
            name: name.to_ascii_uppercase(),
            global,
            value,
        };
        self.symbols
            .retain(|x| !(x.name == s.name && x.global == global));
        self.symbols.push(s.clone());
        self.changes.push(Change::Set(s));
    }

    /// LIB$DELETE_SYMBOL. False if there was no such symbol.
    pub fn delete_symbol(&mut self, name: &str, global: bool) -> bool {
        let name = name.to_ascii_uppercase();
        let before = self.symbols.len();
        self.symbols
            .retain(|x| !(x.name == name && x.global == global));
        let found = self.symbols.len() != before;
        if found {
            self.changes.push(Change::Delete { name, global });
        }
        found
    }

    /// Records a process logical name change for DCL.
    pub fn changed_logical(&mut self, c: Change) {
        self.changes.push(c);
    }

    /// Tells DCL what changed and the status.
    pub fn finish(mut self, st: Cond) {
        let mut msg = String::new();
        for c in &self.changes {
            msg += &match c {
                Change::Set(s) => format!("symbol\tset\t{}\n", encode_symbol(s)),
                Change::Delete { name, global } => {
                    format!("symbol\tdel\t{}\t{}\n", scope(*global), escape(name))
                }
                Change::Define(l) => format!("lnm\tset\t{}\n", vmsportd::encode(l)),
                Change::Deassign(n) => format!("lnm\tdel\t{}\n", escape(n)),
            };
        }
        msg += &format!("status\t%X{:08X}\n", st.0);
        let _ = self.stream.write_all(msg.as_bytes());
    }
}

/// What a utility starts with: a session, its parsed command, and DCL's
/// context if DCL ran it.
pub struct Image {
    pub session: Session,
    pub command: vms_cld::ParseResult,
    pub catalog: vms_msg::Catalog,
    ctx: Option<Context>,
}

impl Image {
    /// Sets up a utility. Run by DCL, it takes DCL's context; run from a
    /// Unix shell, it parses its argv (`directory /size [.src]`) with its
    /// own CLD `cld`. A command that doesn't parse ends the program with
    /// DCL's message.
    pub fn start(cld: &str) -> Image {
        let ctx = Context::get();
        let mut session = match Session::new() {
            Ok(s) => s,
            Err(e) => {
                eprintln!("%VMSPORT-F-NODAEMON, cannot reach vmsportd: {e}");
                std::process::exit(2);
            }
        };
        let catalog = session.catalog();
        let (tables, line) = match &ctx {
            Some(c) => {
                let _ = session.set_default(&c.default);
                for l in &c.process {
                    let _ = session.define(vms_lnm::PROCESS_TABLE, l.clone());
                }
                (c.tables.clone(), c.line.clone())
            }
            None => {
                let t = vms_cld::compile(cld).expect("built-in CLD");
                (cld.to_string(), shell_line(&t))
            }
        };
        let tables = vms_cld::compile(&tables).expect("command tables");
        let command = match vms_cld::parse(&tables, &line) {
            Ok(c) => c,
            Err(e) => {
                message(&e.to_string());
                if let Some(c) = ctx {
                    c.finish(Cond(e.code.0 | 0x1000_0000));
                }
                std::process::exit(1);
            }
        };
        Image {
            session,
            command,
            catalog,
            ctx,
        }
    }

    /// `$PUTMSG`: prints the message(s), FAO arguments filled in.
    pub fn put_msg(&self, msgs: &[(Cond, Vec<vms_fao::Arg>)]) {
        message(&self.catalog.put_msg(msgs, vms_msg::Flags::ALL).join("\n"));
    }

    /// Ends the image with status `st`.
    pub fn exit(self, st: Cond) -> ! {
        if let Some(c) = self.ctx {
            c.finish(st);
        }
        std::process::exit(if st.is_success() { 0 } else { 1 });
    }
}

/// A command from a Unix shell: the table's first verb, then the argv,
/// quoted where it holds blanks (`directory /size [.src]`; a qualifier's
/// value alone: `/sign=with love` is `/sign="with love"`).
pub fn shell_line(t: &vms_cld::Tables) -> String {
    let mut line = t.verbs.first().map_or(String::new(), |v| v.name.clone());
    for a in std::env::args().skip(1) {
        line.push(' ');
        if !a.contains(char::is_whitespace) {
            line.push_str(&a);
            continue;
        }
        let quote = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
        match a.split_once('=').filter(|_| a.starts_with('/')) {
            Some((q, v)) => line.push_str(&format!("{q}={}", quote(v))),
            None => line.push_str(&quote(&a)),
        }
    }
    line
}

/// Messages go to SYS$OUTPUT, and to SYS$ERROR too when that is somewhere
/// else (as $PUTMSG does): stdout, and stderr if only stderr is a terminal.
pub fn message(text: &str) {
    use std::io::IsTerminal;
    println!("{text}");
    if !std::io::stdout().is_terminal() && std::io::stderr().is_terminal() {
        eprintln!("{text}");
    }
}
