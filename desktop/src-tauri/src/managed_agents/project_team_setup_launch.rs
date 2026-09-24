//! Durable launch of the ordinary setup actor through the existing local provider.
//! Relay acknowledgements never stand in for provider receipts.

use super::{
    actor, authoring, context, read_draft, verify_context, ProjectTeamSetupDraft, SetupError,
};
use crate::app_state::AppState;
use crate::session_provider::{commands, store, CodingSessionProviderState};
use buzz_core_pkg::coding_session_command::CodingSessionTarget;
use buzz_core_pkg::coding_session_identity::ProviderInstanceAlias;
use buzz_core_pkg::coding_session_lifecycle_command::{
    CodingSessionLifecycleAction, CodingSessionLifecycleCommandPayload,
    CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA,
};
use buzz_core_pkg::coding_session_payload::PackRef;
use nostr::{Event, Keys};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

#[path = "project_team_setup_launch_journal.rs"]
mod journal;
#[cfg(test)]
#[path = "project_team_setup_launch_tests.rs"]
mod tests;
#[path = "project_team_setup_launch_wire.rs"]
pub(crate) mod wire;

/// The full runtime choice; retries must keep every field unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LaunchChoice {
    pub provider_pubkey: String,
    pub provider_instance_ref: String,
    pub runtime: String,
    pub model: String,
}

/// Honest observations of a durable create, distinct from current process health.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchStatus {
    Prepared,
    AwaitingReceipt,
    Ambiguous,
    Created,
    InitialTurnFailed,
    Failed,
}

/// A durable request and its latest verified provider outcome.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupAuthoringLaunch {
    pub setup_id: String,
    pub session_ref: String,
    pub channel_id: String,
    pub create_command_id: String,
    #[serde(flatten)]
    pub choice: LaunchChoice,
    pub actor_pubkey: String,
    pub pack_ref: PackRef,
    pub status: LaunchStatus,
    pub message: Option<String>,
    pub target: Option<CodingSessionTarget>,
    pub receipt_event_id: Option<String>,
}

fn invalid(message: impl Into<String>) -> SetupError {
    SetupError::new("invalid_setup_launch", message)
}
fn external(message: impl Into<String>) -> SetupError {
    SetupError::new("setup_launch_unavailable", message)
}

fn bound_draft(
    root: &std::path::Path,
    scope: &super::SetupScope,
    setup_id: &str,
) -> Result<ProjectTeamSetupDraft, SetupError> {
    let draft =
        read_draft(root, scope)?.ok_or_else(|| invalid("Prepare the setup draft first."))?;
    if draft.setup_id != setup_id {
        return Err(invalid("The setup ID does not match the preserved draft."));
    }
    Ok(draft)
}

fn provider_identity(
    app: &AppHandle,
    draft: &ProjectTeamSetupDraft,
    choice: &LaunchChoice,
) -> Result<String, SetupError> {
    let records =
        store::load_provider_readiness_store(app, Some(&draft.owner_pubkey)).map_err(external)?;
    let record = records
        .get(&draft.relay_url)
        .ok_or_else(|| external("Prepare the local session provider before starting setup."))?;
    if record.provider_pubkey != choice.provider_pubkey
        || !record.auth_tag_present
        || record.auth_tag_invalid
        || record.auth_tag_owner_mismatch
    {
        return Err(invalid(
            "The selected local provider is not authorized by this setup's owner.",
        ));
    }
    if crate::session_provider::canonical_relay_key(&record.relay_url) != draft.relay_url {
        return Err(invalid(
            "The local provider belongs to a different community.",
        ));
    }
    Ok(record.instance_id.clone())
}

async fn runtime_driver(choice: &LaunchChoice) -> Result<String, SetupError> {
    let runtimes = commands::coding_session_provider_runtimes()
        .await
        .map_err(external)?;
    let runtime = runtimes
        .iter()
        .find(|row| {
            row.instance_ref == choice.provider_instance_ref && row.runtime == choice.runtime
        })
        .ok_or_else(|| invalid("The selected runtime is not offered by this local provider."))?;
    if !runtime.capabilities.thread_turn_start {
        return Err(external("The selected runtime cannot start a turn."));
    }
    if runtime.auth_state != crate::session_provider::runtimes::CodingSessionRuntimeAuthState::Ready
    {
        return Err(external(
            "Install and sign in to the selected runtime before starting setup.",
        ));
    }
    let models =
        commands::coding_session_provider_models(Some(choice.provider_instance_ref.clone()))
            .await
            .map_err(external)?;
    if models.instance_ref != choice.provider_instance_ref
        || !models.allowed_models.contains(&choice.model)
    {
        return Err(invalid(
            "The selected model is not advertised by this runtime.",
        ));
    }
    Ok(runtime.driver.clone())
}

