use super::*;
use crate::hire_evidence::{verify_hire_evidence, HireEvidenceRequest, HiredSeat, SeatReceipt};
use crate::receipts::{classify_create_receipts, CreateWaitOutcome};
use beekeeper_core::coding_session_command::coding_session_target_key;
use beekeeper_core::coding_session_payload::{LifecycleReceipt, SessionMetadata, SessionStatus};
use beekeeper_core::kind::{
    KIND_CODING_SESSION_LIFECYCLE_COMMAND as COMMAND,
    KIND_CODING_SESSION_LIFECYCLE_RECEIPT as RECEIPT, KIND_CODING_SESSION_METADATA as METADATA,
    KIND_CODING_SESSION_PROVIDER_CATALOG as CATALOG,
};
use beekeeper_sdk::coding_session::*;
use serde_json::Value;

fn tags(event: &Event, expected: &[(&str, &str)]) -> bool {
    event.tags.len() == expected.len()
        && event
            .tags
            .iter()
            .zip(expected)
            .all(|(a, (k, v))| a.as_slice() == [*k, *v])
}
fn metadata(event: &Event, channel: &str) -> Option<SessionMetadata> {
    if event.kind.as_u16() as u32 != METADATA || event.verify().is_err() {
        return None;
    }
    let m: SessionMetadata = serde_json::from_str(&event.content).ok()?;
    tags(
        event,
        &[
            ("h", channel),
            ("csm-v", CODING_SESSION_METADATA_TAG_VERSION),
            ("cs-target", &coding_session_target_key(&m.session)),
            ("csm-key", &coding_session_metadata_semantic_key(&m.session)),
        ],
    )
    .then_some(m)
}
// A stop receipt is durable evidence for the exact generation. Metadata uses
// second-resolution timestamps and cannot undo that evidence when rows tie.
fn stopped(
    rows: &[Value],
    channel: &str,
    owner: &str,
    provider: &str,
    target: &CodingSessionTarget,
) -> bool {
    rows.iter().filter_map(|v| serde_json::from_value::<Event>(v.clone()).ok()).any(|e| {
        if e.kind.as_u16() as u32 != COMMAND || e.pubkey.to_hex() != owner || e.verify().is_err() {
            return false;
        }
        let Ok(p) = decode_coding_session_lifecycle_command(&e.content) else { return false; };
        if !tags(&e, &[("h", channel), ("csl-v", beekeeper_core::coding_session_lifecycle_command::CODING_SESSION_LIFECYCLE_COMMAND_TAG_VERSION), ("csl-command", &p.command_id)]) {
            return false;
        }
        matches!(p.action, Action::SessionStop {session, provider_authority_pubkey} if session == *target && provider_authority_pubkey == provider)
            && crate::receipts::verified_command_receipts(rows, channel, &p.command_id, provider).iter().any(|(_, r)| r.status == beekeeper_core::coding_session_payload::ReceiptStatus::Stopped && r.session.as_ref() == Some(target))
    })
}
#[cfg(test)]
#[path = "hire_stop_tests.rs"]
mod stop_tests;

