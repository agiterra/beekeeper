//! Bounded, symlink-free draft IO and real persona-pack validation.

use super::{
    ProjectTeamSetupDraft, ProjectTeamSetupValidation, SetupDiagnostic, SetupError,
    SetupRoleIdentity, SetupStatus,
};
use std::path::{Path, PathBuf};

const MAX_FILE_BYTES: u64 = 256 * 1024;
const MAX_TREE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_ENTRIES: usize = 2048;
const MAX_DEPTH: usize = 24;

pub(super) fn create_private_directory(path: &Path) -> Result<(), SetupError> {
    if path.try_exists()? {
        let metadata = std::fs::symlink_metadata(path)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(SetupError::new(
                "filesystem",
                "The setup storage path must be a real directory.",
            ));
        }
        return Ok(());
    }
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

pub(super) fn ensure_contained_directory(root: &Path, path: &Path) -> Result<(), SetupError> {
    let relative = path.strip_prefix(root).map_err(|_| {
        SetupError::new("invalid_draft", "The draft path is outside setup storage.")
    })?;
    let mut cursor = root.to_path_buf();
    for component in std::iter::once(None).chain(relative.components().map(Some)) {
        if let Some(component) = component {
            if !matches!(component, std::path::Component::Normal(_)) {
                return Err(SetupError::new(
                    "invalid_draft",
                    "The draft path contains traversal.",
                ));
            }
            cursor.push(component.as_os_str());
        }
        let metadata = std::fs::symlink_metadata(&cursor)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(SetupError::new(
                "invalid_draft",
                format!(
                    "{} must be a real directory, without symlinks.",
                    cursor.display()
                ),
            ));
        }
    }
    Ok(())
}

pub(super) fn check_regular_file(path: &Path, limit: u64) -> Result<(), SetupError> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.file_type().is_file() || metadata.len() > limit {
        return Err(SetupError::new(
            "invalid_draft",
            format!(
                "{} must be a regular file no larger than {limit} bytes.",
                path.display()
            ),
        ));
    }
    Ok(())
}

fn walk(
    root: &Path,
    path: &Path,
    files: &mut Vec<PathBuf>,
    count: &mut usize,
    bytes: &mut u64,
    depth: usize,
) -> Result<(), SetupError> {
    *count += 1;
    if *count > MAX_ENTRIES || depth > MAX_DEPTH {
        return Err(SetupError::new(
            "invalid_draft",
            "The draft exceeds the file-count or directory-depth limit.",
        ));
    }
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(SetupError::new(
            "invalid_draft",
            format!(
                "Symlinks are not allowed in setup drafts: {}",
                path.display()
            ),
        ));
    }
    if metadata.is_dir() {
        for entry in std::fs::read_dir(path)? {
            walk(root, &entry?.path(), files, count, bytes, depth + 1)?;
        }
    } else {
        check_regular_file(path, MAX_FILE_BYTES)?;
        *bytes += metadata.len();
        if *bytes > MAX_TREE_BYTES {
            return Err(SetupError::new(
                "invalid_draft",
                "The draft exceeds the 8 MiB size limit.",
            ));
        }
        files.push(
            path.strip_prefix(root)
                .map_err(|_| SetupError::new("invalid_draft", "A file escaped the draft."))?
                .to_path_buf(),
        );
    }
    Ok(())
}

fn tree_files(path: &Path) -> Result<Vec<PathBuf>, SetupError> {
    let mut files = Vec::new();
    walk(path, path, &mut files, &mut 0, &mut 0, 0)?;
    files.sort();
    Ok(files)
}

pub(super) fn copy_tree(source: &Path, destination: &Path) -> Result<(), SetupError> {
    let files = tree_files(source)?;
    std::fs::create_dir_all(destination)?;
    for relative in files {
        let output = destination.join(&relative);
        if let Some(parent) = output.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // Bounded read and create_new never follows a pre-existing output link.
        check_regular_file(&source.join(&relative), MAX_FILE_BYTES)?;
        let bytes = std::fs::read(source.join(relative))?;
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(output)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
    }
    Ok(())
}

