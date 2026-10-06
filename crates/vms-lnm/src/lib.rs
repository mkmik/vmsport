//! Logical names: tables, `$CRELNM` / `$TRNLNM` / `$DELLNM`, table search
//! lists, the iterative translation file specs go through, and what
//! SHOW LOGICAL and F$TRNLNM print.
//!
//! A process sees its own tables (the process directory and table, and
//! tables it creates) and the shared ones (job, group, system), which live
//! in vmsportd; [`Shared`] is how they are reached. Formatting follows what
//! VMS does, see fixtures/lnm/recorded.

mod show;
mod spec;

pub use show::{item, show};
pub use spec::{Resolved, resolve};

use std::borrow::Cow;
use vms_cond::Cond;

pub const SS_NORMAL: Cond = Cond(1);
pub const SS_SUPERSEDE: Cond = Cond(0x631);
pub const SS_NOLOGNAM: Cond = Cond(0x1BC);
pub const SS_IVLOGNAM: Cond = Cond(0x154);
pub const SS_TOOMANYLNAM: Cond = Cond(0x374);
pub const SS_NOLOGTAB: Cond = Cond(0x2294);

pub const PROCESS_DIRECTORY: &str = "LNM$PROCESS_DIRECTORY";
pub const SYSTEM_DIRECTORY: &str = "LNM$SYSTEM_DIRECTORY";
pub const PROCESS_TABLE: &str = "LNM$PROCESS_TABLE";
pub const SYSTEM_TABLE: &str = "LNM$SYSTEM_TABLE";

/// Iterative translation stops after this many levels (`SS$_TOOMANYLNAM`).
pub const MAX_DEPTH: usize = 10;

/// Access mode, kept as data: nothing is protected by it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Mode {
    Kernel,
    Executive,
    Supervisor,
    User,
}

