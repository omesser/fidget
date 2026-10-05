---
name: verify-fidget
description: "Drive the fidget desktop mascot (Tauri overlay) the way a user does — launch, doctor, poke/perch/summon via existing verify-overlay* scripts, capture evidence. Use when proving overlay, gesture, or chat behavior for this repo."
---

# Verify Fidget

Project-local control skill for **fidget**, a Tauri desktop mascot whose primary surface is a full-display overlay sprite (perch, poke, summon/chat). Agents read this cold mid-task: every command below is literal.

**Ask before every run.** Launching or driving the app takes over the owner's screen. Follow `docs/agents/gui-takeover.md`: set up, post what happens and for how long, and wait for a go-ahead for that run.

## Interview summary (do not re-derive)

| Axis | Finding |
|---|---|
| **Surface** | Desktop overlay mascot (macOS / Linux X11 / Windows). Secondary: Settings webview, Chat surface (Summon), tray menu. |
| **Run** | `cargo run -p fidget` from repo root (or `target/debug/fidget` after `cargo build -p fidget`). Offline by default (Static Director). A release binary writes its stderr only to the process log (see Launch), so verify with debug. |
| **Drive** | Prefer existing scripts: `scripts/verify-overlay.sh` (macOS), `scripts/verify-overlay-x11.sh` (Linux X11 under a WM), `scripts/verify-overlay-win.ps1`, `scripts/verify-settings-webview-phase2-win.ps1`, `scripts/verify-settings-zorder-x11.sh`, `scripts/verify-settings-keyboard-webview.sh`, `scripts/probe-harness.sh`, `scripts/test_verify_overlay_diagnostics.sh`. Plus `cargo test` / `node --test tests/*.test.js`. Poke and Summon are CLI-native on macOS: `cargo run -p fidget-verify -- poke` / `-- summon` (ADR-0027 stone 2) — do not hand-roll a click with the bash helpers for those two verbs. |
| **Observe** | Overlay logs (`FIDGET_TRACE_FRAMES=1`, `FIDGET_TRACE_HITTEST=1`) on a debug build's stderr, `.verify/` stamp dirs, screenshots when capturable, exit codes, `verbs: …Poke` / `verbs: …Summon` lines. |
| **Isolate** | **Refuse double-drive on one display.** Two instances share the same window list / hit-test path (`FIDGET_INSTANCES` is multi-character in *one* process, not two agents). Kill only the PID this run started. |

## Evidence location (survives cleanup)

```sh
export RUN_ID="${RUN_ID:-$(date +%Y%m%d-%H%M%S)-$$}"
export FIDGET_VERIFY_ROOT="/tmp/fidget-verify-$RUN_ID"
export FIDGET_VERIFY_EVIDENCE="$FIDGET_VERIFY_ROOT/evidence"
mkdir -p "$FIDGET_VERIFY_EVIDENCE"
```

Cleanup removes processes and scratch under `$FIDGET_VERIFY_ROOT/scratch` only. **Never delete `$FIDGET_VERIFY_EVIDENCE`.** Name that path in every proof report.

## Launch

From the repo root:

```sh
# One-time build. Debug, because only debug echoes stderr (#1325)
cargo build -p fidget

# Helpers set RUN_ID / evidence / scratch and start a traced instance when a
# platform script does not already own the lifecycle:
.agents/skills/verify-fidget/helpers/launch.sh
```

Since #1325 every build sends stderr to `process.log` in the data dir (Linux `~/.local/share/fidget/process.log`), each line prefixed with Unix seconds. Only a debug build also echoes it to the terminal, so a release binary's terminal log stays empty and nothing matching `^overlay:` ever arrives. `scripts/verify-overlay-x11.sh` builds and runs release, so it fails with `App never published an overlay line` on main; `scripts/verify-settings-zorder-x11.sh` does the same whenever `target/release/fidget` exists. Those scripts sit outside this skill: report the failure, and drive the debug binary through `launch.sh` meanwhile.

Ready signals (any one is enough for doctor):

- Log line matching `^overlay:`
- On X11: an `Fidget` class window ≥200×200 (GDK leaves a 10×10 placeholder — ignore it)
- Process still alive (`kill -0 $APP_PID`)

Env that unattended runs usually want:

