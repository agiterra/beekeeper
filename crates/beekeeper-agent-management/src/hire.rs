//! Durable owner-side hire answering. Execution and stop authority remain with
//! the existing provider. An uncertain publication never releases a reservation.
use crate::management::local::{fail, operation_path, owner, read_operation, save, Operation};
use crate::{Context, Error, RelayTransport};
use beekeeper_core::coding_session_command::CodingSessionTarget;
use beekeeper_core::coding_session_lifecycle_command::{
    decode_coding_session_lifecycle_command, CodingSessionLifecycleAction,
};
use beekeeper_core::coding_session_payload::PackRef;
use nostr::Event;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::PathBuf;
use uuid::Uuid;

/// Explicit isolated cutover scope. This is not a distributed responder claim.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Scope {
    /// Project whose canonical association must be checked by the adapter.
    pub project: String,
    /// One channel, never global discovery.
    pub channel: Uuid,
    /// Exact signed genesis, not an umbrella label lookup.
    pub genesis: Event,
}

/// Immutable chosen material, saved before cutting a tree or staging custody.
/// No secret, private prompt, or worker output belongs in this record.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SeatPlan {
    /// Exact owner-signed create. Retries never refresh its timestamp.
    pub create: Event,
    /// Canonical owner-signed managed definition revision.
    pub definition_revision: String,
    /// Protected configuration snapshot used for selection.
    pub private_snapshot_hash: String,
    /// Exact creator-signed 30624 used for this role.
    pub source_event_id: String,
    /// Exact composed role provenance, also staged for provider metadata.
    pub pack_ref: PackRef,
    /// Trusted project checkout, never the listener's incidental cwd.
    pub checkout: PathBuf,
    /// Exact code commit from which this responsibility is cut.
    pub checkout_revision: String,
    /// Fixed seat tree reserved by this request.
    pub worktree: PathBuf,
    /// Immutable composed pack outside the code checkout.
    pub pack_dir: PathBuf,
    /// Fixed sibling agents clone, if the role grants it.
    pub agents_checkout: Option<PathBuf>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Progress {
    plan: SeatPlan,
    staged: bool,
    publication_attempted: bool,
    #[serde(default)]
    refusal: Option<Refusal>,
    #[serde(default = "initial_grant_cursor")]
    grant_cursor: usize,
    #[serde(default)]
    completed: bool,
}

fn initial_grant_cursor() -> usize {
    1
}

#[derive(Clone, Serialize, Deserialize)]
struct Refusal {
    receipt_event_id: String,
    error: beekeeper_core::coding_session_payload::ReceiptError,
}

/// Stable full-SHA256 key scoped by relay, owner, channel, genesis and the
/// original protocol command ID. Wire IDs are never rewritten or narrowed.
pub fn journal_path(
    context: &Context,
    relay: &str,
    owner: &str,
    scope: &Scope,
    command: &str,
) -> Result<PathBuf, Error> {
    use sha2::{Digest, Sha256};
    let namespace = serde_json::to_vec(&json!([
        "hire/v1",
        relay,
        owner,
        scope.channel,
        scope.genesis.id,
        command
    ]))
    .map_err(fail)?;
    let directory = operation_path(context, Uuid::nil())?;
    Ok(directory.with_file_name(format!(
        "hire-{}.json",
        hex::encode(Sha256::digest(namespace))
    )))
}

