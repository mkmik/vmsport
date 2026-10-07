//! Files-11 images as devices. MOUNT copies a volume's files out to a host
//! tree in the run directory (`mnt/DKA100/root`), the device then names
//! that tree, and DISMOUNT writes what changed back into the image. The run
//! directory is the mount table, so all of the user's processes see it.
//!
//! ponytail: the whole volume is copied at MOUNT and nothing goes back
//! before DISMOUNT; stage files as they are opened when images get big.

use crate::{files, split_host, sys};
use ods_image::{Conversion, Fid, Image, MFD, RecordAttrs, fch};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use vms_cond::Cond;
use vms_rms::{Fab, Org, Rfm};

pub const DEVMOUNT: Cond = Cond(0x6C);
pub const DEVNOTMOUNT: Cond = Cond(0x7C);
pub const IVDEVNAM: Cond = Cond(0x144);
pub const FILESTRUCT: Cond = Cond(0x8C4);
pub const NOHOMEBLK: Cond = Cond(0x8E4);

/// An error and what it was about.
pub type Error = (Cond, String);

fn ods(e: ods_image::Error) -> Error {
    let c = match e.status().1 {
        "FNF" => crate::status::FNF,
        "DNF" => crate::status::DNF,
        "NOHOMEBLK" => NOHOMEBLK,
        // The rest, with the library's text.
        _ => FILESTRUCT,
    };
    (c, e.to_string())
}

fn io(e: std::io::Error, what: &Path) -> Error {
    (files::io_status(e), what.display().to_string())
}

/// A device name as the mount table keeps it: `_dka100:` is `DKA100`.
pub fn device_name(spec: &str) -> Result<String, Error> {
    let d = spec.trim().trim_start_matches('_').trim_end_matches(':');
    if d.is_empty()
        || !d
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'$' || c == b'_')
    {
        return Err((IVDEVNAM, spec.to_string()));
    }
    Ok(d.to_ascii_uppercase())
}

fn device_dir(device: &str) -> PathBuf {
    vmsportd::run_dir().join("mnt").join(device)
}

/// The host tree of a mounted device.
pub fn root(device: &str) -> Option<PathBuf> {
    let r = device_dir(&device_name(device).ok()?).join("root");
    r.is_dir().then_some(r)
}

/// The image and volume label mounted on `device`.
pub fn mounted(device: &str) -> Option<(PathBuf, String)> {
    let info =
        std::fs::read_to_string(device_dir(&device_name(device).ok()?).join("IMAGE")).ok()?;
    let (image, label) = info.trim_end().split_once('\n')?;
    Some((image.into(), label.into()))
}

/// What a volume says it is.
pub struct Volume {
    pub label: String,
    pub owner: String,
    /// `DECFILE11B`
    pub format: String,
}

/// The volume in `image`.
pub fn volume(image: &Path) -> Result<Volume, Error> {
    let info = Image::open(image, ods_image::Mode::ReadOnly)
        .and_then(|mut i| i.info())
        .map_err(ods)?;
    Ok(Volume {
        label: info.label,
        owner: info.owner_name,
        format: info.format,
    })
}

/// MOUNT: `image` as `device`. Returns the volume label.
pub fn mount(image: &Path, device: &str) -> Result<String, Error> {
    let device = device_name(device)?;
    let image = std::fs::canonicalize(image).map_err(|e| io(e, image))?;
    let mnt = vmsportd::run_dir().join("mnt");
    std::fs::create_dir_all(&mnt).map_err(|e| io(e, &mnt))?;
    // The same image on two devices would be written back twice.
    for e in std::fs::read_dir(&mnt).map_err(|e| io(e, &mnt))?.flatten() {
        if mounted(&e.file_name().to_string_lossy()).is_some_and(|(i, _)| i == image) {
            return Err((DEVMOUNT, image.display().to_string()));
        }
    }
    let dir = mnt.join(&device);
    if let Err(e) = std::fs::create_dir(&dir) {
        return Err(match e.kind() {
            std::io::ErrorKind::AlreadyExists => (DEVMOUNT, device),
            _ => io(e, &dir),
        });
    }
    let staged = (|| {
        let mut img = Image::open(&image, ods_image::Mode::ReadOnly).map_err(ods)?;
        let label = img.info().map_err(ods)?.label;
        let mut list = String::new();
        let tmp = dir.join("staging");
        stage_out(&mut img, MFD, &tmp, "", &mut list, &mut vec![MFD])?;
        std::fs::write(dir.join("STAGED"), list).map_err(|e| io(e, &dir))?;
        std::fs::write(dir.join("IMAGE"), format!("{}\n{label}\n", image.display()))
            .map_err(|e| io(e, &dir))?;
        // The tree appears whole or not at all.
        std::fs::rename(&tmp, dir.join("root")).map_err(|e| io(e, &dir))?;
        Ok(label)
    })();
    if staged.is_err() {
        let _ = std::fs::remove_dir_all(&dir);
    }
    staged
}

