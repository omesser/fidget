//! Whether the user is pressing the mouse.
//!
//! `CGEventSource` asks the window server about the buttons. It is a state
//! query and not an event tap, so it needs no Accessibility permission — the
//! same trade `sensing` makes to read how long ago the last input was without
//! reading the input itself.
//!
//! The Shell reads it beside the cursor position it already reads each tick.
//! A drag that outruns the art still has to be seen, and the overlay is
//! click-through wherever the sprite is not drawn, so the webview alone would
//! drop the button the moment the cursor left the pixels. Each button's down
//! and up event counters say when a press began and ended, so a click between
//! two ticks still counts; the button level only says whether a counted press
//! is still held. In the #1858 trace the counters saw every press the overlay
//! saw, and presses it missed, so the overlay is not a second source here.

use std::ptr::NonNull;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::Instant;

use block2::RcBlock;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSEvent, NSEventMask};
use objc2_core_graphics::{
    CGEventFlags, CGEventSource, CGEventSourceStateID, CGEventType, CGMouseButton,
};
use objc2_foundation::{NSDate, NSDefaultRunLoopMode};

use crate::platform::{trace_ms, ButtonsDown};

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
fn control_click(raw: ButtonsDown) -> ButtonsDown {
    let mut press = PRIMARY_PRESS.lock().unwrap_or_else(PoisonError::into_inner);
    let (next, buttons) = fold(*press, raw, control_key_down());
    *press = next;
    buttons
}

/// The overlay felt a primary press. Settle what it means now, while Control
/// is still held: an idle frame loop can tick up to a second later. Whether
/// there was a press at all is still the counters' call, so a pointerdown
/// with no counted press pending is ignored.
pub fn primary_press_began() {
    let downs = CGEventSource::counter_for_event_type(
        CGEventSourceStateID::CombinedSessionState,
        CGEventType::LeftMouseDown,
    );
    let pending = SESSION
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .is_some_and(|session| session.left.pending(downs));
    let mut press = PRIMARY_PRESS.lock().unwrap_or_else(PoisonError::into_inner);
    *press = press_began(*press, pending, control_key_down());
}

/// A pointerdown can trail its press by about 220 ms (#1795). One that lands
/// after the tick that saw the press end would settle the next click instead.
fn press_began(press: PrimaryPress, pending: bool, control: bool) -> PrimaryPress {
    if pending {
        press.seen(control)
    } else {
        press
    }
}

/// One button at one tick, as the session reports it. `downs` and `ups` are
/// the session's running event counters, which a click between two ticks
/// still bumps.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ButtonSample {
    level: bool,
    downs: u32,
    ups: u32,
}

impl ButtonSample {
    fn read(button: CGMouseButton, down: CGEventType, up: CGEventType) -> Self {
        let state = CGEventSourceStateID::CombinedSessionState;
        Self {
            level: CGEventSource::button_state(state, button),
            downs: CGEventSource::counter_for_event_type(state, down),
            ups: CGEventSource::counter_for_event_type(state, up),
        }
    }
}

/// One button as the frame loop is told it, one level per tick. The counters
/// say a press began or ended; the level only says a counted press is still
/// held. `Tapped` and `Owed` report a level the button no longer has, so a
/// press or a release that fit between two ticks still gets a tick of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Press {
    Up,
    /// Pressed, and the level says still held.
    Held,
    /// Pressed and let go since the last tick: down now, up next tick.
    Tapped,
    /// Let go and pressed again since the last tick: up now, down next tick.
    Owed,
}

impl Press {
    fn step(self, downs: u32, ups: u32, level: bool) -> Self {
        let pressed = if level { Self::Held } else { Self::Tapped };
        let let_go = if downs > 0 { Self::Owed } else { Self::Up };
        match self {
            Self::Up if downs > 0 => pressed,
            Self::Up => Self::Up,
            Self::Held if ups > 0 || !level => let_go,
            Self::Held => Self::Held,
            Self::Tapped => let_go,
            Self::Owed => pressed,
        }
    }

