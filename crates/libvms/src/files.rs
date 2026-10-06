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
    pub fn open(path: &Path) -> Result<Reader, Cond> {
        let fab = fab(path);
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
