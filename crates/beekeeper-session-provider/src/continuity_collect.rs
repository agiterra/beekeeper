//! Exact actor-authorized, per-read continuity collection; no cached successes.
use super::continuity_bootstrap::{PreparedBootstrap, Refusal, RequiredFacts};
use beekeeper_acp::relay::RestClient;
use beekeeper_core::continuity::*;
use nostr::{Event, Filter, Kind, PublicKey};
use serde::Serialize;
use std::collections::BTreeMap;

/// A private read result with bounded, explicit coverage.
#[derive(Clone, Serialize)]
pub struct Knowledge {
    /// Head observation without decrypted raw storage bytes.
    pub status: String,
    /// Per-claim dispositions for every claim in the head.
    pub claims: Vec<ClaimOutcome>,
    /// Sources actually read on this invocation.
    pub source_ids: Vec<String>,
    /// Exact query candidate count, including malformed rows.
    pub result_count: usize,
    /// Read time in Unix seconds.
    pub read_at: u64,
}
/// Current actor credentials, retained only by its running execution.
#[derive(Clone)]
pub struct Custody {
    pub(crate) rest: RestClient,
    pub(crate) owner: PublicKey,
}
impl std::fmt::Debug for Custody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Custody(<private>)")
    }
}
impl Custody {
    /// Validate existing managed launch custody. Never mint an identity.
    pub fn from_launch(request: &crate::session::CreateRequest) -> Result<Self, Refusal> {
        let seat = request.seat.as_ref().ok_or(Refusal::Unavailable)?;
        let get = |name: &str| {
            request
                .post_fence_env
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.as_str())
                .ok_or(Refusal::Unavailable)
        };
        let keys =
            nostr::Keys::parse(get("BEEKEEPER_PRIVATE_KEY")?).map_err(|_| Refusal::Unavailable)?;
        let tag = get("BEEKEEPER_AUTH_TAG")?;
        let owner = beekeeper_sdk::nip_oa::verify_auth_tag(tag, &keys.public_key())
            .map_err(|_| Refusal::Unavailable)?;
        let base_url =
            canonical_origin(get("BEEKEEPER_RELAY_URL")?).map_err(|_| Refusal::Unavailable)?;
        if keys.public_key().to_hex() != seat.actor_pubkey
            || canonical_origin(&seat.relay_url).map_err(|_| Refusal::Unavailable)? != base_url
        {
            return Err(Refusal::Unavailable);
        }
        Ok(Self {
            owner,
            rest: RestClient {
                http: reqwest::Client::new(),
                base_url,
                keys,
                auth_tag_json: Some(tag.into()),
            },
        })
    }
    /// Revalidate attestation on each read, including expiry and owner identity.
    pub fn validate(&self, scope: &Scope, actor: &str) -> Result<(), Refusal> {
        if self.rest.base_url != scope.community || self.rest.keys.public_key().to_hex() != actor {
            return Err(Refusal::Unavailable);
        }
        let owner = beekeeper_sdk::nip_oa::verify_auth_tag(
            self.rest
                .auth_tag_json
                .as_deref()
                .ok_or(Refusal::Unavailable)?,
            &self.rest.keys.public_key(),
        )
        .map_err(|_| Refusal::Unavailable)?;
        if owner != self.owner {
            return Err(Refusal::Unavailable);
        }
        Ok(())
    }
}
/// Query one exact address and every needed exact source under current access.
pub async fn collect(custody: &Custody, scope: &Scope) -> Knowledge {
    let mut result = Knowledge {
        status: "unknown/query_failed".into(),
        claims: vec![],
        source_ids: vec![],
        result_count: 0,
        read_at: crate::now_secs(),
    };
    if custody
        .validate(scope, &custody.rest.keys.public_key().to_hex())
        .is_err()
    {
        result.status = "unknown/custody_or_scope".into();
        return result;
    }
    let Ok(slug) = scope.slug() else {
        result.status = "unknown/invalid_scope".into();
        return result;
    };
    let actor = custody.rest.keys.public_key();
    let key =
        beekeeper_core::engram::conversation_key(custody.rest.keys.secret_key(), &custody.owner);
    let filter = Filter::new()
        .kind(Kind::Custom(30174))
        .author(actor)
        .pubkey(custody.owner)
        .identifier(beekeeper_core::engram::d_tag(&key, &slug))
        .limit(HEAD_QUERY_LIMIT);
    let Ok(rows) = custody.rest.query(&[filter]).await else {
        return result;
    };
    let Some(rows) = rows.as_array() else {
        return result;
    };
    result.result_count = rows.len();
    let events: Result<Vec<Event>, _> = rows
        .iter()
        .map(|r| serde_json::from_value(r.clone()))
        .collect();
    let Ok(events) = events else {
        result.status = "unknown/invalid_candidate".into();
        return result;
    };
    let identity = ReadIdentity {
        actor: &actor,
        owner: &custody.owner,
        reader_secret: custody.rest.keys.secret_key(),
        peer: &custody.owner,
    };
    let head = select_head(scope, &identity, &events, QueryCoverage::Complete);
    match head {
        HeadRead::AbsentAtRead => result.status = "absent_at_read".into(),
        HeadRead::WithdrawnRecord { event_id } => {
            result.status = "withdrawn_record".into();
            result.source_ids.push(event_id);
        }
        HeadRead::Unknown { reason } => {
            result.status = format!(
                "unknown/{}",
                serde_json::to_value(reason)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_else(|| "unavailable".into())
            )
        }
        HeadRead::Present { head } => {
            result.status = "observed_head".into();
            result.source_ids.push(head.event_id.clone());
            let mut evidence = BTreeMap::new();
            for claim in &head.record.claims {
                if claim.state == ClaimState::Withdrawn || head.conflicts.contains(&claim.id) {
                    continue;
                }
                for reference in &claim.evidence {
                    let key = (
                        reference.event_id.clone(),
                        reference.kind,
                        reference.channel.clone(),
                    );
                    if evidence.contains_key(&key) {
                        continue;
                    }
                    let fetched = fetch_evidence(&custody.rest, reference).await;
                    result.source_ids.push(reference.event_id.clone());
                    evidence.insert(key, fetched);
                }
            }
            match evaluate_claims(&head, &[], |r| {
                evidence
                    .get(&(r.event_id.clone(), r.kind, r.channel.clone()))
                    .cloned()
                    .unwrap_or(Err(UnknownReason::EvidenceUnavailable))
            }) {
                Ok(claims) => result.claims = claims,
                Err(_) => result.status = "unknown/invalid_head".into(),
            }
        }
    }
    result
}
async fn fetch_evidence(rest: &RestClient, reference: &Evidence) -> Result<Event, UnknownReason> {
    let id = nostr::EventId::from_hex(&reference.event_id)
        .map_err(|_| UnknownReason::EvidenceInvalid)?;
    let filter = Filter::new()
        .id(id)
        .kind(Kind::Custom(reference.kind))
        .custom_tag(
            nostr::SingleLetterTag::lowercase(nostr::Alphabet::H),
            reference.channel.clone(),
        )
        .limit(2);
    let rows = rest
        .query(&[filter])
        .await
        .map_err(|_| UnknownReason::EvidenceUnavailable)?;
    let rows = rows.as_array().ok_or(UnknownReason::EvidenceUnavailable)?;
    if rows.len() != 1 {
        return Err(UnknownReason::EvidenceUnavailable);
    }
    let event: Event =
        serde_json::from_value(rows[0].clone()).map_err(|_| UnknownReason::EvidenceInvalid)?;
    verify_evidence(reference, &event)?;
    Ok(event)
}