/// Observable facts, deliberately distinguishing create from ready authority.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Disposition {
    /// Durable terminal owner decision; never becomes a create on later replay.
    PolicyRefused {
        /// Stable public refusal code.
        code: String,
        /// Public reason, also carried by the requester-facing turn.
        reason: String,
        /// Exact signed disclosure, if a requester target has been resolved.
        disclosure_event_id: Option<String>,
        /// Whether relay acceptance of that exact disclosure was established.
        disclosure_confirmed: bool,
    },
    /// Provider conclusively refused this create; the old request remains answered.
    Refused {
        /// Signed provider refusal.
        receipt_event_id: String,
        /// Public provider reason.
        error: beekeeper_core::coding_session_payload::ReceiptError,
    },
    /// Published or attempted, without a verified provider answer.
    PublicationUnconfirmed {
        /// Stable create event ID.
        create_event_id: String,
    },
    /// Provider created the seat; authority repair is still required.
    CreatedAwaitingGrants {
        /// Exact provider-admitted target, never an inferred generation.
        target: CodingSessionTarget,
    },
    /// Receipt and both operator/role grants have been verified.
    Ready {
        /// Exact provider-admitted target.
        target: CodingSessionTarget,
        /// Verified provider receipt ID.
        receipt_event_id: String,
    },
}

/// Canonical environment adapters. Implementations must use current managed
/// definitions/private snapshots, trusted projects and the existing authority
/// projection, never Desktop's legacy agent store or an attestation as authority.
#[allow(async_fn_in_trait)]
pub trait Adapter {
    /// Verify the request signer against the founder/accepted operator grants
    /// and the receipt-backed umbrella's project. Called before any effect.
    async fn authorize(&self, scope: &Scope, request: &Event) -> Result<(), Error>;
    /// Resolve the existing requester execution for the ordinary 44220 host answer.
    /// Missing evidence is retryable; a terminal decision remains durable meanwhile.
    async fn refusal_target(
        &self,
        _scope: &Scope,
        _request: &Event,
    ) -> Result<CodingSessionTarget, Error> {
        Err(Error::Unconfirmed("requester target unavailable".into()))
    }
    /// Select once, including policy, catalog/runtime, role and trusted checkout.
    /// Must count `pending` reservations as live selections and be read-only
    /// apart from immutable role-cache materialization. `Error::Refused` is a
    /// terminal authorized decision, journaled and disclosed before effects;
    /// transport/storage uncertainty must use `Unconfirmed`, `Relay` or `Other`.
    async fn plan(
        &self,
        scope: &Scope,
        request: &Event,
        pending: &[SeatPlan],
    ) -> Result<SeatPlan, Error>;
    /// Recheck current canonical association, definition, runtime, source pin and
    /// checkout against the saved selection. Never choose replacement material.
    async fn revalidate(&self, scope: &Scope, plan: &SeatPlan) -> Result<(), Error>;
    /// Idempotently prepare the fixed tree/agents clone and stage canonical
    /// actor custody and role pack under the create's exact command ID.
    async fn stage(&self, scope: &Scope, plan: &SeatPlan) -> Result<(), Error>;
    /// Build missing owner authority transitions from verified receipt-backed
    /// projection. Returned signed bytes are journaled before submission.
    async fn grants(
        &self,
        scope: &Scope,
        plan: &SeatPlan,
        target: &CodingSessionTarget,
    ) -> Result<Vec<Event>, Error>;
    /// Whether canonical accepted-chain evidence proves this saved grant was
    /// accepted or can never be accepted at its original sequence. An unknown
    /// publish outcome alone must return false. Exact old bytes remain journaled.
    async fn grant_reconciled(&self, _scope: &Scope, _event: &Event) -> Result<bool, Error> {
        Ok(false)
    }
    /// Verify accepted operator AND role grants. A transport ACK is insufficient.
    async fn grants_ready(
        &self,
        scope: &Scope,
        plan: &SeatPlan,
        target: &CodingSessionTarget,
    ) -> Result<bool, Error>;
}

