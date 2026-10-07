//! The host side of vmsport: logical names through vmsportd, VMS file
//! specs on the host file system (versions as `NAME.TYP;n`), sequential
//! RMS files with their attributes in the `vms.fab` extended attribute,
//! and the context channel between DCL and the images it runs.
//!
//! See docs/design/m1.md.

pub mod cli;
pub mod fileinfo;
pub mod files;
pub mod help;
pub mod image;
pub mod mount;
pub mod rms;
pub mod sys;

use std::path::{Path, PathBuf};
use vms_cond::Cond;
use vms_filespec::{Directory, FileSpec, Version};
use vms_lnm::{Logical, Mode, Names, Resolved};
use vmsportd::Client;

/// RMS and system statuses libvms returns.
pub mod status {
    use vms_cond::Cond;
    pub const NORMAL: Cond = Cond(1);
    pub const EOF: Cond = Cond(0x1827A);
    pub const FEX: Cond = Cond(0x18282);
    pub const FNF: Cond = Cond(0x18292);
    pub const PRV: Cond = Cond(0x1829A);
    pub const NMF: Cond = Cond(0x182CA);
    pub const DEV: Cond = Cond(0x184C4);
    pub const DIR: Cond = Cond(0x184CC);
    pub const SYN: Cond = Cond(0x186D4);
    pub const CRE: Cond = Cond(0x1C00A);
    pub const DNF: Cond = Cond(0x1C04A);
    pub const RER: Cond = Cond(0x1C0F2);
    pub const WER: Cond = Cond(0x1C112);
}

/// The `.MSG` source of the system messages: every file in
/// `$VMSPORT/sys/SYSMSG`, SYSMSG.MSG first.
pub fn system_messages() -> String {
    let dir = vmsport().join("sys/SYSMSG");
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("msg")))
        .collect();
    files.sort_by_key(|p| (!p.ends_with("SYSMSG.MSG"), p.clone()));
    files
        .iter()
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .collect::<Vec<_>>()
        .join("\n")
}

/// The physical device that is the host's `/`.
pub const HOST_DEVICE: &str = "HOST";

/// A process's view of the system: its logical names and default
/// directory.
pub struct Session {
    pub names: Names<Client>,
    /// Default device and directory (`SYS$DISK` and `[dir]`).
    pub default: FileSpec,
    /// `$SEARCH` contexts by stream number.
    searches: std::collections::HashMap<u32, (String, Vec<String>)>,
}

/// The job id: the Unix session, unless `VMSPORT_JOB` says otherwise.
pub fn job_id() -> u32 {
    std::env::var("VMSPORT_JOB")
        .ok()
        .and_then(|j| u32::from_str_radix(&j, 16).ok())
        // SAFETY: plain getter.
        .unwrap_or_else(|| unsafe { libc::getsid(0) } as u32)
}

/// `$VMSPORT`: the install tree (the source tree in development).
pub fn vmsport() -> PathBuf {
    vmsportd::vmsport()
}

impl Session {
    /// Connects to vmsportd (starting it if needed) and joins the job.
    /// The default directory is the host's current directory.
    pub fn new() -> std::io::Result<Session> {
        let mut client = Client::connect()?;
        let (job, group) = client.job(job_id())?;
        let cwd = std::env::current_dir()?;
        let mut s = Session {
            names: Names::new(client, &job, &group),
            default: FileSpec::default(),
            searches: Default::default(),
        };
        s.set_default(&vmsportd::host_dir(&cwd, false))
            .map_err(|_| std::io::ErrorKind::InvalidInput)?;
        Ok(s)
    }

    /// SYS$DISK:[dir] as text.
    pub fn default_directory(&self) -> String {
        let d = &self.default;
        format!(
            "{}:{}",
            d.device.as_deref().unwrap_or(""),
            d.directory.clone().unwrap_or_default()
        )
    }

    /// SET DEFAULT: a device, a directory (relative ones from the current
    /// default) or both. Doesn't check that it exists, as VMS doesn't.
    pub fn set_default(&mut self, spec: &str) -> Result<(), Cond> {
        let s: FileSpec = spec.parse().map_err(|_| status::SYN)?;
        if s.device.is_none() && s.directory.is_none() {
            return Err(status::DIR);
        }
        let base = self.default.directory.clone().unwrap_or_default();
        if let Some(d) = s.device {
            self.default.device = Some(d.to_ascii_uppercase());
        }
        if let Some(d) = s.directory {
            self.default.directory = Some(d.resolve(&base));
        }
        let disk = format!("{}:", self.default.device.as_deref().unwrap_or(""));
        let _ = self
            .names
            .define(vms_lnm::PROCESS_TABLE, Logical::new("SYS$DISK", &[&disk]));
        Ok(())
    }

