# One Harness session is the Director, chat, and everything else

A second model for idle life is a second mind. The HTTP Completer that #11
ships is a stand-in until a Harness is attached. After that, Director
proposals, chat, and any later tool use share that Harness's conversation —
one session, not a cheap classifier plus Claude Code.

Static weights stay the Director when nothing is attached, when the Director
is off, and when a session call fails. They cost nothing, so they may stay
chatty. A session wake spends tokens, so it is not a heartbeat.

## Considered Options

- **Standalone inference API forever**: An OpenAI-compatible Completer for
  idle Behaviors, a Harness for chat. Cheap ambient life without installing
  an agent — and two voices, two memories of what just happened. Rejected.
- **Harness is the only model, including before attach**: No Completer at
  all. Correct end state; leaves #11 with nothing to measure against Static
  until #16 lands. The HTTP path stays as a disposable Completer behind the
  same `Director` trait.
- **Fixed interval (two minutes, or a settings slider)**: Predictable, and
  a bill for scratching. Rejected for any path that reaches a model.

## Wake policy

A session wake is either **reactive** — the user addressed the character (Poke,
Throw, picking it up, placing it on a Perch, Summon, a chat turn) — or
**proactive**, on an exponential backoff the Character Manifest names
(`model_base` and `model_power` under `[director]`), paused
while every display is asleep. The Engine and Static Director keep the
sprite moving between those calls. ADR-0004 still holds: the model is never
in the frame loop.

## Consequences

#16 does not grow a second Director. The attached Harness *is* the
Completer. Chat (#17) is another turn in the same conversation, not a
different prompt to a different endpoint.

That conversation is per Character Instance. Two Instances never share an
ACP session; a Character switch on one Instance is a different identity
and keeps the previous id so a switch back can load it. There is still
one Harness process and one ACP connection — no second mind. #558.

An attached handle stays the configured Completer once it exists, alive or
not. #469 asked the other way round: a Harness whose child never spawned, or
died mid-session, leaves `model::completer_from` handing the Director a handle
that answers nothing, so a key typed into the HTTP rows #452 keeps live is not
read until the next launch. Treating a dead handle as unconfigured would
recover faster and put the HTTP Completer behind one session's failure, which
is the second mind this decision refuses — and it would do it unasked, at the
moment the user is least able to tell which mind replied. So the failure still
goes to Static, and Settings says the wait rather than hiding it: the source
row's line names static weights as what is answering, and the three HTTP rows
say they are read at the next launch. Live recovery, if it is ever wanted,
belongs in re-opening the attachment (#469's third option), not in swapping
the mind out from under it.

Choosing Off in Settings is the user naming the HTTP Completer. That drop
happens in this process: the handle is shut down and Retarget installs the
HTTP Completer. It is not a dead session falling through unasked. Switching
to a different Harness does the same, in this process — the third option
above, taken up in #500. Aliveness is still not an input: a dead child is
retried, never traded for a second mind.

Reversing this means living with split-brain, or rewriting how #15–#17
talk to a model.
