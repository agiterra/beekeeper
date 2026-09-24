//! The host's automatic evidence binding (ledger 257(d)): what it signs, that
//! the records are the CLI's records under this host's key, and every case in
//! which it binds nothing.

use std::cell::Cell;
use std::time::Duration;

use buzz_core::kind::KIND_GIT_REPO_STATE;
use buzz_core::project_work::{
    decode_project_work_content, ProjectWorkBody, ProjectWorkDeclared, ProjectWorkEvidenceKind,
};
use buzz_sdk::project_work::build_project_work_declared;

use super::*;
use crate::host_result_wake::is_host_result_pointer;
use crate::host_result_wake::HostResultWakeStore;

const KETTLE: &str =
    include_str!("../../../conformance/project-work/fixtures/plans/valid/kettle.md");

const REVIEW_ONLY: &str = r#"---
schema: beekeeper-plan/v1
id: kettle-cli
status: in-force
title: Kettle
code_repository: pivot-test
delivery_ref: refs/heads/main
criteria:
  - id: cli-behaviour
    accept: the CLI does what the plan says
    proof: {kind: review}
  - id: delivered-main
    accept: it is on main
    proof: {kind: git-ref}
retired_criteria: []
---

Body.
"#;

const DELIVERED: &str = "d7b4a649c3399e5c4da95753220bb6712bcfae5e";
const SEED: &str = "7070283000000000000000000000000000000000";
const RESULT: &str = "4623000000000000000000000000000000000000000000000000000000000000";
const REF_STATE: &str = "3061800000000000000000000000000000000000000000000000000000000000";

fn envelope() -> ProjectWorkEnvelope {
    ProjectWorkEnvelope {
        channel_ref: "56ef0396-f1d0-4cbd-9cc2-4f4fb592e823".into(),
        session_ref: "ce7d32cb-b703-4db8-b06e-2adbd2b942b3".into(),
        genesis_ref: "12".repeat(32),
        project_ref: format!("30621:{}:kettle", "3d".repeat(32)),
    }
}

/// A fake relay and clone: each read answers from a field and counts itself.
struct Fixture {
    may_bind: bool,
    plan: &'static str,
    records: Vec<ProjectWorkEvent>,
    delivered: &'static str,
    relay: Keys,
    ref_reads: Cell<u32>,
    /// Reads answered with no rows before the record is served.
    empty_reads: u32,
    /// Reads answered with a ref state that exists but names no
    /// `refs/heads/main`, before the record naming it is served — the lag
    /// shape a push's own ref-state write has not caught up with yet.
    no_delivery_ref_reads: u32,
    /// Whether the project's kind:30621 is served to this key.
    project_served: bool,
    pauses: Cell<u32>,
}

impl Fixture {
    fn new(lead: &Keys) -> Self {
        let declared = build_project_work_declared(
            &envelope(),
            ProjectWorkDeclared {
                work_id: "0b6f6a3c-7c1a-5a8e-9c2d-0a1b2c3d4e5f".into(),
                goal_ref: "cc".repeat(32),
                decision_ref: None,
                responsible_actor: lead.public_key().to_hex(),
                plan_ref: ProjectWorkPlanRef {
                    repository: format!("30617:{}:kettle-agents", "3d".repeat(32)),
                    commit: "21ece7f946797575e79902ae8b1002913be0152c".into(),
                    path: "plans/kettle.md".into(),
                },
                supersedes: Vec::new(),
            },
        )
        .expect("declared")
        .sign_with_keys(lead)
        .expect("sign");
        Self {
            may_bind: true,
            plan: KETTLE,
            records: vec![ProjectWorkEvent::from(&declared)],
            delivered: DELIVERED,
            relay: Keys::generate(),
            ref_reads: Cell::new(0),
            empty_reads: 0,
            no_delivery_ref_reads: 0,
            project_served: true,
            pauses: Cell::new(0),
        }
    }

