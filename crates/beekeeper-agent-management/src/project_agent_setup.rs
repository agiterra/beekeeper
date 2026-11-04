//! Set up one durable conversational project agent, resumably.
//!
//! The shared operation behind Manage Host and `bee host manage`. It composes
//! the management operations that already exist and adds the three things only
//! a chain needs: a journal written before any effect, operation ids minted
//! once and reused for ever, and refusals named before anything is published.
//!
//! # What makes a retry safe
//!
//! The install and start operation ids live in the journal. Each operation's
//! own journal already binds its id to one identity and one set of signed
//! events, so re-running any step after a lost reply or a `kill -9` republishes
//! the same bytes instead of minting a second agent.
//!
//! # What this does not establish
//!
//! A `running` setup means the provider signed `created`, holds an unexpired
//! lease and the signed definition matches the operator's choices. It does not
//! mean the agent has answered anyone: only a signed turn result proves the
//! runtime's account works. Nothing here says otherwise.

#[path = "project_agent_setup_model.rs"]
mod model;
pub use model::{plan_model_change, record_model_change, ModelChangePlan};

use crate::hire_source::{host, materialize, projects};
use crate::management::configuration::{current_head, require_private_channel};
use crate::management::local::{self, fail};
use crate::role_install::{self, role_conflict, RoleInstallRequest, CANONICAL_ROLE_PROMPT};
use crate::{Context, CreateRequest, Error, LifecycleRequest, RelayTransport};
use beekeeper_core::managed_agent::{
    validate_agent_definition_text, ManagedAgentEventContent, RespondTo,
};
use beekeeper_host_core::agent_setup::{
    self, AgentSetup, AgentSetupSpec, AttemptRecord, PendingAction, SetupAccess, SetupRefusal,
    SetupState, StepName, StepRecord, AGENT_SETUP_VERSION,
};
use beekeeper_host_core::config::HostConfig;
use nostr::Event;
use serde::Serialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// How long a start or stop waits for the provider's signed receipt.
const RECEIPT_WAIT_SECS: u64 = 60;
/// How long verification waits for the provider's first lease after `created`.
const LEASE_WAIT: std::time::Duration = std::time::Duration::from_secs(20);

/// What the caller learned from the Host's own socket. This crate does not
/// speak that protocol: the adapter asks and hands over the answer.
#[derive(Clone, Debug)]
pub struct ProviderProbe {
    /// The live provider's pid, or `None` when none is running.
    pub live_pid: Option<u32>,
    /// A sentence for the refusal when it is not live.
    pub state: String,
}

/// One preflight fact.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    /// Stable code; a refusal's code when `ok` is false.
    pub code: String,
    /// Whether this one passed.
    pub ok: bool,
    /// A sentence for a person.
    pub message: String,
    /// True and still worth saying: nothing stops the setup over this.
    #[serde(default)]
    pub warning: bool,
}

/// All preflight facts. A person fixes several things in one go, so a refusal
/// does not stop the checks that can still run.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preflight {
    /// Every check passed.
    pub ok: bool,
    /// In the order they ran.
    pub checks: Vec<Check>,
}

impl Preflight {
    fn first_refusal(&self) -> Option<&Check> {
        self.checks.iter().find(|check| !check.ok)
    }
}

fn now() -> String {
    beekeeper_host_core::logs::now_iso()
}

fn refused(code: &str, message: impl std::fmt::Display) -> Error {
    Error::Refused(format!("{code}: {message}"))
}

/// Split a management error into its stable code, sentence and whether the
/// operation may have taken effect anyway.
fn classify(error: &Error) -> (String, String, bool) {
    let text = error.to_string();
    // Anything short of an answer leaves the outcome unknown.
    let uncertain = matches!(
        error,
        Error::Unconfirmed(_) | Error::Relay { .. } | Error::Other(_)
    );
    if let Some((code, rest)) = text.split_once(':') {
        if !code.is_empty()
            && code
                .chars()
                .all(|c| c.is_ascii_uppercase() || c == '_' || c.is_ascii_digit())
        {
            return (code.to_string(), rest.trim().to_string(), uncertain);
        }
    }
    let code = match error {
        Error::Unconfirmed(_) => "OPERATION_UNCONFIRMED",
        Error::Conflict(_) => "OPERATION_CONFLICT",
        Error::Auth(_) => "NOT_AUTHORIZED",
        Error::NotFound(_) => "NOT_FOUND",
        Error::Usage(_) => "INVALID_REQUEST",
        Error::Relay { .. } | Error::Refused(_) | Error::Other(_) => "OPERATION_FAILED",
    };
    (code.to_string(), text, uncertain)
}

/// Validate the access choice. Separate so its rules read as a list.
fn access_refusal(access: &SetupAccess, owner: &str) -> Option<String> {
    let SetupAccess::Allowlist(pubkeys) = access else {
        return None;
    };
    if pubkeys.is_empty() {
        return Some("an allowlist must name at least one identity".into());
    }
    for key in pubkeys {
        if key.len() != 64
            || !key
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        {
            return Some(format!(
                "`{key}` is not a 64-character lowercase hex public key"
            ));
        }
        if key == owner {
            return Some(
                "the owner is always admitted, so the owner does not belong on the allowlist"
                    .into(),
            );
        }
    }
    None
}

fn respond_to(access: &SetupAccess) -> (RespondTo, Vec<String>) {
    match access {
        SetupAccess::OwnerOnly => (RespondTo::OwnerOnly, Vec::new()),
        SetupAccess::Allowlist(keys) => (RespondTo::Allowlist, keys.clone()),
    }
}

/// The signer's role in a channel, from the relay-signed membership record.
async fn channel_role(
    client: &impl RelayTransport,
    channel: Uuid,
    signer: &str,
) -> Result<Option<String>, Error> {
    let relay = client.relay_identity().await?;
    let rows = client
        .query_all(json!({"kinds":[39002],"authors":[relay],"#d":[channel.to_string()]}))
        .await?;
    let head = rows
        .into_iter()
        .filter_map(|value| serde_json::from_value::<Event>(value).ok())
        .filter(|event| {
            event.pubkey.to_hex() == relay && beekeeper_core::verify_event(event).is_ok()
        })
        .max_by_key(|event| (event.created_at, std::cmp::Reverse(event.id)));
    Ok(head.and_then(|event| {
        event.tags.iter().find_map(|tag| {
            let fields = tag.as_slice();
            (fields.first().is_some_and(|name| name == "p")
                && fields.get(1).is_some_and(|key| key == signer))
            .then(|| {
                fields
                    .get(3)
                    .cloned()
                    .unwrap_or_else(|| "member".to_string())
            })
        })
    }))
}

