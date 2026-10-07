//! ANALYZE/RMS_FILE for indexed files: the checks behind /CHECK, and the
//! numbers behind /FDL's ANALYSIS_OF_KEY sections.

use super::*;

/// What a walk of one key's buckets found.
#[derive(Debug, Clone, Default)]
pub struct KeyStats {
    /// Index levels, the root's included.
    pub depth: u8,
    /// Data buckets, and their blocks.
    pub data_buckets: u32,
    pub data_blocks: u32,
    /// Bytes used in data buckets (after their headers).
    pub data_bytes: u64,
    /// Data records (primary key: live records; others: SIDRs).
    pub data_records: u64,
    /// SIDR elements of live records.
    pub pointers: u64,
    /// The records' lengths (primary key).
    pub data_length: u64,
    pub longest: usize,
    /// Bytes the keys would take uncompressed, and take.
    pub key_bytes: (u64, u64),
    /// Bytes the records' rest would take uncompressed, and take.
    pub rest_bytes: (u64, u64),
    pub index_buckets: u32,
    pub index_blocks: u32,
    pub index_bytes: u64,
    /// Index entries in level 1, and in all levels.
    pub level1_records: u64,
    pub index_records: u64,
    /// Bytes index keys would take uncompressed, and take.
    pub index_key_bytes: (u64, u64),
}

impl<B: Blocks> File<B> {
    /// Walks key `ki`'s index and data buckets: what is wrong with them,
    /// and what they hold.
    pub fn audit(&mut self, ki: usize) -> Result<(Vec<String>, KeyStats), Cond> {
        let p = self.prologue()?;
        let k = p.keys[ki].clone();
        let mut errs = Vec::new();
        let mut st = KeyStats::default();
        if k.empty {
            return Ok((errs, st));
        }
        st.depth = k.root_level;
        // The index, level by level from the root.
        let mut level_vbns = vec![k.root];
        let mut level1 = Vec::new();
        for level in (1..=k.root_level).rev() {
            let mut next_level = Vec::new();
            let mut prev_key: Option<Vec<u8>> = None;
            for &vbn in &level_vbns {
                let b = self.read(vbn, k.index_bucket)?;
                if b.b[0] != b.b[b.b.len() - 1] || b.level() != level || b.b[1] != k.desc.number {
                    errs.push(format!("***  VBN {vbn}:  bad index bucket header"));
                    continue;
                }
                let es = Self::index_entries(&k, &b)?;
                st.index_buckets += 1;
                st.index_blocks += k.index_bucket as u32;
                st.index_bytes += (b.free() - HDR + es.len() * b.ptr_size()) as u64;
                st.index_records += es.len() as u64;
                for e in &es {
                    if let Some(pk) = &prev_key
                        && compare(&k.desc, pk, &e.key) != Ordering::Less
                    {
                        errs.push(format!("***  VBN {vbn}:  index keys out of order"));
                    }
                    prev_key = Some(e.key.clone());
                    st.index_key_bytes.0 += k.len() as u64;
                    next_level.push(e.vbn);
                }
                st.index_key_bytes.1 += (b.free() - HDR) as u64;
                if level == 1 {
                    st.level1_records += es.len() as u64;
                    level1.extend(es);
                }
            }
            level_vbns = next_level;
        }
        // The data buckets along the chain from the first: those the
        // index names, and continuations between them.
        let mut vbn = k.first;
        let mut seen = std::collections::HashSet::new();
        let mut entry = 0;
        let mut prev: Option<Vec<u8>> = None;
        loop {
            if !seen.insert(vbn) {
                errs.push(format!("***  VBN {vbn}:  data bucket chain loops"));
                break;
            }
            let b = self.read(vbn, k.data_bucket)?;
            if b.b[0] != b.b[b.b.len() - 1] || b.level() != 0 || b.b[1] != k.desc.number {
                errs.push(format!("***  VBN {vbn}:  bad data bucket header"));
                break;
            }
            st.data_buckets += 1;
            st.data_blocks += k.data_bucket as u32;
            st.data_bytes += (b.free() - HDR) as u64;
            let keys = if k.desc.number == 0 {
                self.audit_primary(&k, &b, &mut st, &mut errs)?
            } else {
                self.audit_sidrs(&k, &b, &mut st, &mut errs)?
            };
            for key in &keys {
                if let Some(pk) = &prev
                    && compare(&k.desc, pk, key) == Ordering::Greater
                {
                    errs.push(format!("***  VBN {vbn}:  data keys out of order"));
                }
                prev = Some(key.clone());
            }
            // The index names this bucket, unless it continues the one
            // before with the same key.
            if level1.get(entry).is_some_and(|e| e.vbn == vbn) {
                if let Some(last) = keys.last()
                    && compare(&k.desc, last, &level1[entry].key) == Ordering::Greater
                {
                    errs.push(format!("***  VBN {vbn}:  keys above the index's"));
                }
                entry += 1;
            } else if keys.is_empty() {
                errs.push(format!(
                    "***  VBN {vbn}:  empty bucket the index doesn't name"
                ));
            }
            if b.last() {
                if b.next() != k.first {
                    errs.push(format!(
                        "***  VBN {vbn}:  last bucket doesn't lead to the first"
                    ));
                }
                break;
            }
            vbn = b.next();
        }
        if entry != level1.len() {
            errs.push(format!(
                "***  key {}: {} of {} level 1 index entries name buckets off the chain",
                k.desc.number,
                level1.len() - entry,
                level1.len()
            ));
        }
        Ok((errs, st))
    }

