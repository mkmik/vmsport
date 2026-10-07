//! SORT and MERGE, one image as on VMS: the records of files of any
//! organization put in order by their keys, into a new file of the
//! organization and format the output qualifiers ask for (sequential,
//! like the first input, by default), or into an existing one (/OVERLAY).
//! Equal keys come out as VMS's sort leaves them (vms_utils::sort), or in
//! input order with /STABLE.

use libvms::files::{self, Reader, Writer};
use libvms::rms;
use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use vms_cond::Cond;
use vms_fao::Arg;
use vms_rms::{Area, Design, Fab, Org, Record, Rfm};
use vms_utils::sort::{self, Key, Typ, Value};
use vms_utils::{E, NOSUCHFILE, Util, inhibit, shr};

const SORT: u32 = 28;
const BAD_KEY: Cond = Cond(0x001C_8034);
const IND_OVR: Cond = Cond(0x001C_8050);
const VAR_FIX: Cond = Cond(0x001C_8060);
const KEY_LEN: Cond = Cond(0x001C_80A8);
const BAD_ORDER: Cond = Cond(0x001C_80D0);
const BAD_SRL: Cond = Cond(0x001C_80F8);
const F: u32 = 4;

/// A record, where it came from, and its key values.
struct Item {
    rec: Record,
    rfa: [u8; 6],
    keys: Vec<Value>,
    /// Which input (MERGE's order between equal keys).
    input: usize,
}

/// The /KEY qualifiers in $LINE, in order: CLI keeps only the last.
fn key_texts(line: &str) -> Vec<String> {
    let b: Vec<char> = line.chars().collect();
    let (mut out, mut i, mut quoted) = (Vec::new(), 0, false);
    while i < b.len() {
        match b[i] {
            '"' => quoted = !quoted,
            '/' if !quoted => {
                let start = i + 1;
                let mut j = start;
                while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == '_') {
                    j += 1;
                }
                let name: String = b[start..j].iter().collect::<String>().to_ascii_uppercase();
                if !name.is_empty()
                    && "KEY".starts_with(&name)
                    && j < b.len()
                    && matches!(b[j], '=' | ':')
                {
                    let (mut k, mut depth) = (j + 1, 0);
                    while k < b.len() && !(depth == 0 && (b[k] == '/' || b[k].is_whitespace())) {
                        match b[k] {
                            '(' => depth += 1,
                            ')' => depth -= 1,
                            _ => {}
                        }
                        k += 1;
                    }
                    out.push(
                        b[j + 1..k]
                            .iter()
                            .collect::<String>()
                            .trim_matches(['(', ')'])
                            .to_string(),
                    );
                    i = k;
                    continue;
                }
            }
            _ => {}
        }
        i += 1;
    }
    out
}

/// One /KEY's value, `(POSITION:9,SIZE:4,DESCENDING)`: the key and its
/// NUMBER, or None if it lacks a position or a size.
fn parse_key(text: &str) -> Option<(Key, Option<u32>)> {
    const WORDS: [&str; 16] = [
        "POSITION",
        "SIZE",
        "NUMBER",
        "ASCENDING",
        "DESCENDING",
        "CHARACTER",
        "BINARY",
        "DECIMAL",
        "PACKED_DECIMAL",
        "ZONED",
        "SIGNED",
        "UNSIGNED",
        "LEADING_SIGN",
        "TRAILING_SIGN",
        "OVERPUNCHED_SIGN",
        "SEPARATE_SIGN",
    ];
    let (mut pos, mut size, mut number) = (None, None, None);
    let mut key = Key {
        pos: 0,
        size: 0,
        typ: Typ::Character,
        descending: false,
    };
    let (mut signed, mut leading, mut separate, mut decimal) = (true, false, false, false);
    for item in text.split(',') {
        let (name, value) = item.split_once([':', '=']).unwrap_or((item, ""));
        let name = name.trim().to_ascii_uppercase();
        let word = WORDS.iter().find(|w| w.starts_with(name.as_str()))?;
        let n = || value.trim().parse::<usize>().ok();
        match *word {
            "POSITION" => pos = n(),
            "SIZE" => size = n(),
            "NUMBER" => number = n().map(|n| n as u32),
            "DESCENDING" => key.descending = true,
            "ASCENDING" => key.descending = false,
            "BINARY" => key.typ = Typ::Binary { signed: true },
            "DECIMAL" | "ZONED" => decimal = true,
            "PACKED_DECIMAL" => key.typ = Typ::Packed,
            "UNSIGNED" => signed = false,
            "LEADING_SIGN" => leading = true,
            "SEPARATE_SIGN" => separate = true,
            _ => {}
        }
    }
    if let Typ::Binary { .. } = key.typ {
        key.typ = Typ::Binary { signed };
    }
    if decimal {
        key.typ = Typ::Decimal { leading, separate };
    }
    // POSITION:0 is kept as such, for KEY_LEN.
    key.pos = pos?.wrapping_sub(1);
    key.size = size?;
    Some((key, number))
}

