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

use buzz_core::kind::KIND_PROJECT_PACK_SOURCE;
use buzz_core::project_pack_source::{
    build_project_pack_source, PackPin, DEFAULT_PACK_PATH, PACK_SOURCE_WORD_CHECKOUT,
    PACK_SOURCE_WORD_REPOSITORY, PACK_SOURCE_WORD_SHIPPED,
};

use crate::client::BuzzClient;
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
/// the relay's own refusal otherwise — a non-founder gets HTTP 403 and exit 3
/// with the sentence naming what was searched.
pub async fn cmd_set_source(
    client: &BuzzClient,
    project: &str,
    repo: &str,
    pin: &PackSourcePin<'_>,
    path: Option<&str>,
    note: Option<&str>,
) -> Result<(), CliError> {
    let pin = pin.resolve()?;
    let draft =
        build_project_pack_source(project, repo, &pin, path, note).map_err(CliError::Usage)?;

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
    let resp = client.submit_event(event).await?;
    crate::client::print_create_response(&resp, "project", &draft.d_tag);
    Ok(())
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
pub async fn cmd_get_source(client: &BuzzClient, project: &str) -> Result<(), CliError> {
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
    client: &BuzzClient,
    project: &str,
    role: Option<&str>,
    packs_dir: Option<&Path>,
) -> Result<(), CliError> {
    let coordinate = normalize_project(project)?;
    if let Some(role) = role {
        buzz_core::coding_session_lifecycle_command::validate_role_slug(role)
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

    let cache_root = packs_dir.map(Path::to_path_buf).or_else(default_packs_dir);
    let cache_dir = cache_root
        .as_ref()
        .zip(source.cache_dir_name())
        .map(|(root, name)| root.join(name));
    let role_dirs = cache_dir
        .as_ref()
        .map(|dir| role_directories(&dir.join(source.path())));

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
            "cache_present": cache_dir.as_ref().map(|dir| dir.is_dir()),
            "roles_found": role_dirs,
        })
    );
    Ok(())
}

/// What `bee packs init` was asked to build.
pub struct PackInitRequest<'a> {
    /// Project coordinate `30621:<owner-hex>:<slug>`.
    pub project: &'a str,
    /// Repository id to announce. Defaults to `<project slug>-packs`.
    pub repo_id: Option<&'a str>,
    /// Directory of role packs to seed from. Defaults to the nearest
    /// `personas/roles` at or above the working directory.
    pub from: Option<&'a Path>,
    /// Directory inside the repository to write the packs to.
    pub path: Option<&'a str>,
    /// Print the plan and touch nothing.
    pub dry_run: bool,
}

