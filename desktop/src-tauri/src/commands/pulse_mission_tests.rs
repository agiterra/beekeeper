//! What the native mission-row adapter accepts, refuses, and discloses.

use super::*;

fn request() -> PulseMissionRowsRequest {
    PulseMissionRowsRequest {
        schema: PULSE_MISSION_REQUEST_SCHEMA.to_owned(),
        project: "30621:11:beekeeper".to_owned(),
        channel_ids: Vec::new(),
        now_unix: 1_756_800_960,
        viewer_pubkey: None,
        display_names: BTreeMap::new(),
        open_session_count: 0,
        sessions: Vec::new(),
        read_errors: Vec::new(),
    }
}

fn session(session_key: &str, founder: &str) -> PulseMissionSessionInput {
    PulseMissionSessionInput {
        session_key: session_key.to_owned(),
        channel_ref: "05ef0ecf-745f-5fb8-b7ff-f9cba21e01c2".to_owned(),
        session_ref: "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10".to_owned(),
        genesis_ref: "ab".repeat(32),
        founder_pubkey: founder.to_owned(),
        name: Some("Route rail honesty".to_owned()),
        latest_observation_at: Some(1_756_800_600),
        active_seats: Vec::new(),
        active_grants: Vec::new(),
        claimed_seats: Vec::new(),
        team_events: Vec::new(),
        policy_events: Vec::new(),
        observation_events: Vec::new(),
        lifecycle_commands: Vec::new(),
        lifecycle_receipts: Vec::new(),
        ref_state: Vec::new(),
        overlap_files: Vec::new(),
        overlap_sha: None,
        overlap_as_of: None,
        overlap_author: None,
    }
}

#[test]
fn an_unknown_request_schema_is_refused_by_name() {
    let mut request = request();
    request.schema = "buzz-pulse-mission-rows-request/v2".to_owned();
    let error = mission_rows(request).expect_err("refused");
    assert_eq!(
        error,
        "unsupported pulse-mission request schema: buzz-pulse-mission-rows-request/v2"
    );
}

#[test]
fn more_sessions_than_the_cap_are_refused_rather_than_quietly_trimmed() {
    let mut request = request();
    request.sessions = (0..9)
        .map(|index| session(&format!("session-{index}"), &"11".repeat(32)))
        .collect();
    let error = mission_rows(request).expect_err("refused");
    assert_eq!(
        error,
        "pulse-mission request carries 9 sessions; the cap is 8"
    );
}

#[test]
fn the_response_carries_the_eight_keys_and_the_scope_it_read() {
    let response = mission_rows(request()).expect("folded");
    assert_eq!(response.missions_schema, "buzz-pulse-mission-rows/v1");
    assert_eq!(
        response.mission_scope,
        "project channels · the newest 8 open sessions by observation time"
    );
    assert!(response.missions.is_empty());
    assert!(response.mission_errors.is_empty());
    assert!(response.viewer_pubkey.is_none());
}

#[test]
fn a_ninth_open_session_is_disclosed_by_name_rather_than_dropped() {
    let mut request = request();
    request.open_session_count = 9;
    request.sessions = vec![session("session-0", &"11".repeat(32))];
    let response = mission_rows(request).expect("folded");
    assert_eq!(
        response
            .mission_errors
            .iter()
            .map(|error| error.message.as_str())
            .collect::<Vec<_>>(),
        vec![
            "9 open sessions in scope; the newest 8 by observation time were read",
            // Complementary, not redundant: the first says the cap selected 8
            // of 9, the second says only 1 of those 8 was actually read.
            "9 open sessions in scope; 1 had their signed records read, so 7 are not shown here",
        ]
    );
}

#[test]
fn a_read_error_the_caller_already_had_survives_into_the_response() {
    let mut request = request();
    request.read_errors = vec![PulseMissionError {
        scope: "missions:aa".to_owned(),
        message: "relay closed the subscription".to_owned(),
    }];
    let response = mission_rows(request).expect("folded");
    assert_eq!(response.mission_errors.len(), 1);
    assert_eq!(response.mission_errors[0].scope, "missions:aa");
}

#[test]
fn no_checkpoint_files_means_no_overlap_row_rather_than_a_guessed_one() {
    let mut request = request();
    // Two umbrellas, neither carrying `checkpoint.files` — the key Lane L5 owns.
    request.sessions = vec![
        session("session-a", &"11".repeat(32)),
        session("session-b", &"22".repeat(32)),
    ];
    let response = mission_rows(request).expect("folded");
    assert!(
        response.overlaps.is_empty(),
        "an overlap is computed from published paths or it is not computed"
    );
}

