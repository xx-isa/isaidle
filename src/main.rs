mod logind;
mod rules;
mod state;
mod wayland;

use futures_util::StreamExt;
use logind::SessionProxy;
use niri_ipc::{socket::Socket, Action, Request};
use rules::{Action as RuleAction, Scope, Rule};
use state::{DaemonState, Event};
use std::sync::mpsc as std_mpsc;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{info, warn};
use wayland::WlCommand;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let rules = build_rules();

    let (tx, mut rx) = mpsc::channel::<Event>(64);
    let (cmd_tx, cmd_rx) = std_mpsc::sync_channel::<WlCommand>(8);

    let (conn, queue, wl_state) = wayland::connect(tx.clone(), cmd_rx)?;
    wayland::start_dispatch_loop(conn, queue, wl_state);
    info!("connected to Wayland, idle notifications registered");

    let dbus_conn = zbus::Connection::system()
        .await
        .inspect_err(|e| warn!("dbus connect: {e}"))?;
    let session = logind::get_session_proxy(&dbus_conn)
        .await
        .inspect_err(|e| warn!("get session: {e}"))?;
    let mut lock_stream = session
        .receive_lock_signal()
        .await
        .inspect_err(|e| warn!("subscribe Lock: {e}"))?;
    let mut unlock_stream = session
        .receive_unlock_signal()
        .await
        .inspect_err(|e| warn!("subscribe Unlock: {e}"))?;
    info!("subscribed to logind Lock/Unlock signals");
    let _ = sd_notify::notify(true, &[sd_notify::NotifyState::Ready]);

    let tx_lock = tx.clone();
    tokio::spawn(async move {
        while lock_stream.next().await.is_some() {
            let _ = tx_lock.send(Event::LogindLock).await;
        }
    });

    let tx_unlock = tx.clone();
    tokio::spawn(async move {
        while unlock_stream.next().await.is_some() {
            let _ = tx_unlock.send(Event::LogindUnlock).await;
        }
    });

    let mut state = DaemonState::new();

    // Activate rules for the initial scope.
    send_scope_rules(Scope::Unlocked, &rules, &cmd_tx);

    while let Some(event) = rx.recv().await {
        handle_event(event, &mut state, &rules, &cmd_tx, &session);
    }

    Ok(())
}

fn build_rules() -> Vec<Rule> {
    vec![
        Rule {
            scope: Scope::Unlocked,
            timeout: Duration::from_secs(30),
            action: RuleAction::DpmsOff,
            on_exit: Some(RuleAction::DpmsOn),
        },
        Rule {
            scope: Scope::Unlocked,
            timeout: Duration::from_secs(60),
            action: RuleAction::LockSession,
            on_exit: None,
        },
        Rule {
            scope: Scope::Locked,
            timeout: Duration::from_secs(10),
            action: RuleAction::DpmsOff,
            on_exit: Some(RuleAction::DpmsOn),
        },
    ]
}

fn send_scope_rules(scope: Scope, rules: &[Rule], cmd_tx: &std_mpsc::SyncSender<WlCommand>) {
    let active: Vec<(usize, Duration)> = rules
        .iter()
        .enumerate()
        .filter(|(_, r)| r.scope == scope)
        .map(|(i, r)| (i, r.timeout))
        .collect();
    let _ = cmd_tx.send(WlCommand::EnterScope(active));
}

fn enter_scope(
    new_scope: Scope,
    rules: &[Rule],
    state: &mut DaemonState,
    cmd_tx: &std_mpsc::SyncSender<WlCommand>,
    session: &SessionProxy<'static>,
) {
    let exit_actions: Vec<RuleAction> = rules
        .iter()
        .filter(|r| r.scope == state.current_scope)
        .filter_map(|r| r.on_exit.clone())
        .collect();
    for action in &exit_actions {
        handle_action(action, state, session);
    }
    state.current_scope = new_scope;
    send_scope_rules(new_scope, rules, cmd_tx);
}

fn rule_applies(rule: &Rule, state: &DaemonState) -> bool {
    rule.scope == state.current_scope
}

fn handle_event(
    event: Event,
    state: &mut DaemonState,
    rules: &[Rule],
    cmd_tx: &std_mpsc::SyncSender<WlCommand>,
    session: &SessionProxy<'static>,
) {
    info!("{event}");
    match event {
        Event::IdleTimerFired(i) => {
            if rule_applies(&rules[i], state) {
                handle_action(&rules[i].action, state, session);
            }
        }
        Event::IdleResumed(i) => {
            if rule_applies(&rules[i], state) {
                if let Some(action) = &rules[i].on_exit {
                    handle_action(action, state, session);
                }
            }
        }
        Event::LogindLock => {
            enter_scope(Scope::Locked, rules, state, cmd_tx, session);
            spawn_swaylock(session.clone());
        }
        Event::LogindUnlock => {
            enter_scope(Scope::Unlocked, rules, state, cmd_tx, session);
        }
    }
}

fn handle_action(action: &RuleAction, state: &mut DaemonState, session: &SessionProxy<'static>) {
    match action {
        RuleAction::SpawnProcess(argv) => spawn_process(argv),
        RuleAction::LockSession => {
            let session = session.clone();
            tokio::spawn(async move {
                if let Err(e) = session.lock().await {
                    warn!("logind Lock call failed: {e}");
                }
            });
        }
        RuleAction::DpmsOff => {
            if state.display_on {
                dpms_off();
                state.display_on = false;
            }
        }
        RuleAction::DpmsOn => {
            if !state.display_on {
                dpms_on();
                state.display_on = true;
            }
        }
    }
}

fn spawn_swaylock(session: SessionProxy<'static>) {
    tokio::spawn(async move {
        let status = tokio::process::Command::new("swaylock").status().await;
        match status {
            Ok(s) if s.success() => {
                if let Err(e) = session.unlock().await {
                    warn!("logind Unlock call failed: {e}");
                }
            }
            Ok(s) => warn!("swaylock exited with status {s}"),
            Err(e) => warn!("swaylock failed to start: {e}"),
        }
    });
}

fn spawn_process(argv: &[String]) {
    let Some((bin, args)) = argv.split_first() else { return };
    let bin = bin.clone();
    let args: Vec<String> = args.to_vec();
    tokio::spawn(async move {
        let status = tokio::process::Command::new(&bin).args(&args).status().await;
        match status {
            Ok(s) if s.success() => info!("{bin} exited"),
            Ok(s) => warn!("{bin} exited with status {s}"),
            Err(e) => warn!("{bin} failed to start: {e}"),
        }
    });
}

fn niri_action(action: Action) {
    tokio::task::spawn_blocking(move || {
        match Socket::connect() {
            Ok(mut sock) => {
                if let Err(e) = sock.send(Request::Action(action)) {
                    warn!("niri IPC error: {e}");
                }
            }
            Err(e) => warn!("could not connect to niri socket: {e}"),
        }
    });
}

fn dpms_off() {
    niri_action(Action::PowerOffMonitors {});
}

fn dpms_on() {
    niri_action(Action::PowerOnMonitors {});
}
