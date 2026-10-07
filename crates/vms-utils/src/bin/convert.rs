//! CONVERT: the records of files of any organization into a file of the
//! organization and record format an FDL file (or else the first input)
//! gives: padded (/PAD), truncated (/TRUNCATE), sorted by the primary key
//! for an indexed file (/SORT), into a new file, an existing one
//! (/NOCREATE), at its end (/APPEND), or among its records (/MERGE).
//! Records that don't fit are exceptions: reported, and kept in
//! /EXCEPTIONS_FILE.

use libvms::files::{self, Reader, Writer};
use libvms::rms;
use std::cmp::Ordering;
use std::path::PathBuf;
use vms_cond::Cond;
use vms_fao::Arg;
use vms_rms::{Design, Fab, KeyType, Org, Record, Rfm, status};
use vms_utils::sort::{self, Key, Typ};
use vms_utils::{Util, inhibit, shr};

const CONV: u32 = 178;
const F: u32 = 4;
const RTS: Cond = Cond(0x00B2_8323);
const RTL: Cond = Cond(0x00B2_832B);
const RSK: Cond = Cond(0x00B2_8333);
const SEQ: Cond = Cond(0x00B2_833B);
const DUP: Cond = Cond(0x00B2_8343);
const RSZ: Cond = Cond(0x00B2_834B);
const OPENFDL: Cond = Cond(0x00B4_808C);

/// Where records go.
enum Out {
    Seq(Writer),
    Rms(rms::File),
}

impl Out {
    fn put(&mut self, rec: &Record) -> Result<(), Cond> {
        match self {
            Out::Seq(w) => w.put(rec),
            Out::Rms(f) => f.put(rec, None).map(drop),
        }
    }
}

