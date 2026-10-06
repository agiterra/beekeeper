//! The one fixture both surfaces are pinned to.
//!
//! Split out of `pulse_declared_work_tests.rs` so no file here passes 1,000
//! lines — the same split `pulse_mission_tests.rs` uses. A child module, so it
//! reads its parent's helpers and imports unchanged.

use super::*;

// ── The one fixture both surfaces are pinned to ──────────────────────────────
//
// Written from the **real** projection over **real signed events**, never
// hand-composed, so the bytes the TypeScript decoder is pinned to cannot drift
// from what this module actually emits (`pulse_mission_fixture_tests.rs`'s
// pattern: the Rust test writes the fixture the TS test reads). Every key is
// fixed, so every event id — and therefore every byte — is stable.

const FIXTURE_VIEWER: &str = "3d3b7169aa11c2d0f0b7a1e6d5c4b3a2918070605040302010fedcba98765432";

/// A key from a fixed secret, so the fixture's ids never move.
fn fixed_keys(secret: &str) -> Keys {
    Keys::parse(secret).expect("a fixed fixture key")
}

/// The open umbrella: one assignment, one report, one `changes-requested`.
fn fixture_open_session() -> PulseDeclaredWorkSession {
    let channel = "05ef0ecf-745f-5fb8-b7ff-f9cba21e01c2";
    let session_ref = "0f1e2d3c-4b5a-4978-8796-a5b4c3d2e1f0";
    let genesis = "aa".repeat(32);
    let founder = fixed_keys(&"11".repeat(32));
    let actor = fixed_keys(&"22".repeat(32));
    let context = context_for(
        channel,
        session_ref,
        &genesis,
        &founder,
        vec![(&actor, "builder")],
    );

    let assignment = signed(
        channel,
        &payload(
            session_ref,
            &genesis,
            assignment_body(&actor, "Build the declared-work wire"),
        ),
        &founder,
        1_756_790_160,
    );
    let assignment_id = assignment.id.to_hex();
    let report = signed(
        channel,
        &payload(session_ref, &genesis, report_body(&assignment_id)),
        &actor,
        1_756_798_440,
    );
    let report_id = report.id.to_hex();
    let disposition = signed(
        channel,
        &payload(
            session_ref,
            &genesis,
            disposition_body(
                &assignment_id,
                &report_id,
                CodingSessionTeamDispositionDecision::ChangesRequested,
            ),
        ),
        &founder,
        1_756_800_120,
    );
    let events = vec![assignment, report, disposition];

    project_declared_work(&PulseDeclaredWorkSources {
        session_key: session_ref,
        channel_id: channel,
        session_ref,
        name: Some("Declared work in Pulse"),
        lifecycle: PulseDeclaredWorkLifecycle::Open,
        latest_observation_at: Some(1_756_800_600),
        context: &context,
        team_events: &events,
    })
}

/// The closed umbrella: one settled assignment under a `mission.completed`.
fn fixture_closed_session() -> PulseDeclaredWorkSession {
    let channel = "1a2b3c4d-5e6f-4071-8293-a4b5c6d7e8f9";
    let session_ref = "2b3c4d5e-6f70-4182-93a4-b5c6d7e8f901";
    let genesis = "bb".repeat(32);
    let founder = fixed_keys(&"33".repeat(32));
    let actor = fixed_keys(&"44".repeat(32));
    let context = context_for(
        channel,
        session_ref,
        &genesis,
        &founder,
        vec![(&actor, "builder")],
    );

    let assignment = signed(
        channel,
        &payload(
            session_ref,
            &genesis,
            assignment_body(&actor, "Land the session read"),
        ),
        &founder,
        1_756_700_000,
    );
    let assignment_id = assignment.id.to_hex();
    let report = signed(
        channel,
        &payload(session_ref, &genesis, report_body(&assignment_id)),
        &actor,
        1_756_710_000,
    );
    let report_id = report.id.to_hex();
    let disposition = signed(
        channel,
        &payload(
            session_ref,
            &genesis,
            disposition_body(
                &assignment_id,
                &report_id,
                CodingSessionTeamDispositionDecision::Approve,
            ),
        ),
        &founder,
        1_756_720_000,
    );
    let acknowledgement = signed(
        channel,
        &payload(
            session_ref,
            &genesis,
            acknowledgement_body(&disposition.id.to_hex()),
        ),
        &actor,
        1_756_730_000,
    );
    let completion = signed(
        channel,
        &payload(session_ref, &genesis, completed_body(&assignment_id)),
        &founder,
        1_756_740_000,
    );
    let events = vec![assignment, report, disposition, acknowledgement, completion];

    project_declared_work(&PulseDeclaredWorkSources {
        session_key: session_ref,
        channel_id: channel,
        session_ref,
        name: None,
        lifecycle: PulseDeclaredWorkLifecycle::Closed,
        latest_observation_at: Some(1_756_740_000),
        context: &context,
        team_events: &events,
    })
}

