//! Replays fixtures/lnm/LNM.COM through vms-lnm and compares SHOW LOGICAL,
//! F$TRNLNM and F$PARSE results with what OpenVMS printed
//! (fixtures/lnm/recorded/lnm.log). DCL's own messages are left out.

use std::collections::HashMap;
use vms_lnm::*;

fn sections() -> HashMap<String, String> {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/lnm/recorded/lnm.log");
    let log = std::fs::read_to_string(p).unwrap();
    log.split("@@ ")
        .skip(1)
        .map(|s| {
            let (name, body) = s.split_once('\n').unwrap();
            // DCL's messages and their \TOKEN\ lines aren't vms-lnm's.
            let body: String = body
                .lines()
                .filter(|l| !l.starts_with("%DCL-") && !l.starts_with(" \\"))
                .map(|l| format!("{l}\n"))
                .collect();
            (name.to_string(), body)
        })
        .collect()
}

struct T {
    n: Names<Vec<Table>>,
    out: String,
}

impl T {
    fn def(&mut self, table: &str, name: &str, values: &[&str]) -> &mut Logical {
        self.n.define(table, Logical::new(name, values)).unwrap();
        let t = self.n.tables(table, Mode::User).remove(0);
        // For the attribute tweaks the cases make next.
        if let Some(i) = self.n.process.iter().position(|x| x.name == t) {
            let tab = &mut self.n.process[i];
            return tab.logicals.iter_mut().find(|l| l.name == name).unwrap();
        }
        let tab = self.n.shared.iter_mut().find(|x| x.name == t).unwrap();
        tab.logicals.iter_mut().find(|l| l.name == name).unwrap()
    }

    fn show(&mut self, name: Option<&str>, table: &str, full: bool) {
        self.out += &show(&self.n, name, table, full);
    }

    /// F$TRNLNM(name, table, index, mode, , item), case blind as it is.
    fn trn(&self, name: &str, table: &str, index: usize, mode: Mode, it: &str) -> String {
        item(
            self.n.translate(name, table, mode, true).as_ref(),
            index,
            it,
        )
        .unwrap()
    }

    fn line(&mut self, s: &str) {
        self.out += s;
        self.out.push('\n');
    }

    fn check(&mut self, sections: &HashMap<String, String>, name: &str) {
        assert_eq!(
            std::mem::take(&mut self.out),
            sections[name],
            "section {name}"
        );
    }
}

const DCL: &str = "LNM$DCL_LOGICAL";

