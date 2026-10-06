//! The few host calls std doesn't have: local time, extended attributes,
//! process facts.

use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

/// Local time as a VMS time (100 ns since 17-NOV-1858, local).
pub fn now() -> i64 {
    let d = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = d.as_secs() as i64;
    vms_time::UNIX_EPOCH
        + (secs + utc_offset(secs)) * 10_000_000
        + i64::from(d.subsec_nanos() / 100)
}

/// A Unix time as a VMS time, local.
pub fn vms_time(t: std::time::SystemTime) -> i64 {
    let d = t.duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let secs = d.as_secs() as i64;
    vms_time::UNIX_EPOCH
        + (secs + utc_offset(secs)) * 10_000_000
        + i64::from(d.subsec_nanos() / 100)
}

/// Seconds east of UTC at Unix time `t`.
fn utc_offset(t: i64) -> i64 {
    // SAFETY: localtime_r writes into `tm`, which we own.
    unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        let tt = t as libc::time_t;
        if libc::localtime_r(&tt, &mut tm).is_null() {
            return 0;
        }
        tm.tm_gmtoff as i64
    }
}

fn cpath(p: &Path) -> Option<CString> {
    CString::new(p.as_os_str().as_bytes()).ok()
}

/// An extended attribute, if the file has it.
pub fn get_xattr(p: &Path, name: &str) -> Option<Vec<u8>> {
    let (path, name) = (cpath(p)?, CString::new(name).ok()?);
    let mut buf = vec![0u8; 512];
    // SAFETY: the buffer is as long as we say; the strings are NUL-terminated.
    let n = unsafe {
        #[cfg(target_os = "macos")]
        {
            libc::getxattr(
                path.as_ptr(),
                name.as_ptr(),
                buf.as_mut_ptr().cast(),
                buf.len(),
                0,
                0,
            )
        }
        #[cfg(not(target_os = "macos"))]
        {
            libc::getxattr(
                path.as_ptr(),
                name.as_ptr(),
                buf.as_mut_ptr().cast(),
                buf.len(),
            )
        }
    };
    if n < 0 {
        return None;
    }
    buf.truncate(n as usize);
    Some(buf)
}

pub fn set_xattr(p: &Path, name: &str, value: &[u8]) -> std::io::Result<()> {
    let (Some(path), Ok(name)) = (cpath(p), CString::new(name)) else {
        return Err(std::io::ErrorKind::InvalidInput.into());
    };
    // SAFETY: as above.
    let r = unsafe {
        #[cfg(target_os = "macos")]
        {
            libc::setxattr(
                path.as_ptr(),
                name.as_ptr(),
                value.as_ptr().cast(),
                value.len(),
                0,
                0,
            )
        }
        #[cfg(not(target_os = "macos"))]
        {
            libc::setxattr(
                path.as_ptr(),
                name.as_ptr(),
                value.as_ptr().cast(),
                value.len(),
                0,
            )
        }
    };
    if r < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// F$GETJPI / F$GETSYI items vmsport can answer.
pub fn info(item: &str) -> Option<String> {
    // SAFETY: plain getters.
    let (uid, gid, pid) = unsafe { (libc::getuid(), libc::getgid(), libc::getpid()) };
    Some(match item {
        "USERNAME" => std::env::var("USER")
            .unwrap_or_default()
            .to_ascii_uppercase(),
        "PID" | "MASTER_PID" => format!("{pid:08X}"),
        "UIC" => format!("[{gid:o},{uid:o}]"),
        "GRP" => gid.to_string(),
        "MEM" => uid.to_string(),
        "NODENAME" | "SCSNODE" => hostname(),
        "VERSION" => "V8.4-2L1".into(),
        "ARCH_NAME" => if cfg!(target_arch = "aarch64") {
            "ARM64"
        } else {
            "x86_64"
        }
        .into(),
        "PRCNAM" => std::env::var("USER")
            .unwrap_or_default()
            .to_ascii_uppercase(),
        "MODE" => "INTERACTIVE".into(),
        _ => return None,
    })
}

fn hostname() -> String {
    let mut buf = [0u8; 256];
    // SAFETY: the buffer is as long as we say.
    unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) };
    let s =
        String::from_utf8_lossy(&buf[..buf.iter().position(|&b| b == 0).unwrap_or(0)]).to_string();
    s.split('.').next().unwrap_or("").to_ascii_uppercase()
}
