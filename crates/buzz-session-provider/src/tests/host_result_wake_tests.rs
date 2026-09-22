//! The defect ledger 236(g) measured, reproduced and then closed.
//!
//! Every case here drives the provider's **real** relay-event path —
//! [`Provider::handle_command_event`], the same entry the run loop calls for
//! every frame the channel subscription delivers — and then asks the outbox
//! what, if anything, this host decided to say. Nothing is called by hand
//! that production does not call.
//!
//! The wire chain the fixture replays is the one the control run produced, in
//! order:
//!
//! 1. the seat signs a kind:46020 project-action trigger;
//! 2. the relay mints the run and issues a kind:46013 whose `triggerContext`
//!    carries the *signature-derived* author of that kind:46020;
//! 3. the host runs the step and signs a kind:46023;
//! 4. the relay validates it and echoes it as a kind:46014.
//!
//! The kind:46020 is published from the seated actor's own key exactly as the
//! ruling asks, and this provider deliberately does **nothing** with it: the
//! run id does not exist yet when it is signed, and the trigger's own tags
//! are chosen by its signer. What the provider believes is the relay's
//! account of who signed it, which is step 2.

use super::*;

use std::path::PathBuf;

use buzz_core::coding_session_command::{
    CodingSessionAction, CodingSessionCommandPayload, CodingSessionDelivery,
};
use buzz_core::host_step::{
    build_host_step_exited, build_host_step_requested, build_host_step_result, HostStepDisposition,
    HostStepExited, HostStepRequested, HostStepResult, HOST_STEP_KIND_RUN_ON_HOST,
    HOST_STEP_SCHEMA,
};
use buzz_core::kind::{
    KIND_CODING_SESSION_COMMAND, KIND_HOST_STEP_RESULT, KIND_WORKFLOW_HOST_STEP_EXITED,
    KIND_WORKFLOW_HOST_STEP_REQUESTED, KIND_WORKFLOW_TRIGGER,
};
use nostr::{EventBuilder, Kind, Tag};

use crate::host_result_wake::{
    command_id, is_host_result_pointer, outbox_semantic_key, trigger_author, verify_exited,
    HostResultPointer, HOST_RESULT_WAKE_SCHEMA,
};

const RUN_ID: &str = "3f1d2c4b-5a6e-4f80-9b12-7c3d4e5f6a70";
const WORKFLOW_ID: &str = "8c2a1b3d-4e5f-4a61-8b72-9d0e1f2a3b40";
const STEP_ID: &str = "verify";

