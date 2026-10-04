#![deny(unsafe_code)]
#![warn(missing_docs)]
//! `buzz-core` — zero-I/O foundation types for the Buzz relay.
//!
//! Provides [`StoredEvent`], filter matching, kind constants, and event
//! verification. All other Buzz crates depend on this one.

/// NIP-AM: Agent Turn Metric — payload type and encrypt/decrypt helpers.
pub mod agent_turn_metric;
/// Project Pulse entries (44240): the explicit coordination claim contract.
/// Agents-repository draft ops (44250): the wire contract of one proposed
/// file change to a project's agents repository.
pub mod agents_repo_draft;
/// Pure agents-repository draft fold shared by every adapter, pinned by
/// `conformance/agents-repo-draft-fold/`.
pub mod agents_repo_draft_fold;
/// This build's own commit, its ordinal and its time — the client-side
/// counterpart of the relay's NIP-11 `software_commit`, with the same
/// disclosed `unknown`/`null` non-answers.
pub mod build_info;
/// Channel and membership enums shared across crates.
pub mod channel;
/// Relay-recorded terminal CI results and their exact correlation contract.
pub mod ci_result;
/// The canonical execution-claim fold over an accepted 44228 chain: who holds
/// a session, on which body, and when a regrant does *not* hand it back.
pub mod coding_session_authority_claim;
/// NIP-CSAT (draft): append-only coding-session authority-chain transitions
/// (44228) — grants, seats, and the `takeover`/`transfer` claims.
pub mod coding_session_authority_transition;
/// NIP-CSPC: the kind:44222 provider catalog wire schema and its canonical
/// reader — the only list of models this product offers.
pub mod coding_session_catalog;
/// NIP-CSCK: provider-signed turn checkpoints (44231) — the tree and commit
/// SHAs at a turn's end, its transcript range, and the files it changed.
pub mod coding_session_checkpoint;
/// Append-only coding-session closure revisions (44230): provider-independent
/// shared close/reopen state rooted at the session genesis.
pub mod coding_session_closure;
/// NIP-CSC: Coding-session command — provider-neutral turn request payload.
pub mod coding_session_command;
/// Private, bounded package for verified cross-machine session rehydration.
pub mod coding_session_context;
/// NIP-CSG: Coding-session genesis — the operator-signed origin of an umbrella
/// session, and the founder every later operation resolves back to.
pub mod coding_session_genesis;
/// NIP-CSG: append-only human-authored session-goal revisions (44227).
pub mod coding_session_goal;
/// NIP-CSH: signed coding-session handover records (44247) — durable
/// checkpoints of the work, and the continuations that pick it up.
pub mod coding_session_handover;
/// The canonical projection of one umbrella's handover records: standing,
/// latest authorized checkpoint, active continuation, retirement.
pub mod coding_session_handover_fold;
/// One name per thing: the four coding-session identity words (provider
/// instance alias, provider instance id, runtime word, driver slug).
pub mod coding_session_identity;
/// Ephemeral provider-signed liveness leases for exact coding-session generations.
pub mod coding_session_lease;
/// NIP-CSL: Coding-session lifecycle command — session creation payload.
pub mod coding_session_lifecycle_command;
/// NIP-CSN: append-only human-authored session-name revisions (44229).
pub mod coding_session_name;
/// NIP-CSOB: signed coding-session observations (44246) - checkpoint reports,
/// gate rows, findings dispositions, and phase timing.
pub mod coding_session_observation;
/// Provider-authored coding-session facts: receipts (44224), metadata (44223),
/// and transcript envelopes (44225).
pub mod coding_session_payload;
/// NIP-CSP: the signed session policy record (44245) - posture, budget,
/// attention, gates, bench, irreversible acts, and stop conditions.
pub mod coding_session_policy;
/// NIP-CSAT: the narrow, owner-signed project-action delegation carried in a
/// session's authority chain, and the fold that says whether one is live.
pub mod coding_session_project_action_grant;
/// The model registry and the router: which execution target a class, a risk
/// tier and the live catalog select, and why.
pub mod coding_session_routing;
/// Runtime descriptors shared by the desktop host and the coding-session
/// provider sidecar (`BUZZ_CSP_RUNTIMES`).
pub mod coding_session_runtime;
/// Where a seat's skill bundle lives: the directory names and the session-id
/// sanitizer the provider and the desktop host both compose paths from.
pub mod coding_session_seat_bundle;
/// NIP-CSTX: signed, append-only team transactions inside a coding session
/// (44244).
pub mod coding_session_team_transaction;
/// The rule a `require-verdict` ref enforces: which canonical mission ruling
/// admits a commit, and which key may land it.
pub mod coding_session_verdict_admission;
/// NIP-AE Agent Engrams — slug grammar, conversation key, d-tag derivation,
/// body parse/serialize, envelope build/validate, head selection.
pub mod engram;
/// Relay-side error types.
pub mod error;
/// Relay-side event wrapper with verification tracking.
pub mod event;
/// NIP-01 subscription filter matching.
pub mod filter;
/// Fractional ranks for ordered, concurrently edited lists.
pub mod fractional_rank;
/// Git permission types — ref patterns, protection rules, policy evaluation.
pub mod git_perms;
/// Host-executed workflow step request, claim and result payloads (kinds
/// 46013, 46014, 46022, 46023).
pub mod host_step;
/// Shared invite-link contract constants.
pub mod invite;
/// Buzz kind number registry — custom event type constants.
pub mod kind;
/// Where the model registry is looked for, in order, and which copy answered:
/// the project's agents repository, then its code checkout.
pub mod model_registry_source;
/// Network utilities — SSRF-safe IP classification.
pub mod network;
/// Agent observer frame helpers.
pub mod observer;
/// NIP-AB device pairing — crypto primitives, message types, and errors.
pub mod pairing;
/// Presence status types shared across crates.
pub mod presence;
/// NIP-PMA owner-encrypted private managed-agent wire codec.
pub mod private_managed_agent;
pub mod project_agent_association;
/// NIP-AR: project artifact pins (44251) — the wire contract of one pin or
/// one reorder.
pub mod project_artifact_pin;
/// Pure project artifact pin fold shared by every adapter, pinned by
/// `conformance/project-artifact-pin-fold/`.
pub mod project_artifact_pin_fold;
pub mod project_pack_source;
/// NIP-PW: the `beekeeper-plan/v1` plan file — the committed statement of
/// what success means, parsed from bytes with no I/O and no clock.
pub mod project_plan;
/// Project to-do ops (44248): the wire contract of one field-level edit.
pub mod project_todo;
/// Pure project to-do fold shared by every adapter, pinned by
/// `conformance/project-todo-fold/`.
pub mod project_todo_fold;
/// NIP-PW: the closed kind:44249 work-record envelope — a lead's adoption of
/// a plan, and the assignments and evidence bound to its criteria.
pub mod project_work;
/// NIP-PW: which evidence bindings follow mechanically from relay-signed
/// facts, so the host that ran a step can bind them itself.
pub mod project_work_autobind;
/// NIP-PW: the pure, order-independent coverage fold over a session's work
/// records — what remains, who owes it and what proves it.
pub mod project_work_fold;
/// NIP-PW: the one shared assembler from fetched events to the fold's input.
pub mod project_work_inputs;
pub mod pulse;
/// Declared work in Project Pulse: assignments, their evidence, and the one
/// word the canonical fold settles them with.
pub mod pulse_declared_work;
/// Pure Project Pulse v2 digest model and fold shared by every adapter.
pub mod pulse_fold;
/// What Pulse knows about a mission without asking anyone to report.
pub mod pulse_mission;
/// Two umbrellas touching the same file, computed rather than reported.
pub mod pulse_overlap;
/// The registry bench — the mechanical scorer that turns a routing prior into
/// a measurement, and the rules a proposed row must clear.
pub mod registry_bench;
/// Canonical relay runtime identities.
pub mod relay;
/// Who founded a repository — signer, NIP-34 maintainers, project owners.
pub mod repository_founders;
pub mod repository_protection;
/// `sandbox.yml` — how a project's build state is seeded into a fresh sandbox,
/// and what counts as build state when that sandbox is reclaimed.
pub mod sandbox_manifest;
/// Carrying out a `sandbox.yml` declaration, and the inverse verb that frees
/// what it seeded.
pub mod sandbox_seed;
/// [`sandbox_seed::SeedOps`] over a real filesystem, with git delegated to
/// whoever is allowed to run it.
pub mod sandbox_seed_fs;
/// The git identity a seat's commits are authored as — derived from its own
/// key, its role and its project, never asked of a person.
pub mod seat_commit_identity;
/// The git hooks a seat's checkout gets so its local commits reach Pulse
/// without anyone being asked to report them.
pub mod seat_git_hooks;
/// The team-fold vocabulary, carried in the binary rather than in the source
/// tree — the words `bee sessions explain` answers with.
pub mod team_vocabulary;
/// Tenant identity — the server-resolved community key carried on scoped paths.
pub mod tenant;
/// Schnorr signature and event ID verification.
pub mod verification;
/// Workflow approval grant content and autorun grant/revoke payloads (kinds
/// 46030, 46015, 46032).
pub mod workflow_autorun;
// L11: pure classification for a coding session's git worktree — what may be
// pruned, what is held, and what nothing may touch. No filesystem, no `git`.
/// Worktree lifecycle: what may be done with a coding session's git worktree.
pub mod worktree_lifecycle;
pub mod worktree_placement;

