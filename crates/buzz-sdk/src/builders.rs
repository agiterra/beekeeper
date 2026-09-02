//! Typed event builder functions (38 builders).
//!
//! All functions return `Result<nostr::EventBuilder, SdkError>`.
//! The caller signs: `builder.sign_with_keys(&keys)?`.

use buzz_core::{
    coding_session_authority_transition::{
        CodingSessionAuthorityTransitionPayload, CODING_SESSION_AUTHORITY_TRANSITION_TAG_VERSION,
        MAX_AUTHORITY_TRANSITION_CONTENT_BYTES,
    },
    coding_session_closure::{
        CodingSessionClosurePayload, CODING_SESSION_CLOSURE_TAG_VERSION,
        MAX_CODING_SESSION_CLOSURE_CONTENT_BYTES,
    },
    coding_session_command::{
        coding_session_target_key, CodingSessionCommandPayload, CodingSessionTarget,
        CODING_SESSION_COMMAND_TAG_VERSION, MAX_IDENTIFIER_BYTES, MAX_SAFE_GENERATION,
    },
    coding_session_genesis::{
        CodingSessionGenesisPayload, CODING_SESSION_GENESIS_TAG_VERSION, MAX_GENESIS_CONTENT_BYTES,
    },
    coding_session_goal::{
        validate_coding_session_goal_content, validate_coding_session_goal_session_ref,
        CODING_SESSION_GOAL_TAG_VERSION,
    },
    coding_session_lease::{
        CodingSessionLease, CODING_SESSION_LEASE_TAG_VERSION,
        MAX_CODING_SESSION_LEASE_CONTENT_BYTES,
    },
    coding_session_lifecycle_command::{
        CodingSessionLifecycleCommandPayload, CODING_SESSION_LIFECYCLE_COMMAND_TAG_VERSION,
        MAX_LIFECYCLE_CONTENT_BYTES,
    },
    coding_session_name::{
        validate_coding_session_name_content, validate_coding_session_name_session_ref,
        CODING_SESSION_NAME_TAG_VERSION,
    },
    coding_session_payload::ReceiptStatus,
    kind::{
        KIND_AGENT_OBSERVER_FRAME, KIND_APPROVAL_DENY, KIND_APPROVAL_GRANT,
        KIND_CODING_SESSION_AUTHORITY_TRANSITION, KIND_CODING_SESSION_CLOSURE,
        KIND_CODING_SESSION_COMMAND, KIND_CODING_SESSION_GENESIS, KIND_CODING_SESSION_GOAL,
        KIND_CODING_SESSION_LEASE, KIND_CODING_SESSION_LIFECYCLE_COMMAND,
        KIND_CODING_SESSION_LIFECYCLE_RECEIPT, KIND_CODING_SESSION_METADATA,
        KIND_CODING_SESSION_NAME, KIND_CODING_SESSION_PROVIDER_CATALOG,
        KIND_CODING_SESSION_TRANSCRIPT, KIND_DELETION, KIND_DM_ADD_MEMBER, KIND_DM_OPEN,
        KIND_EMOJI_SET, KIND_GIT_ISSUE, KIND_GIT_PATCH, KIND_GIT_PR_UPDATE, KIND_GIT_PULL_REQUEST,
        KIND_GIT_REPO_ANNOUNCEMENT, KIND_GIT_STATUS_CLOSED, KIND_GIT_STATUS_DRAFT,
        KIND_GIT_STATUS_MERGED, KIND_GIT_STATUS_OPEN, KIND_IA_ARCHIVE_REQUEST,
        KIND_IA_UNARCHIVE_REQUEST, KIND_MODERATION_BAN, KIND_MODERATION_RESOLVE_REPORT,
        KIND_MODERATION_TIMEOUT, KIND_MODERATION_UNBAN, KIND_MODERATION_UNTIMEOUT,
        KIND_PRESENCE_UPDATE, KIND_PROJECT, KIND_PULSE_ENTRY, KIND_USER_STATUS, KIND_WORKFLOW_DEF,
        KIND_WORKFLOW_TRIGGER,
    },
    observer::{
        content_looks_like_nip44, OBSERVER_AGENT_TAG, OBSERVER_FRAME_CONTROL, OBSERVER_FRAME_TAG,
        OBSERVER_FRAME_TELEMETRY,
    },
    pulse::{validate_pulse_entry_envelope, PulseEntry, PULSE_ENTRY_TAG_VERSION},
};
use nostr::{EventBuilder, Kind, Tag};
use uuid::Uuid;

use crate::coding_session::{
    coding_session_lifecycle_receipt_semantic_key, coding_session_metadata_semantic_key,
    coding_session_provider_catalog_semantic_key, coding_session_transcript_semantic_key,
    encode_structured_key, CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION,
    CODING_SESSION_METADATA_TAG_VERSION, CODING_SESSION_PROVIDER_CATALOG_TAG_VERSION,
    CODING_SESSION_TRANSCRIPT_TAG_VERSION, MAX_LIFECYCLE_RECEIPT_CONTENT_BYTES,
    MAX_METADATA_CONTENT_BYTES, MAX_PROVIDER_CATALOG_CONTENT_BYTES, MAX_TRANSCRIPT_CONTENT_BYTES,
};
use crate::{
    ChannelKind, CustomEmoji, DiffMeta, MemberRole, SdkError, ThreadRef, Visibility, VoteDirection,
};

/// Parse a tag slice, mapping errors to `SdkError::InvalidTag`.
fn tag(parts: &[&str]) -> Result<Tag, SdkError> {
    Tag::parse(parts.iter().copied()).map_err(|e| SdkError::InvalidTag(e.to_string()))
}

/// Validate content byte length.
fn check_content(content: &str, max: usize) -> Result<(), SdkError> {
    let got = content.len();
    if got > max {
        return Err(SdkError::ContentTooLarge { max, got });
    }
    Ok(())
}

/// Validate hex string has at least `min_len` hex characters.
fn check_hex_len(s: &str, min_len: usize, field: &str) -> Result<(), SdkError> {
    if s.len() < min_len || !s.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(SdkError::InvalidDiffMeta(format!(
            "{field} must be at least {min_len} hex characters (got {:?})",
            s
        )));
    }
    Ok(())
}

/// Validate a git commit-like hex id (commit, parent-commit, euc,
/// merge-commit, applied-as-commit). Git object ids are full SHA-1 (40 hex
/// chars) or SHA-256 (64 hex chars) — anything shorter is an abbreviated
/// ref that NIP-34 canonical tags shouldn't carry, since consumers resolve
/// these against the actual repo.
fn check_commit_hex(s: &str, field: &str) -> Result<(), SdkError> {
    if (s.len() != 40 && s.len() != 64) || !s.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(SdkError::InvalidInput(format!(
            "{field} must be a full 40-character (SHA-1) or 64-character (SHA-256) hex commit id (got {:?})",
            s
        )));
    }
    Ok(())
}

fn check_pubkey_hex(s: &str, field: &str) -> Result<String, SdkError> {
    if s.len() != 64 || !s.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(SdkError::InvalidInput(format!(
            "{field} must be a 64-character hex pubkey"
        )));
    }
    Ok(s.to_ascii_lowercase())
}

/// Validate an exact-length hex string (event ids), returning it lowercased.
fn check_hex_exact(s: &str, len: usize, field: &str) -> Result<String, SdkError> {
    if s.len() != len || !s.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(SdkError::InvalidInput(format!(
            "{field} must be a {len}-character hex string"
        )));
    }
    Ok(s.to_ascii_lowercase())
}

/// Validate a git repo identifier: `[a-zA-Z0-9._-]{1,64}`, no leading dot,
/// no `..`. Shared by `build_repo_announcement` and `GitRepoCoord` so a
/// repo coordinate built directly through the SDK (bypassing CLI-side
/// `validate_repo_id`) can't slip an invalid `d`-tag into an `a`-tag value.
fn check_repo_id(repo_id: &str) -> Result<(), SdkError> {
    if repo_id.is_empty() {
        return Err(SdkError::InvalidInput("repo_id must not be empty".into()));
    }
    if repo_id.len() > 64 {
        return Err(SdkError::InvalidInput(format!(
            "repo_id exceeds 64 characters (got {})",
            repo_id.len()
        )));
    }
    if !repo_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
    {
        return Err(SdkError::InvalidInput(
            "repo_id may only contain [a-zA-Z0-9._-]".into(),
        ));
    }
    if repo_id.starts_with('.') {
        return Err(SdkError::InvalidInput(
            "repo_id must not start with a dot".into(),
        ));
    }
    if repo_id.contains("..") {
        return Err(SdkError::InvalidInput(
            "repo_id must not contain '..'".into(),
        ));
    }
    Ok(())
}

/// Maximum length of a custom emoji shortcode.
pub const MAX_CUSTOM_EMOJI_SHORTCODE_LEN: usize = 64;
/// Maximum reaction payload length for a colon-wrapped custom emoji shortcode.
pub const MAX_CUSTOM_EMOJI_REACTION_LEN: usize = MAX_CUSTOM_EMOJI_SHORTCODE_LEN + 2;

/// Validate and normalize a NIP-30 custom emoji shortcode.
///
/// Shortcodes are case-insensitive in Buzz's relay-global set; lowercase
/// normalization prevents `party_parrot` and `Party_Parrot` from colliding.
pub fn normalize_custom_emoji_shortcode(shortcode: &str) -> Result<String, SdkError> {
    let trimmed = shortcode.trim().trim_matches(':');
    if trimmed.is_empty() {
        return Err(SdkError::InvalidInput(
            "emoji shortcode must not be empty".into(),
        ));
    }
    if trimmed.len() > MAX_CUSTOM_EMOJI_SHORTCODE_LEN {
        return Err(SdkError::InvalidInput(format!(
            "emoji shortcode exceeds {MAX_CUSTOM_EMOJI_SHORTCODE_LEN} bytes (got {})",
            trimmed.len()
        )));
    }
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(SdkError::InvalidInput(
            "emoji shortcode may only contain ASCII letters, digits, hyphens, and underscores"
                .into(),
        ));
    }
    Ok(trimmed.to_ascii_lowercase())
}

fn check_custom_emoji_url(url: &str) -> Result<(), SdkError> {
    if url.is_empty() {
        return Err(SdkError::InvalidInput(
            "emoji image URL must not be empty".into(),
        ));
    }
    if url.len() > 2048 {
        return Err(SdkError::InvalidInput(format!(
            "emoji image URL exceeds 2048 bytes (got {})",
            url.len()
        )));
    }
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(SdkError::InvalidInput(
            "emoji image URL must start with http:// or https://".into(),
        ));
    }
    Ok(())
}

/// Emit NIP-10 e-tags for a `ThreadRef`.
fn thread_tags(thread_ref: &ThreadRef, tags: &mut Vec<Tag>) -> Result<(), SdkError> {
    let root = thread_ref.root_event_id.to_hex();
    let parent = thread_ref.parent_event_id.to_hex();
    if root == parent {
        // Direct reply
        tags.push(tag(&["e", &root, "", "reply"])?);
    } else {
        // Nested reply
        tags.push(tag(&["e", &root, "", "root"])?);
        tags.push(tag(&["e", &parent, "", "reply"])?);
    }
    Ok(())
}

/// Deduplicate and cap mentions, emitting p-tags.
fn mention_tags(mentions: &[&str], tags: &mut Vec<Tag>) -> Result<(), SdkError> {
    if mentions.len() > crate::mentions::MENTION_CAP {
        return Err(SdkError::TooManyMentions);
    }
    let mut seen = std::collections::HashSet::new();
    for &hex in mentions {
        let lower = hex.to_ascii_lowercase();
        if seen.insert(lower.clone()) {
            tags.push(tag(&["p", &lower])?);
        }
    }
    Ok(())
}

/// Emit imeta tags from raw tag vectors.
fn imeta_tags(media_tags: &[Vec<String>], tags: &mut Vec<Tag>) -> Result<(), SdkError> {
    for mt in media_tags {
        let parts: Vec<&str> = mt.iter().map(String::as_str).collect();
        tags.push(Tag::parse(parts).map_err(|e| SdkError::InvalidTag(e.to_string()))?);
    }
    Ok(())
}

/// Build a stream message (kind 9).
///
/// - `channel_id`: target channel UUID
/// - `content`: message text (max 64 KiB)
/// - `thread_ref`: optional NIP-10 reply context
/// - `mentions`: pubkey hex strings to p-tag (deduped, max 50)
/// - `broadcast`: if true, adds `["broadcast", "1"]` tag
/// - `media_tags`: raw imeta tag vectors
pub fn build_message(
    channel_id: Uuid,
    content: &str,
    thread_ref: Option<&ThreadRef>,
    mentions: &[&str],
    broadcast: bool,
    media_tags: &[Vec<String>],
) -> Result<EventBuilder, SdkError> {
    check_content(content, 64 * 1024)?;
    let mut tags = vec![tag(&["h", &channel_id.to_string()])?];
    if let Some(tr) = thread_ref {
        thread_tags(tr, &mut tags)?;
    }
    mention_tags(mentions, &mut tags)?;
    if broadcast {
        tags.push(tag(&["broadcast", "1"])?);
    }
    imeta_tags(media_tags, &mut tags)?;
    Ok(EventBuilder::new(Kind::Custom(9), content)
        .tags(tags)
        .allow_self_tagging())
}

/// Build an encrypted agent observer frame (kind 24200).
///
/// `recipient_pubkey` is the cleartext `p` tag used by the relay for owner-only
/// routing. `agent_pubkey` identifies the managed agent whose observer stream
/// this frame belongs to. `encrypted_content` must be NIP-44 v2 ciphertext.
pub fn build_agent_observer_frame(
    recipient_pubkey: &str,
    agent_pubkey: &str,
    frame: &str,
    encrypted_content: &str,
) -> Result<EventBuilder, SdkError> {
    if frame != OBSERVER_FRAME_TELEMETRY && frame != OBSERVER_FRAME_CONTROL {
        return Err(SdkError::InvalidInput(format!(
            "observer frame must be {OBSERVER_FRAME_TELEMETRY:?} or {OBSERVER_FRAME_CONTROL:?}"
        )));
    }
    if !content_looks_like_nip44(encrypted_content) {
        return Err(SdkError::InvalidInput(
            "observer frame content must be NIP-44 v2 ciphertext".into(),
        ));
    }

    let recipient_pubkey = check_pubkey_hex(recipient_pubkey, "recipient_pubkey")?;
    let agent_pubkey = check_pubkey_hex(agent_pubkey, "agent_pubkey")?;
    let tags = vec![
        tag(&["p", &recipient_pubkey])?,
        tag(&[OBSERVER_AGENT_TAG, &agent_pubkey])?,
        tag(&[OBSERVER_FRAME_TAG, frame])?,
    ];

    Ok(EventBuilder::new(
        Kind::Custom(KIND_AGENT_OBSERVER_FRAME as u16),
        encrypted_content,
    )
    .tags(tags))
}

/// Build a forum post thread root (kind 45001).
pub fn build_forum_post(
    channel_id: Uuid,
    content: &str,
    mentions: &[&str],
    media_tags: &[Vec<String>],
) -> Result<EventBuilder, SdkError> {
    check_content(content, 64 * 1024)?;
    let mut tags = vec![tag(&["h", &channel_id.to_string()])?];
    mention_tags(mentions, &mut tags)?;
    imeta_tags(media_tags, &mut tags)?;
    Ok(EventBuilder::new(Kind::Custom(45001), content)
        .tags(tags)
        .allow_self_tagging())
}

/// Build a forum comment reply (kind 45003).
pub fn build_forum_comment(
    channel_id: Uuid,
    content: &str,
    thread_ref: &ThreadRef,
    mentions: &[&str],
    media_tags: &[Vec<String>],
) -> Result<EventBuilder, SdkError> {
    check_content(content, 64 * 1024)?;
    let mut tags = vec![tag(&["h", &channel_id.to_string()])?];
    thread_tags(thread_ref, &mut tags)?;
    mention_tags(mentions, &mut tags)?;
    imeta_tags(media_tags, &mut tags)?;
    Ok(EventBuilder::new(Kind::Custom(45003), content)
        .tags(tags)
        .allow_self_tagging())
}

/// Build a diff/patch message (kind 40008).
pub fn build_diff_message(
    channel_id: Uuid,
    content: &str,
    diff_meta: &DiffMeta,
    thread_ref: Option<&ThreadRef>,
) -> Result<EventBuilder, SdkError> {
    check_content(content, 60 * 1024)?;

    // Validate DiffMeta
    if !diff_meta.repo_url.starts_with("http://") && !diff_meta.repo_url.starts_with("https://") {
        return Err(SdkError::InvalidDiffMeta(
            "repo_url must start with http:// or https://".into(),
        ));
    }
    check_hex_len(&diff_meta.commit_sha, 7, "commit_sha")?;
    if let Some(ref pc) = diff_meta.parent_commit {
        check_hex_len(pc, 7, "parent_commit")?;
    }
    match &diff_meta.branch {
        Some((src, tgt)) if src.is_empty() || tgt.is_empty() => {
            return Err(SdkError::InvalidDiffMeta(
                "branch requires both source and target to be non-empty".into(),
            ));
        }
        _ => {}
    }
    if let Some(pr) = diff_meta.pr_number {
        if pr == 0 {
            return Err(SdkError::InvalidDiffMeta(
                "pr_number must be positive".into(),
            ));
        }
    }

    let mut tags = vec![
        tag(&["h", &channel_id.to_string()])?,
        tag(&["repo", &diff_meta.repo_url])?,
        tag(&["commit", &diff_meta.commit_sha])?,
    ];
    if let Some(ref fp) = diff_meta.file_path {
        tags.push(tag(&["file", fp])?);
    }
    if let Some(ref pc) = diff_meta.parent_commit {
        tags.push(tag(&["parent-commit", pc])?);
    }
    if let Some((ref src, ref tgt)) = diff_meta.branch {
        tags.push(tag(&["branch", src, tgt])?);
    }
    if let Some(pr) = diff_meta.pr_number {
        tags.push(tag(&["pr", &pr.to_string()])?);
    }
    if let Some(ref lang) = diff_meta.language {
        tags.push(tag(&["l", lang])?);
    }
    if let Some(ref desc) = diff_meta.description {
        tags.push(tag(&["description", desc])?);
    }
    if diff_meta.truncated {
        tags.push(tag(&["truncated", "true"])?);
    }
    if let Some(ref alt) = diff_meta.alt_text {
        tags.push(tag(&["alt", alt])?);
    }
    if let Some(tr) = thread_ref {
        thread_tags(tr, &mut tags)?;
    }
    Ok(EventBuilder::new(Kind::Custom(40008), content).tags(tags))
}

/// Build an edit event targeting an existing message (kind 40003).
pub fn build_edit(
    channel_id: Uuid,
    target_event_id: nostr::EventId,
    new_content: &str,
) -> Result<EventBuilder, SdkError> {
    check_content(new_content, 64 * 1024)?;
    let tags = vec![
        tag(&["h", &channel_id.to_string()])?,
        tag(&["e", &target_event_id.to_hex()])?,
    ];
    Ok(EventBuilder::new(Kind::Custom(40003), new_content).tags(tags))
}

/// Optional metadata for moderator delete tombstones (kind 9005).
#[derive(Debug, Clone, Default)]
pub struct DeleteMessageOptions<'a> {
    /// Audit action UUID to link from the public tombstone.
    pub action_id: Option<Uuid>,
    /// Machine-readable, public-safe reason code.
    pub reason_code: Option<&'a str>,
    /// Human-readable reason safe for the room-facing tombstone.
    pub public_reason: Option<&'a str>,
}

/// Build a Buzz-native delete event (kind 9005).
pub fn build_delete_message(
    channel_id: Uuid,
    target_event_id: nostr::EventId,
) -> Result<EventBuilder, SdkError> {
    build_delete_message_with_options(channel_id, target_event_id, DeleteMessageOptions::default())
}

/// Build a Buzz-native delete event (kind 9005) with optional moderation metadata.
pub fn build_delete_message_with_options(
    channel_id: Uuid,
    target_event_id: nostr::EventId,
    options: DeleteMessageOptions<'_>,
) -> Result<EventBuilder, SdkError> {
    let mut tags = vec![
        tag(&["h", &channel_id.to_string()])?,
        tag(&["e", &target_event_id.to_hex()])?,
    ];
    if let Some(action_id) = options.action_id {
        tags.push(tag(&["action_id", &action_id.to_string()])?);
    }
    if let Some(reason_code) = options.reason_code {
        tags.push(tag(&["reason_code", reason_code])?);
    }
    if let Some(public_reason) = options.public_reason {
        tags.push(tag(&["public_reason", public_reason])?);
    }
    Ok(EventBuilder::new(Kind::Custom(9005), "").tags(tags))
}

/// Build a NIP-09 deletion event (kind 5). The `h` tag is non-standard for
/// NIP-09 but is required so channel-scoped subscriptions observe the delete.
pub fn build_delete_compat(
    channel_id: Uuid,
    target_event_id: nostr::EventId,
) -> Result<EventBuilder, SdkError> {
    let tags = vec![
        tag(&["h", &channel_id.to_string()])?,
        tag(&["e", &target_event_id.to_hex()])?,
    ];
    Ok(EventBuilder::new(Kind::Custom(5), "").tags(tags))
}

/// Build a forum vote event (kind 45002). Content is `"+"` or `"-"`.
pub fn build_vote(
    channel_id: Uuid,
    target_event_id: nostr::EventId,
    direction: VoteDirection,
) -> Result<EventBuilder, SdkError> {
    let content = match direction {
        VoteDirection::Up => "+",
        VoteDirection::Down => "-",
    };
    let tags = vec![
        tag(&["h", &channel_id.to_string()])?,
        tag(&["e", &target_event_id.to_hex()])?,
    ];
    Ok(EventBuilder::new(Kind::Custom(45002), content).tags(tags))
}

/// Build a NIP-25 reaction event (kind 7). Emoji max 64 chars.
pub fn build_reaction(
    target_event_id: nostr::EventId,
    emoji: &str,
) -> Result<EventBuilder, SdkError> {
    if emoji.chars().count() > 64 {
        return Err(SdkError::EmojiTooLong);
    }
    let tags = vec![tag(&["e", &target_event_id.to_hex()])?];
    Ok(EventBuilder::new(Kind::Custom(7), emoji).tags(tags))
}

/// Build a NIP-25 reaction event using a NIP-30 custom emoji.
///
/// The reaction content is `:shortcode:` and the event carries exactly one
/// `["emoji", shortcode, url]` tag, matching NIP-25's custom emoji reaction
/// guidance.
pub fn build_custom_emoji_reaction(
    target_event_id: nostr::EventId,
    shortcode: &str,
    url: &str,
) -> Result<EventBuilder, SdkError> {
    let shortcode = normalize_custom_emoji_shortcode(shortcode)?;
    check_custom_emoji_url(url)?;
    let content = format!(":{shortcode}:");
    let tags = vec![
        tag(&["e", &target_event_id.to_hex()])?,
        tag(&["emoji", &shortcode, url])?,
    ];
    Ok(EventBuilder::new(Kind::Custom(7), content).tags(tags))
}

/// Build a deletion event targeting a reaction (kind 5).
pub fn build_remove_reaction(reaction_event_id: nostr::EventId) -> Result<EventBuilder, SdkError> {
    let tags = vec![tag(&["e", &reaction_event_id.to_hex()])?];
    Ok(EventBuilder::new(Kind::Custom(5), "").tags(tags))
}

/// d-tag for a member's own custom emoji set. Each member publishes one
/// user-signed kind:30030 under this d-tag; the workspace palette is the
/// client-side union of every member's set.
pub const CUSTOM_EMOJI_SET_D_TAG: &str = "buzz:custom-emoji";

/// Build a member's own custom emoji set event (kind:30030, NIP-30/NIP-51).
///
/// User-signed and parameterized-replaceable, keyed by `(pubkey, 30030,
/// "buzz:custom-emoji")`. Replaces the caller's prior set. The workspace
/// palette shown in clients is the union of every member's set, deduped by
/// `(shortcode, url)` on read. Add/remove is read-own-set → mutate → rebuild.
pub fn build_custom_emoji_set(emojis: &[CustomEmoji]) -> Result<EventBuilder, SdkError> {
    let mut seen = std::collections::HashSet::with_capacity(emojis.len());
    let mut tags = Vec::with_capacity(emojis.len() + 1);
    tags.push(tag(&["d", CUSTOM_EMOJI_SET_D_TAG])?);
    for emoji in emojis {
        let shortcode = normalize_custom_emoji_shortcode(&emoji.shortcode)?;
        check_custom_emoji_url(&emoji.url)?;
        if !seen.insert(shortcode.clone()) {
            return Err(SdkError::InvalidInput(format!(
                "duplicate emoji shortcode: {shortcode}"
            )));
        }
        tags.push(tag(&["emoji", &shortcode, &emoji.url])?);
    }
    Ok(EventBuilder::new(Kind::Custom(KIND_EMOJI_SET as u16), "").tags(tags))
}

/// Build a canvas update event (kind 40100).
pub fn build_set_canvas(channel_id: Uuid, content: &str) -> Result<EventBuilder, SdkError> {
    let tags = vec![tag(&["h", &channel_id.to_string()])?];
    Ok(EventBuilder::new(Kind::Custom(40100), content).tags(tags))
}

/// Build a NIP-01 profile metadata event (kind 0).
///
/// Only present (Some) fields are included in the JSON object.
pub fn build_profile(
    display_name: Option<&str>,
    name: Option<&str>,
    picture: Option<&str>,
    about: Option<&str>,
    nip05: Option<&str>,
) -> Result<EventBuilder, SdkError> {
    let mut map = serde_json::Map::new();
    if let Some(v) = display_name {
        map.insert("display_name".into(), serde_json::Value::String(v.into()));
    }
    if let Some(v) = name {
        map.insert("name".into(), serde_json::Value::String(v.into()));
    }
    if let Some(v) = picture {
        map.insert("picture".into(), serde_json::Value::String(v.into()));
    }
    if let Some(v) = about {
        map.insert("about".into(), serde_json::Value::String(v.into()));
    }
    if let Some(v) = nip05 {
        map.insert("nip05".into(), serde_json::Value::String(v.into()));
    }
    let content = serde_json::Value::Object(map).to_string();
    Ok(EventBuilder::new(Kind::Custom(0), content).tags([]))
}

/// Build a NIP-29 add-member event (kind 9000).
pub fn build_add_member(
    channel_id: Uuid,
    target_pubkey: &str,
    role: Option<MemberRole>,
) -> Result<EventBuilder, SdkError> {
    check_hex_len(target_pubkey, 64, "target_pubkey")?;
    let mut tags = vec![
        tag(&["h", &channel_id.to_string()])?,
        tag(&["p", &target_pubkey.to_ascii_lowercase()])?,
    ];
    if let Some(r) = role {
        tags.push(tag(&["role", r.as_str()])?);
    }
    Ok(EventBuilder::new(Kind::Custom(9000), "").tags(tags))
}

/// Build a NIP-29 remove-member event (kind 9001).
pub fn build_remove_member(
    channel_id: Uuid,
    target_pubkey: &str,
) -> Result<EventBuilder, SdkError> {
    check_hex_len(target_pubkey, 64, "target_pubkey")?;
    let tags = vec![
        tag(&["h", &channel_id.to_string()])?,
        tag(&["p", &target_pubkey.to_ascii_lowercase()])?,
    ];
    Ok(EventBuilder::new(Kind::Custom(9001), "").tags(tags))
}

/// Build a NIP-29 leave-request event (kind 9022).
pub fn build_leave(channel_id: Uuid) -> Result<EventBuilder, SdkError> {
    let tags = vec![tag(&["h", &channel_id.to_string()])?];
    Ok(EventBuilder::new(Kind::Custom(9022), "").tags(tags))
}

/// Build a NIP-29 edit-metadata event for name/about/visibility/ttl (kind 9002).
///
/// `ttl`: outer `None` leaves it unchanged; `Some(Some(secs))` sets the
/// ephemeral timeout; `Some(None)` clears it (emits `["ttl", ""]`).
pub fn build_update_channel(
    channel_id: Uuid,
    name: Option<&str>,
    about: Option<&str>,
    visibility: Option<&str>,
    ttl: Option<Option<i32>>,
) -> Result<EventBuilder, SdkError> {
    if name.is_none() && about.is_none() && visibility.is_none() && ttl.is_none() {
        return Err(SdkError::InvalidTag(
            "at least one of name, about, visibility, or ttl must be provided".into(),
        ));
    }
    if let Some(v) = visibility {
        if v != "open" && v != "private" {
            return Err(SdkError::InvalidTag(
                "visibility must be \"open\" or \"private\"".into(),
            ));
        }
    }
    if name
        .map(buzz_core::channel::canonical_channel_name)
        .is_some_and(|name| name.trim().is_empty())
    {
        return Err(SdkError::InvalidTag("channel name is required".into()));
    }
    let mut tags = vec![tag(&["h", &channel_id.to_string()])?];
    if let Some(n) = name {
        tags.push(tag(&[
            "name",
            buzz_core::channel::canonical_channel_name(n),
        ])?);
    }
    if let Some(a) = about {
        tags.push(tag(&["about", a])?);
    }
    if let Some(v) = visibility {
        tags.push(tag(&["visibility", v])?);
    }
    if let Some(ttl) = ttl {
        match ttl {
            Some(secs) => tags.push(tag(&["ttl", &secs.to_string()])?),
            None => tags.push(tag(&["ttl", ""])?),
        }
    }
    Ok(EventBuilder::new(Kind::Custom(9002), "").tags(tags))
}

/// Build a NIP-29 edit-metadata event for topic (kind 9002).
pub fn build_set_topic(channel_id: Uuid, topic: &str) -> Result<EventBuilder, SdkError> {
    let tags = vec![
        tag(&["h", &channel_id.to_string()])?,
        tag(&["topic", topic])?,
    ];
    Ok(EventBuilder::new(Kind::Custom(9002), "").tags(tags))
}

/// Build a NIP-29 edit-metadata event for purpose (kind 9002).
pub fn build_set_purpose(channel_id: Uuid, purpose: &str) -> Result<EventBuilder, SdkError> {
    let tags = vec![
        tag(&["h", &channel_id.to_string()])?,
        tag(&["purpose", purpose])?,
    ];
    Ok(EventBuilder::new(Kind::Custom(9002), "").tags(tags))
}

/// Build a NIP-29 create-group event (kind 9007).
///
/// `ttl`: `Some(secs)` makes the channel ephemeral with that lifetime in
/// seconds (the relay archives it once the deadline passes without activity);
/// `None` leaves it permanent.
pub fn build_create_channel(
    channel_id: Uuid,
    name: &str,
    visibility: Option<Visibility>,
    channel_type: Option<ChannelKind>,
    about: Option<&str>,
    ttl: Option<i32>,
) -> Result<EventBuilder, SdkError> {
    let name = buzz_core::channel::canonical_channel_name(name);
    if name.trim().is_empty() {
        return Err(SdkError::InvalidTag("channel name is required".into()));
    }
    let mut tags = vec![tag(&["h", &channel_id.to_string()])?, tag(&["name", name])?];
    if let Some(v) = visibility {
        tags.push(tag(&["visibility", v.as_str()])?);
    }
    if let Some(ct) = channel_type {
        tags.push(tag(&["channel_type", ct.as_str()])?);
    }
    if let Some(a) = about {
        tags.push(tag(&["about", a])?);
    }
    if let Some(secs) = ttl {
        tags.push(tag(&["ttl", &secs.to_string()])?);
    }
    Ok(EventBuilder::new(Kind::Custom(9007), "").tags(tags))
}

