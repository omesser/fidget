//! The quick-message draft an Instance carries while its pill is open.

/// What the pill holds. The Shell hands it to whichever overlay owns the
/// Instance's bubble, so the text follows the Character across a display seam.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct QmDraft {
    pub text: String,
    /// Whether the field had the caret, so the new owner takes it only then.
    pub focused: bool,
}
