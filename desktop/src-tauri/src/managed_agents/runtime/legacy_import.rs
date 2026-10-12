//! One-way upgrade capability. This module cannot launch or supervise an executor.
use super::super::ManagedAgentRuntimeKey;
use std::{
    path::PathBuf,
    process::Command,
    time::{Duration, Instant},
};

/// Exact legacy generation supplied by the restricted migration manifest.
#[derive(Debug, Clone)]
pub(crate) struct LegacyAgentGeneration {
    pub key: ManagedAgentRuntimeKey,
    pub pid: u32,
    pub start_nonce: String,
    pub desktop_instance_id: String,
    pub log_path: PathBuf,
}

pub(super) fn selected_process_running(pid: u32) -> Result<bool, String> {
    let output = Command::new("/bin/ps")
        .args(["-p", &pid.to_string(), "-o", "stat="])
        .output()
        .map_err(|_| "selected process state could not be established")?;
    let state = String::from_utf8_lossy(&output.stdout);
    if output.status.success() && !state.trim().is_empty() {
        return Ok(!state.trim().starts_with('Z'));
    }
    if output.status.code() == Some(1) && output.stdout.is_empty() && output.stderr.is_empty() {
        return Ok(false);
    }
    Err("selected process state is uncertain; import remains blocked".into())
}

pub(crate) fn process_is_running(pid: u32) -> bool {
    // Uncertainty never certifies a stopped owner.
    !matches!(selected_process_running(pid), Ok(false))
}

/// Inspect only the selected legacy PID; never return its captured credentials.
pub(crate) fn inspect_legacy_generation(
    key: &ManagedAgentRuntimeKey,
    pid: u32,
    desktop_instance_id: &str,
    log_path: PathBuf,
) -> Result<LegacyAgentGeneration, String> {
    if pid == 0 || desktop_instance_id.is_empty() || !log_path.is_absolute() {
        return Err(
            "legacy import requires a selected PID, desktop identity and absolute log path".into(),
        );
    }

    if !cfg!(unix) {
        return Err("verified legacy generation inspection is unavailable on this platform".into());
    }
    if selected_process_running(pid)? {
        let output = Command::new("/bin/ps")
            .args(["eww", "-p", &pid.to_string(), "-o", "command="])
            .output()
            .map_err(|e| e.to_string())?;
        let text = String::from_utf8_lossy(&output.stdout);
        let nonce = text
            .split_whitespace()
            .find_map(|token| token.strip_prefix("BEEKEEPER_MANAGED_AGENT_START_NONCE="))
            .filter(|nonce| valid_nonce(nonce))
            .ok_or("legacy generation has no verifiable nonce; import requires custody repair")?;
        let generation = LegacyAgentGeneration {
            key: key.clone(),
            pid,
            start_nonce: nonce.to_string(),
            desktop_instance_id: desktop_instance_id.to_string(),
            log_path,
        };
        if !output.status.success() || !generation_identity_matches(&text, &generation) {
            return Err(
                "selected process does not match actor, community and legacy generation custody"
                    .into(),
            );
        }
        return Ok(generation);
    }
    let parent = log_path.parent().ok_or("legacy log path has no parent")?;
    let mut found = None;
    for entry in std::fs::read_dir(parent).map_err(|_| "stopped legacy generation has no drain evidence; preserve backup and repair interrupted/native context")? {
        let entry=entry.map_err(|e|e.to_string())?;
        let name=entry.file_name();let name=name.to_string_lossy();
        let Some(nonce)=name.strip_prefix("quiesced-").and_then(|value|value.strip_suffix(".json")) else {continue;};
        let Ok(value)=read_restricted_ack(&entry.path()) else {continue;};
        let generation=LegacyAgentGeneration {key:key.clone(),pid,start_nonce:nonce.into(),desktop_instance_id:desktop_instance_id.into(),log_path:log_path.clone()};
        if receipt_matches(&value,&generation) { if found.is_some() { return Err("stopped legacy generation has ambiguous drain evidence; repair the protected manifest".into()); } found=Some(generation); }
    }
    found.ok_or_else(||"stopped legacy generation has no matching drain acknowledgment; preserve backup and repair interrupted/native context".into())
}