/// Build a NIP-29 join-request event (kind 9021).
pub fn build_join(channel_id: Uuid) -> Result<EventBuilder, SdkError> {
    let tags = vec![tag(&["h", &channel_id.to_string()])?];
    Ok(EventBuilder::new(Kind::Custom(9021), "").tags(tags))
}

/// Build a NIP-29 archive event (kind 9002, `["archived", "true"]`).
pub fn build_archive(channel_id: Uuid) -> Result<EventBuilder, SdkError> {
    let tags = vec![
        tag(&["h", &channel_id.to_string()])?,
        tag(&["archived", "true"])?,
    ];
    Ok(EventBuilder::new(Kind::Custom(9002), "").tags(tags))
}

/// Build a NIP-29 unarchive event (kind 9002, `["archived", "false"]`).
pub fn build_unarchive(channel_id: Uuid) -> Result<EventBuilder, SdkError> {
    let tags = vec![
        tag(&["h", &channel_id.to_string()])?,
        tag(&["archived", "false"])?,
    ];
    Ok(EventBuilder::new(Kind::Custom(9002), "").tags(tags))
}

/// Build a NIP-29 delete-group event (kind 9008).
pub fn build_delete_channel(channel_id: Uuid) -> Result<EventBuilder, SdkError> {
    let tags = vec![tag(&["h", &channel_id.to_string()])?];
    Ok(EventBuilder::new(Kind::Custom(9008), "").tags(tags))
}

/// Build a global text note (kind:1, NIP-01).
///
/// `reply_to_event_id`: adds a single `["e", <id>, "", "reply"]` tag.
/// This is intentionally simpler than the full `ThreadRef` mechanism used
/// for channel messages — social notes use a flat reply model for now.
/// Full NIP-10 threading (root + reply + p-tags) is deferred.
pub fn build_note(
    content: &str,
    reply_to_event_id: Option<nostr::EventId>,
) -> Result<EventBuilder, SdkError> {
    check_content(content, 64 * 1024)?;
    let mut tags = vec![];
    if let Some(reply_id) = reply_to_event_id {
        tags.push(tag(&["e", &reply_id.to_hex(), "", "reply"])?);
    }
    Ok(EventBuilder::new(Kind::Custom(1), content).tags(tags))
}

/// Maximum number of contacts allowed in a single contact list event.
const MAX_CONTACTS: usize = 10_000;

/// Build a contact list replacement event (kind:3, NIP-02).
///
/// Each contact is `(pubkey_hex, relay_url, petname)`.
/// `pubkey_hex` must be exactly 64 hex characters (any case accepted, normalized
/// to lowercase before storage). Non-hex or wrong-length pubkeys are rejected
/// with `SdkError::InvalidInput`.
/// `relay_url` and `petname` may be `None` (stored as empty string per NIP-02).
///
/// Duplicate pubkeys are silently deduplicated — the first occurrence is kept.
///
/// Replaces the entire contact list — callers must read-before-write for deltas.
pub fn build_contact_list(
    contacts: &[(&str, Option<&str>, Option<&str>)],
) -> Result<EventBuilder, SdkError> {
    if contacts.len() > MAX_CONTACTS {
        return Err(SdkError::InvalidInput(format!(
            "contact list exceeds maximum of {} contacts (got {})",
            MAX_CONTACTS,
            contacts.len()
        )));
    }
    let mut seen = std::collections::HashSet::with_capacity(contacts.len());
    let mut tags = Vec::with_capacity(contacts.len());
    for &(pubkey_hex, relay_url, petname) in contacts {
        if pubkey_hex.len() != 64 || !pubkey_hex.chars().all(|ch| ch.is_ascii_hexdigit()) {
            return Err(SdkError::InvalidInput(format!(
                "contact pubkey must be exactly 64 hex chars, got len={}",
                pubkey_hex.len()
            )));
        }
        if let Some(url) = relay_url {
            if url.len() > 2048 {
                return Err(SdkError::InvalidInput(format!(
                    "relay_url exceeds 2048 bytes (got {})",
                    url.len()
                )));
            }
        }
        if let Some(name) = petname {
            if name.len() > 256 {
                return Err(SdkError::InvalidInput(format!(
                    "petname exceeds 256 bytes (got {})",
                    name.len()
                )));
            }
        }
        let lower = pubkey_hex.to_ascii_lowercase();
        if !seen.insert(lower.clone()) {
            continue;
        }
        tags.push(tag(&[
            "p",
            &lower,
            relay_url.unwrap_or(""),
            petname.unwrap_or(""),
        ])?);
    }
    Ok(EventBuilder::new(Kind::Custom(3), "").tags(tags))
}

/// Extract the channel UUID from an event's `h` tag.
///
/// Returns `None` if no `h` tag is present or the value is not a valid UUID.
pub fn extract_channel_id(event: &nostr::Event) -> Option<Uuid> {
    event.tags.iter().find_map(|t| {
        let vec = t.as_slice();
        if vec.first().map(|s| s.as_str()) == Some("h") {
            vec.get(1).and_then(|v| Uuid::parse_str(v.as_str()).ok())
        } else {
            None
        }
    })
}

/// Build a git repository announcement event (kind:30617, NIP-34).
///
/// Creates or updates a repository. The `repo_id` is the unique identifier
/// (d-tag) — must be `[a-zA-Z0-9._-]{1,64}`, no leading dots, no `..`.
///
/// This is a parameterized replaceable event: publishing again with the same
/// `repo_id` updates the announcement (relay overwrites the previous one).
pub fn build_repo_announcement(
    repo_id: &str,
    name: Option<&str>,
    description: Option<&str>,
    clone_urls: &[&str],
    web_url: Option<&str>,
    relays: &[&str],
) -> Result<EventBuilder, SdkError> {
    // Validate repo_id
    check_repo_id(repo_id)?;

    // Validate optional name
    if let Some(n) = name {
        if n.len() > 128 {
            return Err(SdkError::InvalidInput(format!(
                "name exceeds 128 characters (got {})",
                n.len()
            )));
        }
    }

    // Validate optional description
    if let Some(d) = description {
        if d.len() > 1024 {
            return Err(SdkError::InvalidInput(format!(
                "description exceeds 1024 characters (got {})",
                d.len()
            )));
        }
    }

    // Validate clone_urls
    if clone_urls.len() > 5 {
        return Err(SdkError::InvalidInput(format!(
            "too many clone_urls (max 5, got {})",
            clone_urls.len()
        )));
    }
    for url in clone_urls {
        if url.is_empty() {
            return Err(SdkError::InvalidInput("clone_url must not be empty".into()));
        }
        if url.len() > 512 {
            return Err(SdkError::InvalidInput(format!(
                "clone_url exceeds 512 characters (got {})",
                url.len()
            )));
        }
    }

    // Validate web_url
    if let Some(url) = web_url {
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err(SdkError::InvalidInput(format!(
                "web_url must start with http:// or https:// (got {:?})",
                url
            )));
        }
        if url.len() > 512 {
            return Err(SdkError::InvalidInput(format!(
                "web_url exceeds 512 characters (got {})",
                url.len()
            )));
        }
    }

    // Validate relays
    if relays.len() > 10 {
        return Err(SdkError::InvalidInput(format!(
            "too many relays (max 10, got {})",
            relays.len()
        )));
    }
    for relay in relays {
        if !relay.starts_with("ws://") && !relay.starts_with("wss://") {
            return Err(SdkError::InvalidInput(format!(
                "relay must start with ws:// or wss:// (got {:?})",
                relay
            )));
        }
        if relay.len() > 256 {
            return Err(SdkError::InvalidInput(format!(
                "relay exceeds 256 characters (got {})",
                relay.len()
            )));
        }
    }

    // Build tags
    let mut tags = vec![tag(&["d", repo_id])?];
    if let Some(n) = name {
        tags.push(tag(&["name", n])?);
    }
    if let Some(d) = description {
        tags.push(tag(&["description", d])?);
    }
    if !clone_urls.is_empty() {
        let mut clone_tag = vec!["clone"];
        clone_tag.extend_from_slice(clone_urls);
        tags.push(tag(&clone_tag)?);
    }
    if let Some(url) = web_url {
        tags.push(tag(&["web", url])?);
    }
    if !relays.is_empty() {
        let mut relay_tag = vec!["relays"];
        relay_tag.extend_from_slice(relays);
        tags.push(tag(&relay_tag)?);
    }

    Ok(EventBuilder::new(Kind::Custom(KIND_GIT_REPO_ANNOUNCEMENT as u16), "").tags(tags))
}

/// Build a repository announcement while preserving caller-supplied metadata.
///
/// This is intended for read-modify-write updates to kind:30617 announcements.
/// Every supplied tag is retained except `d`; all existing `d` tags are replaced
/// with exactly one validated canonical repository identifier.
pub fn build_repo_announcement_with_tags(
    repo_id: &str,
    content: &str,
    mut tags: Vec<Tag>,
) -> Result<EventBuilder, SdkError> {
    check_repo_id(repo_id)?;
    tags.retain(|tag| tag.as_slice().first().map(String::as_str) != Some("d"));
    tags.insert(0, tag(&["d", repo_id])?);

    Ok(EventBuilder::new(Kind::Custom(KIND_GIT_REPO_ANNOUNCEMENT as u16), content).tags(tags))
}

/// Repository coordinate — owner pubkey + `d`-tag identifier.
///
/// Renders as the `a`-tag value clients use to address a kind:30617
/// announcement: `30617:<owner>:<id>`.
pub struct GitRepoCoord {
    /// 64-char hex pubkey of the repo's announcing owner.
    pub owner: String,
    /// The repo's `d`-tag identifier.
    pub id: String,
}

impl GitRepoCoord {
    fn to_a_tag_value(&self) -> Result<String, SdkError> {
        let owner = check_pubkey_hex(&self.owner, "repo owner")?;
        check_repo_id(&self.id)?;
        Ok(format!("30617:{owner}:{}", self.id))
    }
}

/// Metadata for a git patch event (kind:1617, NIP-34).
#[derive(Default)]
pub struct GitPatchMeta {
    /// Earliest-unique-commit of the repo (`r` tag, `euc` marker).
    pub euc: Option<String>,
    /// Additional pubkeys to `p`-tag besides the repo owner.
    pub recipients: Vec<String>,
    /// Previous patch in a series, or the original root patch when this is
    /// the first patch of a revision — emits `["e", id, "", "reply"]`.
    pub reply_to: Option<String>,
    /// First patch in a new series — emits `["t", "root"]`.
    pub root: bool,
    /// First patch in a revision of an existing series — emits `["t", "root-revision"]`.
    pub root_revision: bool,
    /// Commit ID this patch produces when applied (`commit` tag + `r` tag).
    pub commit: Option<String>,
    /// Parent commit ID (`parent-commit` tag).
    pub parent_commit: Option<String>,
    /// PGP signature of the commit, or `Some("")` for an explicitly unsigned commit.
    pub commit_pgp_sig: Option<String>,
    /// Committer identity: `(name, email, unix-timestamp, tz-offset-minutes)`.
    pub committer: Option<(String, String, String, String)>,
}

/// Build a git patch event (kind:1617, NIP-34).
///
/// `content` is the verbatim output of `git format-patch` — not truncated.
/// NIP-34 says patches SHOULD be used when under 60KB (PRs otherwise); this
/// builder enforces that bound rather than silently truncating a patch that
/// must remain applyable.
pub fn build_git_patch(
    repo: &GitRepoCoord,
    content: &str,
    meta: &GitPatchMeta,
) -> Result<EventBuilder, SdkError> {
    if content.trim().is_empty() {
        return Err(SdkError::InvalidInput(
            "patch content must not be empty — refusing to publish an unappliable patch".into(),
        ));
    }
    check_content(content, 60 * 1024)?;
    let a_value = repo.to_a_tag_value()?;
    let owner = check_pubkey_hex(&repo.owner, "repo owner")?;

    let mut tags = vec![tag(&["a", &a_value])?];
    if let Some(ref euc) = meta.euc {
        check_commit_hex(euc, "euc")?;
        tags.push(tag(&["r", euc, "euc"])?);
    }
    tags.push(tag(&["p", &owner])?);
    for recipient in &meta.recipients {
        let pk = check_pubkey_hex(recipient, "recipient")?;
        tags.push(tag(&["p", &pk])?);
    }
    if let Some(ref prev) = meta.reply_to {
        let event_id = check_hex_exact(prev, 64, "reply_to")?;
        tags.push(tag(&["e", &event_id, "", "reply"])?);
    }
    if meta.root && meta.root_revision {
        return Err(SdkError::InvalidInput(
            "patch cannot be both --root and --root-revision".into(),
        ));
    }
    if meta.root {
        tags.push(tag(&["t", "root"])?);
    }
    if meta.root_revision {
        tags.push(tag(&["t", "root-revision"])?);
    }
    if let Some(ref commit) = meta.commit {
        check_commit_hex(commit, "commit")?;
        tags.push(tag(&["commit", commit])?);
        tags.push(tag(&["r", commit])?);
    }
    if let Some(ref parent) = meta.parent_commit {
        check_commit_hex(parent, "parent_commit")?;
        tags.push(tag(&["parent-commit", parent])?);
    }
    if let Some(ref sig) = meta.commit_pgp_sig {
        tags.push(tag(&["commit-pgp-sig", sig])?);
    }
    if let Some((ref name, ref email, ref ts, ref tz)) = meta.committer {
        tags.push(tag(&["committer", name, email, ts, tz])?);
    }

    Ok(EventBuilder::new(Kind::Custom(KIND_GIT_PATCH as u16), content).tags(tags))
}

/// Metadata for a git issue event (kind:1621, NIP-34).
#[derive(Default)]
pub struct GitIssueMeta {
    /// Labels (`t` tags).
    pub labels: Vec<String>,
    /// Additional pubkeys to `p`-tag besides the repo owner.
    pub recipients: Vec<String>,
}

/// Build a git issue event (kind:1621, NIP-34). `content` is the markdown body.
pub fn build_git_issue(
    repo: &GitRepoCoord,
    subject: &str,
    content: &str,
    meta: &GitIssueMeta,
) -> Result<EventBuilder, SdkError> {
    check_content(content, 64 * 1024)?;
    if subject.is_empty() {
        return Err(SdkError::InvalidInput("subject must not be empty".into()));
    }
    if subject.len() > 256 {
        return Err(SdkError::InvalidInput(format!(
            "subject exceeds 256 characters (got {})",
            subject.len()
        )));
    }
    let a_value = repo.to_a_tag_value()?;
    let owner = check_pubkey_hex(&repo.owner, "repo owner")?;

    let mut tags = vec![tag(&["a", &a_value])?, tag(&["p", &owner])?];
    for recipient in &meta.recipients {
        let pk = check_pubkey_hex(recipient, "recipient")?;
        tags.push(tag(&["p", &pk])?);
    }
    tags.push(tag(&["subject", subject])?);
    for label in &meta.labels {
        tags.push(tag(&["t", label])?);
    }

    Ok(EventBuilder::new(Kind::Custom(KIND_GIT_ISSUE as u16), content).tags(tags))
}

/// Build an issue assignment note (kind:1) — a labeled comment whose `p`
/// tags are the assignees, mirroring the Desktop app's assignment events.
///
/// Tag layout: `["e", <issue>, "", "root"]`, `["a", <repo>]`, one `["p", ..]`
/// per assignee, and `["t", "assignment"]`.
///
/// Clients only trust assignments signed by the issue author or the repo
/// owner (who may assign anyone), or a self-assignment whose sole assignee
/// is the signer. Assignments from other signers are ignored on read.
pub fn build_git_issue_assignment(
    repo: &GitRepoCoord,
    issue_id: &str,
    assignees: &[String],
    content: &str,
) -> Result<EventBuilder, SdkError> {
    build_git_issue_assignment_with_prior(repo, issue_id, assignees, content, None)
}

/// Build an issue assignment note with an optional causal assignment-operation
/// event ID in a `["prior", <event-id>]` tag.
///
/// `prior`, when present, must be a 64-character hexadecimal event ID.
pub fn build_git_issue_assignment_with_prior(
    repo: &GitRepoCoord,
    issue_id: &str,
    assignees: &[String],
    content: &str,
    prior: Option<&str>,
) -> Result<EventBuilder, SdkError> {
    build_git_issue_assignee_operation(
        repo,
        issue_id,
        assignees,
        content,
        GitIssueAssigneeOperation::Assign,
        prior,
    )
}

/// Build an issue unassignment note (kind:1) whose `p` tags name the people
/// being removed and whose operation label is `t: unassignment`.
///
/// Clients trust unassignments signed by the issue author or repository owner,
/// or a self-unassignment whose sole `p` tag is the signer.
pub fn build_git_issue_unassignment(
    repo: &GitRepoCoord,
    issue_id: &str,
    assignees: &[String],
    content: &str,
) -> Result<EventBuilder, SdkError> {
    build_git_issue_unassignment_with_prior(repo, issue_id, assignees, content, None)
}

/// Build an issue unassignment note with an optional causal
/// assignment-operation event ID in a `["prior", <event-id>]` tag.
///
/// `prior`, when present, must be a 64-character hexadecimal event ID.
pub fn build_git_issue_unassignment_with_prior(
    repo: &GitRepoCoord,
    issue_id: &str,
    assignees: &[String],
    content: &str,
    prior: Option<&str>,
) -> Result<EventBuilder, SdkError> {
    build_git_issue_assignee_operation(
        repo,
        issue_id,
        assignees,
        content,
        GitIssueAssigneeOperation::Unassign,
        prior,
    )
}

#[derive(Clone, Copy)]
enum GitIssueAssigneeOperation {
    Assign,
    Unassign,
}

impl GitIssueAssigneeOperation {
    fn label(self) -> &'static str {
        match self {
            Self::Assign => "assignment",
            Self::Unassign => "unassignment",
        }
    }
}

fn build_git_issue_assignee_operation(
    repo: &GitRepoCoord,
    issue_id: &str,
    assignees: &[String],
    content: &str,
    operation: GitIssueAssigneeOperation,
    prior: Option<&str>,
) -> Result<EventBuilder, SdkError> {
    check_content(content, 64 * 1024)?;
    let issue = check_hex_exact(issue_id, 64, "issue")?;
    let a_value = repo.to_a_tag_value()?;
    if assignees.is_empty() || assignees.len() > 50 {
        return Err(SdkError::InvalidInput(
            "between 1 and 50 assignees are required".into(),
        ));
    }
    let mut normalized = assignees
        .iter()
        .map(|assignee| check_pubkey_hex(assignee, "assignee"))
        .collect::<Result<Vec<_>, _>>()?;
    normalized.sort();
    normalized.dedup();

    let mut tags = vec![tag(&["e", &issue, "", "root"])?, tag(&["a", &a_value])?];
    for assignee in &normalized {
        tags.push(tag(&["p", assignee])?);
    }
    tags.push(tag(&["t", operation.label()])?);
    if let Some(prior) = prior {
        let prior = check_hex_exact(prior, 64, "prior assignment operation")?;
        tags.push(tag(&["prior", &prior])?);
    }

    Ok(EventBuilder::new(Kind::Custom(1), content).tags(tags))
}

/// Status to apply to a patch or issue root (kind:1630/1631/1632/1633, NIP-34).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitStatus {
    /// 1630 — Open (the default state).
    Open,
    /// 1631 — Applied/Merged for patches; Resolved for issues.
    AppliedOrResolved,
    /// 1632 — Closed.
    Closed,
    /// 1633 — Draft.
    Draft,
}

/// A reference to an applied/merged patch event for a status `q` tag,
/// optionally carrying a relay-url and/or pubkey hint per NIP-34:
/// `['q', <id>, <relay-url>, <pubkey>]`.
///
/// Parsed from the CLI's `--q <id>[:<relay-url>[:<pubkey>]]` syntax via
/// [`GitAppliedPatchRef::parse`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitAppliedPatchRef {
    /// The applied/merged patch event id (64-char hex).
    pub id: String,
    /// Optional relay-url hint where the patch event can be found.
    pub relay: Option<String>,
    /// Optional pubkey hint of the patch's author. Only meaningful when
    /// `relay` is also set, per NIP-34's `['q', id, relay, pubkey]` shape.
    pub pubkey: Option<String>,
}

impl GitAppliedPatchRef {
    /// Parse `<id>`, `<id>:<relay-url>`, or `<id>:<relay-url>:<pubkey>`.
    ///
    /// The relay-url segment may itself contain `:` (e.g. `wss://host:port`),
    /// so splitting is bounded to at most 3 parts rather than splitting on
    /// every colon.
    pub fn parse(spec: &str) -> Result<Self, SdkError> {
        let mut parts = spec.splitn(3, ':');
        let id = parts.next().unwrap_or_default().to_string();
        let rest = parts.next();
        match rest {
            None => Ok(GitAppliedPatchRef {
                id,
                relay: None,
                pubkey: None,
            }),
            Some(_) => {
                // Re-split with the relay url glued back together, since a
                // relay URL itself contains colons (`wss://host:port`); the
                // pubkey, if present, is always the last `:`-delimited
                // segment.
                let rest_str = &spec[id.len() + 1..];
                if let Some(idx) = rest_str.rfind(':') {
                    let candidate_pubkey = &rest_str[idx + 1..];
                    if candidate_pubkey.len() == 64
                        && candidate_pubkey.chars().all(|c| c.is_ascii_hexdigit())
                    {
                        return Ok(GitAppliedPatchRef {
                            id,
                            relay: Some(rest_str[..idx].to_string()),
                            pubkey: Some(candidate_pubkey.to_ascii_lowercase()),
                        });
                    }
                }
                Ok(GitAppliedPatchRef {
                    id,
                    relay: Some(rest_str.to_string()),
                    pubkey: None,
                })
            }
        }
    }
}

impl GitStatus {
    fn kind(self) -> u16 {
        match self {
            GitStatus::Open => KIND_GIT_STATUS_OPEN as u16,
            GitStatus::AppliedOrResolved => KIND_GIT_STATUS_MERGED as u16,
            GitStatus::Closed => KIND_GIT_STATUS_CLOSED as u16,
            GitStatus::Draft => KIND_GIT_STATUS_DRAFT as u16,
        }
    }
}

/// Metadata for a git status event (kind:1630-1633, NIP-34). Applies to a
/// patch root, a patch revision root, an issue, or a PR.
#[derive(Default)]
pub struct GitStatusMeta {
    /// The issue/PR/original-root-patch event being given a status — required.
    pub root_event: String,
    /// When a revision was the one applied/merged, its root id.
    pub accepted_revision_root: Option<String>,
    /// Repo coordinate, included as an `a` tag for subscription efficiency.
    pub repo: Option<GitRepoCoord>,
    /// Earliest-unique-commit of the repo (`r` tag, no marker).
    pub euc: Option<String>,
    /// Additional `p` tags (root author, revision author, etc.) besides the repo owner.
    pub recipients: Vec<String>,
    /// Applied/merged patch event references (`q` tags) — kind:1631 only.
    pub applied_patches: Vec<GitAppliedPatchRef>,
    /// Merge commit id — kind:1631 only.
    pub merge_commit: Option<String>,
    /// Commit ids applied to the target branch — kind:1631 only.
    pub applied_as_commits: Vec<String>,
}

/// Build a git status event (kind:1630/1631/1632/1633, NIP-34).
/// `content` is optional markdown context for the status change.
pub fn build_git_status(
    status: GitStatus,
    content: &str,
    meta: &GitStatusMeta,
) -> Result<EventBuilder, SdkError> {
    check_content(content, 64 * 1024)?;
    let root = check_hex_exact(&meta.root_event, 64, "root_event")?;

    let mut tags = vec![tag(&["e", &root, "", "root"])?];
    if let Some(ref revision) = meta.accepted_revision_root {
        let revision = check_hex_exact(revision, 64, "accepted_revision_root")?;
        tags.push(tag(&["e", &revision, "", "reply"])?);
    }
    for recipient in &meta.recipients {
        let pk = check_pubkey_hex(recipient, "recipient")?;
        tags.push(tag(&["p", &pk])?);
    }
    if let Some(ref repo) = meta.repo {
        let a_value = repo.to_a_tag_value()?;
        tags.push(tag(&["a", &a_value])?);
    }
    if let Some(ref euc) = meta.euc {
        check_commit_hex(euc, "euc")?;
        tags.push(tag(&["r", euc])?);
    }

    if status != GitStatus::AppliedOrResolved
        && (!meta.applied_patches.is_empty()
            || meta.merge_commit.is_some()
            || !meta.applied_as_commits.is_empty())
    {
        return Err(SdkError::InvalidInput(
            "applied_patches/merge_commit/applied_as_commits only apply to the merged/resolved status".into(),
        ));
    }
    for patch_ref in &meta.applied_patches {
        let patch_id = check_hex_exact(&patch_ref.id, 64, "applied_patch")?;
        match (&patch_ref.relay, &patch_ref.pubkey) {
            (None, None) => tags.push(tag(&["q", &patch_id])?),
            (Some(relay), None) => tags.push(tag(&["q", &patch_id, relay])?),
            (Some(relay), Some(pubkey)) => {
                let pubkey = check_pubkey_hex(pubkey, "applied_patch pubkey")?;
                tags.push(tag(&["q", &patch_id, relay, &pubkey])?)
            }
            (None, Some(_)) => {
                return Err(SdkError::InvalidInput(
                    "applied_patch pubkey hint requires a relay-url hint".into(),
                ))
            }
        }
    }
    if let Some(ref merge_commit) = meta.merge_commit {
        check_commit_hex(merge_commit, "merge_commit")?;
        tags.push(tag(&["merge-commit", merge_commit])?);
        tags.push(tag(&["r", merge_commit])?);
    }
    if !meta.applied_as_commits.is_empty() {
        let mut commits_tag = vec!["applied-as-commits"];
        for commit in &meta.applied_as_commits {
            check_commit_hex(commit, "applied_as_commit")?;
        }
        commits_tag.extend(meta.applied_as_commits.iter().map(String::as_str));
        tags.push(tag(&commits_tag)?);
        for commit in &meta.applied_as_commits {
            tags.push(tag(&["r", commit])?);
        }
    }

    Ok(EventBuilder::new(Kind::Custom(status.kind()), content).tags(tags))
}

/// Metadata for a git pull-request event (kind:1618, NIP-34).
///
/// A PR points reviewers at a branch tip they can fetch — `commit` (the tip)
/// plus at least one `clone_urls` entry where that commit is reachable. Unlike
/// a [`build_git_patch`], the change is *not* inlined; the diff is whatever the
/// tip introduces over its merge base. Per NIP-34 the tip SHOULD already be
/// pushed to `refs/nostr/<pr-event-id>` (or otherwise reachable) in the clone
/// repos before the event is signed — this builder does no network work and
/// does not verify reachability, mirroring [`build_git_patch`]'s philosophy.
#[derive(Default)]
pub struct GitPullRequestMeta {
    /// Earliest-unique-commit of the repo (`r` tag) — lets clients subscribe
    /// to all PRs against a local repo.
    pub euc: Option<String>,
    /// Additional pubkeys to `p`-tag besides the repo owner.
    pub recipients: Vec<String>,
    /// NIP-29 channel where the pull request originated (`h` tag).
    pub channel_id: Option<String>,
    /// PR subject line (`subject` tag) — required, used as the header.
    pub subject: String,
    /// Labels (`t` tags).
    pub labels: Vec<String>,
    /// Tip commit id of the PR branch (`c` tag) — required.
    pub commit: String,
    /// Clone URL(s) where the tip can be fetched (`clone` tag) — at least one.
    pub clone_urls: Vec<String>,
    /// Recommended branch name (`branch-name` tag).
    pub branch_name: Option<String>,
    /// Most recent common ancestor with the target branch (`merge-base` tag).
    pub merge_base: Option<String>,
    /// Root patch event this PR revises, which should then be closed
    /// (`e` tag) — optional.
    pub revision_of: Option<String>,
}

/// Build a git pull-request event (kind:1618, NIP-34). `content` is the
/// markdown PR description.
pub fn build_git_pull_request(
    repo: &GitRepoCoord,
    content: &str,
    meta: &GitPullRequestMeta,
) -> Result<EventBuilder, SdkError> {
    check_content(content, 64 * 1024)?;
    if meta.subject.is_empty() {
        return Err(SdkError::InvalidInput("subject must not be empty".into()));
    }
    if meta.subject.len() > 256 {
        return Err(SdkError::InvalidInput(format!(
            "subject exceeds 256 characters (got {})",
            meta.subject.len()
        )));
    }
    check_commit_hex(&meta.commit, "commit")?;
    if meta.clone_urls.is_empty() {
        return Err(SdkError::InvalidInput(
            "a pull request needs at least one --clone url where the tip commit can be fetched"
                .into(),
        ));
    }
    let a_value = repo.to_a_tag_value()?;
    let owner = check_pubkey_hex(&repo.owner, "repo owner")?;

    let mut tags = vec![tag(&["a", &a_value])?];
    if let Some(ref euc) = meta.euc {
        check_commit_hex(euc, "euc")?;
        tags.push(tag(&["r", euc])?);
    }
    tags.push(tag(&["p", &owner])?);
    for recipient in &meta.recipients {
        let pk = check_pubkey_hex(recipient, "recipient")?;
        tags.push(tag(&["p", &pk])?);
    }
    tags.push(tag(&["subject", &meta.subject])?);
    for label in &meta.labels {
        tags.push(tag(&["t", label])?);
    }
    tags.push(tag(&["c", &meta.commit])?);
    if let Some(ref channel_id) = meta.channel_id {
        let channel_id = Uuid::parse_str(channel_id)
            .map_err(|e| SdkError::InvalidInput(format!("channel_id must be a valid UUID: {e}")))?;
        tags.push(tag(&["h", &channel_id.to_string()])?);
    }
    let mut clone_tag = vec!["clone"];
    clone_tag.extend(meta.clone_urls.iter().map(String::as_str));
    tags.push(tag(&clone_tag)?);
    if let Some(ref branch) = meta.branch_name {
        tags.push(tag(&["branch-name", branch])?);
    }
    if let Some(ref base) = meta.merge_base {
        check_commit_hex(base, "merge_base")?;
        tags.push(tag(&["merge-base", base])?);
    }
    if let Some(ref patch) = meta.revision_of {
        let patch_id = check_hex_exact(patch, 64, "revision_of")?;
        tags.push(tag(&["e", &patch_id])?);
    }

    Ok(EventBuilder::new(Kind::Custom(KIND_GIT_PULL_REQUEST as u16), content).tags(tags))
}

