//! Durable, reversible transfer of a participant/community pair.
//!
//! This journal grants no relay authority and never kills a PID. The legacy
//! custodian verifies its tracked generation has stopped before quiescence;
//! adoption requires the existing provider's verified create receipt. Both
//! custodians refuse to execute while transfer is between owners.
use std::path::Path;

use beekeeper_core::coding_session_command::CodingSessionTarget;
use serde::{Deserialize, Serialize};

use crate::{
    atomic_write,
    layout::{self, Instance},
    record::canonical_relay_key,
};

/// The process owner at a recoverable transfer boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    /// Custody was staged; the legacy generation remains responsible.
    Prepared,
    /// The tracked legacy generation stopped; neither custodian may run it.
    Quiesced,
    /// Verified provider adoption makes the host the sole execution owner.
    Adopted,
    /// Host admission is fenced while all accepted responsibilities are stopped.
    Restoring,
    /// The host execution stopped; neither owner may run before restoration.
    HostStopped,
    /// Preserved legacy custody was restored after the host stopped.
    LegacyRestored,
}

/// One stable operation, bound to exact identity, community and generations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Transfer {
    /// Stable client operation identity; reused with other inputs is refused.
    pub operation_id: String,
    /// Scoped addressed participant, never the provider's key.
    pub actor_pubkey: String,
    /// Canonical community transport.
    pub relay_url: String,
    /// Preserved legacy harness generation (not a PID).
    pub legacy_nonce: String,
    /// Safe authorized definition event ID at staging.
    pub definition_event_id: String,
    /// Exact provider execution adopted after a verified receipt.
    pub target: Option<CodingSessionTarget>,
    /// Current durable custody boundary.
    pub phase: Phase,
}

#[derive(Default, Serialize, Deserialize)]
struct Journal {
    transfers: Vec<Transfer>,
    #[serde(default)]
    history: Vec<Transfer>,
}

/// Serialize client provisioning and transfer transitions using the same
/// owner-only lock. Drop the returned file before invoking another client.
pub fn management_lock(home: &Path, instance: Instance) -> Result<std::fs::File, String> {
    let dir = layout::host_dir(home, instance).join("management");
    atomic_write::create_dir_all_restricted(&dir)?;
    let mut options = std::fs::OpenOptions::new();
    options.create(true).read(true).write(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let path = dir.join("operations.lock");
    if std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err("management lock must not be a symlink".into());
    }
    let lock = options.open(path).map_err(|e| e.to_string())?;
    lock.lock().map_err(|e| e.to_string())?;
    Ok(lock)
}

/// Serialize runtime admission with a rollback fence for one participant pair.
/// This is separate from the client management lock: clients may await relay
/// receipts while the worker admits their execution. Hold through runtime
/// startup and durable record insertion, or through `begin_restoration`.
pub fn admission_lock(
    home: &Path,
    instance: Instance,
    actor: &str,
    relay: &str,
) -> Result<std::fs::File, String> {
    let lock = open_admission_lock(home, instance, actor, relay)?;
    lock.lock().map_err(|e| e.to_string())?;
    Ok(lock)
}

/// Attempt worker admission without blocking its event loop. A busy pair is a
/// disclosed retryable prerequisite, never permission for a second execution.
pub fn try_admission_lock(
    home: &Path,
    instance: Instance,
    actor: &str,
    relay: &str,
) -> Result<std::fs::File, String> {
    let lock = open_admission_lock(home, instance, actor, relay)?;
    lock.try_lock()
        .map_err(|e| format!("ACTOR_OWNERSHIP_BUSY: {e}"))?;
    Ok(lock)
}

