//! Process diagnostics: stderr and panics, rotated beside Memory.
//!
//! Packaged builds have no console. This file is what those lines become. It is
//! not the Action Log. That file is JSONL Harness events with a different
//! contract.
//!
//! ## Growth policy
//!
//! Same rotator as the Action Log: `file-rotate` 0.8, `BytesSurpassed` at 2 MB
//! so a line stays whole, `AppendCount` 10 so the oldest falls off. Files are
//! `process.log`, `process.log.1`, ..., `process.log.10`. Ceiling is ~22 MB.
//!
//! Write failures drop. A diagnostic that blocks the app is worse than a
//! missing line. Rate-limited error reporting (once per 60s) avoids storms.
//!
//! **Thread-safety**: stderr and the panic hook can fire from any thread.
//! One long-lived `FileRotate` per data-dir is cached behind a `Mutex`; all
//! writes go through that locked handle so rotation does not race.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use file_rotate::compression::Compression;
use file_rotate::suffix::AppendCount;
use file_rotate::{ContentLimit, FileRotate};

/// The log file name, in the data folder beside Memory.
pub const FILE: &str = "process.log";

/// The log as it stands now, for anything that opens it rather than writes it.
///
/// Rotated siblings get no accessor: the current file is what a user wants after a crash.
pub fn current_path() -> PathBuf {
    fidget_core::memory::data_dir().join(FILE)
}

/// The size bound per log file, in bytes.
///
/// K=10 retention is 11 files, so the disk ceiling is ~22 MB (11 × 2 MB).
const MAX_SIZE_BYTES: usize = 2 * 1024 * 1024;

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

/// Install the panic hook and redirect stderr to the process log.
///
/// Safe to call more than once. Call early, before anything that might panic.
/// After this, all stderr writes (including `eprintln!`) land in the process log.
pub fn init() {
    static STARTED: OnceLock<()> = OnceLock::new();
    STARTED.get_or_init(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            append(&format!("panic: {info}"));
            previous(info);
        }));

        #[cfg(unix)]
        start_stderr_capture_unix();
        #[cfg(windows)]
        start_stderr_capture_windows();
    });
}

/// Append one timestamped line to the process log beside Memory.
///
/// Failures drop so a diagnostic never blocks the app.
pub fn append(msg: &str) {
    append_to(&fidget_core::memory::data_dir(), msg);
}

/// Write to the process log always, and to stderr in debug builds.
///
/// Release packages have no console. Debug still wants the line in the
/// terminal the developer launched from.
#[macro_export]
macro_rules! eprintln_and_log {
    ($($arg:tt)*) => {{
        let msg = format!($($arg)*);
        #[cfg(debug_assertions)]
        {
            eprintln!("{msg}");
        }
        $crate::process_log::append(&msg);
    }};
}