/// Discover only exact actor/community drained evidence when old Stop removed its PID receipt.
/// A reused live PID is never permission to signal it; ambiguous generations require repair.
pub(crate) fn inspect_stopped_legacy_generation(
    key: &ManagedAgentRuntimeKey,
    desktop_instance_id: &str,
    log_path: PathBuf,
) -> Result<LegacyAgentGeneration, String> {
    if desktop_instance_id.is_empty() || !log_path.is_absolute() {
        return Err("stopped legacy source scope unavailable".into());
    }
    let parent = log_path
        .parent()
        .ok_or("stopped legacy log has no parent")?;
    let mut found = None;
    for (index, entry) in std::fs::read_dir(parent)
        .map_err(|_| "stopped legacy drain evidence unavailable")?
        .enumerate()
    {
        if index > 10_000 {
            return Err("stopped legacy evidence exceeds bounded selection budget".into());
        }
        let entry = entry.map_err(|_| "stopped legacy evidence entry unavailable")?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Some(nonce) = name
            .strip_prefix("quiesced-")
            .and_then(|name| name.strip_suffix(".json"))
        else {
            continue;
        };
        let Ok(value) = read_restricted_ack(&entry.path()) else {
            continue;
        };
        let Some(pid) = value["harnessPid"]
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
        else {
            continue;
        };
        let generation = LegacyAgentGeneration {
            key: key.clone(),
            pid,
            start_nonce: nonce.into(),
            desktop_instance_id: desktop_instance_id.into(),
            log_path: log_path.clone(),
        };
        if !receipt_matches(&value, &generation) {
            continue;
        }
        if selected_process_running(pid)? {
            return Err("stopped source PID is live/reused; no process signalled, explicit source repair required".into());
        }
        if found.is_some() {
            return Err("stopped legacy drain generations ambiguous; select protected source evidence explicitly".into());
        }
        found = Some(generation);
    }
    found.ok_or_else(||"stopped legacy source lacks exact drained evidence; preserve backup and repair interrupted context".into())
}

pub(super) fn read_restricted_ack(path: &std::path::Path) -> Result<serde_json::Value, String> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|_| "legacy drained acknowledgment is unavailable")?;
    if !metadata.is_file() {
        return Err("legacy drained acknowledgment is not a regular custody file".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err("legacy drained acknowledgment has unsafe permissions; preserve backup and repair custody".into());
        }
    }
    serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|_| "legacy drained acknowledgment is malformed".into())
}

