use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Phase {
    Locked,
    Unlocked,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum Action {
    SpawnProcess(Vec<String>),
    LockSession,
    DpmsOff,
    DpmsOn,
}

#[derive(Debug, Clone)]
pub struct Rule {
    pub phase: Phase,
    pub timeout: Duration,
    pub action: Action,
    pub on_exit: Option<Action>,
}
