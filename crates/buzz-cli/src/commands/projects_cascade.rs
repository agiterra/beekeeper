//! Opt-in child cascade for `buzz projects delete --cascade`.
//!
//! NIP-MP (`docs/nips/NIP-MP.md`) is explicit that deleting a kind:30621
//! project deletes **only** the project event: member repositories, channels,
//! and everything inside them survive. That contract is unchanged — this
//! module is reached only when the caller passes `--cascade`, and the default
//! `buzz projects delete <slug>` path never touches it.
//!
//! The cascade is orchestrated entirely client-side, so every child deletion
//! travels its own existing authorization path (kind:9008 for channels, a
//! kind:5 `a`-tag tombstone for workflow definitions). There is no privileged
//! relay-side sweep.
//!
//! Ordering is load-bearing and lives in one place, [`cascade_steps`]:
//!
//! 1. channels (kind:9008)
//! 2. workflow definitions (kind:5 `a`-tag)
//! 3. **the project tombstone last**
//!
//! A failure part-way through therefore leaves the kind:30621 head in place
//! and the whole operation retryable.
//!
//! Two things are deliberately *not* deleted:
//!
//! - **Repositories are detached, never deleted.** This mirrors the relay's
//!   `clear_repo_project_ref` (`crates/buzz-relay/src/handlers/side_effects.rs`),
//!   which keeps the name reservation so a deletion can never free a repo name
//!   for another owner to squat.
//! - **Shell sessions and coding sessions are counted and reported, not
//!   deleted.** Coding-session events are `h`-scoped to the channels being
//!   deleted; shell-session announces (kind:30623) are owner-addressable and
//!   outside the channel graph.

use std::collections::{BTreeSet, HashSet};

use buzz_core::kind::{
    KIND_CODING_SESSION_GENESIS, KIND_GIT_REPO_ANNOUNCEMENT, KIND_NIP29_GROUP_ADMINS,
    KIND_NIP29_GROUP_METADATA, KIND_PROJECT, KIND_SHELL_SESSION, KIND_WORKFLOW_DEF,
};
use serde_json::{json, Value};

use crate::client::{extract_d_tag, extract_tag_value, BuzzClient};
use crate::commands::parse_write_response;
use crate::error::CliError;

/// Channel type string for a hidden per-project transport channel.
///
/// Mirrors `buzz_core::channel::ChannelType::Transport`. Transport channels
/// admit project members through the **project ACL** rather than an explicit
/// channel-member row, so dropping the project ACL — which the plain
/// (non-cascade) delete already does — makes them unreachable rather than
/// merely orphaned. The cascade deletes them outright and says so.
pub const TRANSPORT_CHANNEL_TYPE: &str = "transport";

// ── Plan model ────────────────────────────────────────────────────────────────

/// One channel a cascade would delete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CascadeChannel {
    /// Channel UUID (the kind:39000 `d` tag).
    pub channel_id: String,
    /// Human-readable channel name, empty when the metadata carries none.
    pub name: String,
    /// Channel type token (`stream`, `forum`, `transport`, …).
    pub channel_type: String,
}

impl CascadeChannel {
    /// `true` when this is a hidden per-project transport channel.
    pub fn is_transport(&self) -> bool {
        self.channel_type == TRANSPORT_CHANNEL_TYPE
    }
}

/// One workflow definition (kind:30620) a cascade would delete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CascadeWorkflow {
    /// Workflow UUID (the kind:30620 `d` tag).
    pub workflow_id: String,
    /// The channel the definition is scoped to (its `h` tag).
    pub channel_id: String,
}

/// Everything a `--cascade` delete would touch, enumerated before any write.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CascadePlan {
    /// Project slug (the kind:30621 `d` tag).
    pub slug: String,
    /// Full project coordinate `30621:<owner-hex>:<slug>`.
    pub coordinate: String,
    /// Channels bound to the project, transport channels included.
    pub channels: Vec<CascadeChannel>,
    /// Caller-authored workflow definitions in those channels.
    pub workflows: Vec<CascadeWorkflow>,
    /// Workflow definitions in those channels authored by somebody else. A
    /// kind:5 `a`-tag tombstone only deletes the signer's own addressable
    /// events, so these survive the cascade and are reported, not deleted.
    pub foreign_workflows: usize,
    /// Shell-session announces (kind:30623) on the project coordinate.
    /// Counted, never deleted.
    pub shell_sessions: usize,
    /// Coding-session genesis events (kind:44226) inside the project's
    /// channels. Counted, never deleted — they die with their channel.
    pub coding_sessions: usize,
    /// Repository coordinates that will be **detached**, never deleted.
    pub detached_repos: Vec<String>,
    /// Channels in [`CascadePlan::channels`] where the caller is **not** listed
    /// as an owner in the kind:39001 admin projection. kind:9008 is owner-only
    /// (`crates/buzz-relay/src/handlers/side_effects.rs`), so these are the
    /// channels most likely to fail the cascade — reported up front so a
    /// caller is not left re-running a command that fails identically.
    pub unowned_channels: Vec<CascadeChannel>,
}

impl CascadePlan {
    /// Number of transport channels in the plan.
    pub fn transport_channel_count(&self) -> usize {
        self.channels.iter().filter(|c| c.is_transport()).count()
    }

    /// `true` when the plan has no child deletions at all — the cascade
    /// then degenerates to exactly the default tombstone-only delete.
    pub fn has_no_children(&self) -> bool {
        self.channels.is_empty() && self.workflows.is_empty()
    }
}

