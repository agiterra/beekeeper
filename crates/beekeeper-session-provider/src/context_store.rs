//! Private on-disk handoff from the relay projector to the context MCP.
//!
//! Packages live under the provider's identity-scoped state directory. They
//! are never published, placed in the agent environment, or mixed with the
//! provider's opaque native-session cursor.

use std::fs::{File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};

use beekeeper_core::coding_session_context::CodingSessionContextPackage;

const CONTEXT_PACKAGE_DIRECTORY: &str = "context-packages";

/// How many package generations one execution keeps on disk.
///
/// The reader only ever serves the newest one it can validate; the older
/// retained files exist so a reader mid-call is never reading a file the
/// pruner has just unlinked.
pub const CONTEXT_PACKAGE_GENERATIONS_RETAINED: usize = 3;

/// Generation written at session open. Every refresh writes a higher sequence.
pub const OPEN_TIME_CONTEXT_PACKAGE_GENERATION: u64 = 0;

/// Failure to validate or persist one private rehydration package.
#[derive(Debug, thiserror::Error)]
pub enum ContextStoreError {
    /// The shared strict package rejected the projected value.
    #[error("context package validation failed: {0}")]
    InvalidPackage(String),
    /// The provider state path could not safely hold a private package.
    #[error("context package storage failed: {0}")]
    Storage(String),
    /// The package directory a refresh was meant to extend is no longer there.
    ///
    /// Only a refresh can see this: generation 0 creates the directory, every
    /// later generation requires it. It means the execution was stopped (or
    /// resumed onto a fresh package) while this refresh was in flight, and the
    /// refresh must be dropped rather than resurrect a directory the operator's
    /// stop already deleted.
    #[error("context package directory is gone")]
    PackageGone,
}

/// Persist a strict package as generation 0 of `package_id`.
///
/// `package_id` is the provider-minted UUID naming this execution's package
/// directory. The returned path is absolute so it can cross the ACP process
/// boundary without depending on either subprocess's working directory.
pub fn write_context_package(
    state_dir: &Path,
    package_id: &str,
    package: &CodingSessionContextPackage,
) -> Result<PathBuf, ContextStoreError> {
    write_context_package_generation(
        state_dir,
        package_id,
        OPEN_TIME_CONTEXT_PACKAGE_GENERATION,
        package,
    )
}

/// Write generation `seq` of one execution's package.
///
/// **Only generation 0 creates directories.** A refresh writes a higher
/// sequence and requires the package directory to already exist, failing with
/// [`ContextStoreError::PackageGone`] when it does not. Without that
/// asymmetry, a refresh whose relay round trip outlived the session's stop
/// would `create_dir_all` the directory that stop had just deleted and write
/// private verified context back into it — with the session→package binding
/// already dropped, nothing short of the next startup sweep would ever remove
/// it again.
///
/// Each generation is opened `create_new(true)` **at its final path**, so the
/// OS-enforced write-once guarantee is per file and survives verbatim: there is
/// no temp-file-plus-`rename(2)` step, because `rename(2)` replaces its
/// destination and would silently repeal that guarantee. A write that fails
/// partway unlinks its own partial file; a process death mid-write leaves a
/// candidate the reader refuses, and the next refresh writes a higher sequence
/// that supersedes it.
pub fn write_context_package_generation(
    state_dir: &Path,
    package_id: &str,
    seq: u64,
    package: &CodingSessionContextPackage,
) -> Result<PathBuf, ContextStoreError> {
    package
        .validate()
        .map_err(ContextStoreError::InvalidPackage)?;
    // Both levels are created and checked: a symlinked `context-packages/`
    // would otherwise be followed silently while the per-package subdirectory
    // it created looked like a perfectly ordinary directory.
    let opening = seq == OPEN_TIME_CONTEXT_PACKAGE_GENERATION;
    let root = context_package_root(state_dir)?;
    if opening {
        std::fs::create_dir_all(&root)
            .map_err(|error| storage_error("cannot create private package directory", error))?;
    } else {
        require_existing_directory(&root)?;
    }
    reject_symlink_or_non_directory(&root)?;
    set_private_directory_permissions(&root)?;
    let directory = package_directory(state_dir, package_id)?;
    if opening {
        std::fs::create_dir_all(&directory)
            .map_err(|error| storage_error("cannot create private package directory", error))?;
    } else {
        require_existing_directory(&directory)?;
    }
    reject_symlink_or_non_directory(&directory)?;
    set_private_directory_permissions(&directory)?;

    // Encoded before the file exists: an encode failure then leaves nothing on
    // disk at all, rather than an empty generation a reader would have to
    // refuse.
    let bytes = serde_json::to_vec(package)
        .map_err(|error| ContextStoreError::Storage(format!("cannot encode package: {error}")))?;
    let path = directory.join(generation_file_name(seq));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    configure_private_file(&mut options);
    let mut file = options
        .open(&path)
        .map_err(|error| storage_error("cannot create private package file", error))?;
    set_private_file_permissions(&file)?;
    if let Err(error) = write_and_sync(&mut file, &bytes) {
        drop(file);
        let _ = std::fs::remove_file(&path);
        return Err(error);
    }
    Ok(path)
}

