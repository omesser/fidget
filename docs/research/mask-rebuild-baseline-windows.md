# Windows Click-Through Mask Rebuild Baseline

Benchmark for issue [#428](https://github.com/omesser/fidget/issues/428): per-pixel cost of rebuilding the Windows DWM click-through region when the sprite animation frame changes.

The older numbers below (14.5–22 ms for BMO) came from a debug build at `a38ba3b`. A release build at `759205a6` rebuilds BMO's region in about 2.4 ms; see [Release build (2026-10-26)](#release-build-2026-10-26).

## Environment

### Oded's DESKTOP-UQIE144 workstation

- **Host**: DESKTOP-UQIE144 (machineId `9723dbe7-21c6-40c0-afcd-2172f499de66`)
- **OS**: Microsoft Windows 11 Pro for Workstations version 10.0.26200 (Build 26200)
- **Display**: Primary 3440×1440 at (0,0) work area height 1392; Secondary 1200×1920 at (-1200,-209)
- **Character**: BMO (126×128 px sprite, ~6231–7888 opaque mask cells depending on frame); Black Mage (37×33 source @3x scale, ~570–630 opaque)
- **Binary**: Debug `fidget.exe` at tip `a38ba3b48bb43ed4b909cb26866b6b1af97dbcd1` (PR #981 TRACE instrumentation)
- **Evidence**: `docs/research/mask-rebuild-baseline-windows/` (slim pack: environment, summaries, sample lines)

This is Oded's development workstation with a real Windows GUI. Cursor automation used `SetCursorPos` to sprite center derived from `TRACE_FRAMES` instrumentation (not human cursor movement).

## Method

### Instrumentation

Timing in Windows click-through mask rebuild path (PR #981 tip `a38ba3b` — instrumentation not necessarily merged at time of measurement):

- Atomic counters for rebuild count and total nanoseconds
- Per-rebuild timing from start to DWM region update completion
- Logging of sprite dimensions, scale, opaque pixel count, and rebuild time when `FIDGET_TRACE_MASK_REBUILD=1`

The Windows DWM rebuild iterates the source mask and constructs a region from opaque cells. Scale multiplies rectangle size, not the opaque source count.

### When rebuilds run

The overlay only updates the input region while the cursor is over the sprite (or a control / during a hold). Walking under the cursor rebuilds at motion rate because the cache key includes sprite position.

### Measurement

- **Cursor method**: `SetCursorPos` to sprite center from `TRACE_FRAMES` `sprite(x,y)` lines (not human; not moving perch under fixed cursor)
- **Behavior trigger**:
  - **walk**: StaticDirector spontaneous walk (MCP `play_behavior` not used — Settings Bearer token not obtained)
  - **fast**: Rush `SetCursorPos` onto sprite (rush_reaction=react) + `mouse_event` LBUTTON poke
  - **large**: Cursor follow only
- **Environment variable**: `FIDGET_TRACE_MASK_REBUILD=1` (plus `TRACE_FRAMES` for correlation)

**Method gaps documented honestly:**

1. **MCP `play_behavior` not used** — Settings Bearer token (BYO) not obtained; walk triggered via StaticDirector spontaneous behavior.
2. **Walk numbers are walk-attributed subset only** — Opaque 6231–6298 matches X11 walk frames; full 15s session included climb/react rebuilds while cursor tracked sprite, but the walk scenario isolates walk-attributed rebuilds only.
3. **Fast rate ≠ continuous 10 fps** — React animation is loop=once; per-rebuild ~22 ms is the useful metric, not sustained rate.

## Release build (2026-10-26)

- **Build:** release `fidget.exe` and `fidget.pdb` at `759205a6`, the #1876 tip.
- **Host:** DESKTOP-UQIE144, elevated PowerShell, WPR recording.
- **Scenario:** `scripts/bench-mask-rebuild-windows-flame.ps1` with its defaults (`-Target 100`, BMO). The script holds the pointer on the sprite centre, which passes the cursor-over gate, until 100 region rebuilds land. It read the sprite size from the overlay line: a half-size of 63×64, so BMO at 126×128.
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
- **Trace length:** 100 rebuilds is a short trace. The pending runs use `-Target 1000`, for Nim and BMO.
- **Flamegraph:** pending. `scripts/export-mask-flame-windows.ps1` runs `wpaexporter` on `trace.etl` without admin and folds the CPU Usage (Sampled) stacks into `folded.txt`, `mask-folded.txt` and, with `inferno-flamegraph`, `flame.svg`. To read the trace by hand instead, open it in WPA: CPU Usage (Sampled), process `fidget.exe`, Flame by Stack, and search for `SetWindowRgn`.

## Results

### Idle perched (BMO, 126×128@1x), cursor not over sprite

**12-second measure window**:

- Total rebuilds: **0**
- Rate: **0.0 rebuilds/sec**

With `SetCursorPos(50,50)` away from the sprite on primary display, idle animation produced **no** mask rebuilds, matching the cursor-over gate.

### Walking (BMO walk), cursor over sprite

**Method:** `SetCursorPos` follow `TRACE_FRAMES` sprite center; StaticDirector spontaneous walk. Rebuild lines attributed to walk animation.

**Walk-attributed subset from ~5.0-second bout:**

- Total rebuilds: **223**
- Rate: **~44.4 rebuilds/sec** (motion-driven; cache key includes `x,y`)
- Average time: **14.54 ms/rebuild**
- Range: 14.03 – 15.74 ms
- Opaque pixels: **6231–6298** (matches X11 walk frames)

The walk-only subset isolates rebuilds attributed to walk animation. The full ~15s cursor-follow session also included climb (351 rebuilds) and react (40 rebuilds); walk scenario reports only the walk-attributed 223 rebuilds over ~5.0 seconds of walk.

Opaque counts 6231–6298 match the X11 walk baseline, confirming the same sprite frames.

### Fast Animation (BMO react, 10 fps)

**Method:** Rush `SetCursorPos` onto sprite (BMO rush_reaction=react) + LBUTTON poke; hold cursor over during react frames.

**React-only rebuilds over 12.1 seconds:**

- Total rebuilds: **68**
- Rate: **5.6 rebuilds/sec** (react is loop=once, not sustained 10 fps)
- Average time: **21.79 ms/rebuild**
- Range: 17.3 – 30.82 ms
- Opaque pixels: **6360–7888**

Fast animation frame rate (10 fps) doesn't increase per-rebuild cost — each rebuild is driven by opaque pixel count. The ~22 ms per-rebuild time is the useful metric; react loop=once means the rate reflects trigger frequency, not continuous animation.

A supplemental sample from the walk session (40 rebuilds when rush onto walking sprite triggered react under cursor) measured avg 23.6 ms, min 17.11 ms, max 30.03 ms — consistent with the primary fast measurement.

### Large Sprite (Black Mage, scale=3)

**Method:** `FIDGET_INSTANCES='Black Mage'` (scale=3 in manifest); `SetCursorPos` follow sprite center.

**Measurements over 12.1 seconds:**

- Sprite: **111×99 rendered (@3x scale from 37×33 source)**
- Total rebuilds: **6**
- Rate: **0.5 rebuilds/sec**
- Average time: **2.50 ms/rebuild**
- Range: 2.03 – 2.77 ms
- Opaque pixels: **570–630**

Black Mage rebuilds **much faster** than BMO (~2.5ms vs ~15-22ms) because it has far fewer opaque source pixels (570–630 vs 6231–7888). Scale affects rendered size and rectangle dimensions, but rebuild cost is dominated by the iteration over opaque source cells.

By animation: idle 4 rebuilds (avg 2.55 ms, opaque 570–579); react 2 rebuilds (avg 2.40 ms, opaque 579–630).

### Small Sprite

**Status:** Pending. Nim (`characters/nim`, 32×32 source, `scale = 4`, 344–350 opaque cells) is the smallest shipped sprite. On X11 it costs 0.028 ms per rebuild at 4x and at 1x; see the [X11 per-scenario table](./mask-rebuild-baseline-x11.md#per-scenario-rates-2026-10-28). The Windows run is `scripts/bench-mask-rebuild-windows-flame.ps1 -Instances Nim -Target 1000`, with `-Scale 1` for Nim at 1x.

## Comparison to X11 Baseline

X11 baseline: `docs/research/mask-rebuild-baseline-x11.md` (issue #428, shipped in PR #968).

The Windows numbers are from the debug build at `a38ba3b` unless marked release. The X11 numbers are from the release build at `b893e92c`, with the pointer held on the sprite.

| Scenario | Windows (DESKTOP-UQIE144) | X11 (Grok Bot box, release) |
|--------|---------------------------|------------------------|
| **Idle** (cursor away) | 0 rebuilds / 12s = **0.0/s** | **0/s** |
| **Walk** (BMO, cursor over) | 223 / ~5.0s = **~44/s**, avg **14.5 ms** | **62.6/s**; 13% full at 0.086 ms, the rest position-only at 0.011 ms |
| **Fast** (BMO react) | 68 / 12.1s = **5.6/s**, avg **21.8 ms** | **9.8/s**, 0.13 ms |
| **Pointer held on BMO** (release) | about **50/s**, median **2.42 ms** (`759205a6`) | see Fast |
| **Large** (Black Mage @3x) | 6 / 12.1s = **0.5/s**, avg **2.5 ms** | 0.040 ms (2026-10-26 release run) |
| **Large** (BMO @4x) | pending | **9.8/s**, 0.14 ms |
| **Small** (Nim @4x) | pending | **12.1/s**, 0.028 ms |

**Key observations:**

1. **Idle cursor-away is 0 rebuilds/sec on both.** The cursor-over gate is effective; idle animation does not trigger rebuilds when the pointer is elsewhere.
2. **Release Windows is about 16–18 times slower per BMO rebuild than release X11:** 2.42 ms against 0.13–0.15 ms. The hosts differ, so treat the ratio as rough.
3. **Rates follow the same limits on both:** the frame-loop tick while walking, the animation's fps while standing. The debug Windows rates were measured with ad hoc pointer moves, so they are lower bounds.
4. **Cost is driven by opaque pixel count, not scale.** Black Mage at 3× scale is faster than BMO at 1× because the source opaque count is lower. On X11, BMO at 4x costs the same as at 1x.

## Findings

1. **Mask rebuilds take about 2.4 ms in release builds on Windows:** a median of 2.42 ms (1.47–4.75 ms) for BMO with the pointer held on it, at `759205a6` (release build, 2026-10-26). The debug-build figures that follow came from older code at `a38ba3b`: ~15 ms for BMO walk (126×128, ~6300 opaque) at 1×; ~22 ms for BMO react (mixed 6360 and 7888 opaque); ~2.5 ms for Black Mage (37×33 source, ~570–630 opaque) at 3×. In those debug runs, cost is dominated by opaque source pixel count, not rendered size or scale factor.

2. **Rebuilds are gated on cursor-over:** With the pointer away, idle animation logs no rebuilds. Cost shows up when the user moves the cursor over the sprite.

3. **Walking under the cursor rebuilds at motion rate:** Because the cache key includes position, walking under the pointer triggers rebuilds for every position change, not only when the animation frame's opaque set changes. Windows walk measured ~44 rebuilds/sec during the ~5.0s walk bout under cursor-follow.

4. **Scale multiplies rendered size, not opaque count:** Black Mage at scale=3 has smaller source dimensions (37×33) than BMO@1× (126×128), so despite the 3× scale it rebuilds much faster (~2.5ms vs ~15-22ms). The iteration count is the source opaque cells; scale only affects the rectangle dimensions in the region.

5. **In debug builds, the Windows DWM rebuild is similar to X11 XShapeCombineMask for walk, slower for react:** These are the debug figures at `a38ba3b`. In release builds Windows is slower, 2.42 ms against X11's 0.148 ms (see [Release build (2026-10-26)](#release-build-2026-10-26)). Walk per-rebuild times are nearly identical (~14.5 ms Windows vs ~15 ms X11 for 6231–6298 opaque). React is slower on Windows (~22 ms vs ~10 ms); this may reflect DWM region API cost or measurement differences (Windows sample included more high-opaque frames).

## Evidence

Slim evidence pack in [`docs/research/mask-rebuild-baseline-windows/`](./mask-rebuild-baseline-windows/):

- [`environment.txt`](./mask-rebuild-baseline-windows/environment.txt) — host/OS/display/binary config
- [`idle-summary.txt`](./mask-rebuild-baseline-windows/idle-summary.txt) — idle measure summary (0 rebuilds in 12s)
- [`walk-summary.txt`](./mask-rebuild-baseline-windows/walk-summary.txt) — walk-attributed subset summary (223 rebuilds)
- [`fast-summary.txt`](./mask-rebuild-baseline-windows/fast-summary.txt) — BMO react summary (68 rebuilds)
- [`large-summary.txt`](./mask-rebuild-baseline-windows/large-summary.txt) — Black Mage @3x summary (6 rebuilds)
- [`walk-mask-rebuild-sample.log`](./mask-rebuild-baseline-windows/walk-mask-rebuild-sample.log) — sample `mask_rebuild:` log lines
- [`fast-mask-rebuild-sample.log`](./mask-rebuild-baseline-windows/fast-mask-rebuild-sample.log) — fast/react sample lines
- [`large-mask-rebuild-sample.log`](./mask-rebuild-baseline-windows/large-mask-rebuild-sample.log) — Black Mage sample lines
- [`README.md`](./mask-rebuild-baseline-windows/README.md) — pack provenance

Full TRACE logs remain on the measurement workstation.

### Code

- Windows: `src-tauri/src/platform/windows/overlay.rs` — timing / counters / SetWindowRgn
- Instrumentation: PR #981 tip `a38ba3b` — TRACE instrumentation
- `src-tauri/src/frame_loop.rs` — `MaskParams` cache + cursor-over gate

## Comparison to Issue #432 (System Wakeups)

Issue #432 measured system wakeups during idle on Linux. Windows idle with cursor away shows **0 rebuilds/sec**, so mask rebuilds are **not** a contributor to idle wakeup rate on Windows either. The rebuild time, a median of 2.42 ms in release at `759205a6` (~15–22 ms in the debug build at `a38ba3b`), matters when the cursor is over an animating/moving sprite.

### Instrumentation sample

**BMO walking under cursor:**
```
mask_rebuild: 126x128 @1x scale, 6251 opaque pixels, 14.43 ms
mask_rebuild: 126x128 @1x scale, 6251 opaque pixels, 14.51 ms
mask_rebuild: 126x128 @1x scale, 6251 opaque pixels, 14.56 ms
```

**BMO react (10 fps fast animation):**
```
mask_rebuild: 126x128 @1x scale, 6360 opaque pixels, 20.71 ms
mask_rebuild: 126x128 @1x scale, 7888 opaque pixels, 30.82 ms
mask_rebuild: 126x128 @1x scale, 6360 opaque pixels, 22.00 ms
```

**Black Mage (37×33 source @3x scale, rendered 111×99):**
```
mask_rebuild: 111x99 @3x scale, 579 opaque pixels, 2.03 ms
mask_rebuild: 111x99 @3x scale, 630 opaque pixels, 2.77 ms
mask_rebuild: 111x99 @3x scale, 579 opaque pixels, 2.64 ms
```

## Recommendations for Follow-Up

1. **Windows Performance Analyzer flamegraph** for rebuild hotspots — a WPR trace is recorded (see [Release build (2026-10-26)](#release-build-2026-10-26)) but no flamegraph image is exported yet.
2. **Sustained fast animation samples** — Current react measurements are from poke interactions (loop=once). Extended cursor-over with a hypothetical looping 10 fps animation would clarify sustained rate vs per-rebuild cost.
3. **Isolate DWM region update cost vs iteration cost** — Instrument the region construction separately from the DWM API call to understand the cost breakdown.
4. **Compare debug vs release builds** — Done for the cursor-held BMO case: median 2.42 ms in release at `759205a6`, against 14.5–22 ms in debug at `a38ba3b`. The code changed between the two tips, so not all of the gap is the build profile.
