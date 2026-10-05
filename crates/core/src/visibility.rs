//! When the Character gets out of the way. DESIGN.md decision 8 gives fidget
//! one window level and no restacking, so staying out of the user's way means
//! disappearing. A rule (fullscreen on every display) fades; the hotkey answers
//! at once. Fullscreen on only some displays moves the Character instead.
//!
//! Two conditions deliberately not rules: Do Not Disturb leaves the Character on
//! screen and only stops it starting things, which is the Director's. Screen
//! share is the window server's: the overlay is marked never-captured instead
//! (`platform::macos::overlay_panel`), since macOS cannot say when a share is on.

use crate::window_source::Rect;

/// How long a rule takes to take the Character away, and to give it back. Long
/// enough to read as leaving rather than a dropped frame, short enough not to
/// leave a ghost in a fullscreen app; 200ms read as a blink on a real desktop.
pub const FADE_MS: u32 = 500;

/// How far a window's edge may sit from a display's and still count as covering
/// it, in points. Under a fractional scale factor the two need not divide back
/// onto the same number; a point of slack still tells fullscreen from zoomed.
const EDGE_TOLERANCE: f64 = 1.0;

/// Thickness at or below which a window can be the Dock or the menu bar,
/// as a fraction of the display. A zoomed window is most of the display.
/// Same third as `plausible_dock`.
const STRIP_FRACTION: f64 = 0.3;

/// A Dock or menu bar spans most of the edge it sits on. A short window on that
/// edge is still the frontmost app; skipping it would treat the fullscreen
/// window behind it as frontmost and hide the Character.
const STRIP_SPAN: f64 = 0.5;

/// What the desktop says about whether the Character belongs on screen. A
/// platform that cannot see it reports no displays, the same answer as a
/// desktop where it is not happening: the Character stays.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Desktop {
    /// Whether a fullscreen application holds each display, at the same
    /// indexes as the display frames.
    pub fullscreen: Vec<bool>,
}

impl Desktop {
    /// The first display no fullscreen application holds: where a Character
    /// standing on a fullscreen one goes. `None` when every display is held.
    pub fn refuge(&self) -> Option<usize> {
        self.fullscreen.iter().position(|held| !held)
    }
}

/// Whether the Character is on screen, and what put it there. The two absences
/// differ: a rule hands the Character back on its own, so it leaves gently; the
/// hotkey was asked for, so it is obeyed at once and outlasts every rule.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Presence {
    #[default]
    Shown,
    Faded,
    Away,
}

impl Presence {
    fn visible(self) -> bool {
        matches!(self, Presence::Shown)
    }
}

/// One instruction for the overlay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Change {
    pub visible: bool,
    /// How long the change takes. Zero is at once.
    pub fade_ms: u32,
}

/// The hide rules, and the user's standing wish over them.
pub struct HideRules {
    /// The user asked for the Character to go away, by hotkey or by menu. Kept
    /// apart from what is on screen because it survives every rule: a fullscreen
    /// app that comes and goes must not hand back a Character its owner sent away.
    away: bool,
    /// Whether a fullscreen application moves the Character off its display,
    /// and off screen once every display has one. On by default; settings can
    /// turn the rule off without sending the Character away by hand.
    hide_in_fullscreen: bool,
    /// What the overlay must be. Asked every tick rather than announced when
    /// it changes, so a webview still loading its art when a rule fired is
    /// told the answer on the first frame it draws.
    presence: Presence,
    /// How long the move into that presence was given, so the answer above
    /// arrives late with the fade that produced it rather than a fresh one.
    fade_ms: u32,
}

impl Default for HideRules {
    fn default() -> Self {
        Self {
            away: false,
            hide_in_fullscreen: true,
            presence: Presence::default(),
            fade_ms: 0,
        }
    }
}

impl HideRules {
    /// Flip the user's wish. Takes effect on the next `update`, so a press and
    /// the desktop it lands on are decided together rather than racing.
    pub fn toggle(&mut self) {
        self.away = !self.away;
    }

    /// Restore a persisted Go-away. Same flag `toggle` flips, so the hotkey
    /// and a restart cannot disagree.
    pub fn set_away(&mut self, away: bool) {
        self.away = away;
    }

    pub fn is_away(&self) -> bool {
        self.away
    }

    /// Settings' hide-in-fullscreen toggle. Takes effect on the next `update`.
    pub fn set_hide_in_fullscreen(&mut self, enabled: bool) {
        self.hide_in_fullscreen = enabled;
    }

