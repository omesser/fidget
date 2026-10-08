# Performance baseline v1

Rollup for [#423](https://github.com/omesser/fidget/issues/423). Each number below comes from a script run or a merged pull request, or the row says unavailable and why.

Anchor: `31d2f245`. The architecture section names symbols from that tree. Linux numbers were measured on `target/release/fidget` built from that tree. The bench scripts on this branch do not change that binary. The cadence script printed `git_rev=57d12388` because that was `HEAD` during the run. Rust sources at `57d12388` match the anchor.

Since that anchor, macOS riding calls `SnapshotAssembler::detach_poll`. The window-list read runs on a thread named `window-poll`, and the tick copies the last finished sample. This branch did not run a new `riding` bench. `riding` and `matrix` in `scripts/bench-window-list-macos.sh` still need `FIDGET_BENCH_GREEN_LIGHT=1`.

Fidget is the product. A fidget is one running instance. Character stays the name of a package such as BMO.

## What to optimize first

The rank is milliseconds of one core per second in the scenario that was measured. A cost that only happens while the pointer is on the sprite, or only while riding, is ranked on that rate.

1. **Software paint of a moving overlay on this VM:** A 60 s read of `/proc`, pointer at (2, 2), held the heavier `WebKitWebProcess` at 83.8% of one core across 1475 walk frames and 612 climb frames. That is 838 ms of one core per second. Xtigervnc in that same window was 36.2%. The scripted 15 s walk from `scripts/bench-gpu-compositing-linux.sh`, 495 walk frames with the pointer away, held Xtigervnc at 54.5%, which is 545 ms per second. That script does not sample the fidget tree. Held-still idle on the same machine held the heavier `WebKitWebProcess` at 5.4% and Xtigervnc at 3.0%. GPU% is unavailable. `glxinfo -B` reports `llvmpipe` and acceleration off, and there is no DRM device. A Mac with a GPU measured 0.6% GPU and 0.06 W for the perched case, in the macOS GPU section.

2. **Click-through mask rebuild while the pointer is on a moving sprite:** On the Grok Bot X11 desktop, a BMO walk under the cursor rebuilt at 26.7/s and 15.0 ms per call. Computed from those two published figures, that is 400 ms of one core per second of that walk. On the Windows workstation the walk-attributed subset was 44.4/s at 14.54 ms, which is 646 ms of one core per second. Both machines recorded 0.0/s with the pointer away from the sprite. Sources are [mask-rebuild-baseline-x11.md](./mask-rebuild-baseline-x11.md) from [#968](https://github.com/omesser/fidget/pull/968) and [mask-rebuild-baseline-windows.md](./mask-rebuild-baseline-windows.md) from [#983](https://github.com/omesser/fidget/pull/983). This VM's release build, same opaque counts of 6290 to 7888, logged 11 rebuilds at 2.187 ms to 2.904 ms, mean 2.55 ms. The 5 s pointer-on-sprite window counted 8 of them, 1.60/s, and the walk aborted after 12 walk frames. Per-call time depends on the machine. The desktop rates are the ones that spend a large fraction of a core. Idle with the pointer away stays at 0.

3. **macOS WindowSource poll while riding, once many windows are open:** Riding at 156 windows polled at 40.2 Hz with a median of 2.79 ms. The WindowSource section computes 112 ms of one core per second from that row, and the p95 was 10.8 ms. Riding at 56 windows was 68 ms per second. The sweep at 351 windows, idle only, had a p95 of 14.6 ms and a max of 39 ms. Idle polls at about 10 Hz, so one of those polls stalls one tick. Sources are the WindowSource section below, from [#1042](https://github.com/omesser/fidget/pull/1042) and [#1128](https://github.com/omesser/fidget/pull/1128). A quiet desk at about 50 windows is about 21 ms per second, which would not make this list.

macOS idle host CPU ranks under those three. On a quiet Mac15,7, release build of `ecb92b8d`, four interleaved rounds, a perched fidget used 8.4% of one core, range 8.0 to 8.5. That is 84 ms of one core per second for as long as the fidget is perched, with 179.8 interrupt wakeups/s, range 178.9 to 186.4. Package-idle wakeups were 2.32/s, range 1.77 to 2.44. The per-thread census after the display-link fixes names the frame loop at about 35/s and the host `libpas` scavenger at about 27/s. `powermetrics` counted 187 interrupt wakeups/s for the host pid in that census launch. Source is [macos-idle-wakeups.md](./macos-idle-wakeups.md), quiet-machine table and the census under #761, from [#960](https://github.com/omesser/fidget/pull/960) and [#843](https://github.com/omesser/fidget/pull/843). Cluster idle residency stayed below what the instrument resolved. The same research file records that.

GPU compositing of the transparent overlay, the other suspect in #423, measured 0.6% GPU and 0.06 W on that Mac while perched, the same wattage as the desktop with no fidget running. The section below is [#1034](https://github.com/omesser/fidget/pull/1034).

## Where each child stands

| Issue | State | Result |
| --- | --- | --- |
| [#431](https://github.com/omesser/fidget/issues/431) macOS idle | Closed | Host 8.4% CPU and 179.8 interrupt wakeups/s perched, quiet machine. Detail in [macos-idle-wakeups.md](./macos-idle-wakeups.md). |
| [#432](https://github.com/omesser/fidget/issues/432) Linux idle | Open | Release idle, walking, and hidden are below. C-state residency unavailable on this VM. Chat was not sampled. |
| [#429](https://github.com/omesser/fidget/issues/429) macOS GPU | Closed | Idle 0.6% GPU, 0.06 W. Section below, [#1034](https://github.com/omesser/fidget/pull/1034). |
| [#430](https://github.com/omesser/fidget/issues/430) Windows GPU | Closed | Section below, [#1025](https://github.com/omesser/fidget/pull/1025). xperf frame time was not measured. |
| [#425](https://github.com/omesser/fidget/issues/425) Linux GPU | Open | GPU% unavailable. Release X server proxy measured below. Wayland, a second compositor, and uncomposited X11 were not running. |
| [#427](https://github.com/omesser/fidget/issues/427) WindowSource | Closed | Section below, [#1042](https://github.com/omesser/fidget/pull/1042) and [#1128](https://github.com/omesser/fidget/pull/1128). |
| [#428](https://github.com/omesser/fidget/issues/428) mask rebuild | Open | Desktop numbers in the mask docs. This VM adds release per-call times. No `perf` flamegraph. |
| [#424](https://github.com/omesser/fidget/issues/424) RSS | Open | macOS footprint in [memory-rss-and-multi-monitor.md](./memory-rss-and-multi-monitor.md). This VM has one display. heaptrack was not installed. |
| [#426](https://github.com/omesser/fidget/issues/426) frame cadence | Open | macOS, Linux release, Linux ride, Windows matrix, and a Windows dual-display seam ride are below. A display seam on Linux was not measured. |

## How a frame gets on screen

The frame loop `run_frame_loop` waits with `scheduler::next_tick`. `ENGINE_TICK` is 16 ms. `scheduler::mode` returns `ScheduleMode::Active` while the fidget is visible and a Behavior is playing, and while it is falling, dragged, or climbing. It returns `ScheduleMode::Idle` when the fidget is hidden, asleep, or visible and still with nothing playing. `scheduler::moving` is true when velocity is non-zero, or the state is falling, dragged, climbing, or riding. A moving Active tick counts from the last deadline. A still Active tick counts from the wake.

Each display is one transparent webview. `arm` in `src/main.js` calls `requestAnimationFrame` only when `draw` still has a placement to interpolate. `FRAME_RESEND` is 250 ms. The frame loop resends an unchanged placement on that interval so a webview that just started listening still hears `visible`.

On macOS, `WindowSource` reads the on-screen window list every `POLL_INTERVAL`, 100 ms, and every `RIDE_POLL_INTERVAL`, 16 ms, while any fidget is riding. On X11 and Windows the click-through region is rebuilt only while the pointer is over the sprite. The cache key includes position, so a walk under the pointer rebuilds as the sprite moves.

## Linux measurement on this VM

Host is Ubuntu 24.04.4 LTS, kernel 6.12.94+, 4 vCPU, X11 on `DISPLAY=:1` via Xtigervnc, one screen 1920x1200 at 60 Hz, compositor `xfwm4` with compositing on and `vblank_mode` auto. `WAYLAND_DISPLAY` unset. No `/dev/dri`. `intel_gpu_top` found no i915 device. `radeontop` found no DRM device. `glxinfo -B` reported `llvmpipe (LLVM 20.1.2, 256 bits)` and `Accelerated: no`. Character package BMO, sprite 126x128. Release binary. There is no `/sys/devices/system/cpu/cpu0/cpuidle`. `powertop` 2.15 wrote a CSV whose overview wakeup column is blank and whose processor idle-state table is empty. `modprobe cpufreq_stats` failed. C-state residency is unavailable here.

Held-still idle used a copy of BMO whose Behavior weights are 0 except `fidget`. The copy is how the cadence bench keeps StaticDirector from walking. The GPU script's own `idle` row does not do that. One default `idle` window happened to contain no `walk` frames and is listed separately.

### Idle wakeups, #432

60 s windows. CPU% is utime plus stime from `/proc/<pid>/stat`, as a percent of one core. Wakeups are voluntary context switches from `/proc/<pid>/status`, divided by the same window. Baseline X and `xfwm4` are two sequential 60 s windows with no fidget running. The idle window is one 60 s window after the overlay line, with the pointer parked at x=2, y=2. The process log for that launch has 3744 `idle` frames, 39 `talk`, 38 `land`, 26 `fall`, and zero `walk` frames. The spawn fall can overlap the start of the window. No walk frame exists anywhere in that log.

| Process | CPU% of one core | Voluntary switches/s |
| --- | --- | --- |
| Xtigervnc, no fidget | 0.0 | 0.2 |
| xfwm4, no fidget | 0.0 | 0.1 |
| fidget, idle perched | 2.3 | 146.9 |
| WebKitNetworkProcess | 0.0 | 0.0 |
| WebKitWebProcess, lighter | 0.0 | 2.4 |
| WebKitWebProcess, heavier | 5.4 | 16.4 |
| Xtigervnc, during that idle window | 2.3 | 353.3 |
| xfwm4, during that idle window | 0.0 | 8.5 |

The four fidget processes together are 7.7% of one core and 165.7 voluntary switches/s. The earlier debug capture further down in this file reported about 271 voluntary switches/s and about 3% CPU for one pid, without a still-only Character. It is a different scenario.

Walking and hidden use the same `/proc` reads for 60 s. Walking starts after a `walk#` frame, pointer at (2, 2). Hidden starts after `presence: hidden over 500ms`, under a fullscreen terminal. The walking window contained 1475 walk frames and 612 climb frames. Frames in the hidden window are about 1 s apart (15 walk, 33 idle, 6 land, 6 talk). Chat was not sampled.

| Process | Walking CPU% | Walking voluntary/s | Hidden CPU% | Hidden voluntary/s |
| --- | --- | --- | --- | --- |
| fidget | 6.3 | 322.8 | 0.5 | 16.3 |
| WebKitNetworkProcess | 0.0 | 0.2 | 0.0 | 0.2 |
| WebKitWebProcess, lighter | 0.0 | 2.3 | 0.0 | 2.3 |
| WebKitWebProcess, heavier | 83.8 | 240.9 | 2.5 | 51.4 |
| Xtigervnc | 36.2 | 427.9 | 1.0 | 39.4 |
| xfwm4 | 0.6 | 301.1 | 0.0 | 4.9 |

The four fidget processes while walking are 90.1% of one core and 566.2 voluntary switches/s. While hidden they are 3.0% and 70.2 voluntary switches/s. These two windows are a shell sample of `/proc`. Multi-monitor was not sampled. The host has one display.

### GPU proxy and mask rate, #425 and #428

`scripts/bench-gpu-compositing-linux.sh matrix --seconds 15 --bin target/release/fidget`. GPU% is N/A on every row. Chat is unavailable this run. Three attempts, one of them on the still Character, each logged `verbs: ... [Poke]` and none logged `Summon`.

| Scenario | Mask calls | Mask Hz | xfwm4 CPU% | Xtigervnc CPU% | Notes |
| --- | --- | --- | --- | --- | --- |
| Baseline, no fidget | N/A | N/A | 0.0 | 0.0 | |
| Idle, default script, no walk frames in the log | 0 | 0.00 | 0.1 | 4.9 | 1071 idle frames, plus land and react |
| Idle, still Character, pointer at (2, 2) | 0 | 0.00 | 0.0 | 3.0 | 1074 idle frames, plus the spawn fall, land, and 7 talk |
| Walking, pointer away | 0 | 0.00 | 0.9 | 54.5 | 495 walk frames |
| Pointer on sprite, 5 s | 8 | 1.60 | 0.0 | 5.0 | 12 walk frames, then the walk aborted |
| Chat | N/A | N/A | N/A | N/A | Double-click did not log Summon |
| Multi-monitor | N/A | N/A | N/A | N/A | xrandr reports 1 display |
| Hidden, fullscreen terminal | 0 | 0.00 | 0.1 | 1.2 | Log line `presence: hidden` |
| Wayland | N/A | N/A | N/A | N/A | No Wayland display |
| Mutter, KWin, uncomposited X11 | N/A | N/A | N/A | N/A | Not running |

A second default `idle` window included 516 walk frames and 358 climb frames, and Xtigervnc read 49.7% CPU. That row is a motion sample that the script labeled idle. The held-still number is the 3.0% row.

The walking log contains 11 `mask_rebuild:` lines, 2.187 ms to 2.904 ms, mean 2.55 ms, opaque counts 6290 to 7888. The script's 8 calls are the 5 s window. The other lines are earlier in the same process. Per-call time on the Grok Bot desktop for a similar opaque count was about 15 ms. Both figures are measured. They are different machines.

### RSS, #424

`scripts/bench-rss-linux.sh --settle 30 --seconds 45 --interval 3 --bin target/release/fidget`, still Character, one display. The run did not set `HOME` to a scratch directory. Median of 15 samples.

| Roster | Total RSS median | fidget RSS median | fidget VmHWM |
| --- | --- | --- | --- |
| `bmo:One` | 846 MB | 210 MB | 209.8 MB |
| `bmo:One,bmo:Two,bmo:Three,bmo:Four` | 816 MB | 210 MB | 210.4 MB |

The log line for the second run is `BMO as One, BMO as Two, BMO as Three, BMO as Four`. The Rust peak did not move. Total RSS did not rise. Each launch had four processes, the main pid plus `WebKitNetworkProcess` plus two `WebKitWebProcess`. One display, so a second overlay was not measured. heaptrack was not installed, so this run has no heap profile. The macOS peak footprint for one fidget on two displays is 583 MB in [memory-rss-and-multi-monitor.md](./memory-rss-and-multi-monitor.md), debug build, 300 s settle. These RSS figures are a different OS and a shorter settle, so they do not extend that curve.

### Frame cadence, #426

`scripts/bench-frame-cadence-macos.sh` with `FIDGET_BENCH_GREEN_LIGHT=1`, `--seconds 20`, `--bin target/release/fidget`. The machine header printed `x86_64`, Ubuntu 24.04.4 LTS, refresh 60.00, 4 cpus, `git_rev=57d12388`. The X authority export this branch adds was already in the working tree. That is why the script could open the display.

The first `walking` and `load` windows played a walk animation at a single position, `pos(960,1200)`. Those windows are not moving-sprite samples. The walking and load rows below are the reruns, which changed position. Unique positions were 928 and 978. Idle is the matrix run. Walk frames in that idle window were 0.

| Scenario | Display frames | Mean fps of armed stretches | Drops over 20 ms | Restarts | Engine Hz, loop counter | Moving ticks | Still ticks | Lag p50, moving | Lag p95, moving |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Idle, still Character | 58 | N/A | 0 | 57 | 62 | 0 | 1240 at 16.1 ms, 62 Hz | N/A | N/A |
| Idle, `FIDGET_TRACE_FRAMES` off | 22 | 52.6 | 0 | 20 | 62.1 | untraced | untraced | N/A | N/A |
| Walking, position changed | 801 | 60 | 4 | 140 | 62.3 | 814 at 16.1 ms, 61.9 Hz | 433 at 15.9 ms, 63.1 Hz | 16 ms, 1 sample | 21 ms, 1.14 samples |
| Walking plus one `yes` per core | 539 | 34.9 | 371 | 105 | 61.9 | 896 at 16.5 ms, 60.6 Hz | 343 at 15.2 ms, 65.6 Hz | 18 ms, 1 sample | 29 ms, 1.71 samples |

Mean fps counts only consecutive frames where the first asked for the second. Idle's 57 restarts in 58 frames means the loop did not stay armed. The engine still ticked at 62 Hz. On this VM a still Active tick and a moving tick both sit near 16 ms. The macOS capture further down measured a 16 ms sleep returning in about 20 ms, and still ticks near 53 Hz. That overshoot did not show up in these Linux tick gaps.

Under load, 371 of the armed gaps exceeded 20 ms, and the engine stayed at 61.9 Hz. The present path dropped frames while the engine did not. This is one software-rendered VM with all four CPUs in `yes`. This matrix did not ride a window, and it did not cross a display seam. The ride is the next section. The seam is still unmeasured. This VM has one screen.

Armed-stretch histogram for the moving walking window, display frames then engine ticks. 0 to 10 ms is 1 and 44. 10 to 14 is 5 and 63. 14 to 18 is 511 and 909. 18 to 20 is 126 and 141. 20 to 25 is 16 and 79. 25 to 34 is 0 and 9. 34 to 50 is 1 and 2. 50 and above is 0 and 0.

Armed-stretch histogram for the load window. 0 to 10 ms is 2 and 152. 10 to 14 is 5 and 143. 14 to 18 is 27 and 571. 18 to 20 is 15 and 108. 20 to 25 is 84 and 162. 25 to 34 is 201 and 95. 34 to 50 is 94 and 8. 50 and above is 5 and 0.

### Riding a window, #426

Release binary built at `543dad19` on this VM, recorded in [#1263](https://github.com/omesser/fidget/pull/1263). `x86_64`, Ubuntu 24.04.4 LTS, 4 cpus, `DISPLAY=:1`, one Xtigervnc screen at 1920x1200 and 60 Hz. Still BMO, the same weight rewrite the cadence script uses, `FIDGET_DIRECTOR=0`, `FIDGET_TRACE_FRAMES=1`, `FIDGET_TRACE_CADENCE=1`, `FIDGET_INSTANCES=BMO`, `HOME` set to a scratch directory. An `xterm` titled `perch-prop`, 1100 by 180, was placed under the spawn so the sprite landed on its top edge. After a `Perched` frame, `xdotool windowmove` stepped that window 2 px about every 20 ms and reversed within 180 px of the start, for 20 s. That stays under the 1000 pt/s yank.

The sample window held 392 `hold#` frames. Every one of them is at y=806, the frame top `xdotool` reported. x runs from 769 to 1177, in 262 distinct `pos()` values. `scripts/frame-cadence.mjs` reduced that window.

| Scenario | Display frames | Mean fps of armed stretches | Drops over 20 ms | Restarts | Engine Hz, loop counter | Moving ticks | Still ticks | Lag p50, moving | Lag p95, moving |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Riding a gliding window | 546 | 62.2 | 1 | 146 | 62.1 | 393 at 17.5 ms, 57 Hz | 832 at 15.4 ms, 64.9 Hz | 17 ms, 1 sample | 114 ms, 1.07 samples |

Armed-stretch histogram, display frames then engine ticks. 0 to 10 ms is 0 and 47. 10 to 14 is 0 and 68. 14 to 18 is 398 and 936. 18 to 20 is 0 and 88. 20 to 25 is 0 and 61. 25 to 34 is 1 and 24. 34 to 50 is 0 and 1. 50 and above is 0 and 0.

One armed gap exceeded 20 ms. The engine counter stayed at 62.1 Hz. Lag p50 is one sample, 17 ms. Lag p95 is 114 ms at 1.07 samples. This is one glide on one software-rendered display. A display seam was not measured.

### Windows frame cadence, #426

`scripts/bench-frame-cadence-windows.ps1 matrix -Seconds 20 -Bin target\release\fidget.exe` with `FIDGET_BENCH_GREEN_LIGHT=1` on DESKTOP-UQIE144 (MS-7D25). The machine header printed `Microsoft Windows 11 Pro for Workstations`, refresh 59, 20 cpus, `git_rev=b0f57a78`. Release binary built from that tip. The tables below were read from the script printout and placed in the Linux column layout. They are not the raw printout.

| Scenario | Display frames | Mean fps of armed stretches | Drops over 20 ms | Restarts | Engine Hz, loop counter | Moving ticks | Still ticks | Lag p50, moving | Lag p95, moving |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Idle, still Character | 20 | N/A | 0 | 19 | 61.2 | 0 | 1224 at 16.3 ms, 61.2 Hz | N/A | N/A |
| Idle, `FIDGET_TRACE_FRAMES` off | 23 | 59.9 | 0 | 21 | 61.1 | untraced | untraced | N/A | N/A |
| Walking | 1199 | 60 | 0 | 18 | 62.5 | 1249 at 16 ms, 62.5 Hz | 0 | 16 ms, 1 sample | 17.1 ms, 1 sample |
| Walking plus one `yes` per core | 1199 | 60 | 0 | 15 | 62.5 | 1249 at 16 ms, 62.5 Hz | 1 at 6 ms, 166.7 Hz | 16 ms, 1 sample | 16.4 ms, 1 sample |

Idle stayed mostly unarmed (19 restarts in 20 display frames) while the engine still ticked at 61.2 Hz. Walking and load held 60 fps on armed stretches with zero armed display gaps over 20 ms. Engine ticks still stretch: walking has 1 tick from 20 to 25 ms; load has 12 from 20 to 25 ms and 1 from 25 to 34 ms. Lag p50 stayed one sample (16 ms) on both moving windows. This matrix did not ride a window. The seam ride is the next subsection.

Armed-stretch histogram for the walking window, display frames then engine ticks. 0 to 10 ms is 0 and 0. 10 to 14 is 0 and 3. 14 to 18 is 1180 and 1231. 18 to 20 is 0 and 14. 20 to 25 is 0 and 1. 25 to 34 is 0 and 0. 34 to 50 is 0 and 0. 50 and above is 0 and 0.

Armed-stretch histogram for the load window. 0 to 10 ms is 0 and 2. 10 to 14 is 0 and 14. 14 to 18 is 1183 and 1212. 18 to 20 is 0 and 9. 20 to 25 is 0 and 12. 25 to 34 is 0 and 1. 34 to 50 is 0 and 0. 50 and above is 0 and 0.

#### Windows seam ride, #426

Same release binary and host as the matrix above (`git_rev=b0f57a78`), two displays: `\\.\DISPLAY1` at (-1200, -209) 1200x1920 and primary `\\.\DISPLAY2` at (0, 0) 3440x1440, seam at x=0. Still BMO, the same weight rewrite the cadence script uses, `FIDGET_DIRECTOR=0`, `FIDGET_TRACE_FRAMES=1`, `FIDGET_TRACE_CADENCE=1`, `FIDGET_INSTANCES=BMO`, scratch `APPDATA` / `USERPROFILE` / `HOME`. A WinForms perch 900 by 220 was placed under the primary-center spawn so the sprite landed `Perched`. After that, `SetWindowPos` stepped the perch 2 px about every 20 ms from the spawn under the left edge of the primary, across the seam, and about 520 px onto the left display, then reversed, for 60 s. That matches the Linux ride step and stays under the 1000 pt/s yank.

The sample window held 1004 `hold#` frames and 3696 `Perched` frames. `pos()` x ran from -303 to 1720 (529 frames with x < 0, 177 with |x| ≤ 50), in 877 distinct `pos()` values. `scripts/frame-cadence.mjs` reduced that window. The numbers below were read from that printout and placed in the Linux column layout. They are not the raw printout. The 1850 display frames are the analyzer's pooled count across every overlay in the window (two series), not one overlay alone; armed gaps plus restarts equal display frames minus 2.

| Scenario | Display frames | Mean fps of armed stretches | Drops over 20 ms | Restarts | Engine Hz, loop counter | Moving ticks | Still ticks | Lag p50, moving | Lag p95, moving |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Riding across the display seam | 1850 | 58.8 | 28 | 482 | 61.6 | 1004 at 16.3 ms, 61.5 Hz | 2691 at 16.2 ms, 61.6 Hz | 89.7 ms, 1 sample | 115.7 ms, 1 sample |

Armed-stretch histogram, display frames then engine ticks. 0 to 10 ms is 0 and 0. 10 to 14 is 0 and 32. 14 to 18 is 1338 and 3433. 18 to 20 is 0 and 212. 20 to 25 is 0 and 18. 25 to 34 is 14 and 0. 34 to 50 is 14 and 0. 50 and above is 0 and 0.

Compared with the single-display walking matrix on the same machine (0 drops, 60 fps, lag p50 16 ms), this seam ride kept the engine near 61.6 Hz but armed display gaps over 20 ms rose to 28, mean armed fps fell to 58.8, and lag p50 rose to 89.7 ms. Restarts were 482 in 1850 display frames. A Linux seam was not measured.

## How to reproduce the Linux numbers

Build the release binary, then run the blocks from `/workspace` with `DISPLAY=:1`. The still Character is required for a perched idle that does not walk.

```bash
cargo build --release -p fidget --bin fidget
dir=/tmp/still-characters
rm -rf "$dir" && mkdir -p "$dir" && cp -R characters/bmo "$dir/bmo"
awk '/^\[/ { section = $0 }
  /^weight = / && section ~ /^\[behaviors\./ && section != "[behaviors.fidget]" { $0 = "weight = 0" }
  1' characters/bmo/character.manifest > "$dir/bmo/character.manifest"
```

GPU matrix, then held-still idle:

```bash
DISPLAY=:1 scripts/bench-gpu-compositing-linux.sh matrix --seconds 15 --bin target/release/fidget --out /tmp/fidget-bench-425
DISPLAY=:1 FIDGET_CHARACTERS=/tmp/still-characters FIDGET_INSTANCES=BMO \
  scripts/bench-gpu-compositing-linux.sh idle --seconds 15 --bin target/release/fidget --out /tmp/fidget-bench-425-still
```

RSS, one fidget and four:

```bash
DISPLAY=:1 FIDGET_DIRECTOR=0 FIDGET_TRACE_ENGINE=1 FIDGET_CHARACTERS=/tmp/still-characters FIDGET_INSTANCES='bmo:One' \
  scripts/bench-rss-linux.sh --settle 30 --seconds 45 --interval 3 --bin target/release/fidget --out /tmp/fidget-rss-1.tsv
DISPLAY=:1 FIDGET_DIRECTOR=0 FIDGET_TRACE_ENGINE=1 FIDGET_CHARACTERS=/tmp/still-characters \
  FIDGET_INSTANCES='bmo:One,bmo:Two,bmo:Three,bmo:Four' \
  scripts/bench-rss-linux.sh --settle 30 --seconds 45 --interval 3 --bin target/release/fidget --out /tmp/fidget-rss-4.tsv
```

Cadence. The matrix idle row is the idle sample. Rerun `walking` and `load` when the first window stays on one `pos(...)`.

```bash
DISPLAY=:1 FIDGET_BENCH_GREEN_LIGHT=1 scripts/bench-frame-cadence-macos.sh matrix --seconds 20 --bin target/release/fidget --out /tmp/fidget-cadence
DISPLAY=:1 FIDGET_BENCH_GREEN_LIGHT=1 scripts/bench-frame-cadence-macos.sh walking --seconds 20 --bin target/release/fidget --out /tmp/fidget-cadence-walk2
DISPLAY=:1 FIDGET_BENCH_GREEN_LIGHT=1 scripts/bench-frame-cadence-macos.sh load --seconds 20 --bin target/release/fidget --out /tmp/fidget-cadence-load2
```

Idle wakeups. Sample Xtigervnc for 60 s with no fidget, then launch the release binary with the still Character, park the pointer at x=2, y=2, and sample the main pid plus `pgrep -P` children for 60 s. CPU% is `utime+stime` from `/proc/<pid>/stat` over `CLK_TCK`. Voluntary switches are the delta of `voluntary_ctxt_switches` in `/proc/<pid>/status`. The idle table did those two reads in one window for the fidget tree, and a separate 60 s window for X before launch. Walking used the same 60 s read after a `walk#` frame, pointer at (2, 2), with `FIDGET_TRACE_FRAMES=1`. Hidden used it after `presence: hidden over 500ms`, under `xfce4-terminal --fullscreen`. `sudo powertop --time=10 --csv=/tmp/powertop.csv` prints a blank wakeup column and an empty idle-state table on this VM.

## Earlier captures

The tables below are the captures already on main. Linux idle here is the debug VM run from [#927](https://github.com/omesser/fidget/pull/927). The release measurement is the section above.

## Earlier capture, Linux idle (#432)

**Environment:**
- Ubuntu 24.04.4 LTS (Noble)
- Kernel 6.12.94+ (cloud VM, not bare metal laptop)
- X11 via Xtigervnc (no Wayland)
- No desktop environment (headless VM)
- 4 vCPU Intel Xeon (virtualized)

**Measurement limitations:**
- VM has no real CPU C-states (no laptop power management)
- `powertop` system-wide wakeup measurement unavailable in VM
- No multi-monitor testing (single VNC display)
- Limited GUI interaction testing (headless environment)
- `perf` unavailable for kernel version
- Used context switches from `/proc/<pid>/status` as wakeup proxy

**Tools used:**
- `powertop 2.15` (limited data in VM)
- `/proc/<pid>/status` for context switch counting
- `htop`/`top` for CPU%
- Direct process measurement (60s samples)

**Metrics:**

| Scenario | Wakeups/sec (voluntary ctx switches) | CPU% | Notes |
|----------|--------------------------------------|------|-------|
| Baseline (no fidget) | 3.6 | - | X server (Xtigervnc) idle |
| Idle perched | ~271 | ~3% | Sprite visible, no interaction |
| Walking | N/A | N/A | Wakeups not measured; mask rebuild while walking measured on Grok Bot desktop (see #428 doc) |
| Chat open | N/A | N/A | Not measured (requires GUI interaction) |
| Multi-monitor | N/A | N/A | Not available in VM |
| Hidden | N/A | N/A | Not measured |

**Calculation details:**

Idle perched (60s sample, PID 12927):
- Initial voluntary context switches: 3,556
- After 60s: 19,821
- Rate: (19,821 - 3,556) / 60 = **271 voluntary ctx switches/sec**
- Average CPU: ~3%

Baseline X server (60s sample, PID 1594):
- Initial: 82,558
- After 60s: 82,772
- Rate: (82,772 - 82,558) / 60 = **3.6 ctx switches/sec**

**Findings:**

1. **High idle wakeup rate:** fidget idle shows ~271 wakeups/sec vs baseline 3.6/sec (75x increase). Hypothesis: unconditional frame loop sleep (~16ms = ~60Hz) plus additional subsystem polling.

2. **VM measurement constraints:** C-state residency and system-wide wakeup counting unavailable. Context switches are a coarse proxy. Bare-metal measurements would provide more accurate power impact data.

3. **Comparison to macOS target:** macOS issue [#431](https://github.com/omesser/fidget/issues/431) targets ~60 wakeups/sec idle. Linux VM shows 4.5x higher rate. Unknown how much is VM overhead vs real difference.

4. **Untested scenarios:** Walking, chat open, and window state changes require GUI automation not feasible in headless VM. Multi-monitor testing requires different environment.

**Evidence:**

Startup log excerpt:
```
character: BMO from target/debug/characters/bmo
libEGL warning: DRI3 error: Could not get DRI3 device
window_source: 0 visible windows
overlay: overlay-0 covers 1920x1200 at (0,0)
overlay: 1 display(s); sprite 126x128; BMO as BMO
director: StaticDirector
```

`powertop` output shows minimal data in VM (see issue comment for full CSV).

**Status:** Partial baseline captured. Idle perched wakeup rate measured. Full scenario matrix blocked by VM/headless constraints. Bare-metal Linux desktop testing recommended for complete baseline.

_Measured in cloud agent environment. Real laptop measurements would capture C-state residency and battery impact._

## macOS idle (#431)

Closed. The numbers live in [macos-idle-wakeups.md](./macos-idle-wakeups.md). Quiet-machine medians, release build of `ecb92b8d`, four rounds of 45 s. Idle perched is 8.4% CPU, range 8.0 to 8.5, 179.8 interrupt wakeups/s, range 178.9 to 186.4, and 2.32 package-idle wakeups/s. An earlier interleaved run on the same machine put hidden at 2.9% CPU, range 1.7 to 4.3, against perched at 7.9%, range 6.5 to 9.5. Cluster residency did not resolve a magnitude. `pmset -g assertions` did not gain an assertion named fidget. Multi-monitor wakeups were left to #424.

## GPU Compositing

### macOS Metal (issue #429)

Re-run with `sudo -v && FIDGET_BENCH_GREEN_LIGHT=1 scripts/bench-gpu-compositing-macos.sh matrix --seconds 15`. The script refuses every scenario but `env` and `baseline` without that variable, because the rest launch fidget on the live desktop, warp the cursor, or cover the main display. Written against `79cd3061`.

**Tools:**

- `scripts/bench-gpu-compositing-macos.sh`
- GPU% is `ioreg -c IOAccelerator` `PerformanceStatistics` `Device Utilization %`, sampled once a second, no sudo. VRAM is `In use system memory` from the same dictionary, which on Apple silicon is the GPU's share of unified memory.
- Watts and HW active residency are `sudo powermetrics --samplers gpu_power`, reduced by `scripts/parse-powermetrics.py`.
- Frame rate is N/A. The compositor's presented rate needs Instruments (Metal System Trace). `ticks_hz` counts the engine's `frame:` lines instead, so it says how often the rAF loop ticked, not how often WindowServer composited.
- Chat and hidden reuse `scripts/click-cursor.swift` and `scripts/fullscreen-window.swift` from `scripts/bench-wakeups-macos.sh`.

**Environment:**

- Mac15,7 (Apple M3 Pro, `AGXAcceleratorG15X`), macOS 26.7 (25G229)
- Two displays, 60 Hz
- `target/debug/fidget`, one 15 s window per scenario

**Metrics:**

| Scenario | GPU% (ioreg) | GPU active% (powermetrics) | Power W | VRAM MB | ticks/s | Notes |
|----------|--------------|----------------------------|---------|---------|---------|-------|
| Baseline (no fidget) | 0.3 | 5.50 | 0.06 | 565 | N/A | No fidget running |
| Idle perched | 0.6 | 6.13 | 0.06 | 708 | 52.73 | Pointer left alone, 791 ticks |
| Walking | 0.8 | 7.33 | 0.04 | 672 | 52.40 | 209 walk frames during the sample |
| Chat open | 0.3 | 5.68 | 0.03 | 678 | 53.00 | Summon logged at 960 923 |
| Multi-monitor | 0.5 | 4.92 | 0.03 | 636 | 52.87 | Two displays, two overlays |
| Hidden (fullscreen) | 0.0 | 4.80 | 0.03 | 540 | 1.07 | `presence: hidden`, 16 ticks |

**What this refutes.**

The issue predicted idle perched would hold 5-15% GPU and cost 0.5 to 2 W. It holds 0.6% and 0.06 W, the same wattage as an idle desktop with no fidget on it. Fullscreen transparent compositing is not a measurable GPU cost on this machine.

It also predicted multi-monitor would roughly double, two overlays being two compositing passes. Two displays measured 0.5% against one display's 0.6%. There is no doubling to find.

Walking against idle was predicted to be similar, and is: 0.8% against 0.6%.

**The noise floor is the result.** Every fidget scenario falls between 0.3% and 0.8%. A 10 s baseline taken minutes earlier on the same idle desktop read 1.2%, above every one of them. The overlay's GPU compositing cost is smaller than this instrument's run-to-run spread, so these deltas rank nothing. Anyone optimizing against them is fitting noise.

**What the hide rule actually saves.** GPU% does drop to 0.0 when a fullscreen app hides the sprite, which is what the issue asked to confirm. But the saving that shows up clearly is on the other axis: engine ticks collapse from roughly 53/s to 1.07/s. The hide rule earns its keep by stopping the rAF loop, not by sparing the compositor. That points the remaining #423 work at CPU wakeups (#431), not at compositing.

**Limits.** One machine, Apple silicon, unified memory, and a debug build. An Intel Mac with a discrete GPU composites transparency differently and the issue's Intel Power Gadget route is unrun. `ticks_hz` is the engine's own loop, not presented frames; the compositor's real rate still needs Instruments.

### Windows DWM (issue #430)

Re-run on the workstation in [mask-rebuild-baseline-windows.md](./mask-rebuild-baseline-windows.md):

`powershell -NoProfile -File scripts\bench-gpu-compositing-windows.ps1 matrix --seconds 15`

The script waits for an explicit green light before it launches fidget or moves the cursor.

A Linux cloud VM has no DWM, so it cannot measure GPU%, power, xperf frame time, or mask rate. The Windows desktop numbers are below.

Crop Task Manager's Performance GPU page during idle perched to about 280px wide and attach it with `gh pr comment --attach` in `file#alt` form. The script does not write the image. The image does not belong in the tree.

**Tools**

- `scripts/bench-gpu-compositing-windows.ps1`
- GPU% on Windows is `\GPU Engine(*)\Utilization Percentage` summed for `dwm.exe` `engtype_3D`, then WMI `Win32_PerfFormattedData_GPUPerformanceCounters_GPUEngine`, then `nvidia-smi` for the whole adapter.
- Power is `nvidia-smi` `power.draw` when that field is numeric. Those watts are the adapter.
- `mask_rebuild:` lines are `SetWindowRgn` calls. Per-call time stays in [#428](https://github.com/omesser/fidget/issues/428).
- `parse-log --seconds 2` on a 4-line fixture printed `mask_calls=4` and `mask_hz=2.00`. That fixture is not a Windows trace.
- Walking-over aims at the last `walk` or `ballwalk` frame. The cursor coordinate is that point times the primary's physical width over the overlay width in the log. `GetCursorPos` has to match.

**Metrics**

`matrix --seconds 15` at `f1020b2f` on the workstation in the mask-rebuild doc. Evidence is `.verify/430-gpu-remeasure/` there. The logs stay out of the tree. GPU% is `dwm.exe` `engtype_3D`. Power is `nvidia-smi` `power.draw`. xperf frame time was not measured. No `fidget.exe` was left running.

| Scenario | GPU% | Power W | Mask calls | Mask Hz | Notes |
|----------|------|---------|------------|---------|-------|
| Baseline (no fidget) | 5.5 | 14.2 | N/A | N/A | |
| Idle perched, pointer at (2,2) | 3.8 | 16.8 | 0 | 0.00 | |
| Walking, pointer away | 12.0 | 15.5 | 0 | 0.00 | walk_frames=742 |
| Walking-over, 5 s | 8.0 | 14.1 | 0 | 0.00 | Cursor landed at 3438,1328. scale 1.00. actual 3438,1328. walk_frames=0, walk aborted on hover. |
| Chat open | 5.5 | 14.2 | 17 | 1.13 | Summon logged. Pointer left on the sprite. |
| Multi-monitor | 5.5 | 14.2 | 0 | 0.00 | screens=2 |
| Hidden | 0.5 | 12.1 | 0 | 0.00 | Fullscreen cover. presence hidden. |

Walking-over mask rate is 0.00/s because the walk aborted once the pointer was on the sprite. The aim hit.

### Linux X11/Wayland (issue #425)

This table is the earlier debug capture. The release measurement above records held-still idle at 3.0% Xtigervnc CPU. A default idle window that contained walk and climb frames read 49.7%.

Re-run with `scripts/bench-gpu-compositing-linux.sh matrix --seconds 15`. Add `--shot path.png` to grab the GPU tool window during idle perched. The grab stays out of the tree.

**Environment:**

- Ubuntu 24.04.4 LTS
- Kernel 6.12.94+ (cloud VM, Xtigervnc, not a bare-metal GPU)
- X11 on `DISPLAY=:1`. `WAYLAND_DISPLAY` unset. X.Org 21.1.11, vendor The X.Org Foundation
- One screen, 1920×1200, xrandr mode `60.00*+`
- Compositor `xfwm4`, `use_compositing` true, `vblank_mode` auto
- Mutter, KWin, and Xfwm4's uncomposited mode were not running
- Character BMO, 126×128, from `characters/bmo`
- App scenarios ran with `FIDGET_TRACE_FRAMES=1` and `FIDGET_TRACE_MASK_REBUILD=1`

**Measurement limitations:**

- No `/dev/dri` and no `/dev/nvidiactl`. GPU% is N/A on every row.
- `intel_gpu_top` exits with "no discrete/integrated i915 devices found".
- `radeontop` exits with "Failed to find DRM devices" and "Can't find Radeon cards".
- `nvidia-smi` is not installed.
- `glxinfo -B` reports renderer `llvmpipe (LLVM 20.1.2, 256 bits)` and `Accelerated: no`. That is the GL setup check. It is not a utilization percent.
- No Wayland session, so the ADR-0014 degraded lane (X11 does not answer, and the build does not switch protocols) was not exercised. This host is the X11 lane. [ADR-0014](https://github.com/omesser/fidget/blob/3d16d6fc5d9dc8222861a49c05ca68fc4b0053ce/docs/adr/0014-x11-lane-no-native-wayland.md) is superseded by [ADR-0020](../adr/0020-x11-lane-no-native-wayland.md).
- One display, so multi-monitor is N/A.
- A second compositor was not available. The script records whichever of Mutter, KWin, xfwm4, picom, or Sway is running, and a re-run on that desktop fills the same columns.

**Tools:**

- `intel_gpu_top`, `radeontop`, `glxinfo -B`
- `xfconf-query` for xfwm4 compositing and vblank
- `xrandr` for screen count and refresh
- `mask_rebuild:` lines as the XShapeCombineMask call count. Per-call time stays in the [#428](https://github.com/omesser/fidget/issues/428) study.
- `/proc/<pid>/stat` utime+stime for `xfwm4` and `Xtigervnc`, as percent of one core over the sample window. This is a proxy for where the software composite lands. It is not GPU%.

**Metrics (15s windows, except the aborted pointer-on-sprite window at 5s):**

| Scenario | GPU% | Mask calls | Mask Hz | xfwm4 CPU% | Xtigervnc CPU% | Notes |
|----------|------|------------|---------|------------|----------------|-------|
| Baseline (no fidget) | N/A | N/A | N/A | 0.0 | 0.0 | No client, so no mask caller |
| Idle perched | N/A | 0 | 0.00 | 0.9 | 35.0 | Pointer at (2,2) |
| Walking | N/A | 0 | 0.00 | 1.1 | 46.7 | Pointer away. 457 `walk` frames |
| Pointer on sprite, walk aborted | N/A | 22 | 4.40 | 0.2 | 5.6 | 5s only. 8 `walk` frames, then react and talk. Not a sustained walk rate. That rate is [#428](https://github.com/omesser/fidget/issues/428) |
| Chat open | N/A | 17 | 1.13 | 0.3 | 3.4 | `Summon` logged. Pointer left on the sprite |
| Multi-monitor | N/A | N/A | N/A | N/A | N/A | xrandr reports 1 display |
| Hidden (fullscreen) | N/A | 0 | 0.00 | 0.0 | 0.8 | Log line `presence: hidden over 500ms` |
| Wayland | N/A | N/A | N/A | N/A | N/A | No Wayland display |
| Mutter / KWin | N/A | N/A | N/A | N/A | N/A | Not running |

**Findings:**

1. **GPU% is unread:** The vendor tools exit because the VM has no DRM node. Publishing a 0 here would be a guess. The renderer string is llvmpipe with acceleration off, and the app log repeats the DRI3 failure from the [#432](https://github.com/omesser/fidget/issues/432) run.

2. **X server CPU is the number that moves:** Baseline 0.0%, idle perched 35.0%, walking with the pointer away 46.7%, hidden 0.8%. `xfwm4` stays near 1% or below. Inference from the renderer string: with llvmpipe and no DRM device, that CPU is the software paint of the overlay inside `Xtigervnc`. A bare-metal run with `radeontop`, `intel_gpu_top`, or `nvidia-smi` replaces the N/A column.

3. **XShapeCombineMask stays at 0/s while the pointer is off the sprite:** Idle is 0 calls in 15s. Walking is 0 calls in 15s across 457 walk frames. The walking rate in this run is that 0.00/s. A later 5s window put the pointer on the sprite and the walk aborted. It logged 22 mask calls (4.40/s) and 8 walk frames, then react and talk. 4.40/s is that aborted window, not a sustained walk under the cursor. [#428](https://github.com/omesser/fidget/issues/428) measured 26.7 rebuilds/s when a walk stayed under the cursor. This issue leaves per-call time to that study.

4. **Chat open is a real Summon, with the pointer still on the sprite:** 17 mask calls in 15s (1.13/s). X server CPU in that window is 3.4%. The pointer was not parked away, so the mask rate is the cursor-over rate during chat, and the CPU drop against idle is under that same condition.

5. **Hiding for a fullscreen window returns X server CPU near the baseline:** 0.8% against 0.0% with no client and 35.0% while perched. The compositor flag on xfwm4 stayed on. There is no uncomposited X11 row.

**Evidence:**

The process loaded BMO from the repo path `characters/bmo` (`FIDGET_CHARACTERS=$PWD/characters`). The log named a box-local absolute path, omitted here.

```
libEGL warning: DRI3 error: Could not get DRI3 device
libEGL warning: Ensure your X server supports DRI3 to get accelerated rendering
overlay: 1 display(s); sprite 126x128; BMO as BMO
```

`glxinfo -B` during idle perched: `OpenGL renderer string: llvmpipe (LLVM 20.1.2, 256 bits)`, `Accelerated: no`. The same window shows `intel_gpu_top` and `radeontop` failing for lack of a device.

**Status:** Partial. This section leaves [#425](https://github.com/omesser/fidget/issues/425) open. GPU% per scenario and compositor is still N/A. Two compositors, a Wayland row, and an uncomposited X11 row are still missing. X11 under xfwm4 has a mask-rate pair (idle 0.00/s, walking with the pointer away 0.00/s) and an X-server CPU proxy.

## WindowSource (issue #427)

Re-run the ungated half with `scripts/bench-window-list-macos.sh micro`. The gated half is `sudo -v && FIDGET_BENCH_GREEN_LIGHT=1 scripts/bench-window-list-macos.sh matrix --seconds 15 --windows 100`; the script refuses `idle`, `riding`, and `matrix` without that variable because they launch fidget on the live desktop and flood it with windows. Written against `8588715e`.

**What the app does.** One poll is `CGWindowListCopyWindowInfo(OptionOnScreenOnly | ExcludeDesktopElements, 0)` plus a decode of every entry's bounds, number, layer, and (with Screen Recording consent) owner name, in `walk_visible` at `src-tauri/src/platform/macos/window_source.rs:78-80`. `SnapshotAssembler::assemble` reads it once per `POLL_INTERVAL` (100 ms, `crates/core/src/window_source.rs:10`) and once per `RIDE_POLL_INTERVAL` (16 ms, `crates/core/src/window_source.rs:15`) while any Instance reports `riding` (`src-tauri/src/frame_loop.rs:1317`, switched at `src-tauri/src/frame_loop.rs:1692`). The read is synchronous on the frame loop thread (`crates/core/src/snapshot.rs:84-87`), so a poll's cost lands inside the tick that makes it.

**Tools:**

- `scripts/bench-window-list-macos.swift` times the same call and decode from its own process against whatever is on the desktop. It opens nothing.
- `scripts/bench-window-list-macos.sh` wraps it (`micro`) and, gated, samples a running fidget with dtrace (`idle`, `riding`, `matrix`). The added windows come from `scripts/window-flood-macos.swift`, under review in [#1043](https://github.com/omesser/fidget/pull/1043); when that file is absent the added-window rows skip and say so. The ride comes from `scripts/perch-window.swift --glide`, which slides the perch every frame so `riding` stays on for the whole sample.
- The issue's dtrace one-liner matches no probe on this machine: `dtrace: probe description pid<n>::CGWindowListCopyWindowInfo:entry does not match any probes`. On macOS 26 CoreGraphics forwards to SkyLight, and the pid provider lists `SLWindowListCopyWindowInfo` there. Probing that on the microbench counted 8439 calls in 4 s at 460 µs average, against the microbench's own 455 µs median, so the two instruments agree. `sudo` is required; System Integrity Protection prints a warning but lets the pid provider attach to an unsigned binary.
- `xctrace record --template 'Time Profiler' --attach <pid> --time-limit 5s` records headless and its export names `SLWindowListCopyWindowInfo` in the sampled frames, so the issue's Instruments route works without opening Instruments. The script uses dtrace instead because it yields a call count and a per-call duration directly.

**Environment:**

- Mac15,7 (Apple M3 Pro), macOS 26.7 (25G229), two displays
- The desktop as found: 53 on-screen windows under the app's options, 201 under the every-window option
- `swift` interpreter and a `swiftc -O` build agree within run-to-run spread

**Metrics (in-process microbenchmark, measured, opens nothing):**

| Row | Windows | Median µs/poll | p95 µs | Max µs | µs/window | Notes |
|-----|---------|----------------|--------|--------|-----------|-------|
| `app-call` (the bare call, app's options) | 52 to 53 | 382 to 436 | 417 to 898 | 3298 to 3684 | 7.2 to 8.2 | Seven runs of 300 iterations |
| `app` (call + decode, no names) | 52 to 53 | 404 to 468 | 447 to 910 | 1856 to 2060 | 7.7 to 8.8 | Eight runs; the consent-off path |
| `app-names` (call + decode + owner name) | 52 to 53 | 424 to 517 | 448 to 715 | 1406 to 6851 | 8.1 to 9.8 | The consent-on path |
| `all` (every window, every Space) | 201 | 1378 to 2269 | 2321 to 3893 | 5638 to 6105 | 6.9 to 11.3 | Nine runs over two days: 1.4 ms, 1.93 ms (400 iterations), and 2.27 ms are three separate runs of the same row |

The `all` row is a range on purpose. Three runs, minutes to hours apart, put its median at 1.4, 1.93, and 2.27 ms. Max is the two runs that kept a file. A window-count figure from one run of this row is a snapshot of that desktop, not a property of the call.

**Metrics (the app under dtrace, measured, one run each, `target/debug` build):**

The operator approved one `matrix --seconds 15 --windows 100` run. Every number below is from that run, on a debug build of fidget, and the +100 rows used the flood script under review in [#1043](https://github.com/omesser/fidget/pull/1043). Windows is the on-screen count under the app's options, read by the microbench beside the sample. Hz is dtrace's call count divided by 15 s.

| Scenario | Windows | Poll Hz (target) | Median µs | p95 µs | Max µs | dtrace calls | Notes |
|----------|---------|------------------|-----------|--------|--------|--------------|-------|
| Idle perched, desktop as found | 55 | 9.80 (10) | 2169 | 5076 | 8057 | 147 | pointer left alone |
| Idle perched, +100 flood windows | 155 | 9.80 (10) | 3925 | 6390 | 9347 | 147 | |
| Walking | N/A | N/A | N/A | N/A | N/A | N/A | Not in the script. The poll rate depends on `riding` alone (`src-tauri/src/frame_loop.rs:1692`), so a walk polls at the idle cadence; a row would restate the idle one |
| Riding a gliding perch, desktop as found | 56 | 45.40 (60) | 1500 | 4750 | 15165 | 681 | 378 distinct perched positions over the sample |
| Riding a gliding perch, +100 flood windows | 156 | 40.20 (60) | 2785 | 10834 | 12796 | 603 | 296 distinct perched positions |

**The in-app call is slower than the same call timed alone.** Measured: at 55 to 56 windows the app's median poll is 2169 µs idle and 1500 µs riding, against 404 µs for the microbench's `app` row at 52 windows in the same matrix run. That is 3.7 to 5.4 times the in-process figure. The worst riding sample at 56 windows took 15165 µs, against a 16667 µs frame at 60 Hz; two of 682 riding samples passed 8 ms. At 156 windows the riding p95 is 10834 µs, 65% of the frame, and the max 12796 µs. Whether a tick that contains one of those polls overran the frame is not measured: dtrace timed the call, not the tick. Why the app's call is slower than the microbench's is not measured either. A guess is that the debug build and the window server's per-process state both add to it. The microbench numbers say what the call costs at best, not what it costs fidget.

**Poll rate.** Measured: idle polls at 9.80 Hz against the 10 Hz target. Riding polls at 45.4 Hz on the desktop as found and 40.2 Hz with 100 windows added, against a 60 Hz target. What sits behind the shortfall is a guess: the debug build's frame loop not holding 60 Hz, rather than the poll. The poll's median at 56 windows is 1.5 ms, so by itself it cannot stretch a 16.7 ms frame to the 22 ms the 45.4 Hz rate implies. A release build measured with the same script would settle it.

**Scaling, computed from the rows above.** Adding 100 windows raised the app's median poll by 1756 µs idle (17.6 µs per window) and by 1285 µs riding (12.9 µs per window). The microbench's own figure across 52 to 201 windows is 7 to 11 µs per window. Four counts under the app's options now exist (52 to 56, 155, 156, and the 201 every-window row), and the table is what stands in for the curve the issue asks to plot. Nothing is plotted.

**What the CPU share is, computed from the rows above.** Idle at 55 windows, 9.8 Hz times 2.17 ms is 21 ms of one core a second. Riding at 56 windows, 45.4 Hz times 1.5 ms is 68 ms a second, 6.8% of one core. Riding at 156 windows, 40.2 Hz times 2.79 ms is 112 ms a second, 11% of one core. The issue's Activity Monitor CPU% for the whole process was not taken.

**What this refutes.** The issue's hypothesis was about 50 µs per window. The measured figure is 7 to 11 µs per window in-process and 13 to 18 µs per added window in the app, and over 90% of a microbench poll is the call itself: the decode fidget adds costs 30 to 50 µs at 53 windows, and reading the owner name adds another 20 to 50 µs. At 156 windows the app's median riding poll is 2.8 ms, not the 10 to 50 ms the issue predicted for 200 windows. The tail is another matter: a p95 of 10.8 ms and a max of 12.8 ms at 156 windows, and a 15.2 ms worst sample at 56, are within one frame each but leave little of it.

**Limits.** One machine, one desktop, one run of the gated matrix, on a debug build. The microbench times a separate process and the app's calls are 3.7 to 5.4 times slower, so the microbench alone understates the cost. The `all` row's spread across nine runs is wider than the `app` row's, so window-count scaling on a busy desktop needs more than one run per count. Not produced: the Instruments Time Profiler screenshot the issue asks for (the headless `xctrace` recording exists, but a screenshot needs Instruments on the screen) and a plotted curve.

**Scaling curve (the app idle and the microbench, measured, one run, `target/debug` build, `346fe2a7`).** The operator approved one `sweep` run: `FIDGET_BENCH_GREEN_LIGHT=1 scripts/bench-window-list-macos.sh sweep`, with `sudo -n` working for dtrace. It adds 0 to 300 flood windows in steps of 50. At each count it runs the microbench's `app` row (300 iterations) and then samples an idle fidget with dtrace for 15 s. The rows are in [`window-list-sweep-macos/rows.tsv`](./window-list-sweep-macos/rows.tsv), and `node scripts/plot-window-list-sweep.mjs rows.tsv scaling.svg` re-plots them.

![Poll time against on-screen window count](./window-list-sweep-macos/scaling.svg)

| Windows (app / micro) | App median µs | App p95 µs | App max µs | Micro median µs |
|-----------------------|---------------|------------|------------|-----------------|
| 51 / 46 | 1058 | 2929 | 8394 | 416 |
| 101 / 97 | 1049 | 2714 | 4206 | 1282 |
| 151 / 147 | 3211 | 5496 | 15261 | 1967 |
| 201 / 197 | 2596 | 7466 | 22290 | 2519 |
| 251 / 247 | 3417 | 9082 | 38826 | 3174 |
| 301 / 297 | 4065 | 10717 | 30277 | 3265 |
| 351 / 347 | 4706 | 14561 | 39195 | 2676 |

Every app row polled at 9.80 to 9.87 Hz. Computed from the table, the app's median grows by about 12 µs per window from 51 to 351 windows, close to the matrix's 13 to 18 µs. The p95 grows faster, by about 39 µs per window, and reaches 14.6 ms at 351 windows. The max passes one 16.7 ms frame from 201 windows on, at 22 to 39 ms. Idle only polls every 100 ms, so each such poll stalls one tick, not every tick. Whether the stalled tick drops a presented frame is not measured. Riding makes the same call up to six times as often (inferred from the poll intervals), so at 300 or more windows its p95 would sit near a whole frame. That is a guess, since the sweep did not ride.

In this run the app's median was 0.8 to 2.5 times the microbench's at the same count, not the matrix's 3.7 to 5.4 times. At about 100 windows the app was the faster of the two. The microbench's own curve flattens from 247 windows and drops at 347. Why it drops is not measured.

**Time Profiler (measured, one 15 s recording, 47 windows read by the microbench right after it).** `FIDGET_BENCH_GREEN_LIGHT=1 scripts/bench-window-list-macos.sh profile` attaches `xctrace` with the Time Profiler template to an idle fidget. It then reduces the exported call tree to the samples under `SLWindowListCopyWindowInfo` and demangles the names with Homebrew's `llvm-cxxfilt` when it is installed. The report is [`window-list-sweep-macos/time-profile.txt`](./window-list-sweep-macos/time-profile.txt), and it stands in for the issue's screenshot. fidget used 1510 ms of CPU in 15 s, 10.1% of one core. The call accounted for 84 ms of that, 5.6%. All of it was on the thread named `fidget`, under `run_frame_loop` → `SnapshotAssembler::assemble` → `WindowSource::snapshot` → `WindowSource::read` → `visible_windows` → `walk_visible`, so that thread is the frame loop's (inferred from the stack). The Time Profiler counts only on-CPU samples. Those 84 ms (5.6 ms a second) are less than the roughly 10 ms a second of wall time computed from the sweep's 51-window row (1058 µs median × 9.8 Hz). dtrace's wall time also counts the time the call waits on the window server (inferred from the two tools' definitions).

## Click-through mask (issue #428)

See [mask-rebuild-baseline-x11.md](./mask-rebuild-baseline-x11.md) for detailed X11 measurements.

**Summary (X11 on Grok Bot Linux desktop, 1280×800, tip `6b4acbf`):**
- **Idle perched** (cursor not over sprite): **0.0 rebuilds/sec** (BMO 126×128@1x)
- **Walking under cursor** (BMO on perch): **~26.7 rebuilds/sec**, **~15.0 ms/rebuild** (motion-driven; MaskParams includes x,y)
- **Fast animation** (BMO react, 10 fps): **~9.6 ms/rebuild** (6360 opaque pixels) — FPS doesn't increase rebuild cost, which remains driven by opaque count
- **Large sprite** (Black Mage 37×33@3x): **~1.4 ms/rebuild** (477–579 opaque pixels) — much faster than BMO@1x despite 3× scale, because fewer source opaque pixels. Scale multiplies rendered size, not source opaque count.
- Small sprite: No genuinely smaller shipped character at scale=1; scenario dropped.
- Prior cloud-VM idle: 0.05/sec, 11–13 ms (kept for comparison in detailed doc)
- **Windows:** Measured on the workstation. See [mask-rebuild-baseline-windows.md](./mask-rebuild-baseline-windows.md). Walk under the cursor was about 44/s at 14.5 ms.
- **`perf` flamegraph:** Not yet collected

## Memory and multi-monitor (#424)

macOS is measured in [memory-rss-and-multi-monitor.md](./memory-rss-and-multi-monitor.md), debug build, 300 s settle. One fidget on two displays peaked at 583 MB physical footprint. Four fidgets of one Character added 56 MB, all of it in the webviews. The Rust peak stayed 82.9 MB. The heavier panel is the larger one. A single-display macOS run was not taken, because both displays stay attached. No Instruments heap profile.

Linux and Windows in that file are short settles on other machines. The Grok Bot Linux run, debug, one display, one fidget, median RSS 823 MB over 30 s. The Windows workstation, two displays, one fidget, median working set 612 MB over 30 s. This VM's release numbers are in the Linux measurement section above. One display, so the multi-monitor slope was not measured here. No heap profile on any platform.

## Frame cadence (#426)

Linux release numbers, the Windows matrix from DESKTOP-UQIE144, and the Windows dual-monitor seam ride are in the measurement section above. The tables here are the macOS release captures.

### macOS

Re-run with `FIDGET_BENCH_GREEN_LIGHT=1 scripts/bench-frame-cadence-macos.sh matrix --seconds 20 --bin target/release/fidget`. It launches fidget once per scenario on the live desktop and sends no input. To compare two builds, pass `--bin` twice: one run alternates them per scenario and reports B minus A. Measured at `0eb84a3f`, this branch rebased onto `main` at `94b23feb`. That includes `aa02d774` (an Active tick sleeps only the rest of its 16 ms). The analyzer counts lag only on moving frames.

**Tools:**

- `FIDGET_TRACE_CADENCE=1` makes the overlay log every display frame it draws: the rAF timestamp, whether it asked for the next frame, and the two arrivals it interpolated between. It also makes the frame loop print its tick count once a second. `FIDGET_TRACE_FRAMES=1` supplies the Engine ticks, one `frame:` line each.
- `scripts/frame-cadence.mjs` reduces a log window to the tables below. Interpolation lag runs `interpolate()` over the arrival times, so it uses the renderer's own arithmetic.
- Lag is measured from each placement's arrival in the webview to the display frame that draws it. Both stamps come from the webview's clock. #426 asked for Engine tick against render. Arrivals follow ticks at the IPC delay, so the spacing matches, but no stamp here is the Engine's own.
- "Moving" is decided by the Engine position traced at or before each arrival. That match crosses clocks: `frame:` lines are Rust wall time and rounded to whole points. A skew between the two clocks, or motion under half a point, can drop a lag sample. At walking speed neither shows.
- WebKit rounds `performance.now()` and rAF timestamps to 1 ms, so a frame delta reads as 16 or 17 ms, never 16.7.

**Environment:** Mac15,7 (Apple M3 Pro, 12 cores), macOS 26.7 (25G229), two displays at 60 Hz, release build, BMO, Director off, 20 s per scenario. Load is one `yes` per core.

| Scenario | Display frames | Mean fps | Drops (>20 ms) | Loop restarts | Engine ticks/s | Lag p50, moving | Lag p95, moving | Moving frames held |
|---|---|---|---|---|---|---|---|---|
| Idle perched | 83 | N/A | 0 | 82 | 53.8 | N/A | N/A | N/A |
| Idle perched, `FIDGET_TRACE_FRAMES` off | 257 | 59.8 | 1 | 85 | 53.9 | N/A | N/A | N/A |
| Walking | 440 | 59.9 | 1 | 103 | 53.9 | 19 ms (1 sample) | 22 ms (1 sample) | 21 |
| Walking + CPU load | 385 | 59.9 | 5 | 97 | 50.8 | 20 ms (1 sample) | 28 ms (1.18 samples) | 42 |

Engine ticks/s is the frame loop's own counter, so it reads the same with `FIDGET_TRACE_FRAMES` off.

Frame deltas count only consecutive frames where the first asked for the second. A frame the loop started again on an arrival is a restart, and the quiet gap before it is not a drop. So mean fps describes the stretches the loop stayed armed, not frames over the window. The walking and load windows start at the first walk frame and run 20 s of wall time, so they also cover pauses between walks.

| Delta ms | Walking frames | Walking ticks | Load frames | Load ticks | Idle ticks |
|---|---|---|---|---|---|
| 0-10 | 0 | 0 | 0 | 13 | 0 |
| 10-14 | 1 | 27 | 13 | 41 | 34 |
| 14-18 | 314 | 304 | 218 | 169 | 300 |
| 18-20 | 20 | 337 | 43 | 189 | 310 |
| 20-25 | 0 | 408 | 13 | 527 | 432 |
| 25-34 | 1 | 2 | 0 | 69 | 1 |
| 34-50 | 0 | 0 | 0 | 10 | 0 |

**Against the design claim.**

- **60 fps while moving: confirmed:** Walking holds 59.9 fps, with one delta of 25 to 34 ms. Under full CPU load 5 of 287 deltas miss a vsync, and every one lands under 25 ms.
- **Lag is one sample: confirmed:** The sprite is drawn one Engine tick behind, about 19 ms. Under load, p95 reaches 1.18 samples, because a late tick leaves the sprite held at the latest placement for a frame.
- **"About 44 Hz, 16 to 38 ms" is out of date:** The Engine now ticks at 53 Hz. Idle and walking put 97% of tick gaps between 14 and 25 ms; load puts 87% there. The comment in `src/interpolate.js` now says so.

**Why the Engine ticks at 53 Hz and not 60.** A 16 ms `sleep` on this machine returns after 20 ms. Measured in a separate process with `Time::HiRes::sleep`, 150 sleeps at each length:

| Requested | p50 returned |
|---|---|
| 4 ms | 5.0 ms |
| 8 ms | 10.0 ms |
| 16 ms | 20.0 ms |
| 32 ms | 36.6 ms |

The overshoot is about a quarter of the request, capped near 5 ms. That is consistent with macOS timer coalescing plus scheduler slack, but unmeasured: no A/B of `kern.timer.coalescing_enabled` was run. `active_wait` already subtracts the tick's own work, so the rest of the gap is the sleep itself. Idle perched ticks at the same rate because the frame loop stays Active there: a looping idle animation and sleep accrual both keep it on the 16 ms timer (#183).

**Rust's sleep overshoots the same way (#1156).** `crates/core/examples/sleep_overshoot.rs` times `std::thread::sleep` in-process, then runs `scheduler::next_tick` still and moving. Same Mac, release build, 2000 samples each, on the #1156 branch cut from `main` at `3bd3a617`:

| Measured | p50 | p90 | p99 | Rate |
|---|---|---|---|---|
| `sleep(16 ms)` returns after | 20.01 ms | 20.05 ms | 20.07 ms | N/A |
| `sleep(8 ms)` returns after | 10.01 ms | 10.05 ms | 10.08 ms | N/A |
| Still tick gap (counts from the wake) | 20.01 ms | 20.06 ms | 20.08 ms | 52.1 Hz |
| Moving tick gap (counts from the deadline) | 15.99 ms | 18.15 ms | 19.82 ms | 62.5 Hz |

Overshoot explains the 53 Hz: a wait counted from the wake adds each sleep's 4 ms lateness to every period. So a moving sprite (falling, dragged, climbing, riding, or with any velocity) now waits toward the last deadline, and the lateness shortens the next wait instead. A still sprite that stays Active for an idle animation or sleep accrual keeps 52 Hz on purpose, so a perched sprite wakes no more often than before (#183).

Moving ticks now run at 62.5 Hz, the rate of the 16 ms `ENGINE_TICK` period, not exactly 60 Hz. The deadline fixes the average rate, not the per-tick jitter: moving gaps still reach 18 ms at p90. A precise platform timer is #1168. Re-run with `cargo run --release -p fidget-core --example sleep_overshoot -- 2000`.

**In the app, at `761e93ac`.** The bench's walking scenario reads 51.9 Engine ticks/s, but that rate covers the whole 20 s window, and BMO stands idle between walks for most of it. Splitting the `frame:` ticks inside each scenario's window by whether the traced position changed since the previous tick:

| Scenario | Moving ticks | Moving mean gap | Still ticks | Still mean gap | Loop counter |
|---|---|---|---|---|---|
| Idle (BMO walked) | 338 | 16.0 ms (62.5 Hz) | 780 | 18.7 ms (53.5 Hz) | 55.8 Hz |
| Walking | 330 | 16.0 ms (62.5 Hz) | 709 | 20.8 ms (48.1 Hz) | 51.9 Hz |
| Load | 364 | 16.1 ms (62.1 Hz) | 760 | 18.7 ms (53.5 Hz) | 56.3 Hz |

Moving ticks reach 62.5 Hz in the app. The loop counter blends the two paces: the split's gaps average to 55.9, 51.9 and 56.1 Hz. A still tick waits as it did before this change.

**The frame trace does not slow the Engine.** Idle ticks at 53.8 Hz with `FIDGET_TRACE_FRAMES` on and 53.9 Hz with it off, by the loop's own counter. So the per-tick `frame:` print is not what holds the Engine under 60 Hz.

The idle run with the trace off drew 257 display frames at 59.8 fps, where the traced idle run drew 83 with no armed stretch. This run does not explain the difference. It is one 20 s sample, and the idle animation it landed on may differ.

**Why an idle, still sprite redraws about 4 times a second.** Of the 83 idle frames:

- 22 follow a change of animation frame (sit, idle and talk). That is new art, and it needs a draw.
- 61 follow `FRAME_RESEND` in `src-tauri/src/frame_loop.rs`, which sends an unchanged placement again every 250 ms. The overlay's `frame` listener asks for a display frame on every arrival, even one identical to the last, so each resend costs a rAF with nothing to draw.

Each frame is matched to the Engine state traced at or before its latest arrival. It counts as new art when the state, placement or animation frame differs from the previous frame's, and as a resend when none of them does. The match crosses the same clocks as the moving gate above.

**Limits.**

- One machine, 60 Hz panels. A ProMotion display would show 8.3 ms deltas.
- This macOS capture did not ride a window, and it did not cross the seam between its two displays. The Linux ride and the Windows dual-monitor seam ride are in the measurement section above. A Linux seam is still unmeasured.
- The trace records the first Instance only.
- The analyzer pools every overlay's frames into one count. A sprite straddling a seam draws on two overlays, so those frames are two series in one total. The Windows seam ride's 1850 display frames are that pooled count, not one overlay alone; armed gaps plus restarts equal frames minus 2. A Linux seam is still unmeasured.
