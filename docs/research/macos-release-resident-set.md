# macOS release resident set

Three macOS release runs for #424. Debug numbers stay in
[Memory, RSS, and multi-monitor scaling](memory-rss-and-multi-monitor.md).
This document does not replace them.

The two-display run measured commit `c8bfd7afaaf8b1e1ea5b407208935ab2435e445b`.
The running binary is `target/release/fidget`, built with
`cargo build --release --bin fidget` from the same Rust sources as that
commit. The commit itself only changes the sampler.

The "One display" and "Heap profile" sections are two later runs. They
measured `target/release/fidget` built the same way at `c8120e6f` on the #424
branch, and their citations read that tree. The one exception is the
allocator table, which a later `scripts/heap-top-allocators.sh` recomputed
from the saved report.

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

## Dropped from #424

On 2026-10-19 the operator dropped the three-display macOS run and the Linux
multi-monitor runs from #424: "macos - only 2 displays available / linux
multi monitor setup - unavailable at this time / Eliminate those
requirements". Linear scaling with display count is not confirmed on either
platform, and #424 no longer asks for it.

## One display

Both later runs used the same machine, roster, and Director setting as the
run above. Free memory was not recorded for either.

The operator unplugged the DELL P2414H, so the built-in Retina panel was the
only overlay, 1728×1117 points at (0, 0). The run was
`scripts/bench-rss-macos.sh --bin target/release/fidget --research` from
2026-10-19 01:20:59Z to 01:30:54Z. The script found the three WebKit helpers it
expects for one display.

Total peak physical footprint is 430.0 MB. Median RSS is 289 MB, over 59
samples. The minimum summed RSS is 286 MB and the maximum is 292 MB. #424
guessed 200-400 MB for one display and one Instance.

| Process | RSS median | Footprint peak |
|---|---|---|
| `fidget` | 155 MB | 80.9 MB |
| `com.apple.WebKit.GPU` | 41 MB | 134.1 MB |
| `com.apple.WebKit.Networking` | 14 MB | 7.3 MB |
| 3456×2234 `WebContent` | 79 MB | 207.7 MB |

Networking's `vmmap` line is `7504K`, reported here as 7.3 MB.

Against the two-display run above, one display reads 209.9 MB less peak
footprint and 157 MB less median RSS. Each figure comes from a single run nine
days apart, under a different free-memory state, so the gap is indicative and
not a measured per-display cost. It is also larger than the second panel's own
`WebContent`, because the Retina `WebContent` peaked at 207.7 MB here against
263.4 MB above.

## Heap profile

The run was `scripts/bench-rss-macos.sh --bin target/release/fidget --settle 300
--seconds 10 --heap heap/` with both displays attached, at 2026-10-19 01:36Z.
Stack logging adds a Performance Tool Data region, 16 MB in `fidget`, so this
run's footprint and RSS do not compare with the figures above and are not
reported as results.

Under `MallocStackLogging=1`, `malloc_history -allBySize` also lists VM regions
(`mach_vm_map`, `mmap`, IOSurface, thread stacks) at their virtual size.
Summing every record reads 4.4 GB live in a process whose peak footprint was
106.5 MB. `scripts/heap-top-allocators.sh` ranks only records whose innermost
frame is in `libsystem_malloc.dylib` and prints the rest as one virtual
total. It demangles with `c++filt` first, since the report's Rust symbols are
mangled and the allocator-glue patterns match demangled names.

`scripts/heap-top-allocators.sh heap/malloc-history.txt fidget` on the saved
report:

