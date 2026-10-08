//! X11 window geometry from the window manager, consent-free.
//!
//! Reads _NET_CLIENT_LIST for the window list, XGetWindowAttributes for geometry,
//! _NET_FRAME_EXTENTS for decorations, and WM_CLASS for the owner. All EWMH and ICCCM
//! properties that require no consent, exactly like macOS's CGWindowListCopyWindowInfo.

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{self, AtomEnum, Window};
use x11rb::rust_connection::RustConnection;

use fidget_core::window_source::{Capabilities, Rect, WindowRect, WindowSource, WorldGeometry};

use crate::mcp_resources::WindowTitle;

/// The X11 window manager's view of the desktop.
pub struct X11WindowSource {
    /// Where the usable part of each display comes from: the work area the
    /// window manager reports per display. A desktop panel reaches the Engine
    /// only through it, as the Windows taskbar does: a bottom one raises the
    /// floor, a side one is a wall, a top one lowers the ceiling.
    read_displays: Box<dyn Fn() -> Vec<Rect> + Send + Sync>,
    can_read_titles: Box<dyn Fn() -> bool + Send + Sync>,
}

impl X11WindowSource {
    pub fn new(
        read_displays: impl Fn() -> Vec<Rect> + Send + Sync + 'static,
        can_read_titles: impl Fn() -> bool + Send + Sync + 'static,
    ) -> Self {
        Self {
            read_displays: Box::new(read_displays),
            can_read_titles: Box::new(can_read_titles),
        }
    }
}

impl WindowSource for X11WindowSource {
    /// `window_geometry` stays true under XWayland, where the list is partial.
    /// The rectangles are real; a missing client costs one Perch. Declaring
    /// false, or reading `WAYLAND_DISPLAY` again, is the test #266 removed.
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            window_geometry: true,
            absolute_positioning: true,
        }
    }

    fn read(&self) -> WorldGeometry {
        let can_read_titles = (self.can_read_titles)();
        WorldGeometry {
            usable_frames: (self.read_displays)(),
            windows: visible_windows(can_read_titles),
            // No Dock: a desktop panel is only the work area it reserves.
            dock: None,
        }
    }
}

/// Visible windows, frontmost first.
/// `_NET_CLIENT_LIST_STACKING` is bottom-to-top, so reverse it. Fall back to
/// `_NET_CLIENT_LIST` when stacking is missing. Own windows stay; `window_rect` sets the overlay's level.
fn visible_windows(can_read_names: bool) -> Vec<WindowRect> {
    use std::sync::atomic::{AtomicBool, Ordering};
    static LOGGED: AtomicBool = AtomicBool::new(false);

    let Some(conn) = super::connection::connection() else {
        return Vec::new();
    };
    let screen = &conn.setup().roots[0];
    let root = screen.root;

    let windows = window_list_stacking(conn, root)
        .filter(|list| !list.is_empty())
        .or_else(|| window_list(conn, root))
        .unwrap_or_default();

    let result: Vec<WindowRect> = windows
        .into_iter()
        .rev()
        .filter_map(|w| window_rect(conn, w, can_read_names))
        .collect();

    if !LOGGED.swap(true, Ordering::Relaxed) {
        eprintln!("window_source: {} visible windows", result.len());
    }

    result
}

/// Owner plus title, same walk and order as `visible_windows`. `_NET_WM_NAME`
/// first, `WM_NAME` if the EWMH name is missing.
pub fn visible_window_titles() -> Vec<WindowTitle> {
    // Named: this walk runs only once the consent is usable, and the resource
    // reports the owner beside the title. No connection means no windows,
    // which `visible_windows` already answers with an empty list.
    visible_windows(true)
        .into_iter()
        .map(|window| WindowTitle {
            title: window.title.unwrap_or_default(),
            owner: window.owner.unwrap_or_default(),
        })
        .collect()
}

pub(super) fn window_title(conn: &RustConnection, window: Window) -> String {
    if let Some(atoms) = super::atoms::atoms() {
        if let Some(name) = property_text(conn, window, atoms.net_wm_name, atoms.utf8_string) {
            return name;
        }
    }
    property_text(conn, window, AtomEnum::WM_NAME, AtomEnum::STRING).unwrap_or_default()
}

