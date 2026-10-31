//! Owner cleanup is independent of the requester's current delegation grant.
use super::*;
use beekeeper_core::coding_session_lifecycle_command::CodingSessionLifecycleCommandPayload;

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Cleanup {
    pub(super) deadline: Option<u64>,
    pub(super) reason: Option<String>,
    stop: Option<Event>,
    stopped: bool,
    stop_refusal: Option<String>,
}
impl Cleanup {
    pub(super) fn new(seconds: Option<u64>) -> Self {
        Self {
            deadline: seconds.map(|s| nostr::Timestamp::now().as_secs().saturating_add(s)),
            ..Self::default()
        }
    }
    pub(super) fn settled(&self) -> bool {
        self.stopped
    }
}

pub(super) async fn reconcile(
    path: &std::path::Path,
    operation: &mut Operation,
    client: &impl RelayTransport,
    scope: &Scope,
    request: &Event,
    bound: Option<u64>,
    authorization: &Result<(), Error>,
) -> Result<Option<Disposition>, Error> {
    let Some(progress) = operation.hire.as_mut() else {
        return Ok(None);
    };
    if bound.is_none() && progress.cleanup.deadline.is_none() {
        return Ok(None);
    }
    if progress.cleanup.stopped {
        return Ok(Some(Disposition::Stopped {
            target: operation.target.clone(),
        }));
    }
    // Adopting an older journal cannot reset its lifetime from now.
    if progress.cleanup.deadline.is_none() {
        progress.cleanup.deadline =
            bound.map(|s| progress.plan.create.created_at.as_secs().saturating_add(s));
    }
    if progress.cleanup.reason.is_none() {
        if progress
            .cleanup
            .deadline
            .is_some_and(|d| nostr::Timestamp::now().as_secs() >= d)
        {
            progress.cleanup.reason = Some(
                "worker deadline reached; outcome requires evidence, not a success claim".into(),
            );
        } else if matches!(authorization, Err(Error::Auth(_)) | Err(Error::Refused(_))) {
            progress.cleanup.reason =
                Some("requester authority revoked; owner cleanup only".into());
        }
    }
    let progress = operation
        .hire
        .clone()
        .ok_or_else(|| fail("hire progress missing"))?;
    save(path, operation)?;
    let Some(reason) = progress.cleanup.reason.clone() else {
        return Ok(None);
    };
    let owner = owner(client)?;
    validate_plan(scope, request, &progress.plan, &owner)?;
    let create =
        decode_coding_session_lifecycle_command(&progress.plan.create.content).map_err(fail)?;
    let CodingSessionLifecycleAction::SessionCreate {
        provider_authority_pubkey,
        ..
    } = create.action
    else {
        return Err(Error::Conflict("cleanup requires original create".into()));
    };
    if operation.target.is_none() && progress.publication_attempted && progress.refusal.is_none() {
        let rows = client.query_all(json!({"kinds":[44224],"#h":[scope.channel.to_string()],"#csl-command":[create.command_id]})).await?;
        if let Some(outcome) = crate::receipts::classify_create_receipts(
            &rows,
            &scope.channel.to_string(),
            &create.command_id,
            &provider_authority_pubkey,
        )
        .map_err(Error::Conflict)?
        {
            observe_receipt(path, operation, outcome)?;
        }
    }
    let Some(target) = operation.target.clone() else {
        if !progress.publication_attempted
            || operation.hire.as_ref().is_some_and(|p| p.refusal.is_some())
        {
            if let Some(p) = operation.hire.as_mut() {
                p.cleanup.stopped = true;
            }
            save(path, operation)?;
            return Ok(Some(Disposition::Stopped { target: None }));
        }
        // A possibly consumed create cannot be re-published after expiry or
        // released merely because no receipt is visible yet.
        return Ok(Some(Disposition::CleanupPending {
            target: None,
            reason,
        }));
    };
    if progress.cleanup.stop.is_none() {
        let payload = CodingSessionLifecycleCommandPayload {
            schema: beekeeper_core::coding_session_lifecycle_command::CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA.into(),
            command_id: Uuid::new_v4().to_string(),
            action: CodingSessionLifecycleAction::SessionStop { session: target.clone(), provider_authority_pubkey: provider_authority_pubkey.clone() },
        };
        let stop = client.sign_event_unchecked(
            beekeeper_sdk::build_coding_session_lifecycle_command(scope.channel, &payload)
                .map_err(fail)?,
        )?;
        if let Some(p) = operation.hire.as_mut() {
            p.cleanup.stop = Some(stop);
        }
        save(path, operation)?;
    }
    let stop = operation
        .hire
        .as_ref()
        .and_then(|p| p.cleanup.stop.clone())
        .ok_or_else(|| fail("cleanup stop absent"))?;
    let stop_id = decode_coding_session_lifecycle_command(&stop.content)
        .map_err(fail)?
        .command_id;
    // An ACK is not quiescence. A refusal stays a durable blocked obligation;
    // it never releases the worker or authorizes a replacement.
    if progress.cleanup.stop_refusal.is_none() {
        if let Ok(ack) = client.submit_event(stop).await {
            let reason = ack["message"].as_str().unwrap_or("");
            if ack["accepted"] == false
                && (reason.starts_with("invalid:") || reason.starts_with("blocked:"))
            {
                if let Some(p) = operation.hire.as_mut() {
                    p.cleanup.stop_refusal = Some(reason.into());
                }
                save(path, operation)?;
            }
        }
    }
    let rows = client
        .query_all(
            json!({"kinds":[44224],"#h":[scope.channel.to_string()],"#csl-command":[stop_id]}),
        )
        .await?;
    if crate::receipts::verified_command_receipts(
        &rows,
        &scope.channel.to_string(),
        &stop_id,
        &provider_authority_pubkey,
    )
    .iter()
    .any(|(_, r)| {
        r.status == beekeeper_core::coding_session_payload::ReceiptStatus::Stopped
            && r.session.as_ref() == Some(&target)
    }) {
        if let Some(p) = operation.hire.as_mut() {
            p.cleanup.stopped = true;
        }
        save(path, operation)?;
        return Ok(Some(Disposition::Stopped {
            target: Some(target),
        }));
    }
    let reason = operation
        .hire
        .as_ref()
        .and_then(|p| p.cleanup.stop_refusal.as_ref())
        .map(|r| format!("{reason}; stop permanently refused: {r}"))
        .unwrap_or(reason);
    Ok(Some(Disposition::CleanupPending {
        target: Some(target),
        reason,
    }))
}
