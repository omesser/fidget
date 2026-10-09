//! The attached Harness as the Completer.
//! Spawns the Harness in ACP mode; every wake is one `session/prompt`
//! (ADR-0008, ADR-0018). Auth is the Harness's own. Protocol in `acp_wire.rs`.

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use fidget_core::director::{Completer, Reply, Wake, WakeRequest};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
#[cfg(windows)]
use windows_sys::Win32::System::Threading::{CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW};

use crate::acp_wire::{
    kill_harness_tree, Event, Handshake, McpChoice, McpLaunch, OpenError, SpawnError, TurnError,
    Wire,
};
use crate::action_log;

pub use crate::acp_wire::{
    ElicitationAnswer, ElicitationForm, PermissionAsk, PlanStep, Replayed, SignIn,
};

/// `pub(crate)` so the settings window can name the variable that owns a row.
pub(crate) const VAR: &str = "FIDGET_HARNESS";
/// Where the stdio MCP server binary is, when it is not beside the app.
pub(crate) const MCP_BIN: &str = "FIDGET_MCP_BIN";
/// Spawn `current_dir` and ACP session cwd. Empty is the data folder, not
/// `$HOME`: bare home mixes app files with harness project configs (#782).
pub(crate) const CWD: &str = "FIDGET_HARNESS_CWD";
/// How long an unauthenticated Harness is left alone, in seconds. Named here
/// so the Development row it owns can print it.
pub(crate) const AUTH_RETRY_SECS: &str = "FIDGET_HARNESS_AUTH_RETRY_SECS";
/// How long a Harness `session/prompt` may run, in seconds. Named here so
/// the Development row it owns can print it.
pub(crate) const TURN_TIMEOUT_SECS: &str = "FIDGET_HARNESS_TURN_TIMEOUT";

/// The one file the session survives a restart in.
const SESSION_FILE: &str = "harness-session.json";

/// How long a not-yet-authenticated Harness is left alone before `session/new`
/// is tried again. Long enough not to hammer it, short enough that a user who
/// runs the login command sees the character pick it up without a restart.
const AUTH_RETRY: Duration = Duration::from_secs(60);

/// What an empty auth-retry field means, in seconds.
pub(crate) fn auth_retry_placeholder() -> String {
    AUTH_RETRY.as_secs().to_string()
}

/// How long a Harness `session/prompt` may run before `session/cancel`.
/// Twenty seconds cancelled a web lookup. Forever leaves a hung child.
/// Two minutes covers a lookup and still maps expiry to cancel.
pub(crate) const TURN_TIMEOUT: Duration = Duration::from_secs(120);

/// Settings / `FIDGET_HARNESS_TURN_TIMEOUT` still wins when set.
pub(crate) fn turn_timeout() -> Duration {
    crate::dev_flags::harness_turn_timeout_secs().map_or(TURN_TIMEOUT, Duration::from_secs)
}

pub(crate) fn turn_timeout_placeholder() -> String {
    TURN_TIMEOUT.as_secs().to_string()
}

/// Respawn backoff after a wake the child could not serve. Doubles from the
/// first up to the cap, so a missing binary costs one attempt every five
/// minutes, not a loop.
const BACKOFF_FIRST: Duration = Duration::from_secs(5);
const BACKOFF_CAP: Duration = Duration::from_secs(5 * 60);

/// How long a superseding wake waits for the cancelled turn to hand the lock
/// back, and how often it looks. Generous for a Harness that answers
/// `session/cancel` at all, since the loser only has a log line left to write.
const HANDOVER: Duration = Duration::from_secs(3);
const HANDOVER_POLL: Duration = Duration::from_millis(20);

/// What every caller is told when the child is gone.
pub(crate) const LOST: &str = "harness exited";

/// Prefix on an attach error that is a rejected reasoning effort. The agent's
/// own words follow it, and Chat shows the whole line.
const EFFORT_REJECTED: &str = "reasoning effort: ";

/// How long a withdrawal waits for the `parsed` line that belongs to it. The
/// Shell writes that line frames after the turn ended, so this only has to
/// outlast one frame. Being wrong mislabels one wake and does not leak.
const WITHDRAWAL_GRACE: Duration = Duration::from_secs(30);

/// The stop reason a cancelled turn comes back with. A turn the Completer gave
/// up waiting on is `TurnError::Timeout` instead, so this reason on a turn is
/// always a cancel someone else asked for.
const CANCELLED: &str = "cancelled";

/// Which Harness, and the command line that starts it in ACP mode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Launch {
    pub name: String,
    pub argv: Vec<String>,
}

/// The launch table. `None` is the HTTP Completer path. Unknown values are a
/// command line of the user's own. Keep the README Harness Support table in step.
pub fn launch(value: Option<&str>) -> Option<Launch> {
    let value = value?.trim();
    let (name, argv): (&str, Vec<&str>) = match value {
        "" => return None,
        // `@latest` is load-bearing. npx serves the first cache it built, and
        // the adapter bundles the Claude Code it was built against. A pin
        // drifts the same way, slower. `claude update` hits a different install.
        "claude" => (
            value,
            vec!["npx", "-y", "@agentclientprotocol/claude-agent-acp@latest"],
        ),
        "codex" => (
            value,
            vec!["npx", "-y", "@agentclientprotocol/codex-acp@latest"],
        ),
        // `copilot` alone is the interactive TUI. `--acp` is the documented
        // ACP-over-stdio flag ("Start as Agent Client Protocol server"). The
        // `--stdio` the README used to name is absent from `--help` and
        // changes nothing: with or without it, `initialize` answers the same.
        "copilot" => (value, vec!["copilot", "--acp"]),
        // `acp` is absent from `cursor-agent --help`, which lists `agent`,
        // `login` and `mcp`. It answers `initialize` all the same.
        "cursor-agent" => (value, vec!["cursor-agent", "acp"]),
        // `grok` alone is the interactive TUI. The ACP agent is the
        // subcommand. Without this arm the escape hatch below launches that
        // TUI on stdio and the attach times out.
        "grok" => (value, vec!["grok", "agent", "stdio"]),
        // `goose` alone is the interactive CLI. ACP over stdio is the `acp`
        // subcommand; the ACP registry publishes the same argv.
        "goose" => (value, vec!["goose", "acp"]),
        "hermes" => (value, vec!["hermes", "acp"]),
        "opencode" => (value, vec!["opencode", "acp"]),
        "pi" => (value, vec!["npx", "-y", "pi-acp@latest"]),
        // Google's own server, not `agy`, which has no ACP mode. The argv is
        // the ACP registry's per platform, and `localharness_external` from
        // the same archive has to sit beside it.
        "antigravity" => (value, antigravity_argv()),
        custom => {
            let argv: Vec<&str> = custom.split_whitespace().collect();
            (argv[0], argv)
        }
    };
    Some(Launch {
        name: name.to_string(),
        argv: argv.into_iter().map(str::to_string).collect(),
    })
}

fn antigravity_argv() -> Vec<&'static str> {
    if cfg!(windows) {
        vec!["agy_acp_server.exe"]
    } else if cfg!(target_os = "linux") {
        vec!["agy_acp_server.par", "--uid="]
    } else {
        vec!["agy_acp_server.par"]
    }
}

/// `cursor-agent` loads MCP servers from its own project config and not from
/// `session/new` (#1020). The preset and a custom `cursor-agent …` line both
/// carry that name.
fn takes_cursor_config(launch: &Launch) -> bool {
    launch.name == "cursor-agent"
}

/// The exported variable, else the Completer source row Settings saved.
/// Exported-and-empty is not unexported. `FIDGET_HARNESS=` is the kill
/// switch, and falling through would spawn the Harness the export cleared.
pub fn from_settings(saved: Option<&str>) -> Option<Launch> {
    match std::env::var(VAR) {
        Ok(exported) => launch(Some(&exported)),
        Err(_) => launch(saved),
    }
}

impl Launch {
    fn codex_mcp_config(&self) -> Option<(String, String)> {
        if self.name != "codex" {
            return None;
        }
        let endpoint = crate::mcp_http::endpoint()?;
        let config = codex_fidget_config(std::env::var("CODEX_CONFIG").ok().as_deref(), &endpoint)?;
        let (_, token) = endpoint.registration();
        Some((config, token))
    }

    /// The version flag for this launcher. npx presets probe npx itself; first-party
    /// CLIs probe their own binary.
    fn version_flag(&self) -> &str {
        match self.argv[0].as_str() {
            "npx" => "--version",
            "copilot" => "--version",
            "cursor-agent" => "--version",
            "grok" => "--version",
            "goose" => "--version",
            "hermes" => "--version",
            "opencode" => "--version",
            _ => {
                if self.argv[0].ends_with("agy_acp_server.par")
                    || self.argv[0].ends_with("agy_acp_server.exe")
                {
                    "--help"
                } else {
                    "--version"
                }
            }
        }
    }

    /// The child, inheriting our environment. No provider key in it, because
    /// one overrides a subscription login with no prompt. No `CLAUDE_CONFIG_DIR`
    /// and no `--bare`, because both cut the child off from the login the user
    /// already has. The `pi` preset adds the loopback URL and token, because
    /// `pi-acp` forwards `process.env` to `pi` and the project file names those
    /// variables instead of the values. Own process group once `own_interrupt`
    /// has taken Ctrl+C, so a SIGINT on `cargo run` misses it.
    fn command(&self, cwd: &AttachCwd, mcp_available: bool) -> Command {
        let mut command = Command::new(resolved_program(&self.argv[0], None));
        let endpoint = crate::mcp_http::endpoint();
        let mut args = self.argv[1..].to_vec();
        if endpoint.is_some() && mcp_available {
            match self.name.as_str() {
                "copilot" => args.push("--allow-tool=fidget".into()),
                "grok" => {
                    args.insert(0, "MCPTool(fidget__*)".into());
                    args.insert(0, "--allow".into());
                }
                _ => {}
            }
        }
        command.args(args).current_dir(cwd.as_path());
        if self.name == "pi" {
            if let Some(endpoint) = &endpoint {
                let (url, token) = endpoint.registration();
                command.env(fidget_mcp_server::URL_VAR, url);
                command.env(fidget_mcp_server::TOKEN_VAR, token);
            }
        }
        if self.name == "opencode" && endpoint.is_some() && mcp_available {
            if let Some(config) = opencode_fidget_permissions(
                std::env::var("OPENCODE_CONFIG_CONTENT").ok().as_deref(),
            ) {
                command.env("OPENCODE_CONFIG_CONTENT", config);
            }
        }
        if let Some((config, token)) = self.codex_mcp_config() {
            command.env("CODEX_CONFIG", config);
            command.env(fidget_mcp_server::TOKEN_VAR, token);
        }
        isolate_from_interrupt(&mut command);
        command
    }

    fn line(&self) -> String {
        self.argv.join(" ")
    }
}

fn codex_fidget_config(
    existing: Option<&str>,
    endpoint: &crate::mcp_http::Endpoint,
) -> Option<String> {
    let server = format!("fidget_attached_{}", std::process::id());
    let mut config: Value =
        existing.map_or_else(|| Some(json!({})), |text| serde_json::from_str(text).ok())?;
    let servers = config
        .as_object_mut()?
        .entry("mcp_servers")
        .or_insert_with(|| json!({}))
        .as_object_mut()?;
    if servers.contains_key(&server) {
        return None;
    }
    servers.insert(
        server,
        json!({
            "url": endpoint.url,
            "bearer_token_env_var": fidget_mcp_server::TOKEN_VAR,
            "default_tools_approval_mode": "approve"
        }),
    );
    Some(config.to_string())
}

fn opencode_fidget_permissions(existing: Option<&str>) -> Option<String> {
    let mut config: Value =
        existing.map_or_else(|| Some(json!({})), |text| serde_json::from_str(text).ok())?;
    let permissions = config
        .as_object_mut()?
        .entry("permission")
        .or_insert_with(|| json!({}));
    if permissions.is_string() {
        let default = permissions.take();
        *permissions = json!({"*": default});
    }
    let permissions = permissions.as_object_mut()?;
    if permissions.contains_key("fidget_*") {
        return Some(config.to_string());
    }
    for tool in fidget_core::dispatch::list_tools() {
        permissions
            .entry(format!("fidget_{}", tool.name))
            .or_insert_with(|| json!("allow"));
    }
    Some(config.to_string())
}

/// Spawn `current_dir` and ACP session cwd. User-owned. Never `create_dir_all`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct AttachCwd(PathBuf);

/// Directory we own. `create_dir_all` OK. Session file and Action Log.
struct SessionDataDir(PathBuf);

/// Retarget identity. Launch alone would Stand on a cwd-only edit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Target {
    launch: Launch,
    cwd: Result<AttachCwd, CwdError>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum CwdError {
    Relative(PathBuf),
}

impl fmt::Display for CwdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CwdError::Relative(path) => {
                write!(f, "relative path {} is not a project", path.display())
            }
        }
    }
}

fn attach_cwd_display(cwd: &Result<AttachCwd, CwdError>) -> String {
    match cwd {
        Ok(cwd) => cwd.as_path().display().to_string(),
        Err(error) => error.to_string(),
    }
}

/// Where the Harness runs when the Working directory row is blank, for that
/// row's placeholder. Resolved rather than described, so the row shows the
/// path instead of leaving the user to know it (#913). Reads
/// `FIDGET_HARNESS_CWD` first, because a row the environment owns runs
/// somewhere else again.
pub(crate) fn attach_cwd_placeholder() -> String {
    project_dir_label("")
}

/// The path a project `.mcp.json` would be written under, or the reason that
/// path is not a directory spawn would accept. Does not create anything.
pub(crate) fn project_dir_label(raw: &str) -> String {
    attach_cwd_display(&AttachCwd::resolve(&crate::model::env_or_file(CWD, raw)))
}

/// The directory spawn will use. The data directory is created, because a
/// first launch has not got it yet and that folder is ours. A user path is
/// not created.
pub(crate) fn attach_dir(raw: &str) -> Result<std::path::PathBuf, String> {
    let cwd = AttachCwd::resolve(&crate::model::env_or_file(CWD, raw))
        .map_err(|error| error.to_string())?;
    if cwd.as_path() == fidget_core::memory::data_dir() {
        std::fs::create_dir_all(cwd.as_path())
            .map_err(|error| format!("{}: {error}", cwd.as_path().display()))?;
    }
    cwd.checked().map_err(|error| match error {
        SpawnError::Failed(why) => why,
        _ => "missing".to_string(),
    })?;
    Ok(cwd.as_path().to_path_buf())
}

impl AttachCwd {
    fn resolve(raw: &str) -> Result<Self, CwdError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Ok(Self(fidget_core::memory::data_dir()));
        }
        let path = PathBuf::from(trimmed);
        if !path.is_absolute() {
            return Err(CwdError::Relative(path));
        }
        Ok(Self(path))
    }

    fn as_path(&self) -> &Path {
        &self.0
    }

    /// Existing directory, else spawn names this path. Do not create it.
    fn checked(&self) -> Result<(), SpawnError> {
        if self.0.is_dir() {
            Ok(())
        } else {
            Err(SpawnError::Failed(self.0.display().to_string()))
        }
    }
}

impl SessionDataDir {
    fn app() -> Self {
        Self(fidget_core::memory::data_dir())
    }

    fn probe() -> Self {
        Self(fidget_core::memory::data_dir().join("probe"))
    }

    #[cfg(test)]
    fn at(path: PathBuf) -> Self {
        Self(path)
    }

    fn ensure(&self) -> Result<(), SpawnError> {
        std::fs::create_dir_all(&self.0)
            .map_err(|why| SpawnError::Failed(format!("{}: {why}", self.0.display())))
    }

    fn as_path(&self) -> &Path {
        &self.0
    }

    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Target {
    pub(crate) fn from_settings(source: Option<&str>, cwd_row: &str) -> Option<Self> {
        from_settings(source).map(|launch| Self {
            launch,
            cwd: AttachCwd::resolve(&crate::model::env_or_file(CWD, cwd_row)),
        })
    }
}

/// Tests isolate without installing a `ctrlc` handler. Production stays
/// false until `own_interrupt`. A failed handler must not orphan a tree
/// Ctrl+C can no longer reach.
static INTERRUPT_OWNED: AtomicBool = AtomicBool::new(cfg!(test));
static INTERRUPT_QUITTING: AtomicBool = AtomicBool::new(false);

/// Isolation is on. Ctrl+C is ours, so the child may leave this process group.
pub fn own_interrupt() {
    INTERRUPT_OWNED.store(true, Ordering::SeqCst);
}

/// True on the second Ctrl+C, which should `exit` rather than nest `shutdown`.
pub fn interrupt_already_quitting() -> bool {
    INTERRUPT_QUITTING.swap(true, Ordering::SeqCst)
}

fn isolate_from_interrupt(command: &mut Command) {
    apply_isolation(command, INTERRUPT_OWNED.load(Ordering::SeqCst));
}

/// Version probe timeout. Short enough not to block the UI thread, long
/// enough for a cold npx or a slow network.
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

/// Probe outcome: the three states before attach.
#[derive(Debug)]
enum ProbeOutcome {
    NotFound,
    Unhealthy(LaunchFailure),
    Healthy,
}

/// Probe the launcher with a version check before spawning. Returns NotFound
/// if the binary is not on PATH, Unhealthy if it exits nonzero, times out, or
/// produces no output, and Healthy otherwise. Runs on the preflight thread.
fn probe_launcher(launch: &Launch) -> ProbeOutcome {
    probe_launcher_within(launch, PROBE_TIMEOUT)
}

/// `probe_launcher` with the timeout as a parameter, so a test need not wait out
/// the production one.
fn probe_launcher_within(launch: &Launch, timeout: Duration) -> ProbeOutcome {
    let flag = launch.version_flag();
    let node_check = (launch.argv[0] == "npx").then(|| "node --version".to_string());
    let refused = |reason: String, output: String| {
        ProbeOutcome::Unhealthy(LaunchFailure {
            command: Some(format!("{} {flag}", launch.argv[0])),
            reason,
            output,
            node_check: node_check.clone(),
        })
    };
    let output = match timed_command(&launch.argv[0], &[flag], timeout) {
        Ok(output) => output,
        Err(TimedCommandError::NotFound) => return ProbeOutcome::NotFound,
        Err(TimedCommandError::Spawn(error)) => {
            return refused(format!("could not be run: {error}"), String::new());
        }
        Err(TimedCommandError::Wait(error)) => {
            return refused(format!("could not be monitored: {error}"), String::new());
        }
        Err(TimedCommandError::TimedOut) => {
            let secs = timeout.as_secs_f32();
            return refused(format!("timed out after {secs:.1}s"), String::new());
        }
    };

    let printed = [&output.stdout, &output.stderr]
        .map(|bytes| String::from_utf8_lossy(bytes).trim().to_string())
        .into_iter()
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n");

    if !output.status.success() {
        return refused(format!("exited with {}", output.status), printed);
    }

    if printed.is_empty() {
        return refused("produced no output".to_string(), printed);
    }

    ProbeOutcome::Healthy
}

enum TimedCommandError {
    NotFound,
    Spawn(std::io::Error),
    Wait(std::io::Error),
    TimedOut,
}

fn timed_command(
    program: &str,
    args: &[&str],
    timeout: Duration,
) -> Result<std::process::Output, TimedCommandError> {
    let mut command = Command::new(resolved_program(program, None));
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    without_console_window(&mut command);
    let child = match command.spawn() {
        Ok(child) => child,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(TimedCommandError::NotFound);
        }
        Err(error) => return Err(TimedCommandError::Spawn(error)),
    };
    let pid = child.id();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    match rx.recv_timeout(timeout) {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(error)) => Err(TimedCommandError::Wait(error)),
        Err(_) => {
            kill_harness_tree(pid);
            Err(TimedCommandError::TimedOut)
        }
    }
}

/// On Windows, resolves stand-in executables (.cmd, .bat, .exe) to absolute
/// paths because CreateProcess with a bare name + PATH override does not
/// reliably find them the way the shell does. Rust's Command wraps .cmd/.bat
/// via ComSpec/cmd.exe automatically when given an absolute path.
fn resolved_program(program: &str, path_override: Option<&Path>) -> OsString {
    windows_program(program, path_override).unwrap_or_else(|| OsString::from(program))
}

#[cfg(windows)]
fn windows_program(program: &str, path_override: Option<&Path>) -> Option<OsString> {
    if program.contains(['/', '\\']) {
        return None;
    }
    let dirs: Vec<PathBuf> = match path_override {
        Some(dir) => vec![dir.to_path_buf()],
        None => std::env::var_os("PATH")
            .map(|path| std::env::split_paths(&path).collect())
            .unwrap_or_default(),
    };
    for dir in dirs {
        for candidate in windows_candidates(&dir, program) {
            if candidate.is_file() {
                return Some(candidate.into_os_string());
            }
        }
    }
    None
}

#[cfg(windows)]
fn windows_candidates(dir: &Path, program: &str) -> Vec<PathBuf> {
    let base = dir.join(program);
    if Path::new(program).extension().is_some() {
        return vec![base];
    }
    ["exe", "cmd", "bat"]
        .into_iter()
        .map(|extension| base.with_extension(extension))
        .collect()
}

#[cfg(not(windows))]
fn windows_program(_program: &str, _path_override: Option<&Path>) -> Option<OsString> {
    None
}

/// Spawns the child with no console window. A release build has no console to
/// share, so on Windows a console child would otherwise open a window of its own.
pub(crate) fn without_console_window(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    let _ = command;
}

fn apply_isolation(command: &mut Command, isolate: bool) {
    #[cfg(unix)]
    if isolate {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // The Job Object is created and assigned at spawn time in
        // `acp_wire::windows_job::spawn_in_job`, so descendants die on
        // shutdown. Isolated, it is also a new process group, so Ctrl+C misses it.
        let group = if isolate { CREATE_NEW_PROCESS_GROUP } else { 0 };
        command.creation_flags(group | CREATE_NO_WINDOW);
    }
}

#[cfg(all(test, windows))]
mod console_window_tests {
    use std::os::windows::process::CommandExt;
    use std::time::Duration;

    const PROBE: &str = "FIDGET_CONSOLE_WINDOW_PROBE";

    fn rerun(name: &str) -> [String; 3] {
        let module = module_path!();
        let rest = module.split_once("::").map_or(module, |(_, rest)| rest);
        [
            format!("{rest}::{name}"),
            "--exact".into(),
            "--test-threads=1".into(),
        ]
    }

    /// Runs only when re-executed. Exits 0 when this process shows no console window.
    #[test]
    fn console_window_child() {
        if std::env::var_os(PROBE).is_none() {
            return;
        }
        use windows_sys::Win32::System::Console::GetConsoleWindow;
        use windows_sys::Win32::UI::WindowsAndMessaging::IsWindowVisible;
        // SAFETY: both take no pointer this process owns, and accept null.
        let shown = unsafe { IsWindowVisible(GetConsoleWindow()) != 0 };
        std::process::exit(if shown { 9 } else { 0 });
    }

    /// Runs only when re-executed with no console, as a release exe has none, and
    /// spawns the child the way a version probe does.
    #[test]
    fn console_window_parent() {
        if std::env::var_os(PROBE).is_none() {
            return;
        }
        let exe = std::env::current_exe().expect("test binary");
        let args = rerun("console_window_child");
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let code = match super::timed_command(
            exe.to_str().expect("utf-8 path"),
            &args,
            Duration::from_secs(60),
        ) {
            Ok(output) => output.status.code().unwrap_or(8),
            Err(_) => 7,
        };
        std::process::exit(code);
    }

    #[test]
    fn a_console_child_of_a_process_with_no_console_opens_no_window() {
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
            .env(PROBE, "1")
            .args(rerun("console_window_parent"))
            .creation_flags(DETACHED_PROCESS)
            .output()
            .expect("parent probe");
        assert_eq!(
            output.status.code(),
            Some(0),
            "9 is a visible console window on the child\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

/// What Settings and the Chat surface can say about the attachment.
#[derive(Clone, Debug, Default, Serialize)]
pub struct HarnessInspect {
    pub name: String,
    pub command: String,
    pub agent: Option<String>,
    pub session_id: Option<String>,
    /// Attached but not authenticated. The one command that fixes it.
    pub login: Option<String>,
    /// Whether `initialize` offered HTTP MCP.
    pub mcp_http: bool,
    pub alive: bool,
    /// The binary `PATH` has not got, when that is why nothing is running.
    /// Told apart from a child that died because no respawn mends it and the
    /// sentence a user needs is a different one.
    pub missing: Option<String>,
    /// Whether ACP handshake/spawn is in progress. Gates chat until ready or failed.
    pub initializing: bool,
    /// Why the last spawn gave no wire, when the launcher was there to run.
    pub failed: Option<LaunchFailure>,
    /// What the last turn came back with, when it came back with an error, and
    /// `None` once a turn answers. A Harness that refuses every prompt is
    /// attached, alive, and authenticated, so nothing else here tells it apart.
    pub last_error: Option<String>,
    /// The Harness's own words when it failed the last turn, as against an
    /// error the Shell names. Chat boxes them under the Harness's name.
    pub turn_failure: Option<String>,
    /// Why preflight refused a launcher that is there: its version check
    /// exited nonzero, timed out, or printed nothing. Chat boxes it as `failed`.
    pub unhealthy: Option<LaunchFailure>,
}

/// Why a launcher that was there gave no wire. Chat draws the parts apart:
/// the command and what it printed are raw text to read or paste, and the
/// reason is prose. Settings and the wake take `sentence`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct LaunchFailure {
    /// The launch line as run, when the launch itself failed. `None` for a
    /// failure before it, such as a working folder that is not there.
    pub command: Option<String>,
    /// What went wrong, as a clause after the command: "exited before
    /// initialize, signal: 6 (SIGABRT)".
    pub reason: String,
    /// The tail of what the launcher printed before it died: stderr, and its
    /// stdout before `initialize`. Empty when it printed nothing.
    pub output: String,
    /// For an `npx` launcher, the command that checks Node.js starts.
    pub node_check: Option<String>,
}

impl LaunchFailure {
    /// A failure with no launch line and nothing printed.
    fn before_launch(reason: String) -> Self {
        Self {
            reason,
            ..Self::default()
        }
    }

    /// The one sentence for surfaces that show a line of text.
    pub fn sentence(&self) -> String {
        let reason = self.reason.trim_end_matches('.');
        let mut sentence = match &self.command {
            Some(command) => format!("`{command}` {reason}."),
            None => format!("{reason}."),
        };
        if let Some(check) = &self.node_check {
            sentence.push_str(&format!(
                " `npx` runs on Node.js: run `{check}` in a terminal to check that it starts."
            ));
        }
        sentence
    }
}

/// One remembered ACP session, keyed so a restart can load it for that
/// identity and no other.
#[derive(Clone, Debug, Deserialize, Serialize)]
struct SavedSlot {
    instance: String,
    character: String,
    /// Blank-AI mode's slot, kept apart from the shaped one. Defaulted so a
    /// file written before the mode existed loads as a session opened with a
    /// Character Prompt in it.
    #[serde(default)]
    blank: bool,
    session_id: String,
}

/// What `harness-session.json` holds. `session_id` is the older single
/// pointer, kept so an existing file still parses. That leftover is not
/// applied to any Character Instance.
#[derive(Deserialize, Serialize)]
struct SavedSession {
    harness: String,
    agent: Option<String>,
    #[serde(default)]
    sessions: Vec<SavedSlot>,
    #[serde(default, skip_serializing)]
    session_id: Option<String>,
}

impl SavedSlot {
    /// The identity half of a remembered slot. One place, so the three sites
    /// that match a slot against a key cannot drift apart when the key gains
    /// a field.
    fn key(&self) -> SessionKey {
        SessionKey {
            instance: self.instance.clone(),
            character: self.character.clone(),
            blank: self.blank,
        }
    }
}

/// Instance plus Character. Two Instances of one Character do not share, and
/// a retarget is a different Character Prompt (ADR-0012). Blank-AI mode joins
/// them so a shaped session cannot keep answering after the mode switches on.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct SessionKey {
    pub(crate) instance: String,
    pub(crate) character: String,
    pub(crate) blank: bool,
}

impl SessionKey {
    fn from_request(request: &WakeRequest) -> Self {
        Self {
            instance: request.instance.clone(),
            character: request.character.clone(),
            blank: request.blank,
        }
    }
}

/// Between-turn agent text from a Harness-initiated turn.
#[derive(Clone, Debug)]
pub struct InboundWake {
    pub instance: String,
    pub speech: String,
}

/// A loaded session's replay, for its Instance's Chat. Not a wake: nothing
/// here addresses the Instance or touches Pace.
#[derive(Clone, Debug)]
pub struct Restored {
    pub instance: String,
    pub history: Vec<Replayed>,
}

/// Whose Chat draws a forwarded ask or form.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Owner {
    /// The Instance that owes the answer. Only its Chat draws the row.
    Instance(String),
    /// Nobody can be named, so every Chat draws the row and any can answer.
    EveryChat,
}

/// What the session on the wire tells the Chat surface, live or on replay.
/// `Settled` exists because an ask can be drawn on more than one surface and
/// only one takes the click. Without it the others keep offering live buttons.
#[derive(Debug)]
pub enum Forwarded {
    /// `owner` is the asking session's Instance, or the turn holder for a
    /// form no session scopes. `EveryChat` when neither is known.
    Ask {
        owner: Owner,
        ask: PermissionAsk,
    },
    /// Addressed as `Ask` is.
    Form {
        owner: Owner,
        form: ElicitationForm,
    },
    /// A request that is no longer answerable, and the option that won it.
    /// `None` when nothing was picked and the turn simply ended.
    Settled {
        request: String,
        option: Option<String>,
    },
    /// The Harness's thinking so far, blank lines included: the whole
    /// thought, which the Chat surface's Thinking row redraws. ADR-0034.
    /// `instance` is whose session thought it.
    Thought {
        instance: String,
        line: String,
    },
    /// The agent's plan, replacing whatever the surface holds. Empty ends it.
    /// `instance` is whose session planned.
    Plan {
        instance: String,
        steps: Vec<PlanStep>,
    },
    InboundWake(InboundWake),
    Restored(Restored),
    /// The attachment moved: preflight finished, a session opened, or a login
    /// went missing or came back mid-session (#991). Chat's first ReloadChat
    /// races preflight and `session/new`, so a missing launcher would never
    /// reach the landing (#726) and the header would keep "no session yet".
    AttachSettled,
}

type Forward = Box<dyn Fn(Forwarded) + Send + Sync>;

/// The attached Harness. One child, one ACP connection, and one conversation
/// per Character Instance identity.
pub struct Session {
    launch: Launch,
    cwd: Result<AttachCwd, CwdError>,
    data: SessionDataDir,
    forward: Arc<Forward>,
    timeout: Duration,
    auth_retry: Duration,
    backoff_first: Duration,
    /// One prompt in flight. A newer wake cancels the turn holding this and
    /// takes it; only the wake that may not do that is told "harness busy",
    /// and nothing is ever queued (ADR-0008, ADR-0016).
    turn: Mutex<()>,
    /// Whether the turn holding `turn` answers something the user did. Beside
    /// the lock rather than inside it because it is read exactly when the lock
    /// cannot be taken, which is the moment the ordering rule is decided.
    serving_reactive: AtomicBool,
    /// Instance whose wake holds `turn`. One child serves every Instance
    /// (ADR-0008) and `wire.cancel` names no session, so a cancel without this
    /// name lands on whichever character is mid-reply.
    /// Shared with the wire's reader, which names it as the owner of a form
    /// no session scopes.
    serving_instance: Arc<Mutex<Option<String>>>,
    /// Questions the Harness has put to the user that nothing has settled,
    /// by request id to the asking session (or `Turn` when none). The Instance
    /// is resolved through `owners` when `awaiting_user` reads, so an ask that
    /// arrives before open records its owner still resolves once it does.
    /// Raised and lowered by the wire's own events, which `end_turn` balances
    /// by settling everything it still holds. Shared with the wire's reader.
    asked: Arc<Mutex<HashMap<String, Asker>>>,
    /// Instance behind each session id this child opened, so the wire's
    /// reader thread can name whose a thought or an ask is without taking
    /// `state`. Filled at open, so work between turns has an owner too.
    owners: Arc<Mutex<HashMap<String, String>>>,
    /// The Instance a cancel has just gone out for, until the turn it cancels
    /// names itself. One slot, because one prompt is in flight at a time.
    withdrawing: Mutex<Option<String>>,
    /// Withdrawn turn to the Instance whose wake took the session, until the
    /// `parsed` line for that wake. Keyed by loser so a later cancel cannot
    /// pair a withdrawn `turn` line with a `parsed` line that says failed.
    withdrawn: Mutex<HashMap<String, (String, Instant)>>,
    /// Bookkeeping only. Chat and the frame loop take this while a handshake
    /// is in flight, so the blocking hop lives on `attach_gate` instead.
    state: Mutex<State>,
    /// The session file. Not `state`: a save must not stall Chat or the frame
    /// loop, and those two locks are never held together.
    session_file: Mutex<()>,
    /// One spawn or session open at a time. Not taken by Chat or the frame
    /// loop: those wait on `state`, and this one is held across the wire.
    attach_gate: Mutex<()>,
    /// Separate from `state` so `shutdown` never waits on an attach in flight.
    wire: Mutex<Option<Arc<Wire>>>,
    inspect: Mutex<HarnessInspect>,
    /// Off drops the handle while `spawn_preflight` may still be opening a
    /// wire. `shutdown` clears this first so a spawn that lands afterwards
    /// kills the child instead of storing it.
    wanted: AtomicBool,
    /// `authenticate` is in flight. A second click is refused, and the flag
    /// clears on the way out so a panic cannot leave the landing stuck.
    signing_in: AtomicBool,
    /// What the first attach wrote into `cursor-agent`'s config, for
    /// `shutdown` to take back. The first record is kept across respawns:
    /// it is the one that knows what did not exist before us.
    cursor: Mutex<Option<crate::cursor_mcp::Installed>>,
}

/// What a Harness takes only when it opens a conversation. A conversation
/// opened on other inputs is not asked again: its next wake opens a new one.
#[derive(Clone, PartialEq, Eq)]
struct OpenInputs {
    /// The AI-tab model, with `FIDGET_DIRECTOR_MODEL` winning. Blank stays
    /// blank. `apply_completer` omits it, the same way HTTP omits `model`.
    model: String,
    effort: Option<String>,
}

struct OpenedSession {
    id: String,
    opened_on: OpenInputs,
    /// Whether this id came from `session/load` and has not served a turn
    /// yet. That is the one condition `reopen_loaded` answers to.
    loaded: bool,
}

#[derive(Default)]
struct State {
    sessions: HashMap<SessionKey, OpenedSession>,
    handshake: Handshake,
    login: Option<String>,
    auth_tried: Option<Instant>,
    spawn_failures: u32,
    spawn_wait_until: Option<Instant>,
    /// Per Instance, bumped when its conversation is dropped. An open that
    /// started earlier must not store its id afterwards.
    conversation_gen: HashMap<String, u64>,
    /// Every session id opened in this run. Kept across respawns: a respawn
    /// loads the same id, and its replay is what Chat already holds.
    opened_ids: HashSet<String>,
}

impl State {
    fn conversation_generation(&self, instance: &str) -> u64 {
        self.conversation_gen.get(instance).copied().unwrap_or(0)
    }

    /// The child answered, so no earlier death is a death in a row any more.
    /// Every outcome but a loss proves it. A stop reason, and even a turn we
    /// gave up waiting on, came back from a process that is still there.
    fn answered(&mut self) {
        self.spawn_failures = 0;
        self.spawn_wait_until = None;
    }
}

/// Names the Instance whose wake holds the turn lock, for exactly as long as
/// it holds it. A save reads that name before cancelling. A leftover slot
/// would aim the cancel at the turn that replaced it.
struct Serving<'a>(&'a Session);

impl<'a> Serving<'a> {
    fn new(session: &'a Session, instance: &str) -> Self {
        if let Ok(mut slot) = session.serving_instance.lock() {
            *slot = Some(instance.to_string());
        }
        Self(session)
    }
}

impl Drop for Serving<'_> {
    fn drop(&mut self) {
        if let Ok(mut slot) = self.0.serving_instance.lock() {
            *slot = None;
        }
    }
}

/// Clears `signing_in` however `sign_in` leaves, panic included.
struct SignInFlight<'a>(&'a AtomicBool);

impl Drop for SignInFlight<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

impl Session {
    fn new(
        launch: Launch,
        cwd: Result<AttachCwd, CwdError>,
        data: SessionDataDir,
        forward: Arc<Forward>,
    ) -> Self {
        let inspect = HarnessInspect {
            name: launch.name.clone(),
            command: launch.line(),
            ..Default::default()
        };
        Self {
            launch,
            cwd,
            data,
            forward,
            timeout: turn_timeout(),
            auth_retry: crate::dev_flags::harness_auth_retry_secs()
                .map_or(AUTH_RETRY, Duration::from_secs),
            backoff_first: BACKOFF_FIRST,
            turn: Mutex::new(()),
            serving_reactive: AtomicBool::new(false),
            serving_instance: Arc::default(),
            asked: Arc::default(),
            owners: Arc::default(),
            withdrawing: Mutex::new(None),
            withdrawn: Mutex::new(HashMap::new()),
            state: Mutex::new(State::default()),
            session_file: Mutex::new(()),
            attach_gate: Mutex::new(()),
            wire: Mutex::new(None),
            inspect: Mutex::new(inspect),
            wanted: AtomicBool::new(true),
            signing_in: AtomicBool::new(false),
            cursor: Mutex::new(None),
        }
    }

    #[cfg(test)]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    #[cfg(test)]
    pub fn with_auth_retry(mut self, retry: Duration) -> Self {
        self.auth_retry = retry;
        self
    }

    #[cfg(test)]
    pub fn with_backoff(mut self, first: Duration) -> Self {
        self.backoff_first = first;
        self
    }

    fn target(&self) -> Target {
        Target {
            launch: self.launch.clone(),
            cwd: self.cwd.clone(),
        }
    }

    fn backoff(&self, failures: u32) -> Duration {
        self.backoff_first
            .saturating_mul(1u32 << failures.saturating_sub(1).min(16))
            .min(BACKOFF_CAP)
    }

    /// One counter and one offset for every way a wake goes unserved. A spawn
    /// that never started, an `initialize` that never came back, and a child
    /// that died under a turn must not price the same failure twice.
    fn charge_loss(&self, state: &mut State) {
        state.spawn_failures += 1;
        state.spawn_wait_until = Some(Instant::now() + self.backoff(state.spawn_failures));
    }

