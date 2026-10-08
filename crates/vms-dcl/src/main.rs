//! dcl: a DCL session.
//!
//!     dcl                     interactive
//!     dcl -c 'COMMAND'        one command, then exit with its status
//!     dcl FILE.COM [P1...]    @FILE.COM
//!
//! An interactive session runs SYS$LOGIN:LOGIN.COM first. Run by DCL with
//! a context (SPAWN, PIPE), it is a subprocess.

use std::io::IsTerminal;
use vms_dcl::Dcl;
use vms_dcl::real::RealHost;

fn main() {
    let ctx = libvms::image::Context::get();
    let mut session = match libvms::Session::new() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("%DCL-F-NODAEMON, cannot reach vmsportd: {e}");
            std::process::exit(2);
        }
    };
    // A subprocess (SPAWN, PIPE) starts with its parent's default
    // directory and process logical names.
    if let Some(c) = &ctx {
        let _ = session.set_default(&c.default);
        for l in &c.process {
            let _ = session.define(vms_lnm::PROCESS_TABLE, l.clone());
        }
    }
    let mut dcl = Dcl::new(Box::new(RealHost { session }));
    if let Some(c) = ctx.filter(|c| c.spawn) {
        let st = subprocess(&mut dcl, &c);
        c.finish(st);
        std::process::exit(if st.is_success() { 0 } else { 1 });
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    let status = match args.first().map(String::as_str) {
        Some("-c") => {
            let mut st = vms_cond::Cond(1);
            for c in &args[1..] {
                st = dcl.command(c);
            }
            st
        }
        // A Unix path works too: dcl ~/bin/backup.com
        Some(file) if file.contains('/') => {
            let spec = libvms::vms_spec(std::path::Path::new(file));
            dcl.execute(&spec, &args[1..])
        }
        Some(file) => dcl.execute(file, &args[1..]),
        None => {
            login(&mut dcl);
            interactive(&mut dcl)
        }
    };
    std::process::exit(if status.is_success() { 0 } else { 1 });
}

/// A subprocess: the parent's symbols, then its command, or commands from
/// its input until it logs out.
fn subprocess(dcl: &mut Dcl, c: &libvms::image::Context) -> vms_cond::Cond {
    for s in &c.symbols {
        let v = match &s.value {
            libvms::image::Value::Int(n) => vms_dcl::expr::Value::Int(*n),
            libvms::image::Value::Str(v) => vms_dcl::expr::Value::Str(v.clone()),
        };
        if s.name == "$STATUS" {
            dcl.status = vms_cond::Cond(v.to_int() as u32);
        } else {
            dcl.set_symbol(&s.name, v, s.global);
        }
    }
    if !c.line.trim().is_empty() {
        return dcl.command(&c.line);
    }
    let st = interactive(dcl);
    let when = vms_time::asctim(libvms::sys::now(), false);
    println!(
        "  Process {} logged out at {}",
        dcl.process_name,
        &when[..20]
    );
    st
}

/// SYS$LOGIN:LOGIN.COM, if there is one, as an interactive session starts.
fn login(dcl: &mut Dcl) {
    if dcl
        .host
        .parse("SYS$LOGIN:LOGIN.COM", "", "", false, true)
        .is_some()
        && dcl.host.search("SYS$LOGIN:LOGIN.COM", 99).is_some()
    {
        dcl.execute("SYS$LOGIN:LOGIN.COM", &[]);
    }
}

fn interactive(dcl: &mut Dcl) -> vms_cond::Cond {
    let stdin = std::io::stdin();
    let tty = stdin.is_terminal() && std::io::stdout().is_terminal();
    let mut editor = vms_dcl::lineedit::Editor::default();
    let mut pending = String::new();
    loop {
        let prompt = if pending.is_empty() { "$ " } else { "_$ " };
        let line = if tty {
            match editor.read(prompt) {
                Some(l) => l,
                None => return dcl.status,
            }
        } else {
            match vms_dcl::real::read_line() {
                Some(l) => l,
                None => return dcl.status,
            }
        };
        let line = line.as_str();
        // A trailing - continues the command on the next line.
        if let Some(head) = line.trim_end().strip_suffix('-') {
            pending.push_str(head);
            continue;
        }
        pending.push_str(line);
        let cmd = std::mem::take(&mut pending);
        dcl.command(&cmd);
        if dcl.logged_out {
            return dcl.status;
        }
    }
}
