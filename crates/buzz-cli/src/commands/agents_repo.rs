//! `bee agents-repo` — read a project's agents repository, propose changes
//! to it as shared drafts (NIP-AD, kind 44250), and commit them to `main`.
//!
//! Reads come from two places and say which: the repository's `main` tip,
//! through the relay's `tree` and `raw` routes (no clone), and the draft
//! log, fetched by the project's coordinate and folded with
//! `buzz_core::agents_repo_draft_fold` — the same fold Desktop and Mobile
//! bind to through `conformance/agents-repo-draft-fold/`.
//!
//! A draft is not a commit. `draft put` publishes the whole new text of one
//! file with the blob it was based on and the draft head it was edited
//! from; a save whose `prev` is no longer the head is refused before it is
//! signed (exit 5), never silently layered. `commit` fetches `main`, checks
//! every head's base against it, builds the tree, validates it with the
//! composer, pushes under a lease and publishes a `commit.record` so every
//! reader closes those drafts. `main` of the agents repository stays the
//! only thing a seat stages from.

use std::collections::HashMap;
use std::path::Path;

use buzz_core::agents_repo_draft::{
    archive_counterpart, validate_draft_path, AgentsRepoDraftOp, AgentsRepoDraftOpValue, DraftBase,
    DraftPathClass, MAX_AGENTS_REPO_DRAFT_TEXT_BYTES,
};
use buzz_core::agents_repo_draft_fold::{
    fold_agents_repo_drafts, AgentsRepoDraftDigest, DraftFoldEvent, DraftRow,
};
use buzz_core::kind::KIND_AGENTS_REPO_DRAFT_OP;
use buzz_core::project_pack_source::{PackPin, ProjectPackSource};
use nostr::Timestamp;
use serde_json::{json, Value};

use super::agents_repo_git::{commit_drafts, CommitOutcome, CommitRequest, DraftChange, Identity};
use super::pulse::resolve_project;
use super::repos::next_replaceable_created_at;
use crate::client::BuzzClient;
use crate::error::CliError;

/// The relay's ingest window is ±900 s; a bump that would land past this
/// margin is refused here rather than by the relay.
const MAX_FUTURE_SKEW_SECS: u64 = 890;

/// Everything a command needs about the project's agents repository and
/// its open drafts.
struct Snapshot {
    coordinate: String,
    source: ProjectPackSource,
    repo_owner: String,
    repo_id: String,
    clone_url: String,
    digest: AgentsRepoDraftDigest,
    /// Latest `created_at` per path across every decoded op, for the write
    /// bump; `""` is the record target.
    latest: HashMap<String, u64>,
    truncated: bool,
}

impl Snapshot {
    fn repo_coordinate(&self) -> &str {
        self.source.repo()
    }

    fn head(&self, path: &str) -> Option<&DraftRow> {
        self.digest
            .paths
            .iter()
            .find(|entry| entry.path == path)
            .map(|entry| &entry.head)
    }

    /// Where a seat stages from today: a branch, or a sha the source pins.
    fn pin_note(&self) -> Option<String> {
        match self.source.pin() {
            PackPin::Ref(_) => None,
            PackPin::Sha(sha) => Some(format!(
                "the project's source is pinned to {}; seats stage nothing new until the pin moves (`bee packs set-source`)",
                &sha[..8.min(sha.len())]
            )),
        }
    }
}

async fn snapshot(client: &BuzzClient, project: Option<&str>) -> Result<Snapshot, CliError> {
    let coordinate = resolve_project(client, project).await?;
    let sources = super::packs::query_pack_sources(client, &coordinate).await?;
    let Some((source, _)) = sources.into_iter().next() else {
        return Err(CliError::NotFound(format!(
            "{coordinate} has no agents repository (no kind:30624 source); create one with \
             `bee packs init --project {coordinate}` or Finish repository setup in the app"
        )));
    };
    if !buzz_core::project_pack_source::is_root_pack_path(source.path()) {
        return Err(CliError::Usage(format!(
            "{coordinate}'s source is a pack-layout repository ({} at path {:?}), not an agents \
             repository; drafts need the flat layout at the repository root",
            source.repo(),
            source.path()
        )));
    }
    let mut parts = source.repo().splitn(3, ':');
    let _kind = parts.next();
    let repo_owner = parts.next().unwrap_or_default().to_owned();
    let repo_id = parts.next().unwrap_or_default().to_owned();
    let clone_url = format!("{}/git/{repo_owner}/{repo_id}", client.relay_url());

    let raw = client
        .query_all(json!({ "kinds": [KIND_AGENTS_REPO_DRAFT_OP], "#a": [coordinate] }))
        .await?;
    let truncated = raw.len() >= 1000;
    let mut events = Vec::with_capacity(raw.len());
    let mut latest = HashMap::new();
    for value in raw {
        let Ok(event) = serde_json::from_value::<DraftFoldEvent>(value) else {
            continue;
        };
        let repo_tag = event
            .tags
            .iter()
            .find(|t| t.first().map(String::as_str) == Some("ad-repo"))
            .and_then(|t| t.get(1))
            .cloned()
            .unwrap_or_default();
        if let Ok(op) =
            buzz_core::agents_repo_draft::decode_agents_repo_draft_op(&event.content, &repo_tag)
        {
            let targets: Vec<String> = match op.path() {
                Some(_) => op.paths().into_iter().map(str::to_owned).collect(),
                None => vec![String::new()],
            };
            for target in targets {
                let slot = latest.entry(target).or_insert(0);
                *slot = (*slot).max(event.created_at);
            }
        }
        events.push(event);
    }
    let digest = fold_agents_repo_drafts(&coordinate, source.repo(), &events);
    Ok(Snapshot {
        coordinate,
        source,
        repo_owner,
        repo_id,
        clone_url,
        digest,
        latest,
        truncated,
    })
}

