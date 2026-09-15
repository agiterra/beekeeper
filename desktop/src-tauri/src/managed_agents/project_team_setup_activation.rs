//! Durable project-role installation, channel reservation, and lead launch.
//! The publication journal owns the fixed inputs; this module only advances or
//! observes the retained local activation operation.

use super::*;
use crate::commands::project_git_exec::build_git_auth_config_for_keys;
use crate::managed_agents::{project_agent_association as association, ManagedAgentRecord};
use nostr::nips::nip44;

#[path = "project_team_setup_activation_channel.rs"]
pub(crate) mod channel;
#[path = "project_team_setup_activation_projection.rs"]
mod projection;
#[cfg(test)]
#[path = "project_team_setup_activation_test_helpers.rs"]
mod test_helpers;
pub(super) use projection::activation;
use projection::observed_activation;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTeamActivationSource {
    pub repo_ref: String,
    pub commit: String,
    pub pack_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTeamInstalledRole {
    pub role: String,
    pub agent_pubkey: String,
    pub pack_ref: packs_cache::PackRef,
}

#[allow(dead_code)] // IPC schema reserves terminal states for the lead launcher.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectTeamInstallationStatus {
    NotInstalled,
    Installed,
    Unknown,
    Refused,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTeamInstallation {
    pub status: ProjectTeamInstallationStatus,
    pub installed_roles: Vec<ProjectTeamInstalledRole>,
    pub message: Option<String>,
}

#[allow(dead_code)] // The launch journal reports these states once its seam lands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectTeamLeadStatus {
    NeedsChannel,
    Ready,
    Starting,
    Started,
    Unknown,
    Refused,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTeamLead {
    pub status: ProjectTeamLeadStatus,
    pub channel_id: Option<String>,
    pub session_ref: Option<String>,
    /// The reserved lead identity, or `None` when no lead is installed.
    pub lead_pubkey: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTeamActivation {
    pub source: Option<ProjectTeamActivationSource>,
    pub installation: ProjectTeamInstallation,
    pub lead: ProjectTeamLead,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct InstallationJournal {
    pub(super) team_id: String,
    /// Every fresh role identity is encrypted to the setup owner and saved
    /// before a persona, agent, or team store is changed.  The journal can
    /// therefore replay one exact install after any store-write boundary
    /// without carrying an nsec in plaintext.
    #[serde(default)]
    pub(super) planned_roles: Vec<PlannedRoleIdentity>,
    /// A host-owned project transport-channel request. Its UUID and signed
    /// create bytes survive a close or a lost relay response, so a retry never
    /// asks the browser to invent another channel.
    #[serde(default)]
    pub(super) channel: Option<ActivationChannelJournal>,
    pub(super) roles: Vec<ProjectTeamInstalledRole>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ActivationChannelJournal {
    pub(super) channel_id: String,
    pub(super) create_event: Event,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct PlannedRoleIdentity {
    pub(super) role: String,
    pub(super) pubkey: String,
    /// NIP-44 ciphertext to the owner, never an nsec or a keyring reference.
    pub(super) encrypted_secret: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PlannedRoleSecret {
    pubkey: String,
    private_key_nsec: String,
    auth_tag: String,
}

fn reserve_role_identity(
    owner: &nostr::Keys,
    role: &str,
) -> Result<PlannedRoleIdentity, SetupError> {
    let (_, minted) = crate::commands::mint_agent_identity(owner).map_err(external)?;
    let auth_tag = minted
        .auth_tag
        .ok_or_else(|| invalid("The planned project role identity has no owner attestation."))?;
    let secret = PlannedRoleSecret {
        pubkey: minted.pubkey.clone(),
        private_key_nsec: minted.private_key_nsec,
        auth_tag,
    };
    let plaintext = serde_json::to_vec(&secret).map_err(|error| invalid(error.to_string()))?;
    let encrypted_secret = nip44::encrypt(
        owner.secret_key(),
        &owner.public_key(),
        plaintext,
        nip44::Version::V2,
    )
    .map_err(|error| invalid(error.to_string()))?;
    Ok(PlannedRoleIdentity {
        role: role.to_string(),
        pubkey: minted.pubkey,
        encrypted_secret,
    })
}

fn recover_role_identity(
    owner: &nostr::Keys,
    planned: &PlannedRoleIdentity,
) -> Result<crew_roles::MintedCrewIdentity, SetupError> {
    if planned.role.trim().is_empty() {
        return Err(invalid("A planned project role has no role name."));
    }
    let plaintext = nip44::decrypt(
        owner.secret_key(),
        &owner.public_key(),
        &planned.encrypted_secret,
    )
    .map_err(|_| {
        invalid("The planned project role identity cannot be authenticated by this owner.")
    })?;
    let secret: PlannedRoleSecret = serde_json::from_str(&plaintext)
        .map_err(|_| invalid("The planned project role identity is malformed."))?;
    let keys = nostr::Keys::parse(&secret.private_key_nsec)
        .map_err(|_| invalid("The planned project role key is malformed."))?;
    if secret.pubkey != planned.pubkey || keys.public_key().to_hex() != planned.pubkey {
        return Err(invalid(
            "The planned project role identity does not match its preserved key.",
        ));
    }
    let attested = buzz_sdk_pkg::nip_oa::verify_auth_tag(&secret.auth_tag, &keys.public_key())
        .map_err(|_| invalid("The planned project role owner attestation is invalid."))?;
    if attested != owner.public_key() {
        return Err(invalid("The planned project role has a different owner."));
    }
    Ok(crew_roles::MintedCrewIdentity {
        pubkey: secret.pubkey,
        private_key_nsec: secret.private_key_nsec,
        auth_tag: Some(secret.auth_tag),
    })
}

fn missing_role_packs<'a>(
    scan: &'a crew_roles::RolePackScan,
    agents: &[ManagedAgentRecord],
    team_id: &str,
) -> Vec<&'a crew_roles::DiscoveredRolePack> {
    scan.packs
        .iter()
        .filter(|pack| {
            !agents.iter().any(|agent| {
                agent.team_id.as_deref() == Some(team_id)
                    && agent.persona_team_dir.as_deref() == Some(pack.dir.as_path())
                    && agent.persona_name_in_team.as_deref() == Some(pack.persona_name.as_str())
            })
        })
        .collect()
}

fn reserve_installation_identities(
    journal: &mut PublicationJournal,
    scan: &crew_roles::RolePackScan,
    owner: &nostr::Keys,
) -> Result<(), SetupError> {
    let installation = journal
        .installation
        .as_mut()
        .ok_or_else(|| invalid("The project installation has no durable team reservation."))?;
    let mut roles = std::collections::HashSet::new();
    for planned in &installation.planned_roles {
        if !roles.insert(planned.role.as_str()) {
            return Err(invalid(
                "A project role identity was reserved more than once.",
            ));
        }
        let _ = recover_role_identity(owner, planned)?;
    }
    for pack in &scan.packs {
        if installation
            .planned_roles
            .iter()
            .any(|planned| planned.role == pack.role)
        {
            continue;
        }
        installation
            .planned_roles
            .push(reserve_role_identity(owner, &pack.role)?);
    }
    Ok(())
}

fn expected_pack_ref(source: &ProjectTeamActivationSource, role: &str) -> packs_cache::PackRef {
    packs_cache::PackRef {
        repo: source.repo_ref.clone(),
        sha: source.commit.clone(),
        role: role.to_string(),
        path: format!("{}/{}", source.pack_path, role),
    }
}

fn exact_project_pack_source(
    source: &ProjectTeamActivationSource,
) -> crate::managed_agents::actor_seats::ProjectPackSourceInput {
    crate::managed_agents::actor_seats::ProjectPackSourceInput {
        repo: source.repo_ref.clone(),
        git_ref: None,
        sha: Some(source.commit.clone()),
        path: Some(source.pack_path.clone()),
    }
}

fn require_exact_staged_pack(
    staged: &crate::managed_agents::actor_seats::StagedActorSeat,
    expected: &packs_cache::PackRef,
) -> Result<(), SetupError> {
    if !staged.pack_staged || staged.pack_ref.as_ref() != Some(expected) {
        return Err(invalid(
            "The project lead seat was not staged from its adopted immutable role pack.",
        ));
    }
    Ok(())
}

async fn require_current_adopted_source(
    state: &AppState,
    journal: &PublicationJournal,
    source: &ProjectTeamActivationSource,
) -> Result<(), SetupError> {
    let saved_event = journal
        .source_event
        .as_ref()
        .ok_or_else(|| invalid("The adopted project source has no saved event."))?;
    let current = current_source(state, &journal.project_ref).await?;
    let Some(current) = current else {
        return Err(invalid(
            "The adopted project source is no longer effective; reopen publication before installing or launching.",
        ));
    };
    if current.event_id != saved_event.id.to_hex()
        || current.destination.repo_ref != source.repo_ref
        || current.destination.pack_path != source.pack_path
        || current.destination.base_commit.as_deref() != Some(source.commit.as_str())
    {
        return Err(invalid(
            "The adopted project source changed; refuse to install or launch from a stale revision.",
        ));
    }
    Ok(())
}

pub(super) fn install_adopted_roles(
    app: &AppHandle,
    draft: &ProjectTeamSetupDraft,
    journal: &mut PublicationJournal,
    owner: &nostr::Keys,
) -> Result<(), SetupError> {
    let source = adopted_source(journal)?;
    let (repo_owner, repo_id) =
        packs_cache::parse_repo_coordinate(&source.repo_ref).map_err(invalid)?;
    let source_for_checkout = packs_cache::ProjectPackSource {
        repo: source.repo_ref.clone(),
        git_ref: None,
        sha: Some(source.commit.clone()),
        path: source.pack_path.clone(),
    };
    let root = packs_cache::packs_root(app).map_err(external)?;
    let checkout = packs_cache::packs_checkout_dir(&root, &repo_owner, &repo_id);
    let remote = packs_cache::packs_clone_url(
        &crate::relay::relay_http_base_url(&draft.relay_url),
        &repo_owner,
        &repo_id,
    );
    let auth = build_git_auth_config_for_keys(owner).map_err(external)?;
    let observed =
        packs_cache::sync_packs_checkout(&checkout, &remote, &source_for_checkout, &auth)
            .map_err(external)?;
    if observed != source.commit {
        return Err(invalid(
            "The installed checkout did not resolve the adopted immutable commit.",
        ));
    }
    let packs = checkout.join(&source.pack_path);
    let scan = crew_roles::scan_role_packs(&packs).map_err(invalid)?;
    if scan.packs.iter().all(|pack| pack.role != "lead") {
        return Err(invalid(
            "The adopted project source contains no lead role to install.",
        ));
    }
    reserve_installation_identities(journal, &scan, owner)?;
    // The encrypted plan is the crash boundary: no agent/persona/team store
    // is touched until every role has one exact recoverable key.
    save_journal(draft, journal)?;
    let team_id = journal
        .installation
        .as_ref()
        .ok_or_else(|| invalid("The project installation reservation disappeared."))?
        .team_id
        .clone();
    let store = app.state::<AppState>();
    let _guard = store
        .managed_agents_store_lock
        .lock()
        .map_err(|_| invalid("Managed-agent storage lock is unavailable."))?;
    let definitions = load_personas(app).map_err(external)?;
    let agents = load_managed_agents(app).map_err(external)?;
    let teams = load_teams(app).map_err(external)?;
    let installation = journal
        .installation
        .as_ref()
        .ok_or_else(|| invalid("The project installation reservation disappeared."))?;
    let planned_roles = installation.planned_roles.clone();
    let channel = installation.channel.clone();
    let mut planned = missing_role_packs(&scan, &agents, &installation.team_id)
        .into_iter()
        .map(|pack| {
            installation
                .planned_roles
                .iter()
                .find(|identity| identity.role == pack.role)
                .ok_or_else(|| invalid("A missing project role has no reserved identity."))
                .and_then(|identity| recover_role_identity(owner, identity))
        })
        .collect::<Vec<_>>()
        .into_iter();
    let mut mint = || {
        planned
            .next()
            .ok_or_else(|| {
                "The saved project identity plan has no identity for this role.".to_string()
            })?
            .map_err(|error| error.message)
    };
    let mut result = crew_roles::install_role_packs_in_named_team(
        &scan,
        definitions,
        agents,
        &teams,
        &crate::util::now_iso(),
        &Default::default(),
        &mut mint,
        team_id.clone(),
        &format!("Project team {}", draft.project_ref),
    )
    .map_err(|error| external(error.detail))?;
    association::associate_installation(&mut result.agents, &draft.project_ref, &result.installed)
        .map_err(invalid)?;
    save_personas(app, &result.definitions).map_err(external)?;
    save_managed_agents(app, &result.agents).map_err(external)?;
    association::retain_installed_agents(app, &store, &result.agents, &result.installed);
    let mut next_teams: Vec<_> = teams
        .into_iter()
        .filter(|team| team.id != result.team.id)
        .collect();
    next_teams.push(result.team);
    save_teams(app, &next_teams).map_err(external)?;
    let roles = result
        .installed
        .into_iter()
        .map(|role| ProjectTeamInstalledRole {
            pack_ref: expected_pack_ref(&source, &role.role),
            role: role.role,
            agent_pubkey: role.agent_pubkey,
        })
        .collect();
    journal.installation = Some(InstallationJournal {
        team_id,
        planned_roles,
        channel,
        roles,
    });
    Ok(())
}
/// Read the adopted source and this computer's separate installation fact.
/// This performs no checkout, identity mint or session launch.
#[tauri::command]
pub async fn project_team_setup_get_activation(
    app: AppHandle,
    state: State<'_, AppState>,
    setup_id: String,
    project_ref: String,
    expected_relay_url: String,
    publication_id: String,
) -> Result<ProjectTeamActivation, SetupError> {
    let (root, scope) = context(&app, &state, &project_ref, &expected_relay_url)?;
    let draft = bound_draft(&root, &scope, &setup_id)?;
    let journal = load_journal(&draft)?
        .ok_or_else(|| invalid("Start publication before reading activation."))?;
    if journal.publication_id != publication_id {
        return Err(invalid(
            "The publication ID does not match this setup's journal.",
        ));
    }
    let keys = state.signing_keys().map_err(external)?;
    if keys.public_key().to_hex() != draft.owner_pubkey {
        return Err(invalid(
            "The signing identity changed before lead observation.",
        ));
    }
    verify_context(&state, &scope)?;
    Ok(observed_activation(&state, &draft, &journal, &keys).await)
}

/// Bind the one project session channel to this publication before a lead
/// launch. The channel is proved against the signed project metadata, then
/// retained so closing the workbench cannot turn a retry into another channel.
#[tauri::command]
pub async fn project_team_setup_record_lead_channel(
    app: AppHandle,
    state: State<'_, AppState>,
    setup_id: String,
    project_ref: String,
    expected_relay_url: String,
    publication_id: String,
    channel_id: String,
) -> Result<ProjectTeamActivation, SetupError> {
    let (root, scope) = context(&app, &state, &project_ref, &expected_relay_url)?;
    let draft = bound_draft(&root, &scope, &setup_id)?;
    let _guard = PUBLICATION_LOCK.lock().await;
    verify_context(&state, &scope)?;
    let mut journal = load_journal(&draft)?
        .ok_or_else(|| invalid("Start publication before recording a project session channel."))?;
    if journal.publication_id != publication_id {
        return Err(invalid(
            "The publication ID does not match this setup's journal.",
        ));
    }
    let keys = state.signing_keys().map_err(external)?;
    if keys.public_key().to_hex() != draft.owner_pubkey {
        return Err(invalid("The signing identity changed before lead handoff."));
    }
    if journal
        .installation
        .as_ref()
        .is_none_or(|installation| installation.roles.is_empty())
    {
        return Err(invalid(
            "Install the adopted project roles before selecting its session channel.",
        ));
    }
    if let Some(lead) = &journal.lead {
        if lead.channel_id != channel_id {
            return Err(invalid(
                "This publication already reserved a different project session channel; its lead request was preserved.",
            ));
        }
        verify_context(&state, &scope)?;
        return Ok(observed_activation(&state, &draft, &journal, &keys).await);
    }
    crate::managed_agents::project_team_setup::launch::wire::verify_project_channel(
        &state,
        &draft,
        &channel_id,
        &keys,
    )
    .await?;
    let lead_pubkey = journal
        .installation
        .as_ref()
        .and_then(|installation| installation.roles.iter().find(|role| role.role == "lead"))
        .map(|role| role.agent_pubkey.clone())
        .ok_or_else(|| invalid("The installed project roles contain no lead identity."))?;
    journal.lead = Some(LeadJournal {
        channel_id,
        lead_pubkey,
        session_ref: None,
        create_command_id: None,
        provider_pubkey: None,
        provider_instance_ref: None,
        runtime: None,
        driver: None,
        provider_host_instance_id: None,
        model: None,
        genesis_event: None,
        create_event: None,
        status: ProjectTeamLeadStatus::Ready,
        message: Some(
            "Project session channel is saved. Start the project lead when ready.".to_string(),
        ),
    });
    save_journal(&draft, &journal)?;
    verify_context(&state, &scope)?;
    Ok(observed_activation(&state, &draft, &journal, &keys).await)
}

/// Resolve the retained adopted SHA into this host's cache and install exactly
/// one identity per project role.  The journal's team id is retained on every
/// retry so a lost response cannot mint another lead.
#[tauri::command]
pub async fn project_team_setup_install_adopted_roles(
    app: AppHandle,
    state: State<'_, AppState>,
    setup_id: String,
    project_ref: String,
    expected_relay_url: String,
    publication_id: String,
) -> Result<ProjectTeamActivation, SetupError> {
    let (root, scope) = context(&app, &state, &project_ref, &expected_relay_url)?;
    let draft = bound_draft(&root, &scope, &setup_id)?;
    let _guard = PUBLICATION_LOCK.lock().await;
    verify_context(&state, &scope)?;
    let mut journal = load_journal(&draft)?
        .ok_or_else(|| invalid("Start publication before installing roles."))?;
    if journal.publication_id != publication_id {
        return Err(invalid(
            "The publication ID does not match this setup's journal.",
        ));
    }
    let keys = state.signing_keys().map_err(external)?;
    if keys.public_key().to_hex() != draft.owner_pubkey {
        return Err(invalid("The signing identity changed before installation."));
    }
    let source = adopted_source(&journal)?;
    require_current_adopted_source(&state, &journal, &source).await?;
    verify_context(&state, &scope)?;
    if journal
        .installation
        .as_ref()
        .is_none_or(|installation| installation.roles.is_empty())
    {
        // Record the durable installation boundary before mutable local stores
        // are changed. The role list is filled only after their exact records
        // have been written.
        adopted_source(&journal)?;
        if journal.installation.is_none() {
            journal.installation = Some(InstallationJournal {
                team_id: uuid::Uuid::new_v4().to_string(),
                planned_roles: Vec::new(),
                channel: None,
                roles: Vec::new(),
            });
        }
        verify_context(&state, &scope)?;
        save_journal(&draft, &journal)?;
        let (install_app, install_draft) = (app.clone(), draft.clone());
        let updated = tokio::task::spawn_blocking(move || {
            install_adopted_roles(&install_app, &install_draft, &mut journal, &keys)
                .map(|_| journal)
        })
        .await
        .map_err(|error| external(error.to_string()))??;
        verify_context(&state, &scope)?;
        save_journal(&draft, &updated)?;
        journal = updated;
    }
    // Journals installed by an earlier build gain their association here too,
    // then every newly associated agent's project visibility is read off-path.
    let owner = scope.owner.clone();
    let _ = tokio::task::spawn_blocking(move || {
        association::backfill_project_agents_logged(&app, &owner);
        crate::managed_agents::project_association_authority::spawn_project_visibility_verification(
            app,
        );
    })
    .await;
    verify_context(&state, &scope)?;
    Ok(activation(&journal))
}

/// Start or retry the one lead session reserved for this adopted publication.
/// The exact genesis and create events are written before either reaches the
/// relay; therefore a lost response can only resend those same bytes.
#[tauri::command]
// Tauri injects the three host state handles as ordinary command arguments;
// the remaining five are the fixed IPC scope. Keeping this public wire shape
// avoids a compatibility-only wrapper that would merely unpack another struct.
#[allow(clippy::too_many_arguments)]
pub async fn project_team_setup_start_lead(
    app: AppHandle,
    state: State<'_, AppState>,
    provider: State<'_, CodingSessionProviderState>,
    setup_id: String,
    project_ref: String,
    expected_relay_url: String,
    publication_id: String,
    channel_id: String,
) -> Result<ProjectTeamActivation, SetupError> {
    let (root, scope) = context(&app, &state, &project_ref, &expected_relay_url)?;
    let draft = bound_draft(&root, &scope, &setup_id)?;
    let _guard = PUBLICATION_LOCK.lock().await;
    verify_context(&state, &scope)?;
    let mut journal = load_journal(&draft)?
        .ok_or_else(|| invalid("Start publication before handing work to its lead."))?;
    if journal.publication_id != publication_id {
        return Err(invalid(
            "The publication ID does not match this setup's journal.",
        ));
    }
    let keys = state.signing_keys().map_err(external)?;
    if keys.public_key().to_hex() != draft.owner_pubkey {
        return Err(invalid("The signing identity changed before lead handoff."));
    }
    let source = adopted_source(&journal)?;
    require_current_adopted_source(&state, &journal, &source).await?;
    let installed = journal
        .installation
        .as_ref()
        .ok_or_else(|| invalid("Install the adopted project roles before starting a lead."))?;
    let installed_lead = installed
        .roles
        .iter()
        .find(|role| role.role == "lead")
        .ok_or_else(|| invalid("The installed project roles contain no lead identity."))?;
    if installed_lead.pack_ref.repo != source.repo_ref
        || installed_lead.pack_ref.sha != source.commit
        || installed_lead.pack_ref.role != "lead"
        || installed_lead.pack_ref.path != format!("{}/lead", source.pack_path)
    {
        return Err(invalid(
            "The installed lead is not bound to this adopted project revision.",
        ));
    }
    let lead = journal.lead.as_mut().ok_or_else(|| {
        invalid("Create or select this project's session channel before starting its lead.")
    })?;
    if lead.channel_id != channel_id {
        return Err(invalid(
            "The supplied channel does not match the saved project session channel.",
        ));
    }
    if lead.lead_pubkey != installed_lead.agent_pubkey {
        return Err(invalid(
            "The saved lead identity no longer matches this installed project team.",
        ));
    }
    crate::managed_agents::project_team_setup::launch::wire::verify_project_channel(
        &state,
        &draft,
        &channel_id,
        &keys,
    )
    .await?;

    if lead.create_event.is_none() {
        let agents = load_managed_agents(&app).map_err(external)?;
        let agent = agents
            .iter()
            .find(|agent| agent.pubkey == lead.lead_pubkey)
            .ok_or_else(|| {
                invalid("The installed lead identity is unavailable on this computer.")
            })?;
        if agent.relay_url != draft.relay_url || agent.home_role.as_deref() != Some("lead") {
            return Err(invalid(
                "The installed lead no longer belongs to this project community and role.",
            ));
        }
        let readiness =
            provider_store::load_provider_readiness_store(&app, Some(&draft.owner_pubkey))
                .map_err(external)?;
        let provider_record = readiness.get(&draft.relay_url).ok_or_else(|| {
            external("Prepare the local session provider before starting the project lead.")
        })?;
        if !provider_record.auth_tag_present
            || provider_record.auth_tag_invalid
            || provider_record.auth_tag_owner_mismatch
        {
            return Err(invalid(
                "The local session provider is not authorized by this setup's owner.",
            ));
        }
        let runtimes = provider_commands::coding_session_provider_runtimes()
            .await
            .map_err(external)?;
        let runtime = runtimes
            .iter()
            .find(|runtime| {
                runtime.auth_state
                    == crate::session_provider::runtimes::CodingSessionRuntimeAuthState::Ready
                    && runtime.capabilities.thread_turn_start
                    && agent.runtime.as_deref().is_none_or(|configured| {
                        configured == runtime.runtime
                            || configured == runtime.instance_ref
                            || configured == runtime.driver
                    })
            })
            .ok_or_else(|| {
                external("No ready local runtime can launch this project's installed lead.")
            })?;
        let model = agent
            .model
            .clone()
            .unwrap_or_else(|| runtime.default_model.clone());
        if !runtime.allowed_models.contains(&model) {
            return Err(invalid(
                "The installed lead's model is not offered by the selected local runtime.",
            ));
        }
        let session_ref = uuid::Uuid::new_v4().to_string();
        let create_command_id = format!("csl-{}", uuid::Uuid::new_v4());
        let channel =
            uuid::Uuid::parse_str(&channel_id).map_err(|error| invalid(error.to_string()))?;
        let genesis_event = buzz_sdk_pkg::build_coding_session_genesis(
            channel,
            &CodingSessionGenesisPayload::new(&session_ref),
        )
        .map_err(|error| invalid(error.to_string()))?
        .sign_with_keys(&keys)
        .map_err(|error| invalid(error.to_string()))?;
        let payload = CodingSessionLifecycleCommandPayload {
            schema: CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA.into(),
            command_id: create_command_id.clone(),
            action: CodingSessionLifecycleAction::SessionCreate {
                project_ref: Some(draft.project_ref.clone()),
                repo_ref: Some(source.repo_ref.clone()),
                session_ref: Some(session_ref.clone()),
                genesis_ref: Some(genesis_event.id.to_hex()),
                provider_instance_ref: ProviderInstanceAlias::from_wire(
                    runtime.instance_ref.clone(),
                )
                .map_err(invalid)?,
                provider_authority_pubkey: provider_record.provider_pubkey.clone(),
                model: Some(model.clone()),
                title: Some("Project lead".to_string()),
                initial_turn: Some(projection::lead_initial_turn(
                    &source,
                    &projection::local_project_agents(
                        &draft.project_ref,
                        &installed.roles,
                        &agents,
                    ),
                )),
                actor: Some(lead.lead_pubkey.clone()),
                role: Some("lead".to_string()),
                hire_ref: None,
                routing: None,
            },
        };
        let create_event =
            buzz_sdk_pkg::builders::build_coding_session_lifecycle_command(channel, &payload)
                .map_err(|error| invalid(error.to_string()))?
                .sign_with_keys(&keys)
                .map_err(|error| invalid(error.to_string()))?;
        lead.session_ref = Some(session_ref);
        lead.create_command_id = Some(create_command_id);
        lead.provider_pubkey = Some(provider_record.provider_pubkey.clone());
        lead.provider_instance_ref = Some(runtime.instance_ref.clone());
        lead.runtime = Some(runtime.runtime.clone());
        lead.driver = Some(runtime.driver.clone());
        lead.provider_host_instance_id = Some(provider_record.instance_id.clone());
        lead.model = Some(model);
        lead.genesis_event = Some(genesis_event);
        lead.create_event = Some(create_event);
        lead.status = ProjectTeamLeadStatus::Starting;
        lead.message = Some(
            "The exact project lead request was saved; provider receipt is pending.".to_string(),
        );
        save_journal(&draft, &journal)?;
    }

    let lead = journal
        .lead
        .as_ref()
        .ok_or_else(|| invalid("The saved project lead reservation disappeared."))?;
    let create_command_id = lead
        .create_command_id
        .as_deref()
        .ok_or_else(|| invalid("The saved lead request has no command ID."))?;
    let provider_pubkey = lead
        .provider_pubkey
        .as_deref()
        .ok_or_else(|| invalid("The saved lead request has no provider."))?;
    let genesis_event = lead
        .genesis_event
        .as_ref()
        .ok_or_else(|| invalid("The saved lead request has no genesis."))?;
    let create_event = lead
        .create_event
        .as_ref()
        .ok_or_else(|| invalid("The saved lead request has no create event."))?;
    if !supervisor::ensure_running(&app, &provider, &draft.relay_url).map_err(external)? {
        return Err(external("The local session provider is not provisioned."));
    }
    crate::managed_agents::project_team_setup::launch::wire::ensure_membership(
        &state,
        &draft,
        &channel_id,
        &[provider_pubkey, &lead.lead_pubkey],
        &keys,
    )
    .await?;
    require_current_adopted_source(&state, &journal, &source).await?;
    verify_context(&state, &scope)?;
    let staged = crate::managed_agents::actor_seats::stage_coding_session_actor_seat(
        app.clone(),
        app.state::<AppState>(),
        create_command_id.to_string(),
        lead.lead_pubkey.clone(),
        Some("lead".to_string()),
        Some(exact_project_pack_source(&source)),
        None,
        Some(draft.project_ref.clone()),
        Some(true),
    )
    .await
    .map_err(external)?;
    require_exact_staged_pack(&staged, &expected_pack_ref(&source, "lead"))?;
    crate::coding_sessions::workdir_store::stage_coding_session_create_hint_at(
        &app,
        &draft.relay_url,
        create_command_id,
        Path::new(&draft.project_directory),
    )
    .map_err(external)?;
    let publish = async {
        crate::relay::submit_signed_event_with_keys(genesis_event, &state, &keys, None)
            .await
            .map_err(external)?;
        crate::relay::submit_signed_event_with_keys(create_event, &state, &keys, None)
            .await
            .map_err(external)?;
        Ok::<(), SetupError>(())
    }
    .await;
    if let Err(error) = publish {
        let lead = journal
            .lead
            .as_mut()
            .ok_or_else(|| invalid("The saved project lead reservation disappeared."))?;
        lead.status = ProjectTeamLeadStatus::Unknown;
        lead.message = Some(format!(
            "The saved lead request may have reached the provider; retry only replays it: {}",
            error.message
        ));
        save_journal(&draft, &journal)?;
        verify_context(&state, &scope)?;
        return Ok(observed_activation(&state, &draft, &journal, &keys).await);
    }
    let lead = journal
        .lead
        .as_mut()
        .ok_or_else(|| invalid("The saved project lead reservation disappeared."))?;
    lead.status = ProjectTeamLeadStatus::Starting;
    lead.message = Some("The project lead request was submitted. Reopen this setup to check its recorded provider outcome.".to_string());
    save_journal(&draft, &journal)?;
    verify_context(&state, &scope)?;
    Ok(observed_activation(&state, &draft, &journal, &keys).await)
}