/// Metadata for a git pull-request update event (kind:1619, NIP-34). A PR
/// update changes the tip commit of an existing PR; it references the PR via
/// NIP-22 uppercase root tags (`E`/`P`).
#[derive(Default)]
pub struct GitPrUpdateMeta {
    /// Earliest-unique-commit of the repo (`r` tag).
    pub euc: Option<String>,
    /// Additional pubkeys to `p`-tag besides the repo owner.
    pub recipients: Vec<String>,
    /// The pull-request event being updated (`E` tag, NIP-22 root) — required.
    pub pr_event: String,
    /// The pull-request author (`P` tag, NIP-22 root) — required.
    pub pr_author: String,
    /// Updated tip commit id (`c` tag) — required.
    pub commit: String,
    /// Clone URL(s) where the new tip can be fetched (`clone` tag) — at least one.
    pub clone_urls: Vec<String>,
    /// Most recent common ancestor with the target branch (`merge-base` tag).
    pub merge_base: Option<String>,
}

/// Build a git pull-request update event (kind:1619, NIP-34). `content` is
/// optional markdown context for the update.
pub fn build_git_pr_update(
    repo: &GitRepoCoord,
    content: &str,
    meta: &GitPrUpdateMeta,
) -> Result<EventBuilder, SdkError> {
    check_content(content, 64 * 1024)?;
    let pr_event = check_hex_exact(&meta.pr_event, 64, "pr_event")?;
    let pr_author = check_pubkey_hex(&meta.pr_author, "pr_author")?;
    check_commit_hex(&meta.commit, "commit")?;
    if meta.clone_urls.is_empty() {
        return Err(SdkError::InvalidInput(
            "a pull request update needs at least one --clone url where the tip commit can be fetched"
                .into(),
        ));
    }
    let a_value = repo.to_a_tag_value()?;
    let owner = check_pubkey_hex(&repo.owner, "repo owner")?;

    let mut tags = vec![tag(&["a", &a_value])?];
    if let Some(ref euc) = meta.euc {
        check_commit_hex(euc, "euc")?;
        tags.push(tag(&["r", euc])?);
    }
    tags.push(tag(&["p", &owner])?);
    for recipient in &meta.recipients {
        let pk = check_pubkey_hex(recipient, "recipient")?;
        tags.push(tag(&["p", &pk])?);
    }
    tags.push(tag(&["E", &pr_event])?);
    tags.push(tag(&["P", &pr_author])?);
    tags.push(tag(&["c", &meta.commit])?);
    let mut clone_tag = vec!["clone"];
    clone_tag.extend(meta.clone_urls.iter().map(String::as_str));
    tags.push(tag(&clone_tag)?);
    if let Some(ref base) = meta.merge_base {
        check_commit_hex(base, "merge_base")?;
        tags.push(tag(&["merge-base", base])?);
    }

    Ok(EventBuilder::new(Kind::Custom(KIND_GIT_PR_UPDATE as u16), content).tags(tags))
}

/// Build a workflow definition event (kind 30620).
///
/// - `channel_id`: the channel this workflow belongs to (h-tag)
/// - `workflow_id`: unique workflow UUID (d-tag)
/// - `yaml`: workflow YAML definition as content
pub fn build_workflow_def(
    channel_id: Uuid,
    workflow_id: Uuid,
    yaml: &str,
) -> Result<EventBuilder, SdkError> {
    check_content(yaml, 64 * 1024)?;
    let tags = vec![
        tag(&["d", &workflow_id.to_string()])?,
        tag(&["h", &channel_id.to_string()])?,
    ];
    Ok(EventBuilder::new(Kind::Custom(KIND_WORKFLOW_DEF as u16), yaml).tags(tags))
}

/// Build a workflow update event (kind 30620) for an existing workflow.
///
/// Updates an existing workflow definition in-place via the parameterized
/// replaceable event mechanism — same d-tag overwrites the previous version.
/// The h-tag (channel scope) is required by the relay for authorization.
pub fn build_workflow_update(
    channel_id: Uuid,
    workflow_id: Uuid,
    yaml: &str,
    expected_revision: &str,
) -> Result<EventBuilder, SdkError> {
    check_content(yaml, 64 * 1024)?;
    let tags = vec![
        tag(&["d", &workflow_id.to_string()])?,
        tag(&["h", &channel_id.to_string()])?,
        tag(&["expected-revision", expected_revision])?,
    ];
    Ok(EventBuilder::new(Kind::Custom(KIND_WORKFLOW_DEF as u16), yaml).tags(tags))
}

/// Build a NIP-09 deletion event targeting a workflow definition (kind 5).
///
/// The `a`-tag addresses the parameterized replaceable event
/// `<KIND_WORKFLOW_DEF>:<pubkey>:<workflow_id>`.
pub fn build_workflow_delete(
    author_pubkey: &str,
    workflow_id: Uuid,
) -> Result<EventBuilder, SdkError> {
    build_delete_addressable(KIND_WORKFLOW_DEF, author_pubkey, &workflow_id.to_string())
}

/// Build a workflow trigger event (kind 46020).
pub fn build_workflow_trigger(workflow_id: Uuid) -> Result<EventBuilder, SdkError> {
    let tags = vec![tag(&["d", &workflow_id.to_string()])?];
    Ok(EventBuilder::new(Kind::Custom(KIND_WORKFLOW_TRIGGER as u16), "").tags(tags))
}

/// Build a workflow approval event — kind 46030 (grant) or 46031 (deny).
///
/// - `token_hash`: hex-encoded SHA-256 of the approval token UUID (d-tag).
///   Must be exactly 64 hex characters.
/// - `approved`: `true` emits kind 46030 (grant), `false` emits kind 46031 (deny)
/// - `note`: optional human-readable note as event content
pub fn build_workflow_approval(
    token_hash: &str,
    approved: bool,
    note: &str,
) -> Result<EventBuilder, SdkError> {
    if token_hash.len() != 64 || !token_hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(SdkError::InvalidInput(
            "token_hash must be a 64-character hex SHA-256 digest".into(),
        ));
    }
    let kind = if approved {
        KIND_APPROVAL_GRANT
    } else {
        KIND_APPROVAL_DENY
    };
    let tags = vec![tag(&["d", token_hash])?];
    Ok(EventBuilder::new(Kind::Custom(kind as u16), note).tags(tags))
}

/// Build a DM open event (kind 41010).
///
/// `pubkeys` must be 1–8 hex-encoded pubkeys to include in the DM conversation.
pub fn build_dm_open(pubkeys: &[&str]) -> Result<EventBuilder, SdkError> {
    if pubkeys.is_empty() || pubkeys.len() > 8 {
        return Err(SdkError::InvalidInput(
            "dm open requires 1-8 pubkeys".into(),
        ));
    }
    let mut tags = Vec::with_capacity(pubkeys.len());
    for pk in pubkeys {
        let validated = check_pubkey_hex(pk, "pubkey")?;
        tags.push(tag(&["p", &validated])?);
    }
    Ok(EventBuilder::new(Kind::Custom(KIND_DM_OPEN as u16), "").tags(tags))
}

/// Build a DM add-member event (kind 41011).
pub fn build_dm_add_member(channel_id: Uuid, pubkey: &str) -> Result<EventBuilder, SdkError> {
    let pk = check_pubkey_hex(pubkey, "pubkey")?;
    let tags = vec![tag(&["h", &channel_id.to_string()])?, tag(&["p", &pk])?];
    Ok(EventBuilder::new(Kind::Custom(KIND_DM_ADD_MEMBER as u16), "").tags(tags))
}

/// Build a presence update event (kind 20001).
///
/// `status` must be one of: `"online"`, `"away"`, `"offline"`.
/// The status is placed in `event.content` (relay reads it there) and also
/// in a `["status", ...]` tag for structured access.
pub fn build_presence_update(status: &str) -> Result<EventBuilder, SdkError> {
    match status {
        "online" | "away" | "offline" => {}
        _ => {
            return Err(SdkError::InvalidInput(format!(
                "status must be online, away, or offline (got: {status})"
            )))
        }
    }
    let tags = vec![tag(&["status", status])?];
    Ok(EventBuilder::new(Kind::Custom(KIND_PRESENCE_UPDATE as u16), status).tags(tags))
}

/// Build a NIP-38 user status event (kind 30315) on the `d:general` coordinate.
///
/// `text` becomes the event content and `emoji`, when non-blank, an
/// `["emoji", ...]` tag; both are trimmed. Blank text with no emoji clears the
/// status — kind 30315 is parameterized-replaceable, so an event carrying
/// neither is what clients read as "no status".
pub fn build_user_status(text: &str, emoji: Option<&str>) -> Result<EventBuilder, SdkError> {
    let text = text.trim();
    check_content(text, 64 * 1024)?;
    let mut tags = vec![tag(&["d", "general"])?];
    if let Some(emoji) = emoji.map(str::trim).filter(|e| !e.is_empty()) {
        tags.push(tag(&["emoji", emoji])?);
    }
    Ok(EventBuilder::new(Kind::Custom(KIND_USER_STATUS as u16), text).tags(tags))
}

// ---------------------------------------------------------------------------
// Community moderation commands (kinds 9040–9044).
//
// These mirror the NIP-43 relay-admin 9030-series: mod-signed command events
// that the relay validates + executes directly and never stores. The tenant
// (community) is bound by the connection host, so no `h` tag is carried — a
// stray `h` would be rejected as channel-scoping a global-only command. The
// tag vocabulary below is pinned by `moderation_commands.rs` (relay and CLI
// must agree).
// ---------------------------------------------------------------------------

/// Build a community ban command (kind 9040).
///
/// `expires_at`: `None` ⇒ permanent; `Some(unix_secs)` ⇒ ban lifts at that time.
pub fn build_moderation_ban(
    target_pubkey: &str,
    expires_at: Option<u64>,
    reason: Option<&str>,
) -> Result<EventBuilder, SdkError> {
    let target_pubkey = check_pubkey_hex(target_pubkey, "target_pubkey")?;
    let mut tags = vec![tag(&["p", &target_pubkey])?];
    if let Some(exp) = expires_at {
        tags.push(tag(&["expiration", &exp.to_string()])?);
    }
    if let Some(r) = reason {
        tags.push(tag(&["reason", r])?);
    }
    Ok(EventBuilder::new(Kind::Custom(KIND_MODERATION_BAN as u16), "").tags(tags))
}

/// Build a community unban command (kind 9041).
pub fn build_moderation_unban(target_pubkey: &str) -> Result<EventBuilder, SdkError> {
    let target_pubkey = check_pubkey_hex(target_pubkey, "target_pubkey")?;
    let tags = vec![tag(&["p", &target_pubkey])?];
    Ok(EventBuilder::new(Kind::Custom(KIND_MODERATION_UNBAN as u16), "").tags(tags))
}

/// Build a community timeout (write-block) command (kind 9042).
///
/// `expires_at` (required) is the unix-seconds timestamp the timeout lifts at.
pub fn build_moderation_timeout(
    target_pubkey: &str,
    expires_at: u64,
    reason: Option<&str>,
) -> Result<EventBuilder, SdkError> {
    let target_pubkey = check_pubkey_hex(target_pubkey, "target_pubkey")?;
    let mut tags = vec![
        tag(&["p", &target_pubkey])?,
        tag(&["expiration", &expires_at.to_string()])?,
    ];
    if let Some(r) = reason {
        tags.push(tag(&["reason", r])?);
    }
    Ok(EventBuilder::new(Kind::Custom(KIND_MODERATION_TIMEOUT as u16), "").tags(tags))
}

/// Build a community untimeout command (kind 9043).
pub fn build_moderation_untimeout(target_pubkey: &str) -> Result<EventBuilder, SdkError> {
    let target_pubkey = check_pubkey_hex(target_pubkey, "target_pubkey")?;
    let tags = vec![tag(&["p", &target_pubkey])?];
    Ok(EventBuilder::new(Kind::Custom(KIND_MODERATION_UNTIMEOUT as u16), "").tags(tags))
}

/// Build a resolve-report command (kind 9044).
///
/// `report_event_id`: hex event id of the kind:1984 report being resolved.
/// `status`: `resolved` | `dismissed`. `action`: `delete` | `kick` | `ban` |
/// `timeout` | `dismiss` | `escalate` (`dismiss` pairs with `dismissed`,
/// everything else with `resolved` — the relay enforces the pairing).
/// `reason`: optional; audited into `public_reason` and relayed in the
/// reporter notice DM, so it must be safe for the reporter to read.
pub fn build_moderation_resolve_report(
    report_event_id: &str,
    status: &str,
    action: &str,
    reason: Option<&str>,
) -> Result<EventBuilder, SdkError> {
    let report_event_id = check_hex_exact(report_event_id, 64, "report_event_id")?;
    match status {
        "resolved" | "dismissed" => {}
        _ => {
            return Err(SdkError::InvalidInput(format!(
                "status must be resolved or dismissed (got: {status})"
            )))
        }
    }
    match action {
        "delete" | "kick" | "ban" | "timeout" | "dismiss" | "escalate" => {}
        _ => {
            return Err(SdkError::InvalidInput(format!(
                "action must be delete, kick, ban, timeout, dismiss, or escalate (got: {action})"
            )))
        }
    }
    let mut tags = vec![
        tag(&["report", &report_event_id])?,
        tag(&["status", status])?,
        tag(&["action", action])?,
    ];
    if let Some(r) = reason {
        tags.push(tag(&["reason", r])?);
    }
    Ok(EventBuilder::new(Kind::Custom(KIND_MODERATION_RESOLVE_REPORT as u16), "").tags(tags))
}

// ---------------------------------------------------------------------------
// NIP-IA identity archival (kinds 9035/9036).
//
// kind:9035 archive request, kind:9036 unarchive request. Both protected by
// NIP-70 (`["-"]`), p-tag the target, and may carry an optional machine-
// readable `reason` code, a `replaced-by` rotation pointer (9035 only), and a
// NIP-OA `auth` tag for owner-of-agent requests. The relay verifies; this
// builder's job is to produce a well-formed, signed request — the relay
// selects the consent path (self / admin / owner). Mirrors the desktop's
// `identity_archive_tags`/`build_archive_identity_request` (events.rs:624-743)
// so both clients emit the same wire form.
// ---------------------------------------------------------------------------

/// Maximum `reason` length in UTF-8 bytes (not chars — see
/// `desktop/src-tauri/src/events.rs:635-647`, whose `.len()` check is
/// already byte-based despite its error text saying "chars").
const MAX_REASON_BYTES: usize = 64;

fn check_reason(reason: &str) -> Result<(), SdkError> {
    if reason.len() > MAX_REASON_BYTES {
        return Err(SdkError::InvalidInput(format!(
            "reason code exceeds maximum length of {MAX_REASON_BYTES} UTF-8 bytes (got {})",
            reason.len()
        )));
    }
    if reason.chars().any(|c| c.is_control()) {
        return Err(SdkError::InvalidInput(
            "reason code must not contain control characters".into(),
        ));
    }
    Ok(())
}

/// Structural check only — the relay performs full NIP-OA verification.
/// Requires the `auth` label, a 64-hex owner pubkey, and a 128-hex signature.
fn check_auth_tag_shape(auth: &[String; 4]) -> Result<(), SdkError> {
    if auth[0] != "auth" {
        return Err(SdkError::InvalidInput(format!(
            "auth tag label must be \"auth\" (got \"{}\")",
            auth[0]
        )));
    }
    check_pubkey_hex(&auth[1], "auth tag owner pubkey")?;
    if auth[3].len() != 128 || !auth[3].chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(SdkError::InvalidInput(
            "auth tag signature must be 128-character hex".into(),
        ));
    }
    Ok(())
}

fn identity_archive_tags(
    target_pubkey: &str,
    reason: Option<&str>,
    replaced_by: Option<&str>,
    auth: Option<&[String; 4]>,
) -> Result<Vec<Tag>, SdkError> {
    let target_lower = check_pubkey_hex(target_pubkey, "target_pubkey")?;

    // NIP-70: mark as protected administrative state.
    let mut tags = vec![tag(&["-"])?, tag(&["p", &target_lower])?];

    if let Some(r) = reason {
        check_reason(r)?;
        tags.push(tag(&["reason", r])?);
    }

    if let Some(rb) = replaced_by {
        let rb_lower = check_pubkey_hex(rb, "replaced_by")?;
        if rb_lower == target_lower {
            return Err(SdkError::InvalidInput(
                "replaced-by must differ from the target".into(),
            ));
        }
        tags.push(tag(&["replaced-by", &rb_lower])?);
    }

    if let Some(auth_tag) = auth {
        check_auth_tag_shape(auth_tag)?;
        tags.push(tag(&["auth", &auth_tag[1], &auth_tag[2], &auth_tag[3]])?);
    }

    Ok(tags)
}

/// Build a NIP-IA archive request (kind 9035).
///
/// `content` is an optional human-readable reason (clients MUST NOT parse
/// authorization semantics from it; capped at 64 KiB like other content
/// fields). `reason` is a machine-readable code (`rotated`, `retired`,
/// `bot-rebuilt`, `left-organization`, `spam`, ... — unknown codes are
/// allowed per spec), capped at 64 UTF-8 bytes. `replaced_by` is the
/// rotation pointer and must differ from `target_pubkey`. `auth` is a
/// NIP-OA owner-attestation tag required only for the owner-of-agent
/// consent path; full verification happens relay-side.
///
/// `.allow_self_tagging()` is required: NIP-IA's self path has
/// `actor == target`, so the request's `["p", target]` matches the signer.
/// nostr 0.44 strips matching `p` tags by default — this keeps the wire
/// form intact.
pub fn build_archive_identity_request(
    target_pubkey: &str,
    content: &str,
    reason: Option<&str>,
    replaced_by: Option<&str>,
    auth: Option<&[String; 4]>,
) -> Result<EventBuilder, SdkError> {
    check_content(content, 64 * 1024)?;
    let tags = identity_archive_tags(target_pubkey, reason, replaced_by, auth)?;
    Ok(
        EventBuilder::new(Kind::Custom(KIND_IA_ARCHIVE_REQUEST as u16), content)
            .tags(tags)
            .allow_self_tagging(),
    )
}

/// Build a NIP-IA unarchive request (kind 9036).
///
/// Same shape as [`build_archive_identity_request`] minus `replaced-by`
/// (which has no defined meaning on unarchive per spec). `auth` is used for
/// owner-of-agent unarchive paths. See that function for the rationale on
/// `.allow_self_tagging()`.
pub fn build_unarchive_identity_request(
    target_pubkey: &str,
    content: &str,
    reason: Option<&str>,
    auth: Option<&[String; 4]>,
) -> Result<EventBuilder, SdkError> {
    check_content(content, 64 * 1024)?;
    let tags = identity_archive_tags(target_pubkey, reason, None, auth)?;
    Ok(
        EventBuilder::new(Kind::Custom(KIND_IA_UNARCHIVE_REQUEST as u16), content)
            .tags(tags)
            .allow_self_tagging(),
    )
}

// ─── NIP-MP: Multi-repo projects (kind:30621) ────────────────────────────────
//
//  Public surface:
//  • `validate_project_envelope` — Layer A protocol validator (12 ingest rules)
//  • `build_project_with_tags`   — Layer A raw builder (content + tags, no canonicalization)
//  • `ProjectMemberCoord`        — parsed member coordinate + optional relay hint
//  • `build_project`             — Layer B writer-policy builder
//  • `build_delete_addressable`  — generic NIP-09 kind:5 coordinate delete
//
//  Byte-length bounds from NIP-MP §Relay Processing:
/// Maximum byte length of a project `d` tag value.
pub const PROJECT_D_MAX_LEN: usize = 1024;
/// Maximum byte length of a project `name` tag value.
pub const PROJECT_NAME_MAX: usize = 256;
/// Maximum byte length of a project `description` tag value.
pub const PROJECT_DESCRIPTION_MAX: usize = 2048;
/// Maximum byte length of a project `buzz-channel` tag value.
pub const PROJECT_CHANNEL_MAX: usize = 256;
/// Maximum byte length of a project `buzz-visibility` tag value.
pub const PROJECT_VISIBILITY_MAX: usize = 256;
/// Maximum number of `a` member tags per project event (checked before dedup).
pub const PROJECT_MEMBER_CAP: usize = 64;
/// Maximum byte length of a project `buzz-access` tag value.
pub const PROJECT_ACCESS_MAX: usize = 256;
/// Maximum number of invited-member `p` tags per project event (checked
/// before per-tag work, same rationale as [`PROJECT_MEMBER_CAP`]).
pub const PROJECT_INVITE_CAP: usize = 256;

/// A validated NIP-MP member `a`-tag coordinate with an optional relay hint.
///
/// Equality and `Hash` are by `coord` only (per spec: duplicate detection ignores hint).
#[derive(Clone, Debug)]
pub struct ProjectMemberCoord {
    /// The full `30617:<owner-hex>:<repo-d>` coordinate string.
    pub coord: String,
    /// Optional opaque relay hint (third `a`-tag element, never validated by content).
    pub hint: Option<String>,
}

impl PartialEq for ProjectMemberCoord {
    fn eq(&self, other: &Self) -> bool {
        self.coord == other.coord
    }
}

impl Eq for ProjectMemberCoord {}

impl std::hash::Hash for ProjectMemberCoord {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.coord.hash(state);
    }
}

impl ProjectMemberCoord {
    /// Parse a full `30617:<owner-hex>:<repo-d>` coordinate string.
    ///
    /// Accepts an optional relay hint as the third colon-separated element
    /// after the split, but the split is always first-two-colons: kind, owner,
    /// everything-else-as-repo-d.
    ///
    /// Rules enforced:
    /// - Exactly three segments after splitting on the first two colons
    /// - First segment must be the literal string `"30617"`
    /// - Second segment must be exactly 64 lowercase hex characters
    /// - Third segment (repo-d) must be non-empty
    /// - Uppercase owners are rejected (never normalized)
    pub fn parse_full(coord: &str) -> Result<Self, SdkError> {
        // Split on first two colons only: kind:owner:rest
        let mut parts = coord.splitn(3, ':');
        let kind_part = parts.next().unwrap_or("");
        let owner_part = parts.next().unwrap_or("");
        let rest = parts.next().unwrap_or("");

        if kind_part != "30617" {
            return Err(SdkError::InvalidInput(format!(
                "member coordinate must start with '30617:' (got kind {kind_part:?})"
            )));
        }
        if owner_part.len() != 64 || !owner_part.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(SdkError::InvalidInput(format!(
                "member owner must be a 64-character hex pubkey (got {owner_part:?})"
            )));
        }
        // Reject uppercase (spec: lowercase hex required)
        if owner_part.chars().any(|c| c.is_ascii_uppercase()) {
            return Err(SdkError::InvalidInput(
                "member owner hex must be lowercase".into(),
            ));
        }
        if rest.is_empty() {
            return Err(SdkError::InvalidInput(
                "member coordinate repo-d must not be empty".into(),
            ));
        }
        Ok(ProjectMemberCoord {
            coord: format!("30617:{owner_part}:{rest}"),
            hint: None,
        })
    }

    /// Returns the `a`-tag element slice: `[coord]` or `[coord, hint]`.
    pub fn to_tag_parts(&self) -> Vec<String> {
        let mut parts = vec!["a".to_string(), self.coord.clone()];
        if let Some(h) = &self.hint {
            parts.push(h.clone());
        }
        parts
    }
}

/// **Layer A**: Validate a complete kind:30621 envelope against the 12 NIP-MP
/// ingest rules (including the Buzz access extension).  This is the single
/// source of protocol truth used by both
/// `build_project_with_tags` (raw path) and `build_project` (policy path).
///
/// Rules enforced (matches relay `buzz-db` ingest logic):
/// 1. `d` cardinality: exactly one `d` tag.
/// 2. `d` value: non-empty, ≤1024 bytes.
/// 3. Member cap: raw count of every `a` tag ≤ 64 (checked **before** per-tag
///    parsing, matching relay rule order).
/// 4. Member tag arity: every `a` tag has 2 or 3 elements (no more, no fewer).
/// 5. Member coordinate grammar: first-two-colons split; kind literal `"30617"`;
///    owner lowercase 64-hex; repo-d non-empty verbatim.
/// 6. Member deduplication: coordinate equality only (hint ignored); any
///    coordinate that appears more than once is a duplicate.
/// 7. Singleton metadata: each of `name`, `description`, `buzz-channel`,
///    `buzz-visibility`, `buzz-access` appears at most once.
/// 8. Metadata byte lengths: `name` ≤256, `description` ≤2048,
///    `buzz-channel` ≤256, `buzz-visibility` ≤256, `buzz-access` ≤256.
/// 9. Access value: if present, `buzz-access` must be `"private"` or
///    `"public"` — an unrecognized value is rejected rather than silently
///    falling open to public (fail closed). The community's shared default
///    project (`d` = `"general"`) can never be private
///    (rule `access-general-forced-public`).
/// 10. Invite cap: raw count of every `p` tag ≤256 (checked before per-tag
///     parsing, matching relay rule order).
/// 11. Invite tag arity: every `p` tag has 2 to 4 elements — pubkey, optional
///     relay hint, optional role (Buzz roles extension, mirroring the NIP-29
///     39002 grammar). A present 4th element must be a pinned
///     [`buzz_core::kind::PROJECT_ROLES`] value (rule `invite-role`); a
///     role-less invite is a legacy collaborator.
/// 12. Invite grammar + deduplication: each `p` value is a lowercase 64-hex
///     pubkey; any pubkey that appears more than once is a duplicate.
pub fn validate_project_envelope(tags: &[Tag], _content: &str) -> Result<(), SdkError> {
    // --- Rule 1 & 2: d tag ---
    let d_tags: Vec<&Tag> = tags.iter().filter(|t| tag_name(t) == Some("d")).collect();
    match d_tags.len() {
        0 => {
            return Err(SdkError::InvalidInput(
                "project must have exactly one 'd' tag (rule: d-cardinality)".into(),
            ))
        }
        1 => {}
        _ => {
            return Err(SdkError::InvalidInput(
                "project must have exactly one 'd' tag (rule: d-cardinality)".into(),
            ))
        }
    }
    let d_val = tag_value(d_tags[0]).unwrap_or("");
    if d_val.is_empty() {
        return Err(SdkError::InvalidInput(
            "project 'd' tag must not be empty (rule: d-empty)".into(),
        ));
    }
    if d_val.len() > PROJECT_D_MAX_LEN {
        return Err(SdkError::InvalidInput(format!(
            "project 'd' tag exceeds {PROJECT_D_MAX_LEN} bytes (rule: d-empty)"
        )));
    }

    let a_tags: Vec<&Tag> = tags.iter().filter(|t| tag_name(t) == Some("a")).collect();

    // --- Rule 3: member cap (checked before per-tag parsing, matching relay rule order) ---
    if a_tags.len() > PROJECT_MEMBER_CAP {
        return Err(SdkError::InvalidInput(format!(
            "project exceeds member cap of {PROJECT_MEMBER_CAP} (got {}) (rule: member-cap)",
            a_tags.len()
        )));
    }

    // --- Rule 4: member arity ---
    for a in &a_tags {
        let len = a.as_slice().len() - 1; // exclude the "a" name element
        if !(1..=2).contains(&len) {
            return Err(SdkError::InvalidInput(format!(
                "member 'a' tag must have 1 or 2 value elements (got {len}) (rule: member-tag-arity)"
            )));
        }
    }

    // --- Rules 5 & 6: coordinate grammar + deduplication ---
    let mut seen_coords: std::collections::HashSet<String> = std::collections::HashSet::new();
    for a in &a_tags {
        let coord_val = tag_value(a).unwrap_or("");
        ProjectMemberCoord::parse_full(coord_val).map_err(|e| {
            SdkError::InvalidInput(format!("{e} (rule: member-coordinate-malformed)"))
        })?;
        if !seen_coords.insert(coord_val.to_string()) {
            return Err(SdkError::InvalidInput(format!(
                "duplicate member coordinate {coord_val:?} (rule: member-duplicate)"
            )));
        }
    }

    // --- Rules 7 & 8: singleton metadata + byte bounds ---
    let singleton_fields = [
        (
            "name",
            PROJECT_NAME_MAX,
            "metadata-cardinality",
            "metadata-length",
        ),
        (
            "description",
            PROJECT_DESCRIPTION_MAX,
            "metadata-cardinality",
            "metadata-length",
        ),
        (
            "buzz-channel",
            PROJECT_CHANNEL_MAX,
            "metadata-cardinality",
            "metadata-length",
        ),
        (
            "buzz-visibility",
            PROJECT_VISIBILITY_MAX,
            "metadata-cardinality",
            "metadata-length",
        ),
        (
            buzz_core::kind::PROJECT_ACCESS_TAG,
            PROJECT_ACCESS_MAX,
            "metadata-cardinality",
            "metadata-length",
        ),
    ];
    let mut buzz_access: Option<&str> = None;
    for (field, max_bytes, card_rule, len_rule) in singleton_fields {
        let matches: Vec<&Tag> = tags.iter().filter(|t| tag_name(t) == Some(field)).collect();
        if matches.len() > 1 {
            return Err(SdkError::InvalidInput(format!(
                "project must have at most one '{field}' tag (rule: {card_rule})"
            )));
        }
        if let Some(t) = matches.first() {
            let val = tag_value(t).unwrap_or("");
            if val.len() > max_bytes {
                return Err(SdkError::InvalidInput(format!(
                    "'{field}' tag exceeds {max_bytes} bytes (rule: {len_rule})"
                )));
            }
            if field == buzz_core::kind::PROJECT_ACCESS_TAG {
                buzz_access = Some(val);
            }
        }
    }

    // --- Rule 9: access value (Buzz access extension) ---
    // `buzz-access` is an access-control input, not a display hint — an
    // unrecognized value must be rejected rather than silently falling open
    // to public (fail closed), matching relay ingest.
    if let Some(access) = buzz_access {
        if access != buzz_core::kind::PROJECT_ACCESS_PRIVATE
            && access != buzz_core::kind::PROJECT_ACCESS_PUBLIC
        {
            return Err(SdkError::InvalidInput(format!(
                "project 'buzz-access' tag must be \"private\" or \"public\" (got {access:?}) (rule: access-value)"
            )));
        }
        // The community's shared default project can never be private,
        // mirroring relay ingest (`access-general-forced-public`).
        if access == buzz_core::kind::PROJECT_ACCESS_PRIVATE
            && d_val == buzz_core::kind::GENERAL_PROJECT_DTAG
        {
            return Err(SdkError::InvalidInput(
                "the \"general\" project is the community's shared default and cannot be private \
                 (rule: access-general-forced-public)"
                    .into(),
            ));
        }
    }

    // --- Rules 10, 11, 12: invited-member `p` tags (Buzz access extension) ---
    let p_tags: Vec<&Tag> = tags.iter().filter(|t| tag_name(t) == Some("p")).collect();

    // Rule 10: invite cap (checked before per-tag work, matching member-cap order).
    if p_tags.len() > PROJECT_INVITE_CAP {
        return Err(SdkError::InvalidInput(format!(
            "project exceeds invite cap of {PROJECT_INVITE_CAP} (got {}) (rule: invite-cap)",
            p_tags.len()
        )));
    }

    // Rule 11: invite tag arity — `["p", pubkey]` plus NIP-01's optional relay
    // hint, plus an optional 4th role element (Buzz roles extension). A
    // present role must be from the pinned vocabulary — a role typo must not
    // silently grant or deny (mirrors relay ingest `invite-role`).
    for p in &p_tags {
        let parts = p.as_slice();
        let len = parts.len();
        if !(2..=4).contains(&len) {
            return Err(SdkError::InvalidInput(format!(
                "invited-member 'p' tag must have 2 to 4 elements (got {len}) (rule: invite-tag-arity)"
            )));
        }
        if let Some(role) = parts.get(3) {
            if !buzz_core::kind::is_valid_project_role(role.as_str()) {
                return Err(SdkError::InvalidInput(format!(
                    "invited-member role must be one of {:?} (got {role:?}) (rule: invite-role)",
                    buzz_core::kind::PROJECT_ROLES
                )));
            }
        }
    }

    // Rule 12: invite grammar + deduplication — lowercase 64-hex pubkeys only,
    // same byte-exact-matching reason as member coordinates.
    let mut seen_invites: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for p in &p_tags {
        let invite = tag_value(p).unwrap_or("");
        if invite.len() != 64
            || !invite
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err(SdkError::InvalidInput(format!(
                "invited-member 'p' tag must be a lowercase 64-hex pubkey (got {invite:?}) (rule: invite-malformed)"
            )));
        }
        if !seen_invites.insert(invite) {
            return Err(SdkError::InvalidInput(format!(
                "duplicate invited-member 'p' tag {invite:?} (rule: invite-duplicate)"
            )));
        }
    }

    Ok(())
}

