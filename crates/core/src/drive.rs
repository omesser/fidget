//! Manual drive: turning key edges into what a possessed sprite does this tick.
//! The keyboard twin of `input::Pointer`, and pure for the same reason.

/// A walk key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arrow {
    Left,
    Right,
}

/// A key drive listens to. The Shell drops every other key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Arrow(Arrow),
    Space,
    Escape,
}

/// How long Space takes to fill the bar, in milliseconds.
const FULL_CHARGE_MS: u32 = 1_000;

/// How full the jump bar is, from 0 for a tap to 1 for a second's hold.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Charge(f64);

impl Charge {
    fn held(ms: u32) -> Self {
        Self((f64::from(ms) / f64::from(FULL_CHARGE_MS)).min(1.0))
    }

    /// The bar's fill, from 0 to 1.
    pub fn fraction(self) -> f64 {
        self.0
    }

    /// How high this charge lifts the feet: a plain hop for a tap, rising in
    /// step with the bar to `full` for a full one. Both are the Engine's, which
    /// knows the hop and the display under the feet.
    pub fn height(self, hop: f64, full: f64) -> f64 {
        hop + (full - hop) * self.0
    }
}

/// What drive asks of the Engine this tick.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Intent {
    /// The arrow to walk towards, or `None` to stand.
    pub walk: Option<Arrow>,
    /// Space is down, and the bar shows this.
    pub charging: Option<Charge>,
    /// Space came up since the last tick: jump with this charge.
    pub launch: Option<Charge>,
    /// Escape was pressed: end the possession.
    pub release: bool,
}

/// Held keys, between key edges and ticks.
#[derive(Clone, Debug, Default)]
pub struct Drive {
    /// Held arrows in press order. The latest walks, and letting it go hands
    /// the walk back to the other.
    arrows: Vec<Arrow>,
    /// How long Space has been down, or `None` while it is up.
    space_ms: Option<u32>,
    /// A Space keyup the next tick launches. Apart from `space_ms` so a press
    /// straight after a keyup, inside one tick, still launches the first.
    launch: Option<Charge>,
    release: bool,
}

impl Drive {
    /// One key edge, `down` for a press and not for a release. A keydown for a
    /// key already down is auto-repeat, and changes nothing.
    pub fn key(&mut self, key: Key, down: bool) {
        match (key, down) {
            (Key::Arrow(arrow), true) if !self.arrows.contains(&arrow) => self.arrows.push(arrow),
            (Key::Arrow(arrow), false) => self.arrows.retain(|held| *held != arrow),
            (Key::Space, true) if self.space_ms.is_none() => self.space_ms = Some(0),
            (Key::Space, false) => {
                self.launch = self.space_ms.take().map(Charge::held).or(self.launch);
            }
            (Key::Escape, true) => {
                self.pause();
                self.release = true;
            }
            _ => {}
        }
    }

    /// The keyboard went to another app. Its keyups never arrive here, so every
    /// key counts as let go, and a jump would launch a sprite that should stand.
    pub fn pause(&mut self) {
        *self = Self {
            release: self.release,
            ..Self::default()
        };
    }

