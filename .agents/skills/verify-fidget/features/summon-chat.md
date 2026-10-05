# Summon / chat

Double-clicking the fidget opens its Chat surface: the same conversation that drives desktop Behavior, so answers show as bubble speech and as a Behavior, not only as text.

## Sub-features

- `summon-verb` double-click records `verbs:.*Summon` in the frame/trace log.
- `summon-chat-open` a Chat window for that fidget appears (when the session can show windows).
- `summon-status` the Chat status bar shows plain-language activity; Behavior/State values appear under Advanced.

## How to get to it (user POV)

- Double-click the sprite body. (Only this path emits the `verbs:.*Summon` trace; other paths open Chat without the verb.)
- Or choose the speech bubble's **Open chat** control when speech is truncated.
- Or select **Chat…** from the tray icon menu.

## Driving it with verify-overlay helpers

Preconditions:

- Overlay is running with `FIDGET_TRACE_FRAMES=1`.
- A real interactive display for Chat window proof; Xvfb can still prove the Summon verb.
- Doctor green for the lane.

- **macOS Summon (preferred).** Run `cargo run -p fidget-verify -- summon`. Real double-click, asserts `verbs:.*Summon`, writes evidence to `$FIDGET_VERIFY_EVIDENCE/summon/`.
- **Summon verb (X11 hand-rolled).** After overlay is up (or after a successful `drive-overlay-x11.sh` with `--keep`-style hold if you extend the helper), locate sprite feet `pos()` from the last `frame:` line. Click the body above the feet: `xdotool mousemove --sync $X $(($Y - 40))`, then `xdotool mousedown 1; sleep 0.12; xdotool mouseup 1; sleep 0.08; xdotool mousedown 1; sleep 0.12; xdotool mouseup 1`. Assert `grep -E 'verbs:.*Summon' "$TRACE_LOG"`. Copy the matching lines into `$FIDGET_VERIFY_EVIDENCE/summon-chat/`.
- **Chat window.** After the double-click, observe a Chat window belonging to the fidget. On X11 it is a `Fidget` class client titled with the Character's name (`xdotool search --name '^Buddy Bot$'` for the default Character) and its status bar reads `Idle` with an Advanced disclosure. Xvfb plus openbox is enough. Capture a screenshot with `FIDGET_CAPTURABLE=1` into evidence when the platform allows.
- **Harness without sprite.** Chat Completer wiring without the overlay: `FIDGET_HARNESS=<name> scripts/probe-harness.sh` (exit `0` = end_turn). This does **not** prove Summon UI; record it as harness-only if used.
- **Proof.** Require the Summon verb line for the gesture path. Treat Chat window visibility as a second observer when a GUI session exists.

## Gotchas

- Existing `verify-overlay*.` scripts prove Poke, not Summon — do not mark Summon verified solely because overlay-x11 passed.
- Double-click timing follows the OS interval; a slow second click becomes two Pokes.
- On X11, each click of the double-click must be held across a couple of ~16ms polls (~120ms); short `click --repeat 2 --delay 50` usually yields a single Poke with no Summon.
- Chat needs the Shell's windowing path; a crashed WebKit / missing display shows the verb without a usable Chat surface — report both observations.
- Under Xvfb with openbox both halves are provable on a debug build. Without a WM or WebKitGTK the Chat surface may be unprovable while the verb remains provable; say which half you proved.
- Close Chat through the WM (alt+F4 under openbox, or the title-bar close). `xdotool windowclose` destroys the X window behind GTK's back; on Settings it took the whole app down (`GdkWindow … unexpectedly destroyed`), so treat every fidget window the same way.
- A minimized Chat counts as closed for that Instance (no longer "chatting"); the window can still be on the taskbar — unminimize or Summon again to reopen (#1255).
