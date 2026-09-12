//! Scoped relay reads and existing signed-event writes for setup authoring.
use super::*;
use crate::relay::{
    query_relay_at_with_keys, relay_http_base_url, submit_signed_event_at_with_keys,
};
use buzz_core_pkg::coding_session_payload::{
    decode_coding_session_lifecycle_receipt, LifecycleReceipt, ReceiptStatus,
};
use buzz_core_pkg::kind::{KIND_CODING_SESSION_LIFECYCLE_RECEIPT, KIND_PROJECT};
use serde_json::json;

fn exact_tag(event: &Event, name: &str, value: &str) -> bool {
    let tags: Vec<_> = event
        .tags
        .iter()
        .filter(|tag| tag.as_slice().first().map(String::as_str) == Some(name))
        .collect();
    tags.len() == 1 && tags[0].as_slice() == [name, value]
}
async fn query(
    state: &AppState,
    draft: &ProjectTeamSetupDraft,
    keys: &Keys,
    filter: serde_json::Value,
) -> Result<Vec<Event>, SetupError> {
    query_relay_at_with_keys(
        state,
        &relay_http_base_url(&draft.relay_url),
        &[filter],
        keys,
        None,
    )
    .await
    .map_err(external)
}
async fn relay_signer(
    state: &AppState,
    draft: &ProjectTeamSetupDraft,
) -> Result<String, SetupError> {
    let response = state
        .http_client
        .get(relay_http_base_url(&draft.relay_url))
        .header("Accept", "application/nostr+json")
        .send()
        .await
        .map_err(|e| external(e.to_string()))?;
    if !response.status().is_success() {
        return Err(external(
            "The community did not disclose its metadata signing identity.",
        ));
    }
    let value: serde_json::Value = response.json().await.map_err(|e| external(e.to_string()))?;
    let signer = value
        .get("self")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| external("The community did not disclose its metadata signing identity."))?;
    nostr::PublicKey::from_hex(signer)
        .map(|key| key.to_hex())
        .map_err(|_| invalid("The community metadata signing identity is malformed."))
}
fn latest<'a>(
    events: &'a [Event],
    kind: u16,
    signer: &str,
    id: &str,
) -> Result<&'a Event, SetupError> {
    events
        .iter()
        .filter(|event| {
            event.kind.as_u16() == kind
                && event.pubkey.to_hex() == signer
                && exact_tag(event, "d", id)
                && event.verify().is_ok()
        })
        .max_by_key(|event| (event.created_at, event.id))
        .ok_or_else(|| external("No verified current project or channel metadata was returned."))
}

pub(super) fn project_channel_proof(
    project: &Event,
    channel: &Event,
    project_ref: &str,
    channel_id: &str,
    relay_signer: &str,
) -> Result<(), SetupError> {
    let parts: Vec<_> = project_ref.splitn(3, ':').collect();
    if parts.len() != 3
        || parts[0] != "30621"
        || project.kind.as_u16() != KIND_PROJECT as u16
        || project.pubkey.to_hex() != parts[1]
        || !exact_tag(project, "d", parts[2])
        || project.verify().is_err()
        || channel.kind.as_u16() != 39000
        || channel.pubkey.to_hex() != relay_signer
        || !exact_tag(channel, "d", channel_id)
        || channel.verify().is_err()
    {
        return Err(invalid(
            "Project/channel metadata failed its signed identity binding.",
        ));
    }
    let backlinks: Vec<_> = channel
        .tags
        .iter()
        .filter(|tag| tag.as_slice().first().map(String::as_str) == Some("project"))
        .collect();
    if backlinks.len() > 1
        || backlinks
            .first()
            .is_some_and(|tag| tag.as_slice() != ["project", project_ref])
    {
        return Err(invalid(
            "The channel is bound to a different or ambiguous project.",
        ));
    }
    let forward = project
        .tags
        .iter()
        .any(|tag| tag.as_slice() == ["channel", channel_id]);
    if !forward && !exact_tag(channel, "project", project_ref) {
        return Err(invalid(
            "The selected authoring channel does not belong to this project.",
        ));
    }
    if exact_tag(channel, "archived", "true") {
        return Err(invalid("The selected authoring channel is archived."));
    }
    Ok(())
}

