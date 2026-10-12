//! Bounded, read-only selection of native history for one verified import.
//!
//! Paths and hashes are private custody metadata, not public relay records.
//! Only an explicitly selected runtime configuration home is inspected. This
//! module does not resume a session or copy unrelated account history.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
};

#[path = "native_artifacts_claude.rs"]
mod claude;

/// One exact native artifact selected by cursor and workspace provenance.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeArtifact {
    /// Existing source file. Keep this path in restricted local storage only.
    pub source_path: PathBuf,
    /// Runtime-home-relative destination; never contains parent traversal.
    pub relative_path: PathBuf,
    /// Exact captured byte count.
    pub bytes: u64,
    /// SHA-256 of the selected artifact, without exporting transcript contents.
    pub sha256: String,
}

/// Select one supported native transcript using actual child launch facts.
///
/// Read-only preflight may run against a separately verified live generation.
/// Admission requires a proven drain and rechecked hashes before copying into
/// the restricted backup. The supplied environment must identify the exact
/// configuration home; this reader never derives one from its own HOME.
/// Unsupported runtime formats require explicit repair.
pub fn resolve(
    command: &str,
    environment: &BTreeMap<String, String>,
    cursor: &str,
    cwd: &Path,
) -> Result<Vec<NativeArtifact>, String> {
    resolve_selected(command, environment, cursor, Some(cwd)).map(|(artifacts, _)| artifacts)
}

/// Recover the workspace only from the exact selected transcript's session_meta.
/// Callers must separately prove actor/generation custody and the explicit
/// configuration home. This never guesses a desktop layout or scans history.
pub fn resolve_workspace(
    command: &str,
    environment: &BTreeMap<String, String>,
    cursor: &str,
) -> Result<(Vec<NativeArtifact>, PathBuf), String> {
    resolve_selected(command, environment, cursor, None)
}

/// Return the selected transcript's first verified provenance timestamp.
/// Callers supply only selected hash-verified private bytes, never global history.
/// Codex metadata and Claude transcript rows retain their own vendor schemas.
pub fn source_timestamp(
    runtime: &str,
    bytes: &[u8],
    cursor: &str,
    cwd: &Path,
) -> Result<String, String> {
    if runtime == "claude" {
        return claude::provenance(bytes, cursor, Some(cwd)).map(|(_, timestamp, _)| timestamp);
    }
    if runtime != "codex" || bytes.len() > 64 * 1024 * 1024 {
        return Err("native source timestamp format unsupported".into());
    }
    let row: serde_json::Value = serde_json::from_slice(
        bytes
            .split(|byte| *byte == b'\n')
            .next()
            .ok_or("native source header missing")?,
    )
    .map_err(|_| "native source header malformed")?;
    let payload = row.get("payload").ok_or("native source metadata missing")?;
    let expected_cwd = cwd
        .canonicalize()
        .map_err(|_| "native source workspace unavailable")?;
    if row.get("type").and_then(serde_json::Value::as_str) != Some("session_meta")
        || payload.get("id").and_then(serde_json::Value::as_str) != Some(cursor)
        || payload
            .get("cwd")
            .and_then(serde_json::Value::as_str)
            .map(Path::new)
            .and_then(|cwd| cwd.canonicalize().ok())
            != Some(expected_cwd)
    {
        return Err("native source timestamp provenance mismatch".into());
    }
    let timestamp = row
        .get("timestamp")
        .and_then(serde_json::Value::as_str)
        .ok_or("native source timestamp missing")?;
    let instant = chrono::DateTime::parse_from_rfc3339(timestamp)
        .map_err(|_| "native source timestamp malformed")?;
    if instant.timestamp() < 0 {
        return Err("native source timestamp precedes epoch".into());
    }
    Ok(timestamp.into())
}

/// Identify only adapter families whose native transcript formats are verified.
/// This never infers a vendor from whichever account environment happens to exist.
pub fn runtime_name(command: &str) -> Result<&'static str, String> {
    let name = Path::new(command)
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("native runtime command unavailable")?;
    if matches!(
        name,
        "claude" | "claude-agent-acp" | "claude-code-acp" | "claude-agent-acp.js"
    ) || Path::new(command).components().any(|part| {
        matches!(
            part.as_os_str().to_str(),
            Some("claude-agent-acp" | "claude-code-acp")
        )
    }) {
        return Ok("claude");
    }
    if matches!(name, "codex" | "codex-acp" | "codex-acp.js")
        || Path::new(command)
            .components()
            .any(|part| part.as_os_str() == "codex-acp")
    {
        return Ok("codex");
    }
    Err("NATIVE_ARTIFACT_UNSUPPORTED: runtime transcript format is not verified".into())
}

