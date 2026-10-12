//! Claude's SDK-documented JSONL layout, scoped to one selected native cursor.
//! Source: anthropics/claude-agent-sdk-python/_internal/sessions.py (path
//! sanitization, explicit config directory and sessionId transcript entries).
use super::*;

pub(super) fn resolve(
    environment: &BTreeMap<String, String>,
    cursor: &str,
    cwd: Option<&Path>,
) -> Result<(Vec<NativeArtifact>, PathBuf), String> {
    let home = environment
        .get("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .ok_or("NATIVE_ARTIFACT_UNAVAILABLE: exact CLAUDE_CONFIG_DIR was not exported")?;
    if !home.is_absolute() {
        return Err("native configuration home must be absolute".into());
    }
    directory(&home)?;
    let home = home
        .canonicalize()
        .map_err(|_| "Claude native configuration home unavailable")?;
    let projects = home.join("projects");
    directory(&projects)?;
    let mut candidates = Vec::new();
    if let Some(cwd) = cwd {
        let canonical = cwd
            .canonicalize()
            .map_err(|_| "selected workspace unavailable")?;
        for path in [cwd.to_path_buf(), canonical] {
            let directory_name = project_key(&path)?;
            let project = projects.join(directory_name);
            if !project.exists() {
                continue;
            }
            directory(&project)?;
            let candidate = project.join(format!("{cursor}.jsonl"));
            if candidate.exists() && !candidates.contains(&candidate) {
                candidates.push(candidate);
            }
        }
    } else {
        // Metadata-only lookup of this exact UUID; no unrelated transcript reads.
        for (index, entry) in std::fs::read_dir(&projects)
            .map_err(|_| "Claude project metadata unavailable")?
            .enumerate()
        {
            if index >= 100_000 {
                return Err("Claude artifact selection exceeds bounded directory budget".into());
            }
            let entry = entry.map_err(|_| "Claude project entry unavailable")?;
            let candidate = entry.path().join(format!("{cursor}.jsonl"));
            if candidate.exists() {
                directory(&entry.path())?;
                candidates.push(candidate);
            }
        }
    }
    if candidates.len() != 1 {
        return Err("NATIVE_ARTIFACT_UNAVAILABLE: Claude exact cursor missing or ambiguous".into());
    }
    let path = &candidates[0];
    let metadata = regular(path)?;
    let related = path.with_extension("");
    if related.exists() {
        return Err("NATIVE_ARTIFACT_UNSUPPORTED: selected Claude session has auxiliary state requiring explicit preservation".into());
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|_| "Claude native artifact unreadable")?
        .take(metadata.len().saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| "Claude native artifact unreadable")?;
    if bytes.len() as u64 != metadata.len() {
        return Err("Claude native transcript changed during selection".into());
    }
    let (selected, _, recorded_key) = provenance(&bytes, cursor, cwd)?;
    let encoded = project_key(&selected)?;
    let lexical_key = cwd.map(project_key).transpose()?;
    if path.parent().and_then(Path::file_name).is_none_or(|name| {
        name != std::ffi::OsStr::new(&encoded)
            && name != std::ffi::OsStr::new(&recorded_key)
            && lexical_key
                .as_ref()
                .is_none_or(|key| name != std::ffi::OsStr::new(key))
    }) {
        return Err("Claude native project path provenance mismatch".into());
    }
    let artifact = NativeArtifact {
        source_path: path.clone(),
        relative_path: path
            .strip_prefix(&home)
            .map_err(|_| "Claude artifact escaped selected home")?
            .to_path_buf(),
        bytes: metadata.len(),
        sha256: hex::encode(Sha256::digest(&bytes)),
    };
    Ok((vec![artifact], selected))
}
pub(super) fn provenance(
    bytes: &[u8],
    cursor: &str,
    cwd: Option<&Path>,
) -> Result<(PathBuf, String, String), String> {
    if bytes.len() > 64 * 1024 * 1024 {
        return Err("Claude transcript exceeds import budget".into());
    }
    let mut selected = None;
    let mut first_timestamp = None;
    let mut recorded_key = None;
    for line in BufReader::new(bytes).lines() {
        let line = line.map_err(|_| "Claude native transcript unreadable")?;
        let row: serde_json::Value =
            serde_json::from_str(&line).map_err(|_| "Claude native transcript malformed")?;
        if row
            .get("sessionId")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|id| id != cursor)
        {
            return Err("Claude native transcript contains foreign session provenance".into());
        }
        let Some(workspace) = row.get("cwd").and_then(serde_json::Value::as_str) else {
            continue;
        };
        if row.get("sessionId").and_then(serde_json::Value::as_str) != Some(cursor) {
            return Err("Claude workspace lacks exact session provenance".into());
        }
        if !matches!(
            row.get("type").and_then(serde_json::Value::as_str),
            Some("user" | "assistant" | "system" | "progress" | "attachment")
        ) {
            return Err("Claude native metadata type unsupported".into());
        }
        let timestamp = row
            .get("timestamp")
            .and_then(serde_json::Value::as_str)
            .ok_or("Claude native source timestamp missing")?;
        let instant = chrono::DateTime::parse_from_rfc3339(timestamp)
            .map_err(|_| "Claude native source timestamp malformed")?;
        if instant.timestamp() < 0 {
            return Err("Claude native source timestamp precedes epoch".into());
        }
        let workspace = PathBuf::from(workspace);
        if !workspace.is_absolute() {
            return Err("Claude native workspace must be absolute".into());
        }
        let canonical = workspace
            .canonicalize()
            .map_err(|_| "Claude native workspace unavailable")?;
        if cwd.is_some_and(|cwd| cwd.canonicalize().ok() != Some(canonical.clone()))
            || selected.as_ref().is_some_and(|held| held != &canonical)
        {
            return Err("Claude native workspace provenance mismatch".into());
        }
        if recorded_key.is_none() {
            recorded_key = Some(project_key(&workspace)?);
        }
        selected = Some(canonical);
        if first_timestamp.is_none() {
            first_timestamp = Some(timestamp.to_owned());
        }
    }
    Ok((
        selected.ok_or("Claude native session/workspace provenance unavailable")?,
        first_timestamp.ok_or("Claude native timestamp unavailable")?,
        recorded_key.ok_or("Claude native project path unavailable")?,
    ))
}
fn project_key(cwd: &Path) -> Result<String, String> {
    let path = cwd
        .to_str()
        .ok_or("Claude workspace path encoding unsupported")?;
    let key: String = path
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect();
    if key.len() > 200 {
        return Err("NATIVE_ARTIFACT_UNSUPPORTED: Claude long hashed workspace paths require explicit source selection".into());
    }
    Ok(key)
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
        let root = tempfile::tempdir().unwrap();
        let cwd = root.path().join("workspace");
        std::fs::create_dir(&cwd).unwrap();
        let cwd = cwd.canonicalize().unwrap();
        let home = root.path().join("claude");
        let project = home.join("projects").join(project_key(&cwd).unwrap());
        std::fs::create_dir_all(&project).unwrap();
        let file = project.join(format!("{CURSOR}.jsonl"));
        let row = serde_json::json!({"type":"user","uuid":"message","parentUuid":null,"sessionId":CURSOR,"cwd":cwd,"timestamp":"2026-10-11T01:02:03Z","message":{"role":"user","content":"fixture"}});
        let bytes = format!("{{\"type\":\"file-history-snapshot\"}}\n{row}\n");
        crate::atomic_write::atomic_write_json_restricted(&file, bytes.as_bytes()).unwrap();
        let env = BTreeMap::from([(
            "CLAUDE_CONFIG_DIR".into(),
            home.to_string_lossy().into_owned(),
        )]);
        (root, cwd, file, env)
    }
    #[test]
    fn claude_exact_cursor_and_config_preserve_full_jsonl_without_codex_home() {
        let (_root, cwd, file, env) = fixture();
        let (artifacts, workspace) = resolve(&env, CURSOR, Some(&cwd)).unwrap();
        assert_eq!(workspace, cwd);
        assert_eq!(
            artifacts[0].sha256,
            hex::encode(Sha256::digest(std::fs::read(&file).unwrap()))
        );
        assert_eq!(
            crate::native_artifacts::source_timestamp(
                "claude",
                &std::fs::read(&file).unwrap(),
                CURSOR,
                &cwd
            )
            .unwrap(),
            "2026-10-11T01:02:03Z"
        );
        assert_eq!(resolve(&env, CURSOR, None).unwrap().0, artifacts);
        assert!(resolve(&BTreeMap::new(), CURSOR, Some(&cwd)).is_err());
        let foreign = cwd.parent().unwrap().join("foreign");
        std::fs::create_dir(&foreign).unwrap();
        assert!(resolve(&env, CURSOR, Some(&foreign)).is_err());
    }
    #[test]
    fn claude_foreign_identity_duplicate_and_auxiliary_state_refuse() {
        let (_root, cwd, file, env) = fixture();
        let bytes = std::fs::read_to_string(&file).unwrap();
        std::fs::write(
            &file,
            bytes.replace(CURSOR, "22222222-2222-4222-8222-222222222222"),
        )
        .unwrap();
        assert!(resolve(&env, CURSOR, Some(&cwd)).is_err());
        std::fs::write(&file, bytes).unwrap();
        std::fs::create_dir(file.with_extension("")).unwrap();
        assert!(resolve(&env, CURSOR, Some(&cwd))
            .unwrap_err()
            .contains("auxiliary"));
        std::fs::remove_dir(file.with_extension("")).unwrap();
        let duplicate = file.parent().unwrap().parent().unwrap().join("foreign");
        std::fs::create_dir(&duplicate).unwrap();
        std::fs::copy(&file, duplicate.join(file.file_name().unwrap())).unwrap();
        assert!(resolve(&env, CURSOR, None).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn claude_symlink_and_broad_read_permissions_refuse() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let (_root, cwd, file, env) = fixture();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(resolve(&env, CURSOR, Some(&cwd)).is_err());
        let target = file.with_extension("private");
        std::fs::rename(&file, &target).unwrap();
        symlink(target, &file).unwrap();
        assert!(resolve(&env, CURSOR, Some(&cwd)).is_err());
    }
}
