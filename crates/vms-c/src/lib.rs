//! The C ABI: CLI$, LIB$ and SYS$ routines under their VMS names, over
//! libvms::cli, for C programs built with $VMSPORT/include and -lvms.
//! See docs/design/m2.md.
//!
//! Strings come and go in descriptors. Optional trailing arguments are
//! null (the headers pad omitted ones); lib$signal and lib$stop are
//! variadic in C only, in a header wrapper that calls [`signal`].

use libvms::cli;
use std::ffi::{CStr, c_char};
use vms_cond::Cond;

/// `struct dsc$descriptor_s` (and `_d`).
#[repr(C)]
pub struct Desc {
    len: u16,
    dtype: u8,
    class: u8,
    ptr: *mut u8,
}

const CLASS_D: u8 = 2;

/// A descriptor's text; `None` for a null descriptor.
unsafe fn text(d: *const Desc) -> Option<String> {
    let d = unsafe { d.as_ref() }?;
    if d.ptr.is_null() {
        return Some(String::new());
    }
    let bytes = unsafe { std::slice::from_raw_parts(d.ptr, d.len as usize) };
    Some(String::from_utf8_lossy(bytes).into_owned())
}

/// Stores `s` in a descriptor: a fixed one gets it blank-padded (and
/// LIB$_STRTRU if it doesn't fit), a dynamic one is resized. `len`, if
/// given, gets the length stored.
unsafe fn store(d: *mut Desc, s: &str, len: *mut u16) -> Option<Cond> {
    let d = unsafe { d.as_mut() }?;
    let b = s.as_bytes();
    let n = if d.class == CLASS_D {
        let n = b.len().min(u16::MAX as usize);
        let p = unsafe { libc::realloc(d.ptr.cast(), n.max(1)) }.cast::<u8>();
        if p.is_null() {
            return Some(cli::status::LIB_INVARG);
        }
        (d.ptr, d.len) = (p, n as u16);
        n
    } else {
        b.len().min(d.len as usize)
    };
    unsafe {
        std::ptr::copy_nonoverlapping(b.as_ptr(), d.ptr, n);
        if d.class != CLASS_D {
            std::ptr::write_bytes(d.ptr.add(n), b' ', d.len as usize - n);
        }
        if let Some(l) = len.as_mut() {
            *l = n as u16;
        }
    }
    (n < b.len()).then_some(cli::status::LIB_STRTRU)
}

/// C's stdout buffer first, so output keeps its order.
fn flush_c() {
    // SAFETY: fflush(NULL) flushes every C stream.
    unsafe { libc::fflush(std::ptr::null_mut()) };
}

/// # Safety
/// `entity` is a valid descriptor.
#[unsafe(export_name = "cli$present")]
pub unsafe extern "C" fn present(entity: *const Desc) -> u32 {
    let name = unsafe { text(entity) }.unwrap_or_default();
    cli::present(&name).0
}

/// # Safety
/// The descriptors are valid; `retlen` may be null.
#[unsafe(export_name = "cli$get_value")]
pub unsafe extern "C" fn get_value(entity: *const Desc, ret: *mut Desc, retlen: *mut u16) -> u32 {
    let name = unsafe { text(entity) }.unwrap_or_default();
    match cli::get_value(&name) {
        Ok((v, st)) => {
            unsafe { store(ret, &v, retlen) };
            st.0
        }
        Err(st) => st.0,
    }
}

/// `table` is the CLD text (`vmsport cdu` makes it a C array); a null
/// `command` parses the command DCL ran us with, or the argv. The prompt
/// arguments are taken but not used.
///
/// # Safety
/// `command` is null or a valid descriptor; `table` a NUL-terminated string.
#[unsafe(export_name = "cli$dcl_parse")]
pub unsafe extern "C" fn dcl_parse(
    command: *const Desc,
    table: *const c_char,
    _param_routine: *const u8,
    _prompt_routine: *const u8,
    _prompt: *const Desc,
) -> u32 {
    if table.is_null() {
        return cli::status::CLI_INVTAB.0;
    }
    let tables = unsafe { CStr::from_ptr(table) }.to_string_lossy();
    let cmd = unsafe { text(command) };
    flush_c();
    cli::dcl_parse(cmd.as_deref(), &tables).0
}

