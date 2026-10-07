//! RMS's services for C: sys$open ... sys$close on FAB, RAB, NAM and XAB
//! blocks, over libvms::rms. The blocks have VMS's fields, in VMS's order,
//! with host-width pointers (include/fab.h, rab.h, nam.h, xab.h); this
//! file and those headers must agree (tests/layout.rs checks them).
//!
//! A status goes in the block's STS and is returned; the ERR or SUC
//! routine an argument names is called before returning, as RMS calls it
//! for a synchronous operation.

use libvms::rms::{self as host, At, Match, Rop};
use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::Mutex;
use vms_cond::Cond;
use vms_filespec::{FileSpec, Version};
use vms_rms::{Area, Design, Fab as Attrs, KeyDesc, KeyType, Org, Record, Rfm, Segment, status};

pub const FAB_BID: u8 = 3;
pub const RAB_BID: u8 = 1;
pub const NAM_BID: u8 = 2;
pub const XAB_DAT: u8 = 18;
pub const XAB_PRO: u8 = 19;
pub const XAB_ALL: u8 = 20;
pub const XAB_KEY: u8 = 21;
pub const XAB_SUM: u8 = 22;
pub const XAB_FHC: u8 = 29;

/// FAB$L_FOP bits used here.
mod fop {
    pub const SUP: u32 = 0x4;
    pub const CIF: u32 = 0x200_0000;
}

/// RAB$L_ROP bits used here.
mod rop {
    pub const UIF: u32 = 0x10;
    pub const EOF: u32 = 0x100;
    pub const WAT: u32 = 0x2_0000;
    pub const NLK: u32 = 0x10_0000;
    pub const KGE: u32 = 0x20_0000;
    pub const KGT: u32 = 0x40_0000;
}

/// RAB$B_RAC.
const RAC_KEY: u8 = 1;
const RAC_RFA: u8 = 2;

/// NAM$L_FNB bits.
mod fnb {
    pub const EXP_VER: u32 = 0x1;
    pub const EXP_TYPE: u32 = 0x2;
    pub const EXP_NAME: u32 = 0x4;
    pub const WILD_VER: u32 = 0x8;
    pub const WILD_TYPE: u32 = 0x10;
    pub const WILD_NAME: u32 = 0x20;
    pub const EXP_DIR: u32 = 0x40;
    pub const EXP_DEV: u32 = 0x80;
    pub const WILDCARD: u32 = 0x100;
}

/// XABKEY flag bits (XAB$B_FLG).
mod flg {
    pub const DUP: u8 = 0x1;
    pub const CHG: u8 = 0x2;
    pub const NUL: u8 = 0x4;
    pub const IDX_NCMPR: u8 = 0x8;
    pub const KEY_NCMPR: u8 = 0x40;
    pub const DAT_NCMPR: u8 = 0x80;
}

#[repr(C)]
pub struct Fab {
    pub bid: u8,
    pub bln: u8,
    pub ifi: u16,
    pub fop: u32,
    pub sts: u32,
    pub stv: u32,
    pub alq: u32,
    pub deq: u16,
    pub fac: u8,
    pub shr: u8,
    pub ctx: u32,
    pub rtv: u8,
    pub org: u8,
    pub rat: u8,
    pub rfm: u8,
    pub jnl: *mut c_void,
    pub xab: *mut Xab,
    pub nam: *mut Nam,
    pub fna: *const u8,
    pub dna: *const u8,
    pub fns: u8,
    pub dns: u8,
    pub mrs: u16,
    pub mrn: u32,
    pub bls: u16,
    pub bks: u8,
    pub fsz: u8,
    pub dev: u32,
    pub sdc: u32,
    pub gbc: u16,
    pub acmodes: u8,
    pub rcf: u8,
}

#[repr(C)]
pub struct Rab {
    pub bid: u8,
    pub bln: u8,
    pub isi: u16,
    pub rop: u32,
    pub sts: u32,
    pub stv: u32,
    pub rfa0: u32,
    pub rfa4: u16,
    pub rfa_fill: u16,
    pub ctx: u32,
    pub rac: u8,
    pub tmo: u8,
    pub usz: u16,
    pub rsz: u16,
    pub ubf: *mut u8,
    pub rbf: *mut u8,
    pub rhb: *mut u8,
    pub kbf: *const u8,
    pub ksz: u8,
    pub krf: u8,
    pub mbf: u8,
    pub mbc: u8,
    pub bkt: u32,
    pub fab: *mut Fab,
    pub xab: *mut Xab,
}

#[repr(C)]
pub struct Nam {
    pub bid: u8,
    pub bln: u8,
    pub rss: u8,
    pub rsl: u8,
    pub fnb: u32,
    pub rlf: *mut Nam,
    pub rsa: *mut u8,
    pub esa: *mut u8,
    pub ess: u8,
    pub esl: u8,
    pub nop: u8,
    pub rfs: u8,
    pub wcc: u32,
    pub fid: [u16; 3],
    pub did: [u16; 3],
    pub dvi: [u8; 16],
    /// Lengths of node, device, directory, name, type and version.
    pub lens: [u8; 6],
    /// Where they are in the expanded or resultant string.
    pub ptrs: [*mut u8; 6],
}

/// What every XAB starts with.
#[repr(C)]
pub struct Xab {
    pub cod: u8,
    pub bln: u8,
    pub nxt: *mut Xab,
}

