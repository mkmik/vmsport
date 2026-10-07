//! The main menu's ADD, MODIFY, DELETE and SET.

use super::{
    Answer, Console, Editor, Q, Takes, inappropriate, indented, keyword, normalized, syntax, token,
};
use vms_rms::fdl::{Fdl, Section};

/// The prompt after a line is changed.
pub(super) const MENU: &str = "\t Press RETURN or ^Z for Main Menu         ";
const RETURN: &str = "\t Press RETURN to continue (^Z for Main Menu)    ";

/// The primary attributes ADD offers.
const PRIMARIES: [&str; 11] = [
    "ACCESS", "AREA", "CONNECT", "DATE", "FILE", "KEY", "NETWORK", "RECORD", "SHARING", "SYSTEM",
    "TITLE",
];

impl Editor<'_> {
    /// SET: one of the editor's characteristics.
    pub(super) fn set(&mut self, c: &mut impl Console) {
        const WORDS: [&str; 8] = [
            "ANALYSIS",
            "DISPLAY",
            "EMPHASIS",
            "GRANULARITY",
            "NUMBER_KEYS",
            "OUTPUT",
            "PROMPTING",
            "RESPONSES",
        ];
        let mut table = String::from("\t\t\t FDL Editor SET Function \n\n");
        for (name, what) in [
            ("Analysis", "filespec of FDL Analysis file"),
            ("Display", "type of graph to display"),
            ("Emphasis", "of default bucketsize calculations"),
            ("Granularity", "number of areas in Indexed files"),
            ("Number_Keys", "number of keys in Indexed files"),
            ("Output", "filespec of FDL Output file"),
            ("Prompting", "Full or Brief prompting of menus"),
            ("Responses", "usage of default responses in scripts"),
        ] {
            table += &format!("\t{name:<16}{what}\n");
        }
        let q = Q::new(
            "Editor characteristic to set",
            Takes::Other {
                shown: "(keyword)",
                list: "\t(Analysis Display Emphasis Granularity\n\t Number_Keys Output Prompting Responses)\n",
            },
        )
        .tabled(table + "\n");
        let what = self.ask_as(c, &q, |t| {
            let t = token(t.unwrap_or(""));
            keyword(t, &WORDS).ok_or_else(|| syntax(t))
        });
        let done = match what {
            Some("ANALYSIS") => self.set_analysis(c),
            Some("DISPLAY") => self.graph(c, 0).map(|g| self.graph = g),
            Some("EMPHASIS") => self.emphasis(c).map(|e| self.emphasis = e),
            Some("GRANULARITY") => self.granularity(c).map(|g| self.granularity = g),
            Some("NUMBER_KEYS") => {
                let q = Q::number("Number of Keys to Define", 1, 255)
                    .or("1", Answer::Number(1))
                    .explained("\tAn Indexed file can have from 1 to 255 keys.\n");
                self.ask(c, &q).map(|a| {
                    if let Answer::Number(n) = a {
                        self.keys = n;
                    }
                })
            }
            Some("OUTPUT") => {
                let q = Q::new("Output File file-spec", Takes::Text { max: 512 })
                    .or("null", Answer::Text(String::new()));
                self.ask(c, &q).map(|a| {
                    if let Answer::Text(t) = a {
                        self.output = Some(t).filter(|t| !t.is_empty());
                    }
                })
            }
            Some("PROMPTING") => {
                let q = Q::new(
                    "Prompting level for menus",
                    Takes::Keyword {
                        words: &["BRIEF", "FULL"],
                        list: "\t(Brief Full)\n",
                    },
                )
                .or("Full", Answer::Word("FULL"))
                .explained("\tThis controls whether full menus are displayed.\n");
                self.ask(c, &q)
                    .map(|a| self.full = a == Answer::Word("FULL"))
            }
            Some(_) => {
                let q = Q::new(
                    "Default responses in scripts",
                    Takes::Keyword {
                        words: &["AUTOMATIC", "MANUAL"],
                        list: "\t(Automatic Manual)\n",
                    },
                )
                .or("Auto", Answer::Word("AUTOMATIC"))
                .explained(
                    "\tAutomatic means the default answers will be used without\n\twaiting for confirmation.\n",
                );
                self.ask(c, &q)
                    .map(|a| self.automatic = a == Answer::Word("AUTOMATIC"))
            }
            None => None,
        };
        if done.is_some() {
            c.say("\n");
        }
    }

    /// SET ANALYSIS: the analysis file OPTIMIZE uses.
    /// ponytail: a file that can't be read or parsed is ignored; VMS's
    /// messages for it aren't recorded.
    fn set_analysis(&mut self, c: &mut impl Console) -> Option<()> {
        let q = Q::new("Analysis File file-spec\t", Takes::Text { max: 512 })
            .or("null", Answer::Text(String::new()));
        let Answer::Text(spec) = self.ask(c, &q)? else {
            return Some(());
        };
        if !spec.is_empty()
            && let Some(f) = c.read(&spec).and_then(|t| vms_rms::fdl::parse(&t).ok())
        {
            self.analysis = Some(f);
        }
        Some(())
    }

    /// The graph to show for key `key`: all six for the primary key, the
    /// first three for the others.
    pub(super) fn graph(&self, c: &mut impl Console, key: u32) -> Option<&'static str> {
        let rows = [
            (
                "Line",
                "Bucket Size vs Index Depth      as a 2 dimensional plot",
            ),
            (
                "Fill",
                "Bucket Size vs     Load Fill Percent     vs Index Depth",
            ),
            (
                "Key",
                "Bucket Size vs         Key Length        vs Index Depth",
            ),
            (
                "Record",
                "Bucket Size vs        Record Size        vs Index Depth",
            ),
            (
                "Init",
                "Bucket Size vs Initial Load Record Count vs Index Depth",
            ),
            (
                "Add",
                "Bucket Size vs  Additional Record Count  vs Index Depth",
            ),
        ];
        let n = if key == 0 { 6 } else { 3 };
        let mut table = format!("\t\t\t Key {key:>2} Graph Type Selection \n\n");
        for (name, what) in &rows[..n] {
            table += &format!("\t{name:<8}{what}\n");
        }
        let (words, list): (&'static [&'static str], _) = match key {
            0 => (
                &["LINE", "FILL", "KEY", "RECORD", "INIT", "ADD"],
                "\t(Line Fill Key Record Init Add)\n",
            ),
            _ => (&["LINE", "FILL", "KEY"], "\t(Line Fill Key)\n"),
        };
        let q = Q::new("Graph type to display", Takes::Keyword { words, list })
            .or("Line", Answer::Word("LINE"))
            .tabled(table + "\n");
        match self.ask(c, &q)? {
            Answer::Word(w) => Some(w),
            _ => None,
        }
    }

    pub(super) fn emphasis(&self, c: &mut impl Console) -> Option<&'static str> {
        let q = Q::new(
            "Emphasis for Default Bucket_Size",
            Takes::Keyword {
                words: &["SMALLER_BUFFERS", "FLATTER_FILES"],
                list: "\t(Smaller_Buffers Flatter_Files)\n",
            },
        )
        .or("Flat", Answer::Word("FLATTER_FILES"))
        .explained(
            "\tSmaller_Buffers: less memory and RMS processing used\n\tFlatter_Files:   fewer actual disk accesses needed\n",
        );
        match self.ask(c, &q)? {
            Answer::Word(w) => Some(w),
            _ => None,
        }
    }

    fn granularity(&self, c: &mut impl Console) -> Option<&'static str> {
        const WORDS: [&str; 5] = ["ONE", "TWO", "THREE", "FOUR", "DOUBLE"];
        let table = [
            "\t\t\t Area Granularity Selection \n\n",
            "   +-------------+     +-------------+     +-------------+     +-------------+\n",
            " 0 | Key 0 Data  |   0 | Key 0 Data  |   0 | Key 0 Data  |   0 | Key 0 Data  |\n",
            "   |             |     +-------------+     +-------------+     +-------------+\n",
            "   | Key 0 Index |   1 | Key 0 Index |   1 | Key 0 Index |   1 | Key 0 Index |\n",
            "   |             |     |             |     +-------------+     +-------------+\n",
            "   | Key n Data  |     | Key n Data  |   2 | Key n Data  |   2 | Key n Data  |\n",
            "   |             |     |             |     |             |     +-------------+\n",
            "   | Key n Index |     | Key n Index |     | Key n Index |   3 | Key n Index |\n",
            "   +-------------+     +-------------+     +-------------+     +-------------+\n",
            "       One (1)             Two (2)            Three (3)            Four (4)\n\n",
        ]
        .concat();
        let q = Q::new(
            "(Type \"Double\" to allocate 2 areas per key)\nNumber of areas to allocate",
            Takes::Other {
                shown: "(keyword)",
                list: "\t(One Two Three Four Double)\n",
            },
        )
        .or("Three", Answer::Word("THREE"))
        .tabled(table);
        self.ask_as(c, &q, |t| {
            let t = token(t.unwrap_or("Three"));
            keyword(t, &WORDS).ok_or_else(|| syntax(t))
        })
    }

    /// ADD: a line, in a section made for it if there isn't one.
    pub(super) fn add(&mut self, c: &mut impl Console) {
        let mut table = String::from("\t\t\t Legal Primary Attributes \n\n");
        for (name, what) in [
            (
                "ACCESS",
                "attributes set the run-time access mode of the file",
            ),
            (
                "AREA x",
                "attributes define the characteristics of file area x",
            ),
            ("CONNECT", "attributes set various RMS run-time options"),
            ("DATE", "attributes set the date parameters of the file"),
            ("FILE", "attributes affect the entire RMS data file"),
            ("KEY y", "attributes define the characteristics of key y"),
            (
                "NETWORK",
                "attributes set-run time network access parameters",
            ),
            (
                "RECORD",
                "attributes set the non-key aspects of each record",
            ),
            (
                "SHARING",
                "attributes set the run-time sharing mode of the file",
            ),
            (
                "SYSTEM",
                "attributes document operating system-specific items",
            ),
            ("TITLE", "is the header line for the FDL file"),
        ] {
            table += &format!("\t{name:<8}{what}\n");
        }
        let list =
            "\t(ACCESS AREA CONNECT DATE FILE\n\t KEY NETWORK RECORD SHARING SYSTEM TITLE)\n";
        let Some((name, value)) = self.primary(c, list, table + "\n", false) else {
            return;
        };
        // TITLE is a line of its own: only its text is asked.
        if name == "TITLE" {
            let text = self
                .section("TITLE", "")
                .map_or("\"\"", |s| s.value.as_str());
            c.say(&format!("\n\tTITLE\t{text}\n\n"));
            let q = Q::new("Enter value for this Secondary", Takes::Text { max: 126 })
                .or("null", Answer::Text(String::new()));
            let Some(Answer::Text(t)) = self.ask(c, &q) else {
                return;
            };
            self.fdl.sections.retain(|s| s.name != "TITLE");
            self.fdl
                .sections
                .insert(0, Section::new("TITLE", format!("\"{t}\"")));
            self.resulting(c, "TITLE", &format!("\"{t}\""));
            self.done(c);
            return;
        }
        let shown = shown(&name, &value);
        let names: Vec<&str> = legal(&name).iter().map(|(n, _)| *n).collect();
        let q = Q::new(
            &format!("Enter {shown} Attribute"),
            Takes::Other {
                shown: "(Keyword)",
                list: "\t(Type \"?\" for list of Keywords)\n",
            },
        )
        .tabled(legal_table(&name, &shown));
        let Some(attr) = self.ask_as(c, &q, |t| {
            let t = t.unwrap_or("");
            secondary(t, &name, &names).ok_or_else(|| syntax(token(t)))
        }) else {
            return;
        };
        let tabs = "\t".repeat((32 - (16 + attr.len()).min(31)).div_ceil(8));
        c.say(&format!("\n\t{}\n\t\t{attr}{tabs}\n", title(&name, &value)));
        if self
            .section(&name, &value)
            .is_some_and(|s| s.get(&attr).is_some())
        {
            let mut q = Q::new("Replace this existing secondary", Takes::YesNo)
                .or("No", Answer::Yes(false));
            q.lead = false;
            match self.ask(c, &q) {
                None => return,
                Some(Answer::Yes(true)) => c.say("\n"),
                Some(_) => {
                    c.say("\n");
                    self.done(c);
                    return;
                }
            }
        }
        let Some(v) = self.value(c, &name, &attr) else {
            return;
        };
        self.set_attr(&name, &value, &attr, v);
        self.resulting(c, &name, &value);
        self.done(c);
    }

    /// MODIFY: a line's value.
    pub(super) fn modify(&mut self, c: &mut impl Console) {
        let Some((name, value, attr)) = self.existing(c) else {
            return;
        };
        let line = self.line(&name, &value, &attr);
        c.say(&format!("\n{line}\n"));
        let Some(v) = self.value(c, &name, &attr) else {
            return;
        };
        self.set_attr(&name, &value, &attr, v);
        self.resulting(c, &name, &value);
        self.done(c);
    }

    /// DELETE: a line, and its section with its last line.
    pub(super) fn delete(&mut self, c: &mut impl Console) {
        let Some((name, value, attr)) = self.existing(c) else {
            return;
        };
        let line = self.line(&name, &value, &attr);
        c.say(&format!("\n{line}\n"));
        if self.press_return(c, RETURN).is_none() {
            return;
        }
        if let Some(s) = self.section_mut(&name, &value) {
            s.attrs.retain(|(k, _)| *k != attr);
        }
        if self
            .section(&name, &value)
            .is_some_and(|s| s.attrs.is_empty())
        {
            c.say("\n\n\t No more Secondaries with this Primary, deleting Primary. \n");
            self.fdl
                .sections
                .retain(|s| !(s.name == name && s.value.trim() == value));
            self.primary = "FILE".into();
            return;
        }
        self.resulting(c, &name, &value);
        self.done(c);
    }

    /// MODIFY's and DELETE's questions: a primary and a secondary there.
    fn existing(&mut self, c: &mut impl Console) -> Option<(String, String, String)> {
        let mut table = String::from("\t\t\t Current Primary Attributes \n\n");
        for s in self.fdl.sections.iter().filter(|s| s.name != "IDENT") {
            table += &format!("\t{}\n", title(&s.name, s.value.trim()));
        }
        let list = "\t(Type \"?\" for a list of existing Primary Attributes)\n";
        let (name, value) = self.primary(c, list, table + "\n", true)?;
        let section = self.section(&name, &value)?;
        let shown = shown(&name, &value);
        let names: Vec<String> = section.attrs.iter().map(|(k, _)| k.clone()).collect();
        let q = Q::new(
            &format!("Enter {shown} Attribute"),
            Takes::Other {
                shown: "(Keyword)",
                list: "\t(Type \"?\" for list of Keywords)\n",
            },
        )
        .tabled(format!(
            "\t\t\t Current {shown} Secondary Attributes \n\n{}\n",
            indented(&vms_rms::edf::text(&Fdl {
                sections: vec![section.clone()]
            }))
        ));
        let legal: Vec<&str> = legal(&name).iter().map(|(n, _)| *n).collect();
        let legal = &legal;
        let attr = self.ask_as(c, &q, |t| {
            let t = t.unwrap_or("");
            let mine: Vec<&str> = names.iter().map(String::as_str).collect();
            match secondary(t, &name, &mine) {
                Some(a) => Ok(a),
                None if secondary(t, &name, legal).is_some() => Err(inappropriate(token(t))),
                None => Err(syntax(token(t))),
            }
        })?;
        Some((name, value, attr))
    }

    /// Which primary attribute: one there already if `existing`.
    fn primary(
        &mut self,
        c: &mut impl Console,
        list: &'static str,
        table: String,
        existing: bool,
    ) -> Option<(String, String)> {
        let mut again = false;
        loop {
            let q = Q::new(
                "Enter Desired Primary",
                Takes::Other {
                    shown: "(Keyword)",
                    list,
                },
            )
            .or(&self.primary.clone(), Answer::Word(""))
            .tabled(table.clone());
            let typed = std::cell::RefCell::new(None);
            let got = self.ask_once(c, &q, again, &|t| {
                let t = t.unwrap_or(&self.primary);
                let word = token(t);
                let Some(name) = keyword(word, &PRIMARIES) else {
                    return Err(syntax(word));
                };
                let rest = t[word.len()..].trim();
                let value = match name {
                    "AREA" | "KEY" => match token(rest) {
                        n if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => n,
                        n => return Err(syntax(n)),
                    },
                    _ => "",
                };
                *typed.borrow_mut() = Some(format!("{name} {value}").trim().to_string());
                if existing && self.section(name, value).is_none() {
                    return Err(inappropriate(if value.is_empty() { name } else { value }));
                }
                Ok((name.to_string(), value.to_string()))
            });
            if let Some(p) = typed.into_inner() {
                self.primary = p;
            }
            match got {
                None => {
                    self.primary = "FILE".into();
                    return None;
                }
                Some(Ok(p)) => return Some(p),
                Some(Err(())) => again = true,
            }
        }
    }

    /// The value for `attr` of a `name` section.
    fn value(&self, c: &mut impl Console, name: &str, attr: &str) -> Option<String> {
        let kind = legal(name)
            .iter()
            .find(|(n, _)| *n == attr || n.contains("SEGn") && attr.starts_with("SEG"))
            .map_or("string", |(_, k)| k);
        let text = "Enter value for this Secondary";
        let q = match kind {
            _ if name == "DATE" => Q::new(text, Takes::Date),
            "number" => {
                let mut q = Q::number(text, 0, top(name, attr));
                q.sep = Some("\t:  ");
                q
            }
            "yes/no" => Q::new(text, Takes::YesNo),
            "keyword" => {
                let (words, list) = words(attr);
                Q::new(text, Takes::Keyword { words, list })
            }
            _ => Q::new(text, Takes::Text { max: 126 }).or("null", Answer::Text(String::new())),
        };
        let q = q.explained("\tThe value entered will be put into the Definition.\n\n");
        Some(match self.ask(c, &q)? {
            Answer::Number(n) => n.to_string(),
            Answer::Yes(y) => if y { "yes" } else { "no" }.to_string(),
            Answer::Word(w) => w.to_ascii_lowercase(),
            Answer::Text(t) if kind == "string" => format!("\"{t}\""),
            Answer::Text(t) => t,
        })
    }

    fn set_attr(&mut self, name: &str, value: &str, attr: &str, v: String) {
        if self.section(name, value).is_none() {
            self.fdl.sections.push(Section::new(name, value));
        }
        self.section_mut(name, value).unwrap().set(attr, &v);
        self.fdl = normalized(std::mem::take(&mut self.fdl));
    }

    fn section(&self, name: &str, value: &str) -> Option<&Section> {
        self.fdl
            .sections
            .iter()
            .find(|s| s.name == name && s.value.trim() == value)
    }

    fn section_mut(&mut self, name: &str, value: &str) -> Option<&mut Section> {
        self.fdl
            .sections
            .iter_mut()
            .find(|s| s.name == name && s.value.trim() == value)
    }

    /// One line of a section, as VIEW shows it with its section.
    fn line(&self, name: &str, value: &str, attr: &str) -> String {
        let mut s = Section::new(name, value);
        if let Some(v) = self.section(name, value).and_then(|s| s.get(attr)) {
            s.push(attr, v);
        }
        indented(&vms_rms::edf::text(&Fdl { sections: vec![s] }))
    }

    /// The section a line was changed in.
    fn resulting(&self, c: &mut impl Console, name: &str, value: &str) {
        let s = self
            .section(name, value)
            .cloned()
            .unwrap_or_else(|| Section::new(name, value));
        c.say(&format!(
            "\n\t\t\t Resulting Primary Section \n\n{}\n",
            indented(&vms_rms::edf::text(&Fdl { sections: vec![s] }))
        ));
    }

    /// Back to the main menu when Return is pressed.
    /// ponytail: a numbered primary typed here (`KEY 1`) makes the next
    /// default FILE, as one recording had it; why is unknown.
    fn done(&mut self, c: &mut impl Console) {
        loop {
            let Some(typed) = c.ask(&format!("\n{MENU}")) else {
                return;
            };
            let word = token(&typed);
            if word.is_empty() {
                c.say("\n");
                return;
            }
            if matches!(keyword(word, &PRIMARIES), Some("AREA" | "KEY")) {
                self.primary = "FILE".into();
            }
            c.say(&syntax(word));
        }
    }
}

