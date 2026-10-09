use super::*;

/// Binary name fragments for all known agent/harness processes that Beekeeper
/// may spawn. Used by `process_belongs_to_us()` and the orphan sweep to
/// identify processes we should clean up. Both hyphenated and underscored
/// variants are listed because macOS `proc_name()` and Linux `/proc/comm`
/// may report either form depending on how the binary was built.
pub(crate) const KNOWN_AGENT_BINARIES: &[&str] = &[
    "beekeeper-acp",
    "beekeeper_acp",
    "beekeeper-agent",
    "beekeeper_agent",
    // The pre-rename binary names. A harness started by an older build is
    // still ours to reap after an upgrade, so these stay.
    "buzz-acp",
    "buzz_acp",
    "buzz-agent",
    "buzz_agent",
    "claude-agent-acp",
    "claude_agent_acp",
    "claude-code-acp",
    "claude_code_acp",
    "codex-acp",
    "codex_acp",
    "goose",
    // beekeeper-dev-mcp's multicall personalities (rg, tree, buzz,
    // git-credential-nostr, git-sign-nostr) are short-lived per-tool-call
    // invocations — not listed here.
    //
    // `beekeeper-dev-mcp` is 17 bytes, so Linux `/proc/<pid>/comm` reports it
    // as `beekeeper-dev-m` and only the `/proc/<pid>/exe` fallback in
    // `process_belongs_to_us` matches it. `beekeeper-agent` is exactly 15
    // bytes and survives the truncation.
    "beekeeper-dev-mcp",
    "beekeeper_dev_mcp",
    "buzz-dev-mcp",
    "buzz_dev_mcp",
];

/// Script interpreters that may host managed agent wrappers (e.g. npm shims).
/// A process whose name matches here is NOT immediately claimed — it must also
/// carry `BEEKEEPER_MANAGED_AGENT` in its environment (checked by the caller via
/// `process_has_beekeeper_marker()`). This avoids sweeping unrelated node processes.
pub(crate) const KNOWN_SCRIPT_INTERPRETERS: &[&str] = &["node"];

/// Check if a process name matches any of our known agent binaries.
/// Uses exact match or prefix-with-separator to avoid false positives
/// (e.g. `"goose"` must not match `"mongoose"`).
pub(super) fn name_matches_known_binary(name: &str) -> bool {
    KNOWN_AGENT_BINARIES.iter().any(|&binary| {
        name == binary || {
            name.starts_with(binary) && {
                let rest = &name[binary.len()..];
                rest.starts_with('-') || rest.starts_with('_') || rest.starts_with('.')
            }
        }
    })
}

/// Check if a process name is a known script interpreter that may be hosting
/// a managed agent wrapper (e.g. `node` running an npm shim for `codex-acp`).
/// Callers must additionally verify `BEEKEEPER_MANAGED_AGENT` ownership.
pub(super) fn name_matches_interpreter(name: &str) -> bool {
    KNOWN_SCRIPT_INTERPRETERS.contains(&name)
}