#[repr(C)]
pub struct XabKey {
    pub head: Xab,
    pub ian: u8,
    pub lan: u8,
    pub dan: u8,
    pub lvl: u8,
    pub ibs: u8,
    pub dbs: u8,
    pub flg: u8,
    pub dtp: u8,
    pub rvb: u32,
    pub nsg: u8,
    pub nul: u8,
    pub tks: u8,
    pub r#ref: u8,
    pub mrl: u16,
    pub ifl: u16,
    pub dfl: u16,
    pub pos: [u16; 8],
    pub siz: [u8; 8],
    pub prolog: u8,
    pub knm: *mut u8,
    pub dvb: u32,
    pub typ: [u8; 8],
}

#[repr(C)]
pub struct XabAll {
    pub head: Xab,
    pub aop: u8,
    pub aln: u8,
    pub vol: u16,
    pub loc: u32,
    pub alq: u32,
    pub deq: u16,
    pub bkz: u8,
    pub aid: u8,
    pub rfi: [u16; 3],
}

#[repr(C)]
pub struct XabDat {
    pub head: Xab,
    pub rvn: u16,
    pub cdt: u64,
    pub rdt: u64,
    pub edt: u64,
    pub bdt: u64,
    pub acc: u64,
    pub att: u64,
    pub r#mod: u64,
}

#[repr(C)]
pub struct XabFhc {
    pub head: Xab,
    pub rfo: u8,
    pub atr: u8,
    pub lrl: u16,
    pub hbk: u32,
    pub ebk: u32,
    pub ffb: u16,
    pub bkz: u8,
    pub hsz: u8,
    pub mrz: u16,
    pub dxq: u16,
    pub gbc: u16,
    pub verlimit: u16,
    pub sbn: u32,
}

#[repr(C)]
pub struct XabPro {
    pub head: Xab,
    pub pro: u16,
    pub mtacc: u8,
    pub prot_opt: u8,
    /// Member in the low word, group in the high one.
    pub uic: u32,
}

#[repr(C)]
pub struct XabSum {
    pub head: Xab,
    pub noa: u8,
    pub nok: u8,
    pub pvn: u16,
}

macro_rules! prototypes {
    ($($t:ty),*) => { $(unsafe impl Sync for $t {})* };
}
prototypes!(
    Fab, Rab, Nam, XabKey, XabAll, XabDat, XabFhc, XabPro, XabSum
);

/// An all-zero block (null pointers): the prototypes start from it.
const fn zero<T>() -> T {
    // SAFETY: every block is integers and raw pointers, for which zero is valid.
    unsafe { std::mem::zeroed() }
}

const fn head(cod: u8, bln: u8) -> Xab {
    Xab {
        cod,
        bln,
        nxt: std::ptr::null_mut(),
    }
}

#[unsafe(export_name = "cc$rms_fab")]
pub static CC_RMS_FAB: Fab = Fab {
    bid: FAB_BID,
    bln: 80,
    rfm: 2,
    ..zero()
};
#[unsafe(export_name = "cc$rms_rab")]
pub static CC_RMS_RAB: Rab = Rab {
    bid: RAB_BID,
    bln: 68,
    ..zero()
};
#[unsafe(export_name = "cc$rms_nam")]
pub static CC_RMS_NAM: Nam = Nam {
    bid: NAM_BID,
    bln: 96,
    ..zero()
};
#[unsafe(export_name = "cc$rms_xabkey")]
pub static CC_RMS_XABKEY: XabKey = XabKey {
    head: head(XAB_KEY, 100),
    ..zero()
};
#[unsafe(export_name = "cc$rms_xaball")]
pub static CC_RMS_XABALL: XabAll = XabAll {
    head: head(XAB_ALL, 32),
    ..zero()
};
#[unsafe(export_name = "cc$rms_xabdat")]
pub static CC_RMS_XABDAT: XabDat = XabDat {
    head: head(XAB_DAT, 84),
    ..zero()
};
#[unsafe(export_name = "cc$rms_xabfhc")]
pub static CC_RMS_XABFHC: XabFhc = XabFhc {
    head: head(XAB_FHC, 44),
    ..zero()
};
#[unsafe(export_name = "cc$rms_xabpro")]
pub static CC_RMS_XABPRO: XabPro = XabPro {
    head: head(XAB_PRO, 88),
    ..zero()
};
#[unsafe(export_name = "cc$rms_xabsum")]
pub static CC_RMS_XABSUM: XabSum = XabSum {
    head: head(XAB_SUM, 12),
    ..zero()
};

/// The open files, by FAB$W_IFI - 1. Each has one record stream: RAB$W_ISI
/// is its IFI (ponytail: FAB$V_MSE's several streams when needed).
struct Open {
    file: host::File,
    connected: bool,
}

static OPEN: Mutex<Vec<Option<Open>>> = Mutex::new(Vec::new());

/// $SEARCH's place, by the NAM's address: the spec $PARSE left, and the
/// files not yet returned (`None` before the first $SEARCH).
type Search = (FileSpec, Option<std::vec::IntoIter<(PathBuf, FileSpec)>>);
static SEARCH: Mutex<Vec<(usize, Search)>> = Mutex::new(Vec::new());

fn with_open<R>(
    ifi: u16,
    f: impl FnOnce(&mut Open) -> Result<R, Cond>,
    bad: Cond,
) -> Result<R, Cond> {
    let mut t = OPEN.lock().unwrap_or_else(|e| e.into_inner());
    let o = t
        .get_mut((ifi as usize).wrapping_sub(1))
        .and_then(|o| o.as_mut())
        .ok_or(bad)?;
    f(o)
}

/// Calls the ERR or SUC routine, if one is named, with the block.
unsafe fn complete<T>(block: *mut T, st: Cond, err: *const c_void, suc: *const c_void) -> u32 {
    let r = if st.is_success() { suc } else { err };
    if !r.is_null() {
        // SAFETY: the caller passed a routine taking the block.
        let f: unsafe extern "C" fn(*mut T) = unsafe { std::mem::transmute(r) };
        unsafe { f(block) };
    }
    st.0
}

