//! This computer's session-title mode (SV-56, decision D9).
//!
//! The person picks, in the desktop's Settings → Session titles, who names an
//! unnamed session:
//!
//! - **agent** (the default): the provider that runs the founder's first turn
//!   asks the session's own runtime for a title and signs it as a kind-44252
//!   generated title (SV-31, `auto_title.rs`).
//! - **my-model**: the person's naming model only suggests a name in the
//!   desktop's Name field before Start; nothing titles the session after
//!   Start, and the provider publishes no generated title.
//! - **off**: nobody names it; the session keeps its operator title or
//!   "Untitled", and the provider publishes no generated title.
//!
//! A name a person set wins in every mode. The choice is this machine's
//! preference, so it lives on this machine: a file in the provider's state
//! directory, written by the desktop and read live by the provider before a
//! titling job starts and again immediately before it signs. Nothing on the
//! relay carries it.
//!
//! ```json
//! { "version": 1, "mode": "agent" }
//! ```
//!
//! An absent file is the default (`agent`). An unreadable or malformed file
//! titles nothing — the provider fails closed and says why — because a person
//! who asked for no title must never get one by accident. The host-wide
//! `BUZZ_CSP_AUTO_TITLE=off` switch wins over every mode.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// The file name, in the provider's state directory.
pub const SESSION_TITLE_MODE_FILE: &str = "session-title-mode.json";

/// The schema version this provider reads and writes.
const VERSION: u64 = 1;

/// Who names an unnamed session on this computer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SessionTitleMode {
    /// The session's own agent runtime names it, through the provider
    /// (SV-31). The default: it needs no setup.
    #[default]
    Agent,
    /// The person's own naming model names it, from the desktop; the provider
    /// stays out of it.
    MyModel,
    /// Nothing names it.
    Off,
}

impl SessionTitleMode {
    /// Whether the provider generates and publishes a title in this mode
    /// (`agent` only).
    #[must_use]
    pub fn provider_generates(self) -> bool {
        matches!(self, Self::Agent)
    }

    /// The mode's wire spelling, as written in the file.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::MyModel => "my-model",
            Self::Off => "off",
        }
    }
}

#[derive(Serialize, Deserialize)]
struct ModeFile {
    version: u64,
    mode: SessionTitleMode,
}

/// Read this computer's mode from `state_dir`.
///
/// An absent file is `Ok(SessionTitleMode::Agent)`.
///
/// # Errors
/// The file exists but cannot be read, is not JSON, names a version other
/// than 1, or names an unknown mode. The reason is returned; callers title
/// nothing on an error.
pub fn read(state_dir: &Path) -> Result<SessionTitleMode, String> {
    let path = state_dir.join(SESSION_TITLE_MODE_FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(SessionTitleMode::Agent)
        }
        Err(error) => return Err(format!("could not be read: {error}")),
    };
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|error| format!("is not JSON: {error}"))?;
    match value.get("version").and_then(serde_json::Value::as_u64) {
        Some(VERSION) => {}
        Some(other) => return Err(format!("names unknown version {other}")),
        None => return Err("names no version".to_owned()),
    }
    let file: ModeFile =
        serde_json::from_value(value).map_err(|error| format!("names no known mode: {error}"))?;
    Ok(file.mode)
}

/// Write this computer's mode into `state_dir`, atomically: a staged file
/// renamed over the old one, readable only by its owner on unix.
///
/// # Errors
/// The state directory could not be written; the reason is returned and the
/// previous file, if any, is left as it was.
pub fn write(state_dir: &Path, mode: SessionTitleMode) -> Result<(), String> {
    let path = state_dir.join(SESSION_TITLE_MODE_FILE);
    let staged = state_dir.join(format!("{SESSION_TITLE_MODE_FILE}.tmp"));
    let body = serde_json::to_string(&ModeFile {
        version: VERSION,
        mode,
    })
    .map_err(|error| format!("could not encode the session-title mode: {error}"))?;
    std::fs::create_dir_all(state_dir)
        .map_err(|error| format!("could not create {}: {error}", state_dir.display()))?;
    std::fs::write(&staged, body)
        .map_err(|error| format!("could not write {}: {error}", staged.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o600))
            .map_err(|error| format!("could not restrict {}: {error}", staged.display()))?;
    }
    std::fs::rename(&staged, &path)
        .map_err(|error| format!("could not replace {}: {error}", path.display()))
}

/// Whether the provider may title a session right now, given the host-wide
/// switch and this computer's mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Admission {
    /// Generate (switch on, mode `agent`).
    Generate,
    /// `BUZZ_CSP_AUTO_TITLE=off`: the host switch wins over every mode.
    HostOff,
    /// The mode leaves titling to someone else, or to no one.
    Declined(SessionTitleMode),
    /// The mode file could not be read; nothing is titled.
    Unreadable(String),
}

/// Decide [`Admission`] from the host switch and the file in `state_dir`,
/// read now.
pub(crate) fn admission(host_enabled: bool, state_dir: &Path) -> Admission {
    if !host_enabled {
        return Admission::HostOff;
    }
    match read(state_dir) {
        Ok(mode) if mode.provider_generates() => Admission::Generate,
        Ok(mode) => Admission::Declined(mode),
        Err(reason) => Admission::Unreadable(reason),
    }
}

/// Whether a titling job may start for `session_ref` now, by this
/// computer's mode (the host switch was already checked). A mode that
/// declines is logged at info, an unreadable file at warn naming the file and
/// the reason; the caller marks the umbrella decided, so each is said once.
pub(crate) fn admit_job(state_dir: &Path, session_ref: &str) -> bool {
    match admission(true, state_dir) {
        Admission::Generate => true,
        Admission::HostOff => false,
        Admission::Declined(mode) => {
            tracing::info!(
                target: "csp::auto_title",
                %session_ref,
                "session-title mode is {} on this computer; the provider does not title this session",
                mode.as_str()
            );
            false
        }
        Admission::Unreadable(reason) => {
            tracing::warn!(
                target: "csp::auto_title",
                %session_ref,
                "{} {reason}; session not titled",
                state_dir.join(SESSION_TITLE_MODE_FILE).display()
            );
            false
        }
    }
}

/// Say at startup which mode the file in `state_dir` holds (the host switch
/// is on when this is called).
pub(crate) fn log_startup(state_dir: &Path) {
    let file = state_dir.join(SESSION_TITLE_MODE_FILE);
    match read(state_dir) {
        Ok(mode) if mode.provider_generates() => tracing::info!(
            target: "csp::auto_title",
            "session-title mode is {} ({}); read again before every title",
            mode.as_str(),
            file.display()
        ),
        Ok(mode) => tracing::info!(
            target: "csp::auto_title",
            "session-title mode is {} ({}): the provider titles nothing until it is agent",
            mode.as_str(),
            file.display()
        ),
        Err(reason) => tracing::warn!(
            target: "csp::auto_title",
            "{} {reason}: the provider titles nothing until it can be read",
            file.display()
        ),
    }
}

#[cfg(test)]
#[path = "session_title_mode_tests.rs"]
mod tests;
