//! Project-scoped PostgreSQL purge: reclaim the storage a soft-deleted
//! project still occupies.
//!
//! # Relationship to whole-community deletion
//!
//! [`crate::deletion`] owns the durable, staged, cross-store deletion of an
//! entire tenant. This module is deliberately *not* a second copy of that
//! machinery: it reuses its vocabulary (frozen inventory → digest-bound
//! approval → destructive transaction), its safety primitives
//! ([`crate::deletion::SCHEMA_DESTRUCTION_LOCK_KEY`], the
//! `buzz.nip_rs_hard_delete` opt-in), and its error type
//! ([`crate::DbError::DeletionSafety`]), but it does **not** add a staged
//! control plane.
//!
//! A staged control plane exists for whole-community deletion because that
//! operation spans three stores (PostgreSQL, S3, Redis), must fence serving
//! writes across replicas, runs long enough to crash mid-flight, and is
//! irreversible at tenant scale. None of that holds here:
//!
//! * **One store.** Object storage is keyed by *community* prefixes
//!   (`_meta/<community>/`, `_uploads/<community>/`, `repos/<community>/`),
//!   never by project, so a project purge has no S3 or Redis work to stage.
//! * **No fence needed.** Every row this reaches should already be unreachable
//!   through the serving path — the project head is tombstoned and its
//!   channels are soft-deleted. The community itself stays `active`; the
//!   universal write fence (`enforce_community_write_fence`) admits these
//!   deletes normally.
//!
//!   "Should" is doing real work in that sentence, so it is checked rather
//!   than assumed. Nothing actually *stops* a write into a soft-deleted
//!   channel: `check_channel_membership` admits an existing member on their
//!   `channel_members` row, which soft-deleting a channel does not remove. A
//!   row committed between [`compute_scope`] and the first `DELETE` would
//!   otherwise be destroyed without ever entering the digest the operator
//!   approved. [`ProjectPurgeStore::purge`] therefore proves *equality* with
//!   the approved inventory — not just absence afterwards — and aborts the
//!   whole transaction if any unit deleted a different number of rows than
//!   was counted. That is the durable write fence's guarantee reconstructed
//!   from inside the one transaction, which is all a single-store,
//!   single-transaction operation needs.
//! * **Bounded.** One throwaway project fits comfortably in one transaction,
//!   so "crash-resumable" reduces to "atomic".
//!
//! What is *kept* from the staged shape is the part that carries the safety:
//! an operator inspects a frozen inventory, and the destructive command
//! refuses unless the inventory recomputed **inside the purge transaction**
//! still hashes to the digest the operator approved. That is strictly
//! stronger than a stored manifest, which can go stale between freeze and
//! execution.
//!
//! # What "already soft-deleted" means here
//!
//! Purge is never a shortcut around authorization. Two tiers, both enforced:
//!
//! 1. **Row-level tombstone.** Rows that carry their own `deleted_at` and are
//!    the *anchors* of the scope — the `channels` rows and the coordinate's
//!    own addressable `events` (kind 30621 head, kind 39010 roster) — are in
//!    scope only when `deleted_at IS NOT NULL`. A live one is never purged.
//!
//! 2. **Tombstoned-parent cascade.** Rows that have no soft-delete concept of
//!    their own (`channel_members`, `thread_metadata`, `workflows`,
//!    `coding_session_authority_acl`, …) are in scope when their owning
//!    `channel` row is a tier-1 anchor. The authorizing act is that channel's
//!    own kind:9008 tombstone, travelled through the normal path.
//!
//! [`ProjectPurgeScope::blockers`] makes tier 2 safe: if **any** channel
//! carrying `project_ref = <coordinate>` is still live, or the project head
//! itself is live, or its `project_acl` projection still exists, the purge
//! refuses outright. Tier 2 therefore can never reach a live channel.
//!
//! ## The one place this is broader than `deleted_at IS NOT NULL`
//!
//! Soft-deleting a channel (`soft_delete_channel`) sets `channels.deleted_at`
//! and soft-deletes only the relay's own kind:39000/39001/39002 discovery
//! events. The channel's *messages* keep `deleted_at IS NULL`; they become
//! unreachable rather than tombstoned. This purge deletes them, and it is the
//! **only** class of row it deletes without a tombstone of its own.
//!
//! Leaving them behind is not a safer option, it is a worse one: `project_ref`
//! lives on the `channels` row, so purging the channel while keeping its
//! events would strand those events with no remaining path from the project
//! coordinate back to them — permanently unreclaimable, and invisible to any
//! later purge. Either both go or neither does.
//!
//! So they are included, but never silently:
//! [`ProjectPurgeScope::live_events_in_tombstoned_channels`] reports exactly
//! how many (and, being part of the scope, moves the approval digest), and
//! [`ProjectPurgeStore::purge`] **refuses** while that count is non-zero
//! unless the operator passes the separate `acknowledge_live_events` opt-in
//! (`--acknowledge-live-messages`). `--confirm` does not imply it.

use std::collections::BTreeMap;

use buzz_core::CommunityId;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{AssertSqlSafe, PgConnection, PgPool, Row};
use uuid::Uuid;

use crate::deletion::lock_schema_destruction_shared;
use crate::error::{DbError, Result};

/// Nostr kind of a NIP-MP project head (`buzz_core::kind::KIND_PROJECT`).
const KIND_PROJECT: i32 = 30621;
/// Nostr kind of the relay-signed project roster projection
/// (`buzz_core::kind::KIND_PROJECT_MEMBERS`).
const KIND_PROJECT_MEMBERS: i32 = 39010;

/// Live tables that carry a project coordinate and therefore define this
/// purge's surface.
///
/// Mirrors [`crate::deletion::EXPECTED_SCOPED_TABLES`]: the live catalog is
/// compared against this exact set before the destructive transaction
/// commits, so a migration that introduces a new project-scoped table blocks
/// the purge until this manifest is intentionally updated rather than
/// silently leaving rows behind.
///
/// Membership rule: a `public` table with a `project_ref` column (the
/// coordinate a row belongs to) or a `coordinate` column (a projection keyed
/// by a coordinate).
pub const PROJECT_COORDINATE_TABLES: &[&str] = &[
    "channels",
    "git_repo_names",
    "project_acl",
    "shell_session_acl",
];

/// Live tables that reach this purge's rows through `channel_id`.
///
/// Ten of the sixteen [`purge_units`] find their rows this way, not through a
/// coordinate column: the project's tombstoned `channels` are the anchor, and
/// `project_ref` lives on the `channels` row that is deleted **last**. A
/// migration that adds a `channel_id` table without adding a purge unit for it
/// would therefore leave rows behind carrying a dangling `channel_id` with no
/// remaining path back from the project coordinate — the same "permanently
/// unreclaimable" outcome the module docs invoke to justify deleting live
/// messages. `coding_session_authority_acl` is the in-tree proof that this
/// class is not hypothetical: it carries neither `project_ref` nor
/// `coordinate`, so [`PROJECT_COORDINATE_TABLES`] cannot see it at all.
///
/// Membership rule: a `public` table with a `channel_id` column. Compared
/// against the live catalog inside the purge transaction, exactly like
/// [`PROJECT_COORDINATE_TABLES`], and a hard failure on drift.
pub const PROJECT_CHANNEL_SCOPED_TABLES: &[&str] = &[
    "channel_members",
    "coding_session_authority_acl",
    "event_mentions",
    "events",
    "moderation_actions",
    "moderation_reports",
    "thread_metadata",
    "workflows",
];

/// Tables deliberately left untouched by a project purge, with the reason.
///
/// Reported verbatim on every inventory and receipt so a purge can never read
/// as "reclaimed everything" when it did not.
pub const PROJECT_PURGE_EXCLUSIONS: &[(&str, &str)] = &[
    (
        "git_repo_names",
        "repositories are detached, never deleted — dropping the row would free \
         the community-unique repo name for another owner to squat",
    ),
    (
        "shell_session_acl",
        "roster projection of kind:30623 announce heads, which a project cascade \
         never tombstones; the rows are live. Enforced, not assumed: an announce \
         h-tagged into one of this project's tombstoned channels would be \
         destroyed by the channel arm and leave its projection row pointing at \
         nothing, so the purge blocks on that instead \
         (shell_announces_in_tombstoned_channels)",
    ),
    (
        "shell_session_acl_members",
        "child of shell_session_acl (ON DELETE CASCADE); retained with its parent",
    ),
    (
        "parameterized_event_watermarks",
        "NIP-RS replay guard; dropping the watermark for a purged coordinate \
         re-opens a replay window",
    ),
    (
        "delivery_log",
        "operator delivery evidence, partitioned by delivered_at; carries no \
         tenant-visible content",
    ),
    (
        "project_acl / project_acl_members",
        "already hard-deleted by the authorized kind:5 tombstone \
         (delete_project_acl); a surviving row means the project is live and \
         blocks the purge",
    ),
    (
        "communities",
        "the community tombstone and host reservation are never touched",
    ),
    (
        "events (kind:30078 NIP-RS read-state, global rows only)",
        "a per-user read marker written by today's ingest is global \
         (channel_id NULL) and its channel contexts are NIP-44 encrypted to \
         the author, so the relay cannot attribute one to a project. Those \
         global rows are never in scope: they are left for the owner or a \
         whole-community deletion. This is NOT a whole-kind exclusion. Rows \
         written before migration 0011 can still carry a channel id, and one \
         inside a tombstoned channel of this project IS hard-deleted with that \
         channel — the purge opts into buzz.nip_rs_hard_delete for exactly \
         that case. Those deletions are counted separately as \
         legacy_channel_scoped_read_state on the inventory and the receipt",
    ),
];

/// The events a project purge is allowed to reach.
///
/// One source of truth, embedded by every unit that needs it:
///
/// * `$2` — the tombstoned channel ids of this project. Their contents go
///   with them (tier 2; see the module docs).
/// * `$3`/`$4` — the project head coordinate, soft-deleted rows only.
/// * `$5` — the relay-signed roster coordinate, soft-deleted rows only.
const SCOPE_EVENT_PREDICATE: &str = "(e.channel_id = ANY($2) \
     OR (e.kind = 30621 AND e.pubkey = $3 AND e.d_tag = $4 AND e.deleted_at IS NOT NULL) \
     OR (e.kind = 39010 AND e.d_tag = $5 AND e.deleted_at IS NOT NULL))";

/// Parameters a purge unit binds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UnitBinds {
    /// `$1` community, `$2` tombstoned channel ids.
    Channels,
    /// `$1` community, `$2` channel ids, `$3` owner, `$4` d tag, `$5` coordinate.
    Coordinate,
}

/// One `(table, predicate)` unit, counted for the inventory and deleted by the
/// purge from the identical predicate.
struct PurgeUnit {
    table: &'static str,
    target: &'static str,
    predicate: &'static str,
    binds: UnitBinds,
}