/// Newest generation sequence present for `package_id`, if any.
///
/// Reports the highest sequence *on disk*, including a corpse left by a
/// process death mid-write, so the next sequence is always above it and a
/// refresh can never collide with a file it cannot see.
pub fn latest_context_package_generation(
    state_dir: &Path,
    package_id: &str,
) -> Result<Option<u64>, ContextStoreError> {
    Ok(read_generations(state_dir, package_id)?
        .into_iter()
        .map(|(seq, _)| seq)
        .max())
}

/// Keep the newest `keep` generations of `package_id`; unlink the rest.
///
/// `keep` of zero is treated as one: a package directory that exists must keep
/// something to serve.
pub fn prune_context_package_generations(
    state_dir: &Path,
    package_id: &str,
    keep: usize,
) -> Result<(), ContextStoreError> {
    let keep = keep.max(1);
    let mut generations = read_generations(state_dir, package_id)?;
    if generations.len() <= keep {
        return Ok(());
    }
    generations.sort_by_key(|(seq, _)| *seq);
    let retire = generations.len() - keep;
    for (_, path) in generations.into_iter().take(retire) {
        std::fs::remove_file(&path)
            .map_err(|error| storage_error("cannot remove superseded package generation", error))?;
    }
    Ok(())
}

/// Remove one package directory entirely.
///
/// A directory that is already gone is success: this runs on the stop path,
/// which must not fail because a cleanup already happened.
pub fn remove_context_packages(
    state_dir: &Path,
    package_id: &str,
) -> Result<(), ContextStoreError> {
    let root = context_package_root(state_dir)?;
    reject_symlink_if_present(&root)?;
    let directory = package_directory(state_dir, package_id)?;
    match std::fs::symlink_metadata(&directory) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(ContextStoreError::Storage(
            "private package directory must be a real directory".into(),
        )),
        Ok(_) => std::fs::remove_dir_all(&directory)
            .map_err(|error| storage_error("cannot remove private package directory", error)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(storage_error(
            "cannot inspect private package directory",
            error,
        )),
    }
}

/// Remove every package directory.
///
/// Startup-only. A provider restart leaves no ACP subprocess alive, so at the
/// moment this runs no reader holds any package here, and the session→package
/// binding that named these directories died with the process.
pub fn remove_all_context_packages(state_dir: &Path) -> Result<(), ContextStoreError> {
    let root = context_package_root(state_dir)?;
    match std::fs::symlink_metadata(&root) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(ContextStoreError::Storage(
            "private package directory must be a real directory".into(),
        )),
        Ok(_) => std::fs::remove_dir_all(&root)
            .map_err(|error| storage_error("cannot sweep private package directories", error)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(storage_error(
            "cannot inspect private package directory",
            error,
        )),
    }
}

