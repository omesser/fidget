//! The one `fidget` server in `<cwd>/.cursor/mcp.json`, and the approval
//! that makes `cursor-agent` load it.
//!
//! `cursor-agent acp` ignores the `mcpServers` handed over on `session/new`
//! and loads servers only from an approved `mcp.json` (#1020). Reaching it
//! takes two mechanisms because Cursor splits the job in two, and neither
//! half can do the other's:
//!
//! - `mcp.json` is the only place a server can be *defined*. `cursor-agent
//!   mcp` offers `login`, `list`, `list-tools`, `enable` and `disable`, and
//!   no `add`.
//! - `cursor-agent mcp enable` is the only way to *approve* one. Its own help
//!   calls it "Add an MCP server to the local approved list" for a server
//!   already "configured in .cursor/mcp.json or ~/.cursor/mcp.json".
//!
//! So attach writes the file and then runs the CLI. That split is Cursor's,
//! not ours.
//!
//! The entry is the same loopback URL and bearer token every other Harness
//! gets (ADR-0023), which is what keeps this working on every platform fidget
//! ships. Both are new every app run, so the entry is rewritten and re-approved
//! on each attach rather than set up once. Rewriting is skipped when the entry
//! is already what we would write, but `enable` runs every attach regardless,
//! ~380ms. Approving twice is harmless: an unchanged entry hashes to the id
//! Cursor already holds, so no second approval is recorded.
//!
//! The token is why the file is 0600 and why `remove` runs on detach: it is
//! live credential for as long as the session is, and a stale copy authorises
//! nothing once the app restarts.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const SERVER: &str = "fidget";
const FILE: &str = "mcp.json";
const CLI_FILE: &str = "cli.json";
const TOOL_PERMISSION: &str = "Mcp(fidget:*)";
const DIR: &str = ".cursor";

/// `cursor-agent mcp enable` takes 380ms measured; ten seconds is the point
/// past which it is hung, and the attach goes on without it.
const ENABLE_TIMEOUT: Duration = Duration::from_secs(10);

/// What `install` changed, so `remove` undoes exactly that and nothing of the
/// user's.
#[derive(Debug)]
pub struct Installed {
    file: PathBuf,
    created_dir: bool,
    created_file: bool,
    permission: Option<PermissionGrant>,
}

#[derive(Debug)]
struct PermissionGrant {
    file: PathBuf,
    created_file: bool,
    created_permissions: bool,
    created_allow: bool,
    added: bool,
}

/// Merge the `fidget` server into `<cwd>/.cursor/mcp.json` beside whatever
/// the user already has there. A file that does not parse is an error and is
/// not written over.
pub fn install(cwd: &Path, url: &str, authorization: &str) -> Result<Installed, String> {
    let dir = cwd.join(DIR);
    let file = dir.join(FILE);
    let created_dir = !dir.exists();
    let (mut root, created_file) = match fs::read_to_string(&file) {
        Ok(text) => (parse(&file, &text)?, false),
        Err(error) if error.kind() == ErrorKind::NotFound => (json!({}), true),
        Err(error) => return Err(format!("{}: {error}", file.display())),
    };
    let before = root.clone();
    let servers = root
        .as_object_mut()
        .expect("parse checked the root is an object")
        .entry("mcpServers")
        .or_insert_with(|| json!({}));
    let servers = servers
        .as_object_mut()
        .ok_or_else(|| format!("{}: mcpServers is not an object", file.display()))?;
    servers.insert(
        SERVER.to_string(),
        json!({"url": url, "headers": {"Authorization": authorization}}),
    );
    if root != before {
        fs::create_dir_all(&dir).map_err(|error| format!("{}: {error}", dir.display()))?;
        write(&file, &root)?;
    }
    restrict(&file)?;
    Ok(Installed {
        file,
        created_dir,
        created_file,
        permission: None,
    })
}

fn parse(file: &Path, text: &str) -> Result<Value, String> {
    let root: Value = serde_json::from_str(text)
        .map_err(|_| format!("{} is not JSON, so it was left alone", file.display()))?;
    if !root.is_object() {
        return Err(format!(
            "{} is not an object, so it was left alone",
            file.display()
        ));
    }
    Ok(root)
}

fn write(path: &Path, value: &Value) -> Result<(), String> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    fs::write(path, bytes).map_err(|error| format!("{}: {error}", path.display()))
}

/// Owner-only, because the entry holds this run's bearer token. Windows has no
/// mode bits to set here and the file keeps the project directory's ACL, which
/// docs/harness.md names as the gap it is.
#[cfg(unix)]
fn restrict(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|error| format!("{}: {error}", path.display()))
}

#[cfg(not(unix))]
fn restrict(_path: &Path) -> Result<(), String> {
    Ok(())
}

