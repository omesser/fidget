//! Turning a pointer into interaction verbs: Poke, Grab, Throw, Summon
//! (double-click) and Menu (right-click). Pure, so every gesture is tested by
//! writing it down rather than performing it.

use crate::engine::{Point, Verb};

/// How far the cursor travels before a press becomes a Grab, in points. Large
/// enough that a click with an unsteady hand stays a click, small enough that
/// picking the sprite up feels immediate.
const DRAG_THRESHOLD: f64 = 4.0;

/// How long a press is held before it becomes a Grab without moving at all:
/// pressing and holding is how you pick something up when you do not want to
/// move it yet.
const DRAG_DELAY_MS: u32 = 180;

/// How fast the hand must still be moving for a release to be a Throw, in
/// points per second. Nobody holds a mouse perfectly still, so testing for no
/// movement at all would make every release a throw at the last twitch's speed.
const THROW_MIN_SPEED: f64 = 80.0;

/// How long a throw's velocity is measured over, in milliseconds. The last tick
/// alone is hostage to one jittery sample; the whole gesture averages away the
/// flick at the end.
const VELOCITY_WINDOW_MS: u32 = 60;

/// What the pointer is currently doing to the sprite.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// The button is up, or was pressed somewhere that is not the sprite.
    Idle,
    /// Pressed on the sprite, and not yet known to be a Poke or a Grab.
    Pressed,
    /// Holding the sprite.
    Grabbing,
}

/// One tick of pointer input.
#[derive(Clone, Copy, Debug)]
pub struct Pointer {
    phase: Phase,
    /// The button last tick, so a press is a transition rather than a state.
    /// Without this, dragging a window under a resting sprite picks the sprite
    /// up the moment the cursor crosses it.
    was_held: bool,
    /// Secondary button last tick. Menu is the press edge, not held state.
    was_secondary_held: bool,
    /// Where the press began, so travel is measured from it rather than from
    /// the previous tick — a slow drag never moves far in 16ms.
    pressed_at: Point,
    pressed_ms: u32,
    /// `None` until the first tick. Assuming an origin would make the first
    /// tick's travel the whole cursor position, and a Grab that began inside
    /// the velocity window would be thrown at a speed nobody moved at.
    cursor: Option<Point>,
    /// Cursor travel and the time it took, in two buckets. The older one is
    /// kept so the window never empties: a release just after a reset would
    /// otherwise measure a flick as stillness.
    travel: Point,
    travel_ms: u32,
    prior: Point,
    prior_ms: u32,
    /// Since the last click, so the next one can tell a double-click from a
    /// second prod. `u32::MAX` means there is no click to pair with.
    since_click_ms: u32,
    /// Whether this run of clicks has already summoned. Spending the pair
    /// alone would only re-arm it on the click after, so drumming on the
    /// sprite would open a chat surface every second click.
    summoned: bool,
    /// A Poke went out and its pair window is still open.
    unsettled_poke: bool,
    /// See `poke_settled`.
    poke_settled: bool,
    /// How close together two clicks must be to count as one double-click, in
    /// milliseconds. Injected from the OS double-click setting, or the
    /// fallback when that is not available.
    double_click_ms: u32,
}

impl Default for Pointer {
    fn default() -> Self {
        Self::with_double_click_ms(400)
    }
}

impl Pointer {
    /// A pointer with an explicit double-click interval, injected by the
    /// platform layer from the OS setting so core stays platform-neutral.
    pub fn with_double_click_ms(double_click_ms: u32) -> Self {
        Self {
            phase: Phase::Idle,
            was_held: false,
            was_secondary_held: false,
            pressed_at: Point::default(),
            pressed_ms: 0,
            cursor: None,
            travel: Point::default(),
            travel_ms: 0,
            prior: Point::default(),
            prior_ms: 0,
            since_click_ms: u32::MAX,
            summoned: false,
            unsettled_poke: false,
            poke_settled: false,
            double_click_ms,
        }
    }
}

