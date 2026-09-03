#![deny(unsafe_code)]
#![warn(missing_docs)]
//! `buzz-core` — zero-I/O foundation types for the Buzz relay.
//!
//! Provides [`StoredEvent`], filter matching, kind constants, and event
//! verification. All other Buzz crates depend on this one.

/// NIP-AM: Agent Turn Metric — payload type and encrypt/decrypt helpers.
pub mod agent_turn_metric;
/// Channel and membership enums shared across crates.
pub mod channel;
/// NIP-CSAT (draft): append-only coding-session authority-chain transitions
/// (44228) — one `grant-operator` step at a time today.
pub mod coding_session_authority_transition;
/// NIP-CSPC: the kind:44222 provider catalog wire schema and its canonical
/// reader — the only list of models this product offers.
pub mod coding_session_catalog;
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
/// The model registry and the router: which execution target a class, a risk
/// tier and the live catalog select, and why.
pub mod coding_session_routing;
/// Runtime descriptors shared by the desktop host and the coding-session
/// provider sidecar (`BUZZ_CSP_RUNTIMES`).
pub mod coding_session_runtime;
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
/// Git permission types — ref patterns, protection rules, policy evaluation.
pub mod git_perms;
/// Shared invite-link contract constants.
pub mod invite;
/// Buzz kind number registry — custom event type constants.
pub mod kind;
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
/// Project Pulse entries (44240): the explicit coordination claim contract.
pub mod pulse;
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
/// The git hooks a seat's checkout gets so its local commits reach Pulse
/// without anyone being asked to report them.
pub mod seat_git_hooks;
/// Tenant identity — the server-resolved community key carried on scoped paths.
pub mod tenant;
/// Schnorr signature and event ID verification.
pub mod verification;
// L11: pure classification for a coding session's git worktree — what may be
// pruned, what is held, and what nothing may touch. No filesystem, no `git`.
/// Worktree lifecycle: what may be done with a coding session's git worktree.
pub mod worktree_lifecycle;

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
