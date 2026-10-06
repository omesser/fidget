//! The in-flight Director session.
//!
//! `model` is the HTTP Completer and `harness` is the ACP adapter. Both
//! implement `Completer`. This module is the choice between them and the one
//! cancellable slot per Character Instance. The abandon flag lives with the
//! slot because the worker `Slots` starts is the thread the HTTP reader checks
//! between frames.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread;

use fidget_core::director::{
    self, Claim, Completer, Context, ModelDirector, Reply, Wake, WakeRequest,
};
use fidget_core::roster::InstanceId;

use crate::model::{blank, endpoint_from, tracing, DirectorSettings, Endpoint};

/// Whichever Completer this process has: the attached Harness for every
/// Instance, else an HTTP `Endpoint` per Instance.
/// An enum so `Endpoint`'s inherent methods keep their type.
pub enum AnyCompleter {
    // Boxed: an `Endpoint` carries the whole session and the agent, and the
    // Harness arm is one `Arc`, so the enum would otherwise be moved around at
    // the size of the larger arm.
    Http(Box<Endpoint>),
    Harness(Arc<crate::harness::Session>),
}

impl Completer for AnyCompleter {
    fn complete(&self, request: &WakeRequest) -> Result<Reply, String> {
        match self {
            AnyCompleter::Http(endpoint) => endpoint.complete(request),
            AnyCompleter::Harness(session) => session.complete(request),
        }
    }

    fn awaiting_user(&self, instance: &str) -> bool {
        match self {
            AnyCompleter::Http(endpoint) => endpoint.awaiting_user(instance),
            AnyCompleter::Harness(session) => session.awaiting_user(instance),
        }
    }
}

/// The Completer `configured` promises. Harness first: with one attached the
/// HTTP settings are not consulted at all.
pub fn completer_from(settings: &DirectorSettings) -> Option<AnyCompleter> {
    match crate::harness::attached() {
        Some(session) => Some(AnyCompleter::Harness(session)),
        None => endpoint_from(settings)
            .map(Box::new)
            .map(AnyCompleter::Http),
    }
}

thread_local! {
    /// The abandon flag for the model call on this thread.
    /// Thread-local, not an `Endpoint` field, because the socket lives in
    /// the worker stack. Cancellation is a Shell resource, not `Completer`.
    static ABANDONED: std::cell::RefCell<Option<Arc<AtomicBool>>> =
        const { std::cell::RefCell::new(None) };
}

/// Whether the call on this thread has been dropped by the frame loop.
/// False when unset, so the probe and the tests need no second path.
pub(crate) fn abandoned() -> bool {
    ABANDONED.with_borrow(|flag| {
        flag.as_ref()
            .is_some_and(|flag| flag.load(Ordering::SeqCst))
    })
}

/// One slot per Character Instance, in one registry.
/// Each HTTP session stays inside that Instance's `Endpoint`. There is no cap,
/// so N Instances make N calls.
#[derive(Default)]
pub struct Slots {
    slots: HashMap<InstanceId, Slot>,
}

struct Slot {
    /// Which call is this Instance's current one. A reply stamped with any
    /// other number was computed for a moment the Instance has left.
    epoch: u64,
    tx: Sender<Delivered>,
    rx: Receiver<Delivered>,
    /// Raised when the call is superseded. The worker reads it between SSE
    /// frames so an abandoned call closes its connection.
    abandoned: Arc<AtomicBool>,
    waiting: bool,
    /// Whether the call answers something the user did, which is the whole of
    /// what the Thinking ellipsis asks.
    reactive: bool,
}

/// What `wake` did with the call it was handed. A dropped wake started no
/// call, so the Shell must leave the caret and the turn bookkeeping alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Woke {
    Started,
    Dropped,
    /// Dropped because the call on the wire waits on the user's answer.
    AwaitingUser,
}

/// One worker's answer, stamped with the call it belongs to.
struct Delivered {
    epoch: u64,
    answered: Answered,
}

