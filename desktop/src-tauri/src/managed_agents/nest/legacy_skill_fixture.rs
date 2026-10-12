//! Historical legacy pack input fixtures; no production materializer.
use super::*;
/// The directories on this computer no single seat owns.
///
/// The two that matter are the nest (`~/.beekeeper`) and the user's home
/// directory, which is what [`super::default_agent_workdir`] falls back to
/// when the nest is missing or is a symlink. Neither belongs to one seat.
fn shared_agent_workdir_roots() -> Vec<PathBuf> {
    [nest_dir(), dirs::home_dir()]
        .into_iter()
        .flatten()
        .collect()
}

/// Is `workdir` one of `shared_roots`?
///
/// The roots are a parameter rather than a call to
/// [`shared_agent_workdir_roots`] so the refusal can be *proved* against
/// directories a test owns. A test that passed the operator's real home here
/// would assert on whatever that home happens to contain — which is how this
/// module's own guard test went red on a machine where an earlier build had
/// already written `~/.agents/skills/brief/SKILL.md`. Nothing under test may
/// read or write a person's home.
fn is_shared_agent_workdir(workdir: &Path, shared_roots: &[PathBuf]) -> bool {
    let target = workdir
        .canonicalize()
        .unwrap_or_else(|_| workdir.to_path_buf());
    shared_roots
        .iter()
        .any(|shared| shared.canonicalize().unwrap_or_else(|_| shared.clone()) == target)
}

/// Write the pack skills of the persona this agent was created from into
/// `workdir` as `.agents/skills/<name>/SKILL.md`.
///
/// A no-op for an agent with no pack behind it (a hand-built agent, or one
/// whose team was not installed from a directory) — those have no skills to
/// write and no pack to read. `AgentDefinition::into_agent_record` sets
/// `persona_team_dir` and `persona_name_in_team` to `None` and `detach` clears
/// them, so a hand-built agent reaches this and returns an empty vec. The
/// crew-role installer is the one production writer that sets them
/// (`crew_roles.rs:778`), so a role agent it minted *does* have skills to
/// write here — which is why the shared-workdir refusal below stopped being
/// theoretical the moment that installer shipped (finding 68).
///
/// Refuses a `workdir` that is shared rather than per-seat. The plan's rule is
/// "into that seat's own workdir, never into a shared dir" (§4 lane 5A), and
/// the live call site passes [`super::default_agent_workdir`] — the shared
/// nest, or `$HOME` when the nest is absent or a symlink. Writing there would
/// let one persona's `brief/SKILL.md` overwrite another's, and in the `$HOME`
/// case would overwrite a person's own `~/.agents/skills/<name>/SKILL.md`
/// without warning.
pub(crate) fn materialize_persona_skills(
    record: &ManagedAgentRecord,
    workdir: &std::path::Path,
) -> Result<Vec<beekeeper_persona_pkg::skills::MaterializedSkill>, String> {
    materialize_persona_skills_outside(record, workdir, &shared_agent_workdir_roots())
}

/// [`materialize_persona_skills`], with the shared directories named.
///
/// The seam exists for the guard's test: the refusal is a property of "this
/// workdir is one of the shared roots", and a test proves that against roots
/// it created in a temp directory instead of against the operator's home.
pub(crate) fn materialize_persona_skills_outside(
    record: &ManagedAgentRecord,
    workdir: &std::path::Path,
    shared_roots: &[PathBuf],
) -> Result<Vec<beekeeper_persona_pkg::skills::MaterializedSkill>, String> {
    let (Some(pack_dir), Some(persona_name)) = (
        record.persona_team_dir.as_deref(),
        record.persona_name_in_team.as_deref(),
    ) else {
        return Ok(Vec::new());
    };
    if is_shared_agent_workdir(workdir, shared_roots) {
        return Err(format!(
            "refusing to write persona {persona_name:?} skills into {} — that directory is \
             shared by every agent on this computer (and may be your own home directory). A \
             pack's skills belong in one seat's own working directory.",
            workdir.display()
        ));
    }
    let persona = beekeeper_persona_pkg::resolve::resolve_persona_by_name(pack_dir, persona_name)
        .map_err(|error| {
        format!(
            "cannot read persona {persona_name:?} in pack {}: {error}",
            pack_dir.display()
        )
    })?;
    beekeeper_persona_pkg::skills::materialize_skills(&persona, workdir).map_err(|error| {
        format!(
            "cannot materialize skills for persona {persona_name:?} into {}: {error}",
            workdir.display()
        )
    })
}

#[cfg(test)]
#[path = "skill_materialization_tests.rs"]
mod skill_materialization_tests;
