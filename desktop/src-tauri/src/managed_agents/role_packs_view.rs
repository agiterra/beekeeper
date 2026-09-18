//! The Roles view's account of one project's role packs: for every role this
//! computer could stage, the pack it *would* stage, read the way staging reads
//! it.
//!
//! The dashboard's Roles tab asks one question per role — "what would a seat
//! in this role run with, in this project?" — and the only honest answer is
//! the one `stage_coding_session_actor_seat` would give. So this module walks
//! the same ladder [`crate::managed_agents::actor_seats`] stages from, in the
//! same order, with the same building blocks:
//!
//! 1. **project** — the project's kind:30624 packs repository, synced to the
//!    commit it names ([`packs_cache::sync_packs_checkout`]). When a project
//!    names one, it is the *only* rung staging consults: a role the
//!    repository does not hold is refused, not quietly served from a lower
//!    rung ([`packs_cache::HIRE_PACK_UNAVAILABLE`]).
//! 2. **checkout** — `<session checkout>/personas/roles/<role>`
//!    ([`packs_cache::checkout_role_pack`]).
//! 3. **installed** — a pack on this computer behind an agent whose home role
//!    is the role ([`resolve_local_seat_pack`]), named as *shipped* when it is
//!    byte-for-byte one of this build's packs ([`installed_seat_pack_ref`]).
//! 4. **shipped** — the packs bundled into this build
//!    ([`packs_cache::shipped_packs_dir`]).
//!
//! The set of roles shown is the union of every rung, so a role that exists
//! only lower down still appears — with that rung as its origin, and with the
//! refusal sentence when the project would not let it be staged. A row never
//! guesses: `packRef` is `null` when nothing can vouch for the directory, a
//! skill is listed only when its `SKILL.md` was read, and a pack that cannot
//! be read says so in `refusal` rather than rendering as an empty role.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::AppHandle;

use crate::app_state::AppState;
use crate::managed_agents::actor_seats::{
    installed_seat_pack_ref, resolve_local_seat_pack, SeatPackOrigin,
};
use crate::managed_agents::packs_cache::{self, PackRef};
use crate::managed_agents::{ManagedAgentRecord, TeamRecord};

/// Longest `summary` a role row carries, in characters. A longer first
/// paragraph is cut to this and ends in an ellipsis.
pub const SUMMARY_MAX_CHARS: usize = 400;

/// One skill a role carries, as its own `SKILL.md` describes it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RolePackSkill {
    /// The skill's name (`buzz_persona::skill_meta::SkillMeta::name`).
    pub name: String,
    /// Its frontmatter description, or `""` when it declares none.
    pub description: String,
    /// `true` when no persona in the pack claims it, so every persona gets it.
    pub shared: bool,
}

/// One role, from the rung of the staging ladder this computer would stage
/// it from. The Roles tab's wire contract; field names are law.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RolePackSummary {
    /// The role slug (`lead`, `builder`, …).
    pub role: String,
    /// The persona's display name, or its name when it has none.
    pub display_name: String,
    /// The persona's one-line frontmatter description; `""` when the pack
    /// could not be read.
    pub description: String,
    /// The first paragraph of the persona's prompt, at most
    /// [`SUMMARY_MAX_CHARS`] characters; `""` when there is none.
    pub summary: String,
    /// The pack's `.plugin/plugin.json` version, or `null`.
    pub version: Option<String>,
    /// Which rung answered — never hidden behind a default label.
    pub origin: SeatPackOrigin,
    /// Absolute path of the directory that would be staged.
    pub pack_dir: String,
    /// Exactly the `packRef` staging would stamp on the seat, or `null` when
    /// nothing on the wire can vouch for the directory.
    pub pack_ref: Option<PackRef>,
    /// The role's skills, each read from its `SKILL.md`. Empty means none
    /// were read, not that none were declared.
    pub skills: Vec<RolePackSkill>,
    /// The sentence a hire in this role would be refused with, or `null`
    /// when staging would go ahead.
    pub refusal: Option<String>,
    /// What the composer wanted said while staging this role. Never a
    /// refusal.
    pub warnings: Vec<String>,
    /// The staged composition's `sha256:…` digest, or `null` when nothing
    /// could be staged.
    pub compose_digest: Option<String>,
    /// `team.yml` `workspace.agents_repo` for this role (spec § 4.11): whether
    /// a seat in this role gets the project's agents repository beside its
    /// worktree, and whether it may write there.
    pub agents_repo: packs_cache::AgentsRepoAccess,
    /// A retired role under `roles/archive/`: listed so a reader sees it,
    /// never hireable, never composed (spec § 4.11).
    pub archived: bool,
}

