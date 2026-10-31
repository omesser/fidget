//! Which overlay windows hold the keyboard. A quick message pill does, and so
//! does a possession card: closing the pill while the card is up must not hand
//! the keyboard back.

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

/// Open pills by overlay window label, each a set of Instance ids, plus the
/// windows whose possession card is holding the keyboard.
#[derive(Default)]
pub struct OpenPills {
    pills: HashMap<String, HashSet<String>>,
    drive: HashSet<String>,
}

impl OpenPills {
    /// Record `instance`'s pill on `window` as open or closed. Returns each
    /// window whose "keyboard held" state flipped, with its new state.
    pub fn note(&mut self, window: &str, instance: &str, open: bool) -> Vec<(String, bool)> {
        let before = self.on();
        if open {
            // One pill per Instance: opening here closes it on any other overlay.
            for (label, pills) in &mut self.pills {
                if label != window {
                    pills.remove(instance);
                }
            }
            self.pills
                .entry(window.to_string())
                .or_default()
                .insert(instance.to_string());
        } else if let Some(pills) = self.pills.get_mut(window) {
            pills.remove(instance);
        }
        self.flips(&before)
    }

    /// The possession card on `window` is holding the keyboard, or it is not.
    /// Same flip report as a pill, so a card and a pill share one hand-back.
    pub fn note_drive(&mut self, window: &str, holding: bool) -> Vec<(String, bool)> {
        let before = self.on();
        if holding {
            self.drive.insert(window.to_string());
        } else {
            self.drive.remove(window);
        }
        self.flips(&before)
    }

    fn flips(&self, before: &HashSet<String>) -> Vec<(String, bool)> {
        let after = self.on();
        let mut flips: Vec<(String, bool)> = before
            .difference(&after)
            .map(|label| (label.clone(), false))
            .chain(after.difference(before).map(|label| (label.clone(), true)))
            .collect();
        flips.sort();
        flips
    }

    fn on(&self) -> HashSet<String> {
        let mut labels: HashSet<String> = self
            .pills
            .iter()
            .filter(|(_, pills)| !pills.is_empty())
            .map(|(label, _)| label.clone())
            .collect();
        labels.extend(self.drive.iter().cloned());
        labels
    }

    /// Whether any overlay in this process holds the keyboard. One remembered
    /// app serves every overlay, so the last holder on any display is the one
    /// that hands the keyboard back.
    pub fn any_open(&self) -> bool {
        self.pills.values().any(|pills| !pills.is_empty()) || !self.drive.is_empty()
    }
}

/// What to do with the keyboard when an overlay's last pill closes.
#[derive(Debug, PartialEq, Eq)]
pub enum HandBack {
    /// The user already moved on: leave the keyboard where it is.
    Stay,
    /// Give it back to the app or window in front when the pill took it.
    Restore,
    /// That app or window is gone: step Fidget aside for whatever is next.
    StepAside,
}

/// `holding`: Fidget's overlay still has the keyboard. `previous_alive`: the
/// app or window remembered at focus time still exists. Same on every OS.
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub fn hand_back(holding: bool, previous_alive: bool) -> HandBack {
    match (holding, previous_alive) {
        (false, _) => HandBack::Stay,
        (true, true) => HandBack::Restore,
        (true, false) => HandBack::StepAside,
    }
}

/// What was in front when a pill took the keyboard. Fidget's own Chat is a
/// window number, not a stand-in pid.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RememberedFront {
    None,
    OtherApp(i32),
    OwnWindow(isize),
}

/// Where the keyboard goes when the last pill closes. A `Stay` keeps what
/// was remembered for the next close.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandBackPlan {
    Stay(RememberedFront),
    RestoreApp(i32),
    RestoreWindow(isize),
    StepAside,
}

/// `pid` is another app, or 0. `window` is Fidget's own key window, or 0.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn remembered_front(pid: i32, window: isize) -> RememberedFront {
    if pid > 0 {
        RememberedFront::OtherApp(pid)
    } else if window != 0 {
        RememberedFront::OwnWindow(window)
    } else {
        RememberedFront::None
    }
}

/// `holding`: the overlay still has the keyboard. `target_alive`: the
/// remembered app or window still exists.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn plan_hand_back(
    holding: bool,
    remembered: RememberedFront,
    target_alive: bool,
) -> HandBackPlan {
    if !holding {
        return HandBackPlan::Stay(remembered);
    }
    match remembered {
        RememberedFront::OtherApp(pid) if target_alive => HandBackPlan::RestoreApp(pid),
        RememberedFront::OwnWindow(number) if target_alive => HandBackPlan::RestoreWindow(number),
        RememberedFront::None | RememberedFront::OtherApp(_) | RememberedFront::OwnWindow(_) => {
            HandBackPlan::StepAside
        }
    }
}