    fn input(&self, keys: &Keys, relay_self: &str) -> AutoEvidenceInput {
        AutoEvidenceInput {
            keys: keys.clone(),
            relay_self: relay_self.to_string(),
            envelope: envelope(),
            result: HostResultFacts {
                result_event_id: RESULT.into(),
                action_name: "verify".into(),
                step_id: "verify".into(),
                exited: true,
                exit_code: Some(0),
                dirty: Some(false),
                head_sha: Some(DELIVERED.into()),
            },
            retry: RefReadRetry {
                attempts: 3,
                interval: Duration::from_secs(2),
            },
        }
    }
}

impl AutoEvidenceReads for Fixture {
    async fn provider_may_bind(&self, _provider: &str) -> Result<bool, String> {
        Ok(self.may_bind)
    }

    async fn work_records(&self) -> Result<Vec<ProjectWorkEvent>, String> {
        Ok(self.records.clone())
    }

    async fn plan_blob(&self, _plan_ref: &ProjectWorkPlanRef) -> Result<String, String> {
        Ok(self.plan.to_owned())
    }

    async fn ref_states(&self, repository: &str) -> Result<RefStateRows, String> {
        self.ref_reads.set(self.ref_reads.get() + 1);
        if self.ref_reads.get() <= self.empty_reads {
            return Ok(RefStateRows::default());
        }
        if self.ref_reads.get() <= self.no_delivery_ref_reads {
            let states = vec![ProjectWorkEvent {
                id: REF_STATE.into(),
                pubkey: self.relay.public_key().to_hex(),
                created_at: 100,
                kind: KIND_GIT_REPO_STATE,
                tags: vec![vec!["d".into(), repository.into()]],
                content: String::new(),
            }];
            return Ok(RefStateRows { rows: 1, states });
        }
        let states = vec![ProjectWorkEvent {
            id: REF_STATE.into(),
            pubkey: self.relay.public_key().to_hex(),
            created_at: 100,
            kind: KIND_GIT_REPO_STATE,
            tags: vec![
                vec!["d".into(), repository.into()],
                vec!["refs/heads/main".into(), self.delivered.into()],
            ],
            content: String::new(),
        }];
        Ok(RefStateRows { rows: 1, states })
    }

    async fn project_served(&self, _project_ref: &str) -> Result<bool, String> {
        Ok(self.project_served)
    }

    async fn pause(&self, interval: Duration) {
        assert_eq!(interval, Duration::from_secs(2));
        self.pauses.set(self.pauses.get() + 1);
    }
}

fn body_of(event: &Event) -> ProjectWorkEvidenceBound {
    match decode_project_work_content(&event.content)
        .expect("decodes")
        .body
    {
        ProjectWorkBody::EvidenceBound(body) => body,
        other => panic!("not an evidence binding: {other:?}"),
    }
}

#[tokio::test]
async fn a_green_delivered_verify_is_bound_under_this_hosts_key() {
    let lead = Keys::generate();
    let provider = Keys::generate();
    let fixture = Fixture::new(&lead);
    let relay_self = fixture.relay.public_key().to_hex();
    let prepared = prepare(&fixture, &fixture.input(&provider, &relay_self)).await;

    assert_eq!(prepared.summary.skipped, None, "{:?}", prepared.summary);
    assert_eq!(prepared.events.len(), 2);
    assert_eq!(prepared.summary.signed_by, provider.public_key().to_hex());
    assert_eq!(prepared.summary.host_result, RESULT);
    assert_eq!(prepared.summary.ref_state.as_deref(), Some(REF_STATE));
    for event in &prepared.events {
        assert_eq!(event.pubkey, provider.public_key(), "never the seat's key");
        event.verify().expect("a valid signature");
        validate_project_work_envelope(&ProjectWorkEvent::from(event))
            .expect("a record the relay admits");
    }
    let action = body_of(&prepared.events[0]);
    assert_eq!(action.criterion_ids, vec!["verified-landed-revision"]);
    assert_eq!(action.artifact_commit, DELIVERED);
    assert_eq!(
        action.evidence_refs[0].kind,
        ProjectWorkEvidenceKind::ActionResult
    );
    assert_eq!(action.evidence_refs[0].event_id, RESULT);
    assert_eq!(action.declaration_ref, fixture.records[0].id);
    let git_ref = body_of(&prepared.events[1]);
    assert_eq!(git_ref.criterion_ids, vec!["delivered-main"]);
    assert_eq!(
        git_ref.evidence_refs[0].kind,
        ProjectWorkEvidenceKind::RefObservation
    );
    assert_eq!(git_ref.evidence_refs[0].event_id, REF_STATE);
    assert!(prepared
        .summary
        .bound
        .iter()
        .all(|binding| binding.published));
    assert_eq!(
        prepared.summary.bound[0].evidence,
        format!("action_result:{RESULT}")
    );
}

