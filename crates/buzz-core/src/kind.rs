//! Buzz V2 kind number registry.
//!
//! This module is the authoritative source for Buzz kind numbers.
//! All constants are `u32` — NIP-01 specifies kind as an unsigned integer,
//! and u32 covers the full range without truncation.

// Standard NIP kinds
/// NIP-01: User profile metadata.
pub const KIND_PROFILE: u32 = 0;
/// NIP-01: Short text note.
pub const KIND_TEXT_NOTE: u32 = 1;
/// NIP-02: Contact list / follow list.
pub const KIND_CONTACT_LIST: u32 = 3;
/// NIP-51: Mute list (replaceable, 10000–19999 range) — pubkeys/events/threads/words a user has muted.
///
/// User-owned global state, keyed by `(pubkey, kind)`. Same ownership/scope shape as kind:3.
pub const KIND_MUTE_LIST: u32 = 10000;
/// NIP-51: Pin list (replaceable) — events the user has pinned to their profile.
///
/// User-owned global state, keyed by `(pubkey, kind)`. The events referenced may live in
/// channels, but the pin list itself is profile-level state.
pub const KIND_PIN_LIST: u32 = 10001;
/// NIP-65: Relay list metadata (replaceable) — read/write relay preferences for the outbox model.
///
/// User-owned global state, keyed by `(pubkey, kind)`. Tags are `["r", url]` or
/// `["r", url, "read"]` / `["r", url, "write"]`.
pub const KIND_NIP65_RELAY_LIST_METADATA: u32 = 10002;
/// NIP-51: Bookmark list (replaceable) — events/articles/hashtags/URLs the user has bookmarked.
///
/// User-owned global state, keyed by `(pubkey, kind)`. References content but is not itself
/// channel-scoped content.
pub const KIND_BOOKMARK_LIST: u32 = 10003;
/// NIP-51: Emoji list (replaceable) — user preferred emojis and pointers to emoji sets.
pub const KIND_EMOJI_LIST: u32 = 10030;
/// NIP-51: Follow set (parameterized replaceable, 30000–39999 range) — named curated lists of pubkeys.
///
/// User-owned, keyed by `(pubkey, kind, d_tag)`. Allows multiple named follow lists on top of
/// the single kind:3 contact list (e.g. "close-friends", "news", "devs").
pub const KIND_FOLLOW_SET: u32 = 30000;
/// NIP-51: Bookmark set (parameterized replaceable) — named curated bookmark collections.
///
/// User-owned, keyed by `(pubkey, kind, d_tag)`.
pub const KIND_BOOKMARK_SET: u32 = 30003;
/// NIP-51 / NIP-30: Emoji set (parameterized replaceable).
///
/// User-owned, keyed by `(pubkey, kind, d_tag)`. Each member publishes their own
/// kind:30030 set (signed as themselves); the workspace emoji "palette" is the
/// client-side union of everyone's sets — a view computed on read, not stored
/// state. Ingest allowlists member-authored kind:30030/10030 (see
/// `required_scope_for_kind`), and the generic NIP-33 replace path keeps only the
/// latest per `(pubkey, d_tag)`.
pub const KIND_EMOJI_SET: u32 = 30030;
/// NIP-01: Channel metadata (replaceable). Not used by Buzz today.
pub const KIND_CHANNEL_METADATA: u32 = 41;
/// NIP-09: Event deletion request.
pub const KIND_DELETION: u32 = 5;
/// NIP-25: Content is emoji char or `+`/`-`.
pub const KIND_REACTION: u32 = 7;
/// NIP-17: Outer envelope for private DMs — hides sender, content, timestamp.
pub const KIND_GIFT_WRAP: u32 = 1059;
/// NIP-94: File metadata attachment.
pub const KIND_FILE_METADATA: u32 = 1063;
/// NIP-23: Long-form content (articles, blog posts, RFCs).
/// Parameterized replaceable (NIP-33, 30000–39999 range) — keyed by `(pubkey, kind, d_tag)`.
/// Stored globally (channel_id = NULL); author-owned, not channel-scoped.
pub const KIND_LONG_FORM: u32 = 30023;
/// NIP-38: User status (general, music, or custom d-tag).
/// Parameterized replaceable (NIP-33, 30000–39999 range) — keyed by `(pubkey, kind, d_tag)`.
/// Stored globally (channel_id = NULL); user-owned personal data, not channel-scoped.
pub const KIND_USER_STATUS: u32 = 30315;
/// NIP-78 / NIP-RS: Per-client read state blob for cross-device read position sync.
/// Parameterized replaceable (NIP-33, 30000–39999 range) — keyed by `(pubkey, kind, d_tag)`.
/// Stored globally (channel_id = NULL); user-owned personal data, not channel-scoped.
/// Content is NIP-44 encrypted to the user's own keypair.
pub const KIND_READ_STATE: u32 = 30078;
/// NIP-42 auth event — never stored (carries bearer tokens).
pub const KIND_AUTH: u32 = 22242;
/// BUD-01: Blossom upload auth (used in upload.rs, not stored).
pub const KIND_BLOSSOM_AUTH: u32 = 24242;
/// Buzz custom one-time identity binding proof (ephemeral, not stored).
pub const KIND_NOSTR_IDENTITY_BINDING: u32 = 24243;
/// NIP-98: HTTP auth event (used in nip98.rs, not stored).
pub const KIND_HTTP_AUTH: u32 = 27235;

// NEW: Buzz command kinds (Pure Nostr plan)
/// Agent metadata + owner reference (replaceable, agent-authored).
pub const KIND_AGENT_PROFILE: u32 = 10100;

/// NIP-AE: Agent Engram (parameterized replaceable, agent-authored).
///
/// Encrypted memory record for AI agents. Addressed by `(pubkey_a, kind, d_tag)`,
/// where `d_tag` is an HMAC over the agent↔owner conversation key. See
/// `docs/nips/NIP-AE.md` and [`crate::engram`].
pub const KIND_AGENT_ENGRAM: u32 = 30174;

/// NIP-ER: Event Reminder (parameterized replaceable, author-only).
///
/// Encrypted, author-only reminder addressed by `(pubkey, kind, d_tag)`. The
/// public `not_before` tag tells supporting relays when the reminder is due;
/// the target, note, and state are NIP-44 encrypted to the author. Reads are
/// author-only (see [`AUTHOR_ONLY_KINDS`]). See `docs/nips/NIP-ER.md`.
pub const KIND_EVENT_REMINDER: u32 = 30300;

/// NIP-PL: encrypted push lease (parameterized replaceable, author-only).
///
/// The source event contains endpoint-bearing NIP-44 ciphertext and is readable
/// only by its authenticated author. Effective delivery state lives in the
/// dedicated push lease tables.
pub const KIND_PUSH_LEASE: u32 = 30350;

/// NIP-PMA: owner-encrypted private managed-agent aggregate.
///
/// Addressed by `(owner pubkey, kind, agent pubkey)`. The signed outer tags
/// expose only the agent coordinate, CAS generation/predecessor, and active/deleted
/// state required for relay enforcement. Content is NIP-44 v2 encrypted from
/// the owner's key to itself and contains the runnable identity/configuration
/// plus exact public projection bindings. See `docs/nips/NIP-PMA.md`.
pub const KIND_PRIVATE_MANAGED_AGENT: u32 = 30179;

/// Kinds whose stored events are readable only by their author.
///
/// The relay must never reveal the existence, count, tags, content, schedule,
/// or search matches of these events to anyone but the authenticated author.
/// Shared across the ingest write path (NIP-ER `not_before` validation) and the
/// read path (REQ/COUNT/subscription author-only filtering).
///
/// Currently a tiny linear set. If this grows past ~4 kinds, convert to a
/// compile-time bitset or sorted array with binary search for hot-path use.
pub const AUTHOR_ONLY_KINDS: &[u32] = &[
    KIND_EVENT_REMINDER,
    KIND_PUSH_LEASE,
    KIND_PRIVATE_MANAGED_AGENT,
];

/// Kinds that require a result-level read gate beyond the filter-layer
/// `#p` check: even a reader who knows an event id MUST match the event's
/// `#p` tag to receive the event. This closes the kindless `{ids:[…]}` read
/// path for events whose existence must not be leaked.
///
/// Used by `filter_can_match_result_gated_kinds` to force the per-event
/// fallback path in COUNT rather than the fast SQL `count_events()`.
pub const RESULT_GATED_KINDS: &[u32] = &[KIND_DM_VISIBILITY, KIND_AGENT_TURN_METRIC];

/// Kinds whose stored events have `#p`-bound read access — readable only by
/// subscribers whose pubkey appears in the event's `#p` tag.
///
/// The relay enforces this at the filter layer (`p_gated_filters_authorized`):
/// a REQ that can match any kind in this set is closed unless the filter's
/// `#p` values exactly equal the authenticated reader's pubkey. For stored
/// (non-ephemeral) kinds in this set, the storage layer additionally writes a
/// NULL `search_tsv` so the event is unsearchable through NIP-50 FTS
/// (`schema/schema.sql` and `migrations/0001_initial_schema.sql` — drift
/// caught by `p_gated_persistent_kinds_have_storage_null_tsvector` in
/// `crates/buzz-search/tests/fts_integration.rs`).
///
/// Ephemeral kinds (20000–29999, e.g. [`KIND_AGENT_OBSERVER_FRAME`]) are
/// included for filter-layer enforcement but are never stored, so the
/// storage-layer search defense does not apply to them.
pub const P_GATED_KINDS: &[u32] = &[
    KIND_AGENT_OBSERVER_FRAME,
    KIND_MEMBER_ADDED_NOTIFICATION,
    KIND_MEMBER_REMOVED_NOTIFICATION,
    KIND_GIFT_WRAP,
    KIND_DM_VISIBILITY,
    // NIP-AM: agent turn metrics are encrypted to the owner and must not be
    // readable by any unauthenticated or non-owner party, including via `ids`
    // filters — see NIP-AM §Relay Behavior.
    KIND_AGENT_TURN_METRIC,
];

/// NIP-AP: Agent Persona (parameterized replaceable, owner-authored).
///
/// Persona definition event published by the workspace owner. Addressed by
/// `(pubkey, kind, d_tag)` where `d_tag` is the plaintext persona slug.
/// Content is a JSON body containing persona fields (system_prompt,
/// display_name, avatar_url, runtime, model, provider, name_pool).
///
/// # Access control: author-only-unless-shared
///
/// Kind 30175 uses **shared-tag-gated** read semantics to protect system
/// prompts and `respond_to_allowlist` pubkeys from being visible to all
/// community members as a side-effect of device sync:
///
/// - Events WITHOUT a `["shared", "true"]` tag are readable only by their
///   author. Foreign REQ/COUNT/fan-out/ids-lookup requests silently omit them.
/// - Events WITH exactly `["shared", "true"]` are readable community-wide,
///   enabling the opt-in agent catalog (`{kinds:[30175]}` all-authors).
///
/// Device sync already queries `authors:[self]`, so this gate never affects
/// self-reads. The `shared` tag is a tag (not a content field) so toggling
/// sharing does not change content bytes or the drift/`source_version` hash
/// (`persona_content_hash`) used by persona sync.
///
/// Ingest rejects malformed `shared` tags (any value other than `"true"`,
/// or more than one `shared` tag) so no ambiguous heads can exist.
pub const KIND_PERSONA: u32 = 30175;

/// Kinds that use the author-only-unless-shared read model.
///
/// Events of these kinds may only be delivered to foreign readers when the
/// event carries exactly `["shared", "true"]`. Every relay read chokepoint
/// consults this set: REQ historical delivery, live fan-out, COUNT fallback,
/// the `ids`-lookup result gate, both HTTP surfaces, and the pre-`LIMIT` SQL
/// visibility pushdown in `buzz-db`.
///
/// Membership is a privacy decision, not a convenience: adding a kind here
/// makes its events invisible to foreign readers until their author opts in,
/// and the opt-in must be a `shared` TAG (not a content field) so that
/// toggling it leaves content bytes — and any content hash derived from them —
/// unchanged.
///
/// `KIND_TEAM` (30176) is deliberately NOT a member. Its writers never emit
/// `shared`, so catalog opt-in semantics do not describe it; it needs
/// owner-private read semantics instead, which is a separate change.
pub const SHARED_GATED_KINDS: &[u32] = &[KIND_PERSONA, KIND_TEAM_CATALOG];

/// Returns `true` if `kind` uses the author-only-unless-shared read model
/// (see [`SHARED_GATED_KINDS`]).
pub fn is_shared_gated_kind(kind: u32) -> bool {
    SHARED_GATED_KINDS.contains(&kind)
}

/// Returns `true` if the event is a shared-gated kind AND the requester is NOT
/// the author AND the event does NOT carry `["shared", "true"]`. All three
/// conditions must hold to withhold the event.
///
/// This is the per-event gate used by REQ historical delivery, live fan-out,
/// and COUNT fallback paths. It is intentionally independent of
/// `is_author_only_event` — shared-gated events with `["shared", "true"]` MUST
/// reach foreign readers; stripping them at the author-only layer would break
/// the catalog query.
pub fn is_unshared_gated_event(event: &nostr::Event, requester_pubkey_bytes: &[u8]) -> bool {
    let kind = event.kind.as_u16() as u32;
    if !is_shared_gated_kind(kind) {
        return false;
    }
    // Author reads are always allowed.
    if event.pubkey.to_bytes() == requester_pubkey_bytes {
        return false;
    }
    // Foreign reader: allowed only if the event is explicitly shared.
    !event_is_shared(event)
}

/// Returns `true` if the event carries exactly one `["shared", "true"]` tag.
///
/// Kind-agnostic: this is purely the tag-shape predicate. The kind check lives
/// in [`is_shared_gated_kind`], so callers that need "is this event shared"
/// for a kind they already know (e.g. a client deciding whether its own
/// retained head is published) can use this directly.
///
/// Requires the tag to have exactly two elements so that a three-element shape
/// like `["shared","true","extra"]` is NOT treated as shared. Ingest enforces
/// the same exact shape, so a well-stored event either has no `shared` tag
/// (author-only) or exactly one with precisely two elements and value `"true"`
/// (community-readable). This helper fails closed on any non-exact shape
/// independently of ingest guarantees.
pub fn event_is_shared(event: &nostr::Event) -> bool {
    let mut count = 0usize;
    for tag in event.tags.iter() {
        let parts = tag.as_slice();
        if parts.len() == 2 && parts[0].as_str() == "shared" {
            if parts[1].as_str() != "true" {
                return false;
            }
            count += 1;
        } else if !parts.is_empty() && parts[0].as_str() == "shared" {
            // Non-exact shape (wrong length) — fail closed: not shared.
            return false;
        }
    }
    count == 1
}

/// NIP-AP: Agent Team (parameterized replaceable, owner-authored).
///
/// Team definition event published by the workspace owner. Addressed by
/// `(pubkey, kind, d_tag)` where `d_tag` is the team's stable id. Content is a
/// JSON body projecting public team fields (name, description, persona_ids).
/// A team is a user-facing grouping of personas; publishing keeps it
/// authoritative across clients and reboots, mirroring `KIND_PERSONA`.
pub const KIND_TEAM: u32 = 30176;

/// NIP-AP: Managed Agent (parameterized replaceable, owner-authored).
///
/// Managed-agent definition event published by the workspace owner. Addressed
/// by `(pubkey, kind, d_tag)` where `d_tag` is the agent's pubkey. Content is
/// an explicit opt-IN allowlist projection of the agent record — it MUST never
/// carry the agent's secret key, NIP-OA auth tag, env vars, or runtime fields,
/// since these events are world-readable on the relay.
pub const KIND_MANAGED_AGENT: u32 = 30177;

/// NIP-AP: Team Catalog projection (parameterized replaceable, owner-authored).
///
/// The shareable projection of a team, addressed by `(pubkey, kind, d_tag)`
/// where `d_tag` is the team's stable id. Content is a versioned JSON body
/// carrying sanitized team fields plus ordered, EMBEDDED member definition
/// projections.
///
/// # Why this is not a `shared` tag on [`KIND_TEAM`]
///
/// A team's members live in kind 30175 events that are author-only unless
/// individually shared, so a foreign reader of a shared team could never
/// hydrate its members. This kind therefore embeds the member projections
/// rather than referencing them: the share is atomic, it covers built-in
/// members that have no 30175 head at all, it is immune to local-id/d-tag
/// divergence, and an unshared 30175 stays private. Kind 30176's wire body is
/// untouched, so device sync keeps its contract.
///
/// # Access control
///
/// Member of [`SHARED_GATED_KINDS`]: author-only unless the event carries
/// exactly `["shared", "true"]`. Ingest additionally requires exactly one
/// non-empty, bounded `d` tag — generic NIP-33 storage maps a missing `d` to
/// the empty coordinate, which would collapse every team into one slot.
///
/// Content carries only sanitized fields: no env vars, no `respond_to`
/// allowlist pubkeys, no source or local ids, no filesystem paths, no secrets.
pub const KIND_TEAM_CATALOG: u32 = 30178;

// NIP-56 reporting
/// NIP-56: Report an event, pubkey, or blob to relay moderators (kind:1984).
///
/// Accepted at ingest, persisted to the tenant-scoped `moderation_reports`
/// queue, and never fanned out publicly. Reports are signals, not triggers:
/// the relay never auto-actions on them (NIP-56).
pub const KIND_REPORT: u32 = 1984;

