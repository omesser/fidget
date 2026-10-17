//! Which overlay windows hold an open quick message pill. An overlay takes the
//! keyboard only while one of its own pills is open (#1565).

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

/// Open pills by overlay window label, each a set of Instance ids.
#[derive(Default)]
pub struct OpenPills(HashMap<String, HashSet<String>>);

impl OpenPills {
    /// Record `instance`'s pill on `window` as open or closed. Returns each
    /// window whose "any pill open" state flipped, with its new state.
    pub fn note(&mut self, window: &str, instance: &str, open: bool) -> Vec<(String, bool)> {
        let before = self.on();
        if open {
            // One pill per Instance: opening here closes it on any other overlay.
            for (label, pills) in &mut self.0 {
                if label != window {
                    pills.remove(instance);
                }
            }
            self.0
                .entry(window.to_string())
                .or_default()
                .insert(instance.to_string());
        } else if let Some(pills) = self.0.get_mut(window) {
            pills.remove(instance);
        }
        let after = self.on();
        let mut flips: Vec<(String, bool)> = before
            .difference(&after)
            .map(|label| (label.clone(), false))
            .chain(after.difference(&before).map(|label| (label.clone(), true)))
            .collect();
        flips.sort();
        flips
    }

    fn on(&self) -> HashSet<String> {
        self.0
            .iter()
            .filter(|(_, pills)| !pills.is_empty())
            .map(|(label, _)| label.clone())
            .collect()
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
pub fn hand_back(holding: bool, previous_alive: bool) -> HandBack {
    match (holding, previous_alive) {
        (false, _) => HandBack::Stay,
        (true, true) => HandBack::Restore,
        (true, false) => HandBack::StepAside,
    }
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
}
