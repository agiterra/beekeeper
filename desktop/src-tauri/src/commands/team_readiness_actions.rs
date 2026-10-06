//! One readiness row: can this session's team publish and trigger this
//! project's actions (ledger 186, finding 178(f))?
//!
//! # Why the panel has to say this before Start
//!
//! On 2026-09-20 a team session was founded on a routine goal — "build kettle
//! with tests and a verify action, land it on main" — and could not finish it.
//! Publishing the action (kind 30620) and triggering it (46020) are admitted
//! for the project's creator, a roster owner or an endorsed repository's
//! founder, and a lead seat is none of those. The lead did the right thing and
//! asked the founder for a ruling eleven minutes in. Readiness had said
//! "ready" without ever asking whether the team it was about to seat could do
//! the work.
//!
//! So readiness now asks, and answers with the fact that decides it:
//!
//! - the founder is one of this project's owners, so founding a team with
//!   roles signs the narrow project-action delegation for its lead
//!   ([`PROJECT_ACTIONS_DELEGABLE`]);
//! - the founder is not, so the lead will be refused, and the row names the
//!   keys that can grant it ([`PROJECT_ACTIONS_NOT_DELEGABLE`]);
//! - or it could not be read, which is said as exactly that
//!   ([`PROJECT_ACTIONS_AUTHORITY_UNKNOWN`]) rather than assumed either way.
//!
//! # Why this row warns and does not gate
//!
//! It is `wire` scope, so it never blocks Start (the local/wire split is
//! `teamReadinessModel.ts`'s `teamReadinessRequiredUnknownFact`). Plenty of
//! team sessions never touch an action, and a relay that cannot answer this
//! question is no reason to refuse work that does not need it — "gates must
//! earn their delay" (`VISION_COLLABORATION.md`). What it must never do is
//! stay silent and let a session be founded on a goal it cannot finish.

use serde_json::json;

use super::{TeamReadinessFact, TeamReadinessFactState, TeamReadinessResponse, TeamReadinessScope};

/// The founder owns the project, so Start signs the lead's delegation.
pub const PROJECT_ACTIONS_DELEGABLE: &str = "PROJECT_ACTIONS_DELEGABLE";
/// The founder owns nothing here, so no delegation can be signed at Start.
pub const PROJECT_ACTIONS_NOT_DELEGABLE: &str = "PROJECT_ACTIONS_NOT_DELEGABLE";
/// The project's ownership could not be read, so nothing is claimed.
pub const PROJECT_ACTIONS_AUTHORITY_UNKNOWN: &str = "PROJECT_ACTIONS_AUTHORITY_UNKNOWN";

/// What this computer could establish about the project's owners.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectActionOwnership {
    /// The project's owner keys, read and verified — the creator plus every
    /// `owner` row on the relay-signed roster.
    Owners(Vec<String>),
    /// Ownership could not be read. `detail` names what failed, verbatim.
    Unreadable { detail: String },
}

