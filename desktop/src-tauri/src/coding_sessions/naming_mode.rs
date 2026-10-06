//! Who titles a new coding session on this computer (D9, SV-56).
//!
//! The person chooses in Settings → Coding sessions → Session titles:
//! the session's own agent (the default, SV-31), their own naming model
//! (the per-device endpoint in [`super::naming`]), or nothing. Two things
//! honour that choice, and both are on this machine:
//!
//! - **The desktop's naming commands.** `generate_coding_session_name` and
//!   `generate_coding_session_goal` refuse unless the mode is `my-model`, so
//!   the card's "nothing is sent" is true whatever the webview does.
//! - **This computer's agent host.** Every provider identity in the local
//!   store gets the mode as `session-title-mode.json` in its state directory
//!   (the format is the provider's: `beekeeper_session_provider::session_title_mode`),
//!   written on every save and again at provisioning, before the host
//!   starts. The provider reads it live before titling, so a save is in
//!   force for the next session without a restart.
//!
//! The choice is this machine's preference and never goes on the relay. A
//! session run by another computer follows that computer's file.

use std::path::{Path, PathBuf};

use beekeeper_session_provider_pkg::session_title_mode;
use tauri::AppHandle;

use super::naming::CodingSessionNamingProvider;
use crate::session_provider::store::load_provider_readiness_store;

/// The mode type is the provider's own, so the desktop cannot write a
/// spelling the provider does not read.
pub use beekeeper_session_provider_pkg::session_title_mode::SessionTitleMode as CodingSessionTitleMode;

/// The mode in force for a record.
///
/// A record saved before modes existed has none. Configuring a naming
/// endpoint was an opt-in to "my model", so that is preserved; any other
/// record takes the default.
#[must_use]
pub fn effective_title_mode(
    stored: Option<CodingSessionTitleMode>,
    provider: CodingSessionNamingProvider,
) -> CodingSessionTitleMode {
    match stored {
        Some(mode) => mode,
        None if provider != CodingSessionNamingProvider::Off => CodingSessionTitleMode::MyModel,
        None => CodingSessionTitleMode::Agent,
    }
}

/// Refuse to consult the naming model unless the mode is `my-model`.
///
/// # Errors
/// The stated reason, naming the mode in force on this computer.
pub fn require_naming_model(mode: CodingSessionTitleMode) -> Result<(), String> {
    match mode {
        CodingSessionTitleMode::MyModel => Ok(()),
        CodingSessionTitleMode::Agent => Err(
            "Session titles on this computer are set to “Generate with the session's agent”, \
             so your naming model is not asked; the session's agent titles it after Start."
                .to_string(),
        ),
        CodingSessionTitleMode::Off => Err(
            "Session titles are Off on this computer, so nothing is sent to a naming model."
                .to_string(),
        ),
    }
}

/// Check that a mode can be stored with this endpoint.
///
/// "Use my naming model" with no endpoint would be a mode that does
/// nothing while reading as on.
///
/// # Errors
/// `my-model` with the endpoint `off`.
pub fn validate_mode_with_provider(
    mode: CodingSessionTitleMode,
    provider: CodingSessionNamingProvider,
) -> Result<(), String> {
    if mode == CodingSessionTitleMode::MyModel && provider == CodingSessionNamingProvider::Off {
        return Err(
            "“Use my naming model” needs an endpoint: choose the Anthropic API or an \
             OpenAI-compatible API."
                .to_string(),
        );
    }
    Ok(())
}

/// Write `mode` into every state directory in `dirs`.
///
/// Every directory is attempted even after one fails, so one unwritable
/// identity does not leave the others stale; every failure is reported.
///
/// # Errors
/// One sentence per directory that could not be written.
pub fn write_title_mode_to_dirs(
    dirs: &[PathBuf],
    mode: CodingSessionTitleMode,
) -> Result<(), String> {
    let failures: Vec<String> = dirs
        .iter()
        .filter_map(|dir| write_one(dir, mode).err())
        .collect();
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "this computer's agent host was not told the session-title mode: {}",
            failures.join("; ")
        ))
    }
}

