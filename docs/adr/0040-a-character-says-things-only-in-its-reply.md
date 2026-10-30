# A Character says things only in its reply

**Status:** Accepted.

## Context

A Harness turn could put words in the bubble two ways: the reply, parsed by the
`Behavior | line` contract, and an MCP `speak` tool that wrote a line straight
into an Instance's Dialogue at any time, from any session that loaded fidget's
MCP server. Nothing ordered the two. A line could show twice, a cancelled
turn's last `speak` could land after the newer turn's reply, the user's own
sessions could speak through the stdio shim outside any turn, and an agent that
said everything through the tool left a reply that did not parse (#1701).

## Decision

**The reply is the only channel for Dialogue. fidget's MCP server has no tool
for saying something.** Whatever the parser does not sort out of a reply is
Dialogue, and it streams to the Chat surface and the bubble alike. Thinking
prose before a `behavior:` line, known Harness startup banners, a reply that
only names a Behavior, and the Behavior line itself are sorted out; text after
the pipe is said.

A Character stays lively during long work through a line in its own
Personality Prompt (#1934), not through the system prompt or a tool.

"Speech" is reserved for a future audio speech engine. The text a Character
says is Dialogue.

## Consequences

Double lines, ordering between channels, late lines after a superseded turn
and lines from outside a turn cannot happen, because only one channel exists.
A superseded turn's reply is already dropped by its slot
([ADR-0016](./0016-one-cancellable-slot-per-instance.md)).

A Harness cannot interject mid-turn with a line of its own. A long turn says
nothing until its reply streams, and a Character that should chat along asks
for it in its prompt.

The tool that plays a Behavior stays, so a Harness can still move the sprite
during a turn.

Reversing this means a second channel and the rules between the two that #1701
listed.

## Considered Options

- **Keep `speak` and write the rules between the channels (#1701, #1910).** A
  Voice gate that accepts or declines each line, an `aside` flag on Chat rows,
  and ordering and pacing rules. Each rule exists only because there are two
  channels.
- **Keep `speak` and drop the reply's line.** The reply must still parse, so a
  turn that only called tools would fail as unparsed, and a Harness that never
  loads the MCP server could not say anything.
- **Keep the Character lively from the system prompt.** It changes every
  Character to fix a trait some of them want, which the Personality Prompt
  already expresses.
