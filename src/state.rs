use std::fmt;

use crate::rules::Phase;

#[derive(Debug)]
pub enum Event {
    IdleTimerFired(usize),
    IdleResumed(usize),
    LogindLock,
    LogindUnlock,
}

impl fmt::Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Event::IdleTimerFired(i) => write!(f, "idle timer fired (rule {i})"),
            Event::IdleResumed(i) => write!(f, "user activity resumed (rule {i})"),
            Event::LogindLock => f.write_str("logind: session locked"),
            Event::LogindUnlock => f.write_str("logind: session unlocked"),
        }
    }
}

pub struct DaemonState {
    pub display_on: bool,
    pub current_phase: Phase,
}

impl DaemonState {
    pub fn new() -> Self {
        Self {
            display_on: true,
            current_phase: Phase::Unlocked,
        }
    }
}
