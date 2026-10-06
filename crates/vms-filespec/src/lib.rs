//! VMS file specifications: `NODE::DEV:[DIR.SUB]NAME.TYP;VER`, with ODS-5
//! extended names (`^` escapes, multiple dots, 8-bit and Unicode characters).
//!
//! Pure parsing and formatting, no host I/O. Name components are kept in
//! canonical VMS syntax (`a^.b^_c`), so `*` and `%` stay wildcards and a
//! literal `%` is `^%`. [`unescape`] and [`escape`] convert a component to and
//! from the host string. Case is preserved; comparing case-insensitively is
//! the caller's job.

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
    /// `[.A]`, `[-]`, `[]`, `[...]`.
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

/// One input character; `esc` is true if it came from a `^` escape.
#[derive(Clone, Copy, PartialEq)]
struct Tok {
    c: char,
    esc: bool,
}

impl Tok {
    fn is(self, c: char) -> bool {
        !self.esc && self.c == c
    }
}

fn hex(s: &[char]) -> Option<u32> {
    let s: String = s.iter().collect();
    if s.chars().all(|c| c.is_ascii_hexdigit()) {
        u32::from_str_radix(&s, 16).ok()
    } else {
        None
    }
}

/// Decodes `^` escapes: `^_` space, `^XX` hex byte (Latin-1), `^Uxxxx`
/// UCS-2, `^c` any ASCII punctuation.
fn tokenize(s: &str) -> Result<Vec<Tok>, Error> {
    let cs: Vec<char> = s.chars().collect();
    let mut out = Vec::with_capacity(cs.len());
    let mut i = 0;
    while i < cs.len() {
        if cs[i] != '^' {
            out.push(Tok {
                c: cs[i],
                esc: false,
            });
            i += 1;
            continue;
        }
        let rest = &cs[i + 1..];
        let (c, n) = if rest.len() >= 5 && rest[0] == 'U' && hex(&rest[1..5]).is_some() {
            let c = char::from_u32(hex(&rest[1..5]).unwrap()).ok_or(Error("invalid ^U escape"))?;
            (c, 5)
        } else if let Some(b) = rest.get(..2).and_then(hex) {
            (char::from(b as u8), 2)
        } else {
            match rest.first() {
                Some('_' | ' ') => (' ', 1),
                Some(&c) if c.is_ascii_punctuation() => (c, 1),
                _ => return Err(Error("invalid ^ escape")),
            }
        };
        out.push(Tok { c, esc: true });
        i += 1 + n;
    }
    Ok(out)
}

/// Characters that appear in names without an escape. `*` and `%` are wildcards.
fn is_plain(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '$' | '_' | '-' | '~') || c as u32 > 0x9f
}

fn push_escaped(out: &mut String, c: char) {
    use std::fmt::Write;
    match c {
        _ if is_plain(c) => out.push(c),
        ' ' => out.push_str("^_"),
        '!' | '#' | '&' | '\'' | '(' | ')' | '+' | ',' | '.' | ';' | '=' | '@' | '[' | ']'
        | '^' | '`' | '{' | '}' | '%' => {
            out.push('^');
            out.push(c);
        }
        _ => write!(out, "^{:02X}", c as u32).unwrap(),
    }
}

/// Canonical VMS syntax for a component given as tokens.
fn canon(toks: &[Tok], what: &'static str) -> Result<String, Error> {
    let mut s = String::new();
    for t in toks {
        if t.esc {
            push_escaped(&mut s, t.c);
        } else if is_plain(t.c) || t.c == '*' || t.c == '%' {
            s.push(t.c);
        } else {
            return Err(Error(what));
        }
    }
    Ok(s)
}

/// Host string to canonical VMS syntax, escaping everything special,
/// including `*` and `%`.
pub fn escape(host: &str) -> String {
    let mut s = String::new();
    for c in host.chars() {
        push_escaped(&mut s, c);
    }
    s
}

