//! X11 pointer settings. Button presses come from XI2 raw events in
//! `input_events`.

use gtk::prelude::*;

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
