//! Two-clock scheduler: 16ms timer when the sprite needs ticks, `recv` when it
//! does not. Hidden sleeps so XI2 cannot wake the loop. Visible Idle keeps
//! `recv` for hit-test and gesture. DND stays visible and quiet.

use std::time::{Duration, Instant};

use crate::engine::{Frame, State};

/// How the frame loop should wait for the next tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScheduleMode {
    /// Run the 16ms timer.
    Active,
    /// Block on `recv()` until an input event arrives.
    Idle,
}

/// Which clock the next tick uses.
/// A playing Behavior may animate while Grounded, so State alone cannot decide.
pub fn mode(frame: &Frame, visible: bool, behavior_playing: bool) -> ScheduleMode {
    if !visible {
        return ScheduleMode::Idle;
    }

    match frame.state {
        State::Falling | State::Dragged | State::Climbing => ScheduleMode::Active,
        State::Grounded | State::Perched => {
            if behavior_playing {
                ScheduleMode::Active
            } else {
                ScheduleMode::Idle
            }
        }
        State::Asleep => ScheduleMode::Idle,
    }
}

/// Whether the sprite changes place this tick. A walk keeps its velocity after
/// the Behavior that started it ends, so the Behavior name cannot say.
pub fn moving(frame: &Frame) -> bool {
    matches!(
        frame.state,
        State::Falling | State::Dragged | State::Climbing
    ) || frame.velocity.x != 0.0
        || frame.velocity.y != 0.0
        || frame.riding
}

/// When the next Active tick is due. Moving counts from the last deadline, so a
/// late sleep shortens the next wait; still counts from the wake, keeping a
/// perched sprite's slower rate. A deadline a tick behind the wake is stale.
pub fn next_tick(
    tick: Duration,
    last_deadline: Instant,
    woke: Instant,
    now: Instant,
    moving: bool,
) -> Instant {
    let from = if moving && last_deadline + tick > woke {
        last_deadline
    } else {
        woke
    };
    (from + tick).max(now)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grounded_frame() -> Frame {
        Frame {
            position: crate::engine::Point { x: 100.0, y: 100.0 },
            velocity: crate::engine::Point { x: 0.0, y: 0.0 },
            state: State::Grounded,
            animation: "idle",
            animation_ms: 0,
            variant_draw: 0,
            dialogue: None,
            behavior: None,
            playing_behavior: None,
            playing_primitive: None,
            riding: false,
            facing: 1.0,
            addressed: false,
            cue: None,
            contact: None,
            refused: None,
            charge: None,
        }
    }

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn a_moving_tick_takes_the_last_sleeps_lateness_off_its_wait() {
        let t0 = Instant::now();
        let (deadline, woke, now) = (t0 + ms(16), t0 + ms(20), t0 + ms(21));
        let next = next_tick(ms(16), deadline, woke, now, true);
        assert_eq!(next - now, ms(11), "woke 4 ms late, worked 1 ms");
    }

    #[test]
    fn a_still_tick_counts_its_period_from_when_it_woke() {
        let t0 = Instant::now();
        let (deadline, woke, now) = (t0 + ms(16), t0 + ms(20), t0 + ms(21));
        let next = next_tick(ms(16), deadline, woke, now, false);
        assert_eq!(next - now, ms(15), "no extra wakeups while perched");
    }

    #[test]
    fn an_overrun_tick_runs_at_once_and_drops_the_ticks_it_missed() {
        let t0 = Instant::now();
        let (deadline, woke, now) = (t0 + ms(16), t0 + ms(20), t0 + ms(40));
        let next = next_tick(ms(16), deadline, woke, now, true);
        assert_eq!(next - now, ms(0));
        let after = next_tick(ms(16), next, now, now, true);
        assert_eq!(after - now, ms(16), "paced from the late tick, not a burst");
    }

    #[test]
    fn a_moving_tick_after_an_idle_wait_counts_from_its_wake() {
        let t0 = Instant::now();
        let (deadline, woke, now) = (t0, t0 + ms(3000), t0 + ms(3001));
        let next = next_tick(ms(16), deadline, woke, now, true);
        assert_eq!(next - now, ms(15), "a stale deadline is not a tick owed");
    }

    #[test]
    fn a_walk_that_outlives_its_behavior_is_still_moving() {
        let mut frame = grounded_frame();
        assert!(!moving(&frame), "standing still");
        frame.playing_behavior = Some("talk".into());
        assert!(!moving(&frame), "talking in place");
        frame.playing_behavior = None;
        frame.velocity.x = 40.0;
        assert!(moving(&frame), "walking on its own velocity");
    }

    #[test]
    fn a_ride_or_a_fall_is_moving() {
        let mut frame = grounded_frame();
        frame.riding = true;
        assert!(moving(&frame));
        frame.riding = false;
        frame.state = State::Falling;
        assert!(moving(&frame));
    }

    #[test]
    fn falling_sprite_is_active() {
        let mut frame = grounded_frame();
        frame.state = State::Falling;
        assert_eq!(mode(&frame, true, false), ScheduleMode::Active);
    }

    #[test]
    fn dragged_sprite_is_active() {
        let mut frame = grounded_frame();
        frame.state = State::Dragged;
        assert_eq!(mode(&frame, true, false), ScheduleMode::Active);
    }

    #[test]
    fn climbing_sprite_is_active() {
        let mut frame = grounded_frame();
        frame.state = State::Climbing;
        assert_eq!(mode(&frame, true, false), ScheduleMode::Active);
    }

    #[test]
    fn grounded_sprite_playing_behavior_is_active() {
        let frame = grounded_frame();
        assert_eq!(
            mode(&frame, true, true),
            ScheduleMode::Active,
            "a behavior may walk, talk, or react while grounded"
        );
    }

    #[test]
    fn grounded_sprite_idle_is_idle() {
        let frame = grounded_frame();
        assert_eq!(
            mode(&frame, true, false),
            ScheduleMode::Idle,
            "state-based check: grounded with no behavior → Idle.
             frame_loop adds animation + sleep-accrual checks to stay Active."
        );
    }

    #[test]
    fn perched_sprite_idle_is_idle() {
        let mut frame = grounded_frame();
        frame.state = State::Perched;
        assert_eq!(
            mode(&frame, true, false),
            ScheduleMode::Idle,
            "state-based check: perched with no behavior → Idle.
             frame_loop adds animation + sleep-accrual checks to stay Active."
        );
    }

    #[test]
    fn asleep_sprite_is_idle() {
        let mut frame = grounded_frame();
        frame.state = State::Asleep;
        assert_eq!(mode(&frame, true, false), ScheduleMode::Idle);
    }

    #[test]
    fn hidden_sprite_is_idle() {
        let frame = grounded_frame();
        assert_eq!(
            mode(&frame, false, false),
            ScheduleMode::Idle,
            "hidden sprite never needs frequent ticks"
        );
    }

    #[test]
    fn hidden_while_falling_is_idle() {
        let mut frame = grounded_frame();
        frame.state = State::Falling;
        assert_eq!(
            mode(&frame, false, false),
            ScheduleMode::Idle,
            "hidden overrides active state — nobody is watching"
        );
    }
}
