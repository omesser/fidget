//! Whether the user is pressing the mouse.
//!
//! `CGEventSourceButtonState` asks the window server what the button is doing
//! right now. It is a state query and not an event tap, so it needs no
//! Accessibility permission — the same trade `sensing` makes to read how long
//! ago the last input was without reading the input itself.
//!
//! The Shell polls this beside the cursor position it already reads each tick.
//! A drag that outruns the art still has to be seen, and the overlay is
//! click-through wherever the sprite is not drawn, so the webview alone would
//! drop the button the moment the cursor left the pixels. The overlay still
//! reports the press that landed on it: this query has been seen to stay
//! false for a click our own window swallowed.

use std::sync::{Mutex, PoisonError};
use std::time::Instant;

use objc2_app_kit::NSEvent;
use objc2_core_graphics::{
    CGEventFlags, CGEventSource, CGEventSourceStateID, CGEventType, CGMouseButton,
};

use crate::platform::{trace_ms, ButtonsDown};

/// Whether the primary mouse button is down.
/// `CombinedSessionState` rather than HID, so a trackpad, mouse, and tablet
/// all count, including a hold while another app is frontmost.
pub fn primary_button_down() -> bool {
    CGEventSource::button_state(
        CGEventSourceStateID::CombinedSessionState,
        CGMouseButton::Left,
    )
}

/// Whether the secondary mouse button (right-click) is down.
pub fn secondary_button_down() -> bool {
    CGEventSource::button_state(
        CGEventSourceStateID::CombinedSessionState,
        CGMouseButton::Right,
    )
}

/// Whether Control is held. The same state query as the buttons: no event tap.
fn control_key_down() -> bool {
    CGEventSource::flags_state(CGEventSourceStateID::CombinedSessionState)
        .contains(CGEventFlags::MaskControl)
}

/// What the primary button's current press means. macOS reads a Control-click
/// as a secondary click and AppKit decides on the mouse-down, so the first
/// sighting of a press settles it until the button comes up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PrimaryPress {
    Up,
    Primary,
    Secondary,
}

impl PrimaryPress {
    fn seen(self, control: bool) -> Self {
        match self {
            Self::Up if control => Self::Secondary,
            Self::Up => Self::Primary,
            held => held,
        }
    }
}

static PRIMARY_PRESS: Mutex<PrimaryPress> = Mutex::new(PrimaryPress::Up);

/// One tick's buttons, with a Control-press moved from primary to secondary.
fn fold(press: PrimaryPress, raw: ButtonsDown, control: bool) -> (PrimaryPress, ButtonsDown) {
    let press = if raw.primary {
        press.seen(control)
    } else {
        PrimaryPress::Up
    };
    let buttons = ButtonsDown {
        primary: press == PrimaryPress::Primary,
        secondary: raw.secondary || press == PrimaryPress::Secondary,
    };
    (press, buttons)
}

/// The buttons as macOS means them: a Control-click is a secondary click.
pub fn control_click(raw: ButtonsDown) -> ButtonsDown {
    let mut press = PRIMARY_PRESS.lock().unwrap_or_else(PoisonError::into_inner);
    let (next, buttons) = fold(*press, raw, control_key_down());
    *press = next;
    buttons
}

/// The overlay felt a primary press. Settle it now, while Control is still
/// held: an idle frame loop can tick up to a second later.
pub fn primary_press_began() {
    let mut press = PRIMARY_PRESS.lock().unwrap_or_else(PoisonError::into_inner);
    *press = press.seen(control_key_down());
}

/// One button at one tick, as each source saw it. `downs` and `ups` are the
/// session's running event counters, which a click between two ticks still bumps.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ButtonSample {
    session: bool,
    /// The overlay witness's consuming read: down now, or pressed since the last tick.
    witness: bool,
    downs: u32,
    ups: u32,
}