/// Publish one op, stamped past the latest op on its path.
async fn publish(
    snap: &mut Snapshot,
    client: &BuzzClient,
    op: &AgentsRepoDraftOp,
) -> Result<Value, CliError> {
    let now = Timestamp::now().as_secs();
    let targets: Vec<String> = match op.path() {
        Some(_) => op.paths().into_iter().map(str::to_owned).collect(),
        None => vec![String::new()],
    };
    let head = targets
        .iter()
        .filter_map(|t| snap.latest.get(t).copied())
        .max()
        .unwrap_or(0);
    let created_at = next_replaceable_created_at(head, now)
        .ok_or_else(|| CliError::Other("draft timestamp cannot be advanced".into()))?;
    if created_at > now + MAX_FUTURE_SKEW_SECS {
        return Err(CliError::Other(format!(
            "the latest op on this path is stamped {} s in the future; retry later rather than \
             publishing outside the relay's window",
            head.saturating_sub(now)
        )));
    }
    let builder = buzz_sdk::builders::build_agents_repo_draft_op(&snap.coordinate, op)
        .map_err(crate::validate::sdk_err)?
        .custom_created_at(Timestamp::from(created_at));
    // Signed verbatim: the tag grammar is a closed key set, so the NIP-OA
    // `auth` tag `sign_event` injects would be rejected at ingest. Membership
    // delegation still travels as the `x-auth-tag` header.
    let event = client.sign_event_unchecked(builder)?;
    let event_id = event.id.to_hex();
    let raw = client.submit_event(event).await?;
    let normalized = super::parse_write_response(&raw, "draft op was superseded")?;
    for target in targets {
        snap.latest.insert(target, created_at);
    }
    let mut response: Value = serde_json::from_str(&normalized).unwrap_or(Value::Null);
    if let Some(object) = response.as_object_mut() {
        object.entry("event_id").or_insert(json!(event_id));
        object.insert("op".into(), json!(op.kind().as_str()));
        if let Some(path) = op.path() {
            object.insert("path".into(), json!(path));
        }
        object.insert("created_at".into(), json!(created_at));
        if let Some(note) = snap.pin_note() {
            object.insert("note".into(), json!(note));
        }
    }
    Ok(response)
}

/// A file as `main` has it.
struct TipFile {
    text: String,
    blob: String,
    commit: String,
}

/// Read `path` at `main`'s tip through the relay; `Ok(None)` when `main`
/// has no such file.
async fn read_tip(
    snap: &Snapshot,
    client: &BuzzClient,
    path: &str,
) -> Result<Option<TipFile>, CliError> {
    let tail = format!("raw/refs/heads/main/{path}");
    match client
        .get_git_read(&snap.repo_owner, &snap.repo_id, &tail)
        .await
    {
        Ok(response) => {
            let text = String::from_utf8(response.bytes).map_err(|_| {
                CliError::Usage(format!(
                    "{path} on main is not UTF-8 text; edit it with git"
                ))
            })?;
            Ok(Some(TipFile {
                text,
                blob: response.blob.unwrap_or_default(),
                commit: response.commit.unwrap_or_default(),
            }))
        }
        Err(CliError::Relay { status: 404, body }) if body.contains("path not found") => Ok(None),
        Err(error) => Err(error),
    }
}

fn row_json(row: &DraftRow) -> Value {
    json!({
        "id": row.id,
        "author": row.author,
        "created_at": row.created_at,
        "op": row.op,
        "path": row.path,
        "to": row.to,
        "base": row.base,
        "base_commit": row.base_commit,
        "prev": row.prev,
        "message": row.message,
        "bytes": row.text.as_ref().map(String::len),
    })
}

fn honesty(snap: &Snapshot, out: &mut Value) {
    if snap.digest.ignored > 0 {
        out["ignored"] = json!(snap.digest.ignored);
    }
    if snap.digest.other_repo > 0 {
        out["other_repo"] = json!(snap.digest.other_repo);
        out["other_repo_note"] =
            json!("drafts for a repository this project no longer pins; they are kept, not folded");
    }
    if snap.truncated {
        out["truncated"] = json!(true);
    }
    if let Some(note) = snap.pin_note() {
        out["note"] = json!(note);
    }
}

