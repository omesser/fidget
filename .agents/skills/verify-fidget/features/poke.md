# Poke

A single click on the sprite body makes the fidget react (react animation) and records a Poke verb; clicks on transparent pixels pass through to whatever is underneath.

## Sub-features

- `poke-hit` click on drawn pixels produces `verbs:.*Poke` in the trace log.
- `poke-miss` (macOS verify) cursor over transparent corner logs `HIT` or click-through status without hitting the sprite.
- `poke-resume` after the react animation (~600ms), the fidget returns to idle (Grounded) or sit (Perched); any interrupted walk does not resume (Director's fresh call determines the next motion).

## How to get to it (user POV)

- With the fidget visible on the desktop, click once on its body without dragging.
- Do not double-click (that is Summon).
- Do not start a drag (that is pick-up).

## Driving it with verify-overlay helpers

Preconditions:

- Overlay presence already proven in this session, or drive the platform script that includes poke.
- `FIDGET_TRACE_FRAMES=1` and `FIDGET_TRACE_HITTEST=1` (the verify scripts set these).
- Sprite has a known `pos()` in the log (feet); click slightly above the feet so the body is hit.

- **Linux X11.** Run `xvfb-run -a -s "-screen 0 1280x720x24" .agents/skills/verify-fidget/helpers/drive-overlay-x11.sh`. The script moves the pointer to `(sprite_x, sprite_y - 40)`, holds button 1 across a couple of polls, and asserts `verbs:.*Poke`. Exit `0` is proof. Evidence copied to `$FIDGET_VERIFY_EVIDENCE/overlay-presence/`.
- **macOS hit-test (HIT/miss).** Run `.agents/skills/verify-fidget/helpers/drive-overlay-macos.sh`. The hit-test section warps the cursor onto drawn pixels (`HIT`) and the transparent corner (`miss`). Evidence copied to `$FIDGET_VERIFY_EVIDENCE/overlay-presence/`.
- **macOS poke-hit (gesture verb).** Run `cargo run -p fidget-verify -- poke`. Real click, asserts `verbs:.*Poke`, writes evidence to `$FIDGET_VERIFY_EVIDENCE/poke/`.
- **Proof.** Require the `verbs:` line containing `Poke` from the app/trace log.

## Gotchas

- A click shorter than one ~16ms poll can miss `XQueryPointer`; hold the button (~120ms) as the X11 script does.
- Clicking the feet `pos()` may miss the art (feet are at the bottom); aim ~40px above on X11.
- Double-click is Summon, not Poke — wait between attempts.
- Headless boxes without `xterm`/`openbox` cannot reach this path via `verify-overlay-x11.sh`; prove units instead and record the unmet packages in Gotchas / PROOF.md — do not invent a parallel poke harness.