/// Buzz product feedback submission. Accepted at ingest, sidecarred to the
/// deployment feedback table, and never stored or fanned out as an event.
pub const KIND_PRODUCT_FEEDBACK: u32 = 42000;

// NIP-29 group admin events
/// NIP-29: Add a user to a group.
pub const KIND_NIP29_PUT_USER: u32 = 9000;
/// NIP-29: Remove a user from a group.
pub const KIND_NIP29_REMOVE_USER: u32 = 9001;
/// NIP-29: Edit group metadata.
pub const KIND_NIP29_EDIT_METADATA: u32 = 9002;
/// NIP-29: Delete an event from a group.
pub const KIND_NIP29_DELETE_EVENT: u32 = 9005;
/// NIP-29: Create a new group.
pub const KIND_NIP29_CREATE_GROUP: u32 = 9007;
/// NIP-29: Delete a group.
pub const KIND_NIP29_DELETE_GROUP: u32 = 9008;
/// NIP-29: Create an invite to a group.
pub const KIND_NIP29_CREATE_INVITE: u32 = 9009;
/// NIP-29: Request to join a group.
pub const KIND_NIP29_JOIN_REQUEST: u32 = 9021;
/// NIP-29: Request to leave a group.
pub const KIND_NIP29_LEAVE_REQUEST: u32 = 9022;

// NIP-MP project membership ops (user-signed, stored; processed like the
// NIP-29 9000-series: validated, applied to the project ACL, then projected
// into a relay-signed kind:39010 roster). See docs/nips/NIP-MP.md.
/// NIP-MP: Add project members or change their roles.
///
/// Tags: `["a", "30621:<owner-hex>:<dtag>"]` (required singleton) plus one or
/// more `["p", <lowercase-64-hex>, "", <role>]` where `role` is a
/// [`PROJECT_ROLES`] value. Signer must be the project creator or a roster
/// `owner`. Re-putting an existing member changes their role.
pub const KIND_PROJECT_PUT_MEMBER: u32 = 9010;
/// NIP-MP: Remove project members.
///
/// Tags: `["a", coordinate]` (required singleton) plus one or more
/// `["p", <lowercase-64-hex>]`. Signer must be the project creator or a
/// roster `owner`. The creator can never be removed (they are the project's
/// address) — such an op is refused outright.
pub const KIND_PROJECT_REMOVE_MEMBER: u32 = 9011;

// Buzz community moderation commands (mod-signed, processed like 9030-series:
// validated + executed directly, never stored as regular events; every
// accepted command writes a `moderation_actions` audit row).
/// Moderation: ban a pubkey from the community (`p` tag target, optional
/// `expiration` + `reason` tags).
pub const KIND_MODERATION_BAN: u32 = 9040;
/// Moderation: lift a community ban (`p` tag target).
pub const KIND_MODERATION_UNBAN: u32 = 9041;
/// Moderation: timeout (write-block) a pubkey until an `expiration` tag
/// timestamp (`p` tag target, optional `reason`).
pub const KIND_MODERATION_TIMEOUT: u32 = 9042;
/// Moderation: clear a timeout early (`p` tag target).
pub const KIND_MODERATION_UNTIMEOUT: u32 = 9043;
/// Moderation: resolve a report (`report` tag = report event id hex,
/// `status` tag = resolved|dismissed, `action` tag =
/// delete|kick|ban|timeout|dismiss|escalate — see
/// `handlers/moderation_commands.rs` for the pinned vocabulary).
pub const KIND_MODERATION_RESOLVE_REPORT: u32 = 9044;

/// Returns `true` for community moderation command kinds (9040–9044).
///
/// The canonical route check — use this instead of scattering
/// `9040..=9044` matches across ingest/dispatch.
pub const fn is_moderation_command_kind(kind: u32) -> bool {
    matches!(
        kind,
        KIND_MODERATION_BAN
            | KIND_MODERATION_UNBAN
            | KIND_MODERATION_TIMEOUT
            | KIND_MODERATION_UNTIMEOUT
            | KIND_MODERATION_RESOLVE_REPORT
    )
}

// NIP-43 relay membership admin commands
/// NIP-43: Add a pubkey to the relay member list.
pub const RELAY_ADMIN_ADD_MEMBER: u32 = 9030;
/// NIP-43: Remove a pubkey from the relay member list.
pub const RELAY_ADMIN_REMOVE_MEMBER: u32 = 9031;
/// NIP-43: Change the role of an existing relay member.
pub const RELAY_ADMIN_CHANGE_ROLE: u32 = 9032;
/// Buzz: Set the workspace profile (icon). Admin/owner-signed command.
pub const RELAY_ADMIN_SET_WORKSPACE_PROFILE: u32 = 9033;
// NIP-43 relay membership announcement events (relay-signed)
/// NIP-43: Relay membership list snapshot (relay-signed, replaceable by convention).
pub const KIND_NIP43_MEMBERSHIP_LIST: u32 = 13534;
/// NIP-43: Member added announcement (relay-signed).
pub const KIND_NIP43_MEMBER_ADDED: u32 = 8000;
/// NIP-43: Member removed announcement (relay-signed).
pub const KIND_NIP43_MEMBER_REMOVED: u32 = 8001;
/// NIP-43: User leave request (user-signed, ephemeral).
pub const KIND_NIP43_LEAVE_REQUEST: u32 = 28936;

// NIP-IA identity archival requests (user/agent/owner-signed)
/// NIP-IA: Request that the relay archive a target identity.
pub const KIND_IA_ARCHIVE_REQUEST: u32 = 9035;
/// NIP-IA: Request that the relay unarchive a target identity.
pub const KIND_IA_UNARCHIVE_REQUEST: u32 = 9036;

// NIP-IA identity archival announcement events (relay-signed)
/// NIP-IA: Archived-identity delta (relay-signed).
pub const KIND_IA_ARCHIVED: u32 = 8002;
/// NIP-IA: Unarchived-identity delta (relay-signed).
pub const KIND_IA_UNARCHIVED: u32 = 8003;
/// NIP-IA: Archived identities list snapshot (relay-signed, replaceable).
pub const KIND_IA_ARCHIVED_LIST: u32 = 13535;

// NIP-29 group state (addressable range 39000–39003)
/// NIP-29: Addressable group metadata state.
pub const KIND_NIP29_GROUP_METADATA: u32 = 39000;
/// NIP-29: Addressable group admins list.
pub const KIND_NIP29_GROUP_ADMINS: u32 = 39001;
/// NIP-29: Addressable group members list.
pub const KIND_NIP29_GROUP_MEMBERS: u32 = 39002;
/// NIP-29: Addressable group roles definition.
pub const KIND_NIP29_GROUP_ROLES: u32 = 39003;

/// NIP-MP: Relay-signed addressable project roster projection
/// (`d` = the project coordinate `30621:<owner-hex>:<dtag>`).
///
/// One `["p", <hex>, "", <role>]` per member, mirroring the NIP-29 39002 tag
/// grammar. Re-emitted by the relay after every accepted membership op
/// ([`KIND_PROJECT_PUT_MEMBER`]/[`KIND_PROJECT_REMOVE_MEMBER`]) and after
/// head-sourced roster changes. Never client-submitted; withheld from readers
/// outside a private project by the coordinate predicate
/// [`project_membership_event_hidden_from`].
pub const KIND_PROJECT_MEMBERS: u32 = 39010;

// Channel-window overlays (relay-signed, synthesized at query time, never
// stored). Appended to bridge `/query` responses for `top_level` window
// requests — see docs/bridge-channel-window.md.
/// Thread summary overlay: `e`/`d` tag = root event id, content =
/// `{reply_count, descendant_count, last_reply_at, participants}`.
pub const KIND_THREAD_SUMMARY: u32 = 39005;
/// Window bounds overlay: `d` tag = `<channel_id>:<request-cursor-or-head>`,
/// content = `{has_more, next_cursor}`. The only authority on exhaustion —
/// clients must not infer `has_more` from row counts.
pub const KIND_WINDOW_BOUNDS: u32 = 39006;

/// Workflow definition (parameterized replaceable, d=workflow_uuid).
pub const KIND_WORKFLOW_DEF: u32 = 30620;

/// NIP-DV: per-viewer DM visibility snapshot (relay-signed, parameterized
/// replaceable, d=viewer_pubkey). Carries one `h` tag per DM the viewer has
/// hidden from their sidebar. Re-published by the relay on every hide/unhide so
/// the latest event is always the authoritative hidden set. The relay knows
/// `hidden_at` per viewer; this is the only Nostr-visible projection of it.
pub const KIND_DM_VISIBILITY: u32 = 30622;

/// Lower bound of the NIP-33 parameterized replaceable range (30000–39999).
pub const PARAM_REPLACEABLE_KIND_MIN: u32 = 30000;
/// Upper bound of the NIP-33 parameterized replaceable range (30000–39999).
pub const PARAM_REPLACEABLE_KIND_MAX: u32 = 39999;

/// Lower bound of the ephemeral event range (20000–29999). Never stored.
pub const EPHEMERAL_KIND_MIN: u32 = 20000;
/// Upper bound of the ephemeral event range (20000–29999). Never stored.
pub const EPHEMERAL_KIND_MAX: u32 = 29999;

// Ephemeral events (20000–29999) — Redis pub/sub only, never stored.
/// Ephemeral: user presence update (online/away/offline).
pub const KIND_PRESENCE_UPDATE: u32 = 20001;
/// NIP-AB: Device pairing event. Ephemeral — relay may discard after delivery.
pub const KIND_PAIRING: u32 = 24134;
/// Ephemeral: typing indicator for a channel.
pub const KIND_TYPING_INDICATOR: u32 = 20002;
/// Ephemeral: owner-scoped encrypted agent observer telemetry and control frame.
pub const KIND_AGENT_OBSERVER_FRAME: u32 = 24200;
/// NIP-ST: shared-terminal watch open/keepalive/resync (ephemeral, observer →
/// session owner). Tags: `p` = owner (routing), `d` = session id, `a` = the
/// project coordinate the session is shared under. Content is a small JSON
/// `{"action":"watch"|"stop"|"resync"}`. Never stored; project-membership
/// gated at ingest and fan-out for private projects.
pub const KIND_SHELL_WATCH: u32 = 24310;
/// NIP-ST: shared-terminal frame stream (ephemeral, session owner →
/// observers). Tags: `d` = session id, `a` = project coordinate, `t` = frame
/// type (`tail`|`snap`|`diff`|`resize`|`end`), `seq`, `epoch`, optional
/// `dims`/`chunk`. Content is base64 raw terminal bytes an observer terminal
/// can write verbatim. Never stored; same project gating as
/// [`KIND_SHELL_WATCH`].
pub const KIND_SHELL_FRAME: u32 = 24311;
/// NIP-ST: shared-terminal input stream (ephemeral, roster collaborator →
/// session owner). Tags: `p` = owner (routing target), `d` = session id,
/// `a` = project coordinate. Content is base64 raw input bytes the owner
/// host writes to the PTY after independently re-verifying the sender is a
/// roster `collaborator` on the session's kind:30623 announce. The relay
/// accepts input only from the owner or a roster collaborator and delivers
/// it only to the owner's connections — never mere project members, never
/// viewers. Never stored. See `docs/nips/NIP-ST.md` §Input.
pub const KIND_SHELL_INPUT: u32 = 24312;
/// Ephemeral provider-signed liveness lease for one exact coding-session generation.
///
/// Ordered tags: `h`, `cslease-v`, `cs-target`, `csl-command`, `cslease-seq`.
/// The relay validates lifecycle authority and holds the original event only in
/// its expiring Redis register; it is never inserted into Postgres. See
/// `docs/nips/NIP-CSL.md`.
pub const KIND_CODING_SESSION_LEASE: u32 = 24223;
/// Ephemeral: huddle emoji reaction burst. Channel-scoped to the ephemeral
/// huddle channel with an `h` tag; never stored in the timeline.
pub const KIND_HUDDLE_REACTION: u32 = 24810;
// Stream messaging
/// NIP-29 group chat message kind. V1 used kind:10001 (replaceable range — wrong), then 40001.
///
/// Agent shutdown convention: the agent's owner sends a kind:9 message with content
/// `"!shutdown"` and a `#p` tag mentioning the agent. The harness exits gracefully.
/// This is a convention, not a new event kind — uses regular stream messages.
pub const KIND_STREAM_MESSAGE: u32 = 9;
/// V1 used kind:10002 (replaceable range — wrong).
pub const KIND_STREAM_MESSAGE_V2: u32 = 40002;
/// V1 used kind:10004 (replaceable range + NIP-51 collision — wrong).
pub const KIND_STREAM_MESSAGE_EDIT: u32 = 40003;
/// A stream message that has been pinned in a channel.
pub const KIND_STREAM_MESSAGE_PINNED: u32 = 40004;
/// A stream message that has been bookmarked by a user.
pub const KIND_STREAM_MESSAGE_BOOKMARKED: u32 = 40005;
/// A stream message scheduled for future delivery.
pub const KIND_STREAM_MESSAGE_SCHEDULED: u32 = 40006;
/// A reminder attached to a stream message or time.
pub const KIND_STREAM_REMINDER: u32 = 40007;
/// A diff/patch message showing file changes (unified diff format).
pub const KIND_STREAM_MESSAGE_DIFF: u32 = 40008;
/// Canvas (shared document) for a channel.
pub const KIND_CANVAS: u32 = 40100;
/// System message for channel state changes (join, leave, rename, etc.).
pub const KIND_SYSTEM_MESSAGE: u32 = 40099;

// Relay-only sidecar kinds (never client-submitted)
/// Channel metadata with computed fields (relay-signed sidecar).
pub const KIND_CHANNEL_SUMMARY: u32 = 40901;
/// Bulk presence state (relay-signed sidecar).
pub const KIND_PRESENCE_SNAPSHOT: u32 = 40902;

// Direct messages (41000–41999)
/// Open/create DM (p-tags = participants).
pub const KIND_DM_OPEN: u32 = 41010;
/// Add member to group DM.
pub const KIND_DM_ADD_MEMBER: u32 = 41011;
/// Hide DM from sidebar.
pub const KIND_DM_HIDE: u32 = 41012;
/// A new direct-message conversation was created.
pub const KIND_DM_CREATED: u32 = 41001;

// Agent job protocol (43000–43999)
// Not using NIP-90 kinds (5000–6999) — Buzz requires auth chains (depth ≤ 3, breadth ≤ 10).
/// An agent job was requested.
pub const KIND_JOB_REQUEST: u32 = 43001;
/// An agent accepted a job request.
pub const KIND_JOB_ACCEPTED: u32 = 43002;
/// Progress update for an in-flight agent job.
pub const KIND_JOB_PROGRESS: u32 = 43003;
/// Final result of a completed agent job.
pub const KIND_JOB_RESULT: u32 = 43004;
/// A job cancellation was requested.
pub const KIND_JOB_CANCEL: u32 = 43005;
/// An agent job failed with an error.
pub const KIND_JOB_ERROR: u32 = 43006;

/// Relay-signed notification: the target pubkey was added to a channel.
/// Stored globally (channel_id = None) with p-tag = target, h-tag = channel UUID.
pub const KIND_MEMBER_ADDED_NOTIFICATION: u32 = 44100;

/// Relay-signed notification: the target pubkey was removed from a channel.
/// Stored globally (channel_id = None) with p-tag = target, h-tag = channel UUID.
pub const KIND_MEMBER_REMOVED_NOTIFICATION: u32 = 44101;

/// NIP-AM: Agent Turn Metric — durable per-turn token-usage record (agent-authored).
///
/// Regular stored event (append-only, never replaced). The agent publishes one
/// event per completed turn, NIP-44 encrypted to its owner. Tags: exactly one `p`
/// (owner pubkey) and one `agent` (agent pubkey == event pubkey); no `h` tag.
/// Stored globally (channel_id = NULL); owner-scoped reads only (p-gated, NIP-42).
/// See `docs/nips/NIP-AM.md`.
pub const KIND_AGENT_TURN_METRIC: u32 = 44200;

/// Project Pulse entry — one explicit coordination claim scoped to a project.
///
/// Regular stored event (append-only), scoped to exactly one NIP-MP project by
/// a canonical `a` coordinate, optionally to a channel by `h` and to an
/// umbrella coding session by `pu-session`. Content is strict public JSON
/// ([`crate::pulse::PulseEntry`]) naming the claim type, its prose, the code
/// areas the author claims, the branch, and the entry it supersedes. Tags are
/// position-independent with a closed key set: `a`, `pu-v`, `pu-type`, and the
/// optional `h`, `branch`, `pu-session`.
///
/// Deliberately **not** parameterized-replaceable: a revision is a new entry
/// naming its predecessor in `supersedes`, and the fold decides whether that
/// claim is honored (same author only). A replaceable Pulse would let one
/// author's write erase the record another author's advisory was built on.
pub const KIND_PULSE_ENTRY: u32 = 44240;