fn valid_nonce(nonce: &str) -> bool {
    !nonce.is_empty()
        && nonce.len() <= 128
        && nonce
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

pub(super) fn receipt_matches(
    value: &serde_json::Value,
    generation: &LegacyAgentGeneration,
) -> bool {
    valid_nonce(&generation.start_nonce)
        && generation.pid != 0
        && generation.log_path.is_absolute()
        && value["schema"] == "beekeeper-local-quiescence/v1"
        && value["actorPubkey"] == generation.key.pubkey
        && value["startNonce"] == generation.start_nonce
        && value["harnessPid"].as_u64() == Some(u64::from(generation.pid))
        && value["acceptedWorkDrained"] == true
        && value["relayUrl"].as_str().is_some_and(|relay| {
            beekeeper_host_core::record::canonical_relay_key(relay)
                == beekeeper_host_core::record::canonical_relay_key(&generation.key.relay_url)
        })
}

pub(super) fn verified_live_generation(generation: &LegacyAgentGeneration) -> Result<(), String> {
    if generation.pid == 0
        || generation.start_nonce.is_empty()
        || generation.desktop_instance_id.is_empty()
    {
        return Err("legacy import requires a verified generation, not a PID alone".into());
    }
    // Inspect only the already-selected PID. The captured environment stays local
    // and is never logged or returned; no global process scan or name-based claim.
    let output = Command::new("/bin/ps")
        .args(["eww", "-p", &generation.pid.to_string(), "-o", "command="])
        .output()
        .map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() || !generation_identity_matches(&text, generation) {
        return Err("legacy generation identity is unconfirmed; no process was signalled".into());
    }
    Ok(())
}

pub(super) fn generation_identity_matches(text: &str, generation: &LegacyAgentGeneration) -> bool {
    let tokens: Vec<_> = text.split_whitespace().collect();
    let value = |name: &str| tokens.iter().find_map(|token| token.strip_prefix(name));
    let actor = value("BEEKEEPER_PRIVATE_KEY=")
        .or_else(|| value("BUZZ_PRIVATE_KEY="))
        .and_then(|key| nostr::Keys::parse(key).ok())
        .map(|keys| keys.public_key().to_hex());
    let relay = value("BEEKEEPER_RELAY_URL=").or_else(|| value("BUZZ_RELAY_URL="));
    value("BEEKEEPER_MANAGED_AGENT=") == Some(generation.desktop_instance_id.as_str())
        && value("BEEKEEPER_MANAGED_AGENT_START_NONCE=") == Some(generation.start_nonce.as_str())
        && !tokens
            .iter()
            .any(|token| token.starts_with("BEEKEEPER_HOST_CHILD="))
        && text.contains(&format!(
            "BEEKEEPER_MANAGED_AGENT_QUIESCE_RECEIPT={}",
            generation
                .log_path
                .with_file_name(format!("quiesced-{}.json", generation.start_nonce))
                .display()
        ))
        && actor.as_deref() == Some(generation.key.pubkey.as_str())
        && relay.is_some_and(|relay| {
            beekeeper_host_core::record::canonical_relay_key(relay)
                == beekeeper_host_core::record::canonical_relay_key(&generation.key.relay_url)
        })
}

/// Drain only a verified legacy harness. Never kill a process group or escalate.
pub(crate) fn quiesce_legacy_generation(
    generation: &LegacyAgentGeneration,
) -> Result<serde_json::Value, String> {
    if !valid_nonce(&generation.start_nonce)
        || generation.pid == 0
        || !generation.log_path.is_absolute()
    {
        return Err("legacy generation manifest is invalid; no process was signalled".into());
    }

    if !cfg!(unix) {
        return Err("verified graceful legacy import is unavailable on this platform; preserve backup and repair on a supported executor".into());
    }
    let path = generation
        .log_path
        .with_file_name(format!("quiesced-{}.json", generation.start_nonce));
    if selected_process_running(generation.pid)? {
        verified_live_generation(generation)?;
        let status = Command::new("/bin/kill")
            .args(["-s", "TERM", &generation.pid.to_string()])
            .status()
            .map_err(|e| e.to_string())?;
        if !status.success() {
            return Err("legacy harness did not accept the graceful drain request".into());
        }
        let deadline = Instant::now() + Duration::from_secs(90);
        while selected_process_running(generation.pid)? {
            if Instant::now() >= deadline {
                return Err(
                    "legacy work has not drained; import remains blocked without forced cleanup"
                        .into(),
                );
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    let value = read_restricted_ack(&path)?;
    if !receipt_matches(&value, generation) {
        return Err("legacy drain acknowledgment does not match manifest custody".into());
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stale_or_foreign_acknowledgment_cannot_certify_import() {
        let generation = LegacyAgentGeneration {
            key: ManagedAgentRuntimeKey::new("ab".repeat(32), "wss://relay.example").unwrap(),
            pid: 17,
            start_nonce: "nonce-a".into(),
            desktop_instance_id: "test".into(),
            log_path: PathBuf::from("/tmp/import.log"),
        };
        let mut value = serde_json::json!({"schema":"beekeeper-local-quiescence/v1","actorPubkey":generation.key.pubkey,"relayUrl":generation.key.relay_url,"harnessPid":17,"startNonce":"nonce-a","acceptedWorkDrained":true});
        assert!(receipt_matches(&value, &generation));
        value["startNonce"] = "nonce-b".into();
        assert!(!receipt_matches(&value, &generation));
        value["startNonce"] = "nonce-a".into();
        value["acceptedWorkDrained"] = false.into();
        assert!(!receipt_matches(&value, &generation));
    }
    #[test]
    fn live_identity_requires_actor_relay_nonce_and_excludes_host_owner() {
        let keys = nostr::Keys::generate();
        let generation = LegacyAgentGeneration {
            key: ManagedAgentRuntimeKey::new(keys.public_key().to_hex(), "wss://relay.example")
                .unwrap(),
            pid: 17,
            start_nonce: "nonce-a".into(),
            desktop_instance_id: "test".into(),
            log_path: PathBuf::from("/tmp/import.log"),
        };
        let text=format!("BEEKEEPER_MANAGED_AGENT=test BEEKEEPER_MANAGED_AGENT_START_NONCE=nonce-a BEEKEEPER_PRIVATE_KEY={} BEEKEEPER_RELAY_URL=wss://relay.example BEEKEEPER_MANAGED_AGENT_QUIESCE_RECEIPT=/tmp/quiesced-nonce-a.json",keys.secret_key().to_secret_hex());
        assert!(generation_identity_matches(&text, &generation));
        assert!(!generation_identity_matches(
            &text.replace("nonce-a", "nonce-b"),
            &generation
        ));
        assert!(!generation_identity_matches(
            &text.replace("relay.example", "foreign.example"),
            &generation
        ));
        assert!(!generation_identity_matches(
            &(text.clone() + " BEEKEEPER_HOST_CHILD=dev"),
            &generation
        ));
        let mut foreign = generation;
        foreign.key.pubkey = "ab".repeat(32);
        assert!(!generation_identity_matches(&text, &foreign));
    }
    #[cfg(unix)]
    #[test]
    fn stopped_import_requires_unique_matching_drain_evidence() {
        let dir = tempfile::tempdir().unwrap();
        let key = ManagedAgentRuntimeKey::new("ab".repeat(32), "wss://relay.example").unwrap();
        let log = dir.path().join("legacy.log");
        let mut child = Command::new("/usr/bin/true").spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        assert!(inspect_legacy_generation(&key, pid, "desktop-test", log.clone()).is_err());
        let value = serde_json::json!({"schema":"beekeeper-local-quiescence/v1","actorPubkey":key.pubkey,"relayUrl":key.relay_url,"harnessPid":pid,"startNonce":"nonce-a","acceptedWorkDrained":true});
        std::fs::write(
            dir.path().join("quiesced-nonce-a.json"),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                dir.path().join("quiesced-nonce-a.json"),
                std::fs::Permissions::from_mode(0o600),
            )
            .unwrap();
        }
        let selected = inspect_legacy_generation(&key, pid, "desktop-test", log.clone()).unwrap();
        assert_eq!(selected.start_nonce, "nonce-a");
        assert_eq!(
            inspect_stopped_legacy_generation(&key, "desktop-test", log.clone())
                .unwrap()
                .start_nonce,
            "nonce-a"
        );
        assert!(quiesce_legacy_generation(&selected).is_ok());
        let mut second = value;
        second["startNonce"] = "nonce-b".into();
        std::fs::write(
            dir.path().join("quiesced-nonce-b.json"),
            serde_json::to_vec(&second).unwrap(),
        )
        .unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                dir.path().join("quiesced-nonce-b.json"),
                std::fs::Permissions::from_mode(0o600),
            )
            .unwrap();
        }
        assert!(inspect_legacy_generation(&key, pid, "desktop-test", log.clone()).is_err());
        assert!(inspect_stopped_legacy_generation(&key, "desktop-test", log).is_err());
    }
}
