# Memory, RSS, and multi-monitor scaling

Baseline for #424, under #423's plan. This document covers macOS, Linux, and
Windows. Each platform uses a different overlay toolkit and a different process
layout, so a number on one operating system is not a number on another.

## Summary of macOS findings

A two-display, one-Instance character peaks at 583 MB of physical footprint
across five processes. In a window with a third of the machine's memory free,
the median resident set was 259 MB. Footprint scales with pixels, not with
Instances. Each display is one `WebContent` process. In every run, the
3456×2234 panel is 75-110 MB heavier than the 1920×1080 panel. Four Instances
cost 56 MB more, and all of that cost is inside the webviews. The Rust
process peak did not move. Character art is not what makes a character large.
All eight installed packages are 4.5 MB of base64. The app loads every package
at launch, so switching Character cannot make RSS grow. What the sprite is
doing does not move the number either. Idle, walking, sitting, and talking
read within a few MB of each other.

---

## macOS

### The machine and the build

| | |
|---|---|
| Machine | MacBook Pro `Mac15,7`, 36 GB, macOS 26.6.2, build 25G83 |
| Display 1 | DELL P2414H, 1920×1080 at 1×, main, overlay at (0, 0) |
| Display 2 | Built-in Liquid Retina XDR, 3456×2234 at 2×, 1728×1117 in points, overlay at (1920, 0) |
| Build | `target/debug/fidget`, debug, ad-hoc signed at a worktree path |
| Completer | None. `FIDGET_DIRECTOR=0`, so `StaticDirector` picks every Behavior and no HTTP leaves the process |
| Date | 2026-09-09 |

#424 asked for the debug build. A user does not run that build. Read the Rust
process share as an upper bound. The WebKit helpers are release code either
way, and the debug build does not change them.

### How to reproduce

`scripts/bench-rss-macos.sh` launches the app, waits out the settling curve
below, samples every process RSS on a fixed interval, and reads each process
peak physical footprint before it stops the app.

The default is a brief smoke test. It settles for about 3 seconds and samples
for about 10 seconds. That is fast enough for a test matrix with many
scenarios. It is not a research soak.

Research mode passes `--research`. That restores a 300 second settle and a
300 second sample, for the shape of the curve and for measurement studies.

```sh
cd src-tauri && cargo build --bin fidget && cd ..

HOME=/tmp/bench-home \
FIDGET_DIRECTOR=0 FIDGET_DIRECTOR_API_KEY=x FIDGET_CAPTURABLE=1 \
FIDGET_TRACE_ENGINE=1 FIDGET_CHARACTERS="$PWD/characters" \
FIDGET_INSTANCES="bmo:One" \
scripts/bench-rss-macos.sh --out /tmp/one-instance.tsv

# Research mode (300s settle + 300s sample) for measurement studies:
scripts/bench-rss-macos.sh --research --out /tmp/research-run.tsv
```

Five variables carry the run. `FIDGET_INSTANCES` is the roster.
`FIDGET_CHARACTERS` is the set of installed packages. `FIDGET_DIRECTOR=0`
keeps the network out of the run. `HOME` points at a scratch directory so the
run does not touch the real install's settings or Action Log.
`FIDGET_DIRECTOR_API_KEY` is not a credential here. A worktree build is a new
path to the Keychain, and without the variable the launch stops on a password
dialog. See #283 and #290.

`FIDGET_TRACE_ENGINE=1` is what makes a run reportable rather than a bare
number. An RSS figure with no record of what the sprite was doing is not a
measurement. The trace is the only record of that.

### Where the memory is

WKWebView runs its content out of process. Those processes are children of
`launchd`, not of the app. No process-tree walk finds them.

```
fidget                          Rust binary. Engine, frame loop, art, Tauri
com.apple.WebKit.WebContent     one per overlay, so one per display
com.apple.WebKit.WebContent
com.apple.WebKit.GPU            one, shared
com.apple.WebKit.Networking     one, shared
```

