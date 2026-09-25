//! Project-service admission: this computer's session provider may serve a
//! project only once the relay lets the provider's own key read it
//! (ledger 266).
//!
//! The provider signs this host's session work (kind 46014/46023, host steps,
//! verify results) under its own key, never the person's. A private
//! project's roster must name that key, or the relay's read gate withholds
//! the project's kind:30621 and every repository event — relay-signed
//! kind:30618 ref state included — from the host
//! (`crates/buzz-db/src/git_repo.rs::hidden_repos_for_reader`,
//! `crates/buzz-core/src/kind.rs::project_container_hidden_from` and
//! `repo_event_hidden_from`). Three control runs in a row (2026-09-24/25)
//! found the host off the roster: two earlier repairs hung off a provider
//! *start*, and a provider already running since app launch never passed
//! through one.
//!
//! So admission is a step of **acquiring a host**, not of starting one:
//! [`admit_host_for_session`] is the one seam, run by every founding path
//! (the create flow's `admit_this_computer_to_project`, the team-setup
//! launch and lead activation through
//! [`ensure_host_serving_project`](super::project_roster::ensure_host_serving_project)),
//! by project creation (`project_verify_setup`) as pre-provisioning, and by
//! every provider start for the projects this computer has a saved
//! association with ([`reconcile_saved_project_admissions`]). The answer is
//! one of four named states, logged every time and never collapsed into
//! "nothing happened": [`HostAdmission`].
//!
//! Roster authority is unchanged: only the project's creator or a roster
//! owner adds a member, a key this computer holds is never used to sweep
//! projects it has no association with, and a host an owner removed (a
//! kind:9011 naming it, newer than any kind:9010 that did) is never put back
//! silently.

use std::future::Future;
use std::time::Duration;

use nostr::{Event, Keys};
use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::app_state::AppState;
use crate::managed_agents::project_roster::{
    lower_hex64, put_project_agents, read_project_head, read_project_roster, roster_write_refusal,
    split_coordinate,
};
use buzz_core_pkg::kind::{
    is_private_project_event, KIND_PROJECT, KIND_PROJECT_PUT_MEMBER, KIND_PROJECT_REMOVE_MEMBER,
    PROJECT_ROLE_COLLABORATOR,
};

/// Whether this computer's session provider may serve a project, as proven
/// against the relay. Serialized for the webview as
/// `{"state": "public" | "admitted" | "unauthorized" | "unreadable", ...}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum HostAdmission {
    /// The project is public: its repositories are not gated on roster
    /// membership, so nothing was published.
    Public,
    /// The host's key is on the private project's roster **and** a read of
    /// the project made as the host succeeded. `already` is true when no
    /// roster op was needed.
    Admitted {
        /// The host was already on the roster; nothing was published.
        already: bool,
    },
    /// The host is not on the roster and this computer's person may not put
    /// it there (not the creator or an owner), or an owner removed it.
    Unauthorized {
        /// Why, in words.
        reason: String,
    },
    /// The relay could not be read or refused a write, or the host still
    /// cannot read the project after admission.
    Unreadable {
        /// Why, in words.
        reason: String,
    },
}

impl HostAdmission {
    fn unreadable(reason: impl Into<String>) -> Self {
        Self::Unreadable {
            reason: reason.into(),
        }
    }
}

/// How often the host's read is retried after admission: the relay projects
/// a roster op into its ACL as a side effect of ingest, which can trail the
/// op's `OK` by a moment.
const VERIFY_ATTEMPTS: u32 = 3;
const VERIFY_BACKOFF: Duration = Duration::from_millis(400);

/// The reason reported when the roster names the host but the host's own
/// read still comes back without the project.
pub(crate) const ADMITTED_BUT_UNREAD: &str = "admitted but the host cannot read the project yet";

/// Whether the newest roster op naming `host` for `coordinate` removed it.
fn newest_op_removed(events: &[Event], coordinate: &str, host: &str) -> bool {
    events
        .iter()
        .filter(|event| {
            let kind = u32::from(event.kind.as_u16());
            (kind == KIND_PROJECT_PUT_MEMBER || kind == KIND_PROJECT_REMOVE_MEMBER)
                && event.tags.iter().any(|tag| {
                    let parts = tag.as_slice();
                    parts.first().map(String::as_str) == Some("a")
                        && parts.get(1).map(String::as_str) == Some(coordinate)
                })
                && event.tags.iter().any(|tag| {
                    let parts = tag.as_slice();
                    parts.first().map(String::as_str) == Some("p")
                        && parts.get(1).map(|p| p.eq_ignore_ascii_case(host)) == Some(true)
                })
        })
        .max_by_key(|event| (event.created_at, event.id))
        .is_some_and(|event| u32::from(event.kind.as_u16()) == KIND_PROJECT_REMOVE_MEMBER)
}

