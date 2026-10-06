//! 32-bit VMS condition values (`STS$` layout).
//!
//! ```text
//! 31   28 27  26     16 15  14      3 2    0
//! control cust facility facsp  code   severity
//! ```

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Warning = 0,
    Success = 1,
    Error = 2,
    Info = 3,
    Severe = 4,
}

impl Severity {
    /// The letter in `%FACIL-S-IDENT`.
    pub fn letter(self) -> char {
        match self {
            Severity::Warning => 'W',
            Severity::Success => 'S',
            Severity::Error => 'E',
            Severity::Info => 'I',
            Severity::Severe => 'F',
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Cond(pub u32);

impl Cond {
    pub const SS_NORMAL: Cond = Cond(1);
    pub const SS_CONTROLC: Cond = Cond(0x651);

    pub fn new(facility: u16, msg_no: u16, severity: Severity) -> Cond {
        Cond(((facility as u32 & 0xfff) << 16) | ((msg_no as u32 & 0x1fff) << 3) | severity as u32)
    }

    /// Reserved severities 5..7 read as `Severe`, as VMS does.
    pub fn severity(self) -> Severity {
        match self.0 & 7 {
            0 => Severity::Warning,
            1 => Severity::Success,
            2 => Severity::Error,
            3 => Severity::Info,
            _ => Severity::Severe,
        }
    }

    /// Low bit set: success or informational.
    pub fn is_success(self) -> bool {
        self.0 & 1 != 0
    }

    pub fn facility(self) -> u16 {
        ((self.0 >> 16) & 0xfff) as u16
    }

    /// 13-bit message number, including the facility-specific bit.
    pub fn msg_no(self) -> u16 {
        ((self.0 >> 3) & 0x1fff) as u16
    }

    pub fn is_customer(self) -> bool {
        self.0 & (1 << 27) != 0
    }

    /// `STS$V_INHIB_MSG`: already reported, don't print again.
    pub fn inhibit_msg(self) -> bool {
        self.0 & (1 << 28) != 0
    }

    /// Condition identity, ignoring severity and control bits (`STS$V_COND_ID`).
    pub fn cond_id(self) -> u32 {
        (self.0 >> 3) & 0x1ff_ffff
    }

    /// `$STATUS` matching: same condition regardless of severity/control.
    pub fn matches(self, other: Cond) -> bool {
        self.cond_id() == other.cond_id()
    }
}

impl fmt::Display for Cond {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "%X{:08X}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_values() {
        assert_eq!(Cond::SS_NORMAL.severity(), Severity::Success);
        assert_eq!(Cond::SS_CONTROLC.severity(), Severity::Success);
        assert_eq!(Cond::SS_CONTROLC.facility(), 0);

        let fnf = Cond(0x18292); // RMS$_FNF
        assert_eq!(fnf.facility(), 1);
        assert_eq!(fnf.severity(), Severity::Error);
        assert!(!fnf.is_success());
        assert_eq!(Cond::new(1, fnf.msg_no(), Severity::Error), fnf);
        assert_eq!(fnf.to_string(), "%X00018292");
    }

    #[test]
    fn bits() {
        let c = Cond(0x1800_0004 | (0x123 << 16) | (5 << 3));
        assert_eq!(c.severity(), Severity::Severe);
        assert!(c.is_customer() && c.inhibit_msg());
        // STS$V_FAC_NO is 12 bits wide and includes the customer bit.
        assert_eq!((c.facility(), c.msg_no()), (0x923, 5));
        assert!(c.matches(Cond(Cond::new(0x123, 5, Severity::Warning).0 | 1 << 27)));
        assert_eq!(Severity::Severe.letter(), 'F');
    }
}
