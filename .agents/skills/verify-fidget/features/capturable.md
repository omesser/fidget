# Capturable / hide from captures

By default the fidget appears in screenshots and screen shares. On macOS and Windows, Settings → Presence → Hide → "Appear in screenshots and screen shares" (and `FIDGET_CAPTURABLE`) force capturable or hidden so verify scripts can screenshot the sprite or test the hide path. Linux has no such row.

## Sub-features

- `capturable-default` default is capturable (`true`).
- `capturable-force-on` `FIDGET_CAPTURABLE=1` forces visible in captures.
- `capturable-force-off` `FIDGET_CAPTURABLE=0` forces exclusion where the platform supports it.
- `capturable-settings` (macOS, Windows) the Settings checkbox matches the in-force value when no env override is set. Linux Settings does not draw the row.

## How to get to it (user POV)

- macOS / Windows: open Settings (tray / platform menu) → Presence → Hide → toggle "Appear in screenshots and screen shares". The help line says "Needs restart."
- Or export `FIDGET_CAPTURABLE=0` or `=1` before launch for a one-process override (CI / verify scripts).

## Driving it with verify-overlay helpers

Preconditions:

- macOS or Windows for real capture-exclusion APIs. Linux has no equivalent exclusion API in this codebase — see Gotchas.
- Doctor green; binary built.

- **Force visible for screenshot proofs.** `export FIDGET_CAPTURABLE=1` then run the platform `drive-overlay-*` helper. Screenshots in the stamp dir should include the sprite when the capture tool honors capturable windows.
- **Force hidden (macOS).** Run `FIDGET_CAPTURABLE=0 .agents/skills/verify-fidget/helpers/drive-overlay-macos.sh`. The script expects every overlay at `sharing=0` when `FIDGET_CAPTURABLE` is an off word (`0`, `off`, `false`, `no`) and `sharing=1` otherwise.
- **Windows.** `scripts/verify-overlay-win.ps1` asserts `WDA_EXCLUDEFROMCAPTURE` behavior per its checks; copy `$Out` into evidence.
- **Settings smoke (Windows).** `scripts/verify-settings-webview-phase2-win.ps1` for the Settings window chrome (not capturable-specific alone).
- **Linux Settings row absent.** Launch with `FIDGET_OPEN_SETTINGS=1 .agents/skills/verify-fidget/helpers/launch.sh` inside `dbus-run-session`, then `python3 scripts/ax-window-linux.py dump "$(cat "$FIDGET_VERIFY_SCRATCH/pids/app.pid")" Settings`. The Presence page lists `check-box|Go away` and `check-box|Hide in fullscreen apps` and no "Appear in screenshots" row. Dismiss Settings with alt+F4 under openbox, which leaves the app running, not `xdotool windowclose` (see Summon / chat Gotchas).
- **Proof.** Record env value, platform, and either a screenshot with/without sprite or the platform property the script already asserts. Put artifacts under `$FIDGET_VERIFY_EVIDENCE/capturable/`.

## Gotchas

- `docs/DEVELOPMENT.md`: `FIDGET_CAPTURABLE=1` forces visible; `=0` forces hide. `scripts/verify-overlay.sh` agrees.
- Linux overlay does not implement macOS `NSWindowSharingNone` / Windows `WDA_EXCLUDEFROMCAPTURE`, and Linux Settings omits the checkbox. Do not fail a Linux run for missing capture exclusion or a missing row; document the platform gap.
- Env overrides Settings for that process; a Settings toggle will not win while the env is set.
- Screenshot tools that capture the compositor differently may still omit or include the sprite — prefer the platform property checks the verify scripts already encode.