    pub fn inspect(&self) -> HarnessInspect {
        self.inspect
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    fn update_inspect(&self, apply: impl FnOnce(&mut HarnessInspect)) {
        if let Ok(mut inspect) = self.inspect.lock() {
            apply(&mut inspect);
        }
    }

    /// `initialize` and `session` methods get at least a minute whatever the
    /// turn budget is. A cold `npx` first run downloads the adapter and its CLI,
    /// 12.5 to 18 s on a fast Mac and link (#1147); a minute leaves room for slower ones.
    fn attach_timeout(&self) -> Duration {
        self.timeout.max(Duration::from_secs(60))
    }

    /// Where the Harness should be by the time the first wake arrives. Spawned
    /// so startup does not wait on `npx`; the outcome is one stderr line.
    pub fn spawn_preflight(self: &Arc<Self>) {
        self.update_inspect(|inspect| {
            inspect.initializing = true;
            inspect.failed = None;
            inspect.unhealthy = None;
        });
        let session = Arc::clone(self);
        thread::spawn(move || {
            // A preflight is someone asking now: startup, a new row, or a
            // re-pick. The backoff is for wakes, which nobody asked for.
            if let Ok(mut state) = session.state.lock() {
                state.spawn_wait_until = None;
            }

            match probe_launcher(&session.launch) {
                ProbeOutcome::NotFound => {
                    let why = session.note_missing();
                    eprintln!("harness: {why}; StaticDirector is in force until it is installed");
                    (session.forward)(Forwarded::AttachSettled);
                    return;
                }
                ProbeOutcome::Unhealthy(failure) => {
                    let sentence = failure.sentence();
                    session.update_inspect(|inspect| {
                        inspect.alive = false;
                        inspect.initializing = false;
                        inspect.unhealthy = Some(failure);
                    });
                    eprintln!("harness: {sentence} StaticDirector is in force until it is fixed");
                    (session.forward)(Forwarded::AttachSettled);
                    return;
                }
                ProbeOutcome::Healthy => {}
            }

            let attached = session.attach(None);
            match &attached {
                Ok(_) => eprintln!("harness: {} attached", session.launch.name),
                Err(why) => {
                    let why = why.trim_end_matches('.');
                    eprintln!("harness: {why}; StaticDirector is in force until it answers")
                }
            }
            if attached.is_err() {
                session.update_inspect(|inspect| inspect.initializing = false);
            }
            // Chat's ReloadChat after a pick races this thread. A second
            // opening is how `inspect.missing` reaches the landing (#726).
            (session.forward)(Forwarded::AttachSettled);
        });
    }

    /// Connect on the Harness already attached. Down after a failed launch or
    /// a dead child is asked again. Up but not signed in opens `key` afresh, so
    /// a terminal login is proved now and not at the next wake. Answering stands.
    pub(crate) fn repick(self: &Arc<Self>, key: Option<SessionKey>) -> Result<(), String> {
        let inspect = self.inspect();
        if inspect.alive && inspect.login.is_some() {
            let key = key.ok_or_else(|| "unknown instance".to_string())?;
            let wire = self.current_wire().ok_or_else(|| LOST.to_string())?;
            return self.open_after_login(&wire, &key);
        }
        if !inspect.alive && !inspect.initializing {
            self.spawn_preflight();
        }
        Ok(())
    }

    /// Take the turn lock from the wake in flight by cancelling it, or `None`
    /// for the one wake that may not. Newest-wins (ADR-0016) on one session
    /// (ADR-0008). A reactive Poke must not give way to the next proactive wake.
    fn supersede(&self, request: &WakeRequest) -> Option<MutexGuard<'_, ()>> {
        if !request.reactive && self.serving_reactive.load(Ordering::SeqCst) {
            return None;
        }
        let wire = self.current_wire()?;
        self.note_withdrawal(Some(request.instance.clone()));
        wire.cancel();
        let until = Instant::now() + HANDOVER;
        loop {
            if let Ok(turn) = self.turn.try_lock() {
                return Some(turn);
            }
            if Instant::now() >= until {
                // A Harness that ignored the cancel keeps its turn, so no turn
                // was withdrawn for this wake and the next cancelled one must
                // not be read as though it were.
                self.note_withdrawal(None);
                return None;
            }
            thread::sleep(HANDOVER_POLL);
        }
    }

    /// Say that the turn in flight is being taken for `winner`'s wake, or that
    /// no turn was taken after all. Written before the cancel goes out. The
    /// loser holds the lock until it has written its log line.
    fn note_withdrawal(&self, winner: Option<String>) {
        if let Ok(mut slot) = self.withdrawing.lock() {
            *slot = winner;
        }
    }

    /// The Instance a turn of `loser`'s was withdrawn for, named by the turn
    /// itself and kept for the `parsed` line the Shell writes for that wake.
    fn claim_withdrawn_turn(&self, loser: &str, reason: &str) -> Option<String> {
        if reason != CANCELLED {
            return None;
        }
        let winner = self.withdrawing.lock().ok()?.take()?;
        if let Ok(mut withdrawn) = self.withdrawn.lock() {
            withdrawn.insert(loser.to_string(), (winner.clone(), Instant::now()));
        }
        Some(winner)
    }

    /// The same withdrawal, taken by the wake that lost the session. Taken
    /// rather than read so it cannot colour this Instance's next wake. The
    /// grace bounds the entry no `parsed` line comes for (ADR-0016).
    fn claim_withdrawn_wake(&self, instance: &str) -> Option<String> {
        let (winner, at) = self.withdrawn.lock().ok()?.remove(instance)?;
        (at.elapsed() < WITHDRAWAL_GRACE).then_some(winner)
    }

    fn turn(&self, request: &WakeRequest, said: &dyn Fn(&str)) -> Result<Reply, String> {
        let _turn = match self.turn.try_lock() {
            Ok(turn) => turn,
            Err(_) => match self.supersede(request) {
                Some(turn) => turn,
                None => return Err(self.refused(request, "harness busy")),
            },
        };
        self.serving_reactive
            .store(request.reactive, Ordering::SeqCst);
        // Declared after the turn lock, so it is cleared before the lock is
        // released. No window has the lock free and this slot still naming a
        // turn that has ended.
        let _serving = Serving::new(self, &request.instance);
        let (session_id, outcome) = self.attempt(request, said)?;
        // `session/load` can succeed with a dead id (`hermes`). The first turn
        // is the evidence. Reopen once. Not a loss (already charged), a
        // timeout (would spend the budget twice), or our own cancel (ADR-0012).
        let key = SessionKey::from_request(request);
        let (session_id, outcome) = match &outcome {
            Err(TurnError::Stopped(reason)) if reason != CANCELLED && self.reopen_loaded(&key) => {
                self.attempt(request, said)?
            }
            Err(TurnError::Failed(_)) if self.reopen_loaded(&key) => self.attempt(request, said)?,
            _ => (session_id, outcome),
        };
        let mut withdrawn = false;
        let failure = match &outcome {
            Err(TurnError::Failed(why)) => Some(why.clone()),
            _ => None,
        };
        let answer = match outcome {
            Ok(reply) => {
                // A cap-ended turn is logged as what was shown and why there
                // was no more of it, the same pair the HTTP lane writes.
                action_log::append(
                    self.data.as_path(),
                    "turn",
                    match reply.truncated {
                        true => json!({"text": reply.text, "stop": "max_tokens"}),
                        false => json!({"text": reply.text}),
                    },
                );
                Ok(reply)
            }
            Err(TurnError::Lost) => Err(LOST.to_string()),
            Err(TurnError::Timeout) => {
                action_log::append(
                    self.data.as_path(),
                    "timeout",
                    json!({"session_id": session_id}),
                );
                Err(format!(
                    "harness turn exceeded {}s; cancelled",
                    self.timeout.as_secs()
                ))
            }
            Err(TurnError::Stopped(reason)) => {
                // One child serves every Instance (ADR-0008), so a cancel for
                // another character's wake reaches this turn without superseding
                // this Instance's slot. The log line must say it was given up.
                let withdrawn_for = self.claim_withdrawn_turn(&request.instance, &reason);
                withdrawn = withdrawn_for.is_some();
                action_log::append(
                    self.data.as_path(),
                    "turn",
                    json!({"stop": reason, "withdrawn_for": withdrawn_for}),
                );
                Err(format!("harness stopped: {reason}"))
            }
            Err(TurnError::Busy) => Err("harness busy".to_string()),
            Err(TurnError::AuthRequired) => {
                let refused = self
                    .state
                    .lock()
                    .map(|mut state| self.refuse_login(&mut state))
                    .unwrap_or_else(|_| LOST.to_string());
                action_log::append(self.data.as_path(), "turn", json!({"error": refused}));
                Err(refused)
            }
            Err(TurnError::Failed(why)) => {
                action_log::append(self.data.as_path(), "turn", json!({"error": why}));
                Err(why)
            }
        };
        // Kept for the readers on the other side of the Completer, where
        // `Result<String, String>` narrows to "no proposal". A withdrawal is
        // not among them. Naming our own cancel would report a Harness fault.
        self.update_inspect(|inspect| {
            inspect.last_error = (!withdrawn)
                .then(|| answer.as_ref().err().cloned())
                .flatten();
            inspect.turn_failure = failure;
        });
        answer
    }

    /// One `session/prompt` on the session `attach` hands over. Split out of
    /// `turn` so the load retry pays the same bookkeeping the first attempt
    /// did. `Err` is a wake that never reached the wire, already refused.
    fn attempt(
        &self,
        request: &WakeRequest,
        said: &dyn Fn(&str),
    ) -> Result<(String, Result<Reply, TurnError>), String> {
        let (wire, session_id) = match self.attach(Some(&SessionKey::from_request(request))) {
            Ok(pair) => pair,
            // A rejected effort is the harness answering, so Chat reads it on
            // the same path as a failed turn. Other attach refusals never
            // reached `session/prompt`.
            Err(why) if why.starts_with(EFFORT_REJECTED) => {
                return Ok((String::new(), Err(TurnError::Failed(why))));
            }
            Err(why) => return Err(self.refused(request, &why)),
        };
        // The Instance and the wake kind come through the seam rather than
        // from anything here. One child serves every character, so the process is
        // not whose wake this is.
        action_log::append(
            self.data.as_path(),
            "prompt",
            json!({
                "session_id": session_id,
                "instance": request.instance,
                "wake": wake_kind(request.reactive),
                "chars": request.prompt.len(),
            }),
        );
        let outcome = wire.prompt(&session_id, &request.prompt, self.timeout, said);
        // Charge the loss before the outcome is dressed for the caller. A
        // death under every turn must pay the same backoff a failed spawn
        // does. An answering child must not carry a death from hours ago.
        if let Ok(mut state) = self.state.lock() {
            match &outcome {
                Err(TurnError::Lost) => self.lost(&wire, &mut state),
                _ => state.answered(),
            }
            // A finished turn is the proof `session/load` was not, so any
            // later failure on this session is the Harness's own answer.
            if outcome.is_ok() {
                if let Some(opened) = state.sessions.get_mut(&SessionKey::from_request(request)) {
                    opened.loaded = false;
                }
                self.signed_in(&mut state);
            }
        }
        Ok((session_id, outcome))
    }

    /// The Harness wants a login it does not have. Remembered so the Chat
    /// surface disables its composer and names the command, and forwarded so
    /// a window that is already open hears it now, not at its next opening.
    fn refuse_login(&self, state: &mut State) -> String {
        let command = login_command(&self.launch.name, &state.handshake);
        state.login = Some(command.clone());
        state.auth_tried = Some(Instant::now());
        self.update_inspect(|inspect| inspect.login = Some(command.clone()));
        (self.forward)(Forwarded::AttachSettled);
        not_authenticated(&command)
    }

    /// In-app sign-in for one advertised agent method. `Ok` from the agent is
    /// not signed-in: only a session that then opens clears the landing.
    pub fn sign_in(
        &self,
        method_id: &str,
        instance: &str,
        character: &str,
        blank: bool,
    ) -> Result<(), String> {
        let Some(wire) = self.current_wire() else {
            return Err(LOST.to_string());
        };
        {
            let state = self.state.lock().map_err(|_| LOST.to_string())?;
            if state.login.is_none() {
                return Ok(());
            }
            let known = state.handshake.auth_methods.iter().any(|offer| {
                matches!(
                    offer,
                    crate::acp_wire::AuthOffer::Agent { id, .. }
                        if id.as_str() == method_id && crate::acp_wire::sign_in_offered(method_id)
                )
            });
            if !known {
                return Err("that sign-in stays in your terminal".to_string());
            }
        }
        if self
            .signing_in
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err("sign-in already in progress".to_string());
        }
        let _flight = SignInFlight(&self.signing_in);
        // The device flow can sit for minutes. The gate stays free so another
        // Instance can attach; it is taken again only for the open after.
        wire.authenticate(method_id)?;
        let key = SessionKey {
            instance: instance.to_string(),
            character: character.to_string(),
            blank,
        };
        self.open_after_login(&wire, &key)
    }

    /// The open that proves a sign-in, after `authenticate` or a terminal
    /// login. Fresh: a saved `session/load` that returns Ok would clear the
    /// landing even when nothing changed.
    fn open_after_login(&self, wire: &Arc<Wire>, key: &SessionKey) -> Result<(), String> {
        let _gate = self.attach_gate.lock().map_err(|_| LOST.to_string())?;
        {
            let mut state = self.state.lock().map_err(|_| LOST.to_string())?;
            if state.login.is_none() {
                return Ok(());
            }
            // The retry gate would swallow the session/new this click just earned.
            state.auth_tried = None;
        }
        self.open_session(wire, key, true).map(|_| ())
    }

    fn offered_sign_in(&self) -> Vec<SignIn> {
        let Ok(state) = self.state.lock() else {
            return Vec::new();
        };
        crate::acp_wire::sign_in_button(state.login.is_some(), &state.handshake.auth_methods)
    }

    /// An answer is the proof the login happened, in a terminal fidget never
    /// sees. The composer it disabled comes back the same way it went.
    fn signed_in(&self, state: &mut State) {
        if self.clear_login(state) {
            (self.forward)(Forwarded::AttachSettled);
        }
    }

    fn clear_login(&self, state: &mut State) -> bool {
        if state.login.take().is_none() {
            return false;
        }
        self.update_inspect(|inspect| inspect.login = None);
        true
    }

    /// Drop a loaded session so the next `attach` opens a fresh one. A refused
    /// `session/new` is the Harness answering, and reopening it would loop.
    /// The file goes first so a restart cannot load the same id back.
    fn reopen_loaded(&self, key: &SessionKey) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        let Some(opened) = state.sessions.get(key) else {
            return false;
        };
        if !opened.loaded {
            return false;
        }
        let dropped = opened.id.clone();
        state.sessions.remove(key);
        drop(state);
        self.drop_saved(key);
        self.update_inspect(|inspect| {
            if inspect.session_id.as_deref() == Some(dropped.as_str()) {
                inspect.session_id = None;
            }
        });
        true
    }

    /// A wake that never reached `session/prompt`. Logged because the Shell
    /// writes a `parsed` line for every reply it takes, this refusal included.
    /// A `parsed` with no line of its own would join the last logged wake.
    fn refused(&self, request: &WakeRequest, why: &str) -> String {
        action_log::append(
            self.data.as_path(),
            "refused",
            json!({
                "instance": request.instance,
                "wake": wake_kind(request.reactive),
                "why": why,
            }),
        );
        why.to_string()
    }

    /// The wire died. Drop it, charge the loss, and let a later wake respawn.
    /// Empty the slot here. `alive()` flips only once the wire's thread has
    /// dropped its receiver, and a wake arriving before then would reuse it.
    fn lost(&self, wire: &Wire, state: &mut State) {
        wire.shutdown();
        if let Ok(mut slot) = self.wire.lock() {
            *slot = None;
        }
        self.update_inspect(|inspect| {
            inspect.alive = false;
            inspect.initializing = false;
        });
        self.charge_loss(state);
    }

    /// Record that the binary we spawn is not on `PATH`. The name is
    /// `argv[0]`, never the preset. `codex` attaches through `npx` and logs
    /// in through `codex`, so naming the preset would accuse the wrong binary.
    fn note_missing(&self) -> String {
        let command = self.launch.argv[0].clone();
        self.update_inspect(|inspect| {
            inspect.alive = false;
            inspect.missing = Some(command.clone());
            inspect.initializing = false;
            inspect.failed = None;
        });
        not_installed(&command)
    }

    /// Record why a launcher that was there gave no wire, and charge the loss.
    /// The wake gets the sentence; Chat and Settings read the parts.
    fn note_failed(&self, state: &mut State, failure: LaunchFailure) -> String {
        let why = failure.sentence();
        self.charge_loss(state);
        self.update_inspect(|inspect| inspect.failed = Some(failure));
        why
    }

    /// The user's answer to a forwarded permission request. Never chosen here.
    pub fn answer_permission(&self, request: &str, option: &str) {
        let Some(wire) = self.current_wire() else {
            return;
        };
        action_log::append(
            self.data.as_path(),
            "permission_answer",
            json!({"request": request, "option": option}),
        );
        wire.answer(request, option);
    }

    /// The user's answer to a forwarded elicitation form. Decline is valid.
    pub fn answer_elicitation(&self, request: &str, answer: ElicitationAnswer) {
        let Some(wire) = self.current_wire() else {
            return;
        };
        let logged = match &answer {
            ElicitationAnswer::Accept(value) => json!({"request": request, "value": value}),
            ElicitationAnswer::Decline => json!({"request": request, "action": "decline"}),
        };
        action_log::append(self.data.as_path(), "elicitation_answer", logged);
        wire.answer_elicitation(request, answer);
    }

    /// Cancel in-flight work, kill the child, and wait until it is reaped.
    /// The child is in its own process group, so ending this process does not
    /// take it with us. `npx` does not reliably die on stdin EOF.
    pub fn shutdown(&self) {
        // Ctrl+C and `RunEvent::Exit` both call this. The second returns
        // before the reap: waiting out `REAP` again would stall the exit.
        if !self.wanted.swap(false, Ordering::SeqCst) {
            return;
        }
        if let Some(installed) = self.cursor.lock().ok().and_then(|mut slot| slot.take()) {
            installed.remove();
        }
        let Some(wire) = self.wire.lock().ok().and_then(|mut slot| slot.take()) else {
            return;
        };
        wire.shutdown();
        if !wire.wait_for_exit(REAP) {
            eprintln!(
                "harness: `{}` was still running {}s after shutdown",
                self.launch.line(),
                REAP.as_secs()
            );
        }
    }

    /// Config and `enable` before the `acp` spawn. Cursor reads approvals once
    /// per process, so afterwards is too late (#1020). A failure is logged and
    /// the spawn goes ahead: a session without tools beats no session.
    fn install_cursor(&self, cwd: &Path) {
        let Some(endpoint) = crate::mcp_http::endpoint() else {
            return;
        };
        match crate::cursor_mcp::install(cwd, &endpoint.url, &endpoint.authorization()) {
            Ok(installed) => {
                if let Ok(mut slot) = self.cursor.lock() {
                    slot.get_or_insert(installed);
                }
            }
            Err(why) => {
                eprintln!("harness: cursor mcp: {why}");
                return;
            }
        }
        if let Err(why) = crate::cursor_mcp::allow_tools(cwd) {
            eprintln!("harness: cursor permissions: {why}");
        }
        if let Err(why) = crate::cursor_mcp::enable(Path::new(&self.launch.argv[0]), cwd) {
            eprintln!("harness: cursor mcp: {why}");
        }
    }

    fn current_wire(&self) -> Option<Arc<Wire>> {
        self.wire
            .lock()
            .ok()
            .and_then(|slot| slot.clone())
            .filter(|wire| wire.alive())
    }

    fn attach(&self, key: Option<&SessionKey>) -> Result<(Arc<Wire>, String), String> {
        if !self.wanted.load(Ordering::SeqCst) {
            return Err("harness detached".to_string());
        }
        let _gate = self
            .attach_gate
            .lock()
            .map_err(|_| "harness state poisoned")?;
        if !self.wanted.load(Ordering::SeqCst) {
            return Err("harness detached".to_string());
        }
        let wire = match self.current_wire() {
            Some(wire) => wire,
            None => {
                // The probe already refused this launcher. A wake that spawns
                // it anyway is the hang the probe is there to stop.
                if let Some(failure) = self.inspect().unhealthy {
                    return Err(failure.sentence());
                }
                {
                    let mut state = self.state.lock().map_err(|_| "harness state poisoned")?;
                    // A new child does not have the previous process's sessions.
                    state.sessions.clear();
                    // Request ids restart with the child, so leftover ask and
                    // owner entries must not match a new ask by coincidence.
                    if let Ok(mut asked) = self.asked.lock() {
                        asked.clear();
                    }
                    if let Ok(mut owners) = self.owners.lock() {
                        owners.clear();
                    }
                    if let Some(until) = state.spawn_wait_until {
                        if Instant::now() < until {
                            return Err(format!(
                                "harness {} not running; retrying in {}s",
                                self.launch.line(),
                                (until - Instant::now()).as_secs()
                            ));
                        }
                    }
                }
                match self.spawn_and_initialize() {
                    Ok(wire) => wire,
                    Err(why) => {
                        let failure = match why {
                            // A file `PATH` has not got is not a child that might
                            // come back. Backoff would only refuse the next wake
                            // for up to five minutes after the user installs the CLI.
                            SpawnError::Missing => return Err(self.note_missing()),
                            SpawnError::Exited { status, output } => {
                                exited(&self.launch, status, output)
                            }
                            SpawnError::Failed(message) => LaunchFailure::before_launch(message),
                        };
                        let mut state = self.state.lock().map_err(|_| "harness state poisoned")?;
                        return Err(self.note_failed(&mut state, failure));
                    }
                }
            }
        };
        let Some(key) = key else {
            return Ok((wire, String::new()));
        };
        let inputs = self.open_inputs();
        let outdated = {
            let mut state = self.state.lock().map_err(|_| "harness state poisoned")?;
            let outdated = match state.sessions.get(key) {
                Some(opened) if opened.opened_on == inputs => {
                    return Ok((wire, opened.id.clone()));
                }
                // Its saved slot is the same conversation, so not loaded either.
                Some(_) => {
                    state.sessions.remove(key);
                    true
                }
                None => false,
            };
            if let (Some(command), Some(tried)) = (&state.login, state.auth_tried) {
                if tried.elapsed() < self.auth_retry {
                    return Err(not_authenticated(command));
                }
            }
            outdated
        };
        let id = self.open_session(&wire, key, outdated)?;
        Ok((wire, id))
    }

    fn spawn_and_initialize(&self) -> Result<Arc<Wire>, SpawnError> {
        // The store first. Empty cwd is `data_dir`, which we own; `checked`
        // would refuse a first-run folder that does not exist yet. A user
        // path still goes through `checked` with no `create_dir_all` (#782).
        self.data.ensure()?;
        let cwd = match &self.cwd {
            Ok(cwd) => {
                cwd.checked()?;
                cwd
            }
            Err(error) => return Err(SpawnError::Failed(error.to_string())),
        };
        if takes_cursor_config(&self.launch) {
            self.install_cursor(cwd.as_path());
        }
        let data = self.data.as_path().to_path_buf();
        let forward = Arc::clone(&self.forward);
        let asked = Arc::clone(&self.asked);
        let owners = Arc::clone(&self.owners);
        let serving = Arc::clone(&self.serving_instance);
        let spawned = Wire::spawn(
            self.launch.command(cwd, mcp_stdio().is_some()),
            self.attach_timeout(),
            Box::new(move |event| note_event(&data, &forward, &asked, &owners, &serving, event)),
        );
        // Anything but `Missing` means `PATH` had the file to run. Clear the
        // old `missing` on the failing edge too, or Settings keeps telling the
        // user to install a CLI that is already there.
        if !matches!(spawned, Err(SpawnError::Missing)) {
            self.update_inspect(|inspect| inspect.missing = None);
        }
        let wire = spawned.map_err(|why| match why {
            SpawnError::Failed(why) => {
                SpawnError::Failed(format!("`{}` {why}", self.launch.line()))
            }
            other => other,
        })?;
        let handshake = wire.handshake().clone();
        {
            let mut state = self
                .state
                .lock()
                .map_err(|_| SpawnError::Failed("harness state poisoned".to_string()))?;
            state.handshake = handshake.clone();
            // A fresh process is a fresh chance to sign in. The previous
            // retry window does not apply to it.
            state.login = None;
            state.auth_tried = None;
        }
        self.update_inspect(|inspect| {
            inspect.agent = handshake.agent.clone();
            inspect.mcp_http = handshake.mcp_http;
            inspect.alive = true;
            inspect.initializing = false;
            inspect.failed = None;
        });
        let wire = Arc::new(wire);
        if !self.wanted.load(Ordering::SeqCst) {
            wire.shutdown();
            return Err(SpawnError::Failed("harness detached".to_string()));
        }
        if let Ok(mut slot) = self.wire.lock() {
            *slot = Some(Arc::clone(&wire));
        }
        if !self.wanted.load(Ordering::SeqCst) {
            self.shutdown();
            return Err(SpawnError::Failed("harness detached".to_string()));
        }
        Ok(wire)
    }

    fn open_inputs(&self) -> OpenInputs {
        let settings =
            crate::settings::Settings::load(&crate::settings::settings_path(self.data.as_path()));
        OpenInputs {
            model: crate::model::env_or_file(crate::model::MODEL, &settings.director_model),
            effort: crate::dev_flags::director_reasoning_effort(),
        }
    }

    /// `session/load` when the Harness can and the file names this Harness,
    /// else `session/new`. Either way the file ends up naming what is open.
    fn open_session(
        &self,
        wire: &Arc<Wire>,
        key: &SessionKey,
        fresh: bool,
    ) -> Result<String, String> {
        let (saved, mcp, generation) = {
            let state = self.state.lock().map_err(|_| LOST.to_string())?;
            let saved = if fresh {
                None
            } else {
                state
                    .handshake
                    .load_session
                    .then(|| self.saved_id(key))
                    .flatten()
            };
            let mcp = mcp_server(&state.handshake);
            let generation = state.conversation_generation(&key.instance);
            (saved, mcp, generation)
        };
        let cwd = self
            .cwd
            .as_ref()
            .map_err(|error| error.to_string())?
            .as_path();
        let inputs = self.open_inputs();
        // codex-acp replaces its session `mcp_servers` config when ACP also
        // supplies servers, which would discard Fidget's scoped approval.
        let mcp_for_wire = if self.launch.codex_mcp_config().is_some() {
            None
        } else {
            mcp.clone()
        };
        let opened = match wire.open(
            saved.clone(),
            cwd,
            mcp_for_wire,
            self.launch.name == "claude",
            self.attach_timeout(),
            &inputs.model,
            inputs.effort.as_deref(),
        ) {
            Ok(opened) => opened,
            Err(OpenError::Lost) => {
                let mut state = self.state.lock().map_err(|_| LOST.to_string())?;
                self.lost(wire, &mut state);
                return Err(LOST.to_string());
            }
            Err(OpenError::AuthRequired) => {
                let mut state = self.state.lock().map_err(|_| LOST.to_string())?;
                return Err(self.refuse_login(&mut state));
            }
            Err(OpenError::EffortRejected(why)) => return Err(format!("{EFFORT_REJECTED}{why}")),
            Err(OpenError::Failed(why)) => {
                self.update_inspect(|inspect| {
                    inspect.last_error = Some(why.clone());
                    inspect.turn_failure = Some(why.clone());
                });
                return Err(format!("session/new: {why}"));
            }
        };
        let id = opened.id;
        let (replaced, first_open) = {
            let mut state = self.state.lock().map_err(|_| LOST.to_string())?;
            // `drop_conversation` does not wait on this open. Storing the id
            // puts back the conversation it throws away. Its cancel has no
            // prompt, so the withdrawal note is not a turn.
            if state.conversation_generation(&key.instance) != generation {
                self.note_withdrawal(None);
                (true, false)
            } else {
                state.sessions.insert(
                    key.clone(),
                    OpenedSession {
                        id: id.clone(),
                        opened_on: inputs,
                        loaded: saved.as_deref() == Some(id.as_str()),
                    },
                );
                // The AttachSettled below carries a cleared login too.
                self.clear_login(&mut state);
                self.update_inspect(|inspect| inspect.session_id = Some(id.clone()));
                if let Ok(mut owners) = self.owners.lock() {
                    owners.insert(id.clone(), key.instance.clone());
                }
                (false, state.opened_ids.insert(id.clone()))
            }
        };
        if replaced {
            // The agent already opened this id. Leaving it untracked keeps a
            // session alive that no wake will load.
            if let Err(why) = wire.close(&id) {
                eprintln!("harness: session/close {id}: {why}");
            }
            return Err("session replaced".to_string());
        }
        if first_open && !opened.history.is_empty() {
            (self.forward)(Forwarded::Restored(Restored {
                instance: key.instance.clone(),
                history: opened.history,
            }));
        }
        // An open Chat surface drew its header from an opening asked for
        // before this session existed, and asks again only on its next send.
        (self.forward)(Forwarded::AttachSettled);
        self.save_session(key, &id);
        action_log::append(
            self.data.as_path(),
            "attach",
            // The label, never the choice. An `McpChoice::Http` carries the
            // loopback token and the Action Log is a file on disk.
            json!({
                "harness": self.launch.name,
                "session_id": id,
                "mcp": mcp.as_ref().map(McpChoice::label),
            }),
        );
        // A drop during the write puts the id back on disk. Remove that id
        // only, so a newer open for this Instance is left where it is.
        let stale = self
            .state
            .lock()
            .map(|state| state.conversation_generation(&key.instance) != generation)
            .unwrap_or(false);
        if stale {
            self.forget_saved_id(key, &id);
        }
        Ok(id)
    }

    fn read_saved(&self) -> Option<SavedSession> {
        let text = std::fs::read_to_string(self.data.join(SESSION_FILE)).ok()?;
        serde_json::from_str(&text).ok()
    }

    fn saved_id(&self, key: &SessionKey) -> Option<String> {
        let saved = self.read_saved()?;
        if saved.harness != self.launch.name {
            return None;
        }
        saved
            .sessions
            .into_iter()
            .find_map(|slot| (slot.key() == *key).then_some(slot.session_id))
    }

    /// Cancel this Instance's own turn, recording the withdrawal so it reads
    /// as given up (ADR-0016). `wire.cancel()` names no session (ADR-0008), so
    /// ask who holds it first. The caller is the frame loop and must not wait.
    fn cancel_own_turn(&self, instance: &str) {
        let ours = self
            .serving_instance
            .lock()
            .is_ok_and(|serving| serving.as_deref() == Some(instance));
        if !ours {
            return;
        }
        let Some(wire) = self.current_wire() else {
            return;
        };
        self.note_withdrawal(Some(instance.to_string()));
        wire.cancel();
    }

    /// Forget this Instance's ACP conversations so the next wake is
    /// `session/new`. Every lane opened with the old Instance Prompt
    /// (ADR-0012). In-memory ids and saved slots both go, or a restart loads it.
    pub fn drop_conversation(&self, instance: &str) {
        self.cancel_own_turn(instance);
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        *state
            .conversation_gen
            .entry(instance.to_string())
            .or_default() += 1;
        let keys: Vec<SessionKey> = state
            .sessions
            .keys()
            .filter(|key| key.instance == instance)
            .cloned()
            .collect();
        let dropped: Vec<String> = keys
            .iter()
            .filter_map(|key| state.sessions.remove(key).map(|opened| opened.id))
            .collect();
        drop(state);
        self.forget_saved(instance);
        self.update_inspect(|inspect| {
            if inspect
                .session_id
                .as_deref()
                .is_some_and(|id| dropped.iter().any(|dropped| dropped == id))
            {
                inspect.session_id = None;
            }
        });
    }

    /// Apply saved the Completer settings.
    pub fn inputs_applied(&self) {}

    fn forget_saved(&self, instance: &str) {
        let _file = self
            .session_file
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(mut record) = self.read_saved() else {
            return;
        };
        record.sessions.retain(|slot| slot.instance != instance);
        if let Ok(text) = serde_json::to_string(&record) {
            let _ = std::fs::write(self.data.join(SESSION_FILE), format!("{text}\n"));
        }
    }

    fn drop_saved(&self, key: &SessionKey) {
        let _file = self
            .session_file
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(mut record) = self.read_saved() else {
            return;
        };
        record.sessions.retain(|slot| slot.key() != *key);
        if let Ok(text) = serde_json::to_string(&record) {
            let _ = std::fs::write(self.data.join(SESSION_FILE), format!("{text}\n"));
        }
    }

    fn forget_saved_id(&self, key: &SessionKey, id: &str) {
        let _file = self
            .session_file
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(mut record) = self.read_saved() else {
            return;
        };
        record
            .sessions
            .retain(|slot| !(slot.key() == *key && slot.session_id == id));
        if let Ok(text) = serde_json::to_string(&record) {
            let _ = std::fs::write(self.data.join(SESSION_FILE), format!("{text}\n"));
        }
    }

    fn save_session(&self, key: &SessionKey, id: &str) {
        let _file = self
            .session_file
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        #[cfg(test)]
        if SESSION_SAVE_STALL.swap(false, Ordering::SeqCst) {
            SESSION_SAVE_STALLING.store(true, Ordering::SeqCst);
            thread::sleep(SESSION_SAVE_STALL_FOR);
            SESSION_SAVE_STALLING.store(false, Ordering::SeqCst);
        }
        let mut record = self.read_saved().unwrap_or(SavedSession {
            harness: self.launch.name.clone(),
            agent: None,
            sessions: Vec::new(),
            session_id: None,
        });
        record.harness = self.launch.name.clone();
        record.agent = self.inspect().agent;
        record.session_id = None;
        match record.sessions.iter_mut().find(|slot| slot.key() == *key) {
            Some(slot) => slot.session_id = id.to_string(),
            None => record.sessions.push(SavedSlot {
                instance: key.instance.clone(),
                character: key.character.clone(),
                blank: key.blank,
                session_id: id.to_string(),
            }),
        }
        if let Ok(text) = serde_json::to_string(&record) {
            let _ = std::fs::write(self.data.join(SESSION_FILE), format!("{text}\n"));
        }
    }
}

impl Completer for Session {
    fn complete(&self, request: &WakeRequest, said: &dyn Fn(&str)) -> Result<Reply, String> {
        if crate::model::tracing() {
            eprintln!("harness: prompt to {}", self.launch.name);
        }
        let reply = self.turn(request, said);
        if crate::model::tracing() {
            match &reply {
                Ok(reply) if reply.truncated => {
                    eprintln!("harness: reply cut off at the cap {}", reply.text)
                }
                Ok(reply) => eprintln!("harness: reply {}", reply.text),
                Err(why) => eprintln!("harness: {why}"),
            }
        }
        reply
    }

    /// One child serves every Instance (ADR-0008), so an outstanding ask
    /// belongs to the Instance whose session asked it, mid-turn or between
    /// turns. Another character's wake is not held by this one's question.
    fn awaiting_user(&self, instance: &str) -> bool {
        let serving = self
            .serving_instance
            .lock()
            .ok()
            .and_then(|serving| serving.clone());
        let owners = self.owners.lock().ok();
        self.asked.lock().is_ok_and(|asked| {
            asked
                .values()
                .any(|asker| asker.owed_by(instance, owners.as_deref(), serving.as_deref()))
        })
    }
}

/// Who owes the user an answer to one open ask. The asking session is the
/// fact; the Instance is a lookup through `owners`, so an ask that arrives
/// before the open records its owner still resolves once that owner is known.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Asker {
    /// The session the ask came on.
    Session(String),
    /// Whoever holds the turn. A form no session scopes, as a sign-in link.
    Turn,
}

impl Asker {
    fn of(session: Option<&str>) -> Self {
        session.map_or(Self::Turn, |session| Self::Session(session.to_string()))
    }

    fn owed_by(
        &self,
        instance: &str,
        owners: Option<&HashMap<String, String>>,
        serving: Option<&str>,
    ) -> bool {
        self.instance(owners, serving) == Some(instance)
    }

    /// The Instance that owes the answer, if one can be named.
    fn instance<'a>(
        &self,
        owners: Option<&'a HashMap<String, String>>,
        serving: Option<&'a str>,
    ) -> Option<&'a str> {
        match self {
            Self::Session(session) => match owners.and_then(|owners| owners.get(session)) {
                Some(owner) => Some(owner),
                // A session this child never opened, or not yet recorded.
                None => serving,
            },
            Self::Turn => serving,
        }
    }
}

/// The one prompt the probe sends. Shaped like the last line of a Character
/// Prompt so a reply that does not parse is the Harness's doing, not the
/// prompt's. Asked for explicitly rather than left to the Harness.
const PROBE_PROMPT: &str =
    "Reply with exactly this one line and nothing else: Wave | Hello from the probe.";

/// How long shutdown waits for the child to be reaped before saying so.
const REAP: Duration = Duration::from_secs(2);

#[cfg(test)]
static SESSION_SAVE_STALL: AtomicBool = AtomicBool::new(false);
#[cfg(test)]
static SESSION_SAVE_STALLING: AtomicBool = AtomicBool::new(false);
#[cfg(test)]
const SESSION_SAVE_STALL_FOR: Duration = Duration::from_secs(2);
#[cfg(test)]
static FAIL_SWITCH_SPAWN: AtomicBool = AtomicBool::new(false);

/// Attach the configured Harness and run one turn, with no overlay.
/// Exit 2 means never asked, 1 means asked and unanswered, 0 is `end_turn`.
/// A Harness that is not signed in names the command for the user's terminal.
pub fn run_probe() -> i32 {
    // No settings file on this path. `dev_flags::seed` is where the exported
    // timeout is read.
    crate::dev_flags::seed(&crate::settings::Settings::default());
    // The probe loads no settings file, so the exported variable is the only
    // source it has.
    let Some(target) = Target::from_settings(None, "") else {
        eprintln!("probe-harness: {VAR} is unset, so there is no Harness to attach");
        return 2;
    };
    let session = Arc::new(Session::new(
        target.launch,
        target.cwd,
        // The probe's own folder keeps the session file and Action Log out of a
        // real install. Memory cannot be isolated. The MCP server resolves it
        // from the data folder, so a probe `remember` writes the real `memory.md`.
        SessionDataDir::probe(),
        // Named, never answered. Only a click on the Chat surface may answer a
        // permission request (ADR-0022), and the probe has no surface. The ask
        // times out with the turn, which is itself the report.
        Arc::new(Box::new(|forwarded| match forwarded {
            Forwarded::Ask { ask, .. } => println!(
                "  permission   {} [{}]",
                ask.title.as_deref().unwrap_or("—"),
                ask.request
            ),
            Forwarded::Form { form, .. } => {
                println!("  elicitation  {} [{}]", form.message, form.request)
            }
            _ => {}
        }) as Forward),
    ));
    // Ctrl+C is ours before spawn so the child leaves this process group and
    // `kill_harness_tree` can reap `npx` grandchildren. The handler shuts the
    // session down. 130 is the shell's SIGINT code, not a probe verdict.
    let interrupted = session.clone();
    match ctrlc::set_handler(move || {
        interrupted.shutdown();
        std::process::exit(130);
    }) {
        Ok(()) => own_interrupt(),
        Err(why) => eprintln!(
            "probe-harness: could not catch interrupt: {why}; the Harness stays in this process group"
        ),
    }
    let code = probe(&session);
    session.shutdown();
    code
}

