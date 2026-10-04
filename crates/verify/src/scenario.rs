//! One end-to-end scenario from `scripts/scenarios/`, with the binaries its
//! `Usage` line names built and passed in. Without `--go` the script prints
//! its takeover header and exits 2, which is this contract's skip.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::contract::{Outcome, RunReport};

const TEST_BINARY: &str = "<fidget test binary>";

/// One platform's script for a scenario name.
struct Leaf {
    path: PathBuf,
    text: String,
}

/// A scenario name and the host scripts that implement it.
/// macOS is `name.sh`, X11 is `name.x11.sh`, Windows is `name.win.ps1`.
struct IndexedScenario {
    name: String,
    macos: Option<Leaf>,
    linux: Option<Leaf>,
    windows: Option<Leaf>,
}

impl IndexedScenario {
    fn leaf(&self, os: &str) -> Option<&Leaf> {
        match os {
            "macos" => self.macos.as_ref(),
            "linux" => self.linux.as_ref(),
            "windows" => self.windows.as_ref(),
            _ => None,
        }
    }

    /// Header to print when there is no go-ahead. Prefer this host's script,
    /// then macOS, then whichever leaf exists.
    fn header_text(&self, os: &str) -> &str {
        self.leaf(os)
            .or(self.macos.as_ref())
            .or(self.linux.as_ref())
            .or(self.windows.as_ref())
            .map(|leaf| leaf.text.as_str())
            .unwrap_or("")
    }
}

fn host_label(os: &str) -> &str {
    match os {
        "linux" => "X11",
        "windows" => "Windows",
        "macos" => "macOS",
        other => other,
    }
}

/// `thinking-row.sh` is macOS, `thinking-row.x11.sh` is X11, `thinking-row.win.ps1` is Windows.
fn scenario_file(file_name: &str) -> Option<(String, &'static str)> {
    if let Some(name) = file_name.strip_suffix(".x11.sh") {
        return Some((name.to_string(), "linux"));
    }
    if let Some(name) = file_name.strip_suffix(".win.ps1") {
        return Some((name.to_string(), "windows"));
    }
    if let Some(name) = file_name.strip_suffix(".sh") {
        return Some((name.to_string(), "macos"));
    }
    None
}

/// Run the scenario `name`, taking over the GUI only when `go` is set.
pub fn run(repo_root: &Path, name: &str, go: bool, extra: &[String], report: &mut RunReport) {
    let dir = repo_root.join("scripts/scenarios");
    let known = index_scenarios(&dir);
    let Some(scenario) = known.iter().find(|s| s.name == name) else {
        let names: Vec<&str> = known.iter().map(|s| s.name.as_str()).collect();
        report.check(
            Outcome::Error,
            "scenario",
            &format!("no scenario {name}; known: {}", names.join(", ")),
        );
        return;
    };

    let os = std::env::consts::OS;
    if !go {
        let header = parse_scenario_header(scenario.header_text(os));
        report.say(&header);
        report.check(
            Outcome::Skip,
            name,
            "printed the takeover header; post it, and rerun with --go once the owner says go",
        );
        return;
    }

    let Some(leaf) = scenario.leaf(os) else {
        report.check(
            Outcome::Skip,
            "scenario",
            &format!("no {} scenario {name}", host_label(os)),
        );
        return;
    };

    let Some(binaries) = binaries(repo_root, leaf.text.contains(TEST_BINARY), report) else {
        return;
    };

    let mut script = script_command(&leaf.path);
    script
        .arg("--go")
        .args(binaries)
        .args(extra)
        .current_dir(repo_root);

    let (outcome, detail) = match report.exec(&mut script, None) {
        Some(0) => (Outcome::Pass, "passed".to_string()),
        Some(1) => (
            Outcome::Fail,
            "failed; the script names its evidence".to_string(),
        ),
        Some(2) => (Outcome::Skip, "the script skipped".to_string()),
        Some(c) => (Outcome::Error, format!("exited {c}")),
        None => (Outcome::Error, "could not run bash".to_string()),
    };
    report.check(outcome, name, &detail);
}

/// Parse the scenario header from script text. Extracts consecutive comment
/// lines after the shebang, stripping leading `#` and optional space.
fn parse_scenario_header(text: &str) -> String {
    let lines = text.lines().skip(1);
    let mut header = Vec::new();
    for line in lines {
        if line.starts_with('#') {
            let content = line
                .strip_prefix("# ")
                .unwrap_or_else(|| line.strip_prefix('#').unwrap_or(line));
            header.push(content);
        } else if !line.trim().is_empty() {
            break;
        }
    }
    header.join("\n")
}

