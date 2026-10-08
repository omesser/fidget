# Fidget verification map

This directory is the maintained source for verifying user-facing behavior of the fidget desktop mascot. Read the index before driving the app, then use the matching feature file as the recipe.

## Baseline preconditions

- Work from the repository root (`Cargo.toml` workspace with `src-tauri/`).
- Set `RUN_ID` and evidence paths as in the skill (`/tmp/fidget-verify-$RUN_ID/evidence`).
- Prefer a debug binary this run built (`target/debug/fidget`). A release binary's stderr goes only to `process.log` in the data dir, so its terminal log has no `overlay:` or `frame:` lines (#1325).
- Run `.agents/skills/verify-fidget/helpers/doctor.sh` and require exit `0` before Drive.
- Never drive an instance that was not started by this verification run (or by the platform verify script it wraps).
- **One agent per display**: Two overlapping drives share Perches and hit-test state; refuse double-drive.

## Driving conventions

- Start every recipe from the baseline unless its preconditions say otherwise.
- Prefer log tokens and WM properties over pixel coordinates when asserting.
- Treat every command as literal. Keep quoted names and flags unchanged.
- Overlay proofs go through the `drive-overlay-*` helpers (wrappers around `scripts/verify-overlay*`).
- Unit proofs go through `prove-units.sh` / `doctor.sh --units`.
- Restore nothing that would delete `$FIDGET_VERIFY_EVIDENCE`.

## Proof and skip reporting

- Capture the user action and the resulting state, not only the final log line.
- Overlay proof includes the stamp dir under `.verify/` copied into evidence, plus exit code.
- Mutation / gesture proof includes the verb line (`Poke`, `Summon`) and a second observer (window props, Chat window, or frame state).
- Record the feature ID and entry point with every artifact.
- Report an unreachable path with the attempted command and the unmet precondition (exact package or env missing).
- Do not report a skipped entry point as verified through a different path.

## Feature entry contract

Each feature file starts with an H1 title and one paragraph describing the user-visible behavior. It then uses exactly four H2 sections in this order.

1. `Sub-features` lists short IDs with one line for each behavior.
2. `How to get to it (user POV)` lists every user entry point.
3. `Driving it with <harness>` starts with `Preconditions:` and uses labeled bullets that pair each user action with an exact command and observable result.
4. `Gotchas` lists traps that can waste or invalidate a verification run.

Keep implementation details out of the map. Name only user paths, stable handles, required state, commands, and observable proof.

## Features

- [Overlay presence](./overlay-presence.md) — display-sized overlay, EWMH / floating level, frame loop start.
- [Poke](./poke.md) — single click on the sprite body yields a react / Poke verb.
- [Summon / chat](./summon-chat.md) — double-click opens Chat for that fidget.
- [Capturable / hide from captures](./capturable.md) — appear-in-screenshots override via env and Settings.
- [Quick message](./quick-message.md) — hover composer pill above the sprite (send without opening Chat).
- [Fullscreen apps](./fullscreen.md) — move to a free display, or fade when every display is fullscreen.
