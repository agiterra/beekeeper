//! The one fixture both surfaces are pinned to.
//!
//! Split out of `pulse_mission_tests.rs` so no file here passes 1,000 lines.

use super::*;
use crate::coding_session_observation::{
    CodingSessionObservationGateOutcome, CodingSessionObservationPhase,
};

// ── The fixture both surfaces are pinned to ──────────────────────────────────

/// Build the exact rows the TypeScript decoder is pinned to.
///
/// Composed through the real renderer, never hand-written, so the fixture
/// cannot drift from what this module actually emits (L5.1's pattern: the Rust
/// test writes the fixture the TS test reads).
fn fixture_rows() -> PulseMissionRows {
    let viewer_key = "3d3b7169aa11c2d0f0b7a1e6d5c4b3a2918070605040302010fedcba98765432";
    fixture_rows_for(Some(viewer_key))
}

/// The same rows rendered for one viewer, or for a surface with no identity.
///
/// Parameterised rather than hand-edited, because the sentence a no-identity
/// surface shows is composed in Rust: a test that injected it would prove the
/// component and not the model — exactly what REVIEW-L9 §6 caught the e2e spec
/// doing.
fn fixture_rows_for(viewer_pubkey: Option<&str>) -> PulseMissionRows {
    let viewer = "3d3b7169aa11c2d0f0b7a1e6d5c4b3a2918070605040302010fedcba98765432";
    let bob = "11aa22bb33cc44dd55ee66ff7788990011223344556677889900112233445566";
    let ira = "22bb33cc44dd55ee66ff77889900112233445566778899001122334455667788";
    let names = PulseMissionNames {
        names: [
            (bob.to_owned(), "Bob".to_owned()),
            (ira.to_owned(), "Ira".to_owned()),
            (viewer.to_owned(), "Brian".to_owned()),
        ]
        .into_iter()
        .collect(),
        // The renderer knows exactly the viewer the response declares. An
        // earlier draft left this `None` while `viewerPubkey` was set — a
        // fixture claiming an identity the renderer did not have, which is the
        // contradiction REVIEW-L9 F4.1 surfaced from the other side.
        viewer: viewer_pubkey.map(str::to_owned),
    };
    let now = 1_756_800_960_i64;

    let mut running = PulseMissionFacts {
        session_key: "0f1e2d3c-4b5a-4978-8796-a5b4c3d2e1f0".into(),
        session_ref: Some("0f1e2d3c-4b5a-4978-8796-a5b4c3d2e1f0".into()),
        channel_id: "aa11bb22-cc33-4d44-ae55-ff6600778899".into(),
        name: Some("Route rail honesty".into()),
        latest_observation_at: Some(1_756_800_600),
        state: PulseMissionState::Running,
        unreadable: None,
        waiting: Some(PulseMissionRuling {
            session_key: "0f1e2d3c-4b5a-4978-8796-a5b4c3d2e1f0".into(),
            request_id: format!("2099cdb3{}", "44".repeat(28)),
            held_on: "founder".into(),
            asked_by: ira.into(),
            asked_at: Some(1_756_798_440),
            question: Some("Do we widen the push gate for seats?".into()),
        }),
        terminal_event_id: None,
        verdict: Some(PulseMissionVerdict {
            event_id: format!("5c6d7e8f{}", "aa".repeat(28)),
            author: ira.into(),
            token: "approve".into(),
        }),
        excluded_completion: None,
        policy: PulseMissionPolicy {
            author: Some(viewer.into()),
            withdrawn: false,
            posture: Some("ship".into()),
            budget_turns: Some(40),
            irreversible: vec!["push".into(), "deploy".into()],
        },
        seats: vec![
            PulseMissionSeat {
                pubkey: bob.into(),
                role: Some("builder".into()),
                checkpoint: Some(PulseMissionCheckpoint {
                    phase: CodingSessionObservationPhase::Green,
                    tests_written: 12,
                    tests_red: 12,
                    tests_green: 11,
                    at: Some(1_756_800_600),
                }),
                gates: vec![
                    PulseMissionGate {
                        gate: "cargo test".into(),
                        outcome: CodingSessionObservationGateOutcome::Failed,
                        command: "cargo test -p beekeeper-core".into(),
                        source: PulseGateSource::Observed,
                        over_declared: true,
                        event_id: format!("11112222{}", "33".repeat(28)),
                    },
                    PulseMissionGate {
                        gate: "clippy".into(),
                        outcome: CodingSessionObservationGateOutcome::NotRun,
                        command: "cargo clippy --all-targets -- -D warnings".into(),
                        source: PulseGateSource::Declared,
                        over_declared: false,
                        event_id: format!("44445555{}", "66".repeat(28)),
                    },
                ],
                gates_truncated: 2,
                owed: vec![PulseMissionOwed {
                    assignment_id: format!("4a5b6c7d{}", "88".repeat(28)),
                    assigned_at: Some(1_756_790_160),
                }],
                wip: Some(PulseMissionWip {
                    ref_name: "refs/heads/wip/builder/1f2e3d4c".into(),
                    sha: "9a1c4e7b2d3f40516273849506172839405a6b7c".into(),
                    as_of: Some(1_756_800_600),
                }),
            },
            PulseMissionSeat {
                pubkey: ira.into(),
                role: Some("verifier".into()),
                checkpoint: None,
                gates: Vec::new(),
                gates_truncated: 0,
                owed: Vec::new(),
                wip: None,
            },
        ],
        moved: vec![
            PulseMissionMoved {
                kind: PulseMovedKind::Wip,
                sha: "9a1c4e7b2d3f40516273849506172839405a6b7c".into(),
                ref_name: "refs/heads/wip/builder/1f2e3d4c".into(),
                author_pubkey: bob.into(),
                subject: Some("test: red for the overlap row".into()),
                age_seconds: Some(360),
                verdict: None,
            },
            PulseMissionMoved {
                kind: PulseMovedKind::Landing,
                sha: "c1d2e3f4a5b60718293a4b5c6d7e8f9001122334".into(),
                ref_name: "refs/heads/main".into(),
                author_pubkey: viewer.into(),
                subject: None,
                age_seconds: Some(5_400),
                verdict: None,
            },
        ],
        timing: vec![
            PulseMissionPhase {
                phase: "red".into(),
                duration_ms: Some(2_040_000),
            },
            PulseMissionPhase {
                phase: "green".into(),
                duration_ms: Some(4_320_000),
            },
        ],
        seat_claims_refused: Vec::new(),
        ref_state_present: true,
        gate_provenance_checked: true,
    };
    running
        .moved
        .sort_by(|left, right| left.ref_name.cmp(&right.ref_name));

    let blocked = PulseMissionFacts {
        session_key: "1a2b3c4d-5e6f-4071-8293-a4b5c6d7e8f9".into(),
        session_ref: Some("1a2b3c4d-5e6f-4071-8293-a4b5c6d7e8f9".into()),
        channel_id: "bb22cc33-dd44-4e55-af66-00778899aabb".into(),
        name: None,
        latest_observation_at: Some(1_756_800_100),
        state: PulseMissionState::Blocked,
        unreadable: None,
        waiting: None,
        terminal_event_id: Some(format!("7788aabb{}", "cc".repeat(28))),
        verdict: None,
        excluded_completion: Some(PulseMissionExcludedCompletion {
            event_id: format!("99aabbcc{}", "dd".repeat(28)),
            code: "completionNotApproved".into(),
        }),
        policy: PulseMissionPolicy::default(),
        seats: Vec::new(),
        moved: Vec::new(),
        timing: Vec::new(),
        seat_claims_refused: vec![format!("ee11ff22{}", "99".repeat(28))],
        ref_state_present: false,
        gate_provenance_checked: true,
    };

    let unreadable = PulseMissionFacts {
        session_key: "2b3c4d5e-6f70-4182-93a4-b5c6d7e8f901".into(),
        session_ref: None,
        channel_id: "cc33dd44-ee55-4f66-a077-8899aabbccdd".into(),
        name: Some("Unreadable umbrella".into()),
        latest_observation_at: None,
        state: PulseMissionState::Unreadable,
        unreadable: Some("duplicate supplied team transaction 4c5d6e7f".into()),
        waiting: None,
        terminal_event_id: None,
        verdict: None,
        excluded_completion: None,
        policy: PulseMissionPolicy::default(),
        seats: Vec::new(),
        moved: Vec::new(),
        timing: Vec::new(),
        seat_claims_refused: Vec::new(),
        ref_state_present: false,
        gate_provenance_checked: true,
    };

    let open = vec![
        running.waiting.clone().expect("the founder-held request"),
        PulseMissionRuling {
            session_key: "1a2b3c4d-5e6f-4071-8293-a4b5c6d7e8f9".into(),
            request_id: format!("30aadcb4{}", "55".repeat(28)),
            held_on: viewer.into(),
            asked_by: ira.into(),
            asked_at: None,
            question: None,
        },
    ];
    let founder_of = |_: &str| Some(String::new());
    let waiting_on_viewer = rulings_waiting_on_viewer(&open, viewer_pubkey, &founder_of);

    let overlaps = crate::pulse_overlap::render_pulse_overlap_rows(
        &crate::pulse_overlap::fold_pulse_overlaps(&[
            crate::pulse_overlap::PulseOverlapSide {
                session_key: "0f1e2d3c-4b5a-4978-8796-a5b4c3d2e1f0".into(),
                author_pubkey: bob.into(),
                sha: "9a1c4e7b2d3f40516273849506172839405a6b7c".into(),
                as_of: Some(1_756_800_600),
                files: vec![
                    "crates/beekeeper-core/src/pulse.rs".into(),
                    "justfile".into(),
                ],
            },
            crate::pulse_overlap::PulseOverlapSide {
                session_key: "1a2b3c4d-5e6f-4071-8293-a4b5c6d7e8f9".into(),
                author_pubkey: ira.into(),
                sha: "b7c8d9e0f1a2334455667788990011223344556f".into(),
                as_of: None,
                files: vec![
                    "crates/beekeeper-core/src/pulse.rs".into(),
                    "justfile".into(),
                ],
            },
        ]),
        &names,
        now,
    );

    PulseMissionRows {
        missions_schema: PULSE_MISSION_ROWS_SCHEMA.into(),
        mission_scope: PULSE_MISSION_SCOPE.into(),
        missions: [running, blocked, unreadable]
            .iter()
            .map(|facts| render_pulse_mission_lines(facts, &names, now))
            .collect(),
        mission_errors: vec![
            PulseMissionError {
                scope: "missions:cc33dd44-ee55-4f66-a077-8899aabbccdd".into(),
                message: "duplicate supplied team transaction 4c5d6e7f".into(),
            },
            pulse_mission_cap_disclosure(9).expect("the cap disclosure"),
        ],
        open_rulings: open,
        rulings_waiting_on_viewer: waiting_on_viewer,
        overlaps,
        viewer_pubkey: viewer_pubkey.map(str::to_owned),
    }
}