#[tokio::test]
async fn the_same_result_twice_publishes_one_set_of_records() {
    let lead = Keys::generate();
    let provider = Keys::generate();
    let mut fixture = Fixture::new(&lead);
    let relay_self = fixture.relay.public_key().to_hex();
    let first = prepare(&fixture, &fixture.input(&provider, &relay_self)).await;
    assert_eq!(first.events.len(), 2);

    // The durable custody: a second arrival of the same result id, in this
    // process or after a restart, is refused before any read.
    let dir = tempfile::tempdir().expect("tempdir");
    let mut store = HostResultWakeStore::open(dir.path()).expect("store");
    assert!(store.claim_evidence(RESULT).expect("first claim"));
    assert!(!store.claim_evidence(RESULT).expect("second claim"));
    let reopened = HostResultWakeStore::open(dir.path()).expect("reopen");
    assert!(
        reopened.evidence_checked(RESULT),
        "custody survives a restart"
    );

    // And if custody were ever lost, the records already on the wire are
    // found and nothing is republished.
    fixture
        .records
        .extend(first.events.iter().map(ProjectWorkEvent::from));
    let second = prepare(&fixture, &fixture.input(&provider, &relay_self)).await;
    assert!(second.events.is_empty(), "{:?}", second.summary);
    assert_eq!(second.summary.bound.len(), 2);
    assert!(second
        .summary
        .bound
        .iter()
        .all(|binding| !binding.published));
}

#[tokio::test]
async fn a_failing_result_binds_nothing() {
    let lead = Keys::generate();
    let provider = Keys::generate();
    let fixture = Fixture::new(&lead);
    let relay_self = fixture.relay.public_key().to_hex();
    let mut input = fixture.input(&provider, &relay_self);
    input.result.exit_code = Some(1);
    let prepared = prepare(&fixture, &input).await;
    assert!(prepared.events.is_empty());
    assert!(prepared.summary.bound.is_empty());
    assert_eq!(fixture.ref_reads.get(), 0, "refused before any ref read");
    assert!(prepared.summary.skipped.is_some());
}

#[tokio::test]
async fn a_result_at_a_commit_the_relay_has_not_seen_delivered_binds_nothing() {
    let lead = Keys::generate();
    let provider = Keys::generate();
    let mut fixture = Fixture::new(&lead);
    fixture.delivered = SEED;
    let relay_self = fixture.relay.public_key().to_hex();
    let prepared = prepare(&fixture, &fixture.input(&provider, &relay_self)).await;
    assert!(prepared.events.is_empty());
    assert!(prepared.summary.bound.is_empty());
    let skipped = prepared.summary.skipped.expect("says why");
    assert!(skipped.contains("names the delivery ref at"), "{skipped}");
}

