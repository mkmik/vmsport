//! EDIT/HOST: a text file in the host's editor ($VISUAL, $EDITOR, or vi),
//! and what it leaves as a new version.

use vms_cond::Cond;
use vms_fao::Arg;
use vms_filespec::FileSpec;
use vms_rms::{Fab, Record, Rfm, rat};
use vms_utils::{E, Util, inhibit, shr};

/// Its messages are TPU's, the editor EDIT runs on VMS.
const TPU: u32 = 1010;

/// The file in the host's text editor ($VISUAL, $EDITOR, or vi), and
/// what it leaves as a new version (or /OUTPUT; nothing with /READ_ONLY,
/// or if nothing changed). A file that isn't stream_LF text is edited as
/// its records, a line each, and written in its own record format (a
/// relative or indexed file's records, as variable ones).
///
/// ponytail: no EVE or EDT of vmsport's own; the PRD's keypad editor
/// comes later.
fn main() {
    let mut u = Util::new(
        include_str!("../../../../sys/SYSLIB/DCLTABLES/EDIT.CLD"),
        TPU,
    );
    let spec = u.value("FILE").unwrap_or_default();
    let output = u.value("OUTPUT").filter(|o| !o.is_empty());
    let (read_only, create) = (u.present("READ_ONLY"), u.present("CREATE"));
    let s = &u.img.session;
    let failed = |u: Util, msg: u32, what: &str, e: Cond| -> ! {
        u.msg(&[(u.shared(msg, E), vec![Arg::Str(what)]), (e, vec![])]);
        u.exit(inhibit(e))
    };
    let parsed = match s.parse(&spec, "", "") {
        Ok(p) => p,
        Err(e) => failed(u, shr::OPENIN, &spec, e),
    };
    let found = s.find(&parsed).ok();
    if found.is_none() && !create {
        failed(u, shr::OPENIN, &parsed.expanded(), libvms::status::FNF);
    }
    let fab = found
        .as_ref()
        .map_or(Fab::default(), |(p, _)| libvms::files::fab(p));
    let plain = fab == Fab::default();
    let before = match &found {
        None => Ok(Vec::new()),
        Some((p, _)) if plain => std::fs::read(p).map_err(libvms::files::io_status),
        Some((p, _)) => libvms::files::Reader::open(p).map(|mut r| {
            std::iter::from_fn(|| r.get())
                .flat_map(|rec| [rec.data, b"\n".to_vec()].concat())
                .collect()
        }),
    };
    let before = match before {
        Ok(b) => b,
        Err(e) => failed(u, shr::OPENIN, &parsed.expanded(), e),
    };
    // Named like the file, so the editor knows what kind it is.
    let host_name = match &found {
        Some((p, _)) => libvms::split_host(&p.file_name().unwrap().to_string_lossy()),
        None => (
            parsed.name.to_lowercase(),
            parsed.typ.clone().unwrap_or_default().to_lowercase(),
            0,
        ),
    };
    let dir = std::env::temp_dir().join(format!("vmsport-edit-{}", std::process::id()));
    let tmp = dir.join(format!("{}.{}", host_name.0, host_name.1).trim_end_matches('.'));
    let edited = std::fs::create_dir_all(&dir)
        .and_then(|()| std::fs::write(&tmp, &before))
        .and_then(|()| {
            let editor = std::env::var("VISUAL")
                .or_else(|_| std::env::var("EDITOR"))
                .unwrap_or_else(|_| "vi".into());
            std::process::Command::new("sh")
                .args(["-c", &format!("{editor} \"$1\""), "sh"])
                .arg(&tmp)
                .status()
        })
        .and_then(|st| match st.success() {
            true => std::fs::read(&tmp),
            false => Err(std::io::Error::other("the editor failed")),
        });
    let _ = std::fs::remove_dir_all(&dir);
    let after = match edited {
        Ok(a) => a,
        Err(_) => failed(u, shr::OPENIN, &parsed.expanded(), Cond(0x2C)), // SS$_ABORT
    };
    // Nothing typed, nothing written, as VMS's editors do.
    if read_only || after == before && output.is_none() {
        u.exit(Cond(1));
    }
    let target = match output {
        Some(o) => s.parse(&o, "", &parsed.expanded()),
        None => Ok(parsed.clone()),
    };
    let made = target.and_then(|t| s.new_version(&FileSpec { version: None, ..t }));
    let shown = made.as_ref().map_or(parsed.expanded(), |m| m.1.expanded());
    let written = made.and_then(|(path, _)| {
        if plain {
            return std::fs::write(&path, &after).map_err(libvms::files::io_status);
        }
        let fab = match fab.org {
            vms_rms::Org::Seq => fab,
            _ => Fab {
                rfm: Rfm::Var,
                rat: rat::CR,
                ..Fab::default()
            },
        };
        let mut w = libvms::files::Writer::create(&path, fab)?;
        let text = after.strip_suffix(b"\n").unwrap_or(&after);
        text.split(|b| *b == b'\n')
            .try_for_each(|line| w.put(&Record::new(line.to_vec())))
    });
    if let Err(e) = written {
        failed(u, shr::OPENOUT, &shown, e);
    }
    u.exit(Cond(1));
}