/// Foreign-key-safe, child-before-parent purge order.
///
/// `channels` is last: `channel_members` and `thread_metadata` reference it,
/// and it carries the `project_ref` that makes every other unit findable.
fn purge_units() -> Vec<PurgeUnit> {
    vec![
        PurgeUnit {
            table: "workflow_approvals",
            target: "workflow_approvals",
            predicate: "community_id = $1 AND workflow_id IN \
                 (SELECT w.id FROM workflows w \
                   WHERE w.community_id = $1 AND w.channel_id = ANY($2))",
            binds: UnitBinds::Channels,
        },
        PurgeUnit {
            table: "scheduled_workflow_fires",
            target: "scheduled_workflow_fires",
            predicate: "community_id = $1 AND workflow_id IN \
                 (SELECT w.id FROM workflows w \
                   WHERE w.community_id = $1 AND w.channel_id = ANY($2))",
            binds: UnitBinds::Channels,
        },
        PurgeUnit {
            table: "workflow_runs",
            target: "workflow_runs",
            predicate: "community_id = $1 AND workflow_id IN \
                 (SELECT w.id FROM workflows w \
                   WHERE w.community_id = $1 AND w.channel_id = ANY($2))",
            binds: UnitBinds::Channels,
        },
        PurgeUnit {
            table: "workflows",
            target: "workflows",
            predicate: "community_id = $1 AND channel_id = ANY($2)",
            binds: UnitBinds::Channels,
        },
        PurgeUnit {
            table: "coding_session_authority_acl",
            target: "coding_session_authority_acl",
            predicate: "community_id = $1 AND channel_id = ANY($2)",
            binds: UnitBinds::Channels,
        },
        PurgeUnit {
            table: "moderation_reports",
            target: "moderation_reports",
            predicate: "community_id = $1 AND channel_id = ANY($2)",
            binds: UnitBinds::Channels,
        },
        PurgeUnit {
            table: "moderation_actions",
            target: "moderation_actions",
            predicate: "community_id = $1 AND channel_id = ANY($2)",
            binds: UnitBinds::Channels,
        },
        PurgeUnit {
            table: "channel_members",
            target: "channel_members",
            predicate: "community_id = $1 AND channel_id = ANY($2)",
            binds: UnitBinds::Channels,
        },
        PurgeUnit {
            table: "thread_metadata",
            target: "thread_metadata",
            predicate: "community_id = $1 AND channel_id = ANY($2)",
            binds: UnitBinds::Channels,
        },
        PurgeUnit {
            table: "event_mentions",
            target: "event_mentions m",
            predicate: "m.community_id = $1 AND EXISTS (SELECT 1 FROM events e \
                 WHERE e.community_id = m.community_id AND e.id = m.event_id \
                   AND SCOPE_EVENT_PREDICATE)",
            binds: UnitBinds::Coordinate,
        },
        PurgeUnit {
            table: "reactions",
            target: "reactions r",
            predicate: "r.community_id = $1 AND EXISTS (SELECT 1 FROM events e \
                 WHERE e.community_id = r.community_id \
                   AND e.created_at = r.event_created_at AND e.id = r.event_id \
                   AND SCOPE_EVENT_PREDICATE)",
            binds: UnitBinds::Coordinate,
        },
        PurgeUnit {
            table: "push_match_queue",
            target: "push_match_queue q",
            predicate: "q.community_id = $1 AND EXISTS (SELECT 1 FROM events e \
                 WHERE e.community_id = q.community_id AND e.id = q.event_id \
                   AND SCOPE_EVENT_PREDICATE)",
            binds: UnitBinds::Coordinate,
        },
        PurgeUnit {
            table: "push_wake_outbox",
            target: "push_wake_outbox o",
            predicate: "o.community_id = $1 AND EXISTS (SELECT 1 FROM events e \
                 WHERE e.community_id = o.community_id AND e.id = o.event_id \
                   AND SCOPE_EVENT_PREDICATE)",
            binds: UnitBinds::Coordinate,
        },
        PurgeUnit {
            table: "events",
            target: "events e",
            predicate: "e.community_id = $1 AND SCOPE_EVENT_PREDICATE",
            binds: UnitBinds::Coordinate,
        },
        PurgeUnit {
            table: "channels",
            target: "channels",
            predicate: "community_id = $1 AND id = ANY($2) AND deleted_at IS NOT NULL",
            binds: UnitBinds::Channels,
        },
    ]
}

impl PurgeUnit {
    fn where_clause(&self) -> String {
        self.predicate
            .replace("SCOPE_EVENT_PREDICATE", SCOPE_EVENT_PREDICATE)
    }

    fn count_sql(&self) -> String {
        format!(
            "SELECT COUNT(*) FROM {} WHERE {}",
            self.target,
            self.where_clause()
        )
    }

    fn delete_sql(&self) -> String {
        format!("DELETE FROM {} WHERE {}", self.target, self.where_clause())
    }
}

/// A parsed, normalized NIP-MP project coordinate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectCoordinate {
    /// Canonical `30621:<lowercase-hex-owner>:<dtag>`.
    ///
    /// The lowercase hex owner is the middle segment of this string, and the
    /// raw bytes are in [`Self::owner`]; there is deliberately no third,
    /// separately-stored hex copy to drift from either.
    pub coordinate: String,
    /// 32-byte owner pubkey.
    pub owner: Vec<u8>,
    /// The project's `d` tag (slug).
    pub dtag: String,
}

impl ProjectCoordinate {
    /// Parse `30621:<owner-hex>:<dtag>`, normalizing the owner to lowercase.
    ///
    /// Rejects anything [`buzz_core::kind::normalize_project_coordinate`]
    /// rejects, so the operator CLI and the relay's ingest validation cannot
    /// drift on what a project coordinate is.
    pub fn parse(value: &str) -> Result<Self> {
        let coordinate =
            buzz_core::kind::normalize_project_coordinate(value.trim()).ok_or_else(|| {
                DbError::DeletionSafety(format!(
                    "not a well-formed project coordinate: {value:?} \
                     (expected 30621:<64-hex-owner>:<slug>)"
                ))
            })?;
        let mut parts = coordinate.splitn(3, ':');
        let (_, owner_hex, dtag) = (
            parts.next().unwrap_or_default(),
            parts.next().unwrap_or_default().to_owned(),
            parts.next().unwrap_or_default().to_owned(),
        );
        let owner = hex::decode(&owner_hex).map_err(|error| {
            DbError::DeletionSafety(format!("project owner is not hex: {error}"))
        })?;
        Ok(Self {
            coordinate,
            owner,
            dtag,
        })
    }
}

/// Anchor-row tombstone census for one addressable coordinate.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TombstoneCensus {
    /// Rows with `deleted_at IS NOT NULL` — in scope.
    pub tombstoned: i64,
    /// Rows with `deleted_at IS NULL` — never in scope.
    pub live: i64,
}

/// Everything a project purge would destroy, and everything it would keep.
///
/// This is the value the approval digest is taken over; it deliberately
/// excludes the digest itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectPurgeScope {
    /// Tenant this purge is fenced to.
    pub community_id: Uuid,
    /// Canonical project coordinate.
    pub coordinate: String,
    /// Soft-deleted channels bound to the coordinate — the purge anchors.
    pub tombstoned_channels: Vec<Uuid>,
    /// Live channels bound to the coordinate. Any entry blocks the purge.
    pub live_channels: Vec<Uuid>,
    /// kind:30621 head rows for the coordinate.
    pub project_head: TombstoneCensus,
    /// kind:39010 relay-signed roster rows for the coordinate.
    pub project_roster: TombstoneCensus,
    /// Surviving `project_acl` projection rows. Any entry blocks the purge.
    pub project_acl_rows: i64,
    /// Rows this purge would delete, per table.
    pub purge_rows: BTreeMap<String, i64>,
    /// Rows this purge deliberately keeps, per table. See
    /// [`PROJECT_PURGE_EXCLUSIONS`] for the reasons.
    pub retained_rows: BTreeMap<String, i64>,
    /// Events inside the tombstoned channels that are themselves still
    /// `deleted_at IS NULL`.
    ///
    /// **These are purged.** A soft-deleted channel does not tombstone its
    /// messages, only makes them unreachable; keeping them while dropping the
    /// `channels` row that carries `project_ref` would strand them forever.
    /// Surfaced as its own number so the trade is never silent.
    pub live_events_in_tombstoned_channels: i64,
    /// Legacy channel-scoped kind:30078 NIP-RS read-state rows inside the
    /// tombstoned channels.
    ///
    /// **These are hard-deleted**, and they are the one part of the read-state
    /// exclusion that is *not* a retention promise. Today's ingest stores a
    /// read marker with `channel_id IS NULL`, so a conforming row is never in
    /// scope; rows written before migration 0011 can carry a channel id, and
    /// one of those inside a tombstoned channel goes with the channel. Counted
    /// on its own rather than folded anonymously into `purge_rows["events"]`
    /// so an operator can see it happened.
    pub legacy_channel_scoped_read_state: i64,
    /// kind:30623 NIP-ST shell-session announces h-tagged into the tombstoned
    /// channels. Any entry blocks the purge.
    ///
    /// `shell_session_acl` is reported as retained, so destroying the announce
    /// its row projects would make that report a lie and strand the row with a
    /// coordinate that resolves to nothing. No in-tree client h-tags a 30623
    /// (the desktop and CLI announce builders emit `d`/`a`/`status`/`title`/
    /// `dims`/`p` only), but nothing in ingest forbids it, so this is checked
    /// rather than assumed.
    pub shell_announces_in_tombstoned_channels: i64,
    /// Why this scope cannot be purged. Empty means purgeable.
    pub blockers: Vec<String>,
}

impl ProjectPurgeScope {
    /// Hex SHA-256 over the canonical JSON encoding of this scope.
    ///
    /// Binds operator approval to an exact observation, the same role
    /// [`crate::deletion::FrozenInventory::digest`] plays for whole-community
    /// deletion.
    pub fn digest(&self) -> Result<String> {
        Ok(hex::encode(Sha256::digest(serde_json::to_vec(self)?)))
    }

    /// Total rows this purge would delete.
    pub fn total_rows(&self) -> i64 {
        self.purge_rows.values().sum()
    }
}

/// A frozen scope plus its digest — the operator-facing inventory document.
#[derive(Debug, Clone, Serialize)]
pub struct ProjectPurgeInventory {
    /// The observed scope.
    pub scope: ProjectPurgeScope,
    /// Digest to pass back as `--approved-digest`.
    pub digest: String,
    /// Table exclusions and their reasons, verbatim.
    pub exclusions: Vec<ExclusionNote>,
}

/// One deliberate exclusion, reported so scope reduction is never silent.
#[derive(Debug, Clone, Serialize)]
pub struct ExclusionNote {
    /// Table (or table pair) left untouched.
    pub table: String,
    /// Why.
    pub reason: String,
}

fn exclusion_notes() -> Vec<ExclusionNote> {
    PROJECT_PURGE_EXCLUSIONS
        .iter()
        .map(|(table, reason)| ExclusionNote {
            table: (*table).to_owned(),
            reason: (*reason).to_owned(),
        })
        .collect()
}