/// Admit `host_pubkey` to `project_ref` as `owner_keys` (this computer's
/// person), then prove it with `verify_host_read` — a read of the project's
/// kind:30621 made **as the host** that answers whether the head came back.
///
/// 1. The head is read; a public project is [`HostAdmission::Public`].
/// 2. The roster is read; a host already on it skips to verification.
/// 3. A host an owner removed is [`HostAdmission::Unauthorized`] — never
///    re-added silently.
/// 4. A person who is neither creator nor owner is
///    [`HostAdmission::Unauthorized`]; nothing is published.
/// 5. Otherwise one kind:9010 puts the host on as a collaborator.
/// 6. [`HostAdmission::Admitted`] only once the host's read succeeds.
///
/// Every relay failure is [`HostAdmission::Unreadable`] with its words.
pub(crate) async fn admit_host_to_project<F, Fut>(
    state: &AppState,
    owner_keys: &Keys,
    project_ref: &str,
    host_pubkey: &str,
    verify_host_read: F,
) -> HostAdmission
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<bool, String>>,
{
    let (owner, dtag) = match split_coordinate(project_ref) {
        Ok(parts) => parts,
        Err(error) => return HostAdmission::unreadable(error),
    };
    let host = match lower_hex64(host_pubkey) {
        Ok(host) => host,
        Err(error) => return HostAdmission::unreadable(error),
    };
    let coordinate = format!("{KIND_PROJECT}:{owner}:{dtag}");
    let head = match read_project_head(state, &owner, &dtag).await {
        Ok(Some(head)) => head,
        Ok(None) => {
            return HostAdmission::unreadable(format!(
                "the relay returned no project head for {coordinate}"
            ))
        }
        Err(error) => return HostAdmission::unreadable(error),
    };
    if !is_private_project_event(&head) {
        return HostAdmission::Public;
    }
    let roster = match read_project_roster(state, project_ref).await {
        Ok(roster) => roster,
        Err(error) => return HostAdmission::unreadable(error),
    };
    let already = host == owner || roster.iter().any(|(member, _)| *member == host);
    if !already {
        let ops = match crate::relay::query_relay(
            state,
            &[serde_json::json!({
                "kinds": [KIND_PROJECT_PUT_MEMBER, KIND_PROJECT_REMOVE_MEMBER],
                "#a": [coordinate],
                "limit": 500,
            })],
        )
        .await
        {
            Ok(ops) => ops,
            Err(error) => {
                return HostAdmission::unreadable(format!(
                    "could not read the project's roster history: {error}"
                ))
            }
        };
        if newest_op_removed(&ops, &coordinate, &host) {
            return HostAdmission::Unauthorized {
                reason: "an owner removed this computer from the project's roster; only an \
                         owner can add it back"
                    .to_string(),
            };
        }
        let viewer = owner_keys.public_key().to_hex();
        if let Some(refusal) = roster_write_refusal(project_ref, &viewer, &roster) {
            return HostAdmission::Unauthorized { reason: refusal };
        }
        if let Err(error) = put_project_agents(
            state,
            owner_keys,
            project_ref,
            &[(host.clone(), PROJECT_ROLE_COLLABORATOR)],
        )
        .await
        {
            return HostAdmission::unreadable(format!(
                "the relay refused the roster update: {error}"
            ));
        }
    }
    let mut last_error: Option<String> = None;
    for attempt in 0..VERIFY_ATTEMPTS {
        if attempt > 0 {
            tokio::time::sleep(VERIFY_BACKOFF).await;
        }
        match verify_host_read().await {
            Ok(true) => return HostAdmission::Admitted { already },
            Ok(false) => last_error = None,
            Err(error) => last_error = Some(error),
        }
    }
    HostAdmission::unreadable(match last_error {
        Some(error) => format!("{ADMITTED_BUT_UNREAD}: {error}"),
        None => ADMITTED_BUT_UNREAD.to_string(),
    })
}

