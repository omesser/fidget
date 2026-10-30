//! XI2 raw input events on X11: the only source of button presses, and the
//! wake that lets the idle frame loop block on `recv()` instead of waking at
//! 16ms. Permission-free raw events on the root window.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::{Mutex, PoisonError};
use std::thread;
use x11rb::connection::Connection;
use x11rb::protocol::xinput::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{self, ConnectionExt as _};

use crate::platform::{trace_edge, ButtonsDown, Witness};

/// An input event from XI2 that the frame loop cares about.
#[derive(Clone, Copy, Debug)]
pub enum InputEvent {
    /// A button went down or up on one physical `device`, numbered after the
    /// core pointer mapping: 1 is primary, 3 secondary.
    Button {
        button: u32,
        down: bool,
        device: u16,
    },
    /// Mouse moved. The frame loop doesn't need the position (it polls the
    /// cursor separately), only that motion happened.
    Motion,
}

/// Each button as XI2 reported it. A latch per button rather than a queue, so
/// a click shorter than a tick still reads as one press.
struct Buttons {
    primary: Witness,
    secondary: Witness,
    /// Each (button, device) down now. With two mice, a button is down while
    /// either holds it.
    held: Mutex<BTreeSet<(u32, u16)>>,
    /// Where the latest press began. Raw events carry no position.
    pressed_at: Mutex<Option<(i32, i32)>>,
}

impl Buttons {
    const fn new() -> Self {
        Self {
            primary: Witness::new(),
            secondary: Witness::new(),
            held: Mutex::new(BTreeSet::new()),
            pressed_at: Mutex::new(None),
        }
    }

    fn report(&self, event: InputEvent) {
        let InputEvent::Button {
            button,
            down,
            device,
        } = event
        else {
            return;
        };
        let (name, witness) = match button {
            1 => ("left", &self.primary),
            3 => ("right", &self.secondary),
            _ => return,
        };
        trace_edge("xi2", name, down);
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        // False for the master's repeat of an edge, which must not press again.
        let changed = if down {
            held.insert((button, device))
        } else {
            held.remove(&(button, device))
        };
        if changed {
            witness.report(held.iter().any(|&(held_button, _)| held_button == button));
        }
    }

    fn take(&self) -> ButtonsDown {
        ButtonsDown {
            primary: self.primary.take(),
            secondary: self.secondary.take(),
        }
    }
}

static BUTTONS: Buttons = Buttons::new();

/// True while a listener thread feeds `BUTTONS`.
static LISTENING: AtomicBool = AtomicBool::new(false);

/// What XI2 reported since the last call, or `None` when no listener runs.
pub fn buttons_down() -> Option<ButtonsDown> {
    LISTENING.load(Ordering::SeqCst).then(|| BUTTONS.take())
}

/// Where the latest primary or secondary press began, in root pixels, or
/// `None` when no listener runs.
pub fn press_point() -> Option<(i32, i32)> {
    if !LISTENING.load(Ordering::SeqCst) {
        return None;
    }
    *BUTTONS
        .pressed_at
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
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

    let Some(root) = display.setup().roots.first().map(|screen| screen.root) else {
        return;
    };

    LISTENING.store(true, Ordering::SeqCst);
    let mut map = pointer_mapping(display);
    let events = std::iter::from_fn(|| display.wait_for_event().ok());
    let classified = events.filter_map(|event| {
        if let x11rb::protocol::Event::MappingNotify(notify) = &event {
            if notify.request == xproto::Mapping::POINTER {
                map = pointer_mapping(display);
            }
        }
        let input_event = classify_event(&event, &map)?;
        // Read before the edge is latched, so a tick that sees the press
        // finds where it began.
        if let InputEvent::Button {
            button: 1 | 3,
            down: true,
            ..
        } = input_event
        {
            *BUTTONS
                .pressed_at
                .lock()
                .unwrap_or_else(PoisonError::into_inner) = pointer_at(display, root);
        }
        Some(input_event)
    });
    forward_wakes(classified, &BUTTONS, &sender);
    LISTENING.store(false, Ordering::SeqCst);
}

/// Latch each button edge, then offer the frame loop a wake, dropped when one
/// is already pending. The edge never waits on the channel.
fn forward_wakes(
    events: impl IntoIterator<Item = InputEvent>,
    buttons: &Buttons,
    sender: &SyncSender<InputEvent>,
) {
    for input_event in events {
        buttons.report(input_event);
        match sender.try_send(input_event) {
            Ok(()) | Err(TrySendError::Full(_)) => {}
            Err(TrySendError::Disconnected(_)) => break,
        }
    }
}

/// The pointer on the root window, read once per press.
fn pointer_at(display: &impl Connection, root: xproto::Window) -> Option<(i32, i32)> {
    let reply = display.query_pointer(root).ok()?.reply().ok()?;
    Some((i32::from(reply.root_x), i32::from(reply.root_y)))
}