pub use error::VerificationError;
pub use event::StoredEvent;
pub use nostr::{Event, EventId, Filter, Keys, Kind, PublicKey};
pub use presence::PresenceStatus;
pub use tenant::{normalize_host, CommunityId, TenantContext};
pub use verification::verify_event;

#[cfg(any(test, feature = "test-utils"))]
/// Test helper utilities for creating events and stored events.
pub mod test_helpers {
    use crate::StoredEvent;
    use chrono::Utc;
    use nostr::{EventBuilder, Keys, Kind};

    /// Create a signed test event with the given kind and random keys.
    pub fn make_event(kind: Kind) -> nostr::Event {
        let keys = Keys::generate();
        EventBuilder::new(kind, "test")
            .tags([])
            .sign_with_keys(&keys)
            .expect("sign")
    }

    /// Create a signed test event with the given keys and kind.
    pub fn make_event_with_keys(keys: &Keys, kind: Kind) -> nostr::Event {
        EventBuilder::new(kind, "test")
            .tags([])
            .sign_with_keys(keys)
            .expect("sign")
    }

    /// Create a [`StoredEvent`] wrapper around a test event.
    pub fn make_stored_event(kind: Kind, channel_id: Option<uuid::Uuid>) -> StoredEvent {
        StoredEvent::with_received_at(make_event(kind), Utc::now(), channel_id, true)
    }
}