/// Read `owner`'s kind:30621 `dtag` **as the host** (`host_keys`, with its
/// NIP-OA `auth_tag`), the key the relay's private-project read gate judges.
/// `Ok(true)` when the head came back.
pub(crate) async fn host_can_read_project(
    state: &AppState,
    host_keys: &Keys,
    auth_tag: Option<&str>,
    owner: &str,
    dtag: &str,
) -> Result<bool, String> {
    let events = crate::relay::query_relay_at_with_keys(
        state,
        &crate::relay::relay_api_base_url_with_override(state),
        &[serde_json::json!({
            "kinds": [KIND_PROJECT],
            "authors": [owner],
            "#d": [dtag],
            "limit": 1,
        })],
        host_keys,
        auth_tag,
    )
    .await?;
    Ok(events.iter().any(|event| {
        u32::from(event.kind.as_u16()) == KIND_PROJECT
            && event.pubkey.to_hex() == owner
            && event.tags.iter().any(|tag| {
                let parts = tag.as_slice();
                parts.first().map(String::as_str) == Some("d")
                    && parts.get(1).map(String::as_str) == Some(dtag)
            })
    }))
}

fn log_admission(project_ref: &str, host: &str, admission: &HostAdmission) {
    const TARGET: &str = "managed_agents::project_admission";
    match admission {
        HostAdmission::Public => tracing::info!(
            target: TARGET, project = %project_ref, host = %host,
            "host admission: public — the project is not roster-gated"
        ),
        HostAdmission::Admitted { already } => tracing::info!(
            target: TARGET, project = %project_ref, host = %host, already = *already,
            "host admission: admitted — the host read the project as itself"
        ),
        HostAdmission::Unauthorized { reason } => tracing::warn!(
            target: TARGET, project = %project_ref, host = %host,
            "host admission: unauthorized — {reason}"
        ),
        HostAdmission::Unreadable { reason } => tracing::warn!(
            target: TARGET, project = %project_ref, host = %host,
            "host admission: unreadable — {reason}"
        ),
    }
}

/// **The seam.** Admit this computer's session provider for `relay_url` to
/// `project_ref`, whatever state the provider is in, and log the result.
///
/// The host key and its NIP-OA tag come from the provider record this
/// desktop provisioned, so the verification read is made as the host itself.
/// `expected_provider` is the provider a caller already resolved for the
/// session; a different record on disk is reported, not papered over.
pub(crate) async fn admit_host_for_session(
    app: &AppHandle,
    state: &AppState,
    owner_keys: &Keys,
    project_ref: &str,
    relay_url: &str,
    expected_provider: Option<&str>,
) -> HostAdmission {
    let admission = admit_with_record(
        app,
        state,
        owner_keys,
        project_ref,
        relay_url,
        expected_provider,
    )
    .await;
    log_admission(
        project_ref,
        expected_provider.unwrap_or("(this computer's provider)"),
        &admission,
    );
    admission
}

async fn admit_with_record(
    app: &AppHandle,
    state: &AppState,
    owner_keys: &Keys,
    project_ref: &str,
    relay_url: &str,
    expected_provider: Option<&str>,
) -> HostAdmission {
    let store = match crate::session_provider::store::load_provider_store(app) {
        Ok(store) => store,
        Err(error) => return HostAdmission::unreadable(error),
    };
    let Some(record) = store.get(relay_url) else {
        return HostAdmission::unreadable(
            "this computer has no session provider for this relay yet",
        );
    };
    if let Some(expected) = expected_provider {
        if !record.provider_pubkey.eq_ignore_ascii_case(expected.trim()) {
            return HostAdmission::unreadable(format!(
                "the session names provider {expected}, but this computer's provider for this \
                 relay is {}",
                record.provider_pubkey
            ));
        }
    }
    if record.private_key_nsec.is_empty() {
        return HostAdmission::unreadable(
            "this computer's provider key is not available (the OS keyring may be locked), so \
             the host cannot prove it can read the project",
        );
    }
    let host_keys = match Keys::parse(&record.private_key_nsec) {
        Ok(keys) => keys,
        Err(error) => {
            return HostAdmission::unreadable(format!("the provider key does not parse: {error}"))
        }
    };
    let Ok((owner, dtag)) = split_coordinate(project_ref) else {
        return HostAdmission::unreadable(format!("{project_ref:?} is not a project coordinate"));
    };
    let auth_tag = record.auth_tag.clone();
    admit_host_to_project(
        state,
        owner_keys,
        project_ref,
        &record.provider_pubkey,
        || host_can_read_project(state, &host_keys, auth_tag.as_deref(), &owner, &dtag),
    )
    .await
}