```
top 20 allocators in fidget, live bytes by innermost frame
    5036 KB     374 calls  <fidget_core::overlay::AlphaMask>::from_png
    5027 KB     538 calls  fidget::art_urls
    4028 KB     824 calls  fidget_core::character::resolve::resolve_animations
    2886 KB   19731 calls  <tao::platform_impl::platform::event_loop::EventLoop<tauri_runtime_wry::Message<tauri::EventLoopMessage>>>::run::<tauri_runtime_wry::make_event_handler<tauri::EventLoopMessage, <tauri::app::App>::make_run_event_loop_callback<fidget::main::{closure#4}>::{closure#0}>::{closure#0}>
    1183 KB    7664 calls  <tao::platform_impl::platform::window::UnownedWindow>::new
     991 KB    3054 calls  <wry::wkwebview::InnerWebView>::new_ns_view
     802 KB    3686 calls  <tauri_runtime_wry::Wry<tauri::EventLoopMessage> as tauri_runtime::Runtime<tauri::EventLoopMessage>>::new
     718 KB    4692 calls  <objc2_web_kit::generated::__WKWebView::WKWebView>::loadRequest
     437 KB    2733 calls  tao::platform_impl::platform::app::send_event
     332 KB    1656 calls  fidget::main::inner::<tauri_runtime_wry::Wry<tauri::EventLoopMessage>, tauri_utils::assets::EmbeddedAssets>::{closure#0}
     306 KB    3354 calls  <muda::platform_impl::platform::ns_menu_item::NsMenuItem>::create
     272 KB       1 calls  <image::images::dynimage::DynamicImage>::to_rgba8
     243 KB     978 calls  tauri::app::on_event_loop_event::<tauri_runtime_wry::Wry<tauri::EventLoopMessage>>
     203 KB    3248 calls  <muda::platform_impl::platform::PlatformMenuItem>::attach
     199 KB    1476 calls  <objc2_app_kit::generated::__NSStatusBar::NSStatusBar>::statusItemWithLength
     121 KB    1229 calls  <objc2_app_kit::generated::__NSWorkspace::NSWorkspace>::frontmostApplication
     110 KB    1414 calls  <wry::wkwebview::InnerWebView>::eval::<alloc::boxed::Box<dyn core::ops::function::Fn<(alloc::string::String,), Output = ()> + core::marker::Send>>
      61 KB     426 calls  <objc2_app_kit::generated::__NSStatusItem::NSStatusItem>::button
      48 KB       1 calls  png::encoder::write_chunk::<&mut core::io::cursor::Cursor<&mut alloc::vec::Vec<u8>>>
      48 KB       1 calls  <mio::event::events::Events>::with_capacity
   25111 KB live in malloc zones, 1351 KB of it with no fidget frame
 4602192 KB virtual in VM regions outside malloc, not a resident figure
```

The 24.5 MB of live malloc-zone bytes sits under the 64 MB that `footprint`
reports dirty in Malloc Small and Malloc Large. Those categories count dirty
pages, not live allocations, so freed and fragmented pages are the likely
difference. That is an inference, not a measurement.

`fidget`'s own code holds three of the top rows (inferred from the symbols):

- **`AlphaMask::from_png`:** 5036 KB, one click mask per distinct frame.
- **`fidget::art_urls`:** 5027 KB. Each Animation's frame list holds its own
  clone of the frame's base64 `data:` URL, so a frame two Animations share is
  stored twice.
- **`resolve_animations`:** 4028 KB, the Character's resolved Animations and
  art.

The WebKit helpers, by `footprint` category and `heap -s` totals, in MB. The
1920×1080 `WebContent` is the lower pid, as in the run above.

| Helper | Footprint now | Largest categories | `heap -s` live |
|---|---|---|---|
| `GPU` | 128 | Owned physical footprint (unmapped) (graphics) 106, IOAccelerator (graphics) 6.2, Malloc Small 5.8 | 10.4 |
| 1920×1080 `WebContent` | 71 | WebKit Malloc 47, Owned physical footprint (unmapped) (graphics) 11, Untagged 4.8 | 48.7 |
| 3456×2234 `WebContent` | 106 | WebKit Malloc 60, Owned physical footprint (unmapped) (graphics) 37, Untagged 2.7 | 55.0 |
| `Networking` | 5.5 | WebKit Malloc 2.0, `__DATA_DIRTY` 1.0, `__DATA` 0.5 | 0.7 |

GPU memory dominates the GPU helper. WebKit Malloc dominates each `WebContent`.
In `heap -s`, 96% and 94% of the two `WebContent`s' live bytes are
`non-object`. The largest named classes are `JSC::StringSourceProvider`, 5,103
and 11,131 instances, and `WebCore::CachedScriptFetcher`, 4,911 and 10,938. One
per evaluated script is the likely reading, which would point at the
`InnerWebView::eval` calls in the table above. That is a guess.

## Still open

A run with Chat open. None of these runs opened Chat. Linux and Windows numbers
are not in this document.
