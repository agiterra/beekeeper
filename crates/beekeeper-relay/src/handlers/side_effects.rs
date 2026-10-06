//! NIP-29 and NIP-25 side-effect handlers.

use std::sync::Arc;

use nostr::{Event, EventBuilder, Kind, Tag};
use tracing::{info, warn};
use uuid::Uuid;

use beekeeper_core::coding_session_authority_transition::decode_coding_session_authority_transition;
use beekeeper_core::coding_session_genesis::decode_coding_session_genesis;
use beekeeper_core::kind::{
    event_kind_u32, is_parameterized_replaceable, KIND_AGENT_PROFILE,
    KIND_CODING_SESSION_AUTHORITY_TRANSITION, KIND_CODING_SESSION_CLOSURE,
    KIND_CODING_SESSION_GENESIS, KIND_DM_VISIBILITY, KIND_GIT_REPO_ANNOUNCEMENT,
    KIND_GIT_REPO_STATE, KIND_IA_ARCHIVED, KIND_IA_ARCHIVED_LIST, KIND_IA_UNARCHIVED,
    KIND_MEMBER_ADDED_NOTIFICATION, KIND_MEMBER_REMOVED_NOTIFICATION, KIND_NIP29_GROUP_ADMINS,
    KIND_NIP29_GROUP_MEMBERS, KIND_NIP29_GROUP_METADATA, KIND_NIP43_MEMBERSHIP_LIST, KIND_PROJECT,
    KIND_REACTION, KIND_SHELL_SESSION, KIND_THREAD_SUMMARY,
};
use beekeeper_core::StoredEvent;
use beekeeper_db::channel::{MemberRecord, MemberRole};
use beekeeper_db::project_acl::ProjectRole;

use super::event::dispatch_persistent_event;
use super::moderation_authz::{authorize_moderation_action, ModerationAction, ModerationTarget};
use crate::protocol::RelayMessage;
use crate::state::AppState;
use beekeeper_core::tenant::TenantContext;
use beekeeper_pubsub::EventTopic;

/// Check if a kind is an admin kind (9000-9022) that needs pre-storage validation.
pub fn is_admin_kind(kind: u32) -> bool {
    // 9010/9011 sit inside the NIP-29 admin range numerically but are
    // NIP-MP project membership ops: project-scoped (`a` tag, no channel),
    // validated by `validate_project_member_op` instead.
    matches!(kind, 9000..=9022) && !beekeeper_core::kind::is_project_membership_kind(kind)
}

/// Check if a kind triggers side effects after storage.
///
/// NOTE: kind:7 (reaction) is intentionally excluded — dedup and DB writes are
/// handled in `ingest_event()` before storage so we can short-circuit on
/// duplicates without storing the event at all.
pub fn is_side_effect_kind(kind: u32) -> bool {
    matches!(
        kind,
        0 | 5
            | 9000..=9022
            | KIND_GIT_REPO_ANNOUNCEMENT
            | KIND_AGENT_PROFILE
            | KIND_PROJECT
            | KIND_CODING_SESSION_AUTHORITY_TRANSITION
            | 41001..=41003
            | 40099
    ) || kind == beekeeper_core::kind::KIND_SHELL_SESSION
}

async fn evict_live_channel_subscriptions(
    tenant: &TenantContext,
    state: &Arc<AppState>,
    channel_id: Uuid,
    target_pubkey: &[u8],
) {
    let conn_ids = state
        .conn_manager
        .connection_ids_for_pubkey_in_community(tenant.community(), target_pubkey);

    for conn_id in conn_ids {
        evict_conn_channel_subscriptions(tenant, state, channel_id, conn_id).await;
    }
}

/// Durably disable a departing member's workflows in the channel (SEC-006).
///
/// A workflow runs with its owner's standing authority; once the owner is no
/// longer a member (removed via kind 9001 or left via kind 9022) their
/// workflows must stop firing on every path — event triggers, the scheduler,
/// manual triggers, and the webhook endpoint all honor `enabled = FALSE`.
/// The per-fire authority gate in `buzz-workflow` is the fail-closed backstop;
/// this makes the revocation durable and immediately visible.
///
/// Failures are logged, not propagated: membership removal has already been
/// committed, and the per-fire gate still denies a removed owner even if this
/// disable write is lost.
async fn disable_departed_member_workflows(
    tenant: &TenantContext,
    state: &Arc<AppState>,
    channel_id: Uuid,
    target_pubkey: &[u8],
) {
    match state
        .db
        .disable_workflows_for_owner_in_channel(tenant.community(), channel_id, target_pubkey)
        .await
    {
        Ok(0) => {}
        Ok(n) => {
            tracing::info!(
                channel = %channel_id,
                owner = %hex::encode(target_pubkey),
                disabled = n,
                "Disabled departed member's workflows"
            );
            state
                .workflow_engine
                .invalidate_channel_workflows(tenant.community(), channel_id);
        }
        Err(e) => {
            warn!(
                channel = %channel_id,
                owner = %hex::encode(target_pubkey),
                error = %e,
                "Failed to disable departed member's workflows — per-fire authority gate still denies"
            );
        }
    }
}

/// Close every live channel-scoped subscription on `conn_id`, removing them from
/// the connection's local map and sending `CLOSED restricted` for each.
async fn evict_conn_channel_subscriptions(
    tenant: &TenantContext,
    state: &Arc<AppState>,
    channel_id: Uuid,
    conn_id: uuid::Uuid,
) {
    let removed = state.sub_registry.remove_channel_subscriptions_scoped(
        tenant.community(),
        conn_id,
        channel_id,
    );
    if removed.is_empty() {
        return;
    }

    if let Some(subscriptions) = state.conn_manager.subscriptions_for(conn_id) {
        let mut conn_subscriptions = subscriptions.lock().await;
        for update in &removed {
            if update.removed {
                conn_subscriptions.remove(&update.sub_id);
            }
        }
    }

    for update in removed {
        state
            .pubsub
            .release_topic(tenant, beekeeper_pubsub::EventTopic::Channel(channel_id))
            .await;
        if update.removed {
            let _ = state.conn_manager.send_to(
                conn_id,
                RelayMessage::closed(&update.sub_id, "restricted: channel access revoked"),
            );
        }
    }
}

/// Revoke live channel subscriptions held by connections whose authenticated
/// pubkey is not a current member. Used when an open channel flips to private:
/// non-members could have subscribed while it was open. Fan-out now re-checks
/// membership per event as the delivery-time safety net; this eviction closes
/// subscriptions promptly so clients stop treating the channel as live.
async fn evict_non_member_channel_subscriptions(
    tenant: &TenantContext,
    state: &Arc<AppState>,
    channel_id: Uuid,
) -> anyhow::Result<()> {
    let members = state.db.get_members(tenant.community(), channel_id).await?;
    let member_pubkeys: std::collections::HashSet<Vec<u8>> =
        members.into_iter().map(|m| m.pubkey).collect();

    for conn_id in state
        .sub_registry
        .channel_subscriber_conns_scoped(tenant.community(), channel_id)
    {
        let is_member = match state.conn_manager.pubkey_for_conn(conn_id) {
            Some(pubkey) => member_pubkeys.contains(&pubkey),
            None => false,
        };
        if !is_member {
            evict_conn_channel_subscriptions(tenant, state, channel_id, conn_id).await;
        }
    }
    Ok(())
}

/// Close every live subscription on a channel, for all subscribers, sending
/// `CLOSED restricted: channel access revoked` to each.
///
/// Used when a channel is archived (e.g. the ephemeral-channel reaper): the
/// channel becomes unusable for everyone, so all live subscriptions must close.
/// The `channel access revoked` reason is in the client's drop-set, so a
/// connected agent drops just that channel and keeps its socket — no reconnect
/// storm. Offline/reconnecting clients are covered by the discovery-time
/// `archived=true` skip in `discover_channels`.
pub async fn evict_all_channel_subscriptions(
    tenant: &TenantContext,
    state: &Arc<AppState>,
    channel_id: Uuid,
) {
    for conn_id in state
        .sub_registry
        .channel_subscriber_conns_scoped(tenant.community(), channel_id)
    {
        evict_conn_channel_subscriptions(tenant, state, channel_id, conn_id).await;
    }
}

/// The `type` this relay stamps on a whole-session deletion receipt.
///
/// Its verifier is `beekeeper_session_provider::authority::verify_deletion_receipt`,
/// which compares this exact string; the two are held together by name here
/// and by the test that asserts it there, the same way
/// `ACCEPTANCE_RECEIPT_TYPE` and the authority receipt above are.
pub const CODING_SESSION_DELETION_RECEIPT_TYPE: &str = "coding_session_deletion_accepted";

/// Dispatch side effects for a stored event.
pub async fn handle_side_effects(
    tenant: &TenantContext,
    kind: u32,
    event: &Event,
    state: &Arc<AppState>,
) -> anyhow::Result<()> {
    match kind {
        0 => handle_kind0_profile(tenant, event, state).await,
        5 => handle_standard_deletion_event(tenant, event, state).await,
        9000 => handle_put_user(tenant, event, state).await,
        9001 => handle_remove_user(tenant, event, state).await,
        9002 => handle_edit_metadata(tenant, event, state).await,
        9005 => handle_delete_event_side_effect(tenant, event, state).await,
        9007 => handle_create_group(tenant, event, state).await,
        9008 => handle_delete_group(tenant, event, state).await,
        9009 => {
            warn!(
                kind = kind,
                "NIP-29 kind 9009 handler deferred to future phase"
            );
            Ok(())
        }
        9021 => handle_join_request(tenant, event, state).await,
        9022 => handle_leave_request(tenant, event, state).await,
        // NIP-34: Git repo announcement → reserve name + seed manifest pointer.
        KIND_GIT_REPO_ANNOUNCEMENT => handle_git_repo_announcement(tenant, event, state).await,
        KIND_AGENT_PROFILE => handle_agent_profile(tenant, event, state).await,
        // NIP-MP Buzz access extension: project the head's access level and
        // invited-member `p` tags into `project_acl` (store+project) so the
        // accessible-channels query can gate private-project channels in SQL.
        KIND_PROJECT => handle_project_acl_projection(tenant, event, state).await,
        // NIP-MP membership ops: apply to the relay-managed roster, then
        // re-project the relay-signed 39010 roster head.
        beekeeper_core::kind::KIND_PROJECT_PUT_MEMBER
        | beekeeper_core::kind::KIND_PROJECT_REMOVE_MEMBER => {
            handle_project_member_op(tenant, kind, event, state).await
        }
        // NIP-ST: project the announce head's roster + status so the
        // ephemeral input/watch gates resolve membership from a row, not
        // from re-parsing the stored head per keystroke.
        beekeeper_core::kind::KIND_SHELL_SESSION => {
            handle_shell_session_acl_projection(tenant, event, state).await
        }
        KIND_CODING_SESSION_AUTHORITY_TRANSITION => {
            handle_coding_session_authority_transition_accepted(tenant, event, state).await
        }
        // kind:7 (reaction) handled inline in ingest_event() before storage.
        _ => Ok(()),
    }
}

/// Refuse a *piecemeal* deletion of coding-session facts whose history is
/// permanent.
///
/// A coding-session genesis (44226) is the answer to "who founded this
/// umbrella", and a closure (44230) is an append-only revision of its shared
/// state. Deleting either **on its own** is refused, and the two reasons are
/// different:
///
/// * A lone genesis deletion would strand every closure that references it —
///   revisions of a session that no longer exists.
/// * A lone closure deletion would silently roll shared state back to an
///   older action without authoring a counter-revision. Sessions change state
///   through another closure, never by erasing the last one.
///
/// What *is* now allowed is deleting a session outright: one kind:5 that
/// names the genesis and every live closure together. Then there is no
/// half-state to be inconsistent about — the whole authority chain goes at
/// once, which is what "delete this session" has to mean if it is to mean
/// anything.
///
/// The original refusal's stated reason was that a genesis deletion "would
/// advertise 'delete your genesis' as a way to re-found a session and then
/// silently not do that". That argument is against *re-founding*, and it
/// still holds: the storage layer's uniqueness probe counts soft-deleted
/// geneses, so a `sessionRef` stays claimed for good and the reference is
/// never released. Deleting a whole session does not hand it back — the same
/// rule repository names already follow, where deletion never frees a name
/// for another owner to squat. Every surface that offers this says so.
///
/// This function is the piecemeal half only. It takes the *set* of kinds a
/// deletion targets so it can tell "the closure alone" from "the closure
/// with its genesis"; the whole-session case is authorized separately by
/// [`authorize_coding_session_deletion`], which also decides who may do it.
///
/// Applies to both deletion paths — NIP-09 `kind:5` and the NIP-29 moderator
/// `kind:9005` — because a moderator is no more able to reassign foundership
/// than an author is. The moderator path deletes one event at a time, so a
/// session deletion is not expressible there at all and every refusal below
/// still fires for it, unchanged.
fn refuse_permanent_identity_deletion(target_kind: u32) -> anyhow::Result<()> {
    if target_kind == KIND_CODING_SESSION_GENESIS {
        return Err(anyhow::anyhow!(
            "coding-session genesis events cannot be deleted on their own — a session's founder \
             is permanent; close the session with a closure revision, or delete the whole \
             session (its genesis and every closure in one deletion)"
        ));
    }
    if target_kind == KIND_CODING_SESSION_CLOSURE {
        return Err(anyhow::anyhow!(
            "coding-session closure events cannot be deleted on their own — reopen or close the \
             session with another closure revision, or delete the whole session (its genesis and \
             every closure in one deletion)"
        ));
    }
    Ok(())
}

/// How many closures one channel may hold before this gate gives up rather
/// than guess. A session accumulates one revision per close or reopen, so a
/// channel reaching this has thousands, and under-counting here would let a
/// partial deletion through as if it were complete.
const MAX_CLOSURES_SCANNED: i64 = 2_000;

/// Every live closure revision of one umbrella, as event ids.
///
/// Closures are grouped by `d = sessionRef` inside `h = channel`, but 44230
/// is a *regular* kind and `extract_d_tag` only populates the indexed
/// `d_tag` column for the NIP-33 range — so the column is NULL for every
/// closure and filtering on it silently matches nothing. (It did, in the
/// first version of this function: the partial-chain test caught a gate that
/// found zero closures and therefore called every chain complete.) The scan
/// is therefore by channel, with the `d` tag matched here.
///
/// Fails closed on a channel too large to scan: an under-count would let a
/// partial deletion through as a complete one, which is the exact outcome
/// this whole gate exists to prevent.
async fn live_closure_ids(
    tenant: &TenantContext,
    state: &Arc<AppState>,
    channel_id: Uuid,
    session_ref: &str,
) -> anyhow::Result<Vec<Vec<u8>>> {
    let rows = state
        .db
        .query_events(&beekeeper_db::event::EventQuery {
            kinds: Some(vec![KIND_CODING_SESSION_CLOSURE as i32]),
            channel_id: Some(channel_id),
            max_limit: Some(MAX_CLOSURES_SCANNED),
            limit: Some(MAX_CLOSURES_SCANNED),
            ..beekeeper_db::event::EventQuery::for_community(tenant.community())
        })
        .await?;
    if rows.len() as i64 >= MAX_CLOSURES_SCANNED {
        return Err(anyhow::anyhow!(
            "this channel holds too many coding-session closure revisions to verify a complete \
             session deletion against"
        ));
    }
    Ok(rows
        .into_iter()
        .filter(|stored| {
            stored.event.tags.iter().any(|tag| {
                let parts = tag.as_slice();
                parts.len() >= 2 && parts[0] == "d" && parts[1] == session_ref
            })
        })
        .map(|stored| stored.event.id.to_bytes().to_vec())
        .collect())
}

/// Authorize a whole-session deletion, or refuse it as piecemeal.
///
/// Returns `Ok(Some(session))` when this deletion is a complete session
/// delete that the actor may perform — the caller then skips
/// [`refuse_permanent_identity_deletion`] for its targets, and uses the
/// returned session to check that each one actually belongs to it. Returns
/// `Ok(None)` when no genesis is targeted at all, which is every ordinary
/// deletion and costs one pass over an already-loaded list.
///
/// Three conditions, all required:
///
/// 1. **Exactly one session.** A deletion naming two geneses is refused
///    rather than half-applied; batching two irreversible session deletes
///    into one event buys nothing and makes a partial failure ambiguous.
/// 2. **The chain is complete.** Every live closure for that `sessionRef`
///    must be named by the same deletion. Otherwise the survivors are
///    revisions of a session that no longer exists.
/// 3. **The actor is the founder or a project Owner.** The founder is the
///    genesis signer — the payload deliberately never restates it, because
///    the signature already settles it. A project Owner reaches it through
///    the session's channel, which carries the project coordinate; this is
///    the same `get_project_role_by_coordinate` lookup the git ACL and the
///    addressable-deletion path use, so a session's delete rule is its
///    project's rule and not a second one.
async fn authorize_coding_session_deletion(
    tenant: &TenantContext,
    state: &Arc<AppState>,
    targets: &[StoredEvent],
    actor_bytes: &[u8],
) -> anyhow::Result<Option<AuthorizedSessionDeletion>> {
    let geneses: Vec<&StoredEvent> = targets
        .iter()
        .filter(|stored| event_kind_u32(&stored.event) == KIND_CODING_SESSION_GENESIS)
        .collect();
    let Some(genesis) = geneses.first().copied() else {
        return Ok(None);
    };
    if geneses.len() > 1 {
        return Err(anyhow::anyhow!(
            "a deletion may name at most one coding-session genesis — delete one session at a time"
        ));
    }

    let payload = decode_coding_session_genesis(&genesis.event.content)
        .map_err(|e| anyhow::anyhow!("coding-session genesis is undecodable: {e}"))?;
    let channel_id = extract_h_tag_channel(&genesis.event).ok_or_else(|| {
        anyhow::anyhow!(
            "coding-session genesis carries no channel, so its session cannot be resolved"
        )
    })?;

    // Condition 2: the chain must go whole.
    let named: std::collections::HashSet<Vec<u8>> = targets
        .iter()
        .map(|stored| stored.event.id.to_bytes().to_vec())
        .collect();
    let closures = live_closure_ids(tenant, state, channel_id, &payload.session_ref).await?;
    let missing = closures.iter().filter(|id| !named.contains(*id)).count();
    if missing > 0 {
        return Err(anyhow::anyhow!(
            "deleting this session must delete its closure revisions too — {missing} of them are \
             not named by this deletion, and would be left describing a session that no longer \
             exists"
        ));
    }

    // The executions this deletion's own metadata attributes to the umbrella.
    // Drawn from the named targets, never from a channel scan: a transcript is
    // deletable here because the metadata that binds it to this session is
    // going too, and one that is not named binds nothing.
    let execution_targets: std::collections::HashSet<String> = targets
        .iter()
        .filter(|stored| {
            event_kind_u32(&stored.event) == beekeeper_core::kind::KIND_CODING_SESSION_METADATA
                && content_session_ref(&stored.event.content).as_deref()
                    == Some(payload.session_ref.as_str())
        })
        .filter_map(|stored| extract_tag_value(&stored.event, "cs-target"))
        .collect();
    let session = AuthorizedSessionDeletion {
        session_ref: payload.session_ref.clone(),
        channel_id,
        genesis_id: genesis.event.id.to_bytes().to_vec(),
        execution_targets,
    };

    // Condition 3: founder, or an Owner of the containing project.
    let founder = effective_message_author(&genesis.event, &state.relay_keypair.public_key());
    if founder == actor_bytes {
        return Ok(Some(session));
    }
    let channel = state.db.get_channel(tenant.community(), channel_id).await?;
    if let Some(coordinate) = channel.project_ref.as_deref() {
        let role = state
            .db
            .get_project_role_by_coordinate(tenant.community(), coordinate, actor_bytes)
            .await?;
        if role == Some(ProjectRole::Owner) {
            return Ok(Some(session));
        }
    }
    Err(anyhow::anyhow!(
        "only this session's founder, or an owner of the project it belongs to, may delete it"
    ))
}

/// The project coordinate that governs an addressable deletion target, if any.
///
/// Only the three kinds a project actually contains resolve here. Everything
/// else — workflows above all, which keep their own owner-scoped delete path
/// in [`handle_a_tag_deletion`] — returns `None` and falls back to plain
/// authorship.
///
/// The returned coordinate is always normalized (`30621:<lowercase-hex>:<d>`):
/// the project head's is rebuilt from the decoded a-tag bytes, and the repo
/// and terminal links are normalized on write (`set_repo_project_ref`,
/// `upsert_shell_session_acl`). That matters because `project_acl.coordinate`
/// is compared by string equality with no per-row parsing.
async fn governing_project_coordinate(
    tenant: &TenantContext,
    state: &Arc<AppState>,
    target_kind: u32,
    target_pubkey: &[u8],
    target_dtag: &str,
) -> anyhow::Result<Option<String>> {
    if target_dtag.is_empty() {
        return Ok(None);
    }
    match target_kind {
        KIND_PROJECT => Ok(Some(format!(
            "{KIND_PROJECT}:{}:{target_dtag}",
            hex::encode(target_pubkey)
        ))),
        KIND_GIT_REPO_ANNOUNCEMENT => Ok(state
            .db
            .get_repo_project_ref(tenant.community(), target_dtag)
            .await?),
        KIND_SHELL_SESSION => Ok(state
            .db
            .get_shell_roster(tenant.community(), target_pubkey, target_dtag)
            .await?
            .map(|roster| roster.coordinate)),
        _ => Ok(None),
    }
}