/// One publishable step of a cascade, in execution order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CascadeStep {
    /// Publish a kind:9008 NIP-29 delete-group event.
    Channel(CascadeChannel),
    /// Publish a kind:5 `a`-tag tombstone for a kind:30620 definition.
    Workflow(CascadeWorkflow),
    /// Publish the kind:5 `a`-tag tombstone for the kind:30621 project.
    /// Always the final step.
    Project {
        /// Project slug being tombstoned.
        slug: String,
    },
}

/// Expand a plan into its ordered step list.
///
/// The project tombstone is **always last** so that a failure part-way
/// through leaves the project present and the operation retryable. This is
/// the single source of execution order; [`execute_cascade`] iterates it.
pub fn cascade_steps(plan: &CascadePlan) -> Vec<CascadeStep> {
    let mut steps: Vec<CascadeStep> =
        Vec::with_capacity(plan.channels.len() + plan.workflows.len() + 1);
    steps.extend(plan.channels.iter().cloned().map(CascadeStep::Channel));
    steps.extend(plan.workflows.iter().cloned().map(CascadeStep::Workflow));
    steps.push(CascadeStep::Project {
        slug: plan.slug.clone(),
    });
    steps
}

// ── Pure enumeration helpers (unit-tested without a relay) ────────────────────

/// Canonicalize an addressable coordinate `kind:pubkey:d` for comparison.
///
/// The relay's `validate_project_ref_tag`
/// (`crates/buzz-relay/src/handlers/ingest.rs`) accepts **any** ASCII hex case
/// in the pubkey component and stores `channels.project_ref` verbatim, so the
/// `["project", …]` tag the relay projects back can be mixed case while the
/// coordinate this CLI builds from its own key is always lowercase. Comparing
/// the raw strings would silently drop those channels from the cascade, so
/// every comparison goes through here. Only the pubkey component is
/// case-folded — the `d`/slug component is case-sensitive by NIP-01.
fn normalize_coordinate(value: &str) -> String {
    let mut parts = value.splitn(3, ':');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(kind), Some(pubkey), Some(d)) => {
            format!("{kind}:{}:{d}", pubkey.to_ascii_lowercase())
        }
        _ => value.to_string(),
    }
}

/// Select the kind:39000 channel-metadata events bound to `coordinate`.
///
/// Binding is **two-sided**, and both sides are first-class:
///
/// - the relay-published `["project", <coordinate>]` back-reference
///   (`crates/buzz-relay/src/handlers/side_effects.rs`, which projects the
///   `channels.project_ref` column into channel metadata), and
/// - the project head's own `["channel", <uuid>]` forward refs, an
///   ingest-validated tag on kind:30621
///   (`crates/buzz-relay/src/handlers/ingest.rs`), passed here as
///   `head_channel_ids`.
///
/// A channel bound by only one of the two is still a member of the project —
/// matching the desktop's `channelBelongsToProject`. Matching the back-ref
/// alone would leave forward-ref-only channels invisible to the cascade, which
/// then reports "nothing to do" for a project that plainly has children.
///
/// Transport channels are included deliberately — see
/// [`TRANSPORT_CHANNEL_TYPE`].
///
/// Results are de-duplicated by channel id and ordered by id so a plan is
/// stable across runs.
pub fn channels_for_project(
    events: &[Value],
    coordinate: &str,
    head_channel_ids: &[String],
) -> Vec<CascadeChannel> {
    let wanted = normalize_coordinate(coordinate);
    let forward: HashSet<&str> = head_channel_ids.iter().map(String::as_str).collect();
    let mut seen = HashSet::new();
    let mut channels: Vec<CascadeChannel> = events
        .iter()
        .filter_map(|event| {
            let channel_id = extract_d_tag(event);
            if channel_id.is_empty() {
                return None;
            }
            let back_ref = normalize_coordinate(&extract_tag_value(event, "project")) == wanted;
            if !back_ref && !forward.contains(channel_id.as_str()) {
                return None;
            }
            if !seen.insert(channel_id.clone()) {
                return None;
            }
            Some(CascadeChannel {
                name: extract_tag_value(event, "name"),
                channel_type: extract_tag_value(event, "t"),
                channel_id,
            })
        })
        .collect();
    channels.sort_by(|a, b| a.channel_id.cmp(&b.channel_id));
    channels
}

/// Channels from `channels` where `author_hex` is **not** listed as an owner in
/// the kind:39001 group-admins projection.
///
/// kind:9008 (delete group) is owner-only, enforced before storage. The relay
/// grants one documented exception the client cannot see — the owning human of
/// an active owner-role *agent* in the channel — so this is a pre-flight
/// **warning**, never a filter: the cascade still attempts every channel and
/// reports what actually happened.
///
/// A channel with no kind:39001 event in `admin_events` is treated as unowned;
/// an absent admin projection is exactly the case where the caller has no
/// owner grant to point at.
pub fn channels_without_owner_grant(
    channels: &[CascadeChannel],
    admin_events: &[Value],
    author_hex: &str,
) -> Vec<CascadeChannel> {
    let owned: HashSet<String> = admin_events
        .iter()
        .filter(|event| has_owner_p_tag(event, author_hex))
        .map(extract_d_tag)
        .collect();
    channels
        .iter()
        .filter(|channel| !owned.contains(&channel.channel_id))
        .cloned()
        .collect()
}

