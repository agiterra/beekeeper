//! What a held wake keeps, and what it clamps.
//!
//! The release itself — a fetch that finishes after the waiter gave up, and
//! the turn opening afterwards with nobody sending a second command — is
//! driven end to end in `src/tests/verification_input_tests.rs`, because it
//! needs a provider. These cases pin the record and the floor.

use super::*;

fn turn(command_id: &str, channel_id: Uuid, created_at: u64) -> DeferredTurn {
    DeferredTurn {
        command_id: command_id.to_owned(),
        channel_id,
        created_at,
        operator_pubkey: "ab".repeat(32),
        event_id: "cd".repeat(32),
        target: CodingSessionTarget {
            driver: "claude".to_owned(),
            instance_id: "instance".to_owned(),
            session_id: "seat-1".to_owned(),
            generation: 1,
        },
        text: serde_json::json!({"type": "assignment", "operationId": "ef".repeat(32)}).to_string(),
        content: String::new(),
        attachments: Vec::new(),
        deliver: CodingSessionDelivery::Boundary,
        operation_key: Some("key".to_owned()),
        assignment_ref: "ef".repeat(32),
        reason: "establishment_in_flight".to_owned(),
        deferred_at: "2026-09-21T00:00:00Z".to_owned(),
    }
}

#[test]
fn a_held_wake_survives_this_process_and_clamps_its_channels_floor() {
    forget_mirror();
    let root = tempfile::tempdir().expect("temp");
    let state = root.path().join("state");
    let channel = Uuid::new_v4();
    assert_eq!(floor_for_channel(&state, channel), None);

    defer(&state, turn("wake-1", channel, 1_700_000_100)).expect("held");
    defer(&state, turn("wake-2", channel, 1_700_000_050)).expect("held");
    defer(&state, turn("wake-3", Uuid::new_v4(), 1_700_000_001)).expect("held");

    assert_eq!(
        floor_for_channel(&state, channel),
        Some(1_700_000_050),
        "the floor is held at the oldest wake this channel still owes"
    );

    // A second process reading the same state directory sees the same wakes:
    // the file is the whole truth, not the mirror.
    forget_mirror();
    let after_restart = held(&state);
    assert_eq!(after_restart.len(), 3);
    assert_eq!(
        after_restart.first().map(|turn| turn.command_id.as_str()),
        Some("wake-3"),
        "oldest first, so the oldest is prepared first"
    );
    assert_eq!(
        after_restart
            .iter()
            .find(|turn| turn.command_id == "wake-2")
            .map(|turn| turn.text.clone()),
        Some(turn("wake-2", channel, 0).text),
        "the exact command is kept, not a summary of it"
    );
}

#[test]
fn deferring_the_same_wake_twice_keeps_one_and_keeps_its_first_created_at() {
    forget_mirror();
    let root = tempfile::tempdir().expect("temp");
    let state = root.path().join("state");
    let channel = Uuid::new_v4();
    defer(&state, turn("wake-1", channel, 1_700_000_100)).expect("held");
    let mut again = turn("wake-1", channel, 1_700_000_900);
    again.reason = "seat_busy".to_owned();
    defer(&state, again).expect("held");

    let held = held(&state);
    assert_eq!(held.len(), 1);
    assert_eq!(held[0].created_at, 1_700_000_100);
    assert_eq!(held[0].reason, "seat_busy", "the newest reason is kept");
}

#[test]
fn a_released_wake_stops_clamping_anything() {
    forget_mirror();
    let root = tempfile::tempdir().expect("temp");
    let state = root.path().join("state");
    let channel = Uuid::new_v4();
    defer(&state, turn("wake-1", channel, 1_700_000_100)).expect("held");
    release(&state, "wake-1").expect("released");
    assert!(held(&state).is_empty());
    assert_eq!(floor_for_channel(&state, channel), None);

    forget_mirror();
    assert!(
        held(&state).is_empty(),
        "the release reached the disk, not only the mirror"
    );
}

#[test]
fn a_file_from_a_version_this_build_does_not_read_is_ignored_rather_than_guessed_at() {
    forget_mirror();
    let root = tempfile::tempdir().expect("temp");
    let state = root.path().join("state");
    std::fs::create_dir_all(&state).expect("dir");
    std::fs::write(
        state.join(DEFERRED_TURNS_FILE),
        serde_json::to_vec(&serde_json::json!({"version": 99, "turns": [{"nonsense": true}]}))
            .expect("serialize"),
    )
    .expect("write");
    assert!(held(&state).is_empty());
}
