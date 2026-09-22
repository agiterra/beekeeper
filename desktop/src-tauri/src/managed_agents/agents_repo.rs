//! Creating a project's two repositories, from inside the app (spec § 4.11).
//!
//! Every project owns a code repository, `<slug>`, seeded with one commit so
//! a worktree can be cut from it (ledger 174), and an agents repository,
//! `<slug>-beekeeper-agents`, seeded with the team and pinned as the
//! project's kind:30624 source (`ref: refs/heads/main`, `path: .`).
//! [`project_agents_init`] is the whole sequence as one host command, run
//! right after the desktop publishes the kind:30621, and again from
//! **Finish setup** when any step did not land — it is idempotent: what
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
//!    already has a pack source naming a *different* repository refuses too,
//!    unless the caller asked for a **migration** and named the exact source
//!    event it saw: re-pointing every seat is deliberate, never a side
//!    effect. The head's own `a` references are read here as well, so a
//!    project created before the pivot keeps the code repository it already
//!    has instead of being given an empty second one under its slug.
//! 2. **Announce** the code repository, then **seed** it with one commit on
//!    `refs/heads/main` — a `README.md` naming the project and where its
//!    roles and plans live — and **push** it. A repository with no commit
//!    cannot host a worktree (`git worktree add … HEAD` on an unborn `HEAD`
//!    fails), which is what every seat is cut from. Skipped when the relay
//!    already holds a push record for it. A failure here is disclosed and
//!    the sequence continues: the announcement stands.
//! 3. **Announce** the agents repository.
//! 4. **Seed** it in this host's packs cache — from this build's shipped role
//!    templates by reference (`buzz_persona::seed`), or, when migrating a
//!    pack-layout source, from that source's own roles converted into the
//!    flat layout (`buzz_persona::migrate`) — one commit authored as this
//!    host's identity, and **push** `refs/heads/main`. Skipped when the
//!    relay already holds a push record for it. A seed or push failure after
//!    this run's own announcement withdraws that announcement, as the packs
//!    flow does.
//! 5. **Publish** the kind:30624 — only once the push landed. A migration
//!    publishes it *conditionally* on the source event it was asked to
//!    replace, so a source that moved while the seed was being built leaves
//!    everything standing and says so rather than overwriting a decision
//!    nobody saw.
//! 6. **Clone** the seeded code repository to `<checkout parent>/<slug>` and
//!    **record** it as the project's folder (`by_project`, ledger 174) — the
//!    folder a founded session pre-fills and a lead's hires are cut from. A
//!    folder already recorded is kept when it is a checkout of this
//!    repository, and refused by name when it is not; nothing is
//!    overwritten.
//! 7. **Roster.** (In [`project_agents_init`], after the default agents are
//!    installed.) Every managed agent associated with the project is put on
//!    its roster as a collaborator in one kind 9010 (ledger 173), so a seat
//!    running under the agent's own key may write Pulse and to-dos. Only the
//!    project's creator or an owner may; otherwise the result says so.
//!
//! The result reports every wire fact and, when the sequence did not finish,
//! one sentence (`gap`) saying what is missing, so a screen never says
//! "created" over a repository the push never filled.

use std::path::{Path, PathBuf};

use nostr::{Event, EventBuilder, Keys, Kind, Tag};
use tauri::{AppHandle, State};

use crate::app_state::AppState;
use crate::commands::project_git_exec::{run_git, GitAuthConfig};
use crate::managed_agents::packs_cache;
use crate::managed_agents::packs_repo::{
    build_repo_announcement, default_repo_id, parse_project_coordinate, read_push_record_id,
    resolve_app_commit_identity, withdraw_announcement, KIND_REPO_ANNOUNCEMENT, SEED_BRANCH,
};
use crate::managed_agents::project_roster;
use buzz_core_pkg::kind::KIND_PROJECT;
use buzz_core_pkg::project_pack_source::{
    decode_project_pack_source, PackPin, PACK_PATH_ROOT, PROJECT_PACK_SOURCE_SCHEMA,
};
use buzz_persona_pkg::template::TemplateCatalog;

