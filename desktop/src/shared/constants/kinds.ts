export const KIND_DELETION = 5;
export const KIND_REACTION = 7;
export const KIND_TEXT_NOTE = 1;
export const KIND_STREAM_MESSAGE = 9;
// Buzz-native deletion. The relay soft-deletes the target and emits a
// kind:40099 system message. Treated as a deletion marker alongside kind:5.
export const KIND_NIP29_DELETE_EVENT = 9005;
// NIP-56 report + community-moderation command kinds. Reports (1984) persist to
// the mod queue only; commands (9040–9044) are relay-validated and never stored.
// Tag shapes are pinned by buzz-sdk builders + relay moderation_commands.rs.
export const KIND_REPORT = 1984;
export const KIND_PRODUCT_FEEDBACK = 42000;
export const KIND_IA_ARCHIVE_REQUEST = 9035;
export const KIND_MODERATION_BAN = 9040;
export const KIND_MODERATION_UNBAN = 9041;
export const KIND_MODERATION_TIMEOUT = 9042;
export const KIND_MODERATION_UNTIMEOUT = 9043;
export const KIND_MODERATION_RESOLVE_REPORT = 9044;
export const KIND_STREAM_MESSAGE_V2 = 40002;
export const KIND_STREAM_MESSAGE_EDIT = 40003;
export const KIND_CHANNEL_THREAD_SUMMARY = 39005;
export const KIND_CHANNEL_WINDOW_BOUNDS = 39006;
export const KIND_STREAM_MESSAGE_DIFF = 40008;
export const KIND_REMINDER = 40007;
export const KIND_SYSTEM_MESSAGE = 40099;
export const KIND_JOB_REQUEST = 43001;
export const KIND_JOB_ACCEPTED = 43002;
export const KIND_JOB_PROGRESS = 43003;
export const KIND_JOB_RESULT = 43004;
export const KIND_JOB_CANCEL = 43005;
export const KIND_JOB_ERROR = 43006;
export const KIND_FORUM_POST = 45001;
export const KIND_FORUM_COMMENT = 45003;
export const KIND_APPROVAL_REQUEST = 46010;
export const KIND_MEMBER_ADDED_NOTIFICATION = 44100;
export const KIND_MEMBER_REMOVED_NOTIFICATION = 44101;
export const KIND_TYPING_INDICATOR = 20002;
export const KIND_PRESENCE_UPDATE = 20001;
export const KIND_HUDDLE_REACTION = 24810;
export const KIND_HUDDLE_STARTED = 48100;
export const KIND_HUDDLE_PARTICIPANT_JOINED = 48101;
export const KIND_HUDDLE_PARTICIPANT_LEFT = 48102;
export const KIND_HUDDLE_ENDED = 48103;
// NIP-78 application-specific data. All use kind 30078; the relay
// differentiates them by d-tag ("read-state:<slotId>", "channel-sections", "channel-mutes", "channel-stars", "channel-sort", "project-order").
export const KIND_READ_STATE = 30078;
export const KIND_CHANNEL_SECTIONS = 30078;
export const KIND_CHANNEL_MUTES = 30078;
export const KIND_CHANNEL_STARS = 30078;
export const KIND_CHANNEL_SORT = 30078;
export const KIND_PROJECT_ORDER = 30078;
export const KIND_COMMUNITY_THEME = 30078;
// NIP-33 persona/team/managed-agent projection events (d-tag keyed). Published
// backend-side as secrets-stripped snapshots; the inbound sync hook subscribes
// to all three to patch local records. Mirror of buzz-core's KIND_PERSONA etc.
export const KIND_PERSONA = 30175;
export const KIND_TEAM = 30176;
export const KIND_MANAGED_AGENT = 30177;
export const KIND_USER_STATUS = 30315;
export const KIND_AGENT_OBSERVER_FRAME = 24200;
export const KIND_AGENT_TURN_METRIC = 44200;
export const KIND_EVENT_REMINDER = 30300;
export const KIND_REPO_ANNOUNCEMENT = 30617;
export const KIND_REPO_STATE = 30618;
// NIP-MP: project grouping above NIP-34 repositories.
export const KIND_PROJECT_ANNOUNCEMENT = 30621;
export const KIND_PROJECT = KIND_PROJECT_ANNOUNCEMENT;
export const KIND_GIT_PATCH = 1617;
export const KIND_GIT_PULL_REQUEST = 1618;
export const KIND_GIT_PR_UPDATE = 1619;
export const KIND_GIT_ISSUE = 1621;
export const KIND_GIT_STATUS_OPEN = 1630;
export const KIND_GIT_STATUS_MERGED = 1631;
export const KIND_GIT_STATUS_CLOSED = 1632;
export const KIND_GIT_STATUS_DRAFT = 1633;
// NIP-DV: relay-signed per-viewer DM visibility snapshot (d=viewer pubkey,
// h-tags = currently-hidden DM channel ids).
export const KIND_DM_VISIBILITY = 30622;
// NIP-ST: shared terminals. 30623 announces an open built-in-shell session
// into its project (addressable, d=session id, a=project coordinate);
// 24310 is the ephemeral observer→owner watch/keepalive/resync; 24311 is the
// ephemeral owner→observers frame stream (base64 terminal bytes); 24312 is
// the ephemeral collaborator→owner input stream (base64 keystrokes),
// relay-gated to the 30623 roster's collaborators and re-verified by the
// owner host before any byte reaches the PTY.
// Keep in sync: crates/buzz-core/src/kind.rs and mobile
// lib/shared/relay/nostr_models.dart.
export const KIND_SHELL_SESSION = 30623;
export const KIND_SHELL_WATCH = 24310;
export const KIND_SHELL_FRAME = 24311;
export const KIND_SHELL_INPUT = 24312;
// LANE-L23: a project's persona-pack source. Addressable, d = the project
// coordinate `30621:<owner-hex>:<slug>`. Author must be a founder of one of
// the project's repositories (L18's RepositoryFounders) or the project
// owner — the relay refuses others. Tags: `["repo", "30617:<owner>:<id>"]`
// (the packs repository), exactly one of `["ref", "refs/heads/main"]` or
// `["sha", "<40-hex>"]`, optional `["path", "personas/roles"]` (default
// `personas/roles`). Content: `{"schema":"buzz-project-pack-source/v1",
// "note":"<=512 bytes, optional>"}`.
// Keep in sync: crates/buzz-core/src/kind.rs (Lane A — not yet landed as of
// this lane's cut; 30624 is confirmed free, immediately after 30623).
export const KIND_PROJECT_PACK_SOURCE = 30624;

