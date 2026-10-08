//! Names of the events a Chat surface listens for, and payloads that are
//! only the bytes on those events.

use serde::Serialize;

/// The event carrying one turn's answer to a Chat surface.
pub(crate) const CHAT_EVENT: &str = "chat";

/// Spatial Layer state for a Chat surface's status bar. Separate from
/// `CHAT_EVENT`: this arrives whenever the sprite does something different,
/// whether or not anyone has typed.
pub(crate) const CHAT_STATUS_EVENT: &str = "chat-status";

/// Full opening to an already-open Chat surface, so `attached()` can re-run
/// without a webview reload. An event rather than a second command, because
/// the window is already listening.
pub(crate) const CHAT_OPENING_EVENT: &str = "chat-opening";

/// The event telling one Chat surface that the session behind it was replaced,
/// carrying why in the words the log prints. `chat.js` says what the window
/// does with it, and why.
pub(crate) const CHAT_SESSION_EVENT: &str = "chat-session";

/// What a loaded session said before this run, as the lines the window
/// draws above its log. Not `CHAT_EVENT`: these answer no question the window
/// is waiting on, and are not a wake (#1393).
pub(crate) const CHAT_RESTORED_EVENT: &str = "chat-restored";

/// Forwarded `session/request_permission` to the Chat surface of the Instance
/// that owes it, or to every open one when none does (`draws_in`). The first
/// answer wins. fidget never answers it.
pub(crate) const CHAT_PERMISSION_EVENT: &str = "chat-permission";

/// A forwarded `elicitation/create` form, addressed as a permission ask is.
/// The first answer wins.
pub(crate) const CHAT_ELICITATION_EVENT: &str = "chat-elicitation";

/// The Harness's whole thought so far, for the Thinking row in the log. Each
/// one replaces the last; an empty one is the turn saying it has stopped.
pub(crate) const CHAT_THOUGHT_EVENT: &str = "chat-thought";

/// The event carrying the agent's plan to the Chat surface of the Instance
/// whose session planned. Each one replaces the whole list, and an empty one
/// is the turn taking it away.
pub(crate) const CHAT_PLAN_EVENT: &str = "chat-plan";

/// Chat UI selection change, telling each chat surface to swap its root class.
pub(crate) const CHAT_UI_EVENT: &str = "chat-ui";

pub(crate) const CHAT_APPEARANCE_EVENT: &str = "chat-appearance";

/// Retires one forwarded request in every open Chat surface, by request id.
/// An ask no Instance owes went to all of them and one took the click; the
/// rest would otherwise keep offering buttons on a question already answered.
/// A window that never drew the request ignores it.
pub(crate) const CHAT_PERMISSION_SETTLED_EVENT: &str = "chat-permission-settled";

/// What retires one row in every open Chat surface.
#[derive(Clone, Serialize)]
pub(crate) struct Settled {
    pub(crate) request: String,
    /// The option that won; `None` when nothing was picked. Every window
    /// draws this rather than its own click: two can draw one request, and
    /// the wire drops every answer after the first.
    pub(crate) option: Option<String>,
}
