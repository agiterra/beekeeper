//! Shared remote-provider discovery, immutable protocol negotiation and deployment.
//! Clients supply custody, resolved payloads and an optional working directory.

use sha2::{Digest, Sha256};
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

const STDERR_CAP: usize = 65536;
/// Provider responses should be small JSON objects. Cap stdout to prevent a
/// buggy or malicious provider from OOM-ing the desktop process.
const STDOUT_CAP: usize = 1_048_576; // 1 MB
const PROVIDER_PROTOCOL_VERSION: u64 = 1;

/// Validate the exact supported provider info response before secret-bearing deploy.
pub fn validate_provider_info(info: &serde_json::Value) -> Result<(), String> {
    let object = info
        .as_object()
        .ok_or_else(|| "provider info response must be a JSON object".to_string())?;
    let actual_version = object
        .get("protocol_version")
        .and_then(serde_json::Value::as_u64);
    if actual_version != Some(PROVIDER_PROTOCOL_VERSION) {
        return Err(match actual_version {
            Some(version) => format!(
                "unsupported provider protocol version {version}; host requires {PROVIDER_PROTOCOL_VERSION}"
            ),
            None => "provider info response missing integer protocol_version".to_string(),
        });
    }
    if object.get("ok") != Some(&serde_json::Value::Bool(true)) {
        return Err("provider info response must contain ok: true".to_string());
    }
    for field in ["name", "version", "description"] {
        if object
            .get(field)
            .is_none_or(|value| value.as_str().is_none_or(str::is_empty))
        {
            return Err(format!(
                "provider info response missing non-empty string {field}"
            ));
        }
    }
    if !object
        .get("config_schema")
        .is_some_and(serde_json::Value::is_object)
    {
        return Err("provider info response missing object config_schema".to_string());
    }

    const FIELDS: &[&str] = &[
        "ok",
        "name",
        "version",
        "protocol_version",
        "description",
        "config_schema",
    ];
    if let Some(field) = object
        .keys()
        .find(|field| !FIELDS.contains(&field.as_str()))
    {
        return Err(format!(
            "provider info response contains unknown field {field}"
        ));
    }
    Ok(())
}

/// Invoke a provider binary: write JSON to stdin, read JSON from stdout.
///
/// Reader threads stream lines/chunks over channels so the caller can receive
/// data as it arrives and time-box the wait. No `read_to_end` — if a provider
/// daemonizes or leaves descendants holding pipes open, the caller still gets
/// all data written before the child exited and returns without leaking threads
/// (the readers drop naturally when the sender is gone and the pipe closes or
/// the desktop process exits).
pub fn invoke_provider(
    binary: &Path,
    request: &serde_json::Value,
    timeout: Duration,
) -> Result<serde_json::Value, String> {
    invoke_provider_at(binary, request, timeout, None)
}

