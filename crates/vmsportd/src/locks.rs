//! The lock manager: `$ENQ`, `$DEQ` and conversions on named resources,
//! with VMS's six modes. A process's locks go when its connection closes.

use std::collections::HashMap;
use std::sync::{Condvar, Mutex};
use vms_cond::Cond;

/// SS$_NORMAL: granted after waiting.
pub const NORMAL: Cond = Cond(1);
/// SS$_SYNCH: granted at once.
pub const SYNCH: Cond = Cond(0x689);
/// SS$_NOTQUEUED: not grantable now, and the caller won't wait.
pub const NOTQUEUED: Cond = Cond(0x9B8);
pub const IVLOCKID: Cond = Cond(0x2124);
pub const CVTUNGRANT: Cond = Cond(0x213C);

/// Lock modes, weakest first: null, concurrent read, concurrent write,
/// protected read, protected write, exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Mode {
    NL,
    CR,
    CW,
    PR,
    PW,
    EX,
}

impl Mode {
    pub fn parse(s: &str) -> Option<Mode> {
        Some(match s {
            "NL" => Mode::NL,
            "CR" => Mode::CR,
            "CW" => Mode::CW,
            "PR" => Mode::PR,
            "PW" => Mode::PW,
            "EX" => Mode::EX,
            _ => return None,
        })
    }

    /// Whether a lock held in `self` lets another be granted in `other`.
    pub fn compatible(self, other: Mode) -> bool {
        use Mode::*;
        match (self.min(other), self.max(other)) {
            (NL, _) => true,
            (CR, EX) => false,
            (CR, _) => true,
            (CW, CW) => true,
            (CW, _) => false,
            (PR, PR) => true,
            _ => false,
        }
    }
}

struct Lock {
    owner: u64,
    resource: String,
    /// The granted mode (NL for a new lock still waiting).
    mode: Mode,
    granted: bool,
    /// The mode being waited for: a new lock or a conversion.
    want: Option<Mode>,
}

#[derive(Default)]
struct State {
    locks: HashMap<u32, Lock>,
    /// Each resource's locks, in the order they were asked for.
    resources: HashMap<String, Vec<u32>>,
    next: u32,
}

impl State {
    /// Whether lock `id` can have the mode it wants now: compatible with
    /// every granted lock, and (a new lock) not behind another waiting.
    fn grantable(&self, id: u32) -> bool {
        let l = &self.locks[&id];
        let Some(want) = l.want else { return false };
        let ids = &self.resources[&l.resource];
        for o in ids.iter().filter(|o| **o != id) {
            let o = &self.locks[o];
            if o.granted && !o.mode.compatible(want) {
                return false;
            }
        }
        if !l.granted {
            // New locks queue behind earlier waiters; conversions don't.
            for o in ids.iter().take_while(|o| **o != id) {
                if self.locks[o].want.is_some() {
                    return false;
                }
            }
        }
        true
    }

    fn grant(&mut self, id: u32) {
        let l = self.locks.get_mut(&id).unwrap();
        l.mode = l.want.take().unwrap();
        l.granted = true;
    }

    fn remove(&mut self, id: u32) {
        if let Some(l) = self.locks.remove(&id) {
            let ids = self.resources.get_mut(&l.resource).unwrap();
            ids.retain(|o| *o != id);
            if ids.is_empty() {
                self.resources.remove(&l.resource);
            }
        }
    }
}

#[derive(Default)]
pub struct Manager {
    state: Mutex<State>,
    changed: Condvar,
}

impl Manager {
    /// `$ENQW`: a new lock on `resource` in `mode`, waiting unless
    /// `noqueue`. Returns its ID and SYNCH (at once) or NORMAL (waited).
    pub fn enq(
        &self,
        owner: u64,
        resource: &str,
        mode: Mode,
        noqueue: bool,
    ) -> Result<(u32, Cond), Cond> {
        let mut s = self.state.lock().unwrap();
        s.next += 1;
        let id = s.next;
        s.locks.insert(
            id,
            Lock {
                owner,
                resource: resource.to_string(),
                mode: Mode::NL,
                granted: false,
                want: Some(mode),
            },
        );
        s.resources
            .entry(resource.to_string())
            .or_default()
            .push(id);
        self.wait(s, id, noqueue).map(|st| (id, st))
    }