    fn audit_primary(
        &mut self,
        k: &Key,
        b: &Bucket,
        st: &mut KeyStats,
        errs: &mut Vec<String>,
    ) -> Result<Vec<Vec<u8>>, Cond> {
        let vbn = b.vbn;
        let (recs, rrvs) = self.recs(k, b)?;
        let mut prev_key: Option<&[u8]> = None;
        for r in &recs {
            if r.live() {
                let rec = self.record(k, r)?;
                st.data_records += 1;
                st.data_length += rec.len() as u64;
                st.longest = st.longest.max(rec.len());
                let kp = if k.desc.data_key_compression {
                    squeeze(&r.key, prev_key).len()
                } else {
                    k.len()
                };
                st.key_bytes.0 += k.len() as u64;
                st.key_bytes.1 += kp as u64;
                st.rest_bytes.0 += (rec.len() - k.len()) as u64;
                st.rest_bytes.1 += r.tail.len() as u64;
                if r.rrv.vbn == vbn && r.rrv.id != r.id {
                    errs.push(format!(
                        "***  VBN {vbn}:  record ID {} has a wrong RRV",
                        r.id
                    ));
                }
                if r.rrv.vbn != vbn && self.locate(k, r.rrv).map(|(x, _)| x.vbn) != Ok(vbn) {
                    errs.push(format!("***  VBN {vbn}:  record ID {}'s RRV is lost", r.id));
                }
            }
            prev_key = Some(&r.key);
            if r.id >= b.next_id() {
                errs.push(format!(
                    "***  VBN {vbn}:  record ID {} not below the next ID",
                    r.id
                ));
            }
        }
        for r in &rrvs {
            if r.ctl & DELETED == 0 && self.locate(k, Rfa { vbn, id: r.id }).is_err() {
                errs.push(format!("***  VBN {vbn}:  RRV ID {} points nowhere", r.id));
            }
        }
        Ok(recs.into_iter().map(|r| r.key).collect())
    }

    fn audit_sidrs(
        &mut self,
        k: &Key,
        b: &Bucket,
        st: &mut KeyStats,
        errs: &mut Vec<String>,
    ) -> Result<Vec<Vec<u8>>, Cond> {
        let vbn = b.vbn;
        let sidrs = Self::sidrs(k, b)?;
        let mut prev_key: Option<&[u8]> = None;
        for s in &sidrs {
            st.data_records += 1;
            st.pointers += s.elems.iter().filter(|e| e.live()).count() as u64;
            let kp = if k.desc.data_key_compression {
                squeeze(&s.key, prev_key).len()
            } else {
                k.len()
            };
            st.key_bytes.0 += k.len() as u64;
            st.key_bytes.1 += kp as u64;
            prev_key = Some(&s.key);
            for e in s.elems.iter().filter(|e| e.live()) {
                match self.get_rfa(e.rfa) {
                    Ok(rec) if k.in_record(&rec) && k.desc.extract(&rec) == s.key => {}
                    _ => errs.push(format!(
                        "***  VBN {vbn}:  SIDR pointer ({},{}) names no record with its key",
                        e.rfa.vbn, e.rfa.id
                    )),
                }
            }
        }
        Ok(sidrs.into_iter().map(|s| s.key).collect())
    }
}

