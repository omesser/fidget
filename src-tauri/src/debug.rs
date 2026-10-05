//! File-based IPC for debug commands: check for place/snapshot requests.

use std::fs;
use std::path::PathBuf;

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

pub fn check_debug_commands(
    position: (i32, i32),
    state: &str,
    roster: &mut fidget_core::roster::Roster,
) {
    let cmd_path = debug_cmd_path();
    let result_path = debug_result_path();

    let Ok(cmd) = fs::read_to_string(&cmd_path) else {
        return;
    };

    let _ = fs::remove_file(&cmd_path);

    let response = if cmd.trim() == "snapshot" {
        format!(
            "position: ({}, {})\nstate: {}",
            position.0, position.1, state
        )
    } else if let Some(place_cmd) = cmd.strip_prefix("place ") {
        if let Some((x_part, y_part)) = place_cmd.split_once(" y=") {
            if let Some(x_str) = x_part.strip_prefix("x=") {
                if let (Ok(x), Ok(y)) = (x_str.parse::<i32>(), y_part.parse::<i32>()) {
                    let ids: Vec<_> = roster.list().iter().map(|(id, _)| id.clone()).collect();
                    let mut moved = false;
                    for id in ids {
                        if let Some(instance) = roster.get_mut(&id) {
                            instance.stand_at(fidget_core::engine::Point {
                                x: x as f64,
                                y: y as f64,
                            });
                            moved = true;
                        }
                    }
                    if moved {
                        format!("placed: ({}, {})", x, y)
                    } else {
                        "error: no instances".to_string()
                    }
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