/// Invoke the existing provider protocol from an explicit client working directory.
pub fn invoke_provider_at(
    binary: &Path,
    request: &serde_json::Value,
    timeout: Duration,
    workdir: Option<&Path>,
) -> Result<serde_json::Value, String> {
    let request_bytes = format!(
        "{}\n",
        serde_json::to_string(request).map_err(|e| e.to_string())?
    );

    let mut cmd = std::process::Command::new(binary);
    // The explicit deploy payload carries scoped custody. Never forward the
    // commissioning client's master signing credentials into a backend process.
    for key in [
        "BEEKEEPER_PRIVATE_KEY",
        "BUZZ_PRIVATE_KEY",
        "BEEKEEPER_ACP_PRIVATE_KEY",
        "BUZZ_ACP_PRIVATE_KEY",
        "BEEKEEPER_AUTH_TAG",
        "BUZZ_AUTH_TAG",
    ] {
        cmd.env_remove(key);
    }
    if let Some(home) = workdir {
        cmd.current_dir(home);
    }
    configure_no_window(&mut cmd);
    let mut child = cmd
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to spawn {}: {e}", binary.display()))?;

    // Write request and close stdin immediately so the provider sees EOF.
    let stdin_result = if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(request_bytes.as_bytes())
    } else {
        Ok(())
    };

    // Stream stdout as raw chunks over a channel. The caller appends chunks
    // to a buffer and attempts incremental JSON parsing — no dependency on
    // newlines or EOF. If a descendant holds the pipe open after the provider
    // exits, the thread blocks on the next read — but the caller already has
    // the response data and proceeds. The thread is not joined; it terminates
    // when the pipe eventually closes or the process exits.
    let (stdout_tx, stdout_rx) = mpsc::channel::<Vec<u8>>();
    if let Some(stdout) = child.stdout.take() {
        std::thread::spawn(move || {
            let mut buf = vec![0u8; 8192];
            let mut reader = BufReader::new(stdout);
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if stdout_tx.send(buf[..n].to_vec()).is_err() {
                            break; // receiver dropped
                        }
                    }
                    Err(_) => break,
                }
            }
        });
    }

    // Drain stderr into a bounded channel. sync_channel(8) caps in-flight
    // chunks — the producer blocks when the buffer is full, applying natural
    // backpressure. The consumer drains during the try_wait loop and caps
    // total bytes at STDERR_CAP, so memory is bounded even for long-running
    // or malicious providers.
    let (stderr_tx, stderr_rx) = mpsc::sync_channel::<Vec<u8>>(8);
    if let Some(stderr) = child.stderr.take() {
        std::thread::spawn(move || {
            let mut buf = vec![0u8; 8192];
            let mut reader = BufReader::new(stderr);
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if stderr_tx.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });
    }

    // Bail early if stdin write failed — child may be in a bad state.
    if let Err(e) = stdin_result {
        let _ = child.kill();
        let _ = child.wait();
        return Err(format!("stdin write failed: {e}"));
    }

    // Poll try_wait with a deadline, collecting stdout chunks and draining
    // stderr as data arrives. Incremental JSON parsing on stdout means we
    // capture the response even without a trailing newline or EOF.
    let timeout_secs = timeout.as_secs();
    let deadline = std::time::Instant::now() + timeout;
    let mut stdout_buf = Vec::new();
    let mut stderr_bytes = Vec::new();
    let exit_status = loop {
        // Drain stdout chunks (non-blocking), enforce byte cap.
        while stdout_buf.len() < STDOUT_CAP {
            match stdout_rx.try_recv() {
                Ok(chunk) => stdout_buf.extend_from_slice(&chunk),
                Err(_) => break,
            }
        }
        // Drain stderr chunks (non-blocking), enforce byte cap.
        while stderr_bytes.len() < STDERR_CAP {
            match stderr_rx.try_recv() {
                Ok(chunk) => stderr_bytes.extend_from_slice(&chunk),
                Err(_) => break,
            }
        }

        match child.try_wait() {
            Ok(Some(status)) => {
                break status;
            }
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("provider timed out after {timeout_secs}s"));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("wait error: {e}"));
            }
        }
    };

    // Drain remaining stdout chunks buffered between last poll and child exit.
    // Keep draining until the channel disconnects (reader finished) or the
    // 2s deadline expires (descendant holding pipe open). Do NOT break on the
    // first timeout — a slightly delayed final chunk should still be captured.
    let drain_deadline = std::time::Instant::now() + Duration::from_secs(2);
    loop {
        if stdout_buf.len() >= STDOUT_CAP {
            break;
        }
        let remaining = drain_deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        match stdout_rx.recv_timeout(remaining.min(Duration::from_millis(100))) {
            Ok(chunk) => stdout_buf.extend_from_slice(&chunk),
            Err(mpsc::RecvTimeoutError::Disconnected) => break, // reader done
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // Keep waiting until the full drain deadline expires.
                if std::time::Instant::now() >= drain_deadline {
                    break;
                }
            }
        }
    }

    // Final stderr drain (non-blocking, cap already enforced).
    while stderr_bytes.len() < STDERR_CAP {
        match stderr_rx.try_recv() {
            Ok(chunk) => stderr_bytes.extend_from_slice(&chunk),
            Err(_) => break,
        }
    }
    stderr_bytes.truncate(STDERR_CAP);
    stdout_buf.truncate(STDOUT_CAP);

    let stderr = String::from_utf8_lossy(&stderr_bytes);
    let env_secrets = env_secrets_from_request(request);
    let env_secret_refs: Vec<&str> = env_secrets.iter().map(String::as_str).collect();
    let stderr_redacted = redact_secrets_with(&stderr, &env_secret_refs);

    let exit_info = exit_status
        .code()
        .map(|c| format!("exit code {c}"))
        .unwrap_or_else(|| "killed by signal".to_string());

    // Fail on non-zero exit regardless of stdout content. A provider that
    // crashes mid-deploy may flush partial JSON before dying — trusting that
    // output would be worse than surfacing the failure.
    let exited_ok = exit_status.success();
    if !exited_ok {
        let stderr_snippet = diagnostic_prefix(&stderr_redacted, 4096);
        if stderr_snippet.is_empty() {
            return Err(format!("provider failed ({exit_info}, empty stderr)"));
        } else {
            return Err(format!(
                "provider failed ({exit_info}). stderr: {stderr_snippet}"
            ));
        }
    }

    // Incremental JSON parse: try each line, then try the entire buffer.
    // Handles providers that emit JSON on a single line (common) as well as
    // providers that write JSON without a trailing newline.
    let stdout_str = String::from_utf8_lossy(&stdout_buf);
    let response: serde_json::Value = stdout_str
        .lines()
        .find_map(|line| serde_json::from_str(line).ok())
        .or_else(|| serde_json::from_str(stdout_str.trim()).ok())
        .ok_or_else(|| {
            let stderr_snippet = diagnostic_prefix(&stderr_redacted, 4096);
            if stderr_snippet.is_empty() {
                format!("provider produced no JSON response ({exit_info}, empty stderr)")
            } else {
                format!(
                    "provider produced no JSON response ({exit_info}). stderr: {stderr_snippet}"
                )
            }
        })?;

    if response.get("ok").and_then(|v| v.as_bool()) == Some(false) {
        let error = response["error"].as_str().unwrap_or("unknown error");
        return Err(redact_secrets_with(error, &env_secret_refs));
    }

    Ok(response)
}