/// What one wake came back with.
/// The proposal is meaningless without the moment that asked for it. The near
/// miss is what tells an undeclared Behavior name from a model that talked.
pub struct Answered {
    pub wake: Wake,
    pub context: Context,
    /// The Behavior name the reply proposed that this Character declares none
    /// of. `None` on every other reply.
    pub near_miss: Option<String>,
    /// The cap ended this turn, so what was said is as far as the model got.
    /// The line the Chat surface remembers is marked with it.
    pub truncated: bool,
}

impl Default for Slot {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            epoch: 0,
            tx,
            rx,
            abandoned: Arc::new(AtomicBool::new(false)),
            waiting: false,
            reactive: false,
        }
    }
}

impl Slot {
    /// Stop treating this slot's in-flight call as this Instance's answer.
    /// The next call gets a fresh abandon flag. The old flag stays with the
    /// worker so it can still close the connection.
    fn supersede(&mut self) {
        self.abandoned.store(true, Ordering::SeqCst);
        self.abandoned = Arc::new(AtomicBool::new(false));
        self.epoch += 1;
        self.waiting = false;
        self.reactive = false;
    }
}

/// The trace line for a proposed Behavior name nobody declared.
/// Carries the declared set so `prowll` beside `prowl` reads as a typo, beside
/// `wave` as a model ignoring the contract. Workers interleave, so the Instance id leads.
fn near_miss_line(id: &str, name: &str, behaviors: &[String]) -> String {
    format!(
        "director: {id} {name} is no declared Behavior; declared: {}",
        behaviors.join(", ")
    )
}

impl Slots {
    pub fn new() -> Self {
        Self::default()
    }

    /// Tests let the Director build the prompt. The frame loop passes its own.
    #[cfg(test)]
    pub fn wake<C: Completer + Send + Sync + 'static>(
        &mut self,
        id: &InstanceId,
        director: Arc<ModelDirector<C>>,
        context: Context,
    ) -> Woke {
        self.wake_sending(id, director, context, None)
    }

    /// Append `line` to the Director's prompt and send it. Newest-wins, except
    /// where ADR-0016 keeps the wake on the wire: mid-answer, an ambient tick,
    /// or a Summon over a reply still generating. The return says whether it started.
    pub fn wake_with_prompt<C: Completer + Send + Sync + 'static>(
        &mut self,
        id: &InstanceId,
        director: Arc<ModelDirector<C>>,
        context: Context,
        line: String,
    ) -> Woke {
        self.wake_sending(id, director, context, Some(line))
    }

    fn wake_sending<C: Completer + Send + Sync + 'static>(
        &mut self,
        id: &InstanceId,
        director: Arc<ModelDirector<C>>,
        context: Context,
        line: Option<String>,
    ) -> Woke {
        let claim = director::claim(&context.happened);
        let reactive = claim != Claim::Ambient;
        let slot = self.slots.entry(id.clone()).or_default();
        if slot.waiting {
            // The user is mid-answer in Chat. A Poke, Throw, Grab, or Summon
            // is dropped, and so is every other wake. The answer is owed first.
            if director.awaiting_user() {
                return Woke::AwaitingUser;
            }
            // A reply the user is waiting for gives way to a touch of the
            // sprite or a typed line, not to opening Chat to read it or to a
            // muse. The Harness refuses a reactive turn to an ambient tick
            // anyway (`Session::supersede`), so superseding would drop an
            // answer still on its way.
            let takes_a_reply = match claim {
                Claim::Interaction | Claim::Line => true,
                Claim::Opener | Claim::Ambient => false,
            };
            if slot.reactive && !takes_a_reply {
                return Woke::Dropped;
            }
        }
        slot.supersede();
        slot.waiting = true;
        slot.reactive = reactive;
        let epoch = slot.epoch;
        let tx = slot.tx.clone();
        let abandoned = Arc::clone(&slot.abandoned);
        let traced = id.clone();
        thread::spawn(move || {
            ABANDONED.with_borrow_mut(|flag| *flag = Some(abandoned));
            // Always send. A panic here would leave the slot waiting forever
            // and skip StaticDirector on every later tick.
            let woken = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let woken = match line {
                    Some(line) => {
                        let mut request = director.request(&context);
                        if !request.prompt.ends_with('\n') {
                            request.prompt.push('\n');
                        }
                        request.prompt.push_str(&line);
                        director.wake_request(request)
                    }
                    None => director.wake_and_near_miss(&context),
                };
                // Traced here, beside the reply it came from. The Action Log
                // takes it from `take` instead, where a superseded reply has
                // already been dropped.
                if tracing() {
                    if let Some(name) = &woken.near_miss {
                        eprintln!("{}", near_miss_line(&traced, name, director.behaviors()));
                    }
                }
                woken
            }))
            .unwrap_or(director::Woken {
                wake: Wake::Failed,
                near_miss: None,
                truncated: false,
            });
            let _ = tx.send(Delivered {
                epoch,
                answered: Answered {
                    wake: woken.wake,
                    context,
                    near_miss: woken.near_miss,
                    truncated: woken.truncated,
                },
            });
        });
        Woke::Started
    }

    /// The reply for `id`, with the moment it was computed for.
    /// A superseded moment is dropped here rather than handed out for a
    /// caller to compare.
    pub fn take(&mut self, id: &InstanceId) -> Option<Answered> {
        let slot = self.slots.get_mut(id)?;
        while let Ok(delivered) = slot.rx.try_recv() {
            if delivered.epoch != slot.epoch {
                continue;
            }
            slot.waiting = false;
            slot.reactive = false;
            return Some(delivered.answered);
        }
        None
    }

    /// Drop whatever `id` has on the wire, and forget the Instance.
    /// Character switch, Completer retarget, and dismissal would apply the
    /// wrong character's answer. Remove the slot so the registry cannot accumulate.
    pub fn abandon(&mut self, id: &InstanceId) {
        if let Some(slot) = self.slots.remove(id) {
            slot.abandoned.store(true, Ordering::SeqCst);
        }
    }

    /// Whether `id` is waiting on the Director. Not a gate on `wake`. An
    /// observation, for the Static Director standing down while a session
    /// proposal is about to land.
    pub fn waiting(&self, id: &InstanceId) -> bool {
        self.slots.get(id).is_some_and(|slot| slot.waiting)
    }

    /// Whether what `id` is waiting on answers something the user did, which is
    /// the Thinking ellipsis's whole question: a proactive wake stays invisible.
    pub fn thinking(&self, id: &InstanceId) -> bool {
        self.slots
            .get(id)
            .is_some_and(|slot| slot.waiting && slot.reactive)
    }
}

