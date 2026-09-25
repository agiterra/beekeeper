//! Project setup's one standing consent (ledger 248, 252; plan "Approval"
//! ruling; Brian's 2026-09-25 ruling: **creation is consent**).
//!
//! A new project's agents repository seeds an **active** manual `verify`
//! action (`buzz_persona::seed::seeded_actions_yml_with_verify`). Before any
//! team starts, setup publishes that definition (kind 30620) so its
//! `definition_hash` exists, and — right after — publishes the standing
//! grant for it directly (kind:46030, `scope: action`,
//! `handle_standing_approval_grant` in
//! `crates/buzz-relay/src/handlers/command_executor.rs`) bound to
//! `(workflow id, definition_hash)`, with this same owner key. Creating a
//! project on this computer, with the owner's key, already says "this
//! computer may run this project's actions"; there is no separate click to
//! wait for. [`ensure_standing_grant`] is idempotent: it reads
//! `GET /workflows/{id}/autorun` first and publishes nothing when an active
//! grant for the exact hash already exists, reporting that grant's id
//! instead.
//!
//! Control run 6 (2026-09-24) found the prior shape started a real run at
//! the code repository's **empty seed commit** purely to park it on a
//! synthetic approval gate and manufacture a kind:46010 for a click to
//! answer — a run nothing had asked for, gated on a host provider that setup
//! never starts (`session_provider::supervisor::ensure_running` is only
//! called from session launch), so it sat unclaimed and then ran red on a
//! commit with no tests. Run 7 (2026-09-25) then found that asking a second
//! time for consent creation itself already gives was its own kind of
//! dishonesty. Setup now asks nobody: no trigger, no run, no host-step
//! ceremony, and no consent card either — the two remaining places it can
//! still appear (`ProjectVerifyConsent`, `ProjectActionCard`) show only when
//! this host holds no active grant for the workflow's current hash: a
//! foreign host that did not create the project, or a definition edited
//! after the grant.
//!
//! What this consent is and is not: it is the person whose computer runs the
//! command agreeing to *this definition* running there — resource consent,
//! not supervision (spec § 5.4). A routine run of the unchanged definition,
//! from any seat, asks nobody; an edited definition hashes differently and
//! asks again (from whichever host publishes the edit, since that host's key
//! holds no grant for the new hash). The publication lands on the same
//! workflow id `bee actions publish` uses (`action_workflow_id`), because
//! the grant binds `(workflow id, hash)`.

use serde::Serialize;
use tauri::State;

use crate::app_state::AppState;

/// The action setup publishes and asks consent for.
pub(crate) const VERIFY_ACTION: &str = "verify";

/// What setup will publish for the seeded `verify`, derived from the file's
/// own bytes.
#[derive(Debug)]
pub(crate) struct VerifyPublication {
    /// `action_workflow_id(project, "verify")`.
    pub workflow_id: uuid::Uuid,
    /// The definition with `project` bound, as the relay receives it.
    pub yaml: String,
    /// Lowercase hex hash of the definition — what the grant binds.
    pub definition_hash: String,
    /// The `run_on_host` argv, for the result to show.
    pub command: Vec<String>,
}

/// Parse `actions_yml` for `project` and plan the `verify` publication.
///
/// # Errors
/// The file does not parse, names no `verify`, or its `verify` runs nothing
/// on a host (so no host-step grant could cover it).
pub(crate) fn plan_verify_publication(
    project: &str,
    actions_yml: &str,
) -> Result<VerifyPublication, String> {
    let entries = buzz_workflow_pkg::actions_file::parse_actions_yml(actions_yml, project)
        .map_err(|error| format!("the seeded actions.yml does not parse: {error}"))?;
    let entry = entries
        .iter()
        .find(|entry| entry.name == VERIFY_ACTION)
        .ok_or_else(|| "the seeded actions.yml has no `verify` action".to_string())?;
    let command = entry
        .def
        .steps
        .iter()
        .find_map(|step| match &step.action {
            buzz_workflow_pkg::ActionDef::RunOnHost { command, .. } => Some(command.clone()),
            _ => None,
        })
        .ok_or_else(|| "the seeded `verify` action runs nothing on a host".to_string())?;
    let yaml = buzz_workflow_pkg::actions_file::bound_definition_yaml(entry)
        .map_err(|error| format!("the `verify` action could not be serialized: {error}"))?;
    Ok(VerifyPublication {
        workflow_id: buzz_workflow_pkg::actions_file::action_workflow_id(project, VERIFY_ACTION),
        yaml,
        definition_hash: entry.hash.clone(),
        command,
    })
}

/// The channel setup creates to file `verify` in, when this key has not
/// already published it somewhere.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ActionsChannelSpec {
    pub name: String,
    pub visibility: &'static str,
    pub channel_type: &'static str,
    pub about: String,
}