fn create_payload(
    draft: &ProjectTeamSetupDraft,
    reservation: &authoring::AuthoringReservation,
    choice: &LaunchChoice,
    actor: &actor::PreparedSetupActor,
) -> Result<CodingSessionLifecycleCommandPayload, SetupError> {
    Ok(CodingSessionLifecycleCommandPayload {
        schema: CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA.into(),
        command_id: reservation.create_command_id.clone(),
        action: CodingSessionLifecycleAction::SessionCreate {
            project_ref: Some(draft.project_ref.clone()), repo_ref: None,
            session_ref: Some(reservation.session_ref.clone()), genesis_ref: Some(reservation.genesis_event_id.clone()),
            provider_instance_ref: ProviderInstanceAlias::from_wire(choice.provider_instance_ref.clone()).map_err(invalid)?,
            provider_authority_pubkey: choice.provider_pubkey.clone(), model: Some(choice.model.clone()),
            title: Some("Project team setup".into()),
            initial_turn: Some("Read PROJECT_TEAM_SETUP.md in this execution’s working directory, then build the project’s draft role packs as instructed. Report evidence and limitations; do not publish or change project access.".into()),
            actor: Some(actor.actor_pubkey.clone()), role: Some("project-setup".into()), hire_ref: None, routing: None,
        },
    })
}

fn create_event(
    draft: &ProjectTeamSetupDraft,
    reservation: &authoring::AuthoringReservation,
    choice: &LaunchChoice,
    actor: &actor::PreparedSetupActor,
    keys: &Keys,
) -> Result<Event, SetupError> {
    let payload = create_payload(draft, reservation, choice, actor)?;
    let channel =
        uuid::Uuid::parse_str(&reservation.channel_id).map_err(|e| invalid(e.to_string()))?;
    buzz_sdk_pkg::builders::build_coding_session_lifecycle_command(channel, &payload)
        .map_err(|e| invalid(e.to_string()))?
        .sign_with_keys(keys)
        .map_err(|e| invalid(e.to_string()))
}

/// Read the saved launch and refresh receipts. No identities, files or relay events are created.
#[tauri::command]
pub async fn project_team_setup_get_launch(
    app: AppHandle,
    state: State<'_, AppState>,
    project_ref: String,
    setup_id: String,
    expected_relay_url: String,
) -> Result<Option<SetupAuthoringLaunch>, SetupError> {
    let (root, scope) = context(&app, &state, &project_ref, &expected_relay_url)?;
    let draft = bound_draft(&root, &scope, &setup_id)?;
    let Some(reservation) = authoring::read_reservation(&draft)? else {
        return Ok(None);
    };
    let Some(saved) = journal::read(&draft, &reservation)? else {
        return Ok(None);
    };
    let keys = state.signing_keys().map_err(external)?;
    if keys.public_key().to_hex() != draft.owner_pubkey {
        return Err(invalid("The signing identity changed before launch."));
    }
    let result = wire::observe(&state, &draft, &saved, &keys).await;
    verify_context(&state, &scope)?;
    Ok(Some(result))
}

