//! The cached codex-acp adapter availability behind the restart badge.

use crate::managed_agents::AcpAvailabilityStatus;

// ── Adapter availability cache (Phase-2 badge fallback) ─────────────────────
//
// `build_managed_agent_summary` needs to compare the spawn-time adapter
// availability against the *current* availability without triggering a live
// `probe_codex_acp_version` subprocess on every poll cycle.  This cache
// stores the last availability status of the codex-acp binary at its resolved
// path.  It is warmed by `discover_acp_runtimes` (which already probes), so
// the badge path reads warm data, and is invalidated by `clear_resolve_cache`
// (called on every Doctor install and every `discover_acp_providers` call).

fn adapter_availability_cache() -> &'static std::sync::Mutex<Option<AcpAvailabilityStatus>> {
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<Option<AcpAvailabilityStatus>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

pub(super) fn clear_adapter_availability_cache() {
    if let Ok(mut guard) = adapter_availability_cache().lock() {
        *guard = None;
    }
}

/// Cache the current codex-acp adapter availability status.
///
/// Called by `discover_acp_runtimes` after it probes the codex adapter so the
/// badge path has a warm value without re-probing.
pub(crate) fn cache_adapter_availability(status: AcpAvailabilityStatus) {
    if let Ok(mut guard) = adapter_availability_cache().lock() {
        *guard = Some(status);
    }
}

/// Return the most recently cached codex-acp adapter availability, or
/// `None` if no discovery has run yet.
///
/// This is a **read from cache only** — it never spawns a subprocess.  The
/// value is populated by `discover_acp_runtimes` and invalidated by
/// `clear_resolve_cache`.  When the cache is cold, returning `None` defers
/// the drift check until discovery has produced a real value, preventing
/// a fabricated `AdapterMissing` stamp from triggering a false restart badge
/// on a newly restarted process.
pub(crate) fn adapter_availability_cached() -> Option<AcpAvailabilityStatus> {
    adapter_availability_cache()
        .lock()
        .ok()
        .and_then(|g| g.clone())
}

/// Pure predicate: does the stamped adapter availability differ from the
/// current cached availability?
///
/// Returns `false` whenever either side is `None` (unknown) — "no data" is
/// not evidence of drift.  This is extracted for unit testing without global
/// state and used by `build_managed_agent_summary`.
pub(crate) fn availability_drift(
    stamped: Option<&AcpAvailabilityStatus>,
    current: Option<AcpAvailabilityStatus>,
) -> bool {
    match (stamped, current) {
        (Some(s), Some(c)) => *s != c,
        _ => false,
    }
}
