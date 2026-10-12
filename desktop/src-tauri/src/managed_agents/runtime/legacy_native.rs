//! Read-only native context provenance for the bounded legacy importer.
use super::legacy_import::{
    generation_identity_matches, read_restricted_ack, receipt_matches, selected_process_running,
    verified_live_generation, LegacyAgentGeneration,
};
use std::{path::PathBuf, process::Command};
/// Non-secret launch provenance retained in the restricted importer stage.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct LegacyNativeLaunch {
    pub runtime_command: String,
    pub runtime_home_environment: std::collections::BTreeMap<String, String>,
    pub cwd: PathBuf,
}

/// Capture actual legacy launch facts before quiescence, without retaining credentials.
pub(crate) fn inspect_legacy_launch(
    generation: &LegacyAgentGeneration,
) -> Result<LegacyNativeLaunch, String> {
    verified_live_generation(generation)?;
    let output = Command::new("/bin/ps")
        .args(["eww", "-p", &generation.pid.to_string(), "-o", "command="])
        .output()
        .map_err(|_| "legacy launch metadata unavailable")?;
    if !output.status.success() {
        return Err("legacy launch metadata unavailable".into());
    }
    let text = String::from_utf8_lossy(&output.stdout);
    if !generation_identity_matches(&text, generation) {
        return Err("legacy launch generation changed".into());
    }
    let runtime_command = selected_environment(&text, "BEEKEEPER_ACP_AGENT_COMMAND")
        .filter(|v| !v.is_empty())
        .ok_or("legacy selected runtime command missing")?
        .to_string();
    let mut runtime_home_environment = std::collections::BTreeMap::new();
    for key in ["HOME", "CODEX_HOME", "CLAUDE_CONFIG_DIR"] {
        if let Some(value) = selected_environment(&text, key) {
            let path = std::path::Path::new(value);
            if path.is_absolute() && path.is_dir() {
                runtime_home_environment.insert(key.into(), value.into());
            } else if key == "CLAUDE_CONFIG_DIR"
                && !value.is_empty()
                && beekeeper_host_core::native_artifacts::runtime_name(&runtime_command)?
                    == "claude"
            {
                return Err("legacy explicit Claude config invalid; no default fallback".into());
            }
        }
    }
    establish_live_claude_default(&runtime_command, &mut runtime_home_environment)?;
    let output = Command::new("/usr/sbin/lsof")
        .args(["-a", "-p", &generation.pid.to_string(), "-d", "cwd", "-Fn"])
        .output()
        .map_err(|_| "legacy workspace provenance unavailable")?;
    if !output.status.success() {
        return Err("legacy workspace provenance unavailable".into());
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let paths: Vec<_> = text
        .lines()
        .filter_map(|line| line.strip_prefix('n'))
        .collect();
    if paths.len() != 1 {
        return Err("legacy workspace provenance ambiguous".into());
    }
    let cwd = PathBuf::from(paths[0]);
    if !cwd.is_absolute() || !cwd.is_dir() {
        return Err("legacy workspace provenance invalid".into());
    }
    verified_live_generation(generation)?;
    Ok(LegacyNativeLaunch {
        runtime_command,
        runtime_home_environment,
        cwd,
    })
}
// c34 ACP inherits its selected generation HOME into the adapter: extra_env
// cannot override a key already present in that process. Anthropic's SDK
// sessions.py108–125 defines the unset override as that HOME/.claude. This
// conversion is live inspection only; stopped records need explicit provenance.
fn establish_live_claude_default(
    command: &str,
    environment: &mut std::collections::BTreeMap<String, String>,
) -> Result<(), String> {
    if beekeeper_host_core::native_artifacts::runtime_name(command)? != "claude"
        || environment.contains_key("CLAUDE_CONFIG_DIR")
    {
        return Ok(());
    }
    let home = environment
        .get("HOME")
        .map(PathBuf::from)
        .ok_or("legacy Claude live HOME provenance unavailable")?;
    if !home.is_absolute() || !home.is_dir() {
        return Err("legacy Claude live HOME provenance invalid".into());
    }
    let config = home.join(".claude");
    let metadata = std::fs::symlink_metadata(&config)
        .map_err(|_| "legacy Claude live default config unavailable")?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("legacy Claude live default config invalid".into());
    }
    environment.insert(
        "CLAUDE_CONFIG_DIR".into(),
        config.to_string_lossy().into_owned(),
    );
    Ok(())
}
fn selected_environment<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let marker = format!(" {key}=");
    let start = text.find(&marker)? + marker.len();
    let rest = &text[start..];
    let end = rest
        .char_indices()
        .find_map(|(index, ch)| {
            if ch != ' ' {
                return None;
            }
            let tail = &rest[index + 1..];
            let name = tail.split_once('=')?.0;
            if !name.is_empty() && name.bytes().all(|v| v.is_ascii_alphanumeric() || v == b'_') {
                Some(index)
            } else {
                None
            }
        })
        .unwrap_or(rest.len());
    Some(rest[..end].trim_end())
}