/// `bee packs init` — announce a packs repository, seed it, and point the
/// project at the commit that landed.
///
/// The same three steps the app's **Create packs repository** performs, in the
/// same order and against the same contract, so a team that never opens the
/// desktop app gets the identical result.
///
/// **Ordering is the safety property.** The announcement must land before the
/// push (the relay's git gate reads it), and the kind:30624 must land *after*
/// the push, pinned to the sha that actually arrived. A failure at any step
/// stops the sequence and prints what already landed, so nothing points at a
/// repository that has no packs in it.
///
/// # Errors
/// [`CliError::Usage`] for a malformed coordinate, a seed directory that does
/// not exist or holds no role directories, or a project that already has a
/// pack source (replace it with `set-source`, deliberately). Relay and git
/// failures surface with what had already landed.
pub async fn cmd_init(client: &BuzzClient, request: &PackInitRequest<'_>) -> Result<(), CliError> {
    let coordinate = normalize_project(request.project)?;
    let slug = coordinate
        .rsplit(':')
        .next()
        .unwrap_or_default()
        .to_string();
    let repo_id = match request.repo_id {
        Some(id) => id.to_string(),
        None => format!("{slug}-packs"),
    };
    let path = request.path.unwrap_or(DEFAULT_PACK_PATH).to_string();
    let seed = resolve_seed_dir(request.from)?;
    let roles = role_directories(&seed);
    if roles.is_empty() {
        return Err(CliError::Usage(format!(
            "{} holds no role directories, so there is nothing to seed; pass --from <dir>",
            seed.display()
        )));
    }

    // A second pack source would silently re-point every seat on the project.
    // Replacing one is a deliberate `set-source`, never a side effect of init.
    if let Some((existing, _)) = query_pack_sources(client, &coordinate)
        .await?
        .into_iter()
        .next()
    {
        return Err(CliError::Usage(format!(
            "{coordinate} already has a pack source ({} at {} {}); use `bee packs set-source` to \
             replace it deliberately",
            existing.repo(),
            existing.pin().tag_name(),
            existing.pin().value()
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
        &format!("{slug} packs"),
        &clone_url,
        &coordinate,
    )?;
    let announce_event = client.sign_event(announce)?;
    let announce_id = announce_event.id.to_hex();
    let announce_response = client.submit_event(announce_event).await?;

    // Step 2 — seed it with one signed commit and push.
    let seeded = match seed_packs_repository(&seed, &path, &clone_url) {
        Ok(seeded) => seeded,
        Err(error) => {
            eprintln!(
                "{}",
                json!({
                    "step": "seed",
                    "announced": repo_coordinate,
                    "announce_event_id": announce_id,
                    "note": "the repository was announced but not seeded, and no pack source was \
                             published — the project still stages the shipped defaults. Fix the \
                             push and re-run `bee packs init`.",
                })
            );
            return Err(error);
        }
    };

    // Step 3 — point the project at the commit that actually landed.
    let draft = build_project_pack_source(
        &coordinate,
        &repo_coordinate,
        &PackPin::Sha(seeded.commit.clone()),
        Some(&path),
        Some(&format!("seeded by bee packs init from {}", seed.display())),
    )
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

/// What the seed step produced.
struct SeededPacks {
    /// The commit that was pushed.
    commit: String,
    /// The ref it was pushed to.
    pushed_ref: String,
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
fn seed_packs_repository(
    seed: &Path,
    path: &str,
    clone_url: &str,
) -> Result<SeededPacks, CliError> {
    let work =
        std::env::temp_dir().join(format!("bee-packs-init-{}", uuid::Uuid::new_v4().simple()));
    let target = work.join(path);
    std::fs::create_dir_all(&target).map_err(|error| {
        CliError::Other(format!("could not create {}: {error}", target.display()))
    })?;
    copy_tree(seed, &target)?;

    let git = |args: &[&str]| -> Result<String, CliError> {
        let output = std::process::Command::new("git")
            .current_dir(&work)
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
async fn query_pack_sources(
    client: &BuzzClient,
    coordinate: &str,
) -> Result<Vec<(buzz_core::project_pack_source::ProjectPackSource, Value)>, CliError> {
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
        if let Ok(source) = buzz_core::project_pack_source::decode_project_pack_source(&event) {
            decoded.push((source, row));
        }
    }
    Ok(decoded)
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

/// The packs cache this machine's desktop host uses, when it can be derived.
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
/// (`io.agiterra.beekeeper.app.dev`) and therefore a *different* packs cache;
/// point `--packs-dir` at it when reading a dev host's cache.
fn default_packs_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    #[cfg(target_os = "macos")]
    let base = home
        .join("Library/Application Support")
        .join(APP_IDENTIFIER);
    #[cfg(not(target_os = "macos"))]
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/share"))
        .join(APP_IDENTIFIER);
    Some(base.join("packs"))
}

/// The desktop app's bundle identifier — the directory Tauri's `app_data_dir()`
/// resolves to, and so the parent of the host's packs cache.
const APP_IDENTIFIER: &str = "io.agiterra.beekeeper.app";

/// Normalize and validate a project coordinate argument.
fn normalize_project(value: &str) -> Result<String, CliError> {
    buzz_core::kind::normalize_project_coordinate(value.trim()).ok_or_else(|| {
        CliError::Usage(format!(
            "--project must be a project coordinate 30621:<64-hex>:<slug> (got {value:?})"
        ))
    })
}

#[cfg(test)]
#[path = "packs_tests.rs"]
mod tests;
