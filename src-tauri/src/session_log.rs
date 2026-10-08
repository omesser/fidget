//! Current-session Chat turns, held because `emit_to` only reaches windows that
//! exist. Permission asks already wait on `PendingAsks`; Speech of this
//! Completer session belongs in the same log (ADR-0018) even when Chat was
//! never opened, and so does the thinking that led to it (ADR-0034).

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::SystemTime;

use tauri::{Emitter, Manager};

use crate::harness::Replayed;

/// Whose row a remembered turn is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Who {
    You,
    Them,
    Thinking,
}

#[derive(Clone)]
pub struct Turn {
    pub who: Who,
    pub said: Option<String>,
    pub reacting_to: Option<String>,
    /// When the line was said, not when Chat later opened. Replay stamps from this.
    pub at: SystemTime,
}

/// The thinking of an Instance's turn on the wire, waiting here until that
/// Instance's wake is remembered.
struct Thinking {
    text: String,
    at: SystemTime,
    over: bool,
}

#[derive(Default)]
pub struct Log {
    turns: BTreeMap<String, Vec<Turn>>,
    thinking: BTreeMap<String, Thinking>,
    /// What a loaded session replayed (#1393), drawn above `turns`.
    restored: BTreeMap<String, Vec<Replayed>>,
}

impl Log {
    pub fn new() -> Self {
        Self::default()
    }

    /// Thinking still waiting here belongs to a turn nothing filed: one that
    /// was cancelled, or one this line cuts off mid-thought. Either way the
    /// typed line drops it.
    pub fn remember_you(&mut self, instance: &str, text: impl Into<String>, at: SystemTime) {
        self.thinking.remove(instance);
        self.turns
            .entry(instance.to_string())
            .or_default()
            .push(Turn {
                who: Who::You,
                said: Some(text.into()),
                reacting_to: None,
                at,
            });
    }

    /// The whole thought so far, or an empty one when the turn stops thinking.
    /// A thought after that is the next turn's, so the unkept one is dropped.
    pub fn think(&mut self, instance: &str, text: &str, at: SystemTime) {
        match self.thinking.get_mut(instance) {
            Some(thinking) if text.is_empty() => thinking.over = true,
            _ if text.is_empty() => {}
            Some(thinking) if !thinking.over => thinking.text = text.to_string(),
            _ => {
                self.thinking.insert(
                    instance.to_string(),
                    Thinking {
                        text: text.to_string(),
                        at,
                        over: false,
                    },
                );
            }
        }
    }

    /// File this Instance's waiting thinking under the wake that just
    /// arrived, spoken or not.
    pub fn remember_thinking(&mut self, instance: &str) {
        if let Some(thinking) = self.thinking.remove(instance) {
            self.turns
                .entry(instance.to_string())
                .or_default()
                .push(Turn {
                    who: Who::Thinking,
                    said: Some(thinking.text),
                    reacting_to: None,
                    at: thinking.at,
                });
        }
    }

    /// The turn's thinking goes in above it. A wake with no words holds
    /// nothing else (ADR-0018).
    pub fn remember_them(
        &mut self,
        instance: &str,
        said: Option<String>,
        reacting_to: Option<String>,
        at: SystemTime,
    ) {
        self.remember_thinking(instance);
        let Some(said) = said else {
            return;
        };
        self.turns
            .entry(instance.to_string())
            .or_default()
            .push(Turn {
                who: Who::Them,
                said: Some(said),
                reacting_to,
                at,
            });
    }

    /// A loaded session's replay, in place of any this Instance held.
    pub fn restore(&mut self, instance: &str, history: Vec<Replayed>) {
        self.restored.insert(instance.to_string(), history);
    }

    pub fn restored(&self, instance: &str) -> Vec<Replayed> {
        self.restored.get(instance).cloned().unwrap_or_default()
    }

    pub fn replay(&self, instance: &str) -> Vec<Turn> {
        self.turns.get(instance).cloned().unwrap_or_default()
    }

