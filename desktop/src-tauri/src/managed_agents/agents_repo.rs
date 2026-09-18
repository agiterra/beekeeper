//! Creating a project's two repositories, from inside the app (spec § 4.11).
//!
//! Every project owns a code repository, `<slug>`, empty at creation, and an
//! agents repository, `<slug>-beekeeper-agents`, seeded with the team and
//! pinned as the project's kind:30624 source (`ref: refs/heads/main`,
//! `path: .`). [`project_agents_init`] is the whole sequence as one host
//! command, run right after the desktop publishes the kind:30621, and again
//! from **Finish setup** when any step did not land — it is idempotent: what
//! already exists under the viewer's key is reused, never re-announced,
//! re-seeded or re-pointed.
//!
//! # What it does, in order
//!
//! 1. **Preflight.** Both repository ids are read community-wide: ids are one
//!    namespace per community (`buzz-db`'s `ReserveOutcome::TakenByOther`),
//!    so an id already announced by *another* key refuses the whole command,
//!    naming the id and the owner, before anything is signed. One announced
//!    by the viewer is the existing repository, reused. A project that
//!    already has a pack source naming a *different* repository refuses too:
//!    re-pointing every seat is a deliberate `bee packs set-source`.
//! 2. **Announce** the code repository — the announcement only; the
//!    repository stays empty until someone pushes.
//! 3. **Announce** the agents repository.
//! 4. **Seed** it in this host's packs cache from this build's shipped role
//!    templates by reference (`buzz_persona::seed`), one commit authored as
//!    this host's identity, and **push** `refs/heads/main`. Skipped when the
//!    relay already holds a push record for it. A seed or push failure after
//!    this run's own announcement withdraws that announcement, as the packs
//!    flow does; the code repository's announcement stands, since an empty
//!    code repository is exactly what was promised.
//! 5. **Publish** the kind:30624 — only once the push landed.
//!
//! The result reports every wire fact and, when the sequence did not finish,
//! one sentence (`gap`) saying what is missing, so a screen never says
//! "created" over a repository the push never filled.

use std::path::{Path, PathBuf};

use nostr::{EventBuilder, Keys, Kind, Tag};
use serde::Serialize;
use tauri::{AppHandle, State};

use crate::app_state::AppState;
use crate::commands::project_git_exec::run_git;
use crate::managed_agents::packs_cache;
use crate::managed_agents::packs_repo::{
    build_repo_announcement, default_repo_id, parse_project_coordinate, read_push_record_id,
    resolve_app_commit_identity, withdraw_announcement, KIND_REPO_ANNOUNCEMENT, SEED_BRANCH,
};
use buzz_core_pkg::project_pack_source::{
    decode_project_pack_source, PACK_PATH_ROOT, PROJECT_PACK_SOURCE_SCHEMA,
};
use buzz_persona_pkg::template::TemplateCatalog;

/// Suffix appended to a project's slug to name its agents repository.
pub const AGENTS_REPO_SUFFIX: &str = "-beekeeper-agents";

/// Kind of a project's pack-source record.
const KIND_PROJECT_PACK_SOURCE: u16 = buzz_core_pkg::kind::KIND_PROJECT_PACK_SOURCE as u16;

/// Message on the one commit the seed writes.
const SEED_COMMIT_MESSAGE: &str = "seed the team from Beekeeper's shipped role templates";

/// `<slug>-beekeeper-agents`, the suffix kept whole inside the 64-byte id.
pub fn default_agents_repo_id(project_slug: &str) -> Result<String, String> {
    default_repo_id(project_slug, AGENTS_REPO_SUFFIX)
}

