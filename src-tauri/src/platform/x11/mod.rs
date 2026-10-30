//! X11 implementation for Linux functional parity.
//!
//! Fills the same seams as `platform::macos`: pointer state, overlay configuration,
//! window geometry, activity sensing. X11 only: where no X server answers,
//! every seam declares its capability absent. DESIGN.md decision 3.

#![cfg(all(unix, not(target_os = "macos")))]

mod atoms;
mod connection;
mod focus_return;
mod input_events;
mod overlay;
mod pointer;
mod sensing;
mod settings_raise;
mod window_source;

pub use input_events::{buttons_down, press_point, spawn_listener, InputEvent};
#[cfg(test)]
pub use overlay::set_composited_for_test;
pub use overlay::{
    change_bounding_shape, cleanup_overlay_window, composited, composited_now, configure_overlay,
    update_input_region, SpriteMask,
};
pub use pointer::double_click_interval_ms;
pub use sensing::X11ActivitySource;

pub(super) use connection::connection;
pub(super) use focus_return::{remember_active_window, return_keyboard};
pub(super) use settings_raise::raise_settings_ewmh_above;
pub(super) use window_source::{visible_window_titles, X11WindowSource};
