//! Windows overlay configuration: non-activating, topmost, click-through.
//!
//! Extended window styles (WS_EX_NOACTIVATE, WS_EX_TOPMOST, WS_EX_TOOLWINDOW,
//! WS_EX_TRANSPARENT) float the overlay above other windows without stealing
//! focus. SetWindowRgn carves the input region from the sprite's alpha mask
//! and unions the rectangles the renderer reported, so a control drawn outside
//! the art still receives clicks and a bubble outside it is not clipped.
//! WDA_EXCLUDEFROMCAPTURE applies only when the capturable setting, on by
//! default (ADR-0024), is turned off.
//!
//! DwmExtendFrameIntoClientArea extends the window frame into the entire client
//! area, compositing the frame with the client area's glass sheet so the overlay
//! has no opaque window background.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::Instant;

use fidget_core::overlay_region::{
    placed_shape, rect_bounds, region_plan, OverlayRect, RegionPlan, RegionRect, RegionShapes,
    SpriteArt,
};
use fidget_core::roster::InstanceId;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::Graphics::Dwm::DwmExtendFrameIntoClientArea;
use windows_sys::Win32::Graphics::Gdi::{
    CombineRgn, CreateRectRgn, DeleteObject, OffsetRgn, SetWindowRgn, HRGN, RGN_ERROR, RGN_OR,
};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Controls::MARGINS;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetWindowLongW, SetWindowDisplayAffinity, SetWindowLongW, SetWindowPos, GWL_EXSTYLE,
    HWND_TOPMOST, SWP_ASYNCWINDOWPOS, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOREDRAW,
    SWP_NOSIZE, SWP_NOZORDER, WDA_EXCLUDEFROMCAPTURE, WS_EX_TRANSPARENT,
};

/// Float above other windows, non-activating, with no opaque background, and
/// capturable unless Presence or `FIDGET_CAPTURABLE=0` excludes it. Returns Err
/// when the handle is not realized yet, so the caller can retry.
pub fn configure_overlay(window: &tauri::WebviewWindow) -> Result<(), String> {
    let hwnd = overlay_hwnd(window)?;
    set_window_styles(hwnd)?;
    set_window_topmost(hwnd)?;
    apply_capture_exclusion(hwnd)?;
    extend_dwm_frame(hwnd)?;
    note_overlay(hwnd as u64);

    Ok(())
}

/// Reinforce overlay styles after a rewrite. `update_input_region` owns
/// WS_EX_TRANSPARENT; this re-applies tool-window bits via `reinforce_overlay`.
pub fn set_click_through(window: &tauri::WebviewWindow) -> Result<(), String> {
    if event_loop_thread() {
        return apply_click_through(window);
    }
    let window = window.clone();
    window
        .clone()
        .run_on_main_thread(move || {
            if let Err(why) = apply_click_through(&window) {
                eprintln!("overlay: click-through restore failed: {why}");
            }
        })
        .map_err(|e| e.to_string())
}

fn apply_click_through(window: &tauri::WebviewWindow) -> Result<(), String> {
    reinforce_overlay(window)
}

/// Toggle WS_EX_TRANSPARENT without reapplying the region. Used when only
/// click-through state changes on an idle sprite (mask unchanged, ignore flipped).
pub fn toggle_click_through_only(
    window: &tauri::WebviewWindow,
    click_through: bool,
) -> Result<(), String> {
    let hwnd = overlay_hwnd(window)?;
    unsafe {
        let current_style = GetWindowLongW(hwnd, GWL_EXSTYLE);
        let new_style = if click_through {
            current_style | (WS_EX_TRANSPARENT as i32)
        } else {
            current_style & !(WS_EX_TRANSPARENT as i32)
        };
        apply_exstyle(hwnd, current_style, new_style)?;
    }
    Ok(())
}

/// Setup builds overlays on the event-loop thread, before the frame loop.
fn event_loop_thread() -> bool {
    static EVENT_LOOP: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    // SAFETY: GetCurrentThreadId only reads the calling thread.
    let current = unsafe { GetCurrentThreadId() };
    *EVENT_LOOP.get_or_init(|| current) == current
}

/// Overlay hwnds. The window-list poll runs off this thread and has no other
/// handle for the windows built here.
static OVERLAY_HWNDS: Mutex<Vec<u64>> = Mutex::new(Vec::new());

