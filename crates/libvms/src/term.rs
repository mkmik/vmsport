//! The terminal for full-screen programs (the keypad editors, the FDL
//! editor): raw mode with the VT keypad in application mode, and keys as
//! VT100/VT220 terminals (and xterm-likes) send them.

use std::io::{Read, Write};

/// A key as the terminal sends it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Key {
    Char(char),
    /// Ctrl/A .. Ctrl/Z and the like, as their letter: Ctrl('Z').
    Ctrl(char),
    Return,
    Tab,
    /// BS (Ctrl/H).
    Backspace,
    /// DEL, the key VT terminals mark <X].
    Delete,
    Escape,
    Up,
    Down,
    Left,
    Right,
    /// PF1 .. PF4 (F1 .. F4 on most keyboards send the same).
    Pf(u8),
    /// The numeric keypad in application mode: '0'..'9', ',', '-', '.'.
    Kp(char),
    KpEnter,
    /// The VT220 editing keypad.
    Find,
    InsertHere,
    Remove,
    Select,
    PrevScreen,
    NextScreen,
    /// F6 .. F20; F15 is Help and F16 is Do on a VT220.
    F(u8),
    Unknown(Vec<u8>),
}

/// The key `bytes` start with, and how many bytes it took; `None` when
/// they are an incomplete sequence.
pub fn decode(bytes: &[u8]) -> Option<(Key, usize)> {
    let b = *bytes.first()?;
    let key = match b {
        b'\r' | b'\n' => Key::Return,
        b'\t' => Key::Tab,
        0x08 => Key::Backspace,
        0x7F => Key::Delete,
        0x1B => return escape(bytes),
        1..=26 => Key::Ctrl((b'A' + b - 1) as char),
        0..=31 => Key::Ctrl((b'@' + b) as char),
        _ => {
            // One UTF-8 character.
            let n = match b {
                0xF0.. => 4,
                0xE0.. => 3,
                0xC0.. => 2,
                _ => 1,
            };
            let s = std::str::from_utf8(bytes.get(..n)?).ok();
            return Some((
                s.and_then(|s| s.chars().next())
                    .map_or(Key::Unknown(bytes[..n].to_vec()), Key::Char),
                n,
            ));
        }
    };
    Some((key, 1))
}

fn escape(bytes: &[u8]) -> Option<(Key, usize)> {
    let Some(&kind) = bytes.get(1) else {
        // ESC alone: the reader decides, after a pause, that it is Escape.
        return None;
    };
    match kind {
        b'O' => {
            let c = *bytes.get(2)?;
            let key = match c {
                b'A' => Key::Up,
                b'B' => Key::Down,
                b'C' => Key::Right,
                b'D' => Key::Left,
                b'P'..=b'S' => Key::Pf(c - b'P' + 1),
                b'p'..=b'y' => Key::Kp((c - b'p' + b'0') as char),
                b'l' => Key::Kp(','),
                b'm' => Key::Kp('-'),
                b'n' => Key::Kp('.'),
                b'M' => Key::KpEnter,
                _ => Key::Unknown(bytes[..3].to_vec()),
            };
            Some((key, 3))
        }
        b'[' => {
            // CSI: parameters, then a final byte.
            let end = bytes[2..].iter().position(|c| (0x40..=0x7E).contains(c))? + 2;
            let params = std::str::from_utf8(&bytes[2..end]).unwrap_or("");
            let n: u8 = params
                .split(';')
                .next()
                .and_then(|p| p.parse().ok())
                .unwrap_or(0);
            let key = match (bytes[end], n) {
                (b'A', _) => Key::Up,
                (b'B', _) => Key::Down,
                (b'C', _) => Key::Right,
                (b'D', _) => Key::Left,
                (b'~', 1) => Key::Find,
                (b'~', 2) => Key::InsertHere,
                (b'~', 3) => Key::Remove,
                (b'~', 4) => Key::Select,
                (b'~', 5) => Key::PrevScreen,
                (b'~', 6) => Key::NextScreen,
                (b'~', 17..=21) => Key::F(n - 11),
                (b'~', 23..=26) => Key::F(n - 12),
                (b'~', 28..=29) => Key::F(n - 13),
                (b'~', 31..=34) => Key::F(n - 14),
                _ => Key::Unknown(bytes[..=end].to_vec()),
            };
            Some((key, end + 1))
        }
        _ => Some((Key::Escape, 1)),
    }
}

/// Keys from a byte stream. An ESC that nothing follows within the
/// terminal's pause (VTIME) is Escape.
pub struct Keys<R> {
    r: R,
    buf: Vec<u8>,
}

