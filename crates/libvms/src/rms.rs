//! RMS on the host: a file's blocks over a host file, and sharing between
//! processes through vmsportd's lock manager (docs/design/m3.md).

use crate::files::io_status;
use std::os::unix::fs::FileExt;
use vms_cond::Cond;
use vms_rms::{BLOCK, Blocks, status};
use vmsportd::locks::Mode;

/// A host file as VMS blocks: VBN n is bytes `(n-1)*512 .. n*512`.
pub struct HostBlocks {
    pub file: std::fs::File,
    blocks: u32,
}

impl HostBlocks {
    pub fn new(file: std::fs::File) -> Result<HostBlocks, Cond> {
        let len = file.metadata().map_err(io_status)?.len();
        Ok(HostBlocks {
            file,
            blocks: len.div_ceil(BLOCK as u64) as u32,
        })
    }
}

impl Blocks for HostBlocks {
    fn read(&mut self, vbn: u32, buf: &mut [u8]) -> Result<(), Cond> {
        let at = (vbn as u64 - 1) * BLOCK as u64;
        // Past the end of the host file (a short last block) reads zeros.
        buf.fill(0);
        let mut done = 0;
        while done < buf.len() {
            match self.file.read_at(&mut buf[done..], at + done as u64) {
                Ok(0) => break,
                Ok(n) => done += n,
                Err(_) => return Err(status::RER),
            }
        }
        Ok(())
    }

    fn write(&mut self, vbn: u32, buf: &[u8]) -> Result<(), Cond> {
        let at = (vbn as u64 - 1) * BLOCK as u64;
        self.file.write_all_at(buf, at).map_err(|_| status::WER)?;
        self.blocks = self.blocks.max(vbn - 1 + buf.len().div_ceil(BLOCK) as u32);
        Ok(())
    }

    fn allocated(&self) -> u32 {
        self.blocks
    }

    fn grow(&mut self, n: u32) -> Result<u32, Cond> {
        let first = self.blocks + 1;
        self.file
            .set_len((self.blocks + n) as u64 * BLOCK as u64)
            .map_err(|_| status::EXT)?;
        self.blocks += n;
        Ok(first)
    }
}

/// FAB$B_FAC and FAB$B_SHR bits.
pub mod fab {
    pub const PUT: u8 = 1;
    pub const GET: u8 = 2;
    pub const DEL: u8 = 4;
    pub const UPD: u8 = 8;
    /// SHR only: no sharing at all.
    pub const NIL: u8 = 0x20;
}

/// The file lock an open takes for its access and sharing: others hold
/// theirs in compatible modes exactly when each one's sharing allows the
/// other's access.
pub fn file_lock_mode(fac: u8, shr: u8) -> Mode {
    let writes = fab::PUT | fab::DEL | fab::UPD;
    let (writer, shares_writes) = (fac & writes != 0, shr & writes != 0);
    match () {
        _ if shr & fab::NIL != 0 || shr & (writes | fab::GET) == 0 => Mode::EX,
        _ if shares_writes => {
            if writer {
                Mode::CW
            } else {
                Mode::CR
            }
        }
        _ if writer => Mode::PW,
        _ => Mode::PR,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fab::*;

    #[test]
    fn sharing() {
        let all = PUT | GET | DEL | UPD;
        let m = |fac, shr| file_lock_mode(fac, shr);
        let ok = |a: (u8, u8), b: (u8, u8)| m(a.0, a.1).compatible(m(b.0, b.1));
        // Readers sharing reads; writers sharing everything.
        assert!(ok((GET, GET), (GET, GET)));
        assert!(ok((GET | PUT, all), (GET | PUT, all)));
        // A writer that shares reads, with readers that allow writers.
        assert!(ok((GET | PUT, GET), (GET, all)));
        assert!(!ok((GET | PUT, GET), (GET, GET)));
        assert!(!ok((GET | PUT, GET), (GET | PUT, GET)));
        // No sharing.
        assert!(!ok((GET, NIL), (GET, all)));
        assert!(!ok((GET, 0), (GET, GET)));
    }

    #[test]
    fn blocks() {
        let path = std::env::temp_dir().join(format!("vpt-hb-{}", std::process::id()));
        let f = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)
            .unwrap();
        let mut b = HostBlocks::new(f).unwrap();
        assert_eq!(b.grow(2).unwrap(), 1);
        b.write(2, &[7; 512]).unwrap();
        let mut buf = [0; 1024];
        b.read(1, &mut buf).unwrap();
        assert_eq!((buf[0], buf[512], b.allocated()), (0, 7, 2));
        std::fs::remove_file(&path).unwrap();
    }
}