/// Canonical (or any valid) VMS syntax to the host string. Wildcards are
/// returned as-is.
pub fn unescape(vms: &str) -> Result<String, Error> {
    Ok(tokenize(vms)?.into_iter().map(|t| t.c).collect())
}

fn plain(toks: &[Tok], what: &'static str) -> Result<String, Error> {
    if toks.iter().any(|t| t.esc) {
        return Err(Error(what));
    }
    Ok(toks.iter().map(|t| t.c).collect())
}

fn find(toks: &[Tok], pred: impl Fn(char) -> bool) -> Option<usize> {
    toks.iter().position(|t| !t.esc && pred(t.c))
}

fn is_version(toks: &[Tok]) -> bool {
    let s: String = toks
        .iter()
        .map(|t| if t.esc { '\0' } else { t.c })
        .collect();
    s == "*"
        || (!s.is_empty()
            && s.trim_start_matches('-')
                .chars()
                .all(|c| c.is_ascii_digit()))
}

impl std::str::FromStr for FileSpec {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Error> {
        let all = tokenize(s.trim())?;
        let mut rest = &all[..];
        let mut spec = FileSpec::default();
        let open = |c| c == '[' || c == '<';

        // Node and device only count if they come before the directory.
        let dir_start = find(rest, open).unwrap_or(rest.len());
        if let Some(i) = rest[..dir_start]
            .windows(2)
            .position(|w| w[0].is(':') && w[1].is(':'))
        {
            spec.node = Some(device_name(&rest[..i], "invalid node name")?);
            rest = &rest[i + 2..];
        }
        let dir_start = find(rest, open).unwrap_or(rest.len());
        if let Some(i) = find(&rest[..dir_start], |c| c == ':') {
            spec.device = Some(device_name(&rest[..i], "invalid device name")?);
            rest = &rest[i + 1..];
        }
        if rest.first().is_some_and(|t| !t.esc && open(t.c)) {
            let (dir, after) = parse_directory(rest)?;
            spec.directory = Some(dir);
            rest = after;
        }

        let (body, ver) = match find(rest, |c| c == ';') {
            Some(i) => (&rest[..i], Some(&rest[i + 1..])),
            None => (rest, None),
        };
        let dots: Vec<usize> = (0..body.len()).filter(|&i| body[i].is('.')).collect();
        let (body, ver) = match (ver, dots.as_slice()) {
            // Old-style NAME.TYP.VER.
            (None, [_, .., v]) if is_version(&body[v + 1..]) => (&body[..*v], Some(&body[v + 1..])),
            _ => (body, ver),
        };
        let mut body = body.to_vec();
        // ODS-5: the last dot starts the type; earlier dots belong to the name.
        let typ_dot = (0..body.len()).rev().find(|&i| body[i].is('.'));
        if let Some(t) = typ_dot {
            for tok in &mut body[..t] {
                tok.esc |= tok.c == '.';
            }
            spec.typ = Some(canon(&body[t + 1..], "invalid file type")?);
        }
        spec.name = canon(&body[..typ_dot.unwrap_or(body.len())], "invalid file name")?;
        spec.version = ver
            .map(|v| parse_version(&plain(v, "invalid version")?))
            .transpose()?;
        Ok(spec)
    }
}