/// Channels the signer can add this agent to, for a picker.
pub async fn manageable_channels(client: &impl RelayTransport) -> Result<Vec<Value>, Error> {
    let signer = local::owner(client)?;
    let relay = client.relay_identity().await?;
    let memberships = client
        .query_all(json!({"kinds":[39002],"authors":[relay],"#p":[signer]}))
        .await?;
    let mut owned = Vec::new();
    for event in memberships
        .into_iter()
        .filter_map(|value| serde_json::from_value::<Event>(value).ok())
        .filter(|event| {
            event.pubkey.to_hex() == relay && beekeeper_core::verify_event(event).is_ok()
        })
    {
        let tags: Vec<&[String]> = event.tags.iter().map(|tag| tag.as_slice()).collect();
        let Some(id) = tags
            .iter()
            .find(|tag| tag.first().is_some_and(|name| name == "d"))
            .and_then(|tag| tag.get(1))
        else {
            continue;
        };
        let role = tags.iter().find_map(|tag| {
            (tag.first().is_some_and(|name| name == "p")
                && tag.get(1).is_some_and(|key| *key == signer))
            .then(|| tag.get(3).cloned().unwrap_or_default())
        });
        if matches!(role.as_deref(), Some("owner" | "admin")) {
            owned.push((id.clone(), role.unwrap_or_default()));
        }
    }
    let ids: Vec<&String> = owned.iter().map(|(id, _)| id).collect();
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let metadata = client
        .query_all(json!({"kinds":[39000],"authors":[relay],"#d":ids}))
        .await?;
    let mut rows = Vec::new();
    for (id, role) in owned {
        let head = metadata
            .iter()
            .filter_map(|value| serde_json::from_value::<Event>(value.clone()).ok())
            .filter(|event| {
                event
                    .tags
                    .iter()
                    .any(|tag| tag.as_slice() == ["d", id.as_str()])
                    && beekeeper_core::verify_event(event).is_ok()
            })
            .max_by_key(|event| event.created_at);
        let Some(head) = head else { continue };
        let tag = |name: &str| {
            head.tags.iter().find_map(|tag| {
                let fields = tag.as_slice();
                (fields.first().is_some_and(|held| held == name))
                    .then(|| fields.get(1).cloned())
                    .flatten()
            })
        };
        let private = head
            .tags
            .iter()
            .any(|tag| tag.as_slice().first().is_some_and(|name| name == "private"));
        rows.push(
            json!({"id":id,"name":tag("name").unwrap_or_default(),"role":role,"private":private}),
        );
    }
    Ok(rows)
}

/// An identity that already exists for this project and role under this name.
async fn duplicate_identity(
    client: &impl RelayTransport,
    spec: &AgentSetupSpec,
    owner: &str,
    own_actor: Option<&str>,
) -> Result<Option<String>, Error> {
    let digest = beekeeper_core::project_agent_association::project_agent_digest(&spec.project);
    let rows = client
        .query_all(json!({"kinds":[30177],"authors":[owner]}))
        .await?;
    for event in rows
        .into_iter()
        .filter_map(|value| serde_json::from_value::<Event>(value).ok())
        .filter(|event| beekeeper_core::verify_event(event).is_ok())
    {
        let Some(actor) = event.tags.iter().find_map(|tag| {
            let fields = tag.as_slice();
            (fields.first().is_some_and(|name| name == "d"))
                .then(|| fields.get(1).cloned())
                .flatten()
        }) else {
            continue;
        };
        if Some(actor.as_str()) == own_actor {
            continue;
        }
        let Ok(content) = serde_json::from_str::<ManagedAgentEventContent>(&event.content) else {
            continue;
        };
        if content.name == spec.display_name
            && content.home_role.as_deref() == Some(spec.role.as_str())
            && (content.project_digest == digest || content.project_withdrawn)
        {
            return Ok(Some(actor));
        }
    }
    Ok(None)
}

/// The identity an install operation already bound, if it got that far. A
/// killed apply can have reserved a key the setup journal never heard about.
fn reserved_actor(context: &Context, setup: &AgentSetup) -> Result<Option<String>, Error> {
    if setup.agent_pubkey.is_some() {
        return Ok(setup.agent_pubkey.clone());
    }
    if setup.spec.existing_actor.is_some() {
        return Ok(setup.spec.existing_actor.clone());
    }
    let path = local::operation_path(context, setup.install_operation)?;
    Ok(local::read_stored_operation(&path)?.map(|operation| operation.actor_pubkey))
}

