//! The indexed files' scripts: INDEXED, ADD_KEY, TOUCHUP and OPTIMIZE.
//! Each key is asked about, then shown as a plot of the index depth at
//! each bucket size with what it was told by two-letter names, which can
//! be changed until FD finishes the design; then the bucket sizes EDF
//! suggests, and the one to use.

use super::scripts::{RETURN, TYPE_LIST, TYPES};
use super::{Answer, Console, Editor, Q, Takes, keyword, plot, surface, syntax, token};
use vms_rms::edf;
use vms_rms::fdl::{Fdl, Section};

/// What a design was told.
#[derive(Clone)]
pub(super) struct Design {
    pub script: &'static str,
    pub cluster: u32,
    pub graph: &'static str,
    /// A surface plot's lowest and highest row.
    pub bounds: (u64, u64),
    pub method: &'static str,
    pub load: u64,
    pub added: u64,
    pub format: &'static str,
    pub mean: u64,
    pub max: u64,
    pub prolog: u64,
    pub emphasis: &'static str,
    pub keys: Vec<KeyDesign>,
}

/// What a key was told.
#[derive(Clone)]
pub(super) struct KeyDesign {
    pub fill: u64,
    pub ktype: &'static str,
    pub segs: Vec<(u64, u64)>,
    pub dups: bool,
    pub dkc: bool,
    pub drc: bool,
    pub ic: bool,
    pub changes: bool,
    pub name: String,
    pub bucket: u32,
}

impl KeyDesign {
    fn new(k: usize) -> KeyDesign {
        KeyDesign {
            fill: 100,
            ktype: "STRING",
            segs: Vec::new(),
            dups: k > 0,
            dkc: true,
            drc: k == 0,
            ic: false,
            changes: false,
            name: String::new(),
            bucket: 0,
        }
    }

    fn length(&self) -> u64 {
        self.segs.iter().map(|s| s.0).sum()
    }
}

/// The largest record an indexed file's questions take.
const TOP: u64 = 32224;

fn number(a: Answer) -> u64 {
    match a {
        Answer::Number(n) => n,
        _ => 0,
    }
}

fn word(a: Answer) -> &'static str {
    match a {
        Answer::Word(w) => w,
        _ => "",
    }
}