fn open_admission_lock(
    home: &Path,
    instance: Instance,
    actor: &str,
    relay: &str,
) -> Result<std::fs::File, String> {
    use nostr::hashes::{sha256, Hash};
    nostr::PublicKey::from_hex(actor).map_err(|_| "invalid admission actor")?;
    let pair = format!(
        "{}:{}",
        actor.to_ascii_lowercase(),
        canonical_relay_key(relay)
    );
    let dir = layout::host_dir(home, instance).join("management");
    atomic_write::create_dir_all_restricted(&dir)?;
    let path = dir.join(format!(
        "admission-{}.lock",
        sha256::Hash::hash(pair.as_bytes())
    ));
    if std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err("admission lock must not be a symlink".into());
    }
    let mut options = std::fs::OpenOptions::new();
    options.create(true).read(true).write(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let lock = options.open(path).map_err(|e| e.to_string())?;
    Ok(lock)
}

/// Fence new host admissions before enumerating and stopping every accepted
/// responsibility. Caller holds the pair admission lock. Existing stop receipts
/// must still be verified before advancing to legacy restoration.
pub fn begin_restoration(
    home: &Path,
    instance: Instance,
    actor: &str,
    relay: &str,
) -> Result<Transfer, String> {
    let mut current =
        for_pair(home, instance, actor, relay)?.ok_or("no ownership transfer to restore")?;
    if current.phase == Phase::Restoring {
        return Ok(current);
    }
    if !matches!(current.phase, Phase::Adopted | Phase::HostStopped) {
        return Err("restoration requires verified host adoption".into());
    }
    current.phase = Phase::Restoring;
    advance(home, instance, current.clone())?;
    Ok(current)
}

fn read(home: &Path, instance: Instance) -> Result<Journal, String> {
    let path = layout::host_dir(home, instance).join("managed-ownership.json");
    match std::fs::read(&path) {
        Ok(bytes) => {
            serde_json::from_slice(&bytes).map_err(|e| format!("invalid ownership journal: {e}"))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Journal::default()),
        Err(e) => Err(format!("cannot read ownership journal: {e}")),
    }
}

/// Read the current transfer for a participant/community pair.
/// An unreadable journal is a refusal, never permission to spawn a fallback.
pub fn for_pair(
    home: &Path,
    instance: Instance,
    actor: &str,
    relay: &str,
) -> Result<Option<Transfer>, String> {
    let relay = canonical_relay_key(relay);
    Ok(read(home, instance)?
        .transfers
        .into_iter()
        .find(|t| t.actor_pubkey.eq_ignore_ascii_case(actor) && t.relay_url == relay))
}

/// Refuse legacy execution after quiescence, including across client restart.
pub fn require_legacy_owner(
    home: &Path,
    instance: Instance,
    actor: &str,
    relay: &str,
) -> Result<(), String> {
    if let Some(t) = for_pair(home, instance, actor, relay)? {
        if !matches!(t.phase, Phase::Prepared | Phase::LegacyRestored) {
            return Err(format!("agent ownership is {:?} for operation {}; use its accepted session through the relay", t.phase, t.operation_id));
        }
    }
    Ok(())
}

/// Record an acknowledged terminal provider execution without restoring the
/// legacy owner. Call only after the existing lifecycle receipt is verified.
pub fn host_stopped(
    home: &Path,
    instance: Instance,
    actor: &str,
    relay: &str,
    target: &CodingSessionTarget,
) -> Result<(), String> {
    let Some(mut current) = for_pair(home, instance, actor, relay)? else {
        return Ok(());
    };
    if current.target.as_ref() != Some(target) {
        return Err("stop receipt belongs to a different ownership generation".into());
    }
    if current.phase == Phase::HostStopped {
        return Ok(());
    }
    if current.phase != Phase::Adopted {
        return Err("host stop acknowledgment requires adopted ownership".into());
    }
    current.phase = Phase::HostStopped;
    advance(home, instance, current)
}

