//! The Free tier on macOS, without consent.
//!
//! Three APIs, chosen because none is gated by TCC:
//!
//! - `NSWorkspace.frontmostApplication` names the application the user is in.
//!   It reports the application, never a window and never a title, so there is
//!   nothing here for Screen Recording to withhold.
//! - `CGEventSourceSecondsSinceLastEventType` reports how long ago the last
//!   input event was. It is a count of seconds, not the events themselves:
//!   reading what the user typed needs an event tap, and an event tap needs
//!   Accessibility. This module installs none.
//! - `CGDisplayIsAsleep` on the main display. A session Director does not
//!   wake while the lid is closed or the screens have gone to sleep.
//!
//! A window title and the clipboard stay unread here, and so do screen
//! contents. Screen capture is not this module (ADR-0031).

use std::time::Duration;

use objc2_app_kit::NSWorkspace;
use objc2_core_graphics::{CGEventSource, CGEventSourceStateID, CGEventType};

use fidget_core::sensing::ActivitySource;

/// `kCGAnyInputEventType`, which the header defines as exactly this and no
/// binding exposes as a constant. Any input at all counts: a key, the mouse, the
/// trackpad, a tablet.
const ANY_INPUT_EVENT: CGEventType = CGEventType(0xFFFF_FFFF);

/// The macOS view of what the user is doing.
#[derive(Clone, Copy, Debug, Default)]
pub struct MacosActivitySource;

impl ActivitySource for MacosActivitySource {
    /// The localized application name — "Safari", not "com.apple.Safari" — since
    /// the Director's context is read by a language model and shown to the user
    /// in settings.
    fn frontmost_application(&self) -> Option<String> {
        Some(
            NSWorkspace::sharedWorkspace()
                .frontmostApplication()?
                .localizedName()?
                .to_string(),
        )
    }

    /// Idle across the whole machine, from the HID system rather than from this
    /// session, so that input to any application counts and a locked screen
    /// still accumulates idle time.
    fn idle(&self) -> Duration {
        let seconds = CGEventSource::seconds_since_last_event_type(
            CGEventSourceStateID::HIDSystemState,
            ANY_INPUT_EVENT,
        );
        idle_from_seconds(seconds)
    }

    fn displays_asleep(&self) -> bool {
        // One display is enough: a lid-closed Mac sleeps the main one.
        // `boolean_t` is a C int, not Rust `bool`.
        // SAFETY: both are thread-safe (sensing is off the main thread); neither takes a pointer.
        unsafe { CGDisplayIsAsleep(CGMainDisplayID()) != 0 }
    }
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGMainDisplayID() -> u32;
    fn CGDisplayIsAsleep(display: u32) -> u32;
}

/// Turn the window server's reading into a duration.
/// Negative, infinite and NaN are unusable; Zero says the user is here,
/// where a wrong large value would put the Character to sleep at a live desk.
fn idle_from_seconds(seconds: f64) -> Duration {
    Duration::try_from_secs_f64(seconds).unwrap_or(Duration::ZERO)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fidget_core::sensing::{DesktopSense, SystemClock};

    /// Hand verification, deliberately not part of the suite: it needs a real
    /// window server, it reads the real clock and it sleeps, all of which
    /// `docs/SPEC.md` rules out for `cargo test`. `#[ignore]` keeps the suite
    /// pure and fast.
    ///
    /// ```text
    /// cargo test \
    ///     sensing -- --ignored --nocapture
    /// ```
    ///
    /// It prints the Free tier once a second for ten seconds. Switch
    /// application while it runs and watch the name follow and `switched`
    /// appear on exactly that read; then take your hands off the keyboard and
    /// watch idle climb, and touch it again to watch idle drop back. What no
    /// assertion can check is the thing the issue cares about most: that no
    /// permission dialog appeared.
    #[test]
    #[ignore = "needs a real desktop; run by hand"]
    fn the_live_desktop_sense_follows_the_real_machine() {
        let mut sense = DesktopSense::default();

        for _ in 0..10 {
            let activity = sense.read(&MacosActivitySource, &SystemClock);

            assert!(
                activity.frontmost_application.is_some(),
                "some application is always frontmost on a real desktop"
            );

            println!(
                "  idle {:>6.1}s  {}{}",
                activity.idle.as_secs_f64(),
                activity.frontmost_application.unwrap_or_default(),
                if activity.switched {
                    "  <- switched"
                } else {
                    ""
                },
            );

            std::thread::sleep(Duration::from_secs(1));
        }
    }

    /// The window server can report a reading that is not a duration. Each of
    /// these is a real possibility from a C API returning a bare double, and
    /// `Duration::try_from_secs_f64` rejects all three.
    #[test]
    fn an_unusable_idle_reading_is_treated_as_the_user_being_here() {
        assert_eq!(idle_from_seconds(-1.0), Duration::ZERO, "negative");
        assert_eq!(idle_from_seconds(f64::NAN), Duration::ZERO, "not a number");
        assert_eq!(idle_from_seconds(f64::INFINITY), Duration::ZERO, "infinite");
    }

    /// Zero is also the safe reading, so a usable value has to be carried
    /// through rather than flattened with the rest.
    #[test]
    fn a_usable_idle_reading_is_carried_through() {
        assert_eq!(idle_from_seconds(0.0), Duration::ZERO);
        assert_eq!(idle_from_seconds(90.5), Duration::from_millis(90_500));
    }
}
