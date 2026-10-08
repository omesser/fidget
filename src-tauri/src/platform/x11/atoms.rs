//! Every EWMH atom the X11 modules name, interned once for the process.
//!
//! Interning is a synchronous round trip, and an atom is stable for the life of
//! a server connection — which is the process here (`connection.rs`). Interning
//! per read cost the window walk seven round trips a window, at `ENGINE_TICK`,
//! for answers already in hand (#268).
//!
//! Resolution can fail because the connection can. `atoms` then returns None
//! and each caller bails the way it already bailed on a failed intern: a window
//! that cannot be classified is skipped and undecorated geometry stands.

use std::sync::OnceLock;
use x11rb::protocol::xproto::{self, Atom};
use x11rb::rust_connection::RustConnection;

/// The atoms, resolved together. One `OnceLock` for all of them rather than one
/// per name: they all come from the same connection, so either it answers or
/// none of them resolve.
pub struct Atoms {
    pub net_active_window: Atom,
    pub net_client_list: Atom,
    pub net_client_list_stacking: Atom,
    pub net_frame_extents: Atom,
    pub net_wm_name: Atom,
    pub net_wm_state: Atom,
    pub net_wm_state_above: Atom,
    pub net_wm_state_skip_pager: Atom,
    pub net_wm_state_skip_taskbar: Atom,
    pub net_wm_window_type: Atom,
    pub net_wm_window_type_normal: Atom,
    pub utf8_string: Atom,
}

/// Interned on first call and reused for the process lifetime. Returns None if
/// there is no connection, or the server would not answer.
pub fn atoms() -> Option<&'static Atoms> {
    static ATOMS: OnceLock<Option<Atoms>> = OnceLock::new();
    ATOMS.get_or_init(intern_all).as_ref()
}

fn intern_all() -> Option<Atoms> {
    let conn = super::connection::connection()?;
    Some(Atoms {
        net_active_window: intern(conn, "_NET_ACTIVE_WINDOW")?,
        net_client_list: intern(conn, "_NET_CLIENT_LIST")?,
        net_client_list_stacking: intern(conn, "_NET_CLIENT_LIST_STACKING")?,
        net_frame_extents: intern(conn, "_NET_FRAME_EXTENTS")?,
        net_wm_name: intern(conn, "_NET_WM_NAME")?,
        net_wm_state: intern(conn, "_NET_WM_STATE")?,
        net_wm_state_above: intern(conn, "_NET_WM_STATE_ABOVE")?,
        net_wm_state_skip_pager: intern(conn, "_NET_WM_STATE_SKIP_PAGER")?,
        net_wm_state_skip_taskbar: intern(conn, "_NET_WM_STATE_SKIP_TASKBAR")?,
        net_wm_window_type: intern(conn, "_NET_WM_WINDOW_TYPE")?,
        net_wm_window_type_normal: intern(conn, "_NET_WM_WINDOW_TYPE_NORMAL")?,
        utf8_string: intern(conn, "UTF8_STRING")?,
    })
}

/// `only_if_exists` is false, as every call site here always passed: a name no
/// window manager has used yet is created rather than refused, so the only way
/// this returns None is the connection failing.
fn intern(conn: &RustConnection, name: &str) -> Option<Atom> {
    xproto::intern_atom(conn, false, name.as_bytes())
        .ok()?
        .reply()
        .ok()
        .map(|reply| reply.atom)
}

/// Parse WM_CLASS property bytes into the application name.
/// Two NUL-terminated strings, instance then class; class wins, instance
/// is the UTF-8 fallback.
pub(super) fn parse_wm_class(property_bytes: &[u8]) -> Option<String> {
    String::from_utf8(property_bytes.to_vec())
        .ok()
        .and_then(|s| s.split('\0').nth(1).map(|c| c.to_string()))
        .or_else(|| {
            String::from_utf8_lossy(property_bytes)
                .split('\0')
                .next()
                .map(|s| s.to_string())
        })
        .filter(|s| !s.is_empty())
}

/// Read WM_CLASS to get the window's application name.
pub(super) fn window_class(conn: &RustConnection, window: xproto::Window) -> Option<String> {
    let reply = xproto::get_property(
        conn,
        false,
        window,
        xproto::AtomEnum::WM_CLASS,
        xproto::AtomEnum::STRING,
        0,
        1024,
    )
    .ok()?
    .reply()
    .ok()?;

    if reply.format != 8 {
        return None;
    }

    parse_wm_class(&reply.value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_wm_class_returns_class_from_valid_property() {
        let property = b"instance\0Class\0";
        assert_eq!(parse_wm_class(property), Some("Class".to_string()));
    }

    #[test]
    fn parse_wm_class_returns_none_when_property_missing() {
        let property = b"";
        assert_eq!(parse_wm_class(property), None);
    }

    #[test]
    fn parse_wm_class_handles_single_string() {
        let property = b"firefox\0";
        assert_eq!(parse_wm_class(property), None);
    }

    #[test]
    fn parse_wm_class_falls_back_to_instance_on_invalid_utf8() {
        let property = b"valid\0\xFF\xFE\0";
        assert_eq!(parse_wm_class(property), Some("valid".to_string()));
    }
}