fn device_name(toks: &[Tok], what: &'static str) -> Result<String, Error> {
    let s = plain(toks, what)?;
    if s.is_empty()
        || !s
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '$' || c == '_')
    {
        return Err(Error(what));
    }
    Ok(s)
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
fn parse_directory(s: &[Tok]) -> Result<(Directory, &[Tok]), Error> {
    let (first, rest) = bracket(s)?;
    if first.last().is_some_and(|t| t.is('.'))
        && rest.first().is_some_and(|t| t.is('[') || t.is('<'))
    {
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

fn bracket(s: &[Tok]) -> Result<(&[Tok], &[Tok]), Error> {
    let close = if s[0].is('[') { ']' } else { '>' };
    let end = find(s, |c| c == close).ok_or(Error("unterminated directory"))?;
    Ok((&s[1..end], &s[end + 1..]))
}

fn parse_dir_body(body: &[Tok]) -> Result<Directory, Error> {
    let mut dir = Directory::default();
    // Split on unescaped '.', keeping "..." as a component.
    let mut parts: Vec<Option<&[Tok]>> = Vec::new(); // None is "..."
    let mut leading_dot = false;
    let (mut i, mut start) = (0, 0);
    while i < body.len() {
        if !body[i].is('.') {
            i += 1;
            continue;
        }
        let n = body[i..].iter().take_while(|t| t.is('.')).count();
        let cur = &body[start..i];
        match n {
            1 if parts.is_empty() && cur.is_empty() => leading_dot = true,
            1 if !cur.is_empty() => parts.push(Some(cur)),
            3 => {
                if !cur.is_empty() {
                    parts.push(Some(cur));
                }
                parts.push(None);
            }
            _ => return Err(Error("invalid directory")),
        }
        i += n;
        start = i;
    }
    if start < body.len() {
        parts.push(Some(&body[start..]));
    } else if body.last().is_some_and(|t| t.is('.')) && parts.last() != Some(&None) {
        return Err(Error("invalid directory"));
    }

    // Leading "-" / "--" / "-.-" steps up.
    let is_up = |p: &Option<&[Tok]>| p.is_some_and(|p| p.iter().all(|t| t.is('-')));
    let ups = parts.iter().take_while(|p| is_up(p)).count();
    if leading_dot && ups > 0 {
        return Err(Error("invalid directory"));
    }
    dir.up = parts[..ups].iter().map(|p| p.unwrap().len()).sum();
    dir.relative = leading_dot || dir.up > 0 || body.is_empty() || parts.first() == Some(&None);
    for p in &parts[ups..] {
        dir.parts.push(match p {
            Some(p) => canon(p, "invalid directory name")?,
            None => "...".to_string(),
        });
    }
    Ok(dir)
}

impl Directory {
    /// A relative directory (`[.A]`, `[-.B]`, `[]`) made absolute against
    /// `base`; absolute ones come back as they are.
    pub fn resolve(&self, base: &Directory) -> Directory {
        if !self.relative {
            return self.clone();
        }
        let mut parts = base.parts.clone();
        parts.truncate(parts.len().saturating_sub(self.up));
        parts.extend(self.parts.iter().cloned());
        Directory { root: base.root.clone(), relative: false, up: 0, parts }
    }
}

impl FileSpec {
    /// `$PARSE` defaulting: what `self` lacks comes from `defaults` in
    /// order, then the device and directory of `current` (the process
    /// default). A relative directory is taken from the process default
    /// directory. The version stays missing if nobody gives one.
    pub fn merge(&self, defaults: &[&FileSpec], current: &FileSpec) -> FileSpec {
        let pick = |f: &dyn Fn(&FileSpec) -> bool| defaults.iter().find(|d| f(d));
        let mut out = self.clone();
        if out.device.is_none() {
            out.device = pick(&|d| d.device.is_some()).and_then(|d| d.device.clone()).or(current.device.clone());
        }
        if out.directory.is_none() {
            out.directory = pick(&|d| d.directory.is_some()).and_then(|d| d.directory.clone());
        }
        let base = current.directory.clone().unwrap_or_default();
        out.directory = Some(out.directory.map_or(base.clone(), |d| d.resolve(&base)));
        if out.name.is_empty() {
            out.name = pick(&|d| !d.name.is_empty()).map(|d| d.name.clone()).unwrap_or_default();
        }
        if out.typ.is_none() {
            out.typ = pick(&|d| d.typ.is_some()).and_then(|d| d.typ.clone());
        }
        if out.version.is_none() {
            out.version = pick(&|d| d.version.is_some()).and_then(|d| d.version);
        }
        out
    }

    /// The full form `$PARSE` returns: every field, `.` and `;` even when
    /// empty.
    pub fn expanded(&self) -> String {
        let mut s = String::new();
        if let Some(n) = &self.node {
            s.push_str(&format!("{n}::"));
        }
        if let Some(d) = &self.device {
            s.push_str(&format!("{d}:"));
        }
        if let Some(d) = &self.directory {
            s.push_str(&d.to_string());
        }
        s.push_str(&format!("{}.{};", self.name, self.typ.as_deref().unwrap_or("")));
        if let Some(v) = self.version {
            s.push_str(&v.to_string());
        }
        s
    }
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
        assert_eq!(p("*.*.*").version, Some(Version::Wildcard));
        assert_eq!(p("A.B").version, None);
        for s in ["A.B;x", "A.B;+1", "A.B;40000", "A.B;^31"] {
            assert!(s.parse::<FileSpec>().is_err(), "{s} should fail");
        }
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
        assert!(p("[...]").directory.unwrap().relative);
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
    fn ods5_names() {
        // Extra dots belong to the name; the last one starts the type.
        let s = p("archive.tar.gz");
        assert_eq!(
            (s.name.as_str(), s.typ.as_deref()),
            ("archive^.tar", Some("gz"))
        );
        assert_eq!(unescape(&s.name).unwrap(), "archive.tar");
        assert_eq!(p("a.b.c;2").name, "a^.b");
        // Escapes, normalized to canonical form.
        let s = p("[my^ dir]My^_File^20x^,1^%.txt");
        assert_eq!(s.directory.as_ref().unwrap().parts, ["my^_dir"]);
        assert_eq!(s.name, "My^_File^_x^,1^%");
        assert_eq!(unescape(&s.name).unwrap(), "My File x,1%");
        // A literal ^. in a directory name is not a separator.
        assert_eq!(p("[a^.b.c]").directory.unwrap().parts, ["a^.b", "c"]);
        // Brackets inside names.
        assert_eq!(p("[d]x^[1^].y").name, "x^[1^]");
        // Unicode: ^U escape and raw.
        assert_eq!(unescape(&p("caf^U00E9.txt").name).unwrap(), "café");
        assert_eq!(p("café.txt").name, "café");
        assert_eq!(p("caf^E9.txt").name, "café");
        assert_eq!(p(".bashrc").typ.as_deref(), Some("bashrc"));
    }

    #[test]
    fn escape_round_trip() {
        for host in [
            "a b.c",
            "50%",
            "x*y",
            "we:ird?",
            "tab\there",
            "^caret",
            "ok-name_1$",
        ] {
            let e = escape(host);
            assert_eq!(unescape(&e).unwrap(), host, "{e}");
            assert!(!e.contains('*') && !e.contains(' '), "{e}");
        }
        assert_eq!(escape("a b.c"), "a^_b^.c");
        assert_eq!(escape("x*y"), "x^2Ay");
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
            "[a^.b]x^_y^.z.tar;1",
        ] {
            assert_eq!(p(s).to_string(), s);
        }
    }

    #[test]
    fn merge_like_parse() {
        // From F$PARSE on VMS with the default directory DKA200:[T.DCL].
        let cur = p("DKA200:[T.DCL]");
        let m = |s: &str, d: &str| p(s).merge(&[&p(d)], &cur).expanded();
        assert_eq!(m("[C]X", "DKA100:[A.B].TXT"), "DKA100:[C]X.TXT;");
        assert_eq!(m("X", "[A.B]Z.TXT"), "DKA200:[A.B]X.TXT;");
        assert_eq!(m("[-.X]Y", "[A.B]"), "DKA200:[T.X]Y.;");
        assert_eq!(m("[.X]Y", "[A.B]"), "DKA200:[T.DCL.X]Y.;");
        assert_eq!(m("X.Y;3", "DKA100:[A.B]Z.TXT"), "DKA100:[A.B]X.Y;3");
    }

    #[test]
    fn errors() {
        for s in [
            "[A", "[A..B]", "[A.]", "[.-]", "A B", ":X", "a^", "a^G", "a^U12", "A,B", "x\"y",
        ] {
            assert!(s.parse::<FileSpec>().is_err(), "{s} should fail");
        }
    }
}
