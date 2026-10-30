//! Canonical one-role installation; identity creation remains the management service's.
use crate::hire_source::{host, materialize, projects};
use crate::management::local::fail;
use crate::{Context, CreateRequest, Error, PrivateConfiguration, RelayTransport};
use beekeeper_core::managed_agent::{ManagedAgentEventContent, RespondTo};
use serde_json::{json, Value};
use std::path::PathBuf;
use uuid::Uuid;

/// Explicit inputs for installing one role without starting an execution.
pub struct RoleInstallRequest {
    /// Stable management operation ID, reused unchanged on retry.
    pub operation_id: Uuid,
    /// Owner's channel to which the identity is admitted.
    pub channel: Uuid,
    /// Authorized project with a trusted local checkout and agents source.
    pub project: String,
    /// Role slug resolved from that exact source.
    pub role: String,
    /// Public identity name.
    pub name: String,
    /// Explicit commissioned runtime alias.
    pub provider_instance: String,
    /// Explicit model; never a silent default or fallback.
    pub model: String,
    /// Explicit concurrent responsibility capacity, including the primary context.
    pub parallelism: u32,
    /// Shipped template catalog used by the common role composer.
    pub templates: PathBuf,
}

/// Install canonical custody, public definition and protected role material.
/// The role's prompt remains in the staged pack; normal provider skill/role
/// materialization reads it. No secret is placed in a definition or provider config.
pub async fn install(
    context: &Context,
    client: &impl RelayTransport,
    args: &RoleInstallRequest,
) -> Result<Value, Error> {
    if !(1..=32).contains(&args.parallelism) {
        return Err(Error::Usage("role parallelism must be 1..32".into()));
    }
    let config = host(context, client)?;
    let runtime = config
        .runtimes
        .iter()
        .find(|r| r.instance_ref == args.provider_instance)
        .ok_or_else(|| Error::Refused("selected runtime is not commissioned".into()))?;
    if !runtime.allowed_models.contains(&args.model) {
        return Err(Error::Refused(
            "selected model is not commissioned for runtime".into(),
        ));
    }
    let material = materialize(context, client, &args.project, &args.role, &args.templates).await?;
    let persona =
        beekeeper_persona::resolve::resolve_persona_by_name(&material.pack_dir, &args.role)
            .map_err(fail)?;
    if persona.role.as_deref() != Some(&args.role)
        || persona
            .runtime
            .as_ref()
            .is_some_and(|r| r != &runtime.runtime)
        || persona.model.as_ref().is_some_and(|m| m != &args.model)
    {
        return Err(Error::Refused(
            "selected role/runtime/model differs from canonical role declaration".into(),
        ));
    }
    if !persona.mcp_servers.is_empty() || persona.hooks.is_some() {
        return Err(Error::Refused("this role requires MCP or hooks not supported by the one-role installer; refusing to discard them".into()));
    }
    let state = projects(&config)?;
    let cwd = state
        .projects
        .get(&args.project)
        .ok_or_else(|| Error::Refused("HIRE_CHECKOUT_NOT_RECORDED".into()))?
        .clone();
    let public_project =
        crate::authorize_project(client, &args.project, &client.keys().public_key().to_hex())
            .await?;
    let prompt = "Follow the canonical role pack staged for this responsibility.".to_string();
    let public = ManagedAgentEventContent {
        name: args.name.clone(),
        persona_id: None,
        system_prompt: Some(prompt.clone()),
        model: Some(args.model.clone()),
        provider: None,
        persona_source_version: None,
        parallelism: args.parallelism,
        respond_to: RespondTo::OwnerOnly,
        respond_to_allowlist: vec![],
        home_role: Some(args.role.clone()),
        project_digest: public_project
            .then(|| beekeeper_core::project_agent_association::project_agent_digest(&args.project))
            .flatten(),
        project_withdrawn: !public_project,
    };
    let environment: std::collections::BTreeMap<_, _> =
        persona.runtime_env_vars.into_iter().collect();
    crate::create(&CreateRequest {
        operation_id:args.operation_id,channel:args.channel,display_name:args.name.clone(),system_prompt:prompt,
        cwd,project:Some(args.project.clone()),model:Some(args.model.clone()),provider_instance:Some(args.provider_instance.clone()),
        wait_secs:1,activate:false,public_definition:Some(public),private_config:Some(PrivateConfiguration {record:json!({
            "role_material":material,"runtime":runtime.runtime,"provider_instance":args.provider_instance,"model":args.model,
            "persona_team_dir":material.pack_dir,"persona_name_in_team":args.role,"effective_env":environment,
            "effective_parallelism":args.parallelism
        })}), ..Default::default()
    },client,context).await
}
