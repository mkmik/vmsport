//! The FDL editor's scripts: INVOKE, /SCRIPT, and what each asks and
//! designs.

use super::{Answer, Console, Editor, Q, Takes};
use vms_rms::fdl::{Fdl, Section};

pub const SCRIPTS: [&str; 7] = [
    "ADD_KEY",
    "DELETE_KEY",
    "INDEXED",
    "OPTIMIZE",
    "RELATIVE",
    "SEQUENTIAL",
    "TOUCHUP",
];

/// A script's name as its title shows it: `Add_Key`.
pub fn title(script: &str) -> String {
    script
        .split('_')
        .map(|w| w[..1].to_string() + &w[1..].to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join("_")
}

/// A key's data types, and how the questions list them.
pub const TYPES: [&str; 18] = [
    "BIN2",
    "BIN4",
    "BIN8",
    "INT2",
    "INT4",
    "INT8",
    "DECIMAL",
    "STRING",
    "COLLATED",
    "DBIN2",
    "DBIN4",
    "DBIN8",
    "DINT2",
    "DINT4",
    "DINT8",
    "DDECIMAL",
    "DSTRING",
    "DCOLLATED",
];
pub const TYPE_LIST: &str = "\t(Bin2  Bin4  Bin8  Int2  Int4  Int8  Decimal  String  Collated\n\t Dbin2 Dbin4 Dbin8 Dint2 Dint4 Dint8 Ddecimal Dstring Dcollated)\n";

const RETURN: &str = "\t Press RETURN to continue (^Z for Main Menu)    ";

impl Editor<'_> {
    /// INVOKE: which script, then it.
    pub(super) fn invoke(&mut self, c: &mut impl Console) {
        let mut table = String::from("\t\t\t Script Title Selection \n\n");
        for (name, what) in [
            (
                "Add_Key",
                "modeling and addition of a new index's parameters",
            ),
            ("Delete_Key", "removal of the highest index's parameters"),
            (
                "Indexed",
                "modeling of parameters for an entire Indexed file",
            ),
            (
                "Optimize",
                "tuning of all indices' parameters using file statistics",
            ),
            ("Relative", "selection of parameters for a Relative file"),
            (
                "Sequential",
                "selection of parameters for a Sequential file",
            ),
            ("Touchup", "remodeling of parameters for a particular index"),
        ] {
            table += &format!("\t{name:<16}{what}\n");
        }
        let q = Q::new(
            "Editing Script Title",
            Takes::Keyword {
                words: &SCRIPTS,
                list: "\t(Add_Key Delete_Key Indexed Optimize\n\t Relative Sequential Touchup)\n",
            },
        )
        .tabled(table + "\n");
        let Some(Answer::Word(script)) = self.ask(c, &q) else {
            return;
        };
        let replaces = matches!(script, "SEQUENTIAL" | "RELATIVE" | "INDEXED");
        if replaces && !self.empty() {
            c.say("\n\t The Current Definition will be replaced. \n\n");
            if self.press_return(c, RETURN).is_none() {
                return;
            }
        }
        self.script(c, script);
    }

    /// Runs `script`; what it asked so far goes if it is left with Ctrl/Z.
    pub(super) fn script(&mut self, c: &mut impl Console, script: &str) {
        self.scripting = true;
        let designed = match script {
            "SEQUENTIAL" => self.sequential(c),
            "RELATIVE" => self.relative(c),
            _ => None,
        };
        self.scripting = false;
        if let Some(f) = designed {
            self.fdl = f;
            c.say("\n");
        }
    }

    pub(super) fn empty(&self) -> bool {
        self.fdl.sections.iter().all(|s| s.name == "IDENT")
    }

    /// What sequential and relative files are asked: the capacity, the
    /// record format and sizes, title, file name, carriage control.
    fn records(&self, c: &mut impl Console, relative: bool) -> Option<Records> {
        let number = |a: Answer| match a {
            Answer::Number(n) => n,
            _ => 0,
        };
        let capacity = number(
            self.ask(
                c,
                &Q::number("File Capacity in Records", 0, 2147483647)
                    .explained("\tThis will determine the allocation of the file.\n"),
            )?,
        );
        let explain = "\tIndexed files are only Fixed or Variable.\n\tStream format (Seq only) is Stream, Stream_CR, or Stream_LF.\n";
        let format = match relative {
            true => Q::new(
                "Record Format",
                Takes::Keyword {
                    words: &["FIXED", "VARIABLE", "VFC"],
                    list: "\t(Fixed Variable VFC)\n",
                },
            ),
            false => Q::new(
                "Record Format",
                Takes::Keyword {
                    words: &[
                        "FIXED",
                        "STREAM",
                        "STREAM_CR",
                        "STREAM_LF",
                        "UNDEFINED",
                        "VARIABLE",
                        "VFC",
                    ],
                    list: "\t(Fixed Stream _CR _LF Undefined Variable VFC)\n",
                },
            ),
        };
        let Answer::Word(format) = self.ask(
            c,
            &format
                .or("Var", Answer::Word("VARIABLE"))
                .explained(explain),
        )?
        else {
            unreachable!()
        };
        let span = match relative {
            true => true,
            false => {
                let q = Q::new("Records can span disk blocks", Takes::YesNo)
                    .or("Yes", Answer::Yes(true));
                self.ask(c, &q)? == Answer::Yes(true)
            }
        };
        let top = match (relative, span, format) {
            (false, false, _) => 510,
            (false, true, _) => 32767,
            (true, _, "FIXED") => 32255,
            (true, _, _) => 32253,
        };
        let (mean, fsz, max) = match format {
            "FIXED" | "UNDEFINED" => {
                let n = number(self.ask(c, &Q::number("Record Size", 1, top))?);
                (n, 0, n)
            }
            _ => {
                let vfc = format == "VFC";
                let text = if vfc {
                    "Mean Record Size w/fix"
                } else {
                    "Mean Record Size"
                };
                let mean = number(self.ask(c, &Q::number(text, 1, top))?);
                let fsz = match vfc {
                    true => {
                        let q = Q::number("Control Field Size", 1, mean).or("2", Answer::Number(2));
                        number(self.ask(c, &q)?)
                    }
                    false => 0,
                };
                let hi = top - fsz;
                let max = match relative {
                    true => Q::new(
                        "Maximum Record Size",
                        Takes::Number {
                            lo: mean,
                            hi,
                            shown: format!("({mean}-{hi})"),
                        },
                    ),
                    false => Q::new(
                        "Maximum Record Size",
                        Takes::Number {
                            lo: 0,
                            hi,
                            shown: format!("(0,{mean}-{hi})"),
                        },
                    )
                    .or("0", Answer::Number(0)),
                };
                (mean, fsz, number(self.ask(c, &max)?))
            }
        };
        let text = |a: Answer| match a {
            Answer::Text(t) => t,
            _ => String::new(),
        };
        let none = || Answer::Text(String::new());
        let title =
            Q::new("Text for FDL Title Section", Takes::Text { max: 126 }).or("null", none());
        let title = text(self.ask(c, &title)?);
        let name = Q::new("Data File file-spec", Takes::Text { max: 512 }).or("null", none());
        let name = text(self.ask(c, &name)?);
        let cc = Q::new(
            "Carriage Control",
            Takes::Keyword {
                words: &["CARRIAGE_RETURN", "FORTRAN", "NONE", "PRINT"],
                list: "\t(Carriage_Return FORTRAN None Print)\n",
            },
        )
        .or("Carr", Answer::Word("CARRIAGE_RETURN"));
        let Answer::Word(cc) = self.ask(c, &cc)? else {
            unreachable!()
        };
        Some(Records {
            capacity,
            format,
            span,
            mean,
            fsz,
            max,
            title,
            name,
            cc,
        })
    }

    fn sequential(&mut self, c: &mut impl Console) -> Option<Fdl> {
        let r = self.records(c, false)?;
        let (alloc, ext) = vms_rms::edf::sequential_space(r.capacity, r.format, r.mean);
        let mut file = Section::new("FILE", "");
        file.push("ALLOCATION", alloc);
        file.push("BEST_TRY_CONTIGUOUS", "yes");
        file.push("EXTENSION", ext);
        if !r.name.is_empty() {
            file.push("NAME", format!("\"{}\"", r.name));
        }
        file.push("ORGANIZATION", "sequential");
        let mut record = Section::new("RECORD", "");
        record.push("BLOCK_SPAN", if r.span { "yes" } else { "no" });
        Some(design(&r, file, record))
    }

    fn relative(&mut self, c: &mut impl Console) -> Option<Fdl> {
        let r = self.records(c, true)?;
        let q =
            Q::number("Target disk volume Cluster Size", 1, 2147483647).or("3", Answer::Number(3));
        let Answer::Number(cluster) = self.ask(c, &q)? else {
            unreachable!()
        };
        let (bks, alloc, ext) =
            vms_rms::edf::relative_space(r.capacity, r.format, r.max, r.fsz, cluster as u32);
        let mut file = Section::new("FILE", "");
        file.push("ALLOCATION", alloc);
        file.push("BEST_TRY_CONTIGUOUS", "yes");
        file.push("BUCKET_SIZE", bks);
        file.push("EXTENSION", ext);
        file.push("MAX_RECORD_NUMBER", r.capacity);
        if !r.name.is_empty() {
            file.push("NAME", format!("\"{}\"", r.name));
        }
        file.push("ORGANIZATION", "relative");
        Some(design(&r, file, Section::new("RECORD", "")))
    }
}

/// What the sequential and relative scripts were told.
struct Records {
    capacity: u64,
    format: &'static str,
    span: bool,
    mean: u64,
    /// VFC's control field.
    fsz: u64,
    max: u64,
    title: String,
    name: String,
    cc: &'static str,
}

/// A designed definition: TITLE, SYSTEM, FILE, RECORD (`record`'s
/// attributes and the format's).
fn design(r: &Records, file: Section, mut record: Section) -> Fdl {
    record.push("CARRIAGE_CONTROL", r.cc.to_ascii_lowercase());
    if r.format == "VFC" {
        record.push("CONTROL_FIELD_SIZE", r.fsz);
    }
    let format = match r.format {
        "VFC" => "VFC".to_string(),
        f => f.to_ascii_lowercase(),
    };
    record.push("FORMAT", format);
    record.push("SIZE", r.max);
    let mut f = Fdl::default();
    if !r.title.is_empty() {
        f.sections
            .push(Section::new("TITLE", format!("\"{}\"", r.title)));
    }
    let mut system = Section::new("SYSTEM", "");
    system.push("SOURCE", "\"OpenVMS\"");
    f.sections.extend([system, file, record]);
    f
}
