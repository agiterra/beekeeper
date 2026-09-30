//! Writing a file that holds a secret, without a window where it is readable.
//!
//! Both launchers write records and config that must never exist on disk
//! world-readable, even for the instant between `create` and `chmod`. The
//! permissions are set on the temporary file *before* the bytes go in, and the
//! rename is what publishes it.

use std::io::Write;
use std::path::Path;

/// Write `payload` to `path` atomically, owner-readable only.
pub fn atomic_write_json_restricted(path: &Path, payload: &[u8]) -> Result<(), String> {
    use atomic_write_file::AtomicWriteFile;

    let resolved = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let mut file = AtomicWriteFile::open(&resolved)
        .map_err(|error| format!("open {} for atomic write: {error}", resolved.display()))?;

    // Owner-only permissions before the secret bytes, not after.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))
            .map_err(|error| format!("set {} permissions: {error}", resolved.display()))?;
    }

    file.write_all(payload)
        .map_err(|error| format!("write {}: {error}", resolved.display()))?;
    file.commit()
        .map_err(|error| format!("commit {}: {error}", resolved.display()))
}

/// Create `dir` and every missing parent, owner-only on Unix.
///
/// `create_dir_all` alone leaves the directory at the process umask, which on
/// a default macOS or Linux account is world-readable. A directory holding a
/// key file and a socket must not be.
pub fn create_dir_all_restricted(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir)
        .map_err(|error| format!("failed to create {}: {error}", dir.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|error| format!("failed to restrict {}: {error}", dir.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_written_file_is_owner_only_and_a_directory_is_too() {
        let dir = tempfile::tempdir().expect("tempdir");
        let nested = dir.path().join("host");
        create_dir_all_restricted(&nested).expect("mkdir");
        let path = nested.join("provider-key");
        atomic_write_json_restricted(&path, b"nsec1secret").expect("write");
        assert_eq!(std::fs::read(&path).expect("read"), b"nsec1secret");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode =
                |p: &Path| std::fs::metadata(p).expect("metadata").permissions().mode() & 0o777;
            assert_eq!(mode(&path), 0o600, "the key file must be owner-only");
            assert_eq!(mode(&nested), 0o700, "its directory must be owner-only");
        }
    }
}
