# Overlay presence

The fidget draws on a display-sized always-on-top overlay that skips the taskbar, loads a Character Package, and runs a frame loop (Falling / Grounded / Perched) the user can see as the sprite on the desktop.

## Sub-features

- `overlay-window` publishes one large overlay window per display (not the GDK 10×10 placeholder).
- `overlay-ewmh` (X11) sets `_NET_WM_STATE_ABOVE`, `_NET_WM_STATE_SKIP_TASKBAR`, and `_NET_WM_STATE_SKIP_PAGER`.
- `overlay-frames` emits `frame:` lines when `FIDGET_TRACE_FRAMES=1` is set.
- `overlay-perch` lands on a real window top edge when a Perch exists before launch.
- `overlay-ride` rides a Perch the user drags slowly (`hold#` animation, still `Perched`).
- `overlay-drop` falls when the Perch window closes (a new `Falling` frame).

## How to get to it (user POV)

- Launch the app (`cargo run -p fidget` or a Release binary) on a desktop session.
- On Linux X11, use a session with a supporting window manager (openbox under Xvfb is enough).
- On macOS / Windows, launch on a real interactive desktop (not a bare CI agent without a display).

## Driving it with verify-overlay helpers

Preconditions:

- `.agents/skills/verify-fidget/helpers/doctor.sh` exits `0` for the active lane.
- Linux: `DISPLAY` set; `xdotool` `xprop` `xwininfo` `xterm` present; supporting WM (`openbox` if Xvfb).
- Evidence dir `$FIDGET_VERIFY_EVIDENCE` exists.

- **Linux X11 full check.** Run `xvfb-run -a -s "-screen 0 1280x720x24" .agents/skills/verify-fidget/helpers/drive-overlay-x11.sh`. Exit code `0` confirms presence, perch, ride, drop and poke. On main it currently exits `1` at `App never published an overlay line` because the wrapped script runs a release build (see Gotchas). When `xprop _NET_WM_STATE` is empty but the app log has `^overlay:`, `EWMH configured`, and `frame:` lines, the script WARNs and continues; WARN is not failure (see Gotchas). Prefer `frame:.*Perched` when a Perch window existed before launch; Falling/Grounded frames still count as presence.
- **macOS full check.** Run `.agents/skills/verify-fidget/helpers/drive-overlay-macos.sh`. Exit code `0`. Evidence contains `.verify/<stamp>/` with frame-loop and overlay PASS lines.
- **Windows full check.** Run `.agents/skills/verify-fidget/helpers/drive-overlay-win.ps1` on a dual-display Windows host. Exit code `0`. Evidence contains `.verify/win-*/`.
- **Linux X11 presence by hand.** Under Xvfb, start `openbox --replace &`, then `.agents/skills/verify-fidget/helpers/launch.sh`. Pick the `Fidget` class window that is at least 200×200 and run `xprop -id $ID _NET_WM_STATE`. Expect `_NET_WM_STATE_ABOVE, _NET_WM_STATE_SKIP_TASKBAR, _NET_WM_STATE_SKIP_PAGER`, plus `frame:` lines in `$FIDGET_VERIFY_SCRATCH/app.log`.
- **Proof.** Keep the helper-copied stamp tree and a `PROOF.md` line naming `overlay-presence` and the helper invoked.

## Gotchas

- Xvfb alone has no window manager: without `openbox` (or another EWMH WM), `_NET_CLIENT_LIST` is empty and Perch never happens — the script fails before poke.
- The Perch window must exist **before** the app starts; a late window sits above the sprite and is not a surface from below.
- GDK creates a tiny `Fidget` window; assert size ≥200×200 when picking the overlay id.
- Wayland-only sessions lose window Perches (screen-edge physics only). That is supported product behavior, not a script failure — do not claim X11 perch parity there.
- Two verification agents on one display corrupt each other's Perch and hit-test; refuse concurrent drives.
- Linux shells panic at tray init without `libayatana-appindicator3` (`libayatana-appindicator3.so.1`). Install `libayatana-appindicator3-1` (and deps) or point `LD_LIBRARY_PATH` at a local extract for verify-only runs.
- Since #1325 a release build's stderr lands only in `~/.local/share/fidget/process.log` (lines prefixed with Unix seconds). `scripts/verify-overlay-x11.sh` hard-codes `cargo build --release` and `target/release/fidget`, so its trace log stays empty and it fails before any check. That is a script regression outside this skill; report it, and use the by-hand recipe on the debug binary meanwhile.
- App log may say `EWMH configured` while `xprop _NET_WM_STATE` is still empty (WM never reflected ABOVE/SKIP_TASKBAR). `scripts/verify-overlay-x11.sh` WARNs and continues in this case — WARN is not failure. Frames/`^overlay:` remain valid presence evidence.