    fn down(self) -> bool {
        matches!(self, Self::Held | Self::Tapped)
    }
}

/// One button's last sample and where its press stands.
#[derive(Clone, Copy, Debug)]
struct Button {
    last: ButtonSample,
    press: Press,
}

impl Button {
    /// The first sample is the baseline: a button already held when Fidget
    /// starts has no counted press, so it is not one.
    fn new(first: ButtonSample) -> Self {
        Self {
            last: first,
            press: Press::Up,
        }
    }

    /// Whether a counted press is still under way: the down counter moved
    /// past the last sample, or the last tick left the button held or owed
    /// a press to the next tick.
    fn pending(&self, downs: u32) -> bool {
        downs != self.last.downs || matches!(self.press, Press::Held | Press::Owed)
    }

    fn step(&mut self, now: ButtonSample) -> bool {
        let downs = now.downs.wrapping_sub(self.last.downs);
        let ups = now.ups.wrapping_sub(self.last.ups);
        self.press = self.press.step(downs, ups, now.level);
        self.last = now;
        self.press.down()
    }
}

struct Session {
    at: Instant,
    left: Button,
    right: Button,
}

static SESSION: Mutex<Option<Session>> = Mutex::new(None);

/// Every left and right press the session has counted, in any app.
fn session_downs() -> u32 {
    let state = CGEventSourceStateID::CombinedSessionState;
    CGEventSource::counter_for_event_type(state, CGEventType::LeftMouseDown).wrapping_add(
        CGEventSource::counter_for_event_type(state, CGEventType::RightMouseDown),
    )
}

/// `session_downs` as of the last mouse-down AppKit handed this app.
static DELIVERED_DOWNS: AtomicU32 = AtomicU32::new(0);

const PRESS_MASK: NSEventMask = NSEventMask::LeftMouseDown.union(NSEventMask::RightMouseDown);

/// Start noting which presses AppKit has delivered. Main thread, once.
pub fn watch_delivered_presses() {
    if MainThreadMarker::new().is_none() {
        return;
    }
    DELIVERED_DOWNS.store(session_downs(), Ordering::Relaxed);
    let block = RcBlock::new(|event: NonNull<NSEvent>| {
        DELIVERED_DOWNS.store(session_downs(), Ordering::Relaxed);
        event.as_ptr()
    });
    // SAFETY: main thread; the block hands back the event it was given.
    let monitor =
        unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(PRESS_MASK, &block) };
    // The monitor lives as long as the app.
    std::mem::forget(monitor);
}

/// Measured mouse-downs arrived up to 40 ms after the counters saw the press;
/// a pointerdown has trailed by 220 ms. Only a press that never comes
/// pays all of it.
const PRESS_DELIVERY_WAIT_SECS: f64 = 0.25;

/// The counters see a press before AppKit delivers its mouse-down, and a menu
/// that starts tracking first takes that mouse-down as a click that closes it.
/// So dispatch it first. A press another app took never comes, hence the wait.
pub fn deliver_pending_press() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    if DELIVERED_DOWNS.load(Ordering::Relaxed) == session_downs() {
        return;
    }
    let app = NSApplication::sharedApplication(mtm);
    let until = NSDate::dateWithTimeIntervalSinceNow(PRESS_DELIVERY_WAIT_SECS);
    // SAFETY: a Foundation constant, defined for the life of the process.
    let mode = unsafe { NSDefaultRunLoopMode };
    if let Some(event) =
        app.nextEventMatchingMask_untilDate_inMode_dequeue(PRESS_MASK, Some(&until), mode, true)
    {
        app.sendEvent(&event);
    } else {
        eprintln!("menu: the press's mouse-down never reached Fidget; opening anyway");
    }
}