`ps -o rss= -p <app>` reports a third of the truth. The script takes the set
of WebKit helpers before launch and again just after it, and calls the
difference the app's. That difference is also the one place a run can go
wrong. Another application that starts a helper inside that window lands in
the set. The script warns when the count is not `displays + 2`. Repeat a run
that warns.

### The launch peak is not the number

RSS after launch falls, then climbs. It does not sit on a flat line. The
series below is fifteen minutes, one Instance, two displays, idle desktop. It
is an earlier run than the four below, and the only one with a Completer
configured, which is why its resting level is not theirs.

| Since launch | Total RSS |
|---|---|
| 0 s | 396 MB |
| 60 s | 272 MB |
| 120 s | 205 MB |
| 150 s | 178 MB |
| 300 s | 224 MB |
| 450 s | 228 MB |
| 600 s | 245 MB |
| 750 s | 265 MB |
| 900 s | 264 MB |

The loader accounts for the first minute. The dip near 150 seconds is macOS
reclaiming pages the launch faulted in and then never touched. Everything
below settles for 300 seconds before it samples, for that reason.

The slow climb out of that dip is about 1 MB per minute, and it is still
rising at fifteen minutes. This document does not explain it. It is small
enough to be page-in of memory the dip compressed away, and large enough that
a multi-hour soak is worth running before anyone calls the curve flat.

### What RSS on macOS does and does not mean

RSS is what the kernel has let a process keep, not what the process needs. A
busy machine takes pages back from an idle character, and the same app then
reads far lighter for reasons that have nothing to do with the app. That
effect is larger than every difference #424 asks about.

| Run | Free memory during sampling | Median RSS |
|---|---|---|
| A, 1 Instance, 8 Characters installed | 33-43% | 259 MB |
| B, 4 Instances of one Character | 25-29% | 226 MB |
| C, 4 Instances, 4 Characters | 37-42% | 288 MB |
| D, 1 Instance, 1 Character installed | 43% | 309 MB |

Read down that column and four Instances look smaller than one, and one
installed Character looks larger than eight. Both are artifacts of when each
run happened to be sampled. This machine was shared with other agents building
and running throughout, and their load is the free-memory column.

Peak physical footprint is Activity Monitor's Memory column, which is what
`vmmap` reports. It only ever rises, so it survives a noisy machine. Every
comparison below uses the footprint. The RSS series is kept for shape, not
for ranking.

### Results

Peak physical footprint per process, in MB, over a 300 second settle plus a
300 second sample.

| Run | `fidget` | GPU | Networking | 1920×1080 overlay | 3456×2234 overlay | Total |
|---|---|---|---|---|---|---|
| A, 1 Instance, 8 Characters installed | 82.9 | 146.3 | 8.0 | 132.7 | 213.2 | 583 |
| B, 4 Instances of one Character | 82.9 | 147.6 | 7.3 | 162.0 | 239.5 | 639 |
| C, 4 Instances, 4 Characters | 85.1 | 150.2 | 7.9 | 170.9 | 280.5 | 695 |
| D, 1 Instance, 1 Character installed | 35.3 | 139.8 | 8.4 | 122.6 | 197.7 | 504 |

`place_overlays` builds overlay-0 over the main display first, so its helper
takes the lower pid. In all four runs the lower pid is also the lighter
process. That is how the table assigns each `WebContent` column to a display.

### Displays cost pixels, not displays

#424 guessed that RSS scales linearly with display count. It scales with
display area. The two overlays are the same document, the same sprite, and
the same art, and one is consistently far heavier than the other.

| Run | 1920×1080 overlay | 3456×2234 overlay | Difference |
|---|---|---|---|
| A | 132.7 | 213.2 | +80.5 |
| B | 162.0 | 239.5 | +77.5 |
| C | 170.9 | 280.5 | +109.6 |
| D | 122.6 | 197.7 | +75.1 |