/// Split a config key into lowercase words on `_`, `-`, `.`, and camelCase boundaries.
///
/// Handles acronyms: consecutive uppercase runs stay together until a lowercase follows.
/// "apiKey" → ["api", "key"], "apiKEY" → ["api", "key"], "APIKey" → ["api", "key"],
/// "access_token" → ["access", "token"], "keyboard" → ["keyboard"],
/// "clientSecret" → ["client", "secret"].
fn split_config_key(key: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = key.chars().collect();
    for (i, &ch) in chars.iter().enumerate() {
        if ch == '_' || ch == '-' || ch == '.' {
            if !current.is_empty() {
                words.push(current.to_lowercase());
                current.clear();
            }
        } else if ch.is_uppercase() {
            // Start a new word on: (a) transition from lowercase to uppercase, or
            // (b) uppercase followed by lowercase (end of acronym run, e.g. "APIKey" → "API" + "Key").
            let prev_lower =
                !current.is_empty() && current.chars().last().is_some_and(|c| c.is_lowercase());
            let acronym_end = !current.is_empty()
                && current.chars().last().is_some_and(|c| c.is_uppercase())
                && chars.get(i + 1).is_some_and(|c| c.is_lowercase());
            if prev_lower || acronym_end {
                words.push(current.to_lowercase());
                current.clear();
            }
            current.push(ch);
        } else {
            current.push(ch);
        }
    }
    if !current.is_empty() {
        words.push(current.to_lowercase());
    }
    words
}

#[cfg(test)]
fn redact_secrets(s: &str) -> String {
    redact_secrets_with(s, &[])
}