/// DISMOUNT: writes the device's changed, new and deleted files back to
/// its image and forgets it.
pub fn dismount(device: &str) -> Result<(), Error> {
    let device = device_name(device)?;
    let dir = device_dir(&device);
    let Some((image, _)) = mounted(&device) else {
        return Err((DEVNOTMOUNT, device));
    };
    let list = std::fs::read_to_string(dir.join("STAGED")).map_err(|e| io(e, &dir))?;
    let staged: HashMap<&str, &str> = list
        .lines()
        .map(|l| l.split_once('\t').unwrap_or((l, "")))
        .collect();
    let mut img = Image::open(&image, ods_image::Mode::ReadWrite).map_err(ods)?;
    let mut seen = HashSet::new();
    write_back(&mut img, &dir.join("root"), &[], "", &staged, &mut seen)?;
    // What left the tree leaves the volume: files, then directories, deepest first.
    let mut gone: Vec<&str> = staged
        .keys()
        .copied()
        .filter(|k| !seen.contains(*k))
        .collect();
    gone.sort_by_key(|k| (k.ends_with('/'), std::cmp::Reverse(k.matches('/').count())));
    for k in gone {
        let mut parts: Vec<&str> = k.trim_end_matches('/').split('/').collect();
        let last = parts.pop().unwrap();
        let spec = match k.ends_with('/') {
            true => format!("{}{}.DIR;1", dir_spec(&parts), vms_filespec::escape(last)),
            false => file_spec(&parts, last),
        };
        img.delete(&spec).map_err(ods)?;
    }
    img.flush().map_err(ods)?;
    std::fs::remove_dir_all(&dir).map_err(|e| io(e, &dir))
}

/// The record attributes of a volume's file as a host file keeps them.
fn fab_of(r: &RecordAttrs) -> Fab {
    const RFMS: [Rfm; 7] = [
        Rfm::Udf,
        Rfm::Fix,
        Rfm::Var,
        Rfm::Vfc,
        Rfm::Stm,
        Rfm::Stmlf,
        Rfm::Stmcr,
    ];
    Fab {
        org: [Org::Seq, Org::Rel, Org::Idx]
            .get(r.rtype as usize >> 4)
            .copied()
            .unwrap_or(Org::Seq),
        rfm: RFMS.get(r.rtype as usize & 15).copied().unwrap_or(Rfm::Udf),
        rat: r.rattrib & 15,
        mrs: r.maxrec,
        lrl: r.rsize,
        fsz: r.vfcsize,
        bks: r.bktsize,
    }
}

/// `old` with a host file's record attributes.
fn record_of(f: &Fab, old: RecordAttrs) -> RecordAttrs {
    let org = [Org::Seq, Org::Rel, Org::Idx]
        .iter()
        .position(|o| *o == f.org)
        .unwrap() as u8;
    let rfm = f.rfm as u8;
    RecordAttrs {
        rtype: org << 4 | rfm,
        rattrib: old.rattrib & !15 | f.rat,
        rsize: f.lrl,
        maxrec: f.mrs,
        vfcsize: f.fsz,
        bktsize: f.bks,
        ..old
    }
}

/// What tells a staged file changed: its size, time and attributes.
fn stamp(p: &Path) -> String {
    let md = std::fs::metadata(p).ok();
    let t = md
        .as_ref()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok());
    let fab = sys::get_xattr(p, vms_rms::XATTR).unwrap_or_default();
    format!(
        "{}\t{}\t{}",
        md.map_or(0, |m| m.len()),
        t.map_or(0, |t| t.as_nanos()),
        String::from_utf8_lossy(&fab)
    )
}

fn latin1(b: &[u8]) -> String {
    b.iter().map(|&c| c as char).collect()
}

/// A file's bytes: a sequential file's up to its end of file, a relative or
/// indexed file's every allocated block (VMS leaves the end of file at 0 in
/// some), zeros past its highwater mark.
fn contents(img: &mut Image, fid: Fid, r: &RecordAttrs) -> Result<Vec<u8>, Error> {
    let mut v = Vec::new();
    if r.rtype >> 4 == 0 {
        img.copy_out(fid, &mut v, Conversion::Binary).map_err(ods)?;
        return Ok(v);
    }
    for (lbn, count) in img.extents(fid).map_err(ods)? {
        for b in lbn..lbn + count {
            v.extend(img.read_block(b).map_err(ods)?);
        }
    }
    v.truncate(r.hiblk as usize * vms_rms::BLOCK);
    if let Some(hw) = img.stat(fid).map_err(ods)?.highwater {
        let from = (hw.max(1) as usize - 1) * vms_rms::BLOCK;
        let from = from.min(v.len());
        v[from..].fill(0);
    }
    Ok(v)
}