impl ButtonSample {
    fn read(session: bool, witness: bool, down: CGEventType, up: CGEventType) -> Self {
        let state = CGEventSourceStateID::CombinedSessionState;
        Self {
            session,
            witness,
            downs: CGEventSource::counter_for_event_type(state, down),
            ups: CGEventSource::counter_for_event_type(state, up),
        }
    }
}

/// The `click-trace: tick` fields for a button that changed since `prev`, or
/// `None` when no source moved.
fn button_change(prev: ButtonSample, now: ButtonSample) -> Option<String> {
    let edge = match (prev.session, now.session) {
        (false, true) => "down",
        (true, false) => "up",
        _ => "-",
    };
    let down_delta = now.downs.wrapping_sub(prev.downs);
    let up_delta = now.ups.wrapping_sub(prev.ups);
    if edge == "-" && down_delta == 0 && up_delta == 0 && prev.witness == now.witness {
        return None;
    }
    Some(format!(
        "session={} session_edge={edge} down_delta={down_delta} up_delta={up_delta} witness={}",
        u8::from(now.session),
        u8::from(now.witness),
    ))
}

struct TraceState {
    at: Instant,
    left: ButtonSample,
    right: ButtonSample,
}

static TRACE: Mutex<Option<TraceState>> = Mutex::new(None);

/// A tick this far behind the last one is logged even when nothing moved, so
/// idle and stalled ticks show up between presses.
const SLOW_TICK_MS: f64 = 50.0;

/// Logs what each click source saw this tick, for #1858. The frame loop reads
/// the buttons once per tick, so the gap since the last call is the tick gap.
/// `session` is the poll this tick already made, not a second read.
pub fn trace_tick(session: ButtonsDown, witness: ButtonsDown, out: ButtonsDown) {
    let left = ButtonSample::read(
        session.primary,
        witness.primary,
        CGEventType::LeftMouseDown,
        CGEventType::LeftMouseUp,
    );
    let right = ButtonSample::read(
        session.secondary,
        witness.secondary,
        CGEventType::RightMouseDown,
        CGEventType::RightMouseUp,
    );
    let at = Instant::now();
    let t = trace_ms();
    let mut trace = TRACE.lock().unwrap_or_else(PoisonError::into_inner);
    let Some(prev) = trace.replace(TraceState { at, left, right }) else {
        eprintln!(
            "click-trace: start t={t:.1} left_downs={} left_ups={} right_downs={} right_ups={}",
            left.downs, left.ups, right.downs, right.ups
        );
        return;
    };
    let gap_ms = at.duration_since(prev.at).as_secs_f64() * 1000.0;
    let mut quiet = true;
    for (name, before, now) in [("left", prev.left, left), ("right", prev.right, right)] {
        if let Some(fields) = button_change(before, now) {
            quiet = false;
            eprintln!(
                "click-trace: tick t={t:.1} gap_ms={gap_ms:.1} button={name} {fields} \
                 control={} out_primary={} out_secondary={}",
                u8::from(control_key_down()),
                u8::from(out.primary),
                u8::from(out.secondary),
            );
        }
    }
    if quiet && gap_ms >= SLOW_TICK_MS {
        eprintln!("click-trace: tick t={t:.1} gap_ms={gap_ms:.1}");
    }
}

