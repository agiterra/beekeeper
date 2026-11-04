//! Changing a setup's model without touching what it runs as.
//!
//! The spec is held to a digest and `readback` holds the provider to the spec, so
//! neither a definition update nor an in-session switch can stand alone. A switch
//! needs a live execution and is answered by the provider, so it comes first: a
//! refused switch changes nothing. Recording it then moves the definition, the
//! private settings and the journal together, resumably, on the same identity.
//!
//! Once the provider's record reads the new model, mentions refuse until the
//! definition agrees, and after a restart nothing is live to take a switch. So a
//! rerun past that point never asks for the switch again: a held change or the
//! provider's own record goes straight to recording.
use super::{config_for, current_head, fail, load, lock, open_for, refused, save, signer_guard};
use crate::management::configuration::{
    private_configuration_snapshot, update_if_private_unchanged,
};
use crate::{Context, Error, RelayTransport, UpdateRequest};
use beekeeper_core::managed_agent::ManagedAgentEventContent;
use beekeeper_host_core::agent_setup::{self, AgentSetup, ModelChange, SetupState};
use std::path::Path;
use uuid::Uuid;

/// What a model change will do, decided before anything is sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelChangePlan {
    /// `model[effort]`, as the spec and the definition spell it.
    pub selection: String,
    pub channel: Uuid,
    pub session_ref: Uuid,
    /// The open execution this setup started, if any. It must take the switch.
    pub session: Option<String>,
}

fn chosen(setup: &AgentSetup, model: &str, effort: Option<&str>) -> agent_setup::AgentSetupSpec {
    let mut spec = setup.spec.clone();
    spec.model = model.to_owned();
    // No effort named keeps the one this setup has.
    if let Some(effort) = effort {
        spec.effort = Some(effort.to_owned());
    }
    spec
}

fn actor_of(setup: &AgentSetup) -> Result<String, Error> {
    setup.agent_pubkey.clone().ok_or_else(|| {
        refused(
            "SETUP_NOT_INSTALLED",
            "nothing is installed yet; change the choice by planning again",
        )
    })
}

/// The definition as this setup left it, or as this same change left it.
async fn head_is_ours(
    client: &impl RelayTransport,
    setup: &AgentSetup,
    actor: &str,
    selection: &str,
) -> Result<nostr::Event, Error> {
    let head = current_head(client, actor).await?;
    let content: ManagedAgentEventContent = serde_json::from_str(&head.content).map_err(fail)?;
    let current = content.model.as_deref();
    if current != Some(setup.spec.selection().as_str()) && current != Some(selection) {
        return Err(refused(
            "OWNER_CHANGED_DEFINITION",
            format!(
                "the signed definition {} says model {current:?}, which is neither what this \
                 setup chose nor `{selection}`; nothing was changed",
                head.id
            ),
        ));
    }
    Ok(head)
}

