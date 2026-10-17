//! Windows window geometry via EnumWindows and GetWindowRect.
//! Visible application windows in z-order; owner is the process image name
//! so `owner` is an application name on every platform. Geometry is
//! consent-free; the owner and the title are not, and one consent covers the
//! pair (ADR-0032).

use std::sync::Mutex;

use fidget_core::window_source::{Capabilities, Rect, WindowRect, WindowSource, WorldGeometry};

use crate::mcp_resources::WindowTitle;
use windows_sys::core::BOOL;
use windows_sys::Win32::Foundation::{HWND, LPARAM, RECT, TRUE};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowLongW, GetWindowRect, GetWindowTextW, IsWindowVisible, GWL_EXSTYLE,
    GWL_STYLE,
};

use super::super::windows_perch::counts_for_desktop;
use super::overlay::overlay_hwnds;
use super::process::window_owner;

/// The Windows window manager's view of the desktop.
pub struct WindowsWindowSource {
    /// Where the usable part of each display comes from. Taskbar/dock bounds
    /// on Windows come from the work area Tauri already reads.
    read_displays: Box<dyn Fn() -> (Vec<Rect>, Option<Rect>) + Send + Sync>,
    /// Whether the window-names consent is usable right now.
    can_read_names: Box<dyn Fn() -> bool + Send + Sync>,
}

impl WindowsWindowSource {
    pub fn new(
        read_displays: impl Fn() -> (Vec<Rect>, Option<Rect>) + Send + Sync + 'static,
        can_read_names: impl Fn() -> bool + Send + Sync + 'static,
    ) -> Self {
        Self {
            read_displays: Box::new(read_displays),
            can_read_names: Box::new(can_read_names),
        }
    }
}

impl WindowSource for WindowsWindowSource {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            window_geometry: true,
            absolute_positioning: true,
        }
    }

    fn read(&self) -> WorldGeometry {
        let (usable_frames, dock) = (self.read_displays)();
        let can_read_names = (self.can_read_names)();
        let windows = visible_windows(can_read_names);

        if std::env::var("FIDGET_TRACE_WINDOWS").is_ok() {
            static LOGGED: std::sync::atomic::AtomicBool =
                std::sync::atomic::AtomicBool::new(false);
            if !LOGGED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                eprintln!("window_source: {} visible windows", windows.len());
                for (i, w) in windows.iter().take(3).enumerate() {
                    eprintln!(
                        "  [{}] owner={}, bounds=({},{})@{}×{}",
                        i,
                        w.owner.as_deref().unwrap_or("(name withheld)"),
                        w.bounds.x,
                        w.bounds.y,
                        w.bounds.width,
                        w.bounds.height
                    );
                }
            }
        }

        WorldGeometry {
            usable_frames,
            windows,
            dock,
        }
    }
}

/// Visible application windows, frontmost first (z-order).
pub(super) fn visible_windows(can_read_names: bool) -> Vec<WindowRect> {
    let state = Mutex::new((Vec::new(), can_read_names, overlay_hwnds()));

    // SAFETY: EnumWindows takes a callback and a pointer-sized parameter. The
    // callback's signature matches the required WNDENUMPROC ABI. The state
    // reference lives until EnumWindows returns, and the callback never escapes.
    unsafe {
        EnumWindows(Some(enum_window_callback), &state as *const _ as LPARAM);
    }

    state.into_inner().unwrap().0
}

/// Owner plus title, same walk and order as `visible_windows`. Title is
/// `GetWindowText` only — never used as an owner fallback.
pub fn visible_window_titles() -> Vec<WindowTitle> {
    visible_windows(true)
        .into_iter()
        .map(|window| WindowTitle {
            title: window.title.unwrap_or_default(),
            owner: window.owner.unwrap_or_default(),
        })
        .collect()
}

/// EnumWindows callback that collects visible application windows.
/// SAFETY: hwnd is valid for the call; lparam is the pointer
/// `visible_windows` passed in, still live and pointing at the Mutex.
unsafe extern "system" fn enum_window_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let state = &*(lparam as *const Mutex<(Vec<WindowRect>, bool, Vec<u64>)>);

    if let Ok(mut locked) = state.lock() {
        let (ref mut windows, can_read_names, ref overlays) = *locked;
        if let Some(window_rect) = window_rect(hwnd, can_read_names, overlays) {
            windows.push(window_rect);
        }
    }

    TRUE
}

fn window_rect(hwnd: HWND, can_read_names: bool, overlays: &[u64]) -> Option<WindowRect> {
    // SAFETY: hwnd comes from EnumWindows, which guarantees it is valid for
    // the callback's execution. IsWindowVisible is a simple read.
    let is_window_visible = unsafe { IsWindowVisible(hwnd) } != 0;

    // SAFETY: GetWindowLongW on GWL_STYLE reads the window's style bits.
    // hwnd is still valid.
    let style = unsafe { GetWindowLongW(hwnd, GWL_STYLE) };

    // SAFETY: GetWindowLongW on GWL_EXSTYLE reads the extended style bits.
    let ex_style = unsafe { GetWindowLongW(hwnd, GWL_EXSTYLE) };

    if !counts_for_desktop(is_window_visible, style, ex_style, hwnd as u64, overlays) {
        return None;
    }

    // SAFETY: zeroed RECT is valid for GetWindowRect to write into — all
    // zeroes is a valid but empty rectangle.
    let mut rect: RECT = unsafe { std::mem::zeroed() };
    // SAFETY: GetWindowRect writes into the out-pointer rect, which lives
    // until this function returns.
    if unsafe { GetWindowRect(hwnd, &mut rect) } == 0 {
        return None;
    }

    if rect.right <= rect.left || rect.bottom <= rect.top {
        return None;
    }

    // No title fallback when the process cannot be named: the title was the
    // bug. "Unknown" matches no exclusion, so the Engine still gets a Perch
    // rectangle and leaks that word, never a title.
    let owner = can_read_names.then(|| window_owner(hwnd).unwrap_or_else(|| "Unknown".to_string()));

    let title = if can_read_names {
        read_window_title(hwnd)
    } else {
        None
    };

    Some(WindowRect {
        id: hwnd as u64,
        bounds: Rect {
            x: f64::from(rect.left),
            y: f64::from(rect.top),
            width: f64::from(rect.right - rect.left),
            height: f64::from(rect.bottom - rect.top),
        },
        owner,
        title,
        layer: 0,
    })
}

/// Returns None when the title is empty or cannot be read.
fn read_window_title(hwnd: HWND) -> Option<String> {
    const MAX_TITLE_LENGTH: usize = 512;
    let mut buffer = [0u16; MAX_TITLE_LENGTH];

    // SAFETY: GetWindowTextW reads the window title into buffer. hwnd is
    // valid from EnumWindows. buffer is properly sized and lives until the
    // function returns. GetWindowTextW includes the null terminator in the
    // count, so the written slice never exceeds buffer.len().
    let len = unsafe { GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32) };

    if len <= 0 {
        return None;
    }

    // len is the count of UTF-16 code units written, excluding the null.
    let title = String::from_utf16_lossy(&buffer[..len as usize]);

    if title.is_empty() {
        None
    } else {
        Some(title)
    }
}
