//! Relative files made on OpenVMS (fixtures/rms REL.DAT, fixtures/rmsrel):
//! vms-rms reads them as DCL READ did, and doing what the procedure did
//! makes the same bytes and attributes.

use vms_cond::Cond;
use vms_rms::rel::{Rel, create};
use vms_rms::{Area, BLOCK, Blocks, Design, Fab, Org, Record, Rfm, rat, status};

fn fixture(path: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(path)
}

/// A number field of `name`'s entry in an ods-manifest.json.
fn field(manifest: &str, name: &str, key: &str) -> usize {
    let entry = &manifest[manifest
        .find(&format!("\"name\": \"{name}\""))
        .unwrap_or_else(|| panic!("{name}"))..];
    let v = &entry[entry.find(&format!("\"{key}\": ")).unwrap() + key.len() + 4..];
    v[..v.find([',', '\n']).unwrap()].trim().parse().unwrap()
}

/// The file's attributes, as its header has them, and its blocks.
fn file(area: &str, name: &str) -> (Fab, Vec<u8>) {
    let m =
        std::fs::read_to_string(fixture(&format!("{area}/recorded/ods-manifest.json"))).unwrap();
    let f = |k| field(&m, name, k);
    let rtype = f("rtype");
    let fab = Fab {
        org: [Org::Seq, Org::Rel, Org::Idx][rtype >> 4],
        rfm: [Rfm::Udf, Rfm::Fix, Rfm::Var, Rfm::Vfc][rtype & 15],
        rat: f("rattrib") as u8,
        mrs: f("maxrec") as u16,
        lrl: f("rsize") as u16,
        fsz: f("vfcsize") as u8,
        bks: f("bktsize") as u8,
        deq: f("defext") as u16,
    };
    let bytes = std::fs::read(fixture(&format!("{area}/recorded/{name}"))).unwrap();
    (fab, bytes)
}

/// Blocks allocated in clusters of 16, as on the volume the files were
/// made on.
struct Clustered(Vec<u8>);

impl Blocks for Clustered {
    fn read(&mut self, vbn: u32, buf: &mut [u8]) -> Result<(), Cond> {
        self.0.read(vbn, buf)
    }
    fn write(&mut self, vbn: u32, buf: &[u8]) -> Result<(), Cond> {
        self.0.write(vbn, buf)
    }
    fn allocated(&self) -> u32 {
        self.0.allocated()
    }
    fn grow(&mut self, n: u32) -> Result<u32, Cond> {
        self.0.grow(n.next_multiple_of(16))
    }
}

/// The lines DCL READ showed between `@@ label` and the next label.
fn dcl_read(label: &str) -> Vec<String> {
    let log = std::fs::read_to_string(fixture("rmsrel/recorded/rmsrel.log")).unwrap();
    let block = log.split("@@ ").find(|b| b.starts_with(label)).unwrap();
    block
        .lines()
        .filter_map(|l| l.strip_prefix('[')?.strip_suffix(']'))
        .map(String::from)
        .collect()
}

fn records(fab: Fab, mut b: Vec<u8>) -> Vec<String> {
    let rel = Rel::new(fab).unwrap();
    let mut out = Vec::new();
    let mut n = 0;
    loop {
        match rel.next(&mut b, n) {
            Ok((m, r)) => {
                out.push(String::from_utf8(r.data).unwrap());
                n = m;
            }
            Err(e) => {
                assert_eq!(e, status::EOF);
                return out;
            }
        }
    }
}

fn design(rfm: Rfm, rat: u8, mrs: u16) -> Design {
    Design {
        fab: Fab {
            rfm,
            rat,
            mrs,
            ..Fab::default()
        },
        ..Design::default()
    }
}

/// Makes `name` as rms.com or rmsrel.com did, checks it is the file VMS
/// made, and that it reads as DCL READ read it.
fn same(area: &str, name: &str, d: &Design, ops: impl Fn(&Rel, &mut Clustered)) {
    let (fab, bytes) = file(area, name);
    let mut b = Clustered(Vec::new());
    let rel = create(&mut b, d).unwrap();
    ops(&rel, &mut b);
    assert_eq!(rel.fab, fab, "{name}");
    assert!(b.0 == bytes, "{name} differs from VMS's");
}

