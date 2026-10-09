//! `bee packs` — where a project's persona packs live, and what this machine
//! would stage from them.
//!
//! Three verbs and no more, because there are only three questions:
//!
//! * `set-source` — publish the kind:30624 that says which repository holds
//!   this project's packs, at which commit or ref, under which path.
//! * `get-source` — read the newest one back, exactly as the relay served it.
//! * `status` — say what *this machine* would stage for a role, and be plain
//!   about the parts it cannot see from here.
//!
//! `status` is the honest one. The CLI does not clone anything and does not
//! own the host's packs cache — the desktop host does — so it reports the
//! wire's answer (repository, pin, path, the role directory that answer names)
//! and then, separately, whether a cache directory for that repository exists
//! on this disk and which role directories are in it. When there is no cache
//! it says so rather than implying the pack is missing from the repository.

use std::path::{Path, PathBuf};

use nostr::{EventBuilder, Kind, Tag};
use serde_json::{json, Value};

use beekeeper_core::kind::{KIND_GIT_REPO_ANNOUNCEMENT, KIND_PROJECT_PACK_SOURCE};
use beekeeper_core::project_pack_source::{
    build_conditional_project_pack_source, build_project_pack_source, is_root_pack_path, PackPin,
    DEFAULT_PACK_PATH, PACK_PATH_ROOT, PACK_SOURCE_WORD_CHECKOUT, PACK_SOURCE_WORD_REPOSITORY,
    PACK_SOURCE_WORD_SHIPPED,
};
use beekeeper_persona::template::TemplateCatalog;
use beekeeper_sdk::build_delete_addressable;

use crate::client::BeekeeperClient;
use crate::error::CliError;

/// How a `set-source` names the tree to stage.
pub struct PackSourcePin<'a> {
    /// `--ref refs/heads/main`.
    pub ref_name: Option<&'a str>,
    /// `--sha <40-hex>`.
    pub sha: Option<&'a str>,
}

impl PackSourcePin<'_> {
    /// Resolve the two flags into the one pin the wire allows.
    ///
    /// # Errors
    /// [`CliError::Usage`] when both or neither flag was given — the same rule
    /// the decoder enforces, stated at the point a person can still fix it.
    fn resolve(&self) -> Result<PackPin, CliError> {
        match (self.ref_name, self.sha) {
            (Some(_), Some(_)) => Err(CliError::Usage(
                "pass exactly one of --ref and --sha: two pins would let two hosts stage two \
                 different trees from the same record"
                    .into(),
            )),
            (None, None) => Err(CliError::Usage(
                "pass exactly one of --ref (e.g. refs/heads/main) or --sha <40-hex>".into(),
            )),
            (Some(ref_name), None) => Ok(PackPin::Ref(ref_name.to_string())),
            (None, Some(sha)) => Ok(PackPin::Sha(sha.to_string())),
        }
    }
}

/// `bee packs set-source` — publish the kind:30624 for a project.
///
/// # Errors
/// [`CliError::Usage`] for a malformed coordinate, pin, path or note (exit 1);
/// the relay's own refusal otherwise — missing publication authority gets
/// HTTP 403 and exit 3.
pub async fn cmd_set_source(
    client: &BeekeeperClient,
    project: &str,
    repo: &str,
    pin: &PackSourcePin<'_>,
    path: Option<&str>,
    note: Option<&str>,
) -> Result<(), CliError> {
    cmd_set_source_conditionally(
        client,
        project,
        repo,
        pin,
        path,
        note,
        PackSourceCondition::Unconditional,
    )
    .await
}

/// Whether publication must compare the current project source atomically.
#[derive(Clone, Copy)]
pub enum PackSourceCondition<'a> {
    /// Preserve legacy unconditional v1 publication.
    Unconditional,
    /// Require that no live source exists.
    IfUnset,
    /// Require the exact live source event ID.
    Expected(&'a str),
}

/// Publish one signed source with an optional relay-enforced condition.
///
/// # Errors
/// Invalid input is [`CliError::Usage`]; a named source conflict is
/// [`CliError::Conflict`] (exit 5). Auth and transport refusals remain distinct.
/// A conflict never signs or submits a replacement event.
pub async fn cmd_set_source_conditionally(
    client: &BeekeeperClient,
    project: &str,
    repo: &str,
    pin: &PackSourcePin<'_>,
    path: Option<&str>,
    note: Option<&str>,
    condition: PackSourceCondition<'_>,
) -> Result<(), CliError> {
    let pin = pin.resolve()?;
    let draft = match condition {
        PackSourceCondition::Unconditional => {
            build_project_pack_source(project, repo, &pin, path, note)
        }
        PackSourceCondition::IfUnset => {
            build_conditional_project_pack_source(project, repo, &pin, path, note, None)
        }
        PackSourceCondition::Expected(id) => {
            build_conditional_project_pack_source(project, repo, &pin, path, note, Some(id))
        }
    }
    .map_err(CliError::Usage)?;

    let tags: Vec<Tag> = draft
        .tags
        .iter()
        .map(|tag| {
            Tag::parse(tag.clone())
                .map_err(|error| CliError::Other(format!("invalid tag: {error}")))
        })
        .collect::<Result<_, _>>()?;
    let builder =
        EventBuilder::new(Kind::Custom(KIND_PROJECT_PACK_SOURCE as u16), draft.content).tags(tags);
    let event = client.sign_event(builder)?;
    let resp = client
        .submit_event(event)
        .await
        .map_err(pack_source_error)?;
    crate::client::print_create_response(&resp, "project", &draft.d_tag);
    Ok(())
}

