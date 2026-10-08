//! Files vms-rms makes, back on OpenVMS (fixtures/rmsback): the test makes
//! them again and checks they are the bytes VMS was given in ours/, that
//! ANALYZE/RMS_FILE/CHECK found nothing wrong in them, that DCL READ read
//! what vms-rms reads, and that what VMS then did to one is what vms-rms
//! does. VMSPORT_WRITE_FIXTURES=1 writes ours/ instead, to record again.

use vms_cond::Cond;
use vms_rms::rel::{Rel, create};
use vms_rms::{Design, Fab, Org, Record, Rfm, encode, rat, status};

fn fixture(path: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/rmsback")
        .join(path)
}

fn rel(
    fab: Fab,
    mrn: u32,
    allocation: u32,
    ops: impl Fn(&Rel, &mut Vec<u8>) -> Result<(), Cond>,
) -> (Fab, Vec<u8>) {
    let mut b = Vec::new();
    let d = Design {
        fab: Fab {
            org: Org::Rel,
            ..fab
        },
        max_record_number: mrn,
        areas: vec![vms_rms::Area {
            allocation,
            ..Default::default()
        }],
        ..Design::default()
    };
    let r = create(&mut b, &d).unwrap();
    ops(&r, &mut b).unwrap();
    (r.fab, b)
}

fn seq(fab: Fab, recs: &[Record]) -> (Fab, Vec<u8>) {
    let lrl = recs.iter().map(|r| r.data.len()).max().unwrap_or(0) as u16;
    (Fab { lrl, ..fab }, encode(&fab, recs).unwrap())
}

fn var(rat: u8, mrs: u16) -> Fab {
    Fab {
        rfm: Rfm::Var,
        rat,
        mrs,
        ..Fab::default()
    }
}

/// RGAPS as given to VMS: records put by number with gaps, one deleted,
/// one made shorter, past the first allocation.
fn gaps(r: &Rel, b: &mut Vec<u8>) -> Result<(), Cond> {
    for (n, s) in [
        (1, "one"),
        (2, "two"),
        (5, "five, longer at first"),
        (13, "thirteen"),
        (40, "forty"),
    ] {
        r.put(b, n, &Record::new(s))?;
    }
    r.delete(b, 2)?;
    r.update(b, 5, &Record::new("five"))
}

/// The files: name, attributes, bytes.
fn ours() -> Vec<(&'static str, Fab, Vec<u8>)> {
    let r = Record::new;
    let mut v = Vec::new();
    let (f, b) = rel(
        Fab {
            deq: 4,
            ..var(rat::CR, 50)
        },
        0,
        3,
        gaps,
    );
    v.push(("RGAPS.DAT", f, b));
    let fix = Fab {
        rfm: Rfm::Fix,
        mrs: 7,
        bks: 2,
        ..Fab::default()
    };
    let (f, b) = rel(fix, 100, 0, |rel, b| {
        rel.put(b, 100, &r("hundred"))?;
        rel.put(b, 1, &r("one...."))?;
        rel.put(b, 50, &r("fifty.."))?;
        rel.delete(b, 50)?;
        assert_eq!(rel.put(b, 101, &r("too far")), Err(status::MRN));
        Ok(())
    });
    v.push(("RFIXG.DAT", f, b));
    let vfc = Fab {
        rfm: Rfm::Vfc,
        rat: rat::PRN,
        fsz: 2,
        mrs: 20,
        ..Fab::default()
    };
    let (f, b) = rel(vfc, 0, 0, |rel, b| {
        for (n, s) in [(3, "three"), (4, ""), (20, "twenty, all of it..")] {
            let rec = Record {
                control: vec![1, 0x8d],
                data: s.into(),
            };
            rel.put(b, n, &rec)?;
        }
        Ok(())
    });
    v.push(("RVFCG.DAT", f, b));
    let mut recs: Vec<Record> = [" first", "0second", "+third"].map(r).into();
    recs.extend((b'a'..b'd').map(|c| Record::new(vec![c; 200])));
    let (f, b) = seq(var(rat::FTN | rat::BLK, 0), &recs);
    v.push(("SFTN.DAT", f, b));
    let fix = Fab {
        rfm: Rfm::Fix,
        rat: rat::CR | rat::BLK,
        mrs: 100,
        ..Fab::default()
    };
    let recs: Vec<Record> = (b'0'..b'6').map(|c| Record::new(vec![c; 100])).collect();
    let (f, b) = seq(fix, &recs);
    v.push(("SCRBLK.DAT", f, b));
    // Damaged, to see what ANALYZE says: a record longer than the cells,
    // a control byte RMS doesn't use, a VAR length past the end of file.
    let (f, mut b) = rel(var(rat::CR, 10), 0, 0, |rel, b| {
        rel.put(b, 1, &r("one"))?;
        rel.put(b, 2, &r("two"))
    });
    b[512 + 13 + 1] = 0x20;
    v.push(("RBADLEN.DAT", f, b));
    let (f, mut b) = rel(var(rat::CR, 10), 0, 0, |rel, b| {
        rel.put(b, 1, &r("one"))?;
        rel.put(b, 2, &r("two"))
    });
    b[512 + 13] = 0x55;
    v.push(("RBADCTL.DAT", f, b));
    let (f, mut b) = seq(var(rat::CR, 0), &[r("one"), r("two")]);
    b[6] = 0x40;
    v.push(("SBAD.DAT", f, b));
    v
}

