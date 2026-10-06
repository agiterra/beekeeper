//! What moved, read from the relay's own kind 30618 ref state.
//!
//! # Two limits this module states rather than implies
//!
//! 1. **30618 is parameterized-replaceable.** It says where a ref *stands now*
//!    and who moved it **last**. It is not a push history, and nothing here may
//!    render it as one: two pushes to one ref leave one row, at the newer SHA.
//! 2. **A wip ref proves a commit was pushed, not that anybody reviewed it.**
//!
//! Both are repeated in `SURFACES.md` §26 and in the row itself, because a
//! reader who takes a wip ref for a reviewed change has been misled by this
//! surface rather than by the seat.

use serde_json::{json, Value};

use beekeeper_core::pulse_mission::{
    is_wip_ref, PulseRefState, WIP_REF_PREFIX, WIP_REF_RETENTION_DAYS,
};

use crate::client::BeekeeperClient;
use crate::error::CliError;

/// Kind of the relay-signed repository state announcement (NIP-34).
pub const KIND_GIT_REPO_STATE: u32 = 30618;

/// Seconds a wip ref lives before it is prunable on age alone.
pub const WIP_REF_RETENTION_SECONDS: i64 = WIP_REF_RETENTION_DAYS as i64 * 24 * 60 * 60;

/// The relay filter for one repository's current ref state.
pub fn ref_state_filter(repo_id: &str) -> Value {
    json!({
        "kinds": [KIND_GIT_REPO_STATE],
        "#d": [repo_id],
        "limit": 1,
    })
}

/// Decode one signature-stripped 30618 event into its refs.
///
/// Every `refs/heads/*` tag is a ref and its commit; the Beekeeper `p` tag is the
/// pusher. A tag that is not a `refs/heads/*` pair is skipped rather than
/// guessed at — `HEAD` is symbolic and `refs/tags/*` is not a branch.
pub fn decode_ref_state(event: &Value) -> Vec<PulseRefState> {
    let Some(tags) = event.get("tags").and_then(Value::as_array) else {
        return Vec::new();
    };
    let as_of = event.get("created_at").and_then(Value::as_i64);
    let pusher = tags
        .iter()
        .filter_map(|tag| tag.as_array())
        .find(|tag| tag.first().and_then(Value::as_str) == Some("p"))
        .and_then(|tag| tag.get(1))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    tags.iter()
        .filter_map(|tag| tag.as_array())
        .filter_map(|tag| {
            let name = tag.first()?.as_str()?;
            let sha = tag.get(1)?.as_str()?;
            if !name.starts_with("refs/heads/") || sha.is_empty() {
                return None;
            }
            Some(PulseRefState {
                ref_name: name.to_owned(),
                sha: sha.to_owned(),
                pusher_pubkey: pusher.clone(),
                as_of,
            })
        })
        .collect()
}

/// Read one repository's current ref state from the relay.
///
/// A read that returns nothing is a real answer — `No ref state on the wire for
/// this repo` — and is never rendered as a repo where nothing moved.
pub async fn fetch_ref_state(
    client: &BeekeeperClient,
    repo_id: &str,
) -> Result<Vec<PulseRefState>, CliError> {
    let events = client.query_all(ref_state_filter(repo_id)).await?;
    Ok(events.iter().flat_map(decode_ref_state).collect())
}

/// What `bee git prune-wip` would delete, keep, and refuse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WipPrunePlan {
    /// Refs to delete: merged, or older than the retention window.
    pub delete: Vec<WipPruneEntry>,
    /// Refs to keep, with the reason.
    pub keep: Vec<WipPruneEntry>,
    /// Refs outside `refs/heads/wip/`, refused rather than touched.
    pub refused: Vec<WipPruneEntry>,
}

/// One ref and why the plan puts it where it does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WipPruneEntry {
    /// Full ref name.
    pub ref_name: String,
    /// The commit it stands at.
    pub sha: String,
    /// One sentence naming the rule that placed it.
    pub reason: String,
}

/// Plan a prune over the relay's current ref state.
///
/// A ref is deleted when its branch has merged **or** its ref state is older
/// than [`WIP_REF_RETENTION_DAYS`]. Two rules never bend:
///
/// - **Nothing outside `refs/heads/wip/` is ever deleted.** It is listed as
///   refused, so a mis-scoped invocation is visible rather than destructive.
/// - **A ref with no readable date and an unmerged branch is kept.** Unknown is
///   not old, and a commit deleted because its date could not be read is a
///   commit that silently stops existing.
pub fn plan_wip_prune(
    refs: &[PulseRefState],
    now_unix: i64,
    merged: &dyn Fn(&str) -> bool,
) -> WipPrunePlan {
    let mut plan = WipPrunePlan {
        delete: Vec::new(),
        keep: Vec::new(),
        refused: Vec::new(),
    };
    for state in refs {
        let entry = |reason: String| WipPruneEntry {
            ref_name: state.ref_name.clone(),
            sha: state.sha.clone(),
            reason,
        };
        if !is_wip_ref(&state.ref_name) {
            plan.refused.push(entry(format!(
                "outside {WIP_REF_PREFIX}: prune-wip never deletes a ref it does not own"
            )));
            continue;
        }
        if merged(&state.sha) {
            plan.delete.push(entry(
                "its commit is merged into the base branch".to_owned(),
            ));
            continue;
        }
        match state.as_of {
            Some(as_of) if now_unix - as_of > WIP_REF_RETENTION_SECONDS => {
                plan.delete.push(entry(format!(
                    "ref state is older than the {WIP_REF_RETENTION_DAYS}-day window"
                )));
            }
            Some(_) => plan.keep.push(entry(format!(
                "unmerged and inside the {WIP_REF_RETENTION_DAYS}-day window"
            ))),
            // Unknown is not old.
            None => plan.keep.push(entry(
                "unmerged, and its ref state carries no readable date".to_owned(),
            )),
        }
    }
    plan
}

/// The prune plan as the JSON `bee pulse prune-wip` prints.
pub fn prune_plan_json(plan: &WipPrunePlan) -> Value {
    let rows = |entries: &[WipPruneEntry]| -> Vec<Value> {
        entries
            .iter()
            .map(|entry| {
                json!({
                    "ref": entry.ref_name,
                    "sha": entry.sha,
                    "reason": entry.reason,
                })
            })
            .collect()
    };
    json!({
        "retentionDays": WIP_REF_RETENTION_DAYS,
        "delete": rows(&plan.delete),
        "keep": rows(&plan.keep),
        "refused": rows(&plan.refused),
    })
}

/// `bee pulse prune-wip --repo <id>` — print the plan, delete nothing.
///
/// Deliberately read-only. Deleting a ref is a push, and a push is
/// irreversible: this command shows what would go and leaves the deletion to
/// the person or the hook that owns the credential.
pub async fn cmd_prune_wip(
    client: &BeekeeperClient,
    repo: &str,
    merged: Option<&str>,
) -> Result<(), CliError> {
    let refs = fetch_ref_state(client, repo).await?;
    let merged_shas: Vec<String> = merged
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect();
    let is_merged = |sha: &str| merged_shas.iter().any(|merged| merged == sha);
    let now_unix = chrono::Utc::now().timestamp();
    let plan = plan_wip_prune(&refs, now_unix, &is_merged);
    println!(
        "{}",
        serde_json::to_string_pretty(&prune_plan_json(&plan))
            .map_err(|error| CliError::Other(error.to_string()))?
    );
    Ok(())
}

#[cfg(test)]
#[path = "wip_refs_tests.rs"]
mod tests;