impl<T: RelayTransport> LocalHost<'_, T> {
    pub(super) async fn authority(
        &self,
        scope: &Scope,
    ) -> Result<crate::authority::ProjectedAuthority, Error> {
        crate::authority::fetch_projected_authority(
            self.client,
            &scope.channel.to_string(),
            &scope.genesis.id.to_hex(),
            &owner(self.client)?,
        )
        .await
    }
    async fn rows(&self, scope: &Scope) -> Result<Vec<Value>, Error> {
        self.client
            .query_all(json!({"kinds":[COMMAND,RECEIPT,METADATA],"#h":[scope.channel.to_string()]}))
            .await
    }
    fn creates(&self, scope: &Scope, rows: &[Value]) -> Vec<Event> {
        rows.iter().filter_map(|v|serde_json::from_value::<Event>(v.clone()).ok()).filter(|e|{
            if e.kind.as_u16() as u32!=COMMAND || e.pubkey!=self.client.keys().public_key() || e.verify().is_err(){return false}
            let Ok(p)=decode_coding_session_lifecycle_command(&e.content) else{return false};
            tags(e,&[("h",&scope.channel.to_string()),("csl-v",beekeeper_core::coding_session_lifecycle_command::CODING_SESSION_LIFECYCLE_COMMAND_TAG_VERSION),("csl-command",&p.command_id)]) &&
                matches!(p.action,Action::SessionCreate{genesis_ref:Some(g),project_ref:Some(project),..} if g==scope.genesis.id.to_hex() && project==scope.project)
        }).collect()
    }
    pub(super) async fn verify_umbrella(&self, scope: &Scope) -> Result<(), Error> {
        let rows = self.rows(scope).await?;
        let session = beekeeper_core::coding_session_genesis::decode_coding_session_genesis(
            &scope.genesis.content,
        )
        .map_err(fail)?
        .session_ref;
        for event in self.creates(scope, &rows) {
            let p = decode_coding_session_lifecycle_command(&event.content).map_err(fail)?;
            let Action::SessionCreate {
                provider_authority_pubkey,
                session_ref: Some(s),
                ..
            } = p.action
            else {
                continue;
            };
            if s != session {
                continue;
            }
            if let Some(CreateWaitOutcome::Confirmed { target, .. }) = classify_create_receipts(
                &rows,
                &scope.channel.to_string(),
                &p.command_id,
                &provider_authority_pubkey,
            )
            .map_err(fail)?
            {
                if rows
                    .iter()
                    .filter_map(|v| serde_json::from_value::<Event>(v.clone()).ok())
                    .any(|e| {
                        e.pubkey.to_hex() == provider_authority_pubkey
                            && metadata(&e, &scope.channel.to_string()).is_some_and(|m| {
                                m.session == target
                                    && m.session_ref.as_deref() == Some(&session)
                                    && m.project_ref.as_deref() == Some(&scope.project)
                            })
                    })
                {
                    return Ok(());
                }
            }
        }
        Err(Error::Refused(
            "hire scope has no owner-created receipt-backed project umbrella".into(),
        ))
    }
    pub(super) async fn requester_target(
        &self,
        scope: &Scope,
        request: &Event,
    ) -> Result<CodingSessionTarget, Error> {
        let rows = self.rows(scope).await?;
        let mut creates = self.creates(scope, &rows);
        creates.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(a.id.cmp(&b.id)));
        for event in creates {
            let p = decode_coding_session_lifecycle_command(&event.content).map_err(fail)?;
            let Action::SessionCreate {
                actor,
                provider_authority_pubkey,
                ..
            } = p.action
            else {
                continue;
            };
            let requester = request.pubkey.to_hex();
            if actor.as_deref() != Some(&requester)
                && !(actor.is_none() && request.pubkey == self.client.keys().public_key())
            {
                continue;
            }
            if let Some(CreateWaitOutcome::Confirmed { target, .. }) = classify_create_receipts(
                &rows,
                &scope.channel.to_string(),
                &p.command_id,
                &provider_authority_pubkey,
            )
            .map_err(fail)?
            {
                if stopped(
                    &rows,
                    &scope.channel.to_string(),
                    &self.client.keys().public_key().to_hex(),
                    &provider_authority_pubkey,
                    &target,
                ) {
                    continue;
                }
                if rows
                    .iter()
                    .filter_map(|v| serde_json::from_value::<Event>(v.clone()).ok())
                    .any(|e| {
                        e.pubkey.to_hex() == provider_authority_pubkey
                            && metadata(&e, &scope.channel.to_string()).is_some_and(|m| {
                                m.session == target
                                    && m.project_ref.as_deref() == Some(&scope.project)
                            })
                    })
                {
                    return Ok(target);
                }
            }
        }
        Err(Error::Unconfirmed(
            "requester has no receipt-backed execution for refusal delivery".into(),
        ))
    }
    pub(crate) async fn runtime(&self, scope: &Scope) -> Result<(), Error> {
        let config = host(self.context, self.client)?;
        let runtime = config
            .runtimes
            .iter()
            .find(|r| {
                r.instance_ref == self.provider_instance && r.allowed_models.contains(&self.model)
            })
            .ok_or_else(|| {
                Error::Refused(
                    "HIRE_PROVIDER_NOT_ALLOWED: runtime/model no longer commissioned".into(),
                )
            })?;
        let mut events:Vec<Event>=self.client.query_all(json!({"kinds":[CATALOG],"#h":[scope.channel.to_string()],"authors":[config.provider_pubkey]})).await?.into_iter().filter_map(|v|serde_json::from_value::<Event>(v).ok()).filter(|e|e.pubkey.to_hex()==config.provider_pubkey && e.kind.as_u16() as u32==CATALOG && e.verify().is_ok()).collect();
        events.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(a.id.cmp(&b.id)));
        for e in events {
            let Ok(c) = beekeeper_core::coding_session_catalog::parse_catalog(&e.content) else {
                continue;
            };
            if !tags(
                &e,
                &[
                    ("h", &scope.channel.to_string()),
                    ("cspc-v", CODING_SESSION_PROVIDER_CATALOG_TAG_VERSION),
                    ("cspc-revision", &c.revision.to_string()),
                    (
                        "cspc-key",
                        &coding_session_provider_catalog_semantic_key(
                            &scope.channel.to_string(),
                            c.revision,
                            &e.content,
                        ),
                    ),
                ],
            ) {
                continue;
            }
            if !c.projects.is_empty()
                && !c.projects.iter().any(|p| {
                    p.project_ref == scope.project && p.providers.contains(&self.provider_instance)
                })
            {
                return Err(Error::Refused(
                    "HIRE_PROVIDER_NOT_ALLOWED: catalog excludes project".into(),
                ));
            }
            if c.providers.iter().any(|p| {
                p.provider_instance_ref == self.provider_instance
                    && p.runtime == runtime.runtime
                    && p.allowed_models.contains(&self.model)
            }) {
                return Ok(());
            }
            return Err(Error::Refused(
                "HIRE_MODEL_NOT_OFFERED: current signed catalog does not offer selected runtime/model"
                    .into(),
            ));
        }
        Err(Error::Unconfirmed(
            "HIRE_CATALOG_UNCONFIRMED: no verified usable catalog observed; retry when available"
                .into(),
        ))
    }
    pub(super) async fn active_seats(&self, scope: &Scope) -> Result<Vec<String>, Error> {
        let rows = self.rows(scope).await?;
        let mut actors = std::collections::BTreeSet::new();
        for event in self.creates(scope, &rows) {
            let p = decode_coding_session_lifecycle_command(&event.content).map_err(fail)?;
            let Action::SessionCreate {
                actor: Some(actor),
                provider_authority_pubkey,
                ..
            } = p.action
            else {
                continue;
            };
            let outcome = classify_create_receipts(
                &rows,
                &scope.channel.to_string(),
                &p.command_id,
                &provider_authority_pubkey,
            )
            .map_err(fail)?;
            match outcome {
                Some(CreateWaitOutcome::Refused { error, .. })
                    if error.code
                        != beekeeper_core::coding_session_payload::CREATE_START_OUTCOME_UNCONFIRMED
                        && error.code != "SESSION_QUIESCENCE_UNCONFIRMED" =>
                {
                    continue
                }
                Some(CreateWaitOutcome::Confirmed { target, .. }) => {
                    if stopped(
                        &rows,
                        &scope.channel.to_string(),
                        &self.client.keys().public_key().to_hex(),
                        &provider_authority_pubkey,
                        &target,
                    ) {
                        continue;
                    }
                    let mut matches: Vec<_> = rows
                        .iter()
                        .filter_map(|v| serde_json::from_value::<Event>(v.clone()).ok())
                        .filter(|e| e.pubkey.to_hex() == provider_authority_pubkey)
                        .filter_map(|e| {
                            metadata(&e, &scope.channel.to_string())
                                .filter(|m| m.session == target)
                                .map(|m| (e, m))
                        })
                        .collect();
                    matches.sort_by(|(a, _), (b, _)| {
                        b.created_at.cmp(&a.created_at).then(a.id.cmp(&b.id))
                    });
                    if matches.first().is_some_and(|(_, m)| {
                        matches!(m.status, SessionStatus::Stopped | SessionStatus::Completed)
                    }) {
                        continue;
                    }
                }
                _ => {}
            }
            actors.insert(actor);
        }
        Ok(actors.into_iter().collect())
    }
    async fn evidence(
        &self,
        scope: &Scope,
        plan: &SeatPlan,
        target: &CodingSessionTarget,
    ) -> Result<(), Error> {
        let mut rows = self.rows(scope).await?;
        rows.push(serde_json::to_value(&scope.genesis).map_err(fail)?);
        let p = decode_coding_session_lifecycle_command(&plan.create.content).map_err(fail)?;
        let Action::SessionCreate {
            actor: Some(actor),
            role: Some(role),
            session_ref: Some(session_ref),
            genesis_ref: Some(genesis_ref),
            provider_authority_pubkey,
            provider_instance_ref,
            model,
            ..
        } = p.action
        else {
            return Err(fail("saved create incomplete"));
        };
        let seat = HiredSeat {
            event_id: plan.create.id.to_hex(),
            create_signer: plan.create.pubkey.to_hex(),
            command_id: p.command_id.clone(),
            actor,
            role,
            session_ref,
            genesis_ref,
            provider_authority_pubkey: provider_authority_pubkey.clone(),
            provider_instance_ref,
            model,
            at: plan.create.created_at.as_secs() as i64,
            raw: serde_json::to_value(&plan.create).map_err(fail)?,
        };
        let Some(CreateWaitOutcome::Confirmed {
            receipt_event_id, ..
        }) = classify_create_receipts(
            &rows,
            &scope.channel.to_string(),
            &p.command_id,
            &provider_authority_pubkey,
        )
        .map_err(fail)?
        else {
            return Err(Error::Unconfirmed("create receipt unavailable".into()));
        };
        let raw = rows
            .iter()
            .find(|v| v["id"] == receipt_event_id)
            .ok_or_else(|| fail("receipt missing"))?
            .clone();
        let e: Event = serde_json::from_value(raw.clone()).map_err(fail)?;
        let r: LifecycleReceipt = serde_json::from_str(&e.content).map_err(fail)?;
        if r.session.as_ref() != Some(target) {
            return Err(Error::Conflict("saved receipt target changed".into()));
        }
        let receipt = SeatReceipt {
            event_id: receipt_event_id,
            signer: e.pubkey.to_hex(),
            status: r.status,
            target_key: Some(coding_session_target_key(target)),
            error_code: r.error.as_ref().map(|e| e.code.clone()),
            error_message: r.error.map(|e| e.message),
            at: e.created_at.as_secs() as i64,
            raw,
        };
        verify_hire_evidence(
            &rows,
            &HireEvidenceRequest {
                channel: &scope.channel.to_string(),
                session_ref: &seat.session_ref,
                genesis: &scope.genesis.id.to_hex(),
                role: &seat.role,
                provider_instance: Some(&self.provider_instance),
            },
            &seat,
            &receipt,
        )?;
        let mut matching: Vec<_> = rows
            .iter()
            .filter_map(|v| serde_json::from_value::<Event>(v.clone()).ok())
            .filter(|e| e.pubkey.to_hex() == provider_authority_pubkey)
            .filter_map(|e| {
                metadata(&e, &scope.channel.to_string())
                    .filter(|m| m.session == *target)
                    .map(|m| (e, m))
            })
            .collect();
        matching.sort_by(|(a, _), (b, _)| b.created_at.cmp(&a.created_at).then(a.id.cmp(&b.id)));
        let (_, m) = matching
            .first()
            .ok_or_else(|| Error::Unconfirmed("metadata unavailable".into()))?;
        if m.pack_ref.as_ref() != Some(&plan.pack_ref)
            || m.project_ref.as_deref() != Some(&scope.project)
            || m.model.as_deref() != Some(&self.model)
        {
            return Err(Error::Refused(
                "provider metadata does not confirm planned role provenance/project/model".into(),
            ));
        }
        if stopped(
            &rows,
            &scope.channel.to_string(),
            &self.client.keys().public_key().to_hex(),
            &provider_authority_pubkey,
            target,
        ) || matches!(
            m.status,
            SessionStatus::Stopped | SessionStatus::Completed | SessionStatus::Failed
        ) {
            return Err(Error::Refused(
                "created responsibility is terminal; no authority will be regranted".into(),
            ));
        }
        Ok(())
    }
    pub(super) async fn ready_grants(
        &self,
        scope: &Scope,
        plan: &SeatPlan,
        target: &CodingSessionTarget,
    ) -> Result<bool, Error> {
        self.evidence(scope, plan, target).await?;
        let a = self.authority(scope).await?;
        Ok(a.grants
            .iter()
            .any(|g| g.actor_pubkey == self.actor && g.may_steer)
            && a.seats
                .iter()
                .any(|s| s.actor_pubkey == self.actor && s.role == plan.pack_ref.role))
    }
    pub(super) async fn missing_grant(
        &self,
        scope: &Scope,
        plan: &SeatPlan,
        target: &CodingSessionTarget,
    ) -> Result<Vec<Event>, Error> {
        self.evidence(scope, plan, target).await?;
        let a = self.authority(scope).await?;
        let seq = a
            .head_seq
            .checked_add(1)
            .ok_or_else(|| fail("authority sequence overflow"))?;
        use beekeeper_core::coding_session_authority_transition::CodingSessionAuthorityTransitionPayload as Grant;
        let payload = if !a
            .grants
            .iter()
            .any(|g| g.actor_pubkey == self.actor && g.may_steer)
        {
            Grant::new_grant_operator(scope.genesis.id.to_hex(), a.head_event_id, seq, &self.actor)
        } else if !a
            .seats
            .iter()
            .any(|s| s.actor_pubkey == self.actor && s.role == plan.pack_ref.role)
        {
            if a.seats.iter().any(|s| s.actor_pubkey == self.actor) {
                return Err(Error::Refused(
                    "actor already holds a different accepted role".into(),
                ));
            }
            Grant::new_grant_seat(
                scope.genesis.id.to_hex(),
                a.head_event_id,
                seq,
                &self.actor,
                &plan.pack_ref.role,
            )
        } else {
            return Ok(vec![]);
        };
        Ok(vec![self.client.sign_event_unchecked(
            beekeeper_sdk::build_coding_session_authority_transition(scope.channel, &payload)
                .map_err(fail)?,
        )?])
    }
}