fn scope_lock(context: &Context, binding: &str) -> Result<std::fs::File, Error> {
    use sha2::{Digest, Sha256};
    let dir = beekeeper_host_core::layout::host_dir(&context.home, context.instance)
        .join("management/hire-locks");
    beekeeper_host_core::atomic_write::create_dir_all_restricted(&dir).map_err(fail)?;
    let path = dir.join(hex::encode(Sha256::digest(binding.as_bytes())));
    if std::fs::symlink_metadata(&path).is_ok_and(|m| !m.file_type().is_file()) {
        return Err(Error::Refused("hire lock is not a regular file".into()));
    }
    let mut options = std::fs::OpenOptions::new();
    options.create(true).read(true).write(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path).map_err(fail)?;
    // Do not block an async executor or serialize other umbrellas/identities.
    file.try_lock()
        .map_err(|_| Error::Unconfirmed("hire umbrella is being reconciled locally".into()))?;
    Ok(file)
}

/// Reconcile one request through the existing restricted operation journal.
/// The umbrella lock excludes local duplicate selection; no management/global
/// actor lock is held while waiting for relay receipts. Legacy clients must be
/// excluded by explicit cutover; this does not claim cross-machine safety.
pub async fn answer(
    context: &Context,
    client: &impl RelayTransport,
    adapter: &impl Adapter,
    scope: &Scope,
    request: &Event,
    wait_secs: u64,
) -> Result<Disposition, Error> {
    let owner = owner(client)?;
    let payload = validate_scope(scope, request, &owner)?;
    if !(1..=300).contains(&wait_secs) {
        return Err(Error::Usage(
            "hire receipt wait must be 1..300 seconds".into(),
        ));
    }
    adapter.authorize(scope, request).await?;
    let relay =
        beekeeper_host_core::record::canonical_relay_key(&crate::to_ws_url(client.relay_url()));
    let umbrella_binding = format!("{relay}:{owner}:{}:{}", scope.channel, scope.genesis.id);
    let request_lock = scope_lock(
        context,
        &format!("request:{umbrella_binding}:{}", payload.command_id),
    )?;
    let selection_lock = scope_lock(context, &format!("selection:{umbrella_binding}"))?;
    let binding =
        json!({"type":"hire","owner":owner,"relay":relay,"scope":scope,"request":request});
    let path = journal_path(context, &relay, &owner, scope, &payload.command_id)?;
    let mut operation = match read_operation(&path, &binding)? {
        Some(held) => held,
        None => {
            let pending = pending_plans(&path, &binding)?;
            let plan = match adapter.plan(scope, request, &pending).await {
                Ok(plan) => plan,
                Err(Error::Refused(reason)) => {
                    let mut operation = crate::hire_refusal::operation(binding, reason);
                    save(&path, &operation)?;
                    drop(selection_lock);
                    return crate::hire_refusal::reconcile(
                        &path,
                        &mut operation,
                        client,
                        adapter,
                        scope,
                        request,
                    )
                    .await;
                }
                // Auth, transport/storage uncertainty and invalid configuration
                // are not terminal policy decisions and must not be mislabeled.
                Err(error) => return Err(error),
            };
            let actor = validate_plan(scope, request, &plan, &owner)?;
            for held in &pending {
                if validate_plan_actor(held)? == actor {
                    let mut operation = crate::hire_refusal::operation(
                        binding,
                        "HIRE_ROLE_BUSY: this actor has a pending hire in the umbrella".into(),
                    );
                    save(&path, &operation)?;
                    drop(selection_lock);
                    return crate::hire_refusal::reconcile(
                        &path,
                        &mut operation,
                        client,
                        adapter,
                        scope,
                        request,
                    )
                    .await;
                }
            }
            let operation = Operation {
                request: binding,
                actor_pubkey: actor,
                events: vec![plan.create.clone()],
                target: None,
                receipt_event_id: None,
                predecessor: None,
                custody_handoff: None,
                hire_decision: None,
                hire: Some(Progress {
                    plan,
                    staged: false,
                    publication_attempted: false,
                    refusal: None,
                    grant_cursor: 1,
                    completed: false,
                }),
            };
            // Interoperate with existing management operations sharing UUIDs.
            let _management = crate::management::local::management_lock(context)?;
            if read_operation(&path, &operation.request)?.is_some() {
                return Err(Error::Conflict(
                    "operation appeared while planning; retry unchanged".into(),
                ));
            }
            save(&path, &operation)?;
            operation
        }
    };
    drop(selection_lock);
    let _request_lock = request_lock;
    if operation.hire_decision.is_some() {
        return crate::hire_refusal::reconcile(
            &path,
            &mut operation,
            client,
            adapter,
            scope,
            request,
        )
        .await;
    }
    let progress = operation
        .hire
        .clone()
        .ok_or_else(|| Error::Conflict("operation is not a hire".into()))?;
    let plan = &progress.plan;
    if validate_plan(scope, request, plan, &owner)? != operation.actor_pubkey
        || operation.events.first() != Some(&plan.create)
    {
        return Err(Error::Conflict(
            "saved hire identity or signed create differs from its reservation".into(),
        ));
    }
    let create = decode_coding_session_lifecycle_command(&plan.create.content).map_err(fail)?;
    let CodingSessionLifecycleAction::SessionCreate {
        provider_authority_pubkey,
        ..
    } = &create.action
    else {
        return Err(Error::Conflict("saved hire has no seated create".into()));
    };
    if operation.target.is_none() && progress.publication_attempted {
        // Observe an already admitted create before checking whether its old
        // selection could still be made today. Configuration drift must not
        // turn grant repair into either rehire or a permanently hidden receipt.
        let rows = client
            .query_all(json!({
                "kinds":[beekeeper_core::kind::KIND_CODING_SESSION_LIFECYCLE_RECEIPT],
                "#h":[scope.channel.to_string()], "#csl-command":[create.command_id]
            }))
            .await?;
        if let Some(outcome) = crate::receipts::classify_create_receipts(
            &rows,
            &scope.channel.to_string(),
            &create.command_id,
            provider_authority_pubkey,
        )
        .map_err(Error::Conflict)?
        {
            observe_receipt(&path, &mut operation, outcome)?;
        }
    }
    if let Some(refusal) = operation.hire.as_ref().and_then(|p| p.refusal.as_ref()) {
        return Ok(Disposition::Refused {
            receipt_event_id: refusal.receipt_event_id.clone(),
            error: refusal.error.clone(),
        });
    }
    if operation.target.is_none() {
        adapter.authorize(scope, request).await?;
        if let Err(error) = adapter.revalidate(scope, plan).await {
            if !progress.publication_attempted {
                if let Error::Refused(reason) = error {
                    operation.hire_decision = Some(crate::hire_refusal::Decision::new(reason));
                    save(&path, &operation)?;
                    return crate::hire_refusal::reconcile(
                        &path,
                        &mut operation,
                        client,
                        adapter,
                        scope,
                        request,
                    )
                    .await;
                }
            }
            return Err(error);
        }
        if !progress.staged {
            if progress.publication_attempted {
                return Err(Error::Unconfirmed(
                    "publication may have begun; refusing to reset incomplete preparation".into(),
                ));
            }
            adapter.stage(scope, plan).await?;
            if let Some(state) = operation.hire.as_mut() {
                state.staged = true;
            }
            save(&path, &operation)?;
        }
        if let Some(state) = operation.hire.as_mut() {
            state.publication_attempted = true;
        }
        save(&path, &operation)?;
        // A failed submission may have reached the relay. Observe the same
        // command regardless; never clear custody, choose again or re-sign.
        let _publication = crate::management::submit(client, &plan.create).await;
        let outcome = crate::receipts::await_create_receipt(
            client,
            &scope.channel.to_string(),
            &create.command_id,
            provider_authority_pubkey,
            wait_secs,
        )
        .await;
        observe_receipt(&path, &mut operation, outcome)?;
        if let Some(refusal) = operation.hire.as_ref().and_then(|p| p.refusal.as_ref()) {
            return Ok(Disposition::Refused {
                receipt_event_id: refusal.receipt_event_id.clone(),
                error: refusal.error.clone(),
            });
        }
        if operation.target.is_none() {
            return Ok(Disposition::PublicationUnconfirmed {
                create_event_id: plan.create.id.to_hex(),
            });
        }
    }
    let target = operation
        .target
        .clone()
        .ok_or_else(|| fail("hire receipt target missing"))?;
    if operation.hire.as_ref().is_some_and(|p| p.completed) {
        return ready(&operation, target);
    }
    // Build only the next transition from the accepted chain. In particular,
    // a grant conflict repairs authority for this same create; it never rehires.
    for _ in 0..4 {
        if adapter
            .grants_ready(scope, plan, &target)
            .await
            .unwrap_or(false)
        {
            if let Some(progress) = operation.hire.as_mut() {
                progress.completed = true;
            }
            save(&path, &operation)?;
            return ready(&operation, target);
        }
        let cursor = operation.hire.as_ref().map(|p| p.grant_cursor).unwrap_or(1);
        if cursor > operation.events.len() || cursor == 0 {
            return Err(Error::Conflict("hire grant journal cursor invalid".into()));
        }
        if cursor == operation.events.len() {
            let grants = match adapter.grants(scope, plan, &target).await {
                Ok(g) => g,
                Err(_) => return Ok(Disposition::CreatedAwaitingGrants { target }),
            };
            if grants.is_empty() {
                return Ok(Disposition::CreatedAwaitingGrants { target });
            }
            for grant in &grants {
                validation::validate_grant(scope, plan, grant, &owner)?;
            }
            operation.events.extend(grants);
            save(&path, &operation)?;
        }
        let event = &operation.events[cursor];
        validation::validate_grant(scope, plan, event, &owner)?;
        if !adapter
            .grant_reconciled(scope, event)
            .await
            .unwrap_or(false)
        {
            let _ack = crate::management::submit(client, event).await;
        }
        if adapter
            .grant_reconciled(scope, event)
            .await
            .unwrap_or(false)
        {
            if let Some(progress) = operation.hire.as_mut() {
                progress.grant_cursor = cursor + 1;
            }
            save(&path, &operation)?;
        } else {
            if adapter
                .grants_ready(scope, plan, &target)
                .await
                .unwrap_or(false)
            {
                if let Some(progress) = operation.hire.as_mut() {
                    progress.completed = true;
                }
                save(&path, &operation)?;
                return ready(&operation, target);
            }
            return Ok(Disposition::CreatedAwaitingGrants { target });
        }
    }
    Ok(Disposition::CreatedAwaitingGrants { target })
}

