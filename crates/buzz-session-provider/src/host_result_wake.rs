//! A finished host step wakes the seat that triggered it, by mechanism.
//!
//! # The defect this exists for
//!
//! Ledger 236(g), measured in the `kettle-control` run on 2026-09-22: the
//! lead triggered project action `verify` (kind:46020 `903ce3d2…`, 13:41:55Z),
//! Brian approved it, this host claimed and executed it, and the relay
//! recorded the result — `exit 0`, 19 tests, 756 ms, checkout `0cbe84e8`, at
//! 13:57:25Z. **Nothing woke the lead.** No kind:44220 was attempted and
//! nothing appeared in the provider log; the lead had itself told the run
//! that "nobody is notified automatically". Brian sent a message 6 m 29 s
//! later and that is the only reason the run continued. By contrast the
//! decision answer at 13:40:13Z carried a wake and reached the lead inside a
//! second.
//!
//! A person filling that gap is not a fallback, it is the defect: the whole
//! point of a host step is that a machine ran it, so a machine says so.
//!
//! # Which event this believes, and why it is not the kind:46023
//!
//! The host signs the terminal result as a kind:46023. The relay validates
//! it — the signer is the host whose claim it recorded, the `requestedEventId`
//! is the request it issued, the `h` tag is the workflow's own channel, the
//! row was still `claimed` — and only then re-signs the whole result verbatim
//! as the kind:46014 echo
//! (`crates/buzz-relay/src/handlers/host_steps.rs:240-434`).
//!
//! This module subscribes to the **kind:46014**, for three reasons that all
//! say the same thing:
//!
//! 1. **It is the durable, relay-validated fact.** A kind:46023 alone proves
//!    nothing: `buzz-core`'s own evidence fold discards a host result with no
//!    matching echo (`crates/buzz-core/src/project_work_inputs.rs:591-603`)
//!    and calls one echoed by anything but the relay's self key *unproved*
//!    (`crates/buzz-core/src/project_work_fold_project.rs:724-737`). A wake
//!    is a claim about what happened; it must rest on the same fact the
//!    evidence fold rests on, or the two will eventually disagree in public.
//! 2. **It has a trust root here and the kind:46023 does not.** This provider
//!    witnesses the relay's identity over NIP-11 `self` and already verifies
//!    every receipt and every kind:46013 against it. A kind:46023 is signed
//!    by an arbitrary host pubkey this provider has no chain to; believing
//!    one directly would let any key that can reach the relay wake somebody
//!    else's seat. The relay additionally refuses client submission of a
//!    kind:46014 outright (`buzz_core::kind::is_relay_only_kind`), so the
//!    echo cannot be forged even by a member.
//! 3. **Nothing is lost.** [`HostStepExited`] carries the accepted
//!    [`HostStepResult`] verbatim, plus `claimedBy` and `resultEventId`.
//!
//! The honest cost, recorded rather than hidden: a kind:46023 *refused before
//! any claim* never produces an echo (`host_steps.rs:270-284`), and the
//! relay's echo publish is best-effort after commit (`:377-379`). Those
//! results wake nobody. That is a narrower gap than the one 236(g) names and
//! it is not closed here.
//!
//! # How the run is routed back to the seat that triggered it
//!
//! Neither the kind:46014 nor the kind:46023 names the person or agent who
//! started the run. The kind:46020 trigger does not name the run either — the
//! run id does not exist when the trigger is signed. The relay is what joins
//! them: it derives the trigger's *signature* into the run's trigger context
//! (`crates/buzz-relay/src/handlers/command_executor.rs:1200-1207`,
//! `author: hex::encode(&self_bytes)`) and carries that context verbatim into
//! the kind:46013 request. So the chain this module walks is
//!
//! ```text
//! 46020 (seat signs)  →  46013 triggerContext.author  →  46014 result
//! ```
//!
//! and every link in it is relay-signed. The seat's own account of what it
//! triggered is never consulted; it could not be, and it should not be.
//!
//! Both the kind:46013 and the kind:46014 carry the workflow's channel in
//! their `h` tag, so both arrive on the channel subscription this provider
//! already holds. There is no poll and no new socket: the request is indexed
//! when it arrives, the echo is answered when it arrives.

use buzz_core::coding_session_command::{coding_session_target_key, CodingSessionTarget};
use buzz_core::host_step::{
    decode_host_step_exited, decode_host_step_requested, HostStepExited, HostStepRequested,
};
use nostr::{Event, Keys};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[path = "host_result_wake_store.rs"]
mod store;
pub use store::{HostResultWakeStore, TriggerFact};