impl Mode {
    /// As SHOW LOGICAL/FULL shows it: `[super]`.
    pub fn short(self) -> &'static str {
        ["kernel", "exec", "super", "user"][self as usize]
    }

    /// As F$TRNLNM's ACCESS_MODE item gives it.
    pub fn long(self) -> &'static str {
        ["KERNEL", "EXECUTIVE", "SUPERVISOR", "USER"][self as usize]
    }

    /// `KERNEL`, `EXEC[UTIVE]`, `SUPER[VISOR]`, `USER` (any case).
    pub fn parse(s: &str) -> Option<Mode> {
        let s = s.to_ascii_uppercase();
        [Mode::Kernel, Mode::Executive, Mode::Supervisor, Mode::User]
            .into_iter()
            .find(|m| {
                s.len() >= 4 && m.long().starts_with(&s) || s == m.short().to_ascii_uppercase()
            })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Equiv {
    pub text: String,
    pub concealed: bool,
    pub terminal: bool,
}

impl Equiv {
    pub fn new(text: impl Into<String>) -> Equiv {
        Equiv {
            text: text.into(),
            concealed: false,
            terminal: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Logical {
    pub name: String,
    pub mode: Mode,
    pub no_alias: bool,
    pub confine: bool,
    /// A table itself, as listed in a directory table.
    pub table: bool,
    pub equivs: Vec<Equiv>,
}

impl Logical {
    /// A supervisor-mode name with these equivalences.
    pub fn new(name: impl Into<String>, equivs: &[&str]) -> Logical {
        Logical {
            name: name.into(),
            mode: Mode::Supervisor,
            no_alias: false,
            confine: false,
            table: false,
            equivs: equivs.iter().map(|e| Equiv::new(*e)).collect(),
        }
    }

    /// The directory entry for a table.
    pub fn table(name: impl Into<String>, mode: Mode) -> Logical {
        Logical {
            mode,
            table: true,
            ..Logical::new(name, &[])
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Table {
    pub name: String,
    pub logicals: Vec<Logical>,
}

impl Table {
    pub fn new(name: impl Into<String>) -> Table {
        Table {
            name: name.into(),
            logicals: Vec::new(),
        }
    }

    /// `name` at `mode` or a more privileged one; the least privileged of
    /// those wins.
    pub fn find(&self, name: &str, mode: Mode, case_blind: bool) -> Option<&Logical> {
        self.logicals
            .iter()
            .filter(|l| {
                l.mode <= mode
                    && if case_blind {
                        l.name.eq_ignore_ascii_case(name)
                    } else {
                        l.name == name
                    }
            })
            .max_by_key(|l| l.mode)
    }

    /// `SS$_SUPERSEDE` if it replaced a name at the same mode.
    pub fn define(&mut self, l: Logical) -> Cond {
        match self
            .logicals
            .iter_mut()
            .find(|x| x.name == l.name && x.mode == l.mode)
        {
            Some(x) => {
                *x = l;
                SS_SUPERSEDE
            }
            None => {
                self.logicals.push(l);
                SS_NORMAL
            }
        }
    }

    /// Deletes `name` at `mode` and the less privileged modes.
    pub fn deassign(&mut self, name: &str, mode: Mode) -> Cond {
        let n = self.logicals.len();
        self.logicals
            .retain(|l| !(l.name == name && l.mode >= mode));
        if self.logicals.len() < n {
            SS_NORMAL
        } else {
            SS_NOLOGNAM
        }
    }
}

/// The tables that live outside the process: in vmsportd, or in memory.
pub trait Shared {
    fn table(&self, name: &str) -> Option<Table>;
    fn define(&mut self, table: &str, l: Logical) -> Result<Cond, Cond>;
    fn deassign(&mut self, table: &str, name: &str, mode: Mode) -> Result<Cond, Cond>;
}

/// In-memory shared tables: what vmsportd keeps, and what tests use.
impl Shared for Vec<Table> {
    fn table(&self, name: &str) -> Option<Table> {
        self.iter().find(|t| t.name == name).cloned()
    }

    fn define(&mut self, table: &str, l: Logical) -> Result<Cond, Cond> {
        Ok(self
            .iter_mut()
            .find(|t| t.name == table)
            .ok_or(SS_NOLOGTAB)?
            .define(l))
    }

    fn deassign(&mut self, table: &str, name: &str, mode: Mode) -> Result<Cond, Cond> {
        let c = self
            .iter_mut()
            .find(|t| t.name == table)
            .ok_or(SS_NOLOGTAB)?
            .deassign(name, mode);
        if c == SS_NOLOGNAM { Err(c) } else { Ok(c) }
    }
}

/// Job table name for a job id.
pub fn job_table(id: u32) -> String {
    format!("LNM$JOB_{id:08X}")
}

/// Group table name for a UIC group (shown in octal).
pub fn group_table(group: u32) -> String {
    format!("LNM$GROUP_{group:06o}")
}

/// A kernel-mode, no-alias name, as the system makes its own.
fn system(l: Logical) -> Logical {
    Logical {
        mode: Mode::Kernel,
        no_alias: true,
        ..l
    }
}

fn terminal(text: &str) -> Equiv {
    Equiv {
        terminal: true,
        ..Equiv::new(text)
    }
}

/// The system directory and tables vmsportd starts with; the caller adds
/// its own names to the system table.
pub fn system_tables() -> Vec<Table> {
    let mut dir = Table::new(SYSTEM_DIRECTORY);
    for t in [SYSTEM_DIRECTORY, SYSTEM_TABLE, "LNM$SYSCLUSTER_TABLE"] {
        dir.define(system(Logical::table(t, Mode::Kernel)));
    }
    dir.define(system(Logical {
        equivs: vec![terminal(SYSTEM_TABLE), Equiv::new("LNM$SYSCLUSTER")],
        ..Logical::new("LNM$SYSTEM", &[])
    }));
    dir.define(system(Logical {
        equivs: vec![terminal("LNM$SYSCLUSTER_TABLE")],
        ..Logical::new("LNM$SYSCLUSTER", &[])
    }));
    dir.define(Logical::new(
        "LNM$FILE_DEV",
        &["LNM$PROCESS", "LNM$JOB", "LNM$GROUP", "LNM$SYSTEM"],
    ));
    dir.define(Logical::new("LNM$DCL_LOGICAL", &["LNM$FILE_DEV"]));
    vec![
        dir,
        Table::new(SYSTEM_TABLE),
        Table::new("LNM$SYSCLUSTER_TABLE"),
    ]
}

/// Adds a job or group table to the shared tables, if it isn't there.
pub fn add_table(shared: &mut Vec<Table>, name: &str) {
    if !shared.iter().any(|t| t.name == name) {
        shared[0].define(system(Logical::table(name, Mode::Kernel)));
        shared.push(Table::new(name));
    }
}

/// What `$TRNLNM` found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub table: String,
    pub logical: Logical,
}

/// A process's view of logical names.
pub struct Names<S: Shared> {
    /// The process directory, process table, and tables the process made.
    pub process: Vec<Table>,
    pub shared: S,
}

impl<S: Shared> Names<S> {
    /// A process in job table `job` and group table `group` (names as
    /// [`job_table`] and [`group_table`] make them).
    pub fn new(shared: S, job: &str, group: &str) -> Names<S> {
        let mut dir = Table::new(PROCESS_DIRECTORY);
        for t in [PROCESS_DIRECTORY, PROCESS_TABLE] {
            dir.define(system(Logical::table(t, Mode::Kernel)));
        }
        for (name, table) in [
            ("LNM$PROCESS", PROCESS_TABLE),
            ("LNM$JOB", job),
            ("LNM$GROUP", group),
        ] {
            dir.define(Logical {
                mode: Mode::Kernel,
                equivs: vec![terminal(table)],
                ..Logical::new(name, &[])
            });
        }
        Names {
            process: vec![dir, Table::new(PROCESS_TABLE)],
            shared,
        }
    }

    /// A table by its real name.
    pub fn get(&self, table: &str) -> Option<Cow<'_, Table>> {
        match self.process.iter().find(|t| t.name == table) {
            Some(t) => Some(Cow::Borrowed(t)),
            None => self.shared.table(table).map(Cow::Owned),
        }
    }

    /// The real tables a table name stands for: itself, or what it
    /// translates to in the directories, as a search list.
    // ponytail: fetches the system directory on every step; cache per call
    // if the daemon round trips ever show up in a profile.
    /// Directory entries are looked up at `mode`: at EXECUTIVE,
    /// LNM$DCL_LOGICAL (a supervisor-mode name) stands for no table at all.
    pub fn tables(&self, name: &str, mode: Mode) -> Vec<String> {
        let mut out = Vec::new();
        self.expand(name, mode, 0, &mut out);
        out
    }

    fn expand(&self, name: &str, mode: Mode, depth: usize, out: &mut Vec<String>) {
        if depth > MAX_DEPTH {
            return;
        }
        for dir in [PROCESS_DIRECTORY, SYSTEM_DIRECTORY] {
            let Some(dir) = self.get(dir) else { continue };
            if let Some(l) = dir.find(name, mode, false) {
                if l.table {
                    out.push(l.name.clone());
                } else {
                    for e in l.equivs.clone() {
                        self.expand(&e.text, mode, depth + 1, out);
                    }
                }
                return;
            }
        }
    }

    /// `$TRNLNM`: the first table of `table`'s search list that has `name`.
    pub fn translate(
        &self,
        name: &str,
        table: &str,
        mode: Mode,
        case_blind: bool,
    ) -> Option<Found> {
        self.tables(table, mode).into_iter().find_map(|t| {
            let l = self.get(&t)?.find(name, mode, case_blind)?.clone();
            Some(Found {
                table: t,
                logical: l,
            })
        })
    }

    /// `$CRELNM` into the first table `table` stands for.
    pub fn define(&mut self, table: &str, l: Logical) -> Result<Cond, Cond> {
        let t = self
            .tables(table, Mode::User)
            .into_iter()
            .next()
            .ok_or(SS_NOLOGTAB)?;
        match self.process.iter_mut().find(|x| x.name == t) {
            Some(x) => Ok(x.define(l)),
            None => self.shared.define(&t, l),
        }
    }

    /// `$DELLNM` from the first table `table` stands for.
    pub fn deassign(&mut self, table: &str, name: &str, mode: Mode) -> Result<Cond, Cond> {
        let t = self
            .tables(table, Mode::User)
            .into_iter()
            .next()
            .ok_or(SS_NOLOGTAB)?;
        match self.process.iter_mut().find(|x| x.name == t) {
            Some(x) => match x.deassign(name, mode) {
                SS_NOLOGNAM => Err(SS_NOLOGNAM),
                c => Ok(c),
            },
            None => self.shared.deassign(&t, name, mode),
        }
    }

    /// The equivalence of `name` in LNM$FILE_DEV, for file specs.
    pub fn resolve(&self, spec: &vms_filespec::FileSpec) -> Result<Vec<Resolved>, Cond> {
        resolve(spec, |n| {
            self.translate(n, "LNM$FILE_DEV", Mode::User, false)
                .map(|f| f.logical)
        })
    }

    /// CREATE/NAME_TABLE: a new process-private table.
    pub fn create_table(&mut self, name: &str, mode: Mode) -> Cond {
        if self.process.iter().any(|t| t.name == name) {
            return SS_SUPERSEDE;
        }
        self.process[0].define(Logical::table(name, mode));
        self.process.push(Table::new(name));
        SS_NORMAL
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn names() -> Names<Vec<Table>> {
        let mut shared = system_tables();
        let (job, group) = (job_table(0x42), group_table(1));
        add_table(&mut shared, &job);
        add_table(&mut shared, &group);
        Names::new(shared, &job, &group)
    }

    #[test]
    fn file_dev_search_list() {
        let n = names();
        let all = [
            "LNM$PROCESS_TABLE",
            "LNM$JOB_00000042",
            "LNM$GROUP_000001",
            "LNM$SYSTEM_TABLE",
            "LNM$SYSCLUSTER_TABLE",
        ];
        assert_eq!(n.tables("LNM$FILE_DEV", Mode::User), all);
        assert_eq!(n.tables("LNM$DCL_LOGICAL", Mode::User), all);
        assert!(n.tables("LNM$DCL_LOGICAL", Mode::Executive).is_empty());
        assert!(n.tables("NOSUCH", Mode::User).is_empty());
    }

    #[test]
    fn define_translate_deassign() {
        let mut n = names();
        assert_eq!(
            n.define("LNM$SYSTEM", Logical::new("X", &["sys"])),
            Ok(SS_NORMAL)
        );
        assert_eq!(
            n.define("LNM$PROCESS", Logical::new("X", &["proc"])),
            Ok(SS_NORMAL)
        );
        assert_eq!(
            n.define("LNM$PROCESS", Logical::new("X", &["proc2"])),
            Ok(SS_SUPERSEDE)
        );
        let f = n.translate("X", "LNM$FILE_DEV", Mode::User, false).unwrap();
        assert_eq!(
            (f.table.as_str(), f.logical.equivs[0].text.as_str()),
            (PROCESS_TABLE, "proc2")
        );
        assert!(
            n.translate("x", "LNM$FILE_DEV", Mode::User, false)
                .is_none()
        );
        assert!(n.translate("x", "LNM$FILE_DEV", Mode::User, true).is_some());
        assert!(
            n.translate("X", PROCESS_TABLE, Mode::Executive, false)
                .is_none()
        );
        assert_eq!(
            n.deassign("LNM$PROCESS", "X", Mode::Supervisor),
            Ok(SS_NORMAL)
        );
        assert_eq!(
            n.deassign("LNM$PROCESS", "X", Mode::Supervisor),
            Err(SS_NOLOGNAM)
        );
        assert_eq!(
            n.translate("X", "LNM$FILE_DEV", Mode::User, false)
                .unwrap()
                .table,
            SYSTEM_TABLE
        );
    }

    #[test]
    fn modes() {
        let mut t = Table::new("T");
        t.define(Logical {
            mode: Mode::Executive,
            ..Logical::new("X", &["exec"])
        });
        t.define(Logical {
            mode: Mode::User,
            ..Logical::new("X", &["user"])
        });
        assert_eq!(
            t.find("X", Mode::User, false).unwrap().equivs[0].text,
            "user"
        );
        assert_eq!(
            t.find("X", Mode::Supervisor, false).unwrap().equivs[0].text,
            "exec"
        );
        assert!(t.find("X", Mode::Kernel, false).is_none());
        assert_eq!(Mode::parse("exec"), Some(Mode::Executive));
        assert_eq!(Mode::parse("SUPERVISOR"), Some(Mode::Supervisor));
    }
}