async fn cmd_ls(client: &BuzzClient, project: Option<&str>) -> Result<(), CliError> {
    let snap = snapshot(client, project).await?;
    let listing = client
        .get_git_read(&snap.repo_owner, &snap.repo_id, "tree/refs/heads/main")
        .await?;
    let tree: Value = serde_json::from_slice(&listing.bytes)
        .map_err(|error| CliError::Other(format!("tree listing is not JSON: {error}")))?;
    let mut files: Vec<Value> = tree["entries"]
        .as_array()
        .map(|entries| {
            entries
                .iter()
                .filter(|entry| entry["kind"] == "blob")
                .map(|entry| {
                    let path = entry["path"].as_str().unwrap_or_default();
                    let mut row = json!({
                        "path": path,
                        "blob": entry["oid"],
                        "size": entry["size"],
                    });
                    if let Some(head) = snap.head(path) {
                        row["draft"] =
                            json!({ "id": head.id, "author": head.author, "op": head.op });
                    }
                    row
                })
                .collect()
        })
        .unwrap_or_default();
    // Drafts for paths not on main yet.
    for entry in &snap.digest.paths {
        if !files.iter().any(|f| f["path"] == entry.path) {
            files.push(json!({
                "path": entry.path,
                "blob": null,
                "size": null,
                "draft": { "id": entry.head.id, "author": entry.head.author, "op": entry.head.op },
                "not_on_main": true,
            }));
        }
    }
    let mut out = json!({
        "project": snap.coordinate,
        "repo": snap.repo_coordinate(),
        "commit": tree["commit"],
        "files": files,
    });
    honesty(&snap, &mut out);
    println!("{out}");
    Ok(())
}

async fn cmd_show(
    client: &BuzzClient,
    project: Option<&str>,
    path: &str,
    draft: bool,
) -> Result<(), CliError> {
    validate_draft_path(path).map_err(CliError::Usage)?;
    let snap = snapshot(client, project).await?;
    let tip = read_tip(&snap, client, path).await?;
    let head = snap.head(path);
    let (source, text) = match (draft, head) {
        (true, Some(row)) => match row.op.as_str() {
            "file.put" => ("draft", row.text.clone()),
            "file.move" if row.to.as_deref() == Some(path) => {
                ("draft (moved here)", tip.as_ref().map(|t| t.text.clone()))
            }
            _ => ("draft (removed)", None),
        },
        (true, None) => {
            return Err(CliError::NotFound(format!("{path} has no open draft")));
        }
        (false, _) => ("main", tip.as_ref().map(|t| t.text.clone())),
    };
    let Some(text) = text else {
        return Err(CliError::NotFound(format!(
            "{path} is not on main{}",
            if head.is_some() {
                " (an open draft names it)"
            } else {
                ""
            }
        )));
    };
    let mut out = json!({
        "path": path,
        "source": source,
        "text": text,
        "main": tip.as_ref().map(|t| json!({ "blob": t.blob, "commit": t.commit })),
        "draft": head.map(row_json),
    });
    honesty(&snap, &mut out);
    println!("{out}");
    Ok(())
}

async fn cmd_drafts(
    client: &BuzzClient,
    project: Option<&str>,
    path: Option<&str>,
    all: bool,
) -> Result<(), CliError> {
    let snap = snapshot(client, project).await?;
    let paths: Vec<Value> = snap
        .digest
        .paths
        .iter()
        .filter(|entry| path.is_none_or(|p| p == entry.path))
        .map(|entry| {
            let mut row = json!({
                "path": entry.path,
                "head": row_json(&entry.head),
                "diverged": entry.diverged,
                "superseded": entry.superseded.len(),
                "updated_at": entry.updated_at,
            });
            if all {
                row["superseded"] =
                    json!(entry.superseded.iter().map(row_json).collect::<Vec<_>>());
            }
            row
        })
        .collect();
    let commits: Vec<Value> = snap
        .digest
        .commits
        .iter()
        .take(if all { usize::MAX } else { 5 })
        .map(|record| {
            json!({
                "id": record.id,
                "commit": record.commit,
                "by": record.by,
                "created_at": record.created_at,
                "paths": record.paths,
                "drafts": record.drafts.len(),
                "message": record.message,
            })
        })
        .collect();
    let mut out = json!({
        "project": snap.coordinate,
        "repo": snap.repo_coordinate(),
        "open": paths,
        "commits": commits,
    });
    honesty(&snap, &mut out);
    println!("{out}");
    Ok(())
}

/// The `prev` a save must carry: the head, unless the caller named one.
fn resolve_prev(
    snap: &Snapshot,
    path: &str,
    named: Option<&str>,
) -> Result<Option<String>, CliError> {
    let head = snap.head(path);
    match (named, head) {
        (Some(named), Some(head)) if head.id == named || (named.len() >= 8 && head.id.starts_with(named)) => {
            Ok(Some(head.id.clone()))
        }
        (Some(named), Some(head)) => Err(CliError::Conflict(format!(
            "{path}: the head draft is {} by {}, not {named}; read it (`bee agents-repo show {path} --draft`) and save again with --prev {}",
            &head.id[..12],
            &head.author[..8],
            &head.id[..12]
        ))),
        (Some(named), None) => Err(CliError::Conflict(format!(
            "{path}: no open draft matches {named}; it was committed or withdrawn — save without --prev"
        ))),
        (None, Some(head)) => Err(CliError::Conflict(format!(
            "{path}: {} saved a newer draft ({}); read it (`bee agents-repo show {path} --draft`) and save again with --prev {}",
            &head.author[..8],
            &head.id[..12],
            &head.id[..12]
        ))),
        (None, None) => Ok(None),
    }
}

