//! A buffer's text and the edits EVE makes to it. Lines are chars; a
//! position is (line, column), and the line just past the last is the end
//! of the buffer, where only column 0 exists.

pub type Pos = (usize, usize);

#[derive(Debug, Clone, Default)]
pub struct Text {
    pub lines: Vec<Vec<char>>,
    /// What typing, Return and deleting did, for the editor to move
    /// other windows' positions by (it takes them).
    pub edits: Vec<Edit>,
}

/// Text went in from the first position up to the second, or what was
/// between them went out.
#[derive(Debug, Clone, Copy)]
pub enum Edit {
    Insert(Pos, Pos),
    Delete(Pos, Pos),
}

impl Edit {
    /// Where position `q` is after the edit: with its character, as TPU's
    /// marks, so text put in at a position goes before it.
    pub fn moved(&self, q: Pos) -> Pos {
        match *self {
            Edit::Insert(a, _) if q < a => q,
            Edit::Insert(a, b) if q.0 == a.0 => (b.0, b.1 + q.1 - a.1),
            Edit::Insert(a, b) => (q.0 + b.0 - a.0, q.1),
            Edit::Delete(a, _) if q <= a => q,
            Edit::Delete(a, b) if q < b => a,
            Edit::Delete(a, b) if q.0 == b.0 => (a.0, a.1 + q.1 - b.1),
            Edit::Delete(a, b) => (q.0 - (b.0 - a.0), q.1),
        }
    }
}

fn blank(c: char) -> bool {
    c == ' ' || c == '\t'
}

impl Text {
    pub fn from_lines<S: AsRef<str>>(lines: &[S]) -> Text {
        Text {
            lines: lines.iter().map(|l| l.as_ref().chars().collect()).collect(),
            edits: Vec::new(),
        }
    }

    pub fn line(&self, n: usize) -> &[char] {
        self.lines.get(n).map_or(&[][..], |l| l)
    }

    /// The line `n` as a string ("" at the end of the buffer).
    pub fn string(&self, n: usize) -> String {
        self.line(n).iter().collect()
    }

    /// Typing a character: inserted, or over the one at `p` (overstrike,
    /// not past the line's end). At the end of the buffer it starts a line.
    pub fn type_char(&mut self, p: Pos, c: char, over: bool) -> Pos {
        if p.0 == self.lines.len() {
            self.lines.push(Vec::new());
            self.edits.push(Edit::Insert(p, (p.0 + 1, 0)));
        }
        let line = &mut self.lines[p.0];
        if over && p.1 < line.len() {
            line[p.1] = c;
        } else {
            let at = (p.0, p.1.min(line.len()));
            line.insert(at.1, c);
            self.edits.push(Edit::Insert(at, (at.0, at.1 + 1)));
        }
        (p.0, p.1 + 1)
    }

    pub fn insert_str(&mut self, p: Pos, s: &str) -> Pos {
        // Whole lines at the end of the buffer go in before it.
        if p.0 == self.lines.len() && s.ends_with('\n') {
            self.lines
                .extend(s[..s.len() - 1].split('\n').map(|l| l.chars().collect()));
            self.edits.push(Edit::Insert(p, (self.lines.len(), 0)));
            return (self.lines.len(), 0);
        }
        let mut p = p;
        for c in s.chars() {
            p = match c {
                '\n' => self.split(p),
                c => self.type_char(p, c, false),
            };
        }
        p
    }

    /// Return: the line breaks at `p`.
    pub fn split(&mut self, p: Pos) -> Pos {
        if p.0 == self.lines.len() {
            self.lines.push(Vec::new());
            self.edits.push(Edit::Insert(p, (p.0 + 1, 0)));
            return (p.0 + 1, 0);
        }
        let at = p.1.min(self.lines[p.0].len());
        let rest = self.lines[p.0].split_off(at);
        self.lines.insert(p.0 + 1, rest);
        self.edits.push(Edit::Insert((p.0, at), (p.0 + 1, 0)));
        (p.0 + 1, 0)
    }

    /// The text from `a` to `b` (a before b), lines joined by '\n'.
    pub fn get(&self, a: Pos, b: Pos) -> String {
        let mut s = String::new();
        let mut p = a;
        while p < b {
            if p.0 >= self.lines.len() {
                break;
            }
            let line = &self.lines[p.0];
            if p.0 == b.0 {
                s.extend(&line[p.1.min(line.len())..b.1.min(line.len())]);
                break;
            }
            s.extend(&line[p.1.min(line.len())..]);
            s.push('\n');
            p = (p.0 + 1, 0);
        }
        s
    }

