//! Exact shipped bootstrap bytes, independent of the editable team draft.

use super::{invalid, tree, SetupError};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub(super) const ROLE: &str = "project-setup";

pub(super) fn read(path: &Path, limit: u64) -> Result<Vec<u8>, SetupError> {
    tree::check_regular_file(path, limit)?;
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(path)?;
    if !file.metadata()?.is_file() {
        return Err(invalid("Setup actor storage must contain regular files."));
    }
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(invalid("Setup actor file exceeds its size limit."));
    }
    Ok(bytes)
}

pub(super) fn write_new(path: &Path, bytes: &[u8]) -> Result<(), SetupError> {
    let mut options = std::fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

pub(super) fn sync(path: &Path) -> Result<(), SetupError> {
    #[cfg(unix)]
    std::fs::File::open(path)?.sync_all()?;
    Ok(())
}

fn collect(
    root: &Path,
    path: &Path,
    entries: &mut usize,
    depth: usize,
    files: &mut Vec<PathBuf>,
) -> Result<(), SetupError> {
    *entries += 1;
    if *entries > 2048 || depth > 24 {
        return Err(invalid(
            "The setup bootstrap exceeds the entry or depth limit.",
        ));
    }
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(invalid("Setup bootstrap symlinks are forbidden."));
    }
    if metadata.is_dir() {
        for entry in std::fs::read_dir(path)? {
            collect(root, &entry?.path(), entries, depth + 1, files)?;
        }
    } else {
        tree::check_regular_file(path, 256 * 1024)?;
        files.push(
            path.strip_prefix(root)
                .map_err(|_| invalid("Bootstrap path escaped storage."))?
                .to_path_buf(),
        );
    }
    Ok(())
}

fn capture(path: &Path) -> Result<Vec<(PathBuf, Vec<u8>)>, SetupError> {
    tree::ensure_contained_directory(path, path)?;
    let mut files = Vec::new();
    collect(path, path, &mut 0, 0, &mut files)?;
    files.sort();
    let mut total = 0;
    let mut contents = Vec::new();
    for relative in files {
        let full = path.join(&relative);
        tree::ensure_contained_directory(
            path,
            full.parent()
                .ok_or_else(|| invalid("Missing bootstrap parent."))?,
        )?;
        let bytes = read(&full, 256 * 1024)?;
        total += bytes.len();
        if total > 8 * 1024 * 1024 {
            return Err(invalid("The setup bootstrap exceeds 8 MiB."));
        }
        contents.push((relative, bytes));
    }
    Ok(contents)
}

pub(super) fn digest(path: &Path) -> Result<String, SetupError> {
    let mut digest = Sha256::new();
    digest.update(b"beekeeper:project-team-setup:bootstrap:v1\0");
    for (relative, bytes) in capture(path)? {
        let name = relative
            .to_str()
            .ok_or_else(|| invalid("Bootstrap paths must be UTF-8."))?
            .replace('\\', "/");
        digest.update((name.len() as u64).to_be_bytes());
        digest.update(name.as_bytes());
        digest.update((bytes.len() as u64).to_be_bytes());
        digest.update(bytes);
    }
    Ok(hex::encode(digest.finalize()))
}

pub(super) fn copy(source: &Path, destination: &Path) -> Result<(), SetupError> {
    let contents = capture(source)?;
    tree::create_private_directory(destination)?;
    for (relative, bytes) in contents {
        let path = destination.join(relative);
        let parent = path
            .parent()
            .ok_or_else(|| invalid("Missing bootstrap parent."))?;
        tree::create_private_directory(parent)?;
        write_new(&path, &bytes)?;
        sync(parent)?;
    }
    sync(destination)
}

pub(super) fn validate(
    path: &Path,
) -> Result<beekeeper_persona_pkg::resolve::ResolvedPersona, SetupError> {
    digest(path)?;
    tree::check_manifest_references(path)?;
    let loaded =
        beekeeper_persona_pkg::pack::load_pack(path).map_err(|e| invalid(e.to_string()))?;
    if loaded.personas.len() != 1 {
        return Err(invalid(
            "The setup bootstrap must contain exactly one persona.",
        ));
    }
    for reference in &loaded.personas[0].skills {
        tree::check_relative_reference(reference)?;
    }
    let report = beekeeper_persona_pkg::validate::validate_pack(path);
    if report.has_errors() {
        return Err(invalid(format!(
            "The shipped setup pack is invalid: {report}"
        )));
    }
    let persona = beekeeper_persona_pkg::resolve::resolve_persona_by_name(path, ROLE)
        .map_err(|e| invalid(e.to_string()))?;
    if persona.name != ROLE
        || persona.role.as_deref() != Some(ROLE)
        || persona.hooks.is_some()
        || !persona.mcp_servers.is_empty()
    {
        return Err(invalid("The setup bootstrap must declare project-setup without executable hook or MCP configuration."));
    }
    for skill in &persona.skills {
        if skill.is_empty() || skill.contains(['/', '\\', ':']) || skill == "." || skill == ".." {
            return Err(invalid("Bootstrap skill references must be local names."));
        }
        let directory = path.join("skills").join(skill);
        tree::ensure_contained_directory(path, &directory)?;
        beekeeper_persona_pkg::skill_meta::read_skill_meta(&directory)
            .map_err(|e| invalid(e.to_string()))?;
    }
    Ok(persona)
}
