//! Possession: turning key edges into what a possessed sprite does this tick.
//! The keyboard twin of `input::Pointer`, and pure for the same reason.

use std::collections::BTreeMap;

use crate::character::{Animation, REQUIRED_ANIMATIONS};

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

/// The Animations possession's controls already draw, each by the path that
/// draws it. A digit for one would replay what a key or the mouse is for,
/// except the names in [`DIGIT_PLAYABLE_ENGINE_ANIMATIONS`].
/// `engine` checks every driven State and Primitive lands in this list.
pub(crate) const DRIVEN_ANIMATIONS: [&str; 10] = [
    // The rest between keys: Grounded.
    "idle",
    // Left and Right: Walk, and Chase's art.
    "walk",
    // Space: Jump.
    "jump",
    // In the air: Falling.
    "fall",
    // The end of every fall: Land.
    "land",
    // Up at a wall: Climbing.
    "climb",
    // On a Perch: Perched.
    "sit",
    // Riding a moving Perch: Hold.
    "hold",
    // Held by the mouse: Dragged.
    "grab",
    // Taking control: Possessed.
    "possessed",
];

/// Engine Animations a digit may play. A key for one plays one pass of the
/// clip, not a Behavior that plays it, and not the Primitive a poke, a
/// Perch, or Dialogue already uses.
pub(crate) const DIGIT_PLAYABLE_ENGINE_ANIMATIONS: [&str; 4] = ["react", "sit", "sleep", "talk"];

/// Whether a digit may play `name`, by what the manifest declares. Not an
/// Animation the controls draw, nor a required slot the Engine plays, except
/// [`DIGIT_PLAYABLE_ENGINE_ANIMATIONS`], nor a drawn variant standing in for
/// its base, nor a left strip, nor a `same_as`, which is another name for a
/// clip with its own. An idle-gesture keeps its digit when a required slot
/// is the same as it. [`digit_animations`] then merges later names that share
/// a frame-path list with one already kept.
fn bindable(name: &str, animation: &Animation, animations: &BTreeMap<String, Animation>) -> bool {
    let engine_owned = (DRIVEN_ANIMATIONS.contains(&name) || REQUIRED_ANIMATIONS.contains(&name))
        && !DIGIT_PLAYABLE_ENGINE_ANIMATIONS.contains(&name);
    let drawn_variant = animation.variant_of.is_some() && !animation.is_idle_gesture();
    let left_strip = animations
        .values()
        .any(|base| base.left_strip.as_deref() == Some(name));
    !engine_owned && !drawn_variant && !left_strip && animation.same_as.is_none()
}

/// One digit on the possession card: the clip a press plays, and every
/// manifest name that shares those frames.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DigitAnimation<'a> {
    /// The Animation a press of this digit plays: the first name kept for
    /// these frames.
    pub play_name: &'a str,
    /// Manifest names merged onto this digit, in first-seen card order.
    /// Always includes [`Self::play_name`].
    pub merged_names: Vec<&'a str>,
}

impl DigitAnimation<'_> {
    /// The card row for this digit: the merged names joined with `/`.
    /// Display capitalization happens in the page, one name at a time.
    pub fn card_label(&self) -> String {
        self.merged_names.join("/")
    }
}

/// What digits 1 to 9 play: the Character's own Animations in name order,
/// minus the ones the Engine or the controls already draw, save `react`,
/// `sit`, `sleep` and `talk`. A later name on a kept frame-path list,
/// ignoring fps and loop, shares that digit: the label joins the names,
/// a press plays the first, and a later name still joins once nine are full.
pub fn digit_animations(animations: &BTreeMap<String, Animation>) -> Vec<DigitAnimation<'_>> {
    let mut digits: Vec<DigitAnimation<'_>> = Vec::new();
    let mut kept_frames: Vec<&[String]> = Vec::new();
    for (name, animation) in animations {
        if !bindable(name, animation, animations) {
            continue;
        }
        let frames = animation.frames.as_slice();
        if let Some(index) = kept_frames.iter().position(|kept| *kept == frames) {
            digits[index].merged_names.push(name.as_str());
            continue;
        }
        if digits.len() == 9 {
            continue;
        }
        kept_frames.push(frames);
        digits.push(DigitAnimation {
            play_name: name.as_str(),
            merged_names: vec![name.as_str()],
        });
    }
    digits
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
    /// Up was pressed since the last tick: take hold of the wall the sprite
    /// stands against.
    pub climb: bool,
    /// Up is down: a climber goes up its wall only while it is, as a walk
    /// moves only under a held arrow.
    pub up: bool,
    /// Down is down: a climber goes down its wall only while it is.
    pub down: bool,
    /// Down was pressed since the last tick: drop through the Perch the
    /// sprite stands on.
    pub drop: bool,
    /// Space is down, and the bar shows this.
    pub charging: Option<Charge>,
    /// Space came up since the last tick: jump with this charge.
    pub launch: Option<Charge>,
    /// Escape was pressed: end the possession.
    pub end_possession: bool,
    /// A digit pressed since the last tick, 1 to 9. The Engine plays one pass
    /// of the Animation at that place in [`digit_animations`], by `play_name`.
    pub play: Option<u8>,
}

