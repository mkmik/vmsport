//! What relative and indexed files are stored on, and how they are
//! described: blocks, keys, areas.

use crate::{BLOCK, Fab};
use vms_cond::Cond;

/// A file's 512-byte blocks, VBN 1 first. The host implements it over a
/// file; `Vec<u8>` does for tests. Relative and indexed files read what
/// they need at each call and keep no buckets across calls.
pub trait Blocks {
    /// Reads `buf.len()` bytes (whole blocks) from block `vbn` on.
    fn read(&mut self, vbn: u32, buf: &mut [u8]) -> Result<(), Cond>;
    fn write(&mut self, vbn: u32, buf: &[u8]) -> Result<(), Cond>;
    /// Blocks allocated (VMS's HIBLK).
    fn allocated(&self) -> u32;
    /// Adds `n` zeroed blocks; returns the first new VBN.
    fn grow(&mut self, n: u32) -> Result<u32, Cond>;
}

/// A borrowed store, for an [`idx::File`](crate::idx::File) over blocks
/// the caller keeps.
impl<B: Blocks + ?Sized> Blocks for &mut B {
    fn read(&mut self, vbn: u32, buf: &mut [u8]) -> Result<(), Cond> {
        (**self).read(vbn, buf)
    }

    fn write(&mut self, vbn: u32, buf: &[u8]) -> Result<(), Cond> {
        (**self).write(vbn, buf)
    }

    fn allocated(&self) -> u32 {
        (**self).allocated()
    }

    fn grow(&mut self, n: u32) -> Result<u32, Cond> {
        (**self).grow(n)
    }
}

impl Blocks for Vec<u8> {
    fn read(&mut self, vbn: u32, buf: &mut [u8]) -> Result<(), Cond> {
        let at = (vbn as usize - 1) * BLOCK;
        let src = self.get(at..at + buf.len()).ok_or(crate::status::RER)?;
        buf.copy_from_slice(src);
        Ok(())
    }

    fn write(&mut self, vbn: u32, buf: &[u8]) -> Result<(), Cond> {
        let at = (vbn as usize - 1) * BLOCK;
        if self.len() < at + buf.len() {
            return Err(crate::status::WER);
        }
        self[at..at + buf.len()].copy_from_slice(buf);
        Ok(())
    }

    fn allocated(&self) -> u32 {
        (self.len() / BLOCK) as u32
    }

    fn grow(&mut self, n: u32) -> Result<u32, Cond> {
        let first = self.allocated() + 1;
        self.resize(self.len() + n as usize * BLOCK, 0);
        Ok(first)
    }
}

/// A record's file address: the bucket's VBN and the record's ID in it
/// (indexed), or the record number (relative: VBN 0 and ID unused).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash, PartialOrd, Ord)]
pub struct Rfa {
    pub vbn: u32,
    pub id: u16,
}

/// FDL KEY TYPE.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KeyType {
    #[default]
    String,
    Int2,
    Int4,
    Int8,
    Bin2,
    Bin4,
    Bin8,
    /// Packed decimal.
    Decimal,
    /// Descending variants (DSTRING, DINT4...): `descending` set.
    Collated,
}

/// One segment of a key: where it is in the record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Segment {
    pub position: u16,
    pub length: u16,
}

/// An indexed file's key (FDL KEY n; prologue key descriptor).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KeyDesc {
    pub number: u8,
    pub name: String,
    pub typ: KeyType,
    pub descending: bool,
    pub segments: Vec<Segment>,
    pub duplicates: bool,
    pub changes: bool,
    pub null_key: bool,
    pub null_value: u8,
    pub data_area: u8,
    pub index_area: u8,
    pub level1_index_area: u8,
    /// Fill, in bytes per bucket (FDL DATA_FILL, INDEX_FILL as percent
    /// turned into bytes when a file is made).
    pub data_fill: u16,
    pub index_fill: u16,
    pub data_key_compression: bool,
    pub data_record_compression: bool,
    pub index_compression: bool,
}

impl KeyDesc {
    /// Total key length.
    pub fn length(&self) -> usize {
        self.segments.iter().map(|s| s.length as usize).sum()
    }

    /// The key's bytes in a record (segments concatenated).
    pub fn extract(&self, record: &[u8]) -> Vec<u8> {
        let mut k = Vec::with_capacity(self.length());
        for s in &self.segments {
            let (a, b) = (s.position as usize, s.position as usize + s.length as usize);
            k.extend_from_slice(record.get(a..b.min(record.len())).unwrap_or(&[]));
        }
        k
    }
}

/// FDL AREA n.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Area {
    pub number: u8,
    pub allocation: u32,
    pub bucket_size: u8,
    pub extension: u16,
    pub contiguous: bool,
    pub best_try_contiguous: bool,
}

/// What a file is to be: its FAB, and for an indexed file its areas and
/// keys (what FDL describes and CREATE/FDL makes).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Design {
    pub fab: Fab,
    /// Relative files: the highest record number allowed, 0 for none.
    pub max_record_number: u32,
    pub prologue: u8,
    pub areas: Vec<Area>,
    pub keys: Vec<KeyDesc>,
}
