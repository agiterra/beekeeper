use super::*;
use beekeeper_core::coding_session_genesis::{
    decode_coding_session_genesis, CODING_SESSION_GENESIS_TAG_VERSION,
};
use beekeeper_core::coding_session_lifecycle_command::{
    CodingSessionLifecycleCommandPayload, CODING_SESSION_HIRE_BRIEF_PREFIX,
    CODING_SESSION_LIFECYCLE_COMMAND_TAG_VERSION,
};
use beekeeper_core::kind::{KIND_CODING_SESSION_GENESIS, KIND_CODING_SESSION_LIFECYCLE_COMMAND};

fn envelope(event: &Event, kind: u16, tags: &[(&str, &str)]) -> Result<(), Error> {
    if event.kind.as_u16() != kind
        || event.verify().is_err()
        || event.tags.len() != tags.len()
        || event
            .tags
            .iter()
            .zip(tags)
            .any(|(a, (k, v))| a.as_slice() != [*k, *v])
    {
        return Err(Error::Auth(
            "hire signed envelope does not match scope".into(),
        ));
    }
    Ok(())
}

pub(super) fn validate_scope(
    scope: &Scope,
    request: &Event,
    owner: &str,
) -> Result<CodingSessionLifecycleCommandPayload, Error> {
    if beekeeper_core::kind::normalize_project_coordinate(&scope.project).as_deref()
        != Some(scope.project.as_str())
    {
        return Err(Error::Usage(
            "hire scope must name a normalized project".into(),
        ));
    }
    if scope.genesis.pubkey.to_hex() != owner {
        return Err(Error::Auth(
            "hire responder is not the umbrella founder".into(),
        ));
    }
    let genesis = decode_coding_session_genesis(&scope.genesis.content).map_err(fail)?;
    let channel = scope.channel.to_string();
    envelope(
        &scope.genesis,
        KIND_CODING_SESSION_GENESIS as u16,
        &[
            ("h", &channel),
            ("csg-v", CODING_SESSION_GENESIS_TAG_VERSION),
            ("csg-session", &genesis.session_ref),
        ],
    )?;
    let payload = decode_coding_session_lifecycle_command(&request.content).map_err(fail)?;
    envelope(
        request,
        KIND_CODING_SESSION_LIFECYCLE_COMMAND as u16,
        &[
            ("h", &channel),
            ("csl-v", CODING_SESSION_LIFECYCLE_COMMAND_TAG_VERSION),
            ("csl-command", &payload.command_id),
        ],
    )?;
    match &payload.action {
        CodingSessionLifecycleAction::SessionHire {
            session_ref,
            genesis_ref,
            ..
        } if session_ref == &genesis.session_ref && genesis_ref == &scope.genesis.id.to_hex() => {
            Ok(payload)
        }
        _ => Err(Error::Auth(
            "request is not a hire into the selected genesis".into(),
        )),
    }
}

pub(super) fn validate_plan(
    scope: &Scope,
    request: &Event,
    plan: &SeatPlan,
    owner: &str,
) -> Result<String, Error> {
    let hire = validate_scope(scope, request, owner)?;
    let CodingSessionLifecycleAction::SessionHire {
        session_ref,
        genesis_ref,
        role,
        brief,
        ..
    } = hire.action
    else {
        return Err(Error::Usage("request is not a hire".into()));
    };
    let create = decode_coding_session_lifecycle_command(&plan.create.content).map_err(fail)?;
    envelope(
        &plan.create,
        KIND_CODING_SESSION_LIFECYCLE_COMMAND as u16,
        &[
            ("h", &scope.channel.to_string()),
            ("csl-v", CODING_SESSION_LIFECYCLE_COMMAND_TAG_VERSION),
            ("csl-command", &create.command_id),
        ],
    )?;
    let brief = brief.trim();
    let initial = if brief.starts_with(CODING_SESSION_HIRE_BRIEF_PREFIX) {
        brief.to_string()
    } else {
        format!("{CODING_SESSION_HIRE_BRIEF_PREFIX}{brief}")
    };
    let CodingSessionLifecycleAction::SessionCreate {
        project_ref: Some(project),
        session_ref: Some(session),
        genesis_ref: Some(genesis),
        actor: Some(actor),
        role: Some(seat_role),
        hire_ref: Some(hire_ref),
        initial_turn: Some(turn),
        ..
    } = create.action
    else {
        return Err(Error::Conflict(
            "hire plan is not a complete seated create".into(),
        ));
    };
    if plan.create.pubkey.to_hex() != owner
        || actor == owner
        || project != scope.project
        || session != session_ref
        || genesis != genesis_ref
        || seat_role != role
        || hire_ref != request.id.to_hex()
        || turn != initial
        || plan.pack_ref.role != role
        || create.command_id == hire.command_id
        || !plan.checkout.is_absolute()
        || !plan.worktree.is_absolute()
        || !plan.pack_dir.is_absolute()
        || plan.checkout == plan.worktree
        || plan.pack_dir.starts_with(&plan.worktree)
        || plan
            .agents_checkout
            .as_ref()
            .is_some_and(|p| !p.is_absolute() || p.starts_with(&plan.worktree))
    {
        return Err(Error::Conflict(
            "hire plan changed the request, identity, role or isolated material".into(),
        ));
    }
    for sha in [&plan.checkout_revision, &plan.pack_ref.sha] {
        if sha.len() != 40
            || !sha
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(Error::Usage(
                "hire code and role sources must name immutable commits".into(),
            ));
        }
    }
    for id in [
        &plan.definition_revision,
        &plan.private_snapshot_hash,
        &plan.source_event_id,
    ] {
        if id.len() != 64
            || !id
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(Error::Usage(
                "hire provenance must contain lowercase 64-hex IDs".into(),
            ));
        }
    }
    Ok(actor)
}

pub(super) fn validate_grant(
    scope: &Scope,
    plan: &SeatPlan,
    event: &Event,
    owner: &str,
) -> Result<(), Error> {
    use beekeeper_core::coding_session_authority_transition::{
        decode_coding_session_authority_transition, CodingSessionAuthorityTransitionType as Kind,
        CODING_SESSION_AUTHORITY_TRANSITION_TAG_VERSION,
    };
    let grant = decode_coding_session_authority_transition(&event.content).map_err(fail)?;
    envelope(
        event,
        beekeeper_core::kind::KIND_CODING_SESSION_AUTHORITY_TRANSITION as u16,
        &[
            ("h", &scope.channel.to_string()),
            ("csat-v", CODING_SESSION_AUTHORITY_TRANSITION_TAG_VERSION),
            ("csat-genesis", &scope.genesis.id.to_hex()),
        ],
    )?;
    if event.pubkey.to_hex() != owner
        || grant.genesis_ref != scope.genesis.id.to_hex()
        || grant.grantee_pubkey != validate_plan_actor(plan)?
        || !matches!(grant.transition_type, Kind::GrantOperator | Kind::GrantSeat)
        || (grant.transition_type == Kind::GrantSeat
            && grant.role.as_deref() != Some(plan.pack_ref.role.as_str()))
    {
        return Err(Error::Auth(
            "hire grant changed the owner, actor, genesis or role".into(),
        ));
    }
    Ok(())
}
