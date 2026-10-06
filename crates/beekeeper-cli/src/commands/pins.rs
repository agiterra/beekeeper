//! `bee pins` — which of a project's artifacts show in every member's sidebar
//! (NIP-AR, kind 44251).
//!
//! Reads fetch every op for the project's coordinate and fold them with
//! `beekeeper_core::project_artifact_pin_fold`, the same fold Desktop and Mobile
//! bind to through `conformance/project-artifact-pin-fold/`. Writes publish
//! one op per field, so two members pinning and reordering never overwrite
//! each other.
//!
//! Every write reads the current fold first, both to place a rank between the
//! right neighbours and to stamp `created_at` past the latest op on the same
//! target, so a write made after looking at the sidebar wins the field even on
//! a slightly slow clock.
//!
//! A pin is **shared**: it is a fact about the project, seen by every member.
//! Nothing here hides a pin for one person — that is a per-device viewing
//! preference in each client.

use std::collections::HashMap;

use beekeeper_core::fractional_rank::rank_between;
use beekeeper_core::kind::KIND_PROJECT_ARTIFACT_PIN_OP;
use beekeeper_core::project_artifact_pin::{
    pin_target_kind_of, validate_pin_target, PinTargetKind, ProjectArtifactPinOp,
    ProjectArtifactPinOpValue,
};
use beekeeper_core::project_artifact_pin_fold::{
    fold_project_artifact_pins, PinFoldEvent, PinRow, ProjectArtifactPinDigest,
};
use nostr::Timestamp;
use serde_json::{json, Value};

use super::pulse::resolve_project;
use super::repos::next_replaceable_created_at;
use crate::client::BuzzClient;
use crate::error::CliError;

/// The relay's ingest window is ±900 s; a bump that would land past this
/// margin is refused here rather than by the relay.
const MAX_FUTURE_SKEW_SECS: u64 = 890;

/// Everything a command needs about the project's current pins.
struct Snapshot {
    coordinate: String,
    repo: String,
    digest: ProjectArtifactPinDigest,
    /// Latest `created_at` per target across every decoded op, for the bump.
    latest: HashMap<String, u64>,
    truncated: bool,
}

impl Snapshot {
    fn row(&self, target: &str) -> Option<&PinRow> {
        self.digest.pins.iter().find(|row| row.target == target)
    }

    /// The pinned rows in sidebar order — what a rank is placed among.
    fn pinned(&self) -> Vec<&PinRow> {
        self.digest.pins.iter().filter(|row| row.pinned).collect()
    }
}

/// Read the project's agents repository coordinate and its pin log.
async fn snapshot(client: &BuzzClient, project: Option<&str>) -> Result<Snapshot, CliError> {
    let coordinate = resolve_project(client, project).await?;
    let sources = super::packs::query_pack_sources(client, &coordinate).await?;
    let Some((source, _)) = sources.into_iter().next() else {
        return Err(CliError::NotFound(format!(
            "{coordinate} has no agents repository (no kind:30624 source); create one with \
             `bee packs init --project {coordinate}` or Finish repository setup in the app"
        )));
    };
    let repo = source.repo().to_owned();
    let raw = client
        .query_all(json!({ "kinds": [KIND_PROJECT_ARTIFACT_PIN_OP], "#a": [coordinate] }))
        .await?;
    let truncated = raw.len() >= 1000;
    let mut events: Vec<PinFoldEvent> = Vec::with_capacity(raw.len());
    let mut latest: HashMap<String, u64> = HashMap::new();
    for value in raw {
        let Ok(event) = serde_json::from_value::<PinFoldEvent>(value) else {
            continue;
        };
        if let Some(target) = event
            .tags
            .iter()
            .find(|t| t.first().map(String::as_str) == Some("ar-target"))
            .and_then(|t| t.get(1))
        {
            let slot = latest.entry(target.clone()).or_insert(0);
            *slot = (*slot).max(event.created_at);
        }
        events.push(event);
    }
    let digest = fold_project_artifact_pins(&coordinate, &repo, &events);
    Ok(Snapshot {
        coordinate,
        repo,
        digest,
        latest,
        truncated,
    })
}

/// Add the fold's honesty counters to an output object when any is non-zero,
/// so a caller never reads a sidebar that silently dropped somebody's pin.
fn honesty(snap: &Snapshot, out: &mut Value) {
    let Some(object) = out.as_object_mut() else {
        return;
    };
    if snap.digest.ignored > 0 {
        object.insert("ignored".into(), json!(snap.digest.ignored));
    }
    if snap.digest.other_repo > 0 {
        object.insert("other_repo".into(), json!(snap.digest.other_repo));
    }
    if snap.digest.ranks_without_pin > 0 {
        object.insert(
            "ranks_without_pin".into(),
            json!(snap.digest.ranks_without_pin),
        );
    }
    if snap.truncated {
        object.insert("truncated".into(), json!(true));
    }
}

