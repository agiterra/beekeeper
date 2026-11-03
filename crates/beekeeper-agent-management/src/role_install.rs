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
    /// Who may instruct the identity. Explicit: a role package does not choose it.
    pub respond_to: RespondTo,
    /// The named identities for [`RespondTo::Allowlist`]; empty otherwise.
    pub respond_to_allowlist: Vec<String>,
}

/// The instruction every canonically installed identity is published with. The
/// role itself is in the staged pack; a later start must repeat these exact
/// words, because a start cannot silently change a definition.
pub const CANONICAL_ROLE_PROMPT: &str =
    "Follow the canonical role pack staged for this responsibility.";

/// The catalog id a `model[effort]` selection was made from. An id the catalog
/// lists whole, brackets included, is compared whole first by each caller.
fn catalog_model(selection: &str) -> &str {
    selection.split('[').next().unwrap_or(selection)
}

/// Why a resolved role cannot be installed as asked, before anything is
/// written: its stable code and a sentence.
pub(crate) fn role_conflict(
    persona: &beekeeper_persona::resolve::ResolvedPersona,
    role: &str,
    runtime: &str,
    model: &str,
) -> Option<(&'static str, String)> {
    if persona.role.as_deref() != Some(role) {
        return Some((
            "ROLE_NOT_IN_SOURCE",
            format!("the project's source has no role `{role}`"),
        ));
    }
    if let Some(declared) = persona
        .runtime
        .as_ref()
        .filter(|declared| *declared != runtime)
    {
        return Some((
            "ROLE_DECLARATION_CONFLICT",
            format!("role `{role}` declares runtime `{declared}`, and `{runtime}` was selected"),
        ));
    }
    if let Some(declared) = persona
        .model
        .as_ref()
        .filter(|declared| *declared != model && *declared != catalog_model(model))
    {
        return Some((
            "ROLE_DECLARATION_CONFLICT",
            format!("role `{role}` declares model `{declared}`, and `{model}` was selected"),
        ));
    }
    if !persona.mcp_servers.is_empty() || persona.hooks.is_some() {
        return Some((
            "ROLE_REQUIRES_UNSUPPORTED_CAPABILITY",
            format!("role `{role}` requires MCP servers or hooks, which this install path cannot honour; refusing rather than discarding them"),
        ));
    }
    None
}

/// Whether a journaled install asked for the same thing as this retry. The
/// source's commit is deliberately not part of intent: it is what was pinned.
fn same_intent(request: &Value, args: &RoleInstallRequest) -> bool {
    let record = &request["privateConfig"]["record"];
    let public = &request["publicDefinition"];
    request["type"] == "create"
        && request["name"] == args.name.as_str()
        && request["model"] == args.model.as_str()
        && request["runtime"] == args.provider_instance.as_str()
        && request["project"] == args.project.as_str()
        && request["channel"] == args.channel.to_string().as_str()
        && record["persona_name_in_team"] == args.role.as_str()
        && record["effective_parallelism"] == args.parallelism
        && serde_json::to_value(args.respond_to).ok().as_ref() == Some(&public["respond_to"])
        && serde_json::to_value(&args.respond_to_allowlist)
            .ok()
            .as_ref()
            == Some(public.get("respond_to_allowlist").unwrap_or(&json!([])))
}

/// Finish a journaled install from its own request, reusing the role material it
/// pinned. A different intent stays a conflict; the original is retained.
async fn resume(
    context: &Context,
    client: &impl RelayTransport,
    args: &RoleInstallRequest,
    request: &Value,
) -> Result<Value, Error> {
    if !same_intent(request, args) {
        return Err(Error::Conflict(
            "operation UUID was already used with a different request; original operation preserved".into(),
        ));
    }
    let public: ManagedAgentEventContent =
        serde_json::from_value(request["publicDefinition"].clone()).map_err(fail)?;
    let private: PrivateConfiguration =
        serde_json::from_value(request["privateConfig"].clone()).map_err(fail)?;
    let pack = private.record["role_material"]["pack_dir"]
        .as_str()
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
        .ok_or_else(|| {
            Error::Refused(
                "ROLE_MATERIAL_MISSING: the role material this install pinned is no longer on this Host; the reserved identity is untouched".into(),
            )
        })?;
    beekeeper_persona::resolve::resolve_persona_by_name(&pack, &args.role).map_err(|_| {
        Error::Refused("ROLE_MATERIAL_MISSING: the pinned role material no longer resolves".into())
    })?;
    let cwd = request["cwd"]
        .as_str()
        .map(PathBuf::from)
        .ok_or_else(|| fail("journaled install has no checkout"))?;
    crate::create(
        &CreateRequest {
            operation_id: args.operation_id,
            channel: args.channel,
            display_name: args.name.clone(),
            system_prompt: request["prompt"]
                .as_str()
                .ok_or_else(|| fail("journaled install has no instructions"))?
                .to_string(),
            cwd,
            project: Some(args.project.clone()),
            model: Some(args.model.clone()),
            provider_instance: Some(args.provider_instance.clone()),
            wait_secs: 1,
            activate: false,
            public_definition: Some(public),
            private_config: Some(private),
            // Every event of an install is idempotent, and the identity is reserved:
            // anything aged out and absent is re-signed in place, so an interrupted
            // install continues whether none or some of it reached the relay.
            reissue_expired: true,
            ..Default::default()
        },
        client,
        context,
    )
    .await
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
    // An operation already journaled is pinned to what it first resolved. The
    // source may have moved since, and that must not change or wedge it.
    let path = crate::management::local::operation_path(context, args.operation_id)?;
    if let Some(held) = crate::management::local::read_stored_operation(&path)? {
        return resume(context, client, args, &held.request).await;
    }
    let runtime = config
        .runtimes
        .iter()
        .find(|r| r.instance_ref == args.provider_instance)
        .ok_or_else(|| Error::Refused("selected runtime is not commissioned".into()))?;
    // The commissioned list, or what discovery replaced it with: on a host whose
    // runtime discovers its models, `host.json` may hold only the word `default`.
    let offered =
        beekeeper_host_core::agent_setup::load_effective_catalog(&config.provider_state_dir)
            .ok()
            .flatten()
            .is_some_and(|file| {
                file.catalog.providers.iter().any(|provider| {
                    provider.provider_instance_ref == args.provider_instance
                        && (provider.allowed_models.contains(&args.model)
                            || provider
                                .allowed_models
                                .iter()
                                .any(|model| model == catalog_model(&args.model)))
                })
            });
    if !runtime.allowed_models.contains(&args.model) && !offered {
        return Err(Error::Refused(
            "selected model is not commissioned for runtime".into(),
        ));
    }
    let material = materialize(context, client, &args.project, &args.role, &args.templates).await?;
    let persona =
        beekeeper_persona::resolve::resolve_persona_by_name(&material.pack_dir, &args.role)
            .map_err(fail)?;
    if let Some((code, message)) =
        role_conflict(&persona, &args.role, &runtime.runtime, &args.model)
    {
        return Err(Error::Refused(format!("{code}: {message}")));
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
    let prompt = CANONICAL_ROLE_PROMPT.to_string();
    let public = ManagedAgentEventContent {
        name: args.name.clone(),
        persona_id: None,
        system_prompt: Some(prompt.clone()),
        model: Some(args.model.clone()),
        provider: None,
        persona_source_version: None,
        parallelism: args.parallelism,
        respond_to: args.respond_to,
        respond_to_allowlist: args.respond_to_allowlist.clone(),
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
