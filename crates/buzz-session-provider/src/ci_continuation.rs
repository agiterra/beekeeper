//! Admission, delivery, expiry and recovery for CI-managed continuations.
//!
//! # The guarantee, stated precisely
//!
//! For one exact target (driver, instance, session, generation) and one CI
//! correlation digest, this provider admits **at most one** continuation turn,
//! durably, across restarts, duplicate result events, reconnect replays and any
//! number of registration command ids. The enforcement is not new: the turn is
//! rebuilt as an ordinary `thread.turn.start` whose *operation pointer* is
//! [`ci_continuation_pointer`], and the existing operation ledger
//! (`operations.jsonl`, first writer wins at `TurnStarted`) is what makes it
//! once. Two command ids naming one digest converge on that one key; the second
//! is refused `DUPLICATE_OPERATION` and spends no turn.
//!
//! It does **not** promise exactly-once external effects of a model turn whose
//! host crashes. CI start permission is durably claimed before the actor may
//! send its prompt: a crash after that claim can lose execution, but cannot
//! admit it twice. Downstream work still needs ordinary idempotence.
//!
//! # The order every write is in, and why
//!
//! - **Registration:** the durable record is written *before* the
//!   `continuation_registered` receipt is enqueued. A crash between them costs
//!   an acknowledgement, never a promise; the sender re-runs the identical
//!   command and is re-acknowledged.
//! - **Delivery:** the rebuilt turn goes through the *same*
//!   [`crate::commands::decide_turn_command`] every other turn does, so
//!   authority, generation, closure, budget, the mailbox and the operation
//!   fence are re-evaluated **now** rather than at registration time. A grant
//!   revoked while CI was running refuses the turn; a regrant does not
//!   resurrect it.
//! - **Start permission:** at the actor's execution boundary the provider
//!   repeats admission, writes operation then command ledgers, and retires the
//!   record before allowing any prompt. A crash after these writes can lose
//!   execution; recovery never grants this operation twice.
//! - **Refusal:** the refusal ledger, then the outbox, then the terminal
//!   record. The retained terminal record is what stops a relay redelivery of
//!   the registration from resurrecting a refused promise.

use tokio::sync::mpsc;
use uuid::Uuid;

use buzz_core::ci_result::{CiConclusion, CiResult, CiResultIdentity};
use buzz_core::coding_session_command::{ci_continuation_pointer, CodingSessionTarget};
use serde::Serialize;

use crate::ci_continuation_store::{
    CiContinuationRecord, ReadyResult, RecordState, RelayObservation,
};
use crate::ci_result_listener::{CiListenerEvent, CiResultListener, ListenerConfig};
use crate::commands::{decide_turn_command, TurnAction, TurnCommand, TurnDecision};
use crate::payload::{
    LifecycleReceipt, CI_CONTINUATION_EXPIRED, CI_RESULT_CONFLICT, CI_RESULT_UNAVAILABLE_OR_HIDDEN,
    COMMAND_ID_CONFLICT,
};
use crate::state::now_secs;
use crate::{commands::ProjectsFile, Provider, TurnDisposition};

/// The requested CI project is not the execution's recorded project.
pub const CI_CONTINUATION_PROJECT_MISMATCH: &str = "CI_CONTINUATION_PROJECT_MISMATCH";

/// Await the next verified-result report, parking forever when there is none.
///
/// A free function over the receiver rather than a `Provider` method, because
/// the run loop's `select!` already borrows the provider mutably for the
/// session inbox and cannot borrow it twice. Once the listener's task is gone
/// the queue closes and this parks, so the arm neither fires nor spins.
pub async fn next_ci_listener_event(
    events: &mut Option<mpsc::Receiver<CiListenerEvent>>,
) -> CiListenerEvent {
    let closed = match events {
        Some(queue) => match queue.recv().await {
            Some(event) => return event,
            None => true,
        },
        None => false,
    };
    if closed {
        *events = None;
        tracing::warn!(
            target: "csp::ci",
            "the CI result listener stopped; pending continuations will expire with their \
             observed disposition"
        );
    }
    std::future::pending().await
}

/// How long a `ready` record waits before another delivery attempt when the
/// execution could not take the turn yet.
const DELIVERY_RETRY_SECS: u64 = 30;

