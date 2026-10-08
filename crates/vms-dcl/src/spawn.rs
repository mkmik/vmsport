//! SPAWN and PIPE: DCL subprocesses (fixtures/spawn records what VMS does).

use crate::{Child, Dcl, DclError, Launch, Mode, NORMAL, RecordFile};
use libvms::image::{Symbol, Value};
use std::process::Stdio;
use vms_cld::{ParseResult, status};
use vms_cond::Cond;

const SPAWNED: Cond = Cond(0x0003_FD01);
const ATTACHED: Cond = Cond(0x0003_FD09);
const RETURNED: Cond = Cond(0x0003_FD11);

type R = Result<Option<Cond>, DclError>;

fn value(r: &mut ParseResult, name: &str) -> Option<String> {
    r.get_value(name).ok().map(|v| v.0)
}

fn present(r: &ParseResult, name: &str) -> bool {
    matches!(
        r.present(name),
        status::PRESENT | status::DEFAULTED | status::LOCPRES
    )
}

/// One command of a pipeline, or a `( ... )` subshell, with its
/// redirections.
#[derive(Debug, Default, PartialEq)]
struct Segment {
    text: String,
    subshell: bool,
    input: Option<String>,
    output: Option<String>,
    error: Option<String>,
}

/// How a pipeline joins the one before it.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Op {
    Seq,
    And,
    Or,
}

