//! VMS times: 64-bit counts of 100 ns since 17-NOV-1858 00:00 local time;
//! negative values are delta times. `$ASCTIM`, `$BINTIM` and `F$CVTIME`.
//!
//! Pure: whatever depends on the current time takes it as an argument.
//! Parsing and `F$CVTIME` follow what VMS does (fixtures/time/recorded).

use vms_cond::Cond;

/// 100 ns ticks in a second, a day.
pub const SECOND: i64 = 10_000_000;
pub const DAY: i64 = 86_400 * SECOND;

/// 100 ns ticks from 17-NOV-1858 (the VMS epoch) to 1-JAN-1970.
pub const UNIX_EPOCH: i64 = 35_067_168_000_000_000;

pub const MONTHS: [&str; 12] = [
    "JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC",
];

const WEEKDAYS: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];

/// A civil date and time, to the hundredth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Civil {
    pub year: i64,
    /// 1..=12
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    pub hundredth: u32,
}

// Days since 1970-01-01 <-> civil dates (Howard Hinnant's algorithms).
// The VMS epoch, 17-NOV-1858, is day -40587.
const EPOCH_DAY: i64 = -40587;

fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

pub fn is_leap(y: i64) -> bool {
    y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)
}

pub fn days_in_month(y: i64, m: u32) -> u32 {
    match m {
        2 if is_leap(y) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

impl Civil {
    /// The VMS time, if the fields are valid and the date is on or after
    /// the VMS epoch.
    pub fn to_vms(&self) -> Option<i64> {
        let ok = (1..=12).contains(&self.month)
            && (1..=days_in_month(self.year, self.month)).contains(&self.day)
            && self.hour < 24
            && self.minute < 60
            && self.second < 60
            && self.hundredth < 100;
        let days = days_from_civil(self.year, self.month, self.day) - EPOCH_DAY;
        let secs = (self.hour * 3600 + self.minute * 60 + self.second) as i64;
        (ok && days >= 0).then(|| days * DAY + secs * SECOND + self.hundredth as i64 * 100_000)
    }

    /// The civil time of an absolute VMS time (hundredths truncated).
    pub fn from_vms(t: i64) -> Civil {
        let (days, rem) = (t.div_euclid(DAY), t.rem_euclid(DAY) / 100_000);
        let (year, month, day) = civil_from_days(days + EPOCH_DAY);
        Civil {
            year,
            month,
            day,
            hour: (rem / 360_000) as u32,
            minute: (rem / 6000 % 60) as u32,
            second: (rem / 100 % 60) as u32,
            hundredth: (rem % 100) as u32,
        }
    }

    /// 1 for 1 January.
    pub fn day_of_year(&self) -> i64 {
        days_from_civil(self.year, self.month, self.day) - days_from_civil(self.year, 1, 1) + 1
    }

    pub fn weekday(&self) -> &'static str {
        // 1-JAN-1970 was a Thursday.
        WEEKDAYS[(days_from_civil(self.year, self.month, self.day) + 3).rem_euclid(7) as usize]
    }
}

/// `$ASCTIM`: `dd-MMM-yyyy hh:mm:ss.cc` (day padded with a space) for an
/// absolute time, `dddd hh:mm:ss.cc` for a delta; only `hh:mm:ss.cc` when
/// `time_only`.
pub fn asctim(t: i64, time_only: bool) -> String {
    let c = Civil::from_vms((t.unsigned_abs() % DAY as u64) as i64);
    let time = format!(
        "{:02}:{:02}:{:02}.{:02}",
        c.hour, c.minute, c.second, c.hundredth
    );
    if time_only {
        return time;
    }
    if t < 0 {
        return format!("{:>4} {time}", t.unsigned_abs() as i64 / DAY);
    }
    let c = Civil::from_vms(t);
    // VMS formats the year with a width of 4: later years print as ****.
    let year = if c.year > 9999 {
        "****".to_string()
    } else {
        c.year.to_string()
    };
    format!(
        "{:>2}-{}-{year} {time}",
        c.day,
        MONTHS[c.month as usize - 1]
    )
}

/// A time DCL rejects: `%DCL-W-IVATIME, text` and ` \TOKEN\`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub code: Cond,
    pub ident: &'static str,
    pub text: &'static str,
    pub token: Option<String>,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "%DCL-{}-{}, {}",
            self.code.severity_letter(),
            self.ident,
            self.text
        )?;
        if let Some(t) = &self.token {
            write!(f, "\n \\{t}\\")?;
        }
        Ok(())
    }
}

impl std::error::Error for Error {}