    /// Drop what one Instance's replaced session said.
    ///
    /// One Instance, not all of them: wiping the others would empty windows still standing.
    pub fn forget(&mut self, instance: &str) {
        self.turns.remove(instance);
        self.thinking.remove(instance);
        self.restored.remove(instance);
    }
}

fn with_log(app: &tauri::AppHandle, f: impl FnOnce(&mut Log)) {
    if let Some(held) = app.try_state::<Mutex<Log>>() {
        if let Ok(mut log) = held.lock() {
            f(&mut log);
        }
    }
}

pub fn remember_you(
    app: &tauri::AppHandle,
    instance: &str,
    text: impl Into<String>,
    at: SystemTime,
) {
    with_log(app, |log| log.remember_you(instance, text, at));
}

pub fn think(app: &tauri::AppHandle, instance: &str, text: &str, at: SystemTime) {
    with_log(app, |log| log.think(instance, text, at));
}

pub fn remember_thinking(app: &tauri::AppHandle, instance: &str) {
    with_log(app, |log| log.remember_thinking(instance));
}

pub fn remember_them(
    app: &tauri::AppHandle,
    instance: &str,
    said: Option<String>,
    reacting_to: Option<String>,
    at: SystemTime,
) {
    with_log(app, |log| {
        log.remember_them(instance, said, reacting_to, at)
    });
}

pub fn restore(app: &tauri::AppHandle, instance: &str, history: Vec<Replayed>) {
    with_log(app, |log| log.restore(instance, history));
}

pub fn restored(app: &tauri::AppHandle, instance: &str) -> Vec<Replayed> {
    app.try_state::<Mutex<Log>>()
        .and_then(|held| held.lock().ok().map(|log| log.restored(instance)))
        .unwrap_or_default()
}

pub fn replay(app: &tauri::AppHandle, instance: &str) -> Vec<Turn> {
    app.try_state::<Mutex<Log>>()
        .and_then(|held| held.lock().ok().map(|log| log.replay(instance)))
        .unwrap_or_default()
}

/// A dismissed Instance's turns go with it.
///
/// Own call so one Instance's new session does not empty another's window.
pub fn forget(app: &tauri::AppHandle, instance: &str) {
    with_log(app, |log| log.forget(instance));
}

