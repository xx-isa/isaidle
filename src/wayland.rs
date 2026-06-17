use std::sync::mpsc;
use std::time::Duration;

use tokio::sync::mpsc as tokio_mpsc;
use tracing::warn;
use wayland_client::{
    delegate_noop,
    protocol::{wl_registry, wl_seat},
    Connection, Dispatch, EventQueue, QueueHandle,
};
use wayland_protocols::ext::idle_notify::v1::client::{
    ext_idle_notification_v1, ext_idle_notifier_v1,
};

use crate::state::Event;

pub enum WlCommand {
    EnterScope(Vec<(usize, Duration)>),
}

pub struct WlState {
    pub seat: Option<wl_seat::WlSeat>,
    pub notifier: Option<ext_idle_notifier_v1::ExtIdleNotifierV1>,
    pub active_notifications: Vec<ext_idle_notification_v1::ExtIdleNotificationV1>,
    pub cmd_rx: mpsc::Receiver<WlCommand>,
    pub tx: tokio_mpsc::Sender<Event>,
}

impl Dispatch<wl_registry::WlRegistry, ()> for WlState {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global { name, interface, version } = event {
            match interface.as_str() {
                "wl_seat" => {
                    state.seat = Some(registry.bind(name, version.min(7), qh, ()));
                }
                "ext_idle_notifier_v1" => {
                    state.notifier = Some(registry.bind(name, version.min(1), qh, ()));
                }
                _ => {}
            }
        }
    }
}

delegate_noop!(WlState: ignore wl_seat::WlSeat);
delegate_noop!(WlState: ignore ext_idle_notifier_v1::ExtIdleNotifierV1);

impl Dispatch<ext_idle_notification_v1::ExtIdleNotificationV1, usize> for WlState {
    fn event(
        state: &mut Self,
        _: &ext_idle_notification_v1::ExtIdleNotificationV1,
        event: ext_idle_notification_v1::Event,
        rule_idx: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let ev = match event {
            ext_idle_notification_v1::Event::Idled => Event::IdleTimerFired(*rule_idx),
            ext_idle_notification_v1::Event::Resumed => Event::IdleResumed(*rule_idx),
            _ => return,
        };
        let _ = state.tx.try_send(ev);
    }
}

pub fn connect(
    tx: tokio_mpsc::Sender<Event>,
    cmd_rx: mpsc::Receiver<WlCommand>,
) -> anyhow::Result<(Connection, EventQueue<WlState>, WlState)> {
    let conn = Connection::connect_to_env()?;
    let mut queue: EventQueue<WlState> = conn.new_event_queue();
    let qh = queue.handle();

    let mut state = WlState {
        seat: None,
        notifier: None,
        active_notifications: Vec::new(),
        cmd_rx,
        tx,
    };
    conn.display().get_registry(&qh, ());
    queue.roundtrip(&mut state)?;

    Ok((conn, queue, state))
}

fn handle_wl_command(cmd: WlCommand, state: &mut WlState, qh: &QueueHandle<WlState>) {
    match cmd {
        WlCommand::EnterScope(rules) => {
            for notification in state.active_notifications.drain(..) {
                notification.destroy();
            }

            let seat = match &state.seat {
                Some(s) => s,
                None => { warn!("no wl_seat, cannot create idle notifications"); return; }
            };
            let notifier = match &state.notifier {
                Some(n) => n,
                None => { warn!("no ext_idle_notifier_v1, cannot create idle notifications"); return; }
            };

            for (rule_idx, timeout) in rules {
                let ms = timeout.as_millis() as u32;
                let notification = notifier.get_idle_notification(ms, seat, qh, rule_idx);
                state.active_notifications.push(notification);
            }
        }
    }
}

pub fn start_dispatch_loop(conn: Connection, mut queue: EventQueue<WlState>, mut state: WlState) {
    tokio::task::spawn_blocking(move || {
        let qh = queue.handle();
        loop {
            while let Ok(cmd) = state.cmd_rx.try_recv() {
                handle_wl_command(cmd, &mut state, &qh);
            }

            if let Err(e) = queue.dispatch_pending(&mut state) {
                warn!("wayland dispatch error: {e}");
                break;
            }
            if let Err(e) = queue.flush() {
                warn!("wayland flush error: {e}");
                break;
            }

            // Wait for Wayland events with a 100ms timeout so we can poll commands promptly.
            if let Some(guard) = conn.prepare_read() {
                use std::os::fd::AsFd as _;
                use std::os::unix::io::AsRawFd as _;
                let raw_fd = conn.as_fd().as_raw_fd();
                let mut pfd = libc::pollfd {
                    fd: raw_fd,
                    events: libc::POLLIN,
                    revents: 0,
                };
                // SAFETY: valid pollfd, 1 entry, 100ms timeout
                let ready = unsafe { libc::poll(&mut pfd, 1, 100) };
                if ready > 0 && (pfd.revents & libc::POLLIN) != 0 {
                    if let Err(e) = guard.read() {
                        warn!("wayland read error: {e}");
                        break;
                    }
                }
                // timeout or not-ready: drop guard and loop to check commands
            }
            // If prepare_read returned None, events are already queued; dispatch_pending handles them.
        }
    });
}
