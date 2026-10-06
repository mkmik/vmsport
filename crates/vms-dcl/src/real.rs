//! DCL's host: libvms.

use crate::host::{Change, Child, Host, Launch, Mode, RecordFile, Table};
use libvms::Session;
use libvms::files::{Reader, Writer};
use std::io::{BufRead, Write};
use vms_cond::Cond;
use vms_lnm::{Equiv, Logical};

pub struct RealHost {
    pub session: Session,
}

enum Io {
    Read(Reader),
    Write(Writer),
}

impl RecordFile for Io {
    fn read(&mut self) -> Result<Option<String>, Cond> {
        match self {
            Io::Read(r) => Ok(r
                .get()
                .map(|rec| String::from_utf8_lossy(&rec.data).into_owned())),
            Io::Write(_) => Err(Cond(0x1C0F2)),
        }
    }

    fn write(&mut self, record: &str) -> Result<(), Cond> {
        match self {
            Io::Write(w) => w.put(&vms_rms::Record::new(record.as_bytes().to_vec())),
            Io::Read(_) => Err(Cond(0x1C112)),
        }
    }

    fn host_file(&self) -> Option<std::fs::File> {
        match self {
            Io::Write(w) => w.file().try_clone().ok(),
            Io::Read(_) => None,
        }
    }
}

struct Terminal;

struct Stderr;

impl RecordFile for Stderr {
    fn read(&mut self) -> Result<Option<String>, Cond> {
        Ok(None)
    }

    fn write(&mut self, record: &str) -> Result<(), Cond> {
        eprintln!("{record}");
        Ok(())
    }
}

impl RecordFile for Terminal {
    fn read(&mut self) -> Result<Option<String>, Cond> {
        Ok(None)
    }

    fn write(&mut self, record: &str) -> Result<(), Cond> {
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "{record}");
        let _ = out.flush();
        Ok(())
    }
}

fn table_name(t: &Table) -> String {
    match t {
        Table::Process => "LNM$PROCESS".into(),
        Table::Job => "LNM$JOB".into(),
        Table::Group => "LNM$GROUP".into(),
        Table::System => "LNM$SYSTEM".into(),
        Table::Named(n) => n.clone(),
    }
}

/// `SYS$SYSTEM:DIRECTORY.EXE` on the host: the file if it is there,
/// otherwise `directory` next to this program (the install's bin, or the
/// cargo target directory).
fn image_path(s: &Session, image: &str) -> Option<std::path::PathBuf> {
    let spec = s.parse(image, ".EXE", "").ok()?;
    if let Ok((p, _)) = s.find(&spec) {
        return Some(p);
    }
    let bin = std::env::var_os("VMSPORT_BIN")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::current_exe()
                .ok()?
                .parent()
                .map(|p| p.to_path_buf())
        })?;
    let p = bin.join(spec.name.to_ascii_lowercase());
    p.exists().then_some(p)
}