/// A section's line: `KEY 1`.
fn title(name: &str, value: &str) -> String {
    format!("{name} {value}").trim_end().to_string()
}

/// A primary attribute as the questions name it: `KEY  1`, `AREA 1`.
fn shown(name: &str, value: &str) -> String {
    match value.parse::<u32>() {
        Ok(n) if name == "KEY" => format!("{name} {n:>2}"),
        _ => title(name, value),
    }
}

/// The secondary attribute `t` names of those (`names`) a `primary`
/// section takes; a KEY's segments are numbered (`SEG2_LENGTH`).
fn secondary(t: &str, primary: &str, names: &[&str]) -> Option<String> {
    let word = token(t).to_ascii_uppercase();
    if primary == "KEY"
        && let Some((n, what)) = word.strip_prefix("SEG").and_then(|r| r.split_once('_'))
        && n.parse::<u8>().is_ok_and(|n| n < 8)
        && ["LENGTH", "POSITION"].contains(&what)
    {
        return Some(word);
    }
    keyword(&word, names)
        .filter(|n| !n.contains("SEGn"))
        .map(String::from)
}

/// The secondary attributes a primary takes, as ADD's `?` lists them:
/// each with what its value is (`yes/no`, `number`, `string`...).
fn rows(primary: &str) -> &'static [&'static str] {
    match primary {
        "ACCESS" => &[
            "BLOCK_IO                yes/no",
            "DELETE                  yes/no",
            "GET                     yes/no",
            "PUT                     yes/no",
            "RECORD_IO               yes/no",
            "TRUNCATE                yes/no",
            "UPDATE                  yes/no",
        ],
        "AREA" => &[
            "ALLOCATION              number",
            "BEST_TRY_CONTIGUOUS     yes/no",
            "BUCKET_SIZE             number",
            "CONTIGUOUS              yes/no",
            "EXACT_POSITIONING       yes/no",
            "EXTENSION               number",
            "POSITION    qualifier   number",
            "VOLUME                  number",
        ],
        "CONNECT" => &[
            "ASYNCHRONOUS            yes/no  NOLOCK                  yes/no",
            "ACCESS_SEMANTICS        string  NONEXISTENT_RECORD      yes/no",
            "BLOCK_IO                yes/no  READ_AHEAD              yes/no",
            "BUCKET_CODE             number  READ_REGARDLESS         yes/no",
            "CONTEXT                 number  SYNCSTS                 yes/no",
            "END_OF_FILE             yes/no  TIMEOUT_ENABLE          yes/no",
            "FAST_DELETE             yes/no  TIMEOUT_PERIOD          number",
            "FILL_BUCKETS            yes/no  TRUNCATE_ON_PUT         yes/no",
            "KEY_GREATER_EQUAL       yes/no  TT_CANCEL_CONTROL_O     yes/no",
            "KEY_GREATER_THAN        yes/no  TT_PROMPT               yes/no",
            "KEY_LIMIT               yes/no  TT_PURGE_TYPE_AHEAD     yes/no",
            "KEY_OF_REFERENCE        number  TT_READ_NOECHO          yes/no",
            "LOCATE_MODE             yes/no  TT_READ_NOFILTER        yes/no",
            "LOCK_ON_READ            yes/no  TT_UPCASE_INPUT         yes/no",
            "LOCK_ON_WRITE           yes/no  UPDATE_IF               yes/no",
            "MANUAL_UNLOCKING        yes/no  WAIT_FOR_RECORD         yes/no",
            "MULTIBLOCK_COUNT        number  WRITE_BEHIND            yes/no",
            "MULTIBUFFER_COUNT       number",
        ],
        "DATE" => &[
            "BACKUP                  string",
            "CREATION                string",
            "EXPIRATION              string",
            "REVISION                string",
        ],
        "FILE" => &[
            "ASYNCHRONOUS            yes/no  MT_OPEN_REWIND          yes/no",
            "ALLOCATION              number  MT_PROTECTION           char/num",
            "BEST_TRY_CONTIGUOUS     yes/no  NAME                    string",
            "BUCKET_SIZE             number  NON_FILE_STRUCTURED     yes/no",
            "CLUSTER_SIZE            number  ORGANIZATION            keyword",
            "CONTEXT                 number  OUTPUT_FILE_PARSE       yes/no",
            "CONTIGUOUS              yes/no  OWNER                   uic",
            "CREATE_IF               yes/no  PRINT_ON_CLOSE          yes/no",
            "DEFAULT_NAME            string  PROTECTION              yes/no",
            "DEFERRED_WRITE          yes/no  READ_CHECK              yes/no",
            "DELETE_ON_CLOSE         yes/no  REVISION                number",
            "DIRECTORY_ENTRY         yes/no  SEQUENTIAL_ONLY         yes/no",
            "EXTENSION               number  STORED_SEMANTICS        string",
            "FILE_MONITORING         yes/no  SUBMIT_ON_CLOSE         yes/no",
            "MAX_RECORD_NUMBER       number  SYNCSTS                 yes/no",
            "MAXIMIZE_VERSION        yes/no  TRUNCATE_ON_CLOSE       yes/no",
            "MT_BLOCK_SIZE           number  USER_FILE_OPEN          yes/no",
            "MT_CLOSE_REWIND         yes/no  WINDOW_SIZE             number",
            "MT_CURRENT_POSITION     yes/no  WRITE_CHECK             yes/no",
            "MT_NOT_EOF              yes/no  TEMPORARY               yes/no",
            "SUPERSEDE               yes/no  GLOBAL_BUFFER_COUNT     number",
            "GLBUFF_CNT_V83          number  GLBUFF_FLAGS_V83        keyword ",
        ],
        "KEY" => &[
            "CHANGES                 yes/no  LEVEL1_INDEX_AREA       number",
            "DATA_AREA               number  NAME                    string",
            "DATA_FILL               number  NULL_KEY                yes/no",
            "DATA_KEY_COMPRESSION    yes/no  NULL_VALUE              char/num",
            "DATA_RECORD_COMPRESSION yes/no  POSITION                number",
            "DUPLICATES              yes/no  PROLOG                  number",
            "INDEX_AREA              number  TYPE                    keyword",
            "INDEX_COMPRESSION       yes/no  SEGn_LENGTH             number",
            "INDEX_FILL              number  SEGn_POSITION           number",
            "LENGTH                  number  COLLATING_SEQUENCE      string",
        ],
        "NETWORK" => &[
            "BLOCK_COUNT             number",
            "LINK_CACHE_ENABLE       yes/no",
            "LINK_TIMEOUT            number",
            "NETWORK_DATA_CHECKING   yes/no",
        ],
        "RECORD" => &[
            "BLOCK_SPAN              yes/no",
            "MSB_RECORD_LENGTH       yes/no",
            "CARRIAGE_CONTROL        keyword",
            "CONTROL_FIELD_SIZE      number",
            "FORMAT                  keyword",
            "SIZE                    number",
        ],
        "SHARING" => &[
            "DELETE                  yes/no",
            "GET                     yes/no",
            "MULTISTREAM             yes/no",
            "PROHIBIT                yes/no",
            "PUT                     yes/no",
            "UPDATE                  yes/no",
            "USER_INTERLOCK          yes/no",
        ],
        "SYSTEM" => &[
            "DEVICE                  string",
            "SOURCE                  string",
            "TARGET                  string",
        ],

        _ => &[],
    }
}