/// Like the (test-only) prefix-only `redact_secrets`, but also redacts
/// every occurrence of each
/// `extras` entry verbatim. Used to scrub user-supplied env values out of
/// provider stderr/JSON-error text — providers may echo their request
/// back in failure messages, and persona/agent `env_vars` may carry API
/// keys that the desktop just persisted via `last_error`.
///
/// Entries shorter than 4 chars are skipped: too noisy to scrub blindly
/// (would match every short token in normal log output). Entries are
/// applied in decreasing length order so superstrings get scrubbed before
/// substrings — protects against partial overlap leaks.
pub fn redact_secrets_with(s: &str, extras: &[&str]) -> String {
    let mut result = s.to_string();

    // Extras: longest first to avoid partial-overlap leaks. We use
    // `str::replace` (single-pass, non-overlapping) instead of a `find` +
    // `replace_range` loop — the loop variant would never terminate if a
    // user-supplied env value happened to be a substring of the
    // replacement marker (e.g. value="REDACTED" or "EDACTE").
    let mut sorted: Vec<&str> = extras.iter().copied().filter(|v| v.len() >= 4).collect();
    sorted.sort_by_key(|v| std::cmp::Reverse(v.len()));
    sorted.dedup();
    for value in sorted {
        if !value.is_empty() {
            result = result.replace(value, "[REDACTED]");
        }
    }

    // Then prefix-based scrubbing. This loop *can* re-scan because each
    // replacement shortens the buffer past the matched prefix — the
    // replacement marker `[REDACTED]` contains none of these prefixes, so
    // progress is guaranteed. Any prefix added here must preserve that.
    //
    // GitHub tokens are recognised by shape as well as by variable name: a
    // token reaches output from outside our environment too — embedded in a
    // git remote URL an installer echoes, say — where no name-based rule can
    // see it.
    for prefix in &[
        "nsec1",
        "sprt_tok_",
        "ghp_",
        "gho_",
        "ghu_",
        "ghs_",
        "ghr_",
        "github_pat_",
    ] {
        while let Some(pos) = result.find(prefix) {
            let end = result[pos..]
                .find(|c: char| c.is_whitespace() || c == '"' || c == '\'')
                .map(|i| pos + i)
                .unwrap_or(result.len());
            result.replace_range(pos..end, "[REDACTED]");
        }
    }
    result
}

/// Collect string values from every environment map a deploy request can
/// carry. Providers may echo any of these values in diagnostics, including
/// definition/baked values that exist only in the resolved launch block.
fn env_secrets_from_request(request: &serde_json::Value) -> Vec<String> {
    let agent = request.get("agent");
    let maps = [
        agent.and_then(|value| value.get("env_vars")),
        agent
            .and_then(|value| value.get("launch"))
            .and_then(|value| value.get("env")),
        agent
            .and_then(|value| value.get("launch"))
            .and_then(|value| value.get("policy_env")),
    ];

    let mut values: Vec<String> = maps
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_object)
        .flat_map(|map| map.values())
        .filter_map(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .map(String::from)
        .collect();
    if let Some(key) = agent
        .and_then(|v| v.get("private_key_nsec"))
        .and_then(serde_json::Value::as_str)
    {
        values.push(key.into());
        if let Ok(parsed) = nostr::SecretKey::parse(key) {
            use nostr::ToBech32;
            values.extend([
                parsed.to_secret_hex(),
                parsed.to_secret_hex().to_uppercase(),
            ]);
            let Ok(encoded) = parsed.to_bech32();
            values.push(encoded);
        }
    }
    values
}

/// Public-in-crate helper: redact every non-empty value from `env` (plus
/// the standard nsec/sprt_tok prefix scrubbing) out of `s`. Used by
/// callers that already have a flat env map handy — e.g. model discovery
/// formatting child stderr into a frontend-visible error.
pub fn redact_env_values_in(s: &str, env: &std::collections::BTreeMap<String, String>) -> String {
    let values: Vec<&str> = env
        .values()
        .filter(|v| !v.is_empty())
        .map(String::as_str)
        .collect();
    redact_secrets_with(s, &values)
}

