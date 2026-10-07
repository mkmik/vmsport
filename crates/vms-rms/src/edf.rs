//! EDIT/FDL's optimizer, as `EDIT/FDL/NOINTERACTIVE/ANALYSIS=` runs it:
//! an indexed file's design (bucket sizes, areas, allocations, extensions)
//! from an input FDL and ANALYZE/RMS_FILE/FDL's statistics of the data.
//!
//! The rules are fitted to what the FDL editor of OpenVMS 8.4 wrote for a
//! corpus of cases (fixtures/edf; crates/vms-rms/tests/edf.rs checks them
//! all). For each key EDF estimates a record (or SIDR) size, takes the
//! bucket size giving the flattest index, and sizes the data and index
//! parts; GRANULARITY says which parts share an area.

use crate::fdl::{Fdl, Section};

/// What EDIT/FDL/NOINTERACTIVE makes of its two files.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The optimized design.
    Fdl(Fdl),
    /// The input isn't an indexed file's design, but the analysis is: EDF
    /// says so and writes nothing.
    NotIndexed,
    /// Nothing to optimize (the analysis isn't an indexed file's).
    Nothing,
}

/// Bytes of a data bucket EDF doesn't count on, and of an index bucket.
const DATA_OVERHEAD: u32 = 16;
const INDEX_OVERHEAD: u32 = 17;
/// A one-level index only with buckets up to this many blocks.
const FLAT_LIMIT: u32 = 18;
const MAX_BUCKET: u32 = 63;