/// Start or retry one exact reserved setup create through the local provider.
/// The provider must have been explicitly provisioned before invoking this command.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn project_team_setup_start_authoring(
    app: AppHandle,
    state: State<'_, AppState>,
    provider: State<'_, CodingSessionProviderState>,
    project_ref: String,
    setup_id: String,
    expected_relay_url: String,
    provider_pubkey: String,
    provider_instance_ref: String,
    runtime: String,
    model: String,
) -> Result<SetupAuthoringLaunch, SetupError> {
    let (root, scope) = context(&app, &state, &project_ref, &expected_relay_url)?;
    let draft = bound_draft(&root, &scope, &setup_id)?;
    let reservation = authoring::read_reservation(&draft)?
        .ok_or_else(|| invalid("Reserve the setup authoring session before starting it."))?;
    let keys = state.signing_keys().map_err(external)?;
    if keys.public_key().to_hex() != draft.owner_pubkey {
        return Err(invalid("The signing identity changed before launch."));
    }
    let choice = LaunchChoice {
        provider_pubkey,
        provider_instance_ref,
        runtime,
        model,
    };
    let lock_draft = draft.clone();
    let _lock = tokio::task::spawn_blocking(move || journal::lock(&lock_draft))
        .await
        .map_err(|e| external(e.to_string()))??;
    verify_context(&state, &scope)?;
    let existing = journal::read(&draft, &reservation)?;
    let is_retry = existing.is_some();
    let mut saved = if let Some(saved) = existing {
        if saved.choice != choice {
            return Err(invalid("This setup already has a saved runtime choice. Retry it unchanged to avoid creating a second session."));
        }
        saved
    } else {
        let instance_id = provider_identity(&app, &draft, &choice)?;
        let driver = runtime_driver(&choice).await?;
        verify_context(&state, &scope)?;
        wire::verify_project_channel(&state, &draft, &reservation.channel_id, &keys).await?;
        verify_context(&state, &scope)?;
        let actor = actor::prepare_actor(&app, &state, &draft, &keys)?;
        let event = create_event(&draft, &reservation, &choice, &actor, &keys)?;
        let saved = journal::LaunchJournal::new(
            &draft,
            reservation,
            choice,
            actor,
            instance_id,
            driver,
            event,
        );
        journal::save(&draft, &saved, &keys)?;
        saved
    };
    journal::ensure_marker(&draft, &saved.create_event.id.to_hex())?;
    let observed = wire::observe(&state, &draft, &saved, &keys).await;
    verify_context(&state, &scope)?;
    if matches!(
        observed.status,
        LaunchStatus::Created | LaunchStatus::InitialTurnFailed | LaunchStatus::Failed
    ) {
        return Ok(observed);
    }
    // An unavailable receipt query must never turn into a second launch attempt.
    if observed
        .message
        .as_deref()
        .is_some_and(|message| message.starts_with("Receipt reconciliation failed:"))
    {
        return Ok(observed);
    }
    verify_context(&state, &scope)?;
    if provider_identity(&app, &draft, &saved.choice)? != saved.instance_id {
        return Err(external("The selected provider now has a different instance ID. The original request was preserved."));
    }
    if is_retry && runtime_driver(&saved.choice).await? != saved.driver {
        return Err(external("The saved runtime now resolves to a different driver. The original request was preserved."));
    }
    verify_context(&state, &scope)?;
    wire::verify_project_channel(&state, &draft, &saved.reservation.channel_id, &keys).await?;
    verify_context(&state, &scope)?;
    // Ledger 257: a private project's roster must name this host's own key
    // (the one `saved.choice.provider_pubkey` signs with) or the relay's
    // read gate withholds every repository event, ref state included, from
    // it — proven live in kettle-control-6, 2026-09-24. Repaired here, once
    // per project this host has ever been missing from, rather than only at
    // creation: an existing private project launched for the first time on
    // this host reaches this line too. Never fails the launch: a refusal is
    // logged, not propagated, and the CLI/roster-editor path is unchanged.
    // This is one of two call sites that start the provider; both route
    // through `ensure_host_serving_project` so the roster repair can never
    // be wired into one and missed on the other again (run 7, 2026-09-24).
    if !crate::managed_agents::project_roster::ensure_host_serving_project(
        &app,
        &state,
        &provider,
        &keys,
        &draft.project_ref,
        &saved.choice.provider_pubkey,
        &draft.relay_url,
    )
    .await
    .map_err(external)?
    {
        return Err(external("The local provider is not provisioned."));
    }
    wire::ensure_membership(
        &state,
        &draft,
        &saved.reservation.channel_id,
        &[&saved.choice.provider_pubkey, &saved.actor.actor_pubkey],
        &keys,
    )
    .await?;
    verify_context(&state, &scope)?;
    actor::stage_actor(
        &app,
        &state,
        &draft,
        &keys,
        &saved.reservation.create_command_id,
        &saved.actor,
    )?;
    crate::coding_sessions::workdir_store::stage_coding_session_create_hint_at(
        &app,
        &draft.relay_url,
        &saved.reservation.create_command_id,
        std::path::Path::new(&saved.actor.authoring_directory),
    )
    .map_err(external)?;
    // Persist the uncertainty before either event can leave this host. A crash
    // at either HTTP boundary is recovered using these exact signed bytes.
    saved.status = LaunchStatus::Ambiguous;
    saved.message =
        Some("The saved request is being submitted; no provider receipt has been verified.".into());
    journal::save(&draft, &saved, &keys)?;
    verify_context(&state, &scope)?;
    let result = wire::publish(&state, &draft, &saved, &keys).await;
    match result {
        Ok(()) => {
            saved.status = LaunchStatus::AwaitingReceipt;
            saved.message = None;
        }
        Err(error) => {
            saved.message = Some(error.message);
        }
    }
    journal::save(&draft, &saved, &keys)?;
    let observed = wire::observe(&state, &draft, &saved, &keys).await;
    verify_context(&state, &scope)?;
    Ok(observed)
}
