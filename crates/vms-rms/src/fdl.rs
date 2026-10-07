//! FDL, the File Definition Language that CREATE/FDL reads and
//! ANALYZE/RMS_FILE/FDL writes: sections (FILE, RECORD, AREA 0, KEY 1...)
//! of attributes, and the [`Design`] they describe.

use crate::{Area, BLOCK, Design, Error, Fab, KeyDesc, KeyType, Org, Rfm, Segment, rat};
use std::fmt;

/// The primary attributes: each starts a section.
const PRIMARIES: [&str; 14] = [
    "ACCESS",
    "ANALYSIS_OF_AREA",
    "ANALYSIS_OF_KEY",
    "AREA",
    "CONNECT",
    "DATE",
    "FILE",
    "IDENT",
    "JOURNALING",
    "KEY",
    "RECORD",
    "SHARING",
    "SYSTEM",
    "TITLE",
];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Fdl {
    pub sections: Vec<Section>,
}

/// A primary attribute and its secondary ones.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Section {
    /// Upper case: FILE, KEY, IDENT...
    pub name: String,
    /// The rest of its line: AREA's and KEY's number, IDENT's text.
    pub value: String,
    /// Names upper case; values as written, strings with their quotes.
    pub attrs: Vec<(String, String)>,
}

/// Reads FDL text. Names are not abbreviated.
pub fn parse(text: &str) -> Result<Fdl, Error> {
    let mut fdl = Fdl::default();
    for line in text.lines() {
        // A comment starts at a ! outside quotes.
        let mut quoted = false;
        let end = line
            .find(|c| {
                quoted ^= c == '"';
                c == '!' && !quoted
            })
            .unwrap_or(line.len());
        let line = line[..end].trim();
        if line.is_empty() {
            continue;
        }
        let (word, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        let (word, rest) = (word.to_ascii_uppercase(), rest.trim().to_string());
        if PRIMARIES.contains(&word.as_str()) {
            fdl.sections.push(Section::new(&word, rest));
        } else {
            let s = fdl
                .sections
                .last_mut()
                .ok_or(Error("FDL attribute outside a section"))?;
            s.attrs.push((word, rest));
        }
    }
    Ok(fdl)
}

/// The layout of ANALYZE/RMS_FILE/FDL: a blank line between sections,
/// values from column 24.
impl fmt::Display for Fdl {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        for (i, s) in self.sections.iter().enumerate() {
            if i > 0 {
                writeln!(f)?;
            }
            if s.value.is_empty() {
                writeln!(f, "{}", s.name)?;
            } else if s.value.bytes().all(|c| c.is_ascii_digit()) {
                writeln!(f, "{} {}", s.name, s.value)?;
            } else {
                writeln!(f, "{}\t{}", s.name, s.value)?;
            }
            for (k, v) in &s.attrs {
                writeln!(f, "\t{k:<23} {v}")?;
            }
        }
        Ok(())
    }
}

impl Fdl {
    /// The section `name value`, like ("KEY", "1").
    pub fn section(&self, name: &str, value: &str) -> Option<&Section> {
        self.sections
            .iter()
            .find(|s| s.name == name && s.value == value)
    }

    /// The numbered sections `name` (AREA, KEY), by number.
    fn numbered(&self, name: &str) -> Result<Vec<(u32, &Section)>, Error> {
        let mut v = Vec::new();
        for s in self.sections.iter().filter(|s| s.name == name) {
            v.push((s.value.parse().map_err(|_| Error("invalid FDL number"))?, s));
        }
        v.sort_by_key(|(n, _)| *n);
        Ok(v)
    }
}

