# X11 Click-Through Mask Rebuild Baseline

Benchmark for issue [#428](https://github.com/omesser/fidget/issues/428): how often the X11 input region (XShapeCombineMask) is updated in each scenario, and what each update costs.

The per-scenario table comes from a release build at `b893e92c`; see [Per-scenario rates](#per-scenario-rates-2026-10-28). Numbers from debug builds before the shape cache and the position-only update (10–15 ms per BMO rebuild) are in this file's git history and no longer describe the app.

## Method

### Instrumentation

Timing in `src-tauri/src/platform/x11/overlay.rs`:

- Atomic counters for rebuild count and total nanoseconds
- Per-rebuild timing from start of `apply_input_mask()` to X11 flush completion
- Logging of sprite dimensions, scale, opaque pixel count, and rebuild time when `FIDGET_TRACE_MASK_REBUILD=1`

The rebuild happens in `apply_input_mask()`:

1. Create a 1-bit pixmap at `width * scale` by `height * scale`
2. Iterate every cell of the source mask
3. Call `poly_fill_rectangle` once per opaque source cell, as a `scale` by `scale` rectangle
4. Call `shape::mask` (XShapeCombineMask)
5. Optionally union hotspot rectangles with `shape::rectangles`
6. Flush X11

Scale multiplies rectangle size and pixmap size. It does not multiply the opaque source count.

### When updates run

`frame_loop` only calls `update_input_region` while the overlay is not click-through-ignored, i.e. while the cursor is over the sprite or a control, or during a hold. `MaskParams` holds the mask bits **and** the sprite `x,y`. An update whose bits match the cached pixmap only moves the shape ("position-only"); any other update rebuilds the pixmap ("full").

## Per-scenario rates (2026-10-28)

- **Build:** `cargo build --release -p fidget` at `b893e92c`.
- **Box:** Debian 13, kernel 6.12, 8 cores, Xvfb `:1` at 1920×1200×24. Load average 0.3–1.0 during the runs.
- **Script:** `scripts/measure-mask-rebuild-x11.sh [--scale N] SCENARIO TARGET DEADLINE`. It runs a copy of a shipped character whose only Behavior repeats the scenario's animation, and keeps the pointer on the sprite with `scripts/lib/follow-sprite.sh`. The rate counts updates over the time that animation was on screen. It waits on the app's log, so it never sleeps to poll.
- **Runs:** three interleaved rounds of all six rows. Round 1 of walk was lost because the script was edited while that run was reading it, so walk has two runs.
- **Evidence:** [`scenarios-release.txt`](./mask-rebuild-baseline-x11/scenarios-release.txt), every run's summary and per-kind statistics.

Each cell gives the range of per-run values; in the ms/update columns, that is the per-run medians, then the min–max and n over every update in every run. Per-update times are wall time on the client side, from the `mask_rebuild:` trace. "Mask ms/s" is the rate times each kind's median, so a preempted outlier does not inflate it.

| Scenario | Character | Opaque cells | Updates/s | Full ms/update: medians (min–max, n) | Position-only ms/update: medians (min–max, n) | Mask ms/s |
|---|---|---|---|---|---|---|
| Idle, pointer away | BMO 126×128@1x | n/a | 0 (0 in 3 × 10 s) | n/a | n/a | 0 |
| Walk, pointer on sprite | BMO 126×128@1x | 6231–6298 | 62.55–62.67 | 0.084–0.088 (0.073–0.164, 75) | 0.011 (0.007–0.059, 525) | 1.25–1.30 |
| Fast animation (react, 10 fps) | BMO 126×128@1x | 6360–7888 | 9.81–9.83 | 0.129–0.135 (0.094–0.597, 300) | none | 1.27–1.32 |
| Large (react at 4x, 504×512 on screen) | BMO 126×128@4x | 6360–7888 | 9.79–9.84 | 0.135–0.142 (0.097–2.227, 300) | none | 1.32–1.40 |
| Small (react, 12 fps), shipped scale | Nim 32×32@4x | 344–350 | 12.05–12.23 | 0.027–0.031 (0.018–0.082, 300) | none | 0.33–0.37 |
| Small at 1x | Nim 32×32@1x | 344–350 | 11.95–12.19 | 0.028–0.029 (0.022–0.095, 300) | none | 0.33–0.35 |

### What sets each number

- **Idle: 0.** The pointer is parked in the corner, so the cursor-over gate stays shut and `update_input_region` never runs.
- **Walk: 62.6/s, the frame loop's tick rate.** Each tick moves the sprite, and position is part of `MaskParams`, so every tick under the pointer is an update. Most are position-only. 12–13% are full, about the walk's 8 fps frame flips plus facing changes, which is 8 / 62.6. The per-second cost is about the same as the fast animation because the position-only updates are cheap.
- **Fast, large and small: the animation's fps.** A standing sprite only updates when its frame changes, so every update is full. React plays at 10 fps (BMO) and 12 fps (Nim), and the rates land within 2% of those. Frame changes wait for the next frame-loop tick, which accounts for the difference.
- **Cost per full update: opaque source cells.** BMO (about 6,300–7,900 cells) costs 0.13–0.14 ms and Nim (about 350 cells) 0.028 ms. Scale changes neither: BMO at 4x is within 0.01 ms of BMO at 1x, and Nim at 1x matches Nim at 4x. A straight line through BMO and Nim gives about 16–18 ns per cell plus about 0.02 ms per update. Position-only updates skip the per-cell work and take 0.011 ms.
- **Worst case: about 1.4 ms of client time per second**, 0.14% of one core, while the pointer rests on BMO at 4x. The X server's side is not in these numbers; see the [CPU profile](#cpu-profile).

### Noise

Per-run medians moved by at most 0.007 ms between rounds and rates by at most 0.25/s. The maxima are outliers from preemption: up to 2.2 ms for BMO at 4x and 0.6 ms at 1x. They pull BMO's means up to 0.1 ms above its medians; Nim's means sit within 0.003 ms. The box is a shared VM with software rendering, so an absolute time here is not a desktop's.

### Running it

The bench character turns off two things that would end a hovered scenario early. The Behavior list holds only the scenario's animation, and that animation's `loop = "once"` is dropped so react keeps cycling instead of waiting about 0.6 s for the next behaviour step. `FIDGET_HOVER_DELAY_MS=3600000` holds back the hover bubble. In the shipped app the bubble opens after 1.5 s of hover and stops a walk, so a hovered walk updates at the tick rate for 1.5 s at most.

```sh
cargo build --release -p fidget
DISPLAY=:1 scripts/measure-mask-rebuild-x11.sh idle 0 10
DISPLAY=:1 scripts/measure-mask-rebuild-x11.sh walk 300 20
DISPLAY=:1 scripts/measure-mask-rebuild-x11.sh fast 100 30
DISPLAY=:1 scripts/measure-mask-rebuild-x11.sh large 100 30
DISPLAY=:1 scripts/measure-mask-rebuild-x11.sh small 100 30
DISPLAY=:1 scripts/measure-mask-rebuild-x11.sh --scale 1 small 100 30
```

## Release build (2026-10-26)

- **Build:** `CARGO_PROFILE_RELEASE_DEBUG=line-tables-only cargo build --release -p fidget` at `2355abcb`. The line tables only add names for the profiler.
- **Box:** Debian 13, kernel 6.12, 8 cores, Xvfb `:1` at 1920×1200×24 with software rendering.
- **Scenario:** `scripts/flame-mask-rebuild-x11.sh` moves the pointer to the sprite centre on every frame line, so the cursor-over gate passes and the sprite mostly plays its hovered `talk` animation. The per-scenario table above uses the newer `scripts/measure-mask-rebuild-x11.sh`.
- **What the time covers:** the `mask_rebuild:` trace, from the start of `apply_input_mask()` to the flush, on the client side. The X server handles the requests asynchronously, outside that window. The profile below covers both sides.
- **Evidence:** [`release-summary.txt`](./mask-rebuild-baseline-x11/release-summary.txt) and [`flamegraph-x11-mask.svg`](./mask-rebuild-baseline-x11/flamegraph-x11-mask.svg).

### Timings

There were five alternating rounds over four characters, 100 updates per run, without perf. Each "full" update rebuilds the mask. A "position-only" update keeps the cached pixmap and only moves the shape.

| Character | Opaque cells | Full: median per run (ms) | Full: pooled median / p95 (ms) | Position-only: pooled median (ms) |
|---|---|---|---|---|
| BMO 126×128@1x | 6219–7888 | 0.143–0.154 | 0.148 / 0.529 | 0.013 (n=161) |
| Black Mage 37×33@3x | 567–630 | 0.038–0.041 | 0.040 / 0.062 | 0.011 (n=161) |
| Nim 32×32@4x (shipped) | 278–350 | 0.034–0.035 | 0.035 / 0.054 | 0.011 (n=141) |
| Nim 32×32@1x | 278–350 | 0.033–0.036 | 0.034 / 0.046 | 0.011 (n=33) |

Every character had a few outliers between 5 and 6.4 ms, which looks like scheduling noise on the box. Updates arrived at about 4 per second.

### CPU profile

`perf record -e cpu-clock:u -F 4999 --call-graph dwarf` sampled fidget and Xvfb over 200 BMO updates in 69 s (user space only, since `perf_event_paranoid` is 2). `apply_input_mask()` is inlined, so `update_input_region` is the root of the mask path:

| Frame at or below `update_input_region` | On-CPU |
|---|---|
| `update_input_region` total | 25.8 ms (1.26% of fidget's 2054.8 ms, about 0.13 ms per update) |
| `x11rb` `poly_fill_rectangle` (mostly serializing the rectangles) | 8.2 ms |
| `mask_rects` (5.2 ms of it in `realloc`, from `Vec::new` growing) | 6.8 ms |
| tauri `window_handle` (event-loop round trip for the X window id) | 4.0 ms |
| `shape::mask` (the XShapeCombineMask request) | 1.4 ms |
| `_eprint` (the trace line itself) | 1.0 ms |
| `shape::rectangles` (hotspot union) | 0.6 ms |

On the server side, Xvfb spent 5405.9 ms on-CPU, and 4874.8 ms of that was pixman painting the webview into the software framebuffer. The shape-named server frames were `miSetShape` (3.2 ms) and `SendShapeNotify` (2.4 ms). The packaged Xvfb is partly stripped, so `ProcShapeMask` and its region conversion show up as `[Xvfb]`. Under perf, the trace mean rose to 0.248 ms.

The XShapeCombineMask call costs little on either side. What a mask rebuild does cost is building and sending one rectangle per opaque cell, plus the window-handle round trip. Even so, it stays near 1% of the app's CPU while the pointer is held on BMO.

## Evidence

Slim evidence pack in [`docs/research/mask-rebuild-baseline-x11/`](./mask-rebuild-baseline-x11/):

- [`scenarios-release.txt`](./mask-rebuild-baseline-x11/scenarios-release.txt): per-scenario runs (2026-10-28)
- [`release-summary.txt`](./mask-rebuild-baseline-x11/release-summary.txt): release-build timing series and CPU profile (2026-10-26)
- [`flamegraph-x11-mask.svg`](./mask-rebuild-baseline-x11/flamegraph-x11-mask.svg): fidget stacks through `update_input_region`, 200 BMO updates
- [`README.md`](./mask-rebuild-baseline-x11/README.md): pack provenance

`environment.txt`, `idle-summary.txt`, `walk-summary.txt` and `walk-mask-rebuild-sample.log` in the pack are from the debug-build runs.

### Code

- `src-tauri/src/platform/x11/overlay.rs`: timing and counters
- `src-tauri/src/frame_loop.rs`: `MaskParams` cache and the cursor-over gate
- `scripts/measure-mask-rebuild-x11.sh`: per-scenario rate and cost
- `scripts/flame-mask-rebuild-x11.sh`: release timing (`--no-perf`) and the perf flamegraph
- `scripts/bench-mask-rebuild-x11.sh`: debug-era idle and walk capture
- `scripts/bench-mask-rebuild-windows-flame.ps1` and `scripts/export-mask-flame-windows.ps1`: WPR CPU trace of the Windows region rebuild, and its flamegraph

## Comparison to Issue #432 (System Wakeups)

Issue #432 measured ~271 voluntary context switches/sec during idle perched on Linux (cloud). Mask updates at 0/sec with the cursor away are **not** a contributor to that idle wakeup rate. With the cursor on the sprite, they add at most about 60 short calls per second.

## Follow-up

1. **Windows:** release rates and ms per rebuild for every scenario are in [`mask-rebuild-baseline-windows.md`](./mask-rebuild-baseline-windows.md#per-scenario-rates-release-2026-10-30). Windows has no hover gate and rebuilds twice per frame flip, so its idle is about 2/s and its standing rates twice X11's. Its flamegraph from `scripts/export-mask-flame-windows.ps1` is not exported yet.
2. **If X11 updates ever get hot:** the [CPU profile](#cpu-profile) points at reserving `mask_rects` capacity and caching the window handle. At about 1.4 ms per second at worst, neither is worth doing yet.