#[test]
fn rel_dat() {
    let d = design(Rfm::Var, rat::CR, 40);
    same("rms", "REL.DAT", &d, |rel, b| {
        for n in 1..=20 {
            let r = format!("relative record {n}{}", ".".repeat(n));
            assert_eq!(rel.append(b, &Record::new(r)), Ok(n as u32));
        }
    });
    let (fab, bytes) = file("rms", "REL.DAT");
    let got = records(fab, bytes);
    assert_eq!(got.len(), 20);
    assert_eq!(got[19], format!("relative record 20{}", ".".repeat(20)));
}

#[test]
fn deleted_updated_and_mrn() {
    let mut d = design(Rfm::Fix, rat::CR, 10);
    d.fab.bks = 2;
    d.max_record_number = 30;
    same("rmsrel", "RFIX.DAT", &d, |rel, b| {
        let rec = |n| Record::new(format!("fixrec {n:03}"));
        for n in 1..=25 {
            rel.append(b, &rec(n)).unwrap();
        }
        for n in [3, 4, 10] {
            rel.delete(b, n).unwrap();
        }
        rel.update(b, 5, &Record::new("updated 05")).unwrap();
        for n in 26..=30 {
            assert_eq!(rel.append(b, &rec(n)), Ok(n));
        }
        assert_eq!(rel.append(b, &rec(31)), Err(status::MRN));
    });
    let (fab, bytes) = file("rmsrel", "RFIX.DAT");
    assert_eq!(records(fab, bytes), dcl_read("rfix"));
}

#[test]
fn vfc_and_shorter_update() {
    let mut d = design(Rfm::Vfc, rat::PRN, 30);
    d.fab.fsz = 2;
    d.fab.bks = 3;
    same("rmsrel", "RVFC.DAT", &d, |rel, b| {
        let fill = "abcdefghijklmnopqrstuvwxyz0123456789";
        let rec = |s: &str| Record {
            control: vec![0, 0],
            data: s.into(),
        };
        for n in 1..=12 {
            let k = n * 7 % 29;
            rel.append(b, &rec(&format!("{n:02}{}", &fill[..k])))
                .unwrap();
        }
        rel.delete(b, 4).unwrap();
        rel.delete(b, 12).unwrap();
        rel.update(b, 6, &rec("short")).unwrap();
        // After the deleted last record, not over it.
        assert_eq!(rel.append(b, &rec("appended")), Ok(13));
    });
    let (fab, bytes) = file("rmsrel", "RVFC.DAT");
    assert_eq!(records(fab, bytes), dcl_read("rvfc"));
}

#[test]
fn extends() {
    let mut d = design(Rfm::Var, rat::CR, 600);
    d.fab.deq = 5;
    same("rmsrel", "RVAR.DAT", &d, |rel, b| {
        // DCL made 8: longer ones overflowed its command line.
        for n in 1..=8 {
            let r = format!("{n:02}{}", "v".repeat(n * 29));
            rel.append(b, &Record::new(r)).unwrap();
        }
        assert_eq!(b.0.len(), 32 * BLOCK);
    });
    let (fab, bytes) = file("rmsrel", "RVAR.DAT");
    assert_eq!(records(fab, bytes), dcl_read("rvar"));
}

#[test]
fn empty() {
    let mut d = design(Rfm::Fix, 0, 100);
    d.fab.bks = 4;
    d.areas = vec![Area {
        allocation: 40,
        best_try_contiguous: true,
        ..Area::default()
    }];
    same("rmsrel", "REMPTY.DAT", &d, |_, _| {});
    let (fab, bytes) = file("rmsrel", "REMPTY.DAT");
    assert_eq!(records(fab, bytes), dcl_read("rempty"));
}