/// Run every check that can run. Writes nothing to the relay; the only local
/// effect is the immutable role-source cache the install would use anyway.
pub async fn preflight(
    context: &Context,
    client: &impl RelayTransport,
    probe: &ProviderProbe,
    spec: &AgentSetupSpec,
    templates: &Path,
    own_actor: Option<&str>,
) -> Result<Preflight, Error> {
    let mut checks = Vec::new();
    let mut add = |code: &str, ok: bool, message: String| {
        checks.push(Check {
            code: code.into(),
            ok,
            message,
            warning: false,
        })
    };

    // Nothing else means anything until the signer is the Host's owner.
    let config = match host(context, client) {
        Ok(config) => {
            add(
                "OWNER_SIGNER_MATCHES",
                true,
                "The signer owns this Host's provider.".into(),
            );
            config
        }
        Err(error) => {
            let (code, message) = match &error {
                Error::Refused(text) if text.contains("not commissioned") => (
                    "HOST_NOT_COMMISSIONED",
                    "This Host is not commissioned; run `bee host commission`.".to_string(),
                ),
                other => (
                    "OWNER_SIGNER_MISMATCH",
                    format!("This key cannot act for this Host: {other}"),
                ),
            };
            add(code, false, message);
            return Ok(Preflight { ok: false, checks });
        }
    };
    let owner = local::owner(client)?;

    match probe.live_pid {
        Some(_) => add(
            "PROVIDER_LIVE",
            true,
            "The session provider is running.".into(),
        ),
        None => add(
            "HOST_PROVIDER_NOT_LIVE",
            false,
            format!(
                "The session provider is not running ({}). Start it first.",
                probe.state
            ),
        ),
    }

    let runtime = config
        .runtimes
        .iter()
        .find(|runtime| runtime.instance_ref == spec.provider_instance);
    match runtime {
        Some(_) => add(
            "RUNTIME_COMMISSIONED",
            true,
            format!("`{}` is commissioned on this Host.", spec.provider_instance),
        ),
        None => add(
            "RUNTIME_NOT_COMMISSIONED",
            false,
            format!(
                "`{}` is not commissioned on this Host.",
                spec.provider_instance
            ),
        ),
    }

    // The offer the running provider signed, never `host.json`'s list: on a
    // discovering runtime that list may be the single word `default`.
    match agent_setup::load_effective_catalog(&config.provider_state_dir) {
        Ok(Some(file)) if Some(file.provider_pid) == probe.live_pid => {
            let offered = file.catalog.providers.iter().find(|provider| {
                provider.provider_instance_ref == spec.provider_instance
            });
            match offered {
                Some(provider) if provider.allowed_models.contains(&spec.model) => {
                    use beekeeper_host_core::agent_setup::DiscoveryOutcome;
                    let efforts = provider
                        .models
                        .iter()
                        .find(|model| model.id == spec.model)
                        .map_or(&[][..], |model| model.efforts.as_slice());
                    match spec.effort.as_deref() {
                        Some(effort) if !efforts.iter().any(|offered| offered == effort) => add(
                            "EFFORT_NOT_OFFERED",
                            false,
                            if efforts.is_empty() {
                                format!("`{}` lists no effort setting, so `{effort}` cannot be applied.", spec.model)
                            } else {
                                format!("`{}` offers these efforts: {}. `{effort}` is not one of them.", spec.model, efforts.join(", "))
                            },
                        ),
                        Some(effort) => add(
                            "EFFORT_OFFERED",
                            true,
                            format!("`{}` offers effort `{effort}`.", spec.model),
                        ),
                        // A journaled setup keeps what it chose. A new one chooses.
                        None if !efforts.is_empty() && own_actor.is_none() => add(
                            "EFFORT_REQUIRED",
                            false,
                            format!("`{}` offers these efforts: {}. Choose one.", spec.model, efforts.join(", ")),
                        ),
                        None => {}
                    }
                    match file.discovery.get(&spec.provider_instance) {
                        Some(DiscoveryOutcome::CredentialsUnavailable(code)) => add(
                            "RUNTIME_CREDENTIALS_UNAVAILABLE",
                            false,
                            format!("`{}` has no readable credential ({code}), so nothing can run on it. Restore it and restart agents.", spec.provider_instance),
                        ),
                        Some(DiscoveryOutcome::Failed(reason))
                            if spec.model.eq_ignore_ascii_case("default") =>
                        {
                            add(
                                "MODEL_UNCONFIRMED_BY_ADAPTER",
                                false,
                                format!("The adapter did not answer ({reason}), and `default` names no model, so nothing confirms what would run. Fix the runtime's login and restart agents."),
                            )
                        }
                        _ => add(
                            "MODEL_OFFERED",
                            true,
                            format!("`{}` is in the running provider's catalog.", spec.model),
                        ),
                    }
                }
                Some(provider) => add(
                    "MODEL_NOT_IN_EFFECTIVE_CATALOG",
                    false,
                    format!(
                        "`{}` is not in the running provider's catalog, which offers: {}.",
                        spec.model,
                        provider.allowed_models.join(", ")
                    ),
                ),
                None => add(
                    "MODEL_NOT_IN_EFFECTIVE_CATALOG",
                    false,
                    format!("The running provider does not offer `{}` at all.", spec.provider_instance),
                ),
            }
        }
        Ok(_) => add(
            "EFFECTIVE_CATALOG_UNAVAILABLE",
            false,
            "No catalog from the running provider is readable, so this model cannot be confirmed. Restart agents; a provider older than this Host writes none.".into(),
        ),
        Err(error) => add("EFFECTIVE_CATALOG_UNAVAILABLE", false, error),
    }

    match access_refusal(&spec.access, &owner) {
        None => add(
            "ACCESS_VALID",
            true,
            "The access choice is explicit and valid.".into(),
        ),
        Some(reason) => add("ACCESS_INVALID", false, reason),
    }
    if !(1..=32).contains(&spec.parallelism) {
        add(
            "PARALLELISM_INVALID",
            false,
            "Concurrent responsibilities must be between 1 and 32.".into(),
        );
    }
    if let Err(reason) = validate_agent_definition_text(&spec.display_name, CANONICAL_ROLE_PROMPT) {
        add("NAME_INVALID", false, reason);
    }

    let coordinate = beekeeper_core::kind::normalize_project_coordinate(&spec.project);
    let mut project_public = None;
    match coordinate.as_deref() {
        None => add(
            "PROJECT_NOT_AUTHORIZED",
            false,
            format!("`{}` is not a project coordinate.", spec.project),
        ),
        Some(project) => match crate::authorize_project(client, project, &owner).await {
            // A relay that did not answer has refused nothing.
            Err(error @ (Error::Other(_) | Error::Relay { .. } | Error::Unconfirmed(_))) => {
                return Err(error)
            }
            Err(error) => add(
                "PROJECT_NOT_AUTHORIZED",
                false,
                format!("The signer cannot act for this project: {error}"),
            ),
            Ok(public) => {
                project_public = Some(public);
                // Installing signs a roster row for each new identity, and only a
                // project owner may. Refused here, or custody exists before it fails.
                let creator = project.split(':').nth(1).unwrap_or_default();
                let holds_owner = creator == owner
                    || crate::project_roster(client, project)
                        .await?
                        .iter()
                        .any(|(key, role)| key == &owner && role == "owner");
                if holds_owner {
                    add(
                        "PROJECT_AUTHORIZED",
                        true,
                        "The signer owns this project.".into(),
                    );
                } else {
                    add(
                        "PROJECT_OWNER_REQUIRED",
                        false,
                        "Installing an identity writes the project roster, which needs the project's `owner` role, and this signer is only a collaborator. The project's creator or an owner must grant it first; nothing was changed.".into(),
                    );
                }
            }
        },
    }

    let trusted = projects(&config).ok();
    let recorded = trusted.as_ref().is_some_and(|state| {
        state.projects.contains_key(&spec.project) && state.agents_repos.contains_key(&spec.project)
    });
    if recorded {
        add(
            "PROJECT_TRUSTED",
            true,
            "This Host has the project's checkout and agents clone on record.".into(),
        );
    } else {
        add(
            "PROJECT_NOT_TRUSTED",
            false,
            format!(
                "This Host has no checkout and agents clone on record for this project. Run `bee host trust-project --project {} --checkout <path> --agents-repo <path>`.",
                spec.project
            ),
        );
    }

    match channel_role(client, spec.channel, &owner).await {
        Ok(Some(role)) if role == "owner" || role == "admin" => add(
            "CHANNEL_MANAGED",
            true,
            format!("The signer is this channel's {role}."),
        ),
        Ok(Some(role)) => add(
            "CHANNEL_NOT_OWNED",
            false,
            format!("The signer is only a {role} of this channel, and adding an agent needs its owner or an admin."),
        ),
        Ok(None) => add(
            "CHANNEL_NOT_OWNED",
            false,
            "The signer is not a member of this channel, or it does not exist.".into(),
        ),
        Err(error) => return Err(error),
    }
    if project_public == Some(false) {
        if let Err(error) = require_private_channel(client, spec.channel).await {
            add(
                "PRIVATE_PROJECT_NEEDS_PRIVATE_CHANNEL",
                false,
                format!(
                    "This project is private, so its agent must be in a private channel: {error}"
                ),
            );
        }
    }

    let mut pack_commit: Option<String> = None;
    if own_actor.is_some() && spec.existing_actor.is_none() {
        // Pinned when the identity was reserved: resolving again here would judge
        // that install by a source it no longer depends on.
        add(
            "ROLE_PINNED",
            true,
            "This identity's role material was pinned when it was reserved.".into(),
        );
    } else if recorded && project_public.is_some() {
        match materialize(context, client, &spec.project, &spec.role, templates).await {
            Err(error) => add(
                "ROLE_SOURCE_UNAVAILABLE",
                false,
                format!("The project's role source could not be resolved: {error}"),
            ),
            Ok(material) => {
                match beekeeper_persona::resolve::resolve_persona_by_name(
                    &material.pack_dir,
                    &spec.role,
                ) {
                    Err(_) => add(
                        "ROLE_NOT_IN_SOURCE",
                        false,
                        format!("The project's source has no role `{}`.", spec.role),
                    ),
                    Ok(persona) => {
                        let conflict = runtime.and_then(|runtime| {
                            role_conflict(&persona, &spec.role, &runtime.runtime, &spec.selection())
                        });
                        match conflict {
                            Some((code, message)) => add(code, false, message),
                            None => {
                                pack_commit = Some(material.pack_ref.sha.clone());
                                add(
                                    "ROLE_RESOLVED",
                                    true,
                                    format!(
                                        "Role `{}` resolves at commit {} of the project's source.",
                                        spec.role, material.pack_ref.sha
                                    ),
                                )
                            }
                        }
                    }
                }
            }
        }
    }

    if let Some(actor) = &spec.existing_actor {
        match crate::adopt_identity::verify(context, &config, client, &owner, actor, spec).await {
            Ok(()) => {
                add(
                    "EXISTING_IDENTITY_ADOPTED",
                    true,
                    format!(
                        "This Host holds attested custody of {actor}, and its definition matches."
                    ),
                );
                if let Some(change) =
                    crate::adopt_identity::definition_change(client, actor, &spec.selection())
                        .await?
                {
                    let pack = pack_commit
                        .as_deref()
                        .map(|sha| format!(" The role pack is at commit {sha}."))
                        .unwrap_or_default();
                    add("DEFINITION_WILL_CHANGE", true, format!("{change}{pack}"));
                }
            }
            Err(error) => add("EXISTING_IDENTITY_NOT_ADOPTED", false, error.to_string()),
        }
    }
    let own_actor = own_actor.or(spec.existing_actor.as_deref());
    match duplicate_identity(client, spec, &owner, own_actor).await {
        Ok(None) => add(
            "IDENTITY_UNIQUE",
            true,
            "No other identity holds this name and role in this project.".into(),
        ),
        Ok(Some(actor)) => add(
            "IDENTITY_ALREADY_EXISTS",
            false,
            format!("Identity {actor} already holds this name and role in this project."),
        ),
        Err(error) => return Err(error),
    }

    if let SetupAccess::Allowlist(keys) = &spec.access {
        for key in keys {
            if let Ok(None) = channel_role(client, spec.channel, key).await {
                checks.push(Check {
                    code: "ALLOWLIST_NOT_CHANNEL_MEMBER".into(),
                    ok: true,
                    message: format!("{key} is not in this channel. The allowlist does not add anyone: they cannot post here until the channel's owner adds them."),
                    warning: true,
                });
            }
        }
    }
    let ok = checks.iter().all(|check| check.ok);
    Ok(Preflight { ok, checks })
}