/// The project rung, after the packs repository has been synced — or not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ProjectRung {
    /// The project publishes no kind:30624: staging uses the local rungs.
    Absent,
    /// The repository is on disk at `checkout`, landed on `sha`; role packs
    /// live under `path` inside it.
    Synced {
        /// The `30617:<owner>:<id>` coordinate, for the `packRef`.
        repo: String,
        /// The host's checkout of that repository.
        checkout: PathBuf,
        /// The validated sub-path holding one directory per role.
        path: String,
        /// The commit the checkout is on.
        sha: String,
    },
    /// The project names a source this computer could not stage; every role
    /// is refused with `reason`, exactly as a hire would be.
    Unavailable {
        /// Why the sync failed, verbatim.
        reason: String,
    },
}

/// Everything the ladder reads, gathered by the caller so the walk itself
/// touches no app handle and a test can feed it temp directories.
pub(crate) struct RolePackLadder<'a> {
    /// The project rung.
    pub project: ProjectRung,
    /// The project's session checkout on this computer, when one is recorded.
    pub checkout: Option<&'a Path>,
    /// This computer's managed agents.
    pub records: &'a [ManagedAgentRecord],
    /// This computer's teams (a pack's provenance may route through one).
    pub teams: &'a [TeamRecord],
    /// The packs this build ships, when it ships any.
    pub shipped_root: Option<&'a Path>,
    /// The version that pins the shipped packs.
    pub shipped_version: &'a str,
    /// The template catalog every composition resolves against.
    pub catalog: &'a packs_cache::TemplateCatalog,
    /// The packs cache root composed packs are staged under.
    pub packs_root: &'a Path,
}

/// One rung's answer for one role, before it is composed and read.
#[derive(Clone, Debug)]
struct Candidate {
    source: packs_cache::RoleSource,
    origin: SeatPackOrigin,
    pack_ref: Option<PackRef>,
    /// The staging key and provenance `plan_seat_pack` would stage under.
    source_key: String,
    provenance: packs_cache::SourceProvenance,
}

/// Walk the ladder and describe every role it holds, sorted by slug.
pub(crate) fn walk_role_pack_ladder(ladder: &RolePackLadder<'_>) -> Vec<RolePackSummary> {
    let project = project_candidates(&ladder.project);
    let checkout = checkout_candidates(ladder.checkout);
    let installed = installed_candidates(
        ladder.records,
        ladder.teams,
        ladder.shipped_root,
        ladder.shipped_version,
    );
    let shipped = shipped_candidates(ladder.shipped_root, ladder.shipped_version);

    let roles: BTreeSet<&String> = project
        .keys()
        .chain(checkout.keys())
        .chain(installed.keys())
        .chain(shipped.keys())
        .collect();

    let archived = archived_rows(&ladder.project, &roles);
    let mut rows: Vec<RolePackSummary> = roles
        .into_iter()
        .filter_map(|role| {
            // The local rungs, in staging's order.
            let local = checkout
                .get(role)
                .or_else(|| installed.get(role))
                .or_else(|| shipped.get(role));
            let (candidate, refusal) = match &ladder.project {
                ProjectRung::Absent => (local?, None),
                ProjectRung::Synced { path, sha, .. } => match project.get(role) {
                    Some(candidate) => (candidate, None),
                    // The same sentence `stage_project_role_pack` fails with,
                    // wrapped the way `seat_entry_for_plan` wraps it.
                    None => (
                        local?,
                        Some(project_refusal(&packs_cache::missing_role_pack_reason(
                            sha, path, role,
                        ))),
                    ),
                },
                ProjectRung::Unavailable { reason } => (local?, Some(project_refusal(reason))),
            };
            Some(summarize(
                role,
                candidate,
                refusal,
                ladder.catalog,
                ladder.packs_root,
            ))
        })
        .collect();
    rows.extend(archived);
    rows
}

