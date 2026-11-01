//! Debug tooling for test automation.
//!
//! File-based IPC paths that scenario scripts use to place the character and
//! read its state without modifying the binary under test.

use std::fs;
use std::path::{Path, PathBuf};

use crate::memory;

/// Debug IPC command path for test scripts to place commands.
pub fn debug_cmd_path() -> Option<PathBuf> {
    Some(debug_dir()?.join(".fidget-debug-cmd"))
}

/// Debug IPC result path where fidget writes responses.
pub fn debug_result_path() -> Option<PathBuf> {
    Some(debug_dir()?.join(".fidget-debug-result"))
}

fn debug_dir() -> Option<PathBuf> {
    memory::data_dir_override().or_else(memory::home_dir)
}

/// The other side polls this path every frame, so it must never see a
/// half-written file. A rename on the same filesystem swaps the whole file in.
pub fn write_whole_file_at_once(path: &Path, contents: &str) -> std::io::Result<()> {
    let staging_path = path.with_extension("tmp");
    fs::write(&staging_path, contents)?;
    fs::rename(&staging_path, path).inspect_err(|_| {
        let _ = fs::remove_file(&staging_path);
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    #[test]
    fn a_reader_never_sees_a_partial_debug_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join(".fidget-debug-result");
        let done = Arc::new(AtomicBool::new(false));
        let reader = {
            let (path, done) = (path.clone(), done.clone());
            std::thread::spawn(move || {
                let mut partial = None;
                while !done.load(Ordering::Relaxed) && partial.is_none() {
                    match fs::read_to_string(&path) {
                        Ok(text) if text != "placed: (3, 4)" => partial = Some(text),
                        _ => {}
                    }
                }
                partial
            })
        };
        for _ in 0..2000 {
            write_whole_file_at_once(&path, "placed: (3, 4)").unwrap();
            let _ = fs::remove_file(&path);
        }
        done.store(true, Ordering::Relaxed);
        assert_eq!(reader.join().unwrap(), None);
    }
}