fn config_for(context: &Context, client: &impl RelayTransport) -> Result<HostConfig, Error> {
    host(context, client).map_err(|error| {
        refused(
            "OWNER_SIGNER_MISMATCH",
            format!("this key cannot act for this Host: {error}"),
        )
    })
}

fn lock(config: &HostConfig) -> Result<std::fs::File, Error> {
    agent_setup::try_apply_lock(&config.provider_state_dir)
        .map_err(fail)?
        .ok_or_else(|| {
            refused(
                "SETUP_BUSY",
                "another setup operation is running on this Host; it can be retried when it ends",
            )
        })
}

fn load(config: &HostConfig, setup_id: Uuid) -> Result<AgentSetup, Error> {
    agent_setup::load_setup(&config.provider_state_dir, setup_id)
        .map_err(fail)?
        .ok_or_else(|| {
            Error::NotFound(format!("SETUP_NOT_FOUND: no setup {setup_id} on this Host"))
        })
}

fn save(config: &HostConfig, setup: &mut AgentSetup) -> Result<(), Error> {
    setup.updated_at = now();
    agent_setup::save_setup(&config.provider_state_dir, setup).map_err(fail)
}

fn begin(config: &HostConfig, setup: &mut AgentSetup, step: StepName) -> Result<(), Error> {
    setup.state = SetupState::Applying;
    setup.steps.push(StepRecord {
        step,
        state: "running".into(),
        at: now(),
        detail: None,
    });
    save(config, setup)
}

fn finish(setup: &mut AgentSetup, state: &str, detail: Option<String>) {
    if let Some(last) = setup.steps.last_mut() {
        last.state = state.into();
        last.at = now();
        last.detail = detail;
    }
}

/// Record a failed step as a state, not an exception: the journal is what a
/// client reads, and a retry must find it.
fn attention(config: &HostConfig, setup: &mut AgentSetup, error: &Error) -> Result<(), Error> {
    let (code, message, uncertain) = classify(error);
    finish(setup, "failed", Some(format!("{code}: {message}")));
    setup.state = SetupState::NeedsAttention;
    setup.last_refusal = Some(SetupRefusal {
        code,
        message,
        at: now(),
        uncertain,
        attempt_closed: false,
    });
    save(config, setup)
}

fn is_expired(error: &Error) -> bool {
    matches!(error, Error::Refused(text) if text.starts_with(crate::management::JOURNAL_EVENT_EXPIRED))
}

/// Close an attempt whose outcome is known to be final, and persist what replaces
/// it. The caller saves before anything is sent with the new id.
fn close_attempt(
    setup: &mut AgentSetup,
    action: &str,
    operation_id: Uuid,
    reason: &str,
    evidence: String,
) -> Uuid {
    let replaced_by = Uuid::new_v4();
    setup.attempts.push(AttemptRecord {
        action: action.to_string(),
        operation_id,
        reason: reason.to_string(),
        evidence,
        replaced_by,
        at: now(),
    });
    if action != "context" {
        setup.pending_action = Some(PendingAction {
            action: action.to_string(),
            operation_id: replaced_by,
            attempted: false,
        });
    }
    replaced_by
}

/// What is known about the start this setup would repeat.
#[derive(Clone, Copy, PartialEq, Eq)]
enum StartStatus {
    /// Nothing has been signed for it.
    NotSent,
    /// Signed, and no receipt is recorded: it may be delivered yet.
    Unknown,
    /// The provider's receipt is recorded.
    Landed,
}

fn current_start(setup: &AgentSetup) -> Uuid {
    setup
        .pending_action
        .as_ref()
        .filter(|held| held.action == "start")
        .map_or(setup.start_operation, |held| held.operation_id)
}

fn start_status(context: &Context, setup: &AgentSetup) -> Result<StartStatus, Error> {
    if setup
        .confirmed_start
        .as_ref()
        .is_some_and(|start| start.operation_id == current_start(setup))
    {
        return Ok(StartStatus::Landed);
    }
    let path = local::operation_path(context, current_start(setup))?;
    Ok(match local::read_stored_operation(&path)? {
        None => StartStatus::NotSent,
        Some(operation) if operation.target.is_some() => StartStatus::Landed,
        Some(_) => StartStatus::Unknown,
    })
}

fn open_for(config: &HostConfig, actor: &str) -> Result<Vec<agent_setup::OpenExecution>, Error> {
    Ok(
        agent_setup::read_open_executions(&config.provider_state_dir)
            .map_err(|error| Error::Unconfirmed(format!("PROVIDER_STATE_UNREADABLE: {error}")))?
            .into_iter()
            .filter(|row| row.actor == actor)
            .collect(),
    )
}

/// Start, replacing the attempt once if the relay shows its signed events can
/// never be accepted. Any other ending is recorded and keeps its id.
async fn start_recorded(
    context: &Context,
    client: &impl RelayTransport,
    config: &HostConfig,
    setup: &mut AgentSetup,
    actor: &str,
    operation: Uuid,
) -> Result<(), Error> {
    match start_and_verify(context, client, config, setup, actor, operation).await {
        Err(error) if is_expired(&error) => {
            // A new attempt would add an existing identity to the channel again. If
            // it is not there now, the owner may have removed it, so that stays open.
            if setup.spec.existing_actor.is_some()
                && channel_role(client, setup.spec.channel, actor)
                    .await?
                    .is_none()
            {
                return attention(
                    config,
                    setup,
                    &crate::adopt_identity::start_membership_unknown(),
                );
            }
            let fresh = close_attempt(
                setup,
                "start",
                operation,
                crate::management::JOURNAL_EVENT_EXPIRED,
                format!("{error}; its command was read back and is not stored, so no provider can act on it"),
            );
            finish(setup, "failed", Some(error.to_string()));
            save(config, setup)?;
            if let Err(error) = start_and_verify(context, client, config, setup, actor, fresh).await
            {
                start_failed(config, setup, &error)?;
            }
            Ok(())
        }
        Err(error) => start_failed(config, setup, &error),
        Ok(()) => Ok(()),
    }
}

/// Record how a start ended when it did not reach running.
///
/// Only a refusal the provider signed closes the attempt, and only then is a new
/// id persisted for the next one. Every other ending keeps the exact id: the
/// command may still be delivered, so a new one could run the agent twice.
fn start_failed(config: &HostConfig, setup: &mut AgentSetup, error: &Error) -> Result<(), Error> {
    let Some((receipt, signed)) = crate::management::parse_provider_refusal(error) else {
        return attention(config, setup, error);
    };
    finish(
        setup,
        "failed",
        Some(format!("{}: receipt {receipt}", signed.code)),
    );
    setup.state = SetupState::NeedsAttention;
    setup.last_refusal = Some(SetupRefusal {
        code: signed.code.clone(),
        message: format!(
            "{} The provider refused this attempt (receipt {receipt}). Fix the cause, then Retry starts a new attempt.",
            signed.message
        ),
        at: now(),
        uncertain: false,
        attempt_closed: true,
    });
    let closed = current_start(setup);
    close_attempt(
        setup,
        "start",
        closed,
        &signed.code,
        format!("provider-signed refusal in receipt {receipt}"),
    );
    save(config, setup)
}