/// `true` when `event` carries `["p", <author_hex>, "owner"]`.
fn has_owner_p_tag(event: &Value, author_hex: &str) -> bool {
    event
        .get("tags")
        .and_then(Value::as_array)
        .is_some_and(|tags| {
            tags.iter().any(|tag| {
                let Some(parts) = tag.as_array() else {
                    return false;
                };
                let field = |i: usize| parts.get(i).and_then(Value::as_str).unwrap_or_default();
                field(0) == "p" && field(1).eq_ignore_ascii_case(author_hex) && field(2) == "owner"
            })
        })
}

/// Split kind:30620 workflow definitions into the caller's own (deletable via
/// a kind:5 `a`-tag tombstone) and everybody else's (reported, not deleted).
///
/// `channel_ids` bounds the scan to the project's channels; a definition whose
/// `h` tag is outside that set is ignored.
pub fn workflows_in_channels(
    events: &[Value],
    channel_ids: &HashSet<String>,
    author_hex: &str,
) -> (Vec<CascadeWorkflow>, usize) {
    let mut seen = HashSet::new();
    let mut mine: Vec<CascadeWorkflow> = Vec::new();
    let mut foreign = 0usize;
    for event in events {
        let channel_id = extract_tag_value(event, "h");
        if !channel_ids.contains(&channel_id) {
            continue;
        }
        let workflow_id = extract_d_tag(event);
        if workflow_id.is_empty() || !seen.insert(workflow_id.clone()) {
            continue;
        }
        let author = event
            .get("pubkey")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if author == author_hex {
            mine.push(CascadeWorkflow {
                workflow_id,
                channel_id,
            });
        } else {
            foreign += 1;
        }
    }
    mine.sort_by(|a, b| a.workflow_id.cmp(&b.workflow_id));
    (mine, foreign)
}

/// Collect the repository coordinates a delete would **detach**: the project
/// head's kind:30617 `a` member tags plus any kind:30617 announcement carrying
/// a `["project", <coordinate>]` back-reference.
///
/// `head_member_coords` is the head's **entire** `a` tag set, which legitimately
/// mixes repositories with agent members (kind:30617 personas/teams/managed
/// agents — see the desktop's `eventToProjectContainer`). Only kind:30617
/// coordinates are repositories; counting the rest would inflate
/// `repos_detached` and tell the caller repos are being detached that are not
/// repos at all.
pub fn detached_repo_coords(
    head_member_coords: &[String],
    repo_events: &[Value],
    coordinate: &str,
) -> Vec<String> {
    let repo_prefix = format!("{KIND_GIT_REPO_ANNOUNCEMENT}:");
    let wanted = normalize_coordinate(coordinate);
    let mut coords: BTreeSet<String> = head_member_coords
        .iter()
        .filter(|coord| coord.starts_with(&repo_prefix))
        .map(|coord| normalize_coordinate(coord))
        .collect();
    for event in repo_events {
        if normalize_coordinate(&extract_tag_value(event, "project")) != wanted {
            continue;
        }
        let d = extract_d_tag(event);
        let owner = event
            .get("pubkey")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if d.is_empty() || owner.is_empty() {
            continue;
        }
        coords.insert(format!("{KIND_GIT_REPO_ANNOUNCEMENT}:{owner}:{d}"));
    }
    coords.into_iter().collect()
}

/// Count kind:44226 coding-session genesis events inside the plan's channels.
fn count_sessions_in_channels(events: &[Value], channel_ids: &HashSet<String>) -> usize {
    events
        .iter()
        .filter(|event| channel_ids.contains(&extract_tag_value(event, "h")))
        .count()
}

// ── Warnings ──────────────────────────────────────────────────────────────────

/// Human-readable warnings a caller must see before confirming.
///
/// The transport-channel line is the sharp edge in current behavior and is
/// always emitted when the project has one: a plain delete drops the project
/// ACL, which is the *only* thing admitting members to a transport channel,
/// so those channels become unreachable whether or not `--cascade` is passed.
pub fn cascade_warnings(plan: &CascadePlan) -> Vec<String> {
    let mut warnings = Vec::new();
    let transports = plan.transport_channel_count();
    if transports > 0 {
        warnings.push(format!(
            "{transports} transport channel(s) will be deleted. Transport channels admit \
             project members through the project ACL only, so deleting this project makes \
             them unreachable with or without --cascade; the cascade deletes them outright \
             instead of leaving them stranded."
        ));
    }
    if !plan.unowned_channels.is_empty() {
        let names: Vec<String> = plan
            .unowned_channels
            .iter()
            .map(|c| {
                if c.name.is_empty() {
                    c.channel_id.clone()
                } else {
                    format!("{} ({})", c.name, c.channel_id)
                }
            })
            .collect();
        let it = if plan.unowned_channels.len() == 1 {
            "it"
        } else {
            "them"
        };
        warnings.push(format!(
            "You are not listed as an owner of {} of these channel(s): {}. Deleting a channel \
             (kind:9008) is owner-only, so the cascade will fail on {it} unless you own an \
             owner-role agent in {it} — and the project is NOT deleted when any child fails. \
             Have the channel owner delete {it} first, or detach {it} from the project.",
            plan.unowned_channels.len(),
            names.join(", "),
        ));
    }
    if plan.foreign_workflows > 0 {
        warnings.push(format!(
            "{} workflow definition(s) in these channels were authored by someone else and \
             will NOT be deleted — a kind:5 tombstone only deletes the signer's own events.",
            plan.foreign_workflows
        ));
    }
    if plan.shell_sessions > 0 {
        warnings.push(format!(
            "{} shell-session announce(s) reference this project and will NOT be deleted.",
            plan.shell_sessions
        ));
    }
    if plan.coding_sessions > 0 {
        warnings.push(format!(
            "{} coding-session genesis event(s) live in these channels; they are not deleted \
             individually and become unreachable with their channel.",
            plan.coding_sessions
        ));
    }
    if !plan.detached_repos.is_empty() {
        warnings.push(format!(
            "{} repository/repositories are DETACHED, never deleted — the relay keeps the name \
             reservation so a deletion cannot free a repo name for another owner.",
            plan.detached_repos.len()
        ));
    }
    warnings
}

