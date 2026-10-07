//! ANALYZE/RMS_FILE /FDL and /CHECK of sequential and relative files, made
//! on OpenVMS or by vms-rms (fixtures/rmsback, damaged ones too), give what
//! VMS printed. What comes from the file header (names, file IDs, dates,
//! allocation, length hints) is taken from VMS's report.

use vms_rms::analyze::{Header, check, fdl};
use vms_rms::{Fab, Org, Rfm};

fn fixture(path: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(path)
}

/// A number field of `name`'s entry in an ods-manifest.json.
fn field(manifest: &str, name: &str, key: &str) -> u64 {
    let entry = &manifest[manifest
        .find(&format!("\"name\": \"{name}\""))
        .unwrap_or_else(|| panic!("{name}"))..];
    let v = &entry[entry.find(&format!("\"{key}\": ")).unwrap() + key.len() + 4..];
    v[..v.find([',', '\n']).unwrap()].trim().parse().unwrap()
}

/// What follows `label` on a line of `text`.
fn after<'a>(text: &'a str, label: &str) -> Option<&'a str> {
    text.lines()
        .find_map(|l| l.trim_start().strip_prefix(label))
}

/// Analyzes `dat` (a path in fixtures/AREA) as `name`.DAT, as VMS did into
/// `report`.CHK and .ANL; returns the number of errors.
fn analyzed(area: &str, name: &str, dat: &str, report: &str) -> usize {
    let m =
        std::fs::read_to_string(fixture(&format!("{area}/recorded/ods-manifest.json"))).unwrap();
    let f = |k| field(&m, &format!("{name}.DAT"), k);
    let rtype = f("rtype") as usize;
    let fab = Fab {
        org: [Org::Seq, Org::Rel, Org::Idx][rtype >> 4],
        rfm: [
            Rfm::Udf,
            Rfm::Fix,
            Rfm::Var,
            Rfm::Vfc,
            Rfm::Stm,
            Rfm::Stmlf,
            Rfm::Stmcr,
        ][rtype & 15],
        rat: f("rattrib") as u8,
        mrs: f("maxrec") as u16,
        lrl: f("rsize") as u16,
        fsz: f("vfcsize") as u8,
        bks: f("bktsize") as u8,
        deq: f("defext") as u16,
    };
    let read = |ext| std::fs::read_to_string(fixture(&format!("{area}/recorded/{report}.{ext}")));
    let chk = read("CHK").unwrap();
    let (revised, revision) = after(&chk, "Revision Date:   ")
        .unwrap()
        .split_once(", Number: ")
        .unwrap();
    let fid = after(&chk, "File ID: (").unwrap().trim_end_matches(')');
    let fid: Vec<u32> = fid.split(',').map(|n| n.parse().unwrap()).collect();
    let hint = |l| after(&chk, l).and_then(|n| n.trim().parse().ok());
    let allocated = after(&chk, "Blocks Allocated: ").unwrap();
    let h = Header {
        spec: after(&chk, "File Spec: ").unwrap().to_string(),
        fid: [fid[0], fid[1], fid[2]],
        owner: f("owner") as u32,
        protection: f("protection") as u16,
        created: after(&chk, "Creation Date:   ").unwrap().to_string(),
        revised: revised.to_string(),
        revision: revision.parse().unwrap(),
        expires: None,
        backup: None,
        allocated: allocated[..allocated.find(',').unwrap()].parse().unwrap(),
        cluster: 16,
        eof: f("bytes"),
        contiguous: f("filechar") & 0x80 != 0,
        best_try_contiguous: f("filechar") & 0x20 != 0,
        length_hint: hint("File Length Hint (Record Count):")
            .zip(hint("File Length Hint (Data Byte Count):")),
    };
    let mut b = std::fs::read(fixture(&format!("{area}/{dat}"))).unwrap();
    // The sequential ones were kept up to their end of file.
    b.resize(b.len().next_multiple_of(512), 0);
    let now = &chk.lines().nth(1).unwrap()[45..68];
    let command = chk.lines().last().unwrap();
    let (got, errors) = check(&fab, &h, &mut b, now, command).unwrap();
    assert_eq!(got, chk, "{report}.CHK");
    if let Ok(anl) = read("ANL") {
        let ident = after(&anl, "IDENT\tFDL_VERSION 02 \"").unwrap();
        let got = fdl(&fab, &h, &mut b, &ident[..20]).unwrap();
        assert_eq!(got.to_string(), anl, "{report}.ANL");
    }
    errors
}

fn made_on_vms(area: &str, name: &str) {
    assert_eq!(
        analyzed(area, name, &format!("recorded/{name}.DAT"), name),
        0
    );
}

#[test]
fn sequential() {
    for name in [
        "SEQVAR", "SEQVFC", "SEQSTM", "SEQLFSTM", "SEQCRSTM", "SEQFIX",
    ] {
        made_on_vms("rms", name);
    }
}

#[test]
fn relative() {
    made_on_vms("rms", "REL");
    for name in ["RFIX", "RVFC", "RVAR", "REMPTY"] {
        made_on_vms("rmsrel", name);
    }
}

#[test]
fn made_by_vms_rms() {
    for (name, errors) in [
        ("RGAPS", 0),
        ("RFIXG", 0),
        ("RVFCG", 0),
        ("SFTN", 0),
        ("SCRBLK", 0),
        ("RBADLEN", 1),
        ("RBADCTL", 3),
        // ANALYZE doesn't read a sequential file's records.
        ("SBAD", 0),
    ] {
        let got = analyzed("rmsback", name, &format!("ours/{name}.DAT"), name);
        assert_eq!(got, errors, "{name}");
    }
    // After DCL changed it.
    assert_eq!(
        analyzed("rmsback", "RGAPS", "recorded/RGAPS.DAT", "RGAPSV"),
        0
    );
}