    /// `$PARSE` without the existence checks: `spec` with the defaults
    /// filled in, upcased.
    pub fn parse(&self, spec: &str, default: &str, related: &str) -> Result<FileSpec, Cond> {
        let p = |s: &str| -> Result<FileSpec, Cond> {
            let f: FileSpec = s.trim().to_uppercase().parse().map_err(|_| status::SYN)?;
            Ok(self.name_as_logical(f))
        };
        let (s, d, r) = (p(spec)?, p(default)?, p(related)?);
        // With the default device comes the default directory. A device
        // named in the spec gets it only once its logical name has had its
        // say (`SYS$LOGIN:X` has a directory), in `locate`.
        let named = s.device.is_some() || d.device.is_some() || r.device.is_some();
        let current = if named {
            FileSpec::default()
        } else {
            self.default.clone()
        };
        Ok(s.merge(&[&d, &r], &current))
    }

    /// A spec that is only a name may be a logical name for a file spec
    /// (`DEFINE OUT [.LOG]RUN.TXT` then `OPEN/WRITE F OUT`), as RMS has it.
    fn name_as_logical(&self, f: FileSpec) -> FileSpec {
        let bare = f.device.is_none()
            && f.directory.is_none()
            && f.typ.is_none()
            && f.version.is_none()
            && f.node.is_none();
        if !bare || f.name.is_empty() {
            return f;
        }
        let mut cur = f;
        for _ in 0..vms_lnm::MAX_DEPTH {
            let Some(found) = self
                .names
                .translate(&cur.name, "LNM$FILE_DEV", Mode::User, false)
            else {
                break;
            };
            let Some(next) = found
                .logical
                .equivs
                .first()
                .and_then(|e| e.text.to_uppercase().parse::<FileSpec>().ok())
            else {
                break;
            };
            let again = next.device.is_none()
                && next.directory.is_none()
                && next.typ.is_none()
                && next.version.is_none();
            cur = next;
            if !again {
                break;
            }
        }
        cur
    }

    /// The process default directory.
    fn default_dir(&self) -> Directory {
        self.default.directory.clone().unwrap_or_default()
    }

    /// Where a (merged) spec is on the host: its logical names resolved, one
    /// entry per search-list value. Each gives the spec as it should be
    /// shown, and the host directory.
    pub fn locate(&self, spec: &FileSpec) -> Result<Vec<(FileSpec, PathBuf)>, Cond> {
        let base = self.default_dir();
        let mut spec = spec.clone();
        if let Some(d) = spec.directory.as_mut() {
            *d = d.resolve(&base);
        }
        let mut resolved = self.names.resolve(&spec)?;
        // No directory from the spec or its logical names: the default one.
        if resolved.iter().any(|r| r.display.directory.is_none()) {
            spec.directory = Some(base);
            resolved = self.names.resolve(&spec)?;
        }
        // Search-list values on devices that don't exist are skipped.
        let mut out = Vec::new();
        let mut err = None;
        for r in resolved {
            match host_dir(&r) {
                Ok(d) => out.push((r.display.clone(), d)),
                Err(e) => err = Some(e),
            }
        }
        match (out.is_empty(), err) {
            (true, Some(e)) => Err(e),
            _ => Ok(out),
        }
    }

    /// F$PARSE: unless `syntax_only`, the device must be known, and the
    /// directory must exist if `directory_must_exist` (VMS checks it for a
    /// whole spec, not when it is asked for one field). Returns the
    /// expanded spec.
    pub fn parse_checked(
        &self,
        spec: &str,
        default: &str,
        related: &str,
        syntax_only: bool,
        directory_must_exist: bool,
    ) -> Option<String> {
        let mut s = self.parse(spec, default, related).ok()?;
        if syntax_only {
            let base = self.default_dir();
            s.directory = Some(s.directory.map_or(base.clone(), |d| d.resolve(&base)));
            return Some(s.expanded());
        }
        let (display, dir) = self.locate(&s).ok()?.into_iter().next()?;
        (!directory_must_exist || dir.is_dir()).then(|| display.expanded())
    }