unsafe fn bytes<'a>(p: *const u8, n: usize) -> &'a [u8] {
    if p.is_null() || n == 0 {
        return &[];
    }
    // SAFETY: the caller's block says `n` bytes are there.
    unsafe { std::slice::from_raw_parts(p, n) }
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// The FAB's XABs, in chain order.
unsafe fn xabs(mut x: *mut Xab) -> Vec<*mut Xab> {
    let mut out = Vec::new();
    while !x.is_null() && out.len() < 256 {
        out.push(x);
        x = unsafe { (*x).nxt };
    }
    out
}

/// The NAM a FAB names: RMS$_NAM if its block ID is wrong.
unsafe fn nam_of<'a>(f: &Fab) -> Result<Option<&'a mut Nam>, Cond> {
    match unsafe { f.nam.as_mut() } {
        Some(n) if n.bid != NAM_BID => Err(status::NAM),
        n => Ok(n),
    }
}

/// The FAB's file name, merged with its default name and the related
/// NAM's resultant name.
unsafe fn parse(f: &Fab, nam: Option<&Nam>) -> Result<FileSpec, Cond> {
    let spec = text(unsafe { bytes(f.fna, f.fns as usize) });
    let default = text(unsafe { bytes(f.dna, f.dns as usize) });
    let related = nam
        .and_then(|n| unsafe { n.rlf.as_ref() })
        .map(|r| text(unsafe { bytes(r.rsa, r.rsl as usize) }))
        .unwrap_or_default();
    libvms::cli::session(|s| s.parse(&spec, &default, &related))?
}

/// The spec with its logical names translated, as $PARSE expands it.
fn expanded(spec: &FileSpec) -> Result<FileSpec, Cond> {
    let mut e = libvms::cli::session(|s| s.locate(spec))??
        .into_iter()
        .next()
        .ok_or(status::DEV)?
        .0;
    e.version = spec.version;
    Ok(e)
}

/// Writes `spec` into a NAM's expanded (`resultant` false) or resultant
/// string, and points the field pointers and lengths at its parts.
unsafe fn fill_name(n: &mut Nam, spec: &FileSpec, resultant: bool) -> Result<(), Cond> {
    let dev = spec
        .device
        .as_ref()
        .map_or(String::new(), |d| format!("{d}:"));
    let dir = spec
        .directory
        .as_ref()
        .map_or(String::new(), |d| d.to_string());
    let typ = format!(".{}", spec.typ.as_deref().unwrap_or(""));
    let ver = spec.version.map_or(";".to_string(), |v| format!(";{v}"));
    let parts = [String::new(), dev, dir, spec.name.clone(), typ, ver];
    let all: String = parts.concat();
    let (buf, size, len, err) = match resultant {
        true => (n.rsa, n.rss, &mut n.rsl, status::RSS),
        false => (n.esa, n.ess, &mut n.esl, status::ESS),
    };
    if buf.is_null() || size == 0 {
        return Ok(());
    }
    if all.len() > size as usize {
        return Err(err);
    }
    // SAFETY: the buffer holds `size` bytes.
    unsafe { std::ptr::copy_nonoverlapping(all.as_ptr(), buf, all.len()) };
    *len = all.len() as u8;
    let mut at = 0;
    for (i, p) in parts.iter().enumerate() {
        n.lens[i] = p.len() as u8;
        n.ptrs[i] = unsafe { buf.add(at) };
        at += p.len();
    }
    Ok(())
}

/// NAM$L_FNB: what the typed spec gave explicitly, and its wildcards.
fn fnb_of(typed: &str, spec: &FileSpec) -> u32 {
    let t: FileSpec = typed.parse().unwrap_or_default();
    let wild = |s: &str| s.contains(['*', '%']);
    let mut b = 0;
    for (on, bit) in [
        (t.version.is_some(), fnb::EXP_VER),
        (t.typ.is_some(), fnb::EXP_TYPE),
        (!t.name.is_empty(), fnb::EXP_NAME),
        (t.directory.is_some(), fnb::EXP_DIR),
        (t.device.is_some(), fnb::EXP_DEV),
        (wild(&spec.name), fnb::WILD_NAME),
        (spec.typ.as_deref().is_some_and(wild), fnb::WILD_TYPE),
        (spec.version == Some(Version::Wildcard), fnb::WILD_VER),
    ] {
        if on {
            b |= bit;
        }
    }
    if b & (fnb::WILD_NAME | fnb::WILD_TYPE | fnb::WILD_VER) != 0 {
        b |= fnb::WILDCARD;
    }
    b
}

const ORGS: [Org; 3] = [Org::Seq, Org::Rel, Org::Idx];
const RFMS: [Rfm; 7] = [
    Rfm::Udf,
    Rfm::Fix,
    Rfm::Var,
    Rfm::Vfc,
    Rfm::Stm,
    Rfm::Stmlf,
    Rfm::Stmcr,
];

/// The FAB's attributes, for $CREATE.
fn attrs_of(f: &Fab) -> Result<Attrs, Cond> {
    Ok(Attrs {
        org: *ORGS.get(f.org as usize >> 4).ok_or(status::ORG)?,
        rfm: *RFMS.get(f.rfm as usize).ok_or(status::RFM)?,
        rat: f.rat & 0xF,
        mrs: f.mrs,
        lrl: 0,
        fsz: f.fsz,
        bks: f.bks,
        deq: f.deq,
    })
}

fn key_type(dtp: u8) -> Result<(KeyType, bool), Cond> {
    let t = match dtp & !32 {
        0 => KeyType::String,
        1 => KeyType::Int2,
        2 => KeyType::Bin2,
        3 => KeyType::Int4,
        4 => KeyType::Bin4,
        5 => KeyType::Decimal,
        6 => KeyType::Int8,
        7 => KeyType::Bin8,
        8 => KeyType::Collated,
        _ => return Err(status::DTP),
    };
    Ok((t, dtp & 32 != 0))
}

/// What $CREATE makes: the FAB with its XABKEYs and XABALLs.
unsafe fn design(f: &Fab) -> Result<Design, Cond> {
    let mut d = Design {
        fab: attrs_of(f)?,
        max_record_number: f.mrn,
        prologue: 3,
        ..Design::default()
    };
    for x in unsafe { xabs(f.xab) } {
        match unsafe { (*x).cod } {
            XAB_KEY => {
                let k = unsafe { &*(x as *const XabKey) };
                let (typ, descending) = key_type(k.dtp)?;
                let name = unsafe { bytes(k.knm, if k.knm.is_null() { 0 } else { 32 }) };
                if k.r#ref == 0 && k.prolog != 0 {
                    d.prologue = k.prolog;
                }
                d.keys.push(KeyDesc {
                    number: k.r#ref,
                    name: text(name).trim_end_matches(['\0', ' ']).to_string(),
                    typ,
                    descending,
                    segments: (0..8)
                        .take_while(|&i| k.siz[i] != 0)
                        .map(|i| Segment {
                            position: k.pos[i],
                            length: k.siz[i] as u16,
                        })
                        .collect(),
                    duplicates: k.flg & flg::DUP != 0,
                    changes: k.flg & flg::CHG != 0,
                    null_key: k.flg & flg::NUL != 0,
                    null_value: k.nul,
                    data_area: k.dan,
                    index_area: k.ian,
                    level1_index_area: k.lan,
                    data_fill: k.dfl,
                    index_fill: k.ifl,
                    data_key_compression: k.flg & flg::KEY_NCMPR == 0,
                    data_record_compression: k.flg & flg::DAT_NCMPR == 0,
                    index_compression: k.flg & flg::IDX_NCMPR == 0,
                });
            }
            XAB_ALL => {
                let a = unsafe { &*(x as *const XabAll) };
                d.areas.push(Area {
                    number: a.aid,
                    allocation: a.alq,
                    bucket_size: a.bkz,
                    extension: a.deq,
                    contiguous: a.aop & 0x80 != 0,
                    best_try_contiguous: a.aop & 0x20 != 0,
                });
            }
            _ => {}
        }
    }
    if d.areas.is_empty() {
        d.areas.push(Area {
            allocation: f.alq,
            bucket_size: f.bks,
            extension: f.deq,
            ..Area::default()
        });
    }
    d.keys.sort_by_key(|k| k.number);
    Ok(d)
}

