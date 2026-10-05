//! File-based IPC for debug commands: place character at (x,y) and snapshot state.
//!
//! The verify binary writes commands to `~/.fidget-debug-cmd`, then polls
//! `~/.fidget-debug-result` for responses from the running fidget instance.

use std::fs;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

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

fn send_command(cmd: &str) -> u8 {
    let cmd_path = debug_cmd_path();
    let result_path = debug_result_path();

    let _ = fs::remove_file(&result_path);

    if let Err(e) = fs::write(&cmd_path, cmd) {
        eprintln!("failed to write command: {e}");
        return 2;
    }

    for _ in 0..50 {
        thread::sleep(Duration::from_millis(100));
        if let Ok(result) = fs::read_to_string(&result_path) {
            println!("{}", result.trim());
            let _ = fs::remove_file(&result_path);
            return 0;
        }
    }

    eprintln!("timeout waiting for fidget to respond");
    2
}

pub fn place(x: i32, y: i32) -> u8 {
    send_command(&format!("place x={} y={}", x, y))
}

pub fn snapshot() -> u8 {
    send_command("snapshot")
}
