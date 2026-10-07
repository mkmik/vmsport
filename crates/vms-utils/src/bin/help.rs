//! HELP: topics from help libraries (SYS$HELP:HELPLIB.HLP, /LIBRARY, and
//! the user libraries HLP$LIBRARY, HLP$LIBRARY_1...). At a terminal, or
//! with data lines after it in a procedure, it then asks for subtopics.

use libvms::files::Writer;
use std::io::{IsTerminal, Write};
use vms_fao::Arg;
use vms_rms::Record;
use vms_utils::Util;

fn main() {
    let mut u = Util::new(
        include_str!("../../../../sys/SYSLIB/DCLTABLES/HELP.CLD"),
        118,
    );
    let topic = u.value("TOPIC").unwrap_or_default();
    let library = u.value("LIBRARY").map(|l| l.trim_matches('"').to_string());
    let libraries = match libvms::help::libraries(&u.img.session, library.as_deref()) {
        Ok(l) => l,
        Err((why, spec)) => {
            u.msg(&[(libvms::help::OPENIN, vec![Arg::Str(&spec)]), (why, vec![])]);
            u.exit(vms_utils::inhibit(libvms::help::OPENIN));
        }
    };
    let mut file = match u.value("OUTPUT") {
        Some(spec) => {
            let s = &u.img.session;
            match s
                .parse(spec.trim_matches('"'), ".LIS", "")
                .and_then(|p| s.new_version(&p))
            {
                Ok((path, _)) => Writer::create(&path, Default::default()).ok(),
                Err(_) => None,
            }
        }
        None => None,
    };
    let width = if file.is_some() {
        80
    } else {
        libvms::help::width()
    };
    let help = vms_help::Help {
        libraries,
        width,
        instructions: u.present("INSTRUCTIONS"),
    };
    let terminal = std::io::stdin().is_terminal();
    let mut write = |lines: Vec<String>| match file.as_mut() {
        Some(f) => lines.iter().for_each(|l| {
            let _ = f.put(&Record::new(l.as_bytes().to_vec()));
        }),
        None => {
            let mut out = std::io::stdout().lock();
            lines.iter().for_each(|l| {
                let _ = writeln!(out, "{l}");
            });
            let _ = out.flush();
        }
    };
    let mut out = vms_help::Out::default();
    help.session(
        &mut out,
        &vms_help::words(&topic),
        u.present("PROMPT"),
        &mut |q, out| {
            write(out.take());
            if terminal {
                print!("{q}");
                let _ = std::io::stdout().flush();
            }
            libvms::sys::read_line()
        },
    );
    write(out.take());
    u.exit(vms_cond::Cond(1));
}