/// Every scenario name in `dir`, sorted, with each host leaf that has a
/// `# Scenario:` header. `fixture-harness.sh` has none, so it is not a name.
fn index_scenarios(dir: &Path) -> Vec<IndexedScenario> {
    let mut by_name: BTreeMap<String, IndexedScenario> = BTreeMap::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some((name, host)) = scenario_file(file_name) else {
            continue;
        };
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        if !text.lines().any(|line| line.starts_with("# Scenario:")) {
            continue;
        }
        let slot = by_name
            .entry(name.clone())
            .or_insert_with(|| IndexedScenario {
                name,
                macos: None,
                linux: None,
                windows: None,
            });
        let leaf = Leaf { path, text };
        match host {
            "macos" => slot.macos = Some(leaf),
            "linux" => slot.linux = Some(leaf),
            "windows" => slot.windows = Some(leaf),
            _ => {}
        }
    }
    by_name.into_values().collect()
}

fn script_command(path: &Path) -> Command {
    if path.extension().and_then(|ext| ext.to_str()) == Some("ps1") {
        let mut cmd = Command::new("powershell");
        cmd.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"]);
        cmd.arg(path);
        cmd
    } else {
        let mut cmd = Command::new("bash");
        cmd.arg(path);
        cmd
    }
}

/// The app binary, then the test binary when the scenario takes one.
fn binaries(repo_root: &Path, wants_test: bool, report: &mut RunReport) -> Option<Vec<PathBuf>> {
    report.say("scenario: building fidget");
    let mut build = Command::new("cargo");
    build.args(["build", "-p", "fidget"]).current_dir(repo_root);
    if report.exec(&mut build, None) != Some(0) {
        report.check(
            Outcome::Error,
            "cargo build",
            "cargo build -p fidget failed",
        );
        return None;
    }
    let mut found = vec![repo_root.join("target/debug/fidget")];
    if wants_test {
        report.say("scenario: building the fixture Harness test binary");
        let out = Command::new("cargo")
            .args(["test", "-p", "fidget", "--no-run", "--message-format=json"])
            .current_dir(repo_root)
            .output();
        match out
            .ok()
            .and_then(|out| test_binary(&String::from_utf8_lossy(&out.stdout)))
        {
            Some(path) => found.push(path),
            None => {
                report.check(
                    Outcome::Error,
                    "test binary",
                    "cargo test -p fidget --no-run named no fidget test binary",
                );
                return None;
            }
        }
    }
    Some(found)
}

