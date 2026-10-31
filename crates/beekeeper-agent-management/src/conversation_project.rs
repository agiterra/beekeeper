//! Connect owner-approved project coordinates to the existing Host conversation.
use crate::{hire::Scope, management::local, Context, Error, RelayTransport};
use beekeeper_session_provider::conversation_project::ProjectContext;

/// Bind a canonical installed lead to one existing project umbrella. Repeating
/// the same binding is idempotent; replacing it requires deliberate migration.
/// This installs no responder and grants no authority.
pub async fn configure(
    context: &Context,
    client: &impl RelayTransport,
    scope: &Scope,
    lead: &str,
    selection: Option<beekeeper_session_provider::conversation_project::HireSelection>,
) -> Result<(), Error> {
    configure_with_writer(context, client, scope, lead, selection, |path, value| {
        local::save(path, value)
    })
    .await
}

pub(crate) async fn configure_with_writer(
    context: &Context,
    client: &impl RelayTransport,
    scope: &Scope,
    lead: &str,
    selection: Option<beekeeper_session_provider::conversation_project::HireSelection>,
    write: impl FnOnce(&std::path::Path, &ProjectContext) -> Result<(), Error>,
) -> Result<(), Error> {
    nostr::PublicKey::from_hex(lead).map_err(local::fail)?;
    let owner = local::owner(client)?;
    let host = crate::hire_source::host(context, client)?;
    let configured = ProjectContext {
        project: scope.project.clone(),
        channel: scope.channel,
        actor: lead.into(),
        genesis: scope.genesis.clone(),
        hire_selection: selection,
    };
    configured
        .instructions(&owner, lead, scope.channel, Some(&scope.project))
        .map_err(local::fail)?;
    let _lock = local::management_lock(context)?;
    let path = host
        .provider_state_dir
        .join("conversation-projects")
        .join(format!("{lead}.json"));
    if let Some(value) = local::read_operation_value(&path)? {
        let mut previous: ProjectContext = serde_json::from_value(value).map_err(local::fail)?;
        previous.hire_selection = configured.hire_selection.clone();
        if previous != configured {
            return Err(Error::Conflict(
                "lead already has different project conversation scope; original retained".into(),
            ));
        }
    } else {
        let snapshot = crate::private_configuration_snapshot(context, client, lead)?;
        let definition: beekeeper_core::managed_agent::ManagedAgentEventContent =
            serde_json::from_str(&snapshot.definition.content).map_err(local::fail)?;
        if definition.home_role.as_deref()
            != Some(beekeeper_core::role_vocabulary::RoleVocabulary::foundation().lead())
            || snapshot.private.record["role_material"]["project"] != scope.project
        {
            return Err(Error::Auth(
                "conversation actor is not this project's installed lead".into(),
            ));
        }
        beekeeper_host_core::atomic_write::create_dir_all_restricted(
            &host.provider_state_dir.join("conversation-projects"),
        )
        .map_err(local::fail)?;
    }
    write(&path, &configured)?;
    Ok(())
}
