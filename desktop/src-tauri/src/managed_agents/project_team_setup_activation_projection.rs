use super::*;

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
    let lead = if installation.status == ProjectTeamInstallationStatus::Installed {
        journal.lead.as_ref().map_or_else(|| {
            let reservation = journal.installation.as_ref().and_then(|installation| installation.channel.as_ref());
            ProjectTeamLead { status: reservation.map_or(ProjectTeamLeadStatus::NeedsChannel, |_| ProjectTeamLeadStatus::Unknown), channel_id: reservation.map(|channel| channel.channel_id.clone()), session_ref: None, message: Some(reservation.map_or_else(|| "Create this project's session channel before starting its lead.".to_string(), |_| "The exact project session-channel request was saved; its relay outcome is being reconciled.".to_string())) }
        }, |lead| ProjectTeamLead { status: lead.status.clone(), channel_id: Some(lead.channel_id.clone()), session_ref: lead.session_ref.clone(), message: lead.message.clone() })
    } else {
        ProjectTeamLead {
            status: ProjectTeamLeadStatus::Refused,
            channel_id: None,
            session_ref: None,
            message: Some("Install the adopted project roles before starting a lead.".to_string()),
        }
    };
    ProjectTeamActivation {
        source,
        installation,
        lead,
    }
}
