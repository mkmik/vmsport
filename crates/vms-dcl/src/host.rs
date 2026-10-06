//! What DCL needs from the system it runs on. libvms implements it on the
//! host; tests use a fake.

use vms_cond::Cond;

/// An open record file: OPEN/READ/WRITE, procedure input and /OUTPUT.
pub trait RecordFile {
    /// The next record, `None` at end of file.
    fn read(&mut self) -> Result<Option<String>, Cond>;
    fn write(&mut self, record: &str) -> Result<(), Cond>;
    /// The host file under it, for an image's stdout when SYS$OUTPUT is
    /// this file.
    fn host_file(&self) -> Option<std::fs::File> {
        None
    }
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
    ) -> Result<(Box<dyn RecordFile>, String), Cond>;
    /// SYS$OUTPUT as DCL started with it.
    fn terminal_output(&mut self) -> Box<dyn RecordFile>;
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
    fn run_image(
        &mut self,
        image: &str,
        tables: &str,
        line: &str,
        out: Option<std::fs::File>,
    ) -> Cond;
    /// A foreign command (`X :== $path`): runs `path` with Unix argv.
    fn run_foreign(&mut self, path: &str, args: &[String]) -> Cond;

    /// Local time, VMS format (100 ns since 17-NOV-1858).
    fn now(&mut self) -> i64;
    /// F$GETJPI / F$GETSYI items, F$USER and the like.
    fn info(&mut self, item: &str) -> Option<String>;
}
