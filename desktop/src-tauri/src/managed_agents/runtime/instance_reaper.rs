use super::*;

/// Binary names for the Beekeeper desktop/Tauri process. Used by
/// dead-instance detection to narrow the PID scan before the identifier check
/// below, which is what actually decides ownership.
///
/// These are all spellings of the **Cargo binary name**, not the product name:
/// `CFBundleExecutable` is the *Cargo binary* name, not `productName`, so a
/// Tauri `productName` rename never changes the process name — but renaming the
/// crate does. The crate was `buzz-desktop` before the Beekeeper rebrand, and
/// both names stay listed: an installed older build, or stock Buzz, still runs
/// under the old one. Stock Buzz therefore shares names here — which is fine,
/// and is precisely why
/// [`desktop_is_alive_for_instance`] additionally requires the bundle
/// identifier to appear in the process's argv/environ. That identifier
/// (`io.agiterra.beekeeper.app` vs `xyz.block.buzz.app`) is the only thing keeping
/// two coexisting installs from reaping each other's agents.
const DESKTOP_BINARY_NAMES: &[&str] = &[
    "beekeeper-desktop",
    "beekeeper_desktop",
    "buzz-desktop",
    "buzz_desktop",
    // Linux limits /proc/<pid>/comm to 15 visible bytes, truncating the
    // AppImage shim's real executable name, `<crate>.bin`. At 17 bytes,
    // `beekeeper-desktop` truncates to the same 15 as its `.bin` shim, so one
    // entry covers both; the 12-byte `buzz-desktop` needed a separate one.
    "beekeeper-deskt",
    "buzz-desktop.bi",
];

/// Check if a process name matches a known Beekeeper desktop binary.
pub(super) fn is_desktop_binary(name: &str) -> bool {
    DESKTOP_BINARY_NAMES.contains(&name)
}

/// Check whether `buf` contains `id` as a complete identifier — not as a
/// prefix of a longer dotted name. The identifier appears in the Tauri config
/// JSON as `"identifier":"io.agiterra.beekeeper.app.dev"` and in environment entries
/// as `KEY=...app.dev\0`, so a valid match is followed by a non-identifier byte
/// (not `[A-Za-z0-9._-]`) or sits at the end of the buffer. This prevents
/// `io.agiterra.beekeeper.app` from matching inside `io.agiterra.beekeeper.app.dev`.
pub(super) fn buffer_contains_identifier(buf: &[u8], id: &[u8]) -> bool {
    if id.is_empty() {
        return false;
    }
    buf.windows(id.len()).enumerate().any(|(i, w)| {
        if w != id {
            return false;
        }
        // Boundary check on the byte immediately after the match: end-of-buffer
        // or any byte that can't continue a dotted reverse-DNS identifier.
        match buf.get(i + id.len()) {
            None => true,
            Some(&next) => {
                !next.is_ascii_alphanumeric() && next != b'.' && next != b'_' && next != b'-'
            }
        }
    })
}