fn ivatime(s: &str) -> Error {
    let text = "invalid absolute time - use DD-MMM-YYYY:HH:MM:SS.CC format";
    Error {
        code: Cond(0x38290),
        ident: "IVATIME",
        text,
        token: Some(s.trim().to_uppercase()),
    }
}

fn ivdtime(s: &str) -> Error {
    let text = "invalid delta time - use DDDD-HH:MM:SS.CC format";
    Error {
        code: Cond(0x38298),
        ident: "IVDTIME",
        text,
        token: Some(s.trim().to_uppercase()),
    }
}

fn ivkeyw(s: &str) -> Error {
    let text = "unrecognized keyword - check validity and spelling";
    let token = (!s.is_empty()).then(|| s.to_uppercase());
    Error {
        code: Cond(0x38060),
        ident: "IVKEYW",
        text,
        token,
    }
}

/// A cursor over upcased time text.
struct Cur<'a> {
    s: &'a str,
    i: usize,
}

impl<'a> Cur<'a> {
    fn peek(&self) -> Option<u8> {
        self.s.as_bytes().get(self.i).copied()
    }

    fn eat(&mut self, c: u8) -> bool {
        let hit = self.peek() == Some(c);
        self.i += hit as usize;
        hit
    }

    fn take(&mut self, f: impl Fn(u8) -> bool) -> &'a str {
        let start = self.i;
        while self.peek().is_some_and(&f) {
            self.i += 1;
        }
        &self.s[start..self.i]
    }

    fn digits(&mut self) -> &'a str {
        self.take(|c| c.is_ascii_digit())
    }
}

/// Hundredths from the digits after the point, rounded: `.456` is 46.
fn hundredths(frac: &str) -> i64 {
    let d: String = frac.chars().chain("000".chars()).take(3).collect();
    (d.parse::<i64>().unwrap_or(0) + 5) / 10
}

fn num(s: &str) -> Option<i64> {
    if s.is_empty() {
        Some(0)
    } else {
        s.parse().ok()
    }
}

/// `hh:mm:ss.cc`, any part omitted (as 0): ticks.
fn time_of_day(c: &mut Cur) -> Option<i64> {
    let h = num(c.digits())?;
    let (mut m, mut s, mut frac) = (0, 0, "");
    if c.eat(b':') {
        m = num(c.digits())?;
        if c.eat(b':') {
            s = num(c.digits())?;
            if c.eat(b'.') {
                frac = c.digits();
            }
        }
    }
    (h < 24 && m < 60 && s < 60)
        .then(|| ((h * 60 + m) * 60 + s) * SECOND + hundredths(frac) * 100_000)
}

/// A delta time as typed: `dddd-hh:mm:ss.cc`, every part optional; a
/// number with no `-` after it is the hours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Delta<'a> {
    pub days: &'a str,
    pub hour: &'a str,
    pub minute: &'a str,
    pub second: &'a str,
    pub fraction: &'a str,
    /// The value: negative, as delta times are.
    pub ticks: i64,
}

fn delta<'a>(c: &mut Cur<'a>) -> Option<Delta<'a>> {
    let start = c.i;
    let mut days = c.digits();
    if !c.eat(b'-') {
        days = "";
        c.i = start;
    }
    let hour = c.digits();
    let (mut minute, mut second, mut fraction) = ("", "", "");
    if c.eat(b':') {
        minute = c.digits();
        if c.eat(b':') {
            second = c.digits();
            if c.eat(b'.') {
                fraction = c.digits();
            }
        }
    }
    let (d, h, m, s) = (num(days)?, num(hour)?, num(minute)?, num(second)?);
    let ok = d <= 9999 && h < 24 && m < 60 && s < 60;
    let ticks = -(d * DAY + ((h * 60 + m) * 60 + s) * SECOND + hundredths(fraction) * 100_000);
    ok.then_some(Delta {
        days,
        hour,
        minute,
        second,
        fraction,
        ticks,
    })
}

/// A DCL delta time (`F$CVTIME(t, "DELTA")`, `/INTERVAL=`).
pub fn parse_delta(s: &str) -> Result<Delta<'_>, Error> {
    let t = s.trim();
    let mut c = Cur { s: t, i: 0 };
    match delta(&mut c) {
        Some(d) if c.i == t.len() => Ok(d),
        _ => Err(ivdtime(s)),
    }
}

/// Two-digit years: 00-56 are 2000-2056, 57-99 are 1957-1999.
// ponytail: the pivot is the usual VMS one; only 00 and 99 are recorded.
fn full_year(y: &str) -> Option<i64> {
    let n: i64 = y.parse().ok()?;
    Some(match y.len() {
        1 | 2 if n < 57 => 2000 + n,
        1 | 2 => 1900 + n,
        _ => n,
    })
}

