//! Command executor — transactional event processing for command kinds.
//!
//! Command kinds (41010–41012, 30620, 46020, 46030–46031) are processed
//! transactionally: validate → begin tx → insert event → execute mutations → commit.
//!
//! SECURITY: This module is only reachable AFTER the ingest pipeline has verified:
//! 1. Event signature (verify_event)
//! 2. Timestamp freshness (±15 min)
//! 3. Pubkey/auth identity match
//! 4. Per-kind scope authorization

use std::sync::Arc;

use chrono::Utc;
use nostr::Event;
use tracing::warn;
use uuid::Uuid;

use buzz_core::coding_session_project_action_grant::ProjectActionCapability;
use buzz_core::kind::*;
use buzz_core::tenant::{CommunityId, TenantContext};
use buzz_datastore_tracing::datastore_span;
use buzz_db::workflow::{ApprovalStatus, RunStatus};
use buzz_db::DbError;
use buzz_workflow::executor::TriggerContext;

use crate::state::AppState;
use crate::webhook_secret;
use crate::workflow_ci_result::{authorize_ci_result_binding, CiResultAuthorityError};

use super::ingest::{extract_channel_id, IngestAuth, IngestError, IngestResult};
use super::side_effects::{
    emit_group_discovery_events, emit_membership_notification, emit_system_message,
    publish_dm_visibility_snapshot,
};

/// Route a command-kind event to the appropriate handler.
pub async fn handle_command(
    tenant: &TenantContext,
    state: &Arc<AppState>,
    event: Event,
    auth: IngestAuth,
) -> Result<IngestResult, IngestError> {
    // Ensure the authenticated user exists in the users table (foreign key requirement).
    // The old REST handlers did this via extract_auth_context; command executor must do it explicitly.
    let pubkey_bytes = auth.pubkey().to_bytes().to_vec();
    match state
        .db
        .ensure_user(tenant.community(), &pubkey_bytes)
        .await
    {
        Ok(true) => {
            metrics::counter!(
                "buzz_users_created_total",
                "community" => tenant.host().to_owned()
            )
            .increment(1);
        }
        Ok(false) => {}
        Err(e) => {
            tracing::warn!("command_executor: ensure_user failed: {e}");
        }
    }

    let kind = event.kind.as_u16() as u32;
    match kind {
        KIND_DM_OPEN => handle_dm_open(tenant, state, &event, &auth).await,
        KIND_DM_ADD_MEMBER => handle_dm_add_member(tenant, state, &event, &auth).await,
        KIND_DM_HIDE => handle_dm_hide(tenant, state, &event, &auth).await,
        KIND_WORKFLOW_DEF => handle_workflow_def(tenant, state, &event, &auth).await,
        KIND_WORKFLOW_TRIGGER => handle_workflow_trigger(tenant, state, &event, &auth).await,
        KIND_APPROVAL_GRANT => handle_approval_grant(tenant, state, &event, &auth).await,
        KIND_APPROVAL_DENY => handle_approval_deny(tenant, state, &event, &auth).await,
        buzz_core::kind::KIND_WORKFLOW_AUTORUN_REVOKE => {
            handle_autorun_revoke(tenant, state, &event, &auth).await
        }
        KIND_HOST_STEP_CLAIM => {
            super::host_steps::handle_host_step_claim(tenant, state, &event, &auth).await
        }
        KIND_HOST_STEP_RESULT => {
            super::host_steps::handle_host_step_result(tenant, state, &event, &auth).await
        }
        _ => Err(IngestError::Rejected(format!(
            "unknown command kind: {kind}"
        ))),
    }
}