/// The whole response one page produces, exactly as the adapter assembles it.
fn fixture_response() -> PulseDeclaredWork {
    PulseDeclaredWork {
        schema: PULSE_DECLARED_WORK_SCHEMA.to_owned(),
        viewer_pubkey: Some(FIXTURE_VIEWER.to_owned()),
        sessions: vec![fixture_open_session(), fixture_closed_session()],
        errors: vec![PulseDeclaredWorkError {
            scope: "declared:3c4d5e6f-7081-4293-a4b5-c6d7e8f90123".to_owned(),
            message: "relay closed the subscription before the 44244 page finished".to_owned(),
        }],
    }
}

#[test]
fn the_fixture_carries_the_two_shapes_the_surface_must_tell_apart() {
    let response = fixture_response();
    assert_eq!(response.schema, PULSE_DECLARED_WORK_SCHEMA);
    assert_eq!(response.sessions.len(), 2);

    let open = &response.sessions[0];
    assert_eq!(open.lifecycle, PulseDeclaredWorkLifecycle::Open);
    assert!(open.terminal.is_none());
    assert_eq!(open.assignments.len(), 1);
    assert_eq!(
        open.assignments[0].status,
        PulseDeclaredAssignmentStatus::Reported
    );
    assert_eq!(open.assignments[0].reports.len(), 1);
    assert_eq!(
        open.assignments[0].dispositions[0].decision,
        CodingSessionTeamDispositionDecision::ChangesRequested
    );

    let closed = &response.sessions[1];
    assert_eq!(closed.lifecycle, PulseDeclaredWorkLifecycle::Closed);
    assert_ne!(
        open.genesis_ref, closed.genesis_ref,
        "two umbrellas are two genesis records, and the fixture proves the \
         details block can tell them apart"
    );
    assert_eq!(
        closed
            .terminal
            .as_ref()
            .map(|terminal| terminal.terminal_type.as_str()),
        Some("mission.completed")
    );
    assert_eq!(
        closed.assignments[0].status,
        PulseDeclaredAssignmentStatus::Settled
    );
}

#[test]
fn the_typescript_fixture_is_the_json_this_module_produces() {
    // Compared as JSON rather than as bytes because the checked-in file is
    // formatted by the Desktop formatter, which owns its whitespace and key
    // wrapping. Every key and every value must still match exactly.
    let produced = serde_json::to_value(fixture_response()).expect("serialize");
    let on_disk: serde_json::Value = serde_json::from_str(include_str!(
        "../../../desktop/src/features/project-pulse/lib/pulseDeclaredWork.fixture.json"
    ))
    .expect("the fixture is valid JSON");
    if produced != on_disk {
        // The temp dir rather than a repo-relative `target/`: a lane building
        // with its own `CARGO_TARGET_DIR` has no repo-root `target/` at all,
        // and a disclosure that fails to write is a drift nobody can repair.
        let path = std::env::temp_dir().join("pulseDeclaredWork.fixture.produced.json");
        let _ = std::fs::write(
            &path,
            serde_json::to_string_pretty(&produced).expect("pretty") + "\n",
        );
        panic!(
            "the declared-work fixture has drifted from what this module produces; the current \
             bytes were written to {}",
            path.display()
        );
    }
}