/// # Safety
/// The descriptors are valid or null.
#[unsafe(export_name = "lib$get_foreign")]
pub unsafe extern "C" fn get_foreign(
    ret: *mut Desc,
    prompt: *const Desc,
    retlen: *mut u16,
    _flags: *mut u32,
) -> u32 {
    let p = unsafe { text(prompt) };
    flush_c();
    let line = cli::get_foreign(p.as_deref());
    match unsafe { store(ret, &line, retlen) } {
        Some(st) if st == cli::status::LIB_STRTRU => 1409564, // LIB$_INPSTRTRU
        Some(st) => st.0,
        None => cli::status::SS_NORMAL.0,
    }
}

/// # Safety
/// `d` is a valid descriptor.
#[unsafe(export_name = "lib$put_output")]
pub unsafe extern "C" fn put_output(d: *const Desc) -> u32 {
    flush_c();
    cli::put_output(&unsafe { text(d) }.unwrap_or_default()).0
}

/// LIB$K_CLI_GLOBAL_SYM; LIB$K_CLI_LOCAL_SYM is 1.
const GLOBAL: u32 = 2;

/// # Safety
/// The descriptors are valid; `retlen` and `table` may be null.
#[unsafe(export_name = "lib$get_symbol")]
pub unsafe extern "C" fn get_symbol(
    sym: *const Desc,
    ret: *mut Desc,
    retlen: *mut u16,
    table: *mut u32,
) -> u32 {
    let name = unsafe { text(sym) }.unwrap_or_default();
    match cli::get_symbol(&name) {
        Ok((v, global)) => unsafe {
            if let Some(t) = table.as_mut() {
                *t = if global { GLOBAL } else { 1 };
            }
            store(ret, &v, retlen).unwrap_or(cli::status::SS_NORMAL).0
        },
        Err(st) => st.0,
    }
}

/// # Safety
/// The descriptors are valid; `table` may be null (local).
#[unsafe(export_name = "lib$set_symbol")]
pub unsafe extern "C" fn set_symbol(
    sym: *const Desc,
    value: *const Desc,
    table: *const u32,
) -> u32 {
    let (name, v) = unsafe {
        (
            text(sym).unwrap_or_default(),
            text(value).unwrap_or_default(),
        )
    };
    let global = unsafe { table.as_ref() } == Some(&GLOBAL);
    cli::set_symbol(&name, &v, global).0
}

/// # Safety
/// `sym` is a valid descriptor; `table` may be null (local).
#[unsafe(export_name = "lib$delete_symbol")]
pub unsafe extern "C" fn delete_symbol(sym: *const Desc, table: *const u32) -> u32 {
    let name = unsafe { text(sym) }.unwrap_or_default();
    let global = unsafe { table.as_ref() } == Some(&GLOBAL);
    cli::delete_symbol(&name, global).0
}

#[unsafe(export_name = "sys$exit")]
pub extern "C" fn exit(code: u32) -> ! {
    flush_c();
    cli::exit(Cond(code))
}

/// `outadr`, if given, gets four bytes of message information: zeros here.
///
/// # Safety
/// `buf` is a valid descriptor; `msglen` and `outadr` may be null.
#[unsafe(export_name = "sys$getmsg")]
pub unsafe extern "C" fn getmsg(
    msgid: u32,
    msglen: *mut u16,
    buf: *mut Desc,
    flags: u32,
    outadr: *mut u8,
) -> u32 {
    let m = cli::getmsg(Cond(msgid), flags);
    unsafe {
        if !outadr.is_null() {
            std::ptr::write_bytes(outadr, 0, 4);
        }
        match store(buf, &m, msglen) {
            Some(_) => 1537, // SS$_BUFFEROVF
            None => cli::status::SS_NORMAL.0,
        }
    }
}

/// What an FAO directive takes: a number, a string (descriptor, counted or
/// zero-terminated), or a length and characters.
#[derive(Clone, Copy)]
enum Kind {
    Num,
    Desc,
    Counted,
    Zero,
    LenChars,
}