/// Where setup files the project's actions (ledger 252).
///
/// A workflow's relay-signed events (kind:46010, kind:46013, kind:46014) are
/// `h`-scoped to the channel the workflow is filed in, and a host claims a
/// step only as a reader of that channel. So `verify` goes in the project's
/// sessions transport — the channel every host serving the project and every
/// seat already reads through the project ACL — named and described exactly
/// as `desktop/src/features/projects-container/lib/projectSessionsChannel.ts`
/// names it, so a later session create resolves this channel (rule 0) instead
/// of minting a second transport. A private owner-only channel here hid every
/// host-step request from every host (control run 3).
pub(crate) fn actions_channel_spec(project_name: &str, slug: &str) -> ActionsChannelSpec {
    let collapsed = project_name
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let base = if collapsed.is_empty() {
        slug.trim().to_owned()
    } else {
        collapsed
    };
    ActionsChannelSpec {
        name: format!("{base} sessions"),
        visibility: "private",
        channel_type: "transport",
        about: format!("Coding sessions for {base}."),
    }
}

/// What `project_verify_setup` did, step by step; each field is `null` when
/// its step did not happen, and `error` names the step that stopped.
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectVerifySetup {
    pub workflow_id: Option<String>,
    pub channel_id: Option<String>,
    pub definition_hash: Option<String>,
    pub command: Vec<String>,
    pub publish_event_id: Option<String>,
    /// The definition was already published by this key into `channel_id`.
    pub channel_reused: bool,
    /// The code repository's seed commit, informational only — no run is
    /// bound to it. Reported so the consent question can name it.
    pub checkout: Option<String>,
    /// The standing grant's event id — this run's own publish, or an
    /// existing active grant [`ensure_standing_grant`] found instead of
    /// publishing a second one. `None` when [`Self::error`] or
    /// [`Self::grant_error`] explains why no grant exists.
    pub grant_event_id: Option<String>,
    /// The step that stopped the grant, in the host's words, when
    /// `definition_hash` published but no grant could be confirmed or
    /// published for it. Distinct from [`Self::error`], which stops setup
    /// before a definition even exists to grant.
    pub grant_error: Option<String>,
    pub error: Option<String>,
}

/// Publish the seeded `verify` definition and, right after, the standing
/// grant that answers it (creation is consent — see the module docs). No run
/// happens here.
///
/// `actions_yml` is `ProjectAgentsInit.seededActionsYml`, the bytes this
/// computer seeded and pushed; `checkout` is the code repository's seed
/// commit; `project_name` names the sessions channel. The definition lives in
/// the project's sessions transport ([`actions_channel_spec`]), reused when
/// this key already published it.
#[tauri::command]
pub async fn project_verify_setup(
    state: State<'_, AppState>,
    project_ref: String,
    project_name: String,
    actions_yml: String,
    checkout: String,
) -> Result<ProjectVerifySetup, String> {
    project_verify_setup_with_state(&state, project_ref, project_name, actions_yml, checkout).await
}