fn stage_out(
    img: &mut Image,
    dir: Fid,
    host: &Path,
    rel: &str,
    list: &mut String,
    seen: &mut Vec<Fid>,
) -> Result<(), Error> {
    std::fs::create_dir(host).map_err(|e| io(e, host))?;
    for e in img.list(dir).map_err(ods)? {
        let name = latin1(&e.name);
        // The volume's own files (INDEXF.SYS, BITMAP.SYS, ...) stay out.
        if dir == MFD && e.fid.num <= 16 && name.ends_with(".SYS")
            || !ods_image::safe_host_name(&name)
        {
            continue;
        }
        let a = img.attributes(e.fid).map_err(ods)?;
        if e.is_dir_name() && a.filechar & fch::DIRECTORY != 0 {
            if seen.contains(&e.fid) {
                continue;
            }
            seen.push(e.fid);
            let stem = &name[..name.len() - 4];
            list.push_str(&format!("{rel}{stem}/\n"));
            stage_out(
                img,
                e.fid,
                &host.join(stem),
                &format!("{rel}{stem}/"),
                list,
                seen,
            )?;
            continue;
        }
        let file = format!("{name};{}", e.version);
        let path = host.join(&file);
        std::fs::write(&path, contents(img, e.fid, &a.record)?).map_err(|e| io(e, &path))?;
        sys::set_xattr(
            &path,
            vms_rms::XATTR,
            fab_of(&a.record).to_string().as_bytes(),
        )
        .map_err(|e| io(e, &path))?;
        list.push_str(&format!("{rel}{file}\t{}\n", stamp(&path)));
    }
    Ok(())
}

fn dir_spec(dirs: &[&str]) -> String {
    match dirs {
        [] => "[000000]".into(),
        _ => format!(
            "[{}]",
            dirs.iter()
                .map(|d| vms_filespec::escape(d))
                .collect::<Vec<_>>()
                .join(".")
        ),
    }
}

fn file_spec(dirs: &[&str], host_name: &str) -> String {
    let (name, typ, v) = split_host(host_name);
    format!(
        "{}{}.{};{v}",
        dir_spec(dirs),
        vms_filespec::escape(&name),
        vms_filespec::escape(&typ)
    )
}

fn write_back(
    img: &mut Image,
    host: &Path,
    dirs: &[&str],
    rel: &str,
    staged: &HashMap<&str, &str>,
    seen: &mut HashSet<String>,
) -> Result<(), Error> {
    let mut items: Vec<_> = std::fs::read_dir(host)
        .map_err(|e| io(e, host))?
        .flatten()
        .map(|e| e.path())
        .collect();
    items.sort();
    for p in items {
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        if p.is_dir() {
            let key = format!("{rel}{name}/");
            let mut sub = dirs.to_vec();
            sub.push(&name);
            if !staged.contains_key(key.as_str()) {
                img.mkdir(&dir_spec(&sub)).map_err(ods)?;
            }
            write_back(img, &p, &sub, &key, staged, seen)?;
            seen.insert(key);
            continue;
        }
        let key = format!("{rel}{name}");
        let now = stamp(&p);
        let before = staged.get(key.as_str()).copied();
        seen.insert(key);
        if before == Some(now.as_str()) {
            continue;
        }
        let spec = file_spec(dirs, &name);
        // A changed file keeps its name, version, dates and protection.
        let old = match before {
            Some(_) => {
                let fid = img.lookup(&spec).map_err(ods)?;
                let a = img.attributes(fid).map_err(ods)?;
                img.delete(&spec).map_err(ods)?;
                Some(a)
            }
            None => None,
        };
        let record = record_of(
            &files::fab(&p),
            old.as_ref().map(|a| a.record).unwrap_or_default(),
        );
        let mut f = std::fs::File::open(&p).map_err(|e| io(e, &p))?;
        let size = f.metadata().map(|m| m.len()).ok();
        let (fid, _) = img
            .copy_in(&mut f, &spec, Conversion::Binary, size, Some(record))
            .map_err(ods)?;
        if let Some(mut a) = old {
            a.record = img.attributes(fid).map_err(ods)?.record;
            a.revised = ods_image::time::now();
            a.revision = a.revision.wrapping_add(1);
            img.set_attributes(fid, &a).map_err(ods)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attributes_round_trip() {
        let r = RecordAttrs {
            rtype: 0x21,
            rattrib: 2,
            rsize: 64,
            maxrec: 64,
            bktsize: 1,
            ..Default::default()
        };
        let f = fab_of(&r);
        assert_eq!(
            f.to_string(),
            "org=idx rfm=fix rat=cr mrs=64 lrl=64 fsz=0 bks=1"
        );
        assert_eq!(record_of(&f, RecordAttrs::default()), r);
        assert_eq!(device_name("_dka100:").unwrap(), "DKA100");
        assert!(device_name("DKA 1:").is_err());
    }
}