pub(super) fn git_directory(path: &Path) -> Result<PathBuf, SetupError> {
    if !path.is_absolute() || !path.is_dir() {
        return Err(SetupError::new(
            "invalid_input",
            "Choose an existing absolute Git project folder.",
        ));
    }
    let canonical = std::fs::canonicalize(path)?;
    let auth = crate::commands::project_git_exec::build_local_git_auth_config()
        .map_err(|message| SetupError::new("invalid_input", message))?;
    let top = crate::commands::project_git_exec::run_git(
        &["rev-parse", "--show-toplevel"],
        Some(&canonical),
        &auth,
    )
    .map_err(|message| {
        SetupError::new(
            "invalid_input",
            format!("Choose a Git working-tree root: {message}"),
        )
    })?;
    if std::fs::canonicalize(top.trim())? != canonical {
        return Err(SetupError::new(
            "invalid_input",
            "Choose the Git working-tree root, not a subfolder.",
        ));
    }
    Ok(canonical)
}

fn identities(roles: &Path) -> Result<Vec<SetupRoleIdentity>, SetupError> {
    let mut result = Vec::new();
    for entry in std::fs::read_dir(roles)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let role = entry
            .file_name()
            .into_string()
            .map_err(|_| SetupError::new("invalid_draft", "Role folder names must be UTF-8."))?;
        check_manifest_references(&entry.path())?;
        let pack = buzz_persona_pkg::pack::load_pack(&entry.path())
            .map_err(|error| SetupError::new("invalid_draft", format!("{role}: {error}")))?;
        if pack.personas.len() != 1
            || pack.personas[0].role.as_deref() != Some(&role)
            || pack.personas[0].name != role
        {
            return Err(SetupError::new(
                "invalid_draft",
                format!("{role} must contain exactly one persona declaring role {role}."),
            ));
        }
        for skill in &pack.personas[0].skills {
            check_relative_reference(skill)?;
        }
        for name in buzz_persona_pkg::pack::resolve_skills(&entry.path(), &pack.personas)
            .values()
            .flatten()
        {
            let skill_dir = entry.path().join("skills").join(name);
            ensure_contained_directory(&entry.path(), &skill_dir)?;
            buzz_persona_pkg::skill_meta::read_skill_meta(&skill_dir)
                .map_err(|error| SetupError::new("invalid_draft", format!("{role}: {error}")))?;
        }
        if pack.personas[0].hooks.is_some()
            || !pack.personas[0].mcp_servers.is_empty()
            || pack.shared_mcp_config.is_some()
        {
            return Err(SetupError::new("invalid_draft", "Setup drafts currently support role procedures and skills; executable hooks and MCP configuration need a separate tool-configuration workflow."));
        }
        result.push(SetupRoleIdentity {
            role,
            persona_name: pack.personas[0].name.clone(),
            pack_id: pack.manifest.id,
        });
    }
    result.sort_by(|a, b| a.role.cmp(&b.role));
    if result.len() > 32 || !result.iter().any(|identity| identity.role == "lead") {
        return Err(SetupError::new(
            "invalid_draft",
            "A setup draft must contain lead and at most 32 role packs.",
        ));
    }
    let ids: std::collections::HashSet<_> =
        result.iter().map(|identity| &identity.pack_id).collect();
    if ids.len() != result.len() {
        return Err(SetupError::new(
            "invalid_draft",
            "Every role pack must have a unique pack ID.",
        ));
    }
    Ok(result)
}

pub(super) fn check_relative_reference(reference: &str) -> Result<(), SetupError> {
    // Reject Windows paths on Unix too: a draft can later move to Windows.
    let portable = reference.replace('\\', "/");
    if portable.is_empty()
        || portable.starts_with('/')
        || portable.contains(':')
        || portable.split('/').any(|part| part == "..")
    {
        return Err(SetupError::new(
            "invalid_draft",
            format!("Pack references must stay inside the role pack: {reference}"),
        ));
    }
    Ok(())
}

