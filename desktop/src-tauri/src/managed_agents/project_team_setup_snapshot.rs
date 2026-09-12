//! Content-addressed setup candidates. Stored files remain local files: the
//! digest is an identity, not a filesystem security boundary. Every later
//! consumer must verify the manifest, bytes and pack structure again.

use super::{read_draft, tree, ProjectTeamSetupDraft, SetupError, SetupScope};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

#[cfg(test)]
#[path = "project_team_setup_snapshot_tests.rs"]
mod tests;

const SCHEMA: &str = "project-team-setup-snapshot/v1";
// Match the draft tree's bounded-content contract, including directories.
const MAX_FILE_BYTES: u64 = 256 * 1024;
const MAX_TREE_BYTES: usize = 8 * 1024 * 1024;
const MAX_ENTRIES: usize = 2048;
const MAX_DEPTH: usize = 24;
const MAX_MANIFEST_BYTES: u64 = 4 * 1024 * 1024;
static SNAPSHOT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// A verified candidate. This does not imply publication or adoption.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTeamSetupSnapshot {
    pub setup_id: String,
    /// SHA-256 of the canonical manifest, which binds every role-file byte.
    pub snapshot_id: String,
    pub roles_directory: String,
    pub manifest_path: String,
    pub roles: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct FileEntry {
    path: String,
    length: usize,
    sha256: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: String,
    files: Vec<FileEntry>,
}

struct Capture {
    manifest: Manifest,
    contents: Vec<Vec<u8>>,
}

fn invalid(message: impl Into<String>) -> SetupError {
    SetupError::new("invalid_snapshot", message)
}

fn hash(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn read_file(path: &Path, limit: u64) -> Result<Vec<u8>, SetupError> {
    tree::check_regular_file(path, limit)?;
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err(invalid("Snapshot input is not a bounded regular file."));
    }
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(invalid("A snapshot input grew past its size limit."));
    }
    Ok(bytes)
}

fn portable_component(name: &str) -> bool {
    let base = name.split('.').next().unwrap_or_default().to_lowercase();
    let device = matches!(
        base.as_str(),
        "con" | "prn" | "aux" | "nul" | "conin$" | "conout$"
    ) || ["com", "lpt"].iter().any(|prefix| {
        base.strip_prefix(prefix)
            .is_some_and(|suffix| !suffix.is_empty() && suffix.chars().all(char::is_numeric))
    });
    !name.is_empty()
        && !name.ends_with(['.', ' '])
        && !name.eq_ignore_ascii_case(".git")
        && !name.contains(['/', '\\', ':', '<', '>', '"', '|', '?', '*'])
        && !name.chars().any(char::is_control)
        && !device
}

fn portable_path(relative: &Path) -> Result<String, SetupError> {
    let mut names = Vec::new();
    for component in relative.components() {
        let std::path::Component::Normal(name) = component else {
            return Err(invalid("Snapshot file paths cannot contain traversal."));
        };
        let name = name
            .to_str()
            .ok_or_else(|| invalid("Snapshot file names must be UTF-8."))?;
        if !portable_component(name) {
            return Err(invalid(
                "Snapshot paths must be portable role-file paths without Git metadata.",
            ));
        }
        names.push(name);
    }
    let path = names.join("/");
    if path.is_empty() || path.len() > 1024 {
        return Err(invalid("A snapshot path is empty or too long."));
    }
    Ok(path)
}

fn check_collision(path: &str, seen: &mut HashMap<String, String>) -> Result<(), SetupError> {
    if seen
        .insert(path.to_lowercase(), path.to_owned())
        .is_some_and(|other| other != path)
    {
        return Err(invalid("Snapshot paths differ only in letter case."));
    }
    Ok(())
}

