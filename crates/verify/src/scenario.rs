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

    #[cfg(windows)]
    #[test]
    fn chat_header_narrow_win_fixture_passes_with_web_area_only() {
        let scenarios = repo_scenarios();
        let script = scenarios.join("chat-header-narrow.win.ps1");
        let ax420 = scenarios.join("fixtures/chat-header-narrow-420-win.txt");
        let ax360 = scenarios.join("fixtures/chat-header-narrow-360.txt");
        let ax320 = scenarios.join("fixtures/chat-header-narrow-320.txt");
        let ok = Command::new("powershell")
            .args([
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                script.to_str().unwrap(),
                "--go",
                "C:\\Windows\\System32\\cmd.exe",
                "C:\\Windows\\System32\\cmd.exe",
            ])
            .env("FIDGET_SCENARIO_AX_420", &ax420)
            .env("FIDGET_SCENARIO_AX_360", &ax360)
            .env("FIDGET_SCENARIO_AX_320", &ax320)
            .output()
            .unwrap();
        assert_eq!(
            ok.status.code(),
            Some(0),
            "stdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&ok.stdout),
            String::from_utf8_lossy(&ok.stderr)
        );
    }

    #[cfg(windows)]
    #[test]
    fn thinking_row_win_fixture_passes_and_a_wrong_glyph_fails() {
        let scenarios = repo_scenarios();
        let script = scenarios.join("thinking-row.win.ps1");
        let open = scenarios.join("fixtures/thinking-row-open.txt");
        let done = scenarios.join("fixtures/thinking-row-done.txt");
        let ok = Command::new("powershell")
            .args([
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                script.to_str().unwrap(),
                "--go",
                "C:\\Windows\\System32\\cmd.exe",
                "C:\\Windows\\System32\\cmd.exe",
            ])
            .env("FIDGET_SCENARIO_AX_OPEN", &open)
            .env("FIDGET_SCENARIO_AX_DONE", &done)
            .output()
            .unwrap();
        assert_eq!(
            ok.status.code(),
            Some(0),
            "stdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&ok.stdout),
            String::from_utf8_lossy(&ok.stderr)
        );
        assert!(
            String::from_utf8_lossy(&ok.stdout).contains("PASS: fixture dumps"),
            "stdout:\n{}",
            String::from_utf8_lossy(&ok.stdout)
        );

        let bad_dir = tempfile::tempdir().unwrap();
        let bad = bad_dir.path().join("bad.txt");
        fs::write(&bad, "button|x Thinking|\n").unwrap();
        let fail = Command::new("powershell")
            .args([
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                script.to_str().unwrap(),
                "--go",
                "C:\\Windows\\System32\\cmd.exe",
                "C:\\Windows\\System32\\cmd.exe",
            ])
            .env("FIDGET_SCENARIO_AX_OPEN", &bad)
            .env("FIDGET_SCENARIO_AX_DONE", &done)
            .output()
            .unwrap();
        assert_eq!(
            fail.status.code(),
            Some(1),
            "stdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&fail.stdout),
            String::from_utf8_lossy(&fail.stderr)
        );
        let err = String::from_utf8_lossy(&fail.stderr);
        assert!(err.contains("want it to start with"), "stderr was:\n{err}");
    }

    /// Runs a leaf on fixture dumps with no desktop. `envs` paths are under
    /// `scripts/scenarios/` unless absolute.
    fn fixture_run(script: &str, envs: &[(&str, PathBuf)]) -> std::process::Output {
        let scenarios = repo_scenarios();
        let path = scenarios.join(script);
        let mut cmd = if script.ends_with(".ps1") {
            let mut cmd = Command::new("powershell");
            cmd.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"]);
            cmd.arg(&path).args([
                "--go",
                "C:\\Windows\\System32\\cmd.exe",
                "C:\\Windows\\System32\\cmd.exe",
            ]);
            cmd
        } else {
            let mut cmd = Command::new("bash");
            cmd.arg(&path).args(["--go", "/bin/true", "/bin/true"]);
            cmd
        };
        cmd.env_remove("DISPLAY");
        for (key, value) in envs {
            cmd.env(key, scenarios.join(value));
        }
        cmd.output().unwrap()
    }

    fn assert_exit(out: &std::process::Output, code: i32, stderr_has: &str) {
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(
            out.status.code(),
            Some(code),
            "stdout:\n{}\nstderr:\n{stderr}",
            String::from_utf8_lossy(&out.stdout)
        );
        assert!(stderr.contains(stderr_has), "stderr was:\n{stderr}");
    }

    type Mutate = fn(&str) -> String;

    /// `text` without the lines holding `needle`.
    fn drop_lines(text: &str, needle: &str) -> String {
        text.lines()
            .filter(|line| !line.contains(needle))
            .map(|line| format!("{line}\n"))
            .collect()
    }

    /// `text` with `edit` applied to each line holding `needle`.
    fn edit_lines(text: &str, needle: &str, edit: impl Fn(&str) -> String) -> String {
        text.lines()
            .map(|line| {
                let line = if line.contains(needle) {
                    edit(line)
                } else {
                    line.to_string()
                };
                format!("{line}\n")
            })
            .collect()
    }

    fn reframe(line: &str, frame: &str) -> String {
        format!("{}|{frame}", line.rsplit_once('|').unwrap().0)
    }

    fn retype(line: &str, role: &str) -> String {
        format!("{role}|{}", line.split_once('|').unwrap().1)
    }

    /// Runs `script` on the `good` fixtures, then once per case with that
    /// case's fixture mutated, and wants each case to fail with its message.
    /// One case per assertion, so deleting any assertion turns a case red.
    fn fixture_cases(script: &str, good: &[(&str, &str)], cases: &[(&str, Mutate, &str)]) {
        let dir = tempfile::tempdir().unwrap();
        let run = |case: Option<(&str, Mutate)>| {
            let envs: Vec<(&str, PathBuf)> = good
                .iter()
                .enumerate()
                .map(|(i, &(key, file))| {
                    let mut text = fs::read_to_string(repo_scenarios().join(file)).unwrap();
                    if let Some((broken, mutate)) = case {
                        if broken == key {
                            let changed = mutate(&text);
                            assert_ne!(changed, text, "{key}: the mutation changed nothing");
                            text = changed;
                        }
                    }
                    let path = dir.path().join(format!("{i}.txt"));
                    fs::write(&path, text).unwrap();
                    (key, path)
                })
                .collect();
            fixture_run(script, &envs)
        };
        assert_exit(&run(None), 0, "");
        for &(key, mutate, message) in cases {
            let out = run(Some((key, mutate)));
            let stderr = String::from_utf8_lossy(&out.stderr);
            assert!(
                out.status.code() == Some(1) && stderr.contains(message),
                "{script} with {key} broken, want exit 1 and '{message}', got {:?}:\n{stderr}",
                out.status.code()
            );
        }
    }

    const LANDING: &str = "FIDGET_SCENARIO_AX_LANDING";
    const AFTER: &str = "FIDGET_SCENARIO_AX_AFTER";
    const OPENED: &str = "FIDGET_SCENARIO_OPENED";
    /// OPENED last: the Windows leaves record no hand-off and drop it.
    const LANDING_GOOD: &[(&str, &str)] = &[
        (LANDING, "fixtures/landing-link-click-landing.txt"),
        (AFTER, "fixtures/landing-link-click-landing.txt"),
        (OPENED, "fixtures/landing-link-click-opened.txt"),
    ];
    const LANDING_CASES: &[(&str, Mutate, &str)] = &[
        (
            LANDING,
            |t| drop_lines(t, "Codex needs"),
            "Chat shows no Codex needs npx landing",
        ),
        (
            LANDING,
            |t| t.replace("does not bundle npx.", "does not bundle `npx`."),
            "a backtick is left in the landing copy",
        ),
        (
            LANDING,
            |t| drop_lines(t, "|npx|"),
            "npx is not drawn as its own element",
        ),
        (
            LANDING,
            |t| drop_lines(t, "|https://nodejs.org/|"),
            "https://nodejs.org/ is not drawn as its own element",
        ),
        (
            AFTER,
            |t| drop_lines(t, "Codex needs"),
            "Chat left the landing after the click",
        ),
    ];

    const FAILED: &str = "FIDGET_SCENARIO_AX_FAILED";
    const AX_420: &str = "FIDGET_SCENARIO_AX_420";
    const AX_320: &str = "FIDGET_SCENARIO_AX_320";
    const LIVE: &str = "FIDGET_SCENARIO_AX_LIVE";
    const LAUNCHER_GOOD: &[(&str, &str)] = &[
        (FAILED, "fixtures/launcher-dies-at-startup-420.txt"),
        (AX_420, "fixtures/launcher-dies-at-startup-420.txt"),
        (AX_320, "fixtures/launcher-dies-at-startup-320.txt"),
        (LIVE, "fixtures/launcher-dies-at-startup-live.txt"),
    ];
    const LAUNCHER_CASES: &[(&str, Mutate, &str)] = &[
        (
            FAILED,
            |t| drop_lines(t, "Harness couldn't start"),
            "Chat shows no Harness error landing",
        ),
        (AX_420, |t| drop_lines(t, "frame|"), "420: no frame line"),
        (
            AX_420,
            |t| edit_lines(t, "frame|", |l| reframe(l, "0,0,400,560")),
            "420: frame is 400 wide",
        ),
        (
            AX_320,
            |t| drop_lines(t, "dyld[0]"),
            "320: no box holds 'dyld[0]",
        ),
        (
            AX_320,
            |t| edit_lines(t, "dyld[0]", |l| retype(l, "other")),
            "320: no box holds 'dyld[0]",
        ),
        (
            AX_320,
            |t| drop_lines(t, "script=abort-first"),
            "320: no box holds '|/opt/fidget/",
        ),
        (
            AX_320,
            |t| edit_lines(t, "script=abort-first", |l| retype(l, "other")),
            "320: no box holds '|/opt/fidget/",
        ),
        (
            AX_320,
            |t| edit_lines(t, "dyld[0]", |l| reframe(l, "16,252,400,54")),
            "320: a box ends at 416, past the window edge at 320",
        ),
        (
            AX_320,
            |t| edit_lines(t, "script=abort-first", |l| reframe(l, "16,170,401,54")),
            "320: a box ends at 417, past the window edge at 320",
        ),
        (
            AX_320,
            |t| drop_lines(t, "Nothing can answer yet"),
            "320: no composer",
        ),
        (
            AX_320,
            |t| edit_lines(t, "Nothing can answer yet", |l| retype(l, "label")),
            "320: no composer",
        ),
        (
            AX_320,
            |t| edit_lines(t, "Nothing can answer yet", |l| reframe(l, "8,200,304,40")),
            "320: Error output starts at y 252, under the composer at 200",
        ),
        (
            LIVE,
            |t| drop_lines(t, "fixture-harness.sh ·"),
            "after the re-pick the mind line names no live Harness",
        ),
    ];

    const NEEDS_LOGIN: &str = "FIDGET_SCENARIO_AX_NEEDS_LOGIN";
    const WAITING: &str = "FIDGET_SCENARIO_AX_WAITING";
    const SIGNED_IN: &str = "FIDGET_SCENARIO_AX_SIGNED_IN";
    /// OPENED last: the Windows leaves record no hand-off and drop it.
    const SIGN_IN_GOOD: &[(&str, &str)] = &[
        (NEEDS_LOGIN, "fixtures/sign-in-button-needs-login.txt"),
        (WAITING, "fixtures/sign-in-button-waiting.txt"),
        (SIGNED_IN, "fixtures/sign-in-button-signed-in.txt"),
        (OPENED, "fixtures/sign-in-button-opened.txt"),
    ];
    const SIGN_IN_CASES: &[(&str, Mutate, &str)] = &[
        (
            NEEDS_LOGIN,
            |t| drop_lines(t, "Fake login"),
            "no Fake login button on needs-login",
        ),
        (
            NEEDS_LOGIN,
            |t| edit_lines(t, "Fake login", |l| retype(l, "label")),
            "no Fake login button on needs-login",
        ),
        (
            NEEDS_LOGIN,
            |t| drop_lines(t, "Or run this in a terminal:"),
            "no terminal command hint on needs-login",
        ),
        (
            WAITING,
            |t| drop_lines(t, "Finish signing in"),
            "no waiting line after button press",
        ),
        (
            WAITING,
            |t| edit_lines(t, "Finish signing in", |l| retype(l, "other")),
            "no waiting line after button press",
        ),
        (
            WAITING,
            |t| {
                t.replace(
                    "came from /opt/fidget/scripts/scenarios/fixture-harness.sh,",
                    "came from codex,",
                )
            },
            "waiting line does not name the Harness",
        ),
        (
            SIGNED_IN,
            |t| drop_lines(t, "· session "),
            "mind line does not name the session",
        ),
        (
            SIGNED_IN,
            |t| format!("{t}label|Finish signing in in your browser.|16,140,388,54\n"),
            "waiting line still present after sign-in",
        ),
    ];

    const BEFORE: &str = "FIDGET_SCENARIO_AX_BEFORE";
    const BUBBLE_GOOD: &[(&str, &str)] = &[
        (BEFORE, "fixtures/question-bubble-before.txt"),
        (AFTER, "fixtures/question-bubble-after.txt"),
    ];
    const BUBBLE_CASES: &[(&str, Mutate, &str)] = &[
        (
            BEFORE,
            |t| format!("{t}label|Question for you in the\n"),
            "the question cue showed before the Poke",
        ),
        (
            AFTER,
            |t| drop_lines(t, "Question for you"),
            "the bubble does not read 'Question for you in the'",
        ),
        (
            AFTER,
            |t| edit_lines(t, "Question for you", |l| retype(l, "other")),
            "the bubble does not read 'Question for you in the'",
        ),
        (
            AFTER,
            |t| drop_lines(t, "button|chat"),
            "the bubble has no 'chat' link button",
        ),
        (
            AFTER,
            |t| edit_lines(t, "button|chat", |l| retype(l, "label")),
            "the bubble has no 'chat' link button",
        ),
    ];

    const MENU: &str = "FIDGET_SCENARIO_MENU";
    const MENU_GOOD: &[(&str, &str)] = &[(MENU, "fixtures/control-click-menu-items.txt")];
    const MENU_CASES: &[(&str, Mutate, &str)] = &[
        (MENU, |t| drop_lines(t, "Chat…"), "no 'Chat…' in the menu"),
        (
            MENU,
            |t| t.replace("Settings…", "Settings"),
            "no 'Settings…' in the menu",
        ),
        (MENU, |t| drop_lines(t, "Quit"), "no 'Quit' in the menu"),
    ];

    #[cfg(unix)]
    #[test]
    fn question_bubble_x11_fails_on_each_broken_assertion() {
        fixture_cases("question-bubble.x11.sh", BUBBLE_GOOD, BUBBLE_CASES);
    }

    #[cfg(unix)]
    #[test]
    fn control_click_menu_x11_fails_on_each_broken_assertion() {
        fixture_cases("control-click-menu.x11.sh", MENU_GOOD, MENU_CASES);
    }

    #[cfg(windows)]
    #[test]
    fn question_bubble_win_fails_on_each_broken_assertion() {
        fixture_cases("question-bubble.win.ps1", BUBBLE_GOOD, BUBBLE_CASES);
    }

    #[cfg(windows)]
    #[test]
    fn control_click_menu_win_fails_on_each_broken_assertion() {
        fixture_cases("control-click-menu.win.ps1", MENU_GOOD, MENU_CASES);
    }

    #[cfg(unix)]
    #[test]
    fn landing_link_click_x11_fails_on_each_broken_assertion() {
        let opened: &[(&str, Mutate, &str)] = &[(
            OPENED,
            |_| "https://example.test/\n".into(),
            "the click handed 'https://example.test/' to xdg-open, want https://nodejs.org/",
        )];
        fixture_cases(
            "landing-link-click.x11.sh",
            LANDING_GOOD,
            &[LANDING_CASES, opened].concat(),
        );
    }

    #[cfg(unix)]
    #[test]
    fn launcher_dies_at_startup_x11_fails_on_each_broken_assertion() {
        fixture_cases(
            "launcher-dies-at-startup.x11.sh",
            LAUNCHER_GOOD,
            LAUNCHER_CASES,
        );
    }

    #[cfg(unix)]
    #[test]
    fn sign_in_button_x11_fails_on_each_broken_assertion() {
        let opened: &[(&str, Mutate, &str)] = &[(
            OPENED,
            |_| "https://example.test/other\n".into(),
            "Open handed 'https://example.test/other' to xdg-open",
        )];
        fixture_cases(
            "sign-in-button.x11.sh",
            SIGN_IN_GOOD,
            &[SIGN_IN_CASES, opened].concat(),
        );
    }

    #[cfg(unix)]
    #[test]
    fn x11_leaves_without_a_display_skip() {
        for script in [
            "landing-link-click.x11.sh",
            "launcher-dies-at-startup.x11.sh",
            "sign-in-button.x11.sh",
            "question-bubble.x11.sh",
            "control-click-menu.x11.sh",
            "poke-mid-climb.x11.sh",
        ] {
            assert_exit(&fixture_run(script, &[]), 2, "DISPLAY is unset");
        }
    }

    #[cfg(windows)]
    #[test]
    fn landing_link_click_win_fails_on_each_broken_assertion() {
        let refused: &[(&str, Mutate, &str)] = &[(
            AFTER,
            |t| format!("{t}label|That link did not open: boom.|0,0,1,1\n"),
            "the click did not reach ShellExecuteW",
        )];
        fixture_cases(
            "landing-link-click.win.ps1",
            &LANDING_GOOD[..2],
            &[LANDING_CASES, refused].concat(),
        );
    }

    #[cfg(windows)]
    #[test]
    fn launcher_dies_at_startup_win_fails_on_each_broken_assertion() {
        fixture_cases(
            "launcher-dies-at-startup.win.ps1",
            LAUNCHER_GOOD,
            LAUNCHER_CASES,
        );
    }

    #[cfg(windows)]
    #[test]
    fn sign_in_button_win_fails_on_each_broken_assertion() {
        fixture_cases("sign-in-button.win.ps1", &SIGN_IN_GOOD[..3], SIGN_IN_CASES);
    }

    /// Frame lines after the Poke in `text`, each with its 1-based count.
    /// The fixture traces a frame every 16 ms: frames 1..=143 fall in the
    /// check's 2300 ms pause window, and 163 on in its 2600 ms resume window.
    fn after_poke(text: &str, mut each: impl FnMut(usize, &str) -> Option<String>) -> String {
        let mut frames: Option<usize> = None;
        text.lines()
            .filter_map(|line| {
                if line.starts_with("verbs:") && line.contains("Poke") {
                    frames = Some(0);
                    return Some(line.to_string());
                }
                match frames.as_mut().filter(|_| line.starts_with("frame:")) {
                    Some(n) => {
                        *n += 1;
                        each(*n, line)
                    }
                    None => Some(line.to_string()),
                }
            })
            .map(|line| format!("{line}\n"))
            .collect()
    }

    /// `text` with only the first `keep` frames after the Poke.
    fn keep_after_poke(text: &str, keep: usize) -> String {
        after_poke(text, |n, line| (n <= keep).then(|| line.to_string()))
    }

    /// `text` with the `nth` frame after the Poke put through `edit`.
    fn edit_after_poke(text: &str, nth: usize, edit: fn(&str) -> String) -> String {
        after_poke(text, |n, line| {
            Some(if n == nth {
                edit(line)
            } else {
                line.to_string()
            })
        })
    }

    const TRACE: &str = "FIDGET_SCENARIO_TRACE";
    const CLIMB_GOOD: &[(&str, &str)] = &[(TRACE, "fixtures/poke-mid-climb-trace.txt")];
    const CLIMB_CASES: &[(&str, Mutate, &str)] = &[
        (
            TRACE,
            |t| drop_lines(t, "[Poke]"),
            "no Poke landed on a climbing sprite",
        ),
        (
            TRACE,
            |t| keep_after_poke(t, 20),
            "the trace stops 0.3 s after the Poke",
        ),
        (
            TRACE,
            |t| edit_after_poke(t, 60, |l| l.replace("Climbing", "Falling")),
            "the sprite left the wall during the pause",
        ),
        (
            TRACE,
            |t| edit_after_poke(t, 60, |l| l.replace("pos(0,864)", "pos(0,860)")),
            "the sprite moved during the pause",
        ),
        (
            TRACE,
            |t| edit_after_poke(t, 1, |l| l.replace("react#0", "react#3")),
            "the Poke did not start react over",
        ),
        (
            TRACE,
            |t| edit_after_poke(t, 100, |l| l.replace("climb#0", "climb#3")),
            "the sprite climbed in place during the pause",
        ),
        (
            TRACE,
            |t| keep_after_poke(t, 155),
            "the sprite did not climb on after the cooldown",
        ),
    ];

    #[cfg(unix)]
    #[test]
    fn poke_mid_climb_fails_on_each_broken_assertion() {
        fixture_cases("poke-mid-climb.sh", CLIMB_GOOD, CLIMB_CASES);
    }

    #[cfg(unix)]
    #[test]
    fn poke_mid_climb_x11_fails_on_each_broken_assertion() {
        fixture_cases("poke-mid-climb.x11.sh", CLIMB_GOOD, CLIMB_CASES);
    }

    #[cfg(windows)]
    #[test]
    fn poke_mid_climb_win_fails_on_each_broken_assertion() {
        fixture_cases("poke-mid-climb.win.ps1", CLIMB_GOOD, CLIMB_CASES);
    }
}