/// NIP-CSC: Coding-session command — an operator-authored, provider-neutral turn
/// request.
///
/// Regular stored event (append-only), channel-scoped via an `h` tag. Content is
/// a public JSON [`crate::coding_session_command::CodingSessionCommandPayload`];
/// event authorship is the only actor authority. Consumed by an out-of-relay
/// provider adapter — the relay deliberately never executes this kind, it only
/// validates the envelope and stores it. Tags: exactly one `h`, `cs-v`, and
/// `cs-target`. See `docs/nips/NIP-CSC.md`.
pub const KIND_CODING_SESSION_COMMAND: u32 = 44220;

/// NIP-CSL: Coding-session lifecycle command — a session creation request.
///
/// Regular stored event (append-only), channel-scoped via an `h` tag. Content is
/// a public JSON
/// [`crate::coding_session_lifecycle_command::CodingSessionLifecycleCommandPayload`].
/// Ordered tags: `h`, `csl-v`, `csl-command`. The optional `projectRef` binds a
/// session to a NIP-MP project (kind 30621); standalone sessions omit it.
/// See `docs/nips/NIP-CSL.md`.
pub const KIND_CODING_SESSION_LIFECYCLE_COMMAND: u32 = 44221;

/// NIP-CSPC: Coding-session provider catalog — a provider-authored advertisement
/// of the session drivers, models, and capabilities it can serve.
///
/// Regular stored event (append-only), channel-scoped via an `h` tag. Content is
/// a public JSON catalog carrying a monotonic `revision`; consumers keep the
/// highest revision per (channel, signer). Ordered tags: `h`, `cspc-v`,
/// `cspc-revision`, `cspc-key`. See `docs/nips/NIP-CSPC.md`.
pub const KIND_CODING_SESSION_PROVIDER_CATALOG: u32 = 44222;

/// NIP-CSL: Coding-session metadata — provider-authored observations about one
/// exact session generation.
///
/// Regular stored event (append-only), channel-scoped via an `h` tag. Providers
/// publish on observed transitions; consumers retain history and fold the
/// newest valid observation per generation. Ordered tags: `h`, `csm-v`,
/// `cs-target`, `csm-key`. See `docs/nips/NIP-CSL.md`.
pub const KIND_CODING_SESSION_METADATA: u32 = 44223;

/// NIP-CSL: Coding-session lifecycle receipt — the provider-authored, immutable
/// result of one lifecycle command.
///
/// Regular stored event (append-only), channel-scoped via an `h` tag. Ordered
/// tags: `h`, `cslr-v`, `csl-command`, `csl-key`. See `docs/nips/NIP-CSL.md`.
pub const KIND_CODING_SESSION_LIFECYCLE_RECEIPT: u32 = 44224;

/// NIP-CST: Coding-session transcript item — one provider-authored, sequenced
/// step of one exact session generation.
///
/// Regular stored event (append-only), channel-scoped via an `h` tag. Ordered
/// tags: `h`, `cst-v`, `cs-target`, `cst-seq`, `cst-key`. Sequence numbers are
/// monotonic per (session, generation); gaps are permitted, duplicates are not.
/// See `docs/nips/NIP-CST.md`.
pub const KIND_CODING_SESSION_TRANSCRIPT: u32 = 44225;

/// NIP-CSG: Coding-session genesis — the operator-signed origin of one umbrella
/// session.
///
/// Regular stored event (append-only), channel-scoped via an `h` tag. Content is
/// a public JSON [`crate::coding_session_genesis::CodingSessionGenesisPayload`]
/// carrying only a `sessionRef` and a schema version. Ordered tags: `h`,
/// `csg-v`, `csg-session`. The signer is the session's founder — the authority
/// every later session operation resolves back to — so this kind is signed by
/// the human operator, never by a provider. Canonical identity is this event's
/// **id**, not the `csg-session` tag: the tag exists for the relay's uniqueness
/// probe and for diagnostics, and consumers must never select a founder by it.
/// See `docs/nips/NIP-CSG.md`.
pub const KIND_CODING_SESSION_GENESIS: u32 = 44226;

/// NIP-CSG: Coding-session goal — one human-authored revision of the umbrella
/// session's durable goal.
///
/// Regular stored event (append-only), channel-scoped via `h`, with
/// `d=sessionRef` for lookup and grouping. Content is raw prose. Ordered tags:
/// `h`, `d`, `csgl-v`. Consumers retain every revision and fold latest by
/// `(created_at, event id)`. See `docs/nips/NIP-CSG.md`.
pub const KIND_CODING_SESSION_GOAL: u32 = 44227;

/// NIP-CSAT: Coding-session authority transition — one append-only step of a
/// session's authority chain.
///
/// Regular stored event (append-only), channel-scoped via an `h` tag.
/// Content is a public JSON
/// [`crate::coding_session_authority_transition::CodingSessionAuthorityTransitionPayload`]
/// naming the session's genesis by event id, the previous accepted
/// transition (or `null` for the chain's first link), a sequence number
/// starting at 1, one of `grant-operator`, `grant-viewer`, `revoke`,
/// `grant-seat`, or `revoke-seat`, and the grantee pubkey. Seat transitions
/// also carry an exact normalized role slug. Ordered tags: `h`, `csat-v`,
/// `csat-genesis`. The relay validates linkage and signer standing atomically
/// with storage: legacy grant/revoke transitions are founder-only; seat
/// transitions may be signed by the founder, an active steering operator, or
/// an active lead, but a lead may manage only non-lead seats. On acceptance
/// the relay publishes a signed kind 40099 receipt naming the new canonical
/// head. See `docs/nips/NIP-CSAT.md`.
pub const KIND_CODING_SESSION_AUTHORITY_TRANSITION: u32 = 44228;

/// NIP-CSN: Coding-session name — one human-authored revision of the umbrella
/// session's short navigation label.
///
/// Regular stored event (append-only), channel-scoped via `h`, with
/// `d=sessionRef` for lookup and grouping. Content is single-line text.
/// Ordered tags: `h`, `d`, `csnm-v`. Consumers retain every revision and fold
/// latest by `(created_at, event id)`. See `docs/nips/NIP-CSG.md`.
pub const KIND_CODING_SESSION_NAME: u32 = 44229;

/// Coding-session closure — one append-only shared close/reopen revision of an
/// umbrella session, explicitly rooted at its genesis event.
///
/// Regular stored event, channel-scoped via `h`, with `d=sessionRef` for
/// grouping. Content is strict public JSON naming `closed` or `open` and the
/// canonical genesis event id. See `docs/nips/NIP-CSG.md`.
pub const KIND_CODING_SESSION_CLOSURE: u32 = 44230;

/// NIP-CSTX: Coding-session team transaction — one typed semantic record in a
/// multi-participant session.
///
/// Regular stored event (append-only), channel-scoped via `h`, with
/// `d=sessionRef` for grouping. Content is strict public JSON carrying one of
/// the closed v1 operations: assignment, report, verdict, acknowledgement,
/// mission.completed, or mission.blocked. Ordered tags: `h`, `d`, `cstx-v`,
/// `cstx-genesis`, `cstx-type`. The event signature is the only author field.
/// See `docs/nips/NIP-CSTX.md`.
pub const KIND_CODING_SESSION_TEAM_TRANSACTION: u32 = 44244;

/// NIP-CSP: Coding-session policy — how one umbrella mission is meant to be
/// run.
///
/// Regular stored event (append-only), channel-scoped via `h`, with
/// `d=sessionRef` for lookup and grouping. Content is strict public JSON
/// ([`crate::coding_session_policy::CodingSessionPolicyPayload`]) carrying the
/// posture, budget, attention level, gates, bench, irreversible acts and stop
/// conditions — every one of them optional, and omitted rather than written as
/// an explicit `null`. Ordered tags: `h`, `d`, `csp-v`, `csp-genesis`.
/// Consumers retain every revision and fold the newest accepted one, exactly
/// as 44227/44229/44230 do; the signature is the author, and whether that
/// author held the standing to set policy is the consumer's fold to answer
/// against the accepted NIP-CSAT chain — the relay validates structure only.
///
/// **Allocation.** 44245 is the lowest unused and unreserved kind in this fork
/// and in vanilla: 44231 (checkpoint), 44232 (native snapshot), 44233/44234
/// (git transition/check) and 44235–44239 (headroom) are reserved by the
/// continuity research, 44240 is the shipped Pulse entry with 44241–44243
/// reserved by the Pulse plan, and 44244 is the team transaction.
///
/// The wire contract is `docs/nips/NIP-CSP.md`, the companion to
/// `NIP-CSTX.md`. See `docs/design/portable-team-loop/POLICY.md` for each
/// field's consumer and why v1 rejects unknown fields rather than ignoring
/// them.
pub const KIND_CODING_SESSION_POLICY: u32 = 44245;

/// NIP-CSOB: Coding-session observation — one thing a seat saw while working.
///
/// Regular stored event (append-only), channel-scoped via `h`, with
/// `d=sessionRef` for grouping. Content is strict public JSON
/// ([`crate::coding_session_observation::CodingSessionObservationPayload`])
/// carrying exactly one of four closed types — `checkpoint`, `gate`,
/// `finding`, `phase` — and a pointer-only `assignmentRef`. Ordered tags: `h`,
/// `d`, `csob-v`, `csob-genesis`, `csob-type`. The signature is the author;
/// **any active seat or the founder may observe, and the relay validates
/// structure only**, the same division 44244 and 44245 draw.
///
/// **Why this is not four more 44244 subtypes.** Two facts about that kind's
/// fold decided it, and both are citable. Its operation vocabulary is a closed
/// serde enum with no `other` arm, and `fold_coding_session_team_transactions`
/// returns `Err` for the **whole set** when an envelope fails to validate, so a
/// build predating a new token
/// reads a session carrying one gate row as a broken mission — the finding-13
/// cliff, this time on the stream every seat writes many times an hour, while
/// a build that never heard of 44246 simply never queries it. And that fold is
/// bounded for a handful of assignments: hundreds of observations would evict
/// the records mission state depends on. An observation also carries no
/// authority, supersession or causal reference, so 44244's correction
/// validator and twelve exclusion codes buy nothing and would let an
/// observation's defect become a governance disclosure.
///
/// **Allocation.** 44246 is the lowest unused and unreserved kind in this fork
/// and in vanilla: 44231 (checkpoint), 44232 (native snapshot), 44233/44234
/// (git transition/check) and 44235–44239 (headroom) are reserved by the
/// continuity research, 44240 is the shipped Pulse entry with 44241–44243
/// reserved by the Pulse plan, 44244 is the team transaction and 44245 the
/// policy. A grep for `44246` over this tree and over `vanilla/main` matched
/// nothing but lockfile hashes before this constant.
///
/// The wire contract is `docs/nips/NIP-CSOB.md`.
pub const KIND_CODING_SESSION_OBSERVATION: u32 = 44246;

/// NIP-CSH: Coding-session handover — a durable checkpoint of the work, or the
/// record of a claimant continuing it.
///
/// Regular stored event (append-only), channel-scoped via `h`, with
/// `d=sessionRef` for grouping. Content is strict public JSON
/// ([`crate::coding_session_handover::CodingSessionHandoverPayload`]) carrying
/// exactly one of two closed types — `checkpoint` (the accepted task,
/// decisions, revision, artifacts, tests, unresolved questions, next action
/// and what could not be preserved) or `continuation` (which claim was acted
/// on, whether the work was resumed natively or reconstructed, and on which
/// execution). Ordered tags: `h`, `d`, `csh-v`, `csh-genesis`, `csh-type`.
///
/// **The relay validates structure only**, exactly as it does for 44244,
/// 44245 and 44246. Whether the author held standing to checkpoint, and
/// whether a continuation's `claimRef` is the claim in force, are the
/// consuming fold's questions against the accepted NIP-CSAT chain
/// ([`crate::coding_session_handover_fold`]) — a relay that adjudicated them
/// would be asserting standing it cannot verify.
///
/// **Why this is not a 44246 observation.** An observation settles nothing and
/// can deny nothing; a checkpoint is the thing another participant
/// *reconstructs work from*, and a continuation is the record of somebody
/// having taken the session over. Those are read against authority, retained
/// unbounded per umbrella rather than bounded per author-and-gate, and
/// excluded by name when their author lacked standing — none of which 44246's
/// fold does or should do.
///
/// **Allocation.** 44247 is the lowest unused and unreserved kind in this fork
/// and in vanilla: 44231 (checkpoint), 44232 (native snapshot), 44233/44234
/// (git transition/check) and 44235–44239 (headroom) are reserved by the
/// continuity research, 44240 is the shipped Pulse entry with 44241–44243
/// reserved by the Pulse plan, 44244 is the team transaction, 44245 the policy
/// and 44246 the observation. Both greps were run on 2026-09-08 before this
/// constant existed: `git grep -c 44247` over this tree matched nothing but
/// this kind's own contract document (`docs/HANDOVER_IMPL.md`, then untracked),
/// and `git grep 44247 vanilla/main` (`12201c49b`) matched nothing at all.
///
/// The wire contract is `docs/HANDOVER_IMPL.md` §2.
pub const KIND_CODING_SESSION_HANDOVER: u32 = 44247;

/// NIP-TD: Project to-do operation — one field-level edit to a shared,
/// project-scoped to-do list.
///
/// Regular stored event (append-only), scoped to exactly one NIP-MP project
/// by a canonical `a` coordinate and **never** to a channel: an `h` tag is
/// rejected at ingest. Content is strict public JSON
/// ([`crate::project_todo::ProjectTodoOp`]) naming one operation on one list
/// or one item — create/title/archived for a list; add/text/done/assignee/
/// due/rank/remove for an item. Tags are position-independent with a closed
/// key set: `a`, `td-v`, `td-op`, `td-list`, and `td-item` on item ops; the
/// three `td-*` values must equal the content's `op`, `listId` and `itemId`.
///
/// Deliberately **not** parameterized-replaceable. A shared list has many
/// writers, and NIP-33 replacement keys on `(kind, pubkey, d)` — one head per
/// author — so a replaceable kind could not express "any collaborator edits
/// any item" without a relay-side cross-author head (the 30624 conditional
/// path, ledger 113). Every op instead sets exactly one field, and the fold
/// ([`crate::project_todo_fold`], pinned by `conformance/project-todo-fold/`)
/// takes the latest `(created_at, id)` per field, so two people editing
/// different fields of one item both survive.
///
/// **Allocation.** 44248 is the lowest unused and unreserved kind in this fork
/// and in vanilla: 44231–44239 are reserved by the continuity research, 44240
/// is the shipped Pulse entry with 44241–44243 reserved by the Pulse plan, and
/// 44244–44247 are the team transaction, policy, observation and handover.
/// Both greps were run on 2026-09-17 before this constant existed:
/// `git grep 44248` over this tree matched nothing, and
/// `git grep 44248 vanilla/main` (`12201c49b`) matched only unrelated hashes
/// inside `uv.lock` files.
///
/// The wire contract is `docs/nips/NIP-TD.md`.
pub const KIND_PROJECT_TODO_OP: u32 = 44248;

// Forum / social (45000–45999)
// V1 used addressable range (30001–30003) — wrong.
/// A forum post (thread root).
pub const KIND_FORUM_POST: u32 = 45001;
/// A vote on a forum post.
pub const KIND_FORUM_VOTE: u32 = 45002;
/// A comment reply on a forum post.
pub const KIND_FORUM_COMMENT: u32 = 45003;

// Workflow engine (46000–46999)
/// Trigger workflow execution.
pub const KIND_WORKFLOW_TRIGGER: u32 = 46020;
/// Grant pending approval.
pub const KIND_APPROVAL_GRANT: u32 = 46030;
/// Deny pending approval.
pub const KIND_APPROVAL_DENY: u32 = 46031;
/// A workflow was triggered by a matching event.
pub const KIND_WORKFLOW_TRIGGERED: u32 = 46001;
/// A workflow step began execution.
pub const KIND_WORKFLOW_STEP_STARTED: u32 = 46002;
/// A workflow step completed successfully.
pub const KIND_WORKFLOW_STEP_COMPLETED: u32 = 46003;
/// A workflow step failed.
pub const KIND_WORKFLOW_STEP_FAILED: u32 = 46004;
/// The entire workflow completed successfully.
pub const KIND_WORKFLOW_COMPLETED: u32 = 46005;
/// The entire workflow failed.
pub const KIND_WORKFLOW_FAILED: u32 = 46006;
/// The workflow was cancelled before completion.
pub const KIND_WORKFLOW_CANCELLED: u32 = 46007;
/// Relay-signed immutable terminal result for one exact external CI run attempt.
pub const KIND_CI_RESULT: u32 = 46008;
/// A workflow step is waiting for human approval.
pub const KIND_WORKFLOW_APPROVAL_REQUESTED: u32 = 46010;
/// A pending workflow approval was granted.
pub const KIND_WORKFLOW_APPROVAL_GRANTED: u32 = 46011;
/// A pending workflow approval was denied.
pub const KIND_WORKFLOW_APPROVAL_DENIED: u32 = 46012;
/// Relay-signed request for an operator's host to execute one `run_on_host`
/// workflow step. Carries no command text: the host recompiles the step from
/// the project's own `actions.yml` (its agents repository) and refuses on hash drift.
pub const KIND_WORKFLOW_HOST_STEP_REQUESTED: u32 = 46013;
/// Relay-signed echo of the accepted host result for one `run_on_host` step,
/// plus which host claimed it.
pub const KIND_WORKFLOW_HOST_STEP_EXITED: u32 = 46014;
/// Host-signed claim of one kind:46013 request. Exactly one host wins.
pub const KIND_HOST_STEP_CLAIM: u32 = 46022;
/// Host-signed terminal result for one claimed kind:46013 request.
pub const KIND_HOST_STEP_RESULT: u32 = 46023;
/// Relay-signed record that an autorun grant for a workflow definition was
/// granted or revoked (spec § 5.4).
pub const KIND_WORKFLOW_AUTORUN_CHANGED: u32 = 46015;
/// Operator-signed revocation of a workflow's autorun grants.
pub const KIND_WORKFLOW_AUTORUN_REVOKE: u32 = 46032;

