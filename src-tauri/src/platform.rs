//! The seam between the Shell and whatever operating system it is running on.
//!
//! `fidget-core` declares what the app needs — a `WindowSource`, an
//! `ActivitySource`, a window that refuses keyboard focus. This module picks who
//! answers. macOS answers with AppKit and CoreGraphics; every other platform
//! gets the degraded mode `docs/SPEC.md` describes, which is a supported state
//! rather than an error.
//!
//! The dispatch lives here rather than in `main.rs` so that adding a platform is
//! one edit in one file.

use std::fs;
use std::path::Path;
#[cfg(unix)]
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use fidget_core::sensing::ActivitySource;
use fidget_core::window_source::{Rect, WindowSource};
use tauri::{Emitter, Manager};

/// One button as the overlay webview witnesses it. `CGEventSource` misses
/// clicks on our window. Two bits: a click can begin and end between polls,
/// and the edge keeps it until a read consumes it (#182).
struct Witness {
    /// What the webview last reported: true from pointerdown to pointerup.
    down: AtomicBool,
    /// Set on every pointerdown, cleared by the read that consumes it.
    pressed: AtomicBool,
}

impl Witness {
    const fn new() -> Self {
        Self {
            down: AtomicBool::new(false),
            pressed: AtomicBool::new(false),
        }
    }

    /// The webview heard the button go down or up.
    fn report(&self, down: bool) {
        self.down.store(down, Ordering::SeqCst);
        if down {
            self.pressed.store(true, Ordering::SeqCst);
        }
    }

    /// Whether the button is down now, or was pressed since the last call.
    /// `|` not `||`, so `swap` always runs and the edge is consumed even when
    /// the level is already true.
    fn take(&self) -> bool {
        self.down.load(Ordering::SeqCst) | self.pressed.swap(false, Ordering::SeqCst)
    }

    /// The overlay is no longer a witness, so nothing it holds can be trusted.
    fn forget(&self) {
        self.down.store(false, Ordering::SeqCst);
        self.pressed.store(false, Ordering::SeqCst);
    }
}

static OVERLAY_PRIMARY: Witness = Witness::new();
static OVERLAY_SECONDARY: Witness = Witness::new();

/// Which Instance is typing a quick message. A level, not a press edge:
/// the hold lasts as long as the field has the caret.
static OVERLAY_COMPOSING: Mutex<Option<String>> = Mutex::new(None);

/// Empty is none. The overlay sends "" when the caret leaves.
pub fn set_overlay_composing(instance: Option<String>) {
    if let Ok(mut slot) = OVERLAY_COMPOSING.lock() {
        *slot = instance.filter(|id| !id.is_empty());
    }
}

pub fn overlay_composing() -> Option<String> {
    OVERLAY_COMPOSING.lock().ok().and_then(|slot| slot.clone())
}

/// Quick message state: instance, open, text, focused.
#[derive(Clone, Debug, Default)]
pub struct QmState {
    pub instance: String,
    pub open: bool,
    pub text: String,
    pub focused: bool,
}

/// Which Instance has a visible quick message pill and its state.
static OVERLAY_QM_STATE: Mutex<Option<QmState>> = Mutex::new(None);

/// Which Instance had QM dismissed by drag/leave (for latch).
static OVERLAY_QM_DRAG_DISMISS: Mutex<Option<String>> = Mutex::new(None);

/// Update QM state. Empty instance means none.
pub fn set_overlay_qm_state(state: Option<QmState>) {
    if let Ok(mut slot) = OVERLAY_QM_STATE.lock() {
        *slot = state.filter(|s| !s.instance.is_empty());
    }
}

pub fn overlay_qm_state() -> Option<QmState> {
    OVERLAY_QM_STATE.lock().ok().and_then(|slot| slot.clone())
}

/// Set the drag dismiss latch for an instance.
pub fn set_overlay_qm_drag_dismiss(instance: Option<String>) {
    if let Ok(mut slot) = OVERLAY_QM_DRAG_DISMISS.lock() {
        *slot = instance.filter(|s| !s.is_empty());
    }
}

/// Check and consume the drag dismiss latch for an instance.
pub fn take_overlay_qm_drag_dismiss(instance: &str) -> bool {
    if let Ok(mut slot) = OVERLAY_QM_DRAG_DISMISS.lock() {
        if slot.as_deref() == Some(instance) {
            *slot = None;
            return true;
        }
    }
    false
}

/// Legacy: which instance has QM visible (ignores text/focus).
/// Only returns the instance when the QM is actually open.
pub fn overlay_qm_visible() -> Option<String> {
    overlay_qm_state()
        .filter(|s| s.open)
        .map(|s| s.instance)
}

/// Legacy setter: visible/hidden without text/focus.
pub fn set_overlay_qm_visible(instance: Option<String>) {
    if let Some(id) = instance {
        if id.is_empty() {
            set_overlay_qm_state(None);
        } else {
            set_overlay_qm_state(Some(QmState {
                instance: id,
                open: true,
                text: String::new(),
                focused: false,
            }));
        }
    } else {
        set_overlay_qm_state(None);
    }
}

/// Which mouse buttons one tick found down. One type so X11 pays one
/// XQueryPointer instead of two (#268), and so both consuming witness reads
/// live in one place.
#[derive(Clone, Copy, Debug, Default)]
pub struct ButtonsDown {
    pub primary: bool,
    pub secondary: bool,
}

/// The overlay heard the primary button go down or up.
pub fn set_overlay_primary(down: bool) {
    OVERLAY_PRIMARY.report(down);
    #[cfg(target_os = "macos")]
    if down {
        macos::primary_press_began();
    }
}

/// Rectangles one overlay wants clicks over, besides the art: `(label, [x, y,
/// width, height])` in that overlay's own coordinates. A `Vec` because
/// `Vec::new` is const; only the "Open chat" control (#547) ever fills it.
static OVERLAY_HOTSPOTS: Mutex<Vec<(String, [i32; 4])>> = Mutex::new(Vec::new());

/// Painted UI rectangles (bubble, thinking) per overlay: `(label, [x, y, width, height])`.
/// These go into the Windows region but are NOT clickable.
static OVERLAY_PAINTED: Mutex<Vec<(String, [i32; 4])>> = Mutex::new(Vec::new());

/// Replace everything `label` asked for. An empty list is how an overlay says
/// it wants nothing but the art again.
pub fn set_overlay_hotspots(label: &str, rects: Vec<[i32; 4]>) {
    let Ok(mut hotspots) = OVERLAY_HOTSPOTS.lock() else {
        return;
    };
    hotspots.retain(|(owner, _)| owner != label);
    hotspots.extend(rects.into_iter().map(|rect| (label.to_string(), rect)));
}

/// Update painted rects for an overlay. Windows unions these into the region.
pub fn set_overlay_painted(label: &str, rects: Vec<[i32; 4]>) {
    // Region invalidated automatically on next frame: RegionParams includes
    // painted, so last_mask comparison detects change and triggers rebuild.
    let Ok(mut painted) = OVERLAY_PAINTED.lock() else {
        return;
    };
    painted.retain(|(owner, _)| owner != label);
    painted.extend(rects.into_iter().map(|rect| (label.to_string(), rect)));
}

/// Whether `label`'s overlay wants the click at `(x, y)`, in its coordinates.
pub fn over_overlay_hotspot(label: &str, x: i32, y: i32) -> bool {
    OVERLAY_HOTSPOTS.lock().is_ok_and(|hotspots| {
        hotspots.iter().any(|(owner, [left, top, width, height])| {
            owner == label && x >= *left && x < left + width && y >= *top && y < top + height
        })
    })
}