/// Whether `actor_bytes` may delete an addressable target because they are an
/// Owner of the project that contains it.
///
/// This is the one place a deletion is authorized by *role* rather than by
/// signature, and it is deliberately narrow:
///
/// * Only [`ProjectRole::Owner`]. A Collaborator deletes their own work
///   through the authorship arm above and nothing else; a Viewer deletes
///   nothing.
/// * Only the three project-scoped addressable kinds
///   ([`governing_project_coordinate`]).
/// * Fails closed. An unresolvable project, an unlinked repo, an unprojected
///   terminal — all return `false`, leaving the caller's `must be event
///   author` refusal exactly as it was.
///
/// [`Db::get_project_role_by_coordinate`] treats the project creator as an
/// implicit Owner, which is the whole point: creator and roster Owner reach
/// this gate as the same tier.
async fn project_owner_admits_deletion(
    tenant: &TenantContext,
    state: &Arc<AppState>,
    target_kind: u32,
    target_pubkey: &[u8],
    target_dtag: &str,
    actor_bytes: &[u8],
) -> anyhow::Result<bool> {
    let Some(coordinate) =
        governing_project_coordinate(tenant, state, target_kind, target_pubkey, target_dtag)
            .await?
    else {
        return Ok(false);
    };
    let role = state
        .db
        .get_project_role_by_coordinate(tenant.community(), &coordinate, actor_bytes)
        .await?;
    Ok(role == Some(ProjectRole::Owner))
}

/// What a `kind:5` turned out to be, once it was allowed through.
///
/// The distinction exists for exactly one caller: ingest enforces NIP-09's
/// single target, and a complete coding-session delete is many targets by
/// construction. Reporting the shape here rather than re-deriving it there
/// keeps one answer to "is this a session delete?" instead of two that can
/// disagree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeletionShape {
    /// The ordinary case: one event, or one addressable coordinate.
    SingleTarget,
    /// A session's genesis and everything under it, in one event.
    WholeCodingSession,
}

/// Validate a standard NIP-09 deletion event before it is stored.
///
/// Buzz accepts standard deletions for self-authored events, plus the owning
/// human deleting their agent's events (mirrors `validate_edit_ownership`),
/// plus — on the addressable `a`-tag path only — an Owner of the project that
/// contains the target (see [`project_owner_admits_deletion`]).
/// Channel admin deletions continue to use kind 9005.
pub async fn validate_standard_deletion_event(
    tenant: &TenantContext,
    event: &Event,
    state: &Arc<AppState>,
) -> anyhow::Result<DeletionShape> {
    let actor_bytes = effective_message_author(event, &state.relay_keypair.public_key());
    let target_ids = extract_target_event_ids(event);

    if !has_e_tag(event) {
        // a-tag deletion: verify author owns the addressable event
        let a_tag = event
            .tags
            .iter()
            .find(|t| t.kind().to_string() == "a")
            .and_then(|t| t.content().map(|s| s.to_string()))
            .ok_or_else(|| anyhow::anyhow!("missing e or a tag for target"))?;
        let parts: Vec<&str> = a_tag.splitn(3, ':').collect();
        if parts.len() < 2 {
            return Err(anyhow::anyhow!("invalid a-tag format"));
        }
        let target_pubkey_bytes =
            hex::decode(parts[1]).map_err(|_| anyhow::anyhow!("invalid pubkey in a-tag"))?;
        if target_pubkey_bytes == actor_bytes
            || state
                .db
                .is_agent_owner(tenant.community(), &target_pubkey_bytes, &actor_bytes)
                .await?
        {
            return Ok(DeletionShape::SingleTarget);
        }
        // Third arm: an Owner of the project that contains this target. The
        // kind is parsed only here — the two arms above never needed it, and
        // a coordinate whose kind is not a number cannot name a project
        // resource, so it falls through to the same refusal as before.
        let target_kind: u32 = parts[0].parse().unwrap_or(0);
        let target_dtag = parts.get(2).copied().unwrap_or("");
        if project_owner_admits_deletion(
            tenant,
            state,
            target_kind,
            &target_pubkey_bytes,
            target_dtag,
            &actor_bytes,
        )
        .await?
        {
            return Ok(DeletionShape::SingleTarget);
        }
        return Err(anyhow::anyhow!("must be event author"));
    }

    // Every target is resolved before any is judged, because a coding-session
    // deletion is only legible as a whole: whether a closure may go depends on
    // whether its genesis goes with it, and that is a property of the target
    // *set*, not of any one target.
    let mut targets = Vec::with_capacity(target_ids.len());
    for target_id in target_ids {
        targets.push(
            state
                .db
                .get_event_by_id_including_deleted(tenant.community(), &target_id)
                .await?
                .ok_or_else(|| anyhow::anyhow!("target event not found"))?,
        );
    }

    // A complete session delete is authorized here, by founder-or-project-Owner
    // rather than by authorship of each event — the transcript items and
    // metadata under a session are signed by the provider and the agent seats,
    // not by the person deleting it, so an authorship-only rule could never
    // express "delete this session".
    let whole_session =
        authorize_coding_session_deletion(tenant, state, &targets, &actor_bytes).await?;

    for target_event in &targets {
        // Is this target part of the session that was just authorized? Both
        // exemptions below hang off this one answer, and neither may hang off
        // the kind alone.
        let in_this_session = whole_session
            .as_ref()
            .is_some_and(|session| session_deletion_admits(&target_event.event, session));

        // Checked before authorship: being the founder is not permission to
        // stop being the founder, so this refusal must not be reachable by
        // simply having signed the target. Skipped only for the events of the
        // session actually being deleted — a genesis or a closure from
        // *another* session is still a piecemeal deletion of that session,
        // even inside an event that legitimately deletes this one, and even
        // when the same person signed both.
        if !in_this_session {
            refuse_permanent_identity_deletion(event_kind_u32(&target_event.event))?;
        }

        let target_author =
            effective_message_author(&target_event.event, &state.relay_keypair.public_key());
        if target_author == actor_bytes
            || state
                .db
                .is_agent_owner(tenant.community(), &target_author, &actor_bytes)
                .await?
        {
            continue;
        }
        // A session's own events are signed by whoever produced them — the
        // provider, an agent seat, the founder — so a session delete carries
        // targets its actor never authored. That is exactly what the
        // whole-session authorization above decided, and re-deciding it by
        // authorship here would make the feature unusable by design.
        if in_this_session {
            continue;
        }
        if whole_session.is_some()
            && coding_session_scoped_kind(event_kind_u32(&target_event.event))
        {
            // Named a session event, but not one of *this* session's. Its own
            // sentence rather than "must be event author", which would be a
            // true statement about the wrong problem: the actor is authorized
            // here, they have simply named somebody else's.
            return Err(anyhow::anyhow!(
                "this deletion names a coding-session event that does not belong to the \
                 session it deletes — a session delete reaches its own events only"
            ));
        }
        return Err(anyhow::anyhow!("must be event author"));
    }

    Ok(match whole_session {
        Some(_) => DeletionShape::WholeCodingSession,
        None => DeletionShape::SingleTarget,
    })
}

/// Kinds a complete session deletion is allowed to reach without authorship.
///
/// Deliberately a closed list rather than "anything in the session's channel":
/// a session delete must not become a way to delete a teammate's chat messages
/// by naming them alongside a genesis.
///
/// The kind is only half the admission. Being a 44227 does not make an event
/// *this* session's 44227, and [`session_deletion_admits`] settles that half —
/// see the note there for why both halves are load-bearing.
fn coding_session_scoped_kind(kind: u32) -> bool {
    matches!(
        kind,
        KIND_CODING_SESSION_GENESIS
            | KIND_CODING_SESSION_CLOSURE
            | beekeeper_core::kind::KIND_CODING_SESSION_METADATA
            | beekeeper_core::kind::KIND_CODING_SESSION_TRANSCRIPT
            | beekeeper_core::kind::KIND_CODING_SESSION_CHECKPOINT
            | beekeeper_core::kind::KIND_CODING_SESSION_GOAL
            | beekeeper_core::kind::KIND_CODING_SESSION_NAME
            | beekeeper_core::kind::KIND_CODING_SESSION_GENERATED_TITLE
            | beekeeper_core::kind::KIND_CODING_SESSION_TEAM_TRANSACTION
            | beekeeper_core::kind::KIND_CODING_SESSION_LIFECYCLE_RECEIPT
    )
}

/// Kinds that carry their umbrella in a plain `d` tag.
const SESSION_D_TAG_KINDS: &[u32] = &[
    KIND_CODING_SESSION_CLOSURE,
    beekeeper_core::kind::KIND_CODING_SESSION_GOAL,
    beekeeper_core::kind::KIND_CODING_SESSION_NAME,
    beekeeper_core::kind::KIND_CODING_SESSION_GENERATED_TITLE,
    beekeeper_core::kind::KIND_CODING_SESSION_TEAM_TRANSACTION,
];

/// The `sessionRef` an event's JSON content claims, if any.
///
/// A deliberately loose read rather than the strict decoders: this asks which
/// umbrella an event belongs to, not whether the payload is well-formed. A
/// metadata event written by a newer build, with a key this relay has never
/// heard of, still belongs to the session it names — refusing to see that
/// would turn a legitimate delete into a refusal nobody could act on.
fn content_session_ref(content: &str) -> Option<String> {
    let parsed: serde_json::Value = serde_json::from_str(content).ok()?;
    parsed.get("sessionRef")?.as_str().map(str::to_string)
}

/// One session a deletion has been authorized to remove, and everything
/// needed to tell that session's events from anybody else's.
struct AuthorizedSessionDeletion {
    session_ref: String,
    channel_id: Uuid,
    genesis_id: Vec<u8>,
    /// `cs-target` keys of the executions the *named* metadata attributed to
    /// this umbrella. A transcript reaches its session in two hops and this
    /// is the first one; an execution whose metadata is not in the same
    /// deletion contributes no transcript, which is correct — nothing in this
    /// event ties one to this session.
    execution_targets: std::collections::HashSet<String>,
}

/// Does this target actually belong to the session being deleted?
///
/// The exemption above is by kind, and a kind is not an identity. Without
/// this, "delete session A" would also delete any 44227, 44229, 44252, 44230,
/// 44223, 44225, 44231 or 44244 named alongside it — including another project's,
/// signed by someone else, in a channel the actor has no standing in. That
/// was unreachable only for as long as the ingest gate refused every
/// multi-target `kind:5`; admitting the session shape is what makes it
/// reachable, so the two changes belong together.
///
/// The linkages are the ones both clients select by, and there is no single
/// grouping tag:
///
/// | Kind | Belongs by |
/// |---|---|
/// | 44226 genesis | being *the* genesis this deletion was authorized against |
/// | 44230, 44227, 44229, 44252, 44244 | `["d", sessionRef]` |
/// | 44223 metadata | `sessionRef` in its content |
/// | 44225 transcript | a `cs-target` some named metadata attributed here |
/// | 44231 checkpoint | the same `cs-target` set — it names its execution as a transcript does |
/// | 44224 lifecycle receipt | the target key in its payload, same set |
///
/// The channel is checked first and for every kind. `kind:5` derives its
/// channel from its *first* `e` tag, so the membership, token and archived
/// gates upstream only ever see one channel; a target outside it would be
/// judged against a channel it does not live in.
fn session_deletion_admits(event: &Event, session: &AuthorizedSessionDeletion) -> bool {
    if extract_h_tag_channel(event) != Some(session.channel_id) {
        return false;
    }
    let kind = event_kind_u32(event);
    if kind == KIND_CODING_SESSION_GENESIS {
        return event.id.to_bytes().as_slice() == session.genesis_id.as_slice();
    }
    if SESSION_D_TAG_KINDS.contains(&kind) {
        return extract_tag_value(event, "d").as_deref() == Some(session.session_ref.as_str());
    }
    if kind == beekeeper_core::kind::KIND_CODING_SESSION_METADATA {
        return content_session_ref(&event.content).as_deref()
            == Some(session.session_ref.as_str());
    }
    // A checkpoint carries repo-relative paths, branch names and head SHAs of
    // the session's working tree. Leaving it behind would tell the person the
    // session was deleted while its file list stayed readable on the relay.
    if kind == beekeeper_core::kind::KIND_CODING_SESSION_TRANSCRIPT
        || kind == beekeeper_core::kind::KIND_CODING_SESSION_CHECKPOINT
    {
        return extract_tag_value(event, "cs-target")
            .is_some_and(|target| session.execution_targets.contains(&target));
    }
    if kind == beekeeper_core::kind::KIND_CODING_SESSION_LIFECYCLE_RECEIPT {
        // The only one of these kinds with no `cs-target` tag: a receipt
        // names its execution inside the payload instead.
        let Ok(receipt) =
            beekeeper_core::coding_session_payload::decode_coding_session_lifecycle_receipt(
                &event.content,
            )
        else {
            return false;
        };
        return receipt.session.is_some_and(|target| {
            session.execution_targets.contains(
                &beekeeper_core::coding_session_command::coding_session_target_key(&target),
            )
        });
    }
    false
}