/// The Completer session behind `instance` was replaced, for the reason `why`.
///
/// Turns, the Chat surface, and the Action Log move together. Call beside `completer::retarget_model`.
pub fn new_session(app: &tauri::AppHandle, instance: &str, why: &str) {
    forget(app, instance);
    crate::action_log::append(
        &fidget_core::memory::data_dir(),
        "session",
        serde_json::json!({ "instance": instance, "why": why }),
    );
    let _ = app.emit_to(
        crate::chat_label(instance),
        crate::chat_surface::CHAT_SESSION_EVENT,
        why,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    /// Production change that would fail this: dropping a spoken line because
    /// no Chat surface was listening. ADR-0018 puts every Speech in that log.
    #[test]
    fn a_spoken_line_is_still_there_when_chat_opens_later() {
        let mut log = Log::new();
        log.remember_them(
            "buddy-1",
            Some("hello from the bubble".into()),
            Some("when poked".into()),
            UNIX_EPOCH,
        );

        let turns = log.replay("buddy-1");
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].who, Who::Them);
        assert_eq!(turns[0].said.as_deref(), Some("hello from the bubble"));
        assert_eq!(turns[0].reacting_to.as_deref(), Some("when poked"));
    }

    /// Production change that would fail this: replaying Instance A's Speech
    /// into Instance B's window.
    #[test]
    fn replay_is_the_instance_that_said_it() {
        let mut log = Log::new();
        log.remember_them(
            "a",
            Some("from A".into()),
            Some("unprompted".into()),
            UNIX_EPOCH,
        );
        log.remember_them(
            "b",
            Some("from B".into()),
            Some("when summoned".into()),
            UNIX_EPOCH,
        );

        let a = log.replay("a");
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].said.as_deref(), Some("from A"));
        assert!(log.replay("missing").is_empty());
    }

    /// Production change that would fail this: holding a Behavior-only wake
    /// that has no words (ADR-0018: nothing is held for that).
    #[test]
    fn a_wake_with_no_speech_is_not_held() {
        let mut log = Log::new();
        log.remember_them("buddy-1", None, None, UNIX_EPOCH);
        assert!(log.replay("buddy-1").is_empty());
    }

    /// Production change that would fail this: Chat showing only the fidget's
    /// lines after a close and reopen, dropping the typed request.
    #[test]
    fn typed_and_spoken_stay_in_order() {
        let mut log = Log::new();
        log.remember_them(
            "buddy-1",
            Some("unprompted hi".into()),
            Some("unprompted".into()),
            UNIX_EPOCH,
        );
        log.remember_you("buddy-1", "what are you standing on?", UNIX_EPOCH);
        log.remember_them(
            "buddy-1",
            Some("the desktop floor".into()),
            None,
            UNIX_EPOCH,
        );

        let turns = log.replay("buddy-1");
        assert_eq!(turns.len(), 3);
        assert!(turns[0].reacting_to.is_some());
        assert_eq!(turns[1].who, Who::You);
        assert_eq!(turns[1].said.as_deref(), Some("what are you standing on?"));
        assert_eq!(turns[2].who, Who::Them);
        assert_eq!(turns[2].said.as_deref(), Some("the desktop floor"));
    }

    /// Production change that would fail this: splicing a loaded session's
    /// history into this run's turns, stacking a second restore on the first,
    /// or keeping it after the session is replaced (#1393).
    #[test]
    fn restored_history_stands_apart_once_until_the_session_goes() {
        let mut log = Log::new();
        log.remember_you("buddy-1", "are you back?", UNIX_EPOCH);
        let reply = |text: &str| Replayed::Reply {
            text: text.to_string(),
        };

        log.restore("buddy-1", vec![reply("Hello from before")]);
        log.restore("buddy-1", vec![reply("Hello from before"), reply("Done.")]);
        log.restore("buddy-2", vec![reply("Other window")]);

        assert_eq!(
            log.restored("buddy-1"),
            [reply("Hello from before"), reply("Done.")]
        );
        let turns: Vec<_> = log
            .replay("buddy-1")
            .iter()
            .map(|turn| (turn.who, turn.said.clone()))
            .collect();
        assert_eq!(turns, [(Who::You, Some("are you back?".to_string()))]);
        log.forget("buddy-1");
        assert_eq!(log.restored("buddy-1"), []);
        assert_eq!(log.restored("buddy-2"), [reply("Other window")]);
    }

    /// Production change that would fail this: forgetting every Instance's
    /// turns when one Instance's session is reopened. Saving an Instance Prompt
    /// reopens that session only; the fidget beside it is still mid-conversation.
    #[test]
    fn forgetting_one_instance_leaves_the_others_conversation() {
        let mut log = Log::new();
        log.remember_you("saved", "before the edit", UNIX_EPOCH);
        log.remember_you("other", "still talking", UNIX_EPOCH);

        log.forget("saved");

        assert!(log.replay("saved").is_empty());
        assert_eq!(log.replay("other").len(), 1);
    }

    /// Production change that would fail this: keeping turns after Retarget
    /// replaced the Completer session (#476: Chat is this session only).
    #[test]
    fn retarget_forgets_the_old_session() {
        let mut log = Log::new();
        log.remember_you("buddy-1", "old session", UNIX_EPOCH);
        log.forget("buddy-1");
        assert!(log.replay("buddy-1").is_empty());
    }

    /// Production change that would fail this: emptying every fidget's log on a
    /// Character switch, which replaces one Instance's session and leaves the
    /// rest answering out of the conversation their windows still show. #476.
    #[test]
    fn a_switched_buddy_does_not_forget_the_others() {
        let mut log = Log::new();
        log.remember_you("switched", "before the switch", UNIX_EPOCH);
        log.remember_you("untouched", "still this session", UNIX_EPOCH);

        log.forget("switched");

        assert!(log.replay("switched").is_empty());
        assert_eq!(log.replay("untouched").len(), 1);
    }

    /// Production change that would fail this: stamping a replayed line with
    /// Chat-open time instead of the instant it was said (ADR-0018: one conversation).
    #[test]
    fn replay_keeps_the_moment_the_line_was_said() {
        let mut log = Log::new();
        let first = UNIX_EPOCH + Duration::from_secs(1_000);
        let second = first + Duration::from_secs(20 * 60);
        log.remember_you("buddy-1", "typed twenty minutes ago", first);
        log.remember_them("buddy-1", Some("answered later".into()), None, second);

        let turns = log.replay("buddy-1");
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0].at, first);
        assert_eq!(turns[1].at, second);
        assert_ne!(turns[0].at, turns[1].at);
        assert_ne!(turns[0].at, SystemTime::now());
        assert_ne!(turns[1].at, SystemTime::now());
    }

    /// Production change that would fail this: a reopened Chat showing the
    /// answer without the thinking that led to it (ADR-0034), or a thought
    /// kept as each partial text the wire sent rather than the whole one.
    #[test]
    fn thinking_is_kept_above_the_reply_it_led_to() {
        let mut log = Log::new();
        let first = UNIX_EPOCH + Duration::from_secs(1_000);
        log.remember_you("buddy-1", "what are you standing on?", first);
        log.think("buddy-1", "Reading", first);
        log.think(
            "buddy-1",
            "Reading the roster",
            first + Duration::from_secs(2),
        );
        log.think("buddy-1", "", first + Duration::from_secs(3));
        log.remember_them(
            "buddy-1",
            Some("the desktop floor".into()),
            None,
            first + Duration::from_secs(4),
        );

        let turns = log.replay("buddy-1");
        let kept: Vec<_> = turns
            .iter()
            .map(|t| (t.who, t.said.as_deref(), t.at))
            .collect();
        assert_eq!(
            kept,
            [
                (Who::You, Some("what are you standing on?"), first),
                (Who::Thinking, Some("Reading the roster"), first),
                (
                    Who::Them,
                    Some("the desktop floor"),
                    first + Duration::from_secs(4)
                ),
            ]
        );
    }

    /// Production change that would fail this: a cancelled turn's thinking,
    /// which nothing filed, surfacing above the next turn's reply instead.
    #[test]
    fn a_cancelled_turns_thinking_gives_way_to_the_next_turns() {
        let mut log = Log::new();
        let later = UNIX_EPOCH + Duration::from_secs(60);
        log.think("buddy-1", "Should I nap?", UNIX_EPOCH);
        log.think("buddy-1", "", UNIX_EPOCH);
        log.think("buddy-1", "Checking the desk", later);
        log.remember_them("buddy-1", Some("on it".into()), None, later);

        let turns = log.replay("buddy-1");
        assert_eq!(turns[0].who, Who::Thinking);
        assert_eq!(turns[0].said.as_deref(), Some("Checking the desk"));
        assert_eq!(turns[0].at, later);
        assert_eq!(turns.len(), 2);
    }

    /// Production change that would fail this: a turn with thinking and no
    /// line losing the thinking too, which is the only trace that turn left.
    #[test]
    fn thinking_is_kept_when_the_turn_said_nothing() {
        let mut log = Log::new();
        log.think("buddy-1", "Weighing a nap", UNIX_EPOCH);
        log.remember_them("buddy-1", None, None, UNIX_EPOCH);

        let turns = log.replay("buddy-1");
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].who, Who::Thinking);
    }

    /// Production change that would fail this: a wake that moved the sprite and
    /// said nothing dropping the thinking the live window showed, or leaving it
    /// to land above the next, unrelated reply.
    #[test]
    fn a_wake_that_says_nothing_keeps_its_thinking_and_no_more() {
        let mut log = Log::new();
        log.think("buddy-1", "Should I nap?", UNIX_EPOCH);
        log.think("buddy-1", "", UNIX_EPOCH);
        log.remember_thinking("buddy-1");
        log.remember_them("buddy-1", Some("hi".into()), None, UNIX_EPOCH);

        let who: Vec<_> = log.replay("buddy-1").iter().map(|t| t.who).collect();
        assert_eq!(who, [Who::Thinking, Who::Them]);
    }

    /// Production change that would fail this: a typed question's reply, which
    /// thought nothing, filed under the thinking of a turn that was cancelled.
    #[test]
    fn a_typed_line_drops_thinking_nothing_filed() {
        let mut log = Log::new();
        log.think("buddy-1", "Half a thought", UNIX_EPOCH);
        log.think("buddy-1", "", UNIX_EPOCH);
        log.remember_you("buddy-1", "next question", UNIX_EPOCH);
        log.remember_them("buddy-1", Some("answer".into()), None, UNIX_EPOCH);

        let who: Vec<_> = log.replay("buddy-1").iter().map(|t| t.who).collect();
        assert_eq!(who, [Who::You, Who::Them]);
    }

    /// Production change that would fail this: a turn the typed line cut off
    /// mid-thought, whose thinking never got its empty settle, filing that
    /// thinking above the typed question's reply.
    #[test]
    fn a_typed_line_drops_thinking_cut_off_mid_stream() {
        let mut log = Log::new();
        log.think("buddy-1", "Half a thought", UNIX_EPOCH);
        log.remember_you("buddy-1", "next question", UNIX_EPOCH);
        log.remember_them("buddy-1", Some("answer".into()), None, UNIX_EPOCH);

        let who: Vec<_> = log.replay("buddy-1").iter().map(|t| t.who).collect();
        assert_eq!(who, [Who::You, Who::Them]);
    }

    /// Production change that would fail this: the old session's thinking
    /// landing above the new session's first reply.
    #[test]
    fn a_replaced_session_forgets_thinking_still_on_the_wire() {
        let mut log = Log::new();
        log.think("buddy-1", "From the old session", UNIX_EPOCH);
        log.forget("buddy-1");
        log.remember_them("buddy-1", Some("fresh".into()), None, UNIX_EPOCH);

        let who: Vec<_> = log.replay("buddy-1").iter().map(|t| t.who).collect();
        assert_eq!(who, [Who::Them]);
    }

    /// Production change that would fail this: thinking waiting in one slot
    /// for whichever Instance wakes next, so two Instances thinking at once on
    /// the HTTP lane file each other's thinking above their replies.
    #[test]
    fn two_instances_thinking_at_once_file_to_their_own_replies() {
        let mut log = Log::new();
        log.think("buddy-a", "A reads the roster", UNIX_EPOCH);
        log.think("buddy-b", "B checks the desk", UNIX_EPOCH);
        log.think("buddy-a", "A reads the roster twice", UNIX_EPOCH);
        log.think("buddy-b", "", UNIX_EPOCH);
        log.remember_them("buddy-b", Some("B answers".into()), None, UNIX_EPOCH);
        log.think("buddy-a", "", UNIX_EPOCH);
        log.remember_them("buddy-a", Some("A answers".into()), None, UNIX_EPOCH);

        let kept = |instance| {
            log.replay(instance)
                .into_iter()
                .map(|t| (t.who, t.said))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            kept("buddy-a"),
            [
                (Who::Thinking, Some("A reads the roster twice".into())),
                (Who::Them, Some("A answers".into())),
            ]
        );
        assert_eq!(
            kept("buddy-b"),
            [
                (Who::Thinking, Some("B checks the desk".into())),
                (Who::Them, Some("B answers".into())),
            ]
        );
    }

    /// Production change that would fail this: a line typed to one fidget, or
    /// one fidget's replaced session, dropping another fidget's thinking.
    #[test]
    fn one_instances_typed_line_or_new_session_keeps_anothers_thinking() {
        let mut log = Log::new();
        log.think("buddy-b", "B checks the desk", UNIX_EPOCH);
        log.remember_you("buddy-a", "hello A", UNIX_EPOCH);
        log.forget("buddy-a");
        log.remember_them("buddy-b", Some("B answers".into()), None, UNIX_EPOCH);

        let who: Vec<_> = log.replay("buddy-b").iter().map(|t| t.who).collect();
        assert_eq!(who, [Who::Thinking, Who::Them]);
    }
}