pub(crate) use super::agents_repo_migrate::MigrateFromSource;
pub use super::agents_repo_migrate::MigrateRequest;
pub(crate) use super::agents_repo_migrate::{
    adopted_code_repo_id, build_migrated_pack_source, convert_agents_checkout, LegacySource,
};
pub use super::agents_repo_result::ProjectAgentsInit;

/// Suffix appended to a project's slug to name its agents repository.
pub const AGENTS_REPO_SUFFIX: &str = "-beekeeper-agents";

/// Kind of a project's pack-source record.
pub(crate) const KIND_PROJECT_PACK_SOURCE: u16 =
    buzz_core_pkg::kind::KIND_PROJECT_PACK_SOURCE as u16;

/// Message on the one commit the agents seed writes.
const SEED_COMMIT_MESSAGE: &str = "seed the team from Beekeeper's shipped role templates";

/// Message on the one commit the code seed writes.
const CODE_SEED_COMMIT_MESSAGE: &str = "seed the project's code repository";

/// Message on the one commit a migration's conversion writes.
pub(crate) const MIGRATE_COMMIT_MESSAGE: &str =
    "convert the project's roles from its pack-layout source into this repository";

/// The relay's refusal code when a conditional source's expectation did not
/// match what it holds (`crates/buzz-relay/src/handlers/ingest_error.rs`).
const PACK_SOURCE_CONFLICT: &str = "PACK_SOURCE_CONFLICT";

/// `<slug>-beekeeper-agents`, the suffix kept whole inside the 64-byte id.
pub fn default_agents_repo_id(project_slug: &str) -> Result<String, String> {
    default_repo_id(project_slug, AGENTS_REPO_SUFFIX)
}

/// What the seed step (write, git init/commit, then push) produced.
enum SeedOutcome {
    Seeded {
        commit: String,
        roles: Vec<String>,
        /// What a conversion could not carry across, one sentence per role;
        /// empty for an ordinary seed.
        notes: Vec<String>,
        push_error: Option<String>,
    },
    SeedFailed {
        seed_error: String,
    },
}

/// How [`project_agents_init_with_paths`] reaches this host's disk and git —
/// the command fills it from the app; tests from scratch directories.
pub(crate) struct ProjectAgentsInitOptions {
    /// The folder the code checkout goes *under*: the clone lands at
    /// `<checkout_parent>/<slug>`.
    pub checkout_parent: PathBuf,
    /// The folder `by_project` already names for this project, if any.
    pub recorded_checkout: Option<PathBuf>,
    /// The git configuration a remote operation (push, clone) runs with for
    /// the given keys — the credentialed git in production
    /// (`build_git_auth_config_for_keys`, ledger 168).
    pub git_auth: fn(&Keys) -> Result<GitAuthConfig, String>,
    /// Set when the caller asked to move a project off a source that names
    /// another repository. Absent, such a project refuses.
    pub migrate: Option<MigrateFromSource>,
}

/// Where a checkout was recorded — the command writes `by_project`; tests
/// capture the path.
pub(crate) type RecordCheckout<'a> = &'a mut (dyn FnMut(&Path) -> Result<(), String> + Send);

