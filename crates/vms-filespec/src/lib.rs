//! VMS file specifications: `NODE::DEV:[DIR.SUB]NAME.TYP;VER`.
//!
//! Pure parsing and formatting, no host I/O. Case is preserved; comparing
//! case-insensitively is the caller's job.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileSpec {
    pub node: Option<String>,
    pub device: Option<String>,
    pub directory: Option<Directory>,
    pub name: String,
    /// Without the dot. `Some("")` for `NAME.`.
    pub typ: Option<String>,
    pub version: Option<Version>,
}

/// `[ROOT.][-.A...B]`
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Directory {
    /// Rooted-directory prefix, `[ROOT.]` in `[ROOT.][SUB]`. Empty if not rooted.
    pub root: Vec<String>,
    /// `[.A]`, `[-]`, `[]`.
    pub relative: bool,
    /// Number of leading `-` (parent) steps.
    pub up: usize,
    /// Components; `...` appears as its own component.
    pub parts: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Version {
    /// `;N`. `0` (and a bare `;`) means highest, negatives count back from it.
    Number(i16),
    /// `;*`
    Wildcard,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub &'static str);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for Error {}

// ponytail: ODS-2 charset plus wildcards, case preserved. ODS-5 `^` escapes
// (spaces, extra dots, unicode) are not parsed yet; add when host names need them.
fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '$' | '_' | '-' | '*' | '%')
}

fn check_name(s: &str, what: &'static str) -> Result<(), Error> {
    if s.chars().all(is_name_char) {
        Ok(())
    } else {
        Err(Error(what))
    }
}

impl std::str::FromStr for FileSpec {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Error> {
        let mut spec = FileSpec::default();
        let mut rest = s.trim();
        // Node and device only count if they come before the directory.
        let dir_start = rest.find(['[', '<']).unwrap_or(rest.len());

        if let Some(i) = rest[..dir_start].find("::") {
            check_name(&rest[..i], "invalid node name")?;
            spec.node = Some(rest[..i].to_string());
            rest = &rest[i + 2..];
        }
        let dir_start = rest.find(['[', '<']).unwrap_or(rest.len());
        if let Some(i) = rest[..dir_start].find(':') {
            if i == 0 {
                return Err(Error("empty device name"));
            }
            check_name(&rest[..i], "invalid device name")?;
            spec.device = Some(rest[..i].to_string());
            rest = &rest[i + 1..];
        }
        if rest.starts_with(['[', '<']) {
            let (dir, after) = parse_directory(rest)?;
            spec.directory = Some(dir);
            rest = after;
        }

        let (body, ver) = match rest.split_once(';') {
            Some((b, v)) => (b, Some(v)),
            None => (rest, None),
        };
        let mut dots = body.splitn(3, '.');
        spec.name = dots.next().unwrap_or("").to_string();
        check_name(&spec.name, "invalid file name")?;
        if let Some(t) = dots.next() {
            check_name(t, "invalid file type")?;
            spec.typ = Some(t.to_string());
        }
        let ver = match (dots.next(), ver) {
            (Some(_), Some(_)) => return Err(Error("two versions")),
            (Some(v), None) | (None, Some(v)) => Some(v),
            (None, None) => None,
        };
        spec.version = ver.map(parse_version).transpose()?;
        Ok(spec)
    }
}

fn parse_version(v: &str) -> Result<Version, Error> {
    match v {
        "" => Ok(Version::Number(0)),
        "*" => Ok(Version::Wildcard),
        _ => match v.parse::<i16>() {
            Ok(n)
                if n != i16::MIN
                    && (v.starts_with('-') || v.bytes().all(|b| b.is_ascii_digit())) =>
            {
                Ok(Version::Number(n))
            }
            _ => Err(Error("invalid version")),
        },
    }
}