/// Fills the FAB and its XABs from an open file.
unsafe fn display(f: &mut Fab, file: &host::File) {
    let a = &file.fab;
    let info = libvms::files::info(&file.path).ok();
    let used = std::fs::metadata(&file.path).map_or(0, |m| m.len());
    let alloc = used.div_ceil(512) as u32;
    let mrn = match a.org {
        Org::Rel => std::fs::File::open(&file.path)
            .ok()
            .and_then(|h| host::HostBlocks::new(h).ok())
            .and_then(|mut b| vms_rms::rel::Prologue::read(&mut b).ok())
            .map_or(0, |p| p.mrn),
        _ => 0,
    };
    f.org = (ORGS.iter().position(|o| *o == a.org).unwrap_or(0) as u8) << 4;
    f.rfm = RFMS.iter().position(|r| *r == a.rfm).unwrap_or(0) as u8;
    f.rat = a.rat;
    f.mrs = a.mrs;
    f.fsz = a.fsz;
    f.bks = a.bks;
    f.deq = a.deq;
    f.alq = alloc;
    f.mrn = mrn;
    // End of file: a sequential file's last byte; a relative or indexed
    // file's whole allocation.
    let (ebk, ffb) = match a.org {
        Org::Seq => ((used / 512) as u32 + 1, (used % 512) as u16),
        _ => (alloc + 1, 0),
    };
    for x in unsafe { xabs(f.xab) } {
        match unsafe { (*x).cod } {
            XAB_FHC => {
                let h = unsafe { &mut *(x as *mut XabFhc) };
                h.rfo = f.org | f.rfm;
                h.atr = a.rat;
                h.lrl = a.lrl;
                h.hbk = alloc;
                h.ebk = ebk;
                h.ffb = ffb;
                h.bkz = a.bks;
                h.hsz = a.fsz;
                h.mrz = a.mrs;
                h.dxq = a.deq;
            }
            XAB_DAT => {
                let d = unsafe { &mut *(x as *mut XabDat) };
                d.rvn = 1;
                if let Some(i) = &info {
                    (d.cdt, d.rdt) = (i.created as u64, i.revised as u64);
                }
            }
            XAB_PRO => {
                use std::os::unix::fs::MetadataExt;
                let p = unsafe { &mut *(x as *mut XabPro) };
                if let Ok(m) = std::fs::metadata(&file.path) {
                    p.pro = protection(m.mode());
                    p.uic = m.gid() << 16 | (m.uid() & 0xFFFF);
                }
            }
            XAB_SUM => {
                let s = unsafe { &mut *(x as *mut XabSum) };
                s.noa = 0;
                s.nok = 0;
                s.pvn = 0;
            }
            XAB_ALL => {
                let al = unsafe { &mut *(x as *mut XabAll) };
                al.alq = alloc;
                al.bkz = a.bks;
                al.deq = a.deq;
            }
            _ => {}
        }
    }
}

