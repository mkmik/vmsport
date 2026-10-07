//! The lock manager for C: sys$enq, sys$enqw and sys$deq over vmsportd's,
//! on one connection for the process, so its locks go when it ends.
//!
//! ponytail: $ENQ completes before it returns, as $ENQW does (waiting if it
//! must), then sets the AST off; no event flags, blocking ASTs or lock
//! value blocks (LCK$M_VALBLK is SS$_BADPARAM). Parent locks qualify the
//! resource name; access modes and LCK$M_SYSTEM don't.

use crate::Desc;
use std::ffi::c_void;
use std::sync::Mutex;
use vms_cond::Cond;
use vmsportd::Client;
use vmsportd::locks::{self, Mode};

const NORMAL: Cond = Cond(1);
const BADPARAM: Cond = Cond(0x14);
const IVBUFLEN: Cond = Cond(844);
const ABORT: Cond = Cond(0x2C);

/// LCK$M_ flags.
const VALBLK: u32 = 0x1;
const CONVERT: u32 = 0x2;
const NOQUEUE: u32 = 0x4;
const SYNCSTS: u32 = 0x8;
const XVALBLK: u32 = 0x1_0000;
/// $DEQ's LCK$M_DEQALL.
const DEQALL: u32 = 0x1;

#[repr(C)]
pub struct Lksb {
    pub status: u16,
    pub reserved: u16,
    pub lkid: u32,
    pub valblk: [u8; 16],
}

/// The process's connection (remade in a forked child, whose inherited one
/// is its parent's) and the locks it took.
struct Conn {
    pid: u32,
    client: Client,
    ids: Vec<u32>,
}

static CONN: Mutex<Option<Conn>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut Conn) -> Result<R, Cond>) -> Result<R, Cond> {
    let mut c = CONN.lock().unwrap_or_else(|e| e.into_inner());
    let pid = std::process::id();
    if c.as_ref().is_none_or(|c| c.pid != pid) {
        // A forked child closes its copy of the parent's socket; the
        // connection, and the parent's locks, stay the parent's.
        drop(c.take());
        let client = Client::connect().map_err(|_| ABORT)?;
        *c = Some(Conn {
            pid,
            client,
            ids: Vec::new(),
        });
    }
    f(c.as_mut().unwrap())
}

const MODES: [Mode; 6] = [Mode::NL, Mode::CR, Mode::CW, Mode::PR, Mode::PW, Mode::EX];

/// # Safety
/// `lksb` is a lock status block, `resnam` a descriptor (or null for a
/// conversion); `astadr`, if given, a routine taking `astprm`.
#[unsafe(export_name = "sys$enqw")]
pub unsafe extern "C" fn enqw(
    _efn: u32,
    lkmode: u32,
    lksb: *mut Lksb,
    flags: u32,
    resnam: *const Desc,
    parid: u32,
    astadr: *const c_void,
    astprm: usize,
    _blkast: *const c_void,
    _acmode: u32,
    _rsdm_id: u32,
) -> u32 {
    let Some(l) = (unsafe { lksb.as_mut() }) else {
        return BADPARAM.0;
    };
    let Some(&mode) = MODES.get(lkmode as usize) else {
        return BADPARAM.0;
    };
    if flags & (VALBLK | XVALBLK) != 0 {
        return BADPARAM.0;
    }
    let noqueue = flags & NOQUEUE != 0;
    let r = if flags & CONVERT != 0 {
        with(|c| c.client.convert(l.lkid, mode, noqueue))
    } else {
        let name = unsafe { crate::text(resnam) }.unwrap_or_default();
        if name.is_empty() || name.len() > 31 {
            return IVBUFLEN.0;
        }
        // User locks apart from RMS's, under their parent's.
        let res = match parid {
            0 => format!("LCK${name}"),
            p => format!("LCK${p:X}${name}"),
        };
        with(|c| {
            let (id, st) = c.client.enq(&res, mode, noqueue)?;
            c.ids.push(id);
            l.lkid = id;
            Ok(st)
        })
    };
    let st = match r {
        Ok(st) => st,
        Err(e) => return e.0,
    };
    l.status = NORMAL.0 as u16;
    if !astadr.is_null() {
        // SAFETY: the caller passed an AST routine taking its parameter.
        let f: unsafe extern "C" fn(usize) = unsafe { std::mem::transmute(astadr) };
        unsafe { f(astprm) };
    }
    if st == locks::SYNCH && flags & SYNCSTS != 0 {
        st.0
    } else {
        NORMAL.0
    }
}

/// # Safety
/// As for [`enqw`].
#[unsafe(export_name = "sys$enq")]
pub unsafe extern "C" fn enq(
    efn: u32,
    lkmode: u32,
    lksb: *mut Lksb,
    flags: u32,
    resnam: *const Desc,
    parid: u32,
    astadr: *const c_void,
    astprm: usize,
    blkast: *const c_void,
    acmode: u32,
    rsdm_id: u32,
) -> u32 {
    unsafe {
        enqw(
            efn, lkmode, lksb, flags, resnam, parid, astadr, astprm, blkast, acmode, rsdm_id,
        )
    }
}

/// $DEQ of lock `lkid`, or with LCK$M_DEQALL every lock the process has.
///
/// # Safety
/// `valblk` is ignored.
#[unsafe(export_name = "sys$deq")]
pub unsafe extern "C" fn deq(lkid: u32, _valblk: *mut u8, _acmode: u32, flags: u32) -> u32 {
    let r = with(|c| {
        if flags & DEQALL != 0 && lkid == 0 {
            for id in std::mem::take(&mut c.ids) {
                let _ = c.client.deq(id);
            }
            return Ok(NORMAL);
        }
        let st = c.client.deq(lkid)?;
        c.ids.retain(|i| *i != lkid);
        Ok(st)
    });
    r.unwrap_or_else(|e| e).0
}
