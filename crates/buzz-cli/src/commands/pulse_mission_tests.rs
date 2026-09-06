//! The golden test between the two consumers.

use super::*;

/// The fixture Desktop's decoder is pinned to, produced by
/// `buzz_core::pulse_mission`'s own test.
fn fixture() -> PulseMissionRows {
    serde_json::from_str(include_str!(
        "../../../../desktop/src/features/project-pulse/lib/pulseMissionResponse.fixture.json"
    ))
    .expect("the fixture decodes into the same type Desktop receives")
}

#[test]
fn the_cli_prints_exactly_the_sentences_desktop_renders() {
    // `bee pulse missions --format compact` prints the `text` of every line and
    // nothing else; Desktop renders those same strings into elements with
    // testids. Both come from `render_pulse_mission_lines`, so this asserts
    // that the CLI adds no prose of its own.
    let rows = fixture();
    let printed = mission_text_lines(&rows);

    let mut expected: Vec<String> = Vec::new();
    for row in &rows.missions {
        expected.extend(row.lines.iter().map(|line| line.text.clone()));
        for seat in &row.seats {
            expected.extend(seat.lines.iter().map(|line| line.text.clone()));
        }
        for moved in &row.moved {
            expected.extend(moved.lines.iter().map(|line| line.text.clone()));
        }
        expected.extend(row.timing.iter().map(|line| line.text.clone()));
    }
    for overlap in &rows.overlaps {
        expected.extend(overlap.lines.iter().map(|line| line.text.clone()));
    }
    for error in &rows.mission_errors {
        // The message, and only the message: `scope` is a machine field the
        // JSON form keeps and Desktop never renders (REVIEW-L9 F4.2).
        expected.push(error.message.clone());
    }

    assert_eq!(printed, expected);
    assert!(
        printed.contains(
            &"Waiting on the founder · asked by Ira · 42m ago: Do we widen the push gate for seats?"
                .to_owned()
        ),
        "{printed:#?}"
    );
    assert!(
        printed.contains(
            &"No gate row on the wire for Ira — a claim in prose is not a gate row".to_owned()
        ),
        "{printed:#?}"
    );
    assert!(
        printed.contains(&"Ira's local commits: not shared".to_owned()),
        "{printed:#?}"
    );
}

/// The rows a surface with no identity renders, produced by the same renderer.
fn no_identity_fixture() -> PulseMissionRows {
    serde_json::from_str(include_str!(
        "../../../../desktop/src/features/project-pulse/lib/pulseMissionNoIdentity.fixture.json"
    ))
    .expect("the no-identity fixture decodes into the same type Desktop receives")
}

#[test]
fn a_surface_with_no_identity_says_so_rather_than_printing_a_zero() {
    // The sentence arrives on a `not-read` line the model composed, rather than
    // being appended here — see the fix-round test below (REVIEW-L9 F4.1).
    let rows = no_identity_fixture();
    let printed = mission_text_lines(&rows);
    assert!(
        printed
            .iter()
            .any(|line| line == "No identity on this surface, so nothing here can be held on you"),
        "{printed:#?}"
    );
    assert!(
        !printed.iter().any(|line| line == "0"),
        "an empty waiting list is never a zero"
    );
}

#[test]
fn a_read_error_is_printed_rather_than_rendering_a_quiet_project() {
    let rows = fixture();
    let printed = mission_text_lines(&rows);
    assert!(
        printed
            .iter()
            .any(|line| line
                == "9 open sessions in scope; the newest 8 by observation time were read"),
        "{printed:#?}"
    );
}

#[test]
fn no_sentence_in_this_module_is_composed_outside_buzz_core() {
    // Every human sentence lives in `buzz-core::pulse_mission`. This file may
    // print them and join a scope to a message; it may not write English.
    let source = include_str!("pulse_mission.rs");
    for phrase in [
        "Waiting on",
        "Mission running",
        "No gate row",
        "local commits",
        "No verdict",
        "No policy",
    ] {
        assert!(
            !source.contains(phrase),
            "the CLI must not compose `{phrase}`: it belongs in buzz-core"
        );
    }
}

// ── Fix round 1 (REVIEW-L9) ──────────────────────────────────────────────────

