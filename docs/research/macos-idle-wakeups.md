# CPU wakeups and idle C-state impact on macOS

Baseline for #431, under #423's plan. #423 names the suspect: an unconditional
~60 Hz frame-loop tick that never backs off, even when the sprite is still and
the desktop is idle (#183). PR #718 shipped idle back-off for macOS and
Windows against that suspect (#183 Stage 2b) and has since merged as
`8ba9481d`; these captures were taken while it was still open, against its
head and against the `main` it branched from, so the comparison is between two
fixed commits rather than against a moving branch.

## The numbers

A character sitting perched, doing nothing, on an idle desktop:

| | `main` | after #718 |
|---|---|---|
| **CPU** | **14.6%** | **14.9%** |
| **Wakeups/sec** | **369** | **369** |
| Wakeups/sec, package-idle only | 4.22 | 3.52 |

Release build, M3 Pro, medians of three interleaved 45-second captures per arm.

**#741 has since split these two numbers between the threads that produce
them and moved both.** Read "What the 369 is made of" below before this
table: half the wakeups belong to WebKit display-link threads and the 16ms
tick holds 12% of them, which is not what any of the issues above assumed.

**#761 has since named the 313 leftover, after #760 and #790.** The
display-link rows are gone. The host `libpas` scavenger is still the
largest named leftover besides the 16 ms frame loop, it is one thread per
process rather than per webview, and no public `WKWebViewConfiguration` or
JSC environment knob bounds it. See "The leftover after #760 and #790
(#761)".

## What this means

**14.6% of a CPU to animate a still sprite, and it did not move.** That is the
result. Everything below is about why.

**The 16 ms tick is not what costs.** #183 and #423 are both built on the
premise that a 60 Hz frame loop that never backs off is the idle cost. A 60 Hz
tick is 60 wakeups/sec. This process does **369**. The tick is at most a sixth
of the problem, so backing it off cannot fix idle cost — and measurably did
not. #718 moved the headline wakeup count by nothing and CPU by nothing.

**The webview's animation loop never stops, and that is in the code, not a
guess.** `src/main.js:343` calls `requestAnimationFrame(draw)` at the top of
`draw`, before it checks whether any view has anything to redraw. So the
overlay webview re-schedules itself at display refresh forever, per display,
whatever the sprite is doing and whatever the Rust frame loop decides. On a
120 Hz panel that is 120 wakeups/sec per display that no Rust-side back-off
can reach.

That is the shape of the finding: **#183's whole line of work is aimed at the
smaller half of the problem, and the larger half is a JavaScript loop nobody
has looked at.**

**What was a guess, and is now measured.** That the rAF loop is the single
biggest remaining contributor was a guess when this was written. #741 split
the count per thread and the answer is below: the loop is the largest single
cause of the wakeups, but not through its own JavaScript, and it is not where
the CPU goes. The async runtime, log rotation and the tray icon are all
negligible.

## What the 369 is made of

Split per thread under #741, on the same machine and the same scenario, with
`ktrace trace -f S0x0140`. That filter records the kernel's Mach scheduling
subclass, in which `MACH_MKRUNNABLE` names the thread being made runnable and
the emitting context says what made it so. Counting only the events raised
from kernel context isolates the timer and interrupt wakeups, which is what
`powermetrics` puts in its `Wakeups intr` column. `sample` names the threads.

Two 20-second windows on the `main` arm, interleaved with the fix arm, 298.2
and 293.5 interrupt wakeups/sec in total:

| Thread | Wakeups/sec | Share |
|---|---|---|
| `JavaScriptCore libpas scavenger` | 113.0, 111.3 | 38% |
| `CVDisplayLink` (built-in panel) | 96.3, 95.5 | 32% |
| `CVDisplayLink` (external panel) | 46.8, 48.2 | 16% |
| frame loop (`run_frame_loop`) | 36.0, 34.0 | 12% |
| `main` | 6.0, 4.6 | 2% |

**The 16ms tick is 12% of the wakeups, and the two display links are 48%.**
There is one `CVDisplayLink` thread per display, in the host process, and
WebKit runs them for as long as the page it hosts wants animation frames.
Their rates are the panels' own: 96/sec on the 120Hz built-in display and
47/sec on the 60Hz external one. The overlay asked for a frame at the top of
every frame, so they never stopped. That is the rAF loop's real cost in this
column, and it is not in the webview's process at all.

The `libpas scavenger` is JavaScriptCore's allocator, in the host process
because Tauri links WebKit there.

Under both fixes the same two windows read 207.2 and 210.3 in total, and the
one row that does not move is the frame loop: 36.5 and 37.9. Every other row
falls, and the display links stop running as two standing threads. Nothing a
Rust-side tick policy can reach is what changed.

### Where the CPU goes, which is somewhere else

The host process is not the only one. `powermetrics` reports per process, and
an overlay's WKWebView content runs in XPC services of its own:

| Process | CPU (ms/s) | Wakeups/sec |
|---|---|---|
| `fidget` | 163.6 | 392.1 |
| `WebKit.WebContent` (both overlays) | 122.4 | 24.5 |
| `WebKit.GPU` | 12.0 | 10.8 |
| `WebKit.Networking` | 0.5 | 9.7 |
| **Total** | **302.7** | **436.5** |

Medians of three interleaved 30-second captures on the `main` arm, launched
the same way `scripts/bench-wakeups-macos.sh` launches one and sampled the
same way, with the XPC services the app spawned recorded alongside its own
pid. It says the thing this document had no way to see before: the 14.6% at
the top counts one of four processes, and the character's real idle cost is
nearly twice the host process alone.

It also says where the rAF loop's own work lands, which is not in the wakeup
count: two `WebContent` processes burn 122ms/s of CPU between them and wake
24 times a second. The loop is expensive in CPU where it runs and expensive
in wakeups two processes away.

Inside the host process, the cost is the event emit. Tauri delivers an event
by evaluating JavaScript in the target webview, so a tick that sends a frame
to two overlays is two `WKWebView` script evaluations. `log stream --process`
on the host counts **205 of them a second**, each taking and releasing a
WebKit process assertion and writing four `os_log` lines on the way. In the
same `ktrace` capture, `main` takes 730 thread-to-thread wakeups a second
against 5 interrupt ones: an emit wakes a thread with a Mach message, so it
lands as CPU and never in the column this document has been reading.

### After the two fixes

Both halves have a fix and neither one covers the other, so all three arms ran
together, A/B/C interleaved, three 45-second captures each, idle-family bucket:

| Arm | Wakeups/sec, median [range] | CPU%, median [range] |
|---|---|---|
| `main` | 404.9 [404.5–424.0] | 12.48 [7.82–13.63] |
| skip an unchanged emit | 409.0 [393.1–411.9] | 10.14 [6.76–10.45] |
| and stop re-arming rAF | **313.4** [246.8–365.9] | **6.38** [4.84–8.21] |

Read the arms against each other rather than against the table at the top of
this document: this round ran on a quieter machine than #733 did and its
`main` arm reads 405 wakeups/sec and 12.5% CPU where #733 read 369 and 14.6%.

**Skipping an unchanged emit moves CPU and not wakeups.** Its whole range sits
inside `main`'s on wakeups, which is the same non-result #718 got, and for the
same reason: an emit wakes a thread by sending it a Mach message, and a
message is not an interrupt. It cut the evaluation rate from 205/sec to
16/sec, and the CPU followed.

**Stopping the rAF re-arm moves wakeups, and the ranges do not overlap.** 405
to 313, a 23% cut, with `main` at 404.5–424.0 and the fix at 246.8–365.9.
Nothing in the two arms' ranges touches. The display links are what moved.

Together: **12.5% CPU and 405 wakeups/sec become 6.4% and 313.**

The whole-family capture above, rerun across all three arms, agrees and reads
tighter, because it samples every second rather than the idle-animation ones:

| Arm | `fidget` CPU (ms/s) | `fidget` wakeups/sec | Family CPU (ms/s) |
|---|---|---|---|
| `main` | 163.6 [163.6–169.2] | 392.1 [391.6–394.1] | 302.7 |
| skip an unchanged emit | 124.7 [121.4–125.4] | 393.0 [392.4–393.0] | 221.5 |
| and stop re-arming rAF | 76.5 [73.9–77.2] | 230.4 [228.1–231.1] | 98.8 |

Every column separates cleanly, and the middle row is the same split again:
the emit fix takes a quarter of the family's CPU and none of its wakeups.

## The leftover after #760 and #790 (#761)

Attribute before fixing. #761 asked whether the `libpas` scavenger that
held 38% of the pre-#741 table is reachable from this repository at all.
The answer is no, as a public knob. The measurements that settle the three
unknowns are below. Closing with "not reachable from here, documented" is
the Done.

Same machine as the rest of this note (`Mac15,7`, macOS 26.6.2), same
`ktrace trace -f S0x0140 --csv` filter, same kernel-context
`MACH_MKRUNNABLE` count, `sample` for names. Two 20-second windows on a
still perched sprite, release binary at `0393f222`. One display this
round (`overlay: 1 display(s)`), so there is no second `CVDisplayLink`
row to compare. `sample` names a thread as `Thread_<tid>: <name>` with a
colon; a regex that expects whitespace after the id misses the scavenger
entirely.

The binary was built with one local compile fix in `harness.rs`
(`self.dir` → `self.data.as_path()`) so `origin/main` would link. That
path is elicitation logging. It is not on the overlay, the frame loop, or
WebKit.

### Per-thread census, idle perched

Kernel `MACH_MKRUNNABLE` for threads `sample` named in the host and in
the WebKit XPC services this launch spawned. Two 20-second windows:

| Thread | Process | Wakeups/sec |
|---|---|---|
| `JavaScriptCore libpas scavenger` | `fidget` (host) | 28.0, 26.1 |
| frame loop (`run_frame_loop`) | `fidget` (host) | 35.0, 35.1 |
| WebContent main thread | `WebKit.WebContent` | 11.45, 13.45 |
| `JavaScriptCore libpas scavenger` | `WebKit.GPU` | 4.65, 4.65 |
| `JavaScriptCore libpas scavenger` | `WebKit.WebContent` | 4.0, 4.1 |
| host `main` | `fidget` | 0.45, 0.3 |
| `CVDisplayLink` | `fidget` (host) | 0, 0 |
| `JavaScriptCore libpas scavenger` | `WebKit.Networking` | 0, 0 |

Named family total 85.5 and 87.7 interrupt wakeups/sec. The frame loop is
the same 35/sec #741 already measured. The scavenger in the host is the
row that moved: 113.0 and 111.3 before the rAF stop, 28.0 and 26.1 now.
A `CVDisplayLink` thread still exists in `sample` and posts no kernel
`MACH_MKRUNNABLE` in either window, which is the #760 / #790 display-link
win on a one-display desk.

`powermetrics --samplers tasks` on the same binary, one 30-second idle
launch, idle-family bucket (n=21 seconds): **187 interrupt wakeups/sec**
and **2.8% CPU** for the host pid. Walk seconds in that same capture
read 300/sec. That 187 is the leftover to read against #741's 313, not
the 86 from named ktrace rows. ktrace only names threads `sample` saw;
powermetrics counts the whole process. They are different columns, as
they were in #741.

### Whether the scavenger rate follows webview allocation

**It followed the rAF stop. It does not look like a remaining per-frame
knob on the overlay.** Measured, not guessed.

The host scavenger dropped from 113/sec to 27/sec once the overlay
stopped asking for a display frame every refresh and stopped evaluating
unchanged ticks. That is the #741 / #760 / #790 work. The WebContent
scavenger that remains is 4/sec, so a renderer that allocated still less
per frame would be working on a 4/sec row, not the 27.

A `JSC_libpasScavengeContinuously=true` launch, which the host JSC
honored (`Modified JSC options: libpasScavengeContinuously=true`), left
the family scavenger at 33.0/sec against idle's 36.7. The option does
not bound the rate, and turning it on did not inflate it either.

A chat-open window is not a clean "more allocation" arm. Summon
confirmed, then the frame loop itself fell to 6.9/sec and the scavenger
to 0.6/sec, so both columns moved and no percentage is derived from that
pair.

Walk versus idle in powermetrics (300 vs 187) says process wakeups still
follow animation. It does not say the leftover scavenger would follow a
smaller overlay allocation, because the leftover scavenger is mostly in
the host process, where Tauri's `evaluateJavaScript` lives, not in
WebContent.

### Whether one scavenger runs per WebContent

**One scavenger thread per process that links libpas, not per webview.**
`sample` named exactly one `JavaScriptCore libpas scavenger` in the host,
one in `WebKit.WebContent`, one in `WebKit.GPU`, and one in
`WebKit.Networking` (the last posted no kernel wakeups in these
windows).

This capture had one overlay and one WebContent (`overlay: 1
display(s)`). Two overlays sharing one webview is therefore
**cannot-measure here**. The per-process finding still prices that idea:
sharing would drop the extra WebContent's ~4/sec scavenger, not the
host's ~27/sec. `WKProcessPool` is deprecated on the SDK this machine
ships (`macos(10.10, 12.0)`) and "no longer has any effect".

### Whether any WKWebView or JSC knob bounds it

**Not from here.** The public `WKWebViewConfiguration` on this Mac's SDK
(`MacOSX.sdk/.../WKWebViewConfiguration.h`) has process pool (deprecated),
preferences, user content, website data store, media, HTTPS upgrade,
inline predictions, and similar. It has no scavenger period, no
scavenger disable, and no libpas field. `WKWebpagePreferences` can
disable content JavaScript or turn on lockdown / security-restriction
mode. Those are not a bound on the allocator thread, and lockdown is not
a product setting this overlay should flip to save 27 wakeups/sec.

JSC options are overridable as `JSC_<name>` before the first VM.
`libpasScavengeContinuously` is the only scavenger-shaped option in
`OptionsList.h`. Default already `false`. The host process applies it
(the `Modified JSC options` line above). It does not expose
`pas_scavenger_period_in_milliseconds`, and it does not disable the
thread. `pas_scavenger_suspend` exists in JavaScriptCore as SPI. It is
not a `WKWebViewConfiguration` property and not a JSC environment knob.

WebKit's `pas_scavenger.c` sleeps `pas_scavenger_period_in_milliseconds`
(100 ms on Darwin ARM in that source) and shuts the thread down after
10 s with no eligible pages. The leftover host scavenger is still posting
27/sec after a 15-second settle plus two 20-second windows, so it has not
shut down. 10 Hz would be 10/sec. This repository does not own that
thread's timer.

### What a small fix would have to be, and why it is not in this change

A reachable fix would be a public WebKit configuration that lengthens or
stops the scavenger, or a host-side stop to in-process JSC allocation
that lets it deep-sleep. Neither is present. Guessing `dlsym` of
`pas_scavenger_suspend`, or merging two overlays into one webview for a
4/sec WebContent row, is out of scope. The 16 ms frame loop is still
35/sec and is #183's line, not this issue's.

## How much to trust these numbers

**The package-idle sub-metric does not resolve.** That is the one column where
#718 could have shown a win. Three interleaved captures per arm put `main` at
2.92, 4.22, 5.09 and #718 at 3.24, 3.52, 4.32 package-idle wakeups/sec. #718's
whole range sits inside `main`'s. The median gap of about 17% falls on #718's
side but is smaller than the spread each arm shows against itself, so this
sample establishes neither direction nor magnitude. A real effect and no
effect are both consistent with it. This does not soften the headline: CPU and
total wakeups did not move at all, and those are not close calls.

**A withdrawn number, recorded so it is not re-quoted.** An earlier draft
reported one 60-second capture per arm and called the gap a **65-75%
reduction**, in bold. That was arithmetic on a single observation per arm. It
did not survive repetition and it is wrong.

**Shared machine.** A dev laptop with other agents building and running their
own work throughout every capture, including their own `fidget` instances.
That is the direct cause of the overlap above. Re-measuring chat-open fifteen
minutes later moved it ~60% on both arms with no code change. Rerun on a quiet
machine and expect different figures.

## The machine and the build

| | |
|---|---|
| Machine | MacBook Pro `Mac15,7`, Apple M3 Pro, 36 GB |
| OS | macOS 26.6.2 (25G83) |
| Build | `target/release/fidget`, **release**, per branch, ad-hoc signed at its worktree path |
| Director | `FIDGET_DIRECTOR=0` — `StaticDirector`, offline, no HTTP leaves the process |
| Character | Buddy Bot (the default when none is set) |
| Date | 2026-09-15 |
| Control arm, written `main` below | `7a58e02f` — this issue's base, **before** #718 |
| Treatment arm, written `#718` below | `9864d789` (`cursor/macos-idle-backoff-183-3b49`) |
| Baseline, idle and hidden arms, 2026-09-23 | `dac3c1ae` (`main` after #760 and #790), macOS 26.7 (25G229), two displays |
| Quiet-machine residency rerun, 2026-09-24 | `ecb92b8d`, macOS 26.7 (25G229), release build from `.worktrees/bench-960` |

**#718 has since merged** (`8ba9481d`). So the arm labelled `#718` throughout this
document is what `main` does today, and the arm labelled `main` is the pre-back-off
state that no longer exists on the branch. The A/B below is unaffected: it compares two
fixed commits and both are still in history. What changed is which arm a future reader
should treat as the baseline to beat. That is the `#718` column, not the `main` one.

Both binaries were built with `cargo build -p fidget --release` from a
dedicated worktree per branch (`.worktrees/431-macos-wakeups` and
`.worktrees/718-idle-backoff`), never in the primary checkout.

**This is a shared dev machine, not a clean-room rig.** Other agents were
building and running their own work on it throughout this capture, including
their own `fidget` instances (a concurrent, unrelated `fidget` process
under a different PID showed up in a real capture during this task — the
tooling below filters by exact PID for that reason, see Gotchas). Per-process
wakeup counts are scoped by PID, which rules out double-counting a different
process, but does not rule out this process itself running busier or
quieter depending on what else the machine is doing at that moment — see
Headline above for two measured examples of exactly that, one of which
(idle-perched) is why this document reports medians and ranges rather than
single numbers. The `cpu_power` sampler's idle-residency and package-power
numbers are **system-wide**, not per-process, so they carry the same
confound a second time, more directly.

## Tools

- `scripts/bench-wakeups-macos.sh --binary PATH --scenario idle|chat|hidden [--duration SECS] [--out DIR]` —
  launches the release binary with the env a worktree build needs
  (`FIDGET_DIRECTOR_API_KEY` skips the Keychain prompt, #283), waits for the
  overlay to report ready, optionally drives Summon (see below) or the
  fullscreen hide (see "Baseline (no fidget) and hidden, interleaved"), then runs
  `sudo powermetrics -i 1000 -n DURATION --samplers tasks,cpu_power` for the
  window and writes `powermetrics.txt` + `app.log` + `meta.txt` to `--out`.
  `--scenario baseline` launches nothing and refuses to sample while any
  `fidget` process is alive, since another agent's build would otherwise
  become the baseline without anyone noticing. Every `meta.txt` records the
  load average and whether `cargo`/`rustc` were running at the end of the
  capture.
- `scripts/fullscreen-window.swift [quit-after-secs]` — a borderless,
  click-through, near-transparent window covering the whole main display.
  `fullscreen_frontmost` in `crates/core/src/visibility.rs` reads rectangles
  only, so this fires the hide rule the same way a real fullscreen app does
  without taking the desktop from whoever is using the machine. It re-asserts
  itself at the front of ordinary windows every 50 ms, the same trick as
  `perch-window.swift`, and quits on its own as a backstop.
- `scripts/parse-powermetrics.py POWERMETRICS_TXT --pid PID [--frame-log APP_LOG]` —
  reduces the capture to wakeups/sec (interrupt and pkg-idle), CPU%, cluster
  idle residency and package power, for the exact PID. With `--frame-log` it
  also buckets seconds by whatever animation the frame trace says was on
  screen at that wall-clock second (idle-family vs. active, and per
  animation), so a single idle-perched capture that happens to catch a
  natural walk answers two scenarios at once. `--process ''` reduces a
  baseline capture to its system-wide numbers alone.
- `scripts/click-cursor.swift x y [clicks]` — posts one or two real HID
  left-clicks at a point, warping the cursor there first. **Shared with
  #728** (`crates/verify`'s `summon`/`poke` subcommands add the same path;
  one click poster lives in the tree, not two — this document's tooling
  calls #728's script rather than forking it). Needed because
  `osascript ... System Events click at` resolves an Accessibility UI
  element under the point first, and this overlay's borderless
  always-on-top panel does not present one to resolve — it fails with error
  -25208 even after the app's own hit-test has already flipped click-through
  off. `CGEventPost` needs the same Accessibility grant and posts to the HID
  event stream directly, the same path a real click takes. Confirmed against
  the running app's own `verbs: ... [Poke]` / `[Summon]` trace lines, not
  assumed.
- `sudo ktrace trace -f S0x0140 -T <secs> --csv` — the per-thread split
  above. `powermetrics` cannot attribute below the process, and this is what
  answers instead: every row carries the emitting thread and its process, and
  a `MACH_MKRUNNABLE` row's first argument is the thread being woken. Count
  by that argument for wakeups per thread, and keep only the rows whose
  emitting process is the kernel for the interrupt-driven ones.
- `sudo -n powermetrics` worked non-interactively in this environment (no
  password prompt). If a reviewer's machine prompts, powermetrics needs a
  session where `sudo` is already primed, or these numbers are not
  reproducible unattended — see Gotchas.

Every scenario's Summon (chat-open) uses `click-cursor.swift $CX $CY 2` at
the sprite's on-screen centre, read from the app's own `frame:` trace line,
after waiting for a `Grounded` or `Perched` state (the sprite spawns mid-air
and plays `Falling` first — clicking during that window misses because the
art has moved by the time the click lands, discovered by hitting it
directly). Every chat-scenario run in this document confirmed
`verbs:.*Summon` in the log; a capture that could not confirm it is not in
this document.

## Results: idle-perched, interleaved

Three 45-second captures per branch, run **A, B, A, B, A, B** (`main`, #718,
`main`, #718, `main`, #718) rather than all of one branch then all of the
other, so a drift in shared-machine load lands on both arms in the same
round instead of correlating with one branch. All numbers below are the
idle-family bucket (`idle` + `sit` animation-seconds) from
`scripts/parse-powermetrics.py --frame-log`.

| Run | Branch | n | Wakeups/sec (interrupt) | Wakeups/sec (pkg-idle) | CPU% |
|---|---|---|---|---|---|
| 1 | `main` | 37 | 369.1 | 5.09 | 14.6 |
| 2 | `main` | 37 | 369.0 | 4.22 | 14.6 |
| 3 | `main` | 37 | 387.5 | 2.92 | 22.3 |
| 1 | #718 | 36 | 368.5 | 4.32 | 14.1 |
| 2 | #718 | 35 | 300.4 | 3.52 | 14.9 |
| 3 | #718 | 37 | 384.5 | 3.24 | 21.4 |

Round 3's CPU% jump (22.3% and 21.4%, versus ~14–15% in rounds 1–2) landed on
**both** branches in the same round — direct evidence the interleaving is
doing its job: a load spike shows up as a shared-round effect, not as a
branch effect, which is exactly why it does not need to be thrown out.

**Median and range per arm:**

| | `main` (n=3 runs) | #718 (n=3 runs) |
|---|---|---|
| Wakeups/sec (pkg-idle), median [range] | 4.22 [2.92–5.09] | 3.52 [3.24–4.32] |
| Wakeups/sec (interrupt), median [range] | 369.1 [369.0–387.5] | 368.5 [300.4–384.5] |
| CPU%, median [range] | 14.6 [14.6–22.3] | 14.9 [14.1–21.4] |

**The ranges overlap.** #718's package-idle range (3.24–4.32) sits almost
entirely inside `main`'s (2.92–5.09). The median difference (4.22 vs. 3.52)
falls on #718's side, but it is smaller than the spread each branch shows
against itself across three runs on this machine, so this data resolves
neither the effect size nor the direction.

## Walking

Never triggered synthetically (this harness posts no synthetic walk into the
app); `StaticDirector` picked a `walk`-containing Behavior on its own inside
five of the six idle-perched captures above, pooled here instead of read
from a single opportunistic run:

| | `main` (3 runs, n=18 walk-seconds pooled) | #718 (3 runs, n=22 walk-seconds pooled) |
|---|---|---|
| Wakeups/sec (interrupt) | 369.1 | 373.4 |
| Wakeups/sec (pkg-idle) | 4.06 | 3.92 |
| CPU% | 16.3 | 16.4 |

No meaningful difference between branches while walking, and both read close
to their own idle-family numbers above — consistent with #718's own design
(a multi-frame animation is Active mode, unchanged 16 ms tick, on both
branches) and with #431's own hypothesis that walking should not read
differently from idle. Unlike the idle-perched table, this is not a claim
about a cross-branch delta; it is pooled n to replace what was previously a
single 5–7 second sample per branch, nothing more.

## Chat-open (single pair, not interleaved, no percentage derived)

Chat-open needs Summon (a driven double-click, sprite-position read, and a
verified trace line per run) rather than a plain launch-and-wait, so it was
not cheap enough to also run three-times-interleaved in this pass. **This
is one capture per branch, not a range, and no delta is derived from it:**

| Branch | n | Wakeups/sec (interrupt) | Wakeups/sec (pkg-idle) | CPU% |
|---|---|---|---|---|
| `main` | 30 | 417.4 | 4.28 | 14.13 |
| #718 | 30 | 416.8 | 3.98 | 15.06 |

**This pair itself already moved once, which is why no percentage is drawn
from it.** A first pass measured `main` at 2.64 pkg-idle wakeups/sec and
#718 at 1.63–2.12. Re-measured about fifteen minutes later, with no code
change other than swapping the click script for #728's shared one (a
one-time ~300 ms event inside a 30-second average — not a plausible cause of
a 60% shift), both branches moved to the ~4/sec figures in the table above.
Whatever moved both branches' chat-open number by the same ~60% in the same
fifteen minutes was the machine, not this document's code, and it is the
same effect the interleaved idle-perched runs were built to control for.
Treat this table as two single samples, not as a measurement of #718's
effect on chat-open.

## Baseline (no Fidget) and hidden, interleaved

The two scenarios from #431's list that the sections above left out, captured
2026-09-23 against `dac3c1ae` (`main` after #760 and #790, so the idle arm here
is the post-fix process, not the 369-wakeup one at the top of this document).
Same machine, now on macOS 26.7 (25G229), two displays this round
(`overlay: 2 display(s)`, a 1920×1080 main and a 1728×1117 second), release
build from `.worktrees/bench-431`. Three arms per round, run **baseline, idle,
hidden, baseline, idle, hidden, ...** so a load drift lands on all three arms
of the same round:

- **baseline** — `scripts/bench-wakeups-macos.sh --scenario baseline`. No
  fidget process alive; the script refuses to sample if one is, and
  `meta.txt` records the `pgrep -x fidget` result before and after.
- **idle** — the idle-perched scenario, rerun here so the fidget-vs-no-fidget
  comparison #431's last acceptance box asks for is a same-round pair rather
  than a comparison against a capture from a week earlier.
- **hidden** — `--scenario hidden`. The app launches, waits for a
  `Grounded`/`Perched` frame, then `scripts/fullscreen-window.swift` covers
  the main display and the script waits for the engine's own
  `presence: hidden over 500ms` line before sampling. A capture in which
  `presence: shown` appears afterwards is marked invalid and exits non-zero.
  While hidden the frame loop traced one `frame:` line per second (47–48 in
  45 s) against ~60/s visible, which is the hidden-idle path in
  `src-tauri/src/frame_loop.rs` sleeping to its next Director or sense
  deadline.

Five rounds ran; four are published. Round 3's hidden arm failed its own
proof: `presence: shown` for one second at second 35 of the sample (11
frames traced in that second, 1/s everywhere else), so something covered the
prop for a second and the sprite came back. That capture is not a hidden
sample, the harness said so, and the whole round is out because arms are
paired by round. Every published run had no `cargo` or `rustc` alive at the
end of its capture; the 1-minute load average is in the last column.

| Round | Arm | n | Wakeups/sec (interrupt) | Wakeups/sec (pkg-idle) | CPU% | E-Cluster idle % | P-Cluster idle % | Package mW | Load (1 min) |
|---|---|---|---|---|---|---|---|---|---|
| 1 | baseline | 45 | — | — | — | 23.5 | 84.4 | 729 | 3.34 |
| 1 | idle | 45 | 190.4 | 1.99 | 6.5 | 25.5 | 71.1 | 1311 | 3.46 |
| 1 | hidden | 45 | 129.6 | 1.58 | 2.3 | 26.2 | 78.3 | 735 | 3.50 |
| 2 | baseline | 45 | — | — | — | 27.0 | 83.7 | 663 | 2.88 |
| 2 | idle | 45 | 181.6 | 2.12 | 7.5 | 29.7 | 82.2 | 467 | 2.69 |
| 2 | hidden | 45 | 173.1 | 2.49 | 3.6 | 29.0 | 82.9 | 519 | 2.55 |
| 3 | baseline | 45 | — | — | — | 19.0 | 59.2 | 2007 | 2.96 |
| 3 | idle | 45 | 181.7 | 2.68 | 8.2 | 29.3 | 86.0 | 505 | 2.44 |
| 3 | hidden | 45 | 72.1 | 0.89 | 1.7 | 26.2 | 90.6 | 379 | 2.89 |
| 4 | baseline | 45 | — | — | — | 31.2 | 92.1 | 315 | 2.32 |
| 4 | idle | 45 | 204.6 | 3.98 | 9.5 | 30.6 | 87.8 | 386 | 3.76 |
| 4 | hidden | 45 | 189.4 | 2.49 | 4.3 | 29.0 | 89.4 | 410 | 3.65 |

Per-process columns are the exact PID the script launched. Cluster idle and
package power are system-wide. Round 3's baseline arm (59.2% P-Cluster idle,
2007 mW, with no fidget alive) is the machine doing something else for
those 45 seconds, and it is kept: dropping the inconvenient no-app round
would be the single-sample mistake this document already withdrew once.

**Median and range per arm, four rounds:**

| | baseline | idle | hidden |
|---|---|---|---|
| Wakeups/sec (interrupt) | — | 186.1 [181.6–204.6] | 151.3 [72.1–189.4] |
| Wakeups/sec (pkg-idle) | — | 2.40 [1.99–3.98] | 2.03 [0.89–2.49] |
| CPU% | — | 7.9 [6.5–9.5] | 2.9 [1.7–4.3] |
| E-Cluster idle residency | 25.2% [19.0–31.2%] | 29.5% [25.5–30.6%] | 27.6% [26.2–29.0%] |
| P-Cluster idle residency | 84.1% [59.2–92.1%] | 84.1% [71.1–87.8%] | 86.2% [78.3–90.6%] |
| Package CPU power | 696 mW [315–2007] | 486 mW [386–1311] | 464 mW [379–735] |

**Hidden costs less than a third of the CPU of perched, and the ranges do not
touch.** 7.9% [6.5–9.5] perched against 2.9% [1.7–4.3] hidden. That is the
hide rule working as #183 intended: no webview redraws, one engine tick a
second. Interrupt wakeups fall by median (186 to 151) but the hidden range is
wide (72–189) and overlaps perched, so wakeups resolve direction, not
magnitude. What stays while hidden is what #761 named: the host and WebKit
processes keep their own cadence whether or not the sprite is drawn, and the
hide rule does not reach it.

**Does per-cluster idle residency drop with fidget running? This data cannot
say.** Same-round differences, idle minus baseline: E-Cluster idle residency
+2.0, +2.7, +10.3, −0.6 points; P-Cluster +26.9, −13.3, −1.5, −4.3 points;
package power +582, −196, −1501, +71 mW. In three of four rounds the machine
was *more* idle with the character perched than with nothing running, which is not
a property of the character. The effect this box is looking for has a ceiling: a
process at 7.9% of one core on a 12-core package (6 P + 6 E) can move a
cluster's idle residency by under one percentage point, and the round-to-round
swing of the no-app arm alone is 12 points on E and 33 on P. On this machine
the residency number is set by everything else running, and the absence of one
process is below its noise floor. Neither direction nor magnitude is resolved.

What *is* resolved is the column the box is really about. "Wakeups/sec
(pkg-idle)" counts the wakeups that pulled the whole package out of idle, per
process, and it reads about 2 per second perched (2.40 [1.99–3.98]) and about
2 per second hidden (2.03 [0.89–2.49]). That is the direct measurement of
"does this process prevent package idle": yes, roughly twice a second, in both
states, an amount the system-wide residency cannot see here. A residency
comparison that could confirm it needs a machine with nothing else on it.

**That machine has since been found, and it did not change the verdict.** See
"The quiet-machine result" below: four more interleaved rounds with the no-app
arm's P-Cluster spread down from 33 points to 7.5, and the answer is still
Inconclusive, now because the effect is under the instrument's floor rather than
under the room's noise.

## Does Fidget keep the machine awake? No

This is the question "deep sleep prevention" is usually reaching for, and it is
not the question idle residency answers. macOS decides system and display sleep
from `IOPMAssertion`, not from C-states. A process keeps the machine awake only
by taking an assertion out, so the check is one command and it is decisive.

Measured 2026-09-24, release binary, overlay up, `FIDGET_DIRECTOR=0`:

```
$ pmset -g assertions            # before launch
   PreventUserIdleDisplaySleep    0
   PreventSystemSleep             0
   PreventUserIdleSystemSleep     1
   pid 80906(caffeinate) ...  pid 559(powerd) ...

$ pmset -g assertions            # fidget running
   PreventUserIdleDisplaySleep    0
   PreventSystemSleep             0
   PreventUserIdleSystemSleep     1
   (no assertion names fidget)
```

The count does not move and nothing names the character. The single assertion
present belongs to `caffeinate` and `powerd` and predates the launch, so take
the baseline in the same session rather than reading a lone count as ownership.

So the display and the system sleep on their normal idle timers with fidget
running. What the character does cost is package idle, about 2.32 wakeups a second
while perched, which is a different and much smaller claim. #741 owns reducing
that.

Run this before reaching for residency. It took one command and answered what
two rounds of interleaved `powermetrics` captures could not.

## What "C-state" means on this hardware

#431 asks for "C3/C6/C7 residency," which is Intel nomenclature. This machine
is Apple Silicon (M3 Pro); `powermetrics --samplers cpu_power` does not report
those states at all. The closest available analog is per-cluster idle
residency, from the same interleaved idle-perched runs above (median
[range] across the three runs per branch):

| | `main` | #718 |
|---|---|---|
| E-Cluster idle residency | 29.9% [16.2–30.8%] | 21.5% [15.2–29.6%] |
| P-Cluster idle residency | 74.8% [74.5–75.8%] | 74.9% [73.7–78.9%] |
| Package CPU power | 644 mW [564–1063 mW] | 856 mW [638–969 mW] |

These ranges overlap even more than the wakeup counts do — P-Cluster idle
residency in particular is essentially identical between branches. Read that
as the same shared-machine confound as above, not as evidence #718 has no
effect on idle residency: these are system-wide numbers, so anything else
running on the machine during a given round shows up here as strongly as
`fidget` does, or more. The per-process wakeup counts are the more
trustworthy comparison in this document; these cluster-residency and
package-power numbers are included for completeness and are not a second
confirmation of anything.

The comparison this section could not make when it was written, character against
no character, is now in "Baseline (no fidget) and hidden, interleaved" above,
and reaches the same verdict for the same reason: the no-app arm's residency
moved by more between rounds than one process could move it at all.

If what you want to know is whether the character stops the machine sleeping, this
section is the wrong one. See "Does fidget keep the machine awake?" above.

## Gotchas

- **Filter by PID, never by process name alone:** `powermetrics` is
  system-wide. During this task a second, unrelated `fidget` process
  (another agent's own build) appeared in a real capture under a different
  PID. `parse-powermetrics.py --process fidget` without `--pid` will warn
  and silently pick whichever line comes first in that sample — always pass
  `--pid`, which `bench-wakeups-macos.sh` prints on every run.
- **A single capture per branch is not a measurement, it is a sample:** The
  first draft of this document reported one 60-second run per branch as a
  65–75% reduction. Interleaved repetition (three runs per branch, A/B
  alternating) showed the true per-branch spread is wider than that gap.
  Do not trust a single A-vs-B comparison on this machine; interleave and
  report medians and ranges, or say explicitly that a pair is a single
  sample and derive no percentage from it (see Chat-open).
- **`osascript ... click at` does not work on this overlay:** See Tools;
  use `click-cursor.swift`, and use the shared one (#728's), not a fork of
  it — two scripts at the same path is a silent collision, not a merge
  conflict git will catch.
- **The sprite falls before it lands:** Compute the click point only after a
  `Grounded`/`Perched` frame line, not the first `frame:` line seen.
- **`sudo powermetrics` needs non-interactive sudo:** This environment had
  it (`sudo -n true` succeeded with no prompt). Where it does not, these
  captures cannot run unattended; there is no non-root substitute for
  per-process wakeup counts on macOS that this task found.
- **This machine is not quiet, and the numbers say so:** See Headline. Do
  not read a single capture as a stable constant; a rerun on a quiet
  machine is expected to disagree. One has since run, and it did disagree
  on the spreads and agree on the verdict — see "The quiet-machine result".
- **A build immediately before a capture is a build during it:** `cargo build`
  is not reliably a no-op on a second invocation in this tree, and the load
  average it leaves behind outlives the process by minutes. Put the build in
  its own step and gate the first capture on `sysctl -n vm.loadavg` coming
  back down. Measured: load 2.49 before a 17-second relink, 4.02 in the
  baseline capture that started straight after it.

## Completing the residency confirmation on a quiet machine

The last acceptance criterion in #431 asks whether per-cluster idle residency
drops with fidget running, to confirm deep sleep prevention. The baseline and
idle measurements exist (see "Baseline (no fidget) and hidden, interleaved"
above), but were taken on a shared development machine where background activity
produces larger residency swings than one 7.9%-CPU process can cause.

**#960 wrote the protocol below to settle that on a quiet machine, and this
section now carries both halves: its instructions, and the run they were written
to get.** The protocol ran on 2026-09-24 against `ecb92b8d`. The verdict is in
"The quiet-machine result" after the protocol block, and it is
**Inconclusive** — on #960's own definitions, and for a different reason than
#931 was.

### Prerequisites

- A Mac with no other builds or agents running in the background
- Non-interactive `sudo` for powermetrics (`sudo -n true` succeeds)
- A release binary built from `main` at the commit to be measured
- The 1-minute load average back at its floor before the first capture, checked
  with `sysctl -n vm.loadavg`, not assumed from the build having exited

**A quiet Apple Silicon Mac does not read near 100% idle on either cluster, and
waiting for that is waiting forever.** The run below was as quiet as this
machine gets (load 2.2–2.8, no `cargo`, no `rustc`, no other `fidget`) and its
no-app arm still read 83.8% median on P-Cluster and 25.3% on E-Cluster. macOS
parks its own background work on the E-cores, so E-Cluster idle residency is
structurally low on a desktop that is merely unattended. Treat the baseline
arm's *spread across rounds* as the quietness signal instead, which is what the
comparison actually needs. Under 10 points on P-Cluster is quiet enough to
attempt the measurement; #931 saw 33.

### Protocol

Run four interleaved rounds of baseline and idle-perched, following the same
A/B pattern used in "Baseline (no fidget) and hidden, interleaved".

**Build in a separate step and wait for the load average to come back down.**
The build is not part of the capture loop, and it is not enough to put it at the
top of the same script. On the 2026-09-24 run the load average was 2.49 at the
moment the loop was launched, the loop's own `cargo build` relinked the
`fidget` crate in 17 seconds, and round 1's baseline arm — the very next
command — recorded 4.02. That capture was discarded and the run restarted with
the build outside the loop. A build during a capture is the confound that made
#931 inconclusive, and a build immediately *before* one is the same confound.

```bash
# Step 1, on its own. Build, then wait for the machine to come back down.
cargo build -p fidget --release
BINARY="$PWD/target/release/fidget"
until awk -v l="$(sysctl -n vm.loadavg | awk '{print $2}')" \
  'BEGIN{exit !(l < 2.6)}'; do sleep 15; done
```

Replace `2.6` with whatever this machine reads when it is doing nothing.
`cargo build` is not reliably a no-op on a second invocation here, so do not
leave it inside the loop as insurance.

```bash
# Step 2. Four interleaved rounds: baseline, idle, baseline, idle, ...
OUT_BASE=".verify/residency-$(date +%Y%m%d-%H%M%S)"
mkdir -p "$OUT_BASE"

for i in 1 2 3 4; do
  echo "=== Round $i === $(date) load=$(sysctl -n vm.loadavg)" \
    | tee -a "$OUT_BASE/summary.txt"

  # Baseline: no fidget running. The script refuses if one is.
  scripts/bench-wakeups-macos.sh --scenario baseline --duration 45 \
    --out "$OUT_BASE/round${i}-baseline"

  sleep 10

  # Idle perched.
  scripts/bench-wakeups-macos.sh --binary "$BINARY" --scenario idle --duration 45 \
    --out "$OUT_BASE/round${i}-idle"

  echo "Round $i baseline:" >> "$OUT_BASE/summary.txt"
  scripts/parse-powermetrics.py "$OUT_BASE/round${i}-baseline/powermetrics.txt" \
    --process '' >> "$OUT_BASE/summary.txt"

  echo "Round $i idle:" >> "$OUT_BASE/summary.txt"
  IDLE_PID=$(grep '^pid:' "$OUT_BASE/round${i}-idle/meta.txt" | awk '{print $2}')
  scripts/parse-powermetrics.py "$OUT_BASE/round${i}-idle/powermetrics.txt" \
    --pid "$IDLE_PID" --frame-log "$OUT_BASE/round${i}-idle/app.log" \
    >> "$OUT_BASE/summary.txt"

  sleep 10
done
```

The per-round parse above is the whole summary. An earlier draft of this
protocol followed it with a second loop that re-extracted cluster residency, and
that loop does nothing: it pipes a `grep` of the capture into
`parse-powermetrics.py`, which reads its file argument and never reads stdin, and
it reuses the `IDLE_PID` the first loop happened to leave in scope — round 4's —
for all four rounds. The two lines it prints are right anyway, because cluster
residency is system-wide and does not depend on the PID, which is exactly what
makes it worth deleting rather than fixing. Read the per-round blocks in
`summary.txt`, or reduce the eight `powermetrics.txt` files with a parser of
your own.

### The quiet-machine result

Run 2026-09-24 on the same `Mac15,7` as everything above, macOS 26.7 (25G229),
release build of `ecb92b8d` from `.worktrees/bench-960`. Four rounds, baseline
then idle, 45 seconds each, 10 seconds between arms. Every capture's `meta.txt`
recorded `cargo_or_rustc_running: 0`, no `fidget` alive before a baseline arm,
and the load average below. Per-process columns are the exact PID the script
launched; cluster residency and package power are system-wide, over the whole
45-second capture rather than the idle-family bucket, so the two arms are
measured the same way.

| Round | Arm | n | Wakeups/sec (interrupt) | Wakeups/sec (pkg-idle) | CPU% | E-Cluster idle % | P-Cluster idle % | Package mW | Load (1 min) |
|---|---|---|---|---|---|---|---|---|---|
| 1 | baseline | 45 | — | — | — | 25.0 | 83.5 | 790 | 2.58 |
| 1 | idle | 45 | 178.9 | 2.28 | 8.4 | 24.4 | 81.9 | 676 | 2.24 |
| 2 | baseline | 45 | — | — | — | 25.6 | 88.7 | 622 | 2.60 |
| 2 | idle | 45 | 179.8 | 2.44 | 8.5 | 26.3 | 81.7 | 635 | 2.81 |
| 3 | baseline | 45 | — | — | — | 26.2 | 84.0 | 830 | 2.21 |
| 3 | idle | 45 | 179.9 | 1.77 | 8.0 | 22.0 | 78.9 | 855 | 2.82 |
| 4 | baseline | 45 | — | — | — | 23.0 | 81.2 | 1160 | 3.54 |
| 4 | idle | 45 | 186.4 | 2.36 | 8.4 | 24.0 | 79.2 | 762 | 3.01 |

Nothing was dropped. Round 4's baseline arm is the noisiest capture in the set
(load 3.54, 1160 mW, and the lowest P-Cluster idle of any baseline at 81.2%),
and it is the round that most narrows the gap this section is looking for. It
stays, for the same reason #931's round 3 stayed.

**Median and range per arm, four rounds:**

| | baseline | idle |
|---|---|---|
| Wakeups/sec (interrupt) | — | 179.8 [178.9–186.4] |
| Wakeups/sec (pkg-idle) | — | 2.32 [1.77–2.44] |
| CPU% | — | 8.4 [8.0–8.5] |
| E-Cluster idle residency | 25.3% [23.0–26.2%] | 24.2% [22.0–26.3%] |
| P-Cluster idle residency | 83.8% [81.2–88.7%] | 80.4% [78.9–81.9%] |
| Package CPU power | 810 mW [622–1160] | 719 mW [635–855] |
| Load average (1 min) | 2.59 [2.21–3.54] | 2.81 [2.24–3.01] |

**The machine was quiet, and the numbers prove it rather than assert it.** Every
spread narrowed against #931's run on the same hardware: the no-app arm's
E-Cluster range went from 12.2 points to 3.2, its P-Cluster range from 32.9
points to 7.5, and the idle arm's CPU% from a 3.0-point spread to 0.5. The
precondition this protocol was written to obtain was obtained.

**Same-round deltas, idle minus baseline:**

| Round | E-Cluster | P-Cluster | Package power |
|---|---|---|---|
| 1 | −0.5 pts | −1.7 pts | −113 mW |
| 2 | +0.7 pts | −7.0 pts | +13 mW |
| 3 | −4.1 pts | −5.1 pts | +25 mW |
| 4 | +1.0 pts | −2.1 pts | −398 mW |

**P-Cluster now resolves a direction, and it is the direction #431 predicted.**
All four rounds read less P-Cluster idle residency with the character perched than
with nothing running. #931's four rounds split two up and two down on the same
column, which is what "cannot say" looked like. That is the one thing a quiet
machine bought. E-Cluster still swings both ways (−0.5, +0.7, −4.1, +1.0) and
resolves nothing.

**It does not resolve a magnitude, and on #960's own bar that is
Inconclusive.** The P-Cluster medians are 83.8% against 80.4%, a gap of 3.4
points where the protocol asks for at least 5. The ranges overlap by 0.6 points
(baseline bottoms out at 81.2%, idle tops out at 81.9%), which meets the
"minimally overlapping" half of the Confirmed bar and only that half. Against
the Inconclusive definition the answer is plainer still: the baseline arm's own
variation across rounds is 7.5 points, more than twice the 3.4-point gap it
would have to explain. Four rounds of one sign is a sign test at p = 0.0625,
suggestive and not significant.

**This is a different Inconclusive from #931's, and that is the finding.** #931
said the machine was too loud to see the effect. This says the machine is quiet
and the effect is still smaller than cluster residency can resolve, which is
what the arithmetic predicted all along: a host process at 8.4% of one core,
plus the WebKit services #741 measured at roughly the same again, is a few
percent of a six-core P-Cluster's capacity, and a few percent is where the
measured 3.4-point gap sits. Chasing a larger gap means chasing something that
is not there. Another four rounds would narrow the ranges and not move the
ceiling.

**The column that does answer the question has not changed.** Per-process
`pkg-idle` wakeups read 2.32/sec [1.77–2.44] perched, in line with the
2.40/sec #931 measured on a loud machine. That is a direct, per-PID count of
wakeups that pulled the whole package out of idle, it resolves cleanly in both
environments, and it says yes: this process prevents package idle, about twice
a second. The system-wide residency column cannot see a process that small, and
two runs on two noise floors now say so.

### What "confirm" versus "inconclusive" looks like

After running the protocol, examine the per-cluster idle residency medians and
ranges:

**Confirmed (deep sleep prevented):** The idle-perched arm shows consistently
lower cluster idle residency than baseline, with non-overlapping or minimally
overlapping ranges across the four rounds. Specifically:

- P-Cluster idle-perched median lower by ≥5 percentage points
- Ranges that do not fully overlap (e.g., baseline 88-95%, idle 75-82%)
- E-Cluster may show smaller or no effect (it runs lower-power cores)

The first bar this list carried, "baseline median near 90-100% idle," is struck.
The quiet run above never reached it and no unattended Apple Silicon desktop
will; see Prerequisites. It was a precondition on the machine, not a property of
the result, and leaving it in makes a reachable Confirmed look unreachable.

Example of confirmation:

| Arm | P-Cluster idle % median [range] |
|---|---|
| baseline (no app) | 92.5% [90.1-94.2%] |
| idle perched | 78.3% [75.8-80.1%] |

The 14-point gap with non-overlapping ranges confirms the process prevents
deep sleep.

**Inconclusive (noise floor):** The baseline arm's own variation across rounds
is as large as or larger than the difference between baseline and idle. Both
runs of this protocol landed here, for reasons worth telling apart.

| | #931, shared machine | 2026-09-24, quiet machine |
|---|---|---|
| Baseline P-Cluster | 84.1% [59.2–92.1%] | 83.8% [81.2–88.7%] |
| Idle P-Cluster | 84.1% [71.1–87.8%] | 80.4% [78.9–81.9%] |
| Baseline's own spread | 32.9 pts | 7.5 pts |
| Median gap | 0.0 pts | 3.4 pts |
| Same-round delta signs | 2 down, 2 up | 4 down |

On the shared machine the background activity dominated the signal and the
measurement was unanswerable there. On the quiet machine the signal is not
dominated and the direction comes out, but the gap is still smaller than the
baseline's own spread and short of the 5-point bar. **A second Inconclusive on a
quiet machine is not a repeat of the first.** It says the effect is below what
system-wide cluster residency can resolve for a process this size, which is a
result about the instrument rather than about the room.

Either way the `pkg-idle` wakeup count (about 2/sec for fidget) is the direct
per-process measurement that does resolve, and it does not answer the
cluster-level residency question the acceptance box asks.

### Script validation

The claim that the harness needed no changes survived the run. Eight captures,
eight clean exits, no edit to `scripts/bench-wakeups-macos.sh` or
`scripts/parse-powermetrics.py`. It already:

- Refuses to run a baseline while any `fidget` process is alive
- Records load average and `cargo`/`rustc` status in `meta.txt`
- Filters `powermetrics` output by exact PID (not process name)
- Proves the scenario via frame log (Summon for chat, `presence: hidden` for hidden)

The inconclusive result from #931 was environmental, not a tooling defect, and
that reading held up.

What did not survive the run was the surrounding protocol, in two places, both
fixed above. The build belongs outside the capture loop behind a load-average
gate, because the loop's own `cargo build` relinked `fidget` and put 4.02 on
the load average of the baseline capture that followed it. And the trailing
cluster-residency loop is dead code that reads correctly by accident. Neither is
a criticism of writing a protocol without a Mac to run it on; they are the two
things only a run could find.

## Not measured

Scoped out per this task's instructions, not fabricated:

- **Multi-monitor** — #424's scope, not this issue's.
- **Baseline (no fidget running)** and **hidden/fullscreen** — measured
  since, see "Baseline (no fidget) and hidden, interleaved" above, and
  rerun on a quiet machine in "The quiet-machine result". A clean per-cluster
  idle-residency comparison is the one thing neither run produced, and the
  second run says why: the effect is below what a system-wide residency
  column can resolve for a process this size, not merely below the shared
  machine's noise.
- **Linux and Windows** — #431 is macOS-only (#432 covers Linux).
- **Chat-open, interleaved** — left as a single pair per branch; see
  Chat-open above for why and for the explicit no-percentage rule that
  applies to it.
- **A tighter idle-perched effect size** — three interleaved runs per branch
  established direction, not a resolved magnitude (see Headline and the
  idle-perched table). More runs, or a quieter machine, would narrow the
  range; this document stops at the n the task asked for.
- **What actually produces the ~300–450 wakeups/sec baseline** — isolated
  since, under #741. See "What the 369 is made of". The candidates this
  document originally named (the async runtime, log rotation, the tray icon)
  are none of them.
- **Two overlays sharing one webview** — #761 needed one scavenger per
  `WebContent` first. This capture had one display and one `WebContent`,
  so a share-the-webview experiment is cannot-measure here. The
  per-process census is the answer that experiment would have used.
