//! Possession: turning key edges into what a possessed sprite does this tick.
//! The keyboard twin of `input::Pointer`, and pure for the same reason.

use std::collections::BTreeMap;

use crate::character::{animation_of, Behavior, Primitive, Step};

/// Which way a possessed walk goes, from the Left or Right key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Walk {
    Left,
    Right,
}

/// A key possession hears. The Shell drops every other key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Walk(Walk),
    Up,
    Down,
    Space,
    Escape,
    /// 1 through 9. Letters stay unbound, so A and D do not collide with WASD.
    Digit(u8),
}

/// What digits 1 to 9 play: declared Behaviors by their own Primitives, not
/// `then`, minus movement-only ones, one digit per Animation loop, name order.
pub fn digit_behaviors(behaviors: &BTreeMap<String, Behavior>) -> Vec<Vec<&str>> {
    let mut groups: Vec<(Vec<&str>, Vec<&str>)> = Vec::new();
    for (name, behavior) in behaviors {
        let own = behavior.steps.as_slice();
        // Idle between moves is the pause in a stroll, which the arrows
        // already give. Idle alone is not movement. A gesture is not movement.
        let moves = own.iter().any(|step| {
            matches!(step, Step::Primitive(primitive) if primitive.owned_by_arrows_and_space())
        });
        let only_moves = moves
            && own.iter().all(|step| match step {
                Step::Primitive(primitive) => {
                    *primitive == Primitive::Idle || primitive.owned_by_arrows_and_space()
                }
                Step::Gesture(_) => false,
            });
        if behavior.not_in_manifest || only_moves {
            continue;
        }
        // Composable Primitives only fall back to art no other one draws, so
        // `animation_of` alone says what is drawn.
        let drawn: Vec<&str> = own
            .iter()
            .map(|step| match step {
                Step::Primitive(primitive) => animation_of(*primitive),
                Step::Gesture(name) => name.as_str(),
            })
            .collect();
        match groups.iter_mut().find(|(played, _)| *played == drawn) {
            Some((_, names)) => names.push(name),
            None => groups.push((drawn, vec![name])),
        }
    }
    groups.truncate(9);
    groups.into_iter().map(|(_, names)| names).collect()
}

/// How long Space takes to fill the bar, in milliseconds. Long enough to aim a
/// jump by feel, short enough that a full one does not stall the play.
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

/// What possession asks of the Engine this tick.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Intent {
    /// The side to walk towards, or `None` to stand.
    pub walk: Option<Walk>,
    /// Up was pressed since the last tick: climb the wall the sprite stands
    /// against.
    pub climb: bool,
    /// Down was pressed since the last tick: drop through the Perch the
    /// sprite stands on.
    pub drop: bool,
    /// Space is down, and the bar shows this.
    pub charging: Option<Charge>,
    /// Space came up since the last tick: jump with this charge.
    pub launch: Option<Charge>,
    /// Escape was pressed: end the possession.
    pub end_possession: bool,
    /// A digit pressed since the last tick, 1 to 9. The Engine plays the
    /// first Behavior at that place in [`digit_behaviors`], without its `then`.
    pub play: Option<u8>,
}

/// Held keys, between key edges and ticks.
#[derive(Clone, Debug, Default)]
pub struct Possession {
    /// Held walk keys in press order. The latest walks, and letting it go hands
    /// the walk back to the other.
    walks: Vec<Walk>,
    /// Up and Down are down. A press asks once, through `climb` and `drop`,
    /// so a held key neither climbs again after the let-go nor drops through
    /// every Perch below.
    up: bool,
    down: bool,
    climb: bool,
    drop: bool,
    /// How long Space has been down, or `None` while it is up.
    space_ms: Option<u32>,
    /// A Space keyup the next tick launches. Apart from `space_ms` so a press
    /// straight after a keyup, inside one tick, still launches the first.
    launch: Option<Charge>,
    end_possession: bool,
    /// Digits 1 to 9 currently down. A keydown for one already down is a repeat.
    digits: u16,
    /// The digit the next tick plays.
    play: Option<u8>,
}

impl Possession {
    /// One key edge, `down` for a press and not for a release. A keydown for a
    /// key already down is auto-repeat and changes nothing, bar Escape, whose
    /// repeat only asks again to end a possession that is already ending.
    pub fn key(&mut self, key: Key, down: bool) {
        match (key, down) {
            (Key::Walk(walk), true) if !self.walks.contains(&walk) => self.walks.push(walk),
            (Key::Walk(walk), false) => self.walks.retain(|held| *held != walk),
            (Key::Up, true) if !self.up => (self.up, self.climb) = (true, true),
            (Key::Up, false) => self.up = false,
            (Key::Down, true) if !self.down => (self.down, self.drop) = (true, true),
            (Key::Down, false) => self.down = false,
            (Key::Space, true) if self.space_ms.is_none() => self.space_ms = Some(0),
            (Key::Space, false) => {
                self.launch = self.space_ms.take().map(Charge::held).or(self.launch);
            }
            (Key::Escape, true) => {
                self.pause();
                self.end_possession = true;
            }
            (Key::Digit(digit), down) => self.digit(digit, down),
            _ => {}
        }
    }