/// The confirmation a `--cascade` delete must obtain before it publishes.
///
/// Returns `Some(message)` when the command must print the plan and refuse,
/// `None` only when `yes` is set. The gate is **unconditional**: an empty plan
/// does not bypass it, because "no children" and "enumeration came up short"
/// look identical from here and only one of them is safe to act on silently.
pub fn cascade_confirmation_required(plan: &CascadePlan, yes: bool) -> Option<String> {
    if yes {
        return None;
    }
    let slug = &plan.slug;
    if plan.has_no_children() {
        return Some(format!(
            "cascade delete of project {slug:?} enumerated NO channels and NO workflows, so it \
             would delete only the project itself — identical to `buzz projects delete {slug}` \
             without --cascade. If you expected children here, the enumeration came up short; \
             check the plan above before proceeding. Re-run with --yes to confirm, or --dry-run \
             to inspect the plan without publishing."
        ));
    }
    Some(format!(
        "cascade delete of project {slug:?} would delete {} channel(s) ({} transport) and {} \
         workflow(s) as well as the project itself. Re-run with --yes to confirm, or --dry-run \
         to inspect the plan without publishing.",
        plan.channels.len(),
        plan.transport_channel_count(),
        plan.workflows.len(),
    ))
}

/// Render a plan as the JSON the CLI prints for `--dry-run` and before a
/// confirmed cascade.
pub fn plan_json(plan: &CascadePlan, dry_run: bool) -> Value {
    json!({
        "project": plan.slug,
        "coordinate": plan.coordinate,
        "cascade": true,
        "dry_run": dry_run,
        "counts": {
            "channels": plan.channels.len(),
            "transport_channels": plan.transport_channel_count(),
            "channels_without_owner_grant": plan.unowned_channels.len(),
            "workflows": plan.workflows.len(),
            "foreign_workflows": plan.foreign_workflows,
            "shell_sessions": plan.shell_sessions,
            "coding_sessions": plan.coding_sessions,
            "repos_detached": plan.detached_repos.len(),
        },
        "channels": plan.channels.iter().map(|c| json!({
            "channel_id": c.channel_id,
            "name": c.name,
            "type": c.channel_type,
        })).collect::<Vec<_>>(),
        "channels_without_owner_grant": plan.unowned_channels.iter().map(|c| json!({
            "channel_id": c.channel_id,
            "name": c.name,
            "type": c.channel_type,
        })).collect::<Vec<_>>(),
        "workflows": plan.workflows.iter().map(|w| json!({
            "workflow_id": w.workflow_id,
            "channel_id": w.channel_id,
        })).collect::<Vec<_>>(),
        "repos_detached": plan.detached_repos,
        "warnings": cascade_warnings(plan),
    })
}

// ── Enumeration (network) ─────────────────────────────────────────────────────

/// Enumerate every child of `coordinate` before any write happens.
///
/// `head_member_coords` are the `a` tag values on the project head (repos and
/// agent members mixed — [`detached_repo_coords`] does the kind filtering).
/// `head_channel_ids` are the head's `["channel", <uuid>]` forward refs, which
/// bind a channel to the project independently of the relay's back-reference.
pub async fn enumerate_cascade(
    client: &BuzzClient,
    slug: &str,
    coordinate: &str,
    head_member_coords: &[String],
    head_channel_ids: &[String],
) -> Result<CascadePlan, CliError> {
    // Channels: the `project` tag is multi-character, so it is not a Nostr
    // generic-tag filter key. Fetch the community's channel metadata and match
    // client-side — the same shape `channels list` uses.
    let channel_events = client
        .query_all(json!({ "kinds": [KIND_NIP29_GROUP_METADATA] }))
        .await?;
    let channels = channels_for_project(&channel_events, coordinate, head_channel_ids);
    let channel_ids: HashSet<String> = channels
        .iter()
        .map(|c| c.channel_id.clone())
        .collect::<HashSet<_>>();

    let author_hex = client.keys().public_key().to_hex();

    // Pre-flight ownership: kind:9008 is owner-only, so surface the channels
    // the caller has no owner grant on before anything is published.
    let unowned_channels = if channels.is_empty() {
        Vec::new()
    } else {
        let ids: Vec<&String> = channels.iter().map(|c| &c.channel_id).collect();
        let admin_events = client
            .query_all(json!({ "kinds": [KIND_NIP29_GROUP_ADMINS], "#d": ids }))
            .await?;
        channels_without_owner_grant(&channels, &admin_events, &author_hex)
    };

    // Workflows and coding sessions are `h`-scoped, so both are single
    // indexed queries once the channel set is known.
    let (workflows, foreign_workflows, coding_sessions) = if channel_ids.is_empty() {
        (Vec::new(), 0, 0)
    } else {
        let ids: Vec<&String> = channels.iter().map(|c| &c.channel_id).collect();
        let workflow_events = client
            .query_all(json!({ "kinds": [KIND_WORKFLOW_DEF], "#h": ids }))
            .await?;
        let (mine, foreign) = workflows_in_channels(&workflow_events, &channel_ids, &author_hex);
        let session_events = client
            .query_all(json!({ "kinds": [KIND_CODING_SESSION_GENESIS], "#h": ids }))
            .await?;
        let sessions = count_sessions_in_channels(&session_events, &channel_ids);
        (mine, foreign, sessions)
    };

    // Shell-session announces carry the project coordinate in a single-letter
    // `a` tag, so the relay can filter them directly.
    let shell_events = client
        .query_all(json!({ "kinds": [KIND_SHELL_SESSION], "#a": [coordinate] }))
        .await?;

    // Repos: `project` is again multi-character, so scan announcements and
    // match client-side, unioned with the head's curated `a` members.
    let repo_events = client
        .query_all(json!({ "kinds": [KIND_GIT_REPO_ANNOUNCEMENT] }))
        .await?;
    let detached_repos = detached_repo_coords(head_member_coords, &repo_events, coordinate);

    Ok(CascadePlan {
        slug: slug.to_string(),
        coordinate: coordinate.to_string(),
        channels,
        workflows,
        foreign_workflows,
        shell_sessions: shell_events.len(),
        coding_sessions,
        detached_repos,
        unowned_channels,
    })
}