fn append_to(dir: &Path, msg: &str) {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    // Panic payloads and stderr dumps embed newlines. One append must stay
    // one rotation unit or BytesSurpassed cannot keep a line whole.
    let message = msg.replace(['\n', '\r'], " ");
    let line = format!("{ts} {message}");

    // init() can run before Memory creates the data folder. A startup panic
    // still has to land somewhere.
    if let Err(e) = std::fs::create_dir_all(dir) {
        report_error(&format!("process_log: failed to create dir: {e}"));
        return;
    }

    let log_path = dir.join(FILE);

    let rotators = ROTATORS.get_or_init(|| Mutex::new(HashMap::new()));
    let rotator_arc = {
        let mut rotators_map = rotators.lock().unwrap();
        rotators_map
            .entry(dir.to_path_buf())
            .or_insert_with(|| {
                // BytesSurpassed rotates after a write that pushes past the limit,
                // keeping lines whole. Bytes(n) can split mid-write.
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
    if let Err(e) = writeln!(rotator, "{line}") {
        report_error(&format!("process_log: failed to write line: {e}"));
        return;
    }

    if let Err(e) = rotator.flush() {
        report_error(&format!("process_log: failed to flush: {e}"));
    }
}

/// Rate-limit error reports so a write loop cannot storm stderr.
fn report_error(msg: &str) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());

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

#[cfg(unix)]
fn start_stderr_capture_unix() {
    use std::io::BufRead;
    use std::os::unix::io::FromRawFd;

    let pipe_result = unsafe {
        let mut fds = [0; 2];
        if libc::pipe(fds.as_mut_ptr()) != 0 {
            return;
        }
        (fds[0], fds[1])
    };

    let (read_fd, write_fd) = pipe_result;

    #[cfg(debug_assertions)]
    let original_stderr = unsafe { libc::dup(libc::STDERR_FILENO) };

    unsafe {
        libc::dup2(write_fd, libc::STDERR_FILENO);
        libc::close(write_fd);
    }

    std::thread::spawn(move || {
        let reader = unsafe { std::fs::File::from_raw_fd(read_fd) };
        let reader = std::io::BufReader::new(reader);

        for line in reader.lines().map_while(Result::ok) {
            append(&line);

            #[cfg(debug_assertions)]
            unsafe {
                use std::io::Write;
                let mut stderr = std::fs::File::from_raw_fd(original_stderr);
                let _ = writeln!(stderr, "{}", line);
                std::mem::forget(stderr);
            }
        }
    });
}

#[cfg(windows)]
fn start_stderr_capture_windows() {
    use std::io::BufRead;
    use std::os::windows::io::FromRawHandle;
    use windows_sys::Win32::Foundation::{
        DuplicateHandle, DUPLICATE_SAME_ACCESS, HANDLE, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::System::Pipes::CreatePipe;
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    unsafe {
        let mut read_handle: HANDLE = INVALID_HANDLE_VALUE;
        let mut write_handle: HANDLE = INVALID_HANDLE_VALUE;

        if CreatePipe(&mut read_handle, &mut write_handle, std::ptr::null_mut(), 0) == 0 {
            return;
        }

        #[cfg(debug_assertions)]
        let original_stderr = {
            let stderr_handle = windows_sys::Win32::System::Console::GetStdHandle(
                windows_sys::Win32::System::Console::STD_ERROR_HANDLE,
            );
            let mut dup: HANDLE = INVALID_HANDLE_VALUE;
            DuplicateHandle(
                GetCurrentProcess(),
                stderr_handle,
                GetCurrentProcess(),
                &mut dup,
                0,
                0,
                DUPLICATE_SAME_ACCESS,
            );
            std::fs::File::from_raw_handle(dup as _)
        };

        windows_sys::Win32::System::Console::SetStdHandle(
            windows_sys::Win32::System::Console::STD_ERROR_HANDLE,
            write_handle,
        );

        let reader = std::fs::File::from_raw_handle(read_handle as _);

        std::thread::spawn(move || {
            let reader = std::io::BufReader::new(reader);

            for line in reader.lines().flatten() {
                append(&line);

                #[cfg(debug_assertions)]
                {
                    use std::io::Write;
                    let mut stderr = &original_stderr;
                    let _ = writeln!(stderr, "{}", line);
                }
            }
        });
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
                "process-log-{label}-{}-{unique}",
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

    fn log_paths(dir: &Path) -> Vec<PathBuf> {
        let mut paths = vec![dir.join(FILE)];
        for n in 1..=RETENTION_COUNT {
            paths.push(dir.join(format!("{FILE}.{n}")));
        }
        paths
    }

    fn read_all_log_text(dir: &Path) -> String {
        let mut body = String::new();
        for path in log_paths(dir).iter().filter(|path| path.exists()) {
            body.push_str(&fs::read_to_string(path).unwrap());
        }
        body
    }

    /// Every writer and reader must land on one file.
    #[test]
    fn the_log_sits_beside_memory() {
        let opened = current_path();

        assert_eq!(opened.file_name().unwrap(), FILE);
        assert_eq!(
            opened.parent().unwrap(),
            fidget_core::memory::data_dir(),
            "the data folder Memory is in, not a temp copy"
        );
    }

    #[test]
    fn append_writes_a_timestamped_line() {
        let dir = TempDir::new("timestamp");

        append_to(dir.path(), "hello from process log");

        let content = fs::read_to_string(dir.path().join(FILE)).expect("the log file exists");
        let line = content
            .lines()
            .next()
            .expect("append writes one line")
            .to_string();
        let (ts, message) = line
            .split_once(' ')
            .expect("a line is a unix timestamp, a space, then the message");
        let ts: u64 = ts
            .parse()
            .expect("the timestamp is seconds since the epoch");
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_secs());

        assert_eq!(message, "hello from process log");
        assert!(
            ts <= now && now.saturating_sub(ts) < 60,
            "timestamp {ts} is within a minute of now {now}"
        );
        assert!(content.ends_with('\n'), "the line is terminated");
    }

    #[test]
    fn a_message_with_newlines_stays_one_line() {
        let dir = TempDir::new("newlines");

        append_to(dir.path(), "hello\nworld\r\nagain");

        let content = fs::read_to_string(dir.path().join(FILE)).unwrap();
        let lines: Vec<&str> = content.lines().filter(|line| !line.is_empty()).collect();
        assert_eq!(lines.len(), 1, "one append is one physical line");
        let message = lines[0].split_once(' ').map(|(_, rest)| rest).unwrap();
        assert_eq!(message, "hello world  again");
    }

    #[test]
    fn a_small_log_is_not_rotated() {
        let dir = TempDir::new("no-rotation");

        for i in 0..10 {
            append_to(dir.path(), &format!("line {i}"));
        }

        let log = dir.path().join(FILE);
        assert!(log.exists(), "the log file exists");
        assert!(
            fs::metadata(&log).unwrap().len() < MAX_SIZE_BYTES as u64,
            "the log is well under the rotation threshold"
        );

        let backup_1 = dir.path().join("process.log.1");
        assert!(!backup_1.exists(), "no backup was created");
    }

    #[test]
    fn a_large_log_is_rotated_when_it_surpasses_max_size() {
        let dir = TempDir::new("rotation");
        let log = dir.path().join(FILE);

        let large_line = "x".repeat(1024);
        for i in 0..2500 {
            append_to(dir.path(), &format!("{i} {large_line}"));
        }

        let backup_1 = dir.path().join("process.log.1");
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
            backup_content.contains("0 xxxx"),
            "the .1 backup contains early events"
        );
    }

    #[test]
    fn rotation_keeps_k_files_and_drops_oldest() {
        let dir = TempDir::new("k-retention");

        let large_line = "x".repeat(1024);
        for i in 0..15_000 {
            append_to(dir.path(), &format!("{i} {large_line}"));
        }

        let existing: Vec<PathBuf> = log_paths(dir.path())
            .into_iter()
            .filter(|path| path.exists())
            .collect();

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
    fn a_line_stays_whole_after_rotation() {
        let dir = TempDir::new("whole-line");

        let large_line = "x".repeat(1024);
        for i in 0..2500 {
            let payload = format!(r#"{{"index":{i},"data":"{large_line}"}}"#);
            append_to(dir.path(), &payload);
        }

        for path in log_paths(dir.path()).iter().filter(|path| path.exists()) {
            let content = fs::read_to_string(path).unwrap();

            for (i, line) in content.lines().enumerate() {
                if line.is_empty() {
                    continue;
                }

                let message = line.split_once(' ').map(|(_, rest)| rest).unwrap_or(line);
                serde_json::from_str::<serde_json::Value>(message).unwrap_or_else(|e| {
                    panic!(
                        "line {i} in {} is not a whole JSON message: {e}\nLine content: {}",
                        path.display(),
                        if line.len() > 200 { &line[..200] } else { line }
                    );
                });
            }
        }
    }

    #[test]
    fn concurrent_appends_all_land() {
        let dir = TempDir::new("concurrent");
        let threads = 8usize;
        let per_thread = 100usize;

        std::thread::scope(|scope| {
            for thread in 0..threads {
                let path = dir.path().to_path_buf();
                scope.spawn(move || {
                    for i in 0..per_thread {
                        append_to(&path, &format!("thread-{thread}-msg-{i}"));
                    }
                });
            }
        });

        let body = read_all_log_text(dir.path());
        let lines: Vec<&str> = body.lines().filter(|line| !line.is_empty()).collect();
        assert_eq!(
            lines.len(),
            threads * per_thread,
            "every concurrent append is one line"
        );

        for thread in 0..threads {
            for i in 0..per_thread {
                let needle = format!("thread-{thread}-msg-{i}");
                assert!(
                    lines.iter().any(|line| line.ends_with(needle.as_str())),
                    "missing {needle}"
                );
            }
        }
    }

    #[test]
    fn sustained_append_storm_stays_within_disk_bound() {
        let dir = TempDir::new("append-storm");

        let line = "x".repeat(200);
        for i in 0..30_000 {
            append_to(dir.path(), &format!("{i} {line}"));
        }

        let mut total_size = 0u64;
        for path in log_paths(dir.path()).iter().filter(|path| path.exists()) {
            total_size += fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        }

        // BytesSurpassed rotates after a write that pushes past the limit,
        // so each file can be at most MAX_SIZE_BYTES + one max line. Bound:
        // (K+1) * (MAX_SIZE + 250).
        let headroom_per_file = 250;
        let bound = ((RETENTION_COUNT + 1) as u64) * ((MAX_SIZE_BYTES as u64) + headroom_per_file);
        assert!(
            total_size <= bound,
            "total disk footprint {total_size} bytes is within ~(K+1)*MAX_SIZE+headroom bound ({bound} bytes)"
        );
    }
}
