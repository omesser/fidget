# Platform Support Degraded Cells Investigation

Investigation of the four "degraded" cells in the README Platform Support table, per issue #1195.

sha: `ab6bd57b`

## Summary

| Cell | Current Behavior | Root Constraint | V1 Disposition |
|---|---|---|---|
| Linux: Dock/panel Perch | Only bottom panels detected | Implementation only checks strut[3]; side/top panels ignored | **Pursue to `yes`** - fixable |
| Windows: Dock/panel Perch | Taskbar from work area (full-width strip) | No Windows API equivalent to CoreDockGetRect for exact bounds | **Document as accepted degrade** |
| Windows: Fullscreen fade | Works for true fullscreen; unreliable for borderless windowed or maximized apps | Windows apps use mixed fullscreen modes; no reliable exclusive-fullscreen detection | **Document as accepted degrade** |
| Wayland: Fullscreen fade | Native Wayland fullscreen apps do not trigger fade or move | Wayland gives clients no global window list by design; no compositor-agnostic detection | **Document as accepted degrade** |
| Linux: Capturable opt-out | Always capturable; setting has no effect | Linux has no platform API to exclude windows from screen capture | **Already documented** in ADR-0024 |

## Linux: Dock or panel as a Perch

**Current behavior:**  
Bottom panels (like a bottom GNOME panel or bottom-configured taskbar) are detected and used as Perches. Side panels (left/right) and top panels are not detected; they stay as full-width reserved strips, and the Character cannot perch on them.

**Root constraint:**  
Implementation, not platform. The code in `src-tauri/src/platform/x11/window_source.rs:strut_panel_bounds()` reads `_NET_WM_STRUT_PARTIAL` from dock/panel windows but only checks `strut[3]` (bottom edge). The EWMH `_NET_WM_STRUT_PARTIAL` property contains 12 values:

```
strut[0] = left edge thickness
strut[1] = right edge thickness
strut[2] = top edge thickness
strut[3] = bottom edge thickness
strut[4-11] = start/end coordinates for each edge
```

The function only handles `strut[3] > 0` (bottom panels), ignoring strut[0], strut[1], and strut[2].

**Options to complete:**

1. **Extend `strut_panel_bounds` to handle all four edges.** Check `strut[0-3]` and construct the appropriate `Rect` for each case. The coordinates in `strut[4-11]` provide the span for each edge.

2. **Test with real panels.** GNOME, KDE, XFCE, and standalone panels (tint2, polybar) all publish `_NET_WM_STRUT_PARTIAL` for their reserved strips. A complete implementation should handle all four edges.

**V1 call:**  
**Pursue to `yes`.** This is fixable code that does not require new platform APIs or consent gates. The X11 protocol provides the needed information; the implementation just needs to consume it.

**Follow-up:**  
Open issue for "Support side and top panels as Perches on Linux" with acceptance criteria:
- Detect and use left-edge panels (`strut[0]`)
- Detect and use right-edge panels (`strut[1]`)
- Detect and use top-edge panels (`strut[2]`)
- Verify with GNOME, KDE, XFCE panels in each position

---

## Windows: Dock or panel as a Perch

**Current behavior:**  
The taskbar is treated as a full-width (or full-height for side taskbars) reserved strip. The Character cannot perch on the taskbar itself as a raised island.

**Root constraint:**  
Platform limitation. Windows provides `GetMonitorInfo` which returns a work area (the usable portion of the display), but not the exact taskbar rectangle the way macOS provides via `CoreDockGetRect`. The taskbar can be auto-hidden, moved to any edge, or resized, and Windows reports only the work area that excludes it.

From `src-tauri/src/platform/windows/window_source.rs:26`:
> Taskbar/dock bounds on Windows come from the work area Tauri already reads.

Tauri's `work_area()` gives the space outside the taskbar, not the taskbar's own bounds. The difference:
- macOS: `CoreDockGetRect` → exact Dock island (e.g., `(234, 988, 1452, 92)`)
- Windows: work area → full-width strip inference (e.g., `(0, 0, 1920, 988)` work area implies `(0, 988, 1920, 92)` taskbar strip)