fn paths(root: &Path, reject_empty: bool) -> Result<Vec<PathBuf>, SetupError> {
    fn visit(
        root: &Path,
        path: &Path,
        count: &mut usize,
        depth: usize,
        files: &mut Vec<PathBuf>,
        reject_empty: bool,
        seen: &mut HashMap<String, String>,
    ) -> Result<(), SetupError> {
        *count += 1;
        if *count > MAX_ENTRIES || depth > MAX_DEPTH {
            return Err(invalid("The snapshot exceeds the entry or depth limit."));
        }
        if path != root {
            let relative = path
                .strip_prefix(root)
                .map_err(|_| invalid("A snapshot file escaped its root."))?;
            check_collision(&portable_path(relative)?, seen)?;
        }
        let metadata = std::fs::symlink_metadata(path)?;
        if metadata.file_type().is_symlink() {
            return Err(invalid("Symlinks are not allowed in setup snapshots."));
        }
        if metadata.is_dir() {
            let mut entries = std::fs::read_dir(path)?.peekable();
            if reject_empty && entries.peek().is_none() {
                return Err(invalid(
                    "The snapshot contains an unmanifested empty directory.",
                ));
            }
            for entry in entries {
                visit(
                    root,
                    &entry?.path(),
                    count,
                    depth + 1,
                    files,
                    reject_empty,
                    seen,
                )?;
            }
        } else {
            tree::check_regular_file(path, MAX_FILE_BYTES)?;
            let relative = path
                .strip_prefix(root)
                .map_err(|_| invalid("A snapshot file escaped its root."))?;
            files.push(relative.to_path_buf());
        }
        Ok(())
    }
    tree::ensure_contained_directory(root, root)?;
    let mut files = Vec::new();
    visit(
        root,
        root,
        &mut 0,
        0,
        &mut files,
        reject_empty,
        &mut HashMap::new(),
    )?;
    files.sort_by_cached_key(|path| path.to_string_lossy().replace('\\', "/"));
    Ok(files)
}

fn capture(root: &Path, reject_empty: bool) -> Result<Capture, SetupError> {
    let mut files = Vec::new();
    let mut contents = Vec::new();
    let mut size = 0;
    for relative in paths(root, reject_empty)? {
        let path = root.join(&relative);
        let parent = path
            .parent()
            .ok_or_else(|| invalid("Missing file parent."))?;
        tree::ensure_contained_directory(root, parent)?;
        let bytes = read_file(&path, MAX_FILE_BYTES)?;
        size += bytes.len();
        if size > MAX_TREE_BYTES {
            return Err(invalid("The snapshot exceeds the 8 MiB size limit."));
        }
        files.push(FileEntry {
            path: portable_path(&relative)?,
            length: bytes.len(),
            sha256: hash(&bytes),
        });
        contents.push(bytes);
    }
    Ok(Capture {
        manifest: Manifest {
            schema: SCHEMA.to_owned(),
            files,
        },
        contents,
    })
}

fn manifest_bytes(manifest: &Manifest) -> Result<Vec<u8>, SetupError> {
    let bytes = serde_json::to_vec(manifest).map_err(|error| invalid(error.to_string()))?;
    if bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(invalid("The snapshot manifest exceeds its size limit."));
    }
    Ok(bytes)
}

fn validate_roles(record: &ProjectTeamSetupDraft, roles: &Path) -> Result<Vec<String>, SetupError> {
    let mut copied = record.clone();
    copied.draft_directory = roles.to_string_lossy().into_owned();
    copied.roles_directory = copied.draft_directory.clone();
    let validation = tree::validate(&copied);
    if !validation.valid {
        return Err(invalid(
            validation
                .diagnostics
                .into_iter()
                .filter(|diagnostic| diagnostic.level == "error")
                .map(|diagnostic| diagnostic.message)
                .collect::<Vec<_>>()
                .join("; "),
        ));
    }
    Ok(validation.roles)
}

fn storage(record: &ProjectTeamSetupDraft) -> Result<PathBuf, SetupError> {
    let directory = Path::new(&record.draft_directory)
        .parent()
        .ok_or_else(|| invalid("The draft has no scoped parent directory."))?;
    tree::ensure_contained_directory(directory, directory)?;
    Ok(directory.join("snapshots"))
}