fn main() {
    let mut u = Util::new(
        include_str!("../../../../sys/SYSLIB/DCLTABLES/CONVERT.CLD"),
        CONV,
    );
    let own = vms_msg::compile(include_str!("CONVERT.MSG")).expect("CONVERT.MSG");
    u.img.catalog.add_process(own);
    let started = std::time::Instant::now();
    let create = u.present("CREATE");
    let append = u.present("APPEND");
    let merge = u.present("MERGE");
    let truncate = u.present("TRUNCATE");
    let do_sort = u.present("SORT");
    let fast = u.present("FAST_LOAD");
    let statistics = u.present("STATISTICS");
    let pad = u.present("PAD").then(|| match u.value("PAD") {
        Some(v) => pad_char(&v),
        None => 0,
    });
    let exceptions = u
        .present("EXCEPTIONS_FILE")
        .then(|| u.value("EXCEPTIONS_FILE"));
    let fail = |u: Util, c: Cond, what: &str, e: Cond| -> ! {
        u.msg(&[(c, vec![Arg::Str(what)]), (e, vec![])]);
        u.exit(inhibit(c))
    };

    // The design: from the FDL file, or the first input's.
    let fdl = match u.value("FDL") {
        Some(t) => {
            let parsed = u.img.session.parse(&t, ".FDL", "");
            let found = parsed.clone().and_then(|s| u.img.session.find(&s));
            let shown = parsed.map(|s| s.expanded()).unwrap_or(t.clone());
            let text = match found {
                Ok((p, _)) => Reader::open(&p).map(|mut r| {
                    std::iter::from_fn(|| r.get())
                        .map(|r| String::from_utf8_lossy(&r.data).into_owned() + "\n")
                        .collect::<String>()
                }),
                Err(e) => Err(e),
            };
            match text.map(|t| {
                vms_rms::fdl::parse(&t)
                    .ok()
                    .and_then(|f| vms_rms::fdl::to_design(&f).ok())
            }) {
                Ok(Some(d)) => Some(d),
                Ok(None) => fail(u, OPENFDL, &shown, status::SYN),
                Err(e) => fail(u, OPENFDL, &shown, e),
            }
        }
        None => None,
    };

    let texts = vms_utils::texts(&u.values("INPUT"));
    let mut inputs = Vec::new();
    for item in u.expand(&texts, "") {
        match item.files {
            Ok(f) if !f.is_empty() => inputs.extend(f),
            r => {
                let open = u.shared(shr::OPENIN, F);
                fail(
                    u,
                    open,
                    &item.spec.expanded(),
                    r.err().unwrap_or(libvms::status::FNF),
                );
            }
        }
    }
    let mut records = Vec::new();
    let mut first: Option<(Fab, PathBuf)> = None;
    for (path, spec) in &inputs {
        match Reader::open(path) {
            Ok(mut r) => {
                first.get_or_insert((r.fab, path.clone()));
                records.extend(std::iter::from_fn(|| r.get()));
            }
            Err(e) => {
                let open = u.shared(shr::OPENIN, F);
                fail(u, open, &spec.expanded(), e);
            }
        }
    }
    let (in_fab, in_path) = first.unwrap_or_default();
    let blocks = files::info(&in_path).map_or(1, |i| i.used.max(1));

    // The output: made new, or the one there is.
    let out_text = u.value("OUTPUT").unwrap_or_default();
    let parsed = match u.img.session.parse(&out_text, "", "") {
        Ok(s) => s,
        Err(e) => {
            let open = u.shared(shr::OPENOUT, F);
            fail(u, open, &out_text, e)
        }
    };
    let existing = !create || append || merge;
    let target = if existing {
        u.img.session.find(&parsed)
    } else {
        u.img.session.new_version(&parsed)
    };
    let (path, shown) = match target {
        Ok((p, s)) => (p, s.expanded()),
        Err(e) => {
            let open = u.shared(shr::OPENOUT, F);
            fail(u, open, &parsed.expanded(), e)
        }
    };
    let mut design = fdl.unwrap_or(Design {
        fab: Fab { lrl: 0, ..in_fab },
        ..Design::default()
    });
    if existing {
        design.fab = files::fab(&path);
    }
    let fab = design.fab;
    // The longest record the file will have, as RMS keeps it.
    let longest = records
        .iter()
        .filter_map(|r| fit(r, &fab, pad, truncate).ok())
        .map(|r| r.data.len() as u16)
        .max()
        .unwrap_or(0)
        .max(if existing { fab.lrl } else { 0 });
    let made = if existing {
        match fab.org {
            Org::Seq if append => Writer::append(&path).map(Out::Seq),
            // Sequential files are written only at their end.
            Org::Seq if std::fs::metadata(&path).is_ok_and(|m| m.len() > 0) => Err(status::NEF),
            Org::Seq => Writer::append(&path).map(Out::Seq),
            _ => rms::File::open(&path, rms::fab::PUT | rms::fab::GET, 0).map(Out::Rms),
        }
    } else {
        match fab.org {
            Org::Seq => Writer::create(
                &path,
                Fab {
                    lrl: longest,
                    ..fab
                },
            )
            .map(Out::Seq),
            _ => {
                // In 16-block clusters, as on the VMS disk the fixtures come from.
                let mut d = design.clone();
                if d.areas.is_empty() {
                    d.areas.push(Default::default());
                }
                for a in &mut d.areas {
                    a.allocation = a.allocation.max(1).next_multiple_of(16);
                }
                rms::File::create(&path, &d, rms::fab::PUT | rms::fab::GET, 0).map(Out::Rms)
            }
        }
    };
    let mut out = match made {
        Ok(o) => o,
        Err(e) if e == status::NEF => {
            let write = u.shared(shr::WRITEERR, F);
            fail(u, write, &shown, e)
        }
        Err(e) => {
            let open = u.shared(shr::OPENOUT, F);
            fail(u, open, &shown, e)
        }
    };

    // An indexed file is loaded in primary key order: sorted, or (/NOSORT,
    // or from an indexed file) records out of order are exceptions.
    // The primary key, a sort key for each of its segments.
    let key: Option<Vec<Key>> = design.keys.first().map(|k| {
        let typ = match k.typ {
            KeyType::Int2 | KeyType::Int4 | KeyType::Int8 => Typ::Binary { signed: true },
            KeyType::Bin2 | KeyType::Bin4 | KeyType::Bin8 => Typ::Binary { signed: false },
            KeyType::Decimal => Typ::Packed,
            _ => Typ::Character,
        };
        k.segments
            .iter()
            .map(|s| Key {
                pos: s.position as usize,
                size: s.length as usize,
                typ,
                descending: k.descending,
            })
            .collect()
    });
    let keyed = fab.org == Org::Idx && !merge;
    // Exceptions, by input record: they are reported in input order.
    let mut excepted: Vec<(usize, Cond)> = Vec::new();
    let mut fitted: Vec<(usize, Record)> = Vec::new();
    for (i, rec) in records.iter().enumerate() {
        match fit(rec, &fab, pad, truncate) {
            Ok(r) => fitted.push((i, r)),
            Err(c) => excepted.push((i, c)),
        }
    }
    if let (true, Some(k)) = (keyed, &key) {
        let end = k.iter().map(|k| k.pos + k.bytes()).max().unwrap_or(0);
        fitted.retain(|(i, r)| {
            let short = r.data.len() < end;
            if short {
                excepted.push((*i, if fast { RSK } else { RSZ }));
            }
            !short
        });
        let keys: Vec<Vec<sort::Value>> = fitted
            .iter()
            .map(|(_, r)| sort::values(k, &r.data).unwrap_or_default())
            .collect();
        if do_sort && in_fab.org != Org::Idx {
            let p = sort::tree_size(blocks, in_fab.lrl as usize);
            let order = sort::select(fitted.len(), p, |a, b| sort::compare(k, &keys[a], &keys[b]));
            fitted = order.into_iter().map(|i| fitted[i].clone()).collect();
        } else {
            let mut last: Option<usize> = None;
            let mut kept = Vec::new();
            for (n, f) in fitted.into_iter().enumerate() {
                if last.is_some_and(|l| sort::compare(k, &keys[l], &keys[n]) == Ordering::Greater) {
                    excepted.push((f.0, SEQ));
                } else {
                    last = Some(n);
                    kept.push(f);
                }
            }
            fitted = kept;
        }
    }
    let mut written = 0;
    for (i, rec) in &fitted {
        match out.put(rec) {
            Ok(()) => written += 1,
            Err(e) if e == status::DUP => excepted.push((*i, DUP)),
            Err(e) if e == status::RSZ => excepted.push((*i, RSZ)),
            Err(e) => {
                let write = u.shared(shr::WRITEERR, F);
                fail(u, write, &shown, e);
            }
        }
    }
    drop(out);
    if existing && fab.org == Org::Seq && longest > fab.lrl {
        let fab = Fab {
            lrl: longest,
            ..fab
        };
        let _ = libvms::sys::set_xattr(&path, vms_rms::XATTR, fab.to_string().as_bytes());
    }
    excepted.sort_by_key(|e| e.0);

    // /EXCEPTIONS_FILE=file keeps them as read; alone, on SYS$OUTPUT.
    let mut exc_out = match &exceptions {
        Some(Some(t)) => {
            let longest = excepted
                .iter()
                .map(|e| records[e.0].data.len())
                .max()
                .unwrap_or(0);
            let made = u
                .img
                .session
                .parse(t, ".EXC", "")
                .and_then(|s| u.img.session.new_version(&s));
            made.and_then(|(p, _)| {
                Writer::create(
                    &p,
                    Fab {
                        org: Org::Seq,
                        lrl: longest as u16,
                        ..in_fab
                    },
                )
            })
            .ok()
        }
        _ => None,
    };
    for &(i, c) in &excepted {
        u.msg(&[(c, vec![])]);
        match (&mut exc_out, &exceptions) {
            (Some(w), _) => {
                let _ = w.put(&records[i]);
            }
            (None, Some(None)) => println!("{}", String::from_utf8_lossy(&records[i].data)),
            _ => {}
        }
    }

    if statistics {
        let d = started.elapsed();
        let cs = d.as_millis() / 10;
        let t = format!(
            "0 {:02}:{:02}:{:02}.{:02}",
            cs / 360_000,
            cs / 6000 % 60,
            cs / 100 % 60,
            cs % 100
        );
        println!(" \r\n CONVERT Statistics");
        println!("Number of Files Processed:{:>10}", inputs.len());
        println!(
            "Total Records Processed:{:>12}\tBuffered I/O Count: \t{:>8}",
            records.len(),
            0
        );
        println!(
            "Total Exception Records:{:>12}\tDirect I/O Count: \t{:>8}",
            excepted.len(),
            0
        );
        println!(
            "Total Valid Records:{written:>16}\tPage Faults: \t\t{:>8}",
            0
        );
        // ponytail: CPU time as the elapsed time.
        println!("Elapsed Time:{t:>23}\tCPU Time:{t:>23}");
    }
    u.exit(Cond(1));
}

/// /PAD=x: a character, or %X, %O, %D and the like.
fn pad_char(v: &str) -> u8 {
    let v = v.trim().trim_matches('"');
    let up = v.to_ascii_uppercase();
    let num = |s: &str, radix| u8::from_str_radix(s, radix).ok();
    match up.get(..2) {
        Some("%X") => num(&up[2..], 16),
        Some("%O") => num(&up[2..], 8),
        Some("%D") => num(&up[2..], 10),
        _ => v.bytes().next(),
    }
    .unwrap_or(0)
}

/// A record as the output's format takes it, or the exception it makes.
fn fit(rec: &Record, fab: &Fab, pad: Option<u8>, truncate: bool) -> Result<Record, Cond> {
    let mut r = rec.clone();
    let max = fab.mrs as usize;
    if max != 0 && r.data.len() > max {
        if !truncate {
            return Err(RTL);
        }
        r.data.truncate(max);
    }
    if fab.rfm == Rfm::Fix && r.data.len() < max {
        let Some(c) = pad else { return Err(RTS) };
        r.data.resize(max, c);
    }
    if fab.rfm == Rfm::Vfc {
        r.control.resize(fab.fsz as usize, 0);
    } else {
        r.control.clear();
    }
    Ok(r)
}