impl<R: Read> Keys<R> {
    pub fn new(r: R) -> Keys<R> {
        Keys { r, buf: Vec::new() }
    }
}

impl<R: Read> Iterator for Keys<R> {
    type Item = Key;

    fn next(&mut self) -> Option<Key> {
        loop {
            if let Some((k, n)) = decode(&self.buf) {
                self.buf.drain(..n);
                return Some(k);
            }
            let mut b = [0u8; 64];
            match self.r.read(&mut b) {
                Ok(0) | Err(_) if self.buf.is_empty() => return None,
                Ok(0) | Err(_) => {
                    // A sequence cut short: what there is, as typed.
                    let k = match self.buf.as_slice() {
                        [0x1B] => Key::Escape,
                        b => Key::Unknown(b.to_vec()),
                    };
                    self.buf.clear();
                    return Some(k);
                }
                Ok(n) => self.buf.extend_from_slice(&b[..n]),
            }
        }
    }
}

/// The terminal in raw mode, its keypad and cursor keys in application
/// mode, until dropped. Reads return what came within 0.1 s, so a lone
/// ESC ends a read.
pub struct Raw(libc::termios, bool);

impl Raw {
    pub fn new() -> std::io::Result<Raw> {
        let raw = Raw::plain()?;
        // Application keypad and cursor keys.
        print!("\x1b=\x1b[?1h");
        let _ = std::io::stdout().flush();
        Ok(Raw(raw.0, true))
    }

    /// Raw mode, the keypad left as it is: for a program that reads lines
    /// its own way (Ctrl/Z ends one, say).
    pub fn plain() -> std::io::Result<Raw> {
        // SAFETY: tcgetattr/tcsetattr on stdin with a termios we own.
        let old = unsafe {
            let mut t: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(0, &mut t) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            let old = t;
            t.c_lflag &= !(libc::ICANON | libc::ECHO | libc::ISIG | libc::IEXTEN);
            t.c_iflag &= !(libc::IXON | libc::ICRNL | libc::INLCR);
            t.c_oflag &= !libc::OPOST;
            t.c_cc[libc::VMIN] = 0;
            t.c_cc[libc::VTIME] = 1;
            libc::tcsetattr(0, libc::TCSANOW, &t);
            old
        };
        Ok(Raw(old, false))
    }
}

impl Drop for Raw {
    fn drop(&mut self) {
        if self.1 {
            print!("\x1b>\x1b[?1l");
            let _ = std::io::stdout().flush();
        }
        // SAFETY: restores the settings we saved.
        unsafe { libc::tcsetattr(0, libc::TCSANOW, &self.0) };
    }
}

/// Keys from the terminal, waiting for each (Raw's reads time out).
pub fn keys() -> Keys<impl Read> {
    struct Stdin;
    impl Read for Stdin {
        fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
            loop {
                match std::io::stdin().read(b)? {
                    // A timeout with nothing typed: wait on.
                    0 if libc_isatty() => continue,
                    n => return Ok(n),
                }
            }
        }
    }
    fn libc_isatty() -> bool {
        // SAFETY: a plain query on fd 0.
        unsafe { libc::isatty(0) == 1 }
    }
    Keys::new(Stdin)
}

/// The terminal's rows and columns ($LINES and $COLUMNS first), 24 by 80
/// when it won't say.
pub fn size() -> (usize, usize) {
    let env = |v: &str| std::env::var(v).ok().and_then(|s| s.parse().ok());
    // SAFETY: TIOCGWINSZ fills a winsize.
    let (r, c) = unsafe {
        let mut ws: libc::winsize = std::mem::zeroed();
        match libc::ioctl(1, libc::TIOCGWINSZ, &mut ws) == 0 && ws.ws_row > 0 {
            true => (ws.ws_row as usize, ws.ws_col as usize),
            false => (24, 80),
        }
    };
    (env("LINES").unwrap_or(r), env("COLUMNS").unwrap_or(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_as_terminals_send_them() {
        let all: Vec<Key> = Keys::new(
            &b"a\x1bOP\x1bOw\x1bOM\x1b[A\x1b[1~\x1b[6~\x1b[29~\x1b[28~\x1b[17~\x1a\x7f\r\xc3\xa9\x1b"[..],
        )
        .collect();
        assert_eq!(
            all,
            [
                Key::Char('a'),
                Key::Pf(1),
                Key::Kp('7'),
                Key::KpEnter,
                Key::Up,
                Key::Find,
                Key::NextScreen,
                Key::F(16),
                Key::F(15),
                Key::F(6),
                Key::Ctrl('Z'),
                Key::Delete,
                Key::Return,
                Key::Char('é'),
                Key::Escape,
            ]
        );
    }
}