impl Pointer {
    /// The verbs this tick. `over_sprite` is the Shell's alpha hit-test and
    /// decides whether a press belongs to the sprite at all; once a Grab is
    /// under way the cursor is free to leave the art, which is what dragging is.
    pub fn update(
        &mut self,
        over_sprite: bool,
        held: bool,
        secondary_held: bool,
        cursor: Point,
        elapsed_ms: u32,
    ) -> Vec<Verb> {
        let moved = match self.cursor {
            Some(previous) => Point {
                x: cursor.x - previous.x,
                y: cursor.y - previous.y,
            },
            None => Point::default(),
        };
        self.cursor = Some(cursor);
        self.sample(moved, elapsed_ms);
        self.since_click_ms = self.since_click_ms.saturating_add(elapsed_ms);
        self.poke_settled = self.unsettled_poke && self.since_click_ms > self.double_click_ms;
        self.unsettled_poke &= !self.poke_settled;

        let pressed = held && !self.was_held;
        self.was_held = held;

        let secondary_pressed = secondary_held && !self.was_secondary_held;
        self.was_secondary_held = secondary_held;

        let mut verbs = Vec::new();

        if secondary_pressed && over_sprite {
            verbs.push(Verb::Menu);
        }

        let gesture_verbs = match (self.phase, held) {
            // Pressing anywhere else is somebody else's click, and a button
            // already down when the cursor arrives was never pressed here.
            (Phase::Idle, true) if over_sprite && pressed => {
                self.phase = Phase::Pressed;
                self.pressed_at = cursor;
                self.pressed_ms = 0;
                Vec::new()
            }
            (Phase::Idle, _) => Vec::new(),

            (Phase::Pressed, true) => {
                self.pressed_ms = self.pressed_ms.saturating_add(elapsed_ms);
                if self.dragging_yet() {
                    self.phase = Phase::Grabbing;
                    // A gesture between two clicks is not a double-click.
                    self.since_click_ms = u32::MAX;
                    vec![Verb::Grab]
                } else {
                    Vec::new()
                }
            }
            // Let go without ever becoming a drag: a click, and so a Poke. Where
            // the sprite is now does not come into it: it can fall out from under
            // a held press, and a click that landed on it is a click on it.
            (Phase::Pressed, false) => {
                self.phase = Phase::Idle;
                let paired = self.since_click_ms <= self.double_click_ms;
                self.since_click_ms = 0;
                // A gap ends the run, and the next pair may summon again.
                self.summoned &= paired;
                // The second click is a Summon instead of a Poke, not as well,
                // so a cue keyed on the verb stream hears one gesture. The first
                // already went out as a Poke: holding the verb back would delay
                // every click's reaction, so `poke_settled` holds only the Director's.
                let summon = paired && !self.summoned;
                self.summoned |= summon;
                self.unsettled_poke = !summon;
                if summon {
                    vec![Verb::Summon]
                } else {
                    vec![Verb::Poke]
                }
            }

            (Phase::Grabbing, true) => vec![Verb::Grab],
            // A Throw only when the hand was moving. Released still, the sprite
            // simply drops, which the Engine does for any Grab that stops.
            (Phase::Grabbing, false) => {
                self.phase = Phase::Idle;
                let velocity = self.velocity();
                if velocity.x.hypot(velocity.y) < THROW_MIN_SPEED {
                    Vec::new()
                } else {
                    vec![Verb::Throw { velocity }]
                }
            }
        };

        verbs.extend(gesture_verbs);
        verbs
    }

    /// Whether a Poke settled this tick: its pair window closed with no second
    /// click. `Verb::Poke` is the click the sprite reacts to at once; this is
    /// the Poke the Director hears. Never true for the first click of a double-click.
    pub fn poke_settled(&self) -> bool {
        self.poke_settled
    }

    /// Whether the sprite is being held. The Shell suspends its hit-test while true.
    pub fn grabbing(&self) -> bool {
        self.phase == Phase::Grabbing
    }