/// Schema string carried by the host-result wake pointer.
pub const HOST_RESULT_WAKE_SCHEMA: &str = "buzz-host-result-wake/v1";

/// The `type` every host-result wake pointer declares.
pub const HOST_RESULT_WAKE_TYPE: &str = "host_result";

/// Prefix of the deterministic command id a host-result wake is minted under.
const COMMAND_ID_PREFIX: &str = "host-result-wake-";

/// Why a candidate kind:46013 or kind:46014 was not believed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostStepRejection(pub String);

impl std::fmt::Display for HostStepRejection {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Verify the Schnorr signature and the witnessed relay signer of a candidate
/// relay-signed event.
///
/// Fail closed and with no fallback, exactly as
/// [`crate::action_step_listener::verify_request_event`] does for the
/// kind:46013 it serves: an unwitnessed relay identity means *no* host-step
/// fact is believed, rather than one believed on the assumption that whoever
/// signed it must have been the relay.
fn verify_relay_signed(event: &Event, relay_self: &str) -> Result<(), HostStepRejection> {
    event.verify().map_err(|error| {
        HostStepRejection(format!(
            "host step event failed cryptographic verification: {error}"
        ))
    })?;
    let signer = event.pubkey.to_hex();
    if !signer.eq_ignore_ascii_case(relay_self) {
        return Err(HostStepRejection(format!(
            "host step event signer {signer} does not match the witnessed relay self {relay_self}"
        )));
    }
    Ok(())
}

/// Verify one candidate kind:46013 and decode it.
///
/// Deliberately *not* expiry-checked. The listener that executes a request
/// refuses an expired one because there is nothing an honest host can do with
/// it; this reader only wants the run's trigger author, and the fact that a
/// claim window closed says nothing about who started the run.
pub fn verify_requested(
    event: &Event,
    relay_self: &str,
) -> Result<HostStepRequested, HostStepRejection> {
    verify_relay_signed(event, relay_self)?;
    decode_host_step_requested(event)
        .map_err(|error| HostStepRejection(format!("invalid host step request: {error}")))
}

/// Verify one candidate kind:46014 and decode it.
pub fn verify_exited(event: &Event, relay_self: &str) -> Result<HostStepExited, HostStepRejection> {
    verify_relay_signed(event, relay_self)?;
    let exited = decode_host_step_exited(event)
        .map_err(|error| HostStepRejection(format!("invalid host step echo: {error}")))?;
    if exited.result_event_id.len() != 64
        || !exited
            .result_event_id
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(HostStepRejection(
            "host step echo does not name a result event id".to_owned(),
        ));
    }
    Ok(exited)
}

/// The hex pubkey the relay derived from the kind:46020 trigger's signature.
///
/// `None` when the context carries no author — a scheduled or `ref_updated`
/// run has no triggering seat, and a run with no triggering seat wakes
/// nobody. Read from the relay-supplied `triggerContext`, never from a tag
/// the trigger's signer could have chosen.
pub fn trigger_author(request: &HostStepRequested) -> Option<String> {
    let author = request
        .trigger_context
        .get("author")
        .and_then(serde_json::Value::as_str)?
        .trim()
        .to_ascii_lowercase();
    (author.len() == 64 && author.bytes().all(|byte| byte.is_ascii_hexdigit())).then_some(author)
}

/// The short summary one wake carries.
///
/// Exactly the fields ledger 236(g) names — run, step, exit code, checkout
/// sha, duration and the result event id — plus the disposition, because
/// `exitCode: null` on a `timed_out` and on a `lost_on_restart` would
/// otherwise read the same. Absent facts are absent keys, never guessed
/// values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostResultPointer {
    pub schema: String,
    #[serde(rename = "type")]
    pub pointer_type: String,
    pub run_id: String,
    pub step_id: String,
    pub disposition: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkout_sha: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    pub result_event_id: String,
}

impl HostResultPointer {
    /// Build the pointer for one accepted echo.
    pub fn of(exited: &HostStepExited) -> Self {
        let result = &exited.result;
        Self {
            schema: HOST_RESULT_WAKE_SCHEMA.to_owned(),
            pointer_type: HOST_RESULT_WAKE_TYPE.to_owned(),
            run_id: result.run_id.clone(),
            step_id: result.step_id.clone(),
            disposition: disposition_slug(result.disposition).to_owned(),
            exit_code: result.exit_code,
            // The sha the command actually ran against, sampled after it ran.
            checkout_sha: result.head_sha.clone(),
            duration_ms: result.duration_ms,
            result_event_id: exited.result_event_id.clone(),
        }
    }
}

