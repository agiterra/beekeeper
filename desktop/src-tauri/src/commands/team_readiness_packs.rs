//! Where a project's role packs come from, and what that means for a launch.
//!
//! Split out of `team_readiness.rs` for the repository's 1,000-line ceiling,
//! and it is a clean seam: everything here answers one question — *which rung
//! of the staging ladder would seat this launch's roles, and can it?*
//!
//! # The finding this module exists for
//!
//! Ledger 135(c). With "Use roles" ticked the founding form blocked on
//! `ROLE_PACKS_MISSING` ("Restore personas/roles") and `REGISTRY_UNREADABLE`
//! inside a checkout, while the seated create staged every pack from the
//! project's kind:30624 pack source on the relay and never opened that
//! directory
//! (`desktop/src/features/coding-sessions/lib/codingSessionSeatedCreate.ts:209`,
//! `desktop/src-tauri/src/managed_agents/actor_seats.rs:599-617`). The check
//! was enforcing a layout the product no longer uses, and the operator's way
//! out was to untick the box. A control that lies about what it enforces is a
//! bug of crash severity.
//!
//! So the project rung is asked first, and it is answered by the staging code
//! itself ([`list_project_role_packs_blocking`]) rather than by a second
//! opinion about where packs live. `personas/roles` inside the checkout is
//! consulted only for a project that names no source.

use std::collections::HashSet;
use std::path::Path;

use sha2::{Digest as _, Sha256};
use tauri::AppHandle;

use super::auth_facts::append_agent_auth_facts;
use super::host::ReadinessHost;
use super::{
    identity_projection, Gathered, TeamReadinessFact, TeamReadinessFactState,
    TeamReadinessKeyState, TeamReadinessRolePack,
};
use crate::managed_agents::crew_roles::{
    project_role_packs_dir, role_pack_source_version, scan_role_packs,
};
use crate::managed_agents::role_packs_view::{
    fetch_project_pack_source, list_project_role_packs_blocking,
};
use crate::managed_agents::ManagedAgentReadinessMetadata;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SelectedRolePackState {
    Current,
    Dirty,
    SourceUnknown,
    WrongProject,
    Missing,
}

pub(super) fn selected_role_pack_state(
    metadata: &[ManagedAgentReadinessMetadata],
    pack: &crate::managed_agents::crew_roles::DiscoveredRolePack,
) -> SelectedRolePackState {
    if let Some(row) = metadata.iter().find(|row| {
        row.persona_team_dir.as_deref() == Some(pack.dir.as_path())
            && row.persona_name_in_team.as_deref() == Some(pack.persona_name.as_str())
    }) {
        return match row.persona_source_version.as_deref() {
            Some(source) if source == role_pack_source_version(pack, &row.name) => {
                SelectedRolePackState::Current
            }
            Some(_) => SelectedRolePackState::Dirty,
            None => SelectedRolePackState::SourceUnknown,
        };
    }
    if metadata.iter().any(|row| {
        row.home_role.as_deref() == Some(pack.role.as_str())
            && (row.persona_team_dir.as_deref() != Some(pack.dir.as_path())
                || row.persona_name_in_team.as_deref() != Some(pack.persona_name.as_str()))
    }) {
        SelectedRolePackState::WrongProject
    } else {
        SelectedRolePackState::Missing
    }
}

/// Where this project's role packs actually come from, read the way a hire
/// reads it.
///
/// # Why readiness asks the relay at all
///
/// Ledger 135(c). With "Use roles" ticked the founding form blocked on
/// `ROLE_PACKS_MISSING` ("Restore personas/roles") and `REGISTRY_UNREADABLE`
/// inside a checkout, while the seated create staged every pack from the
/// project's kind:30624 pack source on the relay and never looked at that
/// directory at all
/// (`desktop/src/features/coding-sessions/lib/codingSessionSeatedCreate.ts:209`,
/// `desktop/src-tauri/src/managed_agents/actor_seats.rs:599-617`). A check
/// enforcing a layout the product no longer uses is a control that lies about
/// what it enforces, which is a bug of crash severity here.
///
/// So the project rung is asked first and answered by the staging code itself
/// ([`list_project_role_packs_blocking`]); `personas/roles` inside the
/// checkout is consulted only for a project that names no source.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) enum ProjectPackSourceProbe {
    /// Nobody asked — a direct `gather` with no relay behind it. Falls back to
    /// the in-checkout rung, and says nothing about a source either way.
    #[default]
    NotProbed,
    /// The project publishes no kind:30624, so staging uses the local rungs
    /// and `personas/roles` in the checkout is the answer.
    Absent,
    /// The project's packs repository was synced and holds these roles.
    Available {
        /// The `30617:<owner>:<id>` coordinate the packs came from.
        repo: String,
        /// The commit actually staged from, when a row could vouch for one.
        sha: Option<String>,
        /// The role slugs the repository holds, sorted.
        roles: Vec<String>,
    },
    /// The project names a source this computer could not stage from. Carries
    /// the sentence a hire would be refused with, verbatim.
    Unavailable {
        /// Why, exactly as staging says it.
        reason: String,
    },
}