```sh
export FIDGET_TRACE_FRAMES=1
export FIDGET_TRACE_HITTEST=1
export FIDGET_CAPTURABLE=1   # force visible in captures (default is already capturable; =0 forces hide)
# Optional: FIDGET_DIRECTOR_API_KEY=… to skip Keychain prompts on macOS
```

`scripts/verify-overlay.sh` and `scripts/verify-overlay-x11.sh` launch (and tear down) the app themselves — use those for overlay/poke proofs rather than a parallel launcher.

## Doctor

Read-only health check. Run before Drive whenever anything looks off:

```sh
.agents/skills/verify-fidget/helpers/doctor.sh
```

Doctor answers:

1. Repo root looks like fidget (`Cargo.toml` workspace + `src-tauri/`).
2. Debug binary exists (`target/debug/fidget`) or `cargo` can build.
3. Platform tools for the active lane are on `PATH` (macOS: `swift`; Linux X11: `xdotool` `xprop` `xwininfo` `xterm` + `DISPLAY` + supporting WM / `openbox`; Windows: PowerShell + dual display for overlay-win).
4. If `$APP_PID` is set, that process is alive and its log (if any) contains `^overlay:`.
5. Unit lanes green when asked: `cargo test -p fidget-core` and `node --test tests/*.test.js` (see Helpers).

Exit `0` = worth driving. Non-zero = fix Launch before Drive.

## Drive

Map lives in [`features/`](features/README.md). Prefer one feature per proof run.

| Lane | Command |
|---|---|
| Linux X11 overlay + perch/ride/drop + poke | `xvfb-run -a -s "-screen 0 1280x720x24" .agents/skills/verify-fidget/helpers/drive-overlay-x11.sh` (needs `openbox` + `xterm` on bare Xvfb; optional `FIDGET_VERIFY_PREFIX=/path/to/extracted` for deb-extracted libs/themes). Fails at `App never published an overlay line` until the wrapped script runs a debug build (see Launch) |
| macOS overlay + physics + hit-test | `.agents/skills/verify-fidget/helpers/drive-overlay-macos.sh` |
| macOS Poke (gesture verb) | `cargo run -p fidget-verify -- poke` — real click, asserts `verbs:.*Poke` |
| macOS Summon (gesture verb) | `cargo run -p fidget-verify -- summon` — real double-click, asserts `verbs:.*Summon` |
| e2e scenario | `cargo run -p fidget-verify -- scenario <name>` prints this host's takeover header; `--go` runs `scripts/scenarios/<name>.sh` (macOS), `<name>.x11.sh`, or `<name>.win.ps1` after the owner's go-ahead (`scripts/scenarios/README.md`) |
| Windows overlay | `.agents/skills/verify-fidget/helpers/drive-overlay-win.ps1` |
| Windows Settings | `scripts/verify-settings-webview-phase2-win.ps1` (copy `$Out` into evidence after) |
| Linux Settings z-order | `xvfb-run -a -s "-screen 0 1280x720x24" scripts/verify-settings-zorder-x11.sh` (proves webview stacks above overlay) |
| Harness ACP (no sprite) | `FIDGET_HARNESS=hermes scripts/probe-harness.sh` |
| macOS Keychain diagnostic unit | `scripts/test_verify_overlay_diagnostics.sh` |
| Settings keyboard checks on fixtures | `scripts/test_verify_settings_keyboard.sh` (no app, no Accessibility) |
| macOS Settings keyboard | `scripts/verify-settings-keyboard-webview.sh` (needs an Accessibility grant) |
| macOS Settings select | `scripts/verify-settings-webview-select-macos.sh` |
| macOS Settings clipboard | `scripts/verify-settings-webview-clipboard-macos.sh` |
| Core + renderer units | `.agents/skills/verify-fidget/helpers/doctor.sh --units` |

Stable handles: log patterns (`frame: N Perched`, `verbs:.*Poke`, `verbs:.*Summon`, `EWMH configured`), X11 WM_CLASS `Fidget`, EWMH `_NET_WM_STATE_ABOVE` + `_NET_WM_STATE_SKIP_TASKBAR`. Prefer those over click coordinates when asserting.

`fidget-verify` exits `0` pass, `1` fail, `2` skip (nothing provable on this host), `3` tool error, and `--json` puts one result object on stdout — the contract is `crates/verify/src/contract.rs`.