/// The core pointer mapping. Raw events carry the physical button, before the
/// swap `xmodmap -e "pointer = 3 2 1"` makes for a left-handed mouse; empty
/// when the server does not answer.
fn pointer_mapping(display: &impl Connection) -> Vec<u8> {
    display
        .get_pointer_mapping()
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .map(|reply| reply.map)
        .unwrap_or_default()
}

/// The button the core events would report for a physical one. Entry n - 1
/// of `map` is the button physical n sends; 0 disables it.
fn logical_button(map: &[u8], physical: u32) -> u32 {
    let entry = usize::try_from(physical)
        .ok()
        .and_then(|n| n.checked_sub(1))
        .and_then(|index| map.get(index));
    entry.map_or(physical, |&button| u32::from(button))
}

fn classify_event(event: &x11rb::protocol::Event, map: &[u8]) -> Option<InputEvent> {
    use x11rb::protocol::Event;

    match event {
        Event::XinputRawButtonPress(event) => Some(InputEvent::Button {
            button: logical_button(map, event.detail),
            down: true,
            device: event.sourceid,
        }),
        Event::XinputRawButtonRelease(event) => Some(InputEvent::Button {
            button: logical_button(map, event.detail),
            down: false,
            device: event.sourceid,
        }),
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
        forward_wakes(
            std::iter::repeat_n(InputEvent::Motion, 1000),
            &Buttons::new(),
            &sender,
        );
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
        forward_wakes(events.take(1000), &Buttons::new(), &sender);
        assert_eq!(offered, 1);
    }

    fn edge(button: u32, down: bool) -> InputEvent {
        from_device(2, button, down)
    }

    fn from_device(device: u16, button: u32, down: bool) -> InputEvent {
        InputEvent::Button {
            button,
            down,
            device,
        }
    }

    /// Two mice. XI2 also repeats each edge from the master device, with the
    /// same source.
    #[test]
    fn a_button_stays_down_while_any_mouse_holds_it() {
        let buttons = Buttons::new();
        let (sender, _receiver) = wake_channel();
        let both_down_one_up = [
            from_device(10, 1, true),
            from_device(10, 1, true),
            from_device(11, 1, true),
            from_device(10, 1, false),
            from_device(10, 1, false),
        ];
        forward_wakes(both_down_one_up, &buttons, &sender);
        assert!(buttons.take().primary, "pressed since the last tick");
        assert!(buttons.take().primary, "the second mouse still holds it");
        forward_wakes([from_device(11, 1, false)], &buttons, &sender);
        assert_eq!(buttons.take(), ButtonsDown::default(), "both let go");
    }

    /// XI2 repeats each edge from the master device. A repeated press that
    /// lands after the tick read the first must not read as a second press.
    #[test]
    fn a_repeated_press_reads_once() {
        let buttons = Buttons::new();
        let (sender, _receiver) = wake_channel();
        forward_wakes([edge(3, true)], &buttons, &sender);
        assert!(buttons.take().secondary, "pressed");
        forward_wakes(
            [edge(3, true), edge(3, false), edge(3, false)],
            &buttons,
            &sender,
        );
        assert_eq!(buttons.take(), ButtonsDown::default(), "released");
    }

    /// The wake channel holds one event and is full while the loop is busy.
    /// The edges must reach the next tick anyway, each read once.
    #[test]
    fn edges_reach_buttons_down_with_a_wake_already_pending() {
        let buttons = Buttons::new();
        let (sender, _receiver) = wake_channel();
        let events = [
            InputEvent::Motion,
            edge(1, true),
            edge(1, false),
            edge(3, true),
        ];
        forward_wakes(events, &buttons, &sender);
        assert_eq!(
            buttons.take(),
            ButtonsDown {
                primary: true,
                secondary: true,
            },
            "a left click shorter than a tick, and a right button held down"
        );
        assert_eq!(
            buttons.take(),
            ButtonsDown {
                primary: false,
                secondary: true,
            },
            "the left click is read once, and the right button is still held"
        );
        forward_wakes([edge(3, false)], &buttons, &sender);
        assert_eq!(buttons.take(), ButtonsDown::default(), "both released");
    }

    /// The wheel is buttons 4 to 7 and the middle button is 2. None is a click.
    #[test]
    fn only_buttons_1_and_3_are_presses() {
        let buttons = Buttons::new();
        let (sender, _receiver) = wake_channel();
        let events = [2, 4, 5, 6, 7].map(|button| edge(button, true));
        forward_wakes(events, &buttons, &sender);
        assert_eq!(buttons.take(), ButtonsDown::default());
    }

    #[test]
    fn a_left_handed_mapping_makes_physical_3_the_primary() {
        let swapped = [3, 2, 1, 4, 5];
        assert_eq!(logical_button(&swapped, 3), 1);
        assert_eq!(logical_button(&swapped, 1), 3);
        assert_eq!(logical_button(&swapped, 4), 4);
        assert_eq!(logical_button(&[0, 2, 3], 1), 0, "a disabled button");
        assert_eq!(logical_button(&[], 3), 3, "no mapping read");
        assert_eq!(logical_button(&swapped, 0), 0);
    }

    /// The one listener the X server tests share. A second would split the
    /// shared connection's events with it. The lock runs them one at a time.
    fn listener() -> std::sync::MutexGuard<'static, mpsc::Receiver<InputEvent>> {
        static LISTENER: std::sync::OnceLock<std::sync::Mutex<mpsc::Receiver<InputEvent>>> =
            std::sync::OnceLock::new();
        LISTENER
            .get_or_init(|| std::sync::Mutex::new(spawn_listener().expect("XI2 on $DISPLAY")))
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// One XTEST event. `detail` is the button, or 1 for a motion relative to
    /// where the pointer is, so a rerun on the same server still moves it.
    fn fake_input(kind: u8, detail: u8) {
        let (conn, screen) = x11rb::connect(None).expect("an X server on $DISPLAY");
        let root = conn.setup().roots[screen].root;
        conn.xtest_fake_input(kind, detail, x11rb::CURRENT_TIME, root, 1, 0, 0)
            .unwrap();
        conn.sync().unwrap();
    }

    /// Read `buttons_down` after each wake, as the idle frame loop does, until
    /// a read `wanted` accepts. No read may show the left button.
    fn read_until(
        events: &mpsc::Receiver<InputEvent>,
        wanted: impl Fn(ButtonsDown) -> bool,
    ) -> ButtonsDown {
        loop {
            if let Err(error) = events.recv_timeout(Duration::from_secs(2)) {
                panic!("no wake: {error}");
            }
            let read = buttons_down().expect("the listener is running");
            assert!(!read.primary, "only the right button was pressed");
            if wanted(read) {
                return read;
            }
        }
    }

    /// The pointer warped to a spot on the root window.
    fn fake_motion_to(x: i16, y: i16) {
        let (conn, screen) = x11rb::connect(None).expect("an X server on $DISPLAY");
        let root = conn.setup().roots[screen].root;
        conn.xtest_fake_input(
            xproto::MOTION_NOTIFY_EVENT,
            0,
            x11rb::CURRENT_TIME,
            root,
            x,
            y,
            0,
        )
        .unwrap();
        conn.sync().unwrap();
    }

    /// The overlay latch answers only if XI2 stops. A press it saw while XI2
    /// answered is old news by then and must not come through.
    #[test]
    #[ignore = "needs an X server with XI2 and XTEST, such as Xvfb"]
    fn the_fallback_does_not_inherit_a_press_from_while_xi2_answered() {
        let _events = listener();
        crate::platform::set_overlay_secondary(true);
        crate::platform::set_overlay_secondary(false);
        crate::platform::buttons_down();
        LISTENING.store(false, Ordering::SeqCst);
        let fallback = crate::platform::buttons_down();
        LISTENING.store(true, Ordering::SeqCst);
        assert_eq!(fallback, ButtonsDown::default());
    }

    /// Run with: Xvfb :77 & DISPLAY=:77 cargo test --bin fidget input_events -- --include-ignored
    #[test]
    #[ignore = "needs an X server with XI2 and XTEST, such as Xvfb"]
    fn pointer_motion_wakes_the_frame_loop() {
        let events = listener();
        while events.try_recv().is_ok() {}
        fake_input(xproto::MOTION_NOTIFY_EVENT, 1);
        // The idle frame loop blocks on this same `recv_timeout`.
        let woken = events.recv_timeout(Duration::from_secs(2));
        assert!(matches!(woken, Ok(InputEvent::Motion)), "got {woken:?}");
    }

    /// The edge is latched before its wake is offered, and a full channel is
    /// a wake already pending, so some wake always comes after the edge.
    #[test]
    #[ignore = "needs an X server with XI2 and XTEST, such as Xvfb"]
    fn a_right_click_reads_down_then_up_after_its_wakes() {
        let events = listener();
        fake_input(xproto::BUTTON_PRESS_EVENT, 3);
        read_until(&events, |read| read.secondary);
        fake_input(xproto::BUTTON_RELEASE_EVENT, 3);
        read_until(&events, |read| !read.secondary);
    }

    /// A press and a drag can land in one tick. The press is hit-tested where
    /// it began, not where the tick finds the cursor.
    #[test]
    #[ignore = "needs an X server with XI2 and XTEST, such as Xvfb"]
    fn a_press_keeps_where_it_began_after_the_pointer_moves() {
        let events = listener();
        fake_motion_to(100, 100);
        fake_input(xproto::BUTTON_PRESS_EVENT, 3);
        read_until(&events, |read| read.secondary);
        fake_motion_to(300, 300);
        assert_eq!(press_point(), Some((100, 100)));
        fake_input(xproto::BUTTON_RELEASE_EVENT, 3);
        read_until(&events, |read| !read.secondary);
    }
}