impl Section {
    pub fn new(name: &str, value: impl fmt::Display) -> Section {
        Section {
            name: name.to_string(),
            value: value.to_string(),
            attrs: Vec::new(),
        }
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    pub fn push(&mut self, name: &str, value: impl fmt::Display) {
        self.attrs.push((name.to_string(), value.to_string()));
    }

    /// Sets `name`, adding it where it goes alphabetically if it is new.
    pub fn set(&mut self, name: &str, value: impl fmt::Display) {
        let value = value.to_string();
        match self.attrs.iter().position(|(k, _)| k.as_str() >= name) {
            Some(i) if self.attrs[i].0 == name => self.attrs[i].1 = value,
            Some(i) => self.attrs.insert(i, (name.to_string(), value)),
            None => self.attrs.push((name.to_string(), value)),
        }
    }

    /// A keyword, lower case.
    fn word(&self, name: &str) -> Option<String> {
        self.get(name).map(|v| v.to_ascii_lowercase())
    }

    fn num(&self, name: &str) -> Result<Option<u32>, Error> {
        self.get(name)
            .map(|v| v.parse().map_err(|_| Error("invalid FDL number")))
            .transpose()
    }

    fn yes(&self, name: &str) -> Result<Option<bool>, Error> {
        match self.word(name).as_deref() {
            None => Ok(None),
            Some("yes" | "true") => Ok(Some(true)),
            Some("no" | "false") => Ok(Some(false)),
            _ => Err(Error("FDL yes or no expected")),
        }
    }

    fn string(&self, name: &str) -> Option<String> {
        let v = self.get(name)?;
        let inner = v.strip_prefix('"').and_then(|v| v.strip_suffix('"'));
        Some(inner.map_or(v.to_string(), |s| s.replace("\"\"", "\"")))
    }
}

/// Bytes in a bucket of area `n`: its bucket size, else the file's.
fn bucket_bytes(areas: &[Area], bks: u8, n: u8) -> u32 {
    let a = areas
        .iter()
        .find(|a| a.number == n)
        .map_or(0, |a| a.bucket_size);
    (if a == 0 { bks } else { a }).max(1) as u32 * BLOCK as u32
}

fn yes_no(b: bool) -> &'static str {
    if b { "yes" } else { "no" }
}

const ORGS: [(Org, &str); 3] = [
    (Org::Seq, "sequential"),
    (Org::Rel, "relative"),
    (Org::Idx, "indexed"),
];

const FORMATS: [(Rfm, &str); 7] = [
    (Rfm::Fix, "fixed"),
    (Rfm::Var, "variable"),
    (Rfm::Vfc, "vfc"),
    (Rfm::Stm, "stream"),
    (Rfm::Stmlf, "stream_lf"),
    (Rfm::Stmcr, "stream_cr"),
    (Rfm::Udf, "undefined"),
];

const CONTROLS: [(u8, &str); 4] = [
    (rat::CR, "carriage_return"),
    (rat::FTN, "fortran"),
    (rat::PRN, "print"),
    (0, "none"),
];

/// FDL TYPE: the ascending names; D before one makes it descending.
const TYPES: [(KeyType, &str); 9] = [
    (KeyType::String, "string"),
    (KeyType::Int2, "int2"),
    (KeyType::Int4, "int4"),
    (KeyType::Int8, "int8"),
    (KeyType::Bin2, "bin2"),
    (KeyType::Bin4, "bin4"),
    (KeyType::Bin8, "bin8"),
    (KeyType::Decimal, "decimal"),
    (KeyType::Collated, "collated"),
];

fn keyword<T: Copy>(table: &[(T, &str)], v: Option<String>, default: T) -> Result<T, Error> {
    match v {
        None => Ok(default),
        Some(v) => table
            .iter()
            .find(|t| t.1 == v)
            .map(|t| t.0)
            .ok_or(Error("invalid FDL keyword")),
    }
}

