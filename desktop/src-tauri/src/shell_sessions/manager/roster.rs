//! Invite-roster mutation for shared terminals (NIP-ST).
//!
//! The roster lives on [`super::ShellSessionInfo`] and rides the owner-signed
//! kind:30623 announce as arity-4 `p` tags — the refreshed announce is both
//! the grant and the revocation signal observers see. Split from `manager.rs`
//! for the file-size ratchet; these functions are `super`'s API and lean on
//! its registries.

use tauri::AppHandle;

use super::types::{dormant, lock_registry};
use super::{RosterEntry, MAX_ROSTER};

/// Validate and normalize a roster from the UI: lowercase-hex pubkeys,
/// pinned roles, no duplicates, capped at [`MAX_ROSTER`]. Rejects (rather
/// than silently drops) malformed input so the UI can't believe a grant
/// exists that was never stored.
fn normalize_roster(roster: Vec<RosterEntry>) -> Result<Vec<RosterEntry>, String> {
    if roster.len() > MAX_ROSTER {
        return Err(format!("roster exceeds {MAX_ROSTER} members"));
    }
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(roster.len());
    for entry in roster {
        let pubkey = entry.pubkey.trim().to_ascii_lowercase();
        if pubkey.len() != 64 || !pubkey.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(format!("invalid roster pubkey: {}", entry.pubkey));
        }
        if !buzz_core_pkg::kind::is_valid_shell_role(&entry.role) {
            return Err(format!("invalid roster role: {}", entry.role));
        }
        if !seen.insert(pubkey.clone()) {
            // Duplicate pubkey — keep the first entry, drop the repeat.
            continue;
        }
        out.push(RosterEntry {
            pubkey,
            role: entry.role,
        });
    }
    Ok(out)
}

/// Replace a session's invite roster (mirrors `set_shared`): update the
/// live/dormant registries, persist the app-owned sidecar, and re-announce —
/// the refreshed kind:30623 announce (with its `p` tags) is the grant AND
/// the revocation signal observers see.
pub fn set_roster(
    app: &AppHandle,
    session_id: &str,
    roster: Vec<RosterEntry>,
) -> Result<(), String> {
    let roster = normalize_roster(roster)?;
    let updated = {
        let mut sessions = lock_registry()?;
        match sessions.get_mut(session_id) {
            Some(session) => {
                session.info.roster = roster.clone();
                Some(session.info.clone())
            }
            None => {
                drop(sessions);
                let mut map = dormant()
                    .lock()
                    .map_err(|_| "shell-session dormant lock poisoned".to_string())?;
                match map.get_mut(session_id) {
                    Some(session) => {
                        session.info.roster = roster.clone();
                        Some(session.info.clone())
                    }
                    None => None,
                }
            }
        }
    };
    let Some(info) = updated else {
        return Err(format!("shell session {session_id} not found"));
    };
    crate::shell_sessions::persist::set_app_meta(
        app,
        session_id,
        crate::shell_sessions::persist::AppMeta {
            project_ref: info.project_ref.clone(),
            shared: info.shared,
            roster,
        },
    );
    if crate::shell_sessions::broadcast::may_broadcast(&info).is_some() {
        crate::shell_sessions::broadcast::announce(app, &info, "open");
        // An invite revocation must also drop that watcher's live stream.
        crate::shell_sessions::broadcast::refresh_admission(session_id);
    } else {
        crate::shell_sessions::broadcast::announce(app, &info, "closed");
        crate::shell_sessions::broadcast::session_ended(session_id);
    }
    Ok(())
}

/// Append (or role-update) one roster entry — the access-request "Enable
/// full control" path, which grants a requesting agent collaborator access.
pub fn add_roster_entry(
    app: &AppHandle,
    session_id: &str,
    pubkey: &str,
    role: &str,
) -> Result<(), String> {
    let info =
        super::info(session_id).ok_or_else(|| format!("shell session {session_id} not found"))?;
    let mut roster = info.roster;
    let normalized = pubkey.trim().to_ascii_lowercase();
    if let Some(existing) = roster.iter_mut().find(|e| e.pubkey == normalized) {
        if existing.role == role {
            return Ok(());
        }
        existing.role = role.to_string();
    } else {
        roster.push(RosterEntry {
            pubkey: normalized,
            role: role.to_string(),
        });
    }
    set_roster(app, session_id, roster)
}