fn read_text_arg(file: Option<&Path>) -> Result<String, CliError> {
    let bytes = match file {
        Some(path) => std::fs::read(path).map_err(|error| {
            CliError::Usage(format!("could not read {}: {error}", path.display()))
        })?,
        None => {
            use std::io::Read;
            let mut buf = Vec::new();
            std::io::stdin()
                .read_to_end(&mut buf)
                .map_err(|error| CliError::Usage(format!("could not read stdin: {error}")))?;
            buf
        }
    };
    let text =
        String::from_utf8(bytes).map_err(|_| CliError::Usage("the text must be UTF-8".into()))?;
    if text.len() > MAX_AGENTS_REPO_DRAFT_TEXT_BYTES {
        return Err(CliError::Usage(format!(
            "the text is {} bytes; drafts carry at most {MAX_AGENTS_REPO_DRAFT_TEXT_BYTES} — a file this size is edited with git",
            text.len()
        )));
    }
    Ok(text)
}

#[allow(clippy::too_many_arguments)]
async fn cmd_draft_put(
    client: &BuzzClient,
    project: Option<&str>,
    path: &str,
    file: Option<&Path>,
    message: Option<String>,
    prev: Option<&str>,
) -> Result<(), CliError> {
    validate_draft_path(path).map_err(CliError::Usage)?;
    let text = read_text_arg(file)?;
    let mut snap = snapshot(client, project).await?;
    let prev = resolve_prev(&snap, path, prev)?;
    let tip = read_tip(&snap, client, path).await?;
    if let Some(tip) = &tip {
        if tip.text == text && prev.is_none() {
            return Err(CliError::Usage(format!(
                "{path} already reads exactly this on main; nothing to draft"
            )));
        }
    }
    if path == "team.yml" {
        if let Err(error) = buzz_persona::team::parse_team_yml(&text, Path::new(path)) {
            eprintln!("warning: team.yml does not parse ({error}); the commit will refuse it until it does");
        }
    }
    let op = AgentsRepoDraftOp {
        repo: snap.repo_coordinate().to_owned(),
        message,
        value: AgentsRepoDraftOpValue::FilePut {
            path: path.to_owned(),
            text,
            base: DraftBase {
                base: tip
                    .as_ref()
                    .map(|t| t.blob.clone())
                    .filter(|b| !b.is_empty()),
                base_commit: tip
                    .as_ref()
                    .map(|t| t.commit.clone())
                    .filter(|c| !c.is_empty()),
                prev,
            },
        },
    };
    let out = publish(&mut snap, client, &op).await?;
    println!("{out}");
    Ok(())
}

async fn cmd_draft_move(
    client: &BuzzClient,
    project: Option<&str>,
    path: &str,
    expect: Option<DraftPathClass>,
    message: Option<String>,
    prev: Option<&str>,
) -> Result<(), CliError> {
    let class = validate_draft_path(path).map_err(CliError::Usage)?;
    if let Some(expected) = expect {
        if class != expected {
            return Err(CliError::Usage(format!(
                "{path} is not a {} — pass the path to {}",
                match expected {
                    DraftPathClass::Role | DraftPathClass::Plan => "live role or plan",
                    _ => "archived role or plan",
                },
                match expected {
                    DraftPathClass::Role | DraftPathClass::Plan => "archive",
                    _ => "unarchive",
                }
            )));
        }
    }
    let to = archive_counterpart(path).ok_or_else(|| {
        CliError::Usage(format!(
            "{path} is not a role or plan that can move to or from archive/"
        ))
    })?;
    let mut snap = snapshot(client, project).await?;
    let prev = resolve_prev(&snap, path, prev)?;
    let Some(tip) = read_tip(&snap, client, path).await? else {
        return Err(CliError::NotFound(format!(
            "{path} is not on main; only a committed file can be moved"
        )));
    };
    let op = AgentsRepoDraftOp {
        repo: snap.repo_coordinate().to_owned(),
        message,
        value: AgentsRepoDraftOpValue::FileMove {
            path: path.to_owned(),
            to,
            base: DraftBase {
                base: Some(tip.blob),
                base_commit: Some(tip.commit).filter(|c| !c.is_empty()),
                prev,
            },
        },
    };
    let out = publish(&mut snap, client, &op).await?;
    println!("{out}");
    Ok(())
}

async fn cmd_draft_delete(
    client: &BuzzClient,
    project: Option<&str>,
    path: &str,
    message: Option<String>,
    prev: Option<&str>,
) -> Result<(), CliError> {
    validate_draft_path(path).map_err(CliError::Usage)?;
    let mut snap = snapshot(client, project).await?;
    let prev = resolve_prev(&snap, path, prev)?;
    let Some(tip) = read_tip(&snap, client, path).await? else {
        return Err(CliError::NotFound(format!(
            "{path} is not on main; withdraw the draft that creates it instead (`bee agents-repo draft withdraw <id>`)"
        )));
    };
    let op = AgentsRepoDraftOp {
        repo: snap.repo_coordinate().to_owned(),
        message,
        value: AgentsRepoDraftOpValue::FileDelete {
            path: path.to_owned(),
            base: DraftBase {
                base: Some(tip.blob),
                base_commit: Some(tip.commit).filter(|c| !c.is_empty()),
                prev,
            },
        },
    };
    let out = publish(&mut snap, client, &op).await?;
    println!("{out}");
    Ok(())
}