/// Returns `true` if `actor_bytes` is the NIP-OA owner of **any** active owner-role
/// member in `members`. Used by kind:9002 and kind:9008 to authorize the owning
/// human of a channel's agent-owner(s) even when the human is not a channel member.
///
/// Checking all active owners (not just the first) is correct under co-ownership:
/// an agent may be promoted to owner later, or multiple agents may co-own a channel.
async fn actor_owns_any_owner_agent(
    state: &Arc<AppState>,
    community_id: beekeeper_core::CommunityId,
    members: &[beekeeper_db::channel::MemberRecord],
    actor_bytes: &[u8],
) -> anyhow::Result<bool> {
    for owner in members.iter().filter(|m| m.role == "owner") {
        if state
            .db
            .is_agent_owner(community_id, &owner.pubkey, actor_bytes)
            .await?
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Validate an admin kind event BEFORE storage.
pub async fn validate_admin_event(
    tenant: &TenantContext,
    kind: u32,
    event: &Event,
    state: &Arc<AppState>,
) -> anyhow::Result<()> {
    // CREATE_GROUP doesn't need an existing channel — skip h-tag extraction
    if kind == 9007 {
        return Ok(());
    }

    // Extract channel from h tag
    let channel_id =
        extract_h_tag_channel(event).ok_or_else(|| anyhow::anyhow!("missing or invalid h tag"))?;

    let actor_bytes = event.pubkey.to_bytes().to_vec();

    // Reject mutations on archived channels — except kind:9002 with archived=false
    // (unarchive), which must be allowed through so the channel can be restored.
    let channel = state
        .db
        .get_channel(tenant.community(), channel_id)
        .await
        .map_err(|_| anyhow::anyhow!("channel not found"))?;
    let is_unarchive_request = kind == 9002
        && event.tags.iter().any(|t| {
            let parts = t.as_slice();
            parts.len() >= 2 && parts[0] == "archived" && parts[1] == "false"
        });
    if channel.archived_at.is_some() && !is_unarchive_request {
        return Err(anyhow::anyhow!("channel is archived"));
    }

    match kind {
        9000 => {
            // An absent role tag means "no role change requested": for an existing
            // member that preserves the role they already hold, and only defaults
            // to Member for a genuinely new member. Defaulting unconditionally to
            // Member made a bare self-targeted PUT_USER silently demote an owner.
            let role_str = extract_tag_value(event, "role");
            let requested_role = match role_str {
                Some(ref s) => match s.parse::<beekeeper_db::channel::MemberRole>() {
                    Ok(r) => Some(r),
                    Err(_) => return Err(anyhow::anyhow!("invalid role: {s}")),
                },
                None => None,
            };

            let members = state.db.get_members(tenant.community(), channel_id).await?;
            let actor_role: Option<beekeeper_db::channel::MemberRole> = members
                .iter()
                .find(|m| m.pubkey == actor_bytes)
                .and_then(|m| m.role.parse().ok());
            let target_pubkey =
                extract_p_tag(event).ok_or_else(|| anyhow::anyhow!("missing p tag"))?;

            // PUT_USER: open channels allow any authenticated user; private channels
            // require the actor to be an existing active member. Any active member may
            // add an ordinary member, guest, or bot, but only owners/admins may grant
            // an elevated role.
            //
            // Session-transport channels extend the private arm: a project
            // member admitted through the transport gate has member-level
            // authority even without a channel_members row — they must be
            // able to add their own provider bot to a transport channel some
            // other member created. Member-level only: the elevated-role
            // check below still requires a real elevated channel role, and a
            // gate lookup failure fails closed to the members-only rule.
            if channel.visibility == "private" {
                let transport_admitted = if actor_role.is_none() {
                    state
                        .channel_transport_gate_cached(tenant.community(), channel_id)
                        .await
                        .ok()
                        .flatten()
                        .is_some_and(|gate| gate.admits_write(&actor_bytes))
                } else {
                    false
                };
                if actor_role.is_none() && !transport_admitted {
                    return Err(anyhow::anyhow!("actor not authorized"));
                }

                if requested_role.is_some_and(|role| role.is_elevated())
                    && !actor_role.is_some_and(|role| role.is_elevated())
                {
                    return Err(anyhow::anyhow!(
                        "only owners/admins may grant elevated roles"
                    ));
                }
            }

            // Changing an ACTIVE existing member's role is privileged in both
            // directions, on every visibility. `get_members` filters
            // `removed_at IS NULL`, so a soft-removed row is deliberately not an
            // "existing member" here: its stored role is history, not live
            // authority, and reactivation is governed by the elevated-granter
            // check above rather than by the role the row remembers.
            //
            // `add_member` is the authority (it also covers the desktop/admin
            // callers that skip this validator); rejecting here too means the
            // client gets a real error instead of an OK for an event whose side
            // effect then fails. Re-adding at the same role stays idempotent —
            // the huddle bot-add path relies on that.
            if let Some((target, role)) = members
                .iter()
                .find(|m| m.pubkey == target_pubkey)
                .zip(requested_role)
                .filter(|(m, role)| m.role != role.as_str())
            {
                if !actor_role.is_some_and(|r| r.is_elevated()) {
                    return Err(anyhow::anyhow!(
                        "only owners/admins may change an active member's role"
                    ));
                }
                if target.role == "owner"
                    && role != beekeeper_db::channel::MemberRole::Owner
                    && members.iter().filter(|m| m.role == "owner").count() <= 1
                {
                    return Err(anyhow::anyhow!(
                        "cannot demote the last owner — transfer ownership first"
                    ));
                }
            }

            // Self-add: always allowed regardless of policy.
            if target_pubkey == actor_bytes {
                return Ok(());
            }

            // Third-party add: check channel_add_policy on the target.
            if let Some((policy, owner)) = state
                .db
                .get_agent_channel_policy(tenant.community(), &target_pubkey)
                .await?
            {
                match policy.as_str() {
                    "owner_only" => {
                        let owner_bytes = owner.ok_or_else(|| {
                            anyhow::anyhow!("policy:owner_only — agent has no owner set")
                        })?;
                        if actor_bytes != owner_bytes {
                            return Err(anyhow::anyhow!(
                                "policy:owner_only — only the agent owner can add this agent"
                            ));
                        }
                    }
                    "nobody" => {
                        return Err(anyhow::anyhow!(
                            "policy:nobody — this agent has disabled external channel additions"
                        ));
                    }
                    // "anyone" or any unknown value → allow.
                    // NOTE: DB ENUM constraint prevents unknown values from being stored.
                    // If a new policy value is added to the ENUM, update this match.
                    _ => {}
                }
            }

            Ok(())
        }
        9001 => {
            // REMOVE_USER: self-remove allowed unless actor is the last owner; removing others requires owner/admin
            let target_pubkey =
                extract_p_tag(event).ok_or_else(|| anyhow::anyhow!("missing p tag"))?;
            if target_pubkey == actor_bytes {
                // Self-removal: must be an active member, and cannot be the last owner.
                let members = state.db.get_members(tenant.community(), channel_id).await?;
                let actor_member = members.iter().find(|m| m.pubkey == actor_bytes);
                match actor_member {
                    None => {
                        return Err(anyhow::anyhow!("actor is not an active member"));
                    }
                    Some(m) if m.role == "owner" => {
                        let owner_count = members.iter().filter(|m| m.role == "owner").count();
                        if owner_count <= 1 {
                            return Err(anyhow::anyhow!("cannot remove the last owner"));
                        }
                    }
                    _ => {}
                }
                Ok(())
            } else {
                let members = state.db.get_members(tenant.community(), channel_id).await?;
                let actor_member = members.iter().find(|m| m.pubkey == actor_bytes);
                match actor_member {
                    Some(m) if m.role == "owner" || m.role == "admin" => Ok(()),
                    Some(_) => {
                        if state
                            .db
                            .is_agent_owner(tenant.community(), &target_pubkey, &actor_bytes)
                            .await?
                        {
                            Ok(())
                        } else {
                            Err(anyhow::anyhow!("actor not authorized"))
                        }
                    }
                    // Non-members fall here. We intentionally do NOT check
                    // is_agent_owner for non-members — you must be in the channel
                    // to remove anyone, even your own bot.
                    _ => Err(anyhow::anyhow!("actor not authorized")),
                }
            }
        }
        9002 => {
            // EDIT_METADATA: require at least one recognized metadata tag.
            const RECOGNIZED_TAGS: &[&str] = &[
                "name",
                "about",
                "archived",
                "topic",
                "purpose",
                "visibility",
                "ttl",
                "project",
            ];
            let has_recognized = event
                .tags
                .iter()
                .any(|t| RECOGNIZED_TAGS.contains(&t.kind().to_string().as_str()));
            if !has_recognized {
                return Err(anyhow::anyhow!(
                    "kind:9002 must include at least one metadata tag (name, about, archived, topic, purpose, visibility, ttl, project)"
                ));
            }

            // Validate archived values before storage.
            for t in event.tags.iter() {
                if t.kind().to_string() == "archived" {
                    match t.content() {
                        Some("true") | Some("false") => {}
                        Some(v) => {
                            return Err(anyhow::anyhow!(
                                "invalid archived value: {v} (must be \"true\" or \"false\")"
                            ));
                        }
                        None => {
                            return Err(anyhow::anyhow!("archived tag must have a value"));
                        }
                    }
                }
            }

            // Validate channel names before storage. A name made entirely of
            // display-prefix hashes becomes empty after canonicalization.
            for t in event.tags.iter() {
                if t.kind().to_string() == "name" {
                    match t.content() {
                        Some(v)
                            if !beekeeper_core::channel::canonical_channel_name(v)
                                .trim()
                                .is_empty() => {}
                        _ => {
                            return Err(anyhow::anyhow!("channel name is required"));
                        }
                    }
                }
            }

            // Validate visibility values before storage.
            for t in event.tags.iter() {
                if t.kind().to_string() == "visibility" {
                    match t.content() {
                        Some("open") | Some("private") => {}
                        Some(v) => {
                            return Err(anyhow::anyhow!(
                                "invalid visibility value: {v} (must be \"open\" or \"private\")"
                            ));
                        }
                        None => {
                            return Err(anyhow::anyhow!("visibility tag must have a value"));
                        }
                    }
                }
            }

            // Validate ttl values before storage. Empty string clears the TTL
            // (channel becomes permanent); any other value must parse as a
            // positive integer number of seconds. A bare tag with no value is
            // rejected so clearing is always explicit (`["ttl", ""]`).
            for t in event.tags.iter() {
                if t.kind().to_string() == "ttl" {
                    match t.content() {
                        Some("") => {}
                        Some(v) => match v.parse::<i32>() {
                            Ok(n) if n > 0 => {}
                            _ => {
                                return Err(anyhow::anyhow!(
                                    "invalid ttl value: {v} (must be a positive integer of seconds, or empty to clear)"
                                ));
                            }
                        },
                        None => {
                            return Err(anyhow::anyhow!(
                                "ttl tag must have a value (seconds, or empty string to clear)"
                            ));
                        }
                    }
                }
            }

            // Validate the project tag before storage. Empty string clears the
            // project association (removes the channel from any project); any
            // other value must be a well-formed `30621:pubkey:slug` coordinate.
            // Shares its shape validator with the kind:9007 create-time check.
            for t in event.tags.iter() {
                if t.kind().to_string() == "project" {
                    match t.content() {
                        Some("") => {}
                        Some(v) => {
                            super::ingest::validate_project_ref_tag(v)
                                .map_err(|e| anyhow::anyhow!("invalid project tag: {e}"))?;
                        }
                        None => {
                            return Err(anyhow::anyhow!(
                                "project tag must have a value (coordinate, or empty string to clear)"
                            ));
                        }
                    }
                }
            }

            // name/about/archived/visibility/ttl/project require owner/admin;
            // topic/purpose allow any member.
            let has_privileged_tag = event.tags.iter().any(|t| {
                let k = t.kind().to_string();
                k == "name"
                    || k == "about"
                    || k == "archived"
                    || k == "visibility"
                    || k == "ttl"
                    || k == "project"
            });
            if has_privileged_tag {
                let members = state.db.get_members(tenant.community(), channel_id).await?;
                let actor_member = members.iter().find(|m| m.pubkey == actor_bytes);
                match actor_member {
                    Some(m) if m.role == "owner" || m.role == "admin" => Ok(()),
                    _ => {
                        // Allow the owning human of any active owner-role agent in the
                        // channel, even when the human is not a channel member —
                        // diverges from kind:9001 intentionally.
                        if actor_owns_any_owner_agent(
                            state,
                            tenant.community(),
                            &members,
                            &actor_bytes,
                        )
                        .await?
                        {
                            return Ok(());
                        }
                        Err(anyhow::anyhow!(
                            "actor not authorized for name/about/archived/visibility/ttl/project changes"
                        ))
                    }
                }
            } else {
                // topic/purpose: any member
                let is_member = state
                    .is_member_cached(tenant.community(), channel_id, &actor_bytes)
                    .await?;
                if is_member {
                    Ok(())
                } else {
                    Err(anyhow::anyhow!("not a member"))
                }
            }
        }
        9005 => {
            // DELETE_EVENT: event author OR channel owner/admin.
            if let Some(action_id) = extract_tag_value(event, "action_id") {
                Uuid::parse_str(&action_id)
                    .map_err(|_| anyhow::anyhow!("invalid action_id tag"))?;
            }

            // Extract target event from e tag to check authorship.
            let target_id = event
                .tags
                .iter()
                .find_map(|tag| {
                    if tag.kind().to_string() == "e" {
                        tag.content().and_then(|v| hex::decode(v).ok())
                    } else {
                        None
                    }
                })
                .ok_or_else(|| anyhow::anyhow!("missing e tag for target event"))?;

            // Verify the target event exists and belongs to the h-tag channel
            // BEFORE storage. Fail closed: missing target → reject.
            let target_event = state
                .db
                .get_event_by_id(tenant.community(), &target_id)
                .await
                .map_err(|e| anyhow::anyhow!("db error looking up target: {e}"))?
                .ok_or_else(|| anyhow::anyhow!("target event not found"))?;

            // Before the owner/admin branch: a moderator cannot reassign a
            // session's foundership any more than its author can.
            refuse_permanent_identity_deletion(event_kind_u32(&target_event.event))?;

            match target_event.channel_id {
                Some(target_ch) if target_ch != channel_id => {
                    return Err(anyhow::anyhow!(
                        "target event belongs to a different channel"
                    ));
                }
                None => {
                    return Err(anyhow::anyhow!("target event has no channel"));
                }
                _ => {} // Same channel — OK
            }

            // Check if actor is the event author.
            // For relay-signed REST messages, the real author is in the p tag.
            let author =
                effective_message_author(&target_event.event, &state.relay_keypair.public_key());
            if author_delete_can_use_self_delete_path(&author, &actor_bytes, event) {
                // Author deleting their own message: re-gate on membership/open visibility so that
                // a removed private-channel member cannot mutate old messages after access is revoked.
                let is_member = state
                    .is_member_cached(tenant.community(), channel_id, &actor_bytes)
                    .await?;
                if is_member {
                    return Ok(());
                }
                let is_open = state
                    .db
                    .get_channel(tenant.community(), channel_id)
                    .await
                    .map(|ch| ch.visibility == "open")
                    .unwrap_or(false);
                if is_open {
                    return Ok(());
                }
                // Not a member and channel is private — fall through to owner/admin/owner-of-agent check.
            }

            // Not the author, or author who is no longer a member of a private channel —
            // must hold moderation authority or be the owning human of the message's
            // agent-author. Authority routes through the single moderation seam, so a
            // community owner/admin who never joined this channel is authorized here
            // exactly as they are for kind:9001 — strictly wider than the previous
            // `channel_members`-only check, which this call subsumes (`ChannelRole`).
            match authorize_moderation_action(
                tenant,
                state,
                &actor_bytes,
                Some(channel_id),
                ModerationTarget::Event(&target_id),
                ModerationAction::DeleteMessage,
            )
            .await
            {
                Ok(authority) => {
                    // The authority is the *why* behind an accepted moderator
                    // delete, and the tombstone this event produces names only
                    // the actor. kind:9005 writes no `moderation_actions` row:
                    // this is a pre-storage validator, and a row written here
                    // would claim a delete that a later storage failure never
                    // performed (the tombstone is emitted only after the
                    // soft-delete lands, in `handle_delete_event`). Until the
                    // authority is threaded through to that post-storage seam,
                    // this log line is the record.
                    info!(
                        actor = %hex::encode(&actor_bytes),
                        channel_id = %channel_id,
                        target_event = %hex::encode(&target_id),
                        authority = ?authority,
                        "kind:9005 moderator delete authorized"
                    );
                    Ok(())
                }
                Err(e) => {
                    // Denied (or the role/channel lookup failed — fail closed
                    // either way). Allow the owning human of the agent that
                    // authored the target message, even when the human holds no
                    // role at all: the moderation seam intentionally does not
                    // model NIP-OA ownership.
                    if state
                        .db
                        .is_agent_owner(tenant.community(), &author, &actor_bytes)
                        .await?
                    {
                        return Ok(());
                    }
                    // Logged only once the agent-owner fallback has also failed,
                    // so a legitimate owner delete does not log a denial. `warn`
                    // rather than `debug`: this is the only trace of a denial,
                    // including a DB-fault-induced one, and debug is off in
                    // production — a database outage would otherwise reach the
                    // user as a bare permissions message with nothing in the log.
                    warn!(
                        error = %e,
                        actor = %hex::encode(&actor_bytes),
                        channel_id = %channel_id,
                        target_event = %hex::encode(&target_id),
                        "kind:9005 delete denied"
                    );
                    Err(anyhow::anyhow!(
                        "must be the event author, the owner of the authoring agent, \
                         or a moderator with authority over this channel"
                    ))
                }
            }
        }
        9008 => {
            // DELETE_GROUP: owner only, or the owning human of the channel's agent-owner.
            let members = state.db.get_members(tenant.community(), channel_id).await?;
            let actor_member = members.iter().find(|m| m.pubkey == actor_bytes);
            match actor_member {
                Some(m) if m.role == "owner" => Ok(()),
                _ => {
                    // Allow the owning human of any active owner-role agent in the
                    // channel, even when the human is not a channel member —
                    // diverges from kind:9001 intentionally.
                    if actor_owns_any_owner_agent(state, tenant.community(), &members, &actor_bytes)
                        .await?
                    {
                        return Ok(());
                    }
                    Err(anyhow::anyhow!("only owner can delete group"))
                }
            }
        }
        9022 => {
            // LEAVE_REQUEST: must be an active member, and cannot be the last owner.
            let members = state.db.get_members(tenant.community(), channel_id).await?;
            let actor_member = members.iter().find(|m| m.pubkey == actor_bytes);
            match actor_member {
                None => {
                    return Err(anyhow::anyhow!("actor is not an active member"));
                }
                Some(m) if m.role == "owner" => {
                    let owner_count = members.iter().filter(|m| m.role == "owner").count();
                    if owner_count <= 1 {
                        return Err(anyhow::anyhow!("cannot remove the last owner"));
                    }
                }
                _ => {}
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Emit a system message (kind 40099) signed by the relay keypair.
pub async fn emit_system_message(
    tenant: &TenantContext,
    state: &Arc<AppState>,
    channel_id: Uuid,
    content: serde_json::Value,
) -> anyhow::Result<()> {
    let channel_tag = Tag::parse(["h", &channel_id.to_string()])?;

    let event = EventBuilder::new(Kind::Custom(40099), content.to_string())
        .tags([channel_tag])
        .sign_with_keys(&state.relay_keypair)
        .map_err(|e| anyhow::anyhow!("failed to sign system message: {e}"))?;

    if let Err(e) = state
        .db
        .insert_event(tenant.community(), &event, Some(channel_id))
        .await
    {
        warn!(channel = %channel_id, error = %e, "system message insert failed");
    }

    // Fan out to subscribers
    if let Err(e) = state
        .pubsub
        .publish_event(tenant, EventTopic::Channel(channel_id), &event)
        .await
    {
        warn!("System message fan-out failed: {e}");
    }

    Ok(())
}

/// Publish the relay-signed acceptance receipt for a just-accepted
/// coding-session authority transition (kind 44228).
///
/// Runs only when [`super::event::dispatch_persistent_event`]'s caller has
/// already confirmed the transition was newly inserted — `ingest_event_inner`
/// short-circuits a replayed event with `duplicate:` before side effects ever
/// fire, so a resubmission of an already-accepted transition does not mint a
/// second receipt. The receipt is a `kind:40099` system message (the
/// existing relay-signed-emission pattern) naming exactly the facts a
/// consumer needs to establish the new canonical head: the genesis, the
/// accepted transition, its sequence number, its type, and the grantee. Seat
/// transition receipts additionally echo their required role, and a
/// project-action delegation echoes the project it is scoped to.
async fn handle_coding_session_authority_transition_accepted(
    tenant: &TenantContext,
    event: &Event,
    state: &Arc<AppState>,
) -> anyhow::Result<()> {
    let channel_id = extract_h_tag_channel(event)
        .ok_or_else(|| anyhow::anyhow!("coding-session authority transition missing h tag"))?;
    let payload = decode_coding_session_authority_transition(&event.content)
        .map_err(|e| anyhow::anyhow!("undecodable accepted authority transition: {e}"))?;

    // A grant/revoke changes who may steer and who may read the transport
    // channel: flush the authority caches and every reader's
    // accessible-channel set so the change lands now, not after the TTL.
    state.invalidate_session_authority_caches();
    state.invalidate_all_accessible_channels(tenant);

    let mut receipt = serde_json::json!({
        "type": "coding_session_authority_transition_accepted",
        "genesisRef": payload.genesis_ref,
        "acceptedEventId": event.id.to_hex(),
        "seq": payload.seq,
        "transitionType": payload.transition_type,
        "granteePubkey": payload.grantee_pubkey,
    });
    if let (Some(object), Some(role)) = (receipt.as_object_mut(), payload.role) {
        object.insert("role".into(), serde_json::Value::String(role));
    }
    // The body a `takeover`/`transfer` named, carried through to every
    // consumer that folds receipts rather than re-reading the chain. Without
    // it a provider learns *that* the session was claimed and not *which
    // machine* now carries it, which is the one fact the fence compares
    // against its own key (`docs/HANDOVER_IMPL.md` §1, §3). Present exactly
    // when the transition carried one, so every receipt signed before claims
    // existed keeps its exact shape.
    if let (Some(object), Some(body_pubkey)) = (receipt.as_object_mut(), payload.body_pubkey) {
        object.insert("bodyPubkey".into(), serde_json::Value::String(body_pubkey));
    }
    // Same rule for the project a `grant-project-actions` delegates
    // (ledger 186): a receipt that said only "a delegation was accepted for
    // this key" would leave a reader to guess its scope, and the scope is the
    // entire point of the link. Present exactly when the transition carried
    // one, so every earlier receipt keeps its exact shape.
    if let (Some(object), Some(project_ref)) = (receipt.as_object_mut(), payload.project_ref) {
        object.insert("projectRef".into(), serde_json::Value::String(project_ref));
    }
    emit_system_message(tenant, state, channel_id, receipt).await
}

/// Sign and fan out a fresh relay-signed `kind:39005` thread-summary overlay
/// for `root_id` after a thread mutation (reply insert or threaded delete).
///
/// Fan-out only — never stored. Channel-window pages recompute summaries from
/// `thread_metadata` on every fetch (`api/bridge.rs`), so a persisted copy
/// would only add staleness; this live emit exists purely so subscribed
/// clients can update badge counts without refetching the head window. The
/// counts are re-read from `thread_metadata` post-commit rather than
/// incremented, so the emitted summary is exact even under concurrent
/// replies/deletes (newest `created_at` wins client-side).
///
/// Spawned: runs after the triggering write committed and must not add
/// latency to the ingest acknowledgement, mirroring
/// `dispatch_persistent_event`.
pub fn emit_live_thread_summary(
    tenant: &TenantContext,
    state: &Arc<AppState>,
    channel_id: Uuid,
    root_id: Vec<u8>,
) {
    let tenant = tenant.clone();
    let state = Arc::clone(state);
    tokio::spawn(async move {
        let summary = match state
            .db
            .get_thread_summary(tenant.community(), &root_id)
            .await
        {
            Ok(Some(summary)) => summary,
            // Root has no thread row — nothing to summarize (e.g., the root
            // itself was just deleted).
            Ok(None) => return,
            Err(e) => {
                warn!(
                    root = %hex::encode(&root_id),
                    "live thread summary lookup failed: {e}"
                );
                return;
            }
        };

        let root_hex = hex::encode(&root_id);
        // Same tags/content shape as the channel-window page overlay in
        // `api/bridge.rs` — one contract, two delivery doors.
        let content = serde_json::json!({
            "reply_count": summary.reply_count,
            "descendant_count": summary.descendant_count,
            "last_reply_at": summary.last_reply_at.map(|t| t.timestamp()),
            "participants": summary.participants.iter().map(hex::encode).collect::<Vec<_>>(),
        });
        let tags = [
            Tag::parse(["e", &root_hex]),
            Tag::parse(["d", &root_hex]),
            Tag::parse(["h", &channel_id.to_string()]),
        ];
        let mut parsed = Vec::with_capacity(tags.len());
        for tag in tags {
            match tag {
                Ok(tag) => parsed.push(tag),
                Err(e) => {
                    warn!(root = %root_hex, "live thread summary tag failed: {e}");
                    return;
                }
            }
        }
        let event = match EventBuilder::new(
            Kind::Custom(KIND_THREAD_SUMMARY as u16),
            content.to_string(),
        )
        .tags(parsed)
        .sign_with_keys(&state.relay_keypair)
        {
            Ok(event) => event,
            Err(e) => {
                warn!(root = %root_hex, "live thread summary sign failed: {e}");
                return;
            }
        };

        // Redis before local fan-out so subscribers on other relay pods
        // receive it too, matching `dispatch_persistent_event`.
        state.mark_local_event(tenant.community(), &event.id);
        if let Err(e) = state
            .pubsub
            .publish_event(&tenant, EventTopic::Channel(channel_id), &event)
            .await
        {
            state
                .local_event_ids
                .invalidate(&(tenant.community(), event.id.to_bytes()));
            warn!(root = %root_hex, "live thread summary Redis publish failed: {e}");
        }
        let stored = StoredEvent::new(event, Some(channel_id));
        crate::handlers::event::fan_out_event_to_local_subscribers(
            &state,
            tenant.community(),
            &stored,
        )
        .await;
    });
}

/// Emit a relay-signed membership notification event stored globally (channel_id = None).
///
/// kind:44100 = member added, kind:44101 = member removed.
/// The p tag addresses the target pubkey; the h tag carries the channel UUID as metadata.
/// Stored with channel_id = None so global subscribers receive it via slow-path fan-out.
pub async fn emit_membership_notification(
    tenant: &TenantContext,
    state: &Arc<AppState>,
    channel_id: Uuid,
    target_pubkey: &[u8],
    actor_pubkey: &[u8],
    notification_kind: u32,
) -> anyhow::Result<()> {
    let target_hex = hex::encode(target_pubkey);
    let actor_hex = hex::encode(actor_pubkey);
    let channel_id_str = channel_id.to_string();

    let p_tag = Tag::parse(["p", &target_hex])
        .map_err(|e| anyhow::anyhow!("failed to build p tag: {e}"))?;
    let h_tag = Tag::parse(["h", &channel_id_str])
        .map_err(|e| anyhow::anyhow!("failed to build h tag: {e}"))?;

    let event_type = match notification_kind {
        KIND_MEMBER_ADDED_NOTIFICATION => "member_added",
        KIND_MEMBER_REMOVED_NOTIFICATION => "member_removed",
        _ => {
            return Err(anyhow::anyhow!(
                "invalid notification kind: {notification_kind}"
            ))
        }
    };

    let content = serde_json::json!({
        "type": event_type,
        "channel_id": channel_id_str,
        "actor": actor_hex,
    })
    .to_string();

    let event = EventBuilder::new(Kind::Custom(notification_kind as u16), content)
        .tags([p_tag, h_tag])
        .sign_with_keys(&state.relay_keypair)
        .map_err(|e| anyhow::anyhow!("failed to sign membership notification: {e}"))?;

    // Store with channel_id = None → globally scoped, reachable by global subscribers.
    let (stored, was_inserted) = state
        .db
        .insert_event(tenant.community(), &event, None)
        .await?;
    if !was_inserted {
        return Ok(());
    }

    // Fan-out only — skip search indexing and workflow evaluation. Publish through
    // Redis before local fan-out so agents connected to other relay pods receive
    // the global membership notification and can subscribe to the new channel.
    // Use the nil UUID sentinel for globally-scoped events, matching
    // `dispatch_persistent_event` and `fan_out_pubsub_event`.
    state.mark_local_event(tenant.community(), &stored.event.id);
    if let Err(e) = state
        .pubsub
        .publish_event(tenant, EventTopic::Global, &stored.event)
        .await
    {
        state
            .local_event_ids
            .invalidate(&(tenant.community(), stored.event.id.to_bytes()));
        warn!(
            channel = %channel_id,
            target = %target_hex,
            kind = notification_kind,
            "membership notification Redis publish failed: {e}"
        );
    }

    // Routed through the guarded send path for uniformity; the access gate no-ops
    // for these globally-scoped (channel_id = None) events.
    crate::handlers::event::fan_out_event_to_local_subscribers(state, tenant.community(), &stored)
        .await;

    info!(
        channel = %channel_id,
        target = %target_hex,
        kind = notification_kind,
        "membership notification emitted"
    );
    Ok(())
}

/// Sign, store (replacing previous), and fan-out a single addressable discovery event.
async fn emit_addressable_discovery_event(
    tenant: &TenantContext,
    state: &Arc<AppState>,
    channel_id: Uuid,
    kind: u32,
    tags: Vec<Tag>,
    relay_pubkey_hex: &str,
) -> anyhow::Result<()> {
    // Ensure the new event's created_at is strictly greater than any existing event
    // of the same (kind, pubkey, channel_id). Without this, rapid successive updates
    // (e.g. set topic then set purpose in the same second) can produce events with
    // identical created_at, causing the second to be rejected by stale-write protection
    // (NIP-16 tiebreaker: lower event ID wins, which is random).
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let min_ts = {
        let existing = state
            .db
            .query_events(&beekeeper_db::event::EventQuery {
                kinds: Some(vec![kind as i32]),
                channel_id: Some(channel_id),
                limit: Some(1),
                ..beekeeper_db::event::EventQuery::for_community(tenant.community())
            })
            .await
            .unwrap_or_default();
        existing
            .first()
            .map(|e| e.event.created_at.as_secs() + 1)
            .unwrap_or(now)
    };
    let ts = now.max(min_ts);

    let event = EventBuilder::new(Kind::Custom(kind as u16), "")
        .tags(tags)
        .custom_created_at(nostr::Timestamp::from(ts))
        .sign_with_keys(&state.relay_keypair)
        .map_err(|e| anyhow::anyhow!("failed to sign kind:{kind}: {e}"))?;

    let (stored, was_inserted) = state
        .db
        .replace_addressable_event(tenant.community(), &event, Some(channel_id))
        .await?;
    if was_inserted {
        let kind_u32 = event_kind_u32(&stored.event);
        dispatch_persistent_event(tenant, state, &stored, kind_u32, relay_pubkey_hex, None).await;
    }
    Ok(())
}

fn group_members_tags(group_id: &str, members: &[MemberRecord]) -> anyhow::Result<Vec<Tag>> {
    let mut tags: Vec<Tag> = Vec::with_capacity(members.len() + 1);
    tags.push(Tag::parse(["d", group_id])?);
    for member in members {
        let pubkey_hex = hex::encode(&member.pubkey);
        // NIP-29 convention: ["p", pubkey, relay_url, role]. Empty relay_url
        // because the canonical relay is implicit (this event is signed by it).
        tags.push(Tag::parse(["p", &pubkey_hex, "", &member.role])?);
    }
    Ok(tags)
}

/// Emit NIP-29 group discovery events (39000, 39001, 39002) signed by the relay keypair.
/// Called after group creation, metadata changes, or membership changes.
/// Events are stored channel-scoped (`channel_id = Some(...)`) so that existing
/// access control applies — private channel member lists are only visible to members.
///
/// NOTE: Channel-scoped storage means live global subscriptions (e.g. `{kinds:[39000]}`)
/// won't receive these events via fan-out. Clients discover groups via historical REQ
/// queries. Live push for open-channel discovery is a future enhancement.
pub async fn emit_group_discovery_events(
    tenant: &TenantContext,
    state: &Arc<AppState>,
    channel_id: Uuid,
) -> anyhow::Result<()> {
    let channel = state.db.get_channel(tenant.community(), channel_id).await?;
    let members = state.db.get_members(tenant.community(), channel_id).await?;

    let relay_pubkey_hex = hex::encode(state.relay_keypair.public_key().to_bytes());
    let group_id = channel_id.to_string();

    {
        let mut tags: Vec<Tag> = vec![Tag::parse(["d", &group_id])?];
        tags.push(Tag::parse(["name", &channel.name])?);
        if let Some(ref desc) = channel.description {
            if !desc.is_empty() {
                tags.push(Tag::parse(["about", desc])?);
            }
        }
        if channel.visibility == "private" {
            tags.push(Tag::parse(["private"])?);
        } else {
            // Explicit "public" tag complements NIP-29's absence-of-"private" convention,
            // making channel visibility self-describing for clients.
            tags.push(Tag::parse(["public"])?);
        }
        // NIP-29 hidden tag: hint to clients not to show session-transport
        // channels in group lists. Not a security boundary — access control
        // is handled by channel-scoped storage and the transport gate.
        if channel.channel_type == "transport" {
            tags.push(Tag::parse(["hidden"])?);
        }
        // NIP-29 hidden tag: hint to clients not to show DMs in public group lists.
        // Not a security boundary — access control is handled by channel-scoped storage.
        if channel.channel_type == "dm" {
            tags.push(Tag::parse(["hidden"])?);
            // Include participant pubkeys in kind:39000 for DMs so clients can
            // resolve display names without a separate kind:39002 fetch.
            for m in &members {
                let pubkey_hex = hex::encode(&m.pubkey);
                tags.push(Tag::parse(["p", &pubkey_hex])?);
            }
        }
        // Buzz channels always require explicit membership
        tags.push(Tag::parse(["closed"])?);
        // Channel type tag so clients can distinguish stream/forum/dm without inference
        tags.push(Tag::parse(["t", &channel.channel_type])?);
        // Optional topic / purpose for richer client UX
        if let Some(ref topic) = channel.topic {
            if !topic.is_empty() {
                tags.push(Tag::parse(["topic", topic])?);
            }
        }
        if let Some(ref purpose) = channel.purpose {
            if !purpose.is_empty() {
                tags.push(Tag::parse(["purpose", purpose])?);
            }
        }
        // Optional project-container coordinate — lets clients group channels by
        // project without a separate per-channel query.
        if let Some(ref project_ref) = channel.project_ref {
            if !project_ref.is_empty() {
                tags.push(Tag::parse(["project", project_ref])?);
            }
        }
        // Archived state — clients use this to hide channels from the sidebar.
        if channel.archived_at.is_some() {
            tags.push(Tag::parse(["archived", "true"])?);
        }
        // Ephemeral channel TTL — clients use this to show countdown timers.
        if let Some(ttl) = channel.ttl_seconds {
            tags.push(Tag::parse(["ttl", &ttl.to_string()])?);
        }
        if let Some(ref deadline) = channel.ttl_deadline {
            tags.push(Tag::parse(["ttl_deadline", &deadline.to_rfc3339()])?);
        }
        emit_addressable_discovery_event(
            tenant,
            state,
            channel_id,
            KIND_NIP29_GROUP_METADATA,
            tags,
            &relay_pubkey_hex,
        )
        .await?;
    }

    {
        let mut tags: Vec<Tag> = vec![Tag::parse(["d", &group_id])?];
        for m in members
            .iter()
            .filter(|m| m.role == "owner" || m.role == "admin")
        {
            let pubkey_hex = hex::encode(&m.pubkey);
            tags.push(Tag::parse(["p", &pubkey_hex, &m.role])?);
        }
        emit_addressable_discovery_event(
            tenant,
            state,
            channel_id,
            KIND_NIP29_GROUP_ADMINS,
            tags,
            &relay_pubkey_hex,
        )
        .await?;
    }

    {
        let tags = group_members_tags(&group_id, &members)?;
        emit_addressable_discovery_event(
            tenant,
            state,
            channel_id,
            KIND_NIP29_GROUP_MEMBERS,
            tags,
            &relay_pubkey_hex,
        )
        .await?;
    }

    Ok(())
}

async fn handle_agent_profile(
    tenant: &TenantContext,
    event: &Event,
    state: &Arc<AppState>,
) -> anyhow::Result<()> {
    let content: serde_json::Value = serde_json::from_str(&event.content)
        .map_err(|e| anyhow::anyhow!("kind:10100 content parse error: {e}"))?;

    let policy = content
        .get("channel_add_policy")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("kind:10100 missing channel_add_policy field"))?;

    let pubkey_bytes = event.pubkey.to_bytes().to_vec();
    if state
        .db
        .ensure_user(tenant.community(), &pubkey_bytes)
        .await?
    {
        metrics::counter!(
            "buzz_users_created_total",
            "community" => tenant.host().to_owned()
        )
        .increment(1);
    }
    state
        .db
        .set_channel_add_policy(tenant.community(), &pubkey_bytes, policy)
        .await?;

    info!(pubkey = %hex::encode(&pubkey_bytes), policy, "kind:10100 channel_add_policy updated");
    Ok(())
}

/// Kind:0 (NIP-01 profile metadata) side effect — sync profile fields to users table.
async fn handle_kind0_profile(
    tenant: &TenantContext,
    event: &Event,
    state: &Arc<AppState>,
) -> anyhow::Result<()> {
    let content: serde_json::Value = serde_json::from_str(&event.content)
        .map_err(|e| anyhow::anyhow!("kind:0 content parse error: {e}"))?;

    // Kind:0 is absolute state (NIP-01 replaceable event). Fields present in the
    // event are set; fields absent are cleared. We use Some("") to clear absent
    // fields, since update_user_profile only writes Some values.
    let display_name = content
        .get("display_name")
        .or_else(|| content.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let avatar_url = content
        .get("picture")
        .or_else(|| content.get("image"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let about = content.get("about").and_then(|v| v.as_str()).unwrap_or("");

    // Validate NIP-05 handle: must be user@domain where domain matches the
    // bound tenant host. Invalid or off-domain handles are silently cleared
    // (treated as absent) rather than stored, since the event is already
    // persisted and can't be rejected.
    let nip05_owned = content
        .get("nip05")
        .and_then(|v| v.as_str())
        .and_then(|raw| crate::api::nip05::canonicalize_nip05(raw, tenant.host()).ok());
    let nip05_handle = nip05_owned.as_deref().unwrap_or("");

    let pubkey_bytes = event.pubkey.to_bytes().to_vec();

    if state
        .db
        .ensure_user(tenant.community(), &pubkey_bytes)
        .await?
    {
        metrics::counter!(
            "buzz_users_created_total",
            "community" => tenant.host().to_owned()
        )
        .increment(1);
    }

    // Pass all fields as Some — empty string clears the field in the DB.
    // This ensures kind:0 is treated as absolute state, not a partial update.
    // If the NIP-05 handle collides with another user's UNIQUE constraint, retry
    // without it so display_name/about/avatar_url are still written.
    let result = state
        .db
        .update_user_profile(
            tenant.community(),
            &pubkey_bytes,
            Some(display_name),
            Some(avatar_url),
            Some(about),
            Some(nip05_handle),
        )
        .await;

    if let Err(ref e) = result {
        let msg = format!("{e}");
        if msg.contains("duplicate key value") || msg.contains("23505") {
            warn!(pubkey = %hex::encode(&pubkey_bytes),
                "kind:0 NIP-05 handle contested, syncing profile without it");
            state
                .db
                .update_user_profile(
                    tenant.community(),
                    &pubkey_bytes,
                    Some(display_name),
                    Some(avatar_url),
                    Some(about),
                    None, // skip contested NIP-05
                )
                .await?;
        } else {
            result?;
        }
    }

    info!(pubkey = %hex::encode(&pubkey_bytes), "kind:0 profile synced to users table");
    Ok(())
}

async fn handle_put_user(
    tenant: &TenantContext,
    event: &Event,
    state: &Arc<AppState>,
) -> anyhow::Result<()> {
    let channel_id =
        extract_h_tag_channel(event).ok_or_else(|| anyhow::anyhow!("missing h tag"))?;
    let target_pubkey = extract_p_tag(event).ok_or_else(|| anyhow::anyhow!("missing p tag"))?;
    // No role tag = no role change: preserve an existing member's current role and
    // fall back to Member only for a new member. Unconditionally defaulting to
    // Member let a bare PUT_USER silently demote an existing owner/admin.
    let role: MemberRole = match extract_tag_value(event, "role") {
        Some(role_str) => role_str
            .parse()
            .map_err(|_| anyhow::anyhow!("invalid role: {role_str}"))?,
        None => state
            .db
            .get_members(tenant.community(), channel_id)
            .await?
            .iter()
            .find(|m| m.pubkey == target_pubkey)
            .and_then(|m| m.role.parse().ok())
            .unwrap_or(MemberRole::Member),
    };

    let actor_bytes = event.pubkey.to_bytes().to_vec();

    state
        .db
        .add_member(
            tenant.community(),
            channel_id,
            &target_pubkey,
            role,
            Some(&actor_bytes),
        )
        .await?;
    state.invalidate_membership(tenant, channel_id, &target_pubkey);

    let actor_hex = hex::encode(&actor_bytes);
    let target_hex = hex::encode(&target_pubkey);
    emit_system_message(
        tenant,
        state,
        channel_id,
        serde_json::json!({
            "type": "member_joined",
            "actor": actor_hex,
            "target": target_hex,
        }),
    )
    .await?;

    if let Err(e) = emit_group_discovery_events(tenant, state, channel_id).await {
        warn!(channel = %channel_id, error = %e, "NIP-29 group discovery emission failed");
    }

    if let Err(e) = emit_membership_notification(
        tenant,
        state,
        channel_id,
        &target_pubkey,
        &actor_bytes,
        KIND_MEMBER_ADDED_NOTIFICATION,
    )
    .await
    {
        warn!(channel = %channel_id, error = %e, "membership notification emission failed");
    }

    info!(channel = %channel_id, target = %target_hex, "NIP-29 PUT_USER processed");
    Ok(())
}

async fn handle_remove_user(
    tenant: &TenantContext,
    event: &Event,
    state: &Arc<AppState>,
) -> anyhow::Result<()> {
    let channel_id =
        extract_h_tag_channel(event).ok_or_else(|| anyhow::anyhow!("missing h tag"))?;
    let target_pubkey = extract_p_tag(event).ok_or_else(|| anyhow::anyhow!("missing p tag"))?;
    let actor_bytes = event.pubkey.to_bytes().to_vec();

    // Guard: prevent last-owner orphaning on self-removal (kind 9001).
    if target_pubkey == actor_bytes {
        let members = state.db.get_members(tenant.community(), channel_id).await?;
        let owner_count = members.iter().filter(|m| m.role == "owner").count();
        let actor_is_owner = members
            .iter()
            .any(|m| m.pubkey == actor_bytes && m.role == "owner");
        if actor_is_owner && owner_count <= 1 {
            return Err(anyhow::anyhow!(
                "cannot remove the last owner — transfer ownership first"
            ));
        }
    }

    state
        .db
        .remove_member(tenant.community(), channel_id, &target_pubkey, &actor_bytes)
        .await?;
    state.invalidate_membership(tenant, channel_id, &target_pubkey);
    evict_live_channel_subscriptions(tenant, state, channel_id, &target_pubkey).await;
    disable_departed_member_workflows(tenant, state, channel_id, &target_pubkey).await;

    let actor_hex = hex::encode(&actor_bytes);
    let target_hex = hex::encode(&target_pubkey);
    let msg_type = if target_pubkey == actor_bytes {
        "member_left"
    } else {
        "member_removed"
    };
    emit_system_message(
        tenant,
        state,
        channel_id,
        serde_json::json!({
            "type": msg_type,
            "actor": actor_hex,
            "target": target_hex,
        }),
    )
    .await?;

    if let Err(e) = emit_group_discovery_events(tenant, state, channel_id).await {
        warn!(channel = %channel_id, error = %e, "NIP-29 group discovery emission failed");
    }

    if let Err(e) = emit_membership_notification(
        tenant,
        state,
        channel_id,
        &target_pubkey,
        &actor_bytes,
        KIND_MEMBER_REMOVED_NOTIFICATION,
    )
    .await
    {
        warn!(channel = %channel_id, error = %e, "membership notification emission failed");
    }

    Ok(())
}

async fn handle_edit_metadata(
    tenant: &TenantContext,
    event: &Event,
    state: &Arc<AppState>,
) -> anyhow::Result<()> {
    let channel_id =
        extract_h_tag_channel(event).ok_or_else(|| anyhow::anyhow!("missing h tag"))?;
    let actor_bytes = event.pubkey.to_bytes().to_vec();
    let actor_hex = hex::encode(&actor_bytes);

    for tag in event.tags.iter() {
        let key = tag.kind().to_string();
        if let Some(val) = tag.content() {
            match key.as_str() {
                "name" => {
                    state
                        .db
                        .update_channel(
                            tenant.community(),
                            channel_id,
                            beekeeper_db::channel::ChannelUpdate {
                                name: Some(val.to_string()),
                                ..Default::default()
                            },
                        )
                        .await?;
                }
                "about" => {
                    state
                        .db
                        .update_channel(
                            tenant.community(),
                            channel_id,
                            beekeeper_db::channel::ChannelUpdate {
                                description: Some(val.to_string()),
                                ..Default::default()
                            },
                        )
                        .await?;
                }
                "topic" => {
                    state
                        .db
                        .set_topic(tenant.community(), channel_id, val, &actor_bytes)
                        .await?;
                    emit_system_message(
                        tenant,
                        state,
                        channel_id,
                        serde_json::json!({
                            "type": "topic_changed", "actor": actor_hex, "topic": val
                        }),
                    )
                    .await?;
                }
                "purpose" => {
                    state
                        .db
                        .set_purpose(tenant.community(), channel_id, val, &actor_bytes)
                        .await?;
                    emit_system_message(
                        tenant,
                        state,
                        channel_id,
                        serde_json::json!({
                            "type": "purpose_changed", "actor": actor_hex, "purpose": val
                        }),
                    )
                    .await?;
                }
                "visibility" => {
                    let was_open = state
                        .db
                        .get_channel(tenant.community(), channel_id)
                        .await
                        .map(|c| c.visibility == "open")
                        .unwrap_or(false);
                    state
                        .db
                        .update_channel(
                            tenant.community(),
                            channel_id,
                            beekeeper_db::channel::ChannelUpdate {
                                visibility: Some(val.to_string()),
                                ..Default::default()
                            },
                        )
                        .await?;
                    // A visibility flip changes who can see the channel, so the
                    // accessible-channels and visibility caches must be cleared
                    // before any later event for this channel fans out.
                    state.invalidate_all_accessible_channels(tenant);
                    state.invalidate_channel_visibility(tenant, channel_id);
                    // On open -> private, eagerly close non-members' live subs
                    // for an immediate CLOSED on this node. The fan-out access
                    // filter is the cluster-wide correctness backstop.
                    if was_open && val == "private" {
                        evict_non_member_channel_subscriptions(tenant, state, channel_id).await?;
                    }
                    emit_system_message(
                        tenant,
                        state,
                        channel_id,
                        serde_json::json!({
                            "type": "visibility_changed", "actor": actor_hex, "visibility": val
                        }),
                    )
                    .await?;
                }
                "project" => {
                    // Empty string clears the project association; otherwise
                    // the value is a `30621:pubkey:slug` coordinate, already
                    // shape-validated pre-storage in validate_admin_event.
                    let project_change: Option<String> = if val.is_empty() {
                        None
                    } else {
                        Some(val.to_string())
                    };
                    state
                        .db
                        .update_channel(
                            tenant.community(),
                            channel_id,
                            beekeeper_db::channel::ChannelUpdate {
                                project_ref: Some(project_change.clone()),
                                ..Default::default()
                            },
                        )
                        .await?;
                    emit_system_message(
                        tenant,
                        state,
                        channel_id,
                        serde_json::json!({
                            "type": "project_changed", "actor": actor_hex, "project_ref": project_change
                        }),
                    )
                    .await?;
                }
                "ttl" => {
                    // Empty string clears the TTL (permanent); otherwise it is a
                    // positive integer of seconds, validated during authorization.
                    // Fail closed: a parse failure must reject, never silently
                    // clear the TTL to permanent.
                    let ttl_change: Option<i32> = if val.is_empty() {
                        None
                    } else {
                        Some(val.parse::<i32>().map_err(|_| {
                            anyhow::anyhow!("invalid ttl value: {val} (must be a positive integer)")
                        })?)
                    };
                    state
                        .db
                        .update_channel(
                            tenant.community(),
                            channel_id,
                            beekeeper_db::channel::ChannelUpdate {
                                ttl_seconds: Some(ttl_change),
                                ..Default::default()
                            },
                        )
                        .await?;
                    emit_system_message(
                        tenant,
                        state,
                        channel_id,
                        serde_json::json!({
                            "type": "ttl_changed", "actor": actor_hex, "ttl_seconds": ttl_change
                        }),
                    )
                    .await?;
                }
                "archived" => {
                    match val {
                        "true" => {
                            state
                                .db
                                .archive_channel(tenant.community(), channel_id)
                                .await?;
                            emit_system_message(
                                tenant,
                                state,
                                channel_id,
                                serde_json::json!({
                                    "type": "channel_archived", "actor": actor_hex
                                }),
                            )
                            .await?;
                        }
                        "false" => {
                            state
                                .db
                                .unarchive_channel(tenant.community(), channel_id)
                                .await?;
                            emit_system_message(
                                tenant,
                                state,
                                channel_id,
                                serde_json::json!({
                                    "type": "channel_unarchived", "actor": actor_hex
                                }),
                            )
                            .await?;

                            // Resubscribe connected agents after restore: archiving evicts their
                            // live subscriptions (CLOSED "channel access revoked") and unarchive
                            // otherwise emits no signal that makes a connected agent resubscribe.
                            // We reuse the member_added notification (44100) purely as a resubscribe
                            // trigger — no membership actually changed here — because it flows on the
                            // agent's always-live global membership subscription, the same path
                            // remove/re-add uses to recover. Humans self-heal via the re-emitted
                            // kind:39000 discovery, so this is intentionally agent-scoped.
                            //
                            // Known limitation: emit_membership_notification builds a created_at=now
                            // event with no nonce, and insert_event skips fan-out on a duplicate id.
                            // Four sub-second toggles (archive->unarchive->archive->unarchive) on the
                            // same channel by the same actor could collide ids and skip a fan-out.
                            // Not reachable in practice — unarchive has a single human-driven caller;
                            // the reaper only auto-archives — so we don't engineer around it.
                            for member in
                                state.db.get_members(tenant.community(), channel_id).await?
                            {
                                if let Err(e) = emit_membership_notification(
                                    tenant,
                                    state,
                                    channel_id,
                                    &member.pubkey,
                                    &actor_bytes,
                                    KIND_MEMBER_ADDED_NOTIFICATION,
                                )
                                .await
                                {
                                    warn!(
                                        channel = %channel_id,
                                        error = %e,
                                        "post-unarchive resubscribe notification failed"
                                    );
                                }
                            }
                        }
                        _ => {} // ignore invalid values
                    }
                }
                _ => {}
            }
        }
    }

    if let Err(e) = emit_group_discovery_events(tenant, state, channel_id).await {
        warn!(channel = %channel_id, error = %e, "NIP-29 group discovery emission failed");
    }

    Ok(())
}

async fn handle_delete_event_side_effect(
    tenant: &TenantContext,
    event: &Event,
    state: &Arc<AppState>,
) -> anyhow::Result<()> {
    let channel_id =
        extract_h_tag_channel(event).ok_or_else(|| anyhow::anyhow!("missing h tag"))?;

    // Extract target event ID from e tag
    let target_id = event
        .tags
        .iter()
        .find_map(|tag| {
            if tag.kind().to_string() == "e" {
                tag.content().and_then(|v| {
                    let bytes = hex::decode(v).ok()?;
                    if bytes.len() == 32 {
                        Some(bytes)
                    } else {
                        None
                    }
                })
            } else {
                None
            }
        })
        .ok_or_else(|| anyhow::anyhow!("missing e tag for target event"))?;

    // Verify the target event belongs to the same channel as the h-tag.
    // Without this check, an admin of channel A could delete events in channel B
    // by sending h=A, e=<event-in-B>.
    if let Some(target_event) = state
        .db
        .get_event_by_id_including_deleted(tenant.community(), &target_id)
        .await
        .map_err(|e| anyhow::anyhow!("get_event_by_id failed: {e}"))?
    {
        match target_event.channel_id {
            Some(target_ch) if target_ch != channel_id => {
                return Err(anyhow::anyhow!(
                    "target event belongs to a different channel"
                ));
            }
            None => {
                return Err(anyhow::anyhow!("target event has no channel"));
            }
            _ => {} // Same channel — OK
        }
    }

    // Look up thread metadata so we can pass parent/root IDs to the
    // transactional delete function.
    let meta = state
        .db
        .get_thread_metadata_by_event(tenant.community(), &target_id)
        .await
        .map_err(|e| anyhow::anyhow!("get_thread_metadata failed: {e}"))?;

    let parent_id = meta.as_ref().and_then(|m| m.parent_event_id.clone());
    let root_id = meta.as_ref().and_then(|m| m.root_event_id.clone());

    // Atomically soft-delete the event and decrement thread counters in one transaction.
    let deleted = state
        .db
        .soft_delete_event_and_update_thread(
            tenant.community(),
            &target_id,
            parent_id.as_deref(),
            root_id.as_deref(),
        )
        .await
        .map_err(|e| anyhow::anyhow!("soft_delete_event failed: {e}"))?;

    if !deleted {
        warn!(target_event = %hex::encode(&target_id), "event already deleted or not found");
        return Ok(()); // No-op: skip system message to avoid false audit records.
    }

    // Thread counters were decremented in the same transaction — push a fresh
    // relay-signed 39005 so live badge counts also count *down*.
    if let Some(root_id) = root_id {
        emit_live_thread_summary(tenant, state, channel_id, root_id);
    }

    let actor_hex = hex::encode(event.pubkey.to_bytes());
    let mut tombstone = serde_json::json!({
        "type": "message_deleted",
        "actor": actor_hex,
        "target_event_id": hex::encode(&target_id),
    });
    copy_optional_string_field(event, &mut tombstone, "action_id");
    copy_optional_string_field(event, &mut tombstone, "reason_code");
    copy_optional_string_field(event, &mut tombstone, "public_reason");

    emit_system_message(tenant, state, channel_id, tombstone).await?;

    info!(target_event = %hex::encode(&target_id), "NIP-29 DELETE_EVENT processed");
    Ok(())
}

async fn handle_create_group(
    tenant: &TenantContext,
    event: &Event,
    state: &Arc<AppState>,
) -> anyhow::Result<()> {
    let name =
        extract_tag_value(event, "name").ok_or_else(|| anyhow::anyhow!("missing name tag"))?;
    let visibility_str =
        extract_tag_value(event, "visibility").unwrap_or_else(|| "open".to_string());
    let channel_type_str =
        extract_tag_value(event, "channel_type").unwrap_or_else(|| "stream".to_string());

    let visibility: beekeeper_db::channel::ChannelVisibility = visibility_str
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid visibility: {visibility_str}"))?;
    let channel_type: beekeeper_db::channel::ChannelType = channel_type_str
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid channel_type: {channel_type_str}"))?;

    let actor_bytes = event.pubkey.to_bytes().to_vec();
    let description = extract_tag_value(event, "about");
    let ttl_seconds = super::resolve_ttl(event, state.config.ephemeral_ttl_override);
    // Already shape-validated in ingest.rs (validate_project_ref_tag) before this
    // event was persisted; just carry the raw coordinate through here.
    let project_ref = extract_tag_value(event, "project");

    // If the event has an h-tag UUID, ingest_event() already created the channel
    // via create_channel_with_id(). Fetch it rather than creating a duplicate.
    // If no h-tag, fall back to the original auto-UUID creation path.
    //
    // Double-count analysis (C5): the counter increments below do NOT
    // double-count vs. ingest.rs. For the h-tag path, ingest increments on
    // was_created=true and this handler only reaches create_channel() on a DB
    // lookup Err — an error recovery path where ingest's channel is
    // inaccessible, so the counter correctly records a new creation. For the
    // no-h-tag path, ingest never creates the channel, so this is the sole
    // increment.
    let channel = if let Some(client_uuid) = extract_h_tag_channel(event) {
        match state.db.get_channel(tenant.community(), client_uuid).await {
            Ok(ch) => ch,
            Err(_) => {
                // Channel not found — shouldn't happen (ingest_event pre-created it),
                // but fall back to creation to stay resilient.
                let ch = state
                    .db
                    .create_channel(
                        tenant.community(),
                        &name,
                        channel_type,
                        visibility,
                        description.as_deref(),
                        &actor_bytes,
                        ttl_seconds,
                        project_ref.as_deref(),
                    )
                    .await?;
                metrics::counter!(
                    "buzz_channels_created_total",
                    "community" => tenant.host().to_owned(),
                    "type" => channel_type.to_string()
                )
                .increment(1);
                ch
            }
        }
    } else {
        let ch = state
            .db
            .create_channel(
                tenant.community(),
                &name,
                channel_type,
                visibility,
                description.as_deref(),
                &actor_bytes,
                ttl_seconds,
                project_ref.as_deref(),
            )
            .await?;
        metrics::counter!(
            "buzz_channels_created_total",
            "community" => tenant.host().to_owned(),
            "type" => channel_type.to_string()
        )
        .increment(1);
        ch
    };

    // Creator becomes owner — evict any stale negative membership lookup.
    state.invalidate_membership(tenant, channel.id, &actor_bytes);
    // Open channels appear in everyone's accessible set; private channels only
    // affect the creator (the sole initial member).
    if visibility == beekeeper_db::channel::ChannelVisibility::Open {
        state.invalidate_all_accessible_channels(tenant);
    }

    let actor_hex = hex::encode(&actor_bytes);
    emit_system_message(
        tenant,
        state,
        channel.id,
        serde_json::json!({
            "type": "channel_created", "actor": actor_hex
        }),
    )
    .await?;

    if let Err(e) = emit_group_discovery_events(tenant, state, channel.id).await {
        warn!(channel = %channel.id, error = %e, "NIP-29 group discovery emission failed");
    }

    if let Err(e) = emit_membership_notification(
        tenant,
        state,
        channel.id,
        &actor_bytes,
        &actor_bytes, // creator is both actor and target
        KIND_MEMBER_ADDED_NOTIFICATION,
    )
    .await
    {
        warn!(channel = %channel.id, error = %e, "membership notification emission failed");
    }

    info!(channel_id = %channel.id, name = %name, "NIP-29 CREATE_GROUP processed");
    Ok(())
}

async fn handle_delete_group(
    tenant: &TenantContext,
    event: &Event,
    state: &Arc<AppState>,
) -> anyhow::Result<()> {
    let channel_id =
        extract_h_tag_channel(event).ok_or_else(|| anyhow::anyhow!("missing h tag"))?;
    let actor_bytes = event.pubkey.to_bytes().to_vec();

    // Soft-delete the channel.
    let deleted = state
        .db
        .soft_delete_channel(tenant.community(), channel_id)
        .await
        .map_err(|e| anyhow::anyhow!("soft_delete_channel failed: {e}"))?;

    if !deleted {
        warn!(channel = %channel_id, "channel already deleted or not found");
    }

    // Clean up NIP-29 discovery events for the deleted group.
    if let Err(e) = state
        .db
        .soft_delete_discovery_events(
            tenant.community(),
            channel_id,
            state.relay_keypair.public_key().as_bytes(),
        )
        .await
    {
        warn!(channel = %channel_id, error = %e, "failed to clean up NIP-29 discovery events");
    }

    // Deleted channel: clear both membership and accessible-channels caches.
    // Stale is_member=true entries would bypass the DB's deleted_at guard.
    state.invalidate_channel_deleted(tenant);

    let actor_hex = hex::encode(&actor_bytes);
    emit_system_message(
        tenant,
        state,
        channel_id,
        serde_json::json!({
            "type": "channel_deleted", "actor": actor_hex
        }),
    )
    .await?;

    info!(channel = %channel_id, "NIP-29 DELETE_GROUP processed");
    Ok(())
}

async fn handle_join_request(
    tenant: &TenantContext,
    event: &Event,
    state: &Arc<AppState>,
) -> anyhow::Result<()> {
    let channel_id =
        extract_h_tag_channel(event).ok_or_else(|| anyhow::anyhow!("missing h tag"))?;
    let actor_bytes = event.pubkey.to_bytes().to_vec();

    // Only open channels allow self-join via kind:9021.
    let channel = state
        .db
        .get_channel(tenant.community(), channel_id)
        .await
        .map_err(|_| anyhow::anyhow!("channel not found"))?;
    if channel.visibility != "open" {
        return Err(anyhow::anyhow!(
            "channel is private — request an invitation"
        ));
    }

    // Skip if already an active member — prevents duplicate join notifications.
    // Fail closed on DB errors rather than falling through to add_member.
    if state
        .is_member_cached(tenant.community(), channel_id, &actor_bytes)
        .await?
    {
        info!(channel = %channel_id, "kind:9021 join — already a member, skipping");
        return Ok(());
    }

    // Add as member (idempotent — add_member handles duplicates).
    state
        .db
        .add_member(
            tenant.community(),
            channel_id,
            &actor_bytes,
            beekeeper_db::channel::MemberRole::Member,
            None,
        )
        .await?;
    state.invalidate_membership(tenant, channel_id, &actor_bytes);

    let actor_hex = hex::encode(&actor_bytes);
    emit_system_message(
        tenant,
        state,
        channel_id,
        serde_json::json!({
            "type": "member_joined",
            "actor": actor_hex,
            "target": actor_hex,
        }),
    )
    .await?;

    if let Err(e) = emit_group_discovery_events(tenant, state, channel_id).await {
        warn!(channel = %channel_id, error = %e, "NIP-29 group discovery emission failed");
    }

    if let Err(e) = emit_membership_notification(
        tenant,
        state,
        channel_id,
        &actor_bytes,
        &actor_bytes,
        beekeeper_core::kind::KIND_MEMBER_ADDED_NOTIFICATION,
    )
    .await
    {
        warn!("membership notification for join failed: {e}");
    }

    info!(channel = %channel_id, "kind:9021 join processed");
    Ok(())
}

async fn handle_leave_request(
    tenant: &TenantContext,
    event: &Event,
    state: &Arc<AppState>,
) -> anyhow::Result<()> {
    // Kind 9022: functionally identical to self-remove via kind 9001
    let channel_id =
        extract_h_tag_channel(event).ok_or_else(|| anyhow::anyhow!("missing h tag"))?;
    let actor_bytes = event.pubkey.to_bytes().to_vec();

    // Guard: prevent last-owner orphaning on leave.
    let members = state.db.get_members(tenant.community(), channel_id).await?;
    let owner_count = members.iter().filter(|m| m.role == "owner").count();
    let actor_is_owner = members
        .iter()
        .any(|m| m.pubkey == actor_bytes && m.role == "owner");
    if actor_is_owner && owner_count <= 1 {
        return Err(anyhow::anyhow!(
            "cannot remove the last owner — transfer ownership first"
        ));
    }

    state
        .db
        .remove_member(tenant.community(), channel_id, &actor_bytes, &actor_bytes)
        .await?;
    state.invalidate_membership(tenant, channel_id, &actor_bytes);
    evict_live_channel_subscriptions(tenant, state, channel_id, &actor_bytes).await;
    disable_departed_member_workflows(tenant, state, channel_id, &actor_bytes).await;

    let actor_hex = hex::encode(&actor_bytes);
    emit_system_message(
        tenant,
        state,
        channel_id,
        serde_json::json!({
            "type": "member_left",
            "actor": actor_hex,
        }),
    )
    .await?;

    if let Err(e) = emit_group_discovery_events(tenant, state, channel_id).await {
        warn!(channel = %channel_id, error = %e, "NIP-29 group discovery emission failed");
    }

    if let Err(e) = emit_membership_notification(
        tenant,
        state,
        channel_id,
        &actor_bytes,
        &actor_bytes, // self-leave: actor == target
        KIND_MEMBER_REMOVED_NOTIFICATION,
    )
    .await
    {
        warn!(channel = %channel_id, error = %e, "membership notification emission failed");
    }

    Ok(())
}

// handle_reaction() removed — kind:7 reaction dedup and DB writes are now
// handled inline in ingest_event() before storage (see ingest.rs step 20a).

/// Handle NIP-09 deletion via `a` tag (addressable/parameterized-replaceable events).
/// Parses "kind:pubkey:d-tag" and deletes the corresponding DB record.
/// Handle a kind:30621 project head (NIP-MP Buzz access extension).
///
/// Projects the head's `buzz-access` level and invited-member `p` tags into
/// `project_acl` / `project_acl_members`. The event stays authoritative
/// (owner-curated republish); the projection backs the accessible-channels
/// SQL gate and the write-path project-membership check. Stale replays are
/// ignored by the projection's `head_created_at` guard.
async fn handle_project_acl_projection(
    tenant: &TenantContext,
    event: &Event,
    state: &Arc<AppState>,
) -> anyhow::Result<()> {
    let dtag =
        extract_tag_value(event, "d").ok_or_else(|| anyhow::anyhow!("kind:30621 missing d tag"))?;
    let visibility = if beekeeper_core::kind::is_private_project_event(event) {
        "private"
    } else {
        "public"
    };
    // Ingest validated each `p` value as lowercase 64-hex with no duplicates
    // and any 4th tag element as a pinned role; a role-less invite is a
    // legacy collaborator (pre-role members could already write).
    let members: Vec<beekeeper_db::project_acl::ProjectMember> = event
        .tags
        .iter()
        .filter_map(|t| {
            let parts = t.as_slice();
            if parts.first().map(|s| s.as_str()) != Some("p") {
                return None;
            }
            let pubkey = parts.get(1).and_then(|v| hex::decode(v.as_str()).ok())?;
            let role = parts
                .get(3)
                .and_then(|r| {
                    r.as_str()
                        .parse::<beekeeper_db::project_acl::ProjectRole>()
                        .ok()
                })
                .unwrap_or(beekeeper_db::project_acl::ProjectRole::Collaborator);
            Some((pubkey, role))
        })
        .collect();
    let members_applied = state
        .db
        .upsert_project_acl(
            tenant.community(),
            &event.pubkey.to_bytes(),
            &dtag,
            visibility,
            &members,
            event.created_at.as_secs() as i64,
        )
        .await?;
    // The accessible-channel set of every non-member changes when a project
    // flips visibility or its invite list; 30621 writes are rare relative to
    // reads, so the coarse community-wide flush is the right trade.
    state.invalidate_all_accessible_channels(tenant);
    let coordinate = format!("30621:{}:{}", hex::encode(event.pubkey.to_bytes()), dtag);
    emit_project_members_projection(tenant, state, &coordinate).await?;
    info!(
        d_tag = %dtag,
        visibility,
        members = members.len(),
        members_applied,
        "kind:30621 project ACL projected"
    );
    Ok(())
}

/// Project a kind:30623 shared-terminal announce head into
/// `shell_session_acl` (+ members): status, project coordinate, and the
/// role-tagged roster. Stale replays are ignored by the LWW guard; the
/// roster cache is flushed so revocations land immediately.
async fn handle_shell_session_acl_projection(
    tenant: &TenantContext,
    event: &Event,
    state: &Arc<AppState>,
) -> anyhow::Result<()> {
    let session_id =
        extract_tag_value(event, "d").ok_or_else(|| anyhow::anyhow!("kind:30623 missing d tag"))?;
    let coordinate = beekeeper_core::kind::shell_observe_project_ref(event)
        .ok_or_else(|| anyhow::anyhow!("kind:30623 missing a tag"))?;
    let status = extract_tag_value(event, "status").unwrap_or_else(|| "open".to_string());
    // Ingest validated every roster tag; unknown roles were rejected there,
    // and the parser skips them anyway (never a silent grant).
    let members: Vec<beekeeper_db::shell_session_acl::ShellMember> =
        beekeeper_core::kind::shell_session_roster(event)
            .into_iter()
            .filter_map(|(pubkey_hex, role)| {
                let pubkey = hex::decode(&pubkey_hex).ok()?;
                let role = role
                    .parse::<beekeeper_db::shell_session_acl::ShellRole>()
                    .ok()?;
                Some((pubkey, role))
            })
            .collect();
    state
        .db
        .upsert_shell_session_acl(
            tenant.community(),
            &event.pubkey.to_bytes(),
            &session_id,
            &coordinate,
            &status,
            &members,
            event.created_at.as_secs() as i64,
        )
        .await?;
    state.shell_roster_cache.invalidate_all();
    Ok(())
}

/// Apply a stored NIP-MP membership op (kind 9010 put-member / 9011
/// remove-member) to the relay-managed roster, then flush the read caches
/// and re-project the kind:39010 roster head.
///
/// Ingest validated the envelope and authorized the signer pre-storage; the
/// DB op re-checks inside its row-locked transaction, so a refusal here is a
/// lost race (e.g. the signer's own owner role was revoked concurrently) —
/// logged, never applied.
async fn handle_project_member_op(
    tenant: &TenantContext,
    kind: u32,
    event: &Event,
    state: &Arc<AppState>,
) -> anyhow::Result<()> {
    let coordinate = beekeeper_core::kind::project_membership_event_coordinate(event)
        .ok_or_else(|| anyhow::anyhow!("membership op missing project `a` tag"))?;
    let normalized = beekeeper_core::kind::normalize_project_coordinate(&coordinate)
        .ok_or_else(|| anyhow::anyhow!("membership op has malformed coordinate {coordinate:?}"))?;
    let mut segments = normalized.splitn(3, ':');
    let (_, Some(owner_hex), Some(dtag)) = (segments.next(), segments.next(), segments.next())
    else {
        return Err(anyhow::anyhow!(
            "membership op has malformed coordinate {coordinate:?}"
        ));
    };
    let owner = hex::decode(owner_hex)?;
    let actor = event.pubkey.to_bytes();

    let outcome = if kind == beekeeper_core::kind::KIND_PROJECT_PUT_MEMBER {
        // Ingest validated every target as `["p", hex, hint, role]`.
        let members: Vec<beekeeper_db::project_acl::ProjectMember> = event
            .tags
            .iter()
            .filter_map(|t| {
                let parts = t.as_slice();
                if parts.first().map(|s| s.as_str()) != Some("p") {
                    return None;
                }
                let pubkey = parts.get(1).and_then(|v| hex::decode(v.as_str()).ok())?;
                let role = parts.get(3).and_then(|r| {
                    r.as_str()
                        .parse::<beekeeper_db::project_acl::ProjectRole>()
                        .ok()
                })?;
                Some((pubkey, role))
            })
            .collect();
        state
            .db
            .put_project_members(tenant.community(), &owner, dtag, &actor, &members)
            .await?
    } else {
        let targets: Vec<Vec<u8>> = event
            .tags
            .iter()
            .filter_map(|t| {
                let parts = t.as_slice();
                if parts.first().map(|s| s.as_str()) == Some("p") {
                    parts.get(1).and_then(|v| hex::decode(v.as_str()).ok())
                } else {
                    None
                }
            })
            .collect();
        state
            .db
            .remove_project_members(tenant.community(), &owner, dtag, &actor, &targets)
            .await?
    };

    match outcome {
        beekeeper_db::project_acl::ProjectMemberOpOutcome::Applied => {
            state.invalidate_all_accessible_channels(tenant);
            emit_project_members_projection(tenant, state, &normalized).await?;
            info!(coordinate = %normalized, kind, "project membership op applied");
            Ok(())
        }
        beekeeper_db::project_acl::ProjectMemberOpOutcome::Refused(refusal) => {
            warn!(
                coordinate = %normalized,
                kind,
                ?refusal,
                "project membership op refused at apply time (ingest validation raced)"
            );
            Ok(())
        }
    }
}

/// Emit the relay-signed kind:39010 project roster projection for
/// `coordinate` (`d` = the coordinate, one `["p", hex, "", role]` per
/// member). Called after every roster change — head-sourced (30621 ingest)
/// or ops-sourced (9010/9011). Stored globally (no channel scope) and
/// withheld from readers outside a private project at the read chokepoints
/// via the hidden-project-coordinate predicate.
pub async fn emit_project_members_projection(
    tenant: &TenantContext,
    state: &Arc<AppState>,
    coordinate: &str,
) -> anyhow::Result<()> {
    let Some(roster) = state
        .db
        .get_project_roster(tenant.community(), coordinate)
        .await?
    else {
        // Unknown coordinate (e.g. the project was deleted between the op
        // and this projection) — nothing to project.
        return Ok(());
    };
    let mut tags: Vec<Tag> = Vec::with_capacity(roster.members.len() + 1);
    tags.push(Tag::parse(["d", coordinate])?);
    for (pubkey, role) in &roster.members {
        let pubkey_hex = hex::encode(pubkey);
        // Same convention as the NIP-29 39002 projection: ["p", pubkey,
        // relay_url, role], empty relay_url because the canonical relay is
        // implicit (this event is signed by it).
        tags.push(Tag::parse(["p", &pubkey_hex, "", role.as_str()])?);
    }

    // Strictly-increasing created_at, same reasoning as
    // `emit_addressable_discovery_event`: two roster ops inside one second
    // must not produce a replacement the stale-write guard rejects.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let min_ts = {
        let existing = state
            .db
            .query_events(&beekeeper_db::event::EventQuery {
                kinds: Some(vec![beekeeper_core::kind::KIND_PROJECT_MEMBERS as i32]),
                d_tag: Some(coordinate.to_string()),
                limit: Some(1),
                ..beekeeper_db::event::EventQuery::for_community(tenant.community())
            })
            .await
            .unwrap_or_default();
        existing
            .first()
            .map(|e| e.event.created_at.as_secs() + 1)
            .unwrap_or(now)
    };
    let ts = now.max(min_ts);

    let relay_pubkey_hex = hex::encode(state.relay_keypair.public_key().to_bytes());
    let projection = EventBuilder::new(
        Kind::Custom(beekeeper_core::kind::KIND_PROJECT_MEMBERS as u16),
        "",
    )
    .tags(tags)
    .custom_created_at(nostr::Timestamp::from(ts))
    .sign_with_keys(&state.relay_keypair)
    .map_err(|e| anyhow::anyhow!("failed to sign kind:39010: {e}"))?;

    // NIP-33 replacement keyed by the `d` coordinate: every project's roster
    // is its own slot. (`replace_addressable_event` keys by channel_id and
    // would make all 39010s share one global slot, so concurrent emissions
    // for different projects would clobber each other.)
    let (stored, was_inserted) = state
        .db
        .replace_parameterized_event(tenant.community(), &projection, coordinate, None)
        .await?;
    if was_inserted {
        let kind_u32 = event_kind_u32(&stored.event);
        dispatch_persistent_event(tenant, state, &stored, kind_u32, &relay_pubkey_hex, None).await;
    }
    Ok(())
}

/// Make a tombstoned repository actually gone, not merely unlisted.
///
/// Soft-deleting the kind:30617 announcement removes the repo from every
/// listing and drops its project link, and for a long time that was the
/// whole of "delete a repo". It left the repository fully cloneable: the git
/// transport resolves an object-store pointer, never the announcement, so
/// anybody who still had access could `git clone` a repo the UI said was
/// deleted. That is the shape of bug this codebase treats as severe — a
/// control that lies about what it enforces — so the deletion now reaches
/// the two things that actually serve the repo:
///
/// * the relay-signed kind:30618 ref state, which is a *second* public
///   record of the repo's branches and tags and is addressed to the relay's
///   own key, so the owner's tombstone cannot reach it; and
/// * the pointer at `repos/<community>/<owner>/<repo>/pointer`, whose
///   absence every read path already treats as a definitive
///   `repository not found` — the same generic denial every other refusal
///   uses.
///
/// What is deliberately kept: the `git_repo_names` reservation (deletion
/// never frees a name for another owner to squat) and the content-addressed
/// pack and manifest objects, which are shared between repositories and
/// cannot be reclaimed from here without risking another repo's history.
///
/// **Failures are logged, never propagated.** This runs after the tombstone
/// is stored, so returning an error cannot un-delete anything — it would
/// only lose the record of what was left behind. A pointer that survives is
/// the dangerous case (the repo stays cloneable), so it is logged at `error`
/// with everything an operator needs to finish the job by hand.
async fn delete_repo_git_state(
    tenant: &TenantContext,
    event: &Event,
    state: &Arc<AppState>,
    repo_id: &str,
    owner_hex: &str,
) {
    let owner_hex = owner_hex.to_ascii_lowercase();
    let deleted_at = event.created_at.as_secs() as i64;

    // The 30618 is relay-signed and addressed `30618:<relay>:<repo_id>`, so
    // it is invisible to the owner's own coordinate deletion. Same
    // created_at fence as every other branch here: a stale replayed
    // tombstone must not erase the ref state of a newer re-announce.
    let relay_pubkey = state.relay_keypair.public_key().to_bytes().to_vec();
    match state
        .db
        .soft_delete_by_coordinate(
            tenant.community(),
            KIND_GIT_REPO_STATE as i32,
            &relay_pubkey,
            repo_id,
            deleted_at,
        )
        .await
    {
        Ok(true) => tracing::info!(repo_id, "repo deletion: soft-deleted kind:30618 ref state"),
        Ok(false) => tracing::debug!(repo_id, "repo deletion: no live kind:30618 ref state"),
        Err(e) => tracing::error!(
            repo_id,
            "repo deletion: failed to soft-delete kind:30618 ref state, \
             the repo's branches and tags remain publicly readable: {e}"
        ),
    }

    let key = crate::api::git::manifest::pointer_key(tenant.community(), &owner_hex, repo_id);
    if let Err(e) = state.git_store.delete_pointer(&key).await {
        tracing::error!(
            repo_id,
            key,
            "repo deletion: FAILED TO DELETE POINTER — the repository is still \
             cloneable by anyone who can reach it. Delete this object-store key \
             by hand: {e}"
        );
    } else {
        tracing::info!(repo_id, key, "repo deletion: pointer removed");
    }
}

async fn handle_a_tag_deletion(
    tenant: &TenantContext,
    event: &Event,
    state: &Arc<AppState>,
) -> anyhow::Result<()> {
    let a_value = event
        .tags
        .iter()
        .find(|t| t.kind().to_string() == "a")
        .and_then(|t| t.content().map(|s| s.to_string()))
        .ok_or_else(|| anyhow::anyhow!("missing a tag for addressable deletion"))?;

    let parts: Vec<&str> = a_value.splitn(3, ':').collect();
    if parts.len() < 3 {
        return Err(anyhow::anyhow!("invalid a-tag format: {a_value}"));
    }
    let kind_num: u32 = parts[0]
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid kind in a-tag"))?;
    let pubkey_hex = parts[1];
    let d_tag = parts[2];
    let actor_bytes = effective_message_author(event, &state.relay_keypair.public_key());

    match kind_num {
        // kind:30350 revocation is exclusively a higher-generation inactive replacement.
        super::push_lease::KIND_PUSH_LEASE => {
            tracing::debug!(d_tag, "NIP-09 deletion ignored for push lease");
        }
        beekeeper_core::kind::KIND_WORKFLOW_DEF => {
            // Try UUID first (workflow_id); fall back to name-based lookup.
            if let Ok(wf_id) = uuid::Uuid::parse_str(d_tag) {
                let channel_id = state
                    .db
                    .delete_workflow_for_owner(tenant.community(), wf_id, &actor_bytes)
                    .await
                    .map_err(|e| anyhow::anyhow!("failed to delete workflow {wf_id}: {e}"))?;
                if let Some(channel_id) = channel_id {
                    state
                        .workflow_engine
                        .invalidate_channel_workflows(tenant.community(), channel_id);
                }
                tracing::info!(workflow_id = %wf_id, "Workflow deleted via NIP-09 a-tag (UUID)");
            } else {
                // Name-based lookup
                match state
                    .db
                    .find_workflow_by_owner_and_name(tenant.community(), &actor_bytes, d_tag)
                    .await
                {
                    Ok(Some(wf)) => {
                        let channel_id = state
                            .db
                            .delete_workflow_for_owner(tenant.community(), wf.id, &actor_bytes)
                            .await
                            .map_err(|e| {
                                anyhow::anyhow!("failed to delete workflow {}: {e}", wf.id)
                            })?;
                        if let Some(channel_id) = channel_id {
                            state
                                .workflow_engine
                                .invalidate_channel_workflows(tenant.community(), channel_id);
                        }
                        tracing::info!(workflow_id = %wf.id, name = d_tag, "Workflow deleted via NIP-09 a-tag (name)");
                    }
                    Ok(None) => {
                        tracing::warn!(
                            "NIP-09 a-tag deletion: no workflow '{d_tag}' found for owner"
                        );
                    }
                    Err(e) => {
                        tracing::warn!("NIP-09 a-tag deletion: DB lookup failed: {e}");
                    }
                }
            }
        }
        // Generic NIP-33 (parameterized-replaceable) soft-delete by coordinate.
        //
        // Listed after the workflow branch so workflow's bespoke deletion
        // (which doesn't soft-delete the `events` row by design — that's a
        // separate concern) takes precedence. For every other addressable
        // kind, including kind:30023 (NIP-23 long-form), we soft-delete the
        // live row matching `(kind, pubkey, d_tag)` so REQs stop returning it.
        // See https://github.com/block/sprout/issues/714.
        k if is_parameterized_replaceable(k) => {
            let pubkey_bytes = match hex::decode(pubkey_hex) {
                Ok(b) => b,
                Err(e) => {
                    return Err(anyhow::anyhow!(
                        "invalid pubkey hex in a-tag {pubkey_hex}: {e}"
                    ));
                }
            };
            // Safe cast: NIP-33 kinds are 30000–39999, well within i32.
            let kind_i32 = k as i32;
            // NIP-09 scopes an a-tag deletion to versions at or before the
            // deletion's own created_at, so a stale/replayed tombstone can never
            // erase a newer replacement head.
            let deleted = state
                .db
                .soft_delete_by_coordinate(
                    tenant.community(),
                    kind_i32,
                    &pubkey_bytes,
                    d_tag,
                    event.created_at.as_secs() as i64,
                )
                .await
                .map_err(|e| {
                    anyhow::anyhow!(
                        "failed to soft-delete by coordinate {kind_i32}:{pubkey_hex}:{d_tag}: {e}"
                    )
                })?;
            if deleted {
                tracing::info!(
                    kind = k,
                    d_tag = d_tag,
                    "NIP-09 a-tag deletion: soft-deleted addressable event by coordinate"
                );
            } else {
                tracing::debug!(
                    kind = k,
                    d_tag = d_tag,
                    "NIP-09 a-tag deletion: no live row matched coordinate"
                );
            }
            // A deleted project's ACL row must go with it (same created_at
            // scoping), so its channels revert to their own access rules.
            if k == KIND_PROJECT {
                let acl_dropped = state
                    .db
                    .delete_project_acl(
                        tenant.community(),
                        &pubkey_bytes,
                        d_tag,
                        event.created_at.as_secs() as i64,
                    )
                    .await?;
                if acl_dropped {
                    state.invalidate_all_accessible_channels(tenant);
                }
            }
            // A deleted repo announcement drops its project link (same
            // created_at scoping): with the 30617 gone the repo's child
            // events revert to their own access rules, mirroring
            // project-deletion semantics. The name reservation itself stays —
            // deletion never frees a name for another owner to squat.
            if k == KIND_GIT_REPO_ANNOUNCEMENT {
                let link_cleared = state
                    .db
                    .clear_repo_project_ref(
                        tenant.community(),
                        d_tag,
                        // git_repo_names stores the owner lowercase-hex.
                        &pubkey_hex.to_ascii_lowercase(),
                        event.created_at.as_secs() as i64,
                    )
                    .await?;
                if link_cleared {
                    state.invalidate_all_accessible_channels(tenant);
                }
                delete_repo_git_state(tenant, event, state, d_tag, pubkey_hex).await;
            }
            // A deleted shared-terminal announce drops its roster projection
            // (same created_at scoping), so the 24310 watch / 24312 input gates
            // fall back to project-only access instead of honoring invites on a
            // session that no longer exists. The flushed cache is the roster
            // cache — what this ACL gates — not the channel-access caches the
            // project/repo branches above flush.
            if k == KIND_SHELL_SESSION {
                let roster_dropped = state
                    .db
                    .delete_shell_session_acl(
                        tenant.community(),
                        &pubkey_bytes,
                        d_tag,
                        event.created_at.as_secs() as i64,
                    )
                    .await?;
                if roster_dropped {
                    state.shell_roster_cache.invalidate_all();
                }
            }
        }
        _ => {
            tracing::debug!(
                kind = kind_num,
                d_tag = d_tag,
                "NIP-09 a-tag deletion for non-NIP-33 kind — no side effect"
            );
        }
    }

    Ok(())
}

async fn handle_standard_deletion_event(
    tenant: &TenantContext,
    event: &Event,
    state: &Arc<AppState>,
) -> anyhow::Result<()> {
    let target_ids = extract_target_event_ids(event);
    if !has_e_tag(event) {
        // NIP-09 a-tag deletion path for addressable events. Keyed on the
        // absence of *any* e tag (not just valid e-ids): a malformed e + a must
        // not route here and silently soft-delete the coordinate.
        return handle_a_tag_deletion(tenant, event, state).await;
    }

    // The session this deletion retired, if it retired one. Collected in the
    // loop and answered once at the end, because the receipt is about the
    // *session*, not about each of the twenty-odd events a session delete
    // names.
    let mut retired_session: Option<(Vec<u8>, String, Uuid)> = None;

    for target_id in target_ids {
        let target_event = match state
            .db
            .get_event_by_id_including_deleted(tenant.community(), &target_id)
            .await?
        {
            Some(target) => target,
            None => continue,
        };
        if u32::from(target_event.event.kind.as_u16()) == super::push_lease::KIND_PUSH_LEASE {
            tracing::debug!(
                target_id = %hex::encode(&target_id),
                "NIP-09 deletion ignored for push lease"
            );
            continue;
        }

        let meta = state
            .db
            .get_thread_metadata_by_event(tenant.community(), &target_id)
            .await?;
        let parent_id = meta.as_ref().and_then(|m| m.parent_event_id.clone());
        let root_id = meta.as_ref().and_then(|m| m.root_event_id.clone());

        let deleted = state
            .db
            .soft_delete_event_and_update_thread(
                tenant.community(),
                &target_id,
                parent_id.as_deref(),
                root_id.as_deref(),
            )
            .await?;

        if !deleted {
            continue;
        }

        // A genesis row only ever goes soft-deleted as part of a whole-session
        // deletion: `refuse_permanent_identity_deletion` refuses it on its
        // own, and `authorize_coding_session_deletion` has already decided
        // that this actor — the founder, or an owner of the containing project
        // — may perform it and that the chain goes whole. So *this* line, on
        // the far side of a committed soft delete, is the only place that can
        // truthfully say "the relay applied a session deletion", which is
        // exactly what the receipt asserts. A replay finds the row already
        // deleted, `deleted` is false, and no second receipt is minted.
        if event_kind_u32(&target_event.event) == KIND_CODING_SESSION_GENESIS {
            match (
                decode_coding_session_genesis(&target_event.event.content),
                extract_h_tag_channel(&target_event.event),
            ) {
                (Ok(payload), Some(genesis_channel)) => {
                    retired_session = Some((
                        target_event.event.id.to_bytes().to_vec(),
                        payload.session_ref,
                        genesis_channel,
                    ));
                }
                _ => {
                    // Nothing is asserted about a genesis this relay cannot
                    // read back. The deletion still applied — the row is gone
                    // — but a receipt naming a session reference we had to
                    // guess at would be worse than none, and providers keep
                    // the founder-signed path (§3.2 (b)) for exactly this.
                    tracing::warn!(
                        genesis_id = %hex::encode(&target_id),
                        "deleted a coding-session genesis this relay cannot decode or scope; \
                         publishing no deletion receipt for it"
                    );
                }
            }
        }

        // Thread counters were decremented in the same transaction — push a
        // fresh relay-signed 39005 so live badge counts also count *down*.
        if let (Some(root_id), Some(channel_id)) = (root_id, target_event.channel_id) {
            emit_live_thread_summary(tenant, state, channel_id, root_id);
        }

        if u32::from(target_event.event.kind.as_u16()) == KIND_REACTION {
            // Try by reaction_event_id first; fall back to tuple-based removal
            // if the backfill was missed (set_reaction_event_id is best-effort).
            let removed = state
                .db
                .remove_reaction_by_source_event_id(tenant.community(), &target_id)
                .await
                .unwrap_or(false);
            if !removed {
                // Derive (target, actor, emoji) from the reaction event itself.
                // Use effective_message_author to handle legacy relay-signed
                // reactions where event.pubkey is the relay key, not the user.
                let actor = super::ingest::effective_message_author(
                    &target_event.event,
                    &state.relay_keypair.public_key(),
                );
                let emoji = if target_event.event.content.is_empty() {
                    "+"
                } else {
                    &target_event.event.content
                };
                if let Some(react_target_hex) = target_event.event.tags.iter().rev().find_map(|t| {
                    if t.kind().to_string() == "e" {
                        t.content().and_then(|v| {
                            if v.len() == 64 && v.chars().all(|c| c.is_ascii_hexdigit()) {
                                Some(v.to_string())
                            } else {
                                None
                            }
                        })
                    } else {
                        None
                    }
                }) {
                    if let Ok(react_target_id) = hex::decode(&react_target_hex) {
                        if let Ok(Some(react_target_event)) = state
                            .db
                            .get_event_by_id(tenant.community(), &react_target_id)
                            .await
                        {
                            let react_target_ts = chrono::DateTime::from_timestamp(
                                react_target_event.event.created_at.as_secs() as i64,
                                0,
                            )
                            .unwrap_or_else(chrono::Utc::now);
                            if let Err(e) = state
                                .db
                                .remove_reaction(
                                    tenant.community(),
                                    &react_target_id,
                                    react_target_ts,
                                    &actor,
                                    emoji,
                                )
                                .await
                            {
                                tracing::warn!(
                                    error = %e,
                                    "failed to remove reaction from DB during NIP-09 deletion"
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    if let Some((genesis_id, session_ref, channel_id)) = retired_session {
        emit_coding_session_deletion_receipt(
            tenant,
            state,
            channel_id,
            &genesis_id,
            &session_ref,
            event,
        )
        .await?;
    }

    Ok(())
}

/// Announce, under the relay's own key, that a whole-session deletion was
/// applied.
///
/// A provider must retire an umbrella before it republishes anything, and it
/// cannot verify a project owner's deletion on its own: the owner is not the
/// founder, and "was this actor an owner of the containing project" is a
/// question only the relay can answer. Without this receipt the only
/// verifiable retirement is a founder-signed one
/// (`docs/HANDOVER_IMPL.md` §3.2 (b)), which leaves every owner deletion
/// invisible to the machines that must stop working on it.
///
/// The shape is deliberately the authority receipt's: kind 40099, in the
/// session's own channel, signed by the relay identity a provider already
/// witnessed at connect time, with a `type` a reader routes on. Four facts and
/// no prose — the immutable genesis, its session reference, the deletion event
/// that did it, and the channel — because a provider that has to parse a
/// sentence to decide whether to stop is a provider that will get it wrong.
async fn emit_coding_session_deletion_receipt(
    tenant: &TenantContext,
    state: &Arc<AppState>,
    channel_id: Uuid,
    genesis_id: &[u8],
    session_ref: &str,
    deletion_event: &Event,
) -> anyhow::Result<()> {
    emit_system_message(
        tenant,
        state,
        channel_id,
        coding_session_deletion_receipt_content(
            genesis_id,
            session_ref,
            &deletion_event.id.to_hex(),
            channel_id,
        ),
    )
    .await
}

/// The receipt's exact content object, in one place so a test can assert its
/// shape without a database, a relay identity or a live channel.
///
/// Split from [`emit_coding_session_deletion_receipt`] deliberately: the shape
/// is the contract a provider parses, and the emission is plumbing. Only one of
/// those two can be checked cheaply, and it is the one that matters.
fn coding_session_deletion_receipt_content(
    genesis_id: &[u8],
    session_ref: &str,
    deletion_event_id: &str,
    channel_id: Uuid,
) -> serde_json::Value {
    serde_json::json!({
        "type": CODING_SESSION_DELETION_RECEIPT_TYPE,
        "genesisRef": hex::encode(genesis_id),
        "sessionRef": session_ref,
        "deletionEventId": deletion_event_id,
        "channelId": channel_id.to_string(),
    })
}

#[cfg(test)]
mod deletion_receipt_tests {
    use super::*;

    /// The four facts and the type, in the exact spelling
    /// `beekeeper_session_provider::authority::verify_deletion_receipt` parses.
    /// Both sides are `camelCase`, both name the immutable genesis rather than
    /// the reusable session label, and the receipt says which deletion did it
    /// so a provider can tell one retirement from a later re-founding.
    #[test]
    fn the_deletion_receipt_carries_exactly_the_four_facts_a_provider_verifies() {
        let channel_id = Uuid::from_u128(0x1234_5678_9abc_4def_8123_4567_89ab_cdef);
        let content = coding_session_deletion_receipt_content(
            &[0xab; 32],
            "dc580cfb-6c80-4fc2-8f4e-dfc328acf222",
            &"cd".repeat(32),
            channel_id,
        );
        assert_eq!(
            content,
            serde_json::json!({
                "type": "coding_session_deletion_accepted",
                "genesisRef": "ab".repeat(32),
                "sessionRef": "dc580cfb-6c80-4fc2-8f4e-dfc328acf222",
                "deletionEventId": "cd".repeat(32),
                "channelId": channel_id.to_string(),
            })
        );
        // The genesis is hex-encoded from bytes, never printed as a debug
        // slice: a provider comparing it against its own `genesis_ref` string
        // would never match otherwise.
        assert_eq!(
            content["genesisRef"].as_str().map(str::len),
            Some(64),
            "the genesis reference is a lowercase 64-hex id"
        );
    }

    /// The type string is the whole routing key on the provider's side, so it
    /// is asserted here rather than only where it is defined.
    #[test]
    fn the_receipt_type_is_the_word_the_provider_routes_on() {
        assert_eq!(
            CODING_SESSION_DELETION_RECEIPT_TYPE,
            "coding_session_deletion_accepted"
        );
    }
}

/// Extract channel UUID from `h` tag (NIP-29 group ID).
fn extract_h_tag_channel(event: &Event) -> Option<Uuid> {
    for tag in event.tags.iter() {
        if tag.kind().to_string() == "h" {
            if let Some(val) = tag.content() {
                if let Ok(id) = val.parse::<Uuid>() {
                    return Some(id);
                }
            }
        }
    }
    None
}

/// Extract target pubkey from first `p` tag.
fn extract_p_tag(event: &Event) -> Option<Vec<u8>> {
    for tag in event.tags.iter() {
        if tag.kind().to_string() == "p" {
            if let Some(val) = tag.content() {
                if let Ok(bytes) = hex::decode(val) {
                    if bytes.len() == 32 {
                        return Some(bytes);
                    }
                }
            }
        }
    }
    None
}

/// Extract the effective message author from a stored event.
///
/// REST-created messages are signed by the relay keypair and attribute the real
/// sender via a `p` tag. For user-signed events (WebSocket), `event.pubkey` is
/// the author. Returns the correct author bytes in both cases.
fn effective_message_author(event: &Event, relay_pubkey: &nostr::PublicKey) -> Vec<u8> {
    if event.pubkey == *relay_pubkey {
        if let Some(actor_hex) = extract_tag_value(event, "actor") {
            if let Ok(bytes) = hex::decode(actor_hex) {
                if bytes.len() == 32 {
                    return bytes;
                }
            }
        }
        for tag in event.tags.iter() {
            if tag.kind().to_string() == "p" {
                if let Some(hex) = tag.content() {
                    if let Ok(bytes) = hex::decode(hex) {
                        if bytes.len() == 32 {
                            return bytes;
                        }
                    }
                }
            }
        }
    }
    event.pubkey.to_bytes().to_vec()
}

/// True if the event carries any `e` tag at all, regardless of whether its
/// value decodes to a valid 32-byte id. NIP-09 treats `e`/`a` as target
/// classes: a malformed `e` makes the deletion ambiguous, not addressable-only.
/// Routing keys on this rather than on decoded-target count so a malformed `e`
/// alongside an `a` never silently soft-deletes a coordinate.
fn has_e_tag(event: &Event) -> bool {
    event.tags.iter().any(|t| t.kind().to_string() == "e")
}

fn extract_target_event_ids(event: &Event) -> Vec<Vec<u8>> {
    event
        .tags
        .iter()
        .filter_map(|tag| {
            if tag.kind().to_string() != "e" {
                return None;
            }

            tag.content().and_then(|value| {
                if value.len() == 64 && value.chars().all(|c| c.is_ascii_hexdigit()) {
                    hex::decode(value).ok().filter(|bytes| bytes.len() == 32)
                } else {
                    None
                }
            })
        })
        .collect()
}

/// Extract value of a named tag.
fn extract_tag_value(event: &Event, tag_name: &str) -> Option<String> {
    for tag in event.tags.iter() {
        if tag.kind().to_string() == tag_name {
            return tag.content().map(|s| s.to_string());
        }
    }
    None
}

fn copy_optional_string_field(event: &Event, object: &mut serde_json::Value, tag_name: &str) {
    let Some(value) = extract_tag_value(event, tag_name) else {
        return;
    };
    copy_optional_string_value(object, tag_name, value);
}

fn copy_optional_string_value(object: &mut serde_json::Value, field_name: &str, value: String) {
    if let Some(map) = object.as_object_mut() {
        map.insert(field_name.to_string(), serde_json::Value::String(value));
    }
}

fn has_moderation_delete_metadata(event: &Event) -> bool {
    ["action_id", "reason_code", "public_reason"]
        .iter()
        .any(|tag_name| extract_tag_value(event, tag_name).is_some())
}

fn author_delete_can_use_self_delete_path(author: &[u8], actor: &[u8], event: &Event) -> bool {
    author == actor && !has_moderation_delete_metadata(event)
}

#[cfg(test)]
fn delete_tombstone_content(
    actor_hex: String,
    target_event_id: String,
    action_id: Option<String>,
    reason_code: Option<String>,
    public_reason: Option<String>,
) -> serde_json::Value {
    let mut tombstone = serde_json::json!({
        "type": "message_deleted",
        "actor": actor_hex,
        "target_event_id": target_event_id,
    });
    if let Some(action_id) = action_id {
        copy_optional_string_value(&mut tombstone, "action_id", action_id);
    }
    if let Some(reason_code) = reason_code {
        copy_optional_string_value(&mut tombstone, "reason_code", reason_code);
    }
    if let Some(public_reason) = public_reason {
        copy_optional_string_value(&mut tombstone, "public_reason", public_reason);
    }
    tombstone
}

/// Validate a git repo identifier (d-tag value from kind:30617).
///
/// Rules: `[a-zA-Z0-9._-]{1,64}`, no leading dots, no `..`.
fn validate_repo_id(repo_id: &str) -> bool {
    !repo_id.is_empty()
        && repo_id.len() <= 64
        && !repo_id.starts_with('.')
        && !repo_id.contains("..")
        && repo_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
}

/// Handle kind:30617 (NIP-34 Git Repository Announcement).
///
/// Reserves the repo name and seeds its empty-manifest pointer when a repo
/// announcement event is stored. The event's `d` tag is the repo identifier;
/// the pubkey is the owner. No bare repo is created on disk — runtime reads
/// and writes hydrate an ephemeral repo from object storage per request.
///
/// Security hardening:
/// - Repo name validated: `[a-zA-Z0-9._-]{1,64}`, no leading dots, no `..`
/// - Name reserved atomically in Postgres (`git_repo_names`), unique per community
/// - Per-pubkey repo count limit enforced
async fn handle_git_repo_announcement(
    tenant: &TenantContext,
    event: &Event,
    state: &Arc<AppState>,
) -> anyhow::Result<()> {
    handle_git_repo_announcement_inner(tenant, event, state, &GitRepoAnnouncementHooks::default())
        .await
}

#[derive(Default)]
pub(crate) struct GitRepoAnnouncementHooks {
    #[cfg(test)]
    pub(crate) post_lease_gate: Option<Arc<GitRepoAnnouncementGate>>,
}

#[cfg(test)]
#[derive(Default)]
pub(crate) struct GitRepoAnnouncementGate {
    pub(crate) reached: tokio::sync::Notify,
    pub(crate) resume: tokio::sync::Notify,
}

pub(crate) async fn handle_git_repo_announcement_inner(
    tenant: &TenantContext,
    event: &Event,
    state: &Arc<AppState>,
    hooks: &GitRepoAnnouncementHooks,
) -> anyhow::Result<()> {
    #[cfg(not(test))]
    let _ = hooks;
    // Extract repo identifier from d tag (required for NIP-33 parameterized replaceable events).
    let repo_id =
        extract_tag_value(event, "d").ok_or_else(|| anyhow::anyhow!("kind:30617 missing d tag"))?;

    if !validate_repo_id(&repo_id) {
        return Err(anyhow::anyhow!(
            "invalid repo identifier: must be [a-zA-Z0-9._-]{{1,64}}, no leading dots, no '..'"
        ));
    }

    let owner_hex = hex::encode(event.pubkey.to_bytes());

    // The relay holds no persistent per-repo disk state: runtime reads and
    // writes hydrate an ephemeral bare repo from object storage per request
    // (see `api::git::hydrate`). Announce only (1) reserves the repo name and
    // (2) seeds the empty-manifest pointer that makes the repo clone-able.
    //
    // The `git_repo_names` table (Postgres) is the relay's name registry,
    // keyed `(community_id, repo_id)`. It serves three jobs at once inside the
    // server-resolved community boundary:
    //   - uniqueness: `INSERT … ON CONFLICT DO NOTHING` is atomic, so
    //     concurrent kind:30617 events for the same community/name can't both
    //     claim it (TOCTOU-free — the DB PK is the race guard);
    //   - idempotent re-announce: a reservation owned by the same pubkey is an
    //     update, not a collision;
    //   - per-pubkey quota: `COUNT` reservations owned by this pubkey.
    //
    // This replaces the v1 local-disk `.names/` index. Moving it into Postgres
    // (which the relay already requires) removes the last persistent local-disk
    // state, so separate replicas no longer need a shared ReadWriteMany volume
    // to agree on name ownership. Actual ref-state safety remains the
    // object-store pointer CAS (`api::git::cas_publish`, `Inv_NoFork`); this
    // registry only governs name allocation.
    let community = tenant.community();
    use beekeeper_db::git_repo::ReserveOutcome;

    // Classify the name first: same-owner re-announce is idempotent; a name
    // held by anyone else is a collision (the relay signs kind:30618 with
    // d-tag = repo_name, so a shared name would let one owner overwrite
    // another's ref state). For a not-yet-owned name we must check quota
    // *before* claiming, so we peek the current holder rather than inserting
    // blindly.
    //
    // Crucially, we do NOT return early on a same-owner existing row: the row
    // proves name *ownership*, not that the manifest pointer was actually
    // seeded. A concurrent same-owner announce could hold the row while its
    // seed is still in flight (or failed and rolled back), so trusting the row
    // alone would let this handler "accept" an uncloneable repo. Instead we
    // fall through to `seed_manifest_pointer`, which is idempotent under
    // concurrency (create-only `put_pointer(IfNoneMatchStar)`; a `LostRace` on
    // the same empty digest is success, a different non-empty pointer is a
    // hard error). So re-announce *ensures* the pointer rather than assuming it.
    let outcome =
        if let Some(existing_owner) = state.db.repo_name_owner(community, &repo_id).await? {
            if existing_owner != owner_hex {
                return Err(anyhow::anyhow!(
                    "repo name '{repo_id}' already taken by another owner"
                ));
            }
            // Same owner: the reservation already exists (this attempt did not
            // create it), so it must never be rolled back by this attempt, and the
            // per-pubkey quota is unchanged (re-announce never grows the count).
            ReserveOutcome::AlreadyOwned
        } else {
            // Not yet owned by anyone we saw: enforce the per-pubkey quota, then
            // claim the name atomically. The `ON CONFLICT` guard resolves a
            // concurrent announce even though the peek above missed it —
            // `Reserved` means *this attempt* won the insert, `AlreadyOwned` means
            // a same-owner sibling won it, `TakenByOther` is a cross-owner
            // collision.
            let limit = state.config.git_max_repos_per_pubkey as i64;
            let owned = state
                .db
                .count_repos_for_owner(community, &owner_hex)
                .await?;
            if owned >= limit {
                return Err(anyhow::anyhow!("repo limit exceeded: {owned} >= {limit}"));
            }
            match state
                .db
                .reserve_repo_name(community, &repo_id, &owner_hex)
                .await?
            {
                outcome @ (ReserveOutcome::Reserved | ReserveOutcome::AlreadyOwned) => outcome,
                ReserveOutcome::TakenByOther => {
                    return Err(anyhow::anyhow!(
                        "repo name '{repo_id}' already taken by another owner"
                    ));
                }
            }
        };

    // Only a genuinely fresh claim by *this* attempt may be rolled back on a
    // pointer failure. An `AlreadyOwned` outcome means the row is owned by some
    // other attempt (a same-owner sibling, or a prior announce that has since
    // pushed), and deleting it here would strand a repo whose pointer that
    // other attempt already established.
    let reserved_by_this_attempt = matches!(outcome, ReserveOutcome::Reserved);

    // The event row and name registry are ordinary database state: if deletion
    // quiescing wins before they commit, the DB write fence rejects them; if
    // they committed first, the destructive DB stage purges them. The manifest
    // and pointer below are external S3 effects, so acquire the durable
    // serving-write lease immediately before that sequence. Once acquired,
    // deletion must drain this lease before it can freeze the final object list.
    let serving_write = beekeeper_deletion::acquire_serving_write(
        &state.db,
        tenant.community(),
        "git_repo_announcement",
    )
    .await
    .map_err(|e| anyhow::anyhow!("repo announcement rejected by community deletion fence: {e}"))?;

    #[cfg(test)]
    if let Some(gate) = &hooks.post_lease_gate {
        gate.reached.notify_one();
        gate.resume.notified().await;
    }

    if let Err(error) = serving_write.verify().await {
        return Err(anyhow::anyhow!(
            "repo announcement lost community serving lease: {error}"
        ));
    }

    // Establish/confirm the manifest pointer, keeping the invariant
    // "repo announced ⟺ pointer exists" so the read path can rely on
    // pointer-absent meaning never-announced (keeping `info_refs`'s fail-closed
    // `Ok(None) → 404` unambiguous). Two distinct cases:
    //
    // - Fresh `Reserved` claim → `seed_manifest_pointer` (strict). This creates
    //   the empty pointer, and correctly *fails* if a non-empty pointer already
    //   exists for a name we just reserved — that would be a suspicious stale
    //   pointer from a prior repo lifecycle, not a legitimate re-announce.
    // - Same-owner `AlreadyOwned` (re-announce) → `ensure_manifest_pointer`
    //   (tolerant). A non-empty pointer is the *normal* post-push state, so
    //   re-announce must accept it untouched; only an absent pointer is
    //   repaired by seeding. Using the strict seed here would wrongly reject
    //   every re-announce after the first push.
    let pointer_operation = async {
        if reserved_by_this_attempt {
            seed_manifest_pointer(state, tenant, &owner_hex, &repo_id).await
        } else {
            ensure_manifest_pointer(state, tenant, &owner_hex, &repo_id).await
        }
    };
    let pointer_result = match serving_write.protect(pointer_operation).await {
        Ok(result) => result,
        Err(error) => Err(error),
    };
    if let Err(pointer_err) = pointer_result {
        // A reserved name without a clone-able pointer is exactly the broken
        // state this step exists to prevent — but ONLY roll back the
        // reservation if this attempt is the one that freshly created it. A
        // genuine failure from a fresh `Reserved` attempt means the pointer
        // truly could not be established, so releasing our own just-inserted
        // row is safe and correct (all-or-nothing). For an `AlreadyOwned`
        // attempt we release nothing: the row belongs to another attempt that
        // may have seeded (or pushed) successfully.
        if reserved_by_this_attempt {
            if let Err(release_err) = state
                .db
                .release_repo_name(community, &repo_id, &owner_hex)
                .await
            {
                warn!(
                    repo_id = %repo_id,
                    error = %release_err,
                    "failed to release repo name reservation after seed failure"
                );
            }
        }
        return Err(anyhow::anyhow!(
            "failed to ensure manifest pointer: {pointer_err}"
        ));
    }

    // Project the repo → project link (NIP-MP access extension phase 2).
    // Ingest already validated the tag's shape and the author's project
    // membership, so a `None` here means "no (valid) project tag" and clears
    // any previous link. LWW-guarded like the project ACL projection; the
    // repo-gating caches flush community-wide because a link change alters
    // the hidden-repo set of every non-member.
    let project_ref = beekeeper_core::kind::repo_project_ref(event);
    let link_changed = state
        .db
        .set_repo_project_ref(
            community,
            &repo_id,
            &owner_hex,
            project_ref.as_deref(),
            event.created_at.as_secs() as i64,
        )
        .await?;
    if link_changed {
        state.invalidate_all_accessible_channels(tenant);
    }

    info!(
        repo_id = %repo_id,
        owner = %owner_hex,
        reserved = reserved_by_this_attempt,
        project_ref = project_ref.as_deref().unwrap_or(""),
        "kind:30617 repo announced (name reserved, manifest pointer ensured)"
    );

    // Derived after the pointer commits: kind:30618 ref-state event over the
    // seeded empty manifest. Pointer is the commit; this event is the
    // notification that the repo exists (with empty refs) so subscribers see
    // a first signal without waiting for the first push.
    //
    // Emit ONLY on a fresh `Reserved` claim. On a same-owner `AlreadyOwned`
    // re-announce the pointer already exists (and, after the first push, holds
    // real refs). Re-emitting the empty-refs 30618 here would publish a *newer*
    // replaceable event that, under NIP-16 latest-wins ordering, shadows the
    // real pushed refs — making a live repo look empty to subscribers. The
    // initial empty signal is a one-time seeding notification, not something a
    // re-announce should replay.
    if reserved_by_this_attempt {
        if let Err(e) =
            emit_initial_ref_state(tenant, state, serving_write.lease(), &owner_hex, &repo_id).await
        {
            // Non-fatal: the manifest is the source of truth; this is just the
            // derived notification. A failure here means subscribers miss the
            // "repo now exists" event, but clone/push still works.
            warn!(
                repo_id = %repo_id,
                owner = %owner_hex,
                error = %e,
                "failed to emit initial kind:30618 ref state (non-fatal)"
            );
        }
    }

    serving_write.finish().await.map_err(|e| {
        anyhow::anyhow!("repo announcement lost community serving lease on release: {e}")
    })?;
    Ok(())
}

/// Default symbolic HEAD for a freshly-announced (empty) repo. Matches
/// `init.defaultBranch=main` (git ≥ 2.28) and the seed used by
/// `live_hydrate_empty_repo`. Pinned in one place so the seeded manifest
/// and the initial kind:30618 emission can't drift.
///
/// The first push's `cas_publish` overwrites this with the real symbolic
/// HEAD observed in the receive-pack workspace via standard CAS, so the
/// default is a stand-in, not a permanent commitment.
const DEFAULT_HEAD: &str = "refs/heads/main";

/// Seed the manifest-pointer for a newly-announced repo with an empty manifest.
///
/// Idempotent: a `CasOutcome::LostRace` is treated as success **only if** the
/// existing pointer names the same empty manifest digest. Any other pre-existing
/// pointer body (e.g. a non-empty manifest from a previous announce/push pair
/// for the same `(owner, repo)`) surfaces as an error rather than silently
/// succeeding — that would mask a real misconfiguration.
async fn seed_manifest_pointer(
    state: &Arc<AppState>,
    tenant: &TenantContext,
    owner_hex: &str,
    repo_id: &str,
) -> anyhow::Result<()> {
    use crate::api::git::manifest::{pointer_key, Manifest, MANIFEST_VERSION};
    use crate::api::git::store::{CasOutcome, Precond};
    use std::collections::BTreeMap;

    // The empty manifest. All empty manifests across all repos share canonical
    // bytes — by design — so `put_manifest` is idempotent at the store level
    // too.
    let empty = Manifest {
        version: MANIFEST_VERSION,
        head: DEFAULT_HEAD.to_string(),
        refs: BTreeMap::new(),
        packs: Vec::new(),
        parent: None,
    };
    empty
        .validate()
        .map_err(|e| anyhow::anyhow!("empty manifest failed validation: {e}"))?;
    let bytes = empty
        .canonical_bytes()
        .map_err(|e| anyhow::anyhow!("empty manifest serialize: {e}"))?;
    let manifest_key = state
        .git_store
        .put_manifest(&bytes)
        .await
        .map_err(|e| anyhow::anyhow!("put_manifest: {e}"))?;
    let digest = manifest_key
        .strip_prefix("manifests/")
        .ok_or_else(|| anyhow::anyhow!("put_manifest returned non-standard key: {manifest_key}"))?;

    let pkey = pointer_key(tenant.community(), owner_hex, repo_id);
    let outcome = state
        .git_store
        .put_pointer(&pkey, digest.as_bytes(), Precond::IfNoneMatchStar)
        .await
        .map_err(|e| anyhow::anyhow!("put_pointer: {e}"))?;
    match outcome {
        CasOutcome::Won(_) => Ok(()),
        CasOutcome::LostRace => {
            // Pointer already exists. Idempotency check: only treat as success
            // if it names the same empty manifest digest. Any other value is
            // either a stale pointer from a prior repo lifecycle for the same
            // (owner, repo) or a real misconfiguration — surface, don't swallow.
            let (_etag, body) = state
                .git_store
                .get_pointer(&pkey)
                .await
                .map_err(|e| anyhow::anyhow!("re-read pointer after LostRace: {e}"))?
                .ok_or_else(|| anyhow::anyhow!("pointer vanished after LostRace race"))?;
            let existing = std::str::from_utf8(&body)
                .map_err(|e| anyhow::anyhow!("pointer body not utf-8: {e}"))?
                .trim();
            if existing != digest {
                return Err(anyhow::anyhow!(
                    "repo '{repo_id}' for owner {owner_hex} already has a non-empty pointer \
                     ({existing}); refusing to overwrite via announce"
                ));
            }
            Ok(())
        }
    }
}

/// Ensure a manifest pointer exists for an already-owned repo (same-owner
/// re-announce path). Unlike [`seed_manifest_pointer`], which is strict for
/// *creation* (it refuses when a non-empty pointer already exists, since a
/// freshly-reserved name with a populated pointer is suspicious), this is the
/// tolerant *idempotent* path for a name this owner already holds:
///
/// - **pointer present** (empty *or* non-empty) → success, left untouched. A
///   non-empty pointer is the normal state after the owner has pushed; a
///   re-announce must not fail just because the repo has commits, and must
///   never overwrite real ref state.
/// - **pointer absent** → seed the empty pointer (repair the "row exists but
///   pointer missing" window, e.g. a prior announce whose seed failed after
///   the row was inserted by a sibling attempt). This restores the
///   "announced ⟺ pointer exists" invariant.
///
/// The read-then-conditional-seed is race-safe: the repair uses
/// `seed_manifest_pointer`'s create-only `put_pointer(IfNoneMatchStar)`, so a
/// concurrent seeder that wins is resolved by that function's `LostRace`
/// handling (same empty digest → Ok), and a concurrent *pusher* that populates
/// the pointer between our read and our seed loses the create race and is
/// likewise treated as an already-present pointer, not an overwrite.
async fn ensure_manifest_pointer(
    state: &Arc<AppState>,
    tenant: &TenantContext,
    owner_hex: &str,
    repo_id: &str,
) -> anyhow::Result<()> {
    use crate::api::git::manifest::pointer_key;

    let pkey = pointer_key(tenant.community(), owner_hex, repo_id);
    let existing = state
        .git_store
        .get_pointer(&pkey)
        .await
        .map_err(|e| anyhow::anyhow!("get_pointer: {e}"))?;
    match existing {
        // Any existing pointer (empty or non-empty) is valid for a same-owner
        // re-announce — leave it exactly as-is.
        Some(_) => Ok(()),
        // No pointer yet: repair by seeding the empty pointer. `LostRace` to a
        // concurrent seeder/pusher is handled by `seed_manifest_pointer`.
        None => seed_manifest_pointer(state, tenant, owner_hex, repo_id).await,
    }
}

/// Emit the initial kind:30618 ref-state event for a freshly-announced repo.
///
/// The seeded empty manifest is the source of truth; this event is the
/// derived notification. Fires once per announce, signed by the relay,
/// carrying the announcer's pubkey in the `p` tag (buzz extension).
async fn emit_initial_ref_state(
    tenant: &TenantContext,
    state: &Arc<AppState>,
    lease: &beekeeper_db::deletion::ServingWriteLease,
    owner_hex: &str,
    repo_id: &str,
) -> anyhow::Result<()> {
    use crate::api::git::manifest_event::{build_ref_state_event, RefStateInputs};
    use std::collections::BTreeMap;

    let empty_refs: BTreeMap<String, String> = BTreeMap::new();
    let inputs = RefStateInputs {
        repo_id,
        head: DEFAULT_HEAD,
        refs: &empty_refs,
        actor_pubkey_hex: owner_hex,
    };
    let event = build_ref_state_event(&inputs, &state.relay_keypair)
        .map_err(|e| anyhow::anyhow!("build_ref_state_event: {e}"))?;
    let (stored, was_inserted) = state
        .db
        .insert_event_with_serving_write_guard(lease, &event, None)
        .await
        .map_err(|e| anyhow::anyhow!("insert kind:30618: {e}"))?;
    if was_inserted {
        // Routed through the guarded send path for uniformity; the access gate
        // no-ops for this globally-scoped (channel_id = None) ref-state event.
        crate::handlers::event::fan_out_event_to_local_subscribers(
            state,
            tenant.community(),
            &stored,
        )
        .await;
    }
    Ok(())
}

/// Reconcile every community's event-backed NIP-43 membership view.
///
/// `relay_members` is canonical. A snapshot is rebuilt only when it is absent
/// or its member/role set differs from the canonical rows. This makes the sweep
/// safe to run at startup and periodically without producing an event stream
/// when nothing changed. A failure in one community is logged and counted but
/// does not prevent the remaining communities from being repaired.
pub async fn reconcile_nip43_membership_snapshots(state: &Arc<AppState>) -> anyhow::Result<usize> {
    let communities = state.db.usage_community_hosts().await?;
    let mut reconciled = 0usize;

    for community in communities {
        let community_id = beekeeper_core::CommunityId::from_uuid(community.id);
        let host = community.host;
        let result = async {
            if !state
                .db
                .nip43_membership_snapshot_needs_reconciliation(
                    community_id,
                    &state.relay_keypair.public_key(),
                )
                .await?
            {
                return Ok::<bool, anyhow::Error>(false);
            }

            let tenant = TenantContext::resolved(community_id, host.clone());
            publish_nip43_membership_list(&tenant, state).await?;
            Ok::<bool, anyhow::Error>(true)
        }
        .await;

        match result {
            Ok(true) => reconciled += 1,
            Ok(false) => {}
            Err(error) => {
                metrics::counter!("buzz_nip43_membership_reconciliation_failures_total")
                    .increment(1);
                warn!(%community_id, %host, %error, "NIP-43 membership reconciliation failed");
            }
        }
    }

    metrics::counter!("buzz_nip43_membership_reconciliations_total").increment(reconciled as u64);
    Ok(reconciled)
}

/// Publish a kind:13534 relay membership list event (NIP-43).
///
/// Queries all current relay members and emits a relay-signed, NIP-70-protected
/// addressable event listing every member pubkey. Replaces any previous list.
///
/// The member query, event construction, and replacement all happen inside
/// the same per-community advisory lock held by
/// `publish_nip43_membership_locked`. This prevents a stale snapshot from
/// overwriting a newer one when two publications race: the lock serializes
/// the entire read-build-write cycle, not just the write.
pub async fn publish_nip43_membership_list(
    tenant: &TenantContext,
    state: &Arc<AppState>,
) -> anyhow::Result<()> {
    let started_at = std::time::Instant::now();
    metrics::counter!("buzz_nip43_membership_publications_total", "result" => "attempted")
        .increment(1);
    let result = publish_nip43_membership_list_inner(tenant, state).await;
    metrics::histogram!("buzz_nip43_membership_publication_seconds")
        .record(started_at.elapsed().as_secs_f64());
    metrics::counter!(
        "buzz_nip43_membership_publications_total",
        "result" => if result.is_ok() { "succeeded" } else { "failed" }
    )
    .increment(1);
    result
}

async fn publish_nip43_membership_list_inner(
    tenant: &TenantContext,
    state: &Arc<AppState>,
) -> anyhow::Result<()> {
    let relay_pubkey_hex = state.relay_keypair.public_key().to_hex();
    let community = tenant.community();

    // The DB method acquires the per-community snapshot lock BEFORE reading
    // members, so that the entire read-build-write cycle is serialized. A
    // concurrent publication that reads newer state will block until our
    // replacement commits, then read the updated membership. This prevents a
    // stale snapshot from winning by arrival order.
    let (stored, was_inserted, member_count) = state
        .db
        .publish_nip43_membership_locked(community, &state.relay_keypair)
        .await?;

    if was_inserted {
        dispatch_persistent_event(
            tenant,
            state,
            &stored,
            KIND_NIP43_MEMBERSHIP_LIST,
            &relay_pubkey_hex,
            None,
        )
        .await;
    }

    info!(member_count, "NIP-43 membership list published");
    Ok(())
}

/// Shared helper: publish a NIP-43 membership delta event (kind 8000 or 8001).
///
/// Signs a relay event with `["-"]` (NIP-70) + `["p", target]` tags, stores it
/// globally, and fans out to matching subscribers.
async fn publish_nip43_delta(
    tenant: &TenantContext,
    state: &Arc<AppState>,
    kind: u16,
    target_pubkey_hex: &str,
    label: &str,
) -> anyhow::Result<()> {
    let relay_pubkey_hex = state.relay_keypair.public_key().to_hex();

    let tags = vec![
        Tag::parse(["-"]).map_err(|e| anyhow::anyhow!("failed to build '-' tag: {e}"))?,
        Tag::parse(["p", target_pubkey_hex])
            .map_err(|e| anyhow::anyhow!("failed to build p tag: {e}"))?,
    ];

    let event = EventBuilder::new(Kind::Custom(kind), "")
        .tags(tags)
        .sign_with_keys(&state.relay_keypair)
        .map_err(|e| anyhow::anyhow!("failed to sign kind:{kind}: {e}"))?;

    let (stored, was_inserted) = state
        .db
        .insert_event(tenant.community(), &event, None)
        .await?;
    if !was_inserted {
        return Ok(());
    }

    // Routed through the guarded send path for uniformity; the access gate
    // no-ops for this globally-scoped (channel_id = None) NIP-43 event.
    crate::handlers::event::fan_out_event_to_local_subscribers(state, tenant.community(), &stored)
        .await;

    info!(
        target = %target_pubkey_hex,
        relay = %relay_pubkey_hex,
        "NIP-43 {label} event published"
    );
    Ok(())
}

/// Publish a kind:8000 relay member-added announcement event (NIP-43).
pub async fn publish_nip43_member_added(
    tenant: &TenantContext,
    state: &Arc<AppState>,
    target_pubkey_hex: &str,
) -> anyhow::Result<()> {
    publish_nip43_delta(tenant, state, 8000, target_pubkey_hex, "member-added").await
}

/// Publish a kind:8001 relay member-removed announcement event (NIP-43).
pub async fn publish_nip43_member_removed(
    tenant: &TenantContext,
    state: &Arc<AppState>,
    target_pubkey_hex: &str,
) -> anyhow::Result<()> {
    publish_nip43_delta(tenant, state, 8001, target_pubkey_hex, "member-removed").await
}

/// Reconcile channels that exist in the DB but don't have kind:39000 events.
///
/// This handles the case where channels were created via direct SQL inserts
/// (e.g. test seed scripts) rather than through the Nostr event pipeline.
/// Emits kind:39000 (metadata) and kind:39002 (members) for each channel
/// that is missing its discovery events.
///
/// Idempotent: checks for existing kind:39000 events before emitting.
pub async fn reconcile_channel_events(
    tenant: &TenantContext,
    state: &Arc<AppState>,
) -> anyhow::Result<()> {
    use beekeeper_db::event::EventQuery;

    let channels = state.db.list_channels(tenant.community(), None).await?;
    if channels.is_empty() {
        return Ok(());
    }

    let mut reconciled = 0u32;
    for channel in &channels {
        // Check if kind:39000 event already exists for this channel.
        let channel_id_str = channel.id.to_string();
        let existing = match state
            .db
            .query_events(&EventQuery {
                kinds: Some(vec![39000]),
                d_tag: Some(channel_id_str.clone()),
                limit: Some(1),
                ..EventQuery::for_community(tenant.community())
            })
            .await
        {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(
                    channel_id = %channel.id,
                    error = %e,
                    "reconcile: failed to query existing discovery events"
                );
                continue;
            }
        };

        if existing.is_empty() {
            // No discovery event — emit one.
            if let Err(e) = emit_group_discovery_events(tenant, state, channel.id).await {
                tracing::warn!(
                    channel_id = %channel.id,
                    error = %e,
                    "reconcile: failed to emit discovery events"
                );
            } else {
                reconciled += 1;
            }
        }
    }

    if reconciled > 0 {
        tracing::info!(count = reconciled, "reconciled channel discovery events");
    }
    Ok(())
}

/// Publish a kind:13535 archived identities list event (NIP-IA).
///
/// Queries all current archived identities and emits a relay-signed,
/// NIP-70-protected replaceable-by-convention snapshot with bare `p` tags.
pub async fn publish_nipia_archival_list(
    tenant: &TenantContext,
    state: &Arc<AppState>,
) -> anyhow::Result<()> {
    let archived = state.db.list_archived(tenant.community()).await?;
    let relay_pubkey_hex = state.relay_keypair.public_key().to_hex();

    let mut tags: Vec<Tag> = Vec::with_capacity(archived.len() + 1);
    tags.push(Tag::parse(["-"]).map_err(|e| anyhow::anyhow!("failed to build '-' tag: {e}"))?);

    for identity in &archived {
        tags.push(
            Tag::parse(["p", &identity.pubkey])
                .map_err(|e| anyhow::anyhow!("failed to build p tag: {e}"))?,
        );
    }

    let event = EventBuilder::new(Kind::Custom(KIND_IA_ARCHIVED_LIST as u16), "")
        .tags(tags)
        .sign_with_keys(&state.relay_keypair)
        .map_err(|e| anyhow::anyhow!("failed to sign kind:{KIND_IA_ARCHIVED_LIST}: {e}"))?;

    let (stored, was_inserted) = state
        .db
        .replace_addressable_event(tenant.community(), &event, None)
        .await?;
    if was_inserted {
        dispatch_persistent_event(
            tenant,
            state,
            &stored,
            KIND_IA_ARCHIVED_LIST,
            &relay_pubkey_hex,
            None,
        )
        .await;
    }

    info!(
        archived_count = archived.len(),
        "NIP-IA archived identities list published"
    );
    Ok(())
}

/// NIP-DV: publish the relay-signed, per-viewer DM visibility snapshot for
/// `viewer`. The event is parameterized-replaceable (`d` = viewer pubkey) and
/// carries one `h` tag per DM the viewer currently has hidden. Called after any
/// hide (41012) or unhide (41010 that clears `hidden_at`); the latest event is
/// always the authoritative hidden set, so no client-side delta merge is needed.
pub async fn publish_dm_visibility_snapshot(
    tenant: &TenantContext,
    state: &Arc<AppState>,
    viewer: &[u8],
) -> anyhow::Result<()> {
    let viewer_hex = hex::encode(viewer);
    let hidden = state.db.list_hidden_dms(tenant.community(), viewer).await?;
    let relay_pubkey_hex = state.relay_keypair.public_key().to_hex();

    let mut tags: Vec<Tag> = Vec::with_capacity(hidden.len() + 2);
    tags.push(
        Tag::parse(["d", &viewer_hex])
            .map_err(|e| anyhow::anyhow!("failed to build d tag: {e}"))?,
    );
    // `p` = viewer so the relay's `#p`-gated read path scopes the snapshot to
    // its owner; no one else may query another viewer's hidden-DM set.
    tags.push(
        Tag::parse(["p", &viewer_hex])
            .map_err(|e| anyhow::anyhow!("failed to build p tag: {e}"))?,
    );
    for channel_id in &hidden {
        tags.push(
            Tag::parse(["h", &channel_id.to_string()])
                .map_err(|e| anyhow::anyhow!("failed to build h tag: {e}"))?,
        );
    }

    // Force created_at strictly past any prior snapshot for this viewer: a same-second
    // replacement whose random event id sorts higher is rejected by stale-write
    // protection, so a hide→re-open within one second could otherwise strand the stale
    // snapshot. Same guard as emit_addressable_discovery_event.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let ts = {
        let existing = state
            .db
            .query_events(&beekeeper_db::event::EventQuery {
                kinds: Some(vec![KIND_DM_VISIBILITY as i32]),
                pubkey: Some(state.relay_keypair.public_key().to_bytes().to_vec()),
                d_tag: Some(viewer_hex.clone()),
                limit: Some(1),
                ..beekeeper_db::event::EventQuery::for_community(tenant.community())
            })
            .await
            .unwrap_or_default();
        existing
            .first()
            .map(|e| (e.event.created_at.as_secs() + 1).max(now))
            .unwrap_or(now)
    };

    let event = EventBuilder::new(Kind::Custom(KIND_DM_VISIBILITY as u16), "")
        .tags(tags)
        .custom_created_at(nostr::Timestamp::from(ts))
        .sign_with_keys(&state.relay_keypair)
        .map_err(|e| anyhow::anyhow!("failed to sign kind:{KIND_DM_VISIBILITY}: {e}"))?;

    let (stored, was_inserted) = state
        .db
        .replace_parameterized_event(tenant.community(), &event, &viewer_hex, None)
        .await?;
    if was_inserted {
        dispatch_persistent_event(
            tenant,
            state,
            &stored,
            KIND_DM_VISIBILITY,
            &relay_pubkey_hex,
            None,
        )
        .await;
    }

    info!(
        viewer = %viewer_hex,
        hidden_count = hidden.len(),
        "NIP-DV DM visibility snapshot published"
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn publish_nipia_delta(
    tenant: &TenantContext,
    state: &Arc<AppState>,
    kind: u32,
    target_pubkey_hex: &str,
    consent_path: &str,
    actor_pubkey_hex: &str,
    request_event_id: &str,
    content: &str,
    reason: Option<&str>,
    replaced_by: Option<&str>,
) -> anyhow::Result<()> {
    let relay_pubkey_hex = state.relay_keypair.public_key().to_hex();

    let mut tags = vec![
        Tag::parse(["-"]).map_err(|e| anyhow::anyhow!("failed to build '-' tag: {e}"))?,
        Tag::parse(["p", target_pubkey_hex])
            .map_err(|e| anyhow::anyhow!("failed to build p tag: {e}"))?,
        Tag::parse(["consent", consent_path, actor_pubkey_hex])
            .map_err(|e| anyhow::anyhow!("failed to build consent tag: {e}"))?,
        Tag::parse(["e", request_event_id])
            .map_err(|e| anyhow::anyhow!("failed to build e tag: {e}"))?,
    ];

    if let Some(reason) = reason {
        tags.push(
            Tag::parse(["reason", reason])
                .map_err(|e| anyhow::anyhow!("failed to build reason tag: {e}"))?,
        );
    }
    if let Some(replaced_by) = replaced_by {
        tags.push(
            Tag::parse(["replaced-by", replaced_by])
                .map_err(|e| anyhow::anyhow!("failed to build replaced-by tag: {e}"))?,
        );
    }

    let event = EventBuilder::new(Kind::Custom(kind as u16), content)
        .tags(tags)
        .sign_with_keys(&state.relay_keypair)
        .map_err(|e| anyhow::anyhow!("failed to sign kind:{kind}: {e}"))?;

    let (stored, was_inserted) = state
        .db
        .insert_event(tenant.community(), &event, None)
        .await?;
    if !was_inserted {
        return Ok(());
    }

    dispatch_persistent_event(tenant, state, &stored, kind, &relay_pubkey_hex, None).await;

    info!(
        target = %target_pubkey_hex,
        relay = %relay_pubkey_hex,
        kind,
        consent = %consent_path,
        "NIP-IA delta event published"
    );
    Ok(())
}

/// Publish a kind:8002 archived-identity delta event (NIP-IA).
#[allow(clippy::too_many_arguments)]
pub async fn publish_nipia_archived(
    tenant: &TenantContext,
    state: &Arc<AppState>,
    target_pubkey_hex: &str,
    consent_path: &str,
    actor_pubkey_hex: &str,
    request_event_id: &str,
    content: &str,
    reason: Option<&str>,
    replaced_by: Option<&str>,
) -> anyhow::Result<()> {
    publish_nipia_delta(
        tenant,
        state,
        KIND_IA_ARCHIVED,
        target_pubkey_hex,
        consent_path,
        actor_pubkey_hex,
        request_event_id,
        content,
        reason,
        replaced_by,
    )
    .await
}

/// Publish a kind:8003 unarchived-identity delta event (NIP-IA).
#[allow(clippy::too_many_arguments)]
pub async fn publish_nipia_unarchived(
    tenant: &TenantContext,
    state: &Arc<AppState>,
    target_pubkey_hex: &str,
    consent_path: &str,
    actor_pubkey_hex: &str,
    request_event_id: &str,
    content: &str,
    reason: Option<&str>,
) -> anyhow::Result<()> {
    publish_nipia_delta(
        tenant,
        state,
        KIND_IA_UNARCHIVED,
        target_pubkey_hex,
        consent_path,
        actor_pubkey_hex,
        request_event_id,
        content,
        reason,
        None,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_members_snapshot_keeps_members_past_one_thousand() {
        let channel_id = Uuid::new_v4();
        let members: Vec<MemberRecord> = (0_u16..1_501)
            .map(|index| MemberRecord {
                channel_id,
                pubkey: vec![(index >> 8) as u8, index as u8],
                role: if index == 1_500 { "owner" } else { "member" }.to_string(),
                joined_at: chrono::Utc::now(),
                invited_by: None,
                removed_at: None,
            })
            .collect();

        let tags = group_members_tags(&channel_id.to_string(), &members).expect("build tags");
        assert_eq!(tags.len(), 1_502, "d tag plus every member p tag");

        let late_pubkey = hex::encode(&members[1_500].pubkey);
        assert!(tags.iter().any(|tag| {
            let fields = tag.as_slice();
            fields.len() == 4
                && fields[0] == "p"
                && fields[1] == late_pubkey
                && fields[3] == "owner"
        }));
    }

    /// A genesis is refused by *both* deletion paths when it is deleted on its
    /// own, and refused on the kind alone — no authorship, membership, or
    /// moderator role can reach past it. (Deleting a *whole session* is a
    /// different act, authorized by `authorize_coding_session_deletion`
    /// before this is consulted; the moderator path deletes one event at a
    /// time and so cannot express it at all.)
    #[test]
    fn coding_session_genesis_cannot_be_deleted() {
        let refusal = refuse_permanent_identity_deletion(KIND_CODING_SESSION_GENESIS)
            .expect_err("deleting a genesis must be refused");
        let message = refusal.to_string();
        assert!(
            message.contains("cannot be deleted"),
            "the refusal must say plainly that the deletion did not happen, got {message:?}"
        );
        assert!(
            message.contains("closure"),
            "the refusal must point at the supported way to end a session, got {message:?}"
        );
    }

    /// Deleting a closure revision on its own would make the fold fall back to
    /// older state without an attributable counter-revision, so both NIP-09
    /// and moderator deletion paths refuse it by kind. A closure deleted
    /// *with* its genesis is a whole-session delete and never reaches here.
    #[test]
    fn coding_session_closure_cannot_be_deleted() {
        let refusal = refuse_permanent_identity_deletion(KIND_CODING_SESSION_CLOSURE)
            .expect_err("deleting a closure revision must be refused");
        let message = refusal.to_string();
        assert!(message.contains("cannot be deleted"), "got {message:?}");
        assert!(
            message.contains("another closure revision"),
            "got {message:?}"
        );
    }

    /// The gate is narrow on purpose. Commands and provider-authored receipts,
    /// metadata, and transcript remain ordinary content; only the genesis
    /// identity anchor and append-only closure state are permanent.
    #[test]
    fn ordinary_coding_session_events_stay_deletable() {
        for kind in [
            beekeeper_core::kind::KIND_CODING_SESSION_COMMAND,
            beekeeper_core::kind::KIND_CODING_SESSION_LIFECYCLE_COMMAND,
            beekeeper_core::kind::KIND_CODING_SESSION_PROVIDER_CATALOG,
            beekeeper_core::kind::KIND_CODING_SESSION_METADATA,
            beekeeper_core::kind::KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
            beekeeper_core::kind::KIND_CODING_SESSION_TRANSCRIPT,
            beekeeper_core::kind::KIND_CODING_SESSION_GENERATED_TITLE,
            beekeeper_core::kind::KIND_STREAM_MESSAGE,
        ] {
            assert!(
                refuse_permanent_identity_deletion(kind).is_ok(),
                "kind {kind} must keep its ordinary deletion rules"
            );
        }
    }

    /// A whole-session delete reaches the session's generated titles (44252)
    /// by their `d` tag, exactly as it reaches its 44229 names — and only
    /// this session's, in this channel.
    #[test]
    fn a_session_delete_reaches_its_own_generated_titles_by_d_tag() {
        let channel_id = Uuid::new_v4();
        let session_ref = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
        let title = |channel: Uuid, d: &str| {
            nostr::EventBuilder::new(
                nostr::Kind::Custom(
                    beekeeper_core::kind::KIND_CODING_SESSION_GENERATED_TITLE as u16,
                ),
                "{}",
            )
            .tags([
                nostr::Tag::parse(["h", &channel.to_string()]).expect("h"),
                nostr::Tag::parse(["d", d]).expect("d"),
                nostr::Tag::parse(["cstl-v", "cstl1-1"]).expect("v"),
            ])
            .sign_with_keys(&nostr::Keys::generate())
            .expect("sign")
        };
        let session = AuthorizedSessionDeletion {
            session_ref: session_ref.to_owned(),
            channel_id,
            genesis_id: vec![0; 32],
            execution_targets: std::collections::HashSet::new(),
        };
        assert!(coding_session_scoped_kind(
            beekeeper_core::kind::KIND_CODING_SESSION_GENERATED_TITLE
        ));
        assert!(session_deletion_admits(
            &title(channel_id, session_ref),
            &session
        ));
        assert!(!session_deletion_admits(
            &title(channel_id, "e2c4a6b8-1d3f-4e5a-8b7c-9d0e1f2a3b4c"),
            &session
        ));
        assert!(!session_deletion_admits(
            &title(Uuid::new_v4(), session_ref),
            &session
        ));
    }

    #[test]
    fn delete_tombstone_omits_absent_moderation_metadata() {
        let content =
            delete_tombstone_content("actor".to_string(), "target".to_string(), None, None, None);

        assert_eq!(content["type"], "message_deleted");
        assert_eq!(content["actor"], "actor");
        assert_eq!(content["target_event_id"], "target");
        assert!(content.get("action_id").is_none());
        assert!(content.get("reason_code").is_none());
        assert!(content.get("public_reason").is_none());
    }

    #[test]
    fn delete_tombstone_carries_optional_moderation_metadata() {
        let content = delete_tombstone_content(
            "actor".to_string(),
            "target".to_string(),
            Some("550e8400-e29b-41d4-a716-446655440000".to_string()),
            Some("spam".to_string()),
            Some("Removed for spam.".to_string()),
        );

        assert_eq!(content["type"], "message_deleted");
        assert_eq!(content["actor"], "actor");
        assert_eq!(content["target_event_id"], "target");
        assert_eq!(content["action_id"], "550e8400-e29b-41d4-a716-446655440000");
        assert_eq!(content["reason_code"], "spam");
        assert_eq!(content["public_reason"], "Removed for spam.");
        assert!(!content.to_string().contains("reporter"));
    }

    #[test]
    fn author_self_delete_with_moderation_metadata_skips_self_delete_path() {
        let keys = nostr::Keys::generate();
        let actor = keys.public_key().to_bytes();
        let event = EventBuilder::new(Kind::Custom(9005), "")
            .tags([Tag::parse(["public_reason", "Removed for spam."]).unwrap()])
            .sign_with_keys(&keys)
            .expect("sign");

        assert!(!author_delete_can_use_self_delete_path(
            &actor, &actor, &event
        ));
    }
}

#[cfg(test)]
#[path = "side_effects_project_owner_delete_tests.rs"]
mod project_owner_delete_tests;

#[cfg(test)]
#[path = "side_effects_session_checkpoint_delete_tests.rs"]
mod session_checkpoint_delete_tests;
