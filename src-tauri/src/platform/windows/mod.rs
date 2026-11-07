//! Windows Win32 implementation for functional parity.
//!
//! Fills the same seams as `platform::macos` and `platform::x11`: pointer state,
//! overlay configuration, window geometry, activity sensing.

#![cfg(not(unix))]

mod console;
mod focus_return;
mod input_events;
mod overlay;
mod pointer;
mod process;
mod sensing;
mod settings_raise;
mod window_source;

pub(super) use console::{restore_ctrl_c, suppress_ctrl_c_for_children};
pub(super) use input_events::{press_point, spawn_press_listener};
pub use overlay::{
    cleanup_overlay_window, configure_overlay, set_click_through, toggle_click_through_only,
    update_input_region,
};
pub use pointer::{buttons_down, double_click_interval_ms};
pub use sensing::WindowsActivitySource;
pub(super) use settings_raise::{activate_for_typing, lower_to_normal_level, raise_above_overlay};

pub(super) use focus_return::{hand_back_keyboard, remember_foreground};
pub(super) use window_source::{visible_window_titles, WindowsWindowSource};
