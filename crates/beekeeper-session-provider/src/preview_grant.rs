//! Minting the per-execution **preview grant** (SV-33 S1/S2).
//!
//! Every execution this provider spawns or restores, seated or not, gets two
//! variables after the credential fence: [`PREVIEW_GRANT_ENV`], a short-lived
//! bearer token naming the session it speaks for, and
//! [`SESSION_BROKER_SOCK_ENV`], the absolute path of this machine's desktop
//! session broker. `bee preview` presents the first to the broker at the
//! second; it takes no session argument, so the grant is the only thing that
//! binds a call to a session. The token format and the checks the broker runs
//! are `beekeeper_core::preview_grant`; this module only decides the inputs.
//!
//! - **Issuer**: this provider's own key ([`Config::keys`]), which the desktop
//!   already trusts for this provider's transcripts.
//! - **Audience**: the desktop broker of the identity this provider serves,
//!   read from the owner pubkey in its NIP-OA attestation
//!   ([`Config::auth_tag`], `["auth", <owner hex>, …]`). The desktop mints that
//!   attestation with the same identity it signs in as, so the audience is the
//!   one the desktop computes from its own key. A provider with no attestation
//!   has no owner to address, mints nothing, and says so once.
//! - **Lifetime**: [`PREVIEW_GRANT_DEFAULT_TTL_SECS`]. A process's environment
//!   cannot change once it runs, so renewal is re-minting: every resume and
//!   every restore spawns a new process and gets a fresh grant (new nonce, new
//!   expiry). An execution that outlives the lifetime without either is
//!   refused `preview_grant_expired` and recovers on its next restore.
//! - **Execution id**: the lifecycle command that minted this generation —
//!   the create's or resume's `commandId`, which a restore of the same
//!   generation reuses — so one generation has one execution id however often
//!   its process is reopened.
//!
//! Neither value is a relay credential. The token is never logged here (only
//! its execution id is); the socket path is host-local, as every `bee
//! session` caller's already is.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use beekeeper_core::coding_session_command::CodingSessionTarget;
use beekeeper_core::preview_grant::{
    mint_preview_grant, preview_grant_audience, PreviewGrantRequest,
    PREVIEW_GRANT_DEFAULT_TTL_SECS, PREVIEW_GRANT_ENV, SESSION_BROKER_SOCK_ENV,
};
use nostr::{Keys, PublicKey};
use uuid::Uuid;

use crate::config::Config;

/// The broker socket's file name inside the per-instance state root.
const BROKER_SOCKET_NAME: &str = "session-broker.sock";

/// The dev app's Tauri identifier; a worktree instance appends `.<name>`.
/// Mirrors `desktop/src-tauri/src/migration/identifiers.rs`.
const DEV_APP_IDENTIFIER: &str = "io.agiterra.beekeeper.app.dev";

/// The host's instance variable (`beekeeper_host_core::layout::INSTANCE_VAR`),
/// read only when the state directory does not say which instance this is.
const HOST_INSTANCE_ENV: &str = "BEEKEEPER_HOST_INSTANCE";

/// Set once the "no grant" disclosure has been logged, so a provider without
/// an attestation says it once rather than at every spawn.
static NO_GRANT_LOGGED: AtomicBool = AtomicBool::new(false);

/// Why no grant was pushed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PreviewEnvSkip {
    /// No NIP-OA attestation, or one whose owner is not a pubkey.
    NoOwner,
    /// `$HOME` is unknown and no explicit socket path was given.
    NoSocket,
    /// The core refused to mint (bounds on the target or execution id).
    Mint(String),
}

/// Append the preview grant and broker socket to `env`, the execution's
/// post-fence environment. Pushes both or neither; when neither, logs the
/// reason once per process and `bee preview` reports `preview_no_grant`.
pub(crate) fn push_preview_env(
    env: &mut Vec<(String, String)>,
    config: &Config,
    channel_id: Uuid,
    target: &CodingSessionTarget,
    execution_id: &str,
) {
    let socket = broker_socket_path(
        std::env::var_os(SESSION_BROKER_SOCK_ENV),
        std::env::var_os("HOME").map(PathBuf::from),
        std::env::var_os(HOST_INSTANCE_ENV),
        &config.state_dir,
    );
    let owner = attested_owner(config.auth_tag.as_ref());
    match preview_env(
        &config.keys,
        owner,
        socket,
        channel_id,
        target,
        execution_id,
        crate::state::now_secs(),
    ) {
        Ok(pairs) => {
            tracing::debug!(
                target: "csp::preview",
                execution_id,
                session_id = %target.session_id,
                "preview grant minted"
            );
            env.extend(pairs);
        }
        Err(skip) => {
            if !NO_GRANT_LOGGED.swap(true, Ordering::Relaxed) {
                tracing::warn!(
                    target: "csp::preview",
                    execution_id,
                    reason = ?skip,
                    "no preview grant for executions of this provider; `bee preview` \
                     will report preview_no_grant"
                );
            }
        }
    }
}