/// Copy a resolved provider into a private staging directory while hashing
/// exactly the bytes copied. The staged file becomes non-writable before either
/// invocation, closing the path replacement and in-place rewrite races.
fn stage_provider(
    binary: &Path,
) -> Result<(tempfile::TempDir, PathBuf, String, std::fs::File), String> {
    let directory = tempfile::Builder::new()
        .prefix("buzz-provider-")
        .tempdir()
        .map_err(|error| format!("failed to create provider staging directory: {error}"))?;
    let suffix = if cfg!(windows) { ".exe" } else { "" };
    let staged_path = directory.path().join(format!("provider{suffix}"));
    let mut source = std::fs::File::open(binary)
        .map_err(|error| format!("failed to open provider for staging: {error}"))?;
    let mut staged = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&staged_path)
        .map_err(|error| format!("failed to create staged provider: {error}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = source
            .read(&mut buffer)
            .map_err(|error| format!("failed to read provider for staging: {error}"))?;
        if count == 0 {
            break;
        }
        staged
            .write_all(&buffer[..count])
            .map_err(|error| format!("failed to write staged provider: {error}"))?;
        hasher.update(&buffer[..count]);
    }
    staged
        .sync_all()
        .map_err(|error| format!("failed to sync staged provider: {error}"))?;

    let mut permissions = staged
        .metadata()
        .map_err(|error| format!("failed to inspect staged provider: {error}"))?
        .permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(0o500);
    }
    #[cfg(not(unix))]
    permissions.set_readonly(true);
    std::fs::set_permissions(&staged_path, permissions)
        .map_err(|error| format!("failed to protect staged provider: {error}"))?;
    drop(staged);

    #[cfg(windows)]
    let execution_guard = {
        use std::os::windows::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .read(true)
            // Permit CreateProcess to read the image while denying replacement,
            // writes, and deletion until both invocations finish.
            .share_mode(windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ)
            .open(&staged_path)
    };
    #[cfg(not(windows))]
    let execution_guard = std::fs::File::open(&staged_path);
    let execution_guard = execution_guard
        .map_err(|error| format!("failed to lock staged provider for execution: {error}"))?;
    Ok((
        directory,
        staged_path,
        hex::encode(hasher.finalize()),
        execution_guard,
    ))
}

/// Deploy through one immutable staged copy: negotiate protocol v1 before the
/// secret-bearing request, then invoke deploy on those exact same bytes.
pub fn provider_deploy(
    binary: &Path,
    agent: &serde_json::Value,
    provider_config: &serde_json::Value,
) -> Result<String, String> {
    provider_deploy_at(binary, agent, provider_config, None)
}

/// Deploy through the immutable protocol boundary with explicit client workdir.
pub fn provider_deploy_at(
    binary: &Path,
    agent: &serde_json::Value,
    provider_config: &serde_json::Value,
    workdir: Option<&Path>,
) -> Result<String, String> {
    let (_directory, staged, _digest, _execution_guard) = stage_provider(binary)?;
    let info_request = serde_json::json!({
        "op": "info",
        "request_id": uuid::Uuid::new_v4().to_string(),
    });
    let info = invoke_provider_at(&staged, &info_request, Duration::from_secs(10), workdir)?;
    validate_provider_info(&info)?;

    let request = serde_json::json!({
        "op": "deploy",
        "request_id": uuid::Uuid::new_v4().to_string(),
        "agent": agent,
        "provider_config": provider_config,
    });
    let resp = invoke_provider_at(&staged, &request, Duration::from_secs(600), workdir)?;
    resp["agent_id"]
        .as_str()
        .map(String::from)
        .ok_or_else(|| "deploy response missing agent_id".to_string())
}

/// Validate provider_config: flat object, scalar values, no secret-like keys.
pub fn validate_provider_config(config: &serde_json::Value) -> Result<(), String> {
    let obj = config
        .as_object()
        .ok_or("provider_config must be a JSON object")?;
    if obj.len() > 20 {
        return Err("provider_config: max 20 fields".to_string());
    }
    let json_str = serde_json::to_string(config).unwrap_or_default();
    if json_str.len() > 65536 {
        return Err("provider_config: max 64KB".to_string());
    }
    // Split on separators AND camelCase boundaries, then check each word.
    // Catches: api_key, apiKey, access-token, clientSecret, etc.
    // Allows: keyboard, monkey_wrench (no forbidden word as a segment).
    let forbidden = ["secret", "password", "token", "key", "credential"];
    for (k, v) in obj {
        let words = split_config_key(k);
        for f in &forbidden {
            if words.iter().any(|w| w == f) {
                return Err(format!("provider_config: key '{}' looks like a secret", k));
            }
        }
        if v.is_object() || v.is_array() {
            return Err(format!(
                "provider_config: value for '{}' must be a scalar",
                k
            ));
        }
    }
    Ok(())
}

