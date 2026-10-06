//! Running images: DCL hands an image its context (default directory,
//! process logical names, command tables and command line) on an inherited
//! socket, `VMSPORT_CONTEXT=<fd>`, and gets its 32-bit status back the
//! same way. See docs/design/m1.md.

use crate::{Session, status};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use vms_cond::Cond;
use vms_lnm::Logical;

/// What an image gets from DCL.
pub struct Context {
    pub default: String,
    pub process: Vec<Logical>,
    /// CLD text: the verb, its syntaxes and types.
    pub tables: String,
    pub line: String,
    stream: UnixStream,
}

/// Without a status line, a Unix exit code n stands for this plus n*8: an
/// error in the C run-time's facility, already shown.
const C_EXIT: u32 = 0x1035_A002;

/// Runs `program` as an image with `ctx`'s context and returns its status.
/// `stdout`, if given, is where its output goes (SYS$OUTPUT redirected).
pub fn run(
    program: &Path,
    default: &str,
    process: &[Logical],
    tables: &str,
    line: &str,
    stdout: Option<std::fs::File>,
) -> Cond {
    let Ok((ours, theirs)) = UnixStream::pair() else {
        return Cond(0x2C);
    };
    let fd = theirs.as_raw_fd();
    let mut cmd = Command::new(program);
    cmd.env("VMSPORT_CONTEXT", fd.to_string())
        .env("VMSPORT_JOB", format!("{:X}", crate::job_id()));
    if let Some(out) = stdout {
        cmd.stdout(Stdio::from(out));
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
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(_) => return status::FNF,
    };
    drop(theirs);
    let mut msg = format!("vmsport-context\t1\ndefault\t{default}\n");
    for l in process {
        msg += &format!("lnm\t{}\n", vmsportd::encode(l));
    }
    msg += &format!("tables\t{}\n{tables}line\t{line}\n\n", tables.len());
    let mut writer = ours.try_clone().expect("socket");
    // A writer thread, so an image that never reads can't block us.
    let w = std::thread::spawn(move || {
        let _ = writer.write_all(msg.as_bytes());
    });
    let exit = child.wait();
    let _ = w.join();
    let _ = ours.shutdown(std::net::Shutdown::Write);
    let mut reply = String::new();
    let _ = BufReader::new(&ours).read_to_string(&mut reply);
    if let Some(st) = reply.lines().find_map(|l| l.strip_prefix("status\t")) {
        return Cond(u32::from_str_radix(st.trim_start_matches("%X"), 16).unwrap_or(0x2C));
    }
    match exit.ok().and_then(|e| e.code()) {
        Some(0) => status::NORMAL,
        Some(n) => Cond(C_EXIT + (n as u32) * 8),
        None => Cond(0x2C), // killed: SS$_ABORT
    }
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
            tables: String::new(),
            line: String::new(),
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
            let (k, v) = l.split_once('\t').unwrap_or((l, ""));
            match k {
                "default" => ctx.default = v.to_string(),
                "lnm" => ctx
                    .process
                    .extend(vmsportd::decode(&v.split('\t').collect::<Vec<_>>())),
                "tables" => {
                    let mut buf = vec![0; v.parse().ok()?];
                    r.read_exact(&mut buf).ok()?;
                    ctx.tables = String::from_utf8(buf).ok()?;
                }
                "line" => ctx.line = v.to_string(),
                _ => {}
            }
        }
        Some(ctx)
    }

    /// Tells DCL the image's status.
    pub fn finish(mut self, st: Cond) {
        let _ = writeln!(self.stream, "status\t%X{:08X}", st.0);
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
                let mut line = t.verbs[0].name.clone();
                for a in std::env::args().skip(1) {
                    line.push(' ');
                    if a.contains(char::is_whitespace) {
                        line.push_str(&format!("\"{}\"", a.replace('"', "\"\"")));
                    } else {
                        line.push_str(&a);
                    }
                }
                (cld.to_string(), line)
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

/// Messages go to SYS$OUTPUT, and to SYS$ERROR too when that is somewhere
/// else (as $PUTMSG does): stdout, and stderr if only stderr is a terminal.
pub fn message(text: &str) {
    use std::io::IsTerminal;
    println!("{text}");
    if !std::io::stdout().is_terminal() && std::io::stderr().is_terminal() {
        eprintln!("{text}");
    }
}
