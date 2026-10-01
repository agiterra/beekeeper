//! Building and launching one provider process.
//!
//! Everything about the environment comes from
//! `beekeeper_host_core::env`, which the desktop app uses too — that is the
//! whole point of sharing it. What this module adds is host-specific: the
//! marker that identifies a host-owned child, and its own process group so
//! signal escalation can reach the per-session adapters.

use std::path::{Path, PathBuf};

use beekeeper_host_core::config::HostConfig;
use beekeeper_host_core::env::{build_provider_env, ProviderEnvInputs, INHERITED_KEYS_TO_CLEAR};
use beekeeper_host_core::layout::CHILD_MARKER_VAR;
use beekeeper_host_core::logs::{append_log_marker, now_iso, open_log_file};
use beekeeper_host_core::record::CodingSessionProviderRecord;

use crate::discovery::{augmented_path, resolve_command};

/// Binary name of the provider.
pub const PROVIDER_BINARY: &str = "buzz-session-provider";
/// ACP adapter the provider spawns for each coding session.
const PROVIDER_AGENT_BINARY: &str = "claude-agent-acp";
/// Read-only context sidecar the provider hands to verified sessions.
const CONTEXT_MCP_BINARY: &str = "buzz-dev-mcp";
/// The CLI the ACP adapter drives.
const CLAUDE_CLI_BINARY: &str = "claude";

/// Resolve the provider binary, saying how to get one when it is missing.
pub fn resolve_provider_binary(config: &HostConfig) -> Result<PathBuf, String> {
    if let Some(explicit) = &config.provider_command {
        return resolve_command(&explicit.to_string_lossy()).ok_or_else(|| {
            format!(
                "the provider binary named in host.json is not executable: {}",
                explicit.display()
            )
        });
    }
    resolve_command(PROVIDER_BINARY).ok_or_else(|| {
        format!(
            "{PROVIDER_BINARY} was not found beside this host, in its workspace, or on PATH — \
             build it with `cargo build -p buzz-session-provider`, or name it in host.json"
        )
    })
}

/// Resolve the Claude Code CLI the adapter should drive.
///
/// The provider passes `CLAUDE_CODE_EXECUTABLE` down into every
/// `claude-agent-acp` it spawns, so resolving it here keeps coding sessions on
/// the binary this machine's Beekeeper manages rather than whatever the
/// adapter's own lookup finds.
fn resolve_claude_code_executable() -> Option<PathBuf> {
    // Found once, kept for the host's lifetime — the rule `login_shell_path`
    // already follows: every provider (re)start used to run the login-shell
    // probe again (ledger 302(g)). Only a hit is kept, so a Claude Code
    // installed after the host started is still found at the next start.
    static FOUND: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    if let Some(path) = FOUND.get() {
        return Some(path.clone());
    }
    let path = resolve_command(CLAUDE_CLI_BINARY)?;
    if beekeeper_host_core::path_env::should_skip_claude_executable(&path, cfg!(windows)) {
        return None;
    }
    Some(FOUND.get_or_init(|| path).clone())
}