/// The retired roles of a synced flat source (`roles/archive/<role>.md`,
/// spec § 4.11): one row each, refused as archived, so the Roles page can
/// say what is kept for history without offering to hire it. A retired
/// role that a live row also names is not listed twice.
fn archived_rows(rung: &ProjectRung, live: &BTreeSet<&String>) -> Vec<RolePackSummary> {
    let ProjectRung::Synced { checkout, path, .. } = rung else {
        return Vec::new();
    };
    let mut root = checkout.clone();
    for segment in path
        .split('/')
        .filter(|segment| !segment.is_empty() && *segment != ".")
    {
        root.push(segment);
    }
    buzz_persona_pkg::compose::archived_role_files(&root)
        .into_iter()
        .filter(|role| !live.iter().any(|live| *live == role))
        .map(|role| RolePackSummary {
            display_name: role.clone(),
            description: "retired: kept under roles/archive/ for history".to_string(),
            summary: String::new(),
            version: None,
            origin: SeatPackOrigin::Project,
            pack_dir: String::new(),
            pack_ref: None,
            skills: Vec::new(),
            refusal: Some(format!(
                "archived (not hireable): move roles/archive/{role}.md to roles/{role}.md to put it in force"
            )),
            warnings: Vec::new(),
            compose_digest: None,
            agents_repo: packs_cache::AgentsRepoAccess::None,
            archived: true,
            role,
        })
        .collect()
}

/// The refusal a hire carries, with its reason, byte-for-byte as
/// `actor_seats::seat_entry_for_plan` formats it.
fn project_refusal(reason: &str) -> String {
    format!("{} ({reason})", packs_cache::HIRE_PACK_UNAVAILABLE)
}

/// The immediate child directories of `parent` whose names are role slugs,
/// sorted. A name is a *candidate* only — each rung still asks the pack
/// whether a persona inside declares that role.
fn role_directories(parent: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(parent) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().to_str().map(str::to_owned))
        .filter(|name| packs_cache::is_role_slug(name))
        .collect();
    names.sort();
    names
}

fn project_candidates(rung: &ProjectRung) -> BTreeMap<String, Candidate> {
    let ProjectRung::Synced {
        repo,
        checkout,
        path,
        sha,
    } = rung
    else {
        return BTreeMap::new();
    };
    let mut parent = checkout.clone();
    for segment in path.split('/').filter(|segment| !segment.is_empty()) {
        parent.push(segment);
    }
    let mut roles = role_directories(&parent);
    for role in flat_role_files(&parent) {
        if !roles.contains(&role) {
            roles.push(role);
        }
    }
    roles.sort();
    let Ok((owner, id)) = packs_cache::parse_repo_coordinate(repo) else {
        return BTreeMap::new();
    };
    roles
        .into_iter()
        .filter_map(|role| {
            let source = packs_cache::locate_role_source(checkout, path, &role)?;
            let ref_path = packs_cache::pack_ref_path(&source, path);
            // Field-for-field what `stage_project_role_pack` stamps.
            let pack_ref = PackRef {
                repo: repo.clone(),
                sha: sha.clone(),
                role: role.clone(),
                path: ref_path.clone(),
            };
            Some((
                role,
                Candidate {
                    source,
                    origin: SeatPackOrigin::Project,
                    pack_ref: Some(pack_ref),
                    source_key: format!("{}-{sha}", packs_cache::pack_cache_dir_name(&owner, &id)),
                    provenance: packs_cache::SourceProvenance {
                        kind: "repository".to_string(),
                        repo: Some(repo.clone()),
                        sha: Some(sha.clone()),
                        path: ref_path,
                    },
                },
            ))
        })
        .collect()
}