/// Everything one case needs: a provider that has witnessed its relay, one
/// seated live execution, and the keys that sign each half of the chain.
struct Fixture {
    provider: Provider,
    /// This provider's own identity, kept so a restart can reopen the same
    /// state directory as the same host rather than as a stranger.
    provider_keys: Keys,
    state_dir: PathBuf,
    relay: Keys,
    seat: Keys,
    /// The channel the seat's execution publishes into — where its wake goes.
    seat_channel: Uuid,
    /// The workflow's channel, where the host-step events arrive.
    workflow_channel: Uuid,
    target: CodingSessionTarget,
    _dir: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        Self::with_actor(Keys::generate())
    }

    fn with_actor(seat: Keys) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = dir.path().join("state");
        let relay = Keys::generate();
        let provider_keys = Keys::generate();
        let mut provider = Provider::new(config_of(
            provider_keys.clone(),
            &state,
            None,
            "/nonexistent-agent".into(),
        ))
        .expect("provider");
        provider.set_relay_self(relay.public_key().to_hex());

        let seat_channel = Uuid::new_v4();
        let mut record = governed_record(seat_channel, dir.path(), &"ab".repeat(32));
        record.actor = Some(seat.public_key().to_hex());
        record.role = Some("lead".into());
        // The ordinary shape on a developer's own machine and in the control
        // run: the provider runs under the founder's key, so it may steer the
        // seats it hosts. `a_provider_with_no_authority_over_the_seat…` below
        // covers the other case.
        record.founder_pubkey = Some(provider_keys.public_key().to_hex());
        let target = record.target(&provider.config.instance_id);
        provider
            .state
            .insert_session(record)
            .expect("insert the seated execution");

        Self {
            provider,
            provider_keys,
            state_dir: state,
            relay,
            seat,
            seat_channel,
            workflow_channel: Uuid::new_v4(),
            target,
            _dir: dir,
        }
    }

    /// Step 1: the seat's own kind:46020, signed with the seat's key.
    fn trigger(&self) -> Event {
        EventBuilder::new(Kind::Custom(KIND_WORKFLOW_TRIGGER as u16), "")
            .tags(vec![Tag::parse(vec![
                "d".to_owned(),
                WORKFLOW_ID.to_owned(),
            ])
            .expect("tag")])
            .sign_with_keys(&self.seat)
            .expect("sign the trigger")
    }

    /// Step 2: the relay's kind:46013, carrying the trigger's author.
    fn requested_by(&self, author: &str) -> Event {
        let request = HostStepRequested {
            schema: HOST_STEP_SCHEMA.into(),
            run_id: RUN_ID.into(),
            workflow_id: WORKFLOW_ID.into(),
            workflow_name: "verify".into(),
            step_id: STEP_ID.into(),
            step_index: 0,
            definition_hash: "cd".repeat(32),
            step_kind: HOST_STEP_KIND_RUN_ON_HOST.into(),
            channel_id: self.workflow_channel.to_string(),
            project: format!("30621:{}:kettle-control", "11".repeat(32)),
            approval: None,
            trigger_context: serde_json::json!({
                "author": author,
                "channel_id": self.workflow_channel.to_string(),
            }),
            inputs: serde_json::json!({}),
            expires_at: crate::state::now_secs() + 3_600,
        };
        let (tags, content) = build_host_step_requested(&request).expect("build the request");
        signed(
            &self.relay,
            KIND_WORKFLOW_HOST_STEP_REQUESTED,
            tags,
            content,
        )
    }

    fn requested(&self) -> Event {
        self.requested_by(&self.seat.public_key().to_hex())
    }

    /// Step 3: the host's own kind:46023 for `run_id`, host-signed.
    fn result_event(&self, host: &Keys, run_id: &str) -> Event {
        let result = self.result(run_id);
        let (tags, content) = build_host_step_result(&result).expect("build the result");
        signed(host, KIND_HOST_STEP_RESULT, tags, content)
    }

    fn result(&self, run_id: &str) -> HostStepResult {
        HostStepResult {
            schema: HOST_STEP_SCHEMA.into(),
            run_id: run_id.into(),
            step_id: STEP_ID.into(),
            requested_event_id: "ef".repeat(32),
            claim_event_id: Some("ab".repeat(32)),
            channel_id: self.workflow_channel.to_string(),
            disposition: HostStepDisposition::Exited,
            exit_code: Some(0),
            refusal: None,
            timed_out: false,
            duration_ms: Some(756),
            head_sha: Some("0cbe84e8".to_owned() + &"0".repeat(32)),
            agents_commit: None,
            dirty: Some(false),
            checkout: None,
            stdout_tail: "19 passed".into(),
            stderr_tail: String::new(),
            truncated: false,
            artifact_path: None,
            routed: None,
            artifacts: Vec::new(),
        }
    }

    /// Step 4: the relay's kind:46014 echo of one kind:46023.
    fn exited_for(&self, result_event: &Event, host: &Keys) -> Event {
        let exited = HostStepExited {
            schema: HOST_STEP_SCHEMA.into(),
            result: buzz_core::host_step::decode_host_step_result(result_event)
                .expect("the fixture's own result decodes"),
            claimed_by: host.public_key().to_hex(),
            result_event_id: result_event.id.to_hex(),
        };
        let (tags, content) = build_host_step_exited(&exited).expect("build the echo");
        signed(&self.relay, KIND_WORKFLOW_HOST_STEP_EXITED, tags, content)
    }

    /// Replace the running provider with a second process over the same state
    /// directory, as the same host, witnessing the same relay.
    ///
    /// Nothing is re-seeded: the execution, the trigger index and the waked
    /// ledger are whatever the first process left on disk, which is the only
    /// version of this test that proves anything about a restart.
    fn restart(&mut self) {
        let mut restarted = Provider::new(config_of(
            self.provider_keys.clone(),
            &self.state_dir,
            None,
            "/nonexistent-agent".into(),
        ))
        .expect("restarted provider");
        restarted.set_relay_self(self.relay.public_key().to_hex());
        self.provider = restarted;
    }

    /// Feed one frame through the same entry point the run loop uses.
    async fn deliver(&mut self, channel_id: Uuid, event: &Event) {
        self.provider
            .handle_command_event(channel_id, event)
            .await
            .expect("the provider handles the frame");
    }

    /// Whether a wake for this result event id is queued for this seat.
    fn wake_queued(&self, result_event_id: &str) -> bool {
        self.provider.outbox.contains(
            KIND_CODING_SESSION_COMMAND,
            &outbox_semantic_key(result_event_id, &self.target),
        )
    }

    fn queued_commands(&self) -> usize {
        self.provider.outbox.pending_len()
    }
}