/// A DCL absolute or combination time: `dd-MMM-yyyy[:| ]hh:mm:ss.cc`,
/// `TODAY`, `TOMORROW` or `YESTERDAY` for the date, then optionally `+` or
/// `-` and a delta. Omitted date fields are today's, omitted time fields 0;
/// an empty string is `now`. A combination before 17-NOV-1858 comes out
/// negative, as on VMS.
pub fn parse_absolute(s: &str, now: i64) -> Result<i64, Error> {
    let up = s.trim().to_uppercase();
    if up.is_empty() {
        return Ok(now);
    }
    let bad = || ivatime(s);
    let today = Civil {
        hour: 0,
        minute: 0,
        second: 0,
        hundredth: 0,
        ..Civil::from_vms(now)
    };
    let mut c = Cur { s: &up, i: 0 };
    let mut date = None;
    let word = c.take(|b| b.is_ascii_alphabetic());
    if !word.is_empty() {
        let kws = [("TODAY", 0), ("TOMORROW", 1), ("YESTERDAY", -1)];
        let hits: Vec<_> = kws.iter().filter(|k| k.0.starts_with(word)).collect();
        let [(_, days)] = hits[..] else {
            return Err(bad());
        };
        date = Some(today.to_vms().ok_or_else(bad)? + days * DAY);
    } else {
        // A date is digits, `-`, then a month name, `-` or the end.
        let start = c.i;
        let day = c.digits();
        let is_date = c.eat(b'-')
            && c.peek()
                .is_none_or(|b| b.is_ascii_alphabetic() || b"-: ".contains(&b));
        if is_date {
            let month = c.take(|b| b.is_ascii_alphabetic());
            let year = if c.eat(b'-') { c.digits() } else { "" };
            let civil = Civil {
                day: if day.is_empty() {
                    today.day
                } else {
                    day.parse().map_err(|_| bad())?
                },
                month: if month.is_empty() {
                    today.month
                } else {
                    MONTHS.iter().position(|m| *m == month).ok_or_else(bad)? as u32 + 1
                },
                year: if year.is_empty() {
                    today.year
                } else {
                    full_year(year).ok_or_else(bad)?
                },
                ..today
            };
            if civil.year > 9999 {
                return Err(bad());
            }
            date = Some(civil.to_vms().ok_or_else(bad)?);
        } else {
            c.i = start;
        }
    }
    // The time: after a date, only past a `:` or blanks.
    let mut t = date.unwrap_or(today.to_vms().ok_or_else(bad)?);
    let has_time = date.is_none() || c.eat(b':') || !c.take(|b| b == b' ').is_empty();
    if has_time {
        t += time_of_day(&mut c).ok_or_else(bad)?;
    }
    if let Some(op) = c.peek().filter(|b| b"+-".contains(b)) {
        c.i += 1;
        let d = delta(&mut c).ok_or_else(bad)?;
        t = if op == b'+' { t - d.ticks } else { t + d.ticks };
    }
    if c.i != up.len() {
        return Err(bad());
    }
    Ok(t)
}

/// `$BINTIM`: `$ASCTIM`'s formats back to a time; `dddd hh:mm:ss.cc` is a
/// delta, anything else goes through [`parse_absolute`].
pub fn bintim(s: &str, now: i64) -> Result<i64, Error> {
    if let Some((days, time)) = s.trim().split_once(' ')
        && !days.is_empty()
        && days.bytes().all(|b| b.is_ascii_digit())
    {
        return Ok(parse_delta(&format!("{days}-{}", time.trim()))?.ticks);
    }
    parse_absolute(s, now)
}

const FIELDS: [&str; 15] = [
    "DATETIME",
    "DATE",
    "TIME",
    "DAY",
    "HOUR",
    "MINUTE",
    "SECOND",
    "HUNDREDTH",
    "MONTH",
    "YEAR",
    "WEEKDAY",
    "DAYOFYEAR",
    "HOUROFYEAR",
    "MINUTEOFYEAR",
    "SECONDOFYEAR",
];

