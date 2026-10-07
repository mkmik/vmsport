//! Sequential RMS files on the host: records through vms-rms, attributes
//! in the `vms.fab` extended attribute (none: stream_LF text).

use crate::{status, sys};
use std::io::Write;
use std::path::Path;
use vms_cond::Cond;
use vms_rms::{Fab, Record};

/// The file's record attributes.
pub fn fab(path: &Path) -> Fab {
    sys::get_xattr(path, vms_rms::XATTR)
        .and_then(|v| String::from_utf8(v).ok())
        .and_then(|s| s.parse().ok())
        .unwrap_or_default()
}

/// A sequential file being read.
pub struct Reader {
    pub fab: Fab,
    records: std::vec::IntoIter<Record>,
}

impl Reader {
    /// A file's records: a relative or indexed file's in order (by its
    /// primary key), shared with writers.
    pub fn open(path: &Path) -> Result<Reader, Cond> {
        let fab = fab(path);
        if fab.org != vms_rms::Org::Seq {
            use crate::rms::{At, File, Rop, fab::*};
            let mut f = File::open(path, GET, GET | PUT | UPD | DEL)?;
            let rop = Rop {
                nolock: true,
                ..Rop::default()
            };
            let records: Vec<Record> = std::iter::from_fn(|| f.get(At::Next, rop).ok()).collect();
            return Ok(Reader {
                fab,
                records: records.into_iter(),
            });
        }
        let bytes = std::fs::read(path).map_err(io_status)?;
        let records = vms_rms::decode(&fab, &bytes).map_err(|_| status::RER)?;
        Ok(Reader {
            fab,
            records: records.into_iter(),
        })
    }

    pub fn get(&mut self) -> Option<Record> {
        self.records.next()
    }
}

/// A sequential file being written: a new file, or appending.
pub struct Writer {
    pub fab: Fab,
    file: std::fs::File,
    offset: usize,
}

impl Writer {
    /// A new file with `fab`'s attributes; the default (stream_LF) needs
    /// no extended attribute.
    pub fn create(path: &Path, fab: Fab) -> Result<Writer, Cond> {
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(io_status)?;
        if fab != Fab::default() {
            sys::set_xattr(path, vms_rms::XATTR, fab.to_string().as_bytes())
                .map_err(|_| status::CRE)?;
        }
        Ok(Writer {
            fab,
            file,
            offset: 0,
        })
    }

    pub fn append(path: &Path) -> Result<Writer, Cond> {
        let fab = fab(path);
        let file = std::fs::OpenOptions::new()
            .append(true)
            .open(path)
            .map_err(io_status)?;
        let offset = file.metadata().map_err(io_status)?.len() as usize;
        Ok(Writer { fab, file, offset })
    }

    pub fn put(&mut self, rec: &Record) -> Result<(), Cond> {
        let bytes = vms_rms::encode_record(&self.fab, self.offset, rec).map_err(|_| status::WER)?;
        self.file.write_all(&bytes).map_err(|_| status::WER)?;
        self.offset += bytes.len();
        Ok(())
    }

    /// The host file, to hand an image as its stdout.
    pub fn file(&self) -> &std::fs::File {
        &self.file
    }
}

pub fn io_status(e: std::io::Error) -> Cond {
    match e.kind() {
        std::io::ErrorKind::NotFound => status::FNF,
        std::io::ErrorKind::PermissionDenied => status::PRV,
        std::io::ErrorKind::AlreadyExists => status::FEX,
        _ => status::RER,
    }
}

/// What DIRECTORY shows about a file.
pub struct Info {
    /// Blocks up to the end of file.
    pub used: u64,
    /// Blocks the host gave it (at least `used`).
    pub allocated: u64,
    /// VMS times.
    pub created: i64,
    pub revised: i64,
    pub fab: Fab,
}

pub fn info(path: &Path) -> Result<Info, Cond> {
    use std::os::unix::fs::MetadataExt;
    let m = std::fs::metadata(path).map_err(io_status)?;
    let used = m.len().div_ceil(512);
    let revised = m.modified().map(sys::vms_time).unwrap_or(0);
    Ok(Info {
        used,
        allocated: (m.blocks() * 512 / 512).max(used),
        created: m.created().map(sys::vms_time).unwrap_or(revised),
        revised,
        fab: fab(path),
    })
}

/// Deletes one file (one version).
pub fn delete(path: &Path) -> Result<(), Cond> {
    std::fs::remove_file(path).map_err(io_status)
}