/// Parses `[...]` (or `<...>`), plus a following group when the first is a
/// root (`[A.][B]`). Returns the rest of the input.
fn parse_directory(s: &str) -> Result<(Directory, &str), Error> {
    let (first, rest) = bracket(s)?;
    if first.ends_with('.') && rest.starts_with(['[', '<']) {
        let (second, rest) = bracket(rest)?;
        let root = parse_dir_body(&first[..first.len() - 1])?;
        if root.relative || root.up > 0 {
            return Err(Error("invalid root directory"));
        }
        let mut dir = parse_dir_body(second)?;
        dir.root = root.parts;
        return Ok((dir, rest));
    }
    Ok((parse_dir_body(first)?, rest))
}

fn bracket(s: &str) -> Result<(&str, &str), Error> {
    let close = if s.starts_with('[') { ']' } else { '>' };
    let end = s.find(close).ok_or(Error("unterminated directory"))?;
    Ok((&s[1..end], &s[end + 1..]))
}

fn parse_dir_body(body: &str) -> Result<Directory, Error> {
    let mut dir = Directory::default();
    // Split on '.', keeping "..." as a component.
    let mut parts = Vec::new();
    let mut cur = String::new();
    let mut chars = body.chars().peekable();
    let mut leading_dot = false;
    while let Some(c) = chars.next() {
        if c != '.' {
            cur.push(c);
            continue;
        }
        let mut n = 1;
        while chars.peek() == Some(&'.') {
            chars.next();
            n += 1;
        }
        match n {
            1 if parts.is_empty() && cur.is_empty() => leading_dot = true,
            1 if !cur.is_empty() => parts.push(std::mem::take(&mut cur)),
            3 => {
                if !cur.is_empty() {
                    parts.push(std::mem::take(&mut cur));
                }
                parts.push("...".to_string());
            }
            _ => return Err(Error("invalid directory")),
        }
    }
    if !cur.is_empty() {
        parts.push(cur);
    } else if body.ends_with('.') && !body.ends_with("...") {
        return Err(Error("invalid directory"));
    }

    // Leading "-" / "--" / "-.-" steps up.
    let ups = parts
        .iter()
        .take_while(|p| p.chars().all(|c| c == '-'))
        .count();
    if leading_dot && ups > 0 {
        return Err(Error("invalid directory"));
    }
    dir.up = parts[..ups].iter().map(String::len).sum();
    parts.drain(..ups);
    for p in &parts {
        if p != "..." {
            check_name(p, "invalid directory name")?;
        }
    }
    dir.relative = leading_dot || dir.up > 0 || (parts.is_empty() && body.is_empty());
    dir.parts = parts;
    Ok(dir)
}

impl fmt::Display for Directory {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("[")?;
        if !self.root.is_empty() {
            write!(f, "{}.][", self.root.join("."))?;
        }
        let mut prev: Option<&str> = None;
        for _ in 0..self.up {
            if prev.is_some() {
                f.write_str(".")?;
            }
            f.write_str("-")?;
            prev = Some("-");
        }
        if self.relative && self.up == 0 && self.parts.first().is_some_and(|p| p != "...") {
            f.write_str(".")?;
        }
        for p in &self.parts {
            if prev.is_some_and(|q| q != "...") && p != "..." {
                f.write_str(".")?;
            }
            f.write_str(p)?;
            prev = Some(p);
        }
        f.write_str("]")
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Version::Number(n) => write!(f, "{n}"),
            Version::Wildcard => f.write_str("*"),
        }
    }
}

