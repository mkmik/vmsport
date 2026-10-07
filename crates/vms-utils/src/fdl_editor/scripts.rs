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

const RETURN: &str = "\t Press RETURN to continue (^Z for Main Menu)    ";

impl Editor<'_> {
    /// INVOKE: which script, then it.
    pub(super) fn invoke(&mut self, c: &mut impl Console) {
        let q = Q::new(
            "Editing Script Title",
            Takes::Keyword {
                words: &SCRIPTS,
                list: "\t(Add_Key Delete_Key Indexed Optimize\n\t Relative Sequential Touchup)\n",
            },
        );
        let Some(Answer::Word(script)) = self.ask(c, &q) else {
            return;
        };
        let replaces = matches!(script, "SEQUENTIAL" | "RELATIVE" | "INDEXED");
        if replaces && !self.empty() {
            c.say("\n\t The Current Definition will be replaced. \n\n\n");
            if self.press_return(c, RETURN).is_none() {
                return;
            }
        }
        self.script(c, script);
    }

    /// Runs `script`; what it asked so far goes if it is left with Ctrl/Z.
    pub(super) fn script(&mut self, c: &mut impl Console, script: &str) {
        let designed = match script {
            "SEQUENTIAL" => self.sequential(c),
            "RELATIVE" => self.relative(c),
            _ => None,
        };
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
        let capacity = self.ask(
            c,
            &Q::number("File Capacity in Records", 0, 2147483647)
                .explained("\tThis will determine the allocation of the file.\n"),
        )?;
        let format = if relative {
            Q::new(
                "Record Format",
                Takes::Keyword {
                    words: &["FIXED", "VARIABLE", "VFC"],
                    list: "\t(Fixed Variable VFC)\n",
                },
            )
        } else {
            Q::new(
                "Record Format",
                Takes::Keyword {
                    words: &[
                        "FIXED",
                        "STREAM",
                        "STREAM_CR",
                        "STREAM_LF",
                        "_CR",
                        "_LF",
                        "UNDEFINED",
                        "VARIABLE",
                        "VFC",
                    ],
                    list: "\t(Fixed Stream _CR _LF Undefined Variable VFC)\n",
                },
            )
        };
        let format = match self.ask(c, &format.or("Var", Answer::Word("VARIABLE")))? {
            Answer::Word("_CR") => "STREAM_CR",
            Answer::Word("_LF") => "STREAM_LF",
            Answer::Word(w) => w,
            _ => unreachable!(),
        };
        let span = match relative {
            true => true,
            false => {
                let q = Q::new("Records can span disk blocks", Takes::YesNo)
                    .or("Yes", Answer::Yes(true));
                self.ask(c, &q)? == Answer::Yes(true)
            }
        };
        let top = if relative { 32253 } else { 32767 };
        let Answer::Number(mean) = self.ask(c, &Q::number("Mean Record Size", 1, top))? else {
            unreachable!()
        };
        let max = if relative {
            let q = Q::new(
                "Maximum Record Size",
                Takes::Number {
                    lo: mean,
                    hi: top,
                    shown: format!("({mean}-{top})"),
                },
            );
            self.ask(c, &q)?
        } else {
            let mut q = Q::new(
                "Maximum Record Size",
                Takes::Number {
                    lo: 0,
                    hi: top,
                    shown: format!("(0,{mean}-{top})"),
                },
            )
            .or("0", Answer::Number(0));
            q.sep = " : ";
            self.ask(c, &q)?
        };
        let Answer::Number(max) = max else {
            unreachable!()
        };
        let title = self.ask(
            c,
            &Q::new("Text for FDL Title Section", Takes::Text { max: 126 })
                .or("null", Answer::Text(String::new())),
        )?;
        let name = self.ask(
            c,
            &Q::new("Data File file-spec", Takes::Text { max: 512 })
                .or("null", Answer::Text(String::new())),
        )?;
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
        let text = |a: Answer| match a {
            Answer::Text(t) => t,
            _ => String::new(),
        };
        let Answer::Number(capacity) = capacity else {
            unreachable!()
        };
        Some(Records {
            capacity,
            format,
            span,
            mean,
            max,
            title: text(title),
            name: text(name),
            cc,
        })
    }

    fn sequential(&mut self, c: &mut impl Console) -> Option<Fdl> {
        let r = self.records(c, false)?;
        let (alloc, ext) =
            vms_rms::edf::sequential_space(r.capacity, &r.format.to_ascii_lowercase(), r.mean);
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
        record.push("CARRIAGE_CONTROL", r.cc.to_ascii_lowercase());
        record.push("FORMAT", r.format.to_ascii_lowercase());
        record.push("SIZE", r.max);
        Some(design(&r, file, record))
    }

    fn relative(&mut self, c: &mut impl Console) -> Option<Fdl> {
        let r = self.records(c, true)?;
        let q =
            Q::number("Target disk volume Cluster Size", 1, 2147483647).or("3", Answer::Number(3));
        let Answer::Number(cluster) = self.ask(c, &q)? else {
            unreachable!()
        };
        let (bks, alloc, ext) = vms_rms::edf::relative_space(
            r.capacity,
            &r.format.to_ascii_lowercase(),
            r.max,
            cluster as u32,
        );
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
        let mut record = Section::new("RECORD", "");
        record.push("CARRIAGE_CONTROL", r.cc.to_ascii_lowercase());
        record.push("FORMAT", r.format.to_ascii_lowercase());
        record.push("SIZE", r.max);
        Some(design(&r, file, record))
    }
}

/// What the sequential and relative scripts were told.
struct Records {
    capacity: u64,
    format: &'static str,
    span: bool,
    mean: u64,
    max: u64,
    title: String,
    name: String,
    cc: &'static str,
}

/// A designed definition: TITLE, SYSTEM, FILE, RECORD.
fn design(r: &Records, file: Section, record: Section) -> Fdl {
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