    /// Deletes from `a` to `b` (a before b); returns what it took.
    pub fn delete(&mut self, a: Pos, b: Pos) -> String {
        let got = self.get(a, b);
        if a >= b || a.0 >= self.lines.len() {
            return got;
        }
        let b = if b.0 >= self.lines.len() {
            (self.lines.len() - 1, self.lines[self.lines.len() - 1].len())
        } else {
            b
        };
        let tail: Vec<char> = self.lines[b.0][b.1.min(self.lines[b.0].len())..].to_vec();
        let head = &mut self.lines[a.0];
        head.truncate(a.1.min(head.len()));
        head.extend(tail);
        self.lines.drain(a.0 + 1..=b.0);
        self.edits.push(Edit::Delete(a, b));
        // What was taken up to the end of the buffer leaves no empty last line.
        if got.ends_with('\n')
            && a.1 == 0
            && self.lines.get(a.0).is_some_and(|l| l.is_empty())
            && a.0 == self.lines.len() - 1
        {
            self.lines.pop();
        }
        got
    }

    /// The position after `p`: the next character, or the next line's start.
    pub fn next(&self, p: Pos) -> Pos {
        if p.0 >= self.lines.len() {
            p
        } else if p.1 < self.lines[p.0].len() {
            (p.0, p.1 + 1)
        } else {
            (p.0 + 1, 0)
        }
    }

    pub fn prev(&self, p: Pos) -> Pos {
        if p.1 > 0 {
            (p.0, p.1 - 1)
        } else if p.0 > 0 {
            (p.0 - 1, self.line(p.0 - 1).len())
        } else {
            p
        }
    }

    /// ERASE WORD's extent: on a word, from its start through the blanks
    /// after it; on blanks, from there through the next word and its
    /// blanks; at a line's end, the line break and the next line's
    /// leading blanks.
    pub fn word_extent(&self, p: Pos) -> (Pos, Pos) {
        let line = self.line(p.0);
        if p.1 >= line.len() {
            let next = self.line(p.0 + 1);
            let lead = next.iter().take_while(|c| blank(**c)).count();
            return (p, (p.0 + 1, lead));
        }
        let mut a = p.1;
        if !blank(line[a]) {
            while a > 0 && !blank(line[a - 1]) {
                a -= 1;
            }
        }
        let mut b = p.1;
        while b < line.len() && blank(line[b]) {
            b += 1;
        }
        while b < line.len() && !blank(line[b]) {
            b += 1;
        }
        while b < line.len() && blank(line[b]) {
            b += 1;
        }
        ((p.0, a), (p.0, b))
    }

    /// The start of the word at or after `p` (forward) or before it.
    pub fn word_move(&self, p: Pos, forward: bool) -> Pos {
        let line = self.line(p.0);
        if forward {
            if p.1 >= line.len() {
                return (p.0 + 1, 0).min((self.lines.len(), 0));
            }
            let mut b = p.1;
            while b < line.len() && !blank(line[b]) {
                b += 1;
            }
            while b < line.len() && blank(line[b]) {
                b += 1;
            }
            (p.0, b)
        } else {
            if p.1 == 0 {
                return if p.0 == 0 {
                    p
                } else {
                    (p.0 - 1, self.line(p.0 - 1).len())
                };
            }
            let mut a = p.1;
            while a > 0 && blank(line[a - 1]) {
                a -= 1;
            }
            while a > 0 && !blank(line[a - 1]) {
                a -= 1;
            }
            (p.0, a)
        }
    }

    /// The word at `p` (start, end), not its blanks.
    pub fn word_at(&self, p: Pos) -> (usize, usize) {
        let line = self.line(p.0);
        let mut a = p.1.min(line.len());
        while a < line.len() && blank(line[a]) {
            a += 1;
        }
        while a > 0 && a <= line.len() && !blank(line[a - 1]) {
            a -= 1;
        }
        let mut b = a;
        while b < line.len() && !blank(line[b]) {
            b += 1;
        }
        (a, b)
    }

