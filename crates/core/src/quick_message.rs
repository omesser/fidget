//! What a quick-message pill holds while it is open.
//!
//! The overlay owns the text: it reports the draft as it changes. The shell
//! carries it every tick to whichever overlay owns the bubble, so the text
//! follows the Character across a display seam. A spoken line differs: it
//! comes from the engine, and the shell re-sends it only when the owner moves.

use std::time::Instant;

use crate::engine::AnimationSource;
use crate::speech::SpeechBubble;

/// What the pill holds.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct QmDraft {
    pub text: String,
    /// Whether the field had the caret, so the new owner takes it only then.
    pub focused: bool,
}

/// Who the sprite speaks for at `now`, if anyone: a line on screen, else an
/// open pill. Either stops the walk, and a sprite on its feet talks as them.
/// A line still up outranks even an empty pill.
pub fn speaking_for(
    speech: &SpeechBubble,
    draft: Option<&QmDraft>,
    now: Instant,
) -> Option<AnimationSource> {
    speech
        .source_at(now)
        .or(draft.map(|_| AnimationSource::UserInvoked))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Dialogue;
    use std::time::Duration;

    fn line(text: &str, source: AnimationSource) -> Dialogue {
        Dialogue {
            text: text.to_string(),
            source,
        }
    }

    #[test]
    fn speaking_for_table() {
        let now = Instant::now();
        let empty_unfocused = QmDraft {
            text: String::new(),
            focused: false,
        };
        let mut replying = SpeechBubble::default();
        replying.show_line(now, &line("hi", AnimationSource::AiRequested));
        let mut touring = SpeechBubble::default();
        touring.show_line(now, &line("hi", AnimationSource::Onboarding));
        let mut asking = SpeechBubble::default();
        asking.show_ask(now);
        let quiet = SpeechBubble::default();
        // "hi" holds 2000 ms, and the deadline itself is already past.
        let deadline = now + Duration::from_millis(2000);
        let just_before = now + Duration::from_millis(1999);

        let cases = [
            ("nothing on screen", &quiet, None, now, None),
            (
                "a reply line",
                &replying,
                None,
                now,
                Some(AnimationSource::AiRequested),
            ),
            (
                "the tour's line",
                &touring,
                None,
                now,
                Some(AnimationSource::Onboarding),
            ),
            (
                "the question-wait ask",
                &asking,
                None,
                now,
                Some(AnimationSource::AiFeedback),
            ),
            (
                "an empty, unfocused pill",
                &quiet,
                Some(&empty_unfocused),
                now,
                Some(AnimationSource::UserInvoked),
            ),
            (
                "a line beats the pill",
                &replying,
                Some(&empty_unfocused),
                just_before,
                Some(AnimationSource::AiRequested),
            ),
            (
                "the line expires first, the pill stays",
                &replying,
                Some(&empty_unfocused),
                deadline,
                Some(AnimationSource::UserInvoked),
            ),
            (
                "the pill closes first, the line runs on",
                &replying,
                None,
                just_before,
                Some(AnimationSource::AiRequested),
            ),
            ("both end", &replying, None, deadline, None),
        ];
        for (case, speech, draft, at, speaker) in cases {
            assert_eq!(speaking_for(speech, draft, at), speaker, "{case}");
        }
    }

    /// Reading who speaks is a question, so asking it at the deadline's edge
    /// neither clears it nor moves it.
    #[test]
    fn reading_who_speaks_leaves_the_deadline_alone() {
        let now = Instant::now();
        let mut bubble = SpeechBubble::default();
        bubble.show_line(now, &line("hi", AnimationSource::AiRequested));
        let before = bubble.clone();
        let late = now + Duration::from_millis(5000);
        assert_eq!(speaking_for(&bubble, None, late), None);
        assert_eq!(bubble, before);
        assert_eq!(
            bubble.source_at(now + Duration::from_millis(1999)),
            Some(AnimationSource::AiRequested)
        );
    }
}
