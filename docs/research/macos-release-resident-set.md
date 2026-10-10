# macOS release resident set

One macOS release run for #424. Debug numbers stay in
[Memory, RSS, and multi-monitor scaling](memory-rss-and-multi-monitor.md).
This document does not replace them.

The run measured commit `c8bfd7afaaf8b1e1ea5b407208935ab2435e445b`.
The running binary is `target/release/fidget`, built with
`cargo build --release --bin fidget` from the same Rust sources as that
commit. The commit itself only changes the sampler.

---

## The run

| | |
|---|---|
| Machine | MacBook Pro `Mac15,7`, Apple M3 Pro, 36 GB, macOS 27.0.1, build 26A434 |
| Display 1 | DELL P2414H, 1920×1080, main, overlay at (0, 0) |
| Display 2 | Built-in Liquid Retina XDR, 3456×2234 at 2×, 1728×1117 points, overlay at (1920, 0) |
| Build | `target/release/fidget`, release |
| Roster | `bmo:One`. `FIDGET_CHARACTERS` is the repo `characters` directory |
| Director | Off. `FIDGET_DIRECTOR=0`, so `StaticDirector` picks every Behavior |
| When | 2026-10-10, 18:19:19Z to 18:29:30Z |
| Free memory | 75% to 76% for the whole run, from `memory_pressure` |

`scripts/bench-rss-macos.sh --bin target/release/fidget --research` settled
for 300 seconds and sampled for 300 seconds, every 5 seconds. No clicks,
drags, or keystrokes. The script killed the app on exit. Stderr had no
helper-count warning. Four WebKit helpers is what the script expects for
two displays, and that is the set it sampled.

Release builds do not copy stderr back to the terminal. `process_log::init`
keeps those lines in the process log. The sampler appends the bytes written
after launch, and reads the overlay count from those bytes.

## Result

Peak physical footprint is the `vmmap` value `Physical footprint (peak)`,
read while each process was still alive. It only rises. Median RSS is the
script's median of the summed resident set across the five processes.

Total peak physical footprint is 639.9 MB. Median RSS is 446 MB, over 60
samples. The minimum summed RSS is 433 MB and the maximum is 459 MB.

| Process | RSS median | Footprint peak |
|---|---|---|
| `fidget` | 160 MB | 89.1 MB |
| `com.apple.WebKit.GPU` | 48 MB | 132.2 MB |
| `com.apple.WebKit.Networking` | 11 MB | 7.4 MB |
| 1920×1080 `WebContent` | 99 MB | 147.8 MB |
| 3456×2234 `WebContent` | 129 MB | 263.4 MB |

Networking's `vmmap` line is `7552K`, reported here as 7.4 MB. The footprint
total adds that figure to the four megabyte peaks the script printed.

`place_overlays` builds overlay-0 on the main display first, so that helper
takes the lower pid. The lighter `WebContent` is the 1920×1080 panel. The
heavier one is the Retina panel.

The debug document's run A, on 2026-09-09 under macOS 26.6.2, reports 583 MB
peak footprint and 259 MB median RSS with 33% to 43% of memory free.
This run is a later OS build with 75% to 76% free, and later commits sit
between the two trees. Do not read 639.9 MB against 583 MB as a
release-versus-debug result. Do not read 446 MB against 259 MB that way
either. That document says a busy machine reclaims pages and makes RSS look
lighter, and this machine was not busy.

## What the sprite was doing

The overlay line reports a 126×128 sprite, BMO as One. A first-run gesture
tour logged during the settle, 26 seconds after launch. It did not
fall inside the sample window.

Each of the 60 RSS samples takes the `engine:` animation in effect at that
second. `FIDGET_TRACE_ENGINE=1` writes those lines.

| Animation | Samples | Median RSS |
|---|---|---|
| idle | 38 | 446 MB |
| walk | 15 | 446 MB |
| fall | 3 | 448 MB |
| climb | 2 | 447 MB |
| land | 1 | 445 MB |
| sit | 1 | 449 MB |

Idle and walking read the same median. The other four animations are a few
samples, and they sit within a few MB of that median.

## Still open

A single-display run, a three-display run, an Instruments heap profile, and
a run with Chat open are still open. This run kept both attached displays
and did not open Chat. Linux and Windows numbers are not in this document.