fn verify(
    record: &ProjectTeamSetupDraft,
    id: &str,
) -> Result<ProjectTeamSetupSnapshot, SetupError> {
    if id.len() != 64
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(invalid("A snapshot ID must be a lowercase SHA-256 digest."));
    }
    let root = storage(record)?;
    let directory = root.join(id);
    tree::ensure_contained_directory(&root, &directory)?;
    let mut names = std::fs::read_dir(&directory)?
        .take(3)
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<Result<Vec<_>, _>>()?;
    names.sort();
    if names
        != [
            std::ffi::OsString::from("manifest.json"),
            std::ffi::OsString::from("roles"),
        ]
    {
        return Err(invalid(
            "The snapshot contains unmanifested files or directories.",
        ));
    }
    let manifest_path = directory.join("manifest.json");
    let bytes = read_file(&manifest_path, MAX_MANIFEST_BYTES)?;
    let expected: Manifest = serde_json::from_slice(&bytes)
        .map_err(|error| invalid(format!("Invalid snapshot manifest: {error}")))?;
    if expected.schema != SCHEMA || hash(&bytes) != id || manifest_bytes(&expected)? != bytes {
        return Err(invalid(
            "The snapshot manifest no longer matches its content address.",
        ));
    }
    let roles = directory.join("roles");
    let actual = capture(&roles, true)?;
    if actual.manifest != expected {
        return Err(invalid(
            "The snapshot files no longer match their manifest.",
        ));
    }
    let role_names = validate_roles(record, &roles)?;
    // Pack validation performs its own reads. Refuse changes during those reads.
    if capture(&roles, true)?.manifest != expected {
        return Err(invalid("The snapshot changed during validation."));
    }
    Ok(ProjectTeamSetupSnapshot {
        setup_id: record.setup_id.clone(),
        snapshot_id: id.to_owned(),
        roles_directory: roles.to_string_lossy().into_owned(),
        manifest_path: manifest_path.to_string_lossy().into_owned(),
        roles: role_names,
    })
}