    fn digit(&mut self, digit: u8, down: bool) {
        let Some(bit) = (1..=9).contains(&digit).then_some(1u16 << digit) else {
            return;
        };
        if down {
            if self.digits & bit == 0 {
                self.digits |= bit;
                self.play = Some(digit);
            }
        } else {
            self.digits &= !bit;
        }
    }

    /// The keyboard went to another app. Its keyups never arrive here, so every
    /// key counts as let go, and a jump would launch a sprite that should stand.
    pub fn pause(&mut self) {
        *self = Self {
            end_possession: self.end_possession,
            ..Self::default()
        };
    }

    /// What the held keys ask for this tick.
    pub fn tick(&mut self, elapsed_ms: u32) -> Intent {
        if let Some(ms) = &mut self.space_ms {
            *ms = ms.saturating_add(elapsed_ms);
        }
        Intent {
            walk: self.walks.last().copied(),
            climb: std::mem::take(&mut self.climb),
            drop: std::mem::take(&mut self.drop),
            charging: self.space_ms.map(Charge::held),
            launch: self.launch.take(),
            end_possession: std::mem::take(&mut self.end_possession),
            play: self.play.take(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME_MS: u32 = 16;

    #[test]
    fn a_held_arrow_walks_until_it_is_released() {
        let mut keys = Possession::default();
        keys.key(Key::Walk(Walk::Left), true);
        assert_eq!(keys.tick(FRAME_MS).walk, Some(Walk::Left));
        assert_eq!(keys.tick(FRAME_MS).walk, Some(Walk::Left));
        keys.key(Key::Walk(Walk::Left), false);
        assert_eq!(keys.tick(FRAME_MS).walk, None);
    }

    #[test]
    fn the_later_arrow_walks_and_letting_it_go_hands_back_to_the_other() {
        let mut keys = Possession::default();
        keys.key(Key::Walk(Walk::Left), true);
        keys.key(Key::Walk(Walk::Right), true);
        assert_eq!(keys.tick(FRAME_MS).walk, Some(Walk::Right));
        keys.key(Key::Walk(Walk::Right), false);
        assert_eq!(keys.tick(FRAME_MS).walk, Some(Walk::Left));
    }

    #[test]
    fn an_auto_repeat_keydown_does_not_steal_the_walk() {
        let mut keys = Possession::default();
        keys.key(Key::Walk(Walk::Left), true);
        keys.key(Key::Walk(Walk::Right), true);
        keys.key(Key::Walk(Walk::Left), true);
        assert_eq!(keys.tick(FRAME_MS).walk, Some(Walk::Right));
    }

    #[test]
    fn up_and_down_ask_once_per_press() {
        let mut keys = Possession::default();
        keys.key(Key::Up, true);
        keys.key(Key::Down, true);
        let pressed = keys.tick(FRAME_MS);
        assert!(pressed.climb && pressed.drop, "{pressed:?}");
        keys.key(Key::Up, true);
        keys.key(Key::Down, true);
        assert_eq!(keys.tick(FRAME_MS), Intent::default(), "held or repeated");
        keys.key(Key::Up, false);
        keys.key(Key::Up, true);
        assert!(keys.tick(FRAME_MS).climb, "pressed again");
    }

    #[test]
    fn a_pause_lets_go_of_up_and_down() {
        let mut keys = Possession::default();
        keys.key(Key::Up, true);
        keys.key(Key::Down, true);
        keys.pause();
        assert_eq!(keys.tick(FRAME_MS), Intent::default());
    }

    #[test]
    fn holding_space_fills_the_bar_over_a_second_and_letting_go_launches() {
        let mut keys = Possession::default();
        keys.key(Key::Space, true);
        assert_eq!(keys.tick(250).charging.map(Charge::fraction), Some(0.25));
        assert_eq!(keys.tick(250).charging.map(Charge::fraction), Some(0.5));
        assert_eq!(keys.tick(250).launch, None);
        keys.key(Key::Space, false);
        let launched = keys.tick(FRAME_MS);
        assert_eq!(launched.launch.map(Charge::fraction), Some(0.75));
        assert_eq!(launched.charging, None);
        assert_eq!(keys.tick(FRAME_MS).launch, None);
    }

    #[test]
    fn the_bar_stops_full_at_a_second() {
        let mut keys = Possession::default();
        keys.key(Key::Space, true);
        keys.tick(1_500);
        keys.key(Key::Space, false);
        assert_eq!(keys.tick(FRAME_MS).launch.map(Charge::fraction), Some(1.0));
    }

    #[test]
    fn a_tap_launches_with_an_empty_bar() {
        let mut keys = Possession::default();
        keys.key(Key::Space, true);
        keys.key(Key::Space, false);
        assert_eq!(keys.tick(FRAME_MS).launch.map(Charge::fraction), Some(0.0));
    }

    #[test]
    fn an_auto_repeat_space_keeps_charging() {
        let mut keys = Possession::default();
        keys.key(Key::Space, true);
        keys.tick(400);
        keys.key(Key::Space, true);
        assert_eq!(keys.tick(100).charging.map(Charge::fraction), Some(0.5));
    }

    #[test]
    fn the_jump_rises_from_a_hop_to_the_full_height_in_step_with_the_bar() {
        let launch = |held_ms| {
            let mut keys = Possession::default();
            keys.key(Key::Space, true);
            keys.tick(held_ms);
            keys.key(Key::Space, false);
            keys.tick(FRAME_MS).launch.unwrap().height(110.0, 910.0)
        };
        assert_eq!(launch(0), 110.0);
        assert_eq!(launch(250), 310.0);
        assert_eq!(launch(500), 510.0);
        assert_eq!(launch(1_000), 910.0);
    }

    #[test]
    fn a_pause_lets_go_of_every_key_without_jumping() {
        let mut keys = Possession::default();
        keys.key(Key::Walk(Walk::Left), true);
        keys.key(Key::Walk(Walk::Right), true);
        keys.key(Key::Space, true);
        keys.tick(500);
        keys.pause();
        assert_eq!(keys.tick(FRAME_MS), Intent::default());
        // Keyups that straggle in after the pause launch nothing.
        keys.key(Key::Space, false);
        keys.key(Key::Walk(Walk::Left), false);
        assert_eq!(keys.tick(FRAME_MS), Intent::default());
    }

    #[test]
    fn a_pause_drops_a_jump_not_yet_launched() {
        let mut keys = Possession::default();
        keys.key(Key::Space, true);
        keys.key(Key::Space, false);
        keys.pause();
        assert_eq!(keys.tick(FRAME_MS).launch, None);
    }

    #[test]
    fn escape_asks_once_to_end_possession_and_lets_go_of_every_key() {
        let mut keys = Possession::default();
        keys.key(Key::Walk(Walk::Right), true);
        keys.key(Key::Space, true);
        keys.tick(500);
        keys.key(Key::Escape, true);
        assert_eq!(
            keys.tick(FRAME_MS),
            Intent {
                end_possession: true,
                ..Intent::default()
            }
        );
        assert_eq!(keys.tick(FRAME_MS), Intent::default());
    }

    #[test]
    fn letting_go_of_escape_asks_nothing() {
        let mut keys = Possession::default();
        keys.key(Key::Escape, false);
        assert_eq!(keys.tick(FRAME_MS), Intent::default());
    }

    /// Escape hands the keyboard back, so the pause that follows can land
    /// before the next tick.
    #[test]
    fn a_pause_after_escape_still_ends_possession() {
        let mut keys = Possession::default();
        keys.key(Key::Escape, true);
        keys.pause();
        assert!(keys.tick(FRAME_MS).end_possession);
    }

    fn behaviors(table: &[(&str, &[Primitive], Option<&str>)]) -> BTreeMap<String, Behavior> {
        table
            .iter()
            .map(|(name, primitives, then)| {
                let behavior = Behavior {
                    steps: primitives.iter().copied().map(Step::Primitive).collect(),
                    then: then.map(str::to_string),
                    weight: 1,
                    trigger: None,
                    not_in_manifest: false,
                    idle_type: false,
                };
                (name.to_string(), behavior)
            })
            .collect()
    }

    /// A Behavior not in the manifest (weight 0, added by the loader) repeats
    /// art the Character already shows, so it gets no digit. Only what the
    /// package declares does.
    #[test]
    fn only_behaviors_the_package_declares_get_digits() {
        use crate::character::tests::{declaring, load_manifest};
        use crate::character::REQUIRED_ANIMATIONS;

        let manifest = format!(
            "{}[behaviors.nap]\nplay = [\"sit\", \"sleep\"]\n",
            declaring(&REQUIRED_ANIMATIONS)
        );
        let character = load_manifest(&manifest).expect("package is valid");
        assert!(
            character.behaviors.contains_key("react"),
            "loading adds react"
        );
        assert_eq!(digit_behaviors(&character.behaviors), [["nap"]]);
    }

    /// A package that declares the very shape loading would add still declared
    /// it, so it keeps its digit.
    #[test]
    fn a_declared_weightless_react_keeps_its_digit() {
        use crate::character::tests::{declaring, load_manifest};
        use crate::character::REQUIRED_ANIMATIONS;

        let manifest = format!(
            "{}[behaviors.react]\nplay = [\"react\"]\nweight = 0\n",
            declaring(&REQUIRED_ANIMATIONS)
        );
        let character = load_manifest(&manifest).expect("package is valid");
        assert_eq!(digit_behaviors(&character.behaviors), [["react"]]);
    }

    /// The arrows and Space already move the sprite, so a Behavior made only
    /// of movement, and the idle between moves, gets no digit. Its own Primitives
    /// decide, not its `then`.
    #[test]
    fn behaviors_made_only_of_movement_get_no_digit() {
        let table = behaviors(&[
            ("greet", &[Primitive::Talk], Some("stroll")),
            ("stroll", &[Primitive::Walk], None),
            ("hop", &[Primitive::Jump], None),
            ("play-with-cursor", &[Primitive::Chase], None),
            ("touch-down", &[Primitive::Land], None),
            ("ride", &[Primitive::Hold, Primitive::Walk], None),
            ("wander", &[Primitive::Walk, Primitive::Idle], None),
            ("pace", &[Primitive::Walk, Primitive::Talk], None),
            ("stare", &[Primitive::Idle], None),
        ]);
        assert_eq!(digit_behaviors(&table), [["greet"], ["pace"], ["stare"]]);
    }

    /// Behaviors whose own Primitives play the same Animations in the same
    /// order share one digit, named together.
    #[test]
    fn behaviors_playing_the_same_loop_share_a_digit() {
        let table = behaviors(&[
            ("wave", &[Primitive::React], None),
            ("startle", &[Primitive::React], None),
            ("greet", &[Primitive::React], Some("nap")),
            ("nap", &[Primitive::Sit, Primitive::Sleep], None),
            ("doze", &[Primitive::Sleep, Primitive::Sit], None),
        ]);
        assert_eq!(
            digit_behaviors(&table),
            [vec!["doze"], vec!["greet", "startle", "wave"], vec!["nap"]]
        );
    }

    /// Chase draws the walk Animation, so chasing while talking looks the same
    /// as walking while talking and gets the same digit.
    #[test]
    fn behaviors_drawing_the_same_animations_share_a_digit() {
        let table = behaviors(&[
            ("pace", &[Primitive::Walk, Primitive::Talk], None),
            ("pester", &[Primitive::Chase, Primitive::Talk], None),
        ]);
        assert_eq!(digit_behaviors(&table), [["pace", "pester"]]);
    }

    #[test]
    fn only_the_first_nine_behaviors_get_digits() {
        let names = ["k", "j", "i", "h", "g", "f", "e", "d", "c", "b", "a"];
        let loops: Vec<Vec<Primitive>> = (1..=names.len())
            .map(|sits| vec![Primitive::Sit; sits])
            .collect();
        let table: Vec<_> = names
            .iter()
            .zip(&loops)
            .map(|(name, sits)| (*name, &sits[..], None))
            .collect();
        assert_eq!(
            digit_behaviors(&behaviors(&table)),
            [
                ["a"],
                ["b"],
                ["c"],
                ["d"],
                ["e"],
                ["f"],
                ["g"],
                ["h"],
                ["i"]
            ]
        );
    }

    #[test]
    fn a_digit_press_asks_once_to_play_and_a_repeat_does_not() {
        let mut keys = Possession::default();
        keys.key(Key::Digit(3), true);
        assert_eq!(keys.tick(FRAME_MS).play, Some(3));
        keys.key(Key::Digit(3), true);
        assert_eq!(keys.tick(FRAME_MS).play, None);
        keys.key(Key::Digit(3), false);
        keys.key(Key::Digit(3), true);
        assert_eq!(keys.tick(FRAME_MS).play, Some(3));
    }

    #[test]
    fn a_digit_outside_1_to_9_asks_for_nothing() {
        let mut keys = Possession::default();
        keys.key(Key::Digit(0), true);
        keys.key(Key::Digit(10), true);
        assert_eq!(keys.tick(FRAME_MS).play, None);
    }

    #[test]
    fn a_pause_drops_a_digit_not_yet_played() {
        let mut keys = Possession::default();
        keys.key(Key::Digit(1), true);
        keys.pause();
        assert_eq!(keys.tick(FRAME_MS).play, None);
    }
}
