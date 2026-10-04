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
//! area, making DWM composite the window with full transparency. This prevents
//! the window frame's white background from showing during drag operations and
//! monitor transitions (#1327).

use std::sync::Mutex;
use std::time::Instant;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::Graphics::Dwm::DwmExtendFrameIntoClientArea;
use windows_sys::Win32::Graphics::Gdi::{CreateRectRgn, DeleteObject, SetWindowRgn, HRGN, RGN_OR};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Controls::MARGINS;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetWindowLongW, SetWindowDisplayAffinity, SetWindowLongW, SetWindowPos, GWL_EXSTYLE,
    HWND_TOPMOST, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
    WDA_EXCLUDEFROMCAPTURE, WS_EX_TRANSPARENT,
};

/// Float above other windows, non-activating. Capturable unless Presence or
/// `FIDGET_CAPTURABLE=0` excludes it from shares. Extends DWM frame into the
/// entire client area to prevent white flashes during window moves and monitor
/// transitions. Returns Err when the handle is not realized yet, so the caller
/// can retry.
pub fn configure_overlay(window: &tauri::WebviewWindow) -> Result<(), String> {
    let hwnd = overlay_hwnd(window)?;
    set_window_styles(hwnd)?;
    set_window_topmost(hwnd)?;
    apply_capture_exclusion(hwnd)?;
    extend_dwm_frame(hwnd)?;
    note_overlay(hwnd as u64);

    Ok(())
}

/// Click-through, then put the tool-window bits back.
/// Off the event-loop thread tao posts the style rewrite and returns first,
/// so both steps run there, rewrite first.
pub fn set_click_through(window: &tauri::WebviewWindow, ignore: bool) -> Result<(), String> {
    if event_loop_thread() {
        return apply_click_through(window, ignore);
    }
    let window = window.clone();
    window
        .clone()
        .run_on_main_thread(move || {
            if let Err(why) = apply_click_through(&window, ignore) {
                eprintln!("overlay: click-through restore failed: {why}");
            }
        })
        .map_err(|e| e.to_string())
}