/// Resolve stopped legacy workspace from uniquely selected protected native history.
/// Requires the exact drained generation, single-worker old log provenance and
/// explicit preserved runtime configuration home; no desktop layout fallback.
pub(crate) fn resolve_stopped_legacy_workspace(
    generation: &LegacyAgentGeneration,
    runtime_command: &str,
    environment: &std::collections::BTreeMap<String, String>,
) -> Result<LegacyNativeLaunch, String> {
    let launch = LegacyNativeLaunch {
        runtime_command: runtime_command.into(),
        runtime_home_environment: environment.clone(),
        cwd: PathBuf::new(),
    };
    let log = drained_legacy_log(generation)?;
    let context = legacy_context_inner(&log, generation, &launch, true)?;
    let cwd = context
        .get("sessions")
        .and_then(|v| v.as_array())
        .and_then(|v| v.first())
        .and_then(|v| v.get("cwd"))
        .and_then(|v| v.as_str())
        .map(PathBuf::from)
        .ok_or("selected legacy workspace unavailable")?;
    Ok(LegacyNativeLaunch { cwd, ..launch })
}

/// Validate supported source context before signalling the verified old owner.
/// This is capability/provenance preflight, not a drained snapshot or permission
/// to start a replacement. Final export still requires the exact drain ACK.
pub(crate) fn preflight_legacy_native_context(
    generation: &LegacyAgentGeneration,
    launch: &LegacyNativeLaunch,
) -> Result<serde_json::Value, String> {
    verified_live_generation(generation)?;
    let log = selected_legacy_log(generation)?;
    let context = legacy_context_from_log(&log, generation, launch)?;
    // Validate the timestamp contract too, before any source owner is stopped.
    // An active writer changing the selected bytes is a retryable preflight
    // refusal; only the after-drain capture can become an import snapshot.
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let session = context
        .get("sessions")
        .and_then(|v| v.as_array())
        .and_then(|v| v.first())
        .ok_or("native preflight session unavailable")?;
    let runtime = session
        .get("runtimeName")
        .and_then(|v| v.as_str())
        .ok_or("native preflight runtime unavailable")?;
    let cursor = session
        .get("resumeCursor")
        .and_then(|v| v.as_str())
        .ok_or("native preflight cursor unavailable")?;
    let artifacts: Vec<beekeeper_host_core::native_artifacts::NativeArtifact> =
        serde_json::from_value(
            session
                .get("nativeArtifacts")
                .cloned()
                .ok_or("native preflight artifacts unavailable")?,
        )
        .map_err(|_| "native preflight descriptors malformed")?;
    for artifact in artifacts {
        let mut bytes = Vec::new();
        std::fs::File::open(&artifact.source_path)
            .map_err(|_| "native preflight source unavailable")?
            .take(artifact.bytes.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|_| "native preflight source unreadable")?;
        if bytes.len() as u64 != artifact.bytes
            || hex::encode(Sha256::digest(&bytes)) != artifact.sha256
        {
            return Err(
                "native source changed during preflight; retry after its safe boundary".into(),
            );
        }
        beekeeper_host_core::native_artifacts::source_timestamp(
            runtime,
            &bytes,
            cursor,
            &launch.cwd,
        )?;
    }

    verified_live_generation(generation)?;
    Ok(context)
}

