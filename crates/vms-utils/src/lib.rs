//! What the utilities share: their command, file lists, messages.

use libvms::image::Image;
use std::path::PathBuf;
use vms_cld::status as cli;
use vms_cond::Cond;
use vms_fao::Arg;
use vms_filespec::{FileSpec, Version};

/// Shared message numbers (sys/SYSMSG/SYSMSG.MSG, facility 0): used with
/// a utility's own facility, as `%DELETE-I-FILDEL`.
pub mod shr {
    pub const APPENDED_RECORDS: u32 = 513;
    pub const COPIED_BLOCKS: u32 = 524;
    pub const COPIED_RECORDS: u32 = 525;
    pub const NEWFILES: u32 = 530;
    pub const OPENIN: u32 = 531;
    pub const OPENOUT: u32 = 532;
    pub const DELVER: u32 = 577;
    pub const SEARCHFAIL: u32 = 583;
    pub const TOTAL: u32 = 610;
    pub const FILPURG: u32 = 611;
    pub const FILDEL: u32 = 612;
}

/// Severities in condition values.
pub const W: u32 = 0;
pub const S: u32 = 1;
pub const E: u32 = 2;
pub const I: u32 = 3;

/// `SS$_NOSUCHFILE`, the secondary status of a missing directory.
pub const NOSUCHFILE: Cond = Cond(0x910);
/// Success, message already shown.
pub const DONE: Cond = Cond(0x1000_0001);

pub fn inhibit(c: Cond) -> Cond {
    Cond(c.0 | 0x1000_0000)
}

pub struct Util {
    pub img: Image,
    /// The facility number messages and statuses use.
    pub fac: u32,
}

impl Util {
    pub fn new(cld: &str, fac: u32) -> Util {
        Util {
            img: Image::start(cld),
            fac,
        }
    }

    /// A shared message in our facility.
    pub fn shared(&self, msgno: u32, sev: u32) -> Cond {
        Cond(self.fac << 16 | msgno << 3 | sev)
    }

    /// The values of an entity, with the status each came with (COMMA,
    /// CONCAT or NORMAL).
    pub fn values(&mut self, name: &str) -> Vec<(String, Cond)> {
        std::iter::from_fn(|| self.img.command.get_value(name).ok()).collect()
    }

    pub fn value(&mut self, name: &str) -> Option<String> {
        self.img.command.get_value(name).ok().map(|v| v.0)
    }

    pub fn present(&self, name: &str) -> bool {
        matches!(
            self.img.command.present(name),
            cli::PRESENT | cli::DEFAULTED | cli::LOCPRES
        )
    }

    pub fn msg(&self, msgs: &[(Cond, Vec<Arg>)]) {
        self.img.put_msg(msgs);
    }

    pub fn exit(self, st: Cond) -> ! {
        self.img.exit(st)
    }

    /// Each spec of a list, its device and directory carried to the next
    /// one (VMS's temporary defaults), merged with `default`, and the files
    /// it matches (none: an empty list) or why it matched none.
    pub fn expand(&self, items: &[String], default: &str) -> Vec<Item> {
        let mut related = String::new();
        let mut out = Vec::new();
        for text in items {
            let typed = text
                .trim()
                .to_uppercase()
                .parse::<FileSpec>()
                .unwrap_or_default();
            let item = match self.img.session.parse(text, default, &related) {
                Ok(spec) => {
                    related = format!(
                        "{}:{}",
                        spec.device.as_deref().unwrap_or(""),
                        spec.directory.clone().unwrap_or_default()
                    );
                    let files = self.img.session.search_all(&spec);
                    Item { typed, spec, files }
                }
                Err(e) => Item {
                    typed,
                    spec: FileSpec::default(),
                    files: Err(e),
                },
            };
            out.push(item);
        }
        out
    }
}

pub struct Item {
    /// The spec as typed.
    pub typed: FileSpec,
    /// With defaults applied.
    pub spec: FileSpec,
    pub files: Result<Vec<(PathBuf, FileSpec)>, Cond>,
}

impl Item {
    /// Wildcards in name, type or version.
    pub fn wild(&self) -> bool {
        let w = |s: &str| s.contains(['*', '%']);
        w(&self.spec.name)
            || self.spec.typ.as_deref().is_some_and(w)
            || self.spec.version == Some(Version::Wildcard)
    }
}

/// The text values a file list entity has.
pub fn texts(v: &[(String, Cond)]) -> Vec<String> {
    v.iter().map(|x| x.0.clone()).collect()
}