// ── Execution ─────────────────────────────────────────────────────────────────

/// What one cascade step did.
#[derive(Debug, Clone, PartialEq, Eq)]
enum StepOutcome {
    Deleted,
    AlreadyDeleted,
    Failed(String),
}

/// Publish a kind:9008 delete-group event for one channel.
async fn delete_channel(client: &BuzzClient, channel: &CascadeChannel) -> StepOutcome {
    let uuid = match uuid::Uuid::parse_str(&channel.channel_id) {
        Ok(uuid) => uuid,
        Err(e) => return StepOutcome::Failed(format!("invalid channel id: {e}")),
    };
    let builder = match buzz_sdk::build_delete_channel(uuid) {
        Ok(builder) => builder,
        Err(e) => return StepOutcome::Failed(format!("build_delete_channel failed: {e}")),
    };
    submit_delete(client, builder, "channel was already deleted").await
}

/// Publish a kind:5 `a`-tag tombstone for one workflow definition.
async fn delete_workflow(client: &BuzzClient, workflow: &CascadeWorkflow) -> StepOutcome {
    let uuid = match uuid::Uuid::parse_str(&workflow.workflow_id) {
        Ok(uuid) => uuid,
        Err(e) => return StepOutcome::Failed(format!("invalid workflow id: {e}")),
    };
    let author = client.keys().public_key().to_hex();
    let builder = match buzz_sdk::build_workflow_delete(&author, uuid) {
        Ok(builder) => builder,
        Err(e) => return StepOutcome::Failed(format!("build_workflow_delete failed: {e}")),
    };
    submit_delete(client, builder, "workflow was already deleted").await
}

/// Sign, submit, and classify one child deletion. A relay-reported duplicate
/// means the tombstone is already in effect — that is success, not failure,
/// or a retried cascade could never converge.
async fn submit_delete(
    client: &BuzzClient,
    builder: nostr::EventBuilder,
    duplicate_msg: &str,
) -> StepOutcome {
    let event = match client.sign_event(builder) {
        Ok(event) => event,
        Err(e) => return StepOutcome::Failed(e.to_string()),
    };
    match client.submit_event(event).await {
        Err(e) => StepOutcome::Failed(e.to_string()),
        Ok(raw) => match parse_write_response(&raw, duplicate_msg) {
            Ok(_) => StepOutcome::Deleted,
            Err(CliError::Conflict(_)) => StepOutcome::AlreadyDeleted,
            Err(e) => StepOutcome::Failed(e.to_string()),
        },
    }
}

/// Run a cascade: every child step in [`cascade_steps`] order, then the
/// project tombstone via `publish_tombstone` — but **only** if every child
/// succeeded.
///
/// `publish_tombstone` is the caller's existing head-based delete path, passed
/// in so the cascade cannot drift from the default delete's semantics.
///
/// On any child failure this reports exactly which children were deleted and
/// which were not, leaves the project head in place, and returns
/// [`CliError::Other`] (exit code 4) — a partially-completed cascade is never
/// reported as success.
pub async fn execute_cascade<F, Fut>(
    client: &BuzzClient,
    plan: &CascadePlan,
    publish_tombstone: F,
) -> Result<(), CliError>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<(), CliError>>,
{
    let mut deleted: Vec<Value> = Vec::new();
    let mut failed: Vec<Value> = Vec::new();

    for step in cascade_steps(plan) {
        match step {
            CascadeStep::Channel(channel) => {
                let outcome = delete_channel(client, &channel).await;
                record(
                    &mut deleted,
                    &mut failed,
                    outcome,
                    json!({ "type": "channel", "id": channel.channel_id, "channel_type": channel.channel_type }),
                );
            }
            CascadeStep::Workflow(workflow) => {
                let outcome = delete_workflow(client, &workflow).await;
                record(
                    &mut deleted,
                    &mut failed,
                    outcome,
                    json!({ "type": "workflow", "id": workflow.workflow_id, "channel_id": workflow.channel_id }),
                );
            }
            CascadeStep::Project { slug } => {
                if !failed.is_empty() {
                    // Tombstone withheld: the project head stays, so the
                    // whole cascade can simply be re-run.
                    println!(
                        "{}",
                        json!({
                            "project": slug,
                            "cascade": true,
                            "status": "partial",
                            "project_deleted": false,
                            "deleted": deleted,
                            "failed": failed,
                        })
                    );
                    return Err(CliError::Other(format!(
                        "cascade delete of project {slug:?} stopped after {} child failure(s); \
                         the project was NOT deleted — fix the failures and re-run",
                        failed.len()
                    )));
                }
                publish_tombstone().await?;
                println!(
                    "{}",
                    json!({
                        "project": slug,
                        "cascade": true,
                        "status": "ok",
                        "project_deleted": true,
                        "deleted": deleted,
                        "repos_detached": plan.detached_repos,
                    })
                );
                return Ok(());
            }
        }
    }
    // `cascade_steps` always ends with `DeleteProject`, so this is unreachable
    // in practice; returning an error beats claiming a success that never ran.
    Err(CliError::Other(
        "cascade plan produced no project tombstone step".into(),
    ))
}

