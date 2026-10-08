# The Shell owns one cancellable slot per Instance, and a superseded reply is never applied

**Status:** Accepted.

**Supersedes:** [ADR-0033](https://github.com/omesser/fidget/blob/15f839ed0a11c90c8510fec5c36fc5cea05d2799/docs/adr/0033-a-call-waiting-on-the-user-is-not-superseded.md) (removed).

One `Slots` registry holds at most one session call per Character Instance, and
starting a call *is* the cancellation of that Instance's previous one, with the
two exceptions below. `wake` either starts a call or drops the wake. Neither is
an error, so there is no busy to report and no check a caller can forget. The
reply comes back through `take` as the `Wake` and the `Context` it was computed
for, together, and a reply from a superseded moment is dropped inside `take`
rather than compared against the present by whoever applies it.

The bug that argues for it is one the user can feel. Grab the sprite, a wake
goes out saying `what just happened: picked up`, throw it, it flies and lands —
and fifteen seconds later the character says "hey, put me down!" from the floor. The
Engine already refuses a Behavior that no longer fits, because `permitted`
requires `on_feet`; Speech had no such guard, and a Poke arriving a millisecond
after a proactive wake waited out the whole of `timeout_global` — 20s hosted,
120s on a local server — before its prompt was even sent.

## A call waiting on the user is not superseded

The Harness asks the user questions through `session/request_permission` and
`elicitation/create`, and since #1001 the turn waits for the answer with no
timeout. Replacing that call lost the answer after a permission ask (#1037),
and a Poke during an ask cancelled the turn the user was answering (#1038).

`Slots::wake` alone decides whether a new wake replaces the call on the wire.
`director::claim` puts every `Happened` in one of four classes with an
exhaustive match, so a new event does not compile until someone classes it.
Poke, Throw, Grab, and Perch are Interactions. Summon is an Opener. Chat is a
Line. An ambient tick is Ambient.

- While the call waits on the user's answer, every new wake is dropped.
- While a reactive call is still generating, an Opener or Ambient wake is
  dropped. An Interaction or a Line replaces the call.
- Otherwise the newest wake wins.

A dropped wake is not queued. `Completer::awaiting_user` reports the wait. It
defaults to false, because an HTTP endpoint cannot ask. On the HTTP lane an
ambient tick used to cancel the reactive call, and the user got the tick's
reply instead of the answer to their Poke.

## Considered Options

- **Keep the convention and check it at the call site**: One `ready()` term in a
  five-term condition, which is what shipped until #312. Correct by inspection
  of one caller, and correct only for as long as there is one caller: #16's
  Harness and #17's chat are the second and the third.
- **Queue the events instead of superseding them**: The character then works through
  a backlog of Pokes the user has forgotten making. Coalescing into a one-slot
  latch is the right shape for a mascot; a queue is the right shape for a job
  runner. The same holds for a dropped wake.
- **Compare the reply's `Context` against the present at the apply site**: The
  defensive shape this codebase avoids. A check the caller must remember is the
  thing being removed, not a smaller version of it.
- **Enforce it at the `Completer` seam**: `complete` is already inside the
  worker thread, so a refusal there arrives after the thread exists and the
  prompt is built. And `crates/core` does no I/O: admission control and
  cancellation are properties of a socket only the Shell holds.
- **Exempt only a call waiting on the user**: That fixes #1038 but not #1037,
  because an ambient tick would still replace a slow reactive turn.
- **Let a Summon replace a reply still generating**: The operator rejected
  this. A Summon opens Chat to read that reply, so cancelling it defeats the
  Summon.
- **Put a time limit on the wait**: A setting this decision avoids.
  `PendingAsks` shows an open question in any Chat window, and opens one if
  none is showing.

## Per-Instance newest-wins, global concurrency cap

Two different questions, deliberately answered in different places. "Should this
character's old Poke be abandoned for its new Throw?" is always yes, and the slot
settles it with no dial. "Should character B wait because character A is mid-call?" is a
policy, and the registry is shared across Instances so that it has somewhere to
be expressed at all — N separate slots could not express it without a second,
outer mechanism. It starts at no cap, which is exactly what N independent
in-flight calls already did, and waits for #18 to have a panel to show the spend
on.

Only the slot is centralised. Sessions stay per-Instance inside each `Endpoint`,
so [ADR-0008](0008-one-harness-session.md) is untouched, and
[ADR-0004](0004-director-outside-frame-loop.md) is reinforced: every blocking
call is still on a worker thread and the frame loop still only polls.
ADR-0008 is also why an open question belongs to one Instance.

## Consequences

Abandoning a call stops costing the endpoint rather than merely going unheard.
Superseding raises the flag #302 put behind the streaming reader, so the loser
closes its connection between SSE frames and the host stops generating.

Two calls can now overlap on one `Endpoint`, which the session bookkeeping had
been relying on not happening. A turn carries a number, because the position of
the last message no longer says whose question it is: a question nobody answered
is withdrawn by the next `open_turn`, and a turn some later one has replaced
closes without touching the session at all.

An unanswered question holds the slot. Every new wake for that Instance is
dropped until the user answers or rejects it. A dropped Poke, Throw, Grab, or
Perch shows a bubble that reads "Question for you in the chat", and "chat"
opens Chat, where `PendingAsks` shows the question. An ambient tick shows
nothing, and neither does a dropped Summon or typed line, whose Chat is open.
A dropped Summon still opens Chat, because the slot does not open windows.

`wake` returns `Woke::Started`, `Woke::Dropped`, or `Woke::AwaitingUser`,
which is a drop because the user owes an answer. The Shell updates the caret,
`chat_turn`, and `happened_last` only on `Started`, so a dropped wake leaves
the running turn's caret in place. An ambient tick during a reactive turn is
skipped. The pace still advances, so the ambient cadence does not change.

Reversing this means going back to a convention checked by hand at every call
site, and to a character that answers a question the world has moved past.
