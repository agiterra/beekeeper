use super::*;

/// One of this project's agents on this computer, as the lead's first turn
/// names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LocalProjectAgent {
    pub(super) role: String,
    pub(super) name: String,
    pub(super) pubkey: String,
}

/// This project's agents on this computer: each installed role whose local
/// record is associated with `project_ref` (with the journal's role), then
/// every other local record associated with it (with its primary role).
/// A role whose agent is missing here, or belongs to no or another project,
/// is left out: a hire could not seat it. Ordered by role, name, pubkey.
pub(super) fn local_project_agents(
    project_ref: &str,
    installed_roles: &[ProjectTeamInstalledRole],
    agents: &[ManagedAgentRecord],
) -> Vec<LocalProjectAgent> {
    let Some(project) = association::normalize_project_ref(project_ref) else {
        return Vec::new();
    };
    let associated = |record: &&ManagedAgentRecord| {
        record
            .project_ref
            .as_deref()
            .and_then(association::normalize_project_ref)
            .as_deref()
            == Some(project.as_str())
    };
    let entry = |role: &str, record: &ManagedAgentRecord| LocalProjectAgent {
        role: role.trim().to_string(),
        name: record.name.split_whitespace().collect::<Vec<_>>().join(" "),
        pubkey: record.pubkey.clone(),
    };
    let mut listed: Vec<LocalProjectAgent> = Vec::new();
    let installed = installed_roles.iter().filter_map(|installed| {
        agents
            .iter()
            .filter(associated)
            .find(|record| record.pubkey == installed.agent_pubkey)
            .map(|record| entry(&installed.role, record))
    });
    let others = agents.iter().filter(associated).filter_map(|record| {
        let role = record
            .home_role
            .as_deref()
            .filter(|role| !role.trim().is_empty())?;
        Some(entry(role, record))
    });
    for agent in installed.chain(others) {
        if listed.iter().all(|known| known.pubkey != agent.pubkey) {
            listed.push(agent);
        }
    }
    listed.sort_by(|a, b| (&a.role, &a.name, &a.pubkey).cmp(&(&b.role, &b.name, &b.pubkey)));
    listed
}

