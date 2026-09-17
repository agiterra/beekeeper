//! A managed agent's durable, local project association
//! (`docs/PROJECT_AGENT_HIRING_IMPL.md`, *Association: what records it*).
//!
//! An agent belongs to at most one project, recorded as the normalized
//! `30621:<owner-hex>:<dtag>` coordinate on [`ManagedAgentRecord::project_ref`].
//! Three writers set it, all through this module:
//!
//! - project setup installation, for each installed role's agent
//!   ([`associate_installation`]);
//! - an idempotent backfill from this owner's setup journals
//!   ([`backfill_project_agents_from_journals`]);
//! - the explicit [`crate::commands::associate_managed_agent_with_project`]
//!   command ([`decide_association`]).
//!
//! None of them ever moves an agent from one project to another, creates or
//! deletes an agent, or changes a role, relay ACL or channel membership. The
//! association travels to other computers as a digest on the agent's
//! kind:30177 (`agent_events::agent_event_content`).

use tauri::{AppHandle, Manager};

use super::ManagedAgentRecord;
use crate::app_state::AppState;

/// The sentence a new seat is refused with when the agent is not the
/// session project's agent.
pub(crate) const SEAT_NOT_PROJECT_AGENT: &str = "This agent does not belong to this session's project, so it cannot take a new seat here. Borrowing agents from other projects is not supported.";

/// Refusal for an explicit association of an agent that has no primary role.
pub(crate) const ASSOCIATION_NEEDS_PRIMARY_ROLE: &str =
    "An agent without a primary role cannot be a project agent.";

/// Refusal for a coordinate that is not `30621:<owner-hex>:<dtag>`.
pub(crate) const ASSOCIATION_MALFORMED_PROJECT: &str =
    "Expected a project coordinate 30621:<owner>:<slug>.";

/// The sentence a new seat in a session with no project is refused with when
/// the agent belongs to a project.
pub(crate) const SEAT_PROJECT_AGENT_OUTSIDE_PROJECT: &str = "This agent belongs to a project, so it cannot take a new seat in a session outside that project. Borrowing agents from other projects is not supported.";

/// Refusal code for a new seat whose role is not the agent's primary role.
/// The sentence itself is [`seat_role_not_primary`].
pub(crate) const SEAT_ROLE_NOT_PRIMARY: &str = "SEAT_ROLE_NOT_PRIMARY";

/// The normalized project coordinate, or `None` when `value` is not one.
///
/// Trims ASCII whitespace only, exactly as `project_agent_digest` does, so a
/// coordinate this computer records and the digest other readers match can
/// never disagree about surrounding U+0085 or U+FEFF.
pub(crate) fn normalize_project_ref(value: &str) -> Option<String> {
    buzz_core_pkg::kind::normalize_project_coordinate(
        buzz_core_pkg::project_agent_association::trim_ascii_whitespace(value),
    )
}

/// The [`SEAT_ROLE_NOT_PRIMARY`] sentence for `name`, whose primary role is
/// `home_role`, asked to sit as `role`.
pub(crate) fn seat_role_not_primary(name: &str, home_role: &str, role: &str) -> String {
    format!(
        "A new seat takes the agent's primary role. {name} is a {home_role}, so it cannot be seated as {role}; hire or pick a {role} agent instead."
    )
}

