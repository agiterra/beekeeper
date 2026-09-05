//! Phase 2b/2c of runtime discovery: settle each entry's auth column once
//! the `auth status` probes are back. Split from `discovery.rs` for size.

use super::{auth_preflight, AuthPreflightState, AuthPreflightVerdict, PartialEntry};
use crate::managed_agents::{AcpAvailabilityStatus, AuthStatus};

/// Finish the auth column after the probe results have been patched in.
///
/// Phase 2b: `auth status` reads the credential file and never exercises
/// the token (finding 71: every turn failed "OAuth session expired" while
/// it said `loggedIn: true`). A live pre-flight verdict reached within the
/// last ten minutes overrides it; when there is none, one is warmed on a
/// detached thread so the catalog read itself never waits the 20s bound.
///
/// Phase 2c: entries that were never probed get `NotApplicable` (available,
/// with no probe defined) or stay `Unknown`.
pub(super) fn settle_auth_statuses(partials: &mut [PartialEntry]) {
    for partial in partials.iter_mut() {
        if partial.entry.auth_status != AuthStatus::LoggedIn
            || auth_preflight::preflight_command_for(partial.runtime.id).is_none()
        {
            continue;
        }
        match auth_preflight::cached_preflight_verdict(partial.runtime.id) {
            Some(AuthPreflightVerdict {
                state: AuthPreflightState::CredentialDead { sentence },
                ..
            }) => {
                partial.entry.auth_status = AuthStatus::LoggedOut;
                partial.entry.login_hint =
                    Some(auth_preflight::credential_dead_login_hint(&sentence));
            }
            Some(_) => {}
            None => {
                let runtime_id = partial.runtime.id;
                std::thread::spawn(move || {
                    let _ = auth_preflight::run_runtime_auth_preflight(runtime_id, false);
                });
            }
        }
    }

    // Fill NotApplicable / Unknown for non-probed entries.
    for partial in partials.iter_mut() {
        if partial.entry.auth_status == AuthStatus::Unknown {
            partial.entry.auth_status = if partial.entry.availability
                == AcpAvailabilityStatus::Available
                && partial.runtime.auth_probe_args.is_none()
            {
                AuthStatus::NotApplicable
            } else {
                AuthStatus::Unknown
            };
        }
    }
}