/// The role slugs named by flat role files `<parent>/roles/<role>.md`,
/// sorted. A name is a candidate only; [`packs_cache::locate_role_source`]
/// still decides.
fn flat_role_files(parent: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(parent.join(buzz_persona_pkg::compose::FLAT_ROLES_DIR))
    else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|entry| entry.path().is_file())
        .filter_map(|entry| {
            entry
                .file_name()
                .to_str()?
                .strip_suffix(".md")
                .map(str::to_owned)
        })
        .filter(|name| packs_cache::is_role_slug(name))
        .collect();
    names.sort();
    names
}

fn checkout_candidates(checkout: Option<&Path>) -> BTreeMap<String, Candidate> {
    let Some(checkout) = checkout else {
        return BTreeMap::new();
    };
    // The pack layout under `personas/roles`, the one place `plan_seat_pack`
    // looks in a code checkout (a flat team lives in the agents repository
    // since 2026-09-18, spec § 4.11).
    let mut out = BTreeMap::new();
    for path in [packs_cache::DEFAULT_PACK_PATH] {
        let mut parent = checkout.to_path_buf();
        for segment in path.split('/').filter(|segment| !segment.is_empty()) {
            parent.push(segment);
        }
        let mut roles = role_directories(&parent);
        roles.extend(flat_role_files(&parent));
        for role in roles {
            if out.contains_key(&role) {
                continue;
            }
            let Some(source) = packs_cache::locate_role_source(checkout, path, &role) else {
                continue;
            };
            let provenance =
                packs_cache::SourceProvenance::local(packs_cache::pack_ref_path(&source, path));
            out.insert(
                role,
                Candidate {
                    source,
                    origin: SeatPackOrigin::Checkout,
                    pack_ref: None,
                    source_key: packs_cache::local_source_key(&parent),
                    provenance,
                },
            );
        }
    }
    out
}

/// The installed rung: one candidate per home role some agent on this
/// computer declares, resolved by the same rule a seat in that role is.
fn installed_candidates(
    records: &[ManagedAgentRecord],
    teams: &[TeamRecord],
    shipped_root: Option<&Path>,
    shipped_version: &str,
) -> BTreeMap<String, Candidate> {
    let mut out = BTreeMap::new();
    for record in records {
        let Some(role) = record
            .home_role
            .as_deref()
            .map(str::trim)
            .filter(|role| !role.is_empty())
        else {
            continue;
        };
        if out.contains_key(role) {
            continue;
        }
        let Some((dir, persona)) = resolve_local_seat_pack(record, records, teams, Some(role))
        else {
            continue;
        };
        let (origin, pack_ref) =
            installed_seat_pack_ref(shipped_root, shipped_version, &dir, Some(role));
        let (source_key, provenance) = local_or_shipped_key(&dir, &origin, pack_ref.as_ref());
        out.insert(
            role.to_owned(),
            Candidate {
                source: packs_cache::RoleSource::Pack {
                    dir,
                    role: role.to_owned(),
                    persona: Some(persona),
                },
                origin,
                pack_ref,
                source_key,
                provenance,
            },
        );
    }
    out
}

/// The staging key and provenance `plan_seat_pack` uses for a local rung:
/// the app version for a pack this build vouches for, a path hash otherwise.
fn local_or_shipped_key(
    dir: &Path,
    origin: &SeatPackOrigin,
    pack_ref: Option<&PackRef>,
) -> (String, packs_cache::SourceProvenance) {
    match (origin, pack_ref) {
        (SeatPackOrigin::Shipped, Some(pack_ref)) => (
            format!("app-{}", pack_ref.sha),
            packs_cache::SourceProvenance {
                kind: "shipped".to_string(),
                repo: Some(pack_ref.repo.clone()),
                sha: Some(pack_ref.sha.clone()),
                path: pack_ref.path.clone(),
            },
        ),
        _ => (
            packs_cache::local_source_key(dir),
            packs_cache::SourceProvenance::local(dir.to_string_lossy().into_owned()),
        ),
    }
}

