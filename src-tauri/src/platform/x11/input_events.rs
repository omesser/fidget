//! XI2 raw input events for event-driven pointer input on X11.
//! Permission-free raw events on the root window, so the frame loop can
//! block on `recv()` instead of waking at 16ms while idle.

use std::sync::mpsc;
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
    let (sender, receiver) = mpsc::channel();

    let screen = display.setup().roots.first()?;
    let root = screen.root;

    if setup_xi2(display, root).is_err() {
        return None;
    }

    let sender_clone = sender.clone();
    thread::Builder::new()
        .name("xi2-input".to_string())
        .spawn(move || listen_loop(sender_clone))
        .ok()?;

    Some(receiver)
}

fn setup_xi2(
    display: &impl Connection,
    root: xproto::Window,
) -> Result<(), Box<dyn std::error::Error>> {
    let version = display.xinput_xi_query_version(2, 0)?.reply()?;

    if version.major_version < 2 {
        return Err("XI2 not available".into());
    }

    // One mask word with every bit set; a word per event is BadValue.
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

fn listen_loop(sender: mpsc::Sender<InputEvent>) {
    let Some(display) = super::connection::connection() else {
        return;
    };

    while let Ok(event) = display.wait_for_event() {
        let Some(input_event) = classify_event(display, &event) else {
            continue;
        };

        if sender.send(input_event).is_err() {
            break;
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
    use std::time::Duration;
    use x11rb::protocol::xtest::ConnectionExt as _;

    /// Run with: Xvfb :77 & DISPLAY=:77 cargo test --bin fidget input_events -- --include-ignored
    #[test]
    #[ignore = "needs an X server with XI2 and XTEST, such as Xvfb"]
    fn pointer_motion_reaches_the_channel_the_idle_frame_loop_waits_on() {
        let events = spawn_listener().expect("XI2 on $DISPLAY");
        let (conn, screen) = x11rb::connect(None).expect("an X server on $DISPLAY");
        let root = conn.setup().roots[screen].root;
        conn.xtest_fake_input(
            xproto::MOTION_NOTIFY_EVENT,
            0,
            x11rb::CURRENT_TIME,
            root,
            41,
            43,
            0,
        )
        .unwrap();
        conn.flush().unwrap();
        // The idle frame loop blocks on this same `recv_timeout`.
        let woken = events.recv_timeout(Duration::from_secs(3));
        assert!(matches!(woken, Ok(InputEvent::Motion)), "got {woken:?}");
    }
}
