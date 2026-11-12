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
//! drop the button the moment the cursor left the pixels. The button level
//! says when a press began and ended. Each button's down and up event
//! counters catch a click between two ticks, which the level misses. In the
//! #1858 trace the counters saw every press the overlay saw, and presses it
//! missed, so the overlay is not a third source here.

use std::ptr::NonNull;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::Instant;

use block2::RcBlock;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSEvent, NSEventMask};
use objc2_core_graphics::{
    CGEventFlags, CGEventSource, CGEventSourceStateID, CGEventType, CGMouseButton,
};
use objc2_foundation::{NSDate, NSDefaultRunLoopMode, NSProcessInfo};

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
/// there was a press at all is still the session's call, so a pointerdown
/// with no press pending is ignored.
pub fn primary_press_began() {
    let now = ButtonSample::read(
        CGMouseButton::Left,
        CGEventType::LeftMouseDown,
        CGEventType::LeftMouseUp,
    );
    let pending = SESSION
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .is_some_and(|session| session.left.pending(now));
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
        // Counters first: `Button` takes a count that lands a tick ahead of
        // its level edge for a second press.
        let downs = CGEventSource::counter_for_event_type(state, down);
        let ups = CGEventSource::counter_for_event_type(state, up);
        let level = CGEventSource::button_state(state, button);
        Self { level, downs, ups }
    }
}

/// Presses and releases since the baseline.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Edges {
    downs: u64,
    ups: u64,
}

/// One button as the frame loop is told it, one edge per tick. The level
/// misses a press between two ticks and the macOS 27.2 counters move only
/// every 500 ms, so each edge is taken from whichever source shows it first.
#[derive(Clone, Copy, Debug)]
struct Button {
    last: ButtonSample,
    seen: Edges,
    /// Edges the level showed whose counts are still to land.
    uncounted: Edges,
    told: Edges,
    /// A press held at the baseline has no down. Both sources count its
    /// release, so this absorbs exactly that one release from each.
    held_at_start: bool,
    down: bool,
}

impl Button {
    /// The first sample is the baseline: a button already held when Fidget
    /// starts was never pressed here.
    fn new(first: ButtonSample) -> Self {
        Self {
            last: first,
            seen: Edges::default(),
            uncounted: Edges::default(),
            told: Edges::default(),
            held_at_start: first.level,
            down: false,
        }
    }

    fn observe(&mut self, now: ButtonSample) {
        let (pressed, released) = (!self.last.level && now.level, self.last.level && !now.level);
        let mut downs = u64::from(now.downs.wrapping_sub(self.last.downs));
        let mut ups = u64::from(now.ups.wrapping_sub(self.last.ups));
        // Counts land in event order, so with the held press's down still
        // uncounted, an up count past the uncounted releases ended an earlier
        // click. It is told ahead of the hold or not at all, never inside it.
        if now.level && self.uncounted.downs + u64::from(pressed) > self.uncounted.ups {
            let earlier = downs.min(ups.saturating_sub(self.uncounted.ups));
            downs -= earlier;
            ups -= earlier;
            if pressed {
                self.seen.downs += earlier;
                self.seen.ups += earlier;
            }
        }
        merge(
            &mut self.seen.downs,
            &mut self.uncounted.downs,
            pressed,
            downs,
        );
        merge(&mut self.seen.ups, &mut self.uncounted.ups, released, ups);
        self.last = now;
    }

    fn presses(&self) -> u64 {
        self.seen.downs
    }

    fn releases(&self) -> u64 {
        self.seen.ups.saturating_sub(u64::from(self.held_at_start))
    }

    /// Whether a press is under way at `now`: held, or seen and not yet told.
    fn pending(&self, now: ButtonSample) -> bool {
        let mut ahead = *self;
        ahead.observe(now);
        ahead.down || ahead.presses() > ahead.told.downs
    }

    /// One edge per tick, so a press or a release that fit between two ticks
    /// still gets a tick of its own, and a double-click the counters report
    /// at once still reaches the frame loop as two presses.
    fn step(&mut self, now: ButtonSample) -> bool {
        self.observe(now);
        if !self.down && self.presses() > self.told.downs {
            self.told.downs += 1;
            self.down = true;
        } else if self.down && self.releases() > self.told.ups {
            self.told.ups += 1;
            self.down = false;
        }
        self.down
    }
}

/// Adds one tick's edges of one kind. A count lands with its level edge or
/// after it, never before, so it repeats an edge the level showed only while
/// one is `uncounted`. Any other count is an edge the level missed.
fn merge(seen: &mut u64, uncounted: &mut u64, level_edge: bool, counted: u64) {
    *uncounted += u64::from(level_edge);
    let repeats = counted.min(*uncounted);
    *uncounted -= repeats;
    *seen += u64::from(level_edge) + counted - repeats;
}