/// Derive a provider id from the filename Tauri stages at runtime. Tauri
/// removes its target-triple suffix while copying an external binary, but on
/// Windows leaves the executable/script extension, which is not part of the
/// provider id.
///
/// Both the current `beekeeper-backend-` prefix and the pre-rename
/// `buzz-backend-` are accepted: a provider a user installed before the rename
/// (`~/.local/bin/buzz-backend-<id>`) is still theirs.
fn provider_id_from_filename(name: &str) -> Option<&str> {
    let raw = PROVIDER_PREFIXES
        .iter()
        .find_map(|prefix| name.strip_prefix(prefix))?;
    let id = [".exe", ".bat", ".cmd"]
        .into_iter()
        .find_map(|extension| {
            raw.get(raw.len().saturating_sub(extension.len())..)
                .filter(|suffix| suffix.eq_ignore_ascii_case(extension))
                .map(|_| &raw[..raw.len() - extension.len()])
        })
        .unwrap_or(raw);

    (!id.is_empty()).then_some(id)
}

/// Provider executable filename prefixes, current first.
pub const PROVIDER_PREFIXES: [&str; 2] = ["beekeeper-backend-", "buzz-backend-"];

/// Pick one binary per provider id from executable `(filename, path)` entries
/// in search order: the first current-prefix binary when there is one,
/// otherwise the first legacy one. A stale `buzz-backend-kubernetes` left in
/// `~/.local/bin` cannot shadow the bundled `beekeeper-backend-kubernetes`.
pub fn select_provider_candidates(
    entries: impl IntoIterator<Item = (String, PathBuf)>,
) -> Vec<(String, PathBuf)> {
    let mut results: Vec<(String, PathBuf, bool)> = Vec::new();
    for (name, path) in entries {
        let Some(id) = provider_id_from_filename(&name) else {
            continue;
        };
        let legacy = !name.starts_with(PROVIDER_PREFIXES[0]);
        match results.iter_mut().find(|(found, _, _)| found == id) {
            Some(existing) if existing.2 && !legacy => *existing = (id.to_string(), path, false),
            Some(_) => {}
            None => results.push((id.to_string(), path, legacy)),
        }
    }
    results
        .into_iter()
        .map(|(id, path, _)| (id, path))
        .collect()
}

/// Enumerate PATH for `beekeeper-backend-*` (and legacy `buzz-backend-*`)
/// executables. Returns (id, path) pairs.
/// Only includes files that are executable. Does NOT execute any binaries.
///
/// On macOS, GUI apps inherit a minimal PATH from launchd (`/usr/bin:/bin:/usr/sbin:/sbin`)
/// which excludes both the app bundle's `Contents/MacOS/` dir and `~/.local/bin`.
/// We augment the search with those directories so bundled and user-installed providers
/// are always discovered regardless of how the desktop was launched.
pub fn discover_provider_candidates() -> Vec<(String, PathBuf)> {
    let mut seen = std::collections::HashSet::new();
    let mut entries_in_order = Vec::new();

    let path_var = std::env::var_os("PATH").unwrap_or_default();
    let mut dirs: Vec<PathBuf> = std::env::split_paths(&path_var).collect();

    // Prepend the exe parent dir (Contents/MacOS/ in a .app bundle) so bundled
    // providers are found even when the process PATH is minimal.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let parent_buf = parent.to_path_buf();
            if !dirs.contains(&parent_buf) {
                dirs.insert(0, parent_buf);
            }
        }
    }

    // Also include ~/.local/bin — the conventional location for user-installed
    // provider binaries (symlinks created by install scripts).
    if let Some(home) = dirs::home_dir() {
        let local_bin = home.join(".local").join("bin");
        if !dirs.contains(&local_bin) {
            dirs.push(local_bin);
        }
    }

    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if provider_id_from_filename(&name).is_some()
                && !seen.contains(&name)
                && is_executable(&entry.path())
            {
                seen.insert(name.clone());
                entries_in_order.push((name, entry.path()));
            }
        }
    }
    select_provider_candidates(entries_in_order)
}