fn ready(operation: &Operation, target: CodingSessionTarget) -> Result<Disposition, Error> {
    Ok(Disposition::Ready {
        target,
        receipt_event_id: operation
            .receipt_event_id
            .clone()
            .ok_or_else(|| fail("hire receipt ID missing"))?,
    })
}

#[path = "hire_validation.rs"]
mod validation;
use validation::{validate_plan, validate_scope};

#[cfg(test)]
#[path = "hire_tests.rs"]
mod tests;

fn validate_plan_actor(plan: &SeatPlan) -> Result<String, Error> {
    match decode_coding_session_lifecycle_command(&plan.create.content)
        .map_err(fail)?
        .action
    {
        CodingSessionLifecycleAction::SessionCreate {
            actor: Some(actor), ..
        } => Ok(actor),
        _ => Err(Error::Conflict(
            "pending hire is not a seated create".into(),
        )),
    }
}

fn pending_plans(
    path: &std::path::Path,
    binding: &serde_json::Value,
) -> Result<Vec<SeatPlan>, Error> {
    let parent = path
        .parent()
        .ok_or_else(|| fail("operation directory missing"))?;
    let mut pending = Vec::new();
    for entry in std::fs::read_dir(parent).map_err(fail)? {
        let path = entry.map_err(fail)?.path();
        if path.extension().and_then(|p| p.to_str()) != Some("json") {
            continue;
        }
        let Some(value) = crate::management::local::read_operation_value(&path)? else {
            continue;
        };
        // The existing directory also holds lifecycle and remote-start
        // journals. Their schemas remain owned by their existing readers.
        if value["request"]["type"] != "hire" {
            continue;
        }
        let operation: Operation =
            serde_json::from_value(value).map_err(|_| fail("hire reservation is malformed"))?;
        if operation.hire_decision.is_none()
            && operation.target.is_none()
            && ["type", "owner", "relay", "scope"]
                .iter()
                .all(|key| operation.request[key] == binding[key])
        {
            if let Some(progress) = operation.hire {
                if progress.refusal.is_none() {
                    pending.push(progress.plan);
                }
            }
        }
    }
    Ok(pending)
}