fn pack_source_error(error: CliError) -> CliError {
    match error {
        CliError::Relay { status: 409, body }
            if body.starts_with("conflict: PACK_SOURCE_CONFLICT:") =>
        {
            CliError::Conflict(body.strip_prefix("conflict: ").unwrap_or(&body).to_owned())
        }
        other => other,
    }
}

/// `bee packs get-source` — the newest kind:30624 for a project, decoded.
///
/// Prints an array so it composes with every other read verb; an empty array
/// means the project has published no pack source, which is a real state (the
/// session checkout's own `personas/roles/` is what gets staged) and not an
/// error.
///
/// # Errors
/// [`CliError::Usage`] for a coordinate that is not a project coordinate;
/// transport and auth errors from the relay.
pub async fn cmd_get_source(client: &BeekeeperClient, project: &str) -> Result<(), CliError> {
    let coordinate = normalize_project(project)?;
    let rows = query_pack_sources(client, &coordinate).await?;
    println!(
        "{}",
        Value::Array(rows.into_iter().map(|(_, row)| row).collect())
    );
    Ok(())
}

/// `bee packs status` — what this machine would stage for a role.
///
/// # Errors
/// [`CliError::Usage`] for a malformed coordinate or role slug; transport and
/// auth errors from the relay.
pub async fn cmd_status(
    client: &BeekeeperClient,
    project: &str,
    role: Option<&str>,
    packs_dir: Option<&Path>,
    templates: Option<&Path>,
) -> Result<(), CliError> {
    let coordinate = normalize_project(project)?;
    if let Some(role) = role {
        beekeeper_core::coding_session_lifecycle_command::validate_role_slug(role)
            .map_err(|error| CliError::Usage(error.replace("action.role", "--role")))?;
    }
    let rows = query_pack_sources(client, &coordinate).await?;
    let Some((source, row)) = rows.into_iter().next() else {
        println!(
            "{}",
            json!({
                "project": coordinate,
                "source": Value::Null,
                "source_kind": PACK_SOURCE_WORD_SHIPPED,
                "would_stage": Value::Null,
                "fallback_order": [
                    PACK_SOURCE_WORD_REPOSITORY,
                    PACK_SOURCE_WORD_CHECKOUT,
                    PACK_SOURCE_WORD_SHIPPED,
                ],
                "note": "this project has published no pack source (kind 30624). A seat is \
                         staged from the session checkout's own personas/roles/<role>/ when the \
                         checkout has one, and otherwise from the app's shipped defaults; its \
                         44223 then names them as app:shipped with the app version as the sha. \
                         Run `bee packs init --project <coord>` to give the project a packs \
                         repository of its own.",
            })
        );
        return Ok(());
    };

    let (cache_root, cache_dir_source) = match packs_dir {
        Some(dir) => (Some(dir.to_path_buf()), CacheDirSource::Override),
        None => match default_packs_dir() {
            Some((dir, source)) => (Some(dir), source),
            None => (None, CacheDirSource::Default),
        },
    };
    let cache_dir = cache_root
        .as_ref()
        .zip(source.cache_dir_name())
        .map(|(root, name)| root.join(name));
    let role_dirs = cache_dir.as_ref().map(|dir| {
        let root = dir.join(source.path());
        // A flat source's roles are `roles/<role>.md` and nothing else. Its
        // root also holds `.git`, `plans/`, `roles/` and `skills/`, and
        // listing those as roles told an operator that `.git` was one
        // (2026-09-22, seen right after Beekeeper's own migration). Only a
        // pack layout keeps one directory per role at the path.
        let mut found = if is_root_pack_path(source.path()) {
            Vec::new()
        } else {
            role_directories(&root)
        };
        for flat in flat_role_files(&root) {
            if !found.contains(&flat) {
                found.push(flat);
            }
        }
        found.sort();
        found
    });

    let would_stage = match role {
        Some(role) => json!({
            "role": role,
            "path": source.role_path(role),
            "present_in_cache": role_dirs
                .as_ref()
                .map(|found| found.iter().any(|name| name == role)),
        }),
        None => Value::Null,
    };
    // What the host's composer would make of the cached role: a pure read
    // of this disk, written nowhere. `null` when there is no role or no
    // cache to compose from — a different fact from a refusal.
    let compose = match (role, cache_dir.as_ref()) {
        (Some(role), Some(dir)) if dir.is_dir() => {
            compose_status(dir, source.path(), role, templates)
        }
        _ => Value::Null,
    };

    println!(
        "{}",
        json!({
            "project": coordinate,
            "source": row,
            "source_kind": PACK_SOURCE_WORD_REPOSITORY,
            "repo": source.repo(),
            "pin": {
                "kind": source.pin().tag_name(),
                "value": source.pin().value(),
            },
            "path": source.path(),
            "would_stage": would_stage,
            // Everything below is about *this disk*, not the wire. `null`
            // means "this machine has never fetched these packs", which is a
            // different fact from "the repository has no such role".
            "cache_dir": cache_dir.as_ref().map(|dir| dir.display().to_string()),
            "cache_dir_source": cache_dir_source.as_str(),
            "cache_present": cache_dir.as_ref().map(|dir| dir.is_dir()),
            "roles_found": role_dirs,
            "compose": compose,
        })
    );
    Ok(())
}

