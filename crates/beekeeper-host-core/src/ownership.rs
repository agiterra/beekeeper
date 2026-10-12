//! Durable sole host custody and modern enrollment of a participant/community pair.
//!
//! This journal grants no relay authority and never signals a persisted PID.
//! Enrollment and successor transitions require verified provider receipts.
//! Historical import phases remain readable solely to deny unsupported execution.

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
    /// Unsupported historical staged custody; decode only, never admitted.
    Prepared,
    /// Unsupported historical import boundary; decode only, never admitted.
    Quiesced,
    /// Verified provider adoption makes the host the sole execution owner.
    Adopted,
    /// Historical rollback fence; retained for reading old journals, never newly entered.
    Restoring,
    /// Host execution is stopped; only verified host admission may succeed it.
    HostStopped,
    /// Historical restored custody; retained for reading history, never newly entered.
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
    /// Enrollment identity marker; historical legacy values remain read-only.
    pub legacy_nonce: String,
    /// Safe authorized definition event ID at staging.
    pub definition_event_id: String,
    /// Exact provider execution adopted after a verified receipt.
    pub target: Option<CodingSessionTarget>,
    /// Current durable custody boundary.
    pub phase: Phase,
}

/// Whether custody originated in canonical fresh host enrollment. Historical
/// import markers remain readable, but cannot authorize new execution.
pub fn is_modern_enrollment(transfer: &Transfer) -> bool {
    transfer
        .legacy_nonce
        .strip_prefix("fresh-identity:")
        .is_some_and(|operation| uuid::Uuid::parse_str(operation).is_ok())
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

// Internal short-lived serialization for the shared journal, independent of
// caller management/pair locks. No await or admission lock is acquired here.
fn journal_lock(home: &Path, instance: Instance) -> Result<std::fs::File, String> {
    let dir = layout::host_dir(home, instance).join("management");
    atomic_write::create_dir_all_restricted(&dir)?;
    let path = dir.join("ownership-journal.lock");
    if std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err("ownership journal lock must not be a symlink".into());
    }
    let mut options = std::fs::OpenOptions::new();
    options.create(true).read(true).write(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let lock = options.open(path).map_err(|e| e.to_string())?;
    lock.lock().map_err(|e| e.to_string())?;
    Ok(lock)
}

/// Serialize runtime admission and verified stop boundaries for one pair.
/// This is separate from the client management lock: clients may await relay
/// receipts while the worker admits their execution. Hold through runtime
/// startup and durable record insertion, or through a verified stop boundary.
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

/// Record sole host custody for a newly enrolled identity after the shared
/// service verifies its custody/publication and, when running, create receipt.
/// A stopped enrollment has no execution target. Existing legacy/import
/// ownership cannot be overwritten by this fresh-identity boundary.
#[allow(clippy::too_many_arguments)]
pub fn record_host_enrollment(
    home: &Path,
    instance: Instance,
    actor: &str,
    relay: &str,
    operation: &str,
    revision: &str,
    target: Option<CodingSessionTarget>,
) -> Result<(), String> {
    nostr::PublicKey::from_hex(actor).map_err(|_| "invalid enrolled actor")?;
    nostr::EventId::from_hex(revision).map_err(|_| "invalid enrolled revision")?;
    uuid::Uuid::parse_str(operation).map_err(|_| "enrollment requires operation UUID")?;
    let _journal_lock = journal_lock(home, instance)?;
    let mut journal = read(home, instance)?;
    let relay = canonical_relay_key(relay);
    let next = Transfer {
        operation_id: operation.into(),
        actor_pubkey: actor.to_ascii_lowercase(),
        relay_url: relay,
        legacy_nonce: format!("fresh-identity:{operation}"),
        definition_event_id: revision.into(),
        phase: if target.is_some() {
            Phase::Adopted
        } else {
            Phase::HostStopped
        },
        target,
    };
    if let Some(held) = journal
        .transfers
        .iter_mut()
        .find(|t| t.actor_pubkey == next.actor_pubkey && t.relay_url == next.relay_url)
    {
        if *held == next {
            return Ok(());
        }
        if held.phase == Phase::HostStopped
            && held.target.is_none()
            && next.phase == Phase::Adopted
            && next.target.is_some()
            && held.operation_id == next.operation_id
            && held.definition_event_id == next.definition_event_id
            && held.legacy_nonce == next.legacy_nonce
        {
            // Only the exact configured fresh enrollment may be completed by
            // its verified receipt. A legacy/import nonce never matches this.
            *held = next;
        } else {
            return Err(
                "existing ownership must be reconciled; fresh enrollment cannot replace it".into(),
            );
        }
    } else {
        journal.transfers.push(next);
    }
    atomic_write::atomic_write_json_restricted(
        &layout::host_dir(home, instance).join("managed-ownership.json"),
        &serde_json::to_vec_pretty(&journal).map_err(|_| "cannot encode enrolled ownership")?,
    )
}

/// Replace the signed definition revision of an unstarted fresh enrollment.
/// Callers must hold the pair admission lock and verify the new owner-signed
/// head readback before this compare-and-swap. The internal journal lock
/// serializes different pairs; a worker must not acquire client management. Imported custody
/// and any enrollment with an execution target are never changed here.
#[allow(clippy::too_many_arguments)]
pub fn revise_fresh_configured_enrollment(
    home: &Path,
    instance: Instance,
    actor: &str,
    relay: &str,
    operation: &str,
    previous_revision: &str,
    verified_revision: &str,
) -> Result<(), String> {
    nostr::EventId::from_hex(verified_revision)
        .map_err(|_| "invalid verified definition revision")?;
    let _journal_lock = journal_lock(home, instance)?;
    let mut journal = read(home, instance)?;
    let relay = canonical_relay_key(relay);
    let held = journal
        .transfers
        .iter_mut()
        .find(|t| t.actor_pubkey.eq_ignore_ascii_case(actor) && t.relay_url == relay)
        .ok_or("fresh configured enrollment unavailable")?;
    if held.phase != Phase::HostStopped
        || held.target.is_some()
        || held.operation_id != operation
        || held.legacy_nonce != format!("fresh-identity:{operation}")
        || (held.definition_event_id != previous_revision
            && held.definition_event_id != verified_revision)
    {
        return Err("definition revision update requires exact unstarted fresh enrollment".into());
    }
    held.definition_event_id = verified_revision.into();
    atomic_write::atomic_write_json_restricted(
        &layout::host_dir(home, instance).join("managed-ownership.json"),
        &serde_json::to_vec_pretty(&journal).map_err(|_| "cannot encode configured ownership")?,
    )
}

/// Follow one verified resume of the same adopted execution.
/// The worker must hold pair admission and durably accept the new record before
/// calling this exact-target compare-and-swap, then publish its Resumed receipt.
/// Custody, operation, definition and history remain unchanged. Unmanaged pairs
/// are untouched; an existing managed pair never accepts a stale/foreign target.
pub fn follow_generation(
    home: &Path,
    instance: Instance,
    actor: &str,
    relay: &str,
    previous: &CodingSessionTarget,
    next: &CodingSessionTarget,
) -> Result<(), String> {
    if previous.driver != next.driver
        || previous.instance_id != next.instance_id
        || previous.session_id != next.session_id
        || previous.generation.checked_add(1) != Some(next.generation)
    {
        return Err(
            "resume requires the immediately following generation of the same execution".into(),
        );
    }
    let _journal_lock = journal_lock(home, instance)?;
    let mut journal = read(home, instance)?;
    let relay = canonical_relay_key(relay);
    let Some(held) = journal
        .transfers
        .iter_mut()
        .find(|t| t.actor_pubkey.eq_ignore_ascii_case(actor) && t.relay_url == relay)
    else {
        return Ok(());
    };
    if held.phase != Phase::Adopted || held.target.as_ref() != Some(previous) {
        return Err("resume ownership target is stale or not adopted".into());
    }
    held.target = Some(next.clone());
    atomic_write::atomic_write_json_restricted(
        &layout::host_dir(home, instance).join("managed-ownership.json"),
        &serde_json::to_vec_pretty(&journal).map_err(|_| "cannot encode resumed ownership")?,
    )
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
    let _journal_lock = journal_lock(home, instance)?;
    let mut journal = read(home, instance)?;
    let held = journal
        .transfers
        .iter_mut()
        .find(|t| {
            t.actor_pubkey.eq_ignore_ascii_case(actor) && t.relay_url == canonical_relay_key(relay)
        })
        .ok_or("ownership disappeared")?;
    if held.phase == Phase::HostStopped && held.target.as_ref() == Some(target) {
        return Ok(());
    }
    if held.phase != Phase::Adopted || held.target.as_ref() != Some(target) {
        return Err("ownership changed before stop commit".into());
    }
    *held = current;
    atomic_write::atomic_write_json_restricted(
        &layout::host_dir(home, instance).join("managed-ownership.json"),
        &serde_json::to_vec_pretty(&journal).map_err(|_| "ownership encoding failed")?,
    )
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
    let _journal_lock = journal_lock(home, instance)?;
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
    if !is_modern_enrollment(&old) {
        return Err("historical setup is retired; recreate with a fresh identity".into());
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn concurrent_same_target_stop_acknowledgments_are_idempotent() {
        let home = tempfile::tempdir().unwrap();
        let actor = "ab".repeat(32);
        let relay = "wss://isolated.example";
        let target = CodingSessionTarget {
            driver: "managed-acp".into(),
            instance_id: "primary".into(),
            session_id: uuid_for_test(),
            generation: 1,
        };
        record_host_enrollment(
            home.path(),
            Instance::Dev,
            &actor,
            relay,
            &uuid_for_test(),
            &"cd".repeat(32),
            Some(target.clone()),
        )
        .unwrap();
        std::thread::scope(|scope| {
            let mut threads = Vec::new();
            for _ in 0..16 {
                let target = &target;
                let actor = &actor;
                let home = home.path();
                threads.push(
                    scope.spawn(move || host_stopped(home, Instance::Dev, actor, relay, target)),
                );
            }
            for thread in threads {
                thread.join().unwrap().unwrap();
            }
        });
        assert_eq!(
            for_pair(home.path(), Instance::Dev, &actor, relay)
                .unwrap()
                .unwrap()
                .phase,
            Phase::HostStopped
        );
    }
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
    fn exact_fresh_configured_enrollment_can_complete_without_overwriting_import() {
        let home = tempfile::tempdir().unwrap();
        let actor = "ab".repeat(32);
        let relay = "wss://relay.example";
        let revision = "cd".repeat(32);
        let operation = uuid_for_test();
        record_host_enrollment(
            home.path(),
            Instance::Dev,
            &actor,
            relay,
            &operation,
            &revision,
            None,
        )
        .unwrap();
        let initial = for_pair(home.path(), Instance::Dev, &actor, relay)
            .unwrap()
            .unwrap();
        let target = CodingSessionTarget {
            driver: "codex-acp".into(),
            instance_id: "primary".into(),
            session_id: uuid_for_test(),
            generation: 1,
        };
        assert!(record_host_enrollment(
            home.path(),
            Instance::Dev,
            &actor,
            relay,
            "00000000-0000-0000-0000-000000000002",
            &revision,
            Some(target.clone())
        )
        .is_err());
        assert!(record_host_enrollment(
            home.path(),
            Instance::Dev,
            &actor,
            relay,
            &operation,
            &"ef".repeat(32),
            Some(target.clone())
        )
        .is_err());
        assert_eq!(
            for_pair(home.path(), Instance::Dev, &actor, relay)
                .unwrap()
                .unwrap(),
            initial
        );
        record_host_enrollment(
            home.path(),
            Instance::Dev,
            &actor,
            relay,
            &operation,
            &revision,
            Some(target.clone()),
        )
        .unwrap();
        record_host_enrollment(
            home.path(),
            Instance::Dev,
            &actor,
            relay,
            &operation,
            &revision,
            Some(target),
        )
        .unwrap();
        assert_eq!(
            for_pair(home.path(), Instance::Dev, &actor, relay)
                .unwrap()
                .unwrap()
                .phase,
            Phase::Adopted
        );
        let imported_home = tempfile::tempdir().unwrap();
        atomic_write::create_dir_all_restricted(&layout::host_dir(
            imported_home.path(),
            Instance::Dev,
        ))
        .unwrap();
        let mut imported = initial;
        imported.legacy_nonce = "verified-old-generation".into();
        atomic_write::atomic_write_json_restricted(
            &layout::host_dir(imported_home.path(), Instance::Dev).join("managed-ownership.json"),
            &serde_json::to_vec(&Journal {
                transfers: vec![imported],
                history: vec![],
            })
            .unwrap(),
        )
        .unwrap();
        assert!(record_host_enrollment(
            imported_home.path(),
            Instance::Dev,
            &actor,
            relay,
            &operation,
            &revision,
            Some(CodingSessionTarget {
                driver: "codex-acp".into(),
                instance_id: "primary".into(),
                session_id: uuid_for_test(),
                generation: 1
            })
        )
        .is_err());
    }
    #[test]
    fn only_exact_unstarted_fresh_enrollment_revision_can_change() {
        let home = tempfile::tempdir().unwrap();
        let actor = "ab".repeat(32);
        let relay = "wss://relay.example";
        let operation = uuid_for_test();
        let old = "cd".repeat(32);
        let new = "ef".repeat(32);
        record_host_enrollment(
            home.path(),
            Instance::Dev,
            &actor,
            relay,
            &operation,
            &old,
            None,
        )
        .unwrap();
        assert!(revise_fresh_configured_enrollment(
            home.path(),
            Instance::Dev,
            &actor,
            relay,
            "wrong",
            &old,
            &new
        )
        .is_err());
        assert!(revise_fresh_configured_enrollment(
            home.path(),
            Instance::Dev,
            &actor,
            relay,
            &operation,
            &"01".repeat(32),
            &new
        )
        .is_err());
        revise_fresh_configured_enrollment(
            home.path(),
            Instance::Dev,
            &actor,
            relay,
            &operation,
            &old,
            &new,
        )
        .unwrap();
        revise_fresh_configured_enrollment(
            home.path(),
            Instance::Dev,
            &actor,
            relay,
            &operation,
            &old,
            &new,
        )
        .unwrap();
        let target = CodingSessionTarget {
            driver: "codex-acp".into(),
            instance_id: "primary".into(),
            session_id: uuid_for_test(),
            generation: 1,
        };
        record_host_enrollment(
            home.path(),
            Instance::Dev,
            &actor,
            relay,
            &operation,
            &new,
            Some(target),
        )
        .unwrap();
        assert!(revise_fresh_configured_enrollment(
            home.path(),
            Instance::Dev,
            &actor,
            relay,
            &operation,
            &new,
            &old
        )
        .is_err());
    }
    #[test]
    fn independent_pair_updates_preserve_all_enrollments() {
        let home = tempfile::tempdir().unwrap();
        std::thread::scope(|scope| {
            let mut tasks = Vec::new();
            for index in 1u8..=16 {
                let path = home.path();
                tasks.push(scope.spawn(move || {
                    let actor = format!("{index:02x}").repeat(32);
                    record_host_enrollment(
                        path,
                        Instance::Dev,
                        &actor,
                        "wss://relay.example",
                        &uuid_for_test(),
                        &"cd".repeat(32),
                        None,
                    )
                    .unwrap();
                }));
            }
            for task in tasks {
                task.join().unwrap();
            }
        });
        assert_eq!(
            read(home.path(), Instance::Dev).unwrap().transfers.len(),
            16
        );
    }
    #[test]
    fn resume_follows_only_exact_next_generation_and_preserves_custody() {
        let home = tempfile::tempdir().unwrap();
        let actor = "ab".repeat(32);
        let relay = "wss://relay.example";
        let previous = CodingSessionTarget {
            driver: "codex-acp".into(),
            instance_id: "primary".into(),
            session_id: uuid_for_test(),
            generation: 1,
        };
        record_host_enrollment(
            home.path(),
            Instance::Dev,
            &actor,
            relay,
            &uuid_for_test(),
            &"cd".repeat(32),
            Some(previous.clone()),
        )
        .unwrap();
        let before = for_pair(home.path(), Instance::Dev, &actor, relay)
            .unwrap()
            .unwrap();
        let mut next = previous.clone();
        next.generation = 2;
        let mut wrong = next.clone();
        wrong.session_id = "foreign-session".into();
        assert!(
            follow_generation(home.path(), Instance::Dev, &actor, relay, &previous, &wrong)
                .is_err()
        );
        wrong = next.clone();
        wrong.driver = "foreign-driver".into();
        assert!(
            follow_generation(home.path(), Instance::Dev, &actor, relay, &previous, &wrong)
                .is_err()
        );
        wrong = next.clone();
        wrong.instance_id = "foreign-runtime".into();
        assert!(
            follow_generation(home.path(), Instance::Dev, &actor, relay, &previous, &wrong)
                .is_err()
        );
        wrong = next.clone();
        wrong.generation = 3;
        assert!(
            follow_generation(home.path(), Instance::Dev, &actor, relay, &previous, &wrong)
                .is_err()
        );
        assert_eq!(
            for_pair(home.path(), Instance::Dev, &actor, relay)
                .unwrap()
                .unwrap(),
            before
        );
        follow_generation(home.path(), Instance::Dev, &actor, relay, &previous, &next).unwrap();
        let mut expected = before;
        expected.target = Some(next.clone());
        assert_eq!(
            for_pair(home.path(), Instance::Dev, &actor, relay)
                .unwrap()
                .unwrap(),
            expected
        );
        assert!(read(home.path(), Instance::Dev).unwrap().history.is_empty());
        assert!(
            follow_generation(home.path(), Instance::Dev, &actor, relay, &previous, &next).is_err()
        );
        host_stopped(home.path(), Instance::Dev, &actor, relay, &next).unwrap();
        let mut later = next.clone();
        later.generation = 3;
        assert!(
            follow_generation(home.path(), Instance::Dev, &actor, relay, &next, &later).is_err()
        );
    }
    fn uuid_for_test() -> String {
        "00000000-0000-0000-0000-000000000001".into()
    }
}