pub(super) fn overlay_hwnds() -> Vec<u64> {
    OVERLAY_HWNDS
        .lock()
        .map(|hwnds| hwnds.clone())
        .unwrap_or_default()
}

fn note_overlay(hwnd: u64) {
    if hwnd == 0 {
        return;
    }
    let Ok(mut hwnds) = OVERLAY_HWNDS.lock() else {
        return;
    };
    if !hwnds.contains(&hwnd) {
        hwnds.push(hwnd);
    }
}

/// A region this module built and still owns. SetWindowRgn gets a union
/// that holds it, never the region itself.
#[derive(Clone, Copy)]
struct KeptRgn(HRGN);

// SAFETY: a region handle is a GDI object usable from any thread. A kept
// region is used only under REGION_SHAPES' lock, and freed once removed.
unsafe impl Send for KeptRgn {}

/// Each Character's recently built regions at the origin, by overlay label.
type OverlayShapes = HashMap<InstanceId, RegionShapes<KeptRgn>>;

static REGION_SHAPES: LazyLock<Mutex<HashMap<String, OverlayShapes>>> =
    LazyLock::new(Default::default);

fn free_shapes(shapes: &mut RegionShapes<KeptRgn>) {
    for KeptRgn(rgn) in shapes.take_all() {
        // SAFETY: the region was built here and nothing else holds it.
        unsafe { DeleteObject(rgn) };
    }
}

fn free_kept_regions(label: &str) {
    let removed = REGION_SHAPES.lock().unwrap().remove(label);
    for mut shapes in removed.into_iter().flat_map(HashMap::into_values) {
        free_shapes(&mut shapes);
    }
}

/// Free the regions kept for a destroyed overlay.
pub fn cleanup_overlay_window(window: &tauri::WebviewWindow) {
    free_kept_regions(window.label());
}

/// SetWindowRgn from `art` plus the renderer's rects; `None` clears the region.
/// `click_through` sets WS_EX_TRANSPARENT; the region stays either way. Returns
/// whether the window now passes clicks, which a cleared region forces.
pub fn update_input_region(
    window: &tauri::WebviewWindow,
    art: Option<&[SpriteArt]>,
    rects: &[OverlayRect],
    click_through: bool,
) -> Result<bool, String> {
    let hwnd = overlay_hwnd(window)?;
    match art {
        Some(art) => apply_input_mask(window, hwnd, art, rects, click_through),
        None => clear_input_region(window.label(), hwnd).map(|()| true),
    }
}

fn set_window_styles(hwnd: HWND) -> Result<(), String> {
    // SAFETY: hwnd is a live HWND from Tauri.
    unsafe {
        let current_style = GetWindowLongW(hwnd, GWL_EXSTYLE);
        let new_style = super::super::windows_perch::restore_overlay_exstyle(current_style)
            | (WS_EX_TRANSPARENT as i32);
        apply_exstyle(hwnd, current_style, new_style)?;
    }
    Ok(())
}

/// Log a debug message if FIDGET_DEBUG_REINFORCE is set.
fn reinforce_debug_log(msg: impl FnOnce() -> String) {
    if crate::dev_flags::DEBUG_REINFORCE.is_on() {
        if let Ok(now) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
            eprintln!(
                "[{}.{:03}] reinforce_overlay {}",
                now.as_secs(),
                now.subsec_millis(),
                msg()
            );
        }
    }
}

/// Put the tool-window bits back after a click-through rewrite drops them.
fn reinforce_overlay(window: &tauri::WebviewWindow) -> Result<(), String> {
    let hwnd = overlay_hwnd(window)?;

    reinforce_debug_log(|| format!("start hwnd={:x}", hwnd as usize));

    if let Err(e) = extend_dwm_frame(hwnd) {
        eprintln!("overlay: extend_dwm_frame failed: {e}");
    }

    // SAFETY: hwnd comes from the window's raw handle, valid for this call.
    unsafe {
        let current_style = GetWindowLongW(hwnd, GWL_EXSTYLE);
        let new_style = super::super::windows_perch::restore_overlay_exstyle(current_style);

        reinforce_debug_log(|| {
            format!(
                "ex-style write: current={:#x} new={:#x}",
                current_style, new_style
            )
        });

        apply_exstyle(hwnd, current_style, new_style)?;
    }

    note_overlay(hwnd as u64);

    reinforce_debug_log(|| format!("end hwnd={:x}", hwnd as usize));

    Ok(())
}

