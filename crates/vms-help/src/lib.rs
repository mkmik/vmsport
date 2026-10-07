//! Help libraries: `.HLP` source (what LIBRARY/HELP reads on VMS), and
//! HELP's lookups and pages.

/// A key and what is under it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Topic {
    pub key: String,
    pub text: Vec<String>,
    pub subtopics: Vec<Topic>,
}

/// A help library: its level-1 topics.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Library {
    pub topics: Vec<Topic>,
}

/// Reads `.HLP` source. `n KEY` in column 1 starts a level-n key; `/NAME`
/// in column 1 starts a key one level below the last numbered one (the
/// qualifiers under `2 Qualifiers`); other lines are the current key's text.
pub fn parse(src: &str) -> Library {
    let mut root = Topic::default();
    // The path of open topics, as indices from the root.
    let mut path: Vec<usize> = Vec::new();
    let mut numbered: usize = 0;
    for line in src.lines() {
        let line = line.trim_end();
        let digits = line.chars().take_while(|c| c.is_ascii_digit()).count();
        let key = if digits > 0 && line[digits..].starts_with(' ') {
            numbered = line[..digits].parse().unwrap_or(1);
            Some((
                numbered,
                line[digits..]
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .to_string(),
            ))
        } else if line.starts_with('/') && numbered > 0 {
            Some((
                numbered + 1,
                line.split_whitespace().next().unwrap_or("").to_string(),
            ))
        } else {
            None
        };
        match key {
            Some((level, key)) => {
                path.truncate(level.saturating_sub(1).min(path.len()));
                let parent = path.iter().fold(&mut root, |t, &i| &mut t.subtopics[i]);
                parent.subtopics.push(Topic {
                    key,
                    ..Default::default()
                });
                path.push(parent.subtopics.len() - 1);
            }
            None if !path.is_empty() => {
                let t = path.iter().fold(&mut root, |t, &i| &mut t.subtopics[i]);
                t.text.push(line.to_string());
            }
            None => {}
        }
    }
    // Trailing blank lines aren't part of the text.
    fn trim(t: &mut Topic) {
        while t.text.last().is_some_and(|l| l.is_empty()) {
            t.text.pop();
        }
        t.subtopics.iter_mut().for_each(trim);
    }
    trim(&mut root);
    // Level-1 keys are a library's index, which is sorted; the rest keep
    // their order.
    root.subtopics.sort_by_key(|t| t.key.to_ascii_uppercase());
    Library {
        topics: root.subtopics,
    }
}

impl Topic {
    fn is_qualifier(&self) -> bool {
        self.key.starts_with('/')
    }

    /// Qualifier keys under this topic: its own, and those under a
    /// subtopic that holds them (`2 Qualifiers`).
    fn qualifiers(&self) -> Vec<&Topic> {
        let mut out: Vec<&Topic> = self.subtopics.iter().filter(|t| t.is_qualifier()).collect();
        for t in self.subtopics.iter().filter(|t| !t.is_qualifier()) {
            out.extend(t.subtopics.iter().filter(|q| q.is_qualifier()));
        }
        out
    }
}

/// `*` and `%` wildcards, both sides upcased.
fn wild(s: &str, p: &str) -> bool {
    fn m(c: &[char], p: &[char]) -> bool {
        match p.first() {
            None => c.is_empty(),
            Some('*') => (0..=c.len()).any(|i| m(&c[i..], &p[1..])),
            Some('%') => !c.is_empty() && m(&c[1..], &p[1..]),
            Some(x) => c.first() == Some(x) && m(&c[1..], &p[1..]),
        }
    }
    m(
        &s.chars().collect::<Vec<_>>(),
        &p.chars().collect::<Vec<_>>(),
    )
}

/// The topics among `choices` a typed word names: wildcards match any, an
/// exact key wins, else every key it abbreviates.
fn matches<'a>(choices: &[&'a Topic], word: &str) -> Vec<&'a Topic> {
    let w = word.to_ascii_uppercase();
    if w.contains(['*', '%']) {
        return choices
            .iter()
            .filter(|t| wild(&t.key.to_ascii_uppercase(), &w))
            .copied()
            .collect();
    }
    if let Some(t) = choices.iter().find(|t| t.key.to_ascii_uppercase() == w) {
        return vec![t];
    }
    choices
        .iter()
        .filter(|t| t.key.to_ascii_uppercase().starts_with(&w))
        .copied()
        .collect()
}

/// Splits what HELP was asked into words: `ALPHA/LOG /COUNT X...`.
pub fn words(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for w in text.split_whitespace() {
        let mut cur = String::new();
        for c in w.chars() {
            if c == '/' && !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            cur.push(c);
        }
        if !cur.is_empty() {
            out.push(cur);
        }
    }
    out.iter().map(|w| w.to_ascii_uppercase()).collect()
}