fn observe_receipt(
    path: &std::path::Path,
    operation: &mut Operation,
    outcome: crate::receipts::CreateWaitOutcome,
) -> Result<(), Error> {
    use crate::receipts::CreateWaitOutcome;
    match outcome {
        CreateWaitOutcome::Confirmed {
            target,
            receipt_event_id,
            ..
        } => {
            operation.target = Some(target);
            operation.receipt_event_id = Some(receipt_event_id);
            save(path, operation)
        }
        CreateWaitOutcome::Refused {
            receipt_event_id,
            error,
        } => {
            // A failed receipt can describe unresolved physical startup. That
            // is not terminal refusal and must keep the actor reservation.
            if error.code
                == beekeeper_core::coding_session_payload::CREATE_START_OUTCOME_UNCONFIRMED
                || error.code == "SESSION_QUIESCENCE_UNCONFIRMED"
            {
                return Err(Error::Unconfirmed(format!(
                    "{}: {}",
                    error.code, error.message
                )));
            }
            if let Some(progress) = operation.hire.as_mut() {
                progress.refusal = Some(Refusal {
                    receipt_event_id,
                    error,
                });
            }
            save(path, operation)
        }
        CreateWaitOutcome::Conflicted { detail } => Err(Error::Conflict(detail)),
        CreateWaitOutcome::Unconfirmed { .. } => Ok(()),
    }
}