async fn cmd_draft_withdraw(
    client: &BuzzClient,
    project: Option<&str>,
    id: &str,
) -> Result<(), CliError> {
    let snap = snapshot(client, project).await?;
    let mine = client.keys().public_key().to_hex();
    let matching: Vec<&DraftRow> = snap
        .digest
        .paths
        .iter()
        .flat_map(|entry| std::iter::once(&entry.head).chain(entry.superseded.iter()))
        .filter(|row| row.id == id || (id.len() >= 8 && row.id.starts_with(id)))
        .collect();
    let row = match matching.as_slice() {
        [row] => *row,
        [] => return Err(CliError::NotFound(format!("no open draft matches {id}"))),
        _ => {
            return Err(CliError::Usage(format!(
                "{id} is ambiguous; pass more of the id"
            )))
        }
    };
    if row.author != mine {
        return Err(CliError::Usage(format!(
            "draft {} was written by {}; only its author can withdraw it (NIP-09). Supersede it instead: `bee agents-repo draft put {} --prev {}`",
            &row.id[..12],
            &row.author[..8],
            row.path,
            &row.id[..12]
        )));
    }
    let builder =
        buzz_sdk::builders::build_delete_event(&row.id).map_err(crate::validate::sdk_err)?;
    let event = client.sign_event_unchecked(builder)?;
    let deletion_id = event.id.to_hex();
    let raw = client.submit_event(event).await?;
    let normalized = super::parse_write_response(&raw, "deletion was superseded")?;
    let mut out: Value = serde_json::from_str(&normalized).unwrap_or(Value::Null);
    if let Some(object) = out.as_object_mut() {
        object.entry("event_id").or_insert(json!(deletion_id));
        object.insert("withdrawn".into(), json!(row.id));
        object.insert("path".into(), json!(row.path));
    }
    println!("{out}");
    Ok(())
}

/// Display names for commit trailers, from kind:0 profiles; a pubkey with
/// none is named by its first eight characters.
async fn author_identities(client: &BuzzClient, pubkeys: &[String]) -> Vec<Identity> {
    let mut names: HashMap<String, String> = HashMap::new();
    if !pubkeys.is_empty() {
        if let Ok(rows) = client
            .query_all(json!({ "kinds": [0], "authors": pubkeys, "limit": pubkeys.len() }))
            .await
        {
            for row in rows {
                let Some(pubkey) = row["pubkey"].as_str() else {
                    continue;
                };
                let Some(content) = row["content"].as_str() else {
                    continue;
                };
                let profile: Value = serde_json::from_str(content).unwrap_or(Value::Null);
                let name = profile["display_name"]
                    .as_str()
                    .or_else(|| profile["name"].as_str())
                    .map(str::trim)
                    .filter(|n| !n.is_empty());
                if let Some(name) = name {
                    names
                        .entry(pubkey.to_owned())
                        .or_insert_with(|| name.to_owned());
                }
            }
        }
    }
    pubkeys
        .iter()
        .map(|pubkey| {
            let short: String = pubkey.chars().take(8).collect();
            Identity {
                name: names
                    .get(pubkey)
                    .cloned()
                    .unwrap_or_else(|| format!("Beekeeper {short}")),
                email: format!("{short}@beekeeper.local"),
            }
        })
        .collect()
}