/// The wire slug of a disposition, taken from the same `serde` rename the
/// wire uses so the two cannot drift.
fn disposition_slug(disposition: buzz_core::host_step::HostStepDisposition) -> &'static str {
    use buzz_core::host_step::HostStepDisposition as Disposition;
    match disposition {
        Disposition::Exited => "exited",
        Disposition::TimedOut => "timed_out",
        Disposition::LostOnRestart => "lost_on_restart",
        Disposition::Refused => "refused",
    }
}

/// The text one host-result wake delivers.
pub fn wake_text(exited: &HostStepExited) -> Result<String, String> {
    serde_json::to_string(&HostResultPointer::of(exited))
        .map_err(|error| format!("host result wake pointer could not be encoded: {error}"))
}

/// Whether a parsed JSON object is a host-result wake pointer this provider
/// mints.
///
/// Exact: this provider's own schema and type, every required key present and
/// of the right JSON type, and no key beyond the ones [`HostResultPointer`]
/// emits. Anything looser would let ordinary operator JSON claim to be a
/// relay-validated host result.
pub fn is_host_result_pointer(object: &serde_json::Map<String, serde_json::Value>) -> bool {
    const REQUIRED: [&str; 6] = [
        "schema",
        "type",
        "runId",
        "stepId",
        "disposition",
        "resultEventId",
    ];
    const OPTIONAL: [&str; 3] = ["exitCode", "checkoutSha", "durationMs"];
    if object.get("schema").and_then(serde_json::Value::as_str) != Some(HOST_RESULT_WAKE_SCHEMA)
        || object.get("type").and_then(serde_json::Value::as_str) != Some(HOST_RESULT_WAKE_TYPE)
    {
        return false;
    }
    if !REQUIRED
        .iter()
        .all(|key| object.get(*key).is_some_and(serde_json::Value::is_string))
    {
        return false;
    }
    if object
        .keys()
        .any(|key| !REQUIRED.contains(&key.as_str()) && !OPTIONAL.contains(&key.as_str()))
    {
        return false;
    }
    object.get("exitCode").is_none_or(serde_json::Value::is_i64)
        && object
            .get("checkoutSha")
            .is_none_or(serde_json::Value::is_string)
        && object
            .get("durationMs")
            .is_none_or(serde_json::Value::is_u64)
}

/// The command id one host-result wake is minted under.
///
/// Derived only from caller-supplied stable inputs — the result event id and
/// the exact generation being woken — so a retry after a crash mints the same
/// id rather than a second command (the rule ledger 208's deterministic-id
/// finding put in force: an id that hashes anything `now`-relative promises an
/// idempotence it does not have).
pub fn command_id(result_event_id: &str, target: &CodingSessionTarget) -> String {
    let mut digest = Sha256::new();
    digest.update(b"buzz-provider-host-result-wake/v1\0");
    digest.update(result_event_id.as_bytes());
    digest.update(b"\0");
    digest.update(coding_session_target_key(target).as_bytes());
    format!("{COMMAND_ID_PREFIX}{}", hex::encode(digest.finalize()))
}

/// The outbox row identity of one host-result wake.
///
/// The result event id and the target, which is the same pair the durable
/// wake ledger is keyed by, so a row that is already queued is never queued
/// twice within one process either.
pub fn outbox_semantic_key(result_event_id: &str, target: &CodingSessionTarget) -> String {
    format!(
        "host-result-wake:{result_event_id}:{}",
        coding_session_target_key(target)
    )
}

/// Sign the boundary wake for one accepted echo.
///
/// Deliberately [`crate::action_route::build_route_event`] and not a second
/// builder beside it: an action that *routes* a brief to a seat and an action
/// that *reports* a result to a seat are the same wire act — one kind:44220
/// carrying `thread.turn.start` with `deliver: boundary` — and two builders
/// for one act is how the two drift.
///
/// Boundary, never interrupt: a finished step is news, not an emergency, and
/// a seat that is mid-turn should finish the thought it is having. The
/// provider that owns the seat then applies boundary delivery exactly as it
/// does for every other kind:44220.
pub fn build_wake_event(
    keys: &Keys,
    channel_id: Uuid,
    target: CodingSessionTarget,
    command_id: String,
    text: String,
) -> Result<Event, String> {
    crate::action_route::build_route_event(keys, channel_id, target, command_id, text)
}

