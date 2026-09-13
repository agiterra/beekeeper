use super::*;

fn lead_pubkey(journal: &PublicationJournal) -> Result<String, SetupError> {
    journal
        .installation
        .as_ref()
        .and_then(|installation| installation.roles.iter().find(|role| role.role == "lead"))
        .map(|role| role.agent_pubkey.clone())
        .ok_or_else(|| invalid("The installed project roles contain no lead identity."))
}

pub(super) fn channel_event(
    draft: &ProjectTeamSetupDraft,
    id: uuid::Uuid,
    keys: &nostr::Keys,
) -> Result<Event, SetupError> {
    let slug = draft
        .project_ref
        .rsplit(':')
        .next()
        .filter(|slug| !slug.is_empty())
        .ok_or_else(|| invalid("The bound project coordinate has no slug."))?;
    crate::events::build_create_channel(
        id,
        &format!("{slug} sessions"),
        "private",
        "transport",
        Some(&format!("Coding sessions for project {slug}.")),
        None,
        Some(&draft.project_ref),
    )
    .map_err(invalid)?
    .sign_with_keys(keys)
    .map_err(|error| invalid(error.to_string()))
}

fn validate_reserved(
    reserved: &ActivationChannelJournal,
    draft: &ProjectTeamSetupDraft,
    owner: &nostr::Keys,
) -> Result<(), SetupError> {
    if uuid::Uuid::parse_str(&reserved.channel_id).is_err()
        || reserved.create_event.kind.as_u16() != 9007
        || reserved.create_event.pubkey != owner.public_key()
        || reserved.create_event.verify().is_err()
        || !reserved
            .create_event
            .tags
            .iter()
            .any(|tag| tag.as_slice() == ["h", reserved.channel_id.as_str()])
        || !reserved
            .create_event
            .tags
            .iter()
            .any(|tag| tag.as_slice() == ["project", draft.project_ref.as_str()])
    {
        return Err(invalid(
            "The saved project session-channel request is not bound to this project and owner.",
        ));
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn project_team_setup_ensure_lead_channel(
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
        .ok_or_else(|| invalid("Start publication before creating a project session channel."))?;
    if journal.publication_id != publication_id {
        return Err(invalid(
            "The publication ID does not match this setup's journal.",
        ));
    }
    let keys = state.signing_keys().map_err(external)?;
    if keys.public_key().to_hex() != draft.owner_pubkey {
        return Err(invalid(
            "The signing identity changed before project channel creation.",
        ));
    }
    if journal
        .installation
        .as_ref()
        .is_none_or(|installation| installation.roles.is_empty())
    {
        return Err(invalid(
            "Install the adopted project roles before creating its session channel.",
        ));
    }
    let source = adopted_source(&journal)?;
    require_current_adopted_source(&state, &journal, &source).await?;
    verify_context(&state, &scope)?;
    if journal.lead.is_some() {
        return Ok(observed_activation(&state, &draft, &journal, &keys).await);
    }
    if journal
        .installation
        .as_ref()
        .and_then(|installation| installation.channel.as_ref())
        .is_none()
    {
        let id = uuid::Uuid::new_v4();
        journal
            .installation
            .as_mut()
            .ok_or_else(|| invalid("The project installation reservation disappeared."))?
            .channel = Some(ActivationChannelJournal {
            channel_id: id.to_string(),
            create_event: channel_event(&draft, id, &keys)?,
        });
        verify_context(&state, &scope)?;
        save_journal(&draft, &journal)?;
    }
    let reserved = journal
        .installation
        .as_ref()
        .and_then(|installation| installation.channel.as_ref())
        .cloned()
        .ok_or_else(|| invalid("The project channel reservation disappeared."))?;
    validate_reserved(&reserved, &draft, &keys)?;
    if crate::managed_agents::project_team_setup::launch::wire::verify_project_channel(
        &state,
        &draft,
        &reserved.channel_id,
        &keys,
    )
    .await
    .is_err()
    {
        require_current_adopted_source(&state, &journal, &source).await?;
        verify_context(&state, &scope)?;
        if let Err(error) = crate::relay::submit_signed_event_at_with_keys(
            &reserved.create_event,
            &state,
            &crate::relay::relay_http_base_url(&draft.relay_url),
            &keys,
        )
        .await
        {
            return unknown(&mut journal, &draft, &scope, &state, &keys, error).await;
        }
        state.mark_pending_owned_channel(&draft.owner_pubkey, &reserved.channel_id);
    }
    match crate::managed_agents::project_team_setup::launch::wire::verify_project_channel(
        &state,
        &draft,
        &reserved.channel_id,
        &keys,
    )
    .await
    {
        Ok(()) => {
            journal.lead = Some(LeadJournal {
                channel_id: reserved.channel_id,
                lead_pubkey: lead_pubkey(&journal)?,
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
                    "Project session channel is saved. Start the project lead when ready."
                        .to_string(),
                ),
            });
            verify_context(&state, &scope)?;
            save_journal(&draft, &journal)?;
        }
        Err(error) => {
            return unknown(&mut journal, &draft, &scope, &state, &keys, error.message).await
        }
    }
    verify_context(&state, &scope)?;
    Ok(observed_activation(&state, &draft, &journal, &keys).await)
}

async fn unknown(
    journal: &mut PublicationJournal,
    draft: &ProjectTeamSetupDraft,
    scope: &SetupScope,
    state: &AppState,
    keys: &nostr::Keys,
    error: String,
) -> Result<ProjectTeamActivation, SetupError> {
    verify_context(state, scope)?;
    save_journal(draft, journal)?;
    let mut activation = observed_activation(state, draft, journal, keys).await;
    activation.lead.status = ProjectTeamLeadStatus::Unknown;
    activation.lead.message = Some(format!("The exact project session-channel request may have reached the relay; retry only replays it: {error}"));
    Ok(activation)
}