/// The row, decided from already-read facts. Pure, so it is testable.
///
/// `founder_pubkey` is the identity that would found the session — `None`
/// when this computer could not load its own key, which is itself an
/// unreadable answer rather than a "not an owner".
pub fn project_action_authority_fact(
    founder_pubkey: Option<&str>,
    ownership: &ProjectActionOwnership,
) -> TeamReadinessFact {
    let owners =
        match ownership {
            ProjectActionOwnership::Unreadable { detail } => return fact(
                PROJECT_ACTIONS_AUTHORITY_UNKNOWN,
                TeamReadinessFactState::Unknown,
                format!(
                    "Whether this session's team could publish and trigger this project's \
                     actions is unknown: {detail}"
                ),
                Some(
                    "Re-read this panel once the relay answers. A session founded meanwhile can \
                     still be refused when it tries to publish an action."
                        .into(),
                ),
            ),
            ProjectActionOwnership::Owners(owners) => owners,
        };
    let Some(founder) = founder_pubkey.map(str::to_ascii_lowercase) else {
        return fact(
            PROJECT_ACTIONS_AUTHORITY_UNKNOWN,
            TeamReadinessFactState::Unknown,
            "Whether this session's team could publish and trigger this project's actions is \
             unknown: this computer could not read the key it would found the session with"
                .into(),
            Some("Unlock the keychain and relaunch Beekeeper, then re-read this panel.".into()),
        );
    };
    if owners.contains(&founder) {
        return fact(
            PROJECT_ACTIONS_DELEGABLE,
            TeamReadinessFactState::Ready,
            "This session's lead may publish and trigger this project's actions: you own this \
             project, so starting the session signs that delegation for its lead"
                .into(),
            None,
        );
    }
    fact(
        PROJECT_ACTIONS_NOT_DELEGABLE,
        TeamReadinessFactState::Limited,
        format!(
            "This session's lead will be refused if it tries to publish or trigger this \
             project's actions: you are not one of its owners ({})",
            name_owners(owners)
        ),
        Some(
            "Ask one of this project's owners to start the session, or to sign the \
             project-action delegation for its lead. The team can still plan, build, review and \
             land; only publishing and running this project's actions is refused."
                .into(),
        ),
    )
}

/// The owners, named by the only identifier this read has: their keys.
///
/// Short keys rather than display names because a name would be a second
/// read this row does not make, and a wrong name beside an authority claim is
/// worse than an abbreviated key.
fn name_owners(owners: &[String]) -> String {
    if owners.is_empty() {
        return "this project's records name no owner at all".into();
    }
    let named = owners
        .iter()
        .take(3)
        .map(|owner| owner.chars().take(8).collect::<String>())
        .collect::<Vec<_>>()
        .join(", ");
    match owners.len() {
        1..=3 => format!("owner {named}"),
        n => format!("owners {named} and {} more", n - 3),
    }
}

fn fact(
    code: &str,
    state: TeamReadinessFactState,
    summary: String,
    remedy: Option<String>,
) -> TeamReadinessFact {
    TeamReadinessFact {
        category: "actions".into(),
        code: code.into(),
        scope: TeamReadinessScope::Wire,
        state,
        summary,
        remedy,
    }
}

/// Read the project's owners from the relay: the coordinate's creator plus
/// every `owner` row on the newest kind:39010 roster the community's own
/// metadata identity signed.
///
/// An unverifiable roster is *not* read as "no owners": a forged 39010 by any
/// author must not be able to make this row say Ready, and a roster that
/// cannot be checked must not be able to make it say Limited either. Both
/// become [`ProjectActionOwnership::Unreadable`].
pub async fn observe_project_action_ownership(
    app: &tauri::AppHandle,
    project_ref: &str,
) -> ProjectActionOwnership {
    use tauri::Manager;

    let state = app.state::<crate::app_state::AppState>();
    let Some((creator, dtag)) = split_project_coordinate(project_ref) else {
        return ProjectActionOwnership::Unreadable {
            detail: "this project's coordinate could not be read".into(),
        };
    };
    let base = crate::relay::relay_api_base_url_with_override(&state);
    let signer = match community_metadata_signer(&state, &base).await {
        Ok(signer) => signer,
        Err(detail) => return ProjectActionOwnership::Unreadable { detail },
    };
    let filter = json!({
        "kinds": [beekeeper_core_pkg::kind::KIND_PROJECT_MEMBERS],
        "authors": [signer],
        "#d": [project_ref],
        "limit": 8,
    });
    let events = match crate::relay::query_relay_at(&state, &base, &[filter]).await {
        Ok(events) => events,
        Err(error) => {
            return ProjectActionOwnership::Unreadable {
                detail: format!("the relay did not answer for this project's roster ({error})"),
            }
        }
    };
    let mut owners = vec![creator];
    // No roster is a real answer, not a failure: a project nobody has added a
    // member to has exactly one owner, its creator.
    if let Some(roster) = newest_verified_roster(&events, &signer, project_ref) {
        owners.extend(roster_owner_keys(roster));
    }
    let _ = dtag;
    owners.sort();
    owners.dedup();
    ProjectActionOwnership::Owners(owners)
}