/// `run_probe` minus the environment, so the fake agent can run the whole of
/// it in a test.
fn probe(session: &Session) -> i32 {
    // Served before attach, so `choose_mcp` takes the branch the app takes.
    // Without it a Harness got the stdio shim with nothing to dial (#984).
    let (calls, answers) = std::sync::mpsc::channel();
    let served = crate::mcp_http::serve(calls).is_some();
    thread::spawn(move || answer_probe_calls(answers));
    println!("probe-harness");
    println!("  harness      {}", session.launch.name);
    println!("  command      {}", session.launch.line());
    println!("  cwd          {}", attach_cwd_display(&session.cwd));
    println!("  data         {}", session.data.as_path().display());
    println!(
        "  timeout      turn {}s, attach {}s",
        session.timeout.as_secs(),
        session.attach_timeout().as_secs()
    );
    println!();

    println!("attach");
    let session_id = match session.attach(Some(&SessionKey {
        instance: "probe".to_string(),
        character: "probe".to_string(),
        blank: false,
    })) {
        Ok((_, id)) => id,
        // Nothing was asked, so this is configuration and not a turn. The
        // message names a missing binary, a login, or a refused `session/new`.
        // The code says only that the prompt never went out.
        Err(why) => {
            println!("  {why}");
            return 2;
        }
    };
    let handshake = session
        .state
        .lock()
        .map(|state| state.handshake.clone())
        .unwrap_or_default();
    let methods: Vec<&str> = handshake
        .auth_methods
        .iter()
        .map(|method| method.name())
        .collect();
    println!(
        "  agent        {}",
        handshake.agent.as_deref().unwrap_or("unnamed")
    );
    println!("  loadSession  {}", handshake.load_session);
    println!("  mcp http     {}", handshake.mcp_http);
    // What the session was actually handed, not what was on offer. The label
    // never carries the loopback token.
    println!(
        "  mcp          {}",
        mcp_server(&handshake).map_or_else(|| "none".to_string(), |choice| choice.label())
    );
    if !served {
        println!("  mcp          NO LOOPBACK LISTENER: the server above has nothing to dial, so no tool claim from this run holds");
    }
    println!(
        "  authMethods  {}",
        if methods.is_empty() {
            "none".to_string()
        } else {
            methods.join(", ")
        }
    );
    println!("  session      {session_id}");
    println!();

    println!("turn");
    println!("  prompt       {PROBE_PROMPT}");
    let code = match session.turn(
        &WakeRequest {
            prompt: PROBE_PROMPT.to_string(),
            // Reactive, because a probe is someone asking on purpose. Named for
            // the probe so the Action Log line cannot be read as a character's own wake.
            instance: "probe".to_string(),
            character: "probe".to_string(),
            reactive: true,
            // The probe sends its own fixed prompt, not a Character's, so the mode
            // it would have been assembled under decides nothing here.
            blank: false,
        },
        &|_| {},
    ) {
        Ok(reply) => {
            let text = reply.text;
            println!(
                "  stop         {}",
                if reply.truncated {
                    "max_tokens"
                } else {
                    "end_turn"
                }
            );
            println!("  reply        {text}");
            // Reported, not part of the verdict. Whether a model obeys a
            // one-line format is the Director's problem. `end_turn` proved
            // the wire either way.
            match fidget_core::director::parse_proposal(&text) {
                Ok(proposal) => println!(
                    "  proposal     {} | {}",
                    proposal.behavior,
                    proposal.dialogue.as_deref().unwrap_or("(no dialogue)")
                ),
                Err(_) => println!("  proposal     no, the first line is not a Behavior name"),
            }
            0
        }
        Err(why) => {
            println!("  {why}");
            1
        }
    };
    // Reported after the turn: a Harness may fetch the list lazily. Zero
    // means the Harness never asked, whatever `initialize` advertised.
    match crate::mcp_http::tools_listed() {
        0 => println!("  mcp listed   no, the Harness never asked for the tool list"),
        n => println!("  mcp listed   yes, {n} tools/list request(s)"),
    }
    code
}

/// Answer the Harness's tool calls through the frame loop's `dispatch`,
/// against an empty desktop and no Instances. `speak` reports that it
/// reached nobody rather than a success nothing shows (ADR-0026).
fn answer_probe_calls(calls: std::sync::mpsc::Receiver<crate::mcp_http::Call>) {
    use fidget_core::dispatch::{dispatch, DenyList, DispatchContext, PlacementQuery};
    let memory_path = fidget_core::memory::shared_path();
    while let Ok(call) = calls.recv() {
        println!("  mcp call     {}", call.tool);
        let mut context = DispatchContext {
            window_source: &fidget_core::window_source::StubWindowSource,
            memory_path: memory_path.clone(),
            denylist: DenyList {
                excluded_applications: Vec::new(),
                filter_password_fields: true,
            },
            roster: &[],
            expression: None,
            placement: PlacementQuery::empty(),
        };
        let _ = call
            .reply
            .send(dispatch(&call.tool, call.arguments, &mut context));
    }
}

/// What the session stream said, into the Action Log, and a permission
/// request on to the Chat surface. Runs on the wire thread.
fn note_event(
    dir: &Path,
    forward: &Forward,
    asked: &Mutex<HashMap<String, Asker>>,
    owners: &Mutex<HashMap<String, String>>,
    serving: &Mutex<Option<String>>,
    event: Event,
) {
    let instance_of = |session: &str| owners.lock().ok()?.get(session).cloned();
    // Chat is named once, on arrival. `awaiting_user` resolves the same
    // `Asker` each time it reads, so an ask on a session still opening outside
    // a wake is drawn in the turn holder's Chat but owed by its own Instance
    // once the open records it (docs/harness.md).
    let owe = |request: &str, asker: Asker| {
        let serving = serving.lock().ok().and_then(|serving| serving.clone());
        let instance = owners.lock().ok().and_then(|owners| {
            asker
                .instance(Some(&owners), serving.as_deref())
                .map(str::to_string)
        });
        if let Ok(mut asked) = asked.lock() {
            asked.insert(request.to_string(), asker);
        }
        instance.map_or(Owner::EveryChat, Owner::Instance)
    };
    match event {
        // A tool call and a usage tick are logged and never forwarded, so a
        // turn shows the surface no phases. ADR-0028 bounds what a later
        // surface may draw from events that already arrive here.
        Event::ToolCall {
            id,
            title,
            kind,
            status,
        } => action_log::append(
            dir,
            "tool_call",
            json!({"id": id, "title": title, "kind": kind, "status": status}),
        ),
        Event::Plan { session, steps } => {
            // Guarded because `end_turn` clears the plan on every turn, and an
            // unguarded line would log a zero-step plan for turns that had none.
            if !steps.is_empty() {
                action_log::append(dir, "plan", json!({"entries": steps.len()}));
            }
            if let Some(instance) = instance_of(&session) {
                forward(Forwarded::Plan { instance, steps });
            }
        }
        Event::Usage { used, size } => {
            action_log::append(dir, "usage_update", json!({"used": used, "size": size}))
        }
        Event::Permission { session, ask } => {
            action_log::append(
                dir,
                "permission_request",
                json!({"request": ask.request, "title": ask.title, "kind": ask.kind}),
            );
            let owner = owe(&ask.request, Asker::of(Some(&session)));
            forward(Forwarded::Ask { owner, ask });
        }
        Event::Elicitation { session, form } => {
            action_log::append(
                dir,
                "elicitation_create",
                json!({"request": form.request, "field": form.field}),
            );
            let owner = owe(&form.request, Asker::of(session.as_deref()));
            forward(Forwarded::Form { owner, form });
        }
        Event::PermissionSettled { request, option } => {
            if let Ok(mut asked) = asked.lock() {
                asked.remove(&request);
            }
            forward(Forwarded::Settled { request, option })
        }
        // Forwarded and not logged. The Action Log points at the Harness's own
        // session dump rather than copying it (CONTEXT.md), and a reply is not
        // copied there either (ADR-0034).
        Event::InboundWake { session, speech } => {
            if let Some(instance) = instance_of(&session) {
                forward(Forwarded::InboundWake(InboundWake { instance, speech }));
            }
        }
        Event::Thought { session, text } => {
            if let Some(instance) = instance_of(&session) {
                forward(Forwarded::Thought {
                    instance,
                    line: text,
                });
            }
        }
    }
}

/// The Action Log line for what one reply parsed to.
/// The Shell writes it where it takes the wake out of `Slots`, because
/// `crates/core` parses and does no I/O.
pub fn note_parsed(instance: &str, wake: &Wake, reactive: bool, near_miss: Option<&str>) {
    let session = attached();
    let dir = session
        .as_ref()
        .map(|session| session.data.as_path().to_path_buf())
        .unwrap_or_else(fidget_core::memory::data_dir);
    // Asked here rather than carried through `crates/core`. The caller has the
    // wake and not the words, and this already holds the session that knows
    // whose wake took it.
    let withdrawn_for = match (&session, wake) {
        (Some(session), Wake::Failed) => session.claim_withdrawn_wake(instance),
        _ => None,
    };
    let error = matches!(wake, Wake::Failed)
        .then(|| session.and_then(|session| session.inspect().last_error))
        .flatten();
    action_log::append(
        &dir,
        "parsed",
        parsed_fields(
            instance,
            wake,
            reactive,
            near_miss,
            error.as_deref(),
            withdrawn_for.as_deref(),
        ),
    );
}

/// The six answers: `near_miss`, `proposal`, `speech`, `failed`, `error`,
/// `withdrawn`. `near_miss` arrives as speech but is not `speech`.
/// `withdrawn` outranks `error` because our own cancel arrives as one.
fn parsed_fields(
    instance: &str,
    wake: &Wake,
    reactive: bool,
    near_miss: Option<&str>,
    error: Option<&str>,
    withdrawn_for: Option<&str>,
) -> Value {
    let (result, behavior) = match (near_miss, wake) {
        (Some(named), _) => ("near_miss", Some(named)),
        (None, Wake::Proposed(proposal)) if !proposal.behavior.is_empty() => {
            ("proposal", Some(proposal.behavior.as_str()))
        }
        (None, Wake::Proposed(_)) => ("speech", None),
        (None, Wake::Failed) if withdrawn_for.is_some() => ("withdrawn", None),
        (None, Wake::Failed) if error.is_some() => ("error", None),
        (None, Wake::Failed) => ("failed", None),
    };
    json!({
        "instance": instance,
        "wake": wake_kind(reactive),
        "result": result,
        "behavior": behavior,
        // The cancel we sent is not words the Harness chose, so a withdrawal
        // carries none. `withdrawn_for` is the whole account of that line.
        "error": withdrawn_for.is_none().then_some(error).flatten(),
        "withdrawn_for": withdrawn_for,
    })
}

/// The Action Log's word for each of ADR-0008's two wake kinds. Proactive is
/// the wake policy's word; Ambient in the glossary is Ambient Capture.
fn wake_kind(reactive: bool) -> &'static str {
    if reactive {
        "reactive"
    } else {
        "proactive"
    }
}

/// A launcher that ran and was gone before `initialize`. `npx` is Node.js, and
/// a Node.js that cannot start (a broken Homebrew link) dies exactly here.
fn exited(
    launch: &Launch,
    status: Option<std::process::ExitStatus>,
    output: String,
) -> LaunchFailure {
    let status = status
        .map(|status| format!(", {status}"))
        .unwrap_or_default();
    LaunchFailure {
        command: Some(launch.line()),
        reason: format!("exited before initialize{status}"),
        output,
        node_check: (launch.argv[0] == "npx").then(|| "node --version".to_string()),
    }
}

fn not_authenticated(command: &str) -> String {
    format!("harness not authenticated: run `{command}`")
}

/// A Harness is the user's to install and never ours to ship (ADR-0018), so
/// the only fix is one the user makes outside the app, the same shape as
/// `not_authenticated` and for the same reason.
fn not_installed(command: &str) -> String {
    let install_hint = install_page(command)
        .map(|(product, url)| format!(" Install {product} from {url}."))
        .unwrap_or_default();
    format!("`{command}` is not installed; Fidget does not bundle a Harness.{install_hint}")
}

/// The product a launcher belongs to and the page that installs it. The one
/// table Chat, Settings and the wake all name it from (ADR-0035).
pub(crate) fn install_page(command: &str) -> Option<(&'static str, &'static str)> {
    Some(match command {
        "npx" => ("Node.js", "https://nodejs.org/"),
        "hermes" => ("Hermes", "https://hermes-agent.nousresearch.com/"),
        // The ACP registry still names https://block.github.io/goose/, which
        // now redirects to goose-docs.ai. The install page is the CLI instructions.
        "goose" => ("Goose", "https://goose-docs.ai/docs/getting-started/installation/"),
        "copilot" => ("GitHub Copilot CLI", "https://docs.github.com/en/copilot/how-tos/copilot-cli/set-up-copilot-cli/install-copilot-cli"),
        "cursor-agent" => ("Cursor", "https://www.cursor.com/"),
        "grok" => ("Grok", "https://x.ai/"),
        "opencode" => ("OpenCode", "https://opencode.ai/"),
        // Google publishes the archive through the ACP registry only. This
        // page is where it names the registry and the sign-in.
        "agy_acp_server.par" | "agy_acp_server.exe" => (
            "the Antigravity ACP server",
            "https://antigravity.google/docs/ide/extensions",
        ),
        _ => return None,
    })
}

/// The command that logs the user in. The table outranks the handshake,
/// because ACP describes `authMethods` in prose and ADR-0018 hosts no
/// terminal to run a `terminal` method. A custom command keeps the adapter's text.
fn login_command(name: &str, handshake: &Handshake) -> String {
    named_login(name).map(str::to_string).unwrap_or_else(|| {
        handshake
            .auth_methods
            .first()
            .map(|method| {
                method
                    .description()
                    .map(str::to_string)
                    .unwrap_or_else(|| method.name().to_string())
            })
            .unwrap_or_else(|| login_hint(name))
    })
}

/// The documented sign-in line for a named Harness, before any handshake.
/// Chat's Connect reads it at the pick, Settings after `-32000` (ADR-0022).
/// fidget never runs it (ADR-0018).
pub(crate) fn login_hint(name: &str) -> String {
    named_login(name)
        .map(str::to_string)
        .unwrap_or_else(|| format!("{name} (run it once in a terminal and sign in)"))
}

fn named_login(name: &str) -> Option<&'static str> {
    Some(match name {
        "claude" => "claude /login",
        "codex" => "codex login",
        // The one `authMethods` entry, `copilot-login`, describes this line.
        "copilot" => "copilot login",
        // Not the line the handshake offers. Cursor describes "agent login",
        // and `agent` is what the binary calls itself, not the `cursor-agent`
        // the installer puts on `PATH`. Following it verbatim is a command not found.
        "cursor-agent" => "cursor-agent login",
        "grok" => "grok login",
        // No `goose login`. Provider setup is `goose configure`.
        "goose" => "goose configure",
        "hermes" => "hermes login",
        "opencode" => "opencode login",
        "pi" => "npx -y pi-acp@latest --terminal-login",
        // The server has no login command. Its Google methods open the
        // browser themselves on `authenticate`, so Chat's buttons are the way in.
        "antigravity" => "Log in with Google from Chat",
        _ => return None,
    })
}

/// The MCP server to hand this session. Loopback first (ADR-0023). No
/// `mcpCapabilities.http` on handshake means the stdio shim (ADR-0026).
/// Branch on that bit only. Token in the Authorization header, never URL or argv.
fn mcp_server(handshake: &Handshake) -> Option<McpChoice> {
    choose_mcp(handshake, crate::mcp_http::endpoint(), mcp_stdio())
}

/// The choice itself, with both candidates handed in. A test binary is not
/// named `fidget` and has no sidecar beside it, so `mcp_stdio` finds nothing
/// there and the stdio branch would never be exercised.
fn choose_mcp(
    handshake: &Handshake,
    endpoint: Option<crate::mcp_http::Endpoint>,
    stdio: Option<McpLaunch>,
) -> Option<McpChoice> {
    if handshake.mcp_http {
        if let Some(endpoint) = &endpoint {
            return Some(McpChoice::Http {
                url: endpoint.url.clone(),
                authorization: endpoint.authorization(),
            });
        }
    }
    let mut launch = stdio?;
    launch.env = endpoint
        .map(|endpoint| endpoint.stdio_env())
        .unwrap_or_default();
    Some(McpChoice::Stdio(launch))
}

/// The stdio MCP server to hand the session, when one can be launched.
/// Read here rather than at construction, so a path typed in the window is
/// the one the next attach hands over. `FIDGET_MCP_BIN` still outranks the file.
fn mcp_stdio() -> Option<McpLaunch> {
    let configured = crate::dev_flags::mcp_bin();
    mcp_launch(
        configured.as_deref(),
        std::env::current_exe().ok()?.as_path(),
    )
}

fn mcp_launch(configured: Option<&Path>, current_exe: &Path) -> Option<McpLaunch> {
    if let Some(path) = configured.filter(|path| path.is_file()) {
        return Some(McpLaunch {
            path: path.to_path_buf(),
            args: Vec::new(),
            env: Vec::new(),
        });
    }
    let beside = current_exe.parent()?.join("fidget-mcp");
    let sibling = if cfg!(windows) {
        beside.with_extension("exe")
    } else {
        beside
    };
    if sibling.is_file() {
        return Some(McpLaunch {
            path: sibling,
            args: Vec::new(),
            env: Vec::new(),
        });
    }
    // Sibling / configured path still win; this is how `cargo run` and a bundle
    // with no sidecar still hand the Harness a server. The loopback server
    // above is what a Harness that can take it gets instead.
    (current_exe.file_stem()? == "fidget").then(|| McpLaunch {
        path: current_exe.to_path_buf(),
        args: vec!["--mcp-stdio".into()],
        env: Vec::new(),
    })
}

/// The one attachment, and what it needs to be opened again.
/// `forward` belongs to the Shell's window handle, not to any one child, and
/// `retarget` has no other way to get one.
struct Attachment {
    session: Option<Arc<Session>>,
    forward: Option<Arc<Forward>>,
    /// Bumped on every swap. A switch still reaping the previous child checks
    /// this before it spawns, so a newer pick is the one that starts.
    switch_gen: u64,
}

static ATTACHED: Mutex<Attachment> = Mutex::new(Attachment {
    session: None,
    forward: None,
    switch_gen: 0,
});

fn attachment() -> MutexGuard<'static, Attachment> {
    ATTACHED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Read the source, the variable, else `saved` from Settings, and hold the
/// Session until the row moves it or the process exits. Process-global because
/// the session is one per app (ADR-0008) and a Retarget rebuilds `DirectorSettings`.
pub(crate) fn attach(target: Option<Target>, forward: Forward) -> Option<Arc<Session>> {
    let mut slot = attachment();
    if slot.forward.is_some() {
        return slot.session.clone();
    }
    let forward = Arc::new(forward);
    slot.forward = Some(Arc::clone(&forward));
    slot.session = open(target, forward);
    slot.session.clone()
}

fn open(target: Option<Target>, forward: Arc<Forward>) -> Option<Arc<Session>> {
    target.map(|target| {
        Arc::new(Session::new(
            target.launch,
            target.cwd,
            SessionDataDir::app(),
            forward,
        ))
    })
}

/// What a Completer source row now in force asks of the attachment.
/// Alive is not an input. A set Harness is still the Completer (ADR-0008).
/// `Drop` is only a row that names no Harness, never a session failing.
#[derive(Debug, PartialEq, Eq)]
enum Reattach {
    /// The row still names what is attached, a dead one included. A fresh
    /// Session for the same command line would throw away the backoff the old
    /// one earned and respawn on every Apply.
    Stand,
    Drop,
    Open(Target),
}

fn reattach(attached: Option<&Target>, wanted: Option<Target>) -> Reattach {
    match wanted {
        None if attached.is_none() => Reattach::Stand,
        None => Reattach::Drop,
        Some(target) if attached == Some(&target) => Reattach::Stand,
        Some(target) => Reattach::Open(target),
    }
}

/// Re-open the attachment for the Completer source now in force.
/// A wire that dies is the Session's own business. `spawning` is the
/// Director's switch, so a session no wake will reach is not opened.
pub(crate) fn retarget(wanted: Option<Target>, spawning: bool) {
    let mut slot = attachment();
    // The probe and the tests never call `attach`, so there is no forward to
    // rebuild a Session with and nothing of theirs to move.
    let Some(forward) = slot.forward.clone() else {
        return;
    };
    let attached = slot.session.as_ref().map(|session| session.target());
    let opened = match reattach(attached.as_ref(), wanted) {
        Reattach::Stand => return,
        Reattach::Drop => None,
        Reattach::Open(target) => open(Some(target), forward),
    };
    // Swapped under the one lock `attached` reads. A gap here is a wake landing
    // on the HTTP Completer that nobody chose, which is what ADR-0008 refuses.
    slot.switch_gen = slot.switch_gen.wrapping_add(1);
    let switch_gen = slot.switch_gen;
    let old = std::mem::replace(&mut slot.session, opened.clone());
    drop(slot);
    match &opened {
        None => eprintln!("harness: detached; HTTP Completer is the Director's \"AI brain\""),
        Some(session) => {
            eprintln!("harness: {} is the Completer now", session.launch.line());
            if spawning {
                // Visible before this returns. Chat shows the wait, and a
                // same-pick repick does not start a second child.
                session.update_inspect(|inspect| {
                    inspect.initializing = true;
                    inspect.failed = None;
                });
            }
        }
    }
    let spawn_after = opened.filter(|_| spawning);
    // The caller is the UI thread. Reaping waits up to `REAP`, so it does
    // not happen here. The new child starts after that reap, off this thread.
    let Some(old) = old else {
        if let Some(session) = spawn_after {
            session.spawn_preflight();
        }
        return;
    };
    // A thread that does not start still has to reap. `Wire`'s drop only posts
    // shutdown, and the child would outlive the switch.
    #[cfg(test)]
    let refuse_thread = FAIL_SWITCH_SPAWN.swap(false, Ordering::SeqCst);
    #[cfg(not(test))]
    let refuse_thread = false;
    if refuse_thread {
        finish_switch(old, spawn_after, switch_gen);
        return;
    }
    let old_bg = Arc::clone(&old);
    let spawn_bg = spawn_after.clone();
    let switched = thread::Builder::new()
        .name("harness-switch".into())
        .spawn(move || finish_switch(old_bg, spawn_bg, switch_gen));
    if let Err(why) = switched {
        eprintln!("harness: could not switch off the caller: {why}");
        finish_switch(old, spawn_after, switch_gen);
    }
}

/// Reap the child being replaced, then start the new one if this switch is
/// still the latest. A newer pick bumps `switch_gen` and starts itself.
fn finish_switch(old: Arc<Session>, spawn_after: Option<Arc<Session>>, switch_gen: u64) {
    {
        // Hold the new session's gate across the reap, when there is one. A
        // wake blocks there instead of spawning beside the child being reaped.
        let _hold = spawn_after.as_ref().map(|session| {
            session
                .attach_gate
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
        });
        old.shutdown();
    }
    let slot = attachment();
    if slot.switch_gen != switch_gen {
        return;
    }
    if let Some(session) = spawn_after {
        session.spawn_preflight();
    }
}

pub fn attached() -> Option<Arc<Session>> {
    attachment().session.clone()
}

/// Agent sign-in buttons for the attached session, or nothing when login is
/// not still required. Callers do not re-check the method kind.
pub(crate) fn sign_in_actions() -> Vec<SignIn> {
    #[cfg(test)]
    if let Some(actions) = SIGN_IN_SESSION.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(|session| session.offered_sign_in())
    }) {
        return actions;
    }
    attached()
        .map(|session| session.offered_sign_in())
        .unwrap_or_default()
}

#[cfg(test)]
thread_local! {
    static SIGN_IN_SESSION: std::cell::RefCell<Option<Arc<Session>>> =
        const { std::cell::RefCell::new(None) };
}