/// Compose `role` out of the cached checkout the way the host stages it —
/// a pack directory `<path>/<role>` first, then a flat `<path>/roles/<role>.md`
/// — and report the result without writing anything.
///
/// `templates` is the catalog to resolve `![[beekeeper/…]]` against; absent,
/// the catalog is empty and such an include is a disclosed refusal, because
/// this CLI cannot see which templates the desktop build ships.
fn compose_status(cache_dir: &Path, path: &str, role: &str, templates: Option<&Path>) -> Value {
    use beekeeper_persona::compose::{compose_role, ComposeOptions, RoleSource, FLAT_ROLES_DIR};
    let mut root = cache_dir.to_path_buf();
    for segment in path
        .split('/')
        .filter(|segment| !segment.is_empty() && *segment != PACK_PATH_ROOT)
    {
        root.push(segment);
    }
    // `path: .` — the agents repository — joins to nothing.
    let prefix = if is_root_pack_path(path) {
        String::new()
    } else {
        format!("{}/", path.trim_matches('/'))
    };
    let (source, source_path, layout) = if root
        .join(role)
        .join(".plugin")
        .join("plugin.json")
        .is_file()
    {
        (
            RoleSource::Pack {
                dir: root.join(role),
                role: role.to_owned(),
                persona: None,
            },
            format!("{prefix}{role}"),
            "pack",
        )
    } else if root
        .join(FLAT_ROLES_DIR)
        .join(format!("{role}.md"))
        .is_file()
    {
        (
            RoleSource::Flat {
                root: root.clone(),
                role: role.to_owned(),
            },
            format!("{prefix}{FLAT_ROLES_DIR}/{role}"),
            "flat",
        )
    } else {
        return json!({
            "ok": false,
            "layout": Value::Null,
            "reason": format!("the cache holds neither {prefix}{role}/ nor {prefix}{FLAT_ROLES_DIR}/{role}.md"),
            "templates": templates.map(|dir| dir.display().to_string()),
        });
    };
    let catalog = match templates {
        Some(dir) => match TemplateCatalog::load(dir, "cli") {
            Ok(catalog) => catalog,
            Err(error) => {
                return json!({
                    "ok": false,
                    "layout": layout,
                    "reason": format!("template catalog at {}: {error}", dir.display()),
                    "templates": dir.display().to_string(),
                })
            }
        },
        None => TemplateCatalog::empty("cli"),
    };
    match compose_role(&source, &catalog, &ComposeOptions::local(source_path)) {
        Ok(composed) => json!({
            "ok": true,
            "layout": layout,
            "digest": composed.provenance.digest,
            "includes": composed.provenance.includes,
            "warnings": composed.provenance.warnings,
            "skills": composed.skills.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
            "agents_repo": composed.agents_repo,
            // Retired roles under roles/archive/ (spec § 4.11): listed, never hireable.
            "archived": beekeeper_persona::compose::archived_role_files(&root),
            "templates": templates.map(|dir| dir.display().to_string()),
            "note": if templates.is_none() {
                "no template catalog was given; pass --templates or set BEEKEEPER_TEMPLATES_DIR to \
                 resolve ![[beekeeper/…]] includes the way the desktop host does"
            } else {
                ""
            },
        }),
        Err(error) => json!({
            "ok": false,
            "layout": layout,
            "reason": error.to_string(),
            "templates": templates.map(|dir| dir.display().to_string()),
        }),
    }
}

/// The two layouts a role source comes in (spec § 4.8, § 4.11).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackLayout {
    /// One pack directory per role: `<path>/<role>/.plugin/plugin.json`.
    Pack,
    /// The flat team layout: `<path>/roles/<role>.md`, `<path>/team.yml` —
    /// the agents repository, at its root.
    Flat,
}

