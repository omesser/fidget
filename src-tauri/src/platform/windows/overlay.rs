//! Windows overlay configuration: non-activating, topmost, click-through.
//!
//! Extended window styles (WS_EX_NOACTIVATE, WS_EX_TOPMOST, WS_EX_TOOLWINDOW,
//! WS_EX_TRANSPARENT) float the overlay above other windows without stealing
//! focus. SetWindowRgn carves the input region from the sprite's alpha mask
//! and unions any hotspot rectangles the renderer reported so a control drawn
//! outside the art still receives clicks. WDA_EXCLUDEFROMCAPTURE applies only
//! when the capturable setting, on by default (ADR-0024), is turned off.
//!
//! DwmExtendFrameIntoClientArea extends the window frame into the entire client
//! area, compositing the frame with the client area's glass sheet so the overlay
//! has no opaque window background (#1327).

use std::sync::Mutex;
use std::time::Instant;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::Graphics::Dwm::DwmExtendFrameIntoClientArea;
use windows_sys::Win32::Graphics::Gdi::{
    CombineRgn, CreateRectRgn, DeleteObject, SetWindowRgn, RGN_OR,
};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Controls::MARGINS;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetWindowLongW, SetWindowDisplayAffinity, SetWindowLongW, SetWindowPos, GWL_EXSTYLE,
    HWND_TOPMOST, SWP_ASYNCWINDOWPOS, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOREDRAW,
    SWP_NOSIZE, SWP_NOZORDER, WDA_EXCLUDEFROMCAPTURE, WS_EX_TRANSPARENT,
};

/// Float above other windows, non-activating. Capturable unless Presence or
/// `FIDGET_CAPTURABLE=0` excludes it from shares. Extends DWM frame into the
/// entire client area to composite the frame with the client area's glass sheet.
/// Returns Err when the handle is not realized yet, so the caller can retry.
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
/// `_ignore` unused; kept for cross-platform signature parity.
pub fn set_click_through(window: &tauri::WebviewWindow, _ignore: bool) -> Result<(), String> {
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

/// SetWindowRgn from `art` plus hotspots plus painted rects; `None` clears the region.
/// `click_through` sets WS_EX_TRANSPARENT; the region stays either way.
pub fn update_input_region(
    window: &tauri::WebviewWindow,
    art: Option<&[[i32; 4]]>,
    hotspot_rects: &[[i32; 4]],
    painted_rects: &[[i32; 4]],
    click_through: bool,
) -> Result<(), String> {
    let hwnd = overlay_hwnd(window)?;
    match art {
        Some(art) => apply_input_mask(
            window,
            hwnd,
            art,
            hotspot_rects,
            painted_rects,
            click_through,
        ),
        None => clear_input_region(hwnd),
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

/// `art` is `[left, top, right, bottom]` from `AlphaMask::swept_rects`; hotspots
/// and painted are `[x, y, width, height]`. The region also clips drawing, which
/// is why `art` is swept over every position the renderer may draw the sprite at,
/// and painted rects (bubble, thinking) must be included so Windows doesn't clip them.
///
/// bRedraw=1: With bRedraw=0, the region update could race sprite placement
/// from an earlier SetWindowPos, leaving the wrong region visible until the
/// next frame forced a redraw.
fn apply_input_mask(
    window: &tauri::WebviewWindow,
    hwnd: HWND,
    art: &[[i32; 4]],
    hotspot_rects: &[[i32; 4]],
    painted_rects: &[[i32; 4]],
    click_through: bool,
) -> Result<(), String> {
    if art.is_empty() {
        return clear_input_region(hwnd);
    }
    let rebuild_start = Instant::now();

    let rects =
        fidget_core::overlay_region::overlay_region_rects(art, hotspot_rects, painted_rects);

    // SAFETY: hwnd is valid. Region handles are checked for null and freed on
    // every error path. SetWindowRgn takes ownership of combined_rgn on
    // success, so it is not freed afterward.
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
            if combined == 0 {
                DeleteObject(combined_rgn);
                return Err("Failed to combine regions".to_string());
            }
        }

        let current_style = GetWindowLongW(hwnd, GWL_EXSTYLE);
        let new_style = if click_through {
            current_style | (WS_EX_TRANSPARENT as i32)
        } else {
            current_style & !(WS_EX_TRANSPARENT as i32)
        };
        if let Err(e) = apply_exstyle(hwnd, current_style, new_style) {
            DeleteObject(combined_rgn);
            return Err(e);
        }

        if SetWindowRgn(hwnd, combined_rgn, 1) == 0 {
            DeleteObject(combined_rgn);
            return Err("Failed to set window region".to_string());
        }
    }

    let trace_bubble = std::env::var("FIDGET_TRACE_BUBBLE").is_ok();
    let trace_mask = std::env::var("FIDGET_TRACE_MASK_REBUILD").is_ok();

    if trace_bubble || trace_mask {
        eprintln!(
            "overlay {}: region rebuild {} rects (art {} + hotspots {} + painted {}), {:.2} ms",
            window.label(),
            rects.len(),
            art.len(),
            hotspot_rects.len(),
            painted_rects.len(),
            rebuild_start.elapsed().as_secs_f64() * 1000.0
        );
    }

    Ok(())
}

/// Clear the input region, making the entire window click-through.
fn clear_input_region(hwnd: HWND) -> Result<(), String> {
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