fn signer_guard(client: &impl RelayTransport, setup: &AgentSetup) -> Result<(), Error> {
    let signer = local::owner(client)?;
    if signer != setup.owner {
        return Err(refused(
            "OWNER_SIGNER_MISMATCH",
            format!("this setup belongs to owner {}", setup.owner),
        ));
    }
    let digest = setup.spec.digest().map_err(fail)?;
    if digest != setup.spec_digest {
        return Err(refused(
            "SETUP_SPEC_CONFLICT",
            "the journal's choices no longer match its digest; refusing to act on them",
        ));
    }
    Ok(())
}

/// Preflight, then journal. Nothing is published. A refusal writes no journal.
pub async fn plan(
    context: &Context,
    client: &impl RelayTransport,
    probe: &ProviderProbe,
    spec: AgentSetupSpec,
    templates: &Path,
) -> Result<AgentSetup, Error> {
    let config = config_for(context, client)?;
    let _lock = lock(&config)?;
    let (existing, unreadable) = agent_setup::list_setups(&config.provider_state_dir);
    if let Some(sentence) = unreadable.first() {
        return Err(refused(
            "SETUP_JOURNAL_UNREADABLE",
            format!(
                "{sentence}; refusing to plan another setup, which could duplicate an identity"
            ),
        ));
    }
    for held in existing
        .iter()
        .filter(|held| held.spec.project == spec.project && held.spec.role == spec.role)
    {
        // A cancelled setup of an existing identity reserved nothing: it minted no one.
        let reserved = if held.spec.existing_actor.is_some() && held.agent_pubkey.is_none() {
            None
        } else {
            reserved_actor(context, held)?
        };
        if held.state != SetupState::Cancelled || reserved.is_some() {
            return Err(refused(
                "SETUP_ALREADY_EXISTS",
                format!(
                    "setup {} already covers this project and role{}; retry or resume it instead",
                    held.setup_id,
                    reserved
                        .map(|actor| format!(" with identity {actor}"))
                        .unwrap_or_default()
                ),
            ));
        }
    }
    let report = preflight(context, client, probe, &spec, templates, None).await?;
    if let Some(check) = report.first_refusal() {
        return Err(refused(&check.code, &check.message));
    }
    let created = now();
    let mut setup = AgentSetup {
        version: AGENT_SETUP_VERSION,
        setup_id: Uuid::new_v4(),
        spec_digest: spec.digest().map_err(fail)?,
        spec,
        owner: local::owner(client)?,
        relay_url: config.relay_url.clone(),
        created_at: created.clone(),
        updated_at: created.clone(),
        state: SetupState::Planned,
        install_operation: Uuid::new_v4(),
        start_operation: Uuid::new_v4(),
        confirmed_start: None,
        context_operation: Uuid::new_v4(),
        session_ref: Uuid::new_v4(),
        genesis_event_id: None,
        project_context: None,
        pending_action: None,
        attempts: Vec::new(),
        agent_pubkey: None,
        revision: None,
        receipt_event_id: None,
        steps: vec![StepRecord {
            step: StepName::Preflight,
            state: "done".into(),
            at: created,
            detail: None,
        }],
        last_refusal: None,
        readback: None,
        model_change: None,
    };
    save(&config, &mut setup)?;
    Ok(setup)
}

fn start_request(
    config: &HostConfig,
    setup: &AgentSetup,
    actor: &str,
    operation_id: Uuid,
) -> Result<CreateRequest, Error> {
    let state = projects(config)?;
    let cwd: PathBuf = state
        .projects
        .get(&setup.spec.project)
        .ok_or_else(|| {
            refused(
                "PROJECT_NOT_TRUSTED",
                "this Host no longer records the project's checkout",
            )
        })?
        .clone();
    Ok(CreateRequest {
        operation_id,
        channel: setup.spec.channel,
        display_name: setup.spec.display_name.clone(),
        system_prompt: CANONICAL_ROLE_PROMPT.to_string(),
        cwd,
        project: Some(setup.spec.project.clone()),
        model: Some(setup.spec.selection()),
        provider_instance: Some(setup.spec.provider_instance.clone()),
        agent_pubkey: Some(actor.to_string()),
        wait_secs: RECEIPT_WAIT_SECS,
        activate: true,
        ..Default::default()
    })
}

/// Read back what the relay holds and require it to match the operator's choices.
async fn readback(
    client: &impl RelayTransport,
    config: &HostConfig,
    setup: &AgentSetup,
    actor: &str,
    wait_for_lease: bool,
) -> Result<Value, Error> {
    let head = current_head(client, actor).await?;
    let definition: ManagedAgentEventContent = serde_json::from_str(&head.content).map_err(fail)?;
    let (expected_policy, expected_list) = respond_to(&setup.spec.access);
    let selection = setup.spec.selection();
    if definition.model.as_deref() != Some(selection.as_str())
        || definition.respond_to != expected_policy
        || definition.respond_to_allowlist != expected_list
        || definition.home_role.as_deref() != Some(setup.spec.role.as_str())
    {
        return Err(refused(
            "EFFECTIVE_CONFIG_MISMATCH",
            format!(
                "the signed definition {} says model {:?}, access {:?}, role {:?}, which is not what this setup chose",
                head.id, definition.model, definition.respond_to, definition.home_role
            ),
        ));
    }
    let request = LifecycleRequest {
        agent_pubkey: actor.to_string(),
        channels: vec![setup.spec.channel],
        ..Default::default()
    };
    let discovery = discovery_of(setup);
    let deadline = std::time::Instant::now() + LEASE_WAIT;
    let status = loop {
        let status = crate::lifecycle::status_in(&request, discovery, client).await?;
        let available = status["responsibilities"]
            .as_array()
            .is_some_and(|rows| rows.iter().any(|row| row["runtime_available"] == true));
        if available || !wait_for_lease || std::time::Instant::now() >= deadline {
            break status;
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    };
    let live = status["responsibilities"]
        .as_array()
        .and_then(|rows| rows.iter().find(|row| row["runtime_available"] == true));

    // What the provider itself signed about this generation, by the exact event
    // the responsibility came from.
    let metadata_id = live
        .or_else(|| {
            status["responsibilities"]
                .as_array()
                .and_then(|rows| rows.last())
        })
        .and_then(|row| row["responsibility"]["metadataEventId"].as_str())
        .map(str::to_owned);
    let mut provider_view = None;
    if let Some(id) = metadata_id {
        let rows = client
            .query_all(json!({"kinds":[44223],"authors":[config.provider_pubkey],"ids":[id],"#h":[setup.spec.channel.to_string()]}))
            .await?;
        provider_view = rows
            .into_iter()
            .filter_map(|value| serde_json::from_value::<Event>(value).ok())
            .find(|event| beekeeper_core::verify_event(event).is_ok())
            .and_then(|event| {
                let content: Value = serde_json::from_str(&event.content).ok()?;
                Some(json!({"eventId":event.id.to_hex(),"model":content["model"],"runtime":content["runtime"],
                    "status":content["status"],"credentialSource":content["credentialSource"]}))
            });
    }
    // A provider that says it runs something else is not what was chosen, however
    // the definition reads.
    if let Some(signed) = provider_view
        .as_ref()
        .and_then(|view| view["model"].as_str())
        .filter(|signed| *signed != selection)
    {
        return Err(refused(
            "EFFECTIVE_CONFIG_MISMATCH",
            format!(
                "the provider signed model `{signed}` for this generation, and this setup chose `{selection}`"
            ),
        ));
    }
    // No runtime reports the effort a turn ran at: the signed model is all there is,
    // so an effort it does not name was not applied as far as anyone can show.
    let signed_model = provider_view
        .as_ref()
        .and_then(|view| view["model"].as_str());
    if setup.spec.effort.is_some() && live.is_some() && signed_model != Some(selection.as_str()) {
        return Err(refused(
            "EFFECTIVE_CONFIG_MISMATCH",
            format!(
                "this setup chose `{selection}`, and the provider signed {:?} for this generation, so that effort is unconfirmed",
                signed_model
            ),
        ));
    }
    Ok(json!({
        "readAt": now(),
        "definition": {"eventId":head.id.to_hex(),"model":definition.model,"respondTo":definition.respond_to,
                       "allowlist":definition.respond_to_allowlist,"homeRole":definition.home_role},
        "provider": provider_view,
        "execution": live
            .or_else(|| status["responsibilities"].as_array().and_then(|rows| rows.last()))
            .map(|row| row["execution"].clone()),
        "availability": live.map(|row| row["availability"].clone()),
        "turnEvidence": "This reading does not show a turn. Only a signed turn result proves the runtime's account works.",
    }))
}

async fn start_and_verify(
    context: &Context,
    client: &impl RelayTransport,
    config: &HostConfig,
    setup: &mut AgentSetup,
    actor: &str,
    operation_id: Uuid,
) -> Result<(), Error> {
    if let Some(held) = setup
        .pending_action
        .as_mut()
        .filter(|held| held.operation_id == operation_id)
    {
        held.attempted = true;
    }
    begin(config, setup, StepName::Start)?;
    let request = start_request(config, setup, actor, operation_id)?;
    let started = crate::start(&request, client, context).await?;
    // The service's success is the evidence, whichever path it took: an execution
    // the provider already adopted leaves that operation's own journal without a
    // target. Saved as one write with the promotion, so no crash can leave a
    // confirmed start beside a stale current one.
    let target = started
        .get("target")
        .filter(|target| !target.is_null())
        .cloned()
        .ok_or_else(|| {
            Error::Unconfirmed(
                "START_TARGET_MISSING: a successful start did not identify its execution".into(),
            )
        })
        .and_then(|target| serde_json::from_value(target).map_err(local::fail))?;
    setup.confirmed_start = Some(agent_setup::ConfirmedSetupStart {
        operation_id,
        target,
    });
    setup.start_operation = operation_id;
    setup.pending_action = None;
    setup.receipt_event_id = started
        .get("receipt_event_id")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| setup.receipt_event_id.clone());
    finish(setup, "done", None);
    save(config, setup)?;
    begin(config, setup, StepName::Verify)?;
    let view = readback(client, config, setup, actor, true).await?;
    if view["availability"].is_null() {
        setup.readback = Some(view);
        return Err(Error::Unconfirmed(
            "START_NOT_VERIFIED: the provider acknowledged the start, and no unexpired lease appeared; retry this setup".into(),
        ));
    }
    setup.readback = Some(view);
    finish(setup, "done", None);
    setup.state = SetupState::Running;
    setup.last_refusal = None;
    setup.pending_action = None;
    save(config, setup)
}