/// Evidence of a committed purge.
#[derive(Debug, Clone, Serialize)]
pub struct ProjectPurgeReceipt {
    /// Tenant the purge was fenced to.
    pub community_id: Uuid,
    /// Coordinate purged.
    pub coordinate: String,
    /// Digest the operator approved and the transaction re-verified.
    pub approved_digest: String,
    /// Operator identity recorded on the purge.
    pub purged_by: String,
    /// Optional operator note.
    pub note: Option<String>,
    /// Rows actually deleted, per table.
    pub deleted_rows: BTreeMap<String, u64>,
    /// Rows deliberately kept, per table.
    pub retained_rows: BTreeMap<String, i64>,
    /// Events hard-deleted despite carrying no tombstone of their own,
    /// because their owning channel was tombstoned. Non-zero only when the
    /// operator passed the explicit acknowledgement.
    pub acknowledged_live_events: i64,
    /// Legacy channel-scoped kind:30078 NIP-RS read-state rows destroyed by
    /// this purge, broken out of `deleted_rows["events"]`.
    ///
    /// The read-state exclusion promises only that *global* markers survive.
    /// This is the number it does not cover, reported so a receipt can never
    /// read as "read state was untouched" when it was not. See
    /// [`ProjectPurgeScope::legacy_channel_scoped_read_state`].
    pub deleted_legacy_read_state: i64,
    /// Table exclusions and their reasons, verbatim.
    pub exclusions: Vec<ExclusionNote>,
}

/// Operator-only, CLI-only project purge store.
///
/// There is deliberately no HTTP surface: the deployment admin router is
/// read-only by contract, and a project purge is an irreversible operator
/// action gated on shell access to the relay host.
#[derive(Clone)]
pub struct ProjectPurgeStore {
    pool: PgPool,
}

impl ProjectPurgeStore {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Freeze and return the purge inventory for one project coordinate.
    ///
    /// Read-only. The returned digest is what
    /// [`Self::purge`] requires as `approved_digest`.
    pub async fn inventory(
        &self,
        community: CommunityId,
        coordinate: &ProjectCoordinate,
    ) -> Result<ProjectPurgeInventory> {
        let mut conn = self.pool.acquire().await?;
        validate_project_catalog(&mut conn).await?;
        let scope = compute_scope(&mut conn, community, coordinate).await?;
        let digest = scope.digest()?;
        Ok(ProjectPurgeInventory {
            scope,
            digest,
            exclusions: exclusion_notes(),
        })
    }

    /// Hard-delete one soft-deleted project's rows in a single transaction.
    ///
    /// Refuses unless, recomputed inside that transaction:
    ///
    /// * the live project-coordinate catalog still matches
    ///   [`PROJECT_COORDINATE_TABLES`];
    /// * the scope has no [`ProjectPurgeScope::blockers`] — nothing live;
    /// * the scope hashes to exactly `approved_digest`.
    ///
    /// `acknowledge_live_events` is the explicit opt-in for the one class of
    /// row this purge deletes without a tombstone of its own: messages inside
    /// an already-tombstoned channel, which
    /// [`ProjectPurgeScope::live_events_in_tombstoned_channels`] counts. When
    /// that count is non-zero and the flag is `false`, the purge refuses
    /// rather than silently including them.
    ///
    /// After deleting, every unit is recounted and must observe zero rows.
    pub async fn purge(
        &self,
        community: CommunityId,
        coordinate: &ProjectCoordinate,
        approved_digest: &str,
        purged_by: &str,
        note: Option<&str>,
        acknowledge_live_events: bool,
    ) -> Result<ProjectPurgeReceipt> {
        let purged_by = purged_by.trim();
        if purged_by.is_empty() {
            return Err(DbError::DeletionSafety(
                "project purge requires a non-empty operator identity".to_string(),
            ));
        }
        let approved_digest = approved_digest.trim().to_ascii_lowercase();

        let mut tx = self.pool.begin().await?;
        // Same exclusion the community purge takes: migrations hold the
        // exclusive counterpart for their whole run, so the catalog validated
        // below cannot change before this transaction commits.
        lock_schema_destruction_shared(&mut tx).await?;
        // The lock above is *shared* on purpose — migrations hold its exclusive
        // counterpart — so it does not serialize two purges against each other.
        // Without this second, exclusive, per-coordinate lock both would freeze
        // the same scope, both would pass the digest check, and the loser would
        // block on the winner's row locks and then commit a receipt with
        // all-zero `deleted_rows` beside a non-zero `acknowledged_live_events`:
        // evidence that reads as "nothing was there" for a purge that deleted
        // nothing only because someone else already had.
        lock_project_coordinate(&mut tx, community, coordinate).await?;
        validate_project_catalog(&mut tx).await?;

        let scope = compute_scope(&mut tx, community, coordinate).await?;
        if !scope.blockers.is_empty() {
            return Err(DbError::DeletionSafety(format!(
                "project {} is not purgeable: {}",
                coordinate.coordinate,
                scope.blockers.join("; ")
            )));
        }
        let observed_digest = scope.digest()?;
        if observed_digest != approved_digest {
            return Err(DbError::DeletionSafety(format!(
                "project {} inventory changed since approval \
                 (approved {approved_digest}, observed {observed_digest}); \
                 re-run `project-purge inventory`",
                coordinate.coordinate
            )));
        }

        // The only rows this purge hard-deletes without a tombstone of their
        // own. Soft-deleting a channel does not soft-delete its messages, and
        // `project_ref` lives on the `channels` row, so keeping them while
        // dropping the channel would strand them permanently. The trade is
        // real, so it is acknowledged explicitly, never assumed.
        if scope.live_events_in_tombstoned_channels > 0 && !acknowledge_live_events {
            return Err(DbError::DeletionSafety(format!(
                "project {} holds {} event(s) inside its tombstoned channels that \
                 are not themselves soft-deleted; a channel tombstone does not \
                 tombstone its messages. Re-run acknowledging that they are \
                 hard-deleted with the channel, or leave the project unpurged",
                coordinate.coordinate, scope.live_events_in_tombstoned_channels
            )));
        }

        // Migration 0011 fences hard deletion of NIP-RS read-state rows
        // against legacy writers (`trg_events_guard_nip_rs_hard_delete`,
        // BEFORE DELETE ON events WHEN kind = 30078 AND the read-state d tag).
        //
        // Today's ingest stores kind:30078 with `channel_id = NULL`
        // (`is_global_only_kind`), so a conforming row is never in this
        // purge's scope — see the read-state entry in
        // [`PROJECT_PURGE_EXCLUSIONS`]. Rows written before that guard existed
        // can still carry a channel id, and one of those inside a tombstoned
        // channel would abort the whole transaction. Opt in transaction-locally
        // (`is_local = true`, so it is scoped to this transaction and reverts
        // on commit or rollback) rather than let a legacy row make the purge
        // unrunnable.
        sqlx::query("SELECT set_config('buzz.nip_rs_hard_delete', 'on', true)")
            .execute(&mut *tx)
            .await?;

        let community_uuid = *community.as_uuid();
        let channels = scope.tombstoned_channels.clone();
        let mut deleted_rows = BTreeMap::new();
        for unit in purge_units() {
            let affected =
                unit_delete(&mut tx, &unit, community_uuid, &channels, coordinate).await?;
            deleted_rows.insert(unit.table.to_owned(), affected);
        }

        // Equality proof against the approved inventory. `compute_scope`
        // counted these rows at the top of the transaction; the DELETEs run
        // under READ COMMITTED and so take their own, later snapshots. A row
        // committed in that window would be destroyed without ever entering
        // the digest the operator approved — and unlike whole-community
        // deletion there is no durable write fence stopping it: the module's
        // "no serving write targets them" claim is unenforced, because
        // `check_channel_membership` still admits an existing member writing
        // into a soft-deleted channel. So verify the claim rather than assert
        // it: deleting *more* than was approved fails the whole transaction.
        for unit in purge_units() {
            let approved = scope
                .purge_rows
                .get(unit.table)
                .copied()
                .unwrap_or_default();
            let actual = i64::try_from(deleted_rows.get(unit.table).copied().unwrap_or_default())
                .unwrap_or(i64::MAX);
            if actual != approved {
                return Err(DbError::DeletionSafety(format!(
                    "project {} deleted {actual} rows from {} but the approved \
                     inventory counted {approved}; the scope moved inside the \
                     purge transaction — re-run `project-purge inventory`",
                    coordinate.coordinate, unit.table
                )));
            }
        }

        // Absence proof over the exact predicates that just ran.
        for unit in purge_units() {
            let remaining =
                unit_count(&mut tx, &unit, community_uuid, &channels, coordinate).await?;
            if remaining != 0 {
                return Err(DbError::DeletionSafety(format!(
                    "project {} purge left {remaining} rows in {}",
                    coordinate.coordinate, unit.table
                )));
            }
        }

        tx.commit().await?;
        Ok(ProjectPurgeReceipt {
            community_id: *community.as_uuid(),
            coordinate: coordinate.coordinate.clone(),
            approved_digest,
            purged_by: purged_by.to_owned(),
            note: note.map(str::to_owned),
            deleted_rows,
            retained_rows: scope.retained_rows,
            acknowledged_live_events: scope.live_events_in_tombstoned_channels,
            deleted_legacy_read_state: scope.legacy_channel_scoped_read_state,
            exclusions: exclusion_notes(),
        })
    }
}

/// Advisory-lock class for project-purge coordinate locks.
///
/// Distinct from [`crate::deletion::SCHEMA_DESTRUCTION_LOCK_KEY`], which is a
/// single-argument key; the two-argument `pg_advisory_xact_lock(int, int)`
/// space is disjoint from the one-argument `bigint` space, so these can never
/// collide.
const PROJECT_PURGE_LOCK_CLASS: i32 = 0x6275_7a70;

/// Serialize purges of one `(community, coordinate)` against each other.
///
/// Transaction-scoped, so it releases on commit or rollback with no unlock
/// path to forget.
async fn lock_project_coordinate(
    conn: &mut PgConnection,
    community: CommunityId,
    coordinate: &ProjectCoordinate,
) -> Result<()> {
    sqlx::query("SELECT pg_advisory_xact_lock($1, $2)")
        .bind(PROJECT_PURGE_LOCK_CLASS)
        .bind(project_coordinate_lock_key(community, coordinate))
        .execute(conn)
        .await?;
    Ok(())
}

/// The `objid` half of one coordinate's advisory lock.
fn project_coordinate_lock_key(community: CommunityId, coordinate: &ProjectCoordinate) -> i32 {
    let mut hasher = Sha256::new();
    hasher.update(community.as_uuid().as_bytes());
    hasher.update([0u8]);
    hasher.update(coordinate.coordinate.as_bytes());
    let digest = hasher.finalize();
    i32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]])
}

/// Count the rows one unit currently selects.
async fn unit_count(
    conn: &mut PgConnection,
    unit: &PurgeUnit,
    community: Uuid,
    channels: &[Uuid],
    coordinate: &ProjectCoordinate,
) -> Result<i64> {
    let query = sqlx::query_scalar::<_, i64>(AssertSqlSafe(unit.count_sql()))
        .bind(community)
        .bind(channels.to_vec());
    let query = match unit.binds {
        UnitBinds::Channels => query,
        UnitBinds::Coordinate => query
            .bind(coordinate.owner.clone())
            .bind(coordinate.dtag.clone())
            .bind(coordinate.coordinate.clone()),
    };
    Ok(query.fetch_one(conn).await?)
}

/// Delete the rows one unit selects, returning the affected count.
async fn unit_delete(
    conn: &mut PgConnection,
    unit: &PurgeUnit,
    community: Uuid,
    channels: &[Uuid],
    coordinate: &ProjectCoordinate,
) -> Result<u64> {
    let query = sqlx::query(AssertSqlSafe(unit.delete_sql()))
        .bind(community)
        .bind(channels.to_vec());
    let query = match unit.binds {
        UnitBinds::Channels => query,
        UnitBinds::Coordinate => query
            .bind(coordinate.owner.clone())
            .bind(coordinate.dtag.clone())
            .bind(coordinate.coordinate.clone()),
    };
    Ok(query.execute(conn).await?.rows_affected())
}

