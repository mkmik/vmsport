//! ANALYZE/RMS_FILE's /FDL and /CHECK reports for sequential and relative
//! files, as OpenVMS prints them (fixtures/rms, fixtures/rmsrel,
//! fixtures/rmsback).

use crate::fdl::{Fdl, Section, from_design};
use crate::rel::{DELETED, PRESENT, Prologue};
use crate::{Area, BLOCK, Blocks, Design, Fab, Org, Rfm, rat};
use std::fmt::Write;
use vms_cond::Cond;

/// What the reports show from the file header.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Header {
    /// The full file spec: DKA200:[T]REL.DAT;1.
    pub spec: String,
    /// File ID: number, sequence, volume.
    pub fid: [u32; 3],
    pub owner: u32,
    /// VMS protection bits: 4 per class, set to deny.
    pub protection: u16,
    /// Dates as VMS shows them: " 7-OCT-2026 00:28:23.27".
    pub created: String,
    pub revised: String,
    pub revision: u16,
    pub expires: Option<String>,
    pub backup: Option<String>,
    pub allocated: u32,
    pub cluster: u32,
    /// End of file, in bytes.
    pub eof: u64,
    pub contiguous: bool,
    pub best_try_contiguous: bool,
    /// The record and data byte counts a header may keep; `None` when it
    /// has none (all ones).
    pub length_hint: Option<(u64, u64)>,
}

/// The `[g,m]` form of a UIC.
fn uic(u: u32) -> String {
    format!("[{:o},{:o}]", u >> 16, u & 0xffff)
}

/// What each class may do: ["RWED", "RWED", "RE", ""].
fn access(p: u16) -> [String; 4] {
    std::array::from_fn(|class| {
        "RWED"
            .chars()
            .enumerate()
            .filter(|(i, _)| p >> (4 * class + i) & 1 == 0)
            .map(|(_, c)| c)
            .collect()
    })
}

fn design(fab: &Fab, h: &Header, mrn: u32) -> Design {
    Design {
        fab: *fab,
        max_record_number: mrn,
        areas: vec![Area {
            allocation: h.allocated,
            contiguous: h.contiguous,
            best_try_contiguous: h.best_try_contiguous,
            ..Area::default()
        }],
        ..Design::default()
    }
}

fn prologue(fab: &Fab, b: &mut impl Blocks) -> Result<Option<Prologue>, Cond> {
    match fab.org {
        Org::Rel => Prologue::read(b).map(Some),
        _ => Ok(None),
    }
}

/// ANALYZE/RMS_FILE/FDL's output. `now` is the time as VMS shows it, as in
/// [`Header`]'s dates.
pub fn fdl(fab: &Fab, h: &Header, b: &mut impl Blocks, now: &str) -> Result<Fdl, Cond> {
    let p = prologue(fab, b)?;
    let mut f = from_design(&design(fab, h, p.map_or(0, |p| p.mrn)));
    let file = &mut f.sections[0];
    file.set("CLUSTER_SIZE", h.cluster);
    file.set("FILE_MONITORING", "no");
    file.set("NAME", format!("\"{}\"", h.spec));
    file.set("OWNER", uic(h.owner));
    let [s, o, g, w] = access(h.protection);
    file.set(
        "PROTECTION",
        format!("(system:{s}, owner:{o}, group:{g}, world:{w})"),
    );
    file.push("GLOBAL_BUFFER_COUNT", 0);
    file.push("GLBUFF_CNT_V83", 0);
    file.push("GLBUFF_FLAGS_V83", "none");
    let mut system = Section::new("SYSTEM", "");
    system.push("SOURCE", "OpenVMS");
    let ident = Section::new(
        "IDENT",
        format!(
            "FDL_VERSION 02 \"{}   OpenVMS ANALYZE/RMS_FILE Utility\"",
            &now[..now.len().min(20)]
        ),
    );
    f.sections.splice(0..0, [ident, system]);
    Ok(f)
}

/// What is wrong in a relative file's cells, as ANALYZE says it (it
/// doesn't look at a sequential file's records).
fn errors(fab: &Fab, b: &mut impl Blocks, p: &Prologue) -> Result<Vec<String>, Cond> {
    let mut out = Vec::new();
    let bks = fab.bks.max(1) as u32;
    let fsz = if fab.rfm == Rfm::Vfc { fab.fsz } else { 0 };
    let cell = crate::rel::cell_size(fab);
    let mut buf = vec![0; bks as usize * BLOCK];
    let mut vbn = p.dvbn;
    while vbn + bks <= p.eof {
        b.read(vbn, &mut buf)?;
        for c in buf.chunks_exact(cell) {
            for bit in (0..8).filter(|i| c[0] & !(PRESENT | DELETED) & 1 << i != 0) {
                out.push(format!("***  VBN {vbn}:  Reserved flag bit {bit} is set."));
            }
            if fab.rfm != Rfm::Fix
                && c[0] & PRESENT != 0
                && u16::from_le_bytes([c[1], c[2]]) > fsz as u16 + fab.mrs
            {
                out.push(format!(
                    "***  VBN {vbn}:  Record is too large to fit in record cell."
                ));
            }
        }
        vbn += bks;
    }
    Ok(out)
}