/// Read the project's pack source and the roles it carries.
///
/// Read-only in the same sense [`list_project_role_packs`] is: the newest
/// kind:30624 comes off the relay and the packs repository is synced into this
/// computer's own cache. Nothing is staged, nothing is installed, nothing is
/// published.
///
/// [`list_project_role_packs`]: crate::commands::role_packs::list_project_role_packs
pub(super) async fn probe_project_pack_source(
    app: &AppHandle,
    state: &tauri::State<'_, crate::app_state::AppState>,
    project_ref: &str,
) -> ProjectPackSourceProbe {
    let source = match fetch_project_pack_source(state, project_ref).await {
        Ok(None) => return ProjectPackSourceProbe::Absent,
        Ok(Some(source)) => source,
        Err(reason) => return ProjectPackSourceProbe::Unavailable { reason },
    };
    let repo = source.repo.clone();
    let handle = app.clone();
    let owned_ref = project_ref.to_owned();
    let rows = tauri::async_runtime::spawn_blocking(move || {
        list_project_role_packs_blocking(&handle, Some(&owned_ref), Some(source))
    })
    .await;
    let rows = match rows {
        Ok(Ok(rows)) => rows,
        // The refusal the walk itself produced, or the task's own failure.
        Ok(Err(reason)) => return ProjectPackSourceProbe::Unavailable { reason },
        Err(error) => {
            return ProjectPackSourceProbe::Unavailable {
                reason: format!("the project's role packs could not be read: {error}"),
            }
        }
    };
    // With a project source present, a row without a refusal is a row the
    // project's own repository answered for — that is exactly the rule
    // `walk_role_pack_ladder` applies, so this cannot drift from staging.
    let mut roles: Vec<String> = rows
        .iter()
        .filter(|row| row.refusal.is_none())
        .map(|row| row.role.clone())
        .collect();
    if roles.is_empty() {
        return ProjectPackSourceProbe::Unavailable {
            reason: rows
                .iter()
                .find_map(|row| row.refusal.clone())
                .unwrap_or_else(|| format!("the packs repository {repo} holds no role packs")),
        };
    }
    roles.sort();
    roles.dedup();
    ProjectPackSourceProbe::Available {
        repo,
        sha: rows
            .iter()
            .filter(|row| row.refusal.is_none())
            .find_map(|row| row.pack_ref.as_ref().map(|pack| pack.sha.clone())),
        roles,
    }
}

/// The roles this project's packs repository carries, as one fact.
///
/// Reports the repository, the commit and the roles rather than a bare
/// "ready": a person whose founding form just stopped blocking needs to be
/// able to check that the roles named are the roles they meant.
fn collect_team_from_project_source(
    repo: &str,
    sha: Option<&str>,
    roles: &[String],
    gathered: &mut Gathered,
) {
    gathered.team.available_roles = roles.to_vec();
    gathered.team.packs_revision = sha.map(str::to_owned);
    let commit = match sha {
        Some(sha) if sha.len() >= 8 => format!(" at {}", &sha[..8]),
        Some(sha) => format!(" at {sha}"),
        None => String::new(),
    };
    gathered.facts.push(TeamReadinessFact::local(
        "team",
        "ROLE_PACKS_FROM_PROJECT_SOURCE",
        TeamReadinessFactState::Ready,
        format!(
            "Role packs come from this project's packs repository {repo}{commit}, which carries: {}",
            roles.join(", ")
        ),
    ));
    for role in &gathered.team.selected_roles {
        if roles.iter().any(|carried| carried == role) {
            continue;
        }
        gathered.facts.push(TeamReadinessFact::blocked(
            "team",
            "SELECTED_ROLE_UNAVAILABLE",
            format!("This session asks for the role {role}, which {repo} does not carry"),
            "Pick a role the packs repository carries, or add that role to the repository.",
        ));
    }
}

