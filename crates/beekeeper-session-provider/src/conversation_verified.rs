//! Historical admission and typed Host notice provenance; origins alone prove neither.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Admission {
    actor: String,
    channel: Uuid,
    target: CodingSessionTarget,
    source: String,
    author: String,
    root: String,
    project: String,
    genesis: String,
    owner: String,
    admitted: bool,
    refused: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct HostNotice {
    hire: String,
    source: String,
    requester: String,
    source_author: String,
    project: String,
    genesis: String,
    owner: String,
    pub(super) hire_event: Event,
    #[serde(default)]
    transcript: Option<Event>,
}

pub(super) fn prepare_admission(
    origin: &mut Origin,
    scope: &crate::conversation_project::ProjectContext,
    owner: &str,
) -> anyhow::Result<()> {
    // Never upgrade an already framed, historically unbound origin.
    if origin.admission.is_none() && origin.instructions.is_some() {
        return Ok(());
    }
    let source = origin
        .signed_source
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("source signature missing"))?;
    source.verify()?;
    anyhow::ensure!(
        source.id.to_hex() == origin.event
            && origin.author.as_deref() == Some(source.pubkey.to_hex().as_str()),
        "source author binding missing"
    );
    let proposed = Admission {
        actor: origin.actor.clone(),
        channel: origin.channel,
        target: origin.target.clone(),
        source: origin.event.clone(),
        author: source.pubkey.to_hex(),
        root: origin.root.clone(),
        project: scope.project.clone(),
        genesis: scope.genesis.id.to_hex(),
        owner: owner.into(),
        admitted: false,
        refused: false,
    };
    if let Some(previous) = &origin.admission {
        let mut comparison = proposed;
        comparison.admitted = previous.admitted;
        comparison.refused = previous.refused;
        anyhow::ensure!(
            *previous == comparison,
            "historical admission scope changed"
        );
    } else {
        origin.admission = Some(proposed);
    }
    Ok(())
}

pub(super) fn record_admission_receipt(
    origin: &mut Origin,
    receipt: &crate::payload::LifecycleReceipt,
) {
    use crate::payload::ReceiptStatus;
    if receipt.session.as_ref() != Some(&origin.target) {
        return;
    }
    if let Some(admission) = &mut origin.admission {
        match receipt.status {
            ReceiptStatus::TurnRefused | ReceiptStatus::TurnDropped => {
                admission.refused = true;
                admission.admitted = false;
            }
            ReceiptStatus::TurnQueued
            | ReceiptStatus::TurnStarted
            | ReceiptStatus::TurnInjected
                if !admission.refused =>
            {
                admission.admitted = true
            }
            _ => {}
        }
    }
}

fn verified_source<'a>(
    origins: &'a Origins,
    actor: &str,
    channel: Uuid,
    source: &str,
    project: &str,
    genesis: &str,
) -> anyhow::Result<(&'a Origin, &'a Admission)> {
    let origin = origins
        .requests
        .get(source)
        .ok_or_else(|| anyhow::anyhow!("HIRE_CHAT_ORIGIN_UNRESOLVED: source missing"))?;
    let facts = origin
        .admission
        .as_ref()
        .filter(|facts| facts.admitted && !facts.refused)
        .ok_or_else(|| {
            anyhow::anyhow!("HIRE_CHAT_ADMISSION_UNVERIFIED: no successful admission")
        })?;
    let event = origin
        .signed_source
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("HIRE_CHAT_SOURCE_UNVERIFIED"))?;
    event.verify()?;
    anyhow::ensure!(
        origin.actor == actor
            && origin.channel == channel
            && origin.event == source
            && facts.actor == actor
            && facts.channel == channel
            && facts.source == source
            && facts.target == origin.target
            && facts.project == project
            && facts.genesis == genesis
            && origin.author.as_deref() == Some(facts.author.as_str())
            && event.pubkey.to_hex() == facts.author
            && event.id.to_hex() == source
            && event.kind.as_u16() == 9
            && root(event) == origin.root
            && facts.root == origin.root
            && event
                .tags
                .iter()
                .any(|tag| tag.as_slice() == ["h", &channel.to_string()]),
        "HIRE_CHAT_ADMISSION_SCOPE_MISMATCH: historical source/author/execution/project differs"
    );
    Ok((origin, facts))
}