pub(super) fn check_manifest_references(pack: &Path) -> Result<(), SetupError> {
    let path = pack.join(".plugin/plugin.json");
    check_regular_file(&path, MAX_FILE_BYTES)?;
    let manifest: serde_json::Value = serde_json::from_slice(&std::fs::read(path)?)
        .map_err(|error| SetupError::new("invalid_draft", error.to_string()))?;
    if ["mcp_config", "hooks_config"]
        .iter()
        .any(|key| manifest.get(key).is_some_and(|value| !value.is_null()))
    {
        return Err(SetupError::new(
            "invalid_draft",
            "Executable hooks and MCP configuration are not supported in project setup drafts yet.",
        ));
    }
    for key in ["pack_instructions", "mcp_config", "hooks_config"] {
        if let Some(reference) = manifest.get(key).and_then(serde_json::Value::as_str) {
            check_relative_reference(reference)?;
            check_regular_file(&pack.join(reference), MAX_FILE_BYTES)?;
        }
    }
    if let Some(personas) = manifest
        .get("personas")
        .and_then(serde_json::Value::as_array)
    {
        for reference in personas.iter().filter_map(serde_json::Value::as_str) {
            check_relative_reference(reference)?;
        }
    }
    Ok(())
}

pub(super) fn seed_identities(seed: &Path) -> Result<Vec<SetupRoleIdentity>, SetupError> {
    tree_files(seed)?;
    let expected = identities(seed)?;
    for role in &expected {
        let report = buzz_persona_pkg::validate::validate_pack(&seed.join(&role.role));
        if report.has_errors() {
            return Err(SetupError::new(
                "invalid_draft",
                format!("The bundled {} baseline is invalid: {report}", role.role),
            ));
        }
    }
    Ok(expected)
}

pub(super) fn validate(record: &ProjectTeamSetupDraft) -> ProjectTeamSetupValidation {
    let mut diagnostics = Vec::new();
    let mut actual_roles = Vec::new();
    let roles = Path::new(&record.roles_directory);
    let structural = ensure_contained_directory(Path::new(&record.draft_directory), roles)
        .and_then(|()| tree_files(Path::new(&record.draft_directory)).map(|_| ()))
        .and_then(|()| identities(roles))
        .and_then(|found| {
            if found.iter().all(|identity| {
                record
                    .expected_roles
                    .iter()
                    .find(|seed| seed.role == identity.role)
                    .is_none_or(|seed| seed == identity)
            }) {
                Ok(found)
            } else {
                Err(SetupError::new(
                    "invalid_draft",
                    "A retained baseline role's persona or pack identity changed.",
                ))
            }
        });
    match structural {
        Err(error) => diagnostics.push(SetupDiagnostic {
            level: "error",
            message: error.message,
        }),
        Ok(found) => {
            actual_roles = found.into_iter().map(|identity| identity.role).collect();
            for role in &actual_roles {
                for diagnostic in
                    buzz_persona_pkg::validate::validate_pack(&roles.join(role)).diagnostics
                {
                    let (level, message) = match diagnostic {
                        buzz_persona_pkg::validate::ValidationDiagnostic::Error(message) => {
                            ("error", message)
                        }
                        buzz_persona_pkg::validate::ValidationDiagnostic::Warning(message) => {
                            ("warning", message)
                        }
                    };
                    diagnostics.push(SetupDiagnostic {
                        level,
                        message: format!("{role}: {message}"),
                    });
                }
            }
        }
    }
    ProjectTeamSetupValidation {
        setup_id: record.setup_id.clone(),
        status: SetupStatus::Draft,
        valid: !diagnostics.iter().any(|d| d.level == "error"),
        roles: actual_roles,
        diagnostics,
    }
}