/// What a close leaves remembered. A Stay keeps its payload. Every hand-back
/// spends it, so the next close cannot restore a window this one already used.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn remembered_after(plan: HandBackPlan) -> Option<RememberedFront> {
    match plan {
        HandBackPlan::Stay(remembered) => Some(remembered),
        HandBackPlan::RestoreApp(_) | HandBackPlan::RestoreWindow(_) | HandBackPlan::StepAside => {
            None
        }
    }
}

/// A real window that is not one of these overlays. Chat and Settings are
/// not overlays, so a close can return the keyboard to them. The other
/// display's overlay is in front when a pill moves across.
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub fn worth_remembering(foreground: u64, overlays: &[u64]) -> bool {
    foreground != 0 && !overlays.contains(&foreground)
}

/// Whether the overlay `overlay` is the foreground window `foreground`
/// (Win32 handles as integers; 0 is none).
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn holds_foreground(foreground: isize, overlay: isize) -> bool {
    foreground != 0 && foreground == overlay
}

/// One on-screen window, as macOS lists them front to back.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub struct AppWindow {
    pub pid: i32,
    /// The window server layer: 0 is ordinary app windows.
    pub layer: i64,
}

/// Where a macOS step aside hands activation: the app owning the frontmost
/// ordinary window that is not Fidget's. `regular` (a Dock icon) is asked
/// only until the first match.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn next_app_in_front(
    windows: &[AppWindow],
    own: i32,
    regular: impl Fn(i32) -> bool,
) -> Option<i32> {
    windows
        .iter()
        .find(|w| w.pid != own && w.layer == 0 && regular(w.pid))
        .map(|w| w.pid)
}

