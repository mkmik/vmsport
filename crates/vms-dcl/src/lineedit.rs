//! Command line editing and recall at the terminal, the DCL keys where
//! they don't fight Unix habits: arrows and Ctrl/B recall, Ctrl/A and
//! Ctrl/E (or Home, End) move to the ends, Ctrl/U deletes to the start,
//! Ctrl/Z (or Ctrl/D on an empty line) ends input, Ctrl/C cancels the line.

use std::io::{Read, Write};

/// The terminal in raw mode until dropped.
struct Raw(libc::termios);

impl Raw {
    fn new() -> Option<Raw> {
        // SAFETY: tcgetattr/tcsetattr on stdin with a termios we own.
        unsafe {
            let mut t: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(0, &mut t) != 0 {
                return None;
            }
            let old = t;
            t.c_lflag &= !(libc::ICANON | libc::ECHO | libc::ISIG | libc::IEXTEN);
            t.c_iflag &= !(libc::IXON | libc::ICRNL);
            t.c_cc[libc::VMIN] = 1;
            t.c_cc[libc::VTIME] = 0;
            libc::tcsetattr(0, libc::TCSANOW, &t);
            Some(Raw(old))
        }
    }
}

impl Drop for Raw {
    fn drop(&mut self) {
        // SAFETY: restores the settings we saved.
        unsafe { libc::tcsetattr(0, libc::TCSANOW, &self.0) };
    }
}

#[derive(Default)]
pub struct Editor {
    history: Vec<String>,
}

fn byte() -> Option<u8> {
    let mut b = [0u8];
    match std::io::stdin().read(&mut b) {
        Ok(1) => Some(b[0]),
        _ => None,
    }
}

impl Editor {
    /// Reads a line with editing; `None` at end of input.
    pub fn read(&mut self, prompt: &str) -> Option<String> {
        let _raw = Raw::new()?;
        let mut out = std::io::stdout();
        let mut line: Vec<char> = Vec::new();
        let mut pos = 0;
        let mut recall = self.history.len();
        let redraw = |line: &[char], pos: usize, out: &mut std::io::Stdout| {
            let text: String = line.iter().collect();
            let back = line.len() - pos;
            let _ = write!(out, "\r{prompt}{text}\x1b[K");
            if back > 0 {
                let _ = write!(out, "\x1b[{back}D");
            }
            let _ = out.flush();
        };
        redraw(&line, pos, &mut out);
        loop {
            let b = byte()?;
            match b {
                b'\r' | b'\n' => {
                    let _ = write!(out, "\r\n");
                    let s: String = line.iter().collect();
                    if !s.trim().is_empty() && self.history.last() != Some(&s) {
                        self.history.push(s.clone());
                    }
                    return Some(s);
                }
                0x1A => {
                    let _ = write!(out, "\r\n");
                    return None;
                }
                0x04 if line.is_empty() => {
                    let _ = write!(out, "\r\n");
                    return None;
                }
                0x03 | 0x19 => {
                    let _ = write!(
                        out,
                        "{}\r\n",
                        if b == 0x03 {
                            " *Cancel*"
                        } else {
                            " *Interrupt*"
                        }
                    );
                    line.clear();
                    pos = 0;
                }
                0x7F | 0x08 if pos > 0 => {
                    pos -= 1;
                    line.remove(pos);
                }
                0x01 => pos = 0,
                0x05 => pos = line.len(),
                0x15 => {
                    line.drain(..pos);
                    pos = 0;
                }
                0x02 => self.step(&mut recall, -1, &mut line, &mut pos),
                0x1B => match (byte()?, byte()?) {
                    (b'[', b'A') => self.step(&mut recall, -1, &mut line, &mut pos),
                    (b'[', b'B') => self.step(&mut recall, 1, &mut line, &mut pos),
                    (b'[', b'C') if pos < line.len() => pos += 1,
                    (b'[', b'D') if pos > 0 => pos -= 1,
                    (b'[', b'H') => pos = 0,
                    (b'[', b'F') => pos = line.len(),
                    (b'[', b'3') => {
                        byte()?; // the ~
                        if pos < line.len() {
                            line.remove(pos);
                        }
                    }
                    _ => {}
                },
                b if b >= 0x20 => {
                    // UTF-8: gather the rest of the character.
                    let n = match b {
                        0xC0..=0xDF => 1,
                        0xE0..=0xEF => 2,
                        0xF0..=0xF7 => 3,
                        _ => 0,
                    };
                    let mut buf = vec![b];
                    for _ in 0..n {
                        buf.push(byte()?);
                    }
                    for c in String::from_utf8_lossy(&buf).chars() {
                        line.insert(pos, c);
                        pos += 1;
                    }
                }
                _ => {}
            }
            redraw(&line, pos, &mut out);
        }
    }

    /// Recalls the previous (`-1`) or next (`1`) command.
    fn step(&self, recall: &mut usize, dir: isize, line: &mut Vec<char>, pos: &mut usize) {
        let next = *recall as isize + dir;
        if next < 0 || next as usize > self.history.len() {
            return;
        }
        *recall = next as usize;
        *line = self
            .history
            .get(*recall)
            .map(|s| s.chars().collect())
            .unwrap_or_default();
        *pos = line.len();
    }
}