pub(super) async fn verify_project_channel(
    state: &AppState,
    draft: &ProjectTeamSetupDraft,
    channel_id: &str,
    keys: &Keys,
) -> Result<(), SetupError> {
    let parts: Vec<_> = draft.project_ref.splitn(3, ':').collect();
    if parts.len() != 3 {
        return Err(invalid("Invalid project coordinate."));
    }
    let signer = relay_signer(state, draft).await?;
    let project_events = query(
        state,
        draft,
        keys,
        json!({"kinds":[KIND_PROJECT],"authors":[parts[1]],"#d":[parts[2]],"limit":8}),
    )
    .await?;
    let channels = query(
        state,
        draft,
        keys,
        json!({"kinds":[39000],"authors":[signer],"#d":[channel_id],"limit":8}),
    )
    .await?;
    project_channel_proof(
        latest(&project_events, KIND_PROJECT as u16, parts[1], parts[2])?,
        latest(&channels, 39000, &signer, channel_id)?,
        &draft.project_ref,
        channel_id,
        &signer,
    )
}

async fn members(
    state: &AppState,
    draft: &ProjectTeamSetupDraft,
    channel: &str,
    signer: &str,
    keys: &Keys,
) -> Result<Vec<String>, SetupError> {
    let events = query(
        state,
        draft,
        keys,
        json!({"kinds":[39002],"authors":[signer],"#d":[channel],"limit":8}),
    )
    .await?;
    Ok(latest(&events, 39002, signer, channel)?
        .tags
        .iter()
        .filter_map(|tag| {
            let p = tag.as_slice();
            (p.first().map(String::as_str) == Some("p"))
                .then(|| p.get(1).cloned())
                .flatten()
        })
        .collect())
}
pub(super) async fn ensure_membership(
    state: &AppState,
    draft: &ProjectTeamSetupDraft,
    channel: &str,
    pubkeys: &[&str],
    keys: &Keys,
) -> Result<(), SetupError> {
    let signer = relay_signer(state, draft).await?;
    let channel_uuid = uuid::Uuid::parse_str(channel).map_err(|e| invalid(e.to_string()))?;
    let current = members(state, draft, channel, &signer, keys).await?;
    for pubkey in pubkeys {
        if current.iter().any(|member| member == pubkey) {
            continue;
        }
        let event = buzz_sdk_pkg::builders::build_add_member(
            channel_uuid,
            pubkey,
            Some(buzz_core_pkg::channel::MemberRole::Bot),
        )
        .map_err(|e| invalid(e.to_string()))?
        .sign_with_keys(keys)
        .map_err(|e| invalid(e.to_string()))?;
        // A publish error may mean an already-applied membership. Read back
        // before refusing, and never downgrade an existing member's role.
        let publish = submit_signed_event_at_with_keys(
            &event,
            state,
            &relay_http_base_url(&draft.relay_url),
            keys,
        )
        .await;
        let mut found = false;
        for attempt in 0..3 {
            if attempt > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
            if members(state, draft, channel, &signer, keys)
                .await?
                .iter()
                .any(|member| member == pubkey)
            {
                found = true;
                break;
            }
        }
        if !found {
            return Err(external(format!(
                "The setup actor or provider is not an observed channel member. {}",
                publish.err().unwrap_or_else(|| {
                    "The relay accepted the request without applying membership.".into()
                })
            )));
        }
    }
    Ok(())
}