async fn cmd_commit(
    client: &BuzzClient,
    project: Option<&str>,
    all: bool,
    drafts: &[String],
    message: Option<String>,
    templates: Option<&Path>,
) -> Result<(), CliError> {
    if !all && drafts.is_empty() {
        return Err(CliError::Usage(
            "pass --all or one or more --draft <id>".into(),
        ));
    }
    let mut snap = snapshot(client, project).await?;
    let heads: Vec<&DraftRow> = snap
        .digest
        .paths
        .iter()
        .map(|entry| &entry.head)
        .filter(|head| {
            all || drafts
                .iter()
                .any(|d| head.id == *d || (d.len() >= 8 && head.id.starts_with(d)))
        })
        .collect();
    if heads.is_empty() {
        return Err(CliError::NotFound("no open drafts to commit".into()));
    }
    for wanted in drafts {
        if !heads
            .iter()
            .any(|h| h.id == *wanted || (wanted.len() >= 8 && h.id.starts_with(wanted)))
        {
            return Err(CliError::NotFound(format!(
                "{wanted} is not an open head draft; `bee agents-repo drafts --all` lists what is"
            )));
        }
    }
    // A move claims two paths; one change per op, not per path.
    let mut changes: Vec<DraftChange> = Vec::new();
    for head in &heads {
        if changes.iter().any(|c| c.id == head.id) {
            continue;
        }
        changes.push(DraftChange {
            id: head.id.clone(),
            author: head.author.clone(),
            op: head.op.clone(),
            path: head.path.clone(),
            to: head.to.clone(),
            text: head.text.clone(),
            base: head.base.clone(),
            message: head.message.clone(),
        });
    }
    let templates = super::pack::resolve_templates_dir(templates)
        .ok_or_else(|| CliError::Usage(super::pack::no_templates_message()))?;
    let catalog =
        buzz_persona::template::TemplateCatalog::load(&templates, "cli").map_err(|error| {
            CliError::Other(format!("templates at {}: {error}", templates.display()))
        })?;
    let me = client.keys().public_key().to_hex();
    let mut authors: Vec<String> = changes.iter().map(|c| c.author.clone()).collect();
    authors.push(me.clone());
    authors.sort();
    authors.dedup();
    let identities = author_identities(client, &authors).await;
    let committer = identities
        .iter()
        .zip(authors.iter())
        .find(|(_, pubkey)| **pubkey == me)
        .map(|(identity, _)| identity.clone())
        .unwrap_or_else(|| Identity {
            name: format!("Beekeeper {}", &me[..8]),
            email: format!("{}@beekeeper.local", &me[..8]),
        });
    let subject = message.unwrap_or_else(|| {
        let paths: Vec<&str> = changes.iter().map(|c| c.path.as_str()).collect();
        format!(
            "docs(agents): {} draft{} — {}",
            changes.len(),
            if changes.len() == 1 { "" } else { "s" },
            paths.join(", ")
        )
    });
    let request = CommitRequest {
        remote: &snap.clone_url,
        expected_tip: None,
        changes: &changes,
        message: &subject,
        committer: &committer,
        coauthors: &identities,
        catalog: &catalog,
        project: &snap.coordinate,
    };
    let outcome = commit_drafts(&request)?;
    let mut out = serde_json::to_value(&outcome).unwrap_or(Value::Null);
    if let Some(object) = out.as_object_mut() {
        object.insert("project".into(), json!(snap.coordinate));
        object.insert("repo".into(), json!(snap.repo_coordinate()));
        object.insert(
            "drafts".into(),
            json!(changes.iter().map(|c| c.id.clone()).collect::<Vec<_>>()),
        );
        if let Some(note) = snap.pin_note() {
            object.insert("note".into(), json!(note));
        }
    }
    match &outcome {
        CommitOutcome::Yes { commit, .. } => {
            // The seat validates and adopts from its own clone; without this
            // it would read a clone that never saw the commit it just made.
            out["agents_clone"] = super::agents_repo_clone::clone_refresh_report(
                std::env::current_dir().ok().as_deref(),
                &snap.clone_url,
                commit,
            );
            // The record closes every open op on each landed path — the head
            // and everything it superseded — because the head's text is what
            // main now says; a superseded op left open would read as a
            // pending proposal of text the committer already judged.
            let mut paths: Vec<String> = Vec::new();
            let mut ids: Vec<String> = Vec::new();
            for change in &changes {
                paths.push(change.path.clone());
                if let Some(to) = &change.to {
                    paths.push(to.clone());
                }
                for entry in snap.digest.paths.iter().filter(|e| e.path == change.path) {
                    ids.push(entry.head.id.clone());
                    ids.extend(entry.superseded.iter().map(|row| row.id.clone()));
                }
            }
            ids.sort();
            ids.dedup();
            match publish_record(&mut snap, client, commit, paths, ids, Some(subject.clone())).await
            {
                Ok(record) => {
                    out["record"] = record;
                }
                Err(error) => {
                    let repair: String = changes
                        .iter()
                        .map(|c| format!(" --draft {}", c.id))
                        .collect();
                    out["record_error"] = json!(format!(
                        "the commit is on main but the drafts could not be marked committed: {error}. \
                         Repair with `bee agents-repo commit-record {commit}{repair}`"
                    ));
                }
            }
            println!("{out}");
            Ok(())
        }
        CommitOutcome::No { .. } => {
            println!("{out}");
            Err(CliError::Refused("nothing was pushed; see refusals".into()))
        }
        CommitOutcome::Unknown { .. } => {
            println!("{out}");
            Err(CliError::Unconfirmed(
                "the push's result could not be confirmed; reload before retrying".into(),
            ))
        }
    }
}

async fn publish_record(
    snap: &mut Snapshot,
    client: &BuzzClient,
    commit: &str,
    paths: Vec<String>,
    drafts: Vec<String>,
    message: Option<String>,
) -> Result<Value, CliError> {
    let mut paths = paths;
    paths.sort();
    paths.dedup();
    let op = AgentsRepoDraftOp {
        repo: snap.repo_coordinate().to_owned(),
        message,
        value: AgentsRepoDraftOpValue::CommitRecord {
            commit: commit.to_owned(),
            paths,
            drafts,
        },
    };
    publish(snap, client, &op).await
}

