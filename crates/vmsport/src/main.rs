//! vmsport: VMS names for Unix tools.
//!
//!     vmsport path SPEC    the host path of a file or directory
//!     vmsport spec PATH    the VMS spec of a host path
//!     vmsport cdu FILE.CLD [-o FILE.c]
//!                          command tables as C (SET COMMAND/OBJECT), for
//!                          cli$dcl_parse; named after the CLD's MODULE

use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let r = match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["path", spec] => path(spec),
        ["spec", p] => Ok(spec(Path::new(p))),
        ["cdu", cld] => cdu(cld, None),
        ["cdu", cld, "-o", out] => cdu(cld, Some(out)),
        _ => Err(
            "usage: vmsport path SPEC | vmsport spec PATH | vmsport cdu FILE.CLD [-o FILE.c]"
                .to_string(),
        ),
    };
    match r {
        Ok(s) if s.is_empty() => {}
        Ok(s) => println!("{}", s.trim_end()),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}

/// The CLD as C: `const char MODULE[]`, or FILE_TABLES without a MODULE.
fn cdu(cld: &str, out: Option<&str>) -> Result<String, String> {
    let text = std::fs::read_to_string(cld)
        .map_err(|e| format!("%CDU-E-OPENIN, error opening {cld} as input: {e}"))?;
    let t = vms_cld::compile(&text).map_err(|e| format!("%CDU-E-SYNTAX, {cld}: {e}"))?;
    let stem = Path::new(cld)
        .file_stem()
        .map(|s| s.to_string_lossy().to_ascii_uppercase())
        .unwrap_or_default();
    let name = t.module.clone().unwrap_or(format!("{stem}_TABLES"));
    let c = t.to_c(&name);
    match out {
        Some(o) => std::fs::write(o, c)
            .map(|_| String::new())
            .map_err(|e| format!("%CDU-E-OPENOUT, {o}: {e}")),
        None => Ok(c),
    }
}

fn path(spec: &str) -> Result<String, String> {
    let s = libvms::Session::new()
        .map_err(|e| format!("%VMSPORT-F-NODAEMON, cannot reach vmsportd: {e}"))?;
    let catalog = s.catalog();
    let msg = |c| catalog.get_msg(c, vms_msg::Flags::ALL);
    let parsed = s.parse(spec, "", "").map_err(msg)?;
    // A device or directory alone: the directory.
    let named = !parsed.name.is_empty() || parsed.typ.as_deref().is_some_and(|t| !t.is_empty());
    if !spec.contains(['.', ';']) && parsed.name.is_empty() || !named {
        let (_, dir) = s
            .locate(&parsed)
            .map_err(msg)?
            .into_iter()
            .next()
            .ok_or_else(|| msg(libvms::status::DEV))?;
        return Ok(dir.display().to_string());
    }
    let (p, _) = s.find(&parsed).map_err(msg)?;
    Ok(p.display().to_string())
}

fn spec(p: &Path) -> String {
    let abs = std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf());
    if abs.is_dir() {
        return vmsportd::host_dir(&abs, false);
    }
    let dir = vmsportd::host_dir(abs.parent().unwrap_or(Path::new("/")), false);
    let file = abs
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_default();
    let (name, typ, ver) = libvms::split_host(&file);
    format!(
        "{dir}{}.{};{ver}",
        vms_filespec::escape(&name),
        vms_filespec::escape(&typ)
    )
}