/// A keyword as the questions show it: `String`, `Fast_Conv`.
fn shown(w: &str) -> String {
    w.split('_')
        .map(|p| p[..1].to_string() + &p[1..].to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join("_")
}

impl Editor<'_> {
    /// INDEXED: a new indexed file, its keys one after another.
    pub(super) fn indexed(&mut self, c: &mut impl Console) -> Option<Fdl> {
        let mut d = self.design_start("INDEXED");
        d.cluster = self.cluster(c)?;
        let q = Q::number("Number of Keys to Define", 1, 255)
            .or(&self.keys.to_string(), Answer::Number(self.keys))
            .explained("\tAn Indexed file can have from 1 to 255 keys.\n");
        let keys = number(self.ask(c, &q)?) as usize;
        let mut title = String::new();
        let mut file = String::new();
        let mut cc = "CARRIAGE_RETURN";
        let mut buffers = None;
        for k in 0..keys {
            d.keys.push(KeyDesign::new(k));
            self.graph_question(c, &mut d, k)?;
            if k == 0 {
                self.load(c, &mut d)?;
            }
            self.key_questions(c, &mut d, k)?;
            self.screen(c, &mut d, k)?;
            if k == 0 {
                (title, file, cc) = self.file_questions(c)?;
            }
            self.finish_key(c, &mut d, k)?;
            if k == 0 {
                buffers = Some(self.buffers(c)?);
            }
            self.depth_message(c, &d, k)?;
        }
        let f = self.design_fdl(&d, &title, &file, cc, buffers.flatten())?;
        Some(self.prologue_deferred(f, &d))
    }

    /// ADD_KEY: one more key for the indexed file.
    pub(super) fn add_key(&mut self, c: &mut impl Console) -> Option<Fdl> {
        let k = self.key_sections().len();
        let mut d = self.design_start("ADD_KEY");
        d.cluster = self.cluster(c)?;
        d.keys = (0..=k).map(KeyDesign::new).collect();
        self.graph_question(c, &mut d, k)?;
        self.load(c, &mut d)?;
        self.key_questions(c, &mut d, k)?;
        self.screen(c, &mut d, k)?;
        self.finish_key(c, &mut d, k)?;
        self.depth_message(c, &d, k)?;
        Some(self.key_into(&d, k))
    }

    /// TOUCHUP: one key designed again.
    pub(super) fn touchup(&mut self, c: &mut impl Console) -> Option<Fdl> {
        let n = self.key_sections().len().max(1) as u64;
        let mut d = self.design_start("TOUCHUP");
        d.cluster = self.cluster(c)?;
        let q = Q::number("Key of Reference", 0, n - 1).or("0", Answer::Number(0));
        let k = number(self.ask(c, &q)?) as usize;
        self.replaced(c, k)?;
        d.keys = (0..=k).map(KeyDesign::new).collect();
        self.graph_question(c, &mut d, k)?;
        self.load(c, &mut d)?;
        self.key_questions(c, &mut d, k)?;
        self.screen(c, &mut d, k)?;
        if k > 0 {
            self.finish_key(c, &mut d, k)?;
            self.depth_message(c, &d, k)?;
            return Some(self.key_into(&d, k));
        }
        // The primary key is designed as INDEXED designs it; the other
        // keys stay.
        let (title, file, cc) = self.file_questions(c)?;
        self.finish_key(c, &mut d, k)?;
        let flags = self.buffers(c)?;
        self.depth_message(c, &d, k)?;
        let mut f = self.design_fdl(&d, &title, &file, cc, flags)?;
        f.sections.extend(
            self.key_sections()
                .into_iter()
                .filter(|s| s.value.trim() != "0")
                .cloned(),
        );
        Some(self.prologue_deferred(f, &d))
    }

    /// A designed definition as the editor holds it until EXIT: AREA 0
    /// without the prologue's blocks, which EXIT adds (VMS's VIEW after a
    /// design shows the one, the file written the other).
    fn prologue_deferred(&mut self, mut f: Fdl, d: &Design) -> Fdl {
        let keys = d.keys.len() as u32;
        let p = (2 + (keys.max(1) - 1).div_ceil(2)).div_ceil(d.cluster) * d.cluster;
        if let Some(a) = f
            .sections
            .iter_mut()
            .find(|s| s.name == "AREA" && s.value.trim() == "0")
        {
            let alloc: u32 = a
                .get("ALLOCATION")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            a.set("ALLOCATION", alloc.saturating_sub(p));
            self.prologue = p;
        }
        super::normalized(f)
    }

    /// OPTIMIZE: every key designed again for the data the analysis
    /// describes.
    pub(super) fn optimize(&mut self, c: &mut impl Console) -> Option<Fdl> {
        if self.analysis.is_none() {
            c.say("\n\tAn Input Analysis File is necessary for Optimizing Keys.\n\t\n");
            let mut q = Q::new("Analysis File file-spec\t", Takes::Text { max: 512 })
                .or("null", Answer::Text(String::new()));
            q.lead = false;
            let Answer::Text(spec) = self.ask(c, &q)? else {
                return None;
            };
            c.say("\n\tParsing Analysis File\n");
            self.analysis = c.read(&spec).and_then(|t| vms_rms::fdl::parse(&t).ok());
        } else {
            c.say("\n\tParsing Analysis File\n\tAnalysis Parse Complete\n");
        }
        let analysis = self.analysis.clone().unwrap_or_default();
        if analysis.section("ANALYSIS_OF_KEY", "0").is_none() {
            c.say("\tThe Analysis File must contain ANALYSIS_OF_KEY primary sections.\n\tThe DCL command \"ANALYZE/RMS_FILE/FDL\" produces Analysis Files.\n");
            return None;
        }
        let mut d = self.design_start("OPTIMIZE");
        d.cluster = vms_rms::edf::keys(&self.fdl, &analysis).map_or(3, |k| k.0);
        let record = self.fdl.section("RECORD", "");
        d.format = match record.and_then(|r| r.get("FORMAT")) {
            Some(f) if f.eq_ignore_ascii_case("fixed") => "FIXED",
            _ => "VARIABLE",
        };
        let size = record
            .and_then(|r| r.get("SIZE"))
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let stat = |k: usize, name: &str| {
            analysis
                .section("ANALYSIS_OF_KEY", &k.to_string())
                .and_then(|s| s.get(name))
                .and_then(|v| v.trim().parse::<u64>().ok())
                .unwrap_or(0)
        };
        (d.mean, d.max) = match d.format {
            "FIXED" => (size, size),
            _ => (stat(0, "MEAN_DATA_LENGTH"), size),
        };
        d.keys = self
            .key_sections()
            .iter()
            .enumerate()
            .map(|(k, s)| from_section(k, s))
            .collect();
        let mut title = String::new();
        let mut file = String::new();
        let mut cc = "CARRIAGE_RETURN";
        for k in 0..d.keys.len() {
            self.replaced(c, k)?;
            self.graph_question(c, &mut d, k)?;
            if k == 0 {
                self.reload(c, &mut d, stat(0, "DATA_RECORD_COUNT"))?;
            }
            self.key_questions(c, &mut d, k)?;
            self.screen(c, &mut d, k)?;
            if k == 0 {
                (title, file, cc) = self.file_questions(c)?;
            }
            self.finish_key(c, &mut d, k)?;
            self.depth_message(c, &d, k)?;
        }
        let f = self.design_fdl(&d, &title, &file, cc, None)?;
        Some(self.prologue_deferred(f, &d))
    }

    fn design_start(&self, script: &'static str) -> Design {
        Design {
            script,
            cluster: 3,
            graph: self.graph,
            bounds: (50, 100),
            method: "FAST_CONVERT",
            load: 0,
            added: 0,
            format: "VARIABLE",
            mean: 0,
            max: 0,
            prolog: 3,
            emphasis: self.emphasis,
            keys: Vec::new(),
        }
    }

    pub(super) fn key_sections(&self) -> Vec<&Section> {
        self.fdl
            .sections
            .iter()
            .filter(|s| s.name == "KEY")
            .collect()
    }

    pub(super) fn cluster(&self, c: &mut impl Console) -> Option<u32> {
        let q =
            Q::number("Target disk volume Cluster Size", 1, 2147483647).or("3", Answer::Number(3));
        Some(number(self.ask(c, &q)?) as u32)
    }

    fn replaced(&self, c: &mut impl Console, k: usize) -> Option<()> {
        c.say(&format!(
            "\n\tThe Definition of Key {k:>2} will be replaced.\n"
        ));
        self.press_return(c, RETURN)
    }

    fn graph_question(&self, c: &mut impl Console, d: &mut Design, k: usize) -> Option<()> {
        d.graph = self.graph(c, k as u32)?;
        c.say("\n");
        Some(())
    }

    /// The records loaded, how, and those added after.
    fn load(&self, c: &mut impl Console, d: &mut Design) -> Option<()> {
        let q = Q::number(
            "Number of Records that will be Initially Loaded\ninto the File",
            0,
            2147483647,
        )
        .explained("\tThese are the records initially loaded into the file.\n\tIf the file will have no \"Load\" operation, specify \"0\".\n");
        d.load = number(self.ask(c, &q)?);
        d.method = self.method(c, "Initial File Load Method")?;
        d.added = self.added(c, d.load, "the Initial File Load")?;
        Some(())
    }

    /// OPTIMIZE's: the records reloaded, how, and those added after.
    fn reload(&self, c: &mut impl Console, d: &mut Design, count: u64) -> Option<()> {
        let q = Q::number(
            "Number of Records that will be Reloaded\ninto the File",
            0,
            2147483647,
        )
        .or(&count.to_string(), Answer::Number(count));
        d.load = number(self.ask(c, &q)?);
        d.method = self.method(c, "File Reloading Method")?;
        d.added = self.added(c, d.load, "the Reloading the File")?;
        Some(())
    }

    fn method(&self, c: &mut impl Console, text: &str) -> Option<&'static str> {
        let q = Q::new(
            text,
            Takes::Keyword {
                words: &["FAST_CONVERT", "NOFAST_CONVERT", "RMS_PUTS"],
                list: "\t(Fast_Convert NoFast_Convert RMS_Puts)\n",
            },
        )
        .or("Fast", Answer::Word("FAST_CONVERT"));
        Some(word(self.ask(c, &q)?))
    }

    fn added(&self, c: &mut impl Console, load: u64, after: &str) -> Option<u64> {
        let mut q = Q::number(
            &format!("Number of Additional Records to be Added After\n{after}"),
            0,
            2147483645 - load.min(2147483645),
        )
        .or("0", Answer::Number(0));
        q.sep = Some("\t: ");
        Some(number(self.ask(c, &q)?))
    }

    fn fill(&self, c: &mut impl Console, k: usize, d: &mut Design) -> Option<()> {
        let q = Q::number(&format!("Key {k:>2} Load Fill Percent"), 50, 100)
            .or("100", Answer::Number(100));
        d.keys[k].fill = number(self.ask(c, &q)?);
        Some(())
    }

    /// A key's questions, as the script asks them.
    fn key_questions(&self, c: &mut impl Console, d: &mut Design, k: usize) -> Option<()> {
        let optimize = d.script == "OPTIMIZE";
        if d.graph == "FILL" {
            let bound = |hi: bool, d: u64| {
                Q::number(
                    &format!(
                        "{} bound: Key {k:>2} Init Fill %",
                        if hi { "High" } else { "Low" }
                    ),
                    50,
                    100,
                )
                .or(&d.to_string(), Answer::Number(d))
            };
            let lo = number(self.ask(c, &bound(false, 50))?);
            let hi = number(self.ask(c, &bound(true, 100))?);
            d.bounds = (lo, hi);
        } else {
            self.fill(c, k, d)?;
        }
        if !optimize && (k == 0 || d.script != "INDEXED") {
            self.record(c, d)?;
        }
        let q = Q::new(
            &format!("Key {k:>2} Data Type"),
            Takes::Keyword {
                words: &TYPES,
                list: TYPE_LIST,
            },
        )
        .or("Str", Answer::Word("STRING"));
        d.keys[k].ktype = word(self.ask(c, &q)?);
        let q = Q::new(&format!("Key {k:>2} Segmentation desired"), Takes::YesNo)
            .or("No", Answer::Yes(false));
        let segmented = self.ask(c, &q)? == Answer::Yes(true);
        if !optimize {
            self.segments(c, d, k, segmented)?;
            let q = Q::new(&format!("Key {k:>2} Duplicates allowed"), Takes::YesNo)
                .or(if k == 0 { "No" } else { "Yes" }, Answer::Yes(k > 0));
            d.keys[k].dups = self.ask(c, &q)? == Answer::Yes(true);
        }
        if k == 0 || matches!(d.script, "ADD_KEY" | "TOUCHUP") {
            let q = Q::number("File Prolog Version", 0, 3).or("3", Answer::Number(3));
            d.prolog = number(self.ask(c, &q)?);
        }
        let yes = |q: &str, dflt: bool| {
            Q::new(q, Takes::YesNo).or(if dflt { "Yes" } else { "No" }, Answer::Yes(dflt))
        };
        d.keys[k].dkc =
            self.ask(c, &yes("Data Key Compression desired", true))? == Answer::Yes(true);
        if k == 0 {
            d.keys[k].drc =
                self.ask(c, &yes("Data Record Compression desired", true))? == Answer::Yes(true);
        }
        d.keys[k].ic = self.ask(c, &yes("Index Compression desired", false))? == Answer::Yes(true);
        Some(())
    }

    /// The record format and sizes.
    fn record(&self, c: &mut impl Console, d: &mut Design) -> Option<()> {
        let q = Q::new(
            "Record Format",
            Takes::Keyword {
                words: &["FIXED", "VARIABLE"],
                list: "\t(Fixed Variable)\n",
            },
        )
        .or("Var", Answer::Word("VARIABLE"));
        d.format = word(self.ask(c, &q)?);
        if d.format == "FIXED" {
            d.mean = number(self.ask(c, &Q::number("Record Size", 1, TOP))?);
            d.max = d.mean;
        } else {
            d.mean = number(self.ask(c, &Q::number("Mean Record Size", 1, TOP))?);
            let q = Q::new(
                "Maximum Record Size",
                Takes::Number {
                    lo: 0,
                    hi: TOP,
                    shown: format!("(0,{}-{TOP})", d.mean),
                },
            )
            .or("0", Answer::Number(0));
            d.max = number(self.ask(c, &q)?);
        }
        Some(())
    }

    /// The key's length and position, or its segments'.
    fn segments(
        &self,
        c: &mut impl Console,
        d: &mut Design,
        k: usize,
        segmented: bool,
    ) -> Option<()> {
        let top = if d.format == "FIXED" { d.mean } else { TOP };
        let explain = "\tThis is the length of the key (segment) in bytes.\n\t(With multi-segment keys, answer \"0\" after the last segment.)\n";
        let mut lens = Vec::new();
        loop {
            let n = lens.len();
            let text = match segmented {
                true => format!("Key {k:>2} Length\t\tSEG{n}"),
                false => format!("Key {k:>2} Length"),
            };
            // A 0 ends the segments, though the range shown starts at 1.
            let hi = top.min(255);
            let lo = u64::from(n == 0 || !segmented);
            let q = Q::new(
                &text,
                Takes::Number {
                    lo,
                    hi,
                    shown: format!("(1-{hi})"),
                },
            )
            .explained(explain);
            let len = number(self.ask(c, &q)?);
            if len == 0 {
                break;
            }
            lens.push(len);
            if !segmented || lens.len() == 8 {
                break;
            }
        }
        // INDEXED puts a key after the one before it.
        let mut at = match (d.script, k) {
            ("INDEXED", 1..) => d.keys[k - 1].segs.last().map_or(0, |s| s.0 + s.1),
            _ => 0,
        };
        let mut segs = Vec::new();
        for (n, len) in lens.iter().enumerate() {
            let text = match segmented {
                true => format!("Key {k:>2} Position\t\tSEG{n}"),
                false => format!("Key {k:>2} Position"),
            };
            let q = Q::number(&text, 0, top.saturating_sub(*len))
                .or(&at.to_string(), Answer::Number(at));
            let pos = number(self.ask(c, &q)?);
            segs.push((*len, pos));
            at = pos + len;
        }
        d.keys[k].segs = segs;
        Some(())
    }

    /// The design screen: the plot and the mnemonics, until FD.
    fn screen(&self, c: &mut impl Console, d: &mut Design, k: usize) -> Option<()> {
        let mut lead = "\n\n";
        loop {
            c.say(lead);
            c.say(&self.picture(d, k));
            let mut q = Q::new(
                "Which File Parameter\t",
                Takes::Other {
                    shown: "(Mnemonic)",
                    list: "\t(Type \"FD\" to Finish Design)\n",
                },
            )
            .or("refresh", Answer::Word(""))
            .explained("\tType the 2 letter mnemonic of the selected option.\n");
            q.lead = false;
            q.sep = Some("\t: ");
            const NAMES: [&str; 16] = [
                "FD", "PV", "KT", "EM", "DK", "KL", "KP", "RC", "KC", "IC", "BF", "RF", "RS", "LM",
                "IL", "AR",
            ];
            let m = self.ask_as(c, &q, |t| match t {
                None => Ok(""),
                Some(t) => {
                    let w = token(t).to_ascii_uppercase();
                    NAMES
                        .iter()
                        .find(|n| **n == w)
                        .copied()
                        .ok_or_else(|| syntax(&w))
                }
            })?;
            lead = "\n";
            let key = &mut d.keys[k];
            let yes = |a: Answer| a == Answer::Yes(true);
            match m {
                "FD" => return Some(()),
                "" => {}
                "PV" => {
                    let q = Q::number("File Prolog Version", 0, 3).or("3", Answer::Number(3));
                    d.prolog = number(self.ask(c, &q)?);
                }
                "KT" => {
                    let q = Q::new(
                        &format!("Key {k:>2} Data Type"),
                        Takes::Keyword {
                            words: &TYPES,
                            list: TYPE_LIST,
                        },
                    )
                    .or("Str", Answer::Word("STRING"));
                    key.ktype = word(self.ask(c, &q)?);
                }
                "EM" => d.emphasis = self.emphasis(c)?,
                "DK" => {
                    let q = Q::new(&format!("Key {k:>2} Duplicates allowed"), Takes::YesNo)
                        .or("No", Answer::Yes(false));
                    key.dups = yes(self.ask(c, &q)?);
                }
                "KL" | "KP" => {
                    let top = if d.format == "FIXED" { d.mean } else { TOP };
                    let (len, pos) = key.segs.first().copied().unwrap_or((1, 0));
                    let n = if m == "KL" {
                        let q = Q::number(&format!("Key {k:>2} Length"), 1, top.min(255));
                        number(self.ask(c, &q)?)
                    } else {
                        let q = Q::number(&format!("Key {k:>2} Position"), 0, top - len)
                            .or(&pos.to_string(), Answer::Number(pos));
                        number(self.ask(c, &q)?)
                    };
                    key.segs = vec![if m == "KL" { (n, pos) } else { (len, n) }];
                }
                "RC" | "KC" | "IC" => {
                    let (text, dflt) = match m {
                        "RC" => ("Data Record Compression desired", key.drc),
                        "KC" => ("Data Key Compression desired", key.dkc),
                        _ => ("Index Compression desired", key.ic),
                    };
                    let q = Q::new(text, Takes::YesNo)
                        .or(if dflt { "Yes" } else { "No" }, Answer::Yes(dflt));
                    let a = yes(self.ask(c, &q)?);
                    match m {
                        "RC" => key.drc = a,
                        "KC" => key.dkc = a,
                        _ => key.ic = a,
                    }
                }
                "BF" => self.fill(c, k, d)?,
                "RF" | "RS" => self.record(c, d)?,
                "LM" => d.method = self.method(c, "Initial File Load Method")?,
                "IL" => {
                    let q = Q::number(
                        "Number of Records that will be Initially Loaded\ninto the File",
                        0,
                        2147483647,
                    );
                    d.load = number(self.ask(c, &q)?);
                }
                _ => d.added = self.added(c, d.load, "the Initial File Load")?,
            }
        }
    }

    /// The plot and the mnemonics' table.
    /// ponytail: the KEY, RECORD, INIT and ADD surfaces are drawn as FILL's
    /// is (bounds 50-100 of their quantity); only FILL's was recorded.
    fn picture(&self, d: &Design, k: usize) -> String {
        let figures = |d: &Design| self.figures(d, k);
        let mut out = String::new();
        let line = d.graph == "LINE" || d.graph.is_empty();
        if line {
            let f = figures(d);
            let depths: [Option<u32>; 32] =
                std::array::from_fn(|b| f.and_then(|f| f.depth(b as u32 + 1)));
            out += &plot(&depths);
        } else {
            out += "\tWorking ...\n";
            let (lo, hi) = d.bounds;
            let rows: Vec<_> = (0..13)
                .map(|i| {
                    let v = hi.saturating_sub(i * (hi - lo) / 10).max(1);
                    let mut e = d.clone();
                    match d.graph {
                        "FILL" => e.keys[k].fill = v,
                        "KEY" => e.keys[k].segs = vec![(v, 0)],
                        "RECORD" => e.mean = v,
                        "INIT" => e.load = v,
                        _ => e.added = v,
                    }
                    let f = figures(&e);
                    let depths = std::array::from_fn(|b| f.and_then(|f| f.depth(b as u32 + 1)));
                    let (flat, third) = f.map_or((0, 0), |f| f.marks());
                    (v, depths, flat, third)
                })
                .collect();
            let names: &[(usize, &str)] = match d.graph {
                "FILL" => &[(3, "Initial"), (5, "Load"), (7, "Fill"), (9, "Percent")],
                "KEY" => &[(5, "Key"), (7, "Length")],
                "RECORD" => &[(5, "Record"), (7, "Size")],
                "INIT" => &[(3, "Initial"), (5, "Load"), (7, "Record"), (9, "Count")],
                _ => &[(3, "Added"), (5, "Record"), (7, "Count")],
            };
            out += &surface(&rows, names);
        }
        out += "\n\n\n";
        let key = &d.keys[k];
        let f = figures(d);
        let flat = f.map_or(0, |f| f.flatter(d.cluster));
        let emphasis = match line {
            true => format!(
                "  {} ({flat:>2})",
                if d.emphasis == "FLATTER_FILES" {
                    "Flatter"
                } else {
                    "Smaller"
                }
            ),
            false => "Flatter".into(),
        };
        let cell = |label: &str, v: &str| format!(" {label}{v:>w$}", w = 25 - label.len());
        let (kc, rc, ic) = self.compressions(d, k);
        let rs = match d.format {
            "FIXED" => cell("RS-Record Size", &d.mean.to_string()),
            _ => cell("RS-Mean Record Size", &d.mean.to_string()),
        };
        let rows = [
            vec![
                cell("PV-Prolog Version", &d.prolog.to_string()),
                cell(&format!("KT-Key {k:>2} Type"), &shown(key.ktype)),
                format!(" {}", cell("EM-Emphasis", &emphasis)),
            ],
            vec![
                cell(
                    &format!("DK-Dup Key {k:>2} Values"),
                    if key.dups { "Yes" } else { "No" },
                ),
                cell(&format!("KL-Key {k:>2} Length"), &key.length().to_string()),
                cell(
                    &format!("KP-Key {k:>2} Position"),
                    &key.segs.first().map_or(0, |s| s.1).to_string(),
                ),
            ],
            vec![
                cell("RC-Data Record Comp", &format!("{rc}%")),
                cell("KC-Data Key Comp", &format!("{kc}%")),
                cell("IC-Index Record Comp", &format!("{ic}%")),
            ],
            [
                (d.graph != "FILL").then(|| cell("BF-Bucket Fill", &format!("{}%", key.fill))),
                Some(cell("RF-Record Format", &shown(d.format))),
                Some(rs),
            ]
            .into_iter()
            .flatten()
            .collect(),
            vec![
                cell("LM-Load Method", &shown(d.method)[..9.min(d.method.len())]),
                cell("IL-Initial Load", &self.count(d, k).to_string()),
                cell("AR-Added Records", &d.added.to_string()),
            ],
        ];
        for (i, r) in rows.iter().enumerate() {
            out += &r.concat();
            out += if i == 0 { "\n" } else { " \n" };
        }
        out
    }

    /// The analysis's compression percentages for key `k` (none for a
    /// new design): key, record, index.
    fn compressions(&self, d: &Design, k: usize) -> (u64, u64, u64) {
        if d.script != "OPTIMIZE" {
            return (0, 0, 0);
        }
        let stat = |name: &str| {
            self.analysis
                .as_ref()
                .and_then(|a| a.section("ANALYSIS_OF_KEY", &k.to_string()))
                .and_then(|s| s.get(name))
                .and_then(|v| v.trim().parse::<i64>().ok())
                .filter(|v| (1..100).contains(v))
                .unwrap_or(0) as u64
        };
        let rc = if k == 0 {
            stat("DATA_RECORD_COMPRESSION")
        } else {
            0
        };
        (stat("DATA_KEY_COMPRESSION"), rc, stat("INDEX_COMPRESSION"))
    }

    /// The entries key `k` has: the records, or for OPTIMIZE's alternate
    /// keys their SIDRs as analysed.
    fn count(&self, d: &Design, k: usize) -> u64 {
        match (d.script, k) {
            ("OPTIMIZE", 1..) => self
                .analysis
                .as_ref()
                .and_then(|a| a.section("ANALYSIS_OF_KEY", &k.to_string()))
                .and_then(|s| s.get("DATA_RECORD_COUNT"))
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(0),
            _ => d.load,
        }
    }

    /// The definition and the analysis a design amounts to so far.
    fn design_files(&self, d: &Design) -> (Fdl, Fdl) {
        let mut input = Fdl::default();
        let mut file = Section::new("FILE", "");
        file.push("ORGANIZATION", "indexed");
        let mut record = Section::new("RECORD", "");
        record.push("FORMAT", d.format.to_ascii_lowercase());
        record.push("SIZE", d.max);
        input.sections.extend([file, record]);
        let mut analysis = Fdl::default();
        let mut file = Section::new("FILE", "");
        file.push("CLUSTER_SIZE", d.cluster);
        file.push("ORGANIZATION", "indexed");
        analysis.sections.push(file);
        let given = self.analysis.as_ref().filter(|_| d.script == "OPTIMIZE");
        for (k, key) in d.keys.iter().enumerate() {
            input.sections.push(key_section(d, k, key));
            let mut stats = given
                .and_then(|a| a.section("ANALYSIS_OF_KEY", &k.to_string()))
                .cloned()
                .unwrap_or_else(|| {
                    let mut s = Section::new("ANALYSIS_OF_KEY", k);
                    s.push("MEAN_DATA_LENGTH", d.mean);
                    s
                });
            if k == 0 || d.script != "OPTIMIZE" {
                stats.set("DATA_RECORD_COUNT", (d.load + d.added).max(1));
            }
            analysis.sections.push(stats);
        }
        (input, analysis)
    }

    /// Key `k`'s figures as designed so far: no key's buckets smaller
    /// than the records' (the optimizer lets an alternate key's be).
    fn figures(&self, d: &Design, k: usize) -> Option<edf::Key> {
        let (input, analysis) = self.design_files(d);
        let (_, keys) = edf::keys(&input, &analysis).ok()?;
        let mut key = *keys.get(k)?;
        let var = d.format != "FIXED";
        key.bmin = key.bmin.max(edf::min_bucket(d.mean.max(d.max) as u32, var));
        // ADD_KEY's and TOUCHUP's key is fitted to the clusters, as the
        // primary key is (their recorded suggestions say so).
        key.adjust |= matches!(d.script, "ADD_KEY" | "TOUCHUP");
        Some(key)
    }

    /// After FD, for the first key: the title, the data file, its
    /// carriage control.
    fn file_questions(&self, c: &mut impl Console) -> Option<(String, String, &'static str)> {
        let none = || Answer::Text(String::new());
        let text = |a: Answer| match a {
            Answer::Text(t) => t,
            _ => String::new(),
        };
        let q = Q::new("Text for FDL Title Section", Takes::Text { max: 126 }).or("null", none());
        let title = text(self.ask(c, &q)?);
        let q = Q::new("Data File file-spec", Takes::Text { max: 512 }).or("null", none());
        let file = text(self.ask(c, &q)?);
        let q = Q::new(
            "Carriage Control",
            Takes::Keyword {
                words: &["CARRIAGE_RETURN", "FORTRAN", "NONE"],
                list: "\t(Carriage_Return FORTRAN None)\n",
            },
        )
        .or("Carr", Answer::Word("CARRIAGE_RETURN"));
        Some((title, file, word(self.ask(c, &q)?)))
    }

    /// After FD: the summary of the suggested bucket sizes, the one to
    /// use, the key's name.
    fn finish_key(&self, c: &mut impl Console, d: &mut Design, k: usize) -> Option<()> {
        if d.graph == "FILL" {
            self.fill(c, k, d)?;
        }
        let f = self.figures(d, k)?;
        let s = f.suggestions(d.cluster);
        let col = |g: &dyn Fn(u32) -> u32| -> String {
            format!("({:>8}{:>7}{:>7} )", g(s[0]), g(s[1]), g(s[2]))
        };
        let index = |b: u32| f.index(b).unwrap_or((0, 0));
        let smaller = d.emphasis == "SMALLER_BUFFERS";
        let emphasis = if smaller {
            "    Smaller_buffers    "
        } else {
            "     Flatter_files     "
        };
        let mut out = format!(
            "\n\t{:<40}({emphasis})\n",
            "Emphasis Used In Defining Default:"
        );
        for (label, v) in [
            ("Suggested Bucket Sizes:", col(&|b| b)),
            ("Number of Levels in Index:", col(&|b| index(b).0)),
            ("Number of Buckets in Index:", col(&|b| index(b).1)),
            ("Pages Required to Cache Index:", col(&|b| index(b).1 * b)),
            (
                "Processing Used to Search Index:",
                col(&|b| f.processing(b)),
            ),
        ] {
            out += &format!("\t{label:<40}{v}\n");
        }
        c.say(&out);
        // The smallest bucket holding two index entries.
        let lo = (1..=63)
            .find(|&b| (512 * b - 17) / (f.klen + 4) >= 2)
            .unwrap_or(1);
        let dflt = s[if smaller { 0 } else { 1 }];
        let q = Q::number(&format!("Key {k:>2} Bucket Size"), lo.into(), 63)
            .or(&dflt.to_string(), Answer::Number(dflt.into()));
        d.keys[k].bucket = number(self.ask(c, &q)?) as u32;
        if k > 0 && d.script != "OPTIMIZE" {
            let q = Q::new(&format!("Key {k:>2} Changes allowed"), Takes::YesNo)
                .or("No", Answer::Yes(false));
            d.keys[k].changes = self.ask(c, &q)? == Answer::Yes(true);
        }
        let q = Q::new(&format!("Key {k:>2} Name"), Takes::Text { max: 32 })
            .or("null", Answer::Text(String::new()));
        if let Answer::Text(t) = self.ask(c, &q)? {
            d.keys[k].name = t;
        }
        Some(())
    }

    /// INDEXED's global buffer questions: the V8.3 flags, if given.
    /// ponytail: a Yes to either count question isn't followed by the
    /// count's own question, which wasn't recorded.
    fn buffers(&self, c: &mut impl Console) -> Option<Option<String>> {
        let no = |q: &str| Q::new(q, Takes::YesNo).or("No", Answer::Yes(false));
        self.ask(c, &no("Global Buffers desired for pre-V8.3 system\t\t"))?;
        let q = Q::new(
            "Global Buffer flags value for V8.3+\t\t",
            Takes::Other {
                shown: "(default/percent)",
                list: "",
            },
        )
        .or("none", Answer::Word(""));
        let flags = self.ask_as(c, &q, |t| match t {
            None => Ok(None),
            Some(t) => match keyword(token(t), &["DEFAULT", "PERCENT", "NONE"]) {
                Some("NONE") => Ok(None),
                Some(w) => Ok(Some(w.to_ascii_lowercase())),
                None => Err(syntax(token(t))),
            },
        })?;
        self.ask(c, &no("Global Buffers desired for V8.3+ system\t\t\t"))?;
        Some(flags)
    }

    fn depth_message(&self, c: &mut impl Console, d: &Design, k: usize) -> Option<()> {
        let levels = self
            .figures(d, k)
            .and_then(|f| f.depth(d.keys[k].bucket))
            .unwrap_or(1);
        c.say(&format!(
            "\n\n\tThe Depth of Key {k:>2} is Estimated to be No Greater\n\tthan {levels} Index levels, which is {} Total levels.\n",
            levels + 1
        ));
        self.press_return(c, RETURN)
    }

    /// The whole design: what the optimizer makes of the answers.
    fn design_fdl(
        &self,
        d: &Design,
        title: &str,
        file: &str,
        cc: &str,
        flags: Option<String>,
    ) -> Option<Fdl> {
        let (mut input, analysis) = self.design_files(d);
        if d.script == "OPTIMIZE" {
            // The definition's own sections, its keys as designed.
            let mut kept = self.fdl.clone();
            kept.sections
                .retain(|s| !matches!(s.name.as_str(), "KEY" | "AREA" | "IDENT"));
            kept.sections
                .extend(input.sections.into_iter().filter(|s| s.name == "KEY"));
            input = kept;
        }
        if let Some(r) = input.sections.iter_mut().find(|s| s.name == "RECORD") {
            r.set("CARRIAGE_CONTROL", cc.to_ascii_lowercase());
        }
        if !file.is_empty()
            && let Some(f) = input.sections.iter_mut().find(|s| s.name == "FILE")
        {
            f.set("NAME", format!("\"{file}\""));
        }
        let buckets: Vec<u32> = d.keys.iter().map(|k| k.bucket).collect();
        let edf::Outcome::Fdl(mut f) =
            edf::optimize_with(&input, &analysis, self.areas(), "", &buckets)
        else {
            return None;
        };
        if !title.is_empty() {
            f.sections
                .insert(0, Section::new("TITLE", format!("\"{title}\"")));
        }
        if let Some(flags) = flags
            && let Some(s) = f.sections.iter_mut().find(|s| s.name == "FILE")
        {
            s.set("GLBUFF_FLAGS_V83", flags);
        }
        Some(f)
    }

    /// SET GRANULARITY as the optimizer's number of areas.
    /// ponytail: DOUBLE (two areas a key) is taken as FOUR.
    fn areas(&self) -> u8 {
        match self.granularity {
            "ONE" => 1,
            "TWO" => 2,
            "THREE" => 3,
            _ => 4,
        }
    }

    /// ADD_KEY's and TOUCHUP's result: the definition with key `k` as
    /// designed, its area sized.
    /// ponytail: VMS also leaves an AREA 1 of BUCKET_SIZE 12 behind (see
    /// fixtures/edf/recorded/others.log); that isn't copied.
    fn key_into(&self, d: &Design, k: usize) -> Fdl {
        let mut f = self.fdl.clone();
        let key = &d.keys[k];
        let area = if k == 0 { 0 } else { 2 };
        let mut s = key_section(d, k, key);
        s.set("DATA_AREA", area);
        s.set("INDEX_AREA", if k == 0 { 1 } else { area });
        s.set("LEVEL1_INDEX_AREA", if k == 0 { 1 } else { area });
        s.set("INDEX_FILL", key.fill);
        if k > 0 {
            s.attrs.retain(|(n, _)| n != "PROLOG");
        }
        s.attrs.sort();
        f.sections
            .retain(|x| !(x.name == "KEY" && x.value.trim() == k.to_string()));
        f.sections.push(s);
        if let Some(fig) = self.figures(d, k) {
            let (alloc, ext) = fig.space(key.bucket, d.cluster);
            let mut a = Section::new("AREA", area);
            a.push("ALLOCATION", alloc);
            if d.script == "TOUCHUP" {
                a.push("BEST_TRY_CONTIGUOUS", "yes");
            }
            a.push("BUCKET_SIZE", key.bucket);
            a.push("EXTENSION", ext);
            f.sections
                .retain(|x| !(x.name == "AREA" && x.value.trim() == area.to_string()));
            f.sections.push(a);
        }
        super::normalized(f)
    }
}

