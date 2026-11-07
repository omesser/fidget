# Windows Region Shape Reuse

Before and after numbers for [#1526](https://github.com/omesser/fidget/issues/1526). Each Character on an overlay keeps its recent region shapes. When a Character's art this frame has the same shape as one it built before, that region is moved into place instead of being built rect by rect. Anchored to `e2a1c745` (main, before) and the #1526 branch on top of it (after). The method and the pre-change baseline are in [`mask-rebuild-baseline-windows.md`](./mask-rebuild-baseline-windows.md).

## Environment

- **Host:** DESKTOP-UQIE144, the same workstation and displays as the baseline.
- **Builds:** release, `CARGO_TARGET_DIR=E:\workspace\target-1526-base` for main and `E:\workspace\target-1526` for the branch.
- **Script:** `scripts/bench-mask-rebuild-windows-scenarios.ps1 -Only idle,walk`, BMO@1x, unchanged from the baseline. It counts only `region rebuild` trace lines. A region with every Character's shape reused logs `region shift`, which this document counts from each run's `data\process.log`.
- **Runs:** main and an early 8-shape, one-cache-per-overlay build, alternating, 3 rounds each. A 32-shape build of the same design, 3 rounds. The shipped design, 32 shapes per Character, 3 rounds. The run folders stay on the workstation under `E:\workspace\fidget-1526-runs\` (`before-*`, `after-*`, `kept32-*`, `final-*`).

## Results (2026-11-07)

| Scenario | Build | Rebuilds/s (3 runs) | Rebuild ms, median | Shift ms, median |
|---|---|---|---|---|
| Idle, pointer away | main | 1.99–2.03 | 1.91–2.13 | n/a |
| Idle, pointer away | shipped | 0.36–0.55, all during warm-up | 1.82–1.86 | 0.98–1.16 |
| Walk, pointer on sprite | main | 62.62–62.63 | 1.73–1.75 | n/a |
| Walk, pointer on sprite | 8 per overlay | 3.58–20.06 | 1.74–1.97 | 1.07–1.12 |
| Walk, pointer on sprite | 32 per overlay | 0.95–1.56 | 1.88–2.00 | 1.06–1.07 |
| Walk, pointer on sprite | shipped | 1.56 | 1.93–2.02 | 1.09–1.13 |

## What sets each number

- **Why 32, not 8:** BMO's walk is 4 frames (`walk-0`, `walk-1`, `walk-2`, `walk-1`) at 8 fps. Each frame, and each tick where the `DrawTrail` still holds the previous frame, is its own shape. A sprite that moves a fraction of a pixel a tick alternates between two step sizes, and each step size sweeps a different shape. That makes more than 8 shapes a cycle, so an 8-entry least-recently-used cache missed on most frame changes: 20/s in two of three runs, about twice the walk's frame rate. 32 holds the whole cycle.
- **Why per Character:** one cache per overlay keys on the whole window's rects. Two Characters on one display that move independently almost never repeat that combined shape, so they would rebuild every tick and evict each other's regions. The shipped cache keys each Character's art apart, and the window region is the union of each Character's placed region and the renderer's rects. `characters_sharing_an_overlay_keep_their_own_regions` in `src-tauri/src/platform/windows/overlay.rs` checks that. The bench runs one Character, so the two-Character case is tested but not timed.
- **Idle reaches 0 rebuilds:** in every shipped idle run, the rebuilds came first, from the launch fall and landing, and after them only shifts. The bench's window still catches the warm-up, so its rate reads about 0.5/s.
- **What a shift still costs, one `SetWindowRgn`:** a one-run probe on the 32-per-overlay build timed the halves separately. Moving a kept region took 0.012 ms against 1.36 ms for building from rects. `SetWindowRgn` with the style update took 1.27–1.37 ms either way. A moving region still needs one `SetWindowRgn` per tick, so `SetWindowRgn` is now the limiter.
- **Walking under the pointer:** about 1.6 × 2.0 + 61 × 1.1, roughly 70 ms of a core per second, down from 109 ms on main. The 61 shifts a second is inferred from the tick rate, not counted inside the bench window.
- **Idle has no further skip in #1526:** the idle comment on #1526 asks whether a hover gate belongs there. The Windows region clips drawing as well as input, so it has to follow the art whatever the pointer does. A gate would cut off frames the renderer draws. Idle's remaining cost is 2 `SetWindowRgn` calls a second, about 2 ms of a core per second.

## Bench script note

`scripts/bench-mask-rebuild-windows-flame.ps1` stops at `-Target` region rebuilds. With shapes reused, a walk reaches that target only through warm-up and frame changes, so a run usually ends at `-Seconds` instead. Its rect statistics cover rebuilds only.

## Code

- `crates/core/src/overlay_region.rs`: `SpriteArt`, `placed_shape` and `RegionShapes`, the per-Character art, the shape key and the cache.
- `src-tauri/src/platform/windows/overlay.rs`: `window_region`, which moves each Character's kept region into place or builds it, and the `region rebuild` / `region shift` trace.
- `src-tauri/src/frame_loop.rs`: the Windows branch, which groups each Character's `DrawTrail::clip_rects` into a `SpriteArt`.