/// Helper: tag name (first element).
fn tag_name(tag: &Tag) -> Option<&str> {
    tag.as_slice().first().map(String::as_str)
}

/// Helper: tag value (second element).
fn tag_value(tag: &Tag) -> Option<&str> {
    tag.as_slice().get(1).map(String::as_str)
}

/// **Layer A raw builder**: Build a kind:30621 project event from a raw
/// `content` string and a raw `tags` slice, without any canonicalization.
///
/// Validates the entire envelope through `validate_project_envelope` before
/// accepting it.  The caller is responsible for supplying the correct `d` tag.
/// This is the path exercised by fixture conformance tests and by read-modify-
/// write mutations in the CLI.
pub fn build_project_with_tags(content: &str, tags: Vec<Tag>) -> Result<EventBuilder, SdkError> {
    validate_project_envelope(&tags, content)?;
    Ok(EventBuilder::new(Kind::Custom(KIND_PROJECT as u16), content).tags(tags))
}

/// **Layer B writer-policy builder**: Build a kind:30621 project event with
/// enforced writer policy:
/// - The `d` tag is constructed from `slug`; `check_project_slug` rejects
///   an empty or over-length slug.
/// - `channel` must be a valid UUID string.
/// - `visibility` must be `"listed"` or `"unlisted"`.
/// - Content is always empty.
/// - Member coordinates are parsed through `ProjectMemberCoord::parse_full`.
///
/// The resulting envelope is validated through Layer A before the builder is
/// returned.
pub fn build_project(
    slug: &str,
    name: Option<&str>,
    description: Option<&str>,
    members: &[ProjectMemberCoord],
    channel: Option<&str>,
    visibility: Option<&str>,
) -> Result<EventBuilder, SdkError> {
    // Slug validation
    if slug.is_empty() {
        return Err(SdkError::InvalidInput(
            "project slug must not be empty".into(),
        ));
    }
    if slug.len() > PROJECT_D_MAX_LEN {
        return Err(SdkError::InvalidInput(format!(
            "project slug must not exceed {PROJECT_D_MAX_LEN} bytes (got {})",
            slug.len()
        )));
    }

    // Channel UUID validation
    if let Some(ch) = channel {
        uuid::Uuid::parse_str(ch).map_err(|_| {
            SdkError::InvalidInput(format!("buzz-channel must be a valid UUID (got {ch:?})"))
        })?;
    }

    // Visibility enum validation
    if let Some(vis) = visibility {
        if vis != "listed" && vis != "unlisted" {
            return Err(SdkError::InvalidInput(format!(
                "buzz-visibility must be 'listed' or 'unlisted' (got {vis:?})"
            )));
        }
    }

    let mut tags: Vec<Tag> = Vec::new();
    tags.push(tag(&["d", slug])?);

    if let Some(n) = name {
        tags.push(tag(&["name", n])?);
    }
    if let Some(d) = description {
        tags.push(tag(&["description", d])?);
    }
    for m in members {
        let tag_parts = m.to_tag_parts();
        let parts: Vec<&str> = tag_parts.iter().map(|s| s.as_str()).collect();
        // Safety: to_tag_parts always produces ["a", coord, ...hint]
        tags.push(
            Tag::parse(parts.iter().copied()).map_err(|e| SdkError::InvalidTag(e.to_string()))?,
        );
    }
    if let Some(ch) = channel {
        tags.push(tag(&["buzz-channel", ch])?);
    }
    if let Some(vis) = visibility {
        tags.push(tag(&["buzz-visibility", vis])?);
    }

    build_project_with_tags("", tags)
}

/// **Generic NIP-09 coordinate delete**: Build a kind:5 deletion event with
/// a single `a`-tag addressing `<kind>:<pubkey>:<d>`.
///
/// Validates:
/// - `kind` is an addressable kind (10000–19999 or 30000–39999).
/// - `pubkey` is a 64-character lowercase hex string.
/// - `d` is non-empty.
///
/// `build_workflow_delete` delegates to this function.
pub fn build_delete_addressable(
    kind: u32,
    pubkey: &str,
    d: &str,
) -> Result<EventBuilder, SdkError> {
    let is_addressable = (10000..20000).contains(&kind) || (30000..40000).contains(&kind);
    if !is_addressable {
        return Err(SdkError::InvalidInput(format!(
            "kind {kind} is not an addressable kind (must be 10000–19999 or 30000–39999)"
        )));
    }
    let pk = check_pubkey_hex(pubkey, "pubkey")?;
    if d.is_empty() {
        return Err(SdkError::InvalidInput("d must not be empty".into()));
    }
    let coord = format!("{kind}:{pk}:{d}");
    let tags = vec![tag(&["a", &coord])?];
    Ok(EventBuilder::new(Kind::Custom(KIND_DELETION as u16), "").tags(tags))
}

// ---- Project Pulse (kind 44240) --------------------------------------------

/// Build one Project Pulse entry (kind 44240).
///
/// `coordinate` is the NIP-MP project coordinate the entry is scoped to and
/// must already be canonical (`30621:<lowercase-hex>:<dtag>`); `channel` adds
/// the optional `h` tag; `session_ref` adds the optional `pu-session` tag and
/// is the umbrella **`sessionRef` UUID**, never a genesis event id — the same
/// value 44227/44229/44230 carry in their `d` tag, so a consumer's fold joins
/// on it with no lookup.
///
/// Tags are emitted in the canonical order `a`, `pu-v`, `pu-type`, `[h]`,
/// `[branch]`, `[pu-session]`. The `branch` tag is derived from the payload —
/// never supplied separately — so the tag and the content it addresses can
/// never disagree.
///
/// Every rule is [`validate_pulse_entry_envelope`]'s: this builder owns no
/// copy of the tag grammar, the canonical-coordinate rule, or the content
/// caps. The one rule it cannot decide is `supersedes != this event's own id`,
/// because the id does not exist until the caller signs; the relay re-runs the
/// same validator against the real id at ingest.
pub fn build_pulse_entry(
    coordinate: &str,
    entry: &PulseEntry,
    channel: Option<Uuid>,
    session_ref: Option<&str>,
) -> Result<EventBuilder, SdkError> {
    let content = serde_json::to_string(entry)
        .map_err(|error| SdkError::InvalidInput(format!("pulse entry serialization: {error}")))?;
    let mut tags = vec![
        tag(&["a", coordinate])?,
        tag(&["pu-v", PULSE_ENTRY_TAG_VERSION])?,
        tag(&["pu-type", entry.entry_type.as_str()])?,
    ];
    if let Some(channel) = channel {
        tags.push(tag(&["h", &channel.to_string()])?);
    }
    if let Some(branch) = entry.branch.as_deref() {
        tags.push(tag(&["branch", branch])?);
    }
    if let Some(session_ref) = session_ref {
        tags.push(tag(&["pu-session", session_ref])?);
    }
    validate_pulse_entry_envelope(&pulse_entry_probe(&tags, &content)?)
        .map_err(SdkError::InvalidInput)?;
    Ok(EventBuilder::new(Kind::Custom(KIND_PULSE_ENTRY as u16), content).tags(tags))
}

/// Assemble the unsigned tags and content into the shape buzz-core validates.
///
/// [`validate_pulse_entry_envelope`] reads a *signed* event, and a builder has
/// no id, author, or signature yet, so the probe carries placeholders for the
/// three. Only `event.id` is read by any rule (the self-supersession check),
/// and an all-zero id is not a value any real entry can carry.
fn pulse_entry_probe(tags: &[Tag], content: &str) -> Result<nostr::Event, SdkError> {
    // The only way to build the placeholder signature rejects a wrong length,
    // and this slice is exactly 64 bytes — the error arm is unreachable, and is
    // mapped rather than unwrapped.
    let signature = nostr::secp256k1::schnorr::Signature::from_slice(&[0u8; 64])
        .map_err(|error| SdkError::InvalidInput(format!("pulse entry probe: {error}")))?;
    Ok(nostr::Event::new(
        nostr::EventId::from_byte_array([0u8; 32]),
        nostr::PublicKey::from_byte_array([0u8; 32]),
        nostr::Timestamp::from_secs(0),
        Kind::Custom(KIND_PULSE_ENTRY as u16),
        tags.to_vec(),
        content,
        signature,
    ))
}

// ---- Coding sessions (NIP-CSC / NIP-CSL / NIP-CSPC / NIP-CST) --------------
//
// Six builders covering both halves of the contract: the two operator-authored
// commands a provider consumes, and the four provider-authored facts a consumer
// projects. Every tag list is ordered and derived from the payload — a tag and
// the content it addresses can never disagree, because the caller never supplies
// the tag.

/// Build a coding-session genesis event (kind 44226).
///
/// The **human operator** signs the returned builder — never a provider, never
/// an agent. That signature is the founder record for the umbrella session, and
/// nothing downstream can substitute for it, so a caller that signs this with a
/// service key has silently made that service the founder.
///
/// `payload` is either a fresh founding
/// ([`CodingSessionGenesisPayload::new`]) or an explicit legacy adoption
/// ([`CodingSessionGenesisPayload::new_adoption`]) — the envelope (three
/// tags) is identical either way; only the content differs. See the module
/// doc on [`buzz_core::coding_session_genesis`] for what the relay verifies
/// against an adoption reference.
///
/// The `csg-session` tag is re-derived from the payload here and re-derived
/// again by the relay, so the filterable reference and the signed reference are
/// the same string by construction.
pub fn build_coding_session_genesis(
    channel_id: Uuid,
    payload: &CodingSessionGenesisPayload,
) -> Result<EventBuilder, SdkError> {
    payload.validate().map_err(SdkError::InvalidInput)?;
    let content = serde_json::to_string(payload).map_err(|error| {
        SdkError::InvalidInput(format!("coding-session genesis serialization: {error}"))
    })?;
    check_content(&content, MAX_GENESIS_CONTENT_BYTES)?;
    let tags = vec![
        tag(&["h", &channel_id.to_string()])?,
        tag(&["csg-v", CODING_SESSION_GENESIS_TAG_VERSION])?,
        tag(&["csg-session", &payload.session_ref])?,
    ];
    Ok(EventBuilder::new(Kind::Custom(KIND_CODING_SESSION_GENESIS as u16), content).tags(tags))
}

/// Build one append-only coding-session goal revision (kind 44227).
///
/// The human operator signs this builder. `d=sessionRef` groups revisions but
/// does not replace them; consumers fold the regular events by `(created_at,
/// event id)` so every earlier goal remains queryable.
pub fn build_coding_session_goal(
    channel_id: Uuid,
    session_ref: &str,
    content: &str,
) -> Result<EventBuilder, SdkError> {
    validate_coding_session_goal_session_ref(session_ref).map_err(SdkError::InvalidInput)?;
    validate_coding_session_goal_content(content).map_err(SdkError::InvalidInput)?;
    let tags = vec![
        tag(&["h", &channel_id.to_string()])?,
        tag(&["d", session_ref])?,
        tag(&["csgl-v", CODING_SESSION_GOAL_TAG_VERSION])?,
    ];
    Ok(EventBuilder::new(
        Kind::Custom(KIND_CODING_SESSION_GOAL as u16),
        content.to_owned(),
    )
    .tags(tags))
}

/// Build one append-only coding-session name revision (kind 44229).
///
/// The human operator signs this builder. `d=sessionRef` groups revisions but
/// does not replace them; consumers fold the regular events by `(created_at,
/// event id)` so every earlier name remains queryable.
pub fn build_coding_session_name(
    channel_id: Uuid,
    session_ref: &str,
    content: &str,
) -> Result<EventBuilder, SdkError> {
    validate_coding_session_name_session_ref(session_ref).map_err(SdkError::InvalidInput)?;
    validate_coding_session_name_content(content).map_err(SdkError::InvalidInput)?;
    let tags = vec![
        tag(&["h", &channel_id.to_string()])?,
        tag(&["d", session_ref])?,
        tag(&["csnm-v", CODING_SESSION_NAME_TAG_VERSION])?,
    ];
    Ok(EventBuilder::new(
        Kind::Custom(KIND_CODING_SESSION_NAME as u16),
        content.to_owned(),
    )
    .tags(tags))
}

/// Build one append-only coding-session closure revision (kind 44230).
///
/// The caller signs the returned builder. The relay resolves the explicit
/// `genesisRef` before applying action-specific authority: only the founder may
/// close; a current project member may reopen a project session, while a
/// standalone session remains founder-only.
pub fn build_coding_session_closure(
    channel_id: Uuid,
    payload: &CodingSessionClosurePayload,
) -> Result<EventBuilder, SdkError> {
    payload.validate().map_err(SdkError::InvalidInput)?;
    let content = serde_json::to_string(payload).map_err(|error| {
        SdkError::InvalidInput(format!("coding-session closure serialization: {error}"))
    })?;
    check_content(&content, MAX_CODING_SESSION_CLOSURE_CONTENT_BYTES)?;
    let tags = vec![
        tag(&["h", &channel_id.to_string()])?,
        tag(&["d", &payload.session_ref])?,
        tag(&["cscl-v", CODING_SESSION_CLOSURE_TAG_VERSION])?,
        tag(&["cscl-genesis", &payload.genesis_ref])?,
    ];
    Ok(EventBuilder::new(Kind::Custom(KIND_CODING_SESSION_CLOSURE as u16), content).tags(tags))
}

/// Build a provider-neutral coding-session command (kind 44220).
///
/// The operator signs the returned builder. The relay validates the exact
/// payload and tags, including active channel membership, before storing it.
pub fn build_coding_session_command(
    channel_id: Uuid,
    payload: &CodingSessionCommandPayload,
) -> Result<EventBuilder, SdkError> {
    payload.validate().map_err(SdkError::InvalidInput)?;
    let content = serde_json::to_string(payload).map_err(|e| {
        SdkError::InvalidInput(format!("coding-session payload serialization: {e}"))
    })?;
    let target_key = coding_session_target_key(&payload.target);
    let tags = vec![
        tag(&["h", &channel_id.to_string()])?,
        tag(&["cs-v", CODING_SESSION_COMMAND_TAG_VERSION])?,
        tag(&["cs-target", &target_key])?,
    ];
    Ok(EventBuilder::new(Kind::Custom(KIND_CODING_SESSION_COMMAND as u16), content).tags(tags))
}

/// Build a provider-neutral coding-session lifecycle command (kind 44221).
///
/// The operator signs the returned builder. The relay validates the exact
/// payload, ordered tags, and active channel membership before storing it.
/// Provider adapters remain responsible for resolving references into
/// host-local execution state — notably the working directory, which is
/// machine-local and never appears in signed content.
///
/// The payload's `projectRef` may be `None` for a standalone session; when
/// present it must be a `30621:` project coordinate, which
/// [`CodingSessionLifecycleCommandPayload::validate`] enforces.
pub fn build_coding_session_lifecycle_command(
    channel_id: Uuid,
    payload: &CodingSessionLifecycleCommandPayload,
) -> Result<EventBuilder, SdkError> {
    payload.validate().map_err(SdkError::InvalidInput)?;
    let content = serde_json::to_string(payload).map_err(|error| {
        SdkError::InvalidInput(format!(
            "coding-session lifecycle payload serialization: {error}"
        ))
    })?;
    check_content(&content, MAX_LIFECYCLE_CONTENT_BYTES)?;
    let tags = vec![
        tag(&["h", &channel_id.to_string()])?,
        tag(&["csl-v", CODING_SESSION_LIFECYCLE_COMMAND_TAG_VERSION])?,
        tag(&["csl-command", &payload.command_id])?,
    ];
    Ok(EventBuilder::new(
        Kind::Custom(KIND_CODING_SESSION_LIFECYCLE_COMMAND as u16),
        content,
    )
    .tags(tags))
}

/// Build a coding-session provider catalog (kind 44222).
///
/// `content` is the exact canonical catalog JSON the provider will sign; it is
/// passed through byte-for-byte because `cspc-key` digests it, so any
/// re-serialization here would break the consumer's key check. `revision` is
/// the provider's own monotonic counter and must match the `revision` inside
/// `content` — the caller owns that agreement, since this builder does not
/// parse provider-authored content.
pub fn build_coding_session_provider_catalog(
    channel_id: Uuid,
    revision: u64,
    content: &str,
) -> Result<EventBuilder, SdkError> {
    if revision == 0 || revision > MAX_SAFE_GENERATION {
        return Err(SdkError::InvalidInput(
            "catalog revision must be a positive safe integer".into(),
        ));
    }
    check_content(content, MAX_PROVIDER_CATALOG_CONTENT_BYTES)?;
    let channel = channel_id.to_string();
    let key = coding_session_provider_catalog_semantic_key(&channel, revision, content);
    let tags = vec![
        tag(&["h", &channel])?,
        tag(&["cspc-v", CODING_SESSION_PROVIDER_CATALOG_TAG_VERSION])?,
        tag(&["cspc-revision", &revision.to_string()])?,
        tag(&["cspc-key", &key])?,
    ];
    Ok(EventBuilder::new(
        Kind::Custom(KIND_CODING_SESSION_PROVIDER_CATALOG as u16),
        content.to_owned(),
    )
    .tags(tags))
}

/// Build an ephemeral provider-signed lease for one exact session generation (kind 24223).
///
/// `command_id` is the accepted create/resume lifecycle command that minted
/// `payload.target`. The relay resolves that immutable command/receipt chain
/// and requires the event signer to equal its `providerAuthorityPubkey`; this
/// builder only makes the signed target, command, and sequence binding exact.
pub fn build_coding_session_lease(
    channel_id: Uuid,
    command_id: &str,
    payload: &CodingSessionLease,
) -> Result<EventBuilder, SdkError> {
    payload.validate().map_err(SdkError::InvalidInput)?;
    check_identifier(command_id, "csl-command")?;
    let content = serde_json::to_string(payload).map_err(|error| {
        SdkError::InvalidInput(format!("coding-session lease serialization: {error}"))
    })?;
    check_content(&content, MAX_CODING_SESSION_LEASE_CONTENT_BYTES)?;
    let tags = vec![
        tag(&["h", &channel_id.to_string()])?,
        tag(&["cslease-v", CODING_SESSION_LEASE_TAG_VERSION])?,
        tag(&["cs-target", &coding_session_target_key(&payload.target)])?,
        tag(&["csl-command", command_id])?,
        tag(&["cslease-seq", &payload.lease_sequence.to_string()])?,
    ];
    Ok(EventBuilder::new(Kind::Custom(KIND_CODING_SESSION_LEASE as u16), content).tags(tags))
}

/// Build coding-session metadata for one exact generation (kind 44223).
///
/// Metadata is append-only observation history within a generation. Providers
/// publish a new event on each observed transition; consumers fold the newest
/// valid observation by `(created_at, event id)` without rewriting old rows.
pub fn build_coding_session_metadata(
    channel_id: Uuid,
    target: &CodingSessionTarget,
    content: &str,
) -> Result<EventBuilder, SdkError> {
    check_target(target)?;
    check_content(content, MAX_METADATA_CONTENT_BYTES)?;
    let tags = vec![
        tag(&["h", &channel_id.to_string()])?,
        tag(&["csm-v", CODING_SESSION_METADATA_TAG_VERSION])?,
        tag(&["cs-target", &coding_session_target_key(target)])?,
        tag(&["csm-key", &coding_session_metadata_semantic_key(target)])?,
    ];
    Ok(EventBuilder::new(
        Kind::Custom(KIND_CODING_SESSION_METADATA as u16),
        content.to_owned(),
    )
    .tags(tags))
}

/// Build a coding-session lifecycle receipt (kind 44224).
///
/// One lifecycle command has exactly one outcome, so `command_id` alone keys
/// the receipt: a second receipt for the same command is a duplicate to drop,
/// never a revision to apply.
pub fn build_coding_session_lifecycle_receipt(
    channel_id: Uuid,
    command_id: &str,
    content: &str,
) -> Result<EventBuilder, SdkError> {
    check_identifier(command_id, "commandId")?;
    check_content(content, MAX_LIFECYCLE_RECEIPT_CONTENT_BYTES)?;
    let tags = vec![
        tag(&["h", &channel_id.to_string()])?,
        tag(&["cslr-v", CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION])?,
        tag(&["csl-command", command_id])?,
        tag(&[
            "csl-key",
            &coding_session_lifecycle_receipt_semantic_key(command_id),
        ])?,
    ];
    Ok(EventBuilder::new(
        Kind::Custom(KIND_CODING_SESSION_LIFECYCLE_RECEIPT as u16),
        content.to_owned(),
    )
    .tags(tags))
}

/// The `csl-key` tag value for one *stage* receipt of a turn command (44220).
///
/// A lifecycle command has one outcome, so
/// [`coding_session_lifecycle_receipt_semantic_key`] keys its receipt by
/// `commandId` alone. A turn command has several stages — queued, then
/// started; or a single drop or refusal — so its receipts key by
/// `(commandId, status)`. Keying them by `commandId` alone would make the
/// second stage a duplicate of the first and fence it out of the outbox, and
/// the operator would never learn that the turn they watched queue actually
/// began.
pub fn coding_session_turn_receipt_semantic_key(command_id: &str, status: ReceiptStatus) -> String {
    encode_structured_key(
        "coding-session-lifecycle-receipt/v1",
        &[command_id, status.as_str()],
    )
}

/// Build one stage receipt for a coding-session turn command (kind 44224).
///
/// Same envelope as [`build_coding_session_lifecycle_receipt`] — a consumer
/// reads both with one decoder — but keyed per stage, so the `turn_started`
/// that follows a `turn_queued` is a new fact rather than a duplicate.
pub fn build_coding_session_turn_receipt(
    channel_id: Uuid,
    command_id: &str,
    status: ReceiptStatus,
    content: &str,
) -> Result<EventBuilder, SdkError> {
    check_identifier(command_id, "commandId")?;
    check_content(content, MAX_LIFECYCLE_RECEIPT_CONTENT_BYTES)?;
    if !status.is_turn_stage() {
        return Err(SdkError::InvalidInput(
            "a turn receipt must carry a turn status".to_owned(),
        ));
    }
    let tags = vec![
        tag(&["h", &channel_id.to_string()])?,
        tag(&["cslr-v", CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION])?,
        tag(&["csl-command", command_id])?,
        tag(&[
            "csl-key",
            &coding_session_turn_receipt_semantic_key(command_id, status),
        ])?,
    ];
    Ok(EventBuilder::new(
        Kind::Custom(KIND_CODING_SESSION_LIFECYCLE_RECEIPT as u16),
        content.to_owned(),
    )
    .tags(tags))
}

/// Build one coding-session transcript item (kind 44225).
///
/// `event_seq` is the producer's per-(session, generation) monotonic counter,
/// persisted before publish. Gaps are permitted — a crashed producer resumes
/// past whatever it had already reserved — but duplicates are not, which is
/// exactly what `cst-key` lets a consumer enforce.
pub fn build_coding_session_transcript_item(
    channel_id: Uuid,
    target: &CodingSessionTarget,
    event_seq: u64,
    content: &str,
) -> Result<EventBuilder, SdkError> {
    check_target(target)?;
    if event_seq == 0 || event_seq > MAX_SAFE_GENERATION {
        return Err(SdkError::InvalidInput(
            "transcript eventSeq must be a positive safe integer".into(),
        ));
    }
    check_content(content, MAX_TRANSCRIPT_CONTENT_BYTES)?;
    let tags = vec![
        tag(&["h", &channel_id.to_string()])?,
        tag(&["cst-v", CODING_SESSION_TRANSCRIPT_TAG_VERSION])?,
        tag(&["cs-target", &coding_session_target_key(target)])?,
        tag(&["cst-seq", &event_seq.to_string()])?,
        tag(&[
            "cst-key",
            &coding_session_transcript_semantic_key(target, event_seq),
        ])?,
    ];
    Ok(EventBuilder::new(
        Kind::Custom(KIND_CODING_SESSION_TRANSCRIPT as u16),
        content.to_owned(),
    )
    .tags(tags))
}

/// Build one coding-session authority-chain transition (kind 44228).
///
/// The **current session owner** signs the returned builder — today, always
/// the genesis signer (see the module doc on
/// [`buzz_core::coding_session_authority_transition`]). The relay validates
/// the chain linkage (`prevAccepted`/`seq` against the current accepted
/// head) and the signer's standing atomically at ingest; this builder only
/// enforces the payload's own self-consistency before signing.
///
/// The `csat-genesis` tag is re-derived from the payload here and re-derived
/// again by the relay, so the filterable reference and the signed reference
/// are the same string by construction.
pub fn build_coding_session_authority_transition(
    channel_id: Uuid,
    payload: &CodingSessionAuthorityTransitionPayload,
) -> Result<EventBuilder, SdkError> {
    payload.validate().map_err(SdkError::InvalidInput)?;
    let content = serde_json::to_string(payload).map_err(|error| {
        SdkError::InvalidInput(format!(
            "coding-session authority-transition serialization: {error}"
        ))
    })?;
    check_content(&content, MAX_AUTHORITY_TRANSITION_CONTENT_BYTES)?;
    let tags = vec![
        tag(&["h", &channel_id.to_string()])?,
        tag(&["csat-v", CODING_SESSION_AUTHORITY_TRANSITION_TAG_VERSION])?,
        tag(&["csat-genesis", &payload.genesis_ref])?,
    ];
    Ok(EventBuilder::new(
        Kind::Custom(KIND_CODING_SESSION_AUTHORITY_TRANSITION as u16),
        content,
    )
    .tags(tags))
}

/// Validate a coding-session target the same way `buzz-core` validates the one
/// inside a 44220 payload, so provider-authored events cannot name a target no
/// command could ever have addressed.
fn check_target(target: &CodingSessionTarget) -> Result<(), SdkError> {
    check_identifier(&target.driver, "target.driver")?;
    check_identifier(&target.instance_id, "target.instanceId")?;
    check_identifier(&target.session_id, "target.sessionId")?;
    if target.generation == 0 || target.generation > MAX_SAFE_GENERATION {
        return Err(SdkError::InvalidInput(
            "target.generation must be a positive safe integer".into(),
        ));
    }
    Ok(())
}