fn property_text(
    conn: &RustConnection,
    window: Window,
    property: impl Into<xproto::Atom>,
    type_: impl Into<xproto::Atom>,
) -> Option<String> {
    let reply = xproto::get_property(conn, false, window, property, type_, 0, 1024)
        .ok()?
        .reply()
        .ok()?;
    if reply.format != 8 || reply.value.is_empty() {
        return None;
    }
    let text = String::from_utf8_lossy(&reply.value)
        .trim_end_matches('\0')
        .to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

/// Read _NET_CLIENT_LIST_STACKING: windows in stacking order, bottom to top.
fn window_list_stacking(conn: &RustConnection, root: Window) -> Option<Vec<Window>> {
    let stacking_atom = super::atoms::atoms()?.net_client_list_stacking;
    let reply = xproto::get_property(
        conn,
        false,
        root,
        stacking_atom,
        AtomEnum::WINDOW,
        0,
        u32::MAX,
    )
    .ok()?
    .reply()
    .ok()?;

    if reply.format != 32 || reply.value.len() % 4 != 0 {
        return None;
    }

    Some(
        reply
            .value
            .as_chunks::<4>()
            .0
            .iter()
            .map(|chunk| u32::from_ne_bytes(*chunk))
            .collect(),
    )
}

/// Read `_NET_CLIENT_LIST`: windows in arbitrary order.
/// Fallback when STACKING is missing. Order is undefined, so occlusion may
/// be wrong, but some Perches are better than none.
fn window_list(conn: &RustConnection, root: Window) -> Option<Vec<Window>> {
    let list_atom = super::atoms::atoms()?.net_client_list;
    let reply = xproto::get_property(conn, false, root, list_atom, AtomEnum::WINDOW, 0, u32::MAX)
        .ok()?
        .reply()
        .ok()?;

    if reply.format != 32 || reply.value.len() % 4 != 0 {
        return None;
    }

    Some(
        reply
            .value
            .as_chunks::<4>()
            .0
            .iter()
            .map(|chunk| u32::from_ne_bytes(*chunk))
            .collect(),
    )
}

fn window_rect(conn: &RustConnection, window: Window, can_read_names: bool) -> Option<WindowRect> {
    if !is_normal_window(conn, window) {
        return None;
    }

    // One consent covers the class and the title alike (ADR-0032). X11 hands
    // both over for free; withholding them is this project's choice.
    let owner =
        can_read_names.then(|| window_class(conn, window).unwrap_or_else(|| "Unknown".to_string()));
    let title = if can_read_names {
        let title_str = window_title(conn, window);
        if title_str.is_empty() {
            None
        } else {
            Some(title_str)
        }
    } else {
        None
    };

    let geom = xproto::get_geometry(conn, window).ok()?.reply().ok()?;
    let translated = xproto::translate_coordinates(conn, window, geom.root, 0, 0)
        .ok()?
        .reply()
        .ok()?;

    let (x, y, width, height) = frame_geometry(
        conn,
        window,
        translated.dst_x,
        translated.dst_y,
        geom.width,
        geom.height,
    );

    Some(WindowRect {
        id: u64::from(window),
        bounds: Rect {
            x: f64::from(x),
            y: f64::from(y),
            width: f64::from(width),
            height: f64::from(height),
        },
        owner,
        title,
        layer: i32::from(is_above(conn, window)),
    })
}

/// Whether the window manager keeps this window above ordinary ones.
/// `_NET_WM_STATE_ABOVE` is written on the overlay itself. WM_CLASS exclusion
/// covered every GTK window, Chat (#362) and Settings included.
fn is_above(conn: &RustConnection, window: Window) -> bool {
    let Some(atoms) = super::atoms::atoms() else {
        return false;
    };

    let Some(reply) = xproto::get_property(
        conn,
        false,
        window,
        atoms.net_wm_state,
        AtomEnum::ATOM,
        0,
        32,
    )
    .ok()
    .and_then(|cookie| cookie.reply().ok()) else {
        return false;
    };

    reply.format == 32
        && reply
            .value
            .as_chunks::<4>()
            .0
            .iter()
            .any(|chunk| u32::from_ne_bytes(*chunk) == atoms.net_wm_state_above)
}

/// Skip docks, desktops, and menus: `_NET_WM_WINDOW_TYPE_NORMAL` only.
/// Missing type is treated as normal — older windows omit it.
fn is_normal_window(conn: &RustConnection, window: Window) -> bool {
    let Some(atoms) = super::atoms::atoms() else {
        return false;
    };

    let reply = match xproto::get_property(
        conn,
        false,
        window,
        atoms.net_wm_window_type,
        AtomEnum::ATOM,
        0,
        32,
    )
    .ok()
    .and_then(|cookie| cookie.reply().ok())
    {
        Some(r) => r,
        None => return true,
    };

    if reply.value.is_empty() {
        return true;
    }

    if reply.format != 32 {
        return false;
    }

    reply
        .value
        .as_chunks::<4>()
        .0
        .iter()
        .any(|chunk| u32::from_ne_bytes(*chunk) == atoms.net_wm_window_type_normal)
}

/// Include decorations: Perch is the outer top edge, and XGetWindowAttributes
/// reports the client rect inside the frame.
fn frame_geometry(
    conn: &RustConnection,
    window: Window,
    x: i16,
    y: i16,
    width: u16,
    height: u16,
) -> (i16, i16, u16, u16) {
    let Some(atoms) = super::atoms::atoms() else {
        return (x, y, width, height);
    };

    let reply = match xproto::get_property(
        conn,
        false,
        window,
        atoms.net_frame_extents,
        AtomEnum::CARDINAL,
        0,
        4,
    )
    .ok()
    .and_then(|cookie| cookie.reply().ok())
    {
        Some(r) => r,
        None => return (x, y, width, height),
    };

    if reply.format != 32 || reply.value.len() != 16 {
        return (x, y, width, height);
    }

    let left = i32::from_ne_bytes([
        reply.value[0],
        reply.value[1],
        reply.value[2],
        reply.value[3],
    ]);
    let right = i32::from_ne_bytes([
        reply.value[4],
        reply.value[5],
        reply.value[6],
        reply.value[7],
    ]);
    let top = i32::from_ne_bytes([
        reply.value[8],
        reply.value[9],
        reply.value[10],
        reply.value[11],
    ]);
    let bottom = i32::from_ne_bytes([
        reply.value[12],
        reply.value[13],
        reply.value[14],
        reply.value[15],
    ]);

    (
        x - left as i16,
        y - top as i16,
        (i32::from(width) + left + right) as u16,
        (i32::from(height) + top + bottom) as u16,
    )
}

fn window_class(conn: &RustConnection, window: Window) -> Option<String> {
    super::atoms::window_class(conn, window)
}