fn record(
    deleted: &mut Vec<Value>,
    failed: &mut Vec<Value>,
    outcome: StepOutcome,
    mut item: Value,
) {
    match outcome {
        StepOutcome::Deleted => deleted.push(item),
        StepOutcome::AlreadyDeleted => {
            if let Some(obj) = item.as_object_mut() {
                obj.insert("note".into(), json!("already deleted"));
            }
            deleted.push(item);
        }
        StepOutcome::Failed(reason) => {
            if let Some(obj) = item.as_object_mut() {
                obj.insert("error".into(), json!(reason));
            }
            failed.push(item);
        }
    }
}

/// The project coordinate a cascade is scoped to.
pub fn project_coordinate(owner_hex: &str, slug: &str) -> String {
    format!("{KIND_PROJECT}:{owner_hex}:{slug}")
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const OWNER: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const OTHER: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const COORD: &str =
        "30621:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa:platform";

    fn channel_event(id: &str, name: &str, ty: &str, project: Option<&str>) -> Value {
        let mut tags = vec![json!(["d", id]), json!(["name", name]), json!(["t", ty])];
        if let Some(project) = project {
            tags.push(json!(["project", project]));
        }
        json!({ "kind": 39000, "pubkey": OWNER, "tags": tags })
    }

    fn workflow_event(id: &str, channel: &str, author: &str) -> Value {
        json!({
            "kind": 30620,
            "pubkey": author,
            "tags": [["d", id], ["h", channel]],
        })
    }

    fn plan_with(channels: Vec<CascadeChannel>, workflows: Vec<CascadeWorkflow>) -> CascadePlan {
        CascadePlan {
            slug: "platform".into(),
            coordinate: COORD.into(),
            channels,
            workflows,
            ..CascadePlan::default()
        }
    }

    fn channel(id: &str, ty: &str) -> CascadeChannel {
        CascadeChannel {
            channel_id: id.into(),
            name: format!("#{id}"),
            channel_type: ty.into(),
        }
    }

    // ── channels_for_project ─────────────────────────────────────────────────

    #[test]
    fn channels_for_project_matches_only_the_project_tag() {
        let events = vec![
            channel_event("c1", "general", "stream", Some(COORD)),
            channel_event("c2", "elsewhere", "stream", Some("30621:deadbeef:other")),
            channel_event("c3", "global", "stream", None),
        ];
        let picked = channels_for_project(&events, COORD, &[]);
        assert_eq!(picked.len(), 1);
        assert_eq!(picked[0].channel_id, "c1");
    }

    #[test]
    fn channels_for_project_includes_transport_channels() {
        let events = vec![
            channel_event("c1", "general", "stream", Some(COORD)),
            channel_event("c2", "forum", "forum", Some(COORD)),
            channel_event("c3", "sessions", TRANSPORT_CHANNEL_TYPE, Some(COORD)),
        ];
        let picked = channels_for_project(&events, COORD, &[]);
        assert_eq!(
            picked.len(),
            3,
            "transport channel must not be filtered out"
        );
        assert!(
            picked.iter().any(CascadeChannel::is_transport),
            "the transport channel must be present in the plan"
        );
    }

    #[test]
    fn channels_for_project_dedupes_and_orders_by_id() {
        let events = vec![
            channel_event("c9", "nine", "stream", Some(COORD)),
            channel_event("c1", "one", "stream", Some(COORD)),
            channel_event("c1", "one-again", "stream", Some(COORD)),
        ];
        let picked = channels_for_project(&events, COORD, &[]);
        assert_eq!(
            picked
                .iter()
                .map(|c| c.channel_id.as_str())
                .collect::<Vec<_>>(),
            vec!["c1", "c9"]
        );
    }

    #[test]
    fn channels_for_project_includes_head_forward_refs_without_a_back_ref() {
        // `c2` is bound only by the project head's `["channel", "c2"]` tag —
        // the relay never wrote a `project` back-reference onto it.
        let events = vec![
            channel_event("c1", "general", "stream", Some(COORD)),
            channel_event("c2", "forward-only", "forum", None),
            channel_event("c3", "unrelated", "stream", None),
        ];
        let picked = channels_for_project(&events, COORD, &["c2".to_string()]);
        assert_eq!(
            picked
                .iter()
                .map(|c| c.channel_id.as_str())
                .collect::<Vec<_>>(),
            vec!["c1", "c2"],
            "a forward-ref-only channel must not be invisible to the cascade"
        );
        assert_eq!(picked[1].name, "forward-only");
        assert_eq!(picked[1].channel_type, "forum");
    }

    #[test]
    fn channels_for_project_dedupes_a_channel_bound_both_ways() {
        let events = vec![channel_event("c1", "general", "stream", Some(COORD))];
        let picked = channels_for_project(&events, COORD, &["c1".to_string()]);
        assert_eq!(
            picked.len(),
            1,
            "a channel bound by BOTH a forward ref and a back-ref is one channel"
        );
    }

    #[test]
    fn channels_for_project_matches_an_uppercase_back_reference() {
        // `validate_project_ref_tag` accepts any ASCII hex case and the relay
        // stores `channels.project_ref` verbatim, so the projected tag can be
        // mixed case while our own coordinate is always lowercase.
        let upper = format!("30621:{}:platform", OWNER.to_uppercase());
        let events = vec![channel_event("c1", "general", "stream", Some(&upper))];
        let picked = channels_for_project(&events, COORD, &[]);
        assert_eq!(
            picked.len(),
            1,
            "a mixed-case project back-reference must still bind"
        );
    }

    // ── channels_without_owner_grant ─────────────────────────────────────────

    #[test]
    fn channels_without_owner_grant_flags_channels_the_caller_cannot_delete() {
        let channels = vec![channel("c1", "stream"), channel("c2", "stream")];
        let admins = vec![
            json!({ "kind": 39001, "tags": [["d", "c1"], ["p", OWNER, "owner"]] }),
            json!({ "kind": 39001, "tags": [["d", "c2"], ["p", OTHER, "owner"], ["p", OWNER, "admin"]] }),
        ];
        let unowned = channels_without_owner_grant(&channels, &admins, OWNER);
        assert_eq!(
            unowned
                .iter()
                .map(|c| c.channel_id.as_str())
                .collect::<Vec<_>>(),
            vec!["c2"],
            "admin is not owner — kind:9008 is owner-only"
        );
    }

    #[test]
    fn channels_without_owner_grant_treats_a_missing_admin_event_as_unowned() {
        let channels = vec![channel("c1", "stream")];
        let unowned = channels_without_owner_grant(&channels, &[], OWNER);
        assert_eq!(unowned.len(), 1);
    }

    #[test]
    fn unowned_channels_produce_a_named_pre_flight_warning() {
        let mut plan = plan_with(vec![channel("c1", "stream")], Vec::new());
        plan.unowned_channels = vec![channel("c1", "stream")];
        let warnings = cascade_warnings(&plan);
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("not listed as an owner") && w.contains("c1")),
            "the warning must name the un-deletable channel, got {warnings:?}"
        );
    }

    // ── workflows_in_channels ────────────────────────────────────────────────

    #[test]
    fn workflows_in_channels_splits_own_from_foreign_and_ignores_outsiders() {
        let ids: HashSet<String> = ["c1".to_string(), "c2".to_string()].into_iter().collect();
        let events = vec![
            workflow_event("w1", "c1", OWNER),
            workflow_event("w2", "c2", OTHER),
            workflow_event("w3", "c-elsewhere", OWNER),
        ];
        let (mine, foreign) = workflows_in_channels(&events, &ids, OWNER);
        assert_eq!(mine.len(), 1);
        assert_eq!(mine[0].workflow_id, "w1");
        assert_eq!(mine[0].channel_id, "c1");
        assert_eq!(
            foreign, 1,
            "another author's workflow is reported, not deleted"
        );
    }

    // ── detached_repo_coords ─────────────────────────────────────────────────

    #[test]
    fn detached_repo_coords_unions_head_members_and_back_refs() {
        let head = vec![format!("30617:{OWNER}:buzz")];
        let repos = vec![
            json!({ "kind": 30617, "pubkey": OTHER, "tags": [["d", "infra"], ["project", COORD]] }),
            json!({ "kind": 30617, "pubkey": OTHER, "tags": [["d", "unrelated"]] }),
        ];
        let coords = detached_repo_coords(&head, &repos, COORD);
        assert_eq!(
            coords,
            vec![
                format!("30617:{OWNER}:buzz"),
                format!("30617:{OTHER}:infra")
            ]
        );
    }

    #[test]
    fn detached_repo_coords_ignores_non_repo_head_members() {
        // A project head's `a` tags legitimately mix repos with agent members
        // (persona / team / managed agent). Counting those as detached repos
        // would report "repos_detached: 4" for a project with one repo.
        let head = vec![
            format!("30617:{OWNER}:buzz"),
            format!("30618:{OWNER}:reviewer"),
            format!("30619:{OWNER}:build-team"),
            format!("31337:{OWNER}:managed-agent"),
        ];
        let coords = detached_repo_coords(&head, &[], COORD);
        assert_eq!(
            coords,
            vec![format!("30617:{OWNER}:buzz")],
            "only kind:30617 coordinates are repositories"
        );
    }

    #[test]
    fn detached_repo_coords_matches_an_uppercase_project_back_reference() {
        let upper = format!("30621:{}:platform", OWNER.to_uppercase());
        let repos = vec![
            json!({ "kind": 30617, "pubkey": OTHER, "tags": [["d", "infra"], ["project", upper]] }),
        ];
        let coords = detached_repo_coords(&[], &repos, COORD);
        assert_eq!(coords, vec![format!("30617:{OTHER}:infra")]);
    }

    // ── cascade_steps ordering ───────────────────────────────────────────────

    #[test]
    fn cascade_steps_put_the_project_tombstone_last() {
        let plan = plan_with(
            vec![
                channel("c1", "stream"),
                channel("c2", TRANSPORT_CHANNEL_TYPE),
            ],
            vec![CascadeWorkflow {
                workflow_id: "w1".into(),
                channel_id: "c1".into(),
            }],
        );
        let steps = cascade_steps(&plan);
        assert_eq!(steps.len(), 4);
        assert!(matches!(steps[0], CascadeStep::Channel(_)));
        assert!(matches!(steps[1], CascadeStep::Channel(_)));
        assert!(matches!(steps[2], CascadeStep::Workflow(_)));
        assert_eq!(
            steps[3],
            CascadeStep::Project {
                slug: "platform".into()
            },
            "the kind:30621 tombstone must be the final step"
        );
        // And nothing else may be a project step.
        assert_eq!(
            steps
                .iter()
                .filter(|s| matches!(s, CascadeStep::Project { .. }))
                .count(),
            1
        );
    }

    #[test]
    fn cascade_steps_include_transport_channels_before_the_tombstone() {
        let plan = plan_with(vec![channel("t1", TRANSPORT_CHANNEL_TYPE)], Vec::new());
        let steps = cascade_steps(&plan);
        assert_eq!(
            steps[0],
            CascadeStep::Channel(channel("t1", TRANSPORT_CHANNEL_TYPE))
        );
        assert!(matches!(steps[1], CascadeStep::Project { .. }));
    }

    #[test]
    fn cascade_steps_on_an_empty_plan_is_just_the_tombstone() {
        let plan = plan_with(Vec::new(), Vec::new());
        assert!(plan.has_no_children());
        let steps = cascade_steps(&plan);
        assert_eq!(steps.len(), 1);
        assert!(matches!(steps[0], CascadeStep::Project { .. }));
    }

    // ── warnings ─────────────────────────────────────────────────────────────

    #[test]
    fn transport_channels_always_produce_a_warning() {
        let plan = plan_with(
            vec![
                channel("c1", "stream"),
                channel("t1", TRANSPORT_CHANNEL_TYPE),
            ],
            Vec::new(),
        );
        assert_eq!(plan.transport_channel_count(), 1);
        let warnings = cascade_warnings(&plan);
        assert!(
            warnings.iter().any(|w| w.contains("transport channel")),
            "expected a transport-channel warning, got {warnings:?}"
        );
    }

    #[test]
    fn no_transport_channels_means_no_transport_warning() {
        let plan = plan_with(vec![channel("c1", "stream")], Vec::new());
        assert!(cascade_warnings(&plan).is_empty());
    }

    #[test]
    fn detached_repos_warn_that_they_are_never_deleted() {
        let mut plan = plan_with(Vec::new(), Vec::new());
        plan.detached_repos = vec![format!("30617:{OWNER}:buzz")];
        let warnings = cascade_warnings(&plan);
        assert!(warnings.iter().any(|w| w.contains("DETACHED")));
    }

    // ── confirmation gate ────────────────────────────────────────────────────

    #[test]
    fn an_empty_cascade_plan_does_not_bypass_the_confirmation_gate() {
        let plan = plan_with(Vec::new(), Vec::new());
        assert!(
            plan.has_no_children(),
            "precondition: this is the plan shape that used to skip the gate"
        );
        let message = cascade_confirmation_required(&plan, false)
            .expect("a childless cascade must still require --yes");
        assert!(
            message.contains("enumerated NO channels"),
            "the refusal must say the plan was empty rather than imply children: {message}"
        );
    }

    #[test]
    fn a_non_empty_cascade_plan_requires_confirmation_and_states_the_counts() {
        let plan = plan_with(
            vec![
                channel("c1", "stream"),
                channel("t1", TRANSPORT_CHANNEL_TYPE),
            ],
            vec![CascadeWorkflow {
                workflow_id: "w1".into(),
                channel_id: "c1".into(),
            }],
        );
        let message = cascade_confirmation_required(&plan, false).expect("must require --yes");
        assert!(message.contains("2 channel(s) (1 transport)"), "{message}");
        assert!(message.contains("1 workflow(s)"), "{message}");
    }

    #[test]
    fn yes_is_the_only_thing_that_opens_the_gate() {
        assert!(cascade_confirmation_required(&plan_with(Vec::new(), Vec::new()), true).is_none());
        assert!(cascade_confirmation_required(
            &plan_with(vec![channel("c1", "stream")], Vec::new()),
            true
        )
        .is_none());
    }

    // ── plan_json ────────────────────────────────────────────────────────────

    #[test]
    fn plan_json_reports_counts_per_child_type() {
        let mut plan = plan_with(
            vec![
                channel("c1", "stream"),
                channel("t1", TRANSPORT_CHANNEL_TYPE),
            ],
            vec![CascadeWorkflow {
                workflow_id: "w1".into(),
                channel_id: "c1".into(),
            }],
        );
        plan.shell_sessions = 2;
        plan.coding_sessions = 3;
        plan.unowned_channels = vec![channel("c1", "stream")];
        let value = plan_json(&plan, true);
        assert_eq!(value["dry_run"], json!(true));
        assert_eq!(value["counts"]["channels"], json!(2));
        assert_eq!(value["counts"]["transport_channels"], json!(1));
        assert_eq!(value["counts"]["channels_without_owner_grant"], json!(1));
        assert_eq!(value["counts"]["workflows"], json!(1));
        assert_eq!(value["counts"]["shell_sessions"], json!(2));
        assert_eq!(value["counts"]["coding_sessions"], json!(3));
        assert!(!value["warnings"].as_array().unwrap_or(&vec![]).is_empty());
    }

    #[test]
    fn project_coordinate_uses_kind_30621() {
        assert_eq!(project_coordinate(OWNER, "platform"), COORD);
    }
}
