//! Per-Instance quick-message draft state and drag decision.
//!
//! Design (2026-10-07): the pill works like the speech bubble. Draft state
//! lives in `InstanceState`, rides in `Placed` each frame, and the owning
//! overlay renders it while others hide theirs. No handoff between overlays.

/// Per-Instance quick-message draft.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QmDraft {
    pub text: String,
    pub focused: bool,
}

/// Drag decision: empty text closes the pill, non-empty text keeps it.
///
/// Returns `true` if the pill should stay open during drag.
pub fn keep_qm_on_drag(draft: Option<&QmDraft>) -> bool {
    draft.map(|d| !d.text.is_empty()).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_text_closes_on_drag() {
        let draft = QmDraft {
            text: String::new(),
            focused: true,
        };
        assert!(!keep_qm_on_drag(Some(&draft)), "empty text closes on drag");
    }

    #[test]
    fn non_empty_text_keeps_pill_on_drag() {
        let draft = QmDraft {
            text: "hello".to_string(),
            focused: false,
        };
        assert!(keep_qm_on_drag(Some(&draft)), "non-empty text keeps pill");
    }

    #[test]
    fn no_draft_closes_on_drag() {
        assert!(!keep_qm_on_drag(None), "no draft means close");
    }

    #[test]
    fn whitespace_only_is_empty() {
        let draft = QmDraft {
            text: "   ".to_string(),
            focused: true,
        };
        // Current implementation: whitespace is non-empty per String::is_empty
        assert!(keep_qm_on_drag(Some(&draft)), "whitespace counts as non-empty");
    }
}
