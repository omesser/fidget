# Director in-flight discipline, and making completions feel faster

Research prompted by the Director reacting slowly to Poke, Grab and Throw. Two
questions. Is at most one model call in flight enforced by design, and can a
new event cancel one already sent? And what makes a completion feel faster,
other than running a local model?

**Answer.** Single-in-flight is a convention, not a structure: one
`pending.ready()` term in a five-term condition at one call site, and the
invariant is per-Instance, so N characters make N concurrent calls. Cancellation
does not exist and cannot, because the call is a blocking non-streaming POST
on a thread nobody holds a handle to — and because vendors only honour an
abort on a streaming connection. A new event is coalesced into a latch and
deferred until the old reply lands, so a Poke can wait out a 20-second (hosted)
or 2-minute (local) timeout before its prompt is sent. A reply is then applied
with no staleness check, which is why the character can say "put me down" from the
floor it was already thrown onto. The cheapest fix is unrelated to all of it:
`Endpoint` built a use-once `ureq::Agent` per request and so paid a fresh
TCP and TLS handshake every wake. Streaming is the one change that buys
perceived latency, real cancellation, and a sprite that moves on the first
line. Prompt caching buys nearly nothing at this prompt size.

- **Anchored at `c3cb15f9`:** Every citation below — the symbol names and the line numbers alike — describes the tree at that commit, not `main`, and is verified against it. Citations name a symbol wherever the prose allows, because a name outlives a refactor and a line number does not. `docs/agents/docs.md` holds the convention.
- Three of the recommendations below have since shipped, so §1 and §2.6 are the furthest from `main`: `model::InFlight` is now `model::Slots` (item 2), `Endpoint` holds one pooled `ureq::Agent` (item 1), and the request streams (item 3).
- Date of the research half: **September 4, 2026**. Where a source looks dated, it is called out.
- Claims are marked **[vendor]**, **[community]** or **[inference]** throughout.

Vocabulary is `CONTEXT.md`: Director, Character Prompt, Completer, Harness, Character Instance, Behavior, Pace, Thinking ellipsis.

---

## Question 1 — Is single-in-flight enforced by design?

### 1.0 What the pieces actually are

The thing behind `live.pending` is `model::InFlight` (`src-tauri/src/model.rs`), and it is small enough to quote whole in the mind: an `mpsc` channel plus an `AtomicBool`.

```rust
/// One model call in flight. The frame loop starts it and polls `try_take`.
pub struct InFlight {
    tx: Sender<Wake>,
    rx: Receiver<Wake>,
    busy: Arc<AtomicBool>,
}
// ready() == !busy;  start() sets busy and spawns a thread;  try_take() clears busy
```

- `InFlight::ready` is `!busy`.
- `InFlight::start` sets `busy`, then `thread::spawn`s a plain OS thread that calls `ModelDirector::wake` and sends the `Wake` down the channel. The `catch_unwind` is there so a panic cannot leave `busy` stuck.
- `InFlight::try_take` is a non-blocking `try_recv` that clears `busy` on success.
- `InFlight::cancel` is `*self = Self::new()` — a fresh channel. Its own doc comment is honest about what that does and does not do: *"The worker still finishes; its Wake lands on a channel nobody reads. ureq cannot abort a POST already on the wire."*

The Completer is `model::Endpoint`, a **blocking** OpenAI-compatible HTTP client built on `ureq` 3.4 (the comment on the `ureq` dependency in `src-tauri/Cargo.toml` — *"Sync HTTP for the model Director. A worker thread posts; the frame loop does not wait. ureq instead of reqwest: no tokio."*). It carries the conversation itself in `session: Mutex<Vec<Message>>`, which is what makes ADR-0008's "one session" real and, as §1.1 shows, is also what makes a second concurrent call actively dangerous.

Per-Instance state lives in `main::InstanceState` (`src-tauri/src/main.rs`), one per character, each with its own `model` field, its own `pending: model::InFlight`, and its own `in_flight: Option<Context>`.

### 1.1 Is single-in-flight actually guaranteed?

**No. It is a convention, not a structure — and the invariant is per-Instance, not global.**

There is exactly one call site that starts a request: the `live.pending.start` inside `run_frame_loop`'s session-wake block (`frame_loop.rs:915`). It is guarded by a conjunction assembled by the caller:

```877:887:src-tauri/src/frame_loop.rs
                        if director::session_due(
                            live.addressed,
                            live.since_proactive,
                            &live.pace,
                            activity.displays_asleep,
                            instance.do_not_disturb(),
                            config.proactive_allowed,
                        ) && config.enabled
                            && live.pending.ready()
                            && !applied
                        {
```

`live.pending.ready()` is one `&&` in a five-term condition written by hand at the call site. Nothing in the type system stops a second caller from doing this:

```rust
live.pending.start(Arc::clone(model), context.clone()); // no check, compiles fine
live.pending.start(Arc::clone(model), context.clone()); // second thread, also fine
```

`InFlight::start` takes `&self`, not `&mut self`, so it does not even need unique access. Two `start` calls set `busy` twice and spawn two threads; both send into the same channel; `try_take` reads one `Wake` per tick and clears `busy` on the first, so the second reply is delivered on a later tick as a phantom proposal whose `live.in_flight` context has already been `take`n — and the context is taken with `.expect("a started call still has its context")`, which would **panic the frame loop**.

Worse, both threads would be inside `Endpoint::post` on the same `Endpoint`, mutating `session` under a `Mutex`. The push-snapshot-clone-then-pop-on-error dance is not atomic across the HTTP hop, so two concurrent calls would interleave user turns into one conversation and, on error, pop each other's messages. The session would silently corrupt.

So the current code is correct, and it is correct by inspection of one call site rather than by construction. It is a `pending.ready()` check that a future caller could forget or bypass. The one mitigating fact is that today there is exactly one caller and it is 40 lines long; the risk is entirely about #16 (Harness) and #17 (chat) adding a second and third path to the same session.

**Per-Instance, and yes, N characters means N concurrent requests.** Each `InstanceState` has its own `InFlight` and its own `Endpoint`, constructed independently in `spawn_instances` (and again on spawn in `spawn_live`). Nothing is shared. This is intentional and documented — the comment sitting right above it says so, and flags the gap:

```1164:1171:src-tauri/src/main.rs
            // One per Instance, because each character wakes on its own clock and
            // carries its own conversation.
            //
            // ponytail: N Instances with a key make N times the model calls, on
            // N independent `Pace` clocks and against no shared budget. Fine for
            // the handful a desktop holds; a budget the Instances draw from is
            // the upgrade, and it wants somewhere to show the spend, which is
            // #18's panel.
```

The per-Instance conversation is required by ADR-0008 (one session per character, so two characters are two minds, not one). The per-Instance *concurrency* is not required by anything — it is a side effect of the session being per-Instance. Those two can be separated, and §1.4 argues they should be.

One real consequence today: a Poke on character A and a Poke on character B in the same tick fire two simultaneous POSTs to the same endpoint. Both are reactive, both reset their `Pace`, and neither knows the other exists. With three or four characters on a slow endpoint that is three or four concurrent slots of somebody's rate limit spent on a cartoon deciding to scratch itself.

### 1.2 What happens today when a new event arrives mid-flight?

**Coalesced into a one-slot latch, then deferred until the in-flight call lands. Nothing is queued and nothing is cancelled.** Trace:

1. The pointer loop marks the verb. `frame_loop.rs:654-679` sets `live.addressed = true` and overwrites `live.happened` with a single value, priority-ordered within the tick: `Throw` > `Grab` (first held tick only) > `Poke`/`Menu` > `Summon`. Becoming Perched does the same through `became_perched` (`frame_loop.rs:860-866`), and the Engine's own `frame.addressed` re-latches just above it.
2. So `live.addressed` is a **latch bit** and `live.happened` is a **one-slot, last-write-wins register**. A Poke followed by a Throw during the same flight leaves `happened == Throw`; the Poke is gone. Three pokes are one poke. This is coalescing, and for the ambient case it is the right shape.
3. The session-wake block runs, `session_due` says yes, but `live.pending.ready()` is false, so the whole `if` is skipped. `live.addressed` and `live.happened` are **not** consumed — they are only cleared inside the taken branch (`frame_loop.rs:902-903`). The event therefore survives and fires on the first tick after the reply lands.
4. The wait is bounded by the Completer timeout, not by anything responsive: `model::TIMEOUT = 20s` for hosted and `model::LOCAL_TIMEOUT = 120s` for a local server, chosen by `timeout_for` and applied as `ureq`'s `timeout_global`. **A Poke arriving one millisecond after a proactive wake starts can wait up to 20 seconds (hosted) or 2 minutes (local) before its prompt is even sent.** That is the responsiveness bug, and it is squarely past Nielsen's 10-second attention limit (§2.7).

**Is the reply applied against a world that has moved on? Yes, and there is no staleness check.**

`live.pending.try_take` takes whatever arrived and `world.proposal` is assigned it for this tick unconditionally. The stored `live.in_flight` context is taken by that same `.expect("a started call still has its context")`, but it is used **only** to give `director::fallback` a `Context` for the Static fallback — never compared against the present moment. There is no epoch, no sequence number, no "was this computed for the state we are still in".

The Engine absorbs part of the damage but not all of it:

- **Motion is protected:** A proposal is advisory (`WorldSnapshot::proposal`, `crates/core/src/engine.rs`), and `permitted` requires `on_feet` — `Grounded` or `Perched` — for anything that moves. A reply computed for "picked up" that lands while the sprite is `Falling` has its Behavior refused, and the refusal interrupts nothing (the proposal arm of `Engine::tick`). `director::remember` is then fed only `frame.behavior` — what actually *played* — so the Static Director's suppression list stays honest.
- **Dialogue is not protected:** The frame's `dialogue` is read straight off `snapshot.proposal` with no reference to whether the Behavior played (`Frame::dialogue`, built in `Engine::tick`); only Do Not Disturb suppresses it. So this is live today: Grab → ambient/reactive wake sends `what just happened: picked up` → user throws the sprite, it flies, lands, settles → 15 seconds later the reply arrives and the character says *"hey, put me down!"* from the floor. The Behavior is refused; the line is spoken.

There is one adjacent place that already reasons about staleness correctly, and it is worth naming as prior art: `SettingsOp::Retarget` calls `model::retarget_model`, whose comment is *"Completer target changed, not Character. A Wake still on the wire would propose against the old host and session; drop it and open a new turn."* So the repo already has the concept "a reply from before an event is invalid" — it is just applied to settings changes and not to world events.

### 1.3 Is there any cancellation at all?

**No. There is discard, and the code says so in as many words.**

`InFlight::cancel` replaces the channel. The worker thread keeps running, keeps blocking in `ureq`, keeps holding the server's slot, completes the generation, and sends its `Wake` into a `Sender` whose `Receiver` has been dropped — the `let _ = tx.send(wake)` closing `InFlight::start` swallows the error. Both `InFlight::cancel` and `retarget_model` carry the same doc line: *"ureq cannot abort a POST already on the wire."*

Concretely, the Completer is:

- **not** a Tauri async task, **not** a future, **not** tokio-driven;
- a bare `std::thread::spawn` inside `InFlight::start` running a **blocking** `ureq` call in `Endpoint::post`;
- non-streaming: `send_json(body)` then `model::read_response` reads the whole body to a `String`, so the thread is parked inside a single blocking read for the entire generation.

Dropping anything on the shell side cannot reach that thread. `std::thread` has no cancellation, `ureq` has no per-request abort handle, and there is no `Drop` on the guard that could close the socket, because the socket is owned by the stack frame of a thread nobody holds a handle to. The `timeout_global` (20s / 120s) is the only thing that ever ends it early.

**What cancellation would actually require.** Two options, and the cheaper one is unusually cheap:

1. **Close the socket from the worker itself, between reads:** This needs the request to be *streaming* so there is more than one read to be between. `ureq` already supports this with no new dependency: `Body::into_reader()` returns a blocking `Read` (`ureq-3.4.0`, `src/body/mod.rs`). The worker reads SSE lines, checks a shared "is my epoch still current" atomic between lines, and on a stale epoch drops the reader — which closes the connection, which is exactly the documented way to stop a synchronous generation (§2.2). Latency to actually free the slot is one SSE chunk, tens of milliseconds.
2. **Move to an async client with an abort handle:** `reqwest` 0.13.4 and `tokio` 1.53.1 are *already in `Cargo.lock`* — `tokio` via `tauri` itself plus the `fidget-mcp-server` crate, `reqwest` via `tauri-plugin-updater` (`cargo tree -i`). So this is not "adding tokio where there is none" in the dependency-graph sense. It is still much the bigger ask: it means a second runtime concern in a shell whose whole model.rs preamble is *"ureq instead of reqwest: no tokio"*, and it buys nothing that option 1 does not, because dropping a `reqwest` future also just closes the connection.