/// The design of `input` optimized for the data `analysis` describes, as
/// GRANULARITY (1 to 4) groups it into areas; `now` dates it
/// (` 7-OCT-2026 03:59:08`).
pub fn optimize(input: &Fdl, analysis: &Fdl, granularity: u8, now: &str) -> Outcome {
    let org = |f: &Fdl| {
        f.section("FILE", "")
            .and_then(|s| s.get("ORGANIZATION"))
            .map(str::to_ascii_lowercase)
    };
    if org(analysis).is_some_and(|o| o != "indexed") {
        return Outcome::Nothing;
    }
    if org(input).as_deref() != Some("indexed") {
        return Outcome::NotIndexed;
    }
    // An analysis that doesn't say has the default cluster of 3.
    let cluster = num(analysis.section("FILE", ""), "CLUSTER_SIZE")
        .unwrap_or(3)
        .max(1);
    let keys: Vec<&Section> = input.sections.iter().filter(|s| s.name == "KEY").collect();
    if keys.is_empty() {
        return Outcome::Nothing;
    }
    let record = input.section("RECORD", "");
    let var = record
        .and_then(|r| r.get("FORMAT"))
        .is_none_or(|f| f.eq_ignore_ascii_case("variable"));
    let size = num(record, "SIZE").unwrap_or(0);

    let mut parts = Vec::new();
    for (k, key) in keys.iter().enumerate() {
        let Some(stats) = analysis.section("ANALYSIS_OF_KEY", &k.to_string()) else {
            return Outcome::Nothing;
        };
        let n = num(Some(stats), "DATA_RECORD_COUNT").unwrap_or(0).max(1);
        let klen = key_length(key);
        let kc = string_key(key) && yes(key, "DATA_KEY_COMPRESSION", true);
        let dkc = percent(stats, "DATA_KEY_COMPRESSION");
        let keypart = if kc {
            (klen * (100 - dkc)).div_ceil(100)
        } else {
            klen
        };
        let fill = num(Some(key), "DATA_FILL").unwrap_or(100).clamp(1, 100);
        let (entry, bmin, adjust) = if k == 0 {
            // A data record: compressed key and data, their overheads.
            let len = if var {
                num(Some(stats), "MEAN_DATA_LENGTH").unwrap_or(size)
            } else {
                size
            };
            let rc = yes(key, "DATA_RECORD_COMPRESSION", true);
            let drc = percent(stats, "DATA_RECORD_COMPRESSION");
            let rest = len.saturating_sub(klen);
            let datapart = if rc {
                (rest * (100 - drc)).div_ceil(100)
            } else {
                rest
            };
            let overhead =
                9 + if kc { 2 } else { 0 } + if rc { 3 } else { 0 } + if var { 2 } else { 0 };
            // A bucket holds the largest record, uncompressed.
            let largest = size.max(len) + 9 + if var { 2 } else { 0 };
            (
                keypart + datapart + overhead,
                (largest + DATA_OVERHEAD).div_ceil(512),
                rc,
            )
        } else {
            // A SIDR: the key and a record pointer, 13 bytes; with
            // duplicates, as if each of them took 22 and shared the key.
            let dps = match yes(key, "DUPLICATES", true) {
                true => num(Some(stats), "DUPLICATES_PER_SIDR").unwrap_or(0),
                false => 0,
            };
            ((22 * dps + keypart + 13).div_ceil(dps + 1), 1, false)
        };
        let plan = plan(entry, klen + 4, n, fill, bmin, cluster, adjust);
        parts.push((plan, fill));
    }

    // Areas: which parts (key, data or index) each holds.
    let area_of = |k: usize, index: bool| -> u32 {
        match (granularity, k, index) {
            (1, _, _) => 0,
            (_, 0, false) => 0,
            (2, _, _) => 1,
            (_, 0, true) => 1,
            (3, _, _) | (_, _, false) => 2,
            _ => 3,
        }
    };
    let prologue = 1 + (keys.len() as u32 - 1).div_ceil(2) + 1;
    let mut areas: Vec<Area> = Vec::new();
    for (k, (p, _)) in parts.iter().enumerate() {
        for (index, blocks) in [(false, p.data), (true, p.index)] {
            let a = area_of(k, index) as usize;
            if areas.len() <= a {
                areas.resize(a + 1, Area::default());
            }
            let m = lcm(p.bks, cluster);
            let area = &mut areas[a];
            area.allocation += blocks.div_ceil(m) * m;
            area.extension += (blocks / 4).max(p.bks).div_ceil(m) * m;
            area.bks = area.bks.max(p.bks);
        }
    }
    areas[0].allocation += prologue.div_ceil(cluster) * cluster;

    let mut out = Fdl::default();
    out.sections.push(Section::new(
        "IDENT",
        format!("FDL_VERSION 02\t\"{now}  OpenVMS FDL Editor\""),
    ));
    let mut system = Section::new("SYSTEM", "");
    system.push("SOURCE", "\"OpenVMS\"");
    out.sections.push(system);
    let mut file = carry(
        input.section("FILE", ""),
        &[
            "ALLOCATION",
            "BEST_TRY_CONTIGUOUS",
            "BUCKET_SIZE",
            "CLUSTER_SIZE",
            "EXTENSION",
        ],
    );
    default(&mut file, "GLBUFF_FLAGS_V83", "none");
    file.name = "FILE".into();
    file.attrs.sort_by_key(|(k, _)| file_order(k));
    out.sections.push(file);
    let mut rec = carry(record, &[]);
    rec.name = "RECORD".into();
    default(&mut rec, "CARRIAGE_CONTROL", "carriage_return");
    rec.attrs.sort();
    out.sections.push(rec);
    for (n, a) in areas.iter().enumerate() {
        let mut s = Section::new("AREA", n);
        s.push("ALLOCATION", a.allocation);
        s.push("BEST_TRY_CONTIGUOUS", "yes");
        s.push("BUCKET_SIZE", a.bks);
        s.push("EXTENSION", a.extension.min(65535 / a.bks * a.bks));
        out.sections.push(s);
    }
    for (k, key) in keys.iter().enumerate() {
        let mut s = carry(
            Some(key),
            &["DATA_AREA", "INDEX_AREA", "LEVEL1_INDEX_AREA", "INDEX_FILL"],
        );
        s.name = "KEY".into();
        s.value = k.to_string();
        let string = string_key(key);
        s.push("DATA_AREA", area_of(k, false));
        s.push("INDEX_AREA", area_of(k, true));
        s.push("LEVEL1_INDEX_AREA", area_of(k, true));
        s.push("INDEX_FILL", parts[k].1);
        default(&mut s, "CHANGES", "no");
        default(&mut s, "DATA_FILL", "100");
        default(&mut s, "DUPLICATES", if k == 0 { "no" } else { "yes" });
        default(&mut s, "INDEX_COMPRESSION", "no");
        default(&mut s, "TYPE", "string");
        if string {
            default(&mut s, "DATA_KEY_COMPRESSION", "yes");
        } else {
            s.set("DATA_KEY_COMPRESSION", "no");
        }
        if k == 0 {
            default(&mut s, "DATA_RECORD_COMPRESSION", "yes");
            default(&mut s, "PROLOG", "3");
        }
        s.attrs.sort();
        out.sections.push(s);
    }
    Outcome::Fdl(out)
}

#[derive(Clone, Default)]
struct Area {
    allocation: u32,
    extension: u32,
    bks: u32,
}

/// One key's bucket size, and its data and index parts in blocks.
#[derive(Debug, Clone, Copy)]
struct Plan {
    bks: u32,
    data: u32,
    index: u32,
}