The ratio of backing stores is the ratio of pixels. 3456 × 2234 × 4 bytes is
30.9 MB a buffer. 1920 × 1080 × 4 bytes is 8.3 MB a buffer. The gap above is
two to three buffers. The planning number is not N displays times 150 MB. A
1080p display is about 120-170 MB. A Retina display is about 200-280 MB. A
5K panel will cost more.

This measurement is the process split of a two-display configuration. It is
not a comparison of one display against two. Both displays stay attached to
this machine, and an agent cannot unplug one. The split is sound. One
`WebContent` process exists per overlay, and `place_overlays` closes an
overlay when its display goes away. A single-display run is still worth
taking on a machine where unplugging a display is possible.

### Instances cost webview, not engine

Four Instances of one Character against one Instance, run B against run A, is
56 MB more. Every MB of it is in the two `WebContent` processes, 29 MB and
26 MB. The Rust process peak was 82.9 MB in both runs, to the tenth of a MB.
The Engine's per-Instance state and the frame it emits are nothing next to
four `img` elements and four bubble layers per overlay.

Making those four Instances four different Characters, run C against run B,
is another 56 MB. Nearly all of it is in the webviews, 9 MB and 41 MB. The
Rust side moves 2.2 MB. Four Characters on screen means WebKit decodes four
sprite sheets. Four Instances of one Character means WebKit decodes one.

### Character art, and why a switch cannot leak

The app decodes every installed package's art, base64-encodes it, and holds
it for the life of the process. `load_all_characters` runs at launch, not on
demand, so a switch or a spawn does not wait for a load the overlay never
does. The art is keyed by Character rather than by Instance, so two Instances
of one Character share one copy.

| Character | Frames | PNG | as base64 `data:` URLs |
|---|---|---|---|
| black-mage | 25 | 20 KB | 27 KB |
| bmo | 33 | 222 KB | 296 KB |
| buddy-bot | 78 | 575 KB | 767 KB |
| cat | 45 | 720 KB | 960 KB |
| jotaro-kujo | 38 | 538 KB | 717 KB |
| nim | 44 | 18 KB | 23 KB |
| timber-wolf | 41 | 664 KB | 885 KB |
| trump | 38 | 635 KB | 846 KB |
| all eight | 342 | 3.3 MB | 4.5 MB |

Two answers #424 asks for.

**A Character switch cannot grow RSS permanently.** The art switched to is
already resident, and the app never frees the art left behind, so there is
nothing to accumulate. The cost was paid at launch. This is a claim about
the code path, checked against the code, not a measured switch. Driving the
tray menu is a human step.

**Seven unused installed Characters cost 79 MB of peak footprint.** That is
run A against run D. 47.6 MB of it is in the Rust process. That is ten times
the 4.5 MB of base64 they amount to. The likely reason is that the peak is
not the resting size. `art_urls` allocates a fresh `String` per animation
frame, and the `character` command serializes the whole map to JSON once per
overlay. Neither is confirmed. This document has no heap profile.

### What the sprite was doing

`StaticDirector` picks ambient Behaviors, so a run is a mix rather than a
held pose. Run A's 300 second window was idle 59 percent, walk 22 percent,
sit 8 percent, talk 5 percent, climb 4 percent, and the rest fall, land, and
react.

Matching each RSS sample to the animation live at that instant.

| Animation | Samples | Median RSS |
|---|---|---|
| idle | 37 | 260 MB |
| walk | 12 | 245 MB |
| sit | 5 | 260 MB |
| talk | 2 | 260 MB |
| climb | 2 | 240 MB |

The spread across animations is smaller than the spread within any one of
them. Idle perched and walking cost the same memory. That is the useful
negative result. Whatever the frame loop costs, it does not cost pages.
See #431. The other three runs agree. B reads 226 MB idle and 226 MB walking.

### Not measured on macOS

**A single-display comparison, and three displays.** Both displays are
permanently attached and no third exists. The per-display section says what
stands in for that comparison.

**A heap profile with top allocators.** #424 asks for Instruments Allocations
or `heaptrack`. Instruments needs a GUI session and a human. The allocator
ranking is unanswered, and the 47.6 MB above is the first thing to point it
at.