/// Give a lead its project scope for every turn, through the existing operations.
///
/// Founds an umbrella, which starts nothing and spends no turn, and binds it with
/// no hire selection. That absence is deliberate and is what the provider tells
/// the model: no listener is running, so it must not try to hire. Only a lead has
/// this context; any other role is recorded as not applicable.
async fn bind_project_context(
    context: &Context,
    client: &impl RelayTransport,
    config: &HostConfig,
    setup: &mut AgentSetup,
    actor: &str,
) -> Result<(), Error> {
    let lead = beekeeper_core::role_vocabulary::RoleVocabulary::foundation().lead();
    if setup.spec.role != lead {
        setup.project_context = Some("not_applicable".into());
        return Ok(());
    }
    let request = |operation_id, session_ref: Uuid, channel| crate::session_found::FoundRequest {
        operation_id,
        channel,
        session_ref: session_ref.to_string(),
    };
    let (session_ref, channel) = (setup.session_ref, setup.spec.channel);
    let founded = match crate::session_found::found(
        &request(setup.context_operation, session_ref, channel),
        client,
        context,
    )
    .await
    {
        // Read back after the refusal and absent: nothing was founded. The new id is
        // saved before it is used, so an interruption here cannot found twice.
        Err(error) if is_expired(&error) => {
            let closed = setup.context_operation;
            let fresh = close_attempt(
                setup,
                "context",
                closed,
                crate::management::JOURNAL_EVENT_EXPIRED,
                format!("{error}; read back and not stored"),
            );
            setup.context_operation = fresh;
            save(config, setup)?;
            crate::session_found::found(&request(fresh, session_ref, channel), client, context)
                .await?
        }
        other => other?,
    };
    let genesis: Event = serde_json::from_value(founded["event"].clone()).map_err(fail)?;
    let scope = crate::hire::Scope {
        project: setup.spec.project.clone(),
        channel: setup.spec.channel,
        genesis,
    };
    crate::conversation_project::configure(context, client, &scope, actor, None).await?;
    setup.genesis_event_id = Some(scope.genesis.id.to_hex());
    setup.project_context = Some("bound".into());
    Ok(())
}

/// Run or resume the chain. A step that fails is recorded, and the journal comes
/// back with the refusal in it rather than an error: the record is the answer.
pub async fn apply(
    context: &Context,
    client: &impl RelayTransport,
    probe: &ProviderProbe,
    setup_id: Uuid,
    templates: &Path,
    resume_cancelled: bool,
) -> Result<AgentSetup, Error> {
    let config = config_for(context, client)?;
    let _lock = lock(&config)?;
    let mut setup = load(&config, setup_id)?;
    signer_guard(client, &setup)?;
    match setup.state {
        SetupState::Running => return Ok(setup),
        SetupState::Stopped => {
            return Err(refused(
                "SETUP_STOPPED",
                "this agent is stopped; use start rather than apply",
            ))
        }
        SetupState::Cancelled if !resume_cancelled => {
            return Err(refused(
                "SETUP_CANCELLED",
                "this setup was cancelled; resume it explicitly to continue",
            ))
        }
        _ => {}
    }
    if resume_cancelled && setup.state == SetupState::Cancelled {
        refuse_resume_over_another(&config, &setup)?;
    }
    // Retry does what was asked: a held stop is finished, never turned into a start.
    if let Some(held) = setup
        .pending_action
        .as_ref()
        .filter(|held| held.action == "stop")
    {
        let operation = held.operation_id;
        let actor = setup
            .agent_pubkey
            .clone()
            .ok_or_else(|| fail("a stop is held for a setup with no identity"))?;
        run_stop(context, client, &config, &mut setup, actor, operation).await?;
        return Ok(setup);
    }

    begin(&config, &mut setup, StepName::Preflight)?;
    let own = reserved_actor(context, &setup)?;
    let report = match preflight(
        context,
        client,
        probe,
        &setup.spec,
        templates,
        own.as_deref(),
    )
    .await
    {
        Ok(report) => report,
        Err(error) => {
            attention(&config, &mut setup, &error)?;
            return Ok(setup);
        }
    };
    if let Some(check) = report.first_refusal() {
        attention(&config, &mut setup, &refused(&check.code, &check.message))?;
        return Ok(setup);
    }
    finish(&mut setup, "done", None);

    if setup.agent_pubkey.is_none() {
        begin(&config, &mut setup, StepName::Install)?;
        let (policy, allowlist) = respond_to(&setup.spec.access);
        let installed = role_install::install(
            context,
            client,
            &RoleInstallRequest {
                operation_id: setup.install_operation,
                channel: setup.spec.channel,
                project: setup.spec.project.clone(),
                role: setup.spec.role.clone(),
                name: setup.spec.display_name.clone(),
                provider_instance: setup.spec.provider_instance.clone(),
                model: setup.spec.selection(),
                parallelism: setup.spec.parallelism,
                templates: templates.to_path_buf(),
                respond_to: policy,
                respond_to_allowlist: allowlist,
                existing_actor: setup.spec.existing_actor.clone(),
            },
        )
        .await;
        match installed {
            Ok(value) => {
                setup.agent_pubkey = value["agent_pubkey"].as_str().map(str::to_owned);
                setup.revision = value["revision"].as_str().map(str::to_owned);
                finish(&mut setup, "done", None);
                save(&config, &mut setup)?;
            }
            Err(error) if is_expired(&error) && setup.spec.existing_actor.is_some() => {
                retire_expired_install(context, client, &config, &mut setup, &error).await?;
                return Ok(setup);
            }
            Err(error) => {
                attention(&config, &mut setup, &error)?;
                return Ok(setup);
            }
        }
    }
    let actor = setup
        .agent_pubkey
        .clone()
        .ok_or_else(|| fail("install returned no identity"))?;
    if setup.project_context.is_none() {
        begin(&config, &mut setup, StepName::Context)?;
        if let Err(error) = bind_project_context(context, client, &config, &mut setup, &actor).await
        {
            attention(&config, &mut setup, &error)?;
            return Ok(setup);
        }
        let outcome = setup.project_context.clone();
        finish(&mut setup, "done", outcome);
        save(&config, &mut setup)?;
    }
    // A start that landed and has since closed is finished, and repeating its id
    // would only ever read back that old answer. Anything unknown is not touched.
    let open = match open_for(&config, &actor) {
        Ok(open) => open,
        Err(error) => {
            attention(&config, &mut setup, &error)?;
            return Ok(setup);
        }
    };
    if start_status(context, &setup)? == StartStatus::Landed && open.is_empty() {
        let closed = current_start(&setup);
        close_attempt(
            &mut setup,
            "start",
            closed,
            "LANDED_AND_CLOSED",
            "its receipt is recorded, and the provider's record of the execution is closed".into(),
        );
        save(&config, &mut setup)?;
    }
    let operation = current_start(&setup);
    start_recorded(context, client, &config, &mut setup, &actor, operation).await?;
    Ok(setup)
}