/// `part` of `whole` in percent, as ANALYZE gives it: truncated.
fn percent(part: i64, whole: i64) -> i64 {
    if whole == 0 { 0 } else { part * 100 / whole }
}

/// `a / b` rounded.
fn mean(a: u64, b: u64) -> i64 {
    (a + b / 2).checked_div(b).unwrap_or(0) as i64
}

/// An FDL attribute line.
fn attr(name: &str, value: i64) -> String {
    format!("\t{name:<24}{value}\n")
}

fn type_name(typ: KeyType, descending: bool) -> String {
    let t = match typ {
        KeyType::String => "string",
        KeyType::Int2 => "signed word",
        KeyType::Bin2 => "unsigned word",
        KeyType::Int4 => "signed longword",
        KeyType::Bin4 => "unsigned longword",
        KeyType::Decimal => "packed decimal",
        KeyType::Int8 => "signed quadword",
        KeyType::Bin8 => "unsigned quadword",
        KeyType::Collated => "collated",
    };
    if descending {
        format!("descending {t}")
    } else {
        t.to_string()
    }
}

impl<B: Blocks> File<B> {
    /// ANALYZE/RMS_FILE/FDL's sections after the file's design, as FDL
    /// text: ANALYSIS_OF_AREA for each area, ANALYSIS_OF_KEY for each key.
    pub fn analysis(&mut self) -> Result<String, Cond> {
        let p = self.prologue()?;
        let mut sections = Vec::new();
        for (i, a) in p.areas.iter().enumerate() {
            let reclaimed = if a.reclaimed == 0 {
                0
            } else {
                a.bucket_size as i64
            };
            sections.push(format!(
                "ANALYSIS_OF_AREA {i}\n{}",
                attr("RECLAIMED_SPACE", reclaimed)
            ));
        }
        for (ki, k) in p.keys.iter().enumerate() {
            if k.empty {
                sections.push(format!(
                    "ANALYSIS_OF_KEY {ki}\n\t! This index is uninitialized - there are no records.\n"
                ));
                continue;
            }
            let (_, s) = self.audit(ki)?;
            let primary = ki == 0;
            let data_used = s.data_bytes + 15 * s.data_buckets as u64;
            let index_used = s.index_bytes + (HDR + IDX_TAIL) as u64 * s.index_buckets as u64;
            let index_full = s.index_records * (k.len() as u64 + 2);
            let mut a: Vec<(&'static str, i64)> = vec![
                (
                    "DATA_FILL",
                    percent(data_used as i64, s.data_blocks as i64 * BLK as i64),
                ),
                (
                    "DATA_KEY_COMPRESSION",
                    percent(
                        s.key_bytes.0 as i64 - s.key_bytes.1 as i64,
                        s.key_bytes.0 as i64,
                    ),
                ),
            ];
            if primary {
                a.push((
                    "DATA_RECORD_COMPRESSION",
                    percent(
                        s.rest_bytes.0 as i64 - s.rest_bytes.1 as i64,
                        s.rest_bytes.0 as i64,
                    ),
                ));
            }
            a.push(("DATA_RECORD_COUNT", s.data_records as i64));
            a.push(("DATA_SPACE_OCCUPIED", s.data_blocks as i64));
            a.push(("DEPTH", s.depth as i64));
            if !primary {
                a.push((
                    "DUPLICATES_PER_SIDR",
                    mean(s.pointers.saturating_sub(s.data_records), s.data_records),
                ));
            }
            a.push((
                "INDEX_COMPRESSION",
                percent(index_full as i64 - s.index_bytes as i64, index_full as i64),
            ));
            a.push((
                "INDEX_FILL",
                percent(index_used as i64, s.index_blocks as i64 * BLK as i64),
            ));
            a.push(("INDEX_SPACE_OCCUPIED", s.index_blocks as i64));
            a.push(("LEVEL1_RECORD_COUNT", s.level1_records as i64));
            if primary {
                a.push(("MEAN_DATA_LENGTH", mean(s.data_length, s.data_records)));
            } else {
                a.push(("MEAN_DATA_LENGTH", mean(s.data_bytes, s.data_records)));
            }
            a.push(("MEAN_INDEX_LENGTH", k.len() as i64 + 2));
            if primary {
                a.push(("LONGEST_RECORD_LENGTH", s.longest as i64));
            }
            let a: String = a.into_iter().map(|(n, v)| attr(n, v)).collect();
            sections.push(format!("ANALYSIS_OF_KEY {ki}\n{a}"));
        }
        Ok(sections.join("\n"))
    }