    /// Whether this pointer is in a gesture at all, held or not yet decided.
    /// Wider than `grabbing` on purpose: a press not yet a Grab still belongs to
    /// the Instance it landed on, or one press could pick up two Instances.
    pub fn gesturing(&self) -> bool {
        self.phase != Phase::Idle
    }

    /// Where the press under way landed, or `None` with no press under way.
    /// The first Grab is reported only after the cursor has moved off it.
    pub fn pressed_at(&self) -> Option<Point> {
        self.gesturing().then_some(self.pressed_at)
    }

    fn dragging_yet(&self) -> bool {
        let travelled = self.cursor.is_some_and(|cursor| {
            (cursor.x - self.pressed_at.x).hypot(cursor.y - self.pressed_at.y) >= DRAG_THRESHOLD
        });
        travelled || self.pressed_ms >= DRAG_DELAY_MS
    }

    /// Accumulate recent travel, rolling the older half out as it ages. Two
    /// buckets rather than a ring: the window lands anywhere between half and
    /// all of `VELOCITY_WINDOW_MS`, close enough for a throw and less bookkeeping.
    fn sample(&mut self, moved: Point, elapsed_ms: u32) {
        if self.travel_ms >= VELOCITY_WINDOW_MS / 2 {
            self.prior = self.travel;
            self.prior_ms = self.travel_ms;
            self.travel = Point::default();
            self.travel_ms = 0;
        }
        self.travel.x += moved.x;
        self.travel.y += moved.y;
        self.travel_ms = self.travel_ms.saturating_add(elapsed_ms);
    }

    /// Points per second, from the recent travel.
    fn velocity(&self) -> Point {
        let millis = self.travel_ms.saturating_add(self.prior_ms);
        if millis == 0 {
            return Point::default();
        }
        let seconds = f64::from(millis) / 1000.0;
        Point {
            x: (self.travel.x + self.prior.x) / seconds,
            y: (self.travel.y + self.prior.y) / seconds,
        }
    }
}