/// Adopt a newly accepted responsibility after the previous host execution
/// stopped. Preserve the retired record; this is a new execution, not native
/// resumption of a terminal session. Callers verify the existing create receipt.
pub fn successor(
    home: &Path,
    instance: Instance,
    actor: &str,
    relay: &str,
    operation: &str,
    revision: &str,
    target: CodingSessionTarget,
) -> Result<(), String> {
    let mut journal = read(home, instance)?;
    let relay = canonical_relay_key(relay);
    let Some(index) = journal
        .transfers
        .iter()
        .position(|t| t.actor_pubkey.eq_ignore_ascii_case(actor) && t.relay_url == relay)
    else {
        return Ok(());
    };
    let old = journal.transfers[index].clone();
    if old.phase == Phase::Adopted
        && old.operation_id == operation
        && old.target.as_ref() == Some(&target)
    {
        return Ok(());
    }
    if old.phase != Phase::HostStopped {
        return Err("successor requires acknowledged terminal prior execution".into());
    }
    nostr::EventId::from_hex(revision).map_err(|_| "invalid successor definition revision")?;
    if operation.is_empty() || old.operation_id == operation {
        return Err("successor requires a new operation identity".into());
    }
    let mut next = old.clone();
    next.operation_id = operation.into();
    next.definition_event_id = revision.into();
    next.target = Some(target);
    next.phase = Phase::Adopted;
    journal.history.push(old);
    journal.transfers[index] = next;
    let dir = layout::host_dir(home, instance);
    atomic_write::atomic_write_json_restricted(
        &dir.join("managed-ownership.json"),
        &serde_json::to_vec_pretty(&journal).map_err(|e| e.to_string())?,
    )
}