/// Extract the `BEEKEEPER_MANAGED_AGENT` (or legacy `BUZZ_MANAGED_AGENT`)
/// value from a process's environment.
/// Returns `None` if the process doesn't have the marker or can't be read.
#[cfg(target_os = "macos")]
fn extract_buzz_marker_value(pid: u32) -> Option<String> {
    let buf = sweep::procargs2_buffer(pid)?;

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

    // Skip exec path.
    while pos < buf.len() && buf[pos] != 0 {
        pos += 1;
    }
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
    // Search environment entries for our marker, under either spelling.
    super::process::marker_value(buf[pos..].split(|&b| b == 0))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn extract_buzz_marker_value(pid: u32) -> Option<String> {
    let data = std::fs::read(format!("/proc/{pid}/environ")).ok()?;
    super::process::marker_value(data.split(|&b| b == 0))
}

#[cfg(not(unix))]
fn extract_buzz_marker_value(_pid: u32) -> Option<String> {
    None
}

/// Check if a Beekeeper desktop process is still alive for the given instance ID.
/// Scans all user-owned processes named "buzz-desktop" and checks
/// whether any has the identifier in its command-line args (KERN_PROCARGS2 buffer
/// includes both argv and environ — the `--config` JSON from `tauri dev` contains
/// the identifier string).
#[cfg(target_os = "macos")]
fn desktop_is_alive_for_instance(instance_id: &str) -> bool {
    extern "C" {
        fn proc_name(pid: libc::c_int, buffer: *mut libc::c_void, buffersize: u32) -> libc::c_int;
    }

    let my_uid = unsafe { libc::getuid() };
    let identifier_bytes = instance_id.as_bytes();

    let pids = sweep::collect_all_pids();
    if pids.is_empty() {
        return false;
    }

    for &pid in &pids {
        if pid <= 0 {
            continue;
        }
        // Check binary name — only look at desktop binaries.
        let mut name_buf = [0u8; 1024];
        let len = unsafe {
            proc_name(
                pid,
                name_buf.as_mut_ptr() as *mut libc::c_void,
                name_buf.len() as u32,
            )
        };
        if len <= 0 {
            continue;
        }
        let name = String::from_utf8_lossy(&name_buf[..len as usize]);
        if !is_desktop_binary(&name) {
            continue;
        }
        // Verify UID.
        let mut info = std::mem::MaybeUninit::<BSDInfo>::zeroed();
        let ret = unsafe {
            proc_pidinfo(
                pid,
                PROC_PIDTBSDINFO,
                0,
                info.as_mut_ptr() as *mut libc::c_void,
                std::mem::size_of::<BSDInfo>() as libc::c_int,
            )
        };
        if ret <= 0 {
            continue;
        }
        let info = unsafe { info.assume_init() };
        if info.pbi_uid != my_uid {
            continue;
        }
        // Check if this desktop process's args/env contain the identifier.
        // The KERN_PROCARGS2 buffer holds argv + environ as null-delimited strings.
        let Some(args_buf) = sweep::procargs2_buffer(pid as u32) else {
            continue;
        };
        // Boundary-anchored search: the identifier in the config JSON is
        // followed by a non-identifier char (typically `"`). A raw substring
        // match would let `...app` match inside `...app.dev`.
        if buffer_contains_identifier(&args_buf, identifier_bytes) {
            return true;
        }
    }
    false
}

#[cfg(all(unix, not(target_os = "macos")))]
fn desktop_is_alive_for_instance(instance_id: &str) -> bool {
    let my_uid = unsafe { libc::getuid() };
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return false;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else {
            continue;
        };
        let Ok(pid) = name_str.parse::<u32>() else {
            continue;
        };
        // Check ownership.
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        use std::os::unix::fs::MetadataExt;
        if meta.uid() != my_uid {
            continue;
        }
        // Check binary name via /proc/<pid>/comm.
        let Ok(comm) = std::fs::read_to_string(format!("/proc/{pid}/comm")) else {
            continue;
        };
        if !is_desktop_binary(comm.trim()) {
            continue;
        }
        // Check cmdline for the identifier with boundary anchoring.
        let Ok(cmdline) = std::fs::read(format!("/proc/{pid}/cmdline")) else {
            continue;
        };
        if buffer_contains_identifier(&cmdline, instance_id.as_bytes()) {
            return true;
        }
    }
    false
}

#[cfg(not(unix))]
fn desktop_is_alive_for_instance(_instance_id: &str) -> bool {
    false
}

