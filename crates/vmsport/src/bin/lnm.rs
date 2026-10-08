//! lnm: logical names from a Unix shell, in the job table (a shell has no
//! process table that lasts).
//!
//!     lnm                  show the job's logical names
//!     lnm NAME             show NAME (searching LNM$DCL_LOGICAL)
//!     lnm NAME=VALUE[,..]  define NAME in the job table
//!     lnm -d NAME          deassign NAME from the job table

use vms_lnm::Logical;

fn main() {
    let mut s = match libvms::Session::new() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("%LNM-F-NODAEMON, cannot reach vmsportd: {e}");
            std::process::exit(2);
        }
    };
    let catalog = s.catalog();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let r = match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        [] => Ok(vms_lnm::show(&s.names, None, "LNM$JOB", false)),
        ["-d", name] => s
            .deassign("LNM$JOB", &name.to_ascii_uppercase())
            .map(|_| String::new()),
        [def] if def.contains('=') => {
            let (n, v) = def.split_once('=').unwrap();
            let values: Vec<&str> = v.split(',').collect();
            s.define("LNM$JOB", Logical::new(n.to_ascii_uppercase(), &values))
                .map(|_| String::new())
        }
        [name] => Ok(vms_lnm::show(
            &s.names,
            Some(&name.to_ascii_uppercase()),
            "LNM$DCL_LOGICAL",
            false,
        )),
        _ => {
            eprintln!("usage: lnm [NAME | NAME=VALUE[,...] | -d NAME]");
            std::process::exit(2);
        }
    };
    match r {
        Ok(out) => print!("{out}"),
        Err(c) => {
            eprintln!("{}", catalog.get_msg(c, vms_msg::Flags::ALL));
            std::process::exit(1);
        }
    }
}