fn check_identifier(value: &str, field: &str) -> Result<(), SdkError> {
    if value.trim().is_empty() {
        return Err(SdkError::InvalidInput(format!("{field} must not be empty")));
    }
    if value.len() > MAX_IDENTIFIER_BYTES {
        return Err(SdkError::InvalidInput(format!(
            "{field} exceeds {MAX_IDENTIFIER_BYTES} bytes"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::{EventId, Keys};

    fn keys() -> Keys {
        Keys::generate()
    }

    fn sign(b: EventBuilder) -> nostr::Event {
        b.sign_with_keys(&keys()).expect("sign")
    }

    fn event_id() -> EventId {
        let k = keys();
        EventBuilder::new(Kind::Custom(1), "x")
            .tags([])
            .sign_with_keys(&k)
            .expect("sign")
            .id
    }

    fn uuid() -> Uuid {
        Uuid::new_v4()
    }

    fn tag_values(event: &nostr::Event, key: &str) -> Vec<String> {
        event
            .tags
            .iter()
            .filter_map(|t| {
                let s = t.as_slice();
                if s.first().map(|v| v.as_str()) == Some(key) {
                    s.get(1).map(|v| v.to_string())
                } else {
                    None
                }
            })
            .collect()
    }

    fn has_tag(event: &nostr::Event, key: &str, val: &str) -> bool {
        event.tags.iter().any(|t| {
            let s = t.as_slice();
            s.first().map(|v| v.as_str()) == Some(key) && s.get(1).map(|v| v.as_str()) == Some(val)
        })
    }

    #[test]
    fn message_happy_path() {
        let cid = uuid();
        let ev = sign(build_message(cid, "hello", None, &[], false, &[]).unwrap());
        assert_eq!(ev.kind.as_u16(), 9);
        assert_eq!(ev.content, "hello");
        assert!(has_tag(&ev, "h", &cid.to_string()));
    }

    #[test]
    fn message_preserves_self_mention_p_tag() {
        // nostr 0.44 strips p tags matching the signer by default.
        // build_message must opt in via allow_self_tagging() so that
        // explicit self-mentions survive signing. See #4906.
        let cid = uuid();
        let sender = keys();
        let self_pk = sender.public_key().to_hex();
        let builder = build_message(cid, "self-canary", None, &[&self_pk], false, &[]).unwrap();
        let ev = builder.sign_with_keys(&sender).expect("sign");
        assert!(
            has_tag(&ev, "p", &self_pk),
            "self-mention p tag must survive signing"
        );
    }

    #[test]
    fn forum_post_preserves_self_mention_p_tag() {
        let cid = uuid();
        let sender = keys();
        let self_pk = sender.public_key().to_hex();
        let builder = build_forum_post(cid, "self-canary", &[&self_pk], &[]).unwrap();
        let ev = builder.sign_with_keys(&sender).expect("sign");
        assert!(
            has_tag(&ev, "p", &self_pk),
            "self-mention p tag must survive signing"
        );
    }

    #[test]
    fn forum_comment_preserves_self_mention_p_tag() {
        let cid = uuid();
        let sender = keys();
        let self_pk = sender.public_key().to_hex();
        let root = event_id();
        let tr = ThreadRef {
            root_event_id: root,
            parent_event_id: root,
        };
        let builder = build_forum_comment(cid, "self-canary", &tr, &[&self_pk], &[]).unwrap();
        let ev = builder.sign_with_keys(&sender).expect("sign");
        assert!(
            has_tag(&ev, "p", &self_pk),
            "self-mention p tag must survive signing"
        );
    }

    #[test]
    fn agent_observer_frame_happy_path() {
        let sender = keys();
        let recipient = keys();
        let agent = keys();
        let encrypted = buzz_core::observer::encrypt_observer_payload(
            &sender,
            &recipient.public_key(),
            &serde_json::json!({"type": "acp_read"}),
        )
        .unwrap();
        let ev = sign(
            build_agent_observer_frame(
                &recipient.public_key().to_hex(),
                &agent.public_key().to_hex(),
                OBSERVER_FRAME_TELEMETRY,
                &encrypted,
            )
            .unwrap(),
        );

        assert_eq!(ev.kind.as_u16(), KIND_AGENT_OBSERVER_FRAME as u16);
        assert_eq!(ev.content, encrypted);
        assert!(has_tag(&ev, "p", &recipient.public_key().to_hex()));
        assert!(has_tag(
            &ev,
            OBSERVER_AGENT_TAG,
            &agent.public_key().to_hex()
        ));
        assert!(has_tag(&ev, OBSERVER_FRAME_TAG, OBSERVER_FRAME_TELEMETRY));
    }

    #[test]
    fn agent_observer_frame_rejects_plaintext_content() {
        let err = build_agent_observer_frame(
            &"a".repeat(64),
            &"b".repeat(64),
            OBSERVER_FRAME_TELEMETRY,
            "not encrypted",
        )
        .unwrap_err();
        assert!(matches!(err, SdkError::InvalidInput(_)));
    }

    #[test]
    fn message_direct_reply() {
        let cid = uuid();
        let eid = event_id();
        let tr = ThreadRef {
            root_event_id: eid,
            parent_event_id: eid,
        };
        let ev = sign(build_message(cid, "reply", Some(&tr), &[], false, &[]).unwrap());
        // Direct reply: only one e-tag with "reply" marker
        let e_tags: Vec<_> = ev
            .tags
            .iter()
            .filter(|t| t.as_slice().first().map(|v| v.as_str()) == Some("e"))
            .collect();
        assert_eq!(e_tags.len(), 1);
        assert_eq!(
            e_tags[0].as_slice().get(3).map(|v| v.as_str()),
            Some("reply")
        );
    }

    #[test]
    fn message_nested_reply() {
        let cid = uuid();
        let root = event_id();
        let parent = event_id();
        let tr = ThreadRef {
            root_event_id: root,
            parent_event_id: parent,
        };
        let ev = sign(build_message(cid, "nested", Some(&tr), &[], false, &[]).unwrap());
        let e_tags: Vec<_> = ev
            .tags
            .iter()
            .filter(|t| t.as_slice().first().map(|v| v.as_str()) == Some("e"))
            .collect();
        assert_eq!(e_tags.len(), 2);
        let markers: Vec<_> = e_tags
            .iter()
            .filter_map(|t| t.as_slice().get(3).map(|v| v.as_str()))
            .collect();
        assert!(markers.contains(&"root"));
        assert!(markers.contains(&"reply"));
    }

    #[test]
    fn message_broadcast_flag() {
        let cid = uuid();
        let ev = sign(build_message(cid, "hi", None, &[], true, &[]).unwrap());
        assert!(has_tag(&ev, "broadcast", "1"));
    }

    #[test]
    fn message_mentions_deduped() {
        let cid = uuid();
        let hex = "abcd1234abcd1234abcd1234abcd1234abcd1234abcd1234abcd1234abcd1234";
        let ev = sign(build_message(cid, "hi", None, &[hex, hex], false, &[]).unwrap());
        let p_tags = tag_values(&ev, "p");
        assert_eq!(p_tags.len(), 1);
    }

    #[test]
    fn message_too_many_mentions() {
        let cid = uuid();
        let hex = "abcd1234abcd1234abcd1234abcd1234abcd1234abcd1234abcd1234abcd1234";
        let _mentions: Vec<&str> = (0..51).map(|_| hex).collect();
        // All same hex so dedup would reduce to 1, but the check is on raw len
        // Let's use 51 distinct-ish values by varying the first char
        let hexes: Vec<String> = (0..51u8)
            .map(|i| {
                format!(
                    "{:02x}cd1234abcd1234abcd1234abcd1234abcd1234abcd1234abcd1234abcd12",
                    i
                )
            })
            .collect();
        let refs: Vec<&str> = hexes.iter().map(|s| s.as_str()).collect();
        let result = build_message(cid, "hi", None, &refs, false, &[]);
        assert!(matches!(result, Err(SdkError::TooManyMentions)));
    }

    #[test]
    fn message_content_too_large() {
        let cid = uuid();
        let big = "x".repeat(64 * 1024 + 1);
        let result = build_message(cid, &big, None, &[], false, &[]);
        assert!(matches!(result, Err(SdkError::ContentTooLarge { .. })));
    }

    #[test]
    fn message_max_content_ok() {
        let cid = uuid();
        let max = "x".repeat(64 * 1024);
        assert!(build_message(cid, &max, None, &[], false, &[]).is_ok());
    }

    #[test]
    fn forum_post_happy_path() {
        let cid = uuid();
        let ev = sign(build_forum_post(cid, "post body", &[], &[]).unwrap());
        assert_eq!(ev.kind.as_u16(), 45001);
        assert!(has_tag(&ev, "h", &cid.to_string()));
    }

    #[test]
    fn forum_post_content_too_large() {
        let cid = uuid();
        let big = "x".repeat(64 * 1024 + 1);
        assert!(matches!(
            build_forum_post(cid, &big, &[], &[]),
            Err(SdkError::ContentTooLarge { .. })
        ));
    }

    #[test]
    fn forum_comment_happy_path() {
        let cid = uuid();
        let eid = event_id();
        let tr = ThreadRef {
            root_event_id: eid,
            parent_event_id: eid,
        };
        let ev = sign(build_forum_comment(cid, "comment", &tr, &[], &[]).unwrap());
        assert_eq!(ev.kind.as_u16(), 45003);
        assert!(has_tag(&ev, "h", &cid.to_string()));
    }

    fn good_diff_meta() -> DiffMeta {
        DiffMeta {
            repo_url: "https://github.com/example/repo".into(),
            commit_sha: "abc1234".into(),
            file_path: Some("src/main.rs".into()),
            parent_commit: None,
            branch: None,
            pr_number: None,
            language: Some("rust".into()),
            description: None,
            truncated: false,
            alt_text: None,
        }
    }

    #[test]
    fn diff_message_happy_path() {
        let cid = uuid();
        let ev = sign(build_diff_message(cid, "diff content", &good_diff_meta(), None).unwrap());
        assert_eq!(ev.kind.as_u16(), 40008);
        assert!(has_tag(&ev, "repo", "https://github.com/example/repo"));
        assert!(has_tag(&ev, "commit", "abc1234"));
        assert!(has_tag(&ev, "l", "rust"));
    }

    #[test]
    fn diff_message_bad_repo_url() {
        let cid = uuid();
        let mut meta = good_diff_meta();
        meta.repo_url = "ftp://bad.url".into();
        assert!(matches!(
            build_diff_message(cid, "x", &meta, None),
            Err(SdkError::InvalidDiffMeta(_))
        ));
    }

    #[test]
    fn diff_message_short_commit_sha() {
        let cid = uuid();
        let mut meta = good_diff_meta();
        meta.commit_sha = "abc12".into(); // only 5 chars
        assert!(matches!(
            build_diff_message(cid, "x", &meta, None),
            Err(SdkError::InvalidDiffMeta(_))
        ));
    }

    #[test]
    fn diff_message_invalid_commit_sha_chars() {
        let cid = uuid();
        let mut meta = good_diff_meta();
        meta.commit_sha = "xyz1234".into(); // 'x', 'y', 'z' not hex
        assert!(matches!(
            build_diff_message(cid, "x", &meta, None),
            Err(SdkError::InvalidDiffMeta(_))
        ));
    }

    #[test]
    fn diff_message_branch_only_source() {
        let cid = uuid();
        let mut meta = good_diff_meta();
        meta.branch = Some(("main".into(), "".into())); // target empty
        assert!(matches!(
            build_diff_message(cid, "x", &meta, None),
            Err(SdkError::InvalidDiffMeta(_))
        ));
    }

    #[test]
    fn diff_message_pr_zero() {
        let cid = uuid();
        let mut meta = good_diff_meta();
        meta.pr_number = Some(0);
        assert!(matches!(
            build_diff_message(cid, "x", &meta, None),
            Err(SdkError::InvalidDiffMeta(_))
        ));
    }

    #[test]
    fn diff_message_content_too_large() {
        let cid = uuid();
        let big = "x".repeat(60 * 1024 + 1);
        assert!(matches!(
            build_diff_message(cid, &big, &good_diff_meta(), None),
            Err(SdkError::ContentTooLarge { .. })
        ));
    }

    #[test]
    fn diff_message_all_optional_fields() {
        let cid = uuid();
        let meta = DiffMeta {
            repo_url: "https://github.com/example/repo".into(),
            commit_sha: "abc1234def".into(),
            file_path: Some("src/lib.rs".into()),
            parent_commit: Some("1234567".into()),
            branch: Some(("feature".into(), "main".into())),
            pr_number: Some(42),
            language: Some("rust".into()),
            description: Some("fix bug".into()),
            truncated: true,
            alt_text: Some("patch for bug fix".into()),
        };
        let ev = sign(build_diff_message(cid, "diff", &meta, None).unwrap());
        assert!(has_tag(&ev, "file", "src/lib.rs"));
        assert!(has_tag(&ev, "parent-commit", "1234567"));
        assert!(has_tag(&ev, "pr", "42"));
        assert!(has_tag(&ev, "truncated", "true"));
        assert!(has_tag(&ev, "alt", "patch for bug fix"));
    }

    #[test]
    fn edit_happy_path() {
        let cid = uuid();
        let eid = event_id();
        let ev = sign(build_edit(cid, eid, "new content").unwrap());
        assert_eq!(ev.kind.as_u16(), 40003);
        assert!(has_tag(&ev, "e", &eid.to_hex()));
    }

    #[test]
    fn edit_content_too_large() {
        let cid = uuid();
        let eid = event_id();
        let big = "x".repeat(64 * 1024 + 1);
        assert!(matches!(
            build_edit(cid, eid, &big),
            Err(SdkError::ContentTooLarge { .. })
        ));
    }

    #[test]
    fn delete_message_happy_path() {
        let cid = uuid();
        let eid = event_id();
        let ev = sign(build_delete_message(cid, eid).unwrap());
        assert_eq!(ev.kind.as_u16(), 9005);
        assert!(has_tag(&ev, "h", &cid.to_string()));
        assert!(has_tag(&ev, "e", &eid.to_hex()));
        assert_eq!(ev.content, "");
    }

    #[test]
    fn delete_message_with_moderation_metadata() {
        let cid = uuid();
        let eid = event_id();
        let action_id = Uuid::new_v4();
        let ev = sign(
            build_delete_message_with_options(
                cid,
                eid,
                DeleteMessageOptions {
                    action_id: Some(action_id),
                    reason_code: Some("spam"),
                    public_reason: Some("Removed for spam."),
                },
            )
            .unwrap(),
        );
        assert_eq!(ev.kind.as_u16(), 9005);
        assert!(has_tag(&ev, "h", &cid.to_string()));
        assert!(has_tag(&ev, "e", &eid.to_hex()));
        assert!(has_tag(&ev, "action_id", &action_id.to_string()));
        assert!(has_tag(&ev, "reason_code", "spam"));
        assert!(has_tag(&ev, "public_reason", "Removed for spam."));
    }

    #[test]
    fn delete_compat_happy_path() {
        let cid = uuid();
        let eid = event_id();
        let ev = sign(build_delete_compat(cid, eid).unwrap());
        assert_eq!(ev.kind.as_u16(), 5);
        assert!(has_tag(&ev, "h", &cid.to_string()));
        assert!(has_tag(&ev, "e", &eid.to_hex()));
        assert_eq!(ev.content, "");
    }

    #[test]
    fn vote_up() {
        let cid = uuid();
        let eid = event_id();
        let ev = sign(build_vote(cid, eid, VoteDirection::Up).unwrap());
        assert_eq!(ev.kind.as_u16(), 45002);
        assert_eq!(ev.content, "+");
    }

    #[test]
    fn vote_down() {
        let cid = uuid();
        let eid = event_id();
        let ev = sign(build_vote(cid, eid, VoteDirection::Down).unwrap());
        assert_eq!(ev.content, "-");
    }

    #[test]
    fn reaction_happy_path() {
        let eid = event_id();
        let ev = sign(build_reaction(eid, "👍").unwrap());
        assert_eq!(ev.kind.as_u16(), 7);
        assert_eq!(ev.content, "👍");
    }

    #[test]
    fn reaction_emoji_too_long() {
        let eid = event_id();
        let long_emoji = "a".repeat(65);
        assert!(matches!(
            build_reaction(eid, &long_emoji),
            Err(SdkError::EmojiTooLong)
        ));
    }

    #[test]
    fn reaction_emoji_max_len_ok() {
        let eid = event_id();
        let max_emoji = "a".repeat(64);
        assert!(build_reaction(eid, &max_emoji).is_ok());
    }

    #[test]
    fn custom_emoji_reaction_happy_path() {
        let eid = event_id();
        let ev = sign(
            build_custom_emoji_reaction(eid, ":Party_Parrot:", "https://example.com/parrot.png")
                .unwrap(),
        );
        assert_eq!(ev.kind.as_u16(), 7);
        assert_eq!(ev.content, ":party_parrot:");
        assert!(has_tag(&ev, "emoji", "party_parrot"));
    }

    #[test]
    fn custom_emoji_reaction_accepts_max_shortcode_length() {
        let eid = event_id();
        let shortcode = "a".repeat(MAX_CUSTOM_EMOJI_SHORTCODE_LEN);
        let ev = sign(
            build_custom_emoji_reaction(eid, &shortcode, "https://example.com/max.png").unwrap(),
        );

        assert_eq!(ev.content, format!(":{shortcode}:"));
        assert_eq!(ev.content.chars().count(), MAX_CUSTOM_EMOJI_REACTION_LEN);
        assert!(has_tag(&ev, "emoji", &shortcode));
    }

    #[test]
    fn custom_emoji_reaction_rejects_overlong_shortcode() {
        let eid = event_id();
        let shortcode = "a".repeat(MAX_CUSTOM_EMOJI_SHORTCODE_LEN + 1);

        assert!(matches!(
            build_custom_emoji_reaction(eid, &shortcode, "https://example.com/too-long.png"),
            Err(SdkError::InvalidInput(message)) if message.contains("exceeds 64 bytes")
        ));
    }

    #[test]
    fn custom_emoji_set_happy_path() {
        let ev = sign(
            build_custom_emoji_set(&[CustomEmoji {
                shortcode: "party".to_string(),
                url: "https://example.com/party.png".to_string(),
            }])
            .unwrap(),
        );
        assert_eq!(ev.kind.as_u16(), 30030);
        assert!(has_tag(&ev, "d", CUSTOM_EMOJI_SET_D_TAG));
        assert!(has_tag(&ev, "emoji", "party"));
    }

    #[test]
    fn remove_reaction_happy_path() {
        let eid = event_id();
        let ev = sign(build_remove_reaction(eid).unwrap());
        assert_eq!(ev.kind.as_u16(), 5);
        assert!(has_tag(&ev, "e", &eid.to_hex()));
    }

    #[test]
    fn set_canvas_happy_path() {
        let cid = uuid();
        let ev = sign(build_set_canvas(cid, "# Canvas\nHello").unwrap());
        assert_eq!(ev.kind.as_u16(), 40100);
        assert!(has_tag(&ev, "h", &cid.to_string()));
        assert_eq!(ev.content, "# Canvas\nHello");
    }

    #[test]
    fn profile_all_fields() {
        let ev = sign(
            build_profile(
                Some("Alice"),
                Some("alice"),
                Some("https://example.com/pic.jpg"),
                Some("Hello world"),
                Some("alice@example.com"),
            )
            .unwrap(),
        );
        assert_eq!(ev.kind.as_u16(), 0);
        let v: serde_json::Value = serde_json::from_str(&ev.content).unwrap();
        assert_eq!(v["display_name"], "Alice");
        assert_eq!(v["name"], "alice");
        assert_eq!(v["nip05"], "alice@example.com");
    }

    #[test]
    fn profile_some_fields() {
        let ev = sign(build_profile(Some("Bob"), None, None, None, None).unwrap());
        let v: serde_json::Value = serde_json::from_str(&ev.content).unwrap();
        assert_eq!(v["display_name"], "Bob");
        assert!(
            v.get("name").is_none()
                || !v["name"].is_null() && v.get("name") == Some(&serde_json::Value::Null)
                || !v.as_object().unwrap().contains_key("name")
        );
    }

    #[test]
    fn profile_no_fields() {
        let ev = sign(build_profile(None, None, None, None, None).unwrap());
        let v: serde_json::Value = serde_json::from_str(&ev.content).unwrap();
        assert!(v.as_object().unwrap().is_empty());
    }

    #[test]
    fn add_member_with_role() {
        let cid = uuid();
        let pubkey = "abcd1234abcd1234abcd1234abcd1234abcd1234abcd1234abcd1234abcd1234";
        let ev = sign(build_add_member(cid, pubkey, Some(MemberRole::Admin)).unwrap());
        assert_eq!(ev.kind.as_u16(), 9000);
        assert!(has_tag(&ev, "p", pubkey));
        assert!(has_tag(&ev, "role", "admin"));
    }

    #[test]
    fn add_member_without_role() {
        let cid = uuid();
        let pubkey = "abcd1234abcd1234abcd1234abcd1234abcd1234abcd1234abcd1234abcd1234";
        let ev = sign(build_add_member(cid, pubkey, None::<MemberRole>).unwrap());
        assert_eq!(ev.kind.as_u16(), 9000);
        assert!(tag_values(&ev, "role").is_empty());
    }

    #[test]
    fn remove_member_happy_path() {
        let cid = uuid();
        let pubkey = "abcd1234abcd1234abcd1234abcd1234abcd1234abcd1234abcd1234abcd1234";
        let ev = sign(build_remove_member(cid, pubkey).unwrap());
        assert_eq!(ev.kind.as_u16(), 9001);
        assert!(has_tag(&ev, "p", pubkey));
    }

    #[test]
    fn leave_happy_path() {
        let cid = uuid();
        let ev = sign(build_leave(cid).unwrap());
        assert_eq!(ev.kind.as_u16(), 9022);
        assert!(has_tag(&ev, "h", &cid.to_string()));
    }

    #[test]
    fn update_channel_name_and_about() {
        let cid = uuid();
        let ev = sign(
            build_update_channel(cid, Some("new-name"), Some("new about"), None, None).unwrap(),
        );
        assert_eq!(ev.kind.as_u16(), 9002);
        assert!(has_tag(&ev, "name", "new-name"));
        assert!(has_tag(&ev, "about", "new about"));
    }

    #[test]
    fn update_channel_strips_all_leading_hashes_from_name() {
        let ev =
            sign(build_update_channel(uuid(), Some("  ###new-name  "), None, None, None).unwrap());
        assert!(has_tag(&ev, "name", "new-name"));
    }

    #[test]
    fn update_channel_rejects_hash_only_name() {
        assert!(matches!(
            build_update_channel(uuid(), Some("  ###  "), None, None, None),
            Err(SdkError::InvalidTag(_))
        ));
    }

    #[test]
    fn update_channel_visibility_and_ttl() {
        let cid = uuid();
        let ev =
            sign(build_update_channel(cid, None, None, Some("private"), Some(Some(3600))).unwrap());
        assert_eq!(ev.kind.as_u16(), 9002);
        assert!(has_tag(&ev, "visibility", "private"));
        assert!(has_tag(&ev, "ttl", "3600"));
    }

    #[test]
    fn update_channel_clears_ttl() {
        let cid = uuid();
        let ev = sign(build_update_channel(cid, None, None, None, Some(None)).unwrap());
        assert!(has_tag(&ev, "ttl", ""));
    }

    #[test]
    fn update_channel_invalid_visibility_rejected() {
        let cid = uuid();
        assert!(matches!(
            build_update_channel(cid, None, None, Some("secret"), None),
            Err(SdkError::InvalidTag(_))
        ));
    }

    #[test]
    fn update_channel_no_fields_rejected() {
        let cid = uuid();
        assert!(matches!(
            build_update_channel(cid, None, None, None, None),
            Err(SdkError::InvalidTag(_))
        ));
    }

    #[test]
    fn set_topic_happy_path() {
        let cid = uuid();
        let ev = sign(build_set_topic(cid, "Rust async patterns").unwrap());
        assert_eq!(ev.kind.as_u16(), 9002);
        assert!(has_tag(&ev, "topic", "Rust async patterns"));
    }

    #[test]
    fn set_purpose_happy_path() {
        let cid = uuid();
        let ev = sign(build_set_purpose(cid, "Team coordination").unwrap());
        assert_eq!(ev.kind.as_u16(), 9002);
        assert!(has_tag(&ev, "purpose", "Team coordination"));
    }

    #[test]
    fn create_channel_all_fields() {
        let cid = uuid();
        let ev = sign(
            build_create_channel(
                cid,
                "general",
                Some(Visibility::Open),
                Some(ChannelKind::Stream),
                Some("General chat"),
                None,
            )
            .unwrap(),
        );
        assert_eq!(ev.kind.as_u16(), 9007);
        assert!(has_tag(&ev, "name", "general"));
        assert!(has_tag(&ev, "visibility", "open"));
        assert!(has_tag(&ev, "channel_type", "stream"));
        assert!(has_tag(&ev, "about", "General chat"));
    }

    #[test]
    fn create_channel_minimal() {
        let cid = uuid();
        let ev = sign(
            build_create_channel(
                cid,
                "dev",
                None::<Visibility>,
                None::<ChannelKind>,
                None,
                None,
            )
            .unwrap(),
        );
        assert_eq!(ev.kind.as_u16(), 9007);
        assert!(has_tag(&ev, "name", "dev"));
    }

    #[test]
    fn create_channel_strips_all_leading_hashes_from_name() {
        let ev = sign(
            build_create_channel(
                uuid(),
                "  ###dev  ",
                None::<Visibility>,
                None::<ChannelKind>,
                None,
                None,
            )
            .unwrap(),
        );
        assert!(has_tag(&ev, "name", "dev"));
    }

    #[test]
    fn create_channel_rejects_hash_only_name() {
        assert!(matches!(
            build_create_channel(
                uuid(),
                "  ###  ",
                None::<Visibility>,
                None::<ChannelKind>,
                None,
                None,
            ),
            Err(SdkError::InvalidTag(_))
        ));
    }

    #[test]
    fn create_channel_ephemeral_emits_ttl() {
        let cid = uuid();
        let ev = sign(
            build_create_channel(
                cid,
                "standup",
                Some(Visibility::Open),
                Some(ChannelKind::Stream),
                None,
                Some(3600),
            )
            .unwrap(),
        );
        assert_eq!(ev.kind.as_u16(), 9007);
        assert!(has_tag(&ev, "ttl", "3600"));
    }

    #[test]
    fn join_happy_path() {
        let cid = uuid();
        let ev = sign(build_join(cid).unwrap());
        assert_eq!(ev.kind.as_u16(), 9021);
        assert!(has_tag(&ev, "h", &cid.to_string()));
    }

    #[test]
    fn archive_happy_path() {
        let cid = uuid();
        let ev = sign(build_archive(cid).unwrap());
        assert_eq!(ev.kind.as_u16(), 9002);
        assert!(has_tag(&ev, "archived", "true"));
    }

    #[test]
    fn unarchive_happy_path() {
        let cid = uuid();
        let ev = sign(build_unarchive(cid).unwrap());
        assert_eq!(ev.kind.as_u16(), 9002);
        assert!(has_tag(&ev, "archived", "false"));
    }

    #[test]
    fn delete_channel_happy_path() {
        let cid = uuid();
        let ev = sign(build_delete_channel(cid).unwrap());
        assert_eq!(ev.kind.as_u16(), 9008);
        assert!(has_tag(&ev, "h", &cid.to_string()));
    }

    #[test]
    fn extract_channel_id_present() {
        let cid = uuid();
        let ev = sign(build_join(cid).unwrap());
        assert_eq!(extract_channel_id(&ev), Some(cid));
    }

    #[test]
    fn extract_channel_id_absent() {
        // build_note (kind 1) is a global text note — no h tag.
        let ev = sign(build_note("hello", None).unwrap());
        assert_eq!(extract_channel_id(&ev), None);
    }

    #[test]
    fn extract_channel_id_invalid_uuid() {
        // Build an event with a malformed h-tag value
        let tags = vec![Tag::parse(["h", "not-a-uuid"]).unwrap()];
        let ev = EventBuilder::new(Kind::Custom(9), "x")
            .tags(tags)
            .sign_with_keys(&keys())
            .unwrap();
        assert_eq!(extract_channel_id(&ev), None);
    }

    #[test]
    fn build_note_happy_path() {
        let builder = build_note("hello world", None).unwrap();
        let keys = nostr::Keys::generate();
        let event = builder.sign_with_keys(&keys).unwrap();
        assert_eq!(event.kind, Kind::Custom(1));
        assert_eq!(event.content, "hello world");
        assert!(event.tags.is_empty());
    }

    #[test]
    fn build_note_with_reply() {
        let keys = nostr::Keys::generate();
        // Create a dummy event to get a valid EventId
        let dummy = EventBuilder::new(Kind::Custom(1), "dummy")
            .tags(vec![])
            .sign_with_keys(&keys)
            .unwrap();
        let builder = build_note("reply text", Some(dummy.id)).unwrap();
        let event = builder.sign_with_keys(&keys).unwrap();
        assert_eq!(event.kind, Kind::Custom(1));
        assert_eq!(event.content, "reply text");
        assert_eq!(event.tags.len(), 1);
        let tag = event.tags.iter().next().unwrap();
        assert_eq!(tag.as_slice()[0], "e");
        assert_eq!(tag.as_slice()[1], dummy.id.to_hex());
        assert_eq!(tag.as_slice()[3], "reply");
    }

    #[test]
    fn build_note_content_too_large() {
        let big = "x".repeat(64 * 1024 + 1);
        let err = build_note(&big, None).unwrap_err();
        assert!(matches!(err, SdkError::ContentTooLarge { .. }));
    }

    #[test]
    fn build_note_empty_content() {
        // Empty content is valid per NIP-01.
        let builder = build_note("", None).unwrap();
        let keys = nostr::Keys::generate();
        let event = builder.sign_with_keys(&keys).unwrap();
        assert_eq!(event.kind, Kind::Custom(1));
        assert_eq!(event.content, "");
        assert!(event.tags.is_empty());
    }

    #[test]
    fn build_contact_list_happy_path() {
        let pubkey = "a".repeat(64);
        let contacts = vec![(pubkey.as_str(), None, None)];
        let builder = build_contact_list(&contacts).unwrap();
        let keys = nostr::Keys::generate();
        let event = builder.sign_with_keys(&keys).unwrap();
        assert_eq!(event.kind, Kind::Custom(3));
        assert_eq!(event.content, "");
        assert_eq!(event.tags.len(), 1);
        let tag = event.tags.iter().next().unwrap();
        assert_eq!(tag.as_slice()[0], "p");
        assert_eq!(tag.as_slice()[1], pubkey);
    }

    #[test]
    fn build_contact_list_normalizes_uppercase() {
        let upper = "A".repeat(64);
        let contacts = vec![(upper.as_str(), None, None)];
        let builder = build_contact_list(&contacts).unwrap();
        let keys = nostr::Keys::generate();
        let event = builder.sign_with_keys(&keys).unwrap();
        let tag = event.tags.iter().next().unwrap();
        assert_eq!(tag.as_slice()[1], "a".repeat(64));
    }

    #[test]
    fn build_contact_list_with_relay_and_petname() {
        let pubkey = "b".repeat(64);
        let contacts = vec![(
            pubkey.as_str(),
            Some("wss://relay.example.com"),
            Some("alice"),
        )];
        let builder = build_contact_list(&contacts).unwrap();
        let keys = nostr::Keys::generate();
        let event = builder.sign_with_keys(&keys).unwrap();
        let tag = event.tags.iter().next().unwrap();
        assert_eq!(tag.as_slice()[0], "p");
        assert_eq!(tag.as_slice()[2], "wss://relay.example.com");
        assert_eq!(tag.as_slice()[3], "alice");
    }

    #[test]
    fn build_contact_list_empty() {
        let builder = build_contact_list(&[]).unwrap();
        let keys = nostr::Keys::generate();
        let event = builder.sign_with_keys(&keys).unwrap();
        assert_eq!(event.kind, Kind::Custom(3));
        assert!(event.tags.is_empty());
    }

    #[test]
    fn build_contact_list_rejects_short_pubkey() {
        let short = "a".repeat(63);
        let contacts = vec![(short.as_str(), None, None)];
        let err = build_contact_list(&contacts).unwrap_err();
        assert!(matches!(err, SdkError::InvalidInput(_)));
    }

    #[test]
    fn build_contact_list_rejects_long_pubkey() {
        let long = "a".repeat(65);
        let contacts = vec![(long.as_str(), None, None)];
        let err = build_contact_list(&contacts).unwrap_err();
        assert!(matches!(err, SdkError::InvalidInput(_)));
    }

    #[test]
    fn build_contact_list_rejects_non_hex() {
        let non_hex = "g".repeat(64);
        let contacts = vec![(non_hex.as_str(), None, None)];
        let err = build_contact_list(&contacts).unwrap_err();
        assert!(matches!(err, SdkError::InvalidInput(_)));
    }

    #[test]
    fn build_contact_list_rejects_long_relay_url() {
        let pubkey = "a".repeat(64);
        let long_url = "x".repeat(2049);
        let contacts = vec![(pubkey.as_str(), Some(long_url.as_str()), None)];
        let err = build_contact_list(&contacts).unwrap_err();
        assert!(matches!(err, SdkError::InvalidInput(_)));
    }

    #[test]
    fn build_contact_list_rejects_long_petname() {
        let pubkey = "a".repeat(64);
        let long_name = "x".repeat(257);
        let contacts = vec![(pubkey.as_str(), None, Some(long_name.as_str()))];
        let err = build_contact_list(&contacts).unwrap_err();
        assert!(matches!(err, SdkError::InvalidInput(_)));
    }

    #[test]
    fn build_contact_list_duplicate_pubkeys() {
        let pubkey = "c".repeat(64);
        // Same pubkey twice — only one p-tag should be emitted.
        let contacts = vec![
            (pubkey.as_str(), None, None),
            (
                pubkey.as_str(),
                Some("wss://relay.example.com"),
                Some("bob"),
            ),
        ];
        let builder = build_contact_list(&contacts).unwrap();
        let keys = nostr::Keys::generate();
        let event = builder.sign_with_keys(&keys).unwrap();
        assert_eq!(event.tags.len(), 1);
        let tag = event.tags.iter().next().unwrap();
        assert_eq!(tag.as_slice()[0], "p");
        assert_eq!(tag.as_slice()[1], pubkey);
    }

    #[test]
    fn build_contact_list_too_many() {
        let pubkey = "d".repeat(64);
        // MAX_CONTACTS + 1 entries (all same pubkey — uniqueness doesn't matter,
        // the cap is checked before deduplication).
        let entry = (pubkey.as_str(), None, None);
        let contacts = vec![entry; MAX_CONTACTS + 1];
        let err = build_contact_list(&contacts).unwrap_err();
        assert!(matches!(err, SdkError::InvalidInput(_)));
    }

    #[test]
    fn repo_announcement_happy_path_all_fields() {
        let ev = sign(
            build_repo_announcement(
                "my-repo",
                Some("My Repo"),
                Some("A test repository"),
                &["https://github.com/example/my-repo.git"],
                Some("https://github.com/example/my-repo"),
                &["wss://relay.example.com"],
            )
            .unwrap(),
        );
        assert_eq!(ev.kind.as_u16(), 30617);
        assert_eq!(ev.content, "");
        assert!(has_tag(&ev, "d", "my-repo"));
        assert!(has_tag(&ev, "name", "My Repo"));
        assert!(has_tag(&ev, "description", "A test repository"));
        assert!(has_tag(
            &ev,
            "clone",
            "https://github.com/example/my-repo.git"
        ));
        assert!(has_tag(&ev, "web", "https://github.com/example/my-repo"));
        // relays is a multi-value tag — check the tag key exists
        assert!(ev.tags.iter().any(|t| {
            let s = t.as_slice();
            s.first().map(|v| v.as_str()) == Some("relays")
                && s.get(1).map(|v| v.as_str()) == Some("wss://relay.example.com")
        }));
    }

    #[test]
    fn repo_announcement_happy_path_minimal() {
        let ev = sign(build_repo_announcement("bare-repo", None, None, &[], None, &[]).unwrap());
        assert_eq!(ev.kind.as_u16(), 30617);
        assert_eq!(ev.content, "");
        assert!(has_tag(&ev, "d", "bare-repo"));
        // No optional tags present
        assert!(!ev
            .tags
            .iter()
            .any(|t| t.as_slice().first().map(|v| v.as_str()) == Some("name")));
        assert!(!ev
            .tags
            .iter()
            .any(|t| t.as_slice().first().map(|v| v.as_str()) == Some("clone")));
    }

    #[test]
    fn repo_announcement_with_tags_preserves_metadata_and_canonicalizes_d() {
        let tags = vec![
            Tag::parse(["d", "wrong-repo"]).unwrap(),
            Tag::parse(["name", "Protected Repo"]).unwrap(),
            Tag::parse(["buzz-channel", "channel-id"]).unwrap(),
            Tag::parse(["future-metadata", "preserve-me"]).unwrap(),
        ];

        let ev = sign(
            build_repo_announcement_with_tags("protected-repo", "repository content", tags)
                .unwrap(),
        );

        assert_eq!(ev.kind.as_u16(), 30617);
        assert_eq!(ev.content, "repository content");
        assert_eq!(
            ev.tags
                .iter()
                .filter(|tag| tag.as_slice().first().map(String::as_str) == Some("d"))
                .count(),
            1
        );
        assert!(has_tag(&ev, "d", "protected-repo"));
        assert!(has_tag(&ev, "buzz-channel", "channel-id"));
        assert!(has_tag(&ev, "future-metadata", "preserve-me"));
    }

    #[test]
    fn repo_announcement_rejects_empty_repo_id() {
        let err = build_repo_announcement("", None, None, &[], None, &[]).unwrap_err();
        assert!(matches!(err, SdkError::InvalidInput(_)));
    }

    #[test]
    fn repo_announcement_rejects_leading_dot() {
        let err = build_repo_announcement(".hidden", None, None, &[], None, &[]).unwrap_err();
        assert!(matches!(err, SdkError::InvalidInput(_)));
    }

    #[test]
    fn repo_announcement_rejects_double_dot() {
        let err = build_repo_announcement("some..repo", None, None, &[], None, &[]).unwrap_err();
        assert!(matches!(err, SdkError::InvalidInput(_)));
    }

    #[test]
    fn repo_announcement_rejects_repo_id_over_64_chars() {
        let long_id = "a".repeat(65);
        let err = build_repo_announcement(&long_id, None, None, &[], None, &[]).unwrap_err();
        assert!(matches!(err, SdkError::InvalidInput(_)));
    }

    #[test]
    fn repo_announcement_rejects_invalid_chars_in_repo_id() {
        let err = build_repo_announcement("bad repo!", None, None, &[], None, &[]).unwrap_err();
        assert!(matches!(err, SdkError::InvalidInput(_)));
    }

    #[test]
    fn repo_announcement_multiple_clone_urls_multi_value_tag() {
        let ev = sign(
            build_repo_announcement(
                "multi-clone",
                None,
                None,
                &[
                    "https://relay.example.com/git/abc/multi-clone",
                    "ssh://git@github.com/org/multi-clone.git",
                ],
                None,
                &[],
            )
            .unwrap(),
        );
        // clone is a multi-value tag per NIP-34: ["clone", url1, url2, ...]
        let clone_tag = ev
            .tags
            .iter()
            .find(|t| t.as_slice().first().map(|v| v.as_str()) == Some("clone"))
            .expect("clone tag missing");
        let vals: Vec<&str> = clone_tag
            .as_slice()
            .iter()
            .skip(1)
            .map(|v| v.as_str())
            .collect();
        assert_eq!(vals.len(), 2);
        assert_eq!(vals[0], "https://relay.example.com/git/abc/multi-clone");
        assert_eq!(vals[1], "ssh://git@github.com/org/multi-clone.git");
    }

    #[test]
    fn git_patch_happy_path_minimal() {
        let owner = "a".repeat(64);
        let repo = GitRepoCoord {
            owner: owner.clone(),
            id: "my-repo".to_string(),
        };
        let ev =
            sign(build_git_patch(&repo, "diff --git a/x b/x", &GitPatchMeta::default()).unwrap());
        assert_eq!(ev.kind.as_u16(), 1617);
        assert_eq!(ev.content, "diff --git a/x b/x");
        assert!(has_tag(&ev, "a", &format!("30617:{owner}:my-repo")));
        assert!(has_tag(&ev, "p", &owner));
    }

    #[test]
    fn git_patch_root_and_metadata_tags() {
        let owner = "a".repeat(64);
        let commit = "c".repeat(40);
        let parent = "d".repeat(40);
        let repo = GitRepoCoord {
            owner,
            id: "repo".to_string(),
        };
        let meta = GitPatchMeta {
            root: true,
            commit: Some(commit.clone()),
            parent_commit: Some(parent.clone()),
            ..Default::default()
        };
        let ev = sign(build_git_patch(&repo, "patch body", &meta).unwrap());
        assert!(has_tag(&ev, "t", "root"));
        assert!(has_tag(&ev, "commit", &commit));
        assert!(has_tag(&ev, "parent-commit", &parent));
        assert!(has_tag(&ev, "r", &commit));
    }

    #[test]
    fn git_patch_rejects_root_and_root_revision_together() {
        let repo = GitRepoCoord {
            owner: "a".repeat(64),
            id: "repo".to_string(),
        };
        let meta = GitPatchMeta {
            root: true,
            root_revision: true,
            ..Default::default()
        };
        let err = build_git_patch(&repo, "x", &meta).unwrap_err();
        assert!(matches!(err, SdkError::InvalidInput(_)));
    }

    #[test]
    fn git_patch_rejects_oversized_content() {
        let repo = GitRepoCoord {
            owner: "a".repeat(64),
            id: "repo".to_string(),
        };
        let big = "x".repeat(60 * 1024 + 1);
        let err = build_git_patch(&repo, &big, &GitPatchMeta::default()).unwrap_err();
        assert!(matches!(err, SdkError::ContentTooLarge { .. }));
    }

    #[test]
    fn git_patch_rejects_bad_repo_owner() {
        let repo = GitRepoCoord {
            owner: "not-hex".to_string(),
            id: "repo".to_string(),
        };
        let err = build_git_patch(&repo, "x", &GitPatchMeta::default()).unwrap_err();
        assert!(matches!(err, SdkError::InvalidInput(_)));
    }

    #[test]
    fn git_issue_happy_path() {
        let owner = "a".repeat(64);
        let repo = GitRepoCoord {
            owner: owner.clone(),
            id: "repo".to_string(),
        };
        let meta = GitIssueMeta {
            labels: vec!["bug".to_string(), "p1".to_string()],
            recipients: vec![],
        };
        let ev =
            sign(build_git_issue(&repo, "Crashes on startup", "steps to repro", &meta).unwrap());
        assert_eq!(ev.kind.as_u16(), 1621);
        assert_eq!(ev.content, "steps to repro");
        assert!(has_tag(&ev, "a", &format!("30617:{owner}:repo")));
        assert!(has_tag(&ev, "subject", "Crashes on startup"));
        assert!(has_tag(&ev, "t", "bug"));
        assert!(has_tag(&ev, "t", "p1"));
    }

    #[test]
    fn git_issue_rejects_empty_subject() {
        let repo = GitRepoCoord {
            owner: "a".repeat(64),
            id: "repo".to_string(),
        };
        let err = build_git_issue(&repo, "", "body", &GitIssueMeta::default()).unwrap_err();
        assert!(matches!(err, SdkError::InvalidInput(_)));
    }

    #[test]
    fn git_issue_assignment_happy_path() {
        let owner = "a".repeat(64);
        let repo = GitRepoCoord {
            owner: owner.clone(),
            id: "repo".to_string(),
        };
        let issue = "b".repeat(64);
        // Duplicates (case-insensitive) collapse to a single p tag.
        let assignees = vec!["C".repeat(64), "c".repeat(64), "d".repeat(64)];
        let ev = sign(
            build_git_issue_assignment(&repo, &issue, &assignees, "Assigned this issue to Thomas")
                .unwrap(),
        );
        assert_eq!(ev.kind.as_u16(), 1);
        assert_eq!(ev.content, "Assigned this issue to Thomas");
        assert!(has_tag(&ev, "e", &issue));
        assert!(has_tag(&ev, "a", &format!("30617:{owner}:repo")));
        assert!(has_tag(&ev, "p", &"c".repeat(64)));
        assert!(has_tag(&ev, "p", &"d".repeat(64)));
        assert!(has_tag(&ev, "t", "assignment"));
        let p_count = ev
            .tags
            .iter()
            .filter(|tag| tag.as_slice().first().map(String::as_str) == Some("p"))
            .count();
        assert_eq!(p_count, 2);
    }

    #[test]
    fn git_issue_assignment_rejects_bad_input() {
        let repo = GitRepoCoord {
            owner: "a".repeat(64),
            id: "repo".to_string(),
        };
        let issue = "b".repeat(64);
        // No assignees.
        let err = build_git_issue_assignment(&repo, &issue, &[], "x").unwrap_err();
        assert!(matches!(err, SdkError::InvalidInput(_)));
        // Malformed assignee pubkey.
        let err =
            build_git_issue_assignment(&repo, &issue, &["nope".to_string()], "x").unwrap_err();
        assert!(matches!(err, SdkError::InvalidInput(_)));
        // Malformed issue id.
        let err = build_git_issue_assignment(&repo, "short", &["c".repeat(64)], "x").unwrap_err();
        assert!(matches!(err, SdkError::InvalidInput(_)));
    }

    #[test]
    fn git_issue_assignment_with_prior_emits_valid_causal_tag() {
        let repo = GitRepoCoord {
            owner: "a".repeat(64),
            id: "repo".to_string(),
        };
        let issue = "b".repeat(64);
        let assignee = "c".repeat(64);
        let prior = "d".repeat(64);
        let ev = sign(
            build_git_issue_assignment_with_prior(
                &repo,
                &issue,
                &[assignee],
                "Assigned this issue",
                Some(&prior),
            )
            .unwrap(),
        );

        assert!(has_tag(&ev, "prior", &prior));
        let unassignment = sign(
            build_git_issue_unassignment_with_prior(
                &repo,
                &issue,
                &["c".repeat(64)],
                "Unassigned this issue",
                Some(&prior),
            )
            .unwrap(),
        );
        assert!(has_tag(&unassignment, "prior", &prior));
        assert!(build_git_issue_assignment_with_prior(
            &repo,
            &issue,
            &["c".repeat(64)],
            "Assigned this issue",
            Some("invalid"),
        )
        .is_err());
    }

    #[test]
    fn git_issue_unassignment_happy_path() {
        let owner = "a".repeat(64);
        let repo = GitRepoCoord {
            owner: owner.clone(),
            id: "repo".to_string(),
        };
        let issue = "b".repeat(64);
        let assignee = "c".repeat(64);
        let ev = sign(
            build_git_issue_unassignment(
                &repo,
                &issue,
                std::slice::from_ref(&assignee),
                "Unassigned Thomas from this issue",
            )
            .unwrap(),
        );
        assert_eq!(ev.kind.as_u16(), 1);
        assert_eq!(ev.content, "Unassigned Thomas from this issue");
        assert!(has_tag(&ev, "e", &issue));
        assert!(has_tag(&ev, "a", &format!("30617:{owner}:repo")));
        assert!(has_tag(&ev, "p", &assignee));
        assert!(has_tag(&ev, "t", "unassignment"));
        assert!(!has_tag(&ev, "t", "assignment"));
    }

    #[test]
    fn legacy_issue_assignment_builders_omit_prior() {
        let repo = GitRepoCoord {
            owner: "a".repeat(64),
            id: "repo".to_string(),
        };
        let issue = "b".repeat(64);
        let assignees = vec!["c".repeat(64)];
        let assignment = sign(
            build_git_issue_assignment(&repo, &issue, &assignees, "Assigned this issue").unwrap(),
        );
        let unassignment = sign(
            build_git_issue_unassignment(&repo, &issue, &assignees, "Unassigned this issue")
                .unwrap(),
        );

        assert!(!assignment
            .tags
            .iter()
            .any(|tag| { tag.as_slice().first().map(String::as_str) == Some("prior") }));
        assert!(!unassignment
            .tags
            .iter()
            .any(|tag| { tag.as_slice().first().map(String::as_str) == Some("prior") }));
    }

    #[test]
    fn git_status_open_happy_path() {
        let root = event_id().to_hex();
        let meta = GitStatusMeta {
            root_event: root.clone(),
            ..Default::default()
        };
        let ev = sign(build_git_status(GitStatus::Open, "", &meta).unwrap());
        assert_eq!(ev.kind.as_u16(), 1630);
        assert!(has_tag(&ev, "e", &root));
    }

    #[test]
    fn git_status_merged_with_applied_patches() {
        let root = event_id().to_hex();
        let patch_id = event_id().to_hex();
        let merge_commit = "f".repeat(40);
        let meta = GitStatusMeta {
            root_event: root,
            applied_patches: vec![GitAppliedPatchRef {
                id: patch_id.clone(),
                relay: None,
                pubkey: None,
            }],
            merge_commit: Some(merge_commit.clone()),
            ..Default::default()
        };
        let ev =
            sign(build_git_status(GitStatus::AppliedOrResolved, "merged, thanks!", &meta).unwrap());
        assert_eq!(ev.kind.as_u16(), 1631);
        assert!(has_tag(&ev, "q", &patch_id));
        assert!(has_tag(&ev, "merge-commit", &merge_commit));
        assert!(has_tag(&ev, "r", &merge_commit));
    }

    #[test]
    fn git_status_rejects_merge_fields_on_non_merged_status() {
        let root = event_id().to_hex();
        let meta = GitStatusMeta {
            root_event: root,
            merge_commit: Some("f".repeat(40)),
            ..Default::default()
        };
        let err = build_git_status(GitStatus::Closed, "", &meta).unwrap_err();
        assert!(matches!(err, SdkError::InvalidInput(_)));
    }

    #[test]
    fn git_status_rejects_bad_root_event() {
        let meta = GitStatusMeta {
            root_event: "not-an-event-id".to_string(),
            ..Default::default()
        };
        let err = build_git_status(GitStatus::Open, "", &meta).unwrap_err();
        assert!(matches!(err, SdkError::InvalidInput(_)));
    }

    #[test]
    fn git_patch_rejects_empty_content() {
        let repo = GitRepoCoord {
            owner: "a".repeat(64),
            id: "repo".to_string(),
        };
        let err = build_git_patch(&repo, "", &GitPatchMeta::default()).unwrap_err();
        assert!(matches!(err, SdkError::InvalidInput(_)));
    }

    #[test]
    fn git_patch_rejects_whitespace_only_content() {
        // Regression: a failed `git format-patch | bee patches send
        // --patch-file -` must not silently publish a whitespace-only
        // (i.e. unappliable) patch.
        let repo = GitRepoCoord {
            owner: "a".repeat(64),
            id: "repo".to_string(),
        };
        let err = build_git_patch(&repo, "   \n\t\n", &GitPatchMeta::default()).unwrap_err();
        assert!(matches!(err, SdkError::InvalidInput(_)));
    }

    #[test]
    fn git_repo_coord_rejects_invalid_repo_id_chars() {
        let repo = GitRepoCoord {
            owner: "a".repeat(64),
            id: "../etc/passwd".to_string(),
        };
        let err = build_git_patch(&repo, "diff", &GitPatchMeta::default()).unwrap_err();
        assert!(matches!(err, SdkError::InvalidInput(_)));
    }

    #[test]
    fn git_patch_rejects_short_commit_hex() {
        let repo = GitRepoCoord {
            owner: "a".repeat(64),
            id: "repo".to_string(),
        };
        let meta = GitPatchMeta {
            commit: Some("a".to_string()),
            ..Default::default()
        };
        let err = build_git_patch(&repo, "diff", &meta).unwrap_err();
        assert!(matches!(err, SdkError::InvalidInput(_)));
    }

    #[test]
    fn git_patch_accepts_full_sha1_and_sha256_commit_hex() {
        let repo = GitRepoCoord {
            owner: "a".repeat(64),
            id: "repo".to_string(),
        };
        let meta_sha1 = GitPatchMeta {
            commit: Some("c".repeat(40)),
            ..Default::default()
        };
        assert!(build_git_patch(&repo, "diff", &meta_sha1).is_ok());
        let meta_sha256 = GitPatchMeta {
            commit: Some("c".repeat(64)),
            ..Default::default()
        };
        assert!(build_git_patch(&repo, "diff", &meta_sha256).is_ok());
    }

    #[test]
    fn git_status_defaults_p_tag_to_repo_owner_when_repo_given() {
        // SDK-level: GitStatusMeta.recipients is the caller's responsibility
        // (the CLI defaults it), but verify the repo owner ends up p-tagged
        // when the CLI-style recipients list includes it.
        let owner = "a".repeat(64);
        let root = event_id().to_hex();
        let meta = GitStatusMeta {
            root_event: root,
            repo: Some(GitRepoCoord {
                owner: owner.clone(),
                id: "repo".to_string(),
            }),
            recipients: vec![owner.clone()],
            ..Default::default()
        };
        let ev = sign(build_git_status(GitStatus::Open, "", &meta).unwrap());
        assert!(has_tag(&ev, "p", &owner));
    }

    #[test]
    fn git_applied_patch_ref_parse_id_only() {
        let id = "a".repeat(64);
        let parsed = GitAppliedPatchRef::parse(&id).unwrap();
        assert_eq!(parsed.id, id);
        assert_eq!(parsed.relay, None);
        assert_eq!(parsed.pubkey, None);
    }

    #[test]
    fn git_applied_patch_ref_parse_id_and_relay() {
        let id = "a".repeat(64);
        let spec = format!("{id}:wss://relay.example.com");
        let parsed = GitAppliedPatchRef::parse(&spec).unwrap();
        assert_eq!(parsed.id, id);
        assert_eq!(parsed.relay, Some("wss://relay.example.com".to_string()));
        assert_eq!(parsed.pubkey, None);
    }

    #[test]
    fn git_applied_patch_ref_parse_id_relay_and_pubkey() {
        let id = "a".repeat(64);
        let pubkey = "b".repeat(64);
        let spec = format!("{id}:wss://relay.example.com:{pubkey}");
        let parsed = GitAppliedPatchRef::parse(&spec).unwrap();
        assert_eq!(parsed.id, id);
        assert_eq!(parsed.relay, Some("wss://relay.example.com".to_string()));
        assert_eq!(parsed.pubkey, Some(pubkey));
    }

    #[test]
    fn git_status_q_tag_includes_relay_and_pubkey_hints() {
        let root = event_id().to_hex();
        let patch_id = event_id().to_hex();
        let pubkey = "b".repeat(64);
        let meta = GitStatusMeta {
            root_event: root,
            applied_patches: vec![GitAppliedPatchRef {
                id: patch_id.clone(),
                relay: Some("wss://relay.example.com".to_string()),
                pubkey: Some(pubkey.clone()),
            }],
            ..Default::default()
        };
        let ev = sign(build_git_status(GitStatus::AppliedOrResolved, "", &meta).unwrap());
        let q_tag = ev
            .tags
            .iter()
            .find(|t| t.as_slice().first().map(|v| v.as_str()) == Some("q"))
            .expect("q tag present");
        let parts = q_tag.as_slice();
        assert_eq!(parts.get(1).map(|v| v.as_str()), Some(patch_id.as_str()));
        assert_eq!(
            parts.get(2).map(|v| v.as_str()),
            Some("wss://relay.example.com")
        );
        assert_eq!(parts.get(3).map(|v| v.as_str()), Some(pubkey.as_str()));
    }

    #[test]
    fn workflow_def_happy_path() {
        let cid = uuid();
        let wid = uuid();
        let ev = sign(build_workflow_def(cid, wid, "name: test\ntrigger:\n  on: webhook").unwrap());
        assert_eq!(ev.kind.as_u16(), 30620);
        assert!(has_tag(&ev, "d", &wid.to_string()));
        assert!(has_tag(&ev, "h", &cid.to_string()));
        assert!(ev.content.contains("name: test"));
    }

    #[test]
    fn workflow_def_rejects_oversized_yaml() {
        let big = "x".repeat(65 * 1024);
        let err = build_workflow_def(uuid(), uuid(), &big).unwrap_err();
        assert!(matches!(err, SdkError::ContentTooLarge { .. }));
    }

    #[test]
    fn workflow_update_includes_h_tag() {
        let cid = uuid();
        let wid = uuid();
        let revision = "a".repeat(64);
        let ev = sign(build_workflow_update(cid, wid, "name: updated", &revision).unwrap());
        assert_eq!(ev.kind.as_u16(), 30620);
        assert!(has_tag(&ev, "d", &wid.to_string()));
        assert!(has_tag(&ev, "h", &cid.to_string()));
        assert!(has_tag(&ev, "expected-revision", &revision));
    }

    #[test]
    fn workflow_update_rejects_oversized_yaml() {
        let big = "x".repeat(65 * 1024);
        let err = build_workflow_update(uuid(), uuid(), &big, &"a".repeat(64)).unwrap_err();
        assert!(matches!(err, SdkError::ContentTooLarge { .. }));
    }

    #[test]
    fn workflow_delete_happy_path() {
        let pk = "a".repeat(64);
        let wid = uuid();
        let ev = sign(build_workflow_delete(&pk, wid).unwrap());
        assert_eq!(ev.kind.as_u16(), 5);
        let a_vals = tag_values(&ev, "a");
        assert_eq!(a_vals.len(), 1);
        assert!(a_vals[0].starts_with("30620:"));
        assert!(a_vals[0].contains(&wid.to_string()));
    }

    #[test]
    fn workflow_delete_rejects_bad_pubkey() {
        let err = build_workflow_delete("bad", uuid()).unwrap_err();
        assert!(matches!(err, SdkError::InvalidInput(_)));
    }

    #[test]
    fn workflow_trigger_happy_path() {
        let wid = uuid();
        let ev = sign(build_workflow_trigger(wid).unwrap());
        assert_eq!(ev.kind.as_u16(), 46020);
        assert!(has_tag(&ev, "d", &wid.to_string()));
    }

    #[test]
    fn workflow_approval_grant() {
        let hash = "a".repeat(64);
        let ev = sign(build_workflow_approval(&hash, true, "lgtm").unwrap());
        assert_eq!(ev.kind.as_u16(), 46030);
        assert!(has_tag(&ev, "d", &hash));
        assert_eq!(ev.content, "lgtm");
    }

    #[test]
    fn workflow_approval_deny() {
        let hash = "b".repeat(64);
        let ev = sign(build_workflow_approval(&hash, false, "").unwrap());
        assert_eq!(ev.kind.as_u16(), 46031);
    }

    #[test]
    fn workflow_approval_rejects_bad_token_hash() {
        let err = build_workflow_approval("not-hex", true, "").unwrap_err();
        assert!(matches!(err, SdkError::InvalidInput(_)));
    }

    #[test]
    fn workflow_approval_rejects_short_hash() {
        let short = "a".repeat(32);
        let err = build_workflow_approval(&short, true, "").unwrap_err();
        assert!(matches!(err, SdkError::InvalidInput(_)));
    }

    #[test]
    fn dm_open_happy_path() {
        let pk = "a".repeat(64);
        let ev = sign(build_dm_open(&[&pk]).unwrap());
        assert_eq!(ev.kind.as_u16(), 41010);
        assert!(has_tag(&ev, "p", &pk));
    }

    #[test]
    fn dm_open_rejects_empty_pubkeys() {
        let err = build_dm_open(&[]).unwrap_err();
        assert!(matches!(err, SdkError::InvalidInput(_)));
    }

    #[test]
    fn dm_open_rejects_over_8_pubkeys() {
        let pk = "a".repeat(64);
        let pks: Vec<&str> = vec![pk.as_str(); 9];
        let err = build_dm_open(&pks).unwrap_err();
        assert!(matches!(err, SdkError::InvalidInput(_)));
    }

    #[test]
    fn dm_open_rejects_bad_pubkey() {
        let err = build_dm_open(&["bad-hex"]).unwrap_err();
        assert!(matches!(err, SdkError::InvalidInput(_)));
    }

    #[test]
    fn dm_add_member_happy_path() {
        let cid = uuid();
        let pk = "b".repeat(64);
        let ev = sign(build_dm_add_member(cid, &pk).unwrap());
        assert_eq!(ev.kind.as_u16(), 41011);
        assert!(has_tag(&ev, "h", &cid.to_string()));
        assert!(has_tag(&ev, "p", &pk));
    }

    #[test]
    fn dm_add_member_rejects_bad_pubkey() {
        let err = build_dm_add_member(uuid(), "short").unwrap_err();
        assert!(matches!(err, SdkError::InvalidInput(_)));
    }

    #[test]
    fn presence_update_content_is_status() {
        let ev = sign(build_presence_update("online").unwrap());
        assert_eq!(ev.kind.as_u16(), 20001);
        assert_eq!(ev.content, "online");
        assert!(has_tag(&ev, "status", "online"));
    }

    #[test]
    fn presence_update_away() {
        let ev = sign(build_presence_update("away").unwrap());
        assert_eq!(ev.content, "away");
    }

    #[test]
    fn presence_update_offline() {
        let ev = sign(build_presence_update("offline").unwrap());
        assert_eq!(ev.content, "offline");
    }

    #[test]
    fn presence_update_rejects_invalid_status() {
        let err = build_presence_update("dnd").unwrap_err();
        assert!(matches!(err, SdkError::InvalidInput(_)));
    }

    // ── build_user_status ─────────────────────────────────────────────────────

    #[test]
    fn user_status_carries_text_and_emoji_on_d_general() {
        let ev = sign(build_user_status("shipping the CLI", Some("🚀")).unwrap());
        assert_eq!(ev.kind.as_u16(), 30315);
        assert_eq!(ev.content, "shipping the CLI");
        assert_eq!(tag_values(&ev, "d"), vec!["general"]);
        assert_eq!(tag_values(&ev, "emoji"), vec!["🚀"]);
    }

    #[test]
    fn user_status_trims_text_and_emoji() {
        let ev = sign(build_user_status("  heads down  ", Some("  🎧 ")).unwrap());
        assert_eq!(ev.content, "heads down");
        assert_eq!(tag_values(&ev, "emoji"), vec!["🎧"]);
    }

    #[test]
    fn user_status_omits_blank_emoji_tag() {
        let ev = sign(build_user_status("on call", Some("   ")).unwrap());
        assert_eq!(ev.content, "on call");
        assert!(tag_values(&ev, "emoji").is_empty());
    }

    #[test]
    fn user_status_keeps_emoji_when_text_is_blank() {
        let ev = sign(build_user_status("", Some("🎶")).unwrap());
        assert_eq!(ev.content, "");
        assert_eq!(tag_values(&ev, "emoji"), vec!["🎶"]);
    }

    #[test]
    fn user_status_clear_shape_is_empty_content_and_d_tag_only() {
        let ev = sign(build_user_status("", None).unwrap());
        assert_eq!(ev.kind.as_u16(), 30315);
        assert_eq!(ev.content, "");
        assert_eq!(tag_values(&ev, "d"), vec!["general"]);
        assert_eq!(ev.tags.len(), 1);
    }

    #[test]
    fn user_status_rejects_oversize_text() {
        let err = build_user_status(&"x".repeat(64 * 1024 + 1), None).unwrap_err();
        assert!(matches!(err, SdkError::ContentTooLarge { .. }));
    }

    // ── build_git_pull_request / build_git_pr_update ──────────────────────────

    fn pr_repo() -> GitRepoCoord {
        GitRepoCoord {
            owner: "a".repeat(64),
            id: "repo".to_string(),
        }
    }

    fn full_clone_tag(event: &nostr::Event) -> Vec<String> {
        event
            .tags
            .iter()
            .find(|t| t.as_slice().first().map(|v| v.as_str()) == Some("clone"))
            .map(|t| t.as_slice()[1..].iter().map(|v| v.to_string()).collect())
            .unwrap_or_default()
    }

    #[test]
    fn git_pr_happy_path() {
        let meta = GitPullRequestMeta {
            subject: "Add feature X".to_string(),
            commit: "c".repeat(40),
            clone_urls: vec!["https://example.com/repo.git".to_string()],
            branch_name: Some("feat/x".to_string()),
            labels: vec!["enhancement".to_string()],
            channel_id: Some("11111111-1111-4111-8111-111111111111".to_string()),
            ..Default::default()
        };
        let ev = sign(build_git_pull_request(&pr_repo(), "PR body", &meta).unwrap());
        assert_eq!(ev.kind.as_u16(), 1618);
        assert_eq!(ev.content, "PR body");
        assert!(has_tag(&ev, "a", &format!("30617:{}:repo", "a".repeat(64))));
        assert!(has_tag(&ev, "p", &"a".repeat(64)));
        assert!(has_tag(&ev, "subject", "Add feature X"));
        assert!(has_tag(&ev, "c", &"c".repeat(40)));
        assert!(has_tag(&ev, "t", "enhancement"));
        assert!(has_tag(&ev, "h", "11111111-1111-4111-8111-111111111111"));
        assert!(has_tag(&ev, "branch-name", "feat/x"));
        assert_eq!(
            full_clone_tag(&ev),
            vec!["https://example.com/repo.git".to_string()]
        );
    }

    #[test]
    fn git_pr_emits_multi_url_clone_tag() {
        let meta = GitPullRequestMeta {
            subject: "s".to_string(),
            commit: "c".repeat(40),
            clone_urls: vec![
                "https://a.example/repo.git".to_string(),
                "https://b.example/repo.git".to_string(),
            ],
            ..Default::default()
        };
        let ev = sign(build_git_pull_request(&pr_repo(), "", &meta).unwrap());
        assert_eq!(full_clone_tag(&ev).len(), 2);
    }

    #[test]
    fn git_pr_rejects_empty_subject() {
        let meta = GitPullRequestMeta {
            subject: String::new(),
            commit: "c".repeat(40),
            clone_urls: vec!["https://example.com/repo.git".to_string()],
            ..Default::default()
        };
        let err = build_git_pull_request(&pr_repo(), "body", &meta).unwrap_err();
        assert!(matches!(err, SdkError::InvalidInput(_)));
    }

    #[test]
    fn git_pr_rejects_missing_clone_url() {
        let meta = GitPullRequestMeta {
            subject: "s".to_string(),
            commit: "c".repeat(40),
            clone_urls: vec![],
            ..Default::default()
        };
        let err = build_git_pull_request(&pr_repo(), "body", &meta).unwrap_err();
        assert!(matches!(err, SdkError::InvalidInput(_)));
    }

    #[test]
    fn git_pr_rejects_short_commit() {
        let meta = GitPullRequestMeta {
            subject: "s".to_string(),
            commit: "abc".to_string(),
            clone_urls: vec!["https://example.com/repo.git".to_string()],
            ..Default::default()
        };
        let err = build_git_pull_request(&pr_repo(), "body", &meta).unwrap_err();
        assert!(matches!(err, SdkError::InvalidInput(_)));
    }

    #[test]
    fn git_pr_rejects_invalid_channel_id() {
        for channel_id in ["not-a-uuid", " 11111111-1111-4111-8111-111111111111 "] {
            let meta = GitPullRequestMeta {
                subject: "s".to_string(),
                commit: "c".repeat(40),
                clone_urls: vec!["https://example.com/repo.git".to_string()],
                channel_id: Some(channel_id.to_string()),
                ..Default::default()
            };
            let err = build_git_pull_request(&pr_repo(), "body", &meta).unwrap_err();
            assert!(matches!(err, SdkError::InvalidInput(_)));
        }
    }

    #[test]
    fn git_pr_canonicalizes_channel_id() {
        let meta = GitPullRequestMeta {
            subject: "s".to_string(),
            commit: "c".repeat(40),
            clone_urls: vec!["https://example.com/repo.git".to_string()],
            channel_id: Some("AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE".to_string()),
            ..Default::default()
        };
        let ev = sign(build_git_pull_request(&pr_repo(), "body", &meta).unwrap());
        assert!(has_tag(&ev, "h", "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee"));
    }

    #[test]
    fn git_pr_revision_of_emits_e_tag() {
        let patch = event_id().to_hex();
        let meta = GitPullRequestMeta {
            subject: "s".to_string(),
            commit: "c".repeat(40),
            clone_urls: vec!["https://example.com/repo.git".to_string()],
            revision_of: Some(patch.clone()),
            ..Default::default()
        };
        let ev = sign(build_git_pull_request(&pr_repo(), "", &meta).unwrap());
        assert!(has_tag(&ev, "e", &patch));
    }

    #[test]
    fn git_pr_update_happy_path() {
        let pr = event_id().to_hex();
        let meta = GitPrUpdateMeta {
            pr_event: pr.clone(),
            pr_author: "b".repeat(64),
            commit: "d".repeat(40),
            clone_urls: vec!["https://example.com/repo.git".to_string()],
            merge_base: Some("e".repeat(40)),
            ..Default::default()
        };
        let ev = sign(build_git_pr_update(&pr_repo(), "rebased", &meta).unwrap());
        assert_eq!(ev.kind.as_u16(), 1619);
        assert!(has_tag(&ev, "E", &pr));
        assert!(has_tag(&ev, "P", &"b".repeat(64)));
        assert!(has_tag(&ev, "c", &"d".repeat(40)));
        assert!(has_tag(&ev, "merge-base", &"e".repeat(40)));
        assert_eq!(full_clone_tag(&ev).len(), 1);
    }

    #[test]
    fn git_pr_update_rejects_bad_pr_event() {
        let meta = GitPrUpdateMeta {
            pr_event: "not-hex".to_string(),
            pr_author: "b".repeat(64),
            commit: "d".repeat(40),
            clone_urls: vec!["https://example.com/repo.git".to_string()],
            ..Default::default()
        };
        let err = build_git_pr_update(&pr_repo(), "", &meta).unwrap_err();
        assert!(matches!(err, SdkError::InvalidInput(_)));
    }

    #[test]
    fn git_pr_update_rejects_missing_clone_url() {
        let meta = GitPrUpdateMeta {
            pr_event: event_id().to_hex(),
            pr_author: "b".repeat(64),
            commit: "d".repeat(40),
            clone_urls: vec![],
            ..Default::default()
        };
        let err = build_git_pr_update(&pr_repo(), "", &meta).unwrap_err();
        assert!(matches!(err, SdkError::InvalidInput(_)));
    }

    // --- community moderation commands (9040–9044) ------------------------

    #[test]
    fn moderation_ban_permanent() {
        let pk = "a".repeat(64);
        let ev = sign(build_moderation_ban(&pk, None, None).unwrap());
        assert_eq!(ev.kind.as_u16(), KIND_MODERATION_BAN as u16);
        assert!(has_tag(&ev, "p", &pk));
        assert!(tag_values(&ev, "expiration").is_empty());
        assert!(tag_values(&ev, "reason").is_empty());
    }

    #[test]
    fn moderation_ban_temporary_with_reason() {
        let pk = "b".repeat(64);
        let ev = sign(build_moderation_ban(&pk, Some(1783500000), Some("spam")).unwrap());
        assert_eq!(ev.kind.as_u16(), KIND_MODERATION_BAN as u16);
        assert!(has_tag(&ev, "p", &pk));
        assert!(has_tag(&ev, "expiration", "1783500000"));
        assert!(has_tag(&ev, "reason", "spam"));
    }

    #[test]
    fn moderation_ban_lowercases_pubkey() {
        let ev = sign(build_moderation_ban(&"A".repeat(64), None, None).unwrap());
        assert!(has_tag(&ev, "p", &"a".repeat(64)));
    }

    #[test]
    fn moderation_ban_rejects_short_pubkey() {
        let err = build_moderation_ban("abc", None, None).unwrap_err();
        assert!(matches!(err, SdkError::InvalidInput(_)));
    }

    #[test]
    fn moderation_ban_rejects_overlong_pubkey() {
        // Relay `extract_p_tag_bytes` requires exactly 64 hex; the SDK must
        // reject 65+ hex here rather than sign a `p` tag the relay drops.
        let err = build_moderation_ban(&"a".repeat(65), None, None).unwrap_err();
        assert!(matches!(err, SdkError::InvalidInput(_)));
    }

    #[test]
    fn moderation_unban_shape() {
        let pk = "c".repeat(64);
        let ev = sign(build_moderation_unban(&pk).unwrap());
        assert_eq!(ev.kind.as_u16(), KIND_MODERATION_UNBAN as u16);
        assert!(has_tag(&ev, "p", &pk));
    }

    #[test]
    fn moderation_timeout_shape() {
        let pk = "d".repeat(64);
        let ev = sign(build_moderation_timeout(&pk, 1783500000, Some("cool off")).unwrap());
        assert_eq!(ev.kind.as_u16(), KIND_MODERATION_TIMEOUT as u16);
        assert!(has_tag(&ev, "p", &pk));
        assert!(has_tag(&ev, "expiration", "1783500000"));
        assert!(has_tag(&ev, "reason", "cool off"));
    }

    #[test]
    fn moderation_untimeout_shape() {
        let pk = "e".repeat(64);
        let ev = sign(build_moderation_untimeout(&pk).unwrap());
        assert_eq!(ev.kind.as_u16(), KIND_MODERATION_UNTIMEOUT as u16);
        assert!(has_tag(&ev, "p", &pk));
    }

    #[test]
    fn moderation_resolve_shape() {
        let rid = event_id().to_hex();
        let ev =
            sign(build_moderation_resolve_report(&rid, "resolved", "ban", Some("rule 3")).unwrap());
        assert_eq!(ev.kind.as_u16(), KIND_MODERATION_RESOLVE_REPORT as u16);
        assert!(has_tag(&ev, "report", &rid));
        assert!(has_tag(&ev, "status", "resolved"));
        assert!(has_tag(&ev, "action", "ban"));
        assert!(has_tag(&ev, "reason", "rule 3"));
    }

    #[test]
    fn moderation_resolve_dismiss_no_reason() {
        let rid = event_id().to_hex();
        let ev = sign(build_moderation_resolve_report(&rid, "dismissed", "dismiss", None).unwrap());
        assert!(has_tag(&ev, "status", "dismissed"));
        assert!(has_tag(&ev, "action", "dismiss"));
        assert!(tag_values(&ev, "reason").is_empty());
    }

    #[test]
    fn moderation_resolve_rejects_bad_status() {
        let rid = event_id().to_hex();
        let err = build_moderation_resolve_report(&rid, "escalated", "ban", None).unwrap_err();
        assert!(matches!(err, SdkError::InvalidInput(_)));
    }

    #[test]
    fn moderation_resolve_rejects_bad_action() {
        let rid = event_id().to_hex();
        let err = build_moderation_resolve_report(&rid, "resolved", "nuke", None).unwrap_err();
        assert!(matches!(err, SdkError::InvalidInput(_)));
    }

    #[test]
    fn moderation_resolve_rejects_short_report_id() {
        let err = build_moderation_resolve_report("abc", "resolved", "ban", None).unwrap_err();
        assert!(matches!(err, SdkError::InvalidInput(_)));
    }

    #[test]
    fn moderation_resolve_rejects_overlong_report_id() {
        // Relay `extract_report_tag` requires exactly 64 hex; the SDK must
        // reject 65+ hex here rather than sign a `report` tag the relay drops.
        let err =
            build_moderation_resolve_report(&"a".repeat(65), "resolved", "ban", None).unwrap_err();
        assert!(matches!(err, SdkError::InvalidInput(_)));
    }

    // ── NIP-IA identity archival (kinds 9035/9036) ────────────────────────
    //
    // Mirrors `desktop/src-tauri/src/events.rs`'s own tests so both clients
    // are pinned to the same wire form. See NIP-IA.md §Vector 1.

    const IA_OWNER_HEX: &str = "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";
    const IA_TARGET_HEX: &str = "c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5";
    const IA_CONDITIONS: &str = "kind=1&created_at<1713957000";
    const IA_SIG: &str = "8b7df2575caf0a108374f8471722b233c53f9ff827a8b0f91861966c3b9dd5cb2e189eae9f49d72187674c2f5bd244145e10ff86c9f257ffe65a1ee5f108b369";

    #[test]
    fn archive_identity_request_matches_spec_vector_1_layout() {
        let auth: [String; 4] = [
            "auth".into(),
            IA_OWNER_HEX.into(),
            IA_CONDITIONS.into(),
            IA_SIG.into(),
        ];
        let ev = sign(
            build_archive_identity_request(
                IA_TARGET_HEX,
                "Archiving zombie agent after rebuild.",
                Some("bot-rebuilt"),
                None,
                Some(&auth),
            )
            .unwrap(),
        );

        let tags: Vec<Vec<String>> = ev.tags.iter().map(|t| t.as_slice().to_vec()).collect();
        assert_eq!(ev.kind, Kind::Custom(KIND_IA_ARCHIVE_REQUEST as u16));
        // Spec layout: ["-"], ["p", target], ["reason", code], ["auth", ...]
        assert_eq!(tags[0], vec!["-"]);
        assert_eq!(tags[1], vec!["p", IA_TARGET_HEX]);
        assert_eq!(tags[2], vec!["reason", "bot-rebuilt"]);
        assert_eq!(tags[3], vec!["auth", IA_OWNER_HEX, IA_CONDITIONS, IA_SIG]);
        assert_eq!(tags.len(), 4);
    }

    #[test]
    fn archive_request_rejects_replaced_by_equal_target() {
        let err =
            build_archive_identity_request(IA_TARGET_HEX, "", None, Some(IA_TARGET_HEX), None)
                .unwrap_err();
        assert!(matches!(err, SdkError::InvalidInput(_)));
    }

    #[test]
    fn unarchive_request_layout_self_path() {
        // Self-unarchive: actor == target, so the `p` tag points at the
        // signer. Pins that `.allow_self_tagging()` survives nostr 0.44's
        // default same-pubkey `p`-tag scrub, and that no `auth` tag rides
        // along on the self path.
        let builder = build_unarchive_identity_request(
            IA_TARGET_HEX,
            "I am active again.",
            Some("returned"),
            None,
        )
        .unwrap();
        let target_secret = nostr::SecretKey::from_hex(
            "0000000000000000000000000000000000000000000000000000000000000002",
        )
        .unwrap();
        let ev = builder
            .sign_with_keys(&Keys::new(target_secret))
            .expect("sign");
        let tags: Vec<Vec<String>> = ev.tags.iter().map(|t| t.as_slice().to_vec()).collect();
        assert_eq!(ev.kind, Kind::Custom(KIND_IA_UNARCHIVE_REQUEST as u16));
        assert_eq!(tags[0], vec!["-"]);
        assert_eq!(tags[1], vec!["p", IA_TARGET_HEX]);
        assert_eq!(tags[2], vec!["reason", "returned"]);
        assert_eq!(tags.len(), 3, "self unarchive must not carry an auth tag");
        assert_eq!(ev.pubkey.to_hex(), IA_TARGET_HEX);
    }

    #[test]
    fn identity_archive_reason_accepts_64_bytes() {
        // check_reason compares `.len()` (bytes), not chars — use a
        // multi-byte char so a chars-based off-by-one would slip through.
        let reason: String = "é".repeat(32); // 32 * 2 bytes = 64 bytes exactly
        assert_eq!(reason.len(), 64);
        build_archive_identity_request(IA_TARGET_HEX, "", Some(&reason), None, None)
            .expect("64-byte reason must be accepted");
    }

    #[test]
    fn identity_archive_reason_rejects_65_bytes() {
        let reason: String = "a".repeat(65);
        let err = build_archive_identity_request(IA_TARGET_HEX, "", Some(&reason), None, None)
            .unwrap_err();
        assert!(matches!(err, SdkError::InvalidInput(_)));
    }

    #[test]
    fn identity_archive_reason_rejects_control_chars() {
        let err = build_unarchive_identity_request(IA_TARGET_HEX, "", Some("bad\nreason"), None)
            .unwrap_err();
        assert!(matches!(err, SdkError::InvalidInput(_)));
    }

    #[test]
    fn identity_archive_rejects_malformed_auth_tag() {
        let bad_auth: [String; 4] = [
            "auth".into(),
            IA_OWNER_HEX.into(),
            IA_CONDITIONS.into(),
            "not-hex".into(),
        ];
        let err = build_archive_identity_request(IA_TARGET_HEX, "", None, None, Some(&bad_auth))
            .unwrap_err();
        assert!(matches!(err, SdkError::InvalidInput(_)));
    }

    #[test]
    fn unarchive_request_has_no_replaced_by_param() {
        // 9036 has no `replaced_by` parameter at all (unlike 9035) — this is
        // a compile-time guarantee, but pin the tag count so a future
        // signature change that reintroduces it doesn't silently widen the
        // wire form without a test failing.
        let ev = sign(
            build_unarchive_identity_request(IA_TARGET_HEX, "", Some("returned"), None).unwrap(),
        );
        assert!(!ev
            .tags
            .iter()
            .any(|t| t.as_slice().first().map(String::as_str) == Some("replaced-by")));
    }

    // ── NIP-MP cap-before-arity ordering ─────────────────────────────────────

    /// When an envelope exceeds the member cap AND contains a malformed `a` tag,
    /// the validator must fire `member-cap` (rule 3) — not `member-tag-arity`
    /// (rule 4).  This matches the relay's ingest ordering and means a client
    /// sending an oversized list never receives a per-tag parse error.
    #[test]
    fn validate_project_envelope_cap_wins_over_arity_when_both_fail() {
        let owner = "a".repeat(64);
        // Build 65 well-formed `a` tags — enough to trigger the cap.
        let mut tags = vec![Tag::parse(["d", "platform"]).unwrap()];
        for i in 0..65usize {
            let coord = format!("30617:{owner}:repo-{i}");
            tags.push(Tag::parse(["a", &coord]).unwrap());
        }
        // Also add one malformed tag (four elements) that would fire
        // member-tag-arity if evaluated before the cap check.
        let coord_extra = format!("30617:{owner}:repo-extra");
        tags.push(
            Tag::parse([
                "a",
                &coord_extra,
                "wss://relay.example.com",
                "extra-element",
            ])
            .unwrap(),
        );

        let err = validate_project_envelope(&tags, "").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("member-cap"),
            "expected member-cap to win, got: {msg}"
        );
        assert!(
            !msg.contains("member-tag-arity"),
            "arity rule must not fire before cap rule, got: {msg}"
        );
    }

    // ── Layer B writer-policy builder ───────────────────────────────────────

    const OWNER64: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const VALID_UUID: &str = "3580ca9b-47b4-4af9-b22a-1068778f26c6";

    fn member_coord(repo: &str) -> ProjectMemberCoord {
        ProjectMemberCoord::parse_full(&format!("30617:{OWNER64}:{repo}")).unwrap()
    }

    #[test]
    fn build_project_emitted_envelope_has_correct_shape() {
        // slug, name, description, channel, visibility, and one member.
        let m = member_coord("buzz");
        let ev = sign(
            build_project(
                "my-proj",
                Some("My Project"),
                Some("A description"),
                &[m],
                Some(VALID_UUID),
                Some("listed"),
            )
            .expect("Layer B must accept valid inputs"),
        );

        // Kind must be 30621.
        assert_eq!(ev.kind.as_u16(), KIND_PROJECT as u16);
        // Content must be empty (Layer B policy).
        assert!(ev.content.is_empty(), "content must be empty");

        let all_tags: Vec<Vec<String>> = ev.tags.iter().map(|t| t.as_slice().to_vec()).collect();

        // d tag must be present exactly once.
        let d_tags: Vec<_> = all_tags.iter().filter(|t| t[0] == "d").collect();
        assert_eq!(d_tags.len(), 1);
        assert_eq!(d_tags[0][1], "my-proj");

        // name, description, buzz-channel, buzz-visibility present.
        let name_tags: Vec<_> = all_tags.iter().filter(|t| t[0] == "name").collect();
        assert_eq!(name_tags.len(), 1);
        assert_eq!(name_tags[0][1], "My Project");

        let desc_tags: Vec<_> = all_tags.iter().filter(|t| t[0] == "description").collect();
        assert_eq!(desc_tags.len(), 1);
        assert_eq!(desc_tags[0][1], "A description");

        let ch_tags: Vec<_> = all_tags.iter().filter(|t| t[0] == "buzz-channel").collect();
        assert_eq!(ch_tags.len(), 1);
        assert_eq!(ch_tags[0][1], VALID_UUID);

        let vis_tags: Vec<_> = all_tags
            .iter()
            .filter(|t| t[0] == "buzz-visibility")
            .collect();
        assert_eq!(vis_tags.len(), 1);
        assert_eq!(vis_tags[0][1], "listed");

        // member a tag.
        let a_tags: Vec<_> = all_tags.iter().filter(|t| t[0] == "a").collect();
        assert_eq!(a_tags.len(), 1);
        assert_eq!(a_tags[0][1], format!("30617:{OWNER64}:buzz"));
    }

    #[test]
    fn build_project_optional_fields_absent_when_not_supplied() {
        let m = member_coord("core");
        let ev = sign(
            build_project("my-proj", None, None, &[m], None, None)
                .expect("minimal build must succeed"),
        );
        let names: Vec<_> = ev
            .tags
            .iter()
            .filter(|t| t.as_slice().first().map(|s| s.as_str()) == Some("name"))
            .collect();
        assert!(names.is_empty(), "name tag must not be emitted when absent");
    }

    #[test]
    fn build_project_rejects_empty_slug() {
        let m = member_coord("r");
        let err = build_project("", None, None, &[m], None, None).unwrap_err();
        assert!(
            matches!(err, SdkError::InvalidInput(_)),
            "empty slug must be InvalidInput, got: {err:?}"
        );
        assert!(err.to_string().contains("empty"));
    }

    #[test]
    fn build_project_rejects_overlong_slug() {
        let long_slug = "a".repeat(PROJECT_D_MAX_LEN + 1);
        let m = member_coord("r");
        let err = build_project(&long_slug, None, None, &[m], None, None).unwrap_err();
        assert!(matches!(err, SdkError::InvalidInput(_)));
    }

    #[test]
    fn build_project_rejects_invalid_channel_uuid() {
        let m = member_coord("r");
        let err = build_project("slug", None, None, &[m], Some("not-a-uuid"), None).unwrap_err();
        assert!(matches!(err, SdkError::InvalidInput(_)));
        assert!(err.to_string().contains("UUID") || err.to_string().contains("uuid"));
    }

    #[test]
    fn build_project_rejects_invalid_visibility_token() {
        let m = member_coord("r");
        let err = build_project("slug", None, None, &[m], None, Some("chartreuse")).unwrap_err();
        assert!(matches!(err, SdkError::InvalidInput(_)));
        assert!(err.to_string().contains("listed") || err.to_string().contains("unlisted"));
    }

    #[test]
    fn build_project_rejects_over_cap_members() {
        let members: Vec<_> = (0..=PROJECT_MEMBER_CAP)
            .map(|i| member_coord(&format!("repo-{i}")))
            .collect();
        let err = build_project("slug", None, None, &members, None, None).unwrap_err();
        assert!(matches!(err, SdkError::InvalidInput(_)));
        assert!(
            err.to_string().contains("member-cap"),
            "over-cap must report member-cap, got: {err}"
        );
    }

    #[test]
    fn build_project_rejects_duplicate_members() {
        let m = member_coord("same");
        let err = build_project("slug", None, None, &[m.clone(), m], None, None).unwrap_err();
        assert!(matches!(err, SdkError::InvalidInput(_)));
        assert!(
            err.to_string().contains("dedup") || err.to_string().contains("duplicate"),
            "duplicate member must report dedup, got: {err}"
        );
    }

    #[test]
    fn build_project_content_is_always_empty() {
        // build_project forces content="" regardless; Layer A also enforces
        // that the envelope is valid. Any non-empty content would be dropped.
        // This test pins the Layer B content-forced-empty policy.
        let m = member_coord("r");
        let ev = sign(build_project("slug", None, None, &[m], None, None).unwrap());
        assert!(
            ev.content.is_empty(),
            "Layer B must always emit empty content"
        );
    }

    // ── NIP-MP conformance fixtures ──────────────────────────────────────────
    // `build_project_with_tags` directly.  Accept cases must build; reject
    // cases must fail with an error message containing the expected rule name.
    // A count assertion guards against silent omissions.
    //
    // `include_str!` path is relative to this source file.
    fn nip_mp_fixture_tags(json_tags: &serde_json::Value) -> Vec<Tag> {
        json_tags
            .as_array()
            .unwrap()
            .iter()
            .map(|t| {
                let parts: Vec<String> = t
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_str().unwrap().to_string())
                    .collect();
                let parts_ref: Vec<&str> = parts.iter().map(String::as_str).collect();
                Tag::parse(parts_ref.iter().copied())
                    .unwrap_or_else(|e| panic!("fixture tag parse error: {e}\n  raw: {t}"))
            })
            .collect()
    }

    #[test]
    fn nip_mp_fixtures_all_cases_exercised() {
        const FIXTURE_JSON: &str = include_str!("../../../docs/nips/NIP-MP.fixtures.json");

        let data: serde_json::Value =
            serde_json::from_str(FIXTURE_JSON).expect("fixture JSON must parse");
        let cases = data["cases"].as_array().expect("cases must be array");

        // Count gate (tamper detection): if NIP-MP.fixtures.json gains or
        // loses cases, this assert must be updated in the same change —
        // otherwise new fixtures would silently sit unexercised by the
        // accept/reject loop below. Keep this an exact-count assert.
        assert_eq!(
            cases.len(),
            45,
            "expected 45 fixture cases, got {} — was NIP-MP.fixtures.json edited?",
            cases.len()
        );

        let mut accept_count = 0usize;
        let mut reject_count = 0usize;

        for case in cases {
            let name = case["name"].as_str().unwrap();
            let expect = case["expect"].as_str().unwrap();
            let template = &case["template"];
            let content = template["content"].as_str().unwrap_or("");
            let tags = nip_mp_fixture_tags(&template["tags"]);

            match expect {
                "accept" => {
                    build_project_with_tags(content, tags).unwrap_or_else(|e| {
                        panic!("fixture '{name}' (accept) must build successfully, got: {e}")
                    });
                    accept_count += 1;
                }
                "reject" => {
                    let reject_rules = case["reject_rules"]
                        .as_array()
                        .expect("reject case must have reject_rules")
                        .iter()
                        .map(|r| r.as_str().unwrap().to_string())
                        .collect::<Vec<_>>();

                    let err = build_project_with_tags(content, tags).unwrap_err();
                    let err_msg = err.to_string();

                    // The error must mention at least one of the expected rules.
                    let rule_matched = reject_rules.iter().any(|r| err_msg.contains(r.as_str()));
                    assert!(
                        rule_matched,
                        "fixture '{name}' rejected with wrong rule.\n  expected one of: {reject_rules:?}\n  got error: {err_msg}"
                    );
                    reject_count += 1;
                }
                other => panic!("fixture '{name}' has unknown expect value: {other:?}"),
            }
        }

        assert_eq!(accept_count, 16, "expected 16 accept cases");
        assert_eq!(reject_count, 29, "expected 29 reject cases");
    }

    // ---- Project Pulse (44240) ---------------------------------------------

    const PULSE_COORD: &str =
        "30621:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa:platform";
    const PULSE_SESSION_REF: &str = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";

    fn pulse_entry() -> buzz_core::pulse::PulseEntry {
        buzz_core::pulse::PulseEntry {
            schema: buzz_core::pulse::PULSE_ENTRY_SCHEMA.to_owned(),
            entry_type: buzz_core::pulse::PulseEntryType::Plan,
            text: "Refactoring session creation in buzz-acp.".to_owned(),
            code_areas: vec!["crates/buzz-acp/src/pool.rs".to_owned()],
            branch: None,
            supersedes: None,
            cost: None,
        }
    }

    #[test]
    fn pulse_entry_emits_the_canonical_tag_order() {
        let channel = uuid();
        let mut entry = pulse_entry();
        entry.branch = Some("wip/project-pulse".to_owned());
        let event = sign(
            build_pulse_entry(PULSE_COORD, &entry, Some(channel), Some(PULSE_SESSION_REF))
                .expect("build"),
        );
        assert_eq!(event.kind.as_u16(), 44240);
        assert_eq!(
            ordered_tags(&event),
            vec![
                ("a".to_string(), PULSE_COORD.to_string()),
                ("pu-v".to_string(), "pu1-1".to_string()),
                ("pu-type".to_string(), "plan".to_string()),
                ("h".to_string(), channel.to_string()),
                ("branch".to_string(), "wip/project-pulse".to_string()),
                ("pu-session".to_string(), PULSE_SESSION_REF.to_string()),
            ]
        );
    }

    #[test]
    fn pulse_entry_omits_every_absent_optional_tag() {
        let event =
            sign(build_pulse_entry(PULSE_COORD, &pulse_entry(), None, None).expect("build"));
        assert_eq!(
            ordered_tags(&event),
            vec![
                ("a".to_string(), PULSE_COORD.to_string()),
                ("pu-v".to_string(), "pu1-1".to_string()),
                ("pu-type".to_string(), "plan".to_string()),
            ]
        );
        // The payload always spells out every optional key, so a consumer never
        // has to tell "absent" from "null".
        let decoded: serde_json::Value = serde_json::from_str(&event.content).expect("json");
        assert_eq!(decoded["branch"], serde_json::Value::Null);
        assert_eq!(decoded["supersedes"], serde_json::Value::Null);
        assert_eq!(decoded["codeAreas"][0], "crates/buzz-acp/src/pool.rs");
    }

    #[test]
    fn pulse_entry_branch_tag_is_derived_from_the_payload() {
        let mut entry = pulse_entry();
        entry.branch = Some("wip/project-pulse".to_owned());
        let event = sign(build_pulse_entry(PULSE_COORD, &entry, None, None).expect("build"));
        assert_eq!(
            tag_values(&event, "branch"),
            vec!["wip/project-pulse".to_string()]
        );
    }

    /// Every rejection below is buzz-core's, reached through the builder — the
    /// point of the test is that the builder owns no second copy of the rules.
    #[test]
    fn pulse_entry_delegates_every_rejection_to_buzz_core() {
        // Non-canonical coordinate (upper-case owner hex).
        let upper = format!(
            "30621:{}:platform",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_uppercase()
        );
        assert!(matches!(
            build_pulse_entry(&upper, &pulse_entry(), None, None),
            Err(SdkError::InvalidInput(_))
        ));

        // Absolute code area.
        let mut absolute = pulse_entry();
        absolute.code_areas = vec!["/etc/passwd".to_owned()];
        assert!(matches!(
            build_pulse_entry(PULSE_COORD, &absolute, None, None),
            Err(SdkError::InvalidInput(_))
        ));

        // Parent traversal.
        let mut traversal = pulse_entry();
        traversal.code_areas = vec!["crates/../etc".to_owned()];
        assert!(matches!(
            build_pulse_entry(PULSE_COORD, &traversal, None, None),
            Err(SdkError::InvalidInput(_))
        ));

        // Empty prose.
        let mut blank = pulse_entry();
        blank.text = "   ".to_owned();
        assert!(matches!(
            build_pulse_entry(PULSE_COORD, &blank, None, None),
            Err(SdkError::InvalidInput(_))
        ));

        // Over the text cap.
        let mut long = pulse_entry();
        long.text = "a".repeat(buzz_core::pulse::MAX_PULSE_TEXT_BYTES + 1);
        assert!(matches!(
            build_pulse_entry(PULSE_COORD, &long, None, None),
            Err(SdkError::InvalidInput(_))
        ));

        // Wrong schema.
        let mut schema = pulse_entry();
        schema.schema = "buzz-pulse-entry/v2".to_owned();
        assert!(matches!(
            build_pulse_entry(PULSE_COORD, &schema, None, None),
            Err(SdkError::InvalidInput(_))
        ));

        // `pu-session` that is not a canonical UUID.
        assert!(matches!(
            build_pulse_entry(PULSE_COORD, &pulse_entry(), None, Some("not-a-uuid")),
            Err(SdkError::InvalidInput(_))
        ));

        // `supersedes` that is not a 64-char lowercase hex id.
        let mut supersedes = pulse_entry();
        supersedes.supersedes = Some("NOTHEX".to_owned());
        assert!(matches!(
            build_pulse_entry(PULSE_COORD, &supersedes, None, None),
            Err(SdkError::InvalidInput(_))
        ));
    }

    #[test]
    fn pulse_entry_accepts_a_syntactically_valid_unknown_supersedes() {
        let mut entry = pulse_entry();
        entry.supersedes = Some("ff".repeat(32));
        let event = sign(build_pulse_entry(PULSE_COORD, &entry, None, None).expect("build"));
        let decoded: serde_json::Value = serde_json::from_str(&event.content).expect("json");
        assert_eq!(decoded["supersedes"], "ff".repeat(32));
    }

    #[test]
    fn pulse_entry_accepts_every_entry_type() {
        for entry_type in [
            buzz_core::pulse::PulseEntryType::Plan,
            buzz_core::pulse::PulseEntryType::Milestone,
            buzz_core::pulse::PulseEntryType::Note,
            buzz_core::pulse::PulseEntryType::Handoff,
            buzz_core::pulse::PulseEntryType::Blocker,
        ] {
            let mut entry = pulse_entry();
            entry.entry_type = entry_type;
            let event = sign(build_pulse_entry(PULSE_COORD, &entry, None, None).expect("build"));
            assert_eq!(
                tag_values(&event, "pu-type"),
                vec![entry_type.as_str().to_string()]
            );
        }
    }

    // ---- Coding sessions ---------------------------------------------------

    fn cs_target() -> CodingSessionTarget {
        CodingSessionTarget {
            driver: "provider-a".into(),
            instance_id: "instance-1".into(),
            session_id: "session-1".into(),
            generation: 1,
        }
    }

    /// Flatten an event's tags into `(name, value)` pairs in signed order.
    /// Order is load-bearing for four of the six kinds: the relay's 44221
    /// validator and the consumer's ingress both read tags positionally.
    fn ordered_tags(event: &nostr::Event) -> Vec<(String, String)> {
        event
            .tags
            .iter()
            .map(|t| {
                let s = t.as_slice();
                (
                    s.first().cloned().unwrap_or_default(),
                    s.get(1).cloned().unwrap_or_default(),
                )
            })
            .collect()
    }

    fn cs_command_payload() -> CodingSessionCommandPayload {
        CodingSessionCommandPayload {
            schema: buzz_core::coding_session_command::CODING_SESSION_COMMAND_SCHEMA.into(),
            command_id: "cmd-1".into(),
            target: cs_target(),
            action: buzz_core::coding_session_command::CodingSessionAction::ThreadTurnStart {
                attachments: Vec::new(),
                deliver: buzz_core::coding_session_command::CodingSessionDelivery::Boundary,
                text: "Ship it".into(),
            },
        }
    }

    fn cs_lifecycle_payload(project_ref: Option<String>) -> CodingSessionLifecycleCommandPayload {
        use buzz_core::coding_session_lifecycle_command::CodingSessionLifecycleAction;
        CodingSessionLifecycleCommandPayload {
            schema:
                buzz_core::coding_session_lifecycle_command::CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA
                    .into(),
            command_id: "create-1".into(),
            action: CodingSessionLifecycleAction::SessionCreate {
                project_ref,
                repo_ref: None,
                session_ref: Some("5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10".into()),
                genesis_ref: None,
                provider_instance_ref: "claude-primary".into(),
                provider_authority_pubkey: "ab".repeat(32),
                model: None,
                title: Some("Advance Buzz live sessions".into()),
                initial_turn: None,
                actor: None,
                role: None,
                hire_ref: None,
                routing: None,
            },
        }
    }

    /// The seat pair is enforced at the builder, not only at the relay: a
    /// producer cannot sign half a seat.
    #[test]
    fn a_lifecycle_command_builder_refuses_half_an_agent_seat() {
        use buzz_core::coding_session_lifecycle_command::CodingSessionLifecycleAction;
        let channel = Uuid::new_v4();

        let mut payload = cs_lifecycle_payload(None);
        let CodingSessionLifecycleAction::SessionCreate { actor, .. } = &mut payload.action else {
            panic!("expected create action")
        };
        *actor = Some("cd".repeat(32));
        let error = build_coding_session_lifecycle_command(channel, &payload)
            .expect_err("an actor with no role must not build");
        assert!(
            format!("{error}").contains(buzz_core::coding_session_payload::ACTOR_ROLE_PAIR),
            "got {error}"
        );

        let CodingSessionLifecycleAction::SessionCreate { role, .. } = &mut payload.action else {
            panic!("expected create action")
        };
        *role = Some("lead".into());
        let builder = build_coding_session_lifecycle_command(channel, &payload)
            .expect("a complete seat builds");
        let event = builder
            .sign_with_keys(&Keys::generate())
            .expect("sign the seated create");
        assert!(event.content.contains(r#""actor":"#));
        assert!(event.content.contains(r#""role":"lead""#));
        assert!(
            !event.content.contains("nsec"),
            "a seat's key material must never reach signed content: {}",
            event.content
        );
    }

    /// A hire signs through the same 44221 builder as every other lifecycle
    /// action: three ordered tags, the seven-key action, and a brief that
    /// round-trips through the strict decoder the relay runs.
    #[test]
    fn a_lifecycle_command_builder_signs_a_session_hire() {
        use buzz_core::coding_session_lifecycle_command::{
            decode_coding_session_lifecycle_command, CodingSessionLifecycleAction,
            CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA, CODING_SESSION_LIFECYCLE_COMMAND_TAG_VERSION,
        };
        let channel = Uuid::new_v4();
        let payload = CodingSessionLifecycleCommandPayload {
            schema: CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA.into(),
            command_id: "hire-1".into(),
            action: CodingSessionLifecycleAction::SessionHire {
                session_ref: "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10".into(),
                genesis_ref: "12".repeat(32),
                role: "builder".into(),
                provider_instance_ref: None,
                model: None,
                brief: "Rebase the lane and run the gate.".into(),
                requested_by: None,
                routing: None,
            },
        };
        let event = build_coding_session_lifecycle_command(channel, &payload)
            .expect("a hire builds")
            .sign_with_keys(&Keys::generate())
            .expect("sign the hire");
        assert_eq!(
            event
                .tags
                .iter()
                .map(|tag| tag.as_slice().to_vec())
                .collect::<Vec<_>>(),
            vec![
                vec!["h".to_string(), channel.to_string()],
                vec![
                    "csl-v".to_string(),
                    CODING_SESSION_LIFECYCLE_COMMAND_TAG_VERSION.to_string()
                ],
                vec!["csl-command".to_string(), "hire-1".to_string()],
            ]
        );
        assert_eq!(
            decode_coding_session_lifecycle_command(&event.content).expect("decodes"),
            payload
        );

        // A role that is not a slug never reaches the wire.
        let mut bad = payload.clone();
        let CodingSessionLifecycleAction::SessionHire { role, .. } = &mut bad.action else {
            panic!("expected a hire action")
        };
        *role = "Builder".into();
        assert!(build_coding_session_lifecycle_command(channel, &bad).is_err());
    }

    /// The founder record: three ordered tags, and a `csg-session` tag that is
    /// the payload's own reference so a filter lookup and the signed content can
    /// never name different umbrellas.
    #[test]
    fn coding_session_genesis_builder_emits_ordered_payload_derived_tags() {
        let channel = Uuid::new_v4();
        let session_ref = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
        let payload = CodingSessionGenesisPayload::new(session_ref);
        let event = build_coding_session_genesis(channel, &payload)
            .unwrap()
            .sign_with_keys(&keys())
            .unwrap();

        assert_eq!(event.kind.as_u16() as u32, KIND_CODING_SESSION_GENESIS);
        assert_eq!(
            ordered_tags(&event),
            vec![
                ("h".into(), channel.to_string()),
                ("csg-v".into(), "csg1-1".into()),
                ("csg-session".into(), session_ref.into()),
            ]
        );
        assert_eq!(
            buzz_core::coding_session_genesis::decode_coding_session_genesis(&event.content)
                .unwrap(),
            payload
        );
    }

    #[test]
    fn coding_session_genesis_builder_rejects_invalid_payloads() {
        let channel = Uuid::new_v4();
        assert!(build_coding_session_genesis(
            channel,
            &CodingSessionGenesisPayload::new("umbrella")
        )
        .is_err());

        let mut payload = CodingSessionGenesisPayload::new("5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10");
        payload.v = 2;
        assert!(build_coding_session_genesis(channel, &payload).is_err());
    }

    #[test]
    fn coding_session_goal_builder_emits_regular_revision_envelope() {
        let channel = Uuid::new_v4();
        let session_ref = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
        let event = build_coding_session_goal(channel, session_ref, "Make authority visible")
            .unwrap()
            .sign_with_keys(&keys())
            .unwrap();
        assert_eq!(event.kind.as_u16() as u32, KIND_CODING_SESSION_GOAL);
        assert_eq!(event.content, "Make authority visible");
        assert_eq!(
            ordered_tags(&event),
            vec![
                ("h".into(), channel.to_string()),
                ("d".into(), session_ref.into()),
                ("csgl-v".into(), "csgl1-1".into()),
            ]
        );
    }

    #[test]
    fn coding_session_goal_builder_rejects_invalid_and_blank_inputs() {
        let channel = Uuid::new_v4();
        assert!(build_coding_session_goal(channel, "umbrella", "goal").is_err());
        assert!(
            build_coding_session_goal(channel, "5B7E1C2A-90D4-4B0E-A1F3-7C2D8E6F4A10", "goal")
                .is_err()
        );
        assert!(
            build_coding_session_goal(channel, "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10", "  \n")
                .is_err()
        );
    }

    #[test]
    fn coding_session_name_builder_emits_regular_revision_envelope() {
        let channel = Uuid::new_v4();
        let session_ref = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
        let event = build_coding_session_name(channel, session_ref, "Authority phase")
            .unwrap()
            .sign_with_keys(&keys())
            .unwrap();
        assert_eq!(event.kind.as_u16() as u32, KIND_CODING_SESSION_NAME);
        assert_eq!(event.content, "Authority phase");
        assert_eq!(
            ordered_tags(&event),
            vec![
                ("h".into(), channel.to_string()),
                ("d".into(), session_ref.into()),
                ("csnm-v".into(), "csnm1-1".into()),
            ]
        );
    }

    #[test]
    fn coding_session_name_builder_rejects_invalid_blank_and_multiline_inputs() {
        let channel = Uuid::new_v4();
        assert!(build_coding_session_name(channel, "umbrella", "name").is_err());
        assert!(
            build_coding_session_name(channel, "5B7E1C2A-90D4-4B0E-A1F3-7C2D8E6F4A10", "name")
                .is_err()
        );
        assert!(
            build_coding_session_name(channel, "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10", "  \n")
                .is_err()
        );
        assert!(build_coding_session_name(
            channel,
            "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10",
            "first\nsecond"
        )
        .is_err());
    }

    #[test]
    fn coding_session_closure_builder_emits_exact_regular_revision_envelope() {
        use buzz_core::coding_session_closure::CodingSessionClosureAction;

        let channel = Uuid::new_v4();
        let session_ref = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
        let genesis_ref = "ab".repeat(32);
        let payload = CodingSessionClosurePayload::new(
            CodingSessionClosureAction::Closed,
            &genesis_ref,
            session_ref,
        );
        let event = build_coding_session_closure(channel, &payload)
            .unwrap()
            .sign_with_keys(&keys())
            .unwrap();
        assert_eq!(event.kind.as_u16() as u32, KIND_CODING_SESSION_CLOSURE);
        assert_eq!(
            event.content,
            format!(
                r#"{{"action":"closed","genesisRef":"{genesis_ref}","sessionRef":"{session_ref}","v":1}}"#
            )
        );
        assert_eq!(
            ordered_tags(&event),
            vec![
                ("h".into(), channel.to_string()),
                ("d".into(), session_ref.into()),
                ("cscl-v".into(), "cscl1-1".into()),
                ("cscl-genesis".into(), genesis_ref),
            ]
        );
    }

    #[test]
    fn coding_session_closure_builder_rejects_invalid_references_and_version() {
        use buzz_core::coding_session_closure::CodingSessionClosureAction;

        let channel = Uuid::new_v4();
        let session_ref = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
        assert!(build_coding_session_closure(
            channel,
            &CodingSessionClosurePayload::new(
                CodingSessionClosureAction::Open,
                "AB".repeat(32),
                session_ref,
            )
        )
        .is_err());
        assert!(build_coding_session_closure(
            channel,
            &CodingSessionClosurePayload::new(
                CodingSessionClosureAction::Open,
                "ab".repeat(32),
                "umbrella",
            )
        )
        .is_err());

        let mut wrong_version = CodingSessionClosurePayload::new(
            CodingSessionClosureAction::Open,
            "ab".repeat(32),
            session_ref,
        );
        wrong_version.v = 2;
        assert!(build_coding_session_closure(channel, &wrong_version).is_err());
    }

    /// The adoption form emits the same three-tag envelope as a fresh
    /// founding — only the content differs — and the `csg-session` tag still
    /// tracks the payload's own `sessionRef`, not anything inside `adopts`.
    #[test]
    fn coding_session_genesis_builder_supports_the_adoption_form() {
        let channel = Uuid::new_v4();
        let session_ref = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
        let payload = CodingSessionGenesisPayload::new_adoption(
            session_ref,
            "ab".repeat(32),
            "cd".repeat(32),
        );
        let event = build_coding_session_genesis(channel, &payload)
            .unwrap()
            .sign_with_keys(&keys())
            .unwrap();

        assert_eq!(event.kind.as_u16() as u32, KIND_CODING_SESSION_GENESIS);
        assert_eq!(
            ordered_tags(&event),
            vec![
                ("h".into(), channel.to_string()),
                ("csg-v".into(), "csg1-1".into()),
                ("csg-session".into(), session_ref.into()),
            ],
            "the adoption form's envelope must be indistinguishable from a fresh founding"
        );
        assert_eq!(
            buzz_core::coding_session_genesis::decode_coding_session_genesis(&event.content)
                .unwrap(),
            payload
        );
    }

    /// The chain's first link: three ordered tags, and a `csat-genesis` tag
    /// that is the payload's own reference so a filter lookup and the signed
    /// content can never disagree on which genesis this transition anchors
    /// to.
    #[test]
    fn coding_session_authority_transition_builder_emits_ordered_payload_derived_tags() {
        let channel = Uuid::new_v4();
        let genesis_ref = "ab".repeat(32);
        let grantee = "cd".repeat(32);
        let payload = CodingSessionAuthorityTransitionPayload::new_grant_operator(
            genesis_ref.clone(),
            None,
            1,
            grantee.clone(),
        );
        let event = build_coding_session_authority_transition(channel, &payload)
            .unwrap()
            .sign_with_keys(&keys())
            .unwrap();

        assert_eq!(
            event.kind.as_u16() as u32,
            KIND_CODING_SESSION_AUTHORITY_TRANSITION
        );
        assert_eq!(
            ordered_tags(&event),
            vec![
                ("h".into(), channel.to_string()),
                ("csat-v".into(), "csat1-1".into()),
                ("csat-genesis".into(), genesis_ref.clone()),
            ]
        );
        assert_eq!(
            buzz_core::coding_session_authority_transition::decode_coding_session_authority_transition(
                &event.content
            )
            .unwrap(),
            payload
        );
    }

    /// A second, chained transition emits the same envelope shape with the
    /// predecessor's id carried in content only — the tag still names the
    /// genesis, not the predecessor.
    #[test]
    fn coding_session_authority_transition_builder_supports_a_chained_grant() {
        let channel = Uuid::new_v4();
        let genesis_ref = "ab".repeat(32);
        let prev = "11".repeat(32);
        let grantee = "cd".repeat(32);
        let payload = CodingSessionAuthorityTransitionPayload::new_grant_operator(
            genesis_ref.clone(),
            Some(prev),
            2,
            grantee,
        );
        let event = build_coding_session_authority_transition(channel, &payload)
            .unwrap()
            .sign_with_keys(&keys())
            .unwrap();
        assert_eq!(
            ordered_tags(&event),
            vec![
                ("h".into(), channel.to_string()),
                ("csat-v".into(), "csat1-1".into()),
                ("csat-genesis".into(), genesis_ref),
            ]
        );
    }

    #[test]
    fn coding_session_authority_transition_builder_rejects_invalid_payloads() {
        let channel = Uuid::new_v4();
        // seq = 0 is never valid.
        assert!(build_coding_session_authority_transition(
            channel,
            &CodingSessionAuthorityTransitionPayload::new_grant_operator(
                "ab".repeat(32),
                None,
                0,
                "cd".repeat(32),
            )
        )
        .is_err());
        // seq/prevAccepted disagreement.
        assert!(build_coding_session_authority_transition(
            channel,
            &CodingSessionAuthorityTransitionPayload::new_grant_operator(
                "ab".repeat(32),
                Some("11".repeat(32)),
                1,
                "cd".repeat(32),
            )
        )
        .is_err());
        // Malformed genesisRef.
        assert!(build_coding_session_authority_transition(
            channel,
            &CodingSessionAuthorityTransitionPayload::new_grant_operator(
                "not-hex",
                None,
                1,
                "cd".repeat(32),
            )
        )
        .is_err());
    }

    #[test]
    fn coding_session_command_builder_emits_ordered_payload_derived_tags() {
        let channel = Uuid::new_v4();
        let payload = cs_command_payload();
        let event = build_coding_session_command(channel, &payload)
            .unwrap()
            .sign_with_keys(&keys())
            .unwrap();

        assert_eq!(event.kind.as_u16() as u32, KIND_CODING_SESSION_COMMAND);
        assert_eq!(
            ordered_tags(&event),
            vec![
                ("h".into(), channel.to_string()),
                ("cs-v".into(), "csc1-1".into()),
                (
                    "cs-target".into(),
                    "coding-session/v1|10:provider-a10:instance-19:session-11:1".into()
                ),
            ]
        );
        // Content round-trips through the same contract the relay decodes with.
        let decoded: CodingSessionCommandPayload = serde_json::from_str(&event.content).unwrap();
        assert_eq!(decoded, payload);
    }

    #[test]
    fn coding_session_command_builder_rejects_invalid_payloads() {
        let channel = Uuid::new_v4();
        let mut payload = cs_command_payload();
        payload.target.generation = 0;
        assert!(build_coding_session_command(channel, &payload).is_err());
    }

    /// Both arms of the fork amendment: a project-bound session and a
    /// standalone one produce identically shaped envelopes, because no tag
    /// carries the project reference.
    #[test]
    fn coding_session_lifecycle_builder_handles_optional_project_ref() {
        let channel = Uuid::new_v4();
        let project = format!("30621:{}:amas-redux", "cd".repeat(32));
        for project_ref in [Some(project), None] {
            let payload = cs_lifecycle_payload(project_ref.clone());
            let event = build_coding_session_lifecycle_command(channel, &payload)
                .unwrap()
                .sign_with_keys(&keys())
                .unwrap();
            assert_eq!(
                event.kind.as_u16() as u32,
                KIND_CODING_SESSION_LIFECYCLE_COMMAND
            );
            assert_eq!(
                ordered_tags(&event),
                vec![
                    ("h".into(), channel.to_string()),
                    ("csl-v".into(), "csl1-1".into()),
                    ("csl-command".into(), "create-1".into()),
                ]
            );
            // Even a standalone session writes `projectRef` explicitly, so a
            // truncated payload can never be read as a deliberate one — and a
            // new producer always writes `sessionRef`, umbrella claimed or not.
            assert!(event.content.contains("\"projectRef\":"));
            assert!(event.content.contains("\"sessionRef\":"));
            assert_eq!(
                buzz_core::coding_session_lifecycle_command::decode_coding_session_lifecycle_command(
                    &event.content
                )
                .unwrap(),
                payload
            );
        }
    }

    #[test]
    fn coding_session_lifecycle_builder_rejects_non_project_refs() {
        let channel = Uuid::new_v4();
        let owner = "cd".repeat(32);
        for rejected in [format!("30178:{owner}:x"), "amas-redux".to_string()] {
            assert!(
                build_coding_session_lifecycle_command(
                    channel,
                    &cs_lifecycle_payload(Some(rejected.clone()))
                )
                .is_err(),
                "should reject {rejected:?}"
            );
        }
    }

    #[test]
    fn provider_catalog_builder_emits_ordered_tags_and_digests_exact_content() {
        let channel = Uuid::new_v4();
        let content =
            r#"{"schema":"buzz-coding-session-provider-catalog/v1","revision":3,"providers":[]}"#;
        let event = build_coding_session_provider_catalog(channel, 3, content)
            .unwrap()
            .sign_with_keys(&keys())
            .unwrap();

        assert_eq!(
            event.kind.as_u16() as u32,
            KIND_CODING_SESSION_PROVIDER_CATALOG
        );
        // Content is passed through byte-for-byte: cspc-key digests it, so any
        // re-serialization here would break the consumer's key check.
        assert_eq!(event.content, content);
        assert_eq!(
            ordered_tags(&event),
            vec![
                ("h".into(), channel.to_string()),
                ("cspc-v".into(), "cspc1-1".into()),
                ("cspc-revision".into(), "3".into()),
                (
                    "cspc-key".into(),
                    crate::coding_session::coding_session_provider_catalog_semantic_key(
                        &channel.to_string(),
                        3,
                        content
                    )
                ),
            ]
        );
    }

    #[test]
    fn provider_catalog_builder_enforces_revision_and_size() {
        let channel = Uuid::new_v4();
        assert!(build_coding_session_provider_catalog(channel, 0, "{}").is_err());
        let oversize = "a".repeat(MAX_PROVIDER_CATALOG_CONTENT_BYTES + 1);
        assert!(matches!(
            build_coding_session_provider_catalog(channel, 1, &oversize),
            Err(SdkError::ContentTooLarge { .. })
        ));
        let at_limit = "a".repeat(MAX_PROVIDER_CATALOG_CONTENT_BYTES);
        assert!(build_coding_session_provider_catalog(channel, 1, &at_limit).is_ok());
    }

    #[test]
    fn metadata_builder_emits_ordered_tags() {
        let channel = Uuid::new_v4();
        let target = cs_target();
        let event = build_coding_session_metadata(channel, &target, "{}")
            .unwrap()
            .sign_with_keys(&keys())
            .unwrap();

        assert_eq!(event.kind.as_u16() as u32, KIND_CODING_SESSION_METADATA);
        assert_eq!(
            ordered_tags(&event),
            vec![
                ("h".into(), channel.to_string()),
                ("csm-v".into(), "csm1-1".into()),
                (
                    "cs-target".into(),
                    "coding-session/v1|10:provider-a10:instance-19:session-11:1".into()
                ),
                (
                    "csm-key".into(),
                    "coding-session-metadata/v1|10:provider-a10:instance-19:session-11:1".into()
                ),
            ]
        );
    }

    #[test]
    fn lease_builder_emits_strict_content_and_ordered_tags() {
        use buzz_core::coding_session_lease::{
            CodingSessionLease, CodingSessionLeaseState, CODING_SESSION_LEASE_SCHEMA,
        };

        let channel = Uuid::parse_str("5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10").unwrap();
        let payload = CodingSessionLease {
            schema: CODING_SESSION_LEASE_SCHEMA.into(),
            target: cs_target(),
            state: CodingSessionLeaseState::Live,
            lease_sequence: 42,
        };
        let event = build_coding_session_lease(channel, "create-1", &payload)
            .unwrap()
            .sign_with_keys(&keys())
            .unwrap();

        assert_eq!(
            event.kind.as_u16() as u32,
            buzz_core::kind::KIND_CODING_SESSION_LEASE
        );
        assert_eq!(
            serde_json::from_str::<CodingSessionLease>(&event.content).unwrap(),
            payload
        );
        assert_eq!(
            ordered_tags(&event),
            vec![
                ("h".into(), channel.to_string()),
                ("cslease-v".into(), "cslease1-1".into()),
                (
                    "cs-target".into(),
                    "coding-session/v1|10:provider-a10:instance-19:session-11:1".into()
                ),
                ("csl-command".into(), "create-1".into()),
                ("cslease-seq".into(), "42".into()),
            ]
        );
    }

    #[test]
    fn lease_builder_rejects_invalid_payload_and_command_identifier() {
        use buzz_core::coding_session_lease::{CodingSessionLease, CodingSessionLeaseState};

        let channel = Uuid::new_v4();
        let mut payload =
            CodingSessionLease::new(cs_target(), CodingSessionLeaseState::Released, 1).unwrap();
        assert!(build_coding_session_lease(channel, " ", &payload).is_err());
        assert!(build_coding_session_lease(
            channel,
            &"x".repeat(MAX_IDENTIFIER_BYTES + 1),
            &payload
        )
        .is_err());

        payload.lease_sequence = 0;
        assert!(build_coding_session_lease(channel, "create-1", &payload).is_err());

        payload.lease_sequence = 1;
        payload.target.generation = 0;
        assert!(build_coding_session_lease(channel, "create-1", &payload).is_err());
    }

    #[test]
    fn lifecycle_receipt_builder_emits_ordered_tags_keyed_by_command_id() {
        let channel = Uuid::new_v4();
        let event = build_coding_session_lifecycle_receipt(channel, "create-1", "{}")
            .unwrap()
            .sign_with_keys(&keys())
            .unwrap();

        assert_eq!(
            event.kind.as_u16() as u32,
            KIND_CODING_SESSION_LIFECYCLE_RECEIPT
        );
        assert_eq!(
            ordered_tags(&event),
            vec![
                ("h".into(), channel.to_string()),
                ("cslr-v".into(), "cslr1-1".into()),
                ("csl-command".into(), "create-1".into()),
                (
                    "csl-key".into(),
                    "coding-session-lifecycle-receipt/v1|8:create-1".into()
                ),
            ]
        );
        assert!(build_coding_session_lifecycle_receipt(channel, "  ", "{}").is_err());
    }

    /// A turn command produces up to three receipts. Keying them by
    /// `commandId` alone — the lifecycle rule — would make the second one a
    /// duplicate of the first and fence it out of the outbox, so each stage
    /// carries its own key and no two stages collide.
    #[test]
    fn turn_receipt_keys_differ_per_stage_and_never_collide_with_a_lifecycle_key() {
        let channel = Uuid::new_v4();
        let stages = [
            ReceiptStatus::TurnQueued,
            ReceiptStatus::TurnStarted,
            ReceiptStatus::TurnDropped,
            ReceiptStatus::TurnRefused,
            ReceiptStatus::TurnDegraded,
            ReceiptStatus::InterruptDelivered,
        ];
        let mut stage_keys: Vec<String> = stages
            .iter()
            .map(|status| coding_session_turn_receipt_semantic_key("turn-1", *status))
            .collect();
        stage_keys.push(coding_session_lifecycle_receipt_semantic_key("turn-1"));
        let unique: std::collections::BTreeSet<&String> = stage_keys.iter().collect();
        assert_eq!(unique.len(), stage_keys.len(), "{stage_keys:?}");

        assert_eq!(
            coding_session_turn_receipt_semantic_key("turn-1", ReceiptStatus::TurnStarted),
            "coding-session-lifecycle-receipt/v1|6:turn-112:turn_started"
        );

        let event =
            build_coding_session_turn_receipt(channel, "turn-1", ReceiptStatus::TurnQueued, "{}")
                .unwrap()
                .sign_with_keys(&keys())
                .unwrap();
        assert_eq!(
            ordered_tags(&event),
            vec![
                ("h".into(), channel.to_string()),
                ("cslr-v".into(), "cslr1-1".into()),
                ("csl-command".into(), "turn-1".into()),
                (
                    "csl-key".into(),
                    "coding-session-lifecycle-receipt/v1|6:turn-111:turn_queued".into()
                ),
            ]
        );

        // A downgraded steer publishes `turn_degraded` and then `turn_queued`
        // for the same command; keyed by `commandId` alone the second would
        // fence the first out and the operator would never learn their steer
        // was not honoured.
        assert_ne!(
            coding_session_turn_receipt_semantic_key("turn-1", ReceiptStatus::TurnDegraded),
            coding_session_turn_receipt_semantic_key("turn-1", ReceiptStatus::TurnQueued)
        );
        assert!(build_coding_session_turn_receipt(
            channel,
            "turn-1",
            ReceiptStatus::InterruptDelivered,
            "{}"
        )
        .is_ok());

        // A lifecycle status has one outcome and must never be published on
        // the per-stage key.
        assert!(
            build_coding_session_turn_receipt(channel, "turn-1", ReceiptStatus::Created, "{}")
                .is_err()
        );
        assert!(
            build_coding_session_turn_receipt(channel, "  ", ReceiptStatus::TurnQueued, "{}")
                .is_err()
        );
    }

    #[test]
    fn transcript_builder_emits_ordered_tags_with_sequence() {
        let channel = Uuid::new_v4();
        let target = cs_target();
        let event = build_coding_session_transcript_item(channel, &target, 7, "{}")
            .unwrap()
            .sign_with_keys(&keys())
            .unwrap();

        assert_eq!(event.kind.as_u16() as u32, KIND_CODING_SESSION_TRANSCRIPT);
        assert_eq!(
            ordered_tags(&event),
            vec![
                ("h".into(), channel.to_string()),
                ("cst-v".into(), "cst1-1".into()),
                (
                    "cs-target".into(),
                    "coding-session/v1|10:provider-a10:instance-19:session-11:1".into()
                ),
                ("cst-seq".into(), "7".into()),
                (
                    "cst-key".into(),
                    "coding-session-transcript/v1|10:provider-a10:instance-19:session-11:11:7"
                        .into()
                ),
            ]
        );
        // Sequence numbers start at 1; a zero would collide with "unset".
        assert!(build_coding_session_transcript_item(channel, &target, 0, "{}").is_err());
    }

    /// Provider-authored events must not be able to name a target no command
    /// could ever have addressed.
    #[test]
    fn provider_authored_builders_validate_the_target() {
        let channel = Uuid::new_v4();
        let mut target = cs_target();
        target.generation = 0;
        assert!(build_coding_session_metadata(channel, &target, "{}").is_err());
        assert!(build_coding_session_transcript_item(channel, &target, 1, "{}").is_err());

        target = cs_target();
        target.session_id = "  ".into();
        assert!(build_coding_session_metadata(channel, &target, "{}").is_err());
    }

    #[test]
    fn provider_authored_builders_enforce_their_size_caps() {
        let channel = Uuid::new_v4();
        let target = cs_target();
        assert!(matches!(
            build_coding_session_metadata(
                channel,
                &target,
                &"a".repeat(MAX_METADATA_CONTENT_BYTES + 1)
            ),
            Err(SdkError::ContentTooLarge { .. })
        ));
        assert!(matches!(
            build_coding_session_lifecycle_receipt(
                channel,
                "create-1",
                &"a".repeat(MAX_LIFECYCLE_RECEIPT_CONTENT_BYTES + 1)
            ),
            Err(SdkError::ContentTooLarge { .. })
        ));
        assert!(matches!(
            build_coding_session_transcript_item(
                channel,
                &target,
                1,
                &"a".repeat(MAX_TRANSCRIPT_CONTENT_BYTES + 1)
            ),
            Err(SdkError::ContentTooLarge { .. })
        ));
    }

    /// The `h` tag is what makes a coding-session event inherit the channel
    /// ACL. The relay requires it; every builder must therefore emit it first.
    #[test]
    fn every_coding_session_builder_emits_the_channel_tag_first() {
        let channel = Uuid::new_v4();
        let target = cs_target();
        let lease = CodingSessionLease::new(
            target.clone(),
            buzz_core::coding_session_lease::CodingSessionLeaseState::Live,
            1,
        )
        .unwrap();
        let builders = [
            build_coding_session_command(channel, &cs_command_payload()).unwrap(),
            build_coding_session_lifecycle_command(channel, &cs_lifecycle_payload(None)).unwrap(),
            build_coding_session_provider_catalog(channel, 1, "{}").unwrap(),
            build_coding_session_lease(channel, "create-1", &lease).unwrap(),
            build_coding_session_metadata(channel, &target, "{}").unwrap(),
            build_coding_session_lifecycle_receipt(channel, "create-1", "{}").unwrap(),
            build_coding_session_transcript_item(channel, &target, 1, "{}").unwrap(),
        ];
        for builder in builders {
            let event = builder.sign_with_keys(&keys()).unwrap();
            assert_eq!(
                ordered_tags(&event).first(),
                Some(&("h".to_string(), channel.to_string())),
                "kind {} must tag its channel first",
                event.kind.as_u16()
            );
        }
    }
}