**A release build.** Everything in the macOS section is `target/debug`. One release run is recorded in [macOS release resident set](macos-release-resident-set.md).

**Chat windows open.** Every macOS run is overlays only.

---

## Linux

One unattended run completed on Grok Bot box. The method is below.

### Results

The run followed the stderr-file fix, commit f27703b.

| | |
|---|---|
| Machine | Grok Bot box |
| Display | 1, `DISPLAY=:3` |
| Scenario | 1 Instance, `bmo:One` |
| Settle | 5 seconds |
| Sample duration | 30 seconds |
| Sample interval | 2 seconds |
| Samples | 15 |
| Total RSS minimum | 821 MB |
| Total RSS median | 823 MB |
| Total RSS maximum | 933 MB |
| Process count | 4. fidget, WebKitNetworkProcess, and two WebKitWebProcess |
| Exit code | 0 |

Per-process RSS median and peak RSS, from `VmHWM`.

| Process | RSS median | Peak RSS |
|---|---|---|
| fidget | 237 MB | 250 MB |
| WebKitNetworkProcess | 61 MB | 60.5 MB |
| WebKitWebProcess | 238 MB | 238 MB |
| WebKitWebProcess | 287 MB | 396 MB |

Total RSS includes the main fidget process plus all WebKitGTK helper
processes.

### How to run on a machine with a display

`scripts/bench-rss-linux.sh` implements the same contract as the macOS
script, adapted for Linux.

The default is a brief smoke test. It settles for about 3 seconds and samples
for about 10 seconds. That is fast enough for a test matrix with many
scenarios. It is not a research soak.

Research mode passes `--research`. That restores a 300 second settle and a
300 second sample, for the shape of the curve and for measurement studies.

```sh
cd src-tauri && cargo build --bin fidget && cd ..

HOME=/tmp/bench-home \
FIDGET_DIRECTOR=0 FIDGET_DIRECTOR_API_KEY=x FIDGET_CAPTURABLE=1 \
FIDGET_TRACE_ENGINE=1 FIDGET_CHARACTERS="$PWD/characters" \
FIDGET_INSTANCES="bmo:One" \
scripts/bench-rss-linux.sh --out /tmp/one-instance.tsv

# Research mode (300s settle + 300s sample) for measurement studies:
scripts/bench-rss-linux.sh --research --out /tmp/research-run.tsv
```

### Process architecture

WebKitGTK's process model depends on version and build configuration.

Modern WebKitGTK, version 2.26 and newer, uses a multi-process layout like
macOS. It runs separate processes for WebContent, GPU, and Network.

Older builds, and builds with sandboxing disabled, may run everything in one
process.

The Linux script uses `pgrep -P <pid>` to find every child of the main fidget
process. WebKitGTK helpers on Linux are children of the main process. On
macOS they are children of `launchd`. The process tree walk finds the Linux
helpers on its own.

### Measurement method

The script reads RSS from the `VmRSS` field of `/proc/[pid]/status`. That is
the current resident set.

The script reads peak RSS from the `VmHWM` field of `/proc/[pid]/status`.
That is the high-water mark. It only ever rises, so it survives a noisy
machine. It is the Linux equivalent of the macOS peak physical footprint.

Settling defaults to 3 seconds for a brief smoke test. Pass `--research`, or
pass `--settle 300 --seconds 300`, for the 300 second settle and 300 second
sample measured on macOS. Measure the settling curve on Linux before treating
300 seconds as the right wait here.

### Display count

Run `xrandr`, or read the app log line `overlay: N display`, to see how many
displays the app detected. If WebKitGTK runs one WebContent process per
overlay, as WebKit does on macOS, the per-display cost shows up in the
process split.

### Expected behavior from the macOS findings

These expectations come from the macOS findings. Linux has not checked them
at scale.

**Displays cost pixels, not count.** A 1920×1080 overlay may be 100-170 MB.
A 2560×1440 overlay may be 200-300 MB. The size depends on WebKitGTK's
backing store.