/// Reap agent processes belonging to dead Beekeeper desktop instances.
///
/// Scans all user processes for `BEEKEEPER_MANAGED_AGENT=*`, groups them by
/// instance ID, and for each foreign instance (≠ `our_instance_id`) checks
/// whether a Beekeeper desktop binary is still alive for that instance. If not,
/// all agents from that dead instance are reaped.
#[cfg(target_os = "macos")]
pub(crate) fn reap_dead_instance_agents(our_instance_id: &str, skip_pids: &[u32]) {
    let my_uid = unsafe { libc::getuid() };
    let my_pid = std::process::id() as i32;

    let pids = sweep::collect_all_pids();
    if pids.is_empty() {
        return;
    }

    // Collect (pid, instance_id) for all foreign agent processes.
    let mut foreign_agents: HashMap<String, Vec<i32>> = HashMap::new();

    for &pid in &pids {
        if pid <= 0 || pid == my_pid {
            continue;
        }
        let upid = pid as u32;
        if skip_pids.contains(&upid) {
            continue;
        }
        // Verify UID and PPID via proc_pidinfo before the more expensive env scan.
        let mut info = std::mem::MaybeUninit::<BSDInfo>::zeroed();
        let ret = unsafe {
            proc_pidinfo(
                pid,
                PROC_PIDTBSDINFO,
                0,
                info.as_mut_ptr() as *mut libc::c_void,
                std::mem::size_of::<BSDInfo>() as libc::c_int,
            )
        };
        if ret <= 0 {
            continue;
        }
        let info = unsafe { info.assume_init() };
        if info.pbi_uid != my_uid {
            continue;
        }
        // Extract the instance ID from this agent's env.
        // Do NOT name-gate via process_belongs_to_us — custom harnesses use
        // arbitrary binary names and BEEKEEPER_MANAGED_AGENT is the authoritative
        // ownership proof.
        let Some(agent_instance_id) = extract_buzz_marker_value(upid) else {
            continue;
        };
        // Skip agents belonging to our own instance (handled by sweep_system_agent_processes).
        if agent_instance_id == our_instance_id {
            continue;
        }
        foreign_agents
            .entry(agent_instance_id)
            .or_default()
            .push(pid);
    }

    // For each foreign instance, check if its desktop is still alive.
    for (instance_id, agent_pids) in &foreign_agents {
        if desktop_is_alive_for_instance(instance_id) {
            continue;
        }
        eprintln!(
            "beekeeper-desktop: reaping {} orphaned agent(s) from dead instance '{instance_id}'",
            agent_pids.len()
        );
        resolve_pgids_and_kill(agent_pids);
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
pub(crate) fn reap_dead_instance_agents(our_instance_id: &str, skip_pids: &[u32]) {
    let my_uid = unsafe { libc::getuid() };
    let my_pid = std::process::id() as i32;
    let mut foreign_agents: HashMap<String, Vec<i32>> = HashMap::new();

    let Ok(entries) = std::fs::read_dir("/proc") else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else {
            continue;
        };
        let Ok(pid) = name_str.parse::<i32>() else {
            continue;
        };
        if pid <= 0 || pid == my_pid {
            continue;
        }
        let upid = pid as u32;
        if skip_pids.contains(&upid) {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        use std::os::unix::fs::MetadataExt;
        if meta.uid() != my_uid {
            continue;
        }
        // Do NOT name-gate via process_belongs_to_us — custom harnesses use
        // arbitrary binary names and BEEKEEPER_MANAGED_AGENT is the authoritative
        // ownership proof.
        let Some(agent_instance_id) = extract_buzz_marker_value(upid) else {
            continue;
        };
        if agent_instance_id == our_instance_id {
            continue;
        }
        foreign_agents
            .entry(agent_instance_id)
            .or_default()
            .push(pid);
    }

    for (instance_id, agent_pids) in &foreign_agents {
        if desktop_is_alive_for_instance(instance_id) {
            continue;
        }
        eprintln!(
            "beekeeper-desktop: reaping {} orphaned agent(s) from dead instance '{instance_id}'",
            agent_pids.len()
        );
        resolve_pgids_and_kill(agent_pids);
    }
}

#[cfg(not(unix))]
pub(crate) fn reap_dead_instance_agents(_our_instance_id: &str, _skip_pids: &[u32]) {}