fn overlay_hwnd(window: &tauri::WebviewWindow) -> Result<HWND, String> {
    let raw_window_handle = window
        .window_handle()
        .map_err(|e| format!("Window handle not available yet: {e}"))?;
    match raw_window_handle.as_raw() {
        RawWindowHandle::Win32(win32_window) => Ok(win32_window.hwnd.get() as HWND),
        _ => Err("Not a Windows window handle".to_string()),
    }
}

/// SAFETY: caller holds a live HWND. SetWindowLongW returns 0 both on error
/// and when the previous value was 0.
unsafe fn apply_exstyle(hwnd: HWND, current_style: i32, new_style: i32) -> Result<(), String> {
    if new_style == current_style {
        return Ok(());
    }
    if SetWindowLongW(hwnd, GWL_EXSTYLE, new_style) == 0 && current_style != 0 {
        return Err("Failed to set extended window styles".to_string());
    }
    // FRAMECHANGED is what makes the extended-style write take effect.
    // ASYNCWINDOWPOS prevents deadlock when called during a modal drag loop.
    if SetWindowPos(
        hwnd,
        HWND_TOPMOST,
        0,
        0,
        0,
        0,
        SWP_NOMOVE
            | SWP_NOSIZE
            | SWP_NOACTIVATE
            | SWP_FRAMECHANGED
            | SWP_NOREDRAW
            | SWP_ASYNCWINDOWPOS,
    ) == 0
    {
        return Err("Failed to apply overlay extended styles".to_string());
    }
    Ok(())
}

fn set_window_topmost(hwnd: HWND) -> Result<(), String> {
    // SAFETY: hwnd is a valid HWND from Tauri's raw window handle. SetWindowPos
    // is documented safe with valid HWNDs and standard z-order/positioning flags.
    unsafe {
        if SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER,
        ) == 0
        {
            return Err("Failed to set window topmost".to_string());
        }
    }
    Ok(())
}

/// Apply capture policy from user settings. The Presence "Appear in
/// screenshots and screen shares" checkbox binds directly: checked (the
/// default) is `capturable = true`, which clears WDA_EXCLUDEFROMCAPTURE.
fn apply_capture_exclusion(hwnd: HWND) -> Result<(), String> {
    // SAFETY: hwnd is a valid HWND from Tauri's raw window handle.
    // SetWindowDisplayAffinity is documented safe with valid HWNDs.
    let affinity = if crate::dev_flags::CAPTURABLE.is_on() {
        0 // WDA_NONE - window is capturable
    } else {
        WDA_EXCLUDEFROMCAPTURE
    };

    unsafe {
        if SetWindowDisplayAffinity(hwnd, affinity) == 0 {
            return Err("Failed to set window display affinity".to_string());
        }
    }
    Ok(())
}

fn extend_dwm_frame(hwnd: HWND) -> Result<(), String> {
    let margins = MARGINS {
        cxLeftWidth: -1,
        cxRightWidth: -1,
        cyTopHeight: -1,
        cyBottomHeight: -1,
    };

    // SAFETY: hwnd is a valid HWND from Tauri's raw window handle.
    // DwmExtendFrameIntoClientArea is documented safe with valid HWNDs and margins.
    unsafe {
        let result = DwmExtendFrameIntoClientArea(hwnd, &margins);
        if result != 0 {
            return Err(format!("Failed to extend DWM frame: {result:#x}"));
        }
    }

    Ok(())
}

