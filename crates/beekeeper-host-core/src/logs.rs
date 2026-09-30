//! The supervised child's log file: opened for append, rotated by size.
//!
//! Shared because the log is the operator's only window into a provider that
//! died, and it must read the same whichever launcher started it. A host that
//! rotated at a different size, or wrote its markers in a different shape,
//! would make the file that gets pasted into a bug report depend on who
//! launched the process.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;

/// Maximum log file size before rotation (10 MB).
pub const MAX_LOG_FILE_SIZE: u64 = 10 * 1024 * 1024;

/// If `path` exceeds [`MAX_LOG_FILE_SIZE`], rotate it to `<path>.1`.
fn maybe_rotate_log(path: &Path) {
    let size = match std::fs::metadata(path) {
        Ok(metadata) => metadata.len(),
        Err(_) => return,
    };
    if size <= MAX_LOG_FILE_SIZE {
        return;
    }
    let mut rotated = path.as_os_str().to_owned();
    rotated.push(".1");
    let _ = std::fs::rename(path, &rotated);
}

/// Open `path` for appending, rotating first if it has grown too large.
pub fn open_log_file(path: &Path) -> Result<File, String> {
    maybe_rotate_log(path);
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| format!("failed to open log file {}: {error}", path.display()))
}

/// Append one `=== … ===` lifecycle line to `path`.
pub fn append_log_marker(path: &Path, message: &str) -> Result<(), String> {
    let mut file = open_log_file(path)?;
    writeln!(file, "{message}").map_err(|error| format!("failed to write log marker: {error}"))
}

/// An RFC 3339 timestamp for a log marker.
pub fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_marker_appends_rather_than_replacing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("provider.log");
        append_log_marker(&path, "=== first ===").expect("first");
        append_log_marker(&path, "=== second ===").expect("second");
        let content = std::fs::read_to_string(&path).expect("read");
        assert_eq!(content, "=== first ===\n=== second ===\n");
    }

    #[test]
    fn an_oversized_log_rotates_to_dot_one_and_starts_empty() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("provider.log");
        std::fs::write(&path, vec![b'x'; (MAX_LOG_FILE_SIZE + 1) as usize]).expect("oversized");
        append_log_marker(&path, "=== after rotation ===").expect("marker");
        assert_eq!(
            std::fs::read_to_string(&path).expect("current"),
            "=== after rotation ===\n"
        );
        assert_eq!(
            std::fs::metadata(dir.path().join("provider.log.1"))
                .expect("rotated")
                .len(),
            MAX_LOG_FILE_SIZE + 1
        );
    }
}