/// Whether `record` may take the seat a stage or preview names, or the
/// refusal sentence.
///
/// Without `new_selection` (absent or `false`) this is exactly
/// [`seat_project_refusal`]: resume and restage keep working, and a caller
/// that passes only `require_project_ref` keeps its check.
///
/// A **new selection** (`Some(true)`) additionally requires:
/// - association: a named project must be the agent's project; a session with
///   no project (absent or blank `require_project_ref`) never takes an agent
///   that belongs to any project ([`SEAT_PROJECT_AGENT_OUTSIDE_PROJECT`]). A
///   recorded association this computer cannot read still counts as one;
/// - role: an agent with a primary role (`home_role`) sits only in that role
///   ([`SEAT_ROLE_NOT_PRIMARY`]). An agent without one, or a stage that names
///   no role, is unaffected.
pub(crate) fn new_seat_refusal(
    record: &ManagedAgentRecord,
    role: Option<&str>,
    require_project_ref: Option<&str>,
    new_selection: Option<bool>,
) -> Option<String> {
    if new_selection != Some(true) {
        return seat_project_refusal(record, require_project_ref).map(str::to_owned);
    }
    let required = require_project_ref
        .map(trim_ascii)
        .filter(|v| !v.is_empty());
    if required.is_some() {
        if let Some(refusal) = seat_project_refusal(record, required) {
            return Some(refusal.to_owned());
        }
    } else if record
        .project_ref
        .as_deref()
        .is_some_and(|project| !trim_ascii(project).is_empty())
    {
        return Some(SEAT_PROJECT_AGENT_OUTSIDE_PROJECT.to_owned());
    }
    let home_role = record.home_role.as_deref().map(str::trim)?;
    let role = role.map(str::trim).filter(|role| !role.is_empty())?;
    (!home_role.is_empty() && home_role != role).then(|| {
        tracing::info!(
            code = SEAT_ROLE_NOT_PRIMARY,
            agent = %record.pubkey,
            home_role,
            role,
            "refusing a new seat outside the agent's primary role"
        );
        seat_role_not_primary(&record.name, home_role, role)
    })
}

fn trim_ascii(value: &str) -> &str {
    buzz_core_pkg::project_agent_association::trim_ascii_whitespace(value)
}

/// Whether `record` may take a **new** seat in the project `require_project_ref`
/// names. `None` when it may; otherwise the refusal sentence.
///
/// An absent or blank requirement imposes nothing: resume and restage of an
/// existing execution pass none, so historical executions stay resumable. A
/// requirement that is not a well-formed coordinate matches no agent.
pub(crate) fn seat_project_refusal(
    record: &ManagedAgentRecord,
    require_project_ref: Option<&str>,
) -> Option<&'static str> {
    let required = require_project_ref
        .map(trim_ascii)
        .filter(|value| !value.is_empty())?;
    let matches = normalize_project_ref(required).is_some_and(|required| {
        record
            .project_ref
            .as_deref()
            .and_then(normalize_project_ref)
            == Some(required)
    });
    (!matches).then_some(SEAT_NOT_PROJECT_AGENT)
}

/// The preview a refused new seat answers with: nothing staged, the refusal
/// sentence set, so the dialog can show it before anything is signed.
pub(crate) fn refused_seat_preview(
    role: Option<&str>,
    refusal: &str,
) -> super::actor_seats::SeatPackPreview {
    super::actor_seats::SeatPackPreview {
        pack_staged: false,
        origin: super::actor_seats::SeatPackOrigin::None,
        role: role
            .map(str::trim)
            .filter(|role| !role.is_empty())
            .map(str::to_owned),
        pack_dir: None,
        persona_id: None,
        pack_ref: None,
        refusal: Some(refusal.to_string()),
        reason: None,
        warnings: Vec::new(),
        compose_digest: None,
        source_kind: None,
        roles_visible: false,
    }
}

/// What an explicit association request resolves to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AssociationDecision {
    /// Already this project's agent: a no-op that returns the agent.
    AlreadyAssociated,
    /// Record this normalized coordinate.
    Associate(String),
}