    /// The existing file `spec` names (version 0 or none: the highest; -n:
    /// counting back from it).
    pub fn find(&self, spec: &FileSpec) -> Result<(PathBuf, FileSpec), Cond> {
        let mut last = status::FNF;
        for (display, dir) in self.locate(spec)? {
            if !dir.is_dir() {
                last = status::DNF;
                continue;
            }
            let mut v = versions(&dir, &spec.name, spec.typ.as_deref().unwrap_or(""));
            if v.is_empty() {
                continue;
            }
            v.sort_by_key(|e| std::cmp::Reverse(e.0));
            let pick = match spec.version {
                None | Some(Version::Number(0)) => v.first(),
                Some(Version::Number(n)) if n < 0 => v.get((-n) as usize),
                Some(Version::Number(n)) => v.iter().find(|e| e.0 == n as u32),
                Some(Version::Wildcard) => v.first(),
            };
            if let Some((ver, host)) = pick {
                let mut shown = display.clone();
                shown.version = Some(Version::Number(*ver as i16));
                return Ok((dir.join(host), shown));
            }
        }
        Err(last)
    }

    /// The host path for a new version of `spec`: one more than the highest
    /// there is, or the version asked for.
    pub fn new_version(&self, spec: &FileSpec) -> Result<(PathBuf, FileSpec), Cond> {
        let (display, dir) = self.locate(spec)?.into_iter().next().ok_or(status::DEV)?;
        if !dir.is_dir() {
            return Err(status::DNF);
        }
        let typ = spec.typ.clone().unwrap_or_default();
        let existing = versions(&dir, &spec.name, &typ);
        let ver = match spec.version {
            Some(Version::Number(n)) if n > 0 => {
                if existing.iter().any(|e| e.0 == n as u32) {
                    return Err(status::FEX);
                }
                n as u32
            }
            _ => existing.iter().map(|e| e.0).max().unwrap_or(0) + 1,
        };
        if ver > 32767 {
            return Err(status::CRE);
        }
        // A new version is named as the file's others are (README.md;2).
        let base = match existing.iter().max() {
            Some((_, host)) => split_host(host),
            None => (
                vms_filespec::unescape(&spec.name).map_err(|_| status::SYN)?,
                vms_filespec::unescape(&typ).map_err(|_| status::SYN)?,
                0,
            ),
        };
        let name = format!("{}.{};{ver}", base.0, base.1);
        let mut shown = display;
        shown.version = Some(Version::Number(ver as i16));
        Ok((dir.join(name), shown))
    }

    /// `$SEARCH`: every file matching `spec` (wildcards in name, type and
    /// version), in VMS order, as full specs. Directories show as
    /// `NAME.DIR;1`. No version means the highest of each.
    pub fn search_all(&self, spec: &FileSpec) -> Result<Vec<(PathBuf, FileSpec)>, Cond> {
        let mut out = Vec::new();
        let mut any_dir = false;
        for (display, dir) in self.wild_dirs(spec)? {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            any_dir = true;
            let mut files: Vec<(String, String, u32, String)> = Vec::new();
            for e in entries.flatten() {
                let host = e.file_name().to_string_lossy().to_string();
                let is_dir = e.file_type().is_ok_and(|t| t.is_dir());
                let (name, typ, ver) = if is_dir {
                    (host.clone(), "DIR".to_string(), 1)
                } else {
                    split_host(&host)
                };
                let (vn, vt) = (vms_filespec::escape(&name), vms_filespec::escape(&typ));
                if wild(&vn.to_uppercase(), &spec.name.to_uppercase())
                    && wild(
                        &vt.to_uppercase(),
                        &spec.typ.clone().unwrap_or_default().to_uppercase(),
                    )
                {
                    files.push((vn, vt, ver, host));
                }
            }
            files.sort_by(|a, b| {
                (
                    a.0.to_uppercase(),
                    a.1.to_uppercase(),
                    std::cmp::Reverse(a.2),
                )
                    .cmp(&(
                        b.0.to_uppercase(),
                        b.1.to_uppercase(),
                        std::cmp::Reverse(b.2),
                    ))
            });
            // Position of each entry among its name's versions, newest first.
            let mut last: Option<(String, String)> = None;
            let mut nth = 0;
            for (n, t, v, host) in files {
                let key = (n.to_uppercase(), t.to_uppercase());
                nth = if last.as_ref() == Some(&key) {
                    nth + 1
                } else {
                    0
                };
                last = Some(key);
                let keep = match spec.version {
                    Some(Version::Wildcard) => true,
                    Some(Version::Number(x)) if x > 0 => v == x as u32,
                    Some(Version::Number(x)) if x < 0 => nth == (-x) as usize,
                    _ => nth == 0,
                };
                if keep {
                    let mut shown = display.clone();
                    (shown.name, shown.typ, shown.version) =
                        (n, Some(t), Some(Version::Number(v as i16)));
                    out.push((dir.join(host), shown));
                }
            }
        }
        if !any_dir {
            return Err(status::DNF);
        }
        Ok(out)
    }