/// VMS protection from Unix mode bits: system and owner get the owner's,
/// a set bit denies (read, write, execute, delete).
fn protection(mode: u32) -> u16 {
    let class = |rwx: u32| {
        let mut deny = 0xF;
        if rwx & 4 != 0 {
            deny &= !1;
        }
        if rwx & 2 != 0 {
            deny &= !(2 | 8);
        }
        if rwx & 1 != 0 {
            deny &= !4;
        }
        deny
    };
    let (o, g, w) = (class(mode >> 6 & 7), class(mode >> 3 & 7), class(mode & 7));
    (o | o << 4 | g << 8 | w << 12) as u16
}

fn insert(file: host::File) -> u16 {
    let mut t = OPEN.lock().unwrap_or_else(|e| e.into_inner());
    let o = Some(Open {
        file,
        connected: false,
    });
    match t.iter().position(Option::is_none) {
        Some(i) => {
            t[i] = o;
            i as u16 + 1
        }
        None => {
            t.push(o);
            t.len() as u16
        }
    }
}

/// FAB$B_FAC, GET when none is set.
fn fac(f: &Fab) -> u8 {
    if f.fac & 0xF == 0 {
        host::fab::GET
    } else {
        f.fac & 0xF
    }
}

/// A FAB service: the block checked, `op` run, STS set, ERR or SUC called.
unsafe fn fab_service(
    fab: *mut Fab,
    err: *const c_void,
    suc: *const c_void,
    op: impl FnOnce(&mut Fab) -> Result<Cond, Cond>,
) -> u32 {
    let Some(f) = (unsafe { fab.as_mut() }).filter(|f| f.bid == FAB_BID) else {
        return status::FAB.0;
    };
    let st = op(f).unwrap_or_else(|e| e);
    f.sts = st.0;
    unsafe { complete(fab, st, err, suc) }
}

/// # Safety
/// `fab` is a FAB; its pointers point where its sizes say.
#[unsafe(export_name = "sys$open")]
pub unsafe extern "C" fn open(fab: *mut Fab, err: *const c_void, suc: *const c_void) -> u32 {
    unsafe {
        fab_service(fab, err, suc, |f| {
            let nam = nam_of(f)?;
            let spec = parse(f, nam.as_deref())?;
            let (path, shown) = libvms::cli::session(|s| s.find(&spec))??;
            let file = host::File::open(&path, fac(f), f.shr)?;
            display(f, &file);
            if let Some(n) = nam {
                fill_name(n, &expanded(&spec)?, false)?;
                fill_name(n, &shown, true)?;
            }
            f.ifi = insert(file);
            Ok(status::NORMAL)
        })
    }
}

/// # Safety
/// As for [`open`]; its XABKEYs and XABALLs are valid.
#[unsafe(export_name = "sys$create")]
pub unsafe extern "C" fn create(fab: *mut Fab, err: *const c_void, suc: *const c_void) -> u32 {
    unsafe {
        fab_service(fab, err, suc, |f| {
            let nam = nam_of(f)?;
            let spec = parse(f, nam.as_deref())?;
            let existing = libvms::cli::session(|s| s.find(&spec))?;
            let (file, shown, st) = match existing {
                Ok((path, shown)) if f.fop & fop::CIF != 0 => {
                    let file = host::File::open(&path, fac(f), f.shr)?;
                    (file, shown, status::NORMAL)
                }
                found => {
                    let superseded = found.ok().filter(|_| f.fop & fop::SUP != 0);
                    let (path, shown, st) = match superseded {
                        Some((path, shown)) => {
                            std::fs::remove_file(&path).map_err(|_| status::SUPERSEDE)?;
                            (path, shown, status::SUPERSEDE)
                        }
                        None => {
                            let (p, s) = libvms::cli::session(|s| s.new_version(&spec))??;
                            let st = if f.fop & fop::CIF != 0 {
                                status::CREATED
                            } else {
                                status::NORMAL
                            };
                            (p, s, st)
                        }
                    };
                    let file =
                        host::File::create(&path, &design(f)?, fac(f) | host::fab::PUT, f.shr)?;
                    (file, shown, st)
                }
            };
            display(f, &file);
            if let Some(n) = nam {
                fill_name(n, &expanded(&spec)?, false)?;
                fill_name(n, &shown, true)?;
            }
            f.ifi = insert(file);
            Ok(st)
        })
    }
}

/// # Safety
/// `fab` is a FAB.
#[unsafe(export_name = "sys$close")]
pub unsafe extern "C" fn close(fab: *mut Fab, err: *const c_void, suc: *const c_void) -> u32 {
    unsafe {
        fab_service(fab, err, suc, |f| {
            let mut t = OPEN.lock().unwrap_or_else(|e| e.into_inner());
            let i = (f.ifi as usize).wrapping_sub(1);
            t.get_mut(i).and_then(Option::take).ok_or(status::IFI)?;
            f.ifi = 0;
            Ok(status::NORMAL)
        })
    }
}

/// # Safety
/// `fab` is an open file's FAB, its XABs valid.
#[unsafe(export_name = "sys$display")]
pub unsafe extern "C" fn display_(fab: *mut Fab, err: *const c_void, suc: *const c_void) -> u32 {
    unsafe {
        fab_service(fab, err, suc, |f| {
            let ifi = f.ifi;
            with_open(
                ifi,
                |o| {
                    display(f, &o.file);
                    Ok(status::NORMAL)
                },
                status::IFI,
            )
        })
    }
}

/// # Safety
/// As for [`open`].
#[unsafe(export_name = "sys$erase")]
pub unsafe extern "C" fn erase(fab: *mut Fab, err: *const c_void, suc: *const c_void) -> u32 {
    unsafe {
        fab_service(fab, err, suc, |f| {
            let nam = nam_of(f)?;
            let spec = parse(f, nam.as_deref())?;
            let (path, _) = libvms::cli::session(|s| s.find(&spec))??;
            libvms::files::delete(&path)?;
            Ok(status::NORMAL)
        })
    }
}

