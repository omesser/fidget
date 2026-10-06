//! File-based IPC for debug commands: place character at (x,y) and snapshot state.
//!
//! The verify binary writes commands to `~/.fidget-debug-cmd`, then polls
//! `~/.fidget-debug-result` for responses from the running fidget instance.
//! Requires fidget to run with FIDGET_DEBUG_IPC=1.

use std::fs;
use std::thread;
use std::time::Duration;

use crate::contract::{Outcome, RunReport};

fn send_command(cmd: &str, report: &mut RunReport) -> Outcome {
    let Some(cmd_path) = fidget_core::memory::debug_cmd_path() else {
        report.check(
            Outcome::Error,
            "debug ipc",
            "cannot determine home directory",
        );
        return Outcome::Error;
    };
    let Some(result_path) = fidget_core::memory::debug_result_path() else {
        report.check(
            Outcome::Error,
            "debug ipc",
            "cannot determine home directory",
        );
        return Outcome::Error;
    };

    let _ = fs::remove_file(&result_path);

    if let Err(e) = fs::write(&cmd_path, cmd) {
        report.check(
            Outcome::Error,
            "debug ipc",
            &format!("failed to write command: {e}"),
        );
        return Outcome::Error;
    }

    for _ in 0..50 {
        thread::sleep(Duration::from_millis(100));
        if let Ok(result) = fs::read_to_string(&result_path) {
            let trimmed = result.trim();
            let _ = fs::remove_file(&result_path);

            if trimmed.starts_with("error:") {
                report.check(Outcome::Fail, "debug ipc", trimmed);
                return Outcome::Fail;
            } else {
                report.check(Outcome::Pass, "debug ipc", trimmed);
                return Outcome::Pass;
            }
        }
    }

    let _ = fs::remove_file(&cmd_path);
    report.check(
        Outcome::Error,
        "debug ipc",
        "timeout waiting for fidget to respond",
    );
    Outcome::Error
}

pub fn place(x: i32, y: i32, report: &mut RunReport) {
    send_command(&format!("place x={} y={}", x, y), report);
}

pub fn snapshot(report: &mut RunReport) {
    send_command("snapshot", report);
}