    /// The directories a spec with directory wildcards names (`[...]`,
    /// `[*]`, `[A%.B...]`): the directory itself, then those below, depth
    /// first, sorted.
    fn wild_dirs(&self, spec: &FileSpec) -> Result<Vec<(FileSpec, PathBuf)>, Cond> {
        let Some(d) = &spec.directory else {
            return self.locate(spec);
        };
        let Some(i) = d
            .parts
            .iter()
            .position(|p| p == "..." || p.contains(['*', '%']))
        else {
            return self.locate(spec);
        };
        let mut base = spec.clone();
        if let Some(bd) = base.directory.as_mut() {
            bd.parts.truncate(i);
        }
        let mut out = Vec::new();
        for (display, path) in self.locate(&base)? {
            expand_dirs(&display, &path, &d.parts[i..], &mut out);
        }
        Ok(out)
    }

    /// F$SEARCH: the next match in stream `stream`; a different spec
    /// starts over.
    pub fn search_next(&mut self, spec: &str, stream: u32) -> Option<String> {
        let restart = self.searches.get(&stream).is_none_or(|s| s.0 != spec);
        if restart {
            let parsed = self.parse(spec, "", "").ok()?;
            let all = self
                .search_all(&parsed)
                .ok()?
                .into_iter()
                .map(|f| f.1.expanded())
                .collect::<Vec<_>>();
            self.searches
                .insert(stream, (spec.to_string(), all.into_iter().rev().collect()));
        }
        self.searches.get_mut(&stream)?.1.pop()
    }

    /// The system messages: every .MSG file in SYS$MESSAGE.
    pub fn catalog(&self) -> vms_msg::Catalog {
        let mut c = vms_msg::Catalog::default();
        if let Ok(m) = vms_msg::compile(&system_messages()) {
            c.add_system(m);
        }
        c
    }

    /// Defines a logical name; `table` is a table name or LNM$PROCESS...
    pub fn define(&mut self, table: &str, l: Logical) -> Result<Cond, Cond> {
        self.names.define(table, l)
    }

    pub fn deassign(&mut self, table: &str, name: &str) -> Result<Cond, Cond> {
        self.names.deassign(table, name, Mode::Supervisor)
    }
}

/// The host directory of a resolved spec: on `HOST:`, or on a mounted
/// image's device.
fn host_dir(r: &Resolved) -> Result<PathBuf, Cond> {
    let dev = r.physical.device.as_deref().unwrap_or("");
    let mut p = match dev.eq_ignore_ascii_case(HOST_DEVICE) {
        true => PathBuf::from("/"),
        false => mount::root(dev).ok_or(status::DEV)?,
    };
    for part in r.path() {
        let name = vms_filespec::unescape(&part).map_err(|_| status::DIR)?;
        p = case_blind(p, &name);
    }
    Ok(p)
}

/// A host path with each component found case-blind (DCL upcases
/// `X :== $/bin/ls`).
pub fn case_blind_path(p: &Path) -> PathBuf {
    let mut out = PathBuf::from("/");
    for c in p.components().skip(1) {
        out = case_blind(out, &c.as_os_str().to_string_lossy());
    }
    out
}

/// `dir/name`, or the entry of `dir` that differs from `name` only in case
/// (VMS names are case-blind; Linux file systems are not).
fn case_blind(dir: PathBuf, name: &str) -> PathBuf {
    let exact = dir.join(name);
    if exact.exists() {
        return exact;
    }
    let found = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(name));
    found.map_or(exact, |e| e.path())
}

/// The subdirectories of `path` matching `pat` (directory components,
/// `...` for any depth), appended to `out` with their specs.
fn expand_dirs(
    display: &FileSpec,
    path: &Path,
    pat: &[String],
    out: &mut Vec<(FileSpec, PathBuf)>,
) {
    let Some((head, rest)) = pat.split_first() else {
        out.push((display.clone(), path.to_path_buf()));
        return;
    };
    let below = |name: &str| {
        let mut d = display.clone();
        if let Some(dir) = d.directory.as_mut() {
            dir.parts.push(name.to_string());
        }
        d
    };
    let mut subdirs: Vec<(String, PathBuf)> = std::fs::read_dir(path)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| {
            (
                vms_filespec::escape(&e.file_name().to_string_lossy()),
                e.path(),
            )
        })
        .collect();
    subdirs.sort_by_key(|s| s.0.to_uppercase());
    if head == "..." {
        expand_dirs(display, path, rest, out);
        for (name, p) in subdirs {
            expand_dirs(&below(&name), &p, pat, out);
        }
    } else if head.contains(['*', '%']) {
        for (name, p) in subdirs
            .into_iter()
            .filter(|(n, _)| wild(&n.to_uppercase(), &head.to_uppercase()))
        {
            expand_dirs(&below(&name), &p, rest, out);
        }
    } else if let Ok(host) = vms_filespec::unescape(head) {
        let p = case_blind(path.to_path_buf(), &host);
        if p.is_dir() {
            expand_dirs(&below(head), &p, rest, out);
        }
    }
}

