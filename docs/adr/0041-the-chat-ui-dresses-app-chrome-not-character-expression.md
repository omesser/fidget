# The Chat UI dresses app chrome, not Character expression

**Supersedes:** [ADR-0036](https://github.com/omesser/fidget/blob/5b513c99db57f38eb81bf4288c30cc1bf717be45/docs/adr/0036-chat-ui-is-an-app-wide-design-over-one-token-seam.md) (removed).

**Status:** Accepted.

## Context

A **Chat UI** is a named, swappable visual design: palette, type scale and
shape. The user picks one in Settings, and Chat and Settings both wear it.

ADR-0036 kept the Chat UI off everything the overlay draws. The overlay copied
the default design once, by hand, and didn't follow the switcher. That rule
was meant to protect the Speech bubble: a Chat UI must never change a
Character's expression.

The overlay draws more than expression, though. The Possession card is app
chrome. It explains keys and offers buttons, the same job a Settings row does.
Under that rule it stayed a fixed dark panel with its own buttons, while Chat
and Settings repainted for Terminal, Glass and Light. To the user, one app
looked like two.

## Decision

The user picks one Chat UI for the whole app. It is an application setting,
not per Character or per Instance.

The seam is class and token based. A class names the design, and tokens hold
every colour, font family and radius. A surface reads tokens only. A literal
outside a design's token block breaks the seam.

The line is drawn by what a surface is, not by which window draws it:

- **App chrome wears the Chat UI and Light/Dark:** Chat, Settings, and the
  overlay's Possession card. A switch in Settings repaints it while it is
  open.
- **Character expression does not:** the Speech bubble, the quick-message
  pill, and the gesture cues. They share only the opaque `--bubble-*` tokens
  with the default design. Changing the Chat UI never changes a Character's
  expression.

## Considered Options

- **Keep ADR-0036 and restyle the card by hand to the default design:** The
  smallest change. Rejected: the card would still stay dark under Terminal,
  Glass and Light, and every later design would need its own hand copy.
- **Load the whole Chat stylesheet in the overlay:** Rejected: it styles the
  page body and names rows the overlay doesn't have, and the overlay would
  only stay transparent by winning every one of those rules.
- **Theme the whole overlay, the Speech bubble included:** Rejected for the
  same reason ADR-0036 gave. Per-design speech would change a Character's
  expression.

## Consequences

Adding a design still means adding one token block. Chat, Settings and the
card pick it up together. The tokens are `--ui-*` in `src/ui-tokens.css`,
named for the app they dress rather than for Chat.

Glass is the one design whose panel depends on the window. Chat on macOS
sits on a native blur of the desktop and wears Glass's frosted panel. The
overlay is a transparent window that nothing blurs, so its chrome always
takes the unblurred tint, the same token Chat takes on Windows. A frosted
panel over a sharp desktop would leave the card's text on whatever is
behind it.

A new piece of overlay chrome takes the Chat UI by default. Overlay
expression stays on the bubble tokens, and a reviewer has to decide which
of the two a new surface is.

Reversing this means hand-copying a design into the card again.