/// Create the project's two repositories, or finish creating them.
///
/// See the module docs for the order and the rollback. The viewer's own key
/// signs everything and pushes; the relay decides whether that key may
/// announce inside this project, and its refusal is returned verbatim.
/// `checkout_parent` is the folder the code checkout goes under; `None`
/// means the community names no repositories folder, so the default one
/// (`default_repos_root`) is used.
#[tauri::command]
pub async fn project_agents_init(
    app: AppHandle,
    state: State<'_, AppState>,
    project_ref: String,
    checkout_parent: Option<String>,
    migrate: Option<MigrateRequest>,
) -> Result<ProjectAgentsInit, String> {
    let project = project_ref.trim().to_string();
    parse_project_coordinate(&project)?;
    let checkout_parent = match checkout_parent
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        Some(parent) => {
            let parent = PathBuf::from(parent);
            if !parent.is_absolute() {
                return Err(format!(
                    "the repository folder {} must be an absolute path",
                    parent.display()
                ));
            }
            parent
        }
        None => PathBuf::from(crate::commands::default_repos_root()?),
    };
    let catalog = packs_cache::template_catalog(&app);
    let packs_root = packs_cache::packs_root(&app)?;
    let recorded_checkout =
        crate::coding_sessions::workdir_store::project::recorded_project_checkout(&app, &project);
    let options = ProjectAgentsInitOptions {
        checkout_parent,
        recorded_checkout,
        git_auth: crate::commands::project_git_exec::build_git_auth_config_for_keys,
        migrate: migrate.map(MigrateFromSource::try_from).transpose()?,
    };
    let record_app = app.clone();
    let record_project = project.clone();
    let mut record = |path: &Path| {
        use tauri::Manager;
        let state = record_app.state::<AppState>();
        crate::coding_sessions::workdir_store::project::set_project_checkout(
            &record_app,
            &state,
            &record_project,
            path.to_path_buf(),
        )
    };
    let mut result = project_agents_init_with_paths(
        &state,
        project.clone(),
        catalog,
        packs_root.clone(),
        options,
        &mut record,
    )
    .await?;
    if result.pushed {
        // The provider reads `actions.yml` from this clone from now on
        // (spec § 4.11). Best-effort: the repositories exist either way.
        let viewer = state.signing_keys()?.public_key().to_hex();
        let checkout =
            packs_cache::packs_checkout_dir(&packs_root, &viewer, &result.agents_repo_id);
        if let Err(error) = crate::coding_sessions::workdir_store::record_agents_repo(
            &app,
            &state,
            &result.project_ref,
            checkout,
            &format!("refs/heads/{}", result.branch),
        ) {
            tracing::warn!(
                target: "agents_repo",
                %error,
                "the agents repository was created but this host could not record its clone"
            );
        }
        // The project's default agents: one identity per seeded role
        // (spec § 4.11). Blocking work — git and the composer — off the
        // async runtime. A failure is disclosed on the result, never a
        // failed create.
        let source = packs_cache::ProjectPackSource {
            repo: result.agents_repo_ref.clone(),
            git_ref: Some(format!("refs/heads/{}", result.branch)),
            sha: None,
            path: PACK_PATH_ROOT.to_string(),
        };
        let project = result.project_ref.clone();
        let install_app = app.clone();
        let installed = tokio::task::spawn_blocking(move || {
            use tauri::Manager;
            let state = install_app.state::<AppState>();
            crate::managed_agents::default_agents::install_default_agents(
                &install_app,
                &state,
                &project,
                &source,
            )
        })
        .await
        .map_err(|error| format!("installing the default agents did not finish: {error}"))?;
        match installed {
            Ok(agents) => result.agents_installed = agents,
            Err(error) => result.agents_error = Some(error),
        }
    }
    // Step 7 — the roster: every agent associated with the project, the ones
    // just installed and any already on disk, as collaborators (ledger 173).
    // Runs whether or not the seed landed: agents from an earlier run are
    // owed their membership too.
    let roster_app = app.clone();
    let roster_project = project.clone();
    let agents = tokio::task::spawn_blocking(move || {
        use tauri::Manager;
        let state = roster_app.state::<AppState>();
        crate::managed_agents::default_agents::project_agent_pubkeys(
            &roster_app,
            &state,
            &roster_project,
        )
    })
    .await
    .map_err(|error| format!("listing the project's agents did not finish: {error}"))?;
    match agents {
        Ok(agents) => {
            let keys = state.signing_keys()?;
            let outcome =
                project_roster::ensure_project_agents_on_roster(&state, &keys, &project, &agents)
                    .await;
            result.roster_added = outcome.added;
            result.roster_error = outcome.error;
        }
        Err(error) => {
            result.roster_error = Some(format!("could not list the project's agents: {error}"))
        }
    }
    Ok(result.settle())
}

