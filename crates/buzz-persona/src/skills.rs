//! Materialize a resolved persona's skills where the agent can read them.
//!
//! A pack's `skills/<name>/SKILL.md` is craft the persona's prompt only refers
//! to; an agent can read it only if it exists somewhere it can open. This
//! module is the step that puts it there, and there are two of those:
//!
//! - [`materialize_skill_bundle`] writes into an **explicit root** the caller
//!   owns, copies each skill directory whole, and prunes what the persona
//!   dropped. The session provider gives each seat a bundle of its own, under
//!   the app's data directory, and names the absolute `SKILL.md` paths in the
//!   briefing — so a seat's craft never lands in the checkout it commits from.
//! - [`materialize_skills`] writes `SKILL.md` alone into
//!   `<workdir>/.agents/skills/<name>/`, the harness-agnostic path the desktop
//!   nest uses for a managed agent that has no bundle. Unchanged, and the
//!   notes below describe it.
//!
//! - It writes into `<workdir>/.agents/skills/<name>/SKILL.md` — the same
//!   harness-agnostic path the nest uses for the `buzz-cli` skill, so every
//!   runtime that reads `.agents/skills` finds a crew seat's skills the same
//!   way it finds that one.
//! - It writes **per workdir**, never into a shared directory: two seats of the
//!   same crew running in two checkouts get two copies, and neither can edit
//!   the other's.
//! - It is idempotent: a second call with unchanged pack content writes
//!   nothing, which is what makes it safe on the spawn path of a session that
//!   restarts.
//! - It refuses a skill name that is not a single path component, and a source
//!   directory *or `SKILL.md`* that resolves outside the pack's `skills/`
//!   directory. A pack is ordinary data — treating one of its strings as a
//!   path without checking is how a pack becomes a write primitive, and
//!   following a symlink out of the pack without checking is how it becomes a
//!   read one: the bytes of whatever it points at would land in a seat's
//!   working directory as instructions.
//! - It refuses the same escape on the *destination*. "Per workdir" is a claim
//!   about where the bytes land, so the target is canonicalized the way the
//!   source is: a `.agents` symlink in the checkout, a symlinked skill
//!   directory, or a symlinked destination `SKILL.md` that resolves outside
//!   `<workdir>/.agents/skills` is refused rather than written through.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

use crate::resolve::ResolvedPersona;

/// The workdir-relative directory skills are written under.
pub const SEAT_SKILLS_DIR: &str = ".agents/skills";

/// Why skill materialization refused or failed.
#[derive(Debug, thiserror::Error)]
pub enum SkillError {
    /// The persona names skills but the pack has no `skills/` directory.
    #[error("persona \"{persona}\" claims skill \"{skill}\" but the pack has no skills directory")]
    NoSkillsDir { persona: String, skill: String },

    /// The skill name is not a single, ordinary path component.
    #[error("skill name is not a single path component: {0:?}")]
    UnsafeName(String),

    /// The skill directory, or its `SKILL.md`, resolves outside the pack's
    /// `skills/` directory.
    #[error("skill \"{name}\" resolves outside the pack skills directory: {path}")]
    SourceEscape { name: String, path: PathBuf },

    /// The destination skill directory, or a file in it, resolves outside the
    /// skills root being written into.
    #[error("skill \"{name}\" would be written outside the skills directory: {path}")]
    TargetEscape { name: String, path: PathBuf },

    /// The skill directory has no `SKILL.md`.
    #[error("skill \"{name}\" has no SKILL.md at {path}")]
    MissingSkillMd { name: String, path: PathBuf },

    /// A filesystem operation failed.
    #[error("{operation} {path}: {source}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// One skill as it exists on disk after a call to [`materialize_skills`] or
/// [`materialize_skill_bundle`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterializedSkill {
    /// Bare skill name (the directory name in the pack and in the workdir).
    pub name: String,
    /// Absolute, symlink-resolved path to the written `SKILL.md` — where the
    /// bytes actually landed, which is what the destination guard checked.
    pub path: PathBuf,
    /// Whether this call changed anything for this skill: the `SKILL.md` for
    /// [`materialize_skills`], any file under the skill's directory for
    /// [`materialize_skill_bundle`]. `false` means the destination already
    /// held the pack's current bytes — the idempotent case.
    pub written: bool,
}

/// Write each of `persona`'s resolved skills into `workdir`.
///
/// Returns one [`MaterializedSkill`] per skill, in `persona.skills` order.
/// A persona with no skills is a no-op that creates no directories.
///
/// # Errors
///
/// Refuses (writing nothing further) on an unsafe skill name, a source that
/// escapes the pack, a destination that escapes `<workdir>/.agents/skills`, a
/// skill directory with no `SKILL.md`, or any filesystem failure. Callers on a spawn path should treat a refusal as a failed spawn:
/// a seat whose prompt names craft it does not hold is a seat that lies.
pub fn materialize_skills(
    persona: &ResolvedPersona,
    workdir: &Path,
) -> Result<Vec<MaterializedSkill>, SkillError> {
    if persona.skills.is_empty() {
        return Ok(Vec::new());
    }

    let mut out = Vec::with_capacity(persona.skills.len());
    for name in &persona.skills {
        let Some(skills_dir) = persona.skills_dir.as_deref() else {
            return Err(SkillError::NoSkillsDir {
                persona: persona.name.clone(),
                skill: name.clone(),
            });
        };
        out.push(materialize_one(name, skills_dir, workdir)?);
    }
    Ok(out)
}