/// The suffix the project's agents repository id carries (spec § 4.11).
pub const AGENTS_REPO_SUFFIX: &str = "-beekeeper-agents";

/// The suffix a pack-layout packs repository id carries.
pub const PACKS_REPO_SUFFIX: &str = "-packs";

/// `<slug><suffix>`, keeping the suffix whole inside the 64-byte repository
/// id — a truncated suffix would collide with the project's own code
/// repository. The same rule as the desktop host's `default_packs_repo_id`.
pub fn default_repo_id(project_slug: &str, suffix: &str) -> String {
    let room = 64usize.saturating_sub(suffix.len());
    let head: String = project_slug.chars().take(room).collect();
    format!("{}{suffix}", head.trim_end_matches('-'))
}

impl PackLayout {
    /// Parse `--layout`.
    ///
    /// # Errors
    /// [`CliError::Usage`] naming the two accepted words.
    pub fn parse(word: &str) -> Result<Self, CliError> {
        match word.trim() {
            "pack" => Ok(Self::Pack),
            "flat" => Ok(Self::Flat),
            other => Err(CliError::Usage(format!(
                "--layout must be `pack` or `flat`, got {other:?}"
            ))),
        }
    }

    /// The `path` a source of this layout defaults to.
    pub fn default_path(self) -> &'static str {
        match self {
            Self::Pack => DEFAULT_PACK_PATH,
            Self::Flat => PACK_PATH_ROOT,
        }
    }

    /// The repository id suffix a source of this layout defaults to.
    pub fn default_suffix(self) -> &'static str {
        match self {
            Self::Pack => PACKS_REPO_SUFFIX,
            Self::Flat => AGENTS_REPO_SUFFIX,
        }
    }

    /// The roles a seed directory of this layout holds, sorted.
    pub fn roles_in(self, seed: &Path) -> Vec<String> {
        match self {
            Self::Pack => role_directories(seed),
            Self::Flat => flat_role_files(seed),
        }
    }
}

/// What `bee packs init` was asked to build.
pub struct PackInitRequest<'a> {
    /// Project coordinate `30621:<owner-hex>:<slug>`.
    pub project: &'a str,
    /// Repository id to announce. Defaults to `<project slug>` plus the
    /// layout's suffix.
    pub repo_id: Option<&'a str>,
    /// Directory to seed from. For the pack layout, defaults to the nearest
    /// `personas/roles` at or above the working directory; for the flat
    /// layout, `None` means "write the seed" (`beekeeper_persona::seed`).
    pub from: Option<&'a Path>,
    /// Directory inside the repository to write the packs to.
    pub path: Option<&'a str>,
    /// The layout of the seed directory, which also picks the default path.
    pub layout: PackLayout,
    /// The template catalog the flat seed references; resolved as
    /// `bee pack compose` resolves it when `None`.
    pub templates: Option<&'a Path>,
    /// The kind:30624 event id this run replaces. Without it a project that
    /// already has a source refuses; with it the new source is published
    /// conditionally on that exact event, so one that moved is a refusal.
    pub expect_source: Option<&'a str>,
    /// Print the plan and touch nothing.
    pub dry_run: bool,
}