/// `art` is `[left, top, right, bottom]` from `AlphaMask::swept_rects`, swept
/// over every draw position with drawn-only rects in it, since the region also
/// clips drawing. bRedraw=1: with 0 it races an earlier SetWindowPos.
fn apply_input_mask(
    window: &tauri::WebviewWindow,
    hwnd: HWND,
    art: &[SpriteArt],
    overlay_rects: &[OverlayRect],
    click_through: bool,
) -> Result<bool, String> {
    let rebuild_start = Instant::now();
    let art_rects = || art.iter().flat_map(|sprite| &sprite.rects);
    if region_plan(art_rects(), overlay_rects) == RegionPlan::Clear {
        return clear_input_region(window.label(), hwnd).map(|()| true);
    }
    let bounds: Vec<RegionRect> = rect_bounds(overlay_rects).collect();
    let (window_rgn, work) = window_region(window.label(), art, &bounds)?;

    // SAFETY: hwnd is valid and window_rgn is a region nothing else holds.
    // SetWindowRgn takes ownership of it on success, so it is freed only on
    // the error paths.
    unsafe {
        let current_style = GetWindowLongW(hwnd, GWL_EXSTYLE);
        let new_style = if click_through {
            current_style | (WS_EX_TRANSPARENT as i32)
        } else {
            current_style & !(WS_EX_TRANSPARENT as i32)
        };
        if let Err(e) = apply_exstyle(hwnd, current_style, new_style) {
            DeleteObject(window_rgn);
            return Err(e);
        }

        if SetWindowRgn(hwnd, window_rgn, 1) == 0 {
            DeleteObject(window_rgn);
            return Err("Failed to set window region".to_string());
        }
    }

    let trace_bubble = std::env::var("FIDGET_TRACE_BUBBLE").is_ok();
    let trace_mask = std::env::var("FIDGET_TRACE_MASK_REBUILD").is_ok();

    if trace_bubble || trace_mask {
        let clickable = overlay_rects.iter().filter(|rect| rect.clickable).count();
        let art_count = art_rects().count();
        eprintln!(
            "overlay {}: region {} {} rects (art {} + hotspots {} + painted {}), {:.2} ms",
            window.label(),
            match work {
                RegionWork::Rebuilt => "rebuild",
                RegionWork::Shifted => "shift",
            },
            art_count + bounds.len(),
            art_count,
            clickable,
            overlay_rects.len() - clickable,
            rebuild_start.elapsed().as_secs_f64() * 1000.0
        );
    }

    Ok(click_through)
}

/// Whether any Character's art had to be built from its rects this time.
#[derive(Debug, PartialEq, Eq)]
enum RegionWork {
    Rebuilt,
    Shifted,
}

/// A region for SetWindowRgn holding every Character's art and `bounds`.
/// A Character whose shape is kept has that region moved into place instead
/// of rebuilt. Characters no longer on this overlay have theirs freed.
fn window_region(
    label: &str,
    art: &[SpriteArt],
    bounds: &[RegionRect],
) -> Result<(HRGN, RegionWork), String> {
    let mut all = REGION_SHAPES.lock().unwrap();
    let overlay = all.entry(label.to_string()).or_default();
    overlay.retain(|instance, shapes| {
        let here = art.iter().any(|sprite| &sprite.instance == instance);
        if !here {
            free_shapes(shapes);
        }
        here
    });

    let window = build_region(bounds)?;
    let mut work = RegionWork::Shifted;
    for sprite in art.iter().filter(|sprite| !sprite.rects.is_empty()) {
        let placed = placed_shape(&sprite.rects);
        let shapes = overlay.entry(sprite.instance.clone()).or_default();
        let kept = match shapes.get(&placed.shape) {
            Some(KeptRgn(kept)) => kept,
            None => {
                let built = build_region(&placed.shape).inspect_err(|_| {
                    // SAFETY: window was built above and is held only here.
                    unsafe { DeleteObject(window) };
                })?;
                if let Some(KeptRgn(oldest)) = shapes.keep(placed.shape, KeptRgn(built)) {
                    // SAFETY: an evicted region is no longer kept or held anywhere.
                    unsafe { DeleteObject(oldest) };
                }
                work = RegionWork::Rebuilt;
                built
            }
        };
        add_at(window, kept, placed.at).inspect_err(|_| {
            // SAFETY: window was built above and is held only here.
            unsafe { DeleteObject(window) };
        })?;
    }
    if art.iter().all(|sprite| sprite.rects.is_empty()) {
        work = RegionWork::Rebuilt;
    }
    Ok((window, work))
}

