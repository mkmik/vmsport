//! FDL against what OpenVMS read and wrote: every FDL in fixtures parses,
//! ANALYZE/RMS_FILE/FDL's output comes back out the same, and what CREATE/FDL
//! made of an input (as ANALYZE describes it) is what vms-rms makes of it.

use vms_rms::fdl::{Fdl, Section, from_design, parse, to_design};
use vms_rms::{KeyType, Org, Rfm, rat};

fn fixtures() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

/// The fixture files under fixtures/AREA/ and fixtures/AREA/recorded/ ending
/// in `ext`.
fn all(ext: &str) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for area in std::fs::read_dir(fixtures()).unwrap() {
        let area = area.unwrap().path();
        for dir in [area.clone(), area.join("recorded")] {
            for f in std::fs::read_dir(&dir).into_iter().flatten() {
                let f = f.unwrap().path();
                if f.to_string_lossy().ends_with(ext) {
                    out.push(f);
                }
            }
        }
    }
    out.sort();
    out
}

fn read(p: &std::path::Path) -> Fdl {
    parse(&std::fs::read_to_string(p).unwrap()).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// FILE attributes that come from the file header, not the design.
const HOST: [&str; 8] = [
    "CLUSTER_SIZE",
    "FILE_MONITORING",
    "NAME",
    "OWNER",
    "PROTECTION",
    "GLOBAL_BUFFER_COUNT",
    "GLBUFF_CNT_V83",
    "GLBUFF_FLAGS_V83",
];

#[test]
fn analyze_output_round_trips() {
    let anl = all(".ANL");
    assert!(anl.len() >= 13);
    for p in anl {
        let text = std::fs::read_to_string(&p).unwrap();
        let fdl = read(&p);
        assert_eq!(fdl.to_string(), text, "{}", p.display());
        // The design sections, through a Design and back.
        let mut want: Vec<Section> = fdl
            .sections
            .iter()
            .filter(|s| ["FILE", "RECORD", "AREA", "KEY"].contains(&s.name.as_str()))
            .cloned()
            .collect();
        want[0].attrs.retain(|(k, _)| !HOST.contains(&k.as_str()));
        let mut got = from_design(&to_design(&fdl).unwrap()).sections;
        // An indexed file's allocation is its header's, in whole clusters,
        // not its areas'.
        if want[0].get("ORGANIZATION") == Some("indexed") {
            for s in [&mut want[0], &mut got[0]] {
                s.attrs.retain(|(k, _)| k != "ALLOCATION");
            }
        }
        assert_eq!(got, want, "{}", p.display());
    }
}

/// CREATE/FDL's defaults: the RECORD and KEY sections ANALYZE gave for the
/// files made from an input are what vms-rms makes of the input.
#[test]
fn inputs_as_vms_made_them() {
    let inputs = all(".FDL");
    assert!(inputs.len() >= 19);
    for p in inputs {
        let d = to_design(&read(&p)).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
        let anl = p
            .with_file_name("recorded")
            .join(p.file_name().unwrap())
            .with_extension("ANL");
        if !anl.exists() {
            continue;
        }
        let made = from_design(&d);
        let vms = read(&anl);
        for s in made
            .sections
            .iter()
            .filter(|s| s.name != "FILE" && s.name != "AREA")
        {
            assert_eq!(Some(s), vms.section(&s.name, &s.value), "{}", p.display());
        }
    }
}

#[test]
fn designs() {
    let d = to_design(&read(&fixtures().join("rms/IDX.FDL"))).unwrap();
    assert_eq!(
        (d.fab.org, d.fab.rfm, d.fab.rat, d.fab.mrs),
        (Org::Idx, Rfm::Fix, rat::CR, 64)
    );
    assert_eq!((d.prologue, d.areas.len(), d.keys.len()), (3, 1, 4));
    assert_eq!((d.areas[0].allocation, d.areas[0].extension), (30, 12));
    let split = &d.keys[2];
    assert_eq!(split.name, "SPLIT");
    assert_eq!(split.length(), 8);
    assert!(split.duplicates && split.changes);
    assert_eq!(split.data_fill, 512);
    assert_eq!(d.keys[3].typ, KeyType::Bin4);
    assert!(!d.keys[3].data_key_compression);

    let fdl = parse(
        "! comment\nfile ! here too\n  organization relative\n  max_record_number 7\n\
         record\n  format vfc\n  size 30\n  block_span no\n\
         key 0\n  type dint4\n  name \"A \"\"B\"\"!\"\n",
    )
    .unwrap();
    let d = to_design(&fdl).unwrap();
    assert_eq!((d.fab.org, d.fab.rfm, d.fab.fsz), (Org::Rel, Rfm::Vfc, 2));
    assert_eq!(d.fab.rat, rat::CR | rat::BLK);
    assert_eq!((d.max_record_number, d.prologue), (7, 1));
    assert_eq!((d.keys[0].typ, d.keys[0].descending), (KeyType::Int4, true));
    assert_eq!(d.keys[0].name, "A \"B\"!");
    assert!(parse("SIZE 3\n").is_err());
    assert!(to_design(&parse("FILE\n ORGANIZATION heap\n").unwrap()).is_err());
}