/// The first turn a newly created project lead receives, naming `agents`
/// (this project's agents on this computer, which hosts the session and
/// answers its hires).
///
/// Rendered only while a lead has no saved create event: once the signed
/// create event is journaled, every retry replays those stored bytes, so a
/// change to this text or roster never alters a reserved lead request.
pub(super) fn lead_initial_turn(
    source: &ProjectTeamActivationSource,
    agents: &[LocalProjectAgent],
) -> String {
    let roster = if agents.is_empty() {
        "- none".to_string()
    } else {
        agents
            .iter()
            .map(|agent| {
                let short: String = agent.pubkey.chars().take(8).collect();
                format!("- {}: {} ({short})", agent.role, agent.name)
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        "You are the project lead. Read this project's repository instructions before acting. Your role pack is pinned to {} at {}. Establish the work from the project evidence, preserve Solo as independent, and use the project team only when it helps. Before depending on existing project instructions, reconcile any that name specific agents, reviewers, budgets or staffing arrangements from an earlier setup: keep the underlying product, security, testing and independent-review requirements, map them to available roles, and ask the owner once when the mapping is unclear. Inspect tools, configuration and repository state yourself before asking a person. To staff a task, list this project's agents with `bee projects agents` (your seat's project is the default) and hire the role the task needs with `bee sessions hire`. The computer that answers a hire seats only agents that belong to this project; another project's agent is never borrowed, so when no project agent holds a role, say so instead of working around it.\n\nThis project's agents on the computer hosting this session (role: name, short pubkey):\n{roster}\n\n`bee projects agents` lists agents published for public projects; this project's agents on the hosting computer are listed above.",
        source.repo_ref, source.commit
    )
}

/// The reserved lead identity: the lead journal's when one exists, otherwise
/// the installed `lead` role's, otherwise unknown.
fn lead_pubkey(journal: &PublicationJournal) -> Option<String> {
    journal
        .lead
        .as_ref()
        .map(|lead| lead.lead_pubkey.clone())
        .or_else(|| {
            journal
                .installation
                .as_ref()
                .and_then(|installation| installation.roles.iter().find(|role| role.role == "lead"))
                .map(|role| role.agent_pubkey.clone())
        })
}

pub(crate) fn activation(journal: &PublicationJournal) -> ProjectTeamActivation {
    let source = adopted_source(journal).ok();
    let installation = match &journal.installation {
        Some(installed) if !installed.roles.is_empty() => ProjectTeamInstallation {
            status: ProjectTeamInstallationStatus::Installed,
            installed_roles: installed.roles.clone(),
            message: None,
        },
        Some(_) => ProjectTeamInstallation {
            status: ProjectTeamInstallationStatus::Unknown,
            installed_roles: Vec::new(),
            message: Some(
                "Project-role installation was interrupted; retry it with the saved team identity."
                    .to_string(),
            ),
        },
        None => ProjectTeamInstallation {
            status: ProjectTeamInstallationStatus::NotInstalled,
            installed_roles: Vec::new(),
            message: source.is_none().then(|| {
                "The project source must be adopted before this computer can install roles."
                    .to_string()
            }),
        },
    };
    let lead_pubkey = lead_pubkey(journal);
    let lead = if installation.status == ProjectTeamInstallationStatus::Installed {
        journal.lead.as_ref().map_or_else(|| {
            let reservation = journal.installation.as_ref().and_then(|installation| installation.channel.as_ref());
            ProjectTeamLead { status: reservation.map_or(ProjectTeamLeadStatus::NeedsChannel, |_| ProjectTeamLeadStatus::Unknown), channel_id: reservation.map(|channel| channel.channel_id.clone()), session_ref: None, lead_pubkey: lead_pubkey.clone(), message: Some(reservation.map_or_else(|| "Create this project's session channel before starting its lead.".to_string(), |_| "The exact project session-channel request was saved; its relay outcome is being reconciled.".to_string())) }
        }, |lead| ProjectTeamLead { status: lead.status.clone(), channel_id: Some(lead.channel_id.clone()), session_ref: lead.session_ref.clone(), lead_pubkey: lead_pubkey.clone(), message: lead.message.clone() })
    } else {
        ProjectTeamLead {
            status: ProjectTeamLeadStatus::Refused,
            channel_id: None,
            session_ref: None,
            lead_pubkey,
            message: Some("Install the adopted project roles before starting a lead.".to_string()),
        }
    };
    ProjectTeamActivation {
        source,
        installation,
        lead,
    }
}

/// Read a provider receipt without changing the saved request. Reopen can
/// report an observed start while a retry still reuses the exact signed bytes.
pub(super) async fn observed_activation(
    state: &AppState,
    draft: &ProjectTeamSetupDraft,
    journal: &PublicationJournal,
    keys: &nostr::Keys,
) -> ProjectTeamActivation {
    let mut result = activation(journal);
    let Some(lead) = journal.lead.as_ref() else {
        return result;
    };
    let (Some(command_id), Some(provider_pubkey), Some(instance_id), Some(driver)) = (
        lead.create_command_id.as_deref(),
        lead.provider_pubkey.as_deref(),
        lead.provider_host_instance_id.as_deref(),
        lead.driver.as_deref(),
    ) else {
        return result;
    };
    let events = crate::relay::query_relay_at_with_keys(
        state,
        &crate::relay::relay_http_base_url(&draft.relay_url),
        &[json!({
            "kinds": [KIND_CODING_SESSION_LIFECYCLE_RECEIPT],
            "authors": [provider_pubkey],
            "#h": [lead.channel_id],
            "#csl-command": [command_id],
            "limit": 32,
        })],
        keys,
        None,
    )
    .await;
    let events = match events {
        Ok(events) if events.len() < 32 => events,
        Ok(_) => {
            result.lead.status = ProjectTeamLeadStatus::Unknown;
            result.lead.message = Some(
                "The lead receipt query reached its safety bound; its outcome remains unknown."
                    .to_string(),
            );
            return result;
        }
        Err(error) => {
            result.lead.status = ProjectTeamLeadStatus::Unknown;
            result.lead.message = Some(format!("Lead receipt reconciliation failed: {error}"));
            return result;
        }
    };
    let Ok(channel) = uuid::Uuid::parse_str(&lead.channel_id) else {
        return result;
    };
    let mut receipt = None;
    for event in events {
        if event.kind.as_u16() != KIND_CODING_SESSION_LIFECYCLE_RECEIPT as u16
            || event.pubkey.to_hex() != provider_pubkey
            || event.verify().is_err()
        {
            continue;
        }
        let Ok(decoded) = decode_coding_session_lifecycle_receipt(&event.content) else {
            continue;
        };
        if decoded.command_id != command_id {
            continue;
        }
        let Ok(expected) = beekeeper_sdk_pkg::builders::build_coding_session_lifecycle_receipt(
            channel,
            command_id,
            &event.content,
        ) else {
            continue;
        };
        let Ok(expected_pubkey) = nostr::PublicKey::from_hex(provider_pubkey) else {
            continue;
        };
        if event.tags != expected.build(expected_pubkey).tags
            || decoded
                .session
                .as_ref()
                .is_some_and(|target| target.instance_id != instance_id || target.driver != driver)
        {
            continue;
        }
        if receipt.replace(decoded).is_some() {
            result.lead.status = ProjectTeamLeadStatus::Unknown;
            result.lead.message = Some(
                "The provider returned contradictory outcomes for this lead request.".to_string(),
            );
            return result;
        }
    }
    if let Some(receipt) = receipt {
        match receipt.status {
            ReceiptStatus::Created => {
                result.lead.status = ProjectTeamLeadStatus::Started;
                result.lead.message = receipt
                    .error
                    .map(|error| format!("{}: {}", error.code, error.message));
            }
            ReceiptStatus::CreatedWithFailedInitialTurn | ReceiptStatus::Failed => {
                result.lead.status = ProjectTeamLeadStatus::Unknown;
                result.lead.message = receipt
                    .error
                    .map(|error| format!("{}: {}", error.code, error.message))
                    .or_else(|| {
                        Some(
                            "The exact lead request completed without a usable initial turn."
                                .to_string(),
                        )
                    });
            }
            _ => {}
        }
    }
    result
}