/// A session `sign_in_actions` can see without publishing it into the process
/// attachment. Other tests read that slot, and a session left there would
/// make them think a Harness is attached.
#[cfg(test)]
pub(crate) fn with_sign_in_gate(
    login: Option<&str>,
    methods: Vec<agent_client_protocol::schema::v1::AuthMethod>,
    body: impl FnOnce(),
) {
    let dir = std::env::temp_dir().join(format!("fidget-sign-in-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let session = Arc::new(Session::new(
        Launch {
            name: "codex".to_string(),
            argv: vec!["codex".to_string()],
        },
        Ok(AttachCwd(dir.clone())),
        SessionDataDir::at(dir.clone()),
        Arc::new(Box::new(|_| {}) as Forward),
    ));
    {
        let mut state = session.state.lock().unwrap();
        state.login = login.map(str::to_string);
        state.handshake.auth_methods = methods.iter().map(crate::acp_wire::auth_offer).collect();
    }
    SIGN_IN_SESSION.with(|slot| *slot.borrow_mut() = Some(session));
    struct Clear(PathBuf);
    impl Drop for Clear {
        fn drop(&mut self) {
            SIGN_IN_SESSION.with(|slot| *slot.borrow_mut() = None);
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _clear = Clear(dir);
    body();
}

/// The error the attached Harness answered the last turn with, if it did.
/// The Completer seam hands core a bare `Err`, so a version refusal and an
/// unparsable reply both reach the Shell as `Wake::Failed`. Read only on failure.
pub fn last_error() -> Option<String> {
    attached().and_then(|session| session.inspect().last_error)
}

/// `HarnessInspect::turn_failure` of the attached Harness. Read only on failure.
pub fn turn_failure() -> Option<String> {
    attached().and_then(|session| session.inspect().turn_failure)
}

/// What `startup_lines` says about the attachment, if there is one.
/// `spawning` is whether one is actually coming. A line promising a spawn
/// the Director's switch has already refused is worse than no line.
pub fn startup_lines(spawning: bool) -> Vec<String> {
    let Some(session) = attached() else {
        return Vec::new();
    };
    let mut lines = vec![
        format!(
            "harness: {} via `{}`",
            session.launch.name,
            session.launch.line()
        ),
        // Before the handshake, so this says what is on offer rather than
        // which one the session got. The probe's `mcp` line says the latter.
        match (crate::mcp_http::endpoint(), mcp_stdio()) {
            (Some(endpoint), Some(launch)) => format!(
                "harness: MCP server {}, or `{}` (relays here) for a Harness that advertises no mcpCapabilities.http",
                endpoint.url,
                launch.line()
            ),
            (Some(endpoint), None) => format!("harness: MCP server {}", endpoint.url),
            (None, Some(launch)) => {
                format!(
                    "harness: MCP server {} (no app endpoint to relay to)",
                    launch.line()
                )
            }
            (None, None) => "harness: no MCP server; the session gets no tools".to_string(),
        },
    ];
    // Startup cannot report a spawn that has not happened. `attach` runs on
    // the preflight thread and lands after these lines. Said here so the next
    // `harness:` line reads as this attachment's outcome.
    if spawning {
        lines.push(
            "harness: attaching now; the next `harness:` line on this stream is how it went"
                .to_string(),
        );
    }
    lines
}

pub fn shutdown() {
    if let Some(session) = attached() {
        session.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fidget_core::director::{Context, Happened, ModelDirector, Wake};
    use fidget_core::engine::BehaviorProposal;
    use std::io::{BufRead, Write};
    use std::sync::mpsc::{self, Receiver};

    /// A tool-using Ask is not the Model API hop.
    #[test]
    fn an_unset_timeout_gives_a_harness_turn_minutes_not_the_http_hop() {
        crate::model::tests::with_env(None, None, None, || {
            crate::dev_flags::seed(&crate::settings::Settings::default());
            assert_eq!(turn_timeout(), Duration::from_secs(120));
            assert_ne!(
                turn_timeout(),
                crate::model::TIMEOUT,
                "the Model API hop is not a tool-using turn's budget"
            );
        });
    }

    /// The Model API field is not this budget.
    #[test]
    fn a_director_timeout_does_not_set_the_harness_turn() {
        crate::model::tests::with_env(None, None, None, || {
            crate::dev_flags::seed(&crate::settings::Settings {
                director_timeout_secs: "45".into(),
                ..Default::default()
            });
            assert_eq!(turn_timeout(), TURN_TIMEOUT);
        });
    }

    /// The Development field and `FIDGET_HARNESS_TURN_TIMEOUT` still win.
    #[test]
    fn a_set_timeout_is_the_harness_turn_budget() {
        crate::model::tests::with_env(None, None, None, || {
            crate::dev_flags::seed(&crate::settings::Settings {
                harness_turn_timeout_secs: "45".into(),
                ..Default::default()
            });
            assert_eq!(turn_timeout(), Duration::from_secs(45));
        });
    }

    /// How long the `permission-after-work` Harness works before it asks, and
    /// again after the answer. A test that sets the turn budget between one
    /// and two of these can tell a fresh budget from a resumed one.
    const ASK_WORK: Duration = Duration::from_millis(600);

    /// Longer than a responsive surface tick, shorter than an attach timeout.
    /// A handshake that holds the frame loop or Chat open blocks for this long.
    const SURFACE_STALL: Duration = Duration::from_secs(2);

    /// The fake ACP agent. This test binary re-executed with `script=<name>`
    /// among its filters, speaking newline JSON-RPC on stdio. Returns at once
    /// under a normal `cargo test`, where no script is named.
    #[test]
    fn fake_acp_agent() {
        let args: Vec<String> = std::env::args().collect();
        let Some(script) = args.iter().find_map(|arg| arg.strip_prefix("script=")) else {
            return;
        };
        let count = args
            .iter()
            .find_map(|arg| arg.strip_prefix("count="))
            .map(PathBuf::from);
        fake_main(script, count.as_deref());
        std::process::exit(0);
    }

    fn record(count: Option<&Path>, what: &str) {
        if let Some(path) = count {
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .unwrap();
            writeln!(file, "{what}").unwrap();
        }
    }

    fn record_open(count: Option<&Path>, message: &Value) {
        if let Some(cwd) = message.pointer("/params/cwd").and_then(Value::as_str) {
            record(count, &format!("cwd={cwd}"));
        }
        if message.pointer("/params/_meta/claudeCode/options/allowedTools")
            == Some(&json!(["mcp__fidget__*"]))
        {
            record(count, "fidget-approval");
        }
        // The transport of the first server handed over. An http entry says
        // so; a stdio one names a command and no type.
        if let Some(server) = message
            .pointer("/params/mcpServers/0")
            .filter(|server| !server.is_null())
        {
            let transport = server
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("stdio");
            record(count, &format!("mcp={transport}"));
        }
    }

    fn recorded(count: Option<&Path>, what: &str) -> usize {
        count
            .and_then(|path| std::fs::read_to_string(path).ok())
            .map_or(0, |text| text.lines().filter(|line| *line == what).count())
    }

    fn say(value: Value) {
        // Authenticate can answer from another thread while this loop writes
        // the next reply. One line at a time, or the two JSON objects join.
        static OUT: Mutex<()> = Mutex::new(());
        let _line = OUT.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        println!("{value}");
    }

    fn chunk(session: &str, text: &str) {
        say(
            json!({"jsonrpc": "2.0", "method": "session/update", "params": {
                "sessionId": session,
                "update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": text}},
            }}),
        );
    }

    fn user_chunk(session: &str, text: &str) {
        say(
            json!({"jsonrpc": "2.0", "method": "session/update", "params": {
                "sessionId": session,
                "update": {"sessionUpdate": "user_message_chunk", "content": {"type": "text", "text": text}},
            }}),
        );
    }

    fn thought(session: &str, text: &str) {
        say(
            json!({"jsonrpc": "2.0", "method": "session/update", "params": {
                "sessionId": session,
                "update": {"sessionUpdate": "agent_thought_chunk", "content": {"type": "text", "text": text}},
            }}),
        );
    }

    fn tool_call(session: &str, title: &str) {
        say(
            json!({"jsonrpc": "2.0", "method": "session/update", "params": {
                "sessionId": session,
                "update": {"sessionUpdate": "tool_call", "toolCallId": title, "title": title, "kind": "other", "status": "completed"},
            }}),
        );
    }

    /// codex-acp's MCP startup shape: a session-scoped link, unprompted.
    /// `mcp-link` sends it right after `session/new`, and `mcp-link-turn`
    /// inside the first turn, which is where a slow MCP server's lands.
    /// `mcp-link-tool` scopes it to a tool call, which blocks that turn.
    fn mcp_link(session: &str, tool_call: Option<&str>) {
        let mut link = json!({"jsonrpc": "2.0", "id": 102, "method": "elicitation/create", "params": {
            "sessionId": session,
            "mode": "url",
            "elicitationId": "mcp-1",
            "url": "https://example.test/oauth",
            "message": "Authenticate with MCP server linear",
        }});
        if let Some(call) = tool_call {
            link["params"]["toolCallId"] = json!(call);
        }
        say(link);
    }

    fn stop(id: &Value, reason: &str) {
        say(json!({"jsonrpc": "2.0", "id": id, "result": {"stopReason": reason}}));
    }

    fn effort_option(category: &str, id: &str) -> Value {
        json!({
            "id": id,
            "name": "Reasoning",
            "category": category,
            "type": "select",
            "currentValue": "low",
            "options": [
                {"value": "low", "name": "Low"},
                {"value": "medium", "name": "Medium"},
                {"value": "high", "name": "High"}
            ]
        })
    }

    fn model_option() -> Value {
        json!({
            "id": "llm",
            "name": "Model",
            "category": "model",
            "type": "select",
            "currentValue": "default-model",
            "options": [
                {"value": "default-model", "name": "Default"},
                {"value": "some-model", "name": "Some"}
            ]
        })
    }

    /// Options the fake advertises. `thought_level` is listed after
    /// `model_config` so a client that takes the first select picks wrong.
    fn completer_config_options(script: &str) -> Option<Value> {
        match script {
            "completer-effort"
            | "completer-effort-reject"
            | "load-completer-effort"
            | "permission" => {
                Some(json!([
                    effort_option("model_config", "knob"),
                    effort_option("thought_level", "reasoning"),
                ]))
            }
            "completer-effort-mc" => Some(json!([
                model_option(),
                effort_option("model_config", "knob"),
            ])),
            "completer-model" | "load-completer-model" => Some(json!([
                effort_option("thought_level", "thought"),
                model_option(),
            ])),
            "completer-thought" => Some(json!([effort_option("thought_level", "thought")])),
            "completer-both" => Some(json!([
                model_option(),
                effort_option("thought_level", "reasoning"),
            ])),
            _ => None,
        }
    }

    fn fake_main(script: &str, count: Option<&Path>) {
        // libtest writes `test <name> ... ` with no newline before the test
        // runs; end that line so the first reply is a line of its own.
        println!();
        record(count, "spawn");
        let spawns = recorded(count, "spawn");
        // A launcher that dies before it reads a byte, the way `npx` does when
        // dyld cannot load `node`. `abort-first` is that Node fixed afterwards.
        if script == "abort-at-start" || (script == "abort-first" && spawns == 1) {
            eprintln!(
                "dyld[0]: Library not loaded: /opt/homebrew/opt/llhttp/lib/libllhttp.9.3.dylib"
            );
            std::process::abort();
        }
        let mut session = "fresh-id".to_string();
        let mut pending_prompt: Option<Value> = None;
        let mut pending_auth: Option<Value> = None;
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            let Ok(line) = line else { break };
            let Ok(message) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            let id = message.get("id").cloned().unwrap_or(Value::Null);
            match message.get("method").and_then(Value::as_str) {
                // A child that starts and then fails. The spawn is not
                // `Missing`.
                Some("initialize") if script == "die-initializing" => std::process::exit(3),
                Some("initialize") => {
                    if script == "stall-initialize" {
                        thread::sleep(SURFACE_STALL);
                    }
                    if let Some(path) = count {
                        let _ = std::fs::write(
                            path.with_file_name("initialize.json"),
                            serde_json::to_vec(message.get("params").unwrap_or(&Value::Null))
                                .unwrap_or_default(),
                        );
                    }
                    say(json!({"jsonrpc": "2.0", "id": id, "result": {
                        "protocolVersion": 1,
                        "agentInfo": {"name": "fake-agent", "version": "0"},
                        "agentCapabilities": {"loadSession": script.starts_with("load"), "mcpCapabilities": {"http": true}},
                        "authMethods": [{"id": "fake", "name": "Fake login", "description": "fake --login"}],
                    }}));
                }
                Some("session/new") => {
                    record(count, "new");
                    record_open(count, &message);
                    if script == "die-opening" {
                        std::process::exit(3);
                    }
                    // After `authenticate`, so the first refusal that puts the
                    // landing up stays quick and only the sign-in open stalls.
                    if script == "stall-open"
                        || (script == "stall-sign-in" && recorded(count, "authenticate") > 0)
                    {
                        record(count, "open-stall");
                        thread::sleep(SURFACE_STALL);
                    }
                    // `auth-sign-in` opens once `authenticate` has run.
                    // `auth-sign-in-noop` never does, like an adapter whose
                    // `authenticate` is Ok and changes nothing.
                    let refuse = match script {
                        "auth" => recorded(count, "new") == 1,
                        "auth-sign-in" | "auth-sign-in-link" | "stall-sign-in"
                        | "stall-authenticate" => recorded(count, "authenticate") == 0,
                        "auth-sign-in-noop" => true,
                        _ => false,
                    };
                    if refuse {
                        say(
                            json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32000, "message": "auth required"}}),
                        );
                    } else {
                        // A new session is a new id. Without the reset the fake
                        // would hand back whichever id the last prompt named.
                        // Counted so two Instances cannot share a minted id.
                        let n = recorded(count, "new");
                        session = if n <= 1 {
                            "fresh-id".to_string()
                        } else {
                            format!("fresh-id-{n}")
                        };
                        let mut result = json!({"sessionId": session});
                        if let Some(options) = completer_config_options(script) {
                            result["configOptions"] = options;
                        }
                        // Reminder on the new session before `session/new`
                        // answers, so the ask is queued during the open drain.
                        if script == "ask-on-open" {
                            say(
                                json!({"jsonrpc": "2.0", "id": 99, "method": "session/request_permission", "params": {
                                    "sessionId": &session,
                                    "toolCall": {"toolCallId": "t-open", "title": "Remind on open", "kind": "other"},
                                    "options": [
                                        {"optionId": "allow", "name": "Allow", "kind": "allow_once"},
                                        {"optionId": "reject", "name": "Reject", "kind": "reject_once"},
                                    ],
                                }}),
                            );
                        }
                        say(json!({"jsonrpc": "2.0", "id": id, "result": result}));
                        if script == "mcp-link" || script == "mcp-link-complete-turn" {
                            mcp_link(&session, None);
                        }
                        // The user signs in some other way, so the Harness
                        // says the link is done. The first names no form.
                        if script == "mcp-link-complete" {
                            mcp_link(&session, None);
                            for link in ["nope", "mcp-1"] {
                                say(
                                    json!({"jsonrpc": "2.0", "method": "elicitation/complete", "params": {
                                        "elicitationId": link,
                                    }}),
                                );
                            }
                        }
                    }
                }
                Some("authenticate") => {
                    record(count, "authenticate");
                    // The long hop of login. The reply is another thread so
                    // `session/new` for a second Instance is read during it.
                    if script == "stall-authenticate" {
                        record(count, "auth-stall");
                        let id = id.clone();
                        thread::spawn(move || {
                            thread::sleep(SURFACE_STALL);
                            say(json!({"jsonrpc": "2.0", "id": id, "result": {}}));
                        });
                    } else if script == "auth-sign-in-link" {
                        // codex-acp's device-code shape: the code rides in a
                        // URL elicitation, and `authenticate` waits on it.
                        say(
                            json!({"jsonrpc": "2.0", "id": 101, "method": "elicitation/create", "params": {
                                "requestId": &id,
                                "mode": "url",
                                "elicitationId": "device-1",
                                "url": "https://example.test/device?code=ABCD-1234",
                                "message": "Enter ABCD-1234 on the sign-in page.",
                            }}),
                        );
                        pending_auth = Some(id);
                    } else {
                        say(json!({"jsonrpc": "2.0", "id": id, "result": {}}));
                    }
                }
                Some("session/close") => {
                    record(count, "close");
                    say(json!({"jsonrpc": "2.0", "id": id, "result": {}}));
                }
                Some("session/load") => {
                    record(count, "load");
                    record_open(count, &message);
                    let loaded = message
                        .pointer("/params/sessionId")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    // ACP replays the conversation as updates before it
                    // answers `session/load`, even one that then fails.
                    if script == "load-replay" {
                        tool_call(loaded, "replayed");
                    }
                    if script == "load-history" {
                        let update = |update: Value| {
                            say(
                                json!({"jsonrpc": "2.0", "method": "session/update", "params": {
                                    "sessionId": loaded, "update": update,
                                }}),
                            )
                        };
                        let plan = |status: &str| {
                            update(json!({"sessionUpdate": "plan", "entries": [
                                {"content": "Read the roster", "priority": "medium", "status": status},
                            ]}))
                        };
                        let message = |id: &str, text: &str| {
                            update(
                                json!({"sessionUpdate": "agent_message_chunk", "messageId": id,
                                "content": {"type": "text", "text": text}}),
                            )
                        };
                        user_chunk(loaded, "the first wake's ");
                        user_chunk(loaded, "prompt");
                        thought(loaded, "Weighing it");
                        chunk(loaded, "wave\n");
                        chunk(loaded, "Hello from before");
                        tool_call(loaded, "replayed");
                        plan("pending");
                        update(
                            json!({"sessionUpdate": "tool_call_update", "toolCallId": "replayed",
                            "status": "failed",
                            "content": [{"type": "content", "content": {"type": "text", "text": "no such file"}}]}),
                        );
                        plan("completed");
                        user_chunk(loaded, "the second wake's prompt");
                        chunk(loaded, "<think>hmm</think>nod | Still here");
                        message("m1", "Done.");
                        message("m2", "Stretch ");
                        message("m2", "break!");
                    }
                    if loaded == "stale" {
                        say(
                            json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32602, "message": "no such session"}}),
                        );
                    } else {
                        let mut result = json!({});
                        if let Some(options) = completer_config_options(script) {
                            result["configOptions"] = options;
                        }
                        say(json!({"jsonrpc": "2.0", "id": id, "result": result}));
                    }
                }
                Some("session/set_config_option") => {
                    let config_id = message
                        .pointer("/params/configId")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    let value = message
                        .pointer("/params/value")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    record(count, &format!("config={config_id}={value}"));
                    if value == "nope" {
                        say(json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "error": {"code": -32602, "message": "unknown model"}
                        }));
                    } else if script == "completer-effort-reject" {
                        say(json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "error": {"code": -32602, "message": "not a level this agent takes"}
                        }));
                    } else if script == "completer-both" && config_id == "llm" {
                        say(json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": {"configOptions": [effort_option("thought_level", "after-model")]}
                        }));
                    } else {
                        let options = completer_config_options(script).unwrap_or_else(|| json!([]));
                        say(json!({"jsonrpc": "2.0", "id": id, "result": {"configOptions": options}}));
                    }
                }
                Some("session/prompt") => {
                    record(count, "prompt");
                    // The SDK routes updates by session id, so a loaded
                    // session's chunks must carry the loaded id.
                    if let Some(id) = message.pointer("/params/sessionId").and_then(Value::as_str) {
                        session = id.to_string();
                    }
                    let prompts = recorded(count, "prompt");
                    match script {
                        "refusal" => stop(&id, "refusal"),
                        // The token died between the attach and this turn.
                        // claude-code-acp's shape, then ACP's own code.
                        "auth-turn" if prompts == 1 => {
                            say(json!({"jsonrpc": "2.0", "id": id, "error": {
                                "code": -32603,
                                "message": "Internal error: Failed to authenticate: OAuth session expired and could not be refreshed",
                                "data": {"errorKind": "authentication_failed"},
                            }}))
                        }
                        // grok's out-of-credit answer: the reason is in `data` only.
                        "balance-exhausted" => say(json!({"jsonrpc": "2.0", "id": id, "error": {
                            "code": -32603,
                            "message": "Internal error",
                            "data": {
                                "message": "API error (status 402 Payment Required): Grok Build usage balance exhausted",
                                "http_status": 402,
                            },
                        }})),
                        "auth-turn-32000" if prompts == 1 => say(
                            json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32000, "message": "Authentication required"}}),
                        ),
                        // The session the load claimed to restore is not
                        // there, so the first prompt refuses and the one
                        // after the reopen is served.
                        "load-dead" if prompts == 1 => stop(&id, "refusal"),
                        "load-refusal" => stop(&id, "refusal"),
                        "permission" | "permission-after-work" | "permission-stall" => {
                            if script == "permission-after-work" {
                                thread::sleep(ASK_WORK);
                            }
                            pending_prompt = Some(id);
                            say(
                                json!({"jsonrpc": "2.0", "id": 99, "method": "session/request_permission", "params": {
                                    "sessionId": &session,
                                    "toolCall": {
                                        "toolCallId": "t1",
                                        "title": "rm -rf /",
                                        "kind": "execute",
                                        "content": [{"type": "content", "content": {"type": "text", "text": "Delete everything?"}}],
                                        "rawInput": {"command": "rm -rf /"},
                                        "locations": [{"path": "/"}],
                                    },
                                    "options": [
                                        {"optionId": "allow", "name": "Allow", "kind": "allow_once"},
                                        {"optionId": "reject", "name": "Reject", "kind": "reject_once"},
                                    ],
                                }}),
                            );
                        }
                        "elicitation" => {
                            pending_prompt = Some(id);
                            say(
                                json!({"jsonrpc": "2.0", "id": 100, "method": "elicitation/create", "params": {
                                    "sessionId": &session,
                                    "mode": "form",
                                    "message": "How should I approach this refactoring?",
                                    "requestedSchema": {
                                        "type": "object",
                                        "properties": {
                                            "strategy": {
                                                "type": "string",
                                                "enum": ["conservative", "balanced", "aggressive"]
                                            }
                                        },
                                        "required": ["strategy"]
                                    }
                                }}),
                            );
                        }
                        "slow" | "load-slow" if prompts == 1 => pending_prompt = Some(id),
                        // Works for a while and then answers, asking nothing.
                        // A turn long enough for another wake to land inside
                        // it, with no ask to confuse the two.
                        "working" => {
                            thread::sleep(ASK_WORK);
                            chunk(&session, "Hello");
                            stop(&id, "end_turn");
                        }
                        // Writes its answer over three chunks, the Behavior
                        // name split across two of them.
                        "speaking" => {
                            for text in ["wa", "ve\nGood ", "morning"] {
                                chunk(&session, text);
                            }
                            stop(&id, "end_turn");
                        }
                        // `scripts/scenarios/streaming-bubble.sh`. Slow enough
                        // to photograph the bubble mid-sentence.
                        "scenario-streaming" => {
                            chunk(&session, "gre");
                            thread::sleep(Duration::from_millis(600));
                            chunk(&session, "et\n");
                            for (n, word) in "Watch these words arrive one at a time."
                                .split_inclusive(' ')
                                .enumerate()
                            {
                                chunk(&session, word);
                                record(count, &format!("word {}", n + 1));
                                thread::sleep(Duration::from_millis(700));
                            }
                            record(count, "spoken");
                            stop(&id, "end_turn");
                        }
                        // Thinks before it answers, so each turn's thought
                        // names the session it came from.
                        "thinking" => {
                            thought(&session, &format!("thinking in {session}"));
                            chunk(&session, "Hello");
                            stop(&id, "end_turn");
                        }
                        // `scripts/scenarios/thinking-row.sh`. The first turn
                        // asks, so Chat opens by itself, and its end cancels
                        // the ask. Later turns think slowly enough to capture.
                        "scenario-thinking" if prompts == 1 => {
                            say(
                                json!({"jsonrpc": "2.0", "id": 99, "method": "session/request_permission", "params": {
                                    "sessionId": &session,
                                    "toolCall": {
                                        "toolCallId": "t1",
                                        "title": "Open Chat for the scenario",
                                        "kind": "other",
                                        "content": [{"type": "content", "content": {"type": "text", "text": "Opens Chat."}}],
                                    },
                                    "options": [
                                        {"optionId": "allow", "name": "Allow", "kind": "allow_once"},
                                        {"optionId": "reject", "name": "Reject", "kind": "reject_once"},
                                    ],
                                }}),
                            );
                            record(count, "asked");
                            thread::sleep(Duration::from_secs(3));
                            chunk(&session, "fidget\nChat is open.");
                            stop(&id, "end_turn");
                        }
                        "scenario-thinking" => {
                            for n in 1..=4 {
                                thought(&session, &format!("Thought {n} of four. "));
                                record(count, &format!("thought {n}"));
                                thread::sleep(Duration::from_millis(1200));
                            }
                            chunk(&session, "fidget\nI thought it over and I am staying put.");
                            stop(&id, "end_turn");
                            record(count, "replied");
                        }
                        "scenario-asking" if prompts == 1 => {
                            say(
                                json!({"jsonrpc": "2.0", "id": 99, "method": "session/request_permission", "params": {
                                    "sessionId": &session,
                                    "toolCall": {
                                        "toolCallId": "t1",
                                        "title": "May I proceed?",
                                        "kind": "other",
                                        "content": [{"type": "content", "content": {"type": "text", "text": "The question that waits on the user."}}],
                                    },
                                    "options": [
                                        {"optionId": "allow", "name": "Allow", "kind": "allow_once"},
                                        {"optionId": "reject", "name": "Reject", "kind": "reject_once"},
                                    ],
                                }}),
                            );
                            record(count, "asked");
                            pending_prompt = Some(id);
                        }
                        "mcp-link-turn" if prompts == 1 => {
                            mcp_link(&session, None);
                            chunk(&session, "Hello");
                            stop(&id, "end_turn");
                        }
                        // A tool call's own sign-in: the turn holds until it is answered.
                        "mcp-link-tool" => {
                            mcp_link(&session, Some("call-1"));
                            pending_prompt = Some(id);
                        }
                        // The link was held between turns; the user finishes
                        // it elsewhere while this turn runs.
                        "mcp-link-complete-turn" if prompts == 1 => {
                            for link in ["nope", "mcp-1"] {
                                say(
                                    json!({"jsonrpc": "2.0", "method": "elicitation/complete", "params": {
                                        "elicitationId": link,
                                    }}),
                                );
                            }
                            chunk(&session, "Hello");
                            stop(&id, "end_turn");
                        }
                        // Answers, then works on with no prompt open, the way
                        // a Claude Code cron fire does: a tool call, a
                        // thought, agent text, and an ask the user has to answer.
                        "between-turn" | "between-turn-held" if prompts == 1 => {
                            chunk(&session, "Hello");
                            stop(&id, "end_turn");
                            // Past the turn's end on Fidget's side, or the
                            // turn reads these as its own on the way out.
                            thread::sleep(Duration::from_millis(300));
                            tool_call(&session, "cron fired");
                            thought(&session, "between turns");
                            chunk(&session, "Your reminder: time to stretch!");
                            say(
                                json!({"jsonrpc": "2.0", "id": 99, "method": "session/request_permission", "params": {
                                    "sessionId": &session,
                                    "toolCall": {"toolCallId": "t2", "title": "Remind the user", "kind": "other"},
                                    "options": [
                                        {"optionId": "allow", "name": "Allow", "kind": "allow_once"},
                                        {"optionId": "reject", "name": "Reject", "kind": "reject_once"},
                                    ],
                                }}),
                            );
                        }
                        // The next turn waits on the ask held from between turns.
                        "between-turn-held" => pending_prompt = Some(id),
                        // Another Instance's session, the first one opened,
                        // speaks and asks while this turn runs. The turn ends
                        // right after that ask, with the ask still open.
                        "crosstalk" => {
                            chunk("fresh-id", "Not yours");
                            say(
                                json!({"jsonrpc": "2.0", "id": 99, "method": "session/request_permission", "params": {
                                    "sessionId": "fresh-id",
                                    "toolCall": {"toolCallId": "t3", "title": "Remind the other user", "kind": "other"},
                                    "options": [
                                        {"optionId": "allow", "name": "Allow", "kind": "allow_once"},
                                        {"optionId": "reject", "name": "Reject", "kind": "reject_once"},
                                    ],
                                }}),
                            );
                            chunk(&session, "Hello");
                            stop(&id, "end_turn");
                        }
                        // While this turn runs, the other Instance's session
                        // plans, asks and sends a form; then this one plans.
                        "crosstalk-rows" => {
                            let plan = |session: &str, step: &str| {
                                say(
                                    json!({"jsonrpc": "2.0", "method": "session/update", "params": {
                                        "sessionId": session,
                                        "update": {"sessionUpdate": "plan", "entries": [
                                            {"content": step, "priority": "medium", "status": "in_progress"},
                                        ]},
                                    }}),
                                )
                            };
                            plan("fresh-id", "B's step");
                            say(
                                json!({"jsonrpc": "2.0", "id": 99, "method": "session/request_permission", "params": {
                                    "sessionId": "fresh-id",
                                    "toolCall": {"toolCallId": "t3", "title": "Remind the other user", "kind": "other"},
                                    "options": [
                                        {"optionId": "allow", "name": "Allow", "kind": "allow_once"},
                                    ],
                                }}),
                            );
                            mcp_link("fresh-id", None);
                            plan(&session, "A's step");
                            chunk(&session, "Hello");
                            stop(&id, "end_turn");
                        }
                        "exit" | "load-history" if spawns == 1 => std::process::exit(3),
                        "die" => std::process::exit(3),
                        _ => {
                            chunk(&session, "Hell");
                            if script == "garbage" {
                                println!("this is not json");
                            }
                            chunk(&session, "o");
                            stop(&id, "end_turn");
                        }
                    }
                }
                Some("session/cancel") => {
                    record(count, "cancel");
                    if let Some(id) = pending_prompt.take() {
                        stop(&id, "cancelled");
                    }
                }
                Some(_) => {}
                // A reply to our own permission request.
                None if id == json!(99) => {
                    let outcome = message
                        .pointer("/result/outcome/outcome")
                        .and_then(Value::as_str)
                        .unwrap_or("?")
                        .to_string();
                    record(count, &format!("perm:{outcome}"));
                    if outcome == "selected"
                        && script != "permission-stall"
                        && script != "crosstalk"
                    {
                        if script == "permission-after-work" {
                            thread::sleep(ASK_WORK);
                        }
                        if script == "scenario-asking" {
                            chunk(&session, "fidget\nYou answered the question.");
                            record(count, "replied");
                        } else {
                            let option = message
                                .pointer("/result/outcome/optionId")
                                .and_then(Value::as_str)
                                .unwrap_or("");
                            chunk(&session, &format!("ok:{option}"));
                        }
                        if let Some(id) = pending_prompt.take() {
                            stop(&id, "end_turn");
                        }
                    }
                }
                None if id == json!(101) => {
                    let action = message
                        .pointer("/result/action")
                        .and_then(Value::as_str)
                        .unwrap_or("?");
                    record(count, &format!("elicit-url:{action}"));
                    if let Some(id) = pending_auth.take() {
                        if action == "accept" {
                            say(json!({"jsonrpc": "2.0", "id": id, "result": {}}));
                        } else {
                            say(
                                json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32000, "message": "sign-in was declined"}}),
                            );
                        }
                    }
                }
                None if id == json!(102) => {
                    let action = message
                        .pointer("/result/action")
                        .and_then(Value::as_str)
                        .unwrap_or("?");
                    record(count, &format!("elicit-mcp:{action}"));
                    if let Some(id) = pending_prompt.take() {
                        chunk(&session, "Hello");
                        stop(&id, "end_turn");
                    }
                }
                None if id == json!(100) => {
                    let action = message
                        .pointer("/result/action")
                        .and_then(Value::as_str)
                        .unwrap_or("?")
                        .to_string();
                    record(count, &format!("elicit:{action}"));
                    if action == "accept" {
                        let value = message
                            .pointer("/result/content/strategy")
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        record(count, &format!("elicit-value:{value}"));
                        chunk(&session, &format!("ok:{value}"));
                    } else if action == "decline" {
                        chunk(&session, "ok:declined");
                    }
                    if action == "accept" || action == "decline" {
                        if let Some(id) = pending_prompt.take() {
                            stop(&id, "end_turn");
                        }
                    }
                }
                None => {}
            }
        }
    }

    /// One launcher script for every Fixture in this test process. macOS vets
    /// the first exec of each new executable file, one file at a time, so a
    /// script per Fixture queues parallel tests past the probe's 3 s.
    fn fake_agent_wrapper() -> &'static Path {
        static WRAPPER: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
        WRAPPER.get_or_init(|| {
            let exe = std::env::current_exe().unwrap();
            let test = module_path!()
                .split_once("::")
                .map_or("", |(_, rest)| rest)
                .to_string()
                + "::fake_acp_agent";
            let dir = std::env::temp_dir().join(format!("fidget-harness-launcher-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();

            #[cfg(unix)]
            let path = {
                use std::os::unix::fs::PermissionsExt;
                let path = dir.join("launcher-wrapper.sh");
                let contents = format!(
                    "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then\n  echo \"fake-acp-agent 1.0.0\"\n  exit 0\nfi\nexec '{}' {} --exact --nocapture --test-threads=1 \"$@\"\n",
                    exe.display(),
                    test,
                );
                std::fs::write(&path, contents).unwrap();
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
                path
            };

            #[cfg(windows)]
            let path = {
                let path = dir.join("launcher-wrapper.bat");
                let contents = format!(
                    "@echo off\nif \"%~1\"==\"--version\" (\n  echo fake-acp-agent 1.0.0\n  exit /b 0\n)\n\"{}\" {} --exact --nocapture --test-threads=1 %*\n",
                    exe.display(),
                    test,
                );
                std::fs::write(&path, contents).unwrap();
                path
            };

            path
        })
    }

    struct Fixture {
        dir: PathBuf,
        cwd: PathBuf,
        count: PathBuf,
        forwarded: Receiver<Forwarded>,
    }

    impl Fixture {
        fn new(script: &str) -> (Self, Session) {
            Self::build(script, false)
        }

        fn split(script: &str) -> (Self, Session) {
            Self::build(script, true)
        }

        fn build(script: &str, split: bool) -> (Self, Session) {
            let dir = std::env::temp_dir().join(format!("fidget-harness-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).unwrap();
            let cwd = if split {
                let cwd = std::env::temp_dir()
                    .join(format!("fidget-harness-cwd-{}", uuid::Uuid::new_v4()));
                std::fs::create_dir_all(&cwd).unwrap();
                cwd
            } else {
                dir.clone()
            };
            let count = dir.join("count.txt");
            let launch = Launch {
                name: "fake".into(),
                argv: vec![
                    fake_agent_wrapper().to_string_lossy().to_string(),
                    format!("script={script}"),
                    format!("count={}", count.display()),
                ],
            };
            let (tx, forwarded) = mpsc::channel();
            let session = Session::new(
                launch,
                Ok(AttachCwd(cwd.clone())),
                SessionDataDir::at(dir.clone()),
                Arc::new(Box::new(move |forwarded| {
                    let _ = tx.send(forwarded);
                }) as Forward),
            )
            .with_timeout(Duration::from_secs(10));
            (
                Self {
                    dir,
                    cwd,
                    count,
                    forwarded,
                },
                session,
            )
        }

        /// The next forwarded ask, past the plan and thought a turn's end
        /// forwards on the way and the reload a session open forwards, or a
        /// panic naming what came instead.
        fn ask(&self) -> PermissionAsk {
            loop {
                match self.forwarded.recv_timeout(Duration::from_secs(5)) {
                    Ok(Forwarded::Ask { ask, .. }) => return ask,
                    Ok(
                        Forwarded::Plan { .. }
                        | Forwarded::Thought { .. }
                        | Forwarded::InboundWake(_)
                        | Forwarded::Restored(_)
                        | Forwarded::AttachSettled,
                    ) => {}
                    other => panic!("expected an ask, got {:?}", other.map(|_| "settled")),
                }
            }
        }

        fn form(&self) -> ElicitationForm {
            loop {
                match self.forwarded.recv_timeout(Duration::from_secs(5)) {
                    Ok(Forwarded::Form { form, .. }) => return form,
                    Ok(Forwarded::Restored(_) | Forwarded::AttachSettled) => {}
                    other => panic!("expected a form, got {:?}", other.map(|_| "other")),
                }
            }
        }

        fn initialize_params(&self) -> Value {
            let path = self.dir.join("initialize.json");
            let until = Instant::now() + Duration::from_secs(5);
            while Instant::now() < until {
                if let Ok(text) = std::fs::read_to_string(&path) {
                    return serde_json::from_str(&text).expect("initialize.json is JSON");
                }
                thread::sleep(Duration::from_millis(20));
            }
            panic!("initialize.json was never written");
        }

        /// The next forwarded settlement, the request and what won it.
        fn settled(&self) -> (String, Option<String>) {
            loop {
                match self.forwarded.recv_timeout(Duration::from_secs(5)) {
                    Ok(Forwarded::Settled { request, option }) => return (request, option),
                    Ok(Forwarded::Restored(_) | Forwarded::AttachSettled) => {}
                    other => panic!("expected a settlement, got {:?}", other.map(|_| "ask")),
                }
            }
        }

        /// The next forwarded reload of the Chat surface, past the plan and
        /// thought a turn's end forwards on the way, or a panic naming what
        /// came instead.
        fn attach_settled(&self) {
            loop {
                match self.forwarded.recv_timeout(Duration::from_secs(5)) {
                    Ok(Forwarded::AttachSettled) => return,
                    Ok(
                        Forwarded::Plan { .. } | Forwarded::Thought { .. } | Forwarded::Restored(_),
                    ) => {}
                    other => panic!("expected AttachSettled, got {other:?}"),
                }
            }
        }

        fn count(&self, what: &str) -> usize {
            recorded(Some(&self.count), what)
        }

        /// Every Action Log line of one kind, oldest first.
        fn events(&self, event: &str) -> Vec<Value> {
            let text = std::fs::read_to_string(self.dir.join(action_log::FILE)).unwrap_or_default();
            text.lines()
                .filter_map(|line| serde_json::from_str::<Value>(line).ok())
                .filter(|line| line["event"] == json!(event))
                .collect()
        }

        fn wait_for(&self, what: &str, n: usize) -> bool {
            let until = Instant::now() + Duration::from_secs(5);
            while Instant::now() < until {
                if self.count(what) >= n {
                    return true;
                }
                thread::sleep(Duration::from_millis(20));
            }
            false
        }
    }

    /// A Session for a test that never reaches a permission request.
    fn silent() -> Arc<Forward> {
        Arc::new(Box::new(|_| {}) as Forward)
    }

    /// One reactive wake for `buddy-1` as BMO, which is every turn a test
    /// sends unless it is naming another identity.
    fn asking(prompt: &str) -> WakeRequest {
        asking_as("buddy-1", "bmo", prompt)
    }

    fn asking_as(instance: &str, character: &str, prompt: &str) -> WakeRequest {
        WakeRequest {
            prompt: prompt.to_string(),
            instance: instance.to_string(),
            character: character.to_string(),
            reactive: true,
            blank: false,
        }
    }

    /// The same wake, arriving on the Director's own backoff rather than
    /// because the user did something. ADR-0008 names the two kinds.
    fn proactive(prompt: &str) -> WakeRequest {
        WakeRequest {
            reactive: false,
            ..asking(prompt)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
            if self.cwd != self.dir {
                let _ = std::fs::remove_dir_all(&self.cwd);
            }
        }
    }

    fn isolated_session(launch: Launch, dir: PathBuf, forward: Arc<Forward>) -> Session {
        Session::new(
            launch,
            Ok(AttachCwd(dir.clone())),
            SessionDataDir::at(dir),
            forward,
        )
    }

    fn launched(source: &str) -> Target {
        Target {
            launch: launch(Some(source)).unwrap(),
            cwd: Ok(AttachCwd::resolve("").expect("data_dir is always a path")),
        }
    }

    fn tmp_attach() -> AttachCwd {
        AttachCwd(PathBuf::from("/tmp"))
    }

    #[test]
    fn launch_table_names_the_four_shapes_and_leaves_http_alone_when_unset() {
        assert_eq!(launch(None), None);
        assert_eq!(launch(Some("")), None);
        assert_eq!(launch(Some("  ")), None);
        let claude = launch(Some("claude")).unwrap();
        assert_eq!(claude.name, "claude");
        assert_eq!(
            claude.argv,
            ["npx", "-y", "@agentclientprotocol/claude-agent-acp@latest"]
        );
        assert_eq!(
            launch(Some("codex")).unwrap().argv,
            ["npx", "-y", "@agentclientprotocol/codex-acp@latest"]
        );
        let copilot = launch(Some("copilot")).unwrap();
        assert_eq!(copilot.name, "copilot");
        assert_eq!(copilot.argv, ["copilot", "--acp"]);
        assert_eq!(
            launch(Some("grok")).unwrap().argv,
            ["grok", "agent", "stdio"]
        );
        assert_eq!(launch(Some("hermes")).unwrap().argv, ["hermes", "acp"]);
        let goose = launch(Some("goose")).unwrap();
        assert_eq!(goose.name, "goose");
        assert_eq!(goose.argv, ["goose", "acp"]);
        assert_eq!(launch(Some("opencode")).unwrap().argv, ["opencode", "acp"]);
        assert_eq!(
            launch(Some("pi")).unwrap().argv,
            ["npx", "-y", "pi-acp@latest"]
        );
        let antigravity = launch(Some("antigravity")).unwrap();
        assert_eq!(antigravity.name, "antigravity");
        #[cfg(target_os = "macos")]
        assert_eq!(antigravity.argv, ["agy_acp_server.par"]);
        #[cfg(target_os = "linux")]
        assert_eq!(antigravity.argv, ["agy_acp_server.par", "--uid="]);
        #[cfg(windows)]
        assert_eq!(antigravity.argv, ["agy_acp_server.exe"]);
        let custom = launch(Some("  my-agent --acp  --quiet ")).unwrap();
        assert_eq!(custom.name, "my-agent");
        assert_eq!(custom.argv, ["my-agent", "--acp", "--quiet"]);
    }

    #[test]
    fn codex_inline_config_approves_only_the_fidget_server() {
        let (calls, _rx) = mpsc::channel();
        let endpoint = crate::mcp_http::serve(calls).unwrap();
        let config = codex_fidget_config(
            Some(r#"{"model":"example","mcp_servers":{"other":{"command":"other"}}}"#),
            &endpoint,
        )
        .unwrap();
        let parsed: Value = serde_json::from_str(&config).unwrap();
        assert_eq!(parsed["model"], "example");
        assert_eq!(parsed["mcp_servers"]["other"]["command"], "other");
        let server = format!("fidget_attached_{}", std::process::id());
        assert_eq!(parsed["mcp_servers"][server.as_str()]["url"], endpoint.url);
        assert_eq!(
            parsed["mcp_servers"][server.as_str()]["bearer_token_env_var"],
            fidget_mcp_server::TOKEN_VAR
        );
        assert_eq!(
            parsed["mcp_servers"][server.as_str()]["default_tools_approval_mode"],
            "approve"
        );
        assert!(!config.contains(&endpoint.registration().1));
        assert!(codex_fidget_config(Some("invalid json"), &endpoint).is_none());
        let existing_fidget = codex_fidget_config(
            Some(r#"{"mcp_servers":{"fidget":{"command":"own","default_tools_approval_mode":"prompt"}}}"#),
            &endpoint,
        )
        .unwrap();
        let existing_fidget: Value = serde_json::from_str(&existing_fidget).unwrap();
        assert_eq!(existing_fidget["mcp_servers"]["fidget"]["command"], "own");
        assert_eq!(
            existing_fidget["mcp_servers"]["fidget"]["default_tools_approval_mode"],
            "prompt"
        );
        let conflicting = format!(r#"{{"mcp_servers":{{"{server}":{{"command":"own"}}}}}}"#);
        assert!(codex_fidget_config(Some(&conflicting), &endpoint).is_none());

        let (fx, mut session) = Fixture::new("hello");
        session.launch.name = "codex".into();
        let expected_acp_server = usize::from(session.launch.codex_mcp_config().is_none());
        assert_eq!(
            session.complete(&asking("hi"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        assert_eq!(fx.count("mcp=http"), expected_acp_server);
        session.shutdown();
    }

    /// Detach takes the token back out of `.cursor/mcp.json` and leaves the
    /// `Mcp(fidget:*)` allow in `.cursor/cli.json` for the next attach.
    #[test]
    fn a_cursor_attach_leaves_its_tool_allow_after_detach() {
        let (calls, _rx) = mpsc::channel();
        assert!(crate::mcp_http::serve(calls).is_some());
        let (fx, mut session) = Fixture::new("hello");
        session.launch.name = "cursor-agent".into();
        let cursor = fx.cwd.join(".cursor");
        let read = |name: &str| -> Value {
            serde_json::from_str(&std::fs::read_to_string(cursor.join(name)).unwrap()).unwrap()
        };
        let allowed = json!({"permissions": {"allow": ["Mcp(fidget:*)"]}});

        assert_eq!(
            session.complete(&asking("hi"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        assert_eq!(read("cli.json"), allowed);
        assert!(read("mcp.json")["mcpServers"]["fidget"]["url"].is_string());

        session.shutdown();
        assert_eq!(read("cli.json"), allowed);
        assert!(
            !cursor.join("mcp.json").exists(),
            "the token outlived detach"
        );
    }

    #[test]
    fn opencode_inline_policy_preserves_other_permissions() {
        let config = opencode_fidget_permissions(Some(
            r#"{"model":"example/model","permission":{"bash":"ask","other_*":"deny"}}"#,
        ))
        .unwrap();
        let config: Value = serde_json::from_str(&config).unwrap();
        assert_eq!(config["model"], "example/model");
        assert_eq!(config["permission"]["bash"], "ask");
        assert_eq!(config["permission"]["other_*"], "deny");
        for tool in fidget_core::dispatch::list_tools() {
            assert_eq!(
                config["permission"][format!("fidget_{}", tool.name)],
                "allow"
            );
        }
        assert!(config["permission"].get("fidget_*").is_none());
        assert!(config["permission"].get("fidget_extra_speak").is_none());

        let global = opencode_fidget_permissions(Some(r#"{"permission":"ask"}"#)).unwrap();
        let global: Value = serde_json::from_str(&global).unwrap();
        assert_eq!(global["permission"]["*"], "ask");
        assert_eq!(global["permission"]["fidget_speak"], "allow");

        let explicit =
            opencode_fidget_permissions(Some(r#"{"permission":{"fidget_*":"ask"}}"#)).unwrap();
        let explicit: Value = serde_json::from_str(&explicit).unwrap();
        assert_eq!(explicit["permission"]["fidget_*"], "ask");
        assert!(opencode_fidget_permissions(Some("invalid json")).is_none());
    }

    #[test]
    fn mcp_launch_permissions_are_scoped_to_fidget() {
        let (calls, _rx) = mpsc::channel();
        assert!(crate::mcp_http::serve(calls).is_some());
        let args = |name, mcp_available| {
            launch(Some(name))
                .unwrap()
                .command(&tmp_attach(), mcp_available)
                .get_args()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(args("copilot", true), ["--acp", "--allow-tool=fidget"]);
        assert_eq!(
            args("grok", true),
            ["--allow", "MCPTool(fidget__*)", "agent", "stdio"]
        );
        assert_eq!(args("goose", true), ["acp"]);
        assert_eq!(args("copilot", false), ["--acp"]);
        assert_eq!(args("grok", false), ["agent", "stdio"]);
    }

    /// A thought is forwarded to the Chat surface of the Instance whose session
    /// thought it, and written nowhere. The Action Log points at the Harness's
    /// own session dump rather than copying it (CONTEXT.md). Replies are not in
    /// it either (ADR-0034).
    #[test]
    fn a_thought_reaches_its_instances_surface_and_not_the_action_log() {
        let dir = std::env::temp_dir().join(format!("fidget-thought-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let (tx, forwarded) = mpsc::channel();
        let forward = Box::new(move |what| {
            let _ = tx.send(what);
        }) as Forward;

        let owners = Mutex::new(HashMap::from([
            ("session-a".to_string(), "buddy-a".to_string()),
            ("session-b".to_string(), "buddy-b".to_string()),
        ]));

        note_event(
            &dir,
            &forward,
            &Mutex::default(),
            &owners,
            &Mutex::default(),
            Event::Thought {
                session: "session-b".to_string(),
                text: "Reading the roster".to_string(),
            },
        );

        assert!(matches!(
            forwarded.try_recv(),
            Ok(Forwarded::Thought { instance, line })
                if instance == "buddy-b" && line == "Reading the roster"
        ));
        assert!(!dir.join(action_log::FILE).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The steps go to the reader on the Chat surface, and the Action Log
    /// keeps the count. The log points at the Harness's own session dump
    /// rather than copying it (CONTEXT.md). The step text is that copy.
    #[test]
    fn a_plan_reaches_the_surface_and_the_action_log_keeps_the_count() {
        let dir = std::env::temp_dir().join(format!("fidget-plan-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let (tx, forwarded) = mpsc::channel();
        let forward = Box::new(move |what| {
            let _ = tx.send(what);
        }) as Forward;

        note_event(
            &dir,
            &forward,
            &Mutex::default(),
            &Mutex::new(HashMap::from([(
                "session-a".to_string(),
                "buddy-a".to_string(),
            )])),
            &Mutex::default(),
            Event::Plan {
                session: "session-a".to_string(),
                steps: vec![PlanStep {
                    content: "read the roster".to_string(),
                    priority: "high".to_string(),
                    status: "in_progress".to_string(),
                }],
            },
        );

        assert!(matches!(
            forwarded.try_recv(),
            Ok(Forwarded::Plan { instance, steps })
                if instance == "buddy-a" && steps[0].content == "read the roster"
        ));
        let logged = std::fs::read_to_string(dir.join(action_log::FILE)).unwrap();
        assert!(logged.contains(r#""entries":1"#), "{logged}");
        assert!(!logged.contains("read the roster"), "{logged}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A form no session scopes, as a sign-in link, is owed by the Instance
    /// whose turn is running. Between turns nobody owes it, so every Chat
    /// draws it and whichever is open can answer (#1422).
    #[test]
    fn a_form_no_session_scopes_goes_to_the_turn_holder_or_to_every_chat() {
        let dir = std::env::temp_dir().join(format!("fidget-unscoped-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let (tx, forwarded) = mpsc::channel();
        let forward = Box::new(move |what| {
            let _ = tx.send(what);
        }) as Forward;
        let sign_in = |request: &str| Event::Elicitation {
            session: None,
            form: ElicitationForm {
                request: request.to_string(),
                message: "Enter ABCD-1234 on the sign-in page.".to_string(),
                field: String::new(),
                options: Vec::new(),
                url: Some("https://example.test/device?code=ABCD-1234".to_string()),
                waits: false,
            },
        };
        let serving = Mutex::new(Some("buddy-a".to_string()));
        let owners = Mutex::default();

        note_event(
            &dir,
            &forward,
            &Mutex::default(),
            &owners,
            &serving,
            sign_in("1"),
        );
        *serving.lock().unwrap() = None;
        note_event(
            &dir,
            &forward,
            &Mutex::default(),
            &owners,
            &serving,
            sign_in("2"),
        );

        let owed: Vec<_> = forwarded
            .try_iter()
            .map(|row| match row {
                Forwarded::Form { owner, form } => (form.request, owner),
                other => panic!("expected a form, got {other:?}"),
            })
            .collect();
        assert_eq!(
            owed,
            [
                ("1".to_string(), Owner::Instance("buddy-a".to_string())),
                ("2".to_string(), Owner::EveryChat),
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The clear `end_turn` fires on every turn. It has to reach the surface
    /// and leave the Action Log alone. A line per turn saying zero steps is
    /// noise about a turn that never planned.
    #[test]
    fn an_empty_plan_clears_the_surface_and_writes_no_log_line() {
        let dir = std::env::temp_dir().join(format!("fidget-plan-end-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let (tx, forwarded) = mpsc::channel();
        let forward = Box::new(move |what| {
            let _ = tx.send(what);
        }) as Forward;

        note_event(
            &dir,
            &forward,
            &Mutex::default(),
            &Mutex::new(HashMap::from([(
                "session-a".to_string(),
                "buddy-a".to_string(),
            )])),
            &Mutex::default(),
            Event::Plan {
                session: "session-a".to_string(),
                steps: Vec::new(),
            },
        );

        assert!(matches!(
            forwarded.try_recv(),
            Ok(Forwarded::Plan { instance, steps }) if instance == "buddy-a" && steps.is_empty()
        ));
        assert!(!dir.join(action_log::FILE).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Asserted through `reattach` rather than a live retarget, because the
    /// attachment is process-global. A set Harness is the Completer even if
    /// silent (ADR-0008). Only Off reaches `Drop`.
    #[test]
    fn only_the_source_row_moves_the_attachment_and_only_off_drops_it() {
        let hermes = launched("hermes");
        let opencode = launched("opencode");

        assert_eq!(reattach(None, None), Reattach::Stand);
        assert_eq!(
            reattach(Some(&hermes), Some(hermes.clone())),
            Reattach::Stand,
            "a dead child is the Session's own retry, not a reason to rebuild it"
        );
        // The preset and the command line it joins to are one Harness, so
        // re-picking the same one another way keeps the session it has.
        assert_eq!(
            reattach(Some(&hermes), Some(launched("hermes acp"))),
            Reattach::Stand
        );
        assert_eq!(
            reattach(Some(&hermes), Some(opencode.clone())),
            Reattach::Open(opencode.clone())
        );
        assert_eq!(
            reattach(None, Some(opencode.clone())),
            Reattach::Open(opencode)
        );
        assert_eq!(reattach(Some(&hermes), None), Reattach::Drop);
    }

    #[test]
    fn reattach_stands_on_the_same_resolved_cwd_and_opens_on_a_different_one() {
        crate::model::tests::with_env(None, None, None, || {
            let hermes = launched("hermes");
            let data = fidget_core::memory::data_dir();
            let data_row = data.to_string_lossy().into_owned();
            let same_data = Target::from_settings(Some("hermes"), &data_row).unwrap();
            assert_eq!(
                reattach(Some(&hermes), Some(same_data)),
                Reattach::Stand,
                "empty and an explicit data_dir resolve equal"
            );

            let home = fidget_core::memory::home_dir().expect("the test user has a home");
            let home_row = home.to_string_lossy().into_owned();
            let named_home = Target::from_settings(Some("hermes"), &home_row).unwrap();
            assert!(
                matches!(reattach(Some(&hermes), Some(named_home)), Reattach::Open(_)),
                "explicit home is a different project from empty"
            );

            let other_dir = std::env::temp_dir();
            let other = Target {
                launch: hermes.launch.clone(),
                cwd: AttachCwd::resolve(&other_dir.to_string_lossy()),
            };
            assert!(
                matches!(
                    reattach(Some(&hermes), Some(other.clone())),
                    Reattach::Open(_)
                ),
                "same Launch plus a different cwd must rebuild"
            );
            assert_eq!(reattach(Some(&other), Some(other.clone())), Reattach::Stand);
        });
    }

    #[test]
    fn attach_cwd_resolve_empty_is_data_dir_absolute_kept_relative_refused() {
        crate::model::tests::with_env(None, None, None, || {
            let data = fidget_core::memory::data_dir();
            assert_eq!(AttachCwd::resolve("").unwrap().as_path(), data.as_path());
            assert_eq!(AttachCwd::resolve("   ").unwrap().as_path(), data.as_path());
            // `/tmp/...` is relative on Windows (`Path::is_absolute` wants a drive).
            let kept = std::env::temp_dir().join("kept");
            assert_eq!(
                AttachCwd::resolve(&kept.to_string_lossy())
                    .unwrap()
                    .as_path(),
                kept.as_path()
            );
            assert_eq!(
                AttachCwd::resolve("relative/project"),
                Err(CwdError::Relative(PathBuf::from("relative/project")))
            );

            let from_env = std::env::temp_dir().join("from-env");
            let from_file = std::env::temp_dir().join("from-file");
            std::env::set_var(CWD, from_env.as_os_str());
            let target =
                Target::from_settings(Some("hermes"), &from_file.to_string_lossy()).unwrap();
            assert_eq!(
                target.cwd.unwrap().as_path(),
                from_env.as_path(),
                "env outranks the file row"
            );
            std::env::remove_var(CWD);
        });
    }

    /// The Working directory row shows this path instead of naming the data
    /// folder in words (#913). A placeholder that stopped resolving would put
    /// a path on screen that the attach does not use.
    #[test]
    fn working_directory_placeholder_is_the_path_a_blank_row_runs_in() {
        crate::model::tests::with_env(None, None, None, || {
            assert_eq!(
                attach_cwd_placeholder(),
                fidget_core::memory::data_dir().display().to_string()
            );

            let from_env = std::env::temp_dir().join("from-env");
            std::env::set_var(CWD, from_env.as_os_str());
            let owned = attach_cwd_placeholder();
            std::env::remove_var(CWD);
            assert_eq!(
                owned,
                from_env.display().to_string(),
                "a row the environment owns shows where it actually runs"
            );
        });
    }

    #[test]
    fn session_new_cwd_is_the_attach_dir_and_durable_files_live_in_the_store() {
        let (fx, session) = Fixture::split("happy");
        assert_eq!(
            session.complete(&asking("hi"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        assert_eq!(
            fx.count(&format!("cwd={}", fx.cwd.display())),
            1,
            "session/new cwd was not the attach dir"
        );
        assert_eq!(
            fx.count(&format!("cwd={}", fx.dir.display())),
            0,
            "session/new cwd was the store"
        );
        assert!(fx.dir.join(SESSION_FILE).is_file());
        assert!(fx.dir.join(action_log::FILE).is_file());
        assert!(!fx.cwd.join(SESSION_FILE).exists());
        assert!(!fx.cwd.join(action_log::FILE).exists());
        assert!(
            std::fs::read_dir(&fx.cwd).unwrap().next().is_none(),
            "Fidget writes nothing into the user's directory"
        );
        session.shutdown();
    }

    #[test]
    fn session_load_cwd_is_the_attach_dir_not_the_store() {
        let (fx, session) = Fixture::split("load");
        std::fs::write(
            fx.dir.join(SESSION_FILE),
            r#"{"harness":"fake","sessions":[{"instance":"buddy-1","character":"bmo","session_id":"saved-ok"}]}"#,
        )
        .unwrap();
        assert_eq!(
            session.complete(&asking("hi"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        assert_eq!(fx.count("load"), 1);
        assert_eq!(
            fx.count(&format!("cwd={}", fx.cwd.display())),
            1,
            "session/load cwd was not the attach dir"
        );
        assert_eq!(
            fx.count(&format!("cwd={}", fx.dir.display())),
            0,
            "session/load cwd was the store"
        );
        session.shutdown();
    }

    #[test]
    fn a_relative_cwd_fails_spawn_and_does_not_create_the_path() {
        let relative = PathBuf::from(format!("fidget-rel-cwd-{}", uuid::Uuid::new_v4()));
        let data = std::env::temp_dir().join(format!("fidget-rel-data-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&data).unwrap();
        let launch = Launch {
            name: "nope".into(),
            argv: vec!["/nonexistent/fidget-no-such-harness".into()],
        };
        let session = Session::new(
            launch,
            Err(CwdError::Relative(relative.clone())),
            SessionDataDir::at(data.clone()),
            silent(),
        );
        let err = session.complete(&asking("hi"), &|_| {}).unwrap_err();
        assert!(
            err.contains(&relative.display().to_string()),
            "spawn named the relative path, got {err}"
        );
        assert!(!relative.exists(), "spawn must not create a relative cwd");
        let _ = std::fs::remove_dir_all(data);
    }

    #[test]
    fn a_missing_cwd_fails_spawn_and_does_not_create_the_path() {
        let missing =
            std::env::temp_dir().join(format!("fidget-missing-cwd-{}", uuid::Uuid::new_v4()));
        assert!(!missing.exists());
        let data =
            std::env::temp_dir().join(format!("fidget-missing-data-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&data).unwrap();
        let launch = Launch {
            name: "nope".into(),
            argv: vec!["/nonexistent/fidget-no-such-harness".into()],
        };
        let session = Session::new(
            launch,
            AttachCwd::resolve(&missing.to_string_lossy()),
            SessionDataDir::at(data.clone()),
            silent(),
        );
        let err = session.complete(&asking("hi"), &|_| {}).unwrap_err();
        assert!(
            err.contains(&missing.display().to_string()),
            "spawn named the missing path, got {err}"
        );
        assert!(!missing.exists(), "spawn must not create a missing cwd");
        let _ = std::fs::remove_dir_all(data);
    }

    #[test]
    fn spawn_creates_the_data_dir_we_own_before_checking_cwd() {
        let dir = std::env::temp_dir().join(format!("fidget-default-cwd-{}", uuid::Uuid::new_v4()));
        assert!(!dir.exists());
        let launch = Launch {
            name: "nope".into(),
            argv: vec!["/nonexistent/fidget-no-such-harness".into()],
        };
        let session = Session::new(
            launch,
            Ok(AttachCwd(dir.clone())),
            SessionDataDir::at(dir.clone()),
            silent(),
        );
        let _ = session.complete(&asking("hi"), &|_| {});
        assert!(
            dir.is_dir(),
            "empty default is a folder we own; spawn must create it"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn probe_layout_isolates_the_store_and_keeps_production_cwd() {
        crate::model::tests::with_env(None, None, None, || {
            let target = Target::from_settings(Some("hermes"), "").unwrap();
            let session =
                Session::new(target.launch, target.cwd, SessionDataDir::probe(), silent());
            assert!(
                session.data.as_path().ends_with("probe"),
                "got {}",
                session.data.as_path().display()
            );
            let data = fidget_core::memory::data_dir();
            assert_eq!(
                session.cwd.as_ref().unwrap().as_path(),
                data.as_path(),
                "probe cwd must be production resolve, not the probe folder"
            );
            assert_ne!(session.data.as_path(), data.as_path());
        });
    }

    /// Provider login stays inherited; only scoped MCP transport or approval
    /// configuration is added to child environments.
    #[test]
    fn child_command_passes_only_scoped_env_and_no_bare() {
        for name in [
            "claude",
            "codex",
            "copilot",
            "cursor-agent",
            "goose",
            "grok",
            "hermes",
            "opencode",
            "pi",
            "antigravity",
        ] {
            let launch = launch(Some(name)).unwrap();
            let endpoint_was_served = crate::mcp_http::endpoint().is_some();
            let inline_config = std::env::var("OPENCODE_CONFIG_CONTENT").ok();
            let codex_config = launch.codex_mcp_config();
            let command = launch.command(&tmp_attach(), true);
            let mut env: Vec<_> = command
                .get_envs()
                .map(|(key, _)| key.to_string_lossy().into_owned())
                .collect();
            env.sort();
            if name == "pi" {
                let expected = vec![
                    fidget_mcp_server::TOKEN_VAR.to_string(),
                    fidget_mcp_server::URL_VAR.to_string(),
                ];
                if endpoint_was_served {
                    assert_eq!(env, expected);
                } else {
                    assert!(env.is_empty() || env == expected);
                }
            } else if name == "codex" {
                if let Some((config, _)) = codex_config {
                    assert_eq!(env, ["CODEX_CONFIG", fidget_mcp_server::TOKEN_VAR]);
                    assert_eq!(
                        command
                            .get_envs()
                            .find(|(key, _)| *key == "CODEX_CONFIG")
                            .and_then(|(_, value)| value)
                            .and_then(|value| value.to_str()),
                        Some(config.as_str())
                    );
                } else {
                    assert!(env.is_empty());
                }
            } else if name == "opencode" {
                let expected = endpoint_was_served
                    .then(|| opencode_fidget_permissions(inline_config.as_deref()))
                    .flatten();
                let expected_env: Vec<String> = if expected.is_some() {
                    vec!["OPENCODE_CONFIG_CONTENT".into()]
                } else {
                    Vec::new()
                };
                assert_eq!(env, expected_env);
                if let Some(expected) = expected {
                    assert_eq!(
                        command
                            .get_envs()
                            .next()
                            .and_then(|(_, value)| value)
                            .and_then(|value| value.to_str()),
                        Some(expected.as_str())
                    );
                }
            } else {
                assert!(env.is_empty(), "{name} sets {env:?}");
            }
            assert_eq!(command.get_current_dir(), Some(Path::new("/tmp")));
            assert!(
                !launch.argv.iter().any(|arg| arg == "--bare"),
                "{name} passes --bare"
            );
            assert!(!launch
                .argv
                .iter()
                .any(|arg| arg.contains("CLAUDE_CONFIG_DIR") || arg.contains("ANTHROPIC_API_KEY")));
        }
    }

    /// Production change that would fail this. The child stays in the app's
    /// process group, so Ctrl+C SIGINTs Claude's adapter and it dumps
    /// `Query closed before response received` on the way down.
    #[cfg(unix)]
    #[test]
    fn the_harness_child_is_not_in_the_app_process_group() {
        let launch = Launch {
            name: "sleep".into(),
            argv: vec!["/bin/sleep".into(), "8".into()],
        };
        let mut command = launch.command(&tmp_attach(), false);
        command.stdout(std::process::Stdio::null());
        command.stderr(std::process::Stdio::null());
        let mut child = command.spawn().expect("sleep");
        let child_pgid = pgid_of(child.id()).expect("child pgid");
        let app_pgid = pgid_of(std::process::id()).expect("app pgid");
        let _ = child.kill();
        let _ = child.wait();
        assert_ne!(
            child_pgid, app_pgid,
            "Ctrl+C in the terminal would SIGINT the Harness"
        );
    }

    #[cfg(unix)]
    #[test]
    fn an_unowned_interrupt_leaves_the_child_in_the_app_group() {
        let mut command = std::process::Command::new("/bin/sleep");
        command.arg("8");
        command.stdout(std::process::Stdio::null());
        command.stderr(std::process::Stdio::null());
        apply_isolation(&mut command, false);
        let mut child = command.spawn().expect("sleep");
        let child_pgid = pgid_of(child.id()).expect("child pgid");
        let app_pgid = pgid_of(std::process::id()).expect("app pgid");
        let _ = child.kill();
        let _ = child.wait();
        assert_eq!(
            child_pgid, app_pgid,
            "a failed ctrlc handler must not orphan a tree Ctrl+C can no longer reap"
        );
    }

    /// `kill_harness_tree` SIGKILLs a group without checking whose it is, so a
    /// child that never left ours takes this process with it. Asserted through
    /// the predicate. Calling it on our group would SIGKILL this test binary.
    #[cfg(unix)]
    #[test]
    fn our_own_group_is_never_killable() {
        let shared = sleep_pgid(false);
        let isolated = sleep_pgid(true);
        assert_eq!(
            Some(shared),
            pgid_of(std::process::id()),
            "the unisolated child under test has to share our group"
        );
        assert!(
            !crate::acp_wire::killable_group(shared as libc::pid_t),
            "SIGKILLing this group would take the probe, cargo test and the shell with it"
        );
        assert!(
            crate::acp_wire::killable_group(isolated as libc::pid_t),
            "an isolated child's group is the one kill_harness_tree exists to reap"
        );
    }

    /// The process group a `/bin/sleep` spawned under
    /// `apply_isolation(isolate)` landed in. The child is reaped before this
    /// returns; only its group is of interest.
    #[cfg(unix)]
    fn sleep_pgid(isolate: bool) -> i32 {
        let mut command = std::process::Command::new("/bin/sleep");
        command.arg("8");
        command.stdout(std::process::Stdio::null());
        command.stderr(std::process::Stdio::null());
        apply_isolation(&mut command, isolate);
        let mut child = command.spawn().expect("sleep");
        let pgid = pgid_of(child.id()).expect("child pgid");
        let _ = child.kill();
        let _ = child.wait();
        pgid
    }

    #[cfg(unix)]
    #[test]
    fn shutdown_kills_the_harness_process_group() {
        let launch = Launch {
            name: "shell".into(),
            argv: vec![
                "/bin/sh".into(),
                "-c".into(),
                r#"trap "" HUP; sleep 30 & wait"#.into(),
            ],
        };
        let mut command = launch.command(&tmp_attach(), false);
        command.stdout(std::process::Stdio::null());
        command.stderr(std::process::Stdio::null());
        let mut child = command.spawn().expect("sh");
        let pgid = pgid_of(child.id()).expect("child pgid");
        let members = wait_for_group_members(pgid, 2);
        crate::acp_wire::kill_harness_tree(child.id());
        let _ = child.kill();
        let _ = child.wait();
        let until = Instant::now() + Duration::from_millis(500);
        let lingering = loop {
            let still: Vec<_> = members
                .iter()
                .copied()
                .filter(|&pid| still_running(pid))
                .collect();
            if still.is_empty() || Instant::now() >= until {
                break still;
            }
            thread::sleep(Duration::from_millis(20));
        };
        assert!(
            lingering.is_empty(),
            "grandchildren survived a direct kill: {lingering:?}"
        );
    }

    #[cfg(unix)]
    fn pgid_of(pid: u32) -> Option<i32> {
        let n = unsafe { libc::getpgid(pid as libc::pid_t) };
        (n >= 0).then_some(n)
    }

    #[cfg(unix)]
    fn alive(pid: u32) -> bool {
        unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
    }

    /// `kill(pid, 0)` is true for a zombie. SIGKILL'd grandchildren show up
    /// that way until init reaps them; they are not still running.
    #[cfg(unix)]
    fn still_running(pid: u32) -> bool {
        let output = std::process::Command::new("ps")
            .args(["-o", "state=", "-p", &pid.to_string()])
            .output()
            .ok();
        let Some(output) = output else {
            return false;
        };
        if !output.status.success() {
            return false;
        }
        matches!(
            String::from_utf8_lossy(&output.stdout)
                .chars()
                .find(|c| !c.is_whitespace()),
            Some(c) if c != 'Z'
        )
    }

    #[cfg(unix)]
    fn live_pids_in_group(pgid: i32) -> Vec<u32> {
        let output = std::process::Command::new("ps")
            .args(["-axo", "pid=,pgid="])
            .output()
            .expect("ps");
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| {
                let mut cols = line.split_whitespace();
                let pid: u32 = cols.next()?.parse().ok()?;
                let group: i32 = cols.next()?.parse().ok()?;
                (group == pgid && alive(pid)).then_some(pid)
            })
            .collect()
    }

    #[cfg(unix)]
    fn wait_for_group_members(pgid: i32, n: usize) -> Vec<u32> {
        let until = Instant::now() + Duration::from_secs(2);
        loop {
            let pids = live_pids_in_group(pgid);
            if pids.len() >= n {
                return pids;
            }
            if Instant::now() >= until {
                panic!("group {pgid} never grew to {n}: {pids:?}");
            }
            thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    fn happy_path_concatenates_chunks_and_records_the_session() {
        let (fx, session) = Fixture::new("happy");
        assert_eq!(
            session.complete(&asking("hi"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        let saved: SavedSession =
            serde_json::from_str(&std::fs::read_to_string(fx.dir.join(SESSION_FILE)).unwrap())
                .unwrap();
        assert_eq!(saved.sessions[0].session_id, "fresh-id");
        assert_eq!(saved.harness, "fake");
        assert_eq!(saved.agent.as_deref(), Some("fake-agent"));
        let inspect = session.inspect();
        assert!(inspect.mcp_http);
        assert_eq!(inspect.agent.as_deref(), Some("fake-agent"));
        assert!(std::fs::read_to_string(fx.dir.join(action_log::FILE))
            .unwrap()
            .contains("\"event\":\"turn\""));
        session.shutdown();
    }

    /// The file is a map of remembered ids, not one pointer the next character
    /// would inherit. Instance and Character together, because a retarget
    /// keeps the Instance and changes the Character Prompt.
    #[test]
    fn the_session_file_keys_the_id_by_instance_and_character() {
        let (fx, session) = Fixture::new("happy");
        assert_eq!(
            session.complete(&asking("hi"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        let saved: Value =
            serde_json::from_str(&std::fs::read_to_string(fx.dir.join(SESSION_FILE)).unwrap())
                .unwrap();
        let slots = saved["sessions"]
            .as_array()
            .expect("remembered ids are keyed, not a single session_id");
        assert_eq!(slots.len(), 1, "{saved}");
        assert_eq!(slots[0]["instance"], json!("buddy-1"));
        assert_eq!(slots[0]["character"], json!("bmo"));
        assert_eq!(slots[0]["session_id"], json!("fresh-id"));
        session.shutdown();
    }

    /// Two Character Instances never share an ACP session, even when they
    /// are the same Character. One Harness child throughout.
    #[test]
    fn two_character_instances_do_not_share_an_acp_session() {
        let (fx, session) = Fixture::new("happy");
        assert_eq!(
            session.complete(&asking_as("buddy-1", "bmo", "hi"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        assert_eq!(
            session.complete(&asking_as("buddy-2", "bmo", "hi"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        session.shutdown();

        let prompts = fx.events("prompt");
        assert_eq!(prompts.len(), 2, "{prompts:?}");
        assert_ne!(
            prompts[0]["session_id"], prompts[1]["session_id"],
            "Instance B continued Instance A's session: {prompts:?}"
        );
        assert_eq!(fx.count("spawn"), 1, "a second Completer was spawned");
        let saved: SavedSession =
            serde_json::from_str(&std::fs::read_to_string(fx.dir.join(SESSION_FILE)).unwrap())
                .unwrap();
        assert_eq!(saved.sessions.len(), 2, "{:?}", saved.sessions);
    }

    /// One child serves every Instance (ADR-0008), so the wire alone does not
    /// say whose turn a thought is. Production change that would fail this: a
    /// thought forwarded without the Instance whose session thought it, which
    /// draws one character's thinking in another character's Chat window.
    #[test]
    fn each_thought_names_the_instance_whose_session_thought_it() {
        let (fx, session) = Fixture::new("thinking");
        assert!(session
            .complete(&asking_as("buddy-a", "rex", "hi"), &|_| {})
            .is_ok());
        assert!(session
            .complete(&asking_as("buddy-b", "rex", "hi"), &|_| {})
            .is_ok());
        session.shutdown();

        let thoughts: Vec<(String, String)> = fx
            .forwarded
            .try_iter()
            .filter_map(|forwarded| match forwarded {
                Forwarded::Thought { instance, line } => Some((instance, line)),
                _ => None,
            })
            .collect();
        let pair = |instance: &str, line: &str| (instance.to_string(), line.to_string());
        assert_eq!(
            thoughts,
            [
                pair("buddy-a", "thinking in fresh-id"),
                pair("buddy-a", ""),
                pair("buddy-b", "thinking in fresh-id-2"),
                pair("buddy-b", ""),
            ]
        );
    }

    /// The Harness Director's Speech reaches the bubble a chunk at a time,
    /// before the turn ends, and the Behavior name never does.
    #[test]
    fn a_harness_answer_is_spoken_as_its_chunks_land() {
        let (_fx, session) = Fixture::new("speaking");
        let director = ModelDirector::new(session, ["wave"], "buddy-1", "bmo", false);
        let heard = std::cell::RefCell::new(Vec::new());

        let woken = director.wake_request(asking("hi"), &|line| heard.borrow_mut().push(line));

        assert_eq!(heard.into_inner(), ["Good ", "Good morning"]);
        assert_eq!(
            woken.wake,
            Wake::Proposed(BehaviorProposal {
                behavior: "wave".to_string(),
                dialogue: Some("Good morning".to_string()),
            })
        );
    }

    /// An Instance Prompt change cannot retrofit the opening turn (ADR-0012),
    /// so the next wake must be `session/new`, not a turn on the old id.
    #[test]
    fn dropping_a_conversation_opens_a_new_acp_session() {
        let (fx, session) = Fixture::new("happy");
        assert_eq!(
            session.complete(&asking("hi"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        session.drop_conversation("buddy-1");
        assert_eq!(
            session.complete(&asking("again"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        session.shutdown();

        let prompts = fx.events("prompt");
        assert_eq!(prompts.len(), 2, "{prompts:?}");
        assert_ne!(
            prompts[0]["session_id"], prompts[1]["session_id"],
            "the new Character Prompt continued the old ACP session: {prompts:?}"
        );
        assert_eq!(fx.count("new"), 2, "session/new was not asked again");
        let saved: SavedSession =
            serde_json::from_str(&std::fs::read_to_string(fx.dir.join(SESSION_FILE)).unwrap())
                .unwrap();
        assert_eq!(saved.sessions.len(), 1, "{:?}", saved.sessions);
        assert_eq!(saved.sessions[0].session_id, "fresh-id-2");
    }

    /// The Instance Prompt is in every lane, so dropping the Instance must
    /// forget Blank AI as well as the shaped conversation.
    #[test]
    fn dropping_an_instance_forgets_every_lane() {
        let (fx, session) = Fixture::new("happy");
        assert_eq!(
            session.complete(&asking("hi"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        let blank = WakeRequest {
            blank: true,
            ..asking("hi")
        };
        assert_eq!(session.complete(&blank, &|_| {}), Ok(Reply::whole("Hello")));
        session.drop_conversation("buddy-1");
        assert_eq!(
            session.complete(&asking("again"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        assert_eq!(session.complete(&blank, &|_| {}), Ok(Reply::whole("Hello")));
        session.shutdown();

        let prompts = fx.events("prompt");
        assert_eq!(prompts.len(), 4, "{prompts:?}");
        assert_ne!(
            prompts[0]["session_id"], prompts[2]["session_id"],
            "the shaped lane continued: {prompts:?}"
        );
        assert_ne!(
            prompts[1]["session_id"], prompts[3]["session_id"],
            "the blank lane continued: {prompts:?}"
        );
        assert_eq!(fx.count("new"), 4);
    }

    /// A restart must not `session/load` the id that save just tore down.
    #[test]
    fn dropping_a_conversation_forgets_the_saved_id() {
        let (fx, session) = Fixture::new("load");
        std::fs::write(
            fx.dir.join(SESSION_FILE),
            r#"{"harness":"fake","sessions":[{"instance":"buddy-1","character":"bmo","session_id":"saved-ok"},{"instance":"buddy-2","character":"bmo","session_id":"id-b"}]}"#,
        )
        .unwrap();
        session.drop_conversation("buddy-1");
        assert_eq!(
            session.complete(&asking("hi"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        assert_eq!(fx.count("load"), 0, "the dropped id was loaded back");
        assert_eq!(fx.count("new"), 1);
        assert_eq!(
            session.complete(&asking_as("buddy-2", "bmo", "again"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        assert_eq!(fx.count("load"), 1, "the other Instance was dropped too");
        session.shutdown();
    }

    /// Start a new session drops every live Instance, so every lane has to
    /// open with `session/new`. A restart between the two would load a
    /// torn-down id back, which is the box this is for.
    #[test]
    fn a_new_session_for_every_instance_loads_no_old_id() {
        let (fx, session) = Fixture::new("load");
        std::fs::write(
            fx.dir.join(SESSION_FILE),
            r#"{"harness":"fake","sessions":[{"instance":"buddy-1","character":"bmo","session_id":"old-a"},{"instance":"buddy-2","character":"bmo","session_id":"old-b"}]}"#,
        )
        .unwrap();
        for instance in ["buddy-1", "buddy-2"] {
            session.drop_conversation(instance);
        }
        assert_eq!(
            session.complete(&asking_as("buddy-1", "bmo", "hi"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        assert_eq!(
            session.complete(&asking_as("buddy-2", "bmo", "hi"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        session.shutdown();

        assert_eq!(fx.count("load"), 0, "a dropped id was loaded back");
        assert_eq!(fx.count("new"), 2, "each Instance opens its own session");
        let prompts = fx.events("prompt");
        assert_eq!(prompts.len(), 2, "{prompts:?}");
        for prompt in &prompts {
            assert_ne!(prompt["session_id"], json!("old-a"), "{prompts:?}");
            assert_ne!(prompt["session_id"], json!("old-b"), "{prompts:?}");
        }
        let saved: SavedSession =
            serde_json::from_str(&std::fs::read_to_string(fx.dir.join(SESSION_FILE)).unwrap())
                .unwrap();
        assert!(
            saved
                .sessions
                .iter()
                .all(|slot| slot.session_id != "old-a" && slot.session_id != "old-b"),
            "{:?}",
            saved.sessions
        );
    }

    /// Dropping one Instance's conversation must not mint a new session for
    /// another Instance that still holds its opening turn.
    #[test]
    fn dropping_one_instance_leaves_the_other_conversation() {
        let (fx, session) = Fixture::new("happy");
        assert_eq!(
            session.complete(&asking_as("buddy-1", "bmo", "a"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        assert_eq!(
            session.complete(&asking_as("buddy-2", "bmo", "b"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        session.drop_conversation("buddy-1");
        assert_eq!(
            session.complete(&asking_as("buddy-2", "bmo", "c"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        session.shutdown();

        let prompts = fx.events("prompt");
        assert_eq!(prompts.len(), 3, "{prompts:?}");
        assert_eq!(
            prompts[1]["session_id"], prompts[2]["session_id"],
            "Instance B was reopened when Instance A saved: {prompts:?}"
        );
    }

    /// Blank-AI mode is a different conversation. One session serving both
    /// would answer a blank prompt out of a Character it was told to forget,
    /// and the mode would measure a prompt it claims not to have sent.
    #[test]
    fn a_blank_wake_does_not_continue_the_shaped_session() {
        let (fx, session) = Fixture::new("happy");
        assert_eq!(
            session.complete(&asking_as("buddy-1", "bmo", "hi"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        let blank = WakeRequest {
            blank: true,
            ..asking_as("buddy-1", "bmo", "hi")
        };
        assert_eq!(session.complete(&blank, &|_| {}), Ok(Reply::whole("Hello")));
        session.shutdown();

        let prompts = fx.events("prompt");
        assert_eq!(prompts.len(), 2, "{prompts:?}");
        assert_ne!(
            prompts[0]["session_id"], prompts[1]["session_id"],
            "the blank wake continued the Character's session: {prompts:?}"
        );
        // Remembered apart too, so a restart resumes each mode where it was
        // rather than loading one into the other.
        let saved: SavedSession =
            serde_json::from_str(&std::fs::read_to_string(fx.dir.join(SESSION_FILE)).unwrap())
                .unwrap();
        assert_eq!(saved.sessions.len(), 2, "{:?}", saved.sessions);
        assert!(
            saved.sessions.iter().any(|slot| slot.blank),
            "the blank session is marked as one: {:?}",
            saved.sessions
        );
    }

    /// Switching away and back resumes the first Instance's session. The
    /// second Instance's id is still remembered.
    #[test]
    fn switching_back_resumes_the_first_instance_session() {
        let (fx, session) = Fixture::new("happy");
        assert_eq!(
            session.complete(&asking_as("buddy-1", "bmo", "a"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        assert_eq!(
            session.complete(&asking_as("buddy-2", "bmo", "b"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        assert_eq!(
            session.complete(&asking_as("buddy-1", "bmo", "c"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        session.shutdown();

        let prompts = fx.events("prompt");
        assert_eq!(prompts.len(), 3, "{prompts:?}");
        assert_eq!(prompts[0]["session_id"], prompts[2]["session_id"]);
        assert_ne!(prompts[0]["session_id"], prompts[1]["session_id"]);
        assert_eq!(
            fx.count("new"),
            2,
            "A was minted again: {}",
            fx.count("new")
        );
        assert_eq!(fx.count("spawn"), 1);
    }

    /// A Character switch is a different identity. Switching back loads the
    /// previous Character's session rather than leaving the new Character
    /// holding the old transcript.
    #[test]
    fn a_character_switch_does_not_keep_the_previous_transcript() {
        let (fx, session) = Fixture::new("happy");
        assert_eq!(
            session.complete(&asking_as("buddy-1", "bmo", "a"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        assert_eq!(
            session.complete(&asking_as("buddy-1", "timber-wolf", "b"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        assert_eq!(
            session.complete(&asking_as("buddy-1", "bmo", "c"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        session.shutdown();

        let prompts = fx.events("prompt");
        assert_eq!(prompts.len(), 3, "{prompts:?}");
        assert_ne!(
            prompts[0]["session_id"], prompts[1]["session_id"],
            "Timber Wolf continued BMO's session"
        );
        assert_eq!(prompts[0]["session_id"], prompts[2]["session_id"]);
        assert_eq!(fx.count("new"), 2);
    }

    /// A new Session reading the file restores each identity's id.
    #[test]
    fn a_restart_loads_each_instance_session_from_the_file() {
        let (fx, session) = Fixture::new("load");
        std::fs::write(
            fx.dir.join(SESSION_FILE),
            r#"{"harness":"fake","sessions":[{"instance":"buddy-1","character":"bmo","session_id":"id-a"},{"instance":"buddy-2","character":"bmo","session_id":"id-b"}]}"#,
        )
        .unwrap();
        assert_eq!(
            session.complete(&asking_as("buddy-1", "bmo", "again"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        assert_eq!(
            session.complete(&asking_as("buddy-2", "bmo", "again"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        session.shutdown();
        let prompts = fx.events("prompt");
        assert_eq!(prompts[0]["session_id"], json!("id-a"));
        assert_eq!(prompts[1]["session_id"], json!("id-b"));
        assert_eq!(fx.count("load"), 2);
        assert_eq!(fx.count("new"), 0, "restart minted instead of loading");
    }

    /// The old single pointer cannot be attributed, so it is not applied
    /// to every character.
    #[test]
    fn an_unattributed_legacy_id_is_not_applied_to_an_instance() {
        let (fx, session) = Fixture::new("load");
        std::fs::write(
            fx.dir.join(SESSION_FILE),
            r#"{"session_id":"saved-ok","harness":"fake"}"#,
        )
        .unwrap();
        assert_eq!(
            session.complete(&asking("hi"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        assert_eq!(fx.count("load"), 0, "the leftover id was applied");
        assert_eq!(fx.count("new"), 1);
        session.shutdown();
    }

    /// Ctrl+C shuts the Harness down, then `RunEvent::Exit` does it again.
    /// The second call must not wait out another reap.
    #[test]
    fn a_second_shutdown_does_not_reap_again() {
        let (_fx, session) = Fixture::new("happy");
        assert_eq!(
            session.complete(&asking("hi"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        session.shutdown();
        let again = Instant::now();
        session.shutdown();
        assert!(
            again.elapsed() < Duration::from_millis(500),
            "the second shutdown waited {:?} — that is another reap",
            again.elapsed()
        );
    }

    /// No child yet. Both calls return, and the second does not panic.
    #[test]
    fn shutdown_before_a_child_exists_is_repeatable() {
        let (_fx, session) = Fixture::new("happy");
        session.shutdown();
        session.shutdown();
    }

    /// Off may race `spawn_preflight`. A spawn that lands after `shutdown`
    /// must not store a live child the HTTP Completer then cannot see.
    #[test]
    fn shutdown_refuses_a_later_turn() {
        let (_fx, session) = Fixture::new("happy");
        assert_eq!(
            session.complete(&asking("hi"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        session.shutdown();
        assert_eq!(
            session.complete(&asking("again"), &|_| {}),
            Err("harness detached".to_string())
        );
    }

    /// `session_id` names the conversation, not the wake. Which Instance woke,
    /// and whether the user asked for it, come through the `WakeRequest`.
    #[test]
    fn the_prompt_event_names_the_instance_and_the_wake_kind() {
        let (fx, session) = Fixture::new("happy");
        assert_eq!(
            session.complete(&asking("hi"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        let proactive = WakeRequest {
            reactive: false,
            ..asking("nobody asked")
        };
        assert_eq!(
            session.complete(&proactive, &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        session.shutdown();

        let prompts = fx.events("prompt");
        assert_eq!(prompts.len(), 2, "{prompts:?}");
        assert_eq!(prompts[0]["instance"], json!("buddy-1"));
        assert_eq!(prompts[0]["wake"], json!("reactive"));
        assert_eq!(prompts[0]["chars"], json!(2));
        assert_eq!(prompts[1]["wake"], json!("proactive"));
        assert_eq!(prompts[1]["session_id"], json!("fresh-id"));
    }

    /// Without this the log said a prompt went out and never what came of it.
    /// Taking what the Harness proposed would have needed the trace flag.
    #[test]
    fn the_parsed_event_says_what_the_reply_became() {
        let spoke = |line: &str| {
            Wake::Proposed(BehaviorProposal {
                behavior: String::new(),
                dialogue: Some(line.to_string()),
            })
        };

        let named = parsed_fields(
            "buddy-1",
            &Wake::Proposed(BehaviorProposal {
                behavior: "prowl".to_string(),
                dialogue: Some("mine now".to_string()),
            }),
            true,
            None,
            None,
            None,
        );
        assert_eq!(named["instance"], json!("buddy-1"));
        assert_eq!(named["wake"], json!("reactive"));
        assert_eq!(named["result"], json!("proposal"));
        assert_eq!(named["behavior"], json!("prowl"));

        // An empty name is the Engine's "talk and speak". The model chose to
        // talk rather than name a Behavior.
        let talked = parsed_fields("buddy-1", &spoke("hello?"), false, None, None, None);
        assert_eq!(talked["wake"], json!("proactive"));
        assert_eq!(talked["result"], json!("speech"));
        assert_eq!(talked["behavior"], json!(null));

        // The same shape as speech on the wire, and a different thing. The
        // name it named is what makes it readable as a miss.
        let missed = parsed_fields(
            "buddy-1",
            &spoke("prowll"),
            true,
            Some("prowll"),
            None,
            None,
        );
        assert_eq!(missed["result"], json!("near_miss"));
        assert_eq!(missed["behavior"], json!("prowll"));

        let failed = parsed_fields("buddy-1", &Wake::Failed, true, None, None, None);
        assert_eq!(failed["result"], json!("failed"));
        assert_eq!(failed["behavior"], json!(null));
        assert_eq!(failed["withdrawn_for"], json!(null));

        // The Harness answered, and what it answered was an error. That is
        // not the same outcome as a reply nothing could be parsed out of.
        let errored = parsed_fields(
            "buddy-1",
            &Wake::Failed,
            true,
            None,
            Some("harness: API Error: 400 does not support this model"),
            None,
        );
        assert_eq!(errored["result"], json!("error"));
        assert_eq!(
            errored["error"],
            json!("harness: API Error: 400 does not support this model")
        );

        // Our own cancel reaches this as an error, and the withdrawal is what
        // the line has to say.
        let withdrawn = parsed_fields(
            "buddy-1",
            &Wake::Failed,
            true,
            None,
            Some("harness stopped: cancelled"),
            Some("buddy-2"),
        );
        assert_eq!(withdrawn["result"], json!("withdrawn"));
        assert_eq!(withdrawn["withdrawn_for"], json!("buddy-2"));
        assert_eq!(withdrawn["error"], json!(null));
    }

    #[test]
    fn a_refusal_is_an_err() {
        let (fx, session) = Fixture::new("refusal");
        let reply = session.complete(&asking("hi"), &|_| {});
        assert!(
            reply.as_ref().is_err_and(|why| why.contains("refusal")),
            "{reply:?}"
        );
        // Only a loaded id is reopened. This session came from `session/new`,
        // so the refusal is the Harness's answer to the prompt.
        assert_eq!(fx.count("new"), 1);
        assert_eq!(fx.count("prompt"), 1);
        // The words the turn came back with outlive it, because every reader
        // downstream otherwise has only "no proposal" to say about a Harness
        // that is answering.
        assert!(
            session
                .inspect()
                .last_error
                .is_some_and(|why| why.contains("refusal")),
            "{:?}",
            session.inspect().last_error
        );
        session.shutdown();
    }

    fn write_completer_model(dir: &std::path::Path, model: &str) {
        let settings = crate::settings::Settings {
            director_model: model.to_string(),
            ..crate::settings::Settings::default()
        };
        settings
            .save(&crate::settings::settings_path(dir))
            .expect("settings");
    }

    fn config_lines(fx: &Fixture) -> Vec<String> {
        std::fs::read_to_string(&fx.count)
            .unwrap_or_default()
            .lines()
            .filter(|line| line.starts_with("config="))
            .map(str::to_string)
            .collect()
    }

    /// Blank effort sends no `session/set_config_option`. low, medium, and
    /// high are the value on `thought_level`, not on `model_config`.
    #[test]
    fn blank_effort_is_omitted_and_each_level_uses_thought_level() {
        let (fx, session) = Fixture::new("completer-effort");
        crate::model::tests::with_env(None, None, None, || {
            crate::dev_flags::seed(&crate::settings::Settings::default());
            assert_eq!(
                session.complete(&asking("hi"), &|_| {}),
                Ok(Reply::whole("Hello"))
            );
        });
        assert_eq!(fx.count("new"), 1);
        assert_eq!(fx.count("prompt"), 1);
        assert!(config_lines(&fx).is_empty(), "{:?}", config_lines(&fx));
        session.shutdown();

        for level in ["low", "medium", "high"] {
            let (fx, session) = Fixture::new("completer-effort");
            crate::model::tests::with_env(None, None, None, || {
                crate::dev_flags::seed(&crate::settings::Settings {
                    director_reasoning_effort: level.to_string(),
                    ..crate::settings::Settings::default()
                });
                assert_eq!(
                    session.complete(&asking("hi"), &|_| {}),
                    Ok(Reply::whole("Hello"))
                );
                crate::dev_flags::seed(&crate::settings::Settings::default());
            });
            assert_eq!(config_lines(&fx), vec![format!("config=reasoning={level}")]);
            session.shutdown();
        }
    }

    /// No `thought_level` falls through to `model_config`, and not to `model`.
    #[test]
    fn effort_uses_model_config_when_thought_level_is_absent() {
        let (fx, session) = Fixture::new("completer-effort-mc");
        crate::model::tests::with_env(None, None, None, || {
            crate::dev_flags::seed(&crate::settings::Settings {
                director_reasoning_effort: "medium".to_string(),
                ..crate::settings::Settings::default()
            });
            assert_eq!(
                session.complete(&asking("hi"), &|_| {}),
                Ok(Reply::whole("Hello"))
            );
            crate::dev_flags::seed(&crate::settings::Settings::default());
        });
        assert_eq!(config_lines(&fx), vec!["config=knob=medium".to_string()]);
        session.shutdown();
    }

    #[test]
    fn a_loaded_session_is_asked_for_the_same_effort() {
        let (fx, session) = Fixture::new("load-completer-effort");
        std::fs::write(
            fx.dir.join(SESSION_FILE),
            r#"{"harness":"fake","sessions":[{"instance":"buddy-1","character":"bmo","session_id":"saved-ok"}]}"#,
        )
        .unwrap();
        crate::model::tests::with_env(None, None, None, || {
            crate::dev_flags::seed(&crate::settings::Settings {
                director_reasoning_effort: "low".to_string(),
                ..crate::settings::Settings::default()
            });
            assert_eq!(
                session.complete(&asking("hi"), &|_| {}),
                Ok(Reply::whole("Hello"))
            );
            crate::dev_flags::seed(&crate::settings::Settings::default());
        });
        assert_eq!(fx.count("load"), 1);
        assert_eq!(fx.count("new"), 0);
        assert_eq!(config_lines(&fx), vec!["config=reasoning=low".to_string()]);
        session.shutdown();
    }

    #[test]
    fn a_rejected_effort_shows_on_the_turn_and_attach_is_not_stuck() {
        let (fx, session) = Fixture::new("completer-effort-reject");
        crate::model::tests::with_env(None, None, None, || {
            crate::dev_flags::seed(&crate::settings::Settings {
                director_reasoning_effort: "high".to_string(),
                ..crate::settings::Settings::default()
            });
            let first = session.complete(&asking("hi"), &|_| {});
            assert!(
                first
                    .as_ref()
                    .is_err_and(|why| why.contains("not a level this agent takes")),
                "{first:?}"
            );
            let inspect = session.inspect();
            assert!(
                inspect
                    .turn_failure
                    .as_deref()
                    .is_some_and(|why| why.contains("not a level this agent takes")),
                "{:?}",
                inspect.turn_failure
            );
            assert_eq!(fx.count("prompt"), 0, "the rejection is not a prompt");
            assert_eq!(config_lines(&fx), vec!["config=reasoning=high".to_string()]);
            assert!(inspect.alive, "the child is still attached");
            let second = session.complete(&asking("again"), &|_| {});
            assert!(second.is_err(), "a second wake returns: {second:?}");
            assert!(session.inspect().alive);
            crate::dev_flags::seed(&crate::settings::Settings::default());
        });
        session.shutdown();
    }

    /// `some-model` is set on the model option. Blank leaves the model unset.
    #[test]
    fn a_named_model_is_sent_and_a_blank_one_is_not() {
        crate::model::tests::with_env(None, None, None, || {
            crate::dev_flags::seed(&crate::settings::Settings::default());
            let (fx, session) = Fixture::new("completer-model");
            write_completer_model(&fx.dir, "some-model");
            assert_eq!(
                session.complete(&asking("hi"), &|_| {}),
                Ok(Reply::whole("Hello"))
            );
            assert_eq!(fx.count("new"), 1);
            assert_eq!(config_lines(&fx), vec!["config=llm=some-model".to_string()]);
            session.shutdown();

            let (fx, session) = Fixture::new("completer-model");
            write_completer_model(&fx.dir, "   ");
            assert_eq!(
                session.complete(&asking("hi"), &|_| {}),
                Ok(Reply::whole("Hello"))
            );
            assert!(config_lines(&fx).is_empty(), "{:?}", config_lines(&fx));
            session.shutdown();

            let (fx, session) = Fixture::new("load-completer-model");
            std::fs::write(
            fx.dir.join(SESSION_FILE),
            r#"{"harness":"fake","sessions":[{"instance":"buddy-1","character":"bmo","session_id":"saved-ok"}]}"#,
        )
        .unwrap();
            write_completer_model(&fx.dir, "some-model");
            assert_eq!(
                session.complete(&asking("hi"), &|_| {}),
                Ok(Reply::whole("Hello"))
            );
            assert_eq!(fx.count("load"), 1);
            assert_eq!(fx.count("new"), 0);
            assert_eq!(config_lines(&fx), vec!["config=llm=some-model".to_string()]);
            session.shutdown();
        });
    }

    #[test]
    fn a_named_model_is_not_sent_when_no_model_option_is_advertised() {
        let (fx, session) = Fixture::new("completer-thought");
        write_completer_model(&fx.dir, "some-model");
        crate::model::tests::with_env(None, None, None, || {
            crate::dev_flags::seed(&crate::settings::Settings::default());
            assert_eq!(
                session.complete(&asking("hi"), &|_| {}),
                Ok(Reply::whole("Hello"))
            );
        });
        assert_eq!(fx.count("prompt"), 1);
        assert!(config_lines(&fx).is_empty(), "{:?}", config_lines(&fx));
        session.shutdown();
    }

    #[test]
    fn a_rejected_model_is_the_last_error_and_a_later_value_attaches() {
        crate::model::tests::with_env(None, None, None, || {
            crate::dev_flags::seed(&crate::settings::Settings::default());
            let (fx, session) = Fixture::new("completer-model");
            write_completer_model(&fx.dir, "nope");
            let reply = session.complete(&asking("hi"), &|_| {});
            assert!(
                reply
                    .as_ref()
                    .is_err_and(|why| why.contains("unknown model")),
                "{reply:?}"
            );
            assert_eq!(
                session.inspect().turn_failure.as_deref(),
                session.inspect().last_error.as_deref()
            );
            assert!(session.inspect().alive);
            assert_eq!(fx.count("prompt"), 0);
            assert_eq!(fx.count("close"), 1);

            write_completer_model(&fx.dir, "some-model");
            assert_eq!(
                session.complete(&asking("again"), &|_| {}),
                Ok(Reply::whole("Hello"))
            );
            assert_eq!(fx.count("config=llm=some-model"), 1);
            assert_eq!(session.inspect().last_error, None);
            assert_eq!(session.inspect().turn_failure, None);
            session.shutdown();
        });
    }

    /// Apply in `SettingsSession::apply`'s order: fold and seed, save the file a
    /// Harness reads at open, then tell the attached Harness.
    fn applied(fx: &Fixture, session: &Session, completer: crate::settings::CompleterPatch) {
        let path = crate::settings::settings_path(&fx.dir);
        let mut settings = crate::settings::Settings::load(&path);
        crate::settings::apply_with_store(
            &mut settings,
            &crate::secrets::MemoryStore::new(),
            crate::settings::SettingsPatch {
                completer,
                ..crate::settings::SettingsPatch::default()
            },
        )
        .expect("apply");
        settings.save(&path).expect("settings");
        session.inputs_applied();
    }

    fn model_patch(model: &str) -> crate::settings::CompleterPatch {
        crate::settings::CompleterPatch {
            director_model: Some(model.to_string()),
            ..crate::settings::CompleterPatch::default()
        }
    }

    fn effort_patch(effort: &str) -> crate::settings::CompleterPatch {
        crate::settings::CompleterPatch {
            director_reasoning_effort: Some(effort.to_string()),
            ..crate::settings::CompleterPatch::default()
        }
    }

    fn prompt_sessions(fx: &Fixture) -> Vec<Value> {
        fx.events("prompt")
            .into_iter()
            .map(|prompt| prompt["session_id"].clone())
            .collect()
    }

    /// Three wakes on one Instance with an Apply before the second and the
    /// third. The prompts' session ids, in order.
    fn sessions_across_applies(
        fx: &Fixture,
        session: &Session,
        same: crate::settings::CompleterPatch,
        changed: crate::settings::CompleterPatch,
    ) -> Vec<Value> {
        for (apply, prompt) in [(None, "hi"), (Some(same), "same"), (Some(changed), "changed")] {
            if let Some(patch) = apply {
                applied(fx, session, patch);
            }
            assert_eq!(
                session.complete(&asking(prompt), &|_| {}),
                Ok(Reply::whole("Hello"))
            );
        }
        prompt_sessions(fx)
    }

    /// #1430, #1434: the open conversation takes an applied model through
    /// `session/set_config_option` and keeps its id. The same model sets nothing.
    #[test]
    fn an_applied_model_is_set_on_the_open_conversation() {
        crate::model::tests::with_env(None, None, None, || {
            crate::dev_flags::seed(&crate::settings::Settings::default());
            let (fx, session) = Fixture::new("load-completer-model");
            write_completer_model(&fx.dir, "default-model");
            let ids = sessions_across_applies(
                &fx,
                &session,
                model_patch("default-model"),
                model_patch("some-model"),
            );
            session.shutdown();

            assert_eq!(
                config_lines(&fx),
                vec![
                    "config=llm=default-model".to_string(),
                    "config=llm=some-model".to_string(),
                ]
            );
            assert_eq!(ids, vec![ids[0].clone(); 3], "the conversation changed");
            assert_eq!((fx.count("new"), fx.count("load")), (1, 0));
        });
    }

    #[test]
    fn an_applied_effort_is_set_on_the_open_conversation() {
        crate::model::tests::with_env(None, None, None, || {
            crate::dev_flags::seed(&crate::settings::Settings::default());
            let (fx, session) = Fixture::new("load-completer-effort");
            applied(&fx, &session, effort_patch("low"));
            let ids =
                sessions_across_applies(&fx, &session, effort_patch("low"), effort_patch("high"));
            session.shutdown();
            crate::dev_flags::seed(&crate::settings::Settings::default());

            assert_eq!(
                config_lines(&fx),
                vec![
                    "config=reasoning=low".to_string(),
                    "config=reasoning=high".to_string(),
                ]
            );
            assert_eq!(ids, vec![ids[0].clone(); 3], "the conversation changed");
            assert_eq!((fx.count("new"), fx.count("load")), (1, 0));
        });
    }

    /// `session/set_config_option` sets a value and cannot unset one, so a
    /// blanked effort opens a new conversation that never had it.
    #[test]
    fn a_blanked_effort_opens_a_new_conversation_without_it() {
        crate::model::tests::with_env(None, None, None, || {
            crate::dev_flags::seed(&crate::settings::Settings::default());
            let (fx, session) = Fixture::new("load-completer-effort");
            applied(&fx, &session, effort_patch("high"));
            let ids =
                sessions_across_applies(&fx, &session, effort_patch("high"), effort_patch(""));
            session.shutdown();
            crate::dev_flags::seed(&crate::settings::Settings::default());

            assert_eq!(config_lines(&fx), vec!["config=reasoning=high".to_string()]);
            assert_eq!(ids[0], ids[1], "an unchanged effort reopened the conversation");
            assert_ne!(ids[1], ids[2], "the blanked effort stayed on the old conversation");
            assert_eq!((fx.count("new"), fx.count("load")), (2, 0));
        });
    }

    /// Apply of a new effort cancels the turn still running on the old one, so
    /// its unanswered ask does not hold the next wake, which goes on in the
    /// same conversation at the new effort. An unchanged Apply does nothing.
    #[test]
    fn an_applied_effort_cancels_the_running_turn_and_the_next_wake_keeps_the_conversation() {
        crate::model::tests::with_env(None, None, None, || {
            crate::dev_flags::seed(&crate::settings::Settings::default());
            let (fx, session) = Fixture::new("permission");
            let session = Arc::new(session);
            applied(&fx, &session, effort_patch("low"));
            let first = {
                let session = Arc::clone(&session);
                thread::spawn(move || session.complete(&asking("hi"), &|_| {}))
            };
            let ask = fx.ask();

            applied(&fx, &session, effort_patch("low"));
            thread::sleep(Duration::from_millis(300));
            assert_eq!(fx.count("cancel"), 0, "an unchanged Apply cancelled the turn");
            assert!(!first.is_finished(), "an unchanged Apply ended the turn");

            applied(&fx, &session, effort_patch("high"));
            assert_eq!(fx.settled(), (ask.request, None));
            let displaced = first.join().unwrap().unwrap_err();
            assert!(displaced.contains("cancelled"), "{displaced}");

            let next = {
                let session = Arc::clone(&session);
                thread::spawn(move || session.complete(&proactive("tick"), &|_| {}))
            };
            let again = fx.ask();
            session.answer_permission(&again.request, "allow");
            assert_eq!(next.join().unwrap(), Ok(Reply::whole("ok:allow")));
            session.shutdown();
            crate::dev_flags::seed(&crate::settings::Settings::default());

            assert_eq!(
                config_lines(&fx),
                vec![
                    "config=reasoning=low".to_string(),
                    "config=reasoning=high".to_string(),
                ]
            );
            let ids = prompt_sessions(&fx);
            assert_eq!(ids.len(), 2, "{ids:?}");
            assert_eq!(ids[0], ids[1], "the next wake left the conversation");
            assert_eq!(fx.count("new"), 1);
        });
    }

    /// Model is applied before effort, against the options the model call returns.
    #[test]
    fn model_is_applied_before_effort_on_the_refreshed_option() {
        let (fx, session) = Fixture::new("completer-both");
        write_completer_model(&fx.dir, "some-model");
        crate::model::tests::with_env(None, None, None, || {
            crate::dev_flags::seed(&crate::settings::Settings {
                director_reasoning_effort: "high".to_string(),
                ..crate::settings::Settings::default()
            });
            assert_eq!(
                session.complete(&asking("hi"), &|_| {}),
                Ok(Reply::whole("Hello"))
            );
            crate::dev_flags::seed(&crate::settings::Settings::default());
        });
        assert_eq!(
            config_lines(&fx),
            vec![
                "config=llm=some-model".to_string(),
                "config=after-model=high".to_string(),
            ]
        );
        session.shutdown();
    }

    #[test]
    fn garbage_between_messages_is_skipped() {
        let (_fx, session) = Fixture::new("garbage");
        assert_eq!(
            session.complete(&asking("hi"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        session.shutdown();
    }

    #[test]
    fn initialize_payload_advertises_form_and_url_elicitation() {
        let (fx, session) = Fixture::new("hello");
        assert_eq!(
            session.complete(&asking("hi"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        let params = fx.initialize_params();
        assert_eq!(
            params["clientCapabilities"]["elicitation"],
            json!({"form": {}, "url": {}})
        );
        session.shutdown();
    }

    #[test]
    fn elicitation_round_trip_records_the_chosen_option() {
        let (fx, session) = Fixture::new("elicitation");
        let session = Arc::new(session);
        let worker = {
            let session = Arc::clone(&session);
            thread::spawn(move || session.complete(&asking("hi"), &|_| {}))
        };
        let form = fx.form();
        assert_eq!(form.message, "How should I approach this refactoring?");
        assert_eq!(form.field, "strategy");
        assert_eq!(form.options.len(), 3);
        session.answer_elicitation(&form.request, ElicitationAnswer::Accept("balanced".into()));
        assert_eq!(worker.join().unwrap(), Ok(Reply::whole("ok:balanced")));
        assert!(fx.wait_for("elicit:accept", 1));
        assert!(fx.wait_for("elicit-value:balanced", 1));
        assert_eq!(fx.settled(), (form.request, Some("balanced".to_string())));
        session.shutdown();
    }

    #[test]
    fn elicitation_round_trip_records_a_decline() {
        let (fx, session) = Fixture::new("elicitation");
        let session = Arc::new(session);
        let worker = {
            let session = Arc::clone(&session);
            thread::spawn(move || session.complete(&asking("hi"), &|_| {}))
        };
        let form = fx.form();
        session.answer_elicitation(&form.request, ElicitationAnswer::Decline);
        assert_eq!(worker.join().unwrap(), Ok(Reply::whole("ok:declined")));
        assert!(fx.wait_for("elicit:decline", 1));
        assert_eq!(fx.settled(), (form.request, Some("decline".to_string())));
        session.shutdown();
    }

    #[test]
    fn claude_sessions_start_with_fidget_mcp_approved() {
        let (calls, _rx) = mpsc::channel();
        assert!(crate::mcp_http::serve(calls).is_some());

        let (fx, mut session) = Fixture::new("hello");
        session.launch.name = "claude".into();
        assert_eq!(
            session.complete(&asking("hi"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        assert_eq!(fx.count("fidget-approval"), 1);
        session.shutdown();

        let (fx, mut session) = Fixture::new("load-replay");
        session.launch.name = "claude".into();
        std::fs::write(
            fx.dir.join(SESSION_FILE),
            r#"{"harness":"claude","sessions":[{"instance":"buddy-1","character":"bmo","session_id":"saved-ok"}]}"#,
        )
        .unwrap();
        assert_eq!(
            session.complete(&asking("hi"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        assert_eq!(fx.count("load"), 1);
        assert_eq!(fx.count("fidget-approval"), 1);
        session.shutdown();

        let (fx, session) = Fixture::new("hello");
        assert_eq!(
            session.complete(&asking("hi"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        assert_eq!(fx.count("fidget-approval"), 0);
        session.shutdown();

        let (fx, mut session) = Fixture::new("permission");
        session.launch.name = "claude".into();
        let session = Arc::new(session);
        let worker = {
            let session = Arc::clone(&session);
            thread::spawn(move || session.complete(&asking("run shell"), &|_| {}))
        };
        let ask = fx.ask();
        assert_eq!(ask.title.as_deref(), Some("rm -rf /"));
        session.answer_permission(&ask.request, "reject");
        assert_eq!(worker.join().unwrap(), Ok(Reply::whole("ok:reject")));
        assert_eq!(fx.count("fidget-approval"), 1);
        session.shutdown();
    }

    #[test]
    fn permission_request_reaches_the_hook_and_the_answer_completes_the_turn() {
        let (fx, session) = Fixture::new("permission");
        let session = Arc::new(session);
        let worker = {
            let session = Arc::clone(&session);
            thread::spawn(move || session.complete(&asking("hi"), &|_| {}))
        };
        let ask = fx.ask();
        assert_eq!(ask.title.as_deref(), Some("rm -rf /"));
        assert_eq!(ask.kind.as_deref(), Some("execute"));
        // What the tool call says about itself has to survive the trip, or
        // the surface is left asking the user to approve a kind.
        assert_eq!(ask.content, ["Delete everything?"]);
        assert_eq!(ask.input, Some(json!({"command": "rm -rf /"})));
        assert_eq!(ask.locations, ["/"]);
        assert_eq!(ask.options.len(), 2);
        session.answer_permission(&ask.request, "allow");
        assert_eq!(worker.join().unwrap(), Ok(Reply::whole("ok:allow")));
        assert!(fx.wait_for("perm:selected", 1));
        // Every other open window has to retire the row this one answered,
        // and draw the option that actually won rather than its own click.
        assert_eq!(fx.settled(), (ask.request, Some("allow".to_string())));
        session.shutdown();
    }

    /// A Harness that works on after its turn ends, as a cron fire does. Its
    /// tool call reaches the Action Log, its thought reaches the Instance's
    /// Thinking row, its agent text reaches Chat/bubble, and its ask reaches
    /// Chat and is answered.
    #[test]
    fn work_between_turns_reaches_the_user_and_its_ask_is_answered() {
        let (fx, session) = Fixture::new("between-turn");
        assert_eq!(
            session.complete(&asking("hi"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        let mut thoughts = Vec::new();
        let mut said = Vec::new();
        let ask = loop {
            match fx.forwarded.recv_timeout(Duration::from_secs(5)) {
                Ok(Forwarded::Ask { ask, .. }) => break ask,
                Ok(Forwarded::Thought { instance, line }) => thoughts.push((instance, line)),
                Ok(Forwarded::InboundWake(wake)) => said.push((wake.instance, wake.speech)),
                Ok(_) => {}
                Err(_) => panic!("the between-turn ask never reached Chat"),
            }
        };
        assert_eq!(ask.title.as_deref(), Some("Remind the user"));
        assert!(
            thoughts.contains(&("buddy-1".to_string(), "between turns".to_string())),
            "{thoughts:?}"
        );
        assert!(
            said.contains(&(
                "buddy-1".to_string(),
                "Your reminder: time to stretch!".to_string()
            )),
            "between-turn agent text never reached the user: {said:?}"
        );
        let titles: Vec<Value> = fx
            .events("tool_call")
            .iter()
            .map(|line| line["title"].clone())
            .collect();
        assert_eq!(titles, [json!("cron fired")]);
        session.answer_permission(&ask.request, "allow");
        assert!(
            fx.wait_for("perm:selected", 1),
            "the answer never reached the Harness"
        );
        assert_eq!(fx.settled(), (ask.request, Some("allow".to_string())));
        session.shutdown();
    }

    /// An ask held from between turns is answered while Fidget's next turn
    /// runs, so the answer has to reach the Harness from inside that turn.
    #[test]
    fn a_held_ask_is_answered_during_a_later_turn() {
        let (fx, session) = Fixture::new("between-turn-held");
        let session = Arc::new(session.with_timeout(Duration::from_secs(2)));
        assert_eq!(
            session.complete(&asking("hi"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        let ask = fx.ask();
        let worker = {
            let session = Arc::clone(&session);
            thread::spawn(move || session.complete(&asking("again"), &|_| {}))
        };
        assert!(fx.wait_for("prompt", 2), "the second turn never went out");
        session.answer_permission(&ask.request, "allow");
        assert_eq!(worker.join().unwrap(), Ok(Reply::whole("ok:allow")));
        let settled = fx
            .forwarded
            .try_iter()
            .find_map(|forwarded| match forwarded {
                Forwarded::Settled { request, option } => Some((request, option)),
                _ => None,
            });
        assert_eq!(settled, Some((ask.request, Some("allow".to_string()))));
        session.shutdown();
    }

    /// A closed session's thought from between turns has ended, so its
    /// Thinking row closes with it. Speech accumulated between turns is
    /// emitted as an inbound wake before the close.
    #[test]
    fn closing_a_session_ends_its_thought_from_between_turns() {
        let (fx, session) = Fixture::new("between-turn");
        assert_eq!(
            session.complete(&asking("hi"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        thread::sleep(Duration::from_millis(400));
        let wire = session.current_wire().expect("attached");
        assert_eq!(wire.close("fresh-id"), Ok(()));
        let mut speech_seen = false;
        let mut thought_ended = false;
        for forwarded in fx.forwarded.try_iter() {
            match forwarded {
                Forwarded::InboundWake(wake) if wake.instance == "buddy-1" => {
                    assert_eq!(wake.speech, "Your reminder: time to stretch!");
                    speech_seen = true;
                }
                Forwarded::Thought { instance, line }
                    if instance == "buddy-1" && line.is_empty() =>
                {
                    thought_ended = true;
                }
                _ => {}
            }
        }
        assert!(speech_seen, "between-turn speech was dropped on close");
        assert!(thought_ended, "the Thinking row stayed open after close");
        session.shutdown();
    }

    /// Prompting again before between-turn ask arrives still emits the
    /// accumulated speech as an inbound wake, not drops it.
    #[test]
    fn prompting_before_between_turn_ask_flushes_speech() {
        let (fx, session) = Fixture::new("between-turn");
        assert_eq!(
            session.complete(&asking("hi"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        thread::sleep(Duration::from_millis(400));
        let worker = thread::spawn(move || session.complete(&asking("again"), &|_| {}));
        let mut speech_seen = false;
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !speech_seen && std::time::Instant::now() < deadline {
            match fx.forwarded.try_recv() {
                Ok(Forwarded::InboundWake(wake)) if wake.instance == "buddy-1" => {
                    assert_eq!(wake.speech, "Your reminder: time to stretch!");
                    speech_seen = true;
                }
                _ => thread::sleep(Duration::from_millis(10)),
            }
        }
        assert!(
            speech_seen,
            "between-turn speech was dropped when prompting again"
        );
        drop(worker);
    }

    /// Instance B's session is open and has never had a turn. While Instance
    /// A's turn runs, B's session speaks and asks. Returns the ask once it
    /// reached Chat, every inbound wake forwarded before it, and A's turn.
    fn crosstalk() -> Crosstalk {
        let (fx, session) = Fixture::new("crosstalk");
        let session = Arc::new(session);
        let b = SessionKey {
            instance: "buddy-b".to_string(),
            character: "bmo".to_string(),
            blank: false,
        };
        let (_, opened) = session.attach(Some(&b)).expect("B's session opens");
        assert_eq!(opened, "fresh-id");
        let worker = {
            let session = Arc::clone(&session);
            thread::spawn(move || session.complete(&asking_as("buddy-a", "bmo", "hi"), &|_| {}))
        };
        let mut wakes = Vec::new();
        let ask = loop {
            match fx.forwarded.recv_timeout(Duration::from_secs(5)) {
                Ok(Forwarded::Ask { ask, .. }) => break ask,
                Ok(Forwarded::InboundWake(wake)) => wakes.push((wake.instance, wake.speech)),
                Ok(_) => {}
                Err(_) => panic!("B's ask never reached Chat"),
            }
        };
        Crosstalk {
            fx,
            session,
            ask,
            wakes,
            worker,
        }
    }

    struct Crosstalk {
        fx: Fixture,
        session: Arc<Session>,
        ask: PermissionAsk,
        /// Instance and speech of each inbound wake before the ask.
        wakes: Vec<(String, String)>,
        worker: thread::JoinHandle<Result<Reply, String>>,
    }

    /// An ask on B's session while A is mid-turn is owed by B. A's wake is
    /// not held for it, and B's is until the user answers (#1395).
    #[test]
    fn an_ask_on_another_session_mid_turn_is_owed_by_that_sessions_instance() {
        let Crosstalk {
            fx,
            session,
            ask,
            worker,
            wakes: _,
        } = crosstalk();
        assert_eq!(ask.title.as_deref(), Some("Remind the other user"));
        assert!(session.awaiting_user("buddy-b"), "B's ask is not B's");
        assert!(
            !session.awaiting_user("buddy-a"),
            "B's ask was counted against A's turn"
        );
        // A's turn already ended with B's ask still open.
        assert_eq!(worker.join().unwrap(), Ok(Reply::whole("Hello")));
        assert!(
            session.awaiting_user("buddy-b"),
            "ending A's turn cancelled B"
        );
        session.answer_permission(&ask.request, "allow");
        assert!(
            fx.wait_for("perm:selected", 1),
            "B's later answer was cancelled"
        );
        assert!(!session.awaiting_user("buddy-b"), "the answer left B owing");
        session.shutdown();
    }

    /// B's speech during A's turn is not A's reply. It reaches B's Chat as
    /// work between turns (#1395).
    #[test]
    fn another_sessions_update_mid_turn_is_not_folded_into_the_turn() {
        // Bound so the Fixture's directory outlives the turn.
        let Crosstalk {
            fx: _fx,
            session,
            ask,
            wakes,
            worker,
        } = crosstalk();
        assert_eq!(
            wakes,
            [("buddy-b".to_string(), "Not yours".to_string())],
            "B's speech did not reach B"
        );
        assert_eq!(worker.join().unwrap(), Ok(Reply::whole("Hello")));
        session.answer_permission(&ask.request, "allow");
        session.shutdown();
    }

    /// Two Chats are open, A's and B's. While A's turn runs, B's session
    /// plans, asks and sends a form. A's Chat draws none of those rows, and
    /// B's Chat draws none of A's plan (#1422).
    #[test]
    fn a_second_chat_shows_none_of_the_first_chats_ask_form_or_plan_rows() {
        let (fx, session) = Fixture::new("crosstalk-rows");
        let b = SessionKey {
            instance: "buddy-b".to_string(),
            character: "bmo".to_string(),
            blank: false,
        };
        let (_, opened) = session.attach(Some(&b)).expect("B's session opens");
        assert_eq!(opened, "fresh-id");
        assert_eq!(
            session.complete(&asking_as("buddy-a", "bmo", "hi"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        let rows: Vec<_> = fx
            .forwarded
            .try_iter()
            .filter_map(|row| chat_rows(&row))
            .collect();
        assert_eq!(
            rows,
            [
                ("plan: B's step".to_string(), vec!["chat-buddy-b"]),
                (
                    "ask: Remind the other user".to_string(),
                    vec!["chat-buddy-b"]
                ),
                (
                    "form: Authenticate with MCP server linear".to_string(),
                    vec!["chat-buddy-b"]
                ),
                ("plan: A's step".to_string(), vec!["chat-buddy-a"]),
                ("plan: ".to_string(), vec!["chat-buddy-a"]),
            ]
        );
        session.shutdown();
    }

    /// Each ask, form and plan row, and the Chats out of A's and B's that
    /// `main.rs` draws it in.
    fn chat_rows(row: &Forwarded) -> Option<(String, Vec<&'static str>)> {
        let (row, owner) = match row {
            Forwarded::Ask { owner, ask } => (
                format!("ask: {}", ask.title.as_deref().unwrap_or("")),
                owner.clone(),
            ),
            Forwarded::Form { owner, form } => (format!("form: {}", form.message), owner.clone()),
            Forwarded::Plan { instance, steps } => (
                format!(
                    "plan: {}",
                    steps.first().map_or("", |step| step.content.as_str())
                ),
                Owner::Instance(instance.clone()),
            ),
            _ => return None,
        };
        let chats = ["chat-buddy-a", "chat-buddy-b"]
            .into_iter()
            .filter(|label| crate::draws_in(&owner, label))
            .collect();
        Some((row, chats))
    }

    /// An ask queued on the new session before `session/new` answers is owed
    /// by that session's Instance once `attach` returns.
    #[test]
    fn an_ask_during_open_is_owed_once_attach_returns() {
        let (fx, session) = Fixture::new("ask-on-open");
        let b = SessionKey {
            instance: "buddy-b".to_string(),
            character: "bmo".to_string(),
            blank: false,
        };
        let (_, opened) = session.attach(Some(&b)).expect("B's session opens");
        assert_eq!(opened, "fresh-id");
        let ask = fx.ask();
        assert_eq!(ask.title.as_deref(), Some("Remind on open"));
        assert!(
            session.awaiting_user("buddy-b"),
            "ask during open was not B's once attach returned"
        );
        assert!(
            !session.awaiting_user("buddy-a"),
            "ask during open was counted against a turn nobody held"
        );
        session.answer_permission(&ask.request, "allow");
        assert!(fx.wait_for("perm:selected", 1));
        assert!(!session.awaiting_user("buddy-b"));
        session.shutdown();
    }

    /// `session/load` replays the conversation as updates before it answers,
    /// and a load that fails after replaying falls back to `session/new`.
    /// Both are history, not a Harness working between turns.
    #[test]
    fn a_loaded_sessions_replay_is_not_work_between_turns() {
        for saved in ["saved-ok", "stale"] {
            let (fx, session) = Fixture::new("load-replay");
            std::fs::write(
                fx.dir.join(SESSION_FILE),
                format!(
                    r#"{{"harness":"fake","sessions":[{{"instance":"buddy-1","character":"bmo","session_id":"{saved}"}}]}}"#
                ),
            )
            .unwrap();
            assert_eq!(
                session.complete(&asking("hi"), &|_| {}),
                Ok(Reply::whole("Hello"))
            );
            assert_eq!(fx.count("load"), 1);
            assert_eq!(fx.events("tool_call"), Vec::<Value>::new(), "{saved}");
            session.shutdown();
        }
    }

    /// The conversation a loaded session replays reaches that Instance's Chat
    /// whole and in order, once: prompts, thinking, replies, tool calls, and
    /// the plan. The respawn loads the same id and replays it again, and Chat
    /// already holds it. Neither replay is a wake, a live plan, or live
    /// thinking (#1393).
    #[test]
    fn a_loaded_sessions_replay_is_restored_once_and_wakes_nobody() {
        let (fx, session) = Fixture::new("load-history");
        let session = session.with_backoff(Duration::ZERO);
        std::fs::write(
            fx.dir.join(SESSION_FILE),
            r#"{"harness":"fake","sessions":[{"instance":"buddy-1","character":"bmo","session_id":"saved-ok"}]}"#,
        )
        .unwrap();
        assert_eq!(
            session.complete(&asking("hi"), &|_| {}),
            Err("harness exited".to_string())
        );
        assert_eq!(
            session.complete(&asking("again"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        assert_eq!(fx.count("load"), 2);
        let mut restored = Vec::new();
        for forwarded in fx.forwarded.try_iter() {
            match forwarded {
                Forwarded::Restored(Restored { instance, history }) => {
                    restored.push((instance, history))
                }
                Forwarded::InboundWake(wake) => {
                    panic!("replay woke {}: {}", wake.instance, wake.speech)
                }
                Forwarded::Plan { steps, .. } if !steps.is_empty() => {
                    panic!("replay drew a live plan: {steps:?}")
                }
                Forwarded::Thought { line, .. } if !line.is_empty() => {
                    panic!("replay drew live thinking: {line}")
                }
                _ => {}
            }
        }
        let text = |text: &str| text.to_string();
        assert_eq!(
            restored,
            vec![(
                text("buddy-1"),
                vec![
                    Replayed::Prompt {
                        text: text("the first wake's prompt")
                    },
                    Replayed::Thought {
                        text: text("Weighing it")
                    },
                    Replayed::Reply {
                        text: text("wave\nHello from before")
                    },
                    Replayed::ToolCall {
                        id: text("replayed"),
                        title: text("replayed"),
                        kind: Some(text("other")),
                        status: Some(text("failed")),
                        content: vec![text("no such file")],
                    },
                    Replayed::Plan {
                        steps: vec![PlanStep {
                            content: text("Read the roster"),
                            priority: text("medium"),
                            status: text("completed"),
                        }]
                    },
                    Replayed::Prompt {
                        text: text("the second wake's prompt")
                    },
                    Replayed::Thought { text: text("hmm") },
                    Replayed::Reply {
                        text: text("nod | Still here")
                    },
                    Replayed::Reply {
                        text: text("Done.")
                    },
                    Replayed::Reply {
                        text: text("Stretch break!")
                    },
                ]
            )]
        );
        assert_eq!(fx.events("tool_call"), Vec::<Value>::new());
        session.shutdown();
    }

    /// A load that fails after replaying falls back to `session/new`. The new
    /// session never said what that replay holds, so Chat is not handed it.
    #[test]
    fn a_failed_loads_replay_is_not_restored() {
        let (fx, session) = Fixture::new("load-history");
        let session = session.with_backoff(Duration::ZERO);
        std::fs::write(
            fx.dir.join(SESSION_FILE),
            r#"{"harness":"fake","sessions":[{"instance":"buddy-1","character":"bmo","session_id":"stale"}]}"#,
        )
        .unwrap();
        let _ = session.complete(&asking("hi"), &|_| {});
        assert_eq!(fx.count("load"), 1);
        assert_eq!(fx.count("new"), 1);
        let restored = fx
            .forwarded
            .try_iter()
            .filter(|forwarded| matches!(forwarded, Forwarded::Restored(_)))
            .count();
        assert_eq!(restored, 0);
        session.shutdown();
    }

    /// The budget guards a Harness that went quiet, not a user who is busy
    /// (#1001). Three budgets pass before the click and the turn still ends
    /// on the answer, the ask retired by that answer and not by a cancel.
    #[test]
    fn a_slow_answer_to_an_ask_is_not_a_timeout() {
        let (fx, session) = Fixture::new("permission");
        let session = Arc::new(session.with_timeout(Duration::from_millis(300)));
        let worker = {
            let session = Arc::clone(&session);
            thread::spawn(move || session.complete(&asking("hi"), &|_| {}))
        };
        let ask = fx.ask();
        thread::sleep(Duration::from_millis(900));
        session.answer_permission(&ask.request, "allow");
        assert_eq!(worker.join().unwrap(), Ok(Reply::whole("ok:allow")));
        assert_eq!(fx.count("cancel"), 0);
        assert_eq!(fx.settled(), (ask.request, Some("allow".to_string())));
        session.shutdown();
    }

    #[test]
    fn a_slow_answer_to_a_form_is_not_a_timeout() {
        let (fx, session) = Fixture::new("elicitation");
        let session = Arc::new(session.with_timeout(Duration::from_millis(300)));
        let worker = {
            let session = Arc::clone(&session);
            thread::spawn(move || session.complete(&asking("hi"), &|_| {}))
        };
        let form = fx.form();
        thread::sleep(Duration::from_millis(900));
        session.answer_elicitation(&form.request, ElicitationAnswer::Accept("balanced".into()));
        assert_eq!(worker.join().unwrap(), Ok(Reply::whole("ok:balanced")));
        assert_eq!(fx.count("cancel"), 0);
        assert_eq!(fx.settled(), (form.request, Some("balanced".to_string())));
        session.shutdown();
    }

    /// The answer starts the budget over. The Harness works for most of one
    /// budget before it asks and for as long again after the answer. A fresh
    /// budget covers that; a paused clock resumed would run out.
    #[test]
    fn an_answer_starts_a_fresh_turn_budget() {
        let (fx, session) = Fixture::new("permission-after-work");
        let session = Arc::new(session.with_timeout(ASK_WORK * 5 / 3));
        let worker = {
            let session = Arc::clone(&session);
            thread::spawn(move || session.complete(&asking("hi"), &|_| {}))
        };
        let ask = fx.ask();
        session.answer_permission(&ask.request, "allow");
        assert_eq!(worker.join().unwrap(), Ok(Reply::whole("ok:allow")));
        assert_eq!(fx.count("cancel"), 0);
        assert_eq!(fx.settled(), (ask.request, Some("allow".to_string())));
        session.shutdown();
    }

    /// The guard is still there once the user has answered. A Harness that
    /// takes the answer and then goes quiet is cancelled on the fresh budget.
    #[test]
    fn a_stall_after_the_answer_still_times_out() {
        let (fx, session) = Fixture::new("permission-stall");
        let session = Arc::new(session.with_timeout(Duration::from_millis(500)));
        let worker = {
            let session = Arc::clone(&session);
            thread::spawn(move || session.complete(&asking("hi"), &|_| {}))
        };
        let ask = fx.ask();
        session.answer_permission(&ask.request, "allow");
        assert_eq!(fx.settled(), (ask.request, Some("allow".to_string())));
        let reply = worker.join().unwrap();
        assert!(
            reply.as_ref().is_err_and(|why| why.contains("cancelled")),
            "{reply:?}"
        );
        assert!(fx.wait_for("cancel", 1));
        assert_eq!(
            fx.count("perm:cancelled"),
            0,
            "the answer was taken, not cancelled"
        );
        session.shutdown();
    }

    /// A row nobody answered is retired all the same when the turn is taken
    /// from under it, with no option, because nothing was chosen. Or its
    /// buttons outlive the turn. Reached through a newer wake now that a
    /// wait on the user is not a timeout (#1001).
    #[test]
    fn a_newer_wake_retires_the_ask_nobody_answered() {
        let (fx, session) = Fixture::new("permission");
        let session = Arc::new(session);
        let first = {
            let session = Arc::clone(&session);
            thread::spawn(move || session.complete(&asking("hi"), &|_| {}))
        };
        let ask = fx.ask();
        let second = {
            let session = Arc::clone(&session);
            thread::spawn(move || session.complete(&asking("again"), &|_| {}))
        };
        assert_eq!(fx.settled(), (ask.request, None));
        assert!(fx.wait_for("perm:cancelled", 1));
        let displaced = first.join().unwrap().unwrap_err();
        assert!(displaced.contains("cancelled"), "{displaced}");
        let again = fx.ask();
        session.answer_permission(&again.request, "allow");
        assert_eq!(second.join().unwrap(), Ok(Reply::whole("ok:allow")));
        assert_eq!(fx.settled(), (again.request, Some("allow".to_string())));
        session.shutdown();
    }

    /// The Instance every slot test wakes. One character is enough: the rule
    /// under test is per-Instance.
    const WOKEN: &str = "buddy-1";

    /// A session Director over this Harness, as the frame loop builds one.
    fn harness_director(
        session: &Arc<Session>,
    ) -> Arc<fidget_core::director::ModelDirector<crate::completer::AnyCompleter>> {
        Arc::new(fidget_core::director::ModelDirector::new(
            crate::completer::AnyCompleter::Harness(Arc::clone(session)),
            ["stroll", "nap"],
            WOKEN,
            "bmo",
            false,
        ))
    }

    fn woken(happened: Happened) -> Context {
        Context {
            happened,
            ..crate::completer::tests::wake_context()
        }
    }

    /// Poll the slot the way the frame loop does, until an answer lands.
    fn polled(slots: &mut crate::completer::Slots) -> Option<crate::completer::Answered> {
        let id = WOKEN.to_string();
        for _ in 0..300 {
            if let Some(crate::completer::Arrived::Answered(answered)) = slots.take(&id) {
                return Some(*answered);
            }
            thread::sleep(Duration::from_millis(20));
        }
        None
    }

    /// The line the Shell would remember, or `None` for a wake that failed.
    fn said(answered: &crate::completer::Answered) -> Option<&str> {
        match &answered.wake {
            Wake::Proposed(parsed) => parsed.dialogue.as_deref(),
            Wake::Failed => None,
        }
    }

    /// #1037. `Session::supersede` refuses to give a reactive turn up for an
    /// ambient tick, so the Harness answers the chat turn. The slot must not
    /// have thrown its claim on that answer away in the meantime, or the
    /// reply the user is waiting on reaches nobody.
    #[test]
    fn an_ambient_tick_does_not_take_a_chat_turn_the_harness_keeps() {
        let (fx, session) = Fixture::new("working");
        let session = Arc::new(session);
        let id = WOKEN.to_string();
        let mut slots = crate::completer::Slots::new();
        slots.wake(
            &id,
            harness_director(&session),
            woken(Happened::Chat("hi".into())),
        );
        assert!(fx.wait_for("prompt", 1));
        slots.wake(&id, harness_director(&session), woken(Happened::Proactive));

        let answered = polled(&mut slots).expect("the chat turn's answer");
        assert!(
            matches!(answered.context.happened, Happened::Chat(_)),
            "the answer belongs to the typed line, not {:?}",
            answered.context.happened
        );
        assert_eq!(said(&answered), Some("Hello"));
        assert_eq!(fx.count("cancel"), 0);
        assert_eq!(fx.count("prompt"), 1, "the ambient tick was dropped");
        session.shutdown();
    }

    /// #1038, ADR-0016. The character has a question out to the user, and every
    /// wake is dropped until the user answers it. Newest-wins is about the world
    /// moving past a moment; the user mid-answer is not that.
    #[test]
    fn no_wake_takes_a_turn_blocked_on_the_users_answer() {
        use Happened::*;
        let every = [
            Poke,
            Throw,
            Summon,
            Grab,
            Perch,
            Chat("and another thing".into()),
            Proactive,
        ];
        // No wildcard, so a new variant does not compile until it is listed above.
        for happened in &every {
            match happened {
                Poke | Throw | Summon | Grab | Perch | Chat(_) | Proactive => {}
            }
        }
        for happened in every {
            let (fx, session) = Fixture::new("permission");
            let session = Arc::new(session);
            let id = WOKEN.to_string();
            let mut slots = crate::completer::Slots::new();
            slots.wake(&id, harness_director(&session), woken(Chat("hi".into())));
            let ask = fx.ask();
            assert_eq!(
                slots.wake(&id, harness_director(&session), woken(happened.clone())),
                crate::completer::Woke::AwaitingUser,
                "{happened:?} took the turn the user is answering"
            );
            thread::sleep(Duration::from_millis(300));
            assert_eq!(
                fx.count("perm:cancelled"),
                0,
                "{happened:?} took the ask from under the user"
            );

            session.answer_permission(&ask.request, "allow");
            let answered = polled(&mut slots).expect("the chat turn's answer");
            assert_eq!(
                answered.context.happened,
                Chat("hi".into()),
                "{happened:?} replaced the typed line"
            );
            assert_eq!(said(&answered), Some("ok:allow"));
            assert_eq!(fx.count("cancel"), 0, "{happened:?} cancelled the turn");
            assert_eq!(fx.count("prompt"), 1, "{happened:?} was sent, not dropped");
            session.shutdown();
        }
    }

    /// A Poke is a touch of the sprite, so a turn that is only thinking still
    /// gives way to it. Opening Chat does not. ADR-0016 stands for the touch.
    #[test]
    fn a_poke_still_takes_a_chat_turn_that_is_only_thinking() {
        let (fx, session) = Fixture::new("slow");
        let session = Arc::new(session);
        let id = WOKEN.to_string();
        let mut slots = crate::completer::Slots::new();
        slots.wake(
            &id,
            harness_director(&session),
            woken(Happened::Chat("hi".into())),
        );
        assert!(fx.wait_for("prompt", 1));
        slots.wake(&id, harness_director(&session), woken(Happened::Poke));

        let answered = polled(&mut slots).expect("the Poke's own answer");
        assert_eq!(answered.context.happened, Happened::Poke);
        assert_eq!(said(&answered), Some("Hello"));
        assert!(fx.wait_for("cancel", 1));
        session.shutdown();
    }

    /// Opening Chat is not a touch of the sprite. A reply already generating
    /// is the one the user is about to read, so the Summon is dropped and
    /// the turn on the wire is not cancelled.
    #[test]
    fn a_summon_does_not_take_a_chat_turn_that_is_only_thinking() {
        let (fx, session) = Fixture::new("working");
        let session = Arc::new(session);
        let id = WOKEN.to_string();
        let mut slots = crate::completer::Slots::new();
        slots.wake(
            &id,
            harness_director(&session),
            woken(Happened::Chat("hi".into())),
        );
        assert!(fx.wait_for("prompt", 1));
        assert_eq!(
            slots.wake(&id, harness_director(&session), woken(Happened::Summon),),
            crate::completer::Woke::Dropped,
            "opening Chat cancelled the reply already on its way"
        );

        let answered = polled(&mut slots).expect("the chat turn's answer");
        assert!(
            matches!(answered.context.happened, Happened::Chat(_)),
            "the answer belongs to the typed line, not {:?}",
            answered.context.happened
        );
        assert_eq!(said(&answered), Some("Hello"));
        assert_eq!(fx.count("cancel"), 0);
        assert_eq!(fx.count("prompt"), 1, "the Summon was dropped, not queued");
        session.shutdown();
    }

    /// The guard with nothing asked: silence on the wire is a stall (#1001).
    #[test]
    fn a_slow_turn_is_cancelled_and_the_next_one_works() {
        let (fx, session) = Fixture::new("slow");
        let session = session.with_timeout(Duration::from_millis(500));
        let reply = session.complete(&asking("hi"), &|_| {});
        assert!(
            reply.as_ref().is_err_and(|why| why.contains("cancelled")),
            "{reply:?}"
        );
        assert!(fx.wait_for("cancel", 1));
        assert_eq!(
            session.complete(&asking("again"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        assert_eq!(fx.count("spawn"), 1, "cancel is not a respawn");
        session.shutdown();
    }

    #[test]
    fn a_child_that_exits_mid_turn_is_respawned_on_the_next_wake() {
        let (fx, session) = Fixture::new("exit");
        // The wait a death buys is the next test's subject; this one is about
        // the respawn that has to happen once the wait is over.
        let session = session.with_backoff(Duration::ZERO);
        assert_eq!(
            session.complete(&asking("hi"), &|_| {}),
            Err("harness exited".to_string())
        );
        assert!(!session.inspect().alive);
        assert_eq!(
            session.complete(&asking("again"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        assert_eq!(fx.count("spawn"), 2);
        assert!(session.inspect().alive);
        session.shutdown();
    }

    /// One count is shared by every way a wake goes unserved that leaves a
    /// child which might come back, so a Harness that dies is not respawned
    /// on the very next wake.
    #[test]
    fn a_death_under_a_turn_buys_the_same_wait_a_failed_spawn_does() {
        let (fx, session) = Fixture::new("die");
        assert_eq!(
            session.complete(&asking("hi"), &|_| {}),
            Err("harness exited".to_string())
        );
        let next = session.complete(&asking("again"), &|_| {}).unwrap_err();
        assert!(next.contains("retrying in"), "{next}");
        assert_eq!(fx.count("spawn"), 1, "the dying child was spawned again");
        session.shutdown();
    }

    /// The death that reaches nobody. `initialize` is answered and the child
    /// exits before `session/new`, so the loss surfaces in `open_session`.
    /// Charged all the same, or this Harness is respawned on every wake forever.
    #[test]
    fn a_death_before_the_session_opens_buys_the_same_wait() {
        let (fx, session) = Fixture::new("die-opening");
        assert_eq!(
            session.complete(&asking("hi"), &|_| {}),
            Err("harness exited".to_string())
        );
        assert_eq!(fx.count("new"), 1);
        let next = session.complete(&asking("again"), &|_| {}).unwrap_err();
        assert!(next.contains("retrying in"), "{next}");
        assert_eq!(fx.count("spawn"), 1, "the dying child was spawned again");
        session.shutdown();
    }

    /// The other half of one counter. A turn the child answered clears a death
    /// from earlier, whether the answer was a reply, a stop reason, or a
    /// timeout we gave up on.
    #[test]
    fn a_turn_the_child_answered_clears_an_earlier_death() {
        let (_fx, session) = Fixture::new("exit");
        let session = session.with_backoff(Duration::ZERO);
        assert_eq!(
            session.complete(&asking("hi"), &|_| {}),
            Err("harness exited".to_string())
        );
        assert_eq!(
            session.complete(&asking("again"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        let state = session.state.lock().unwrap();
        assert_eq!(state.spawn_failures, 0, "the death is still counted");
        assert!(
            state.spawn_wait_until.is_none(),
            "the served turn left a wait behind"
        );
        drop(state);
        session.shutdown();
    }

    #[test]
    fn auth_required_names_the_login_and_the_retry_gate_holds() {
        let (fx, session) = Fixture::new("auth");
        let reply = session.complete(&asking("hi"), &|_| {});
        assert_eq!(reply, Err(not_authenticated("fake --login")));
        assert_eq!(session.inspect().login.as_deref(), Some("fake --login"));
        // Inside the gate. Fails fast, no second session/new on the wire.
        assert!(session.complete(&asking("hi"), &|_| {}).is_err());
        assert_eq!(fx.count("new"), 1);
        session.shutdown();

        let (fx, session) = Fixture::new("auth");
        let session = session.with_auth_retry(Duration::ZERO);
        assert!(session.complete(&asking("hi"), &|_| {}).is_err());
        assert_eq!(
            session.complete(&asking("hi"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        assert_eq!(fx.count("new"), 2);
        assert_eq!(session.inspect().login, None);
        session.shutdown();
    }

    /// The Chat button's path (#1000): `authenticate`, then a fresh
    /// `session/new`. Ok from `authenticate` is not signed-in, so the landing
    /// clears only when that session opens, and stays up when it is refused.
    #[test]
    fn a_sign_in_clears_the_landing_only_when_the_next_session_opens() {
        let (fx, session) = Fixture::new("auth-sign-in");
        assert_eq!(
            session.complete(&asking("hi"), &|_| {}),
            Err(not_authenticated("fake --login"))
        );
        assert_eq!(session.sign_in("fake", "buddy-1", "bmo", false), Ok(()));
        assert_eq!(fx.count("authenticate"), 1);
        assert_eq!(fx.count("new"), 2);
        assert_eq!(session.inspect().login, None);
        assert_eq!(
            session.complete(&asking("hi"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        session.shutdown();

        let (fx, session) = Fixture::new("auth-sign-in-noop");
        assert!(session.complete(&asking("hi"), &|_| {}).is_err());
        assert_eq!(
            session.sign_in("fake", "buddy-1", "bmo", false),
            Err(not_authenticated("fake --login"))
        );
        assert_eq!(fx.count("authenticate"), 1);
        assert_eq!(fx.count("new"), 2);
        assert_eq!(session.inspect().login.as_deref(), Some("fake --login"));
        session.shutdown();
    }

    /// A Harness that refused `session/new` is alive, so a pick of the same
    /// preset asks it for a session again rather than standing. The answer is
    /// proof of a terminal login, or the refusal again.
    #[test]
    fn repicking_an_unauthenticated_harness_rechecks_the_sign_in() {
        let key = SessionKey {
            instance: "buddy-1".to_string(),
            character: "bmo".to_string(),
            blank: false,
        };
        let (fx, session) = Fixture::new("auth");
        let session = Arc::new(session);
        assert_eq!(
            session.complete(&asking("hi"), &|_| {}),
            Err(not_authenticated("fake --login"))
        );
        assert!(session.inspect().alive);
        fx.attach_settled();
        assert_eq!(
            session.repick(None),
            Err("unknown instance".to_string()),
            "a pick with no session slot to open said nothing"
        );
        assert_eq!(session.repick(Some(key.clone())), Ok(()));
        fx.attach_settled();
        assert_eq!(fx.count("new"), 2, "the pick asked for no session");
        assert_eq!(fx.count("spawn"), 1, "the pick restarted the child");
        assert_eq!(
            fx.count("authenticate"),
            0,
            "a pick is not a sign-in button"
        );
        assert_eq!(session.inspect().login, None);
        assert_eq!(
            session.complete(&asking("hi"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        session.shutdown();

        let (fx, session) = Fixture::new("auth-sign-in-noop");
        let session = Arc::new(session);
        assert!(session.complete(&asking("hi"), &|_| {}).is_err());
        assert_eq!(
            session.repick(Some(key)),
            Err(not_authenticated("fake --login"))
        );
        assert_eq!(fx.count("new"), 2);
        assert_eq!(session.inspect().login.as_deref(), Some("fake --login"));
        session.shutdown();
    }

    /// A Harness that signs in through a link hands it over mid-`authenticate`,
    /// before any session exists. Chat gets the link and the code in it, and
    /// the user's Open or Decline is what `authenticate` waits on.
    #[test]
    fn a_sign_in_link_reaches_chat_and_the_answer_finishes_authenticate() {
        for (answer, option, action, signed_in) in [
            (
                ElicitationAnswer::Accept("open".into()),
                "open",
                "accept",
                true,
            ),
            (ElicitationAnswer::Decline, "decline", "decline", false),
        ] {
            let (fx, session) = Fixture::new("auth-sign-in-link");
            let session = Arc::new(session);
            assert!(session.complete(&asking("hi"), &|_| {}).is_err());
            fx.attach_settled();
            let worker = {
                let session = Arc::clone(&session);
                thread::spawn(move || session.sign_in("fake", "buddy-1", "bmo", false))
            };
            let form = fx.form();
            assert_eq!(
                form.url.as_deref(),
                Some("https://example.test/device?code=ABCD-1234")
            );
            assert!(!form.waits, "a sign-in Fidget started opens Chat");
            assert_eq!(form.message, "Enter ABCD-1234 on the sign-in page.");
            assert!(form.options.is_empty());
            session.answer_elicitation(&form.request, answer);
            assert_eq!(fx.settled(), (form.request, Some(option.to_string())));
            assert_eq!(worker.join().unwrap().is_ok(), signed_in, "{action}");
            assert!(fx.wait_for(&format!("elicit-url:{action}"), 1));
            assert_eq!(fx.count("new"), 1 + usize::from(signed_in), "{action}");
            session.shutdown();
        }
    }

    /// An MCP server's link after `session/new` is not one Fidget asked for,
    /// so it waits in Chat. It lands between turns or in the first one, and
    /// either way outlives that turn, so the next Chat to open can answer it.
    #[test]
    fn a_link_nobody_asked_for_waits_past_the_turn_and_can_still_be_answered() {
        for script in ["mcp-link", "mcp-link-turn"] {
            let (fx, session) = Fixture::new(script);
            assert_eq!(
                session.complete(&asking("hi"), &|_| {}),
                Ok(Reply::whole("Hello"))
            );
            let mut form = None;
            while let Ok(forwarded) = fx.forwarded.recv_timeout(Duration::from_millis(500)) {
                match forwarded {
                    Forwarded::Form { form: link, .. } => form = Some(link),
                    Forwarded::Settled { request, .. } => panic!("{script}: {request} was retired"),
                    _ => {}
                }
            }
            let form = form.expect("the link was never forwarded");
            assert_eq!(form.url.as_deref(), Some("https://example.test/oauth"));
            assert!(form.waits, "{script}");
            session.answer_elicitation(&form.request, ElicitationAnswer::Decline);
            assert!(fx.wait_for("elicit-mcp:decline", 1), "{script}");
            assert_eq!(fx.settled(), (form.request, Some("decline".to_string())));
            session.shutdown();
        }
    }

    /// A link from a tool call blocks its turn, so it does not wait: Chat opens
    /// for it as for a permission ask, and the answer finishes the turn.
    #[test]
    fn a_tool_call_link_does_not_wait_and_its_answer_finishes_the_turn() {
        let (fx, session) = Fixture::new("mcp-link-tool");
        let session = Arc::new(session);
        let worker = {
            let session = Arc::clone(&session);
            thread::spawn(move || session.complete(&asking("hi"), &|_| {}))
        };
        let form = fx.form();
        assert_eq!(form.url.as_deref(), Some("https://example.test/oauth"));
        assert!(!form.waits);
        session.answer_elicitation(&form.request, ElicitationAnswer::Decline);
        assert_eq!(worker.join().unwrap(), Ok(Reply::whole("Hello")));
        assert!(fx.wait_for("elicit-mcp:decline", 1));
        assert_eq!(fx.settled(), (form.request, Some("decline".to_string())));
        session.shutdown();
    }

    /// `elicitation/complete` for a held link retires its row as an answer
    /// would, with no option, and answers the request with `cancel`. One for
    /// an id no form holds is ignored.
    #[test]
    fn a_completed_link_retires_its_row_and_answers_cancel() {
        let (fx, session) = Fixture::new("mcp-link-complete");
        let key = SessionKey {
            instance: "buddy-1".to_string(),
            character: "bmo".to_string(),
            blank: false,
        };
        assert!(session.attach(Some(&key)).is_ok());
        let form = fx.form();
        assert_eq!(form.url.as_deref(), Some("https://example.test/oauth"));
        assert_eq!(fx.settled(), (form.request, None));
        let stray =
            std::iter::from_fn(|| fx.forwarded.recv_timeout(Duration::from_millis(300)).ok())
                .find(|forwarded| !matches!(forwarded, Forwarded::AttachSettled));
        assert!(
            stray.is_none(),
            "the unknown id retired something: {stray:?}"
        );
        assert!(fx.wait_for("elicit-mcp:cancel", 1));
        assert_eq!(fx.count("elicit-mcp:cancel"), 1);
        for action in ["accept", "decline"] {
            assert_eq!(fx.count(&format!("elicit-mcp:{action}")), 0, "{action}");
        }
        session.shutdown();
    }

    /// A link held between turns, completed while a turn runs, is retired by
    /// that turn and answered with `cancel`. An unknown id mid-turn is ignored.
    #[test]
    fn a_link_completed_mid_turn_retires_the_row_held_between_turns() {
        let (fx, session) = Fixture::new("mcp-link-complete-turn");
        assert_eq!(
            session.complete(&asking("hi"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        let mut form = None;
        let mut settled = None;
        while let Ok(forwarded) = fx.forwarded.recv_timeout(Duration::from_millis(500)) {
            match forwarded {
                Forwarded::Form { form: link, .. } => form = Some(link),
                Forwarded::Settled { request, option } => settled = Some((request, option)),
                _ => {}
            }
        }
        let form = form.expect("the link was never forwarded");
        assert_eq!(settled, Some((form.request, None)));
        assert!(fx.wait_for("elicit-mcp:cancel", 1));
        assert_eq!(fx.count("elicit-mcp:cancel"), 1);
        for action in ["accept", "decline"] {
            assert_eq!(fx.count(&format!("elicit-mcp:{action}")), 0, "{action}");
        }
        session.shutdown();
    }

    /// The attach path's translation, on the turn path (#991). The session
    /// stays open: the token died, not the child, and the next answer is the
    /// proof the user signed in somewhere else.
    #[test]
    fn an_auth_refusal_on_the_turn_names_the_login_and_the_next_answer_clears_it() {
        for script in ["auth-turn", "auth-turn-32000"] {
            let (fx, session) = Fixture::new(script);
            assert_eq!(
                session.complete(&asking("hi"), &|_| {}),
                Err(not_authenticated("fake --login")),
                "{script}"
            );
            assert_eq!(session.inspect().login.as_deref(), Some("fake --login"));
            fx.attach_settled();
            assert_eq!(
                session.complete(&asking("again"), &|_| {}),
                Ok(Reply::whole("Hello"))
            );
            assert_eq!(session.inspect().login, None);
            fx.attach_settled();
            assert_eq!(fx.count("new"), 1, "{script}: the session was kept");
            session.shutdown();
        }
    }

    const BALANCE_EXHAUSTED: &str = "Internal error: API error (status 402 Payment Required): Grok Build usage balance exhausted";

    /// A Harness that fails the turn says why in `data`. Dropping it left
    /// Chat with "Internal error" and nothing to act on.
    #[test]
    fn a_failed_turn_carries_the_reason_the_harness_put_in_data() {
        let (_fx, session) = Fixture::new("balance-exhausted");
        assert_eq!(
            session.complete(&asking("hi"), &|_| {}),
            Err(BALANCE_EXHAUSTED.to_string())
        );
        assert_eq!(
            session.inspect().turn_failure.as_deref(),
            Some(BALANCE_EXHAUSTED)
        );
        session.shutdown();
    }

    #[test]
    fn a_saved_session_is_loaded_and_a_stale_one_falls_back_to_new() {
        let (fx, session) = Fixture::new("load");
        std::fs::write(
            fx.dir.join(SESSION_FILE),
            r#"{"harness":"fake","sessions":[{"instance":"buddy-1","character":"bmo","session_id":"saved-ok"}]}"#,
        )
        .unwrap();
        assert_eq!(
            session.complete(&asking("hi"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        assert_eq!(fx.count("load"), 1);
        assert_eq!(fx.count("new"), 0);
        assert_eq!(session.inspect().session_id.as_deref(), Some("saved-ok"));
        session.shutdown();

        let (fx, session) = Fixture::new("load");
        std::fs::write(
            fx.dir.join(SESSION_FILE),
            r#"{"harness":"fake","sessions":[{"instance":"buddy-1","character":"bmo","session_id":"stale"}]}"#,
        )
        .unwrap();
        assert_eq!(
            session.complete(&asking("hi"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        assert_eq!(fx.count("load"), 1);
        assert_eq!(fx.count("new"), 1);
        let saved = std::fs::read_to_string(fx.dir.join(SESSION_FILE)).unwrap();
        assert!(saved.contains("fresh-id"), "{saved}");
        session.shutdown();

        // Another Harness's session is not ours to load.
        let (fx, session) = Fixture::new("load");
        std::fs::write(
            fx.dir.join(SESSION_FILE),
            r#"{"harness":"other","sessions":[{"instance":"buddy-1","character":"bmo","session_id":"saved-ok"}]}"#,
        )
        .unwrap();
        assert_eq!(
            session.complete(&asking("hi"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        assert_eq!(fx.count("load"), 0);
        session.shutdown();
    }

    /// `hermes` answers a `session/load` it cannot honour with an empty
    /// success result, so the id is dead and only a turn says so. One reopen,
    /// and the retry lands on a session the Harness actually has.
    #[test]
    fn a_load_that_did_not_restore_reopens_once_and_the_retry_lands() {
        let (fx, session) = Fixture::new("load-dead");
        std::fs::write(
            fx.dir.join(SESSION_FILE),
            r#"{"harness":"fake","sessions":[{"instance":"buddy-1","character":"bmo","session_id":"saved-ok"}]}"#,
        )
        .unwrap();
        assert_eq!(
            session.complete(&asking("hi"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        assert_eq!(fx.count("load"), 1);
        assert_eq!(fx.count("new"), 1, "the dead id was kept");
        assert_eq!(fx.count("prompt"), 2);
        assert_eq!(fx.count("spawn"), 1, "a reopen is not a respawn");
        assert_eq!(session.inspect().session_id.as_deref(), Some("fresh-id"));
        // And the next launch cannot read the dead id back out of the file.
        let saved = std::fs::read_to_string(fx.dir.join(SESSION_FILE)).unwrap();
        assert!(saved.contains("fresh-id"), "{saved}");
        session.shutdown();
    }

    /// The reopen is one attempt, not a ladder. A Harness that refuses the
    /// fresh session too has answered.
    #[test]
    fn a_reopened_session_that_refuses_again_is_a_refusal() {
        let (fx, session) = Fixture::new("load-refusal");
        std::fs::write(
            fx.dir.join(SESSION_FILE),
            r#"{"harness":"fake","sessions":[{"instance":"buddy-1","character":"bmo","session_id":"saved-ok"}]}"#,
        )
        .unwrap();
        let reply = session.complete(&asking("hi"), &|_| {});
        assert!(
            reply.as_ref().is_err_and(|why| why.contains("refusal")),
            "{reply:?}"
        );
        assert_eq!(fx.count("new"), 1);
        assert_eq!(fx.count("prompt"), 2, "the reopen was tried more than once");
        session.shutdown();
    }

    /// `cancelled` is a stop reason, not evidence the load failed. Reopening
    /// would re-send a prompt carrying the Instance Prompt a save just
    /// replaced (ADR-0012). A cancel says nothing about the id.
    #[test]
    fn a_cancelled_turn_is_not_reopened_as_a_failed_load() {
        let (fx, session) = Fixture::new("load-slow");
        std::fs::write(
            fx.dir.join(SESSION_FILE),
            r#"{"harness":"fake","sessions":[{"instance":"buddy-1","character":"bmo","session_id":"saved-ok"}]}"#,
        )
        .unwrap();
        let session = Arc::new(session.with_timeout(Duration::from_secs(10)));
        let worker = {
            let session = Arc::clone(&session);
            thread::spawn(move || session.complete(&asking("hi"), &|_| {}))
        };
        assert!(fx.wait_for("prompt", 1), "the first turn never went out");
        // The cancel arrives the way a save's does, through the path a newer
        // wake already takes. The loser writes its own line before the winner
        // gets the lock, so what it did with the cancel is settled here.
        assert_eq!(
            session.complete(&asking("again"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        let withdrawn = worker.join().unwrap().unwrap_err();
        assert!(withdrawn.contains("cancelled"), "{withdrawn}");
        assert_eq!(fx.count("load"), 1);
        assert_eq!(fx.count("new"), 0, "the cancelled turn threw the id away");
        assert_eq!(fx.count("prompt"), 2, "the cancelled turn was re-prompted");
        session.shutdown();
    }

    /// A binary that `PATH` has not got is not charged the respawn ladder. The
    /// next wake asks again. The ladder itself is untouched, which the three
    /// `backoff` reads below still hold.
    #[test]
    fn a_missing_binary_says_so_instead_of_backing_off() {
        const NOPE: &str = "/nonexistent/fidget-no-such-harness";
        let dir = std::env::temp_dir().join(format!("fidget-harness-{}", uuid::Uuid::new_v4()));
        let launch = Launch {
            name: "nope".into(),
            argv: vec![NOPE.into()],
        };
        std::fs::create_dir_all(&dir).unwrap();
        let session = isolated_session(launch, dir.clone(), silent());
        assert_eq!(
            session.complete(&asking("hi"), &|_| {}),
            Err(not_installed(NOPE)),
            "the wake was answered with an errno"
        );
        assert!(
            session.state.lock().unwrap().spawn_wait_until.is_none(),
            "a binary that is not there was charged a respawn wait"
        );
        // No wait means the next wake asks again rather than being refused, so
        // installing the CLI is picked up without a relaunch.
        assert_eq!(
            session.complete(&asking("hi"), &|_| {}),
            Err(not_installed(NOPE)),
            "the second wake was refused by a backoff"
        );
        let inspect = session.inspect();
        assert_eq!(inspect.missing.as_deref(), Some(NOPE));
        assert!(!inspect.alive);
        assert_eq!(session.backoff(1), BACKOFF_FIRST);
        assert_eq!(session.backoff(2), Duration::from_secs(10));
        assert_eq!(session.backoff(40), BACKOFF_CAP);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Chat's first ReloadChat races preflight. The forwarded settle is how
    /// a missing launcher reaches an already-open surface (#726).
    #[test]
    fn spawn_preflight_forwards_when_the_launcher_is_missing() {
        const NOPE: &str = "/nonexistent/fidget-no-such-harness";
        let dir = std::env::temp_dir().join(format!("fidget-harness-{}", uuid::Uuid::new_v4()));
        let (tx, rx) = mpsc::channel();
        let launch = Launch {
            name: "nope".into(),
            argv: vec![NOPE.into()],
        };
        let session = Arc::new(Session::new(
            launch,
            Ok(AttachCwd(dir.clone())),
            SessionDataDir::at(dir.clone()),
            Arc::new(Box::new(move |forwarded| {
                let _ = tx.send(forwarded);
            }) as Forward),
        ));
        session.spawn_preflight();
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Forwarded::AttachSettled) => {}
            other => panic!("expected AttachSettled, got {other:?}"),
        }
        let inspect = session.inspect();
        assert_eq!(inspect.missing.as_deref(), Some(NOPE));
        assert!(!inspect.alive);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A launcher that is on `PATH` and dies before `initialize` is not
    /// missing, so only `failed` can tell Chat and Settings why nothing runs.
    #[test]
    fn spawn_preflight_names_a_launcher_that_dies_at_startup() {
        let (fx, session) = Fixture::new("abort-at-start");
        let session = Arc::new(session);
        session.spawn_preflight();
        match fx.forwarded.recv_timeout(Duration::from_secs(5)) {
            Ok(Forwarded::AttachSettled) => {}
            other => panic!("expected AttachSettled, got {other:?}"),
        }
        let inspect = session.inspect();
        assert!(!inspect.alive && !inspect.initializing && inspect.missing.is_none());
        let failed = inspect.failed.expect("the landing has no reason to show");
        assert_eq!(failed.command, Some(session.launch.line()));
        // Linux adds " (core dumped)" when the runner keeps cores; macOS does not.
        #[cfg(unix)]
        assert!(
            failed
                .reason
                .starts_with("exited before initialize, signal: 6 (SIGABRT)"),
            "{}",
            failed.reason
        );
        // Its stderr, and libtest's own stdout from before the fixture ran.
        assert!(
            failed.output.contains(
                "dyld[0]: Library not loaded: /opt/homebrew/opt/llhttp/lib/libllhttp.9.3.dylib"
            ),
            "stderr not captured: {:?}",
            failed.output
        );
        assert!(
            failed.output.contains("running 1 test"),
            "stdout before initialize not captured: {:?}",
            failed.output
        );
        session.shutdown();
        let _ = std::fs::remove_dir_all(&fx.dir);
    }

    /// Picking the same Harness again after fixing what killed it attaches
    /// now, not after the backoff the dead launch earned.
    #[test]
    fn a_repick_after_a_startup_death_attaches_at_once() {
        let (fx, session) = Fixture::new("abort-first");
        let session = Arc::new(session);
        session.spawn_preflight();
        fx.attach_settled();
        assert!(session.inspect().failed.is_some());
        assert_eq!(session.repick(None), Ok(()));
        fx.attach_settled();
        let inspect = session.inspect();
        assert!(inspect.alive, "the re-pick stood behind the backoff");
        assert_eq!(inspect.failed, None);
        session.shutdown();
        let _ = std::fs::remove_dir_all(&fx.dir);
    }

    /// A Chat surface asks for its opening once, before the first wake has
    /// opened a session, and draws `no session yet`. The session that wake
    /// opens has to reach the open window, or the header keeps saying so.
    #[test]
    fn a_session_the_first_wake_opens_reaches_an_open_chat_surface() {
        let (fx, session) = Fixture::new("happy");
        let session = Arc::new(session);
        session.spawn_preflight();
        fx.attach_settled();
        assert_eq!(session.inspect().session_id, None);
        assert_eq!(
            session.complete(&asking("hi"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        fx.attach_settled();
        assert_eq!(session.inspect().session_id.as_deref(), Some("fresh-id"));
        session.shutdown();
        let _ = std::fs::remove_dir_all(&fx.dir);
    }

    /// A re-pick never restarts a Harness that is answering.
    #[test]
    fn a_repick_of_a_live_harness_stands() {
        let (fx, session) = Fixture::new("plain");
        let session = Arc::new(session);
        session.spawn_preflight();
        fx.attach_settled();
        assert_eq!(session.repick(None), Ok(()));
        assert!(session.inspect().alive);
        assert_eq!(fx.count("spawn"), 1, "the re-pick restarted a live Harness");
        session.shutdown();
        let _ = std::fs::remove_dir_all(&fx.dir);
    }

    fn surface_tick(session: &Arc<Session>) -> (Duration, Vec<SignIn>) {
        use fidget_core::character::{
            Character, CursorReaction, DEFAULT_MODEL_BASE, DEFAULT_MODEL_POWER,
        };
        use fidget_core::engine::{Point, WorldSnapshot};
        use fidget_core::roster::Roster;

        struct Clear;
        impl Drop for Clear {
            fn drop(&mut self) {
                SIGN_IN_SESSION.with(|slot| *slot.borrow_mut() = None);
            }
        }
        let started = Instant::now();
        let inspect = session.inspect();
        let mut roster = Roster::new();
        let character = Character {
            name: "bmo".to_string(),
            personality: String::new(),
            animations: std::collections::BTreeMap::new(),
            behaviors: std::collections::BTreeMap::new(),
            art: std::collections::BTreeMap::new(),
            smooth: false,
            scale: 1,
            model_base: DEFAULT_MODEL_BASE,
            model_power: DEFAULT_MODEL_POWER,
            near_reaction: CursorReaction::default(),
            rush_reaction: CursorReaction::default(),
            source: None,
        };
        let id = roster.spawn(&character, "bmo".to_string(), Point { x: 10.0, y: 20.0 });
        let frame = roster.get_mut(&id).expect("spawned").tick(&WorldSnapshot {
            elapsed_ms: 16,
            ..WorldSnapshot::default()
        });
        assert!(!frame.animation.is_empty());
        // Chat reads sign-in off this session. Set before the opening is built,
        // which asks for the buttons itself.
        SIGN_IN_SESSION.with(|slot| *slot.borrow_mut() = Some(Arc::clone(session)));
        let _clear = Clear;
        let opening = crate::chat_opening_layers(
            roster.get(&id).expect("still there"),
            &crate::model::DirectorInspect {
                enabled: true,
                configured: true,
                proactive_wakes: true,
                wake_secs: 60,
                last_payload: None,
                harness: Some(inspect),
                model: String::new(),
                host: String::new(),
            },
            "",
            std::iter::empty::<&str>(),
            "minimal",
            crate::settings::ChatAppearance::System,
        );
        assert_eq!(opening.character, "bmo");
        let actions = sign_in_actions();
        assert_eq!(opening.sign_in.len(), actions.len());
        session.drop_conversation("surface");
        (started.elapsed(), actions)
    }

    #[test]
    fn a_slow_initialize_leaves_chat_and_the_character_responsive() {
        let (fx, session) = Fixture::new("stall-initialize");
        let session = Arc::new(session);
        session.spawn_preflight();
        assert!(
            fx.wait_for("spawn", 1),
            "the child never started, so the stall was not in flight"
        );
        let (elapsed, _) = surface_tick(&session);
        assert!(
            elapsed < Duration::from_millis(500),
            "chat and the character waited {elapsed:?} on initialize"
        );
        assert!(
            session.inspect().initializing,
            "the handshake finished before the surface tick"
        );
        session.shutdown();
        let _ = fx.forwarded.recv_timeout(Duration::from_secs(5));
    }

    /// A turn budget shorter than the handshake stands in for a cold `npx`
    /// start. 18 s is the slowest cold start measured, codex's.
    #[test]
    fn an_initialize_slower_than_the_turn_budget_still_attaches() {
        let (fx, session) = Fixture::new("stall-initialize");
        let session = Arc::new(session.with_timeout(Duration::from_secs(1)));
        assert!(
            session.attach_timeout() > Duration::from_secs(18),
            "a cold codex start would time out"
        );
        session.spawn_preflight();
        assert!(matches!(
            fx.forwarded.recv_timeout(Duration::from_secs(10)),
            Ok(Forwarded::AttachSettled)
        ));
        let inspect = session.inspect();
        assert_eq!(
            (inspect.alive, inspect.initializing, inspect.failed),
            (true, false, None)
        );
        session.shutdown();
    }

    #[test]
    fn signing_in_leaves_chat_and_the_character_responsive() {
        let (fx, session) = Fixture::new("stall-sign-in");
        let session = Arc::new(session);
        assert_eq!(
            session.complete(&asking("hi"), &|_| {}),
            Err(not_authenticated("fake --login"))
        );
        let worker = {
            let session = Arc::clone(&session);
            thread::spawn(move || session.sign_in("fake", "buddy-1", "bmo", false))
        };
        assert!(
            fx.wait_for("open-stall", 1),
            "sign-in never reached session/new"
        );
        let (elapsed, actions) = surface_tick(&session);
        assert!(
            elapsed < Duration::from_millis(500),
            "chat and the character waited {elapsed:?} on sign-in"
        );
        assert!(
            !actions.is_empty(),
            "chat lost the sign-in button while login was in flight"
        );
        assert_eq!(worker.join().unwrap(), Ok(()));
        assert_eq!(session.inspect().login, None);
        session.shutdown();
    }

    #[test]
    fn authenticate_leaves_chat_and_the_character_responsive() {
        let (fx, session) = Fixture::new("stall-authenticate");
        let session = Arc::new(session);
        assert_eq!(
            session.complete(&asking("hi"), &|_| {}),
            Err(not_authenticated("fake --login"))
        );
        let worker = {
            let session = Arc::clone(&session);
            thread::spawn(move || session.sign_in("fake", "buddy-1", "bmo", false))
        };
        assert!(
            fx.wait_for("auth-stall", 1),
            "sign-in never reached authenticate"
        );
        let (elapsed, actions) = surface_tick(&session);
        assert!(
            elapsed < Duration::from_millis(500),
            "chat and the character waited {elapsed:?} on authenticate"
        );
        assert!(
            !actions.is_empty(),
            "chat lost the sign-in button while authenticate was in flight"
        );
        assert_eq!(worker.join().unwrap(), Ok(()));
        assert_eq!(session.inspect().login, None);
        session.shutdown();
    }

    #[test]
    fn a_sign_in_does_not_block_another_instances_attach() {
        let (fx, session) = Fixture::new("stall-authenticate");
        let session = Arc::new(session.with_auth_retry(Duration::ZERO));
        assert_eq!(
            session.complete(&asking("hi"), &|_| {}),
            Err(not_authenticated("fake --login"))
        );
        let worker = {
            let session = Arc::clone(&session);
            thread::spawn(move || session.sign_in("fake", "buddy-1", "bmo", false))
        };
        assert!(
            fx.wait_for("auth-stall", 1),
            "sign-in never reached authenticate"
        );
        let started = Instant::now();
        let peer = session.complete(&asking_as("buddy-2", "bmo", "hi"), &|_| {});
        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_millis(500),
            "another instance waited {elapsed:?} on sign-in"
        );
        assert_eq!(peer, Ok(Reply::whole("Hello")));
        assert_eq!(worker.join().unwrap(), Ok(()));
        session.shutdown();
    }

    #[test]
    fn a_drop_during_session_open_is_not_put_back() {
        let (fx, session) = Fixture::new("stall-open");
        let session = Arc::new(session);
        let worker = {
            let session = Arc::clone(&session);
            thread::spawn(move || session.complete(&asking("hi"), &|_| {}))
        };
        assert!(fx.wait_for("open-stall", 1), "the open never stalled");
        let started = Instant::now();
        session.drop_conversation("buddy-1");
        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_millis(500),
            "the character tick waited {elapsed:?} on session/new"
        );
        let first = worker.join().unwrap();
        assert!(
            first.is_err(),
            "the open that was dropped still counted: {first:?}"
        );
        assert_eq!(session.inspect().session_id, None);
        assert_eq!(
            session.complete(&asking("again"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        assert_eq!(fx.count("new"), 2, "the dropped id was reused");
        assert_eq!(
            fx.count("close"),
            1,
            "the dropped session stayed open on the agent"
        );
        session.shutdown();
    }

    /// Initializing gates chat during ACP handshake. Set true at spawn_preflight,
    /// false on success or failure.
    #[test]
    fn initializing_gates_chat_until_spawn_completes() {
        const NOPE: &str = "/nonexistent/fidget-no-such-harness";
        let dir = std::env::temp_dir().join(format!("fidget-harness-{}", uuid::Uuid::new_v4()));
        let (tx, rx) = mpsc::channel();
        let launch = Launch {
            name: "nope".into(),
            argv: vec![NOPE.into()],
        };
        let session = Arc::new(Session::new(
            launch,
            Ok(AttachCwd(dir.clone())),
            SessionDataDir::at(dir.clone()),
            Arc::new(Box::new(move |forwarded| {
                let _ = tx.send(forwarded);
            }) as Forward),
        ));

        // Before spawn_preflight, initializing is false
        assert!(!session.inspect().initializing, "initializing starts false");

        session.spawn_preflight();

        // Immediately after spawn_preflight, initializing is true
        assert!(
            session.inspect().initializing,
            "initializing is true during spawn"
        );

        // Wait for attach to complete
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Forwarded::AttachSettled) => {}
            other => panic!("expected AttachSettled, got {other:?}"),
        }

        // After spawn fails, initializing is false
        let inspect = session.inspect();
        assert!(
            !inspect.initializing,
            "initializing is false after spawn fails"
        );
        assert!(!inspect.alive, "alive is false after spawn fails");

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn initializing_clears_on_spawn_failed_not_just_missing() {
        let dir = std::env::temp_dir().join(format!("fidget-harness-{}", uuid::Uuid::new_v4()));
        let (tx, rx) = mpsc::channel();
        let launch = Launch {
            name: "fails".into(),
            argv: vec!["any".into()],
        };
        let session = Arc::new(Session::new(
            launch,
            Err(CwdError::Relative(PathBuf::from("relative/path"))),
            SessionDataDir::at(dir.clone()),
            Arc::new(Box::new(move |forwarded| {
                let _ = tx.send(forwarded);
            }) as Forward),
        ));

        assert!(!session.inspect().initializing);

        session.spawn_preflight();

        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Forwarded::AttachSettled) => {}
            other => panic!("expected AttachSettled, got {other:?}"),
        }

        assert!(!session.inspect().initializing);
        assert!(!session.inspect().alive);

        let _ = std::fs::remove_dir_all(dir);
    }

    /// `missing` has to be cleared by a spawn that fails, not only by one
    /// that works. Otherwise Settings tells the user to install what they
    /// just installed and never names the real failure.
    #[test]
    fn a_spawn_that_fails_for_another_reason_stops_saying_not_installed() {
        let (fx, session) = Fixture::new("die-initializing");
        session.note_missing();
        let reply = session.complete(&asking("hi"), &|_| {});
        assert!(
            reply.as_ref().is_err_and(|why| why.contains("initialize")),
            "{reply:?}"
        );
        let inspect = session.inspect();
        assert_eq!(inspect.missing, None, "the row still says not installed");
        assert!(!inspect.alive);
        // The child did start, so the ladder is still the right answer for it.
        assert!(session.state.lock().unwrap().spawn_wait_until.is_some());
        session.shutdown();
        let _ = std::fs::remove_dir_all(&fx.dir);
    }

    /// `codex` attaches through `npx` and logs in through `codex`, so `argv[0]`
    /// being present says nothing about the vendor CLI. What is named is the
    /// file that was looked for, never the preset.
    #[test]
    fn a_missing_launcher_is_not_a_missing_vendor_cli() {
        let launch = launch(Some("codex")).unwrap();
        assert_eq!(launch.argv[0], "npx", "the codex preset stopped using npx");
        let dir = std::env::temp_dir().join(format!("fidget-harness-{}", uuid::Uuid::new_v4()));
        let session = isolated_session(launch, dir, silent());
        assert_eq!(session.note_missing(), not_installed("npx"));
        assert_eq!(session.inspect().missing.as_deref(), Some("npx"));
    }

    /// Documents the missing-binary contract: `missing` = `argv[0]` for every
    /// preset. npx adapters (claude, codex, pi) report `npx` missing, not the
    /// vendor CLI. First-party CLIs (copilot, cursor-agent, goose, grok,
    /// hermes, opencode) report their own name.
    #[test]
    fn each_preset_reports_its_argv_0_as_missing() {
        let cases = [
            ("claude", "npx"),
            ("codex", "npx"),
            ("pi", "npx"),
            ("copilot", "copilot"),
            ("cursor-agent", "cursor-agent"),
            ("goose", "goose"),
            ("grok", "grok"),
            ("hermes", "hermes"),
            ("opencode", "opencode"),
        ];
        for (preset, expected_argv0) in cases {
            let launch = launch(Some(preset)).unwrap();
            assert_eq!(
                launch.argv[0], expected_argv0,
                "preset {preset} should have argv[0]={expected_argv0}"
            );
        }
    }

    #[test]
    fn missing_binary_messages_include_install_urls() {
        let cases = [
            ("npx", "nodejs.org"),
            ("hermes", "hermes-agent.nousresearch.com"),
            ("goose", "goose-docs.ai/docs/getting-started/installation"),
            ("copilot", "docs.github.com/en/copilot/how-tos/copilot-cli/set-up-copilot-cli/install-copilot-cli"),
            ("cursor-agent", "cursor.com"),
            ("grok", "x.ai"),
            ("opencode", "opencode.ai"),
            ("agy_acp_server.par", "antigravity.google/docs/ide/extensions"),
            ("agy_acp_server.exe", "antigravity.google/docs/ide/extensions"),
        ];
        for (command, url_part) in cases {
            let message = not_installed(command);
            assert!(
                message.contains(url_part),
                "missing {command} should mention {url_part}, got: {message}"
            );
        }
    }

    /// An `npx` preset that dies at startup points at Node.js, not the preset.
    #[test]
    fn an_npx_launcher_that_exits_says_to_check_node() {
        assert_eq!(
            exited(&launch(Some("codex")).unwrap(), None, String::new()).sentence(),
            "`npx -y @agentclientprotocol/codex-acp@latest` exited before initialize. \
             `npx` runs on Node.js: run `node --version` in a terminal to check that it starts."
        );
        assert_eq!(
            exited(&launch(Some("hermes")).unwrap(), None, String::new()).sentence(),
            "`hermes acp` exited before initialize."
        );
    }

    /// ADR-0016's newest-wins, at the Harness seam. The Poke that arrives
    /// under a turn takes it rather than being refused.
    #[test]
    fn a_newer_wake_cancels_the_turn_in_flight_and_takes_it() {
        let (fx, session) = Fixture::new("slow");
        let session = Arc::new(session.with_timeout(Duration::from_secs(10)));
        let worker = {
            let session = Arc::clone(&session);
            thread::spawn(move || session.complete(&asking("hi"), &|_| {}))
        };
        // The first prompt is on the wire, so the turn lock is held and the
        // wake below is the one that has to displace it.
        assert!(fx.wait_for("prompt", 1), "the first turn never went out");
        assert_eq!(
            session.complete(&asking("again"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        assert_eq!(
            fx.count("cancel"),
            1,
            "the displaced turn was not cancelled"
        );
        assert_eq!(fx.count("prompt"), 2, "the second prompt never went out");
        // A withdrawal, not a failure. Cancelled by name in the log, no
        // `refused` line, and no respawn backoff charged.
        let displaced = worker.join().unwrap().unwrap_err();
        assert!(displaced.contains("cancelled"), "{displaced}");
        assert!(
            fx.events("refused").is_empty(),
            "{:?}",
            fx.events("refused")
        );
        assert_eq!(session.state.lock().unwrap().spawn_failures, 0);
        let turns = fx.events("turn");
        assert_eq!(turns[0]["stop"], json!("cancelled"), "{turns:?}");
        session.shutdown();
    }

    /// Switching the AI source commits on the UI thread, so `retarget` must
    /// not wait on the child it is dropping. That is cheap only because
    /// `Msg::Shutdown` ends the turn in flight instead of being swallowed by it.
    #[test]
    fn retarget_does_not_wait_out_a_turn_in_flight() {
        let (fx, session) = Fixture::new("slow");
        let session = Arc::new(session);
        let worker = {
            let session = Arc::clone(&session);
            thread::spawn(move || session.complete(&asking("hi"), &|_| {}))
        };
        // The prompt is on the wire and unanswered. `shutdown` posts into a
        // `turn`, not into `serve`.
        assert!(fx.wait_for("prompt", 1), "the turn never went out");
        {
            let mut slot = attachment();
            slot.forward = Some(silent());
            slot.session = Some(Arc::clone(&session));
        }
        let start = Instant::now();
        // Off is `None`, which `reattach` reads as `Drop`. The session goes and
        // nothing replaces it.
        retarget(None, false);
        let waited = start.elapsed();
        // The session is process-global; leave the slot as the other tests
        // expect to find it.
        {
            let mut slot = attachment();
            slot.session = None;
            slot.forward = None;
        }
        // Fast and reaped are the same fact. `shutdown` returns when the
        // wire thread ends after `kill_harness_tree`, or when `REAP` runs
        // out at 2s. Under half a second is killed. 2s is still running.
        assert!(
            waited < Duration::from_millis(500),
            "retarget waited {waited:?} on the dropped session"
        );
        assert!(attached().is_none(), "the Off pick did not take");
        // And the child really was killed rather than left behind. The wire
        // thread ends only after `kill_harness_tree`, and the turn's caller
        // only returns once that thread has hung up on it.
        assert!(
            worker.join().unwrap().is_err(),
            "the turn outlived the kill"
        );
        assert!(
            !session.inspect().alive,
            "the dropped session still reads live"
        );
    }

    /// `session/new` holds the wire thread, so an inline reap waits out `REAP`.
    /// The switch returns while that open is still asleep.
    #[test]
    fn retarget_does_not_wait_out_a_session_open() {
        let (fx, session) = Fixture::new("stall-open");
        let session = Arc::new(session);
        let worker = {
            let session = Arc::clone(&session);
            thread::spawn(move || session.complete(&asking("hi"), &|_| {}))
        };
        assert!(fx.wait_for("open-stall", 1), "the open never stalled");
        {
            let mut slot = attachment();
            slot.forward = Some(silent());
            slot.session = Some(Arc::clone(&session));
        }
        let start = Instant::now();
        retarget(None, false);
        let waited = start.elapsed();
        {
            let mut slot = attachment();
            slot.session = None;
            slot.forward = None;
        }
        assert!(
            waited < Duration::from_millis(500),
            "retarget waited {waited:?} on the session being opened"
        );
        assert!(attached().is_none(), "the Off pick did not take");
        assert!(
            worker.join().unwrap().is_err(),
            "the open finished on the harness retarget dropped"
        );
    }

    #[test]
    fn a_failed_switch_thread_still_reaps_the_old_child() {
        let (fx, session) = Fixture::new("stall-open");
        let session = Arc::new(session);
        let worker = {
            let session = Arc::clone(&session);
            thread::spawn(move || session.complete(&asking("hi"), &|_| {}))
        };
        assert!(fx.wait_for("open-stall", 1), "the open never stalled");
        {
            let mut slot = attachment();
            slot.forward = Some(silent());
            slot.session = Some(Arc::clone(&session));
        }
        struct Clear;
        impl Drop for Clear {
            fn drop(&mut self) {
                FAIL_SWITCH_SPAWN.store(false, Ordering::SeqCst);
            }
        }
        let _clear = Clear;
        FAIL_SWITCH_SPAWN.store(true, Ordering::SeqCst);
        let start = Instant::now();
        retarget(None, false);
        let waited = start.elapsed();
        {
            let mut slot = attachment();
            slot.session = None;
            slot.forward = None;
        }
        assert!(
            waited >= Duration::from_millis(1000),
            "the failed switch returned in {waited:?} without waiting out the reap"
        );
        assert!(
            !session.wanted.load(Ordering::SeqCst),
            "the old child was dropped without shutdown"
        );
        assert!(session.current_wire().is_none());
        let _ = worker.join();
    }

    #[test]
    fn saving_a_session_does_not_hold_chat_state() {
        let (_fx, session) = Fixture::new("plain");
        let session = Arc::new(session);
        SESSION_SAVE_STALL.store(true, Ordering::SeqCst);
        let worker = {
            let session = Arc::clone(&session);
            thread::spawn(move || session.complete(&asking("hi"), &|_| {}))
        };
        let until = Instant::now() + Duration::from_secs(5);
        while !SESSION_SAVE_STALLING.load(Ordering::SeqCst) {
            assert!(Instant::now() < until, "the session save never stalled");
            thread::sleep(Duration::from_millis(10));
        }
        let started = Instant::now();
        let _state = session.state.lock().expect("state");
        let elapsed = started.elapsed();
        drop(_state);
        assert!(
            elapsed < Duration::from_millis(500),
            "chat state waited {elapsed:?} on the session file"
        );
        assert_eq!(worker.join().unwrap(), Ok(Reply::whole("Hello")));
        session.shutdown();
    }

    /// One child serves every Instance (ADR-0008), so character B's wake takes
    /// character A's turn without A's own slot ever having been superseded.
    /// Nothing raised A's abandon flag, so A took a wake that read as broken.
    #[test]
    fn a_turn_taken_for_another_instance_is_recorded_as_withdrawn() {
        let (fx, session) = Fixture::new("slow");
        let session = Arc::new(session.with_timeout(Duration::from_secs(10)));
        let worker = {
            let session = Arc::clone(&session);
            thread::spawn(move || session.complete(&asking("hi"), &|_| {}))
        };
        assert!(fx.wait_for("prompt", 1), "the first turn never went out");
        let poke = WakeRequest {
            instance: "buddy-2".to_string(),
            ..asking("again")
        };
        assert_eq!(session.complete(&poke, &|_| {}), Ok(Reply::whole("Hello")));
        assert!(worker.join().unwrap().is_err(), "the turn was not taken");

        // The loser's own line, written while it still held the lock, so it
        // comes before the winner's.
        let turns = fx.events("turn");
        assert_eq!(turns[0]["stop"], json!("cancelled"), "{turns:?}");
        assert_eq!(turns[0]["withdrawn_for"], json!("buddy-2"), "{turns:?}");

        // And what `note_parsed` writes for the wake that lost the session.
        // Called here rather than through the Shell's entry point, which reads
        // the process-global attached session.
        let withdrawn = session.claim_withdrawn_wake("buddy-1");
        let parsed = parsed_fields(
            "buddy-1",
            &Wake::Failed,
            true,
            None,
            None,
            withdrawn.as_deref(),
        );
        assert_eq!(parsed["result"], json!("withdrawn"), "{parsed}");
        assert_eq!(parsed["withdrawn_for"], json!("buddy-2"), "{parsed}");
        // One wake, one withdrawal. The next wake this Instance takes is its
        // own however that one ends.
        assert_eq!(session.claim_withdrawn_wake("buddy-1"), None);
        session.shutdown();
    }

    /// Saving an Instance Prompt cancels the reply in flight as well as
    /// tearing the conversation down (ADR-0012). Dropping the conversation
    /// alone leaves the old ACP session generating on a replaced transcript.
    #[test]
    fn saving_an_instance_prompt_cancels_that_instances_turn() {
        let (fx, session) = Fixture::new("slow");
        let session = Arc::new(session.with_timeout(Duration::from_secs(10)));
        let worker = {
            let session = Arc::clone(&session);
            thread::spawn(move || session.complete(&asking("hi"), &|_| {}))
        };
        assert!(fx.wait_for("prompt", 1), "the turn never went out");
        session.drop_conversation("buddy-1");
        assert!(
            fx.wait_for("cancel", 1),
            "the saved-over turn was not cancelled"
        );
        let stopped = worker.join().unwrap().unwrap_err();
        assert!(stopped.contains("cancelled"), "{stopped}");
        // A withdrawal, not a fault. The turn line says who the turn was given
        // up for, and the Chat surface is told nothing broke.
        let turns = fx.events("turn");
        assert_eq!(turns[0]["stop"], json!("cancelled"), "{turns:?}");
        assert_eq!(turns[0]["withdrawn_for"], json!("buddy-1"), "{turns:?}");
        assert_eq!(session.inspect().last_error, None);
        assert_eq!(
            session.claim_withdrawn_wake("buddy-1").as_deref(),
            Some("buddy-1")
        );
        session.shutdown();
    }

    /// One child serves every Instance (ADR-0008), so a save that cancelled
    /// whatever held the turn would stop character B mid-reply because character A
    /// edited a prompt B has nothing to do with.
    #[test]
    fn saving_one_instances_prompt_leaves_another_instances_turn_alone() {
        let (fx, session) = Fixture::new("slow");
        let session = Arc::new(session.with_timeout(Duration::from_secs(10)));
        let worker = {
            let session = Arc::clone(&session);
            thread::spawn(move || session.complete(&asking("hi"), &|_| {}))
        };
        assert!(fx.wait_for("prompt", 1), "the turn never went out");
        session.drop_conversation("buddy-2");
        // Long enough for a cancel to have reached the child and been
        // recorded. The one below is recorded well inside this wait.
        thread::sleep(Duration::from_millis(250));
        assert_eq!(
            fx.count("cancel"),
            0,
            "another Instance's turn was cancelled"
        );
        assert!(!worker.is_finished(), "another Instance's turn was ended");
        // And the same save for the Instance that does hold the turn ends it,
        // so the assertions above are about who saved rather than about a
        // cancel that never works.
        session.drop_conversation("buddy-1");
        assert!(
            fx.wait_for("cancel", 1),
            "the owning Instance's turn survived"
        );
        assert!(
            worker.join().unwrap().is_err(),
            "the turn was not cancelled"
        );
        session.shutdown();
    }

    /// The withdrawal a later supersede must not erase. The `parsed` line is
    /// written on the Shell side, frames after the turn ended, so a third
    /// wake may already have taken the session from a fourth.
    #[test]
    fn a_later_supersede_does_not_erase_an_unclaimed_withdrawal() {
        let dir = std::env::temp_dir().join(format!("fidget-harness-{}", uuid::Uuid::new_v4()));
        let launch = Launch {
            name: "nope".into(),
            argv: vec!["/nonexistent/fidget-no-such-harness".into()],
        };
        std::fs::create_dir_all(&dir).unwrap();
        let session = isolated_session(launch, dir.clone(), silent());

        session.note_withdrawal(Some("buddy-2".to_string()));
        assert_eq!(
            session
                .claim_withdrawn_turn("buddy-1", CANCELLED)
                .as_deref(),
            Some("buddy-2")
        );

        // buddy-3 takes it from someone else, and the handover it does not win
        // clears the pending slot. Neither is buddy-1's.
        session.note_withdrawal(Some("buddy-3".to_string()));
        session.note_withdrawal(None);

        let withdrawn = session.claim_withdrawn_wake("buddy-1");
        assert_eq!(withdrawn.as_deref(), Some("buddy-2"));
        let parsed = parsed_fields(
            "buddy-1",
            &Wake::Failed,
            true,
            None,
            None,
            withdrawn.as_deref(),
        );
        assert_eq!(parsed["result"], json!("withdrawn"), "{parsed}");
        assert_eq!(
            session.claim_withdrawn_wake("buddy-1"),
            None,
            "claimed twice"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The one wake that waits instead. A proactive wake cancelling the Poke
    /// it arrived behind would be worse than the refusal.
    #[test]
    fn a_proactive_wake_never_cancels_a_reactive_turn() {
        let (fx, session) = Fixture::new("slow");
        let session = Arc::new(session.with_timeout(Duration::from_secs(3)));
        let worker = {
            let session = Arc::clone(&session);
            thread::spawn(move || session.complete(&asking("hi"), &|_| {}))
        };
        assert!(fx.wait_for("prompt", 1), "the first turn never went out");
        assert_eq!(
            session.complete(&proactive("again"), &|_| {}),
            Err("harness busy".to_string())
        );
        // Read before the reactive turn's own timeout cancel, which is the
        // only other thing that would put a `cancel` on this wire.
        assert_eq!(fx.count("cancel"), 0, "the Poke was cancelled for a tick");
        // Stop the fake child before joining. The assertion above proves the
        // refusal while the reactive turn is live; waiting for its timeout
        // would add the full three-second budget to this unit test.
        session.shutdown();
        let _ = worker.join();

        // The refused wake sent no prompt, so without a line of its own the
        // `parsed` line the Shell writes for it would read against the prompt
        // the wake before it logged.
        let refused = fx.events("refused");
        assert_eq!(refused.len(), 1, "{refused:?}");
        assert_eq!(refused[0]["instance"], json!("buddy-1"));
        assert_eq!(refused[0]["wake"], json!("proactive"));
        assert_eq!(refused[0]["why"], json!("harness busy"));
        assert_eq!(fx.events("prompt").len(), 1, "the refused wake sent none");
    }

    /// The probe's two exit codes are its two phases. 1 is a turn that did
    /// not finish, whatever the reply text turns out to be, and 2 is never
    /// having asked.
    #[test]
    fn the_probe_exits_zero_on_a_turn_one_on_a_refusal_and_two_on_no_attach() {
        let (_fx, session) = Fixture::new("happy");
        assert_eq!(probe(&session), 0);
        session.shutdown();

        let (_fx, session) = Fixture::new("refusal");
        assert_eq!(probe(&session), 1);
        session.shutdown();

        let (_fx, session) = Fixture::new("auth");
        assert_eq!(probe(&session), 2);
        session.shutdown();

        let dir = std::env::temp_dir().join(format!("fidget-probe-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let launch = Launch {
            name: "nope".into(),
            argv: vec!["/nonexistent/fidget-no-such-harness".into()],
        };
        assert_eq!(probe(&isolated_session(launch, dir.clone(), silent())), 2);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The probe hands a Harness that advertised `mcpCapabilities.http` the
    /// same loopback URL the app would. Without a listener of its own,
    /// `choose_mcp` fell through to a stdio shim with nothing to dial (#984).
    #[test]
    fn the_probe_serves_the_loopback_endpoint_before_it_attaches() {
        let (fx, session) = Fixture::new("happy");
        assert_eq!(probe(&session), 0);
        session.shutdown();
        assert_eq!(
            fx.count("mcp=http"),
            1,
            "the fake advertised http and got the shim"
        );
        assert!(crate::mcp_http::endpoint().is_some());
    }

    /// `shutdown` kills and waits; a zero-length wait can only succeed if
    /// that wait already finished.
    #[test]
    fn the_probe_waits_for_the_child_to_be_reaped() {
        let (_fx, session) = Fixture::new("happy");
        assert_eq!(
            session.complete(&asking("hi"), &|_| {}),
            Ok(Reply::whole("Hello"))
        );
        let wire = session.current_wire().expect("attached");
        session.shutdown();
        assert!(
            wire.wait_for_exit(Duration::ZERO),
            "the wire thread outlived the reap"
        );
    }

    /// A button is an agent method with a real id. A terminal method's id, args,
    /// and env are a credential path and never ride along on the offer.
    #[test]
    fn sign_in_buttons_keep_agent_ids_and_drop_terminal_secrets() {
        use agent_client_protocol::schema::v1::{AuthMethod, AuthMethodAgent, AuthMethodTerminal};
        use std::collections::HashMap;

        let mut env = HashMap::new();
        env.insert("TOKEN".to_string(), "super-secret".to_string());
        let terminal = AuthMethod::Terminal(
            AuthMethodTerminal::new("term", "Outside")
                .args(vec!["--not-stored".to_string()])
                .env(env),
        );
        let agent = AuthMethod::Agent(AuthMethodAgent::new("chatgpt", "ChatGPT"));
        let offers = vec![
            crate::acp_wire::auth_offer(&agent),
            crate::acp_wire::auth_offer(&terminal),
        ];
        let actions = crate::acp_wire::sign_in_button(true, &offers);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].id.as_str(), "chatgpt");
        assert_eq!(actions[0].label, "ChatGPT");
        let debug = format!("{offers:?}");
        assert!(!debug.contains("--not-stored"), "{debug}");
        assert!(!debug.contains("super-secret"), "{debug}");
        // `Terminal` is the variant name, so the id `term` is what remains.
        assert!(!debug.replace("Terminal", "").contains("term"), "{debug}");

        assert!(crate::acp_wire::sign_in_button(false, &offers).is_empty());
        assert!(crate::acp_wire::sign_in_button(true, &[]).is_empty());
        assert_eq!(
            login_command("claude", &Handshake::default()),
            "claude /login"
        );

        let pi = Handshake {
            auth_methods: vec![crate::acp_wire::auth_offer(&terminal)],
            ..Default::default()
        };
        assert_eq!(
            login_command("pi", &pi),
            "npx -y pi-acp@latest --terminal-login"
        );
        assert!(crate::acp_wire::sign_in_button(true, &pi.auth_methods).is_empty());

        let blank = AuthMethod::Agent(AuthMethodAgent::new(" ", "ChatGPT"));
        assert!(
            crate::acp_wire::sign_in_button(true, &[crate::acp_wire::auth_offer(&blank)])
                .is_empty()
        );

        let two = [
            crate::acp_wire::auth_offer(&AuthMethod::Agent(AuthMethodAgent::new(
                "chatgpt", "ChatGPT",
            ))),
            crate::acp_wire::auth_offer(&AuthMethod::Agent(AuthMethodAgent::new(
                "apikey", "API Key",
            ))),
        ];
        let actions = crate::acp_wire::sign_in_button(true, &two);
        assert_eq!(
            actions
                .iter()
                .map(|action| (action.id.as_str(), action.label.as_str()))
                .collect::<Vec<_>>(),
            vec![("chatgpt", "ChatGPT"), ("apikey", "API Key")]
        );

        let codex = [
            crate::acp_wire::auth_offer(&AuthMethod::Agent(AuthMethodAgent::new(
                "api-key", "API Key",
            ))),
            crate::acp_wire::auth_offer(&AuthMethod::Agent(AuthMethodAgent::new(
                "chat-gpt", "ChatGPT",
            ))),
        ];
        let actions = crate::acp_wire::sign_in_button(true, &codex);
        assert_eq!(
            actions
                .iter()
                .map(|action| (action.id.as_str(), action.label.as_str()))
                .collect::<Vec<_>>(),
            vec![("chat-gpt", "ChatGPT")]
        );
        assert_eq!(
            login_command(
                "codex",
                &Handshake {
                    auth_methods: codex.to_vec(),
                    ..Default::default()
                }
            ),
            "codex login"
        );

        // What agy_acp_server 1.2.1 advertised. The two Google logins open the
        // browser from `authenticate`; the key and cloud methods need what
        // this app never sends.
        let antigravity = [
            ("oauth-personal", "Log in with Google"),
            ("oauth-business", "Log in with Gemini Enterprise"),
            ("gemini-api-key", "Gemini API key"),
            ("agent-platform", "Gemini Enterprise Agent Platform"),
        ]
        .map(|(id, name)| {
            crate::acp_wire::auth_offer(&AuthMethod::Agent(AuthMethodAgent::new(id, name)))
        });
        let actions = crate::acp_wire::sign_in_button(true, &antigravity);
        assert_eq!(
            actions
                .iter()
                .map(|action| (action.id.as_str(), action.label.as_str()))
                .collect::<Vec<_>>(),
            vec![
                ("oauth-personal", "Log in with Google"),
                ("oauth-business", "Log in with Gemini Enterprise"),
            ]
        );
    }

    #[test]
    fn login_command_takes_the_table_for_a_named_harness_and_the_handshake_for_a_custom_one() {
        let hint = |description: Option<&str>| Handshake {
            auth_methods: vec![crate::acp_wire::AuthOffer::Unrecognized {
                name: "ChatGPT".into(),
                description: description.map(str::to_string),
            }],
            ..Default::default()
        };
        assert_eq!(
            login_command("codex", &hint(Some("Sign in with ChatGPT"))),
            "codex login"
        );
        assert_eq!(
            login_command("x", &hint(Some("run x login"))),
            "run x login"
        );
        assert_eq!(login_command("x", &hint(None)), "ChatGPT");
        assert_eq!(
            login_command("x", &Handshake::default()),
            "x (run it once in a terminal and sign in)"
        );
    }

    /// Chat's Connect button answers with this line instead of spawning the
    /// login itself, and it answers at the moment of the pick, before any
    /// handshake. A named row without one would leave that answer a shrug.
    #[test]
    fn every_named_harness_documents_a_login_for_the_users_own_terminal() {
        for name in [
            "claude",
            "codex",
            "copilot",
            "cursor-agent",
            "goose",
            "grok",
            "hermes",
            "opencode",
            "pi",
            "antigravity",
        ] {
            assert!(launch(Some(name)).is_some(), "{name} is not a named row");
            let hint = login_hint(name);
            assert!(
                !hint.contains("run it once"),
                "{name} falls through to the unnamed hint: {hint}"
            );
            assert_eq!(hint, login_command(name, &Handshake::default()));
        }
    }

    /// `scripts/scenarios/question-bubble.sh` fixture. The first turn asks for
    /// permission and the Harness waits on the answer, putting the Instance in
    /// the awaiting-user state. The mark proves the ask is sent.
    #[test]
    fn scenario_asking_fixture_asks_for_permission() {
        let (fx, session) = Fixture::new("scenario-asking");
        let session = Arc::new(session);
        let id = WOKEN.to_string();
        let mut slots = crate::completer::Slots::new();
        slots.wake(
            &id,
            harness_director(&session),
            woken(Happened::Chat("hi".into())),
        );
        let ask = fx.ask();
        assert_eq!(
            fx.count("asked"),
            1,
            "scenario-asking recorded the ask mark"
        );
        assert_eq!(ask.title.as_deref(), Some("May I proceed?"));
        session.answer_permission(&ask.request, "allow");
        let answered = polled(&mut slots).expect("the first turn's answer");
        assert_eq!(said(&answered), Some("fidget\nYou answered the question."));
        assert_eq!(fx.count("replied"), 1);
        session.shutdown();
    }

    fn mcp_tmp(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "fidget-mcp-launch-{label}-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(path: &Path) {
        std::fs::write(path, []).unwrap();
    }

    fn sidecar(dir: &Path) -> PathBuf {
        dir.join(if cfg!(windows) {
            "fidget-mcp.exe"
        } else {
            "fidget-mcp"
        })
    }

    #[test]
    fn mcp_launch_prefers_an_env_file_over_a_sibling() {
        let dir = mcp_tmp("env-wins");
        let env_bin = dir.join("from-env");
        let current_exe = dir.join("fidget");
        touch(&env_bin);
        touch(&sidecar(&dir));
        touch(&current_exe);

        let launch = mcp_launch(Some(env_bin.as_path()), &current_exe).expect("env file wins");
        assert_eq!(launch.path, env_bin);
        assert!(launch.args.is_empty());

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn mcp_launch_falls_through_to_a_sibling_when_env_is_missing() {
        let dir = mcp_tmp("sibling");
        let current_exe = dir.join("fidget");
        let sibling = sidecar(&dir);
        touch(&sibling);
        touch(&current_exe);

        let launch = mcp_launch(None, &current_exe).expect("sibling");
        assert_eq!(launch.path, sibling);
        assert!(launch.args.is_empty());

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn mcp_launch_runs_the_app_binary_as_stdio_when_nothing_is_beside_it() {
        let dir = mcp_tmp("stdio");
        let current_exe = dir.join("fidget");
        touch(&current_exe);

        let launch = mcp_launch(None, &current_exe).expect("app binary");
        assert_eq!(launch.path, current_exe);
        assert_eq!(launch.args, ["--mcp-stdio"]);

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn mcp_launch_returns_none_when_the_exe_is_not_the_app() {
        let dir = mcp_tmp("test-bin");
        let current_exe = dir.join("harness-unit-tests");
        touch(&current_exe);

        assert_eq!(mcp_launch(None, &current_exe), None);

        let _ = std::fs::remove_dir_all(dir);
    }

    /// ADR-0023's branch. The loopback server for a Harness whose ACP
    /// `initialize` advertised `mcpCapabilities.http`, and never for one that
    /// did not. A future Hermes with that bit set would get the HTTP server directly.
    #[test]
    fn the_loopback_server_goes_only_to_a_harness_that_advertised_http_mcp() {
        let (calls, _held) = mpsc::channel();
        let endpoint = crate::mcp_http::serve(calls).expect("loopback binds");

        let http = Handshake {
            mcp_http: true,
            ..Handshake::default()
        };
        match mcp_server(&http) {
            Some(McpChoice::Http { url, authorization }) => {
                assert_eq!(url, endpoint.url);
                assert!(authorization.starts_with("Bearer "));
            }
            other => panic!(
                "expected the loopback server, got {:?}",
                other.map(|c| c.label())
            ),
        }

        let stdio_only = Handshake::default();
        assert!(
            !matches!(mcp_server(&stdio_only), Some(McpChoice::Http { .. })),
            "a Harness whose initialize advertises no mcpCapabilities.http is never handed a URL"
        );
    }

    /// The stdio server is a shim that dials the app, so the choice has to
    /// carry the endpoint the shim reads, in the environment, never in the
    /// line the Action Log takes.
    #[test]
    fn the_stdio_server_is_handed_the_endpoint_in_its_environment() {
        let (calls, _held) = mpsc::channel();
        let endpoint = crate::mcp_http::serve(calls).expect("loopback binds");

        let sidecar = McpLaunch {
            path: PathBuf::from("/opt/fidget-mcp"),
            args: Vec::new(),
            env: Vec::new(),
        };
        let Some(McpChoice::Stdio(launch)) = choose_mcp(
            &Handshake::default(),
            Some(endpoint.clone()),
            Some(sidecar.clone()),
        ) else {
            panic!("no stdio server to hand over");
        };
        let value = |name: &str| {
            launch
                .env
                .iter()
                .find(|(var, _)| var == name)
                .map(|(_, value)| value.clone())
                .unwrap_or_else(|| panic!("{name} is not in the environment"))
        };
        assert_eq!(value(fidget_mcp_server::URL_VAR), endpoint.url);
        assert_eq!(
            format!("Bearer {}", value(fidget_mcp_server::TOKEN_VAR)),
            endpoint.authorization()
        );
        assert!(
            !launch.line().contains(&value(fidget_mcp_server::TOKEN_VAR)),
            "the token reached the line the Action Log takes"
        );

        // No app serving loopback is a shim that fails visibly, not one
        // dialling an endpoint it invented.
        let Some(McpChoice::Stdio(unreachable)) =
            choose_mcp(&Handshake::default(), None, Some(sidecar))
        else {
            panic!("no stdio server to hand over");
        };
        assert!(unreachable.env.is_empty());
    }

    /// The preset and a custom line both spell the binary's name; `hermes`
    /// takes its servers from `session/new` and needs no file.
    #[test]
    fn only_a_cursor_agent_launch_takes_the_cursor_config() {
        assert!(takes_cursor_config(&launch(Some("cursor-agent")).unwrap()));
        assert!(takes_cursor_config(
            &launch(Some("cursor-agent acp --model gpt")).unwrap()
        ));
        assert!(!takes_cursor_config(&launch(Some("hermes")).unwrap()));
    }

    /// Probe a missing launcher.
    #[test]
    fn probe_missing_launcher() {
        let launch = Launch {
            name: "nope".into(),
            argv: vec!["/nonexistent/fidget-no-such-harness".into()],
        };
        match probe_launcher(&launch) {
            ProbeOutcome::NotFound => {}
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    /// Probe a launcher that exits nonzero.
    #[test]
    fn probe_launcher_exits_nonzero() {
        let launch = Launch {
            name: "false".into(),
            argv: vec!["false".into()],
        };
        match probe_launcher(&launch) {
            ProbeOutcome::Unhealthy(failure) => {
                assert_eq!(failure.command.as_deref(), Some("false --version"));
                assert!(failure.reason.starts_with("exited with"), "{failure:?}");
            }
            other => panic!("expected Unhealthy, got {other:?}"),
        }
    }

    /// Probe a healthy launcher.
    #[test]
    fn probe_healthy_launcher() {
        let launch = Launch {
            name: "echo".into(),
            argv: vec!["echo".into()],
        };
        match probe_launcher(&launch) {
            ProbeOutcome::Healthy => {}
            other => panic!("expected Healthy, got {other:?}"),
        }
    }

    /// npx presets cite npx itself as their probe command.
    #[test]
    fn npx_presets_probe_npx() {
        for preset in ["claude", "codex", "pi"] {
            let launch = launch(Some(preset)).unwrap();
            assert_eq!(launch.version_flag(), "--version");
            assert_eq!(launch.argv[0], "npx", "preset {preset} should probe npx");
        }
    }

    /// First-party CLIs cite their own binary.
    #[test]
    fn first_party_clis_probe_themselves() {
        for preset in [
            "copilot",
            "cursor-agent",
            "grok",
            "goose",
            "hermes",
            "opencode",
        ] {
            let launch = launch(Some(preset)).unwrap();
            assert_eq!(launch.version_flag(), "--version");
            assert_eq!(
                launch.argv[0], preset,
                "preset {preset} should probe its own binary"
            );
        }
    }

    /// antigravity probes with --help instead of --version.
    #[test]
    fn antigravity_probes_with_help() {
        let launch = launch(Some("antigravity")).unwrap();
        assert_eq!(launch.version_flag(), "--help");
    }

    /// Probe timeout produces an unhealthy outcome.
    #[test]
    fn probe_timeout() {
        let dir = std::env::temp_dir().join(format!("fidget-probe-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();

        #[cfg(unix)]
        let script = {
            use std::os::unix::fs::PermissionsExt;
            let path = dir.join("slow-launcher.sh");
            std::fs::write(&path, "#!/bin/sh\nsleep 2\n").unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            path
        };

        #[cfg(windows)]
        let script = {
            let path = dir.join("slow-launcher.bat");
            // Not `timeout`: CI's PATH finds GNU coreutils' first, and Windows'
            // own exits at once when stdin is redirected, as the probe's is.
            std::fs::write(&path, "@echo off\nping -n 3 127.0.0.1 >nul\n").unwrap();
            path
        };

        let launch = Launch {
            name: "timeout-fixture".into(),
            argv: vec![script.to_string_lossy().to_string()],
        };
        match probe_launcher_within(&launch, Duration::from_millis(100)) {
            ProbeOutcome::Unhealthy(failure) => {
                assert_eq!(failure.reason, "timed out after 0.1s");
            }
            other => panic!("expected Unhealthy from timeout, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Probe with no output produces an unhealthy outcome.
    #[test]
    fn probe_no_output() {
        let dir = std::env::temp_dir().join(format!("fidget-probe-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();

        #[cfg(unix)]
        let script = {
            use std::os::unix::fs::PermissionsExt;
            let path = dir.join("no-output.sh");
            std::fs::write(&path, "#!/bin/sh\nexit 0\n").unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            path
        };

        #[cfg(windows)]
        let script = {
            let path = dir.join("no-output.bat");
            std::fs::write(&path, "@echo off\nexit /b 0\n").unwrap();
            path
        };

        let launch = Launch {
            name: "no-output".into(),
            argv: vec![script.to_string_lossy().to_string()],
        };
        // Its own new script queues at the same first-exec gate, so the budget
        // is generous. A timeout here is not what this test asserts.
        match probe_launcher_within(&launch, Duration::from_secs(30)) {
            ProbeOutcome::Unhealthy(failure) => {
                assert_eq!(failure.reason, "produced no output");
                assert_eq!(failure.output, "");
            }
            other => panic!("expected Unhealthy from no output, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A wake after a refused probe must not start the adapter. That spawn is
    /// the hang the probe is there to stop.
    #[test]
    fn an_unhealthy_probe_does_not_let_a_wake_spawn_the_adapter() {
        let dir = std::env::temp_dir().join(format!("fidget-probe-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let marker = dir.join("spawned");
        #[cfg(unix)]
        let script = {
            use std::os::unix::fs::PermissionsExt;
            let path = dir.join("hang.sh");
            std::fs::write(
                &path,
                format!("#!/bin/sh\ntouch \"{}\"\nexit 0\n", marker.display()),
            )
            .unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            path
        };
        #[cfg(windows)]
        let script = {
            let path = dir.join("hang.bat");
            std::fs::write(
                &path,
                format!(
                    "@echo off\ntype nul > \"{}\"\nexit /b 0\n",
                    marker.display()
                ),
            )
            .unwrap();
            path
        };
        let session = Session::new(
            Launch {
                name: "claude".into(),
                argv: vec![script.to_string_lossy().to_string()],
            },
            Ok(AttachCwd(dir.clone())),
            SessionDataDir::at(dir.clone()),
            silent(),
        );
        let failure = LaunchFailure {
            command: Some("npx --version".to_string()),
            reason: "timed out after 3.0s".to_string(),
            output: String::new(),
            node_check: None,
        };
        session.update_inspect(|inspect| inspect.unhealthy = Some(failure));
        let err = session.complete(&asking("hi"), &|_| {}).unwrap_err();
        assert_eq!(err, "`npx --version` timed out after 3.0s.");
        assert!(
            !marker.exists(),
            "the wake spawned the adapter after preflight refused it"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `Command` looks for `npx.exe`. npm installs `npx.cmd`, so the launcher
    /// has to name that file or every Windows run is `NotFound`.
    #[cfg(windows)]
    #[test]
    fn a_cmd_stand_in_is_the_program_when_path_is_only_the_fixture() {
        let dir = std::env::temp_dir().join(format!("fidget-cmd-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("npx.cmd"), "@echo off\r\n").unwrap();
        std::fs::write(dir.join("npx.exe"), "").unwrap();
        assert_eq!(
            windows_program("npx", Some(&dir)).as_deref(),
            Some(dir.join("npx.exe").as_os_str())
        );
        std::fs::remove_file(dir.join("npx.exe")).unwrap();
        assert_eq!(
            windows_program("npx", Some(&dir)).as_deref(),
            Some(dir.join("npx.cmd").as_os_str())
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