/// A host file name as name, type and version: `notes.txt;3`, or
/// `notes.txt` (version 1).
pub fn split_host(host: &str) -> (String, String, u32) {
    let (base, ver) = match host.rsplit_once(';') {
        Some((b, v)) if !v.is_empty() && v.bytes().all(|c| c.is_ascii_digit()) => {
            (b, v.parse().unwrap_or(1))
        }
        _ => (host, 1),
    };
    let (name, typ) = base.rsplit_once('.').unwrap_or((base, ""));
    (name.to_string(), typ.to_string(), ver)
}

/// The versions of `name.typ` (VMS syntax) in `dir`: (version, host name).
fn versions(dir: &Path, name: &str, typ: &str) -> Vec<(u32, String)> {
    let (Ok(n), Ok(t)) = (vms_filespec::unescape(name), vms_filespec::unescape(typ)) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| !t.is_dir()))
        .filter_map(|e| {
            let host = e.file_name().to_string_lossy().to_string();
            let (hn, ht, v) = split_host(&host);
            (hn.eq_ignore_ascii_case(&n) && ht.eq_ignore_ascii_case(&t)).then_some((v, host))
        })
        .collect()
}

/// `*` and `%` wildcards, the strings upcased already.
pub fn wild(s: &str, pat: &str) -> bool {
    fn m(c: &[char], p: &[char]) -> bool {
        match p.first() {
            None => c.is_empty(),
            Some('*') => (0..=c.len()).any(|i| m(&c[i..], &p[1..])),
            Some('%') => !c.is_empty() && m(&c[1..], &p[1..]),
            Some(x) => c.first() == Some(x) && m(&c[1..], &p[1..]),
        }
    }
    m(
        &s.chars().collect::<Vec<_>>(),
        &pat.chars().collect::<Vec<_>>(),
    )
}

/// The VMS spec of a host path: `HOST:[Users.mkm]NOTES.TXT;3` (a
/// directory as `HOST:[Users.mkm.src]`).
pub fn vms_spec(p: &Path) -> String {
    let abs = std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf());
    if abs.is_dir() {
        return vmsportd::host_dir(&abs, false);
    }
    let dir = vmsportd::host_dir(abs.parent().unwrap_or(Path::new("/")), false);
    let file = abs
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_default();
    let (name, typ, ver) = split_host(&file);
    format!(
        "{dir}{}.{};{ver}",
        vms_filespec::escape(&name),
        vms_filespec::escape(&typ)
    )
}

/// A directory spec for a host directory, for showing.
pub fn directory_of(path: &Path) -> Directory {
    let s: FileSpec = vmsportd::host_dir(path, false).parse().unwrap_or_default();
    s.directory.unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_names() {
        assert_eq!(split_host("NOTES.TXT;3"), ("NOTES".into(), "TXT".into(), 3));
        assert_eq!(
            split_host("archive.tar.gz"),
            ("archive.tar".into(), "gz".into(), 1)
        );
        assert_eq!(split_host("Makefile"), ("Makefile".into(), "".into(), 1));
        assert_eq!(split_host("odd;name"), ("odd;name".into(), "".into(), 1));
        assert!(wild("NOTES", "N%TE*") && !wild("NOTES", "N%T"));
    }

    #[test]
    fn directory_wildcards() {
        let root = std::env::temp_dir().join(format!("vpt-wild-{}", std::process::id()));
        for d in ["A/X", "A/Y/Z", "B"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        let shown = |pat: &[&str]| {
            let mut out = Vec::new();
            let pat: Vec<String> = pat.iter().map(|s| s.to_string()).collect();
            let top: FileSpec = "HOST:[T]".parse().unwrap();
            expand_dirs(&top, &root, &pat, &mut out);
            out.iter()
                .map(|(d, _)| d.directory.clone().unwrap().to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            shown(&["..."]),
            ["[T]", "[T.A]", "[T.A.X]", "[T.A.Y]", "[T.A.Y.Z]", "[T.B]"]
        );
        assert_eq!(shown(&["A", "*"]), ["[T.A.X]", "[T.A.Y]"]);
        assert_eq!(shown(&["%"]), ["[T.A]", "[T.B]"]);
        std::fs::remove_dir_all(root).unwrap();
    }
}