// User groups (47000–47999)

// System / admin custom range (48000–48999)
/// An audit log entry was recorded.
pub const KIND_AUDIT_ENTRY: u32 = 48001;
/// A huddle (audio/video session) was started.
pub const KIND_HUDDLE_STARTED: u32 = 48100;
/// A participant joined a huddle.
pub const KIND_HUDDLE_PARTICIPANT_JOINED: u32 = 48101;
/// A participant left a huddle.
pub const KIND_HUDDLE_PARTICIPANT_LEFT: u32 = 48102;
/// A huddle ended.
pub const KIND_HUDDLE_ENDED: u32 = 48103;
/// Huddle channel guidelines/rules document.
pub const KIND_HUDDLE_GUIDELINES: u32 = 48106;

// Media (49000–49999)
/// Internal kind for media upload audit entries. Not a relay event kind.
pub const KIND_MEDIA_UPLOAD: u32 = 49001;

/// NIP-34: Repository announcement (parameterized replaceable, d-tag = repo-id).
pub const KIND_GIT_REPO_ANNOUNCEMENT: u32 = 30617;
/// NIP-34: Repository state — current branch/tag refs (parameterized replaceable, d-tag = repo-id).
pub const KIND_GIT_REPO_STATE: u32 = 30618;
/// NIP-34: Patch (git format-patch output).
pub const KIND_GIT_PATCH: u32 = 1617;
/// NIP-34: Pull request.
pub const KIND_GIT_PULL_REQUEST: u32 = 1618;
/// NIP-34: Pull request update (tip commit change).
pub const KIND_GIT_PR_UPDATE: u32 = 1619;
/// NIP-34: Issue.
pub const KIND_GIT_ISSUE: u32 = 1621;
/// NIP-34: Status — Open.
pub const KIND_GIT_STATUS_OPEN: u32 = 1630;
/// NIP-34: Status — Applied / Merged.
pub const KIND_GIT_STATUS_MERGED: u32 = 1631;
/// NIP-34: Status — Closed.
pub const KIND_GIT_STATUS_CLOSED: u32 = 1632;
/// NIP-34: Status — Draft.
pub const KIND_GIT_STATUS_DRAFT: u32 = 1633;

/// NIP-MP: Multi-repo project — a named grouping of `kind:30617` repository
/// announcements (parameterized replaceable, d=project slug).
///
/// Members are `a` tags holding `30617:<owner-hex>:<repo-d>` coordinates, so one
/// project may span repositories owned by different pubkeys. The signer gains no
/// authority over any member: push policy reads the repository's own
/// announcement, never a project. See `docs/nips/NIP-MP.md`.
pub const KIND_PROJECT: u32 = 30621;

/// NIP-ST: shared-terminal session announce (parameterized replaceable,
/// d = session id). Owner-authored; lists an open built-in-shell session in
/// its project's Terminals view so members can observe it read-only.
///
/// Tags: `a` = `30621:<owner>:<dtag>` project coordinate (required — a
/// session with no real project is never announced), `title` (≤200 chars),
/// `status` = `open`|`closed` (replace-latest lifecycle: rename/close/unshare
/// republish the same address), `dims` = `<rows>x<cols>`. Deliberately
/// carries no cwd or shell path. See `docs/nips/NIP-ST.md`.
pub const KIND_SHELL_SESSION: u32 = 30623;

/// NIP-PK: project pack source — where a project's persona packs live
/// (parameterized replaceable, `d` = the project coordinate
/// `30621:<owner-hex>:<slug>`).
///
/// Packs are trees of text, so they live in a git repository and this record
/// carries the pointer: `["repo", "30617:<owner-hex>:<id>"]`, exactly one of
/// `["ref", "refs/heads/main"]` or `["sha", "<40-hex>"]`, and an optional
/// `["path", "personas/roles"]`. Content is
/// `{"schema":"buzz-project-pack-source/v1","note":"…"}`.
///
/// The relay admits a write only from a founder of one of the project's
/// repositories (`crate::repository_founders`) or an Owner of the project —
/// a record that decides which code a seat runs is not community-writable.
/// Newest per `d` wins; removal is the kind-5 tombstone addressables already
/// honour. See `docs/nips/NIP-PK.md` and
/// [`crate::project_pack_source`].
pub const KIND_PROJECT_PACK_SOURCE: u32 = 30624;

/// A founder-signed repository rule record — `buzz-protect` rows for a
/// repository the signer does not own (parameterized replaceable,
/// `d` = `<repo-owner-hex>:<repo-id>`).
///
/// Finding 33's residual R2: rules live on the kind:30617 announcement, which
/// only its signer can rewrite, so a co-founder's `bee repos protect set`
/// published a second repository instead of changing the rules of the one
/// they co-founded. This kind carries the same `["buzz-protect", …]` rows at
/// an address keyed to the **author**, and the relay admits one only from a
/// founder of the repository the `d` tag names
/// ([`crate::repository_founders`]).
///
/// Content is `{"schema":"buzz-repo-protection/v1"}`. The push gate resolves
/// the announcement's rows and the newest record per founder with last-write-
/// wins per exact ref pattern; removal is the `none` rule token, and a kind-5
/// tombstone by a record's author retires that founder's rows. A repository
/// with no record of this kind is governed by its announcement exactly as
/// before. See `docs/nips/NIP-GS.md` § "A founder-signed rule record" and
/// [`crate::repository_protection`].
pub const KIND_GIT_REPO_PROTECTION: u32 = 30625;

/// Tag carrying a project's access level (Buzz container extension).
///
/// `["buzz-access", "private"]` restricts the project container to its author
/// plus the pubkeys listed in the event's `p` tags; `["buzz-access", "public"]`
/// or an absent tag means community-readable (the default, matching all
/// pre-extension events). Distinct from `buzz-visibility` (listed/unlisted),
/// which is a client-side display filter and grants no access control.
///
/// Ingest enforces a singleton tag with exactly these two values; unknown
/// values are rejected rather than falling open to public — an access typo
/// must not silently publish a private project.
pub const PROJECT_ACCESS_TAG: &str = "buzz-access";
/// `buzz-access` value restricting the project to author + `p`-tag members.
pub const PROJECT_ACCESS_PRIVATE: &str = "private";
/// `buzz-access` value (also the absent-tag default): community-readable.
pub const PROJECT_ACCESS_PUBLIC: &str = "public";

/// The community's shared default project dtag. Always public: ingest rejects
/// a kind:30621 head carrying `["buzz-access","private"]` with this `d` tag,
/// mirroring the client-side guard in `publishProjectContainer`.
pub const GENERAL_PROJECT_DTAG: &str = "general";

/// Project member role: full rights inside the project plus roster
/// management via [`KIND_PROJECT_PUT_MEMBER`]/[`KIND_PROJECT_REMOVE_MEMBER`].
/// The creator (the 30621 address pubkey) is always an implicit owner.
pub const PROJECT_ROLE_OWNER: &str = "owner";
/// Project member role: read everything, write into project contents
/// (channels, own sessions, repos), no roster management. The default for
/// legacy role-less `p` tags — pre-role members could already write.
pub const PROJECT_ROLE_COLLABORATOR: &str = "collaborator";
/// Project member role: read-only across the project and its contents.
pub const PROJECT_ROLE_VIEWER: &str = "viewer";

/// The pinned project role vocabulary, in descending capability order.
///
/// Ingest validates every role-carrying tag element against this list and
/// rejects unknown values — a role typo must not silently grant or deny.
pub const PROJECT_ROLES: &[&str] = &[
    PROJECT_ROLE_OWNER,
    PROJECT_ROLE_COLLABORATOR,
    PROJECT_ROLE_VIEWER,
];

/// Returns `true` when `value` is a pinned [`PROJECT_ROLES`] entry.
pub fn is_valid_project_role(value: &str) -> bool {
    PROJECT_ROLES.contains(&value)
}

/// Shared-terminal roster role: may watch AND type into the owner's PTY via
/// [`KIND_SHELL_INPUT`]. Listed as `["p", <hex>, "", "collaborator"]` on the
/// owner-signed kind:30623 announce (the owner signs and is never listed).
pub const SHELL_ROLE_COLLABORATOR: &str = "collaborator";
/// Shared-terminal roster role: watch-only, independent of project
/// membership (lets an owner share a terminal with someone outside the
/// project, or share without enabling project-wide observe).
pub const SHELL_ROLE_VIEWER: &str = "viewer";

/// The pinned shared-terminal roster role vocabulary.
pub const SHELL_ROLES: &[&str] = &[SHELL_ROLE_COLLABORATOR, SHELL_ROLE_VIEWER];

/// Returns `true` when `value` is a pinned [`SHELL_ROLES`] entry.
pub fn is_valid_shell_role(value: &str) -> bool {
    SHELL_ROLES.contains(&value)
}

/// Returns `true` if the event is a project container marked private.
///
/// Fails closed: any `buzz-access` tag whose value is `"private"` marks the
/// event private regardless of extra tag elements or duplicate tags —
/// ingest rejects those shapes, but a malformed head must hide, not leak.
pub fn is_private_project_event(event: &nostr::Event) -> bool {
    if event_kind_u32(event) != KIND_PROJECT {
        return false;
    }
    event.tags.iter().any(|tag| {
        let parts = tag.as_slice();
        parts.len() >= 2
            && parts[0].as_str() == PROJECT_ACCESS_TAG
            && parts[1].as_str() == PROJECT_ACCESS_PRIVATE
    })
}

/// The project's own coordinate, `30621:<owner-hex>:<d>`, for a kind:30621
/// head. `None` for other kinds or when the `d` tag is absent — the caller
/// treats that as gate-closed, matching
/// [`project_membership_event_coordinate`].
pub fn project_head_coordinate(event: &nostr::Event) -> Option<String> {
    if event_kind_u32(event) != KIND_PROJECT {
        return None;
    }
    let d = nostr::SingleLetterTag::lowercase(nostr::Alphabet::D);
    event
        .tags
        .filter(nostr::TagKind::SingleLetter(d))
        .find_map(|t| t.content())
        .map(|dtag| format!("{KIND_PROJECT}:{}:{dtag}", event.pubkey.to_hex()))
}

/// Returns `true` if the event is a private project container that must be
/// withheld from this reader: kind 30621 with `["buzz-access","private"]`
/// whose coordinate is absent from the reader's admitted set, and which the
/// reader did not author.
///
/// `admitted_private_projects` is the reader's DB-resolved grant set
/// (`buzz_db::git_repo::HiddenRepos::admitted_private_projects`, read off the
/// project ACL projection, so it follows kind:9010/9011 roster ops). The
/// head's own `p` tags are **not** consulted: they are the bootstrap roster
/// only, and NIP-MP § Relay-managed membership makes them inert for access
/// decisions the moment the first op is accepted. Reading them here would
/// both miss every ops-added member and keep serving a member a 9011 removed,
/// since nothing can rewrite a creator-signed event.
///
/// Fails closed: a private head with no resolvable coordinate, or one whose
/// ACL row never projected, admits nobody but its author.
///
/// This is the container half of project visibility (NIP-MP Buzz extension);
/// contents (channels/forums/repos/roster/Pulse) are gated by the same
/// projection through their own predicates. Enforced at every read chokepoint
/// via `event_visible_to_reader`, plus a dedicated live fan-out branch in the
/// relay that resolves the gate straight from the ACL row.
pub fn project_container_hidden_from(
    event: &nostr::Event,
    reader_pubkey_hex: &str,
    admitted_private_projects: &std::collections::HashSet<String>,
) -> bool {
    if !is_private_project_event(event) {
        return false;
    }
    // Author reads are always allowed.
    if event
        .pubkey
        .to_hex()
        .eq_ignore_ascii_case(reader_pubkey_hex)
    {
        return false;
    }
    match project_head_coordinate(event) {
        Some(coordinate) => !admitted_private_projects.contains(&coordinate),
        None => true,
    }
}

/// Returns `true` if a project-membership event must be withheld from this
/// reader: a kind 9010/9011 op (project coordinate in its `a` tag) or a
/// kind:39010 roster projection (coordinate in its `d` tag) whose project is
/// in the reader's hidden-private-project set. Op authors always read their
/// own ops; a membership event with no resolvable coordinate hides from
/// every non-author (fail closed — ingest rejects the shape, but a smuggled
/// event must not leak). An empty hidden set (the common case) hides
/// nothing.
pub fn project_membership_event_hidden_from(
    event: &nostr::Event,
    reader_pubkey_hex: &str,
    hidden_project_coordinates: &std::collections::HashSet<String>,
) -> bool {
    if !is_project_membership_kind(event_kind_u32(event)) {
        return false;
    }
    if event
        .pubkey
        .to_hex()
        .eq_ignore_ascii_case(reader_pubkey_hex)
    {
        return false;
    }
    match project_membership_event_coordinate(event) {
        Some(coord) => hidden_project_coordinates.contains(&coord),
        None => true,
    }
}

/// The project coordinate a membership event is scoped to: the `a` tag of a
/// kind 9010/9011 op, or the `d` tag of a kind:39010 roster projection.
/// `None` for other kinds or when the tag is absent (gate closed, not open).
pub fn project_membership_event_coordinate(event: &nostr::Event) -> Option<String> {
    let letter = match event_kind_u32(event) {
        KIND_PROJECT_PUT_MEMBER | KIND_PROJECT_REMOVE_MEMBER => nostr::Alphabet::A,
        KIND_PROJECT_MEMBERS => nostr::Alphabet::D,
        _ => return None,
    };
    let tag = nostr::SingleLetterTag::lowercase(letter);
    event
        .tags
        .filter(nostr::TagKind::SingleLetter(tag))
        .find_map(|t| t.content().map(str::to_string))
}

/// Returns `true` for the project-membership kinds (ops + roster
/// projection) gated by [`project_membership_event_hidden_from`].
pub const fn is_project_membership_kind(kind: u32) -> bool {
    matches!(
        kind,
        KIND_PROJECT_PUT_MEMBER | KIND_PROJECT_REMOVE_MEMBER | KIND_PROJECT_MEMBERS
    )
}

/// Returns `true` for the NIP-ST shared-terminal kinds (session announce,
/// watch, frame). All three carry the session's project coordinate in a
/// single `a` tag and are membership-gated when that project is private —
/// at ingest (publisher must be admitted), at live fan-out
/// (`filter_fanout_by_access`), and, for the stored 30623, at every read
/// chokepoint via [`shell_session_hidden_from`].
pub const fn is_shell_observe_kind(kind: u32) -> bool {
    matches!(
        kind,
        KIND_SHELL_SESSION | KIND_SHELL_WATCH | KIND_SHELL_FRAME
    )
}

/// The project coordinate a NIP-ST shared-terminal event is scoped to: the
/// content of its single `a` tag. Returns `None` when absent (ingest rejects
/// that shape, but a malformed event must gate closed, not open).
pub fn shell_observe_project_ref(event: &nostr::Event) -> Option<String> {
    let a = nostr::SingleLetterTag::lowercase(nostr::Alphabet::A);
    event
        .tags
        .filter(nostr::TagKind::SingleLetter(a))
        .find_map(|t| t.content().map(str::to_string))
}

/// Kinds scoped to a NIP-MP project by a required, canonical, singleton `a`
/// tag and gated by project membership alone — no channel, no per-event
/// roster. Today: the Pulse entry (44240) and the to-do op (44248). Every
/// relay chokepoint that gates on a project's hidden set keys on this
/// predicate, so a new member of the set inherits ingest admission, the
/// stored-read gate, live fan-out filtering, the SQL pushdown and the HTTP
/// request-shape rule without a per-kind arm.
pub const fn is_project_a_scoped_kind(kind: u32) -> bool {
    matches!(kind, KIND_PULSE_ENTRY | KIND_PROJECT_TODO_OP)
}

/// The members of [`is_project_a_scoped_kind`], for callers that must bind a
/// list (the SQL pushdown in `buzz_db::event`). Keep the two in step; the
/// test `project_a_scoped_kinds_agree` pins it.
pub const PROJECT_A_SCOPED_KINDS: &[u32] = &[KIND_PULSE_ENTRY, KIND_PROJECT_TODO_OP];

/// The project coordinate a project-`a`-scoped event is scoped to,
/// normalized to `30621:<lowercase-hex>:<dtag>`.
///
/// Parsing is tolerant of hex case (like [`git_event_repo_names`]) so a
/// smuggled case-variant coordinate cannot dodge the per-event read gate,
/// even though ingest requires the tag to already be canonical and the SQL
/// containment probe matches exact bytes. Returns `None` when the event does
/// not carry exactly one well-formed `a` tag — the gate closes, it does not
/// open.
pub fn project_a_scoped_coordinate(event: &nostr::Event) -> Option<String> {
    let a = nostr::SingleLetterTag::lowercase(nostr::Alphabet::A);
    let mut values = event
        .tags
        .filter(nostr::TagKind::SingleLetter(a))
        .filter_map(|tag| tag.content());
    let first = values.next()?;
    if values.next().is_some() {
        return None;
    }
    normalize_project_coordinate(first)
}

