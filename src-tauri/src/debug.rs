//! File-based IPC for test scripts: place/snapshot commands.

use std::fs;

fn parse_place(text: &str) -> Result<(i32, i32), String> {
    let parts: Vec<&str> = text.split_whitespace().collect();
    if parts.len() != 2 {
        return Err("syntax: place x=N y=M".to_string());
    }

    let x = parts[0]
        .strip_prefix("x=")
        .and_then(|s| s.parse::<i32>().ok())
        .ok_or_else(|| "invalid x coordinate".to_string())?;

    let y = parts[1]
        .strip_prefix("y=")
        .and_then(|s| s.parse::<i32>().ok())
        .ok_or_else(|| "invalid y coordinate".to_string())?;

    Ok((x, y))
}

pub fn check_debug_commands(
    position: (i32, i32),
    state: &str,
    roster: &mut fidget_core::roster::Roster,
) {
    if !crate::dev_flags::DEBUG_IPC.is_on() {
        return;
    }

    let (Some(cmd_path), Some(result_path)) = (
        fidget_core::memory::debug_cmd_path(),
        fidget_core::memory::debug_result_path(),
    ) else {
        return;
    };

    let Ok(cmd) = fs::read_to_string(&cmd_path) else {
        return;
    };

    let _ = fs::remove_file(&cmd_path);

    let response = match cmd.trim() {
        "snapshot" => format!(
            "position: ({}, {})\nstate: {}",
            position.0, position.1, state
        ),
        text if text.starts_with("place ") => {
            let place_args = text.strip_prefix("place ").unwrap();
            match parse_place(place_args) {
                Ok((x, y)) => {
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
                }
                Err(e) => format!("error: {}", e),
            }
        }
        text => format!("error: unknown command '{}'", text),
    };

    let _ = fs::write(&result_path, &response);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_place_accepts_valid_coordinates() {
        assert_eq!(parse_place("x=0 y=800"), Ok((0, 800)));
        assert_eq!(parse_place("x=-100 y=50"), Ok((-100, 50)));
        assert_eq!(parse_place("x=1920 y=1080"), Ok((1920, 1080)));
    }

    #[test]
    fn parse_place_rejects_missing_prefix() {
        assert!(parse_place("0 y=800").is_err());
        assert!(parse_place("x=0 800").is_err());
    }

    #[test]
    fn parse_place_rejects_non_numeric() {
        assert!(parse_place("x=abc y=800").is_err());
        assert!(parse_place("x=0 y=def").is_err());
    }

    #[test]
    fn parse_place_rejects_wrong_arg_count() {
        assert!(parse_place("x=0").is_err());
        assert!(parse_place("x=0 y=1 z=2").is_err());
    }
}
