//! Immutable, restricted operation-scoped custody snapshots.
//!
//! Callers select only the participant's explicit custody artifacts. This
//! module neither discovers human transcripts nor starts, stops or restores an
//! executor. A partial write is resumable; a completed backup is never replaced.

use std::path::{Component, Path};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{atomic_write, record::canonical_relay_key};

/// Exact identity and operation whose custody was backed up.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BackupScope {
    /// Existing participant identity, never a newly minted replacement.
    pub actor_pubkey: String,
    /// Community whose custody and configuration are being retained.
    pub relay_url: String,
    /// Stable operation UUID, preserved across interruption and retry.
    pub operation_id: String,
}

/// One explicitly selected artifact; bytes may contain private credentials.
pub struct BackupFile<'a> {
    /// Relative name within the protected backup, with no parent traversal.
    pub name: &'a str,
    /// Source bytes; never returned in the integrity manifest.
    pub bytes: &'a [u8],
}

/// Integrity evidence for one protected artifact, without its contents.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BackupEntry {
    /// Backup-relative name.
    pub name: String,
    /// Exact byte count.
    pub bytes: u64,
    /// SHA-256 of the captured bytes.
    pub sha256: String,
}

/// Completed immutable snapshot; presence is not proof of execution termination.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BackupManifest {
    /// Versioned local backup format.
    pub schema: String,
    /// Existing identity/community and stable custody operation.
    pub scope: BackupScope,
    /// Every captured artifact, in deterministic name order.
    pub files: Vec<BackupEntry>,
}

fn relative(name: &str) -> Result<&Path, String> {
    let path = Path::new(name);
    if name.is_empty()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err("backup artifact requires a safe relative name".into());
    }
    if name == "manifest.json" {
        return Err("backup artifact cannot replace its manifest".into());
    }
    Ok(path)
}

fn restricted(path: &Path) -> Result<(), String> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|_| "backup artifact is unavailable".to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("backup artifact must be a regular file".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err("backup custody permissions are too broad".into());
        }
    }
    Ok(())
}

fn directory_path(root: &Path, relative_parent: &Path, create: bool) -> Result<(), String> {
    let mut path = root.to_path_buf();
    for component in std::iter::once(None).chain(relative_parent.components().map(Some)) {
        if let Some(component) = component {
            path.push(component.as_os_str());
        }
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
            Ok(_) => {
                return Err("backup directory must not contain a symlink or non-directory".into())
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && create => {
                atomic_write::create_dir_all_restricted(&path)?;
            }
            Err(_) => return Err("backup directory is unavailable".into()),
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if std::fs::symlink_metadata(&path)
                .map_err(|_| "backup directory unavailable")?
                .permissions()
                .mode()
                & 0o077
                != 0
            {
                return Err("backup directory permissions are too broad".into());
            }
        }
    }
    Ok(())
}

fn entry(name: &str, bytes: &[u8]) -> BackupEntry {
    BackupEntry {
        name: name.into(),
        bytes: bytes.len() as u64,
        sha256: hex::encode(Sha256::digest(bytes)),
    }
}

/// Capture selected custody before changing ownership, or verify the exact
/// prior capture on retry. Caller serializes with the management operation lock.
pub fn capture(
    directory: &Path,
    mut scope: BackupScope,
    files: &[BackupFile<'_>],
) -> Result<BackupManifest, String> {
    nostr::PublicKey::from_hex(&scope.actor_pubkey).map_err(|_| "invalid backup actor")?;
    uuid::Uuid::parse_str(&scope.operation_id).map_err(|_| "backup requires an operation UUID")?;
    scope.relay_url = canonical_relay_key(&scope.relay_url);
    url::Url::parse(&scope.relay_url).map_err(|_| "invalid backup community")?;
    if files.is_empty() {
        return Err("custody backup requires source custody".into());
    }
    let mut manifest = BackupManifest {
        schema: "beekeeper-agent-migration-backup/v1".into(),
        scope,
        files: Vec::new(),
    };
    for file in files {
        relative(file.name)?;
        manifest.files.push(entry(file.name, file.bytes));
    }
    manifest.files.sort_by(|a, b| a.name.cmp(&b.name));
    if manifest.files.windows(2).any(|w| w[0].name == w[1].name) {
        return Err("custody backup repeats an artifact name".into());
    }
    directory_path(directory, Path::new(""), true)?;
    let manifest_path = directory.join("manifest.json");
    if manifest_path.exists() {
        let held = verify(directory)?;
        if held != manifest {
            return Err("custody backup conflicts with preserved source; original retained".into());
        }
        return Ok(held);
    }
    for file in files {
        let path = directory.join(relative(file.name)?);
        directory_path(
            directory,
            relative(file.name)?.parent().unwrap_or(Path::new("")),
            true,
        )?;
        if path.exists() {
            restricted(&path)?;
            let held = std::fs::read(&path).map_err(|_| "cannot verify partial backup")?;
            if held != file.bytes {
                return Err("partial backup conflicts with source; original retained".into());
            }
        } else {
            atomic_write::atomic_write_json_restricted(&path, file.bytes)?;
        }
    }
    atomic_write::atomic_write_json_restricted(
        &manifest_path,
        &serde_json::to_vec_pretty(&manifest).map_err(|_| "cannot encode backup manifest")?,
    )?;
    verify(directory)
}

/// Verify restricted custody and every immutable artifact hash before deleting retained source
/// or explicit offline recovery. This never enables a legacy execution owner.
pub fn verify(directory: &Path) -> Result<BackupManifest, String> {
    directory_path(directory, Path::new(""), false)?;
    let path = directory.join("manifest.json");
    restricted(&path)?;
    let manifest: BackupManifest =
        serde_json::from_slice(&std::fs::read(path).map_err(|_| "cannot read backup manifest")?)
            .map_err(|_| "custody backup manifest is malformed")?;
    if manifest.schema != "beekeeper-agent-migration-backup/v1" || manifest.files.is_empty() {
        return Err("unsupported or empty custody backup".into());
    }
    for expected in &manifest.files {
        directory_path(
            directory,
            relative(&expected.name)?.parent().unwrap_or(Path::new("")),
            false,
        )?;
        let path = directory.join(relative(&expected.name)?);
        restricted(&path)?;
        let bytes = std::fs::read(path).map_err(|_| "cannot read backed-up custody")?;
        if entry(&expected.name, &bytes) != *expected {
            return Err(
                "custody backup integrity check failed; custody verification refused".into(),
            );
        }
    }
    Ok(manifest)
}

#[cfg(test)]
#[path = "custody_backup_tests.rs"]
mod tests;