    /// `$ENQW` with LCK$M_CONVERT: lock `id` to `mode`.
    pub fn convert(&self, owner: u64, id: u32, mode: Mode, noqueue: bool) -> Result<Cond, Cond> {
        let mut s = self.state.lock().unwrap();
        match s.locks.get_mut(&id) {
            Some(l) if l.owner == owner && l.granted => l.want = Some(mode),
            Some(l) if l.owner == owner => return Err(CVTUNGRANT),
            _ => return Err(IVLOCKID),
        }
        self.wait(s, id, noqueue)
    }

    fn wait(
        &self,
        mut s: std::sync::MutexGuard<'_, State>,
        id: u32,
        noqueue: bool,
    ) -> Result<Cond, Cond> {
        if s.grantable(id) {
            s.grant(id);
            // A conversion down lets others in.
            self.changed.notify_all();
            return Ok(SYNCH);
        }
        if noqueue {
            let l = s.locks.get_mut(&id).unwrap();
            if l.granted {
                l.want = None;
            } else {
                s.remove(id);
            }
            self.changed.notify_all();
            return Err(NOTQUEUED);
        }
        loop {
            s = self.changed.wait(s).unwrap();
            if !s.locks.contains_key(&id) {
                return Err(IVLOCKID);
            }
            if s.grantable(id) {
                s.grant(id);
                self.changed.notify_all();
                return Ok(NORMAL);
            }
        }
    }

    /// `$DEQ`.
    pub fn deq(&self, owner: u64, id: u32) -> Result<Cond, Cond> {
        let mut s = self.state.lock().unwrap();
        if s.locks.get(&id).is_none_or(|l| l.owner != owner) {
            return Err(IVLOCKID);
        }
        s.remove(id);
        self.changed.notify_all();
        Ok(NORMAL)
    }

    /// Everything a process held or waited for, when it goes away.
    pub fn release(&self, owner: u64) {
        let mut s = self.state.lock().unwrap();
        let ids: Vec<u32> = s
            .locks
            .iter()
            .filter(|(_, l)| l.owner == owner)
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            s.remove(id);
        }
        self.changed.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn compatibility() {
        use Mode::*;
        let row = |m: Mode| [NL, CR, CW, PR, PW, EX].map(|o| m.compatible(o));
        assert_eq!(row(NL), [true; 6]);
        assert_eq!(row(CR), [true, true, true, true, true, false]);
        assert_eq!(row(CW), [true, true, true, false, false, false]);
        assert_eq!(row(PR), [true, true, false, true, false, false]);
        assert_eq!(row(PW), [true, true, false, false, false, false]);
        assert_eq!(row(EX), [true, false, false, false, false, false]);
    }

    #[test]
    fn waits_and_releases() {
        let m = Arc::new(Manager::default());
        let (a, st) = m.enq(1, "R", Mode::PR, false).unwrap();
        assert_eq!(st, SYNCH);
        assert_eq!(m.enq(2, "R", Mode::PR, false).unwrap().1, SYNCH);
        assert_eq!(m.enq(3, "R", Mode::EX, true), Err(NOTQUEUED));
        // A waiter for EX gets it once both readers are gone.
        let m2 = m.clone();
        let t = std::thread::spawn(move || m2.enq(3, "R", Mode::EX, false));
        std::thread::sleep(std::time::Duration::from_millis(50));
        m.deq(1, a).unwrap();
        m.release(2);
        assert_eq!(t.join().unwrap().unwrap().1, NORMAL);
        // Conversions: up while compatible, NOQUEUE when not.
        let (b, _) = m.enq(4, "S", Mode::CR, false).unwrap();
        assert_eq!(m.convert(4, b, Mode::EX, false), Ok(SYNCH));
        let (c, _) = m.enq(5, "S", Mode::NL, false).unwrap();
        assert_eq!(m.convert(5, c, Mode::PR, true), Err(NOTQUEUED));
        assert_eq!(m.deq(5, b), Err(IVLOCKID));
    }
}