Option 1 is the one to reach for. It costs no dependency and it makes cancellation and streaming (§2.1) the same change.

### 1.4 Where does the enforcement seam belong?

The trait today is uncancellable by signature:

```90:96:crates/core/src/director.rs
/// Completes a Character Prompt.
///
/// The attached Harness, once #16 lands. Until then, an HTTP stand-in in the
/// shell. Tests put a double here.
pub trait Completer {
    fn complete(&self, prompt: &str) -> Result<String, String>;
}
```

**Can single-in-flight be enforced at the `Completer` boundary? No, and it should not be tried there.** Three reasons:

1. `complete` is called *from inside the worker thread*, after the decision to spend a call has been made (`ModelDirector::wake`, reached via `InFlight::start`). By the time a `Completer` could refuse, the thread exists and the prompt is built. A guard there would be a guard in the wrong place.
2. `crates/core` deliberately does no I/O (the `director` module doc: *"This crate does not do I/O."*). Concurrency admission control is a property of the network resource, which core cannot see. Putting a semaphore or an epoch counter in a core trait would be core reasoning about a thing it has decided not to know about.
3. Cancellation is inherently about a resource that only the shell holds — the socket. Threading a cancellation token through `Completer` would mean core defining the vocabulary of an abort it can neither cause nor observe.

**The seam belongs to the shell, and it is where `InFlight` already sits — the module just needs to own the invariant instead of publishing the check.** The minimal interface change is to stop exposing `ready()` as something a caller consults and make "one slot, newest wins" the *only* reachable behaviour:

```rust
// src-tauri/src/model.rs — the shape, not the diff.

/// Every session call the app makes, across every Instance. One slot per
/// Instance; newest wins; the loser's connection is closed.
pub struct Slots { /* shared, owned by the frame loop */ }

/// A ticket the frame loop holds for one Instance's slot.
pub struct Slot<'a> { /* ... */ }

impl Slots {
    /// Send this prompt for `id`, abandoning whatever `id` had on the wire.
    ///
    /// Infallible: there is no "busy" to report, because starting *is* the
    /// cancellation of the previous call. Returns nothing — the reply arrives
    /// through `take`.
    pub fn wake<C: Completer + Send + Sync + 'static>(
        &self,
        id: &InstanceId,
        director: Arc<ModelDirector<C>>,
        context: Context,
    );

    /// The newest reply for `id`, if one landed, with the Context it was
    /// computed for. A reply from a superseded epoch is never returned.
    pub fn take(&self, id: &InstanceId) -> Option<(Wake, Context)>;

    /// Whether `id` has a call on the wire — for the Thinking ellipsis only.
    pub fn thinking(&self, id: &InstanceId) -> bool;
}
```

Four things this makes **impossible**, each of which is possible today:

- **Two calls in flight for one Instance:** `wake` supersedes; there is no code path that leaves two live epochs for one id, because the epoch counter is bumped inside `wake` under the slot's own lock.
- **Forgetting the check:** There is no check to forget. `ready()` disappears from the public surface. The current `if … && live.pending.ready() && !applied` shrinks to `if session_due(…) && config.enabled`.
- **Applying a reply against a moved-on world:** `take` returns the `Wake` *with* the `Context` it was computed for, so the caller cannot get one without the other. The `.expect("a started call still has its context")` — a real panic path if a second `start` ever slips in — stops existing, because the context is carried by the epoch rather than by a parallel `Option` field the caller must keep in step. That removes `InstanceState::in_flight` entirely.
- **A stale reply arriving at all:** A reply whose epoch has been superseded is dropped inside `take`, not by the caller remembering to compare.

**Should it be shared across Instances rather than per-Instance? Yes — one `Slots`, not N `InFlight`.** Sharing the *owner* is orthogonal to sharing the *session*: each Instance keeps its own `Endpoint` and its own conversation (ADR-0008 intact), while the thing that decides how many requests may be on the wire at once sees all of them. That is what makes a global cap expressible at all, and it is the natural home for the budget the `ponytail:` comment above the `InstanceState` literal says is the upgrade and for #18's spend panel. A per-Instance `Slots` cannot ever express "at most two characters talking at once" without a second, outer mechanism.

Note the deliberate asymmetry in the sketch: **per-Instance newest-wins, global concurrency cap.** They are different questions. "Should this character's old poke be abandoned for its new throw?" is always yes. "Should character B wait because character A is mid-call?" is a policy dial, and starting it at "no cap, N characters, N calls" preserves today's behaviour exactly while giving the cap somewhere to live.

### 1.5 Recommendation

**Do this, in this order.**

**(a) Fold `InFlight` into a shell-owned `Slots` with newest-wins and epoch-tagged replies.** This is the one change that converts a convention into a structure. It is small — `InFlight` is ~55 lines and `Slots` is not much more — it deletes state from `InstanceState` rather than adding it, it removes two terms from a five-term condition in the frame loop, and it removes a live `expect` panic path. This is the deep-module shape the repo prefers: one type, three methods, an invariant the caller cannot express its way around. **Cost:** one afternoon; touches `model.rs`, `frame_loop.rs`, `main.rs`; no new dependency; no core change; no ADR needed. *Note:* with cancellation absent (until (b)), "newest wins" means the loser's thread still runs to completion and its reply is discarded. That is strictly better than today — today the loser's reply is *applied* — but it does not yet reduce load, which is the stated motivation. So (a) alone is a correctness win, not a load win.

