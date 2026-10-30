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

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
#[cfg(unix)]
use std::process::Command;
#[cfg(not(target_os = "macos"))]
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use fidget_core::overlay_region::OverlayRect;
use fidget_core::quick_message::QmDraft;
use fidget_core::sensing::ActivitySource;
use fidget_core::window_source::{Rect, WindowSource};
use tauri::{Emitter, Manager};

/// One button as one source reports its edges: XI2 raw events on X11, the
/// overlay webview on Wayland and Windows. Two bits: a click can begin and end
/// between ticks, and the edge keeps it until a read consumes it. macOS counts
/// the session's button events instead, so it has no witness.
#[cfg(not(target_os = "macos"))]
struct Witness {
    /// What the source last reported: true from press to release.
    down: AtomicBool,
    /// Set on every press, cleared by the read that consumes it.
    pressed: AtomicBool,
}

#[cfg(not(target_os = "macos"))]
impl Witness {
    const fn new() -> Self {
        Self {
            down: AtomicBool::new(false),
            pressed: AtomicBool::new(false),
        }
    }

    /// The source saw the button go down or up.
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
    /// Returns whether it was holding anything.
    fn forget(&self) -> bool {
        self.down.swap(false, Ordering::SeqCst) | self.pressed.swap(false, Ordering::SeqCst)
    }
}

#[cfg(not(target_os = "macos"))]
static OVERLAY_PRIMARY: Witness = Witness::new();
#[cfg(not(target_os = "macos"))]
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

/// Each Instance's open quick-message draft, as its owning overlay last reported
/// it. A level, read every tick: the walk holds while one is here.
static QM_DRAFTS: Mutex<BTreeMap<String, QmDraft>> = Mutex::new(BTreeMap::new());

/// `None` is a closed pill.
pub fn set_qm_draft(instance: String, draft: Option<QmDraft>) {
    if let Ok(mut drafts) = QM_DRAFTS.lock() {
        match draft {
            Some(draft) => drafts.insert(instance, draft),
            None => drafts.remove(&instance),
        };
    }
}

pub fn qm_draft(instance: &str) -> Option<QmDraft> {
    QM_DRAFTS
        .lock()
        .ok()
        .and_then(|drafts| drafts.get(instance).cloned())
}

/// Which mouse buttons one tick found down. One type so both consuming reads
/// live in one place.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ButtonsDown {
    pub primary: bool,
    pub secondary: bool,
}

/// Milliseconds since the Unix epoch, so `FIDGET_CLICK_TRACE` lines from the
/// frame loop, the overlay commands and a test script's markers sort together.
pub fn trace_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |since| since.as_secs_f64() * 1000.0)
}

/// A `FIDGET_CLICK_TRACE` line for one button edge `source` saw.
fn trace_edge(source: &str, button: &str, down: bool) {
    if crate::dev_flags::TRACE_CLICKS.is_on() {
        let edge = if down { "down" } else { "up" };
        eprintln!(
            "click-trace: {source} t={:.1} button={button} edge={edge}",
            trace_ms()
        );
    }
}

/// The overlay heard the primary button go down or up. On macOS that is not
/// a press source, only the earliest moment to read Control.
pub fn set_overlay_primary(down: bool) {
    trace_edge("witness", "left", down);
    #[cfg(not(target_os = "macos"))]
    OVERLAY_PRIMARY.report(down);
    #[cfg(target_os = "macos")]
    if down {
        macos::primary_press_began();
    }
}

/// What each overlay draws outside the art, in its own coordinates. A `Vec`
/// because `Vec::new` is const.
static OVERLAY_RECTS: Mutex<Vec<(String, OverlayRect)>> = Mutex::new(Vec::new());

/// Where the window manager really put each overlay, in points. X11 only, from
/// move events: xfwm4 keeps an ordinary window out of a panel's reserved strip.
/// Empty elsewhere, so every overlay is drawn from the frame it asked for.
static OVERLAY_ORIGINS: Mutex<Vec<(String, (f64, f64))>> = Mutex::new(Vec::new());