/// Sync this host's clone of a project's agents repository and record where
/// it is, so the provider's next host step reads `actions.yml` from its
/// fetched tip (spec § 4.11). `Ok(false)` when the source is not an agents
/// repository (its `path` is not the repository root) — nothing is recorded
/// and the caller says so.
#[tauri::command]
pub async fn record_project_agents_repo(
    app: AppHandle,
    state: State<'_, AppState>,
    project_ref: String,
    repo: String,
    git_ref: Option<String>,
    sha: Option<String>,
    path: Option<String>,
) -> Result<bool, String> {
    let project = project_ref.trim().to_string();
    parse_project_coordinate(&project)?;
    let source = packs_cache::ProjectPackSource {
        repo: repo.trim().to_string(),
        git_ref: git_ref
            .map(|value| value.trim().to_string())
            .filter(|v| !v.is_empty()),
        sha: sha
            .map(|value| value.trim().to_string())
            .filter(|v| !v.is_empty()),
        path: packs_cache::validate_pack_path(path.as_deref().unwrap_or_default())?,
    };
    if !buzz_core_pkg::project_pack_source::is_root_pack_path(&source.path) {
        return Ok(false);
    }
    let Some(ref_name) = source.git_ref.clone() else {
        // A sha-pinned agents repository has no branch to fetch; the
        // provider reads a ref's tip, so there is nothing honest to record.
        return Ok(false);
    };
    let (owner, id) = packs_cache::parse_repo_coordinate(&source.repo)?;
    let packs_root = packs_cache::packs_root(&app)?;
    let checkout = packs_cache::packs_checkout_dir(&packs_root, &owner, &id);
    let relay_http =
        crate::relay::relay_http_base_url(&crate::relay::relay_ws_url_with_override(&state));
    let clone_url = packs_cache::packs_clone_url(&relay_http, &owner, &id);
    let auth = crate::commands::project_git_exec::build_git_auth_config(&state)?;
    let sync_checkout = checkout.clone();
    tokio::task::spawn_blocking(move || {
        packs_cache::sync_packs_checkout(&sync_checkout, &clone_url, &source, &auth)
    })
    .await
    .map_err(|error| format!("syncing the agents repository did not finish: {error}"))??;
    crate::coding_sessions::workdir_store::record_agents_repo(
        &app, &state, &project, checkout, &ref_name,
    )?;
    Ok(true)
}

/// The newest kind:30621 in `events` by `owner` with `d` = `dtag` whose
/// signature verifies.
fn newest_project_head<'a>(events: &'a [Event], owner: &str, dtag: &str) -> Option<&'a Event> {
    events
        .iter()
        .filter(|event| {
            u32::from(event.kind.as_u16()) == KIND_PROJECT
                && event.pubkey.to_hex() == owner
                && event.tags.iter().any(|tag| {
                    let parts = tag.as_slice();
                    parts.first().map(String::as_str) == Some("d")
                        && parts.get(1).map(String::as_str) == Some(dtag)
                })
                && event.verify().is_ok()
        })
        .max_by_key(|event| (event.created_at, event.id))
}

