//! The exact bytes a continuation turn delivers.
//!
//! Pinned as a whole string rather than field by field: the delivered prompt is
//! the *only* thing the woken agent sees, and a silent reordering or a dropped
//! field would change what it believes about a build without failing anything
//! else.

use super::*;

use buzz_core::ci_result::CiPhase;

fn record() -> CiContinuationRecord {
    let identity = CiResultIdentity {
        project: format!("30621:{}:beekeeper", "11".repeat(32)),
        repository: format!("30617:{}:beekeeper", "11".repeat(32)),
        commit: "ab".repeat(20),
        check: "main-validation".into(),
        run: "136".into(),
        attempt: 1,
        workflow: "6f1a2f1e-0b3c-4a5d-8e9f-0a1b2c3d4e5f".into(),
        phase: CiPhase::Build,
    };
    let correlation_id =
        buzz_core::ci_result::correlation_id(&identity).expect("valid identity digest");
    CiContinuationRecord {
        command_id: "cic-0123456789abcdef".into(),
        registration_event_id: "cc".repeat(32),
        payload_digest: "dd".repeat(32),
        channel_id: Uuid::nil(),
        signer: "ee".repeat(32),
        target: CodingSessionTarget {
            driver: "claude".into(),
            instance_id: "instance-1".into(),
            session_id: "session-1".into(),
            generation: 3,
        },
        identity,
        correlation_id,
        continuation: "open a PR with the fix".into(),
        expires_at: 1_788_800_000,
        registered_at: 1_788_700_000,
        relay_answered: true,
        attempts: 0,
        next_check_at: 0,
        last_obstacle: None,
        state: RecordState::Waiting,
    }
}

fn ready(
    record: &CiContinuationRecord,
    evidence: Option<&str>,
    summary: Option<&str>,
) -> ReadyResult {
    let result = CiResult {
        schema: buzz_core::ci_result::CI_RESULT_SCHEMA.into(),
        identity: record.identity.clone(),
        conclusion: CiConclusion::Failure,
        evidence_url: evidence.map(str::to_owned),
        summary: summary.map(str::to_owned),
    };
    ReadyResult {
        result_event_id: "ff".repeat(32),
        result_signer: "ab".repeat(32),
        result_canonical_json: serde_json::to_string(&result).expect("encode result"),
        observed_at: 1_788_750_000,
    }
}

#[test]
fn the_delivered_prompt_is_exactly_the_materialized_result() {
    let record = record();
    let ready = ready(
        &record,
        Some("https://ci.agiterra.org/runs/136"),
        Some("2 tests failed"),
    );
    let text = materialize(&record, &ready).expect("materialize");
    let expected = format!(
        r#"{{
  "type": "ci_result",
  "operationId": "{operation_id}",
  "registration": {{
    "commandId": "cic-0123456789abcdef",
    "signer": "{signer}",
    "registeredAt": 1788700000,
    "expiresAt": 1788800000
  }},
  "result": {{
    "eventId": "{event_id}",
    "signer": "{result_signer}",
    "observedAt": 1788750000,
    "identity": {{
      "project": "{project}",
      "repository": "{repository}",
      "commit": "{commit}",
      "check": "main-validation",
      "run": "136",
      "attempt": 1,
      "workflow": "6f1a2f1e-0b3c-4a5d-8e9f-0a1b2c3d4e5f",
      "phase": "build"
    }},
    "conclusion": "failure",
    "evidenceUrl": "https://ci.agiterra.org/runs/136",
    "summary": "2 tests failed"
  }},
  "continuation": "open a PR with the fix"
}}"#,
        operation_id = record.correlation_id,
        signer = record.signer,
        event_id = ready.result_event_id,
        result_signer = ready.result_signer,
        project = record.identity.project,
        repository = record.identity.repository,
        commit = record.identity.commit,
    );
    assert_eq!(text, expected);
}

#[test]
fn absent_evidence_and_summary_keep_their_keys_as_null() {
    let record = record();
    let text = materialize(&record, &ready(&record, None, None)).expect("materialize");
    let value: serde_json::Value = serde_json::from_str(&text).expect("json");
    assert!(value["result"]["evidenceUrl"].is_null());
    assert!(value["result"]["summary"].is_null());
    assert!(
        value["result"].as_object().is_some_and(
            |result| result.contains_key("evidenceUrl") && result.contains_key("summary")
        ),
        "the shape a reading agent parses must not change with the content"
    );
}

#[test]
fn the_prompt_is_deterministic_across_encodings() {
    let record = record();
    let ready = ready(&record, Some("https://ci.agiterra.org/runs/136"), None);
    let first = materialize(&record, &ready).expect("first");
    // The same record, with the stored result re-encoded through a value map
    // (which reorders keys), must still produce byte-identical text.
    let reordered: serde_json::Value =
        serde_json::from_str(&ready.result_canonical_json).expect("json");
    let shuffled = ReadyResult {
        result_canonical_json: serde_json::to_string(&reordered).expect("reencode"),
        ..ready.clone()
    };
    assert_eq!(first, materialize(&record, &shuffled).expect("second"));
}

#[test]
fn the_fence_pointer_is_the_compact_form_and_never_the_prompt() {
    let record = record();
    let pointer = ci_continuation_pointer(&record.correlation_id);
    assert_eq!(
        pointer,
        format!(
            r#"{{"operationId":"{}","type":"ci_result"}}"#,
            record.correlation_id
        )
    );
    // The pointer is a recognised operation pointer; the prompt is not, which
    // is exactly why the fence must be told the pointer explicitly.
    let target = record.target.clone();
    assert!(crate::team_wake::operation_fence_key(&target, &pointer).is_some());
    let prompt = materialize(&record, &ready(&record, None, None)).expect("materialize");
    assert!(crate::team_wake::operation_fence_key(&target, &prompt).is_none());
}

#[test]
fn an_undecodable_stored_result_is_an_error_rather_than_a_prompt() {
    let record = record();
    let broken = ReadyResult {
        result_canonical_json: "{\"schema\":\"buzz-ci-result/v1\"}".into(),
        ..ready(&record, None, None)
    };
    assert!(materialize(&record, &broken).is_err());
}