#[test]
fn recorded_lnm() {
    let s = sections();
    let mut shared = system_tables();
    let (job, group) = (job_table(0x80CB54C0), group_table(1));
    add_table(&mut shared, &job);
    add_table(&mut shared, &group);
    let mut t = T {
        n: Names::new(shared, &job, &group),
        out: String::new(),
    };

    t.def("LNM$PROCESS", "VPT_ONE", &["first value"]);
    t.show(Some("VPT_ONE"), DCL, false);
    t.show(Some("VPT_ONE"), DCL, true);
    t.check(&s, "simple");

    t.show(Some("VPT_UP"), DCL, false);
    t.check(&s, "upcased");

    t.def("LNM$PROCESS", "VPT_ASG", &["assigned"]);
    t.show(Some("VPT_ASG"), DCL, false);
    t.check(&s, "assign");

    assert_eq!(
        t.n.define("LNM$PROCESS", Logical::new("VPT_ONE", &["second value"])),
        Ok(SS_SUPERSEDE)
    );
    t.show(Some("VPT_ONE"), DCL, false);
    t.check(&s, "supersede");

    t.def("LNM$PROCESS", "VPT_ONE", &["third value"]);
    t.show(Some("VPT_ONE"), DCL, false);
    t.check(&s, "nolog");

    t.def("LNM$PROCESS", "VPT_TWO", &["VPT_ONE"]);
    t.def("LNM$PROCESS", "VPT_THREE", &["VPT_TWO"]);
    t.show(Some("VPT_THREE"), DCL, false);
    t.show(Some("VPT_THREE"), DCL, true);
    t.check(&s, "chain");

    t.def("LNM$PROCESS", "VPT_LIST", &["alpha", "Beta", "GAMMA"]);
    t.show(Some("VPT_LIST"), DCL, false);
    t.show(Some("VPT_LIST"), DCL, true);
    t.check(&s, "search list");

    t.def("LNM$PROCESS", "VPT_LISTREF", &["VPT_LIST"]);
    t.show(Some("VPT_LISTREF"), DCL, false);
    t.check(&s, "search list chain");

    t.def("LNM$PROCESS", "VPT_TERM", &["VPT_ONE"]).equivs[0].terminal = true;
    t.show(Some("VPT_TERM"), DCL, false);
    t.show(Some("VPT_TERM"), DCL, true);
    t.check(&s, "terminal");

    let e = &mut t
        .def("LNM$PROCESS", "VPT_CONC", &["DKA200:[T.LNM.]"])
        .equivs[0];
    (e.concealed, e.terminal) = (true, true);
    t.show(Some("VPT_CONC"), DCL, false);
    t.show(Some("VPT_CONC"), DCL, true);
    t.check(&s, "concealed");

    t.def("LNM$PROCESS", "VPT_MIX", &["a", "b"]).equivs[0].concealed = true;
    t.show(Some("VPT_MIX"), DCL, true);
    t.check(&s, "per value attributes");

    let l = t.def("LNM$PROCESS", "VPT_NA", &["x"]);
    (l.no_alias, l.confine) = (true, true);
    t.show(Some("VPT_NA"), DCL, true);
    t.check(&s, "name attributes");

    t.n.define(
        "LNM$PROCESS",
        Logical {
            mode: Mode::Executive,
            ..Logical::new("VPT_EXEC", &["exec"])
        },
    )
    .unwrap();
    t.def("LNM$PROCESS", "VPT_SUP", &["sup"]);
    t.show(Some("VPT_EXEC"), DCL, true);
    t.show(Some("VPT_SUP"), DCL, true);
    t.check(&s, "modes");

    t.def("LNM$JOB", "VPT_JOBNAME", &["job value"]);
    t.show(Some("VPT_JOBNAME"), DCL, false);
    t.check(&s, "job");

    t.def("LNM$GROUP", "VPT_GRP", &["group value"]);
    t.show(Some("VPT_GRP"), DCL, false);
    t.show(Some("VPT_GRP"), DCL, true);
    t.check(&s, "group");

    t.def("LNM$SYSTEM", "VPT_SYS", &["system value"]);
    t.show(Some("VPT_SYS"), DCL, true);
    t.check(&s, "system");

    t.def("LNM$SYSTEM", "VPT_BOTH", &["in system"]);
    t.def("LNM$PROCESS", "VPT_BOTH", &["in process"]);
    t.show(Some("VPT_BOTH"), DCL, false);
    let v = t.trn("VPT_BOTH", DCL, 0, Mode::User, "VALUE");
    t.line(&v);
    let v = t.trn("VPT_BOTH", "LNM$SYSTEM", 0, Mode::User, "VALUE");
    t.line(&v);
    t.check(&s, "shadowed");

    t.show(Some("VPT_L*"), DCL, false);
    t.show(Some("VPT_T*"), "LNM$PROCESS", false);
    t.check(&s, "wildcard");

    t.n.create_table("VPT_TAB", Mode::Supervisor);
    t.def("VPT_TAB", "VPT_INTAB", &["in table"]);
    t.def("VPT_TAB", "VPT_INTAB2", &["also in table"]);
    t.show(None, "VPT_TAB", false);
    t.show(Some("VPT_INTAB"), "VPT_TAB", false);
    t.show(Some("VPT_TAB"), PROCESS_DIRECTORY, true);
    let v = format!("[{}]", t.trn("VPT_INTAB", DCL, 0, Mode::User, "VALUE"));
    t.line(&v);
    let v = t.trn("VPT_INTAB", "VPT_TAB", 0, Mode::User, "VALUE");
    t.line(&v);
    t.check(&s, "table");

    t.def(
        PROCESS_DIRECTORY,
        "VPT_SEARCH",
        &["VPT_TAB", "LNM$PROCESS_TABLE"],
    );
    for (name, it) in [
        ("VPT_INTAB", "VALUE"),
        ("VPT_ONE", "VALUE"),
        ("VPT_INTAB", "TABLE_NAME"),
    ] {
        let v = t.trn(name, "VPT_SEARCH", 0, Mode::User, it);
        t.line(&v);
    }
    t.show(Some("VPT_INTAB"), "VPT_SEARCH", false);
    t.show(Some("VPT_SEARCH"), PROCESS_DIRECTORY, true);
    t.check(&s, "table search list");

    for (name, dir) in [
        ("LNM$FILE_DEV", SYSTEM_DIRECTORY),
        ("LNM$DCL_LOGICAL", SYSTEM_DIRECTORY),
        ("LNM$PROCESS", PROCESS_DIRECTORY),
        ("LNM$SYSTEM", SYSTEM_DIRECTORY),
        ("LNM$GROUP", SYSTEM_DIRECTORY),
        ("LNM$PROCESS_TABLE", PROCESS_DIRECTORY),
        ("LNM$SYSTEM_TABLE", SYSTEM_DIRECTORY),
    ] {
        t.show(Some(name), dir, true);
    }
    t.check(&s, "directories");

    let items = "VALUE,LENGTH,MAX_INDEX,TABLE,TABLE_NAME,TERMINAL,CONCEALED,CONFINE,NO_ALIAS,CRELOG,ACCESS_MODE";
    for (name, table) in [
        ("VPT_ONE", DCL),
        ("VPT_LIST", DCL),
        ("VPT_TERM", DCL),
        ("VPT_CONC", DCL),
        ("VPT_MIX", DCL),
        ("VPT_NA", DCL),
        ("VPT_EXEC", DCL),
        ("VPT_SYS", DCL),
        ("VPT_INTAB", "VPT_TAB"),
        ("VPT_NONE", DCL),
    ] {
        for it in items.split(',') {
            let v = t.trn(name, table, 0, Mode::User, it);
            t.line(&format!("{name} {it} [{v}]"));
        }
    }
    t.check(&s, "items");

    let v: Vec<String> = (0..4)
        .map(|i| t.trn("VPT_LIST", DCL, i, Mode::User, "VALUE"))
        .collect();
    t.line(&format!("{}{}{}[{}]", v[0], v[1], v[2], v[3]));
    let v = [(1, "CONCEALED"), (0, "CONCEALED"), (0, "LENGTH")]
        .map(|(i, it)| t.trn("VPT_MIX", DCL, i, Mode::User, it));
    t.line(&v.join(" "));
    t.check(&s, "index");

    let v = [
        ("VPT_ONE", Mode::Executive),
        ("VPT_EXEC", Mode::Executive),
        ("VPT_EXEC", Mode::Kernel),
    ]
    .map(|(n, m)| format!("[{}]", t.trn(n, DCL, 0, m, "VALUE")));
    t.line(&v.join(" "));
    t.check(&s, "modes in F$TRNLNM");

    t.def("LNM$PROCESS", "VptMixed", &["case sensitive name"]);
    let v = ["VptMixed", "VPTMIXED", "vptmixed"]
        .map(|n| format!("[{}]", t.trn(n, DCL, 0, Mode::User, "VALUE")));
    t.line(&v.join(" "));
    t.show(Some("VptMixed"), DCL, false);
    t.check(&s, "case");

    t.show(Some("VPT_NONE"), DCL, false);
    let v = format!("[{}]", t.trn("VPT_NONE", DCL, 0, Mode::User, "VALUE"));
    t.line(&v);
    assert_eq!(
        t.n.deassign("LNM$PROCESS", "VPT_NONE", Mode::Supervisor),
        Err(SS_NOLOGNAM)
    );
    t.line("%SYSTEM-F-NOLOGNAM, no logical name match");
    t.check(&s, "undefined");

    t.n.deassign("LNM$PROCESS", "VPT_ASG", Mode::Supervisor)
        .unwrap();
    let v = format!("[{}]", t.trn("VPT_ASG", DCL, 0, Mode::User, "VALUE"));
    t.line(&v);
    t.n.deassign("LNM$SYSTEM", "VPT_BOTH", Mode::Supervisor)
        .unwrap();
    let v = format!(
        "[{}] [{}]",
        t.trn("VPT_BOTH", "LNM$SYSTEM", 0, Mode::User, "VALUE"),
        t.trn("VPT_BOTH", DCL, 0, Mode::User, "VALUE")
    );
    t.line(&v);
    t.n.deassign("VPT_TAB", "VPT_INTAB2", Mode::Supervisor)
        .unwrap();
    t.show(None, "VPT_TAB", false);
    t.check(&s, "deassign");

    t.def("LNM$PROCESS", "VPT_ROOT", &["DKA200:[T.LNM.]"])
        .equivs[0]
        .concealed = true;
    t.def("LNM$PROCESS", "VPT_ROOT2", &["DKA200:[T.LNM.]"]);
    t.def("LNM$PROCESS", "VPT_DEV", &["VPT_ROOT:"]);
    t.def("LNM$PROCESS", "VPT_SUBDIR", &["VPT_ROOT:[SUB]"]);
    t.def("LNM$PROCESS", "VPT_FILE", &["VPT_ROOT:[SUB]Z.DAT"]);
    t.def("LNM$PROCESS", "VPT_PLAIN", &["DKA200:[T.LNM.SUB]"]);
    t.def("LNM$PROCESS", "VPT_CDEV", &["DKA200:"]).equivs[0].concealed = true;
    // F$PARSE shows an empty version as ";".
    let parse = |spec: &str, no_conceal: bool| {
        let r = t.n.resolve(&spec.parse().unwrap()).unwrap().remove(0);
        format!("{};", if no_conceal { r.physical } else { r.display })
    };
    let rooted: Vec<&str> = s["rooted"].lines().collect();
    let r0 =
        t.n.resolve(&"VPT_ROOT:[SUB]X.Y".parse().unwrap())
            .unwrap()
            .remove(0);
    let checks = [
        (0, parse("VPT_ROOT:[SUB]X.Y", false)),
        (1, parse("VPT_ROOT:[SUB]X.Y", true)),
        (
            2,
            format!(
                "{}: {}",
                r0.display.device.as_ref().unwrap(),
                r0.display.directory.as_ref().unwrap()
            ),
        ),
        (3, parse("VPT_ROOT:[000000]X.Y", false)),
        // 4 and 5 need the directory to exist; 6 is SYNTAX_ONLY.
        (6, parse("VPT_ROOT:[SUB.NOSUCH]X.Y", false)),
        (7, parse("VPT_ROOT2:[SUB]X.Y", false)),
        (8, parse("VPT_ROOT2:[SUB]X.Y", true)),
        (9, parse("VPT_DEV:[SUB]X.Y", false)),
        (10, parse("VPT_SUBDIR:X.Y", false)),
        (11, parse("VPT_SUBDIR:X.Y", true)),
        (12, parse("VPT_FILE", false)),
        (13, parse("VPT_FILE", true)),
        (14, parse("VPT_PLAIN:X.Y", false)),
        (15, parse("VPT_CDEV:[T.LNM.SUB]X.Y", false)),
    ];
    for (i, got) in checks {
        assert_eq!(got, rooted[i], "rooted line {i}");
    }
    assert_eq!(r0.path(), ["T", "LNM", "SUB"]);
}