/// # Safety
/// `fab` is a FAB with a NAM.
#[unsafe(export_name = "sys$parse")]
pub unsafe extern "C" fn parse_(fab: *mut Fab, err: *const c_void, suc: *const c_void) -> u32 {
    unsafe {
        fab_service(fab, err, suc, |f| {
            let n = nam_of(f)?.ok_or(status::NAM)?;
            let spec = parse(f, Some(n))?;
            let e = expanded(&spec)?;
            let dir = libvms::cli::session(|s| s.locate(&spec))??;
            if !dir.first().is_some_and(|d| d.1.is_dir()) {
                return Err(status::DNF);
            }
            fill_name(n, &e, false)?;
            n.rsl = 0;
            let typed = text(bytes(f.fna, f.fns as usize));
            n.fnb = fnb_of(&typed, &spec);
            let key = n as *mut Nam as usize;
            let mut s = SEARCH.lock().unwrap_or_else(|e| e.into_inner());
            s.retain(|x| x.0 != key);
            s.push((key, (spec, None)));
            Ok(status::NORMAL)
        })
    }
}

/// # Safety
/// `fab` is a FAB whose NAM $PARSE filled.
#[unsafe(export_name = "sys$search")]
pub unsafe extern "C" fn search(fab: *mut Fab, err: *const c_void, suc: *const c_void) -> u32 {
    unsafe {
        fab_service(fab, err, suc, |f| {
            let n = nam_of(f)?.ok_or(status::NAM)?;
            let key = n as *mut Nam as usize;
            let mut s = SEARCH.lock().unwrap_or_else(|e| e.into_inner());
            let (spec, files) = &mut s.iter_mut().find(|x| x.0 == key).ok_or(status::NAM)?.1;
            let first = files.is_none();
            if first {
                let found = libvms::cli::session(|ses| ses.search_all(spec))?.unwrap_or_default();
                *files = Some(found.into_iter());
            }
            match files.as_mut().and_then(Iterator::next) {
                Some((_, shown)) => {
                    fill_name(n, &shown, true)?;
                    Ok(status::NORMAL)
                }
                None if first => Err(status::FNF),
                None => Err(status::NMF),
            }
        })
    }
}

/// A RAB service: the block checked, `op` run on its stream, STS set, ERR
/// or SUC called.
unsafe fn rab_service(
    rab: *mut Rab,
    err: *const c_void,
    suc: *const c_void,
    op: impl FnOnce(&mut Rab, &mut Open) -> Result<Cond, Cond>,
) -> u32 {
    let Some(r) = (unsafe { rab.as_mut() }).filter(|r| r.bid == RAB_BID) else {
        return status::RAB.0;
    };
    let isi = r.isi;
    let st = with_open(
        isi,
        |o| {
            if o.connected {
                op(r, o)
            } else {
                Err(status::ISI)
            }
        },
        status::ISI,
    )
    .unwrap_or_else(|e| e);
    r.sts = st.0;
    unsafe { complete(rab, st, err, suc) }
}

/// # Safety
/// `rab` is a RAB whose FAB is open.
#[unsafe(export_name = "sys$connect")]
pub unsafe extern "C" fn connect(rab: *mut Rab, err: *const c_void, suc: *const c_void) -> u32 {
    let Some(r) = (unsafe { rab.as_mut() }).filter(|r| r.bid == RAB_BID) else {
        return status::RAB.0;
    };
    let st = match unsafe { r.fab.as_ref() } {
        Some(f) if f.bid == FAB_BID => with_open(
            f.ifi,
            |o| {
                if o.connected {
                    return Err(status::ACT);
                }
                o.file.rewind();
                if r.rop & rop::EOF != 0 {
                    o.file.to_end()?;
                }
                o.connected = true;
                r.isi = f.ifi;
                Ok(status::NORMAL)
            },
            status::IFI,
        ),
        _ => Err(status::FAB),
    }
    .unwrap_or_else(|e| e);
    r.sts = st.0;
    unsafe { complete(rab, st, err, suc) }
}

/// # Safety
/// `rab` is a connected RAB.
#[unsafe(export_name = "sys$disconnect")]
pub unsafe extern "C" fn disconnect(rab: *mut Rab, err: *const c_void, suc: *const c_void) -> u32 {
    unsafe {
        rab_service(rab, err, suc, |r, o| {
            o.file.unlock();
            o.connected = false;
            r.isi = 0;
            Ok(status::NORMAL)
        })
    }
}

/// Where a $GET or $FIND goes: RAB$B_RAC, the key buffer, RAB$L_ROP.
unsafe fn at<'a>(r: &Rab, key: &'a mut [u8; 4]) -> At<'a> {
    match r.rac {
        RAC_KEY => {
            let m = if r.rop & rop::KGT != 0 {
                Match::Gt
            } else if r.rop & rop::KGE != 0 {
                Match::Ge
            } else {
                Match::Eq
            };
            let k = unsafe { bytes(r.kbf, r.ksz as usize) };
            // A relative file's key is its record number: four bytes at KBF.
            let k = if r.ksz == 0 && !r.kbf.is_null() {
                key.copy_from_slice(unsafe { bytes(r.kbf, 4) });
                &key[..]
            } else {
                k
            };
            At::Key(r.krf, k, m)
        }
        RAC_RFA => At::Rfa(vms_rms::Rfa {
            vbn: r.rfa0,
            id: r.rfa4,
        }),
        _ => At::Next,
    }
}

fn rop_of(r: &Rab) -> Rop {
    Rop {
        nolock: r.rop & rop::NLK != 0,
        wait: r.rop & rop::WAT != 0,
    }
}

