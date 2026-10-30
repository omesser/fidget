//! X11 pointer button state via XQueryPointer.
//! The Shell polls this beside the cursor position each tick. XQueryPointer
//! reads the current button state without needing XI2 events or grabs.

use gtk::prelude::*;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{self, ButtonMask};

use crate::platform::ButtonsDown;

/// Both buttons (Button1 and Button3) from one XQueryPointer.
/// One reply is the whole mask: two round trips would re-query an answer
/// already in hand. Failure is nothing held; `platform.rs` witnesses the overlay.
pub fn buttons_down() -> ButtonsDown {
    let Some(mask) = button_state_mask() else {
        return ButtonsDown::default();
    };
    ButtonsDown {
        primary: (mask & u16::from(ButtonMask::M1)) != 0,
        secondary: (mask & u16::from(ButtonMask::M3)) != 0,
    }
}

fn button_state_mask() -> Option<u16> {
    let display = super::connection::connection()?;
    let screen = &display.setup().roots[0];
    let reply = xproto::query_pointer(display, screen.root)
        .ok()?
        .reply()
        .ok()?;
    Some(reply.mask.into())
}

/// The OS double-click interval, in milliseconds.
pub fn double_click_interval_ms() -> Option<u32> {
    if !gtk::is_initialized() {
        return None;
    }
    gtk::Settings::default().and_then(|settings| {
        let interval = settings.gtk_double_click_time();
        if interval > 0 {
            Some(interval as u32)
        } else {
            None
        }
    })
}