/// What creating a project's repositories actually produced — every wire
/// fact, and `gap` when the sequence did not finish.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectAgentsInit {
    /// The project coordinate this ran for.
    pub project_ref: String,
    /// `30617:<viewer>:<slug>`.
    pub code_repo_ref: String,
    /// The code repository's `d` tag.
    pub code_repo_id: String,
    /// Event id of the code repository's announcement when this run
    /// published it; `null` when it already existed or was not reached.
    pub code_announcement_event_id: Option<String>,
    /// The code repository was already announced under the viewer's key.
    pub code_repo_existed: bool,
    /// `30617:<viewer>:<slug>-beekeeper-agents`.
    pub agents_repo_ref: String,
    /// The agents repository's `d` tag.
    pub agents_repo_id: String,
    /// The relay git URL the agents repository is served at.
    pub agents_clone_url: String,
    /// Event id of the agents repository's announcement when this run
    /// published it.
    pub agents_announcement_event_id: Option<String>,
    /// The agents repository was already announced under the viewer's key.
    pub agents_repo_existed: bool,
    /// The branch the seed is on.
    pub branch: String,
    /// The roles seeded, ascending; empty when the seed was skipped or failed.
    pub roles: Vec<String>,
    /// The seed commit, or `null` when seeding failed or was skipped.
    pub seed_commit_sha: Option<String>,
    /// The seed step's own words when it failed before there was a commit.
    pub seed_error: Option<String>,
    /// The relay already held a push record, so nothing was seeded or pushed.
    pub seed_skipped: bool,
    /// Whether the agents repository holds the seed on the relay — pushed
    /// by this run, or already there.
    pub pushed: bool,
    /// The push's own words when it did not land.
    pub push_error: Option<String>,
    /// The relay-signed kind:30618 recording the push, read back; never
    /// fabricated.
    pub push_record_event_id: Option<String>,
    /// Event id of the kind:30624 this run published.
    pub source_event_id: Option<String>,
    /// The project already had a pack source naming the agents repository.
    pub source_existed: bool,
    /// The relay's refusal of a published event, when one was refused.
    pub publication_error: Option<String>,
    /// The identity the seed commit was (or would have been) authored as.
    pub commit_identity_name: String,
    pub commit_identity_email: String,
    /// Event id of the kind:5 withdrawing this run's agents announcement
    /// after its seed or push failed.
    pub agents_announcement_withdrawn_event_id: Option<String>,
    /// The withdrawal's own words when the tombstone itself failed.
    pub agents_announcement_withdrawal_error: Option<String>,
    /// Both repositories announced, the seed on the relay, the source set.
    pub complete: bool,
    /// One sentence naming what is missing when `complete` is `false`.
    pub gap: Option<String>,
}

impl ProjectAgentsInit {
    fn started(
        project_ref: &str,
        viewer: &str,
        code_repo_id: &str,
        agents_repo_id: &str,
        agents_clone_url: &str,
        identity: (String, String),
    ) -> Self {
        Self {
            project_ref: project_ref.to_string(),
            code_repo_ref: format!("30617:{viewer}:{code_repo_id}"),
            code_repo_id: code_repo_id.to_string(),
            code_announcement_event_id: None,
            code_repo_existed: false,
            agents_repo_ref: format!("30617:{viewer}:{agents_repo_id}"),
            agents_repo_id: agents_repo_id.to_string(),
            agents_clone_url: agents_clone_url.to_string(),
            agents_announcement_event_id: None,
            agents_repo_existed: false,
            branch: SEED_BRANCH.to_string(),
            roles: Vec::new(),
            seed_commit_sha: None,
            seed_error: None,
            seed_skipped: false,
            pushed: false,
            push_error: None,
            push_record_event_id: None,
            source_event_id: None,
            source_existed: false,
            publication_error: None,
            commit_identity_name: identity.0,
            commit_identity_email: identity.1,
            agents_announcement_withdrawn_event_id: None,
            agents_announcement_withdrawal_error: None,
            complete: false,
            gap: None,
        }
    }

    /// Settle `complete` and `gap` from the facts. An announcement this run
    /// withdrew counts as not announced: the next run announces it again.
    fn settle(mut self) -> Self {
        let code_ok = self.code_repo_existed || self.code_announcement_event_id.is_some();
        let agents_ok = self.agents_repo_existed
            || (self.agents_announcement_event_id.is_some()
                && self.agents_announcement_withdrawn_event_id.is_none());
        let source_ok = self.source_existed || self.source_event_id.is_some();
        self.complete = code_ok && agents_ok && self.pushed && source_ok;
        self.gap = if self.complete {
            None
        } else if !code_ok {
            Some(format!(
                "code repository {} not announced: {}",
                self.code_repo_id,
                self.publication_error.as_deref().unwrap_or("not reached")
            ))
        } else if !self.pushed {
            Some(format!(
                "agents repository {} not seeded: {}",
                self.agents_repo_id,
                self.seed_error
                    .as_deref()
                    .or(self.push_error.as_deref())
                    .or(self.publication_error.as_deref())
                    .unwrap_or("not reached")
            ))
        } else if !agents_ok {
            Some(format!(
                "agents repository {} not announced: {}",
                self.agents_repo_id,
                self.publication_error.as_deref().unwrap_or("not reached")
            ))
        } else {
            Some(format!(
                "pack source not set: {}",
                self.publication_error.as_deref().unwrap_or("not reached")
            ))
        };
        self
    }
}