/// The two variables for one execution, or why there are none. Pure: every
/// input is passed in, so tests need neither the process environment nor a
/// clock.
pub(crate) fn preview_env(
    provider: &Keys,
    owner: Option<PublicKey>,
    socket: Option<PathBuf>,
    channel_id: Uuid,
    target: &CodingSessionTarget,
    execution_id: &str,
    now: u64,
) -> Result<[(String, String); 2], PreviewEnvSkip> {
    let owner = owner.ok_or(PreviewEnvSkip::NoOwner)?;
    let socket = socket.ok_or(PreviewEnvSkip::NoSocket)?;
    let token = mint_preview_grant(
        provider,
        &PreviewGrantRequest {
            channel_id,
            target: target.clone(),
            execution_id: execution_id.to_owned(),
            audience: preview_grant_audience(&owner),
            ttl_secs: PREVIEW_GRANT_DEFAULT_TTL_SECS,
        },
        now,
    )
    .map_err(|error| PreviewEnvSkip::Mint(error.code().to_owned()))?;
    Ok([
        (PREVIEW_GRANT_ENV.to_owned(), token),
        (
            SESSION_BROKER_SOCK_ENV.to_owned(),
            socket.to_string_lossy().into_owned(),
        ),
    ])
}

/// The owner pubkey in a NIP-OA attestation tag (`["auth", <owner hex>, …]`).
pub(crate) fn attested_owner(auth_tag: Option<&nostr::Tag>) -> Option<PublicKey> {
    match auth_tag?.as_slice() {
        [name, owner, ..] if name == "auth" => PublicKey::from_hex(owner).ok(),
        _ => None,
    }
}

/// This machine's desktop broker socket, absolute.
///
/// An explicit `BEEKEEPER_SESSION_BROKER_SOCK` in the provider's own environment
/// wins (the desktop honours the same override). Otherwise
/// `~/.local/state/buzz-dev/session-broker.sock` for a dev instance and
/// `~/.local/state/buzz/session-broker.sock` for production — the rule the
/// desktop's `shell_sessions::state_dir` applies. The instance is read from
/// the state directory, `<app data dir>/session-provider/<pubkey>`, whose
/// app-data name is the Tauri identifier; a state directory of another shape
/// (a server host) falls back to `BEEKEEPER_HOST_INSTANCE`, then production.
pub(crate) fn broker_socket_path(
    explicit: Option<OsString>,
    home: Option<PathBuf>,
    host_instance: Option<OsString>,
    state_dir: &Path,
) -> Option<PathBuf> {
    if let Some(explicit) = explicit.filter(|value| !value.is_empty()) {
        let path = PathBuf::from(explicit);
        return path.is_absolute().then_some(path);
    }
    let dev = match crate::agent_fence::app_data_dir_from_state_dir(state_dir) {
        Some(app_data) => app_data
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(is_dev_app_identifier),
        None => host_instance
            .and_then(|value| value.into_string().ok())
            .is_some_and(|value| {
                matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "dev" | "development"
                )
            }),
    };
    let namespace = if dev { "buzz-dev" } else { "buzz" };
    let home = home.filter(|home| home.is_absolute())?;
    Some(
        home.join(".local/state")
            .join(namespace)
            .join(BROKER_SOCKET_NAME),
    )
}

fn is_dev_app_identifier(name: &str) -> bool {
    name == DEV_APP_IDENTIFIER
        || name
            .strip_prefix(DEV_APP_IDENTIFIER)
            .is_some_and(|rest| rest.starts_with('.'))
}

#[cfg(test)]
#[path = "preview_grant_tests.rs"]
mod tests;