/// `bee packs init` — announce the project's agents repository (or a packs
/// repository), seed it, and point the project at it.
///
/// The same three steps the app performs when it creates a project, in the
/// same order and against the same contract, so a team that never opens the
/// desktop app gets the identical result.
///
/// **Ordering is the safety property.** The announcement must land before the
/// push (the relay's git gate reads it), and the kind:30624 must land *after*
/// the push. The flat layout — the project's own agents repository — is
/// pinned `ref: refs/heads/main` (spec § 4.7 as amended 2026-09-18: its
/// founders are the project's owners and its push gate is the roster); the
/// pack layout keeps decision 8's immutable sha. A failure at any step stops
/// the sequence and prints what already landed, so nothing points at a
/// repository that has no roles in it.
///
/// # Errors
/// [`CliError::Usage`] for a malformed coordinate, a seed directory that does
/// not exist or holds no role directories, or a project that already has a
/// pack source (replace it with `set-source`, deliberately). Relay and git
/// failures surface with what had already landed.
pub async fn cmd_init(
    client: &BeekeeperClient,
    request: &PackInitRequest<'_>,
) -> Result<(), CliError> {
    let coordinate = normalize_project(request.project)?;
    let slug = coordinate
        .rsplit(':')
        .next()
        .unwrap_or_default()
        .to_string();
    let repo_id = match request.repo_id {
        Some(id) => id.to_string(),
        None => default_repo_id(&slug, request.layout.default_suffix()),
    };
    let path = request
        .path
        .unwrap_or(request.layout.default_path())
        .to_string();
    // The flat seed is written, not copied: one include per shipped role
    // template, resolved against this build's catalog.
    let seed_is_written = matches!((request.layout, request.from), (PackLayout::Flat, None));
    let seed = match (request.layout, request.from) {
        (PackLayout::Flat, None) => {
            let templates = crate::commands::pack::resolve_templates_dir(request.templates)
                .ok_or_else(|| {
                    CliError::Usage(format!(
                        "{}, or pass --from <dir> naming a team root to copy",
                        crate::commands::pack::no_templates_message()
                    ))
                })?;
            let catalog = TemplateCatalog::load(&templates, "cli")
                .map_err(|e| CliError::Usage(format!("template catalog: {e}")))?;
            let written = std::env::temp_dir()
                .join(format!("bee-agents-seed-{}", uuid::Uuid::new_v4().simple()));
            beekeeper_persona::seed::write_agents_repo_seed(&written, &catalog, &slug)
                .map_err(|e| CliError::Usage(format!("seed: {e}")))?;
            written
        }
        _ => resolve_seed_dir(request.from)?,
    };
    let roles = request.layout.roles_in(&seed);
    if roles.is_empty() {
        return Err(CliError::Usage(match request.layout {
            PackLayout::Pack => format!(
                "{} holds no role directories, so there is nothing to seed; pass --from <dir>",
                seed.display()
            ),
            PackLayout::Flat => format!(
                "{} holds no roles/<role>.md files, so there is nothing to seed; pass --from <dir> \
                 naming a flat team root",
                seed.display()
            ),
        }));
    }

    // A second pack source would silently re-point every seat on the
    // project. Replacing one is deliberate: either `bee packs set-source`,
    // or this command with `--expect-source <the event you read>`.
    if let Some((existing, event)) = query_pack_sources(client, &coordinate)
        .await?
        .into_iter()
        .next()
    {
        let existing_id = event
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        match request.expect_source {
            Some(expected) if expected == existing_id => {}
            Some(expected) => {
                return Err(CliError::Usage(format!(
                    "{coordinate}'s pack source is event {existing_id}, not the {expected} this \
                     run was told to replace; someone re-pointed it since you read it — read it \
                     again and decide against what is there now"
                )));
            }
            None => {
                return Err(CliError::Usage(format!(
                    "{coordinate} already has a pack source ({} at {} {}, event {existing_id}); \
                     use `bee packs set-source`, or pass --expect-source {existing_id} to replace \
                     exactly that one",
                    existing.repo(),
                    existing.pin().tag_name(),
                    existing.pin().value()
                )));
            }
        }
    } else if let Some(expected) = request.expect_source {
        return Err(CliError::Usage(format!(
            "{coordinate} has no pack source, so there is nothing to replace; drop \
             --expect-source {expected}"
        )));
    }

    let owner = client.keys().public_key().to_hex();
    let clone_url = format!("{}/git/{owner}/{repo_id}", client.relay_url());
    let repo_coordinate = format!("30617:{owner}:{repo_id}");

    if request.dry_run {
        println!(
            "{}",
            json!({
                "dry_run": true,
                "project": coordinate,
                "repo": repo_coordinate,
                "clone_url": clone_url,
                "path": path,
                "seed_dir": seed.display().to_string(),
                "roles": roles,
            })
        );
        return Ok(());
    }

    // Step 1 — announce the repository inside the project.
    let announce = crate::commands::repos::build_packs_repo_announcement(
        &repo_id,
        &match request.layout {
            PackLayout::Flat => format!("{slug} agents"),
            PackLayout::Pack => format!("{slug} packs"),
        },
        &clone_url,
        &coordinate,
    )?;
    let announce_event = client.sign_event(announce)?;
    let announce_id = announce_event.id.to_hex();
    let announce_response = client.submit_event(announce_event).await?;

    // Step 2 — seed it with one signed commit and push.
    let seeded = seed_packs_repository(&seed, &path, &clone_url);
    if seed_is_written {
        std::fs::remove_dir_all(&seed).ok();
    }
    let seeded = match seeded {
        Ok(seeded) => seeded,
        Err(error) => {
            // The announcement promised a repository with packs in it; a
            // seed or push failure after it landed breaks that promise, so
            // it is withdrawn rather than left as a stray repository with
            // nothing in it under this key. Same rule as the desktop host's
            // `project_packs_init` (LANE-L31) — see that module's docs.
            let (withdrawn_event_id, withdrawal_error) =
                withdraw_repo_announcement(client, &owner, &repo_id).await;
            // `withdraw_repo_announcement` always resolves to exactly one of
            // the two being `Some`: either the tombstone landed, or it did
            // not and its own words say why.
            let note = if withdrawn_event_id.is_some() {
                "the repository was announced but not seeded; the announcement was withdrawn \
                 because it would have pointed at an empty repository. Fix the push and re-run \
                 `bee packs init`."
            } else {
                "the repository was announced but not seeded, and withdrawing the announcement \
                 also failed — delete it by hand: `bee repos delete --id <repo-id>` (the \
                 coordinate is in `announced` above)."
            };
            eprintln!(
                "{}",
                json!({
                    "step": "seed",
                    "announced": repo_coordinate,
                    "announce_event_id": announce_id,
                    "seed_error": error.to_string(),
                    "announcement_withdrawn_event_id": withdrawn_event_id,
                    "announcement_withdrawal_error": withdrawal_error,
                    "note": note,
                })
            );
            return Err(error);
        }
    };

    // Step 3 — point the project at what landed: the branch for the
    // project's own agents repository, the exact commit for a packs one.
    let pin = match request.layout {
        PackLayout::Flat => PackPin::Ref(seeded.pushed_ref.clone()),
        PackLayout::Pack => PackPin::Sha(seeded.commit.clone()),
    };
    let note = match request.layout {
        PackLayout::Flat if request.from.is_none() => {
            "seeded by bee packs init from this build's shipped role templates".to_owned()
        }
        _ => format!("seeded by bee packs init from {}", seed.display()),
    };
    let draft = match request.expect_source {
        None => build_project_pack_source(
            &coordinate,
            &repo_coordinate,
            &pin,
            Some(&path),
            Some(&note),
        ),
        Some(expected) => build_conditional_project_pack_source(
            &coordinate,
            &repo_coordinate,
            &pin,
            Some(&path),
            Some(&note),
            Some(expected),
        ),
    }
    .map_err(CliError::Usage)?;
    let tags: Vec<Tag> = draft
        .tags
        .iter()
        .map(|tag| {
            Tag::parse(tag.clone())
                .map_err(|error| CliError::Other(format!("invalid tag: {error}")))
        })
        .collect::<Result<_, _>>()?;
    let source_event = client.sign_event(
        EventBuilder::new(Kind::Custom(KIND_PROJECT_PACK_SOURCE as u16), draft.content).tags(tags),
    )?;
    let source_id = source_event.id.to_hex();
    let source_response = client.submit_event(source_event).await?;

    println!(
        "{}",
        json!({
            "project": coordinate,
            "repo": repo_coordinate,
            "clone_url": clone_url,
            "path": path,
            "roles": roles,
            "announce_event_id": announce_id,
            "announce_response": crate::client::extract_relay_response_field(&announce_response, "message"),
            "commit": seeded.commit,
            "pushed_ref": seeded.pushed_ref,
            "pack_source_event_id": source_id,
            "pack_source_response": crate::client::extract_relay_response_field(&source_response, "message"),
        })
    );
    Ok(())
}