impl Host for RealHost {
    fn dcl_tables(&mut self) -> String {
        let dir = libvms::vmsport().join("sys/SYSLIB/DCLTABLES");
        let mut files: Vec<_> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .collect();
        files.sort();
        files
            .iter()
            .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("cld")))
            .filter_map(|p| std::fs::read_to_string(p).ok())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn system_messages(&mut self) -> String {
        libvms::system_messages()
    }

    fn open(
        &mut self,
        spec: &str,
        default: &str,
        mode: Mode,
    ) -> Result<(Box<dyn RecordFile>, String), Cond> {
        let s = &self.session;
        let parsed = s.parse(spec, default, "")?;
        let (io, shown) = match mode {
            Mode::Read | Mode::ReadWrite => {
                let (p, shown) = s.find(&parsed)?;
                (Io::Read(Reader::open(&p)?), shown)
            }
            Mode::Write => {
                let (p, shown) = s.new_version(&parsed)?;
                (Io::Write(Writer::create(&p, Default::default())?), shown)
            }
            Mode::Append => {
                let (p, shown) = s.find(&parsed)?;
                (Io::Write(Writer::append(&p)?), shown)
            }
        };
        Ok((Box::new(io), shown.expanded()))
    }

    fn terminal_output(&mut self) -> Box<dyn RecordFile> {
        Box::new(Terminal)
    }

    fn error_output(&mut self) -> (Box<dyn RecordFile>, bool) {
        use std::os::unix::fs::MetadataExt;
        let id = |fd: i32| {
            // SAFETY: borrows a descriptor we don't close.
            let f = std::mem::ManuallyDrop::new(unsafe {
                <std::fs::File as std::os::fd::FromRawFd>::from_raw_fd(fd)
            });
            f.metadata().ok().map(|m| (m.dev(), m.ino(), m.rdev()))
        };
        let same = id(1).is_some() && id(1) == id(2);
        (Box::new(Stderr), same)
    }

    fn input_file(&mut self, spec: &str) -> Result<std::fs::File, Cond> {
        let parsed = self.session.parse(spec, "", "")?;
        let (p, _) = self.session.find(&parsed)?;
        std::fs::File::open(p).map_err(libvms::files::io_status)
    }

    fn read_terminal(&mut self, prompt: &str) -> Option<String> {
        print!("{prompt}");
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        match std::io::stdin().lock().read_line(&mut line) {
            Ok(0) | Err(_) => None,
            Ok(_) => Some(line.trim_end_matches(['\n', '\r']).to_string()),
        }
    }

    fn parse(
        &mut self,
        spec: &str,
        default: &str,
        related: &str,
        syntax_only: bool,
        directory_must_exist: bool,
    ) -> Option<String> {
        self.session
            .parse_checked(spec, default, related, syntax_only, directory_must_exist)
    }

    fn search(&mut self, spec: &str, stream: u32) -> Option<String> {
        self.session.search_next(spec, stream)
    }

    fn default_directory(&mut self) -> String {
        self.session.default_directory()
    }

    fn set_default(&mut self, spec: &str) -> Result<(), Cond> {
        self.session.set_default(spec)
    }

    fn trnlnm(&mut self, name: &str, table: &str, index: u32, item: &str) -> Option<String> {
        let found = self
            .session
            .names
            .translate(name, table, vms_lnm::Mode::User, true);
        vms_lnm::item(found.as_ref(), index as usize, item)
    }

    fn define(
        &mut self,
        name: &str,
        equivs: &[String],
        table: &Table,
        attrs: &[String],
    ) -> Result<Cond, Cond> {
        let has = |k: &str| attrs.iter().any(|a| k.starts_with(a.as_str()));
        let (concealed, terminal) = (has("CONCEALED"), has("TERMINAL"));
        let l = Logical {
            equivs: equivs
                .iter()
                .map(|e| Equiv {
                    concealed,
                    terminal,
                    ..Equiv::new(e.as_str())
                })
                .collect(),
            ..Logical::new(name, &[])
        };
        self.session.define(&table_name(table), l)
    }

    fn deassign(&mut self, name: Option<&str>, table: &Table) -> Result<(), Cond> {
        let t = table_name(table);
        let names: Vec<String> = match name {
            Some(n) => vec![n.to_string()],
            None => {
                let real = self.session.names.tables(&t, vms_lnm::Mode::User);
                real.first()
                    .and_then(|r| self.session.names.get(r))
                    .map(|tb| tb.logicals.iter().map(|l| l.name.clone()).collect())
                    .unwrap_or_default()
            }
        };
        for n in names {
            self.session.deassign(&t, &n)?;
        }
        Ok(())
    }

    fn show_logical(
        &mut self,
        names: &[String],
        tables: &[Table],
        full: bool,
    ) -> Result<Vec<String>, Cond> {
        let tables: Vec<String> = if tables.is_empty() {
            vec!["LNM$DCL_LOGICAL".into()]
        } else {
            tables.iter().map(table_name).collect()
        };
        let mut out = Vec::new();
        let names: Vec<Option<&str>> = if names.is_empty() {
            vec![None]
        } else {
            names.iter().map(|n| Some(n.as_str())).collect()
        };
        for n in names {
            for t in &tables {
                out.extend(
                    vms_lnm::show(&self.session.names, n, t, full)
                        .lines()
                        .map(str::to_string),
                );
            }
        }
        Ok(out)
    }

    fn start(&mut self, what: Child, launch: Launch) -> Result<libvms::image::Child, Cond> {
        let program = match what {
            Child::Image(spec) => image_path(&self.session, spec).ok_or(libvms::status::FNF)?,
            // A foreign command names a host program by path, or by VMS spec.
            Child::Foreign(p) if p.starts_with('/') => std::path::PathBuf::from(p),
            Child::Foreign(spec) => image_path(&self.session, spec)
                .or_else(|| {
                    let p = std::path::PathBuf::from(spec);
                    p.exists().then_some(p)
                })
                .ok_or(libvms::status::FNF)?,
            Child::Dcl => std::env::current_exe().map_err(|_| libvms::status::FNF)?,
        };
        let default = self.session.default_directory();
        let process: Vec<Logical> = self
            .session
            .names
            .get(vms_lnm::PROCESS_TABLE)
            .map(|t| t.logicals.clone())
            .unwrap_or_default();
        // /NOLOGICAL_NAMES passes an empty table: keep it empty.
        let process: &[Logical] = if launch.no_logicals { &[] } else { &process };
        let launch = Launch {
            default: &default,
            process,
            ..launch
        };
        libvms::image::start(&program, launch)
    }

    fn apply(&mut self, changes: &[Change]) {
        for c in changes {
            match c {
                Change::Define(l) => {
                    let _ = self.session.define(vms_lnm::PROCESS_TABLE, l.clone());
                }
                Change::Deassign(n) => {
                    let _ = self.session.deassign(vms_lnm::PROCESS_TABLE, n);
                }
                _ => {}
            }
        }
    }

    fn dcl_path(&mut self, verb: &str) -> Option<(String, bool)> {
        let found =
            self.session
                .names
                .translate("DCL$PATH", "LNM$FILE_DEV", vms_lnm::Mode::User, false)?;
        // Each value is a place to look: a procedure first, then a program.
        for e in &found.logical.equivs {
            for (typ, procedure) in [("COM", true), ("EXE", false), ("", false)] {
                let spec = format!("{}{verb}.{typ}", e.text);
                let Ok(parsed) = self.session.parse(&spec, "", "") else {
                    continue;
                };
                if let Ok((path, shown)) = self.session.find(&parsed) {
                    let runnable = procedure
                        || std::fs::metadata(&path).is_ok_and(|m| {
                            use std::os::unix::fs::PermissionsExt;
                            m.is_file() && m.permissions().mode() & 0o111 != 0
                        });
                    if runnable {
                        let what = if procedure {
                            shown.expanded()
                        } else {
                            path.display().to_string()
                        };
                        return Some((what, procedure));
                    }
                }
            }
        }
        None
    }

    fn now(&mut self) -> i64 {
        libvms::sys::now()
    }

    fn info(&mut self, item: &str) -> Option<String> {
        libvms::sys::info(item)
    }
}