async fn publish(
    snap: &mut Snapshot,
    client: &BuzzClient,
    op: &ProjectArtifactPinOp,
) -> Result<Value, CliError> {
    let now = Timestamp::now().as_secs();
    let head = snap.latest.get(&op.target).copied().unwrap_or(0);
    let created_at = next_replaceable_created_at(head, now)
        .ok_or_else(|| CliError::Other("pin timestamp cannot be advanced".into()))?;
    if created_at > now + MAX_FUTURE_SKEW_SECS {
        return Err(CliError::Other(format!(
            "the latest op on this target is stamped {} s in the future; retry later rather \
             than publishing outside the relay's window",
            head.saturating_sub(now)
        )));
    }
    let builder = beekeeper_sdk::builders::build_project_artifact_pin_op(&snap.coordinate, op)
        .map_err(crate::validate::sdk_err)?
        .custom_created_at(Timestamp::from(created_at));
    // Signed verbatim: the tag grammar is a closed key set, so the NIP-OA
    // `auth` tag `sign_event` injects would be rejected at ingest. Membership
    // delegation still travels as the `x-auth-tag` header.
    let event = client.sign_event_unchecked(builder)?;
    let event_id = event.id.to_hex();
    let raw = client.submit_event(event).await?;
    let normalized = super::parse_write_response(&raw, "pin op was superseded")?;
    snap.latest.insert(op.target.clone(), created_at);
    let mut response: Value = serde_json::from_str(&normalized).unwrap_or(Value::Null);
    if let Some(object) = response.as_object_mut() {
        object.entry("event_id").or_insert(json!(event_id));
        object.insert("op".into(), json!(op.kind().as_str()));
        object.insert("target".into(), json!(op.target));
        object.insert("rank".into(), json!(op.rank()));
        object.insert("created_at".into(), json!(created_at));
    }
    Ok(response)
}

fn row_json(row: &PinRow) -> Value {
    json!({
        "target": row.target,
        "target_kind": row.target_kind,
        "pinned": row.pinned,
        "rank": row.rank,
        "by": row.by,
        "updated_at": row.updated_at,
    })
}

/// The rank that puts `target` at `index` among the pinned rows, skipping the
/// target's own current row so moving one never measures against itself.
fn rank_at(snap: &Snapshot, target: &str, index: Option<usize>) -> Result<String, CliError> {
    let others: Vec<&PinRow> = snap
        .pinned()
        .into_iter()
        .filter(|row| row.target != target)
        .collect();
    let at = index.unwrap_or(others.len()).min(others.len());
    let after = at.checked_sub(1).and_then(|i| others.get(i));
    let before = others.get(at);
    rank_between(
        after.map(|row| row.rank.as_str()),
        before.map(|row| row.rank.as_str()),
    )
    .map_err(CliError::Other)
}

pub async fn dispatch(
    cmd: crate::PinsCmd,
    client: &BuzzClient,
    _format: &crate::OutputFormat,
) -> Result<(), CliError> {
    use crate::PinsCmd;
    match cmd {
        PinsCmd::List { project, all } => {
            let snap = snapshot(client, project.as_deref()).await?;
            let rows: Vec<Value> = snap
                .digest
                .pins
                .iter()
                .filter(|row| all || row.pinned)
                .map(row_json)
                .collect();
            let mut out = json!({
                "project": snap.coordinate,
                "repo": snap.repo,
                "pins": rows,
            });
            honesty(&snap, &mut out);
            println!("{out}");
            Ok(())
        }
        PinsCmd::Pin {
            project,
            target,
            index,
            folder,
        } => {
            // What the target *is* comes from the one grammar, not from a
            // flag — except on the one shape the two grammars overlap on, a
            // folder name with a dot in it, where the inference refuses and
            // `--folder` is how the caller says which they meant.
            let kind = if folder {
                validate_pin_target(&target, PinTargetKind::Folder).map_err(CliError::Usage)?;
                PinTargetKind::Folder
            } else {
                pin_target_kind_of(&target).map_err(CliError::Usage)?
            };
            let mut snap = snapshot(client, project.as_deref()).await?;
            let rank = rank_at(&snap, &target, index)?;
            let op = ProjectArtifactPinOp {
                repo: snap.repo.clone(),
                target: target.clone(),
                value: ProjectArtifactPinOpValue::PinSet {
                    target_kind: kind,
                    pinned: true,
                    rank,
                },
            };
            let mut out = publish(&mut snap, client, &op).await?;
            if let Some(object) = out.as_object_mut() {
                object.insert("target_kind".into(), json!(kind.as_str()));
            }
            println!("{out}");
            Ok(())
        }
        PinsCmd::Unpin { project, target } => {
            let mut snap = snapshot(client, project.as_deref()).await?;
            // Unpinning keeps the target's rank: re-pinning it later should
            // put it back where it was, not at the end.
            let (kind, rank) = match snap.row(&target) {
                Some(row) => (
                    match row.target_kind.as_str() {
                        "folder" => PinTargetKind::Folder,
                        _ => PinTargetKind::File,
                    },
                    row.rank.clone(),
                ),
                None => {
                    return Err(CliError::NotFound(format!(
                        "{target} is not pinned in this project"
                    )))
                }
            };
            let op = ProjectArtifactPinOp {
                repo: snap.repo.clone(),
                target,
                value: ProjectArtifactPinOpValue::PinSet {
                    target_kind: kind,
                    pinned: false,
                    rank,
                },
            };
            let out = publish(&mut snap, client, &op).await?;
            println!("{out}");
            Ok(())
        }
        PinsCmd::Move {
            project,
            target,
            index,
        } => {
            let mut snap = snapshot(client, project.as_deref()).await?;
            if snap.row(&target).is_none() {
                return Err(CliError::NotFound(format!(
                    "{target} is not pinned in this project"
                )));
            }
            let rank = rank_at(&snap, &target, Some(index))?;
            let op = ProjectArtifactPinOp {
                repo: snap.repo.clone(),
                target,
                value: ProjectArtifactPinOpValue::PinRank { rank },
            };
            let out = publish(&mut snap, client, &op).await?;
            println!("{out}");
            Ok(())
        }
    }
}

#[cfg(test)]
#[path = "pins_tests.rs"]
mod tests;