fn signed(keys: &Keys, kind: u32, tags: Vec<Vec<String>>, content: String) -> Event {
    let tags: Vec<Tag> = tags
        .into_iter()
        .map(|tag| Tag::parse(tag).expect("tag"))
        .collect();
    EventBuilder::new(Kind::Custom(kind as u16), content)
        .tags(tags)
        .sign_with_keys(keys)
        .expect("sign")
}

/// **The defect, and its fix.** A seated lead triggers a project action; the
/// relay issues the request; a host runs it and the relay accepts the result.
/// Before lane 240 the provider's channel filter did not carry either
/// host-step kind, so all four frames went past it and the lead sat for
/// 6 m 29 s until a person typed something. It now answers with exactly one
/// boundary kind:44220.
#[tokio::test]
async fn a_green_host_result_wakes_the_seat_that_triggered_it() {
    let mut fixture = Fixture::new();
    let host = Keys::generate();

    // The seat's own trigger. It names no run and this provider draws no
    // conclusion from it; it is here because it is what really happens.
    let trigger = fixture.trigger();
    fixture.deliver(fixture.workflow_channel, &trigger).await;
    assert_eq!(
        fixture.queued_commands(),
        0,
        "a kind:46020 is not a fact about a run; nothing may be said yet"
    );

    let requested = fixture.requested();
    fixture.deliver(fixture.workflow_channel, &requested).await;
    assert_eq!(
        fixture.queued_commands(),
        0,
        "a request is not a result; a wake before the step ran would be a lie"
    );

    let result = fixture.result_event(&host, RUN_ID);
    // The host's own kind:46023 reaches this provider too, and is ignored: it
    // has no trust root here. Only the relay's echo is believed.
    fixture.deliver(fixture.workflow_channel, &result).await;
    assert_eq!(
        fixture.queued_commands(),
        0,
        "a host-signed result alone must never wake a seat"
    );

    let exited = fixture.exited_for(&result, &host);
    fixture.deliver(fixture.workflow_channel, &exited).await;

    assert!(
        fixture.wake_queued(&result.id.to_hex()),
        "the relay-validated result must wake the seat that triggered the run"
    );
    assert_eq!(
        fixture.queued_commands(),
        1,
        "exactly one wake, for one result"
    );
}