**(b) Make the request streaming, and cancel by dropping the reader.** This is the load win, and it is the same change as the biggest responsiveness win in §2, which is why it is worth doing as one piece of work rather than two. The worker reads SSE incrementally through `Body::into_reader()`, checks its epoch between chunks, and drops the reader when superseded — closing the connection, which the vendors document as the way to stop a synchronous generation and stop paying for output (§2.2). It also lets the sprite start moving on the first line (§2.1), which the repo's own reply protocol was already shaped for. **Cost:** an SSE line parser (~40 lines, `data: ` prefix, `[DONE]` sentinel, JSON delta extraction) plus a `stream: true` flag on the request body (`model::request_body`) and a non-streaming fallback for servers that refuse it. No new dependency. Local endpoints (llama.cpp, Ollama, LM Studio, vLLM) all speak the same SSE shape.

**(c) Only then consider a global concurrency cap.** With (a) in place this is a constant in one file. Leave it at "no cap" until #18 has a panel to show the spend on, exactly as that `ponytail:` comment argues.

**What to leave alone.** Do not add a staleness *check* at the apply site — the epoch in (a) makes the class of bug unreachable, and a comparison the caller performs is the defensive shape this codebase avoids. Do not queue events; the one-slot latch at `frame_loop.rs:654-679` is right, and a queue would let the character work through a backlog of pokes the user has forgotten making. Do not reach for `tokio`/`reqwest`; they are in the lock file but the whole point of the `ureq` comment in `src-tauri/Cargo.toml` is that this path stays synchronous, and (b) needs neither.

**ADR impact.**

- **ADR-0004** (Director outside the frame loop) is *reinforced*, not violated. `Slots` keeps every blocking call on a worker thread and the frame loop still only polls. Streaming does not put the model in the frame loop either: the worker accumulates and the loop still takes a finished `Wake` on a later tick. If (b) were ever extended to "apply the first line the instant it arrives, mid-generation", that would still be one `take` on one tick — a proposal delivered sooner, not a model consulted per frame. Worth a sentence in the ADR if it ships, not a reversal.
- **ADR-0008** (one Harness session) is *unaffected*. Sessions stay per-Instance inside each `Endpoint`; only the in-flight *slot* is centralised.
- **A new ADR is warranted for (a)+(b)**, because "the Shell owns one cancellable slot per Instance, and a superseded reply is never applied" is the kind of structural commitment the existing ADRs record, and because it fixes a user-visible behaviour (the character answering a question the world has moved past). The decision has not been taken, so no ADR is filed and no number is claimed here. Draft decision paragraph, for whoever takes it:

> ### Proposed: one cancellable slot per Instance, newest wins
>
> The Shell owns a single `Slots` registry holding at most one in-flight session call per Character Instance, and starting a call *is* the cancellation of that Instance's previous one. A reply is tagged with the epoch and the `Context` it was computed for and handed to the frame loop as one value, so a reply from a superseded moment cannot be applied — which is what stops a character saying "put me down" from the floor it was thrown onto. Requests stream, and abandoning one drops the response reader, closing the connection; vendors document that as the way to stop a synchronous generation, so cancellation frees the endpoint's capacity rather than merely discarding an answer. `Completer` stays synchronous and `crates/core` stays free of I/O: admission control and cancellation are properties of the socket, which only the Shell holds. Sessions remain per-Instance per ADR-0008; only the slot registry is shared, so that a global budget has somewhere to live once #18 has a panel to show it on. ADR-0004 still holds — the frame loop polls and never waits.

---

## Question 2 — Making completions feel faster, other than a local model

Excluding "run a local model", which is understood. Every external claim is cited. Claims are marked **[vendor]** for documented vendor behaviour, **[community]** for forum/blog reports, and **[inference]** for my own reasoning about this repo.

### 2.1 Streaming, and acting on the first line

**[vendor]** OpenAI's own latency guide ranks streaming first among ways to reduce *perceived* latency and is blunt about it: "Streaming: The single most effective approach, as it cuts the waiting time to a second or less." It also makes the sharper point that this is not purely psychological — "streaming & chunking genuinely do reduce overall latency once you consider the app + user system." (https://developers.openai.com/api/docs/guides/latency-optimization) The guide's seven principles are worth reading against this repo wholesale: process tokens faster, generate fewer tokens, use fewer input tokens, make fewer requests, parallelize, make it *feel* faster, and don't default to an LLM.