/// The hotspot rectangles one overlay reported, in its own coordinates.
/// Not `cfg`'d out on macOS: the neighbour-isolation test lives here, and
/// compiling it only on X11/Windows would skip it on the machine this is written on.
#[allow(dead_code)]
pub fn overlay_hotspots_for(label: &str) -> Vec<[i32; 4]> {
    OVERLAY_HOTSPOTS.lock().map_or_else(
        |_| Vec::new(),
        |hotspots| {
            hotspots
                .iter()
                .filter(|(owner, _)| owner == label)
                .map(|(_, rect)| *rect)
                .collect()
        },
    )
}

/// The painted rectangles one overlay reported, in its own coordinates.
#[allow(dead_code)]
pub fn overlay_painted_for(label: &str) -> Vec<[i32; 4]> {
    OVERLAY_PAINTED.lock().map_or_else(
        |_| Vec::new(),
        |painted| {
            painted
                .iter()
                .filter(|(owner, _)| owner == label)
                .map(|(_, rect)| *rect)
                .collect()
        },
    )
}

/// The overlay heard the secondary button go down or up. Same miss as the
/// primary: `CGEventSource` has been seen to skip a right-click on our window.
pub fn set_overlay_secondary(down: bool) {
    OVERLAY_SECONDARY.report(down);
}

/// The overlay is passing clicks through, so it cannot still be holding a
/// press. Must not consult the session poll: that poll misses a press our
/// own window swallowed, which is when this witness is the only one.
pub fn overlay_passes_clicks_through() {
    OVERLAY_PRIMARY.forget();
    OVERLAY_SECONDARY.forget();
}

fn overlay_primary_down() -> bool {
    OVERLAY_PRIMARY.take()
}

fn overlay_secondary_down() -> bool {
    OVERLAY_SECONDARY.take()
}

/// The displays as the frame loop needs to see them, from one read.
/// All of it is `NSScreen`, main-thread only, so the loop is served the last
/// answer rather than asking for its own.
#[derive(Clone, Debug)]
pub struct Displays {
    /// The whole frame of each display, in logical points. Whole rather than
    /// usable: the overlay has to cover the Dock and the menu bar so a held
    /// sprite is not clipped there.
    pub frames: Vec<Rect>,
    /// The part of each display a sprite may occupy, in logical points.
    /// With true Dock bounds, that display's floor drops to the bottom edge
    /// and the reserved strip arrives as `dock`.
    pub usable_frames: Vec<Rect>,
    /// The Dock's true bounds and which source produced them; see
    /// `macos::dock_bounds` for the chain. `None` keeps the full-width strip.
    pub dock: Option<(Rect, DockSource)>,
    /// The scale factor the windowing layer measures the global cursor against.
    /// Always the primary's, whichever display the cursor is over: that is the
    /// one factor the layer multiplied by, so that is the one that undoes it.
    pub cursor_scale: f64,
    /// The platform's name for each display, in the same order as `frames`.
    /// A label, when the platform has one. Not an id.
    pub names: Vec<Option<String>>,
}

impl Default for Displays {
    /// A desktop nothing has been read from yet. The scale is 1 rather than 0
    /// because it is a divisor.
    fn default() -> Self {
        Self {
            frames: Vec::new(),
            usable_frames: Vec::new(),
            dock: None,
            cursor_scale: 1.0,
            names: Vec::new(),
        }
    }
}

/// Which rung of the Dock-geometry chain answered; see `macos::dock_bounds`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
// Only macOS ever names a source: `exact_dock` is `None` on every other lane,
// so no variant is ever constructed there — but `Displays` carries the type on
// all of them.
#[allow(dead_code)]
pub enum DockSource {
    /// `CoreDockGetRect`, the private SPI: exact, no grant needed.
    CoreDock,
    /// The Accessibility API, where trust was already granted.
    Accessibility,
}

/// The last read of the displays, shared between the refresh and its readers.
#[derive(Clone, Default)]
pub struct DisplayCache(Arc<Mutex<Displays>>);

impl DisplayCache {
    /// What the main thread last saw. Stale by up to `USABLE_FRAME_REFRESH`,
    /// which is a desktop that was accurate a moment ago rather than a stall in
    /// the frame loop.
    pub fn read(&self) -> Displays {
        self.0.lock().map(|read| read.clone()).unwrap_or_default()
    }
}

/// How often the reserved strips are re-read. They move at human speed, so
/// 500ms is far more often than needed and still costs at most one read
/// every other poll.
const USABLE_FRAME_REFRESH: Duration = Duration::from_millis(500);

/// Whether enough time has passed to re-read the displays, marking them read
/// if so. Every platform waits the same interval; what differs is the refresh
/// each one then runs, which stays at its call site: macOS posts to the main
/// thread, X11 and Windows read where they stand.
fn due(refreshed: &Mutex<Instant>) -> bool {
    let Ok(mut refreshed) = refreshed.lock() else {
        return false;
    };
    if refreshed.elapsed() < USABLE_FRAME_REFRESH {
        return false;
    }
    *refreshed = Instant::now();
    true
}

#[cfg(target_os = "macos")]
mod macos;

#[cfg(all(unix, not(target_os = "macos")))]
mod x11;

#[cfg(not(unix))]
mod windows;

#[cfg(any(test, not(unix)))]
mod windows_perch;

mod ctrl_c;

/// Children spawned while this is held inherit "ignore Ctrl+C".
/// Nested holds stay ignored until the last drop. This process listens
/// again after that drop, so the quit handler still runs.
pub(crate) struct SpawnedCtrlC;

impl SpawnedCtrlC {
    pub(crate) fn hold() -> Self {
        if ctrl_c::enter() {
            #[cfg(windows)]
            if !windows::suppress_ctrl_c_for_children() {
                eprintln!("quit: could not keep spawned processes off Ctrl+C");
                let _ = ctrl_c::exit();
            }
        }
        Self
    }
}

impl Drop for SpawnedCtrlC {
    fn drop(&mut self) {
        if ctrl_c::exit() {
            #[cfg(windows)]
            if !windows::restore_ctrl_c() {
                eprintln!("quit: could not listen for Ctrl+C again");
            }
        }
    }
}

/// Whether an X server answers this process — a real X11 session, or XWayland
/// proxying for a Wayland one. `WAYLAND_DISPLAY` is set even for XWayland
/// clients; under XWayland the EWMH/XShape path works (#266).
#[cfg(all(unix, not(target_os = "macos")))]
fn x11_answers() -> bool {
    x11::connection().is_some()
}

/// Point GTK at its X11 backend when an X server answers. Must run before
/// GTK initializes. Conditional: forcing `x11` with no XWayland aborts
/// startup. A user-set `GDK_BACKEND` wins.
#[cfg(all(unix, not(target_os = "macos")))]
pub fn prefer_x11_backend() {
    if std::env::var_os("GDK_BACKEND").is_none() && x11_answers() {
        std::env::set_var("GDK_BACKEND", "x11");
    }
}

/// Nothing to choose: macOS and Windows do not run GTK.
#[cfg(any(target_os = "macos", not(unix)))]
pub fn prefer_x11_backend() {}

/// Make the overlay a floating, non-activating panel.
#[cfg(target_os = "macos")]
pub fn configure_overlay(window: &tauri::WebviewWindow) -> Result<(), String> {
    macos::configure_overlay(window)
}

