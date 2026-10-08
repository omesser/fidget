# Capturable / hide from captures

By default the fidget appears in screenshots and screen shares. Settings → Presence → "Appear in screenshots and screen shares" (and `FIDGET_CAPTURABLE`) control the setting. macOS and Windows implement platform capture-exclusion APIs. Linux shows the same setting row **disabled (frozen)** with help text explaining no portable API exists (ADR-0024).

## Sub-features

- `capturable-default` default is capturable (`true`).
- `capturable-force-on` `FIDGET_CAPTURABLE=1` forces the setting visible.
- `capturable-force-off` `FIDGET_CAPTURABLE=0` forces the setting excluded; macOS/Windows apply exclusion, Linux has no exclusion API.
- `capturable-settings` Settings → Presence → "Appear in screenshots and screen shares" row exists on all platforms. Checkbox matches the in-force value when no env override is set. **Linux:** row is disabled (frozen), no config write.

## How to get to it (user POV)

- macOS/Windows: Settings (tray / platform menu) → Presence → toggle "Appear in screenshots and screen shares". The help line says "Needs restart."
- Linux: Settings → Presence shows the row **disabled (frozen)** with help text: "Fidget has no portable API to exclude itself from screen captures on Linux. Always visible." No config write on Linux.
- Or export `FIDGET_CAPTURABLE=0` or `=1` before launch for a one-process override (CI / verify scripts; macOS/Windows only).

## Driving it with verify-overlay helpers

Preconditions:

- Doctor green; binary built.
- macOS or Windows for real capture-exclusion APIs. Linux shows the row disabled (frozen) with no config write (#1338).

- **Force visible for screenshot proofs**: `export FIDGET_CAPTURABLE=1` then run the platform `drive-overlay-*` helper. Screenshots in the stamp dir should include the sprite when the capture tool honors capturable windows.
- **Force hidden (macOS)**: Run `FIDGET_CAPTURABLE=0 .agents/skills/verify-fidget/helpers/drive-overlay-macos.sh`. The script expects every overlay at `sharing=0` when `FIDGET_CAPTURABLE` is an off word (`0`, `off`, `false`, `no`) and `sharing=1` otherwise.
- **Windows**: `scripts/verify-overlay-win.ps1` asserts `WDA_EXCLUDEFROMCAPTURE` behavior per its checks; copy `$Out` into evidence.
- **Settings smoke (Windows)**: `scripts/verify-settings-webview-phase2-win.ps1` for the Settings window chrome (not capturable-specific alone).
- **Linux Settings row present (#1338)**: Launch with `FIDGET_OPEN_SETTINGS=1 .agents/skills/verify-fidget/helpers/launch.sh` inside `dbus-run-session`, then `python3 scripts/ax-window-linux.py dump "$(cat "$FIDGET_VERIFY_SCRATCH/pids/app.pid")" Settings`. The Presence page lists `check-box|Go away`, `check-box|Hide in fullscreen apps`, and `check-box|Appear in screenshots and screen shares`. **The row is disabled (frozen)** with help text explaining no API. No config write on Linux (ADR-0024). Dismiss Settings with alt+F4 under openbox, which leaves the app running, not `xdotool windowclose` (see Summon / chat Gotchas).
- **Proof**: Record env value, platform, and either a screenshot with/without sprite or the platform property the script already asserts. Put artifacts under `$FIDGET_VERIFY_EVIDENCE/capturable/`.

## Gotchas

- `docs/DEVELOPMENT.md`: `FIDGET_CAPTURABLE=1` forces the setting visible; `=0` forces excluded (macOS/Windows only).
- Linux overlay shows the Settings row **disabled (frozen)** with help text explaining no API exists (#1338). No config write on Linux. Per ADR-0024, Linux has no capture-exclusion API (unlike macOS `NSWindowSharingNone` / Windows `WDA_EXCLUDEFROMCAPTURE`). Do not fail a Linux run for missing capture exclusion or config persistence; the disabled UI row is the deliverable.
- Env overrides Settings for that process (macOS/Windows); a Settings toggle will not win while the env is set.
- Screenshot tools that capture the compositor differently may still omit or include the sprite — prefer the platform property checks the verify scripts already encode.