/// `(creator-hex, d)` of a `30621:<owner>:<d>` coordinate.
fn split_project_coordinate(coordinate: &str) -> Option<(String, String)> {
    let mut parts = coordinate.splitn(3, ':');
    if parts.next()? != beekeeper_core_pkg::kind::KIND_PROJECT.to_string() {
        return None;
    }
    let owner = parts.next()?.to_ascii_lowercase();
    let dtag = parts.next()?;
    let owner_ok = owner.len() == 64
        && owner
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    (owner_ok && !dtag.trim().is_empty()).then(|| (owner, dtag.to_owned()))
}

/// The community's metadata signing identity, from NIP-11 `self`.
async fn community_metadata_signer(
    state: &crate::app_state::AppState,
    base: &str,
) -> Result<String, String> {
    let response = state
        .http_client
        .get(base)
        .header("Accept", "application/nostr+json")
        .send()
        .await
        .map_err(|error| format!("the relay's metadata could not be read ({error})"))?;
    if !response.status().is_success() {
        return Err(format!(
            "the relay's metadata answered {}",
            response.status()
        ));
    }
    let value: serde_json::Value = response
        .json()
        .await
        .map_err(|error| format!("the relay's metadata could not be read ({error})"))?;
    let signer = value
        .get("self")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "the relay's metadata names no signing identity".to_string())?;
    nostr::PublicKey::from_hex(signer)
        .map(|key| key.to_hex())
        .map_err(|_| "the relay's metadata signing identity is malformed".to_string())
}

/// The newest roster for this coordinate that the community's own identity
/// signed, with a valid signature and the exact `d` tag.
fn newest_verified_roster<'a>(
    events: &'a [nostr::Event],
    signer: &str,
    project_ref: &str,
) -> Option<&'a nostr::Event> {
    events
        .iter()
        .filter(|event| {
            event.kind.as_u16() as u32 == beekeeper_core_pkg::kind::KIND_PROJECT_MEMBERS
                && event.pubkey.to_hex() == signer
                && event.verify().is_ok()
                && event.tags.iter().any(|tag| {
                    matches!(tag.as_slice(), [name, value, ..] if name == "d" && value == project_ref)
                })
        })
        .max_by_key(|event| event.created_at.as_secs())
}

/// Every pubkey the roster gives the `owner` role.
fn roster_owner_keys(roster: &nostr::Event) -> Vec<String> {
    roster
        .tags
        .iter()
        .filter_map(|tag| {
            let parts = tag.as_slice();
            if parts.first().map(String::as_str) != Some("p") {
                return None;
            }
            // A `p` row with no role is a collaborator everywhere else this
            // roster is read, so an absent role is never an owner here.
            if parts.get(3).map(String::as_str)
                != Some(beekeeper_core_pkg::kind::PROJECT_ROLE_OWNER)
            {
                return None;
            }
            Some(parts.get(1)?.to_ascii_lowercase())
        })
        .collect()
}

/// Append the row to an already-folded readiness response.
///
/// Called after the wire fold so the row lands beside the other `wire` facts;
/// the caller re-summarizes afterwards, which is what keeps `limitedCodes`
/// and `unknownCodes` honest about it.
pub async fn append_project_action_authority_fact(
    app: &tauri::AppHandle,
    project_ref: &str,
    response: &mut TeamReadinessResponse,
) {
    let ownership = observe_project_action_ownership(app, project_ref).await;
    let founder = response.owner_pubkey.clone();
    response.facts.push(project_action_authority_fact(
        founder.as_deref(),
        &ownership,
    ));
}

#[cfg(test)]
#[path = "team_readiness_actions_tests.rs"]
mod tests;