/// The wake carries what ledger 236(g) says a result summary must carry, in
/// the channel the *seat* lives in rather than the workflow's, addressed to
/// the seat's exact generation, and as a `boundary` turn.
#[tokio::test]
async fn the_wake_names_the_run_the_exit_and_the_checkout() {
    let mut fixture = Fixture::new();
    let host = Keys::generate();
    let requested = fixture.requested();
    fixture.deliver(fixture.workflow_channel, &requested).await;
    let result = fixture.result_event(&host, RUN_ID);
    let exited = fixture.exited_for(&result, &host);
    fixture.deliver(fixture.workflow_channel, &exited).await;

    let queued = fixture
        .provider
        .outbox
        .pending_events()
        .next()
        .expect("one queued wake");
    assert_eq!(u32::from(queued.kind.as_u16()), KIND_CODING_SESSION_COMMAND);
    assert!(
        queued
            .tags
            .iter()
            .any(|tag| tag.as_slice() == ["h", &fixture.seat_channel.to_string()]),
        "the wake goes to the channel the seat publishes into, not the workflow's"
    );
    let payload: CodingSessionCommandPayload =
        serde_json::from_str(&queued.content).expect("the wake decodes as a session command");
    assert_eq!(payload.target, fixture.target);
    assert_eq!(
        payload.command_id,
        command_id(&result.id.to_hex(), &fixture.target)
    );
    let CodingSessionAction::ThreadTurnStart { text, deliver, .. } = payload.action else {
        panic!("a wake is a turn start");
    };
    assert_eq!(
        deliver,
        CodingSessionDelivery::Boundary,
        "a finished step is news, not an emergency"
    );

    let pointer: serde_json::Value = serde_json::from_str(&text).expect("the summary is JSON");
    let object = pointer.as_object().expect("an object");
    assert!(
        is_host_result_pointer(object),
        "the summary must be the exact pointer shape this provider mints: {text}"
    );
    assert_eq!(object["schema"], HOST_RESULT_WAKE_SCHEMA);
    assert_eq!(object["runId"], RUN_ID);
    assert_eq!(object["stepId"], STEP_ID);
    assert_eq!(object["exitCode"], 0);
    assert_eq!(object["durationMs"], 756);
    assert_eq!(object["resultEventId"], result.id.to_hex());
    assert_eq!(
        object["checkoutSha"],
        "0cbe84e8".to_owned() + &"0".repeat(32)
    );
}

/// The relay serves stored events again on every reconnect, so the same echo
/// arrives many times. One result is one wake.
#[tokio::test]
async fn a_duplicate_result_wakes_once() {
    let mut fixture = Fixture::new();
    let host = Keys::generate();
    fixture
        .deliver(fixture.workflow_channel, &fixture.requested())
        .await;
    let result = fixture.result_event(&host, RUN_ID);
    let exited = fixture.exited_for(&result, &host);

    fixture.deliver(fixture.workflow_channel, &exited).await;
    fixture.deliver(fixture.workflow_channel, &exited).await;
    fixture.deliver(fixture.workflow_channel, &exited).await;

    assert_eq!(
        fixture.queued_commands(),
        1,
        "three deliveries of one accepted result are one wake"
    );
}

/// A restart between the result and its wake must not buy a second turn of
/// the lead's context. The claim is on disk before the wake is queued, so a
/// second process reading the same replayed frames says nothing.
#[tokio::test]
async fn a_restart_between_the_result_and_the_wake_still_wakes_once() {
    let mut fixture = Fixture::new();
    let host = Keys::generate();
    let requested = fixture.requested();
    fixture.deliver(fixture.workflow_channel, &requested).await;
    let result = fixture.result_event(&host, RUN_ID);
    let exited = fixture.exited_for(&result, &host);
    fixture.deliver(fixture.workflow_channel, &exited).await;
    let result_id = result.id.to_hex();
    assert!(
        fixture.wake_queued(&result_id),
        "the first process wakes the seat"
    );

    fixture.restart();
    assert!(
        fixture.provider.host_result_wakes.already_waked(&result_id),
        "the restarted process reads the claim it inherited"
    );
    // The relay replays both frames to the new subscription, oldest last.
    let before = fixture.queued_commands();
    fixture.deliver(fixture.workflow_channel, &requested).await;
    fixture.deliver(fixture.workflow_channel, &exited).await;

    assert_eq!(
        fixture.queued_commands(),
        before,
        "a restart must not re-wake: the claim for {result_id} is durable"
    );
    // And the seat it would have aimed at is the one it inherited, so a
    // second wake could not have hidden under a different outbox key either.
    assert!(fixture.wake_queued(&result_id));
}