#[tokio::test]
async fn a_plan_without_an_action_criterion_binds_nothing() {
    let lead = Keys::generate();
    let provider = Keys::generate();
    let mut fixture = Fixture::new(&lead);
    fixture.plan = REVIEW_ONLY;
    let relay_self = fixture.relay.public_key().to_hex();
    let prepared = prepare(&fixture, &fixture.input(&provider, &relay_self)).await;
    assert!(prepared.events.is_empty(), "not even the git-ref criterion");
    assert_eq!(fixture.ref_reads.get(), 0);
}

#[tokio::test]
async fn a_host_without_standing_in_the_session_binds_nothing() {
    let lead = Keys::generate();
    let provider = Keys::generate();
    let mut fixture = Fixture::new(&lead);
    fixture.may_bind = false;
    let relay_self = fixture.relay.public_key().to_hex();
    let prepared = prepare(&fixture, &fixture.input(&provider, &relay_self)).await;
    assert!(prepared.events.is_empty());
    assert!(prepared
        .summary
        .skipped
        .as_deref()
        .is_some_and(|why| why.contains("operator grantee")));
}

#[tokio::test]
async fn an_owner_signed_ref_state_is_not_an_observation() {
    let lead = Keys::generate();
    let provider = Keys::generate();
    let fixture = Fixture::new(&lead);
    // The fixture's ref state is signed by `fixture.relay`; name another key
    // as the relay and it becomes somebody's claim about their own branch.
    let impostor_relay = Keys::generate().public_key().to_hex();
    let prepared = prepare(&fixture, &fixture.input(&provider, &impostor_relay)).await;
    assert!(prepared.events.is_empty());
}

#[test]
fn a_wake_carrying_what_the_host_bound_is_still_a_host_result_pointer() {
    let summary = AutoEvidenceSummary {
        host_result: RESULT.into(),
        ref_state: Some(REF_STATE.into()),
        signed_by: "ab".repeat(32),
        bound: vec![AutoEvidenceBinding {
            event_id: "ef".repeat(32),
            criteria: vec!["verified-landed-revision".into()],
            evidence: format!("action_result:{RESULT}"),
            published: true,
        }],
        skipped: None,
        ref_read: Some(RefStateRead {
            kinds: vec![KIND_GIT_REPO_STATE],
            d: "pivot-test".into(),
            author: "1f".repeat(32),
            rows: 1,
            verified: 1,
            read_at: "2026-09-24T13:02:22Z".into(),
            attempts: 1,
            interval_secs: None,
            project_served: None,
        }),
    };
    let pointer = serde_json::json!({
        "schema": crate::host_result_wake::HOST_RESULT_WAKE_SCHEMA,
        "type": "host_result",
        "runId": "r",
        "stepId": "verify",
        "disposition": "exited",
        "exitCode": 0,
        "resultEventId": RESULT,
        "autoEvidence": summary,
    });
    assert!(is_host_result_pointer(pointer.as_object().expect("object")));
    assert_eq!(
        pointer["autoEvidence"]["refRead"],
        serde_json::json!({
            "kinds": [30618],
            "d": "pivot-test",
            "author": "1f".repeat(32),
            "rows": 1,
            "verified": 1,
            "readAt": "2026-09-24T13:02:22Z",
            "attempts": 1,
        }),
        "the read is machine-readable in the wake"
    );
    let mut wrong = pointer;
    wrong["autoEvidence"] = serde_json::json!("bound everything");
    assert!(!is_host_result_pointer(wrong.as_object().expect("object")));
}