**Instances cost webview, not engine.** Several Character Instances should
add cost to the WebContent processes, not to the main Rust process.

**Character art is front-loaded.** The app loads every installed Character
at launch, so a Character switch cannot grow RSS permanently.

### Heap profiling

To profile the heap on Linux, use `heaptrack`.

```sh
heaptrack target/debug/fidget
# ... run the scenario ...
heaptrack --analyze heaptrack.fidget.*.gz
```

Look for the top allocators, and for whether unused Character art in base64
strings accounts for the 47.6 MB gap found on macOS.

### One-Instance cut for #645

Measured on 2026-09-30 on an Ubuntu 24.04 VM. One display, 1920×1200.
WebKitGTK 2.52.6. Release build of `fidget`. Same short scenario as the Grok
Bot run above. One Instance, `bmo:One`. Every package on `FIDGET_CHARACTERS`.
`FIDGET_DIRECTOR=0`. Settle 5 seconds. Sample 30 seconds. Interval 2 seconds.
The script is `scripts/bench-rss-linux.sh`. The script launches
`target/debug/fidget`. That file was a copy of the release build.

The release baseline median is 858 MB. The debug short run above has a median
of 823 MB. The machines differ. The two medians are in the same band. A debug
binary is not what makes one Instance large. The cut below compares a release
build to a release build.

The installed WebKitGTK 2.52 marks
`WEBKIT_PROCESS_MODEL_SHARED_SECONDARY_PROCESS` deprecated. The setting has
no effect. Two webviews cannot share a web process.

The app does not build Chat or Settings at launch. The fourth process in the
baseline was the taskbar anchor. That anchor was a `WebviewWindow` of the
overlay page. Its only job is a panel button. Linux now builds that button
as a GTK window with no webview. Windows still uses a WebView2 window for
the same button. This run did not measure Windows.

| Run | Processes | Minimum RSS | Median RSS | Maximum RSS |
|---|---|---|---|---|
| Before. The anchor is a webview. | 4 | 857 MB | 858 MB | 903 MB |
| After. The anchor is a GTK window. | 3 | 584 MB | 585 MB | 613 MB |

RSS median and `VmHWM` for each process.

| Run | Process | RSS median | VmHWM |
|---|---|---|---|
| Before | fidget | 243 MB | 247 MB |
| Before | Network | 48 MB | 48 MB |
| Before | Web process | 243 MB | 244 MB |
| Before | Web process | 323 MB | 369 MB |
| After | fidget | 218 MB | 222 MB |
| After | Network | 48 MB | 48 MB |
| After | Web process | 319 MB | 361 MB |

The script sums RSS. WebKit's libraries are mapped in every process, so the
sum counts those pages more than once. `Pss` and private pages come from
`smaps_rollup`, summed over the same pids while the RSS series sat on its
median. Before, that was about 501 MB proportional and 350 MB private.
After, that was about 381 MB proportional and 285 MB private. The bench drop
of 273 MB is the script's summed RSS. The proportional drop is about 120 MB.
The private drop is about 65 MB.

Focusing the anchor opened Settings, and a second web process appeared. The
idle bench stayed at three processes. The button asks to park at -32000, -32000. This window manager left the 1×1 on the display. The log line
starts with `anchor: 1x1 at` and then the position. The window is
undecorated. It does not take a web process.

Loading only `bmo`, with `FIDGET_CHARACTERS` pointed at that one package,
used the same after binary and the same script. It did not come out lighter.
Total RSS minimum was 559 MB, median 602 MB, maximum 603 MB, against a median
of 585 MB with all eight packages. `VmHWM` moved from 222 MB to 201 MB in
`fidget`, and from 361 MB to 354 MB in the web process. That gap is not a
second web process. A Character switch draws from the preload, so the
preload stayed.

---

## Windows

One unattended run completed on DESKTOP-UQIE144. The method is below.

### Results

The run followed the stderr-file fix, commit f27703b.

