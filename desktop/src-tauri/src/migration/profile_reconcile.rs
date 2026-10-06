//! The durable profile-reconcile queue: kind:0 profiles a migration renamed
//! locally and still owes the relay.
//!
//! A boot migration runs before any relay is known and cannot publish. So a
//! rename writes `(pubkey, expected_name)` here first, the agent store second,
//! and `spawn_pending_profile_reconciliations` drains it on every workspace
//! apply — when a relay actually exists. **That order is load-bearing**: the
//! drain executes an entry only while `expected_name == record.name`, so a
//! crash between the two writes leaves an inert entry that starts working on
//! the boot that finishes the rename. The reverse order would leave renamed
//! records with no queued publish, permanently.
//!
//! Completion is recorded per relay and the entry is kept afterwards, because
//! Desktop does not persist its community list in Rust: a community that is
//! inactive today, or re-added later, must still get its one repair.
//!
//! This began inside the Bumble→Pollen rename. It is infrastructure, not part
//! of that migration's content rules, and the per-project naming migration
//! (ledger 246) is the second consumer that proved it.

use std::path::Path;

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub(crate) struct ProfileReconcileQueueEntry {
    pub(crate) pubkey: String,
    #[serde(default = "default_profile_reconcile_name")]
    pub(crate) expected_name: String,
    /// Canonical relay identities already repaired for this migrated agent.
    ///
    /// Keep the entry after success: Desktop does not persist its community
    /// list in Rust, so a community that is inactive (or re-added later) must
    /// still get one repair when it is next applied.
    #[serde(default)]
    pub(crate) reconciled_relays: Vec<String>,
}

fn default_profile_reconcile_name() -> String {
    crate::managed_agents::POLLEN_DISPLAY_NAME.to_string()
}

#[derive(serde::Deserialize)]
struct CurrentProfileReconcileQueueEntry {
    pubkey: String,
    #[serde(default = "default_profile_reconcile_name")]
    expected_name: String,
    #[serde(default)]
    reconciled_relays: Vec<String>,
}

#[derive(serde::Deserialize)]
#[serde(untagged)]
enum StoredProfileReconcileQueueEntry {
    Current(CurrentProfileReconcileQueueEntry),
    Legacy(String),
}

impl<'de> serde::Deserialize<'de> for ProfileReconcileQueueEntry {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        match StoredProfileReconcileQueueEntry::deserialize(deserializer)? {
            StoredProfileReconcileQueueEntry::Current(entry) => Ok(Self {
                pubkey: entry.pubkey,
                expected_name: entry.expected_name,
                reconciled_relays: entry.reconciled_relays,
            }),
            StoredProfileReconcileQueueEntry::Legacy(pubkey) => Ok(Self {
                pubkey,
                expected_name: default_profile_reconcile_name(),
                reconciled_relays: Vec::new(),
            }),
        }
    }
}

pub(crate) fn profile_reconcile_queue_path(agent_store_path: &Path) -> std::path::PathBuf {
    agent_store_path.with_file_name("profile-reconcile-pending.json")
}

pub(crate) fn persist_profile_reconcile_queue(
    path: &Path,
    reconciliations: &[(String, String)],
) -> Result<(), String> {
    let queue_path = profile_reconcile_queue_path(path);
    let mut pending = if queue_path.exists() {
        read_profile_reconcile_queue(&queue_path).unwrap_or_default()
    } else {
        Vec::new()
    };
    for (pubkey, expected_name) in reconciliations {
        if let Some(entry) = pending.iter_mut().find(|entry| entry.pubkey == *pubkey) {
            entry.expected_name.clone_from(expected_name);
            entry.reconciled_relays.clear();
        } else {
            pending.push(ProfileReconcileQueueEntry {
                pubkey: pubkey.clone(),
                expected_name: expected_name.clone(),
                reconciled_relays: Vec::new(),
            });
        }
    }
    pending.sort_by(|left, right| left.pubkey.cmp(&right.pubkey));
    write_profile_reconcile_queue(&queue_path, &pending)
}

pub(crate) const PROFILE_RECONCILE_QUEUE_MAX_BYTES: usize = 1024 * 1024;

pub(crate) fn read_profile_reconcile_queue(
    path: &Path,
) -> Result<Vec<ProfileReconcileQueueEntry>, String> {
    let metadata = std::fs::metadata(path)
        .map_err(|error| format!("failed to inspect profile reconcile queue: {error}"))?;
    if metadata.len() > PROFILE_RECONCILE_QUEUE_MAX_BYTES as u64 {
        return Err("profile reconcile queue exceeds its size limit".to_string());
    }
    let contents = std::fs::read_to_string(path)
        .map_err(|error| format!("failed to read profile reconcile queue: {error}"))?;
    serde_json::from_str(&contents)
        .map_err(|error| format!("failed to parse profile reconcile queue: {error}"))
}

pub(crate) fn write_profile_reconcile_queue(
    path: &Path,
    entries: &[ProfileReconcileQueueEntry],
) -> Result<(), String> {
    if entries.is_empty() {
        return match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!(
                "failed to remove empty profile reconcile queue {}: {error}",
                path.display()
            )),
        };
    }
    let bytes = serde_json::to_vec_pretty(entries)
        .map_err(|error| format!("failed to serialize profile reconcile queue: {error}"))?;
    if bytes.len() > PROFILE_RECONCILE_QUEUE_MAX_BYTES {
        return Err("profile reconcile queue exceeds its size limit".to_string());
    }
    crate::managed_agents::atomic_write_json_restricted(path, &bytes)
}

pub(crate) fn profile_reconcile_relay_key(relay_url: &str) -> Result<String, String> {
    beekeeper_core_pkg::relay::normalize_relay_url(relay_url)
        .map_err(|error| format!("invalid profile reconcile relay: {error}"))
}

#[cfg(test)]
pub(crate) fn profile_reconcile_is_pending(
    entries: &[ProfileReconcileQueueEntry],
    pubkey: &str,
    relay_key: &str,
) -> bool {
    entries.iter().any(|entry| {
        entry.pubkey == pubkey
            && !entry
                .reconciled_relays
                .iter()
                .any(|relay| relay == relay_key)
    })
}

pub(crate) fn record_profile_reconciled(
    entries: &mut [ProfileReconcileQueueEntry],
    pubkey: &str,
    relay_key: String,
) {
    if let Some(entry) = entries.iter_mut().find(|entry| entry.pubkey == pubkey) {
        if !entry.reconciled_relays.contains(&relay_key) {
            entry.reconciled_relays.push(relay_key);
            entry.reconciled_relays.sort();
        }
    }
}
