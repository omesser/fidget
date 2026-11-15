//! How long a speech bubble stays up.
//!
//! The overlay times its own bubble. The shell keeps this clock so a resting
//! stroll can stop while that bubble is showing. The engine hears only who
//! it speaks for, so a sprite on its feet talks as that line's producer.

use std::time::{Duration, Instant};

use crate::engine::{AnimationSource, Dialogue};

/// The spoken line or ask on screen, timed by the shell.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SpeechBubble {
    line: Option<Line>,
}

/// The deadline and who the bubble speaks for, set and replaced together so
/// a new line never inherits the last one's producer. An expired line may
/// stay stored, as the deadline always has; `now < until` is what counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Line {
    until: Instant,
    source: AnimationSource,
}

impl SpeechBubble {
    /// Whether the bubble is still showing at `now`.
    pub fn visible_at(&self, now: Instant) -> bool {
        self.source_at(now).is_some()
    }

    /// Who the bubble speaks for at `now`, while it is still showing.
    pub fn source_at(&self, now: Instant) -> Option<AnimationSource> {
        self.line
            .filter(|shown| now < shown.until)
            .map(|shown| shown.source)
    }

    /// Show `line`, replacing any line or ask already up.
    pub fn show_line(&mut self, now: Instant, line: &Dialogue) {
        let units = line.text.encode_utf16().count() as u64;
        let ms = 900u64.saturating_add(55u64.saturating_mul(units));
        let ms = ms.clamp(2000, 8000);
        self.line = Some(Line {
            until: now + Duration::from_millis(ms),
            source: line.source,
        });
    }

    /// Show the ask. The question waits on a Harness turn, so it is the
    /// turn's feedback, not a reply.
    pub fn show_ask(&mut self, now: Instant) {
        self.line = Some(Line {
            until: now + Duration::from_millis(8000),
            source: AnimationSource::AiFeedback,
        });
    }

    /// Take the line down.
    pub fn hide(&mut self) {
        self.line = None;
    }
}

/// Apply one tick's dialogue to `bubble`.
pub fn note_dialogue(
    bubble: &mut SpeechBubble,
    now: Instant,
    line: Option<&Dialogue>,
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

    fn said(text: &str) -> Dialogue {
        Dialogue {
            text: text.to_string(),
            source: AnimationSource::AiRequested,
        }
    }

    #[test]
    fn a_short_line_stays_up_for_two_seconds() {
        let now = Instant::now();
        let mut bubble = SpeechBubble::default();
        note_dialogue(&mut bubble, now, Some(&said("hi")), false, true, false);
        assert!(bubble.visible_at(now + Duration::from_millis(1999)));
        assert!(!bubble.visible_at(now + Duration::from_millis(2000)));
    }

    #[test]
    fn a_thirty_character_line_lasts_until_the_unclamped_duration() {
        let now = Instant::now();
        let mut bubble = SpeechBubble::default();
        note_dialogue(
            &mut bubble,
            now,
            Some(&said(&"a".repeat(30))),
            false,
            true,
            false,
        );
        assert!(bubble.visible_at(now + Duration::from_millis(2549)));
        assert!(!bubble.visible_at(now + Duration::from_millis(2550)));
    }

    #[test]
    fn a_long_line_is_clamped_to_eight_seconds() {
        let now = Instant::now();
        let mut bubble = SpeechBubble::default();
        note_dialogue(
            &mut bubble,
            now,
            Some(&said(&"a".repeat(200))),
            false,
            true,
            false,
        );
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
        note_dialogue(&mut bubble, now, Some(&said("hi")), false, true, false);
        assert!(bubble.visible_at(now));
        note_dialogue(&mut bubble, now, Some(&said("hi")), false, true, true);
        assert!(!bubble.visible_at(now));
    }

    #[test]
    fn a_hidden_character_does_not_put_a_line_up() {
        let now = Instant::now();
        let mut bubble = SpeechBubble::default();
        note_dialogue(&mut bubble, now, Some(&said("hi")), false, false, false);
        assert!(!bubble.visible_at(now));
        assert!(!bubble.visible_at(now + Duration::from_millis(1999)));
    }

    #[test]
    fn a_quiet_tick_leaves_a_line_to_expire() {
        let now = Instant::now();
        let mut bubble = SpeechBubble::default();
        note_dialogue(&mut bubble, now, Some(&said("hi")), false, true, false);
        let later = now + Duration::from_millis(100);
        note_dialogue(&mut bubble, later, None, false, true, false);
        assert!(bubble.visible_at(now + Duration::from_millis(1999)));
        assert!(!bubble.visible_at(now + Duration::from_millis(2000)));
    }

    #[test]
    fn a_second_line_replaces_the_deadline() {
        let now = Instant::now();
        let mut bubble = SpeechBubble::default();
        note_dialogue(&mut bubble, now, Some(&said("hi")), false, true, false);
        let now2 = now + Duration::from_millis(500);
        note_dialogue(
            &mut bubble,
            now2,
            Some(&said(&"a".repeat(200))),
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
        note_dialogue(&mut bubble, now, Some(&said(&line)), false, true, false);
        // 22 UTF-16 units: 900 + 55 * 22. `chars` would count the emoji as one.
        assert!(bubble.visible_at(now + Duration::from_millis(2109)));
        assert!(!bubble.visible_at(now + Duration::from_millis(2110)));
    }
}