fn materialize_one(
    name: &str,
    skills_dir: &Path,
    workdir: &Path,
) -> Result<MaterializedSkill, SkillError> {
    let component = safe_component(name)?;

    // `canonicalize` resolves `..` and symlinks; a skill directory that is a
    // symlink pointing out of the pack is the same escape as a `..` name, and
    // is refused for the same reason.
    let (resolved, root) = resolved_source_dir(name, skills_dir, component)?;

    // The directory check above says nothing about the file inside it: a real
    // directory in the pack may hold a `SKILL.md` that is a symlink to
    // anything on this computer. Resolve the *file* too, check the resolved
    // path, and read that resolved path — so the bytes that are read are the
    // bytes that were checked, not whatever the name points at a moment later.
    let source_md = resolved.join("SKILL.md");
    let resolved_md = match source_md.canonicalize() {
        Ok(path) => path,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            return Err(SkillError::MissingSkillMd {
                name: name.to_owned(),
                path: source_md,
            })
        }
        Err(source) => {
            return Err(SkillError::Io {
                operation: "read",
                path: source_md,
                source,
            })
        }
    };
    if !resolved_md.starts_with(&root) {
        return Err(SkillError::SourceEscape {
            name: name.to_owned(),
            path: resolved_md,
        });
    }
    let body = std::fs::read(&resolved_md).map_err(|source| SkillError::Io {
        operation: "read",
        path: resolved_md.clone(),
        source,
    })?;

    // The pack is not the only untrusted half of this write. The *workdir* is
    // a checkout a person controls, and a `.agents` symlink in it — the
    // ordinary way people share one skills directory between checkouts — makes
    // `create_dir_all` + `write` land the pack's bytes outside the workdir,
    // over the human's own `~/.agents/skills/<name>/SKILL.md`. So the
    // destination is resolved exactly like the source: component by component,
    // following a symlink only while it stays under the workdir.
    let skills_root = resolved_skills_root(name, workdir)?;
    let target_dir = resolve_child(
        name,
        &skills_root,
        &skills_root,
        std::ffi::OsStr::new(component),
    )?;
    let target_md = target_dir.join("SKILL.md");
    // A symlinked destination file is the same escape one level down: writing
    // through it replaces whatever it points at, dangling links included.
    if let Ok(meta) = target_md.symlink_metadata() {
        if meta.file_type().is_symlink() {
            let resolved = target_md
                .canonicalize()
                .map_err(|_| SkillError::TargetEscape {
                    name: name.to_owned(),
                    path: target_md.clone(),
                })?;
            if !resolved.starts_with(&skills_root) {
                return Err(SkillError::TargetEscape {
                    name: name.to_owned(),
                    path: resolved,
                });
            }
        }
    }

    // Idempotence is a read, not a flag: if the bytes already match, the file
    // is left completely alone, mtime included, so a restarting session does
    // not look like a content change to anything watching the workdir.
    if std::fs::read(&target_md).is_ok_and(|existing| existing == body) {
        return Ok(MaterializedSkill {
            name: name.to_owned(),
            path: target_md,
            written: false,
        });
    }

    std::fs::write(&target_md, &body).map_err(|source| SkillError::Io {
        operation: "write",
        path: target_md.clone(),
        source,
    })?;
    Ok(MaterializedSkill {
        name: name.to_owned(),
        path: target_md,
        written: true,
    })
}

// ---------------------------------------------------------------------------
// The execution-owned bundle.
// ---------------------------------------------------------------------------

/// The file every skill directory must contain.
const SKILL_MD: &str = "SKILL.md";

/// The manifest written beside a bundle's `skills/` directory.
pub const SKILL_BUNDLE_MANIFEST_FILE: &str = "manifest.json";

/// The manifest's `schema` string.
///
/// A version in the file itself, so a later reader can tell a bundle written
/// by this code from one written by something that only looks like it.
pub const SKILL_BUNDLE_SCHEMA: &str = "beekeeper.seat-skill-bundle/v1";

/// What a bundle was written from, as [`write_bundle_manifest`] records it.
///
/// `pack_ref` is opaque here on purpose: it is the wire's account of the pack
/// (`buzz_core::coding_session_payload::PackRef`), and this crate has no
/// business knowing its shape — the caller hands over the JSON it already
/// publishes, so the manifest and the 44223 cannot drift apart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillBundleManifest {
    /// The persona within the pack this bundle belongs to.
    pub persona_id: String,
    /// The host-local pack directory the skills were copied from.
    pub pack_dir: PathBuf,
    /// The pack's wire coordinate, when the launcher staged one.
    pub pack_ref: Option<serde_json::Value>,
    /// The skills in the bundle. Sorted and deduplicated before it is written.
    pub skills: Vec<String>,
}

/// Write each of `persona`'s resolved skills into `skills_root`, whole.
///
/// The bundle form of [`materialize_skills`], and the difference is the whole
/// point of it:
///
/// - The root is **explicit**. Nothing here derives a path from a working
///   directory, so a seat's skills can live somewhere the seat does not
///   commit, clean, or `git status` — see the session provider's seat bundle.
/// - A skill directory is copied **whole**, recursively: the supporting files
///   a `SKILL.md` refers to (templates, scripts, examples) reach the seat too.
///   Copying only `SKILL.md`, as [`materialize_skills`] does, hands the seat
///   instructions that name files it does not have.
/// - The bundle is **pruned**: a skill directory under `skills_root` that the
///   persona no longer claims is removed, as is a file inside a skill the pack
///   no longer contains. Nothing outside `skills_root` is ever removed.
///
/// Idempotence is unchanged: a file whose bytes already match is left
/// completely alone, mtime included. [`MaterializedSkill::written`] is `true`
/// when this call changed anything under that skill's directory.
///
/// # Errors
///
/// The same refusals [`materialize_skills`] makes, applied to every file in
/// the tree rather than to `SKILL.md` alone: an unsafe skill name
/// ([`SkillError::UnsafeName`]), a source that escapes the pack's `skills/`
/// root or a symlink anywhere inside the source tree
/// ([`SkillError::SourceEscape`]), a destination that escapes `skills_root` or
/// a symlink at the destination ([`SkillError::TargetEscape`]), a skill
/// directory with no `SKILL.md` ([`SkillError::MissingSkillMd`]), or any
/// filesystem failure. A symlink in the source is **refused, not followed**:
/// a pack is ordinary data, and following one is how a pack becomes a read
/// primitive for whatever it points at.
pub fn materialize_skill_bundle(
    persona: &ResolvedPersona,
    skills_root: &Path,
) -> Result<Vec<MaterializedSkill>, SkillError> {
    let root = create_bundle_root(skills_root)?;
    let mut out = Vec::with_capacity(persona.skills.len());
    for name in &persona.skills {
        let Some(skills_dir) = persona.skills_dir.as_deref() else {
            return Err(SkillError::NoSkillsDir {
                persona: persona.name.clone(),
                skill: name.clone(),
            });
        };
        out.push(bundle_one(name, skills_dir, &root)?);
    }
    // After the copies, not before: a refusal partway through leaves the
    // bundle the previous call wrote rather than emptying it.
    prune_dir(&root, &persona.skills.iter().map(OsString::from).collect())?;
    Ok(out)
}