/// Check a change before the switch. Changes nothing.
pub async fn plan_model_change(
    context: &Context,
    client: &impl RelayTransport,
    probe: &super::ProviderProbe,
    setup_id: Uuid,
    model: &str,
    effort: Option<&str>,
    templates: &Path,
) -> Result<ModelChangePlan, Error> {
    let config = config_for(context, client)?;
    let _lock = lock(&config)?;
    let setup = load(&config, setup_id)?;
    signer_guard(client, &setup)?;
    if !matches!(setup.state, SetupState::Running | SetupState::Stopped)
        || setup.pending_action.is_some()
    {
        return Err(refused(
            "SETUP_NOT_SETTLED",
            "this setup is between steps; finish or retry that first",
        ));
    }
    let actor = actor_of(&setup)?;
    let spec = chosen(&setup, model, effort);
    let selection = spec.selection();
    let plan = |session| ModelChangePlan {
        selection: selection.clone(),
        channel: setup.spec.channel,
        session_ref: setup.session_ref,
        session,
    };
    if let Some(held) = &setup.model_change {
        if held.model != spec.model || held.effort != spec.effort {
            return Err(refused(
                "MODEL_CHANGE_IN_PROGRESS",
                format!(
                    "a change to `{}` is still being recorded; run that again to finish it first",
                    held.model
                ),
            ));
        }
        // Everything was checked before this was held, and recording needs no
        // provider: this is the way out when none is live.
        return Ok(plan(None));
    }
    // The same checks a retried Start would meet, so a change that passes here
    // cannot strand this setup at its next Start.
    let report = super::preflight(context, client, probe, &spec, templates, Some(&actor)).await?;
    if let Some(check) = report.first_refusal() {
        return Err(refused(&check.code, &check.message));
    }
    head_is_ours(client, &setup, &actor, &selection).await?;
    let started = setup
        .confirmed_start
        .as_ref()
        .map(|start| start.target.session_id.clone());
    let session = open_for(&config, &actor)?
        .into_iter()
        .map(|row| row.session_id)
        .find(|id| Some(id) == started.as_ref());
    if setup.state == SetupState::Running && session.is_none() {
        return Err(refused(
            "EXECUTION_NOT_OPEN",
            "this setup reads as running, but its execution is not open here, so nothing can \
             take the switch; nothing was changed",
        ));
    }
    // Already taken, by a run that did not get to record it.
    let session = session.filter(|id| {
        !agent_setup::execution_already_on(&config.provider_state_dir, id, &selection)
    });
    Ok(plan(session))
}

/// Record a change the execution already took, or one with no execution to take
/// it. Repeating it reuses the same operation.
pub async fn record_model_change(
    context: &Context,
    client: &impl RelayTransport,
    setup_id: Uuid,
    model: &str,
    effort: Option<&str>,
) -> Result<AgentSetup, Error> {
    let config = config_for(context, client)?;
    let _lock = lock(&config)?;
    let mut setup = load(&config, setup_id)?;
    signer_guard(client, &setup)?;
    let actor = actor_of(&setup)?;
    let spec = chosen(&setup, model, effort);
    let selection = spec.selection();
    if let Some(held) = &setup.model_change {
        if held.model != spec.model || held.effort != spec.effort {
            return Err(refused(
                "MODEL_CHANGE_IN_PROGRESS",
                format!("a change to `{}` is still being recorded", held.model),
            ));
        }
    }
    // Before any hold, so a definition the owner changed cannot create one.
    let head = head_is_ours(client, &setup, &actor, &selection).await?;
    let snapshot = private_configuration_snapshot(context, client, &actor)?;
    let held = match setup.model_change.clone() {
        Some(held) => held,
        None => {
            let held = ModelChange {
                operation_id: Uuid::new_v4(),
                model: spec.model.clone(),
                effort: spec.effort.clone(),
                expected_revision: head.id.to_hex(),
                snapshot_hash: snapshot.snapshot_hash.clone(),
            };
            setup.model_change = Some(held.clone());
            save(&config, &mut setup)?;
            held
        }
    };
    let mut private = snapshot.private.clone();
    private.record["model"] = serde_json::Value::String(selection.clone());
    let request = UpdateRequest {
        operation_id: held.operation_id,
        agent_pubkey: actor.clone(),
        expected_revision: held.expected_revision,
        display_name: None,
        system_prompt: None,
        model: Some(selection),
        home: None,
        public_definition: None,
        private_config: Some(private),
    };
    update_if_private_unchanged(&request, client, context, &held.snapshot_hash).await?;
    let revision = current_head(client, &actor).await?.id.to_hex();
    setup.spec_digest = spec.digest().map_err(fail)?;
    setup.spec = spec;
    setup.revision = Some(revision);
    setup.model_change = None;
    save(&config, &mut setup)?;
    Ok(setup)
}

#[cfg(test)]
#[path = "project_agent_setup_model_tests.rs"]
mod tests;