/// Which buttons count as down this tick, a Control-click as secondary. The
/// frame loop asks once per tick: a second call would take the next tick's
/// edges.
pub fn buttons_down() -> ButtonsDown {
    let left = ButtonSample::read(
        CGMouseButton::Left,
        CGEventType::LeftMouseDown,
        CGEventType::LeftMouseUp,
    );
    let right = ButtonSample::read(
        CGMouseButton::Right,
        CGEventType::RightMouseDown,
        CGEventType::RightMouseUp,
    );
    let at = Instant::now();
    let mut session = SESSION.lock().unwrap_or_else(PoisonError::into_inner);
    let Some(state) = session.as_mut() else {
        *session = Some(Session {
            at,
            left: Button::new(left),
            right: Button::new(right),
        });
        trace_start(left, right);
        return ButtonsDown::default();
    };
    let (left_before, right_before) = (state.left.last, state.right.last);
    let gap_ms = at.duration_since(state.at).as_secs_f64() * 1000.0;
    state.at = at;
    let buttons = control_click(ButtonsDown {
        primary: state.left.step(left),
        secondary: state.right.step(right),
    });
    drop(session);
    trace_tick(
        gap_ms,
        [("left", left_before, left), ("right", right_before, right)],
        buttons,
    );
    buttons
}

/// Logs the session's counters when the first tick seeds them, for #1858.
fn trace_start(left: ButtonSample, right: ButtonSample) {
    if !crate::dev_flags::TRACE_CLICKS.is_on() {
        return;
    }
    eprintln!(
        "click-trace: start t={:.1} left_downs={} left_ups={} right_downs={} right_ups={}",
        trace_ms(),
        left.downs,
        left.ups,
        right.downs,
        right.ups
    );
}

/// The `click-trace: tick` fields for a button that changed since `prev`, or
/// `None` when nothing moved.
fn button_change(prev: ButtonSample, now: ButtonSample) -> Option<String> {
    let edge = match (prev.level, now.level) {
        (false, true) => "down",
        (true, false) => "up",
        _ => "-",
    };
    let down_delta = now.downs.wrapping_sub(prev.downs);
    let up_delta = now.ups.wrapping_sub(prev.ups);
    if edge == "-" && down_delta == 0 && up_delta == 0 {
        return None;
    }
    Some(format!(
        "session={} session_edge={edge} down_delta={down_delta} up_delta={up_delta}",
        u8::from(now.level),
    ))
}

/// A tick this far behind the last one is logged even when nothing moved, so
/// idle and stalled ticks show up between presses.
const SLOW_TICK_MS: f64 = 50.0;