/// Lines HELP writes. A blank line is only written once something follows
/// it, as VMS HELP does.
#[derive(Default)]
pub struct Out {
    pub lines: Vec<String>,
    blanks: usize,
}

impl Out {
    fn push(&mut self, l: String) {
        if l.is_empty() {
            self.blanks += 1;
        } else {
            self.lines
                .extend(std::iter::repeat_n(String::new(), self.blanks));
            self.blanks = 0;
            self.lines.push(l);
        }
    }

    /// Writes the blank lines still owed, as HELP does when it ends.
    pub fn flush(&mut self) {
        self.lines
            .extend(std::iter::repeat_n(String::new(), self.blanks));
        self.blanks = 0;
    }

    /// Takes the lines written so far.
    pub fn take(&mut self) -> Vec<String> {
        std::mem::take(&mut self.lines)
    }
}

fn indent(n: usize, s: &str) -> String {
    if s.is_empty() {
        String::new()
    } else {
        format!("{:n$}{s}", "")
    }
}

/// HELP over one or more libraries (the first is the main one).
pub struct Help {
    pub libraries: Vec<Library>,
    /// The output's width, for the lists of subtopics.
    pub width: usize,
    pub instructions: bool,
}

impl Help {
    fn top(&self) -> Vec<&Topic> {
        self.libraries.iter().flat_map(|l| &l.topics).collect()
    }

    /// The keys under `t` in 11-column cells; qualifiers on lines of their
    /// own.
    fn list(&self, out: &mut Out, at: usize, topics: &[&Topic]) {
        let mut names: Vec<&str> = Vec::new();
        for t in topics.iter().filter(|t| !t.is_qualifier()) {
            names.push(&t.key);
            names.extend(
                t.subtopics
                    .iter()
                    .filter(|q| q.is_qualifier())
                    .map(|q| q.key.as_str()),
            );
        }
        names.extend(
            topics
                .iter()
                .filter(|t| t.is_qualifier())
                .map(|t| t.key.as_str()),
        );
        let mut line = String::new();
        let mut last_q = None;
        for n in names {
            let q = n.starts_with('/');
            let wraps =
                !line.is_empty() && (at + line.len() + n.len() > self.width || last_q != Some(q));
            if wraps {
                out.push(indent(at, line.trim_end()));
                line.clear();
            }
            last_q = Some(q);
            let cells = n.len() / 11 + 1;
            line.push_str(&format!("{n:<w$}", w = cells * 11));
        }
        if !line.is_empty() {
            out.push(indent(at, line.trim_end()));
        }
    }

    /// The headers of a path: `ALPHA`, then `  SUBTOPIC_ONE`...
    fn headers(&self, out: &mut Out, path: &[&Topic]) {
        out.push(String::new());
        for (i, t) in path.iter().enumerate() {
            if i > 0 {
                out.push(String::new());
            }
            out.push(indent(2 * i, &t.key));
        }
    }

    /// A topic: its path's headers, its text (and its qualifiers', if it
    /// holds them), and with `list` the subtopics there are.
    fn display(&self, out: &mut Out, path: &[&Topic], list: bool) {
        self.headers(out, path);
        let d = path.len();
        let t = path[d - 1];
        if t.is_qualifier() {
            t.text.iter().for_each(|l| out.push(indent(2 * (d - 1), l)));
        } else {
            out.push(String::new());
            t.text.iter().for_each(|l| out.push(indent(2 * d, l)));
            for q in t.subtopics.iter().filter(|q| q.is_qualifier()) {
                out.push(indent(2 * d, &q.key));
                q.text.iter().for_each(|l| out.push(indent(2 * d, l)));
            }
        }
        out.push(String::new());
        let subs: Vec<&Topic> = t.subtopics.iter().filter(|s| !s.is_qualifier()).collect();
        if list && !subs.is_empty() {
            self.available(out, 2 * d, &subs, "Additional information available:");
        }
    }

    fn available(&self, out: &mut Out, at: usize, topics: &[&Topic], title: &str) {
        out.push(String::new());
        out.push(indent(at, title));
        out.push(String::new());
        self.list(out, at, topics);
        out.push(String::new());
    }

    /// HELP with no topic: the library's HELP topic and every topic there
    /// is (or, /NOINSTRUCTIONS, just the topics).
    pub fn top_display(&self, out: &mut Out) {
        let top = self.top();
        if !self.instructions {
            self.available(out, 2, &top, "Information available:");
            return;
        }
        match self
            .libraries
            .first()
            .and_then(|l| l.topics.iter().find(|t| t.key.eq_ignore_ascii_case("HELP")))
        {
            Some(h) => {
                self.display(out, &[h], false);
                self.available(out, 2, &top, "Additional information available:");
            }
            None => self.sorry(out, &[], "HELP", true),
        }
    }