/// Drop an in-flight wake and install a Completer for the new settings.
/// A wake still on the wire would propose against the old host and session.
/// `Slots::abandon` closes the connection so the old host stops generating.
pub fn retarget_model(
    slots: &mut Slots,
    id: &InstanceId,
    model: &mut Option<Arc<ModelDirector<AnyCompleter>>>,
    behaviors: impl IntoIterator<Item = impl Into<String>>,
    character: impl Into<String>,
    settings: &DirectorSettings,
    configured: bool,
) {
    slots.abandon(id);
    *model = configured.then(|| {
        Arc::new(ModelDirector::new(
            completer_from(settings).expect("configured means a Completer exists"),
            behaviors,
            id.clone(),
            character,
            blank(),
        ))
    });
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::model::tests::with_env;
    use crate::model::{config_from, resolve};
    use fidget_core::director::{
        Completer, Context, Happened, ModelDirector, Reply, Wake, WakeRequest,
    };
    use fidget_core::roster::InstanceId;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Duration;

    /// A Context to stand in for a wake already on the wire. `pub(crate)` for
    /// the `settings` tests, which retarget through the same call.
    pub(crate) fn wake_context() -> Context {
        use fidget_core::engine::State;
        use fidget_core::sensing::Activity;
        use std::time::UNIX_EPOCH;

        Context {
            activity: Activity {
                hour: 12,
                ..Activity::quiet()
            },
            ..fidget_core::director::tests::quiet_context()
        }
    }

    #[test]
    fn retarget_drops_an_in_flight_wake_and_installs_the_new_completer() {
        with_env(None, None, None, || {
            let settings = resolve("http://localhost:11434", "gemma4", None);
            let config = config_from(&settings);
            let mut slots = Slots::new();
            let id = "fidget".to_string();
            let saw = Arc::new(AtomicBool::new(false));
            slots.wake(
                &id,
                Arc::new(ModelDirector::new(
                    Watchful {
                        saw: Arc::clone(&saw),
                    },
                    ["stroll"],
                    id.clone(),
                    "cat",
                    false,
                )),
                wake_context(),
            );
            let mut model = None;

            retarget_model(
                &mut slots,
                &id,
                &mut model,
                ["stroll"],
                "cat",
                &settings,
                config.configured,
            );

            assert!(
                waited_for(&saw),
                "the old host must be told to stop generating"
            );
            thread::sleep(Duration::from_millis(50));
            assert!(
                slots.take(&id).is_none(),
                "a Wake computed against the old target cannot answer the new one"
            );
            assert!(model.is_some());
        });
    }

    #[test]
    fn retarget_to_a_remote_without_a_key_leaves_static() {
        with_env(None, None, None, || {
            let settings = resolve("https://api.openai.com", "gpt-4o-mini", None);
            let config = config_from(&settings);
            let mut slots = Slots::new();
            let mut model = None;
            retarget_model(
                &mut slots,
                &"fidget".to_string(),
                &mut model,
                ["stroll"],
                "cat",
                &settings,
                config.configured,
            );
            assert!(model.is_none());
        });
    }

    #[test]
    fn retarget_installs_when_configured_even_if_director_is_off() {
        with_env(None, None, None, || {
            let settings = resolve("http://localhost:11434", "gemma4", None);
            let mut config = config_from(&settings);
            config.enabled = false;
            assert!(config.configured, "local needs no key");
            let mut slots = Slots::new();
            let mut model = None;
            retarget_model(
                &mut slots,
                &"fidget".to_string(),
                &mut model,
                ["stroll"],
                "cat",
                &settings,
                config.configured,
            );
            assert!(
                model.is_some(),
                "Director off must still leave a Completer for ToggleDirector"
            );
        });
    }

    /// The declared set is the half of the line a reader needs. Without it a
    /// typo looks like a model ignoring the contract.
    #[test]
    fn a_near_miss_line_names_the_instance_and_what_was_declared() {
        let line = super::near_miss_line(
            "buddy-1",
            "prowll",
            &["prowl".to_string(), "wave".to_string()],
        );

        assert_eq!(
            line,
            "director: buddy-1 prowll is no declared Behavior; declared: prowl, wave"
        );
    }

    /// A switch must not apply the old Character's reply, and must be able to
    /// start the new opening before that POST returns.
    #[test]
    fn abandon_drops_a_wake_that_still_arrives() {
        let mut slots = Slots::new();
        let id = "fidget".to_string();
        slots.wake(&id, answering("stroll", 40), wake_context());
        assert!(slots.waiting(&id), "the call is in flight");

        slots.abandon(&id);
        assert!(
            !slots.waiting(&id),
            "an abandoned call must not hold the next Character Prompt back"
        );
        thread::sleep(Duration::from_millis(80));
        assert!(
            slots.take(&id).is_none(),
            "the abandoned Wake must not land on the new Character"
        );
    }

    /// The ellipsis is for a turn the user is waiting on. A proactive wake is
    /// nobody's question, and showing it would tell the user the fidget is busy
    /// with them when it is not.
    #[test]
    fn only_a_reactive_call_is_thinking() {
        let mut slots = Slots::new();
        let (ambient, poked) = ("ambient".to_string(), "poked".to_string());

        slots.wake(&ambient, answering("stroll", 200), wake_context());
        slots.wake(
            &poked,
            answering("stroll", 200),
            Context {
                happened: Happened::Poke,
                ..wake_context()
            },
        );

        assert!(slots.waiting(&ambient) && !slots.thinking(&ambient));
        assert!(slots.thinking(&poked));
    }

    /// Stands in for the SSE loop: checks between frames, without a server.
    /// Spins far longer than a test should need.
    struct Watchful {
        saw: Arc<AtomicBool>,
    }

    impl Completer for Watchful {
        fn complete(&self, _: &WakeRequest) -> Result<Reply, String> {
            for _ in 0..400 {
                if abandoned() {
                    self.saw.store(true, Ordering::SeqCst);
                    return Err("abandoned".to_string());
                }
                thread::sleep(Duration::from_millis(5));
            }
            Ok(Reply::whole("idle"))
        }
    }

    fn waited_for(flag: &AtomicBool) -> bool {
        for _ in 0..100 {
            if flag.load(Ordering::SeqCst) {
                return true;
            }
            thread::sleep(Duration::from_millis(10));
        }
        false
    }

    /// A Completer that answers with a fixed Behavior name after a delay, so
    /// a test can tell one call apart from the one that superseded it.
    struct Answers {
        behavior: &'static str,
        delay: Duration,
    }

    impl Completer for Answers {
        fn complete(&self, _: &WakeRequest) -> Result<Reply, String> {
            thread::sleep(self.delay);
            Ok(Reply::whole(self.behavior))
        }
    }

    fn answering(behavior: &'static str, delay_ms: u64) -> Arc<ModelDirector<Answers>> {
        Arc::new(ModelDirector::new(
            Answers {
                behavior,
                delay: Duration::from_millis(delay_ms),
            },
            ["stroll", "nap"],
            "fidget",
            "cat",
            false,
        ))
    }

    /// The worker builds the prompt, then appends the display line.
    #[test]
    fn a_finished_prompt_is_the_one_the_completer_receives() {
        struct Noted {
            prompt: Arc<Mutex<String>>,
        }

        impl Completer for Noted {
            fn complete(&self, request: &WakeRequest) -> Result<Reply, String> {
                *self.prompt.lock().expect("prompt lock") = request.prompt.clone();
                Ok(Reply::whole("idle"))
            }
        }

        let prompt = Arc::new(Mutex::new(String::new()));
        let id = "fidget".to_string();
        let director = Arc::new(ModelDirector::new(
            Noted {
                prompt: Arc::clone(&prompt),
            },
            ["stroll"],
            id.clone(),
            "cat",
            false,
        ));
        let mut slots = Slots::new();
        let line = "displays: 1; on display: 0; placement: (100, 200); frame: 1920x1080";

        slots.wake_with_prompt(&id, director, wake_context(), line.to_string());

        assert!(polled(&mut slots, &id).is_some(), "the wake answers");
        let got = prompt.lock().expect("prompt lock").clone();
        assert!(
            got.ends_with(line),
            "the display line has to ride on the prompt, got {got}"
        );
        assert!(
            got.contains("what just happened: time passed\n"),
            "the Director's prompt has to survive, got {got}"
        );
        assert!(
            got.contains("standing on: nothing\n"),
            "standing has to survive, got {got}"
        );
    }

    /// A registry with a call already out for `id`, long enough to still be
    /// there when the test acts. `pub(crate)` for the `settings` tests, which
    /// retarget through the same call.
    pub(crate) fn slots_awaiting_a_wake(id: &InstanceId) -> Slots {
        let mut slots = Slots::new();
        slots.wake(id, answering("stroll", 200), wake_context());
        slots
    }

    /// Poll the slot the way the frame loop does, until an answer lands.
    fn polled(slots: &mut Slots, id: &InstanceId) -> Option<Answered> {
        for _ in 0..200 {
            if let Some(taken) = slots.take(id) {
                return Some(taken);
            }
            thread::sleep(Duration::from_millis(5));
        }
        None
    }

    fn behavior_of(wake: &Wake) -> &str {
        match wake {
            Wake::Proposed(proposal) => &proposal.behavior,
            Wake::Failed => "failed",
        }
    }

    /// The responsiveness this registry exists for: a Poke arriving while an
    /// proactive wake is still out sends its own prompt at once, and the answer
    /// the user gets is the one to what they just did.
    #[test]
    fn a_new_wake_supersedes_the_one_the_instance_had_on_the_wire() {
        let mut slots = Slots::new();
        let id = "fidget".to_string();

        slots.wake(&id, answering("stroll", 120), wake_context());
        slots.wake(
            &id,
            answering("nap", 0),
            Context {
                happened: Happened::Poke,
                ..wake_context()
            },
        );

        let answered = polled(&mut slots, &id).expect("the newest call answers");
        assert_eq!(behavior_of(&answered.wake), "nap");

        thread::sleep(Duration::from_millis(200));
        assert!(
            slots.take(&id).is_none(),
            "the superseded reply must be dropped, not delivered a tick later"
        );
    }

    /// The other direction, which ADR-0016 turns around. A reactive call is an
    /// answer the user is waiting for and an ambient tick is the fidget musing.
    /// The muse is dropped rather than costing the user their answer.
    #[test]
    fn an_ambient_tick_does_not_supersede_a_reactive_call() {
        let mut slots = Slots::new();
        let id = "fidget".to_string();

        slots.wake(
            &id,
            answering("stroll", 120),
            Context {
                happened: Happened::Poke,
                ..wake_context()
            },
        );
        assert_eq!(
            slots.wake(&id, answering("nap", 0), wake_context()),
            Woke::Dropped
        );

        let answered = polled(&mut slots, &id).expect("the Poke's own call answers");
        assert_eq!(behavior_of(&answered.wake), "stroll");
        assert_eq!(answered.context.happened, Happened::Poke);
    }

    /// A name nobody declared arrives as speech, so the name is the only
    /// thing that tells the two apart. The Action Log is written at `take`,
    /// so the near-miss has to survive the trip.
    #[test]
    fn take_carries_the_near_miss_the_worker_saw() {
        let mut slots = Slots::new();
        let id = "fidget".to_string();

        // `answering` declares stroll and nap, so cartwheel is neither.
        slots.wake(&id, answering("cartwheel", 0), wake_context());

        let answered = polled(&mut slots, &id).expect("the call answers");
        assert_eq!(answered.near_miss.as_deref(), Some("cartwheel"));
        assert_eq!(
            behavior_of(&answered.wake),
            "",
            "a near miss is still played as speech"
        );
    }

    /// The Wake and its Context cannot be separated, so nothing downstream can
    /// read a proposal against a moment it was not computed for.
    #[test]
    fn take_hands_back_the_context_the_wake_was_computed_for() {
        let mut slots = Slots::new();
        let id = "fidget".to_string();
        let asked = Context {
            happened: Happened::Poke,
            standing: "Finder".to_string(),
            ..wake_context()
        };

        slots.wake(&id, answering("stroll", 0), asked);

        let carried = polled(&mut slots, &id).expect("the call answers").context;
        assert_eq!(carried.happened, Happened::Poke);
        assert_eq!(carried.standing, "Finder");
    }

    /// One registry, but the newest-wins latch is each Instance's own. Two
    /// fidgets poked at once are two conversations.
    #[test]
    fn one_instances_wake_leaves_anothers_slot_alone() {
        let mut slots = Slots::new();
        let (first, second) = ("first".to_string(), "second".to_string());

        slots.wake(&first, answering("stroll", 0), wake_context());
        slots.wake(&second, answering("nap", 0), wake_context());
        // Supersedes `first` only. `second` has said nothing about it.
        slots.wake(&first, answering("nap", 0), wake_context());

        let theirs = polled(&mut slots, &second).expect("the second fidget still answers");
        assert_eq!(behavior_of(&theirs.wake), "nap");
        let ours = polled(&mut slots, &first).expect("the first fidget answers too");
        assert_eq!(behavior_of(&ours.wake), "nap");
    }

    /// Superseding has to reach the worker, not just the epoch it answers on.
    /// Closing the connection is what stops a generation. The worker holds
    /// the socket, so a flag it never reads buys nothing.
    #[test]
    fn superseding_raises_the_flag_the_worker_reads() {
        let saw = Arc::new(AtomicBool::new(false));
        let mut slots = Slots::new();
        let id = "fidget".to_string();

        slots.wake(
            &id,
            Arc::new(ModelDirector::new(
                Watchful {
                    saw: Arc::clone(&saw),
                },
                ["stroll"],
                id.clone(),
                "cat",
                false,
            )),
            wake_context(),
        );
        slots.wake(&id, answering("nap", 0), wake_context());

        assert!(
            waited_for(&saw),
            "the superseded worker ran on without ever seeing that it had been dropped"
        );
    }
}
