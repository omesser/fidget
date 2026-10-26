//! Every AppKit and CoreGraphics implementation the Shell provides.
//!
//! macOS's answer to what `platform` asks for. Keeping every AppKit and
//! CoreGraphics call in the Shell is what lets the core crate build and be
//! tested with no platform binding at all.

#![cfg(target_os = "macos")]

mod dock;
mod input_events;
mod minimize;
mod overlay_panel;
mod pointer;
mod precise_sleep;
mod sensing;
mod tray;
mod window_source;

pub use dock::dock_bounds;
pub use input_events::{spawn_listener as spawn_event_tap, EventTap};
pub use minimize::{observe_minimize, MinimizeObserver};
pub use overlay_panel::{configure_overlay, hand_back_keyboard, remember_front_app};
pub use pointer::{
    control_click, double_click_interval_ms, primary_button_down, primary_press_began,
    secondary_button_down, trace_tick,
};
pub(crate) use precise_sleep::sleep_precisely;
pub use sensing::MacosActivitySource;
pub use tray::{seed_status_item_position, tune_tray_icon};
pub(super) use window_source::visible_window_titles;
pub use window_source::MacosWindowSource;