    /// What the held keys ask for this tick.
    pub fn tick(&mut self, elapsed_ms: u32) -> Intent {
        if let Some(ms) = &mut self.space_ms {
            *ms = ms.saturating_add(elapsed_ms);
        }
        Intent {
            walk: self.arrows.last().copied(),
            charging: self.space_ms.map(Charge::held),
            launch: self.launch.take(),
            release: std::mem::take(&mut self.release),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME_MS: u32 = 16;

    #[test]
    fn a_held_arrow_walks_until_it_is_released() {
        let mut drive = Drive::default();
        drive.key(Key::Arrow(Arrow::Left), true);
        assert_eq!(drive.tick(FRAME_MS).walk, Some(Arrow::Left));
        assert_eq!(drive.tick(FRAME_MS).walk, Some(Arrow::Left));
        drive.key(Key::Arrow(Arrow::Left), false);
        assert_eq!(drive.tick(FRAME_MS).walk, None);
    }

    #[test]
    fn the_later_arrow_walks_and_letting_it_go_hands_back_to_the_other() {
        let mut drive = Drive::default();
        drive.key(Key::Arrow(Arrow::Left), true);
        drive.key(Key::Arrow(Arrow::Right), true);
        assert_eq!(drive.tick(FRAME_MS).walk, Some(Arrow::Right));
        drive.key(Key::Arrow(Arrow::Right), false);
        assert_eq!(drive.tick(FRAME_MS).walk, Some(Arrow::Left));
    }

    #[test]
    fn an_auto_repeat_keydown_does_not_steal_the_walk() {
        let mut drive = Drive::default();
        drive.key(Key::Arrow(Arrow::Left), true);
        drive.key(Key::Arrow(Arrow::Right), true);
        drive.key(Key::Arrow(Arrow::Left), true);
        assert_eq!(drive.tick(FRAME_MS).walk, Some(Arrow::Right));
    }

    #[test]
    fn holding_space_fills_the_bar_over_a_second_and_letting_go_launches() {
        let mut drive = Drive::default();
        drive.key(Key::Space, true);
        assert_eq!(drive.tick(250).charging.map(Charge::fraction), Some(0.25));
        assert_eq!(drive.tick(250).charging.map(Charge::fraction), Some(0.5));
        assert_eq!(drive.tick(250).launch, None);
        drive.key(Key::Space, false);
        let launched = drive.tick(FRAME_MS);
        assert_eq!(launched.launch.map(Charge::fraction), Some(0.75));
        assert_eq!(launched.charging, None);
        assert_eq!(drive.tick(FRAME_MS).launch, None);
    }

    #[test]
    fn the_bar_stops_full_at_a_second() {
        let mut drive = Drive::default();
        drive.key(Key::Space, true);
        drive.tick(1_500);
        drive.key(Key::Space, false);
        assert_eq!(drive.tick(FRAME_MS).launch.map(Charge::fraction), Some(1.0));
    }

    #[test]
    fn a_tap_launches_with_an_empty_bar() {
        let mut drive = Drive::default();
        drive.key(Key::Space, true);
        drive.key(Key::Space, false);
        assert_eq!(drive.tick(FRAME_MS).launch.map(Charge::fraction), Some(0.0));
    }

    #[test]
    fn an_auto_repeat_space_keeps_charging() {
        let mut drive = Drive::default();
        drive.key(Key::Space, true);
        drive.tick(400);
        drive.key(Key::Space, true);
        assert_eq!(drive.tick(100).charging.map(Charge::fraction), Some(0.5));
    }

    #[test]
    fn the_jump_rises_from_a_hop_to_the_full_height_in_step_with_the_bar() {
        let launch = |held_ms| {
            let mut drive = Drive::default();
            drive.key(Key::Space, true);
            drive.tick(held_ms);
            drive.key(Key::Space, false);
            drive.tick(FRAME_MS).launch.unwrap().height(110.0, 910.0)
        };
        assert_eq!(launch(0), 110.0);
        assert_eq!(launch(250), 310.0);
        assert_eq!(launch(500), 510.0);
        assert_eq!(launch(1_000), 910.0);
    }

    #[test]
    fn a_pause_lets_go_of_every_key_without_jumping() {
        let mut drive = Drive::default();
        drive.key(Key::Arrow(Arrow::Left), true);
        drive.key(Key::Arrow(Arrow::Right), true);
        drive.key(Key::Space, true);
        drive.tick(500);
        drive.pause();
        assert_eq!(drive.tick(FRAME_MS), Intent::default());
        // Keyups that straggle in after the pause launch nothing.
        drive.key(Key::Space, false);
        drive.key(Key::Arrow(Arrow::Left), false);
        assert_eq!(drive.tick(FRAME_MS), Intent::default());
    }

    #[test]
    fn a_pause_drops_a_jump_not_yet_launched() {
        let mut drive = Drive::default();
        drive.key(Key::Space, true);
        drive.key(Key::Space, false);
        drive.pause();
        assert_eq!(drive.tick(FRAME_MS).launch, None);
    }

    #[test]
    fn escape_asks_once_to_be_released_and_lets_go_of_every_key() {
        let mut drive = Drive::default();
        drive.key(Key::Arrow(Arrow::Right), true);
        drive.key(Key::Space, true);
        drive.tick(500);
        drive.key(Key::Escape, true);
        assert_eq!(
            drive.tick(FRAME_MS),
            Intent {
                release: true,
                ..Intent::default()
            }
        );
        assert_eq!(drive.tick(FRAME_MS), Intent::default());
    }

    #[test]
    fn letting_go_of_escape_asks_nothing() {
        let mut drive = Drive::default();
        drive.key(Key::Escape, false);
        assert_eq!(drive.tick(FRAME_MS), Intent::default());
    }

    /// Escape hands the keyboard back, so the pause that follows can land
    /// before the next tick.
    #[test]
    fn a_pause_after_escape_still_releases() {
        let mut drive = Drive::default();
        drive.key(Key::Escape, true);
        drive.pause();
        assert!(drive.tick(FRAME_MS).release);
    }
}