/// [`project_verify_setup`]'s body, taking `&AppState` directly instead of a
/// [`State`] Tauri cannot construct outside a real app. Tests drive this
/// against a stub relay (`build_app_state()` plus `relay_url_override`), the
/// same split [`super::packs_repo::project_packs_init`] documents.
pub(crate) async fn project_verify_setup_with_state(
    state: &AppState,
    project_ref: String,
    project_name: String,
    actions_yml: String,
    checkout: String,
) -> Result<ProjectVerifySetup, String> {
    let project = project_ref.trim().to_string();
    let (_, slug) = super::packs_repo::parse_project_coordinate(&project)?;
    let plan = plan_verify_publication(&project, &actions_yml)?;
    let mut result = ProjectVerifySetup {
        workflow_id: Some(plan.workflow_id.to_string()),
        definition_hash: Some(plan.definition_hash.clone()),
        command: plan.command.clone(),
        checkout: Some(checkout.trim().to_ascii_lowercase()),
        ..ProjectVerifySetup::default()
    };
    let keys = state.signing_keys()?;
    let me = keys.public_key().to_hex();

    // A retry must land in the channel the first attempt used: the relay
    // refuses an update of a workflow from a different channel.
    let existing = crate::relay::query_relay(
        state,
        &[serde_json::json!({
            "kinds": [30620],
            "authors": [me],
            "#d": [plan.workflow_id.to_string()],
            "limit": 1,
        })],
    )
    .await
    .unwrap_or_default();
    let reused = existing.first().and_then(|event| {
        event
            .tags
            .iter()
            .find(|tag| tag.as_slice().first().map(String::as_str) == Some("h"))
            .and_then(|tag| tag.as_slice().get(1).cloned())
    });
    let channel_id = match reused {
        Some(channel) => {
            result.channel_reused = true;
            channel
        }
        None => {
            let channel_uuid = uuid::Uuid::new_v4();
            let spec = actions_channel_spec(&project_name, &slug);
            let create = |channel_type: &str| {
                crate::events::build_create_channel(
                    channel_uuid,
                    &spec.name,
                    spec.visibility,
                    channel_type,
                    Some(&spec.about),
                    None,
                    Some(&project),
                )
            };
            let submit = |builder| async {
                match crate::relay::submit_event(builder, state).await {
                    Ok(response) if response.accepted => Ok(()),
                    Ok(response) => Err(response.message),
                    Err(error) => Err(error),
                }
            };
            let mut created = submit(create(spec.channel_type)?).await;
            // A relay that predates the transport type: the same legacy
            // fallback the session founder takes.
            if matches!(&created, Err(error) if error.contains("invalid channel_type")) {
                created = submit(create("stream")?).await;
            }
            if let Err(error) = created {
                result.error = Some(format!("the sessions channel was not created: {error}"));
                return Ok(result);
            }
            state.mark_pending_owned_channel(&me, &channel_uuid.to_string());
            channel_uuid.to_string()
        }
    };
    result.channel_id = Some(channel_id.clone());
    let channel_uuid = uuid::Uuid::parse_str(&channel_id)
        .map_err(|_| format!("the actions channel id {channel_id:?} is not a uuid"))?;

    // Membership of a fresh channel can trail its creation by a moment.
    let mut last_error = String::new();
    for attempt in 0..4u64 {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(400 * attempt)).await;
        }
        let builder = buzz_sdk_pkg::build_project_workflow_def(
            channel_uuid,
            plan.workflow_id,
            &project,
            &plan.yaml,
        )
        .map_err(|error| error.to_string())?;
        match crate::relay::submit_event(builder, state).await {
            Ok(response) if response.accepted => {
                result.publish_event_id = Some(response.event_id);
                last_error.clear();
                break;
            }
            Ok(response) => last_error = response.message,
            Err(error) => last_error = error,
        }
        if !last_error.contains("not a member") {
            break;
        }
    }
    if result.publish_event_id.is_none() {
        result.error = Some(format!("the verify action was not published: {last_error}"));
        return Ok(result);
    }

    // Creation is consent (Brian's 2026-09-25 ruling): publish the standing
    // grant for exactly the hash just published, with the same owner key,
    // right after — not a second click to wait for.
    match ensure_standing_grant(state, &plan.workflow_id.to_string(), &plan.definition_hash).await {
        Ok(event_id) => result.grant_event_id = Some(event_id),
        Err(error) => result.grant_error = Some(error),
    }
    Ok(result)
}

/// The definition hash of an active, unrevoked grant this relay already
/// records — decoded from `GET /workflows/{id}/autorun`'s minimal shape,
/// ignoring every field [`ensure_standing_grant`] does not need.
#[derive(serde::Deserialize)]
struct AutorunGrantRead {
    matches_current: bool,
    revoked_at: Option<String>,
    grant_event_id: String,
}

/// Publish the standing grant for `(workflow_id, definition_hash)`, unless
/// an active, unrevoked grant for that exact hash already exists — in which
/// case nothing is published and that grant's event id is returned instead.
///
/// A read failure (the relay's eventual-consistency window right after the
/// definition above just published, or a genuine network error) is not
/// distinguished from "no grant yet": consent was already given by creating
/// the project, so this fails open toward granting rather than open toward
/// asking again. The relay itself still refuses a grant from anyone but the
/// workflow's project owner (or its own owner pubkey), so failing open here
/// grants nothing a foreign host's key could not have asked for directly.
async fn ensure_standing_grant(
    state: &AppState,
    workflow_id: &str,
    definition_hash: &str,
) -> Result<String, String> {
    let existing: Result<serde_json::Value, String> =
        crate::relay::get_relay_json(state, &format!("/workflows/{workflow_id}/autorun")).await;
    if let Ok(read) = existing {
        let grants: Vec<AutorunGrantRead> = read
            .get("grants")
            .cloned()
            .map(serde_json::from_value)
            .transpose()
            .unwrap_or(None)
            .unwrap_or_default();
        if let Some(grant) = grants
            .into_iter()
            .find(|grant| grant.matches_current && grant.revoked_at.is_none())
        {
            return Ok(grant.grant_event_id);
        }
    }
    let builder = crate::events::build_standing_approval_grant(workflow_id, definition_hash, None)?;
    let response = crate::relay::submit_event(builder, state).await?;
    if !response.accepted {
        return Err(response.message);
    }
    Ok(response.event_id)
}