/// Returns `true` if a stored kind:30623 session announce must be withheld
/// from this reader: its project coordinate is in the reader's
/// hidden-private-project set (resolved per reader by
/// `buzz_db::git_repo::hidden_repos_for_reader`) and the reader is neither
/// the announce's author nor on its roster (`p` tags — a per-session invite
/// grants the announce even to readers outside the project). An empty set
/// (the common case) hides nothing; a 30623 with no `a` tag hides from
/// every non-author (fail closed — ingest rejects the shape, but a smuggled
/// head must not leak).
pub fn shell_session_hidden_from(
    event: &nostr::Event,
    reader_pubkey_hex: &str,
    hidden_project_coordinates: &std::collections::HashSet<String>,
) -> bool {
    if event_kind_u32(event) != KIND_SHELL_SESSION {
        return false;
    }
    if event
        .pubkey
        .to_hex()
        .eq_ignore_ascii_case(reader_pubkey_hex)
    {
        return false;
    }
    if shell_session_roster(event)
        .iter()
        .any(|(pubkey, _)| pubkey.eq_ignore_ascii_case(reader_pubkey_hex))
    {
        return false;
    }
    match shell_observe_project_ref(event) {
        Some(coord) => hidden_project_coordinates.contains(&coord),
        None => true,
    }
}

/// Returns `true` if a stored project-`a`-scoped event (kind:44240 Pulse
/// entry or kind:44248 to-do op, [`is_project_a_scoped_kind`]) must be
/// withheld from this reader: its project coordinate is in the reader's
/// hidden-private-project set (resolved per reader by
/// `buzz_db::git_repo::hidden_repos_for_reader`) and the reader is not its
/// author. An empty set (the common case) hides nothing; an event with no
/// resolvable coordinate hides from every non-author (fail closed — ingest
/// rejects that shape, but a smuggled event must not leak).
///
/// These kinds carry no roster: unlike a 30623 announce, there is no
/// per-event invite that grants a reader outside the project. Project
/// membership is the only key.
pub fn project_a_scoped_event_hidden_from(
    event: &nostr::Event,
    reader_pubkey_hex: &str,
    hidden_project_coordinates: &std::collections::HashSet<String>,
) -> bool {
    if !is_project_a_scoped_kind(event_kind_u32(event)) {
        return false;
    }
    if event
        .pubkey
        .to_hex()
        .eq_ignore_ascii_case(reader_pubkey_hex)
    {
        return false;
    }
    // A personal to-do op is its author's alone, whatever the project's
    // visibility: the author check above is the only way through.
    if is_personal_todo_op(event) {
        return true;
    }
    match project_a_scoped_coordinate(event) {
        Some(coord) => hidden_project_coordinates.contains(&coord),
        None => true,
    }
}

/// The `td-vis` tag value on a to-do op, and `true` when it says `personal`.
///
/// Read from the tag, never from content: the relay gates personal ops at
/// every read chokepoint and in live fan-out without parsing the payload.
/// A 44248 with no `td-vis` tag cannot be stored (ingest requires it), so
/// a stored op without one is treated as project-visible rather than
/// hidden — nothing to protect, and the fold ignores it anyway.
pub fn is_personal_todo_op(event: &nostr::Event) -> bool {
    event_kind_u32(event) == KIND_PROJECT_TODO_OP
        && event.tags.iter().any(|tag| {
            let parts = tag.as_slice();
            parts.len() == 2 && parts[0] == "td-vis" && parts[1] == "personal"
        })
}

/// The Pulse-only spelling of [`project_a_scoped_event_hidden_from`], kept
/// for the existing call sites and tests; the gate is the same function.
pub fn pulse_entry_hidden_from(
    event: &nostr::Event,
    reader_pubkey_hex: &str,
    hidden_project_coordinates: &std::collections::HashSet<String>,
) -> bool {
    project_a_scoped_event_hidden_from(event, reader_pubkey_hex, hidden_project_coordinates)
}

/// The roster of a kind:30623 announce: every `["p", <hex>, <hint>, <role>]`
/// tag whose role is a pinned [`SHELL_ROLES`] value, as
/// `(pubkey_hex, role)` pairs. Tags with an unknown or missing role are
/// skipped (never a silent grant); other kinds return an empty roster.
pub fn shell_session_roster(event: &nostr::Event) -> Vec<(String, &'static str)> {
    if event_kind_u32(event) != KIND_SHELL_SESSION {
        return Vec::new();
    }
    event
        .tags
        .iter()
        .filter_map(|tag| {
            let parts = tag.as_slice();
            if parts.first().map(String::as_str) != Some("p") {
                return None;
            }
            let pubkey = parts.get(1)?.as_str();
            let role = match parts.get(3).map(String::as_str) {
                Some(r) if r == SHELL_ROLE_COLLABORATOR => SHELL_ROLE_COLLABORATOR,
                Some(r) if r == SHELL_ROLE_VIEWER => SHELL_ROLE_VIEWER,
                _ => return None,
            };
            Some((pubkey.to_string(), role))
        })
        .collect()
}

/// Kinds whose visibility follows the repo → project link (NIP-MP Buzz
/// access extension, phase 2): the NIP-34 repo surface and relay-signed facts
/// explicitly bound to a repository. When a repo's 30617 carries a
/// `["project", …]` back-reference into a private project, the announcement
/// itself, the relay-signed 30618 ref state, and every child event that
/// `a`-tags the repo are hidden from readers outside the project.
///
/// Enforcement is DB-backed (the project's ACL lives on a different event),
/// so unlike [`SHARED_GATED_KINDS`] there is no stateless per-event
/// predicate: the relay resolves a per-reader hidden-repo set
/// (`buzz_db::git_repo::hidden_repos_for_reader`, cached) and applies
/// [`repo_event_hidden_from`] at each chokepoint, plus an SQL pushdown
/// (`EventQuery::git_gated_reader`).
pub const GIT_PROJECT_GATED_KINDS: &[u32] = &[
    KIND_GIT_REPO_ANNOUNCEMENT,
    KIND_GIT_REPO_STATE,
    KIND_GIT_PATCH,
    KIND_GIT_PULL_REQUEST,
    KIND_GIT_PR_UPDATE,
    KIND_GIT_ISSUE,
    KIND_GIT_STATUS_OPEN,
    KIND_GIT_STATUS_MERGED,
    KIND_GIT_STATUS_CLOSED,
    KIND_GIT_STATUS_DRAFT,
    KIND_CI_RESULT,
];

/// Returns `true` for kinds in [`GIT_PROJECT_GATED_KINDS`].
pub fn is_git_project_gated_kind(kind: u32) -> bool {
    GIT_PROJECT_GATED_KINDS.contains(&kind)
}

/// Extract and normalize the `["project", …]` back-reference from a repo
/// announcement (kind:30617): `Some("30621:<lowercase-hex-owner>:<dtag>")`
/// when the first `project` tag carries a well-formed project coordinate,
/// else `None`.
///
/// Normalization lowercases the owner hex, matching the projection in
/// migration 0031. Malformed values return `None` — fail-open to public for
/// legacy stored events (they predate gating and were always visible);
/// go-forward ingest rejects malformed tags outright.
pub fn repo_project_ref(event: &nostr::Event) -> Option<String> {
    if event_kind_u32(event) != KIND_GIT_REPO_ANNOUNCEMENT {
        return None;
    }
    let value = event.tags.iter().find_map(|tag| {
        let parts = tag.as_slice();
        if parts.first().map(String::as_str) == Some("project") {
            parts.get(1).map(String::as_str)
        } else {
            None
        }
    })?;
    normalize_project_coordinate(value)
}

/// Normalize a project coordinate string to `30621:<lowercase-hex>:<dtag>`,
/// or `None` if it is not a well-formed 30621 coordinate. Shared by
/// [`repo_project_ref`] and the ingest-side validation so they cannot drift.
///
/// Only the live project kind is accepted: this relay never stored the
/// fork-era `30178:` project coordinates, and 30178 is the team-catalog kind
/// here — accepting it would alias an unrelated kind into project refs.
pub fn normalize_project_coordinate(value: &str) -> Option<String> {
    let mut parts = value.splitn(3, ':');
    let kind = parts.next()?;
    let pubkey = parts.next()?;
    let dtag = parts.next()?;
    if kind != "30621" {
        return None;
    }
    if pubkey.len() != 64 || !pubkey.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    if dtag.is_empty() || dtag.chars().count() > 64 || dtag.chars().any(char::is_control) {
        return None;
    }
    Some(format!("30621:{}:{dtag}", pubkey.to_ascii_lowercase()))
}

/// The community-unique repo names a git-gated event belongs to.
///
/// - kind 30617/30618: the event's own `d` tag (repo names are unique per
///   community, so the name alone identifies the repo — necessary for 30618,
///   which is relay-signed and carries no owner reference);
/// - child kinds (patches/PRs/issues/status and CI results): the `<repo-d>`
///   segment of every well-formed `30617:<hex>:<repo-d>` `a` tag.
///
/// Parsing is tolerant (hex case-insensitive) so a case-variant coordinate
/// cannot dodge the per-event check even though the SQL pushdown's exact
/// containment probe would miss it.
pub fn git_event_repo_names(event: &nostr::Event) -> Vec<String> {
    let kind = event_kind_u32(event);
    if !is_git_project_gated_kind(kind) {
        return Vec::new();
    }
    if kind == KIND_GIT_REPO_ANNOUNCEMENT || kind == KIND_GIT_REPO_STATE {
        let d = nostr::SingleLetterTag::lowercase(nostr::Alphabet::D);
        return event
            .tags
            .filter(nostr::TagKind::SingleLetter(d))
            .filter_map(|t| t.content())
            .filter(|v| !v.is_empty())
            .map(str::to_owned)
            .take(1)
            .collect();
    }
    let a = nostr::SingleLetterTag::lowercase(nostr::Alphabet::A);
    event
        .tags
        .filter(nostr::TagKind::SingleLetter(a))
        .filter_map(|t| t.content())
        .filter_map(|value| {
            let mut parts = value.splitn(3, ':');
            let kind = parts.next()?;
            let pubkey = parts.next()?;
            let repo = parts.next()?;
            if kind != "30617"
                || pubkey.len() != 64
                || !pubkey.bytes().all(|b| b.is_ascii_hexdigit())
                || repo.is_empty()
            {
                return None;
            }
            Some(repo.to_owned())
        })
        .collect()
}

/// Returns `true` if a git-gated event must be withheld from this reader:
/// the event belongs to (any) repo in the reader's hidden set and the reader
/// is not its author.
///
/// Both sets come from the per-reader resolution in
/// `buzz_db::git_repo::hidden_repos_for_reader`: `hidden_repo_names` holds
/// the names of repos linked to a private project that does not admit the
/// reader; `hidden_project_coordinates` holds the coordinates of those
/// private projects themselves, matched against a 30617's **own** `project`
/// tag — defense-in-depth for an announcement whose projection never landed
/// (failed side effect), whose intent is still legible on the event. Empty
/// sets (the common case) hide nothing.
pub fn repo_event_hidden_from(
    event: &nostr::Event,
    reader_pubkey_hex: &str,
    hidden_repo_names: &std::collections::HashSet<String>,
    hidden_project_coordinates: &std::collections::HashSet<String>,
) -> bool {
    let kind = event_kind_u32(event);
    if (hidden_repo_names.is_empty() && hidden_project_coordinates.is_empty())
        || !is_git_project_gated_kind(kind)
    {
        return false;
    }
    // Authors always see their own events.
    if event
        .pubkey
        .to_hex()
        .eq_ignore_ascii_case(reader_pubkey_hex)
    {
        return false;
    }
    if kind == KIND_GIT_REPO_ANNOUNCEMENT && !hidden_project_coordinates.is_empty() {
        if let Some(coord) = repo_project_ref(event) {
            if hidden_project_coordinates.contains(&coord) {
                return true;
            }
        }
    }
    git_event_repo_names(event)
        .iter()
        .any(|name| hidden_repo_names.contains(name))
}

/// All registered kind constants — used for duplicate detection and iteration.
pub const ALL_KINDS: &[u32] = &[
    KIND_PROFILE,
    KIND_TEXT_NOTE,
    KIND_CONTACT_LIST,
    KIND_MUTE_LIST,
    KIND_PIN_LIST,
    KIND_NIP65_RELAY_LIST_METADATA,
    KIND_BOOKMARK_LIST,
    KIND_EMOJI_LIST,
    KIND_FOLLOW_SET,
    KIND_BOOKMARK_SET,
    KIND_EMOJI_SET,
    KIND_CHANNEL_METADATA,
    KIND_DELETION,
    KIND_REACTION,
    KIND_GIFT_WRAP,
    KIND_FILE_METADATA,
    KIND_AGENT_PROFILE,
    KIND_AGENT_ENGRAM,
    KIND_EVENT_REMINDER,
    KIND_PERSONA,
    KIND_TEAM,
    KIND_MANAGED_AGENT,
    KIND_TEAM_CATALOG,
    KIND_PRIVATE_MANAGED_AGENT,
    KIND_REPORT,
    KIND_PRODUCT_FEEDBACK,
    KIND_NIP29_PUT_USER,
    KIND_NIP29_REMOVE_USER,
    KIND_NIP29_EDIT_METADATA,
    KIND_NIP29_DELETE_EVENT,
    KIND_NIP29_CREATE_GROUP,
    KIND_NIP29_DELETE_GROUP,
    KIND_NIP29_CREATE_INVITE,
    KIND_NIP29_JOIN_REQUEST,
    KIND_NIP29_LEAVE_REQUEST,
    KIND_PROJECT_PUT_MEMBER,
    KIND_PROJECT_REMOVE_MEMBER,
    KIND_MODERATION_BAN,
    KIND_MODERATION_UNBAN,
    KIND_MODERATION_TIMEOUT,
    KIND_MODERATION_UNTIMEOUT,
    KIND_MODERATION_RESOLVE_REPORT,
    RELAY_ADMIN_ADD_MEMBER,
    RELAY_ADMIN_REMOVE_MEMBER,
    RELAY_ADMIN_CHANGE_ROLE,
    RELAY_ADMIN_SET_WORKSPACE_PROFILE,
    KIND_NIP43_MEMBERSHIP_LIST,
    KIND_NIP43_MEMBER_ADDED,
    KIND_NIP43_MEMBER_REMOVED,
    KIND_NIP43_LEAVE_REQUEST,
    KIND_IA_ARCHIVE_REQUEST,
    KIND_IA_UNARCHIVE_REQUEST,
    KIND_IA_ARCHIVED,
    KIND_IA_UNARCHIVED,
    KIND_IA_ARCHIVED_LIST,
    KIND_NIP29_GROUP_METADATA,
    KIND_NIP29_GROUP_ADMINS,
    KIND_NIP29_GROUP_MEMBERS,
    KIND_NIP29_GROUP_ROLES,
    KIND_PROJECT_MEMBERS,
    KIND_THREAD_SUMMARY,
    KIND_WINDOW_BOUNDS,
    KIND_PRESENCE_UPDATE,
    KIND_TYPING_INDICATOR,
    KIND_HUDDLE_REACTION,
    KIND_BLOSSOM_AUTH,
    KIND_PAIRING,
    KIND_AGENT_OBSERVER_FRAME,
    KIND_CODING_SESSION_LEASE,
    KIND_HTTP_AUTH,
    KIND_STREAM_MESSAGE,
    KIND_STREAM_MESSAGE_V2,
    KIND_STREAM_MESSAGE_EDIT,
    KIND_STREAM_MESSAGE_PINNED,
    KIND_STREAM_MESSAGE_BOOKMARKED,
    KIND_STREAM_MESSAGE_SCHEDULED,
    KIND_STREAM_REMINDER,
    KIND_STREAM_MESSAGE_DIFF,
    KIND_CANVAS,
    KIND_SYSTEM_MESSAGE,
    KIND_CHANNEL_SUMMARY,
    KIND_PRESENCE_SNAPSHOT,
    KIND_DM_VISIBILITY,
    KIND_DM_OPEN,
    KIND_DM_ADD_MEMBER,
    KIND_DM_HIDE,
    KIND_DM_CREATED,
    KIND_JOB_REQUEST,
    KIND_JOB_ACCEPTED,
    KIND_JOB_PROGRESS,
    KIND_JOB_RESULT,
    KIND_JOB_CANCEL,
    KIND_JOB_ERROR,
    KIND_MEMBER_ADDED_NOTIFICATION,
    KIND_MEMBER_REMOVED_NOTIFICATION,
    KIND_AGENT_TURN_METRIC,
    KIND_PULSE_ENTRY,
    KIND_PROJECT_TODO_OP,
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
    KIND_WORKFLOW_DEF,
    KIND_LONG_FORM,
    KIND_USER_STATUS,
    KIND_READ_STATE,
    KIND_FORUM_POST,
    KIND_FORUM_VOTE,
    KIND_FORUM_COMMENT,
    KIND_WORKFLOW_TRIGGER,
    KIND_APPROVAL_GRANT,
    KIND_APPROVAL_DENY,
    KIND_WORKFLOW_TRIGGERED,
    KIND_WORKFLOW_STEP_STARTED,
    KIND_WORKFLOW_STEP_COMPLETED,
    KIND_WORKFLOW_STEP_FAILED,
    KIND_WORKFLOW_COMPLETED,
    KIND_WORKFLOW_FAILED,
    KIND_WORKFLOW_CANCELLED,
    KIND_CI_RESULT,
    KIND_WORKFLOW_APPROVAL_REQUESTED,
    KIND_WORKFLOW_APPROVAL_GRANTED,
    KIND_WORKFLOW_APPROVAL_DENIED,
    KIND_AUDIT_ENTRY,
    KIND_HUDDLE_STARTED,
    KIND_HUDDLE_PARTICIPANT_JOINED,
    KIND_HUDDLE_PARTICIPANT_LEFT,
    KIND_HUDDLE_ENDED,
    KIND_HUDDLE_GUIDELINES,
    KIND_MEDIA_UPLOAD,
    KIND_GIT_REPO_ANNOUNCEMENT,
    KIND_GIT_REPO_STATE,
    KIND_GIT_PATCH,
    KIND_GIT_PULL_REQUEST,
    KIND_GIT_PR_UPDATE,
    KIND_GIT_ISSUE,
    KIND_GIT_STATUS_OPEN,
    KIND_GIT_STATUS_MERGED,
    KIND_GIT_STATUS_CLOSED,
    KIND_GIT_STATUS_DRAFT,
    KIND_PROJECT,
    KIND_SHELL_SESSION,
    KIND_SHELL_WATCH,
    KIND_SHELL_FRAME,
    KIND_SHELL_INPUT,
];