/// X11 on Linux: EWMH states for floating, skip-taskbar, skip-pager, plus
/// per-pixel click-through via XShapeCombineMask from the sprite alpha.
/// GDK Wayland yields a `wl_surface` and this returns Err (DESIGN.md decision 3).
#[cfg(all(unix, not(target_os = "macos")))]
pub fn configure_overlay(window: &tauri::WebviewWindow) -> Result<(), String> {
    x11::configure_overlay(window)
}

/// Raise `window` above the overlay, and give it the keyboard. Main thread only.
///
/// NSStatusWindowLevel sits above the overlay's NSFloatingWindowLevel, so tray-open is not a no-op.
#[cfg(target_os = "macos")]
pub fn raise_above_overlay(window: &tauri::WebviewWindow) -> Result<(), String> {
    use objc2::msg_send;
    use objc2_app_kit::{NSApplication, NSStatusWindowLevel, NSWindowLevel};
    use objc2_foundation::MainThreadMarker;

    let ptr = window
        .ns_window()
        .map_err(|e| format!("window has no native handle: {e}"))?
        as *mut objc2::runtime::AnyObject;

    unsafe {
        let ns_window = &*ptr;
        let mtm = MainThreadMarker::new_unchecked();
        let app = NSApplication::sharedApplication(mtm);

        let _: () = msg_send![ns_window, setLevel: NSStatusWindowLevel as NSWindowLevel];
        let _: () = msg_send![ns_window, setHidesOnDeactivate: false];
        #[allow(deprecated)]
        app.activateIgnoringOtherApps(true);
        let _: () = msg_send![ns_window, orderFrontRegardless];
        let _: () = msg_send![ns_window, makeKeyAndOrderFront: std::ptr::null::<objc2::runtime::AnyObject>()];
    }

    Ok(())
}

/// Nudge the menu bar icon toward the clock on first launch. Main thread only.
#[cfg(target_os = "macos")]
pub fn seed_tray_position() {
    macos::seed_status_item_position();
}

/// Resize the tray icon to standard menu bar height. Main thread only.
#[cfg(target_os = "macos")]
pub fn tune_tray_icon(tray: &tauri::tray::TrayIcon) -> Result<(), tauri::Error> {
    macos::tune_tray_icon(tray)
}

/// Raise `window` above the overlay, and give it the keyboard. Main thread only.
///
/// The overlay is `_NET_WM_STATE_ABOVE`. keep_above shares that band (#799).
#[cfg(all(unix, not(target_os = "macos")))]
pub fn raise_above_overlay(window: &tauri::WebviewWindow) -> Result<(), String> {
    use gtk::prelude::*;

    let gtk_window = window
        .gtk_window()
        .map_err(|e| format!("window has no gtk handle: {e}"))?;
    gtk_window.set_keep_above(true);
    gtk_window.present();
    // Mapped windows need a ClientMessage. GTK keep_above is the native path; this covers a WM that ignored it.
    if let Err(why) = x11::raise_settings_ewmh_above(window) {
        eprintln!("ewmh raise: {why}");
    }
    Ok(())
}

/// Raise `window` above the overlay, and give it the keyboard. Main thread only.
///
/// The overlay is HWND_TOPMOST. A normal window cannot stack above that band.
#[cfg(not(unix))]
pub fn raise_above_overlay(window: &tauri::WebviewWindow) -> Result<(), String> {
    windows::raise_above_overlay(window)
}

#[cfg(target_os = "macos")]
pub use macos::MinimizeObserver;

/// Nothing to remove. Windows and Linux see minimize through Tauri's focus and
/// resize events: Windows sends a resize to 0x0 whether or not it has focus.
#[cfg(not(target_os = "macos"))]
pub struct MinimizeObserver;

/// Call `minimized` when `window` is minimized (`true`) or restored (`false`),
/// focused or not. Dropping the result stops it.
#[cfg(target_os = "macos")]
pub fn observe_minimize(
    window: &tauri::WebviewWindow,
    minimized: impl Fn(bool) + Send + Sync + 'static,
) -> Result<MinimizeObserver, String> {
    macos::observe_minimize(window, minimized)
}

/// Call `minimized` when `window` is minimized (`true`) or restored (`false`).
#[cfg(not(target_os = "macos"))]
pub fn observe_minimize(
    _window: &tauri::WebviewWindow,
    _minimized: impl Fn(bool) + Send + Sync + 'static,
) -> Result<MinimizeObserver, String> {
    Ok(MinimizeObserver)
}

/// Undo `raise_above_overlay`'s level, so `window` stacks with other apps' windows.
/// macOS and Linux go through tao, whose normal level is the one to return to.
#[cfg(unix)]
pub fn lower_to_normal_level(window: &tauri::WebviewWindow) -> Result<(), String> {
    window.set_always_on_top(false).map_err(|e| e.to_string())
}

/// Undo `raise_above_overlay`'s level, so `window` stacks with other apps' windows.
/// Not tao's `set_always_on_top(false)`: tao never saw the topmost insert, so it skips the change.
#[cfg(not(unix))]
pub fn lower_to_normal_level(window: &tauri::WebviewWindow) -> Result<(), String> {
    windows::lower_to_normal_level(window)
}

/// Push a fresh snapshot to the Settings webview. Main thread only.
pub fn refresh_settings(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("settings") {
        if let Err(why) = window.emit("settings-refresh", ()) {
            eprintln!("settings webview refresh: {why}");
        }
    }
}

/// Hand a file the user owns to whatever the desktop opens it with.
/// Created empty first: Memory has no file until the Director remembers, and
/// an opener given a missing path reports it missing.
pub fn open_path(path: &Path) -> Result<(), String> {
    ensure_file(path)?;
    hand_over(path.as_os_str())
}

/// The schemes a link in a reply is allowed to open. `mailto` belongs: a
/// model writing an address as a link means it to be mailed. Everything else
/// is refused; the asker is untrusted text.
const OPENABLE: [&str; 3] = ["http", "https", "mailto"];

/// Hand a URL from a reply to whatever the desktop opens it with.
/// Last-edge gate (#371): parse, don't prefix-match (`&#106;avascript:`). Open
/// the parsed form, never the caller's string; a refused scheme is an error.
pub fn open_url(url: &str) -> Result<(), String> {
    // A parsed URL always starts with its scheme, so it can never be read as an
    // option by `open` or `xdg-open`. `Command` execs directly with no shell,
    // so there is no metacharacter to quote either.
    hand_over(std::ffi::OsStr::new(&openable(url)?))
}

/// The URL to hand over, or why this one is not handed over.
/// Split from `open_url` so policy can be tested without launching a browser.
pub(crate) fn openable(url: &str) -> Result<String, String> {
    let parsed = url::Url::parse(url).map_err(|why| format!("not a URL: {why}"))?;
    if !OPENABLE.contains(&parsed.scheme()) {
        return Err(format!("a {} link does not open", parsed.scheme()));
    }
    Ok(parsed.into())
}