/// Held keys, between key edges and ticks.
#[derive(Clone, Debug, Default)]
pub struct Possession {
    /// Held walk keys in press order. The latest walks, and letting it go hands
    /// the walk back to the other.
    walks: Vec<Walk>,
    /// Up and Down are down. Taking the wall and dropping ask once a press,
    /// through `climb` and `drop`, so a held key neither takes a wall again
    /// after leaving one nor drops through every Perch below.
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
            up: self.up,
            down: self.down,
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
        let held = keys.tick(FRAME_MS);
        assert!(!held.climb && !held.drop, "held or repeated: {held:?}");
        keys.key(Key::Up, false);
        keys.key(Key::Up, true);
        assert!(keys.tick(FRAME_MS).climb, "pressed again");
    }

    #[test]
    fn held_up_and_down_show_every_tick_until_released() {
        let mut keys = Possession::default();
        keys.key(Key::Up, true);
        assert!(keys.tick(FRAME_MS).up);
        let held = keys.tick(FRAME_MS);
        assert!(held.up && !held.down && !held.climb, "{held:?}");
        keys.key(Key::Up, false);
        keys.key(Key::Down, true);
        let down = keys.tick(FRAME_MS);
        assert!(down.down && !down.up, "{down:?}");
        keys.key(Key::Down, false);
        assert_eq!(keys.tick(FRAME_MS), Intent::default());
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

    /// A Character Package declaring the required nine plus `extra`, every
    /// optional Animation drawn on the frames the required ones already ship.
    fn character_with(extra: &str) -> crate::character::Character {
        character_without(&[], extra)
    }

    /// The same, leaving `framed_elsewhere` for `extra` to declare.
    fn character_without(framed_elsewhere: &[&str], extra: &str) -> crate::character::Character {
        use crate::character::tests::{declaring, load_manifest};
        use crate::character::REQUIRED_ANIMATIONS;

        let required: Vec<&str> = REQUIRED_ANIMATIONS
            .into_iter()
            .filter(|name| !framed_elsewhere.contains(name))
            .collect();
        load_manifest(&format!("{}{extra}", declaring(&required))).expect("package is valid")
    }

    fn card_labels(animations: &BTreeMap<String, crate::character::Animation>) -> Vec<String> {
        digit_animations(animations)
            .iter()
            .map(DigitAnimation::card_label)
            .collect()
    }

    /// A digit is "play this clip now", so the card names Animations, never
    /// the Behaviors that compose them, and a Behavior's `then` adds nothing.
    /// `react`, `sit`, `sleep` and `talk` are there as one pass of the clip,
    /// not as the `greet` or `nap` that plays them.
    #[test]
    fn digits_name_animations_not_behaviors() {
        let character = character_with(
            "[animations.wave]\nframes = [\"wave-0.png\"]\nvariant_of = \"idle-gesture\"\n\
             [behaviors.hand-wave]\nplay = [\"wave\"]\n\
             [behaviors.greet]\nplay = [\"react\", \"talk\"]\nthen = \"nap\"\n\
             [behaviors.nap]\nplay = [\"sit\", \"sleep\"]\n",
        );
        assert_eq!(
            card_labels(&character.animations),
            ["react", "sit", "sleep", "talk", "wave"]
        );
    }

    /// Hidden by what the manifest declares, never by comparing clips. A
    /// required slot is the Engine's to play, except `react`, `sit`, `sleep`
    /// and `talk`, which a digit may play as one pass. A `same_as` is still
    /// a second name for its owner, so `react` and `sit` stay off the card
    /// here, and the idle-gesture that owns the frames keeps its key. `talk`
    /// keeps a digit of its own; `flourish` still stands in for it.
    #[test]
    fn a_required_slot_or_same_as_gets_no_digit_and_its_owner_keeps_one() {
        let character = character_without(
            &["react", "sit"],
            "[animations.wave]\nframes = [\"wave-0.png\"]\nvariant_of = \"idle-gesture\"\n\
             [animations.react]\nsame_as = \"wave\"\n\
             [animations.pondering]\nframes = [\"sit-0.png\"]\nvariant_of = \"idle-gesture\"\n\
             [animations.sit]\nsame_as = \"pondering\"\n\
             [animations.hello]\nsame_as = \"wave\"\n\
             [animations.flourish]\nframes = [\"talk-0.png\"]\nvariant_of = \"talk\"\n",
        );
        assert_eq!(
            card_labels(&character.animations),
            ["pondering", "sleep", "talk", "wave"]
        );
    }

    /// `react`, `sit`, `sleep` and `talk` are Engine Animations a digit may
    /// play, so they keep a key. Other required slots stay off the card.
    #[test]
    fn react_sit_sleep_and_talk_get_a_digit() {
        let character = character_with("");
        let animations = &character.animations;
        for name in ["react", "sit", "sleep", "talk"] {
            assert!(bindable(name, &animations[name], animations), "{name}");
        }
        assert!(!bindable("hold", &animations["hold"], animations));
        assert_eq!(card_labels(animations), ["react", "sit", "sleep", "talk"]);
    }

    /// A `sleep` that is a second name for another clip is hidden by the
    /// `same_as` rule, like any other. `react`, `sit` and `talk` still keep
    /// a digit of their own.
    #[test]
    fn a_sleep_that_is_same_as_another_clip_gets_no_digit() {
        let character = character_without(
            &["sleep"],
            "[animations.doze]\nframes = [\"sleep-0.png\"]\nvariant_of = \"idle-gesture\"\n\
             [animations.sleep]\nsame_as = \"doze\"\n",
        );
        assert_eq!(
            card_labels(&character.animations),
            ["doze", "react", "sit", "talk"]
        );
    }

    /// The arrows, Space, falling, climbing, a Perch ride and the mouse
    /// already draw these, so a digit for one would replay a key. The idle
    /// is the rest between keys. A ring member stands in for its base, and a
    /// left strip is its base facing the other way. `react`, `sit`, `sleep`
    /// and `talk` each keep a digit: one pass of the clip. A Perch still
    /// draws `sit`. Ring members `perk-up` and `doze` still stand in for
    /// their bases.
    #[test]
    fn animations_the_controls_already_draw_get_no_digit_and_the_playable_engine_set_keeps_one() {
        let character = character_with(
            "[animations.climb]\nframes = [\"walk-0.png\"]\n\
             [animations.grab]\nframes = [\"fall-0.png\"]\n\
             [animations.jump]\nframes = [\"fall-0.png\"]\n\
             [animations.possessed]\nframes = [\"react-0.png\"]\n\
             [animations.stroll]\nframes = [\"walk-0.png\"]\nvariant_of = \"walk\"\n\
             [animations.walk-left]\nframes = [\"walk-0.png\"]\nleft_of = \"walk\"\n\
             [animations.perk-up]\nframes = [\"sit-0.png\"]\nvariant_of = \"sit\"\n\
             [animations.doze]\nframes = [\"sleep-0.png\"]\nvariant_of = \"sleep\"\n",
        );
        assert_eq!(
            card_labels(&character.animations),
            ["react", "sit", "sleep", "talk"]
        );
    }

    /// Eleven idle-gestures plus the playable required slots. Digits keep
    /// the first nine unique frame-path lists in name order. A later name
    /// on a kept list still joins that label after the cap is full: `b`,
    /// `c`, `d` and `e` reuse the playable required slots' files, so those
    /// four join. `j` and `k` have their own lists and stay off the card.
    #[test]
    fn only_the_first_nine_animations_get_digits() {
        let singles = [
            "wave-0.png",
            "talk-0.png",
            "sleep-0.png",
            "sit-0.png",
            "react-0.png",
            "land-0.png",
            "idle-0.png",
            "hold-0.png",
            "fall-0.png",
            "walk-0.png",
        ];
        let gestures: String = ["k", "j", "i", "h", "g", "f", "e", "d", "c", "b", "a"]
            .iter()
            .map(|name| {
                let index = usize::from(name.as_bytes()[0] - b'a');
                let frames = if let Some(file) = singles.get(index) {
                    format!("[\"{file}\"]")
                } else {
                    "[\"wave-0.png\", \"react-0.png\"]".to_string()
                };
                format!(
                    "[animations.{name}]\nframes = {frames}\n\
                     variant_of = \"idle-gesture\"\n"
                )
            })
            .collect();
        let character = character_with(&gestures);
        assert_eq!(
            card_labels(&character.animations),
            ["a", "b/talk", "c/sleep", "d/sit", "e/react", "f", "g", "h", "i"]
        );
    }

    /// Two Animations may share a frame-path list at different fps, and
    /// both stay bindable. They share one digit. The card joins their
    /// names, first-seen first, and a press plays the first name's clip.
    #[test]
    fn a_later_name_on_the_same_frames_joins_the_kept_digit_label() {
        let character = character_without(
            &["talk"],
            "[animations.talk]\nframes = [\"react-0.png\"]\nfps = 3\n",
        );
        let animations = &character.animations;
        assert!(bindable("react", &animations["react"], animations));
        assert!(bindable("talk", &animations["talk"], animations));
        assert_ne!(animations["react"].fps, animations["talk"].fps);
        assert_eq!(animations["react"].frames, animations["talk"].frames);
        let digits = digit_animations(animations);
        assert_eq!(card_labels(animations), ["react/talk", "sit", "sleep"]);
        assert_eq!(digits[0].play_name, "react");
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
