# One consent gates window titles and application names

**Amends:** two clauses of [ADR-0031](./0031-drop-capture-tiers.md). Its
decision to drop the Capture tiers stands as written.

## Context

One walk over the window list reads a window's position, its owning
application, and its title. One consent gates the title. The owner comes back
either way, and so does the frontmost application's name, which a different
call supplies. Decline, and the character still knows Signal and Tor Browser are
running. What you turned off is knowing which window is which.

Nothing ever argued for that split. It arrived with the first sensing
decision's Free tier, ADR-0031 carried it forward, and the glossary and the spec restate it.
ADR-0031 contradicts itself as a result. It lists the frontmost application
name in a tier it defines as needing no permissions, and says Screen Recording
is never requested while the consent catalog requests it to read titles.

## Decision

**One consent gates window titles and application names alike.** Without it the
character reports neither. Window geometry, stacking order, time, idle duration and
recent Behaviors stay free.

The old split rests on content against category. A title is free text out of
whatever document is open and leaks what you are doing; an application name
comes from the bounded set of installed programs and leaks only that one of
them is running.

A bounded set is also what makes a name readable without interpretation.
`Signal`, `Tor Browser`, a health app named after the condition it tracks, a
competitor's product open during an acquisition. Knowing one of those is
running needs no work to act on, and can say more than a title in a text
editor. Titles are also what make desktop awareness useful, so the old split
gated the useful half and left the risky one free.

macOS returns the owner name with no prompt, so withholding it is this
project's choice, not the operating system's. #975 records the judgment.

### What this changes in ADR-0031

- Free sensing is no longer the tier that needs no permissions. Titles and
  application names need this one; nothing else in the tier does.
- "Screen Recording permission is never requested" is withdrawn, because
  enabling the titles consent requests that grant on macOS. What stands is that
  fidget builds no Capture path, so no screenshot, no pixel analysis, and no
  OCR or vision model for desktop content.

## Consequences

- **Without the consent the character knows less, which is what the row buys:**
  The Director loses the frontmost application name, a Perch is "a window"
  rather than "a Terminal window", `list_windows` and `describe_screen` report
  geometry alone, and the windows resource is empty.
- **An application name now costs a Screen Recording grant on macOS**, which
  the operating system would not have asked for. The settings row has to say
  what is actually read.
- **The excluded-applications list stops applying while the consent is off:**
  It matches on the owning application name, so with no name to match, an
  excluded application's window stays in `list_windows` as an unnamed
  rectangle. The name it hides is the one the consent already withheld. With
  the consent on it works as before.
- **The consent is renamed, and a rename must not silently revoke a grant:** If
  the stored key changes, ask the user again on purpose and say why.
- **#965 has one boundary to explain instead of two.**

## Alternatives Considered

- **Keep the split:** Content against category is why the code looks as it
  does, and the bounded-set counterexample above is what it fails on.
- **A second consent for application names:** It would match the two grants
  macOS needs and let a user decline titles while keeping names. Rejected
  because the permission pane stays readable only while it stays short
  (DESIGN.md decision 11).
- **Amend ADR-0031 in place:** `docs/agents/docs.md` makes an ADR immutable, so
  supersede rather than rewrite. An amendment would leave no record that free
  sensing once meant "no permissions".

## References

- ADR-0031: [Free sensing only; Capture tiers are dropped](./0031-drop-capture-tiers.md), two clauses superseded here
- ADR-0005: [Tiered sensing](https://github.com/omesser/fidget/blob/3d16d6fc5d9dc8222861a49c05ca68fc4b0053ce/docs/adr/0005-sensing-posture.md) (removed), where the Free tier's "no permissions" definition began
- #975 (the decision), #965 (the hint blocked on it), #888 (the settings row copy this revisits)