/// Read pre-export harness context after exact drain; never reconstruct missing thread IDs.
pub(crate) fn read_legacy_native_context(
    generation: &LegacyAgentGeneration,
    launch: &LegacyNativeLaunch,
) -> Result<serde_json::Value, String> {
    let log = drained_legacy_log(generation)?;
    legacy_context_from_log(&log, generation, launch)
}
fn drained_legacy_log(generation: &LegacyAgentGeneration) -> Result<String, String> {
    if selected_process_running(generation.pid)? {
        return Err("legacy native history requires verified drain first".into());
    }
    let ack = read_restricted_ack(
        &generation
            .log_path
            .with_file_name(format!("quiesced-{}.json", generation.start_nonce)),
    )?;
    if !receipt_matches(&ack, generation) {
        return Err("legacy native history lacks matching drained custody".into());
    }
    selected_legacy_log(generation)
}
fn selected_legacy_log(generation: &LegacyAgentGeneration) -> Result<String, String> {
    let metadata = std::fs::symlink_metadata(&generation.log_path)
        .map_err(|_| "legacy native log unavailable")?;
    if !metadata.file_type().is_file() || metadata.len() > 32 * 1024 * 1024 {
        return Err("legacy native log is not a bounded regular file".into());
    }
    let log = std::fs::read_to_string(&generation.log_path)
        .map_err(|_| "legacy native log unreadable")?;
    Ok(log)
}
fn legacy_context_from_log(
    log: &str,
    generation: &LegacyAgentGeneration,
    launch: &LegacyNativeLaunch,
) -> Result<serde_json::Value, String> {
    legacy_context_inner(log, generation, launch, false)
}
fn legacy_context_inner(
    log: &str,
    generation: &LegacyAgentGeneration,
    launch: &LegacyNativeLaunch,
    resolve_workspace: bool,
) -> Result<serde_json::Value, String> {
    if log.matches("beekeeper-acp starting: ").count() != 1 {
        return Err("legacy log generations are ambiguous without a nonce export; select protected source evidence explicitly".into());
    }
    let segment = log
        .rsplit_once("beekeeper-acp starting: ")
        .map(|(_, segment)| segment)
        .ok_or("legacy startup provenance unavailable")?;
    let header = segment
        .lines()
        .next()
        .ok_or("legacy startup provenance missing")?;
    if !header.contains(&format!(" pubkey={} ", generation.key.pubkey))
        || !header.contains(&format!("agent_cmd={} ", launch.runtime_command))
        || !header
            .split_whitespace()
            .find_map(|v| v.strip_prefix("relay="))
            .is_some_and(|relay| {
                beekeeper_host_core::record::canonical_relay_key(relay) == generation.key.relay_url
            })
    {
        return Err("legacy startup actor/community/runtime provenance mismatch".into());
    }
    if header
        .split_whitespace()
        .find_map(|value| value.strip_prefix("agents="))
        != Some("1")
    {
        return Err("legacy worker provenance is not uniquely single-worker; explicit context selection required".into());
    }
    let mut sessions = std::collections::BTreeMap::new();
    for line in segment.lines() {
        let Some((_, native)) = line.split_once("created session ") else {
            continue;
        };
        let Some((cursor, tail)) = native.split_once(" for channel ") else {
            return Err("legacy native log malformed".into());
        };
        let Some((channel, scope)) = tail.split_once(" (scope ") else {
            return Err("legacy native scope unavailable".into());
        };
        uuid::Uuid::parse_str(cursor).map_err(|_| "legacy native cursor malformed")?;
        uuid::Uuid::parse_str(channel).map_err(|_| "legacy native channel malformed")?;
        if !scope.starts_with("conversation)") {
            return Err(
                "legacy native thread identity is truncated; explicit context repair required"
                    .into(),
            );
        }
        sessions.insert((channel.to_string(), cursor.to_string()), ());
    }
    if sessions.len() != 1 {
        return Err(
            "legacy native context missing or ambiguous; explicit selection required".into(),
        );
    }
    let ((channel, cursor), _) = sessions
        .into_iter()
        .next()
        .ok_or("legacy native context missing")?;
    let (artifacts, cwd) = if resolve_workspace {
        beekeeper_host_core::native_artifacts::resolve_workspace(
            &launch.runtime_command,
            &launch.runtime_home_environment,
            &cursor,
        )?
    } else {
        (
            beekeeper_host_core::native_artifacts::resolve(
                &launch.runtime_command,
                &launch.runtime_home_environment,
                &cursor,
                &launch.cwd,
            )?,
            launch.cwd.clone(),
        )
    };
    let runtime_name =
        beekeeper_host_core::native_artifacts::runtime_name(&launch.runtime_command)?;
    let config_key = match runtime_name {
        "claude" => "CLAUDE_CONFIG_DIR",
        _ => "CODEX_HOME",
    };
    let native_home = launch.runtime_home_environment.get(config_key);
    Ok(
        serde_json::json!({"schema":"beekeeper-legacy-native-context/v1","actorPubkey":generation.key.pubkey,"relayUrl":generation.key.relay_url,"harnessPid":generation.pid,"startNonce":generation.start_nonce,"sessions":[{"workerIndex":0,"channelId":channel,"rootEventId":null,"resumeCursor":cursor,"runtimeName":runtime_name,"runtimeCommand":launch.runtime_command,"cwd":cwd,"sourceHome":launch.runtime_home_environment.get("HOME"),"runtimeHomeEnvironment":launch.runtime_home_environment,"nativeConfigHome":native_home,"nativeArtifacts":artifacts}]}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::managed_agents::ManagedAgentRuntimeKey;
    #[test]
    fn old_log_requires_exact_native_artifact_and_preserves_cursor() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().join("workspace");
        std::fs::create_dir(&cwd).unwrap();
        let home = dir.path().join("codex");
        let day = home.join("sessions/2026/10/11");
        std::fs::create_dir_all(&day).unwrap();
        let cursor = "11111111-1111-4111-8111-111111111111";
        let channel = "22222222-2222-4222-8222-222222222222";
        let path = day.join(format!("rollout-2026-10-11-{cursor}.jsonl"));
        std::fs::write(
            &path,
            format!(
                "{}\n",
                serde_json::json!({"type":"session_meta","payload":{"id":cursor,"cwd":cwd}})
            ),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let generation = LegacyAgentGeneration {
            key: ManagedAgentRuntimeKey::new("ab".repeat(32), "wss://relay.example").unwrap(),
            pid: 17,
            start_nonce: "nonce".into(),
            desktop_instance_id: "test".into(),
            log_path: dir.path().join("test.log"),
        };
        let launch = LegacyNativeLaunch {
            runtime_command: "codex-acp".into(),
            runtime_home_environment: std::collections::BTreeMap::from([
                ("HOME".into(), dir.path().to_string_lossy().into_owned()),
                ("CODEX_HOME".into(), home.to_string_lossy().into_owned()),
            ]),
            cwd,
        };
        let log=format!("beekeeper-acp starting: relay=wss://relay.example pubkey={} agent_cmd=codex-acp args agents=1\ncreated session {cursor} for channel {channel} (scope conversation)\n",generation.key.pubkey);
        let value = legacy_context_from_log(&log, &generation, &launch).unwrap();
        assert_eq!(value["sessions"][0]["resumeCursor"], cursor);
        let mut stopped_launch = launch.clone();
        stopped_launch.cwd = PathBuf::new();
        let offline = legacy_context_inner(&log, &generation, &stopped_launch, true).unwrap();
        assert_eq!(
            offline["sessions"][0]["cwd"],
            launch
                .cwd
                .canonicalize()
                .unwrap()
                .to_string_lossy()
                .to_string()
        );
        assert_eq!(offline["sessions"][0]["resumeCursor"], cursor);
        assert_eq!(
            value["sessions"][0]["nativeArtifacts"][0]["sourcePath"],
            path.canonicalize().unwrap().to_string_lossy().to_string()
        );
        assert_eq!(value["sessions"][0]["rootEventId"], serde_json::Value::Null);
        std::fs::remove_file(path).unwrap();
        assert!(legacy_context_from_log(&log, &generation, &launch).is_err());
    }
    #[test]
    fn selected_nonsecret_environment_preserves_spaces_and_excludes_other_values() {
        let text="harness HOME=/source home CODEX_HOME=/source home/codex SECRET=private BEEKEEPER_ACP_AGENT_COMMAND=/runtime/codex-acp";
        assert_eq!(selected_environment(text, "HOME"), Some("/source home"));
        assert_eq!(
            selected_environment(text, "CODEX_HOME"),
            Some("/source home/codex")
        );
        assert_eq!(
            selected_environment(text, "BEEKEEPER_ACP_AGENT_COMMAND"),
            Some("/runtime/codex-acp")
        );
    }
    #[test]
    fn old_log_refuses_wrong_actor_and_truncated_or_ambiguous_native_scope() {
        let generation = LegacyAgentGeneration {
            key: ManagedAgentRuntimeKey::new("ab".repeat(32), "wss://relay.example").unwrap(),
            pid: 17,
            start_nonce: "nonce".into(),
            desktop_instance_id: "test".into(),
            log_path: "/tmp/test.log".into(),
        };
        let launch = LegacyNativeLaunch {
            runtime_command: "codex-acp".into(),
            runtime_home_environment: Default::default(),
            cwd: "/tmp".into(),
        };
        let header = format!(
            "beekeeper-acp starting: relay=wss://relay.example pubkey={} agent_cmd=codex-acp args agents=1
",
            generation.key.pubkey
        );
        let scope="created session 11111111-1111-4111-8111-111111111111 for channel 22222222-2222-4222-8222-222222222222 (scope thread:abcdef01)
";
        assert!(
            legacy_context_from_log(&(header.clone() + scope), &generation, &launch)
                .unwrap_err()
                .contains("truncated")
        );
        assert!(legacy_context_from_log(
            &header.replace(&generation.key.pubkey, &"cd".repeat(32)),
            &generation,
            &launch
        )
        .unwrap_err()
        .contains("provenance mismatch"));
        assert!(legacy_context_from_log(&header, &generation, &launch)
            .unwrap_err()
            .contains("missing or ambiguous"));
    }
}

#[cfg(test)]
mod claude_import_tests {
    use super::*;
    #[test]
    fn live_claude_default_uses_only_verified_selected_home() {
        let selected = tempfile::tempdir().unwrap();
        let config = selected.path().join(".claude");
        std::fs::create_dir(&config).unwrap();
        let mut environment = std::collections::BTreeMap::from([(
            "HOME".into(),
            selected.path().to_str().unwrap().into(),
        )]);
        establish_live_claude_default("claude-agent-acp", &mut environment).unwrap();
        assert_eq!(environment["CLAUDE_CONFIG_DIR"], config.to_str().unwrap());
        environment.insert(
            "CLAUDE_CONFIG_DIR".into(),
            "/explicit-selected-config".into(),
        );
        establish_live_claude_default("claude-agent-acp", &mut environment).unwrap();
        assert_eq!(
            environment["CLAUDE_CONFIG_DIR"],
            "/explicit-selected-config"
        );
        assert!(
            establish_live_claude_default("claude-agent-acp", &mut Default::default()).is_err()
        );
        let mut codex = std::collections::BTreeMap::new();
        establish_live_claude_default("codex-acp", &mut codex).unwrap();
        assert!(!codex.contains_key("CLAUDE_CONFIG_DIR"));
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn unsupported_or_wrong_workspace_preflight_never_signals_verified_old_owner() {
        struct OwnedChild(std::process::Child);
        impl Drop for OwnedChild {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let root = tempfile::tempdir().unwrap();
        let cwd = root.path().join("workspace");
        let wrong = root.path().join("wrong");
        std::fs::create_dir(&cwd).unwrap();
        std::fs::create_dir(&wrong).unwrap();
        let cwd = cwd.canonicalize().unwrap();
        let home = root.path().join("claude");
        let key: String = cwd
            .to_str()
            .unwrap()
            .chars()
            .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
            .collect();
        let project = home.join("projects").join(key);
        std::fs::create_dir_all(&project).unwrap();
        let cursor = "11111111-1111-4111-8111-111111111111";
        let channel = "22222222-2222-4222-8222-222222222222";
        let keys = nostr::Keys::generate();
        let row = serde_json::json!({"type":"user","sessionId":cursor,"cwd":cwd,"timestamp":"2026-10-11T01:02:03Z","uuid":"fixture","message":{"role":"user","content":"fixture"}});
        beekeeper_host_core::atomic_write::atomic_write_json_restricted(
            &project.join(format!("{cursor}.jsonl")),
            format!("{row}\n").as_bytes(),
        )
        .unwrap();
        let log_path = root.path().join("legacy.log");
        let ack = log_path.with_file_name("quiesced-preflight.json");
        let child = std::process::Command::new("/bin/sleep")
            .arg("60")
            .env_clear()
            .env("BEEKEEPER_PRIVATE_KEY", keys.secret_key().to_secret_hex())
            .env("BEEKEEPER_RELAY_URL", "wss://relay.example")
            .env("BEEKEEPER_MANAGED_AGENT", "preflight-test")
            .env("BEEKEEPER_MANAGED_AGENT_START_NONCE", "preflight")
            .env("BEEKEEPER_MANAGED_AGENT_QUIESCE_RECEIPT", &ack)
            .spawn()
            .unwrap();
        let mut child = OwnedChild(child);
        let generation = LegacyAgentGeneration {
            key: crate::managed_agents::ManagedAgentRuntimeKey::new(
                keys.public_key().to_hex(),
                "wss://relay.example",
            )
            .unwrap(),
            pid: child.0.id(),
            start_nonce: "preflight".into(),
            desktop_instance_id: "preflight-test".into(),
            log_path,
        };
        for (command, workspace) in [
            ("unknown-agent-acp", cwd.clone()),
            ("claude-agent-acp", wrong),
        ] {
            let log=format!("beekeeper-acp starting: relay=wss://relay.example pubkey={} agent_cmd={command} args agents=1\ncreated session {cursor} for channel {channel} (scope conversation)\n",generation.key.pubkey);
            std::fs::write(&generation.log_path, log).unwrap();
            let launch = LegacyNativeLaunch {
                runtime_command: command.into(),
                runtime_home_environment: std::collections::BTreeMap::from([
                    ("HOME".into(), root.path().to_str().unwrap().into()),
                    ("CLAUDE_CONFIG_DIR".into(), home.to_str().unwrap().into()),
                ]),
                cwd: workspace,
            };
            assert!(preflight_legacy_native_context(&generation, &launch).is_err());
            assert!(child.0.try_wait().unwrap().is_none());
            assert!(!ack.exists());
        }
    }
    #[test]
    fn claude_preflight_source_parser_selects_actual_vendor_before_any_drain() {
        let root = tempfile::tempdir().unwrap();
        let cwd = root.path().join("work");
        std::fs::create_dir(&cwd).unwrap();
        let cwd = cwd.canonicalize().unwrap();
        let home = root.path().join("claude");
        let key: String = cwd
            .to_str()
            .unwrap()
            .chars()
            .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
            .collect();
        let project = home.join("projects").join(key);
        std::fs::create_dir_all(&project).unwrap();
        let cursor = "11111111-1111-4111-8111-111111111111";
        let channel = "22222222-2222-4222-8222-222222222222";
        let file = project.join(format!("{cursor}.jsonl"));
        let row = serde_json::json!({"type":"user","sessionId":cursor,"cwd":cwd,"timestamp":"2026-10-11T01:02:03Z","uuid":"fixture","message":{"role":"user","content":"fixture"}});
        beekeeper_host_core::atomic_write::atomic_write_json_restricted(
            &file,
            format!("{row}\n").as_bytes(),
        )
        .unwrap();
        let generation = LegacyAgentGeneration {
            key: crate::managed_agents::ManagedAgentRuntimeKey::new(
                "ab".repeat(32),
                "wss://relay.example",
            )
            .unwrap(),
            pid: 17,
            start_nonce: "nonce".into(),
            desktop_instance_id: "fixture".into(),
            log_path: root.path().join("old.log"),
        };
        let launch = LegacyNativeLaunch {
            runtime_command: "claude-agent-acp".into(),
            runtime_home_environment: std::collections::BTreeMap::from([
                ("HOME".into(), root.path().to_str().unwrap().into()),
                ("CLAUDE_CONFIG_DIR".into(), home.to_str().unwrap().into()),
                ("CODEX_HOME".into(), root.path().to_str().unwrap().into()),
            ]),
            cwd,
        };
        let log=format!("beekeeper-acp starting: relay=wss://relay.example pubkey={} agent_cmd=claude-agent-acp args agents=1\ncreated session {cursor} for channel {channel} (scope conversation)\n",generation.key.pubkey);
        let value = legacy_context_from_log(&log, &generation, &launch).unwrap();
        assert_eq!(value["sessions"][0]["runtimeName"], "claude");
        assert_eq!(
            value["sessions"][0]["nativeConfigHome"],
            home.to_str().unwrap()
        );
        let mut unsupported = launch;
        unsupported
            .runtime_home_environment
            .remove("CLAUDE_CONFIG_DIR");
        assert!(legacy_context_from_log(&log, &generation, &unsupported).is_err());
        // The source parser has no signalling side effect and no ACK dependency;
        // the public preflight wraps it with live-generation verification.
        assert!(!generation
            .log_path
            .with_file_name("quiesced-nonce.json")
            .exists());
    }
}