/// Fail closed when either live catalog surface a project purge reaches
/// through drifts.
///
/// Mirrors [`crate::deletion::DeletionStore::validate_catalog`], but a project
/// purge has **two** ways to find a row, and pinning only one of them leaves
/// the other free to grow silently:
///
/// * a `project_ref`/`coordinate` column — [`PROJECT_COORDINATE_TABLES`];
/// * a `channel_id` column — [`PROJECT_CHANNEL_SCOPED_TABLES`], which is how
///   ten of the sixteen [`purge_units`] actually reach their rows.
///
/// Either kind of new table is a new place project rows can hide, so either
/// blocks the purge until the matching manifest is intentionally updated.
async fn validate_project_catalog(conn: &mut PgConnection) -> Result<()> {
    validate_catalog_surface(
        conn,
        "coordinate",
        &["project_ref", "coordinate"],
        PROJECT_COORDINATE_TABLES,
    )
    .await?;
    validate_catalog_surface(
        conn,
        "channel",
        &["channel_id"],
        PROJECT_CHANNEL_SCOPED_TABLES,
    )
    .await
}

/// Compare the live `public` tables carrying any of `columns` against
/// `expected`, erroring on any difference.
async fn validate_catalog_surface(
    conn: &mut PgConnection,
    surface: &str,
    columns: &[&str],
    expected: &[&str],
) -> Result<()> {
    let live: Vec<String> = sqlx::query_scalar(
        r#"
        SELECT DISTINCT c.relname
        FROM pg_class c
        JOIN pg_namespace n ON n.oid = c.relnamespace
        JOIN pg_attribute a ON a.attrelid = c.oid
        WHERE n.nspname = 'public'
          AND c.relkind IN ('r', 'p')
          AND NOT c.relispartition
          AND a.attname = ANY($1)
          AND NOT a.attisdropped
        ORDER BY c.relname
        "#,
    )
    .bind(columns.iter().map(|c| (*c).to_owned()).collect::<Vec<_>>())
    .fetch_all(&mut *conn)
    .await?;
    let expected: Vec<String> = expected.iter().map(|table| (*table).to_owned()).collect();
    if live != expected {
        let missing: Vec<&String> = expected.iter().filter(|t| !live.contains(t)).collect();
        let unknown: Vec<&String> = live.iter().filter(|t| !expected.contains(t)).collect();
        return Err(DbError::DeletionSafety(format!(
            "project purge {surface} catalog drift \
             (missing={missing:?}, unknown={unknown:?})"
        )));
    }
    Ok(())
}

/// Observe one project's purge scope on `conn`.
///
/// Every predicate leads with `community_id = $1`: the tenant fence is part of
/// the scope's definition, not a filter applied afterwards.
async fn compute_scope(
    conn: &mut PgConnection,
    community: CommunityId,
    coordinate: &ProjectCoordinate,
) -> Result<ProjectPurgeScope> {
    let community_uuid = *community.as_uuid();

    let channel_rows = sqlx::query(
        "SELECT id, deleted_at IS NOT NULL AS tombstoned FROM channels \
         WHERE community_id = $1 AND project_ref = $2 ORDER BY id",
    )
    .bind(community_uuid)
    .bind(&coordinate.coordinate)
    .fetch_all(&mut *conn)
    .await?;
    let mut tombstoned_channels = Vec::new();
    let mut live_channels = Vec::new();
    for row in channel_rows {
        let id: Uuid = row.try_get("id")?;
        if row.try_get::<bool, _>("tombstoned")? {
            tombstoned_channels.push(id);
        } else {
            live_channels.push(id);
        }
    }

    let project_head = census_row(
        sqlx::query(
            "SELECT COUNT(*) FILTER (WHERE deleted_at IS NOT NULL) AS tombstoned, \
                    COUNT(*) FILTER (WHERE deleted_at IS NULL) AS live \
               FROM events \
              WHERE community_id = $1 AND kind = $2 AND pubkey = $3 AND d_tag = $4",
        )
        .bind(community_uuid)
        .bind(KIND_PROJECT)
        .bind(&coordinate.owner)
        .bind(&coordinate.dtag)
        .fetch_one(&mut *conn)
        .await?,
    )?;
    let project_roster = census_row(
        sqlx::query(
            "SELECT COUNT(*) FILTER (WHERE deleted_at IS NOT NULL) AS tombstoned, \
                    COUNT(*) FILTER (WHERE deleted_at IS NULL) AS live \
               FROM events \
              WHERE community_id = $1 AND kind = $2 AND d_tag = $3",
        )
        .bind(community_uuid)
        .bind(KIND_PROJECT_MEMBERS)
        .bind(&coordinate.coordinate)
        .fetch_one(&mut *conn)
        .await?,
    )?;

    let project_acl_rows: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM project_acl \
         WHERE community_id = $1 AND owner = $2 AND dtag = $3",
    )
    .bind(community_uuid)
    .bind(&coordinate.owner)
    .bind(&coordinate.dtag)
    .fetch_one(&mut *conn)
    .await?;

    let mut purge_rows = BTreeMap::new();
    for unit in purge_units() {
        let count = unit_count(
            &mut *conn,
            &unit,
            community_uuid,
            &tombstoned_channels,
            coordinate,
        )
        .await?;
        purge_rows.insert(unit.table.to_owned(), count);
    }

    let live_events_in_tombstoned_channels: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM events \
         WHERE community_id = $1 AND channel_id = ANY($2) AND deleted_at IS NULL",
    )
    .bind(community_uuid)
    .bind(&tombstoned_channels)
    .fetch_one(&mut *conn)
    .await?;

    // The read-state exclusion's one carve-out. The predicate is verbatim
    // migration 0011's `trg_events_guard_nip_rs_hard_delete` WHEN clause, so
    // this counts exactly the rows the opt-in at `purge` unblocks, restricted
    // to the ones this purge can reach: legacy, channel-scoped ones.
    let legacy_channel_scoped_read_state: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM events \
         WHERE community_id = $1 AND kind = 30078 AND channel_id = ANY($2) \
           AND d_tag ~ '^read-state:[0-9a-f]{32}$'",
    )
    .bind(community_uuid)
    .bind(&tombstoned_channels)
    .fetch_one(&mut *conn)
    .await?;

    // `shell_session_acl` is reported as retained; an announce h-tagged into a
    // tombstoned channel would be destroyed and make that report false.
    let shell_announces_in_tombstoned_channels: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM events \
         WHERE community_id = $1 AND kind = 30623 AND channel_id = ANY($2)",
    )
    .bind(community_uuid)
    .bind(&tombstoned_channels)
    .fetch_one(&mut *conn)
    .await?;

    let mut retained_rows = BTreeMap::new();
    retained_rows.insert(
        "git_repo_names".to_owned(),
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM git_repo_names \
             WHERE community_id = $1 AND project_ref = $2",
        )
        .bind(community_uuid)
        .bind(&coordinate.coordinate)
        .fetch_one(&mut *conn)
        .await?,
    );
    retained_rows.insert(
        "shell_session_acl".to_owned(),
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM shell_session_acl \
             WHERE community_id = $1 AND coordinate = $2",
        )
        .bind(community_uuid)
        .bind(&coordinate.coordinate)
        .fetch_one(&mut *conn)
        .await?,
    );
    retained_rows.insert(
        "parameterized_event_watermarks".to_owned(),
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM parameterized_event_watermarks \
             WHERE community_id = $1 \
               AND ((kind = 30621 AND pubkey = $2 AND d_tag = $3) \
                 OR (kind = 39010 AND d_tag = $4))",
        )
        .bind(community_uuid)
        .bind(&coordinate.owner)
        .bind(&coordinate.dtag)
        .bind(&coordinate.coordinate)
        .fetch_one(&mut *conn)
        .await?,
    );
    // Live coordinate events the purge refuses to touch: the relay-signed
    // kind:39010 roster is never tombstoned by a project cascade, and neither
    // are kind:9010/9011 membership ops that `a`-tag the coordinate.
    retained_rows.insert(
        "events_live_on_coordinate".to_owned(),
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM events \
             WHERE community_id = $1 AND deleted_at IS NULL \
               AND (channel_id IS NULL OR NOT (channel_id = ANY($4))) \
               AND ((kind = 39010 AND d_tag = $2) \
                 OR (kind IN (9010, 9011) AND tags @> $3))",
        )
        .bind(community_uuid)
        .bind(&coordinate.coordinate)
        .bind(serde_json::json!([["a", coordinate.coordinate]]))
        .bind(&tombstoned_channels)
        .fetch_one(&mut *conn)
        .await?,
    );

    let blockers = compute_blockers(
        &live_channels,
        &project_head,
        project_acl_rows,
        shell_announces_in_tombstoned_channels,
        &coordinate.coordinate,
    );

    Ok(ProjectPurgeScope {
        community_id: community_uuid,
        coordinate: coordinate.coordinate.clone(),
        tombstoned_channels,
        live_channels,
        project_head,
        project_roster,
        project_acl_rows,
        purge_rows,
        retained_rows,
        live_events_in_tombstoned_channels,
        legacy_channel_scoped_read_state,
        shell_announces_in_tombstoned_channels,
        blockers,
    })
}

/// Why a scope is not purgeable, in operator-readable form.
///
/// Pure so the refusal rules are unit-testable without a database. Almost
/// every blocker is a *liveness* signal: something the normal, authorized
/// deletion path has not yet tombstoned. The exception is
/// `shell_announces`, which is a *consistency* signal — see below.
///
/// [`ProjectPurgeScope::project_roster`] is deliberately **not** an input. A
/// live relay-signed kind:39010 roster is satellite state a project cascade
/// never tombstones, so it is retained and reported
/// (`retained_rows["events_live_on_coordinate"]`) rather than blocking. The
/// Postgres test `a_live_roster_row_is_retained_while_a_tombstoned_one_is_purged`
/// pins that behaviour end to end.
fn compute_blockers(
    live_channels: &[Uuid],
    project_head: &TombstoneCensus,
    project_acl_rows: i64,
    shell_announces: i64,
    coordinate: &str,
) -> Vec<String> {
    let mut blockers = Vec::new();
    if project_head.tombstoned == 0 && project_head.live == 0 {
        blockers.push(format!("no kind:30621 head exists for {coordinate}"));
    }
    if project_head.live > 0 {
        blockers.push(format!(
            "{} live kind:30621 head row(s) — delete the project first",
            project_head.live
        ));
    }
    if !live_channels.is_empty() {
        blockers.push(format!(
            "{} channel(s) bound to {coordinate} are still live: {}",
            live_channels.len(),
            live_channels
                .iter()
                .map(Uuid::to_string)
                .collect::<Vec<_>>()
                .join(",")
        ));
    }
    if project_acl_rows > 0 {
        blockers.push(format!(
            "{project_acl_rows} project_acl row(s) survive — the authorized \
             tombstone drops them, so the project is still live"
        ));
    }
    if shell_announces > 0 {
        blockers.push(format!(
            "{shell_announces} kind:30623 shell-session announce(s) are h-tagged \
             into this project's tombstoned channels — purging would destroy them \
             while `shell_session_acl` is reported as retained, leaving the \
             projection pointing at nothing. Have the session owner delete or \
             re-announce them first"
        ));
    }
    blockers
}

