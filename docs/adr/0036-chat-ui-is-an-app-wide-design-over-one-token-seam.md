# The Chat UI is an app-wide design over one token seam

**Supersedes:** [ADR-0019](https://github.com/omesser/fidget/blob/3d16d6fc5d9dc8222861a49c05ca68fc4b0053ce/docs/adr/0019-one-chat-ui-in-v1-switcher-in-v2.md) (removed).
ADR-0019 superseded [ADR-0013](https://github.com/omesser/fidget/blob/3d16d6fc5d9dc8222861a49c05ca68fc4b0053ce/docs/adr/0013-one-chat-ui-in-v1-switcher-in-v2.md) (removed).

## Context

A **Chat UI** is a named, swappable visual design for the Chat surface: palette,
type scale and shape. ADR-0019 shipped one design with the seam that makes
switching possible, and held the switcher for v2.

The switcher has shipped (#1068). Settings offers the three designs from the
#339 mockups. ADR-0019's seam and scope rules still bind, so they move here.

## Decision

The user picks one Chat UI for the whole app in Settings. It is an application
setting, not per Character or per Instance. Per-Instance chrome would make one
product look like two on one screen, to tell apart characters that already look
different.

The seam is class and token based. A root class names the design, and tokens
hold every colour, font family and radius. The Chat surface reads tokens only. A
literal outside a design's token block breaks the seam.

A Chat UI repaints the Chat surface and nothing the overlay draws. The Speech bubble shares only opaque panel tokens with the default
design. Changing the Chat UI never changes a Character's expression.

## Consequences

Adding a design means adding one token block. No part of the surface changes.

The overlay inherits from the default Chat UI once, by hand, and does not
follow the switcher.

Reversing this means writing the Chat surface's colours inline and giving up
the switcher.