/// # Safety
/// `rab` is a connected RAB; UBF holds USZ bytes, RHB the VFC control.
#[unsafe(export_name = "sys$get")]
pub unsafe extern "C" fn get(rab: *mut Rab, err: *const c_void, suc: *const c_void) -> u32 {
    unsafe {
        rab_service(rab, err, suc, |r, o| {
            let mut key = [0; 4];
            let (rfa, rec) = o.file.find(at(r, &mut key), rop_of(r))?;
            (r.rfa0, r.rfa4) = (rfa.vbn, rfa.id);
            if !r.rhb.is_null() && !rec.control.is_empty() {
                std::ptr::copy_nonoverlapping(rec.control.as_ptr(), r.rhb, rec.control.len());
            }
            let n = rec.data.len().min(r.usz as usize);
            if n > 0 && r.ubf.is_null() {
                return Err(status::UBF);
            }
            if n > 0 {
                std::ptr::copy_nonoverlapping(rec.data.as_ptr(), r.ubf, n);
            }
            r.rbf = r.ubf;
            r.rsz = n as u16;
            if n < rec.data.len() {
                r.stv = rec.data.len() as u32;
                return Err(status::RTB);
            }
            Ok(status::NORMAL)
        })
    }
}

/// # Safety
/// `rab` is a connected RAB.
#[unsafe(export_name = "sys$find")]
pub unsafe extern "C" fn find(rab: *mut Rab, err: *const c_void, suc: *const c_void) -> u32 {
    unsafe {
        rab_service(rab, err, suc, |r, o| {
            let mut key = [0; 4];
            let (rfa, _) = o.file.find(at(r, &mut key), rop_of(r))?;
            (r.rfa0, r.rfa4) = (rfa.vbn, rfa.id);
            Ok(status::NORMAL)
        })
    }
}

/// The record RBF and RSZ (and RHB, for VFC) give.
unsafe fn record(r: &Rab, o: &Open) -> Record {
    let fsz = if o.file.fab.rfm == Rfm::Vfc {
        o.file.fab.fsz.max(2)
    } else {
        0
    };
    Record {
        control: unsafe { bytes(r.rhb, if r.rhb.is_null() { 0 } else { fsz as usize }) }.to_vec(),
        data: unsafe { bytes(r.rbf, r.rsz as usize) }.to_vec(),
    }
}

/// # Safety
/// `rab` is a connected RAB; RBF holds RSZ bytes.
#[unsafe(export_name = "sys$put")]
pub unsafe extern "C" fn put(rab: *mut Rab, err: *const c_void, suc: *const c_void) -> u32 {
    unsafe {
        rab_service(rab, err, suc, |r, o| {
            let rec = record(r, o);
            let key = (r.rac == RAC_KEY && o.file.fab.org == Org::Rel && !r.kbf.is_null())
                .then(|| u32::from_le_bytes(bytes(r.kbf, 4).try_into().unwrap()));
            match o.file.put(&rec, key) {
                // RAB$V_UIF: an existing record is updated instead.
                Err(e) if e == status::REX && r.rop & rop::UIF != 0 => {
                    let k = key.unwrap_or(0).to_le_bytes();
                    o.file.find(At::Key(0, &k, Match::Eq), rop_of(r))?;
                    o.file.update(&rec)
                }
                st => {
                    if let (Some(n), Ok(_)) = (key, &st) {
                        (r.rfa0, r.rfa4) = (n, 0);
                    }
                    st
                }
            }
        })
    }
}

/// # Safety
/// `rab` is a connected RAB; RBF holds RSZ bytes.
#[unsafe(export_name = "sys$update")]
pub unsafe extern "C" fn update(rab: *mut Rab, err: *const c_void, suc: *const c_void) -> u32 {
    unsafe {
        rab_service(rab, err, suc, |r, o| {
            let rec = record(r, o);
            o.file.update(&rec)
        })
    }
}

/// # Safety
/// `rab` is a connected RAB.
#[unsafe(export_name = "sys$delete")]
pub unsafe extern "C" fn delete(rab: *mut Rab, err: *const c_void, suc: *const c_void) -> u32 {
    unsafe { rab_service(rab, err, suc, |_, o| o.file.delete()) }
}

/// # Safety
/// `rab` is a connected RAB.
#[unsafe(export_name = "sys$rewind")]
pub unsafe extern "C" fn rewind(rab: *mut Rab, err: *const c_void, suc: *const c_void) -> u32 {
    unsafe {
        rab_service(rab, err, suc, |_, o| {
            o.file.rewind();
            Ok(status::NORMAL)
        })
    }
}

/// # Safety
/// `rab` is a connected RAB.
#[unsafe(export_name = "sys$free")]
pub unsafe extern "C" fn free(rab: *mut Rab, err: *const c_void, suc: *const c_void) -> u32 {
    unsafe {
        rab_service(rab, err, suc, |_, o| {
            o.file.unlock();
            Ok(status::NORMAL)
        })
    }
}

