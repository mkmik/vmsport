//! The screen as EVE draws it, as a grid a test can read, and the VT100
//! sequences that bring a terminal from one grid to the next.

/// How a cell is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Attr {
    #[default]
    Normal,
    Reverse,
    Bold,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grid {
    pub cells: Vec<Vec<(char, Attr)>>,
    /// Where the cursor is, 0-based row and column.
    pub cursor: (usize, usize),
}

impl Grid {
    pub fn new(rows: usize, cols: usize) -> Grid {
        Grid {
            cells: vec![vec![(' ', Attr::Normal); cols]; rows],
            cursor: (0, 0),
        }
    }

    pub fn rows(&self) -> usize {
        self.cells.len()
    }

    pub fn cols(&self) -> usize {
        self.cells.first().map_or(0, |r| r.len())
    }

    /// Writes `s` at (row, col), clipped at the edge.
    pub fn put(&mut self, row: usize, col: usize, s: &str, a: Attr) {
        let Some(line) = self.cells.get_mut(row) else {
            return;
        };
        for (i, c) in s.chars().enumerate() {
            if let Some(cell) = line.get_mut(col + i) {
                *cell = (c, a);
            }
        }
    }

    /// A row's text, without trailing blanks.
    pub fn text(&self, row: usize) -> String {
        let s: String = self.cells[row].iter().map(|c| c.0).collect();
        s.trim_end().to_string()
    }

    /// The whole screen as text, a line per row.
    pub fn dump(&self) -> String {
        (0..self.rows()).map(|r| self.text(r) + "\n").collect()
    }

    /// VT100 output turning `old` (None: an unknown screen) into this one.
    pub fn draw(&self, old: Option<&Grid>) -> String {
        let mut out = String::new();
        if old.is_none() {
            out.push_str("\x1b[m\x1b[2J");
        }
        for r in 0..self.rows() {
            if old.is_some_and(|o| o.cells.get(r) == Some(&self.cells[r])) {
                continue;
            }
            out.push_str(&format!("\x1b[{};1H", r + 1));
            let mut attr = Attr::Normal;
            let row = &self.cells[r];
            let end = row
                .iter()
                .rposition(|c| *c != (' ', Attr::Normal))
                .map_or(0, |i| i + 1);
            for &(c, a) in &row[..end] {
                if a != attr {
                    out.push_str(match a {
                        Attr::Normal => "\x1b[m",
                        Attr::Reverse => "\x1b[;7m",
                        Attr::Bold => "\x1b[;1m",
                    });
                    attr = a;
                }
                // DEC special graphics: the diamond of a line that goes on
                // past the edge, and the symbols for FF, CR, LF and VT.
                match GRAPHICS.iter().find(|g| g.0 == c) {
                    Some(&(_, g)) => out.push_str(&format!("\x0e{g}\x0f")),
                    None => out.push(c),
                }
            }
            if attr != Attr::Normal {
                out.push_str("\x1b[m");
            }
            out.push_str("\x1b[K");
        }
        out.push_str(&format!(
            "\x1b[{};{}H",
            self.cursor.0 + 1,
            self.cursor.1 + 1
        ));
        out
    }
}

/// Characters drawn from the DEC special graphics set, and their codes.
pub const GRAPHICS: [(char, char); 5] = [
    ('\u{25c6}', '`'),
    ('\u{240c}', 'c'),
    ('\u{240d}', 'd'),
    ('\u{240a}', 'e'),
    ('\u{240b}', 'i'),
];

/// Setting the terminal up for a full screen, and back.
pub const START: &str = "\x1b)0\x1b[m\x1b[2J\x1b[?7l";
pub const END: &str = "\x1b[?7h\x1b[m";