#[tokio::test]
async fn an_empty_ref_read_is_read_again_and_binds_once_the_state_is_served() {
    let lead = Keys::generate();
    let provider = Keys::generate();
    let mut fixture = Fixture::new(&lead);
    fixture.empty_reads = 1;
    let relay_self = fixture.relay.public_key().to_hex();
    let first = prepare(&fixture, &fixture.input(&provider, &relay_self)).await;

    assert_eq!(first.summary.skipped, None, "{:?}", first.summary);
    assert_eq!(fixture.ref_reads.get(), 2);
    assert_eq!(fixture.pauses.get(), 1);
    assert_eq!(first.events.len(), 2, "both bindings, after the re-read");
    assert_eq!(first.summary.signed_by, provider.public_key().to_hex());
    assert_eq!(first.summary.ref_state.as_deref(), Some(REF_STATE));
    let read = first.summary.ref_read.clone().expect("the read is carried");
    assert_eq!((read.attempts, read.rows, read.verified), (2, 1, 1));
    assert_eq!(read.project_served, Some(true));
    let evidence: Vec<&str> = first
        .summary
        .bound
        .iter()
        .map(|binding| binding.evidence.as_str())
        .collect();
    assert_eq!(
        evidence,
        vec![
            format!("action_result:{RESULT}"),
            format!("ref_observation:{REF_STATE}")
        ]
    );

    // The same result again: the records are found, nothing is signed twice.
    fixture
        .records
        .extend(first.events.iter().map(ProjectWorkEvent::from));
    fixture.ref_reads.set(0);
    let second = prepare(&fixture, &fixture.input(&provider, &relay_self)).await;
    assert!(second.events.is_empty(), "{:?}", second.summary);
    assert!(second
        .summary
        .bound
        .iter()
        .all(|binding| !binding.published));
}

/// The realistic lag shape: a push landed and the relay signed *a* ref
/// state, but that write has not caught up with `refs/heads/main` yet. Ledger
/// 257 extends the same bounded re-read `NoState` already got.
#[tokio::test]
async fn a_ref_state_missing_the_delivery_ref_is_read_again_and_binds_once_it_is_named() {
    let lead = Keys::generate();
    let provider = Keys::generate();
    let mut fixture = Fixture::new(&lead);
    fixture.no_delivery_ref_reads = 1;
    let relay_self = fixture.relay.public_key().to_hex();
    let prepared = prepare(&fixture, &fixture.input(&provider, &relay_self)).await;

    assert_eq!(prepared.summary.skipped, None, "{:?}", prepared.summary);
    assert_eq!(
        fixture.ref_reads.get(),
        2,
        "one retry after the unnamed state"
    );
    assert_eq!(fixture.pauses.get(), 1);
    assert_eq!(prepared.events.len(), 2, "both bindings, after the re-read");
}

/// Bounded like every other re-read: three attempts, then a skip in words
/// rather than a silent wait forever.
#[tokio::test]
async fn a_ref_state_never_naming_the_delivery_ref_is_read_three_times_then_skipped() {
    let lead = Keys::generate();
    let provider = Keys::generate();
    let mut fixture = Fixture::new(&lead);
    fixture.no_delivery_ref_reads = u32::MAX;
    let relay_self = fixture.relay.public_key().to_hex();
    let prepared = prepare(&fixture, &fixture.input(&provider, &relay_self)).await;

    assert!(prepared.events.is_empty());
    assert_eq!(fixture.ref_reads.get(), 3);
    assert_eq!(fixture.pauses.get(), 2);
    let skipped = prepared.summary.skipped.expect("says why");
    assert!(skipped.contains("names no refs/heads/main"), "{skipped}");
    assert!(
        skipped.contains(", the last of 3 reads 2s apart"),
        "{skipped}"
    );
}

#[tokio::test]
async fn a_ref_state_never_served_is_read_three_times_and_the_wake_says_what_was_read() {
    let lead = Keys::generate();
    let provider = Keys::generate();
    let mut fixture = Fixture::new(&lead);
    fixture.empty_reads = u32::MAX;
    let relay_self = fixture.relay.public_key().to_hex();
    let prepared = prepare(&fixture, &fixture.input(&provider, &relay_self)).await;

    assert!(prepared.events.is_empty());
    assert_eq!(fixture.ref_reads.get(), 3);
    assert_eq!(fixture.pauses.get(), 2);
    let skipped = prepared.summary.skipped.expect("says what was read");
    let expected = format!(
        "0 rows for kinds=[30618] d=pivot-test author={}… at ",
        &relay_self[..8]
    );
    assert!(skipped.starts_with(&expected), "{skipped}");
    assert!(
        skipped.contains(", the last of 3 reads 2s apart"),
        "{skipped}"
    );
    assert!(
        !skipped.contains("has observed no"),
        "no inference: {skipped}"
    );
    let read = prepared.summary.ref_read.expect("machine-readable read");
    assert_eq!((read.rows, read.verified, read.attempts), (0, 0, 3));
    assert_eq!(read.author, relay_self);
    assert!(
        chrono::DateTime::parse_from_rfc3339(&read.read_at).is_ok(),
        "{}",
        read.read_at
    );
}