/// What the seed step (write, git init/commit, then push) produced.
enum SeedOutcome {
    Seeded {
        commit: String,
        roles: Vec<String>,
        push_error: Option<String>,
    },
    SeedFailed {
        seed_error: String,
    },
}

/// Create the project's two repositories, or finish creating them.
///
/// See the module docs for the order and the rollback. The viewer's own key
/// signs everything and pushes; the relay decides whether that key may
/// announce inside this project, and its refusal is returned verbatim.
#[tauri::command]
pub async fn project_agents_init(
    app: AppHandle,
    state: State<'_, AppState>,
    project_ref: String,
) -> Result<ProjectAgentsInit, String> {
    let catalog = packs_cache::template_catalog(&app);
    let packs_root = packs_cache::packs_root(&app)?;
    project_agents_init_with_paths(&state, project_ref, catalog, packs_root).await
}

/// [`project_agents_init`]'s body, taking the template catalog and the packs
/// cache root directly so tests need no Tauri app.
pub(crate) async fn project_agents_init_with_paths(
    state: &AppState,
    project_ref: String,
    catalog: TemplateCatalog,
    packs_root: PathBuf,
) -> Result<ProjectAgentsInit, String> {
    let project = project_ref.trim().to_string();
    let (_owner, slug) = parse_project_coordinate(&project)?;
    let keys = state.signing_keys()?;
    let viewer = keys.public_key().to_hex();
    let code_repo_id = slug.clone();
    let agents_repo_id = default_agents_repo_id(&slug)?;
    // Refuse before anything is signed if either coordinate is not one this
    // host would later stage from or push to.
    packs_cache::parse_repo_coordinate(&format!("30617:{viewer}:{code_repo_id}"))?;
    packs_cache::parse_repo_coordinate(&format!("30617:{viewer}:{agents_repo_id}"))?;
    let relay_http =
        crate::relay::relay_http_base_url(&crate::relay::relay_ws_url_with_override(state));
    let agents_clone_url = packs_cache::packs_clone_url(&relay_http, &viewer, &agents_repo_id);
    let code_clone_url = packs_cache::packs_clone_url(&relay_http, &viewer, &code_repo_id);
    let agents_repo = format!("30617:{viewer}:{agents_repo_id}");

    // Step 1 — preflight, community-wide.
    let preflight = crate::relay::query_relay(
        state,
        &[
            serde_json::json!({ "kinds": [KIND_REPO_ANNOUNCEMENT], "#d": [code_repo_id], "limit": 8 }),
            serde_json::json!({ "kinds": [KIND_REPO_ANNOUNCEMENT], "#d": [agents_repo_id], "limit": 8 }),
            serde_json::json!({ "kinds": [KIND_PROJECT_PACK_SOURCE], "#d": [project], "limit": 1 }),
        ],
    )
    .await
    .map_err(|error| format!("could not read the relay before creating repositories: {error}"))?;
    let mut code_existed = false;
    let mut agents_existed = false;
    for event in &preflight {
        if event.kind != Kind::Custom(KIND_REPO_ANNOUNCEMENT) {
            continue;
        }
        let Some(id) = event.tags.iter().find_map(|tag| {
            let tag = tag.as_slice();
            (tag.first().map(String::as_str) == Some("d")).then(|| tag.get(1).cloned())?
        }) else {
            continue;
        };
        let author = event.pubkey.to_hex();
        if author != viewer {
            return Err(format!(
                "repository id {id:?} is already taken in this community by {}…; repository ids are \
                 one namespace per community, so choose another project name — nothing was changed",
                &author[..8]
            ));
        }
        if id == code_repo_id {
            code_existed = true;
        } else if id == agents_repo_id {
            agents_existed = true;
        }
    }
    let mut source_existed = false;
    if let Some(existing) = preflight
        .iter()
        .find(|event| event.kind == Kind::Custom(KIND_PROJECT_PACK_SOURCE))
    {
        match decode_project_pack_source(existing) {
            Ok(source) if source.repo() == agents_repo => source_existed = true,
            Ok(source) => {
                return Err(format!(
                    "{project} already has a pack source naming {}; replacing it re-points every \
                     seat on the project, which is a deliberate `bee packs set-source` — nothing \
                     was changed",
                    source.repo()
                ));
            }
            Err(error) => {
                return Err(format!(
                    "{project} has a pack source this host cannot read ({error}); nothing was changed"
                ));
            }
        }
    }

    let identity = resolve_app_commit_identity(state, &viewer).await;
    let mut result = ProjectAgentsInit::started(
        &project,
        &viewer,
        &code_repo_id,
        &agents_repo_id,
        &agents_clone_url,
        identity.clone(),
    );
    result.code_repo_existed = code_existed;
    result.agents_repo_existed = agents_existed;
    result.source_existed = source_existed;

    // Step 2 — the code repository: an announcement, nothing more.
    if !code_existed {
        let announcement = build_repo_announcement(
            &keys,
            &code_repo_id,
            &project,
            &slug,
            "This project's code.",
            &code_clone_url,
        )?;
        match crate::relay::submit_signed_event_with_keys(&announcement, state, &keys, None).await {
            Ok(_) => result.code_announcement_event_id = Some(announcement.id.to_hex()),
            Err(error) => {
                result.publication_error = Some(error);
                return Ok(result.settle());
            }
        }
    }

    // Step 3 — the agents repository's announcement.
    let announced_agents_now = !agents_existed;
    if announced_agents_now {
        let announcement = build_repo_announcement(
            &keys,
            &agents_repo_id,
            &project,
            &format!("{slug} agents"),
            "This project's agent team and plans: roles/ and plans/, each with an archive/ for \
             what is retired. Beekeeper's role source (kind:30624, path `.`).",
            &agents_clone_url,
        )?;
        match crate::relay::submit_signed_event_with_keys(&announcement, state, &keys, None).await {
            Ok(_) => result.agents_announcement_event_id = Some(announcement.id.to_hex()),
            Err(error) => {
                result.publication_error = Some(error);
                return Ok(result.settle());
            }
        }
    }

    // Step 4 — seed and push, unless the relay already holds the push.
    if agents_existed {
        if let Some(record) = read_push_record_id(state, &viewer, &agents_repo_id).await {
            result.seed_skipped = true;
            result.pushed = true;
            result.push_record_event_id = Some(record);
        }
    }
    if !result.pushed {
        let checkout = packs_cache::packs_checkout_dir(&packs_root, &viewer, &agents_repo_id);
        let seed_keys = keys.clone();
        let seed_url = agents_clone_url.clone();
        let seed_slug = slug.clone();
        let seeded: SeedOutcome =
            tokio::task::spawn_blocking(move || -> Result<SeedOutcome, String> {
                let mut auth =
                    crate::commands::project_git_exec::build_git_auth_config_for_keys(&seed_keys)?;
                auth.set_commit_identity(identity.0, identity.1);
                match seed_agents_checkout(&checkout, &catalog, &seed_slug, &auth) {
                    Err(seed_error) => Ok(SeedOutcome::SeedFailed { seed_error }),
                    Ok((commit, roles)) => {
                        let push_error = run_git(
                            &[
                                "push",
                                "--quiet",
                                "--",
                                &seed_url,
                                &format!("HEAD:refs/heads/{SEED_BRANCH}"),
                            ],
                            Some(&checkout),
                            &auth,
                        )
                        .err();
                        Ok(SeedOutcome::Seeded {
                            commit,
                            roles,
                            push_error,
                        })
                    }
                }
            })
            .await
            .map_err(|error| format!("seeding the agents repository did not finish: {error}"))??;
        match seeded {
            SeedOutcome::SeedFailed { seed_error } => result.seed_error = Some(seed_error),
            SeedOutcome::Seeded {
                commit,
                roles,
                push_error,
            } => {
                result.seed_commit_sha = Some(commit);
                result.roles = roles;
                result.pushed = push_error.is_none();
                result.push_error = push_error;
            }
        }
        if !result.pushed && announced_agents_now {
            // This run's own announcement promised a seeded repository; keep
            // that promise or withdraw it. The code announcement stands.
            // The announcement's id stays on the result — it did land — and
            // the withdrawal is reported beside it.
            let (withdrawn, error) = withdraw_announcement(state, &keys, &agents_repo_id).await;
            result.agents_announcement_withdrawn_event_id = withdrawn;
            result.agents_announcement_withdrawal_error = error;
        }
        if result.pushed {
            result.push_record_event_id =
                read_push_record_id(state, &viewer, &agents_repo_id).await;
        }
    }

    // Step 5 — the source, only over a repository that holds the seed.
    if result.pushed && !source_existed {
        let source = build_agents_pack_source(&keys, &project, &agents_repo)?;
        match crate::relay::submit_signed_event_with_keys(&source, state, &keys, None).await {
            Ok(_) => result.source_event_id = Some(source.id.to_hex()),
            Err(error) => result.publication_error = Some(error),
        }
    }

    Ok(result.settle())
}