/// ANALYZE/RMS_FILE/CHECK's output, `command` last, and the number of
/// errors found.
pub fn check(
    fab: &Fab,
    h: &Header,
    b: &mut impl Blocks,
    now: &str,
    command: &str,
) -> Result<(String, usize), Cond> {
    let p = prologue(fab, b)?;
    let errors = match &p {
        Some(p) => errors(fab, b, p)?,
        None => Vec::new(),
    };
    let mut o = String::new();
    let [s, ow, g, w] = access(h.protection);
    let contiguity = match (h.contiguous, h.best_try_contiguous) {
        (true, _) => "contiguous",
        (_, true) => "contiguous-best-try",
        _ => "none",
    };
    let _ = write!(
        o,
        "\x0c\n{:<45}{now}   Page 1\n{spec}\n\n\nFILE HEADER\n\n\
         \tFile Spec: {spec}\n\
         \tFile ID: ({},{},{})\n\
         \tOwner UIC: {}\n\
         \tProtection:  System: {s}, Owner: {ow}, Group: {g}, World: {w}\n\
         \tCreation Date:   {}\n\
         \tRevision Date:   {}, Number: {}\n\
         \tExpiration Date: {}\n\
         \tBackup Date:     {}\n\
         \tContiguity Options:  {contiguity}\n\
         \tPerformance Options: none\n\
         \tReliability Options: none\n\
         \tJournaling Enabled:  none\n\n\n\
         RMS FILE ATTRIBUTES\n\n\
         \tFile Organization: {}\n\
         \tRecord Format: {}\n\
         \tRecord Attributes: {} {} \n\
         \tMaximum Record Size: {}\n",
        "Check RMS File Integrity",
        h.fid[0],
        h.fid[1],
        h.fid[2],
        uic(h.owner),
        h.created,
        h.revised,
        h.revision,
        h.expires.as_deref().unwrap_or("none specified"),
        h.backup.as_deref().unwrap_or("none posted"),
        ["sequential", "relative", "indexed"][fab.org as usize],
        [
            "undefined",
            "fixed",
            "variable",
            "variable-with-fixed-control",
            "stream",
            "stream-LF",
            "stream-CR"
        ][fab.rfm as usize],
        if fab.rat & rat::BLK != 0 {
            "no-span"
        } else {
            ""
        },
        [
            (rat::FTN, "fortran"),
            (rat::CR, "carriage-return"),
            (rat::PRN, "print")
        ]
        .iter()
        .find(|(r, _)| fab.rat & r != 0)
        .map_or("", |(_, n)| n),
        fab.mrs,
        spec = h.spec,
    );
    if fab.org == Org::Seq || fab.rfm == Rfm::Fix {
        let _ = writeln!(o, "\tLongest Record: {}", fab.lrl);
    }
    if fab.rfm == Rfm::Vfc {
        let _ = writeln!(o, "\tFixed Control Size: {}", fab.fsz);
    }
    let _ = writeln!(
        o,
        "\tBlocks Allocated: {}, Default Extend Size: {}",
        h.allocated, fab.deq
    );
    if fab.org != Org::Seq {
        let _ = writeln!(o, "\tBucket Size: {}", fab.bks);
    } else {
        let _ = writeln!(
            o,
            "\tEnd-of-File VBN: {}, Offset: %X'{:04X}'",
            h.eof / BLOCK as u64 + 1,
            h.eof % BLOCK as u64
        );
    }
    o.push_str("\tFile Monitoring: disabled\n");
    if fab.org == Org::Seq && matches!(fab.rfm, Rfm::Var | Rfm::Vfc) {
        let (records, bytes) = match h.length_hint {
            Some((r, b)) => (r.to_string(), b.to_string()),
            None => ("-1 (invalid)".into(), "-1 (invalid)".into()),
        };
        let _ = writeln!(o, "\t{:<36}{records}", "File Length Hint (Record Count):");
        let _ = writeln!(o, "\t{:<36}{bytes}", "File Length Hint (Data Byte Count):");
    }
    o.push_str(
        "\tGlobal Buffer Count  pre-V8.3:          0\n\
         \tGlobal Buffer Count post-V8.3:          0\n\
         \tGlobal Buffer Flags post-V8.3:       none\n",
    );
    if let Some(p) = p {
        let _ = write!(
            o,
            "\n\nFIXED PROLOG\n\n\
             \tProlog Flags:\n\
             \t\t(0)  PLG$V_NOEXTEND   0\n\
             \tFirst Data Bucket VBN: {}\n\
             \tMaximum Record Number: {}\n\
             \tEnd-of-File VBN: {}\n\
             \tProlog Version: 1\n",
            p.dvbn, p.mrn, p.eof
        );
    }
    for e in &errors {
        let _ = writeln!(o, "{e}");
    }
    o.push_str("\n\n");
    match errors.len() {
        0 => o.push_str("The analysis uncovered NO errors.\n"),
        1 => o.push_str("The analysis uncovered 1 error.\n"),
        n => {
            let _ = writeln!(o, "The analysis uncovered {n} errors.");
        }
    }
    let _ = writeln!(o, "\n\n{command}");
    Ok((o, errors.len()))
}