struct Session {
    at: Instant,
    left: Button,
    right: Button,
}

static SESSION: Mutex<Option<Session>> = Mutex::new(None);

/// When the session's last left or right press happened, in any app, on the
/// clock `NSEvent` timestamps use.
fn last_session_press() -> f64 {
    let now = NSProcessInfo::processInfo().systemUptime();
    let state = CGEventSourceStateID::CombinedSessionState;
    let since = |kind| CGEventSource::seconds_since_last_event_type(state, kind);
    now - since(CGEventType::LeftMouseDown).min(since(CGEventType::RightMouseDown))
}

/// Read back from its age, a press lands within microseconds of its own
/// mouse-down's timestamp. Two presses a millisecond apart are not a hand's.
const SAME_PRESS_SECS: f64 = 0.001;

/// Whether the session's last press is newer than the last mouse-down AppKit
/// delivered, so its own mouse-down is still to come.
fn press_pending(delivered_at: f64, pressed_at: f64) -> bool {
    pressed_at - delivered_at > SAME_PRESS_SECS
}

/// The timestamp of the last mouse-down AppKit handed this app, as `f64` bits.
static DELIVERED_AT: AtomicU64 = AtomicU64::new(0);

fn delivered_at() -> f64 {
    f64::from_bits(DELIVERED_AT.load(Ordering::Relaxed))
}

const PRESS_MASK: NSEventMask = NSEventMask::LeftMouseDown.union(NSEventMask::RightMouseDown);

/// Start noting which presses AppKit has delivered. Main thread, once.
pub fn watch_delivered_presses() {
    if MainThreadMarker::new().is_none() {
        return;
    }
    let now = NSProcessInfo::processInfo().systemUptime();
    DELIVERED_AT.store(now.to_bits(), Ordering::Relaxed);
    let block = RcBlock::new(|event: NonNull<NSEvent>| {
        // SAFETY: AppKit hands the monitor a live event.
        let at = unsafe { event.as_ref() }.timestamp();
        DELIVERED_AT.store(at.to_bits(), Ordering::Relaxed);
        event.as_ptr()
    });
    // SAFETY: main thread; the block hands back the event it was given.
    let monitor =
        unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(PRESS_MASK, &block) };
    // The monitor lives as long as the app.
    std::mem::forget(monitor);
}

/// Measured mouse-downs arrived up to 40 ms after the session saw the press;
/// a pointerdown has trailed by 220 ms. Only a press that never comes, such
/// as one another app took, pays all of it.
const PRESS_DELIVERY_WAIT_SECS: f64 = 0.25;

