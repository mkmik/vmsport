//! `$FAO`: formatted ASCII output, as used by `$PUTMSG`, `F$FAO` and friends.
//!
//! Widths, truncation and overflow follow what VMS does (see
//! fixtures/fao/recorded): numbers that don't fit print as `*`, hex and octal
//! keep their rightmost digits, strings pad or truncate on the right.

use std::fmt::Write;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arg<'a> {
    Num(i64),
    Str(&'a str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Unknown directive (`SS$_BADPARAM`).
    BadParam,
    /// A string directive got a number, or a number directive a string.
    BadArg,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(match self {
            Error::BadParam => "bad parameter value",
            Error::BadArg => "missing or mistyped argument",
        })
    }
}

impl std::error::Error for Error {}

struct Args<'a, 'b> {
    ctl: &'a str,
    args: &'b [Arg<'a>],
    i: usize,
    last_num: Option<i64>,
}

impl<'a> Args<'a, '_> {
    /// Missing numeric arguments read as 0, as $FAO's zero-filled list does.
    fn num(&mut self) -> Result<i64, Error> {
        let v = match self.args.get(self.i) {
            Some(Arg::Num(n)) => *n,
            Some(Arg::Str(_)) => return Err(Error::BadArg),
            None => 0,
        };
        self.i += 1;
        self.last_num = Some(v);
        Ok(v)
    }

    /// When the arguments run out, the control string stands in, as on VMS.
    fn str(&mut self) -> Result<&'a str, Error> {
        match self.args.get(self.i) {
            Some(Arg::Str(s)) => {
                self.i += 1;
                Ok(s)
            }
            Some(Arg::Num(_)) => Err(Error::BadArg),
            None => Ok(self.ctl),
        }
    }
}

/// Formats `ctl` with `args`.
pub fn fao<'a>(ctl: &'a str, args: &[Arg<'a>]) -> Result<String, Error> {
    let mut a = Args {
        ctl,
        args,
        i: 0,
        last_num: None,
    };
    let cs: Vec<char> = ctl.chars().collect();
    let mut out = String::new();
    // Open `!n<` fields: (start of field in out, width).
    let mut fields: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < cs.len() {
        if cs[i] != '!' {
            out.push(cs[i]);
            i += 1;
            continue;
        }
        i += 1;
        // Repeat count or width: digits, or # for the next argument.
        let mut width = None;
        if cs.get(i) == Some(&'#') {
            width = Some(a.num()?.max(0) as usize);
            i += 1;
        } else {
            let start = i;
            while cs.get(i).is_some_and(|c| c.is_ascii_digit()) {
                i += 1;
            }
            if i > start {
                width = Some(
                    cs[start..i]
                        .iter()
                        .collect::<String>()
                        .parse()
                        .map_err(|_| Error::BadParam)?,
                );
            }
        }
        match cs.get(i) {
            Some('(') => {
                // !n(DD): the directive DD, n times.
                let end = cs[i..]
                    .iter()
                    .position(|&c| c == ')')
                    .ok_or(Error::BadParam)?
                    + i;
                let inner: String = cs[i + 1..end].iter().collect();
                for _ in 0..width.unwrap_or(1) {
                    directive(&inner, &mut a, &mut out, &mut fields)?;
                }
                i = end + 1;
            }
            Some('<') => {
                fields.push((out.len(), width.ok_or(Error::BadParam)?));
                i += 1;
            }
            Some('*') => {
                let c = *cs.get(i + 1).ok_or(Error::BadParam)?;
                out.extend(std::iter::repeat_n(c, width.ok_or(Error::BadParam)?));
                i += 2;
            }
            _ => {
                let len = directive_len(&cs[i..]).ok_or(Error::BadParam)?;
                let d: String = cs[i..i + len].iter().collect();
                let w = width.map(|w| w.to_string()).unwrap_or_default();
                directive(&format!("{w}{d}"), &mut a, &mut out, &mut fields)?;
                i += len;
            }
        }
    }
    Ok(out)
}

fn directive_len(cs: &[char]) -> Option<usize> {
    match cs.first()? {
        '/' | '_' | '^' | '!' | '-' | '+' | '>' => Some(1),
        '%' => cs.get(1).map(|_| 2),
        _ => cs.get(1).map(|_| 2),
    }
}