/// Result of persisting a command event: either a duplicate (already processed)
/// or an open transaction that the handler must commit after executing mutations.
pub(super) enum PersistResult {
    /// Event was already processed — return idempotent success.
    Duplicate,
    /// Event inserted — transaction is open, handler must commit after mutations.
    Inserted(sqlx::Transaction<'static, sqlx::Postgres>),
}

/// Persist a command event inside a transaction. Returns the OPEN transaction
/// as an idempotency guard — if the event was already stored, `Duplicate` is
/// returned and the handler skips execution.
///
/// If the event is a duplicate (ON CONFLICT DO NOTHING), the transaction is
/// rolled back and `PersistResult::Duplicate` is returned — no mutations needed.
///
/// NOTE: Domain mutations (open_dm, upsert_workflow, etc.) execute on the
/// connection pool, NOT inside this transaction. The pattern is idempotent but
/// not strictly atomic: if a mutation succeeds but commit fails, the mutation
/// persists without the event record. On retry, the event INSERT succeeds
/// (no conflict), and the mutation re-executes — which is safe for idempotent
/// operations (open_dm, hide_dm, update_approval, upsert_workflow).
#[datastore_span(name = "persist_command_event", system = "postgresql")]
pub(super) async fn persist_command_event(
    db: &buzz_db::Db,
    tenant: &TenantContext,
    event: &Event,
    channel_id_override: Option<Uuid>,
) -> Result<PersistResult, IngestError> {
    let channel_id = channel_id_override.or_else(|| extract_channel_id(event));

    let mut tx = db
        .begin_transaction()
        .await
        .map_err(|e| IngestError::Internal(format!("error: begin transaction: {e}")))?;
    buzz_deletion::store(db)
        .guard_transaction(&mut tx, tenant.community())
        .await
        .map_err(|error| {
            IngestError::Rejected(format!("restricted: community writes are fenced: {error}"))
        })?;

    // INSERT with ON CONFLICT DO NOTHING — idempotency guard.
    let id_bytes = event.id.as_bytes();
    let pubkey_bytes = event.pubkey.to_bytes();
    let sig_bytes = event.sig.serialize();
    let tags_json = serde_json::to_value(&event.tags)
        .map_err(|e| IngestError::Internal(format!("error: serialize tags: {e}")))?;
    let kind_i32 = event.kind.as_u16() as i32;
    let created_at_secs = event.created_at.as_secs() as i64;
    let created_at = chrono::DateTime::from_timestamp(created_at_secs, 0).ok_or_else(|| {
        IngestError::Rejected(format!("invalid: bad timestamp {created_at_secs}"))
    })?;
    let received_at = chrono::Utc::now();

    // Extract d_tag for parameterized replaceable kinds (NIP-33).
    let d_tag = buzz_db::event::extract_d_tag(event);
    if let Some(ref d_tag) = d_tag {
        if d_tag.len() > buzz_db::event::D_TAG_MAX_LEN {
            return Err(IngestError::Rejected(format!(
                "invalid: d tag too long ({} bytes, max {})",
                d_tag.len(),
                buzz_db::event::D_TAG_MAX_LEN,
            )));
        }

        // Command kinds normally use plain insert semantics, but workflow
        // definitions are NIP-33 events. Serialize writers for the same
        // coordinate and reject stale writes before executing the domain
        // mutation, otherwise old updates can overwrite newer workflow state.
        let lock_key = {
            let mut h: u64 = 0xcbf29ce484222325;
            for b in tenant.community().as_uuid().as_bytes() {
                h ^= *b as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
            for b in kind_i32.to_le_bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
            for b in pubkey_bytes.as_slice() {
                h ^= *b as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
            for b in d_tag.as_bytes() {
                h ^= *b as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
            h as i64
        };

        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(lock_key)
            .execute(tx.as_mut())
            .await
            .map_err(|e| IngestError::Internal(format!("error: lock event coordinate: {e}")))?;

        let existing: Option<(chrono::DateTime<chrono::Utc>, Vec<u8>)> = sqlx::query_as(
            "SELECT created_at, id FROM events \
             WHERE community_id = $1 AND kind = $2 AND pubkey = $3 AND d_tag = $4 AND deleted_at IS NULL \
             ORDER BY created_at DESC, id ASC LIMIT 1",
        )
        .bind(tenant.community().as_uuid())
        .bind(kind_i32)
        .bind(pubkey_bytes.as_slice())
        .bind(d_tag)
        .fetch_optional(tx.as_mut())
        .await
        .map_err(|e| IngestError::Internal(format!("error: query event coordinate: {e}")))?;

        let incoming_id = event.id.as_bytes().as_slice();
        if existing
            .as_ref()
            .is_some_and(|(_, existing_id)| existing_id.as_slice() == incoming_id)
        {
            return Ok(PersistResult::Duplicate);
        }

        let expected_revision = extract_tag(event, "expected-revision");
        validate_workflow_revision(
            kind_i32,
            expected_revision.as_deref(),
            existing.as_ref().map(|(_, id)| id.as_slice()),
        )?;
        if let Some((existing_ts, existing_id)) = existing {
            let dominated = created_at < existing_ts
                || (created_at == existing_ts && incoming_id >= existing_id.as_slice());
            if dominated {
                if kind_i32 == KIND_WORKFLOW_DEF as i32 && expected_revision.is_some() {
                    return Err(IngestError::Rejected(
                        "conflict: workflow update was superseded; refresh and try again".into(),
                    ));
                }
                return Ok(PersistResult::Duplicate);
            }

            sqlx::query(
                "UPDATE events SET deleted_at = NOW() \
                 WHERE community_id = $1 AND kind = $2 AND pubkey = $3 AND d_tag = $4 AND deleted_at IS NULL",
            )
            .bind(tenant.community().as_uuid())
            .bind(kind_i32)
            .bind(pubkey_bytes.as_slice())
            .bind(d_tag)
            .execute(tx.as_mut())
            .await
            .map_err(|e| IngestError::Internal(format!("error: replace old event: {e}")))?;
        }
    }

    let result = sqlx::query(
        r#"
        INSERT INTO events (community_id, id, pubkey, created_at, kind, tags, content, sig, received_at, channel_id, d_tag)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
        ON CONFLICT DO NOTHING
        "#,
    )
    .bind(tenant.community().as_uuid())
    .bind(id_bytes.as_slice())
    .bind(pubkey_bytes.as_slice())
    .bind(created_at)
    .bind(kind_i32)
    .bind(&tags_json)
    .bind(&event.content)
    .bind(sig_bytes.as_slice())
    .bind(received_at)
    .bind(channel_id)
    .bind(d_tag.as_deref())
    .execute(tx.as_mut())
    .await
    .map_err(|e| IngestError::Internal(format!("error: insert event: {e}")))?;

    if result.rows_affected() == 0 {
        // Duplicate — rollback (implicit on drop) and signal idempotent success.
        Ok(PersistResult::Duplicate)
    } else {
        Ok(PersistResult::Inserted(tx))
    }
}

fn validate_workflow_revision(
    kind: i32,
    expected_revision: Option<&str>,
    existing_id: Option<&[u8]>,
) -> Result<(), IngestError> {
    if kind != KIND_WORKFLOW_DEF as i32 {
        return Ok(());
    }

    let expected_id = expected_revision
        .map(|expected| {
            let id = hex::decode(expected).map_err(|_| {
                IngestError::Rejected("invalid: bad expected workflow revision".into())
            })?;
            if id.len() != 32 {
                return Err(IngestError::Rejected(
                    "invalid: bad expected workflow revision".into(),
                ));
            }
            Ok(id)
        })
        .transpose()?;

    match (expected_id.as_deref(), existing_id) {
        (None, _) => Ok(()),
        (Some(_), None) => Err(IngestError::Rejected(
            "conflict: workflow revision does not exist".into(),
        )),
        (Some(expected), Some(existing)) if expected != existing => Err(IngestError::Rejected(
            "conflict: workflow changed since it was loaded".into(),
        )),
        (Some(_), Some(_)) => Ok(()),
    }
}

/// Extract all `p` tag values (hex pubkeys) from an event.
fn extract_p_tags(event: &Event) -> Vec<String> {
    event
        .tags
        .iter()
        .filter_map(|t| {
            if t.kind().to_string() == "p" {
                t.content().map(|s| s.to_string())
            } else {
                None
            }
        })
        .collect()
}

/// Extract the first `h` tag value (channel UUID) from an event.
fn extract_h_tag(event: &Event) -> Option<String> {
    event.tags.iter().find_map(|t| {
        if t.kind().to_string() == "h" {
            t.content().map(|s| s.to_string())
        } else {
            None
        }
    })
}

/// Extract the first `d` tag value from an event.
fn extract_d_tag(event: &Event) -> Option<String> {
    event.tags.iter().find_map(|t| {
        if t.kind().to_string() == "d" {
            t.content().map(|s| s.to_string())
        } else {
            None
        }
    })
}

/// Extract the first `e` tag value from an event.
fn extract_e_tag(event: &Event) -> Option<String> {
    event.tags.iter().find_map(|t| {
        if t.kind().to_string() == "e" {
            t.content().map(|s| s.to_string())
        } else {
            None
        }
    })
}

/// Extract a tag value by name.
fn extract_tag(event: &Event, tag_name: &str) -> Option<String> {
    event.tags.iter().find_map(|t| {
        if t.kind().to_string() == tag_name {
            t.content().map(|s| s.to_string())
        } else {
            None
        }
    })
}

/// Decode a hex pubkey string to 32 bytes.
fn decode_pubkey(hex_str: &str) -> Result<Vec<u8>, IngestError> {
    let bytes = hex::decode(hex_str)
        .map_err(|_| IngestError::Rejected(format!("invalid: bad pubkey hex: {hex_str}")))?;
    if bytes.len() != 32 {
        return Err(IngestError::Rejected(format!(
            "invalid: pubkey must be 32 bytes: {hex_str}"
        )));
    }
    Ok(bytes)
}

async fn handle_dm_open(
    tenant: &TenantContext,
    state: &Arc<AppState>,
    event: &Event,
    auth: &IngestAuth,
) -> Result<IngestResult, IngestError> {
    let self_bytes = auth.pubkey().to_bytes().to_vec();
    let self_hex = hex::encode(&self_bytes);

    // 1. Extract participant pubkeys from `p` tags
    let p_tags = extract_p_tags(event);

    // 2. Validate: at least 1 other participant, max 8 others (9 total)
    if p_tags.is_empty() {
        return Err(IngestError::Rejected(
            "invalid: pubkeys must contain at least 1 other participant".into(),
        ));
    }
    if p_tags.len() > 8 {
        return Err(IngestError::Rejected(
            "invalid: pubkeys may contain at most 8 other participants (9 total)".into(),
        ));
    }

    // Decode all provided pubkeys
    let mut other_bytes: Vec<Vec<u8>> = Vec::with_capacity(p_tags.len());
    for hex_str in &p_tags {
        other_bytes.push(decode_pubkey(hex_str)?);
    }

    // 3. Build full participant set (self + others, deduplicated)
    let mut all_bytes: Vec<Vec<u8>> = vec![self_bytes.clone()];
    for ob in &other_bytes {
        if !all_bytes.iter().any(|b| b == ob) {
            all_bytes.push(ob.clone());
        }
    }

    // Persist the command event (idempotency) — returns open transaction
    let tx = match persist_command_event(&state.db, tenant, event, None).await? {
        PersistResult::Duplicate => {
            return Ok(IngestResult {
                event_id: event.id.to_hex(),
                accepted: true,
                message: "duplicate: already processed".into(),
            });
        }
        PersistResult::Inserted(tx) => tx,
    };

    // 4. Execute: open_dm
    let all_refs: Vec<&[u8]> = all_bytes.iter().map(|b| b.as_slice()).collect();
    let (channel, was_created) = state
        .db
        .open_dm(tenant.community(), &all_refs, &self_bytes)
        .await
        .map_err(|e| IngestError::Internal(format!("error: db open_dm: {e}")))?;

    // Commit: event + mutation succeeded atomically.
    tx.commit()
        .await
        .map_err(|e| IngestError::Internal(format!("error: commit transaction: {e}")))?;

    // 5. Side effects if newly created (post-commit, best-effort)
    if was_created {
        metrics::counter!(
            "buzz_channels_created_total",
            "community" => tenant.host().to_owned(),
            "type" => "dm"
        )
        .increment(1);

        // Invalidate caches for all participants
        for pk in &all_bytes {
            state.invalidate_membership(tenant, channel.id, pk);
        }

        let participant_hexes: Vec<String> = all_bytes.iter().map(hex::encode).collect();
        if let Err(e) = emit_system_message(
            tenant,
            state,
            channel.id,
            serde_json::json!({
                "type": "dm_created",
                "actor": self_hex,
                "participants": participant_hexes,
            }),
        )
        .await
        {
            warn!("DM open: system message failed: {e}");
        }

        if let Err(e) = emit_group_discovery_events(tenant, state, channel.id).await {
            warn!(channel = %channel.id, "DM open: discovery emission failed: {e}");
        }

        for participant in &all_bytes {
            if let Err(e) = emit_membership_notification(
                tenant,
                state,
                channel.id,
                participant,
                &self_bytes,
                KIND_MEMBER_ADDED_NOTIFICATION,
            )
            .await
            {
                warn!("DM open: membership notification failed: {e}");
            }
        }
    } else {
        // Re-open of an existing DM cleared the caller's hidden_at; refresh
        // their NIP-DV snapshot so the DM reappears in the sidebar.
        if let Err(e) = publish_dm_visibility_snapshot(tenant, state, &self_bytes).await {
            warn!("DM re-open: visibility snapshot failed: {e}");
        }
    }

    // 6. Return response
    Ok(IngestResult {
        event_id: event.id.to_hex(),
        accepted: true,
        message: format!(
            "response:{}",
            serde_json::json!({
                "channel_id": channel.id.to_string(),
                "created": was_created,
            })
        ),
    })
}

async fn handle_dm_add_member(
    tenant: &TenantContext,
    state: &Arc<AppState>,
    event: &Event,
    auth: &IngestAuth,
) -> Result<IngestResult, IngestError> {
    let self_bytes = auth.pubkey().to_bytes().to_vec();

    // 1. Extract target channel from `h` tag, new member pubkeys from `p` tags
    let channel_id_str = extract_h_tag(event)
        .ok_or_else(|| IngestError::Rejected("invalid: missing h tag (channel_id)".into()))?;
    let channel_id = Uuid::parse_str(&channel_id_str)
        .map_err(|_| IngestError::Rejected("invalid: bad channel_id format".into()))?;

    let p_tags = extract_p_tags(event);
    if p_tags.is_empty() {
        return Err(IngestError::Rejected(
            "invalid: must specify at least 1 new participant in p tags".into(),
        ));
    }

    // 2. Validate caller is member of existing DM
    let is_member = state
        .is_member_cached(tenant.community(), channel_id, &self_bytes)
        .await
        .map_err(|e| IngestError::Internal(format!("error: membership check: {e}")))?;
    if !is_member {
        return Err(IngestError::Rejected(
            "forbidden: not a member of this DM".into(),
        ));
    }

    // 3. Validate channel is type "dm"
    let existing_channel = state
        .db
        .get_channel(tenant.community(), channel_id)
        .await
        .map_err(|_| IngestError::Rejected("invalid: DM not found".into()))?;
    if existing_channel.channel_type != "dm" {
        return Err(IngestError::Rejected("invalid: channel is not a DM".into()));
    }

    // 4. Get existing members, merge with new
    let existing_members = state
        .db
        .get_members(tenant.community(), channel_id)
        .await
        .map_err(|e| IngestError::Internal(format!("error: get members: {e}")))?;

    let mut all_bytes: Vec<Vec<u8>> = existing_members.into_iter().map(|m| m.pubkey).collect();

    // Decode and merge new pubkeys
    for hex_str in &p_tags {
        let bytes = decode_pubkey(hex_str)?;
        if !all_bytes.iter().any(|b| b == &bytes) {
            all_bytes.push(bytes);
        }
    }

    // 5. Enforce max 9 participants
    if all_bytes.len() > 9 {
        return Err(IngestError::Rejected(
            "invalid: DM supports at most 9 participants".into(),
        ));
    }

    // Persist the command event — returns open transaction
    let tx = match persist_command_event(&state.db, tenant, event, None).await? {
        PersistResult::Duplicate => {
            return Ok(IngestResult {
                event_id: event.id.to_hex(),
                accepted: true,
                message: "duplicate: already processed".into(),
            });
        }
        PersistResult::Inserted(tx) => tx,
    };

    // 6. Execute: open_dm with expanded set (creates NEW DM — DM sets are immutable)
    let all_refs: Vec<&[u8]> = all_bytes.iter().map(|b| b.as_slice()).collect();
    let (new_channel, was_created) = state
        .db
        .open_dm(tenant.community(), &all_refs, &self_bytes)
        .await
        .map_err(|e| IngestError::Internal(format!("error: db open_dm: {e}")))?;

    // Commit: event + mutation succeeded atomically.
    tx.commit()
        .await
        .map_err(|e| IngestError::Internal(format!("error: commit transaction: {e}")))?;

    // 7. Cache invalidation + notifications for new DM (post-commit, best-effort)
    if was_created {
        metrics::counter!(
            "buzz_channels_created_total",
            "community" => tenant.host().to_owned(),
            "type" => "dm"
        )
        .increment(1);

        for pk in &all_bytes {
            state.invalidate_membership(tenant, new_channel.id, pk);
        }

        if let Err(e) = emit_group_discovery_events(tenant, state, new_channel.id).await {
            warn!(channel = %new_channel.id, "DM add_member: discovery emission failed: {e}");
        }

        for participant_bytes in &all_bytes {
            if let Err(e) = emit_membership_notification(
                tenant,
                state,
                new_channel.id,
                participant_bytes,
                &self_bytes,
                KIND_MEMBER_ADDED_NOTIFICATION,
            )
            .await
            {
                warn!("DM add_member: membership notification failed: {e}");
            }
        }
    }

    // 8. Return response
    Ok(IngestResult {
        event_id: event.id.to_hex(),
        accepted: true,
        message: format!(
            "response:{}",
            serde_json::json!({
                "channel_id": new_channel.id.to_string(),
            })
        ),
    })
}

async fn handle_dm_hide(
    tenant: &TenantContext,
    state: &Arc<AppState>,
    event: &Event,
    auth: &IngestAuth,
) -> Result<IngestResult, IngestError> {
    let self_bytes = auth.pubkey().to_bytes().to_vec();

    // 1. Extract channel from `h` tag
    let channel_id_str = extract_h_tag(event)
        .ok_or_else(|| IngestError::Rejected("invalid: missing h tag (channel_id)".into()))?;
    let channel_id = Uuid::parse_str(&channel_id_str)
        .map_err(|_| IngestError::Rejected("invalid: bad channel_id format".into()))?;

    // 2. Validate caller is member of the DM
    let is_member = state
        .is_member_cached(tenant.community(), channel_id, &self_bytes)
        .await
        .map_err(|e| IngestError::Internal(format!("error: membership check: {e}")))?;
    if !is_member {
        return Err(IngestError::Rejected(
            "forbidden: not a member of this DM".into(),
        ));
    }

    // 3. Validate channel is type "dm"
    let channel = state
        .db
        .get_channel(tenant.community(), channel_id)
        .await
        .map_err(|_| IngestError::Rejected("invalid: DM not found".into()))?;
    if channel.channel_type != "dm" {
        return Err(IngestError::Rejected("invalid: channel is not a DM".into()));
    }

    // Persist the command event — returns open transaction
    let tx = match persist_command_event(&state.db, tenant, event, None).await? {
        PersistResult::Duplicate => {
            return Ok(IngestResult {
                event_id: event.id.to_hex(),
                accepted: true,
                message: "duplicate: already processed".into(),
            });
        }
        PersistResult::Inserted(tx) => tx,
    };

    // 4. Execute: hide_dm
    state
        .db
        .hide_dm(tenant.community(), channel_id, &self_bytes)
        .await
        .map_err(|e| IngestError::Internal(format!("error: db hide_dm: {e}")))?;

    // Commit: event + mutation succeeded atomically.
    tx.commit()
        .await
        .map_err(|e| IngestError::Internal(format!("error: commit transaction: {e}")))?;

    // 5. Side effect (post-commit, best-effort): refresh the caller's NIP-DV
    // visibility snapshot so clients can filter this DM out of the sidebar.
    if let Err(e) = publish_dm_visibility_snapshot(tenant, state, &self_bytes).await {
        warn!("DM hide: visibility snapshot failed: {e}");
    }

    // 6. Return response
    Ok(IngestResult {
        event_id: event.id.to_hex(),
        accepted: true,
        message: "{}".into(),
    })
}

async fn handle_workflow_def(
    tenant: &TenantContext,
    state: &Arc<AppState>,
    event: &Event,
    auth: &IngestAuth,
) -> Result<IngestResult, IngestError> {
    let self_bytes = auth.pubkey().to_bytes().to_vec();

    // 1. Extract channel and the canonical workflow UUID from the NIP-33 d-tag.
    let channel_id_str = extract_h_tag(event)
        .ok_or_else(|| IngestError::Rejected("invalid: missing h tag (channel_id)".into()))?;
    let channel_id = Uuid::parse_str(&channel_id_str)
        .map_err(|_| IngestError::Rejected("invalid: bad channel_id format".into()))?;

    let workflow_id_str = extract_d_tag(event)
        .ok_or_else(|| IngestError::Rejected("invalid: missing d tag (workflow_id)".into()))?;
    let workflow_id = Uuid::parse_str(&workflow_id_str)
        .map_err(|_| IngestError::Rejected("invalid: bad workflow_id format".into()))?;

    // 2. Validate caller has channel access (minimum: is a member)
    let is_member = state
        .is_member_cached(tenant.community(), channel_id, &self_bytes)
        .await
        .map_err(|e| IngestError::Internal(format!("error: membership check: {e}")))?;
    if !is_member {
        return Err(IngestError::Rejected(
            "forbidden: not a member of this channel".into(),
        ));
    }

    // 3. Parse YAML from event.content
    let (def, definition_json_str) = buzz_workflow::WorkflowEngine::parse_yaml(&event.content)
        .map_err(|e| IngestError::Rejected(format!("invalid: workflow YAML parse error: {e}")))?;
    let workflow_name = extract_tag(event, "name").unwrap_or_else(|| def.name.clone());

    // CI result authority is bound when the definition is saved, before any
    // workflow row or command event is persisted. Each literal binding must
    // resolve to the exact announced repository, its configured project, and
    // a founder owner. The sink repeats this check at execution time so later
    // revocation cannot be bypassed by an already-stored workflow.
    for step in &def.steps {
        if let buzz_workflow::ActionDef::RecordCiResult {
            project,
            repository,
            ..
        } = &step.action
        {
            authorize_ci_result_binding(
                state,
                tenant.community(),
                &self_bytes,
                project,
                repository,
            )
            .await
            .map_err(|error| match error {
                CiResultAuthorityError::Invalid(detail) => {
                    IngestError::Rejected(format!("invalid: record_ci_result: {detail}"))
                }
                CiResultAuthorityError::Unauthorized(detail) => {
                    IngestError::Rejected(format!("forbidden: record_ci_result: {detail}"))
                }
                CiResultAuthorityError::Storage(detail) => {
                    IngestError::Internal(format!("error: record_ci_result authority: {detail}"))
                }
            })?;
        }
    }

    // SEC-006: definitions with exfiltration-capable actions (call_webhook)
    // require elevated channel authority to save — plain membership is not
    // enough, because the workflow will forward channel content outward with
    // the owner's standing authority. Fail-closed on lookup errors.
    if def.requires_elevated_authority() {
        let role = state
            .db
            .get_member_role(tenant.community(), channel_id, &self_bytes)
            .await
            .map_err(|e| IngestError::Internal(format!("error: role check: {e}")))?;
        if !matches!(role.as_deref(), Some("owner") | Some("admin")) {
            return Err(IngestError::Rejected(
                "forbidden: workflows with call_webhook actions require the owner or admin role"
                    .into(),
            ));
        }
    }

    // A `run_on_host` step runs a command on an operator's machine (spec
    // § 5.6), so saving one takes the same standing authority as saving an
    // exfiltration-capable action: the channel owner or an admin. Fail closed.
    //
    // One additional way in, and only for a definition bound to a project: a
    // live, owner-signed project-action delegation held by that session's
    // active lead seat (ledger 186). Saving the definition is still not
    // permission to *run* it — the synthetic approval gate before the first
    // host step is untouched, and no delegation can release it
    // (`ProjectActionCapability::ApproveHostStep`).
    if def.has_host_steps() {
        let role = state
            .db
            .get_member_role(tenant.community(), channel_id, &self_bytes)
            .await
            .map_err(|e| IngestError::Internal(format!("error: role check: {e}")))?;
        if !matches!(role.as_deref(), Some("owner") | Some("admin")) {
            let delegation = match def.project.as_deref() {
                Some(project) => {
                    crate::handlers::project_action_grant::project_action_delegation_admitted(
                        state,
                        tenant.community(),
                        channel_id,
                        project,
                        &self_bytes,
                        ProjectActionCapability::PublishDefinition,
                    )
                    .await
                }
                None => crate::handlers::project_action_grant::ProjectActionDelegation::Refused(
                    crate::handlers::project_action_grant::ProjectActionDelegationRefusal::NoLiveGrant,
                ),
            };
            if let crate::handlers::project_action_grant::ProjectActionDelegation::Refused(
                refusal,
            ) = delegation
            {
                return Err(IngestError::Rejected(format!(
                    "forbidden: workflows with run_on_host actions require the owner or admin                      role, or a project-action delegation — {}",
                    refusal.detail()
                )));
            }
        }
    }

    // Spec § 5.3: a definition bound to a project (its `project`, mirrored by
    // an optional `a` tag that must agree) is admitted by the kind:30624 rule
    // — the project's creator, a roster Owner, or a founder of one of its
    // endorsed repositories. This is what stops an admin of any channel a
    // host is in from republishing a victim project's public entry under
    // the victim's coordinate and approving it himself (review P2-e).
    let project_ref: Option<String> = match (&def.project, extract_tag(event, "a")) {
        (Some(project), Some(a_tag)) if a_tag != *project => {
            return Err(IngestError::Rejected(format!(
                "invalid: a tag {a_tag:?} must equal the definition's project {project:?}"
            )));
        }
        (None, Some(a_tag)) => {
            return Err(IngestError::Rejected(format!(
                "invalid: a tag {a_tag:?} names a project the definition does not declare"
            )));
        }
        (project, _) => project.clone(),
    };
    if let Some(project) = &project_ref {
        match crate::handlers::pack_source::project_write_admitted(
            state,
            tenant.community(),
            project,
            &hex::encode(&self_bytes),
        )
        .await
        {
            Ok(Ok(_)) => {}
            Ok(Err(refusal)) => {
                // Second way in, and the narrowest one that lets a team
                // finish a goal like "write a verify action and land it": a
                // live delegation of *this* project's actions, signed by a
                // key that writes the project now, held by this session's
                // active lead seat (ledger 186, finding 178(f)). The standing
                // rule above is unchanged; this is consulted only after it
                // has already refused.
                let delegation =
                    crate::handlers::project_action_grant::project_action_delegation_admitted(
                        state,
                        tenant.community(),
                        channel_id,
                        project,
                        &self_bytes,
                        ProjectActionCapability::PublishDefinition,
                    )
                    .await;
                if let crate::handlers::project_action_grant::ProjectActionDelegation::Refused(
                    delegation_refusal,
                ) = delegation
                {
                    return Err(IngestError::Rejected(format!(
                        "forbidden: a project action must be saved by the project creator, a \
                         roster owner or a repository founder ({refusal:?}), or by a key \
                         holding a project-action delegation — {}",
                        delegation_refusal.detail()
                    )));
                }
            }
            Err(()) => {
                return Err(IngestError::Internal(
                    "error: project admission lookup failed".into(),
                ));
            }
        }
    }

    let mut definition_json: serde_json::Value = serde_json::from_str(&definition_json_str)
        .map_err(|e| IngestError::Internal(format!("error: json parse of definition: {e}")))?;

    let existing_workflow = match state.db.get_workflow(tenant.community(), workflow_id).await {
        Ok(workflow) => {
            if workflow.owner_pubkey != self_bytes || workflow.channel_id != Some(channel_id) {
                return Err(IngestError::Rejected(
                    "forbidden: workflow belongs to a different owner or channel".into(),
                ));
            }
            Some(workflow)
        }
        Err(DbError::NotFound(_)) => None,
        Err(e) => {
            return Err(IngestError::Internal(format!(
                "error: db get_workflow: {e}"
            )));
        }
    };

    // Preserve the existing webhook secret across updates. A new secret is
    // returned only when the workflow first gains a webhook trigger.
    let webhook_secret = if matches!(def.trigger, buzz_workflow::TriggerDef::Webhook) {
        let existing_secret = existing_workflow
            .as_ref()
            .and_then(|workflow| webhook_secret::extract_secret(&workflow.definition));
        let secret = existing_secret.unwrap_or_else(webhook_secret::generate_webhook_secret);
        webhook_secret::inject_secret(&mut definition_json, &secret);
        if existing_workflow
            .as_ref()
            .and_then(|workflow| webhook_secret::extract_secret(&workflow.definition))
            .is_none()
        {
            Some(secret)
        } else {
            None
        }
    } else {
        None
    };

    // Compute hash AFTER secret injection, with the one function a host also
    // runs over its own `actions.yml` entry (`buzz_workflow::hash`), so the
    // relay's stored hash and the host's recompiled hash agree byte for byte.
    let definition_json_final = serde_json::to_string(&definition_json)
        .map_err(|e| IngestError::Internal(format!("error: json serialize: {e}")))?;
    let hash = buzz_workflow::hash_definition_value(&definition_json)
        .map_err(|e| IngestError::Internal(format!("error: definition hash: {e}")))?;

    // Persist the command event — returns open transaction
    let tx = match persist_command_event(&state.db, tenant, event, None).await? {
        PersistResult::Duplicate => {
            return Ok(IngestResult {
                event_id: event.id.to_hex(),
                accepted: true,
                message: "duplicate: already processed".into(),
            });
        }
        PersistResult::Inserted(tx) => tx,
    };

    // 4. Execute: upsert by the NIP-33 d-tag UUID. A retry updates the same
    // row instead of creating another enabled workflow that would fan out on
    // every matching event. The workflow's community is the request's
    // server-bound tenant — never re-derived from the (client-supplied) channel
    // id. `community_of_channel(channel_id)` is ambiguous when the same channel
    // UUID exists in two communities and could mint the workflow under the wrong
    // tenant; `tenant.community()` is the authoritative owner. We then verify the
    // channel actually exists *inside that community* (scoped `get_channel`),
    // which fails closed if the client named a channel that belongs to a
    // different community — the same guarantee the `(community_id, channel_id)`
    // composite FK enforces on insert, surfaced here as a clean rejection.
    let community_id = tenant.community();
    state
        .db
        .get_channel(community_id, channel_id)
        .await
        .map_err(|_| IngestError::Rejected("invalid: workflow channel not found".into()))?;

    state
        .db
        .upsert_workflow(
            community_id,
            workflow_id,
            Some(channel_id),
            &self_bytes,
            &workflow_name,
            &definition_json_final,
            &hash,
            project_ref.as_deref(),
        )
        .await
        .map_err(|e| match e {
            DbError::AccessDenied(_) => IngestError::Rejected(
                "forbidden: workflow belongs to a different owner or channel".into(),
            ),
            other => IngestError::Internal(format!("error: db upsert_workflow: {other}")),
        })?;

    // Drop the trigger-path cache entry so the new/updated definition fires on
    // the next matching event instead of after the cache TTL.
    state
        .workflow_engine
        .invalidate_channel_workflows(community_id, channel_id);

    // Commit the event transaction after the idempotent workflow upsert succeeds.
    tx.commit()
        .await
        .map_err(|e| IngestError::Internal(format!("error: commit transaction: {e}")))?;

    // 5. Return response
    let mut resp = serde_json::json!({
        "workflow_id": workflow_id.to_string(),
    });
    if let Some(secret) = webhook_secret {
        resp["webhook_secret"] = serde_json::Value::String(secret);
    }

    Ok(IngestResult {
        event_id: event.id.to_hex(),
        accepted: true,
        message: format!("response:{}", resp),
    })
}

async fn handle_workflow_trigger(
    tenant: &TenantContext,
    state: &Arc<AppState>,
    event: &Event,
    auth: &IngestAuth,
) -> Result<IngestResult, IngestError> {
    let self_bytes = auth.pubkey().to_bytes().to_vec();

    // 1. Extract workflow reference from `d` tag or `e` tag
    let workflow_id_str = extract_d_tag(event)
        .or_else(|| extract_e_tag(event))
        .ok_or_else(|| {
            IngestError::Rejected("invalid: missing workflow reference (d or e tag)".into())
        })?;
    let workflow_id = Uuid::parse_str(&workflow_id_str)
        .map_err(|_| IngestError::Rejected("invalid: bad workflow_id format".into()))?;

    // 2. Validate workflow exists — scoped to the caller's community. The same
    // workflow UUID can exist in another community; a bare-id lookup could load
    // B's workflow and then satisfy the membership check below against B's
    // colliding channel, letting B trigger A's workflow.
    let community_id = tenant.community();
    let workflow = state
        .db
        .get_workflow(community_id, workflow_id)
        .await
        .map_err(|_| IngestError::Rejected("invalid: workflow not found".into()))?;

    // 3. Manual triggers execute with the workflow owner's authority, so only
    // the owner may start them. Channel membership alone is insufficient: a
    // member could otherwise invoke another user's webhook or message actions.
    // A project action (spec § 5.3) may also be started by a project Owner
    // or Collaborator on the kind:39010 roster — the roles that write into
    // the project's contents; Viewers may not.
    if workflow.owner_pubkey != self_bytes {
        let project_role = match &workflow.project_ref {
            Some(project) => state
                .db
                .get_project_role_by_coordinate(community_id, project, &self_bytes)
                .await
                .map_err(|e| IngestError::Internal(format!("error: project role: {e}")))?,
            None => None,
        };
        if !matches!(
            project_role,
            Some(buzz_core::channel::ProjectRole::Owner)
                | Some(buzz_core::channel::ProjectRole::Collaborator)
        ) {
            // Third way in, the same delegation as publication and scoped the
            // same way: a live, owner-signed project-action delegation held by
            // this session's active lead seat may start a manual run of this
            // project's own actions (ledger 186). It is read against the
            // workflow's channel, which is where both the action and the
            // session's authority chain live.
            let delegated = match (&workflow.project_ref, workflow.channel_id) {
                (Some(project), Some(workflow_channel)) => matches!(
                    crate::handlers::project_action_grant::project_action_delegation_admitted(
                        state,
                        community_id,
                        workflow_channel,
                        project,
                        &self_bytes,
                        ProjectActionCapability::TriggerManualRun,
                    )
                    .await,
                    crate::handlers::project_action_grant::ProjectActionDelegation::Admitted { .. }
                ),
                _ => false,
            };
            if !delegated {
                return Err(IngestError::Rejected(
                    "forbidden: not authorized to trigger this workflow — a project action is \
                     started by the workflow's owner, a project Owner or Collaborator, or a key \
                     holding a live project-action delegation for it"
                        .into(),
                ));
            }
        }
    }

    // SEC-006: manual triggers must honor the workflow's lifecycle state and
    // recheck the owner's *current* channel authority before creating a run.
    // Without this, a disabled workflow — including one disabled because its
    // owner was removed from the channel — could still be fired by the owner.
    if !workflow.enabled || workflow.status != buzz_db::workflow::WorkflowStatus::Active {
        return Err(IngestError::Rejected(
            "forbidden: workflow is disabled or inactive".into(),
        ));
    }
    let def: buzz_workflow::WorkflowDef = serde_json::from_value(workflow.definition.clone())
        .map_err(|e| IngestError::Internal(format!("error: corrupt workflow definition: {e}")))?;
    let Some(wf_channel_id) = workflow.channel_id else {
        // No channel scope means no channel authority to verify — fail closed.
        return Err(IngestError::Rejected(
            "forbidden: workflow has no channel scope".into(),
        ));
    };
    state
        .workflow_engine
        .check_owner_authority(community_id, wf_channel_id, &workflow.owner_pubkey, &def)
        .await
        .map_err(|_| {
            IngestError::Rejected("forbidden: not authorized to trigger this workflow".into())
        })?;

    // Spec § 5.1, ledger 178(g): a manual trigger may bind the run to a
    // commit with `checkout`, and a definition whose `run_on_host` step
    // declares `checkout: required` is refused without one — so a
    // verify-style action cannot be answered by whatever the recorded
    // project directory happens to have checked out. The refusal comes
    // before the run exists, and names the flag.
    let content_fields: serde_json::Map<String, serde_json::Value> = if event.content.is_empty() {
        serde_json::Map::new()
    } else {
        match serde_json::from_str(&event.content) {
            Ok(serde_json::Value::Object(map)) => map,
            _ => serde_json::Map::new(),
        }
    };
    let bound_checkout = match content_fields
        .get(buzz_workflow::schema::MANUAL_CHECKOUT_FIELD)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        None => None,
        Some(value) if value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) => {
            Some(value.to_ascii_lowercase())
        }
        Some(_) => {
            return Err(IngestError::Rejected(
                "invalid: checkout must be a full 40-hex commit sha".into(),
            ))
        }
    };
    if let Some(step_id) = def.step_requiring_bound_checkout() {
        if bound_checkout.is_none() {
            return Err(IngestError::Rejected(format!(
                "invalid: step '{step_id}' declares checkout: required, so this run must name the                  commit to test — start it with `bee workflows trigger --checkout <sha>`"
            )));
        }
    }

    // Persist the command event under the workflow channel even though the
    // trigger event itself only carries the workflow UUID. Storing channel
    // triggers as global events leaks workflow IDs to unrelated relay members.
    let tx = match persist_command_event(&state.db, tenant, event, workflow.channel_id).await? {
        PersistResult::Duplicate => {
            return Ok(IngestResult {
                event_id: event.id.to_hex(),
                accepted: true,
                message: "duplicate: already processed".into(),
            });
        }
        PersistResult::Inserted(tx) => tx,
    };

    // 4. Execute: create workflow run
    let mut trigger_ctx = TriggerContext {
        channel_id: workflow
            .channel_id
            .map(|id| id.to_string())
            .unwrap_or_default(),
        author: hex::encode(&self_bytes),
        ..Default::default()
    };
    for (k, v) in content_fields {
        if k == buzz_workflow::schema::MANUAL_CHECKOUT_FIELD {
            // Carried on its own field, normalized; not duplicated as a
            // webhook body value.
            continue;
        }
        let val_str = match v {
            serde_json::Value::String(s) => s,
            other => other.to_string(),
        };
        trigger_ctx.webhook_fields.insert(k, val_str);
    }
    if let Some(checkout) = bound_checkout {
        trigger_ctx.checkout = checkout;
    }
    let trigger_ctx_json = serde_json::to_value(&trigger_ctx).ok();

    let event_id_bytes = event.id.as_bytes().to_vec();
    // Ledger 199: the run is admitted against the hash of the definition this
    // handler read, validated and is about to execute. If the action was
    // republished in between, no run is created and the requester is told so
    // — the checks above (the `checkout: required` refusal, the owner
    // authority check) were made against a definition that is no longer
    // published, so starting anything under them would be a lie.
    let run_id = match state
        .db
        .create_workflow_run(
            community_id,
            workflow_id,
            Some(&event_id_bytes),
            trigger_ctx_json.as_ref(),
            &workflow.definition_hash,
        )
        .await
        .map_err(|e| IngestError::Internal(format!("error: db create_workflow_run: {e}")))?
    {
        buzz_db::workflow::CreateRunOutcome::Created(run_id) => run_id,
        buzz_db::workflow::CreateRunOutcome::DefinitionChanged { current, .. } => {
            return Err(IngestError::Rejected(format!(
                "conflict: the action definition changed while this run was being started \
                 (it is now {}); nothing was started — read it again and trigger it again \
                 (definition_changed)",
                hex::encode(current)
            )));
        }
    };

    // Commit: event + run creation succeeded atomically.
    tx.commit()
        .await
        .map_err(|e| IngestError::Internal(format!("error: commit transaction: {e}")))?;

    // 5. Spawn workflow execution
    let engine = Arc::clone(&state.workflow_engine);
    let db = state.db.clone();
    let def_value = workflow.definition.clone();
    let trigger_ctx_clone = trigger_ctx.clone();
    tokio::spawn(async move {
        let def: buzz_workflow::WorkflowDef = match serde_json::from_value(def_value) {
            Ok(d) => d,
            Err(e) => {
                tracing::error!("workflow_trigger: failed to parse definition: {e}");
                if let Err(db_err) = db
                    .update_workflow_run(
                        community_id,
                        run_id,
                        RunStatus::Failed,
                        0,
                        &serde_json::json!([]),
                        Some(buzz_db::workflow::WorkflowRunFailure {
                            code: "invalid_definition",
                            message: &format!("definition parse error: {e}"),
                        }),
                    )
                    .await
                {
                    tracing::error!("workflow_trigger: failed to mark run as failed: {db_err}");
                }
                return;
            }
        };

        let result = buzz_workflow::executor::execute_from_step(
            &engine,
            community_id,
            run_id,
            &def,
            &trigger_ctx_clone,
            0,
            None,
        )
        .await;
        engine
            .finalize_run(community_id, run_id, result, None)
            .await;
    });

    // 6. Return response
    Ok(IngestResult {
        event_id: event.id.to_hex(),
        accepted: true,
        message: format!(
            "response:{}",
            serde_json::json!({
                "run_id": run_id.to_string(),
            })
        ),
    })
}

/// Enforce the approver_spec field against the requesting pubkey.
///
/// Accepted specs:
/// - `""` or `"any"` — any authenticated user may approve.
/// - 64-char lowercase hex string — only that exact pubkey may approve.
///
/// All other formats are rejected (fail-closed).
/// Admit an approver against a stored `approver_spec`, including the C4
/// `project-owner:<coord>` form: the project's creator or a roster Owner.
async fn approver_admitted(
    state: &Arc<AppState>,
    community: CommunityId,
    approver_spec: &str,
    requester_hex: &str,
    requester_bytes: &[u8],
) -> Result<(), IngestError> {
    if let Some(project) = approver_spec.trim().strip_prefix("project-owner:") {
        let creator = project.split(':').nth(1).unwrap_or_default();
        if creator.eq_ignore_ascii_case(requester_hex) {
            return Ok(());
        }
        let role = state
            .db
            .get_project_role_by_coordinate(community, project, requester_bytes)
            .await
            .map_err(|e| IngestError::Internal(format!("error: project role: {e}")))?;
        if matches!(role, Some(buzz_core::channel::ProjectRole::Owner)) {
            return Ok(());
        }
        return Err(IngestError::Rejected(
            "forbidden: only a project owner may approve this step".into(),
        ));
    }
    check_approver_spec(approver_spec, requester_hex)
}

fn check_approver_spec(approver_spec: &str, requester_hex: &str) -> Result<(), IngestError> {
    let spec = approver_spec.trim();

    // Empty or "any" — anyone may approve
    if spec.is_empty() || spec == "any" {
        return Ok(());
    }

    // Exact pubkey match (64-char hex, case-insensitive)
    if spec.len() == 64 && spec.chars().all(|c| c.is_ascii_hexdigit()) {
        if requester_hex.to_lowercase() == spec.to_lowercase() {
            return Ok(());
        }
        return Err(IngestError::Rejected(
            "forbidden: not the designated approver for this request".into(),
        ));
    }

    // Role-based or unrecognised — fail closed
    Err(IngestError::Rejected(format!(
        "forbidden: approver spec '{}' is not yet supported",
        spec
    )))
}

async fn handle_approval_grant(
    tenant: &TenantContext,
    state: &Arc<AppState>,
    event: &Event,
    auth: &IngestAuth,
) -> Result<IngestResult, IngestError> {
    // A standing grant (spec § 5.4 extension) names its workflow directly
    // instead of an approval token — no run, no kind:46013/46020 needed to
    // manufacture something for it to answer. Its presence is the whole
    // dispatch: a per-run grant never carries a `workflow` tag.
    if let Some(workflow_id_hex) = extract_tag(
        event,
        buzz_core::workflow_autorun::STANDING_GRANT_WORKFLOW_TAG,
    ) {
        return handle_standing_approval_grant(tenant, state, event, auth, &workflow_id_hex).await;
    }

    let self_bytes = auth.pubkey().to_bytes().to_vec();
    let self_hex = hex::encode(&self_bytes);

    // 1. Extract approval reference from `e` tag (references the approval-requested event)
    //    or `d` tag (contains the token hash hex)
    let token_hash_hex = extract_d_tag(event)
        .or_else(|| extract_e_tag(event))
        .ok_or_else(|| {
            IngestError::Rejected("invalid: missing approval reference (d or e tag)".into())
        })?;

    let token_hash = hex::decode(&token_hash_hex)
        .map_err(|_| IngestError::Rejected("invalid: bad approval token hash hex".into()))?;

    // 2. Look up the approval record
    let approval = state
        .db
        .get_approval_by_stored_hash(tenant.community(), &token_hash)
        .await
        .map_err(|_| IngestError::Rejected("invalid: approval not found".into()))?;

    // 3. Validate approval is pending and not expired
    if approval.status != ApprovalStatus::Pending {
        return Err(IngestError::Rejected(format!(
            "invalid: approval already {}",
            approval.status
        )));
    }
    if Utc::now() > approval.expires_at {
        return Err(IngestError::Rejected(
            "invalid: approval token has expired".into(),
        ));
    }

    // 3b. An approval is consent for **one** definition to run a command on
    // the operator's machine (spec § 5.4, ledger 193). If the action has been
    // edited since this request was published, the operator is being asked to
    // release a step they never saw: refuse the grant, and stop the run —
    // resuming it would hand the *current* definition to a host under the
    // earlier consent. A run that carries no binding at all (created before
    // migration 0046) can never establish what was consented to, so it stops
    // too. Either way the remedy is a fresh run and a fresh approval.
    let approved_definition_hash =
        match run_definition_stop_for_approval(state, tenant.community(), &approval)
            .await
            .map_err(|e| IngestError::Internal(format!("error: definition binding: {e}")))?
        {
            ApprovalBinding::Bound(hash) => hash,
            ApprovalBinding::Stop(stop) => {
                stop_run(state, tenant.community(), approval.run_id, &stop).await;
                return Err(IngestError::Rejected(format!(
                    "invalid: {} ({})",
                    stop.message(),
                    stop.code()
                )));
            }
        };

    // 4. Validate caller is authorized approver
    approver_admitted(
        state,
        tenant.community(),
        &approval.approver_spec,
        &self_hex,
        &self_bytes,
    )
    .await?;

    // Persist the command event — returns open transaction
    let tx = match persist_command_event(&state.db, tenant, event, None).await? {
        PersistResult::Duplicate => {
            return Ok(IngestResult {
                event_id: event.id.to_hex(),
                accepted: true,
                message: "duplicate: already processed".into(),
            });
        }
        PersistResult::Inserted(tx) => tx,
    };

    // 5. Execute: update approval status to granted. The content is the C4
    // `{note, scope}` form or a plain pre-C4 note (`decode_approval_grant_content`).
    let grant_content = buzz_core::workflow_autorun::decode_approval_grant_content(&event.content)
        .map_err(|e| IngestError::Rejected(format!("invalid: {e}")))?;
    let note = grant_content.note.as_deref();

    let updated = state
        .db
        .update_approval_by_stored_hash(
            tenant.community(),
            &token_hash,
            ApprovalStatus::Granted,
            Some(&self_bytes),
            note,
        )
        .await
        .map_err(|e| IngestError::Internal(format!("error: db update_approval: {e}")))?;

    if !updated {
        return Err(IngestError::Rejected(
            "invalid: approval already acted on (race)".into(),
        ));
    }

    // Commit: event + approval update succeeded atomically.
    tx.commit()
        .await
        .map_err(|e| IngestError::Internal(format!("error: commit transaction: {e}")))?;

    // 5b. Spec § 5.4: `scope: action` also records an autorun grant bound to
    // the definition hash as stored *now*, and the relay says so as a 46015.
    // An edit changes the hash and re-arms the gate without any revocation.
    if grant_content.scope == buzz_core::workflow_autorun::ApprovalScope::Action {
        // The grant is bound to **the approved run's** hash, which was read
        // once above and cannot move. Reading the workflow row again here and
        // using *its* hash is the race review finding 1 names: a publication
        // landing between the comparison and this insert would record consent
        // for a definition the person never saw, and a fresh run of it would
        // then need no approval at all. The workflow row is still loaded —
        // the kind:46015 needs its channel — but its hash is not consulted.
        #[cfg(test)]
        tests::pause_before_autorun_grant().await;
        let workflow = state
            .db
            .get_workflow(tenant.community(), approval.workflow_id)
            .await
            .map_err(|e| IngestError::Internal(format!("error: db get_workflow: {e}")))?;
        state
            .db
            .create_autorun_grant(
                tenant.community(),
                approval.workflow_id,
                &approved_definition_hash,
                &self_bytes,
                event.id.as_bytes(),
            )
            .await
            .map_err(|e| IngestError::Internal(format!("error: db create_autorun_grant: {e}")))?;
        publish_autorun_changed(
            state,
            tenant.community(),
            &workflow,
            &approved_definition_hash,
            buzz_core::workflow_autorun::AutorunChange::Granted,
            &self_hex,
            &event.id.to_hex(),
        )
        .await;
    }

    // 6. Resume workflow execution (post-commit, async)
    let community_id = tenant.community();
    let run_id = approval.run_id;
    let workflow_id = approval.workflow_id;
    let approval_step_index = approval.step_index.max(0) as usize;
    let engine = Arc::clone(&state.workflow_engine);
    let db = state.db.clone();

    tokio::spawn(async move {
        resume_workflow_after_approval(
            engine,
            db,
            community_id,
            run_id,
            workflow_id,
            approval_step_index,
        )
        .await;
    });

    // 7. Return response
    Ok(IngestResult {
        event_id: event.id.to_hex(),
        accepted: true,
        message: format!(
            "response:{}",
            serde_json::json!({
                "status": "granted",
                "run_id": run_id.to_string(),
            })
        ),
    })
}

/// Grant a **standing** kind:46030 — one naming a workflow and a definition
/// hash directly, with no run and no approval token behind it (spec § 5.4
/// extension; ledger 252's control run 6 finding). It records exactly the
/// autorun grant an in-run `scope: action` grant would, without parking a
/// synthetic run and its kind:46013 first just to manufacture something for
/// the click to answer.
///
/// Authorization mirrors the per-run path's `project-owner:` spec: a project
/// action admits its project's owner or a collaborator with the write role;
/// a workflow with no `project_ref` admits only its own `owner_pubkey`. There
/// is no per-run `approver_spec` to consult, because there is no run.
async fn handle_standing_approval_grant(
    tenant: &TenantContext,
    state: &Arc<AppState>,
    event: &Event,
    auth: &IngestAuth,
    workflow_id_hex: &str,
) -> Result<IngestResult, IngestError> {
    let self_bytes = auth.pubkey().to_bytes().to_vec();
    let self_hex = hex::encode(&self_bytes);

    let workflow_id = Uuid::parse_str(workflow_id_hex)
        .map_err(|_| IngestError::Rejected("invalid: bad workflow id".into()))?;

    let definition_hash_hex = extract_tag(
        event,
        buzz_core::workflow_autorun::STANDING_GRANT_DEFINITION_HASH_TAG,
    )
    .ok_or_else(|| IngestError::Rejected("invalid: missing definitionHash tag".into()))?;
    let definition_hash = hex::decode(&definition_hash_hex)
        .map_err(|_| IngestError::Rejected("invalid: bad definitionHash hex".into()))?;

    // A standing grant only ever means "every later run of this exact
    // definition" — there is no single run for `scope: run` to bind.
    let grant_content = buzz_core::workflow_autorun::decode_approval_grant_content(&event.content)
        .map_err(|e| IngestError::Rejected(format!("invalid: {e}")))?;
    if grant_content.scope != buzz_core::workflow_autorun::ApprovalScope::Action {
        return Err(IngestError::Rejected(
            "invalid: a standing grant must carry scope: action".into(),
        ));
    }

    let workflow = match state.db.get_workflow(tenant.community(), workflow_id).await {
        Ok(workflow) => workflow,
        Err(DbError::NotFound(_)) => {
            return Err(IngestError::Rejected("invalid: workflow not found".into()));
        }
        Err(e) => {
            return Err(IngestError::Internal(format!(
                "error: db get_workflow: {e}"
            )));
        }
    };

    if workflow.definition_hash != definition_hash {
        return Err(IngestError::Rejected(
            "invalid: definitionHash does not match the workflow's published definition".into(),
        ));
    }

    let approver_spec = match &workflow.project_ref {
        Some(project) => format!("project-owner:{project}"),
        None => hex::encode(&workflow.owner_pubkey),
    };
    approver_admitted(
        state,
        tenant.community(),
        &approver_spec,
        &self_hex,
        &self_bytes,
    )
    .await?;

    let tx = match persist_command_event(&state.db, tenant, event, None).await? {
        PersistResult::Duplicate => {
            return Ok(IngestResult {
                event_id: event.id.to_hex(),
                accepted: true,
                message: "duplicate: already processed".into(),
            });
        }
        PersistResult::Inserted(tx) => tx,
    };

    state
        .db
        .create_autorun_grant(
            tenant.community(),
            workflow_id,
            &definition_hash,
            &self_bytes,
            event.id.as_bytes(),
        )
        .await
        .map_err(|e| IngestError::Internal(format!("error: db create_autorun_grant: {e}")))?;

    tx.commit()
        .await
        .map_err(|e| IngestError::Internal(format!("error: commit transaction: {e}")))?;

    publish_autorun_changed(
        state,
        tenant.community(),
        &workflow,
        &definition_hash,
        buzz_core::workflow_autorun::AutorunChange::Granted,
        &self_hex,
        &event.id.to_hex(),
    )
    .await;

    Ok(IngestResult {
        event_id: event.id.to_hex(),
        accepted: true,
        message: format!(
            "response:{}",
            serde_json::json!({
                "status": "granted",
                "standing": true,
            })
        ),
    })
}

/// Publish the relay-signed kind:46015 that records an autorun change. A
/// failure is logged, not fatal: the grant row is the fact, the event is its
/// announcement.
/// `definition_hash` is passed explicitly rather than read off `workflow`:
/// a grant announces the hash it was actually written for — the approved
/// run's — which a concurrent republication must not be able to change.
async fn publish_autorun_changed(
    state: &Arc<AppState>,
    community: CommunityId,
    workflow: &buzz_db::workflow::WorkflowRecord,
    definition_hash: &[u8],
    change: buzz_core::workflow_autorun::AutorunChange,
    by_hex: &str,
    source_event_id: &str,
) {
    let Some(channel_id) = workflow.channel_id else {
        return;
    };
    let changed = buzz_core::workflow_autorun::AutorunChanged {
        schema: buzz_core::workflow_autorun::AUTORUN_SCHEMA.into(),
        workflow_id: workflow.id.to_string(),
        definition_hash: hex::encode(definition_hash),
        change,
        by: by_hex.to_ascii_lowercase(),
        source_event_id: source_event_id.to_owned(),
        channel_id: channel_id.to_string(),
    };
    match buzz_core::workflow_autorun::build_autorun_changed(&changed) {
        Ok((tags, content)) => {
            if let Err(error) = crate::workflow_sink::publish_relay_event(
                state,
                community,
                buzz_core::kind::KIND_WORKFLOW_AUTORUN_CHANGED,
                tags,
                content,
                channel_id,
            )
            .await
            {
                tracing::error!(workflow_id = %workflow.id, "autorun: could not publish kind:46015: {error}");
            }
        }
        Err(error) => {
            tracing::error!(workflow_id = %workflow.id, "autorun: could not build kind:46015: {error}")
        }
    }
}

/// Kind 46032: revoke every autorun grant for a workflow. Admitted from the
/// workflow's owner, and for a project action from anyone the kind:30624
/// rule admits (creator, roster owner, repository founder).
async fn handle_autorun_revoke(
    tenant: &TenantContext,
    state: &Arc<AppState>,
    event: &Event,
    auth: &IngestAuth,
) -> Result<IngestResult, IngestError> {
    let self_bytes = auth.pubkey().to_bytes().to_vec();
    let self_hex = hex::encode(&self_bytes);
    let revoke = buzz_core::workflow_autorun::decode_autorun_revoke(event)
        .map_err(|e| IngestError::Rejected(format!("invalid: {e}")))?;
    let workflow_id = Uuid::parse_str(&revoke.workflow_id)
        .map_err(|_| IngestError::Rejected("invalid: bad workflow_id format".into()))?;
    let workflow = state
        .db
        .get_workflow(tenant.community(), workflow_id)
        .await
        .map_err(|_| IngestError::Rejected("invalid: workflow not found".into()))?;
    if workflow.channel_id.map(|id| id.to_string()).as_deref() != Some(revoke.channel_id.as_str()) {
        return Err(IngestError::Rejected(
            "invalid: h tag must name the workflow's channel".into(),
        ));
    }
    let mut admitted = workflow.owner_pubkey == self_bytes;
    if !admitted {
        if let Some(project) = &workflow.project_ref {
            admitted = matches!(
                crate::handlers::pack_source::project_write_admitted(
                    state,
                    tenant.community(),
                    project,
                    &self_hex,
                )
                .await,
                Ok(Ok(_))
            );
        }
    }
    if !admitted {
        return Err(IngestError::Rejected(
            "forbidden: only the workflow owner or a project owner may revoke autorun".into(),
        ));
    }
    let tx = match persist_command_event(&state.db, tenant, event, workflow.channel_id).await? {
        PersistResult::Duplicate => {
            return Ok(IngestResult {
                event_id: event.id.to_hex(),
                accepted: true,
                message: "duplicate: already processed".into(),
            });
        }
        PersistResult::Inserted(tx) => tx,
    };
    let revoked = state
        .db
        .revoke_autorun_grants(tenant.community(), workflow_id, event.id.as_bytes())
        .await
        .map_err(|e| IngestError::Internal(format!("error: db revoke_autorun_grants: {e}")))?;
    tx.commit()
        .await
        .map_err(|e| IngestError::Internal(format!("error: commit transaction: {e}")))?;
    let revoked_definition_hash = workflow.definition_hash.clone();
    publish_autorun_changed(
        state,
        tenant.community(),
        &workflow,
        &revoked_definition_hash,
        buzz_core::workflow_autorun::AutorunChange::Revoked,
        &self_hex,
        &event.id.to_hex(),
    )
    .await;
    Ok(IngestResult {
        event_id: event.id.to_hex(),
        accepted: true,
        message: format!(
            "response:{}",
            serde_json::json!({ "status": "revoked", "revoked": revoked })
        ),
    })
}

async fn handle_approval_deny(
    tenant: &TenantContext,
    state: &Arc<AppState>,
    event: &Event,
    auth: &IngestAuth,
) -> Result<IngestResult, IngestError> {
    let self_bytes = auth.pubkey().to_bytes().to_vec();
    let self_hex = hex::encode(&self_bytes);

    // 1. Extract approval reference
    let token_hash_hex = extract_d_tag(event)
        .or_else(|| extract_e_tag(event))
        .ok_or_else(|| {
            IngestError::Rejected("invalid: missing approval reference (d or e tag)".into())
        })?;

    let token_hash = hex::decode(&token_hash_hex)
        .map_err(|_| IngestError::Rejected("invalid: bad approval token hash hex".into()))?;

    // 2. Look up the approval record
    let approval = state
        .db
        .get_approval_by_stored_hash(tenant.community(), &token_hash)
        .await
        .map_err(|_| IngestError::Rejected("invalid: approval not found".into()))?;

    // 3. Validate approval is pending and not expired
    if approval.status != ApprovalStatus::Pending {
        return Err(IngestError::Rejected(format!(
            "invalid: approval already {}",
            approval.status
        )));
    }
    if Utc::now() > approval.expires_at {
        return Err(IngestError::Rejected(
            "invalid: approval token has expired".into(),
        ));
    }

    // 4. Validate caller is authorized approver
    approver_admitted(
        state,
        tenant.community(),
        &approval.approver_spec,
        &self_hex,
        &self_bytes,
    )
    .await?;

    // Persist the command event — returns open transaction
    let tx = match persist_command_event(&state.db, tenant, event, None).await? {
        PersistResult::Duplicate => {
            return Ok(IngestResult {
                event_id: event.id.to_hex(),
                accepted: true,
                message: "duplicate: already processed".into(),
            });
        }
        PersistResult::Inserted(tx) => tx,
    };

    // 5. Execute: update approval status to denied
    let note = if event.content.is_empty() {
        None
    } else {
        Some(event.content.as_str())
    };

    let updated = state
        .db
        .update_approval_by_stored_hash(
            tenant.community(),
            &token_hash,
            ApprovalStatus::Denied,
            Some(&self_bytes),
            note,
        )
        .await
        .map_err(|e| IngestError::Internal(format!("error: db update_approval: {e}")))?;

    if !updated {
        return Err(IngestError::Rejected(
            "invalid: approval already acted on (race)".into(),
        ));
    }

    // Commit: event + approval denial succeeded atomically.
    tx.commit()
        .await
        .map_err(|e| IngestError::Internal(format!("error: commit transaction: {e}")))?;

    // 6. Cancel the workflow run (post-commit, async)
    let community_id = tenant.community();
    let run_id = approval.run_id;
    let pubkey_hex = self_hex.clone();
    let db = state.db.clone();

    tokio::spawn(async move {
        let run = match db.get_workflow_run(community_id, run_id).await {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("approval_deny: failed to fetch run {run_id}: {e}");
                return;
            }
        };

        if run.status != RunStatus::WaitingApproval {
            tracing::warn!(
                "approval_deny: run {run_id} has status '{}', expected 'waiting_approval'",
                run.status
            );
            return;
        }

        let cancel_msg = format!("workflow cancelled: approval denied by {pubkey_hex}");
        if let Err(e) = db
            .update_workflow_run(
                community_id,
                run_id,
                RunStatus::Cancelled,
                run.current_step,
                &run.execution_trace,
                Some(buzz_db::workflow::WorkflowRunFailure {
                    code: "approval_denied",
                    message: &cancel_msg,
                }),
            )
            .await
        {
            tracing::error!("approval_deny: failed to cancel run {run_id}: {e}");
        }
    });

    // 7. Return response
    Ok(IngestResult {
        event_id: event.id.to_hex(),
        accepted: true,
        message: format!(
            "response:{}",
            serde_json::json!({
                "status": "denied",
                "run_id": run_id.to_string(),
            })
        ),
    })
}

/// The definition binding of the run an approval belongs to, or `None` when
/// the run may still act on it.
///
/// A database failure is returned, never swallowed: an approval is not
/// admitted because the check could not be made.
async fn run_definition_stop_for_approval(
    state: &Arc<AppState>,
    community_id: CommunityId,
    approval: &buzz_db::workflow::ApprovalRecord,
) -> Result<ApprovalBinding, buzz_db::error::DbError> {
    let run = state
        .db
        .get_workflow_run(community_id, approval.run_id)
        .await?;
    let workflow = state.db.get_workflow(community_id, run.workflow_id).await?;
    match buzz_workflow::run_definition_stop(&run, &workflow) {
        Some(stop) => Ok(ApprovalBinding::Stop(stop)),
        // Reachable only when the run carries a hash equal to the published
        // one, so the `unwrap_or_default` below cannot lose a binding; it is
        // written this way rather than with an `expect` because a panic in an
        // ingest path is never the right answer.
        None => Ok(ApprovalBinding::Bound(
            run.definition_hash.unwrap_or_default(),
        )),
    }
}

/// What the run behind an approval is bound to.
///
/// The bound hash is the run's own, read once, immutable, and it is what any
/// action-scope grant is written against — never a later read of the
/// workflow row, which a republication can move between the comparison and
/// the insert (ledger 199, review finding 1).
enum ApprovalBinding {
    /// The run is bound to this definition hash, and it is the published one.
    Bound(Vec<u8>),
    /// The run may not act on this approval at all.
    Stop(buzz_workflow::RunStop),
}

/// End a run with a named terminal reason, preserving its trace.
///
/// Best effort by design: the refusal the caller is about to return is the
/// fact that matters, and a run left parked cannot execute anything either —
/// every resume path re-checks the same binding.
async fn stop_run(
    state: &Arc<AppState>,
    community_id: CommunityId,
    run_id: Uuid,
    stop: &buzz_workflow::RunStop,
) {
    let (current_step, trace) = match state.db.get_workflow_run(community_id, run_id).await {
        Ok(run) => (run.current_step, run.execution_trace),
        Err(error) => {
            tracing::error!("stop_run: failed to read run {run_id}: {error}");
            return;
        }
    };
    if let Err(error) = state
        .db
        .update_workflow_run(
            community_id,
            run_id,
            RunStatus::Failed,
            current_step,
            &trace,
            Some(buzz_db::workflow::WorkflowRunFailure {
                code: stop.code(),
                message: &stop.message(),
            }),
        )
        .await
    {
        tracing::error!("stop_run: failed to stop run {run_id}: {error}");
    }
}

/// Resume a suspended workflow run after an approval gate has been granted.
/// Resume a run parked on the approval bound to `approval_step_index`.
///
/// The index execution resumes at is derived from the parsed definition
/// (`buzz_workflow::resume_index_after_approval`): a synthetic gate on a
/// `run_on_host` step resumes **at** that step so the executor can hand it
/// to a host; an authored `request_approval` step resumes after itself.
async fn resume_workflow_after_approval(
    engine: Arc<buzz_workflow::WorkflowEngine>,
    db: buzz_db::Db,
    community_id: CommunityId,
    run_id: Uuid,
    workflow_id: Uuid,
    approval_step_index: usize,
) {
    let run = match db.get_workflow_run(community_id, run_id).await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("resume_workflow: failed to fetch run {run_id}: {e}");
            return;
        }
    };

    // Guard: only resume runs that are actually waiting for approval
    if run.status != RunStatus::WaitingApproval {
        tracing::warn!(
            "resume_workflow: run {run_id} has status '{}', expected 'waiting_approval'",
            run.status
        );
        return;
    }

    let workflow = match db.get_workflow(community_id, workflow_id).await {
        Ok(w) => w,
        Err(e) => {
            tracing::error!("resume_workflow: failed to fetch workflow {workflow_id}: {e}");
            return;
        }
    };

    let def: buzz_workflow::WorkflowDef = match serde_json::from_value(workflow.definition.clone())
    {
        Ok(d) => d,
        Err(e) => {
            tracing::error!("resume_workflow: failed to parse workflow definition: {e}");
            if let Err(db_err) = db
                .update_workflow_run(
                    community_id,
                    run_id,
                    RunStatus::Failed,
                    run.current_step,
                    &run.execution_trace,
                    Some(buzz_db::workflow::WorkflowRunFailure {
                        code: "invalid_definition",
                        message: &format!("definition parse error: {e}"),
                    }),
                )
                .await
            {
                tracing::error!("resume_workflow: failed to mark run as failed: {db_err}");
            }
            return;
        }
    };

    let resume_index = buzz_workflow::resume_index_after_approval(&def, approval_step_index);

    // Reconstruct step_outputs from execution trace for template resolution
    let initial_outputs = super::host_steps::outputs_from_trace(&run.execution_trace);

    // Restore trigger context for {{trigger.*}} templates
    let trigger_ctx: TriggerContext = run
        .trigger_context
        .as_ref()
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    // Execute remaining steps
    let existing_trace = run.execution_trace.as_array().cloned();
    let result = buzz_workflow::executor::execute_from_step(
        &engine,
        community_id,
        run_id,
        &def,
        &trigger_ctx,
        resume_index,
        Some(initial_outputs),
    )
    .await;
    engine
        .finalize_run(community_id, run_id, result, existing_trace)
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::{EventBuilder, Keys, Kind, Tag, Timestamp};
    use sha2::Digest;

    async fn persistence_test_context() -> (buzz_db::Db, TenantContext) {
        let url = std::env::var("BUZZ_TEST_DATABASE_URL")
            .or_else(|_| std::env::var("DATABASE_URL"))
            .unwrap_or_else(|_| "postgres://buzz:buzz_dev@localhost:5432/buzz".to_string());
        let pool = sqlx::PgPool::connect(&url)
            .await
            .expect("connect workflow persistence test database");
        let db = buzz_db::Db::from_pool(pool);
        db.migrate()
            .await
            .expect("migrate workflow persistence test database");
        let host = format!("workflow-cas-{}.example", Uuid::new_v4().simple());
        let community = db
            .ensure_configured_community(&host)
            .await
            .expect("create workflow persistence test community")
            .id;
        (db, TenantContext::resolved(community, host))
    }

    fn workflow_event(
        keys: &Keys,
        workflow_id: Uuid,
        created_at: u64,
        expected_revision: Option<&str>,
        name: &str,
    ) -> Event {
        let workflow_id = workflow_id.to_string();
        let channel_id = Uuid::new_v4().to_string();
        let mut tags = vec![
            Tag::parse(["d", workflow_id.as_str()]).expect("d tag"),
            Tag::parse(["h", channel_id.as_str()]).expect("h tag"),
        ];
        if let Some(revision) = expected_revision {
            tags.push(Tag::parse(["expected-revision", revision]).expect("revision tag"));
        }
        EventBuilder::new(
            Kind::Custom(KIND_WORKFLOW_DEF as u16),
            format!("name: {name}\ntrigger:\n  on: message_posted\nsteps: []\n"),
        )
        .tags(tags)
        .custom_created_at(Timestamp::from(created_at))
        .sign_with_keys(keys)
        .expect("workflow event")
    }

    fn rejection_message(result: Result<(), IngestError>) -> String {
        match result {
            Err(IngestError::Rejected(message)) => message,
            Err(other) => panic!("unexpected refusal: {other:?}"),
            Ok(()) => panic!("expected revision validation to fail"),
        }
    }

    #[test]
    fn workflow_revision_accepts_create_and_matching_update() {
        let existing = [0x42; 32];
        assert!(validate_workflow_revision(KIND_WORKFLOW_DEF as i32, None, None).is_ok());
        assert!(validate_workflow_revision(
            KIND_WORKFLOW_DEF as i32,
            Some(&hex::encode(existing)),
            Some(&existing),
        )
        .is_ok());
    }

    #[test]
    fn workflow_revision_rejects_stale_and_malformed_updates() {
        let existing = [0x42; 32];
        let stale = [0x24; 32];
        assert_eq!(
            rejection_message(validate_workflow_revision(
                KIND_WORKFLOW_DEF as i32,
                Some(&hex::encode(stale)),
                Some(&existing),
            )),
            "conflict: workflow changed since it was loaded",
        );
        assert!(
            validate_workflow_revision(KIND_WORKFLOW_DEF as i32, None, Some(&existing)).is_ok(),
            "tagless legacy workflow updates remain compatible during rollout",
        );
        for malformed in ["not-hex", "42"] {
            assert_eq!(
                rejection_message(validate_workflow_revision(
                    KIND_WORKFLOW_DEF as i32,
                    Some(malformed),
                    Some(&existing),
                )),
                "invalid: bad expected workflow revision",
            );
            assert_eq!(
                rejection_message(validate_workflow_revision(
                    KIND_WORKFLOW_DEF as i32,
                    Some(malformed),
                    None,
                )),
                "invalid: bad expected workflow revision",
            );
        }
    }

    #[test]
    fn workflow_revision_rejects_update_for_missing_coordinate() {
        assert_eq!(
            rejection_message(validate_workflow_revision(
                KIND_WORKFLOW_DEF as i32,
                Some(&hex::encode([0x42; 32])),
                None,
            )),
            "conflict: workflow revision does not exist",
        );
    }

    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn workflow_persistence_preserves_replays_and_rejects_dominated_cas_updates() {
        let (db, tenant) = persistence_test_context().await;
        let keys = Keys::generate();
        let workflow_id = Uuid::new_v4();
        let created_at = Timestamp::now().as_secs();
        let create = workflow_event(&keys, workflow_id, created_at, None, "create");

        let PersistResult::Inserted(tx) = persist_command_event(&db, &tenant, &create, None)
            .await
            .expect("persist create")
        else {
            panic!("first create must insert");
        };
        tx.commit().await.expect("commit create");
        assert!(matches!(
            persist_command_event(&db, &tenant, &create, None)
                .await
                .expect("replay create"),
            PersistResult::Duplicate
        ));

        let create_revision = create.id.to_hex();
        let mut updates = (0..64).map(|index| {
            workflow_event(
                &keys,
                workflow_id,
                created_at,
                Some(&create_revision),
                &format!("update-{index}"),
            )
        });
        let update = updates
            .find(|candidate| candidate.id.as_bytes() < create.id.as_bytes())
            .expect("find same-second update that wins NIP-33 ordering");
        let dominated_update = (64..256)
            .map(|index| {
                workflow_event(
                    &keys,
                    workflow_id,
                    created_at,
                    Some(&update.id.to_hex()),
                    &format!("update-{index}"),
                )
            })
            .find(|candidate| candidate.id.as_bytes() > update.id.as_bytes())
            .expect("find same-second CAS-matching update dominated by current head");

        let PersistResult::Inserted(tx) = persist_command_event(&db, &tenant, &update, None)
            .await
            .expect("persist update")
        else {
            panic!("matching update must insert");
        };
        tx.commit().await.expect("commit update");
        assert!(matches!(
            persist_command_event(&db, &tenant, &update, None)
                .await
                .expect("replay update"),
            PersistResult::Duplicate
        ));

        let error = match persist_command_event(&db, &tenant, &dominated_update, None).await {
            Err(error) => error,
            Ok(_) => panic!("distinct dominated CAS update must not report duplicate success"),
        };
        assert!(matches!(
            error,
            IngestError::Rejected(ref message)
                if message == "conflict: workflow update was superseded; refresh and try again"
        ));
    }

    /// The grant handler hands the approval's own step index to the resume
    /// path, which derives where to continue from the parsed definition: a
    /// synthetic gate on a `run_on_host` step resumes at that step, an
    /// authored gate after itself.
    #[test]
    fn approval_resume_index_comes_from_the_parsed_definition() {
        let yaml = format!(
            "name: nightly\nproject: '30621:{}:pulse'\ntrigger:\n  on: manual\nsteps:\n  - id: gate\n    action: request_approval\n    from: any\n    message: ok?\n  - id: build\n    action: run_on_host\n    command: [\"true\"]\n",
            "1".repeat(64)
        );
        let (def, canonical) = buzz_workflow::WorkflowEngine::parse_yaml(&yaml).expect("parse");
        // The stored definition round-trips through the same JSON the resume
        // path reads back from `workflows.definition`.
        let stored: serde_json::Value = serde_json::from_str(&canonical).expect("json");
        let reread: buzz_workflow::WorkflowDef =
            serde_json::from_value(stored.clone()).expect("reread");
        assert_eq!(buzz_workflow::resume_index_after_approval(&reread, 0), 1);
        assert_eq!(buzz_workflow::resume_index_after_approval(&reread, 1), 1);
        assert_eq!(buzz_workflow::resume_index_after_approval(&def, 1), 1);
        // And the hash the relay stores is the one a host recomputes.
        assert_eq!(
            buzz_workflow::hash_definition_value(&stored).expect("hash"),
            buzz_workflow::definition_hash(&reread).expect("hash")
        );
    }

    /// A one-shot rendezvous the race test installs to hold
    /// `handle_approval_grant` between its binding comparison and the autorun
    /// grant insert, so a republication can land in exactly that window.
    ///
    /// `None` by default, so no other test — and no production build, which
    /// never compiles this module — is affected.
    pub(super) struct GrantPause {
        reached: tokio::sync::mpsc::Sender<()>,
        resume: tokio::sync::Mutex<tokio::sync::mpsc::Receiver<()>>,
    }

    static GRANT_PAUSE: std::sync::Mutex<Option<std::sync::Arc<GrantPause>>> =
        std::sync::Mutex::new(None);

    /// Install the pause and return the test's two ends of it.
    fn install_grant_pause() -> (
        tokio::sync::mpsc::Receiver<()>,
        tokio::sync::mpsc::Sender<()>,
    ) {
        let (reached_tx, reached_rx) = tokio::sync::mpsc::channel(1);
        let (resume_tx, resume_rx) = tokio::sync::mpsc::channel(1);
        *GRANT_PAUSE.lock().expect("grant pause lock") = Some(std::sync::Arc::new(GrantPause {
            reached: reached_tx,
            resume: tokio::sync::Mutex::new(resume_rx),
        }));
        (reached_rx, resume_tx)
    }

    fn remove_grant_pause() {
        *GRANT_PAUSE.lock().expect("grant pause lock") = None;
    }

    /// Called by the handler. A no-op unless a test installed a pause.
    pub(super) async fn pause_before_autorun_grant() {
        let pause = GRANT_PAUSE.lock().expect("grant pause lock").clone();
        let Some(pause) = pause else {
            return;
        };
        let _ = pause.reached.send(()).await;
        let _ = pause.resume.lock().await.recv().await;
    }

    /// A community, a channel, a published one-step project action, a run
    /// parked on its synthetic approval gate, and the raw approval token.
    async fn approval_fixture(
        state: &Arc<AppState>,
        command: &str,
    ) -> (TenantContext, Uuid, Uuid, String) {
        let owner = Keys::generate();
        let owner_bytes = owner.public_key().to_bytes().to_vec();
        let host = format!("approval-binding-{}.example", Uuid::new_v4().simple());
        let community = match state
            .db
            .create_community_with_owner(&host, &owner.public_key().to_hex())
            .await
            .expect("create community")
        {
            buzz_db::CreateCommunityWithOwnerResult::Created(record) => record.id,
            other => panic!("expected a fresh community, got {other:?}"),
        };
        let tenant = TenantContext::resolved(community, host);
        state
            .db
            .ensure_user(community, &owner_bytes)
            .await
            .expect("ensure owner");
        let channel = state
            .db
            .create_channel(
                community,
                "actions",
                buzz_core::channel::ChannelType::Stream,
                buzz_core::channel::ChannelVisibility::Private,
                None,
                &owner_bytes,
                None,
                None,
            )
            .await
            .expect("create channel");
        let project = format!("30621:{}:pulse", "1".repeat(64));
        let yaml = format!(
            "name: verify\nproject: '{project}'\ntrigger:\n  on: manual\nsteps:\n  - id: build\n    action: run_on_host\n    command: [\"echo\", \"{command}\"]\n"
        );
        let (def, canonical) = buzz_workflow::WorkflowEngine::parse_yaml(&yaml).expect("parse");
        let hash = buzz_workflow::definition_hash(&def).expect("hash");
        let workflow_id = state
            .db
            .create_workflow(
                community,
                Some(channel.id),
                &owner_bytes,
                "verify",
                &canonical,
                &hash,
                Some(&project),
            )
            .await
            .expect("create workflow");
        let run_id = state
            .db
            .create_workflow_run(community, workflow_id, None, None, &hash)
            .await
            .expect("create run");
        let run_id = match run_id {
            buzz_db::workflow::CreateRunOutcome::Created(id) => id,
            other => panic!("expected a run, got {other:?}"),
        };
        let token = Uuid::new_v4().to_string();
        state
            .db
            .create_approval(buzz_db::workflow::CreateApprovalParams {
                community_id: community,
                token: &token,
                workflow_id,
                run_id,
                step_id: "build",
                step_index: 0,
                approver_spec: "any",
                expires_at: Utc::now() + chrono::Duration::hours(1),
            })
            .await
            .expect("create approval");
        state
            .db
            .update_workflow_run(
                community,
                run_id,
                RunStatus::WaitingApproval,
                0,
                &serde_json::json!([{"step_id": "build", "status": "awaiting_approval"}]),
                None,
            )
            .await
            .expect("park run");
        (tenant, workflow_id, run_id, token)
    }

    /// Republish the action with a different command under the same id.
    async fn republish(state: &Arc<AppState>, community: CommunityId, workflow_id: Uuid) {
        let workflow = state
            .db
            .get_workflow(community, workflow_id)
            .await
            .expect("workflow");
        let project = workflow.project_ref.clone().expect("project ref");
        let yaml = format!(
            "name: verify\nproject: '{project}'\ntrigger:\n  on: manual\nsteps:\n  - id: build\n    action: run_on_host\n    command: [\"echo\", \"v2\"]\n"
        );
        let (def, canonical) = buzz_workflow::WorkflowEngine::parse_yaml(&yaml).expect("parse v2");
        let hash = buzz_workflow::definition_hash(&def).expect("hash v2");
        state
            .db
            .update_workflow(
                community,
                workflow_id,
                "verify",
                &canonical,
                &hash,
                Some(&project),
            )
            .await
            .expect("republish");
    }

    fn grant_event(keys: &Keys, token: &str) -> Event {
        grant_event_with_scope(keys, token, "run")
    }

    fn grant_event_with_scope(keys: &Keys, token: &str, scope: &str) -> Event {
        let token_hash = hex::encode(sha2::Sha256::digest(token.as_bytes()));
        EventBuilder::new(
            Kind::Custom(KIND_APPROVAL_GRANT as u16),
            serde_json::json!({ "note": "ok", "scope": scope }).to_string(),
        )
        .tags(vec![Tag::parse(["d", token_hash.as_str()]).expect("d tag")])
        .sign_with_keys(keys)
        .expect("grant event")
    }

    fn http_auth(keys: &Keys) -> IngestAuth {
        IngestAuth::Http {
            pubkey: keys.public_key(),
            scopes: vec![buzz_auth::Scope::MessagesWrite],
            auth_method: super::super::ingest::HttpAuthMethod::Nip98,
        }
    }

    /// Records the host-step requests the engine would have published, so a
    /// test can count what reached a host.
    #[derive(Default)]
    struct CountingSink {
        host_steps: std::sync::Mutex<Vec<buzz_core::host_step::HostStepRequested>>,
    }

    impl buzz_workflow::ActionSink for CountingSink {
        fn send_message(
            &self,
            _community_id: CommunityId,
            _channel_id: &str,
            _text: &str,
            _author_pubkey: &str,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = Result<String, buzz_workflow::ActionSinkError>>
                    + Send
                    + '_,
            >,
        > {
            Box::pin(async { Ok("00".repeat(32)) })
        }

        fn record_ci_result(
            &self,
            _community_id: CommunityId,
            _result: &buzz_core::ci_result::CiResult,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = Result<String, buzz_workflow::ActionSinkError>>
                    + Send
                    + '_,
            >,
        > {
            Box::pin(async { Ok("00".repeat(32)) })
        }

        fn request_approval(
            &self,
            _community_id: CommunityId,
            _request: &buzz_workflow::ApprovalRequest,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = Result<String, buzz_workflow::ActionSinkError>>
                    + Send
                    + '_,
            >,
        > {
            Box::pin(async { Ok("aa".repeat(32)) })
        }

        fn request_host_step(
            &self,
            _community_id: CommunityId,
            request: &buzz_core::host_step::HostStepRequested,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = Result<String, buzz_workflow::ActionSinkError>>
                    + Send
                    + '_,
            >,
        > {
            self.host_steps
                .lock()
                .expect("host steps lock")
                .push(request.clone());
            Box::pin(async { Ok("bb".repeat(32)) })
        }
    }

    /// Ledger 199, review finding 1: an action-scope grant must record the
    /// definition the person actually approved, not whatever the workflow row
    /// says by the time the grant is written.
    ///
    /// The interleaving, forced with a barrier: the handler's binding
    /// comparison sees A/A; B is published; only then is the grant created.
    /// Before the fix the grant named B, so a fresh B run needed no approval
    /// at all.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn an_action_grant_records_the_definition_that_was_approved() {
        let state = crate::state::tests::test_state().await;
        let sink = std::sync::Arc::new(CountingSink::default());
        state.workflow_engine.set_action_sink(sink.clone());
        let (tenant, workflow_id, run_id, token) = approval_fixture(&state, "v1").await;
        let community = tenant.community();
        let hash_a = state
            .db
            .get_workflow(community, workflow_id)
            .await
            .expect("workflow a")
            .definition_hash
            .clone();

        let (mut reached, resume) = install_grant_pause();
        let approver = Keys::generate();
        let event = grant_event_with_scope(&approver, &token, "action");
        let handler = {
            let state = Arc::clone(&state);
            let tenant = tenant.clone();
            let auth = http_auth(&approver);
            tokio::spawn(async move { handle_approval_grant(&tenant, &state, &event, &auth).await })
        };

        // The handler has compared A against A and is holding before the
        // grant insert. Publish B in exactly that window.
        reached.recv().await.expect("handler reached the pause");
        republish(&state, community, workflow_id).await;
        let hash_b = state
            .db
            .get_workflow(community, workflow_id)
            .await
            .expect("workflow b")
            .definition_hash
            .clone();
        assert_ne!(hash_a, hash_b);
        resume.send(()).await.expect("release the handler");
        let accepted = handler.await.expect("handler joined");
        remove_grant_pause();
        assert!(accepted.is_ok(), "the grant itself is accepted");

        // The grant names A. Nothing has consented to B.
        let granted_a = state
            .db
            .find_active_autorun_grant(community, workflow_id, &hash_a)
            .await
            .expect("grant lookup a")
            .is_some();
        let granted_b = state
            .db
            .find_active_autorun_grant(community, workflow_id, &hash_b)
            .await
            .expect("grant lookup b")
            .is_some();
        assert!(
            granted_a && !granted_b,
            "the grant must name the approved definition and only that one \
             (grant for the approved A: {granted_a}, grant for the unapproved B: {granted_b})"
        );

        // And a fresh run of B still asks: zero host requests without approval.
        let workflow_b = state
            .db
            .get_workflow(community, workflow_id)
            .await
            .expect("workflow b");
        let def_b: buzz_workflow::WorkflowDef =
            serde_json::from_value(workflow_b.definition.clone()).expect("parse b");
        let run_b = match state
            .db
            .create_workflow_run(community, workflow_id, None, None, &hash_b)
            .await
            .expect("create b run")
        {
            buzz_db::workflow::CreateRunOutcome::Created(id) => id,
            other => panic!("expected a run, got {other:?}"),
        };
        let result = buzz_workflow::executor::execute_run(
            &state.workflow_engine,
            community,
            run_b,
            &def_b,
            &TriggerContext::default(),
        )
        .await
        .expect("execute b");
        let unapproved_host_requests = sink.host_steps.lock().expect("lock").len();
        assert_eq!(
            unapproved_host_requests, 0,
            "a fresh run of a definition nobody approved must reach no host"
        );
        assert!(
            matches!(
                result.suspension,
                Some(buzz_workflow::Suspension::Approval { .. })
            ),
            "and it must ask: {:?}",
            result.suspension.map(|s| s.step_id().to_owned())
        );
        // The originally approved run is untouched by any of this.
        assert_eq!(
            state
                .db
                .get_workflow_run(community, run_id)
                .await
                .expect("run")
                .definition_hash,
            Some(hash_a)
        );
    }

    /// Ledger 193: an approval is consent for one definition. Editing the
    /// action after the request was published refuses the grant at ingest and
    /// stops the run, so no resume can hand the new command to a host.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn a_grant_is_refused_when_the_definition_changed() {
        let state = crate::state::tests::test_state().await;
        let (tenant, workflow_id, run_id, token) = approval_fixture(&state, "v1").await;
        republish(&state, tenant.community(), workflow_id).await;

        let approver = Keys::generate();
        let event = grant_event(&approver, &token);
        let message =
            match handle_approval_grant(&tenant, &state, &event, &http_auth(&approver)).await {
                Err(IngestError::Rejected(message)) => message,
                Err(other) => panic!("expected a refusal, got {other:?}"),
                Ok(accepted) => panic!("expected a refusal, got {}", accepted.message),
            };
        assert!(
            message.contains(buzz_workflow::RUN_STOPPED_DEFINITION_CHANGED),
            "the refusal names the reason: {message}"
        );

        let run = state
            .db
            .get_workflow_run(tenant.community(), run_id)
            .await
            .expect("run");
        assert_eq!(run.status, RunStatus::Failed);
        assert_eq!(
            run.error_code.as_deref(),
            Some(buzz_workflow::RUN_STOPPED_DEFINITION_CHANGED)
        );
        let approvals = state
            .db
            .get_run_approvals(tenant.community(), workflow_id, run_id)
            .await
            .expect("approvals");
        assert!(
            approvals
                .iter()
                .all(|approval| approval.status == ApprovalStatus::Pending),
            "the approval was never consumed"
        );
    }

    /// A run created before migration 0046 carries no binding. It stays
    /// readable, and it cannot gain authority to execute: granting its
    /// approval refuses with `definition_unknown`.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn a_run_without_a_binding_cannot_be_approved() {
        let state = crate::state::tests::test_state().await;
        let (tenant, workflow_id, run_id, token) = approval_fixture(&state, "v1").await;

        // Exactly the shape of a run written before the column existed.
        let url = std::env::var("BUZZ_TEST_DATABASE_URL")
            .or_else(|_| std::env::var("DATABASE_URL"))
            .expect("a test database url");
        let pool = sqlx::PgPool::connect(&url).await.expect("connect");
        sqlx::query(
            "UPDATE workflow_runs SET definition_hash = NULL WHERE community_id = $1 AND id = $2",
        )
        .bind(tenant.community().as_uuid())
        .bind(run_id)
        .execute(&pool)
        .await
        .expect("unbind the run");

        let run = state
            .db
            .get_workflow_run(tenant.community(), run_id)
            .await
            .expect("a legacy run is still readable");
        assert!(run.definition_hash.is_none());

        let approver = Keys::generate();
        let event = grant_event(&approver, &token);
        let message =
            match handle_approval_grant(&tenant, &state, &event, &http_auth(&approver)).await {
                Err(IngestError::Rejected(message)) => message,
                Err(other) => panic!("expected a refusal, got {other:?}"),
                Ok(accepted) => panic!("expected a refusal, got {}", accepted.message),
            };
        assert!(
            message.contains(buzz_workflow::RUN_STOPPED_DEFINITION_UNKNOWN),
            "the refusal names the reason: {message}"
        );
        let run = state
            .db
            .get_workflow_run(tenant.community(), run_id)
            .await
            .expect("run");
        assert_eq!(run.status, RunStatus::Failed);
        assert_eq!(
            run.error_code.as_deref(),
            Some(buzz_workflow::RUN_STOPPED_DEFINITION_UNKNOWN)
        );
        assert_eq!(
            state
                .db
                .get_run_approvals(tenant.community(), workflow_id, run_id)
                .await
                .expect("approvals")
                .len(),
            1
        );
    }

    #[test]
    fn revision_tag_does_not_change_other_command_kinds() {
        assert!(validate_workflow_revision(KIND_DM_OPEN as i32, Some("not-hex"), None).is_ok());
    }
}
