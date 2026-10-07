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