/// `<cli> mcp enable fidget` in `cwd`. Approvals are read once per
/// `cursor-agent` process, so this runs before the `acp` spawn, every time.
pub fn enable(cli: &Path, cwd: &Path) -> Result<(), String> {
    let line = format!("`{} mcp enable {SERVER}`", cli.display());
    let mut command = Command::new(cli);
    command
        .args(["mcp", "enable", SERVER])
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    crate::harness::without_console_window(&mut command);
    let mut child = command
        .spawn()
        .map_err(|error| format!("{line}: {error}"))?;
    let deadline = Instant::now() + ENABLE_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => return Err(format!("{line} exited with {status}")),
            Ok(None) => {}
            Err(error) => return Err(format!("{line}: {error}")),
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "{line} did not finish in {}s",
                ENABLE_TIMEOUT.as_secs()
            ));
        }
        thread::sleep(Duration::from_millis(50));
    }
}

impl Installed {
    /// Cursor's CLI permission rule approves calls to Fidget's server while
    /// leaving the user's global configuration and other project rules alone.
    pub fn approve_fidget_tools(&mut self) -> Result<(), String> {
        if self.permission.is_some() {
            return Ok(());
        }
        let file = self.file.with_file_name(CLI_FILE);
        let (mut root, created_file) = match fs::read_to_string(&file) {
            Ok(text) => (parse(&file, &text)?, false),
            Err(error) if error.kind() == ErrorKind::NotFound => (json!({}), true),
            Err(error) => return Err(format!("{}: {error}", file.display())),
        };
        let object = root.as_object_mut().expect("parse checked");
        let created_permissions = !object.contains_key("permissions");
        let permissions = object
            .entry("permissions")
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .ok_or_else(|| format!("{}: permissions is not an object", file.display()))?;
        let created_allow = !permissions.contains_key("allow");
        let allow = permissions
            .entry("allow")
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .ok_or_else(|| format!("{}: permissions.allow is not an array", file.display()))?;
        let added = !allow
            .iter()
            .any(|item| item.as_str() == Some(TOOL_PERMISSION));
        if added {
            allow.push(json!(TOOL_PERMISSION));
            write(&file, &root)?;
        }
        self.permission = Some(PermissionGrant {
            file,
            created_file,
            created_permissions,
            created_allow,
            added,
        });
        Ok(())
    }

    /// Undo `install`. Nothing here fails loudly: a quit is not the moment
    /// to refuse, so each step logs and the next still runs.
    ///
    /// `cursor-agent mcp disable` is not called. Measured, it prunes no
    /// approval and instead marks the server never to load again, which would
    /// break the next attach.
    pub fn remove(self) {
        if let Some(permission) = &self.permission {
            if let Err(why) = permission.remove() {
                eprintln!("harness: cursor mcp: {why}");
            }
        }
        if let Err(why) = self.remove_entry() {
            eprintln!("harness: cursor mcp: {why}");
        }
        if self.created_dir {
            if let Some(dir) = self.file.parent() {
                // Refuses a directory that is not empty, which is the point.
                let _ = fs::remove_dir(dir);
            }
        }
    }

    fn remove_entry(&self) -> Result<(), String> {
        let text = match fs::read_to_string(&self.file) {
            Ok(text) => text,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(format!("{}: {error}", self.file.display())),
        };
        let mut root = parse(&self.file, &text)?;
        let before = root.clone();
        let object = root.as_object_mut().expect("parse checked");
        if let Some(servers) = object.get_mut("mcpServers").and_then(Value::as_object_mut) {
            servers.remove(SERVER);
            if servers.is_empty() {
                object.remove("mcpServers");
            }
        }
        if object.is_empty() && self.created_file {
            return fs::remove_file(&self.file)
                .map_err(|error| format!("{}: {error}", self.file.display()));
        }
        if root != before {
            write(&self.file, &root)?;
        }
        Ok(())
    }
}