/// Write the seed into a fresh `checkout` and commit it once. Returns
/// `(commit, roles)`. A checkout that already exists is cleared first, so a
/// retry after a failed push seeds the same tree rather than layering.
pub(crate) fn seed_agents_checkout(
    checkout: &Path,
    catalog: &TemplateCatalog,
    slug: &str,
    auth: &crate::commands::project_git_exec::GitAuthConfig,
) -> Result<(String, Vec<String>), String> {
    if checkout.exists() {
        std::fs::remove_dir_all(checkout)
            .map_err(|error| format!("clear {}: {error}", checkout.display()))?;
    }
    std::fs::create_dir_all(checkout)
        .map_err(|error| format!("create {}: {error}", checkout.display()))?;
    let report = buzz_persona_pkg::seed::write_agents_repo_seed(checkout, catalog, slug)
        .map_err(|error| error.to_string())?;
    run_git(
        &["init", "--quiet", "--initial-branch", SEED_BRANCH],
        Some(checkout),
        auth,
    )?;
    if !checkout.join(".git").is_dir() {
        return Err("git init produced no repository".to_string());
    }
    run_git(&["add", "--all"], Some(checkout), auth)?;
    run_git(
        &["commit", "--quiet", "-m", SEED_COMMIT_MESSAGE],
        Some(checkout),
        auth,
    )?;
    let commit = run_git(&["rev-parse", "HEAD"], Some(checkout), auth)?
        .trim()
        .to_string();
    Ok((commit, report.roles))
}