impl crate::Provider {
    pub(crate) async fn prepare_continuity(
        &mut self,
        request: &crate::continuity_delivery::Request,
    ) -> Result<PreparedBootstrap, Refusal> {
        let record = self
            .state
            .session(&request.target.session_id)
            .cloned()
            .ok_or(Refusal::Unavailable)?;
        if self.target_for(&record) != request.target
            || record.closed
            || record.is_retired()
            || record.authority_withdrawn.is_some()
            || self.state.is_command_consumed(&request.command_id)
            || self.state.is_command_refused(&request.command_id)
        {
            return Err(Refusal::Unavailable);
        }
        let actor = record.actor.as_deref().ok_or(Refusal::Unavailable)?;
        if self.actor_has_unproved_stop(Some(actor)) {
            return Err(Refusal::Unavailable);
        }
        let _ownership = crate::host_ownership::admit(
            &self.config,
            Some(actor),
            crate::host_ownership::Admission::Turn,
        )
        .map_err(|_| Refusal::Unavailable)?;
        self.require_actor_not_retired(Some(actor))
            .await
            .map_err(|_| Refusal::Unavailable)?;
        let scope = Scope::new(
            &self.config.relay_url,
            &record.channel_id.to_string(),
            record.session_ref.as_deref(),
        )
        .map_err(|_| Refusal::Unavailable)?;
        let custody = request.custody.as_ref().map_err(|r| *r)?;
        custody.validate(&scope, actor)?;
        let operator = request.operator.as_deref().ok_or(Refusal::Unavailable)?;
        if self
            .in_flight
            .get(&request.command_id)
            .is_some_and(|t| t.fenced_after_dispatch || t.native_authority_refusal.is_some())
            || record
                .genesis_ref
                .as_deref()
                .is_some_and(|g| self.claims_pending_reverification.contains(g))
            || crate::commands::handover_fence(&record, operator, &self.pubkey_hex).is_some()
        {
            return Err(Refusal::Unavailable);
        }
        let mut source_ids = vec![request.command_id.clone()];
        let mut directions = if record.role.is_some() || record.pack_ref.is_some() {
            if !crate::commands::operator_may_steer(&record, operator) {
                return Err(Refusal::Unavailable);
            }
            let role = request
                .role_definition
                .as_ref()
                .ok_or(Refusal::Unavailable)?;
            if role.pack != record.pack_ref
                || role.compose != record.compose_ref
                || (role.pack.is_none() && role.compose.is_none())
            {
                return Err(Refusal::Unavailable);
            }
            let pack = role.pack.as_ref().ok_or(Refusal::Unavailable)?;
            if record.role.as_deref() != Some(pack.role.as_str()) {
                return Err(Refusal::Unavailable);
            }
            source_ids.extend(
                current_role_source(
                    &custody.rest,
                    self.relay_self.as_deref(),
                    record.project_ref.as_deref().ok_or(Refusal::Unavailable)?,
                    pack,
                )
                .await?,
            );
            format!("Authorized staged role {}. Pack revision: {}. Composition revision: {}.\n{}\nCurrent authenticated command {} by {} (authorized coordination; human authorship is not inferred).",record.role.as_deref().unwrap_or("none"),serde_json::to_string(&role.pack).map_err(|_|Refusal::Unavailable)?,serde_json::to_string(&role.compose).map_err(|_|Refusal::Unavailable)?,role.text,request.command_id,operator)
        } else {
            if !self
                .conversation_author_allowed(actor, record.channel_id, operator, None)
                .await
                .map_err(|_| Refusal::Unavailable)?
            {
                return Err(Refusal::Unavailable);
            }
            let (owner, definition, _) = self
                .conversation_definition_head(actor)
                .await
                .map_err(|_| Refusal::Unavailable)?;
            if owner != custody.owner {
                return Err(Refusal::Unavailable);
            }
            source_ids.push(definition.id.to_hex());
            self.conversation_instructions(&request.command_id, operator)
                .await
                .map_err(|_| Refusal::Unavailable)?
                .ok_or(Refusal::Unavailable)?
        };
        if let Some(preface) = &request.runtime_preface {
            directions.push_str("\nRequired runtime context:\n");
            directions.push_str(preface);
        }
        let work = self
            .assignment_work_brief(&request.target, &request.command_id, &request.text)
            .await
            .map_err(|_| Refusal::Unavailable)?;
        let mut work = match (work, record.session_ref.as_ref()) {
            (Some(work), _) => work,
            (None, None) => "no_canonical_assignment".into(),
            // An umbrella turn without a current work fold cannot invent an empty responsibility.
            (None, Some(_)) => return Err(Refusal::Unavailable),
        };
        if let Some(card) = self
            .seat_situation_card(&request.target.session_id, &request.text, None)
            .await
        {
            work.push_str("\nCurrent situation:\n");
            work.push_str(&card);
        }
        let knowledge = collect(custody, &scope).await;
        let facts = RequiredFacts {
            target: request.target.clone(),
            command_id: request.command_id.clone(),
            scope,
            actor: actor.into(),
            owner: custody.owner.to_hex(),
            directions,
            work,
            checkpoint: request.checkpoint.clone(),
            read_at: knowledge.read_at,
            source_ids,
        };
        let mut prepared = crate::continuity_bootstrap::prepare(facts, &knowledge)?;
        let binding = crate::continuity_archive::Binding::for_record(
            &record,
            &self.config.instance_id,
            &self.config.relay_url,
            &custody.owner.to_hex(),
        )?;
        let mut archive = match crate::context_store::read_continuity_archive(
            &self.config.state_dir,
            &record.session_id,
        )
        .map_err(|_| Refusal::PrivateDeliveryUnavailable)?
        {
            Some(archive) if archive.binding == binding => archive,
            Some(_) => return Err(Refusal::PrivateDeliveryUnavailable),
            None => crate::continuity_archive::Archive {
                binding,
                fragments: Vec::new(),
            },
        };
        for fragment in prepared.private_fragments() {
            if !archive.fragments.contains(fragment) {
                archive.fragments.push(fragment.clone());
            }
        }
        crate::context_store::write_continuity_archive(&self.config.state_dir, &archive)
            .map_err(|_| Refusal::PrivateDeliveryUnavailable)?;
        prepared.persist_prepared(&self.config.state_dir)?;
        Ok(prepared)
    }
}