/// `F$CVTIME(input, format, field)`. `None` is an omitted argument (the
/// current time, COMPARISON, DATETIME); `now` is the current local time.
pub fn cvtime(
    input: Option<&str>,
    format: Option<&str>,
    field: Option<&str>,
    now: i64,
) -> Result<String, Error> {
    let format = format.map_or("COMPARISON".into(), str::to_uppercase);
    let field = field.map_or("DATETIME".into(), str::to_uppercase);
    let delta_fields = &FIELDS[..8];
    match format.as_str() {
        "DELTA" if !delta_fields.contains(&field.as_str()) => return Err(ivkeyw(&field)),
        "ABSOLUTE" | "COMPARISON" if !FIELDS.contains(&field.as_str()) => {
            return Err(ivkeyw(&field));
        }
        "DELTA" | "ABSOLUTE" | "COMPARISON" => {}
        _ => return Err(ivkeyw(&format)),
    }
    let input = input.unwrap_or("");
    if format == "DELTA" {
        // DCL edits the delta as typed rather than converting it.
        let d = parse_delta(input)?;
        let or = |s: &str, def: &str| {
            if s.is_empty() {
                def.to_string()
            } else {
                s.to_string()
            }
        };
        let time = format!(
            "{}:{}:{}.{}",
            or(d.hour, "0"),
            or(d.minute, "00"),
            or(d.second, "00"),
            or(d.fraction, "00")
        );
        let days = or(d.days, "0");
        return Ok(match field.as_str() {
            "DATETIME" => format!("{days} {time}"),
            "DATE" | "DAY" => days,
            "TIME" => time,
            "HOUR" => or(d.hour, "0"),
            "MINUTE" => or(d.minute, "00"),
            "SECOND" => or(d.second, "00"),
            _ => or(d.fraction, "00"),
        });
    }
    let t = parse_absolute(input, now)?;
    let abs = format == "ABSOLUTE";
    let c = Civil::from_vms(t);
    let time = asctim(t, true);
    let date = if abs {
        format!("{}-{}-{}", c.day, MONTHS[c.month as usize - 1], c.year)
    } else {
        format!("{}-{:02}-{:02}", c.year, c.month, c.day)
    };
    let (doy, h, m, sec) = (
        c.day_of_year(),
        c.hour as i64,
        c.minute as i64,
        c.second as i64,
    );
    Ok(match field.as_str() {
        // $ASCTIM without its leading blank: a negative time shows as a delta.
        "DATETIME" if abs || t < 0 => {
            let s = asctim(t, false);
            s.strip_prefix(' ').unwrap_or(&s).to_string()
        }
        "DATETIME" => format!("{date} {time}"),
        "DATE" => date,
        "TIME" => time,
        "DAY" if abs => c.day.to_string(),
        "DAY" => format!("{:02}", c.day),
        "MONTH" if abs => MONTHS[c.month as usize - 1].to_string(),
        "MONTH" => format!("{:02}", c.month),
        "YEAR" => c.year.to_string(),
        "HOUR" => format!("{h:02}"),
        "MINUTE" => format!("{m:02}"),
        "SECOND" => format!("{sec:02}"),
        "HUNDREDTH" => format!("{:02}", c.hundredth),
        "WEEKDAY" => c.weekday().to_string(),
        "DAYOFYEAR" => doy.to_string(),
        "HOUROFYEAR" => ((doy - 1) * 24 + h).to_string(),
        "MINUTEOFYEAR" => (((doy - 1) * 24 + h) * 60 + m).to_string(),
        _ => ((((doy - 1) * 24 + h) * 60 + m) * 60 + sec).to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calendar() {
        assert_eq!(asctim(0, false), "17-NOV-1858 00:00:00.00");
        assert_eq!(asctim(UNIX_EPOCH, false), " 1-JAN-1970 00:00:00.00");
        assert_eq!(
            asctim(-(2 * DAY + 3 * 3600 * SECOND + 50_000_000), false),
            "   2 03:00:05.00"
        );
        let c = Civil {
            year: 2000,
            month: 2,
            day: 29,
            hour: 0,
            minute: 20,
            second: 34,
            hundredth: 56,
        };
        let t = c.to_vms().unwrap();
        assert_eq!(Civil::from_vms(t), c);
        assert_eq!((c.day_of_year(), c.weekday()), (60, "Tuesday"));
        assert_eq!(Civil { day: 30, ..c }.to_vms(), None);
        assert_eq!(
            Civil {
                year: 1858,
                month: 11,
                day: 16,
                ..Default::default()
            }
            .to_vms(),
            None
        );
    }

    #[test]
    fn bintim_reads_asctim() {
        for t in [
            0,
            UNIX_EPOCH + 12_345_600_000,
            -(3 * DAY + 4 * SECOND + 500_000),
        ] {
            assert_eq!(
                bintim(&asctim(t, false), 0).unwrap(),
                t,
                "{}",
                asctim(t, false)
            );
        }
    }
}
