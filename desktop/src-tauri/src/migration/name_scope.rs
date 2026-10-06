//! Agent names became unique per project; strip the suffix this computer
//! minted only because they used to be unique per computer (ledger 246).
//!
//! Until 2026-09-22 `mint_agent_name` scanned every managed agent on the
//! machine, so the second project to install a `builder` role was given
//! "Builder 2" and the third "Builder 3". The numbers record nothing but the
//! order in which projects happened to be created on one laptop. Andy's
//! machine had grown `Architect 4`, `Builder 5`, `Project Setup 2`.
//!
//! **This migration is deliberately timid.** A number in a name is sometimes
//! the operator's own arrangement, and there is no way to ask them at boot. So
//! it strips one only when five independent facts all say the installer put it
//! there, and it says out loud which records it declined to touch and why. A
//! migration that renames something a person chose is worse than one that
//! leaves a suffix standing.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::managed_agents::ManagedAgentRecord;

use super::team_suffix::{definition_hashes, repin_current_instances};

/// The prefix `default_agents::project_team_name` writes. A team whose name
/// starts with this is one project's team; anything else is not, and its
/// numbers are left alone.
const PROJECT_TEAM_PREFIX: &str = "Project team ";

/// The installer's own definition-id prefix (`crew_roles::CREW_ROLE_ID_PREFIX`).
/// No other writer produces it.
const CREW_ROLE_ID_PREFIX: &str = "crew-role:";

/// One record this migration looked at and did not rename, with the reason.
///
/// Carried out of the core rather than only logged, because a refusal is the
/// half of this migration a person is most likely to need explained: their
/// `Architect 2` is still called `Architect 2` and nothing else on screen says
/// why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KeptSuffix {
    pub(crate) pubkey: String,
    pub(crate) name: String,
    pub(crate) reason: String,
}

/// What one run did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct NameScopeOutcome {
    /// `(pubkey, old name, new name)` for every record renamed.
    pub(crate) renamed: Vec<(String, String, String)>,
    /// Records that carry a suffix this run deliberately left in place.
    pub(crate) kept: Vec<KeptSuffix>,
}

/// Strip installer-minted name suffixes now that names are scoped per project.
///
/// Ordering (see `run_boot_migrations`): AFTER `fold_personas_into_agent_store`
/// so the definition rows renamed in step are already in the unified store, and
/// after `strip_baked_team_instructions` / `refresh_builtin_agent_avatars` so
/// their own content-hash repins have settled and the hashes captured here are
/// the final pre-rename ones. BEFORE `backfill_standalone_agents`, so a
/// manufactured definition never snapshots a name this is about to change.
pub fn scope_agent_names_to_projects(app: &tauri::AppHandle) {
    let Ok(base_dir) = crate::managed_agents::managed_agents_base_dir(app) else {
        return;
    };
    match scope_agent_names_to_projects_in_dir(&base_dir) {
        Ok(outcome) => {
            for (_, old, new) in &outcome.renamed {
                eprintln!("beekeeper-desktop: name-scope: renamed {old:?} to {new:?}");
            }
            for kept in &outcome.kept {
                eprintln!(
                    "beekeeper-desktop: name-scope: kept {:?} as it is — {}",
                    kept.name, kept.reason
                );
            }
        }
        Err(e) => eprintln!("beekeeper-desktop: name-scope: {e}"),
    }
}

/// Split `"Builder 4"` into `("Builder", 4)`.
///
/// Refuses a leading zero, refuses `n < 2` (the installer starts at 2), and
/// refuses a base that itself ends in space-plus-digits — `"Lead 2 3"` is not a
/// shape this installer produces, so it is not one to unpick.
fn split_minted_suffix(name: &str) -> Option<(&str, u32)> {
    let (base, digits) = name.rsplit_once(' ')?;
    if digits.is_empty() || digits.starts_with('0') || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n: u32 = digits.parse().ok()?;
    if n < 2 {
        return None;
    }
    let base = base.trim_end();
    if base.is_empty() {
        return None;
    }
    if split_minted_suffix(base).is_some() {
        return None;
    }
    Some((base, n))
}

fn fold(value: &str) -> String {
    value.trim().to_lowercase()
}

/// The namespace key a record's name is unique inside, as
/// `crew_roles_project::record_name_scope` computes it — `None` is the
/// no-project bucket.
fn scope_key(record: &ManagedAgentRecord) -> Option<String> {
    record
        .project_ref
        .as_deref()
        .and_then(crate::managed_agents::project_agent_association::normalize_project_ref)
}

