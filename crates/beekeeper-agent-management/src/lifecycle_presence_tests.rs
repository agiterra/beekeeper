//! Provider presence (kind 24224) folded into `status` (HEADLESS_PLUGGABLE_SLICE
//! § Step 2): idle with live presence reads `idle`; `disconnected` only when
//! presence is expired or absent.
use super::{execution_word, presence_from_events, ProviderPresence};
use beekeeper_core::coding_session_command::CodingSessionTarget;
use beekeeper_sdk::coding_session::{
    build_coding_session_provider_presence, ProviderPresence as Payload, ProviderPresenceState,
};
use nostr::{EventBuilder, Keys, Kind, Tag};
use serde_json::Value;
use uuid::Uuid;

fn heartbeat(
    keys: &Keys,
    channel: Uuid,
    instance: &str,
    seq: u64,
    ttl: u64,
    state: ProviderPresenceState,
    created_at: u64,
) -> Value {
    let payload = Payload::new(instance, seq, ttl, state, vec![]).expect("payload");
    let event = build_coding_session_provider_presence(channel, &payload)
        .expect("builder")
        .custom_created_at(nostr::Timestamp::from(created_at))
        .sign_with_keys(keys)
        .expect("signed");
    serde_json::to_value(event).expect("json")
}

/// The winning heartbeat's `liveTargets` is what `status` checks a row's
/// target against; a heartbeat that does not name the target makes the
/// execution `absent`, not `idle`.
#[test]
fn live_targets_ride_with_the_winning_heartbeat() {
    let provider = Keys::generate();
    let channel = Uuid::new_v4();
    let now = 1_000_000u64;
    let target = CodingSessionTarget {
        driver: "acme-acp".into(),
        instance_id: "inst".into(),
        session_id: Uuid::new_v4().to_string(),
        generation: 2,
    };
    let payload = Payload::new(
        "inst",
        7,
        90,
        ProviderPresenceState::Idle,
        vec![target.clone()],
    )
    .expect("payload");
    let event = build_coding_session_provider_presence(channel, &payload)
        .expect("builder")
        .custom_created_at(nostr::Timestamp::from(now - 5))
        .sign_with_keys(&provider)
        .expect("signed");
    let rows = vec![serde_json::to_value(event).expect("json")];
    let observed =
        presence_from_events(&rows, &provider.public_key().to_hex(), "inst", channel, now);
    assert_eq!(observed.presence, ProviderPresence::Live);
    assert_eq!(observed.live_targets, vec![target.clone()]);
    let other = CodingSessionTarget {
        generation: 3,
        ..target.clone()
    };
    assert_eq!(
        execution_word(
            false,
            false,
            observed.presence,
            observed.live_targets.contains(&target)
        ),
        "idle"
    );
    assert_eq!(
        execution_word(
            false,
            false,
            observed.presence,
            observed.live_targets.contains(&other)
        ),
        "absent"
    );
    let none = presence_from_events(&[], &provider.public_key().to_hex(), "inst", channel, now);
    assert!(none.live_targets.is_empty());
}

#[test]
fn the_newest_seq_decides_and_ttl_is_judged_at_now() {
    let provider = Keys::generate();
    let channel = Uuid::new_v4();
    let now = 1_000_000u64;
    let rows = vec![
        heartbeat(
            &provider,
            channel,
            "inst",
            3,
            90,
            ProviderPresenceState::Busy,
            now - 200,
        ),
        heartbeat(
            &provider,
            channel,
            "inst",
            2,
            90,
            ProviderPresenceState::Idle,
            now - 10,
        ),
    ];
    // seq 3 wins even though seq 2 is newer by clock; seq 3 is past its TTL.
    let observed =
        presence_from_events(&rows, &provider.public_key().to_hex(), "inst", channel, now);
    assert_eq!(observed.presence, ProviderPresence::Expired);
    assert_eq!(observed.seq, Some(3));
    assert_eq!(observed.state, Some(ProviderPresenceState::Busy));
    let rows = vec![heartbeat(
        &provider,
        channel,
        "inst",
        4,
        90,
        ProviderPresenceState::Idle,
        now - 60,
    )];
    let observed =
        presence_from_events(&rows, &provider.public_key().to_hex(), "inst", channel, now);
    assert_eq!(observed.presence, ProviderPresence::Live);
    assert_eq!(observed.state, Some(ProviderPresenceState::Idle));
}

#[test]
fn foreign_signers_instances_channels_and_malformed_rows_are_none() {
    let provider = Keys::generate();
    let stranger = Keys::generate();
    let channel = Uuid::new_v4();
    let now = 1_000_000u64;
    let authority = provider.public_key().to_hex();
    let wrong_signer = heartbeat(
        &stranger,
        channel,
        "inst",
        9,
        90,
        ProviderPresenceState::Idle,
        now,
    );
    let wrong_instance = heartbeat(
        &provider,
        channel,
        "other",
        9,
        90,
        ProviderPresenceState::Idle,
        now,
    );
    let wrong_channel = heartbeat(
        &provider,
        Uuid::new_v4(),
        "inst",
        9,
        90,
        ProviderPresenceState::Idle,
        now,
    );
    let malformed = serde_json::to_value(
        EventBuilder::new(Kind::Custom(24224), "{}")
            .tags([Tag::parse(["h", &channel.to_string()]).expect("h")])
            .sign_with_keys(&provider)
            .expect("signed"),
    )
    .expect("json");
    for rows in [
        vec![],
        vec![wrong_signer],
        vec![wrong_instance],
        vec![wrong_channel],
        vec![malformed],
        vec![Value::String("not an event".into())],
    ] {
        let observed = presence_from_events(&rows, &authority, "inst", channel, now);
        assert_eq!(observed.presence, ProviderPresence::None, "{rows:?}");
        assert_eq!(observed.seq, None);
    }
}

#[test]
fn execution_word_says_idle_with_live_presence_and_disconnected_only_without_it() {
    assert_eq!(
        execution_word(true, false, ProviderPresence::Live, true),
        "live"
    );
    assert_eq!(
        execution_word(true, false, ProviderPresence::None, false),
        "live"
    );
    assert_eq!(
        execution_word(false, false, ProviderPresence::Live, true),
        "idle"
    );
    // A live provider that does not name this execution among its
    // liveTargets is not idle over it: the execution is absent.
    assert_eq!(
        execution_word(false, false, ProviderPresence::Live, false),
        "absent"
    );
    assert_eq!(
        execution_word(false, false, ProviderPresence::Expired, true),
        "disconnected"
    );
    assert_eq!(
        execution_word(false, false, ProviderPresence::None, false),
        "disconnected"
    );
    assert_eq!(
        execution_word(false, true, ProviderPresence::Live, true),
        "stopped"
    );
    assert_eq!(
        execution_word(true, true, ProviderPresence::Live, false),
        "stopped"
    );
}

#[test]
fn a_live_target_in_the_heartbeat_must_belong_to_the_instance() {
    let target = CodingSessionTarget {
        driver: "d".into(),
        instance_id: "other".into(),
        session_id: "s".into(),
        generation: 1,
    };
    assert!(Payload::new("inst", 1, 90, ProviderPresenceState::Busy, vec![target]).is_err());
}