    /// Where `what` is next found from `p` (at `p` itself too), or before
    /// it, by `matches` at each position. Case-blind when `what` has no
    /// capitals, as EVE searches.
    pub fn find(&self, p: Pos, what: &str, forward: bool) -> Option<(Pos, Pos)> {
        let pat: Vec<char> = what.chars().collect();
        if pat.is_empty() {
            return None;
        }
        let blind = !what.chars().any(|c| c.is_uppercase());
        let eq = |a: char, b: char| {
            if blind {
                a.to_lowercase().eq(b.to_lowercase())
            } else {
                a == b
            }
        };
        // The text as one char sequence with '\n' between lines.
        let at = |q: Pos| -> Option<Pos> {
            let mut r = q;
            for &c in &pat {
                if r.0 >= self.lines.len() {
                    return None;
                }
                let line = &self.lines[r.0];
                if r.1 < line.len() {
                    if !eq(line[r.1], c) {
                        return None;
                    }
                    r = (r.0, r.1 + 1);
                } else if c == '\n' {
                    r = (r.0 + 1, 0);
                } else {
                    return None;
                }
            }
            Some(r)
        };
        let mut q = p;
        loop {
            if let Some(end) = at(q) {
                return Some((q, end));
            }
            let n = if forward { self.next(q) } else { self.prev(q) };
            if n == q {
                return None;
            }
            q = n;
        }
    }

    /// WILDCARD FIND: `*` any characters on a line, `%` one.
    pub fn wildcard(&self, p: Pos, what: &str, forward: bool) -> Option<(Pos, Pos)> {
        let pat: Vec<char> = what.to_lowercase().chars().collect();
        fn m(t: &[char], p: &[char]) -> Option<usize> {
            match p.split_first() {
                None => Some(0),
                Some(('*', rest)) => (0..=t.len())
                    .rev()
                    .find_map(|i| m(&t[i..], rest).map(|n| n + i)),
                Some((c, rest)) => {
                    let first = t.first()?;
                    if *c == '%' || first.to_lowercase().eq(c.to_lowercase()) {
                        m(&t[1..], rest).map(|n| n + 1)
                    } else {
                        None
                    }
                }
            }
        }
        let mut q = p;
        loop {
            if q.0 < self.lines.len() {
                let line = &self.lines[q.0];
                if q.1 <= line.len()
                    && let Some(n) = m(&line[q.1..], &pat).filter(|n| *n > 0)
                {
                    return Some((q, (q.0, q.1 + n)));
                }
            }
            let n = if forward { self.next(q) } else { self.prev(q) };
            if n == q {
                return None;
            }
            q = n;
        }
    }

    /// FILL: the lines `a..=b` as words joined by single spaces (two after
    /// a sentence's end, as EVE leaves them), broken before `right`
    /// columns, starting at column `left` (1-based).
    pub fn fill(&mut self, a: usize, b: usize, left: usize, right: usize) {
        let indent: String = " ".repeat(left.saturating_sub(1));
        let mut out: Vec<Vec<char>> = Vec::new();
        let mut cur = String::new();
        for n in a..=b {
            let line = self.string(n);
            // A blank line ends a paragraph and stays.
            if line.trim().is_empty() {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur).chars().collect());
                }
                out.push(self.line(n).to_vec());
                continue;
            }
            for w in line.split_whitespace() {
                if !cur.is_empty() && cur.chars().count() + 1 + w.chars().count() > right {
                    out.push(std::mem::take(&mut cur).chars().collect());
                }
                if cur.is_empty() {
                    cur = format!("{indent}{w}");
                } else {
                    cur.push(' ');
                    cur.push_str(w);
                }
            }
        }
        if !cur.is_empty() {
            out.push(cur.chars().collect());
        }
        self.lines.splice(a..=b, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_as_eve_erases_them() {
        // fixtures/eve: ERASE WORD at a word, on blanks, inside a word.
        let t = Text::from_lines(&["alpha beta  gamma, delta.epsilon", "  indented", "last"]);
        assert_eq!(t.word_extent((0, 0)), ((0, 0), (0, 6)));
        assert_eq!(t.word_extent((0, 7)), ((0, 6), (0, 12)));
        assert_eq!(t.word_extent((0, 10)), ((0, 10), (0, 19)));
        assert_eq!(t.word_extent((0, 32)), ((0, 32), (1, 2)));
        let mut t = t;
        t.delete((0, 6), (0, 12));
        assert_eq!(t.string(0), "alpha gamma, delta.epsilon");
        assert_eq!(t.find((0, 0), "DELTA", true), None);
        assert_eq!(t.find((0, 0), "delta", true), Some(((0, 13), (0, 18))));
        assert_eq!(t.wildcard((0, 0), "g*,", true), Some(((0, 6), (0, 12))));
    }
}
