# Capturable by default, Presence opt-out

## Context

The overlay first excluded itself from screenshots and screen shares by default.
Only a dev flag and a Development-tab row turned that off.

That default was honest about meetings but dishonest about the screen. The user
sees the character, takes a screenshot, and gets a different picture. Hiding
from captures should be a choice the user makes, not the default.

## Decision

1. **Default is capturable:** With no setting, the character appears in
   screenshots and screen shares.
2. **Opt-out lives in Presence:** One checkbox, "Appear in screenshots and screen
   shares", checked by default. It is a user choice, not a Development row.
3. **Platforms that can exclude honour it:** macOS and Windows read the setting.
   Linux has no exclusion API and stays capturable.
4. **The environment override stays:** It forces either state for verification
   and wins over the stored setting.

## Consequences

- What the user sees is what a capture shows, unless they opt out.
- Meeting privacy is one clearly labelled setting.
- Stored settings keep their meaning. Only the default flipped.

## Alternatives Considered

- **Stay excluded by default, add an opt-in:** Keeps the WYSIWYG mismatch and
  makes the common case need action.
- **Three states: default, always visible, always hidden:** A binary choice does
  not need a third state.
- **Keep it Development-only:** Hides a privacy choice in a tab most users never
  open.