/// Write `manifest` to `<bundle_dir>/manifest.json`, returning whether it
/// changed.
///
/// Idempotent in the same sense the skills are: when every field except the
/// timestamp already matches what is on disk, the file is left alone, its
/// existing timestamp included. So `materializedAt` reads "when this bundle
/// last changed", not "when a session last started" — a stamp that moved on
/// every spawn would say nothing.
///
/// # Errors
///
/// Filesystem failures on the manifest itself, and a JSON encoding failure
/// that cannot happen for the value built here but is not unwrapped anyway.
pub fn write_bundle_manifest(
    bundle_dir: &Path,
    manifest: &SkillBundleManifest,
) -> Result<bool, SkillError> {
    let mut skills = manifest.skills.clone();
    skills.sort_unstable();
    skills.dedup();

    let mut body = serde_json::Map::new();
    body.insert(
        "schema".to_owned(),
        serde_json::Value::String(SKILL_BUNDLE_SCHEMA.to_owned()),
    );
    body.insert(
        "personaId".to_owned(),
        serde_json::Value::String(manifest.persona_id.clone()),
    );
    body.insert(
        "packDir".to_owned(),
        serde_json::Value::String(manifest.pack_dir.to_string_lossy().into_owned()),
    );
    if let Some(pack_ref) = &manifest.pack_ref {
        body.insert("packRef".to_owned(), pack_ref.clone());
    }
    body.insert(
        "skills".to_owned(),
        serde_json::Value::Array(skills.into_iter().map(serde_json::Value::String).collect()),
    );

    let path = bundle_dir.join(SKILL_BUNDLE_MANIFEST_FILE);
    if let Ok(existing) = std::fs::read(&path) {
        if let Ok(serde_json::Value::Object(mut previous)) = serde_json::from_slice(&existing) {
            previous.remove("materializedAt");
            if previous == body {
                return Ok(false);
            }
        }
    }
    body.insert(
        "materializedAt".to_owned(),
        serde_json::Value::String(
            chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        ),
    );

    std::fs::create_dir_all(bundle_dir).map_err(|source| SkillError::Io {
        operation: "create",
        path: bundle_dir.to_path_buf(),
        source,
    })?;
    let mut rendered =
        serde_json::to_string_pretty(&serde_json::Value::Object(body)).map_err(|source| {
            SkillError::Io {
                operation: "encode",
                path: path.clone(),
                source: std::io::Error::new(std::io::ErrorKind::InvalidData, source),
            }
        })?;
    rendered.push('\n');
    std::fs::write(&path, rendered.as_bytes()).map_err(|source| SkillError::Io {
        operation: "write",
        path: path.clone(),
        source,
    })?;
    Ok(true)
}

/// Create `skills_root` if it is absent and return its resolved path.
///
/// The bundle root is the *caller's* directory — the provider's own state, not
/// a checkout a person edits — so creating it is ordinary. It is canonicalized
/// once, here, and every destination guard below is a comparison against that
/// one answer.
fn create_bundle_root(skills_root: &Path) -> Result<PathBuf, SkillError> {
    std::fs::create_dir_all(skills_root).map_err(|source| SkillError::Io {
        operation: "create",
        path: skills_root.to_path_buf(),
        source,
    })?;
    skills_root.canonicalize().map_err(|source| SkillError::Io {
        operation: "read skills bundle directory",
        path: skills_root.to_path_buf(),
        source,
    })
}

/// Copy one skill directory into the bundle.
fn bundle_one(name: &str, skills_dir: &Path, root: &Path) -> Result<MaterializedSkill, SkillError> {
    let component = safe_component(name)?;
    let (source, _pack_root) = resolved_source_dir(name, skills_dir, component)?;

    // The pack's own claim on the name `SKILL.md`: present, and a real file.
    // A symlink here is refused rather than resolved for the reason the whole
    // tree is — the bytes of whatever it points at would land in the seat's
    // bundle as instructions.
    let source_md = source.join(SKILL_MD);
    match source_md.symlink_metadata() {
        Ok(meta) if meta.file_type().is_symlink() => {
            return Err(SkillError::SourceEscape {
                name: name.to_owned(),
                path: source_md,
            })
        }
        Ok(meta) if meta.is_file() => {}
        Ok(_) => {
            return Err(SkillError::MissingSkillMd {
                name: name.to_owned(),
                path: source_md,
            })
        }
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            return Err(SkillError::MissingSkillMd {
                name: name.to_owned(),
                path: source_md,
            })
        }
        Err(source) => {
            return Err(SkillError::Io {
                operation: "read",
                path: source_md,
                source,
            })
        }
    }

    let target = bundle_child_dir(name, root, root, std::ffi::OsStr::new(component))?;
    let written = copy_tree(name, &source, &target, root)?;
    Ok(MaterializedSkill {
        name: name.to_owned(),
        path: target.join(SKILL_MD),
        written,
    })
}

/// The pack's own directory for `component`, resolved and proved to be inside
/// the pack's `skills/` root — which is returned beside it, because a caller
/// that checks a file *inside* the directory needs the same root to check it
/// against.
fn resolved_source_dir(
    name: &str,
    skills_dir: &Path,
    component: &str,
) -> Result<(PathBuf, PathBuf), SkillError> {
    let source_dir = skills_dir.join(component);
    let resolved = source_dir.canonicalize().map_err(|source| SkillError::Io {
        operation: "read skill directory",
        path: source_dir,
        source,
    })?;
    let root = skills_dir.canonicalize().map_err(|source| SkillError::Io {
        operation: "read pack skills directory",
        path: skills_dir.to_path_buf(),
        source,
    })?;
    if !resolved.starts_with(&root) {
        return Err(SkillError::SourceEscape {
            name: name.to_owned(),
            path: resolved,
        });
    }
    Ok((resolved, root))
}

/// Copy `source` onto `target`, recursively, and prune what the pack dropped.
///
/// Returns whether anything changed. Symlinks are refused on both sides; a
/// source entry that is neither a directory nor a regular file (a socket, a
/// device node) is not content and is skipped — and, because it is not
/// recorded as seen, a stale copy of it under the bundle is pruned.
fn copy_tree(name: &str, source: &Path, target: &Path, root: &Path) -> Result<bool, SkillError> {
    let mut changed = false;
    let mut seen: BTreeSet<OsString> = BTreeSet::new();
    let entries = std::fs::read_dir(source).map_err(|io| SkillError::Io {
        operation: "read skill directory",
        path: source.to_path_buf(),
        source: io,
    })?;
    for entry in entries {
        let entry = entry.map_err(|io| SkillError::Io {
            operation: "read skill directory",
            path: source.to_path_buf(),
            source: io,
        })?;
        let path = entry.path();
        // `DirEntry::file_type` does not follow the link, which is what makes
        // the refusal below a refusal rather than a resolution.
        let file_type = entry.file_type().map_err(|io| SkillError::Io {
            operation: "read",
            path: path.clone(),
            source: io,
        })?;
        if file_type.is_symlink() {
            return Err(SkillError::SourceEscape {
                name: name.to_owned(),
                path,
            });
        }
        let file_name = entry.file_name();
        if file_type.is_dir() {
            let child = bundle_child_dir(name, root, target, &file_name)?;
            changed |= copy_tree(name, &path, &child, root)?;
            seen.insert(file_name);
        } else if file_type.is_file() {
            changed |= copy_file(name, &path, &target.join(&file_name))?;
            seen.insert(file_name);
        }
    }
    changed |= prune_dir(target, &seen)?;
    Ok(changed)
}

