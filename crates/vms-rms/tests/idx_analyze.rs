//! ANALYZE/RMS_FILE's indexed parts as VMS printed them: /CHECK's
//! prologue, area and key descriptors and its pages, /FDL's ANALYSIS_OF
//! sections, for files VMS made and files made here (fixtures/idxrt).

mod common;

use common::*;
use vms_rms::idx::File;
use vms_rms::idx::analyze::paginate;

/// Every indexed file with ANALYZE reports: area, name, fixed, mrs.
fn files() -> Vec<(&'static str, String, bool, u16)> {
    let mut out = Vec::new();
    for (area, names) in [
        ("rms", &["IDX", "IDXVAR"][..]),
        (
            "idx",
            &[
                "E1", "E2", "E3", "E4", "E5", "E6", "E7", "P1", "P2", "RB", "AS", "DS", "DEL",
                "PD", "KS", "NC", "TY", "ML",
            ][..],
        ),
        (
            "idxw",
            &[
                "SA0", "SA1", "SA2", "SA3", "SA4", "SA5", "SA6", "SA7", "SD0", "SD1", "SD2", "SD3",
                "SD4", "SD5", "SD6", "SD7", "SV1", "SV2", "CR", "NK", "FK", "SC", "DM1", "DM2",
                "DM3", "DM4", "DM5", "UP", "MC",
            ][..],
        ),
        ("idxv", &["V1", "V2", "V5"][..]),
    ] {
        for n in names {
            let f = fab(area, &format!("{n}.DAT"));
            out.push((area, n.to_string(), f.rfm == vms_rms::Rfm::Fix, f.mrs));
        }
    }
    for (n, fixed, mrs) in [
        ("RT1", true, 64),
        ("RT2", false, 80),
        ("RT3", true, 40),
        ("RT4", true, 200),
        ("RT5", false, 40),
        ("RT6", false, 80),
        ("RT7", false, 200),
        ("RT1V", true, 64),
    ] {
        out.push(("idxrt", n.to_string(), fixed, mrs));
    }
    out
}

fn open(area: &str, name: &str, fixed: bool, mrs: u16) -> File<Vec<u8>> {
    let path = match area {
        "idxrt" if name != "RT1V" => format!("idxrt/{name}.DAT"),
        _ => format!("{area}/recorded/{name}.DAT"),
    };
    File::new(std::fs::read(fixture(&path)).unwrap(), fixed, mrs)
}

fn report(area: &str, name: &str, ext: &str) -> String {
    std::fs::read_to_string(fixture(&format!("{area}/recorded/{name}.{ext}"))).unwrap()
}

/// A /CHECK report's lines without the headings of its pages.
fn flat(chk: &str) -> Vec<&str> {
    let lines: Vec<&str> = chk.lines().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if lines[i] == "\x0c" {
            i += 5;
            continue;
        }
        out.push(lines[i]);
        i += 1;
    }
    out
}

#[test]
fn check_reports_as_vms_printed_them() {
    for (area, name, fixed, mrs) in files() {
        let chk = report(area, &name, "CHK");
        let flat = flat(&chk);
        let start = flat.iter().position(|l| *l == "FIXED PROLOG").unwrap() - 2;
        let end = flat
            .iter()
            .position(|l| l.starts_with("The analysis uncovered"))
            .unwrap()
            - 2;
        let (got, errors) = open(area, &name, fixed, mrs).check_report().unwrap();
        assert_eq!(errors, 0, "{area}/{name}: {got:?}");
        assert_eq!(got, flat[start..end], "{area}/{name}");
    }
}

#[test]
fn check_reports_break_into_pages_as_vms_did() {
    for (area, name, ..) in files() {
        let chk = report(area, &name, "CHK");
        let lines: Vec<&str> = chk.lines().collect();
        let title = lines[1];
        let now = &title[45..title.find("   Page").unwrap()];
        let spec = lines[2];
        // The first page as it is, the others' headings made again; VMS
        // stamps each page with the time it printed it.
        let first = 59.min(lines.len());
        let rest = lines[first..].join("\n");
        let mut body: Vec<&str> = lines[..first].to_vec();
        body.extend(flat(&rest));
        let want: Vec<String> = lines
            .iter()
            .map(|l| match l.find("   Page ") {
                Some(at) if l.starts_with("Check RMS File Integrity") => {
                    format!("{}{now}{}", &l[..45], &l[at..])
                }
                _ => l.to_string(),
            })
            .collect();
        assert_eq!(
            paginate(&body.join("\n"), now, spec),
            want.join("\n"),
            "{area}/{name}"
        );
    }
}

#[test]
fn fdl_analysis_as_vms_printed_it() {
    for (area, name, fixed, mrs) in files() {
        if name == "RT1V" {
            continue; // VMS checked it only
        }
        let anl = report(area, &name, "ANL");
        let want = &anl[anl.find("ANALYSIS_OF_AREA").unwrap()..];
        let got = open(area, &name, fixed, mrs).analysis().unwrap();
        assert_eq!(got, want, "{area}/{name}");
    }
}