/// [`project_agents_init`]'s body, taking the template catalog, the packs
/// cache root and the checkout facts directly so tests need no Tauri app.
/// Steps 1–6; the roster (step 7) needs the installed agents and is the
/// command's.
pub(crate) async fn project_agents_init_with_paths(
    state: &AppState,
    project_ref: String,
    catalog: TemplateCatalog,
    packs_root: PathBuf,
    options: ProjectAgentsInitOptions,
    record_checkout: RecordCheckout<'_>,
) -> Result<ProjectAgentsInit, String> {
    let project = project_ref.trim().to_string();
    let (owner, slug) = parse_project_coordinate(&project)?;
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
    let agents_repo = format!("30617:{viewer}:{agents_repo_id}");

    // Step 1a — the project's own record and the source it points at now.
    // The head is read first because it decides which code repository this
    // project has: one created before the pivot names its own, under an id
    // that is not the slug, and giving it a second empty one would strand
    // the folder every seat is cut from (ledger 175).
    let head_facts = crate::relay::query_relay(
        state,
        &[
            serde_json::json!({ "kinds": [KIND_PROJECT_PACK_SOURCE], "#d": [project], "limit": 1 }),
            serde_json::json!({ "kinds": [KIND_PROJECT], "authors": [owner], "#d": [slug], "limit": 8 }),
        ],
    )
    .await
    .map_err(|error| format!("could not read the relay before creating repositories: {error}"))?;
    let head = newest_project_head(&head_facts, &owner, &slug);
    let project_name = project_roster::project_name(head, &slug);
    let adopted = adopted_code_repo_id(head, &viewer, &agents_repo_id);
    let code_repo_adopted = adopted.is_some();
    let code_repo_id = adopted.unwrap_or(code_repo_id);
    let agents_clone_url = packs_cache::packs_clone_url(&relay_http, &viewer, &agents_repo_id);
    let code_clone_url = packs_cache::packs_clone_url(&relay_http, &viewer, &code_repo_id);
    packs_cache::parse_repo_coordinate(&format!("30617:{viewer}:{code_repo_id}"))?;

    // Step 1b — both ids, community-wide.
    let preflight = crate::relay::query_relay(
        state,
        &[
            serde_json::json!({ "kinds": [KIND_REPO_ANNOUNCEMENT], "#d": [code_repo_id], "limit": 8 }),
            serde_json::json!({ "kinds": [KIND_REPO_ANNOUNCEMENT], "#d": [agents_repo_id], "limit": 8 }),
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
    // Set when this run is a migration: the repository the project points
    // at today, and where its roles are inside it.
    let mut migrating: Option<LegacySource> = None;
    if let Some(existing) = head_facts
        .iter()
        .find(|event| event.kind == Kind::Custom(KIND_PROJECT_PACK_SOURCE))
    {
        let existing_id = existing.id.to_hex();
        match decode_project_pack_source(existing) {
            Ok(source) if source.repo() == agents_repo => source_existed = true,
            Ok(source) => match &options.migrate {
                Some(request) if request.expected_source_id == existing_id => {
                    let (git_ref, sha) = match source.pin() {
                        PackPin::Ref(name) => (Some(name.clone()), None),
                        PackPin::Sha(sha) => (None, Some(sha.clone())),
                    };
                    migrating = Some(LegacySource {
                        repo: source.repo().to_string(),
                        git_ref,
                        sha,
                        path: source.path().to_string(),
                        event_id: existing_id,
                        convert: request.convert,
                    });
                }
                Some(request) => {
                    return Err(format!(
                        "{project}'s pack source is event {existing_id}, not the {} this migration \
                         was asked to replace; someone re-pointed it since you read it — read it \
                         again and decide against what is there now. Nothing was changed",
                        request.expected_source_id
                    ));
                }
                None => {
                    return Err(format!(
                        "{project} already has a pack source naming {}; replacing it re-points \
                         every seat on the project, which is a deliberate `bee packs set-source` \
                         or the **Move this project's roles into an agents repository** button in \
                         Project settings → Packs — nothing was changed",
                        source.repo()
                    ));
                }
            },
            Err(error) => {
                return Err(format!(
                    "{project} has a pack source this host cannot read ({error}); nothing was changed"
                ));
            }
        }
    } else if options.migrate.is_some() {
        return Err(format!(
            "{project} has no pack source to migrate: it has never been pointed at a role \
             repository, so **Create the project's repositories** is the action, not a migration. \
             Nothing was changed"
        ));
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
    result.code_repo_adopted = code_repo_adopted;
    result.agents_repo_existed = agents_existed;
    result.source_existed = source_existed;

    // Step 2 — the code repository: its announcement, then its seed.
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
    // Skipped only over a record that names a pushed `main`: the relay also
    // writes a `HEAD`-only ref state at creation, which is not a commit
    // (ledger 176).
    if code_existed
        && read_push_record_id(state, &viewer, &code_repo_id, SEED_BRANCH)
            .await
            .is_some()
    {
        result.code_seed_skipped = true;
    }
    if !result.code_seed_skipped {
        let checkout = packs_cache::packs_checkout_dir(&packs_root, &viewer, &code_repo_id);
        let seed_keys = keys.clone();
        let seed_url = code_clone_url.clone();
        let seed_name = project_name.clone();
        let seed_agents_repo_id = agents_repo_id.clone();
        let seed_identity = identity.clone();
        let git_auth = options.git_auth;
        let seeded =
            tokio::task::spawn_blocking(move || -> Result<(String, Option<String>), String> {
                let mut auth = git_auth(&seed_keys)?;
                auth.set_commit_identity(seed_identity.0, seed_identity.1);
                let commit =
                    seed_code_checkout(&checkout, &seed_name, &seed_agents_repo_id, &auth)?;
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
                Ok((commit, push_error))
            })
            .await
            .map_err(|error| format!("seeding the code repository did not finish: {error}"))?;
        match seeded {
            Ok((commit, push_error)) => {
                result.code_seed_commit_sha = Some(commit);
                result.code_seed_error = push_error;
            }
            Err(error) => result.code_seed_error = Some(error),
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
        if let Some(record) =
            read_push_record_id(state, &viewer, &agents_repo_id, SEED_BRANCH).await
        {
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
        let git_auth = options.git_auth;
        // A migration fills the new repository from the roles the project
        // already has; an ordinary run seeds it from the shipped templates.
        let legacy = migrating
            .as_ref()
            .filter(|source| source.convert)
            .map(|source| {
                (
                    packs_cache::ProjectPackSource {
                        repo: source.repo.clone(),
                        git_ref: source.git_ref.clone(),
                        sha: source.sha.clone(),
                        path: source.path.clone(),
                    },
                    relay_http.clone(),
                    packs_root.clone(),
                )
            });
        let seeded: SeedOutcome =
            tokio::task::spawn_blocking(move || -> Result<SeedOutcome, String> {
                let mut auth = git_auth(&seed_keys)?;
                auth.set_commit_identity(identity.0, identity.1);
                let written = match legacy {
                    Some((source, relay_http, packs_root)) => convert_agents_checkout(
                        &checkout,
                        &source,
                        &relay_http,
                        &packs_root,
                        &seed_slug,
                        &auth,
                    ),
                    None => seed_agents_checkout(&checkout, &catalog, &seed_slug, &auth)
                        .map(|(commit, roles)| (commit, roles, Vec::new())),
                };
                match written {
                    Err(seed_error) => Ok(SeedOutcome::SeedFailed { seed_error }),
                    Ok((commit, roles, notes)) => {
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
                            notes,
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
                notes,
                push_error,
            } => {
                result.seed_commit_sha = Some(commit);
                result.roles = roles.clone();
                if migrating.as_ref().is_some_and(|source| source.convert) {
                    result.migrated_roles = roles;
                    result.migration_notes = notes;
                }
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
                read_push_record_id(state, &viewer, &agents_repo_id, SEED_BRANCH).await;
        }
    }

    // Step 5 — the source, only over a repository that holds the seed. A
    // migration re-points conditionally on the event it was asked to
    // replace: a source that moved while this ran is the relay's conflict
    // to refuse, not a race for this host to win.
    if result.pushed && !source_existed {
        let source = match &migrating {
            None => build_agents_pack_source(&keys, &project, &agents_repo)?,
            Some(legacy) => {
                result.migrated_from = Some(legacy.repo.clone());
                build_migrated_pack_source(&keys, &project, &agents_repo, &legacy.event_id)?
            }
        };
        match crate::relay::submit_signed_event_with_keys(&source, state, &keys, None).await {
            Ok(_) => result.source_event_id = Some(source.id.to_hex()),
            Err(error) => {
                result.source_conflict = error.contains(PACK_SOURCE_CONFLICT);
                result.publication_error = Some(error);
            }
        }
    }

    // Step 6 — the checkout, only over a code repository that holds a commit.
    if result.code_seeded() {
        let target = options.checkout_parent.join(&code_repo_id);
        // A recorded folder is reused only when it is a checkout of this
        // repository — and reused *through* the clone helper, which fetches
        // `main` into a clone cut while the repository was still empty
        // (ledger 177: the first rerun on RPG Test reported the unborn
        // clone as "already checked out" and left it without a commit).
        let (clone_target, already_recorded) = match options.recorded_checkout {
            Some(recorded) => {
                if !crate::commands::is_checkout_of(&recorded, &code_clone_url) {
                    result.checkout_error = Some(format!(
                        "the project's folder is recorded as {}, which is not a git checkout of \
                         {code_repo_id} ({code_clone_url}); this run would have cloned to {} — \
                         clear the recorded folder or point it at a checkout of the repository, \
                         then finish setup again; nothing was overwritten",
                        recorded.display(),
                        target.display()
                    ));
                    return Ok(result.settle());
                }
                (recorded, true)
            }
            None => (target, false),
        };
        let clone_keys = keys.clone();
        let clone_url = code_clone_url.clone();
        let clone_dir = clone_target.clone();
        let git_auth = options.git_auth;
        let cloned = tokio::task::spawn_blocking(move || {
            let auth = git_auth(&clone_keys)?;
            crate::commands::clone_repository_to_dir(
                &clone_dir,
                &clone_url,
                Some(SEED_BRANCH),
                &auth,
            )
        })
        .await
        .map_err(|error| format!("cloning the code repository did not finish: {error}"))?;
        match cloned {
            Ok(clone) => {
                let path = PathBuf::from(&clone.path);
                result.checkout_cloned = clone.cloned;
                result.checkout_path = Some(clone.path);
                if !already_recorded {
                    if let Err(error) = record_checkout(&path) {
                        result.checkout_error = Some(format!(
                            "{} is cloned but could not be recorded as the project's folder: {error}",
                            path.display()
                        ));
                    }
                }
            }
            Err(error) => {
                result.checkout_error = Some(if already_recorded {
                    format!(
                        "the recorded folder {} could not be brought to {SEED_BRANCH}: {error}",
                        clone_target.display()
                    )
                } else {
                    error
                })
            }
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
    auth: &GitAuthConfig,
) -> Result<(String, Vec<String>), String> {
    if checkout.exists() {
        std::fs::remove_dir_all(checkout)
            .map_err(|error| format!("clear {}: {error}", checkout.display()))?;
    }
    std::fs::create_dir_all(checkout)
        .map_err(|error| format!("create {}: {error}", checkout.display()))?;
    let report = buzz_persona_pkg::seed::write_agents_repo_seed(checkout, catalog, slug)
        .map_err(|error| error.to_string())?;
    let commit = commit_seed(checkout, SEED_COMMIT_MESSAGE, auth)?;
    Ok((commit, report.roles))
}

/// The `README.md` the code seed writes: the project's name, then where its
/// roles and plans live.
pub(crate) fn code_seed_readme(project_name: &str, agents_repo_id: &str) -> String {
    format!(
        "# {}\n\nThis project's code. Roles and plans live in `{agents_repo_id}`.\n",
        project_name.trim()
    )
}

/// Write the code repository's seed — one `README.md` — into a fresh
/// `checkout` and commit it once on `main`. Returns the commit. A checkout
/// that already exists is cleared first, as [`seed_agents_checkout`] does.
pub(crate) fn seed_code_checkout(
    checkout: &Path,
    project_name: &str,
    agents_repo_id: &str,
    auth: &GitAuthConfig,
) -> Result<String, String> {
    if checkout.exists() {
        std::fs::remove_dir_all(checkout)
            .map_err(|error| format!("clear {}: {error}", checkout.display()))?;
    }
    std::fs::create_dir_all(checkout)
        .map_err(|error| format!("create {}: {error}", checkout.display()))?;
    let readme = checkout.join("README.md");
    std::fs::write(&readme, code_seed_readme(project_name, agents_repo_id))
        .map_err(|error| format!("write {}: {error}", readme.display()))?;
    commit_seed(checkout, CODE_SEED_COMMIT_MESSAGE, auth)
}

/// `git init` on [`SEED_BRANCH`], add everything, commit once with
/// `message`; returns the commit.
pub(crate) fn commit_seed(
    checkout: &Path,
    message: &str,
    auth: &GitAuthConfig,
) -> Result<String, String> {
    run_git(
        &["init", "--quiet", "--initial-branch", SEED_BRANCH],
        Some(checkout),
        auth,
    )?;
    if !checkout.join(".git").is_dir() {
        return Err("git init produced no repository".to_string());
    }
    run_git(&["add", "--all"], Some(checkout), auth)?;
    run_git(&["commit", "--quiet", "-m", message], Some(checkout), auth)?;
    Ok(run_git(&["rev-parse", "HEAD"], Some(checkout), auth)?
        .trim()
        .to_string())
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