/// The window manager reported where `label`'s overlay now is.
#[cfg(all(unix, not(target_os = "macos")))]
pub fn set_overlay_origin(label: &str, origin: (f64, f64)) {
    let Ok(mut origins) = OVERLAY_ORIGINS.lock() else {
        return;
    };
    origins.retain(|(owner, _)| owner != label);
    origins.push((label.to_string(), origin));
}

/// Where `label`'s overlay really is, if the window manager has said.
pub fn overlay_origin(label: &str) -> Option<(f64, f64)> {
    OVERLAY_ORIGINS.lock().ok().and_then(|origins| {
        origins
            .iter()
            .find(|(owner, _)| owner == label)
            .map(|(_, origin)| *origin)
    })
}

/// Replace everything `label` reported. An empty list is how an overlay says
/// it draws nothing but the art again.
pub fn set_overlay_rects(label: &str, rects: Vec<OverlayRect>) {
    let Ok(mut all) = OVERLAY_RECTS.lock() else {
        return;
    };
    all.retain(|(owner, _)| owner != label);
    all.extend(rects.into_iter().map(|rect| (label.to_string(), rect)));
}

/// Whether `label`'s overlay wants the click at `(x, y)`, in its coordinates.
pub fn over_overlay_hotspot(label: &str, x: i32, y: i32) -> bool {
    OVERLAY_RECTS.lock().is_ok_and(|all| {
        all.iter()
            .any(|(owner, rect)| owner == label && rect.takes_click_at(x, y))
    })
}

/// The rectangles one overlay reported, in its own coordinates.
/// Not `cfg`'d out on macOS: the neighbour-isolation test lives here, and
/// compiling it only on X11/Windows would skip it on the machine this is written on.
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub fn overlay_rects_for(label: &str) -> Vec<OverlayRect> {
    OVERLAY_RECTS.lock().map_or_else(
        |_| Vec::new(),
        |all| {
            all.iter()
                .filter(|(owner, _)| owner == label)
                .map(|(_, rect)| rect.clone())
                .collect()
        },
    )
}

/// The overlay heard the secondary button go down or up.
pub fn set_overlay_secondary(down: bool) {
    trace_edge("witness", "right", down);
    #[cfg(not(target_os = "macos"))]
    OVERLAY_SECONDARY.report(down);
}