fn census_row(row: sqlx::postgres::PgRow) -> Result<TombstoneCensus> {
    Ok(TombstoneCensus {
        tombstoned: row.try_get("tombstoned")?,
        live: row.try_get("live")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coordinate_parsing_normalizes_and_rejects_malformed_input() {
        let owner = "AB".repeat(32);
        let parsed = ProjectCoordinate::parse(&format!("  30621:{owner}:throwaway  "))
            .expect("well-formed coordinate");
        assert_eq!(
            parsed.coordinate,
            format!("30621:{}:throwaway", "ab".repeat(32))
        );
        assert_eq!(hex::encode(&parsed.owner), "ab".repeat(32));
        assert_eq!(parsed.owner.len(), 32);
        assert_eq!(parsed.dtag, "throwaway");

        for bad in [
            "30621:short:proj",
            "30178:aabb:proj",
            &format!("30621:{}:", "ab".repeat(32)),
            &format!("30621:{}", "ab".repeat(32)),
            "",
        ] {
            assert!(
                ProjectCoordinate::parse(bad).is_err(),
                "expected rejection for {bad:?}"
            );
        }
    }

    #[test]
    fn every_purge_unit_binds_exactly_the_parameters_it_references() {
        for unit in purge_units() {
            let sql = unit.where_clause();
            let highest = (1..=5)
                .filter(|index| sql.contains(&format!("${index}")))
                .max()
                .expect("unit references at least $1");
            let expected = match unit.binds {
                UnitBinds::Channels => 2,
                UnitBinds::Coordinate => 5,
            };
            assert_eq!(
                highest, expected,
                "{} declares {:?} binds but references up to ${highest}",
                unit.table, unit.binds
            );
            for index in 1..=expected {
                assert!(
                    sql.contains(&format!("${index}")),
                    "{} never references ${index}",
                    unit.table
                );
            }
        }
    }

    #[test]
    fn purge_order_is_child_before_parent_and_ends_at_channels() {
        let order: Vec<&str> = purge_units().iter().map(|unit| unit.table).collect();
        let position = |table: &str| {
            order
                .iter()
                .position(|entry| *entry == table)
                .unwrap_or_else(|| panic!("{table} missing from purge order"))
        };
        assert_eq!(*order.last().expect("non-empty order"), "channels");
        // FK children first.
        assert!(position("workflow_approvals") < position("workflow_runs"));
        assert!(position("scheduled_workflow_fires") < position("workflow_runs"));
        assert!(position("workflow_runs") < position("workflows"));
        assert!(position("workflows") < position("channels"));
        assert!(position("channel_members") < position("channels"));
        assert!(position("thread_metadata") < position("channels"));
        // Event-derived rows must resolve against `events` before it is gone.
        for derived in [
            "event_mentions",
            "reactions",
            "push_match_queue",
            "push_wake_outbox",
        ] {
            assert!(position(derived) < position("events"));
        }
    }

    #[test]
    fn no_purge_unit_touches_an_excluded_or_control_plane_table() {
        let purged: Vec<&str> = purge_units().iter().map(|unit| unit.table).collect();
        for excluded in [
            "git_repo_names",
            "shell_session_acl",
            "shell_session_acl_members",
            "parameterized_event_watermarks",
            "delivery_log",
            "communities",
            "project_acl",
            "project_acl_members",
        ] {
            assert!(
                !purged.contains(&excluded),
                "{excluded} must never be purged by a project purge"
            );
        }
        for control in crate::deletion::CONTROL_PLANE_TABLES {
            assert!(!purged.contains(control));
        }
    }

    #[test]
    fn scope_event_predicate_only_admits_tombstoned_coordinate_rows() {
        // The channel arm carries no `deleted_at` test by design (tier 2), but
        // both coordinate arms must, or a live head/roster would be purged.
        let head_arm = "e.kind = 30621 AND e.pubkey = $3 AND e.d_tag = $4 \
                        AND e.deleted_at IS NOT NULL";
        let roster_arm = "e.kind = 39010 AND e.d_tag = $5 AND e.deleted_at IS NOT NULL";
        assert!(SCOPE_EVENT_PREDICATE.contains(head_arm));
        assert!(SCOPE_EVENT_PREDICATE.contains(roster_arm));
        assert_eq!(
            SCOPE_EVENT_PREDICATE
                .matches("deleted_at IS NOT NULL")
                .count(),
            2
        );
        // Tenant fencing is applied by every unit, never by this fragment.
        assert!(!SCOPE_EVENT_PREDICATE.contains("community_id"));
    }

    fn census_of(tombstoned: i64, live: i64) -> TombstoneCensus {
        TombstoneCensus { tombstoned, live }
    }

    #[test]
    fn live_project_head_blocks_the_purge() {
        let blockers = compute_blockers(&[], &census_of(0, 1), 0, 0, "30621:aa:proj");
        assert_eq!(blockers.len(), 1);
        assert!(blockers[0].contains("live kind:30621 head"));
    }

    #[test]
    fn live_channel_blocks_the_purge() {
        let live = Uuid::nil();
        let blockers = compute_blockers(&[live], &census_of(1, 0), 0, 0, "30621:aa:proj");
        assert_eq!(blockers.len(), 1);
        assert!(blockers[0].contains("still live"));
        assert!(blockers[0].contains(&live.to_string()));
    }

    #[test]
    fn surviving_project_acl_row_blocks_the_purge() {
        let blockers = compute_blockers(&[], &census_of(1, 0), 2, 0, "30621:aa:proj");
        assert_eq!(blockers.len(), 1);
        assert!(blockers[0].contains("project_acl row(s) survive"));
    }

    #[test]
    fn unknown_project_blocks_the_purge() {
        let blockers = compute_blockers(&[], &census_of(0, 0), 0, 0, "30621:aa:proj");
        assert_eq!(blockers.len(), 1);
        assert!(blockers[0].contains("no kind:30621 head"));
    }

    /// `shell_session_acl` is reported to the operator as retained. An
    /// announce h-tagged into a tombstoned channel would be destroyed anyway,
    /// so the purge must refuse rather than make that report false.
    #[test]
    fn a_shell_announce_inside_a_tombstoned_channel_blocks_the_purge() {
        let blockers = compute_blockers(&[], &census_of(1, 0), 0, 2, "30621:aa:proj");
        assert_eq!(blockers.len(), 1);
        assert!(blockers[0].contains("kind:30623"), "{blockers:?}");
        assert!(blockers[0].contains("shell_session_acl"), "{blockers:?}");
    }

    #[test]
    fn fully_tombstoned_project_has_no_blockers() {
        let blockers = compute_blockers(&[], &census_of(1, 0), 0, 0, "30621:aa:proj");
        assert!(blockers.is_empty(), "{blockers:?}");
    }

    #[test]
    fn digest_is_stable_and_changes_with_scope() {
        let scope = ProjectPurgeScope {
            community_id: Uuid::nil(),
            coordinate: "30621:aa:proj".to_owned(),
            tombstoned_channels: vec![],
            live_channels: vec![],
            project_head: census_of(1, 0),
            project_roster: census_of(0, 0),
            project_acl_rows: 0,
            purge_rows: BTreeMap::from([("events".to_owned(), 3)]),
            retained_rows: BTreeMap::from([("git_repo_names".to_owned(), 1)]),
            live_events_in_tombstoned_channels: 0,
            legacy_channel_scoped_read_state: 0,
            shell_announces_in_tombstoned_channels: 0,
            blockers: vec![],
        };
        let first = scope.digest().expect("digest");
        assert_eq!(first, scope.digest().expect("digest"));
        assert_eq!(first.len(), 64);

        let mut widened = scope.clone();
        widened.purge_rows.insert("events".to_owned(), 4);
        assert_ne!(first, widened.digest().expect("digest"));
    }

    #[test]
    fn purge_never_reaches_a_table_outside_the_pinned_coordinate_catalog() {
        // `channels` is the only coordinate-carrying table this purge deletes
        // from; the other three are pinned so a schema change fails the purge
        // closed rather than leaving project rows behind.
        let purged: Vec<&str> = purge_units().iter().map(|unit| unit.table).collect();
        let coordinate_tables_purged: Vec<&&str> = PROJECT_COORDINATE_TABLES
            .iter()
            .filter(|table| purged.contains(table))
            .collect();
        assert_eq!(coordinate_tables_purged, vec![&"channels"]);
    }

    #[test]
    fn every_exclusion_has_a_reason() {
        assert!(!PROJECT_PURGE_EXCLUSIONS.is_empty());
        for (table, reason) in PROJECT_PURGE_EXCLUSIONS {
            assert!(!table.is_empty());
            assert!(reason.len() > 20, "{table} needs a real reason");
        }
    }

    /// The catalog manifest is compared with `==` against an `ORDER BY relname`
    /// query, so it has to be sorted and duplicate-free or the purge fails
    /// closed on a correct schema.
    #[test]
    fn the_pinned_coordinate_catalog_is_sorted_and_unique() {
        let mut sorted = PROJECT_COORDINATE_TABLES.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted, PROJECT_COORDINATE_TABLES.to_vec());
    }

    /// Every unit that can reach a coordinate row must carry the coordinate
    /// binds; a `Channels`-bound unit can only ever see the frozen channel
    /// list, which is the tenant- and project-fenced anchor set.
    #[test]
    fn every_unit_is_tenant_fenced_on_its_own_predicate() {
        for unit in purge_units() {
            let sql = unit.where_clause();
            assert!(
                sql.contains("community_id = $1"),
                "{} does not fence on the community",
                unit.table
            );
            match unit.binds {
                UnitBinds::Channels => assert!(
                    sql.contains("$2"),
                    "{} must scope to the frozen channel list",
                    unit.table
                ),
                // Checked on the *raw* predicate: `where_clause()` has already
                // expanded the placeholder into the shared fragment.
                UnitBinds::Coordinate => assert!(
                    unit.predicate.contains("SCOPE_EVENT_PREDICATE")
                        && sql.contains(SCOPE_EVENT_PREDICATE),
                    "{} must scope through the shared event predicate",
                    unit.table
                ),
            }
        }
    }

    /// Every whole-table exclusion must actually be absent from the purge
    /// order, so the reported reasons cannot drift from behaviour.
    ///
    /// A parenthesised entry names a *row class* inside a table the purge does
    /// touch (`events (kind:30078 …)`), so it is exempt from the table check —
    /// [`scope_event_predicate_only_admits_tombstoned_coordinate_rows`] and the
    /// Postgres read-state test cover those instead.
    #[test]
    fn no_whole_table_exclusion_is_also_purged() {
        let purged: Vec<&str> = purge_units().iter().map(|unit| unit.table).collect();
        for (table, _) in PROJECT_PURGE_EXCLUSIONS {
            if table.contains('(') {
                continue;
            }
            // Entries name a single table or a `parent / child` pair.
            for name in table.split('/').map(str::trim) {
                assert!(
                    !name.is_empty() && !name.contains(' '),
                    "exclusion entry {table:?} is neither a table nor a row class"
                );
                assert!(
                    !purged.contains(&name),
                    "{name} is listed as excluded but appears in the purge order"
                );
            }
        }
    }

    /// The read-state exclusion must stay a *row class* note on `events`, not
    /// silently widen into a claim that the whole table is retained.
    #[test]
    fn the_read_state_exclusion_is_scoped_to_a_row_class() {
        let (table, reason) = PROJECT_PURGE_EXCLUSIONS
            .iter()
            .find(|(table, _)| table.contains("read-state"))
            .expect("read-state rows must be named as retained");
        assert!(table.starts_with("events ("), "{table}");
        assert!(reason.contains("30078") || table.contains("30078"));
    }

    /// `purge` opts into `buzz.nip_rs_hard_delete` and the Postgres test
    /// `read_state_rows_survive_and_the_nip_rs_delete_guard_is_opted_into`
    /// proves a legacy channel-scoped read marker IS destroyed. The
    /// operator-facing note is printed verbatim on every inventory and every
    /// receipt, so it must say that — not "never in scope".
    #[test]
    fn the_read_state_exclusion_admits_that_legacy_channel_scoped_rows_are_deleted() {
        let (table, reason) = PROJECT_PURGE_EXCLUSIONS
            .iter()
            .find(|(table, _)| table.contains("read-state"))
            .expect("read-state rows must be named");
        // The promise is narrowed to the global rows, in the title itself.
        assert!(table.contains("global rows only"), "{table}");
        // And the note states the deletion, names the mechanism that enables
        // it, and points at the number that reports it.
        for phrase in [
            "channel_id NULL",
            "NOT a whole-kind exclusion",
            "hard-deleted",
            "buzz.nip_rs_hard_delete",
            "legacy_channel_scoped_read_state",
        ] {
            assert!(
                reason.contains(phrase),
                "read-state exclusion must state {phrase:?}: {reason}"
            );
        }
        // The old wording said "they are never in scope" of the whole kind.
        // Every retention promise here must carry the narrowing qualifier.
        assert!(
            reason.contains("global rows are never in scope"),
            "the read-state note must scope its retention promise to the \
             global rows, not the whole kind: {reason}"
        );
    }

    /// The retained `shell_session_acl` claim is now enforced by a blocker, so
    /// the note must point at it instead of asserting liveness on faith.
    #[test]
    fn the_shell_session_acl_exclusion_names_the_blocker_that_enforces_it() {
        let (_, reason) = PROJECT_PURGE_EXCLUSIONS
            .iter()
            .find(|(table, _)| *table == "shell_session_acl")
            .expect("shell_session_acl must be named as retained");
        assert!(
            reason.contains("shell_announces_in_tombstoned_channels"),
            "{reason}"
        );
    }

    /// Ten of the sixteen units reach rows through `channel_id`, so that table
    /// class needs the same pin-and-compare treatment as the coordinate one.
    #[test]
    fn the_pinned_channel_scoped_catalog_is_sorted_unique_and_fully_purged() {
        let mut sorted = PROJECT_CHANNEL_SCOPED_TABLES.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted, PROJECT_CHANNEL_SCOPED_TABLES.to_vec());

        // A pinned `channel_id` table that no unit purges would be left behind
        // with a dangling channel_id once `channels` is deleted last.
        let purged: Vec<&str> = purge_units().iter().map(|unit| unit.table).collect();
        for table in PROJECT_CHANNEL_SCOPED_TABLES {
            assert!(
                purged.contains(table),
                "{table} carries channel_id but no purge unit reaches it"
            );
        }
    }

    /// The `channel_id`-reachable units must all be covered by the pin, or the
    /// guard is validating a surface narrower than the one the purge deletes.
    #[test]
    fn every_channel_bound_unit_is_in_the_pinned_channel_scoped_catalog() {
        for unit in purge_units() {
            if unit.binds != UnitBinds::Channels {
                continue;
            }
            // Workflow children reach their rows through workflow_id, not a
            // channel_id column of their own.
            if unit.predicate.contains("workflow_id IN") {
                continue;
            }
            assert!(
                PROJECT_CHANNEL_SCOPED_TABLES.contains(&unit.table) || unit.table == "channels",
                "{} is deleted by channel_id but is not pinned",
                unit.table
            );
        }
    }

    /// Whole-community deletion drops the tenant; a project purge must leave
    /// the community row (and its host reservation) untouched.
    #[test]
    fn the_community_tombstone_is_named_as_retained() {
        assert!(PROJECT_PURGE_EXCLUSIONS
            .iter()
            .any(|(table, reason)| *table == "communities" && reason.contains("tombstone")));
    }

    /// Repos must survive so a purge never frees a community-unique repo name.
    #[test]
    fn repositories_are_named_as_retained_and_never_purged() {
        assert!(PROJECT_PURGE_EXCLUSIONS
            .iter()
            .any(|(table, _)| *table == "git_repo_names"));
        assert!(!purge_units()
            .iter()
            .any(|unit| unit.table == "git_repo_names"));
    }
}