// NIP-MP membership ops: 9010 puts members (add or change role) on a project
// roster, 9011 removes them; both carry the project coordinate in an `a` tag
// and are accepted only from the project creator or a roster owner. 39010 is
// the relay-signed addressable roster projection (d=project coordinate, one
// ["p", hex, "", role] per member).
export const KIND_PROJECT_PUT_MEMBER = 9010;
export const KIND_PROJECT_REMOVE_MEMBER = 9011;
export const KIND_PROJECT_MEMBERS = 39010;

// ── Coding sessions (44220–44230) ────────────────────────────────────────────
//
// Provider-neutral kinds for driving a coding agent against a working
// directory. All thirteen, including the ephemeral 24223 lease, are
// channel-scoped (`h` tag) and never enter
// CHANNEL_TIMELINE_CONTENT_KINDS: a session's turns and transcript belong to
// its own workspace surface, not to the chat timeline. Names mirror
// crates/buzz-core/src/kind.rs — keep them in sync.

// NIP-CSC: operator-authored durable turn command (`csc1-1`).
export const KIND_CODING_SESSION_COMMAND = 44220;
// NIP-CSL: operator-authored request to create a session (`csl1-1`). Kept
// separate from the turn command because an exact generation target does not
// exist until the provider creates it.
export const KIND_CODING_SESSION_LIFECYCLE_COMMAND = 44221;
// NIP-CSPC: provider-authored availability catalog (`cspc1-1`). Queried
// separately from chat so message volume cannot age discovery out of a bounded
// history window.
export const KIND_CODING_SESSION_PROVIDER_CATALOG = 44222;
// NIP-CSL: ephemeral provider-signed liveness lease for one exact generation.
// Stored only in Redis with TTL; cold REQ reads return the current snapshot.
export const KIND_CODING_SESSION_LEASE = 24223;
// NIP-CSL: provider-authored immutable facts about one exact generation
// (`csm1-1`).
export const KIND_CODING_SESSION_METADATA = 44223;
// NIP-CSL: provider-authored result of one lifecycle command (`cslr1-1`).
export const KIND_CODING_SESSION_LIFECYCLE_RECEIPT = 44224;
// NIP-CST: one provider-authored, sequenced transcript step (`cst1-1`).
export const KIND_CODING_SESSION_TRANSCRIPT = 44225;
// NIP-CSG: the operator-signed origin of one umbrella session (`csg1-1`). The
// signer is the founder. Consumers resolve it only by the explicit event id in
// a receipt-joined create; `csg-session` is relay enforcement/diagnostics only.
export const KIND_CODING_SESSION_GENESIS = 44226;
// NIP-CSGL: operator-authored append-only goal revision (`csgl1-1`). Regular
// events preserve every revision; consumers fold the latest per (h, d).
export const KIND_CODING_SESSION_GOAL = 44227;
// NIP-CSAT (draft): one append-only authority-chain transition (`csat1-1`).
// Types: `grant-operator` (steer), `grant-viewer` (read-only), `revoke`; the
// relay validates chain linkage (prevAccepted/seq against the current
// accepted head) and owner standing at ingest and publishes a relay-signed
// acceptance receipt (kind 40099).
export const KIND_CODING_SESSION_AUTHORITY_TRANSITION = 44228;
// NIP-CSN: operator-authored append-only short session-name revision
// (`csnm1-1`). Regular events preserve every rename.
export const KIND_CODING_SESSION_NAME = 44229;
// NIP-CSCL: member-authored append-only session-closure revision (`cscl1-1`).
// Closing is founder-only; any relay-accepted member may reopen.
export const KIND_CODING_SESSION_CLOSURE = 44230;
// NIP-CSTX: actor-authored portable team transactions folded canonically by
// buzz-core (`buzz-coding-session-team-transaction/v1`).
export const KIND_CODING_SESSION_TEAM_TRANSACTION = 44244;