/// The arguments a control string takes, in order.
// ponytail: !n(...) repeats and !%D/!%T times (by reference on VMS) read
// as numbers; add them when a message needs them.
fn kinds(ctl: &str) -> Vec<Kind> {
    let cs: Vec<char> = ctl.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < cs.len() {
        if cs[i] != '!' {
            i += 1;
            continue;
        }
        i += 1;
        if cs.get(i) == Some(&'#') {
            out.push(Kind::Num);
            i += 1;
        }
        while cs.get(i).is_some_and(char::is_ascii_digit) {
            i += 1;
        }
        let k = match (cs.get(i), cs.get(i + 1)) {
            (Some('A'), Some('S')) => Some(Kind::Desc),
            (Some('A'), Some('C')) => Some(Kind::Counted),
            (Some('A'), Some('Z')) => Some(Kind::Zero),
            (Some('A'), Some('D' | 'F')) => Some(Kind::LenChars),
            (Some('%'), Some('S')) | (Some('*'), _) => None,
            (Some('%'), Some(_)) => Some(Kind::Num),
            (Some(c), Some(_)) if "OXZUS".contains(*c) => Some(Kind::Num),
            (Some('+'), _) => Some(Kind::Num),
            _ => None,
        };
        out.extend(k);
        i += 1;
    }
    out
}

enum Owned {
    Num(i64),
    Str(String),
}

/// One FAO argument out of `w` (the next words), as `k` says to read it.
unsafe fn arg(k: Kind, w: &mut impl Iterator<Item = isize>) -> Owned {
    let word = w.next().unwrap_or(0);
    let s = |p: *const u8, n: usize| unsafe {
        String::from_utf8_lossy(std::slice::from_raw_parts(p, n)).into_owned()
    };
    match k {
        // Numbers are longwords: the low 32 bits of the word.
        Kind::Num => Owned::Num(word as u32 as i64),
        Kind::Desc => Owned::Str(unsafe { text(word as *const Desc) }.unwrap_or_default()),
        Kind::Counted if word != 0 => {
            let p = word as *const u8;
            Owned::Str(s(unsafe { p.add(1) }, unsafe { *p } as usize))
        }
        Kind::Zero if word != 0 => Owned::Str(
            unsafe { CStr::from_ptr(word as *const c_char) }
                .to_string_lossy()
                .into_owned(),
        ),
        Kind::LenChars => {
            let p = w.next().unwrap_or(0) as *const u8;
            Owned::Str(if p.is_null() {
                String::new()
            } else {
                s(p, word as u32 as usize)
            })
        }
        _ => Owned::Str(String::new()),
    }
}

/// lib$signal and lib$stop: `words` is the C arguments as pointer-sized
/// words, each condition followed by its FAO count and arguments. The
/// header's variadic wrapper calls this.
///
/// # Safety
/// `words` holds `count` words; the FAO arguments point where their
/// directives say.
#[unsafe(export_name = "vms$signal")]
pub unsafe extern "C" fn signal(stop: i32, words: *const isize, count: i32) -> u32 {
    let words = unsafe { std::slice::from_raw_parts(words, count.max(0) as usize) };
    let mut w = words.iter().copied();
    let mut conds: Vec<(Cond, Vec<Owned>)> = Vec::new();
    while let Some(c) = w.next() {
        let cond = Cond(c as u32);
        let n = w.next().unwrap_or(0) as u32 as usize;
        let ctl = cli::getmsg(cond, 1);
        let mut ks = kinds(&ctl);
        ks.resize(ks.len().max(n), Kind::Num);
        let mut args = Vec::new();
        let mut taken = (&mut w).take(n);
        for k in ks.into_iter().take(n) {
            args.push(unsafe { arg(k, &mut taken) });
        }
        conds.push((cond, args));
    }
    let conds: Vec<(Cond, Vec<vms_fao::Arg>)> = conds
        .iter()
        .map(|(c, a)| {
            let a = a
                .iter()
                .map(|x| match x {
                    Owned::Num(n) => vms_fao::Arg::Num(*n),
                    Owned::Str(s) => vms_fao::Arg::Str(s),
                })
                .collect();
            (*c, a)
        })
        .collect();
    flush_c();
    if stop != 0 {
        cli::stop(&conds)
    } else {
        cli::signal(&conds).0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fao_kinds() {
        let k = kinds("!AS deleted (!UL block!%S) !5AS !#UL !AD !+");
        let names: Vec<&str> = k
            .iter()
            .map(|k| match k {
                Kind::Num => "n",
                Kind::Desc => "d",
                Kind::Counted => "c",
                Kind::Zero => "z",
                Kind::LenChars => "l",
            })
            .collect();
        assert_eq!(names, ["d", "n", "d", "n", "n", "l", "n"]);
    }
}