/// Everything [`Provider::register_ci_continuation`] needs, gathered by the
/// decision that admitted it.
#[derive(Debug, Clone)]
pub struct Registration {
    /// Channel the registration arrived on.
    pub channel_id: Uuid,
    /// The registering 44220's `commandId`.
    pub command_id: String,
    /// The exact generation the eventual turn will address.
    pub target: CodingSessionTarget,
    /// The exact CI run attempt awaited.
    pub identity: CiResultIdentity,
    /// `correlation_id(identity)`.
    pub correlation_id: String,
    /// Text to deliver alongside the verified result.
    pub continuation: String,
    /// Unix seconds after which the registration is refused.
    pub expires_at: u64,
    /// SHA-256 of the registration event content.
    pub payload_digest: String,
    /// Event id of the signed registration.
    pub registration_event_id: String,
    /// Pubkey that signed it.
    pub signer: String,
}

/// The registration half of the delivered turn's materialized context.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DeliveredRegistration {
    command_id: String,
    signer: String,
    registered_at: u64,
    expires_at: u64,
}

/// The result half of the delivered turn's materialized context.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DeliveredResult {
    event_id: String,
    signer: String,
    observed_at: u64,
    identity: CiResultIdentity,
    conclusion: CiConclusion,
    evidence_url: Option<String>,
    summary: Option<String>,
}

/// Exactly what a continuation turn's prompt is, and nothing else.
///
/// Field order here *is* the wire order: `serde_json` emits struct fields in
/// declaration order, so the pretty-printed body is deterministic without a
/// canonicalisation pass. Optional result fields are emitted as `null` rather
/// than skipped, so the shape a reading agent parses does not change with the
/// content.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DeliveredContinuation {
    #[serde(rename = "type")]
    kind: &'static str,
    operation_id: String,
    registration: DeliveredRegistration,
    result: DeliveredResult,
    continuation: String,
}

/// The `type` every CI continuation prompt and pointer carries.
const CI_RESULT_TYPE: &str = "ci_result";

/// Build the exact prompt a continuation turn delivers.
///
/// Deliberately *not* the operation pointer: the fence stays on the compact
/// `{"operationId":…,"type":"ci_result"}` so duplicate results and alternate
/// command ids converge, while the prompt carries the whole verified fact so
/// the woken agent does not have to fetch anything to act on it.
fn materialize(record: &CiContinuationRecord, ready: &ReadyResult) -> Result<String, String> {
    let result: CiResult = serde_json::from_str(&ready.result_canonical_json)
        .map_err(|error| format!("stored CI result is not decodable: {error}"))?;
    let delivered = DeliveredContinuation {
        kind: CI_RESULT_TYPE,
        operation_id: record.correlation_id.clone(),
        registration: DeliveredRegistration {
            command_id: record.command_id.clone(),
            signer: record.signer.clone(),
            registered_at: record.registered_at,
            expires_at: record.expires_at,
        },
        result: DeliveredResult {
            event_id: ready.result_event_id.clone(),
            signer: ready.result_signer.clone(),
            observed_at: ready.observed_at,
            identity: result.identity,
            conclusion: result.conclusion,
            evidence_url: result.evidence_url,
            summary: result.summary,
        },
        continuation: record.continuation.clone(),
    };
    serde_json::to_string_pretty(&delivered)
        .map_err(|error| format!("continuation prompt could not be encoded: {error}"))
}

impl Provider {
    /// Start the single CI result listener, if a relay identity is known.
    ///
    /// Called once, after the relay connection and the NIP-11 `self`
    /// witnessing. Without that identity there is no trust root for a result
    /// signer, so no listener is started at all: registrations still hold, and
    /// they expire with the honest "this provider never got to look"
    /// disposition rather than delivering an unverified fact.
    pub fn start_ci_result_listener(&mut self) {
        let Some(relay_self) = self.relay_self.clone() else {
            tracing::warn!(
                target: "csp::ci",
                "no relay identity was witnessed, so CI results cannot be verified; CI \
                 continuations will expire unanswered until the provider restarts against a \
                 relay that advertises NIP-11 self"
            );
            return;
        };
        let (listener, events) = CiResultListener::spawn(ListenerConfig::new(
            self.config.relay_url.clone(),
            self.config.keys.clone(),
            self.config.auth_tag.clone(),
            relay_self,
        ));
        self.ci_listener = Some(listener);
        self.ci_events = Some(events);
        self.sync_ci_listener();
    }

    /// Hand the listener's report queue to the run loop.
    pub fn take_ci_result_events(&mut self) -> Option<mpsc::Receiver<CiListenerEvent>> {
        self.ci_events.take()
    }