    /// ANALYZE/RMS_FILE/CHECK's lines after the RMS FILE ATTRIBUTES of an
    /// indexed file: the fixed prologue, the area and key descriptors, and
    /// what is wrong in the buckets. Returns them and the number of errors.
    pub fn check_report(&mut self) -> Result<(Vec<String>, usize), Cond> {
        let p = self.prologue()?;
        let first = p
            .blocks
            .iter()
            .find(|b| b.0 == 1)
            .map(|b| b.1.clone())
            .ok_or(status::BUG)?;
        let mut o: Vec<String> = vec![
            String::new(),
            String::new(),
            "FIXED PROLOG".into(),
            String::new(),
            format!(
                "\tNumber of Areas: {}, VBN of First Descriptor: {}",
                first[PLG_AMAX], first[PLG_AVBN]
            ),
            format!("\tProlog Version: {}", u16_at(&first, PLG_VER)),
        ];
        for (i, a) in p.areas.iter().enumerate() {
            o.extend([
                String::new(),
                format!(
                    "AREA DESCRIPTOR #{i} (VBN {}, offset %X'{:04X}')",
                    a.at.0, a.at.1
                ),
                String::new(),
                format!("\tBucket Size: {}", a.bucket_size),
                format!("\tReclaimed Bucket VBN: {}", a.reclaimed),
                format!(
                    "\tCurrent Extent Start: {}, Blocks: {}, Used: {}, Next: {}",
                    a.start, a.blocks, a.used, a.next
                ),
                format!("\tDefault Extend Quantity: {}", a.extension),
                format!("\tTotal Allocation: {}", a.total),
            ]);
        }
        let mut errors = Vec::new();
        for (ki, k) in p.keys.iter().enumerate() {
            let raw = &p
                .blocks
                .iter()
                .find(|b| b.0 == k.at.0)
                .ok_or(status::BUG)?
                .1[k.at.1..k.at.1 + KEY_SIZE];
            let d = &k.desc;
            o.extend([
                String::new(),
                format!(
                    "KEY DESCRIPTOR #{ki} (VBN {}, offset %X'{:04X}')",
                    k.at.0, k.at.1
                ),
                String::new(),
            ]);
            if u32_at(raw, 0) != 0 {
                o.push(format!(
                    "\tNext Key Descriptor VBN: {}, Offset: %X'{:04X}'",
                    u32_at(raw, 0),
                    u16_at(raw, 4)
                ));
            }
            o.push(format!(
                "\tIndex Area: {}, Level 1 Index Area: {}, Data Area: {}",
                d.index_area, d.level1_index_area, d.data_area
            ));
            o.push(format!("\tRoot Level: {}", k.root_level));
            o.push(format!(
                "\tIndex Bucket Size: {}, Data Bucket Size: {}",
                k.index_bucket, k.data_bucket
            ));
            if !k.empty {
                o.push(format!("\tRoot VBN: {}", k.root));
            }
            o.push("\tKey Flags:".into());
            let flags = raw[0x10];
            let bits: &[(u8, &str)] = if ki == 0 {
                &[
                    (0, "DUPKEYS"),
                    (3, "IDX_COMPR"),
                    (4, "INITIDX"),
                    (6, "KEY_COMPR"),
                    (7, "REC_COMPR"),
                ]
            } else {
                &[
                    (0, "DUPKEYS"),
                    (1, "CHGKEYS"),
                    (2, "NULKEYS"),
                    (3, "IDX_COMPR"),
                    (4, "INITIDX"),
                    (6, "KEY_COMPR"),
                ]
            };
            for (bit, name) in bits {
                o.push(format!(
                    "\t\t({bit})  {:<17}{}",
                    format!("KEY$V_{name}"),
                    flags >> bit & 1
                ));
            }
            o.push(format!("\tKey Segments: {}", d.segments.len()));
            if d.null_key {
                o.push(format!("\tNull Character: %X'{:02X}'", d.null_value));
            }
            o.push(format!("\tKey Size: {}", k.len()));
            o.push(format!("\tMinimum Record Size: {}", k.min_size));
            o.push(format!(
                "\tIndex Fill Quantity: {}, Data Fill Quantity: {}",
                d.index_fill, d.data_fill
            ));
            let row = |name: &str, first: usize, v: Vec<u16>| {
                let mut s = format!("\t{name}:");
                for (i, x) in v.iter().enumerate() {
                    let w = if i == 0 { first } else { 6 };
                    s.push_str(&format!("{x:>w$}"));
                }
                s
            };
            o.push(row(
                "Segment Positions",
                8,
                d.segments.iter().map(|s| s.position).collect(),
            ));
            o.push(row(
                "Segment Sizes",
                12,
                d.segments.iter().map(|s| s.length).collect(),
            ));
            o.push(format!("\tData Type: {}", type_name(d.typ, d.descending)));
            o.push(format!("\tName: \"{}\"", d.name));
            if !k.empty {
                o.push(format!("\tFirst Data Bucket VBN: {}", k.first));
            }
            errors.extend(self.audit(ki)?.0);
        }
        let n = errors.len();
        o.extend(errors);
        Ok((o, n))
    }
}