/// The overlay is passing clicks through, so it cannot still be holding a
/// press. Drops both bits; the session sources answer for themselves in
/// `buttons_down`.
#[cfg(not(target_os = "macos"))]
pub fn overlay_passes_clicks_through() {
    for (button, witness) in [("left", &OVERLAY_PRIMARY), ("right", &OVERLAY_SECONDARY)] {
        let held = witness.forget();
        if held && crate::dev_flags::TRACE_CLICKS.is_on() {
            eprintln!(
                "click-trace: witness t={:.1} button={button} edge=forget",
                trace_ms()
            );
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn overlay_buttons_down() -> ButtonsDown {
    ButtonsDown {
        primary: OVERLAY_PRIMARY.take(),
        secondary: OVERLAY_SECONDARY.take(),
    }
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
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
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
/// if so. The refresh itself stays at each call site: macOS posts to the main
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

#[cfg(any(test, not(unix)))]
mod windows_hand_back;

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
/// clients; under XWayland the EWMH/XShape path works.
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

/// Remember the frontmost app before a pill takes the keyboard. Main thread.
#[cfg(target_os = "macos")]
pub fn remember_front_app() {
    macos::remember_front_app();
}

/// The overlay's last pill closed: hand the keyboard back. Main thread.
#[cfg(target_os = "macos")]
pub fn hand_back_keyboard(window: &tauri::WebviewWindow) -> Result<(), String> {
    macos::hand_back_keyboard(window)
}

/// Windows: give the keyboard back when the last pill closes.
#[cfg(target_os = "windows")]
pub fn hand_back_keyboard(window: &tauri::WebviewWindow) -> Result<(), String> {
    let hwnd = window
        .hwnd()
        .map_err(|e| format!("window has no native handle: {e}"))?
        .0 as windows_sys::Win32::Foundation::HWND;
    windows::hand_back_keyboard(hwnd)
}

/// The X11 window behind a webview, or None on Wayland.
#[cfg(all(unix, not(target_os = "macos")))]
fn x11_window(window: &impl raw_window_handle::HasWindowHandle) -> Option<u32> {
    use raw_window_handle::RawWindowHandle;
    match window.window_handle().ok()?.as_raw() {
        RawWindowHandle::Xlib(xlib) => Some(xlib.window as u32),
        RawWindowHandle::Xcb(xcb) => Some(xcb.window.get()),
        _ => None,
    }
}

/// The overlay refuses focus until a pill opens and again once the last one
/// closes: macOS keeps the panel from becoming key, X11 clears accept-focus
/// (#1699). Windows hands the foreground over itself; Wayland keeps GTK's
/// default, since its compositor decides focus.
#[cfg(target_os = "macos")]
pub fn overlay_focus_follows_pill() -> bool {
    true
}

/// See the macOS doc. True on an X11 display, read once.
#[cfg(all(unix, not(target_os = "macos")))]
pub fn overlay_focus_follows_pill() -> bool {
    use gtk::prelude::*;
    static ON_X11: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON_X11.get_or_init(|| {
        gtk::gdk::Display::default()
            .is_some_and(|display| display.type_().name() == "GdkX11Display")
    })
}

/// See the macOS doc.
#[cfg(windows)]
pub fn overlay_focus_follows_pill() -> bool {
    false
}

/// Remember the active window before a pill takes the keyboard, unless it
/// is one of Fidget's overlays. Wayland has no X window to remember.
#[cfg(all(unix, not(target_os = "macos")))]
pub fn remember_foreground(window: &tauri::Window) {
    let Some(caller) = x11_window(window) else {
        return;
    };
    let mut overlays: Vec<u32> = window
        .app_handle()
        .webview_windows()
        .into_iter()
        .filter(|(label, _)| crate::is_overlay_label(label))
        .filter_map(|(_, overlay)| x11_window(&overlay))
        .collect();
    // The caller is an overlay even when its label was not listed.
    if !overlays.contains(&caller) {
        overlays.push(caller);
    }
    x11::remember_active_window(&overlays);
}

/// Remember the foreground window before a pill takes the keyboard, unless
/// it is one of these overlays.
#[cfg(target_os = "windows")]
pub fn remember_foreground(
    foreground: windows_sys::Win32::Foundation::HWND,
    overlays: &[windows_sys::Win32::Foundation::HWND],
) {
    windows::remember_foreground(foreground, overlays);
}

/// Last pill closed: on X11, if the overlay still has focus, activate the window
/// that was active before, or else the topmost one that isn't Fidget's. Wayland
/// lets no client activate another's surface, so the compositor decides there.
#[cfg(all(unix, not(target_os = "macos")))]
pub fn hand_back_keyboard(window: &tauri::WebviewWindow) -> Result<(), String> {
    match x11_window(window) {
        Some(overlay) => x11::return_keyboard(overlay),
        None => Ok(()),
    }
}

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

    // SAFETY: `ptr` is the live NSWindow from Tauri on the main thread; msg_send targets are valid.
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

/// Give `window` the keyboard at the normal window level. Main thread only.
/// The overlay stays above so Chat's ledge stays a Perch.
#[cfg(target_os = "macos")]
pub fn activate_for_typing(window: &tauri::WebviewWindow) -> Result<(), String> {
    use objc2::msg_send;
    use objc2_app_kit::{NSApplication, NSNormalWindowLevel, NSWindowLevel};
    use objc2_foundation::MainThreadMarker;

    let ptr = window
        .ns_window()
        .map_err(|e| format!("window has no native handle: {e}"))?
        as *mut objc2::runtime::AnyObject;

    // SAFETY: `ptr` is the live NSWindow from Tauri on the main thread; msg_send targets are valid.
    unsafe {
        let ns_window = &*ptr;
        let mtm = MainThreadMarker::new_unchecked();
        let app = NSApplication::sharedApplication(mtm);

        let _: () = msg_send![ns_window, setLevel: NSNormalWindowLevel as NSWindowLevel];
        let _: () = msg_send![ns_window, setHidesOnDeactivate: false];
        #[allow(deprecated)]
        app.activateIgnoringOtherApps(true);
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
/// The overlay is `_NET_WM_STATE_ABOVE`. keep_above shares that band.
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

/// Give `window` the keyboard without `_NET_WM_STATE_ABOVE`. Main thread only.
/// keep_above is layer 1, which `perch_eligible` drops, so Chat would stop
/// being a Perch while focused.
#[cfg(all(unix, not(target_os = "macos")))]
pub fn activate_for_typing(window: &tauri::WebviewWindow) -> Result<(), String> {
    use gtk::prelude::*;

    let gtk_window = window
        .gtk_window()
        .map_err(|e| format!("window has no gtk handle: {e}"))?;
    gtk_window.set_keep_above(false);
    gtk_window.present();
    Ok(())
}

/// Raise `window` above the overlay, and give it the keyboard. Main thread only.
///
/// The overlay is HWND_TOPMOST. A normal window cannot stack above that band.
#[cfg(not(unix))]
pub fn raise_above_overlay(window: &tauri::WebviewWindow) -> Result<(), String> {
    windows::raise_above_overlay(window)
}

/// Give `window` the keyboard without entering the topmost band. Main thread only.
/// Topmost Chat stacks above the overlay, so the sprite paints behind the ledge.
#[cfg(not(unix))]
pub fn activate_for_typing(window: &tauri::WebviewWindow) -> Result<(), String> {
    windows::activate_for_typing(window)
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

/// Hand a file the user owns to its extension's app, else the text editor.
/// Created empty first: Memory has no file until the Director remembers, and
/// an opener given a missing path reports it missing.
pub fn open_path(path: &Path) -> Result<(), String> {
    ensure_file(path)?;
    #[cfg(unix)]
    {
        let [mut first, mut fallback] = file_openers(path.as_os_str());
        let mut child = first.spawn().map_err(|error| error.to_string())?;
        // The exit status is the only "no app" signal, and a menu click runs on
        // the main thread, so the wait happens off it.
        let path = path.to_path_buf();
        std::thread::spawn(move || {
            if !child.wait().is_ok_and(|status| status.success())
                && !fallback.status().is_ok_and(|status| status.success())
            {
                eprintln!("no app opened {}", path.display());
            }
        });
        Ok(())
    }
    #[cfg(not(unix))]
    {
        open_file(path.as_os_str())
    }
}

/// The schemes a link in a reply is allowed to open. `mailto` belongs: a
/// model writing an address as a link means it to be mailed. Everything else
/// is refused; the asker is untrusted text.
const OPENABLE: [&str; 3] = ["http", "https", "mailto"];

/// Hand a URL from a reply to whatever the desktop opens it with.
/// Last-edge gate: parse, don't prefix-match (`&#106;avascript:`). Open
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

/// The one per-platform call for `open_url`; files go through `open_path`.
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

/// The extension's app, then the text editor; the second runs only when the
/// first exits non-zero.
#[cfg(target_os = "macos")]
fn file_openers(path: &std::ffi::OsStr) -> [Command; 2] {
    let mut text_editor = Command::new("open");
    text_editor.arg("-t").arg(path);
    [opener(path), text_editor]
}

/// The path rides in as `$1`, so the script never holds its text.
/// ponytail: some desktops' xdg-open exits 0 without opening anything, which
/// skips this fallback, and `gtk-launch` ships in an optional package
/// (`libgtk-3-bin`). Resolve the `.desktop` file ourselves if either bites.
#[cfg(all(unix, not(target_os = "macos")))]
fn file_openers(path: &std::ffi::OsStr) -> [Command; 2] {
    let mut text_editor = Command::new("sh");
    text_editor
        .args([
            "-c",
            r#"gtk-launch "$(xdg-mime query default text/plain)" "$1""#,
            "sh",
        ])
        .arg(path);
    [opener(path), text_editor]
}

/// Open with the default application via ShellExecuteW. Not `cmd /C start`: `&`
/// and `%` are syntax there. Neither `open` nor `opener` removes the `unsafe`,
/// so a crate would not change what can go wrong.
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

/// `opener`, then the `.txt` app. Any failure retries, not just
/// `SE_ERR_NOASSOC`: each one leaves the user with nothing open.
#[cfg(not(unix))]
fn open_file(target: &std::ffi::OsStr) -> Result<(), String> {
    opener(target).or_else(|why| {
        if open_as_text(target) {
            Ok(())
        } else {
            Err(why)
        }
    })
}

/// ShellExecuteExW with `lpClass` ".txt" opens the file with the `.txt` verb,
/// so no editor path or command line is built here. The struct's `HKEY` field
/// is why Cargo.toml enables `Win32_System_Registry`.
#[cfg(not(unix))]
fn open_as_text(target: &std::ffi::OsStr) -> bool {
    use windows_sys::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_CLASSNAME, SHELLEXECUTEINFOW};
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let file = shell_execute_file_wide(target);
    let operation: Vec<u16> = "open".encode_utf16().chain(Some(0)).collect();
    let class: Vec<u16> = ".txt".encode_utf16().chain(Some(0)).collect();
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_CLASSNAME,
        lpVerb: operation.as_ptr(),
        lpFile: file.as_ptr(),
        lpClass: class.as_ptr(),
        nShow: SW_SHOWNORMAL,
        ..Default::default()
    };
    // SAFETY: `cbSize` matches the struct, and the three wide strings are
    // NUL-terminated and outlive the call. Unset fields are null by default.
    unsafe { ShellExecuteExW(&mut info) != 0 }
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

/// Clean up X11 pixmap cache for a destroyed overlay window.
#[cfg(all(unix, not(target_os = "macos")))]
pub fn cleanup_overlay_window(window: &tauri::WebviewWindow) {
    x11::cleanup_overlay_window(window)
}

/// Whether a compositor blends the overlays' transparent pixels. Without one
/// X11 fills them with black, so the overlays are clipped to what they draw.
#[cfg(all(unix, not(target_os = "macos")))]
pub fn overlays_composited() -> bool {
    x11::composited()
}

/// Whether Chat and Settings draw right in a see-through window.
/// Main thread: the first call reads GDK's answer.
#[cfg(all(unix, not(target_os = "macos")))]
pub fn see_through_draws() -> bool {
    see_through_draws_with(x11::composited_now(), webkit_version(), |name| {
        std::env::var_os(name)
    })
}

/// Uncomposited, X11 shows alpha as black. WebKitGTK 2.54's non-accelerated
/// renderer, which WebKit's own variables pick, repaints only damage and leaves
/// the rest see-through. An unknown version counts as 2.54.
#[cfg(all(unix, not(target_os = "macos")))]
fn see_through_draws_with(
    composited: bool,
    webkit: Option<(u32, u32)>,
    var: impl Fn(&str) -> Option<std::ffi::OsString>,
) -> bool {
    // As WebKit reads them: any DMABuf value but "0", a non-empty compositing one.
    let unaccelerated = var("WEBKIT_DISABLE_DMABUF_RENDERER").is_some_and(|v| v != "0")
        || var("WEBKIT_DISABLE_COMPOSITING_MODE").is_some_and(|v| !v.is_empty() && v != "0");
    composited && !(unaccelerated && webkit.is_none_or(|version| version >= (2, 54)))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn webkit_version() -> Option<(u32, u32)> {
    let version = tauri::webview_version().ok()?;
    let mut parts = version.split('.').map(str::parse);
    Some((parts.next()?.ok()?, parts.next()?.ok()?))
}

/// Windows and macOS always composite.
#[cfg(any(target_os = "macos", not(unix)))]
pub fn see_through_draws() -> bool {
    true
}

/// Send `label`'s bounding shape change; false when its window is not set up yet.
#[cfg(all(unix, not(target_os = "macos")))]
pub fn change_bounding_shape(
    label: &str,
    change: fidget_core::overlay_region::ShapeChange<'_>,
) -> bool {
    x11::change_bounding_shape(label, change)
}

/// Set the input region from every sprite's alpha mask plus hotspot rects, so
/// a control drawn outside the art still receives clicks. macOS uses
/// `set_ignore_cursor_events`; `click_through` matters only on Windows.
#[cfg(all(unix, not(target_os = "macos")))]
pub fn update_input_region(
    window: &tauri::WebviewWindow,
    sprites: &[SpriteMask<'_>],
    hotspot_rects: &[[i32; 4]],
    _click_through: bool,
) -> Result<(), String> {
    x11::update_input_region(window, sprites, hotspot_rects)
}

#[cfg(all(unix, not(target_os = "macos")))]
pub use x11::SpriteMask;

/// Windows: SetWindowRgn from the sprite's swept ink and the renderer's rects.
/// The region clips drawing as well as input. `click_through` controls
/// WS_EX_TRANSPARENT; returns whether the window now passes clicks.
#[cfg(not(unix))]
pub fn update_input_region(
    window: &tauri::WebviewWindow,
    art: Option<&[[i32; 4]]>,
    rects: &[OverlayRect],
    click_through: bool,
) -> Result<bool, String> {
    windows::update_input_region(window, art, rects, click_through)
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

/// Whether this lane honours the off-art rectangles the renderer reports. macOS
/// hit-tests via boolean click-through; X11 and Windows union them into the
/// input region, so a control above the head takes clicks on every platform.
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

/// Clamp the double-click interval, or fall back. Split from the cached
/// `double_click_interval_ms` so tests can call it.
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

/// Which mouse buttons are down, or were pressed since the last call, from
/// the session's button event counters alone; a Control-click is secondary.
/// The frame loop asks once per tick, so "since the last call" means "since
/// the last tick".
#[cfg(target_os = "macos")]
pub fn buttons_down() -> ButtonsDown {
    macos::buttons_down()
}

/// Start noting which presses AppKit has delivered. Main thread, once.
#[cfg(target_os = "macos")]
pub fn watch_delivered_presses() {
    macos::watch_delivered_presses();
}

/// Dispatch the press a menu is about to open for, if AppKit still holds it.
/// Main thread.
#[cfg(target_os = "macos")]
pub fn deliver_pending_press() {
    macos::deliver_pending_press();
}

/// X11 on Linux: XI2's raw button edges alone. Wayland has no global pointer,
/// so there, and on X11 without XI2, only the overlay latch. The latch is read
/// every call, so if XI2 stops, its fallback holds no press from before.
#[cfg(all(unix, not(target_os = "macos")))]
pub fn buttons_down() -> ButtonsDown {
    let overlay = overlay_buttons_down();
    x11::buttons_down().unwrap_or(overlay)
}

/// Where the latest press began, in the cursor's pixels, when the source
/// knows: XI2 on X11. A press dragged away before the tick is hit-tested here,
/// not where the tick finds the cursor.
#[cfg(all(unix, not(target_os = "macos")))]
pub fn press_point() -> Option<(i32, i32)> {
    x11::press_point()
}

/// Elsewhere the tick's cursor stands in for where the press began.
#[cfg(not(all(unix, not(target_os = "macos"))))]
pub fn press_point() -> Option<(i32, i32)> {
    None
}

/// Windows: GetAsyncKeyState for both buttons, or the overlay latch. Both
/// reads run every call, so each latch is consumed exactly once.
#[cfg(not(unix))]
pub fn buttons_down() -> ButtonsDown {
    let session = windows::buttons_down();
    let overlay = overlay_buttons_down();
    ButtonsDown {
        primary: overlay.primary || session.primary,
        secondary: overlay.secondary || session.secondary,
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

/// Spawn the XI2 listener thread on X11, which feeds `buttons_down` and wakes
/// the idle frame loop. Returns `None` when XI2 is unavailable.
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

/// Spawn the macOS mouse event tap, or `None` until the Input Monitoring row is
/// checked and granted. The frame loop asks again on later idle waits, so a
/// mid-run grant is picked up. Dropping the value stops the tap thread.
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

                cache.read().usable_frames
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

    /// Production change that would fail this: Chat or Settings built
    /// see-through with no X11 compositor, so Chat shows black (#1732).
    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn chat_and_settings_are_see_through_only_where_a_compositor_blends() {
        let clean = |_: &str| None;
        x11::set_composited_for_test(false);
        let opaque = see_through_draws_with(x11::composited_now(), Some((2, 54)), clean);
        assert!(!opaque, "no compositor builds them opaque");
        x11::set_composited_for_test(true);
        let see_through = see_through_draws_with(x11::composited_now(), Some((2, 54)), clean);
        assert!(see_through, "a compositor keeps them see-through");
    }

    /// Production change that would fail this: Chat built see-through while
    /// WebKitGTK 2.54's non-accelerated renderer draws it, so the compositor
    /// shows the desktop through every pixel it didn't repaint.
    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn chat_and_settings_are_opaque_where_webkit_repaints_only_damage() {
        const DMABUF: &str = "WEBKIT_DISABLE_DMABUF_RENDERER";
        const COMPOSITING: &str = "WEBKIT_DISABLE_COMPOSITING_MODE";
        fn check(webkit: Option<(u32, u32)>, vars: &[(&str, &str)], want: bool) {
            let var = |name: &str| {
                let found = vars.iter().find(|(set, _)| *set == name);
                found.map(|(_, value)| std::ffi::OsString::from(value))
            };
            let got = see_through_draws_with(true, webkit, var);
            assert_eq!(got, want, "WebKitGTK {webkit:?} with {vars:?}");
        }
        check(Some((2, 54)), &[], true);
        check(Some((2, 54)), &[(DMABUF, "1")], false);
        check(Some((2, 54)), &[(DMABUF, "")], false);
        check(Some((2, 54)), &[(DMABUF, "0")], true);
        check(Some((2, 54)), &[(COMPOSITING, "1")], false);
        check(Some((2, 54)), &[(COMPOSITING, "")], true);
        check(Some((2, 54)), &[(COMPOSITING, "0")], true);
        check(Some((3, 0)), &[(DMABUF, "1")], false);
        check(Some((2, 52)), &[(DMABUF, "1"), (COMPOSITING, "1")], true);
        check(Some((2, 50)), &[(DMABUF, "1"), (COMPOSITING, "1")], true);
        check(None, &[(DMABUF, "1")], false);
        check(None, &[], true);
    }

    /// Off X11 the window manager always composites, so nothing changes there.
    #[cfg(any(target_os = "macos", not(unix)))]
    #[test]
    fn windows_and_macos_always_draw_a_see_through_window() {
        assert!(see_through_draws());
    }

    /// The frame loop copies this slot into each Instance every tick, so a
    /// draft reaches only its own Instance and a cleared one is gone.
    #[test]
    fn each_instance_reads_only_its_own_draft() {
        let draft = QmDraft {
            text: "wait".to_string(),
            focused: true,
        };
        set_qm_draft("drafting".to_string(), Some(draft.clone()));
        assert_eq!(qm_draft("drafting"), Some(draft));
        assert_eq!(qm_draft("neighbour"), None, "a neighbour carries nothing");

        set_qm_draft("drafting".to_string(), None);
        assert_eq!(qm_draft("drafting"), None, "a closed pill leaves no draft");
    }

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

    /// On Wayland and Windows the overlay's own pointer events are a source
    /// of `buttons_down`. Read directly: on X11 the XI2 listener answers
    /// instead, and an Xvfb test in this process may have started one.
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn overlay_primary_is_enough_for_a_press() {
        set_overlay_primary(false);
        set_overlay_primary(true);
        assert!(
            overlay_buttons_down().primary,
            "a click the overlay felt must count as the button down"
        );
        set_overlay_primary(false);
    }

    #[test]
    fn composing_names_one_instance_until_the_caret_leaves() {
        set_overlay_composing(Some("buddy-a".to_string()));
        assert_eq!(overlay_composing().as_deref(), Some("buddy-a"));
        set_overlay_composing(Some(String::new()));
        assert_eq!(overlay_composing(), None);
    }

    /// What an overlay reports belongs to it. A neighbour must not stop
    /// passing clicks at the same coordinates or draw the same rects, and a
    /// gone bubble must take its rectangles with it.
    #[test]
    fn overlay_rects_belong_to_one_overlay_and_go_when_it_clears_them() {
        let open_chat = OverlayRect {
            rect: [10, 20, 30, 40],
            clickable: true,
            follows: None,
        };
        let bubble = OverlayRect {
            rect: [0, 0, 100, 80],
            clickable: false,
            follows: None,
        };
        set_overlay_rects("overlay-test-a", vec![open_chat.clone(), bubble.clone()]);
        set_overlay_rects("overlay-test-b", vec![bubble.clone()]);

        assert_eq!(
            overlay_rects_for("overlay-test-a"),
            vec![open_chat, bubble.clone()]
        );
        assert_eq!(overlay_rects_for("overlay-test-b"), vec![bubble]);
        assert!(overlay_rects_for("overlay-test-none").is_empty());
        assert!(over_overlay_hotspot("overlay-test-a", 20, 30), "Open chat");
        assert!(
            !over_overlay_hotspot("overlay-test-b", 20, 30),
            "the neighbour's bubble is only drawn"
        );

        set_overlay_rects("overlay-test-a", vec![]);
        assert!(overlay_rects_for("overlay-test-a").is_empty());
        assert!(
            !over_overlay_hotspot("overlay-test-a", 20, 30),
            "the bubble is gone and so is its rectangle"
        );

        set_overlay_rects("overlay-test-b", vec![]);
    }

    /// A click can begin and end between two ticks. The level alone reads
    /// false at both; the edge keeps the down until it has been read once.
    #[cfg(not(target_os = "macos"))]
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
    #[cfg(not(target_os = "macos"))]
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
    #[cfg(not(target_os = "macos"))]
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

    /// No app owns `.jsonl` on a stock Mac, so the text editor is the second
    /// try. `&`, `%` and a space stay inside the one path argument.
    #[cfg(unix)]
    #[test]
    fn a_file_falls_back_to_the_text_editor_with_the_whole_path() {
        let path = Path::new("/tmp/a & b/100%/action-log.jsonl");
        let [first, fallback] = file_openers(path.as_os_str());
        let argv = |command: &Command| {
            std::iter::once(command.get_program())
                .chain(command.get_args())
                .map(|arg| arg.to_str().expect("test argv is UTF-8").to_string())
                .collect::<Vec<_>>()
        };
        let path = path.to_str().expect("test path is UTF-8");

        #[cfg(target_os = "macos")]
        {
            assert_eq!(argv(&first), ["open", path]);
            assert_eq!(argv(&fallback), ["open", "-t", path]);
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            assert_eq!(argv(&first), ["xdg-open", path]);
            assert_eq!(argv(&fallback)[..2], ["sh", "-c"]);
            assert_eq!(
                argv(&fallback)[3..],
                ["sh", path],
                "the path is $1, never script text"
            );
        }
    }

    /// `&` and `%` are `cmd` metacharacters. ShellExecuteW must see the literal
    /// path — including a space — as one wide string, not shell text.
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

    /// A right-click on the overlay counts the same way as a left-click.
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn overlay_secondary_is_enough_for_a_press() {
        set_overlay_secondary(false);
        set_overlay_secondary(true);
        assert!(
            overlay_buttons_down().secondary,
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