/// One directive without its `!`, possibly with a width: `10AS`, `XL`, `%D`.
fn directive(
    d: &str,
    a: &mut Args,
    out: &mut String,
    fields: &mut Vec<(usize, usize)>,
) -> Result<(), Error> {
    let split = d
        .find(|c: char| !c.is_ascii_digit())
        .ok_or(Error::BadParam)?;
    let width: Option<usize> = if split > 0 {
        d[..split].parse().ok()
    } else {
        None
    };
    let d = &d[split..];
    match d {
        "/" => out.push_str("\r\n"),
        "_" => out.push('\t'),
        "^" => out.push('\x0c'),
        "!" => out.push('!'),
        "-" => a.i = a.i.saturating_sub(1),
        "+" => a.i += 1,
        ">" => {
            let (start, w) = fields.pop().ok_or(Error::BadParam)?;
            let field: String = out[start..].to_string();
            out.truncate(start);
            out.push_str(&pad_right(&field, Some(w)));
        }
        "AS" | "AC" | "AZ" => out.push_str(&pad_right(a.str()?, width)),
        "AD" | "AF" => {
            let n = a.num()?.max(0) as usize;
            let s: String = a.str()?.chars().take(n).collect();
            let s = if d == "AF" {
                s.chars()
                    .map(|c| if c.is_control() { '.' } else { c })
                    .collect()
            } else {
                s
            };
            out.push_str(&pad_right(&s, width));
        }
        "%S" => {
            let upper = out.chars().last().is_some_and(|c| c.is_uppercase());
            if a.last_num != Some(1) {
                out.push(if upper { 'S' } else { 's' });
            }
        }
        "%U" | "%I" => {
            let v = a.num()? as u32;
            out.push_str(&pad_right(
                &format!("[{:o},{:o}]", v >> 16, v & 0xffff),
                width,
            ));
        }
        "%D" | "%T" => {
            let t = a.num()?;
            let s = asctim(if t == 0 { now() } else { t }).ok_or(Error::BadParam)?;
            let s = if d == "%T" { &s[12..] } else { &s[..] };
            out.push_str(&pad_right(s, width));
        }
        _ => {
            let mut c = d.chars();
            let (kind, size) = (
                c.next().ok_or(Error::BadParam)?,
                c.next().ok_or(Error::BadParam)?,
            );
            let bits = match size {
                'B' => 8,
                'W' => 16,
                'L' => 32,
                'Q' => 64,
                _ => return Err(Error::BadParam),
            };
            if !"OXZUS".contains(kind) {
                return Err(Error::BadParam);
            }
            let v = a.num()?;
            out.push_str(&number(kind, bits, v, width));
        }
    }
    Ok(())
}

fn pad_right(s: &str, width: Option<usize>) -> String {
    match width {
        Some(w) => format!("{:<w$.w$}", s),
        None => s.to_string(),
    }
}

fn number(kind: char, bits: u32, v: i64, width: Option<usize>) -> String {
    let mask = if bits == 64 {
        u64::MAX
    } else {
        (1u64 << bits) - 1
    };
    let u = v as u64 & mask;
    let mut s = String::new();
    match kind {
        'O' | 'X' => {
            let natural = if kind == 'X' {
                bits as usize / 4
            } else {
                (bits as usize).div_ceil(3)
            };
            if kind == 'X' {
                write!(s, "{u:0natural$X}").unwrap();
            } else {
                write!(s, "{u:0natural$o}").unwrap();
            }
            return match width {
                Some(w) if w <= natural => s[natural - w..].to_string(),
                Some(w) => format!("{s:>w$}"),
                None => s,
            };
        }
        'S' => {
            let sh = 64 - bits;
            write!(s, "{}", ((u << sh) as i64) >> sh).unwrap();
        }
        _ => write!(s, "{u}").unwrap(),
    }
    match width {
        None => s,
        Some(w) if s.len() > w => {
            // VMS fills with asterisks, keeping the sign at the right.
            if s.starts_with('-') && w > 0 {
                format!("{}-", "*".repeat(w - 1))
            } else {
                "*".repeat(w)
            }
        }
        Some(w) if kind == 'Z' => format!("{s:0>w$}"),
        Some(w) => format!("{s:>w$}"),
    }
}

/// 100 ns ticks from 17-NOV-1858 (the VMS epoch) to 1-JAN-1970.
pub const UNIX_EPOCH: i64 = 35_067_168_000_000_000;

fn now() -> i64 {
    let d = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    UNIX_EPOCH + (d.as_nanos() / 100) as i64
}

const MONTHS: [&str; 12] = [
    "JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC",
];

/// `$ASCTIM` of an absolute VMS time: `dd-MMM-yyyy hh:mm:ss.cc`, the day
/// padded with a space. Delta (negative) times are not handled yet.
pub fn asctim(t: i64) -> Option<String> {
    if t < 0 {
        return None;
    }
    let cs = t / 100_000; // hundredths
    let (days, rem) = (cs / 8_640_000, cs % 8_640_000);
    // Days since 1858-11-17 to a civil date (Howard Hinnant's algorithm,
    // shifted to the 1970 epoch: 1858-11-17 is day -40587).
    let z = days - 40587 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    let (h, mi, s, c) = (rem / 360_000, rem / 6000 % 60, rem / 100 % 60, rem % 100);
    Some(format!(
        "{d:>2}-{}-{y} {h:02}:{mi:02}:{s:02}.{c:02}",
        MONTHS[m as usize - 1]
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use Arg::{Num, Str};

    #[test]
    fn basics() {
        assert_eq!(
            fao("!AS has !UL item!%S", &[Str("list"), Num(3)]).unwrap(),
            "list has 3 items"
        );
        assert_eq!(
            fao("!3(3UB)", &[Num(1), Num(2), Num(3)]).unwrap(),
            "  1  2  3"
        );
        assert_eq!(fao("!UL ITEM!%S", &[Num(2)]).unwrap(), "2 ITEMS");
        assert_eq!(fao("!Q", &[]), Err(Error::BadParam));
        assert_eq!(fao("!AS|", &[]).unwrap(), "!AS||");
        assert_eq!(fao("!UL", &[Arg::Str("x")]), Err(Error::BadArg));
    }

    #[test]
    fn time() {
        assert_eq!(asctim(0).unwrap(), "17-NOV-1858 00:00:00.00");
        assert_eq!(asctim(UNIX_EPOCH).unwrap(), " 1-JAN-1970 00:00:00.00");
        let t = UNIX_EPOCH + 951_782_400 * 10_000_000 + 12_345_600_000; // 29-FEB-2000 00:20:34.56
        assert_eq!(
            fao("!%D|!%T", &[Num(t), Num(t)]).unwrap(),
            "29-FEB-2000 00:20:34.56|00:20:34.56"
        );
        assert_eq!(fao("!11%D", &[Num(t)]).unwrap(), "29-FEB-2000");
    }
}