/// A key's section as a design writes it.
fn key_section(d: &Design, k: usize, key: &KeyDesign) -> Section {
    let yn = |b: bool| if b { "yes" } else { "no" };
    let mut s = Section::new("KEY", k);
    s.push("CHANGES", yn(key.changes));
    s.push("DATA_FILL", key.fill);
    s.push("DATA_KEY_COMPRESSION", yn(key.dkc));
    if k == 0 {
        s.push("DATA_RECORD_COMPRESSION", yn(key.drc));
    }
    s.push("DUPLICATES", yn(key.dups));
    s.push("INDEX_COMPRESSION", yn(key.ic));
    if !key.name.is_empty() {
        s.push("NAME", format!("\"{}\"", key.name));
    }
    if k == 0 {
        s.push("PROLOG", d.prolog);
    }
    for (n, (len, pos)) in key.segs.iter().enumerate() {
        s.push(&format!("SEG{n}_LENGTH"), len);
        s.push(&format!("SEG{n}_POSITION"), pos);
    }
    s.push("TYPE", key.ktype.to_ascii_lowercase());
    s
}

/// What a definition's key section tells a design.
fn from_section(k: usize, s: &Section) -> KeyDesign {
    let yes = |n: &str, d: bool| s.get(n).map_or(d, |v| v.eq_ignore_ascii_case("yes"));
    let num = |n: &str| s.get(n).and_then(|v| v.trim().parse::<u64>().ok());
    let segs = (0..8)
        .map_while(|n| {
            Some((
                num(&format!("SEG{n}_LENGTH"))?,
                num(&format!("SEG{n}_POSITION")).unwrap_or(0),
            ))
        })
        .collect();
    let mut key = KeyDesign::new(k);
    key.segs = segs;
    key.dups = yes("DUPLICATES", k > 0);
    key.changes = yes("CHANGES", false);
    key.fill = num("DATA_FILL").unwrap_or(100);
    key
}