| | |
|---|---|
| Machine | DESKTOP-UQIE144 |
| Displays | 2. 3440×1440 and 1200×1920 |
| Scenario | 1 Instance, `bmo:One` |
| Settle | 5 seconds |
| Sample duration | 30 seconds |
| Sample interval | 2 seconds |
| Total working set minimum | 607 MB |
| Total working set median | 612 MB |
| Total working set maximum | 620 MB |
| Exit code | 0 |

Total working set includes fidget.exe plus every msedgewebview2.exe helper
process.

### How to run

`scripts\bench-rss-windows.ps1` implements the same contract as the macOS and
Linux scripts, adapted for Windows PowerShell.

The default is a brief smoke test. It settles for about 3 seconds and samples
for about 10 seconds. That is fast enough for a test matrix with many
scenarios. It is not a research soak.

Research mode passes `-Research`. That restores a 300 second settle and a
300 second sample, for the shape of the curve and for measurement studies.

```powershell
cd src-tauri
cargo build --bin fidget
cd ..

$env:HOME = "C:\Temp\bench-home"
$env:FIDGET_DIRECTOR = "0"
$env:FIDGET_DIRECTOR_API_KEY = "x"
$env:FIDGET_CAPTURABLE = "1"
$env:FIDGET_TRACE_ENGINE = "1"
$env:FIDGET_CHARACTERS = (Get-Location).Path + "\characters"
$env:FIDGET_INSTANCES = "bmo:One"

.\scripts\bench-rss-windows.ps1 -Out "C:\Temp\one-instance.tsv"

# Research mode (300s settle + 300s sample) for measurement studies:
.\scripts\bench-rss-windows.ps1 -Research -Out "C:\Temp\research-run.tsv"
```

### Process architecture

WebView2 on Windows uses the Chromium, Edge, multi-process layout.

The main process is `fidget.exe`, the Rust binary.

WebView2 helpers are several `msedgewebview2.exe` processes. One renderer
runs per webview, so one renderer runs per display for the overlay. One GPU
process is shared. One network service is shared. Utility processes cover
audio, storage, and similar work.

The Windows script uses `Get-Process -Name "msedgewebview2"` before and after
launch to find every WebView2 helper that appeared. These helpers are not
children of fidget.exe. They are children of the Edge browser infrastructure.
The script takes the set difference, the same way the macOS script finds
WebKit helpers.

### Measurement method

Working set is the current memory usage from `Get-Process`, field
`WorkingSet64`. That is the Windows equivalent of RSS.

Peak working set is `Get-Process`, field `PeakWorkingSet64`. It only ever
rises, so it survives a noisy machine.

Settling defaults to 3 seconds for a brief smoke test. Pass `-Research`, or
pass `-Settle 300 -Seconds 300`, for the 300 second settle and 300 second
sample measured on macOS. The Windows measurement above used a 5 second
settle and a 30 second sample, as a quick validation run.

### Display count

Read the app log line `overlay: N display` to see how many displays the app
detected. Windows has one renderer process per webview, so the per-display
cost shows up in the process split.

### Expected behavior from the macOS findings

These expectations come from the macOS findings. Windows has not checked them
at scale.

**Displays cost pixels, not count.** WebView2 renderer memory should scale
with overlay resolution, which is the backing store size.

**Instances cost webview, not engine.** Several Character Instances should
add cost to the renderer processes, not to the main fidget.exe process.

**Character art is front-loaded.** The app loads every installed Character
at launch, so a Character switch cannot grow memory permanently.

### Machine details

The measurement above ran on DESKTOP-UQIE144 after commit f27703b, the
stderr-file fix. The script finished unattended with exit code 0.

### Heap profiling

To profile the heap on Windows, use Windows Performance Analyzer or the
Visual Studio profiler.

```powershell
# Using Windows Performance Recorder (WPR)
wpr -start GeneralProfile -filemode
# ... run fidget ...
wpr -stop profile.etl
# Analyze with Windows Performance Analyzer (wpa.exe profile.etl)
```

Look for the top allocators in the Rust process, and for whether unused
Character art accounts for the gap found on macOS.
