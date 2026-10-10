# Idle frame cadence on one Mac

Anchor: `e72d1e10ff7c1b9a48af7096bd40be183ebe206b`. The release binary ran these app sources on 2026-10-10. It printed `git_rev=3883243a`, and no app file differs between that commit and this anchor. This document does not follow `main`.

## What was measured

One perched BMO, Mac15,7, macOS 27.0.1 (26A434), 12 CPUs. The main display is a DELL P2414H, 1920x1080 at 60 Hz. A built-in display was also online. The sprite stayed on the main display.

The launch was `scripts/bench-frame-cadence-macos.sh idle --seconds 60` against `target/release/fidget`, with `FIDGET_TRACE_CADENCE` and `FIDGET_TRACE_FRAMES`. The idle scenario uses the bench's still BMO copy, so a patrol does not start. No clicks, drags, or keystrokes. The sample window is 1791657062597 through 1791657122624, which is 60.027 s. Walk frames in the window: 0. All 2784 Engine ticks are `Perched` at one position. The clips are `idle` (1973), `sit` (488), and `talk` (323).

Release `process_log::init` writes stderr to `process.log` and does not copy it back to the terminal. The bench reads that file before it deletes the scratch home. `unwrapProcessLog` drops the epoch-second prefix `process_log::append` adds.

## Display frames

`noteCadence` records each animation-frame callback. In this window only `overlay-0` recorded any. There are 95 callbacks over 58.687 s. `summarizeTimestamps` turns those timestamps into the histogram below. The drop threshold is `DROP_MS`, 20 ms.

The average rate across every consecutive pair is 1.6 fps. 93 gaps are longer than 20 ms. 91 of those are longer than 50 ms.

| Delta ms | Callbacks |
| --- | --- |
| 0-10 | 0 |
| 10-14 | 1 |
| 14-18 | 0 |
| 18-20 | 0 |
| 20-25 | 0 |
| 25-34 | 1 |
| 34-50 | 1 |
| 50+ | 91 |

`arm` requests another frame only while the sprite has not arrived at the latest placement. A perched sprite is already there, so the loop stops. A gap after a frame that did not ask for the next one is idle time, not a missed vsync. `analyze` counts armed gaps only. This window has one armed gap, in the 25-34 ms bin, so one drop over 20 ms. The reciprocal of that single gap is 38.5 fps. That figure is not a sustained frame rate. The script's own table is the same count.

| Metric | Value |
| --- | --- |
| Display callbacks | 95 |
| Loop restarts | 93 |
| Armed gaps over 20 ms | 1 |
| Mean of armed gaps | 38.5 fps, from one gap |

The issue expected a steady 16.7 ms callback while idle. This sample does not show that. The overlay does not ask for a display frame once the sprite is still.

## Engine ticks

2784 ticks, all still. Mean gap 21.6 ms, 46.4 Hz. The loop's own counter for the same window is 46.4 Hz.

| Delta ms | Engine ticks |
| --- | --- |
| 0-10 | 0 |
| 10-14 | 1 |
| 14-18 | 322 |
| 18-20 | 422 |
| 20-25 | 1951 |
| 25-34 | 87 |
| 34-50 | 0 |
| 50+ | 0 |

`interpolate` says the Engine ticks at about 53 Hz with gaps of 14 ms to 25 ms, and that the renderer draws one sample behind. This window is slower than 53 Hz. Most still gaps do fall between 14 ms and 25 ms. 87 gaps are 25 ms to 34 ms.

Interpolation lag is absent. The sprite never changed position, and `analyze` does not invent a lag for a still sprite. This sample does not say whether a moving sprite stays one Engine tick behind.

## Not measured here

Walking, CPU load, riding a moving window, and a seam crossing were not run. Riding and the seam need a drag. Linux and Windows were not measured for this note. Those numbers are not here.
