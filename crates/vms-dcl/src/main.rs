//! dcl: a DCL session.
//!
//!     dcl                     interactive
//!     dcl -c 'COMMAND'        one command, then exit with its status
//!     dcl FILE.COM [P1...]    @FILE.COM

use std::io::{BufRead, IsTerminal, Write};
use vms_dcl::Dcl;
use vms_dcl::real::RealHost;

fn main() {
    let session = match libvms::Session::new() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("%DCL-F-NODAEMON, cannot reach vmsportd: {e}");
            std::process::exit(2);
        }
    };
    let mut dcl = Dcl::new(Box::new(RealHost { session }));
    let args: Vec<String> = std::env::args().skip(1).collect();
    let status = match args.first().map(String::as_str) {
        Some("-c") => {
            let mut st = vms_cond::Cond(1);
            for c in &args[1..] {
                st = dcl.command(c);
            }
            st
        }
        Some(file) => dcl.execute(file, &args[1..]),
        None => interactive(&mut dcl),
    };
    std::process::exit(if status.is_success() { 0 } else { 1 });
}

fn interactive(dcl: &mut Dcl) -> vms_cond::Cond {
    let stdin = std::io::stdin();
    let tty = stdin.is_terminal();
    let mut pending = String::new();
    loop {
        if tty {
            print!("{}", if pending.is_empty() { "$ " } else { "_$ " });
            let _ = std::io::stdout().flush();
        }
        let mut line = String::new();
        if stdin.lock().read_line(&mut line).unwrap_or(0) == 0 {
            if tty {
                println!();
            }
            return dcl.status;
        }
        let line = line.trim_end_matches(['\n', '\r']);
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
