//! The Action Log: one JSON line per thing the attached Harness did.
//!
//! Append-only, in the data folder beside Memory. It points at the Harness's
//! own session (session id, tool call ids) rather than copying a transcript,
//! which is what CONTEXT.md asks of it. There is still no reader in the app:
//! the tray hands the current file to the user's editor, and `tail -f` is the UI.
//!
//! ## Growth policy
//!
//! The log is rotated using the `file-rotate` crate (0.8.x). K=10 retention:
//! 10 rotated files + 1 current = 11 files total, ~22 MB ceiling.
//!
//! Files are named `action-log.jsonl`, `action-log.jsonl.1`, ..., `action-log.jsonl.10`.
//! When a write pushes the file past 2 MB, file-rotate moves it to `.1` and
//! older files cascade (`.1` → `.2`, ..., `.9` → `.10`). The oldest (`.10`)
//! is dropped.
//!
//! **ContentLimit::BytesSurpassed** is used instead of `ContentLimit::Bytes`:
//! `Bytes(n)` can split a single write mid-string (documented behavior), which
//! breaks JSONL. `BytesSurpassed(n)` only rotates *after* a write that pushes
//! past the limit, keeping lines whole.
//!
//! Write failures (including rotation failures) are dropped: the log explains
//! the character after the fact and must never block a Director turn. Rate-limited
//! error reporting (max once per 60s) warns operators without log storms.
//!
//! **Thread-safety**: Harness appends from multiple threads (wire events, turn/complete,
//! spawn_preflight). One long-lived `FileRotate` per data-dir is cached behind a `Mutex`;
//! all writes go through that locked handle to avoid races during rotation.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use file_rotate::compression::Compression;
use file_rotate::suffix::AppendCount;
use file_rotate::{ContentLimit, FileRotate};
use serde_json::{json, Value};

pub const FILE: &str = "action-log.jsonl";

/// The log as it stands now, for anything that opens it rather than writes it.
///
/// Rotated siblings get no accessor: the current file is what a user wants after a session.
pub fn current_path() -> PathBuf {
    fidget_core::memory::data_dir().join(FILE)
}

/// The size bound per log file, in bytes.
///
/// K=10 retention is 11 files, so the disk ceiling is ~22 MB (11 × 2 MB).
const MAX_SIZE_BYTES: usize = 2 * 1024 * 1024; // 2 MB

/// How many rotated files to keep (plus the current file = K+1 total).
const RETENTION_COUNT: usize = 10;

/// Minimum seconds between error reports, to avoid log storms.
const ERROR_REPORT_INTERVAL_SECS: u64 = 60;

/// Rate-limited error state: last error timestamp and consecutive failure count.
static ERROR_STATE: Mutex<Option<(u64, u64)>> = Mutex::new(None);

type RotatorHandle = Arc<Mutex<FileRotate<AppendCount>>>;

/// Cached FileRotate instances, one per data directory.
///
/// Concurrent `FileRotate::new` plus rotate would race the rename cascade.
static ROTATORS: OnceLock<Mutex<HashMap<PathBuf, RotatorHandle>>> = OnceLock::new();

/// Write one event. Failed writes drop so a turn is never blocked.
///
/// ponytail: seconds since the epoch, as `memory.rs` does, so no date crate.
pub fn append(dir: &Path, event: &str, mut fields: Value) {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    if let Some(object) = fields.as_object_mut() {
        object.insert("event".into(), json!(event));
        object.insert("ts".into(), json!(ts));
    }

    let log_path = dir.join(FILE);

    // Format JSON to string before writing to avoid serde_json Display issues
    let line = match serde_json::to_string(&fields) {
        Ok(s) => s,
        Err(e) => {
            report_error(&format!("action_log: failed to serialize JSON: {e}"));
            return;
        }
    };

    let rotators = ROTATORS.get_or_init(|| Mutex::new(HashMap::new()));
    let rotator_arc = {
        let mut rotators_map = rotators.lock().unwrap();
        rotators_map
            .entry(dir.to_path_buf())
            .or_insert_with(|| {
                // BytesSurpassed rotates after a write that pushes past the limit,
                // keeping JSONL lines whole. Bytes(n) can split mid-write.
                let rotator = FileRotate::new(
                    &log_path,
                    AppendCount::new(RETENTION_COUNT),
                    ContentLimit::BytesSurpassed(MAX_SIZE_BYTES),
                    Compression::None,
                    None,
                );
                Arc::new(Mutex::new(rotator))
            })
            .clone()
    };

    let mut rotator = rotator_arc.lock().unwrap();
    if let Err(e) = writeln!(rotator, "{}", line) {
        report_error(&format!("action_log: failed to write event: {e}"));
        return;
    }

    if let Err(e) = rotator.flush() {
        report_error(&format!("action_log: failed to flush: {e}"));
    }
}

