//! What a quick-message pill holds while it is open.
//!
//! The overlay owns the text: it reports the draft as it changes. The shell
//! carries it every tick to whichever overlay owns the bubble, so the text
//! follows the Character across a display seam. A spoken line differs: it
//! comes from the engine, and the shell re-sends it only when the owner moves.

use std::time::Instant;

use crate::speech::SpeechBubble;

/// What the pill holds.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct QmDraft {
    pub text: String,
    /// Whether the field had the caret, so the new owner takes it only then.
    pub focused: bool,
}

/// Whether the walk holds at `now`: a line on screen or an open pill stops it.
pub fn walk_held(speech: &SpeechBubble, draft: Option<&QmDraft>, now: Instant) -> bool {
    speech.visible_at(now) || draft.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn walk_held_table() {
        let now = Instant::now();
        let draft = QmDraft {
            text: "wait".to_string(),
            focused: false,
        };
        let mut speaking = SpeechBubble::default();
        speaking.show_line(now, "hi");
        let quiet = SpeechBubble::default();
        let expired = now + Duration::from_millis(2000);

        let cases = [
            ("nothing on screen", &quiet, None, now, false),
            ("a line showing", &speaking, None, now, true),
            ("a pill open", &quiet, Some(&draft), now, true),
            ("both", &speaking, Some(&draft), now, true),
            ("a line expired, no pill", &speaking, None, expired, false),
        ];
        for (case, speech, draft, at, held) in cases {
            assert_eq!(walk_held(speech, draft, at), held, "{case}");
        }
    }
}