pub(super) fn collect_team(
    host: &impl ReadinessHost,
    checkout: Option<&Path>,
    packs: &ProjectPackSourceProbe,
    gathered: &mut Gathered,
) {
    let metadata = match host.agents(gathered.owner_pubkey.as_deref()) {
        Ok(rows) => rows,
        Err(error) => {
            let mut fact = TeamReadinessFact::local(
                "team",
                "AGENT_METADATA_UNREADABLE",
                if gathered.team.selected_roles.is_empty() {
                    TeamReadinessFactState::Limited
                } else {
                    TeamReadinessFactState::Unknown
                },
                error,
            );
            fact.remedy = Some("Repair the managed-agent metadata store.".into());
            gathered.facts.push(fact);
            Vec::new()
        }
    };
    append_agent_auth_facts(&metadata, gathered);
    let live = host.live_agent_pubkeys().unwrap_or_default();
    gathered.team.identities = metadata
        .iter()
        .map(|row| identity_projection(row, &live))
        .collect();
    // The project rung first, and alone when it answers: a hire that finds a
    // kind:30624 stages from it and consults no other rung, so a readiness
    // check that went on measuring the checkout would be enforcing a layout
    // nothing reads (ledger 135(c)).
    match packs {
        ProjectPackSourceProbe::Available { repo, sha, roles } => {
            collect_team_from_project_source(repo, sha.as_deref(), roles, gathered);
            return;
        }
        ProjectPackSourceProbe::Unavailable { reason } => {
            let mut fact = TeamReadinessFact::local(
                "team",
                "ROLE_PACKS_SOURCE_UNAVAILABLE",
                if gathered.team.selected_roles.is_empty() {
                    TeamReadinessFactState::Limited
                } else {
                    TeamReadinessFactState::Blocked
                },
                format!(
                    "This project's packs repository could not be read, so no seat \
                     could be staged: {reason}"
                ),
            );
            fact.remedy = Some(
                "Check the packs repository and the commit it names in Project settings → Roles."
                    .into(),
            );
            gathered.facts.push(fact);
            return;
        }
        ProjectPackSourceProbe::Absent | ProjectPackSourceProbe::NotProbed => {}
    }
    let Some(checkout) = checkout else {
        return;
    };
    let directory = project_role_packs_dir(checkout);
    if !directory.is_dir() {
        let mut fact = TeamReadinessFact::local(
            "team",
            "ROLE_PACKS_MISSING",
            if gathered.team.selected_roles.is_empty() {
                TeamReadinessFactState::Limited
            } else {
                TeamReadinessFactState::Blocked
            },
            "This project names no packs repository, and its checkout has no \
             personas/roles folder, so there is nowhere to stage a seat's role from",
        );
        fact.remedy = Some(
            "Set a packs repository for this project, or add a personas/roles folder \
             to the checkout."
                .into(),
        );
        gathered.facts.push(fact);
        return;
    }
    let scan = match scan_role_packs(&directory) {
        Ok(scan) => scan,
        Err(error) => {
            let mut fact = TeamReadinessFact::local(
                "team",
                "ROLE_PACKS_UNREADABLE",
                if gathered.team.selected_roles.is_empty() {
                    TeamReadinessFactState::Limited
                } else {
                    TeamReadinessFactState::Unknown
                },
                error,
            );
            fact.remedy = Some("Repair the role-pack directory.".into());
            gathered.facts.push(fact);
            return;
        }
    };
    let mut hasher = Sha256::new();
    let mut wrong_project_roles = HashSet::new();
    for pack in scan.packs {
        hasher.update(pack.role.as_bytes());
        hasher.update([0]);
        hasher.update(pack.persona_name.as_bytes());
        hasher.update([0]);
        hasher.update(pack.display_name.as_bytes());
        hasher.update([0]);
        hasher.update(pack.system_prompt.as_bytes());
        hasher.update([0]);
        for value in [
            pack.runtime.as_deref(),
            pack.model.as_deref(),
            pack.provider.as_deref(),
        ] {
            hasher.update(value.unwrap_or("").as_bytes());
            hasher.update([0]);
        }
        hasher.update(pack.avatar_url.as_deref().unwrap_or("").as_bytes());
        hasher.update([0]);
        let installed = metadata.iter().find(|row| {
            row.persona_team_dir.as_deref() == Some(pack.dir.as_path())
                && row.persona_name_in_team.as_deref() == Some(pack.persona_name.as_str())
        });
        if gathered.team.selected_roles.contains(&pack.role) {
            match selected_role_pack_state(&metadata, &pack) {
                SelectedRolePackState::Current | SelectedRolePackState::Missing => {}
                SelectedRolePackState::Dirty => gathered.facts.push(TeamReadinessFact::blocked(
                    "team",
                    "SELECTED_ROLE_PACK_DIRTY",
                    format!(
                        "Selected role {} differs from its installed source",
                        pack.role
                    ),
                    "Use Prepare to refresh the selected role.",
                )),
                SelectedRolePackState::SourceUnknown => {
                    gathered.facts.push(TeamReadinessFact::unknown(
                        "team",
                        "SELECTED_ROLE_PACK_STATE_UNKNOWN",
                        format!("Selected role {} has no source digest", pack.role),
                        "Use Prepare to refresh the selected role.",
                    ))
                }
                SelectedRolePackState::WrongProject => {
                    wrong_project_roles.insert(pack.role.clone());
                }
            }
        }
        gathered.team.packs.push(TeamReadinessRolePack {
            role: pack.role.clone(),
            persona_name: pack.persona_name,
            path: pack.dir.display().to_string(),
            installed_pubkey: installed.map(|row| row.pubkey.clone()),
        });
        gathered.team.available_roles.push(pack.role);
    }
    gathered.team.available_roles.sort();
    gathered.team.available_roles.dedup();
    if gathered.team.packs.is_empty() {
        let mut fact = TeamReadinessFact::local(
            "team",
            "ROLE_PACKS_EMPTY",
            if gathered.team.selected_roles.is_empty() {
                TeamReadinessFactState::Limited
            } else {
                TeamReadinessFactState::Blocked
            },
            "The role-pack directory contains no usable roles",
        );
        fact.remedy = Some("Add at least one role-bearing pack.".into());
        gathered.facts.push(fact);
    } else {
        gathered.team.packs_digest = Some(hex::encode(hasher.finalize()));
        gathered.team.packs_revision = gathered.source.checkout_commit.clone();
        gathered.facts.push(TeamReadinessFact::local(
            "team",
            "ROLE_PACKS_READY",
            TeamReadinessFactState::Ready,
            "Project role packs are readable",
        ));
    }
    for role in &gathered.team.selected_roles {
        let Some(pack) = gathered.team.packs.iter().find(|pack| &pack.role == role) else {
            gathered.facts.push(TeamReadinessFact::blocked(
                "team",
                "SELECTED_ROLE_UNAVAILABLE",
                format!("Selected role {role} has no project pack"),
                "Choose an available role.",
            ));
            continue;
        };
        let Some(pubkey) = pack.installed_pubkey.as_deref() else {
            if wrong_project_roles.contains(role) {
                gathered.facts.push(TeamReadinessFact::blocked(
                    "team",
                    "SELECTED_ROLE_WRONG_PROJECT",
                    format!("Selected role {role} is installed from another project"),
                    "Use Prepare to install this project's role pack.",
                ));
                continue;
            }
            gathered.facts.push(TeamReadinessFact::blocked(
                "team",
                "SELECTED_ROLE_NOT_INSTALLED",
                format!("Selected role {role} has no installed identity"),
                "Use Prepare to install the selected role.",
            ));
            continue;
        };
        if gathered
            .team
            .identities
            .iter()
            .find(|row| row.pubkey == pubkey)
            .is_none_or(|row| row.key_state != TeamReadinessKeyState::LiveProcess)
        {
            gathered.facts.push(TeamReadinessFact::unknown(
                "team",
                "SELECTED_ROLE_KEY_UNVERIFIED",
                format!("Selected role {role} key accessibility is unverified"),
                "Use Prepare to start and re-check the selected identity.",
            ));
        }
    }
}

#[cfg(test)]
#[path = "team_readiness_packs_tests.rs"]
mod tests;