fn write_one(dir: &Path, mode: CodingSessionTitleMode) -> Result<(), String> {
    session_title_mode::write(dir, mode)
}

/// The state directory of every provider identity in this machine's store.
///
/// Read without hydrating keys: only the public keys are needed, and a
/// settings save must never become a keychain prompt.
fn local_provider_state_dirs(app: &AppHandle) -> Result<Vec<PathBuf>, String> {
    let store = load_provider_readiness_store(app, None)?;
    store
        .providers
        .values()
        .map(|record| crate::session_provider::provider_state_dir(app, &record.provider_pubkey))
        .collect()
}

/// Say where the agent-host files in `dirs` disagree with `mode`.
///
/// `None` when every file reads `mode` (an absent file reads as the default,
/// `agent`, exactly as the provider reads it). Otherwise one sentence naming
/// each directory whose file holds another mode or cannot be read — the
/// state a failed save or a failed provisioning leaves behind, which the
/// card shows until a later write succeeds.
#[must_use]
pub fn host_mode_mismatch_in_dirs(
    dirs: &[PathBuf],
    mode: CodingSessionTitleMode,
) -> Option<String> {
    let differences: Vec<String> = dirs
        .iter()
        .filter_map(|dir| match session_title_mode::read(dir) {
            Ok(found) if found == mode => None,
            Ok(found) => Some(format!("{} is on {}", dir.display(), found.as_str())),
            Err(reason) => Some(format!(
                "{} {reason}",
                dir.join(session_title_mode::SESSION_TITLE_MODE_FILE)
                    .display()
            )),
        })
        .collect();
    if differences.is_empty() {
        None
    } else {
        Some(format!(
            "this computer's agent host is not on {}: {}",
            mode.as_str(),
            differences.join("; ")
        ))
    }
}

/// [`host_mode_mismatch_in_dirs`] over every provider identity on this
/// computer. A store that cannot be read is itself a mismatch: nothing can
/// say what the host was told.
#[must_use]
pub fn host_mode_mismatch(app: &AppHandle, mode: CodingSessionTitleMode) -> Option<String> {
    match local_provider_state_dirs(app) {
        Ok(dirs) => host_mode_mismatch_in_dirs(&dirs, mode),
        Err(error) => Some(format!(
            "could not check what this computer's agent host was told: {error}"
        )),
    }
}

/// After a failed publish at provisioning, make the new identity's host fail
/// closed rather than fall back to the default (`agent`, which titles).
///
/// `wanted` is the stored mode, or `None` when the record could not be read.
/// If the new identity's file already reads `wanted` (the failure was some
/// other identity's directory), it is left alone; otherwise it is set to
/// `off`, so a person who chose Off never gets a title because a write failed.
///
/// Returns the mode the directory's file now holds.
///
/// # Errors
/// The fallback write itself failed; the host will read whatever is there.
pub fn fail_closed_in_dir(
    dir: &Path,
    wanted: Option<CodingSessionTitleMode>,
) -> Result<CodingSessionTitleMode, String> {
    if let Some(mode) = wanted {
        if session_title_mode::read(dir).ok() == Some(mode) {
            return Ok(mode);
        }
    }
    write_one(dir, CodingSessionTitleMode::Off)?;
    Ok(CodingSessionTitleMode::Off)
}

/// Tell every provider identity on this computer which mode is in force.
///
/// No identity yet is not an error: provisioning writes the mode before the
/// first host starts.
///
/// # Errors
/// The provider store could not be read, or a state directory could not be
/// written.
pub fn publish_title_mode(app: &AppHandle, mode: CodingSessionTitleMode) -> Result<(), String> {
    write_title_mode_to_dirs(&local_provider_state_dirs(app)?, mode)
}

#[cfg(test)]
#[path = "naming_mode_tests.rs"]
mod tests;