/// Returns `true` if `kind` is in the ephemeral range (20000–29999).
pub const fn is_ephemeral(kind: u32) -> bool {
    kind >= EPHEMERAL_KIND_MIN && kind <= EPHEMERAL_KIND_MAX
}

/// Returns `true` if `kind` is replaceable (NIP-01: kinds 0, 3, 41, 10000–19999).
/// NIP-33 parameterized-replaceable kinds (30000–39999) use a different replacement
/// key (includes `d`-tag) and are handled separately via `replace_parameterized_event`.
pub const fn is_replaceable(kind: u32) -> bool {
    matches!(kind, 0 | 3 | KIND_CHANNEL_METADATA | 10000..=19999)
}

/// Returns `true` if `kind` is in the NIP-33 parameterized replaceable range (30000–39999).
///
/// These events are keyed by `(pubkey, kind, d_tag)` — the latest `created_at` wins.
pub const fn is_parameterized_replaceable(kind: u32) -> bool {
    kind >= PARAM_REPLACEABLE_KIND_MIN && kind <= PARAM_REPLACEABLE_KIND_MAX
}

/// Returns `true` if `kind` is a workflow execution event (46001–46015).
/// These must not trigger workflows (prevents infinite loops).
pub const fn is_workflow_execution_kind(kind: u32) -> bool {
    kind >= KIND_WORKFLOW_TRIGGERED && kind <= KIND_WORKFLOW_AUTORUN_CHANGED
}

/// Returns `true` if `kind` is a NIP-43 relay membership admin command (9030–9032)
/// or the Buzz workspace-profile admin command (9033).
pub const fn is_relay_admin_kind(kind: u32) -> bool {
    matches!(
        kind,
        RELAY_ADMIN_ADD_MEMBER
            | RELAY_ADMIN_REMOVE_MEMBER
            | RELAY_ADMIN_CHANGE_ROLE
            | RELAY_ADMIN_SET_WORKSPACE_PROFILE
    )
}

/// Returns `true` if `kind` is a NIP-IA identity archival request (9035–9036).
///
/// Only the user-signed *request* kinds are matched. The relay-signed delta and
/// snapshot kinds (8002/8003/13535) are emitted by the relay, never ingested as
/// commands, so they are intentionally excluded.
pub const fn is_identity_archive_request_kind(kind: u32) -> bool {
    matches!(kind, KIND_IA_ARCHIVE_REQUEST | KIND_IA_UNARCHIVE_REQUEST)
}

/// Returns `true` if `kind` is a Buzz command kind that requires transactional execution.
pub const fn is_command_kind(kind: u32) -> bool {
    matches!(
        kind,
        KIND_WORKFLOW_DEF
            | KIND_DM_OPEN
            | KIND_DM_ADD_MEMBER
            | KIND_DM_HIDE
            | KIND_WORKFLOW_TRIGGER
            | KIND_APPROVAL_GRANT
            | KIND_APPROVAL_DENY
            | KIND_HOST_STEP_CLAIM
            | KIND_HOST_STEP_RESULT
            | KIND_WORKFLOW_AUTORUN_REVOKE
    )
}

/// Returns `true` if `kind` may only be authored by the relay.
/// Client submission of these kinds must be rejected.
///
/// # Why kind 40099 is on this list
///
/// A system message is the relay **speaking as itself**: membership rows,
/// moderation tombstones, and — since the authority chain landed — the
/// receipts that say a 44228 link was accepted and, since the handover work,
/// that a whole session was deleted. Every reader in this tree already checks
/// the signer against the witnessed NIP-11 identity before believing one
/// (`buzz_session_provider::authority::verify_acceptance_receipt` and
/// `verify_deletion_receipt`), so a forged 40099 was never *exploitable*. It
/// was, however, **publishable**: any channel member could write bytes
/// claiming a session had been handed over or deleted, and every surface that
/// renders 40099 as a system row would have shown it. The deletion receipt
/// makes that worse than cosmetic — it is the one 40099 whose meaning is
/// "stop working on this session" — so the kind is refused at ingest rather
/// than left to each consumer's diligence (review finding N8).
///
/// **This gate refuses the kind, not a signer.** The relay's own system
/// messages never travel through ingest: `emit_system_message` signs with the
/// relay keypair and writes through `db.insert_event` plus a direct pubsub
/// fan-out (`buzz_relay::handlers::side_effects`). So a relay-signed 40099
/// submitted over the wire is refused too, and nothing the relay actually does
/// goes near this branch. A grep of this tree on 2026-09-08 found no client
/// publisher of 40099 anywhere — `bee`, the provider, Desktop, the web client
/// and mobile all only *read*, filter or fixture it.
pub const fn is_relay_only_kind(kind: u32) -> bool {
    matches!(
        kind,
        KIND_NIP43_MEMBERSHIP_LIST
            | KIND_CHANNEL_SUMMARY
            | KIND_PRESENCE_SNAPSHOT
            | KIND_DM_VISIBILITY
            | KIND_THREAD_SUMMARY
            | KIND_WINDOW_BOUNDS
            | KIND_CI_RESULT
            | KIND_WORKFLOW_HOST_STEP_REQUESTED
            | KIND_WORKFLOW_HOST_STEP_EXITED
            | KIND_WORKFLOW_AUTORUN_CHANGED
            | KIND_SYSTEM_MESSAGE
    )
}

/// Extract the kind from a nostr Event as u32.
/// NIP-01 specifies kind as an unsigned integer; u32 covers the full range.
pub fn event_kind_u32(event: &nostr::Event) -> u32 {
    event.kind.as_u16() as u32
}

/// Extract the kind from a nostr Event as i32 (for Postgres INT columns).
/// Safe: all Buzz kinds fit in i32 (max 65535 < i32::MAX).
pub fn event_kind_i32(event: &nostr::Event) -> i32 {
    event.kind.as_u16() as i32
}

// Compile-time: new kinds are in the expected ranges.
const _: () = assert!(is_replaceable(KIND_AGENT_PROFILE)); // 10100 ∈ 10000–19999
const _: () = assert!(is_parameterized_replaceable(KIND_PERSONA)); // 30175 ∈ 30000–39999
const _: () = assert!(is_parameterized_replaceable(KIND_TEAM)); // 30176 ∈ 30000–39999
const _: () = assert!(is_parameterized_replaceable(KIND_MANAGED_AGENT)); // 30177 ∈ 30000–39999
const _: () = assert!(is_parameterized_replaceable(KIND_TEAM_CATALOG)); // 30178 ∈ 30000–39999
const _: () = assert!(is_parameterized_replaceable(KIND_PRIVATE_MANAGED_AGENT)); // 30179 ∈ 30000–39999
const _: () = assert!(is_parameterized_replaceable(KIND_WORKFLOW_DEF)); // 30620 ∈ 30000–39999
const _: () = assert!(is_parameterized_replaceable(KIND_EVENT_REMINDER)); // 30300 ∈ 30000–39999
const _: () = assert!(is_parameterized_replaceable(KIND_DM_VISIBILITY)); // 30622 ∈ 30000–39999
const _: () = assert!(is_parameterized_replaceable(KIND_PROJECT)); // 30621 ∈ 30000–39999
const _: () = assert!(is_parameterized_replaceable(KIND_SHELL_SESSION)); // 30623 ∈ 30000–39999
const _: () = assert!(is_parameterized_replaceable(KIND_PROJECT_PACK_SOURCE)); // 30624 ∈ 30000–39999
const _: () = assert!(is_parameterized_replaceable(KIND_GIT_REPO_PROTECTION)); // 30625 ∈ 30000–39999
const _: () = assert!(is_ephemeral(KIND_SHELL_WATCH)); // 24310 ∈ 20000–29999, never stored
const _: () = assert!(is_ephemeral(KIND_SHELL_FRAME)); // 24311 ∈ 20000–29999, never stored
const _: () = assert!(!is_ephemeral(KIND_SHELL_SESSION));
const _: () = assert!(is_parameterized_replaceable(KIND_THREAD_SUMMARY)); // 39005 ∈ 30000–39999
const _: () = assert!(is_parameterized_replaceable(KIND_WINDOW_BOUNDS)); // 39006 ∈ 30000–39999

// Compile-time: NIP-34 parameterized replaceable kinds are in the correct range.
const _: () = assert!(
    KIND_GIT_REPO_ANNOUNCEMENT >= PARAM_REPLACEABLE_KIND_MIN
        && KIND_GIT_REPO_ANNOUNCEMENT <= PARAM_REPLACEABLE_KIND_MAX
);
const _: () = assert!(
    KIND_GIT_REPO_STATE >= PARAM_REPLACEABLE_KIND_MIN
        && KIND_GIT_REPO_STATE <= PARAM_REPLACEABLE_KIND_MAX
);

// Compile-time: all Buzz kind constants fit in nostr's u16-backed Kind.
const _: () = assert!(KIND_AUTH <= u16::MAX as u32);
const _: () = assert!(KIND_CANVAS <= u16::MAX as u32);
const _: () = assert!(KIND_HUDDLE_GUIDELINES <= u16::MAX as u32);
const _: () = assert!(EPHEMERAL_KIND_MIN < EPHEMERAL_KIND_MAX);
// Compile-time: KIND_AGENT_TURN_METRIC is a regular stored kind (not ephemeral, not replaceable).
const _: () = assert!(!is_ephemeral(KIND_AGENT_TURN_METRIC));
const _: () = assert!(!is_replaceable(KIND_AGENT_TURN_METRIC));
const _: () = assert!(!is_parameterized_replaceable(KIND_AGENT_TURN_METRIC));
const _: () = assert!(KIND_AGENT_TURN_METRIC <= u16::MAX as u32);
// Pulse entries are append-only coordination history: a revision is a new
// entry naming its predecessor, never a replacement, so no author can quietly
// remove the claim a peer's advisory was built on.
const _: () = assert!(!is_ephemeral(KIND_PULSE_ENTRY));
const _: () = assert!(!is_replaceable(KIND_PULSE_ENTRY));
const _: () = assert!(!is_parameterized_replaceable(KIND_PULSE_ENTRY));
const _: () = assert!(KIND_PULSE_ENTRY <= u16::MAX as u32);
const _: () = assert!(!is_ephemeral(KIND_PROJECT_TODO_OP));
const _: () = assert!(!is_replaceable(KIND_PROJECT_TODO_OP));
const _: () = assert!(!is_parameterized_replaceable(KIND_PROJECT_TODO_OP));
const _: () = assert!(KIND_PROJECT_TODO_OP <= u16::MAX as u32);
const _: () = assert!(is_project_a_scoped_kind(KIND_PROJECT_TODO_OP));
const _: () = assert!(is_project_a_scoped_kind(KIND_PULSE_ENTRY));
// A session lease is deliberately ephemeral: it is bounded evidence of recent
// provider reachability, not durable session history or a replaceable head.
const _: () = assert!(is_ephemeral(KIND_CODING_SESSION_LEASE));
const _: () = assert!(!is_replaceable(KIND_CODING_SESSION_LEASE));
const _: () = assert!(!is_parameterized_replaceable(KIND_CODING_SESSION_LEASE));
const _: () = assert!(KIND_CODING_SESSION_LEASE <= u16::MAX as u32);
// Compile-time: coding-session kinds are regular stored kinds (not ephemeral,
// not replaceable). Commands, receipts, metadata, and transcript items are all
// append-only points in a durable record — a replaced command or a replaced
// transcript item would silently rewrite history a provider already acted on.
// The catalog carries its own monotonic `revision` instead of being replaceable
// so that every advertisement stays auditable.
const _: () = assert!(!is_ephemeral(KIND_CODING_SESSION_COMMAND));
const _: () = assert!(!is_replaceable(KIND_CODING_SESSION_COMMAND));
const _: () = assert!(!is_parameterized_replaceable(KIND_CODING_SESSION_COMMAND));
const _: () = assert!(KIND_CODING_SESSION_COMMAND <= u16::MAX as u32);
const _: () = assert!(!is_ephemeral(KIND_CODING_SESSION_LIFECYCLE_COMMAND));
const _: () = assert!(!is_replaceable(KIND_CODING_SESSION_LIFECYCLE_COMMAND));
const _: () = assert!(!is_parameterized_replaceable(
    KIND_CODING_SESSION_LIFECYCLE_COMMAND
));
const _: () = assert!(KIND_CODING_SESSION_LIFECYCLE_COMMAND <= u16::MAX as u32);
const _: () = assert!(!is_ephemeral(KIND_CODING_SESSION_PROVIDER_CATALOG));
const _: () = assert!(!is_replaceable(KIND_CODING_SESSION_PROVIDER_CATALOG));
const _: () = assert!(!is_parameterized_replaceable(
    KIND_CODING_SESSION_PROVIDER_CATALOG
));
const _: () = assert!(KIND_CODING_SESSION_PROVIDER_CATALOG <= u16::MAX as u32);
const _: () = assert!(!is_ephemeral(KIND_CODING_SESSION_METADATA));
const _: () = assert!(!is_replaceable(KIND_CODING_SESSION_METADATA));
const _: () = assert!(!is_parameterized_replaceable(KIND_CODING_SESSION_METADATA));
const _: () = assert!(KIND_CODING_SESSION_METADATA <= u16::MAX as u32);
const _: () = assert!(!is_ephemeral(KIND_CODING_SESSION_LIFECYCLE_RECEIPT));
const _: () = assert!(!is_replaceable(KIND_CODING_SESSION_LIFECYCLE_RECEIPT));
const _: () = assert!(!is_parameterized_replaceable(
    KIND_CODING_SESSION_LIFECYCLE_RECEIPT
));
const _: () = assert!(KIND_CODING_SESSION_LIFECYCLE_RECEIPT <= u16::MAX as u32);
const _: () = assert!(!is_ephemeral(KIND_CODING_SESSION_TRANSCRIPT));
const _: () = assert!(!is_replaceable(KIND_CODING_SESSION_TRANSCRIPT));
const _: () = assert!(!is_parameterized_replaceable(
    KIND_CODING_SESSION_TRANSCRIPT
));
const _: () = assert!(KIND_CODING_SESSION_TRANSCRIPT <= u16::MAX as u32);
// Genesis is the strictest case of the rule above: it is the founder record an
// entire session's authority hangs from, so a replaceable genesis would let a
// founder be swapped out after the fact.
const _: () = assert!(!is_ephemeral(KIND_CODING_SESSION_GENESIS));
const _: () = assert!(!is_replaceable(KIND_CODING_SESSION_GENESIS));
const _: () = assert!(!is_parameterized_replaceable(KIND_CODING_SESSION_GENESIS));
const _: () = assert!(KIND_CODING_SESSION_GENESIS <= u16::MAX as u32);
// Goal revisions must remain regular events: the d tag groups history but does
// not opt the kind into NIP-33 replacement, so older revisions stay queryable.
const _: () = assert!(!is_ephemeral(KIND_CODING_SESSION_GOAL));
const _: () = assert!(!is_replaceable(KIND_CODING_SESSION_GOAL));
const _: () = assert!(!is_parameterized_replaceable(KIND_CODING_SESSION_GOAL));
const _: () = assert!(KIND_CODING_SESSION_GOAL <= u16::MAX as u32);