#[cfg(unix)]
pub(crate) fn process_is_running(pid: u32) -> bool {
    // Use libc::kill with signal 0 instead of forking a subprocess.
    // Returns true only if the process exists AND we can signal it.
    // Returns false for non-existent PIDs (ESRCH) and PIDs owned by
    // other users (EPERM) — callers should not interact with those.
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

#[cfg(not(unix))]
pub(crate) fn process_is_running(_pid: u32) -> bool {
    false
}

/// Check if a PID belongs to a known agent process we spawned.
/// Returns false for recycled PIDs that now belong to other processes.
#[cfg(target_os = "macos")]
pub(crate) fn process_belongs_to_us(pid: u32) -> bool {
    // Use proc_name() from libproc to get the process name without spawning
    // a subprocess.
    extern "C" {
        fn proc_name(pid: libc::c_int, buffer: *mut libc::c_void, buffersize: u32) -> libc::c_int;
    }
    let mut buf = [0u8; 1024];
    let len = unsafe {
        proc_name(
            pid as i32,
            buf.as_mut_ptr() as *mut libc::c_void,
            buf.len() as u32,
        )
    };
    if len <= 0 {
        return false;
    }
    let name = String::from_utf8_lossy(&buf[..len as usize]);
    // Fall through for script interpreters (e.g. `node` hosting an npm shim):
    // the caller's `process_has_beekeeper_marker()` check decides true ownership.
    name_matches_known_binary(&name) || name_matches_interpreter(&name)
}

#[cfg(all(unix, not(target_os = "macos")))]
pub(crate) fn process_belongs_to_us(pid: u32) -> bool {
    // First try /proc/<pid>/comm. Note: comm is truncated to 15 bytes on Linux,
    // so binaries with names longer than 15 chars (e.g. "claude-agent-acp")
    // will never match here.
    if let Ok(name) = std::fs::read_to_string(format!("/proc/{pid}/comm")) {
        if name_matches_known_binary(name.trim()) {
            return true;
        }
        // Interpreter check: `node` is 4 bytes, never truncated.
        if name_matches_interpreter(name.trim()) {
            return true;
        }
    }

    // Fallback: read /proc/<pid>/exe which is a symlink to the full binary path.
    // This is not subject to the 15-byte truncation limit.
    if let Ok(exe_path) = std::fs::read_link(format!("/proc/{pid}/exe")) {
        if let Some(basename) = exe_path.file_name().and_then(|n| n.to_str()) {
            // Fall through for script interpreters — caller checks the marker.
            return name_matches_known_binary(basename) || name_matches_interpreter(basename);
        }
    }

    false
}

#[cfg(not(unix))]
pub(crate) fn process_belongs_to_us(_pid: u32) -> bool {
    false
}

/// The value stamped into the `BEEKEEPER_MANAGED_AGENT` env var of every agent we
/// spawn, identifying *which* desktop instance owns it. We use the app's bundle
/// identifier (`io.agiterra.beekeeper.app` for release, `io.agiterra.beekeeper.app.dev`
/// for `just dev`) because it is stable across restarts — a relaunched dev
/// instance still recognizes its own previously-spawned agents as reclaimable,
/// while never matching another instance's (e.g. a dev build never reaps a DMG
/// build's agents, and vice versa). This is what lets two Buzzs coexist on
/// one machine without one's cleanup nuking the other's agents.
pub(crate) fn current_instance_id(app: &AppHandle) -> String {
    app.config().identifier.clone()
}

/// Build the full `BEEKEEPER_MANAGED_AGENT=<instance-id>` env entry we match
/// against when scanning processes. Kept here so the spawn stamp and the sweep
/// matcher can never drift apart.
#[cfg(test)]
pub(super) fn beekeeper_marker_entry(instance_id: &str) -> Vec<u8> {
    format!("BEEKEEPER_MANAGED_AGENT={instance_id}").into_bytes()
}

/// The null-delimited environment block of a running process, or `None`.
///
/// Extracted from `process_has_beekeeper_marker` so that more than one question can
/// be asked of a process's environment without a second copy of this walk.
/// The second question is [`process_has_env_key`], which is what keeps a
/// process another launcher owns out of this app's sweeps.
#[cfg(target_os = "macos")]
fn process_env_block(pid: u32) -> Option<Vec<u8>> {
    let buf = sweep::procargs2_buffer(pid)?;

    // Buffer layout: [i32 argc][exec_path\0][null padding][argv\0...][env\0...]
    if buf.len() < std::mem::size_of::<libc::c_int>() {
        return None;
    }
    let mut n_args: libc::c_int = 0;
    unsafe {
        std::ptr::copy_nonoverlapping(
            buf.as_ptr(),
            &mut n_args as *mut libc::c_int as *mut u8,
            std::mem::size_of::<libc::c_int>(),
        );
    }
    let mut pos = std::mem::size_of::<libc::c_int>();

    // Skip exec path (scan to first null).
    while pos < buf.len() && buf[pos] != 0 {
        pos += 1;
    }
    // Skip null padding between exec path and argv[0].
    while pos < buf.len() && buf[pos] == 0 {
        pos += 1;
    }
    // Skip argc argument strings.
    let mut args_remaining = n_args;
    while args_remaining > 0 && pos < buf.len() {
        while pos < buf.len() && buf[pos] != 0 {
            pos += 1;
        }
        while pos < buf.len() && buf[pos] == 0 {
            pos += 1;
        }
        args_remaining -= 1;
    }
    // Remaining bytes are null-delimited environment strings.
    Some(buf[pos..].to_vec())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn process_env_block(pid: u32) -> Option<Vec<u8>> {
    std::fs::read(format!("/proc/{pid}/environ")).ok()
}

#[cfg(not(unix))]
fn process_env_block(_pid: u32) -> Option<Vec<u8>> {
    None
}

/// Check if a running process is one of *our* managed agents: it must carry
/// `BEEKEEPER_MANAGED_AGENT=<instance_id>` in its environment, where `instance_id`
/// is this desktop instance's id. A process stamped with a *different*
/// instance id belongs to another live Beekeeper app and must never be reaped here.
pub(crate) fn process_has_beekeeper_marker(pid: u32, instance_id: &str) -> bool {
    let Some(block) = process_env_block(pid) else {
        return false;
    };
    env_block_has_marker(&block, instance_id)
}

/// The ownership marker's name, then its pre-rename spelling. A harness an
/// older build spawned carries only `BUZZ_MANAGED_AGENT`, and is still ours.
pub(super) const MARKER_KEYS: [&str; 2] = ["BEEKEEPER_MANAGED_AGENT", "BUZZ_MANAGED_AGENT"];

/// The ownership marker's value among a process's environment entries: the
/// current name when present, otherwise the legacy one. Reading the legacy
/// name only as a fallback means a process stamped by a newer build is judged
/// by the stamp that build meant, never by a stale inherited twin.
pub(super) fn marker_value<'a>(entries: impl Iterator<Item = &'a [u8]>) -> Option<String> {
    let entries: Vec<&[u8]> = entries.collect();
    MARKER_KEYS.iter().find_map(|key| {
        let prefix = format!("{key}=");
        entries
            .iter()
            .find_map(|entry| entry.strip_prefix(prefix.as_bytes()))
            .and_then(|value| String::from_utf8(value.to_vec()).ok())
    })
}

/// Whether an environment block carries this instance's ownership marker,
/// under either spelling (see [`marker_value`]).
fn env_block_has_marker(block: &[u8], instance_id: &str) -> bool {
    marker_value(block.split(|&byte| byte == 0)).as_deref() == Some(instance_id)
}

/// Whether a running process carries `key` in its environment, with any value.
///
/// This is how a process **another launcher owns** stays out of this app's
/// sweeps. Before `beekeeper-host` existed, "not one of mine" and "nobody's" were
/// the same set, so reaping the second was safe. They are different sets now,
/// and a boot-time sweep that could not tell them apart would silently kill a
/// headless host's child — the symptom being agents that die whenever somebody
/// opens the app, which reads as anything but a sweep.
pub(crate) fn process_has_env_key(pid: u32, key: &str) -> bool {
    let Some(block) = process_env_block(pid) else {
        return false;
    };
    // Either spelling: a launcher built before the rename stamps `BUZZ_*`.
    beekeeper_core_pkg::env_compat::both_spellings(key)
        .iter()
        .any(|key| env_block_has_key(&block, key.as_bytes()))
}

/// Whether a null-delimited environment block holds this exact entry.
///
/// Pure so the byte-level matching is provable without a live process; the
/// platform-specific half above is the part that cannot be.
#[cfg(test)]
fn env_block_has_entry(block: &[u8], entry: &[u8]) -> bool {
    block.split(|&byte| byte == 0).any(|found| found == entry)
}

/// Whether a null-delimited environment block holds `key` with any value.
///
/// Matches `key=` rather than a bare prefix: `BEEKEEPER_HOST` must not match
/// `BEEKEEPER_HOSTNAME`, and a prefix test is exactly how that mistake gets made.
fn env_block_has_key(block: &[u8], key: &[u8]) -> bool {
    block
        .split(|&byte| byte == 0)
        .any(|entry| entry.len() > key.len() && entry.starts_with(key) && entry[key.len()] == b'=')
}

#[cfg(unix)]
fn signal_process_group_or_leader(pid: u32, signal: i32, action: &str) -> Result<(), String> {
    let pgid = -(pid as i32);

    if unsafe { libc::kill(pgid, signal) } == 0 {
        return Ok(());
    }

    let group_err = std::io::Error::last_os_error();
    if !process_is_running(pid) {
        return Ok(());
    }

    // Some local agent trees can no longer be signalled as a process group
    // (for example if the leader changed groups, or macOS returns EPERM for one
    // descendant). Fall back to the leader PID so stop/delete can still recover.
    if matches!(
        group_err.raw_os_error(),
        Some(libc::EPERM) | Some(libc::ESRCH)
    ) {
        if unsafe { libc::kill(pid as i32, signal) } == 0 {
            return Ok(());
        }

        let leader_err = std::io::Error::last_os_error();
        if leader_err.raw_os_error() == Some(libc::ESRCH) || !process_is_running(pid) {
            return Ok(());
        }

        return Err(format!("failed to {action} process {pid}: {leader_err}"));
    }

    Err(format!(
        "failed to {action} process group {pid}: {group_err}"
    ))
}

#[cfg(unix)]
pub(crate) fn terminate_process(pid: u32) -> Result<(), String> {
    // Try graceful shutdown first (SIGTERM to the group).
    signal_process_group_or_leader(pid, libc::SIGTERM, "terminate")?;

    // Wait up to 1s for graceful exit.
    for _ in 0..10 {
        if !process_is_running(pid) {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    // Escalate to SIGKILL on the entire group.
    signal_process_group_or_leader(pid, libc::SIGKILL, "kill")?;

    Ok(())
}

#[cfg(windows)]
pub(crate) fn terminate_process(pid: u32) -> Result<(), String> {
    // No job handle is available on this path (e.g. after an app restart, when
    // we only recovered the PID from the record), so fall back to taskkill on
    // the whole tree.
    super::super::process_lifecycle::taskkill_tree(pid)
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn terminate_process(_pid: u32) -> Result<(), String> {
    Err("managed agent shutdown after app restart is not supported on this platform".to_string())
}

/// Send SIGTERM to all given PIDs (as process groups), wait, then SIGKILL
/// any survivors. Uses `-pid` to kill the entire process group — if an
/// orphaned agent called `setsid()`, it IS the group leader, so this
/// reaches its children too.
#[cfg(unix)]
fn sigterm_then_sigkill(pids: &[i32]) {
    // Send SIGTERM to each process group. Track whether any signal was
    // actually delivered so we can skip the sleep when everything is
    // already gone.
    let mut any_signalled = false;
    for &pid in pids {
        if unsafe { libc::kill(-pid, libc::SIGTERM) } == 0 {
            any_signalled = true;
        }
    }

    if !any_signalled {
        return;
    }

    std::thread::sleep(std::time::Duration::from_millis(200));

    for &pid in pids {
        // Check if the group has any living members, not just the leader.
        // kill(-pid, 0) returns 0 if ANY member of the group is signalable.
        if unsafe { libc::kill(-pid, 0) } == 0 {
            unsafe {
                libc::kill(-pid, libc::SIGKILL);
            }
        }
    }
}

/// Resolve orphan candidate PIDs to their actual process group IDs, dedupe,
/// and signal the groups. An orphaned grandchild (e.g. `goose` or `beekeeper-dev-mcp`)
/// whose harness has exited retains the harness's PGID — signaling that PGID
/// kills the entire orphaned subtree. Falls back to the candidate PID itself
/// when PGID resolution fails (process may have exited between detection and
/// kill).
#[cfg(target_os = "macos")]
pub(super) fn resolve_pgids_and_kill(candidate_pids: &[i32]) {
    let candidate_set: std::collections::HashSet<i32> = candidate_pids.iter().copied().collect();
    let mut pgids = std::collections::HashSet::new();
    for &pid in candidate_pids {
        let pgid = unsafe { libc::getpgid(pid) };
        if pgid > 0 {
            pgids.insert(pgid);
        } else {
            // Process may have exited; try signaling it directly as a group.
            pgids.insert(pid);
        }
    }
    // PID-recycling guard: if a resolved PGID is alive but isn't one of our
    // orphan candidates, the old harness PID was recycled by a new process
    // that called setsid() — skip it to avoid killing an unrelated group.
    let candidate_groups = pgids.len();
    pgids.retain(|&pgid| {
        if candidate_set.contains(&pgid) {
            return true;
        }
        let alive = unsafe { libc::kill(pgid, 0) } == 0;
        !alive
    });
    if pgids.is_empty() && candidate_groups > 0 {
        eprintln!(
            "beekeeper-desktop: orphan sweep: skipped all {candidate_groups} candidate group(s) (live foreign group leader or candidate already exited); nothing signalled"
        );
    }
    let unique: Vec<i32> = pgids.into_iter().collect();
    sigterm_then_sigkill(&unique);
}

/// Resolve orphan candidate PIDs to their actual process group IDs, dedupe,
/// and signal the groups. Linux variant reads PGID from /proc/<pid>/stat.
#[cfg(all(unix, not(target_os = "macos")))]
pub(super) fn resolve_pgids_and_kill(candidate_pids: &[i32]) {
    let candidate_set: std::collections::HashSet<i32> = candidate_pids.iter().copied().collect();
    let mut pgids = std::collections::HashSet::new();
    for &pid in candidate_pids {
        if let Some((_, pgid)) = sweep::proc_stat_ppid_pgid_linux(pid as u32) {
            pgids.insert(pgid as i32);
        } else {
            // Process may have exited; try signaling it directly as a group.
            pgids.insert(pid);
        }
    }
    // PID-recycling guard: if a resolved PGID is alive but isn't one of our
    // orphan candidates, the old harness PID was recycled by a new process
    // that called setsid() — skip it to avoid killing an unrelated group.
    let candidate_groups = pgids.len();
    pgids.retain(|&pgid| {
        if candidate_set.contains(&pgid) {
            return true;
        }
        let alive = unsafe { libc::kill(pgid, 0) } == 0;
        !alive
    });
    if pgids.is_empty() && candidate_groups > 0 {
        eprintln!(
            "beekeeper-desktop: orphan sweep: skipped all {candidate_groups} candidate group(s) (live foreign group leader or candidate already exited); nothing signalled"
        );
    }
    let unique: Vec<i32> = pgids.into_iter().collect();
    sigterm_then_sigkill(&unique);
}

pub(crate) fn valid_agent_runtime_receipt(
    path: &std::path::Path,
    receipt: &super::super::ManagedAgentRuntimeReceipt,
    instance_id: &str,
) -> bool {
    valid_agent_runtime_receipt_with(
        path,
        receipt,
        instance_id,
        process_is_running,
        process_has_beekeeper_marker,
    )
}

/// Injectable version of `valid_agent_runtime_receipt` for testing.
/// `is_running(pid)` and `has_marker(pid, instance_id)` can be substituted by
/// test doubles without spawning real processes.
pub(crate) fn valid_agent_runtime_receipt_with(
    path: &std::path::Path,
    receipt: &super::super::ManagedAgentRuntimeReceipt,
    instance_id: &str,
    is_running: impl Fn(u32) -> bool,
    has_marker: impl Fn(u32, &str) -> bool,
) -> bool {
    let Ok(canonical) =
        ManagedAgentRuntimeKey::new(receipt.key.pubkey.clone(), &receipt.key.relay_url)
    else {
        return false;
    };
    canonical == receipt.key
        && path.file_name().and_then(|name| name.to_str())
            == Some(&format!("{}.json", receipt.key.runtime_id()))
        && receipt.desktop_instance_id == instance_id
        && is_running(receipt.pid)
        // Receipts are written by THIS instance at spawn time, so they are
        // Beekeeper-owned by construction. Marker-only ownership: custom-harness
        // binaries (not in KNOWN_AGENT_BINARIES) must not be rejected by a
        // name gate — see the sweep ownership rule in runtime/orphan_sweep.rs.
        && has_marker(receipt.pid, &receipt.desktop_instance_id)
}

pub(super) fn terminate_runtime_receipt_with(
    path: &std::path::Path,
    receipt: &super::super::ManagedAgentRuntimeReceipt,
    terminate: impl FnOnce(u32) -> Result<(), String>,
    mut is_running: impl FnMut(u32) -> bool,
    remove: impl FnOnce(&std::path::Path),
) -> Result<(), String> {
    terminate(receipt.pid)?;
    for _ in 0..20 {
        if !is_running(receipt.pid) {
            remove(path);
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    Err(format!(
        "prior runtime {} for pair {} on {} did not exit",
        receipt.pid, receipt.key.pubkey, receipt.key.relay_url
    ))
}

/// Replace a valid prior-session process before registering a new child for
/// the same pair. The caller must hold the runtime transition lock so receipt
/// inspection, termination, spawn, and registration cannot race shutdown or
/// another start.
pub(crate) fn terminate_untracked_pair_runtime(
    app: &AppHandle,
    key: &ManagedAgentRuntimeKey,
) -> Result<(), String> {
    let instance_id = current_instance_id(app);
    let Some((path, receipt)) = super::super::read_all_agent_runtime_receipts(app)
        .into_iter()
        .find(|(path, receipt)| {
            receipt.key == *key && valid_agent_runtime_receipt(path, receipt, &instance_id)
        })
    else {
        return Ok(());
    };

    terminate_runtime_receipt_with(
        &path,
        &receipt,
        terminate_process,
        process_is_running,
        super::super::remove_agent_runtime_receipt_path,
    )
}

#[cfg(test)]
mod env_block_tests {
    use super::*;

    fn block(entries: &[&str]) -> Vec<u8> {
        let mut bytes = Vec::new();
        for entry in entries {
            bytes.extend_from_slice(entry.as_bytes());
            bytes.push(0);
        }
        bytes
    }

    #[test]
    fn an_exact_entry_matches_and_a_different_instance_does_not() {
        let block = block(&[
            "PATH=/usr/bin",
            "BEEKEEPER_MANAGED_AGENT=io.agiterra.beekeeper.app",
            "HOME=/home/agent",
        ]);
        assert!(env_block_has_entry(
            &block,
            b"BEEKEEPER_MANAGED_AGENT=io.agiterra.beekeeper.app"
        ));
        assert!(
            !env_block_has_entry(
                &block,
                b"BEEKEEPER_MANAGED_AGENT=io.agiterra.beekeeper.app.dev"
            ),
            "another live instance's agents must never match"
        );
    }

    /// A key test that matched a prefix would spare `BEEKEEPER_HOSTNAME` as if it
    /// were `BEEKEEPER_HOST_CHILD`, so the `=` is load-bearing.
    #[test]
    fn a_key_match_requires_the_equals_sign() {
        let both = block(&["BEEKEEPER_HOST_CHILD=4242", "BEEKEEPER_HOSTNAME=somewhere"]);
        assert!(env_block_has_key(&both, b"BEEKEEPER_HOST_CHILD"));
        assert!(env_block_has_key(&both, b"BEEKEEPER_HOSTNAME"));
        assert!(
            !env_block_has_key(&both, b"BEEKEEPER_HOST"),
            "a prefix must not match a longer key"
        );
        assert!(!env_block_has_key(&both, b"BEEKEEPER_HOST_CHILD_OF"));
        // An empty value is still the key being present: a host that stamped
        // an empty pid still owns the child.
        assert!(env_block_has_key(
            &block(&["BEEKEEPER_HOST_CHILD="]),
            b"BEEKEEPER_HOST_CHILD"
        ));
    }

    #[test]
    fn the_ownership_marker_is_read_under_either_spelling_new_first() {
        let id = "io.agiterra.beekeeper.app";
        // Spawned by a build from before the rename.
        assert!(env_block_has_marker(
            &block(&["PATH=/bin", "BUZZ_MANAGED_AGENT=io.agiterra.beekeeper.app"]),
            id
        ));
        // Spawned now: both spellings, same value.
        assert!(env_block_has_marker(
            &block(&[
                "BEEKEEPER_MANAGED_AGENT=io.agiterra.beekeeper.app",
                "BUZZ_MANAGED_AGENT=io.agiterra.beekeeper.app",
            ]),
            id
        ));
        // The current name decides; a stale legacy twin cannot claim it.
        assert!(!env_block_has_marker(
            &block(&[
                "BUZZ_MANAGED_AGENT=io.agiterra.beekeeper.app",
                "BEEKEEPER_MANAGED_AGENT=io.agiterra.beekeeper.app.dev",
            ]),
            id
        ));
        assert!(!env_block_has_marker(
            &block(&["BUZZ_MANAGED_AGENT=io.agiterra.beekeeper.app.dev"]),
            id
        ));
        assert!(!env_block_has_marker(&block(&["PATH=/bin"]), id));
    }

    #[test]
    fn an_empty_block_matches_nothing() {
        assert!(!env_block_has_entry(&[], b"A=1"));
        assert!(!env_block_has_key(&[], b"A"));
    }
}
