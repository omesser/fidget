# Renderer choice and single-witness click detection

Research for #184, prompted by #171 (overlay latch + session poll), #182 (sub-tick click drop), #183 (60Hz poll while idle). Question: does the Tauri + webview paint surface force two witnesses for click detection, and what would have to move for there to be one?

**Answer.** The webview is incidental. The two-witness OR (#171) is what you get from three constraints together: **full-display overlay** (for multi-display seams), **per-window click-through** (the only API AppKit and GTK offer), and **no event tap** (to stay under TCC decision 9). Changing renderer alone — NSView, wgpu, or keeping the webview but moving button reporting to Rust — changes the implementation surface but not the input witness count. An honest single witness requires relaxing one of those three constraints, and each carries a cost previously refused. Stay two-witness, and treat #182 (fixed by #236) and #183 (permission spike, then event-driven if permitted) as tuning of the current design rather than a reason to change renderer.

## Why two witnesses exist

The overlay is a paint surface, not an input owner. Each display gets a transparent always-on-top window that covers it completely (`src-tauri/src/main.rs`: `create_overlay`). Rust decides where the sprite is; the webview draws it. The window is full-display because a Character on a seam must be whole — each overlay draws the slice that falls inside it, and a sprite-sized window cannot straddle two Spaces on macOS (ADR-0001, DESIGN.md decision 24).

Because the window is the whole screen, it would eat every click unless it is click-through. Tauri (and AppKit's `NSWindow.ignoresMouseEvents`, GTK's input shape) only offer that per window, not per pixel, so Rust alpha-hit-tests the sprite and toggles `ignore_cursor_events` (`src-tauri/src/frame_loop.rs`, lines 1680 and 1709). Off over opaque art, on everywhere else.

The Engine never sees a click. It receives `Verb::Poke` after the shell has decided. `src/main.js` was written as "the webview owns no state."

The original input design is one source: poll the OS. Every tick, Rust asks where the cursor is and whether the primary button is down (`CGEventSourceButtonState` on macOS, `src-tauri/src/platform/macos/pointer.rs`). That needs no Accessibility permission (DESIGN.md decision 9), and it keeps working when a drag leaves the pixels — which a webview cannot, because the moment click-through turns back on, the window gets no mouse events.

When click-through is off (cursor over the art), our window swallows the click so the app underneath does not get it. On macOS, `CGEventSource` has been seen to stay false for that swallowed press. #171 added a second witness: while the overlay is listening, the webview gets `pointerdown`/`pointerup` and JS latches that into Rust via `overlay_primary` invoke (`src/main.js`, lines 347–392). Either source counts.

They are not two algorithms. They cover disjoint regimes the click-through toggle creates:

| | Overlay listening (over art) | Overlay click-through (everywhere else) |
|---|---|---|
| Webview events | hears the button | hears nothing |
| `CGEventSource` | can miss (window swallowed it) | sees a drag that left the pixels |

The JS latch is not a second input architecture. It is a patch on the renderer we already had. The same latch could be an `NSEvent` monitor or a Tauri window mouse handler in Rust and would still be two witnesses.

Painting still needs some renderer. Moving draw to a native `NSView` or wgpu would not by itself give a single button API — you would still have a full-display transparent window and the same click-through hole.

## What one deterministic approach would actually be

Three honest singles, each with a cost previously refused:

1. **CGEvent tap** (or Accessibility). One stream of real clicks, independent of our window. Permission prompt, and a different privacy story than "session state query, no tap." `docs/research/event-driven-input-vs-polling.md` documents the gate: a listen-only `CGEventTap` on mouse events falls under Input Monitoring, not Accessibility. Apple documents the gate for keyboard events and says nothing about mouse-only taps. The permission is unverified until the spike in #183 tests it. DESIGN.md decision 9 forbids the tap in v1 until that answer exists.

2. **Sprite-sized window** (or a shaped view) that only covers the art. Clicks on the pet always hit that window; the desktop never does. No toggle, one event path. Fights the seam / Dock "full display" requirement: a window smaller than the whole display cannot straddle two Spaces (macOS enforces this), so a Character on a seam would be clipped, and the sprite would fall behind the Dock rather than standing in front of it at the floor. Multi-display seams are a requirement (DESIGN.md decision 24) unless explicitly dropped.

3. **Stay click-through off and forward non-sprite clicks** into the app underneath. We become a proxy for the whole screen, hearing every click and deciding whether to handle it or inject it back to the window below. Wrong product: ownership of every desktop click to decide per-pixel pass-through is not "session state query" and is not the privacy boundary decision 9 chose. A CGEvent tap for injection would itself require Accessibility.

The two-witness OR is what you get if you keep all three current constraints: full-display overlay, no event tap, per-window click-through. Coherent as a patch. Not a principle.

## Paint-only changes do not remove a witness

### Webview latch → Rust handler

Keep Tauri webview paint, but hear the overlay press in Rust. Tauri's `on_window_event` sees `WindowEvent::CursorMoved` and `WindowEvent::MouseInput` only when the window is not ignoring events (per-window click-through still applies), so this is the same latch in Rust instead of JS. Still two witnesses; less surface (no invoke round-trip).

### Native/GPU paint

An `NSView` or wgpu renderer without changing the full-display + click-through model. The click-through API is the window's, not the renderer's: `NSWindow.ignoresMouseEvents` is what Tauri calls under the hood. The hit-test still runs per tick to decide the toggle, and the two input regimes (webview events vs session poll, or NSView events vs session poll, or GPU input callback vs session poll) are still disjoint. Expected finding: input stays two-witnessed; measure whether paint or footprint actually improves only if prototyping for another reason (e.g., testing GPU memory cost).

On X11, `wl_surface.set_input_region` with a `wl_region` is core Wayland per-pixel click-through (`docs/research/event-driven-input-vs-polling.md`, line 113), and tao already uses it for the 1×1 shape during `CursorIgnoreEvents`. `x11/overlay.rs` matches only Xlib and Xcb handles and drops the Wayland one. Wiring it is our work, not the renderer's, and the work is identical whether the paint is webview or wgpu.

## What stays

The Engine stays pure and tick-driven (DESIGN.md decision 5). Verbs stay decided in Rust (`Pointer` in `crates/core/src/input.rs`). Multi-display seams stay a requirement unless this issue explicitly drops them. Chat (#17) can keep a webview even if the sprite does not (ADR-0001 contemplated this split).

## Relationship to #182 and #183

**#182 (sub-tick click drop)** was fixed by #236 (`Witness::take`, the sticky bit). `Pointer::update` derives the press edge from `held && !was_held` (`crates/core/src/input.rs`, line 152). Any event producer drained once per tick must carry a sub-tick down+up across the drain; a two-element queue and an edge bit are equivalent for that. `Witness::take` is the edge bit. If a session tap replaces the webview witness, the producer changes and `take` stays. #236 is the minimal correct fix today and is kept, not replaced, under any renderer or witness design.

**#183 (60Hz poll while idle)** asks whether events can replace the poll. `docs/research/event-driven-input-vs-polling.md` answered that: macOS permission is unverified; X11 is event-driven and prompt-free (XI2 raw events); Wayland has no global pointer to poll or hear; events do not remove the hit-test that decides click-through. The loop can block on `recv()` when idle only if a mouse-moved tap is also permission-free. #183 remains in-scope of the current overlay as a permission spike (Stage 1) followed by event-driven input if the spike passes (Stage 2a) or an idle back-off if it fails (Stage 2b). Renderer choice does not change that.

Neither issue is a reason to reopen ADR-0001. The webview is not the thing forcing two witnesses; the window shape and click-through API are.

## Recommendation

1. **Stay two-witness:** The OR is correct for the constraints chosen. #236 fixed sub-tick drop; #183 addresses idle poll separately.
2. **Do not reopen ADR-0001** unless a prototype shows the webview is the thing forcing two witnesses. Current reading: it is not.
3. **If a paint change is prototyped for another reason** (e.g., memory footprint on GPU, renderer maintenance cost), measure battery and resident set size, not input witnesses. Input will stay two-witnessed under the current window model.
4. **If a single witness is required later**, choose which constraint to relax and accept its cost:
   - Event tap → Input Monitoring prompt (test permission first, #183 Stage 1).
   - Sprite-sized window → seams break, Dock overlay lost (document as degraded, not v1).
   - Click forwarding → wrong privacy boundary, requires Accessibility for injection.

## References

- ADR-0001: Greenfield Tauri, not fork WindowPet
- DESIGN.md decisions 5 (Director outside loop), 9 (no permissions until they buy), 24 (multi-display)
- `docs/research/event-driven-input-vs-polling.md`: TCC gates, X11/Wayland/macOS input APIs
- Issue #171: overlay latch + session poll
- Issue #182: sub-tick click drop (CLOSED, fixed by #236)
- Issue #183: 60Hz poll while idle (OPEN, permission spike planned)
- Issue #236: `Witness::take` sticky bit (merged)
- `src-tauri/src/platform/macos/pointer.rs`: `CGEventSourceButtonState` poll
- `src-tauri/src/frame_loop.rs`: click-through toggle (lines 1680, 1709)
- `src/main.js`: webview `pointerdown`/`pointerup` latch (lines 347–392)
- `crates/core/src/input.rs`: `Pointer::update`, `press_target`
- `crates/core/src/overlay.rs`: alpha hit-test, `AlphaMask::hit`