    pub fn hide_in_fullscreen(&self) -> bool {
        self.hide_in_fullscreen
    }

    /// What the overlay must do now, or `None` when nothing the user could see
    /// has changed. Silence is most of the answer: an overlay told to hide sixty
    /// times a second is the flicker decision 8 gave up restacking to avoid.
    pub fn update(&mut self, desktop: Desktop) -> Option<Change> {
        let was = self.presence;
        self.presence = if self.away {
            Presence::Away
        } else if self.hide_in_fullscreen
            && !desktop.fullscreen.is_empty()
            && desktop.refuge().is_none()
        {
            Presence::Faded
        } else {
            Presence::Shown
        };

        if was.visible() == self.presence.visible() {
            return None;
        }
        self.fade_ms = fade_ms(was, self.presence);
        Some(self.presence())
    }

    /// What the overlay must be right now, whether or not this tick changed it.
    /// The hit-test asks, since an unseen Character must not swallow a click, and
    /// so does every frame: a change announced once may be announced to nobody.
    pub fn presence(&self) -> Change {
        Change {
            visible: self.presence.visible(),
            fade_ms: self.fade_ms,
        }
    }
}

/// How long the move from one presence to another takes. The hotkey at either
/// end makes it instant. Everything else is a rule, and a rule fades both ways,
/// so a Character returning from a fullscreen application arrives the way it left.
fn fade_ms(from: Presence, to: Presence) -> u32 {
    if from == Presence::Away || to == Presence::Away {
        0
    } else {
        FADE_MS
    }
}

/// Whether the frontmost application window on each display covers it whole.
/// Whole, which separates fullscreen from zoomed: a zoomed window stops at the
/// menu bar and the Dock. The frames are whole display frames, not the usable ones.
pub fn fullscreen_displays(windows: &[Rect], frames: &[Rect]) -> Vec<bool> {
    frames
        .iter()
        .map(|frame| {
            windows
                .iter()
                // Skip the Dock and menu bar, which never cover a display: the
                // snapshot lists the Dock first, so taking it as frontmost would
                // miss a fullscreen app behind it.
                .find(|window| overlaps(window, frame) && !reserved_strip(window, frame))
                .is_some_and(|window| covers(window, frame))
        })
        .collect()
}

/// Whether any of `window` is on `frame` at all. While an application is
/// fullscreen, macOS keeps the hidden menu bar frontmost as a strip parked just
/// above its display, so what is off every display is not the window being worked in.
fn overlaps(window: &Rect, frame: &Rect) -> bool {
    window.x < frame.x + frame.width
        && window.x + window.width > frame.x
        && window.y < frame.y + frame.height
        && window.y + window.height > frame.y
}

/// The Dock and the menu bar. Skip them so a fullscreen window behind them
/// still counts.
fn reserved_strip(window: &Rect, frame: &Rect) -> bool {
    let hugs_top = (window.y - frame.y).abs() <= EDGE_TOLERANCE;
    let hugs_bottom =
        ((window.y + window.height) - (frame.y + frame.height)).abs() <= EDGE_TOLERANCE;
    let hugs_left = (window.x - frame.x).abs() <= EDGE_TOLERANCE;
    let hugs_right = ((window.x + window.width) - (frame.x + frame.width)).abs() <= EDGE_TOLERANCE;

    let thin_h = window.height > 0.0 && window.height <= frame.height * STRIP_FRACTION;
    let thin_w = window.width > 0.0 && window.width <= frame.width * STRIP_FRACTION;
    let spans_h = window.width >= frame.width * STRIP_SPAN;
    let spans_v = window.height >= frame.height * STRIP_SPAN;

    (thin_h && spans_h && (hugs_top || hugs_bottom))
        || (thin_w && spans_v && (hugs_left || hugs_right))
}

