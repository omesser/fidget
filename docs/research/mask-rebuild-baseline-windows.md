# Windows Click-Through Mask Rebuild Baseline

Benchmark for issue [#428](https://github.com/omesser/fidget/issues/428): how often the Windows click-through region (`SetWindowRgn`) is rebuilt, and what each rebuild costs, per scenario. The X11 side is [`mask-rebuild-baseline-x11.md`](./mask-rebuild-baseline-x11.md).

## Environment

- **Host:** Oded's workstation DESKTOP-UQIE144 (machineId `9723dbe7-21c6-40c0-afcd-2172f499de66`), Windows 11 Pro for Workstations 10.0.26200.
- **Displays:** primary 3440×1440 at (0,0); secondary 1200×1920 at (-1200,-209). Every rebuild in the 2026-10-30 runs was on `overlay-0`, the primary.
- **Pointer:** `SetCursorPos`, aimed by the script at the sprite centre from the `FIDGET_TRACE_FRAMES` lines once per display refresh, not by hand.

## What a rebuild is

`overlay::apply_input_mask` turns the swept art into rects, combines them with `CreateRectRgn`/`CombineRgn`, and hands the region to `SetWindowRgn`. The `region rebuild N rects ..., T ms` trace line under `FIDGET_TRACE_MASK_REBUILD=1` times that whole path.

- **No hover gate on Windows.** In the Windows branch of `src-tauri/src/frame_loop.rs`, `decide_region_action` returns `ApplyMask` whenever there is art, whatever `ignore` (pointer away) says, and `decide_overlay_action` applies it whenever the region changed. So the region follows the drawn art wherever the pointer is. X11 only updates its shape while the pointer is over the sprite, which is why X11 idles at 0 and Windows does not. A hover gate or an unchanged-shape skip for Windows belongs in [#1526](https://github.com/omesser/fidget/issues/1526).
- **Two rebuilds per frame flip.** The region is built from a `DrawTrail` (`crates/core/src/overlay.rs`), which holds the last frame and the new one for one tick and then the new one alone. Each flip therefore changes the region twice, and a standing animation rebuilds at twice its fps. X11's shape is keyed on the current mask only, so it updates once per flip.
- **Rects, not pixels, and scale does not change them.** Each rect is a run of opaque source cells drawn at the sprite's scale, so the rect count is the same at 1x and 4x.

## Per-scenario rates, release (2026-10-30)

- **Build:** `cargo build --release -p fidget` with `CARGO_TARGET_DIR=E:\workspace\target-428`, at `3732ff17` (app code as main `7eb4c09`).
- **Script:** `scripts/bench-mask-rebuild-windows-scenarios.ps1`, which runs `scripts/bench-mask-rebuild-windows-flame.ps1 -Scenario NAME -Check` per row: no WPR, no admin. Each scenario runs a bench copy of BMO or Nim whose only Behavior loops one animation, with the `[cursor]` table dropped and `FIDGET_HOVER_DELAY_MS=3600000`, as the X11 script does. A rebuild counts only while that animation is on screen, and the rate is over that time, so the fall and landing at launch stay out. Walk, fast, large and small hold the pointer on the sprite; idle parks it at (0,0).
- **Runs:** walk, fast, large, small and small at 1x from `E:\workspace\fidget-428-runs\20261030-220843\`. Idle from `E:\workspace\fidget-428-runs\20261030-221634\` at `de9d155c`, rerun after the first idle run counted the launch fall; same binary.
- **Evidence:** [`scenarios-release.txt`](./mask-rebuild-baseline-windows/scenarios-release.txt), both runs' `table.txt`.

"Mask ms/s" is the rate times the median ms. The X11 columns are from the [X11 table](./mask-rebuild-baseline-x11.md#per-scenario-rates-2026-10-28) (release, `b893e92c`, full updates).

| Scenario | Character | Rebuilds/s | ms/rebuild: median (min–max, n) | Rects (median) | Mask ms/s | X11 updates/s | X11 ms/update |
|---|---|---|---|---|---|---|---|
| Idle, pointer away | BMO 126×128@1x | 1.99 | 2.28 (1.53–5.83, 18) | 298 | 4.5 | 0 | n/a |
| Walk, pointer on sprite | BMO 126×128@1x | 62.63 | 1.90 (1.62–4.22, 500) | 319 | 119 | 62.6 | 0.086 full, 0.011 position-only |
| Fast animation (react, 10 fps) | BMO 126×128@1x | 19.80 | 2.08 (1.41–6.19, 200) | 284 | 41 | 9.8 | 0.13 |
| Large (react at 4x, 504×512) | BMO 126×128@4x | 19.76 | 2.06 (1.41–6.39, 200) | 284 | 41 | 9.8 | 0.14 |
| Small (react, 12 fps), shipped scale | Nim 32×32@4x | 24.01 | 1.14 (0.70–4.58, 200) | 86 | 27 | 12.1 | 0.028 |
| Small at 1x | Nim 32×32@1x | 24.03 | 1.18 (0.71–5.86, 200) | 86 | 28 | 12.1 | 0.028 |

### What sets each number

- **Idle: 2/s, not 0.** BMO's idle is 2 frames at 1 fps, one flip a second, and each flip rebuilds twice. Nothing gates it on the pointer (see above).
- **Walk: 62.6/s, the frame loop's tick rate**, the same as X11. The sprite moves every tick, and position is part of the region.
- **Fast, large and small: twice the animation's fps** (BMO react 10 fps, Nim react 12 fps), from the `DrawTrail`.
- **Cost tracks rect count, not scale.** BMO at 4x matches BMO at 1x within 0.02 ms, and Nim at 1x matches Nim at 4x within 0.04 ms, as on X11. BMO's 284 rects cost 2.07 ms and Nim's 86 cost 1.16 ms. Two points give roughly 0.75 ms per rebuild plus 4.6 µs per rect, so a small sprite is cheaper but not proportionally so.
- **Windows is 15–40 times X11 per rebuild:** 2.07 against 0.135 ms for BMO, 1.16 against 0.028 ms for Nim. Different hosts, so the ratio is rough.
- **Worst case: about 119 ms of one core per second (12%)** while the pointer rests on a walking BMO. Standing under the pointer costs 27–41 ms/s, and idle with the pointer away about 4.5 ms/s.

A longer release Nim run with its shipped Behaviors ([#1943](https://github.com/omesser/fidget/pull/1943), 1000 rebuilds in 69 s) had a median of 1.36 ms, in line with the 1.14–1.18 ms here.

### Running it

From a non-elevated PowerShell. It holds the pointer for about 2 minutes; `-Only idle,walk` runs some rows, and `-Report DIR` prints a finished run's table again.

```powershell
$env:CARGO_TARGET_DIR='E:\workspace\target-428'; cargo build --release -p fidget
$env:FIDGET_BENCH_GREEN_LIGHT='1'
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\bench-mask-rebuild-windows-scenarios.ps1 -Bin E:\workspace\target-428\release\fidget.exe
```

## Release build (2026-10-26)

- **Build:** release `fidget.exe` and `fidget.pdb` at `759205a6`, the #1876 tip.
- **Host:** DESKTOP-UQIE144, elevated PowerShell, WPR recording.
- **Scenario:** `scripts/bench-mask-rebuild-windows-flame.ps1` with its defaults (`-Target 100`, BMO). The script holds the pointer on the sprite centre until 100 region rebuilds land. It read the sprite size from the overlay line: a half-size of 63×64, so BMO at 126×128.
- **What the time covers:** the `region rebuild` trace in `apply_input_mask()`, from `region_plan` through the `CreateRectRgn`/`CombineRgn` loop to `SetWindowRgn` returning.
- **Evidence:** `summary.txt` from the run, below, also posted on [#1876](https://github.com/omesser/fidget/pull/1876#issuecomment-6420071657). The 290 MB `trace.etl`, `fidget.pdb` and `rebuilds.log` stay on the workstation under `E:\workspace\fidget-428-flame\20261026-173216\`.

```
rebuilds: 100 in 2s (target 100)
rebuild ms: min 1.47 median 2.42 max 4.75
rects: min 260 median 304 max 322
wpr -stop exit: 0
```

- **Rebuild time:** median 2.42 ms, range 1.47–4.75 ms over 100 rebuilds. The X11 release build at `2355abcb` has a pooled median of 0.148 ms per full BMO rebuild (see [`mask-rebuild-baseline-x11.md`](./mask-rebuild-baseline-x11.md#release-build-2026-10-26)), so Windows is about 16 times slower per rebuild. The two hosts and builds differ, so treat that ratio as rough.
- **Rects:** 260–322 rectangles per region, median 304. These are the swept art rects plus every overlay rect, which the trace line counts as hotspots and painted rects. They are not opaque cells, so they are far fewer than BMO's 6219–7888 opaque cells.
- **Rate:** rebuilds came about every 20 ms (about 50 per second) with the pointer held on the sprite, so the target was reached in 2 s.
- **Trace length:** 100 rebuilds is a short trace; a WPR rerun with `-Target 1000` would give WPA more samples.
- **Flamegraph:** not exported yet; the X11 flamegraph covers #428's flamegraph box. `scripts/export-mask-flame-windows.ps1` runs `wpaexporter` on `trace.etl` without admin and folds the CPU Usage (Sampled) stacks into `folded.txt`, `mask-folded.txt` and, with `inferno-flamegraph`, `flame.svg`. To read the trace by hand instead, open it in WPA: CPU Usage (Sampled), process `fidget.exe`, Flame by Stack, and search for `SetWindowRgn`.

## Debug baseline (2026-09-24)

The first Windows numbers came from a debug build at `a38ba3b` (PR #981 instrumentation), with ad hoc pointer moves and the shipped Behaviors. They are superseded by the release table above and kept for history; the summaries are in [`mask-rebuild-baseline-windows/`](./mask-rebuild-baseline-windows/).

| Scenario | Rebuilds | ms/rebuild |
|---|---|---|
| Idle, pointer at (50,50) | 0 in 12 s | n/a |
| BMO walk under the pointer | 223 in ~5 s, ~44/s | avg 14.5 |
| BMO react (loop once, poked) | 68 in 12.1 s, 5.6/s | avg 21.8 |
| Black Mage @3x (37×33 source) | 6 in 12.1 s, 0.5/s | avg 2.5 |

Debug against release is not only the build profile: the region code has changed since `a38ba3b`, and the 0 idle there does not hold on current main (see [What a rebuild is](#what-a-rebuild-is)).

## Evidence

- [`mask-rebuild-baseline-windows/scenarios-release.txt`](./mask-rebuild-baseline-windows/scenarios-release.txt): the 2026-10-30 release runs, both `table.txt` files.
- [`mask-rebuild-baseline-windows/`](./mask-rebuild-baseline-windows/): the 2026-09-24 debug pack (environment, summaries, sample lines).
- Per-run `summary.txt`, `result.json` and logs stay on the workstation under the run folders above.

### Code

- `src-tauri/src/platform/windows/overlay.rs`: `apply_input_mask`, the `region rebuild` trace, `SetWindowRgn`.
- `src-tauri/src/frame_loop.rs`: `decide_region_action` and `decide_overlay_action`, the Windows region policy.
- `crates/core/src/overlay.rs`: `DrawTrail`, the last and current frame the region covers.

## Comparison to Issue #432 (System Wakeups)

#432 measured idle wakeups on Linux, where mask updates are 0/s with the pointer away. On Windows an idle BMO with the pointer away adds about 2 region rebuilds a second, about 4.5 ms of CPU per second. That is small, but not zero; [#1526](https://github.com/omesser/fidget/issues/1526) tracks skipping it.

## Follow-up

1. **Windows hover gate or unchanged-shape skip:** [#1526](https://github.com/omesser/fidget/issues/1526). The walk row (119 ms/s) costs most, and a skip alone would not help it, since the region moves every tick. X11 handles that case by moving a cached shape (position-only updates).
2. **Windows flamegraph:** `scripts/export-mask-flame-windows.ps1` on the recorded `trace.etl`. Not needed for #428; the X11 flamegraph covers that box.