/// Decide an explicit association of `record` with `project_ref`, refusing
/// in the contract's order: malformed coordinate, builtin, setup actor, no
/// primary role, already another project's agent.
pub(crate) fn decide_association(
    record: &ManagedAgentRecord,
    project_ref: &str,
) -> Result<AssociationDecision, String> {
    let project = normalize_project_ref(project_ref)
        .ok_or_else(|| ASSOCIATION_MALFORMED_PROJECT.to_string())?;
    if record.is_builtin {
        return Err(format!(
            "{} is a built-in agent and cannot be a project agent.",
            record.name
        ));
    }
    if super::project_team_setup::actor::restage::is_setup_actor(record) {
        return Err(format!(
            "{} is a project setup agent and cannot be a project agent.",
            record.name
        ));
    }
    if record
        .home_role
        .as_deref()
        .is_none_or(|role| role.trim().is_empty())
    {
        return Err(ASSOCIATION_NEEDS_PRIMARY_ROLE.to_string());
    }
    match record.project_ref.as_deref() {
        None => Ok(AssociationDecision::Associate(project)),
        // An unreadable recorded association is still a recorded one: it is
        // never silently replaced.
        Some(existing) if normalize_project_ref(existing).as_deref() == Some(&project) => {
            Ok(AssociationDecision::AlreadyAssociated)
        }
        Some(_) => Err(format!(
            "{} belongs to another project; borrowing is not supported.",
            record.name
        )),
    }
}

/// One installed role a setup journal (or a running installation) records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InstalledRoleClaim<'a> {
    pub project_ref: &'a str,
    pub role: &'a str,
    pub agent_pubkey: &'a str,
}

/// An installed role whose agent is already another project's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AssociationConflict {
    pub agent_pubkey: String,
    pub agent_name: String,
    pub role: String,
    pub existing: String,
    pub wanted: String,
}

/// What applying installed-role claims changed.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct AssociationOutcome {
    /// Pubkeys whose `project_ref` was set by this pass.
    pub associated: Vec<String>,
    /// Claims left untouched because the agent belongs to another project.
    pub conflicts: Vec<AssociationConflict>,
}

/// Apply installed-role claims to `agents` in place.
///
/// For each claim whose agent record exists, has no `project_ref`, and whose
/// `home_role` is the claimed role, record the claim's normalized project. A
/// missing record, a role mismatch, a malformed claim coordinate, a builtin
/// and a setup actor are skipped; an agent already associated with a
/// different project is reported as a conflict and left untouched. Never
/// creates or deletes a record. Idempotent: a second pass changes nothing.
pub(crate) fn apply_installed_role_claims<'a>(
    agents: &mut [ManagedAgentRecord],
    claims: impl IntoIterator<Item = InstalledRoleClaim<'a>>,
) -> AssociationOutcome {
    let mut outcome = AssociationOutcome::default();
    for claim in claims {
        let Some(project) = normalize_project_ref(claim.project_ref) else {
            tracing::warn!(
                project = claim.project_ref,
                "project agent backfill skips a malformed project coordinate"
            );
            continue;
        };
        let Some(record) = agents
            .iter_mut()
            .find(|record| record.pubkey == claim.agent_pubkey)
        else {
            continue;
        };
        if record.home_role.as_deref().map(str::trim) != Some(claim.role.trim())
            || record.is_builtin
            || super::project_team_setup::actor::restage::is_setup_actor(record)
        {
            continue;
        }
        match record.project_ref.as_deref() {
            None => {
                record.project_ref = Some(project);
                outcome.associated.push(record.pubkey.clone());
            }
            Some(existing) if normalize_project_ref(existing).as_deref() == Some(&project) => {}
            Some(existing) => outcome.conflicts.push(AssociationConflict {
                agent_pubkey: record.pubkey.clone(),
                agent_name: record.name.clone(),
                role: claim.role.to_string(),
                existing: existing.to_string(),
                wanted: project,
            }),
        }
    }
    outcome
}

/// Associate a running installation's role agents with its project, before
/// any store is saved. Refuses — so nothing is written — when an installed
/// role's agent already belongs to another project.
pub(crate) fn associate_installation(
    agents: &mut [ManagedAgentRecord],
    project_ref: &str,
    installed: &[super::crew_roles::InstalledCrewRole],
) -> Result<AssociationOutcome, String> {
    let outcome = apply_installed_role_claims(
        agents,
        installed.iter().map(|role| InstalledRoleClaim {
            project_ref,
            role: &role.role,
            agent_pubkey: &role.agent_pubkey,
        }),
    );
    match outcome.conflicts.first() {
        None => Ok(outcome),
        Some(conflict) => Err(format!(
            "The installed {} agent {} belongs to another project; this installation cannot claim it.",
            conflict.role, conflict.agent_name
        )),
    }
}