/// The bucket size for `n` entries of `size` bytes and index entries of
/// `entry` bytes at `fill` percent: the smallest with the fewest index
/// levels (one level only up to FLAT_LIMIT blocks); then, if `adjust`,
/// one up to half as big again whose multiple with the cluster is least.
fn plan(size: u32, entry: u32, n: u32, fill: u32, bmin: u32, cluster: u32, adjust: bool) -> Plan {
    let at = |b: u32| -> Option<(u32, Plan)> {
        let usable = |overhead: u32| u64::from(512 * b - overhead) * u64::from(fill);
        let per_bucket = (usable(DATA_OVERHEAD) / (100 * u64::from(size))) as u32;
        let per_index = (usable(INDEX_OVERHEAD) / (100 * u64::from(entry))) as u32;
        if per_bucket == 0 || per_index < 2 {
            return None;
        }
        let data = n.div_ceil(per_bucket);
        let (mut levels, mut index, mut x) = (0, 0, data);
        loop {
            x = x.div_ceil(per_index);
            levels += 1;
            index += x;
            if x <= 1 {
                break;
            }
        }
        Some((
            levels,
            Plan {
                bks: b,
                data: data * b,
                index: index * b,
            },
        ))
    };
    let options: Vec<(u32, Plan)> = (bmin.max(1)..=MAX_BUCKET)
        .filter_map(at)
        .filter(|(levels, p)| *levels > 1 || p.bks <= FLAT_LIMIT)
        .collect();
    let Some(&(_, best)) = options.iter().min_by_key(|(levels, p)| (*levels, p.bks)) else {
        return Plan {
            bks: bmin,
            data: n * bmin,
            index: bmin,
        };
    };
    if !adjust || 3 * best.bks <= cluster {
        return best;
    }
    let b = (best.bks..=(best.bks * 3 / 2).min(MAX_BUCKET))
        .min_by_key(|&b| (lcm(b, cluster), b))
        .unwrap();
    at(b).map_or(best, |(_, p)| p)
}

fn lcm(a: u32, b: u32) -> u32 {
    let (mut x, mut y) = (a, b);
    while y != 0 {
        (x, y) = (y, x % y);
    }
    a / x * b
}

fn num(s: Option<&Section>, name: &str) -> Option<u32> {
    s?.get(name)?.trim().parse().ok()
}

/// A percentage ANALYZE gave: what isn't 1 to 99 counts as 0.
fn percent(s: &Section, name: &str) -> u32 {
    s.get(name)
        .and_then(|v| v.trim().parse::<i64>().ok())
        .filter(|v| (1..100).contains(v))
        .unwrap_or(0) as u32
}

fn yes(s: &Section, name: &str, default: bool) -> bool {
    s.get(name)
        .map_or(default, |v| v.eq_ignore_ascii_case("yes"))
}

fn key_length(key: &Section) -> u32 {
    key.attrs
        .iter()
        .filter(|(k, _)| k.starts_with("SEG") && k.ends_with("_LENGTH"))
        .filter_map(|(_, v)| v.trim().parse::<u32>().ok())
        .sum()
}

/// Only string keys compress.
fn string_key(key: &Section) -> bool {
    key.get("TYPE")
        .is_none_or(|t| t.eq_ignore_ascii_case("string"))
}

/// A section's attributes but `drop`.
fn carry(s: Option<&Section>, drop: &[&str]) -> Section {
    let mut out = Section::default();
    if let Some(s) = s {
        out.attrs = s
            .attrs
            .iter()
            .filter(|(k, _)| !drop.contains(&k.as_str()))
            .cloned()
            .collect();
    }
    out
}

fn default(s: &mut Section, name: &str, value: &str) {
    if s.get(name).is_none() {
        s.push(name, value);
    }
}

/// FILE's attributes in EDF's order: by name, the global buffer ones last.
fn file_order(name: &str) -> (usize, String) {
    let last = ["GLOBAL_BUFFER_COUNT", "GLBUFF_CNT_V83", "GLBUFF_FLAGS_V83"];
    (
        last.iter().position(|l| *l == name).map_or(0, |i| i + 1),
        name.to_string(),
    )
}

/// EDF's layout: values at column 32, reached with tabs.
pub fn text(fdl: &Fdl) -> String {
    let mut out = String::new();
    for (i, s) in fdl.sections.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        if s.name == "IDENT" {
            out += &format!("IDENT\t{}\n", s.value);
            continue;
        }
        out += &if s.value.is_empty() {
            s.name.clone()
        } else {
            format!("{} {}", s.name, s.value)
        };
        out.push('\n');
        for (k, v) in &s.attrs {
            let tabs = (32 - (8 + k.len()).min(31)).div_ceil(8);
            out += &format!("\t{k}{}{v}\n", "\t".repeat(tabs));
        }
    }
    out
}