/// Core logic, decoupled from the Tauri `AppHandle` for testing.
///
/// `base_dir` is the managed-agents base directory (`<AppDataDir>/agents/`).
pub(super) fn scope_agent_names_to_projects_in_dir(
    base_dir: &Path,
) -> Result<NameScopeOutcome, String> {
    let agents_path = base_dir.join("managed-agents.json");
    if !agents_path.exists() {
        return Ok(NameScopeOutcome::default());
    }
    let content = std::fs::read_to_string(&agents_path)
        .map_err(|e| format!("failed to read managed-agents.json: {e}"))?;
    let mut all: Vec<ManagedAgentRecord> = serde_json::from_str(&content)
        .map_err(|e| format!("failed to parse managed-agents.json: {e}"))?;

    // Which team ids are a project's. A team this computer cannot read is not
    // a project team: the refusal side is the safe side.
    let project_team_ids: HashSet<String> =
        crate::managed_agents::load_teams_readonly(&base_dir.join("teams.json"))
            .unwrap_or_default()
            .into_iter()
            .filter(|team| team.name.starts_with(PROJECT_TEAM_PREFIX))
            .map(|team| team.id)
            .collect();

    // `slug -> definition index`, for renaming a definition in step with the
    // instance that links it. Never the other way round: a definition's own
    // `source_team` has drifted from its instance's `team_id` on real stores,
    // so reading it would misfile seven legacy rows on this machine alone.
    let definition_at: HashMap<String, usize> = all
        .iter()
        .enumerate()
        .filter(|(_, record)| record.pubkey.is_empty())
        .filter_map(|(index, record)| record.slug.clone().map(|slug| (slug, index)))
        .collect();

    // Every name already standing, by namespace, plus the globally reserved
    // ones. Seeded from every instance — including the ones this run will not
    // touch — so a strip can never land on a name that is already in use.
    let mut taken: HashMap<Option<String>, HashSet<String>> = HashMap::new();
    let mut reserved: HashSet<String> = HashSet::new();
    for record in all.iter().filter(|record| !record.pubkey.is_empty()) {
        if record.reserves_name_globally {
            reserved.insert(fold(&record.name));
        }
        taken
            .entry(scope_key(record))
            .or_default()
            .insert(fold(&record.name));
    }

    // Deterministic order, so a re-run on the same store makes the same calls.
    let mut order: Vec<usize> = (0..all.len())
        .filter(|&index| !all[index].pubkey.is_empty())
        .collect();
    order.sort_by(|&left, &right| {
        (
            scope_key(&all[left]),
            &all[left].created_at,
            &all[left].pubkey,
        )
            .cmp(&(
                scope_key(&all[right]),
                &all[right].created_at,
                &all[right].pubkey,
            ))
    });

    let mut outcome = NameScopeOutcome::default();
    let mut planned: Vec<(usize, String)> = Vec::new();
    for index in order {
        let record = &all[index];
        let Some((base, n)) = split_minted_suffix(&record.name) else {
            continue;
        };
        let base = base.to_string();

        // (1) The installer is the only writer of `home_role`, and it is never
        // guessed from a name. A hand-made agent has none.
        let Some(home_role) = record.home_role.as_deref().filter(|r| !r.is_empty()) else {
            continue;
        };
        // (2) The instance links a definition this installer minted.
        let Some(definition_index) = record
            .persona_id
            .as_deref()
            .filter(|slug| slug.starts_with(CREW_ROLE_ID_PREFIX))
            .and_then(|slug| definition_at.get(slug))
            .copied()
        else {
            continue;
        };
        // (3) THE DISCRIMINATOR. The installer sets the definition's name and
        // `display_name` to exactly the agent's name, in the same statement.
        // The manual rename path writes `record.name` and nothing else. So
        // this equality holds if and only if the last write to this name was
        // the installer's — which is what makes the suffix *minted* rather
        // than *chosen*.
        let definition = &all[definition_index];
        if definition.name != record.name
            || definition.display_name.as_deref() != Some(record.name.as_str())
        {
            continue;
        }
        // (4) The record belongs to a project's team. The machine-wide
        // `Team roles` teams keep their numbers: there is no project to be the
        // namespace, and on a real store two of them hold an `Architect` and an
        // `Architect 2` that are different identities.
        if !record
            .team_id
            .as_deref()
            .is_some_and(|team| project_team_ids.contains(team))
        {
            outcome.kept.push(KeptSuffix {
                pubkey: record.pubkey.clone(),
                name: record.name.clone(),
                reason: "it belongs to no project's team, so there is no namespace to make \
                         the plain name unambiguous in"
                    .to_string(),
            });
            continue;
        }
        // (5) The suffix has to be explainable by a collision that actually
        // exists here. A lone `Lead 2` with no `Lead` beside it was not one.
        let ladder = all.iter().any(|other| {
            other.pubkey != record.pubkey
                && !other.pubkey.is_empty()
                && other.home_role.as_deref() == Some(home_role)
                && other
                    .persona_id
                    .as_deref()
                    .is_some_and(|slug| slug.starts_with(CREW_ROLE_ID_PREFIX))
                && (fold(&other.name) == fold(&base)
                    || (2..n).any(|m| fold(&other.name) == fold(&format!("{base} {m}"))))
        });
        if !ladder {
            outcome.kept.push(KeptSuffix {
                pubkey: record.pubkey.clone(),
                name: record.name.clone(),
                reason: format!(
                    "no other {home_role} on this computer is called {base:?}, so the number \
                     was not this installer working around a collision"
                ),
            });
            continue;
        }

        // The strip must land somewhere free. If it would not, leave the
        // record exactly as it is: never renumber, never pick a different
        // base, never move the other record out of the way.
        let scope = scope_key(record);
        let slot = taken.entry(scope.clone()).or_default();
        if reserved.contains(&fold(&base)) || slot.contains(&fold(&base)) {
            outcome.kept.push(KeptSuffix {
                pubkey: record.pubkey.clone(),
                name: record.name.clone(),
                reason: format!("{base:?} is already taken in the namespace it would move into"),
            });
            continue;
        }
        slot.remove(&fold(&record.name));
        slot.insert(fold(&base));
        planned.push((index, base));
    }

    if planned.is_empty() {
        return Ok(outcome);
    }

    // Definition hashes BEFORE the rename. `display_name` is part of
    // `persona_event_content`, so renaming a definition changes its
    // `persona_content_hash` — the drift basis every linked instance's pinned
    // `persona_source_version` is compared against. Without the repin below,
    // every agent this migration renames lights up "out of date" in the Agents
    // menu for a change nobody made.
    let pre_rename_hashes = definition_hashes(&all);

    let now = crate::util::now_iso();
    for (index, base) in planned {
        let old = all[index].name.clone();
        let pubkey = all[index].pubkey.clone();
        let definition_index = all[index]
            .persona_id
            .as_deref()
            .and_then(|slug| definition_at.get(slug))
            .copied();
        all[index].name.clone_from(&base);
        all[index].updated_at.clone_from(&now);
        if let Some(definition_index) = definition_index {
            // The slug is NEVER touched: it is the kind:30175 `d` tag and every
            // instance's `persona_id` foreign key.
            all[definition_index].name.clone_from(&base);
            all[definition_index].display_name = Some(base.clone());
            all[definition_index].updated_at.clone_from(&now);
        }
        outcome.renamed.push((pubkey, old, base));
    }

    repin_current_instances(&mut all, &pre_rename_hashes);

    // The queue is written BEFORE the store, on purpose. The drain executes an
    // entry only while `expected_name == record.name`, so a crash between the
    // two writes leaves an inert entry that starts working on the boot that
    // finishes the rename. The reverse order would leave renamed records with
    // no queued kind:0 publish, permanently — and a name only this computer
    // can see is a name nobody else can see.
    let queue: Vec<(String, String)> = outcome
        .renamed
        .iter()
        .map(|(pubkey, _, new)| (pubkey.clone(), new.clone()))
        .collect();
    super::profile_reconcile::persist_profile_reconcile_queue(&agents_path, &queue)?;

    let bak_path =
        crate::util::resolved_backup_path(&agents_path, "managed-agents.json.pre-name-scope.bak");
    crate::util::create_restricted_backup_once(&bak_path, content.as_bytes())
        .map_err(|e| format!("failed to write pre-rename backup: {e}"))?;

    let payload = serde_json::to_vec_pretty(&all)
        .map_err(|e| format!("failed to serialize managed-agents.json: {e}"))?;
    crate::managed_agents::atomic_write_json_restricted(&agents_path, &payload)?;
    Ok(outcome)
}

#[cfg(test)]
#[path = "name_scope_tests.rs"]
mod tests;