/// Withdraw a `30617:<owner>:<repo_id>` announcement this process just
/// published, because the seed or push that was supposed to fill it failed —
/// see [`cmd_init`]'s rollback step. The same kind:5 shape `bee repos delete`
/// publishes (`crate::commands::repos::cmd_delete_repo`).
///
/// Returns `(withdrawn_event_id, error)`, exactly one `Some`: a failed
/// withdrawal is reported, not retried, and `cmd_init` already has the
/// coordinate to hand the operator for a manual `bee repos delete`.
pub(crate) async fn withdraw_repo_announcement(
    client: &BeekeeperClient,
    owner: &str,
    repo_id: &str,
) -> (Option<String>, Option<String>) {
    let builder = match build_delete_addressable(KIND_GIT_REPO_ANNOUNCEMENT, owner, repo_id) {
        Ok(builder) => builder,
        Err(error) => {
            return (
                None,
                Some(format!("could not build the withdrawal: {error}")),
            )
        }
    };
    let event = match client.sign_event(builder) {
        Ok(event) => event,
        Err(error) => {
            return (
                None,
                Some(format!("could not sign the withdrawal: {error}")),
            )
        }
    };
    let event_id = event.id.to_hex();
    match client.submit_event(event).await {
        Ok(_) => (Some(event_id), None),
        Err(error) => (None, Some(error.to_string())),
    }
}

/// What the seed step produced.
pub(crate) struct SeededPacks {
    /// The commit that was pushed.
    pub(crate) commit: String,
    /// The ref it was pushed to.
    pub(crate) pushed_ref: String,
}