/// # Safety
/// `rab` is a connected RAB.
#[unsafe(export_name = "sys$release")]
pub unsafe extern "C" fn release(rab: *mut Rab, err: *const c_void, suc: *const c_void) -> u32 {
    unsafe { free(rab, err, suc) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{offset_of, size_of};

    /// The C headers' layouts are these structs': C says where each field
    /// is, and the sizes.
    #[test]
    fn layouts_agree() {
        macro_rules! fields {
            ($c:literal, $t:ty, $($cf:literal => $($rf:ident).+),*) => {
                vec![
                    (format!("sizeof(struct {})", $c), size_of::<$t>()),
                    $((format!("offsetof(struct {}, {})", $c, $cf), offset_of!($t, $($rf).+)),)*
                ]
            };
        }
        let mut want = Vec::new();
        want.extend(
            fields!("FAB", Fab, "fab$w_ifi" => ifi, "fab$l_fop" => fop, "fab$l_alq" => alq,
            "fab$b_fac" => fac, "fab$l_ctx" => ctx, "fab$b_rfm" => rfm, "fab$l_jnl" => jnl,
            "fab$l_xab" => xab, "fab$l_nam" => nam, "fab$l_fna" => fna, "fab$l_dna" => dna,
            "fab$b_fns" => fns, "fab$w_mrs" => mrs, "fab$l_mrn" => mrn, "fab$w_bls" => bls,
            "fab$b_fsz" => fsz, "fab$l_dev" => dev, "fab$l_sdc" => sdc, "fab$w_gbc" => gbc,
            "fab$b_rcf" => rcf),
        );
        want.extend(fields!("RAB", Rab, "rab$l_rop" => rop, "rab$l_stv" => stv,
            "rab$w_rfa" => rfa0, "rab$l_rfa0" => rfa0, "rab$w_rfa4" => rfa4, "rab$l_ctx" => ctx,
            "rab$b_rac" => rac, "rab$w_usz" => usz, "rab$w_rsz" => rsz, "rab$l_ubf" => ubf,
            "rab$l_rbf" => rbf, "rab$l_rhb" => rhb, "rab$l_kbf" => kbf, "rab$b_ksz" => ksz,
            "rab$b_krf" => krf, "rab$b_mbc" => mbc, "rab$l_bkt" => bkt, "rab$l_fab" => fab,
            "rab$l_xab" => xab));
        want.extend(fields!("NAM", Nam, "nam$b_rsl" => rsl, "nam$l_fnb" => fnb,
            "nam$l_rlf" => rlf, "nam$l_rsa" => rsa, "nam$l_esa" => esa, "nam$b_ess" => ess,
            "nam$b_rfs" => rfs, "nam$l_wcc" => wcc, "nam$w_fid" => fid, "nam$w_did" => did,
            "nam$t_dvi" => dvi, "nam$b_node" => lens, "nam$l_node" => ptrs));
        want.extend(
            fields!("XABKEY", XabKey, "xab$l_nxt" => head.nxt, "xab$b_ian" => ian,
            "xab$b_dbs" => dbs, "xab$b_flg" => flg, "xab$b_dtp" => dtp, "xab$l_rvb" => rvb,
            "xab$b_nsg" => nsg, "xab$b_ref" => r#ref, "xab$w_mrl" => mrl, "xab$w_dfl" => dfl,
            "xab$w_pos0" => pos, "xab$b_siz0" => siz, "xab$b_prolog" => prolog,
            "xab$l_knm" => knm, "xab$l_dvb" => dvb, "xab$b_typ0" => typ),
        );
        want.extend(
            fields!("XABALL", XabAll, "xab$b_aop" => aop, "xab$w_vol" => vol,
            "xab$l_loc" => loc, "xab$l_alq" => alq, "xab$w_deq" => deq, "xab$b_bkz" => bkz,
            "xab$b_aid" => aid, "xab$w_rfi" => rfi),
        );
        want.extend(
            fields!("XABDAT", XabDat, "xab$w_rvn" => rvn, "xab$q_cdt" => cdt,
            "xab$q_rdt" => rdt, "xab$q_edt" => edt, "xab$q_bdt" => bdt, "xab$q_acc" => acc,
            "xab$q_att" => att, "xab$q_mod" => r#mod),
        );
        want.extend(
            fields!("XABFHC", XabFhc, "xab$b_rfo" => rfo, "xab$w_lrl" => lrl,
            "xab$l_hbk" => hbk, "xab$l_ebk" => ebk, "xab$w_ffb" => ffb, "xab$b_hsz" => hsz,
            "xab$w_mrz" => mrz, "xab$w_gbc" => gbc, "xab$w_verlimit" => verlimit,
            "xab$l_sbn" => sbn),
        );
        want.extend(
            fields!("XABPRO", XabPro, "xab$w_pro" => pro, "xab$b_prot_opt" => prot_opt,
            "xab$l_uic" => uic),
        );
        want.extend(
            fields!("XABSUM", XabSum, "xab$b_noa" => noa, "xab$b_nok" => nok,
            "xab$w_pvn" => pvn),
        );

        let dir = std::env::temp_dir().join(format!("vpt-layout-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut c = String::from(
            "#include <stdio.h>\n#include <stddef.h>\n#include <rms.h>\nint main(void) {\n",
        );
        for (expr, _) in &want {
            c += &format!("    printf(\"%zu\\n\", (size_t){expr});\n");
        }
        c += "    return 0;\n}\n";
        std::fs::write(dir.join("layout.c"), c).unwrap();
        let include = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../include");
        let cc = std::process::Command::new("cc")
            .args(["-Wall", "-Werror", "-I"])
            .arg(&include)
            .arg(dir.join("layout.c"))
            .arg("-o")
            .arg(dir.join("layout"))
            .output()
            .unwrap();
        assert!(
            cc.status.success(),
            "{}",
            String::from_utf8_lossy(&cc.stderr)
        );
        let out = std::process::Command::new(dir.join("layout"))
            .output()
            .unwrap();
        let got: Vec<usize> = String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|l| l.parse().unwrap())
            .collect();
        let bad: Vec<String> = want
            .iter()
            .zip(&got)
            .filter(|((_, w), g)| w != *g)
            .map(|((e, w), g)| format!("{e}: Rust {w}, C {g}"))
            .collect();
        assert!(bad.is_empty(), "{}", bad.join("\n"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