/// Resolve a provider ID to a discovered, executable binary path.
///
/// This is the ONLY way to resolve provider binaries for execution. It:
/// 1. Validates the ID against `^[a-z0-9][a-z0-9_-]*$` (no path traversal)
/// 2. Looks up the ID in `discover_provider_candidates()` (PATH-discovered only)
/// 3. Returns the canonical path of the discovered binary
///
/// All deploy, start, and create paths MUST use this instead of raw
/// `resolve_command(format!("beekeeper-backend-{id}"))` to prevent a compromised
/// frontend/IPC caller from steering execution to an arbitrary binary.
pub fn resolve_provider_binary(provider_id: &str) -> Result<PathBuf, String> {
    // Reject IDs that could be path components or shell metacharacters.
    let valid_id = provider_id
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
        && !provider_id.is_empty()
        && provider_id.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit());
    if !valid_id {
        return Err(format!(
            "invalid provider ID '{provider_id}': must match [a-z0-9][a-z0-9_-]*"
        ));
    }

    let candidates = discover_provider_candidates();
    let found = candidates
        .into_iter()
        .find(|(id, _)| id == provider_id)
        .map(|(_, path)| path);

    match found {
        Some(path) => path
            .canonicalize()
            .map_err(|e| format!("provider binary not accessible: {e}")),
        None => Err(format!(
            "provider 'beekeeper-backend-{provider_id}' not found on PATH"
        )),
    }
}

/// Check if a file is executable (Unix: mode bits; other platforms: always true).
fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        true
    }
}

/// Public discovery projection; contains no provider credential material.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendProviderInfo {
    /// Discovered provider identifier.
    pub id: String,
    /// Resolved executable path on the executor.
    pub binary_path: String,
}

/// Existing one-shot replay floor policy key.
pub const REPLAY_FLOOR_ENV_VAR: &str = "BEEKEEPER_ACP_REPLAY_FLOOR";

fn relay_http_base_url(relay: &str) -> String {
    let value = relay.trim().trim_end_matches('/');
    if let Some(rest) = value.strip_prefix("wss://") {
        return format!("https://{rest}");
    }
    if let Some(rest) = value.strip_prefix("ws://") {
        return format!("http://{rest}");
    }
    value.to_string()
}
fn assert_expected_relay_scope(expected: Option<&str>, actual: &str) -> Result<(), String> {
    if expected
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .is_some_and(|v| relay_http_base_url(v) != relay_http_base_url(actual))
    {
        return Err("active community changed before the message was submitted; not sent".into());
    }
    Ok(())
}
fn assert_expected_signer(expected: Option<&str>, actual: &str) -> Result<(), String> {
    if expected
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .is_some_and(|v| !v.eq_ignore_ascii_case(actual))
    {
        return Err("active identity changed before the message was submitted; not sent".into());
    }
    Ok(())
}

/// Check caller-captured relay and signer against the rebuilt provider payload.
/// Scoped callers fail closed when either payload field is unavailable.
pub fn assert_payload_scope(
    agent_json: &serde_json::Value,
    expected_relay_url: Option<&str>,
    expected_signer_pubkey: Option<&str>,
) -> Result<(), String> {
    let has_expectation = |value: Option<&str>| {
        value
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_some()
    };
    match agent_json
        .get("relay_url")
        .and_then(serde_json::Value::as_str)
    {
        Some(relay) => assert_expected_relay_scope(expected_relay_url, relay)?,
        None if has_expectation(expected_relay_url) => {
            return Err("deploy payload carries no relay; not deployed".to_string());
        }
        None => {}
    }
    match agent_json
        .get("launch")
        .and_then(|launch| launch.get("owner_pubkey"))
        .and_then(serde_json::Value::as_str)
    {
        Some(owner) => assert_expected_signer(expected_signer_pubkey, owner)?,
        None if has_expectation(expected_signer_pubkey) => {
            return Err("deploy payload carries no owner identity; not deployed".to_string());
        }
        None => {}
    }
    Ok(())
}

/// Inject a one-shot publish timestamp into the exact provider launch payload.
/// A persisted `launch.env` floor is removed because it would override
/// `policy_env` in provider launch precedence.
pub fn apply_replay_floor(agent_json: &mut serde_json::Value, replay_floor_unix: Option<u64>) {
    let Some(floor) = replay_floor_unix else {
        return;
    };
    let Some(launch) = agent_json
        .get_mut("launch")
        .and_then(serde_json::Value::as_object_mut)
    else {
        return;
    };
    if let Some(env) = launch
        .get_mut("env")
        .and_then(serde_json::Value::as_object_mut)
    {
        let keys: Vec<String> = env
            .keys()
            .filter(|key| key.eq_ignore_ascii_case(REPLAY_FLOOR_ENV_VAR))
            .cloned()
            .collect();
        for key in keys {
            env.remove(&key);
        }
    }
    match launch
        .get_mut("policy_env")
        .and_then(serde_json::Value::as_object_mut)
    {
        Some(policy_env) => {
            policy_env.insert(
                REPLAY_FLOOR_ENV_VAR.to_string(),
                serde_json::Value::String(floor.to_string()),
            );
        }
        None => {
            launch.insert(
                "policy_env".to_string(),
                serde_json::json!({ (REPLAY_FLOOR_ENV_VAR): floor.to_string() }),
            );
        }
    }
}