/// ods-manifest.json for `ods import`.
fn manifest(files: &[(&str, Fab, Vec<u8>)]) -> String {
    // 7-OCT-2026 00:28:23.27, when fixtures/rms was recorded.
    let t = 52980497032782765u64;
    let entries: Vec<String> = files
        .iter()
        .map(|(name, f, b)| {
            format!(
                "  {{\"path\": \"{name}\", \"directory\": false, \"name\": \"{name}\", \"version\": 1, \
                 \"bytes\": {}, \"rtype\": {}, \"rattrib\": {}, \"rsize\": {}, \"bktsize\": {}, \
                 \"vfcsize\": {}, \"maxrec\": {}, \"defext\": {}, \"gbc\": 0, \
                 \"reserved\": [0, 0, 0, 0, 0, 0, 0, 0], \"version_limit\": 0, \"filechar\": 0, \
                 \"owner\": 65540, \"protection\": 64000, \"revision\": 1, \"created\": {t}, \
                 \"revised\": {t}, \"expires\": 0, \"backup\": 0, \"accessed\": {t}, \"attr_changed\": {t}}}",
                b.len(),
                (f.org as u8) << 4 | f.rfm as u8,
                f.rat,
                f.lrl,
                f.bks,
                f.fsz,
                f.mrs,
                f.deq
            )
        })
        .collect();
    format!(
        "{{\"volume\": \"VPTIN\", \"structure_level\": 5, \"root\": \"[T]\", \"entries\": [\n{}\n]}}\n",
        entries.join(",\n")
    )
}

#[test]
fn ours_are_what_vms_checked() {
    let files = ours();
    let m = manifest(&files);
    if std::env::var_os("VMSPORT_WRITE_FIXTURES").is_some() {
        std::fs::create_dir_all(fixture("ours")).unwrap();
        for (name, _, b) in &files {
            std::fs::write(fixture(&format!("ours/{name}")), b).unwrap();
        }
        std::fs::write(fixture("ours/ods-manifest.json"), m).unwrap();
        return;
    }
    assert_eq!(
        std::fs::read_to_string(fixture("ours/ods-manifest.json")).unwrap(),
        m
    );
    for (name, _, b) in &files {
        assert!(
            std::fs::read(fixture(&format!("ours/{name}"))).unwrap() == *b,
            "{name}"
        );
    }
}

/// The lines DCL READ showed after `@@ label`.
fn dcl_read(label: &str) -> Vec<String> {
    let log = std::fs::read_to_string(fixture("recorded/rmsback.log")).unwrap();
    let block = log.split("@@ ").find(|b| b.starts_with(label)).unwrap();
    block
        .lines()
        .filter_map(|l| l.strip_prefix('[')?.strip_suffix(']'))
        .map(String::from)
        .collect()
}

fn read_rel(fab: Fab, b: &mut Vec<u8>) -> Vec<String> {
    let rel = Rel::new(fab).unwrap();
    let (mut n, mut out) = (0, Vec::new());
    while let Ok((m, r)) = rel.next(b, n) {
        out.push(String::from_utf8(r.data).unwrap());
        n = m;
    }
    out
}

#[test]
fn vms_read_what_we_read() {
    for (name, fab, mut b) in ours().into_iter().filter(|f| !f.0.contains("BAD")) {
        let got = match fab.org {
            Org::Rel => read_rel(fab, &mut b),
            _ => vms_rms::decode(&fab, &b)
                .unwrap()
                .into_iter()
                .map(|r| String::from_utf8(r.data).unwrap())
                .collect(),
        };
        let label = format!("read {}", name.trim_end_matches(".DAT"));
        assert_eq!(got, dcl_read(&label), "{name}");
    }
    // READ/KEY="A   ": record number %X20202041, past RFIXG's MRN and
    // RGAPS's end.
    let mut v = ours();
    let (_, gaps, mut g) = v.remove(0);
    let (_, fixg, mut f) = v.remove(0);
    assert_eq!(
        Rel::new(fixg).unwrap().get(&mut f, 0x2020_2041),
        Err(status::MRN)
    );
    assert_eq!(
        Rel::new(gaps).unwrap().get(&mut g, 0x2020_2041),
        Err(status::RNF)
    );
}

/// What DCL did to RGAPS.DAT: read 1, read and delete 5, read 13 and
/// update it, close; open to append, write.
#[test]
fn vms_changes_are_ours() {
    let (_, fab, mut b) = ours().swap_remove(0);
    let rel = Rel::new(fab).unwrap();
    rel.delete(&mut b, 5).unwrap();
    rel.update(&mut b, 13, &Record::new("13 by VMS")).unwrap();
    assert_eq!(rel.append(&mut b, &Record::new("appended on VMS")), Ok(41));
    let vms = std::fs::read(fixture("recorded/RGAPS.DAT")).unwrap();
    // VMS's copy runs on to the highwater mark of its 16 blocks.
    assert!(vms[..b.len()] == b && vms[b.len()..].iter().all(|c| *c == 0));
    assert_eq!(read_rel(fab, &mut b), dcl_read("change"));
}
