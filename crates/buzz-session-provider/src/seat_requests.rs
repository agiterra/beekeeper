//! The provider's account of which open generations still need seat custody.
//!
//! # Why this file exists
//!
//! A seated execution's key material is one-shot host-local custody: the
//! desktop writes an entry into the actor-seats file keyed by the *command id*
//! that mints a generation, and the provider consumes it at spawn
//! ([`crate::actor_seats`]). That channel has exactly one trigger — a
//! lifecycle command being published — and a provider restart is not one.
//! After a restart the provider wants to reopen the generations it already
//! owns *in place* ([`crate::native_restore`]), and for a seated generation
//! that needs the same seat's key again, under the same command id, with no
//! new command anywhere on the wire.
//!
//! So the provider states the need instead of guessing at it: one row per open
//! seated generation, rewritten whenever that set changes, in
//! `seat-requests.json` beside `state.json`. The desktop reads it after every
//! provider spawn and re-stages the custody that is missing
//! (`docs/CI_CONTINUATION_RECOVERY_SPEC.md` §3). The provider never reads it
//! back — its own truth is `state.json`.
//!
//! # What it may contain
//!
//! **No secrets.** Every field here is either already on the wire (the actor's
//! pubkey, its role, the project and session coordinates, the pack reference
//! republished in every kind:44223) or a host-local identifier that names
//! nothing private (the command id). The seat's nsec, its auth tag, its
//! working directory and its pack *directory* are deliberately absent: those
//! are the values the one-shot custody file exists to keep out of any
//! long-lived file, and putting them here would recreate exactly the at-rest
//! credential [`crate::state::SessionRecord`] already refuses to hold.
//!
//! The file is written through [`crate::state::atomic_write`], so a reader
//! that opens it mid-rewrite sees either the whole previous generation of it
//! or the whole next one.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::actor_seats::PackRef;
use crate::state::{atomic_write, SessionRecord};

/// Name of the seat-request file inside the provider state directory.
pub const SEAT_REQUESTS_FILE: &str = "seat-requests.json";

/// Schema version this build writes.
pub const SEAT_REQUESTS_VERSION: u32 = 1;

/// One open seated generation whose custody may need re-staging.
///
/// The key is [`Self::command_id`]: it is the command id the *current*
/// generation was minted by, which is the exact key the desktop files custody
/// under and the exact key [`crate::native_restore`] reads it back from. For a
/// generation that has never been resumed that is the create's own command id;
/// after a resume it is the resume's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeatRequest {
    /// The command id the current generation was minted by.
    pub command_id: String,
    /// The seat's pubkey, lowercase 64-hex. A public fact; the key it names is
    /// not in this file.
    pub actor: String,
    /// The role slug the seat holds, which is what picks its pack. Empty only
    /// for a record written before roles were paired with actors.
    pub role: String,
    /// `30621:<owner>:<id>` when the generation runs inside a project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_ref: Option<String>,
    /// The execution this generation belongs to.
    pub session_id: String,
    /// The generation number, unchanged by any re-staging.
    pub generation: u64,
    /// The pack this seat was last staged with, as the wire describes it.
    ///
    /// Informational: the host re-resolves the pack from the project rather
    /// than trusting this, because a pack source can move between restarts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pack_ref: Option<PackRef>,
}

/// The whole `seat-requests.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeatRequestsFile {
    /// Schema version, so an incompatible future shape is distinguishable.
    pub version: u32,
    /// Process that published the recovered request snapshot.
    #[serde(default)]
    pub provider_pid: u32,
    /// The open seated generations, one row each.
    pub requests: Vec<SeatRequest>,
}

impl Default for SeatRequestsFile {
    fn default() -> Self {
        Self {
            version: SEAT_REQUESTS_VERSION,
            provider_pid: std::process::id(),
            requests: Vec::new(),
        }
    }
}

/// Path of the seat-request file inside `state_dir`.
pub fn seat_requests_path(state_dir: &Path) -> PathBuf {
    state_dir.join(SEAT_REQUESTS_FILE)
}

/// The row one session record contributes, or `None` when it contributes none.
///
/// Two filters, both deliberate. A **closed** record's generation is retired,
/// so re-staging custody for it would put a usable key on disk for an
/// execution that can never take another turn. An **unseated** record runs as
/// the operator, holds no seat, and needs nothing staged — it restores from
/// its cursor alone.
fn request_for(record: &SessionRecord) -> Option<SeatRequest> {
    if record.closed {
        return None;
    }
    let actor = record.actor.clone()?;
    Some(SeatRequest {
        command_id: record
            .generation_command_id
            .clone()
            .unwrap_or_else(|| record.command_id.clone()),
        actor,
        role: record.role.clone().unwrap_or_default(),
        project_ref: record.project_ref.clone(),
        session_id: record.session_id.clone(),
        generation: record.generation,
        pack_ref: record.pack_ref.clone(),
    })
}

/// Every row `records` implies, in the order the records are given.
pub fn derive_seat_requests<'a, I>(records: I) -> Vec<SeatRequest>
where
    I: IntoIterator<Item = &'a SessionRecord>,
{
    records.into_iter().filter_map(request_for).collect()
}

/// Rewrite `seat-requests.json` to describe exactly the open seated
/// generations in `records`.
///
/// A full rewrite rather than an append: the file's whole meaning is "these
/// and only these generations still need custody", and a stale row would ask
/// the desktop to stage a key for a generation that has been stopped. An empty
/// set writes an empty `requests` array rather than deleting the file, so a
/// reader can tell "the provider says nothing is seated" from "the provider
/// has not written yet".
pub fn write_seat_requests<'a, I>(state_dir: &Path, records: I) -> io::Result<()>
where
    I: IntoIterator<Item = &'a SessionRecord>,
{
    let file = SeatRequestsFile {
        version: SEAT_REQUESTS_VERSION,
        provider_pid: std::process::id(),
        requests: derive_seat_requests(records),
    };
    let body = serde_json::to_vec_pretty(&file)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    atomic_write(&seat_requests_path(state_dir), &body)
}

#[cfg(test)]
#[path = "seat_requests_tests.rs"]
mod tests;