/// A result for a run this host never saw triggered — another machine's run,
/// or one that started before this provider did — wakes nobody, and says so.
#[tokio::test]
async fn an_unrelated_run_wakes_nobody() {
    let mut fixture = Fixture::new();
    let host = Keys::generate();
    fixture
        .deliver(fixture.workflow_channel, &fixture.requested())
        .await;

    let other_run = "11111111-2222-4333-8444-555555555555";
    let result = fixture.result_event(&host, other_run);
    let exited = fixture.exited_for(&result, &host);
    fixture.deliver(fixture.workflow_channel, &exited).await;

    assert_eq!(
        fixture.queued_commands(),
        0,
        "a result for a run with no indexed trigger is ignored"
    );
    assert!(
        !fixture
            .provider
            .host_result_wakes
            .already_waked(&result.id.to_hex()),
        "an ignored result is not claimed — another provider may still own it"
    );
}

/// A run triggered by somebody this host does not seat — a founder at a
/// keyboard, or another machine's agent — wakes nobody here.
#[tokio::test]
async fn a_run_triggered_by_an_unseated_key_wakes_nobody() {
    let mut fixture = Fixture::new();
    let host = Keys::generate();
    let stranger = Keys::generate().public_key().to_hex();
    fixture
        .deliver(fixture.workflow_channel, &fixture.requested_by(&stranger))
        .await;

    let result = fixture.result_event(&host, RUN_ID);
    let exited = fixture.exited_for(&result, &host);
    fixture.deliver(fixture.workflow_channel, &exited).await;

    assert_eq!(fixture.queued_commands(), 0);
}

/// A closed execution is not an address. A seat that ended between triggering
/// the run and its result gets no wake rather than one aimed at a generation
/// no turn can reach.
#[tokio::test]
async fn a_closed_seat_is_not_woken() {
    let mut fixture = Fixture::new();
    let host = Keys::generate();
    fixture
        .deliver(fixture.workflow_channel, &fixture.requested())
        .await;
    let session_id = fixture
        .provider
        .state
        .sessions()
        .next()
        .expect("the seat")
        .session_id
        .clone();
    fixture
        .provider
        .state
        .update_session(&session_id, |record| record.closed = true)
        .expect("close the seat");

    let result = fixture.result_event(&host, RUN_ID);
    let exited = fixture.exited_for(&result, &host);
    fixture.deliver(fixture.workflow_channel, &exited).await;

    assert_eq!(fixture.queued_commands(), 0);
}

/// The echo's signer is the whole trust root. A kind:46014 signed by anything
/// but the witnessed relay self is a forgery attempt, and a forged one could
/// otherwise put arbitrary text into a lead's context.
#[tokio::test]
async fn an_echo_signed_by_anyone_but_the_relay_is_never_believed() {
    let mut fixture = Fixture::new();
    let host = Keys::generate();
    fixture
        .deliver(fixture.workflow_channel, &fixture.requested())
        .await;

    let result = fixture.result_event(&host, RUN_ID);
    let honest = fixture.exited_for(&result, &host);
    let forged = signed(
        &host,
        KIND_WORKFLOW_HOST_STEP_EXITED,
        honest
            .tags
            .iter()
            .map(|tag| tag.as_slice().to_vec())
            .collect(),
        honest.content.clone(),
    );
    fixture.deliver(fixture.workflow_channel, &forged).await;

    assert_eq!(fixture.queued_commands(), 0);
    assert!(verify_exited(&forged, &fixture.relay.public_key().to_hex()).is_err());
    assert!(verify_exited(&honest, &fixture.relay.public_key().to_hex()).is_ok());
}

/// A request whose signer is not the relay cannot put an author into the
/// index, which is what stops a member from nominating whose seat a later
/// result wakes.
#[tokio::test]
async fn a_forged_request_cannot_nominate_the_seat_a_result_wakes() {
    let mut fixture = Fixture::new();
    let host = Keys::generate();
    let honest = fixture.requested();
    let forged = signed(
        &fixture.seat,
        KIND_WORKFLOW_HOST_STEP_REQUESTED,
        honest
            .tags
            .iter()
            .map(|tag| tag.as_slice().to_vec())
            .collect(),
        honest.content.clone(),
    );
    fixture.deliver(fixture.workflow_channel, &forged).await;
    assert_eq!(
        fixture.provider.host_result_wakes.trigger_len(),
        0,
        "a request this host cannot attribute to its relay indexes nothing"
    );

    let result = fixture.result_event(&host, RUN_ID);
    let exited = fixture.exited_for(&result, &host);
    fixture.deliver(fixture.workflow_channel, &exited).await;
    assert_eq!(fixture.queued_commands(), 0);
}