**Options to complete:**

1. **Windows UI Automation API.** `consent.rs:104` mentions reading taskbar bounds via UI Automation:
   > Windows UI Automation. The fidget reads the taskbar's bounds; it does not control your computer.

   This could provide exact taskbar bounds, but:
   - Requires a new consent gate or trust grant
   - Adds complexity for a visual refinement
   - Still gives a strip, not an island (the taskbar spans the edge)

2. **Accept the strip.** The work-area strip is accurate; the taskbar does reserve that full span. macOS's island is special because the Dock floats; Windows' taskbar stretches to the edge.

**V1 call:**  
**Document as accepted degrade.** The work-area inference is correct for Windows' taskbar design. The "degraded" label is comparative to macOS's island, but Windows' taskbar is architecturally different (it spans the edge by design). The Character can perch on normal windows above the taskbar, which is the main use case.

**Follow-up:**  
Update README Platform Support table footnote to explain: "Windows taskbar is a full-edge strip (from work area) rather than an island; this is a design difference, not a missing feature."

---

## Windows: Fade out for a fullscreen app

**Current behavior:**  
The Character fades out for true fullscreen applications (exclusive fullscreen mode) but may not fade out reliably for borderless windowed fullscreen or maximized applications.

**Root constraint:**  
Windows application behavior and API limitations. Windows has multiple "fullscreen-like" modes:

1. **Exclusive fullscreen**: App takes over the display, hides taskbar, switches display mode. Fidget detects this correctly.
2. **Borderless windowed fullscreen**: App creates a borderless window sized to cover the display. Reports as a normal window with display-sized bounds. Fidget should detect this.
3. **Maximized**: App is maximized but stops at the taskbar. Correctly NOT treated as fullscreen (per `bench-gpu-compositing-windows.ps1:880`: "Maximized stops at the taskbar. Hide rules treat that as zoomed, not fullscreen").

The issue: borderless windowed mode is indistinguishable from a normal window that happens to cover the display. The `fullscreen_frontmost` function in `crates/core/src/visibility.rs` looks for windows that cover an entire display (within `EDGE_TOLERANCE`), which should catch borderless windowed fullscreen, but:

- Some apps leave a gap for the taskbar even in "fullscreen" mode
- Windows does not expose an "is exclusive fullscreen" property
- Game launchers and apps have inconsistent fullscreen implementations

From the bench script's approach (lines 890-896 in `bench-gpu-compositing-windows.ps1`), Fidget's test creates a borderless window covering `[System.Windows.Forms.Screen]::PrimaryScreen.Bounds` to simulate fullscreen, which should trigger the fade.

**Options to complete:**