impl crate::Provider {
    /// Index who triggered one run, from the relay-signed kind:46013.
    ///
    /// Infallible by construction: nothing here may stop this provider from
    /// serving turns. A store that cannot be written leaves the run
    /// unindexed, and the next subscription replay of the same request tries
    /// again — the request outlives its own claim window in relay storage, so
    /// there is always another chance.
    pub(crate) fn on_host_step_requested(&mut self, event: &nostr::Event) {
        let event_id = event.id.to_hex();
        let Some(relay_self) = self.relay_self.clone() else {
            tracing::debug!(
                target: "csp::host_result_wake",
                %event_id,
                "no relay identity is witnessed yet; this host step request is not indexed"
            );
            return;
        };
        let request = match verify_requested(event, &relay_self) {
            Ok(request) => request,
            Err(rejection) => {
                tracing::warn!(
                    target: "csp::host_result_wake",
                    %event_id,
                    "skipped a candidate host step request: {rejection}"
                );
                return;
            }
        };
        let Some(author) = trigger_author(&request) else {
            // A schedule, a push, a webhook. Real runs with no triggering
            // seat, and a run with no triggering seat wakes nobody.
            tracing::debug!(
                target: "csp::host_result_wake",
                %event_id,
                run_id = %request.run_id,
                "host step request carries no trigger author; nothing to wake"
            );
            return;
        };
        let Ok(channel_ref) = Uuid::parse_str(&request.channel_id) else {
            tracing::warn!(
                target: "csp::host_result_wake",
                %event_id,
                "host step request names an unparseable channel"
            );
            return;
        };
        let run_key = buzz_core::host_step::host_step_d_tag(&request.run_id, &request.step_id);
        let fact = TriggerFact {
            run_key: run_key.clone(),
            author: author.clone(),
            channel_ref,
            project: request.project.clone(),
            workflow_name: request.workflow_name.clone(),
            observed_at: crate::state::now_secs(),
        };
        match self.host_result_wakes.note_trigger(fact) {
            // Every replay of the same request lands here; say nothing.
            Ok(false) => {}
            Ok(true) => tracing::info!(
                target: "csp::host_result_wake",
                %event_id,
                %run_key,
                %author,
                action = %request.workflow_name,
                "indexed the seat that triggered this run"
            ),
            Err(error) => tracing::error!(
                target: "csp::host_result_wake",
                %event_id,
                %run_key,
                "could not index the run's trigger author: {error}"
            ),
        }
    }