fn name_of<T: PartialEq>(table: &[(T, &'static str)], v: T) -> &'static str {
    table.iter().find(|t| t.0 == v).map_or("", |t| t.1)
}

/// What an FDL says a file is, with RMS's defaults for what it leaves out.
/// Without AREA sections, the FILE section's ALLOCATION, EXTENSION,
/// BUCKET_SIZE and contiguity make area 0. Fills become bytes.
pub fn to_design(fdl: &Fdl) -> Result<Design, Error> {
    let none = Section::default();
    let file = fdl.section("FILE", "").unwrap_or(&none);
    let record = fdl.section("RECORD", "").unwrap_or(&none);
    let org = keyword(&ORGS, file.word("ORGANIZATION"), Org::Seq)?;
    let rfm = keyword(&FORMATS, record.word("FORMAT"), Rfm::Var)?;
    let mut rat = keyword(&CONTROLS, record.word("CARRIAGE_CONTROL"), rat::CR)?;
    if record.yes("BLOCK_SPAN")? == Some(false) {
        rat |= rat::BLK;
    }
    let fab = Fab {
        org,
        rfm,
        rat,
        mrs: record.num("SIZE")?.unwrap_or(0) as u16,
        lrl: 0,
        fsz: match rfm {
            Rfm::Vfc => record.num("CONTROL_FIELD_SIZE")?.unwrap_or(2) as u8,
            _ => 0,
        },
        bks: file.num("BUCKET_SIZE")?.unwrap_or(0) as u8,
        deq: file.num("EXTENSION")?.unwrap_or(0) as u16,
    };
    let area = |number, s: &Section| -> Result<Area, Error> {
        Ok(Area {
            number,
            allocation: s.num("ALLOCATION")?.unwrap_or(0),
            bucket_size: s.num("BUCKET_SIZE")?.unwrap_or(0) as u8,
            extension: s.num("EXTENSION")?.unwrap_or(0) as u16,
            contiguous: s.yes("CONTIGUOUS")?.unwrap_or(false),
            best_try_contiguous: s.yes("BEST_TRY_CONTIGUOUS")?.unwrap_or(false),
        })
    };
    let mut areas = Vec::new();
    for (n, s) in fdl.numbered("AREA")? {
        areas.push(area(n as u8, s)?);
    }
    if areas.is_empty() {
        areas.push(area(0, file)?);
    }
    let key0 = fdl.section("KEY", "0");
    let prologue = match key0.map(|k| k.num("PROLOG")).transpose()?.flatten() {
        Some(p) => p as u8,
        None if org == Org::Idx => 3,
        None => (org == Org::Rel) as u8,
    };
    // A fill percentage as bytes of the area's buckets.
    let fill = |pct: Option<u32>, area: u8| {
        (pct.unwrap_or(100) * bucket_bytes(&areas, fab.bks, area) / 100) as u16
    };
    let mut keys = Vec::new();
    for (n, k) in fdl.numbered("KEY")? {
        let alt = n != 0;
        let t = k.word("TYPE").unwrap_or_else(|| "string".into());
        let descending = t.starts_with('d') && !t.starts_with("dec");
        let typ = keyword(
            &TYPES,
            Some(t[descending as usize..].into()),
            KeyType::String,
        )?;
        let mut segments = Vec::new();
        while let Some(length) = k.num(&format!("SEG{}_LENGTH", segments.len()))? {
            let position = k.num(&format!("SEG{}_POSITION", segments.len()))?;
            segments.push(Segment {
                position: position.unwrap_or(0) as u16,
                length: length as u16,
            });
        }
        let compress = typ == KeyType::String && prologue == 3;
        let data_area = k.num("DATA_AREA")?.unwrap_or(0) as u8;
        let index_area = k.num("INDEX_AREA")?.unwrap_or(0) as u8;
        keys.push(KeyDesc {
            number: n as u8,
            name: k.string("NAME").unwrap_or_default(),
            typ,
            descending,
            segments,
            duplicates: k.yes("DUPLICATES")?.unwrap_or(alt),
            changes: k.yes("CHANGES")?.unwrap_or(alt),
            null_key: k.yes("NULL_KEY")?.unwrap_or(false),
            null_value: k.num("NULL_VALUE")?.unwrap_or(0) as u8,
            data_area,
            index_area,
            level1_index_area: k.num("LEVEL1_INDEX_AREA")?.unwrap_or(index_area as u32) as u8,
            data_fill: fill(k.num("DATA_FILL")?, data_area),
            index_fill: fill(k.num("INDEX_FILL")?, index_area),
            data_key_compression: k.yes("DATA_KEY_COMPRESSION")?.unwrap_or(compress),
            data_record_compression: k.yes("DATA_RECORD_COMPRESSION")?.unwrap_or(prologue == 3),
            index_compression: k.yes("INDEX_COMPRESSION")?.unwrap_or(compress),
        });
    }
    Ok(Design {
        fab,
        max_record_number: file.num("MAX_RECORD_NUMBER")?.unwrap_or(0),
        prologue,
        areas,
        keys,
    })
}

/// A design as FDL: FILE, RECORD and, for an indexed file, AREA and KEY
/// sections, attributes in the order ANALYZE/RMS_FILE/FDL gives them.
pub fn from_design(d: &Design) -> Fdl {
    let f = &d.fab;
    let first = d.areas.first().cloned().unwrap_or_default();
    let mut file = Section::new("FILE", "");
    file.push(
        "ALLOCATION",
        d.areas.iter().map(|a| a.allocation).sum::<u32>(),
    );
    file.push("BEST_TRY_CONTIGUOUS", yes_no(first.best_try_contiguous));
    if f.org != Org::Seq {
        file.push("BUCKET_SIZE", f.bks);
    }
    file.push("CONTIGUOUS", yes_no(first.contiguous));
    file.push("EXTENSION", f.deq);
    if f.org == Org::Rel {
        file.push("MAX_RECORD_NUMBER", d.max_record_number);
    }
    file.push("ORGANIZATION", name_of(&ORGS, f.org));
    let mut record = Section::new("RECORD", "");
    record.push("BLOCK_SPAN", yes_no(f.rat & rat::BLK == 0));
    record.push(
        "CARRIAGE_CONTROL",
        name_of(&CONTROLS, f.rat & (rat::CR | rat::FTN | rat::PRN)),
    );
    if f.rfm == Rfm::Vfc {
        record.push("CONTROL_FIELD_SIZE", f.fsz);
    }
    record.push("FORMAT", name_of(&FORMATS, f.rfm));
    record.push("SIZE", f.mrs);
    let mut fdl = Fdl {
        sections: vec![file, record],
    };
    if f.org != Org::Idx {
        return fdl;
    }
    for a in &d.areas {
        let mut s = Section::new("AREA", a.number);
        s.push("ALLOCATION", a.allocation);
        if a.best_try_contiguous {
            s.push("BEST_TRY_CONTIGUOUS", "yes");
        }
        s.push("BUCKET_SIZE", a.bucket_size);
        if a.contiguous {
            s.push("CONTIGUOUS", "yes");
        }
        s.push("EXTENSION", a.extension);
        fdl.sections.push(s);
    }
    let pct = |bytes: u16, area| bytes as u32 * 100 / bucket_bytes(&d.areas, f.bks, area);
    for k in &d.keys {
        let mut s = Section::new("KEY", k.number);
        s.push("CHANGES", yes_no(k.changes));
        s.push("DATA_KEY_COMPRESSION", yes_no(k.data_key_compression));
        if k.number == 0 {
            s.push("DATA_RECORD_COMPRESSION", yes_no(k.data_record_compression));
        }
        s.push("DATA_AREA", k.data_area);
        s.push("DATA_FILL", pct(k.data_fill, k.data_area));
        s.push("DUPLICATES", yes_no(k.duplicates));
        s.push("INDEX_AREA", k.index_area);
        s.push("INDEX_COMPRESSION", yes_no(k.index_compression));
        s.push("INDEX_FILL", pct(k.index_fill, k.index_area));
        s.push("LEVEL1_INDEX_AREA", k.level1_index_area);
        s.push("NAME", format!("\"{}\"", k.name.replace('"', "\"\"")));
        s.push("NULL_KEY", yes_no(k.null_key));
        if k.null_key {
            s.push("NULL_VALUE", k.null_value);
        }
        if k.number == 0 {
            s.push("PROLOG", d.prologue);
        }
        for (i, g) in k.segments.iter().enumerate() {
            s.push(&format!("SEG{i}_LENGTH"), g.length);
            s.push(&format!("SEG{i}_POSITION"), g.position);
        }
        let t = name_of(&TYPES, k.typ);
        s.push(
            "TYPE",
            if k.descending {
                format!("d{t}")
            } else {
                t.into()
            },
        );
        fdl.sections.push(s);
    }
    fdl
}

/// What CREATE/FDL finds wrong in FDL text, by statement (a line with
/// something on it, from 1) and word, as VMS reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// A word that is no attribute of its section: VMS calls it an
    /// unrecognized primary keyword, and won't go on.
    Primary(usize, String),
    /// A value the attribute can't take: a warning, but nothing is made.
    Value(usize, String),
}

/// What an attribute's value must be.
#[derive(Clone, Copy)]
enum Kind {
    Num,
    YesNo,
    Any,
    Words(&'static [&'static str]),
}

const NUM: Kind = Kind::Num;
const YN: Kind = Kind::YesNo;
const ANY: Kind = Kind::Any;

/// The attributes of the sections CREATE/FDL reads; the others (DATE,
/// ACCESS, ANALYSIS_OF_KEY ...) take any.
fn attributes(section: &str) -> Option<&'static [(&'static str, Kind)]> {
    const ORG: &[&str] = &["sequential", "relative", "indexed"];
    const FMT: &[&str] = &[
        "fixed",
        "variable",
        "vfc",
        "stream",
        "stream_lf",
        "stream_cr",
        "undefined",
    ];
    const CC: &[&str] = &["carriage_return", "fortran", "print", "none"];
    Some(match section {
        "FILE" => &[
            ("ALLOCATION", NUM),
            ("BEST_TRY_CONTIGUOUS", YN),
            ("BUCKET_SIZE", NUM),
            ("CLUSTER_SIZE", NUM),
            ("CONTIGUOUS", YN),
            ("DEFAULT_NAME", ANY),
            ("DEFERRED_WRITE", YN),
            ("DELETE_ON_CLOSE", YN),
            ("DIRECTORY_ENTRY", YN),
            ("ERASE_ON_DELETE", YN),
            ("EXTENSION", NUM),
            ("FILE_MONITORING", YN),
            ("GLBUFF_CNT_V83", NUM),
            ("GLBUFF_FLAGS_V83", ANY),
            ("GLOBAL_BUFFER_COUNT", NUM),
            ("MAX_RECORD_NUMBER", NUM),
            ("MAXIMIZE_VERSION", YN),
            ("NAME", ANY),
            ("ORGANIZATION", Kind::Words(ORG)),
            ("OUTPUT_FILE_PARSE", YN),
            ("OWNER", ANY),
            ("PRINT_ON_CLOSE", YN),
            ("PROTECTION", ANY),
            ("READ_CHECK", YN),
            ("REVISION", NUM),
            ("SEQUENTIAL_ONLY", YN),
            ("SUBMIT_ON_CLOSE", YN),
            ("SUPERSEDE", YN),
            ("TEMPORARY", YN),
            ("TRUNCATE_ON_CLOSE", YN),
            ("USER_FILE_OPEN", YN),
            ("WINDOW_SIZE", NUM),
            ("WRITE_CHECK", YN),
        ],
        "RECORD" => &[
            ("BLOCK_SPAN", YN),
            ("CARRIAGE_CONTROL", Kind::Words(CC)),
            ("CONTROL_FIELD_SIZE", NUM),
            ("FORMAT", Kind::Words(FMT)),
            ("SIZE", NUM),
        ],
        "AREA" => &[
            ("ALLOCATION", NUM),
            ("BEST_TRY_CONTIGUOUS", YN),
            ("BUCKET_SIZE", NUM),
            ("CONTIGUOUS", YN),
            ("EXACT_POSITIONING", YN),
            ("EXTENSION", NUM),
            ("POSITION", ANY),
            ("VOLUME", NUM),
        ],
        "KEY" => &[
            ("CHANGES", YN),
            ("DATA_AREA", NUM),
            ("DATA_FILL", NUM),
            ("DATA_KEY_COMPRESSION", YN),
            ("DATA_RECORD_COMPRESSION", YN),
            ("DUPLICATES", YN),
            ("INDEX_AREA", NUM),
            ("INDEX_COMPRESSION", YN),
            ("INDEX_FILL", NUM),
            ("LEVEL1_INDEX_AREA", NUM),
            ("NAME", ANY),
            ("NULL_KEY", YN),
            ("NULL_VALUE", ANY),
            ("PROLOG", NUM),
            ("TYPE", ANY),
        ],
        "SYSTEM" => &[("SOURCE", ANY), ("TARGET", ANY)],
        "IDENT" | "TITLE" => &[],
        _ => return None,
    })
}