/// The session sees a press before AppKit delivers its mouse-down, and a menu
/// that starts tracking first takes that mouse-down as a click that closes it.
/// So dispatch it, and any older one queued ahead of it, first.
pub fn deliver_pending_press() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    let until = NSDate::dateWithTimeIntervalSinceNow(PRESS_DELIVERY_WAIT_SECS);
    // SAFETY: a Foundation constant, defined for the life of the process.
    let mode = unsafe { NSDefaultRunLoopMode };
    while press_pending(delivered_at(), last_session_press()) {
        let Some(event) = app.nextEventMatchingMask_untilDate_inMode_dequeue(
            PRESS_MASK,
            Some(&until),
            mode,
            true,
        ) else {
            eprintln!("menu: no mouse-down matched the last press in time; opening anyway");
            return;
        };
        app.sendEvent(&event);
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

    /// A button held before the first sample was never pressed here, and its
    /// release must not end the next press early.
    #[test]
    fn a_button_held_at_the_baseline_is_not_a_press() {
        assert_eq!(
            levels(&[
                sample(true, 4, 3),
                sample(true, 4, 3),
                sample(false, 4, 4),
                sample(true, 5, 4),
                sample(true, 5, 4),
            ]),
            vec![false, false, true, true]
        );
    }

    /// macOS 27.2 moves the counters every 500 ms. The level's press is told
    /// at once, and the counts that catch up later are the same press.
    #[test]
    fn a_press_the_counters_report_late_is_one_press_told_on_time() {
        assert_eq!(
            levels(&[
                sample(false, 4, 4),
                sample(true, 4, 4),
                sample(false, 4, 4),
                sample(false, 5, 5),
                sample(false, 5, 5),
            ]),
            vec![true, false, false, false]
        );
    }

    /// The counters missed the press, the level went down: a press.
    #[test]
    fn a_level_edge_with_no_count_is_a_press() {
        assert_eq!(
            levels(&[sample(false, 4, 4), sample(true, 4, 4), sample(true, 4, 4)]),
            vec![true, true]
        );
    }

    /// Two taps between ticks that the counters report at once still reach
    /// the frame loop as two presses.
    #[test]
    fn two_taps_counted_at_once_are_two_presses() {
        assert_eq!(
            levels(&[
                sample(false, 4, 4),
                sample(false, 6, 6),
                sample(false, 6, 6),
                sample(false, 6, 6),
                sample(false, 6, 6),
            ]),
            vec![true, false, true, false]
        );
    }

    /// A double-click the level saw press by press, with the counters
    /// catching up after it: the Summon, and no third press.
    #[test]
    fn a_double_click_seen_by_the_level_is_a_summon_with_late_counters() {
        let samples = [
            sample(false, 4, 4),
            sample(true, 4, 4),
            sample(false, 4, 4),
            sample(true, 4, 4),
            sample(false, 4, 4),
            sample(false, 6, 6),
            sample(false, 6, 6),
        ];
        let mut button = Button::new(samples[0]);
        let mut pointer = Pointer::default();
        let at = Point { x: 100.0, y: 100.0 };
        let verbs: Vec<_> = samples[1..]
            .iter()
            .flat_map(|&now| pointer.update(true, button.step(now), false, at, 16))
            .collect();
        assert_eq!(verbs, vec![Verb::Poke, Verb::Summon]);
    }

    /// A tap the level missed is told when its count lands, after a press the
    /// level saw inside the same counter window. Two real clicks that close
    /// together still read as a double-click: the Poke, then the Summon.
    #[test]
    fn a_missed_tap_counted_after_a_level_press_pairs_into_a_summon() {
        let samples = [
            sample(false, 4, 4),
            sample(false, 4, 4),
            sample(true, 4, 4),
            sample(false, 4, 4),
            sample(false, 6, 6),
            sample(false, 6, 6),
            sample(false, 6, 6),
        ];
        let mut button = Button::new(samples[0]);
        let mut pointer = Pointer::default();
        let at = Point { x: 100.0, y: 100.0 };
        let verbs: Vec<_> = samples[1..]
            .iter()
            .flat_map(|&now| pointer.update(true, button.step(now), false, at, 16))
            .collect();
        assert_eq!(verbs, vec![Verb::Poke, Verb::Summon]);
    }

    /// A flick on the sprite at 16 ms ticks, 20 points a tick: 1250 points a
    /// second. The 27.2 counters land once the hand has stopped. `before` is
    /// what the button did first, off the sprite, from the baseline on.
    fn flick_after(before: &[ButtonSample]) -> Vec<Verb> {
        let mut button = Button::new(before[0]);
        let mut pointer = Pointer::default();
        let mut at = Point { x: 100.0, y: 100.0 };
        let mut verbs = Vec::new();
        for &now in &before[1..] {
            verbs.extend(pointer.update(false, button.step(now), false, at, 16));
        }
        let ButtonSample { downs, ups, .. } = before[before.len() - 1];
        let held = [(sample(true, downs, ups), 20.0); 10];
        let let_go = [(sample(false, downs, ups), 20.0)];
        let still = [(sample(false, downs, ups), 0.0); 10];
        let counted = [(sample(false, downs + 1, ups + 1), 0.0); 3];
        for (now, step) in held.into_iter().chain(let_go).chain(still).chain(counted) {
            at.x += step;
            verbs.extend(pointer.update(true, button.step(now), false, at, 16));
        }
        verbs.retain(|verb| !matches!(verb, Verb::Grab));
        verbs
    }

    fn assert_thrown_at_the_hands_speed(verbs: &[Verb]) {
        match verbs {
            [Verb::Throw { velocity }] => {
                assert!((velocity.x - 1250.0).abs() < 1.0, "{velocity:?}");
                assert_eq!(velocity.y, 0.0);
            }
            other => panic!("expected one Throw, got {other:?}"),
        }
    }

    #[test]
    fn a_flick_with_late_counters_is_a_throw() {
        assert_thrown_at_the_hands_speed(&flick_after(&[sample(false, 4, 4)]));
    }

    /// An idle frame loop polls up to a second apart, so a click on a menu
    /// item is a tap only the counters see. The next release must still be
    /// told when the level drops, not when its count lands.
    #[test]
    fn a_flick_after_a_tap_the_level_missed_is_still_a_throw() {
        assert_thrown_at_the_hands_speed(&flick_after(&[
            sample(false, 4, 4),
            sample(false, 5, 5),
            sample(false, 5, 5),
        ]));
    }

    /// A flick on the sprite that starts within the counters' lag of a tap
    /// the level missed. `landed` is what the counters added by mid-hold,
    /// and they reach both clicks once the hand has stopped.
    fn flick_with_counts_landing_mid_hold(landed: (u32, u32)) -> Vec<Verb> {
        let mut button = Button::new(sample(false, 4, 4));
        let mut pointer = Pointer::default();
        let mut at = Point { x: 100.0, y: 100.0 };
        let mid_hold = sample(true, 4 + landed.0, 4 + landed.1);
        let held = [(sample(true, 4, 4), 20.0); 5];
        let held_on = [(mid_hold, 20.0); 5];
        let let_go = [(sample(false, mid_hold.downs, mid_hold.ups), 20.0)];
        let still = [(sample(false, mid_hold.downs, mid_hold.ups), 0.0); 10];
        let counted = [(sample(false, 6, 6), 0.0); 3];
        let ticks = held.into_iter().chain(held_on).chain(let_go);
        let mut verbs = Vec::new();
        for (now, step) in ticks.chain(still).chain(counted) {
            at.x += step;
            verbs.extend(pointer.update(true, button.step(now), false, at, 16));
        }
        verbs.retain(|verb| !matches!(verb, Verb::Grab));
        verbs
    }

    /// The tap's counts land while the flick's press is held. They are not
    /// that press's release: the grab holds, and no click follows the throw.
    #[test]
    fn a_missed_taps_counts_landing_mid_hold_do_not_end_the_grab() {
        assert_thrown_at_the_hands_speed(&flick_with_counts_landing_mid_hold((1, 1)));
    }

    /// The same, when the counters catch up with the held press's down too.
    #[test]
    fn a_missed_taps_counts_landing_with_the_held_presss_down_do_not_end_the_grab() {
        assert_thrown_at_the_hands_speed(&flick_with_counts_landing_mid_hold((2, 1)));
    }

    /// The tap's counts land on the tick the level shows the next press. The
    /// press is not the tap: it is told after it, at once, and stays held.
    #[test]
    fn a_missed_tap_counted_as_the_next_press_shows_is_a_click_then_a_hold() {
        assert_eq!(
            levels(&[
                sample(false, 4, 4),
                sample(true, 5, 5),
                sample(true, 5, 5),
                sample(true, 5, 5),
                sample(true, 5, 5),
            ]),
            vec![true, false, true, true]
        );
    }

    /// Mid-hold, with the held press's down already counted, a release and
    /// re-press inside one poll are the press's own: up a tick, then down.
    #[test]
    fn a_release_and_press_inside_one_tick_of_a_counted_hold_is_up_then_down_again() {
        assert_eq!(
            levels(&[
                sample(false, 4, 4),
                sample(true, 5, 4),
                sample(true, 5, 4),
                sample(true, 5, 4),
                sample(true, 6, 5),
                sample(true, 6, 5),
                sample(true, 6, 5),
            ]),
            vec![true, true, true, false, true, true]
        );
    }

    /// The level saw the first click, and the second fit inside the poll that
    /// saw the release. Both clicks' counts land there: the second is a press.
    #[test]
    fn a_second_tap_counted_on_the_release_tick_is_its_own_press() {
        assert_eq!(
            levels(&[
                sample(false, 4, 4),
                sample(true, 4, 4),
                sample(false, 6, 6),
                sample(false, 6, 6),
                sample(false, 6, 6),
            ]),
            vec![true, false, true, false]
        );
    }

    /// A click the level saw end to end, counted only as the next press shows
    /// with that press's down: the click's up count does not end the press.
    #[test]
    fn a_seen_click_counted_as_the_next_press_shows_does_not_end_that_press() {
        assert_eq!(
            levels(&[
                sample(false, 4, 4),
                sample(true, 4, 4),
                sample(false, 4, 4),
                sample(true, 6, 5),
                sample(true, 6, 5),
                sample(false, 6, 5),
                sample(false, 6, 6),
            ]),
            vec![true, false, true, true, false, false]
        );
    }

    /// The 27.2 Control-click: Control is let go before the counters move,
    /// but the level saw the press while it was held.
    #[test]
    fn a_control_click_with_late_counters_is_a_menu() {
        use Step::Tick;
        assert_eq!(
            session(&[
                Tick(sample(false, 4, 4), false),
                Tick(sample(true, 4, 4), true),
                Tick(sample(false, 4, 4), false),
                Tick(sample(false, 5, 5), false),
            ]),
            vec![Verb::Menu]
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
        PointerDown(ButtonSample, bool),
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
                Step::PointerDown(now, control) => {
                    press = press_began(press, left.pending(now), control);
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
                PointerDown(sample(false, 5, 5), true),
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
                PointerDown(sample(false, 5, 5), false),
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
                PointerDown(sample(true, 6, 5), true),
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
                PointerDown(sample(true, 5, 4), true),
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
    fn the_last_press_delivered_leaves_nothing_pending() {
        assert!(!press_pending(100.0, 100.000_002));
        assert!(!press_pending(100.0, 99.999_998));
    }

    /// A late mouse-down for an older press says nothing of the newer one.
    #[test]
    fn an_older_press_delivered_late_leaves_the_newer_one_pending() {
        assert!(press_pending(100.0, 100.120));
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