type Records = Vec<(Record, [u8; 6])>;

/// A file's records with their RFAs as SORT/PROCESS=ADDRESS gives them:
/// VBN and byte offset of a sequential record, a relative one's number.
fn read(path: &Path) -> Result<(Fab, Records), Cond> {
    let mut r = Reader::open(path)?;
    let fab = r.fab;
    let rfa = |vbn: u32, id: u16| {
        let mut b = [0; 6];
        b[..4].copy_from_slice(&vbn.to_le_bytes());
        b[4..].copy_from_slice(&id.to_le_bytes());
        b
    };
    let mut out = Vec::new();
    let mut offset = 0;
    let mut n = 0;
    while let Some(rec) = r.get() {
        n += 1;
        let a = match fab.org {
            // ponytail: where the record's bytes start, as for files
            // without RAT=BLK (whose records may move to a block's start).
            Org::Seq => {
                let a = rfa((offset / 512) as u32 + 1, (offset % 512) as u16);
                offset += vms_rms::encode_record(&fab, offset, &rec).map_or(0, |b| b.len());
                a
            }
            _ => rfa(n, 0),
        };
        out.push((rec, a));
    }
    Ok((fab, out))
}

enum OutKind {
    Seq(Writer),
    Rms(rms::File),
}

fn main() {
    let mut u = Util::new(
        include_str!("../../../../sys/SYSLIB/DCLTABLES/SORT.CLD"),
        SORT,
    );
    let merge = u.value("$VERB").is_some_and(|v| v.starts_with("MERG"));
    let line = u.value("$LINE").unwrap_or_default();
    let stable = u.present("STABLE");
    let nodup = !u.present("DUPLICATES");
    let check = merge && u.present("CHECK_SEQUENCE");
    let statistics = u.present("STATISTICS");
    let process = u.value("PROCESS").unwrap_or_else(|| "RECORD".into());
    let started = std::time::Instant::now();
    let fail = |u: Util, c: Cond, args: Vec<Arg>| -> ! {
        u.msg(&[(c, args)]);
        u.exit(inhibit(c))
    };

    let mut keys: Vec<(Key, Option<u32>)> = Vec::new();
    for t in key_texts(&line) {
        match parse_key(&t) {
            Some(k) => keys.push(k),
            None => fail(u, Cond(BAD_KEY.0 | F), vec![]),
        }
    }
    if keys.iter().any(|k| k.1.is_some()) {
        keys.sort_by_key(|k| k.1.unwrap_or(u32::MAX));
    }
    let keys: Vec<Key> = keys.into_iter().map(|k| k.0).collect();

    // The inputs, read whole.
    let texts = vms_utils::texts(&u.values("INPUT"));
    let mut inputs = Vec::new();
    for item in u.expand(&texts, "") {
        match item.files {
            Ok(f) if !f.is_empty() => inputs.extend(f),
            r => {
                let e = r.err().unwrap_or(libvms::status::FNF);
                let open = u.shared(shr::OPENIN, F);
                u.msg(&[(open, vec![Arg::Str(&item.spec.expanded())]), (e, vec![])]);
                u.exit(inhibit(open));
            }
        }
    }
    let mut first: Option<Fab> = None;
    let mut read_in = Vec::new();
    let mut blocks = 0;
    for (path, spec) in &inputs {
        match read(path) {
            Ok((fab, recs)) => {
                first.get_or_insert(fab);
                blocks += files::info(path).map_or(1, |i| i.used.max(1));
                read_in.push((spec.expanded(), fab, recs));
            }
            Err(e) => {
                let open = u.shared(shr::OPENIN, F);
                u.msg(&[(open, vec![Arg::Str(&spec.expanded())]), (e, vec![])]);
                u.exit(inhibit(open));
            }
        }
    }
    let in_fab = first.unwrap_or_default();
    let lrl = read_in
        .iter()
        .flat_map(|(_, f, r)| {
            std::iter::once(f.lrl as usize).chain(r.iter().map(|(r, _)| r.data.len()))
        })
        .max()
        .unwrap_or(0);

    // Keys that can't be, or reach past the longest record.
    let mut status = Cond(1);
    for (i, k) in keys.iter().enumerate() {
        let args = vec![Arg::Num(i as i64 + 1), Arg::Num(k.bytes() as i64)];
        // A numeric key no record can hold is an error; a character key
        // past the end of records is padded.
        let past = k.pos.wrapping_add(k.bytes()) > lrl;
        if k.pos == usize::MAX || k.size == 0 || past && k.typ != Typ::Character {
            fail(u, Cond(KEY_LEN.0 | E), args);
        }
        if past {
            u.msg(&[(Cond(KEY_LEN.0 | 3), args)]);
            status = Cond(KEY_LEN.0 | 3);
        }
    }

    let mut items = Vec::new();
    let mut read_count = 0;
    for (n, (spec, _, recs)) in read_in.into_iter().enumerate() {
        for (rec, rfa) in recs {
            read_count += 1;
            match sort::values(&keys, &rec.data) {
                Some(v) => items.push(Item {
                    rec,
                    rfa,
                    keys: v,
                    input: n,
                }),
                None => {
                    u.msg(&[
                        (BAD_SRL, vec![Arg::Num(rec.data.len() as i64)]),
                        (u.shared(shr::READERR, E), vec![Arg::Str(&spec)]),
                    ]);
                    status = BAD_SRL;
                }
            }
        }
    }
    let sorted_count = items.len();

    let cmp = |a: &Item, b: &Item| sort::compare(&keys, &a.keys, &b.keys);
    let order: Vec<usize> = if merge {
        // The inputs' heads, the first input's on equal keys; a record
        // before its input's last is out of order.
        let mut heads: Vec<usize> = Vec::new();
        let mut by_input: Vec<Vec<usize>> = Vec::new();
        for (i, it) in items.iter().enumerate() {
            if by_input.len() <= it.input {
                by_input.resize(it.input + 1, Vec::new());
            }
            by_input[it.input].push(i);
        }
        heads.resize(by_input.len(), 0);
        let mut out = Vec::new();
        let mut last: Vec<Option<usize>> = vec![None; by_input.len()];
        loop {
            let best = (0..by_input.len())
                .filter(|&f| heads[f] < by_input[f].len())
                .min_by(|&a, &b| {
                    cmp(&items[by_input[a][heads[a]]], &items[by_input[b][heads[b]]])
                        .then(a.cmp(&b))
                });
            let Some(f) = best else { break };
            let i = by_input[f][heads[f]];
            heads[f] += 1;
            if check && last[f].is_some_and(|l| cmp(&items[i], &items[l]) == Ordering::Less) {
                u.msg(&[(BAD_ORDER, vec![])]);
                status = BAD_ORDER;
            }
            last[f] = Some(i);
            out.push(i);
        }
        out
    } else if stable {
        let mut o: Vec<usize> = (0..items.len()).collect();
        o.sort_by(|&a, &b| cmp(&items[a], &items[b]));
        o
    } else {
        let p = sort::tree_size(blocks, lrl);
        sort::select(items.len(), p, |a, b| cmp(&items[a], &items[b]))
    };
    // /NODUPLICATES: the first of each key, as far as vmsport knows
    // (ponytail: VMS drops them inside its tree, keeping another).
    let mut order = order;
    if nodup {
        order.dedup_by(|b, a| cmp(&items[*a], &items[*b]) == Ordering::Equal);
    }

    // The output: what it holds, its organization and format.
    let mut out_text = u.value("OUTPUT").unwrap_or_default();
    let relative = u.present("RELATIVE");
    let indexed = u.present("INDEXED_SEQUENTIAL");
    let overlay = u.present("OVERLAY");
    let format = vms_utils::texts(&u.values("FORMAT"));
    out_text = out_text.trim().to_string();
    let key_bytes: usize = keys.iter().map(Key::bytes).sum();
    let (records, mut fab): (Vec<Vec<u8>>, Fab) = match process.as_str() {
        p if p.starts_with("ADDR") => (
            order.iter().map(|&i| items[i].rfa.to_vec()).collect(),
            Fab {
                rfm: Rfm::Fix,
                mrs: 6,
                lrl: 6,
                fsz: 0,
                ..in_fab
            },
        ),
        p if p.starts_with("INDE") => (
            order
                .iter()
                .map(|&i| {
                    let mut r = items[i].rfa.to_vec();
                    for k in &keys {
                        let mut kb = items[i].rec.data.get(k.pos..).unwrap_or(&[]).to_vec();
                        kb.resize(k.bytes(), 0);
                        r.extend(kb);
                    }
                    r
                })
                .collect(),
            Fab {
                rfm: Rfm::Fix,
                mrs: 6 + key_bytes as u16,
                lrl: 6 + key_bytes as u16,
                fsz: 0,
                ..in_fab
            },
        ),
        _ => (
            order.iter().map(|&i| items[i].rec.data.clone()).collect(),
            Fab {
                org: Org::Seq,
                ..in_fab
            },
        ),
    };
    let controls: Vec<Vec<u8>> = order
        .iter()
        .map(|&i| items[i].rec.control.clone())
        .collect();
    for f in &format {
        let (name, value) = f.split_once([':', '=']).unwrap_or((f, ""));
        let n: Option<u16> = value.trim().parse().ok();
        match name.trim() {
            n_ if "FIXED".starts_with(n_) => {
                fab.rfm = Rfm::Fix;
                fab.mrs = n.unwrap_or(lrl as u16);
            }
            n_ if "VARIABLE".starts_with(n_) => {
                fab.rfm = Rfm::Var;
                fab.mrs = n.unwrap_or(0);
            }
            n_ if "CONTROLLED".starts_with(n_) => {
                fab.rfm = Rfm::Vfc;
                fab.mrs = n.unwrap_or(0);
            }
            _ => {}
        }
    }
    if relative && fab.mrs == 0 {
        fab.mrs = lrl as u16;
    }

    let mut parsed = match u.img.session.parse(&out_text, "", "") {
        Ok(s) => s,
        Err(e) => openout(u, &out_text, e),
    };
    if overlay {
        parsed.version = None;
    }
    let target = if overlay {
        u.img.session.find(&parsed)
    } else {
        u.img.session.new_version(&parsed)
    };
    let (path, shown): (PathBuf, String) = match target {
        Ok((p, s)) => (p, s.expanded()),
        Err(e) => openout(u, &out_text, e),
    };
    let existing = overlay.then(|| files::fab(&path));
    if indexed && existing.is_none_or(|f| f.org != Org::Idx) {
        u.msg(&[(IND_OVR, vec![])]);
        status = IND_OVR;
    }
    if let Some(f) = existing {
        fab = f;
    }
    let differs = fab.rfm != in_fab.rfm || fab.mrs != 0 && fab.mrs as usize != lrl;
    if differs && process.starts_with("RECO") || differs && process.starts_with("TAG") {
        u.msg(&[(VAR_FIX, vec![Arg::Str(&shown)])]);
        status = VAR_FIX;
    }
    let made = match fab.org {
        _ if existing.is_some() && fab.org == Org::Seq => std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&path)
            .map_err(files::io_status)
            .map(|_| ())
            .and_then(|()| Writer::append(&path))
            .map(OutKind::Seq),
        _ if existing.is_some() => {
            rms::File::open(&path, rms::fab::PUT | rms::fab::GET, 0).map(OutKind::Rms)
        }
        _ if relative => {
            // What the records need, in 16-block clusters as on the VMS
            // disk the fixtures come from.
            let cells = (fab.mrs as usize + 3) * records.len();
            let area = Area {
                allocation: (1 + cells.div_ceil(512) as u32).next_multiple_of(16),
                ..Area::default()
            };
            let d = Design {
                fab: Fab {
                    org: Org::Rel,
                    ..fab
                },
                areas: vec![area],
                ..Design::default()
            };
            let f = rms::File::create(&path, &d, rms::fab::PUT, 0);
            // SORT leaves the longest record unset.
            let fab0 = Fab {
                lrl: 0,
                ..files::fab(&path)
            };
            let _ = libvms::sys::set_xattr(&path, vms_rms::XATTR, fab0.to_string().as_bytes());
            f.map(OutKind::Rms)
        }
        _ => {
            let longest = records
                .iter()
                .map(|r| out_len(&fab, r.len()))
                .max()
                .unwrap_or(0);
            Writer::create(
                &path,
                Fab {
                    org: Org::Seq,
                    lrl: longest as u16,
                    ..fab
                },
            )
            .map(OutKind::Seq)
        }
    };
    let mut out = match made {
        Ok(k) => k,
        Err(e) => openout(u, &out_text, e),
    };
    let prim = keys.first().map_or(0, |k| k.pos + k.bytes());
    for (data, control) in records.iter().zip(&controls) {
        let mut data = data.clone();
        match fab.rfm {
            Rfm::Fix => data.resize(fab.mrs as usize, 0),
            _ if fab.mrs != 0 => data.truncate(fab.mrs as usize),
            _ => {}
        }
        // An indexed file's records must hold its keys.
        if fab.org == Org::Idx && data.len() < prim {
            data.resize(fab.mrs as usize, 0);
        }
        let rec = Record {
            control: control.clone(),
            data,
        };
        let put = match &mut out {
            OutKind::Seq(w) => w.put(&rec),
            OutKind::Rms(f) => f.put(&rec, None).map(drop),
        };
        if let Err(e) = put {
            let write = u.shared(shr::WRITEERR, F);
            u.msg(&[(write, vec![Arg::Str(&shown)]), (e, vec![])]);
            u.exit(inhibit(write));
        }
    }
    drop(out);

    if statistics {
        let il = match process.as_str() {
            p if !p.starts_with("RECO") => 6 + key_bytes + 2,
            _ => {
                let decimal = keys
                    .iter()
                    .any(|k| matches!(k.typ, Typ::Decimal { .. } | Typ::Packed));
                lrl + in_fab.fsz as usize
                    + 2
                    + if stable { 4 } else { 0 }
                    + if decimal { 4 } else { 0 }
            }
        };
        let tree = if merge {
            0
        } else {
            sort::tree_size(blocks, lrl)
        };
        let runs = if merge { inputs.len() } else { 0 };
        let out_lrl = match fab.rfm {
            Rfm::Fix => fab.mrs as usize,
            _ => lrl,
        };
        report(
            read_count,
            sorted_count,
            order.len(),
            lrl,
            il,
            out_lrl,
            tree,
            runs,
            started.elapsed(),
        );
    }
    if status == Cond(1) {
        u.exit(status);
    }
    u.exit(inhibit(status));
}