fn shipped_candidates(
    shipped_root: Option<&Path>,
    shipped_version: &str,
) -> BTreeMap<String, Candidate> {
    let Some(root) = shipped_root else {
        return BTreeMap::new();
    };
    role_directories(root)
        .into_iter()
        .filter_map(|role| {
            let (dir, persona) = packs_cache::role_pack_in_checkout(root, "", &role)?;
            let pack_ref =
                packs_cache::shipped_pack_ref_for_dir(Some(root), &dir, &role, shipped_version);
            let (source_key, provenance) =
                local_or_shipped_key(&dir, &SeatPackOrigin::Shipped, pack_ref.as_ref());
            let source = packs_cache::RoleSource::Pack {
                dir,
                role: role.clone(),
                persona: Some(persona),
            };
            Some((
                role,
                Candidate {
                    source,
                    origin: SeatPackOrigin::Shipped,
                    pack_ref,
                    source_key,
                    provenance,
                },
            ))
        })
        .collect()
}

/// Compose and stage `candidate` exactly as a seat would be, then read the
/// staged persona and describe the role. A candidate that cannot be composed
/// carries the composer's reason as its refusal — the same refusal a hire
/// would get — rather than a description of a pack no seat would run.
fn summarize(
    role: &str,
    candidate: &Candidate,
    refusal: Option<String>,
    catalog: &packs_cache::TemplateCatalog,
    packs_root: &Path,
) -> RolePackSummary {
    let staged = match packs_cache::stage_composed_pack(
        packs_root,
        &candidate.source_key,
        &candidate.source,
        catalog,
        candidate.provenance.clone(),
    ) {
        Ok(staged) => staged,
        Err(reason) => {
            return RolePackSummary {
                role: role.to_owned(),
                display_name: role.to_owned(),
                description: String::new(),
                summary: String::new(),
                version: None,
                origin: candidate.origin,
                pack_dir: String::new(),
                pack_ref: candidate.pack_ref.clone(),
                skills: Vec::new(),
                refusal: refusal.or_else(|| {
                    Some(format!(
                        "{} ({reason})",
                        packs_cache::SEAT_PACK_UNCOMPOSABLE
                    ))
                }),
                warnings: Vec::new(),
                compose_digest: None,
                agents_repo: packs_cache::AgentsRepoAccess::None,
                archived: false,
            };
        }
    };
    let pack_dir = staged.dir.to_string_lossy().into_owned();
    match buzz_persona_pkg::resolve::resolve_persona_by_name(&staged.dir, &staged.persona) {
        Ok(persona) => {
            let skills = match buzz_persona_pkg::skill_meta::list_skill_meta(&staged.dir, &persona)
            {
                Ok(skills) => skills
                    .into_iter()
                    .map(|skill| RolePackSkill {
                        name: skill.name,
                        description: skill.description,
                        shared: skill.shared,
                    })
                    .collect(),
                Err(error) => {
                    tracing::warn!(
                        pack = %pack_dir,
                        %role,
                        %error,
                        "role pack skills could not be listed; showing none"
                    );
                    Vec::new()
                }
            };
            let display_name = if persona.display_name.trim().is_empty() {
                persona.name.clone()
            } else {
                persona.display_name.clone()
            };
            RolePackSummary {
                role: role.to_owned(),
                display_name,
                description: persona.description.clone(),
                summary: summarize_prompt(&persona.system_prompt),
                version: Some(persona.version.clone()).filter(|version| !version.trim().is_empty()),
                origin: candidate.origin,
                pack_dir,
                pack_ref: candidate.pack_ref.clone(),
                skills,
                refusal,
                warnings: staged.warnings.clone(),
                compose_digest: Some(staged.digest.clone()),
                agents_repo: staged.agents_repo,
                archived: false,
            }
        }
        Err(error) => {
            let unreadable = format!("the pack at {pack_dir} could not be read: {error}");
            RolePackSummary {
                role: role.to_owned(),
                display_name: role.to_owned(),
                description: String::new(),
                summary: String::new(),
                version: None,
                origin: candidate.origin,
                pack_dir,
                pack_ref: candidate.pack_ref.clone(),
                skills: Vec::new(),
                refusal: refusal.or(Some(unreadable)),
                warnings: staged.warnings.clone(),
                compose_digest: Some(staged.digest.clone()),
                agents_repo: staged.agents_repo,
                archived: false,
            }
        }
    }
}