impl Admission {
    /// This exact source, author, execution and channel, and not refused. Turn
    /// start, not admission, sets `admitted`, so a steer on its way to the
    /// boundary is not yet admitted.
    pub(super) fn names_unrefused(
        &self,
        source: &str,
        author: &str,
        target: &CodingSessionTarget,
        channel: Uuid,
    ) -> bool {
        !self.refused
            && self.source == source
            && self.author == author
            && self.target == *target
            && self.channel == channel
    }

    #[cfg(test)]
    pub(super) fn fixture(
        actor: &str,
        channel: Uuid,
        target: &CodingSessionTarget,
        source: &str,
        author: &str,
        refused: bool,
    ) -> Self {
        Self {
            actor: actor.to_owned(),
            channel,
            target: target.clone(),
            source: source.to_owned(),
            author: author.to_owned(),
            root: source.to_owned(),
            project: "project".to_owned(),
            genesis: "ab".repeat(32),
            owner: "cd".repeat(32),
            admitted: false,
            refused,
        }
    }
}

pub(crate) fn admitted_reply_origin(
    state_dir: &std::path::Path,
    actor: &str,
    channel: Uuid,
    source: &str,
    project: &str,
    genesis: &str,
) -> anyhow::Result<beekeeper_sdk::ThreadRef> {
    let origins: Origins = load(&state_dir.join("conversation-origins.json"))?;
    let (origin, _) = verified_source(&origins, actor, channel, source, project, genesis)?;
    Ok(beekeeper_sdk::ThreadRef {
        root_event_id: nostr::EventId::from_hex(&origin.root)?,
        parent_event_id: nostr::EventId::from_hex(&origin.event)?,
    })
}

fn one_tag<'a>(event: &'a Event, name: &str) -> anyhow::Result<Option<&'a str>> {
    let mut tags = event
        .tags
        .iter()
        .filter(|tag| tag.as_slice().first().is_some_and(|v| v == name));
    let first = tags.next();
    anyhow::ensure!(tags.next().is_none(), "HOST_NOTICE_AMBIGUOUS_TAG");
    if let Some(tag) = first {
        anyhow::ensure!(tag.as_slice().len() == 2, "HOST_NOTICE_MALFORMED_TAG");
    }
    Ok(first.map(|tag| tag.as_slice()[1].as_str()))
}

#[derive(Debug, thiserror::Error)]
#[error("HOST_NOTICE_LOOKUP_PENDING: {0}")]
pub(super) struct NoticeLookupPending(String);

pub(super) fn verify_host_notice_origin(
    host: &Provider,
    channel: Uuid,
    event: &Event,
    actor: &str,
) -> anyhow::Result<Option<String>> {
    event.verify()?;
    use beekeeper_core::coding_session_payload::{
        CODING_SESSION_HOST_ANSWER_TAG_HIRE as HIRE, CODING_SESSION_HOST_ANSWER_TAG_NAME as HOST,
    };
    let tag = one_tag(event, HOST)?;
    let hire = one_tag(event, "hire-ref")?;
    if tag.is_none() && hire.is_none() {
        return Ok(None);
    }
    anyhow::ensure!(
        tag == Some(HIRE),
        "HOST_NOTICE_UNVERIFIED: missing recognized Host tag"
    );
    let hire = hire.ok_or_else(|| anyhow::anyhow!("HOST_NOTICE_UNVERIFIED: missing hire"))?;
    let origins = host
        .conversation_origins()
        .map_err(|error| NoticeLookupPending(format!("HOST_NOTICE_STATE_UNAVAILABLE: {error}")))?;
    // Only a historically verified owner notice may wait on transport. A
    // participant's forged tag must not turn a missing hire into replay poison.
    let source_hint = event
        .tags
        .iter()
        .find_map(|tag| {
            let parts = tag.as_slice();
            (parts.len() == 4 && parts[0] == "e" && parts[3] == "reply").then(|| parts[1].as_str())
        })
        .ok_or_else(|| anyhow::anyhow!("HOST_NOTICE_UNVERIFIED: source reply missing"))?;
    let historical = origins
        .requests
        .get(source_hint)
        .and_then(|origin| origin.admission.as_ref())
        .ok_or_else(|| anyhow::anyhow!("HOST_NOTICE_UNVERIFIED: source admission missing"))?;
    let (_, facts) = verified_source(
        &origins,
        actor,
        channel,
        source_hint,
        &historical.project,
        &historical.genesis,
    )?;
    anyhow::ensure!(
        event.pubkey.to_hex() == facts.owner && one_tag(event, "p")? == Some(actor),
        "HOST_NOTICE_UNVERIFIED: owner/recipient differs"
    );
    anyhow::ensure!(
        one_tag(event, "h")? == Some(channel.to_string().as_str()),
        "HOST_NOTICE_UNVERIFIED: channel differs"
    );
    let hire_id = nostr::EventId::from_hex(hire)?;
    anyhow::ensure!(
        hire_id.to_hex() == hire,
        "HOST_NOTICE_UNVERIFIED: noncanonical hire"
    );
    let source_origin = origins
        .requests
        .get(source_hint)
        .ok_or_else(|| anyhow::anyhow!("source missing"))?;
    let refs: Vec<_> = event
        .tags
        .iter()
        .filter(|tag| tag.as_slice().first().is_some_and(|v| v == "e"))
        .collect();
    anyhow::ensure!(
        ((source_origin.root == source_hint && refs.len() == 1)
            || (source_origin.root != source_hint
                && refs.len() == 2
                && refs
                    .iter()
                    .any(|tag| tag.as_slice() == ["e", &source_origin.root, "", "root"])))
            && refs
                .iter()
                .any(|tag| tag.as_slice() == ["e", source_hint, "", "reply"]),
        "HOST_NOTICE_UNVERIFIED: original reply markers differ"
    );
    Ok(Some(hire.to_owned()))
}