const _: () = assert!(!is_ephemeral(KIND_CODING_SESSION_AUTHORITY_TRANSITION));
const _: () = assert!(!is_replaceable(KIND_CODING_SESSION_AUTHORITY_TRANSITION));
const _: () = assert!(!is_parameterized_replaceable(
    KIND_CODING_SESSION_AUTHORITY_TRANSITION
));
const _: () = assert!(KIND_CODING_SESSION_AUTHORITY_TRANSITION <= u16::MAX as u32);
// Name revisions use the same append-only history discipline as goal
// revisions; the d tag groups revisions without invoking NIP-33 replacement.
const _: () = assert!(!is_ephemeral(KIND_CODING_SESSION_NAME));
const _: () = assert!(!is_replaceable(KIND_CODING_SESSION_NAME));
const _: () = assert!(!is_parameterized_replaceable(KIND_CODING_SESSION_NAME));
const _: () = assert!(KIND_CODING_SESSION_NAME <= u16::MAX as u32);
// Closure revisions are append-only history, never a NIP-16/NIP-33 head.
const _: () = assert!(!is_ephemeral(KIND_CODING_SESSION_CLOSURE));
const _: () = assert!(!is_replaceable(KIND_CODING_SESSION_CLOSURE));
const _: () = assert!(!is_parameterized_replaceable(KIND_CODING_SESSION_CLOSURE));
const _: () = assert!(KIND_CODING_SESSION_CLOSURE <= u16::MAX as u32);
// Team transactions are immutable semantic history. The d tag groups a
// session; it never opts this regular kind into NIP-33 replacement.
const _: () = assert!(!is_ephemeral(KIND_CODING_SESSION_TEAM_TRANSACTION));
const _: () = assert!(!is_replaceable(KIND_CODING_SESSION_TEAM_TRANSACTION));
const _: () = assert!(!is_parameterized_replaceable(
    KIND_CODING_SESSION_TEAM_TRANSACTION
));
const _: () = assert!(KIND_CODING_SESSION_TEAM_TRANSACTION <= u16::MAX as u32);
// The policy record is append-only history too. The d tag addresses an
// umbrella so a consumer can fold the newest accepted revision; it never opts
// this regular kind into NIP-33 replacement, because a replaceable policy
// would let one author's write erase the revision another decision was made
// under.
const _: () = assert!(!is_ephemeral(KIND_CODING_SESSION_POLICY));
const _: () = assert!(!is_replaceable(KIND_CODING_SESSION_POLICY));
const _: () = assert!(!is_parameterized_replaceable(KIND_CODING_SESSION_POLICY));
const _: () = assert!(KIND_CODING_SESSION_POLICY <= u16::MAX as u32);
// The next free number, and nothing between it and the team transaction.
const _: () = assert!(KIND_CODING_SESSION_POLICY == KIND_CODING_SESSION_TEAM_TRANSACTION + 1);
// An observation is append-only history for the same reason a policy is, and
// for one more: a replaceable observation would let a seat's newest gate row
// erase the failing one a person was reading. Newest-wins is a *fold* rule
// here, applied per (author, gate) and per (author, findingId), and the older
// event stays on the wire where a reader can still find it.
const _: () = assert!(!is_ephemeral(KIND_CODING_SESSION_OBSERVATION));
const _: () = assert!(!is_replaceable(KIND_CODING_SESSION_OBSERVATION));
const _: () = assert!(!is_parameterized_replaceable(
    KIND_CODING_SESSION_OBSERVATION
));
const _: () = assert!(KIND_CODING_SESSION_OBSERVATION <= u16::MAX as u32);
// The next free number, and nothing between it and the policy.
const _: () = assert!(KIND_CODING_SESSION_OBSERVATION == KIND_CODING_SESSION_POLICY + 1);
// A handover record is append-only history for the strongest reason on this
// list: a checkpoint is what somebody else reconstructs work from, and a
// replaceable one would let a later write change what a person already acted
// on. Its `d` tag groups an umbrella so a reader can fold every record of one
// session; it never opts this regular kind into NIP-33 replacement.
const _: () = assert!(!is_ephemeral(KIND_CODING_SESSION_HANDOVER));
const _: () = assert!(!is_replaceable(KIND_CODING_SESSION_HANDOVER));
const _: () = assert!(!is_parameterized_replaceable(KIND_CODING_SESSION_HANDOVER));
const _: () = assert!(KIND_CODING_SESSION_HANDOVER <= u16::MAX as u32);
// The next free number, and nothing between it and the observation.
const _: () = assert!(KIND_CODING_SESSION_HANDOVER == KIND_CODING_SESSION_OBSERVATION + 1);
// Moderation kinds fit u16 and are neither replaceable nor ephemeral:
// 1984 is a regular event (persisted to the queue, never fanned out);
// 9040–9044 are direct commands (executed, never stored).
const _: () = assert!(KIND_REPORT <= u16::MAX as u32);
const _: () = assert!(KIND_MODERATION_RESOLVE_REPORT <= u16::MAX as u32);
const _: () = assert!(!is_ephemeral(KIND_REPORT));
const _: () = assert!(is_moderation_command_kind(KIND_MODERATION_BAN));
const _: () = assert!(is_moderation_command_kind(KIND_MODERATION_RESOLVE_REPORT));
const _: () = assert!(!is_moderation_command_kind(KIND_REPORT));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_duplicate_kind_values() {
        let mut seen = std::collections::HashSet::new();
        for &k in ALL_KINDS {
            assert!(seen.insert(k), "duplicate kind value: {k}");
        }
    }

    #[test]
    fn nip43_membership_snapshot_is_relay_only() {
        assert!(is_relay_only_kind(KIND_NIP43_MEMBERSHIP_LIST));
        assert!(!is_relay_only_kind(KIND_NIP43_LEAVE_REQUEST));
    }

    /// Review finding N8: kind 40099 is the relay speaking as itself, and the
    /// deletion receipt made it session-stopping. A client may not submit one.
    ///
    /// The kinds beside it are the ones a client legitimately writes into the
    /// same rooms, asserted here so a future edit cannot widen the gate by
    /// accident.
    #[test]
    fn a_system_message_is_relay_only_and_its_neighbours_are_not() {
        assert!(is_relay_only_kind(KIND_SYSTEM_MESSAGE));
        for client_kind in [
            KIND_STREAM_MESSAGE,
            KIND_REACTION,
            KIND_CODING_SESSION_AUTHORITY_TRANSITION,
            KIND_CODING_SESSION_HANDOVER,
            KIND_DELETION,
        ] {
            assert!(
                !is_relay_only_kind(client_kind),
                "kind {client_kind} is written by clients and must stay submittable"
            );
        }
    }

    #[test]
    fn parameterized_replaceable_range() {
        assert!(!is_parameterized_replaceable(29999));
        assert!(is_parameterized_replaceable(30000));
        assert!(is_parameterized_replaceable(30023)); // NIP-23 long-form
        assert!(is_parameterized_replaceable(39000)); // NIP-29 group metadata
        assert!(is_parameterized_replaceable(39999));
        assert!(!is_parameterized_replaceable(40000));
    }

    #[test]
    fn replaceable_and_parameterized_are_disjoint() {
        for kind in 0..=65535u32 {
            assert!(
                !(is_replaceable(kind) && is_parameterized_replaceable(kind)),
                "kind {kind} is both replaceable and parameterized replaceable"
            );
        }
    }

    // ── event_is_shared / is_unshared_gated_event ────────────────────────

    fn make_event_of_kind(kind: u32, tags: &[&[&str]]) -> nostr::Event {
        use nostr::{EventBuilder, Keys, Kind, Tag};
        let keys = Keys::generate();
        let tag_vec: Vec<Tag> = tags
            .iter()
            .map(|parts| Tag::parse(parts.iter().copied()).unwrap())
            .collect();
        EventBuilder::new(Kind::Custom(kind as u16), "")
            .tags(tag_vec)
            .sign_with_keys(&keys)
            .unwrap()
    }

    fn make_persona_event(tags: &[&[&str]]) -> nostr::Event {
        make_event_of_kind(KIND_PERSONA, tags)
    }

    // ── is_private_project_event / project_container_hidden_from ─────────

    fn make_project_event(tags: &[&[&str]]) -> nostr::Event {
        make_event_of_kind(KIND_PROJECT, tags)
    }

    const FOREIGN_HEX: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    #[test]
    fn private_project_event_detected() {
        let ev = make_project_event(&[&["d", "proj"], &["buzz-access", "private"]]);
        assert!(is_private_project_event(&ev));
    }

    #[test]
    fn public_or_tagless_project_not_private() {
        let public = make_project_event(&[&["d", "proj"], &["buzz-access", "public"]]);
        assert!(!is_private_project_event(&public));
        let tagless = make_project_event(&[&["d", "proj"]]);
        assert!(!is_private_project_event(&tagless));
    }

    #[test]
    fn private_tag_on_other_kind_not_private_project() {
        let ev = make_persona_event(&[&["d", "proj"], &["buzz-access", "private"]]);
        assert!(!is_private_project_event(&ev));
    }

    /// The reader's admitted set, as the relay resolves it from
    /// `project_acl` / `project_acl_members`.
    fn admitting(event: &nostr::Event) -> std::collections::HashSet<String> {
        let mut set = std::collections::HashSet::new();
        set.insert(project_head_coordinate(event).expect("project head coordinate"));
        set
    }

    fn nothing_admitted() -> std::collections::HashSet<String> {
        std::collections::HashSet::new()
    }

    #[test]
    fn project_head_coordinate_is_the_canonical_form() {
        let ev = make_project_event(&[&["d", "platform"]]);
        assert_eq!(
            project_head_coordinate(&ev),
            Some(format!("{KIND_PROJECT}:{}:platform", ev.pubkey.to_hex()))
        );
        // Not a project head, and a head with no `d` tag: no coordinate.
        assert_eq!(
            project_head_coordinate(&make_persona_event(&[&["d", "platform"]])),
            None
        );
        assert_eq!(project_head_coordinate(&make_project_event(&[])), None);
    }

    #[test]
    fn malformed_private_tag_fails_closed() {
        // Extra tag element must still count as private — hide, never leak.
        let ev = make_project_event(&[&["d", "proj"], &["buzz-access", "private", "x"]]);
        assert!(is_private_project_event(&ev));
        assert!(project_container_hidden_from(
            &ev,
            FOREIGN_HEX,
            &nothing_admitted()
        ));
    }

    #[test]
    fn private_project_hidden_from_foreign_reader() {
        let ev = make_project_event(&[&["d", "proj"], &["buzz-access", "private"]]);
        assert!(project_container_hidden_from(
            &ev,
            FOREIGN_HEX,
            &nothing_admitted()
        ));
    }

    #[test]
    fn private_project_visible_to_author() {
        let ev = make_project_event(&[&["d", "proj"], &["buzz-access", "private"]]);
        // The author never depends on the ACL projection having landed.
        assert!(!project_container_hidden_from(
            &ev,
            &ev.pubkey.to_hex(),
            &nothing_admitted()
        ));
    }

    #[test]
    fn private_project_visible_to_admitted_member() {
        // The head carries no `p` tag at all — this is the member a kind:9010
        // op added, the case that used to be invisible to everyone but the
        // creator.
        let ev = make_project_event(&[&["d", "proj"], &["buzz-access", "private"]]);
        assert!(!project_container_hidden_from(
            &ev,
            FOREIGN_HEX,
            &admitting(&ev)
        ));
    }

    #[test]
    fn private_project_hidden_from_stale_head_p_tag_member() {
        // The converse: a member kind:9011 removed is gone from the roster,
        // but their `p` tag is still on the creator-signed head and nothing
        // can rewrite it. The roster decides, not the head.
        let ev = make_project_event(&[
            &["d", "proj"],
            &["buzz-access", "private"],
            &["p", FOREIGN_HEX],
        ]);
        assert!(project_container_hidden_from(
            &ev,
            FOREIGN_HEX,
            &nothing_admitted()
        ));
    }

    #[test]
    fn private_project_hidden_when_only_another_project_admits() {
        let ev = make_project_event(&[&["d", "proj"], &["buzz-access", "private"]]);
        let elsewhere = make_project_event(&[&["d", "other"], &["buzz-access", "private"]]);
        assert!(project_container_hidden_from(
            &ev,
            FOREIGN_HEX,
            &admitting(&elsewhere)
        ));
    }

    #[test]
    fn private_project_without_d_tag_admits_nobody_but_its_author() {
        // No `d` tag means no coordinate to check against the roster. Ingest
        // rejects the shape (`d-cardinality`), but a smuggled head must hide
        // rather than leak.
        let ev = make_project_event(&[&["buzz-access", "private"]]);
        let mut everything = std::collections::HashSet::new();
        everything.insert(format!("{KIND_PROJECT}:{}:", ev.pubkey.to_hex()));
        assert!(project_container_hidden_from(&ev, FOREIGN_HEX, &everything));
        assert!(!project_container_hidden_from(
            &ev,
            &ev.pubkey.to_hex(),
            &everything
        ));
    }

    // ── NIP-ST shared terminals: kinds + hidden-from ─────────────────────

    #[test]
    fn shell_observe_kinds_are_recognized() {
        assert!(is_shell_observe_kind(KIND_SHELL_SESSION));
        assert!(is_shell_observe_kind(KIND_SHELL_WATCH));
        assert!(is_shell_observe_kind(KIND_SHELL_FRAME));
        assert!(!is_shell_observe_kind(KIND_PROJECT));
    }

    #[test]
    fn shell_observe_project_ref_reads_single_a_tag() {
        let coord = format!("{KIND_PROJECT}:{FOREIGN_HEX}:platform");
        let ev = make_event_of_kind(KIND_SHELL_FRAME, &[&["d", "s1"], &["a", &coord]]);
        assert_eq!(shell_observe_project_ref(&ev), Some(coord));
        let no_a = make_event_of_kind(KIND_SHELL_FRAME, &[&["d", "s1"]]);
        assert_eq!(shell_observe_project_ref(&no_a), None);
    }

    #[test]
    fn shell_session_hidden_from_reader_outside_private_project() {
        let coord = format!("{KIND_PROJECT}:{FOREIGN_HEX}:platform");
        let ev = make_event_of_kind(
            KIND_SHELL_SESSION,
            &[&["d", "s1"], &["a", &coord], &["status", "open"]],
        );
        let mut hidden = std::collections::HashSet::new();

        // Empty hidden set (public/admitted): visible.
        assert!(!shell_session_hidden_from(&ev, FOREIGN_HEX, &hidden));

        hidden.insert(coord);
        // Foreign reader whose hidden set contains the coordinate: withheld.
        assert!(shell_session_hidden_from(&ev, FOREIGN_HEX, &hidden));
        // The author always sees their own announce.
        assert!(!shell_session_hidden_from(
            &ev,
            &ev.pubkey.to_hex(),
            &hidden
        ));
    }

    #[test]
    fn shell_session_without_coordinate_fails_closed() {
        let ev = make_event_of_kind(KIND_SHELL_SESSION, &[&["d", "s1"], &["status", "open"]]);
        let mut hidden = std::collections::HashSet::new();
        hidden.insert("anything".to_string());
        assert!(shell_session_hidden_from(&ev, FOREIGN_HEX, &hidden));
        assert!(!shell_session_hidden_from(
            &ev,
            &ev.pubkey.to_hex(),
            &hidden
        ));
        // Other kinds never trip this predicate.
        let other = make_event_of_kind(KIND_PROJECT, &[&["d", "s1"]]);
        assert!(!shell_session_hidden_from(&other, FOREIGN_HEX, &hidden));
    }

    // ── Project-a-scoped kinds: the shared gate ──────────────────────────

    #[test]
    fn project_a_scoped_kinds_agree() {
        for kind in PROJECT_A_SCOPED_KINDS {
            assert!(
                is_project_a_scoped_kind(*kind),
                "{kind} listed but not matched"
            );
            assert!(ALL_KINDS.contains(kind), "{kind} not registered");
            assert!(
                !is_parameterized_replaceable(*kind),
                "{kind} must be regular"
            );
            assert!(!is_replaceable(*kind), "{kind} must be regular");
        }
        let matched: Vec<u32> = ALL_KINDS
            .iter()
            .copied()
            .filter(|k| is_project_a_scoped_kind(*k))
            .collect();
        assert_eq!(matched, PROJECT_A_SCOPED_KINDS.to_vec());
    }

    #[test]
    fn personal_todo_op_is_hidden_from_everyone_but_its_author() {
        let coord = format!("{KIND_PROJECT}:{FOREIGN_HEX}:platform");
        let personal = make_event_of_kind(
            KIND_PROJECT_TODO_OP,
            &[&["a", &coord], &["td-vis", "personal"]],
        );
        // Public project (empty hidden set): still hidden from a non-author.
        let empty = std::collections::HashSet::new();
        assert!(is_personal_todo_op(&personal));
        assert!(project_a_scoped_event_hidden_from(
            &personal,
            FOREIGN_HEX,
            &empty
        ));
        assert!(!project_a_scoped_event_hidden_from(
            &personal,
            &personal.pubkey.to_hex(),
            &empty
        ));
        // A project-visible op in a public project is not hidden.
        let shared = make_event_of_kind(
            KIND_PROJECT_TODO_OP,
            &[&["a", &coord], &["td-vis", "project"]],
        );
        assert!(!is_personal_todo_op(&shared));
        assert!(!project_a_scoped_event_hidden_from(
            &shared,
            FOREIGN_HEX,
            &empty
        ));
        // Only a 44248 reads the tag.
        let pulse =
            make_event_of_kind(KIND_PULSE_ENTRY, &[&["a", &coord], &["td-vis", "personal"]]);
        assert!(!is_personal_todo_op(&pulse));
    }

    #[test]
    fn project_todo_op_rides_the_same_gate_as_pulse() {
        let coord = format!("{KIND_PROJECT}:{FOREIGN_HEX}:platform");
        let ev = make_event_of_kind(
            KIND_PROJECT_TODO_OP,
            &[&["a", &coord], &["td-op", "item.add"]],
        );
        let mut hidden = std::collections::HashSet::new();
        assert!(!project_a_scoped_event_hidden_from(
            &ev,
            FOREIGN_HEX,
            &hidden
        ));
        hidden.insert(coord.clone());
        assert!(project_a_scoped_event_hidden_from(
            &ev,
            FOREIGN_HEX,
            &hidden
        ));
        assert!(!project_a_scoped_event_hidden_from(
            &ev,
            &ev.pubkey.to_hex(),
            &hidden
        ));
        // No coordinate: fail closed for a non-author.
        let no_a = make_event_of_kind(KIND_PROJECT_TODO_OP, &[&["td-op", "item.add"]]);
        assert!(project_a_scoped_event_hidden_from(
            &no_a,
            FOREIGN_HEX,
            &hidden
        ));
        // Case-variant coordinate still resolves to the hidden set.
        let upper = format!("{KIND_PROJECT}:{}:platform", FOREIGN_HEX.to_uppercase());
        let variant = make_event_of_kind(KIND_PROJECT_TODO_OP, &[&["a", &upper]]);
        assert!(project_a_scoped_event_hidden_from(
            &variant,
            FOREIGN_HEX,
            &hidden
        ));
        assert_eq!(
            project_a_scoped_coordinate(&variant).as_deref(),
            Some(coord.as_str())
        );
        // A kind outside the set never trips it.
        let other = make_event_of_kind(KIND_SHELL_SESSION, &[&["a", &coord]]);
        assert!(!project_a_scoped_event_hidden_from(
            &other,
            FOREIGN_HEX,
            &hidden
        ));
    }

    // ── Project Pulse entries: hidden-from ───────────────────────────────

    #[test]
    fn pulse_entry_hidden_from_reader_outside_private_project() {
        let coord = format!("{KIND_PROJECT}:{FOREIGN_HEX}:platform");
        let ev = make_event_of_kind(
            KIND_PULSE_ENTRY,
            &[
                &["a", &coord],
                &["pu-v", crate::pulse::PULSE_ENTRY_TAG_VERSION],
                &["pu-type", "plan"],
            ],
        );
        let mut hidden = std::collections::HashSet::new();

        // Empty hidden set (public/admitted): visible.
        assert!(!pulse_entry_hidden_from(&ev, FOREIGN_HEX, &hidden));

        hidden.insert(coord);
        assert!(pulse_entry_hidden_from(&ev, FOREIGN_HEX, &hidden));
        // The author always reads their own entry.
        assert!(!pulse_entry_hidden_from(&ev, &ev.pubkey.to_hex(), &hidden));
    }

    #[test]
    fn pulse_entry_without_single_coordinate_fails_closed() {
        let mut hidden = std::collections::HashSet::new();
        hidden.insert("anything".to_string());

        let no_a = make_event_of_kind(KIND_PULSE_ENTRY, &[&["pu-type", "plan"]]);
        assert!(pulse_entry_hidden_from(&no_a, FOREIGN_HEX, &hidden));
        assert!(!pulse_entry_hidden_from(
            &no_a,
            &no_a.pubkey.to_hex(),
            &hidden
        ));

        // Two `a` tags are not a wider grant — they are an unresolvable scope.
        let coord = format!("{KIND_PROJECT}:{FOREIGN_HEX}:platform");
        let two = make_event_of_kind(
            KIND_PULSE_ENTRY,
            &[
                &["a", &coord],
                &["a", &format!("{KIND_PROJECT}:{FOREIGN_HEX}:other")],
            ],
        );
        assert!(pulse_entry_hidden_from(&two, FOREIGN_HEX, &hidden));

        // Other kinds never trip this predicate.
        let other = make_event_of_kind(KIND_PROJECT, &[&["d", "proj"]]);
        assert!(!pulse_entry_hidden_from(&other, FOREIGN_HEX, &hidden));
    }

    #[test]
    fn pulse_entry_case_variant_coordinate_still_hides() {
        let upper = format!(
            "{KIND_PROJECT}:{}:platform",
            FOREIGN_HEX.to_ascii_uppercase()
        );
        let ev = make_event_of_kind(KIND_PULSE_ENTRY, &[&["a", &upper]]);
        let mut hidden = std::collections::HashSet::new();
        hidden.insert(format!("{KIND_PROJECT}:{FOREIGN_HEX}:platform"));
        assert!(pulse_entry_hidden_from(&ev, FOREIGN_HEX, &hidden));
    }

    #[test]
    fn public_project_never_hidden() {
        let ev = make_project_event(&[&["d", "proj"], &["buzz-access", "public"]]);
        assert!(!project_container_hidden_from(
            &ev,
            FOREIGN_HEX,
            &nothing_admitted()
        ));
        let tagless = make_project_event(&[&["d", "proj"]]);
        assert!(!project_container_hidden_from(
            &tagless,
            FOREIGN_HEX,
            &nothing_admitted()
        ));
    }

    // ── repo_project_ref / git_event_repo_names / repo_event_hidden_from ──

    #[test]
    fn repo_project_ref_normalizes_and_validates() {
        let coord = format!("30621:{FOREIGN_HEX}:my-proj");
        let ev = make_event_of_kind(
            KIND_GIT_REPO_ANNOUNCEMENT,
            &[&["d", "repo"], &["project", &coord]],
        );
        assert_eq!(repo_project_ref(&ev), Some(coord.clone()));

        // Uppercase hex normalizes to lowercase.
        let upper = FOREIGN_HEX.to_ascii_uppercase();
        let mixed = format!("30621:{upper}:my-proj");
        let ev = make_event_of_kind(
            KIND_GIT_REPO_ANNOUNCEMENT,
            &[&["d", "repo"], &["project", &mixed]],
        );
        assert_eq!(repo_project_ref(&ev), Some(coord));

        // Malformed values fail open to "no project" (legacy stored events
        // predate gating; go-forward ingest rejects them). 30178 is the
        // team-catalog kind on this relay, never a project coordinate.
        for bad in [
            "30621:short:proj",
            "9999:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa:p",
            "30178:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa:p",
            "not-a-coordinate",
            "30621::proj",
        ] {
            let ev = make_event_of_kind(
                KIND_GIT_REPO_ANNOUNCEMENT,
                &[&["d", "repo"], &["project", bad]],
            );
            assert_eq!(repo_project_ref(&ev), None, "value {bad:?} must not parse");
        }

        // Wrong kind / no tag → None.
        let no_tag = make_event_of_kind(KIND_GIT_REPO_ANNOUNCEMENT, &[&["d", "repo"]]);
        assert_eq!(repo_project_ref(&no_tag), None);
        let coord = format!("30621:{FOREIGN_HEX}:my-proj");
        let wrong_kind = make_project_event(&[&["d", "x"], &["project", &coord]]);
        assert_eq!(repo_project_ref(&wrong_kind), None);
    }

    #[test]
    fn git_event_repo_names_covers_all_surfaces() {
        // 30617/30618: own d tag.
        let ann = make_event_of_kind(KIND_GIT_REPO_ANNOUNCEMENT, &[&["d", "repo-a"]]);
        assert_eq!(git_event_repo_names(&ann), vec!["repo-a".to_string()]);
        let state = make_event_of_kind(KIND_GIT_REPO_STATE, &[&["d", "repo-a"]]);
        assert_eq!(git_event_repo_names(&state), vec!["repo-a".to_string()]);

        // Child kinds: a-tag coordinates, hex case-insensitive.
        let coord_lower = format!("30617:{FOREIGN_HEX}:repo-b");
        let coord_upper = format!("30617:{}:repo-c", FOREIGN_HEX.to_ascii_uppercase());
        let issue = make_event_of_kind(
            KIND_GIT_ISSUE,
            &[&["a", &coord_lower], &["a", &coord_upper]],
        );
        assert_eq!(
            git_event_repo_names(&issue),
            vec!["repo-b".to_string(), "repo-c".to_string()]
        );

        // Malformed a tags and non-repo coordinates are ignored.
        let noise = make_event_of_kind(
            KIND_GIT_PATCH,
            &[&["a", "30621:aaaa:proj"], &["a", "junk"], &["e", "beef"]],
        );
        assert!(git_event_repo_names(&noise).is_empty());

        // Non-git kinds resolve to no repos at all.
        let note = make_event_of_kind(1, &[&["a", &coord_lower]]);
        assert!(git_event_repo_names(&note).is_empty());
    }

    #[test]
    fn repo_event_hidden_from_gates_by_name_set() {
        let hidden: std::collections::HashSet<String> =
            ["secret-repo".to_string()].into_iter().collect();
        let empty: std::collections::HashSet<String> = Default::default();

        let ann = make_event_of_kind(KIND_GIT_REPO_ANNOUNCEMENT, &[&["d", "secret-repo"]]);
        // Hidden from a stranger, never from its author, never with an empty set.
        assert!(repo_event_hidden_from(&ann, FOREIGN_HEX, &hidden, &empty));
        assert!(!repo_event_hidden_from(
            &ann,
            &ann.pubkey.to_hex(),
            &hidden,
            &empty
        ));
        assert!(!repo_event_hidden_from(&ann, FOREIGN_HEX, &empty, &empty));

        // Child event referencing the hidden repo is hidden; one referencing
        // another repo is not.
        let coord = format!("30617:{FOREIGN_HEX}:secret-repo");
        let issue = make_event_of_kind(KIND_GIT_ISSUE, &[&["a", &coord]]);
        assert!(repo_event_hidden_from(&issue, FOREIGN_HEX, &hidden, &empty));
        let other = make_event_of_kind(
            KIND_GIT_ISSUE,
            &[&["a", &format!("30617:{FOREIGN_HEX}:open-repo")]],
        );
        assert!(!repo_event_hidden_from(
            &other,
            FOREIGN_HEX,
            &hidden,
            &empty
        ));

        // Case-variant coordinate cannot dodge the check.
        let upper = format!("30617:{}:secret-repo", FOREIGN_HEX.to_ascii_uppercase());
        let dodgy = make_event_of_kind(KIND_GIT_PATCH, &[&["a", &upper]]);
        assert!(repo_event_hidden_from(&dodgy, FOREIGN_HEX, &hidden, &empty));

        // Non-git kinds are never gated here.
        let note = make_event_of_kind(1, &[&["d", "secret-repo"], &["a", &coord]]);
        assert!(!repo_event_hidden_from(&note, FOREIGN_HEX, &hidden, &empty));
    }

    #[test]
    fn repo_announcement_hidden_by_its_own_project_tag() {
        // Defense-in-depth: a 30617 whose projection never landed (failed
        // side effect) is still hidden through the coordinate on the event.
        let proj_coord = format!("30621:{FOREIGN_HEX}:secret-proj");
        let hidden_projects: std::collections::HashSet<String> =
            [proj_coord.clone()].into_iter().collect();
        let no_names: std::collections::HashSet<String> = Default::default();

        let ann = make_event_of_kind(
            KIND_GIT_REPO_ANNOUNCEMENT,
            &[&["d", "unprojected"], &["project", &proj_coord]],
        );
        assert!(repo_event_hidden_from(
            &ann,
            FOREIGN_HEX,
            &no_names,
            &hidden_projects
        ));
        // Author still sees it; a repo tagged into some other project does not hide.
        assert!(!repo_event_hidden_from(
            &ann,
            &ann.pubkey.to_hex(),
            &no_names,
            &hidden_projects
        ));
        let other = make_event_of_kind(
            KIND_GIT_REPO_ANNOUNCEMENT,
            &[
                &["d", "open"],
                &["project", &format!("30621:{FOREIGN_HEX}:open-proj")],
            ],
        );
        assert!(!repo_event_hidden_from(
            &other,
            FOREIGN_HEX,
            &no_names,
            &hidden_projects
        ));
    }

    #[test]
    fn event_is_shared_true_tag() {
        let ev = make_persona_event(&[&["d", "my-agent"], &["shared", "true"]]);
        assert!(event_is_shared(&ev));
    }

    #[test]
    fn event_is_shared_no_tag() {
        let ev = make_persona_event(&[&["d", "my-agent"]]);
        assert!(!event_is_shared(&ev));
    }

    #[test]
    fn event_is_shared_wrong_value() {
        let ev = make_persona_event(&[&["d", "my-agent"], &["shared", "false"]]);
        assert!(!event_is_shared(&ev));
    }

    #[test]
    fn event_is_shared_duplicate_shared_tags() {
        // Two ["shared","true"] tags → ambiguous; not considered shared.
        let ev =
            make_persona_event(&[&["d", "my-agent"], &["shared", "true"], &["shared", "true"]]);
        assert!(!event_is_shared(&ev));
    }

    #[test]
    fn event_is_shared_three_element_tag_not_shared() {
        // ["shared","true","extra"] — three elements — must NOT be treated as shared.
        // The helper fails closed on any non-exact shape independently of ingest guarantees.
        let ev = make_persona_event(&[&["d", "my-agent"], &["shared", "true", "extra"]]);
        assert!(!event_is_shared(&ev));
    }

    #[test]
    fn event_is_shared_one_element_tag_not_shared() {
        // ["shared"] — only one element — not shared (fails the == 2 check).
        let ev = make_persona_event(&[&["d", "my-agent"], &["shared"]]);
        assert!(!event_is_shared(&ev));
    }

    #[test]
    fn is_unshared_gated_event_author_always_allowed() {
        // Even without a shared tag the event author should not be blocked.
        use nostr::{EventBuilder, Keys, Kind, Tag};
        let keys = Keys::generate();
        let ev = EventBuilder::new(Kind::Custom(KIND_PERSONA as u16), "")
            .tags(vec![Tag::parse(["d", "my-agent"]).unwrap()])
            .sign_with_keys(&keys)
            .unwrap();
        let author_bytes = keys.public_key().to_bytes();
        assert!(!is_unshared_gated_event(&ev, &author_bytes));
    }

    #[test]
    fn is_unshared_gated_event_foreign_no_tag() {
        let ev = make_persona_event(&[&["d", "my-agent"]]);
        let foreign = [0u8; 32];
        assert!(is_unshared_gated_event(&ev, &foreign));
    }

    #[test]
    fn is_unshared_gated_event_foreign_shared_tag() {
        let ev = make_persona_event(&[&["d", "my-agent"], &["shared", "true"]]);
        let foreign = [0u8; 32];
        assert!(!is_unshared_gated_event(&ev, &foreign));
    }

    #[test]
    fn is_unshared_gated_event_ungated_kind_passthrough() {
        use nostr::{EventBuilder, Keys, Kind};
        let keys = Keys::generate();
        let ev = EventBuilder::new(Kind::Custom(KIND_TEAM as u16), "")
            .sign_with_keys(&keys)
            .unwrap();
        let foreign = [0u8; 32];
        // Kinds outside SHARED_GATED_KINDS are never blocked by this gate.
        assert!(!is_unshared_gated_event(&ev, &foreign));
    }

    #[test]
    fn is_unshared_gated_event_team_catalog_foreign_no_tag() {
        // The gate must cover 30178 identically to 30175 — an unshared team
        // catalog projection is author-only.
        let ev = make_event_of_kind(KIND_TEAM_CATALOG, &[&["d", "team-1"]]);
        let foreign = [0u8; 32];
        assert!(is_unshared_gated_event(&ev, &foreign));
    }

    #[test]
    fn is_unshared_gated_event_team_catalog_foreign_shared_tag() {
        let ev = make_event_of_kind(KIND_TEAM_CATALOG, &[&["d", "team-1"], &["shared", "true"]]);
        let foreign = [0u8; 32];
        assert!(!is_unshared_gated_event(&ev, &foreign));
    }

    #[test]
    fn is_unshared_gated_event_team_catalog_author_always_allowed() {
        use nostr::{EventBuilder, Keys, Kind, Tag};
        let keys = Keys::generate();
        let ev = EventBuilder::new(Kind::Custom(KIND_TEAM_CATALOG as u16), "")
            .tags(vec![Tag::parse(["d", "team-1"]).unwrap()])
            .sign_with_keys(&keys)
            .unwrap();
        let author_bytes = keys.public_key().to_bytes();
        assert!(!is_unshared_gated_event(&ev, &author_bytes));
    }

    #[test]
    fn is_unshared_gated_event_team_catalog_malformed_shared_tag_fails_closed() {
        // A three-element `shared` tag can never be stored (ingest rejects it),
        // but the read gate must independently treat it as NOT shared.
        let ev = make_event_of_kind(
            KIND_TEAM_CATALOG,
            &[&["d", "team-1"], &["shared", "true", "extra"]],
        );
        let foreign = [0u8; 32];
        assert!(is_unshared_gated_event(&ev, &foreign));
    }

    #[test]
    fn shared_gated_kinds_membership() {
        assert!(is_shared_gated_kind(KIND_PERSONA));
        assert!(is_shared_gated_kind(KIND_TEAM_CATALOG));
        // 30176 has owner-private semantics, not catalog opt-in semantics: its
        // writers never emit `shared`, so gating it here would hide every team
        // from its own delegated readers.
        assert!(!is_shared_gated_kind(KIND_TEAM));
        assert!(!is_shared_gated_kind(KIND_MANAGED_AGENT));
    }
}