/// Where this setup's own reads look: an adopted identity's stay in its channel.
fn discovery_of(setup: &AgentSetup) -> crate::lifecycle::Discovery {
    if setup.spec.existing_actor.is_some() {
        crate::lifecycle::Discovery::NamedChannels
    } else {
        crate::lifecycle::Discovery::Membership
    }
}

/// A cancelled setup of an existing identity holds no claim on it, so another setup
/// may have taken it since. Resuming then would make two setups for one identity.
/// Runs under the setup lock, before anything changes.
fn refuse_resume_over_another(config: &HostConfig, setup: &AgentSetup) -> Result<(), Error> {
    let Some(actor) = setup.spec.existing_actor.as_deref() else {
        return Ok(());
    };
    let (others, unreadable) = agent_setup::list_setups(&config.provider_state_dir);
    if let Some(sentence) = unreadable.first() {
        return Err(refused(
            "SETUP_JOURNAL_UNREADABLE",
            format!("{sentence}; refusing to resume, which could duplicate an identity"),
        ));
    }
    let taken = others.iter().find(|other| {
        other.setup_id != setup.setup_id
            && other.state != SetupState::Cancelled
            && (other.spec.existing_actor.as_deref() == Some(actor)
                || other.agent_pubkey.as_deref() == Some(actor))
    });
    if let Some(other) = taken {
        return Err(refused(
            "SETUP_ALREADY_EXISTS",
            format!(
                "setup {} now holds identity {actor}, so this cancelled setup stays cancelled",
                other.setup_id
            ),
        ));
    }
    Ok(())
}

/// An existing identity's install aged out on the relay. Nothing is re-signed. If
/// the definition and membership still read as that attempt left them, the next
/// Retry starts a new attempt, which reads both again. If not, it stays refused.
async fn retire_expired_install(
    context: &Context,
    client: &impl RelayTransport,
    config: &HostConfig,
    setup: &mut AgentSetup,
    error: &Error,
) -> Result<(), Error> {
    let actor = setup
        .spec
        .existing_actor
        .clone()
        .ok_or_else(|| fail("no existing identity"))?;
    let path = local::operation_path(context, setup.install_operation)?;
    let held = local::read_stored_operation(&path)?;
    let events = held.as_ref().map(|op| op.events.as_slice()).unwrap_or(&[]);
    let journaled = events
        .iter()
        .find(|event| event.kind.as_u16() == 30177)
        .map(|event| event.id.to_hex());
    let added = events.iter().any(|event| event.kind.as_u16() == 9000);
    let head = current_head(client, &actor).await?.id.to_hex();
    let member = channel_role(client, setup.spec.channel, &actor)
        .await?
        .is_some();
    if let Err(refusal) =
        crate::adopt_identity::may_retry_install(journaled.as_deref(), &head, member, added)
    {
        return attention(config, setup, &refusal);
    }
    let old = setup.install_operation;
    setup.install_operation = close_attempt(
        setup,
        "install",
        old,
        crate::management::JOURNAL_EVENT_EXPIRED,
        error.to_string(),
    );
    setup.pending_action = None;
    attention(config, setup, error)
}

/// Record that the operator stopped this setup, and what already exists. This is
/// not an undo: identities are retired with `bee agents delete`, on purpose.
pub fn cancel(
    context: &Context,
    config: &HostConfig,
    probe: &ProviderProbe,
    setup_id: Uuid,
) -> Result<Value, Error> {
    let _lock = lock(config)?;
    let mut setup = load(config, setup_id)?;
    if setup.state == SetupState::Running {
        return Err(refused(
            "SETUP_RUNNING",
            "this agent is running; stop it instead of cancelling its setup",
        ));
    }
    let reserved = reserved_actor(context, &setup)?;
    setup.state = SetupState::Cancelled;
    save(config, &mut setup)?;
    // No claim yet on an existing identity: another setup may take it. Its custody,
    // and any definition revision already published, stay.
    let unclaimed = setup.spec.existing_actor.is_some() && setup.agent_pubkey.is_none();
    let definition_changed = setup.spec.existing_actor.as_deref().is_some_and(|actor| {
        config
            .provider_state_dir
            .join("actors")
            .join(actor)
            .join(crate::adopt_identity::PRE_ADOPTION_DEFINITION)
            .exists()
    });
    let published = definition_changed
        || setup
            .steps
            .iter()
            .any(|step| step.step == StepName::Install && step.state == "done");
    let held: Vec<agent_setup::OpenExecution> =
        agent_setup::open_executions(&config.provider_state_dir)
            .into_iter()
            // An unclaimed setup started nothing: anything open belongs to another setup.
            .filter(|row| !unclaimed && reserved.as_deref() == Some(row.actor.as_str()))
            .collect();
    let live = probe.live_pid.is_some();
    let stop_held = setup
        .pending_action
        .as_ref()
        .is_some_and(|held| held.action == "stop" && held.attempted);
    let start_unknown = start_status(context, &setup)? == StartStatus::Unknown;
    let unknown = if stop_held {
        Some("stop")
    } else if start_unknown {
        Some("start")
    } else {
        None
    };
    let control = if stop_held {
        "A stop was sent and its outcome is unknown: Retry stop finishes it."
    } else if start_unknown {
        "A start was sent and its outcome is unknown: Resume learns which, and then it can be stopped."
    } else {
        "Stop it."
    };
    let next = if !held.is_empty() && live {
        format!("The agent is still running: cancelling recorded your intent and stopped nothing. {control}")
    } else if !held.is_empty() {
        format!("The provider's record holds an open execution of this agent. Its provider is not running, so nothing runs now, and it comes back when the provider does. {control}")
    } else if stop_held {
        control.to_string()
    } else if start_unknown {
        "A start was sent and its outcome is unknown, so the agent may still start. Resume learns which.".to_string()
    } else if unclaimed {
        let change = if definition_changed {
            " Its definition was already revised, and that stays; the prior signed one is kept on this Host."
        } else {
            ""
        };
        format!(
            "This setup holds no claim on the identity, whose custody stays on this Host.{change} A new setup for this project and role may start, and resuming this one is then refused."
        )
    } else if reserved.is_some() && published {
        "The identity is kept. Resume to use it, or retire it with `bee agents delete`. A new setup for this project and role stays refused.".to_string()
    } else if reserved.is_some() {
        "An identity is reserved and not fully published. Resume to finish with it. A new setup for this project and role stays refused.".to_string()
    } else {
        "Nothing was reserved. A new setup for this project and role may start.".to_string()
    };
    Ok(json!({
        "setup": setup,
        "residual": {
            "identityReserved": if unclaimed { None } else { reserved.as_deref() },
            "published": published,
            "definitionChanged": definition_changed,
            "executionsHeld": held,
            "providerRunning": live,
            "actionOutcomeUnknown": unknown,
            "next": next,
        },
    }))
}