/**
 * Kind:44245 session policy (NIP-CSP), addressable by `d` = sessionRef.
 *
 * Declared here so no surface writes the integer itself. Desktop never encodes
 * or decodes the record — that belongs to `buzz-core` behind
 * `desktop/src-tauri/src/commands/coding_session_policy.rs` — but the launch
 * form names the kind when it says what pressing the button publishes, and a
 * literal there could drift from the wire without anything failing.
 */
export const KIND_CODING_SESSION_POLICY = 44245;

/**
 * Kind:44246 observation (NIP-CSOB) — a checkpoint, a gate row, a finding or
 * a phase's own measured span.
 *
 * Declared here for the same reason 44245 is, and read by nothing in Desktop
 * yet: the renderer is owed (ledger item 108, lane L5). The integer is
 * mirrored in `mobile/lib/shared/relay/nostr_models.dart`, which CLAUDE.md
 * requires not to drift from this file.
 */
export const KIND_CODING_SESSION_OBSERVATION = 44246;

// ── Project Pulse (44240) ────────────────────────────────────────────────────
//
// NIP-PU: an author's explicit claim about a project — a plan, milestone,
// note, handoff, or blocker (`pu1-1`). Project-scoped by an `a` tag holding
// the canonical 30621 coordinate, never by `h`, and append-only: a revision
// supersedes its predecessor by event id, and the fold honors that only for
// the same author (features/project-pulse/lib/pulseFold.ts). It carries no
// observed fact — worktree state stays in the coding-session kinds.
export const KIND_PULSE_ENTRY = 44240;

// The coding-session kinds Desktop's own consumer reads, in one place, so the
// regression guard keeping them out of the chat timeline cannot silently miss a
// newly added member.
//
// It is NOT every allocated coding-session kind: 44245 (policy) and 44246
// (observation) are deliberately absent, because Desktop does not read either
// off the wire yet and this list also drives which kinds the e2e mock relay
// serves (`src/testing/e2eBridgeSessionFacts.ts`). Nothing leaks from the
// omission — the chat timeline is an allowlist
// (`CHANNEL_TIMELINE_CONTENT_KINDS`), so a kind absent from both sets is
// absent from the timeline too. Add them here when a reader for them lands,
// and bump the count in `kinds.test.mjs`.
export const CODING_SESSION_EVENT_KINDS = [
  KIND_CODING_SESSION_LEASE,
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
] as const;

// Human-visible "new content" message kinds. Used as the unread trigger set
// (sidebar badges, catch-up queries) and as the Home-feed mention query.
// Reactions, edits, diffs, deletions, and system messages are deliberately
// excluded: they can land after the last human-visible message and would
// otherwise create phantom unreads.
//
// One member of this set is not decidable by kind alone: a kind:9 carrying a
// `cs-session` tag is a coding-session lane message *if* this client can open
// that lane, in which case it is invisible in the channel timeline and must
// not trigger unread either. That is filtered per event, with the same rule the
// timeline uses — `isCodingSessionLaneMessageHiddenFromChannel` in
// features/messages/lib/codingSessionLaneVisibility.ts. Any new consumer of
// this kind set that drives badges, notifications, or the mention feed must
// apply it too.
export const CHANNEL_MESSAGE_EVENT_KINDS = [
  KIND_STREAM_MESSAGE,
  KIND_STREAM_MESSAGE_V2,
  KIND_FORUM_POST,
  KIND_FORUM_COMMENT,
] as const;

// Keep this in sync with the Home-feed mention query in buzz-db.
export const HOME_MENTION_EVENT_KINDS = [...CHANNEL_MESSAGE_EVENT_KINDS];