/// The length a record gets in the output.
fn out_len(fab: &Fab, len: usize) -> usize {
    match fab.rfm {
        Rfm::Fix => fab.mrs as usize,
        _ if fab.mrs != 0 => len.min(fab.mrs as usize),
        _ => len,
    }
}

/// SORT-F-OPENOUT on the output as typed, the reason, and for a missing
/// directory the system's.
fn openout(u: Util, text: &str, e: Cond) -> ! {
    let open = u.shared(shr::OPENOUT, F);
    let text = text.to_ascii_uppercase();
    let mut m = vec![(open, vec![Arg::Str(&text)]), (e, vec![])];
    if e == libvms::status::DNF {
        m.push((NOSUCHFILE, vec![]));
    }
    u.msg(&m);
    u.exit(inhibit(open))
}

/// SORT/STATISTICS, laid out as VMS lays it out, in three records.
#[allow(clippy::too_many_arguments)]
fn report(
    read: usize,
    sorted: usize,
    output: usize,
    lrl: usize,
    il: usize,
    out_lrl: usize,
    tree: usize,
    runs: usize,
    elapsed: std::time::Duration,
) {
    let row = |l: &str, a: String, r: &str, b: String| {
        format!(
            "{l}{a:>w$}          {r}{b:>v$}",
            w = 25 - l.len(),
            v = 29 - r.len()
        )
    };
    let n = |v: usize| v.to_string();
    let time = |d: std::time::Duration| {
        let cs = d.as_millis() / 10;
        format!(
            "{:02}:{:02}:{:02}.{:02}",
            cs / 360_000,
            cs / 6000 % 60,
            cs / 100 % 60,
            cs % 100
        )
    };
    // ponytail: CPU time as the elapsed time.
    let cpu = elapsed;
    let order = runs;
    let passes = usize::from(runs > 0);
    print!(
        "\r\n                  OpenVMS Sort/Merge Statistics\r\n\r\n{}\r\n{}\r\n{}\n{}\r\n{}\r\n{}\n{}\r\n{}\r\nElapsed time: {}          Elapsed CPU:      {}   \n",
        row("Records read:", n(read), "Input record length:", n(lrl)),
        row("Records sorted:", n(sorted), "Internal length:", n(il)),
        row(
            "Records output:",
            n(output),
            "Output record length:",
            n(out_lrl)
        ),
        row("Working set:", n(24000), "Sort tree size:", n(tree)),
        row(
            "Virtual memory:",
            n(272),
            "Number of initial runs:",
            n(runs)
        ),
        row("Direct I/O:", n(0), "Maximum merge order:", n(order)),
        row("Buffered I/O:", n(0), "Number of merge passes:", n(passes)),
        row("Page faults:", n(0), "Work file alloc:", n(0)),
        time(elapsed),
        time(cpu),
    );
}