fn sync_directory(path: &Path) -> Result<(), SetupError> {
    // Unix needs directory fsync for durable names after file creation/rename.
    // std offers no portable directory flush on Windows; native Windows
    // crash-durability remains unverified, rather than a claimed guarantee.
    #[cfg(unix)]
    std::fs::File::open(path)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn sync_candidate_directories(roles: &Path, manifest: &Manifest) -> Result<(), SetupError> {
    let mut directories = BTreeSet::from([roles.to_path_buf()]);
    for entry in &manifest.files {
        let path = roles.join(&entry.path);
        let mut parent = path.parent();
        while let Some(directory) = parent.filter(|directory| directory.starts_with(roles)) {
            directories.insert(directory.to_path_buf());
            parent = directory.parent();
        }
    }
    for directory in directories.iter().rev() {
        sync_directory(directory)?;
    }
    Ok(())
}

fn create(record: &ProjectTeamSetupDraft) -> Result<ProjectTeamSetupSnapshot, SetupError> {
    let source = Path::new(&record.roles_directory);
    tree::ensure_contained_directory(Path::new(&record.draft_directory), source)?;
    let captured = capture(source, false)?;
    let bytes = manifest_bytes(&captured.manifest)?;
    let id = hash(&bytes);
    let root = storage(record)?;
    tree::create_private_directory(&root)?;
    if root.join(&id).try_exists()? {
        return verify(record, &id);
    }
    let temporary = tempfile::Builder::new()
        .prefix(".snapshot-")
        .tempdir_in(&root)?;
    let roles = temporary.path().join("roles");
    std::fs::create_dir(&roles)?;
    for (entry, content) in captured.manifest.files.iter().zip(captured.contents.iter()) {
        let path = roles.join(&entry.path);
        let parent = path
            .parent()
            .ok_or_else(|| invalid("Missing snapshot file parent."))?;
        std::fs::create_dir_all(parent)?;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        file.write_all(content)?;
        file.sync_all()?;
    }
    validate_roles(record, &roles)?;
    if capture(&roles, true)?.manifest != captured.manifest
        || capture(source, false)?.manifest != captured.manifest
    {
        return Err(invalid("The draft changed while its snapshot was being prepared. Retry after authoring finishes."));
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temporary.path().join("manifest.json"))?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    sync_candidate_directories(&roles, &captured.manifest)?;
    sync_directory(temporary.path())?;
    match std::fs::rename(temporary.path(), root.join(&id)) {
        Ok(()) => {}
        Err(error) if root.join(&id).try_exists()? => {
            // Another process may have saved these exact bytes. Verify its
            // complete candidate; never replace or repair unknown content.
            return verify(record, &id).map_err(|_| {
                invalid(format!(
                    "A competing snapshot could not be verified after rename failed: {error}"
                ))
            });
        }
        Err(error) => return Err(error.into()),
    }
    // Flush the new candidate entry and the scoped storage ancestry created
    // by preparation (snapshots, scope, setup storage, app data parent).
    for directory in root.ancestors().take(4) {
        sync_directory(directory)?;
    }
    verify(record, &id)
}

/// Capture or reverify a candidate using a trusted host storage root and scope.
/// The setup ID binds retries; paths are derived from the preserved record.
pub(super) fn run(
    root: &Path,
    scope: &SetupScope,
    setup_id: &str,
    snapshot_id: Option<&str>,
) -> Result<ProjectTeamSetupSnapshot, SetupError> {
    let _guard = SNAPSHOT_LOCK
        .lock()
        .map_err(|error| invalid(error.to_string()))?;
    let record =
        read_draft(root, scope)?.ok_or_else(|| invalid("Prepare a project setup draft first."))?;
    if record.setup_id != setup_id {
        return Err(invalid("The setup ID does not match the scoped draft."));
    }
    match snapshot_id {
        Some(id) => verify(&record, id),
        None => {
            let _lock = process_lock(&record)?;
            let candidate = create(&record)?;
            save_latest(&record, &candidate.snapshot_id)?;
            Ok(candidate)
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct LatestSnapshot {
    setup_id: String,
    snapshot_id: String,
}

fn valid_id(id: &str) -> bool {
    id.len() == 64
        && id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn scoped_file(record: &ProjectTeamSetupDraft, name: &str) -> Result<PathBuf, SetupError> {
    Ok(storage(record)?
        .parent()
        .ok_or_else(|| invalid("Missing scoped storage directory."))?
        .join(name))
}

/// Read only the saved pointer; callers must reverify its candidate before use.
pub(super) fn latest(record: &ProjectTeamSetupDraft) -> Result<Option<String>, SetupError> {
    let path = scoped_file(record, "latest-snapshot.json")?;
    match std::fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
        Ok(_) => {}
    }
    let pointer: LatestSnapshot = serde_json::from_slice(&read_file(&path, 4096)?)
        .map_err(|error| invalid(format!("Could not read saved snapshot pointer: {error}")))?;
    if pointer.setup_id != record.setup_id || !valid_id(&pointer.snapshot_id) {
        return Err(invalid(
            "The saved snapshot pointer does not match this setup.",
        ));
    }
    Ok(Some(pointer.snapshot_id))
}

fn process_lock(record: &ProjectTeamSetupDraft) -> Result<std::fs::File, SetupError> {
    let path = scoped_file(record, "snapshot.lock")?;
    match std::fs::symlink_metadata(&path) {
        Ok(_) => tree::check_regular_file(&path, 0)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let mut options = std::fs::OpenOptions::new();
    options.create(true).truncate(false).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(path)?;
    if !file.metadata()?.is_file() {
        return Err(invalid("Snapshot lock must be a regular file."));
    }
    file.lock()?;
    Ok(file)
}

fn save_latest(record: &ProjectTeamSetupDraft, id: &str) -> Result<(), SetupError> {
    let path = scoped_file(record, "latest-snapshot.json")?;
    // Refuse corrupt or foreign metadata rather than silently overwriting it.
    if latest(record)?.as_deref() == Some(id) {
        return Ok(());
    }
    let parent = path
        .parent()
        .ok_or_else(|| invalid("Missing pointer parent."))?;
    let bytes = serde_json::to_vec(&LatestSnapshot {
        setup_id: record.setup_id.clone(),
        snapshot_id: id.to_owned(),
    })
    .map_err(|error| invalid(error.to_string()))?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(&bytes)?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(&path)
        .map_err(|error| invalid(error.to_string()))?;
    sync_directory(parent)?;
    Ok(())
}