/// The process-wide record the overlay commands share.
pub fn shared() -> &'static Mutex<OpenPills> {
    static PILLS: OnceLock<Mutex<OpenPills>> = OnceLock::new();
    PILLS.get_or_init(Mutex::default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_overlay_holds_the_foreground_only_when_it_is_the_window_in_front() {
        assert!(holds_foreground(0x40ec4, 0x40ec4));
        assert!(!holds_foreground(0x150e64, 0x40ec4), "Notepad in front");
        assert!(!holds_foreground(0, 0), "no foreground window is not ours");
    }

    #[test]
    fn the_overlay_or_no_window_is_never_remembered() {
        let overlays = &[0x20, 0x21];
        assert!(worth_remembering(0x10, overlays), "another app");
        assert!(worth_remembering(0x30, overlays), "Fidget's own Chat");
        assert!(!worth_remembering(0x20, overlays), "the overlay that asked");
        assert!(
            !worth_remembering(0x21, overlays),
            "the other display's overlay"
        );
        assert!(!worth_remembering(0, overlays), "no foreground window");
    }

    #[test]
    fn a_step_aside_on_macos_picks_the_frontmost_ordinary_app_not_ours() {
        let w = |pid, layer| AppWindow { pid, layer };
        let windows = [w(7, 0), w(9, 25), w(5, 0), w(3, 0), w(4, 0)];
        let asked = std::cell::RefCell::new(Vec::new());
        let regular = |pid| {
            asked.borrow_mut().push(pid);
            pid != 5
        };
        assert_eq!(next_app_in_front(&windows, 7, regular), Some(3));
        assert_eq!(*asked.borrow(), [5, 3], "stops at the first match");
        assert_eq!(next_app_in_front(&windows[..3], 7, |pid| pid != 5), None);
    }

    #[test]
    fn the_keyboard_goes_back_only_while_fidget_still_holds_it() {
        assert_eq!(hand_back(false, true), HandBack::Stay);
        assert_eq!(hand_back(false, false), HandBack::Stay);
        assert_eq!(hand_back(true, true), HandBack::Restore);
        assert_eq!(hand_back(true, false), HandBack::StepAside);
    }

    #[test]
    fn a_window_number_with_no_pid_is_fidgets_own_window() {
        assert_eq!(remembered_front(0, 680), RememberedFront::OwnWindow(680));
        assert_eq!(remembered_front(42, 0), RememberedFront::OtherApp(42));
        assert_eq!(remembered_front(0, 0), RememberedFront::None);
        assert_eq!(
            remembered_front(-1, 680),
            RememberedFront::OwnWindow(680),
            "a negative pid is not an app"
        );
    }

    #[test]
    fn a_hand_back_over_chat_goes_to_that_window() {
        assert_eq!(
            plan_hand_back(true, RememberedFront::OwnWindow(680), true),
            HandBackPlan::RestoreWindow(680)
        );
    }

    #[test]
    fn a_stay_keeps_the_app_and_the_window() {
        let app = plan_hand_back(false, RememberedFront::OtherApp(42), true);
        let window = plan_hand_back(false, RememberedFront::OwnWindow(680), true);
        assert_eq!(remembered_after(app), Some(RememberedFront::OtherApp(42)));
        assert_eq!(
            remembered_after(window),
            Some(RememberedFront::OwnWindow(680))
        );
    }

    #[test]
    fn a_hand_back_spends_what_was_remembered() {
        assert_eq!(
            remembered_after(plan_hand_back(true, RememberedFront::OwnWindow(680), true)),
            None
        );
        assert_eq!(
            remembered_after(plan_hand_back(true, RememberedFront::OtherApp(42), true)),
            None
        );
        assert_eq!(
            remembered_after(plan_hand_back(true, RememberedFront::OwnWindow(680), false)),
            None
        );
    }

    #[test]
    fn a_gone_window_steps_aside() {
        assert_eq!(
            plan_hand_back(true, RememberedFront::OwnWindow(680), false),
            HandBackPlan::StepAside
        );
        assert_eq!(
            plan_hand_back(true, RememberedFront::OtherApp(42), false),
            HandBackPlan::StepAside
        );
        assert_eq!(
            plan_hand_back(true, RememberedFront::None, false),
            HandBackPlan::StepAside
        );
    }

    fn flips(pairs: &[(&str, bool)]) -> Vec<(String, bool)> {
        pairs.iter().map(|(w, o)| (w.to_string(), *o)).collect()
    }

    #[test]
    fn one_pill_turns_its_overlay_on_and_off() {
        let mut pills = OpenPills::default();
        assert_eq!(
            pills.note("overlay-0", "a", true),
            flips(&[("overlay-0", true)])
        );
        assert_eq!(pills.note("overlay-0", "a", true), flips(&[]));
        assert_eq!(
            pills.note("overlay-0", "a", false),
            flips(&[("overlay-0", false)])
        );
    }

    #[test]
    fn an_overlay_stays_on_while_any_of_its_pills_is_open() {
        let mut pills = OpenPills::default();
        pills.note("overlay-0", "a", true);
        assert_eq!(pills.note("overlay-0", "b", true), flips(&[]));
        assert_eq!(pills.note("overlay-0", "a", false), flips(&[]));
        assert_eq!(
            pills.note("overlay-0", "b", false),
            flips(&[("overlay-0", false)])
        );
    }

    #[test]
    fn a_pill_carried_to_another_display_turns_the_old_overlay_off() {
        let mut pills = OpenPills::default();
        pills.note("overlay-0", "a", true);
        assert_eq!(
            pills.note("overlay-1", "a", true),
            flips(&[("overlay-0", false), ("overlay-1", true)])
        );
    }

    #[test]
    fn a_late_close_from_the_old_overlay_leaves_the_new_one_on() {
        let mut pills = OpenPills::default();
        pills.note("overlay-0", "a", true);
        pills.note("overlay-1", "a", true);
        assert_eq!(pills.note("overlay-0", "a", false), flips(&[]));
        assert_eq!(
            pills.note("overlay-1", "a", false),
            flips(&[("overlay-1", false)])
        );
    }

    #[test]
    fn a_carried_pill_keeps_the_old_overlay_on_for_its_other_pill() {
        let mut pills = OpenPills::default();
        pills.note("overlay-0", "a", true);
        pills.note("overlay-0", "b", true);
        assert_eq!(
            pills.note("overlay-1", "a", true),
            flips(&[("overlay-1", true)])
        );
    }

    #[test]
    fn closing_a_pill_mid_drive_keeps_the_keyboard() {
        let mut pills = OpenPills::default();
        pills.note("overlay-0", "a", true);
        assert_eq!(pills.note_drive("overlay-0", true), flips(&[]));
        assert_eq!(pills.note("overlay-0", "a", false), flips(&[]));
        assert!(
            pills.any_open(),
            "the possession card still holds the keyboard"
        );
        assert_eq!(
            pills.note_drive("overlay-0", false),
            flips(&[("overlay-0", false)])
        );
        assert!(!pills.any_open());
    }

    #[test]
    fn pills_stay_open_somewhere_while_a_second_display_owns_one() {
        let mut pills = OpenPills::default();
        assert!(!pills.any_open());
        pills.note("overlay-0", "a", true);
        pills.note("overlay-1", "b", true);
        // The first display's pill closes while the other display's is open:
        // nothing has handed the keyboard back yet.
        pills.note("overlay-0", "a", false);
        assert!(pills.any_open());
        pills.note("overlay-1", "b", false);
        assert!(!pills.any_open(), "the last close is what hands it back");
    }
}
