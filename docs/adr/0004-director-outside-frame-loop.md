# The Director proposes Behaviors; the model never drives animation

The Character's idle life is driven by a local state machine playing declared
Behaviors. A model — the Director — wakes only occasionally and proposes a
Behavior for the engine to play. It is never in the frame loop and never
decides individual animation frames. Who that model is, and how rare
"occasionally" is, is [ADR-0008](./0008-one-harness-session.md).

## Considered Options

- **Prompt-at-authoring only**: The Personality Prompt produces a behavior config
  once; runtime is a pure local state machine. Zero cost, fully offline,
  deterministic — but the Character never surprises you. Retained as the
  configurable fallback when no model is reachable.
- **Model-in-the-loop**: The model continuously decides what the sprite does.
  Charming for exactly one demo video, then a battery, latency, and cost
  disaster: paying tokens for a cartoon to decide to scratch itself.

## Consequences

This is the structural decision the whole nostalgia layer rests on. It gives a
flat cost floor, instant reactions, and offline function — and critically, the
Character stays alive while the Functional Layer is thinking, which is precisely
where a naive design looks frozen and broken. It also makes the Personality
Prompt a real, testable artifact rather than a vibe.

Reversing this means rewriting the frame loop, the Behavior player, and the
Character Package schema together.