/// `a | b ; c && (d ; e) > f`: pipelines joined by `;`, `&&`, `||`.
fn parse_pipe(s: &str) -> Result<Vec<(Op, Vec<Segment>)>, DclError> {
    let cs: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut pipeline = Vec::new();
    let mut seg = Segment::default();
    let mut op = Op::Seq;
    let mut i = 0;
    let word = |i: &mut usize| -> String {
        while *i < cs.len() && cs[*i].is_whitespace() {
            *i += 1;
        }
        let start = *i;
        while *i < cs.len() && !cs[*i].is_whitespace() && !"|;&()<>".contains(cs[*i]) {
            *i += 1;
        }
        cs[start..*i].iter().collect()
    };
    let after_blank = |i: usize| i == 0 || cs[i - 1].is_whitespace();
    while i < cs.len() {
        let c = cs[i];
        let next = cs.get(i + 1).copied();
        if c == '(' && seg.text.trim().is_empty() && !seg.subshell {
            // A subshell: to the matching parenthesis.
            let (mut depth, mut quoted, start) = (0, false, i + 1);
            while i < cs.len() {
                match cs[i] {
                    '"' => quoted = !quoted,
                    '(' if !quoted => depth += 1,
                    ')' if !quoted => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                i += 1;
            }
            if depth != 0 {
                return Err(DclError::new("SYMDEL"));
            }
            seg.text = cs[start..i].iter().collect();
            seg.subshell = true;
            i += 1;
            continue;
        }
        match (c, next) {
            ('"', _) => {
                seg.text.push(c);
                i += 1;
                while i < cs.len() {
                    seg.text.push(cs[i]);
                    i += 1;
                    if cs[i - 1] == '"' {
                        break;
                    }
                }
                continue;
            }
            ('|', Some('|')) | ('&', Some('&')) | (';', _) => {
                pipeline.push(std::mem::take(&mut seg));
                out.push((op, std::mem::take(&mut pipeline)));
                op = match c {
                    '|' => Op::Or,
                    '&' => Op::And,
                    _ => Op::Seq,
                };
                i += if c == ';' { 1 } else { 2 };
                continue;
            }
            ('|', _) => {
                pipeline.push(std::mem::take(&mut seg));
                i += 1;
                continue;
            }
            ('2', Some('>')) if after_blank(i) => {
                i += 2;
                seg.error = Some(word(&mut i));
                continue;
            }
            ('>', _) if after_blank(i) => {
                i += 1;
                seg.output = Some(word(&mut i));
                continue;
            }
            ('<', _) if after_blank(i) => {
                i += 1;
                seg.input = Some(word(&mut i));
                continue;
            }
            _ => {}
        }
        seg.text.push(c);
        i += 1;
    }
    pipeline.push(seg);
    out.push((op, pipeline));
    for (_, p) in &mut out {
        for s in p.iter_mut() {
            s.text = s.text.trim().to_string();
        }
    }
    Ok(out)
}

impl Dcl {
    /// The symbols a subprocess starts with: DCL's own, and $STATUS.
    fn subprocess_symbols(&self, copy: bool) -> Vec<Symbol> {
        let mut s = if copy {
            self.visible_symbols()
        } else {
            Vec::new()
        };
        s.push(Symbol {
            name: "$STATUS".into(),
            global: true,
            value: Value::Int(self.status.0 as i32),
        });
        s
    }

    /// `%DCL-S-SPAWNED, process X spawned` and its kind.
    fn show_cli(&mut self, code: Cond, arg: &str) {
        let m = self
            .catalog
            .put_msg(&[(code, vec![vms_fao::Arg::Str(arg)])], self.msg_flags);
        self.show(&m.join("\n").replacen("%CLI-", "%DCL-", 1));
    }

    /// A file for a child's output: a new version, kept open by `keep`.
    fn output_file(
        &mut self,
        spec: &str,
        keep: &mut Vec<Box<dyn RecordFile>>,
    ) -> Result<Stdio, DclError> {
        let (f, _) = self
            .host
            .open(spec, "", Mode::Write, crate::Share::None)
            .map_err(DclError::status)?;
        let file = f
            .host_file()
            .ok_or_else(|| DclError::status(Cond(0x1C112)))?;
        keep.push(f);
        Ok(Stdio::from(file))
    }

    /// Where a child's output goes when nothing says: DCL's SYS$OUTPUT.
    fn current_output(&self) -> Option<Stdio> {
        let i = self.frames.last().map_or(0, |f| f.output);
        self.outputs[i].host_file().map(Stdio::from)
    }

    pub(crate) fn spawn(&mut self, r: &mut ParseResult) -> R {
        let command = value(r, "COMMAND").unwrap_or_default();
        let (wait, log) = (present(r, "WAIT"), present(r, "LOG"));
        let symbols = self.subprocess_symbols(present(r, "SYMBOL"));
        self.spawned += 1;
        let name = value(r, "PROCESS")
            .unwrap_or_else(|| format!("{}_{}", self.process_name, self.spawned));
        let mut keep = Vec::new();
        let stdout = match value(r, "OUTPUT") {
            Some(spec) => Some(self.output_file(&spec, &mut keep)?),
            None => self.current_output(),
        };
        let stdin = match value(r, "INPUT") {
            Some(spec) => Some(Stdio::from(
                self.host.input_file(&spec).map_err(DclError::status)?,
            )),
            None => None,
        };
        let launch = Launch {
            symbols: &symbols,
            line: &command,
            spawn: true,
            stdin,
            stdout,
            no_logicals: !present(r, "LOGICAL_NAMES"),
            env: vec![("VMSPORT_PROCESS".into(), name.clone())],
            ..Default::default()
        };
        if log {
            self.show_cli(SPAWNED, &name);
            if wait {
                self.show_cli(ATTACHED, &name);
            }
        }
        let child = self
            .host
            .start(Child::Dcl, launch)
            .map_err(DclError::status)?;
        if !wait {
            std::thread::spawn(move || child.wait());
            return Ok(Some(NORMAL));
        }
        let st = child.wait().status;
        drop(keep);
        if log {
            let me = self.process_name.clone();
            self.show_cli(RETURNED, &me);
        }
        // A failure the subprocess ended with, the parent shows again.
        if !st.is_success() && !st.inhibit_msg() {
            let m = self.message(st);
            self.show(&m);
            self.just_shown = true;
        }
        Ok(Some(st))
    }

    pub(crate) fn pipe(&mut self, text: &str) -> R {
        let mut st = self.status;
        let mut ran = true;
        for (op, pipeline) in parse_pipe(text)? {
            ran = match op {
                Op::Seq => true,
                Op::And => st.is_success(),
                Op::Or => !st.is_success(),
            };
            if !ran {
                continue;
            }
            let simple = pipeline.len() == 1 && {
                let s = &pipeline[0];
                !s.subshell && s.input.is_none() && s.output.is_none() && s.error.is_none()
            };
            st = if simple {
                self.here(&pipeline[0].text)
            } else {
                self.pipeline(&pipeline)?
            };
            self.status = st;
        }
        // VMS leaves 0 when the last part didn't run.
        Ok(Some(if ran { st } else { Cond(0) }))
    }

    /// A PIPE command that runs in this process.
    fn here(&mut self, line: &str) -> Cond {
        match self.dispatch(line) {
            Ok(Some(st)) => st,
            Ok(None) => self.status,
            Err(e) => {
                self.report(&e);
                e.code
            }
        }
    }

    /// Subprocesses joined by pipes; the status is the last one's, shown.
    fn pipeline(&mut self, segs: &[Segment]) -> Result<Cond, DclError> {
        let symbols = self.subprocess_symbols(true);
        let mut keep = Vec::new();
        let mut children = Vec::new();
        let mut prev: Option<std::process::ChildStdout> = None;
        for (k, seg) in segs.iter().enumerate() {
            let last = k + 1 == segs.len();
            let stdin = match (&seg.input, prev.take()) {
                (Some(f), _) => Some(Stdio::from(
                    self.host.input_file(f).map_err(DclError::status)?,
                )),
                (None, Some(p)) => Some(Stdio::from(p)),
                (None, None) if k > 0 => Some(Stdio::null()),
                (None, None) => None,
            };
            let stdout = match &seg.output {
                Some(f) => Some(self.output_file(f, &mut keep)?),
                None if !last => Some(Stdio::piped()),
                None => self.current_output(),
            };
            let stderr = match &seg.error {
                Some(f) => Some(self.output_file(f, &mut keep)?),
                None => None,
            };
            let line = if seg.subshell {
                format!("PIPE {}", seg.text)
            } else {
                seg.text.clone()
            };
            let launch = Launch {
                symbols: &symbols,
                line: &line,
                spawn: true,
                stdin,
                stdout,
                stderr,
                ..Default::default()
            };
            let mut c = self
                .host
                .start(Child::Dcl, launch)
                .map_err(DclError::status)?;
            if !last && seg.output.is_none() {
                prev = c.take_stdout();
            }
            children.push(c);
        }
        let mut st = NORMAL;
        for c in children {
            st = c.wait().status;
        }
        Ok(Cond(st.0 | 0x1000_0000))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipes() {
        let p = parse_pipe(
            "TYPE X | SEARCH SYS$PIPE \"a|b\" ; Y = 1 && (A ; B) > OUT.TXT 2> ERR || C < IN",
        )
        .unwrap();
        assert_eq!(p.len(), 4);
        assert_eq!(
            p[0].1.iter().map(|s| s.text.as_str()).collect::<Vec<_>>(),
            ["TYPE X", "SEARCH SYS$PIPE \"a|b\""]
        );
        assert_eq!((p[1].0, p[1].1[0].text.as_str()), (Op::Seq, "Y = 1"));
        let sub = &p[2].1[0];
        assert_eq!(
            (p[2].0, sub.subshell, sub.text.as_str()),
            (Op::And, true, "A ; B")
        );
        assert_eq!(
            (sub.output.as_deref(), sub.error.as_deref()),
            (Some("OUT.TXT"), Some("ERR"))
        );
        assert_eq!((p[3].0, p[3].1[0].input.as_deref()), (Op::Or, Some("IN")));
        // Parentheses in a command are not a subshell.
        assert_eq!(
            parse_pipe("WRITE SYS$OUTPUT F$LENGTH(\"x\")").unwrap()[0].1[0].text,
            "WRITE SYS$OUTPUT F$LENGTH(\"x\")"
        );
    }
}
