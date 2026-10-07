//! What DCL needs from the system it runs on. libvms implements it on the
//! host; tests use a fake.

use vms_cond::Cond;

pub use libvms::image::{Change, Launch};

/// What to start.
pub enum Child<'a> {
    /// An image named by a VMS file spec (a verb's IMAGE).
    Image(&'a str),
    /// A foreign command: a VMS file spec or a host path.
    Foreign(&'a str),
    /// DCL itself, as a subprocess (SPAWN, PIPE).
    Dcl,
}

pub use libvms::rms::Match;

/// What READ asks beyond the next record.
#[derive(Debug, Default)]
pub struct Get {
    /// /KEY, /MATCH and /INDEX: the key value, how it matches, the key.
    pub key: Option<(Vec<u8>, Match, u8)>,
    /// /DELETE: the record goes once read.
    pub delete: bool,
    /// /NOLOCK
    pub nolock: bool,
}

/// An open record file: OPEN/READ/WRITE, procedure input and /OUTPUT.
pub trait RecordFile {
    /// The next record, `None` at end of file.
    fn read(&mut self) -> Result<Option<String>, Cond>;
    fn write(&mut self, record: &str) -> Result<(), Cond>;
    /// READ with its qualifiers; a plain file takes none of them.
    fn get(&mut self, how: &Get) -> Result<Option<String>, Cond> {
        match how.key {
            Some(_) => Err(vms_rms::status::RAC),
            None => self.read(),
        }
    }
    /// WRITE: RMS's status (OK_DUP when an alternate key repeats).
    fn put(&mut self, record: &str) -> Result<Cond, Cond> {
        self.write(record).map(|()| vms_rms::status::NORMAL)
    }
    /// WRITE/UPDATE: the record READ last.
    fn update(&mut self, _record: &str) -> Result<Cond, Cond> {
        Err(vms_rms::status::IOP)
    }
    /// The host file under it, for an image's stdout when SYS$OUTPUT is
    /// this file.
    fn host_file(&self) -> Option<std::fs::File> {
        None
    }
}

/// OPEN/SHARE: what others may do with the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Share {
    None,
    Read,
    Write,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Read,
    /// A new version.
    Write,
    Append,
    ReadWrite,
}

/// Which logical name table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Table {
    Process,
    Job,
    Group,
    System,
    Named(String),
}

pub trait Host {
    /// The command tables DCL starts with: the CLD text of every file in
    /// SYS$LIBRARY:[DCLTABLES].
    fn dcl_tables(&mut self) -> String;
    /// `.MSG` source of the system messages (SYS$MESSAGE).
    fn system_messages(&mut self) -> String;

    /// Opens a file; `default` supplies missing parts (`.COM`). Returns the
    /// full file spec it opened, too.
    fn open(
        &mut self,
        spec: &str,
        default: &str,
        mode: Mode,
        share: Share,
    ) -> Result<(Box<dyn RecordFile>, String), Cond>;
    /// SYS$OUTPUT as DCL started with it.
    fn terminal_output(&mut self) -> Box<dyn RecordFile>;
    /// SYS$ERROR as DCL started with it, and whether it is the same file as
    /// SYS$OUTPUT (both the terminal, say).
    fn error_output(&mut self) -> (Box<dyn RecordFile>, bool);
    /// A file opened for reading as it is, for a child's input
    /// (SPAWN/INPUT, PIPE <).
    fn input_file(&mut self, spec: &str) -> Result<std::fs::File, Cond>;
    /// A line from the terminal (INQUIRE, READ SYS$COMMAND); `None` at end.
    fn read_terminal(&mut self, prompt: &str) -> Option<String>;

    /// `$PARSE`: the expanded spec (upcased), `None` if it doesn't parse or,
    /// unless `syntax_only`, the device doesn't exist (or the directory, if
    /// it must).
    fn parse(
        &mut self,
        spec: &str,
        default: &str,
        related: &str,
        syntax_only: bool,
        directory_must_exist: bool,
    ) -> Option<String>;
    /// `$SEARCH`: the next file matching `spec` in the search context
    /// `stream`; a new spec restarts it.
    fn search(&mut self, spec: &str, stream: u32) -> Option<String>;
    fn default_directory(&mut self) -> String;
    fn set_default(&mut self, spec: &str) -> Result<(), Cond>;

    /// `$TRNLNM` item for F$TRNLNM: `item` is VALUE, LENGTH, MAX_INDEX, ...
    fn trnlnm(&mut self, name: &str, table: &str, index: u32, item: &str) -> Option<String>;
    fn define(
        &mut self,
        name: &str,
        equivs: &[String],
        table: &Table,
        attrs: &[String],
    ) -> Result<Cond, Cond>;
    fn deassign(&mut self, name: Option<&str>, table: &Table) -> Result<(), Cond>;
    /// SHOW LOGICAL lines.
    fn show_logical(
        &mut self,
        names: &[String],
        tables: &[Table],
        full: bool,
    ) -> Result<Vec<String>, Cond>;

    /// Runs an image for a command: `tables` is the CLD the image parses
    /// `line` with; `out` is SYS$OUTPUT if that is a file. Returns the
    /// image's status.
    /// Starts a child: an image, a foreign command or a DCL subprocess,
    /// with `launch`'s context (its default directory and process logical
    /// names are filled in here).
    fn start(&mut self, what: Child, launch: Launch) -> Result<libvms::image::Child, Cond>;
    /// Applies the process logical names a child changed.
    fn apply(&mut self, changes: &[Change]);
    /// What a command with /HELP shows (`None`: no /HELP in it).
    fn command_help(&mut self, tables: &vms_cld::Tables, line: &str) -> Option<Vec<String>>;
    /// DCL$PATH: where an unknown verb might be, a procedure or a program.
    fn dcl_path(&mut self, verb: &str) -> Option<(String, bool)>;

    /// Local time, VMS format (100 ns since 17-NOV-1858).
    fn now(&mut self) -> i64;
    /// F$GETJPI / F$GETSYI items, F$USER and the like.
    fn info(&mut self, item: &str) -> Option<String>;
}