#[tokio::test]
async fn a_project_the_relay_withholds_from_this_key_is_not_waited_on() {
    let lead = Keys::generate();
    let provider = Keys::generate();
    let mut fixture = Fixture::new(&lead);
    fixture.empty_reads = u32::MAX;
    fixture.project_served = false;
    let relay_self = fixture.relay.public_key().to_hex();
    let prepared = prepare(&fixture, &fixture.input(&provider, &relay_self)).await;

    assert!(prepared.events.is_empty());
    assert_eq!(fixture.ref_reads.get(), 1, "the gate is not lag");
    assert_eq!(fixture.pauses.get(), 0);
    let skipped = prepared.summary.skipped.expect("says why");
    assert!(
        skipped.contains(&format!(
            "the relay also serves this key ({}…) no kind:30621 for {}",
            &provider.public_key().to_hex()[..8],
            envelope().project_ref
        )),
        "{skipped}"
    );
    assert_eq!(
        prepared
            .summary
            .ref_read
            .and_then(|read| read.project_served),
        Some(false)
    );
}

/// The production read, [`RelayReads`], through the real
/// [`buzz_acp::relay::RestClient`] against an HTTP server answering
/// `POST /query` the way the relay's NIP-MP gate does in control run 6:
/// a private project's repository events and its kind:30621 are served to a
/// key on its roster, and to any other key the answer is an empty array.
mod over_http {
    use std::net::SocketAddr;
    use std::sync::{Arc, Mutex};

    use axum::extract::State;
    use axum::http::HeaderMap;
    use axum::routing::post;
    use axum::{Json, Router};
    use base64::Engine as _;
    use nostr::{EventBuilder, Kind, Tag};

    use super::*;

    #[derive(Clone)]
    struct Gate {
        roster: Vec<String>,
        served: Vec<Event>,
        filters: Arc<Mutex<Vec<serde_json::Value>>>,
    }

    async fn query(
        State(gate): State<Gate>,
        headers: HeaderMap,
        Json(filters): Json<serde_json::Value>,
    ) -> Json<serde_json::Value> {
        gate.filters.lock().expect("filters").push(filters.clone());
        let reader = headers
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Nostr "))
            .and_then(|token| base64::engine::general_purpose::STANDARD.decode(token).ok())
            .and_then(|json| serde_json::from_slice::<Event>(&json).ok())
            .map(|auth| auth.pubkey.to_hex())
            .unwrap_or_default();
        if !gate.roster.contains(&reader) {
            return Json(serde_json::json!([]));
        }
        let filter = &filters[0];
        let kind = filter["kinds"][0].as_u64().unwrap_or_default();
        let rows: Vec<&Event> = gate
            .served
            .iter()
            .filter(|event| u64::from(event.kind.as_u16()) == kind)
            .collect();
        Json(serde_json::to_value(rows).expect("rows"))
    }

