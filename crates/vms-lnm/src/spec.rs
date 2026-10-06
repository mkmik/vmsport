//! Iterative translation of the logical names in a file spec, as RMS does
//! it for `$PARSE`, with rooted and concealed devices.

use crate::{Logical, MAX_DEPTH, SS_TOOMANYLNAM};
use vms_cond::Cond;
use vms_filespec::{Directory, FileSpec};

/// `SS$_IVDEVNAM`: an equivalence that isn't a file spec.
const SS_IVDEVNAM: Cond = Cond(0x144);

/// A file spec after logical names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// What `$PARSE` returns: a concealed device keeps its logical name.
    pub display: FileSpec,
    /// The physical device, with a rooted directory kept as `[ROOT.][DIR]`
    /// (what F$PARSE's NO_CONCEAL shows); [`Resolved::path`] flattens it.
    pub physical: FileSpec,
}

impl Resolved {
    /// The directory on the physical device, the root flattened in:
    /// `[T.LNM.][SUB]` is `T`, `LNM`, `SUB`.
    pub fn path(&self) -> Vec<String> {
        let d = self.physical.directory.clone().unwrap_or_default();
        d.root
            .into_iter()
            .chain(d.parts.into_iter().filter(|p| p != "000000"))
            .collect()
    }
}

/// Parses an equivalence string. A rooted directory alone (`DKA200:[T.LNM.]`)
/// is the root's top, `[T.LNM.][000000]`.
fn parse(text: &str) -> Result<FileSpec, Cond> {
    let fixed;
    let text = if text.contains(".]") && !text.contains(".][") {
        fixed = text.replacen(".]", ".][000000]", 1);
        &fixed
    } else {
        text
    };
    text.parse().map_err(|_| SS_IVDEVNAM)
}

/// Just a device: `X:`.
fn device_only(s: &FileSpec) -> bool {
    s.directory.is_none()
        && s.name.is_empty()
        && s.typ.is_none()
        && s.version.is_none()
        && s.node.is_none()
}

/// The equivalence `e` with the fields `s` has itself. A rooted
/// equivalence takes `s`'s directory under its root.
fn merge(e: &FileSpec, s: &FileSpec) -> FileSpec {
    let directory = match (&e.directory, &s.directory) {
        (Some(ed), Some(sd)) if !ed.root.is_empty() => Some(Directory {
            root: ed.root.iter().chain(&sd.root).cloned().collect(),
            ..sd.clone()
        }),
        (ed, sd) => sd.clone().or(ed.clone()),
    };
    FileSpec {
        node: e.node.clone(),
        device: e.device.clone(),
        directory,
        name: if s.name.is_empty() {
            e.name.clone()
        } else {
            s.name.clone()
        },
        typ: s.typ.clone().or(e.typ.clone()),
        version: s.version.or(e.version),
    }
}

/// Translates the device (or a lone file name) of `spec` through logical
/// names until a physical device, one result per search-list value. The
/// display is the spec as it stood when the first concealed value was met
/// (`VPT_DEV:[SUB]` with `VPT_DEV` = `VPT_ROOT:` shows as `VPT_ROOT:[SUB]`).
pub fn resolve(
    spec: &FileSpec,
    translate: impl Fn(&str) -> Option<Logical>,
) -> Result<Vec<Resolved>, Cond> {
    let mut out = Vec::new();
    step(spec.clone(), None, &translate, 0, &mut out)?;
    Ok(out)
}

/// `display`: set once a concealed value froze what is shown.
fn step(
    cur: FileSpec,
    display: Option<&FileSpec>,
    translate: &impl Fn(&str) -> Option<Logical>,
    depth: usize,
    out: &mut Vec<Resolved>,
) -> Result<(), Cond> {
    if depth > MAX_DEPTH {
        return Err(SS_TOOMANYLNAM);
    }
    // A spec that is only a name may be a logical name for a whole spec.
    let lone_name = cur.device.is_none()
        && cur.directory.is_none()
        && cur.typ.is_none()
        && cur.version.is_none();
    let key = match (&cur.device, lone_name) {
        (Some(d), _) => d.clone(),
        (None, true) if !cur.name.is_empty() => cur.name.clone(),
        _ => String::new(),
    };
    let Some(l) = (!key.is_empty()).then(|| translate(&key)).flatten() else {
        out.push(Resolved {
            display: display.cloned().unwrap_or_else(|| cur.clone()),
            physical: cur,
        });
        return Ok(());
    };
    for e in &l.equivs {
        let eq = parse(&e.text)?;
        let next = if cur.device.is_none() {
            eq.clone()
        } else if device_only(&eq) {
            FileSpec {
                device: eq.device.clone(),
                ..cur.clone()
            }
        } else {
            merge(&eq, &cur)
        };
        let shown = if e.concealed && display.is_none() {
            Some(&cur)
        } else {
            display
        };
        if e.terminal {
            out.push(Resolved {
                display: shown.cloned().unwrap_or_else(|| next.clone()),
                physical: next,
            });
            continue;
        }
        step(next, shown, translate, depth + 1, out)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Equiv;

    fn logicals(name: &str) -> Option<Logical> {
        let (text, concealed) = match name {
            "ROOT" => ("DKA200:[T.LNM.]", true),
            "DEV" => ("ROOT:", false),
            "SUB" => ("ROOT:[SUB]", false),
            "LOOP" => ("LOOP:", false),
            _ => return None,
        };
        Some(Logical {
            equivs: vec![Equiv {
                concealed,
                ..Equiv::new(text)
            }],
            ..Logical::new(name, &[])
        })
    }

    fn r(s: &str) -> (String, String) {
        let x = resolve(&s.parse().unwrap(), logicals).unwrap().remove(0);
        (x.display.to_string(), x.physical.to_string())
    }

    #[test]
    fn rooted_and_concealed() {
        assert_eq!(
            r("ROOT:[SUB]X.Y"),
            ("ROOT:[SUB]X.Y".into(), "DKA200:[T.LNM.][SUB]X.Y".into())
        );
        assert_eq!(r("DEV:[SUB]X.Y").0, "ROOT:[SUB]X.Y");
        assert_eq!(
            r("SUB:X.Y"),
            ("ROOT:[SUB]X.Y".into(), "DKA200:[T.LNM.][SUB]X.Y".into())
        );
        let x = resolve(&"ROOT:[A.B]C".parse().unwrap(), logicals)
            .unwrap()
            .remove(0);
        assert_eq!(x.path(), ["T", "LNM", "A", "B"]);
        assert_eq!(
            resolve(&"LOOP:X".parse().unwrap(), logicals),
            Err(SS_TOOMANYLNAM)
        );
    }
}