/// Copy one file, writing only when the bytes differ.
fn copy_file(name: &str, source: &Path, target: &Path) -> Result<bool, SkillError> {
    if target
        .symlink_metadata()
        .is_ok_and(|meta| meta.file_type().is_symlink())
    {
        return Err(SkillError::TargetEscape {
            name: name.to_owned(),
            path: target.to_path_buf(),
        });
    }
    let body = std::fs::read(source).map_err(|io| SkillError::Io {
        operation: "read",
        path: source.to_path_buf(),
        source: io,
    })?;
    let mut changed = false;
    if !std::fs::read(target).is_ok_and(|existing| existing == body) {
        std::fs::write(target, &body).map_err(|io| SkillError::Io {
            operation: "write",
            path: target.to_path_buf(),
            source: io,
        })?;
        changed = true;
    }
    changed |= copy_executable_bit(source, target)?;
    Ok(changed)
}

/// Carry a source file's executable bit onto its copy.
///
/// A skill that ships a helper script is ordinary, and a copy of it that
/// cannot be run is a skill the seat half has. Set only when it differs, so
/// this stays idempotent; changing a mode does not touch mtime.
#[cfg(unix)]
fn copy_executable_bit(source: &Path, target: &Path) -> Result<bool, SkillError> {
    use std::os::unix::fs::PermissionsExt as _;
    let read = |path: &Path| {
        std::fs::metadata(path).map_err(|io| SkillError::Io {
            operation: "read",
            path: path.to_path_buf(),
            source: io,
        })
    };
    let wanted = read(source)?.permissions().mode() & 0o111;
    let mut permissions = read(target)?.permissions();
    if permissions.mode() & 0o111 == wanted {
        return Ok(false);
    }
    let mode = (permissions.mode() & !0o111) | wanted;
    permissions.set_mode(mode);
    std::fs::set_permissions(target, permissions).map_err(|io| SkillError::Io {
        operation: "set permissions on",
        path: target.to_path_buf(),
        source: io,
    })?;
    Ok(true)
}

/// Windows has no executable bit; the copy is the file.
#[cfg(not(unix))]
fn copy_executable_bit(_source: &Path, _target: &Path) -> Result<bool, SkillError> {
    Ok(false)
}

/// Resolve (creating it) one directory under `parent`, refusing a symlink and
/// anything that would land outside `root`.
fn bundle_child_dir(
    name: &str,
    root: &Path,
    parent: &Path,
    child: &std::ffi::OsStr,
) -> Result<PathBuf, SkillError> {
    let escaped = |path: PathBuf| SkillError::TargetEscape {
        name: name.to_owned(),
        path,
    };
    let path = parent.join(child);
    // Defence in depth: every `child` here is either a checked skill name or a
    // single entry name from `read_dir`, so this cannot fire — and if it ever
    // does, it fires before anything is created.
    if !path.starts_with(root) || Path::new(child).components().count() != 1 {
        return Err(escaped(path));
    }
    match path.symlink_metadata() {
        Ok(meta) if meta.file_type().is_symlink() => Err(escaped(path)),
        Ok(meta) if meta.is_dir() => Ok(path),
        // A plain file where the pack now has a directory is stale bundle
        // content, not somebody's data: the bundle root belongs to this
        // execution alone. Replacing it is the same pruning rule one level up.
        Ok(_) => {
            std::fs::remove_file(&path).map_err(|source| SkillError::Io {
                operation: "remove",
                path: path.clone(),
                source,
            })?;
            create_dir(path)
        }
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => create_dir(path),
        Err(source) => Err(SkillError::Io {
            operation: "read",
            path,
            source,
        }),
    }
}

fn create_dir(path: PathBuf) -> Result<PathBuf, SkillError> {
    std::fs::create_dir(&path).map_err(|source| SkillError::Io {
        operation: "create",
        path: path.clone(),
        source,
    })?;
    Ok(path)
}

/// Remove everything in `dir` that is not named in `keep`, and say whether
/// anything went.
///
/// Only ever called with a directory inside the bundle root, and it never
/// follows a link out of one: a symlinked entry is removed as a link
/// (`remove_file`), not walked into.
fn prune_dir(dir: &Path, keep: &BTreeSet<OsString>) -> Result<bool, SkillError> {
    let mut removed = false;
    let entries = std::fs::read_dir(dir).map_err(|source| SkillError::Io {
        operation: "read",
        path: dir.to_path_buf(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| SkillError::Io {
            operation: "read",
            path: dir.to_path_buf(),
            source,
        })?;
        let file_name = entry.file_name();
        if keep.contains(&file_name) {
            continue;
        }
        let path = entry.path();
        let file_type = entry.file_type().map_err(|source| SkillError::Io {
            operation: "read",
            path: path.clone(),
            source,
        })?;
        let outcome = if file_type.is_dir() {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };
        outcome.map_err(|source| SkillError::Io {
            operation: "remove",
            path: path.clone(),
            source,
        })?;
        removed = true;
    }
    Ok(removed)
}

/// Resolve (creating it) `<workdir>/.agents/skills`, refusing a resolution
/// that leaves the workdir.
///
/// The workdir itself is created when absent, because callers on a spawn path
/// hand over a directory they are about to run in; only what lies *under* it
/// is treated as untrusted.
fn resolved_skills_root(name: &str, workdir: &Path) -> Result<PathBuf, SkillError> {
    std::fs::create_dir_all(workdir).map_err(|source| SkillError::Io {
        operation: "create",
        path: workdir.to_path_buf(),
        source,
    })?;
    let root = workdir.canonicalize().map_err(|source| SkillError::Io {
        operation: "read workdir",
        path: workdir.to_path_buf(),
        source,
    })?;
    let mut current = root.clone();
    for part in Path::new(SEAT_SKILLS_DIR).components() {
        // `SEAT_SKILLS_DIR` is this module's own constant of ordinary
        // components; anything else would be a bug here, not input.
        let Component::Normal(part) = part else {
            return Err(SkillError::TargetEscape {
                name: name.to_owned(),
                path: current.join(SEAT_SKILLS_DIR),
            });
        };
        current = resolve_child(name, &root, &current, part)?;
    }
    Ok(current)
}

/// Resolve one directory component under `parent`, creating it when absent,
/// and refuse anything that does not resolve to a directory under `root`.
fn resolve_child(
    name: &str,
    root: &Path,
    parent: &Path,
    child: &std::ffi::OsStr,
) -> Result<PathBuf, SkillError> {
    let path = parent.join(child);
    let escaped = |path: PathBuf| SkillError::TargetEscape {
        name: name.to_owned(),
        path,
    };
    match path.symlink_metadata() {
        Ok(meta) if meta.file_type().is_symlink() => {
            let resolved = path.canonicalize().map_err(|_| escaped(path.clone()))?;
            if !resolved.starts_with(root) || !resolved.is_dir() {
                return Err(escaped(resolved));
            }
            Ok(resolved)
        }
        Ok(meta) if meta.is_dir() => Ok(path),
        Ok(_) => Err(SkillError::Io {
            operation: "create",
            path: path.clone(),
            source: std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "exists and is not a directory",
            ),
        }),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir(&path).map_err(|source| SkillError::Io {
                operation: "create",
                path: path.clone(),
                source,
            })?;
            Ok(path)
        }
        Err(source) => Err(SkillError::Io {
            operation: "read",
            path,
            source,
        }),
    }
}