pub(super) fn verified_receipt(
    event: &Event,
    saved: &journal::LaunchJournal,
) -> Option<LifecycleReceipt> {
    if event.kind.as_u16() != KIND_CODING_SESSION_LIFECYCLE_RECEIPT as u16
        || event.pubkey.to_hex() != saved.choice.provider_pubkey
        || event.verify().is_err()
    {
        return None;
    }
    let receipt = decode_coding_session_lifecycle_receipt(&event.content).ok()?;
    if receipt.command_id != saved.reservation.create_command_id
        || !matches!(
            receipt.status,
            ReceiptStatus::Created
                | ReceiptStatus::CreatedWithFailedInitialTurn
                | ReceiptStatus::Failed
        )
    {
        return None;
    }
    let channel = uuid::Uuid::parse_str(&saved.reservation.channel_id).ok()?;
    let expected = buzz_sdk_pkg::builders::build_coding_session_lifecycle_receipt(
        channel,
        &receipt.command_id,
        &event.content,
    )
    .ok()?
    .build(event.pubkey);
    if event.tags != expected.tags {
        return None;
    }
    if receipt.session.as_ref().is_some_and(|target| {
        target.driver != saved.driver || target.instance_id != saved.instance_id
    }) {
        return None;
    }
    Some(receipt)
}

pub(super) fn fold_receipts(
    saved: &journal::LaunchJournal,
    events: &[Event],
) -> Result<Option<(Event, LifecycleReceipt)>, SetupError> {
    let mut found: Option<(Event, LifecycleReceipt)> = None;
    for event in events {
        let Some(receipt) = verified_receipt(event, saved) else {
            continue;
        };
        if found
            .as_ref()
            .is_some_and(|(_, previous)| previous != &receipt)
        {
            return Err(invalid(
                "The selected provider returned contradictory outcomes for this exact create.",
            ));
        }
        if found.is_none() {
            found = Some((event.clone(), receipt));
        }
    }
    Ok(found)
}

pub(super) async fn observe(
    state: &AppState,
    draft: &ProjectTeamSetupDraft,
    saved: &journal::LaunchJournal,
    keys: &Keys,
) -> SetupAuthoringLaunch {
    let mut response = saved.response();
    let result=async {
        let events=query(state,draft,keys,json!({"kinds":[KIND_CODING_SESSION_LIFECYCLE_RECEIPT],"authors":[saved.choice.provider_pubkey],"#h":[saved.reservation.channel_id],"#csl-command":[saved.reservation.create_command_id],"limit":32})).await?;
        if events.len()>=32{return Err(external("The receipt query reached its safety bound; the outcome remains unknown."));}
        fold_receipts(saved,&events)
    }.await;
    match result {
        Ok(Some((event, receipt))) => {
            response.status = match receipt.status {
                ReceiptStatus::Created => LaunchStatus::Created,
                ReceiptStatus::CreatedWithFailedInitialTurn => LaunchStatus::InitialTurnFailed,
                _ => LaunchStatus::Failed,
            };
            response.message = receipt
                .error
                .map(|error| format!("{}: {}", error.code, error.message));
            response.target = receipt.session;
            response.receipt_event_id = Some(event.id.to_hex());
        }
        Ok(None) => {}
        Err(error) => {
            response.status = LaunchStatus::Ambiguous;
            response.message = Some(format!("Receipt reconciliation failed: {}", error.message));
        }
    }
    response
}

async fn submit_exact(
    event: &Event,
    state: &AppState,
    base: &str,
    keys: &Keys,
) -> Result<(), SetupError> {
    let response = submit_signed_event_at_with_keys(event, state, base, keys)
        .await
        .map_err(external)?;
    if response.event_id != event.id.to_hex() {
        return Err(external(
            "The relay acknowledgement named another event; the launch outcome remains unknown.",
        ));
    }
    Ok(())
}

pub(super) async fn publish(
    state: &AppState,
    draft: &ProjectTeamSetupDraft,
    saved: &journal::LaunchJournal,
    keys: &Keys,
) -> Result<(), SetupError> {
    let base = relay_http_base_url(&draft.relay_url);
    // A previously accepted genesis can answer an exact retry as a duplicate;
    // verify stored identity rather than freshly signing a replacement.
    let existing=query(state,draft,keys,json!({"ids":[saved.reservation.genesis_event_id],"kinds":[saved.reservation.genesis_event.kind.as_u16()],"limit":2})).await?;
    if !existing
        .iter()
        .any(|event| event == &saved.reservation.genesis_event && event.verify().is_ok())
    {
        submit_exact(&saved.reservation.genesis_event, state, &base, keys).await?;
    }
    submit_exact(&saved.create_event, state, &base, keys).await?;
    Ok(())
}
