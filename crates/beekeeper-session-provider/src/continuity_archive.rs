//! Private continuity output boundary retained for as long as native history can resume.
use beekeeper_core::coding_session_command::CodingSessionTarget;
use serde::{Deserialize, Serialize};

/// Exact binding of a native conversation's private output boundary.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Binding {
    pub target: CodingSessionTarget,
    pub actor: String,
    pub owner: String,
    pub community: String,
    pub cursor_hash: String,
}
/// Never derive Debug: fragments contain private model input.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Archive {
    pub binding: Binding,
    pub fragments: Vec<String>,
}
impl Binding {
    pub fn for_record(
        record: &crate::state::SessionRecord,
        instance: &str,
        community: &str,
        owner: &str,
    ) -> Result<Self, crate::continuity_bootstrap::Refusal> {
        use crate::continuity_bootstrap::Refusal;
        Ok(Self {
            target: record.target(instance),
            actor: record.actor.clone().ok_or(Refusal::Unavailable)?,
            owner: owner.into(),
            community: beekeeper_core::continuity::canonical_origin(community)
                .map_err(|_| Refusal::Unavailable)?,
            cursor_hash: beekeeper_core::continuity::value_hash(
                record
                    .resume_cursor
                    .as_deref()
                    .ok_or(Refusal::Unavailable)?
                    .as_bytes(),
            ),
        })
    }
}

pub(crate) fn restore(
    state_dir: &std::path::Path,
    record: &crate::state::SessionRecord,
    instance: &str,
    community: &str,
    identity_env: &[(String, String)],
) -> Result<Option<Archive>, crate::session::CreateFailure> {
    let refusal = || {
        crate::execution_scope::refuse("CONTINUITY_PRIVATE_DELIVERY_UNAVAILABLE","the native conversation's private output boundary is unavailable; no native restore was opened")
    };
    if uuid::Uuid::parse_str(&record.session_id).is_err() {
        return Ok(None);
    }
    let Some(archive) =
        crate::context_store::read_continuity_archive(state_dir, &record.session_id)
            .map_err(|_| refusal())?
    else {
        return Ok(None);
    };
    let actor = nostr::PublicKey::from_hex(record.actor.as_deref().ok_or_else(refusal)?)
        .map_err(|_| refusal())?;
    let tag = identity_env
        .iter()
        .find(|(k, _)| k == "BEEKEEPER_AUTH_TAG")
        .map(|(_, v)| v)
        .ok_or_else(refusal)?;
    let owner = beekeeper_sdk::nip_oa::verify_auth_tag(tag, &actor).map_err(|_| refusal())?;
    let expected =
        Binding::for_record(record, instance, community, &owner.to_hex()).map_err(|_| refusal())?;
    if archive.binding != expected {
        return Err(refusal());
    }
    Ok(Some(archive))
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Marker {
    pub binding: Binding,
    pub digest: String,
}

/// Context that session/new can receive before the first dependent turn.
pub(crate) fn startup_fragments(
    request: &crate::session::CreateRequest,
) -> Result<Vec<String>, crate::session::CreateFailure> {
    let mut fragments = Vec::new();
    if let Some(skills) = &request.seat_skills {
        let persona = beekeeper_persona::resolve::resolve_persona_by_name(
            &skills.pack_dir,
            &skills.persona_id,
        )
        .map_err(|_| {
            crate::execution_scope::refuse(
                "CONTINUITY_BOOTSTRAP_UNAVAILABLE",
                "required staged role unavailable",
            )
        })?;
        if !persona.system_prompt.is_empty() {
            fragments.push(persona.system_prompt);
        }
    }
    if let Some(context) = &request.rehydration_mcp {
        fragments.push(context.first_turn_brief.clone());
    }
    Ok(fragments)
}
/// Mark pending reconstruction before private system context is handed to
/// session/new. A crash before the returned cursor is bound makes restore refuse.
pub(crate) fn stage_reconstruction(
    state_dir: &std::path::Path,
    request: &crate::session::CreateRequest,
    mut archive: Option<Archive>,
) -> Result<Option<Archive>, crate::session::CreateFailure> {
    let fragments = startup_fragments(request)?;
    if fragments.is_empty() {
        return Ok(archive);
    }
    let refusal = || {
        crate::execution_scope::refuse(
            "CONTINUITY_PRIVATE_DELIVERY_UNAVAILABLE",
            "private reconstruction output boundary unavailable",
        )
    };
    let custody =
        crate::continuity_collect::Custody::from_launch(request).map_err(|_| refusal())?;
    let pending = Binding {
        target: request.target.clone(),
        actor: custody.rest.keys.public_key().to_hex(),
        owner: custody.owner.to_hex(),
        community: custody.rest.base_url.clone(),
        cursor_hash: "pending_explicit_reconstruction".into(),
    };
    let archive = archive.get_or_insert(Archive {
        binding: pending.clone(),
        fragments: vec![],
    });
    archive.binding = pending;
    for fragment in fragments {
        if !archive.fragments.contains(&fragment) {
            archive.fragments.push(fragment);
        }
    }
    crate::context_store::write_continuity_archive(state_dir, archive).map_err(|_| refusal())?;
    Ok(Some(archive.clone()))
}