## Evidence

For every Drive:

1. Exercise the real user path (click / perch window / double-click), not internal setters.
2. Capture action **and** resulting state (log excerpts before/after, exit code, optional screenshot).
3. Copy platform script artifacts into `$FIDGET_VERIFY_EVIDENCE/<feature-id>/` (helpers do this).
4. Record `RUN_ID`, feature ID, entry point, and commands in `$FIDGET_VERIFY_EVIDENCE/PROOF.md`.

Proof standards:

- Overlay presence: overlay window + EWMH states + `^overlay:` log.
- Poke: `verbs:.*Poke` after a real click on the sprite body — on macOS, `cargo run -p fidget-verify -- poke` proves this end to end and writes evidence under `<evidence>/poke/`.
- Summon: `verbs:.*Summon` after double-click; Chat window appears when a display session can show it — on macOS, `cargo run -p fidget-verify -- summon` proves this end to end and writes evidence under `<evidence>/summon/`.
- Capturable override: env `FIDGET_CAPTURABLE=0|1` observed in settings/platform behavior (macOS sharing type / Windows WDA); Linux has no capture-exclusion API equivalent — document as Gotcha.

## Cleanup

```sh
.agents/skills/verify-fidget/helpers/cleanup.sh
```

Rules:

- Kill **only** PIDs recorded under `$FIDGET_VERIFY_ROOT/scratch/pids` (and children of those). Never `pkill -f fidget` by name alone when a user's own instance may be running.
- Remove `$FIDGET_VERIFY_ROOT/scratch`.
- **Keep** `$FIDGET_VERIFY_EVIDENCE` intact.
- Platform scripts that trap their own EXIT already tear down the app they started; still run cleanup to clear helper scratch.

After cleanup, confirm:

```sh
test -d "$FIDGET_VERIFY_EVIDENCE" && ls -la "$FIDGET_VERIFY_EVIDENCE"
```

## Helpers

All under `.agents/skills/verify-fidget/helpers/` (executable):

| Script | Invocation | Role |
|---|---|---|
| `common.sh` | sourced by others | `RUN_ID`, evidence/scratch paths, repo root |
| `doctor.sh` | `…/doctor.sh` [`--units`] | Launch readiness + optional unit suites |
| `launch.sh` | `…/launch.sh` | Build debug if needed; start traced app into scratch log when no drive script owns lifecycle |
| `cleanup.sh` | `…/cleanup.sh` | Tear down helper-owned PIDs; preserve evidence |
| `drive-overlay-x11.sh` | `…/drive-overlay-x11.sh` | Wraps `scripts/verify-overlay-x11.sh`; copies `.verify/x11-*` → evidence |
| `drive-overlay-macos.sh` | `…/drive-overlay-macos.sh` | Wraps `scripts/verify-overlay.sh`; copies `.verify/<stamp>` → evidence |
| `drive-overlay-win.ps1` | `…/drive-overlay-win.ps1` | Wraps `scripts/verify-overlay-win.ps1`; copies `.verify/win-*` → evidence |
| `prove-units.sh` | `…/prove-units.sh` | `cargo test -p fidget-core` + `node --test` + diagnostics script; writes evidence |
| `xterm-shim.sh` | used automatically by `drive-overlay-x11.sh` | Translates `xterm -geometry/-title/-e` to `xfce4-terminal` when real xterm is absent |

Example end-to-end (Linux box with X11 deps):

```sh
export RUN_ID=$(date +%Y%m%d-%H%M%S)-$$
.agents/skills/verify-fidget/helpers/doctor.sh --units
xvfb-run -a -s "-screen 0 1280x720x24" \
  .agents/skills/verify-fidget/helpers/drive-overlay-x11.sh
.agents/skills/verify-fidget/helpers/cleanup.sh
ls "$FIDGET_VERIFY_EVIDENCE"
```

When GUI/overlay cannot run (headless without `openbox`/`xterm`, Wayland-only with no Perches, no display): prove the runnable subset with `doctor.sh --units` / `prove-units.sh`, write the exact GUI gap into `$FIDGET_VERIFY_EVIDENCE/PROOF.md` and the feature Gotchas — do not leave a manual chore list for the human.

## Maintenance

Keep the feature map honest with `/maintain-verification-skill` as the app changes.