fn apply_click_through(window: &tauri::WebviewWindow, ignore: bool) -> Result<(), String> {
    window
        .set_ignore_cursor_events(ignore)
        .map_err(|e| e.to_string())?;
    reinforce_overlay(window)
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

/// SetWindowRgn carves the click-through region from the sprite's alpha mask.
/// `None` is WS_EX_TRANSPARENT (whole window). `Some` is the opaque pixels
/// plus hotspots, with WS_EX_TRANSPARENT removed so clicks hit that region.
pub fn update_input_region(
    window: &tauri::WebviewWindow,
    mask_data: Option<&fidget_core::overlay::AlphaMask>,
    sprite_x: i32,
    sprite_y: i32,
    sprite_facing: i32,
    scale: i32,
    hotspot_rects: &[[i32; 4]],
) -> Result<(), String> {
    let hwnd = overlay_hwnd(window)?;

    if let Some(mask) = mask_data {
        apply_input_mask(
            hwnd,
            mask,
            sprite_x,
            sprite_y,
            sprite_facing,
            scale,
            hotspot_rects,
        )?;
    } else {
        clear_input_region(hwnd)?;
    }

    Ok(())
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
fn debug_reinforce(msg: impl FnOnce() -> String) {
    static DEBUG_ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let enabled = *DEBUG_ENABLED.get_or_init(|| std::env::var("FIDGET_DEBUG_REINFORCE").is_ok());

    if enabled {
        if let Ok(now) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
            eprintln!("[{}.{:03}] {}", now.as_secs(), now.subsec_millis(), msg());
        }
    }
}

/// Put the tool-window bits back after a click-through rewrite drops them.
fn reinforce_overlay(window: &tauri::WebviewWindow) -> Result<(), String> {
    let hwnd = overlay_hwnd(window)?;
    debug_reinforce(|| format!("reinforce_overlay start hwnd={:x}", hwnd as usize));

    extend_dwm_frame(hwnd)?;

    // SAFETY: hwnd comes from the window's raw handle, valid for this call.
    unsafe {
        let current_style = GetWindowLongW(hwnd, GWL_EXSTYLE);
        let new_style = super::super::windows_perch::restore_overlay_exstyle(current_style);
        debug_reinforce(|| {
            format!(
                "reinforce_overlay ex-style write: current={:#x} new={:#x}",
                current_style, new_style
            )
        });
        apply_exstyle(hwnd, current_style, new_style)?;
    }

    note_overlay(hwnd as u64);
    debug_reinforce(|| format!("reinforce_overlay end hwnd={:x}", hwnd as usize));

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
    if SetWindowPos(
        hwnd,
        HWND_TOPMOST,
        0,
        0,
        0,
        0,
        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED,
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

    unsafe {
        let result = DwmExtendFrameIntoClientArea(hwnd, &margins);
        if result != 0 {
            return Err(format!("Failed to extend DWM frame: {result:#x}"));
        }
    }

    Ok(())
}

/// Apply the alpha mask as the input region using SetWindowRgn.
/// Facing < 0 mirrors the mask. Hotspot rectangles are OR'd in so a control
/// drawn outside the art still receives clicks.
#[allow(clippy::too_many_arguments)]
fn apply_input_mask(
    hwnd: HWND,
    mask: &fidget_core::overlay::AlphaMask,
    sprite_x: i32,
    sprite_y: i32,
    sprite_facing: i32,
    scale: i32,
    hotspot_rects: &[[i32; 4]],
) -> Result<(), String> {
    let rebuild_start = Instant::now();

    let (width, height, opaque) = mask.raw();
    let scaled_width = width * scale;
    let scaled_height = height * scale;
    let opaque_count = opaque.iter().filter(|&&b| b).count();

    // SAFETY: hwnd is valid. Region handles are checked for null and freed on
    // every error path. SetWindowRgn takes ownership of combined_rgn on
    // success, so it is not freed afterward.
    unsafe {
        let mut combined_rgn: HRGN = std::ptr::null_mut();

        let mirror = sprite_facing < 0;
        for y in 0..height {
            for x in 0..width {
                let idx = (y * width + x) as usize;
                if opaque[idx] {
                    let draw_x = if mirror {
                        (width - 1 - x) * scale
                    } else {
                        x * scale
                    };
                    let scaled_y = y * scale;

                    let rect_rgn = CreateRectRgn(
                        sprite_x + draw_x,
                        sprite_y + scaled_y,
                        sprite_x + draw_x + scale,
                        sprite_y + scaled_y + scale,
                    );

                    if rect_rgn.is_null() {
                        if !combined_rgn.is_null() {
                            DeleteObject(combined_rgn);
                        }
                        return Err("Failed to create region rectangle".to_string());
                    }

                    if combined_rgn.is_null() {
                        combined_rgn = rect_rgn;
                    } else {
                        let temp_rgn = CreateRectRgn(0, 0, 0, 0);
                        if temp_rgn.is_null() {
                            DeleteObject(combined_rgn);
                            DeleteObject(rect_rgn);
                            return Err("Failed to create temp region".to_string());
                        }

                        if windows_sys::Win32::Graphics::Gdi::CombineRgn(
                            temp_rgn,
                            combined_rgn,
                            rect_rgn,
                            RGN_OR,
                        ) == 0
                        {
                            DeleteObject(combined_rgn);
                            DeleteObject(rect_rgn);
                            DeleteObject(temp_rgn);
                            return Err("Failed to combine regions".to_string());
                        }

                        DeleteObject(combined_rgn);
                        DeleteObject(rect_rgn);
                        combined_rgn = temp_rgn;
                    }
                }
            }
        }

        if combined_rgn.is_null() {
            return clear_input_region(hwnd);
        }

        for &[hx, hy, hw, hh] in hotspot_rects {
            let hotspot_rgn = CreateRectRgn(hx, hy, hx + hw, hy + hh);
            if hotspot_rgn.is_null() {
                DeleteObject(combined_rgn);
                return Err("Failed to create hotspot region".to_string());
            }

            let temp_rgn = CreateRectRgn(0, 0, 0, 0);
            if temp_rgn.is_null() {
                DeleteObject(combined_rgn);
                DeleteObject(hotspot_rgn);
                return Err("Failed to create temp region for hotspot".to_string());
            }

            if windows_sys::Win32::Graphics::Gdi::CombineRgn(
                temp_rgn,
                combined_rgn,
                hotspot_rgn,
                RGN_OR,
            ) == 0
            {
                DeleteObject(combined_rgn);
                DeleteObject(hotspot_rgn);
                DeleteObject(temp_rgn);
                return Err("Failed to union hotspot region".to_string());
            }

            DeleteObject(combined_rgn);
            DeleteObject(hotspot_rgn);
            combined_rgn = temp_rgn;
        }

        let current_style = GetWindowLongW(hwnd, GWL_EXSTYLE);
        let new_style = current_style & !(WS_EX_TRANSPARENT as i32);
        SetWindowLongW(hwnd, GWL_EXSTYLE, new_style);

        if SetWindowRgn(hwnd, combined_rgn, 1) == 0 {
            DeleteObject(combined_rgn);
            return Err("Failed to set window region".to_string());
        }
    }

    let rebuild_ns = rebuild_start.elapsed().as_nanos() as u64;

    if std::env::var("FIDGET_TRACE_MASK_REBUILD").is_ok() {
        let rebuild_ms = rebuild_ns as f64 / 1_000_000.0;
        eprintln!(
            "mask_rebuild: {}x{} @{}x scale, {} opaque pixels, {:.2} ms",
            scaled_width, scaled_height, scale, opaque_count, rebuild_ms
        );
    }

    Ok(())
}

/// Clear the input region, making the entire window click-through.
fn clear_input_region(hwnd: HWND) -> Result<(), String> {
    // SAFETY: hwnd is a valid HWND from Tauri's raw window handle. GetWindowLongW,
    // SetWindowLongW, and SetWindowRgn are documented safe with valid HWNDs; passing
    // null to SetWindowRgn clears the region.
    unsafe {
        let current_style = GetWindowLongW(hwnd, GWL_EXSTYLE);
        let new_style = current_style | (WS_EX_TRANSPARENT as i32);
        SetWindowLongW(hwnd, GWL_EXSTYLE, new_style);

        SetWindowRgn(hwnd, std::ptr::null_mut(), 1);
    }

    Ok(())
}
