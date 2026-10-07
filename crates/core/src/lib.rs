//! The pure core: everything fidget knows how to do without a window server.
//! Nothing here depends on Tauri or a platform binding (docs/SPEC.md); adapters
//! to the outside world are declared here as traits and implemented in the shell.

pub mod character;
pub mod debug;
pub mod director;
pub mod dispatch;
pub mod display;
pub mod engine;
pub mod input;
pub mod memory;
pub mod overlay;
pub mod overlay_region;
pub mod roster;
pub mod scheduler;
pub mod sensing;
pub mod snapshot;
pub mod speech;
pub mod visibility;
pub mod window_source;

mod tools;