/// Recover exact signed unresolved requests from the existing operation journal.
/// This read does not stage custody, sign events or infer a new request ID.
pub fn pending_requests(
    context: &Context,
    client: &impl RelayTransport,
    scope: &Scope,
) -> Result<Vec<Event>, Error> {
    let directory = operation_path(context, Uuid::nil())?;
    let parent = directory
        .parent()
        .ok_or_else(|| fail("operation directory missing"))?;
    let owner = owner(client)?;
    let relay =
        beekeeper_host_core::record::canonical_relay_key(&crate::to_ws_url(client.relay_url()));
    let mut requests = Vec::new();
    for entry in std::fs::read_dir(parent).map_err(fail)? {
        let path = entry.map_err(fail)?.path();
        if path.extension().and_then(|p| p.to_str()) != Some("json") {
            continue;
        }
        let Some(value) = crate::management::local::read_operation_value(&path)? else {
            continue;
        };
        if value["request"]["type"] != "hire"
            || value["request"]["owner"] != owner
            || value["request"]["relay"] != relay
            || value["request"]["scope"] != serde_json::to_value(scope).map_err(fail)?
        {
            continue;
        }
        let operation: Operation = serde_json::from_value(value).map_err(fail)?;
        if operation
            .hire_decision
            .as_ref()
            .is_some_and(|d| !d.acknowledged)
            || (operation.hire_decision.is_none()
                && operation
                    .hire
                    .as_ref()
                    .is_some_and(|p| !p.completed && p.refusal.is_none()))
        {
            let event: Event =
                serde_json::from_value(operation.request["request"].clone()).map_err(fail)?;
            validate_scope(scope, &event, &owner)?;
            requests.push(event);
        }
    }
    Ok(requests)
}