/// The kind:30624 for `project` naming its agents repository: the branch,
/// at the root (spec § 4.7 as amended, § 4.11).
fn build_agents_pack_source(
    keys: &Keys,
    project: &str,
    repo: &str,
) -> Result<nostr::Event, String> {
    let content = serde_json::json!({ "schema": PROJECT_PACK_SOURCE_SCHEMA }).to_string();
    let tags = vec![
        Tag::parse(vec!["d".to_string(), project.to_string()])
            .map_err(|error| format!("invalid d tag: {error}"))?,
        Tag::parse(vec!["repo".to_string(), repo.to_string()])
            .map_err(|error| format!("invalid repo tag: {error}"))?,
        Tag::parse(vec!["ref".to_string(), format!("refs/heads/{SEED_BRANCH}")])
            .map_err(|error| format!("invalid ref tag: {error}"))?,
        Tag::parse(vec!["path".to_string(), PACK_PATH_ROOT.to_string()])
            .map_err(|error| format!("invalid path tag: {error}"))?,
    ];
    EventBuilder::new(Kind::Custom(KIND_PROJECT_PACK_SOURCE), content)
        .tags(tags)
        .sign_with_keys(keys)
        .map_err(|error| format!("sign the pack source: {error}"))
}

#[cfg(test)]
#[path = "agents_repo_tests.rs"]
mod tests;