    /// `Sorry, no documentation on ...`, under what was found, and the
    /// topics there are instead.
    fn sorry(&self, out: &mut Out, found: &[&Topic], asked: &str, list: bool) {
        let mut found = found;
        if let Some(q) = found.last().filter(|t| t.is_qualifier()) {
            // A qualifier with nothing under it: VMS shows it alone.
            out.push(String::new());
            out.push(q.key.clone());
            found = &found[..found.len() - 1];
        } else if !found.is_empty() {
            self.headers(out, found);
        }
        out.push(format!("  Sorry, no documentation on {asked}"));
        let choices: Vec<&Topic> = match found.last() {
            Some(t) => t.subtopics.iter().filter(|s| !s.is_qualifier()).collect(),
            None => self.top(),
        };
        if list && !choices.is_empty() {
            out.push(String::new());
            self.available(
                out,
                2 * found.len().max(1),
                &choices,
                "Additional information available:",
            );
        } else {
            out.push(String::new());
        }
    }

    /// Looks up `words` under `at` (a path; empty is the top) and shows
    /// what they name. Returns the paths shown.
    pub fn lookup<'a>(
        &'a self,
        out: &mut Out,
        at: &[&'a Topic],
        words: &[String],
    ) -> Vec<Vec<&'a Topic>> {
        let mut words = words.to_vec();
        let all = words.last().is_some_and(|w| w.ends_with("..."));
        if let Some(w) = words.last_mut().filter(|w| w.ends_with("...")) {
            w.truncate(w.len() - 3);
        }
        let mut paths: Vec<Vec<&Topic>> = vec![at.to_vec()];
        let mut found: Vec<&Topic> = at.to_vec();
        for w in &words {
            let mut next = Vec::new();
            for p in &paths {
                let choices: Vec<&Topic> = match p.last() {
                    None => self.top(),
                    Some(t) if w.starts_with('/') => t.qualifiers(),
                    Some(t) => t.subtopics.iter().filter(|s| !s.is_qualifier()).collect(),
                };
                for m in matches(&choices, w) {
                    let mut q = p.clone();
                    q.push(m);
                    next.push(q);
                }
            }
            if next.is_empty() {
                // What was asked: what matched, from the top, and the rest.
                let matched = found.len() - at.len();
                let asked: Vec<String> = found
                    .iter()
                    .map(|t| t.key.clone())
                    .chain(words[matched..].iter().cloned())
                    .collect();
                self.sorry(out, &found, &asked.join(" "), !all);
                return Vec::new();
            }
            found = next[0].clone();
            paths = next;
        }
        for p in &paths {
            if all {
                self.everything(out, p);
            } else {
                self.display(out, p, true);
            }
        }
        paths
    }

    /// `TOPIC...`: the topic and everything under it, without lists.
    fn everything<'a>(&self, out: &mut Out, path: &[&'a Topic]) {
        self.display(out, path, false);
        for s in path[path.len() - 1]
            .subtopics
            .iter()
            .filter(|s| !s.is_qualifier())
        {
            let mut p = path.to_vec();
            p.push(s);
            self.everything(out, &p);
        }
    }

    /// Shows what `words` name under `level`, and moves `level` into it: a
    /// topic with subtopics, or a leaf's parent.
    fn show<'a>(&'a self, out: &mut Out, level: &mut Vec<&'a Topic>, words: &[String]) {
        let shown = self.lookup(out, level, words);
        if let [p] = &shown[..] {
            let has = p
                .last()
                .is_some_and(|t| t.subtopics.iter().any(|s| !s.is_qualifier()));
            *level = if has {
                p.clone()
            } else {
                p[..p.len() - 1].to_vec()
            };
        }
    }

    /// A HELP session: what `words` ask for (or the top), then, with
    /// `prompt`, topics from `input` until it ends or Return at the top.
    /// `ask` gets the prompt (`Topic?`, `ALPHA Subtopic?`) and returns the
    /// answer, `None` at end of input.
    pub fn session(
        &self,
        out: &mut Out,
        words: &[String],
        prompt: bool,
        ask: &mut dyn FnMut(&str, &mut Out) -> Option<String>,
    ) {
        let mut level: Vec<&Topic> = Vec::new();
        if words.is_empty() {
            self.top_display(out);
        } else {
            self.show(out, &mut level, words);
        }
        if !prompt {
            out.flush();
            return;
        }
        loop {
            let q = if level.is_empty() {
                "Topic? ".to_string()
            } else {
                format!(
                    "{} Subtopic? ",
                    level
                        .iter()
                        .map(|t| t.key.as_str())
                        .collect::<Vec<_>>()
                        .join(" ")
                )
            };
            let Some(answer) = ask(&q, out) else {
                out.flush();
                return;
            };
            let answer = answer.trim();
            if answer.is_empty() {
                if level.pop().is_none() {
                    out.flush();
                    return;
                }
            } else if answer == "?" {
                match level.split_last() {
                    Some(_) => self.display(out, &level, true),
                    None => self.top_display(out),
                }
            } else {
                self.show(out, &mut level, &self::words(answer));
            }
        }
    }
}
