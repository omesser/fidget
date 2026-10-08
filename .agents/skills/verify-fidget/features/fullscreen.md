# Fullscreen apps

When "Hide in fullscreen apps" is on (the default), a fullscreen app sends the fidget off its display. With another display free, the fidget teleports to the first free display and stays visible. When every display is taken, it fades out, and it fades back in once the fullscreen app leaves (#1345).

## Sub-features

- `fullscreen-fade` a fullscreen window on every display fades the fidget out (`presence: hidden over 500ms`), and closing it fades the fidget back in (`presence: shown over 500ms`).
- `fullscreen-move` with one display taken and another free, an Instance on the taken display teleports to the first free display (by index) instead of fading. An Instance in the user's hand stays put until released.
- `fullscreen-setting` Settings → Presence → "Hide in fullscreen apps" (help: "Steps aside for fullscreen apps.") turns both off.

## How to get to it (user POV)

- Make any app fullscreen on the fidget's display (for example `xterm -fullscreen` on X11, or a video player's fullscreen button).
- On several displays, make an app fullscreen on only the display the fidget is on.
- Settings → Presence → toggle "Hide in fullscreen apps".

## Driving it with verify helpers

Preconditions:

- Doctor green; X11 session with a WM (openbox under Xvfb is enough) for `fullscreen-fade`.
- Two or more displays for `fullscreen-move`.
- `FIDGET_TRACE_FRAMES=1` (`launch.sh` sets it). `FIDGET_DEBUG_IPC=1` if you want `fidget-verify snapshot` positions.

- **Fade on one display (X11)**: Under Xvfb with `openbox --replace &`, run `FIDGET_DEBUG_IPC=1 .agents/skills/verify-fidget/helpers/launch.sh`. Then `xterm -fullscreen -title fs-prop -e sleep 120 &`. Expect `xprop -id "$(xdotool search --name '^fs-prop$')" _NET_WM_STATE` to show `_NET_WM_STATE_FULLSCREEN`, and `presence: hidden over 500ms` in `$FIDGET_VERIFY_SCRATCH/app.log`. Kill the xterm. Expect `presence: shown over 500ms`.
- **Move to a free display**: Needs a session with two real displays (Windows, macOS, or a multi-monitor X11 desktop). Put the fidget on one display and make an app fullscreen there. `fidget-verify snapshot` (with `FIDGET_DEBUG_IPC=1`) shows the position on the other display, with no `presence: hidden`.
- **Setting row (X11)**: The Settings dump in [Capturable](./capturable.md) lists `check-box|Hide in fullscreen apps`, checked and enabled by default.
- **Proof**: Keep the `presence:` lines and the fullscreen window's `xprop`/`xwininfo` under `$FIDGET_VERIFY_EVIDENCE/fullscreen/`.

## Gotchas

- Fullscreen is judged per display from the frontmost window that covers it. A window that is merely large but leaves a gap does not count.
- Xvfb cannot give GDK two displays: `+xinerama` with two `-screen`s and `xrandr --setmonitor` both still log `overlay: 1 display(s)`. `fullscreen-move` is unreachable on a box; a single-display run only proves `fullscreen-fade`.
- Pure Wayland reports no windows, so fullscreen never triggers there (supported degrade, not a failure).
- A throw or walk toward a taken display may cross onto it and teleport again on the next tick. That is not a failure; a seam wall is #1350.
