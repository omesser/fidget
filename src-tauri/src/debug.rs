//! File-based IPC for debug commands: check for place/snapshot requests.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI32, Ordering};

fn debug_cmd_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".fidget-debug-cmd")
}

fn debug_result_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".fidget-debug-result")
}

static OVERRIDE_X: AtomicI32 = AtomicI32::new(i32::MIN);
static OVERRIDE_Y: AtomicI32 = AtomicI32::new(i32::MIN);
static OVERRIDE_ACTIVE: AtomicI32 = AtomicI32::new(0);

pub fn get_position_override() -> Option<(i32, i32)> {
    if OVERRIDE_ACTIVE.load(Ordering::Relaxed) > 0 {
        OVERRIDE_ACTIVE.fetch_sub(1, Ordering::Relaxed);
        Some((
            OVERRIDE_X.load(Ordering::Relaxed),
            OVERRIDE_Y.load(Ordering::Relaxed),
        ))
    } else {
        None
    }
}

pub fn check_debug_commands(position: (i32, i32), state: &str) {
    let cmd_path = debug_cmd_path();
    let result_path = debug_result_path();

    let Ok(cmd) = fs::read_to_string(&cmd_path) else {
        return;
    };

    let _ = fs::remove_file(&cmd_path);

    let response = if cmd.trim() == "snapshot" {
        format!("position: ({}, {})\nstate: {}", position.0, position.1, state)
    } else if let Some(place_cmd) = cmd.strip_prefix("place ") {
        if let Some((x_part, y_part)) = place_cmd.split_once(" y=") {
            if let Some(x_str) = x_part.strip_prefix("x=") {
                if let (Ok(x), Ok(y)) = (x_str.parse::<i32>(), y_part.parse::<i32>()) {
                    OVERRIDE_X.store(x, Ordering::Relaxed);
                    OVERRIDE_Y.store(y, Ordering::Relaxed);
                    OVERRIDE_ACTIVE.store(20, Ordering::Relaxed);
                    format!("placed: ({}, {})", x, y)
                } else {
                    "error: invalid coordinates".to_string()
                }
            } else {
                "error: invalid place syntax".to_string()
            }
        } else {
            "error: invalid place syntax".to_string()
        }
    } else {
        format!("error: unknown command '{}'", cmd.trim())
    };

    let _ = fs::write(&result_path, &response);
}