pub(super) async fn verify_host_notice(
    host: &mut Provider,
    channel: Uuid,
    event: &Event,
    actor: &str,
) -> anyhow::Result<Option<HostNotice>> {
    let Some(hire) = verify_host_notice_origin(host, channel, event, actor)? else {
        return Ok(None);
    };
    host.retain_host_notice(channel, event, actor, &hire)
        .map_err(|error| {
            if error.is::<recovery::NoticeCapacity>() {
                error
            } else {
                anyhow::Error::from(NoticeLookupPending(format!(
                    "HOST_NOTICE_RETENTION_FAILED: {error}"
                )))
            }
        })?;
    let origins = host
        .conversation_origins()
        .map_err(|error| NoticeLookupPending(format!("HOST_NOTICE_STATE_UNAVAILABLE: {error}")))?;
    let cached = origins
        .requests
        .get(&event.id.to_hex())
        .and_then(|origin| origin.host_notice.as_ref());
    let request = match cached {
        Some(cached) => cached.hire_event.clone(),
        None if origins
            .pending_notices
            .get(&event.id.to_hex())
            .and_then(|pending| pending.hire.as_ref())
            .is_some() =>
        {
            origins.pending_notices[&event.id.to_hex()]
                .hire
                .clone()
                .ok_or_else(|| anyhow::anyhow!("pending hire missing"))?
        }
        None => {
            let rest = host
                .rest_client
                .as_ref()
                .ok_or_else(|| NoticeLookupPending("transport missing".into()))?;
            tokio::time::timeout(
                host.conversation_backfill.slot.timeout,
                rest.query_event_by_id(
                    &hire,
                    Kind::Custom(
                        beekeeper_core::kind::KIND_CODING_SESSION_LIFECYCLE_COMMAND as u16,
                    ),
                ),
            )
            .await
            .map_err(|_| NoticeLookupPending("exact hire lookup timed out".into()))?
            .map_err(|error| NoticeLookupPending(error.to_string()))?
            .ok_or_else(|| NoticeLookupPending("exact hire not yet readable".into()))?
        }
    };
    request.verify()?;
    let payload =
        beekeeper_core::coding_session_lifecycle_command::decode_coding_session_lifecycle_command(
            &request.content,
        )
        .map_err(anyhow::Error::msg)?;
    anyhow::ensure!(
        payload.hire_requester_matches_signer(actor) != Some(false),
        "HOST_NOTICE_UNVERIFIED: disputed requester"
    );
    let beekeeper_core::coding_session_lifecycle_command::CodingSessionLifecycleAction::SessionHire {reply_to: Some(source), genesis_ref, ..} = payload.action else { anyhow::bail!("HOST_NOTICE_UNVERIFIED: hire lacks source"); };
    anyhow::ensure!(
        request.kind.as_u16() as u32 == beekeeper_core::kind::KIND_CODING_SESSION_LIFECYCLE_COMMAND
            && request.id.to_hex() == hire
            && request.pubkey.to_hex() == actor
            && one_tag(&request, "h")? == Some(channel.to_string().as_str()),
        "HOST_NOTICE_UNVERIFIED: hire requester/channel differs"
    );
    let historical = origins
        .requests
        .get(&source)
        .and_then(|origin| origin.admission.as_ref())
        .ok_or_else(|| anyhow::anyhow!("HOST_NOTICE_UNVERIFIED: source admission missing"))?;
    let (origin, facts) = verified_source(
        &origins,
        actor,
        channel,
        &source,
        &historical.project,
        &genesis_ref,
    )?;
    anyhow::ensure!(
        event.pubkey.to_hex() == facts.owner && one_tag(event, "p")? == Some(actor),
        "HOST_NOTICE_UNVERIFIED: owner/recipient differs"
    );
    let refs: Vec<_> = event
        .tags
        .iter()
        .filter(|tag| tag.as_slice().first().is_some_and(|v| v == "e"))
        .collect();
    anyhow::ensure!(
        ((origin.root == source && refs.len() == 1)
            || (origin.root != source
                && refs.len() == 2
                && refs
                    .iter()
                    .any(|tag| tag.as_slice() == ["e", &origin.root, "", "root"])))
            && refs
                .iter()
                .any(|tag| tag.as_slice() == ["e", &source, "", "reply"]),
        "HOST_NOTICE_UNVERIFIED: original reply markers differ"
    );
    Ok(Some(HostNotice {
        hire,
        source,
        requester: actor.into(),
        source_author: facts.author.clone(),
        project: facts.project.clone(),
        genesis: facts.genesis.clone(),
        owner: facts.owner.clone(),
        hire_event: request,
        transcript: cached.and_then(|cached| cached.transcript.clone()),
    }))
}