/// `<state_dir>/context-packages`.
fn context_package_root(state_dir: &Path) -> Result<PathBuf, ContextStoreError> {
    Ok(absolute_path(state_dir)?.join(CONTEXT_PACKAGE_DIRECTORY))
}

/// `<state_dir>/context-packages/<package_id>`, with `package_id` proven to be
/// a UUID so a path component can never be attacker-shaped.
fn package_directory(state_dir: &Path, package_id: &str) -> Result<PathBuf, ContextStoreError> {
    let package_id = uuid::Uuid::parse_str(package_id)
        .map_err(|_| ContextStoreError::Storage("package id is not a UUID".into()))?;
    Ok(context_package_root(state_dir)?.join(package_id.to_string()))
}

/// Refuse a path that exists and is a symlink. A path that does not exist is
/// fine — the caller is about to treat it as absent.
fn reject_symlink_if_present(path: &Path) -> Result<(), ContextStoreError> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(ContextStoreError::Storage(
            "private package directory must be a real directory".into(),
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(storage_error(
            "cannot inspect private package directory",
            error,
        )),
    }
}

fn generation_file_name(seq: u64) -> String {
    format!("{seq:010}.json")
}

/// Every `<seq>.json` in one package directory, unsorted. A missing directory
/// is an empty list, not an error.
fn read_generations(
    state_dir: &Path,
    package_id: &str,
) -> Result<Vec<(u64, PathBuf)>, ContextStoreError> {
    reject_symlink_if_present(&context_package_root(state_dir)?)?;
    let directory = package_directory(state_dir, package_id)?;
    let entries = match std::fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(storage_error(
                "cannot read private package directory",
                error,
            ))
        }
    };
    let mut generations = Vec::new();
    for entry in entries {
        let entry =
            entry.map_err(|error| storage_error("cannot read private package entry", error))?;
        let path = entry.path();
        let Some(seq) = path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(parse_generation_file_name)
        else {
            continue;
        };
        generations.push((seq, path));
    }
    Ok(generations)
}

fn parse_generation_file_name(name: &str) -> Option<u64> {
    name.strip_suffix(".json")?.parse().ok()
}

fn absolute_path(path: &Path) -> Result<PathBuf, ContextStoreError> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    std::env::current_dir()
        .map(|cwd| cwd.join(path))
        .map_err(|error| storage_error("cannot resolve provider state directory", error))
}

/// A directory a refresh may extend but may never create.
///
/// An absent path is [`ContextStoreError::PackageGone`], not a storage
/// failure: it is the ordinary outcome of a stop that landed while the refresh
/// was fetching, and the caller drops the refresh on it.
fn require_existing_directory(path: &Path) -> Result<(), ContextStoreError> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Err(ContextStoreError::PackageGone)
        }
        Err(error) => Err(storage_error(
            "cannot inspect private package directory",
            error,
        )),
    }
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

