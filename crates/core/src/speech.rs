//! How long a speech bubble stays up.
//!
//! The overlay times its own bubble. The shell keeps this clock so a resting
//! stroll can stop while that bubble is showing. The engine only hears a bool.

use std::time::{Duration, Instant};

/// A hold on a spoken line or an ask, owned by the shell.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SpeechBubble {
    until: Option<Instant>,
}

impl SpeechBubble {
    /// Whether the bubble is still showing at `now`.
    pub fn visible_at(&self, now: Instant) -> bool {
        self.until.is_some_and(|until| now < until)
    }

    /// Start a hold for `line`, replacing any hold already running.
    pub fn show_line(&mut self, now: Instant, line: &str) {
        let units = line.encode_utf16().count() as u64;
        let ms = 900u64.saturating_add(55u64.saturating_mul(units));
        let ms = ms.clamp(2000, 8000);
        self.until = Some(now + Duration::from_millis(ms));
    }

    /// Start the ask hold.
    pub fn show_ask(&mut self, now: Instant) {
        self.until = Some(now + Duration::from_millis(8000));
    }

    /// Drop the hold.
    pub fn hide(&mut self) {
        self.until = None;
    }
}

/// Apply one tick's dialogue to `bubble`.
pub fn note_dialogue(
    bubble: &mut SpeechBubble,
    now: Instant,
    line: Option<&str>,
    asking: bool,
    character_visible: bool,
    instant_hide: bool,
) {
    if instant_hide {
        bubble.hide();
        return;
    }
    if !character_visible {
        return;
    }
    if let Some(line) = line {
        bubble.show_line(now, line);
    } else if asking {
        bubble.show_ask(now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn a_short_line_stays_up_for_two_seconds() {
        let now = Instant::now();
        let mut bubble = SpeechBubble::default();
        note_dialogue(&mut bubble, now, Some("hi"), false, true, false);
        assert!(bubble.visible_at(now + Duration::from_millis(1999)));
        assert!(!bubble.visible_at(now + Duration::from_millis(2000)));
    }

    #[test]
    fn a_thirty_character_line_lasts_until_the_unclamped_duration() {
        let now = Instant::now();
        let mut bubble = SpeechBubble::default();
        note_dialogue(&mut bubble, now, Some(&"a".repeat(30)), false, true, false);
        assert!(bubble.visible_at(now + Duration::from_millis(2549)));
        assert!(!bubble.visible_at(now + Duration::from_millis(2550)));
    }

    #[test]
    fn a_long_line_is_clamped_to_eight_seconds() {
        let now = Instant::now();
        let mut bubble = SpeechBubble::default();
        note_dialogue(&mut bubble, now, Some(&"a".repeat(200)), false, true, false);
        assert!(bubble.visible_at(now + Duration::from_millis(7999)));
        assert!(!bubble.visible_at(now + Duration::from_millis(8000)));
    }

    #[test]
    fn an_ask_with_no_line_stays_up_for_eight_seconds() {
        let now = Instant::now();
        let mut bubble = SpeechBubble::default();
        note_dialogue(&mut bubble, now, None, true, true, false);
        assert!(bubble.visible_at(now + Duration::from_millis(7999)));
        assert!(!bubble.visible_at(now + Duration::from_millis(8000)));
    }

    #[test]
    fn instant_hide_clears_even_when_a_line_arrives() {
        let now = Instant::now();
        let mut bubble = SpeechBubble::default();
        note_dialogue(&mut bubble, now, Some("hi"), false, true, false);
        assert!(bubble.visible_at(now));
        note_dialogue(&mut bubble, now, Some("hi"), false, true, true);
        assert!(!bubble.visible_at(now));
    }

    #[test]
    fn a_hidden_character_does_not_start_a_hold() {
        let now = Instant::now();
        let mut bubble = SpeechBubble::default();
        note_dialogue(&mut bubble, now, Some("hi"), false, false, false);
        assert!(!bubble.visible_at(now));
        assert!(!bubble.visible_at(now + Duration::from_millis(1999)));
    }

    #[test]
    fn a_quiet_tick_leaves_a_hold_to_expire() {
        let now = Instant::now();
        let mut bubble = SpeechBubble::default();
        note_dialogue(&mut bubble, now, Some("hi"), false, true, false);
        let later = now + Duration::from_millis(100);
        note_dialogue(&mut bubble, later, None, false, true, false);
        assert!(bubble.visible_at(now + Duration::from_millis(1999)));
        assert!(!bubble.visible_at(now + Duration::from_millis(2000)));
    }

    #[test]
    fn a_second_line_replaces_the_deadline() {
        let now = Instant::now();
        let mut bubble = SpeechBubble::default();
        note_dialogue(&mut bubble, now, Some("hi"), false, true, false);
        let now2 = now + Duration::from_millis(500);
        note_dialogue(
            &mut bubble,
            now2,
            Some(&"a".repeat(200)),
            false,
            true,
            false,
        );
        assert!(bubble.visible_at(now2 + Duration::from_millis(7999)));
        assert!(!bubble.visible_at(now2 + Duration::from_millis(8000)));
    }

    #[test]
    fn a_line_counts_utf16_units_the_way_the_overlay_does() {
        let now = Instant::now();
        let mut bubble = SpeechBubble::default();
        let line = format!("{}👍", "a".repeat(20));
        note_dialogue(&mut bubble, now, Some(&line), false, true, false);
        // 22 UTF-16 units: 900 + 55 * 22. `chars` would count the emoji as one.
        assert!(bubble.visible_at(now + Duration::from_millis(2109)));
        assert!(!bubble.visible_at(now + Duration::from_millis(2110)));
    }
}