/// Logs what the session reported this tick and what the frame loop was told,
/// for #1858.
fn trace_tick(gap_ms: f64, changes: [(&str, ButtonSample, ButtonSample); 2], out: ButtonsDown) {
    if !crate::dev_flags::TRACE_CLICKS.is_on() {
        return;
    }
    let t = trace_ms();
    let mut quiet = true;
    for (name, before, now) in changes {
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

/// Seconds from AppKit as whole milliseconds. Zero or less is unset.
fn interval_ms_from_seconds(seconds: f64) -> Option<u32> {
    if seconds > 0.0 {
        Some((seconds * 1000.0).round() as u32)
    } else {
        None
    }
}

/// The OS double-click interval, in milliseconds.
pub fn double_click_interval_ms() -> Option<u32> {
    interval_ms_from_seconds(NSEvent::doubleClickInterval())
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn sample(level: bool, downs: u32, ups: u32) -> ButtonSample {
        ButtonSample { level, downs, ups }
    }

    /// The level the frame loop is told at each sample after the first.
    fn levels(samples: &[ButtonSample]) -> Vec<bool> {
        let mut button = Button::new(samples[0]);
        samples[1..].iter().map(|&now| button.step(now)).collect()
    }

    #[test]
    fn a_held_press_is_down_from_its_count_until_its_release() {
        assert_eq!(
            levels(&[
                sample(false, 4, 4),
                sample(true, 5, 4),
                sample(true, 5, 4),
                sample(false, 5, 5),
            ]),
            vec![true, true, false]
        );
    }

    /// The level never showed it, but the counters did: down one tick, up the next.
    #[test]
    fn a_tap_between_two_ticks_is_down_for_one_tick() {
        assert_eq!(
            levels(&[
                sample(false, 4, 4),
                sample(false, 5, 5),
                sample(false, 5, 5)
            ]),
            vec![true, false]
        );
    }

    /// The #1858 trace's 1546.7 ms Control-click tick: the level stayed down
    /// across a release and a new press, and only the counters moved.
    #[test]
    fn a_release_and_press_inside_one_tick_is_up_then_down_again() {
        assert_eq!(
            levels(&[
                sample(false, 4, 4),
                sample(true, 5, 4),
                sample(true, 6, 5),
                sample(true, 6, 5),
                sample(false, 6, 6),
            ]),
            vec![true, false, true, false]
        );
    }

    /// A tap is owed its up tick even when the next press is already held.
    #[test]
    fn a_press_right_after_a_tap_gets_its_own_down_edge() {
        assert_eq!(
            levels(&[
                sample(false, 4, 4),
                sample(false, 5, 5),
                sample(true, 6, 5),
                sample(true, 6, 5),
            ]),
            vec![true, false, true]
        );
    }

    /// Only a counted press starts one, so a button held before the first
    /// sample, or a level the counters never backed, is not a click.
    #[test]
    fn the_level_alone_never_starts_a_press() {
        assert_eq!(
            levels(&[sample(true, 4, 3), sample(true, 4, 3), sample(false, 4, 4)]),
            vec![false, false]
        );
    }

    #[test]
    fn a_counter_that_wraps_still_counts_one_press() {
        assert_eq!(
            levels(&[sample(false, u32::MAX, u32::MAX), sample(true, 0, u32::MAX)]),
            vec![true]
        );
    }

    /// Both Control-clicks of the 1546.7 ms tick open the menu, through the
    /// counters and the Control fold together.
    #[test]
    fn two_control_clicks_inside_one_tick_open_two_menus() {
        let samples = [
            sample(false, 4, 4),
            sample(true, 5, 4),
            sample(true, 6, 5),
            sample(true, 6, 5),
            sample(false, 6, 6),
        ];
        let ticks: Vec<_> = levels(&samples)
            .into_iter()
            .map(|primary| (primary, false, true))
            .collect();
        assert_eq!(verbs(&ticks), vec![Verb::Menu, Verb::Menu]);
    }

    /// One step of a session: a frame loop tick, or the overlay's pointerdown
    /// landing between ticks with the down counter where it then stood.
    enum Step {
        Tick(ButtonSample, bool),
        PointerDown(u32, bool),
    }

    /// Runs the left button through the counters, the overlay's pointerdown
    /// and the Control fold, with the cursor still on the sprite, and
    /// collects every verb. The first tick is the baseline.
    fn session(steps: &[Step]) -> Vec<Verb> {
        let Step::Tick(first, _) = steps[0] else {
            panic!("a session starts with a tick")
        };
        let mut left = Button::new(first);
        let mut press = PrimaryPress::Up;
        let mut pointer = Pointer::default();
        let at = Point { x: 100.0, y: 100.0 };
        let mut verbs = Vec::new();
        for step in &steps[1..] {
            match *step {
                Step::Tick(now, control) => {
                    let primary = left.step(now);
                    let buttons;
                    (press, buttons) = fold(
                        press,
                        ButtonsDown {
                            primary,
                            secondary: false,
                        },
                        control,
                    );
                    verbs.extend(pointer.update(true, buttons.primary, buttons.secondary, at, 400));
                }
                Step::PointerDown(downs, control) => {
                    press = press_began(press, left.pending(downs), control);
                }
            }
        }
        verbs
    }

    /// The overlay's pointerdown can trail the press by about 220 ms (#1795).
    /// A Control-tap's pointerdown that lands after the tick that saw it come
    /// up must not make the next plain click a Menu.
    #[test]
    fn a_late_pointerdown_after_a_control_tap_leaves_the_next_click_a_poke() {
        use Step::{PointerDown, Tick};
        assert_eq!(
            session(&[
                Tick(sample(false, 4, 4), false),
                Tick(sample(false, 5, 5), true),
                Tick(sample(false, 5, 5), true),
                PointerDown(5, true),
                Tick(sample(true, 6, 5), false),
                Tick(sample(false, 6, 6), false),
            ]),
            vec![Verb::Menu, Verb::Poke]
        );
    }

    /// The same late pointerdown after a plain tap must not turn the next
    /// Control-click into a Poke.
    #[test]
    fn a_late_pointerdown_after_a_plain_tap_leaves_the_next_control_click_a_menu() {
        use Step::{PointerDown, Tick};
        assert_eq!(
            session(&[
                Tick(sample(false, 4, 4), false),
                Tick(sample(false, 5, 5), false),
                Tick(sample(false, 5, 5), false),
                PointerDown(5, false),
                Tick(sample(true, 6, 5), true),
                Tick(sample(false, 6, 6), true),
            ]),
            vec![Verb::Poke, Verb::Menu]
        );
    }

    /// A release and a new Control-press inside one tick leave the new press
    /// owed to the next tick. Its pointerdown must still settle it, so the
    /// click stays a Menu after Control is let go.
    #[test]
    fn a_pointerdown_for_a_press_owed_to_the_next_tick_settles_a_control_click() {
        use Step::{PointerDown, Tick};
        assert_eq!(
            session(&[
                Tick(sample(false, 4, 4), false),
                Tick(sample(true, 5, 4), false),
                Tick(sample(true, 6, 5), true),
                PointerDown(6, true),
                Tick(sample(true, 6, 5), false),
                Tick(sample(false, 6, 6), false),
            ]),
            vec![Verb::Poke, Verb::Menu]
        );
    }

    /// A pointerdown that beats the tick still settles the press while
    /// Control is held, even if Control is let go before the tick reads it.
    #[test]
    fn a_pointerdown_before_the_tick_settles_a_control_click() {
        use Step::{PointerDown, Tick};
        assert_eq!(
            session(&[
                Tick(sample(false, 4, 4), false),
                PointerDown(5, true),
                Tick(sample(true, 5, 4), false),
                Tick(sample(false, 5, 5), false),
            ]),
            vec![Verb::Menu]
        );
    }

    #[test]
    fn a_still_button_logs_nothing() {
        let held = sample(true, 7, 6);
        assert_eq!(button_change(held, held), None);
    }

    #[test]
    fn a_press_seen_by_the_level_logs_its_edge_and_counter() {
        assert_eq!(
            button_change(sample(false, 7, 7), sample(true, 8, 7)).as_deref(),
            Some("session=1 session_edge=down down_delta=1 up_delta=0")
        );
    }

    /// A tap between two ticks never shows in the level, only in the counters.
    #[test]
    fn a_tap_between_ticks_logs_the_counter_deltas() {
        assert_eq!(
            button_change(sample(false, 7, 7), sample(false, 9, 9)).as_deref(),
            Some("session=0 session_edge=- down_delta=2 up_delta=2")
        );
    }

    #[test]
    fn double_click_interval_seconds_become_whole_milliseconds() {
        assert_eq!(interval_ms_from_seconds(0.5), Some(500));
        assert_eq!(interval_ms_from_seconds(1.5), Some(1500));
        assert_eq!(interval_ms_from_seconds(0.0024), Some(2));
        assert_eq!(interval_ms_from_seconds(0.0026), Some(3));
        assert_eq!(interval_ms_from_seconds(0.0), None);
        assert_eq!(interval_ms_from_seconds(-0.5), None);
    }
}