#[test]
fn two_umbrellas_sharing_a_path_produce_one_row_and_no_target_of_any_kind() {
    let mut request = request();
    let mut left = session("session-a", &"11".repeat(32));
    left.overlap_files = vec!["crates/beekeeper-core/src/pulse.rs".to_owned()];
    left.overlap_sha = Some("9a1c4e7b2d3f40516273849506172839405a6b7c".to_owned());
    left.overlap_author = Some("11".repeat(32));
    left.overlap_as_of = Some(1_756_800_600);
    let mut right = session("session-b", &"22".repeat(32));
    right.overlap_files = vec!["crates/beekeeper-core/src/pulse.rs".to_owned()];
    right.overlap_sha = Some("b7c8d9e0f1a2334455667788990011223344556f".to_owned());
    right.overlap_author = Some("22".repeat(32));
    right.overlap_as_of = Some(1_756_800_000);
    request.sessions = vec![left, right];

    let response = mission_rows(request).expect("folded");
    assert_eq!(response.overlaps.len(), 1);
    assert_eq!(response.overlaps[0].seats.len(), 2);
    // The line nothing crosses: the row wakes nobody.
    let serialized = serde_json::to_string(&response.overlaps).expect("serialize");
    for forbidden in ["\"target\"", "\"wake\"", "\"commandId\""] {
        assert!(!serialized.contains(forbidden), "{serialized}");
    }
}

#[test]
fn a_founder_held_ruling_reaches_the_founders_own_waiting_list_only() {
    let founder = "11".repeat(32);
    let mut request = request();
    request.viewer_pubkey = Some(founder.clone());
    request.sessions = vec![session("session-a", &founder)];
    let response = mission_rows(request).expect("folded");
    // No records supplied, so there is nothing open — and an empty list is
    // rendered as an empty list, never as a zero.
    assert!(response.open_rulings.is_empty());
    assert!(response.rulings_waiting_on_viewer.is_empty());
    assert_eq!(response.viewer_pubkey.as_deref(), Some(founder.as_str()));
}

#[test]
fn a_read_that_gathered_no_records_says_so_rather_than_showing_nothing() {
    // The Desktop read that feeds this command is not wired yet: it knows the
    // project's open sessions but has not gathered their signed events. That is
    // an unread session, not an empty one, and the row that says so is what
    // keeps the surface from reading as a quiet project.
    let mut request = request();
    request.open_session_count = 3;
    let response = mission_rows(request).expect("folded");
    assert!(response.missions.is_empty());
    assert_eq!(
        response
            .mission_errors
            .iter()
            .map(|error| error.message.as_str())
            .collect::<Vec<_>>(),
        vec!["3 open sessions in scope; 0 had their signed records read, so 3 are not shown here"]
    );
}

#[test]
fn a_read_that_gathered_every_session_discloses_nothing() {
    let mut request = request();
    request.open_session_count = 1;
    request.sessions = vec![session("session-a", &"11".repeat(32))];
    let response = mission_rows(request).expect("folded");
    assert!(
        response.mission_errors.is_empty(),
        "{:?}",
        response.mission_errors
    );
}

/// **S4.** The steering set this adapter resolves from the authority
/// projection the caller already sends: the founder, plus every grant that
/// still confers steering. A revoked or view-only grant commissions nothing.
#[test]
fn the_steering_set_is_the_founder_and_the_grants_that_still_steer() {
    let founder = "11".repeat(32);
    let operator = "22".repeat(32);
    let viewer = "33".repeat(32);
    let revoked = "44".repeat(32);
    let mut session = session("session-1", &founder);
    session.active_grants = vec![
        PulseMissionGrantInput {
            actor_pubkey: operator.clone(),
            grant_event_ref: "aa".repeat(32),
            may_steer: true,
            accepted_at: 1_000,
            granted: true,
        },
        PulseMissionGrantInput {
            actor_pubkey: viewer.clone(),
            grant_event_ref: "bb".repeat(32),
            may_steer: false,
            accepted_at: 1_000,
            granted: true,
        },
        PulseMissionGrantInput {
            actor_pubkey: revoked.clone(),
            grant_event_ref: "cc".repeat(32),
            may_steer: true,
            accepted_at: 2_000,
            granted: false,
        },
    ];

    let signers = steering_signers(&session);
    assert_eq!(signers, vec![founder, operator]);
    assert!(!signers.contains(&viewer), "{signers:?}");
    assert!(!signers.contains(&revoked), "{signers:?}");
}

/// **S4, the older caller.** `lifecycleCommands` and `lifecycleReceipts` are
/// `#[serde(default)]`, so a caller that predates them still decodes — and
/// resolves no provider, which is what makes its gate lines read
/// `(observed, unverified)` rather than crediting a set nobody proved.
#[test]
fn a_caller_that_sends_no_lifecycle_still_decodes_and_proves_no_provider() {
    let session: PulseMissionSessionInput = serde_json::from_value(serde_json::json!({
        "sessionKey": "session-1",
        "channelRef": "05ef0ecf-745f-5fb8-b7ff-f9cba21e01c2",
        "sessionRef": "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10",
        "genesisRef": "ab".repeat(32),
        "founderPubkey": "11".repeat(32),
        "name": null,
        "latestObservationAt": null,
        "activeSeats": [],
        "activeGrants": [],
        "claimedSeats": [],
        "teamEvents": [],
        "policyEvents": [],
        "observationEvents": [],
        "refState": [],
        "overlapFiles": [],
        "overlapSha": null,
        "overlapAsOf": null,
        "overlapAuthor": null,
    }))
    .expect("a request written before S4 must still decode");
    assert!(session.lifecycle_commands.is_empty());
    assert!(session.lifecycle_receipts.is_empty());
    assert!(mission_provider_pubkeys_from_lifecycle(
        &session.session_ref,
        &session.genesis_ref,
        &steering_signers(&session),
        &session.lifecycle_commands,
        &session.lifecycle_receipts,
    )
    .is_empty());
}