/// `report` (its first page's heading already there: a form feed, the
/// title, the file spec and two blank lines) broken into pages as
/// ANALYZE/RMS_FILE/CHECK breaks it: 54 lines under each heading, each
/// next page's heading saying `now` and its number. An area descriptor's
/// seven lines stay on one page (fixtures/fdlutil, IM.DAT); a key's don't
/// (fixtures/idx, E4).
pub fn paginate(report: &str, now: &str, spec: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let (mut page, mut room) = (1, 59);
    for line in report.split('\n') {
        if room == 0 || line.starts_with("AREA DESCRIPTOR") && room < 7 {
            page += 1;
            out.push("\x0c".to_string());
            out.push(format!(
                "{:<45}{now}   Page {page}",
                "Check RMS File Integrity"
            ));
            out.push(spec.to_string());
            out.push(String::new());
            out.push(String::new());
            room = 54;
        }
        out.push(line.to_string());
        room -= 1;
    }
    out.join("\n")
}

#[cfg(test)]
impl<B: Blocks> File<B> {
    /// Records whose stored rest differs from what `pack` makes of them.
    /// VBN, record, its stored rest, and ours.
    #[allow(clippy::type_complexity)]
    pub(crate) fn repack_mismatches(&mut self) -> Vec<(u32, Vec<u8>, Vec<u8>, Vec<u8>)> {
        let p = self.prologue().unwrap();
        let k = p.keys[0].clone();
        let mut out = Vec::new();
        let mut vbn = k.first;
        loop {
            let b = self.read(vbn, k.data_bucket).unwrap();
            for r in self
                .recs(&k, &b)
                .unwrap()
                .0
                .into_iter()
                .filter(|r| r.live())
            {
                let rec = self.record(&k, &r).unwrap();
                let mine = self.pack(&k, &rec);
                if mine.tail != r.tail {
                    out.push((vbn, rec, r.tail.clone(), mine.tail));
                }
            }
            if b.last() {
                return out;
            }
            vbn = b.next();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_records_as_vms_did() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
        for (path, fixed, mrs) in [
            ("idx/recorded/KS.DAT", false, 80),
            ("rms/recorded/IDX.DAT", true, 64),
            ("idx/recorded/NC.DAT", false, 60),
            ("idxw/recorded/MC.DAT", true, 60),
            ("idxv/recorded/V2.DAT", false, 80),
            ("idxw/recorded/CR.DAT", false, 100),
        ] {
            let mut f = File::new(std::fs::read(root.join(path)).unwrap(), fixed, mrs);
            let bad = f.repack_mismatches();
            assert!(bad.is_empty(), "{path}: {:?}", &bad[..bad.len().min(3)]);
        }
    }
}