/// Start a stopped agent again, with an operation id persisted before use.
pub async fn start(
    context: &Context,
    client: &impl RelayTransport,
    setup_id: Uuid,
) -> Result<AgentSetup, Error> {
    let config = config_for(context, client)?;
    let _lock = lock(&config)?;
    let mut setup = load(&config, setup_id)?;
    signer_guard(client, &setup)?;
    let actor = setup.agent_pubkey.clone().ok_or_else(|| {
        refused(
            "SETUP_NOT_INSTALLED",
            "nothing is installed yet; apply this setup first",
        )
    })?;
    if let Some(held) = setup
        .pending_action
        .as_ref()
        .filter(|held| held.action == "stop" && held.attempted)
    {
        return Err(refused(
            "STOP_UNCONFIRMED",
            format!(
                "stop {} was sent and its outcome is unknown; retry it before starting",
                held.operation_id
            ),
        ));
    }
    // A lead must not run without its project scope, however it got here.
    if setup.project_context.is_none() {
        begin(&config, &mut setup, StepName::Context)?;
        if let Err(error) = bind_project_context(context, client, &config, &mut setup, &actor).await
        {
            attention(&config, &mut setup, &error)?;
            return Ok(setup);
        }
        let outcome = setup.project_context.clone();
        finish(&mut setup, "done", outcome);
        save(&config, &mut setup)?;
    }
    // Not an early return for a stored `running`: the journal cannot know the
    // provider lost it, and a start of a live execution reuses it. A start whose
    // outcome is unknown is the one to repeat; a new id could only run it twice.
    let operation = if start_status(context, &setup)? == StartStatus::Unknown {
        current_start(&setup)
    } else {
        pending(&mut setup, "start")
    };
    save(&config, &mut setup)?;
    start_recorded(context, client, &config, &mut setup, &actor, operation).await?;
    Ok(setup)
}

/// The id of the requested action, reusing the one a lost reply left behind.
fn pending(setup: &mut AgentSetup, action: &str) -> Uuid {
    if let Some(held) = setup
        .pending_action
        .as_ref()
        .filter(|held| held.action == action)
    {
        return held.operation_id;
    }
    let operation_id = Uuid::new_v4();
    setup.pending_action = Some(PendingAction {
        action: action.to_string(),
        operation_id,
        attempted: false,
    });
    operation_id
}

/// Stop it, waiting for the signed terminal receipt. The identity stays.
pub async fn stop(
    context: &Context,
    client: &impl RelayTransport,
    setup_id: Uuid,
) -> Result<AgentSetup, Error> {
    let config = config_for(context, client)?;
    let _lock = lock(&config)?;
    let mut setup = load(&config, setup_id)?;
    signer_guard(client, &setup)?;
    let actor = setup.agent_pubkey.clone().ok_or_else(|| {
        refused(
            "SETUP_NOT_INSTALLED",
            "nothing is installed yet, so there is nothing to stop",
        )
    })?;
    let stop_held = setup
        .pending_action
        .as_ref()
        .is_some_and(|held| held.action == "stop");
    if !stop_held {
        let status = start_status(context, &setup)?;
        if status == StartStatus::Unknown {
            return Err(refused(
                "START_UNCONFIRMED",
                format!(
                    "start {} was sent and its outcome is unknown; retry or resume it, so a stop cannot pass a start still on its way",
                    current_start(&setup)
                ),
            ));
        }
        if status == StartStatus::NotSent && open_for(&config, &actor)?.is_empty() {
            return Err(refused(
                "NOTHING_TO_STOP",
                "this agent has not been started, so nothing was sent and nothing changed",
            ));
        }
    }
    let operation = pending(&mut setup, "stop");
    run_stop(context, client, &config, &mut setup, actor, operation).await?;
    Ok(setup)
}

/// Send or resend one stop, and record how it ended.
async fn run_stop(
    context: &Context,
    client: &impl RelayTransport,
    config: &HostConfig,
    setup: &mut AgentSetup,
    actor: String,
    operation: Uuid,
) -> Result<(), Error> {
    if let Some(held) = setup.pending_action.as_mut() {
        held.attempted = true;
    }
    begin(config, setup, StepName::Stop)?;
    let mut operation = operation;
    let mut replaced = false;
    loop {
        let request = LifecycleRequest {
            operation_id: operation,
            agent_pubkey: actor.clone(),
            channels: vec![setup.spec.channel],
            wait_secs: RECEIPT_WAIT_SECS,
        };
        match crate::lifecycle::stop_in(&request, discovery_of(setup), client, context).await {
            Ok(value) if value["terminal"] == true => {
                finish(setup, "done", None);
                setup.state = SetupState::Stopped;
                setup.pending_action = None;
                setup.last_refusal = None;
                return save(config, setup);
            }
            Ok(value) => {
                return attention(
                    config,
                    setup,
                    &Error::Unconfirmed(format!(
                        "STOP_NOT_CONFIRMED: no terminal receipt yet; retry ({value})"
                    )),
                )
            }
            // A stop of the same execution is idempotent, and this one was read
            // back and is not stored. Replaced once; anything else is recorded.
            Err(error) if is_expired(&error) && !replaced => {
                replaced = true;
                operation = close_attempt(
                    setup,
                    "stop",
                    operation,
                    crate::management::JOURNAL_EVENT_EXPIRED,
                    format!("{error}; read back and not stored"),
                );
                if let Some(held) = setup.pending_action.as_mut() {
                    held.attempted = true;
                }
                save(config, setup)?;
            }
            Err(error) => {
                // Nothing signed means nothing sent: no action is left held.
                let path = local::operation_path(context, operation)?;
                let signed = local::read_operation_value(&path)?
                    .and_then(|journal| {
                        journal["events"]
                            .as_array()
                            .map(|events| !events.is_empty())
                    })
                    .unwrap_or(false);
                if !signed {
                    setup.pending_action = None;
                }
                return attention(config, setup, &error);
            }
        }
    }
}

/// The journal beside a fresh signed reading. Read-only.
pub async fn inspect(
    context: &Context,
    client: &impl RelayTransport,
    setup_id: Uuid,
) -> Result<Value, Error> {
    let config = config_for(context, client)?;
    let setup = load(&config, setup_id)?;
    let reserved = reserved_actor(context, &setup)?;
    let live = match setup.agent_pubkey.as_deref() {
        None => Value::Null,
        Some(actor) => match readback(client, &config, &setup, actor, false).await {
            Ok(view) => view,
            Err(error) => json!({"unavailable": error.to_string()}),
        },
    };
    Ok(json!({
        "effectiveState": agent_setup::effective_state(
            setup.state,
            agent_setup::apply_lock_held(&config.provider_state_dir),
        ),
        "identityReserved": reserved,
        "setup": setup,
        "live": live,
    }))
}

#[cfg(test)]
#[path = "project_agent_setup_tests.rs"]
mod tests;