/// Build one signed commit holding the packs and push it.
///
/// **LANE-L23 finalizer note:** the addendum asks for one implementation shared
/// with the desktop host's *Create packs repository*. A Tauri command is not
/// reachable from this process, so these steps are written here against the
/// same contract (announce → seed → 30624, pinned to the pushed sha). When
/// Lane B's host command exists, this body should call it and the duplication
/// should go.
///
/// `git` is invoked as a subprocess, as `bee git setup`'s own checks do, so the
/// credential helper the relay requires is the one git already knows about.
pub(crate) fn seed_packs_repository(
    seed: &Path,
    path: &str,
    clone_url: &str,
) -> Result<SeededPacks, CliError> {
    let work =
        std::env::temp_dir().join(format!("bee-packs-init-{}", uuid::Uuid::new_v4().simple()));
    // The work directory first, on its own: `create_dir_all("<work>/.")` for a
    // `<work>` that does not exist yet fails on macOS, which is exactly the
    // flat layout's `path: "."` (found live 2026-09-21).
    std::fs::create_dir_all(&work).map_err(|error| {
        CliError::Other(format!("could not create {}: {error}", work.display()))
    })?;
    let target = if beekeeper_core::project_pack_source::is_root_pack_path(path) {
        work.clone()
    } else {
        work.join(path)
    };
    std::fs::create_dir_all(&target).map_err(|error| {
        CliError::Other(format!("could not create {}: {error}", target.display()))
    })?;
    copy_tree(seed, &target)?;

    // `git_command` rather than a bare `Command::new("git")`: git exports
    // `GIT_DIR` and its six siblings to hooks, and this crate's own tests run
    // inside the pre-push gate. With those inherited, `git init` in a temp
    // directory writes to the *pushing* repository's config and races its
    // lock. One helper, one list — see its doc in `sessions::worktree`.
    let git = |args: &[&str]| -> Result<String, CliError> {
        let output = crate::commands::sessions::worktree::git_command(&work)
            .args(args)
            .output()
            .map_err(|error| CliError::Other(format!("could not run git {args:?}: {error}")))?;
        if !output.status.success() {
            return Err(CliError::Other(format!(
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    };

    git(&["init", "--quiet", "--initial-branch=main"])?;
    git(&["add", "--all"])?;
    // `-s` because the DCO gate fails any commit without the trailer, and a
    // seeded repository is a repository people will later push to.
    git(&[
        "-c",
        "user.name=bee packs init",
        "-c",
        "user.email=packs@beekeeper.local",
        "commit",
        "--quiet",
        "-s",
        "-m",
        "feat(packs): seed the project's role packs",
    ])?;
    let commit = git(&["rev-parse", "HEAD"])?;
    git(&["push", "--quiet", clone_url, "HEAD:refs/heads/main"]).map_err(|error| {
        CliError::Other(format!(
            "{error}\nIf this is an authentication failure, run `just install-git-credentials` \
             (or `bee git setup`) so git can answer the relay's NIP-98 challenge."
        ))
    })?;
    std::fs::remove_dir_all(&work).ok();
    Ok(SeededPacks {
        commit,
        pushed_ref: "refs/heads/main".to_string(),
    })
}

/// Copy a directory tree, files and directories only.
fn copy_tree(from: &Path, to: &Path) -> Result<(), CliError> {
    let entries = std::fs::read_dir(from)
        .map_err(|error| CliError::Other(format!("could not read {}: {error}", from.display())))?;
    for entry in entries.flatten() {
        let source = entry.path();
        let destination = to.join(entry.file_name());
        if source.is_dir() {
            std::fs::create_dir_all(&destination).map_err(|error| {
                CliError::Other(format!(
                    "could not create {}: {error}",
                    destination.display()
                ))
            })?;
            copy_tree(&source, &destination)?;
        } else if source.is_file() {
            std::fs::copy(&source, &destination).map_err(|error| {
                CliError::Other(format!("could not copy {}: {error}", source.display()))
            })?;
        }
    }
    Ok(())
}

/// The directory of role packs to seed from.
///
/// `--from` when given; otherwise the nearest `personas/roles` at or above the
/// working directory. Refused rather than guessed when neither exists: seeding
/// a packs repository from nothing would publish a source pointing at empty
/// roles.
fn resolve_seed_dir(from: Option<&Path>) -> Result<PathBuf, CliError> {
    if let Some(from) = from {
        if !from.is_dir() {
            return Err(CliError::Usage(format!(
                "--from {} is not a directory",
                from.display()
            )));
        }
        return Ok(from.to_path_buf());
    }
    let mut cursor = std::env::current_dir().map_err(|error| {
        CliError::Other(format!("could not read the working directory: {error}"))
    })?;
    loop {
        let candidate = cursor.join(DEFAULT_PACK_PATH);
        if candidate.is_dir() {
            return Ok(candidate);
        }
        if !cursor.pop() {
            return Err(CliError::Usage(format!(
                "no {DEFAULT_PACK_PATH} directory at or above the working directory; pass \
                 --from <dir> naming the role packs to seed"
            )));
        }
    }
}

/// Query the relay for a project's pack sources, newest first, decoded.
///
/// Rows that do not decode are dropped rather than printed: the relay refuses
/// them at ingest, so one on a read is either a pre-gate record or a bug, and
/// either way a client must not act on a record it cannot fully read.
pub(crate) async fn query_pack_sources(
    client: &BeekeeperClient,
    coordinate: &str,
) -> Result<
    Vec<(
        beekeeper_core::project_pack_source::ProjectPackSource,
        Value,
    )>,
    CliError,
> {
    let filter = json!({
        "kinds": [KIND_PROJECT_PACK_SOURCE],
        "#d": [coordinate],
        "limit": 8,
    });
    let resp = client.query(&filter).await?;
    let rows: Vec<Value> = serde_json::from_str(&resp)
        .map_err(|error| CliError::Other(format!("failed to parse relay response: {error}")))?;
    let mut decoded = Vec::new();
    for row in rows {
        let Ok(event) = serde_json::from_value::<nostr::Event>(row.clone()) else {
            continue;
        };
        if let Ok(source) = beekeeper_core::project_pack_source::decode_project_pack_source(&event)
        {
            decoded.push((source, row));
        }
    }
    Ok(decoded)
}

/// The role slugs named by flat role files `<root>/roles/<role>.md`, sorted.
fn flat_role_files(root: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(root.join("roles")) else {
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
        .collect();
    names.sort();
    names
}

/// The role directories inside a staged pack path, sorted, or an empty list.
fn role_directories(path: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(path) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();
    names.sort();
    names
}

/// How [`default_packs_dir`] picked the app identifier it used, so a caller
/// never mistakes a guess for a read fact (finding 135(e)): a dev bundle's
/// cache lives under a `.dev`-suffixed identifier, and reporting the release
/// identifier's path as `cache_dir` with no qualifier reads as "the cache is
/// empty" when it is really "this CLI guessed the wrong app".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CacheDirSource {
    /// `--packs-dir` named the cache root outright; nothing was derived.
    Override,
    /// `BEEKEEPER_MANAGED_AGENT` named the running app instance. The desktop host
    /// stamps this on every process it spawns for a seat
    /// (`desktop/src-tauri/src/managed_agents/runtime/process.rs`,
    /// `current_instance_id`/`beekeeper_marker_entry`), so a `bee` invoked from
    /// inside a seat — or by a caller that exported the same value by hand —
    /// reads its own host's cache rather than the release default.
    Env,
    /// Neither of the above applied. This is a guess at the release
    /// identifier (`io.agiterra.beekeeper.app`) and is wrong for a dev
    /// bundle or any other instance identity — say so, never report it as a
    /// confirmed path.
    Default,
}

impl CacheDirSource {
    fn as_str(self) -> &'static str {
        match self {
            CacheDirSource::Override => "override",
            CacheDirSource::Env => "env",
            CacheDirSource::Default => "default",
        }
    }
}

/// The app identifier `default_packs_dir` should use, and which fact it came
/// from. Prefers `BEEKEEPER_MANAGED_AGENT` — a fact the host already provides for
/// its own spawned processes — over the hard-coded release identifier, which
/// is only ever a guess.
fn resolve_app_identifier() -> (String, CacheDirSource) {
    if let Ok(value) = std::env::var("BEEKEEPER_MANAGED_AGENT") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return (trimmed.to_string(), CacheDirSource::Env);
        }
    }
    (APP_IDENTIFIER.to_string(), CacheDirSource::Default)
}

/// The packs cache this machine's desktop host uses, when it can be derived,
/// and which fact named the app instance it belongs to.
///
/// Derived from the platform data directory rather than asked of the host: the
/// CLI runs where no host may be running at all, and a `null` here is honest
/// while a guessed absolute path would not be.
///
/// The directory name is the app's bundle identifier
/// (`desktop/src-tauri/tauri.conf.json` `identifier`), because the host writes
/// its cache under Tauri's `app_data_dir()`, which is
/// `<platform data dir>/<identifier>` — not the product name. A **dev** build
/// of the desktop app uses the `.dev` suffixed identifier
/// (`io.agiterra.beekeeper.app.dev`) and therefore a *different* packs cache.
/// When `BEEKEEPER_MANAGED_AGENT` names the running instance (set for every
/// process the host spawns for a seat) that identifier is used instead of the
/// release default; otherwise point `--packs-dir` at the dev cache by hand.
fn default_packs_dir() -> Option<(PathBuf, CacheDirSource)> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    let (identifier, source) = resolve_app_identifier();
    #[cfg(target_os = "macos")]
    let base = home.join("Library/Application Support").join(&identifier);
    #[cfg(not(target_os = "macos"))]
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/share"))
        .join(&identifier);
    Some((base.join("packs"), source))
}

/// The desktop app's bundle identifier — the directory Tauri's `app_data_dir()`
/// resolves to, and so the parent of the host's packs cache. Used only when
/// [`resolve_app_identifier`] has no better fact (`CacheDirSource::Default`).
const APP_IDENTIFIER: &str = "io.agiterra.beekeeper.app";

/// Normalize and validate a project coordinate argument.
fn normalize_project(value: &str) -> Result<String, CliError> {
    beekeeper_core::kind::normalize_project_coordinate(value.trim()).ok_or_else(|| {
        CliError::Usage(format!(
            "--project must be a project coordinate 30621:<64-hex>:<slug> (got {value:?})"
        ))
    })
}

#[cfg(test)]
#[path = "packs_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "packs_source_tests.rs"]
mod source_tests;