/// The rows a surface with no identity renders.
fn fixture_rows_no_identity() -> PulseMissionRows {
    fixture_rows_for(None)
}

#[test]
fn the_no_identity_fixture_is_the_json_this_module_produces() {
    let produced = serde_json::to_value(fixture_rows_no_identity()).expect("serialize");
    let on_disk: Value = serde_json::from_str(include_str!(
        "../../../desktop/src/features/project-pulse/lib/pulseMissionNoIdentity.fixture.json"
    ))
    .expect("the fixture is valid JSON");
    if produced != on_disk {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/pulseMissionNoIdentity.fixture.produced.json");
        let _ = std::fs::write(
            &path,
            serde_json::to_string_pretty(&produced).expect("pretty") + "\n",
        );
        panic!(
            "the no-identity fixture has drifted; the current bytes were written to {}",
            path.display()
        );
    }
    let not_read: Vec<&str> = on_disk["missions"]
        .as_array()
        .expect("missions")
        .iter()
        .flat_map(|row| row["lines"].as_array().expect("lines"))
        .filter(|line| line["id"] == "not-read")
        .filter_map(|line| line["text"].as_str())
        .collect();
    assert!(
        !not_read.is_empty()
            && not_read
                .iter()
                .all(|text| *text == PULSE_NO_VIEWER_IDENTITY),
        "the no-identity sentence is produced, not injected: {not_read:?}"
    );
}

#[test]
fn the_typescript_fixture_is_the_json_this_module_produces() {
    // The one fixture both surfaces are pinned to. `bee pulse digest` prints the
    // `text` of these lines and Desktop renders the same strings into elements,
    // so a drift here is a drift between the two consumers (L5.1's pattern: the
    // Rust test writes the fixture the TS test reads).
    //
    // Compared as JSON rather than as bytes because the checked-in file is
    // formatted by the Desktop formatter, which owns its whitespace and key
    // wrapping. Every key, every value and every sentence must still match
    // exactly.
    let produced = serde_json::to_value(fixture_rows()).expect("serialize");
    let on_disk: Value = serde_json::from_str(include_str!(
        "../../../desktop/src/features/project-pulse/lib/pulseMissionResponse.fixture.json"
    ))
    .expect("the fixture is valid JSON");
    if produced != on_disk {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/pulseMissionResponse.fixture.produced.json");
        let _ = std::fs::write(
            &path,
            serde_json::to_string_pretty(&produced).expect("pretty") + "\n",
        );
        panic!(
            "the fixture has drifted from what this module produces; the current bytes were \
             written to {}",
            path.display()
        );
    }
}