1. **Accept the current behavior.** True fullscreen works; borderless windowed should work via geometry. Apps that don't properly cover the display won't trigger fade, which might be correct (they're not really fullscreen).

2. **Loosen the fullscreen detection threshold.** Reduce `EDGE_TOLERANCE` or allow windows that are "almost" display-sized to count as fullscreen. Risks false positives (zoomed windows being treated as fullscreen).

3. **Check for taskbar visibility.** If the taskbar is hidden (work area == full display), treat large windows as fullscreen. This might improve detection but adds complexity.

**V1 call:**  
**Document as accepted degrade.** The core fullscreen detection works (exclusive fullscreen + properly-sized borderless windowed). The "degraded" label acknowledges that Windows' mixed fullscreen modes mean detection is less reliable than macOS (where fullscreen has clearer platform support). Apps that don't properly cover the display won't trigger fade, which is defensible behavior.

**Follow-up:**  
Update README or add a note: "Windows fullscreen fade works for exclusive fullscreen and borderless windowed modes; apps that leave gaps or use non-standard fullscreen may not trigger fade."

---

## Linux: Capturable; opt-out in settings

**Current behavior:**  
The Character always appears in screenshots and screen recordings. The Settings → Presence → "Appear in screenshots and screen shares" checkbox is ignored on Linux.

**Root constraint:**  
Platform limitation, already documented. From ADR-0024:

> 3. **Platforms that can exclude honour it.** macOS and Windows read the setting. Linux has no exclusion API and stays capturable.

Linux has no standard API to exclude a window from screen capture tools. Wayland compositors and X11 capture tools (OBS, `scrot`, `gnome-screenshot`, etc.) have no per-window capture exclusion flag.

**Options to complete:**

None. This is a permanent platform limitation unless Linux adopts a standard capture-exclusion protocol, which is not on any compositor's roadmap.

**V1 call:**  
**Already documented** as accepted limit in ADR-0024. No action needed beyond linking this write-up from the README.

**Follow-up:**  
Update README Platform Support table to link to ADR-0024 or this research doc for the explanation.

---

## Wayland: Fullscreen fade (native Wayland without XWayland)

**Current behavior:**  
On a pure Wayland session without XWayland, the Character does not move to a free display or fade when a native Wayland app goes fullscreen. The fullscreen rule behaves as if no fullscreen app is present.

**Root constraint:**  
Wayland protocol design. Wayland gives clients no global window list by design. On pure Wayland sessions, Fidget uses `DisplayOnlySource` (`src-tauri/src/platform.rs:949-958`), which reports displays but no windows. This is the documented degraded mode.

From `src-tauri/src/platform.rs:1019-1039`:
> A Wayland session with no XWayland stays DisplayOnlySource: no global window list.

The fullscreen detection function `fullscreen_displays` (`crates/core/src/visibility.rs:180-193`) requires a list of window rectangles. With an empty windows list, it returns all `false`, so the Character never moves to a free display and never fades.

**Options to complete:**

1. **Compositor-specific protocols.** Each major compositor has its own mechanism:
   - `wlr-foreign-toplevel-management` on wlroots compositors (Sway, Hyprland, river, niri)
   - GNOME Shell extension or `org.gnome.Shell.Introspect` D-Bus (allowlisted senders only)
   - KWin scripting

   This would require compositor-specific code paths and ongoing maintenance across multiple compositor implementations. Each compositor's API is different and not guaranteed stable.

2. **xdg-desktop-portal inhibit/idle as proxy.** Apps can request inhibit (don't sleep/lock) via the portal, which correlates with fullscreen but is not the same signal. Unreliable as a fullscreen indicator (media players, presentations, and games all inhibit idle without being fullscreen).

3. **Accept the degraded mode.** XWayland is the canonical Linux path. Pure Wayland without XWayland is explicitly a supported degraded mode (DESIGN.md decision 3, docs/research/wayland-protocols.md). The README already documents: "Rare pure Wayland sessions without an X server fall back to screen edges only."

**V1 call:**  
**Document as accepted degrade.** This is consistent with the project's existing Wayland stance. XWayland serves the fullscreen rule correctly: it lists X clients, and Fidget's fullscreen detection works there. Pure Wayland is the rare degraded path. The gap applies only to native Wayland fullscreen apps (Firefox Wayland, GNOME apps, etc.); X clients under XWayland still trigger fullscreen fade correctly.

Compositor-specific detection would sprawl across three different compositor families, each with its own maintenance burden and stability risks. Wayland's design decision to withhold global window state is architectural, not a temporary gap.

**Follow-up:**  
Update README Platform Support table to note that Linux fullscreen fade is "degraded" rather than "yes", with a footnote: "X11 and XWayland: works for all apps. Pure Wayland without XWayland: native Wayland fullscreen apps do not trigger fade or move. XWayland clients still trigger correctly."

Add entry to the table summary documenting the gap for pure Wayland sessions.

---

## V1 Disposition Summary

- **1 cell → `yes`**: Linux Dock/panel Perch (fixable implementation gap)
- **3 cells → accepted degrade**: Windows Dock/panel Perch, Windows fullscreen fade, Wayland fullscreen fade (platform differences)
- **1 cell → already documented**: Linux Capturable (ADR-0024)

Next step: Open follow-up issue for Linux panel detection, update README with footnotes/links for the accepted degrades.
