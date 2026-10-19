//! Readonly MCP resources: Memory, the Action Log, and window names.
//!
//! One consent covers every name the fidget reports. The owner and title reach
//! `WindowRect` when usable, and this resource reads them again over its own
//! path under the same consent.

use std::fs;
use std::sync::Mutex;

use fidget_core::dispatch::DenyList;

pub const WINDOWS_URI: &str = "fidget://windows";
pub const MEMORY_URI: &str = "fidget://memory";
pub const ACTION_LOG_URI: &str = "fidget://action-log";

/// MCP request bodies stop at 1 MiB (`mcp_http::BODY_LIMIT`), and the Action
/// Log can reach 2 MiB before rotation. 512 KiB of complete JSONL lines is half
/// that budget, leaves room for JSON-RPC wrapping, and never splits a line.
pub const ACTION_LOG_TAIL_BYTES: usize = 512 * 1024;

/// One visible window as the titles resource sees it: owner and title, no
/// bounds, no id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowTitle {
    pub owner: String,
    pub title: String,
}

/// One catalog entry as `resources/list` advertises it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resource {
    pub uri: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub mime_type: &'static str,
}

/// The three URIs, in the order `resources/list` must keep.
pub fn catalog() -> [Resource; 3] {
    [
        Resource {
            uri: WINDOWS_URI,
            name: "Window names",
            description: "Visible windows, frontmost first, with the owning application and the title. One consent covers both, so this is empty without it. The same excluded applications as list_windows.",
            mime_type: "text/plain",
        },
        Resource {
            uri: MEMORY_URI,
            name: "Memory",
            description: "The Memory Manifest file every Character Instance shares.",
            mime_type: "text/markdown",
        },
        Resource {
            uri: ACTION_LOG_URI,
            name: "Action Log",
            description: "The current Action Log file only. Rotated siblings are not this resource.",
            mime_type: "application/x-ndjson",
        },
    ]
}

/// Format `(owner, title)` pairs as the resource text. Frontmost first is
/// the caller's job; empty title means the OS withheld the name.
pub fn format_window_titles(windows: &[WindowTitle]) -> String {
    windows
        .iter()
        .map(|window| {
            if window.title.is_empty() {
                window.owner.clone()
            } else {
                format!("{}\t{}", window.owner, window.title)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Same excluded applications as `list_windows`, same order the caller passed.
pub fn window_titles_text(
    windows: impl IntoIterator<Item = WindowTitle>,
    denylist: &DenyList,
) -> String {
    let kept: Vec<WindowTitle> = windows
        .into_iter()
        .filter(|window| denylist.allows(&window.owner))
        .collect();
    format_window_titles(&kept)
}

/// Memory Manifest bytes. Missing or empty is empty text, not an error.
pub fn memory_text() -> String {
    memory_from_path(&fidget_core::memory::shared_path())
}

/// Memory at `path`. A missing file is empty text, not an error.
pub fn memory_from_path(path: &std::path::Path) -> String {
    match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(_) => String::new(),
    }
}

/// Current Action Log file, tailed to complete JSONL lines when large.
pub fn action_log_text() -> String {
    match fs::read(crate::action_log::current_path()) {
        Ok(bytes) => action_log_from_bytes(&bytes),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(_) => String::new(),
    }
}

/// Recent complete JSONL lines, never a split line.
pub fn action_log_from_bytes(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    let start = bytes.len().saturating_sub(ACTION_LOG_TAIL_BYTES);
    let from = if start == 0 {
        0
    } else if bytes[start - 1] == b'\n' {
        start
    } else {
        match bytes[start..].iter().position(|&b| b == b'\n') {
            Some(rel) => start + rel + 1,
            None => return String::new(),
        }
    };
    String::from_utf8_lossy(&bytes[from..]).into_owned()
}

static EXCLUDED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Live excluded applications, published from the settings the frame loop
/// already holds so this resource matches `list_windows`.
pub fn publish_excluded(names: &[String]) {
    if let Ok(mut excluded) = EXCLUDED.lock() {
        *excluded = names.to_vec();
    }
}

/// The DenyList last published from settings, so titles and `list_windows`
/// drop the same applications.
pub fn live_denylist() -> DenyList {
    DenyList {
        excluded_applications: EXCLUDED.lock().map(|e| e.clone()).unwrap_or_default(),
        filter_password_fields: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(owner: &str, title: &str) -> WindowTitle {
        WindowTitle {
            owner: owner.to_string(),
            title: title.to_string(),
        }
    }

    #[test]
    fn window_titles_text_is_owner_and_title_when_present() {
        assert_eq!(
            format_window_titles(&[
                window("Terminal", "src/main.rs"),
                window("Finder", ""),
                window("Safari", "GitHub"),
            ]),
            "Terminal\tsrc/main.rs\nFinder\nSafari\tGitHub"
        );
    }

    #[test]
    fn window_titles_keep_z_order_and_drop_denylist_matches() {
        let denylist = DenyList {
            excluded_applications: vec!["1Password".to_string()],
            filter_password_fields: true,
        };
        let text = window_titles_text(
            [
                window("Terminal", "fidget"),
                window("1Password", "Login"),
                window("Finder", "Desktop"),
            ],
            &denylist,
        );
        assert_eq!(text, "Terminal\tfidget\nFinder\tDesktop");
    }

    #[test]
    fn denylist_match_is_case_insensitive() {
        let denylist = DenyList {
            excluded_applications: vec!["keychain access".to_string()],
            filter_password_fields: true,
        };
        let text = window_titles_text([window("Keychain Access", "login.keychain")], &denylist);
        assert_eq!(text, "");
    }

    #[test]
    fn memory_is_the_file_bytes_and_missing_is_empty() {
        let dir = std::env::temp_dir().join(format!("fidget-mcp-memory-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a temp dir");
        let path = dir.join("memory.md");
        std::fs::write(&path, "# Facts\n\n- likes tea\n").expect("wrote Memory");
        assert_eq!(memory_from_path(&path), "# Facts\n\n- likes tea\n");
        std::fs::remove_file(&path).ok();
        assert_eq!(memory_from_path(&path), "");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn action_log_tails_complete_jsonl_lines_when_over_the_ceiling() {
        let old = format!("{}\n", "x".repeat(80));
        let kept = "{\"event\":\"kept\"}\n{\"event\":\"also\"}\n";
        let mut bytes = old
            .repeat((ACTION_LOG_TAIL_BYTES / old.len()) + 2)
            .into_bytes();
        bytes.extend_from_slice(kept.as_bytes());
        assert!(bytes.len() > ACTION_LOG_TAIL_BYTES);
        let text = action_log_from_bytes(&bytes);
        let first = text.lines().next().unwrap_or("");
        assert!(
            first == "x".repeat(80) || first.starts_with('{'),
            "must not start mid-line: {first:?}"
        );
        assert!(
            text.ends_with(kept),
            "recent complete lines must survive: {text:?}"
        );
        assert!(text.len() <= ACTION_LOG_TAIL_BYTES);
    }

    #[test]
    fn a_missing_or_empty_action_log_is_empty_text() {
        assert_eq!(action_log_from_bytes(b""), "");
        assert_eq!(
            action_log_from_bytes(b"{\"event\":\"one\"}\n"),
            "{\"event\":\"one\"}\n"
        );
    }
}