/// Advance a verified handoff boundary, preserving custody and operation identity.
/// Callers serialize with the host's management lock. This function performs no
/// process signalling; an acknowledged stopped generation must precede the
/// corresponding transition, and an accepted target must precede adoption.
pub fn advance(home: &Path, instance: Instance, mut next: Transfer) -> Result<(), String> {
    nostr::PublicKey::from_hex(&next.actor_pubkey).map_err(|_| "invalid transfer actor")?;
    nostr::EventId::from_hex(&next.definition_event_id)
        .map_err(|_| "invalid definition revision")?;
    if next.operation_id.trim().is_empty() || next.legacy_nonce.trim().is_empty() {
        return Err("transfer requires operation identity and tracked legacy generation".into());
    }
    next.actor_pubkey.make_ascii_lowercase();
    next.relay_url = canonical_relay_key(&next.relay_url);
    url::Url::parse(&next.relay_url).map_err(|_| "invalid transfer relay")?;
    let mut journal = read(home, instance)?;
    if let Some(index) = journal.transfers.iter().position(|t| {
        t.actor_pubkey == next.actor_pubkey
            && t.relay_url == next.relay_url
            && t.phase == Phase::LegacyRestored
            && t.operation_id != next.operation_id
    }) {
        if next.phase != Phase::Prepared || next.target.is_some() {
            return Err("a fresh transfer must stage legacy custody".into());
        }
        let retired = journal.transfers.remove(index);
        journal.history.push(retired);
    }
    if let Some(old) = journal
        .transfers
        .iter_mut()
        .find(|t| t.actor_pubkey == next.actor_pubkey && t.relay_url == next.relay_url)
    {
        if *old == next {
            return Ok(());
        }
        if old.operation_id != next.operation_id
            || old.legacy_nonce != next.legacy_nonce
            || old.definition_event_id != next.definition_event_id
        {
            return Err("ownership transfer operation conflicts with preserved inputs".into());
        }
        let allowed = matches!(
            (&old.phase, &next.phase),
            (Phase::Prepared, Phase::Quiesced)
                | (Phase::Prepared, Phase::LegacyRestored)
                | (Phase::Quiesced, Phase::Adopted)
                | (Phase::Adopted, Phase::HostStopped)
                | (Phase::Adopted, Phase::Restoring)
                | (Phase::HostStopped, Phase::Restoring)
                | (Phase::Restoring, Phase::LegacyRestored)
                | (Phase::HostStopped, Phase::LegacyRestored)
                | (Phase::Quiesced, Phase::LegacyRestored)
        );
        if !allowed {
            return Err("ownership transfer cannot skip a verified boundary".into());
        }
        if next.phase == Phase::Adopted && next.target.is_none() {
            return Err("adoption requires verified accepted execution".into());
        }
        if old.target.is_some() && old.target != next.target {
            return Err("ownership transfer target conflicts".into());
        }
        *old = next;
    } else {
        if next.phase != Phase::Prepared || next.target.is_some() {
            return Err("transfer must begin with staged legacy custody".into());
        }
        journal.transfers.push(next);
    }
    let dir = layout::host_dir(home, instance);
    atomic_write::create_dir_all_restricted(&dir)?;
    atomic_write::atomic_write_json_restricted(
        &dir.join("managed-ownership.json"),
        &serde_json::to_vec_pretty(&journal).map_err(|e| e.to_string())?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pair_admission_fences_startup_without_blocking_other_communities() {
        let home = tempfile::tempdir().unwrap();
        let actor = "ab".repeat(32);
        let guard =
            admission_lock(home.path(), Instance::Dev, &actor, "wss://one.example/").unwrap();
        assert!(
            try_admission_lock(home.path(), Instance::Dev, &actor, "wss://one.example").is_err()
        );
        assert!(
            try_admission_lock(home.path(), Instance::Dev, &actor, "wss://two.example").is_ok()
        );
        drop(guard);
        assert!(
            try_admission_lock(home.path(), Instance::Dev, &actor, "wss://one.example").is_ok()
        );
    }
    #[test]
    fn interrupted_handoff_refuses_duplicate_owner_and_rollback_requires_host_stop() {
        let home = tempfile::tempdir().unwrap();
        let mut t = Transfer {
            operation_id: "op-1".into(),
            actor_pubkey: "ab".repeat(32),
            relay_url: "wss://relay.example/".into(),
            legacy_nonce: "generation-1".into(),
            definition_event_id: "cd".repeat(32),
            target: None,
            phase: Phase::Prepared,
        };
        advance(home.path(), Instance::Dev, t.clone()).unwrap();
        require_legacy_owner(home.path(), Instance::Dev, &t.actor_pubkey, &t.relay_url).unwrap();
        t.phase = Phase::Quiesced;
        advance(home.path(), Instance::Dev, t.clone()).unwrap();
        advance(home.path(), Instance::Dev, t.clone()).unwrap();
        assert!(
            require_legacy_owner(home.path(), Instance::Dev, &t.actor_pubkey, &t.relay_url)
                .is_err()
        );
        t.target = Some(CodingSessionTarget {
            driver: "codex-acp".into(),
            instance_id: "primary".into(),
            session_id: uuid_for_test(),
            generation: 1,
        });
        t.phase = Phase::Adopted;
        advance(home.path(), Instance::Dev, t.clone()).unwrap();
        t.phase = Phase::LegacyRestored;
        assert!(advance(home.path(), Instance::Dev, t.clone()).is_err());
        t.phase = Phase::HostStopped;
        advance(home.path(), Instance::Dev, t.clone()).unwrap();
        {
            let _admission =
                admission_lock(home.path(), Instance::Dev, &t.actor_pubkey, &t.relay_url).unwrap();
            t = begin_restoration(home.path(), Instance::Dev, &t.actor_pubkey, &t.relay_url)
                .unwrap();
            assert_eq!(t.phase, Phase::Restoring);
            assert!(require_legacy_owner(
                home.path(),
                Instance::Dev,
                &t.actor_pubkey,
                &t.relay_url
            )
            .is_err());
            assert_eq!(
                begin_restoration(home.path(), Instance::Dev, &t.actor_pubkey, &t.relay_url)
                    .unwrap(),
                t
            );
        }
        t.phase = Phase::LegacyRestored;
        advance(home.path(), Instance::Dev, t.clone()).unwrap();
        require_legacy_owner(home.path(), Instance::Dev, &t.actor_pubkey, &t.relay_url).unwrap();
        t.legacy_nonce = "different".into();
        assert!(advance(home.path(), Instance::Dev, t).is_err());
    }
    fn uuid_for_test() -> String {
        "00000000-0000-0000-0000-000000000001".into()
    }
}
