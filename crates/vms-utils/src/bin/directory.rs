//! DIRECTORY: lists files, as VMS lays the listing out (see
//! fixtures/utils/recorded/utils.log).

use libvms::files;
use std::path::PathBuf;
use vms_cond::Cond;
use vms_fao::Arg;
use vms_filespec::FileSpec;
use vms_utils::{E, Util, inhibit, shr, texts};

const DIRECT: u32 = 121;

struct Opts {
    heading: bool,
    trailing: bool,
    total: bool,
    grand: bool,
    size: Option<String>,
    date: bool,
    full: bool,
    /// /COLUMNS, if given.
    columns: Option<usize>,
}

fn main() {
    let mut u = Util::new(
        include_str!("../../../../sys/SYSLIB/DCLTABLES/DIRECTORY.CLD"),
        DIRECT,
    );
    let size = u
        .present("SIZE")
        .then(|| u.value("SIZE").unwrap_or_else(|| "USED".into()));
    let mut o = Opts {
        heading: u.present("HEADING"),
        trailing: u.present("TRAILING"),
        total: u.present("TOTAL"),
        grand: u.present("GRAND_TOTAL"),
        size,
        date: u.present("DATE"),
        full: u.present("FULL"),
        columns: u
            .value("COLUMNS")
            .and_then(|c| c.parse().ok())
            .map(|c: usize| c.max(1)),
    };
    if o.full {
        o.size = Some("ALL".into());
    }
    let versions: Option<usize> = u.value("VERSIONS").and_then(|v| v.parse().ok());
    let mut items = texts(&u.values("INPUT"));
    if items.is_empty() {
        items.push(String::new());
    }

    // Files grouped by directory, in the order found.
    let mut groups: Vec<(String, Vec<(PathBuf, FileSpec)>)> = Vec::new();
    let mut status = Cond(1);
    for item in u.expand(&items, "*.*;*") {
        let files = match item.files {
            Ok(f) => f,
            Err(e) => {
                let spec = item.spec.expanded();
                let mut m = vec![
                    (u.shared(shr::OPENIN, E), vec![Arg::Str(&spec)]),
                    (e, vec![]),
                ];
                if e == libvms::status::DNF {
                    m.push((vms_utils::NOSUCHFILE, vec![]));
                }
                u.msg(&m);
                status = inhibit(e);
                continue;
            }
        };
        let mut kept: Vec<(PathBuf, FileSpec)> = Vec::new();
        for f in files {
            let n = kept
                .iter()
                .rev()
                .take_while(|k| same_name(&k.1, &f.1))
                .count();
            if versions.is_none_or(|v| n < v) {
                kept.push(f);
            }
        }
        for f in kept {
            let dir = dir_of(&f.1);
            match groups.last_mut() {
                Some((d, list)) if *d == dir => list.push(f),
                _ => groups.push((dir, vec![f])),
            }
        }
    }
    if groups.is_empty() {
        if status == Cond(1) {
            u.msg(&[(Cond(0x0079_8008), vec![])]); // DIRECT-W-NOFILES (sys/SYSMSG/DIRECT.MSG)
            status = Cond(0x1001_8290); // RMS$_FNF as a warning, shown
        }
        u.exit(status);
    }

    let (mut files, mut used, mut alloc) = (0, 0, 0);
    let ndirs = groups.len();
    for (dir, list) in &groups {
        let (mut gu, mut ga) = (0, 0);
        let mut lines = Vec::new();
        let mut row = Row::new(&o);
        for (path, spec) in list {
            let info = files::info(path).ok();
            let (fu, fa) = info.as_ref().map_or((0, 0), |i| (i.used, i.allocated));
            gu += fu;
            ga += fa;
            if o.total || o.grand {
                continue;
            }
            let name = if o.heading {
                name_of(spec)
            } else {
                spec.expanded()
            };
            if o.full {
                if !lines.is_empty() {
                    lines.push(String::new());
                }
                lines.extend(full(&name, path, info.as_ref()));
                continue;
            }
            let mut fields = String::new();
            if let Some(s) = &o.size {
                fields += &format!("{:>12}", size_text(s, fu, fa));
            }
            if o.date {
                let t = info
                    .as_ref()
                    .map_or(String::new(), |i| vms_time::asctim(i.created, false));
                fields += &format!("  {t}");
            }
            row.add(&name, &fields, &mut lines);
        }
        row.flush(&mut lines);
        let listed = !lines.is_empty();
        files += list.len();
        used += gu;
        alloc += ga;
        if o.grand {
            continue;
        }
        if o.heading {
            println!("\nDirectory {dir}\n");
        }
        for l in lines {
            println!("{l}");
        }
        if o.trailing {
            if listed {
                println!();
            }
            println!(
                "Total of {}{}.",
                plural(list.len(), "file"),
                blocks(&o, gu, ga)
            );
        }
    }
    if o.trailing && (ndirs > 1 || o.grand) {
        println!(
            "\nGrand total of {}, {}{}.",
            plural(ndirs, "director"),
            plural(files, "file"),
            blocks(&o, used, alloc)
        );
    }
    u.exit(status);
}