impl Provider {
    pub(crate) fn conversation_is_host_answer(&self, command: &str) -> anyhow::Result<bool> {
        Ok(self
            .conversation_origins()?
            .requests
            .get(command)
            .is_some_and(|origin| origin.host_notice.is_some()))
    }

    /// Persist one exact existing Host-answer transcript before publishing it.
    /// A notice never enters a model mailbox or obtains founder override framing.
    pub(crate) fn record_conversation_host_answer(
        &mut self,
        command: &str,
        text: &str,
    ) -> anyhow::Result<bool> {
        let mut origins = self.conversation_origins()?;
        let Some(origin) = origins.requests.get_mut(command) else {
            return Ok(false);
        };
        let Some(notice) = origin.host_notice.as_mut() else {
            return Ok(false);
        };
        if notice.transcript.is_none() {
            let seq = self
                .state
                .allocate_seq(&origin.target.session_id)?
                .ok_or_else(|| anyhow::anyhow!("Host-answer target unavailable"))?;
            let frame = serde_json::json!({"type":"host_hire_refusal", "hire":notice.hire, "source":notice.source,
                "requester":notice.requester,"sourceAuthor":notice.source_author,"project":notice.project,
                "genesis":notice.genesis,"hostOwner":notice.owner,"notice":text});
            let item = crate::payload::host_answer_prompt_item(
                &serde_json::to_string(&frame)?,
                Some(&notice.owner),
                Some(command),
                None,
            );
            let envelope = crate::payload::TranscriptEnvelope::new(
                &origin.target,
                seq,
                crate::state::now_ms(),
                None,
                item,
            );
            notice.transcript = Some(
                beekeeper_sdk::build_coding_session_transcript_item(
                    origin.channel,
                    &origin.target,
                    seq,
                    &serde_json::to_string(&envelope)?,
                )?
                .sign_with_keys(&self.config.keys)?,
            );
        }
        let event = notice
            .transcript
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Host transcript missing"))?;
        #[cfg(test)]
        self.notice_write_fault(0)?;
        self.save_conversation_origins(&origins)?;
        #[cfg(test)]
        self.notice_write_fault(1)?;
        // The signed transcript and sequence survive an ACK loss or process restart.
        self.outbox.enqueue(
            beekeeper_core::kind::KIND_CODING_SESSION_TRANSCRIPT,
            &format!("conversation-host-answer:{command}"),
            Priority::Normal,
            event,
        )?;
        #[cfg(test)]
        self.notice_write_fault(2)?;
        self.state.consume_command(command, crate::now_secs())?;
        Ok(true)
    }
}
