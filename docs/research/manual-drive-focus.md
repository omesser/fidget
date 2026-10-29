# Manual drive: the overlay's keyboard with no pill open

Spike for [#1920](https://github.com/omesser/fidget/issues/1920), step 1 of
[#1655](https://github.com/omesser/fidget/issues/1655). Question: can an
overlay take the keyboard through `overlay_request_focus` with no quick message
pill open, hear key edges, lose the keyboard to a click elsewhere, take it back
on a click, and still pass clicks outside the art through, on macOS, Windows
and X11?

**Anchor.** The tree read is `main` at `2f9ad5c4`. The live runs are the
throwaway probe branch `spike/1920-drive-focus-probe` at `e34dcf28`, which is
`2f9ad5c4` plus the probe and nothing else. One macOS run is at `65b4043c`, the
probe's second commit, whose macOS leaf and helpers match `e34dcf28`. Written
2026-10-29. This document does not follow `main`.

**Answer.** Yes on all three. Every item in #1920 is proven on a CI runner of
each OS, with no change to the focus path. The pill's `overlay_request_focus`
works unchanged with no pill. Two findings shape the drive build: a click on
the sprite takes the keyboard back by itself while the overlay stays focusable,
and the focusable flag the request sets is never cleared when no pill closes.

---

## The probe

The probe adds no product behavior unless `FIDGET_PROBE_DRIVE_FOCUS` is set.

- **Trigger:** on its first frame, `overlay-0` calls `overlay_request_focus`
  from `src/main.js`, the same command the pill calls. No pill opens, so
  `pill_focus::OpenPills` never hears of it.
- **Resume:** a `pointerdown` on `.sprite` calls `overlay_request_focus` again.
- **Trace:** each `keydown`, `keyup` (with `event.repeat`), DOM `focus`/`blur`
  and sprite `pointerdown` goes to stderr as a `drive-probe:` line, and
  `build_overlay` traces each `tauri::WindowEvent::Focused` the overlay gets.
- **Leaves:** `scripts/scenarios/ci/drive-focus.sh`, `drive-focus.x11.sh` and
  `drive-focus.win.ps1`, built from `control-click-menu.sh`,
  `chat-close-pill-type.x11.sh` and `question-bubble.win.ps1`. Each waits on
  trace lines through `wait_for_trace` or `Wait-ForTrace` and marks every item
  on its own, so one miss does not hide the rest.

The second window is a plain app window the leaf owns: a Swift `NSWindow`
(`drive-focus-window.swift`) on macOS, mousepad under xfwm4 on X11, and a
WinForms form (`drive-focus-window.win.ps1`) on Windows. Each one reports the
click and the key it receives, so pass-through is checked at the receiving end.

The overlay covers the whole display, so a click on the second window is a
click on the overlay outside the art. One click proves both "another window
takes the click" and "the overlay loses the keyboard".

## macOS

Run [40843085300](https://github.com/omesser/fidget/actions/runs/40843085300)
at `e34dcf28`, repeating
[40842571054](https://github.com/omesser/fidget/actions/runs/40842571054) at
`65b4043c`. Both passed every item.

| Item | Verdict | Trace |
|---|---|---|
| Overlay takes the keyboard, no pill | proven | `drive-probe: overlay-0 Focused(true)`, `request launch ok`; the helper window logs `window: resign key` |
| `keydown` and `keyup` reach the webview | proven | `keydown key=ArrowRight repeat=false`, `keyup key=ArrowRight repeat=false` |
| `event.repeat` | proven | `keydown key=ArrowRight repeat=true` |
| Click on another window gives `Focused(false)` | proven | `Focused(false)`, `dom blur` |
| Clicks outside the art pass through | proven | `click: down hits=[false]`; the helper window logs `window: mousedown` and `window: keydown z`, and the overlay logs no `z` |
| Click on the overlay takes the keyboard back | proven | `Focused(true)`, then `pointerdown sprite=true`, `request click ok`, `keydown key=ArrowLeft` |

## Windows

Run [40843061086](https://github.com/omesser/fidget/actions/runs/40843061086)
at `e34dcf28`.

| Item | Verdict | Trace |
|---|---|---|
| Overlay takes the keyboard, no pill | proven | `Focused(true)`, `request launch ok hasFocus=true`; the form logs `window: deactivate`; `GetForegroundWindow` names `Fidget` |
| `keydown` and `keyup` reach the webview | proven | `keydown key=ArrowRight repeat=false`, `keyup key=ArrowRight repeat=false` |
| `event.repeat` | proven | `keydown key=ArrowRight repeat=true` |
| Click on another window gives `Focused(false)` | proven | `Focused(false)`, `dom blur` |
| Clicks outside the art pass through | proven | the form logs `window: mousedown` and `window: keydown Z`; the foreground is `drive-focus-window` |
| Click on the overlay takes the keyboard back | proven | `Focused(true)`, `pointerdown sprite=true`, `request click ok`, `keydown key=ArrowLeft` |

At launch the overlay logs `Focused(true)`, `Focused(false)`, `Focused(true)`
within about a second (the trace has one-second resolution) as
`SetForegroundWindow` runs under `AttachThreadInput`. The same
`Focused(false)`, `Focused(true)` pair follows `request click ok` on the
resume. A pause handler must not treat a `Focused(false)` that is followed at
once by `Focused(true)` as the user clicking away, at launch or on any resume.

## X11

Run [40843057277](https://github.com/omesser/fidget/actions/runs/40843057277)
at `e34dcf28`, attempt 2. Attempt 1 of the same run failed before any leaf
ran: the workflow's own Xvfb did not start in 3 s.

| Item | Verdict | Trace |
|---|---|---|
| Overlay takes the keyboard, no pill | proven | `Focused(true)`, `request launch ok`; `_NET_ACTIVE_WINDOW` names the overlay |
| `keydown` and `keyup` reach the webview | proven | `keydown key=ArrowRight repeat=false`, `keyup key=ArrowRight repeat=false` |
| `event.repeat` | proven | `keydown key=ArrowRight repeat=true` |
| Click on another window gives `Focused(false)` | proven | `Focused(false)`, `dom blur` |
| Clicks outside the art pass through | proven | mousepad becomes `_NET_ACTIVE_WINDOW`, and its saved file reads `abczz` after `xdotool type zz` |
| Click on the overlay takes the keyboard back | proven | `Focused(true)`, `pointerdown sprite=true`, `request click ok`, `keydown key=ArrowLeft`; `click: down hits=[true]` |

This is xfwm4, a click-to-focus window manager. A focus-follows-mouse window
manager is not tested.

## What the runs show beyond the table

- **The sprite click resumes by itself:** on all three OSes, `Focused(true)`
  lands before the webview's `pointerdown`, so the click activates the overlay
  before the probe's second `overlay_request_focus` runs. On macOS and X11 that
  is because `focusable` stays set after the launch request. The second request
  is harmless, but drive cannot rely on the request alone being what resumes.
- **Focusable is never cleared without a pill close:** `overlay_report_qm_draft`
  sets `focusable(false)` only when a pill's flip closes, and
  `platform::hand_back_keyboard` runs only from there. A drive release must do
  both itself. Until it does, every later sprite click on macOS or X11 takes
  the keyboard from the user's app, which is the regression #1699 fixed.
- **DOM focus lags the invoke:** the native `Focused(true)` landed before
  `request launch ok` in the final runs and after it in an earlier X11 run, so
  its order against the invoke varies. `request launch ok` reports
  `hasFocus=false` on macOS and X11 and `true` on Windows, and the DOM `focus`
  event follows. Drive should treat the DOM `focus` event, or `Focused(true)`
  on the Rust side, as the moment it holds the keyboard, never the invoke's
  return.
- **A press needs the pointer to arrive first:** on X11 and Windows a
  synthetic move and press in the same instant reached the window below. The
  frame loop makes the art clickable on the tick after the pointer arrives, so
  the press passed through and the webview saw no `pointerdown`, even where
  the frame loop's own hit test logged `click: down hits=[true]`: that test
  reads the pointer against the drawn sprite, while the window's input region
  or input shape is what decides where the press is delivered, and it lagged
  (runs
  [40842574888](https://github.com/omesser/fidget/actions/runs/40842574888) and
  [40842578998](https://github.com/omesser/fidget/actions/runs/40842578998)).
  A hand always gives that tick. The drive scenarios in #1655 step 8 must wait
  for a frame between the move and the press, as these leaves now do.
- **The hover pill can fake a resume:** in run 40842578998 the pointer rested
  on the sprite after the click, the pill opened on hover, and its focus request
  produced the `Focused(true)`. The leaves move the pointer off after the
  press. Drive will meet the same case for real: a hover on the sprite while
  paused opens a pill, and the pill takes the keyboard.

## Limits

- **Repeat is synthetic:** a runner has no hardware auto-repeat. macOS sends a
  second `keyDown` with `kCGKeyboardEventAutorepeat` set. X11 and Windows send
  a second press with no release between. All three map to `repeat=true`,
  which is the field drive filters on. Hardware auto-repeat is not tested.
- **One display:** each runner has one display. Drive on a second display, the
  case #1572 covers, is not tested.
- **One launch per OS:** the #1578 race needs about 24 launches to show. Not
  tested here.
- **Escape and hand-back:** the probe never releases. `hand_back_keyboard` with
  no pill is not tested.
- **Wayland and focus-follows-mouse X11:** not tested. No runner has either.

## What #1578 and #1572 mean for drive

**#1578 (open):** on X11, a sprite click about 1 s after launch once left the
overlay active with no pill. The probe shows the mechanism that makes this
worse under drive: once `focusable` is set, a sprite click activates the
overlay on X11 and macOS by design. Under drive that is the resume, so it is
wanted while possessed. After a release it is the #1578 symptom on every
click, not one launch in 24. The release path has to clear `focusable` and
hand the keyboard back, and its scenario has to assert that a sprite click
after Escape leaves typing in the editor, as `chat-close-pill-type.x11.sh`
case 1 does.

**#1572 (closed by #1694):** typing went to the wrong app when pills on two
displays closed, because one remembered app serves every overlay. #1694 hands
back only when the last pill in the process closes. Drive calls
`overlay_request_focus`, which calls `remember_front_app` on macOS and
`remember_foreground` on Windows and X11. That memory is the same one the pills
use. A pill opened mid-drive finds an overlay in front, and each of those
remember functions skips an overlay, so the app remembered at drive start
survives. But a pill closing mid-drive is a close that empties `OpenPills`,
because drive is not in it, and it hands the keyboard back mid-drive. The same
close flip also sets `focusable(false)` on that overlay, on macOS and X11, even
when another display's pill keeps the process from emptying. That breaks the
sprite-click resume until drive asks for focus again. That is the case #1655's
"keyboard bookkeeping" bullet names. Drive has to count in `OpenPills`, or the
hand-back has to ask whether drive holds the overlay.