/// The secondary attributes a primary takes, and what each value is.
pub fn legal(primary: &str) -> Vec<(&'static str, &'static str)> {
    rows(primary)
        .iter()
        .flat_map(|r| {
            let t: Vec<&str> = r.split_whitespace().collect();
            match t[..] {
                [a, ka, b, kb] => vec![(a, ka), (b, kb)],
                _ => vec![(t[0], t[t.len() - 1])],
            }
        })
        .collect()
}

/// ADD's `?` for a primary (`shown` as the questions name it).
fn legal_table(primary: &str, shown: &str) -> String {
    // FILE's has no blank lines, to fit on the screen.
    let blank = if primary == "FILE" { "" } else { "\n" };
    let rows: String = rows(primary).iter().map(|r| format!("\t{r}\n")).collect();
    format!("\t\t\t Legal {shown} Secondary Attributes \n{blank}{rows}{blank}")
}

/// The largest number `attr` takes.
/// ponytail: 4Giga where no recording says otherwise.
fn top(primary: &str, attr: &str) -> u64 {
    match (primary, attr) {
        (_, "BUCKET_SIZE") => 63,
        (_, "EXTENSION") => 65535,
        (_, "DATA_FILL" | "INDEX_FILL") => 100,
        ("RECORD", "SIZE") => 32240,
        ("KEY", a) if a.starts_with("SEG") => 255,
        _ => 4294967295,
    }
}

/// The keywords a `keyword` attribute takes, and their list.
fn words(attr: &str) -> (&'static [&'static str], &'static str) {
    match attr {
        "ORGANIZATION" => (
            &["INDEXED", "RELATIVE", "SEQUENTIAL"],
            "\t(Indexed Relative Sequential)\n",
        ),
        "FORMAT" => (
            &[
                "FIXED",
                "STREAM",
                "STREAM_CR",
                "STREAM_LF",
                "UNDEFINED",
                "VARIABLE",
                "VFC",
            ],
            "\t(Fixed Stream Stream_CR Stream_LF\n\t Undefined Variable VFC)\n",
        ),
        "CARRIAGE_CONTROL" => (
            &["CARRIAGE_RETURN", "FORTRAN", "NONE", "PRINT"],
            "\t(Carriage_Return FORTRAN None Print)\n",
        ),
        "TYPE" => (&super::scripts::TYPES, super::scripts::TYPE_LIST),
        _ => (
            &["DEFAULT", "NONE", "PERCENT"],
            "\t(Default None Percent)\n",
        ),
    }
}