/// Without a witnessed relay identity nothing is believed at all, rather than
/// believed on the assumption that whoever signed it must have been the relay.
#[tokio::test]
async fn nothing_is_believed_before_the_relay_identity_is_witnessed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let fixture = Fixture::new();
    let host = Keys::generate();
    let requested = fixture.requested();
    let result = fixture.result_event(&host, RUN_ID);
    let exited = fixture.exited_for(&result, &host);

    let mut blind = Provider::new(config_of(
        Keys::generate(),
        &dir.path().join("state"),
        None,
        "/nonexistent-agent".into(),
    ))
    .expect("provider");
    let mut record = governed_record(fixture.seat_channel, dir.path(), &"ab".repeat(32));
    record.actor = Some(fixture.seat.public_key().to_hex());
    blind.state.insert_session(record).expect("insert");

    blind
        .handle_command_event(fixture.workflow_channel, &requested)
        .await
        .expect("handle");
    blind
        .handle_command_event(fixture.workflow_channel, &exited)
        .await
        .expect("handle");

    assert_eq!(blind.host_result_wakes.trigger_len(), 0);
    assert_eq!(blind.outbox.pending_len(), 0);
}

/// The trigger author is read from the relay's derivation and nowhere else.
#[test]
fn the_trigger_author_is_only_ever_a_full_hex_pubkey() {
    let fixture = Fixture::new();
    let seat = fixture.seat.public_key().to_hex();
    let requested = |context: serde_json::Value| -> HostStepRequested {
        let mut request =
            buzz_core::host_step::decode_host_step_requested(&fixture.requested()).expect("decode");
        request.trigger_context = context;
        request
    };

    assert_eq!(
        trigger_author(&requested(serde_json::json!({"author": seat}))),
        Some(seat.clone())
    );
    assert_eq!(
        trigger_author(&requested(
            serde_json::json!({"author": seat.to_uppercase()})
        )),
        Some(seat),
        "case is normalised, because a pubkey is a value and not a spelling"
    );
    assert_eq!(
        trigger_author(&requested(serde_json::json!({}))),
        None,
        "a schedule has no triggering seat"
    );
    assert_eq!(
        trigger_author(&requested(serde_json::json!({"author": ""}))),
        None
    );
    assert_eq!(
        trigger_author(&requested(serde_json::json!({"author": "not-a-pubkey"}))),
        None
    );
    assert_eq!(
        trigger_author(&requested(serde_json::json!({"author": 7}))),
        None
    );
}

/// Two results for one run — an ordinary `verify` and a later one — are two
/// facts and two wakes. The fence is the result event id, not the run.
#[tokio::test]
async fn a_second_result_for_the_same_run_is_a_second_wake() {
    let mut fixture = Fixture::new();
    let host = Keys::generate();
    fixture
        .deliver(fixture.workflow_channel, &fixture.requested())
        .await;

    let first = fixture.result_event(&host, RUN_ID);
    let first_echo = fixture.exited_for(&first, &host);
    fixture.deliver(fixture.workflow_channel, &first_echo).await;

    // A distinct kind:46023 for the same run: same body, a different signer,
    // so a different event id.
    let second_host = Keys::generate();
    let second = fixture.result_event(&second_host, RUN_ID);
    let second_echo = fixture.exited_for(&second, &second_host);
    fixture
        .deliver(fixture.workflow_channel, &second_echo)
        .await;

    assert_ne!(first.id.to_hex(), second.id.to_hex());
    assert!(fixture.wake_queued(&first.id.to_hex()));
    assert!(fixture.wake_queued(&second.id.to_hex()));
    assert_eq!(fixture.queued_commands(), 2);
}