#[test]
fn the_error_lines_the_cli_prints_are_the_ones_desktop_renders() {
    // REVIEW-L9 F4.2: the CLI printed `{scope}: {message}` and Desktop rendered
    // `{message}`, so the one surface that is supposed to prove the two
    // consumers cannot differ was itself a divergence. `scope` is a machine
    // field and stays in the JSON form.
    let rows = fixture();
    let printed = mission_text_lines(&rows);
    for error in &rows.mission_errors {
        assert!(
            printed.contains(&error.message),
            "the message is printed verbatim: {:?}",
            error.message
        );
        assert!(
            !printed
                .iter()
                .any(|line| line == &format!("{}: {}", error.scope, error.message)),
            "and the scope is not glued onto it"
        );
    }
    // The scope survives where a machine reads it.
    let json = serde_json::to_value(&rows).expect("serialize");
    assert!(json["missionErrors"][0]["scope"].is_string(), "{json:#?}");
}

#[test]
fn the_no_identity_sentence_comes_from_the_row_not_from_this_module() {
    // REVIEW-L9 F4.1: the CLI used to append this itself, which is why Desktop
    // had nothing to render. It is now a `not-read` line on the row, so the CLI
    // prints it only because the model composed it.
    let rows = no_identity_fixture();
    assert!(rows.viewer_pubkey.is_none());
    let printed = mission_text_lines(&rows);
    assert!(
        printed
            .iter()
            .any(|line| line == "No identity on this surface, so nothing here can be held on you"),
        "{printed:#?}"
    );

    // And a response that knows its viewer makes no such claim.
    let known = fixture();
    assert!(known.viewer_pubkey.is_some());
    assert!(!mission_text_lines(&known)
        .iter()
        .any(|line| line.contains("No identity on this surface")),);
}

#[test]
fn the_two_fixtures_differ_only_by_who_is_reading() {
    // The same facts, rendered for a viewer and for nobody. Every line that is
    // not about the reader is identical, which is what makes the second fixture
    // a rendering of the model rather than a second hand-written contract.
    let known = fixture();
    let unknown = no_identity_fixture();
    assert_eq!(known.missions.len(), unknown.missions.len());
    assert_eq!(known.mission_scope, unknown.mission_scope);
    assert_eq!(known.open_rulings.len(), unknown.open_rulings.len());
    assert!(
        unknown.rulings_waiting_on_viewer.is_empty(),
        "nothing can be held on a reader nobody identified"
    );
    for (known_row, unknown_row) in known.missions.iter().zip(unknown.missions.iter()) {
        assert_eq!(known_row.session_key, unknown_row.session_key);
        let known_ids: Vec<&str> = known_row.lines.iter().map(|l| l.id.as_str()).collect();
        let unknown_ids: Vec<&str> = unknown_row
            .lines
            .iter()
            .filter(|line| line.id != "not-read")
            .map(|l| l.id.as_str())
            .collect();
        assert_eq!(known_ids, unknown_ids, "{}", known_row.session_key);
    }
}

/// **S4.** Who may commission an execution, as this reader resolves it: the
/// founder always, an operator the accepted chain still grants, and nobody
/// else. The set is what makes a `measured` row's signer checkable at all —
/// with it empty every gate line prints `(observed, unverified)`.
#[test]
fn the_steering_set_is_the_founder_and_the_grants_that_still_stand() {
    use crate::commands::sessions::operations_reads::SessionAuthority;
    use buzz_core::coding_session_authority_transition::CodingSessionAuthorityTransitionType;
    use buzz_core::coding_session_policy::CodingSessionPolicyGrant;
    use buzz_core::coding_session_team_transaction::CodingSessionTeamFoldContext;

    let founder = "aa".repeat(32);
    let operator = "bb".repeat(32);
    let revoked = "cc".repeat(32);
    let authority = SessionAuthority {
        context: CodingSessionTeamFoldContext {
            channel_ref: "c0066ddd-8214-4baf-81d2-3046fead0d32".to_owned(),
            session_ref: "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10".to_owned(),
            genesis_ref: "dd".repeat(32),
            founder_pubkey: founder.clone(),
            active_seats: Vec::new(),
            active_grants: Vec::new(),
            verifier_required: false,
        },
        policy_grants: vec![
            CodingSessionPolicyGrant {
                grantee: operator.clone(),
                accepted_at: 1_000,
                transition_type: CodingSessionAuthorityTransitionType::GrantOperator,
            },
            CodingSessionPolicyGrant {
                grantee: revoked.clone(),
                accepted_at: 1_000,
                transition_type: CodingSessionAuthorityTransitionType::GrantOperator,
            },
            CodingSessionPolicyGrant {
                grantee: revoked.clone(),
                accepted_at: 2_000,
                transition_type: CodingSessionAuthorityTransitionType::Revoke,
            },
        ],
    };

    let signers = steering_signers(&authority, 3_000);
    assert_eq!(signers, vec![founder, operator]);
    assert!(
        !signers.contains(&revoked),
        "a revoked operator commissions nothing: {signers:?}"
    );
}