#[cfg(test)]
mod postgres_tests {
    use super::*;
    use crate::{Db, DbConfig};

    /// Two-part fixture: a target project and a full set of near misses that
    /// must survive every purge.
    struct Fixture {
        db: Db,
        community: CommunityId,
        other_community: CommunityId,
        owner: Vec<u8>,
        /// Tombstoned channel of the target project.
        target_channel: Uuid,
        /// Tombstoned channel of a *different* project in the same community.
        sibling_channel: Uuid,
        /// Tombstoned channel of the *same* coordinate in another community.
        foreign_channel: Uuid,
        /// Live channel with no project binding.
        unrelated_channel: Uuid,
        coordinate: ProjectCoordinate,
    }

    async fn connect() -> Db {
        let database_url = std::env::var("BUZZ_TEST_DATABASE_URL")
            .or_else(|_| std::env::var("DATABASE_URL"))
            .unwrap_or_else(|_| "postgres://buzz:buzz_dev@127.0.0.1:5432/buzz".to_string());
        let db = Db::new(&DbConfig {
            database_url,
            max_connections: 5,
            min_connections: 0,
            ..DbConfig::default()
        })
        .await
        .expect("connect project purge test DB");
        db.migrate().await.expect("migrate project purge test DB");
        db
    }

    async fn new_community(db: &Db, label: &str) -> CommunityId {
        db.ensure_configured_community(&format!(
            "project-purge-{label}-{}.example",
            Uuid::new_v4().simple()
        ))
        .await
        .expect("create community")
        .id
    }

