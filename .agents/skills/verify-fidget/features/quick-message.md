# Quick message

Hovering the sprite for about 1.5 seconds opens a composer pill above the Character so the user can type a short line without opening Chat. Sending the line talks to the fidget the same way Chat does; leaving both the sprite and the pill for three seconds hides an empty pill.

## Sub-features

- `qm-hover-show` after ~1.5s continuous hover on the sprite body, the pill appears (focused when an AI can answer).
- `qm-send` typing a non-empty line and activating Send (or Enter) delivers the line and hides the pill.
- `qm-auto-hide` after ~3s continuous away from both sprite and pill, an empty pill hides; re-entering either resets the timer.
- `qm-bubble-yield` while Speech or a thinking bubble is up, an empty pill the pointer left hides after ~1s instead of 3s (#1298).
- `qm-connect` when no AI can answer yet, the pill shows a connect prompt and an **Open chat** control that opens Chat (not a Summon verb).
- `qm-freeze` while the pill is visible (or the caret is in it), resting stroll / chase locomotion freezes for that Instance, and a climber holds on the wall (#1366).
- `qm-chat-suppress` while Chat is open for that Instance, hover does not open the pill; after Chat closes, leave the sprite and hover again to open it.

## How to get to it (user POV)

- Rest the pointer on the sprite body without clicking until the pill appears (~1.5s).
- Type a line and send, or click away / leave until auto-hide.
- When disconnected, use the pill's **Open chat** control to finish setup in Chat.

## Driving it with verify helpers

Preconditions:

- Doctor green for the lane you will use.
- Unit lane needs Node and `tests/quick-message*.test.js` (no display).
- Live hover needs an overlay session (X11 under Xvfb+WM is enough for the hover gesture; Chat send needs a configured AI).

- **Units (preferred proof on CI / this box)**: Run `node --test tests/quick-message.test.js tests/quick-message-connect.test.js` (also covered by `.agents/skills/verify-fidget/helpers/doctor.sh --units` / `prove-units.sh`). Exit `0` with all tests pass. Copy the tap summary into `$FIDGET_VERIFY_EVIDENCE/quick-message/` when proving alone.
- **Live hover (X11)**: Under Xvfb with `openbox --replace &`, start the debug binary with `.agents/skills/verify-fidget/helpers/launch.sh`. Read sprite feet `pos()` from the last `frame:` line, `xdotool mousemove --sync $X $(($Y - 40))`, sleep ≥1.5s. There is **no** `verbs:` line for show/hide. Proof is a screenshot: `import -window root full.png && convert full.png -crop 360x300+$(($X - 180))+$(($Y - 260)) +repage pill.png` (ImageMagick). With no AI configured the pill reads "No AI connected yet. Open chat". A 0.6s glance shows no pill; 4s away hides it again. Put artifacts under `$FIDGET_VERIFY_EVIDENCE/quick-message/`.
- **Chat-open suppress (X11)**: Double-click to Summon, move the pointer off the sprite, hover again for 2.8s: no pill. Close Chat with the WM (`xdotool windowactivate --sync $CHAT_ID key --clearmodifiers alt+F4` under openbox), leave, hover again: the pill returns.
- **Proof**: Prefer the unit suite exit code and tap summary. Treat live screenshots as a second observer when a capture tool exists.

## Gotchas

- A glance shorter than `HOVER_DELAY_MS` (1500) must not open the pill; do not click (that is Poke) or double-click (Summon).
- After a Summon (or any path that calls the pill's `summon` dismiss), the pill stays down while the pointer remains on the sprite; only `leaveSprite` clears that latch so a fresh hover can open it again (#1237).
- While Chat is open for that Instance (`sprite.chatting` / `setChatOpen(true)`), hover does not open the pill even after the Summon latch is cleared; close Chat, leave the sprite, then hover again (#1243).
- Auto-hide is 3000ms away from **both** sprite and pill; text in the field blocks auto-hide.
- Opening Chat from the connect control is `overlay_open_chat`, not `verbs:.*Summon` — do not mark Summon verified from this path.
- Live X11 screenshots need ImageMagick `import`. Without it, say so and rely on units rather than inventing a parallel harness.