/// Rate-limit error reports so a write loop cannot storm stderr.
fn report_error(msg: &str) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());

    fidget::process_log::append(msg);

    let mut state = ERROR_STATE.lock().unwrap();
    let should_report = match *state {
        None => {
            *state = Some((now, 1));
            true
        }
        Some((last_ts, count)) => {
            let elapsed = now.saturating_sub(last_ts);
            if elapsed >= ERROR_REPORT_INTERVAL_SECS {
                *state = Some((now, 1));
                true
            } else {
                *state = Some((last_ts, count + 1));
                false
            }
        }
    };

    if should_report {
        if let Some((_, count)) = *state {
            if count > 1 {
                eprintln!("{msg} ({count} consecutive failures)");
            } else {
                eprintln!("{msg}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A directory of our own under the system temp dir, removed when the test ends.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(label: &str) -> Self {
            static NEXT: AtomicU32 = AtomicU32::new(0);
            let unique = NEXT.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "action-log-{label}-{}-{unique}",
                std::process::id()
            ));
            fs::create_dir_all(&dir).expect("temp dir is creatable");
            Self(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// The tray row and every writer must land on one file. Two callers joining
    /// the same name are two places for it to stop being the same name.
    #[test]
    fn the_log_the_tray_opens_sits_beside_memory() {
        let opened = current_path();

        assert_eq!(opened.file_name().unwrap(), FILE);
        assert_eq!(
            opened.parent().unwrap(),
            fidget_core::memory::data_dir(),
            "the data folder Memory is in, not a temp copy"
        );
    }

    #[test]
    fn a_small_log_is_not_rotated() {
        let dir = TempDir::new("no-rotation");

        for i in 0..10 {
            append(dir.path(), "test", json!({"index": i}));
        }

        let log = dir.path().join(FILE);
        assert!(log.exists(), "the log file exists");
        assert!(
            fs::metadata(&log).unwrap().len() < MAX_SIZE_BYTES as u64,
            "the log is well under the rotation threshold"
        );

        let backup_1 = dir.path().join("action-log.jsonl.1");
        assert!(!backup_1.exists(), "no backup was created");
    }

    #[test]
    fn a_large_log_is_rotated_when_it_surpasses_max_size() {
        let dir = TempDir::new("rotation");
        let log = dir.path().join(FILE);

        let large_line = "x".repeat(1024);
        for i in 0..2500 {
            append(
                dir.path(),
                "large",
                json!({"index": i, "data": &large_line}),
            );
        }

        let backup_1 = dir.path().join("action-log.jsonl.1");
        assert!(backup_1.exists(), "a .1 backup was created after rotation");

        // BytesSurpassed rotates after a write that pushes past the limit,
        // so the current file can be slightly over MAX_SIZE_BYTES
        let current_size = fs::metadata(&log).unwrap().len();
        assert!(
            current_size < (MAX_SIZE_BYTES as u64) * 2,
            "the current log is under 2×MAX_SIZE_BYTES: {current_size} bytes"
        );

        let backup_content = fs::read_to_string(&backup_1).unwrap();
        assert!(
            backup_content.contains("\"index\":0"),
            "the .1 backup contains early events"
        );
    }

    #[test]
    fn rotation_keeps_k_files_and_drops_oldest() {
        let dir = TempDir::new("k-retention");

        let large_line = "x".repeat(1024);
        for i in 0..15_000 {
            append(
                dir.path(),
                "storm",
                json!({"index": i, "data": &large_line}),
            );
        }

        let mut existing = Vec::new();
        existing.push(dir.path().join(FILE));
        for n in 1..=RETENTION_COUNT {
            let path = dir.path().join(format!("{FILE}.{n}"));
            if path.exists() {
                existing.push(path);
            }
        }

        assert!(!existing.is_empty(), "at least the current log exists");
        assert!(
            existing.len() <= RETENTION_COUNT + 1,
            "no more than K+1 files: found {} files",
            existing.len()
        );

        let beyond_k = dir.path().join(format!("{FILE}.{}", RETENTION_COUNT + 1));
        assert!(!beyond_k.exists(), "no file beyond K retention");
    }

    #[test]
    fn the_log_stays_valid_jsonl_after_rotation() {
        let dir = TempDir::new("valid-jsonl");

        let large_line = "x".repeat(1024);
        for i in 0..2500 {
            append(
                dir.path(),
                "large",
                json!({"index": i, "data": &large_line}),
            );
        }

        let mut log_paths = vec![dir.path().join(FILE)];
        for n in 1..=RETENTION_COUNT {
            log_paths.push(dir.path().join(format!("{FILE}.{n}")));
        }

        for path in log_paths.iter().filter(|p| p.exists()) {
            let content = fs::read_to_string(path).unwrap();

            for (i, line) in content.lines().enumerate() {
                if line.is_empty() {
                    continue;
                }

                serde_json::from_str::<Value>(line).unwrap_or_else(|e| {
                    panic!(
                        "line {i} in {} is not valid JSON: {e}\nLine content: {}",
                        path.display(),
                        if line.len() > 200 { &line[..200] } else { line }
                    );
                });
            }
        }
    }

    #[test]
    fn sustained_append_storm_stays_within_disk_bound() {
        let dir = TempDir::new("append-storm");

        let line = "x".repeat(200);
        for i in 0..30_000 {
            append(dir.path(), "storm", json!({"index": i, "data": &line}));
        }

        let mut total_size = 0u64;
        let mut log_paths = vec![dir.path().join(FILE)];
        for n in 1..=RETENTION_COUNT {
            log_paths.push(dir.path().join(format!("{FILE}.{n}")));
        }

        for path in log_paths.iter().filter(|p| p.exists()) {
            total_size += fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        }

        // BytesSurpassed rotates after a write that pushes past the limit,
        // so each file can be at most MAX_SIZE_BYTES + one max line (~250 bytes
        // with JSON overhead). Bound: (K+1) * (MAX_SIZE + 250).
        let headroom_per_file = 250;
        let bound = ((RETENTION_COUNT + 1) as u64) * ((MAX_SIZE_BYTES as u64) + headroom_per_file);
        assert!(
            total_size <= bound,
            "total disk footprint {total_size} bytes is within ~(K+1)*MAX_SIZE+headroom bound ({bound} bytes)"
        );
    }
}