/// Union `kept`, moved to `(x, y)`, into `window`. `kept` is moved back after,
/// so it stays at the origin.
fn add_at(window: HRGN, kept: HRGN, (x, y): (i32, i32)) -> Result<(), String> {
    // SAFETY: both are live regions; the caller holds REGION_SHAPES' lock,
    // so nothing else touches kept while it is away from the origin.
    unsafe {
        if OffsetRgn(kept, x, y) == RGN_ERROR {
            return Err("Failed to move kept region".to_string());
        }
        let combined = CombineRgn(window, window, kept, RGN_OR);
        OffsetRgn(kept, -x, -y);
        if combined == RGN_ERROR {
            return Err("Failed to combine regions".to_string());
        }
    }
    Ok(())
}

/// The union of `rects`, one CombineRgn per rect.
fn build_region(rects: &[RegionRect]) -> Result<HRGN, String> {
    // SAFETY: region handles are checked for null and freed on every error path.
    unsafe {
        let combined_rgn = CreateRectRgn(0, 0, 0, 0);
        if combined_rgn.is_null() {
            return Err("Failed to create region".to_string());
        }
        for [left, top, right, bottom] in rects.iter().copied() {
            let rect_rgn = CreateRectRgn(left, top, right, bottom);
            if rect_rgn.is_null() {
                DeleteObject(combined_rgn);
                return Err("Failed to create region rectangle".to_string());
            }
            let combined = CombineRgn(combined_rgn, combined_rgn, rect_rgn, RGN_OR);
            DeleteObject(rect_rgn);
            if combined == RGN_ERROR {
                DeleteObject(combined_rgn);
                return Err("Failed to combine regions".to_string());
            }
        }
        Ok(combined_rgn)
    }
}

/// Clear the input region, making the entire window click-through, and free
/// the regions kept for it.
fn clear_input_region(label: &str, hwnd: HWND) -> Result<(), String> {
    free_kept_regions(label);
    // SAFETY: hwnd is a live HWND. GetWindowLongW and SetWindowRgn are safe
    // with a valid hwnd; apply_exstyle validates the same hwnd.
    unsafe {
        let current_style = GetWindowLongW(hwnd, GWL_EXSTYLE);
        let new_style = current_style | (WS_EX_TRANSPARENT as i32);
        apply_exstyle(hwnd, current_style, new_style)?;

        SetWindowRgn(hwnd, std::ptr::null_mut(), 1);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::Graphics::Gdi::PtInRegion;

    fn art(instance: &str, rects: &[RegionRect]) -> SpriteArt {
        SpriteArt {
            instance: instance.to_string(),
            rects: rects.to_vec(),
        }
    }

    fn holds(rgn: HRGN, x: i32, y: i32) -> bool {
        // SAFETY: rgn is a live region returned by window_region.
        unsafe { PtInRegion(rgn, x, y) != 0 }
    }

    /// Two Characters on one display: BMO walks while Tux stands. BMO's steps
    /// shift its kept region, and BMO changing frame never evicts Tux's.
    #[test]
    fn characters_sharing_an_overlay_keep_their_own_regions() {
        let label = "test-shared-overlay";
        let tux = art("tux", &[[500, 100, 540, 160]]);
        let bmo_at = |x: i32| art("bmo", &[[x, 100, x + 30, 120], [x + 5, 120, x + 25, 140]]);
        let mut placed = Vec::new();
        let mut place = |sprites: &[SpriteArt]| {
            let (rgn, work) = window_region(label, sprites, &[]).unwrap();
            placed.push(rgn);
            (rgn, work)
        };

        assert_eq!(place(&[bmo_at(0), tux.clone()]).1, RegionWork::Rebuilt);
        let (stepped, work) = place(&[bmo_at(3), tux.clone()]);
        assert_eq!(work, RegionWork::Shifted);
        assert!(holds(stepped, 32, 110) && !holds(stepped, 1, 110));
        assert!(holds(stepped, 510, 150));

        let next_frame = art("bmo", &[[3, 100, 40, 120]]);
        assert_eq!(place(&[next_frame, tux.clone()]).1, RegionWork::Rebuilt);
        let tux_moved = art("tux", &[[520, 100, 560, 160]]);
        let (rgn, work) = place(&[bmo_at(6), tux_moved]);
        assert_eq!(work, RegionWork::Shifted);
        assert!(holds(rgn, 555, 150) && !holds(rgn, 505, 150));

        free_kept_regions(label);
        for rgn in placed {
            // SAFETY: each region came from window_region and is held only here.
            unsafe { DeleteObject(rgn) };
        }
    }
}
