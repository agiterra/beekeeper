//! Materialize a resolved persona's skills into a seat's working directory.
//!
//! A pack's `skills/<name>/SKILL.md` is craft the persona's prompt only refers
//! to; an agent can read it only if it exists *in the directory the agent runs
//! in*. This module is the step that puts it there, and it is deliberately the
//! smallest possible one:
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

    /// The destination skill directory, or its `SKILL.md`, resolves outside
    /// the workdir's `.agents/skills` root.
    #[error("skill \"{name}\" would be written outside the workdir skills directory: {path}")]
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

/// One skill as it exists in the seat's working directory after a call to
/// [`materialize_skills`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterializedSkill {
    /// Bare skill name (the directory name in the pack and in the workdir).
    pub name: String,
    /// Absolute, symlink-resolved path to the written `SKILL.md` — where the
    /// bytes actually landed, which is what the destination guard checked.
    pub path: PathBuf,
    /// Whether this call changed the file. `false` means the workdir already
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

    let source_dir = skills_dir.join(component);
    // `canonicalize` resolves `..` and symlinks; a skill directory that is a
    // symlink pointing out of the pack is the same escape as a `..` name, and
    // is refused for the same reason.
    let resolved = source_dir.canonicalize().map_err(|source| SkillError::Io {
        operation: "read skill directory",
        path: source_dir.clone(),
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
}