/// After a provider start: run [`admit_host_for_session`] for every project
/// this computer has a saved association with, in the background. A
/// provider that has been running since app launch is reconciled here, not
/// only when a session's start path happens to run.
pub(crate) fn reconcile_saved_project_admissions(app: &AppHandle, relay_url: &str) {
    let app = app.clone();
    let relay_url = relay_url.to_string();
    tauri::async_runtime::spawn(async move {
        let Some(state) = app.try_state::<AppState>() else {
            return;
        };
        let owner_keys = match state.signing_keys() {
            Ok(keys) => keys,
            Err(error) => {
                tracing::warn!(
                    target: "managed_agents::project_admission",
                    "host admission reconcile skipped: {error}"
                );
                return;
            }
        };
        let projects =
            match crate::coding_sessions::workdir_store::saved_association_project_refs(&app) {
                Ok(projects) => projects,
                Err(error) => {
                    tracing::warn!(
                        target: "managed_agents::project_admission",
                        "host admission reconcile could not read saved associations: {error}"
                    );
                    return;
                }
            };
        for project in projects {
            admit_host_for_session(&app, &state, &owner_keys, &project, &relay_url, None).await;
        }
    });
}

/// Admit this computer to `project_ref` before a session is founded or
/// joined on it, and say which of the four states resulted.
#[tauri::command]
pub async fn admit_this_computer_to_project(
    app: AppHandle,
    state: State<'_, AppState>,
    project_ref: String,
    expected_relay_url: Option<String>,
) -> Result<HostAdmission, String> {
    let relay_url = crate::session_provider::commands::provider_command_relay(
        &state,
        expected_relay_url.as_deref(),
    )?;
    let owner_keys = state.signing_keys()?;
    Ok(admit_host_for_session(
        &app,
        &state,
        &owner_keys,
        project_ref.trim(),
        &relay_url,
        None,
    )
    .await)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::{EventBuilder, Kind, Tag};

    fn op(kind: u32, coordinate: &str, host: &str, at: u64) -> Event {
        EventBuilder::new(Kind::Custom(kind as u16), "")
            .tags(vec![
                Tag::parse(vec!["a".to_string(), coordinate.to_string()]).unwrap(),
                Tag::parse(vec!["p".to_string(), host.to_string()]).unwrap(),
            ])
            .custom_created_at(nostr::Timestamp::from(at))
            .sign_with_keys(&Keys::generate())
            .unwrap()
    }

    #[test]
    fn only_a_newest_removal_counts_as_a_revocation() {
        let coord = "30621:aa:demo";
        let host = "c".repeat(64);
        assert!(!newest_op_removed(&[], coord, &host));
        let removed = op(KIND_PROJECT_REMOVE_MEMBER, coord, &host, 20);
        let put = op(KIND_PROJECT_PUT_MEMBER, coord, &host, 10);
        assert!(newest_op_removed(
            &[put.clone(), removed.clone()],
            coord,
            &host
        ));
        let readded = op(KIND_PROJECT_PUT_MEMBER, coord, &host, 30);
        assert!(!newest_op_removed(
            &[put, removed.clone(), readded],
            coord,
            &host
        ));
        // Another project's removal, or another key's, is not this one's.
        assert!(!newest_op_removed(
            std::slice::from_ref(&removed),
            "30621:aa:other",
            &host
        ));
        assert!(!newest_op_removed(&[removed], coord, &"d".repeat(64)));
    }

    #[test]
    fn the_four_states_serialize_with_a_named_state() {
        let json = |a: &HostAdmission| serde_json::to_value(a).unwrap();
        assert_eq!(
            json(&HostAdmission::Public),
            serde_json::json!({"state": "public"})
        );
        assert_eq!(
            json(&HostAdmission::Admitted { already: false }),
            serde_json::json!({"state": "admitted", "already": false})
        );
        assert_eq!(
            json(&HostAdmission::Unauthorized { reason: "r".into() }),
            serde_json::json!({"state": "unauthorized", "reason": "r"})
        );
        assert_eq!(
            json(&HostAdmission::Unreadable { reason: "r".into() }),
            serde_json::json!({"state": "unreadable", "reason": "r"})
        );
    }
}