/// The command id is a function of the result and the generation alone, so a
/// retry mints the same id rather than a second command. Deliberately
/// asserted, because the CI-continuation id once hashed a `now`-relative
/// expiry and promised an idempotence it did not have.
#[test]
fn the_command_id_depends_on_nothing_that_moves() {
    let fixture = Fixture::new();
    let result_event_id = "9a".repeat(32);
    let first = command_id(&result_event_id, &fixture.target);
    let second = command_id(&result_event_id, &fixture.target);
    assert_eq!(first, second);
    assert!(first.starts_with("host-result-wake-"));

    let mut other_generation = fixture.target.clone();
    other_generation.generation += 1;
    assert_ne!(first, command_id(&result_event_id, &other_generation));
    assert_ne!(first, command_id(&"9b".repeat(32), &fixture.target));
}

/// The pointer recogniser is exact: operator JSON that merely looks like a
/// host result is not one.
#[test]
fn only_the_exact_pointer_shape_is_a_host_result_pointer() {
    let fixture = Fixture::new();
    let host = Keys::generate();
    let result = fixture.result_event(&host, RUN_ID);
    let exited = buzz_core::host_step::decode_host_step_exited(&fixture.exited_for(&result, &host))
        .expect("decode");
    let pointer = HostResultPointer::of(&exited);
    let value = serde_json::to_value(&pointer).expect("encode");
    assert!(is_host_result_pointer(value.as_object().expect("object")));

    let mut extra = value.as_object().expect("object").clone();
    extra.insert("verdict".into(), serde_json::json!("green"));
    assert!(
        !is_host_result_pointer(&extra),
        "an extra key is a different claim"
    );

    let mut wrong_schema = value.as_object().expect("object").clone();
    wrong_schema.insert("schema".into(), serde_json::json!("something-else/v1"));
    assert!(!is_host_result_pointer(&wrong_schema));

    let mut missing = value.as_object().expect("object").clone();
    missing.remove("resultEventId");
    assert!(!is_host_result_pointer(&missing));

    assert!(!is_host_result_pointer(
        serde_json::json!({"operationId": "ab".repeat(32), "type": "assignment"})
            .as_object()
            .expect("object")
    ));
}

/// A host-result wake is framed and fenced exactly as every other
/// provider-minted pointer is. Without this the same fact arrives with a
/// `[Context]` block or without one depending on whose key started the host,
/// which is COMMS-MAP finding 3 all over again.
#[test]
fn a_host_result_wake_is_a_team_wake_pointer_for_framing_and_the_fence() {
    let fixture = Fixture::new();
    let host = Keys::generate();
    let result = fixture.result_event(&host, RUN_ID);
    let exited = buzz_core::host_step::decode_host_step_exited(&fixture.exited_for(&result, &host))
        .expect("decode");
    let text = crate::host_result_wake::wake_text(&exited).expect("encode");

    assert!(
        crate::team_wake::is_team_wake_pointer(&text),
        "the framing path must know this is provider-minted, not operator prose"
    );
    let key = crate::team_wake::operation_fence_key(&fixture.target, &text)
        .expect("a wake pointer has an operation identity");
    assert_eq!(
        Some(key),
        crate::team_wake::operation_fence_key(&fixture.target, &text),
        "the fence key is stable"
    );
    assert!(
        crate::team_wake::operation_fence_key(&fixture.target, "just some prose").is_none(),
        "prose is never fenced"
    );
}

/// A provider that is neither the founder nor a granted operator of the seat
/// must not publish a wake it has no authority to send.
///
/// Not merely because the relay would refuse it: an attempt would spend this
/// result's at-most-once claim, and the host that *does* hold the grant could
/// then never answer. So the claim is left untaken too.
#[tokio::test]
async fn a_provider_with_no_authority_over_the_seat_attempts_nothing() {
    let mut fixture = Fixture::new();
    let host = Keys::generate();
    let session_id = fixture
        .provider
        .state
        .sessions()
        .next()
        .expect("the seat")
        .session_id
        .clone();
    fixture
        .provider
        .state
        .update_session(&session_id, |record| {
            record.founder_pubkey = Some(Keys::generate().public_key().to_hex());
            record.granted_operators.clear();
        })
        .expect("re-found the seat under a stranger");

    fixture
        .deliver(fixture.workflow_channel, &fixture.requested())
        .await;
    let result = fixture.result_event(&host, RUN_ID);
    let exited = fixture.exited_for(&result, &host);
    fixture.deliver(fixture.workflow_channel, &exited).await;

    assert_eq!(fixture.queued_commands(), 0);
    assert!(
        !fixture
            .provider
            .host_result_wakes
            .already_waked(&result.id.to_hex()),
        "an unauthorised host must leave the result for one that can answer it"
    );
}