/// Name and type alike (versions of one file).
fn same_name(a: &FileSpec, b: &FileSpec) -> bool {
    a.name.eq_ignore_ascii_case(&b.name)
        && a.typ
            .as_deref()
            .unwrap_or("")
            .eq_ignore_ascii_case(b.typ.as_deref().unwrap_or(""))
        && dir_of(a) == dir_of(b)
}

fn dir_of(s: &FileSpec) -> String {
    format!(
        "{}:{}",
        s.device.as_deref().unwrap_or(""),
        s.directory.clone().unwrap_or_default()
    )
}

fn name_of(s: &FileSpec) -> String {
    let v = s.version.map(|v| v.to_string()).unwrap_or_default();
    format!("{}.{};{v}", s.name, s.typ.as_deref().unwrap_or(""))
}

fn plural(n: usize, what: &str) -> String {
    match (n, what) {
        (1, "director") => "1 directory".into(),
        (_, "director") => format!("{n} directories"),
        (1, _) => format!("1 {what}"),
        _ => format!("{n} {what}s"),
    }
}

fn size_text(kind: &str, used: u64, alloc: u64) -> String {
    match kind {
        "ALL" => format!("{used}/{alloc}"),
        "ALLOCATION" => alloc.to_string(),
        _ => used.to_string(),
    }
}

fn blocks(o: &Opts, used: u64, alloc: u64) -> String {
    match &o.size {
        Some(k) => format!(
            ", {} block{}",
            size_text(k, used, alloc),
            if used == 1 && k != "ALL" { "" } else { "s" }
        ),
        None => String::new(),
    }
}

/// Lays out entries in columns: a name takes as many 20-character columns
/// as it needs. With sizes or dates, a long name gets a line of its own
/// in one column, or is cut short (`NAME|`) in several.
struct Row {
    cols: usize,
    fields: bool,
    line: String,
    col: usize,
}

impl Row {
    fn new(o: &Opts) -> Row {
        let fields = o.size.is_some() || o.date;
        // Sizes and dates list one file a line unless /COLUMNS says more.
        let cols = match o.columns {
            _ if !o.heading => 1,
            Some(c) => c,
            None if fields => 1,
            None => 4,
        };
        Row {
            cols,
            fields,
            line: String::new(),
            col: 0,
        }
    }

    fn add(&mut self, name: &str, fields: &str, lines: &mut Vec<String>) {
        if self.fields {
            let entry = if name.len() <= 19 {
                format!("{name:<19}{fields}")
            } else if self.cols == 1 {
                lines.push(name.to_string());
                format!("{:19}{fields}", "")
            } else {
                format!("{}|{fields}", &name[..18])
            };
            self.line += &entry;
            self.col += 1;
            if self.col == self.cols {
                self.flush(lines);
            } else {
                self.line += "       ";
            }
            return;
        }
        let span = (name.len() / 20 + 1).min(self.cols);
        if self.col + span > self.cols {
            self.flush(lines);
        }
        self.col += span;
        let end = self.col == self.cols;
        if name.len() <= 19 {
            self.line += &format!("{name:<19}");
            if !end {
                self.line.push(' ');
            }
        } else {
            self.line += name;
            if !end {
                let pad = 20 * self.col - self.line.len();
                self.line += &" ".repeat(pad);
            }
        }
        if end {
            self.flush(lines);
        }
    }

