//! Private on-disk handoff from the relay projector to the context MCP.
//!
//! Packages live under the provider's identity-scoped state directory. They
//! are never published, placed in the agent environment, or mixed with the
//! provider's opaque native-session cursor.

use std::fs::{File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};

use buzz_core::coding_session_context::CodingSessionContextPackage;

const CONTEXT_PACKAGE_DIRECTORY: &str = "context-packages";

/// Failure to validate or persist one private rehydration package.
#[derive(Debug, thiserror::Error)]
pub enum ContextStoreError {
    /// The shared strict package rejected the projected value.
    #[error("context package validation failed: {0}")]
    InvalidPackage(String),
    /// The provider state path could not safely hold a private package.
    #[error("context package storage failed: {0}")]
    Storage(String),
}

/// Persist a strict package as a new mode-0600 regular file.
///
/// `execution_id` is the provider-minted UUID for the new execution. The
/// returned path is absolute so it can cross the ACP process boundary without
/// depending on either subprocess's working directory.
pub fn write_context_package(
    state_dir: &Path,
    execution_id: &str,
    package: &CodingSessionContextPackage,
) -> Result<PathBuf, ContextStoreError> {
    package
        .validate()
        .map_err(ContextStoreError::InvalidPackage)?;
    let execution_id = uuid::Uuid::parse_str(execution_id)
        .map_err(|_| ContextStoreError::Storage("execution id is not a UUID".into()))?;
    let state_dir = absolute_path(state_dir)?;
    let directory = state_dir.join(CONTEXT_PACKAGE_DIRECTORY);
    std::fs::create_dir_all(&directory)
        .map_err(|error| storage_error("cannot create private package directory", error))?;
    reject_symlink_or_non_directory(&directory)?;
    set_private_directory_permissions(&directory)?;

    let path = directory.join(format!("{execution_id}.json"));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    configure_private_file(&mut options);
    let mut file = options
        .open(&path)
        .map_err(|error| storage_error("cannot create private package file", error))?;
    set_private_file_permissions(&file)?;
    let bytes = serde_json::to_vec(package)
        .map_err(|error| ContextStoreError::Storage(format!("cannot encode package: {error}")))?;
    if let Err(error) = write_and_sync(&mut file, &bytes) {
        drop(file);
        let _ = std::fs::remove_file(&path);
        return Err(error);
    }
    Ok(path)
}

fn absolute_path(path: &Path) -> Result<PathBuf, ContextStoreError> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    std::env::current_dir()
        .map(|cwd| cwd.join(path))
        .map_err(|error| storage_error("cannot resolve provider state directory", error))
}

fn reject_symlink_or_non_directory(path: &Path) -> Result<(), ContextStoreError> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| storage_error("cannot inspect private package directory", error))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(ContextStoreError::Storage(
            "private package directory must be a real directory".into(),
        ));
    }
    Ok(())
}

fn write_and_sync(file: &mut File, bytes: &[u8]) -> Result<(), ContextStoreError> {
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| storage_error("cannot write private package file", error))
}

fn storage_error(context: &str, error: std::io::Error) -> ContextStoreError {
    ContextStoreError::Storage(format!("{context}: {error}"))
}

#[cfg(unix)]
fn configure_private_file(options: &mut OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt as _;
    options.mode(0o600);
}

#[cfg(not(unix))]
fn configure_private_file(_options: &mut OpenOptions) {}

#[cfg(unix)]
fn set_private_directory_permissions(path: &Path) -> Result<(), ContextStoreError> {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
        .map_err(|error| storage_error("cannot make package directory private", error))
}

#[cfg(not(unix))]
fn set_private_directory_permissions(_path: &Path) -> Result<(), ContextStoreError> {
    Ok(())
}

#[cfg(unix)]
fn set_private_file_permissions(file: &File) -> Result<(), ContextStoreError> {
    use std::os::unix::fs::PermissionsExt as _;
    file.set_permissions(std::fs::Permissions::from_mode(0o600))
        .map_err(|error| storage_error("cannot make package file private", error))
}

#[cfg(not(unix))]
fn set_private_file_permissions(_file: &File) -> Result<(), ContextStoreError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt as _;

    use buzz_core::coding_session_context::{
        CodingSessionContextIdentity, CodingSessionContextPackage, CodingSessionContextProvenance,
        CODING_SESSION_CONTEXT_PACKAGE_VERSION,
    };

    use super::*;

    fn empty_package() -> CodingSessionContextPackage {
        CodingSessionContextPackage {
            v: CODING_SESSION_CONTEXT_PACKAGE_VERSION,
            session: CodingSessionContextIdentity {
                session_ref: "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10".into(),
                genesis_ref: "ab".repeat(32),
                channel_id: uuid::Uuid::nil(),
                name: None,
                goal: None,
                project_ref: None,
            },
            provenance: CodingSessionContextProvenance {
                generated_at: 1,
                complete: true,
                truncated: false,
                source_event_count: 1,
                included_history_items: 0,
                omitted_history_items: 0,
                total_history_items: Some(0),
                notes: vec!["Complete empty fixture".into()],
            },
            history: Vec::new(),
        }
    }

    #[test]
    fn writes_a_new_absolute_private_regular_file() {
        let dir = tempfile::tempdir().unwrap();
        let execution_id = uuid::Uuid::new_v4().to_string();
        let path = write_context_package(dir.path(), &execution_id, &empty_package()).unwrap();
        assert!(path.is_absolute());
        let metadata = std::fs::symlink_metadata(&path).unwrap();
        assert!(metadata.is_file());
        #[cfg(unix)]
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
        let decoded: CodingSessionContextPackage =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        decoded.validate().unwrap();
    }

    #[test]
    fn never_overwrites_an_existing_execution_package() {
        let dir = tempfile::tempdir().unwrap();
        let execution_id = uuid::Uuid::new_v4().to_string();
        write_context_package(dir.path(), &execution_id, &empty_package()).unwrap();
        assert!(write_context_package(dir.path(), &execution_id, &empty_package()).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_a_symlinked_package_directory() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        symlink(elsewhere.path(), dir.path().join(CONTEXT_PACKAGE_DIRECTORY)).unwrap();
        assert!(write_context_package(
            dir.path(),
            &uuid::Uuid::new_v4().to_string(),
            &empty_package()
        )
        .is_err());
    }
}