/// Persist one private continuity phase under the execution's existing cleanup root.
/// Contains references/digest only, never decrypted payload. Restart does not replay it.
pub fn write_continuity_receipt(
    state_dir: &Path,
    receipt: &crate::continuity_bootstrap::Receipt,
) -> Result<(), ContextStoreError> {
    let root = context_package_root(state_dir)?;
    let opening = receipt.phase == crate::continuity_bootstrap::Phase::Prepared;
    if opening {
        std::fs::create_dir_all(&root)
            .map_err(|e| storage_error("cannot create private receipt root", e))?;
    } else {
        require_existing_directory(&root)?;
    }
    reject_symlink_or_non_directory(&root)?;
    set_private_directory_permissions(&root)?;
    let directory = package_directory(state_dir, &receipt.target.session_id)?;
    if opening {
        std::fs::create_dir_all(&directory)
            .map_err(|e| storage_error("cannot create private receipt directory", e))?;
    } else {
        require_existing_directory(&directory)?;
    }
    reject_symlink_or_non_directory(&directory)?;
    set_private_directory_permissions(&directory)?;
    let command = beekeeper_core::continuity::value_hash(receipt.command_id.as_bytes());
    let phase = match receipt.phase {
        crate::continuity_bootstrap::Phase::Prepared => "prepared",
        crate::continuity_bootstrap::Phase::SubmissionIntent => "submission_intent",
        crate::continuity_bootstrap::Phase::Submitted => "submitted",
        crate::continuity_bootstrap::Phase::Confirmed => "confirmed",
    };
    let path = directory.join(format!(
        "continuity-{}-{command}-{phase}.receipt",
        receipt.target.generation
    ));
    let bytes = serde_json::to_vec(receipt)
        .map_err(|_| ContextStoreError::Storage("cannot encode private receipt".into()))?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    configure_private_file(&mut options);
    let mut file = match options.open(&path) {
        Ok(file) => file,
        // Phase events are advisory snapshots: the actor already persisted the
        // evidence before sending them. Accept only an identical durable copy.
        Err(error) if !opening && error.kind() == std::io::ErrorKind::AlreadyExists => {
            return if private_bytes(&path)? == bytes {
                Ok(())
            } else {
                Err(ContextStoreError::Storage("private receipt differs".into()))
            };
        }
        Err(error) => return Err(storage_error("cannot create private receipt", error)),
    };
    set_private_file_permissions(&file)?;
    if let Err(error) = write_and_sync(&mut file, &bytes) {
        drop(file);
        let _ = std::fs::remove_file(path);
        return Err(error);
    }
    // Persist the new directory entry too: a successful file sync alone does
    // not make the phase discoverable after a filesystem restart.
    for parent in [&directory, &root] {
        File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|_| {
                ContextStoreError::Storage("private receipt durability unavailable".into())
            })?;
    }
    Ok(())
}

