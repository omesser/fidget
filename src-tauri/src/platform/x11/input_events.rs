//! XI2 raw input events for event-driven pointer input on X11.
//! Permission-free raw events on the root window, so the frame loop can
//! block on `recv()` instead of waking at 16ms while idle.

use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::thread;
use x11rb::connection::Connection;
use x11rb::protocol::xinput::{self, ConnectionExt as _};
use x11rb::protocol::xproto;

/// An input event from XI2 that the frame loop cares about.
#[derive(Clone, Copy, Debug)]
pub enum InputEvent {
    /// Mouse button pressed or released.
    Button,
    /// Mouse moved. The frame loop doesn't need the position (it polls the
    /// cursor separately), only that motion happened.
    Motion,
}

/// Spawn a thread that listens for XI2 raw events; `None` if XI2 cannot set up.
/// It `wait_for_event()`s on the shared X11 connection the frame thread also
/// uses. x11rb `Sync` allows that; if event processing stalls, look here.
pub fn spawn_listener() -> Option<mpsc::Receiver<InputEvent>> {
    let display = super::connection::connection()?;
    let (sender, receiver) = wake_channel();

    let screen = display.setup().roots.first()?;
    let root = screen.root;

    if setup_xi2(display, root).is_err() {
        return None;
    }

    thread::Builder::new()
        .name("xi2-input".to_string())
        .spawn(move || listen_loop(sender))
        .ok()?;

    Some(receiver)
}

/// One slot, as on macOS: a full one is a wake already pending, and an
/// unbounded queue grows while the loop is active or hidden and not reading.
fn wake_channel() -> (SyncSender<InputEvent>, mpsc::Receiver<InputEvent>) {
    mpsc::sync_channel(1)
}

fn setup_xi2(
    display: &impl Connection,
    root: xproto::Window,
) -> Result<(), Box<dyn std::error::Error>> {
    let version = display.xinput_xi_query_version(2, 0)?.reply()?;

    if version.major_version < 2 {
        return Err("XI2 not available".into());
    }

    // One mask word holding all three bits; a word per event is BadValue.
    let mask = vec![
        xinput::XIEventMask::RAW_BUTTON_PRESS
            | xinput::XIEventMask::RAW_BUTTON_RELEASE
            | xinput::XIEventMask::RAW_MOTION,
    ];

    display.xinput_xi_select_events(
        root,
        &[xinput::EventMask {
            deviceid: xinput::Device::ALL.into(),
            mask,
        }],
    )?;

    display.flush()?;
    Ok(())
}

fn listen_loop(sender: SyncSender<InputEvent>) {
    let Some(display) = super::connection::connection() else {
        return;
    };

    let events = std::iter::from_fn(|| display.wait_for_event().ok());
    forward_wakes(
        events.filter_map(|event| classify_event(display, &event)),
        &sender,
    );
}

/// Offer each event to the frame loop; drop it when a wake is already pending.
fn forward_wakes(events: impl IntoIterator<Item = InputEvent>, sender: &SyncSender<InputEvent>) {
    for input_event in events {
        match sender.try_send(input_event) {
            Ok(()) | Err(TrySendError::Full(_)) => {}
            Err(TrySendError::Disconnected(_)) => break,
        }
    }
}

fn classify_event(
    _display: &impl Connection,
    event: &x11rb::protocol::Event,
) -> Option<InputEvent> {
    use x11rb::protocol::Event;

    match event {
        Event::XinputRawButtonPress(_) | Event::XinputRawButtonRelease(_) => {
            Some(InputEvent::Button)
        }
        Event::XinputRawMotion(_) => Some(InputEvent::Motion),
        // Unchecked requests (the overlay shapes) report their errors here.
        Event::Error(error) => {
            eprintln!("x11: request failed: {error:?}");
            None
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::TryRecvError;
    use std::time::Duration;
    use x11rb::protocol::xtest::ConnectionExt as _;
    use x11rb::wrapper::ConnectionExt as _;

    #[test]
    fn a_burst_of_motion_with_no_reader_leaves_one_wake_waiting() {
        let (sender, receiver) = wake_channel();
        forward_wakes(std::iter::repeat_n(InputEvent::Motion, 1000), &sender);
        assert!(matches!(receiver.try_recv(), Ok(InputEvent::Motion)));
        assert_eq!(receiver.try_recv().unwrap_err(), TryRecvError::Empty);
    }

    #[test]
    fn forwarding_stops_once_the_frame_loop_is_gone() {
        let (sender, receiver) = wake_channel();
        drop(receiver);
        let mut offered = 0;
        let events = std::iter::repeat_with(|| {
            offered += 1;
            InputEvent::Motion
        });
        forward_wakes(events.take(1000), &sender);
        assert_eq!(offered, 1);
    }

    /// Run with: Xvfb :77 & DISPLAY=:77 cargo test --bin fidget input_events -- --include-ignored
    #[test]
    #[ignore = "needs an X server with XI2 and XTEST, such as Xvfb"]
    fn pointer_motion_wakes_the_frame_loop() {
        let events = spawn_listener().expect("XI2 on $DISPLAY");
        let (conn, screen) = x11rb::connect(None).expect("an X server on $DISPLAY");
        let root = conn.setup().roots[screen].root;
        conn.xtest_fake_input(
            xproto::MOTION_NOTIFY_EVENT,
            0,
            x11rb::CURRENT_TIME,
            root,
            40,
            30,
            0,
        )
        .unwrap();
        conn.sync().unwrap();
        // The idle frame loop blocks on this same `recv_timeout`.
        let woken = events.recv_timeout(Duration::from_secs(2));
        assert!(matches!(woken, Ok(InputEvent::Motion)), "got {woken:?}");
    }
}
