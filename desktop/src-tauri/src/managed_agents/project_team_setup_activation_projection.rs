use super::*;

/// The first turn a newly created project lead receives.
///
/// Rendered only while a lead has no saved create event: once the signed
/// create event is journaled, every retry replays those stored bytes, so a
/// change to this text never alters a reserved lead request.
pub(super) fn lead_initial_turn(source: &ProjectTeamActivationSource) -> String {
    format!(
        "You are the project lead. Read this project's repository instructions before acting. Your role pack is pinned to {} at {}. Establish the work from the project evidence, preserve Solo as independent, and use the project team only when it helps. Before depending on existing project instructions, reconcile any that name specific agents, reviewers, budgets or staffing arrangements from an earlier setup: keep the underlying product, security, testing and independent-review requirements, map them to available roles, and ask the owner once when the mapping is unclear. Inspect tools, configuration and repository state yourself before asking a person.",
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