/// The one per-platform call, shared by both entry points above.
fn hand_over(target: &std::ffi::OsStr) -> Result<(), String> {
    #[cfg(unix)]
    {
        opener(target)
            .spawn()
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
    #[cfg(not(unix))]
    {
        opener(target)
    }
}

/// Not platform-specific, so it is written once rather than in each arm below.
fn ensure_file(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    if !path.exists() {
        fs::write(path, "").map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn opener(target: &std::ffi::OsStr) -> Command {
    let mut command = Command::new("open");
    command.arg(target);
    command
}

#[cfg(all(unix, not(target_os = "macos")))]
fn opener(target: &std::ffi::OsStr) -> Command {
    let mut command = Command::new("xdg-open");
    command.arg(target);
    command
}

/// Open with the default application via ShellExecuteW.
/// Not `cmd /C start`: `&` and `%` are syntax there (#255). Neither `open` nor
/// `opener` removes the `unsafe`, so a crate would not change what can go wrong.
#[cfg(not(unix))]
fn opener(target: &std::ffi::OsStr) -> Result<(), String> {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let file = shell_execute_file_wide(target);
    let operation: Vec<u16> = "open".encode_utf16().chain(Some(0)).collect();

    // SAFETY: both wide strings are `.chain(Some(0))` NUL-terminated and outlive
    // the call. Null hwnd/params/directory are documented "no owner, no args,
    // inherit cwd". Return > 32 is MSDN success.
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            operation.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    if (result as isize) > 32 {
        Ok(())
    } else {
        Err(format!(
            "ShellExecuteW failed opening {} (code {})",
            target.to_string_lossy(),
            result as isize
        ))
    }
}

/// The NUL-terminated wide path ShellExecuteW receives. Kept as its own
/// function so tests can assert `&`, `%`, and spaces reach the API intact
/// without spawning a viewer.
#[cfg(not(unix))]
fn shell_execute_file_wide(target: &std::ffi::OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    target.encode_wide().chain(Some(0)).collect()
}

/// Windows: extended window styles for floating, non-activating overlay.
#[cfg(not(unix))]
pub fn configure_overlay(window: &tauri::WebviewWindow) -> Result<(), String> {
    windows::configure_overlay(window)
}

/// Click-through for one overlay.
/// On Windows the tool-window bits are put back after tao's style rewrite,
/// on the event-loop thread, or that rewrite lands last and clears them.
pub fn set_overlay_click_through(
    window: &tauri::WebviewWindow,
    ignore: bool,
) -> Result<(), String> {
    #[cfg(unix)]
    {
        window
            .set_ignore_cursor_events(ignore)
            .map_err(|e| e.to_string())
    }
    #[cfg(not(unix))]
    {
        windows::set_click_through(window, ignore)
    }
}

/// Update the input region for the overlay window based on the sprite's alpha mask.
/// X11 and Windows then union hotspot rects so a control drawn outside the
/// art still receives clicks. macOS uses `set_ignore_cursor_events`.
/// `click_through` (Windows-only) controls whether the sprite region is hit-testable.
#[cfg(all(unix, not(target_os = "macos")))]
#[allow(clippy::too_many_arguments)]
pub fn update_input_region(
    window: &tauri::WebviewWindow,
    mask_data: Option<&fidget_core::overlay::AlphaMask>,
    sprite_x: i32,
    sprite_y: i32,
    sprite_facing: i32,
    scale: i32,
    hotspot_rects: &[[i32; 4]],
    _click_through: bool,
) -> Result<(), String> {
    x11::update_input_region(
        window,
        mask_data,
        sprite_x,
        sprite_y,
        sprite_facing,
        scale,
        hotspot_rects,
    )
}

/// Windows: SetWindowRgn from the sprite's swept ink rectangles and hotspots.
/// The region clips drawing as well as input. `click_through` controls
/// WS_EX_TRANSPARENT.
#[cfg(not(unix))]
pub fn update_input_region(
    window: &tauri::WebviewWindow,
    art: Option<&[[i32; 4]]>,
    hotspot_rects: &[[i32; 4]],
    painted_rects: &[[i32; 4]],
    click_through: bool,
) -> Result<(), String> {
    windows::update_input_region(window, art, hotspot_rects, painted_rects, click_through)
}

/// Toggle WS_EX_TRANSPARENT without reapplying the region. Used when only
/// click-through state changes on an idle sprite (mask unchanged, ignore flipped).
#[cfg(not(unix))]
pub fn toggle_click_through_only(
    window: &tauri::WebviewWindow,
    click_through: bool,
) -> Result<(), String> {
    windows::toggle_click_through_only(window, click_through)
}

/// Whether this lane honours the off-art rectangles the renderer reports (#547).
/// macOS hit-tests via boolean click-through; X11 and Windows union them into
/// the input region, so a control above the head takes clicks on every platform.
#[cfg(target_os = "macos")]
pub fn hotspots_hit_tested() -> bool {
    true
}

#[cfg(all(unix, not(target_os = "macos")))]
pub fn hotspots_hit_tested() -> bool {
    true
}

#[cfg(not(unix))]
pub fn hotspots_hit_tested() -> bool {
    true
}

static DOUBLE_CLICK_INTERVAL_MS: OnceLock<u32> = OnceLock::new();

const FALLBACK_DOUBLE_CLICK_MS: u32 = 400;
const MIN_DOUBLE_CLICK_MS: u32 = 100;
const MAX_DOUBLE_CLICK_MS: u32 = 2000;

/// Resolve and clamp the double-click interval, with fallback.
///
/// Pure helper for testing; the public `double_click_interval_ms` caches this.
fn resolve_double_click_interval(raw: Option<u32>) -> u32 {
    match raw {
        Some(value) if value > 0 => value.clamp(MIN_DOUBLE_CLICK_MS, MAX_DOUBLE_CLICK_MS),
        _ => FALLBACK_DOUBLE_CLICK_MS,
    }
}

/// The OS double-click interval, in milliseconds, clamped and with fallback.
/// Queried once, clamped to [100, 2000]ms against pathological settings.
pub fn double_click_interval_ms() -> u32 {
    *DOUBLE_CLICK_INTERVAL_MS.get_or_init(|| {
        let raw = os_double_click_interval_ms();
        let resolved = resolve_double_click_interval(raw);

        if let Some(value) = raw {
            if value > 0 && value != resolved {
                eprintln!(
                    "overlay: double-click interval {}ms (clamped from {}ms)",
                    resolved, value
                );
            } else {
                eprintln!("overlay: double-click interval {}ms", resolved);
            }
        } else {
            eprintln!(
                "overlay: double-click interval fallback to {}ms",
                FALLBACK_DOUBLE_CLICK_MS
            );
        }

        resolved
    })
}

/// The OS double-click interval, in milliseconds, from the platform layer.
#[cfg(target_os = "macos")]
fn os_double_click_interval_ms() -> Option<u32> {
    macos::double_click_interval_ms()
}

#[cfg(all(unix, not(target_os = "macos")))]
fn os_double_click_interval_ms() -> Option<u32> {
    x11::double_click_interval_ms()
}

#[cfg(not(unix))]
fn os_double_click_interval_ms() -> Option<u32> {
    windows::double_click_interval_ms()
}

/// Which mouse buttons are down, or were pressed since the last call.
/// Session poll OR overlay witness, with a Control-click as secondary.
/// Consuming: the frame loop asks once per tick, which is what makes "since
/// the last call" mean "since the last tick".
#[cfg(target_os = "macos")]
pub fn buttons_down() -> ButtonsDown {
    macos::control_click(ButtonsDown {
        primary: overlay_primary_down() || macos::primary_button_down(),
        secondary: overlay_secondary_down() || macos::secondary_button_down(),
    })
}

/// X11 on Linux: one XQueryPointer for both buttons, or the overlay latch.
/// Wayland has only the overlay latch (no global pointer). Poll before the
/// latches so both consuming reads still happen exactly once each.
#[cfg(all(unix, not(target_os = "macos")))]
pub fn buttons_down() -> ButtonsDown {
    let session = x11::buttons_down();
    ButtonsDown {
        primary: overlay_primary_down() || session.primary,
        secondary: overlay_secondary_down() || session.secondary,
    }
}

/// Windows: GetAsyncKeyState for both buttons, or the overlay latch.
#[cfg(not(unix))]
pub fn buttons_down() -> ButtonsDown {
    let session = windows::buttons_down();
    ButtonsDown {
        primary: overlay_primary_down() || session.primary,
        secondary: overlay_secondary_down() || session.secondary,
    }
}

/// Where the Free tier comes from: what the user is in, and how long since they
/// touched anything.
#[cfg(target_os = "macos")]
pub fn activity_source() -> impl ActivitySource {
    macos::MacosActivitySource
}

/// X11 on Linux: _NET_ACTIVE_WINDOW for frontmost, Xss for idle, DPMS for sleep.
/// No X server stubs. Idle has two paths (Wayland idle-notify vs Mutter D-Bus)
/// and no portable one; frontmost and display sleep have none.
#[cfg(all(unix, not(target_os = "macos")))]
pub fn activity_source() -> LinuxActivitySource {
    if x11_answers() {
        LinuxActivitySource::X11(x11::X11ActivitySource)
    } else {
        LinuxActivitySource::Wayland
    }
}

/// Runtime dispatch between X11 and Wayland activity sources on Linux.
#[cfg(all(unix, not(target_os = "macos")))]
pub enum LinuxActivitySource {
    X11(x11::X11ActivitySource),
    Wayland,
}

/// Spawn an XI2 input event listener thread on X11. Returns `None` on other
/// platforms or when XI2 is unavailable. #183.
#[cfg(all(unix, not(target_os = "macos")))]
pub fn spawn_xi2_listener() -> Option<std::sync::mpsc::Receiver<x11::InputEvent>> {
    x11::spawn_listener()
}

#[cfg(target_os = "macos")]
pub use macos::EventTap;

#[cfg(target_os = "macos")]
pub(crate) use macos::sleep_precisely;

/// Sleep for a moving tick. Only macOS coalesces `thread::sleep` enough to need more.
#[cfg(not(target_os = "macos"))]
pub(crate) fn sleep_precisely(duration: std::time::Duration) {
    std::thread::sleep(duration);
}

/// Spawn the macOS mouse event tap. `None` until the Input Monitoring row is
/// checked and macOS has granted it. The frame loop asks again on later idle
/// waits, so a grant that lands mid-run is picked up. Dropping the value stops
/// the tap thread from the frame thread. #721.
#[cfg(target_os = "macos")]
pub fn spawn_event_tap() -> Option<EventTap> {
    macos::spawn_event_tap()
}

#[cfg(all(unix, not(target_os = "macos")))]
impl ActivitySource for LinuxActivitySource {
    fn frontmost_application(&self) -> Option<String> {
        match self {
            Self::X11(source) => source.frontmost_application(),
            Self::Wayland => None,
        }
    }

    fn idle(&self) -> std::time::Duration {
        match self {
            Self::X11(source) => source.idle(),
            Self::Wayland => std::time::Duration::ZERO,
        }
    }

    fn displays_asleep(&self) -> bool {
        match self {
            Self::X11(source) => source.displays_asleep(),
            Self::Wayland => false,
        }
    }
}

/// Windows: GetForegroundWindow for frontmost, GetLastInputInfo for idle.
#[cfg(not(unix))]
pub fn activity_source() -> impl ActivitySource {
    windows::WindowsActivitySource
}

/// Window titles for the MCP resource, gated behind the window-names consent.
#[cfg(target_os = "macos")]
pub fn list_window_titles() -> Vec<crate::mcp_resources::WindowTitle> {
    if !crate::consent::usable(
        crate::consent::CapabilityId::WindowNames,
        crate::consent::live(),
    ) {
        return Vec::new();
    }
    macos::visible_window_titles()
}

/// Titles from `_NET_WM_NAME` / `WM_NAME`, gated behind the window-names consent.
/// Empty on a Wayland session with no X server, which is the supported stub.
#[cfg(all(unix, not(target_os = "macos")))]
pub fn list_window_titles() -> Vec<crate::mcp_resources::WindowTitle> {
    if !crate::consent::usable(
        crate::consent::CapabilityId::WindowNames,
        crate::consent::live(),
    ) {
        return Vec::new();
    }
    if x11_answers() {
        x11::visible_window_titles()
    } else {
        Vec::new()
    }
}

/// Titles from `GetWindowText`, gated behind the window-names consent.
/// Owner is still the process image, never the title, matching the geometry path.
#[cfg(not(unix))]
pub fn list_window_titles() -> Vec<crate::mcp_resources::WindowTitle> {
    if !crate::consent::usable(
        crate::consent::CapabilityId::WindowNames,
        crate::consent::live(),
    ) {
        return Vec::new();
    }
    windows::visible_window_titles()
}

/// Where window geometry comes from. Usable frames via Tauri: CoreGraphics
/// reports the Dock as covering the whole display. Main thread only: asking
/// AppKit from the frame loop appears to work and is not allowed to.
#[cfg(target_os = "macos")]
pub fn window_source(app: tauri::AppHandle) -> (impl WindowSource, DisplayCache) {
    let cache = DisplayCache(Arc::new(Mutex::new(read_displays(&app))));
    let refreshed = Arc::new(Mutex::new(Instant::now()));

    fn can_read_titles() -> bool {
        crate::consent::usable(
            crate::consent::CapabilityId::WindowNames,
            crate::consent::live(),
        )
    }

    let source = macos::MacosWindowSource::new(
        {
            let cache = cache.clone();
            move || {
                // Posted, not awaited: a poll that arrives while the main thread
                // is busy is served the previous answer.
                if due(&refreshed) {
                    let app = app.clone();
                    let cache = cache.clone();
                    let _ = app.clone().run_on_main_thread(move || {
                        let read = read_displays(&app);
                        if let Ok(mut displays) = cache.0.lock() {
                            *displays = read;
                        }
                    });
                }

                let displays = cache.read();
                (
                    displays.usable_frames,
                    displays.dock.map(|(bounds, _)| bounds),
                )
            }
        },
        can_read_titles,
    );

    (source, cache)
}

/// X11 on Linux: read windows from _NET_CLIENT_LIST, with 500ms refresh for hot-plug.
/// A Wayland session with no XWayland stays DisplayOnlySource: no global window
/// list.
#[cfg(all(unix, not(target_os = "macos")))]
pub fn window_source(app: tauri::AppHandle) -> (LinuxWindowSource, DisplayCache) {
    if !x11_answers() {
        let cache = DisplayCache(Arc::new(Mutex::new(read_displays(&app))));
        return (
            LinuxWindowSource::Wayland(DisplayOnlySource(cache.clone())),
            cache,
        );
    }

    let cache = DisplayCache(Arc::new(Mutex::new(read_displays(&app))));
    let refreshed = Arc::new(Mutex::new(Instant::now()));

    fn can_read_titles() -> bool {
        crate::consent::usable(
            crate::consent::CapabilityId::WindowNames,
            crate::consent::live(),
        )
    }

    let source = x11::X11WindowSource::new(
        {
            let cache = cache.clone();
            let app_clone = app.clone();
            move || {
                if due(&refreshed) {
                    *cache.0.lock().unwrap() = read_displays(&app_clone);
                }

                let displays = cache.read();
                (
                    displays.usable_frames,
                    displays.dock.map(|(bounds, _)| bounds),
                )
            }
        },
        can_read_titles,
    );

    (LinuxWindowSource::X11(source), cache)
}

/// Runtime dispatch between X11 and Wayland window sources on Linux.
#[cfg(all(unix, not(target_os = "macos")))]
pub enum LinuxWindowSource {
    X11(x11::X11WindowSource),
    Wayland(DisplayOnlySource),
}

#[cfg(all(unix, not(target_os = "macos")))]
impl WindowSource for LinuxWindowSource {
    fn capabilities(&self) -> fidget_core::window_source::Capabilities {
        match self {
            Self::X11(source) => source.capabilities(),
            Self::Wayland(source) => source.capabilities(),
        }
    }

    fn read(&self) -> fidget_core::window_source::WorldGeometry {
        match self {
            Self::X11(source) => source.read(),
            Self::Wayland(source) => source.read(),
        }
    }
}

/// Screen-edge physics without window geometry: a supported mode, not a
/// failure (`docs/SPEC.md`). Displays still come from Tauri; only windows
/// are missing. The Wayland fallback; X11 fills `window_source` above.
#[cfg(all(unix, not(target_os = "macos")))]
pub struct DisplayOnlySource(DisplayCache);

/// Screen edges and nothing else, for a session where no X server answers.
/// `Capabilities::default()` has no `window_geometry`, so `snapshot()` clears
/// the windows and the Engine gets a floor and walls with no Perches.
#[cfg(all(unix, not(target_os = "macos")))]
impl WindowSource for DisplayOnlySource {
    fn capabilities(&self) -> fidget_core::window_source::Capabilities {
        fidget_core::window_source::Capabilities::default()
    }

    fn read(&self) -> fidget_core::window_source::WorldGeometry {
        fidget_core::window_source::WorldGeometry {
            usable_frames: self.0.read().usable_frames,
            windows: Vec::new(),
            dock: None,
        }
    }
}

/// Windows: read windows via EnumWindows, with 500ms refresh for hot-plug.
#[cfg(not(unix))]
pub fn window_source(app: tauri::AppHandle) -> (impl WindowSource, DisplayCache) {
    let cache = DisplayCache(Arc::new(Mutex::new(read_displays(&app))));
    let refreshed = Arc::new(Mutex::new(Instant::now()));

    let source = windows::WindowsWindowSource::new(
        {
            let cache = cache.clone();
            let app_clone = app.clone();
            move || {
                if due(&refreshed) {
                    *cache.0.lock().unwrap() = read_displays(&app_clone);
                }

                let displays = cache.read();
                (
                    displays.usable_frames,
                    displays.dock.map(|(bounds, _)| bounds),
                )
            }
        },
        || {
            crate::consent::usable(
                crate::consent::CapabilityId::WindowNames,
                crate::consent::live(),
            )
        },
    );

    (source, cache)
}

/// The displays as the windowing layer sees them right now.
/// Portable Tauri so degraded mode still has screen edges. Convert each
/// monitor with that monitor's scale, never the primary's (`docs/SPEC.md`).
pub(crate) fn read_displays(app: &tauri::AppHandle) -> Displays {
    use fidget_core::window_source::{floor_under_dock, in_points, plausible_dock, usable_frame};

    let Ok(monitors) = app.available_monitors() else {
        return Displays::default();
    };

    let mut displays = Displays {
        cursor_scale: app
            .primary_monitor()
            .ok()
            .flatten()
            .map_or(1.0, |monitor| monitor.scale_factor()),
        ..Displays::default()
    };

    for monitor in monitors.iter() {
        let work = monitor.work_area();
        let frame = Rect {
            x: f64::from(monitor.position().x),
            y: f64::from(monitor.position().y),
            width: f64::from(monitor.size().width),
            height: f64::from(monitor.size().height),
        };
        let work = Rect {
            x: f64::from(work.position.x),
            y: f64::from(work.position.y),
            width: f64::from(work.size.width),
            height: f64::from(work.size.height),
        };

        displays
            .frames
            .push(in_points(frame, monitor.scale_factor()));
        displays
            .usable_frames
            .push(usable_frame(frame, work, monitor.scale_factor()));
        displays.names.push(monitor.name().cloned());
    }

    // With true Dock bounds, that display's floor drops to the bottom edge and
    // the Dock rides as a Perch. Believed only when some work area is shaped
    // and placed like a Dock: the claim is from an unversioned source.
    displays.dock = exact_dock().filter(|(bounds, _)| {
        displays
            .frames
            .iter()
            .zip(&displays.usable_frames)
            .any(|(frame, usable)| plausible_dock(bounds, *frame, *usable))
    });
    if let Some((dock, _)) = &displays.dock {
        for (usable, frame) in displays.usable_frames.iter_mut().zip(&displays.frames) {
            *usable = floor_under_dock(*usable, *frame, dock);
        }
    }

    displays
}

/// The Dock's true bounds — macOS, over the SPI-then-Accessibility chain,
/// and nothing anywhere else. Never prompts; see `macos::dock_bounds`.
#[cfg(target_os = "macos")]
fn exact_dock() -> Option<(Rect, DockSource)> {
    macos::dock_bounds()
}

#[cfg(not(target_os = "macos"))]
fn exact_dock() -> Option<(Rect, DockSource)> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A webview built after setup has to suppress again. The last drop is
    /// what lets this process hear Ctrl+C, so the quit handler still runs.
    #[test]
    fn a_late_hold_suppresses_again_and_the_last_drop_releases() {
        let _lock = ctrl_c::lock_tests();
        assert_eq!(ctrl_c::depth(), 0, "a previous test left Ctrl+C suppressed");
        let outer = SpawnedCtrlC::hold();
        assert_eq!(ctrl_c::depth(), 1);
        {
            let inner = SpawnedCtrlC::hold();
            assert_eq!(ctrl_c::depth(), 2);
            drop(inner);
            assert_eq!(ctrl_c::depth(), 1);
        }
        drop(outer);
        assert_eq!(ctrl_c::depth(), 0);
        let late = SpawnedCtrlC::hold();
        assert_eq!(ctrl_c::depth(), 1);
        drop(late);
        assert_eq!(ctrl_c::depth(), 0);
        assert!(
            !ctrl_c::exit(),
            "a restore with nothing held must leave the host listening"
        );
        assert_eq!(ctrl_c::depth(), 0);
    }

    #[test]
    fn a_web_or_mail_link_opens() {
        for url in [
            "https://example.com/a?b=c#d",
            "http://example.com",
            "mailto:someone@example.com",
        ] {
            assert!(openable(url).is_ok(), "{url} should open");
        }
    }

    /// The gate is the reason a reply cannot reach the shell with anything it
    /// likes. Each of these is a scheme that runs or reads rather than browses,
    /// and every one arrives as ordinary model output.
    #[test]
    fn a_link_that_runs_or_reads_does_not_open() {
        for url in [
            "javascript:alert(1)",
            "data:text/html,<script>alert(1)</script>",
            "vbscript:msgbox(1)",
            "file:///etc/passwd",
        ] {
            assert!(openable(url).is_err(), "{url} must not open");
        }
    }

    /// Why the scheme is parsed and not matched. Each of these is
    /// `javascript:` wearing something a `starts_with` would miss. `Url::parse`
    /// applies the WHATWG rules and resolves every one to the real scheme.
    #[test]
    fn an_obfuscated_scheme_is_still_that_scheme() {
        for url in [
            "JaVaScRiPt:alert(1)",
            "JAVASCRIPT:alert(1)",
            "java\tscript:alert(1)",
            "java\nscript:alert(1)",
            "  javascript:alert(1)",
            "\njavascript:alert(1)",
            "&#106;avascript:alert(1)",
        ] {
            assert!(openable(url).is_err(), "{url:?} must not open");
        }
    }

    /// What is handed to the OS is the parsed form, never the caller's string.
    /// Validating one and opening the other is how a gate gets walked past.
    #[test]
    fn the_opened_url_is_the_parsed_one() {
        let opened = openable("  https://example.com  ").expect("a web link opens");
        assert_eq!(opened, "https://example.com/");
        assert!(
            !opened.starts_with(' '),
            "the untrimmed original must not reach the opener, got {opened:?}"
        );
    }

    /// A press that lands on the overlay is one `CGEventSource` has been
    /// seen to miss. The overlay's own pointer events are the other half of
    /// `buttons_down`; without them a click on the sprite is silent.
    #[test]
    fn overlay_primary_is_enough_for_a_press() {
        set_overlay_primary(false);
        set_overlay_primary(true);
        assert!(
            buttons_down().primary,
            "a click the overlay felt must count as the button down"
        );
        set_overlay_primary(false);
        // The session poll may still be true if a real button is held during
        // the test; only the overlay half is under this test's control.
    }

    #[test]
    fn composing_names_one_instance_until_the_caret_leaves() {
        set_overlay_composing(Some("buddy-a".to_string()));
        assert_eq!(overlay_composing().as_deref(), Some("buddy-a"));
        set_overlay_composing(Some(String::new()));
        assert_eq!(overlay_composing(), None);
    }

    #[test]
    fn qm_visible_names_one_instance_until_the_pill_hides() {
        set_overlay_qm_visible(Some("buddy-b".to_string()));
        assert_eq!(overlay_qm_visible().as_deref(), Some("buddy-b"));
        set_overlay_qm_visible(Some(String::new()));
        assert_eq!(overlay_qm_visible(), None);
    }

    #[test]
    fn qm_visible_freezes_locomotion_for_the_owning_instance() {
        let id = "bmo-instance".to_string();
        set_overlay_qm_visible(Some(id.clone()));

        let speech_visible = false;
        let qm_visible_for_this_instance = overlay_qm_visible().as_deref() == Some(&id);
        let locomotion_frozen = speech_visible || qm_visible_for_this_instance;

        assert!(
            locomotion_frozen,
            "QM visible for this instance should freeze locomotion"
        );

        let qm_visible_for_other = overlay_qm_visible().as_deref() == Some("other-id");
        assert!(
            !qm_visible_for_other,
            "QM not visible for a different instance"
        );

        set_overlay_qm_visible(None);
    }

    #[test]
    fn qm_visible_returns_none_when_closed() {
        let id = "test-instance".to_string();
        // Set QM state with open=false
        set_overlay_qm_state(Some(QmState {
            instance: id.clone(),
            open: false,
            text: "some text".to_string(),
            focused: false,
        }));

        // overlay_qm_visible should return None because open=false
        assert_eq!(
            overlay_qm_visible(),
            None,
            "overlay_qm_visible should return None when QM is closed (open=false)"
        );

        // Now set open=true
        set_overlay_qm_state(Some(QmState {
            instance: id.clone(),
            open: true,
            text: "some text".to_string(),
            focused: false,
        }));

        // Now it should return the instance
        assert_eq!(
            overlay_qm_visible(),
            Some(id),
            "overlay_qm_visible should return instance when QM is open (open=true)"
        );

        set_overlay_qm_state(None);
    }

    /// The bubble's "Open chat" control (#547) belongs to the overlay that
    /// drew it. A neighbour must not stop passing clicks at the same
    /// coordinates, and a gone bubble must take its rectangle with it.
    #[test]
    fn a_hotspot_belongs_to_one_overlay_and_goes_when_it_does() {
        set_overlay_hotspots("overlay-test-a", vec![[10, 20, 30, 40]]);
        set_overlay_hotspots("overlay-test-b", vec![]);

        assert!(over_overlay_hotspot("overlay-test-a", 10, 20), "top left");
        assert!(
            over_overlay_hotspot("overlay-test-a", 39, 59),
            "bottom right"
        );
        assert!(
            !over_overlay_hotspot("overlay-test-a", 40, 60),
            "the far edges are outside, as a rectangle's are"
        );
        assert!(
            !over_overlay_hotspot("overlay-test-b", 20, 30),
            "the neighbour asked for nothing there"
        );

        set_overlay_hotspots("overlay-test-a", vec![]);
        assert!(
            !over_overlay_hotspot("overlay-test-a", 20, 30),
            "the bubble is gone and so is its rectangle"
        );
    }

    /// `overlay_hotspots_for` returns exactly the rectangles the named overlay
    /// reported, and nothing from its neighbours.
    #[test]
    fn hotspots_for_retrieves_only_the_named_overlay() {
        set_overlay_hotspots("overlay-for-a", vec![[1, 2, 3, 4], [5, 6, 7, 8]]);
        set_overlay_hotspots("overlay-for-b", vec![[10, 20, 30, 40]]);

        let a = overlay_hotspots_for("overlay-for-a");
        assert_eq!(a, vec![[1, 2, 3, 4], [5, 6, 7, 8]], "both rects for a");

        let b = overlay_hotspots_for("overlay-for-b");
        assert_eq!(b, vec![[10, 20, 30, 40]], "only b's rect");

        let none = overlay_hotspots_for("overlay-for-none");
        assert!(none.is_empty(), "an overlay that reported nothing");

        set_overlay_hotspots("overlay-for-a", vec![]);
        assert!(
            overlay_hotspots_for("overlay-for-a").is_empty(),
            "clearing wipes all rects"
        );

        set_overlay_hotspots("overlay-for-b", vec![]);
    }

    /// A click can begin and end between two polls. The level alone reads
    /// false at both; the edge keeps the down until it has been read once (#182).
    #[test]
    fn a_click_shorter_than_one_tick_is_still_seen_once() {
        let button = Witness::new();
        button.report(true);
        button.report(false);
        assert!(
            button.take(),
            "a press that came and went before anyone looked is still a press"
        );
        assert!(!button.take(), "and only once: the read consumes the edge");
    }

    /// The edge is for the missed down, not a second gesture. A real hold
    /// reads true from the level every tick; letting go must leave nothing
    /// a later tick could mistake for another press.
    #[test]
    fn a_held_button_reads_true_every_tick_and_nothing_after_release() {
        let button = Witness::new();
        button.report(true);
        assert!(button.take());
        assert!(button.take(), "still held: the level carries it");
        button.report(false);
        assert!(!button.take(), "released");
        assert!(!button.take(), "and no phantom press follows the release");
    }

    /// A pointerup the webview never delivered would leave the level set.
    /// Click-through means both bits must drop, or the sprite glues to a
    /// button nobody is pressing.
    #[test]
    fn passing_clicks_through_forgets_a_press_the_overlay_never_released() {
        let button = Witness::new();
        button.report(true);
        button.forget();
        assert!(
            !button.take(),
            "click-through means the overlay is not a witness, so a lost pointerup must not keep the latch"
        );
    }

    /// Memory has no file until the Director remembers, so the opener is
    /// handed a path that does not exist yet. This tests the step before spawn.
    #[test]
    fn the_file_is_there_before_the_opener_is() {
        let dir = std::env::temp_dir().join(format!(
            "fidget-open-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("memory.md");

        ensure_file(&path).expect("a missing Memory Manifest is created, not an error");
        assert_eq!(fs::read_to_string(&path).unwrap(), "");

        fs::write(&path, "remembered").unwrap();
        ensure_file(&path).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "remembered",
            "opening Memory must not blank it"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    /// A path with a space in it is the case that breaks on Unix openers if
    /// the path is split into multiple arguments. The path is the last
    /// argument; what precedes it is what each arm has to get right.
    #[cfg(unix)]
    #[test]
    fn the_opener_is_handed_the_whole_path() {
        let path = Path::new("/tmp/fidget data/memory.md");
        let command = opener(path.as_os_str());

        assert_eq!(command.get_args().last(), Some(path.as_os_str()));
        #[cfg(target_os = "macos")]
        assert_eq!(command.get_program(), "open");
        #[cfg(all(unix, not(target_os = "macos")))]
        assert_eq!(command.get_program(), "xdg-open");
    }

    /// `#255`: `&` and `%` are `cmd` metacharacters. ShellExecuteW must see
    /// the literal path — including a space — as one wide string, not shell text.
    #[cfg(not(unix))]
    #[test]
    fn windows_opener_keeps_ampersand_percent_and_space() {
        let path = Path::new(r"C:\Users\a & b\100%\memory.md");
        let wide = shell_execute_file_wide(path.as_os_str());
        assert_eq!(
            wide.last().copied(),
            Some(0),
            "ShellExecuteW needs a trailing NUL"
        );
        let decoded = String::from_utf16(&wide[..wide.len() - 1]).expect("path is UTF-16");
        assert_eq!(
            decoded,
            path.to_str().expect("test path is UTF-8"),
            "the wide argument must be the path as written, not a cmd-escaped form"
        );
    }

    /// Live check that ShellExecuteW accepts a path with all three
    /// metacharacters. Opens the default `.md` handler briefly; the
    /// acceptance criterion is a success return, not which app appears.
    #[cfg(not(unix))]
    #[test]
    fn open_path_succeeds_for_metacharacter_path() {
        let root = std::env::temp_dir().join(format!(
            "fidget-open-meta-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let path = root.join("a & b").join("100%").join("memory.md");
        let _ = fs::remove_dir_all(&root);
        open_path(&path).expect("ShellExecuteW must open a path that holds &, %, and a space");
        assert!(path.is_file(), "ensure_file still creates Memory first");
        let _ = fs::remove_dir_all(&root);
    }

    /// A right-click on the overlay is the same miss as a left-click. Without
    /// this witness the webview's Inspect menu is the only thing that hears it.
    #[test]
    fn overlay_secondary_is_enough_for_a_press() {
        set_overlay_secondary(false);
        set_overlay_secondary(true);
        assert!(
            buttons_down().secondary,
            "a right-click the overlay felt must count as the button down"
        );
        set_overlay_secondary(false);
    }

    #[test]
    fn double_click_interval_passes_normal_values() {
        assert_eq!(resolve_double_click_interval(Some(400)), 400);
        assert_eq!(resolve_double_click_interval(Some(500)), 500);
        assert_eq!(resolve_double_click_interval(Some(200)), 200);
    }

    /// A pathologically small interval is clamped to prevent zero or near-zero
    /// windows that would make double-clicks impossible.
    #[test]
    fn double_click_interval_clamps_too_small() {
        assert_eq!(
            resolve_double_click_interval(Some(0)),
            FALLBACK_DOUBLE_CLICK_MS
        );
        assert_eq!(resolve_double_click_interval(Some(50)), MIN_DOUBLE_CLICK_MS);
        assert_eq!(resolve_double_click_interval(Some(99)), MIN_DOUBLE_CLICK_MS);
    }

    /// A pathologically large interval is clamped to prevent multi-day Summon
    /// windows. Win32 caps at 5000; we want a sane shared ceiling.
    #[test]
    fn double_click_interval_clamps_too_large() {
        assert_eq!(
            resolve_double_click_interval(Some(5000)),
            MAX_DOUBLE_CLICK_MS
        );
        assert_eq!(
            resolve_double_click_interval(Some(10000)),
            MAX_DOUBLE_CLICK_MS
        );
        assert_eq!(
            resolve_double_click_interval(Some(2001)),
            MAX_DOUBLE_CLICK_MS
        );
    }

    #[test]
    fn double_click_interval_falls_back_when_os_query_fails() {
        assert_eq!(
            resolve_double_click_interval(None),
            FALLBACK_DOUBLE_CLICK_MS
        );
    }

    #[test]
    fn public_double_click_interval_is_cached_and_resolved() {
        #[cfg(all(unix, not(target_os = "macos")))]
        let _ = gtk::init();

        let cached = double_click_interval_ms();
        assert!(
            (MIN_DOUBLE_CLICK_MS..=MAX_DOUBLE_CLICK_MS).contains(&cached),
            "public double_click_interval_ms must be in [{MIN_DOUBLE_CLICK_MS}, {MAX_DOUBLE_CLICK_MS}], got {cached}"
        );
        assert_eq!(
            double_click_interval_ms(),
            cached,
            "OnceLock must return the same value on a second call"
        );

        // Without a display, GTK init fails; do not touch Settings properties.
        // The OS reader returns None and the public API resolves to FALLBACK.
        #[cfg(all(unix, not(target_os = "macos")))]
        if !gtk::is_initialized() {
            assert_eq!(
                cached, FALLBACK_DOUBLE_CLICK_MS,
                "without GTK init, public double_click_interval_ms must use FALLBACK"
            );
            assert_eq!(
                cached,
                resolve_double_click_interval(None),
                "cached public value must equal resolve(None) when OS returns None"
            );
            return;
        }

        assert_eq!(
            cached,
            resolve_double_click_interval(os_double_click_interval_ms()),
            "cached public value must equal resolve of the raw OS read"
        );
    }

    /// MCP titles resource must gate on the window-names consent. Without wanted,
    /// no titles leak (even on X11 where _NET_WM_NAME is technically consent-free
    /// at the protocol level).
    #[test]
    #[cfg(not(target_os = "windows"))]
    fn mcp_window_titles_require_consent() {
        crate::consent::set_wanted(crate::consent::CapabilityId::WindowNames, false);
        let without_wanted = list_window_titles();
        assert!(
            without_wanted.is_empty(),
            "list_window_titles must return empty when WindowNames is not wanted"
        );
    }

    /// Windows list_window_titles must gate behind the window-names consent.
    /// Probe always grants (no system dialog), but wanted must still gate.
    #[test]
    #[cfg(target_os = "windows")]
    fn windows_mcp_window_titles_require_consent() {
        crate::consent::set_wanted(crate::consent::CapabilityId::WindowNames, false);
        let without_wanted = list_window_titles();
        assert!(
            without_wanted.is_empty(),
            "list_window_titles must return empty when WindowNames is not wanted"
        );

        crate::consent::set_wanted(crate::consent::CapabilityId::WindowNames, true);
        let with_wanted = list_window_titles();
        assert!(
            !with_wanted.is_empty() || cfg!(not(windows)),
            "list_window_titles should return titles when wanted (may be empty in test env)"
        );
    }
}