fn resolve_selected(
    command: &str,
    environment: &BTreeMap<String, String>,
    cursor: &str,
    cwd: Option<&Path>,
) -> Result<(Vec<NativeArtifact>, PathBuf), String> {
    let cursor =
        uuid::Uuid::parse_str(cursor).map_err(|_| "native cursor is not a supported UUID")?;
    let cursor = cursor.hyphenated().to_string();
    if runtime_name(command)? == "claude" {
        return claude::resolve(environment, &cursor, cwd);
    }
    let home = environment
        .get("CODEX_HOME")
        .map(PathBuf::from)
        .ok_or("NATIVE_ARTIFACT_UNAVAILABLE: exact CODEX_HOME was not exported")?;
    if !home.is_absolute() {
        return Err("native configuration home must be absolute".into());
    }
    directory(&home)?;
    let home = home
        .canonicalize()
        .map_err(|_| "native configuration home unavailable")?;
    let sessions = home.join("sessions");
    directory(&sessions)?;
    let mut candidates = Vec::new();
    let mut visited = 0usize;
    for year in date_directories(&sessions, 4, &mut visited)? {
        for month in date_directories(&year, 2, &mut visited)? {
            for day in date_directories(&month, 2, &mut visited)? {
                for entry in
                    std::fs::read_dir(&day).map_err(|_| "native transcript directory unreadable")?
                {
                    visited += 1;
                    if visited > 100_000 {
                        return Err(
                            "native artifact selection exceeds bounded directory budget".into()
                        );
                    }
                    let entry = entry.map_err(|_| "native transcript entry unavailable")?;
                    let name = entry.file_name();
                    let Some(name) = name.to_str() else { continue };
                    if name.starts_with("rollout-") && name.ends_with(&format!("-{cursor}.jsonl")) {
                        candidates.push(entry.path());
                    }
                }
            }
        }
    }
    if candidates.len() != 1 {
        return Err(
            "NATIVE_ARTIFACT_UNAVAILABLE: exact cursor transcript missing or ambiguous".into(),
        );
    }
    let path = &candidates[0];
    let metadata = regular(path)?;
    let file = std::fs::File::open(path).map_err(|_| "native transcript unreadable")?;
    let mut line = String::new();
    BufReader::new(file.take(64 * 1024))
        .read_line(&mut line)
        .map_err(|_| "native provenance unreadable")?;
    let provenance: serde_json::Value =
        serde_json::from_str(&line).map_err(|_| "native provenance malformed")?;
    let payload = provenance
        .get("payload")
        .ok_or("native provenance payload missing")?;
    let selected_cwd = payload
        .get("cwd")
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from)
        .ok_or("native provenance workspace missing")?;
    if provenance.get("type").and_then(serde_json::Value::as_str) != Some("session_meta")
        || payload.get("id").and_then(serde_json::Value::as_str) != Some(cursor.as_str())
        || !selected_cwd.is_absolute()
        || cwd.is_some_and(|cwd| selected_cwd.canonicalize().ok() != cwd.canonicalize().ok())
    {
        return Err("native transcript cursor/workspace provenance mismatch".into());
    }
    let mut file = std::fs::File::open(path).map_err(|_| "native transcript unreadable")?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 8192];
    let mut bytes = 0u64;
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|_| "native transcript hash failed")?;
        if count == 0 {
            break;
        }
        bytes += count as u64;
        if bytes > 64 * 1024 * 1024 {
            return Err("native transcript exceeds import budget".into());
        }
        hash.update(&buffer[..count]);
    }
    if bytes != metadata.len() {
        return Err("native transcript changed after drain".into());
    }
    let selected_cwd = selected_cwd
        .canonicalize()
        .map_err(|_| "native provenance workspace unavailable")?;
    Ok((
        vec![NativeArtifact {
            source_path: path.clone(),
            relative_path: path
                .strip_prefix(&home)
                .map_err(|_| "native artifact escaped selected configuration home")?
                .to_path_buf(),
            bytes,
            sha256: hex::encode(hash.finalize()),
        }],
        selected_cwd,
    ))
}
fn directory(path: &Path) -> Result<(), String> {
    let metadata =
        std::fs::symlink_metadata(path).map_err(|_| "native source directory unavailable")?;
    if !metadata.file_type().is_dir() {
        return Err("native source directory must not be a symlink".into());
    }
    Ok(())
}
fn date_directories(
    path: &Path,
    width: usize,
    visited: &mut usize,
) -> Result<Vec<PathBuf>, String> {
    let mut result = Vec::new();
    for entry in std::fs::read_dir(path).map_err(|_| "native date directory unreadable")? {
        *visited += 1;
        if *visited > 100_000 {
            return Err("native directory budget exceeded".into());
        }
        let entry = entry.map_err(|_| "native date entry unavailable")?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if name.len() == width && name.bytes().all(|v| v.is_ascii_digit()) {
            directory(&entry.path())?;
            result.push(entry.path());
        }
    }
    Ok(result)
}
fn regular(path: &Path) -> Result<std::fs::Metadata, String> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| "native artifact unavailable")?;
    if !metadata.file_type().is_file() || metadata.len() > 64 * 1024 * 1024 {
        return Err("native artifact must be a bounded regular file".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err("native artifact custody permissions are too broad".into());
        }
    }
    Ok(metadata)
}
#[cfg(test)]
mod tests {
    use super::*;
    const CURSOR: &str = "11111111-1111-4111-8111-111111111111";
    fn fixture() -> (
        tempfile::TempDir,
        PathBuf,
        PathBuf,
        BTreeMap<String, String>,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().join("workspace");
        std::fs::create_dir(&cwd).unwrap();
        let home = dir.path().join("codex");
        let day = home.join("sessions/2026/10/11");
        std::fs::create_dir_all(&day).unwrap();
        let file = day.join(format!("rollout-2026-10-11T10-00-00-{CURSOR}.jsonl"));
        std::fs::write(
            &file,
            format!(
                "{}\n",
                serde_json::json!({"type":"session_meta","payload":{"id":CURSOR,"cwd":cwd}})
            ),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let env = BTreeMap::from([("CODEX_HOME".into(), home.to_string_lossy().into_owned())]);
        (dir, cwd, file, env)
    }
    #[test]
    fn selected_cursor_and_workspace_are_required() {
        let (_dir, cwd, file, env) = fixture();
        let selected = resolve("/runtime/codex-acp", &env, CURSOR, &cwd).unwrap();
        let (offline, workspace) = resolve_workspace("/runtime/codex-acp", &env, CURSOR).unwrap();
        assert_eq!(offline, selected);
        assert_eq!(workspace, cwd.canonicalize().unwrap());
        assert_eq!(selected[0].source_path, file.canonicalize().unwrap());
        assert_eq!(
            selected[0].sha256,
            hex::encode(Sha256::digest(std::fs::read(&file).unwrap()))
        );
        assert!(resolve("codex-acp", &BTreeMap::new(), CURSOR, &cwd).is_err());
        let other = cwd.parent().unwrap().join("other");
        std::fs::create_dir(&other).unwrap();
        assert!(resolve("codex-acp", &env, CURSOR, &other).is_err());
        assert!(resolve("unknown-agent-acp", &env, CURSOR, &cwd)
            .unwrap_err()
            .contains("UNSUPPORTED"));
    }
    #[test]
    fn duplicate_and_foreign_provenance_are_refused() {
        let (_dir, cwd, file, env) = fixture();
        let copy = file.with_file_name(format!("rollout-other-{CURSOR}.jsonl"));
        std::fs::copy(&file, &copy).unwrap();
        assert!(resolve("codex-acp", &env, CURSOR, &cwd).is_err());
        std::fs::remove_file(copy).unwrap();
        std::fs::write(
            file,
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"foreign\",\"cwd\":\"/\"}}\n",
        )
        .unwrap();
        assert!(resolve("codex-acp", &env, CURSOR, &cwd).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn selected_symlink_and_readable_custody_are_refused() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let (_dir, cwd, file, env) = fixture();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(resolve("codex-acp", &env, CURSOR, &cwd).is_err());
        let target = file.with_extension("private");
        std::fs::rename(&file, &target).unwrap();
        symlink(&target, &file).unwrap();
        assert!(resolve("codex-acp", &env, CURSOR, &cwd).is_err());
    }
}
