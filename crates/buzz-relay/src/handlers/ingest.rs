//! Transport-neutral event ingestion pipeline.
//!
//! Both WebSocket `["EVENT", ...]` and HTTP `POST /events` feed into
//! [`ingest_event`] — two doors, one room.

use std::sync::Arc;

use chrono::Utc;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use buzz_auth::Scope;
use buzz_core::kind::{
    event_kind_u32, is_identity_archive_request_kind, is_parameterized_replaceable,
    is_relay_admin_kind, KIND_AGENT_ENGRAM, KIND_AGENT_PROFILE, KIND_AGENT_TURN_METRIC,
    KIND_APPROVAL_DENY, KIND_APPROVAL_GRANT, KIND_AUTH, KIND_BOOKMARK_LIST, KIND_BOOKMARK_SET,
    KIND_CANVAS, KIND_CODING_SESSION_AUTHORITY_TRANSITION, KIND_CODING_SESSION_CLOSURE,
    KIND_CODING_SESSION_COMMAND, KIND_CODING_SESSION_GENESIS, KIND_CODING_SESSION_GOAL,
    KIND_CODING_SESSION_HANDOVER, KIND_CODING_SESSION_LEASE, KIND_CODING_SESSION_LIFECYCLE_COMMAND,
    KIND_CODING_SESSION_LIFECYCLE_RECEIPT, KIND_CODING_SESSION_METADATA, KIND_CODING_SESSION_NAME,
    KIND_CODING_SESSION_OBSERVATION, KIND_CODING_SESSION_POLICY,
    KIND_CODING_SESSION_PROVIDER_CATALOG, KIND_CODING_SESSION_TEAM_TRANSACTION,
    KIND_CODING_SESSION_TRANSCRIPT, KIND_CONTACT_LIST, KIND_DELETION, KIND_DM_ADD_MEMBER,
    KIND_DM_HIDE, KIND_DM_OPEN, KIND_EMOJI_LIST, KIND_EMOJI_SET, KIND_EVENT_REMINDER,
    KIND_FOLLOW_SET, KIND_FORUM_COMMENT, KIND_FORUM_POST, KIND_FORUM_VOTE, KIND_GIFT_WRAP,
    KIND_GIT_ISSUE, KIND_GIT_PATCH, KIND_GIT_PR_UPDATE, KIND_GIT_PULL_REQUEST,
    KIND_GIT_REPO_ANNOUNCEMENT, KIND_GIT_REPO_STATE, KIND_GIT_STATUS_CLOSED, KIND_GIT_STATUS_DRAFT,
    KIND_GIT_STATUS_MERGED, KIND_GIT_STATUS_OPEN, KIND_HUDDLE_ENDED, KIND_HUDDLE_GUIDELINES,
    KIND_HUDDLE_PARTICIPANT_JOINED, KIND_HUDDLE_PARTICIPANT_LEFT, KIND_HUDDLE_STARTED,
    KIND_IA_ARCHIVE_REQUEST, KIND_IA_UNARCHIVE_REQUEST, KIND_LONG_FORM, KIND_MANAGED_AGENT,
    KIND_MEMBER_ADDED_NOTIFICATION, KIND_MEMBER_REMOVED_NOTIFICATION, KIND_MODERATION_BAN,
    KIND_MODERATION_RESOLVE_REPORT, KIND_MODERATION_TIMEOUT, KIND_MODERATION_UNBAN,
    KIND_MODERATION_UNTIMEOUT, KIND_MUTE_LIST, KIND_NIP29_CREATE_GROUP, KIND_NIP29_DELETE_EVENT,
    KIND_NIP29_DELETE_GROUP, KIND_NIP29_EDIT_METADATA, KIND_NIP29_JOIN_REQUEST,
    KIND_NIP29_LEAVE_REQUEST, KIND_NIP29_PUT_USER, KIND_NIP29_REMOVE_USER,
    KIND_NIP43_LEAVE_REQUEST, KIND_NIP65_RELAY_LIST_METADATA, KIND_PERSONA, KIND_PIN_LIST,
    KIND_PRIVATE_MANAGED_AGENT, KIND_PRODUCT_FEEDBACK, KIND_PROFILE, KIND_PROJECT, KIND_REACTION,
    KIND_READ_STATE, KIND_REPORT, KIND_STREAM_MESSAGE, KIND_STREAM_MESSAGE_BOOKMARKED,
    KIND_STREAM_MESSAGE_DIFF, KIND_STREAM_MESSAGE_EDIT, KIND_STREAM_MESSAGE_PINNED,
    KIND_STREAM_MESSAGE_SCHEDULED, KIND_STREAM_MESSAGE_V2, KIND_STREAM_REMINDER, KIND_TEAM,
    KIND_TEAM_CATALOG, KIND_TEXT_NOTE, KIND_USER_STATUS, KIND_WORKFLOW_DEF, KIND_WORKFLOW_TRIGGER,
    RELAY_ADMIN_ADD_MEMBER, RELAY_ADMIN_CHANGE_ROLE, RELAY_ADMIN_REMOVE_MEMBER,
    RELAY_ADMIN_SET_WORKSPACE_PROFILE,
};
use buzz_core::tenant::TenantContext;
use buzz_core::verification::verify_event;
use buzz_core::CommunityId;
use nostr::Event;

use crate::state::AppState;

use super::event::dispatch_persistent_event;

use crate::conformance::{
    self as conf, channel_label, claimed_community_from_event, emit, msg_id_label,
    state_for_request, EmitGuard, TraceAction, Verdict,
};

fn validate_custom_emoji_tags(event: &Event) -> Result<(), IngestError> {
    for tag in event.tags.iter() {
        let parts = tag.as_slice();
        if parts.first().map(String::as_str) != Some("emoji") {
            continue;
        }
        let shortcode = parts.get(1).ok_or_else(|| {
            IngestError::Rejected("invalid: emoji tag must include a shortcode".into())
        })?;
        buzz_sdk::normalize_custom_emoji_shortcode(shortcode)
            .map_err(|err| IngestError::Rejected(format!("invalid: {err}")))?;
    }
    Ok(())
}

fn validate_reaction_emoji(event: &Event, emoji: &str) -> Result<(), IngestError> {
    let emoji_char_count = emoji.chars().count();
    if emoji_char_count <= 64 {
        return Ok(());
    }

    let Some(shortcode) = emoji
        .strip_prefix(':')
        .and_then(|value| value.strip_suffix(':'))
    else {
        return Err(IngestError::Rejected(format!(
            "invalid: reaction emoji exceeds 64 characters (got {emoji_char_count})"
        )));
    };
    let normalized = buzz_sdk::normalize_custom_emoji_shortcode(shortcode)
        .map_err(|err| IngestError::Rejected(format!("invalid: {err}")))?;
    if shortcode != normalized {
        return Err(IngestError::Rejected(
            "invalid: long custom emoji reaction shortcode must be canonical lowercase".into(),
        ));
    }
    let has_matching_tag = event.tags.iter().any(|tag| {
        let parts = tag.as_slice();
        parts.first().map(String::as_str) == Some("emoji")
            && parts.get(1).is_some_and(|value| value == shortcode)
    });
    if !has_matching_tag || emoji_char_count > buzz_sdk::MAX_CUSTOM_EMOJI_REACTION_LEN {
        return Err(IngestError::Rejected(format!(
            "invalid: reaction emoji exceeds 64 characters (got {emoji_char_count})"
        )));
    }
    Ok(())
}

/// How the HTTP caller authenticated (for [`IngestAuth::Http`]).
#[derive(Debug, Clone)]
pub enum HttpAuthMethod {
    /// `Authorization: Nostr <base64>` — NIP-98 HTTP Auth.
    Nip98,
    /// `X-Pubkey: <hex>` dev-mode header (backward compat during transition).
    DevPubkey,
}

/// Authentication context for event ingestion — transport-neutral.
#[derive(Debug, Clone)]
pub enum IngestAuth {
    /// WebSocket NIP-42 authenticated connection.
    Nip42 {
        /// The authenticated Nostr public key.
        pubkey: nostr::PublicKey,
        /// Permission scopes granted to this connection.
        scopes: Vec<Scope>,
        /// Token-level channel restriction, if the WebSocket auth used an API token.
        channel_ids: Option<Vec<Uuid>>,
        /// WebSocket connection identifier.
        conn_id: Uuid,
    },
    /// HTTP bridge authenticated request (NIP-98 or dev X-Pubkey).
    Http {
        /// The authenticated Nostr public key.
        pubkey: nostr::PublicKey,
        /// Permission scopes granted to this request.
        scopes: Vec<Scope>,
        /// How the HTTP request was authenticated.
        auth_method: HttpAuthMethod,
    },
}

impl IngestAuth {
    /// The authenticated public key.
    pub fn pubkey(&self) -> &nostr::PublicKey {
        match self {
            Self::Nip42 { pubkey, .. } | Self::Http { pubkey, .. } => pubkey,
        }
    }

    /// Pubkey used for principal-scoped accounting and policy lookups.
    pub fn principal_pubkey_bytes(&self) -> Vec<u8> {
        self.pubkey().to_bytes().to_vec()
    }

    /// Permission scopes for this auth context.
    pub fn scopes(&self) -> &[Scope] {
        match self {
            Self::Nip42 { scopes, .. } | Self::Http { scopes, .. } => scopes,
        }
    }

    /// WebSocket connection ID (Nip42 only).
    pub fn conn_id(&self) -> Option<Uuid> {
        match self {
            Self::Nip42 { conn_id, .. } => Some(*conn_id),
            Self::Http { .. } => None,
        }
    }

    /// Token-level channel restriction (WS connections with scoped tokens — legacy).
    /// In pure Nostr mode this always returns None; channel access is enforced
    /// via NIP-29 membership checks instead.
    pub fn channel_ids(&self) -> Option<&[Uuid]> {
        match self {
            Self::Nip42 {
                channel_ids: Some(ids),
                ..
            } => Some(ids),
            _ => None,
        }
    }

    /// Whether this auth context is an HTTP request (not WebSocket).
    pub fn is_http(&self) -> bool {
        matches!(self, Self::Http { .. })
    }
}

fn emit_product_feedback_success(
    tracer: &Arc<dyn buzz_conformance::Tracer>,
    tenant: &TenantContext,
    event: &Event,
    auth: &IngestAuth,
) {
    emit(
        tracer,
        TraceAction::WriteInsertGlobal {
            msg_id: msg_id_label(event.id.as_bytes()),
            claimed_community: claimed_community_from_event(event),
        },
        state_for_request(tenant, auth.pubkey()),
    );
}

/// Increment the rejection counter with a bounded reason and transport label.
///
/// Shared by the WS `EVENT` handler and the HTTP `POST /events` handler so
/// both transports feed the same series — `transport` distinguishes them so
/// existing WS-only dashboards aren't silently diluted by HTTP volume.
/// `reason` is one of a small closed set ("auth", "invalid", "scope",
/// "error") — bounded, no cardinality risk.
pub fn reject_with_transport(transport: &'static str, reason: &'static str) {
    metrics::counter!(
        "buzz_events_rejected_total",
        "transport" => transport,
        "reason" => reason
    )
    .increment(1);
}

fn valid_link_preview_text(value: &str, max: usize, allow_newlines: bool) -> bool {
    value.len() <= max
        && !value
            .chars()
            .any(|character| character.is_control() && !(allow_newlines && character == '\n'))
}

fn validate_link_preview_tags(event: &Event, media_base_url: &str) -> Result<(), String> {
    const MAX_SNAPSHOTS: usize = 8;
    const MAX_TITLE: usize = 300;
    const MAX_SITE: usize = 100;
    const MAX_DESCRIPTION: usize = 1000;

    let mut count = 0;
    let mut suppressed = false;
    let mut seen = std::collections::HashSet::new();
    for tag in event.tags.iter() {
        let parts = tag.as_slice();
        if parts.first().map(String::as_str) != Some("link-preview") {
            continue;
        }
        count += 1;
        if parts == ["link-preview", "none"] {
            if count > 1 {
                return Err("link-preview suppression cannot include snapshots".into());
            }
            suppressed = true;
            continue;
        }
        if suppressed
            || count > MAX_SNAPSHOTS
            || parts.len() != 11
            || parts[1] != "snapshot"
            || parts[2] != "1"
        {
            return Err("invalid link-preview snapshot tag".into());
        }
        let canonical =
            url::Url::parse(&parts[3]).map_err(|_| "invalid link-preview canonical URL")?;
        if canonical.scheme() != "https"
            || !canonical.username().is_empty()
            || canonical.password().is_some()
            || canonical.fragment().is_some()
            || !seen.insert(parts[3].clone())
            || !event.content.contains(&parts[3])
        {
            return Err("invalid link-preview canonical URL".into());
        }
        for (value, max, allow_newlines) in [
            (&parts[4], MAX_TITLE, false),
            (&parts[5], MAX_SITE, false),
            (&parts[6], MAX_DESCRIPTION, true),
        ] {
            if !valid_link_preview_text(value, max, allow_newlines) {
                return Err("invalid link-preview snapshot text".into());
            }
        }
        if !super::imeta::validate_local_image_media_pair(&parts[7], &parts[8], media_base_url)
            || !super::imeta::validate_local_image_media_pair(&parts[9], &parts[10], media_base_url)
        {
            return Err("link-preview media must reference matching local image blobs".into());
        }
    }
    Ok(())
}

/// Successful ingestion result.
pub struct IngestResult {
    /// Hex-encoded event ID.
    pub event_id: String,
    /// Whether the event was accepted.
    pub accepted: bool,
    /// Optional message (e.g. "duplicate:" for dedup).
    pub message: String,
}

pub use super::ingest_error::IngestError;

/// Map the durable community write-fence lookup onto the ingest error taxonomy.
///
/// An inactive community is an authorization decision and keeps the exact
/// `restricted:` wire text the ephemeral path uses. A lookup outage is a
/// server fault and fails closed as `error:`/500 — a Postgres blip can
/// neither admit a write past the fence nor read as a client mistake.
fn map_serving_fence_state(active: Result<bool, buzz_db::DbError>) -> Result<(), IngestError> {
    match active {
        Ok(true) => Ok(()),
        Ok(false) => Err(IngestError::Rejected(
            "restricted: community writes are fenced".into(),
        )),
        Err(error) => Err(IngestError::Internal(format!(
            "error: checking community write fence: {error}"
        ))),
    }
}

fn map_relay_admin_error(error: super::relay_admin::RelayAdminError) -> IngestError {
    use super::relay_admin::RelayAdminError;
    match error {
        // Same wire prefix and HTTP status (403) as every other durable
        // restriction refusal — see the write-path gate below and `auth.rs`.
        RelayAdminError::Banned => {
            IngestError::AuthFailed("blocked: you are banned from this community".to_string())
        }
        RelayAdminError::Rejected(reason) => IngestError::Rejected(format!("invalid: {reason}")),
        RelayAdminError::Internal(reason) => IngestError::Internal(format!("error: {reason}")),
    }
}

fn map_push_accept_error(error: super::push_lease::AcceptError) -> IngestError {
    match error {
        super::push_lease::AcceptError::Validation(reason) => {
            IngestError::Rejected(format!("invalid: {reason}"))
        }
        super::push_lease::AcceptError::Internal(reason) => IngestError::Internal(reason),
    }
}

/// Determine the required scope for a given event kind.
///
/// Returns `Err` for unknown kinds — the relay rejects them.
fn required_scope_for_kind(kind: u32, event: &Event) -> Result<Scope, &'static str> {
    match kind {
        KIND_PROFILE => Ok(Scope::UsersWrite),
        KIND_TEXT_NOTE | KIND_LONG_FORM => Ok(Scope::MessagesWrite),
        KIND_CONTACT_LIST | KIND_READ_STATE | KIND_USER_STATUS | KIND_AGENT_ENGRAM
        | KIND_EVENT_REMINDER | KIND_PERSONA | KIND_TEAM | KIND_MANAGED_AGENT
        | KIND_PRIVATE_MANAGED_AGENT | KIND_TEAM_CATALOG | super::push_lease::KIND_PUSH_LEASE => {
            Ok(Scope::UsersWrite)
        }
        // NIP-AM: agent turn metrics are agent-authored global events (encrypted to owner).
        KIND_AGENT_TURN_METRIC => Ok(Scope::MessagesWrite),
        // Coding sessions: the operator-signed session origin, goal/name
        // revisions, authority-chain transitions, closure facts (44226–44230),
        // signed team transactions (44244), session policies (44245),
        // observations (44246) and handover records (44247),
        // the operator-authored commands (44220/44221), and the
        // provider-authored facts they produce (44222-44225). All are
        // durable, channel-scoped writes consumed by an out-of-relay
        // provider adapter — the relay validates and stores them, and
        // deliberately never executes them. See docs/nips/NIP-CSC.md,
        // NIP-CSL.md, NIP-CSPC.md, NIP-CST.md, NIP-CSG.md.
        KIND_CODING_SESSION_COMMAND
        | KIND_CODING_SESSION_LIFECYCLE_COMMAND
        | KIND_CODING_SESSION_GENESIS
        | KIND_CODING_SESSION_GOAL
        | KIND_CODING_SESSION_AUTHORITY_TRANSITION
        | KIND_CODING_SESSION_NAME
        | KIND_CODING_SESSION_CLOSURE
        | KIND_CODING_SESSION_PROVIDER_CATALOG
        | KIND_CODING_SESSION_METADATA
        | KIND_CODING_SESSION_LIFECYCLE_RECEIPT
        | KIND_CODING_SESSION_TRANSCRIPT
        | KIND_CODING_SESSION_TEAM_TRANSACTION
        | KIND_CODING_SESSION_POLICY
        | KIND_CODING_SESSION_OBSERVATION
        | KIND_CODING_SESSION_HANDOVER => Ok(Scope::MessagesWrite),
        // NIP-56 reports are ordinary member writes into the mod-only queue.
        // Ingest persists them to `moderation_reports` and suppresses public
        // storage/fanout; reports are signals, never enforcement triggers.
        KIND_REPORT | KIND_PRODUCT_FEEDBACK => Ok(Scope::MessagesWrite),
        // Community moderation commands are direct, mod-authz-gated writes.
        // Scope only proves the transport can submit message writes; the
        // command handler owns role/capability authorization.
        k if buzz_core::kind::is_moderation_command_kind(k) => Ok(Scope::MessagesWrite),
        // NIP-51 standard lists and NIP-65 relay list — user-owned global state,
        // same ownership shape as kind:3 (contacts) and kind:0 (profile).
        KIND_MUTE_LIST
        | KIND_PIN_LIST
        | KIND_NIP65_RELAY_LIST_METADATA
        | KIND_BOOKMARK_LIST
        | KIND_FOLLOW_SET
        | KIND_BOOKMARK_SET
        // NIP-30/NIP-51: per-user custom emoji set (30030) and emoji list (10030).
        // User-owned global state, keyed by (pubkey, kind[, d_tag]); the workspace
        // palette is the client-side union of every member's own set.
        | KIND_EMOJI_SET
        | KIND_EMOJI_LIST
        | KIND_AGENT_PROFILE => Ok(Scope::UsersWrite),
        KIND_DELETION
        | KIND_REACTION
        | KIND_GIFT_WRAP
        | KIND_STREAM_MESSAGE
        | KIND_STREAM_MESSAGE_V2
        | KIND_NIP29_DELETE_EVENT
        | KIND_STREAM_MESSAGE_EDIT
        | KIND_STREAM_MESSAGE_PINNED
        | KIND_STREAM_MESSAGE_BOOKMARKED
        | KIND_STREAM_MESSAGE_SCHEDULED
        | KIND_STREAM_REMINDER
        | KIND_STREAM_MESSAGE_DIFF
        | KIND_FORUM_POST
        | KIND_FORUM_VOTE
        | KIND_FORUM_COMMENT => Ok(Scope::MessagesWrite),
        KIND_NIP29_PUT_USER | KIND_NIP29_REMOVE_USER | KIND_NIP29_DELETE_GROUP => {
            Ok(Scope::AdminChannels)
        }
        // NIP-43: relay membership admin commands (9030–9032) + Buzz
        // workspace-profile command (9033).
        k if k == RELAY_ADMIN_ADD_MEMBER
            || k == RELAY_ADMIN_REMOVE_MEMBER
            || k == RELAY_ADMIN_CHANGE_ROLE
            || k == RELAY_ADMIN_SET_WORKSPACE_PROFILE =>
        {
            Ok(Scope::AdminUsers)
        }
        // NIP-IA: identity archive/unarchive requests (9035/9036).
        // Scope is intentionally UsersWrite, not AdminUsers: NIP-IA's self and
        // owner-of-agent paths are open to ordinary users (a user retiring their
        // own key, or an owner archiving their agent). Real authorization is the
        // consent-path check inside handle_identity_archive_event — the relay
        // verifies self / admin-role / owner-via-live-kind:0 there. This gate
        // only ensures the actor can write user-scoped state, which any
        // profile-publishing user already holds.
        KIND_IA_ARCHIVE_REQUEST | KIND_IA_UNARCHIVE_REQUEST => Ok(Scope::UsersWrite),
        KIND_NIP29_EDIT_METADATA => {
            // kind:9002 scope split: archived tag → AdminChannels, else ChannelsWrite
            let has_archived = event
                .tags
                .iter()
                .any(|t| t.kind().to_string() == "archived");
            if has_archived {
                Ok(Scope::AdminChannels)
            } else {
                Ok(Scope::ChannelsWrite)
            }
        }
        KIND_NIP29_CREATE_GROUP | KIND_CANVAS => Ok(Scope::ChannelsWrite),
        KIND_NIP29_JOIN_REQUEST | KIND_NIP29_LEAVE_REQUEST | KIND_NIP43_LEAVE_REQUEST => {
            Ok(Scope::ChannelsRead)
        }
        // Huddle lifecycle events + guidelines
        KIND_HUDDLE_STARTED
        | KIND_HUDDLE_PARTICIPANT_JOINED
        | KIND_HUDDLE_PARTICIPANT_LEFT
        | KIND_HUDDLE_ENDED
        | KIND_HUDDLE_GUIDELINES => Ok(Scope::ChannelsWrite),
        // NIP-34: Git repository events
        KIND_GIT_REPO_ANNOUNCEMENT | KIND_GIT_REPO_STATE => Ok(Scope::ReposWrite),
        // NIP-MP: a project is repository metadata — grouping repositories needs
        // the same scope as announcing them.
        KIND_PROJECT => Ok(Scope::ReposWrite),
        // NIP-MP membership ops ride the project's own scope; per-project
        // authorization (creator or roster owner) is enforced at ingest and
        // re-checked transactionally when the op is applied.
        buzz_core::kind::KIND_PROJECT_PUT_MEMBER | buzz_core::kind::KIND_PROJECT_REMOVE_MEMBER => {
            Ok(Scope::ReposWrite)
        }
        // NIP-PK: a pack source names a repository for a project, so it rides
        // the same scope as announcing one. Authority — founder or project
        // Owner — is a separate, closed gate at ingest
        // (`pack_source_write_admitted`); this scope alone admits nothing.
        buzz_core::kind::KIND_PROJECT_PACK_SOURCE => Ok(Scope::ReposWrite),
        // A repository rule record is repository metadata, so it rides the
        // same scope as announcing one. Authority — a founder of the
        // repository its `d` names — is a separate, closed gate at ingest
        // (`repo_protection_write_admitted`); this scope alone admits nothing.
        buzz_core::kind::KIND_GIT_REPO_PROTECTION => Ok(Scope::ReposWrite),
        // NIP-ST: a shared-terminal session announce is ordinary member
        // content, not repository metadata.
        buzz_core::kind::KIND_SHELL_SESSION => Ok(Scope::MessagesWrite),
        // NIP-MP Pulse: an explicit project coordination entry is authored
        // member content, not repository metadata. Per-project write
        // admission is enforced separately at ingest (`pulse_write_admitted`).
        //
        // The computed digest (39011) and the relay-signed summary (44242) are
        // deliberately absent from this match: the default arm below is what
        // keeps them unwritable by clients, and adding an arm for 39011 would
        // silently drop it into the generic parameterized-replaceable
        // store-and-replace path.
        buzz_core::kind::KIND_PULSE_ENTRY => Ok(Scope::MessagesWrite),
        KIND_GIT_PATCH
        | KIND_GIT_PULL_REQUEST
        | KIND_GIT_PR_UPDATE
        | KIND_GIT_ISSUE
        | KIND_GIT_STATUS_OPEN
        | KIND_GIT_STATUS_MERGED
        | KIND_GIT_STATUS_CLOSED
        | KIND_GIT_STATUS_DRAFT => Ok(Scope::MessagesWrite),
        // Command kinds — DM management, workflows, approvals
        KIND_DM_OPEN | KIND_DM_ADD_MEMBER | KIND_DM_HIDE => Ok(Scope::MessagesWrite),
        KIND_WORKFLOW_DEF | KIND_WORKFLOW_TRIGGER => Ok(Scope::MessagesWrite),
        KIND_APPROVAL_GRANT | KIND_APPROVAL_DENY => Ok(Scope::MessagesWrite),
        _ => Err("restricted: unknown event kind"),
    }
}

/// Write admission for a NIP-MP Pulse entry (kind:44240), as a pure decision
/// over already-resolved database facts.
///
/// `gate` is [`buzz_db::project_acl::get_project_gate_by_coordinate`]'s result
/// — `Some` only for a **private** project — and `project_exists` answers the
/// question that query cannot: whether any project head with the coordinate
/// exists in this community at all.
///
/// - Private project → `admits_write`, so an owner or collaborator publishes
///   and a read-only viewer does not. This deliberately diverges from the
///   shipped NIP-ST 30623 gate's read-shaped `can_access_project_contents`:
///   a Pulse entry is an authored claim about the project's work, not a
///   view of it.
/// - Public project → any community member may publish, as for any other
///   public project content.
/// - Unknown coordinate → refused. The `a` tag is a required singleton, and an
///   entry naming a project that does not exist is in nobody's hidden set, so
///   it would be shown to everyone as a coordination fact with no project
///   behind it.
fn pulse_write_admitted(
    gate: Option<&buzz_db::project_acl::ProjectGate>,
    project_exists: bool,
    author_pubkey: &[u8],
) -> Result<(), &'static str> {
    match gate {
        Some(gate) if !gate.admits_write(author_pubkey) => {
            Err("restricted: project write access required")
        }
        Some(_) => Ok(()),
        None if !project_exists => Err("restricted: unknown project coordinate"),
        None => Ok(()),
    }
}

/// Extract a channel UUID from the `"h"` NIP-29 group tag.
pub(crate) fn extract_channel_id(event: &Event) -> Option<Uuid> {
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

/// Result of resolving a reaction's target channel.
pub(crate) enum ReactionChannelResult {
    Channel(Uuid),
    NoChannel,
    NotFound,
    NoTarget,
    DbError(String),
}

/// Derive channel_id from the target event for NIP-25 reactions.
pub(crate) async fn derive_reaction_channel(
    community_id: CommunityId,
    db: &buzz_db::Db,
    event: &Event,
) -> ReactionChannelResult {
    let target_hex = match event.tags.iter().rev().find_map(|tag| {
        if tag.kind().to_string() == "e" {
            tag.content().and_then(|v| {
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
        Some(h) => h,
        None => return ReactionChannelResult::NoTarget,
    };

    let id_bytes = match hex::decode(&target_hex) {
        Ok(b) if b.len() == 32 => b,
        _ => return ReactionChannelResult::NoTarget,
    };

    match db.get_event_by_id(community_id, &id_bytes).await {
        Ok(Some(target)) => match target.channel_id {
            Some(ch_id) => ReactionChannelResult::Channel(ch_id),
            None => ReactionChannelResult::NoChannel,
        },
        Ok(None) => ReactionChannelResult::NotFound,
        Err(e) => ReactionChannelResult::DbError(e.to_string()),
    }
}

/// Kinds that are always global (`channel_id = NULL`).
///
/// If a client includes a stray `h` tag on these kinds, the ingest pipeline
/// sets `channel_id = None` — these events are never channel-scoped.
///
/// Note: the raw `h` tag remains on the stored event (Nostr events are signed,
/// so tags cannot be stripped without invalidating the signature). The read-path
/// filter matching in `filter.rs` treats explicit `h` tags as authoritative,
/// which means a stray `h` tag can still match `#h` queries. This is a known
/// limitation affecting all global-only kinds and should be addressed in the
/// filter layer as a follow-up.
pub(crate) fn is_global_only_kind(kind: u32) -> bool {
    matches!(
        kind,
        KIND_PROFILE
            | KIND_TEXT_NOTE
            | KIND_CONTACT_LIST
            | KIND_LONG_FORM
            | KIND_USER_STATUS
            | KIND_READ_STATE
            // NIP-51 standard lists + sets and NIP-65 relay list — user-owned global state.
            // Same as kind:3 (contacts): keyed by (pubkey, kind) or (pubkey, kind, d_tag),
            // never channel-scoped. A stray `h` tag must not channel-scope them.
            | KIND_MUTE_LIST
            | KIND_PIN_LIST
            | KIND_NIP65_RELAY_LIST_METADATA
            | KIND_BOOKMARK_LIST
            | KIND_FOLLOW_SET
            | KIND_BOOKMARK_SET
            // NIP-30 custom emoji set (30030) + emoji list (10030): user-owned,
            // keyed by (pubkey, kind[, d_tag]). A stray `h` tag must not channel-scope them.
            | KIND_EMOJI_SET
            | KIND_EMOJI_LIST
            // NIP-AE agent engrams are addressed by (pubkey_a, kind, d_tag); never channel-scoped.
            | KIND_AGENT_ENGRAM
            // NIP-ER event reminders are addressed by (pubkey, kind, d_tag); never channel-scoped.
            | KIND_EVENT_REMINDER
            // Agent profile (10100): user-owned replaceable, keyed by pubkey.
            | KIND_AGENT_PROFILE
            // NIP-AP: persona definitions (30175): owner-authored, keyed by (pubkey, kind, d_tag).
            | KIND_PERSONA
            // NIP-AP: team (30176) + managed-agent (30177) definitions and the
            // team-catalog projection (30178): owner-authored, keyed by
            // (pubkey, kind, d_tag). A stray `h` tag must not channel-scope them.
            | KIND_TEAM
            | KIND_MANAGED_AGENT
            | KIND_PRIVATE_MANAGED_AGENT
            | KIND_TEAM_CATALOG
            // NIP-34: git events use `a` tags (repo reference), not `h` tags (channel scope).
            // Parameterized replaceable kinds are keyed by (pubkey, kind, d_tag).
            | KIND_GIT_REPO_ANNOUNCEMENT
            | KIND_GIT_REPO_STATE
            | KIND_GIT_PATCH
            | KIND_GIT_PULL_REQUEST
            | KIND_GIT_PR_UPDATE
            | KIND_GIT_ISSUE
            | KIND_GIT_STATUS_OPEN
            | KIND_GIT_STATUS_MERGED
            | KIND_GIT_STATUS_CLOSED
            | KIND_GIT_STATUS_DRAFT
            // NIP-MP: projects are addressed by (pubkey, kind, d_tag). The
            // `buzz-channel` tag is a metadata reference, not a routing directive,
            // so a project's state is never channel-scoped.
            | KIND_PROJECT
            // NIP-PK: a pack source is addressed by (pubkey, kind, d_tag) where
            // the d_tag is the project coordinate. It belongs to a project, not
            // to a room, so a stray `h` tag must never channel-scope it.
            | buzz_core::kind::KIND_PROJECT_PACK_SOURCE
            // A repository rule record is addressed by (pubkey, kind, d_tag)
            // where the d_tag names a repository. It belongs to that
            // repository, not to a room, so a stray `h` tag must never
            // channel-scope it — the push gate reads it globally.
            | buzz_core::kind::KIND_GIT_REPO_PROTECTION
            // Community moderation commands (9040–9044): community-global
            // direct commands, same model as the NIP-43 9030-series. A stray
            // `h` tag must never channel-scope them (pinned contract —
            // handlers/moderation_commands.rs routing docs).
            | KIND_MODERATION_BAN
            | KIND_MODERATION_UNBAN
            | KIND_MODERATION_TIMEOUT
            | KIND_MODERATION_UNTIMEOUT
            | KIND_MODERATION_RESOLVE_REPORT
            // NIP-43: relay admin commands and leave requests are global — they
            // must never be channel-scoped, even if the event carries a stray `h` tag.
            | RELAY_ADMIN_ADD_MEMBER
            | RELAY_ADMIN_REMOVE_MEMBER
            | RELAY_ADMIN_CHANGE_ROLE
            | RELAY_ADMIN_SET_WORKSPACE_PROFILE
            | KIND_NIP43_LEAVE_REQUEST
            // NIP-IA: identity archive/unarchive requests drive relay-global
            // archive state (8002/8003/13535) and are audited as global request
            // events. A stray `h` tag must not channel-scope them.
            | KIND_IA_ARCHIVE_REQUEST
            | KIND_IA_UNARCHIVE_REQUEST
            // NIP-AM: agent turn metrics are owner-scoped global events.
            // Channel identity is encrypted inside the payload — no `h` tag.
            | KIND_AGENT_TURN_METRIC
            // NIP-PL leases are author-owned, addressable global state.
            | super::push_lease::KIND_PUSH_LEASE
    )
}

/// Kinds that require an `h` tag for channel scoping.
pub(crate) fn requires_h_channel_scope(kind: u32) -> bool {
    matches!(
        kind,
        KIND_STREAM_MESSAGE
            | KIND_STREAM_MESSAGE_V2
            | KIND_STREAM_MESSAGE_EDIT
            | KIND_STREAM_MESSAGE_PINNED
            | KIND_STREAM_MESSAGE_BOOKMARKED
            | KIND_STREAM_MESSAGE_SCHEDULED
            | KIND_STREAM_REMINDER
            | KIND_STREAM_MESSAGE_DIFF
            | KIND_CANVAS
            | KIND_FORUM_POST
            | KIND_FORUM_VOTE
            | KIND_FORUM_COMMENT
            // NIP-29 admin kinds (except CREATE_GROUP which creates the channel)
            | KIND_NIP29_PUT_USER
            | KIND_NIP29_REMOVE_USER
            | KIND_NIP29_EDIT_METADATA
            | KIND_NIP29_DELETE_EVENT
            | KIND_NIP29_DELETE_GROUP
            | KIND_NIP29_LEAVE_REQUEST
            // Huddle lifecycle events + guidelines
            | KIND_HUDDLE_STARTED
            | KIND_HUDDLE_PARTICIPANT_JOINED
            | KIND_HUDDLE_PARTICIPANT_LEFT
            | KIND_HUDDLE_ENDED
            | KIND_HUDDLE_GUIDELINES
            // Coding sessions live inside a channel: the channel's ACL is the
            // *only* thing standing between a session transcript and anyone on
            // the relay, and h-scoped events inherit private-project access
            // through `get_accessible_channel_ids`. Require `h` so a command or
            // a transcript item can never become a stray global event readable
            // by every authenticated pubkey. Genesis is scoped for a second
            // reason on top of that one: the channel is what makes a session
            // reference unique, so a genesis without an `h` tag would found an
            // umbrella in no particular room.
            | KIND_CODING_SESSION_COMMAND
            | KIND_CODING_SESSION_LIFECYCLE_COMMAND
            | KIND_CODING_SESSION_PROVIDER_CATALOG
            | KIND_CODING_SESSION_METADATA
            | KIND_CODING_SESSION_LIFECYCLE_RECEIPT
            | KIND_CODING_SESSION_TRANSCRIPT
            | KIND_CODING_SESSION_GENESIS
            | KIND_CODING_SESSION_GOAL
            | KIND_CODING_SESSION_AUTHORITY_TRANSITION
            | KIND_CODING_SESSION_NAME
            | KIND_CODING_SESSION_CLOSURE
            | KIND_CODING_SESSION_TEAM_TRANSACTION
            | KIND_CODING_SESSION_POLICY
            | KIND_CODING_SESSION_OBSERVATION
            | KIND_CODING_SESSION_HANDOVER
    )
}

/// Returns `true` for every persistent coding-session kind.
///
/// One predicate for the strict-membership gate and the tests, so a new
/// kind cannot be added to one gate and forgotten by another.
pub(crate) fn is_coding_session_kind(kind: u32) -> bool {
    matches!(
        kind,
        KIND_CODING_SESSION_COMMAND
            | KIND_CODING_SESSION_LIFECYCLE_COMMAND
            | KIND_CODING_SESSION_PROVIDER_CATALOG
            | KIND_CODING_SESSION_METADATA
            | KIND_CODING_SESSION_LIFECYCLE_RECEIPT
            | KIND_CODING_SESSION_TRANSCRIPT
            | KIND_CODING_SESSION_GENESIS
            | KIND_CODING_SESSION_GOAL
            | KIND_CODING_SESSION_AUTHORITY_TRANSITION
            | KIND_CODING_SESSION_NAME
            | KIND_CODING_SESSION_CLOSURE
            | KIND_CODING_SESSION_TEAM_TRANSACTION
            | KIND_CODING_SESSION_POLICY
            | KIND_CODING_SESSION_OBSERVATION
            | KIND_CODING_SESSION_HANDOVER
    )
}

/// Whether an event uses the strict coding-session membership/project-write
/// gate rather than conversational open-channel admission.
pub(crate) fn requires_strict_coding_session_membership(kind: u32) -> bool {
    kind == KIND_CODING_SESSION_LEASE || is_coding_session_kind(kind)
}

/// Maximum signed content size for each coding-session kind, in bytes.
///
/// 44220, 44221, and 44226–44230 are bounded by their payload
/// contracts in `buzz-core` instead (12 KiB of turn text, 16 KiB of signed
/// content, 1 KiB of genesis, 4 KiB of goal prose, 512 B of an authority
/// transition, 256 B of session-name text, and 512 B of closure JSON), so they are
/// absent here — and those bounds are the stricter ones,
/// since their envelope validators run *before* this table is consulted. The
/// four provider-authored kinds carry no envelope validator — the
/// relay does not parse a provider's facts — so a size cap is the whole of
/// their bound, and each one is sized to its job: a catalog enumerates every
/// provider and model an instance offers, a transcript item carries one
/// coalesced chunk of agent output, a receipt carries a status and a code.
fn coding_session_content_cap(kind: u32) -> Option<usize> {
    match kind {
        KIND_CODING_SESSION_PROVIDER_CATALOG => Some(256 * 1024),
        KIND_CODING_SESSION_METADATA => Some(32 * 1024),
        KIND_CODING_SESSION_LIFECYCLE_RECEIPT => Some(16 * 1024),
        KIND_CODING_SESSION_TRANSCRIPT => Some(32 * 1024),
        _ => None,
    }
}

/// Require active membership without the open-channel fallback used for normal
/// conversational writes, plus — for the steer kinds — NIP-CSAT authority.
///
/// [`check_channel_membership`] admits any authenticated pubkey in an *open*
/// channel. That is the right rule for talking, and the wrong rule here in both
/// directions: on the command side, permission to read a room is not authority
/// to steer an agent that runs shell commands against someone's checkout; on
/// the provider side, it is not authority to write transcripts and receipts
/// that consumers treat as the session's record of what happened.
///
/// Per-kind rules on top of the base membership gate:
///
/// - **44220 (turn command)**: when the channel holds genesis-rooted
///   sessions, the signer must be a founder or hold a live operator grant on
///   one — channel grain, because a command addresses a provider-minted
///   execution id the relay cannot map to a genesis; the session provider
///   enforces the exact per-session rule on top (verified acceptance
///   receipts). Standing alone suffices: an externally-granted operator
///   steers without being a channel/project member. Channels with no genesis
///   (legacy) keep the base rule, matching the provider's own treatment of
///   no-genesis records.
/// - **44227 (goal)**: resolved exactly via its `d` = sessionRef tag — the
///   named session's founder or operators only; an unclaimed label keeps the
///   base rule.
/// - **44221 `session.hire`**: resolved by the exact genesis id plus umbrella
///   named in the action — that founder, a live operator grant, or an active
///   accepted lead seat hiring a non-lead role. A hire asks a *host* to
///   spend a machine, a worktree and an identity, so unlike the other
///   lifecycle actions it cannot be left to the provider: the provider never
///   sees the hire, only the seated create the host publishes afterwards.
///   An umbrella no genesis in this channel claims is refused rather than
///   fallen back on — a hire is new with the relay that validates it, so
///   there is no legacy signer to keep working.
/// - **Everything else** (the other lifecycle actions, genesis, provider
///   kinds, 44228): the base rule. Lifecycle stop/resume founder-onlyness is
///   enforced by the provider (`operator_owns_session`), and 44228
///   owner-signing by the storage transaction.
pub(crate) async fn check_coding_session_membership(
    tenant: &TenantContext,
    state: &AppState,
    channel_id: Uuid,
    pubkey_bytes: &[u8],
    kind: u32,
    event: &nostr::Event,
) -> Result<(), String> {
    debug_assert!(requires_strict_coding_session_membership(kind));
    match kind {
        KIND_CODING_SESSION_COMMAND => {
            match state
                .db
                .channel_has_genesis_sessions(tenant.community(), channel_id)
                .await
            {
                Ok(true) => {
                    return match state
                        .session_steer_standing_cached(tenant.community(), channel_id, pubkey_bytes)
                        .await
                    {
                        Ok(true) => Ok(()),
                        Ok(false) => Err(
                            "restricted: only a session founder or a granted operator may steer"
                                .into(),
                        ),
                        Err(error) => Err(format!("error: database error: {error}")),
                    };
                }
                Ok(false) => {}
                Err(error) => return Err(format!("error: database error: {error}")),
            }
        }
        KIND_CODING_SESSION_GOAL => {
            let session_ref = event.tags.iter().find_map(|t| {
                let parts = t.as_slice();
                if parts.first().map(|s| s.as_str()) == Some("d") {
                    parts.get(1).map(|s| s.to_string())
                } else {
                    None
                }
            });
            if let Some(session_ref) = session_ref {
                match state
                    .db
                    .session_authority_by_ref(tenant.community(), channel_id, &session_ref)
                    .await
                {
                    Ok(Some(authority)) => {
                        return if authority.may_steer(pubkey_bytes) {
                            Ok(())
                        } else {
                            Err(
                                "restricted: only the session founder or a granted operator may \
                                 edit its goal"
                                    .into(),
                            )
                        };
                    }
                    Ok(None) => {}
                    Err(error) => return Err(format!("error: database error: {error}")),
                }
            }
        }
        KIND_CODING_SESSION_LIFECYCLE_COMMAND => {
            if let Some(claim) = hire_authority_claim(event) {
                return match state
                    .db
                    .session_authority_for_hire(
                        tenant.community(),
                        channel_id,
                        &claim.genesis_ref,
                        &claim.session_ref,
                    )
                    .await
                {
                    Ok(authority) => {
                        hire_authority_verdict(authority.as_ref(), pubkey_bytes, &claim.role)
                    }
                    Err(error) => Err(format!("error: database error: {error}")),
                };
            }
        }
        _ => {}
    }

    match state
        .is_member_cached(tenant.community(), channel_id, pubkey_bytes)
        .await
    {
        Ok(true) => return Ok(()),
        Ok(false) => {}
        Err(error) => return Err(format!("error: database error: {error}")),
    }
    // Session-transport channels: project membership IS transport access —
    // the project's owner and write-capable members (owner/collaborator) may
    // publish coding-session events without a channel_members row (the same
    // positive ACL grant the read paths use, minus the read-only viewer
    // tier: reading a session is not authority to steer one). `None` (not a
    // transport, or project unknown) keeps the strict-membership denial;
    // lookup errors fail closed.
    match state
        .channel_transport_gate_cached(tenant.community(), channel_id)
        .await
    {
        Ok(Some(gate)) if gate.admits_write(pubkey_bytes) => Ok(()),
        Ok(_) => coding_session_membership_verdict(false),
        Err(error) => Err(format!("error: database error: {error}")),
    }
}

/// The umbrella a 44221 asks to hire into, or `None` for every other action.
///
/// Reads the event's own content because the hire's umbrella rides in the
/// action, not in a tag — the 44221 envelope is exactly `h` / `csl-v` /
/// `csl-command` and cannot carry a fourth. Content that does not decode is
/// not a hire: the envelope validator refuses it a few steps later, and
/// guessing an umbrella out of malformed bytes would be the wrong kind of
/// helpful.
#[derive(Debug, Clone, PartialEq, Eq)]
struct HireAuthorityClaim {
    session_ref: String,
    genesis_ref: String,
    role: String,
}

fn hire_authority_claim(event: &Event) -> Option<HireAuthorityClaim> {
    let payload =
        buzz_core::coding_session_lifecycle_command::decode_coding_session_lifecycle_command(
            &event.content,
        )
        .ok()?;
    let buzz_core::coding_session_lifecycle_command::CodingSessionLifecycleAction::SessionHire {
        session_ref,
        genesis_ref,
        role,
        ..
    } = payload.action
    else {
        return None;
    };
    Some(HireAuthorityClaim {
        session_ref,
        genesis_ref,
        role,
    })
}

/// Decide a hire against the resolved authority of the umbrella it names.
///
/// Pure so the refusal wording is testable without a database: a viewer grant
/// is read access and is refused here, and an umbrella this channel holds no
/// genesis for is refused by name rather than admitted on channel membership.
fn hire_authority_verdict(
    authority: Option<&buzz_db::coding_session_acl::SessionAuthority>,
    pubkey_bytes: &[u8],
    role: &str,
) -> Result<(), String> {
    let Some(authority) = authority else {
        return Err(
            "restricted: no coding-session genesis in this channel claims that sessionRef, so \
             nothing here can authorize a hire into it"
                .into(),
        );
    };
    if authority.may_hire(pubkey_bytes, role) {
        Ok(())
    } else {
        Err(
            "restricted: only the session founder, a granted operator, or an active lead hiring a non-lead role may hire"
                .into(),
        )
    }
}

/// Refuse a kind only the relay may author.
///
/// A tiny function rather than an inline `if` so the gate is testable without a
/// database, a socket or an authenticated connection — the whole point of it is
/// that it fires *before* any of those matter.
///
/// **Signer-blind on purpose.** It refuses the kind, not the author, including
/// the relay's own key: the relay never submits its system messages through
/// ingest at all. `side_effects::emit_system_message` signs with the relay
/// keypair and writes through `db.insert_event` plus a direct pubsub fan-out,
/// so every acceptance receipt and deletion receipt this relay publishes takes
/// a path that does not pass here. A signer exemption would therefore buy
/// nothing and would hand anyone who ever obtained a relay key a way in
/// through the front door.
fn refuse_relay_only_kind(kind: u32) -> Result<(), IngestError> {
    if buzz_core::kind::is_relay_only_kind(kind) {
        return Err(IngestError::Rejected("restricted: relay-only kind".into()));
    }
    Ok(())
}

fn coding_session_membership_verdict(is_member: bool) -> Result<(), String> {
    if is_member {
        Ok(())
    } else {
        Err("restricted: coding-session events require channel membership".into())
    }
}

/// Check channel membership: member OR open-visibility channel.
///
/// `channel` is the request's already-fetched channel row, when the caller has
/// one (E1 within-request threading; correctness ruling §4.8). Callers without
/// a row pass `None` and the open-visibility fallback reads the DB directly.
///
/// Returns `Ok(())` if allowed, `Err(reason)` if denied.
pub(crate) async fn check_channel_membership(
    tenant: &TenantContext,
    state: &AppState,
    ch_id: Uuid,
    pubkey_bytes: &[u8],
    channel: Option<&buzz_db::channel::ChannelRecord>,
) -> Result<(), String> {
    match state
        .is_member_cached(tenant.community(), ch_id, pubkey_bytes)
        .await
    {
        Ok(true) => return Ok(()),
        Ok(false) => {}
        Err(e) => return Err(format!("error: database error: {e}")),
    }
    // Not a member — check if channel is open.
    let is_open = match channel {
        Some(ch) => ch.visibility == "open",
        None => state
            .db
            .get_channel(tenant.community(), ch_id)
            .await
            .map(|ch| ch.visibility == "open")
            .unwrap_or(false),
    };
    if !is_open {
        // Session-transport channels: the project's owner and write-capable
        // members (owner/collaborator) are admitted without a
        // channel_members row — project membership IS transport access,
        // resolved through the cached ACL projection; viewers read the
        // transport but never write into it. `None` (not a transport
        // channel, or project unknown) falls through to the members-only
        // denial; lookup errors fail closed the same way as the membership
        // lookup above.
        match state
            .channel_transport_gate_cached(tenant.community(), ch_id)
            .await
        {
            Ok(Some(gate)) if gate.admits_write(pubkey_bytes) => return Ok(()),
            Ok(_) => {}
            Err(e) => return Err(format!("error: database error: {e}")),
        }
        return Err("restricted: not a channel member".to_string());
    }
    // Open channel — but an open channel inside a private project must not
    // fall open to non-members of the project (NIP-MP Buzz access extension).
    // Explicit channel members were admitted above; here only the project's
    // owner and write-capable members may write — a project viewer reads the
    // channel but never posts. Fail closed on lookup errors.
    match state
        .channel_project_gate_cached(tenant.community(), ch_id)
        .await
    {
        Ok(None) => Ok(()),
        Ok(Some(gate)) if gate.admits_write(pubkey_bytes) => Ok(()),
        Ok(Some(_)) => Err("restricted: channel belongs to a private project".to_string()),
        Err(e) => Err(format!("error: database error: {e}")),
    }
}

fn check_token_channel_access(auth: &IngestAuth, channel_id: Uuid) -> Result<(), String> {
    if let Some(allowed) = auth.channel_ids() {
        if !allowed.contains(&channel_id) {
            return Err("restricted: token does not have access to this channel".to_string());
        }
    }
    Ok(())
}

/// Owned thread metadata for the DB insert.
pub(crate) struct ThreadMetadataOwned {
    pub event_id: Vec<u8>,
    pub event_created_at: chrono::DateTime<Utc>,
    pub channel_id: Uuid,
    pub parent_event_id: Vec<u8>,
    pub parent_event_created_at: chrono::DateTime<Utc>,
    pub root_event_id: Vec<u8>,
    pub root_event_created_at: chrono::DateTime<Utc>,
    pub depth: i32,
    pub broadcast: bool,
}

impl ThreadMetadataOwned {
    pub fn as_params(&self) -> buzz_db::event::ThreadMetadataParams<'_> {
        buzz_db::event::ThreadMetadataParams {
            event_id: &self.event_id,
            event_created_at: self.event_created_at,
            channel_id: self.channel_id,
            parent_event_id: Some(&self.parent_event_id),
            parent_event_created_at: Some(self.parent_event_created_at),
            root_event_id: Some(&self.root_event_id),
            root_event_created_at: Some(self.root_event_created_at),
            depth: self.depth,
            broadcast: self.broadcast,
        }
    }
}

/// Resolve NIP-10 thread ancestry from e-tags.
pub(crate) async fn resolve_nip10_thread_meta(
    community_id: CommunityId,
    event: &Event,
    channel_id: Uuid,
    state: &AppState,
) -> Result<Option<ThreadMetadataOwned>, String> {
    let mut root_hex: Option<String> = None;
    let mut reply_hex: Option<String> = None;

    for tag in event.tags.iter() {
        let parts = tag.as_slice();
        if parts.len() >= 4 && parts[0] == "e" {
            let hex_val = &parts[1];
            let marker = &parts[3];
            if hex_val.len() == 64 && hex_val.chars().all(|c| c.is_ascii_hexdigit()) {
                match marker.as_str() {
                    "root" => root_hex = Some(hex_val.to_string()),
                    "reply" => reply_hex = Some(hex_val.to_string()),
                    _ => {}
                }
            }
        }
    }

    if root_hex.is_none() && reply_hex.is_none() {
        return Ok(None);
    }

    let (root_hex, parent_hex) = match (root_hex, reply_hex) {
        (Some(r), Some(p)) => (r, p),
        (None, Some(p)) => (p.clone(), p),
        (Some(_), None) | (None, None) => return Ok(None),
    };

    let parent_bytes =
        hex::decode(&parent_hex).map_err(|_| "invalid parent event ID hex".to_string())?;

    let (parent_event_result, parent_meta_result) = tokio::join!(
        state.db.get_event_by_id(community_id, &parent_bytes),
        state
            .db
            .get_thread_metadata_by_event(community_id, &parent_bytes),
    );

    let parent_event = parent_event_result
        .map_err(|e| format!("db error looking up parent: {e}"))?
        .ok_or_else(|| "reply parent not found".to_string())?;

    match parent_event.channel_id {
        Some(parent_ch) if parent_ch != channel_id => {
            return Err("parent event belongs to a different channel".to_string());
        }
        None => return Err("parent event has no channel association".to_string()),
        _ => {}
    }

    let parent_created =
        chrono::DateTime::from_timestamp(parent_event.event.created_at.as_secs() as i64, 0)
            .unwrap_or_else(Utc::now);

    let client_root_bytes =
        hex::decode(&root_hex).map_err(|_| "invalid root event ID hex".to_string())?;

    let parent_meta =
        parent_meta_result.map_err(|e| format!("db error looking up thread metadata: {e}"))?;

    let (final_root_bytes, root_created, depth) = match parent_meta {
        Some(meta) => {
            let effective_root = meta.root_event_id.unwrap_or_else(|| parent_bytes.clone());
            if client_root_bytes != effective_root {
                return Err("root tag does not match thread ancestry".to_string());
            }
            let root_ts = if let Ok(Some(root_ev)) = state
                .db
                .get_event_by_id(community_id, &effective_root)
                .await
            {
                chrono::DateTime::from_timestamp(root_ev.event.created_at.as_secs() as i64, 0)
                    .unwrap_or(parent_created)
            } else {
                parent_created
            };
            let depth = meta.depth + 1;
            if depth > 100 {
                return Err("thread depth limit exceeded".to_string());
            }
            (effective_root, root_ts, depth)
        }
        None => {
            let parent_root = parent_event
                .event
                .tags
                .iter()
                .find_map(|t| {
                    let parts = t.as_slice();
                    if parts.len() >= 4 && parts[0] == "e" && parts[3] == "root" {
                        hex::decode(&parts[1]).ok().filter(|b| b.len() == 32)
                    } else {
                        None
                    }
                })
                .or_else(|| {
                    parent_event.event.tags.iter().find_map(|t| {
                        let parts = t.as_slice();
                        if parts.len() >= 4 && parts[0] == "e" && parts[3] == "reply" {
                            hex::decode(&parts[1]).ok().filter(|b| b.len() == 32)
                        } else {
                            None
                        }
                    })
                })
                .unwrap_or_else(|| parent_bytes.clone());

            if client_root_bytes != parent_root {
                return Err("root tag does not match thread ancestry".to_string());
            }
            let depth = if parent_root == parent_bytes { 1 } else { 2 };
            let root_created = if parent_root != parent_bytes {
                if let Ok(Some(root_ev)) =
                    state.db.get_event_by_id(community_id, &parent_root).await
                {
                    chrono::DateTime::from_timestamp(root_ev.event.created_at.as_secs() as i64, 0)
                        .unwrap_or(parent_created)
                } else {
                    parent_created
                }
            } else {
                parent_created
            };
            (parent_root, root_created, depth)
        }
    };

    let broadcast = event.tags.iter().any(|t| {
        let parts = t.as_slice();
        parts.len() >= 2 && parts[0] == "broadcast" && parts[1] == "1"
    });

    let event_created_at = chrono::DateTime::from_timestamp(event.created_at.as_secs() as i64, 0)
        .unwrap_or_else(Utc::now);

    Ok(Some(ThreadMetadataOwned {
        event_id: event.id.as_bytes().to_vec(),
        event_created_at,
        channel_id,
        parent_event_id: parent_bytes,
        parent_event_created_at: parent_created,
        root_event_id: final_root_bytes,
        root_event_created_at: root_created,
        depth,
        broadcast,
    }))
}

/// Count all `e` tags regardless of content validity.
fn count_e_tags(event: &Event) -> usize {
    event
        .tags
        .iter()
        .filter(|t| t.kind().to_string() == "e")
        .count()
}

/// Extract the effective author of a stored event (handles workflow-generated and
/// legacy relay-signed attributed events).
pub(crate) fn effective_message_author(event: &Event, relay_pubkey: &nostr::PublicKey) -> Vec<u8> {
    if event.pubkey == *relay_pubkey {
        // Workflow-generated or legacy relay-signed attributed event — real author
        // in "actor" or "p" tag.
        if let Some(hex) = event.tags.iter().find_map(|t| {
            if t.kind().to_string() == "actor" {
                t.content().map(|s| s.to_string())
            } else {
                None
            }
        }) {
            if let Ok(bytes) = hex::decode(&hex) {
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

/// Validate kind:40003 edit ownership — event.pubkey must match target's effective author,
/// or the actor must be the owning human of the agent that authored the target message.
async fn validate_edit_ownership(
    community_id: CommunityId,
    event: &Event,
    state: &AppState,
) -> Result<(), String> {
    let target_hex = event
        .tags
        .iter()
        .find_map(|t| {
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
        })
        .ok_or_else(|| "missing e tag for edit target".to_string())?;

    let target_bytes =
        hex::decode(&target_hex).map_err(|_| "invalid target event ID".to_string())?;
    let target_event = state
        .db
        .get_event_by_id(community_id, &target_bytes)
        .await
        .map_err(|e| format!("db error: {e}"))?
        .ok_or_else(|| "edit target event not found".to_string())?;

    // Verify target belongs to the same channel as the edit event.
    let edit_channel_id = extract_channel_id(event);
    match (edit_channel_id, target_event.channel_id) {
        (Some(edit_ch), Some(target_ch)) if edit_ch != target_ch => {
            return Err("target event belongs to a different channel".to_string());
        }
        (Some(_), None) => {
            return Err("target event has no channel".to_string());
        }
        _ => {} // Same channel or no channel context — OK
    }

    let author = effective_message_author(&target_event.event, &state.relay_keypair.public_key());
    let actor = event.pubkey.to_bytes().to_vec();
    if author == actor {
        // Author editing their own message: re-gate on membership/open visibility so that
        // a removed private-channel member cannot mutate old messages after access is revoked.
        if let Some(ch_id) = target_event.channel_id {
            let is_member = state
                .is_member_cached(community_id, ch_id, &actor)
                .await
                .map_err(|e| format!("db error checking membership: {e}"))?;
            if !is_member {
                let is_open = state
                    .db
                    .get_channel(community_id, ch_id)
                    .await
                    .map(|ch| ch.visibility == "open")
                    .unwrap_or(false);
                if !is_open {
                    return Err("restricted: not a channel member".to_string());
                }
            }
        }
    } else {
        // Allow the owning human to edit messages authored by their agent.
        let is_owner = state
            .db
            .is_agent_owner(community_id, &author, &actor)
            .await
            .map_err(|e| format!("db error checking agent ownership: {e}"))?;
        if !is_owner {
            return Err("must be event author to edit".to_string());
        }
    }
    Ok(())
}

/// Validate kind:45002 vote targets a forum post (45001) or comment (45003).
async fn validate_forum_vote_target(
    community_id: CommunityId,
    event: &Event,
    state: &AppState,
) -> Result<(), String> {
    let target_hex = event
        .tags
        .iter()
        .find_map(|t| {
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
        })
        .ok_or_else(|| "missing e tag for vote target".to_string())?;

    let target_bytes =
        hex::decode(&target_hex).map_err(|_| "invalid target event ID".to_string())?;
    let target_event = state
        .db
        .get_event_by_id(community_id, &target_bytes)
        .await
        .map_err(|e| format!("db error: {e}"))?
        .ok_or_else(|| "vote target event not found".to_string())?;

    let target_kind = event_kind_u32(&target_event.event);
    if target_kind != KIND_FORUM_POST && target_kind != KIND_FORUM_COMMENT {
        return Err("vote target must be a forum post or comment".to_string());
    }

    // Verify target belongs to the same channel as the vote event.
    let vote_channel_id = extract_channel_id(event);
    match (vote_channel_id, target_event.channel_id) {
        (Some(vote_ch), Some(target_ch)) if vote_ch != target_ch => {
            return Err("target event belongs to a different channel".to_string());
        }
        (Some(_), None) => {
            return Err("target event has no channel".to_string());
        }
        _ => {}
    }
    Ok(())
}

/// Validate kind:40008 diff event metadata tags.
fn validate_diff_event(event: &Event) -> Result<(), String> {
    // Content max 60KB
    if event.content.len() > 61_440 {
        return Err(format!(
            "diff content exceeds 60KB limit (got {} bytes)",
            event.content.len()
        ));
    }

    let mut has_repo = false;
    let mut has_commit = false;

    for tag in event.tags.iter() {
        let parts = tag.as_slice();
        if parts.len() < 2 {
            continue;
        }
        match parts[0].as_str() {
            "repo" => {
                let url = &parts[1];
                if !url.starts_with("http://") && !url.starts_with("https://") {
                    return Err("repo URL must be http or https".to_string());
                }
                has_repo = true;
            }
            "commit" => {
                let sha = &parts[1];
                if sha.len() < 7 || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
                    return Err("commit SHA must be at least 7 hex characters".to_string());
                }
                has_commit = true;
            }
            "parent-commit" => {
                let sha = &parts[1];
                if sha.len() < 7 || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
                    return Err("parent-commit SHA must be at least 7 hex characters".to_string());
                }
            }
            "branch" if (parts.len() < 3 || parts[1].is_empty() || parts[2].is_empty()) => {
                return Err("branch tag requires both source and target".to_string());
            }
            "pr" if parts[1].parse::<u32>().map(|n| n == 0).unwrap_or(true) => {
                return Err("pr number must be a positive integer".to_string());
            }
            _ => {}
        }
    }

    if !has_repo {
        return Err("diff event requires a repo tag".to_string());
    }
    if !has_commit {
        return Err("diff event requires a commit tag".to_string());
    }
    Ok(())
}

/// Validate the public envelope of a NIP-AE `kind:30174` event before it
/// reaches NIP-33 parameterized replacement.
///
/// We deliberately do this here (not in the d-tag length check downstream)
/// because a malformed envelope can otherwise *replace* a valid head in
/// storage and then be invisible to readers querying `#p`. The relay sees
/// no plaintext, but it can — and must — enforce the public tag shape:
///
/// * exactly one `d` tag with a 64-hex value (`d_tag = lower_hex(HMAC...)`),
/// * exactly one `p` tag with a 64-hex pubkey (the owner counterparty).
///
/// Content is opaque NIP-44 ciphertext; we do not parse it.
fn validate_engram_envelope(event: &Event) -> Result<(), String> {
    let mut d_tags: Vec<&str> = Vec::new();
    let mut p_tags: Vec<&str> = Vec::new();
    for tag in event.tags.iter() {
        let parts = tag.as_slice();
        if parts.len() < 2 {
            continue;
        }
        match parts[0].as_str() {
            "d" => d_tags.push(&parts[1]),
            "p" => p_tags.push(&parts[1]),
            _ => {}
        }
    }
    if d_tags.len() != 1 {
        return Err(format!(
            "agent-engram event must have exactly one `d` tag (got {})",
            d_tags.len()
        ));
    }
    if p_tags.len() != 1 {
        return Err(format!(
            "agent-engram event must have exactly one `p` tag (got {})",
            p_tags.len()
        ));
    }
    let d = d_tags[0];
    if d.len() != 64
        || !d
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err("agent-engram `d` tag must be 64 lowercase hex chars".to_string());
    }
    let p = p_tags[0];
    // Lowercase-only: readers query `#p` with `owner.to_hex()` (lowercase) and
    // Nostr tag matching is byte-exact. Accepting uppercase here would let a
    // submitter replace the lowercase head with an event that subsequent
    // lowercase-`#p` queries cannot see — silently bricking the slug.
    if p.len() != 64
        || !p
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err("agent-engram `p` tag must be 64 lowercase hex chars (pubkey)".to_string());
    }
    // Content must be a syntactically plausible NIP-44 v2 payload. We do not
    // (and cannot) verify the MAC at the relay, but we can reject obvious
    // garbage so a malformed event cannot supersede a valid head via NIP-33
    // replacement and then be silently discarded by readers.
    validate_engram_nip44_content(&event.content)?;
    Ok(())
}

/// Enforce the `shared`-tag shape shared by every kind in
/// [`buzz_core::kind::SHARED_GATED_KINDS`]: at most one `shared` tag, and if
/// present it must be exactly `["shared", "true"]`.
///
/// This ensures no ambiguous heads: either an event has no `shared` tag
/// (author-only) or exactly `["shared", "true"]` (community-readable). Any
/// other value (`"false"`, `"1"`, extra elements, duplicate tags) is rejected
/// at ingest so read-path helpers — including the SQL-level `tags @>
/// '[["shared","true"]]'` containment clause, which would otherwise match a
/// three-element superset — can treat stored events as unambiguously one or the
/// other.
///
/// `label` names the kind in error messages (e.g. `"persona event"`).
fn validate_shared_tag(event: &Event, label: &str) -> Result<(), String> {
    let mut shared_count = 0usize;
    for tag in event.tags.iter() {
        let parts = tag.as_slice();
        if !parts.is_empty() && parts[0].as_str() == "shared" {
            if parts.len() != 2 || parts[1].as_str() != "true" {
                return Err(format!(
                    "{label} `shared` tag must be exactly [\"shared\",\"true\"] (got {:?})",
                    parts.iter().map(|s| s.as_str()).collect::<Vec<_>>()
                ));
            }
            shared_count += 1;
        }
    }
    if shared_count > 1 {
        return Err(format!(
            "{label} must have at most one `shared` tag (got {shared_count})"
        ));
    }
    Ok(())
}

/// Return the event's single `d` tag value, requiring exactly one tag whose
/// value is non-empty, at most 64 characters, and free of Unicode control
/// characters and whitespace.
///
/// Without this check an empty `d` tag collapses every event of the kind into
/// the `(pubkey, kind, "")` slot — last-write-wins data loss. The character
/// bound keeps the value usable as a NIP-33 coordinate (`<kind>:<pubkey>:<d>`)
/// and as a log field: an embedded newline or tab would break line-oriented
/// consumers of both.
///
/// Tags are counted by their first element alone, so a valueless `["d"]`
/// counts. Skipping it would let `["d"]` plus `["d", "team-1"]` pass the
/// exactly-one rule, and a NIP-33 consumer that reads `["d"]` as an
/// empty-valued first `d` tag would then address the event at `""` where this
/// relay addresses it at `"team-1"`.
///
/// `label` names the kind in error messages (e.g. `"persona event"`).
fn single_bounded_d_tag<'a>(event: &'a Event, label: &str) -> Result<&'a str, String> {
    let d_tags: Vec<Option<&str>> = event
        .tags
        .iter()
        .filter_map(|tag| {
            let parts = tag.as_slice();
            (parts.first().map(|name| name.as_str()) == Some("d"))
                .then(|| parts.get(1).map(|value| value.as_str()))
        })
        .collect();
    if d_tags.len() != 1 {
        return Err(format!(
            "{label} must have exactly one `d` tag (got {})",
            d_tags.len()
        ));
    }
    let d = d_tags[0].unwrap_or_default();
    if d.is_empty() {
        return Err(format!("{label} `d` tag must not be empty"));
    }
    let char_count = d.chars().count();
    if char_count > 64 {
        return Err(format!(
            "{label} `d` tag too long ({char_count} chars, max 64)"
        ));
    }
    if d.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(format!(
            "{label} `d` tag must not contain control characters or whitespace"
        ));
    }
    Ok(d)
}

/// Validate the envelope of a kind:30175 persona event.
///
/// Enforces the shared-gated `shared`-tag shape ([`validate_shared_tag`]) plus
/// exactly one `d` tag matching the persona slug grammar
/// `^[a-z0-9][a-z0-9_-]{0,63}$`.
fn validate_persona_envelope(event: &Event) -> Result<(), String> {
    const LABEL: &str = "persona event";
    validate_shared_tag(event, LABEL)?;
    let d = single_bounded_d_tag(event, LABEL)?;
    // Slug grammar: ^[a-z0-9][a-z0-9_-]{0,63}$
    let bytes = d.as_bytes();
    if !bytes[0].is_ascii_lowercase() && !bytes[0].is_ascii_digit() {
        return Err(format!(
            "{LABEL} `d` tag must start with a lowercase letter or digit"
        ));
    }
    if !bytes[1..]
        .iter()
        .all(|&b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
    {
        return Err(format!(
            "{LABEL} `d` tag must match [a-z0-9_-] after the first character"
        ));
    }
    Ok(())
}

/// Validate the envelope of a kind:30178 team-catalog event.
///
/// Enforces the shared-gated `shared`-tag shape ([`validate_shared_tag`]) plus
/// exactly one non-empty, bounded `d` tag.
///
/// Deliberately NOT the persona slug grammar: a team's `d` tag is its stable
/// local id, which is either a UUID or a built-in identifier such as
/// `builtin-team:welcome` — the colon is not slug-legal, and rewriting ids to
/// fit would break NIP-33 addressing against the team's own kind:30176 head.
fn validate_team_catalog_envelope(event: &Event) -> Result<(), String> {
    const LABEL: &str = "team-catalog event";
    validate_shared_tag(event, LABEL)?;
    single_bounded_d_tag(event, LABEL)?;
    Ok(())
}

/// Maximum number of member `a` tags on a kind:30621 project.
///
/// Counted over raw tags, not distinct coordinates: a duplicate-heavy event
/// naming one coordinate thousands of times would otherwise be bounded only by
/// the relay frame limit (`config.rs`), so the cap must be checked before any
/// set proportional to the tag list is built.
const PROJECT_MEMBER_CAP: usize = 64;

/// Maximum byte length of a project `name` tag value.
const PROJECT_NAME_MAX_LEN: usize = 256;

/// Maximum byte length of a project `description` tag value.
const PROJECT_DESCRIPTION_MAX_LEN: usize = 2048;

/// Maximum byte length of `buzz-channel` and `buzz-visibility` tag values.
///
/// Both are opaque strings at the relay layer; the bound exists only so an
/// unbounded value cannot ride into storage on a tag ingest does not interpret.
const PROJECT_METADATA_TAG_MAX_LEN: usize = 256;

/// Metadata tags a project may carry at most once each.
///
/// Duplicates would make the effective value reader-dependent — one client
/// taking the first, another the last. For `buzz-access` a duplicate would be
/// worse than ambiguous display — it would make the *access level* itself
/// reader-dependent.
const PROJECT_SINGLETON_METADATA_TAGS: [&str; 7] = [
    "name",
    "description",
    "buzz-channel",
    "buzz-visibility",
    "buzz-access",
    "icon",
    "color",
];

/// Maximum number of invited-member `p` tags on a kind:30621 project.
///
/// Separate from [`PROJECT_MEMBER_CAP`] (which bounds `a` member coordinates):
/// invites bound who can *read* a private project, members bound what is *in*
/// it. Counted over raw tags before per-tag work, same rationale as
/// `member-cap`.
const PROJECT_INVITE_CAP: usize = 256;

/// The kind segments a project member coordinate may carry. NIP-MP proper
/// allows only repository *announcements* (30617) — notably not kind:30618
/// repository state. The Buzz container extension additionally accepts the
/// agent-surface kinds (30175 persona / 30176 team / 30177 managed agent) as
/// owner-curated members.
const PROJECT_MEMBER_KIND_SEGMENTS: [&str; 4] = ["30617", "30175", "30176", "30177"];
const _: () = assert!(KIND_GIT_REPO_ANNOUNCEMENT == 30617);
const _: () = assert!(KIND_PERSONA == 30175);
const _: () = assert!(KIND_TEAM == 30176);
const _: () = assert!(KIND_MANAGED_AGENT == 30177);

/// A validation failure from [`validate_project_envelope`] or
/// [`parse_project_member_coordinate`].
///
/// Carries the stable NIP-MP rule identifier alongside the human-readable
/// rejection message. The rule ID allows the fixture oracle and any future
/// cross-implementation conformance test to assert *which* rule fired, not just
/// that rejection occurred — an implementation cannot pass a reject fixture by
/// refusing for an unrelated reason.
///
/// The IDs match the `reject_rules` strings in `NIP-MP.fixtures.json`
/// exactly: `d-cardinality`, `d-empty`, `member-cap`, `member-tag-arity`,
/// `member-coordinate-malformed`, `member-duplicate`, `metadata-cardinality`,
/// `metadata-length`, plus the Buzz access-extension rules `access-value`,
/// `invite-cap`, `invite-tag-arity`, `invite-malformed`, `invite-duplicate`.
#[derive(Debug)]
struct ProjectRejection {
    /// Stable rule identifier matching the fixture file's `reject_rules` set.
    rule: &'static str,
    /// Human-readable explanation forwarded to the client's NOTICE/OK message.
    message: String,
}

impl std::fmt::Display for ProjectRejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.rule, self.message)
    }
}

impl ProjectRejection {
    fn new(rule: &'static str, message: impl Into<String>) -> Self {
        Self {
            rule,
            message: message.into(),
        }
    }
}

/// Validate the envelope of a kind:30621 NIP-MP project event.
///
/// Enforces the structural contract in `docs/nips/NIP-MP.md` — exactly one
/// non-empty `d` tag, at most [`PROJECT_MEMBER_CAP`] member `a` tags each
/// holding a canonical `30617:<lowercase-64-hex-owner>:<non-empty-d>`
/// coordinate with no duplicates, and bounded metadata.
///
/// Deliberately absent: any membership authorization. The signer may reference
/// any repository coordinate, including another owner's, because membership
/// grants nothing — push policy reads the repository's own kind:30617
/// (`api/git/policy.rs`) and never a project. Owner-only replacement comes free
/// from NIP-33 addressing.
///
/// Duplicates are rejected rather than deduped: a relay cannot rewrite tags
/// inside a signed event without invalidating its id and signature, so the
/// choice is reject or force every consumer to apply a first-wins rule.
fn validate_project_envelope(event: &Event) -> Result<(), ProjectRejection> {
    let mut d_tags: Vec<&str> = Vec::new();
    let mut members: Vec<&str> = Vec::new();
    let mut channels: Vec<&str> = Vec::new();
    let mut invites: Vec<&str> = Vec::new();
    let mut name: Option<&str> = None;
    let mut description: Option<&str> = None;
    let mut buzz_channel: Option<&str> = None;
    let mut buzz_visibility: Option<&str> = None;
    let mut buzz_access: Option<&str> = None;
    let mut icon: Option<&str> = None;
    let mut color: Option<&str> = None;
    let mut singleton_counts = [0usize; PROJECT_SINGLETON_METADATA_TAGS.len()];

    for tag in event.tags.iter() {
        let parts = tag.as_slice();
        let Some(tag_name) = parts.first().map(|s| s.as_str()) else {
            continue;
        };
        let value = parts.get(1).map(|s| s.as_str()).unwrap_or("");
        match tag_name {
            "d" => d_tags.push(value),
            "a" => members.push(value),
            // Buzz container extension: member channels/forums by channel id.
            "channel" => channels.push(value),
            // Buzz access extension: invited-member pubkeys on private projects.
            "p" => invites.push(value),
            _ => {
                if let Some(i) = PROJECT_SINGLETON_METADATA_TAGS
                    .iter()
                    .position(|k| *k == tag_name)
                {
                    singleton_counts[i] += 1;
                    match tag_name {
                        "name" => name = Some(value),
                        "description" => description = Some(value),
                        "buzz-channel" => buzz_channel = Some(value),
                        "buzz-visibility" => buzz_visibility = Some(value),
                        "buzz-access" => buzz_access = Some(value),
                        "icon" => icon = Some(value),
                        "color" => color = Some(value),
                        _ => {}
                    }
                }
            }
        }
    }

    // `d-cardinality` / `d-empty`: under NIP-33 a missing `d` is treated as
    // empty, which collapses every such project into the `(pubkey, 30621, "")`
    // slot where unrelated projects silently overwrite each other. Several `d`
    // tags make the address reader-dependent. Length is bounded by the generic
    // `D_TAG_MAX_LEN` check the ingest pipeline already applies.
    if d_tags.len() != 1 {
        return Err(ProjectRejection::new(
            "d-cardinality",
            format!(
                "project event must have exactly one `d` tag (got {})",
                d_tags.len()
            ),
        ));
    }
    if d_tags[0].is_empty() {
        return Err(ProjectRejection::new(
            "d-empty",
            "project event `d` tag must not be empty",
        ));
    }

    // `member-cap` before `member-coordinate-malformed` and `member-duplicate`:
    // refuse on count before doing per-tag work.
    if members.len() > PROJECT_MEMBER_CAP {
        return Err(ProjectRejection::new(
            "member-cap",
            format!(
                "project event must have at most {PROJECT_MEMBER_CAP} member `a` tags (got {})",
                members.len()
            ),
        ));
    }
    // `member-tag-arity`: every member `a` tag has exactly 2 or 3 elements per
    // NIP-01's `a` tag grammar. A one-element tag names no coordinate; a fourth
    // element has no defined meaning, and accepting it would let a writer park
    // unbounded unvalidated data in a position no consumer reads.
    for tag in event.tags.iter() {
        let parts = tag.as_slice();
        if parts.first().map(|s| s.as_str()) == Some("a") && !(2..=3).contains(&parts.len()) {
            return Err(ProjectRejection::new(
                "member-tag-arity",
                format!(
                    "project event member `a` tag must have exactly 2 or 3 elements (got {})",
                    parts.len()
                ),
            ));
        }
    }
    let mut seen = std::collections::HashSet::with_capacity(members.len());
    for member in &members {
        parse_project_member_coordinate(member)?;
        if !seen.insert(*member) {
            return Err(ProjectRejection::new(
                "member-duplicate",
                format!("project event has duplicate member coordinate {member:?}"),
            ));
        }
    }
    // Buzz container extension: `channel` member tags must be channel UUIDs.
    for channel in &channels {
        if Uuid::parse_str(channel).is_err() {
            return Err(ProjectRejection::new(
                "member-coordinate-malformed",
                format!("project event `channel` tag must be a UUID (got {channel:?})"),
            ));
        }
    }

    for (i, count) in singleton_counts.iter().enumerate() {
        if *count > 1 {
            return Err(ProjectRejection::new(
                "metadata-cardinality",
                format!(
                    "project event must have at most one `{}` tag (got {count})",
                    PROJECT_SINGLETON_METADATA_TAGS[i]
                ),
            ));
        }
    }
    if let Some(name) = name {
        if name.len() > PROJECT_NAME_MAX_LEN {
            return Err(ProjectRejection::new(
                "metadata-length",
                format!(
                    "project event `name` tag too long ({} bytes, max {PROJECT_NAME_MAX_LEN})",
                    name.len()
                ),
            ));
        }
    }
    if let Some(description) = description {
        if description.len() > PROJECT_DESCRIPTION_MAX_LEN {
            return Err(ProjectRejection::new(
                "metadata-length",
                format!(
                    "project event `description` tag too long ({} bytes, max {PROJECT_DESCRIPTION_MAX_LEN})",
                    description.len()
                ),
            ));
        }
    }
    if let Some(buzz_channel) = buzz_channel {
        if buzz_channel.len() > PROJECT_METADATA_TAG_MAX_LEN {
            return Err(ProjectRejection::new(
                "metadata-length",
                format!(
                    "project event `buzz-channel` tag too long ({} bytes, max {PROJECT_METADATA_TAG_MAX_LEN})",
                    buzz_channel.len()
                ),
            ));
        }
    }
    if let Some(buzz_visibility) = buzz_visibility {
        if buzz_visibility.len() > PROJECT_METADATA_TAG_MAX_LEN {
            return Err(ProjectRejection::new(
                "metadata-length",
                format!(
                    "project event `buzz-visibility` tag too long ({} bytes, max {PROJECT_METADATA_TAG_MAX_LEN})",
                    buzz_visibility.len()
                ),
            ));
        }
    }
    // `icon`/`color` stay opaque at the relay — display hints where an
    // unrecognized value harmlessly reads as unset — but their lengths are
    // bounded like the other pass-through metadata tags.
    if let Some(icon) = icon {
        if icon.len() > PROJECT_METADATA_TAG_MAX_LEN {
            return Err(ProjectRejection::new(
                "metadata-length",
                format!(
                    "project event `icon` tag too long ({} bytes, max {PROJECT_METADATA_TAG_MAX_LEN})",
                    icon.len()
                ),
            ));
        }
    }
    if let Some(color) = color {
        if color.len() > PROJECT_METADATA_TAG_MAX_LEN {
            return Err(ProjectRejection::new(
                "metadata-length",
                format!(
                    "project event `color` tag too long ({} bytes, max {PROJECT_METADATA_TAG_MAX_LEN})",
                    color.len()
                ),
            ));
        }
    }
    // `access-value`: unlike `buzz-visibility` (a display hint where an
    // unrecognized value harmlessly falls back to the default), `buzz-access`
    // is an access-control input — a typo that silently fell open to public
    // would be a privacy leak, so unknown values are rejected at ingest.
    if let Some(buzz_access) = buzz_access {
        if buzz_access != buzz_core::kind::PROJECT_ACCESS_PRIVATE
            && buzz_access != buzz_core::kind::PROJECT_ACCESS_PUBLIC
        {
            return Err(ProjectRejection::new(
                "access-value",
                format!(
                    "project event `buzz-access` tag must be \"private\" or \"public\" (got {buzz_access:?})"
                ),
            ));
        }
        // `access-general-forced-public`: the community's shared default
        // project can never be private — mirrors the client-side guard in
        // `publishProjectContainer`, so a hand-built head cannot hide the
        // one project everything falls back into.
        if buzz_access == buzz_core::kind::PROJECT_ACCESS_PRIVATE
            && d_tags[0] == buzz_core::kind::GENERAL_PROJECT_DTAG
        {
            return Err(ProjectRejection::new(
                "access-general-forced-public",
                "the \"general\" project is the community's shared default and cannot be private",
            ));
        }
    }
    // `invite-cap` before per-tag work, same rationale as `member-cap`.
    if invites.len() > PROJECT_INVITE_CAP {
        return Err(ProjectRejection::new(
            "invite-cap",
            format!(
                "project event must have at most {PROJECT_INVITE_CAP} invited-member `p` tags (got {})",
                invites.len()
            ),
        ));
    }
    // `invite-tag-arity`: `["p", pubkey]` plus NIP-01's optional relay hint,
    // plus an optional 4th role element (`["p", pubkey, hint, role]`,
    // matching the NIP-29 39002 grammar). `invite-role`: a present role must
    // be from the pinned vocabulary — a role typo must not silently grant or
    // deny; a role-less invite is a legacy collaborator.
    for tag in event.tags.iter() {
        let parts = tag.as_slice();
        if parts.first().map(|s| s.as_str()) != Some("p") {
            continue;
        }
        if !(2..=4).contains(&parts.len()) {
            return Err(ProjectRejection::new(
                "invite-tag-arity",
                format!(
                    "project event invited-member `p` tag must have 2 to 4 elements (got {})",
                    parts.len()
                ),
            ));
        }
        if let Some(role) = parts.get(3) {
            if !buzz_core::kind::is_valid_project_role(role.as_str()) {
                return Err(ProjectRejection::new(
                    "invite-role",
                    format!(
                        "project event invited-member role must be one of {:?} (got {role:?})",
                        buzz_core::kind::PROJECT_ROLES
                    ),
                ));
            }
        }
    }
    // `invite-malformed` / `invite-duplicate`: lowercase-only for the same
    // byte-exact-matching reason as member coordinates — the read gate compares
    // `p` values against the authenticated reader's lowercase hex pubkey.
    let mut seen_invites = std::collections::HashSet::with_capacity(invites.len());
    for invite in &invites {
        if invite.len() != 64
            || !invite
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err(ProjectRejection::new(
                "invite-malformed",
                format!(
                    "project event invited-member `p` tag must be a lowercase 64-hex pubkey (got {invite:?})"
                ),
            ));
        }
        if !seen_invites.insert(*invite) {
            return Err(ProjectRejection::new(
                "invite-duplicate",
                format!("project event has duplicate invited-member `p` tag {invite:?}"),
            ));
        }
    }
    Ok(())
}

/// Check that `coordinate` is a canonical repository-announcement address.
///
/// Splits on the first two colons only, matching how NIP-09 deletion handling
/// parses coordinates (`side_effects.rs`), so a repository whose `d` tag
/// contains a colon stays addressable and a project can never disagree with a
/// deletion about where the `d` value begins.
fn parse_project_member_coordinate(coordinate: &str) -> Result<(), ProjectRejection> {
    let malformed = || {
        ProjectRejection::new(
            "member-coordinate-malformed",
            format!(
                "project event member `a` tag must be \
                 `<30617|30175|30176|30177>:<lowercase-64-hex-owner>:<member-d>` (got {coordinate:?})"
            ),
        )
    };
    let mut segments = coordinate.splitn(3, ':');
    let (Some(kind), Some(owner), Some(repo_d)) =
        (segments.next(), segments.next(), segments.next())
    else {
        return Err(malformed());
    };
    if !PROJECT_MEMBER_KIND_SEGMENTS.contains(&kind) {
        return Err(malformed());
    }
    // Lowercase-only: `#a` filter matching is byte-exact, so an uppercase-owner
    // head would be invisible to the lowercase-coordinate queries readers issue.
    if owner.len() != 64
        || !owner
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err(malformed());
    }
    if repo_d.is_empty() {
        return Err(malformed());
    }
    Ok(())
}

/// Validate an optional `["project", "<coordinate>"]` tag on a channel-create
/// (kind:9007) or edit-metadata (kind:9002) event:
/// `30621:<64-hex-pubkey>:<project-d>`, where kind must be [`KIND_PROJECT`]
/// and the `d` segment follows the same envelope rules as
/// [`validate_project_envelope`] (non-empty, bounded, no control characters —
/// not the stricter client slug grammar, so a channel can reference any
/// project a relay would accept).
///
/// This checks shape only — soft enforcement, per VISION_PROJECTS.md: the
/// relay never verifies the referenced project event exists. `pub(crate)`
/// so `handlers::side_effects` can reuse it for the kind:9002 "move to
/// project" tag.
/// Cap on `p` targets per membership op — one op names a batch, not the
/// world; the roster's own cap (`PROJECT_INVITE_CAP` / DB
/// `PROJECT_ROSTER_CAP`) still bounds the total.
const PROJECT_MEMBER_OP_TARGET_CAP: usize = 64;

/// Validate a NIP-MP membership op (kind 9010 put-member / 9011
/// remove-member) BEFORE storage: envelope shape plus authorization (signer
/// is the project creator or a roster owner; the creator is never a target).
///
/// The authorization here gives the publisher a real rejection instead of an
/// OK over an op that then silently no-ops;
/// `buzz_db::project_acl::put_project_members` /
/// `remove_project_members` re-check inside their row-locked transaction,
/// which remains the authority under races.
pub(crate) async fn validate_project_member_op(
    tenant: &TenantContext,
    event: &nostr::Event,
    state: &AppState,
) -> Result<(), String> {
    let kind = event_kind_u32(event);
    let a_values: Vec<&str> = event
        .tags
        .iter()
        .filter_map(|t| {
            let parts = t.as_slice();
            if parts.first().map(|s| s.as_str()) == Some("a") {
                parts.get(1).map(|s| s.as_str())
            } else {
                None
            }
        })
        .collect();
    let [coordinate] = a_values.as_slice() else {
        return Err(format!(
            "membership op must have exactly one project `a` tag (got {})",
            a_values.len()
        ));
    };
    // Canonical coordinate only (lowercase hex owner): the ACL projection
    // joins on string equality, so a case-variant coordinate would dodge it.
    if buzz_core::kind::normalize_project_coordinate(coordinate).as_deref() != Some(*coordinate) {
        return Err(format!(
            "membership op `a` tag must be a canonical `30621:<lowercase-hex>:<dtag>` coordinate (got {coordinate:?})"
        ));
    }

    let p_tags: Vec<&[String]> = event
        .tags
        .iter()
        .map(|t| t.as_slice())
        .filter(|parts| parts.first().map(|s| s.as_str()) == Some("p"))
        .collect();
    if p_tags.is_empty() {
        return Err("membership op must name at least one `p` target".to_string());
    }
    if p_tags.len() > PROJECT_MEMBER_OP_TARGET_CAP {
        return Err(format!(
            "membership op must have at most {PROJECT_MEMBER_OP_TARGET_CAP} `p` targets (got {})",
            p_tags.len()
        ));
    }
    let mut seen = std::collections::HashSet::with_capacity(p_tags.len());
    for parts in &p_tags {
        let Some(target) = parts.get(1) else {
            return Err("membership op `p` tag is missing its pubkey".to_string());
        };
        if target.len() != 64
            || !target
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err(format!(
                "membership op `p` target must be a lowercase 64-hex pubkey (got {target:?})"
            ));
        }
        if !seen.insert(target.as_str()) {
            return Err(format!("membership op has duplicate `p` target {target:?}"));
        }
        if kind == buzz_core::kind::KIND_PROJECT_PUT_MEMBER {
            // Put targets carry an explicit role in element 4 (`["p", hex,
            // relay-hint, role]`). Required, not defaulted: an op is a
            // deliberate grant, and a missing role must not silently pick a
            // tier.
            if parts.len() != 4 {
                return Err(format!(
                    "put-member `p` tag must be [\"p\", pubkey, relay-hint, role] (got {} elements)",
                    parts.len()
                ));
            }
            let role = &parts[3];
            if !buzz_core::kind::is_valid_project_role(role.as_str()) {
                return Err(format!(
                    "put-member role must be one of {:?} (got {role:?})",
                    buzz_core::kind::PROJECT_ROLES
                ));
            }
        } else if !(2..=3).contains(&parts.len()) {
            return Err(format!(
                "remove-member `p` tag must have 2 or 3 elements (got {})",
                parts.len()
            ));
        }
    }

    // Authorization against the live roster. Fail closed on an unknown
    // project — an op cannot create one.
    let roster = state
        .db
        .get_project_roster(tenant.community(), coordinate)
        .await
        .map_err(|e| format!("database error: {e}"))?
        .ok_or_else(|| format!("membership op targets unknown project {coordinate:?}"))?;
    let actor = event.pubkey.to_bytes();
    let actor_is_owner = roster.owner == actor
        || roster
            .members
            .iter()
            .any(|(pk, role)| pk.as_slice() == actor && role.can_manage_roster());
    if !actor_is_owner {
        return Err("only a project owner may manage its members".to_string());
    }
    let creator_hex = hex::encode(&roster.owner);
    if seen.contains(creator_hex.as_str()) {
        return Err(
            "the project creator is the project's address and cannot be added, re-roled, or removed"
                .to_string(),
        );
    }
    Ok(())
}

pub(crate) fn validate_project_ref_tag(value: &str) -> Result<(), String> {
    let mut parts = value.splitn(3, ':');
    let (Some(kind_str), Some(pubkey), Some(slug)) = (parts.next(), parts.next(), parts.next())
    else {
        return Err(format!(
            "project tag must be `{KIND_PROJECT}:pubkey:slug` (got {value:?})"
        ));
    };
    let kind: u32 = kind_str
        .parse()
        .map_err(|_| format!("project tag kind must be an integer (got {value:?})"))?;
    if kind != KIND_PROJECT {
        return Err(format!(
            "project tag kind must be {KIND_PROJECT} (got {value:?})"
        ));
    }
    if pubkey.len() != 64 || !pubkey.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(format!(
            "project tag pubkey must be 64 hex chars (got {value:?})"
        ));
    }
    if slug.is_empty() {
        return Err(format!(
            "project tag slug must not be empty (got {value:?})"
        ));
    }
    if slug.chars().count() > 64 {
        return Err(format!(
            "project tag slug too long ({} chars, max 64) (got {value:?})",
            slug.chars().count()
        ));
    }
    if slug.chars().any(char::is_control) {
        return Err(format!(
            "project tag slug must not contain control characters (got {value:?})"
        ));
    }
    Ok(())
}

/// Validate a NIP-ST kind:30623 shared-terminal session announce and return
/// its project coordinate.
///
/// Fail-closed shape checks: exactly one `d` (bounded session id), exactly
/// one `a` (a valid `30621:<owner>:<dtag>` coordinate — the gate that hides
/// the announce inside a private project keys off it), a `status` of
/// `open`/`closed` (an unknown status is rejected, not defaulted), a bounded
/// `title`, a sane `dims`, and a small content budget.
pub(crate) fn validate_shell_session_envelope(event: &Event) -> Result<String, String> {
    if event.content.len() > 4096 {
        return Err(format!(
            "shell-session content too large ({} bytes, max 4096)",
            event.content.len()
        ));
    }

    let mut d_tags = Vec::new();
    let mut a_tags = Vec::new();
    let mut statuses = Vec::new();
    let mut titles = Vec::new();
    let mut dims = Vec::new();
    for tag in event.tags.iter() {
        let parts = tag.as_slice();
        if parts.len() < 2 {
            continue;
        }
        let value = parts[1].as_str().to_string();
        match parts[0].as_str() {
            "d" => d_tags.push(value),
            "a" => a_tags.push(value),
            "status" => statuses.push(value),
            "title" => titles.push(value),
            "dims" => dims.push(value),
            _ => {}
        }
    }

    let [session_id] = d_tags.as_slice() else {
        return Err(format!(
            "shell-session event must have exactly one `d` tag (got {})",
            d_tags.len()
        ));
    };
    if session_id.is_empty()
        || session_id.len() > 64
        || !session_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return Err("shell-session `d` tag must be a bounded session id".into());
    }

    let [coordinate] = a_tags.as_slice() else {
        return Err(format!(
            "shell-session event must have exactly one `a` tag (got {})",
            a_tags.len()
        ));
    };
    validate_project_ref_tag(coordinate)?;

    let [status] = statuses.as_slice() else {
        return Err(format!(
            "shell-session event must have exactly one `status` tag (got {})",
            statuses.len()
        ));
    };
    if status != "open" && status != "closed" {
        return Err(format!(
            "shell-session `status` must be `open` or `closed` (got {status:?})"
        ));
    }

    if titles.len() > 1 {
        return Err("shell-session event must have at most one `title` tag".into());
    }
    if let Some(title) = titles.first() {
        if title.chars().count() > 200 {
            return Err("shell-session `title` too long (max 200 chars)".into());
        }
    }

    if dims.len() > 1 {
        return Err("shell-session event must have at most one `dims` tag".into());
    }
    if let Some(dims) = dims.first() {
        let valid = dims.split_once('x').is_some_and(|(rows, cols)| {
            (1..=3).contains(&rows.len())
                && (1..=4).contains(&cols.len())
                && rows.bytes().all(|b| b.is_ascii_digit())
                && cols.bytes().all(|b| b.is_ascii_digit())
        });
        if !valid {
            return Err(format!(
                "shell-session `dims` must be `<rows>x<cols>` (got {dims:?})"
            ));
        }
    }

    // Roster `p` tags: `["p", <lowercase-64-hex>, <hint>, <role>]`, arity-4
    // required (a role-less roster entry must not silently pick a tier),
    // role from the pinned vocabulary, no duplicates, owner never listed
    // (the signature is their standing), cap 64.
    let owner_hex = event.pubkey.to_hex();
    let mut roster_seen = std::collections::HashSet::new();
    let mut roster_len = 0usize;
    for tag in event.tags.iter() {
        let parts = tag.as_slice();
        if parts.first().map(String::as_str) != Some("p") {
            continue;
        }
        if parts.len() != 4 {
            return Err(format!(
                "shell-session roster `p` tag must be [\"p\", pubkey, hint, role] (got {} elements)",
                parts.len()
            ));
        }
        let pubkey = parts[1].as_str();
        if pubkey.len() != 64
            || !pubkey
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err(format!(
                "shell-session roster pubkey must be lowercase 64-hex (got {pubkey:?})"
            ));
        }
        if pubkey.eq_ignore_ascii_case(&owner_hex) {
            return Err("shell-session roster must not list the owner".into());
        }
        if !roster_seen.insert(pubkey.to_string()) {
            return Err(format!(
                "shell-session roster has duplicate pubkey {pubkey:?}"
            ));
        }
        let role = parts[3].as_str();
        if !buzz_core::kind::is_valid_shell_role(role) {
            return Err(format!(
                "shell-session roster role must be one of {:?} (got {role:?})",
                buzz_core::kind::SHELL_ROLES
            ));
        }
        roster_len += 1;
    }
    if roster_len > 64 {
        return Err(format!(
            "shell-session roster must have at most 64 members (got {roster_len})"
        ));
    }

    Ok(coordinate.clone())
}

/// Validate the optional `["project", "<coordinate>"]` back-reference on a
/// repo announcement (kind:30617), returning the **normalized** coordinate
/// (`30621:<lowercase-hex>:<dtag>`) when present.
///
/// Stricter than [`validate_project_ref_tag`] in shape (exact two-element
/// arity, singleton) because this tag carries access-control weight — it is
/// what places the repo behind a private project's ACL (NIP-MP access
/// extension phase 2), so a malformed value is rejected rather than ignored:
/// an ignored tag would silently publish a repo its author believes is
/// private. Like the channel variant, this checks shape only; whether the
/// author may join a *private* project is the caller's DB check.
fn validate_repo_announcement_project_tag(event: &Event) -> Result<Option<String>, String> {
    let mut found: Option<String> = None;
    for tag in event.tags.iter() {
        let parts = tag.as_slice();
        if parts.first().map(String::as_str) != Some("project") {
            continue;
        }
        if found.is_some() {
            return Err("repo announcement must carry at most one project tag".to_string());
        }
        if parts.len() != 2 {
            return Err(format!(
                "project tag must be [\"project\", \"<coordinate>\"] ({} elements)",
                parts.len()
            ));
        }
        let value = parts[1].as_str();
        let normalized = buzz_core::kind::normalize_project_coordinate(value).ok_or_else(|| {
            format!("project tag must be `{KIND_PROJECT}:pubkey:slug` (got {value:?})")
        })?;
        found = Some(normalized);
    }
    Ok(found)
}

/// Validate that `content` is a syntactically plausible NIP-44 v2 ciphertext.
///
/// Checks:
/// - Non-empty.
/// - Standard base64 alphabet only (A-Z, a-z, 0-9, +, /, =), with padding only
///   at the end and total length a multiple of 4.
/// - Decoded length >= 99 bytes (1 version + 32 nonce + 32 MAC + minimum 34
///   bytes of length-prefixed padded ciphertext required by NIP-44 v2).
/// - First decoded byte is `0x02` (NIP-44 version 2).
///
/// This is an envelope sanity check, not full validation: the MAC and actual
/// decryption happen at the reader. The intent is to refuse obvious junk so a
/// malformed event cannot win NIP-33 replacement against a valid head and then
/// be silently skipped by `validate_and_decrypt`. Mirrors the validator in
/// `buzz-pair-relay::validate_nip44_content`.
fn validate_engram_nip44_content(content: &str) -> Result<(), String> {
    if content.is_empty() {
        return Err("agent-engram content must not be empty (NIP-44 ciphertext)".to_string());
    }
    let bytes = content.as_bytes();
    let len = bytes.len();
    if !len.is_multiple_of(4) {
        return Err("agent-engram content is not valid base64 (length)".to_string());
    }
    let mut pad_count = 0usize;
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'+' | b'/' => {
                if pad_count > 0 {
                    return Err("agent-engram content is not valid base64".to_string());
                }
            }
            b'=' => {
                if i < len - 2 {
                    return Err("agent-engram content is not valid base64".to_string());
                }
                pad_count += 1;
                if pad_count > 2 {
                    return Err("agent-engram content is not valid base64".to_string());
                }
            }
            _ => return Err("agent-engram content is not valid base64".to_string()),
        }
    }
    let decoded_len = (len / 4) * 3 - pad_count;
    if decoded_len < 99 {
        return Err("agent-engram content too short for NIP-44 v2".to_string());
    }
    let b64_val = |c: u8| -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    };
    let v0 =
        b64_val(bytes[0]).ok_or_else(|| "agent-engram content is not valid base64".to_string())?;
    let v1 =
        b64_val(bytes[1]).ok_or_else(|| "agent-engram content is not valid base64".to_string())?;
    let first_byte = (v0 << 2) | (v1 >> 4);
    if first_byte != 0x02 {
        return Err(
            "agent-engram content is not NIP-44 v2 (expected 0x02 version prefix)".to_string(),
        );
    }
    Ok(())
}

/// Validate the public envelope of a NIP-AM `kind:44200` event.
///
/// Enforces (without touching the encrypted payload):
/// - Exactly one `p` tag: 64 lowercase hex chars (the owner pubkey).
/// - Exactly one `agent` tag: 64 lowercase hex chars equal to `event.pubkey`.
/// - No `h` tag (channel identity belongs inside the encrypted payload).
/// - Content syntactically resembles NIP-44 v2 ciphertext (delegated to
///   `validate_engram_nip44_content`, which does the same length/base64/version check).
///
/// Ownership (`is_agent_owner`) is an async DB check performed separately in
/// `ingest_event_inner` after this synchronous envelope check.
fn validate_agent_turn_metric_envelope(event: &nostr::Event) -> Result<(), String> {
    let event_pubkey_hex = event.pubkey.to_hex();
    let mut p_tags: Vec<&str> = Vec::new();
    let mut agent_tags: Vec<&str> = Vec::new();
    let mut has_h_tag = false;

    for tag in event.tags.iter() {
        let parts = tag.as_slice();
        if parts.len() < 2 {
            continue;
        }
        match parts[0].as_str() {
            "p" => p_tags.push(&parts[1]),
            "agent" => agent_tags.push(&parts[1]),
            "h" => has_h_tag = true,
            _ => {}
        }
    }

    if has_h_tag {
        return Err(
            "agent-turn-metric event must not have an `h` tag (channel identity belongs inside the encrypted payload)".to_string(),
        );
    }

    if p_tags.len() != 1 {
        return Err(format!(
            "agent-turn-metric event must have exactly one `p` tag (got {})",
            p_tags.len()
        ));
    }
    let p = p_tags[0];
    if p.len() != 64
        || !p
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err("agent-turn-metric `p` tag must be 64 lowercase hex chars".to_string());
    }

    if agent_tags.len() != 1 {
        return Err(format!(
            "agent-turn-metric event must have exactly one `agent` tag (got {})",
            agent_tags.len()
        ));
    }
    let agent = agent_tags[0];
    if agent.len() != 64
        || !agent
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err("agent-turn-metric `agent` tag must be 64 lowercase hex chars".to_string());
    }
    if agent != event_pubkey_hex {
        return Err("agent-turn-metric `agent` tag must equal event pubkey".to_string());
    }

    // Content must look like a NIP-44 v2 ciphertext (length, base64, version prefix).
    validate_engram_nip44_content(&event.content)
        .map_err(|e| e.replace("agent-engram", "agent-turn-metric"))?;

    Ok(())
}

/// Validate the exact public envelope for a coding-session genesis (44226).
///
/// The signed event pubkey is the session's founder, and this envelope is the
/// only place that fact is ever established, so the shape is the narrowest of
/// any coding-session kind: three ordered two-field tags and a two-field
/// payload. The `csg-session` tag is re-derived from the decoded content rather
/// than trusted, so a genesis cannot be enforced under one umbrella reference
/// and read as another: the storage layer's uniqueness probe matches on the
/// tag, so a disagreement between the two would let a genesis be stored without
/// contending for the reference its content actually claims.
///
/// The tag is for that probe and for operator diagnostics. It is not a
/// consumer-facing founder lookup — authority resolves only through an explicit
/// genesis event id. See the module doc on
/// [`buzz_core::coding_session_genesis`].
///
/// # Not enforced here: one genesis per `sessionRef`
///
/// This validator is pure, like every other coding-session check, and so it
/// cannot reject a *second* genesis claiming a `sessionRef` some earlier event
/// already founded. That rule needs a lookup, and ingest validation runs
/// hundreds of lines and several round-trips before the insert it would need to
/// be atomic with (`ingest_event_inner` holds no transaction; `state.db` is a
/// pool handle). A `SELECT` here would therefore be a check-then-insert race
/// across relay processes — competing genesis events would both be stored, and
/// the property at stake is *which pubkey is the founder*. A dedupe that fails
/// under exactly the concurrency an attacker controls is worse than a known
/// gap, so this is deliberately left open rather than approximated. Closing it
/// means moving the check into the storage transaction in `buzz-db`, alongside
/// `replace_addressable_event`'s advisory-lock-then-probe-then-insert.
fn validate_coding_session_genesis_envelope(event: &Event) -> Result<(), String> {
    use buzz_core::coding_session_genesis::{
        decode_coding_session_genesis, CODING_SESSION_GENESIS_TAG_VERSION,
    };

    let payload = decode_coding_session_genesis(&event.content)?;
    let tags: Vec<&[String]> = event.tags.iter().map(|tag| tag.as_slice()).collect();
    if tags.len() != 3 || tags.iter().any(|parts| parts.len() != 2) {
        return Err("coding-session genesis requires exactly three two-field tags".into());
    }
    if tags[0][0] != "h" || tags[0][1].parse::<Uuid>().is_err() {
        return Err("coding-session genesis first tag must be a channel UUID h tag".into());
    }
    if tags[1][0] != "csg-v" || tags[1][1] != CODING_SESSION_GENESIS_TAG_VERSION {
        return Err("unsupported coding-session genesis tag version".into());
    }
    if tags[2][0] != "csg-session" || tags[2][1] != payload.session_ref {
        return Err("coding-session genesis csg-session does not match payload sessionRef".into());
    }
    Ok(())
}

/// The wire answer to a genesis that lost the race for its `sessionRef`.
///
/// Deliberately `accepted: false` and nothing else. A rejected duplicate gets
/// no acceptance receipt of any kind — it is a refusal, and naming the winner
/// is the whole of what the loser is owed, so the founder it must resolve to
/// instead is identifiable from the message without a second round trip.
fn coding_session_genesis_duplicate_result(
    event_id_hex: String,
    existing_event_id: &[u8],
) -> IngestResult {
    IngestResult {
        event_id: event_id_hex,
        accepted: false,
        message: format!(
            "duplicate: coding-session already founded by event {}",
            hex::encode(existing_event_id)
        ),
    }
}

/// The wire answer to a genesis whose adoption could not be verified — either
/// it carried no `adopts` reference over `sessionRef` history that requires
/// one, or the reference it gave did not check out (R15, R16).
///
/// `invalid:` rather than `duplicate:`: nothing was duplicated. The reference is
/// in use by history, not by a rival genesis, and telling those apart is the
/// difference between "adopt it explicitly" and "this session has no genesis
/// yet and may never get one from you".
fn coding_session_genesis_adoption_refusal_result(
    event_id_hex: String,
    refusal: &buzz_db::GenesisAdoptionRefusal,
) -> IngestResult {
    let message = match refusal {
        buzz_db::GenesisAdoptionRefusal::LegacyHistoryRequiresAdoption {
            existing_create_event_id,
        } => format!(
            "invalid: this coding session was founded before genesis (create {}) — \
             resubmit with an explicit adopts reference to that founding create and \
             its joining receipt",
            hex::encode(existing_create_event_id)
        ),
        buzz_db::GenesisAdoptionRefusal::ReferencedCreateNotFound => {
            "invalid: adopts.createEventId does not name a session.create this relay has stored"
                .to_string()
        }
        buzz_db::GenesisAdoptionRefusal::ReferencedReceiptNotFound => {
            "invalid: adopts.receiptEventId does not name a lifecycle receipt this relay has stored"
                .to_string()
        }
        buzz_db::GenesisAdoptionRefusal::ReceiptDoesNotJoinCreate => {
            "invalid: the referenced receipt does not genuinely join the referenced create"
                .to_string()
        }
        buzz_db::GenesisAdoptionRefusal::SessionRefMismatch => {
            "invalid: the referenced create claims a different sessionRef than this genesis"
                .to_string()
        }
        buzz_db::GenesisAdoptionRefusal::SignerMismatch { founder_pubkey } => format!(
            "invalid: this genesis's signer is not the referenced create's signer ({})",
            hex::encode(founder_pubkey)
        ),
        buzz_db::GenesisAdoptionRefusal::WrongChannel => {
            "invalid: the referenced create or receipt was not published in this genesis's channel"
                .to_string()
        }
        buzz_db::GenesisAdoptionRefusal::CommandIdAmbiguous { reason } => format!(
            "invalid: other session.create events share the founding commandId and disagree \
             with it ({reason})"
        ),
    };
    IngestResult {
        event_id: event_id_hex,
        accepted: false,
        message,
    }
}

/// Validate the exact public envelope for a coding-session authority
/// transition (44228).
///
/// Three ordered two-field tags — `h`, `csat-v`, `csat-genesis` — mirroring
/// genesis's own envelope shape. `csat-genesis` is re-derived from the
/// decoded content rather than trusted, for the same reason `csg-session` is:
/// the storage transaction's chain lookup matches on the tag, so a
/// disagreement between tag and content would let a transition be stored
/// under one genesis while filed under another.
///
/// # Not enforced here: chain linkage and owner standing
///
/// Like genesis's envelope validator, this is pure and cannot check whether
/// `prevAccepted`/`seq` actually extend the chain, or whether the signer is
/// the session's current owner — both require the current accepted head,
/// which only the storage transaction can answer atomically. See
/// `buzz_db::event::insert_coding_session_authority_transition_event`.
fn validate_coding_session_authority_transition_envelope(event: &Event) -> Result<(), String> {
    use buzz_core::coding_session_authority_transition::{
        decode_coding_session_authority_transition, CODING_SESSION_AUTHORITY_TRANSITION_TAG_VERSION,
    };

    let payload = decode_coding_session_authority_transition(&event.content)?;
    let tags: Vec<&[String]> = event.tags.iter().map(|tag| tag.as_slice()).collect();
    if tags.len() != 3 || tags.iter().any(|parts| parts.len() != 2) {
        return Err(
            "coding-session authority transition requires exactly three two-field tags".into(),
        );
    }
    if tags[0][0] != "h" || tags[0][1].parse::<Uuid>().is_err() {
        return Err(
            "coding-session authority transition first tag must be a channel UUID h tag".into(),
        );
    }
    if tags[1][0] != "csat-v" || tags[1][1] != CODING_SESSION_AUTHORITY_TRANSITION_TAG_VERSION {
        return Err("unsupported coding-session authority transition tag version".into());
    }
    if tags[2][0] != "csat-genesis" || tags[2][1] != payload.genesis_ref {
        return Err(
            "coding-session authority transition csat-genesis does not match payload genesisRef"
                .into(),
        );
    }
    Ok(())
}

/// Apply the action-specific authority rule for one already-resolved closure.
///
/// Closing is an owner act regardless of channel type. Reopening a project
/// session is deliberately broader: any member in the transport channel's
/// *current* project ACL may reopen the shared umbrella. A standalone channel
/// has no project authority source, so reopening remains founder-only.
fn coding_session_closure_authority_verdict(
    action: buzz_core::coding_session_closure::CodingSessionClosureAction,
    signer: &[u8],
    founder: &[u8],
    transport_gate: Option<&buzz_db::project_acl::ProjectGate>,
) -> Result<(), String> {
    use buzz_core::coding_session_closure::CodingSessionClosureAction;

    match action {
        // Archiving is a close that also files the session away: the same
        // owner act, so the same authority.
        CodingSessionClosureAction::Closed | CodingSessionClosureAction::Archived
            if signer == founder =>
        {
            Ok(())
        }
        CodingSessionClosureAction::Closed => {
            Err("restricted: only the session founder may close this session".into())
        }
        CodingSessionClosureAction::Archived => {
            Err("restricted: only the session founder may archive this session".into())
        }
        CodingSessionClosureAction::Open => match transport_gate {
            // Write tier: reopening changes shared session state, so project
            // viewers (read-only) don't qualify — owner/collaborator only.
            Some(gate) if gate.admits_write(signer) => Ok(()),
            Some(_) => {
                Err("restricted: only a current project member may reopen this session".into())
            }
            None if signer == founder => Ok(()),
            None => {
                Err("restricted: only the session founder may reopen a standalone session".into())
            }
        },
    }
}

/// Verify that a directly looked-up event is the exact genesis the closure
/// claims, and return its founder pubkey.
fn coding_session_closure_founder(
    genesis_event: &Event,
    genesis_channel: Option<Uuid>,
    closure_channel: Uuid,
    payload: &buzz_core::coding_session_closure::CodingSessionClosurePayload,
) -> Result<[u8; 32], String> {
    if event_kind_u32(genesis_event) != KIND_CODING_SESSION_GENESIS {
        return Err("genesisRef does not name a coding-session genesis".into());
    }
    if genesis_channel != Some(closure_channel) {
        return Err("the referenced genesis was not published in this closure's channel".into());
    }
    validate_coding_session_genesis_envelope(genesis_event)
        .map_err(|error| format!("referenced genesis is malformed: {error}"))?;
    let genesis_payload =
        buzz_core::coding_session_genesis::decode_coding_session_genesis(&genesis_event.content)
            .map_err(|error| format!("referenced genesis is malformed: {error}"))?;
    if genesis_payload.session_ref != payload.session_ref {
        return Err("closure sessionRef does not match the referenced genesis".into());
    }
    Ok(genesis_event.pubkey.to_bytes())
}

/// Resolve a closure's explicit genesis authority root and enforce the action
/// against current project membership.
async fn validate_coding_session_closure_authority(
    tenant: &TenantContext,
    state: &AppState,
    event: &Event,
    channel_id: Uuid,
    payload: &buzz_core::coding_session_closure::CodingSessionClosurePayload,
) -> Result<(), IngestError> {
    let genesis_id = hex::decode(&payload.genesis_ref).map_err(|_| {
        IngestError::Rejected("invalid: malformed coding-session closure genesisRef".into())
    })?;
    let genesis = state
        .db
        .get_event_by_id_including_deleted(tenant.community(), &genesis_id)
        .await
        .map_err(|error| {
            IngestError::Internal(format!(
                "error: looking up coding-session closure genesis: {error}"
            ))
        })?
        .ok_or_else(|| {
            IngestError::Rejected(
                "invalid: genesisRef does not name a coding-session genesis this relay has stored"
                    .into(),
            )
        })?;

    let founder =
        coding_session_closure_founder(&genesis.event, genesis.channel_id, channel_id, payload)
            .map_err(|error| IngestError::Rejected(format!("invalid: {error}")))?;

    let transport_gate = state
        .channel_transport_gate_cached(tenant.community(), channel_id)
        .await
        .map_err(|error| {
            IngestError::Internal(format!(
                "error: database error resolving closure project membership: {error}"
            ))
        })?;
    coding_session_closure_authority_verdict(
        payload.action,
        event.pubkey.as_bytes(),
        &founder,
        transport_gate.as_deref(),
    )
    .map_err(IngestError::AuthFailed)
}

/// The wire answer to an authority transition the chain refused.
///
/// Each variant of [`buzz_db::AuthorityTransitionRefusal`] gets a distinct,
/// specific message — the publisher needs to know *which* invariant it
/// missed (an unknown genesis is a very different bug from a stale head) to
/// have any hope of resubmitting correctly.
fn coding_session_authority_transition_refusal_result(
    event_id_hex: String,
    refusal: &buzz_db::AuthorityTransitionRefusal,
) -> IngestResult {
    let message = match refusal {
        buzz_db::AuthorityTransitionRefusal::GenesisNotFound => {
            "invalid: genesisRef does not name a coding-session genesis this relay has stored"
                .to_string()
        }
        buzz_db::AuthorityTransitionRefusal::WrongChannel => {
            "invalid: the referenced genesis was not published in this transition's channel, or \
             its stored envelope does not agree with itself"
                .to_string()
        }
        buzz_db::AuthorityTransitionRefusal::StaleHead {
            expected_prev_accepted,
        } => match expected_prev_accepted {
            Some(expected) => format!(
                "invalid: prevAccepted does not match the chain's current head (expected {})",
                hex::encode(expected)
            ),
            None => "invalid: prevAccepted must be null — this chain has no accepted \
                      transitions yet"
                .to_string(),
        },
        buzz_db::AuthorityTransitionRefusal::SeqMismatch { expected_seq } => {
            format!("invalid: seq does not extend the chain (expected {expected_seq})")
        }
        buzz_db::AuthorityTransitionRefusal::SignerNotOwner { owner_pubkey } => format!(
            "invalid: signer is not the session's current owner ({})",
            hex::encode(owner_pubkey)
        ),
        buzz_db::AuthorityTransitionRefusal::NoSuchGrant => {
            "invalid: revoke names a pubkey with no live grant on this session".to_string()
        }
        buzz_db::AuthorityTransitionRefusal::SignerNotAuthorized => {
            "invalid: signer lacks active authority to manage session seats".to_string()
        }
        buzz_db::AuthorityTransitionRefusal::LeadCannotManageLead => {
            "invalid: an active lead cannot grant or revoke lead authority".to_string()
        }
        buzz_db::AuthorityTransitionRefusal::SelfNomination => {
            "invalid: a seat grant cannot nominate its own signer".to_string()
        }
        buzz_db::AuthorityTransitionRefusal::NoSuchSeat => {
            "invalid: revoke-seat names an actor with no active seat".to_string()
        }
        buzz_db::AuthorityTransitionRefusal::SeatRoleMismatch => {
            "invalid: revoke-seat role does not match the actor's active seat".to_string()
        }
        buzz_db::AuthorityTransitionRefusal::ClaimantNotSigner => {
            "invalid: a takeover is a self-claim — granteePubkey must be the signer; use \
             transfer to hand a claim to somebody else"
                .to_string()
        }
        buzz_db::AuthorityTransitionRefusal::NoActiveClaim => {
            "invalid: this session has no claim to transfer — no takeover was accepted, or the \
             claim was voided when its claimant lost standing; publish a takeover instead"
                .to_string()
        }
    };
    IngestResult {
        event_id: event_id_hex,
        accepted: false,
        message,
    }
}

/// Validate the exact public envelope for a coding-session command (44220).
///
/// The signed event pubkey is the operator authority. The payload deliberately
/// carries no actor attribution and the relay does not execute this kind. The
/// `cs-target` tag is re-derived from the decoded payload rather than trusted,
/// so a command cannot be addressed to one session generation in its tag and
/// another in its content — the tag is what adapters route on.
fn validate_coding_session_command_envelope(event: &Event) -> Result<(), String> {
    use buzz_core::coding_session_command::{
        coding_session_target_key, CodingSessionCommandPayload, CODING_SESSION_COMMAND_TAG_VERSION,
    };

    let payload: CodingSessionCommandPayload = serde_json::from_str(&event.content)
        .map_err(|_| "malformed coding-session command payload".to_string())?;
    payload.validate()?;
    let expected_target = coding_session_target_key(&payload.target);
    let mut h_count = 0_u8;
    let mut version_count = 0_u8;
    let mut target_count = 0_u8;

    for tag in event.tags.iter() {
        let parts = tag.as_slice();
        if parts.len() != 2 {
            return Err("coding-session command tags must have exactly two fields".into());
        }
        match parts[0].as_str() {
            "h" => {
                h_count = h_count.saturating_add(1);
                if parts[1].parse::<Uuid>().is_err() {
                    return Err("coding-session command h tag must be a channel UUID".into());
                }
            }
            "cs-v" => {
                version_count = version_count.saturating_add(1);
                if parts[1] != CODING_SESSION_COMMAND_TAG_VERSION {
                    return Err("unsupported coding-session command tag version".into());
                }
            }
            "cs-target" => {
                target_count = target_count.saturating_add(1);
                if parts[1] != expected_target {
                    return Err(
                        "coding-session command cs-target does not match payload target".into(),
                    );
                }
            }
            _ => return Err("unsupported coding-session command tag".into()),
        }
    }

    if h_count != 1 || version_count != 1 || target_count != 1 {
        return Err(
            "coding-session command requires exactly one h, cs-v, and cs-target tag".into(),
        );
    }
    Ok(())
}

/// Validate the exact public envelope for a coding-session lifecycle command (44221).
///
/// The signed event pubkey is the operator authority. Tags are ordered and
/// payload-derived so adapters can reject ambiguous or substituted commands.
///
/// Fork amendment: the decoded payload's `projectRef` may be absent
/// (standalone session). No tag carries the project reference, so nothing here
/// changes shape — `decode_coding_session_lifecycle_command` owns the rule that
/// a *present* reference must be a `30621:` project coordinate.
fn validate_coding_session_lifecycle_command_envelope(event: &Event) -> Result<(), String> {
    use buzz_core::coding_session_lifecycle_command::{
        decode_coding_session_lifecycle_command, CODING_SESSION_LIFECYCLE_COMMAND_TAG_VERSION,
    };

    let payload = decode_coding_session_lifecycle_command(&event.content)?;
    let tags: Vec<&[String]> = event.tags.iter().map(|tag| tag.as_slice()).collect();
    if tags.len() != 3 || tags.iter().any(|parts| parts.len() != 2) {
        return Err(
            "coding-session lifecycle command requires exactly three two-field tags".into(),
        );
    }
    if tags[0][0] != "h" || tags[0][1].parse::<Uuid>().is_err() {
        return Err(
            "coding-session lifecycle command first tag must be a channel UUID h tag".into(),
        );
    }
    if tags[1][0] != "csl-v" || tags[1][1] != CODING_SESSION_LIFECYCLE_COMMAND_TAG_VERSION {
        return Err("unsupported coding-session lifecycle command tag version".into());
    }
    if tags[2][0] != "csl-command" || tags[2][1] != payload.command_id {
        return Err(
            "coding-session lifecycle command csl-command does not match payload commandId".into(),
        );
    }
    Ok(())
}

/// Parse a NIP-ER `not_before` tag value into a Unix timestamp.
///
/// The value MUST be a decimal integer string containing only ASCII digits, with
/// no sign, whitespace, decimal point, or leading zero (except the literal `"0"`),
/// and MUST be in the range 0..=9007199254740991 (`Number.MAX_SAFE_INTEGER`, the
/// interoperable JSON integer bound the spec mandates). Parsing is exact integer
/// parsing — never lossy floating-point — so values that overflow are malformed.
fn validate_not_before(tag_value: &str) -> Result<u64, &'static str> {
    const MAX_NOT_BEFORE: u64 = 9_007_199_254_740_991;

    if tag_value.is_empty() || !tag_value.bytes().all(|b| b.is_ascii_digit()) {
        return Err("malformed not_before");
    }
    // Reject leading zeros (e.g. "007") so each timestamp has one canonical form.
    // "0" itself is the only value allowed to begin with '0'.
    if tag_value.len() > 1 && tag_value.starts_with('0') {
        return Err("malformed not_before");
    }
    // Exact integer parse — `u64::from_str` rejects overflow rather than rounding,
    // so values that would lose precision as f64 are caught before the range check.
    let value: u64 = tag_value.parse().map_err(|_| "malformed not_before")?;
    if value > MAX_NOT_BEFORE {
        return Err("malformed not_before");
    }
    Ok(value)
}

/// Validate the public tag envelope of a NIP-ER `kind:30300` event before it
/// reaches NIP-33 parameterized replacement.
///
/// The relay never decrypts the reminder; it only enforces the public schedule
/// tags. A reminder carries at most one `not_before` (omitted on terminal
/// states), and — when both `not_before` and an optional NIP-40 `expiration`
/// are present — `expiration` MUST be strictly after `not_before` (an
/// `expiration <= not_before` window would expire the reminder before it ever
/// became due).
fn validate_event_reminder(event: &Event) -> Result<(), &'static str> {
    let mut not_before: Option<u64> = None;
    let mut expiration: Option<&str> = None;
    let mut d_count = 0u8;
    let mut d_empty = false;

    for tag in event.tags.iter() {
        let parts = tag.as_slice();
        if parts.len() < 2 {
            continue;
        }
        match parts[0].as_str() {
            "not_before" => {
                // Spec (NIP-ER line 60) collapses invalid and duplicate
                // `not_before` into one wire string clients may match on.
                if not_before.is_some() {
                    return Err("malformed not_before");
                }
                not_before = Some(validate_not_before(&parts[1])?);
            }
            "expiration" => expiration = Some(&parts[1]),
            "d" => {
                d_count = d_count.saturating_add(1);
                if parts[1].is_empty() {
                    d_empty = true;
                }
            }
            _ => {}
        }
    }

    // d-tag: must have exactly one, non-empty
    if d_count == 0 {
        return Err("missing d tag");
    }
    if d_count > 1 {
        return Err("duplicate d tag");
    }
    if d_empty {
        return Err("empty d tag");
    }

    // `not_before` is optional — terminal states (done/cancelled) and bookmarks
    // omit it. The ordering check only applies when both are present.
    if let Some(nb) = not_before {
        // Reject reminders scheduled beyond the configured horizon. The same
        // SPROUT_MAX_NOT_BEFORE_DELTA env var is advertised in NIP-11.
        let max_delta: u64 = std::env::var("SPROUT_MAX_NOT_BEFORE_DELTA")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(31_536_000); // 1 year default
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if nb > now + max_delta {
            return Err("not_before too far in future");
        }

        if let Some(exp) = expiration {
            if let Ok(exp) = exp.parse::<u64>() {
                if exp <= nb {
                    return Err("expiration before not_before");
                }
            }
        }
    }

    Ok(())
}

/// Resolve the `author_type` metric label (`"agent"` / `"human"`) for an
/// event author, from `users.agent_owner_pubkey IS NOT NULL` via a
/// per-community cache. Metric-labeling only — never used for authorization.
/// Unknown pubkeys and lookup errors count as "human" (the label must not
/// add a failure path to ingest).
async fn author_type_label(
    state: &Arc<AppState>,
    tenant: &TenantContext,
    author_pubkey_bytes: Vec<u8>,
) -> &'static str {
    let key = (tenant.community(), author_pubkey_bytes);
    let cached = state.author_type_cache.get(&key);
    let is_agent = match cached {
        Some(v) => v,
        None => {
            let v = match state.db.get_agent_channel_policy(key.0, &key.1).await {
                Ok(Some((_, owner))) => owner.is_some(),
                Ok(None) | Err(_) => false,
            };
            state.author_type_cache.insert(key, v);
            v
        }
    };
    if is_agent {
        "agent"
    } else {
        "human"
    }
}

/// Ingest a signed Nostr event through the full validation pipeline.
///
/// Shared by WebSocket and HTTP transports. The caller constructs [`IngestAuth`]
/// from their transport-specific auth mechanism and maps the result to their
/// transport-specific response format.
///
/// Builds a [`crate::conformance::EmitGuard`] around the actual ingest
/// logic so the trace seam has fail-closed coverage: any exit path that
/// doesn't emit a Write*/SanitizedError action will be caught by the
/// guard's Drop → `ImplBug` → CoverageBreach. The wrapper also maps
/// `IngestError` → SanitizedError in one place, sparing every individual
/// `return Err(...)` from having to emit explicitly. See
/// `crates/buzz-relay/src/conformance/mod.rs` and
/// `docs/spec/MultiTenantRelay.tla`.
pub async fn ingest_event(
    state: &Arc<AppState>,
    tenant: &TenantContext,
    event: Event,
    auth: IngestAuth,
) -> Result<IngestResult, IngestError> {
    // Captured before `event` moves into the inner fn: the stored-events
    // counter below is emitted at this shared seam so WebSocket and HTTP
    // transports are counted identically.
    let kind_label = super::event::bounded_kind_label(event_kind_u32(&event));
    // Classify the authenticated principal, not the event envelope signer:
    // NIP-59 gift wraps deliberately use an unrelated ephemeral pubkey.
    let author_pubkey_bytes = auth.principal_pubkey_bytes();

    let abstract_state = state_for_request(tenant, auth.pubkey());
    let (_guard, tracer) = EmitGuard::arm(
        state.tracer.clone(),
        abstract_state.clone(),
        "ingest_event_exited_without_trace",
    );

    let result = ingest_event_inner(state, &tracer, tenant, event, auth).await;

    // Fleet-wide stored counter: kind + author_type only, no community tag
    // (see the cardinality rationale on buzz_events_received_total —
    // author_type is a 2-value label so it merely doubles the kind series).
    // Emitted here rather than per-transport so HTTP bridge ingests count too.
    if let Ok(r) = &result {
        if r.accepted {
            let author_type = author_type_label(state, tenant, author_pubkey_bytes).await;
            metrics::counter!(
                "buzz_events_stored_total",
                "kind" => kind_label,
                "author_type" => author_type
            )
            .increment(1);
        }
    }

    // Map terminal error variants onto the closed SanitizedReason
    // alphabet (spec line 778). The inner fn's success path emits
    // WriteInsert/WriteInsertGlobal/WriteDuplicate explicitly at its
    // dispatch points — so on Ok we don't emit here.
    if let Err(err) = &result {
        let reason = conf::sanitized_reason_for(err);
        emit(
            &tracer,
            TraceAction::SanitizedError { reason },
            abstract_state.clone(),
        );
    }

    // _guard drops here. If `tracer` received no records during the
    // request (a panic before the first emit, or a future new exit
    // path that forgets to emit), Drop records an ImplBug step on
    // the underlying tracer — the checker treats that as
    // CoverageBreach.
    result
}

async fn ingest_event_inner(
    state: &Arc<AppState>,
    tracer: &Arc<dyn buzz_conformance::Tracer>,
    tenant: &TenantContext,
    event: Event,
    auth: IngestAuth,
) -> Result<IngestResult, IngestError> {
    let event_id_hex = event.id.to_hex();
    let kind_u32 = event_kind_u32(&event);
    debug!(event_id = %event_id_hex, kind = kind_u32, "ingest_event");

    // Durable community write fence: persistent ingest is a DB write the
    // deletion engine cannot exclude via serving-write leases (those cover
    // external side effects only), so the shared WS/HTTP seam must refuse
    // writes once the community leaves the active lifecycle state. Row churn
    // inside the remaining race window is swept by the destructive DB stage.
    map_serving_fence_state(
        buzz_deletion::store(&state.db)
            .is_serving_active(tenant.community())
            .await,
    )?;

    if kind_u32 == KIND_AUTH {
        return Err(IngestError::Rejected(
            "invalid: AUTH events cannot be submitted".into(),
        ));
    }
    if kind_u32 == KIND_MEMBER_ADDED_NOTIFICATION || kind_u32 == KIND_MEMBER_REMOVED_NOTIFICATION {
        return Err(IngestError::Rejected(
            "invalid: membership notifications are relay-signed only".into(),
        ));
    }

    if auth.is_http() && websocket_only_ingest_kind(kind_u32) {
        return Err(IngestError::Rejected(format!(
            "invalid: kind {kind_u32} is only accepted via WebSocket"
        )));
    }

    refuse_relay_only_kind(kind_u32)?;

    // Share the event with the verify task via Arc instead of deep-cloning it
    // (tags + up to 256 KB of content). spawn_blocking only needs 'static, not
    // ownership; once it completes its Arc is dropped, so try_unwrap returns
    // the original event without ever having copied it.
    let event = std::sync::Arc::new(event);
    let event_for_verify = std::sync::Arc::clone(&event);
    let verify_result = tokio::task::spawn_blocking(move || verify_event(&event_for_verify)).await;
    match verify_result {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            return Err(IngestError::Rejected(format!("invalid: {e}")));
        }
        Err(e) => {
            error!("spawn_blocking panicked: {e}");
            return Err(IngestError::Internal(
                "error: internal verification error".into(),
            ));
        }
    }
    let event = std::sync::Arc::try_unwrap(event).unwrap_or_else(|arc| (*arc).clone());

    const MAX_TIMESTAMP_DRIFT_SECS: i64 = 900; // ±15 minutes
    let now = chrono::Utc::now().timestamp();
    let event_ts = event.created_at.as_secs() as i64;
    if (event_ts - now).abs() > MAX_TIMESTAMP_DRIFT_SECS {
        return Err(IngestError::Rejected(
            "invalid: event timestamp too far from server time".into(),
        ));
    }

    const MAX_EVENT_CONTENT_BYTES: usize = 256 * 1024; // 256 KB
    if event.content.len() > MAX_EVENT_CONTENT_BYTES {
        return Err(IngestError::Rejected(format!(
            "invalid: content exceeds maximum size of {} bytes (got {})",
            MAX_EVENT_CONTENT_BYTES,
            event.content.len()
        )));
    }

    let is_gift_wrap = kind_u32 == KIND_GIFT_WRAP;
    if event.pubkey != *auth.pubkey() && !is_gift_wrap {
        return Err(IngestError::AuthFailed(
            "invalid: event pubkey does not match authenticated identity".into(),
        ));
    }

    let required = match required_scope_for_kind(kind_u32, &event) {
        Ok(scope) => scope,
        Err(msg) => return Err(IngestError::Rejected(msg.into())),
    };
    // NIP-43: relay admin commands are global — channel-scoped tokens cannot
    // issue them even if the event has no `h` tag (is_global_only_kind strips
    // channel_id, but we still need to reject the token itself).
    if is_relay_admin_kind(kind_u32) && auth.channel_ids().is_some() {
        return Err(IngestError::AuthFailed(
            "restricted: relay admin commands require a global token, not a channel-scoped token"
                .into(),
        ));
    }
    // NIP-43: leave requests are also global — channel-scoped tokens cannot
    // issue them.
    if kind_u32 == KIND_NIP43_LEAVE_REQUEST && auth.channel_ids().is_some() {
        return Err(IngestError::AuthFailed(
            "restricted: leave requests require a global token".into(),
        ));
    }
    if !auth.scopes().contains(&required) {
        return Err(IngestError::AuthFailed(format!(
            "restricted: insufficient scope (need {})",
            required
        )));
    }

    // Command kinds are routed AFTER signature verification, timestamp check,
    // pubkey/auth match, and scope validation — never before.
    if buzz_core::kind::is_command_kind(kind_u32) {
        return super::command_executor::handle_command(tenant, state, event, auth).await;
    }

    // Product feedback is sidecarred directly into its private deployment table.
    // It never enters ordinary event storage or subscription fan-out.
    if kind_u32 == KIND_PRODUCT_FEEDBACK {
        super::product_feedback::handle(tenant, &event, state)
            .await
            .map_err(IngestError::Rejected)?;
        // Feedback is a host-resolved, channel-less write. Although its row is
        // private to operator tooling rather than ordinary event reads, this is
        // the matching modeled success action at the ingest isolation seam.
        emit_product_feedback_success(tracer, tenant, &event, &auth);
        return Ok(IngestResult {
            event_id: event_id_hex,
            accepted: true,
            message: String::new(),
        });
    }

    // NIP-56 reports are persisted only to the mod queue. They are not stored in
    // the public events table and never fan out to subscribers. Reports remain
    // available while timed out so users can signal abuse during a write-block.
    // A banned actor in the rare missed-disconnect window may also submit a
    // report; that is tolerated because reports are non-actioning signals and
    // remain visible only to moderators.
    if kind_u32 == KIND_REPORT {
        super::report::handle_report_event(tenant, &event, state)
            .await
            .map_err(IngestError::Rejected)?;
        return Ok(IngestResult {
            event_id: event_id_hex,
            accepted: true,
            message: String::new(),
        });
    }

    // Community moderation commands (9040–9044) are direct, community-global
    // mutations. They are never stored or fanned out as ordinary events; the
    // handler writes the durable audit/enforcement rows after its own capability
    // authorization. These commands are intentionally routed before the
    // timeout/write-block gate below so a timed-out admin can lift a timeout.
    // The handler independently checks the durable ban state before executing
    // any command, which also covers NIP-98 and missed live disconnects.
    if buzz_core::kind::is_moderation_command_kind(kind_u32) {
        super::moderation_commands::handle_moderation_command(tenant, state, &event)
            .await
            .map_err(IngestError::Rejected)?;
        return Ok(IngestResult {
            event_id: event_id_hex,
            accepted: true,
            message: String::new(),
        });
    }

    // Community ban / timeout write-block (COMMUNITY_MODERATION_PLAN.md §0
    // decision 4). A timeout is a write-block only — the connection stays open,
    // content writes are refused with `restricted: you are timed out until <ts>`
    // so the desktop can render a countdown. A ban is normally enforced at the
    // auth seam, but an already-authenticated connection never re-auths: if the
    // live-disconnect fan-out is missed (fire-and-forget publish, broadcast lag,
    // subscriber reconnect window), a banned member's open socket would keep
    // writing indefinitely. So the ban is re-checked here — this write-path gate
    // is the durable backstop the fan-out's best-effort delivery relies on.
    // Moderation commands enforce bans inside their handler and remain exempt
    // here only so timeouts do not disarm the tool used to lift them. Relay-admin
    // commands (9030–9033) are exempt for the same reason — a timed-out admin
    // must still be able to administer the roster — and likewise enforce the
    // durable ban inside `relay_admin::handle_relay_admin_event`. Any kind added
    // to this exemption owes the same handler-local ban check.
    //
    // Scope: this gate checks the *authoring* pubkey only, with no NIP-OA
    // owner→agent cascade. That cascade lives at the auth seam for bans, where
    // it is structural: an agent whose owner is banned can never authenticate,
    // so its socket never exists to reach ingest. Timeout has no auth-seam
    // presence (it is write-block-only), so an owner-timeout does not cascade to
    // the owner's agents — a deliberate Phase-1 asymmetry. `IngestAuth` does not
    // carry the self-proving auth tag, so resolving the owner here would mean
    // plumbing it through the whole transport boundary; the follow-up shape is
    // the restriction-state cache (see should-fix), which can fold in owner
    // resolution without a per-write DB round-trip.
    if !buzz_core::kind::is_moderation_command_kind(kind_u32) && !is_relay_admin_kind(kind_u32) {
        match state
            .db
            .moderation_restriction_state(tenant.community(), auth.pubkey().as_bytes())
            .await
        {
            Ok(r) => {
                if r.banned {
                    return Err(IngestError::AuthFailed(
                        "blocked: you are banned from this community".to_string(),
                    ));
                }
                if let Some(until) = r.muted_until {
                    if until > chrono::Utc::now() {
                        return Err(IngestError::AuthFailed(format!(
                            "restricted: you are timed out until {}",
                            until.timestamp()
                        )));
                    }
                }
            }
            Err(e) => {
                // Fail closed: a DB error must not let a banned/timed-out actor
                // write.
                return Err(IngestError::Internal(format!(
                    "error: internal error checking restriction state: {e}"
                )));
            }
        }
    }

    let mut channel_id = if kind_u32 == KIND_REACTION {
        match derive_reaction_channel(tenant.community(), &state.db, &event).await {
            ReactionChannelResult::Channel(ch_id) => Some(ch_id),
            ReactionChannelResult::NoChannel => None,
            ReactionChannelResult::NotFound => {
                return Err(IngestError::Rejected(
                    "invalid: reaction target event not found".into(),
                ));
            }
            ReactionChannelResult::NoTarget => {
                return Err(IngestError::Rejected(
                    "invalid: reaction must reference a target event via e tag".into(),
                ));
            }
            ReactionChannelResult::DbError(e) => {
                return Err(IngestError::Internal(format!(
                    "error: internal error looking up reaction target: {e}"
                )));
            }
        }
    } else if is_gift_wrap {
        None
    } else if kind_u32 == KIND_DELETION {
        // Standard deletion (kind:5): derive channel from the target event.
        // kind:5 events don't carry an h-tag, so we look up the target event
        // and use its channel_id. This ensures token-channel, membership, and
        // archived checks run against the correct channel.
        let target_hex = event.tags.iter().find_map(|t| {
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
        });
        match target_hex {
            Some(hex) => {
                let target_bytes = hex::decode(&hex).map_err(|_| {
                    IngestError::Rejected("invalid: malformed deletion target id".into())
                })?;
                match state
                    .db
                    .get_event_by_id(tenant.community(), &target_bytes)
                    .await
                {
                    Ok(Some(target)) => target.channel_id,
                    Ok(None) => None, // target not found — validate_standard_deletion will catch this
                    Err(e) => {
                        return Err(IngestError::Internal(format!(
                            "error: looking up deletion target: {e}"
                        )));
                    }
                }
            }
            None => None, // no e-tag — will be caught by single-target enforcement (step 12)
        }
    } else {
        extract_channel_id(&event)
    };

    if is_global_only_kind(kind_u32) {
        channel_id = None;
    }

    if requires_h_channel_scope(kind_u32) && channel_id.is_none() {
        return Err(IngestError::Rejected(
            "invalid: channel-scoped events must include an h tag".into(),
        ));
    }

    if let Some(ch_id) = channel_id {
        check_token_channel_access(&auth, ch_id).map_err(IngestError::AuthFailed)?;
    } else if auth.channel_ids().is_some() {
        // Channel-scoped tokens cannot publish global events — that would bypass
        // the token's channel restriction. This covers kind:1 (global text notes),
        // kind:3 (contact lists), kind:0 (profiles), and kind:9007 (create-group
        // without an h-tag, which would auto-assign a server UUID).
        return Err(IngestError::AuthFailed(
            "restricted: channel-scoped tokens cannot publish global events".into(),
        ));
    }

    let pubkey_bytes = auth.pubkey().to_bytes().to_vec();
    // E1 (§4.8): fetch the community-scoped channel row once per request and
    // thread it through the gates below (membership open-fallback, archived
    // check, join visibility) instead of re-SELECTing it at each. `None` when
    // the event is global or the channel doesn't exist yet (kind:9007 creates
    // it later in this request); each gate keeps its existing missing-row
    // behavior.
    let channel_row = match channel_id {
        Some(ch_id) => state.db.get_channel(tenant.community(), ch_id).await.ok(),
        None => None,
    };
    // E1 phase-2 (§4.8 phase-2 addendum): resolve the fan-out visibility once,
    // here, through the same `channel_visibility_cached` gate fan-out uses
    // (fence 2: cached `private` wins over the prefetched row; a `private`
    // read still populates the cache). The value travels to fan-out bundled
    // with the (community, channel) it was resolved under (fence 3). When the
    // row is missing (global event, kind:9007 pre-create) this is `None` and
    // fan-out performs its own fresh fail-closed lookup — `None` is never
    // "assume open" (fence 1).
    let threaded_visibility = match (channel_id, &channel_row) {
        (Some(ch_id), Some(row)) => state
            .channel_visibility_cached(tenant.community(), ch_id, Some(row))
            .await
            .ok()
            .map(|visibility| crate::state::ThreadedChannelVisibility {
                community_id: tenant.community(),
                channel_id: ch_id,
                visibility,
            }),
        _ => None,
    };
    if let Some(ch_id) = channel_id {
        // kind:9021 (join) doesn't require prior membership.
        // kind:9007 (create) — channel doesn't exist yet; creator becomes owner in step 16.
        // kind:40003/9002/9005/9008 — per-kind validators are the authority; they
        // individually enforce authorization and fail closed. Bypassing the generic
        // member/open gate here lets the owning human act on private agent channels
        // without being a member (OQ1 decision; see validate_edit_ownership /
        // validate_admin_event for per-kind enforcement).
        let skip_membership = kind_u32 == KIND_NIP29_JOIN_REQUEST
            || kind_u32 == KIND_NIP29_CREATE_GROUP
            || kind_u32 == KIND_STREAM_MESSAGE_EDIT
            || kind_u32 == KIND_NIP29_EDIT_METADATA
            || kind_u32 == KIND_NIP29_DELETE_EVENT
            || kind_u32 == KIND_NIP29_DELETE_GROUP;
        if !skip_membership {
            // Spec AuthCheck (line 794): emit the verdict at the actual
            // call site. claimed_community comes from the event's h tag
            // (recorded separately to bite M2 / M8 — claim or A-host
            // driving a B-channel verdict — at the checker). The verdict
            // basis is `tenant.community()` server-resolved, confirmed
            // at `check_channel_membership`'s `is_member_cached(tenant
            // .community(), …)` call (see crates/buzz-relay/src/handlers
            // /ingest.rs:424).
            let auth_result =
                check_channel_membership(tenant, state, ch_id, &pubkey_bytes, channel_row.as_ref())
                    .await;
            let claimed = claimed_community_from_event(&event);
            let verdict = if auth_result.is_ok() {
                Verdict::Allow
            } else {
                Verdict::Deny
            };
            emit(
                tracer,
                TraceAction::AuthCheck {
                    channel: channel_label(ch_id),
                    claimed_community: claimed,
                    verdict,
                },
                state_for_request(tenant, auth.pubkey()),
            );
            auth_result.map_err(IngestError::Rejected)?;
        }
        // Coding sessions take a strictly stronger gate than the one above:
        // active membership, with no open-channel fallback. Visibility is not
        // authority to steer a session, nor to author its record.
        if is_coding_session_kind(kind_u32) {
            check_coding_session_membership(tenant, state, ch_id, &pubkey_bytes, kind_u32, &event)
                .await
                .map_err(IngestError::Rejected)?;
        }
    }

    // Handled directly — these mutate relay_members and do NOT get stored.
    // The handler enforces the durable community ban itself: the write-path
    // gate above exempts relay-admin kinds so timed-out admins keep their
    // administrative capability, which leaves bans to the handler.
    if is_relay_admin_kind(event.kind.as_u16() as u32) {
        crate::handlers::relay_admin::handle_relay_admin_event(tenant, state, &event)
            .await
            .map_err(map_relay_admin_error)?;
        return Ok(IngestResult {
            event_id: event_id_hex,
            accepted: true,
            message: String::new(),
        });
    }

    // Handled directly — removes the sender from relay_members. NOT stored.
    if kind_u32 == KIND_NIP43_LEAVE_REQUEST {
        if !state.config.require_relay_membership {
            return Err(IngestError::Rejected(
                "invalid: relay membership is not enabled".into(),
            ));
        }

        // Freshness check: reject events outside ±120s of now (same as admin commands).
        {
            let event_ts = event.created_at.as_secs() as i64;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            if (event_ts - now).abs() > 120 {
                return Err(IngestError::Rejected(format!(
                    "invalid: leave request timestamp out of range (delta={}s, max ±120s)",
                    event_ts - now
                )));
            }
        }

        // NIP-43 spec: "This event MUST include a NIP-70 `-` tag."
        let has_protected_tag = event
            .tags
            .iter()
            .any(|t| t.as_slice().first().map(|s| s.as_str()) == Some("-"));
        if !has_protected_tag {
            return Err(IngestError::Rejected(
                "invalid: leave request must include NIP-70 protected event tag [\"-\"]".into(),
            ));
        }

        let sender_hex = event.pubkey.to_hex();

        // remove_relay_member handles both the NotFound and IsOwner cases atomically.
        let remove_result = state
            .db
            .remove_relay_member(tenant.community(), &sender_hex)
            .await
            .map_err(|e| IngestError::Internal(format!("database error: {e}")))?;

        match remove_result {
            buzz_db::relay_members::RemoveResult::Removed => {}
            buzz_db::relay_members::RemoveResult::NotFound => {
                return Err(IngestError::Rejected(
                    "invalid: you are not a relay member".into(),
                ));
            }
            buzz_db::relay_members::RemoveResult::IsOwner => {
                return Err(IngestError::Rejected(
                    "invalid: relay owner cannot leave".into(),
                ));
            }
            buzz_db::relay_members::RemoveResult::RoleMismatch => {
                // remove_relay_member (no role filter) never returns RoleMismatch —
                // this arm is unreachable but exhaustiveness requires it.
                return Err(IngestError::Internal(
                    "unexpected RoleMismatch from remove_relay_member".into(),
                ));
            }
        }

        // Publish NIP-43 announcements — fire-and-forget.
        if let Err(e) =
            crate::handlers::side_effects::publish_nip43_member_removed(tenant, state, &sender_hex)
                .await
        {
            warn!(error = %e, "failed to publish NIP-43 member removed event");
        }
        if let Err(e) =
            crate::handlers::side_effects::publish_nip43_membership_list(tenant, state).await
        {
            warn!(error = %e, "failed to publish NIP-43 membership list");
        }

        info!(pubkey = %sender_hex, "relay member left via NIP-43 leave request");

        return Ok(IngestResult {
            event_id: event_id_hex,
            accepted: true,
            message: "info: you have left this relay".into(),
        });
    }

    if crate::handlers::side_effects::is_admin_kind(kind_u32) {
        crate::handlers::side_effects::validate_admin_event(tenant, kind_u32, &event, state)
            .await
            .map_err(|e| IngestError::Rejected(format!("invalid: {e}")))?;
    }

    // NIP-MP membership ops: envelope + owner authorization before storage;
    // the DB op re-checks transactionally when the side effect applies.
    if buzz_core::kind::is_project_membership_kind(kind_u32) {
        // The relay-signed 39010 roster projection is never client-submitted.
        if kind_u32 == buzz_core::kind::KIND_PROJECT_MEMBERS
            && event.pubkey != state.relay_keypair.public_key()
        {
            return Err(IngestError::Rejected(
                "invalid: kind 39010 is a relay-signed projection and cannot be submitted".into(),
            ));
        }
        if kind_u32 != buzz_core::kind::KIND_PROJECT_MEMBERS {
            validate_project_member_op(tenant, &event, state)
                .await
                .map_err(|e| IngestError::Rejected(format!("invalid: {e}")))?;
        }
    }

    // Processed here (verify consent, mutate archived_identities, emit the
    // relay-signed 8002/8003 delta + 13535 snapshot), then — unlike the
    // NIP-43 admin commands above — the request itself falls through to normal
    // storage so the delta's `["e", request_id]` audit reference resolves.
    if is_identity_archive_request_kind(kind_u32) {
        crate::handlers::identity_archive::handle_identity_archive_event(tenant, state, &event)
            .await
            .map_err(|e| IngestError::Rejected(format!("invalid: {e}")))?;
    }

    // What the deletion turned out to be, kept for the single-target gate
    // further down. `None` for kind:9005, which never reaches this validator.
    let deletion_shape = if kind_u32 == KIND_DELETION {
        Some(
            crate::handlers::side_effects::validate_standard_deletion_event(tenant, &event, state)
                .await
                .map_err(|e| IngestError::Rejected(format!("invalid: {e}")))?,
        )
    } else {
        None
    };

    if channel_id.is_some() {
        // Allow kind:9002 with archived=false (unarchive operation)
        let is_unarchive = kind_u32 == KIND_NIP29_EDIT_METADATA
            && event.tags.iter().any(|t| {
                let parts = t.as_slice();
                parts.len() >= 2 && parts[0] == "archived" && parts[1] == "false"
            });

        if !is_unarchive {
            if let Some(channel) = &channel_row {
                if channel.archived_at.is_some() {
                    return Err(IngestError::Rejected("invalid: channel is archived".into()));
                }
            }
        }
    }

    // NIP-09: kind:5 may reference targets via `e` tag (regular events) OR
    // `a` tag (addressable/parameterized-replaceable events like kind:30620).
    //
    // One target, with exactly one exception. Deleting a coding session is a
    // single act over many events — the relay refuses a genesis or a closure
    // deleted on its own, and refuses a chain that leaves a live closure
    // behind, so a session goes whole or not at all and there is no shape of
    // it that fits in one `e` tag. `validate_standard_deletion_event` above
    // has already decided that this is such a deletion, that the chain is
    // complete, that the actor may perform it and that every target belongs
    // to the session named; the count is the only thing left to say, and for
    // that shape it has nothing to say. Both clients have always built this
    // event, and until now every real session was refused here with
    // "must reference exactly one target ... (got e=26, a=0)" — the feature's
    // own tests exercised the validator directly and never crossed ingest.
    let whole_session_deletion =
        deletion_shape == Some(crate::handlers::side_effects::DeletionShape::WholeCodingSession);
    if (kind_u32 == KIND_NIP29_DELETE_EVENT || kind_u32 == KIND_DELETION) && !whole_session_deletion
    {
        let e_count = count_e_tags(&event);
        let a_count = event
            .tags
            .iter()
            .filter(|t| t.kind().to_string() == "a")
            .count();
        if (e_count + a_count) != 1 {
            return Err(IngestError::Rejected(format!(
                "invalid: deletion events must reference exactly one target via e or a tag (got e={e_count}, a={a_count})"
            )));
        }
    }

    if kind_u32 == KIND_STREAM_MESSAGE_EDIT {
        validate_edit_ownership(tenant.community(), &event, state)
            .await
            .map_err(|e| IngestError::Rejected(format!("invalid: {e}")))?;
    }

    if kind_u32 == KIND_FORUM_VOTE {
        validate_forum_vote_target(tenant.community(), &event, state)
            .await
            .map_err(|e| IngestError::Rejected(format!("invalid: {e}")))?;
    }

    if kind_u32 == KIND_STREAM_MESSAGE_DIFF {
        validate_diff_event(&event).map_err(|e| IngestError::Rejected(format!("invalid: {e}")))?;
    }

    if kind_u32 == KIND_AGENT_ENGRAM {
        validate_engram_envelope(&event)
            .map_err(|e| IngestError::Rejected(format!("invalid: {e}")))?;
    }

    if kind_u32 == KIND_AGENT_TURN_METRIC {
        validate_agent_turn_metric_envelope(&event)
            .map_err(|e| IngestError::Rejected(format!("invalid: {e}")))?;

        // Ownership check: `p` tag must be the registered owner of `event.pubkey`.
        // Tag shape is already verified above; these extractions are infallible.
        let owner_hex = event
            .tags
            .iter()
            .find_map(|t| {
                let parts = t.as_slice();
                if parts.len() >= 2 && parts[0].as_str() == "p" {
                    Some(parts[1].as_str())
                } else {
                    None
                }
            })
            .expect("p tag present (validated above)");
        let agent_bytes = event.pubkey.to_bytes().to_vec();
        let owner_bytes = hex::decode(owner_hex).expect("hex validated above");
        let is_owner = state
            .db
            .is_agent_owner(tenant.community(), &agent_bytes, &owner_bytes)
            .await
            .map_err(|e| {
                IngestError::Internal(format!(
                    "error: db error checking agent-turn-metric ownership: {e}"
                ))
            })?;
        if !is_owner {
            return Err(IngestError::AuthFailed(
                "restricted: agent-turn-metric `p` tag must be the registered owner of this agent"
                    .into(),
            ));
        }
    }

    if kind_u32 == KIND_CODING_SESSION_COMMAND {
        validate_coding_session_command_envelope(&event)
            .map_err(|error| IngestError::Rejected(format!("invalid: {error}")))?;
    }

    if kind_u32 == KIND_CODING_SESSION_LIFECYCLE_COMMAND {
        validate_coding_session_lifecycle_command_envelope(&event)
            .map_err(|error| IngestError::Rejected(format!("invalid: {error}")))?;
    }

    if kind_u32 == KIND_CODING_SESSION_GENESIS {
        validate_coding_session_genesis_envelope(&event)
            .map_err(|error| IngestError::Rejected(format!("invalid: {error}")))?;
    }

    if kind_u32 == KIND_CODING_SESSION_GOAL {
        buzz_core::coding_session_goal::validate_coding_session_goal_envelope(&event)
            .map_err(|error| IngestError::Rejected(format!("invalid: {error}")))?;
    }

    if kind_u32 == KIND_CODING_SESSION_AUTHORITY_TRANSITION {
        validate_coding_session_authority_transition_envelope(&event)
            .map_err(|error| IngestError::Rejected(format!("invalid: {error}")))?;
    }

    if kind_u32 == KIND_CODING_SESSION_NAME {
        buzz_core::coding_session_name::validate_coding_session_name_envelope(&event)
            .map_err(|error| IngestError::Rejected(format!("invalid: {error}")))?;
    }

    if kind_u32 == KIND_CODING_SESSION_CLOSURE {
        let payload =
            buzz_core::coding_session_closure::validate_coding_session_closure_envelope(&event)
                .map_err(|error| IngestError::Rejected(format!("invalid: {error}")))?;
        let Some(closure_channel) = channel_id else {
            return Err(IngestError::Rejected(
                "invalid: coding-session closure requires a channel".into(),
            ));
        };
        validate_coding_session_closure_authority(tenant, state, &event, closure_channel, &payload)
            .await?;
    }

    if kind_u32 == KIND_CODING_SESSION_TEAM_TRANSACTION {
        buzz_core::coding_session_team_transaction::validate_coding_session_team_transaction_envelope(
            &event,
        )
        .map_err(|error| IngestError::Rejected(format!("invalid: {error}")))?;
    }

    // NIP-CSP: structure only. Schema, the four ordered tags, closed
    // vocabularies, bounds, and tag-to-content parity are self-contained in
    // one event, so the relay checks them. Whether the signer held the
    // standing to set this umbrella's policy is not: that is the consuming
    // fold's question against the accepted NIP-CSAT chain, exactly as it is
    // for the team transaction above. A relay that adjudicated policy
    // authority at ingest would be asserting standing it cannot verify.
    if kind_u32 == KIND_CODING_SESSION_POLICY {
        buzz_core::coding_session_policy::validate_coding_session_policy_envelope(&event)
            .map_err(|error| IngestError::Rejected(format!("invalid: {error}")))?;
    }

    // NIP-CSOB, and NIP-CSP's division again: structure only. The schema, the
    // five ordered two-field tags, the four closed vocabularies, every bound,
    // and the parity between `d`/`csob-genesis`/`csob-type` and the content
    // they restate are all answerable from this one event, so the relay
    // answers them. Whether the signer held an active seat on this umbrella —
    // or was its founder — is not: that is the consuming fold's question
    // against the accepted NIP-CSAT chain, exactly as it is for the team
    // transaction and the policy above. A relay that adjudicated it here would
    // be asserting standing it cannot verify.
    //
    // Membership is still checked before any of this: 44246 is a
    // coding-session kind (`is_coding_session_kind`), so it goes through the
    // strict channel-membership gate first and a non-member is refused before
    // its content is ever parsed.
    //
    // No `coding_session_content_cap` entry: that function bounds storage for
    // the four provider-authored kinds that get *no* envelope validator. 44246
    // has one, and `decode_coding_session_observation` already refuses content
    // over MAX_CODING_SESSION_OBSERVATION_CONTENT_BYTES, so a second bound
    // here would be redundant and could only drift from the first.
    if kind_u32 == KIND_CODING_SESSION_OBSERVATION {
        buzz_core::coding_session_observation::validate_coding_session_observation_envelope(&event)
            .map_err(|error| IngestError::Rejected(format!("invalid: {error}")))?;
    }

    // NIP-CSH, the same division a third time: structure only. The schema, the
    // five ordered two-field tags, the closed type/preserved/artifact/outcome/
    // mode vocabularies, every bound, and the parity between
    // `d`/`csh-genesis`/`csh-type` and the content they restate are all
    // answerable from this one event. Whether the author held standing to
    // checkpoint — or whether a continuation's `claimRef` is the claim in
    // force — is not: that is
    // `buzz_core::coding_session_handover_fold`'s question against the
    // accepted NIP-CSAT chain. A relay that adjudicated it here would be
    // deciding who may take a session over at ingest, which is exactly the
    // authority the chain exists to serialize.
    //
    // No `coding_session_content_cap` entry, for 44246's reason:
    // `decode_coding_session_handover` already refuses content over
    // MAX_CODING_SESSION_HANDOVER_CONTENT_BYTES, and a second bound here could
    // only drift from the first.
    if kind_u32 == KIND_CODING_SESSION_HANDOVER {
        buzz_core::coding_session_handover::validate_coding_session_handover_envelope(&event)
            .map_err(|error| IngestError::Rejected(format!("invalid: {error}")))?;
    }

    // The four provider-authored coding-session kinds get no envelope
    // validator: their content is the provider's own account of what a session
    // did, and a relay that parsed it would be asserting authority over facts
    // it did not observe. Consumers verify signatures and shapes at their own
    // trusted-ingress boundary. What the relay owes them is a bound on storage.
    if let Some(max) = coding_session_content_cap(kind_u32) {
        let got = event.content.len();
        if got > max {
            return Err(IngestError::Rejected(format!(
                "invalid: coding-session kind {kind_u32} content exceeds {max} bytes (got {got})"
            )));
        }
    }

    if kind_u32 == KIND_EVENT_REMINDER {
        validate_event_reminder(&event)
            .map_err(|e| IngestError::Rejected(format!("invalid: {e}")))?;
    }

    if kind_u32 == KIND_PERSONA {
        validate_persona_envelope(&event)
            .map_err(|e| IngestError::Rejected(format!("invalid: {e}")))?;
    }

    if kind_u32 == KIND_TEAM_CATALOG {
        validate_team_catalog_envelope(&event)
            .map_err(|e| IngestError::Rejected(format!("invalid: {e}")))?;
    }

    if kind_u32 == KIND_PROJECT {
        validate_project_envelope(&event)
            .map_err(|e| IngestError::Rejected(format!("invalid: {e}")))?;
    }

    // NIP-PK (kind 30624): where a project's persona packs live. Shape first,
    // then a **closed** authority gate — this record decides which prompt bytes
    // every seat on the project runs, so unlike the soft `project`
    // back-references elsewhere in this function an unrecognized author is
    // refused rather than tolerated. See `handlers/pack_source.rs`.
    if kind_u32 == buzz_core::kind::KIND_PROJECT_PACK_SOURCE {
        buzz_core::project_pack_source::decode_project_pack_source(&event)
            .map_err(|e| IngestError::Rejected(format!("invalid: {e}")))?;
        // Raised as an *auth* failure for the same reason the Pulse gate is:
        // §5.4's exit-code table requires a refused project write to surface as
        // HTTP 403 (→ CLI exit 3), and `bridge.rs` maps every `Rejected` to 400.
        match super::pack_source::pack_source_write_admitted(state, tenant.community(), &event)
            .await
        {
            Ok(Ok(_admission)) => {}
            Ok(Err(refusal)) => {
                return Err(IngestError::AuthFailed(refusal.sentence()));
            }
            // Fail closed: a storage blip must not narrow the founder set and
            // hand one key silent control of the team's packs.
            Err(()) => {
                return Err(IngestError::Internal(
                    "error: pack source authority lookup failed".into(),
                ));
            }
        }
    }

    // Lane L26 (kind 30625): a founder-signed repository rule record. Shape
    // first, then a **closed** authority gate — these rows decide who may push
    // a governed ref, so an unrecognized author is refused rather than
    // tolerated. See `handlers/repo_protection.rs`.
    if kind_u32 == buzz_core::kind::KIND_GIT_REPO_PROTECTION {
        buzz_core::repository_protection::decode_repository_protection(&event)
            .map_err(|e| IngestError::Rejected(format!("invalid: {e}")))?;
        // Raised as an *auth* failure for the same reason the pack-source gate
        // is: a refused repository write must surface as HTTP 403 (→ CLI exit
        // 3), and `bridge.rs` maps every `Rejected` to 400.
        match super::repo_protection::repo_protection_write_admitted(
            state,
            tenant.community(),
            &event,
        )
        .await
        {
            Ok(Ok(_admission)) => {}
            Ok(Err(refusal)) => {
                return Err(IngestError::AuthFailed(refusal.sentence()));
            }
            // Fail closed: a storage blip must not narrow the founder set and
            // hand one key silent control of the repository's rules.
            Err(()) => {
                return Err(IngestError::Internal(
                    "error: repository protection authority lookup failed".into(),
                ));
            }
        }
    }

    if kind_u32 == buzz_core::kind::KIND_SHELL_SESSION {
        let coordinate = validate_shell_session_envelope(&event)
            .map_err(|e| IngestError::Rejected(format!("invalid: {e}")))?;
        // NIP-ST write gate: announcing a terminal into a *private* project
        // requires the author to be admitted (owner or invited member) — the
        // announce surfaces the session in that project's Terminals view.
        // Public/unknown coordinates stay soft references, matching the repo
        // `project` tag semantics.
        let author_bytes = event.pubkey.to_bytes();
        let allowed = state
            .db
            .can_access_project_contents(tenant.community(), &coordinate, &author_bytes)
            .await
            .map_err(|e| {
                IngestError::Internal(format!("error: project gate lookup failed: {e}"))
            })?;
        if !allowed {
            return Err(IngestError::Rejected(
                "restricted: project is private".into(),
            ));
        }
    }

    // NIP-MP Pulse (44240): an explicit project coordination entry. Its `a`
    // tag is a *required* singleton, so unlike the soft `project`
    // back-references above the coordinate must resolve to a project that
    // really exists, and the author must hold write access to it.
    //
    // If the entry also carries `h`, the generic channel-membership gate
    // earlier in this function has already run against that channel: project
    // authorization never widens channel authorization, and vice versa.
    if kind_u32 == buzz_core::kind::KIND_PULSE_ENTRY {
        // Bound the payload before any parse, the standing content-cap idiom
        // in this file. `decode_pulse_entry` repeats the check for callers
        // that reach it directly.
        let got = event.content.len();
        if got > buzz_core::pulse::MAX_PULSE_ENTRY_CONTENT_BYTES {
            return Err(IngestError::Rejected(format!(
                "invalid: pulse entry content exceeds {} bytes (got {got})",
                buzz_core::pulse::MAX_PULSE_ENTRY_CONTENT_BYTES
            )));
        }
        // Tag grammar, the canonical-coordinate rule, and the content
        // envelope. buzz-core owns the validator outright; the relay keeps no
        // local copy (the duplicated-validator drift this avoids already
        // exists in-tree for kind:30621). Note that `supersedes` is checked
        // syntactically only — the relay never looks the target up, so
        // `POST /events` cannot be used as an existence oracle for arbitrary
        // 64-hex ids, and a supersession that arrives before its target under
        // retry reordering still stores.
        buzz_core::pulse::validate_pulse_entry_envelope(&event)
            .map_err(|e| IngestError::Rejected(format!("invalid: {e}")))?;
        let coordinate =
            buzz_core::pulse::pulse_entry_project_coordinate(&event).ok_or_else(|| {
                IngestError::Rejected("invalid: pulse entry requires one a tag".into())
            })?;
        let author_bytes = event.pubkey.to_bytes();
        let gate = state
            .db
            .get_project_gate_by_coordinate(tenant.community(), &coordinate)
            .await
            // Fail closed: an unknown gate must not admit a write, matching
            // the git-child gate below.
            .map_err(|e| {
                IngestError::Internal(format!("error: project gate lookup failed: {e}"))
            })?;
        // `None` from that query means public-*or-unknown*, because it filters
        // `visibility = 'private'`. Resolve the ambiguity with an indexed
        // existence probe rather than the `can_write_project_contents` /
        // `can_access_project_contents` helpers, which cannot: both
        // `.unwrap_or(true)` on a missing row, so an entry naming a
        // coordinate no kind:30621 event ever created would be accepted, and
        // an unknown coordinate is in nobody's hidden set — the relay would
        // then show everyone a coordination fact invented out of nothing.
        let project_exists = match gate {
            Some(_) => true,
            None => state
                .db
                .project_exists_by_coordinate(tenant.community(), &coordinate)
                .await
                .map_err(|e| IngestError::Internal(format!("error: project lookup failed: {e}")))?,
        };
        // Raised as an *auth* failure, not a rejection: §5.4's exit-code table
        // requires a refused Pulse write to surface as HTTP 403 (→ CLI exit 3),
        // and `bridge.rs` maps every `Rejected` to 400 (→ exit 2, transport
        // error). The WS wire text is unchanged — `handlers/event.rs` sends the
        // identical `OK false "restricted: …"` for both variants.
        pulse_write_admitted(gate.as_ref(), project_exists, &author_bytes)
            .map_err(|msg| IngestError::AuthFailed(msg.to_string()))?;
    }

    if kind_u32 == KIND_GIT_REPO_ANNOUNCEMENT {
        // NIP-MP access extension phase 2: the `project` back-reference is
        // what hides a repo's events behind a private project, so its shape
        // is validated fail-closed (a malformed coordinate is rejected, not
        // silently ignored — silently ignoring would publish a repo its
        // author believes is private).
        let project_ref = validate_repo_announcement_project_tag(&event)
            .map_err(|e| IngestError::Rejected(format!("invalid: {e}")))?;
        // Linking a repo into a *private* project additionally requires the
        // author to be admitted to it (owner or invited member): the link
        // both grants the repo that project's ACL and surfaces the repo in
        // the project's Code view, neither of which an outsider may do.
        // Public/unknown projects stay soft references (never verified to
        // exist), matching the channel `project` tag semantics.
        if let Some(ref coord) = project_ref {
            let author_bytes = event.pubkey.to_bytes();
            let allowed = state
                .db
                .can_access_project_contents(tenant.community(), coord, &author_bytes)
                .await
                .map_err(|e| {
                    IngestError::Internal(format!("error: project gate lookup failed: {e}"))
                })?;
            if !allowed {
                return Err(IngestError::Rejected(
                    "restricted: project is private".into(),
                ));
            }
        }
    }

    // NIP-MP access extension phase 2, write gate: ref state (30618) and the
    // NIP-34 child kinds (patches/PRs/issues/status) targeting a repo inside
    // a private project may only be written by identities the repo gate
    // admits — the repo owner, the project owner, or an invited member. The
    // relay's own key is exempt: relay-signed 30618 emissions must succeed
    // for private repos. Repo-name resolution is tolerant of coordinate case
    // so a case-variant `a` tag cannot dodge the gate. (30617 itself is the
    // owner's own announcement, gated above via its project tag instead.)
    if buzz_core::kind::is_git_project_gated_kind(kind_u32)
        && kind_u32 != KIND_GIT_REPO_ANNOUNCEMENT
        && event.pubkey != state.relay_keypair.public_key()
    {
        let author_bytes = event.pubkey.to_bytes();
        for repo_name in buzz_core::kind::git_event_repo_names(&event) {
            match state
                .repo_project_gate_cached(tenant.community(), &repo_name)
                .await
            {
                Ok(None) => {}
                Ok(Some(gate)) => {
                    if !gate.admits_write(&author_bytes) {
                        return Err(IngestError::Rejected(
                            "restricted: repository belongs to a private project".into(),
                        ));
                    }
                }
                // Fail closed: an unknown gate must not admit a write.
                Err(e) => {
                    return Err(IngestError::Internal(format!(
                        "error: repo gate lookup failed: {e}"
                    )));
                }
            }
        }
    }

    // Track pre-created channel UUID for compensation on insert failure.
    let mut pre_created_channel: Option<Uuid> = None;

    if kind_u32 == KIND_NIP29_CREATE_GROUP {
        // Validate name tag is present and non-empty before any DB work.
        let create_name = event.tags.iter().find_map(|t| {
            if t.kind().to_string() == "name" {
                t.content().map(|s| s.to_string())
            } else {
                None
            }
        });
        if create_name
            .as_ref()
            .map(|n| {
                buzz_core::channel::canonical_channel_name(n)
                    .trim()
                    .is_empty()
            })
            .unwrap_or(true)
        {
            return Err(IngestError::Rejected(
                "invalid: channel name is required".into(),
            ));
        }

        // Validate visibility/channel_type for ALL kind:9007 events (with or without h-tag).
        // This runs pre-storage so invalid enums are rejected before the event is persisted.
        let visibility_str = event
            .tags
            .iter()
            .find_map(|t| {
                if t.kind().to_string() == "visibility" {
                    t.content().map(|s| s.to_string())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| "open".to_string());
        let channel_type_str = event
            .tags
            .iter()
            .find_map(|t| {
                if t.kind().to_string() == "channel_type" {
                    t.content().map(|s| s.to_string())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| "stream".to_string());

        let visibility: buzz_db::channel::ChannelVisibility = visibility_str
            .parse()
            .map_err(|_| IngestError::Rejected(format!("invalid visibility: {visibility_str}")))?;
        let channel_type: buzz_db::channel::ChannelType =
            channel_type_str.parse().map_err(|_| {
                IngestError::Rejected(format!("invalid channel_type: {channel_type_str}"))
            })?;

        // Optional project-container association. Absence is fine; presence must
        // be a well-formed `30621:<pubkey>:<slug>` coordinate — malformed input
        // is rejected before any DB work, same as visibility/channel_type above.
        let project_ref = event.tags.iter().find_map(|t| {
            if t.kind().to_string() == "project" {
                t.content().map(|s| s.to_string())
            } else {
                None
            }
        });
        if let Some(ref project_ref) = project_ref {
            validate_project_ref_tag(project_ref)
                .map_err(|e| IngestError::Rejected(format!("invalid: {e}")))?;
        }

        if let Some(client_uuid) = channel_id {
            let name = create_name.unwrap_or_default();
            let name = buzz_core::channel::canonical_channel_name(&name);

            let description = event.tags.iter().find_map(|t| {
                if t.kind().to_string() == "about" {
                    t.content().map(|s| s.to_string())
                } else {
                    None
                }
            });

            let ttl_seconds = super::resolve_ttl(&event, state.config.ephemeral_ttl_override);

            let actor_bytes = event.pubkey.to_bytes().to_vec();
            let (_, was_created) = state
                .db
                .create_channel_with_id(
                    tenant.community(),
                    client_uuid,
                    name,
                    channel_type,
                    visibility,
                    description.as_deref(),
                    &actor_bytes,
                    ttl_seconds,
                    project_ref.as_deref(),
                )
                .await
                .map_err(|e| IngestError::Internal(format!("error: {e}")))?;

            if !was_created {
                return Ok(IngestResult {
                    event_id: event_id_hex,
                    accepted: false,
                    message: "duplicate: channel already exists".into(),
                });
            }
            pre_created_channel = Some(client_uuid);
            metrics::counter!(
                "buzz_channels_created_total",
                "community" => tenant.host().to_owned(),
                "type" => channel_type.to_string()
            )
            .increment(1);
        }
    }

    if kind_u32 == KIND_NIP29_JOIN_REQUEST {
        // A join without an h-tag is meaningless — reject early.
        if channel_id.is_none() {
            return Err(IngestError::Rejected(
                "invalid: join request must include an h tag".into(),
            ));
        }
        if channel_id.is_some() {
            match &channel_row {
                Some(ch) if ch.visibility == "private" => {
                    return Err(IngestError::Rejected(
                        "restricted: channel is private".into(),
                    ));
                }
                None => {
                    return Err(IngestError::Rejected("invalid: channel not found".into()));
                }
                _ => {} // open — OK
            }
        }
    }

    if kind_u32 == super::push_lease::KIND_PUSH_LEASE {
        let outcome = super::push_lease::accept(tenant, state, &event, now)
            .await
            .map_err(map_push_accept_error)?;
        match outcome {
            buzz_db::push::AcceptLeaseOutcome::Accepted => {}
            buzz_db::push::AcceptLeaseOutcome::StaleEvent => {
                return Err(IngestError::Rejected("invalid: stale replacement".into()));
            }
            buzz_db::push::AcceptLeaseOutcome::StaleGeneration => {
                return Err(IngestError::Rejected("invalid: stale generation".into()));
            }
            buzz_db::push::AcceptLeaseOutcome::EndpointAlreadyLeased => {
                return Err(IngestError::Rejected(
                    "invalid: endpoint already leased".into(),
                ));
            }
            buzz_db::push::AcceptLeaseOutcome::LeaseQuotaExceeded => {
                return Err(IngestError::Rejected(
                    "invalid: lease quota exceeded".into(),
                ));
            }
            buzz_db::push::AcceptLeaseOutcome::SourceEventCollision => {
                return Err(IngestError::Rejected(
                    "invalid: source event collision".into(),
                ));
            }
            buzz_db::push::AcceptLeaseOutcome::ConstraintViolation => {
                return Err(IngestError::Rejected(
                    "invalid: lease constraint violation".into(),
                ));
            }
        };
        emit(
            tracer,
            TraceAction::WriteInsertGlobal {
                msg_id: msg_id_label(event.id.as_bytes()),
                claimed_community: claimed_community_from_event(&event),
            },
            state_for_request(tenant, auth.pubkey()),
        );
        return Ok(IngestResult {
            event_id: event_id_hex,
            accepted: true,
            message: String::new(),
        });
    }

    let tenant_media_base =
        crate::api::media::media_base_url_for_tenant(&state.config.relay_url, tenant.host());
    if kind_u32 == KIND_STREAM_MESSAGE {
        validate_link_preview_tags(&event, &tenant_media_base)
            .map_err(|e| IngestError::Rejected(format!("invalid: {e}")))?;
    }

    let imeta_tags: Vec<Vec<String>> = event
        .tags
        .iter()
        .filter(|t| t.kind().to_string() == "imeta")
        .map(|t| t.as_slice().iter().map(|s| s.to_string()).collect())
        .collect();
    if !imeta_tags.is_empty() {
        crate::api::validate_imeta_tags(&imeta_tags, &tenant_media_base)
            .map_err(|e| IngestError::Rejected(format!("invalid: {e}")))?;
        crate::api::verify_imeta_blobs(tenant, &imeta_tags, &state.media_storage)
            .await
            .map_err(|e| IngestError::Rejected(format!("invalid: {e}")))?;
    }

    let thread_meta = if requires_h_channel_scope(kind_u32) {
        if let Some(ch_id) = channel_id {
            resolve_nip10_thread_meta(tenant.community(), &event, ch_id, state)
                .await
                .map_err(|msg| IngestError::Rejected(format!("invalid: {msg}")))?
        } else {
            None
        }
    } else {
        None
    };

    // Pre-validate kind:0 content before storage so we don't store an event
    // whose profile sync will silently fail in the side-effect handler.
    if kind_u32 == KIND_PROFILE
        && serde_json::from_str::<serde_json::Value>(&event.content).is_err()
    {
        return Err(IngestError::Rejected(
            "invalid: kind:0 content must be valid JSON".into(),
        ));
    }

    if kind_u32 == KIND_EMOJI_SET || kind_u32 == KIND_EMOJI_LIST {
        validate_custom_emoji_tags(&event)?;
    }

    // Resolve the target reference, then use one DB transaction to upsert the
    // reaction row (dedup via ON CONFLICT) with reaction_event_id already set and
    // store the kind:7 event. This replaces the post-storage side-effect handler.
    if kind_u32 == KIND_REACTION {
        // Extract target event hex from last e-tag (NIP-25).
        let target_hex = event
            .tags
            .iter()
            .rev()
            .find_map(|tag| {
                if tag.kind().to_string() == "e" {
                    tag.content().and_then(|v| {
                        if v.len() == 64 && v.chars().all(|c| c.is_ascii_hexdigit()) {
                            Some(v.to_string())
                        } else {
                            None
                        }
                    })
                } else {
                    None
                }
            })
            .ok_or_else(|| {
                IngestError::Rejected(
                    "invalid: reaction must reference a target event via e tag".into(),
                )
            })?;

        let target_id = hex::decode(&target_hex)
            .map_err(|_| IngestError::Rejected("invalid: malformed reaction target id".into()))?;

        let actor_bytes = effective_message_author(&event, &state.relay_keypair.public_key());
        let emoji = if event.content.is_empty() {
            "+"
        } else {
            &event.content
        };

        validate_reaction_emoji(&event, emoji)?;

        // Atomically upsert the reaction row with this kind:7 event id, then store
        // the event in the same transaction. Ordering is load-bearing: active
        // duplicate reactions must return before storing a duplicate kind:7 event.
        let thread_params = thread_meta.as_ref().map(|m| m.as_params());
        let (stored_event, was_inserted) = match state
            .db
            .insert_reaction_event_with_thread_metadata(
                tenant.community(),
                &event,
                channel_id,
                thread_params,
                &target_id,
                &actor_bytes,
                emoji,
            )
            .await
            .map_err(|e| IngestError::Internal(format!("error: {e}")))?
        {
            buzz_db::ReactionEventInsertOutcome::TargetMissing => {
                return Err(IngestError::Rejected(
                    "invalid: reaction target event not found".into(),
                ));
            }
            buzz_db::ReactionEventInsertOutcome::Duplicate => {
                return Ok(IngestResult {
                    event_id: event_id_hex,
                    accepted: false,
                    message: "duplicate: reaction already exists".into(),
                });
            }
            buzz_db::ReactionEventInsertOutcome::Inserted {
                stored_event,
                was_inserted,
            } => (stored_event, was_inserted),
        };

        let pubkey_hex = auth.pubkey().to_hex();
        // Spec WriteInsert (line 514) / WriteDuplicate (line 606) /
        // WriteInsertGlobal (line 559): emit the abstract write action. The
        // persist API returns `was_inserted` (true → Insert/Global, false →
        // Duplicate). Reactions on project events (issue/PR roots and their
        // comments) carry no `h` tag, so `channel_id` can be `None` here —
        // mirror the message write's three-way split instead of asserting a
        // channel, which panicked the ingest worker on those events.
        let claimed = claimed_community_from_event(&event);
        let action = match (channel_id, was_inserted) {
            (Some(ch), true) => TraceAction::WriteInsert {
                msg_id: msg_id_label(event.id.as_bytes()),
                channel: channel_label(ch),
                claimed_community: claimed,
            },
            (Some(ch), false) => TraceAction::WriteDuplicate {
                msg_id: msg_id_label(event.id.as_bytes()),
                channel: channel_label(ch),
                claimed_community: claimed,
            },
            (None, _) => TraceAction::WriteInsertGlobal {
                msg_id: msg_id_label(event.id.as_bytes()),
                claimed_community: claimed,
            },
        };
        emit(tracer, action, state_for_request(tenant, auth.pubkey()));
        dispatch_persistent_event(
            tenant,
            state,
            &stored_event,
            kind_u32,
            &pubkey_hex,
            threaded_visibility.clone(),
        )
        .await;

        info!(event_id = %event_id_hex, kind = kind_u32, "Event ingested via pipeline");
        return Ok(IngestResult {
            event_id: event_id_hex,
            accepted: true,
            message: String::new(),
        });
    }

    let (stored_event, was_inserted) = if buzz_core::kind::is_replaceable(kind_u32) {
        // NIP-16 replaceable event — atomic replace with stale-write protection.
        // channel_id is None for global kinds (0, 1, 3) due to step 5b above.
        state
            .db
            .replace_addressable_event(tenant.community(), &event, channel_id)
            .await
            .map_err(|e| IngestError::Internal(format!("error: {e}")))?
    } else if is_parameterized_replaceable(kind_u32) {
        // NIP-33 parameterized replaceable — keyed by (kind, pubkey, d_tag).
        let d_tag = buzz_db::event::extract_d_tag(&event).unwrap_or_default();
        if d_tag.len() > buzz_db::event::D_TAG_MAX_LEN {
            return Err(IngestError::Rejected(format!(
                "invalid: d tag too long ({} bytes, max {})",
                d_tag.len(),
                buzz_db::event::D_TAG_MAX_LEN,
            )));
        }
        state
            .db
            .replace_parameterized_event(tenant.community(), &event, &d_tag, channel_id)
            .await
            .map_err(super::ingest_error::parameterized_write_error)?
    } else if kind_u32 == KIND_CODING_SESSION_GENESIS {
        // Genesis is a regular event, but storing it also decides a question no
        // pure validator can: whether this pubkey is the founder of this
        // umbrella, or merely the second to ask. That check has to be atomic
        // with the insert, so it lives in the storage transaction — see
        // `buzz_db::event::insert_coding_session_genesis_event`.
        let Some(genesis_channel) = channel_id else {
            // `requires_h_channel_scope` already refused a genesis without a
            // resolvable `h` channel. Failing closed rather than falling
            // through keeps an unscoped genesis from being stored *without*
            // the uniqueness check that only a channel makes meaningful.
            return Err(IngestError::Rejected(
                "invalid: coding-session genesis requires a channel".into(),
            ));
        };
        let thread_params = thread_meta.as_ref().map(|m| m.as_params());
        match state
            .db
            .insert_coding_session_genesis_event(
                tenant.community(),
                &event,
                genesis_channel,
                thread_params,
            )
            .await
            .map_err(|e| IngestError::Internal(format!("error: {e}")))?
        {
            buzz_db::CodingSessionGenesisInsertOutcome::Founded {
                stored_event,
                was_inserted,
            } => (*stored_event, was_inserted),
            buzz_db::CodingSessionGenesisInsertOutcome::AlreadyFounded { existing_event_id } => {
                return Ok(coding_session_genesis_duplicate_result(
                    event_id_hex,
                    &existing_event_id,
                ));
            }
            buzz_db::CodingSessionGenesisInsertOutcome::AdoptionRefused { refusal } => {
                return Ok(coding_session_genesis_adoption_refusal_result(
                    event_id_hex,
                    &refusal,
                ));
            }
        }
    } else if kind_u32 == KIND_CODING_SESSION_AUTHORITY_TRANSITION {
        // Same shape as genesis just above: "does this transition extend the
        // chain" is a question the storage transaction alone can answer
        // atomically — see
        // `buzz_db::event::insert_coding_session_authority_transition_event`.
        let Some(transition_channel) = channel_id else {
            return Err(IngestError::Rejected(
                "invalid: coding-session authority transition requires a channel".into(),
            ));
        };
        let thread_params = thread_meta.as_ref().map(|m| m.as_params());
        match state
            .db
            .insert_coding_session_authority_transition_event(
                tenant.community(),
                &event,
                transition_channel,
                thread_params,
            )
            .await
            .map_err(|e| IngestError::Internal(format!("error: {e}")))?
        {
            buzz_db::CodingSessionAuthorityTransitionInsertOutcome::Accepted {
                stored_event,
                was_inserted,
            } => (*stored_event, was_inserted),
            buzz_db::CodingSessionAuthorityTransitionInsertOutcome::Refused { refusal } => {
                return Ok(coding_session_authority_transition_refusal_result(
                    event_id_hex,
                    &refusal,
                ));
            }
        }
    } else {
        let thread_params = thread_meta.as_ref().map(|m| m.as_params());
        match state
            .db
            .insert_event_with_thread_metadata(
                tenant.community(),
                &event,
                channel_id,
                thread_params,
            )
            .await
        {
            Ok(result) => result,
            Err(e) => {
                // Compensate: if we pre-created a channel for kind:9007,
                // soft-delete it so no orphaned channel row remains.
                if let Some(ch_id) = pre_created_channel {
                    if let Err(re) = state
                        .db
                        .soft_delete_channel(tenant.community(), ch_id)
                        .await
                    {
                        warn!(event_id = %event_id_hex, "channel compensation failed: {re}");
                    }
                    state.invalidate_channel_deleted(tenant);
                }
                return Err(match e {
                    buzz_db::DbError::AuthEventRejected => {
                        IngestError::Rejected("invalid: AUTH events cannot be stored".into())
                    }
                    other => IngestError::Internal(format!("error: database error: {other}")),
                });
            }
        }
    };

    if !was_inserted {
        return Ok(IngestResult {
            event_id: event_id_hex,
            accepted: true,
            message: "duplicate:".into(),
        });
    }

    if crate::handlers::side_effects::is_side_effect_kind(kind_u32) {
        if let Err(e) =
            crate::handlers::side_effects::handle_side_effects(tenant, kind_u32, &event, state)
                .await
        {
            // error!, not warn!: the event was accepted but its side effects
            // (channel creation, git repo seeding, …) did not run — the relay
            // is now in a state the client believes it isn't. Production runs
            // RUST_LOG=error, so warn! made these failures invisible during
            // the #3527 triage.
            error!(event_id = %event_id_hex, kind = kind_u32, "Side effect failed: {e}");
            if crate::handlers::side_effects::is_admin_kind(kind_u32) {
                // An admin event's entire meaning is its side effect: a 9000
                // whose membership apply failed is stored, but answering
                // "accepted" over an unchanged roster is the lie that produced
                // silently wedged transport channels. The event stays stored —
                // a client retry of the same bytes lands on the duplicate path
                // above and converges idempotently; producers should treat
                // this error as "the effect did not apply, issue a fresh
                // event". Non-admin side-effect kinds keep best-effort
                // semantics.
                return Err(IngestError::Rejected(format!(
                    "error: stored but its effect did not apply: {e}"
                )));
            }
        }
    }

    // A freshly inserted reply changed its thread's counters (updated in the
    // same transaction as the insert) — push a fresh relay-signed 39005 so
    // subscribed clients can update badge counts without refetching the head
    // window. Page responses recompute summaries independently, so this is
    // fan-out-only and best-effort.
    if let Some(meta) = &thread_meta {
        crate::handlers::side_effects::emit_live_thread_summary(
            tenant,
            state,
            meta.channel_id,
            meta.root_event_id.clone(),
        );
    }

    let pubkey_hex = auth.pubkey().to_hex();
    // Spec WriteInsert (line 514) / WriteInsertGlobal (line 559) /
    // WriteDuplicate (line 606): emit the abstract write at the trailing
    // dispatch site. `channel_id.is_some()` distinguishes channel-bearing
    // (Insert/Duplicate) from channel-less (InsertGlobal); `was_inserted`
    // distinguishes accepted-new (Insert/Global) from no-op-on-conflict
    // (Duplicate). The WriteInsertGlobal duplicate case is not modeled
    // separately in the spec (channel-less duplicates collapse to the
    // same observation shape as channel-less inserts at this seam);
    // see docs/spec/MultiTenantRelay.tla lines 559-595.
    {
        let claimed = claimed_community_from_event(&event);
        let action = match (channel_id, was_inserted) {
            (Some(ch), true) => TraceAction::WriteInsert {
                msg_id: msg_id_label(event.id.as_bytes()),
                channel: channel_label(ch),
                claimed_community: claimed,
            },
            (Some(ch), false) => TraceAction::WriteDuplicate {
                msg_id: msg_id_label(event.id.as_bytes()),
                channel: channel_label(ch),
                claimed_community: claimed,
            },
            (None, _) => TraceAction::WriteInsertGlobal {
                msg_id: msg_id_label(event.id.as_bytes()),
                claimed_community: claimed,
            },
        };
        emit(tracer, action, state_for_request(tenant, auth.pubkey()));
    }
    dispatch_persistent_event(
        tenant,
        state,
        &stored_event,
        kind_u32,
        &pubkey_hex,
        threaded_visibility.clone(),
    )
    .await;

    info!(event_id = %event_id_hex, kind = kind_u32, "Event ingested via pipeline");

    Ok(IngestResult {
        event_id: event_id_hex,
        accepted: true,
        message: String::new(),
    })
}

fn websocket_only_ingest_kind(kind: u32) -> bool {
    kind == KIND_GIFT_WRAP || buzz_core::kind::is_ephemeral(kind)
}

#[cfg(test)]
#[path = "ingest_team_transaction_tests.rs"]
mod team_transaction_tests;

#[cfg(test)]
#[path = "ingest_coding_session_policy_tests.rs"]
mod coding_session_policy_tests;

#[cfg(test)]
#[path = "ingest_coding_session_observation_tests.rs"]
mod coding_session_observation_tests;

#[cfg(test)]
#[path = "ingest_coding_session_handover_tests.rs"]
mod coding_session_handover_tests;

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use buzz_conformance::{TraceStep, Tracer};
    use buzz_core::kind::{
        KIND_CANVAS, KIND_FORUM_COMMENT, KIND_FORUM_POST, KIND_FORUM_VOTE, KIND_LONG_FORM,
        KIND_MANAGED_AGENT, KIND_PERSONA, KIND_PRESENCE_UPDATE, KIND_STREAM_MESSAGE,
        KIND_STREAM_MESSAGE_DIFF, KIND_TEAM, KIND_USER_STATUS,
    };
    use nostr::{EventBuilder, Kind};

    #[test]
    fn reaction_validation_accepts_wrapped_max_shortcode() {
        let shortcode = "a".repeat(buzz_sdk::MAX_CUSTOM_EMOJI_SHORTCODE_LEN);
        let event = EventBuilder::new(Kind::Custom(KIND_REACTION as u16), format!(":{shortcode}:"))
            .tags([
                nostr::Tag::parse(["emoji", &shortcode, "https://example.com/max.png"])
                    .expect("emoji tag"),
            ])
            .sign_with_keys(&nostr::Keys::generate())
            .expect("sign reaction");

        assert!(validate_reaction_emoji(&event, &event.content).is_ok());
    }

    #[test]
    fn reaction_validation_rejects_mixed_case_max_shortcode() {
        let shortcode = "Ab".repeat(buzz_sdk::MAX_CUSTOM_EMOJI_SHORTCODE_LEN / 2);
        let event = EventBuilder::new(Kind::Custom(KIND_REACTION as u16), format!(":{shortcode}:"))
            .tags([
                nostr::Tag::parse(["emoji", &shortcode, "https://example.com/max.png"])
                    .expect("emoji tag"),
            ])
            .sign_with_keys(&nostr::Keys::generate())
            .expect("sign reaction");

        assert!(matches!(
            validate_reaction_emoji(&event, &event.content),
            Err(IngestError::Rejected(_))
        ));
    }

    #[test]
    fn reaction_validation_rejects_case_mismatched_tag() {
        let shortcode = "a".repeat(buzz_sdk::MAX_CUSTOM_EMOJI_SHORTCODE_LEN);
        let uppercase_shortcode = shortcode.to_uppercase();
        let event = EventBuilder::new(Kind::Custom(KIND_REACTION as u16), format!(":{shortcode}:"))
            .tags([nostr::Tag::parse([
                "emoji",
                &uppercase_shortcode,
                "https://example.com/max.png",
            ])
            .expect("emoji tag")])
            .sign_with_keys(&nostr::Keys::generate())
            .expect("sign reaction");

        assert!(matches!(
            validate_reaction_emoji(&event, &event.content),
            Err(IngestError::Rejected(_))
        ));
    }

    #[test]
    fn emoji_set_validation_enforces_shortcode_boundary() {
        let max_shortcode = "a".repeat(buzz_sdk::MAX_CUSTOM_EMOJI_SHORTCODE_LEN);
        let valid_event = EventBuilder::new(Kind::Custom(KIND_EMOJI_SET as u16), "")
            .tags([
                nostr::Tag::parse(["emoji", &max_shortcode, "https://example.com/max.png"])
                    .expect("emoji tag"),
            ])
            .sign_with_keys(&nostr::Keys::generate())
            .expect("sign valid emoji set");
        assert!(validate_custom_emoji_tags(&valid_event).is_ok());

        let shortcode = "a".repeat(buzz_sdk::MAX_CUSTOM_EMOJI_SHORTCODE_LEN + 1);
        let event = EventBuilder::new(Kind::Custom(KIND_EMOJI_SET as u16), "")
            .tags([
                nostr::Tag::parse(["emoji", &shortcode, "https://example.com/long.png"])
                    .expect("emoji tag"),
            ])
            .sign_with_keys(&nostr::Keys::generate())
            .expect("sign emoji set");

        assert!(matches!(
            validate_custom_emoji_tags(&event),
            Err(IngestError::Rejected(message)) if message.contains("exceeds 64 bytes")
        ));
    }

    /// A banned relay admin must be refused with the same wire prefix and
    /// transport status as every other durable-restriction refusal:
    /// `blocked:` and (via `bridge.rs`'s `AuthFailed` arm) HTTP 403 — never
    /// `invalid:`/400, which reads as "your request was malformed" and lets a
    /// client retry-loop against an authorization decision.
    #[test]
    fn relay_admin_ban_maps_to_blocked_auth_failure() {
        let mapped = map_relay_admin_error(super::super::relay_admin::RelayAdminError::Banned);
        match mapped {
            IngestError::AuthFailed(msg) => {
                assert_eq!(msg, "blocked: you are banned from this community");
            }
            other => panic!("banned admin must map to AuthFailed (HTTP 403), got {other:?}"),
        }
    }

    /// Validation/authorization failures keep the pre-existing `invalid:`
    /// prefix and 400 status — this is the arm the whole 9030-series relied on
    /// before the ban category existed, so it must not regress.
    #[test]
    fn relay_admin_rejection_keeps_invalid_prefix() {
        let mapped = map_relay_admin_error(super::super::relay_admin::RelayAdminError::Rejected(
            "actor not authorized: must be admin or owner".to_string(),
        ));
        match mapped {
            IngestError::Rejected(msg) => {
                assert_eq!(
                    msg, "invalid: actor not authorized: must be admin or owner",
                    "existing relay-admin rejections must keep their exact wire text"
                );
            }
            other => panic!("validation failure must map to Rejected, got {other:?}"),
        }
    }

    /// A restriction-lookup outage is a server fault, not a client one. It
    /// must fail closed as `error:`/500 so a Postgres blip can neither admit a
    /// banned admin nor be reported to an innocent one as a bad request.
    #[test]
    fn relay_admin_internal_maps_to_error_not_client_fault() {
        let mapped = map_relay_admin_error(super::super::relay_admin::RelayAdminError::Internal(
            "internal error checking restriction state: pool timed out".to_string(),
        ));
        match mapped {
            IngestError::Internal(msg) => {
                assert!(
                    msg.starts_with("error: "),
                    "internal failures need the `error:` NIP-01 prefix, got {msg:?}"
                );
            }
            other => {
                panic!("restriction DB failure must map to Internal (HTTP 500), got {other:?}")
            }
        }
    }

    /// An active community passes the durable write fence untouched.
    #[test]
    fn serving_fence_active_community_admits_write() {
        assert!(map_serving_fence_state(Ok(true)).is_ok());
    }

    /// A fenced/tombstoned/archived community is an authorization decision:
    /// `restricted:` and (via `bridge.rs`) HTTP 400 — with the exact wire text
    /// the ephemeral WS path uses, so clients see one refusal vocabulary.
    #[test]
    fn serving_fence_inactive_community_maps_to_restricted() {
        match map_serving_fence_state(Ok(false)) {
            Err(IngestError::Rejected(msg)) => {
                assert_eq!(msg, "restricted: community writes are fenced");
            }
            other => panic!("fenced community must map to Rejected, got {other:?}"),
        }
    }

    /// A fence-lookup outage is a server fault and must fail closed as
    /// `error:`/500 — a Postgres blip can neither admit a write past the
    /// fence nor be reported to an innocent client as a bad request.
    #[test]
    fn serving_fence_lookup_outage_fails_closed_as_internal() {
        let outage = buzz_db::DbError::Sqlx(sqlx::Error::PoolTimedOut);
        match map_serving_fence_state(Err(outage)) {
            Err(IngestError::Internal(msg)) => {
                assert!(
                    msg.starts_with("error: "),
                    "fence outages need the `error:` NIP-01 prefix, got {msg:?}"
                );
            }
            other => panic!("fence lookup failure must map to Internal, got {other:?}"),
        }
    }

    /// Production-path regression: the exact predicate `ingest_event_inner`
    /// consults must admit writes while a community is active and refuse them
    /// once the community deletion lifecycle fences it.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn ingest_write_fence_follows_community_deletion_lifecycle() {
        use buzz_db::deletion::{
            FrozenInventory, KeyStreamDigest, PrefixManifest, StorageManifest,
            DEFAULT_LEASE_DURATION,
        };

        let url = std::env::var("BUZZ_TEST_DATABASE_URL")
            .or_else(|_| std::env::var("DATABASE_URL"))
            .unwrap_or_else(|_| "postgres://buzz:buzz_dev@localhost:5432/buzz".to_string()); // sadscan:disable np.postgres.1
        let pool = sqlx::PgPool::connect(&url).await.expect("connect test DB");
        let db = buzz_db::Db::from_pool(pool);
        db.migrate().await.expect("migrate test DB");
        let store = buzz_deletion::store(&db);

        let host = format!("lane3-fence-{}.example", Uuid::new_v4().simple());
        let community = db
            .ensure_configured_community(&host)
            .await
            .expect("community")
            .id;

        assert!(
            map_serving_fence_state(store.is_serving_active(community).await).is_ok(),
            "active community must admit persistent ingest"
        );

        let submitted = store
            .submit(
                &host,
                "test-operator",
                Some("lane3 ingest fence regression"),
            )
            .await
            .expect("submit");
        let inventory = FrozenInventory {
            schema: store
                .inventory_schema(community)
                .await
                .expect("schema inventory"),
            storage: StorageManifest {
                version: 4,
                prefixes: buzz_media::tenant_prefixes(*community.as_uuid())
                    .into_iter()
                    .map(|prefix| PrefixManifest {
                        prefix,
                        object_count: 0,
                        total_bytes: 0,
                        keys_digest: KeyStreamDigest::new().finish().0,
                    })
                    .collect(),
            },
        };
        let request = store
            .freeze_inventory(submitted.id, &inventory)
            .await
            .expect("freeze inventory");
        store
            .approve(request.id, "approver", None)
            .await
            .expect("approve");
        let claim = store
            .claim_specific(request.id, "executor", DEFAULT_LEASE_DURATION)
            .await
            .expect("claim")
            .expect("won claim");
        store.begin_quiescing(&claim.lease).await.expect("quiesce");
        store.fence(&claim.lease).await.expect("fence");

        match map_serving_fence_state(store.is_serving_active(community).await) {
            Err(IngestError::Rejected(msg)) => {
                assert_eq!(msg, "restricted: community writes are fenced");
            }
            other => panic!("fenced community must refuse persistent ingest, got {other:?}"),
        }
    }

    #[derive(Debug, Default)]
    struct VecTracer {
        steps: Mutex<Vec<TraceStep>>,
    }

    impl Tracer for VecTracer {
        fn record(&self, step: TraceStep) {
            self.steps.lock().expect("trace lock").push(step);
        }
    }

    #[test]
    fn feedback_success_action_satisfies_ingest_emit_guard() {
        let community = buzz_core::CommunityId::from_uuid(Uuid::new_v4());
        let tenant = TenantContext::resolved(community, "feedback.test");
        let keys = nostr::Keys::generate();
        let event = EventBuilder::new(
            Kind::Custom(KIND_PRODUCT_FEEDBACK as u16),
            "Useful feedback",
        )
        .sign_with_keys(&keys)
        .expect("sign feedback");
        let auth = IngestAuth::Http {
            pubkey: keys.public_key(),
            scopes: vec![Scope::MessagesWrite],
            auth_method: HttpAuthMethod::Nip98,
        };
        let tracer = Arc::new(VecTracer::default());
        let abstract_state = state_for_request(&tenant, auth.pubkey());

        {
            let (guard, counting) = EmitGuard::arm(
                tracer.clone(),
                abstract_state.clone(),
                "ingest_event_exited_without_trace",
            );
            emit_product_feedback_success(&counting, &tenant, &event, &auth);
            drop(guard);
        }

        let steps = tracer.steps.lock().expect("trace lock");
        assert_eq!(steps.len(), 1);
        assert!(matches!(
            steps[0].action,
            TraceAction::WriteInsertGlobal { .. }
        ));
    }

    #[test]
    fn nip_ia_requests_are_global_only() {
        // NIP-IA requests drive relay-global archive state; a stray `h` tag
        // must not channel-scope them, or the global audit trail breaks.
        for kind in [KIND_IA_ARCHIVE_REQUEST, KIND_IA_UNARCHIVE_REQUEST] {
            assert!(is_global_only_kind(kind), "kind {kind} must be global-only");
            assert!(
                !requires_h_channel_scope(kind),
                "kind {kind} must not require an h tag"
            );
        }
    }

    #[test]
    fn channel_scoped_content_kinds_require_h_tags() {
        for kind in [
            KIND_STREAM_MESSAGE,
            KIND_STREAM_MESSAGE_DIFF,
            KIND_CANVAS,
            KIND_FORUM_POST,
            KIND_FORUM_VOTE,
            KIND_FORUM_COMMENT,
        ] {
            assert!(
                requires_h_channel_scope(kind),
                "kind {kind} should require h"
            );
        }
    }

    #[test]
    fn nip29_admin_kinds_require_h_tags() {
        for kind in [
            KIND_NIP29_PUT_USER,
            KIND_NIP29_REMOVE_USER,
            KIND_NIP29_EDIT_METADATA,
            KIND_NIP29_DELETE_EVENT,
            KIND_NIP29_DELETE_GROUP,
            KIND_NIP29_LEAVE_REQUEST,
        ] {
            assert!(
                requires_h_channel_scope(kind),
                "kind {kind} should require h"
            );
        }
    }

    #[test]
    fn create_group_does_not_require_h_tag() {
        // kind:9007 creates the channel — h-tag is optional (client-chosen UUID)
        assert!(!requires_h_channel_scope(KIND_NIP29_CREATE_GROUP));
    }

    #[test]
    fn join_request_does_not_require_h_tag_via_requires_h() {
        // kind:9021 uses h-tag for channel reference but doesn't go through
        // requires_h_channel_scope — it's handled separately in the pipeline
        // because it needs special "open-only" validation
        assert!(!requires_h_channel_scope(KIND_NIP29_JOIN_REQUEST));
    }

    #[test]
    fn reactions_do_not_require_h_tag() {
        assert!(!requires_h_channel_scope(KIND_REACTION));
    }

    #[test]
    fn long_form_is_in_scope_allowlist() {
        let dummy = make_dummy_event();
        assert!(
            required_scope_for_kind(KIND_LONG_FORM, &dummy).is_ok(),
            "KIND_LONG_FORM (30023) should be accepted"
        );
    }

    #[test]
    fn long_form_requires_messages_write_scope() {
        let dummy = make_dummy_event();
        assert_eq!(
            required_scope_for_kind(KIND_LONG_FORM, &dummy).unwrap(),
            Scope::MessagesWrite,
        );
    }

    #[test]
    fn long_form_does_not_require_h_tag() {
        // kind:30023 is global (author-owned, not channel-scoped)
        assert!(!requires_h_channel_scope(KIND_LONG_FORM));
    }

    #[test]
    fn long_form_is_global_only() {
        // kind:30023 is always global — ingest nulls channel_id even if an h-tag is present
        assert!(is_global_only_kind(KIND_LONG_FORM));
    }

    #[test]
    fn user_status_requires_users_write_scope() {
        let dummy = make_dummy_event();
        assert_eq!(
            required_scope_for_kind(KIND_USER_STATUS, &dummy).unwrap(),
            Scope::UsersWrite,
        );
    }

    #[test]
    fn user_status_is_global_only() {
        assert!(is_global_only_kind(KIND_USER_STATUS));
    }

    #[test]
    fn user_status_does_not_require_h_tag() {
        assert!(!requires_h_channel_scope(KIND_USER_STATUS));
    }

    #[test]
    fn private_sidecars_and_moderation_commands_require_messages_write_scope() {
        let dummy = make_dummy_event();
        for kind in [
            KIND_REPORT,
            KIND_PRODUCT_FEEDBACK,
            KIND_MODERATION_BAN,
            KIND_MODERATION_UNBAN,
            KIND_MODERATION_TIMEOUT,
            KIND_MODERATION_UNTIMEOUT,
            KIND_MODERATION_RESOLVE_REPORT,
        ] {
            assert_eq!(
                required_scope_for_kind(kind, &dummy).unwrap(),
                Scope::MessagesWrite,
                "kind {kind} should require MessagesWrite scope"
            );
        }
    }

    #[test]
    fn moderation_commands_are_global_only() {
        for kind in [
            KIND_MODERATION_BAN,
            KIND_MODERATION_UNBAN,
            KIND_MODERATION_TIMEOUT,
            KIND_MODERATION_UNTIMEOUT,
            KIND_MODERATION_RESOLVE_REPORT,
        ] {
            assert!(is_global_only_kind(kind), "kind {kind} must be global-only");
            assert!(
                !requires_h_channel_scope(kind),
                "kind {kind} must not require an h tag"
            );
        }
    }

    #[test]
    fn moderation_command_rejection_from_ingest_preserves_prefix() {
        let rejection = "restricted: moderator access required".to_string();
        let map_rejection = IngestError::Rejected;
        let result: Result<(), String> = Err(rejection.clone());

        match result.map_err(map_rejection).unwrap_err() {
            IngestError::Rejected(message) => assert_eq!(message, rejection),
            _ => panic!("expected rejected ingest error"),
        }
    }

    #[test]
    fn push_infrastructure_failures_are_internal_not_protocol_invalid() {
        match map_push_accept_error(crate::handlers::push_lease::AcceptError::Internal(
            "gateway unavailable".to_string(),
        )) {
            IngestError::Internal(message) => {
                assert_eq!(message, "gateway unavailable");
                assert!(!message.starts_with("invalid:"));
            }
            _ => panic!("infrastructure failure became a protocol rejection"),
        }
        match map_push_accept_error(crate::handlers::push_lease::AcceptError::Validation(
            "unknown executor key".to_string(),
        )) {
            IngestError::Rejected(message) => {
                assert_eq!(message, "invalid: unknown executor key")
            }
            _ => panic!("validation failure did not become a protocol rejection"),
        }
    }

    #[test]
    fn global_only_and_channel_scoped_are_disjoint() {
        // A kind cannot be both global-only and channel-scoped
        for kind in 0..=65535u32 {
            assert!(
                !(is_global_only_kind(kind) && requires_h_channel_scope(kind)),
                "kind {kind} is both global-only and channel-scoped"
            );
        }
    }

    #[test]
    fn private_managed_agent_kind_is_owner_scoped_global_user_data() {
        let event = make_dummy_event();
        assert_eq!(
            required_scope_for_kind(KIND_PRIVATE_MANAGED_AGENT, &event),
            Ok(Scope::UsersWrite)
        );
        assert!(is_global_only_kind(KIND_PRIVATE_MANAGED_AGENT));
        assert!(!requires_h_channel_scope(KIND_PRIVATE_MANAGED_AGENT));
    }

    #[test]
    fn ephemeral_kinds_not_in_scope_allowlist() {
        assert!(required_scope_for_kind(KIND_PRESENCE_UPDATE, &make_dummy_event()).is_err());
    }

    #[test]
    fn per_kind_scope_allowlist_covers_all_migrated_kinds() {
        let dummy = make_dummy_event();
        let migrated = [
            KIND_PROFILE,
            KIND_DELETION,
            KIND_REACTION,
            KIND_REPORT,
            KIND_PRODUCT_FEEDBACK,
            KIND_MODERATION_BAN,
            KIND_MODERATION_UNBAN,
            KIND_MODERATION_TIMEOUT,
            KIND_MODERATION_UNTIMEOUT,
            KIND_MODERATION_RESOLVE_REPORT,
            KIND_STREAM_MESSAGE,
            KIND_NIP29_PUT_USER,
            KIND_NIP29_REMOVE_USER,
            KIND_NIP29_EDIT_METADATA,
            KIND_NIP29_DELETE_EVENT,
            KIND_NIP29_CREATE_GROUP,
            KIND_NIP29_DELETE_GROUP,
            KIND_NIP29_JOIN_REQUEST,
            KIND_NIP29_LEAVE_REQUEST,
            KIND_STREAM_MESSAGE_EDIT,
            KIND_STREAM_MESSAGE_DIFF,
            KIND_CANVAS,
            KIND_FORUM_POST,
            KIND_FORUM_VOTE,
            KIND_FORUM_COMMENT,
            KIND_LONG_FORM,
            KIND_USER_STATUS,
            // NIP-51 lists + sets, NIP-65 relay list
            KIND_MUTE_LIST,
            KIND_PIN_LIST,
            KIND_NIP65_RELAY_LIST_METADATA,
            KIND_BOOKMARK_LIST,
            KIND_FOLLOW_SET,
            KIND_BOOKMARK_SET,
            KIND_EMOJI_SET,
            KIND_EMOJI_LIST,
            KIND_AGENT_ENGRAM,
            KIND_AGENT_PROFILE,
            KIND_PERSONA,
            KIND_TEAM,
            KIND_MANAGED_AGENT,
            KIND_AGENT_TURN_METRIC,
        ];
        for kind in migrated {
            assert!(
                required_scope_for_kind(kind, &dummy).is_ok(),
                "kind {kind} should be in the allowlist"
            );
        }
    }

    #[test]
    fn nip51_and_nip65_lists_require_users_write() {
        let dummy = make_dummy_event();
        for kind in [
            KIND_MUTE_LIST,
            KIND_PIN_LIST,
            KIND_NIP65_RELAY_LIST_METADATA,
            KIND_BOOKMARK_LIST,
            KIND_FOLLOW_SET,
            KIND_BOOKMARK_SET,
        ] {
            assert_eq!(
                required_scope_for_kind(kind, &dummy).ok(),
                Some(Scope::UsersWrite),
                "kind {kind} should require UsersWrite scope"
            );
        }
    }

    #[test]
    fn agent_turn_metric_is_global_only_and_in_scope_allowlist() {
        let dummy = make_dummy_event();
        assert!(
            is_global_only_kind(KIND_AGENT_TURN_METRIC),
            "kind:44200 must be global-only (no h tag)"
        );
        assert!(
            !requires_h_channel_scope(KIND_AGENT_TURN_METRIC),
            "kind:44200 must not require an h-tag"
        );
        assert_eq!(
            required_scope_for_kind(KIND_AGENT_TURN_METRIC, &dummy).unwrap(),
            Scope::MessagesWrite,
            "kind:44200 requires MessagesWrite scope"
        );
    }

    #[test]
    fn nip51_and_nip65_lists_are_global_only() {
        for kind in [
            KIND_MUTE_LIST,
            KIND_PIN_LIST,
            KIND_NIP65_RELAY_LIST_METADATA,
            KIND_BOOKMARK_LIST,
            KIND_FOLLOW_SET,
            KIND_BOOKMARK_SET,
        ] {
            assert!(
                is_global_only_kind(kind),
                "kind {kind} should be global-only (never channel-scoped)"
            );
            assert!(
                !requires_h_channel_scope(kind),
                "kind {kind} must not require an h-tag channel scope"
            );
        }
    }

    #[test]
    fn persona_is_in_scope_allowlist() {
        let dummy = make_dummy_event();
        assert_eq!(
            required_scope_for_kind(KIND_PERSONA, &dummy).unwrap(),
            Scope::UsersWrite,
        );
    }

    #[test]
    fn persona_is_global_only() {
        assert!(is_global_only_kind(KIND_PERSONA));
        assert!(!requires_h_channel_scope(KIND_PERSONA));
    }

    #[test]
    fn team_and_managed_agent_are_in_scope_allowlist() {
        let dummy = make_dummy_event();
        for kind in [KIND_TEAM, KIND_MANAGED_AGENT] {
            assert_eq!(
                required_scope_for_kind(kind, &dummy).unwrap(),
                Scope::UsersWrite,
                "kind {kind} should require UsersWrite scope"
            );
        }
    }

    #[test]
    fn team_and_managed_agent_are_global_only() {
        for kind in [KIND_TEAM, KIND_MANAGED_AGENT] {
            assert!(
                is_global_only_kind(kind),
                "kind {kind} should be global-only (never channel-scoped)"
            );
            assert!(
                !requires_h_channel_scope(kind),
                "kind {kind} must not require an h-tag channel scope"
            );
        }
    }

    #[test]
    fn unknown_kind_rejected() {
        let dummy = make_dummy_event();
        assert!(required_scope_for_kind(99999, &dummy).is_err());
    }

    #[test]
    fn gift_wrap_is_in_scope_allowlist() {
        // KIND_GIFT_WRAP is still in the per-kind scope allowlist.
        // The HTTP block is transport-level (is_http gate), not scope-level.
        let dummy = make_dummy_event();
        assert!(
            required_scope_for_kind(KIND_GIFT_WRAP, &dummy).is_ok(),
            "KIND_GIFT_WRAP should be in the scope allowlist"
        );
    }

    #[test]
    fn accounting_uses_authenticated_principal_pubkey() {
        let principal = nostr::Keys::generate();
        let envelope_signer = nostr::Keys::generate();
        let auth = IngestAuth::Nip42 {
            pubkey: principal.public_key(),
            scopes: vec![],
            channel_ids: None,
            conn_id: Uuid::new_v4(),
        };

        assert_ne!(principal.public_key(), envelope_signer.public_key());
        assert_eq!(
            auth.principal_pubkey_bytes(),
            principal.public_key().to_bytes().to_vec()
        );
    }

    #[test]
    fn ingest_auth_is_http_returns_true_for_http_variant() {
        use crate::handlers::ingest::{HttpAuthMethod, IngestAuth};
        let keys = nostr::Keys::generate();
        let http_auth = IngestAuth::Http {
            pubkey: keys.public_key(),
            scopes: vec![],
            auth_method: HttpAuthMethod::Nip98,
        };
        assert!(
            http_auth.is_http(),
            "Http variant should return true for is_http()"
        );
    }

    #[test]
    fn ingest_auth_is_http_returns_false_for_nip42_variant() {
        use crate::handlers::ingest::IngestAuth;
        let keys = nostr::Keys::generate();
        let ws_auth = IngestAuth::Nip42 {
            pubkey: keys.public_key(),
            scopes: vec![],
            channel_ids: None,
            conn_id: uuid::Uuid::new_v4(),
        };
        assert!(
            !ws_auth.is_http(),
            "Nip42 variant should return false for is_http()"
        );
    }

    #[test]
    fn presence_update_not_in_scope_allowlist() {
        // KIND_PRESENCE_UPDATE is ephemeral — not in the allowlist regardless of transport.
        let dummy = make_dummy_event();
        assert!(
            required_scope_for_kind(KIND_PRESENCE_UPDATE, &dummy).is_err(),
            "KIND_PRESENCE_UPDATE should not be in the scope allowlist"
        );
    }

    #[test]
    fn gift_wrap_presence_and_session_lease_are_websocket_only_for_ingest() {
        assert!(websocket_only_ingest_kind(KIND_GIFT_WRAP));
        assert!(websocket_only_ingest_kind(KIND_PRESENCE_UPDATE));
        assert!(websocket_only_ingest_kind(KIND_CODING_SESSION_LEASE));
        assert!(!websocket_only_ingest_kind(KIND_TEXT_NOTE));
    }

    #[test]
    fn diff_validation_rejects_missing_repo() {
        let event = make_event_with_tags(
            KIND_STREAM_MESSAGE_DIFF,
            "diff content",
            &[&["commit", "abc1234"]],
        );
        assert!(validate_diff_event(&event).is_err());
    }

    #[test]
    fn diff_validation_rejects_missing_commit() {
        let event = make_event_with_tags(
            KIND_STREAM_MESSAGE_DIFF,
            "diff content",
            &[&["repo", "https://github.com/example/repo"]],
        );
        assert!(validate_diff_event(&event).is_err());
    }

    #[test]
    fn diff_validation_accepts_valid() {
        let event = make_event_with_tags(
            KIND_STREAM_MESSAGE_DIFF,
            "diff content",
            &[
                &["repo", "https://github.com/example/repo"],
                &["commit", "abc1234"],
            ],
        );
        assert!(validate_diff_event(&event).is_ok());
    }

    #[test]
    fn diff_validation_rejects_oversized_content() {
        let big = "x".repeat(61_441);
        let event = make_event_with_tags(
            KIND_STREAM_MESSAGE_DIFF,
            &big,
            &[
                &["repo", "https://github.com/example/repo"],
                &["commit", "abc1234"],
            ],
        );
        assert!(validate_diff_event(&event).is_err());
    }

    #[test]
    fn link_preview_suppression_accepts_blanket_marker() {
        let event = make_event_with_tags(
            KIND_STREAM_MESSAGE,
            "https://example.com",
            &[&["link-preview", "none"]],
        );

        assert!(validate_link_preview_tags(&event, "https://media.example.com").is_ok());
    }

    #[test]
    fn link_preview_suppression_rejects_duplicate_marker() {
        let event = make_event_with_tags(
            KIND_STREAM_MESSAGE,
            "https://example.com",
            &[&["link-preview", "none"], &["link-preview", "none"]],
        );

        assert_eq!(
            validate_link_preview_tags(&event, "https://media.example.com"),
            Err("link-preview suppression cannot include snapshots".into())
        );
    }

    #[test]
    fn link_preview_suppression_rejects_mixed_snapshot_tags_in_either_order() {
        let snapshot = [
            "link-preview",
            "snapshot",
            "1",
            "https://example.com",
            "Example",
            "Example",
            "Description",
            "",
            "",
            "",
            "",
        ];
        for tags in [
            vec![&["link-preview", "none"][..], &snapshot[..]],
            vec![&snapshot[..], &["link-preview", "none"][..]],
        ] {
            let event = make_event_with_tags(KIND_STREAM_MESSAGE, "https://example.com", &tags);
            assert!(validate_link_preview_tags(&event, "https://media.example.com").is_err());
        }
    }

    fn make_link_preview_event(title: &str, site: &str, description: &str) -> Event {
        make_event_with_tags(
            KIND_STREAM_MESSAGE,
            "https://example.com",
            &[&[
                "link-preview",
                "snapshot",
                "1",
                "https://example.com",
                title,
                site,
                description,
                "",
                "",
                "",
                "",
            ]],
        )
    }

    #[test]
    fn link_preview_snapshot_accepts_description_newlines() {
        let event = make_link_preview_event(
            "Example title",
            "Example site",
            "First paragraph\n\nSecond paragraph",
        );

        assert!(validate_link_preview_tags(&event, "https://media.example.com").is_ok());
    }

    #[test]
    fn link_preview_snapshot_rejects_title_and_site_newlines() {
        for (title, site) in [
            ("Example\ntitle", "Example site"),
            ("Example title", "Example\nsite"),
        ] {
            let event = make_link_preview_event(title, site, "Description");
            assert!(validate_link_preview_tags(&event, "https://media.example.com").is_err());
        }
    }

    #[test]
    fn link_preview_snapshot_rejects_non_newline_controls_in_all_text_fields() {
        for (title, site, description) in [
            ("Example\ttitle", "Example site", "Description"),
            ("Example title", "Example\rsite", "Description"),
            ("Example title", "Example site", "Unsafe\tdescription"),
        ] {
            let event = make_link_preview_event(title, site, description);
            assert!(validate_link_preview_tags(&event, "https://media.example.com").is_err());
        }
    }

    fn make_dummy_event() -> Event {
        let keys = nostr::Keys::generate();
        nostr::EventBuilder::new(nostr::Kind::Custom(9), "")
            .tags([])
            .sign_with_keys(&keys)
            .unwrap()
    }

    fn make_event_with_tags(kind: u32, content: &str, tags: &[&[&str]]) -> Event {
        let keys = nostr::Keys::generate();
        let nostr_tags: Vec<nostr::Tag> = tags
            .iter()
            .map(|t| nostr::Tag::parse(t.iter().copied()).unwrap())
            .collect();
        nostr::EventBuilder::new(nostr::Kind::Custom(kind as u16), content)
            .tags(nostr_tags)
            .sign_with_keys(&keys)
            .unwrap()
    }

    #[test]
    fn count_e_tags_includes_malformed() {
        // A deletion event with one valid e-tag and one malformed e-tag
        // should count as 2 e-tags (and be rejected by the "exactly 1" check).
        let event = make_event_with_tags(
            5, // kind:5 deletion
            "",
            &[&["e", "a".repeat(64).as_str()], &["e", "not-valid-hex"]],
        );
        assert_eq!(count_e_tags(&event), 2);
    }

    #[test]
    fn count_e_tags_single_valid() {
        let event = make_event_with_tags(5, "", &[&["e", "a".repeat(64).as_str()]]);
        assert_eq!(count_e_tags(&event), 1);
    }

    fn make_engram(tags: &[&[&str]], content: &str) -> Event {
        make_event_with_tags(KIND_AGENT_ENGRAM, content, tags)
    }

    /// Minimal syntactically-plausible NIP-44 v2 payload (99 zero-filled bytes
    /// with the 0x02 version prefix). Real ciphertexts are larger and have real
    /// MACs; the relay only checks shape, not authenticity.
    fn fake_nip44_v2() -> String {
        // base64(b"\x02" + b"\x00" * 98) — 132 chars, decoded length 99,
        // first byte 0x02.
        let mut s = String::from("Ag");
        s.push_str(&"A".repeat(130));
        s
    }

    #[test]
    fn engram_envelope_accepts_canonical() {
        let d = "a".repeat(64);
        let p = "b".repeat(64);
        let ev = make_engram(&[&["d", &d], &["p", &p]], &fake_nip44_v2());
        assert!(validate_engram_envelope(&ev).is_ok());
    }

    #[test]
    fn engram_envelope_rejects_missing_p() {
        let d = "a".repeat(64);
        let ev = make_engram(&[&["d", &d]], &fake_nip44_v2());
        let err = validate_engram_envelope(&ev).unwrap_err();
        assert!(err.contains("`p` tag"), "got: {err}");
    }

    #[test]
    fn engram_envelope_rejects_duplicate_p() {
        let d = "a".repeat(64);
        let p = "b".repeat(64);
        let ev = make_engram(&[&["d", &d], &["p", &p], &["p", &p]], &fake_nip44_v2());
        let err = validate_engram_envelope(&ev).unwrap_err();
        assert!(err.contains("`p` tag"), "got: {err}");
    }

    #[test]
    fn engram_envelope_rejects_short_d() {
        let p = "b".repeat(64);
        let ev = make_engram(&[&["d", "abcd"], &["p", &p]], &fake_nip44_v2());
        let err = validate_engram_envelope(&ev).unwrap_err();
        assert!(err.contains("`d` tag"), "got: {err}");
    }

    #[test]
    fn engram_envelope_rejects_uppercase_d() {
        let p = "b".repeat(64);
        // 64 chars but uppercase — spec mandates lowercase hex.
        let d = "A".repeat(64);
        let ev = make_engram(&[&["d", &d], &["p", &p]], &fake_nip44_v2());
        let err = validate_engram_envelope(&ev).unwrap_err();
        assert!(err.contains("`d` tag"), "got: {err}");
    }

    /// Regression: uppercase `p` tag must be rejected at ingest. Readers query
    /// `#p` lowercase; an uppercase-tagged event that wins NIP-33 replacement
    /// becomes invisible to readers, silently bricking the slug.
    #[test]
    fn engram_envelope_rejects_uppercase_p() {
        let d = "a".repeat(64);
        let p = "B".repeat(64);
        let ev = make_engram(&[&["d", &d], &["p", &p]], &fake_nip44_v2());
        let err = validate_engram_envelope(&ev).unwrap_err();
        assert!(err.contains("`p` tag"), "got: {err}");
    }

    #[test]
    fn engram_envelope_rejects_short_p() {
        let d = "a".repeat(64);
        let ev = make_engram(&[&["d", &d], &["p", "abcd"]], &fake_nip44_v2());
        let err = validate_engram_envelope(&ev).unwrap_err();
        assert!(err.contains("`p` tag"), "got: {err}");
    }

    #[test]
    fn engram_envelope_rejects_empty_content() {
        let d = "a".repeat(64);
        let p = "b".repeat(64);
        let ev = make_engram(&[&["d", &d], &["p", &p]], "");
        let err = validate_engram_envelope(&ev).unwrap_err();
        assert!(err.contains("content"), "got: {err}");
    }

    /// Regression: non-base64 content must be rejected. Otherwise a signed
    /// event with `content="x"` wins NIP-33 replacement against a valid head,
    /// and the new head is then skipped by `validate_and_decrypt` — making the
    /// slug appear absent to readers.
    #[test]
    fn engram_envelope_rejects_non_base64_content() {
        let d = "a".repeat(64);
        let p = "b".repeat(64);
        let ev = make_engram(&[&["d", &d], &["p", &p]], "x");
        let err = validate_engram_envelope(&ev).unwrap_err();
        assert!(
            err.contains("base64") || err.contains("too short"),
            "got: {err}"
        );
    }

    #[test]
    fn engram_envelope_rejects_wrong_nip44_version() {
        // 99 bytes of valid base64 alphabet, but first byte decodes to 0x00,
        // not the NIP-44 v2 prefix 0x02. Length OK (132 chars / 99 decoded).
        let d = "a".repeat(64);
        let p = "b".repeat(64);
        let bad = "A".repeat(132);
        let ev = make_engram(&[&["d", &d], &["p", &p]], &bad);
        let err = validate_engram_envelope(&ev).unwrap_err();
        assert!(
            err.contains("NIP-44 v2") || err.contains("0x02"),
            "got: {err}"
        );
    }

    #[test]
    fn engram_envelope_rejects_short_content() {
        // Base64 of "Ag==" decodes to 1 byte — version prefix correct but
        // way under the 99-byte floor.
        let d = "a".repeat(64);
        let p = "b".repeat(64);
        let ev = make_engram(&[&["d", &d], &["p", &p]], "Ag==");
        let err = validate_engram_envelope(&ev).unwrap_err();
        assert!(err.contains("too short"), "got: {err}");
    }

    #[test]
    fn engram_envelope_rejects_bad_base64_alphabet() {
        let d = "a".repeat(64);
        let p = "b".repeat(64);
        // Contains '!' which is not in the standard base64 alphabet. Length is
        // a multiple of 4 to defeat the length check.
        let bad = format!("Ag!!{}", "A".repeat(128));
        let ev = make_engram(&[&["d", &d], &["p", &p]], &bad);
        let err = validate_engram_envelope(&ev).unwrap_err();
        assert!(err.contains("base64"), "got: {err}");
    }

    #[test]
    fn not_before_accepts_zero() {
        assert_eq!(validate_not_before("0"), Ok(0));
    }

    #[test]
    fn not_before_accepts_typical_timestamp() {
        assert_eq!(validate_not_before("1717000000"), Ok(1_717_000_000));
    }

    #[test]
    fn not_before_accepts_max_safe_integer() {
        assert_eq!(
            validate_not_before("9007199254740991"),
            Ok(9_007_199_254_740_991)
        );
    }

    #[test]
    fn not_before_rejects_above_max_safe_integer() {
        assert_eq!(
            validate_not_before("9007199254740992"),
            Err("malformed not_before")
        );
    }

    #[test]
    fn not_before_rejects_leading_zero() {
        assert_eq!(validate_not_before("007"), Err("malformed not_before"));
    }

    #[test]
    fn not_before_rejects_empty() {
        assert_eq!(validate_not_before(""), Err("malformed not_before"));
    }

    #[test]
    fn not_before_rejects_non_digits() {
        // Sign, whitespace, decimal point, and non-decimal forms are all
        // rejected — only ASCII decimal digits are valid.
        for value in ["-1", "+1", " 1", "1 ", "1.0", "1e3", "0x10", "abc"] {
            assert_eq!(
                validate_not_before(value),
                Err("malformed not_before"),
                "value {value:?} should be malformed"
            );
        }
    }

    #[test]
    fn not_before_rejects_u64_overflow() {
        // Exceeds u64::MAX — `from_str` errors rather than wrapping, so the
        // value is malformed (not a lossy round-trip).
        assert_eq!(
            validate_not_before("99999999999999999999999999"),
            Err("malformed not_before")
        );
    }

    fn make_reminder(tags: &[&[&str]]) -> Event {
        make_event_with_tags(KIND_EVENT_REMINDER, "ciphertext", tags)
    }

    #[test]
    fn reminder_accepts_single_valid_not_before() {
        let ev = make_reminder(&[&["d", "abc"], &["not_before", "1717000000"]]);
        assert!(validate_event_reminder(&ev).is_ok());
    }

    #[test]
    fn reminder_accepts_expiration_after_not_before() {
        let ev = make_reminder(&[
            &["d", "abc"],
            &["not_before", "1717000000"],
            &["expiration", "1717000001"],
        ]);
        assert!(validate_event_reminder(&ev).is_ok());
    }

    #[test]
    fn reminder_accepts_missing_not_before() {
        // Terminal states (done/cancelled) and bookmarks omit not_before
        let ev = make_reminder(&[&["d", "abc"]]);
        assert!(validate_event_reminder(&ev).is_ok());
    }

    #[test]
    fn reminder_rejects_not_before_too_far_in_future() {
        // `not_before` beyond the max horizon (default 1 year) is rejected.
        let far_future = (chrono::Utc::now().timestamp() as u64) + 63_072_000; // ~2 years
        let ev = make_reminder(&[&["d", "abc"], &["not_before", &far_future.to_string()]]);
        assert_eq!(
            validate_event_reminder(&ev),
            Err("not_before too far in future")
        );
    }

    #[test]
    fn reminder_rejects_duplicate_not_before() {
        let ev = make_reminder(&[
            &["d", "abc"],
            &["not_before", "1717000000"],
            &["not_before", "1717000005"],
        ]);
        assert_eq!(validate_event_reminder(&ev), Err("malformed not_before"));
    }

    #[test]
    fn reminder_rejects_malformed_not_before() {
        let ev = make_reminder(&[&["d", "abc"], &["not_before", "007"]]);
        assert_eq!(validate_event_reminder(&ev), Err("malformed not_before"));
    }

    #[test]
    fn reminder_rejects_expiration_equal_to_not_before() {
        let ev = make_reminder(&[
            &["d", "abc"],
            &["not_before", "1717000000"],
            &["expiration", "1717000000"],
        ]);
        assert_eq!(
            validate_event_reminder(&ev),
            Err("expiration before not_before")
        );
    }

    #[test]
    fn reminder_rejects_expiration_before_not_before() {
        let ev = make_reminder(&[
            &["d", "abc"],
            &["not_before", "1717000000"],
            &["expiration", "1716000000"],
        ]);
        assert_eq!(
            validate_event_reminder(&ev),
            Err("expiration before not_before")
        );
    }

    #[test]
    fn reminder_ignores_malformed_expiration() {
        // A malformed `expiration` is NIP-40's concern, not this validator's:
        // the ordering check runs only when `expiration` parses, so a valid
        // `not_before` with an unparseable expiration is accepted here.
        let ev = make_reminder(&[
            &["d", "abc"],
            &["not_before", "1717000000"],
            &["expiration", "notanumber"],
        ]);
        assert!(validate_event_reminder(&ev).is_ok());
    }

    #[test]
    fn event_reminder_is_global_only_and_param_replaceable() {
        assert!(is_global_only_kind(KIND_EVENT_REMINDER));
        assert!(!requires_h_channel_scope(KIND_EVENT_REMINDER));
        assert!(is_parameterized_replaceable(KIND_EVENT_REMINDER));
    }

    #[test]
    fn reminder_accepts_expiration_without_not_before() {
        // A terminal/bookmark with expiration but no not_before is valid —
        // no ordering check applies when not_before is absent.
        let ev = make_reminder(&[&["d", "abc"], &["expiration", "1777542730"]]);
        assert!(validate_event_reminder(&ev).is_ok());
    }

    #[test]
    fn reminder_rejects_missing_d_tag() {
        let ev = make_event_with_tags(
            KIND_EVENT_REMINDER,
            "ciphertext",
            &[&["not_before", "1717000000"]],
        );
        assert_eq!(validate_event_reminder(&ev), Err("missing d tag"));
    }

    #[test]
    fn reminder_rejects_empty_d_tag() {
        let ev = make_event_with_tags(
            KIND_EVENT_REMINDER,
            "ciphertext",
            &[&["d", ""], &["not_before", "1717000000"]],
        );
        assert_eq!(validate_event_reminder(&ev), Err("empty d tag"));
    }

    #[test]
    fn reminder_rejects_duplicate_d_tag() {
        let ev = make_event_with_tags(
            KIND_EVENT_REMINDER,
            "ciphertext",
            &[&["d", "abc"], &["d", "def"], &["not_before", "1717000000"]],
        );
        assert_eq!(validate_event_reminder(&ev), Err("duplicate d tag"));
    }

    fn make_persona(tags: &[&[&str]]) -> Event {
        make_event_with_tags(
            KIND_PERSONA,
            r#"{"display_name":"x","system_prompt":"y"}"#,
            tags,
        )
    }

    #[test]
    fn persona_envelope_accepts_valid_slug() {
        let ev = make_persona(&[&["d", "my-persona-1"]]);
        assert!(validate_persona_envelope(&ev).is_ok());
    }

    #[test]
    fn persona_envelope_accepts_promptless_content() {
        // Unified agent model: system_prompt is optional — a definition can be
        // pure configuration. The relay validates only the envelope, so a
        // prompt-less body must ingest identically to a full one.
        let ev = make_event_with_tags(
            KIND_PERSONA,
            r#"{"display_name":"config-only"}"#,
            &[&["d", "config-only"]],
        );
        assert!(validate_persona_envelope(&ev).is_ok());
    }

    #[test]
    fn persona_envelope_accepts_behavioral_fields() {
        // Unknown legacy fields in persona content remain relay-opaque;
        // unknown-field tolerance is the contract.
        let ev = make_event_with_tags(
            KIND_PERSONA,
            r#"{"display_name":"x","respond_to":"owner-only","respond_to_allowlist":[],"mcp_toolsets":"default","parallelism":2}"#,
            &[&["d", "behavioral"]],
        );
        assert!(validate_persona_envelope(&ev).is_ok());
    }

    #[test]
    fn persona_envelope_accepts_single_char() {
        let ev = make_persona(&[&["d", "a"]]);
        assert!(validate_persona_envelope(&ev).is_ok());
    }

    #[test]
    fn persona_envelope_accepts_max_length() {
        let slug = "a".repeat(64);
        let ev = make_persona(&[&["d", &slug]]);
        assert!(validate_persona_envelope(&ev).is_ok());
    }

    #[test]
    fn persona_envelope_rejects_missing_d_tag() {
        let ev = make_persona(&[]);
        let err = validate_persona_envelope(&ev).unwrap_err();
        assert!(err.contains("`d` tag"), "got: {err}");
    }

    #[test]
    fn persona_envelope_rejects_empty_d_tag() {
        let ev = make_persona(&[&["d", ""]]);
        let err = validate_persona_envelope(&ev).unwrap_err();
        assert!(err.contains("must not be empty"), "got: {err}");
    }

    #[test]
    fn persona_envelope_rejects_duplicate_d_tags() {
        let ev = make_persona(&[&["d", "slug-a"], &["d", "slug-b"]]);
        let err = validate_persona_envelope(&ev).unwrap_err();
        assert!(err.contains("`d` tag"), "got: {err}");
    }

    #[test]
    fn persona_envelope_rejects_valueless_d_tag() {
        // A lone ["d"] carries no value; it must fail as a missing value, not
        // be skipped as though the event had no `d` tag at all.
        let ev = make_persona(&[&["d"]]);
        let err = validate_persona_envelope(&ev).unwrap_err();
        assert!(err.contains("must not be empty"), "got: {err}");
    }

    #[test]
    fn persona_envelope_rejects_valueless_plus_valued_d_tags() {
        // Counting only tags with a value would see one `d` here and accept the
        // event, breaking the exactly-one rule.
        let ev = make_persona(&[&["d"], &["d", "slug-a"]]);
        let err = validate_persona_envelope(&ev).unwrap_err();
        assert!(err.contains("exactly one `d` tag"), "got: {err}");
    }

    #[test]
    fn persona_envelope_rejects_too_long() {
        let slug = "a".repeat(65);
        let ev = make_persona(&[&["d", &slug]]);
        let err = validate_persona_envelope(&ev).unwrap_err();
        assert!(err.contains("too long"), "got: {err}");
    }

    #[test]
    fn persona_envelope_rejects_uppercase() {
        let ev = make_persona(&[&["d", "My-Persona"]]);
        let err = validate_persona_envelope(&ev).unwrap_err();
        assert!(err.contains("`d` tag"), "got: {err}");
    }

    #[test]
    fn persona_envelope_rejects_leading_underscore() {
        let ev = make_persona(&[&["d", "_invalid"]]);
        let err = validate_persona_envelope(&ev).unwrap_err();
        assert!(err.contains("start with"), "got: {err}");
    }

    #[test]
    fn persona_envelope_rejects_leading_hyphen() {
        let ev = make_persona(&[&["d", "-invalid"]]);
        let err = validate_persona_envelope(&ev).unwrap_err();
        assert!(err.contains("start with"), "got: {err}");
    }

    #[test]
    fn persona_envelope_rejects_spaces() {
        let ev = make_persona(&[&["d", "has space"]]);
        let err = validate_persona_envelope(&ev).unwrap_err();
        assert!(err.contains("`d` tag"), "got: {err}");
    }

    #[test]
    fn persona_envelope_rejects_dots() {
        let ev = make_persona(&[&["d", "has.dot"]]);
        let err = validate_persona_envelope(&ev).unwrap_err();
        assert!(err.contains("`d` tag"), "got: {err}");
    }

    // ─── persona shared-tag envelope tests ───────────────────────────────────

    #[test]
    fn persona_envelope_accepts_shared_true() {
        // A persona event with exactly one ["shared","true"] tag must be accepted.
        let ev = make_persona(&[&["d", "my-persona"], &["shared", "true"]]);
        assert!(validate_persona_envelope(&ev).is_ok());
    }

    #[test]
    fn persona_envelope_accepts_no_shared_tag() {
        // The shared tag is optional; omitting it is the author-only default.
        let ev = make_persona(&[&["d", "my-persona"]]);
        assert!(validate_persona_envelope(&ev).is_ok());
    }

    #[test]
    fn persona_envelope_rejects_shared_false() {
        let ev = make_persona(&[&["d", "my-persona"], &["shared", "false"]]);
        let err = validate_persona_envelope(&ev).unwrap_err();
        assert!(
            err.contains("\"true\""),
            "expected 'true' in error, got: {err}"
        );
    }

    #[test]
    fn persona_envelope_rejects_shared_wrong_value() {
        let ev = make_persona(&[&["d", "my-persona"], &["shared", "yes"]]);
        let err = validate_persona_envelope(&ev).unwrap_err();
        assert!(
            err.contains("\"true\""),
            "expected 'true' in error, got: {err}"
        );
    }

    #[test]
    fn persona_envelope_rejects_shared_missing_value() {
        // A "shared" tag with no value argument must be rejected.
        let ev = make_event_with_tags(
            KIND_PERSONA,
            r#"{"display_name":"x"}"#,
            &[&["d", "slug"], &["shared"]],
        );
        let err = validate_persona_envelope(&ev).unwrap_err();
        assert!(
            err.contains("\"true\""),
            "expected 'true' in error, got: {err}"
        );
    }

    #[test]
    fn persona_envelope_rejects_duplicate_shared_tags() {
        // More than one shared tag, even if both are "true", must be rejected.
        let ev = make_persona(&[
            &["d", "my-persona"],
            &["shared", "true"],
            &["shared", "true"],
        ]);
        let err = validate_persona_envelope(&ev).unwrap_err();
        assert!(
            err.contains("at most one"),
            "expected 'at most one' in error, got: {err}"
        );
    }

    #[test]
    fn persona_envelope_rejects_shared_three_elements() {
        // ["shared","true","extra"] must be rejected — only exactly two elements
        // are valid so the SQL containment check tags @> '[["shared","true"]]'
        // cannot match a three-element stored tag.
        let ev = make_event_with_tags(
            KIND_PERSONA,
            r#"{"display_name":"x"}"#,
            &[&["d", "slug"], &["shared", "true", "extra"]],
        );
        let err = validate_persona_envelope(&ev).unwrap_err();
        assert!(
            err.contains("[\"shared\",\"true\"]"),
            "expected exact-shape error, got: {err}"
        );
    }

    // ─── team-catalog (30178) envelope tests ─────────────────────────────────

    fn make_team_catalog(tags: &[&[&str]]) -> Event {
        make_event_with_tags(
            KIND_TEAM_CATALOG,
            r#"{"v":1,"name":"Team","members":[]}"#,
            tags,
        )
    }

    #[test]
    fn team_catalog_envelope_accepts_uuid_d_tag() {
        let ev = make_team_catalog(&[&["d", "0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0"]]);
        assert!(validate_team_catalog_envelope(&ev).is_ok());
    }

    #[test]
    fn team_catalog_envelope_accepts_builtin_colon_d_tag() {
        // Built-in team ids carry a colon (`builtin-team:welcome`), which the
        // persona slug grammar forbids. The catalog `d` tag must accept them so
        // a built-in team can be shared under its real local id.
        let ev = make_team_catalog(&[&["d", "builtin-team:welcome"]]);
        assert!(validate_team_catalog_envelope(&ev).is_ok());
    }

    #[test]
    fn team_catalog_envelope_accepts_shared_true() {
        let ev = make_team_catalog(&[&["d", "team-1"], &["shared", "true"]]);
        assert!(validate_team_catalog_envelope(&ev).is_ok());
    }

    #[test]
    fn team_catalog_envelope_rejects_missing_d_tag() {
        let ev = make_team_catalog(&[]);
        let err = validate_team_catalog_envelope(&ev).unwrap_err();
        assert!(err.contains("exactly one `d` tag"), "got: {err}");
    }

    #[test]
    fn team_catalog_envelope_rejects_empty_d_tag() {
        // An empty d-tag collapses every team into the (pubkey, 30178, "") slot.
        let ev = make_team_catalog(&[&["d", ""]]);
        let err = validate_team_catalog_envelope(&ev).unwrap_err();
        assert!(err.contains("must not be empty"), "got: {err}");
    }

    #[test]
    fn team_catalog_envelope_rejects_duplicate_d_tags() {
        let ev = make_team_catalog(&[&["d", "team-1"], &["d", "team-2"]]);
        let err = validate_team_catalog_envelope(&ev).unwrap_err();
        assert!(err.contains("exactly one `d` tag"), "got: {err}");
    }

    #[test]
    fn team_catalog_envelope_rejects_valueless_d_tag() {
        // A lone ["d"] carries no value; it must fail as a missing value, not
        // be skipped as though the event had no `d` tag at all.
        let ev = make_team_catalog(&[&["d"]]);
        let err = validate_team_catalog_envelope(&ev).unwrap_err();
        assert!(err.contains("must not be empty"), "got: {err}");
    }

    #[test]
    fn team_catalog_envelope_rejects_valueless_plus_valued_d_tags() {
        // Counting only tags with a value would see one `d` here and accept the
        // event. A NIP-33 consumer that reads ["d"] as an empty-valued first
        // `d` tag would then address this event at "" where we address it at
        // "team-1".
        let ev = make_team_catalog(&[&["d"], &["d", "team-1"]]);
        let err = validate_team_catalog_envelope(&ev).unwrap_err();
        assert!(err.contains("exactly one `d` tag"), "got: {err}");
    }

    #[test]
    fn team_catalog_envelope_bounds_d_tag_by_chars_not_bytes() {
        // 64 multi-byte characters is 192 bytes; the documented bound is
        // characters, so this must be accepted.
        let d = "é".repeat(64);
        assert!(d.len() > 64, "fixture must exceed the bound in bytes");
        let ev = make_team_catalog(&[&["d", &d]]);
        assert!(validate_team_catalog_envelope(&ev).is_ok());
    }

    #[test]
    fn team_catalog_envelope_rejects_too_long_d_tag() {
        let d = "a".repeat(65);
        let ev = make_team_catalog(&[&["d", &d]]);
        let err = validate_team_catalog_envelope(&ev).unwrap_err();
        assert!(err.contains("too long"), "got: {err}");
    }

    #[test]
    fn team_catalog_envelope_accepts_max_length_d_tag() {
        let d = "a".repeat(64);
        let ev = make_team_catalog(&[&["d", &d]]);
        assert!(validate_team_catalog_envelope(&ev).is_ok());
    }

    #[test]
    fn team_catalog_envelope_rejects_whitespace_d_tag() {
        // A newline in the d-tag would break the NIP-33 coordinate and any
        // line-oriented log consumer.
        let ev = make_team_catalog(&[&["d", "team\n1"]]);
        let err = validate_team_catalog_envelope(&ev).unwrap_err();
        assert!(err.contains("control characters"), "got: {err}");
    }

    #[test]
    fn team_catalog_envelope_rejects_shared_false() {
        let ev = make_team_catalog(&[&["d", "team-1"], &["shared", "false"]]);
        let err = validate_team_catalog_envelope(&ev).unwrap_err();
        assert!(err.contains("\"true\""), "got: {err}");
    }

    #[test]
    fn team_catalog_envelope_rejects_shared_three_elements() {
        // Same exact-shape rule as personas: a three-element tag would match the
        // SQL containment clause `tags @> '[["shared","true"]]'` as a superset.
        let ev = make_team_catalog(&[&["d", "team-1"], &["shared", "true", "extra"]]);
        let err = validate_team_catalog_envelope(&ev).unwrap_err();
        assert!(err.contains("[\"shared\",\"true\"]"), "got: {err}");
    }

    #[test]
    fn team_catalog_envelope_rejects_duplicate_shared_tags() {
        let ev = make_team_catalog(&[&["d", "team-1"], &["shared", "true"], &["shared", "true"]]);
        let err = validate_team_catalog_envelope(&ev).unwrap_err();
        assert!(err.contains("at most one"), "got: {err}");
    }

    #[test]
    fn team_catalog_is_in_scope_allowlist() {
        let dummy = make_dummy_event();
        assert_eq!(
            required_scope_for_kind(KIND_TEAM_CATALOG, &dummy).unwrap(),
            Scope::UsersWrite,
        );
    }

    #[test]
    fn team_catalog_is_global_only() {
        assert!(is_global_only_kind(KIND_TEAM_CATALOG));
        assert!(!requires_h_channel_scope(KIND_TEAM_CATALOG));
    }

    // ─── repo announcement (kind:30617) project-tag tests (NIP-MP phase 2) ───

    #[test]
    fn repo_project_tag_absent_is_ok() {
        let ev = make_event_with_tags(KIND_GIT_REPO_ANNOUNCEMENT, "", &[&["d", "repo"]]);
        assert_eq!(validate_repo_announcement_project_tag(&ev).unwrap(), None);
    }

    #[test]
    fn repo_project_tag_valid_is_normalized() {
        let coord = format!("30621:{OWNER_A}:platform");
        let upper = format!("30621:{}:platform", OWNER_A.to_ascii_uppercase());
        for (value, expect) in [(coord.clone(), coord.clone()), (upper, coord)] {
            let ev = make_event_with_tags(
                KIND_GIT_REPO_ANNOUNCEMENT,
                "",
                &[&["d", "repo"], &["project", &value]],
            );
            assert_eq!(
                validate_repo_announcement_project_tag(&ev).unwrap(),
                Some(expect.clone()),
                "value {value:?}"
            );
        }
    }

    #[test]
    fn repo_project_tag_rejects_malformed_coordinate() {
        for bad in [
            "junk",
            "30621:short:x",
            "30622:aaaa:x",
            "",
            // 30178 is the team-catalog kind on this relay, not a project.
            "30178:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa:x",
        ] {
            let ev = make_event_with_tags(
                KIND_GIT_REPO_ANNOUNCEMENT,
                "",
                &[&["d", "repo"], &["project", bad]],
            );
            let err = validate_repo_announcement_project_tag(&ev).unwrap_err();
            assert!(err.contains("project tag"), "value {bad:?} got: {err}");
        }
    }

    #[test]
    fn repo_project_tag_rejects_duplicates_and_bad_arity() {
        let coord = format!("30621:{OWNER_A}:platform");
        let dup = make_event_with_tags(
            KIND_GIT_REPO_ANNOUNCEMENT,
            "",
            &[&["d", "repo"], &["project", &coord], &["project", &coord]],
        );
        let err = validate_repo_announcement_project_tag(&dup).unwrap_err();
        assert!(err.contains("at most one"), "got: {err}");

        let arity = make_event_with_tags(
            KIND_GIT_REPO_ANNOUNCEMENT,
            "",
            &[&["d", "repo"], &["project", &coord, "extra"]],
        );
        let err = validate_repo_announcement_project_tag(&arity).unwrap_err();
        assert!(err.contains("elements"), "got: {err}");
    }

    // ─── project (NIP-MP kind:30621) envelope tests ──────────────────────────

    const OWNER_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const OWNER_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn make_project(tags: &[&[&str]]) -> Event {
        make_event_with_tags(KIND_PROJECT, "", tags)
    }

    fn member_coord(owner: &str, repo_d: &str) -> String {
        format!("30617:{owner}:{repo_d}")
    }

    #[test]
    fn project_envelope_accepts_minimal() {
        let ev = make_project(&[&["d", "platform"]]);
        assert!(validate_project_envelope(&ev).is_ok());
    }

    #[test]
    fn project_envelope_accepts_full_cross_owner_membership() {
        // The motivating case: one project spanning two owners' repositories.
        let a = member_coord(OWNER_A, "buzz");
        let b = member_coord(OWNER_B, "buzz-infra");
        let ev = make_project(&[
            &["d", "platform"],
            &["name", "Platform"],
            &["description", "Relay, desktop, and mobile."],
            &["a", &a],
            &["a", &b],
            &["buzz-channel", "3580ca9b-47b4-4af9-b22a-1068778f26c6"],
            &["buzz-visibility", "listed"],
        ]);
        assert!(validate_project_envelope(&ev).is_ok());
    }

    #[test]
    fn project_envelope_accepts_zero_members() {
        // Legal at the protocol layer: the natural state after removing a final
        // member. The create UI requires >= 1; the relay must not.
        let ev = make_project(&[&["d", "empty"], &["name", "Empty"]]);
        assert!(validate_project_envelope(&ev).is_ok());
    }

    #[test]
    fn project_envelope_accepts_same_repo_d_under_two_owners() {
        // The NIP-34 fork case. Identity is the whole coordinate, so these are
        // two distinct members, not a duplicate.
        let a = member_coord(OWNER_A, "buzz");
        let b = member_coord(OWNER_B, "buzz");
        let ev = make_project(&[&["d", "forks"], &["a", &a], &["a", &b]]);
        assert!(validate_project_envelope(&ev).is_ok());
    }

    #[test]
    fn project_envelope_accepts_member_repo_d_containing_colon() {
        // Coordinates split on the first two colons only, matching NIP-09
        // deletion parsing, so a colon-bearing repository `d` stays addressable.
        let coord = member_coord(OWNER_A, "group:repo");
        let ev = make_project(&[&["d", "external"], &["a", &coord]]);
        assert!(validate_project_envelope(&ev).is_ok());
    }

    #[test]
    fn project_envelope_accepts_member_cap_boundary() {
        let coords: Vec<String> = (0..PROJECT_MEMBER_CAP)
            .map(|i| member_coord(OWNER_A, &format!("repo-{i}")))
            .collect();
        let mut tags: Vec<Vec<&str>> = vec![vec!["d", "wide"]];
        tags.extend(coords.iter().map(|c| vec!["a", c.as_str()]));
        let tag_refs: Vec<&[&str]> = tags.iter().map(|t| t.as_slice()).collect();
        let ev = make_project(&tag_refs);
        assert!(
            validate_project_envelope(&ev).is_ok(),
            "exactly {PROJECT_MEMBER_CAP} members must be accepted"
        );
    }

    #[test]
    fn project_envelope_ignores_unknown_tags() {
        // Forward compatibility: a newer writer's extra metadata must not
        // invalidate the event for this relay.
        let ev = make_project(&[&["d", "platform"], &["future-field", "whatever"]]);
        assert!(validate_project_envelope(&ev).is_ok());
    }

    #[test]
    fn project_envelope_rejects_missing_d_tag() {
        let ev = make_project(&[&["name", "No Identity"]]);
        let err = validate_project_envelope(&ev).unwrap_err();
        assert!(
            err.to_string().contains("exactly one `d` tag"),
            "got: {err}"
        );
    }

    #[test]
    fn project_envelope_rejects_multiple_d_tags() {
        let ev = make_project(&[&["d", "one"], &["d", "two"]]);
        let err = validate_project_envelope(&ev).unwrap_err();
        assert!(
            err.to_string().contains("exactly one `d` tag"),
            "got: {err}"
        );
    }

    #[test]
    fn project_envelope_rejects_empty_d_tag() {
        // An empty `d` collapses every such project into the (pubkey, 30621, "")
        // slot, where unrelated projects silently overwrite each other.
        let ev = make_project(&[&["d", ""]]);
        let err = validate_project_envelope(&ev).unwrap_err();
        assert!(err.to_string().contains("must not be empty"), "got: {err}");
    }

    #[test]
    fn project_envelope_rejects_valueless_d_tag() {
        // `["d"]` with no value is treated as empty, not as absent.
        let ev = make_project(&[&["d"]]);
        let err = validate_project_envelope(&ev).unwrap_err();
        assert!(err.to_string().contains("must not be empty"), "got: {err}");
    }

    #[test]
    fn project_envelope_rejects_duplicate_member_coordinate() {
        let coord = member_coord(OWNER_A, "buzz");
        let ev = make_project(&[&["d", "platform"], &["a", &coord], &["a", &coord]]);
        let err = validate_project_envelope(&ev).unwrap_err();
        assert!(
            err.to_string().contains("duplicate member coordinate"),
            "got: {err}"
        );
    }

    #[test]
    fn project_envelope_rejects_member_cap_exceeded() {
        let coords: Vec<String> = (0..=PROJECT_MEMBER_CAP)
            .map(|i| member_coord(OWNER_A, &format!("repo-{i}")))
            .collect();
        let mut tags: Vec<Vec<&str>> = vec![vec!["d", "wide"]];
        tags.extend(coords.iter().map(|c| vec!["a", c.as_str()]));
        let tag_refs: Vec<&[&str]> = tags.iter().map(|t| t.as_slice()).collect();
        let ev = make_project(&tag_refs);
        let err = validate_project_envelope(&ev).unwrap_err();
        assert!(err.to_string().contains("at most 64 member"), "got: {err}");
    }

    #[test]
    fn project_envelope_rejects_duplicate_heavy_list_on_cap_not_duplicate() {
        // The cap counts raw `a` tags, so a duplicate-heavy list is refused on
        // count — parse volume is never bounded only by the frame limit.
        let coord = member_coord(OWNER_A, "buzz");
        let mut tags: Vec<Vec<&str>> = vec![vec!["d", "wide"]];
        for _ in 0..=PROJECT_MEMBER_CAP {
            tags.push(vec!["a", coord.as_str()]);
        }
        let tag_refs: Vec<&[&str]> = tags.iter().map(|t| t.as_slice()).collect();
        let ev = make_project(&tag_refs);
        let err = validate_project_envelope(&ev).unwrap_err();
        assert!(
            err.to_string().contains("at most 64 member"),
            "cap must be evaluated before the duplicate set is built, got: {err}"
        );
    }

    #[test]
    fn project_envelope_rejects_member_wrong_kind_prefix() {
        // kind:30618 is repository *state*; a project groups announcements.
        let coord = format!("30618:{OWNER_A}:buzz");
        let ev = make_project(&[&["d", "platform"], &["a", &coord]]);
        let err = validate_project_envelope(&ev).unwrap_err();
        assert!(
            err.to_string().contains("member `a` tag must be"),
            "got: {err}"
        );
    }

    #[test]
    fn project_envelope_rejects_member_owner_not_hex() {
        let coord = member_coord(&"z".repeat(64), "buzz");
        let ev = make_project(&[&["d", "platform"], &["a", &coord]]);
        let err = validate_project_envelope(&ev).unwrap_err();
        assert!(
            err.to_string().contains("member `a` tag must be"),
            "got: {err}"
        );
    }

    #[test]
    fn project_envelope_rejects_member_owner_uppercase_hex() {
        // `#a` filter matching is byte-exact: an uppercase-owner head would be
        // invisible to the lowercase-coordinate queries every reader issues.
        let coord = member_coord(&"A".repeat(64), "buzz");
        let ev = make_project(&[&["d", "platform"], &["a", &coord]]);
        let err = validate_project_envelope(&ev).unwrap_err();
        assert!(
            err.to_string().contains("member `a` tag must be"),
            "got: {err}"
        );
    }

    #[test]
    fn project_envelope_rejects_member_owner_wrong_length() {
        let coord = member_coord(&"a".repeat(63), "buzz");
        let ev = make_project(&[&["d", "platform"], &["a", &coord]]);
        let err = validate_project_envelope(&ev).unwrap_err();
        assert!(
            err.to_string().contains("member `a` tag must be"),
            "got: {err}"
        );
    }

    #[test]
    fn project_envelope_rejects_member_empty_repo_d() {
        let coord = member_coord(OWNER_A, "");
        let ev = make_project(&[&["d", "platform"], &["a", &coord]]);
        let err = validate_project_envelope(&ev).unwrap_err();
        assert!(
            err.to_string().contains("member `a` tag must be"),
            "got: {err}"
        );
    }

    #[test]
    fn project_envelope_rejects_member_missing_segment() {
        let coord = format!("30617:{OWNER_A}");
        let ev = make_project(&[&["d", "platform"], &["a", &coord]]);
        let err = validate_project_envelope(&ev).unwrap_err();
        assert!(
            err.to_string().contains("member `a` tag must be"),
            "got: {err}"
        );
    }

    #[test]
    fn project_envelope_rejects_valueless_member_tag() {
        // A one-element `a` tag names no coordinate — caught by the arity check
        // (rule 4) before the coordinate parse (rule 5) even runs.
        let ev = make_project(&[&["d", "platform"], &["a"]]);
        let err = validate_project_envelope(&ev).unwrap_err();
        assert!(
            err.to_string().contains("exactly 2 or 3 elements"),
            "got: {err}"
        );
    }

    #[test]
    fn project_envelope_rejects_duplicate_metadata_tags() {
        // Every singleton metadata tag is bounded: a duplicate would make the
        // effective value reader-dependent.
        for tag_name in PROJECT_SINGLETON_METADATA_TAGS {
            let ev = make_project(&[&["d", "platform"], &[tag_name, "x"], &[tag_name, "y"]]);
            let err = validate_project_envelope(&ev).unwrap_err();
            assert!(
                err.to_string()
                    .contains(&format!("at most one `{tag_name}` tag")),
                "duplicate `{tag_name}` must be rejected, got: {err}"
            );
        }
    }

    #[test]
    fn project_envelope_rejects_name_too_long() {
        let name = "x".repeat(PROJECT_NAME_MAX_LEN + 1);
        let ev = make_project(&[&["d", "platform"], &["name", &name]]);
        let err = validate_project_envelope(&ev).unwrap_err();
        assert!(
            err.to_string().contains("`name` tag too long"),
            "got: {err}"
        );
    }

    #[test]
    fn project_envelope_accepts_name_at_max_length() {
        let name = "x".repeat(PROJECT_NAME_MAX_LEN);
        let ev = make_project(&[&["d", "platform"], &["name", &name]]);
        assert!(validate_project_envelope(&ev).is_ok());
    }

    #[test]
    fn project_envelope_rejects_description_too_long() {
        let description = "x".repeat(PROJECT_DESCRIPTION_MAX_LEN + 1);
        let ev = make_project(&[&["d", "platform"], &["description", &description]]);
        let err = validate_project_envelope(&ev).unwrap_err();
        assert!(
            err.to_string().contains("`description` tag too long"),
            "got: {err}"
        );
    }

    #[test]
    fn project_envelope_accepts_description_at_max_length() {
        let description = "x".repeat(PROJECT_DESCRIPTION_MAX_LEN);
        let ev = make_project(&[&["d", "platform"], &["description", &description]]);
        assert!(validate_project_envelope(&ev).is_ok());
    }

    #[test]
    fn project_envelope_bounds_icon_and_color_but_not_their_values() {
        // Both are client-interpreted display hints: any value within the
        // byte cap passes (an unrecognized color reads as unset client-side),
        // but an unbounded value must not ride into storage.
        let ev = make_project(&[&["d", "platform"], &["icon", "🐝"], &["color", "#3b82f6"]]);
        assert!(validate_project_envelope(&ev).is_ok());
        let odd = make_project(&[&["d", "platform"], &["color", "not-a-color"]]);
        assert!(
            validate_project_envelope(&odd).is_ok(),
            "the relay must not interpret color values"
        );
        for tag_name in ["icon", "color"] {
            let long = "x".repeat(PROJECT_METADATA_TAG_MAX_LEN + 1);
            let ev = make_project(&[&["d", "platform"], &[tag_name, &long]]);
            let err = validate_project_envelope(&ev).unwrap_err();
            assert!(
                err.to_string()
                    .contains(&format!("`{tag_name}` tag too long")),
                "got: {err}"
            );
        }
    }

    /// Membership is an assertion, not a permission grant: the relay must accept
    /// a project naming a repository the signer does not own. Cross-owner
    /// grouping is the entire point of the kind, and it is safe precisely because
    /// membership confers nothing.
    #[test]
    fn project_envelope_accepts_member_owned_by_another_pubkey() {
        let stranger = member_coord(OWNER_B, "not-mine");
        let ev = make_project(&[&["d", "collection"], &["a", &stranger]]);
        assert!(validate_project_envelope(&ev).is_ok());
    }

    #[test]
    fn project_is_in_scope_allowlist() {
        let dummy = make_dummy_event();
        assert_eq!(
            required_scope_for_kind(KIND_PROJECT, &dummy).unwrap(),
            Scope::ReposWrite,
            "a project is repository metadata — same scope as announcing a repo"
        );
    }

    #[test]
    fn project_is_global_only() {
        // `buzz-channel` is a metadata reference, not a routing directive.
        assert!(is_global_only_kind(KIND_PROJECT));
        assert!(!requires_h_channel_scope(KIND_PROJECT));
    }

    #[test]
    fn project_is_parameterized_replaceable() {
        // Owner-only editing comes free from NIP-33 addressing: replacement is
        // keyed by (pubkey, kind, d), so one signer can never overwrite another's
        // project. No relay-side permission check exists or is needed.
        assert!(is_parameterized_replaceable(KIND_PROJECT));
    }

    /// Drive every case in the shared NIP-MP fixture file against
    /// `validate_project_envelope`. All 15 accept cases must pass; all 27
    /// reject cases must return an error whose rule is in the case's allowed
    /// `reject_rules` set — an implementation cannot pass by rejecting for an
    /// unrelated reason. This is the machine-readable oracle the spec promises.
    #[test]
    fn project_envelope_validates_all_shared_fixtures() {
        #[derive(serde::Deserialize)]
        struct FixtureFile {
            cases: Vec<Case>,
        }
        #[derive(serde::Deserialize)]
        struct Case {
            name: String,
            expect: String,
            #[serde(default)]
            reject_rules: Vec<String>,
            template: Template,
        }
        #[derive(serde::Deserialize)]
        struct Template {
            content: String,
            tags: Vec<Vec<String>>,
        }

        let raw = include_str!("../../../../docs/nips/NIP-MP.fixtures.json");
        let file: FixtureFile = serde_json::from_str(raw).expect("fixture file must parse");

        for case in &file.cases {
            let tag_strs: Vec<Vec<&str>> = case
                .template
                .tags
                .iter()
                .map(|t| t.iter().map(|s| s.as_str()).collect())
                .collect();
            let tag_refs: Vec<&[&str]> = tag_strs.iter().map(|t| t.as_slice()).collect();
            let ev = make_event_with_tags(KIND_PROJECT, &case.template.content, &tag_refs);
            let result = validate_project_envelope(&ev);
            match case.expect.as_str() {
                "accept" => assert!(
                    result.is_ok(),
                    "fixture {:?} expected accept, got err: {:?}",
                    case.name,
                    result.unwrap_err()
                ),
                "reject" => {
                    let rejection = match result {
                        Err(r) => r,
                        Ok(()) => {
                            panic!("fixture {:?} expected reject, but was accepted", case.name)
                        }
                    };
                    assert!(
                        case.reject_rules.iter().any(|r| r == rejection.rule),
                        "fixture {:?} fired rule {:?}, which is not in allowed set {:?}",
                        case.name,
                        rejection.rule,
                        case.reject_rules,
                    );
                }
                other => panic!(
                    "unknown expect value {:?} in fixture {:?}",
                    other, case.name
                ),
            }
        }
    }

    // ─── Buzz container-extension tests (agents + channels as members) ───────

    const HEX64: &str = "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef";

    #[test]
    fn project_envelope_accepts_agent_and_channel_members() {
        // The Buzz container extension: agent-surface coordinates ride in the
        // same `a` member tags, and channels join via `channel` UUID tags.
        let repo = format!("30617:{HEX64}:my-repo");
        let persona = format!("30175:{HEX64}:helper");
        let team = format!("30176:{HEX64}:crew");
        let managed = format!("30177:{HEX64}:{HEX64}");
        let ev = make_project(&[
            &["d", "platform"],
            &["name", "Platform"],
            &["a", &repo],
            &["a", &persona],
            &["a", &team],
            &["a", &managed],
            &["channel", "6f7a4e6e-3f0e-4bfb-9a3e-27a2e5f6a111"],
        ]);
        assert!(validate_project_envelope(&ev).is_ok());
    }

    #[test]
    fn project_envelope_rejects_bad_channel_ref() {
        let ev = make_project(&[&["d", "p"], &["name", "P"], &["channel", "not-a-uuid"]]);
        let err = validate_project_envelope(&ev).unwrap_err();
        assert!(
            err.to_string().contains("UUID"),
            "expected UUID error, got: {err}"
        );
    }

    // ─── kind:30623 shared-terminal session announce (NIP-ST) ───────────────

    fn make_shell_session(tags: &[&[&str]]) -> Event {
        make_event_with_tags(buzz_core::kind::KIND_SHELL_SESSION, "", tags)
    }

    #[test]
    fn shell_session_envelope_accepts_well_formed_announce() {
        let coord = format!("30621:{HEX64}:platform");
        let ev = make_shell_session(&[
            &["d", "a4f6c8e0-1111-2222-3333-444455556666"],
            &["a", &coord],
            &["status", "open"],
            &["title", "build shell"],
            &["dims", "34x120"],
        ]);
        assert_eq!(validate_shell_session_envelope(&ev).expect("valid"), coord);
    }

    #[test]
    fn shell_session_envelope_requires_singleton_d_a_status() {
        let coord = format!("30621:{HEX64}:platform");
        let err = validate_shell_session_envelope(&make_shell_session(&[
            &["a", &coord],
            &["status", "open"],
        ]))
        .unwrap_err();
        assert!(err.contains("exactly one `d` tag"), "got: {err}");

        let err = validate_shell_session_envelope(&make_shell_session(&[
            &["d", "abc"],
            &["status", "open"],
        ]))
        .unwrap_err();
        assert!(err.contains("exactly one `a` tag"), "got: {err}");

        let err =
            validate_shell_session_envelope(&make_shell_session(&[&["d", "abc"], &["a", &coord]]))
                .unwrap_err();
        assert!(err.contains("exactly one `status` tag"), "got: {err}");
    }

    #[test]
    fn shell_session_envelope_rejects_unknown_status_fail_closed() {
        let coord = format!("30621:{HEX64}:platform");
        let err = validate_shell_session_envelope(&make_shell_session(&[
            &["d", "abc"],
            &["a", &coord],
            &["status", "sharing"],
        ]))
        .unwrap_err();
        assert!(err.contains("`status` must be"), "got: {err}");
    }

    #[test]
    fn shell_session_envelope_rejects_malformed_coordinate_id_and_dims() {
        let coord = format!("30621:{HEX64}:platform");

        let err = validate_shell_session_envelope(&make_shell_session(&[
            &["d", "abc"],
            &["a", &format!("30617:{HEX64}:repo")],
            &["status", "open"],
        ]))
        .unwrap_err();
        assert!(err.contains("kind"), "got: {err}");

        let err = validate_shell_session_envelope(&make_shell_session(&[
            &["d", "../escape"],
            &["a", &coord],
            &["status", "open"],
        ]))
        .unwrap_err();
        assert!(err.contains("session id"), "got: {err}");

        let err = validate_shell_session_envelope(&make_shell_session(&[
            &["d", "abc"],
            &["a", &coord],
            &["status", "open"],
            &["dims", "-1xhuge"],
        ]))
        .unwrap_err();
        assert!(err.contains("`dims` must be"), "got: {err}");
    }

    #[test]
    fn shell_session_envelope_bounds_title_and_content() {
        let coord = format!("30621:{HEX64}:platform");
        let long_title = "t".repeat(201);
        let err = validate_shell_session_envelope(&make_shell_session(&[
            &["d", "abc"],
            &["a", &coord],
            &["status", "open"],
            &["title", &long_title],
        ]))
        .unwrap_err();
        assert!(err.contains("`title` too long"), "got: {err}");

        let big = "x".repeat(4097);
        let ev = make_event_with_tags(
            buzz_core::kind::KIND_SHELL_SESSION,
            &big,
            &[&["d", "abc"], &["a", &coord], &["status", "open"]],
        );
        let err = validate_shell_session_envelope(&ev).unwrap_err();
        assert!(err.contains("content too large"), "got: {err}");
    }

    // ─── kind:9007/9002 `project` tag (channel↔project association) ─────────

    #[test]
    fn project_ref_tag_accepts_well_formed_coordinate() {
        let coord = format!("30621:{HEX64}:general");
        assert!(validate_project_ref_tag(&coord).is_ok());
    }

    #[test]
    fn project_ref_tag_rejects_wrong_kind() {
        // 30178 is the retired container number (now the team catalog); refs
        // must use the NIP-MP project kind.
        for wrong in ["30617", "30178"] {
            let coord = format!("{wrong}:{HEX64}:general");
            let err = validate_project_ref_tag(&coord).unwrap_err();
            assert!(err.contains("kind"), "expected kind error, got: {err}");
        }
    }

    #[test]
    fn project_ref_tag_rejects_non_integer_kind() {
        let coord = format!("abc:{HEX64}:general");
        assert!(validate_project_ref_tag(&coord).is_err());
    }

    #[test]
    fn project_ref_tag_rejects_short_pubkey() {
        let coord = "30621:deadbeef:general".to_string();
        let err = validate_project_ref_tag(&coord).unwrap_err();
        assert!(err.contains("pubkey"), "expected pubkey error, got: {err}");
    }

    #[test]
    fn project_ref_tag_rejects_non_hex_pubkey() {
        let non_hex = "g".repeat(64);
        let coord = format!("30621:{non_hex}:general");
        assert!(validate_project_ref_tag(&coord).is_err());
    }

    #[test]
    fn project_ref_tag_matches_envelope_d_rules() {
        // The ref check mirrors the envelope's `d` rules rather than the
        // client slug grammar: any project a relay would accept stays
        // referenceable by a channel.
        for ok in ["general", "UPPER", "has space", "dot.dot"] {
            let coord = format!("30621:{HEX64}:{ok}");
            assert!(
                validate_project_ref_tag(&coord).is_ok(),
                "slug {ok:?} should be accepted"
            );
        }
        for bad in ["", "line\nbreak"] {
            let coord = format!("30621:{HEX64}:{bad}");
            assert!(
                validate_project_ref_tag(&coord).is_err(),
                "slug {bad:?} should be rejected"
            );
        }
    }

    #[test]
    fn project_ref_tag_rejects_missing_parts() {
        for bad in ["30621", "30621:only-pubkey", "not-a-coordinate-at-all"] {
            assert!(
                validate_project_ref_tag(bad).is_err(),
                "malformed coordinate {bad:?} should be rejected"
            );
        }
    }

    // ─── agent_turn_metric envelope tests ────────────────────────────────────

    /// Build an event for kind:44200 with the given tags and content.
    /// The signing key IS the agent key, so `event.pubkey` matches the agent.
    fn make_agent_turn_metric(
        agent_keys: &nostr::Keys,
        tags: &[&[&str]],
        content: &str,
    ) -> nostr::Event {
        let nostr_tags: Vec<nostr::Tag> = tags
            .iter()
            .map(|t| nostr::Tag::parse(t.iter().copied()).unwrap())
            .collect();
        nostr::EventBuilder::new(
            nostr::Kind::Custom(buzz_core::kind::KIND_AGENT_TURN_METRIC as u16),
            content,
        )
        .tags(nostr_tags)
        .sign_with_keys(agent_keys)
        .unwrap()
    }

    #[test]
    fn agent_turn_metric_envelope_accepts_canonical() {
        let agent = nostr::Keys::generate();
        let owner_hex = "b".repeat(64);
        let agent_hex = agent.public_key().to_hex();
        let ev = make_agent_turn_metric(
            &agent,
            &[&["p", &owner_hex], &["agent", &agent_hex]],
            &fake_nip44_v2(),
        );
        assert!(validate_agent_turn_metric_envelope(&ev).is_ok());
    }

    #[test]
    fn agent_turn_metric_envelope_rejects_h_tag() {
        let agent = nostr::Keys::generate();
        let owner_hex = "b".repeat(64);
        let agent_hex = agent.public_key().to_hex();
        let ev = make_agent_turn_metric(
            &agent,
            &[
                &["p", &owner_hex],
                &["agent", &agent_hex],
                &["h", "some-channel-uuid"],
            ],
            &fake_nip44_v2(),
        );
        let err = validate_agent_turn_metric_envelope(&ev).unwrap_err();
        assert!(err.contains("`h` tag"), "got: {err}");
    }

    #[test]
    fn agent_turn_metric_envelope_rejects_missing_p() {
        let agent = nostr::Keys::generate();
        let agent_hex = agent.public_key().to_hex();
        let ev = make_agent_turn_metric(&agent, &[&["agent", &agent_hex]], &fake_nip44_v2());
        let err = validate_agent_turn_metric_envelope(&ev).unwrap_err();
        assert!(err.contains("`p` tag"), "got: {err}");
    }

    #[test]
    fn agent_turn_metric_envelope_rejects_missing_agent() {
        let agent = nostr::Keys::generate();
        let owner_hex = "b".repeat(64);
        let ev = make_agent_turn_metric(&agent, &[&["p", &owner_hex]], &fake_nip44_v2());
        let err = validate_agent_turn_metric_envelope(&ev).unwrap_err();
        assert!(err.contains("`agent` tag"), "got: {err}");
    }

    #[test]
    fn agent_turn_metric_envelope_rejects_agent_mismatch() {
        let agent = nostr::Keys::generate();
        let owner_hex = "b".repeat(64);
        let wrong_agent_hex = "c".repeat(64); // not event.pubkey
        let ev = make_agent_turn_metric(
            &agent,
            &[&["p", &owner_hex], &["agent", &wrong_agent_hex]],
            &fake_nip44_v2(),
        );
        let err = validate_agent_turn_metric_envelope(&ev).unwrap_err();
        assert!(err.contains("equal event pubkey"), "got: {err}");
    }

    #[test]
    fn agent_turn_metric_envelope_rejects_bad_content() {
        let agent = nostr::Keys::generate();
        let owner_hex = "b".repeat(64);
        let agent_hex = agent.public_key().to_hex();
        let ev = make_agent_turn_metric(
            &agent,
            &[&["p", &owner_hex], &["agent", &agent_hex]],
            "not-a-ciphertext",
        );
        let err = validate_agent_turn_metric_envelope(&ev).unwrap_err();
        // error comes from validate_engram_nip44_content with label replaced
        assert!(err.contains("agent-turn-metric"), "got: {err}");
    }

    /// The HTTP bridge's `submit_event` 400 arm and the WS `EVENT` handler's
    /// reject path must land on the same counter, distinguished only by the
    /// `transport` label — this is what lets a dashboard tell "server got
    /// hammered with bad HTTP requests" apart from "a WS client is
    /// misbehaving" without losing the combined total.
    #[test]
    fn reject_with_transport_labels_http_and_ws_as_separate_series() {
        let recorder = metrics_util::debugging::DebuggingRecorder::new();
        let snapshotter = recorder.snapshotter();

        metrics::with_local_recorder(&recorder, || {
            reject_with_transport("http", "invalid");
            reject_with_transport("ws", "invalid");
            reject_with_transport("http", "invalid");
        });

        let counts: std::collections::HashMap<(String, String), u64> = snapshotter
            .snapshot()
            .into_vec()
            .into_iter()
            .filter(|(key, ..)| key.key().name() == "buzz_events_rejected_total")
            .map(|(key, _, _, value)| {
                let metrics_util::debugging::DebugValue::Counter(n) = value else {
                    panic!("buzz_events_rejected_total must be a counter");
                };
                let labels: Vec<_> = key.key().labels().collect();
                let transport = labels
                    .iter()
                    .find(|l| l.key() == "transport")
                    .map(|l| l.value().to_owned())
                    .unwrap_or_default();
                let reason = labels
                    .iter()
                    .find(|l| l.key() == "reason")
                    .map(|l| l.value().to_owned())
                    .unwrap_or_default();
                ((transport, reason), n)
            })
            .collect();

        assert_eq!(
            counts.get(&("http".to_owned(), "invalid".to_owned())),
            Some(&2)
        );
        assert_eq!(
            counts.get(&("ws".to_owned(), "invalid".to_owned())),
            Some(&1)
        );
    }

    // ---- NIP-MP Pulse (kind:44240) ingest ----

    /// Build a well-formed 44240 for `keys`, then let the caller mutate the
    /// tag list to produce the malformed shapes under test.
    fn pulse_event_with_tags(keys: &nostr::Keys, tags: &[Vec<&str>]) -> Event {
        let content = serde_json::json!({
            "schema": buzz_core::pulse::PULSE_ENTRY_SCHEMA,
            "type": "plan",
            "text": "Refactoring session creation; pool.rs will churn.",
        })
        .to_string();
        let nostr_tags: Vec<nostr::Tag> = tags
            .iter()
            .map(|t| nostr::Tag::parse(t.iter().copied()).expect("tag"))
            .collect();
        nostr::EventBuilder::new(
            nostr::Kind::Custom(buzz_core::kind::KIND_PULSE_ENTRY as u16),
            content,
        )
        .tags(nostr_tags)
        .sign_with_keys(keys)
        .expect("sign pulse entry")
    }

    fn canonical_pulse_coordinate(keys: &nostr::Keys) -> String {
        format!("30621:{}:pulse-fixture", keys.public_key().to_hex())
    }

    #[test]
    fn pulse_entry_requires_messages_write_scope() {
        let dummy = make_dummy_event();
        assert_eq!(
            required_scope_for_kind(buzz_core::kind::KIND_PULSE_ENTRY, &dummy).unwrap(),
            Scope::MessagesWrite,
        );
    }

    /// The digest (39011) and the relay-signed summary (44242) stay
    /// client-unwritable by *omission* from `required_scope_for_kind`. A future
    /// match arm for either would silently make them stored client content.
    #[test]
    fn pulse_digest_and_summary_kinds_are_not_client_writable() {
        let dummy = make_dummy_event();
        for kind in [39011, 44242] {
            assert_eq!(
                required_scope_for_kind(kind, &dummy),
                Err("restricted: unknown event kind"),
                "kind {kind} must not be client-writable in Slice 1"
            );
        }
    }

    /// 44240 carries `h` only optionally, so it must not be forced global —
    /// otherwise an `h`-tagged entry would skip the channel-membership gate and
    /// project authorization would silently widen channel authorization.
    #[test]
    fn pulse_entry_is_neither_global_only_nor_h_required() {
        assert!(!is_global_only_kind(buzz_core::kind::KIND_PULSE_ENTRY));
        assert!(!requires_h_channel_scope(buzz_core::kind::KIND_PULSE_ENTRY));
    }

    #[test]
    fn pulse_entry_rejects_duplicate_singleton_tags() {
        let keys = nostr::Keys::generate();
        let coordinate = canonical_pulse_coordinate(&keys);
        let channel = uuid::Uuid::new_v4().to_string();
        let cases: Vec<Vec<Vec<&str>>> = vec![
            // duplicate required singletons
            vec![
                vec!["a", coordinate.as_str()],
                vec!["a", coordinate.as_str()],
                vec!["pu-v", buzz_core::pulse::PULSE_ENTRY_TAG_VERSION],
                vec!["pu-type", "plan"],
            ],
            vec![
                vec!["a", coordinate.as_str()],
                vec!["pu-v", buzz_core::pulse::PULSE_ENTRY_TAG_VERSION],
                vec!["pu-v", buzz_core::pulse::PULSE_ENTRY_TAG_VERSION],
                vec!["pu-type", "plan"],
            ],
            vec![
                vec!["a", coordinate.as_str()],
                vec!["pu-v", buzz_core::pulse::PULSE_ENTRY_TAG_VERSION],
                vec!["pu-type", "plan"],
                vec!["pu-type", "plan"],
            ],
            // duplicate optional singletons
            vec![
                vec!["a", coordinate.as_str()],
                vec!["pu-v", buzz_core::pulse::PULSE_ENTRY_TAG_VERSION],
                vec!["pu-type", "plan"],
                vec!["h", channel.as_str()],
                vec!["h", channel.as_str()],
            ],
            vec![
                vec!["a", coordinate.as_str()],
                vec!["pu-v", buzz_core::pulse::PULSE_ENTRY_TAG_VERSION],
                vec!["pu-type", "plan"],
                vec!["branch", "wip/pulse"],
                vec!["branch", "wip/pulse"],
            ],
        ];
        for tags in cases {
            let event = pulse_event_with_tags(&keys, &tags);
            assert!(
                buzz_core::pulse::validate_pulse_entry_envelope(&event).is_err(),
                "duplicate singleton tag must be rejected: {tags:?}"
            );
        }
    }

    #[test]
    fn pulse_entry_rejects_unknown_tag_key() {
        let keys = nostr::Keys::generate();
        let coordinate = canonical_pulse_coordinate(&keys);
        let event = pulse_event_with_tags(
            &keys,
            &[
                vec!["a", coordinate.as_str()],
                vec!["pu-v", buzz_core::pulse::PULSE_ENTRY_TAG_VERSION],
                vec!["pu-type", "plan"],
                vec!["e", &"a".repeat(64)],
            ],
        );
        assert!(buzz_core::pulse::validate_pulse_entry_envelope(&event).is_err());
    }

    /// A case-variant coordinate would dodge the ACL projection (which joins on
    /// string equality) and then be unfindable by any canonical `#a` query —
    /// the same rule and rationale as the NIP-MP membership ops.
    #[test]
    fn pulse_entry_rejects_non_canonical_coordinate() {
        let keys = nostr::Keys::generate();
        let upper = format!(
            "30621:{}:pulse-fixture",
            keys.public_key().to_hex().to_uppercase()
        );
        let event = pulse_event_with_tags(
            &keys,
            &[
                vec!["a", upper.as_str()],
                vec!["pu-v", buzz_core::pulse::PULSE_ENTRY_TAG_VERSION],
                vec!["pu-type", "plan"],
            ],
        );
        let err = buzz_core::pulse::validate_pulse_entry_envelope(&event)
            .expect_err("non-canonical coordinate must be rejected");
        assert!(err.contains("canonical"), "unexpected message: {err}");

        // The canonical form of the same project is accepted.
        let canonical = canonical_pulse_coordinate(&keys);
        let ok = pulse_event_with_tags(
            &keys,
            &[
                vec!["a", canonical.as_str()],
                vec!["pu-v", buzz_core::pulse::PULSE_ENTRY_TAG_VERSION],
                vec!["pu-type", "plan"],
            ],
        );
        assert!(buzz_core::pulse::validate_pulse_entry_envelope(&ok).is_ok());
    }

    /// `supersedes` is syntax-only at ingest: the relay performs no lookup, so
    /// `POST /events` never becomes an existence oracle and a supersession that
    /// arrives before its target under retry reordering still stores.
    #[test]
    fn pulse_entry_supersedes_is_syntactic_only() {
        let keys = nostr::Keys::generate();
        let coordinate = canonical_pulse_coordinate(&keys);
        let content = serde_json::json!({
            "schema": buzz_core::pulse::PULSE_ENTRY_SCHEMA,
            "type": "plan",
            "text": "Superseding an id this relay has never seen.",
            "supersedes": "b".repeat(64),
        })
        .to_string();
        let event = nostr::EventBuilder::new(
            nostr::Kind::Custom(buzz_core::kind::KIND_PULSE_ENTRY as u16),
            content,
        )
        .tags(vec![
            nostr::Tag::parse(["a", coordinate.as_str()]).expect("a"),
            nostr::Tag::parse(["pu-v", buzz_core::pulse::PULSE_ENTRY_TAG_VERSION]).expect("pu-v"),
            nostr::Tag::parse(["pu-type", "plan"]).expect("pu-type"),
        ])
        .sign_with_keys(&keys)
        .expect("sign");
        assert!(buzz_core::pulse::validate_pulse_entry_envelope(&event).is_ok());
    }

    /// The write-admission decision, over the two database facts ingest
    /// resolves. A read-only member of a **private** project cannot publish; a
    /// public fixture would make that assertion vacuous, since a public project
    /// admits any community member by design.
    #[test]
    fn pulse_write_admission_is_role_aware_and_refuses_unknown_projects() {
        use buzz_db::project_acl::{ProjectGate, ProjectRole};

        let owner = vec![1u8; 32];
        let collaborator = vec![2u8; 32];
        let viewer = vec![3u8; 32];
        let stranger = vec![4u8; 32];
        let gate = ProjectGate {
            owner: owner.clone(),
            members: vec![
                (collaborator.clone(), ProjectRole::Collaborator),
                (viewer.clone(), ProjectRole::Viewer),
            ],
        };

        assert_eq!(pulse_write_admitted(Some(&gate), true, &owner), Ok(()));
        assert_eq!(
            pulse_write_admitted(Some(&gate), true, &collaborator),
            Ok(())
        );
        assert_eq!(
            pulse_write_admitted(Some(&gate), true, &viewer),
            Err("restricted: project write access required"),
            "a read-only member of a private project reads the Pulse and never writes it"
        );
        assert_eq!(
            pulse_write_admitted(Some(&gate), true, &stranger),
            Err("restricted: project write access required")
        );

        // Public project (no gate row) that exists: any community member.
        assert_eq!(pulse_write_admitted(None, true, &stranger), Ok(()));
        // Coordinate no kind:30621 event ever created: refused, because an
        // unknown coordinate is in nobody's hidden set and the entry would
        // otherwise be shown to everyone.
        assert_eq!(
            pulse_write_admitted(None, false, &owner),
            Err("restricted: unknown project coordinate")
        );
    }

    // ---- Coding sessions ---------------------------------------------------

    /// Every coding-session kind, in kind order. Kept next to the tests that
    /// sweep it so the next kind lands in the sweep the moment it exists.
    const CODING_SESSION_TEST_KINDS: [u32; 15] = [
        KIND_CODING_SESSION_COMMAND,
        KIND_CODING_SESSION_LIFECYCLE_COMMAND,
        KIND_CODING_SESSION_PROVIDER_CATALOG,
        KIND_CODING_SESSION_METADATA,
        KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
        KIND_CODING_SESSION_TRANSCRIPT,
        KIND_CODING_SESSION_GENESIS,
        KIND_CODING_SESSION_GOAL,
        KIND_CODING_SESSION_AUTHORITY_TRANSITION,
        KIND_CODING_SESSION_NAME,
        KIND_CODING_SESSION_CLOSURE,
        KIND_CODING_SESSION_TEAM_TRANSACTION,
        KIND_CODING_SESSION_POLICY,
        KIND_CODING_SESSION_OBSERVATION,
        KIND_CODING_SESSION_HANDOVER,
    ];

    #[test]
    fn coding_session_predicate_covers_the_registered_session_kinds() {
        for kind in 0..=u16::MAX as u32 {
            assert_eq!(
                is_coding_session_kind(kind),
                (44220..=44230).contains(&kind)
                    || kind == KIND_CODING_SESSION_TEAM_TRANSACTION
                    || kind == KIND_CODING_SESSION_POLICY
                    || kind == KIND_CODING_SESSION_OBSERVATION
                    || kind == KIND_CODING_SESSION_HANDOVER,
                "is_coding_session_kind disagrees at kind {kind}"
            );
        }
        for kind in CODING_SESSION_TEST_KINDS {
            assert!(is_coding_session_kind(kind));
        }
    }

    /// All of them are channel-scoped message writes, and none is global-only
    /// — their whole containment story is the channel ACL, which only
    /// applies to h-scoped events.
    #[test]
    fn coding_session_kinds_are_channel_scoped_message_writes() {
        let dummy = make_dummy_event();
        for kind in CODING_SESSION_TEST_KINDS {
            assert_eq!(
                required_scope_for_kind(kind, &dummy).unwrap(),
                Scope::MessagesWrite,
                "kind {kind} must be a message write",
            );
            assert!(
                requires_h_channel_scope(kind),
                "kind {kind} must require an h tag",
            );
            assert!(
                !is_global_only_kind(kind),
                "kind {kind} must never be global-only",
            );
        }
    }

    /// Unlike `check_channel_membership`, this verdict has no visibility
    /// fallback: an outsider in an *open* channel is still denied. Reading a
    /// room is not authority to steer an agent inside it, nor to author the
    /// record of what that agent did.
    #[test]
    fn coding_session_membership_rejects_open_channel_outsider() {
        assert!(coding_session_membership_verdict(true).is_ok());
        let denial = coding_session_membership_verdict(false).unwrap_err();
        assert!(denial.starts_with("restricted:"), "got {denial:?}");
    }

    #[test]
    fn session_lease_requires_the_strict_gate_even_when_channel_visibility_is_open() {
        assert!(requires_strict_coding_session_membership(
            KIND_CODING_SESSION_LEASE
        ));
        let denial = coding_session_membership_verdict(false).unwrap_err();
        assert_eq!(
            denial,
            "restricted: coding-session events require channel membership"
        );
    }

    /// The relay must accept every delivery class the contract defines, and no
    /// others.
    ///
    /// The relay does not execute a 44220 — it stores it — so its only job
    /// here is to refuse to store a command no provider could act on. It gets
    /// that for free by decoding through the shared `buzz-core` type; this
    /// test pins that it *is* the shared type, because an envelope validator
    /// that quietly diverged from the provider's decoder would let a command
    /// into the mailbox that the provider then ignores as malformed — a turn
    /// that vanishes with no receipt.
    #[test]
    fn coding_session_command_envelope_accepts_every_delivery_class() {
        let channel = Uuid::new_v4().to_string();
        let target = "coding-session/v1|10:provider-a10:instance-19:session-11:2";
        let command = |deliver: Option<&str>| {
            let mut action = serde_json::json!({ "type": "thread.turn.start", "text": "go" });
            if let Some(deliver) = deliver {
                action["deliver"] = serde_json::json!(deliver);
            }
            let content = serde_json::json!({
                "schema": "buzz-coding-session-command/v1",
                "commandId": "cmd-1",
                "target": {
                    "driver": "provider-a",
                    "instanceId": "instance-1",
                    "sessionId": "session-1",
                    "generation": 2,
                },
                "action": action,
            })
            .to_string();
            make_event_with_tags(
                KIND_CODING_SESSION_COMMAND,
                &content,
                &[
                    &["h", &channel],
                    &["cs-v", "csc1-1"],
                    &["cs-target", target],
                ],
            )
        };

        for deliver in [None, Some("boundary"), Some("steer"), Some("interrupt")] {
            assert!(
                validate_coding_session_command_envelope(&command(deliver)).is_ok(),
                "rejected deliver={deliver:?}"
            );
        }
        for deliver in ["cancel", "Boundary", ""] {
            assert!(
                validate_coding_session_command_envelope(&command(Some(deliver))).is_err(),
                "accepted deliver={deliver:?}"
            );
        }
    }

    /// A turn's image attachments are validated at ingest, because the relay
    /// decodes the payload with the same `deny_unknown_fields` type the
    /// provider does. A command it stores but no provider can decode is a turn
    /// that vanishes with no receipt.
    #[test]
    fn coding_session_command_envelope_validates_turn_attachments() {
        let channel = Uuid::new_v4().to_string();
        let target = "coding-session/v1|10:provider-a10:instance-19:session-11:2";
        let sha = "aa0011223344556677889900aabbccddeeff00112233445566778899aabbccdd";
        let command = |attachments: Option<serde_json::Value>| {
            let mut action = serde_json::json!({ "type": "thread.turn.start", "text": "go" });
            if let Some(attachments) = attachments {
                action["attachments"] = attachments;
            }
            let content = serde_json::json!({
                "schema": "buzz-coding-session-command/v1",
                "commandId": "cmd-1",
                "target": {
                    "driver": "provider-a",
                    "instanceId": "instance-1",
                    "sessionId": "session-1",
                    "generation": 2,
                },
                "action": action,
            })
            .to_string();
            make_event_with_tags(
                KIND_CODING_SESSION_COMMAND,
                &content,
                &[
                    &["h", &channel],
                    &["cs-v", "csc1-1"],
                    &["cs-target", target],
                ],
            )
        };

        let good = serde_json::json!([
            { "sha256": sha, "mime": "image/png", "size": 2048 }
        ]);
        // Absent (every command published before the field existed), empty,
        // and well-formed all pass.
        for attachments in [None, Some(serde_json::json!([])), Some(good.clone())] {
            assert!(
                validate_coding_session_command_envelope(&command(attachments.clone())).is_ok(),
                "rejected attachments={attachments:?}"
            );
        }

        for (bad, label) in [
            (
                serde_json::json!([{ "sha256": "abc", "mime": "image/png", "size": 1 }]),
                "short hash",
            ),
            (
                serde_json::json!([{ "sha256": sha, "mime": "application/pdf", "size": 1 }]),
                "non-image mime",
            ),
            (
                serde_json::json!([{ "sha256": sha, "mime": "image/png", "size": 0 }]),
                "zero size",
            ),
            (
                serde_json::json!([{ "sha256": sha, "mime": "image/png", "size": 1, "url": "http://x" }]),
                "unknown key",
            ),
            (
                serde_json::json!(vec![
                    serde_json::json!({ "sha256": sha, "mime": "image/png", "size": 1 });
                    5
                ]),
                "too many",
            ),
        ] {
            assert!(
                validate_coding_session_command_envelope(&command(Some(bad))).is_err(),
                "accepted {label}"
            );
        }

        // The envelope itself is unchanged: attachments ride the content, and
        // an `imeta` tag is still refused outright.
        let with_imeta = make_event_with_tags(
            KIND_CODING_SESSION_COMMAND,
            &serde_json::json!({
                "schema": "buzz-coding-session-command/v1",
                "commandId": "cmd-1",
                "target": {
                    "driver": "provider-a",
                    "instanceId": "instance-1",
                    "sessionId": "session-1",
                    "generation": 2,
                },
                "action": { "type": "thread.turn.start", "text": "go" },
            })
            .to_string(),
            &[
                &["h", &channel],
                &["cs-v", "csc1-1"],
                &["cs-target", target],
                &["imeta", "url https://example/media/x.png"],
            ],
        );
        assert!(validate_coding_session_command_envelope(&with_imeta).is_err());
    }

    #[test]
    fn coding_session_command_requires_exact_content_and_tags() {
        let channel = Uuid::new_v4().to_string();
        let content = serde_json::json!({
            "schema": "buzz-coding-session-command/v1",
            "commandId": "cmd-1",
            "target": {
                "driver": "provider-a",
                "instanceId": "instance-1",
                "sessionId": "session-1",
                "generation": 2,
            },
            "action": { "type": "thread.turn.start", "text": "Steer" },
        })
        .to_string();
        let target = "coding-session/v1|10:provider-a10:instance-19:session-11:2";
        let event = make_event_with_tags(
            KIND_CODING_SESSION_COMMAND,
            &content,
            &[
                &["h", &channel],
                &["cs-v", "csc1-1"],
                &["cs-target", target],
            ],
        );
        assert!(validate_coding_session_command_envelope(&event).is_ok());

        // Interrupt is the other operator action; the Stop button publishes it
        // natively, so the envelope validator must accept it.
        let interrupt_content = serde_json::json!({
            "schema": "buzz-coding-session-command/v1",
            "commandId": "cmd-2",
            "target": {
                "driver": "provider-a",
                "instanceId": "instance-1",
                "sessionId": "session-1",
                "generation": 2,
            },
            "action": { "type": "thread.turn.interrupt" },
        })
        .to_string();
        let interrupt = make_event_with_tags(
            KIND_CODING_SESSION_COMMAND,
            &interrupt_content,
            &[
                &["h", &channel],
                &["cs-v", "csc1-1"],
                &["cs-target", target],
            ],
        );
        assert!(validate_coding_session_command_envelope(&interrupt).is_ok());

        // The tag is what adapters route on, so it must be re-derivable from
        // the content it claims to address.
        let mismatched = make_event_with_tags(
            KIND_CODING_SESSION_COMMAND,
            &content,
            &[
                &["h", &channel],
                &["cs-v", "csc1-1"],
                &["cs-target", "coding-session/v1|wrong"],
            ],
        );
        assert!(validate_coding_session_command_envelope(&mismatched).is_err());

        // A generation swap in the tag alone would steer a different session.
        let wrong_generation = make_event_with_tags(
            KIND_CODING_SESSION_COMMAND,
            &content,
            &[
                &["h", &channel],
                &["cs-v", "csc1-1"],
                &[
                    "cs-target",
                    "coding-session/v1|10:provider-a10:instance-19:session-11:3",
                ],
            ],
        );
        assert!(validate_coding_session_command_envelope(&wrong_generation).is_err());

        let extra_tag = make_event_with_tags(
            KIND_CODING_SESSION_COMMAND,
            &content,
            &[
                &["h", &channel],
                &["cs-v", "csc1-1"],
                &["cs-target", target],
                &["p", &"ab".repeat(32)],
            ],
        );
        assert!(validate_coding_session_command_envelope(&extra_tag).is_err());

        let missing_target = make_event_with_tags(
            KIND_CODING_SESSION_COMMAND,
            &content,
            &[&["h", &channel], &["cs-v", "csc1-1"]],
        );
        assert!(validate_coding_session_command_envelope(&missing_target).is_err());

        let bad_version = make_event_with_tags(
            KIND_CODING_SESSION_COMMAND,
            &content,
            &[
                &["h", &channel],
                &["cs-v", "csc1-0"],
                &["cs-target", target],
            ],
        );
        assert!(validate_coding_session_command_envelope(&bad_version).is_err());
    }

    /// A CI-continuation registration is a 44220 like any other: the relay
    /// stores it, does not execute it, and validates it with the same
    /// envelope rules. What this pins is that the *new* closed action reaches
    /// the store at all — and that a malformed one still does not.
    #[test]
    fn coding_session_command_admits_a_ci_continuation_registration() {
        let channel = Uuid::new_v4().to_string();
        let owner = "ab".repeat(32);
        let identity = serde_json::json!({
            "project": format!("30621:{owner}:beekeeper"),
            "repository": format!("30617:{owner}:beekeeper"),
            "commit": "abcdef0123456789abcdef0123456789abcdef01",
            "check": "main-validation",
            "run": "136",
            "attempt": 1,
            "workflow": "d3e440ea-89f8-4aee-8a02-17edc3e7272e",
            "phase": "build",
        });
        let target = "coding-session/v1|10:provider-a10:instance-19:session-11:2";
        let content = |action: serde_json::Value| {
            serde_json::json!({
                "schema": "buzz-coding-session-command/v1",
                "commandId": format!("cic-{}", "0".repeat(64)),
                "target": {
                    "driver": "provider-a",
                    "instanceId": "instance-1",
                    "sessionId": "session-1",
                    "generation": 2,
                },
                "action": action,
            })
            .to_string()
        };
        let registration = |action: serde_json::Value| {
            make_event_with_tags(
                KIND_CODING_SESSION_COMMAND,
                &content(action),
                &[
                    &["h", &channel],
                    &["cs-v", "csc1-1"],
                    &["cs-target", target],
                ],
            )
        };

        let valid = serde_json::json!({
            "type": "thread.turn.continue_on_ci",
            "identity": identity,
            "continuation": "Report the failing test",
            "expiresAt": 1_788_800_000_u64,
        });
        assert!(validate_coding_session_command_envelope(&registration(valid.clone())).is_ok());

        // No horizon is not "wait forever": the payload contract requires one,
        // and the relay refuses the event rather than storing a registration
        // nothing can ever expire.
        let mut missing_expires_at = valid.clone();
        assert!(missing_expires_at
            .as_object_mut()
            .expect("object")
            .remove("expiresAt")
            .is_some());
        assert!(
            validate_coding_session_command_envelope(&registration(missing_expires_at)).is_err()
        );

        let mut zero_expires_at = valid.clone();
        zero_expires_at["expiresAt"] = serde_json::json!(0);
        assert!(validate_coding_session_command_envelope(&registration(zero_expires_at)).is_err());

        // The action is closed: a key this build does not know would be terms
        // the consumer never read.
        let mut unknown_key = valid.clone();
        unknown_key["deliver"] = serde_json::json!("steer");
        assert!(validate_coding_session_command_envelope(&registration(unknown_key)).is_err());

        // The identity is validated by the same rules a recorded result is,
        // so a registration cannot name a run no result could be filed under.
        let mut bad_identity = valid;
        bad_identity["identity"]["commit"] = serde_json::json!("nothex");
        assert!(validate_coding_session_command_envelope(&registration(bad_identity)).is_err());
    }

    fn lifecycle_content(project_ref: serde_json::Value) -> String {
        serde_json::json!({
            "schema": "buzz-coding-session-lifecycle-command/v1",
            "commandId": "create-1",
            "action": {
                "type": "session.create",
                "projectRef": project_ref,
                "repoRef": null,
                "providerInstanceRef": "claude-primary",
                "providerAuthorityPubkey": "abababababababababababababababababababababababababababababababab",
                "model": null,
                "title": "Advance Buzz live sessions",
                "initialTurn": null,
            },
        })
        .to_string()
    }

    fn lifecycle_event(content: &str, channel: &str) -> Event {
        make_event_with_tags(
            KIND_CODING_SESSION_LIFECYCLE_COMMAND,
            content,
            &[
                &["h", channel],
                &["csl-v", "csl1-1"],
                &["csl-command", "create-1"],
            ],
        )
    }

    fn hire_content(session_ref: &str) -> String {
        serde_json::json!({
            "schema": "buzz-coding-session-lifecycle-command/v1",
            "commandId": "create-1",
            "action": {
                "type": "session.hire",
                "sessionRef": session_ref,
                "genesisRef": "12".repeat(32),
                "role": "builder",
                "providerInstanceRef": null,
                "model": null,
                "brief": "Rebase the lane and run the gate.",
            },
        })
        .to_string()
    }

    fn hire_content_with_brief(session_ref: &str, brief: &str) -> String {
        serde_json::json!({
            "schema": "buzz-coding-session-lifecycle-command/v1",
            "commandId": "create-1",
            "action": {
                "type": "session.hire",
                "sessionRef": session_ref,
                "genesisRef": "12".repeat(32),
                "role": "builder",
                "providerInstanceRef": null,
                "model": null,
                "brief": brief,
            },
        })
        .to_string()
    }

    /// The relay refuses a brief the founder's host could never seat, and it
    /// does so by *inheritance* rather than by a second ceiling of its own.
    ///
    /// Track 2 item 4: ingest admitted 12,288 bytes while the create the host
    /// publishes in reply — the brief behind a 16-byte prefix — is validated
    /// at that same number, so a brief of 12,273..=12,288 bytes signed, was
    /// stored, and then threw in the host. The envelope validator calls
    /// `decode_coding_session_lifecycle_command`, so the decoder's ceiling is
    /// the relay's ceiling with no code here to keep in step.
    #[test]
    fn coding_session_hire_inherits_the_decoder_effective_brief_ceiling() {
        use buzz_core::coding_session_lifecycle_command::{
            MAX_LIFECYCLE_HIRE_BRIEF_BYTES, MAX_LIFECYCLE_INITIAL_TURN_BYTES,
        };

        let channel = Uuid::new_v4().to_string();
        let session_ref = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
        let hire = |bytes: usize| {
            lifecycle_event(
                &hire_content_with_brief(session_ref, &"b".repeat(bytes)),
                &channel,
            )
        };

        assert!(
            validate_coding_session_lifecycle_command_envelope(&hire(
                MAX_LIFECYCLE_HIRE_BRIEF_BYTES
            ))
            .is_ok(),
            "a brief at the effective ceiling is accepted at ingest"
        );

        let error = validate_coding_session_lifecycle_command_envelope(&hire(
            MAX_LIFECYCLE_HIRE_BRIEF_BYTES + 1,
        ))
        .expect_err("one byte past the effective ceiling must be refused at ingest");
        assert_eq!(
            error,
            "coding-session lifecycle command action.brief exceeds 12272 bytes (got 12273): a \
             hire's brief becomes the seat's first turn behind the host's 16-byte \
             \"[From the lead] \" prefix, so its ceiling is the initial-turn ceiling minus that \
             prefix"
        );

        // The old ingest ceiling, in the window that used to be admitted.
        assert!(
            validate_coding_session_lifecycle_command_envelope(&hire(
                MAX_LIFECYCLE_INITIAL_TURN_BYTES
            ))
            .is_err(),
            "the window the host has always thrown on is no longer admitted"
        );
    }

    /// The gate reads the umbrella out of a hire's own content, and reads
    /// nothing out of any other lifecycle action — a create must keep the
    /// provider-enforced rule it has always had.
    #[test]
    fn only_a_session_hire_names_an_umbrella_the_relay_must_authorize() {
        let channel = Uuid::new_v4().to_string();
        let session_ref = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
        let hire = lifecycle_event(&hire_content(session_ref), &channel);
        assert_eq!(
            hire_authority_claim(&hire)
                .as_ref()
                .map(|claim| claim.session_ref.as_str()),
            Some(session_ref),
            "a hire must name its umbrella to the gate"
        );

        let create = lifecycle_event(&lifecycle_content(serde_json::Value::Null), &channel);
        assert_eq!(hire_authority_claim(&create), None);

        // Unparseable content is not a hire. The envelope validator refuses it
        // a few lines later; the gate must not guess an umbrella from it.
        let garbage = lifecycle_event("{", &channel);
        assert_eq!(hire_authority_claim(&garbage), None);
    }

    /// A hire is founder/operator, or active lead for a non-lead role; an
    /// umbrella no exact genesis in this channel claims is refused rather
    /// than fallen back on.
    #[test]
    fn a_hire_is_refused_unless_the_signer_founded_or_was_granted_the_umbrella() {
        let founder = vec![0xaa; 32];
        let operator = vec![0xbb; 32];
        let viewer = vec![0xcc; 32];
        let stranger = vec![0xdd; 32];
        let authority = buzz_db::coding_session_acl::SessionAuthority {
            founder: founder.clone(),
            operators: vec![operator.clone()],
            viewers: vec![viewer.clone()],
            seats: vec![buzz_db::coding_session_acl::SessionSeat {
                actor: vec![0xee; 32],
                role: "lead".into(),
            }],
        };

        assert!(hire_authority_verdict(Some(&authority), &founder, "lead").is_ok());
        assert!(hire_authority_verdict(Some(&authority), &operator, "lead").is_ok());
        assert!(hire_authority_verdict(Some(&authority), &[0xee; 32], "builder").is_ok());
        assert!(hire_authority_verdict(Some(&authority), &[0xee; 32], "lead").is_err());

        for refused in [&viewer, &stranger] {
            let error = hire_authority_verdict(Some(&authority), refused, "builder")
                .expect_err("a viewer or a stranger may not hire");
            assert_eq!(
                error,
                "restricted: only the session founder, a granted operator, or an active lead hiring a non-lead role may hire"
            );
        }

        let unknown = hire_authority_verdict(None, &founder, "builder")
            .expect_err("an umbrella with no genesis here cannot authorize a hire");
        assert!(
            unknown.starts_with("restricted: ") && unknown.contains("genesis"),
            "got {unknown}"
        );
    }

    #[test]
    fn relay_hire_admission_refuses_revoked_stale_and_wrong_genesis_leads() {
        let founder = vec![0xaa; 32];
        let lead = vec![0xbb; 32];
        let active = buzz_db::coding_session_acl::SessionAuthority {
            founder,
            operators: Vec::new(),
            viewers: Vec::new(),
            seats: vec![buzz_db::coding_session_acl::SessionSeat {
                actor: lead.clone(),
                role: "lead".into(),
            }],
        };
        assert!(hire_authority_verdict(Some(&active), &lead, "builder").is_ok());
        assert!(hire_authority_verdict(Some(&active), &lead, "lead").is_err());

        let revoked = buzz_db::coding_session_acl::SessionAuthority {
            seats: Vec::new(),
            ..active.clone()
        };
        assert!(hire_authority_verdict(Some(&revoked), &lead, "builder").is_err());
        // A stale or wrong-genesis reference resolves no exact authority root;
        // it cannot reuse the active lead state selected under another id.
        assert!(hire_authority_verdict(None, &lead, "builder").is_err());
    }

    #[test]
    fn coding_session_lifecycle_command_requires_exact_content_and_ordered_tags() {
        let channel = Uuid::new_v4().to_string();
        let project = format!("30621:{}:amas-redux", "cd".repeat(32));
        let content = lifecycle_content(serde_json::Value::String(project));
        let event = lifecycle_event(&content, &channel);
        assert!(validate_coding_session_lifecycle_command_envelope(&event).is_ok());

        let mismatched = make_event_with_tags(
            KIND_CODING_SESSION_LIFECYCLE_COMMAND,
            &content,
            &[
                &["h", &channel],
                &["csl-v", "csl1-1"],
                &["csl-command", "create-2"],
            ],
        );
        assert!(validate_coding_session_lifecycle_command_envelope(&mismatched).is_err());

        // Ordered, not merely present: adapters read tags positionally.
        let reordered = make_event_with_tags(
            KIND_CODING_SESSION_LIFECYCLE_COMMAND,
            &content,
            &[
                &["csl-v", "csl1-1"],
                &["h", &channel],
                &["csl-command", "create-1"],
            ],
        );
        assert!(validate_coding_session_lifecycle_command_envelope(&reordered).is_err());
    }

    /// The agent-seat amendment at the ingest gate: a create that names an
    /// `actor` and a `role` is stored, and half a seat never is.
    ///
    /// The relay does not resolve the seat — custody is host-local by design —
    /// so all it can do is refuse a payload that describes nothing coherent,
    /// which it does by the name the code carries.
    #[test]
    fn coding_session_lifecycle_command_accepts_a_seated_create_and_refuses_half_a_seat() {
        let channel = Uuid::new_v4().to_string();
        let seated = |actor: serde_json::Value, role: serde_json::Value| {
            let mut content = serde_json::json!({
                "schema": "buzz-coding-session-lifecycle-command/v1",
                "commandId": "create-1",
                "action": {
                    "type": "session.create",
                    "projectRef": null,
                    "repoRef": null,
                    "providerInstanceRef": "claude-primary",
                    "providerAuthorityPubkey": "ab".repeat(32),
                    "model": null,
                    "title": "Advance Buzz live sessions",
                    "initialTurn": null,
                },
            });
            let action = content["action"].as_object_mut().expect("action object");
            if !actor.is_null() {
                action.insert("actor".to_owned(), actor);
            }
            if !role.is_null() {
                action.insert("role".to_owned(), role);
            }
            lifecycle_event(&content.to_string(), &channel)
        };

        let complete = seated(
            serde_json::Value::String("cd".repeat(32)),
            serde_json::Value::String("lead".to_owned()),
        );
        assert!(validate_coding_session_lifecycle_command_envelope(&complete).is_ok());

        for (actor, role) in [
            (
                serde_json::Value::String("cd".repeat(32)),
                serde_json::Value::Null,
            ),
            (
                serde_json::Value::Null,
                serde_json::Value::String("lead".to_owned()),
            ),
        ] {
            let error = validate_coding_session_lifecycle_command_envelope(&seated(actor, role))
                .expect_err("half a seat must be refused at ingest");
            assert!(
                error.contains(buzz_core::coding_session_payload::ACTOR_ROLE_PAIR),
                "the ingest refusal must name the code: {error}"
            );
        }

        // A malformed seat is refused too — the relay never stores a create it
        // cannot describe.
        assert!(validate_coding_session_lifecycle_command_envelope(&seated(
            serde_json::Value::String("cd".repeat(32).to_uppercase()),
            serde_json::Value::String("lead".to_owned()),
        ))
        .is_err());
        assert!(validate_coding_session_lifecycle_command_envelope(&seated(
            serde_json::Value::String("cd".repeat(32)),
            serde_json::Value::String("Lead Builder".to_owned()),
        ))
        .is_err());
    }

    /// Fork amendment: a standalone session (no project) is a first-class,
    /// accepted shape — the relay must not require a project binding.
    #[test]
    fn coding_session_lifecycle_command_accepts_a_null_project_ref() {
        let channel = Uuid::new_v4().to_string();
        let content = lifecycle_content(serde_json::Value::Null);
        assert!(
            validate_coding_session_lifecycle_command_envelope(&lifecycle_event(
                &content, &channel
            ))
            .is_ok()
        );
    }

    /// Optional is not unvalidated. A present reference must be a NIP-MP
    /// project coordinate; the donor-era `30178:` team-catalog form and a bare
    /// slug are both rejected, so a project-bound session cannot be quietly
    /// pointed at something that is not a project.
    #[test]
    fn coding_session_lifecycle_command_rejects_non_project_refs() {
        let channel = Uuid::new_v4().to_string();
        let owner = "cd".repeat(32);
        for rejected in [
            format!("30178:{owner}:amas-redux"),
            format!("30617:{owner}:amas-redux"),
            "amas-redux".to_string(),
        ] {
            let content = lifecycle_content(serde_json::Value::String(rejected.clone()));
            assert!(
                validate_coding_session_lifecycle_command_envelope(&lifecycle_event(
                    &content, &channel
                ))
                .is_err(),
                "should reject projectRef {rejected:?}"
            );
        }
    }

    /// A host filesystem path must never ride along inside signed content —
    /// the working directory is machine-local state, resolved by the producer.
    #[test]
    fn coding_session_lifecycle_command_rejects_unknown_payload_fields() {
        let channel = Uuid::new_v4().to_string();
        let content = serde_json::json!({
            "schema": "buzz-coding-session-lifecycle-command/v1",
            "commandId": "create-1",
            "action": {
                "type": "session.create",
                "projectRef": null,
                "repoRef": null,
                "providerInstanceRef": "claude-primary",
                "providerAuthorityPubkey": "abababababababababababababababababababababababababababababababab",
                "model": null,
                "title": null,
                "initialTurn": null,
                "cwd": "/Users/someone/checkout",
            },
        })
        .to_string();
        assert!(
            validate_coding_session_lifecycle_command_envelope(&lifecycle_event(
                &content, &channel
            ))
            .is_err()
        );
    }

    /// The canonical umbrella reference used by the genesis tests.
    const GENESIS_SESSION_REF: &str = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";

    fn genesis_event(content: &str, channel: &str, session_tag: &str) -> Event {
        make_event_with_tags(
            KIND_CODING_SESSION_GENESIS,
            content,
            &[
                &["h", channel],
                &["csg-v", "csg1-1"],
                &["csg-session", session_tag],
            ],
        )
    }

    fn genesis_content(session_ref: &str) -> String {
        serde_json::json!({ "sessionRef": session_ref, "v": 1 }).to_string()
    }

    #[test]
    fn coding_session_genesis_requires_exact_content_and_ordered_tags() {
        let channel = Uuid::new_v4().to_string();
        let content = genesis_content(GENESIS_SESSION_REF);
        assert!(validate_coding_session_genesis_envelope(&genesis_event(
            &content,
            &channel,
            GENESIS_SESSION_REF
        ))
        .is_ok());

        // Ordered, not merely present: consumers read tags positionally.
        let reordered = make_event_with_tags(
            KIND_CODING_SESSION_GENESIS,
            &content,
            &[
                &["csg-v", "csg1-1"],
                &["h", &channel],
                &["csg-session", GENESIS_SESSION_REF],
            ],
        );
        assert!(validate_coding_session_genesis_envelope(&reordered).is_err());

        // Three tags exactly — no room for a smuggled fourth.
        let extra = make_event_with_tags(
            KIND_CODING_SESSION_GENESIS,
            &content,
            &[
                &["h", &channel],
                &["csg-v", "csg1-1"],
                &["csg-session", GENESIS_SESSION_REF],
                &["p", &"ab".repeat(32)],
            ],
        );
        assert!(validate_coding_session_genesis_envelope(&extra).is_err());

        for bad_tags in [
            vec![
                vec!["h".to_owned(), "not-a-uuid".to_owned()],
                vec!["csg-v".to_owned(), "csg1-1".to_owned()],
                vec!["csg-session".to_owned(), GENESIS_SESSION_REF.to_owned()],
            ],
            vec![
                vec!["h".to_owned(), channel.clone()],
                vec!["csg-v".to_owned(), "csg1-0".to_owned()],
                vec!["csg-session".to_owned(), GENESIS_SESSION_REF.to_owned()],
            ],
            vec![
                vec!["h".to_owned(), channel.clone()],
                vec!["csg-v".to_owned(), "csg1-1".to_owned()],
                vec!["csg-ref".to_owned(), GENESIS_SESSION_REF.to_owned()],
            ],
        ] {
            let borrowed: Vec<Vec<&str>> = bad_tags
                .iter()
                .map(|parts| parts.iter().map(String::as_str).collect())
                .collect();
            let slices: Vec<&[&str]> = borrowed.iter().map(Vec::as_slice).collect();
            let event = make_event_with_tags(KIND_CODING_SESSION_GENESIS, &content, &slices);
            assert!(
                validate_coding_session_genesis_envelope(&event).is_err(),
                "should reject tags {bad_tags:?}"
            );
        }
    }

    /// The tag is what a consumer filters on and the content is what it reads.
    /// If those two could name different umbrellas, a genesis would be
    /// discoverable as the founder of a session it never founded.
    #[test]
    fn coding_session_genesis_rejects_a_tag_that_disagrees_with_content() {
        let channel = Uuid::new_v4().to_string();
        let content = genesis_content(GENESIS_SESSION_REF);
        let substituted = genesis_event(&content, &channel, "0000000a-90d4-4b0e-a1f3-7c2d8e6f4a10");
        assert!(validate_coding_session_genesis_envelope(&substituted).is_err());
    }

    /// A genesis that loses the race for its reference is refused outright.
    /// The `accepted: false` here is the whole of the contract: a duplicate
    /// gets no acceptance receipt, and the message names the founder it must
    /// resolve to instead.
    #[test]
    fn coding_session_genesis_duplicate_is_rejected_without_a_receipt() {
        let loser = "cd".repeat(32);
        let winner = hex::decode("ab".repeat(32)).expect("winner id");
        let result = coding_session_genesis_duplicate_result(loser.clone(), &winner);

        assert_eq!(result.event_id, loser);
        assert!(
            !result.accepted,
            "a rival claim must be refused, not accepted-with-a-note"
        );
        assert!(
            result.message.starts_with("duplicate:"),
            "duplicates keep the established `duplicate:` wire prefix, got {:?}",
            result.message
        );
        assert!(
            result.message.contains(&"ab".repeat(32)),
            "the refusal must name the winning genesis, got {:?}",
            result.message
        );
    }

    /// A genesis over a session that predates the kind is refused, and every
    /// refusal reads as `invalid:`, never `duplicate:` — nothing was
    /// duplicated, and a client that saw `duplicate:` would look for a rival
    /// genesis that does not exist. R15 replaced the old two-refusal
    /// (`NotTheFounder`/`FounderAmbiguous`) shape with an explicit-reference
    /// contract; this sweeps every `GenesisAdoptionRefusal` variant's wire
    /// message.
    #[test]
    fn coding_session_genesis_adoption_refusals_are_reported_as_invalid_never_duplicate() {
        let create = "cd".repeat(32);
        let founder = "ab".repeat(32);

        let cases: Vec<(buzz_db::GenesisAdoptionRefusal, &str)> = vec![
            (
                buzz_db::GenesisAdoptionRefusal::LegacyHistoryRequiresAdoption {
                    existing_create_event_id: hex::decode(&create).expect("create"),
                },
                "adopts",
            ),
            (
                buzz_db::GenesisAdoptionRefusal::ReferencedCreateNotFound,
                "createEventId",
            ),
            (
                buzz_db::GenesisAdoptionRefusal::ReferencedReceiptNotFound,
                "receiptEventId",
            ),
            (
                buzz_db::GenesisAdoptionRefusal::ReceiptDoesNotJoinCreate,
                "join",
            ),
            (
                buzz_db::GenesisAdoptionRefusal::SessionRefMismatch,
                "sessionRef",
            ),
            (
                buzz_db::GenesisAdoptionRefusal::SignerMismatch {
                    founder_pubkey: hex::decode(&founder).expect("founder"),
                },
                "signer",
            ),
            (buzz_db::GenesisAdoptionRefusal::WrongChannel, "channel"),
            (
                buzz_db::GenesisAdoptionRefusal::CommandIdAmbiguous {
                    reason: "two sessionRefs claim the founding command",
                },
                "commandId",
            ),
        ];

        for (refusal, expect_substring) in cases {
            let result = coding_session_genesis_adoption_refusal_result("ef".repeat(32), &refusal);
            assert!(!result.accepted, "{refusal:?} must be refused");
            assert!(
                result.message.starts_with("invalid:"),
                "{refusal:?}: legacy-history refusals are not duplicates, got {:?}",
                result.message
            );
            assert!(
                result.message.contains(expect_substring),
                "{refusal:?}: expected {expect_substring:?} in {:?}",
                result.message
            );
        }
    }

    /// Genesis content is two fields and nothing else. A restated founder
    /// pubkey is the dangerous case: the signature already settles authorship,
    /// so a content field claiming it is a second answer to a settled question.
    #[test]
    fn coding_session_genesis_rejects_off_contract_content() {
        let channel = Uuid::new_v4().to_string();
        for rejected in [
            serde_json::json!({ "sessionRef": GENESIS_SESSION_REF }).to_string(),
            serde_json::json!({ "v": 1 }).to_string(),
            serde_json::json!({ "sessionRef": GENESIS_SESSION_REF, "v": 2 }).to_string(),
            serde_json::json!({
                "sessionRef": GENESIS_SESSION_REF,
                "v": 1,
                "founder": "ab".repeat(32),
            })
            .to_string(),
            serde_json::json!({ "sessionRef": GENESIS_SESSION_REF.to_uppercase(), "v": 1 })
                .to_string(),
            String::new(),
        ] {
            let event = genesis_event(&rejected, &channel, GENESIS_SESSION_REF);
            assert!(
                validate_coding_session_genesis_envelope(&event).is_err(),
                "should reject content {rejected:?}"
            );
        }
    }

    /// R15: the envelope (three tags, `csg-session` mirroring `sessionRef`)
    /// is identical for both payload forms — only the content's optional
    /// `adopts` key differs, and the DB-transactional adoption verification
    /// (event.rs) is a separate concern from this pure envelope check.
    #[test]
    fn coding_session_genesis_envelope_accepts_the_adoption_form() {
        let channel = Uuid::new_v4().to_string();
        let content = serde_json::json!({
            "sessionRef": GENESIS_SESSION_REF,
            "v": 1,
            "adopts": {
                "createEventId": "ab".repeat(32),
                "receiptEventId": "cd".repeat(32),
            },
        })
        .to_string();
        assert!(validate_coding_session_genesis_envelope(&genesis_event(
            &content,
            &channel,
            GENESIS_SESSION_REF
        ))
        .is_ok());
    }

    fn goal_event(content: &str, channel: &str, session_ref: &str) -> Event {
        make_event_with_tags(
            KIND_CODING_SESSION_GOAL,
            content,
            &[&["h", channel], &["d", session_ref], &["csgl-v", "csgl1-1"]],
        )
    }

    #[test]
    fn coding_session_goal_requires_exact_regular_revision_envelope() {
        let channel = Uuid::new_v4().to_string();
        assert!(
            buzz_core::coding_session_goal::validate_coding_session_goal_envelope(&goal_event(
                "Make authority visible",
                &channel,
                GENESIS_SESSION_REF
            ))
            .is_ok()
        );

        let smuggled = make_event_with_tags(
            KIND_CODING_SESSION_GOAL,
            "Make authority visible",
            &[
                &["h", &channel],
                &["d", GENESIS_SESSION_REF],
                &["csgl-v", "csgl1-1"],
                &["p", &"ab".repeat(32)],
            ],
        );
        assert!(
            buzz_core::coding_session_goal::validate_coding_session_goal_envelope(&smuggled)
                .is_err()
        );

        for event in [
            goal_event("", &channel, GENESIS_SESSION_REF),
            goal_event("goal", &channel, &GENESIS_SESSION_REF.to_uppercase()),
            make_event_with_tags(
                KIND_CODING_SESSION_GOAL,
                "goal",
                &[
                    &["d", GENESIS_SESSION_REF],
                    &["h", &channel],
                    &["csgl-v", "csgl1-1"],
                ],
            ),
        ] {
            assert!(
                buzz_core::coding_session_goal::validate_coding_session_goal_envelope(&event)
                    .is_err()
            );
        }
    }

    fn name_event(content: &str, channel: &str, session_ref: &str) -> Event {
        make_event_with_tags(
            KIND_CODING_SESSION_NAME,
            content,
            &[&["h", channel], &["d", session_ref], &["csnm-v", "csnm1-1"]],
        )
    }

    #[test]
    fn coding_session_name_requires_exact_regular_revision_envelope() {
        let channel = Uuid::new_v4().to_string();
        assert!(
            buzz_core::coding_session_name::validate_coding_session_name_envelope(&name_event(
                "Authority phase",
                &channel,
                GENESIS_SESSION_REF
            ))
            .is_ok()
        );

        let smuggled = make_event_with_tags(
            KIND_CODING_SESSION_NAME,
            "Authority phase",
            &[
                &["h", &channel],
                &["d", GENESIS_SESSION_REF],
                &["csnm-v", "csnm1-1"],
                &["p", &"ab".repeat(32)],
            ],
        );
        assert!(
            buzz_core::coding_session_name::validate_coding_session_name_envelope(&smuggled)
                .is_err()
        );

        for event in [
            name_event("", &channel, GENESIS_SESSION_REF),
            name_event("first\nsecond", &channel, GENESIS_SESSION_REF),
            name_event("name", &channel, &GENESIS_SESSION_REF.to_uppercase()),
            make_event_with_tags(
                KIND_CODING_SESSION_NAME,
                "name",
                &[
                    &["d", GENESIS_SESSION_REF],
                    &["h", &channel],
                    &["csnm-v", "csnm1-1"],
                ],
            ),
        ] {
            assert!(
                buzz_core::coding_session_name::validate_coding_session_name_envelope(&event)
                    .is_err()
            );
        }
    }

    fn closure_event(action: &str, channel: &str, session_ref: &str, genesis_ref: &str) -> Event {
        let content = format!(
            r#"{{"action":"{action}","genesisRef":"{genesis_ref}","sessionRef":"{session_ref}","v":1}}"#
        );
        make_event_with_tags(
            KIND_CODING_SESSION_CLOSURE,
            &content,
            &[
                &["h", channel],
                &["d", session_ref],
                &["cscl-v", "cscl1-1"],
                &["cscl-genesis", genesis_ref],
            ],
        )
    }

    #[test]
    fn coding_session_closure_requires_exact_rooted_revision_envelope() {
        let channel = Uuid::new_v4().to_string();
        let genesis_ref = "ab".repeat(32);
        for action in ["closed", "open"] {
            assert!(
                buzz_core::coding_session_closure::validate_coding_session_closure_envelope(
                    &closure_event(action, &channel, GENESIS_SESSION_REF, &genesis_ref)
                )
                .is_ok()
            );
        }

        let mismatched = closure_event("closed", &channel, GENESIS_SESSION_REF, &genesis_ref);
        let mismatched = make_event_with_tags(
            KIND_CODING_SESSION_CLOSURE,
            &mismatched.content,
            &[
                &["h", &channel],
                &["d", GENESIS_SESSION_REF],
                &["cscl-v", "cscl1-1"],
                &["cscl-genesis", &"cd".repeat(32)],
            ],
        );
        assert!(
            buzz_core::coding_session_closure::validate_coding_session_closure_envelope(
                &mismatched
            )
            .is_err()
        );
    }

    #[test]
    fn coding_session_closure_genesis_root_must_match_kind_channel_and_session() {
        use buzz_core::coding_session_closure::{
            CodingSessionClosureAction, CodingSessionClosurePayload,
        };

        let channel = Uuid::new_v4();
        let genesis = genesis_event(
            &genesis_content(GENESIS_SESSION_REF),
            &channel.to_string(),
            GENESIS_SESSION_REF,
        );
        let payload = CodingSessionClosurePayload::new(
            CodingSessionClosureAction::Closed,
            genesis.id.to_hex(),
            GENESIS_SESSION_REF,
        );
        assert_eq!(
            coding_session_closure_founder(&genesis, Some(channel), channel, &payload).unwrap(),
            genesis.pubkey.to_bytes()
        );
        assert!(
            coding_session_closure_founder(&genesis, Some(Uuid::new_v4()), channel, &payload,)
                .is_err()
        );

        let wrong_session = CodingSessionClosurePayload::new(
            CodingSessionClosureAction::Closed,
            genesis.id.to_hex(),
            "11111111-1111-4111-8111-111111111111",
        );
        assert!(
            coding_session_closure_founder(&genesis, Some(channel), channel, &wrong_session,)
                .is_err()
        );

        let not_genesis = name_event("not a genesis", &channel.to_string(), GENESIS_SESSION_REF);
        assert!(
            coding_session_closure_founder(&not_genesis, Some(channel), channel, &payload,)
                .is_err()
        );
    }

    #[test]
    fn coding_session_closure_authority_is_action_and_container_specific() {
        use buzz_core::coding_session_closure::CodingSessionClosureAction;

        let founder = vec![1; 32];
        let project_owner = vec![2; 32];
        let project_member = vec![3; 32];
        let project_viewer = vec![5; 32];
        let outsider = vec![4; 32];
        let gate = buzz_db::project_acl::ProjectGate {
            owner: project_owner.clone(),
            members: vec![
                (
                    project_member.clone(),
                    buzz_db::project_acl::ProjectRole::Collaborator,
                ),
                (
                    project_viewer.clone(),
                    buzz_db::project_acl::ProjectRole::Viewer,
                ),
            ],
        };

        assert!(coding_session_closure_authority_verdict(
            CodingSessionClosureAction::Closed,
            &founder,
            &founder,
            Some(&gate),
        )
        .is_ok());
        assert!(coding_session_closure_authority_verdict(
            CodingSessionClosureAction::Closed,
            &project_member,
            &founder,
            Some(&gate),
        )
        .is_err());
        // Archiving is a close with a filing cabinet: founder-only too.
        assert!(coding_session_closure_authority_verdict(
            CodingSessionClosureAction::Archived,
            &founder,
            &founder,
            Some(&gate),
        )
        .is_ok());
        for refused in [&project_owner, &project_member, &outsider] {
            assert!(coding_session_closure_authority_verdict(
                CodingSessionClosureAction::Archived,
                refused,
                &founder,
                Some(&gate),
            )
            .is_err());
        }

        for project_actor in [&project_owner, &project_member] {
            assert!(coding_session_closure_authority_verdict(
                CodingSessionClosureAction::Open,
                project_actor,
                &founder,
                Some(&gate),
            )
            .is_ok());
        }
        // Reopening changes shared session state — the write tier: a
        // read-only project viewer does not qualify, nor does an outsider.
        for refused in [&project_viewer, &outsider] {
            assert!(coding_session_closure_authority_verdict(
                CodingSessionClosureAction::Open,
                refused,
                &founder,
                Some(&gate),
            )
            .is_err());
        }
        assert!(coding_session_closure_authority_verdict(
            CodingSessionClosureAction::Open,
            &founder,
            &founder,
            None,
        )
        .is_ok());
        assert!(coding_session_closure_authority_verdict(
            CodingSessionClosureAction::Open,
            &project_member,
            &founder,
            None,
        )
        .is_err());
    }

    /// The canonical genesis and grantee ids used by the authority-transition
    /// envelope tests.
    const AUTHORITY_TRANSITION_GENESIS_REF: &str =
        "abababababababababababababababababababababababababababababababab";
    const AUTHORITY_TRANSITION_GRANTEE: &str =
        "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd";

    fn authority_transition_event(content: &str, channel: &str, genesis_tag: &str) -> Event {
        make_event_with_tags(
            KIND_CODING_SESSION_AUTHORITY_TRANSITION,
            content,
            &[
                &["h", channel],
                &["csat-v", "csat1-1"],
                &["csat-genesis", genesis_tag],
            ],
        )
    }

    fn authority_transition_content(
        genesis_ref: &str,
        prev: &str,
        seq: u32,
        grantee: &str,
    ) -> String {
        format!(
            r#"{{"genesisRef":"{genesis_ref}","prevAccepted":{prev},"seq":{seq},"type":"grant-operator","granteePubkey":"{grantee}"}}"#
        )
    }

    #[test]
    fn coding_session_authority_transition_requires_exact_content_and_ordered_tags() {
        let channel = Uuid::new_v4().to_string();
        let content = authority_transition_content(
            AUTHORITY_TRANSITION_GENESIS_REF,
            "null",
            1,
            AUTHORITY_TRANSITION_GRANTEE,
        );
        assert!(validate_coding_session_authority_transition_envelope(
            &authority_transition_event(&content, &channel, AUTHORITY_TRANSITION_GENESIS_REF)
        )
        .is_ok());

        // Ordered, not merely present.
        let reordered = make_event_with_tags(
            KIND_CODING_SESSION_AUTHORITY_TRANSITION,
            &content,
            &[
                &["csat-v", "csat1-1"],
                &["h", &channel],
                &["csat-genesis", AUTHORITY_TRANSITION_GENESIS_REF],
            ],
        );
        assert!(validate_coding_session_authority_transition_envelope(&reordered).is_err());

        // A smuggled fourth tag.
        let extra = make_event_with_tags(
            KIND_CODING_SESSION_AUTHORITY_TRANSITION,
            &content,
            &[
                &["h", &channel],
                &["csat-v", "csat1-1"],
                &["csat-genesis", AUTHORITY_TRANSITION_GENESIS_REF],
                &["p", &"ab".repeat(32)],
            ],
        );
        assert!(validate_coding_session_authority_transition_envelope(&extra).is_err());
    }

    /// The tag is what the storage transaction's chain lookup routes on, so
    /// it must be re-derivable from the content it claims to address — a
    /// disagreement must never be silently stored.
    #[test]
    fn coding_session_authority_transition_rejects_a_tag_that_disagrees_with_content() {
        let channel = Uuid::new_v4().to_string();
        let content = authority_transition_content(
            AUTHORITY_TRANSITION_GENESIS_REF,
            "null",
            1,
            AUTHORITY_TRANSITION_GRANTEE,
        );
        let other_genesis = "11".repeat(32);
        let substituted = authority_transition_event(&content, &channel, &other_genesis);
        assert!(validate_coding_session_authority_transition_envelope(&substituted).is_err());
    }

    #[test]
    fn coding_session_authority_transition_rejects_off_contract_content() {
        let channel = Uuid::new_v4().to_string();
        for rejected in [
            // Missing granteePubkey.
            serde_json::json!({
                "genesisRef": AUTHORITY_TRANSITION_GENESIS_REF,
                "prevAccepted": null,
                "seq": 1,
                "type": "grant-operator",
            })
            .to_string(),
            // Unknown transition type.
            authority_transition_content(
                AUTHORITY_TRANSITION_GENESIS_REF,
                "null",
                1,
                AUTHORITY_TRANSITION_GRANTEE,
            )
            .replace("grant-operator", "takeover"),
            // seq = 0.
            authority_transition_content(
                AUTHORITY_TRANSITION_GENESIS_REF,
                "null",
                0,
                AUTHORITY_TRANSITION_GRANTEE,
            ),
            // seq/prevAccepted disagreement.
            authority_transition_content(
                AUTHORITY_TRANSITION_GENESIS_REF,
                &format!("\"{}\"", "11".repeat(32)),
                1,
                AUTHORITY_TRANSITION_GRANTEE,
            ),
            String::new(),
        ] {
            let event =
                authority_transition_event(&rejected, &channel, AUTHORITY_TRANSITION_GENESIS_REF);
            assert!(
                validate_coding_session_authority_transition_envelope(&event).is_err(),
                "should reject content {rejected:?}"
            );
        }
    }

    /// Every [`buzz_db::AuthorityTransitionRefusal`] variant must map to a
    /// distinct, specific wire message — mirroring the genesis adoption
    /// refusal sweep.
    #[test]
    fn coding_session_authority_transition_refusals_are_reported_as_invalid() {
        let event_id_hex = "ef".repeat(32);
        let owner = vec![0xabu8; 32];
        let stale_head = vec![0xcdu8; 32];

        let cases: Vec<(buzz_db::AuthorityTransitionRefusal, &str)> = vec![
            (
                buzz_db::AuthorityTransitionRefusal::GenesisNotFound,
                "genesisRef",
            ),
            (buzz_db::AuthorityTransitionRefusal::WrongChannel, "channel"),
            (
                buzz_db::AuthorityTransitionRefusal::StaleHead {
                    expected_prev_accepted: Some(stale_head.clone()),
                },
                "current head",
            ),
            (
                buzz_db::AuthorityTransitionRefusal::StaleHead {
                    expected_prev_accepted: None,
                },
                "no accepted",
            ),
            (
                buzz_db::AuthorityTransitionRefusal::SeqMismatch { expected_seq: 3 },
                "seq",
            ),
            (
                buzz_db::AuthorityTransitionRefusal::SignerNotOwner {
                    owner_pubkey: owner.clone(),
                },
                "current owner",
            ),
            (
                buzz_db::AuthorityTransitionRefusal::NoSuchGrant,
                "no live grant",
            ),
            (
                buzz_db::AuthorityTransitionRefusal::SignerNotAuthorized,
                "authority",
            ),
            (
                buzz_db::AuthorityTransitionRefusal::LeadCannotManageLead,
                "lead",
            ),
            (
                buzz_db::AuthorityTransitionRefusal::SelfNomination,
                "own signer",
            ),
            (
                buzz_db::AuthorityTransitionRefusal::NoSuchSeat,
                "no active seat",
            ),
            (
                buzz_db::AuthorityTransitionRefusal::SeatRoleMismatch,
                "does not match",
            ),
        ];

        let mut seen_messages = std::collections::HashSet::new();
        for (refusal, expected_substring) in cases {
            let result =
                coding_session_authority_transition_refusal_result(event_id_hex.clone(), &refusal);
            assert!(!result.accepted, "refusal must set accepted=false");
            assert!(
                result.message.starts_with("invalid:"),
                "got {:?}",
                result.message
            );
            assert!(
                result.message.contains(expected_substring),
                "expected {:?} to mention {expected_substring:?}",
                result.message
            );
            assert!(
                seen_messages.insert(result.message.clone()),
                "duplicate wire message across refusal variants: {:?}",
                result.message
            );
        }
    }

    /// The four provider-authored kinds are bounded by size alone — the relay
    /// does not parse a provider's account of its own session. These are the
    /// exact caps the producer writes against.
    #[test]
    fn provider_authored_coding_session_kinds_have_size_caps() {
        assert_eq!(
            coding_session_content_cap(KIND_CODING_SESSION_PROVIDER_CATALOG),
            Some(256 * 1024)
        );
        assert_eq!(
            coding_session_content_cap(KIND_CODING_SESSION_METADATA),
            Some(32 * 1024)
        );
        assert_eq!(
            coding_session_content_cap(KIND_CODING_SESSION_LIFECYCLE_RECEIPT),
            Some(16 * 1024)
        );
        assert_eq!(
            coding_session_content_cap(KIND_CODING_SESSION_TRANSCRIPT),
            Some(32 * 1024)
        );
        // The four operator-authored kinds are bounded by their payload
        // contracts in buzz-core instead, so they must not also carry a cap
        // here. Their envelope validators run first, and each one rejects
        // oversized content before parsing it.
        assert_eq!(
            coding_session_content_cap(KIND_CODING_SESSION_COMMAND),
            None
        );
        assert_eq!(
            coding_session_content_cap(KIND_CODING_SESSION_LIFECYCLE_COMMAND),
            None
        );
        assert_eq!(
            coding_session_content_cap(KIND_CODING_SESSION_GENESIS),
            None
        );
        assert_eq!(coding_session_content_cap(KIND_CODING_SESSION_GOAL), None);
        assert_eq!(coding_session_content_cap(KIND_CODING_SESSION_NAME), None);
        assert_eq!(
            coding_session_content_cap(KIND_CODING_SESSION_CLOSURE),
            None
        );
        assert_eq!(
            coding_session_content_cap(KIND_CODING_SESSION_AUTHORITY_TRANSITION),
            None
        );
        // And no other kind is bounded by this table.
        assert_eq!(coding_session_content_cap(KIND_STREAM_MESSAGE), None);
    }
}