**[vendor]** Anthropic: `"stream": true` streams via SSE (https://platform.claude.com/docs/en/build-with-claude/streaming). Anthropic also documents that non-streaming is actively unsuitable for long generations — its SDKs "require streaming to avoid HTTP timeouts" at large `max_tokens`.

**[inference] This repo is unusually well set up to profit, because its wire protocol front-loads the actionable token.** `parse_proposal` (`crates/core/src/director.rs`) takes the Behavior name from the first line and treats everything after as optional dialogue; `character_prompt` instructs exactly that: *"Reply with the behavior name on the first line. An optional spoken line may follow on the next line."* Behavior names are single identifiers (`identifier`), so **the first line is typically 1–3 tokens.** Streaming therefore lets the sprite start moving after roughly TTFT + a couple of inter-token gaps, while the dialogue is still being generated — and the Speech bubble can be filled on a second tick when the line completes, which matches how `dialogue` is already modelled as a per-tick event rather than state (`Frame::dialogue`).

That is the difference between "sprite reacts in ~0.5 s and talks at ~1.5 s" and "sprite does nothing for 1.5 s". Given Nielsen's thresholds (§2.7), it is the difference between two bands.

**[inference] Cost.** An SSE parser in the worker, a `stream: true` flag in `request_body` (`src-tauri/src/model.rs`), and a fallback for endpoints that refuse it. No new dependency: `ureq`'s `Body::into_reader()` is a blocking `Read`. It also complicates `Endpoint::post`'s session bookkeeping slightly, since the assistant turn can only be appended once the stream completes — which is the same place a partial-stream cancellation must decide whether to keep or discard the partial turn. Worth being deliberate about: a cancelled stream should pop the user turn just as an error does today (the `session…pop()` on every error arm of `Endpoint::post`), or the session accumulates half-answers.

**[inference] Two-stage application is the more interesting half, and the riskier one.** Applying the Behavior on the first line and the dialogue later means the Engine sees two proposals from one wake. The current `Frame` already separates `behavior` from `dialogue`, so the shape exists — but it is a behavioural change to the Director contract and would want its own issue rather than riding along with the plumbing.

### 2.2 Does aborting actually stop billing and free capacity?

This is the load question, and the answer is **yes for streaming, no for non-streaming** — consistently across vendors and open-source servers.

**[vendor] OpenAI** states it plainly in the Background mode guide, in the notes under cancellation: **"To cancel a synchronous response, terminate the connection."** (https://developers.openai.com/api/docs/guides/background) The same guide gives a server-side primitive for the async case — `POST /v1/responses/{response_id}/cancel`, idempotent — but explicitly **only for responses created with `background: true`** (https://developers.openai.com/api/reference/resources/responses/methods/cancel). That endpoint is the wrong tool here: the same guide warns that "the time to first token you receive from a background response is higher than what you receive from a synchronous one", so buying cancellability with background mode would trade away the very latency this investigation is about.

**[community, consistent and load-bearing]** The mechanism's asymmetry is community-reported rather than spelled out in a vendor table: closing a **streaming** connection stops generation within a few tokens, while closing a **non-streaming** connection leaves the request running to completion server-side and billed. Both OpenAI forum threads say the same thing in the same words (https://community.openai.com/t/cancel-the-openai-api-request-without-deducting-the-cost-from-the-balance/719556, https://community.openai.com/t/cancel-openai-api-request/720603). OpenRouter documents it as product behaviour: "Streaming requests can be cancelled by aborting the connection. For supported providers, this immediately stops model processing and billing… For non-streaming requests or unsupported providers, the model will continue processing and you will be billed for the complete response." (https://openrouter.ai/docs/api_reference/streaming) **Input tokens are billed regardless** — cancelling saves output, never prefill (https://community.openai.com/t/cancel-openai-api-request/720603).

**[vendor] vLLM** is the clearest primary source that a disconnect frees real capacity, because you can read the code. Its serving layer wraps route handlers in a `with_cancellation` decorator that races the handler against an HTTP-disconnect listener and cancels the loser (https://docs.vllm.ai/en/latest/api/vllm/entrypoints/serve/utils/api_utils/), and the engine's documented pattern is to `engine.abort(request_id)` when `request.is_disconnected()` (https://docs.vllm.ai/en/v0.6.3.post1/_modules/vllm/engine/async_llm_engine.html). The v1 output processor produces a terminal `FinishReason.ABORT` and releases request state (https://github.com/vllm-project/vllm/blob/17d87168/vllm/v1/engine/output_processor.py). **[community]** The vLLM forum adds the caveat that a *running* request may only abort after the current execution step completes, so the release is prompt but not instantaneous (https://discuss.vllm.ai/t/how-is-vllm-handling-internal-queue-requests/2615). There is no server-side abort-by-id endpoint; closing the connection is the intended client-side cancel.

**[inference] Direct consequence for this repo.** Today's Completer is non-streaming (`Endpoint::post` sends with `send_json`, and `read_response` reads the whole body), so *no* cancellation can free anything — the request runs to completion no matter what the shell does. **Streaming is the prerequisite for cancellation, not an independent feature.** That is the single most important finding connecting the two halves of this report: §1.5(b) is one change that buys both the load control §1 asks for and the biggest responsiveness win §2 has to offer.

### 2.3 Prompt caching: does this design benefit? Mostly not, today.

**[vendor] OpenAI.** Caching is automatic, with a **strict 1,024-token minimum visible-input prefix** on GPT-5.6 and later (2,048 on older models); "For requests under 1,024 tokens, `cached_tokens` is zero" (https://developers.openai.com/api/docs/guides/prompt-caching). TTL is `prompt_cache_options.ttl`, whose **only supported value is `30m`**, refreshed on reuse. On 5.6+, cache **writes** are billed at 1.25× uncached input and implicit breakpoints land near the latest user message, so a stable prefix with a changing suffix can generate steady writes and zero reads unless you set `prompt_cache_breakpoint: { "mode": "explicit" }` on the last stable block. **[vendor]** Latency payoff scales with prefix size: OpenAI's own cookbook measured cached requests as only **~7% faster at 1,024 tokens**, rising to ~67% at 150k+, against a headline "up to 80%" (https://developers.openai.com/cookbook/examples/prompt_caching_201). **[vendor]** There is also a routing constraint that matters for a desktop app: cached states live on individual machines and "traffic above 15 requests per minute can lead to overflow routing", with `prompt_cache_key` available to influence grouping (same cookbook and guide).

**[vendor] Anthropic.** Opt-in via `cache_control`, up to 4 breakpoints, prefix covers `tools` → `system` → `messages` in that order. Minimum cacheable tokens **varies by model from 512 to 4,096** (https://docs.orq.ai/docs/ai-gateway/features/prompt-caching, mirroring https://platform.claude.com/docs/en/build-with-claude/prompt-caching). Default TTL **5 minutes**, refreshed free on each hit; **1 hour** available at 2× write. Writes 1.25× (5m) or 2× (1h); reads 0.1× (https://platform.claude.com/docs/en/about-claude/pricing). Two details are decisive for this repo:
- **[vendor]** "The lifetime is measured from the start of the request that writes or reads the cache entry, not from the end of its response."
- **[vendor]** On latency specifically, Anthropic is modest: "The 5-minute and 1-hour cache behave the same with respect to latency. You will generally see improved time-to-first-token **for long documents**." (emphasis mine)

**[inference] Three reasons this repo gets little from caching as designed.**

1. **The prompt is far below every cache floor:** `character_prompt` (`crates/core/src/director/prompt.rs`) is a personality line, a Behavior roster, four instruction lines, one voice-rules paragraph, and a six-line `follow_up`. That is on the order of **200–300 tokens** (my estimate from the template text, not measured) — under OpenAI's strict 1,024 and under even Anthropic's most permissive 512. And `follow_up` alone, which is what every wake after the first sends (`ModelDirector::prompt`), is ~60 tokens. The repo's stated virtue — "the Personality Prompt is not paid for again" (`character_prompt`'s doc comment) — is *why* there is nothing to cache: it optimised the prefix away.
2. **`Pace` outruns every TTL:** `Pace::FIRST` is 2 minutes and each proactive wake multiplies the wait up to a 2-hour cap (`director::Pace`). Anthropic's default cache dies after 5 minutes of inactivity; OpenAI's after 30. **[inference]** So by the third or fourth proactive wake the cache is guaranteed cold, and on Anthropic you would be paying the 1.25× write surcharge on nearly every call for a read that never comes.
3. **The one thing that *does* grow is the session.** `Endpoint` accumulates every user and assistant turn and re-sends the whole snapshot each call (`Endpoint::session`, pushed to and cloned at the top of `Endpoint::post`). **[inference]** After enough turns that history crosses 1,024 tokens and becomes a genuinely stable, genuinely cacheable prefix — the accidental beneficiary. This also means the *input* cost per wake grows without bound over a long session, which is a separate concern worth its own issue: nothing trims the conversation.

**[inference] Verdict.** Caching is a poor first move here. If it is pursued, the honest framing is "spend tokens to save latency": pad the opening turn past the floor with something genuinely useful (the full Behavior roster with descriptions, richer personality, memory excerpts), put an explicit breakpoint at its end, and accept that on Anthropic you need ≥1 read within 5 minutes per write to break even. Given `Pace`, that arithmetic only works for reactive bursts — a user poking a character repeatedly — not for ambient life.

**[vendor] One caching trick that *is* cheap and fits.** Anthropic documents **cache pre-warming** with `max_tokens: 0`: the API reads the prompt, writes the cache at each `cache_control` breakpoint, and returns immediately with empty `content` and `stop_reason: "max_tokens"` — explicitly "for latency-sensitive applications" to remove the first-call cache-miss penalty (https://platform.claude.com/docs/en/build-with-claude/prompt-caching). The documented gotchas: put the breakpoint on the last block **shared with the follow-up** (not on the placeholder user message), and match the thinking/effort config, or the warm entry is keyed to something real traffic never hits. **[inference]** For this repo the natural moment is app launch or Character switch — `spawn_instances` and the `model::retarget_model` path are both places where a new `Endpoint` is created and the first real wake is minutes away. But this only pays if the opening prefix is over the floor, i.e. only together with the padding decision above.

### 2.4 Speculative / predictive prefetching

**[vendor] OpenAI Predicted Outputs** is the vendor-supported form of this: pass a `prediction` string, the API verifies it in parallel via speculative decoding, and matched tokens are skipped ahead (https://developers.openai.com/api/docs/guides/predicted-outputs). Available on `gpt-4o`, `gpt-4o-mini`, and the `gpt-4.1` family — **note that model list looks dated as of September 2026** and does not include the GPT-5.x line the caching docs are written against, so treat availability as needing a live check. Hard limits: rejected tokens are **still billed at completion rates**, and `max_completion_tokens`, `tools`, `logprobs`, and `n > 1` are unsupported. **[community]** The lookahead appears to be K=16 tokens, reverse-engineered from accepted/rejected counts (https://vamvas.ch/openai-predicted-outputs). **[community]** Reported 3–5× on rewrite/code-edit workloads (https://www.morphllm.com/openai/predicted-outputs), which is a very different shape from this repo's two-line replies.

**[vendor]** OpenAI's latency guide separately endorses plain speculative *execution* — firing the likely branch early — and notes it is "particularly effective for classification steps where one outcome is more likely than the others" (https://developers.openai.com/api/docs/guides/latency-optimization).

**[inference] Not worth it here, on two counts.** Predicted Outputs accelerates *long* outputs that mostly match a known string; this repo's reply is a one-token Behavior name plus at most five short sentences (`character_prompt`'s voice rules), so there is almost no generation to skip. And speculative prefetch — firing a wake for "about to be poked" before the poke — is the opposite of load control: it *adds* load to a slow API to save a latency that streaming already addresses more cheaply, and it would fight `Pace`'s whole reason for existing (ADR-0008: "A session wake spends tokens, so it is not a heartbeat"). **[inference]** The one variant with a defensible cost is the pre-warm in §2.3, which spends a near-zero-output request rather than a speculative full one — and even that only under the padding precondition.

### 2.5 Model and parameter choices

**[vendor]** OpenAI's latency guide: "Generate fewer tokens" is one of the seven principles, and it is the one with the most direct leverage on end-to-end time, since total latency ≈ TTFT + (output tokens × per-token time). Its worked example shaves 19 output tokens and notes that on a big model "this could shave off up to a second" (https://developers.openai.com/api/docs/guides/latency-optimization).

**[vendor]** Anthropic on thinking models: `effort` should be lowered first for latency, `max_tokens` is "a strict limit" where effort is "soft guidance", and `thinking: {type: "disabled"}` turns it off where the model allows. `display: "omitted"` exists specifically so that "the server skips streaming thinking tokens entirely… so the final text response begins streaming sooner" — but "You're still charged for the full thinking tokens. Omitting reduces latency, not cost." (https://platform.claude.com/docs/en/build-with-claude/thinking)

**[community]** TTFT differs sharply by model tier — roughly 410 ms (Haiku 4.5), 720 ms (Sonnet 4.6), 980 ms (Opus 4.7) in one third-party benchmark (https://apimart.ai/blog/streaming-claude-api-key-features-explained). Treat the absolute numbers as indicative only; the ordering is the point.

**[vendor] Structured output has a real first-call cost.** OpenAI compiles each JSON schema to a grammar, and "the first request with a new schema will incur additional latency… Typical schemas take under 10 seconds to process on the first request, but more complex schemas may take up to a minute." (https://openai.com/index/introducing-structured-outputs-in-the-api/) **[community]** The compiled-artifact TTL is not documented; one forum report claims re-compilation after ~120 s of non-use, and others report a persistent 1–2 s overhead versus plain `json_object` (https://community.openai.com/t/structured-output-caching-and-latency/904483). **[community]** Bedrock documents a comparable compile step cached 24 h per account (https://fourtheorem.com/amazon-bedrock-structured-outputs/).

**[inference] What this says about this repo, which is mostly "you already did it".** The current settings are close to right and the reasoning is already in the comments:

- `model::HOSTED_MAX_TOKENS = 80` is a tight cap in the right spirit. `model::LOCAL_MAX_TOKENS = 512` is raised deliberately for local reasoning models, with the reason recorded in the doc comment above it.
- `"reasoning": { "effort": "low" }` on the xAI Responses path (the Responses arm of `request_body`) with the comment *"grok-4.6 defaults to high: 16s and hundreds of think tokens for a two-line Behavior pick"* is exactly Anthropic's documented advice, arrived at independently. **[inference] A reasoning model is the wrong tool for a Director wake**, and the codebase already concluded that at one endpoint; the finding is that this is only applied to xAI and there is no equivalent lever on `request_body`'s chat-completions arm.
- **No `stop` sequence is sent:** With the reply protocol being "name, then optional dialogue", `stop: ["\n\n"]` would cap the tail cheaply. **[inference]** Small win, and it interacts well with streaming: the client can stop reading once it has what it needs, which is the same drop-the-reader mechanism as cancellation.
- **Do not adopt structured output for this:** The 10 s–1 min first-call grammar compile is disqualifying for a path whose entire budget is a couple of seconds, and the existing line-based parser with its `spoken_or_failed` fallback already degrades gracefully in a way a strict schema would not. The `#231` case-insensitive matching (`ModelDirector::declared`) shows the parser is already tolerant of real model behaviour.
- **Temperature** is not sent at all. **[inference]** Leave it: the variety the Character wants comes from the "Vary" instruction (`character_prompt`'s voice rules) and the Static Director's suppression list, and lowering temperature for latency is not a documented lever on any vendor page I found.

### 2.6 Connection-level wins — the concrete, unglamorous finding

**This is the cheapest real latency on the table, and the current code gives it all away.**

**[vendor, verified against `ureq` 3.4.0 source]** `Endpoint::post` calls `ureq::post(url)` and `Endpoint::get` calls `ureq::get(url)`. `ureq`'s top-level verb functions are documented as **"Run on a use-once `Agent`"** and construct `Agent::new_with_defaults()` per call:

```623:633:~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/ureq-3.4.0/src/lib.rs
/// Make a POST request.
///
/// Run on a use-once [`Agent`].
#[must_use]
pub fn post<T>(uri: T) -> RequestBuilder<WithBody>
{
    RequestBuilder::<WithBody>::new(Agent::new_with_defaults(), Method::POST, uri)
}
```

The connection pool lives on the `Agent` (`ureq` exposes `max_idle_connections`, `max_idle_connections_per_host`, `max_idle_age` on `Config`, with `max_idle_age` documented around 15 s). A use-once Agent is discarded with its pool, so **every Director wake pays a fresh TCP handshake, a fresh TLS handshake, and no TLS session resumption.** On a transatlantic hop that is plausibly 100–300 ms of pure setup on a request the whole app is waiting for; I did not measure it against a real endpoint, so treat the magnitude as **[inference]**.

**The fix is to store one `ureq::Agent` on `Endpoint` and build requests from it** instead of from the free functions. That is a handful of lines, no new dependency, no behaviour change, and it makes the pool and TLS session survive between wakes.

**[vendor, measured just now]** All four endpoints the repo can target negotiate **HTTP/2** over ALPN. I probed them directly:

| Host | Negotiated | Status |
|---|---|---|
| `api.openai.com` | HTTP/2 | 401 |
| `api.anthropic.com` | HTTP/2 | 405 |
| `generativelanguage.googleapis.com` | HTTP/2 | 404 |
| `api.x.ai` | HTTP/2 | 401 |

(`curl -sI -w '%{http_version}'` against `/v1/models`; the status codes are just unauthenticated rejections and confirm the TLS+ALPN handshake completed.)

**[vendor, verified by absence in source]** `ureq` 3.4 is **HTTP/1.1 only** — its `Request` docs reference `HTTP_10` and `HTTP_11` and there is no h2 machinery in the crate. So the repo is on 1.1 against h2-capable servers. **[inference]** For a single small request that costs almost nothing: HTTP/2's wins are multiplexing and header compression, and this workload is one small request at a time with tiny headers. **Do not switch HTTP clients for HTTP/2.** The keep-alive win in the paragraph above is the part that actually matters, and it is available without leaving `ureq`.

**[inference] Region/endpoint selection** is not actionable here: the base URL is the user's (`model::BASE_URL`, carried into the `Endpoint` by `endpoint_from`), and nothing in the app should second-guess it.

### 2.7 De-duplicating across Instances, and latency budgets

**[inference] Batching across Instances is the wrong shape for this repo.** Each character has its own conversation by ADR-0008 and its own personality, so their prompts differ in the very first token — nothing to share. What *is* available is de-duplication of *timing*: `Slots` from §1.4 could stagger simultaneous wakes so two characters do not contend for the same endpoint. That is load smoothing, not latency reduction, and it makes one character slower to make the other faster. Not worth doing for latency; worth having the seam for, once #18 shows spend.

**[vendor] The interactive budget.** Nielsen's three thresholds are unchanged since 1993 and explicitly restated as current: **0.1 s** feels instantaneous and needs no feedback, **1 s** keeps the user's flow of thought, **10 s** is the limit of held attention, after which "users will want to perform other tasks while waiting" and will need to reorient on return (https://www.nngroup.com/articles/response-times-3-important-limits/, https://www.nngroup.com/articles/website-response-times/). **[community]** A practitioner target of TTFT < 500 ms at p95 for interactive LLM products, and the advice to watch p95/p99 rather than the mean, comes from a third-party analysis rather than a vendor (https://tianpan.co/blog/2026/03/10/llm-latency-decomposition-ttft-vs-throughput).

**[inference] Where this repo sits against those numbers.** The Cue is the 0.1 s layer and it is already right: `CONTEXT.md` specifies a procedural visual and a synthesized sound per interaction, drawn by the webview with no model involved (#277). The Director then aims at the 1–10 s band. Streaming to the first line plausibly lands a Poke reaction inside 1 s on a fast tier; today's non-streaming path with a 20 s timeout can exceed the 10 s attention limit outright, and the deferred-wake behaviour in §1.2 makes that *worse* by adding the remainder of an in-flight call before the new prompt is even sent.

**[inference] The `thinking` flag is already the documented masking mechanism, and its tuning matches the research.** The `thinking` local (`frame_loop.rs:952-958`) computes it as "a call is in flight, and that call is reactive (or this tick started a reactive one), and Do Not Disturb is off":

```952:958:src-tauri/src/frame_loop.rs
                let thinking = !live.pending.ready()
                    && (reactive_wake
                        || live
                            .in_flight
                            .as_ref()
                            .is_some_and(|ctx| ctx.happened != Happened::Proactive))
                    && !instance.do_not_disturb();
```

`CONTEXT.md`'s Thinking ellipsis entry gives it a 250 ms grace before appearing and a ≥600 ms hold once shown, with proactive wakes staying invisible — which is Nielsen's rule applied correctly in both directions: below ~250 ms no feedback is needed, and a spinner that flashes for 50 ms is worse than none. **Two findings about it:**

1. **[inference] Streaming changes what it should mean:** Today `thinking` is on for the whole call. With streaming and first-line application, the natural split is: ellipsis from send until the Behavior name arrives, then the sprite is already moving and the ellipsis should give way to the `talk` Animation while the line streams in. The flag's condition would key off "no first line yet" rather than "call in flight". That is a genuine improvement in what the user is told, not just a faster version of the same thing.
2. **[inference] The condition is a symptom of the design gap in §1:** It reaches into `live.in_flight` and inspects `ctx.happened` to reconstruct whether the current call was reactive — information the slot could simply carry. Under the `Slots` sketch this becomes `slots.thinking(&id) && !instance.do_not_disturb()`, with reactive-ness stored alongside the epoch. Worth noting because it is evidence the current two-field (`pending` + `in_flight`) split leaks: three separate places (`frame_loop.rs:769`, `frame_loop.rs:939`, `frame_loop.rs:952-958`) each reassemble state that one slot could hold.

---

## Prioritised recommendations

Highest value per unit of cost first.

1. **Hold one `ureq::Agent` on `Endpoint` instead of calling `ureq::post`/`ureq::get`:** *Done — #303.* Removes a full TCP+TLS handshake from every wake. ~10 lines, no dependency, no behaviour change, no ADR. Verified cause: `ureq`'s free verb functions are "use-once Agent" (`Endpoint::get`, `Endpoint::post`; `ureq::get` and `ureq::post` in `ureq-3.4.0`). Measured against a loopback server counting inbound connections: six requests opened six connections before, one after.

2. **Fold `InFlight` into a shell-owned `Slots`: one slot per Instance, newest wins, replies epoch-tagged and returned with their `Context`:** Converts single-in-flight from a checked convention into an unreachable-otherwise structure, kills the "put me down" stale-dialogue bug (`Frame::dialogue`), removes `InstanceState::in_flight` and a live `expect` panic path, and shrinks the five-term wake condition to three. One afternoon; `model.rs` + `frame_loop.rs` + `main.rs`; no dependency; no core change. **Wants a new ADR** (draft in §1.5).

3. **Make the request streaming and cancel by dropping the reader:** *Filed as #302.* The single highest-value item, and deliberately third only because (2) gives it the epoch to check against. Buys three things at once: OpenAI's own "single most effective approach" to perceived latency; *actual* cancellation that frees the endpoint — non-streaming requests cannot be cancelled at all, per the OpenAI Background-mode guide's "To cancel a synchronous response, terminate the connection"; and the sprite moving on the first line, which this repo's reply protocol was already shaped for. Cost: ~40-line SSE parser, a `stream: true` flag, a non-streaming fallback, and care with the session bookkeeping on a cancelled partial. **No new dependency** — `ureq`'s `Body::into_reader()` is a blocking `Read`.

4. **Send a `stop` sequence (`["\n\n"]`) and extend the low-reasoning-effort lever beyond the xAI branch:** Caps the tail on a reply that only ever needs two lines. The reasoning-effort case is already argued in the repo's own comment in the Responses arm of `request_body`; it is simply not applied on its chat-completions arm. Handful of lines, but per-endpoint compatibility testing is the real cost, since a strict server rejects an unknown field outright — a hazard the repo has already been bitten by (the `LOCAL_MAX_TOKENS` doc comment).

5. **Re-key the Thinking ellipsis off "no first line yet" once streaming lands:** Turns the existing 250 ms-grace / 600 ms-hold flag from "the model is busy" into "the character has not decided yet", so the sprite's own Animation takes over the instant it starts moving. Small, and it is the part the user actually perceives. Do it with (3), not before.

6. **Guard the growing session:** `Endpoint::session` accumulates every turn and re-sends the lot, cloned at the top of `Endpoint::post`, with nothing trimming it. Input cost and prefill time per wake grow without bound over a long run. Not strictly a latency *fix* — it is a latency *regression* that gets worse the longer the app is open. Deserves its own issue; the fix (a turn cap, mirroring `director::REMEMBERED`) is small, but "which turns are safe to drop" is a Director-behaviour question, not a plumbing one.

7. **Consider a global concurrency cap in `Slots` — but not yet:** Free to add once (2) exists, and that `ponytail:` comment already argues the right sequencing: it wants somewhere to show the spend, which is #18's panel. Leave the default at today's behaviour (no cap) so (2) is a pure refactor.

8. **Prompt caching: probably skip; if pursued, do it deliberately as "spend tokens to save latency":** The current prompt is ~200–300 tokens (estimated), below OpenAI's strict 1,024-token floor and below even Anthropic's most permissive 512, and `Pace`'s 2-minute-to-2-hour backoff (`director::Pace`) outruns Anthropic's 5-minute and OpenAI's 30-minute TTLs. OpenAI's own measurement puts the payoff at ~7% at the 1,024-token floor. Getting real benefit means padding the opening past the floor, adding an explicit breakpoint, and pairing it with Anthropic's `max_tokens: 0` pre-warm at launch — a coherent plan, but one that spends tokens against ADR-0008's grain for a single-digit-percent TTFT gain. The one thing that *does* accidentally benefit is the growing session in item 6, which cuts the other way.

9. **Do not do: Predicted Outputs, structured output, an HTTP/2 client switch, speculative prefetch, or cross-Instance batching:** Predicted Outputs accelerates long known-in-advance outputs and bills rejected tokens; this reply is a couple of lines. Structured output's 10 s–1 min first-call grammar compile is disqualifying, and the existing tolerant line parser (`parse_proposal` and `spoken_or_failed`) is better suited. HTTP/2 would mean leaving `ureq` for a multiplexing win this one-request-at-a-time workload cannot use — the keep-alive half (item 1) is the part that pays and needs no such move. Speculative prefetch adds load to a slow API to hide a latency streaming already hides, against ADR-0008's "a session wake is not a heartbeat". Cross-Instance batching has nothing to batch, since per-Instance conversations diverge at the first token by design.