/// Whether a window reaches every edge of a display, give or take the slack a
/// fractional scale factor leaves behind.
fn covers(window: &Rect, frame: &Rect) -> bool {
    window.x <= frame.x + EDGE_TOLERANCE
        && window.y <= frame.y + EDGE_TOLERANCE
        && window.x + window.width >= frame.x + frame.width - EDGE_TOLERANCE
        && window.y + window.height >= frame.y + frame.height - EDGE_TOLERANCE
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window_source::WindowRect;

    fn fullscreen() -> Desktop {
        Desktop {
            fullscreen: vec![true],
        }
    }

    fn faded_out() -> Option<Change> {
        Some(Change {
            visible: false,
            fade_ms: FADE_MS,
        })
    }

    fn faded_in() -> Option<Change> {
        Some(Change {
            visible: true,
            fade_ms: FADE_MS,
        })
    }

    /// While an application is fullscreen macOS keeps the hidden menu bar in the
    /// window list, frontmost, as a strip just above its display. Asking only the
    /// frontmost window asked whether the menu bar is fullscreen, which it never is.
    #[test]
    fn the_hidden_menu_bar_does_not_answer_for_the_window_behind_it() {
        let displays = [
            Rect {
                x: 0.0,
                y: 0.0,
                width: 1920.0,
                height: 1080.0,
            },
            Rect {
                x: 1920.0,
                y: 0.0,
                width: 1728.0,
                height: 1117.0,
            },
        ];
        let menu_bar = window(0.0, -32.0, 1920.0, 32.0);
        let fullscreen = window(0.0, 0.0, 1920.0, 1080.0);

        assert_eq!(
            fullscreen_displays(&[menu_bar.bounds, fullscreen.bounds], &displays),
            [true, false],
            "the frontmost window that is anywhere on a display is the one being worked in"
        );
        // On the display, not above it. OnScreenOnly still reports this
        // strip, so skipping only off-display windows is not enough.
        let on_display = window(0.0, 0.0, 1920.0, 32.0);
        assert_eq!(
            fullscreen_displays(&[on_display.bounds, fullscreen.bounds], &displays),
            [true, false],
            "a menu bar on the display does not hide a fullscreen window behind it"
        );
        assert_eq!(
            fullscreen_displays(&[menu_bar.bounds], &displays),
            [false, false],
            "and a desktop holding nothing but the strip hides nothing"
        );

        // The menu bar is the one this was found on, but "off every display"
        // has four sides, and a window parked past any of them is equally not
        // the one being worked in.
        for parked in [
            window(-1920.0, 0.0, 1920.0, 1080.0),
            window(3648.0, 0.0, 1920.0, 1080.0),
            window(0.0, 1117.0, 1920.0, 1080.0),
        ] {
            assert_eq!(
                fullscreen_displays(&[parked.bounds, fullscreen.bounds], &displays),
                [true, false],
                "a window parked at {parked:?} is not on any display and answers for nothing"
            );
        }
    }

    /// The snapshot lists the Dock first. It never covers a display, so it
    /// must not hide a fullscreen window behind it.
    #[test]
    fn the_dock_does_not_answer_for_the_window_behind_it() {
        // Centered on the bottom edge, not stretched to the sides: the
        // rectangle CoreDock reports.
        let dock = window(234.0, 988.0, 1452.0, 92.0);
        let fullscreen = window(0.0, 0.0, 1920.0, 1080.0);

        assert_eq!(
            fullscreen_displays(&[dock.bounds, fullscreen.bounds], &[display()]),
            [true],
            "Dock in front of a fullscreen window is still fullscreen"
        );
        assert_eq!(
            fullscreen_displays(&[dock.bounds], &[display()]),
            [false],
            "the Dock alone is not fullscreen"
        );

        let side_dock = window(0.0, 200.0, 70.0, 680.0);
        assert_eq!(
            fullscreen_displays(&[side_dock.bounds, fullscreen.bounds], &[display()]),
            [true],
            "a left-edge Dock in front of a fullscreen window is still fullscreen"
        );
    }

    /// A short window on the bottom edge is still the frontmost app. If it were
    /// skipped as the Dock, the fullscreen window behind it would hide the Character.
    #[test]
    fn a_short_window_on_the_bottom_edge_is_still_the_one_being_worked_in() {
        let palette = window(200.0, 880.0, 400.0, 200.0);
        let fullscreen = window(0.0, 0.0, 1920.0, 1080.0);
        assert_eq!(
            fullscreen_displays(&[palette.bounds, fullscreen.bounds], &[display()]),
            [false],
            "a short window on the bottom edge is not the Dock"
        );
    }

    /// The desktop the Character spends almost all of its life on. Nothing to
    /// say means nothing said: an overlay told what it already is sixty times a
    /// second is the flicker decision 8 exists to avoid.
    #[test]
    fn a_quiet_desktop_leaves_the_character_on_screen_and_says_nothing() {
        let mut rules = HideRules::default();

        assert_eq!(rules.update(Desktop::default()), None);
        assert_eq!(rules.update(Desktop::default()), None);
        assert!(rules.presence().visible);
    }

    #[test]
    fn a_fullscreen_application_taking_the_front_fades_the_character_out() {
        let mut rules = HideRules::default();

        assert_eq!(rules.update(fullscreen()), faded_out());
        assert!(!rules.presence().visible);
        assert_eq!(rules.update(fullscreen()), None, "said once, not held");

        assert_eq!(rules.update(Desktop::default()), faded_in());
        assert!(rules.presence().visible);
    }

    /// The hotkey is somebody asking, so it is answered on the frame it lands
    /// on rather than faded through.
    #[test]
    fn the_hotkey_hides_and_shows_the_character_at_once() {
        let mut rules = HideRules::default();

        rules.toggle();
        assert_eq!(
            rules.update(Desktop::default()),
            Some(Change {
                visible: false,
                fade_ms: 0
            })
        );

        rules.toggle();
        assert_eq!(
            rules.update(Desktop::default()),
            Some(Change {
                visible: true,
                fade_ms: 0
            })
        );
    }

    /// The fullscreen rule is a setting, not a law. Off leaves the Character
    /// on a presentation the user still wants it in.
    #[test]
    fn fullscreen_hide_can_be_turned_off() {
        let mut rules = HideRules::default();
        assert!(rules.hide_in_fullscreen());

        rules.set_hide_in_fullscreen(false);
        assert_eq!(rules.update(fullscreen()), None);
        assert!(rules.presence().visible);

        rules.set_hide_in_fullscreen(true);
        assert_eq!(rules.update(fullscreen()), faded_out());
    }

    /// Go-away is the same flag across a restart, so a character sent away does
    /// not come back on its own.
    #[test]
    fn away_can_be_restored_from_settings() {
        let mut rules = HideRules::default();
        rules.set_away(true);
        assert!(rules.is_away());
        assert_eq!(
            rules.update(Desktop::default()),
            Some(Change {
                visible: false,
                fade_ms: 0
            })
        );
    }

    /// A Character the user sent away stays away. A fullscreen application
    /// arriving and leaving must not hand it back.
    #[test]
    fn a_character_sent_away_outlasts_every_rule_that_comes_and_goes() {
        let mut rules = HideRules::default();
        rules.toggle();
        rules.update(Desktop::default());

        assert_eq!(rules.update(fullscreen()), None, "already gone");
        assert_eq!(
            rules.update(Desktop::default()),
            None,
            "the rule lifting does not undo the hotkey"
        );
        assert!(!rules.presence().visible);

        rules.toggle();
        assert_eq!(
            rules.update(Desktop::default()),
            Some(Change {
                visible: true,
                fade_ms: 0
            })
        );
    }

    /// The other order: the hotkey asks for the Character back while a rule is
    /// still holding it. Nothing happens yet, and when the rule clears the
    /// Character fades in, because the rule is what it is returning from.
    #[test]
    fn asking_for_the_character_back_under_a_rule_waits_for_the_rule() {
        let mut rules = HideRules::default();
        rules.toggle();
        rules.update(fullscreen());

        rules.toggle();
        assert_eq!(
            rules.update(fullscreen()),
            None,
            "the rule still has it, so nothing on screen changed"
        );
        assert!(!rules.presence().visible);

        assert_eq!(rules.update(Desktop::default()), faded_in());
    }

    /// The hotkey's answer is instant whatever else changed alongside it: a
    /// fullscreen application quitting and the user asking for the Character
    /// back land in the same tick easily enough, and what the user did is press it.
    #[test]
    fn the_hotkey_answers_at_once_even_when_a_rule_lifts_with_it() {
        let mut rules = HideRules::default();
        rules.update(fullscreen());

        rules.toggle();
        assert_eq!(rules.update(fullscreen()), None, "hidden either way");

        rules.toggle();
        assert_eq!(
            rules.update(Desktop::default()),
            Some(Change {
                visible: true,
                fade_ms: 0
            })
        );
    }

    /// What the hit-test reads. A Character nobody can see must not swallow
    /// the click that lands where it would have been.
    #[test]
    fn a_hidden_character_is_not_there_to_be_clicked() {
        let mut rules = HideRules::default();
        assert!(rules.presence().visible);

        rules.update(fullscreen());
        assert!(!rules.presence().visible);

        rules.update(Desktop::default());
        assert!(rules.presence().visible);
    }

    /// What the renderer is told on every tick, not only when the answer changed:
    /// the first tick lands before the webview has loaded its art and started
    /// listening, and a Character hidden then would sit on the app that hid it.
    #[test]
    fn the_rules_still_say_the_character_is_gone_long_after_the_rule_fired() {
        let mut rules = HideRules::default();
        assert_eq!(
            rules.presence(),
            Change {
                visible: true,
                fade_ms: 0
            },
            "on screen at the start, and not faded there"
        );

        for _ in 0..4 {
            rules.update(fullscreen());
        }
        assert_eq!(
            rules.presence(),
            Change {
                visible: false,
                fade_ms: FADE_MS
            }
        );

        for _ in 0..4 {
            rules.update(Desktop::default());
        }
        assert_eq!(
            rules.presence(),
            Change {
                visible: true,
                fade_ms: FADE_MS
            }
        );
    }

    /// The standing answer carries the fade that produced it, so a Character
    /// put away by hotkey is still put away at once on the ticks that say
    /// nothing — the renderer must not fade in what a keypress banished.
    #[test]
    fn the_standing_answer_carries_the_fade_of_the_change_that_made_it() {
        let mut rules = HideRules::default();
        rules.toggle();
        rules.update(Desktop::default());
        rules.update(Desktop::default());

        assert_eq!(
            rules.presence(),
            Change {
                visible: false,
                fade_ms: 0
            }
        );
    }

    fn display() -> Rect {
        Rect {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        }
    }

    fn window(x: f64, y: f64, width: f64, height: f64) -> WindowRect {
        WindowRect {
            id: 1,
            bounds: Rect {
                x,
                y,
                width,
                height,
            },
            owner: None,
            title: None,
            layer: 0,
        }
    }

    fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    /// In the window list these cover both displays, so the Character fades
    /// and Come back cannot bring it back while that rule stays true.
    #[test]
    fn display_sized_overlays_hide_the_character_until_they_leave_the_list() {
        let displays = [
            rect(0.0, 0.0, 3440.0, 1440.0),
            rect(-1200.0, -209.0, 1200.0, 1920.0),
        ];
        assert_eq!(
            fullscreen_displays(&displays, &displays),
            [true, true],
            "each overlay covers the display it was built for"
        );

        let mut rules = HideRules::default();
        assert_eq!(
            rules.update(Desktop {
                fullscreen: vec![true, true],
            }),
            faded_out()
        );
        rules.toggle();
        rules.toggle();
        assert_eq!(
            rules.update(Desktop {
                fullscreen: vec![true, true],
            }),
            None,
            "come back does not outrank a fullscreen rule that is still true"
        );
        assert!(!rules.presence().visible);

        assert_eq!(fullscreen_displays(&[], &displays), [false, false]);
        assert_eq!(rules.update(Desktop::default()), faded_in());
        assert!(rules.presence().visible);
    }

    #[test]
    fn a_window_covering_its_whole_display_is_a_fullscreen_application() {
        assert_eq!(
            fullscreen_displays(&[rect(0.0, 0.0, 1920.0, 1080.0)], &[display()]),
            [true]
        );
    }

    /// The case that makes this worth computing. A zoomed window stops at the
    /// menu bar and the Dock, and the Character sits on its top edge as usual.
    #[test]
    fn a_zoomed_window_stops_at_the_menu_bar_and_is_not_fullscreen() {
        assert_eq!(
            fullscreen_displays(&[rect(0.0, 30.0, 1920.0, 952.0)], &[display()]),
            [false]
        );
    }

    #[test]
    fn a_fullscreen_window_that_is_not_frontmost_does_not_hide_the_character() {
        assert_eq!(
            fullscreen_displays(
                &[
                    rect(100.0, 100.0, 800.0, 600.0),
                    rect(0.0, 0.0, 1920.0, 1080.0),
                ],
                &[display()]
            ),
            [false]
        );
    }

    /// A display that does not begin at the origin, which is every display but
    /// the primary one. Both of its far edges are its origin plus its size, so
    /// a window matching only the size is short by the whole origin.
    fn second_display() -> Rect {
        Rect {
            x: 1920.0,
            y: 200.0,
            width: 1728.0,
            height: 1117.0,
        }
    }

    /// A second display is another whole screen an application can take. Taking
    /// it leaves the first display somewhere to stand, so nothing fades.
    #[test]
    fn a_fullscreen_window_on_a_second_display_takes_only_that_display() {
        let desktop = Desktop {
            fullscreen: fullscreen_displays(
                &[rect(1920.0, 200.0, 1728.0, 1117.0)],
                &[display(), second_display()],
            ),
        };
        assert_eq!(desktop.fullscreen, [false, true]);
        assert_eq!(desktop.refuge(), Some(0));

        let mut rules = HideRules::default();
        assert_eq!(
            rules.update(desktop),
            None,
            "the Character moves, not fades"
        );
        assert!(rules.presence().visible);
    }

    /// Each display answers for the frontmost window on it. A small window in
    /// front on one display says nothing about a fullscreen one on the other.
    #[test]
    fn each_display_answers_for_its_own_frontmost_window() {
        assert_eq!(
            fullscreen_displays(
                &[
                    rect(100.0, 100.0, 800.0, 600.0),
                    rect(1920.0, 200.0, 1728.0, 1117.0),
                ],
                &[display(), second_display()]
            ),
            [false, true]
        );
    }

    /// Nowhere left to stand is the only fullscreen that fades: one display, or
    /// every display taken. Leaving fullscreen brings the Character back.
    #[test]
    fn fullscreen_on_every_display_fades_the_character() {
        let both = Desktop {
            fullscreen: fullscreen_displays(
                &[
                    rect(0.0, 0.0, 1920.0, 1080.0),
                    rect(1920.0, 200.0, 1728.0, 1117.0),
                ],
                &[display(), second_display()],
            ),
        };
        assert_eq!(both.fullscreen, [true, true]);
        assert_eq!(both.refuge(), None);

        let mut rules = HideRules::default();
        assert_eq!(rules.update(both), faded_out());
        assert_eq!(
            rules.update(Desktop {
                fullscreen: vec![false, true],
            }),
            faded_in(),
            "one display free again is somewhere to stand"
        );
    }

    #[test]
    fn a_desktop_with_no_displays_has_no_refuge_and_does_not_fade() {
        let none = Desktop::default();
        assert_eq!(none.refuge(), None);
        assert_eq!(HideRules::default().update(none), None);
    }

    /// All four edges are measured against where the display starts, not merely
    /// how big it is: dropping the origin calls a quarter-width window on the
    /// second display fullscreen and takes the Character off both screens.
    #[test]
    fn a_window_short_of_any_one_edge_is_not_a_fullscreen_application() {
        let displays = [display(), second_display()];

        // Full height and out to the right edge, but starting a long way in.
        assert_eq!(
            fullscreen_displays(&[rect(200.0, 0.0, 1720.0, 1080.0)], &displays),
            [false, false]
        );
        // Down to the bottom edge, but starting below the menu bar — the
        // zoomed window above stops short of the bottom as well, so without
        // this one nothing measures the top edge at all.
        assert_eq!(
            fullscreen_displays(&[rect(0.0, 30.0, 1920.0, 1050.0)], &displays),
            [false, false]
        );
        // Full height, pinned to the left edge, and narrow.
        assert_eq!(
            fullscreen_displays(&[rect(0.0, 0.0, 400.0, 1080.0)], &displays),
            [false, false]
        );
        // Narrow on the second display: its right edge is past that display's
        // width, and nowhere near its right edge at 1920 + 1728.
        assert_eq!(
            fullscreen_displays(&[rect(1920.0, 200.0, 400.0, 1117.0)], &displays),
            [false, false]
        );
        // Short on the second display: past 1117 points from the top of the
        // desktop, and still short of its bottom edge at 200 + 1117.
        assert_eq!(
            fullscreen_displays(&[rect(1920.0, 200.0, 1728.0, 1000.0)], &displays),
            [false, false]
        );
    }

    #[test]
    fn a_desktop_with_no_windows_has_no_fullscreen_application() {
        assert_eq!(fullscreen_displays(&[], &[display()]), [false]);
        assert_eq!(
            fullscreen_displays(&[rect(0.0, 0.0, 1920.0, 1080.0)], &[]),
            Vec::<bool>::new()
        );
    }

    /// A fractional scale factor divides a window's edges and a display's into
    /// numbers that need not land on each other. Being a hair short is still
    /// fullscreen; being a menu bar short is not.
    #[test]
    fn edges_a_hair_apart_are_still_the_whole_display() {
        assert_eq!(
            fullscreen_displays(&[rect(0.3, 0.3, 1919.4, 1079.4)], &[display()]),
            [true]
        );
        assert_eq!(
            fullscreen_displays(&[rect(0.0, 0.0, 1920.0, 1077.0)], &[display()]),
            [false]
        );
    }
}