/// Which Instance the pointer is acting on: `hits` is the alpha hit-test per
/// Instance in draw order, `holding` the Instance mid-Grab. One cursor and
/// several sprites need one arbiter, or one press would pick up two sprites.
pub fn press_target(hits: &[bool], holding: Option<usize>) -> Option<usize> {
    // A held Instance keeps the press wherever the cursor goes, so a drag that
    // outruns the art is neither dropped nor handed to a sprite it crosses. An
    // index past the end is an Instance dismissed mid-drag: fall back to the cursor.
    if holding.is_some_and(|index| index < hits.len()) {
        return holding;
    }

    // The last hit, not the first: sprites are drawn in roster order, so the
    // last one drawn is the one on top, and the one on top is the one the user
    // sees under the cursor.
    hits.iter().rposition(|hit| *hit)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f64, y: f64) -> Point {
        Point { x, y }
    }

    /// A tick of the frame loop, at roughly the rate it runs.
    const TICK: u32 = 16;

    /// A whole click: press and release without moving.
    fn click(pointer: &mut Pointer) -> Vec<Verb> {
        pointer.update(true, true, false, at(100.0, 100.0), TICK);
        pointer.update(true, false, false, at(100.0, 100.0), TICK)
    }

    /// Waits out the double-click interval with the button up.
    fn pause(pointer: &mut Pointer) {
        for _ in 0..pointer.double_click_ms / TICK + 2 {
            pointer.update(true, false, false, at(100.0, 100.0), TICK);
        }
    }

    /// Two clicks in quick succession are a Summon, the deliberate act that
    /// opens the chat surface. The first is still a Poke, nothing knows a second
    /// is coming, and the second is the Summon alone: one verb per gesture.
    #[test]
    fn two_quick_clicks_poke_then_summon() {
        let mut pointer = Pointer::default();

        assert_eq!(click(&mut pointer), vec![Verb::Poke]);
        assert_eq!(click(&mut pointer), vec![Verb::Summon]);
    }

    /// The interval is the whole of what separates prodding the sprite twice
    /// from asking it for the chat surface.
    #[test]
    fn two_clicks_further_apart_are_two_pokes() {
        let mut pointer = Pointer::default();

        assert_eq!(click(&mut pointer), vec![Verb::Poke]);
        pause(&mut pointer);
        assert_eq!(click(&mut pointer), vec![Verb::Poke]);
    }

    /// A pair is spent for the whole run of clicks, not re-armed by the next
    /// one. Drumming on the sprite would otherwise open the Chat surface every
    /// second click, and each one of those is a window.
    #[test]
    fn drumming_on_the_sprite_summons_once() {
        let mut pointer = Pointer::default();

        let drummed: Vec<Vec<Verb>> = (0..6).map(|_| click(&mut pointer)).collect();

        assert_eq!(
            drummed,
            vec![
                vec![Verb::Poke],
                vec![Verb::Summon],
                vec![Verb::Poke],
                vec![Verb::Poke],
                vec![Verb::Poke],
                vec![Verb::Poke],
            ]
        );
    }

    /// And re-arms once the drumming stops: two clicks after a pause are a
    /// fresh double-click, not part of the run that already summoned.
    #[test]
    fn a_pause_re_arms_the_double_click() {
        let mut pointer = Pointer::default();
        click(&mut pointer);
        assert_eq!(click(&mut pointer), vec![Verb::Summon]);
        click(&mut pointer);

        pause(&mut pointer);

        assert_eq!(click(&mut pointer), vec![Verb::Poke]);
        assert_eq!(click(&mut pointer), vec![Verb::Summon]);
    }

    /// Clicking, picking the sprite up and putting it down, then clicking again
    /// is two clicks with a gesture between them, not a double-click.
    #[test]
    fn a_grab_between_two_clicks_is_not_a_double_click() {
        let mut pointer = Pointer::default();
        assert_eq!(click(&mut pointer), vec![Verb::Poke]);

        pointer.update(true, true, false, at(100.0, 100.0), TICK);
        assert_eq!(
            pointer.update(true, true, false, at(120.0, 100.0), TICK),
            vec![Verb::Grab]
        );
        pointer.update(true, false, false, at(120.0, 100.0), TICK);

        assert_eq!(click(&mut pointer), vec![Verb::Poke]);
    }

    #[test]
    fn a_click_on_the_sprite_is_a_poke_and_never_moves_it() {
        let mut pointer = Pointer::default();

        assert_eq!(
            pointer.update(true, true, false, at(100.0, 100.0), TICK),
            vec![]
        );
        assert_eq!(
            pointer.update(true, true, false, at(100.0, 100.0), TICK),
            vec![],
            "a press alone commits to nothing"
        );

        assert_eq!(
            pointer.update(true, false, false, at(100.0, 100.0), TICK),
            vec![Verb::Poke]
        );
        assert!(!pointer.grabbing());
    }

    /// The bug this whole shape exists to avoid: deciding on the press would
    /// snap the sprite to the cursor and drop it on every click.
    #[test]
    fn a_click_emits_no_grab_at_any_point() {
        let mut pointer = Pointer::default();
        let mut verbs = Vec::new();

        verbs.extend(pointer.update(true, true, false, at(100.0, 100.0), TICK));
        // A hand is not steady; a click drifts a pixel.
        verbs.extend(pointer.update(true, true, false, at(101.0, 100.0), TICK));
        verbs.extend(pointer.update(true, false, false, at(101.0, 100.0), TICK));

        assert_eq!(verbs, vec![Verb::Poke]);
    }

    #[test]
    fn dragging_past_the_threshold_grabs_the_sprite() {
        let mut pointer = Pointer::default();
        pointer.update(true, true, false, at(100.0, 100.0), TICK);

        assert_eq!(
            pointer.update(true, true, false, at(103.0, 100.0), TICK),
            vec![],
            "three points is still a click"
        );
        assert_eq!(
            pointer.update(true, true, false, at(110.0, 100.0), TICK),
            vec![Verb::Grab]
        );
        assert!(pointer.grabbing(), "so the hit-test is suspended");
    }

    /// Picking the sprite up without moving it yet.
    #[test]
    fn holding_still_long_enough_also_grabs() {
        let mut pointer = Pointer::default();
        pointer.update(true, true, false, at(100.0, 100.0), TICK);

        let held = (0..DRAG_DELAY_MS / TICK + 2)
            .map(|_| pointer.update(true, true, false, at(100.0, 100.0), TICK))
            .find(|verbs| !verbs.is_empty());

        assert_eq!(held, Some(vec![Verb::Grab]));
        assert!(pointer.grabbing());
    }

    /// The double-click interval is injected from the platform layer, so tests
    /// can set it explicitly rather than depending on a constant.
    #[test]
    fn double_click_interval_is_injected() {
        let mut fast = Pointer::with_double_click_ms(100);
        let mut slow = Pointer::with_double_click_ms(1000);

        assert_eq!(click(&mut fast), vec![Verb::Poke]);
        assert_eq!(click(&mut fast), vec![Verb::Summon], "100ms is fast");

        assert_eq!(click(&mut slow), vec![Verb::Poke]);
        for _ in 0..5 {
            slow.update(true, false, false, at(100.0, 100.0), TICK);
        }
        assert_eq!(click(&mut slow), vec![Verb::Summon], "1000ms is generous");

        // Negative case: with a 100ms interval, a 200ms gap should produce two
        // Pokes, not a Summon. This fails if the injected value is ignored.
        let mut tight = Pointer::with_double_click_ms(100);
        assert_eq!(click(&mut tight), vec![Verb::Poke]);
        for _ in 0..13 {
            tight.update(true, false, false, at(100.0, 100.0), TICK);
        }
        assert_eq!(
            click(&mut tight),
            vec![Verb::Poke],
            "200ms gap exceeds 100ms interval"
        );
    }

    /// Once held, the cursor is free to leave the art. That is the whole point:
    /// the sprite follows the hand rather than the hand being pinned to it.
    #[test]
    fn a_grab_survives_the_cursor_leaving_the_sprite() {
        let mut pointer = Pointer::default();
        pointer.update(true, true, false, at(100.0, 100.0), TICK);
        pointer.update(true, true, false, at(120.0, 100.0), TICK);
        assert!(pointer.grabbing());

        assert_eq!(
            pointer.update(false, true, false, at(400.0, 300.0), TICK),
            vec![Verb::Grab],
            "the drag outran the sprite and still holds it"
        );
        assert!(pointer.grabbing());
    }

    /// A flick's first Grab arrives 20 points past the press, and the press
    /// point is still what the Engine is handed, until the release.
    #[test]
    fn a_grab_reports_where_the_press_landed() {
        let mut pointer = Pointer::default();
        assert_eq!(pointer.pressed_at(), None);
        pointer.update(true, true, false, at(100.0, 100.0), TICK);
        assert_eq!(
            pointer.update(true, true, false, at(120.0, 100.0), TICK),
            vec![Verb::Grab]
        );
        assert_eq!(pointer.pressed_at(), Some(at(100.0, 100.0)));
        pointer.update(false, false, false, at(130.0, 100.0), TICK);
        assert_eq!(pointer.pressed_at(), None);
    }

    #[test]
    fn releasing_a_moving_grab_throws_the_sprite() {
        let mut pointer = Pointer::default();
        pointer.update(true, true, false, at(100.0, 100.0), TICK);
        pointer.update(true, true, false, at(120.0, 100.0), TICK);

        pointer.update(false, true, false, at(136.0, 100.0), TICK);
        pointer.update(false, true, false, at(152.0, 100.0), TICK);
        let thrown = pointer.update(false, false, false, at(152.0, 100.0), TICK);

        // 52 points across five 16ms ticks is 650 points a second. The window
        // covers the recent ticks rather than the whole gesture, so the figure is
        // near rather than on it, and a quarter off is not the throw the user made.
        let hand = 52.0 / (5.0 * f64::from(TICK) / 1000.0);
        match thrown.as_slice() {
            [Verb::Throw { velocity }] => {
                assert!(
                    (velocity.x - hand).abs() < hand * 0.25,
                    "thrown at {:.0}, where the hand moved at {hand:.0} points a second",
                    velocity.x
                );
                assert_eq!(velocity.y, 0.0, "and not downward, which gravity does");
            }
            other => panic!("expected a Throw, got {other:?}"),
        }
        assert!(!pointer.grabbing(), "and the hit-test comes back");
    }

    /// Letting go of a sprite you were not moving puts it down rather than
    /// flinging it. The Engine drops any Grab that stops, so this emits nothing.
    #[test]
    fn releasing_a_still_grab_drops_the_sprite_rather_than_throwing_it() {
        let mut pointer = Pointer::default();
        pointer.update(true, true, false, at(100.0, 100.0), TICK);
        let grabbed = (0..DRAG_DELAY_MS / TICK + 2)
            .map(|_| pointer.update(true, true, false, at(100.0, 100.0), TICK))
            .any(|verbs| verbs.contains(&Verb::Grab));
        assert!(grabbed);

        // Long enough still that the velocity window holds only stillness.
        for _ in 0..8 {
            pointer.update(true, true, false, at(100.0, 100.0), TICK);
        }
        assert_eq!(
            pointer.update(true, false, false, at(100.0, 100.0), TICK),
            vec![]
        );
        assert!(!pointer.grabbing());
    }

    /// Nobody holds a mouse perfectly still. A release after a pause is putting
    /// the sprite down, and one twitch in the last moments must not turn that
    /// into a fling.
    #[test]
    fn a_twitch_at_the_moment_of_release_is_not_a_throw() {
        let mut pointer = Pointer::default();
        pointer.update(true, true, false, at(100.0, 100.0), TICK);
        pointer.update(true, true, false, at(120.0, 100.0), TICK);
        assert!(pointer.grabbing());

        // Settled, then one point of jitter on the last tick before letting go.
        for _ in 0..8 {
            pointer.update(true, true, false, at(300.0, 300.0), TICK);
        }
        pointer.update(true, true, false, at(301.0, 300.0), TICK);

        assert_eq!(
            pointer.update(true, false, false, at(301.0, 300.0), TICK),
            vec![],
            "put down, not thrown"
        );
    }

    /// A click that begins somewhere else belongs to whatever is under it. The
    /// overlay passes those through, and the sprite must not claim them.
    #[test]
    fn a_press_that_did_not_begin_on_the_sprite_is_ignored() {
        let mut pointer = Pointer::default();

        assert_eq!(
            pointer.update(false, true, false, at(500.0, 500.0), TICK),
            vec![]
        );
        assert_eq!(
            pointer.update(true, true, false, at(100.0, 100.0), TICK),
            vec![],
            "dragging a window under the sprite does not pick the sprite up"
        );
        assert_eq!(
            pointer.update(true, false, false, at(100.0, 100.0), TICK),
            vec![]
        );
        assert!(!pointer.grabbing());
    }

    /// A Grab is reported every tick it is held, not only on the tick it began:
    /// the Engine reads it as a state, and a missing one is a release.
    #[test]
    fn a_held_grab_is_reported_on_every_tick() {
        let mut pointer = Pointer::default();
        pointer.update(true, true, false, at(100.0, 100.0), TICK);
        pointer.update(true, true, false, at(120.0, 100.0), TICK);

        for step in 0..5 {
            let x = 130.0 + f64::from(step) * 10.0;
            assert_eq!(
                pointer.update(false, true, false, at(x, 100.0), TICK),
                vec![Verb::Grab]
            );
        }
    }

    /// A right-click over the sprite is a Menu, emitted on the press edge.
    #[test]
    fn a_right_click_on_the_sprite_is_a_menu() {
        let mut pointer = Pointer::default();

        assert_eq!(
            pointer.update(true, false, true, at(100.0, 100.0), TICK),
            vec![Verb::Menu],
            "secondary button down over the sprite opens the menu"
        );
        assert!(!pointer.grabbing(), "and does not grab");
    }

    /// Menu is the press edge only; holding the right button does not retrigger it.
    #[test]
    fn holding_right_button_does_not_retrigger_menu() {
        let mut pointer = Pointer::default();

        assert_eq!(
            pointer.update(true, false, true, at(100.0, 100.0), TICK),
            vec![Verb::Menu],
            "first right button press emits Menu"
        );

        assert_eq!(
            pointer.update(true, false, true, at(100.0, 100.0), TICK),
            vec![],
            "held right button does not emit a second Menu"
        );

        assert_eq!(
            pointer.update(true, false, true, at(100.0, 100.0), TICK),
            vec![],
            "still no Menu on continued hold"
        );
    }

    /// Right-click elsewhere stays click-through, so the desktop's own context
    /// menu is still reachable.
    #[test]
    fn a_right_click_off_the_sprite_is_ignored() {
        let mut pointer = Pointer::default();

        assert_eq!(
            pointer.update(false, false, true, at(500.0, 500.0), TICK),
            vec![],
            "secondary button down elsewhere is not our Menu"
        );
    }

    /// Right-click during a drag is a Menu, and the drag continues. Both verbs
    /// are emitted in the same tick: Menu from the right button press, Grab
    /// from the ongoing left button hold.
    #[test]
    fn a_right_click_during_a_drag_is_a_menu() {
        let mut pointer = Pointer::default();

        pointer.update(true, true, false, at(100.0, 100.0), TICK);
        pointer.update(true, true, false, at(150.0, 100.0), TICK);
        assert!(pointer.grabbing(), "sprite is being dragged");

        let verbs = pointer.update(true, true, true, at(150.0, 100.0), TICK);
        assert_eq!(
            verbs,
            vec![Verb::Menu, Verb::Grab],
            "right-click emits Menu, held left button emits Grab"
        );
        assert!(pointer.grabbing(), "drag continues, Menu does not drop");
    }

    /// Nothing under the cursor is nobody's press, which is what leaves a click
    /// on the desktop to the desktop.
    #[test]
    fn a_press_over_no_sprite_belongs_to_nobody() {
        assert_eq!(press_target(&[false, false], None), None);
        assert_eq!(press_target(&[], None), None);
    }

    #[test]
    fn a_press_over_one_sprite_belongs_to_it() {
        assert_eq!(press_target(&[false, true, false], None), Some(1));
    }

    /// Two sprites overlap and only the top one is the one the user can see
    /// under the cursor. Drawn in roster order, so the last is the top.
    #[test]
    fn a_press_where_two_sprites_overlap_belongs_to_the_top_one() {
        assert_eq!(press_target(&[true, true], None), Some(1));
        assert_eq!(press_target(&[true, true, true], None), Some(2));
    }

    /// A drag that outruns the art leaves the cursor over no sprite at all, and
    /// the Instance being dragged must not change or be dropped for it, nor pass
    /// to whichever sprite the cursor crossed on the way.
    #[test]
    fn the_instance_being_held_keeps_the_press_wherever_the_cursor_goes() {
        assert_eq!(press_target(&[false, false], Some(0)), Some(0));
        assert_eq!(
            press_target(&[true, false], Some(1)),
            Some(1),
            "crossing another sprite does not hand it over"
        );
    }

    /// An Instance dismissed mid-drag leaves a holder that is no longer there.
    /// The press falls back to what is under the cursor rather than to an index
    /// that would panic or point at somebody else's Instance.
    #[test]
    fn a_holder_that_is_gone_gives_the_press_back_to_the_cursor() {
        assert_eq!(press_target(&[false, true], Some(5)), Some(1));
        assert_eq!(press_target(&[false, false], Some(5)), None);
    }
}
