//! The contract as an agent meets it: the real binary, its exit status, and
//! what lands on stdout (#647 stone 3).

use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;
use tempfile::TempDir;

const BIN: &str = env!("CARGO_BIN_EXE_fidget-verify");

fn verify(evidence: &Path, args: &[&str]) -> Output {
    let mut cmd = Command::new(BIN);
    cmd.args(["--evidence-dir", evidence.to_str().unwrap()]);
    cmd.args(["--run-id", "clitest"]);
    cmd.args(args);
    cmd.output().unwrap()
}

fn evidence_in(dir: &TempDir) -> std::path::PathBuf {
    dir.path().join("run").join("evidence")
}

#[test]
fn json_doctor_puts_one_parseable_object_on_stdout_and_exits_zero() {
    let dir = TempDir::new().unwrap();
    let evidence = evidence_in(&dir);
    let out = verify(&evidence, &["--json", "doctor"]);

    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json: Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "stdout was not one JSON object ({e}): {:?}",
            String::from_utf8_lossy(&out.stdout)
        )
    });
    assert_eq!(json["command"], "doctor");
    assert_eq!(json["outcome"], "pass");
    assert_eq!(json["exit_code"], 0);
    assert_eq!(json["run_id"], "clitest");
    assert_eq!(json["evidence_dir"], evidence.to_str().unwrap());
    assert!(
        json["checks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["name"] == "workspace layout"),
        "checks were {:?}",
        json["checks"]
    );
}

#[test]
fn human_doctor_keeps_its_own_output_and_prints_no_json() {
    let dir = TempDir::new().unwrap();
    let evidence = evidence_in(&dir);
    let out = verify(&evidence, &["doctor"]);

    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("doctor: OK"), "stdout was:\n{stdout}");
    assert!(
        stdout.contains("  PASS  workspace layout"),
        "stdout was:\n{stdout}"
    );
    assert!(!stdout.contains("\"outcome\""), "stdout was:\n{stdout}");
}

#[test]
fn json_cleanup_reports_the_evidence_it_preserved() {
    let dir = TempDir::new().unwrap();
    let evidence = evidence_in(&dir);
    let out = verify(&evidence, &["--json", "cleanup"]);

    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["command"], "cleanup");
    assert_eq!(json["outcome"], "pass");
    assert_eq!(json["exit_code"], 0);
    assert!(evidence.is_dir(), "cleanup must never delete evidence");
}

#[test]
fn a_bad_flag_is_a_tool_error_not_a_verification_failure() {
    let out = Command::new(BIN).arg("--no-such-flag").output().unwrap();

    assert_eq!(out.status.code(), Some(3));
}

#[test]
fn an_unknown_subcommand_is_a_tool_error() {
    let out = Command::new(BIN).arg("frobnicate").output().unwrap();

    assert_eq!(out.status.code(), Some(3));
}

#[test]
fn help_is_not_an_error() {
    let out = Command::new(BIN).arg("--help").output().unwrap();

    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn every_run_appends_its_own_proof_section() {
    let dir = TempDir::new().unwrap();
    let evidence = evidence_in(&dir);
    verify(&evidence, &["doctor"]);
    verify(&evidence, &["--json", "cleanup"]);

    let text = std::fs::read_to_string(evidence.join("PROOF.md")).unwrap();
    assert!(text.contains(" doctor PASS (exit 0)"), "proof was:\n{text}");
    assert!(
        text.contains(" cleanup PASS (exit 0)"),
        "proof was:\n{text}"
    );
    assert!(
        text.contains(&format!("evidence: {}\n", evidence.display())),
        "proof was:\n{text}"
    );
}

#[test]
fn a_scenario_without_go_prints_its_takeover_header_and_skips() {
    let dir = TempDir::new().unwrap();
    let evidence = evidence_in(&dir);
    let out = verify(&evidence, &["--json", "scenario", "thinking-row"]);

    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    let platform = match std::env::consts::OS {
        "linux" => "X11",
        "windows" => "Windows",
        _ => "macOS",
    };
    assert!(
        stderr.contains(&format!("Scenario: thinking-row ({platform})")),
        "stderr was:\n{stderr}"
    );
    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["command"], "scenario");
    assert_eq!(json["outcome"], "skip");
}

#[test]
fn an_unknown_scenario_is_a_tool_error_that_names_the_known_ones() {
    let dir = TempDir::new().unwrap();
    let evidence = evidence_in(&dir);
    let out = verify(&evidence, &["scenario", "../hero-gif"]);

    assert_eq!(out.status.code(), Some(3));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("thinking-row"), "stdout was:\n{stdout}");
    assert!(!stdout.contains("fixture-harness"), "stdout was:\n{stdout}");
}

#[test]
fn data_dir_override_moves_debug_ipc_out_of_home() {
    let dir = TempDir::new().unwrap();
    let data = dir.path().join("data");
    let home = dir.path().join("home");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(&home).unwrap();
    let evidence = evidence_in(&dir);
    let child = Command::new(BIN)
        .args([
            "--evidence-dir",
            evidence.to_str().unwrap(),
            "--run-id",
            "clitest",
        ])
        .args(["place", "--x", "3", "--y", "4"])
        .env("FIDGET_DATA_DIR", &data)
        .env("HOME", &home)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let cmd = data.join(".fidget-debug-cmd");
    let mut sent = None;
    for _ in 0..40 {
        if let Ok(text) = std::fs::read_to_string(&cmd) {
            sent = Some(text);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    std::fs::write(data.join(".fidget-debug-result"), "ok").unwrap();
    let out = child.wait_with_output().unwrap();

    assert_eq!(sent.as_deref(), Some("place x=3 y=4"));
    assert!(!home.join(".fidget-debug-cmd").exists());
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Runs one `place` and returns the first text a busy reader sees at the
/// command file, the way the app's per-frame poll could catch it.
fn first_read_of_a_sent_place_command(data: &Path) -> String {
    let evidence = data.join("evidence");
    let child = Command::new(BIN)
        .args(["--evidence-dir", evidence.to_str().unwrap()])
        .args(["--run-id", "clitest"])
        .args(["place", "--x", "3", "--y", "4"])
        .env("FIDGET_DATA_DIR", data)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let cmd = data.join(".fidget-debug-cmd");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    let first_read = loop {
        if let Ok(text) = std::fs::read_to_string(&cmd) {
            break text;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "no command file within 3 s"
        );
    };
    std::fs::remove_file(&cmd).unwrap();
    std::fs::write(data.join(".fidget-debug-result"), "ok").unwrap();
    child.wait_with_output().unwrap();
    first_read
}

#[test]
fn a_reader_never_sees_a_partial_debug_command() {
    let dir = TempDir::new().unwrap();
    for _ in 0..20 {
        assert_eq!(
            first_read_of_a_sent_place_command(dir.path()),
            "place x=3 y=4"
        );
    }
}