impl PermissionGrant {
    fn remove(&self) -> Result<(), String> {
        if !self.added {
            return Ok(());
        }
        let text = match fs::read_to_string(&self.file) {
            Ok(text) => text,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(format!("{}: {error}", self.file.display())),
        };
        let mut root = parse(&self.file, &text)?;
        let object = root.as_object_mut().expect("parse checked");
        let Some(permissions) = object.get_mut("permissions").and_then(Value::as_object_mut) else {
            return Ok(());
        };
        let Some(allow) = permissions.get_mut("allow").and_then(Value::as_array_mut) else {
            return Ok(());
        };
        let Some(index) = allow
            .iter()
            .position(|item| item.as_str() == Some(TOOL_PERMISSION))
        else {
            return Ok(());
        };
        allow.remove(index);
        if self.created_allow && allow.is_empty() {
            permissions.remove("allow");
        }
        if self.created_permissions && permissions.is_empty() {
            object.remove("permissions");
        }
        if self.created_file && object.is_empty() {
            fs::remove_file(&self.file).map_err(|error| format!("{}: {error}", self.file.display()))
        } else {
            write(&self.file, &root)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const URL: &str = "http://127.0.0.1:54321/mcp";
    const AUTH: &str = "Bearer per-run-token";

    fn dir(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("fidget-cursor-mcp-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("temp dir");
        path
    }

    fn read(path: &Path) -> Value {
        serde_json::from_str(&fs::read_to_string(path).expect("the file")).expect("JSON")
    }

    #[test]
    fn scoped_cli_permission_is_added_and_removed_with_a_fresh_attach() {
        let cwd = dir("permission-fresh");
        let mut installed = install(&cwd, URL, AUTH).expect("installed");
        installed.approve_fidget_tools().expect("approved");

        let file = cwd.join(DIR).join(CLI_FILE);
        assert_eq!(
            read(&file),
            json!({"permissions": {"allow": ["Mcp(fidget:*)"]}})
        );

        installed.remove();
        assert!(!cwd.join(DIR).exists(), ".cursor was left behind");
    }

    #[test]
    fn scoped_cli_permission_preserves_other_rules_and_user_edits() {
        let cwd = dir("permission-existing");
        let file = cwd.join(DIR).join(CLI_FILE);
        fs::create_dir_all(cwd.join(DIR)).unwrap();
        fs::write(
            &file,
            r#"{"version":1,"permissions":{"allow":["Shell(git)"],"deny":["Mcp(other:*)"]}}"#,
        )
        .unwrap();
        let mut installed = install(&cwd, URL, AUTH).expect("installed");
        installed.approve_fidget_tools().expect("approved");
        installed.approve_fidget_tools().expect("idempotent");

        let mut config = read(&file);
        assert_eq!(
            config["permissions"]["allow"],
            json!(["Shell(git)", "Mcp(fidget:*)"])
        );
        config["permissions"]["allow"]
            .as_array_mut()
            .unwrap()
            .push(json!("Read(src/**)"));
        write(&file, &config).unwrap();

        installed.remove();
        assert_eq!(
            read(&file),
            json!({
                "version": 1,
                "permissions": {
                    "allow": ["Shell(git)", "Read(src/**)"],
                    "deny": ["Mcp(other:*)"]
                }
            })
        );
    }

    #[test]
    fn preexisting_fidget_permission_is_not_removed() {
        let cwd = dir("permission-user-owned");
        let file = cwd.join(DIR).join(CLI_FILE);
        fs::create_dir_all(cwd.join(DIR)).unwrap();
        fs::write(&file, r#"{"permissions":{"allow":["Mcp(fidget:*)"]}}"#).unwrap();
        let mut installed = install(&cwd, URL, AUTH).expect("installed");
        installed.approve_fidget_tools().expect("already approved");

        installed.remove();
        assert_eq!(
            read(&file),
            json!({"permissions": {"allow": ["Mcp(fidget:*)"]}})
        );
    }

    #[test]
    fn malformed_cli_permissions_are_left_untouched() {
        let cwd = dir("permission-invalid");
        let file = cwd.join(DIR).join(CLI_FILE);
        fs::create_dir_all(cwd.join(DIR)).unwrap();
        fs::write(&file, r#"{"permissions":{"allow":"all"}}"#).unwrap();
        let mut installed = install(&cwd, URL, AUTH).expect("installed");

        let err = installed
            .approve_fidget_tools()
            .expect_err("accepted invalid allow");
        assert!(err.contains("permissions.allow"), "{err}");
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            r#"{"permissions":{"allow":"all"}}"#
        );
        installed.remove();
        assert!(!cwd.join(DIR).join(FILE).exists());
        assert!(file.exists());
    }

    #[test]
    fn install_keeps_a_foreign_server_and_writes_the_loopback_entry() {
        let cwd = dir("foreign");
        let file = cwd.join(DIR).join(FILE);
        fs::create_dir_all(cwd.join(DIR)).unwrap();
        let other = json!({"command": "echo", "args": ["hi"], "env": {"K": "v"}});
        fs::write(
            &file,
            serde_json::to_string(&json!({"keep": 1, "mcpServers": {"other": other}})).unwrap(),
        )
        .unwrap();

        install(&cwd, URL, AUTH).expect("installed");

        let root = read(&file);
        assert_eq!(root["keep"], json!(1));
        assert_eq!(root["mcpServers"]["other"], other);
        assert_eq!(
            root["mcpServers"]["fidget"],
            json!({"url": URL, "headers": {"Authorization": AUTH}})
        );
    }

    /// The URL and the token are new every app run, so a changed entry has to
    /// land and an unchanged one has to cost nothing.
    #[test]
    fn a_new_token_rewrites_the_entry_and_the_same_one_does_not() {
        let cwd = dir("rewrite");
        let file = cwd.join(DIR).join(FILE);
        install(&cwd, URL, AUTH).expect("installed");
        let bytes = fs::read(&file).unwrap();
        let mtime = fs::metadata(&file).unwrap().modified().unwrap();
        thread::sleep(Duration::from_millis(20));

        install(&cwd, URL, AUTH).expect("installed again");
        assert_eq!(fs::read(&file).unwrap(), bytes);
        assert_eq!(fs::metadata(&file).unwrap().modified().unwrap(), mtime);

        install(&cwd, "http://127.0.0.1:9999/mcp", "Bearer next-run").expect("a later run");
        let root = read(&file);
        assert_eq!(
            root["mcpServers"]["fidget"]["url"],
            "http://127.0.0.1:9999/mcp"
        );
        assert_eq!(
            root["mcpServers"]["fidget"]["headers"]["Authorization"],
            "Bearer next-run"
        );
    }

    /// The entry is a live credential, so the file it lands in is owner-only.
    #[cfg(unix)]
    #[test]
    fn the_file_holding_the_token_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let cwd = dir("mode");
        let file = cwd.join(DIR).join(FILE);
        fs::create_dir_all(cwd.join(DIR)).unwrap();
        fs::write(&file, "{}").unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();

        install(&cwd, URL, AUTH).expect("installed");

        assert_eq!(
            fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn install_then_remove_on_a_fresh_dir_leaves_no_cursor_dir() {
        let cwd = dir("fresh");
        let installed = install(&cwd, URL, AUTH).expect("installed");
        assert!(cwd.join(DIR).join(FILE).is_file());

        installed.remove();

        assert!(!cwd.join(DIR).exists(), ".cursor was left behind");
    }

    #[test]
    fn remove_keeps_a_file_we_did_not_create_and_its_foreign_server() {
        let cwd = dir("theirs");
        let file = cwd.join(DIR).join(FILE);
        fs::create_dir_all(cwd.join(DIR)).unwrap();
        fs::write(&file, r#"{"mcpServers":{"other":{"command":"echo"}}}"#).unwrap();

        install(&cwd, URL, AUTH).expect("installed").remove();

        let root = read(&file);
        assert_eq!(root, json!({"mcpServers": {"other": {"command": "echo"}}}));
    }

    /// No token is left behind in a project directory after a quit.
    #[test]
    fn remove_leaves_no_token_on_disk() {
        let cwd = dir("notoken");
        let file = cwd.join(DIR).join(FILE);
        fs::create_dir_all(cwd.join(DIR)).unwrap();
        fs::write(&file, r#"{"mcpServers":{"other":{"command":"echo"}}}"#).unwrap();

        let installed = install(&cwd, URL, AUTH).expect("installed");
        assert!(fs::read_to_string(&file).unwrap().contains(AUTH));
        installed.remove();

        assert!(!fs::read_to_string(&file).unwrap().contains("per-run-token"));
    }

    #[test]
    fn an_unparsable_file_is_an_error_and_untouched() {
        let cwd = dir("broken");
        let file = cwd.join(DIR).join(FILE);
        fs::create_dir_all(cwd.join(DIR)).unwrap();
        fs::write(&file, "{not json").unwrap();

        let err = install(&cwd, URL, AUTH).expect_err("wrote over garbage");

        assert!(err.contains(FILE), "{err}");
        assert_eq!(fs::read_to_string(&file).unwrap(), "{not json");
    }

    #[cfg(unix)]
    fn fake_cli(dir: &Path, exit: u8) -> (PathBuf, PathBuf) {
        use std::os::unix::fs::PermissionsExt;

        let log = dir.join("calls.log");
        let cli = dir.join("cursor-agent");
        fs::write(
            &cli,
            format!(
                "#!/bin/sh\necho \"$PWD|$@\" >> '{}'\nexit {exit}\n",
                log.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&cli, fs::Permissions::from_mode(0o755)).unwrap();
        (cli, log)
    }

    #[cfg(unix)]
    #[test]
    fn enable_runs_the_cli_once_in_the_cwd() {
        let bin = dir("enable-bin");
        let cwd = dir("enable-cwd");
        let (cli, log) = fake_cli(&bin, 0);

        enable(&cli, &cwd).expect("enabled");

        let seen = fs::read_to_string(&log).unwrap();
        assert_eq!(
            seen,
            format!(
                "{}|mcp enable fidget\n",
                cwd.canonicalize().unwrap().display()
            )
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_failing_enable_is_an_error() {
        let bin = dir("enable-fail");
        let (cli, _log) = fake_cli(&bin, 1);
        let err = enable(&cli, &bin).expect_err("exit 1 passed");
        assert!(err.contains("mcp enable"), "{err}");
    }
}
