//! Which of an adopted identity's records are history, not this Host's work.
//!
//! `actors/<actor>/adoption.json` names the sessions the identity already had.
//! An actor-wide quiescence scan must not read a fence on one of those as this
//! Host's unfinished stop. Nothing here proves those processes gone, and nothing
//! changes the records or their fences: a record is left out of the scan only
//! while it is exactly as the adoption found it. Anything less counts, so the
//! scan refuses as it always did.
use crate::{config::Config, state::SessionRecord};
use std::{collections::BTreeSet, path::PathBuf, time::SystemTime};

#[derive(Debug, Default)]
pub(crate) struct AdoptedBaseline {
    sessions: BTreeSet<String>,
    adopted_at: Option<SystemTime>,
}

impl AdoptedBaseline {
    /// Empty unless the record is readable, names this actor, carries its time,
    /// and names the owner the actor's own attestation names. Empty counts every
    /// record, so an untrusted baseline refuses rather than excuses.
    pub(crate) fn read(config: &Config, actor: &str) -> Self {
        Self::trusted(config, actor).unwrap_or_default()
    }

    fn trusted(config: &Config, actor: &str) -> Option<Self> {
        let record =
            beekeeper_host_core::agent_setup::read_adoption(&config.state_dir, actor).ok()??;
        let actor_key = nostr::PublicKey::from_hex(actor).ok()?;
        let auth = std::fs::read_to_string(
            config
                .state_dir
                .join("actors")
                .join(actor)
                .join("auth-tag.json"),
        )
        .ok()?;
        let attested = beekeeper_sdk::nip_oa::verify_auth_tag(auth.trim(), &actor_key).ok()?;
        if attested.to_hex() != record.owner {
            return None;
        }
        let adopted_at =
            chrono::DateTime::parse_from_rfc3339(record.adopted_at.as_deref()?).ok()?;
        Some(Self {
            sessions: record.prior_sessions,
            adopted_at: Some(adopted_at.into()),
        })
    }

    /// Named by the adoption, whatever has happened to it since.
    pub(crate) fn names(&self, session_id: &str) -> bool {
        self.sessions.contains(session_id)
    }

    /// Named by the adoption and untouched since: closed, no process witness
    /// (which any run on this Host would write), and any stop fence no newer than
    /// the adoption. An open record or a fence written later is current.
    pub(crate) fn settled_before_adoption(&self, config: &Config, record: &SessionRecord) -> bool {
        if !self.names(&record.session_id) || !record.closed || record.process_witness.is_some() {
            return false;
        }
        match std::fs::symlink_metadata(stop_fence(config, &record.session_id)) {
            Err(error) => error.kind() == std::io::ErrorKind::NotFound,
            Ok(fence) if fence.is_file() => fence
                .modified()
                .ok()
                .zip(self.adopted_at)
                .is_some_and(|(written, adopted)| written <= adopted),
            Ok(_) => false,
        }
    }
}

fn stop_fence(config: &Config, session_id: &str) -> PathBuf {
    config
        .state_dir
        .join(format!("stop-quiescence-{session_id}.json"))
}

#[cfg(all(test, unix))]
#[path = "adopted_baseline_tests.rs"]
mod tests;