/// The first paragraph of a persona prompt: leading blank lines skipped,
/// the lines up to the next blank line joined with a space, trimmed, and cut
/// to [`SUMMARY_MAX_CHARS`] characters with an ellipsis when longer.
pub(crate) fn summarize_prompt(prompt: &str) -> String {
    let paragraph = prompt
        .lines()
        .map(str::trim)
        .skip_while(|line| line.is_empty())
        .take_while(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if paragraph.chars().count() <= SUMMARY_MAX_CHARS {
        return paragraph;
    }
    let mut cut: String = paragraph.chars().take(SUMMARY_MAX_CHARS - 1).collect();
    cut.push('…');
    cut
}

/// The project's newest kind:30624, read from the relay, in the shape the
/// host stages from — the same record the launch dialog's preview and the
/// seated create read (finding 84), so this view cannot describe a pack the
/// create would not stage.
///
/// `Ok(None)` when the project publishes none. `Err` when the relay could
/// not be read: staging refuses in that case too, and a view that fell back
/// to the local rungs would be showing packs the create would not use.
pub(crate) async fn fetch_project_pack_source(
    state: &AppState,
    project_ref: &str,
) -> Result<Option<packs_cache::ProjectPackSource>, String> {
    let filter = serde_json::json!({
        "kinds": [buzz_core_pkg::kind::KIND_PROJECT_PACK_SOURCE],
        "#d": [project_ref],
        "limit": 4,
    });
    let events = crate::relay::query_relay(state, &[filter])
        .await
        .map_err(|error| {
            format!("the project's pack source could not be read from the relay: {error}")
        })?;
    Ok(newest_project_pack_source(&events))
}

/// The newest valid kind:30624 among `events`, decoded by the shared
/// validator; records that do not decode are skipped, as the renderer's own
/// reader skips them. On equal `created_at` the first wins.
pub(crate) fn newest_project_pack_source(
    events: &[nostr::Event],
) -> Option<packs_cache::ProjectPackSource> {
    let mut newest: Option<(u64, packs_cache::ProjectPackSource)> = None;
    for event in events {
        let Ok(source) = buzz_core_pkg::project_pack_source::decode_project_pack_source(event)
        else {
            continue;
        };
        let created_at = event.created_at.as_secs();
        if newest.as_ref().is_none_or(|(at, _)| created_at > *at) {
            newest = Some((
                created_at,
                packs_cache::ProjectPackSource {
                    repo: source.repo().to_owned(),
                    git_ref: source.pin().as_ref_name().map(str::to_owned),
                    sha: source.pin().as_sha().map(str::to_owned),
                    path: source.path().to_owned(),
                },
            ));
        }
    }
    newest.map(|(_, source)| source)
}

/// Sync the project's packs repository and say where it landed, or why it
/// could not — never an error, because "could not stage" is the answer a
/// hire gets and the answer every role row carries.
pub(crate) fn project_rung(
    app: &AppHandle,
    state: &AppState,
    source: Option<packs_cache::ProjectPackSource>,
) -> ProjectRung {
    let Some(source) = source else {
        return ProjectRung::Absent;
    };
    match sync_project_packs(app, state, &source) {
        Ok((checkout, path, sha)) => ProjectRung::Synced {
            repo: source.repo,
            checkout,
            path,
            sha,
        },
        Err(reason) => ProjectRung::Unavailable { reason },
    }
}

/// The first half of [`packs_cache::stage_project_role_pack`] — validate the
/// source, land the checkout on its commit — done once for the whole
/// repository rather than once per role, so listing seven roles is one fetch
/// and not seven.
fn sync_project_packs(
    app: &AppHandle,
    state: &AppState,
    source: &packs_cache::ProjectPackSource,
) -> Result<(PathBuf, String, String), String> {
    let root = packs_cache::packs_root(app)?;
    let auth = crate::commands::project_git_exec::build_git_auth_config(state)?;
    let relay_http =
        crate::relay::relay_http_base_url(&crate::relay::relay_ws_url_with_override(state));
    let (owner, id) = packs_cache::parse_repo_coordinate(&source.repo)?;
    let path = packs_cache::validate_pack_path(&source.path)?;
    let checkout = packs_cache::packs_checkout_dir(&root, &owner, &id);
    let clone_url = packs_cache::packs_clone_url(&relay_http, &owner, &id);
    crate::commands::project_git_exec::validate_clone_url(&clone_url)?;
    let sha = packs_cache::sync_packs_checkout(&checkout, &clone_url, source, &auth)?;
    Ok((checkout, path, sha))
}

/// Everything the command does off the async runtime: take the store lock,
/// read this computer's agents and teams, sync the project rung, and walk
/// the ladder.
///
/// **The checkout rung is disabled here** (`checkout: None`) until the create
/// path passes one: today no hire supplies a checkout —
/// `desktop/src/features/coding-sessions/lib/codingSessionSeatedCreate.ts:178-183`
/// calls `stageSeat` without it, and `plan_seat_pack`
/// (`desktop/src-tauri/src/managed_agents/actor_seats.rs:549-552`) therefore
/// skips that rung — so a view that read the project's checkout would show
/// `origin: "checkout", packRef: null` for a role the hire actually stages
/// from the installed or shipped rung with a real sha. [`walk_role_pack_ladder`]
/// keeps the rung so the day the create passes a checkout, this is one line.
///
/// # Errors
/// The hire refusal, with the sync reason, when the project names a packs
/// source this computer could not stage **and** no rung on this computer holds
/// any role — otherwise the reason would vanish into an empty catalog.
pub(crate) fn list_project_role_packs_blocking(
    app: &AppHandle,
    _project_ref: Option<&str>,
    source: Option<packs_cache::ProjectPackSource>,
) -> Result<Vec<RolePackSummary>, String> {
    use tauri::Manager;
    let state = app.state::<AppState>();
    let (records, teams) = {
        let _store_guard = state
            .managed_agents_store_lock
            .lock()
            .map_err(|error| error.to_string())?;
        let records = crate::managed_agents::load_managed_agents(app)?;
        // Staging reads teams with the same tolerance (`plan_seat_pack`).
        let teams = crate::managed_agents::load_teams(app).unwrap_or_default();
        (records, teams)
    };
    let project = project_rung(app, &state, source);
    let shipped_root = packs_cache::shipped_packs_dir(app);
    let shipped_version = packs_cache::shipped_packs_version(app);
    let catalog = packs_cache::template_catalog(app);
    let packs_root = packs_cache::packs_root(app)?;
    rows_or_refusal(&RolePackLadder {
        project,
        checkout: None,
        records: &records,
        teams: &teams,
        shipped_root: shipped_root.as_deref(),
        shipped_version: &shipped_version,
        catalog: &catalog,
        packs_root: &packs_root,
    })
}

/// Walk the ladder; when the project's source could not be staged and the
/// walk found nothing at all, surface the refusal instead of an empty list.
pub(crate) fn rows_or_refusal(ladder: &RolePackLadder<'_>) -> Result<Vec<RolePackSummary>, String> {
    let rows = walk_role_pack_ladder(ladder);
    match &ladder.project {
        ProjectRung::Unavailable { reason } if rows.is_empty() => Err(project_refusal(reason)),
        _ => Ok(rows),
    }
}

#[cfg(test)]
#[path = "role_packs_view_tests.rs"]
mod tests;