    async fn serve(gate: Gate) -> SocketAddr {
        let app = Router::new().route("/query", post(query)).with_state(gate);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let address = listener.local_addr().expect("address");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });
        address
    }

    fn signed(keys: &Keys, kind: u32, tags: &[&[&str]]) -> Event {
        EventBuilder::new(Kind::Custom(kind as u16), "")
            .tags(
                tags.iter()
                    .map(|parts| Tag::parse(parts.iter().copied()).expect("tag")),
            )
            .sign_with_keys(keys)
            .expect("sign")
    }

    fn rest(address: SocketAddr, keys: &Keys) -> buzz_acp::relay::RestClient {
        buzz_acp::relay::RestClient {
            http: reqwest::Client::new(),
            base_url: format!("http://{address}"),
            keys: keys.clone(),
            auth_tag_json: None,
        }
    }

    fn reads_as(client: &buzz_acp::relay::RestClient, relay_self: &str) -> RelayReads {
        RelayReads {
            rest: client.clone(),
            relay_self: relay_self.to_string(),
            scope: crate::team_wake::WakeScope {
                channel_ref: uuid::Uuid::new_v4(),
                session_ref: "s".into(),
                genesis_ref: "g".into(),
            },
            clones: Vec::new(),
        }
    }

    #[tokio::test]
    async fn the_relays_gate_answers_a_key_off_the_roster_nothing_and_the_read_says_so() {
        let relay = Keys::generate();
        let owner = Keys::generate();
        let lead = Keys::generate();
        let host = Keys::generate();
        let relay_self = relay.public_key().to_hex();
        let project_ref = format!("30621:{}:kettle-control-6", owner.public_key().to_hex());
        let state = signed(
            &relay,
            KIND_GIT_REPO_STATE,
            &[&["d", "kettle-control-6"], &["refs/heads/main", DELIVERED]],
        );
        let project = signed(
            &owner,
            KIND_PROJECT,
            &[&["d", "kettle-control-6"], &["buzz-access", "private"]],
        );
        let gate = Gate {
            roster: vec![lead.public_key().to_hex()],
            served: vec![state.clone(), project],
            filters: Arc::default(),
        };
        let address = serve(gate.clone()).await;
        let lead_rest = rest(address, &lead);
        let host_rest = rest(address, &host);

        // The lead, on the roster: one signature-valid relay-signed state.
        let lead_reads = reads_as(&lead_rest, &relay_self);
        let answer = lead_reads
            .ref_states("kettle-control-6")
            .await
            .expect("lead read");
        assert_eq!((answer.rows, answer.states.len()), (1, 1));
        assert_eq!(answer.states[0].id, state.id.to_hex());
        assert!(lead_reads
            .project_served(&project_ref)
            .await
            .expect("lead project read"));

        // The host, off the roster: the identical filter, zero rows.
        let host_reads = reads_as(&host_rest, &relay_self);
        let answer = host_reads
            .ref_states("kettle-control-6")
            .await
            .expect("host read");
        assert_eq!((answer.rows, answer.states.len()), (0, 0));
        assert!(!host_reads
            .project_served(&project_ref)
            .await
            .expect("host project read"));

        // Both reads sent the same filter: kinds, author, #d.
        let filters = gate.filters.lock().expect("filters").clone();
        assert_eq!(filters[0], filters[2], "{filters:?}");
        assert_eq!(filters[0][0]["kinds"], serde_json::json!([30618]));
        assert_eq!(filters[0][0]["authors"], serde_json::json!([relay_self]));
        assert_eq!(filters[0][0]["#d"], serde_json::json!(["kettle-control-6"]));

        // And through `prepare`'s ref read: one read, no waiting, the facts.
        let fixture = Fixture::new(&lead);
        let mut input = fixture.input(&host, &relay_self);
        input.envelope.project_ref = project_ref.clone();
        let (read, observed) =
            read_ref_state(&host_reads, &input, "kettle-control-6", "refs/heads/main")
                .await
                .expect("read");
        assert_eq!(observed, Err(RefObservationMissing::NoState));
        assert_eq!((read.rows, read.attempts), (0, 1));
        assert_eq!(read.project_served, Some(false));
        assert!(read.sentence().starts_with(&format!(
            "0 rows for kinds=[30618] d=kettle-control-6 author={}… at ",
            &relay_self[..8]
        )));
    }
}