impl fmt::Display for FileSpec {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if let Some(n) = &self.node {
            write!(f, "{n}::")?;
        }
        if let Some(d) = &self.device {
            write!(f, "{d}:")?;
        }
        if let Some(d) = &self.directory {
            write!(f, "{d}")?;
        }
        f.write_str(&self.name)?;
        if let Some(t) = &self.typ {
            write!(f, ".{t}")?;
        }
        if let Some(v) = &self.version {
            write!(f, ";{v}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> FileSpec {
        s.parse().unwrap_or_else(|e| panic!("{s}: {e}"))
    }

    #[test]
    fn full_spec() {
        let s = p("NODE::DKA0:[DIR.SUB]NAME.TYP;3");
        assert_eq!(s.node.as_deref(), Some("NODE"));
        assert_eq!(s.device.as_deref(), Some("DKA0"));
        let d = s.directory.as_ref().unwrap();
        assert_eq!(d.parts, ["DIR", "SUB"]);
        assert!(!d.relative);
        assert_eq!(s.name, "NAME");
        assert_eq!(s.typ.as_deref(), Some("TYP"));
        assert_eq!(s.version, Some(Version::Number(3)));
    }

    #[test]
    fn versions() {
        assert_eq!(p("A.B;-1").version, Some(Version::Number(-1)));
        assert_eq!(p("A.B;").version, Some(Version::Number(0)));
        assert_eq!(p("A.B;*").version, Some(Version::Wildcard));
        assert_eq!(p("A.B.7").version, Some(Version::Number(7)));
        assert_eq!(p("A.B").version, None);
        assert!("A.B;x".parse::<FileSpec>().is_err());
        assert!("A.B;+1".parse::<FileSpec>().is_err());
        assert!("A.B;40000".parse::<FileSpec>().is_err());
        assert!("A.B.1;2".parse::<FileSpec>().is_err());
    }

    #[test]
    fn device_only_and_logicals() {
        let s = p("SYS$LOGIN:");
        assert_eq!(s.device.as_deref(), Some("SYS$LOGIN"));
        assert_eq!(s.name, "");
        assert_eq!(s.typ, None);
        assert_eq!(p("SYS$LOGIN:NOTES.TXT").to_string(), "SYS$LOGIN:NOTES.TXT");
    }

    #[test]
    fn relative_dirs() {
        let d = p("[.A.B]").directory.unwrap();
        assert!(d.relative);
        assert_eq!((d.up, d.parts.len()), (0, 2));
        let d = p("[-.-.X]").directory.unwrap();
        assert_eq!(
            (d.up, d.parts.as_slice()),
            (2, ["X".to_string()].as_slice())
        );
        assert_eq!(p("[--]").directory.unwrap().up, 2);
        assert!(p("[]").directory.unwrap().relative);
        assert!(!p("[000000]").directory.unwrap().relative);
    }

    #[test]
    fn ellipsis_and_wildcards() {
        let d = p("[A...B]*.%%%;*").directory.unwrap();
        assert_eq!(d.parts, ["A", "...", "B"]);
        assert_eq!(p("[...]").directory.unwrap().parts, ["..."]);
        assert_eq!(p("[.A...]").directory.unwrap().parts, ["A", "..."]);
    }

    #[test]
    fn rooted() {
        let s = p("DKA0:[ROOT.][SUB]X.Y");
        let d = s.directory.as_ref().unwrap();
        assert_eq!(d.root, ["ROOT"]);
        assert_eq!(d.parts, ["SUB"]);
        assert_eq!(s.to_string(), "DKA0:[ROOT.][SUB]X.Y");
    }

    #[test]
    fn angle_brackets_normalize() {
        assert_eq!(p("<A.B>C.D").to_string(), "[A.B]C.D");
    }

    #[test]
    fn round_trip() {
        for s in [
            "NODE::DKA0:[DIR.SUB]NAME.TYP;3",
            "[.A.B]X.Y;-1",
            "[-]X.",
            "[-.-.X]Y.Z;*",
            "[A...B]*.*;*",
            "[...]A",
            "[.A...]B",
            "[]",
            "[000000]",
            "DKA0:[ROOT.][SUB.DIR]",
            "report.txt;3",
        ] {
            assert_eq!(p(s).to_string(), s);
        }
    }

    #[test]
    fn errors() {
        for s in ["[A", "[A..B]", "[A.]", "[.-]", "A B", ":X", "[A]B.C.D.E"] {
            assert!(s.parse::<FileSpec>().is_err(), "{s} should fail");
        }
    }
}