const CONTINUITY_ARCHIVE_MAX_BYTES: usize = 256 * 1024;
fn continuity_paths(
    state_dir: &Path,
    session: &str,
) -> Result<(PathBuf, PathBuf), ContextStoreError> {
    let session = uuid::Uuid::parse_str(session)
        .map_err(|_| ContextStoreError::Storage("invalid private archive identity".into()))?;
    let root = absolute_path(state_dir)?.join("continuity-private");
    let markers = absolute_path(state_dir)?.join("continuity-required");
    reject_symlink_if_present(&root)?;
    reject_symlink_if_present(&markers)?;
    Ok((
        markers.join(format!("{session}.required")),
        root.join(format!("{session}.archive")),
    ))
}
fn private_bytes(path: &Path) -> Result<Vec<u8>, ContextStoreError> {
    let meta = std::fs::symlink_metadata(path)
        .map_err(|_| ContextStoreError::Storage("private continuity archive unavailable".into()))?;
    if !meta.is_file()
        || meta.file_type().is_symlink()
        || meta.len() > CONTINUITY_ARCHIVE_MAX_BYTES as u64
    {
        return Err(ContextStoreError::Storage(
            "private continuity archive invalid".into(),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o077 != 0 {
            return Err(ContextStoreError::Storage(
                "private continuity archive permissions unsafe".into(),
            ));
        }
    }
    std::fs::read(path)
        .map_err(|_| ContextStoreError::Storage("private continuity archive unreadable".into()))
}
/// An absent marker is an ordinary conversation; an existing marker with a
/// missing/bad archive is always a refusal. This root survives package cleanup
/// and startup sweeps because native history remains resumable after disconnect.
pub(crate) fn read_continuity_archive(
    state_dir: &Path,
    session: &str,
) -> Result<Option<crate::continuity_archive::Archive>, ContextStoreError> {
    let (marker, path) = continuity_paths(state_dir, session)?;
    match std::fs::symlink_metadata(&marker) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if path.exists() {
                return Err(ContextStoreError::Storage(
                    "private continuity marker missing".into(),
                ));
            }
            return Ok(None);
        }
        Err(_) => {
            return Err(ContextStoreError::Storage(
                "private continuity marker unavailable".into(),
            ))
        }
        Ok(_) => {}
    }
    let marker: crate::continuity_archive::Marker =
        serde_json::from_slice(&private_bytes(&marker)?)
            .map_err(|_| ContextStoreError::Storage("private continuity marker invalid".into()))?;
    let archive_bytes = private_bytes(&path)?;
    if beekeeper_core::continuity::value_hash(&archive_bytes) != marker.digest {
        return Err(ContextStoreError::Storage(
            "private continuity archive digest mismatch".into(),
        ));
    }
    let archive: crate::continuity_archive::Archive = serde_json::from_slice(&archive_bytes)
        .map_err(|_| ContextStoreError::Storage("private continuity archive invalid".into()))?;
    if marker.binding != archive.binding || marker.binding.target.session_id != session {
        return Err(ContextStoreError::Storage(
            "private continuity archive binding mismatch".into(),
        ));
    }
    let mut translator = crate::transcript::TranscriptTranslator::new(false);
    translator
        .register_continuity_fragments(&archive.fragments)
        .map_err(|_| {
            ContextStoreError::Storage("private continuity archive bounds invalid".into())
        })?;
    if archive.fragments.is_empty() {
        return Err(ContextStoreError::Storage(
            "private continuity archive empty".into(),
        ));
    }
    Ok(Some(archive))
}
fn replace_private(path: &Path, bytes: &[u8]) -> Result<(), ContextStoreError> {
    if bytes.len() > CONTINUITY_ARCHIVE_MAX_BYTES {
        return Err(ContextStoreError::Storage(
            "private continuity archive exceeds bound".into(),
        ));
    }
    reject_symlink_if_present(path)?;
    let tmp = path.with_extension(format!("tmp-{}", uuid::Uuid::new_v4()));
    let mut opts = OpenOptions::new();
    opts.write(true).create_new(true);
    configure_private_file(&mut opts);
    let mut file = opts.open(&tmp).map_err(|_| {
        ContextStoreError::Storage("private continuity archive write unavailable".into())
    })?;
    let result = write_and_sync(&mut file, bytes).and_then(|()| {
        std::fs::rename(&tmp, path).map_err(|_| {
            ContextStoreError::Storage("private continuity archive replace unavailable".into())
        })
    });
    if result.is_err() {
        let _ = std::fs::remove_file(tmp);
    }
    result
}
/// Archive before any submission. A failed/interrupted replacement leaves a
/// marker/archive mismatch and makes restore refuse rather than lose protection.
pub(crate) fn write_continuity_archive(
    state_dir: &Path,
    archive: &crate::continuity_archive::Archive,
) -> Result<(), ContextStoreError> {
    let (marker, path) = continuity_paths(state_dir, &archive.binding.target.session_id)?;
    let roots = [marker.parent(), path.parent()];
    for root in roots {
        let root = root
            .ok_or_else(|| ContextStoreError::Storage("private continuity root invalid".into()))?;
        std::fs::create_dir_all(root).map_err(|_| {
            ContextStoreError::Storage("private continuity root unavailable".into())
        })?;
        reject_symlink_or_non_directory(root)?;
        set_private_directory_permissions(root)?;
    }
    let mut translator = crate::transcript::TranscriptTranslator::new(false);
    translator
        .register_continuity_fragments(&archive.fragments)
        .map_err(|_| {
            ContextStoreError::Storage("private continuity archive bounds invalid".into())
        })?;
    let bytes = serde_json::to_vec(archive).map_err(|_| {
        ContextStoreError::Storage("private continuity archive encoding failed".into())
    })?;
    let marker_value = crate::continuity_archive::Marker {
        binding: archive.binding.clone(),
        digest: beekeeper_core::continuity::value_hash(&bytes),
    };
    let marker_bytes = serde_json::to_vec(&marker_value).map_err(|_| {
        ContextStoreError::Storage("private continuity marker encoding failed".into())
    })?;
    if bytes.len() > CONTINUITY_ARCHIVE_MAX_BYTES {
        return Err(ContextStoreError::Storage(
            "private continuity archive exceeds bound".into(),
        ));
    }
    replace_private(&marker, &marker_bytes)?;
    replace_private(&path, &bytes)?;
    for root in roots {
        let root = root
            .ok_or_else(|| ContextStoreError::Storage("private continuity root invalid".into()))?;
        File::open(root)
            .and_then(|directory| directory.sync_all())
            .map_err(|_| {
                ContextStoreError::Storage("private continuity durability unavailable".into())
            })?;
    }
    Ok(())
}
/// Remove only when the execution is no longer resumable, never on disconnect.
pub(crate) fn remove_continuity_archive(
    state_dir: &Path,
    session: &str,
) -> Result<(), ContextStoreError> {
    let (marker, path) = continuity_paths(state_dir, session)?;
    for path in [path, marker] {
        reject_symlink_if_present(&path)?;
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => {
                return Err(ContextStoreError::Storage(
                    "private continuity cleanup unavailable".into(),
                ))
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt as _;

    use beekeeper_core::coding_session_context::{
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
                complete_as_of: Some(1),
                complete: true,
                truncated: false,
                source_event_count: 1,
                included_history_items: 0,
                omitted_history_items: 0,
                total_history_items: Some(0),
                source_event_breakdown: None,
                notes: vec!["Complete empty fixture".into()],
            },
            history: Vec::new(),
            roster: Vec::new(),
            inbox: Vec::new(),
            policy: None,
        }
    }

    fn generation_paths(dir: &Path, package_id: &str) -> Vec<u64> {
        let mut sequences: Vec<u64> = read_generations(dir, package_id)
            .expect("read generations")
            .into_iter()
            .map(|(seq, _)| seq)
            .collect();
        sequences.sort_unstable();
        sequences
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

    #[test]
    fn a_refresh_writes_a_new_generation_and_never_overwrites_one() {
        let dir = tempfile::tempdir().unwrap();
        let package_id = uuid::Uuid::new_v4().to_string();
        let first = write_context_package(dir.path(), &package_id, &empty_package()).unwrap();
        let second =
            write_context_package_generation(dir.path(), &package_id, 1, &empty_package()).unwrap();
        assert_ne!(first, second);
        assert!(first.exists(), "the open-time generation still serves");
        assert_eq!(generation_paths(dir.path(), &package_id), vec![0, 1]);
        assert_eq!(
            latest_context_package_generation(dir.path(), &package_id).unwrap(),
            Some(1)
        );
    }

    #[test]
    fn writing_the_same_generation_sequence_twice_errors() {
        let dir = tempfile::tempdir().unwrap();
        let package_id = uuid::Uuid::new_v4().to_string();
        // Generation 0 opens the package; only it may create the directory.
        write_context_package(dir.path(), &package_id, &empty_package()).unwrap();
        write_context_package_generation(dir.path(), &package_id, 7, &empty_package()).unwrap();
        assert!(
            write_context_package_generation(dir.path(), &package_id, 7, &empty_package()).is_err(),
            "create_new at the final path is what keeps the write-once guarantee"
        );
    }

    #[test]
    fn the_next_sequence_is_above_a_corrupt_generation_left_by_a_crash() {
        let dir = tempfile::tempdir().unwrap();
        let package_id = uuid::Uuid::new_v4().to_string();
        write_context_package(dir.path(), &package_id, &empty_package()).unwrap();
        let corpse =
            write_context_package_generation(dir.path(), &package_id, 1, &empty_package()).unwrap();
        std::fs::write(&corpse, b"{\"v\":2,\"session\"").unwrap();

        let latest = latest_context_package_generation(dir.path(), &package_id).unwrap();
        assert_eq!(latest, Some(1), "a corpse still counts as present on disk");
        write_context_package_generation(
            dir.path(),
            &package_id,
            latest.unwrap_or_default() + 1,
            &empty_package(),
        )
        .unwrap();
        assert_eq!(generation_paths(dir.path(), &package_id), vec![0, 1, 2]);
    }

    #[test]
    fn pruning_keeps_the_newest_generations_and_removes_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let package_id = uuid::Uuid::new_v4().to_string();
        for seq in 0..6 {
            write_context_package_generation(dir.path(), &package_id, seq, &empty_package())
                .unwrap();
        }
        prune_context_package_generations(
            dir.path(),
            &package_id,
            CONTEXT_PACKAGE_GENERATIONS_RETAINED,
        )
        .unwrap();
        assert_eq!(generation_paths(dir.path(), &package_id), vec![3, 4, 5]);
    }

    #[test]
    fn stopping_an_execution_removes_its_package_directory() {
        let dir = tempfile::tempdir().unwrap();
        let package_id = uuid::Uuid::new_v4().to_string();
        let kept = uuid::Uuid::new_v4().to_string();
        write_context_package(dir.path(), &package_id, &empty_package()).unwrap();
        write_context_package(dir.path(), &kept, &empty_package()).unwrap();

        remove_context_packages(dir.path(), &package_id).unwrap();
        assert!(generation_paths(dir.path(), &package_id).is_empty());
        assert_eq!(generation_paths(dir.path(), &kept), vec![0]);
        remove_context_packages(dir.path(), &package_id)
            .expect("removing an already-removed directory is success");
    }

    /// A refresh whose relay round trip outlived the stop must not resurrect
    /// the directory that stop deleted: the session→package binding is already
    /// gone, so anything written back there is unreachable private context
    /// that survives until the next startup sweep.
    #[test]
    fn a_refresh_never_recreates_a_package_directory_that_cleanup_removed() {
        let dir = tempfile::tempdir().unwrap();
        let package_id = uuid::Uuid::new_v4().to_string();
        write_context_package(dir.path(), &package_id, &empty_package()).unwrap();

        // The stop lands between the refresh's projection and its write.
        remove_context_packages(dir.path(), &package_id).unwrap();

        let error = write_context_package_generation(dir.path(), &package_id, 1, &empty_package())
            .expect_err("a refresh into a discarded package must fail");
        assert!(
            matches!(error, ContextStoreError::PackageGone),
            "unexpected error: {error}"
        );
        assert!(
            !dir.path()
                .join(CONTEXT_PACKAGE_DIRECTORY)
                .join(&package_id)
                .exists(),
            "the refresh recreated a directory nothing will ever remove"
        );
        assert!(generation_paths(dir.path(), &package_id).is_empty());
    }

    /// The whole state directory can vanish too — a swept root is the same
    /// fact, not a storage failure to retry.
    #[test]
    fn a_refresh_after_a_full_sweep_reports_the_package_as_gone() {
        let dir = tempfile::tempdir().unwrap();
        let package_id = uuid::Uuid::new_v4().to_string();
        write_context_package(dir.path(), &package_id, &empty_package()).unwrap();
        remove_all_context_packages(dir.path()).unwrap();

        let error = write_context_package_generation(dir.path(), &package_id, 7, &empty_package())
            .expect_err("a refresh into a swept root must fail");
        assert!(
            matches!(error, ContextStoreError::PackageGone),
            "unexpected error: {error}"
        );
        assert!(!dir.path().join(CONTEXT_PACKAGE_DIRECTORY).exists());
    }

    #[test]
    fn a_startup_sweep_removes_every_leftover_package_directory() {
        let dir = tempfile::tempdir().unwrap();
        let first = uuid::Uuid::new_v4().to_string();
        let second = uuid::Uuid::new_v4().to_string();
        write_context_package(dir.path(), &first, &empty_package()).unwrap();
        write_context_package(dir.path(), &second, &empty_package()).unwrap();

        remove_all_context_packages(dir.path()).unwrap();
        assert!(!dir.path().join(CONTEXT_PACKAGE_DIRECTORY).exists());
        remove_all_context_packages(dir.path()).expect("sweeping an empty state dir is success");
    }

    #[cfg(unix)]
    #[test]
    fn the_generation_directory_is_0700_and_every_generation_file_is_0600() {
        let dir = tempfile::tempdir().unwrap();
        let package_id = uuid::Uuid::new_v4().to_string();
        let path = write_context_package(dir.path(), &package_id, &empty_package()).unwrap();
        let directory = path.parent().expect("generation parent");
        assert_eq!(
            std::fs::metadata(directory).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(directory.parent().expect("packages root"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
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
