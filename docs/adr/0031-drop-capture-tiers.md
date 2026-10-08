# Free sensing only; Capture tiers are dropped

**Status:** Accepted, with two clauses amended by
[ADR-0032](./0032-one-consent-for-titles-and-application-names.md): free sensing
is no longer the tier needing no permissions, and Screen Recording permission
can be requested for that consent. The text below states the amended clauses.
Dropping the Capture tiers stands.

**Supersedes:** [ADR-0005](https://github.com/omesser/fidget/blob/3d16d6fc5d9dc8222861a49c05ca68fc4b0053ce/docs/adr/0005-sensing-posture.md) (removed).

## Context

The first sensing decision defined three tiers: Free (OS metadata), On-Demand
Capture (one screenshot with per-act consent), and Ambient Capture (periodic
sampling behind a mandatory on-device Local Gate). Only Free shipped. The
"ask the Harness for a screenshot after consent" hook was never wired, and ACP
has no standard desktop-control capability to wire it to.

Computer use belongs outside Fidget: in harness-native capabilities, or in an
MCP driver such as cua-driver that the user attaches.

## Decision

**Free sensing ships. Ambient Capture, On-Demand Capture, and the Local Gate are
dropped**, not deferred. Fidget never takes screenshots, never analyzes screen
pixels, and never embeds OCR or vision models for desktop awareness.

Free sensing is OS metadata: window geometry, stacking order, time, idle
duration and recent Behaviors. Window titles and application names are part of
it too, behind one consent (ADR-0032). The MCP window tools read metadata and
never touch pixels.

## Consequences

- The character keeps perch, fade, window sensing, idle detection, and the MCP
  window tools.
- The character never "looks at the screen" in the content sense. A pet that
  reacts to what a window shows would need Capture, which this drops.
- No Local Gate, because nothing produces frames for it to filter.
- Fidget owns consent for Free sensing only. It builds no Capture path. On macOS
  the titles consent requests Screen Recording to read titles, never to capture.
  The sprite no longer serves as a privacy indicator, because it never looks at
  pixels.
- Agents that need pixels or desktop control get them from the Harness or an
  MCP server the user chose, not from Fidget
  ([ADR-0003](./0003-no-executor-harness-owns-desktop-control.md)).

## Alternatives Considered

- **Defer Capture to v2:** "Coming soon" implies a commitment this retracts.
  Harness-native computer use already fills the role.
- **Capture in Fidget, proxied to the Harness:** Makes Fidget a screenshot relay
  with vision capability it does not need.
- **Keep the Local Gate without Capture:** It filters Capture frames. Without
  Capture it has no input.