/// The OS double-click interval, in milliseconds.
pub fn double_click_interval_ms() -> Option<u32> {
    let seconds = NSEvent::doubleClickInterval();
    if seconds > 0.0 {
        Some((seconds * 1000.0).round() as u32)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2_app_kit::NSEvent;

    use fidget_core::engine::{Point, Verb};
    use fidget_core::input::Pointer;

    /// Runs ticks of `(primary, secondary, control)` through the fold and the
    /// gesture classifier over the sprite, moving 50 points a tick so a
    /// primary press drags, and collects every verb.
    fn verbs(ticks: &[(bool, bool, bool)]) -> Vec<Verb> {
        let mut pointer = Pointer::default();
        let mut press = PrimaryPress::Up;
        let mut verbs = Vec::new();
        for (tick, &(primary, secondary, control)) in ticks.iter().enumerate() {
            let buttons;
            (press, buttons) = fold(press, ButtonsDown { primary, secondary }, control);
            let at = Point {
                x: 100.0 + 50.0 * tick as f64,
                y: 100.0,
            };
            verbs.extend(pointer.update(true, buttons.primary, buttons.secondary, at, 16));
        }
        verbs
    }

    /// The Menu opens on the press, and dragging with it held drags nothing.
    #[test]
    fn a_control_click_on_the_sprite_is_a_menu_and_not_a_grab_or_poke() {
        assert_eq!(
            verbs(&[
                (true, false, true),
                (true, false, true),
                (false, false, true)
            ]),
            vec![Verb::Menu]
        );
    }

    /// AppKit settles a Control-click on the mouse-down, so letting go of
    /// Control mid-press does not turn it back into a Grab.
    #[test]
    fn releasing_control_mid_press_keeps_it_a_menu() {
        assert_eq!(
            verbs(&[
                (true, false, true),
                (true, false, false),
                (false, false, false)
            ]),
            vec![Verb::Menu]
        );
    }

    /// Control pressed after the button went down arrives too late to change it.
    #[test]
    fn control_pressed_mid_press_keeps_the_grab() {
        assert_eq!(
            verbs(&[(true, false, false), (true, false, true)]),
            vec![Verb::Grab]
        );
    }

    #[test]
    fn a_control_up_with_no_press_does_nothing() {
        assert_eq!(
            verbs(&[(false, false, true), (false, false, false)]),
            vec![]
        );
    }

    #[test]
    fn a_plain_click_after_a_control_click_is_primary_again() {
        let (press, _) = fold(PrimaryPress::Secondary, ButtonsDown::default(), false);
        let (_, buttons) = fold(
            press,
            ButtonsDown {
                primary: true,
                secondary: false,
            },
            false,
        );
        assert_eq!((buttons.primary, buttons.secondary), (true, false));
    }

    fn sample(session: bool, witness: bool, downs: u32, ups: u32) -> ButtonSample {
        ButtonSample {
            session,
            witness,
            downs,
            ups,
        }
    }

    #[test]
    fn a_still_button_logs_nothing() {
        let held = sample(true, true, 7, 6);
        assert_eq!(button_change(held, held), None);
    }

    #[test]
    fn a_press_seen_by_the_poll_logs_its_edge_and_counter() {
        assert_eq!(
            button_change(sample(false, false, 7, 7), sample(true, true, 8, 7)).as_deref(),
            Some("session=1 session_edge=down down_delta=1 up_delta=0 witness=1")
        );
    }

    /// A tap between two ticks never shows in the level, only in the counters.
    #[test]
    fn a_tap_between_ticks_logs_the_counter_deltas() {
        assert_eq!(
            button_change(sample(false, false, 7, 7), sample(false, false, 9, 9)).as_deref(),
            Some("session=0 session_edge=- down_delta=2 up_delta=2 witness=0")
        );
    }

    #[test]
    fn a_counter_that_wraps_still_logs_one_press() {
        assert_eq!(
            button_change(
                sample(false, false, u32::MAX, 0),
                sample(false, false, 0, 0)
            )
            .as_deref(),
            Some("session=0 session_edge=- down_delta=1 up_delta=0 witness=0")
        );
    }

    #[test]
    fn double_click_interval_ms_matches_nsevent() {
        let seconds = NSEvent::doubleClickInterval();
        let expected = if seconds > 0.0 {
            Some((seconds * 1000.0).round() as u32)
        } else {
            None
        };
        assert_eq!(
            double_click_interval_ms(),
            expected,
            "macos::double_click_interval_ms must match NSEvent::doubleClickInterval()"
        );
    }
}
