use crate::managed_agents::ManagedAgentReadinessMetadata;
use crate::session_provider::store::CodingSessionProviderReadinessRecord;

use super::{Gathered, TeamReadinessFact, TeamReadinessFactState};

pub(super) fn append_agent_auth_facts(
    metadata: &[ManagedAgentReadinessMetadata],
    gathered: &mut Gathered,
) {
    for row in metadata {
        let selected = row
            .home_role
            .as_ref()
            .is_some_and(|role| gathered.team.selected_roles.contains(role));
        let (code, summary) = if row.auth_tag_invalid {
            (
                "AGENT_AUTH_TAG_INVALID",
                format!("Agent {} has an invalid owner attestation", row.name),
            )
        } else if row.auth_tag_owner_mismatch {
            (
                "AGENT_AUTH_OWNER_MISMATCH",
                format!("Agent {} is attested by another owner", row.name),
            )
        } else {
            continue;
        };
        let mut fact = TeamReadinessFact::local(
            "identity",
            code,
            if selected {
                TeamReadinessFactState::Unknown
            } else {
                TeamReadinessFactState::Limited
            },
            summary,
        );
        fact.remedy = Some("Use Prepare to repair this identity's attestation.".into());
        gathered.facts.push(fact);
    }
}

pub(super) fn append_provider_auth_fact(
    record: &CodingSessionProviderReadinessRecord,
    gathered: &mut Gathered,
) {
    if record.auth_tag_invalid {
        gathered.facts.push(TeamReadinessFact::unknown(
            "provider",
            "PROVIDER_AUTH_TAG_INVALID",
            "The provider owner attestation is invalid",
            "Use Prepare to provision the provider again.",
        ));
    } else if record.auth_tag_owner_mismatch {
        gathered.facts.push(TeamReadinessFact::unknown(
            "provider",
            "PROVIDER_AUTH_OWNER_MISMATCH",
            "The provider is attested by another owner",
            "Use Prepare to provision the provider again.",
        ));
    } else if !record.auth_tag_present {
        gathered.facts.push(TeamReadinessFact::blocked(
            "provider",
            "PROVIDER_AUTH_TAG_MISSING",
            "The provider metadata has no owner authorization tag",
            "Use Prepare to provision the provider again.",
        ));
    }
}