    fn flush(&mut self, lines: &mut Vec<String>) {
        if self.col > 0 {
            lines.push(std::mem::take(&mut self.line));
            self.col = 0;
        }
    }
}

/// DIRECTORY/FULL, as far as the host knows.
fn full(name: &str, path: &std::path::Path, info: Option<&files::Info>) -> Vec<String> {
    use std::os::unix::fs::MetadataExt;
    let Some(i) = info else {
        return vec![name.to_string()];
    };
    let m = std::fs::metadata(path).ok();
    let (ino, uid, gid, mode) = m.map_or((0, 0, 0, 0), |m| (m.ino(), m.uid(), m.gid(), m.mode()));
    let t = |v: i64| vms_time::asctim(v, false);
    let prot = |bits: u32| {
        let mut s = String::new();
        for (b, c) in [(4, 'R'), (2, 'W'), (1, 'E'), (2, 'D')] {
            if bits & b != 0 {
                s.push(c);
            }
        }
        s
    };
    let f = &i.fab;
    let org = match f.org {
        vms_rms::Org::Seq => "Sequential",
        vms_rms::Org::Rel => "Relative",
        vms_rms::Org::Idx => "Indexed",
    };
    let rfm = match f.rfm {
        vms_rms::Rfm::Udf => "Undefined".to_string(),
        vms_rms::Rfm::Fix => format!("Fixed length {} byte records", f.mrs),
        vms_rms::Rfm::Var => format!(
            "Variable length, maximum {} bytes, longest {} bytes",
            f.mrs, f.lrl
        ),
        vms_rms::Rfm::Vfc => format!(
            "VFC, {} byte header, maximum {} bytes, longest {} bytes",
            f.fsz, f.mrs, f.lrl
        ),
        vms_rms::Rfm::Stm => format!("Stream, maximum {} bytes, longest {} bytes", f.mrs, f.lrl),
        vms_rms::Rfm::Stmlf => format!(
            "Stream_LF, maximum {} bytes, longest {} bytes",
            f.mrs, f.lrl
        ),
        vms_rms::Rfm::Stmcr => format!(
            "Stream_CR, maximum {} bytes, longest {} bytes",
            f.mrs, f.lrl
        ),
    };
    let rat = if f.rat & vms_rms::rat::CR != 0 {
        "Carriage return carriage control"
    } else if f.rat & vms_rms::rat::PRN != 0 {
        "Print file carriage control"
    } else if f.rat & vms_rms::rat::FTN != 0 {
        "Fortran carriage control"
    } else {
        "None"
    };
    vec![
        format!("{:<30}File ID:  ({},1,0)", if name.len() < 30 { name.to_string() } else { format!("{name}\n") }, ino),
        format!("{:<30}Owner:    [{gid:o},{uid:o}]", format!("Size:      {:>10}", format!("{}/{}", i.used, i.allocated))),
        format!("Created:    {}", t(i.created)),
        format!("Modified:   {}", t(i.revised)),
        "Expires:    <None specified>".into(),
        "Backup:     <No backup recorded>".into(),
        "Effective:  <None specified>".into(),
        "Recording:  <None specified>".into(),
        format!("Accessed:   {}", t(i.revised)),
        format!("Attr Mod:   {}", t(i.revised)),
        format!("Data Mod:   {}", t(i.revised)),
        "Linkcount:  1".into(),
        format!("File organization:  {org}"),
        "Shelved state:      Online ".into(),
        "Caching attribute:  Writethrough".into(),
        format!("File attributes:    Allocation: {}, Extend: 0, Global buffer count: 0, No version limit", i.allocated),
        format!("Record format:      {rfm}"),
        format!("Record attributes:  {rat}"),
        "RMS attributes:     None".into(),
        "Journaling enabled: None".into(),
        format!(
            "File protection:    System:RWED, Owner:{}, Group:{}, World:{}",
            prot(mode >> 6 & 7),
            prot(mode >> 3 & 7),
            prot(mode & 7)
        ),
        "Access Cntrl List:  None".into(),
        "Client attributes:  None".into(),
    ]
    .into_iter()
    .flat_map(|l| l.split('\n').map(str::to_string).collect::<Vec<_>>())
    .collect()
}