/// Launch one provider process, logging the attempt to the provider's own log.
pub fn spawn_provider_child(
    binary: &Path,
    config: &HostConfig,
    record: &CodingSessionProviderRecord,
    nsec: &str,
    log_path: &Path,
) -> Result<std::process::Child, String> {
    let _ = append_log_marker(
        log_path,
        &format!(
            "=== starting coding-session provider {} under agent host pid {} at {} ===",
            record.provider_pubkey,
            std::process::id(),
            now_iso()
        ),
    );
    let stdout = open_log_file(log_path)?;
    let stderr = stdout
        .try_clone()
        .map_err(|error| format!("failed to clone the provider log handle: {error}"))?;

    let mut command = std::process::Command::new(binary);
    command.stdin(std::process::Stdio::null());
    command.stdout(std::process::Stdio::from(stdout));
    command.stderr(std::process::Stdio::from(stderr));
    for key in INHERITED_KEYS_TO_CLEAR {
        command.env_remove(key);
    }

    // The key reaches the child through its environment and nowhere else. The
    // record's own copy is deliberately replaced rather than trusted: the host
    // resolved and *checked* the key against this record, and the record may
    // carry no inline copy at all.
    let mut keyed = record.clone();
    keyed.private_key_nsec = nsec.to_string();

    let env = build_provider_env(&ProviderEnvInputs {
        record: &keyed,
        relay_url: &config.relay_url,
        state_dir: &config.provider_state_dir,
        agent_command: resolve_command(PROVIDER_AGENT_BINARY),
        context_mcp_command: resolve_command(CONTEXT_MCP_BINARY),
        claude_code_executable: resolve_claude_code_executable(),
        runtimes: config.runtimes.clone(),
        max_sessions: config.max_sessions,
        turn_idle_timeout_secs: config.turn_idle_timeout_secs,
        turn_budget: config.turn_budget,
        augmented_path: augmented_path(),
        // A headless host has no repository checkout of its own to fence off.
        // Absent rather than empty: an empty list would read as "the host
        // looked and found nothing shared", a different claim from "there is
        // nothing to look at".
        app_checkout: None,
        rust_log: std::env::var("RUST_LOG").ok().filter(|v| !v.is_empty()),
        emit_raw_sdk_frames: matches!(
            std::env::var(beekeeper_host_core::env::EMIT_RAW_SDK_FRAMES_VAR)
                .unwrap_or_default()
                .trim()
                .to_ascii_lowercase()
                .as_str(),
            "1" | "true" | "yes" | "on"
        ),
    });
    for (key, value) in env {
        command.env(key, value);
    }

    // The marker that says this child has an owner.
    //
    // The desktop's untracked-harness sweep kills any process whose exe path
    // matches a bundled harness and which is not in *its* tracked set
    // (`managed_agents::runtime::sweep`). Before the host existed, "not mine"
    // and "nobody's" were the same set, so a desktop boot would be within its
    // rights to kill a host-owned child. This is how the child says otherwise.
    command.env(CHILD_MARKER_VAR, std::process::id().to_string());

    // Own process group so escalation can reach per-session adapter children.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    command
        .spawn()
        .map_err(|error| format!("failed to spawn {PROVIDER_BINARY}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use beekeeper_host_core::layout::Instance;

    fn config_for(state_dir: &Path) -> HostConfig {
        HostConfig {
            version: beekeeper_host_core::config::HOST_CONFIG_VERSION,
            instance: Instance::Production,
            relay_url: "wss://hive.example.org".to_string(),
            provider_pubkey: "d".repeat(64),
            session_provider_base_dir: state_dir.parent().expect("parent").to_path_buf(),
            provider_state_dir: state_dir.to_path_buf(),
            runtimes: Vec::new(),
            max_sessions: None,
            turn_idle_timeout_secs: None,
            turn_budget: None,
            provider_command: None,
            written_at: now_iso(),
        }
    }

    /// A provider binary named in `host.json` that is not there must fail by
    /// name at resolve time, not as a spawn error five restarts later.
    #[test]
    fn a_named_provider_binary_that_is_absent_is_refused_with_its_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut config = config_for(&dir.path().join("state"));
        config.provider_command = Some(dir.path().join("nowhere/buzz-session-provider"));
        let error = resolve_provider_binary(&config).expect_err("must be refused");
        assert!(error.contains("nowhere/buzz-session-provider"), "{error}");
    }

    /// The child must carry the marker, and it must be this host's pid — a
    /// stale value inherited from somewhere else would make the desktop spare
    /// a process no host owns.
    #[test]
    fn the_child_marker_names_this_hosts_pid() {
        // Proven through the same path the spawn uses: a real child, told to
        // print the variable back.
        let dir = tempfile::tempdir().expect("tempdir");
        let log = dir.path().join("provider.log");
        let Some(shell) = resolve_command("sh") else {
            return; // No POSIX shell: nothing to prove this against.
        };
        let record = CodingSessionProviderRecord {
            provider_pubkey: "d".repeat(64),
            instance_id: "d".repeat(16),
            auth_tag: None,
            created_at: now_iso(),
            relay_url: "wss://hive.example.org".to_string(),
            private_key_nsec: String::new(),
        };
        let script = dir.path().join("fake-provider");
        std::fs::write(
            &script,
            format!("#!/bin/sh\necho \"marker=${CHILD_MARKER_VAR}\"\n"),
        )
        .expect("write");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
                .expect("chmod");
        }
        let _ = shell;
        let mut child = spawn_provider_child(
            &script,
            &config_for(&dir.path().join("state")),
            &record,
            "nsec1fake",
            &log,
        )
        .expect("spawn");
        let _ = child.wait();
        let logged = std::fs::read_to_string(&log).expect("log");
        assert!(
            logged.contains(&format!("marker={}", std::process::id())),
            "the child must carry this host's pid: {logged}"
        );
        assert!(
            logged.contains("under agent host pid"),
            "the log must say which host started it: {logged}"
        );
    }
}
