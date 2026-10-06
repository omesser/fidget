//! File-based IPC for debug commands: place character at (x,y) and snapshot state.
//!
//! The verify binary writes commands to `~/.fidget-debug-cmd`, then polls
//! `~/.fidget-debug-result` for responses from the running fidget instance.
//! Requires fidget to run with FIDGET_DEBUG_IPC=1.

use std::fs;
use std::thread;
use std::time::Duration;

fn send_command(cmd: &str) -> u8 {
    let Some(cmd_path) = fidget_core::memory::debug_cmd_path() else {
        eprintln!("cannot determine home directory");
        return 2;
    };
    let Some(result_path) = fidget_core::memory::debug_result_path() else {
        eprintln!("cannot determine home directory");
        return 2;
    };

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