/// Accept a skill name only when it is one ordinary path component.
///
/// Rejects `..`, `.`, absolute paths, nested paths, and anything with a
/// separator — the name is joined onto both a pack path and a workdir path.
fn safe_component(name: &str) -> Result<&str, SkillError> {
    let path = Path::new(name);
    let mut components = path.components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(single)), None) if single == name && !name.is_empty() => Ok(name),
        _ => Err(SkillError::UnsafeName(name.to_owned())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn pack_with_skills(dir: &Path, skills: &[(&str, &str)]) -> PathBuf {
        let skills_dir = dir.join("skills");
        for (name, body) in skills {
            let d = skills_dir.join(name);
            fs::create_dir_all(&d).expect("create skill dir");
            fs::write(d.join("SKILL.md"), body).expect("write SKILL.md");
        }
        skills_dir
    }

    fn persona(name: &str, skills: &[&str], skills_dir: Option<PathBuf>) -> ResolvedPersona {
        ResolvedPersona {
            name: name.to_owned(),
            display_name: name.to_owned(),
            description: "test".into(),
            avatar: None,
            version: "0.1.0".into(),
            role: Some("builder".into()),
            system_prompt: String::new(),
            pack_instructions: None,
            model: None,
            llm_provider: None,
            runtime: None,
            temperature: None,
            max_context_tokens: None,
            subscribe: vec![],
            triggers: crate::resolve::ResolvedTriggers {
                mentions: true,
                keywords: vec![],
                all_messages: false,
            },
            thread_replies: true,
            broadcast_replies: false,
            mcp_servers: vec![],
            hooks: None,
            skills: skills.iter().map(|s| (*s).to_owned()).collect(),
            skills_dir,
            runtime_env_vars: vec![],
        }
    }

    #[test]
    fn writes_each_skill_under_the_workdir() {
        let pack = TempDir::new().unwrap();
        let skills_dir =
            pack_with_skills(pack.path(), &[("brief", "# Brief"), ("report", "# Rep")]);
        let workdir = TempDir::new().unwrap();

        let written = materialize_skills(
            &persona("builder", &["brief", "report"], Some(skills_dir)),
            workdir.path(),
        )
        .expect("materializes");

        assert_eq!(written.len(), 2);
        assert!(written.iter().all(|s| s.written));
        let brief = workdir.path().join(".agents/skills/brief/SKILL.md");
        assert_eq!(fs::read_to_string(&brief).unwrap(), "# Brief");
        assert_eq!(written[0].path, brief.canonicalize().unwrap());
        assert_eq!(
            fs::read_to_string(workdir.path().join(".agents/skills/report/SKILL.md")).unwrap(),
            "# Rep"
        );
    }

    #[test]
    fn a_second_call_writes_nothing() {
        let pack = TempDir::new().unwrap();
        let skills_dir = pack_with_skills(pack.path(), &[("brief", "# Brief")]);
        let workdir = TempDir::new().unwrap();
        let p = persona("builder", &["brief"], Some(skills_dir));

        let first = materialize_skills(&p, workdir.path()).expect("first");
        assert!(first[0].written, "first call writes");
        let mtime = fs::metadata(&first[0].path).unwrap().modified().unwrap();

        let second = materialize_skills(&p, workdir.path()).expect("second");
        assert!(!second[0].written, "second call is a no-op");
        assert_eq!(first[0].path, second[0].path);
        assert_eq!(
            fs::metadata(&second[0].path).unwrap().modified().unwrap(),
            mtime,
            "an unchanged skill is not rewritten"
        );
    }

    #[test]
    fn changed_pack_content_is_refreshed() {
        let pack = TempDir::new().unwrap();
        let skills_dir = pack_with_skills(pack.path(), &[("brief", "# Brief v1")]);
        let workdir = TempDir::new().unwrap();
        let p = persona("builder", &["brief"], Some(skills_dir.clone()));
        materialize_skills(&p, workdir.path()).expect("first");

        fs::write(skills_dir.join("brief/SKILL.md"), "# Brief v2").unwrap();
        let again = materialize_skills(&p, workdir.path()).expect("second");

        assert!(again[0].written);
        assert_eq!(fs::read_to_string(&again[0].path).unwrap(), "# Brief v2");
    }

    #[test]
    fn each_workdir_gets_its_own_copy() {
        let pack = TempDir::new().unwrap();
        let skills_dir = pack_with_skills(pack.path(), &[("brief", "# Brief")]);
        let one = TempDir::new().unwrap();
        let two = TempDir::new().unwrap();
        let p = persona("builder", &["brief"], Some(skills_dir));

        materialize_skills(&p, one.path()).expect("one");
        materialize_skills(&p, two.path()).expect("two");

        // A seat edits its own copy; the sibling workdir is untouched.
        fs::write(
            one.path().join(".agents/skills/brief/SKILL.md"),
            "local edit",
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(two.path().join(".agents/skills/brief/SKILL.md")).unwrap(),
            "# Brief"
        );
    }

    #[test]
    fn a_traversing_name_is_refused() {
        let pack = TempDir::new().unwrap();
        let skills_dir = pack_with_skills(pack.path(), &[("brief", "# Brief")]);
        fs::write(pack.path().join("SKILL.md"), "outside").unwrap();
        let workdir = TempDir::new().unwrap();

        for name in ["../..", "..", ".", "nested/brief", "/etc"] {
            let error = materialize_skills(
                &persona("builder", &[name], Some(skills_dir.clone())),
                workdir.path(),
            )
            .expect_err("traversal is refused");
            assert!(
                matches!(error, SkillError::UnsafeName(_)),
                "{name}: unexpected error {error}"
            );
        }
        assert!(
            !workdir.path().join(".agents").exists(),
            "a refused name writes nothing"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_skill_pointing_out_of_the_pack_is_refused() {
        let pack = TempDir::new().unwrap();
        let skills_dir = pack_with_skills(pack.path(), &[("brief", "# Brief")]);
        let outside = TempDir::new().unwrap();
        fs::create_dir_all(outside.path().join("evil")).unwrap();
        fs::write(outside.path().join("evil/SKILL.md"), "escaped").unwrap();
        std::os::unix::fs::symlink(outside.path().join("evil"), skills_dir.join("evil")).unwrap();
        let workdir = TempDir::new().unwrap();

        let error = materialize_skills(
            &persona("builder", &["evil"], Some(skills_dir)),
            workdir.path(),
        )
        .expect_err("escape is refused");
        assert!(
            matches!(error, SkillError::SourceEscape { .. }),
            "unexpected error {error}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_skill_md_pointing_out_of_the_pack_is_refused() {
        // The directory-level check says nothing about the file inside it. A
        // real directory in the pack whose SKILL.md is a symlink to a secret
        // is the same escape one level down, and it turns a pack into an
        // arbitrary-file *read* that deposits the bytes in a seat's workdir.
        let pack = TempDir::new().unwrap();
        let skills_dir = pack_with_skills(pack.path(), &[("brief", "# Brief")]);
        let outside = TempDir::new().unwrap();
        let secret = outside.path().join("key");
        fs::write(&secret, "nsec-not-yours").unwrap();
        let evil = skills_dir.join("evil");
        fs::create_dir_all(&evil).unwrap();
        std::os::unix::fs::symlink(&secret, evil.join("SKILL.md")).unwrap();
        let workdir = TempDir::new().unwrap();

        let error = materialize_skills(
            &persona("builder", &["evil"], Some(skills_dir)),
            workdir.path(),
        )
        .expect_err("a SKILL.md pointing out of the pack is refused");

        assert!(
            matches!(error, SkillError::SourceEscape { .. }),
            "unexpected error {error}"
        );
        assert!(
            !workdir.path().join(".agents/skills/evil/SKILL.md").exists(),
            "the outside file's bytes must not reach the workdir"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_skill_md_inside_the_pack_is_still_read() {
        // Symmetry: the rule is "under the pack", not "never a symlink" — a
        // pack that shares one SKILL.md between two skills is ordinary.
        let pack = TempDir::new().unwrap();
        let skills_dir = pack_with_skills(pack.path(), &[("brief", "# Brief")]);
        let alias = skills_dir.join("alias");
        fs::create_dir_all(&alias).unwrap();
        std::os::unix::fs::symlink(skills_dir.join("brief/SKILL.md"), alias.join("SKILL.md"))
            .unwrap();
        let workdir = TempDir::new().unwrap();

        let written = materialize_skills(
            &persona("builder", &["alias"], Some(skills_dir)),
            workdir.path(),
        )
        .expect("a symlink inside the pack is ordinary");

        assert_eq!(fs::read_to_string(&written[0].path).unwrap(), "# Brief");
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_agents_dir_pointing_out_of_the_workdir_is_refused() {
        // The workdir is the *other* untrusted half of this write. A `.agents`
        // symlink is the ordinary way people share one skills directory
        // between checkouts, and it turns this function into an overwrite of
        // whatever it points at — including the human's own
        // `~/.agents/skills/<name>/SKILL.md`.
        let pack = TempDir::new().unwrap();
        let skills_dir = pack_with_skills(pack.path(), &[("brief", "# Brief")]);
        let outside = TempDir::new().unwrap();
        fs::create_dir_all(outside.path().join("skills/brief")).unwrap();
        fs::write(outside.path().join("skills/brief/SKILL.md"), "# Mine").unwrap();
        let workdir = TempDir::new().unwrap();
        std::os::unix::fs::symlink(outside.path(), workdir.path().join(".agents")).unwrap();

        let error = materialize_skills(
            &persona("builder", &["brief"], Some(skills_dir)),
            workdir.path(),
        )
        .expect_err("a .agents that leaves the workdir is refused");

        assert!(
            matches!(error, SkillError::TargetEscape { .. }),
            "unexpected error {error}"
        );
        assert_eq!(
            fs::read_to_string(outside.path().join("skills/brief/SKILL.md")).unwrap(),
            "# Mine",
            "the file outside the workdir must not be overwritten"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_destination_skill_dir_pointing_out_of_the_workdir_is_refused() {
        let pack = TempDir::new().unwrap();
        let skills_dir = pack_with_skills(pack.path(), &[("brief", "# Brief")]);
        let outside = TempDir::new().unwrap();
        fs::write(outside.path().join("SKILL.md"), "# Mine").unwrap();
        let workdir = TempDir::new().unwrap();
        fs::create_dir_all(workdir.path().join(".agents/skills")).unwrap();
        std::os::unix::fs::symlink(outside.path(), workdir.path().join(".agents/skills/brief"))
            .unwrap();

        let error = materialize_skills(
            &persona("builder", &["brief"], Some(skills_dir)),
            workdir.path(),
        )
        .expect_err("a skill directory that leaves the workdir is refused");

        assert!(
            matches!(error, SkillError::TargetEscape { .. }),
            "unexpected error {error}"
        );
        assert_eq!(
            fs::read_to_string(outside.path().join("SKILL.md")).unwrap(),
            "# Mine"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_destination_skill_md_is_refused() {
        let pack = TempDir::new().unwrap();
        let skills_dir = pack_with_skills(pack.path(), &[("brief", "# Brief")]);
        let outside = TempDir::new().unwrap();
        let mine = outside.path().join("SKILL.md");
        fs::write(&mine, "# Mine").unwrap();
        let workdir = TempDir::new().unwrap();
        let target_dir = workdir.path().join(".agents/skills/brief");
        fs::create_dir_all(&target_dir).unwrap();
        std::os::unix::fs::symlink(&mine, target_dir.join("SKILL.md")).unwrap();

        let error = materialize_skills(
            &persona("builder", &["brief"], Some(skills_dir)),
            workdir.path(),
        )
        .expect_err("a destination SKILL.md that leaves the workdir is refused");

        assert!(
            matches!(error, SkillError::TargetEscape { .. }),
            "unexpected error {error}"
        );
        assert_eq!(fs::read_to_string(&mine).unwrap(), "# Mine");
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_agents_dir_inside_the_workdir_is_still_written() {
        // Symmetry, same as the read side: the rule is "under the workdir",
        // not "never a symlink".
        let pack = TempDir::new().unwrap();
        let skills_dir = pack_with_skills(pack.path(), &[("brief", "# Brief")]);
        let workdir = TempDir::new().unwrap();
        fs::create_dir_all(workdir.path().join("dot-agents")).unwrap();
        std::os::unix::fs::symlink(
            workdir.path().join("dot-agents"),
            workdir.path().join(".agents"),
        )
        .unwrap();

        let written = materialize_skills(
            &persona("builder", &["brief"], Some(skills_dir)),
            workdir.path(),
        )
        .expect("a symlink that stays under the workdir is ordinary");

        assert_eq!(fs::read_to_string(&written[0].path).unwrap(), "# Brief");
        assert_eq!(
            fs::read_to_string(workdir.path().join("dot-agents/skills/brief/SKILL.md")).unwrap(),
            "# Brief"
        );
    }

    #[test]
    fn a_skill_without_a_skill_md_is_an_error() {
        let pack = TempDir::new().unwrap();
        let skills_dir = pack.path().join("skills");
        fs::create_dir_all(skills_dir.join("empty")).unwrap();
        let workdir = TempDir::new().unwrap();

        let error = materialize_skills(
            &persona("builder", &["empty"], Some(skills_dir)),
            workdir.path(),
        )
        .expect_err("missing SKILL.md");
        assert!(
            matches!(error, SkillError::MissingSkillMd { .. }),
            "unexpected error {error}"
        );
    }

    #[test]
    fn a_persona_with_no_skills_creates_nothing() {
        let workdir = TempDir::new().unwrap();
        let written =
            materialize_skills(&persona("builder", &[], None), workdir.path()).expect("no-op");
        assert!(written.is_empty());
        assert!(!workdir.path().join(".agents").exists());
    }

    #[test]
    fn a_claimed_skill_with_no_pack_skills_dir_is_an_error() {
        let workdir = TempDir::new().unwrap();
        let error = materialize_skills(&persona("builder", &["brief"], None), workdir.path())
            .expect_err("no skills dir");
        assert!(
            matches!(error, SkillError::NoSkillsDir { .. }),
            "unexpected error {error}"
        );
    }

    // -----------------------------------------------------------------------
    // The execution-owned bundle.
    // -----------------------------------------------------------------------

    /// Write a skill directory with extra files beside its `SKILL.md`.
    fn skill_with_support(skills_dir: &Path, name: &str, body: &str) {
        let dir = skills_dir.join(name);
        fs::create_dir_all(dir.join("templates")).expect("templates dir");
        fs::write(dir.join("SKILL.md"), body).expect("SKILL.md");
        fs::write(dir.join("checklist.md"), "- [ ] one").expect("checklist");
        fs::write(dir.join("templates/report.md"), "# Report").expect("template");
    }

    /// The whole point of the bundle: a skill's supporting files reach the
    /// seat. Copying `SKILL.md` alone hands an agent instructions that name
    /// files it does not have.
    #[test]
    fn a_bundle_copies_the_whole_skill_directory() {
        let pack = TempDir::new().unwrap();
        let skills_dir = pack.path().join("skills");
        skill_with_support(&skills_dir, "brief", "# Brief");
        let bundle = TempDir::new().unwrap();
        let root = bundle.path().join("skills");

        let written =
            materialize_skill_bundle(&persona("builder", &["brief"], Some(skills_dir)), &root)
                .expect("bundle");

        assert!(written[0].written);
        assert_eq!(
            written[0].path,
            root.join("brief/SKILL.md").canonicalize().unwrap()
        );
        assert_eq!(
            fs::read_to_string(root.join("brief/SKILL.md")).unwrap(),
            "# Brief"
        );
        assert_eq!(
            fs::read_to_string(root.join("brief/checklist.md")).unwrap(),
            "- [ ] one"
        );
        assert_eq!(
            fs::read_to_string(root.join("brief/templates/report.md")).unwrap(),
            "# Report"
        );
    }

    /// A second call with an unchanged pack rewrites nothing, mtime included —
    /// the property that makes this safe on every spawn of a restarting
    /// session.
    #[test]
    fn a_second_bundle_call_writes_nothing() {
        let pack = TempDir::new().unwrap();
        let skills_dir = pack.path().join("skills");
        skill_with_support(&skills_dir, "brief", "# Brief");
        let bundle = TempDir::new().unwrap();
        let root = bundle.path().join("skills");
        let p = persona("builder", &["brief"], Some(skills_dir));

        let first = materialize_skill_bundle(&p, &root).expect("first");
        assert!(first[0].written);
        let stamps: Vec<_> = ["brief/SKILL.md", "brief/templates/report.md"]
            .iter()
            .map(|rel| fs::metadata(root.join(rel)).unwrap().modified().unwrap())
            .collect();

        let second = materialize_skill_bundle(&p, &root).expect("second");

        assert!(!second[0].written, "an unchanged pack is a no-op");
        for (rel, was) in ["brief/SKILL.md", "brief/templates/report.md"]
            .iter()
            .zip(stamps)
        {
            assert_eq!(
                fs::metadata(root.join(rel)).unwrap().modified().unwrap(),
                was,
                "{rel} was rewritten"
            );
        }
    }

    /// Stale craft is worse than missing craft: a seat reading the previous
    /// role's skill acts on it. A skill the persona no longer claims goes, and
    /// so does a file the pack dropped from a skill it still claims.
    #[test]
    fn the_bundle_is_pruned_of_what_the_pack_dropped() {
        let pack = TempDir::new().unwrap();
        let skills_dir = pack.path().join("skills");
        skill_with_support(&skills_dir, "brief", "# Brief");
        skill_with_support(&skills_dir, "retired", "# Retired");
        let bundle = TempDir::new().unwrap();
        let root = bundle.path().join("skills");

        materialize_skill_bundle(
            &persona("builder", &["brief", "retired"], Some(skills_dir.clone())),
            &root,
        )
        .expect("first");
        assert!(root.join("retired/SKILL.md").exists());

        // The pack drops one supporting file; the persona drops a whole skill.
        fs::remove_file(skills_dir.join("brief/checklist.md")).expect("drop file");
        let again =
            materialize_skill_bundle(&persona("builder", &["brief"], Some(skills_dir)), &root)
                .expect("second");

        assert!(again[0].written, "a prune is a change");
        assert!(!root.join("retired").exists(), "the stale skill survived");
        assert!(
            !root.join("brief/checklist.md").exists(),
            "the stale supporting file survived"
        );
        assert!(root.join("brief/templates/report.md").exists());
    }

    /// Nothing outside the bundle root is ever removed, whatever the root
    /// sits next to.
    #[test]
    fn pruning_never_leaves_the_bundle_root() {
        let pack = TempDir::new().unwrap();
        let skills_dir = pack.path().join("skills");
        skill_with_support(&skills_dir, "brief", "# Brief");
        let bundle = TempDir::new().unwrap();
        let root = bundle.path().join("skills");
        let sibling = bundle.path().join("notes.md");
        fs::write(&sibling, "mine").expect("sibling");

        materialize_skill_bundle(&persona("builder", &["brief"], Some(skills_dir)), &root)
            .expect("bundle");

        assert_eq!(fs::read_to_string(&sibling).unwrap(), "mine");
    }

    /// A persona that claims no skills empties its bundle rather than leaving
    /// the previous role's craft in it.
    #[test]
    fn a_persona_with_no_skills_empties_the_bundle() {
        let pack = TempDir::new().unwrap();
        let skills_dir = pack.path().join("skills");
        skill_with_support(&skills_dir, "brief", "# Brief");
        let bundle = TempDir::new().unwrap();
        let root = bundle.path().join("skills");
        materialize_skill_bundle(&persona("builder", &["brief"], Some(skills_dir)), &root)
            .expect("first");

        let written =
            materialize_skill_bundle(&persona("builder", &[], None), &root).expect("no skills");

        assert!(written.is_empty());
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_supporting_file_leaving_the_pack_is_refused() {
        // The same escape as a symlinked SKILL.md, one file along: without
        // this the pack is an arbitrary-file read that deposits the bytes in
        // the seat's bundle.
        let pack = TempDir::new().unwrap();
        let skills_dir = pack.path().join("skills");
        skill_with_support(&skills_dir, "brief", "# Brief");
        let outside = TempDir::new().unwrap();
        let secret = outside.path().join("key");
        fs::write(&secret, "nsec-not-yours").unwrap();
        std::os::unix::fs::symlink(&secret, skills_dir.join("brief/notes.md")).unwrap();
        let bundle = TempDir::new().unwrap();
        let root = bundle.path().join("skills");

        let error =
            materialize_skill_bundle(&persona("builder", &["brief"], Some(skills_dir)), &root)
                .expect_err("a symlinked supporting file is refused");

        assert!(
            matches!(error, SkillError::SourceEscape { .. }),
            "unexpected error {error}"
        );
        assert!(
            !root.join("brief/notes.md").exists(),
            "the outside file's bytes must not reach the bundle"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_inside_the_pack_is_refused_in_a_bundle_too() {
        // Stricter than `materialize_skills`, deliberately: a whole-directory
        // copy cannot check each link's target the way one known file could,
        // and a bundle is written where nobody is watching it.
        let pack = TempDir::new().unwrap();
        let skills_dir = pack.path().join("skills");
        skill_with_support(&skills_dir, "brief", "# Brief");
        std::os::unix::fs::symlink(
            skills_dir.join("brief/checklist.md"),
            skills_dir.join("brief/alias.md"),
        )
        .unwrap();
        let bundle = TempDir::new().unwrap();

        let error = materialize_skill_bundle(
            &persona("builder", &["brief"], Some(skills_dir)),
            &bundle.path().join("skills"),
        )
        .expect_err("a symlink in the source tree is refused");

        assert!(
            matches!(error, SkillError::SourceEscape { .. }),
            "unexpected error {error}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_destination_file_in_the_bundle_is_refused() {
        let pack = TempDir::new().unwrap();
        let skills_dir = pack.path().join("skills");
        skill_with_support(&skills_dir, "brief", "# Brief");
        let outside = TempDir::new().unwrap();
        let mine = outside.path().join("SKILL.md");
        fs::write(&mine, "# Mine").unwrap();
        let bundle = TempDir::new().unwrap();
        let root = bundle.path().join("skills");
        fs::create_dir_all(root.join("brief")).unwrap();
        std::os::unix::fs::symlink(&mine, root.join("brief/SKILL.md")).unwrap();

        let error =
            materialize_skill_bundle(&persona("builder", &["brief"], Some(skills_dir)), &root)
                .expect_err("a symlinked destination is refused");

        assert!(
            matches!(error, SkillError::TargetEscape { .. }),
            "unexpected error {error}"
        );
        assert_eq!(fs::read_to_string(&mine).unwrap(), "# Mine");
    }

    #[test]
    fn a_bundle_refuses_a_traversing_name() {
        let pack = TempDir::new().unwrap();
        let skills_dir = pack.path().join("skills");
        skill_with_support(&skills_dir, "brief", "# Brief");
        let bundle = TempDir::new().unwrap();
        let root = bundle.path().join("skills");

        for name in ["../..", "..", ".", "nested/brief", "/etc"] {
            let error = materialize_skill_bundle(
                &persona("builder", &[name], Some(skills_dir.clone())),
                &root,
            )
            .expect_err("traversal is refused");
            assert!(
                matches!(error, SkillError::UnsafeName(_)),
                "{name}: unexpected error {error}"
            );
        }
    }

    #[test]
    fn a_bundled_skill_without_a_skill_md_is_an_error() {
        let pack = TempDir::new().unwrap();
        let skills_dir = pack.path().join("skills");
        fs::create_dir_all(skills_dir.join("empty")).unwrap();
        fs::write(skills_dir.join("empty/notes.md"), "no skill").unwrap();
        let bundle = TempDir::new().unwrap();

        let error = materialize_skill_bundle(
            &persona("builder", &["empty"], Some(skills_dir)),
            &bundle.path().join("skills"),
        )
        .expect_err("missing SKILL.md");

        assert!(
            matches!(error, SkillError::MissingSkillMd { .. }),
            "unexpected error {error}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_skills_executable_script_stays_executable() {
        use std::os::unix::fs::PermissionsExt as _;
        let pack = TempDir::new().unwrap();
        let skills_dir = pack.path().join("skills");
        skill_with_support(&skills_dir, "brief", "# Brief");
        let script = skills_dir.join("brief/run.sh");
        fs::write(&script, "#!/bin/sh\necho hi\n").unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        let bundle = TempDir::new().unwrap();
        let root = bundle.path().join("skills");

        materialize_skill_bundle(&persona("builder", &["brief"], Some(skills_dir)), &root)
            .expect("bundle");

        let mode = fs::metadata(root.join("brief/run.sh"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o111, 0o111, "the copy cannot be run: {mode:o}");
    }

    #[test]
    fn the_manifest_records_the_pack_and_keeps_its_timestamp() {
        let bundle = TempDir::new().unwrap();
        let manifest = SkillBundleManifest {
            persona_id: "builder".into(),
            pack_dir: PathBuf::from("/packs/roles"),
            pack_ref: Some(serde_json::json!({
                "repo": "30617:abc:packs",
                "sha": "f".repeat(40),
                "role": "builder",
                "path": "personas/roles/builder",
            })),
            skills: vec!["report".into(), "brief".into(), "brief".into()],
        };

        assert!(write_bundle_manifest(bundle.path(), &manifest).expect("write"));
        let path = bundle.path().join(SKILL_BUNDLE_MANIFEST_FILE);
        let first: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(first["schema"], SKILL_BUNDLE_SCHEMA);
        assert_eq!(first["personaId"], "builder");
        assert_eq!(first["packDir"], "/packs/roles");
        assert_eq!(first["packRef"]["role"], "builder");
        assert_eq!(
            first["skills"],
            serde_json::json!(["brief", "report"]),
            "sorted and deduplicated"
        );
        let stamp = first["materializedAt"]
            .as_str()
            .expect("timestamp")
            .to_owned();
        assert!(stamp.ends_with('Z'), "not a UTC timestamp: {stamp}");

        // Same bundle, second spawn: unchanged, so the file is left alone and
        // the stamp still says when the bundle last changed.
        assert!(!write_bundle_manifest(bundle.path(), &manifest).expect("second"));
        let second: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(second["materializedAt"], serde_json::Value::String(stamp));

        // A changed pack rewrites it.
        let moved = SkillBundleManifest {
            pack_dir: PathBuf::from("/packs/other"),
            ..manifest
        };
        assert!(write_bundle_manifest(bundle.path(), &moved).expect("third"));
        let third: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(third["packDir"], "/packs/other");
    }
}