/// Queue a kind:30177 republish for each installed role's saved record. Call
/// inside the store lock, after `save_managed_agents`, never across an await.
pub(crate) fn retain_installed_agents(
    app: &AppHandle,
    state: &AppState,
    agents: &[ManagedAgentRecord],
    installed: &[super::crew_roles::InstalledCrewRole],
) {
    for record in agents.iter().filter(|record| {
        installed
            .iter()
            .any(|role| role.agent_pubkey == record.pubkey)
    }) {
        crate::commands::retain_managed_agent_pending(app, state, record);
    }
}

/// Apply every installed role recorded by `owner_hex`'s setup journals on
/// `relay` (canonical key) under `root` to `agents`, logging conflicts.
/// Filesystem reads only; the caller holds the store lock and saves.
pub(crate) fn backfill_agents_from_journal_root(
    root: &std::path::Path,
    owner_hex: &str,
    relay: &str,
    agents: &mut [ManagedAgentRecord],
) -> Result<AssociationOutcome, String> {
    let installations =
        super::project_team_setup::publication::installed_roles::list_installed_roles(
            root, owner_hex, relay,
        )
        .map_err(|error| error.message)?;
    let outcome = apply_installed_role_claims(
        agents,
        installations.iter().flat_map(|installation| {
            installation.roles.iter().map(|role| InstalledRoleClaim {
                project_ref: &installation.project_ref,
                role: &role.role,
                agent_pubkey: &role.agent_pubkey,
            })
        }),
    );
    for conflict in &outcome.conflicts {
        tracing::warn!(
            agent = %conflict.agent_pubkey,
            role = %conflict.role,
            existing = %conflict.existing,
            wanted = %conflict.wanted,
            "project agent backfill left an agent that belongs to another project untouched"
        );
    }
    Ok(outcome)
}

/// Backfill `project_ref` from this owner's setup journals on the active
/// relay, saving and queueing a republish only for agents it changed. Holds
/// the managed-agent store lock for the load, mutation and save. Returns how
/// many agents were associated.
pub(crate) fn backfill_project_agents_from_journals(
    app: &AppHandle,
    owner_hex: &str,
) -> Result<usize, String> {
    let state = app
        .try_state::<AppState>()
        .ok_or_else(|| "app state is unavailable".to_string())?;
    let relay = crate::session_provider::canonical_relay_key(
        &crate::relay::relay_ws_url_with_override(&state),
    );
    let root = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("project-team-setup");
    if !root.try_exists().map_err(|error| error.to_string())? {
        return Ok(0);
    }
    let _guard = state
        .managed_agents_store_lock
        .lock()
        .map_err(|error| error.to_string())?;
    let mut agents = super::load_managed_agents(app)?;
    let outcome = backfill_agents_from_journal_root(&root, owner_hex, &relay, &mut agents)?;
    if outcome.associated.is_empty() {
        return Ok(0);
    }
    super::save_managed_agents(app, &agents)?;
    for record in agents
        .iter()
        .filter(|record| outcome.associated.contains(&record.pubkey))
    {
        crate::commands::retain_managed_agent_pending(app, &state, record);
    }
    Ok(outcome.associated.len())
}

/// [`backfill_project_agents_from_journals`] as a best-effort step: logs the
/// outcome, never fails its caller.
pub(crate) fn backfill_project_agents_logged(app: &AppHandle, owner_hex: &str) {
    match backfill_project_agents_from_journals(app, owner_hex) {
        Ok(0) => {}
        Ok(count) => tracing::info!("project agent backfill associated {count} agents"),
        Err(error) => tracing::warn!("project agent backfill skipped: {error}"),
    }
}

#[cfg(test)]
#[path = "project_agent_association_tests.rs"]
mod tests;