/// The `fidget` bin's test executable from `cargo --message-format=json`
/// output. It holds `harness::tests::fake_acp_agent`, the fixture Harness.
fn test_binary(messages: &str) -> Option<PathBuf> {
    messages.lines().find_map(|line| {
        let message: serde_json::Value = serde_json::from_str(line).ok()?;
        let target = &message["target"];
        let is_bin = target["kind"].as_array()?.iter().any(|k| k == "bin");
        if message["profile"]["test"] != true || !is_bin || target["name"] != "fidget" {
            return None;
        }
        Some(PathBuf::from(message["executable"].as_str()?))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_test_binary_is_the_fidget_bins_test_executable() {
        let messages = [
            r#"{"reason":"compiler-artifact","target":{"kind":["lib"],"name":"fidget_core"},"profile":{"test":true},"executable":"/t/deps/fidget_core-1"}"#,
            r#"{"reason":"compiler-artifact","target":{"kind":["bin"],"name":"fidget"},"profile":{"test":false},"executable":"/t/fidget"}"#,
            r#"{"reason":"compiler-artifact","target":{"kind":["bin"],"name":"fidget"},"profile":{"test":true},"executable":"/t/deps/fidget-5ec"}"#,
            r#"{"reason":"build-finished","success":true}"#,
        ]
        .join("\n");

        assert_eq!(
            test_binary(&messages),
            Some(PathBuf::from("/t/deps/fidget-5ec"))
        );
    }

    #[test]
    fn no_fidget_test_executable_is_none() {
        let messages = r#"{"reason":"compiler-artifact","target":{"kind":["bin"],"name":"fidget"},"profile":{"test":true},"executable":null}"#;

        assert_eq!(test_binary(messages), None);
    }

    #[test]
    fn an_x11_suffix_is_the_linux_leaf_not_a_second_name() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("thinking-row.sh"),
            "# Scenario: thinking-row (macOS)\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("thinking-row.x11.sh"),
            "# Scenario: thinking-row (X11)\n",
        )
        .unwrap();
        fs::write(dir.path().join("fixture-harness.sh"), "#!/bin/sh\n").unwrap();

        let indexed = index_scenarios(dir.path());
        assert_eq!(indexed.len(), 1);
        assert_eq!(indexed[0].name, "thinking-row");
        assert!(indexed[0]
            .macos
            .as_ref()
            .unwrap()
            .path
            .ends_with("thinking-row.sh"));
        assert!(indexed[0]
            .linux
            .as_ref()
            .unwrap()
            .path
            .ends_with("thinking-row.x11.sh"));
        assert!(indexed[0].windows.is_none());
        assert!(indexed[0].header_text("linux").contains("(X11)"));
        assert!(indexed[0].header_text("macos").contains("(macOS)"));
    }

    #[test]
    fn a_windows_suffix_is_the_windows_leaf() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("thinking-row.win.ps1"),
            "# Scenario: thinking-row (Windows)\n",
        )
        .unwrap();

        let indexed = index_scenarios(dir.path());
        assert_eq!(indexed.len(), 1);
        assert!(indexed[0].windows.is_some());
        assert!(indexed[0].leaf("linux").is_none());
        assert!(indexed[0].header_text("linux").contains("(Windows)"));
    }

    #[cfg(unix)]
    fn repo_scenarios() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scripts/scenarios")
    }

    #[cfg(unix)]
    #[test]
    fn thinking_row_x11_fixture_passes_and_a_wrong_glyph_fails() {
        let scenarios = repo_scenarios();
        let script = scenarios.join("thinking-row.x11.sh");
        let open = scenarios.join("fixtures/thinking-row-open.txt");
        let done = scenarios.join("fixtures/thinking-row-done.txt");
        let ok = Command::new("bash")
            .arg(&script)
            .args(["--go", "/bin/true", "/bin/true"])
            .env("FIDGET_SCENARIO_AX_OPEN", &open)
            .env("FIDGET_SCENARIO_AX_DONE", &done)
            .env_remove("DISPLAY")
            .output()
            .unwrap();
        assert_eq!(
            ok.status.code(),
            Some(0),
            "stdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&ok.stdout),
            String::from_utf8_lossy(&ok.stderr)
        );

        let bad_dir = tempfile::tempdir().unwrap();
        let bad = bad_dir.path().join("bad.txt");
        fs::write(&bad, "button|x Thinking|\n").unwrap();
        let fail = Command::new("bash")
            .arg(&script)
            .args(["--go", "/bin/true", "/bin/true"])
            .env("FIDGET_SCENARIO_AX_OPEN", &bad)
            .env("FIDGET_SCENARIO_AX_DONE", &done)
            .env_remove("DISPLAY")
            .output()
            .unwrap();
        assert_eq!(fail.status.code(), Some(1));
        let err = String::from_utf8_lossy(&fail.stderr);
        assert!(err.contains("want it to start with"), "stderr was:\n{err}");
    }

    #[cfg(unix)]
    #[test]
    fn thinking_row_x11_without_a_display_skips() {
        let script = repo_scenarios().join("thinking-row.x11.sh");
        let out = Command::new("bash")
            .arg(&script)
            .args(["--go", "/bin/true", "/bin/true"])
            .env_remove("DISPLAY")
            .env_remove("FIDGET_SCENARIO_AX_OPEN")
            .env_remove("FIDGET_SCENARIO_AX_DONE")
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(2),
            "stderr:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(String::from_utf8_lossy(&out.stderr).contains("DISPLAY is unset"));
    }

    #[cfg(unix)]
    #[test]
    fn chat_header_narrow_x11_fixture_passes_and_a_wide_page_fails() {
        let scenarios = repo_scenarios();
        let script = scenarios.join("chat-header-narrow.x11.sh");
        let ax420 = scenarios.join("fixtures/chat-header-narrow-420.txt");
        let ax360 = scenarios.join("fixtures/chat-header-narrow-360.txt");
        let ax320 = scenarios.join("fixtures/chat-header-narrow-320.txt");
        let ok = Command::new("bash")
            .arg(&script)
            .args(["--go", "/bin/true", "/bin/true"])
            .env("FIDGET_SCENARIO_AX_420", &ax420)
            .env("FIDGET_SCENARIO_AX_360", &ax360)
            .env("FIDGET_SCENARIO_AX_320", &ax320)
            .env_remove("DISPLAY")
            .output()
            .unwrap();
        assert_eq!(
            ok.status.code(),
            Some(0),
            "stdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&ok.stdout),
            String::from_utf8_lossy(&ok.stderr)
        );

        let bad = scenarios.join("fixtures/chat-header-narrow-bad-scroll.txt");
        let fail = Command::new("bash")
            .arg(&script)
            .args(["--go", "/bin/true", "/bin/true"])
            .env("FIDGET_SCENARIO_AX_420", &ax420)
            .env("FIDGET_SCENARIO_AX_360", &ax360)
            .env("FIDGET_SCENARIO_AX_320", &bad)
            .env_remove("DISPLAY")
            .output()
            .unwrap();
        assert_eq!(fail.status.code(), Some(1));
        let err = String::from_utf8_lossy(&fail.stderr);
        assert!(err.contains("scrolls sideways"), "stderr was:\n{err}");
    }

    #[cfg(unix)]
    #[test]
    fn chat_header_narrow_x11_without_a_display_skips() {
        let script = repo_scenarios().join("chat-header-narrow.x11.sh");
        let out = Command::new("bash")
            .arg(&script)
            .args(["--go", "/bin/true", "/bin/true"])
            .env_remove("DISPLAY")
            .env_remove("FIDGET_SCENARIO_AX_420")
            .env_remove("FIDGET_SCENARIO_AX_360")
            .env_remove("FIDGET_SCENARIO_AX_320")
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(2),
            "stderr:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(String::from_utf8_lossy(&out.stderr).contains("DISPLAY is unset"));
    }
}