    async fn insert_channel(
        db: &Db,
        community: CommunityId,
        project_ref: Option<&str>,
        tombstoned: bool,
    ) -> Uuid {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO channels (id, community_id, name, created_by, project_ref, deleted_at) \
             VALUES ($1, $2, $3, $4, $5, CASE WHEN $6 THEN now() END)",
        )
        .bind(id)
        .bind(community.as_uuid())
        .bind(format!("c-{}", id.simple()))
        .bind(vec![7u8; 32])
        .bind(project_ref)
        .bind(tombstoned)
        .execute(&db.pool)
        .await
        .expect("insert channel");
        id
    }

    #[allow(clippy::too_many_arguments)]
    async fn insert_event(
        db: &Db,
        community: CommunityId,
        kind: i32,
        pubkey: &[u8],
        channel: Option<Uuid>,
        d_tag: Option<&str>,
        tombstoned: bool,
    ) -> Vec<u8> {
        let id = Uuid::new_v4().as_bytes().repeat(2);
        sqlx::query(
            "INSERT INTO events \
             (community_id, id, pubkey, created_at, kind, tags, content, sig, channel_id, d_tag, deleted_at) \
             VALUES ($1, $2, $3, now(), $4, '[]'::jsonb, '', $5, $6, $7, CASE WHEN $8 THEN now() END)",
        )
        .bind(community.as_uuid())
        .bind(&id)
        .bind(pubkey.to_vec())
        .bind(kind)
        .bind(vec![9u8; 64])
        .bind(channel)
        .bind(d_tag)
        .bind(tombstoned)
        .execute(&db.pool)
        .await
        .expect("insert event");
        id
    }

    /// A soft-deleted project whose channel still holds messages, surrounded
    /// by rows that must never be selected.
    async fn fixture() -> Fixture {
        let db = connect().await;
        let community = new_community(&db, "target").await;
        let other_community = new_community(&db, "foreign").await;
        let owner = vec![0xabu8; 32];
        let owner_hex = hex::encode(&owner);
        let coordinate =
            ProjectCoordinate::parse(&format!("30621:{owner_hex}:target")).expect("coordinate");
        let sibling = format!("30621:{owner_hex}:sibling");

        let target_channel =
            insert_channel(&db, community, Some(&coordinate.coordinate), true).await;
        let sibling_channel = insert_channel(&db, community, Some(&sibling), true).await;
        let foreign_channel =
            insert_channel(&db, other_community, Some(&coordinate.coordinate), true).await;
        let unrelated_channel = insert_channel(&db, community, None, false).await;

        // Tombstoned project head + a message inside the tombstoned channel
        // that is itself still `deleted_at IS NULL` (channel deletion does not
        // tombstone messages).
        insert_event(&db, community, 30621, &owner, None, Some("target"), true).await;
        insert_event(
            &db,
            community,
            40001,
            &owner,
            Some(target_channel),
            None,
            false,
        )
        .await;
        // Near misses.
        insert_event(
            &db,
            community,
            40001,
            &owner,
            Some(sibling_channel),
            None,
            false,
        )
        .await;
        insert_event(
            &db,
            other_community,
            40001,
            &owner,
            Some(foreign_channel),
            None,
            false,
        )
        .await;
        insert_event(
            &db,
            other_community,
            30621,
            &owner,
            None,
            Some("target"),
            true,
        )
        .await;
        insert_event(
            &db,
            community,
            40001,
            &owner,
            Some(unrelated_channel),
            None,
            false,
        )
        .await;
        // Repo bound to the target project: detached by the cascade, never
        // purged, so its name stays reserved.
        sqlx::query(
            "INSERT INTO git_repo_names (community_id, repo_id, owner_pubkey, project_ref) \
             VALUES ($1, $2, $3, $4)",
        )
        .bind(community.as_uuid())
        .bind(format!("repo-{}", Uuid::new_v4().simple()))
        .bind(&owner_hex)
        .bind(&coordinate.coordinate)
        .execute(&db.pool)
        .await
        .expect("insert repo name");

        Fixture {
            db,
            community,
            other_community,
            owner,
            target_channel,
            sibling_channel,
            foreign_channel,
            unrelated_channel,
            coordinate,
        }
    }

    async fn channel_exists(db: &Db, community: CommunityId, channel: Uuid) -> bool {
        sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM channels WHERE community_id = $1 AND id = $2)",
        )
        .bind(community.as_uuid())
        .bind(channel)
        .fetch_one(&db.pool)
        .await
        .expect("channel probe")
    }

    async fn event_count(db: &Db, community: CommunityId, channel: Uuid) -> i64 {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM events WHERE community_id = $1 AND channel_id = $2",
        )
        .bind(community.as_uuid())
        .bind(channel)
        .fetch_one(&db.pool)
        .await
        .expect("event probe")
    }

    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn scope_excludes_other_communities_other_projects_live_channels_and_repos() {
        let f = fixture().await;
        let store = f.db.project_purge_store();
        let inventory = store
            .inventory(f.community, &f.coordinate)
            .await
            .expect("inventory");

        assert_eq!(inventory.scope.blockers, Vec::<String>::new());
        assert_eq!(inventory.scope.tombstoned_channels, vec![f.target_channel]);
        assert!(inventory.scope.live_channels.is_empty());
        // One message in the target channel plus the tombstoned 30621 head.
        assert_eq!(inventory.scope.purge_rows["events"], 2);
        assert_eq!(inventory.scope.purge_rows["channels"], 1);
        assert_eq!(inventory.scope.live_events_in_tombstoned_channels, 1);
        // Repos are reported as retained, never purged.
        assert_eq!(inventory.scope.retained_rows["git_repo_names"], 1);
        assert!(inventory
            .exclusions
            .iter()
            .any(|note| note.table == "git_repo_names"));

        let receipt = store
            .purge(
                f.community,
                &f.coordinate,
                &inventory.digest,
                "lane-5-operator",
                Some("throwaway test project"),
                true,
            )
            .await
            .expect("purge");
        assert_eq!(receipt.deleted_rows["events"], 2);
        assert_eq!(receipt.deleted_rows["channels"], 1);

        // Target gone.
        assert!(!channel_exists(&f.db, f.community, f.target_channel).await);
        assert_eq!(event_count(&f.db, f.community, f.target_channel).await, 0);
        // Every near miss survives.
        assert!(channel_exists(&f.db, f.community, f.sibling_channel).await);
        assert_eq!(event_count(&f.db, f.community, f.sibling_channel).await, 1);
        assert!(channel_exists(&f.db, f.other_community, f.foreign_channel).await);
        assert_eq!(
            event_count(&f.db, f.other_community, f.foreign_channel).await,
            1
        );
        assert!(channel_exists(&f.db, f.community, f.unrelated_channel).await);
        assert_eq!(
            event_count(&f.db, f.community, f.unrelated_channel).await,
            1
        );
        // The other community's identically-coordinated 30621 head survives.
        let foreign_heads: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM events WHERE community_id = $1 AND kind = 30621 AND d_tag = $2",
        )
        .bind(f.other_community.as_uuid())
        .bind("target")
        .fetch_one(&f.db.pool)
        .await
        .expect("foreign head probe");
        assert_eq!(foreign_heads, 1);
        // Repo name reservation survives, detached.
        let repos: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM git_repo_names WHERE community_id = $1 AND project_ref = $2",
        )
        .bind(f.community.as_uuid())
        .bind(&f.coordinate.coordinate)
        .fetch_one(&f.db.pool)
        .await
        .expect("repo probe");
        assert_eq!(repos, 1);
    }

    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn a_live_channel_blocks_the_purge_and_nothing_is_deleted() {
        let f = fixture().await;
        let live = insert_channel(&f.db, f.community, Some(&f.coordinate.coordinate), false).await;
        let store = f.db.project_purge_store();

        let inventory = store
            .inventory(f.community, &f.coordinate)
            .await
            .expect("inventory");
        assert_eq!(inventory.scope.live_channels, vec![live]);
        assert!(inventory
            .scope
            .blockers
            .iter()
            .any(|blocker| blocker.contains("still live")));

        let error = store
            .purge(
                f.community,
                &f.coordinate,
                &inventory.digest,
                "lane-5-operator",
                None,
                true,
            )
            .await
            .expect_err("purge must refuse while a channel is live");
        assert!(error.to_string().contains("not purgeable"), "{error}");
        assert!(channel_exists(&f.db, f.community, f.target_channel).await);
        assert_eq!(event_count(&f.db, f.community, f.target_channel).await, 1);
    }

    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn a_live_project_head_blocks_the_purge() {
        let f = fixture().await;
        insert_event(
            &f.db,
            f.community,
            30621,
            &f.owner,
            None,
            Some("target"),
            false,
        )
        .await;
        let store = f.db.project_purge_store();
        let inventory = store
            .inventory(f.community, &f.coordinate)
            .await
            .expect("inventory");
        assert_eq!(inventory.scope.project_head.live, 1);
        assert_eq!(inventory.scope.project_head.tombstoned, 1);

        let error = store
            .purge(
                f.community,
                &f.coordinate,
                &inventory.digest,
                "lane-5-operator",
                None,
                true,
            )
            .await
            .expect_err("purge must refuse while the head is live");
        assert!(
            error.to_string().contains("live kind:30621 head"),
            "{error}"
        );
        assert!(channel_exists(&f.db, f.community, f.target_channel).await);
    }

    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn a_surviving_project_acl_row_blocks_the_purge() {
        let f = fixture().await;
        sqlx::query(
            "INSERT INTO project_acl \
             (community_id, owner, dtag, coordinate, visibility, head_created_at) \
             VALUES ($1, $2, $3, $4, 'private', 1)",
        )
        .bind(f.community.as_uuid())
        .bind(&f.owner)
        .bind(&f.coordinate.dtag)
        .bind(&f.coordinate.coordinate)
        .execute(&f.db.pool)
        .await
        .expect("insert project_acl");

        let store = f.db.project_purge_store();
        let inventory = store
            .inventory(f.community, &f.coordinate)
            .await
            .expect("inventory");
        assert_eq!(inventory.scope.project_acl_rows, 1);
        let error = store
            .purge(
                f.community,
                &f.coordinate,
                &inventory.digest,
                "lane-5-operator",
                None,
                true,
            )
            .await
            .expect_err("purge must refuse while the ACL projection survives");
        assert!(
            error.to_string().contains("project_acl row(s) survive"),
            "{error}"
        );
    }

    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn a_stale_approved_digest_refuses_the_purge() {
        let f = fixture().await;
        let store = f.db.project_purge_store();
        let inventory = store
            .inventory(f.community, &f.coordinate)
            .await
            .expect("inventory");

        // Scope widens after approval.
        insert_event(
            &f.db,
            f.community,
            40001,
            &f.owner,
            Some(f.target_channel),
            None,
            false,
        )
        .await;

        let error = store
            .purge(
                f.community,
                &f.coordinate,
                &inventory.digest,
                "lane-5-operator",
                None,
                true,
            )
            .await
            .expect_err("stale digest must refuse");
        assert!(
            error
                .to_string()
                .contains("inventory changed since approval"),
            "{error}"
        );
        assert_eq!(event_count(&f.db, f.community, f.target_channel).await, 2);

        // Re-inventorying and approving the new digest succeeds.
        let refreshed = store
            .inventory(f.community, &f.coordinate)
            .await
            .expect("re-inventory");
        assert_ne!(refreshed.digest, inventory.digest);
        store
            .purge(
                f.community,
                &f.coordinate,
                &refreshed.digest,
                "lane-5-operator",
                None,
                true,
            )
            .await
            .expect("purge with the refreshed digest");
        assert!(!channel_exists(&f.db, f.community, f.target_channel).await);
    }

    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn an_empty_operator_identity_refuses_the_purge() {
        let f = fixture().await;
        let store = f.db.project_purge_store();
        let inventory = store
            .inventory(f.community, &f.coordinate)
            .await
            .expect("inventory");
        let error = store
            .purge(
                f.community,
                &f.coordinate,
                &inventory.digest,
                "  ",
                None,
                true,
            )
            .await
            .expect_err("empty operator identity must refuse");
        assert!(error.to_string().contains("operator identity"), "{error}");
        assert!(channel_exists(&f.db, f.community, f.target_channel).await);
    }

    /// The one class of row the purge deletes without a tombstone of its own
    /// is gated on its own opt-in. `--confirm` alone must not reach it.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn live_messages_in_a_tombstoned_channel_need_an_explicit_acknowledgement() {
        let f = fixture().await;
        let store = f.db.project_purge_store();
        let inventory = store
            .inventory(f.community, &f.coordinate)
            .await
            .expect("inventory");
        assert_eq!(inventory.scope.live_events_in_tombstoned_channels, 1);
        assert!(inventory.scope.blockers.is_empty());

        let error = store
            .purge(
                f.community,
                &f.coordinate,
                &inventory.digest,
                "lane-5-operator",
                None,
                false,
            )
            .await
            .expect_err("must refuse without the acknowledgement");
        assert!(
            error.to_string().contains("not themselves soft-deleted"),
            "{error}"
        );
        // Nothing was deleted.
        assert!(channel_exists(&f.db, f.community, f.target_channel).await);
        assert_eq!(event_count(&f.db, f.community, f.target_channel).await, 1);

        let receipt = store
            .purge(
                f.community,
                &f.coordinate,
                &inventory.digest,
                "lane-5-operator",
                None,
                true,
            )
            .await
            .expect("purge with the acknowledgement");
        assert_eq!(receipt.acknowledged_live_events, 1);
        assert!(!channel_exists(&f.db, f.community, f.target_channel).await);
    }

    /// Anchor rows that are NOT soft-deleted are never selected, even when the
    /// purge is otherwise unblocked. The relay-signed kind:39010 roster is the
    /// case that actually occurs: a project cascade never tombstones it.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn a_live_roster_row_is_retained_while_a_tombstoned_one_is_purged() {
        let f = fixture().await;
        // Two rows on the same coordinate: one tombstoned, one live.
        insert_event(
            &f.db,
            f.community,
            39010,
            &f.owner,
            None,
            Some(&f.coordinate.coordinate),
            true,
        )
        .await;
        let live_roster = insert_event(
            &f.db,
            f.community,
            39010,
            &f.owner,
            None,
            Some(&f.coordinate.coordinate),
            false,
        )
        .await;

        let store = f.db.project_purge_store();
        let inventory = store
            .inventory(f.community, &f.coordinate)
            .await
            .expect("inventory");
        assert_eq!(inventory.scope.project_roster.tombstoned, 1);
        assert_eq!(inventory.scope.project_roster.live, 1);
        // A live roster is reported as retained, and does not block.
        assert!(
            inventory.scope.blockers.is_empty(),
            "{:?}",
            inventory.scope.blockers
        );
        assert_eq!(
            inventory.scope.retained_rows["events_live_on_coordinate"],
            1
        );
        // Head + message + tombstoned roster; the live roster is not counted.
        assert_eq!(inventory.scope.purge_rows["events"], 3);

        store
            .purge(
                f.community,
                &f.coordinate,
                &inventory.digest,
                "lane-5-operator",
                None,
                true,
            )
            .await
            .expect("purge");

        let survivors: Vec<Vec<u8>> = sqlx::query_scalar(
            "SELECT id FROM events WHERE community_id = $1 AND kind = 39010 AND d_tag = $2",
        )
        .bind(f.community.as_uuid())
        .bind(&f.coordinate.coordinate)
        .fetch_all(&f.db.pool)
        .await
        .expect("roster probe");
        assert_eq!(
            survivors,
            vec![live_roster],
            "only the live roster survives"
        );
    }

    /// A *global* per-user NIP-RS read marker is encrypted and never
    /// attributable to a project, so it survives. A *legacy* channel-scoped
    /// one inside a tombstoned channel does not: it is hard-deleted with the
    /// channel, which is only possible because `purge` opts into
    /// `buzz.nip_rs_hard_delete` transaction-locally. Both halves are asserted
    /// here, and the exclusion note must describe both — see
    /// `the_read_state_exclusion_admits_that_legacy_channel_scoped_rows_are_deleted`
    /// and `legacy_channel_scoped_read_state_is_counted_on_its_own`.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn read_state_rows_survive_and_the_nip_rs_delete_guard_is_opted_into() {
        let f = fixture().await;
        let slot = format!("read-state:{}", "a".repeat(32));
        // Global (channel_id NULL) — the shape today's ingest writes.
        insert_event(
            &f.db,
            f.community,
            30078,
            &f.owner,
            None,
            Some(&slot),
            false,
        )
        .await;
        // Legacy shape: channel-scoped, so the DELETE trigger fires on it.
        let legacy_slot = format!("read-state:{}", "b".repeat(32));
        insert_event(
            &f.db,
            f.community,
            30078,
            &f.owner,
            Some(f.target_channel),
            Some(&legacy_slot),
            false,
        )
        .await;

        let store = f.db.project_purge_store();
        let inventory = store
            .inventory(f.community, &f.coordinate)
            .await
            .expect("inventory");
        assert!(inventory
            .exclusions
            .iter()
            .any(|note| note.table.contains("read-state")));

        store
            .purge(
                f.community,
                &f.coordinate,
                &inventory.digest,
                "lane-5-operator",
                None,
                true,
            )
            .await
            .expect("purge must not be blocked by the NIP-RS delete guard");

        // The global marker survives; the legacy channel-scoped one went with
        // its channel, which is only possible because the guard was opted into.
        let remaining: Vec<String> = sqlx::query_scalar(
            "SELECT d_tag FROM events WHERE community_id = $1 AND kind = 30078 ORDER BY d_tag",
        )
        .bind(f.community.as_uuid())
        .fetch_all(&f.db.pool)
        .await
        .expect("read-state probe");
        assert_eq!(remaining, vec![slot]);
    }

    /// Rows hanging off a tombstoned channel go with it; the identical rows on
    /// a sibling project's channel and on another community's channel do not.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn channel_child_rows_are_scoped_to_the_target_project_only() {
        let f = fixture().await;
        for (community, channel) in [
            (f.community, f.target_channel),
            (f.community, f.sibling_channel),
            (f.other_community, f.foreign_channel),
            (f.community, f.unrelated_channel),
        ] {
            sqlx::query(
                "INSERT INTO channel_members (community_id, channel_id, pubkey, role) \
                 VALUES ($1, $2, $3, 'member')",
            )
            .bind(community.as_uuid())
            .bind(channel)
            .bind(vec![5u8; 32])
            .execute(&f.db.pool)
            .await
            .expect("insert channel member");
        }

        let store = f.db.project_purge_store();
        let inventory = store
            .inventory(f.community, &f.coordinate)
            .await
            .expect("inventory");
        assert_eq!(inventory.scope.purge_rows["channel_members"], 1);

        store
            .purge(
                f.community,
                &f.coordinate,
                &inventory.digest,
                "lane-5-operator",
                None,
                true,
            )
            .await
            .expect("purge");

        for (community, channel, expected) in [
            (f.community, f.target_channel, 0i64),
            (f.community, f.sibling_channel, 1),
            (f.other_community, f.foreign_channel, 1),
            (f.community, f.unrelated_channel, 1),
        ] {
            let count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM channel_members WHERE community_id = $1 AND channel_id = $2",
            )
            .bind(community.as_uuid())
            .bind(channel)
            .fetch_one(&f.db.pool)
            .await
            .expect("member probe");
            assert_eq!(count, expected, "channel {channel} in {community:?}");
        }
    }

    /// The community row, its host reservation, and its lifecycle state are
    /// untouched by a project purge.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn the_community_row_survives_untouched() {
        let f = fixture().await;
        let before: (String, String) =
            sqlx::query_as("SELECT host, deletion_state FROM communities WHERE id = $1")
                .bind(f.community.as_uuid())
                .fetch_one(&f.db.pool)
                .await
                .expect("community probe");

        let store = f.db.project_purge_store();
        let inventory = store
            .inventory(f.community, &f.coordinate)
            .await
            .expect("inventory");
        store
            .purge(
                f.community,
                &f.coordinate,
                &inventory.digest,
                "lane-5-operator",
                None,
                true,
            )
            .await
            .expect("purge");

        let after: (String, String) =
            sqlx::query_as("SELECT host, deletion_state FROM communities WHERE id = $1")
                .bind(f.community.as_uuid())
                .fetch_one(&f.db.pool)
                .await
                .expect("community probe");
        assert_eq!(before, after);
    }

    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn the_live_coordinate_catalog_is_pinned() {
        let db = connect().await;
        let mut conn = db.pool.acquire().await.expect("acquire");
        validate_project_catalog(&mut conn)
            .await
            .expect("live catalog must match the pinned manifests");
    }

    /// The gap this closes: a later migration adds a table that reaches rows by
    /// `channel_id` and carries neither `project_ref` nor `coordinate`. The
    /// coordinate-only guard returned exactly its four pinned names, the purge
    /// reported clean, and the new table's rows survived with a dangling
    /// `channel_id` and no path back from the project coordinate.
    ///
    /// Rolled back, so the schema is unchanged either way.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn a_new_channel_id_table_fails_the_catalog_guard_closed() {
        let db = connect().await;
        let mut tx = db.pool.begin().await.expect("begin");
        validate_project_catalog(&mut tx)
            .await
            .expect("baseline catalog is clean");

        // Exactly the shape the review named: pinned by neither column.
        sqlx::query(
            "CREATE TABLE channel_pins ( \
                 community_id UUID NOT NULL, \
                 channel_id   UUID NOT NULL, \
                 pinned_by    BYTEA NOT NULL \
             )",
        )
        .execute(&mut *tx)
        .await
        .expect("create hypothetical table");

        let error = validate_project_catalog(&mut tx)
            .await
            .expect_err("a new channel_id table must block the purge");
        let message = error.to_string();
        assert!(message.contains("channel catalog drift"), "{message}");
        assert!(message.contains("channel_pins"), "{message}");

        tx.rollback().await.expect("rollback");

        // And the coordinate surface still reports under its own name.
        let mut tx = db.pool.begin().await.expect("begin");
        sqlx::query("CREATE TABLE project_pins (community_id UUID NOT NULL, project_ref TEXT)")
            .execute(&mut *tx)
            .await
            .expect("create hypothetical table");
        let message = validate_project_catalog(&mut tx)
            .await
            .expect_err("a new project_ref table must block the purge")
            .to_string();
        assert!(message.contains("coordinate catalog drift"), "{message}");
        assert!(message.contains("project_pins"), "{message}");
        tx.rollback().await.expect("rollback");
    }

    /// The receipt must report the legacy read-state rows it destroyed as
    /// their own number, not fold them anonymously into `deleted_rows["events"]`
    /// under an exclusion note claiming read state was left alone.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn legacy_channel_scoped_read_state_is_counted_on_its_own() {
        let f = fixture().await;
        // Global marker: survives, and must not be counted.
        insert_event(
            &f.db,
            f.community,
            30078,
            &f.owner,
            None,
            Some(&format!("read-state:{}", "a".repeat(32))),
            false,
        )
        .await;
        // Two legacy channel-scoped markers inside the tombstoned channel.
        for slot in ["b", "c"] {
            insert_event(
                &f.db,
                f.community,
                30078,
                &f.owner,
                Some(f.target_channel),
                Some(&format!("read-state:{}", slot.repeat(32))),
                false,
            )
            .await;
        }
        // A legacy marker in a *sibling* project's channel: out of scope.
        insert_event(
            &f.db,
            f.community,
            30078,
            &f.owner,
            Some(f.sibling_channel),
            Some(&format!("read-state:{}", "d".repeat(32))),
            false,
        )
        .await;

        let store = f.db.project_purge_store();
        let inventory = store
            .inventory(f.community, &f.coordinate)
            .await
            .expect("inventory");
        assert_eq!(inventory.scope.legacy_channel_scoped_read_state, 2);

        let receipt = store
            .purge(
                f.community,
                &f.coordinate,
                &inventory.digest,
                "lane-4-operator",
                None,
                true,
            )
            .await
            .expect("purge");
        assert_eq!(receipt.deleted_legacy_read_state, 2);
        // The operator-facing note printed with that number says so.
        let note = receipt
            .exclusions
            .iter()
            .find(|note| note.table.contains("read-state"))
            .expect("read-state note");
        assert!(note.table.contains("global rows only"), "{}", note.table);
        assert!(
            note.reason.contains("legacy_channel_scoped_read_state"),
            "{}",
            note.reason
        );

        // Exactly the global and sibling markers survive.
        let remaining: Vec<String> = sqlx::query_scalar(
            "SELECT d_tag FROM events WHERE community_id = $1 AND kind = 30078 ORDER BY d_tag",
        )
        .bind(f.community.as_uuid())
        .fetch_all(&f.db.pool)
        .await
        .expect("read-state probe");
        assert_eq!(
            remaining,
            vec![
                format!("read-state:{}", "a".repeat(32)),
                format!("read-state:{}", "d".repeat(32)),
            ]
        );
    }

    /// Two operators purging the same coordinate must not both get a receipt.
    /// The schema-destruction lock is shared with migrations by design and so
    /// cannot serialize them; without the per-coordinate exclusive lock the
    /// loser commits an all-zero `deleted_rows` receipt beside a non-zero
    /// `acknowledged_live_events`.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn concurrent_purges_of_one_coordinate_are_serialized() {
        let f = fixture().await;
        let store = f.db.project_purge_store();
        let inventory = store
            .inventory(f.community, &f.coordinate)
            .await
            .expect("inventory");

        // Stand in for the winning transaction: hold the coordinate's lock.
        let mut holder = f.db.pool.begin().await.expect("begin holder");
        sqlx::query("SELECT pg_advisory_xact_lock($1, $2)")
            .bind(PROJECT_PURGE_LOCK_CLASS)
            .bind(project_coordinate_lock_key(f.community, &f.coordinate))
            .execute(&mut *holder)
            .await
            .expect("hold coordinate lock");

        let blocked = store.purge(
            f.community,
            &f.coordinate,
            &inventory.digest,
            "lane-4-operator",
            None,
            true,
        );
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(2), blocked)
                .await
                .is_err(),
            "a second purge of the same coordinate must block on the first"
        );
        assert!(channel_exists(&f.db, f.community, f.target_channel).await);

        holder.rollback().await.expect("release coordinate lock");
        store
            .purge(
                f.community,
                &f.coordinate,
                &inventory.digest,
                "lane-4-operator",
                None,
                true,
            )
            .await
            .expect("purge proceeds once the lock is free");
        assert!(!channel_exists(&f.db, f.community, f.target_channel).await);
    }

    /// `shell_session_acl` is reported as retained. A kind:30623 announce
    /// h-tagged into a tombstoned channel would be destroyed by the channel
    /// arm and orphan its projection row, so it must block instead.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn a_shell_announce_in_a_tombstoned_channel_blocks_the_purge() {
        let f = fixture().await;
        insert_event(
            &f.db,
            f.community,
            30623,
            &f.owner,
            Some(f.target_channel),
            Some("sess-1"),
            false,
        )
        .await;

        let store = f.db.project_purge_store();
        let inventory = store
            .inventory(f.community, &f.coordinate)
            .await
            .expect("inventory");
        assert_eq!(inventory.scope.shell_announces_in_tombstoned_channels, 1);

        let error = store
            .purge(
                f.community,
                &f.coordinate,
                &inventory.digest,
                "lane-4-operator",
                None,
                true,
            )
            .await
            .expect_err("an h-tagged announce must block the purge");
        assert!(error.to_string().contains("kind:30623"), "{error}");
        assert!(channel_exists(&f.db, f.community, f.target_channel).await);
    }
}