impl crate::Provider {
    pub(crate) async fn resolve_continuity_event(
        &mut self,
        event: crate::session::SessionEvent,
    ) -> crate::session::SessionEvent {
        match event {
            crate::session::SessionEvent::ContinuityAdmissionRequested { request, decision } => {
                let result = tokio::time::timeout(
                    std::time::Duration::from_secs(25),
                    self.prepare_continuity(&request),
                )
                .await
                .unwrap_or(Err(Refusal::Unavailable));
                let _ = decision.send(Some(result));
                crate::session::SessionEvent::ContinuityAdmissionSettled
            }
            other => other,
        }
    }
}

/// Read current project source and relay-signed ref state, rather than treating
/// the execution's old packRef as continuing authority. Local/shipped sources
/// without an authenticated current source are deliberately unavailable here.
pub(crate) async fn current_role_source(
    rest: &RestClient,
    relay_self: Option<&str>,
    project: &str,
    pack: &crate::actor_seats::PackRef,
) -> Result<Vec<String>, Refusal> {
    use beekeeper_core::project_pack_source::{decode_project_pack_source, PackPin};
    let source = exact_current(
        rest,
        Filter::new().kind(Kind::Custom(30624)).identifier(project),
    )
    .await?;
    let decoded = decode_project_pack_source(&source).map_err(|_| Refusal::Unavailable)?;
    if Some(source.pubkey.to_hex().as_str()) != project.split(':').nth(1)
        || decoded.project() != project
        || decoded.repo() != pack.repo
    {
        return Err(Refusal::Unavailable);
    }
    if decoded.path() != "."
        && pack.path != decoded.path()
        && !pack.path.starts_with(&format!("{}/", decoded.path()))
    {
        return Err(Refusal::Unavailable);
    }
    let mut ids = vec![source.id.to_hex()];
    let sha = match decoded.pin() {
        PackPin::Sha(sha) => sha.clone(),
        PackPin::Ref(reference) => {
            let relay = PublicKey::from_hex(relay_self.ok_or(Refusal::Unavailable)?)
                .map_err(|_| Refusal::Unavailable)?;
            let repo = pack
                .repo
                .splitn(3, ':')
                .nth(2)
                .ok_or(Refusal::Unavailable)?;
            let state = exact_current(
                rest,
                Filter::new()
                    .kind(Kind::Custom(30618))
                    .author(relay)
                    .identifier(repo),
            )
            .await?;
            if state.pubkey != relay {
                return Err(Refusal::Unavailable);
            }
            let refs: Vec<_> = state
                .tags
                .iter()
                .filter(|t| t.as_slice().first() == Some(reference))
                .collect();
            if refs.len() != 1 {
                return Err(Refusal::Unavailable);
            }
            let sha = refs[0]
                .as_slice()
                .get(1)
                .ok_or(Refusal::Unavailable)?
                .clone();
            ids.push(state.id.to_hex());
            sha
        }
    };
    if sha != pack.sha {
        return Err(Refusal::Unavailable);
    }
    Ok(ids)
}
async fn exact_current(rest: &RestClient, filter: Filter) -> Result<Event, Refusal> {
    let rows = rest
        .query(&[filter.clone().limit(16)])
        .await
        .map_err(|_| Refusal::Unavailable)?;
    let rows = rows.as_array().ok_or(Refusal::Unavailable)?;
    if rows.is_empty() || rows.len() >= 16 {
        return Err(Refusal::Unavailable);
    }
    let mut events = Vec::new();
    for row in rows {
        let event: Event = serde_json::from_value(row.clone()).map_err(|_| Refusal::Unavailable)?;
        if event.verify().is_err()
            || !filter.match_event(&event, nostr::filter::MatchEventOptions::default())
        {
            return Err(Refusal::Unavailable);
        }
        events.push(event);
    }
    events
        .into_iter()
        .max_by_key(|e| (e.created_at, std::cmp::Reverse(e.id)))
        .ok_or(Refusal::Unavailable)
}