    /// Tell the listener exactly which identities are still worth watching.
    pub(crate) fn sync_ci_listener(&self) {
        if let Some(listener) = &self.ci_listener {
            listener.watch(self.watchable_ci_identities());
        }
    }

    fn watchable_ci_identities(&self) -> std::collections::BTreeMap<String, CiResultIdentity> {
        self.ci_continuations
            .pending()
            .filter(|record| {
                record.expires_at > now_secs()
                    && self
                        .state
                        .session(&record.target.session_id)
                        .and_then(|session| session.project_ref.as_deref())
                        == Some(record.identity.project.as_str())
            })
            .map(|record| (record.correlation_id.clone(), record.identity.clone()))
            .collect()
    }

    /// Durably register one continuation, then acknowledge it.
    ///
    /// The order is the contract: nothing is acknowledged that is not already
    /// on disk. Idempotent for an exact retry — the record is not rewritten and
    /// the same receipt is re-enqueued under the same semantic key, which the
    /// outbox fences.
    pub(crate) fn register_ci_continuation(
        &mut self,
        registration: Registration,
    ) -> anyhow::Result<TurnDisposition> {
        let now = now_secs();
        let conflict_receipt_key = format!(
            "{}:conflict:{}",
            registration.command_id, registration.registration_event_id
        );
        let record = CiContinuationRecord {
            command_id: registration.command_id.clone(),
            registration_event_id: registration.registration_event_id,
            payload_digest: registration.payload_digest,
            channel_id: registration.channel_id,
            signer: registration.signer,
            target: registration.target.clone(),
            identity: registration.identity,
            correlation_id: registration.correlation_id,
            continuation: registration.continuation,
            expires_at: registration.expires_at,
            registered_at: now,
            relay_answered: false,
            attempts: 0,
            next_check_at: 0,
            state: RecordState::Waiting,
        };
        match self.ci_continuations.insert(record) {
            Ok(_) => {}
            // Lost a race with a record carrying different bytes. The first
            // durable record wins, and the loser is told which fact it is.
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let receipt = LifecycleReceipt::turn_refused(
                    &registration.command_id,
                    &registration.target,
                    COMMAND_ID_CONFLICT,
                    "a different CI continuation is already durably registered under this \
                     commandId; the first durable record wins",
                );
                self.enqueue_receipt_with_outbox_key(
                    registration.channel_id,
                    &registration.command_id,
                    &receipt,
                    Some(&conflict_receipt_key),
                )?;
                return Ok(TurnDisposition::Answered(COMMAND_ID_CONFLICT.to_owned()));
            }
            Err(error) => return Err(error.into()),
        }
        let receipt = LifecycleReceipt::continuation_registered(
            &registration.command_id,
            &registration.target,
        );
        self.enqueue_receipt(registration.channel_id, &registration.command_id, &receipt)?;
        tracing::info!(
            target: "csp::ci",
            command_id = %registration.command_id,
            expires_at = registration.expires_at,
            "registered a CI continuation"
        );
        self.sync_ci_listener();
        Ok(TurnDisposition::Registered)
    }

    /// Apply one listener report to durable state.
    pub(crate) async fn handle_ci_listener_event(
        &mut self,
        event: CiListenerEvent,
    ) -> anyhow::Result<()> {
        match event {
            CiListenerEvent::CiResultsAnswered { digests } => {
                // Not a result. The witnessed fact that separates "CI has not
                // reported" from "this provider never got to look", which is
                // the whole difference between the two expiry dispositions.
                self.ci_continuations.mark_relay_answered(&digests)?;
            }
            CiListenerEvent::CiResultReady {
                digest,
                event_id,
                signer,
                canonical_json,
                observed_at,
            } => {
                let ready = ReadyResult {
                    result_event_id: event_id,
                    result_signer: signer,
                    result_canonical_json: canonical_json,
                    observed_at,
                };
                let moved = self.ci_continuations.mark_ready(&digest, &ready)?;
                if moved.is_empty() {
                    // A duplicate, or a result for a turn that already
                    // started. Neither is a second fact, so neither produces a
                    // turn or a receipt.
                    tracing::debug!(
                        target: "csp::ci",
                        %digest,
                        "a verified CI result matched no waiting registration"
                    );
                    return Ok(());
                }
                self.sync_ci_listener();
                for command_id in moved {
                    self.deliver_ci_continuation(&command_id).await?;
                }
            }
            CiListenerEvent::CiResultConflict { digest } => {
                let contested: Vec<String> = self
                    .ci_continuations
                    .pending()
                    .filter(|record| record.correlation_id == digest)
                    .map(|record| record.command_id.clone())
                    .collect();
                for command_id in contested {
                    self.refuse_ci_continuation(
                        &command_id,
                        CI_RESULT_CONFLICT,
                        "the relay holds more than one distinct result for this exact CI run \
                         attempt, so there is no single fact to deliver",
                        None,
                    )?;
                }
                self.sync_ci_listener();
            }
        }
        Ok(())
    }

    /// Deliver, expire, and retry: one pass over the durable registrations.
    ///
    /// Runs on the provider's existing runtime tick. Every failure is logged
    /// and the pass continues: one record that cannot be answered must not
    /// stop the others from being.
    pub(crate) async fn run_ci_continuation_tick(&mut self) -> anyhow::Result<()> {
        let now = now_secs();
        for command_id in self.ci_continuations.ready_for_delivery(now) {
            if let Err(error) = self.deliver_ci_continuation(&command_id).await {
                tracing::error!(
                    target: "csp::ci",
                    %command_id,
                    "CI continuation delivery failed: {error}"
                );
            }
        }
        for command_id in self.ci_continuations.expired(now) {
            if let Err(error) = self.expire_ci_continuation(&command_id) {
                tracing::error!(
                    target: "csp::ci",
                    %command_id,
                    "CI continuation expiry failed: {error}"
                );
            }
        }
        self.sync_ci_listener();
        Ok(())
    }

    /// Rebuild one ready registration's turn and run it through admission.
    pub(crate) async fn deliver_ci_continuation(&mut self, command_id: &str) -> anyhow::Result<()> {
        let Some(record) = self.ci_continuations.record(command_id).cloned() else {
            return Ok(());
        };
        let RecordState::Ready(ready) = &record.state else {
            return Ok(());
        };
        // Enforce the deadline on listener/recovery delivery too. The actor
        // repeats admission at its execution boundary after a busy queue.
        if record.expires_at <= now_secs() {
            return self.expire_ci_continuation(command_id);
        }
        if self
            .state
            .session(&record.target.session_id)
            .and_then(|session| session.project_ref.as_deref())
            != Some(record.identity.project.as_str())
        {
            return self.refuse_ci_continuation(
                command_id,
                CI_CONTINUATION_PROJECT_MISMATCH,
                "the CI result project no longer matches this execution's recorded project",
                None,
            );
        }
        let text = match materialize(&record, ready) {
            Ok(text) => text,
            // The stored result cannot be turned into a prompt. Terminal and
            // visible: silently retrying a body that will never decode would
            // leave the sender waiting on a turn that can never start.
            Err(reason) => {
                tracing::error!(target: "csp::ci", %command_id, "{reason}");
                return self.refuse_ci_continuation(
                    command_id,
                    CI_RESULT_CONFLICT,
                    "the verified CI result could not be rendered into a turn",
                    None,
                );
            }
        };
        let command = TurnCommand {
            command_id: record.command_id.clone(),
            target: record.target.clone(),
            action: TurnAction::Start {
                text,
                attachments: Vec::new(),
                deliver: buzz_core::coding_session_command::CodingSessionDelivery::Boundary,
            },
            // The fence stays on the pointer, never on the materialized text.
            operation_key: Some(ci_continuation_pointer(&record.correlation_id)),
            payload_digest: record.payload_digest.clone(),
        };
        // `now`, not the registration's `created_at`: this is a turn the
        // provider is minting at this moment, and the registration's own age
        // is bounded by `expires_at` rather than by the command horizon.
        let created_at = now_secs();
        let projects = ProjectsFile::default();
        let actor_seats = crate::actor_seats::ActorSeatsFile::default();
        let decision = decide_turn_command(
            &self.context(record.channel_id, &projects, &actor_seats, &record.signer),
            created_at,
            command,
        );
        let disposition = self
            .apply_turn_decision(
                record.channel_id,
                created_at,
                &record.signer,
                &record.registration_event_id,
                decision,
            )
            .await?;
        match disposition {
            // Queue custody is provisional. The actor requests final durable
            // admission at its execution boundary before emitting a prompt.
            TurnDisposition::Delivered => Ok(()),
            TurnDisposition::Answered(code) => {
                tracing::info!(
                    target: "csp::ci",
                    %command_id,
                    %code,
                    "the CI continuation's turn was refused at delivery"
                );
                self.ci_continuations
                    .mark_terminal(command_id, &code, now_secs(), None)?;
                self.sync_ci_listener();
                Ok(())
            }
            TurnDisposition::Registered | TurnDisposition::Interrupt => {
                tracing::warn!(
                    target: "csp::ci",
                    %command_id,
                    "a CI continuation's rebuilt turn produced an impossible disposition"
                );
                Ok(())
            }
            TurnDisposition::Silent => self.settle_silent_delivery(&record),
        }
    }

    /// Recheck a CI turn at the actor's execution boundary and durably claim it
    /// before permitting any prompt. A crash after this claim may lose the
    /// execution, but cannot admit this command or operation a second time.
    pub(crate) fn admit_ci_turn_start(
        &mut self,
        session_id: &str,
        command_id: &str,
    ) -> anyhow::Result<bool> {
        let Some(record) = self.ci_continuations.record(command_id).cloned() else {
            self.in_flight.remove(command_id);
            return Ok(false);
        };
        if record.target.session_id != session_id || !matches!(record.state, RecordState::Ready(_))
        {
            self.in_flight.remove(command_id);
            return Ok(false);
        }
        if record.expires_at <= now_secs() {
            self.expire_ci_continuation(command_id)?;
            self.in_flight.remove(command_id);
            return Ok(false);
        }
        if self
            .state
            .session(session_id)
            .and_then(|session| session.project_ref.as_deref())
            != Some(record.identity.project.as_str())
        {
            self.refuse_ci_continuation(
                command_id,
                CI_CONTINUATION_PROJECT_MISMATCH,
                "the CI result project no longer matches this execution's recorded project",
                None,
            )?;
            self.in_flight.remove(command_id);
            return Ok(false);
        }
        let projects = ProjectsFile::default();
        let actor_seats = crate::actor_seats::ActorSeatsFile::default();
        let mut other_in_flight = self.in_flight.clone();
        other_in_flight.remove(command_id);
        let mut context = self.context(record.channel_id, &projects, &actor_seats, &record.signer);
        context.in_flight = &other_in_flight;
        let decision = decide_turn_command(
            &context,
            now_secs(),
            TurnCommand {
                command_id: command_id.to_owned(),
                target: record.target.clone(),
                action: TurnAction::Start {
                    text: String::new(),
                    attachments: Vec::new(),
                    deliver: buzz_core::coding_session_command::CodingSessionDelivery::Boundary,
                },
                operation_key: Some(ci_continuation_pointer(&record.correlation_id)),
                payload_digest: record.payload_digest,
            },
        );
        match decision {
            TurnDecision::Start { operation_key, .. } => {
                if let Some(key) = operation_key {
                    self.state.consume_operation(&key, command_id, now_secs())?;
                }
                self.state.consume_command(command_id, now_secs())?;
                self.retire_ci_continuation(command_id)?;
                Ok(true)
            }
            TurnDecision::Fail { code, message, .. } => {
                self.refuse_ci_continuation(command_id, code, &message, None)?;
                self.in_flight.remove(command_id);
                Ok(false)
            }
            TurnDecision::Ignore(reason) => {
                if let Some(refusal) = reason.refusal() {
                    self.refuse_ci_continuation(command_id, refusal.code, refusal.message, None)?;
                }
                self.in_flight.remove(command_id);
                Ok(false)
            }
            _ => {
                self.in_flight.remove(command_id);
                Ok(false)
            }
        }
    }

    /// Decide what a silent admission means for a ready record.
    ///
    /// `decide_turn_command` stays silent for the commands it has already
    /// answered. Which of them applies is a durable fact, so it is read from
    /// the ledgers rather than guessed.
    fn settle_silent_delivery(&mut self, record: &CiContinuationRecord) -> anyhow::Result<()> {
        let command_id = &record.command_id;
        if self.state.is_command_consumed(command_id) || self.state.is_command_refused(command_id) {
            // Already run, or already answered, in an earlier process. The
            // ledger is the fence; the record has nothing left to add.
            self.ci_continuations.remove(command_id)?;
            self.sync_ci_listener();
            return Ok(());
        }
        if self.in_flight.contains_key(command_id) {
            // On its way to a mailbox. Retired at `TurnStarted`.
            return Ok(());
        }
        // Nothing durable says why, so this is a transient refusal to admit.
        // Back off rather than spin; expiry is the outer bound.
        self.ci_continuations
            .note_attempt(command_id, now_secs().saturating_add(DELIVERY_RETRY_SECS))?;
        Ok(())
    }

    /// Answer one registration whose window closed, with the disposition the
    /// provider actually observed.
    ///
    /// The provider cannot tell "hidden from my key" from "CI never reported":
    /// both are an empty answer. So it publishes what it witnessed. A relay
    /// that served this provider's subscription and returned nothing is
    /// `CI_RESULT_UNAVAILABLE_OR_HIDDEN`; a window in which no answer was ever
    /// observed is `CI_CONTINUATION_EXPIRED`. Which case it was is recorded on
    /// the terminal record.
    fn expire_ci_continuation(&mut self, command_id: &str) -> anyhow::Result<()> {
        let Some(record) = self.ci_continuations.record(command_id).cloned() else {
            return Ok(());
        };
        let (code, observation, message) = match (&record.state, record.relay_answered) {
            (RecordState::Ready(_), _) => (
                CI_CONTINUATION_EXPIRED,
                None,
                "a verified CI result was found, but the continuation turn could not be admitted \
                 before the registration expired",
            ),
            (_, true) => (
                CI_RESULT_UNAVAILABLE_OR_HIDDEN,
                Some(RelayObservation::AnsweredEmpty),
                "no CI result for this identity was visible to this provider's identity before \
                 expiry; the project may be private to it, or CI never reported",
            ),
            (_, false) => (
                CI_CONTINUATION_EXPIRED,
                Some(RelayObservation::Unanswered),
                "the registration expired before this provider could check the relay",
            ),
        };
        self.refuse_ci_continuation(command_id, code, message, observation)
    }

    /// Publish a terminal refusal for one registration, durably and once.
    ///
    /// Refusal ledger, then outbox, then the terminal record — the same order
    /// every other refusal in this crate takes, so a crash costs a receipt
    /// rather than a second answer.
    fn refuse_ci_continuation(
        &mut self,
        command_id: &str,
        code: &str,
        message: &str,
        observation: Option<RelayObservation>,
    ) -> anyhow::Result<()> {
        let Some(record) = self.ci_continuations.record(command_id).cloned() else {
            return Ok(());
        };
        let now = now_secs();
        self.state.record_refusal(command_id, now)?;
        let receipt = LifecycleReceipt::turn_refused(command_id, &record.target, code, message);
        self.enqueue_receipt(record.channel_id, command_id, &receipt)?;
        self.ci_continuations
            .mark_terminal(command_id, code, now, observation)?;
        tracing::info!(target: "csp::ci", %command_id, %code, "CI continuation refused");
        self.sync_ci_listener();
        Ok(())
    }

    /// Retire a registration after durable start permission.
    ///
    /// CI calls this after operation and command claims, before permitting the
    /// actor's prompt. The ordinary `TurnStarted` arm may call it again.
    pub(crate) fn retire_ci_continuation(&mut self, command_id: &str) -> anyhow::Result<()> {
        if self.ci_continuations.record(command_id).is_none() {
            return Ok(());
        }
        self.ci_continuations.remove(command_id)?;
        self.sync_ci_listener();
        Ok(())
    }

    /// Reconcile the store against the durable ledgers at startup.
    ///
    /// A record whose `commandId` is already consumed or already refused is
    /// dropped: those ledgers are the fence, and a surviving record could only
    /// promise a turn that already ran or re-answer a command already
    /// answered. Everything else is left exactly where it was — a `waiting`
    /// record is picked up by the listener's first REQ, which replays stored
    /// results, and a `ready` record is re-delivered by the first tick.
    pub(crate) fn recover_ci_continuations(&mut self) -> anyhow::Result<()> {
        let stale: Vec<String> = self
            .ci_continuations
            .pending()
            .filter(|record| {
                self.state.is_command_consumed(&record.command_id)
                    || self.state.is_command_refused(&record.command_id)
            })
            .map(|record| record.command_id.clone())
            .collect();
        for command_id in stale {
            tracing::info!(
                target: "csp::ci",
                %command_id,
                "a pending CI continuation was already consumed or refused — reconciling"
            );
            self.ci_continuations.remove(&command_id)?;
        }
        let pending = self.ci_continuations.pending_count();
        if pending > 0 {
            tracing::info!(
                target: "csp::ci",
                pending,
                "recovered pending CI continuations"
            );
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "ci_continuation_materialize_tests.rs"]
mod tests;