    /// Wake the seat that triggered a run whose result the relay accepted.
    ///
    /// The whole of ledger 236(g)'s remedy: one relay-validated result, one
    /// boundary kind:44220, at most once per result event id, decided by this
    /// process without asking anybody anything.
    ///
    /// Infallible for the same reason as [`Self::on_host_step_requested`],
    /// and with one extra consequence worth naming: a failure to *queue* the
    /// wake after the at-most-once claim is already on disk loses that wake
    /// permanently. That is the trade [`HostResultWakeStore`] documents, and
    /// it is logged at `error` so the loss is visible rather than silent.
    pub(crate) fn on_host_step_exited(&mut self, event: &nostr::Event) {
        let event_id = event.id.to_hex();
        let Some(relay_self) = self.relay_self.clone() else {
            tracing::debug!(
                target: "csp::host_result_wake",
                %event_id,
                "no relay identity is witnessed yet; this host step result wakes nobody"
            );
            return;
        };
        let exited = match verify_exited(event, &relay_self) {
            Ok(exited) => exited,
            Err(rejection) => {
                tracing::warn!(
                    target: "csp::host_result_wake",
                    %event_id,
                    "skipped a candidate host step result: {rejection}"
                );
                return;
            }
        };
        let result_event_id = exited.result_event_id.clone();
        // Cheap and first: a replayed echo for a result already answered is
        // the common case on every reconnect, and it must cost no lookup and
        // no log line.
        if self.host_result_wakes.already_waked(&result_event_id) {
            return;
        }
        let run_key =
            buzz_core::host_step::host_step_d_tag(&exited.result.run_id, &exited.result.step_id);
        let Some(author) = self
            .host_result_wakes
            .trigger(&run_key)
            .map(|fact| fact.author.clone())
        else {
            tracing::info!(
                target: "csp::host_result_wake",
                %event_id,
                %run_key,
                "a host result for a run this host never saw triggered is ignored"
            );
            return;
        };
        let Some((channel_id, target, session_id)) = self.seat_of_actor(&author) else {
            tracing::info!(
                target: "csp::host_result_wake",
                %event_id,
                %run_key,
                %author,
                "a host result for a run no live seat on this host triggered is ignored"
            );
            return;
        };
        let text = match wake_text(&exited) {
            Ok(text) => text,
            Err(error) => {
                tracing::error!(
                    target: "csp::host_result_wake",
                    %event_id,
                    %run_key,
                    "the host result summary could not be encoded: {error}"
                );
                return;
            }
        };
        let command_id = command_id(&result_event_id, &target);
        let semantic_key = outbox_semantic_key(&result_event_id, &target);
        let wake = match build_wake_event(
            &self.config.keys,
            channel_id,
            target,
            command_id.clone(),
            text,
        ) {
            Ok(wake) => wake,
            Err(error) => {
                tracing::error!(
                    target: "csp::host_result_wake",
                    %event_id,
                    %run_key,
                    "the host result wake could not be signed: {error}"
                );
                return;
            }
        };
        // Claim before queueing. A crash between the two costs this one wake;
        // the other order costs at-most-once, which is the guarantee.
        match self.host_result_wakes.claim_result(&result_event_id) {
            Ok(true) => {}
            Ok(false) => return,
            Err(error) => {
                tracing::error!(
                    target: "csp::host_result_wake",
                    %event_id,
                    %run_key,
                    "could not take at-most-once custody of this result; no wake is sent: {error}"
                );
                return;
            }
        }
        match self.outbox.enqueue(
            buzz_core::kind::KIND_CODING_SESSION_COMMAND,
            &semantic_key,
            crate::publish::Priority::High,
            wake,
        ) {
            Ok(_) => tracing::info!(
                target: "csp::host_result_wake",
                %event_id,
                %run_key,
                %author,
                %session_id,
                %command_id,
                exit_code = ?exited.result.exit_code,
                "queued a boundary wake for the seat that triggered this run"
            ),
            Err(error) => tracing::error!(
                target: "csp::host_result_wake",
                %event_id,
                %run_key,
                %result_event_id,
                "the host result wake is claimed but could not be queued and is now lost: {error}"
            ),
        }
    }

    /// The one live execution on this host seated by `actor`, with the
    /// channel its commands are published into.
    ///
    /// Closed records are not candidates and the newest generation wins —
    /// the same discipline [`crate::action_route::pick_open_execution`]
    /// applies, and for the same reason: nothing enforces one execution per
    /// actor, because a disposable seat ends one and creates another under
    /// the same key, and a retired seat would supply an address no turn can
    /// reach.
    fn seat_of_actor(&self, actor: &str) -> Option<(Uuid, CodingSessionTarget, String)> {
        let record = self
            .state
            .sessions()
            .filter(|candidate| {
                !candidate.closed
                    && candidate
                        .actor
                        .as_deref()
                        .is_some_and(|seated| seated.eq_ignore_ascii_case(actor))
            })
            .max_by_key(|candidate| {
                (
                    candidate.created_at_ms,
                    candidate.generation,
                    candidate.session_id.clone(),
                )
            })?;
        if !self.may_wake(record) {
            tracing::info!(
                target: "csp::host_result_wake",
                session_id = %record.session_id,
                "this provider holds no authority to steer the seat that triggered the run; \
                 no wake is attempted"
            );
            return None;
        }
        Some((
            record.channel_id,
            self.target_for(record),
            record.session_id.clone(),
        ))
    }

    /// Whether this provider's own key may start a turn on `record`.
    ///
    /// The local form of [`crate::team_wake::provider_may_wake`] — founder or
    /// a folded operator grant — read from the durable record rather than
    /// from a fresh relay projection, because a wake decision made on the
    /// arrival of one echo cannot afford a complete-partition scan and does
    /// not need one: the record's `granted_operators` is the fold this
    /// provider already applied and persisted.
    ///
    /// Checked before the wake is built rather than after it is refused. A
    /// provider that publishes a kind:44220 it has no authority for buys a
    /// refusal receipt and nothing else, and — worse — would have spent the
    /// result's at-most-once claim doing it, so the host that *does* hold the
    /// grant could never answer.
    fn may_wake(&self, record: &crate::state::SessionRecord) -> bool {
        record
            .founder_pubkey
            .as_deref()
            .is_some_and(|founder| founder.eq_ignore_ascii_case(&self.pubkey_hex))
            || record
                .granted_operators
                .iter()
                .any(|operator| operator.eq_ignore_ascii_case(&self.pubkey_hex))
    }
}