/// A granted operator may wake, exactly as the founder may.
#[tokio::test]
async fn a_granted_operator_wakes_the_seat() {
    let mut fixture = Fixture::new();
    let host = Keys::generate();
    let provider_key = fixture.provider.pubkey_hex.clone();
    let session_id = fixture
        .provider
        .state
        .sessions()
        .next()
        .expect("the seat")
        .session_id
        .clone();
    fixture
        .provider
        .state
        .update_session(&session_id, |record| {
            record.founder_pubkey = Some(Keys::generate().public_key().to_hex());
            record.granted_operators.insert(provider_key);
        })
        .expect("grant this provider");

    fixture
        .deliver(fixture.workflow_channel, &fixture.requested())
        .await;
    let result = fixture.result_event(&host, RUN_ID);
    let exited = fixture.exited_for(&result, &host);
    fixture.deliver(fixture.workflow_channel, &exited).await;

    assert!(fixture.wake_queued(&result.id.to_hex()));
}

/// Ledger 250, kettle-control-2 run b60be720: this host refused `verify`
/// before claiming (46023 `dd359da3…`, `ACTION_UNKNOWN`), no kind:46014
/// followed, and the lead sat idle 2 h 47 m. The relay now echoes the
/// refusal; the echo wakes the seat once, with the code and the message.
#[tokio::test]
async fn a_refusal_before_any_claim_wakes_the_seat_with_the_refusal() {
    let mut fixture = Fixture::new();
    let host = Keys::generate();
    fixture
        .deliver(fixture.workflow_channel, &fixture.requested())
        .await;
    let mut refused = fixture.result(RUN_ID);
    refused.claim_event_id = None;
    refused.disposition = HostStepDisposition::Refused;
    refused.exit_code = None;
    refused.duration_ms = None;
    refused.head_sha = None;
    refused.dirty = None;
    refused.stdout_tail = String::new();
    refused.refusal = Some(buzz_core::host_step::HostStepRefusal {
        code: "ACTION_UNKNOWN".into(),
        message: "actions.yml in the agents repository (42bc7697) has no action named \"verify\""
            .into(),
    });
    let (tags, content) = build_host_step_result(&refused).expect("build the refusal");
    let result = signed(&host, KIND_HOST_STEP_RESULT, tags, content);
    let exited = fixture.exited_for(&result, &host);

    fixture.deliver(fixture.workflow_channel, &exited).await;
    fixture.deliver(fixture.workflow_channel, &exited).await;
    assert_eq!(fixture.queued_commands(), 1, "one refusal is one wake");
    assert!(fixture.wake_queued(&result.id.to_hex()));

    let queued = fixture
        .provider
        .outbox
        .pending_events()
        .next()
        .expect("one queued wake");
    let payload: CodingSessionCommandPayload =
        serde_json::from_str(&queued.content).expect("a session command");
    let CodingSessionAction::ThreadTurnStart { text, .. } = payload.action else {
        panic!("a wake is a turn start");
    };
    let pointer: serde_json::Value = serde_json::from_str(&text).expect("JSON");
    let object = pointer.as_object().expect("an object");
    assert!(is_host_result_pointer(object), "{text}");
    assert_eq!(object["disposition"], "refused");
    assert_eq!(object["refusalCode"], "ACTION_UNKNOWN");
    assert!(
        object["refusalMessage"]
            .as_str()
            .is_some_and(|message| message.contains("no action named")),
        "{text}"
    );
    assert!(object.get("exitCode").is_none());
}