pub fn check(text: &str) -> Vec<Problem> {
    let Ok(fdl) = parse(text) else {
        return vec![Problem::Primary(1, String::new())];
    };
    // Statements as parse saw them: each section's line, then its attributes'.
    let mut out = Vec::new();
    let mut n = 0;
    for s in &fdl.sections {
        n += 1;
        let known = attributes(&s.name);
        for (name, value) in &s.attrs {
            n += 1;
            let Some(known) = known else { continue };
            let seg = s.name == "KEY"
                && name
                    .strip_prefix("SEG")
                    .and_then(|r| r.split_once('_'))
                    .is_some_and(|(d, f)| {
                        d.parse::<u8>().is_ok() && (f == "LENGTH" || f == "POSITION")
                    });
            let kind = match known.iter().find(|(k, _)| k == name) {
                Some((_, k)) => *k,
                None if seg => NUM,
                None => {
                    out.push(Problem::Primary(n, name.clone()));
                    return out;
                }
            };
            let v = value.to_ascii_lowercase();
            let ok = match kind {
                Kind::Num => !v.is_empty() && v.bytes().all(|c| c.is_ascii_digit()),
                Kind::YesNo => matches!(v.as_str(), "yes" | "no" | "true" | "false"),
                Kind::Any => true,
                Kind::Words(w) => w.contains(&v.as_str()),
            };
            if !ok {
                out.push(Problem::Value(n, value.to_ascii_uppercase()));
            }
        }
    }
    out
}

#[cfg(test)]
mod check_tests {
    use super::*;

    #[test]
    fn problems() {
        // As CREATE/FDL reported them (fixtures/fdlutil).
        assert_eq!(
            check("FILE\n\tORGANIZATION\tsideways\n"),
            [Problem::Value(2, "SIDEWAYS".into())]
        );
        assert_eq!(
            check("RECORD\n\tFORMATT\t\tfixed\n"),
            [Problem::Primary(2, "FORMATT".into())]
        );
        assert_eq!(
            check("RECORD\n\tSIZE\t\tabc\n"),
            [Problem::Value(2, "ABC".into())]
        );
        assert_eq!(
            check("KEY 0\n\tSEG0_LENGTH 4\n\tSEG0_POSITION 0\n! c\n\n"),
            []
        );
    }
}
