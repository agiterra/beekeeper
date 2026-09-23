//! The per-agent side effects of removing a managed agent, in one place.
//!
//! Extracted from [`super::delete_managed_agent`] when the project teardown
//! needed the same four acts for many agents at once. The teardown cannot
//! simply call that command in a loop, for four reasons worth stating because
//! each one is a bug somebody would otherwise write:
//!
//! 1. **It would deadlock.** `managed_agents_store_lock` is a
//!    `std::sync::Mutex<()>` — not reentrant — and `delete_managed_agent`
//!    takes it itself.
//! 2. **Dropping the lock between agents opens a window** in which another
//!    writer can re-add, rename or re-associate one of the agents being
//!    removed, so the set acted on stops matching the set approved.
//! 3. **It would rewrite the whole store once per agent.** `save_managed_agents`
//!    re-reads the definition half off disk and rewrites the unified file, so
//!    seven agents means seven full rewrites and seven chances to die
//!    half-done.
//! 4. **Its failure mode is wrong for a cascade.** A single stuck process
//!    aborts `delete_managed_agent`; a multi-agent teardown must not be
//!    abandoned half-way by one of them.
//!
//! So the loop is inlined under one lock, and *this* is the part both paths
//! share — which keeps them from drifting.

use tauri::AppHandle;

use crate::app_state::AppState;

/// Retire one agent's identity everywhere outside the agent store.
///
/// Call **after** the record has left `managed-agents.json` and that write
/// has succeeded. The ordering is deliberate and is the same one
/// `delete_persona` keeps: a tombstone published for a record that then fails
/// to delete leaves the relay saying an agent is gone while this computer
/// still runs it.
///
/// Each act is best-effort and independent — the keyring entry, the kind:5
/// over the agent's kind:30177, and the NIP-IA archive request that stops it
/// appearing in member pickers and autocomplete. None of them can fail in a
/// way that should undo the removal, because the removal has already
/// happened.
///
/// `persona_id` is read from the record *before* it is dropped, since the
/// archive request needs it and the record is gone by the time this runs.
pub(crate) fn purge_managed_agent_side_effects(
    app: &AppHandle,
    state: &AppState,
    pubkey: &str,
    persona_id: Option<&str>,
) {
    state.clear_agent_session_caches(pubkey);
    crate::managed_agents::delete_agent_key(pubkey);
    super::tombstone_managed_agent_pending(app, state, pubkey);
    super::archive_managed_agent_pending(app, state, pubkey, persona_id);
}