export const CHANNEL_EVENT_KINDS = [
  KIND_DELETION, // 5 — NIP-09 event deletions
  KIND_REACTION, // 7 — NIP-25 reactions
  KIND_NIP29_DELETE_EVENT, // 9005 — NIP-29 / Buzz-native deletions
  ...CHANNEL_MESSAGE_EVENT_KINDS,
  40001, // legacy: pre-migration stream messages
  KIND_STREAM_MESSAGE_EDIT, // 40003 — message edits
  KIND_STREAM_MESSAGE_DIFF, // 40008 — message diffs
  KIND_SYSTEM_MESSAGE, // 40099 — system messages (join, leave, etc.)
  KIND_HUDDLE_STARTED, // 48100 — visible huddle session card
  KIND_HUDDLE_PARTICIPANT_JOINED, // 48101 — huddle lifecycle overlay
  KIND_HUDDLE_PARTICIPANT_LEFT, // 48102 — huddle lifecycle overlay
  KIND_HUDDLE_ENDED, // 48103 — huddle lifecycle overlay
] as const;

// Auxiliary (non-row) timeline kinds: events that overlay onto or hide an
// existing message rather than rendering their own row — reactions, edits, and
// deletions. History fetches request the visible content kinds only, so the
// `limit` budget buys visible message depth instead of being diluted by these
// (on a reaction-heavy channel a 200-event window was only ~136 messages).
// They are backfilled separately by `#e` reference over the loaded message ids
// — by reference, not by time window, so a late edit/delete for a visible old
// message still applies. NOTE: kind:40008 (diff) renders its OWN row, so it is
// a content kind, not aux.
export const CHANNEL_AUX_EVENT_KINDS = [
  KIND_DELETION, // 5 — NIP-09 event deletions
  KIND_REACTION, // 7 — NIP-25 reactions
  KIND_NIP29_DELETE_EVENT, // 9005 — NIP-29 / Buzz-native deletions
  KIND_STREAM_MESSAGE_EDIT, // 40003 — message edits
] as const;

// Visible content kinds the main timeline renders as their own rows. Mirrors
// `isTimelineContentEvent` in formatTimelineMessages.ts — keep the two in sync.
// This is the kind set the history fetch requests so the `limit` budget maps
// to visible rows; auxiliary overlays (CHANNEL_AUX_EVENT_KINDS) are fetched
// separately by `#e` reference. Forum kinds (45001/45003) are excluded: forum
// channels use a different query path, not this timeline.
export const CHANNEL_TIMELINE_CONTENT_KINDS = [
  KIND_STREAM_MESSAGE, // 9
  KIND_STREAM_MESSAGE_V2, // 40002
  KIND_STREAM_MESSAGE_DIFF, // 40008 — diff messages (own row)
  KIND_SYSTEM_MESSAGE, // 40099 — system rows (join/leave/channel-created)
  KIND_JOB_REQUEST, // 43001
  KIND_JOB_ACCEPTED, // 43002
  KIND_JOB_PROGRESS, // 43003
  KIND_JOB_RESULT, // 43004
  KIND_JOB_CANCEL, // 43005
  KIND_JOB_ERROR, // 43006
  KIND_HUDDLE_STARTED, // 48100 — huddle session card
] as const;

// Timeline kinds that are NOT conversational: relay-signed system rows
// (channel-created, member-joined) and job-lifecycle events. These render in
// the timeline but must not count toward the channel's unread pill — a freshly
// created channel carries one channel_created + N member_joined system rows
// that would otherwise show as phantom unreads ("4 unread, 1 message").
const NON_CONVERSATIONAL_UNREAD_KINDS: ReadonlySet<number> = new Set([
  KIND_SYSTEM_MESSAGE, // 40099
  KIND_JOB_REQUEST, // 43001
  KIND_JOB_ACCEPTED, // 43002
  KIND_JOB_PROGRESS, // 43003
  KIND_JOB_RESULT, // 43004
  KIND_JOB_CANCEL, // 43005
  KIND_JOB_ERROR, // 43006
  KIND_HUDDLE_STARTED, // 48100 — huddle cards are visible but non-conversational
  KIND_HUDDLE_PARTICIPANT_JOINED, // 48101
  KIND_HUDDLE_PARTICIPANT_LEFT, // 48102
  KIND_HUDDLE_ENDED, // 48103
]);

// Whether a timeline message kind should count toward unread tallies. An
// undefined kind (optimistic/pending rows whose kind has not populated) is
// treated as conversational so a legitimately unread message is never dropped.
export function isConversationalUnreadKind(kind: number | undefined): boolean {
  return kind === undefined || !NON_CONVERSATIONAL_UNREAD_KINDS.has(kind);
}