/// `bee agents-repo check` — validate a working tree the way a commit
/// validates the tree it is about to push.
///
/// The committer runs `validate_root` over the tree it built; a seat with
/// `workspace.agents_repo: write` pushes with git and never passes through
/// it. This is how that seat gates itself: the same function, the same
/// refusals, over the directory in front of it. It reads only — nothing is
/// published, committed or pushed.
pub fn check_tree(
    root: &Path,
    templates: Option<&Path>,
    project: Option<&str>,
) -> Result<(), CliError> {
    let templates = super::pack::resolve_templates_dir(templates)
        .ok_or_else(|| CliError::Usage(super::pack::no_templates_message()))?;
    let catalog =
        buzz_persona::template::TemplateCatalog::load(&templates, "cli").map_err(|error| {
            CliError::Other(format!("templates at {}: {error}", templates.display()))
        })?;
    // `actions.yml` is parsed against a project coordinate, so without one
    // it is reported unchecked rather than judged against a made-up
    // coordinate it would always refuse.
    let coordinate = project.map(str::to_owned);
    let mut actions_parser = move |text: &str| -> Result<usize, String> {
        let coordinate = coordinate
            .as_deref()
            .expect("the parser is only installed when a coordinate was given");
        buzz_workflow::actions_file::parse_actions_yml(text, coordinate)
            .map(|entries| entries.len())
            .map_err(|error| error.to_string())
    };
    let parser: Option<buzz_persona::agents_repo::ActionsParser<'_>> = match project {
        Some(_) => Some(&mut actions_parser),
        None => None,
    };
    match buzz_persona::agents_repo::validate_root(root, &catalog, parser) {
        Ok(report) => {
            println!(
                "{}",
                serde_json::json!({
                    "root": root.display().to_string(),
                    "ok": true,
                    "roles": report.roles,
                    "archived": report.archived,
                    "skills": report.skills,
                    "within_limits": report.within_limits,
                    "actions": match report.actions {
                        buzz_persona::agents_repo::ActionsCheck::Absent => "absent".to_owned(),
                        buzz_persona::agents_repo::ActionsCheck::Checked(n) => {
                            format!("checked ({n} actions)")
                        }
                        buzz_persona::agents_repo::ActionsCheck::NotChecked(reason) => {
                            format!("not checked: {reason}")
                        }
                    },
                    "warnings": report.warnings,
                })
            );
            Ok(())
        }
        Err(refusals) => {
            println!(
                "{}",
                serde_json::json!({
                    "root": root.display().to_string(),
                    "ok": false,
                    "refusals": refusals.iter().map(|refusal| serde_json::json!({
                        "path": refusal.path,
                        "reason": refusal.reason,
                    })).collect::<Vec<_>>(),
                })
            );
            Err(CliError::Usage(format!(
                "{} would not commit: {} path(s) refuse",
                root.display(),
                refusals.len()
            )))
        }
    }
}

async fn cmd_commit_record(
    client: &BuzzClient,
    project: Option<&str>,
    commit: &str,
    drafts: &[String],
    message: Option<String>,
) -> Result<(), CliError> {
    if drafts.is_empty() {
        return Err(CliError::Usage("pass one or more --draft <id>".into()));
    }
    let mut snap = snapshot(client, project).await?;
    let mut paths = Vec::new();
    let mut ids = Vec::new();
    for wanted in drafts {
        let hit = snap
            .digest
            .paths
            .iter()
            .flat_map(|entry| std::iter::once(&entry.head).chain(entry.superseded.iter()))
            .find(|row| row.id == *wanted || (wanted.len() >= 8 && row.id.starts_with(wanted)));
        match hit {
            Some(row) => {
                paths.push(row.path.clone());
                if let Some(to) = &row.to {
                    paths.push(to.clone());
                }
                ids.push(row.id.clone());
            }
            None if wanted.len() == 64 => {
                // Already closed or never seen; the record may still name it.
                ids.push(wanted.clone());
            }
            None => {
                return Err(CliError::NotFound(format!(
                    "no open draft matches {wanted}; pass its full id"
                )));
            }
        }
    }
    if paths.is_empty() {
        return Err(CliError::Usage(
            "none of the named drafts is open, so their paths are unknown; a record needs at least one open draft"
                .into(),
        ));
    }
    let out = publish_record(&mut snap, client, commit, paths, ids, message).await?;
    println!("{out}");
    Ok(())
}