/// Serialize the portable launch contract after the shared catalog resolved policy.
/// Environment compatibility mirrors preserve existing deployed provider images.
pub fn launch_payload(
    command: &str,
    args: &[String],
    env: std::collections::BTreeMap<String, String>,
    policy_env: std::collections::BTreeMap<String, String>,
    owner: &str,
) -> serde_json::Value {
    serde_json::json!({"command": wire_command_name(command), "args": args,
        "env": beekeeper_core::env_compat::mirror_map(env),
        "policy_env": beekeeper_core::env_compat::mirror_map(policy_env), "owner_pubkey": owner})
}

/// Existing provider wire command compatibility; explicit custom paths stay exact.
pub fn wire_command_name(command: &str) -> &str {
    match command {
        "beekeeper-acp" => "buzz-acp",
        "beekeeper-agent" => "buzz-agent",
        "beekeeper-dev-mcp" => "buzz-dev-mcp",
        other => other,
    }
}

/// Resolved private deployment inputs. Never publish this projection as agent metadata.
pub struct ProviderDeployPayload<'a> {
    /// Public instance display name.
    pub name: &'a str,
    /// Scoped destination relay.
    pub relay_url: String,
    /// Scoped agent key, not the owner's signing key.
    pub private_key_nsec: &'a str,
    /// Existing owner attestation.
    pub auth_tag: Option<&'a str>,
    /// Configured adapter command.
    pub agent_command: &'a str,
    /// Configured adapter arguments.
    pub agent_args: &'a [String],
    /// Resolved executable instructions.
    pub system_prompt: Option<String>,
    /// Resolved model choice.
    pub model: Option<String>,
    /// Resolved model provider.
    pub provider: Option<String>,
    /// Existing legacy timeout projection.
    pub turn_timeout_seconds: u64,
    /// Idle scope timeout.
    pub idle_timeout_seconds: Option<u64>,
    /// Maximum turn duration.
    pub max_turn_duration_seconds: Option<u64>,
    /// Catalog-constrained concurrency.
    pub parallelism: u32,
    /// Compiled-policy-projected author gate.
    pub respond_to: &'a str,
    /// Compiled-policy-projected operator allowlist.
    pub respond_to_allowlist: &'a [String],
    /// Resolved private runtime environment.
    pub env_vars: std::collections::BTreeMap<String, String>,
    /// Portable launch block from the same descriptor.
    pub launch: serde_json::Value,
}

/// Serialize the existing exact remote deploy schema, keeping custody executor-private.
pub fn deploy_payload(p: ProviderDeployPayload<'_>) -> serde_json::Value {
    serde_json::json!({"name":p.name,"relay_url":p.relay_url,"private_key_nsec":p.private_key_nsec,
        "auth_tag":p.auth_tag,"agent_command":p.agent_command,"agent_args":p.agent_args,
        "system_prompt":p.system_prompt,"model":p.model,"provider":p.provider,
        "turn_timeout_seconds":p.turn_timeout_seconds,"idle_timeout_seconds":p.idle_timeout_seconds,
        "max_turn_duration_seconds":p.max_turn_duration_seconds,"parallelism":p.parallelism,
        "respond_to":p.respond_to,"respond_to_allowlist":p.respond_to_allowlist,
        "env_vars":p.env_vars,"launch":p.launch})
}

fn diagnostic_prefix(value: &str, cap: usize) -> &str {
    let mut end = value.len().min(cap);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

fn configure_no_window(command: &mut std::process::Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        command.creation_flags(0x0800_0000);
    }
    #[cfg(not(windows))]
    let _ = command;
}

#[cfg(test)]
#[path = "provider_boundary_tests.rs"]
mod tests;