/// `bee agents-repo`.
pub async fn dispatch(
    cmd: crate::AgentsRepoCmd,
    client: &BuzzClient,
    _format: &crate::OutputFormat,
) -> Result<(), CliError> {
    use crate::{AgentsRepoCmd, AgentsRepoDraftCmd};
    match cmd {
        AgentsRepoCmd::Ls { project } => cmd_ls(client, project.as_deref()).await,
        AgentsRepoCmd::Show {
            project,
            path,
            draft,
        } => cmd_show(client, project.as_deref(), &path, draft).await,
        AgentsRepoCmd::Drafts { project, path, all } => {
            cmd_drafts(client, project.as_deref(), path.as_deref(), all).await
        }
        AgentsRepoCmd::Draft(sub) => match sub {
            AgentsRepoDraftCmd::Put {
                project,
                path,
                file,
                message,
                prev,
            } => {
                cmd_draft_put(
                    client,
                    project.as_deref(),
                    &path,
                    file.as_deref(),
                    message,
                    prev.as_deref(),
                )
                .await
            }
            AgentsRepoDraftCmd::Move {
                project,
                path,
                message,
                prev,
            } => {
                cmd_draft_move(
                    client,
                    project.as_deref(),
                    &path,
                    None,
                    message,
                    prev.as_deref(),
                )
                .await
            }
            AgentsRepoDraftCmd::Archive {
                project,
                path,
                message,
                prev,
            } => {
                let class = validate_draft_path(&path).map_err(CliError::Usage)?;
                let expected =
                    if matches!(class, DraftPathClass::Plan | DraftPathClass::ArchivedPlan) {
                        DraftPathClass::Plan
                    } else {
                        DraftPathClass::Role
                    };
                cmd_draft_move(
                    client,
                    project.as_deref(),
                    &path,
                    Some(expected),
                    message,
                    prev.as_deref(),
                )
                .await
            }
            AgentsRepoDraftCmd::Unarchive {
                project,
                path,
                message,
                prev,
            } => {
                let class = validate_draft_path(&path).map_err(CliError::Usage)?;
                let expected =
                    if matches!(class, DraftPathClass::Plan | DraftPathClass::ArchivedPlan) {
                        DraftPathClass::ArchivedPlan
                    } else {
                        DraftPathClass::ArchivedRole
                    };
                cmd_draft_move(
                    client,
                    project.as_deref(),
                    &path,
                    Some(expected),
                    message,
                    prev.as_deref(),
                )
                .await
            }
            AgentsRepoDraftCmd::Delete {
                project,
                path,
                message,
                prev,
            } => {
                cmd_draft_delete(client, project.as_deref(), &path, message, prev.as_deref()).await
            }
            AgentsRepoDraftCmd::Withdraw { project, id } => {
                cmd_draft_withdraw(client, project.as_deref(), &id).await
            }
        },
        AgentsRepoCmd::Commit {
            project,
            all,
            draft,
            message,
            templates,
        } => {
            cmd_commit(
                client,
                project.as_deref(),
                all,
                &draft,
                message,
                templates.as_deref(),
            )
            .await
        }
        // Answered before the key check in `run`; unreachable here.
        AgentsRepoCmd::Check {
            root,
            templates,
            project,
        } => check_tree(&root, templates.as_deref(), project.as_deref()),
        AgentsRepoCmd::CommitRecord {
            project,
            commit,
            draft,
            message,
        } => cmd_commit_record(client, project.as_deref(), &commit, &draft, message).await,
    }
}

/// `bee plans` — sugar over `plans/<name>.md`.
pub async fn dispatch_plans(
    cmd: crate::PlansCmd,
    client: &BuzzClient,
    _format: &crate::OutputFormat,
) -> Result<(), CliError> {
    use crate::PlansCmd;
    match cmd {
        // Answered before the key check in `run`; kept here so any caller
        // reaching this dispatcher gets the same offline answer.
        PlansCmd::Example => super::plans_example::cmd_example(),
        PlansCmd::List { project } => {
            let snap = snapshot(client, project.as_deref()).await?;
            let listing = client
                .get_git_read(
                    &snap.repo_owner,
                    &snap.repo_id,
                    "tree/refs/heads/main/plans",
                )
                .await;
            let mut plans: Vec<Value> = Vec::new();
            if let Ok(listing) = listing {
                let tree: Value = serde_json::from_slice(&listing.bytes).unwrap_or(Value::Null);
                for entry in tree["entries"].as_array().into_iter().flatten() {
                    let path = entry["path"].as_str().unwrap_or_default();
                    if entry["kind"] != "blob" || !path.ends_with(".md") {
                        continue;
                    }
                    let archived = path.starts_with("plans/archive/");
                    let mut row =
                        json!({ "path": path, "archived": archived, "size": entry["size"] });
                    if let Some(head) = snap.head(path) {
                        row["draft"] =
                            json!({ "id": head.id, "author": head.author, "op": head.op });
                    }
                    plans.push(row);
                }
            }
            for entry in snap
                .digest
                .paths
                .iter()
                .filter(|e| e.path.starts_with("plans/"))
            {
                if !plans.iter().any(|p| p["path"] == entry.path) {
                    plans.push(json!({
                        "path": entry.path,
                        "archived": entry.path.starts_with("plans/archive/"),
                        "size": null,
                        "draft": { "id": entry.head.id, "author": entry.head.author, "op": entry.head.op },
                        "not_on_main": true,
                    }));
                }
            }
            let mut out = json!({ "project": snap.coordinate, "plans": plans });
            honesty(&snap, &mut out);
            println!("{out}");
            Ok(())
        }
        PlansCmd::Show {
            project,
            name,
            draft,
        } => cmd_show(client, project.as_deref(), &plan_path(&name)?, draft).await,
        PlansCmd::Edit {
            project,
            name,
            file,
            message,
            prev,
        } => {
            cmd_draft_put(
                client,
                project.as_deref(),
                &plan_path(&name)?,
                file.as_deref(),
                message,
                prev.as_deref(),
            )
            .await
        }
    }
}

/// `plans/<name>.md` from a bare name, a `name.md`, or a full path.
fn plan_path(name: &str) -> Result<String, CliError> {
    let path = if name.starts_with("plans/") {
        name.to_owned()
    } else if name.ends_with(".md") {
        format!("plans/{name}")
    } else {
        format!("plans/{name}.md")
    };
    match validate_draft_path(&path) {
        Ok(DraftPathClass::Plan | DraftPathClass::ArchivedPlan) => Ok(path),
        Ok(_) => Err(CliError::Usage(format!("{path} is not a plan"))),
        Err(error) => Err(CliError::Usage(error)),
    }
}

#[cfg(test)]
#[path = "agents_repo_tests.rs"]
mod tests;
