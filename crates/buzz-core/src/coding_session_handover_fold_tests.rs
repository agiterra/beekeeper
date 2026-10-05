//! Fold tests over real signed 44247 events.
//!
//! Every event here is built and signed the way a publisher builds one, so a
//! change to the envelope breaks these tests rather than passing a hand-made
//! struct through a fold that would never have seen it (the
//! `pulse_mission_tests.rs::signed` pattern).

use nostr::{EventBuilder, Keys, Kind, Tag, Timestamp};
use serde_json::{json, Value};

use super::*;
use crate::coding_session_authority_claim::CurrentClaim;
use crate::coding_session_handover::{
    CODING_SESSION_HANDOVER_SCHEMA, CODING_SESSION_HANDOVER_TAG_VERSION,
};
use crate::kind::KIND_CODING_SESSION_HANDOVER;

pub(super) const SESSION: &str = "dc580cfb-6c80-4fc2-8f4e-dfc328acf222";
pub(super) const CHANNEL: &str = "d3e440ea-89f8-4aee-8a02-17edc3e7272e";
pub(super) const GENESIS: &str = "ce5d87ed1b9b4416bb0aa37ea0fb451f211289c54099882917f4ad538d51519b";

/// Deterministic keys, so ids are stable across runs.
pub(super) fn fixed_keys(byte: u8) -> Keys {
    Keys::parse(&format!("{byte:02x}").repeat(32)).expect("a fixed secret key")
}

/// A checkpoint body a publisher would actually write.
pub(super) fn checkpoint_body(next_action: &str, preserved: &str) -> Value {
    checkpoint_body_after(None, next_action, preserved)
}

/// A checkpoint body that names the checkpoint it replaces.
pub(super) fn checkpoint_body_after(
    prev_checkpoint_ref: Option<&str>,
    next_action: &str,
    preserved: &str,
) -> Value {
    json!({
        "prevCheckpointRef": prev_checkpoint_ref.map_or(Value::Null, |value| json!(value)),
        "task": "Fold handover records for one umbrella",
        "assignmentRefs": ["ab".repeat(32)],
        "decisions": [{ "eventId": "cd".repeat(32), "summary": "The claim is umbrella-wide" }],
        "revision": {
            "repoRef": "30617:aa/beekeeper",
            "baseSha": "1a".repeat(20),
            "headSha": "2b".repeat(20),
            "branch": "work/handover",
            "dirty": true,
            "preserved": preserved
        },
        "artifacts": [{
            "kind": "wip-ref",
            "repoRef": "30617:aa/beekeeper",
            "ref": "refs/heads/wip/builder/1f2e3d4c",
            "sha": "2b".repeat(20)
        }],
        "tests": [{ "name": "core", "command": "cargo test -p buzz-core", "outcome": "failed" }],
        "unresolved": ["Whether a voided claim fences sibling executions"],
        "nextAction": next_action,
        "missing": ["uncommitted changes in target/, above the patch bound"]
    })
}

/// A continuation body naming `claim_ref`.
pub(super) fn continuation_body(
    claim_ref: &str,
    mode: &str,
    checkpoint_ref: Option<&str>,
) -> Value {
    json!({
        "claimRef": claim_ref,
        "mode": mode,
        "checkpointRef": checkpoint_ref.map_or(Value::Null, |value| json!(value)),
        "target": {
            "driver": "claude-agent-acp",
            "instanceId": "provider-b",
            "sessionId": "sess-b-1",
            "generation": 1
        },
        "recovered": ["wip-ref refs/heads/wip/builder/1f2e3d4c at 2b2b2b2b"],
        "missing": ["uncommitted changes on A's machine (dirty=true, no patch)"],
        "note": null
    })
}

/// Sign one 44247 event with the exact envelope.
pub(super) fn handover_event(
    keys: &Keys,
    record_type: &str,
    body: Value,
    created_at: u64,
) -> nostr::Event {
    let content = json!({
        "schema": CODING_SESSION_HANDOVER_SCHEMA,
        "sessionRef": SESSION,
        "genesisRef": GENESIS,
        "type": record_type,
        "body": body,
    })
    .to_string();
    EventBuilder::new(Kind::Custom(KIND_CODING_SESSION_HANDOVER as u16), content)
        .tags(vec![
            Tag::parse(["h", CHANNEL]).expect("h"),
            Tag::parse(["d", SESSION]).expect("d"),
            Tag::parse(["csh-v", CODING_SESSION_HANDOVER_TAG_VERSION]).expect("csh-v"),
            Tag::parse(["csh-genesis", GENESIS]).expect("csh-genesis"),
            Tag::parse(["csh-type", record_type]).expect("csh-type"),
        ])
        .custom_created_at(Timestamp::from_secs(created_at))
        .sign_with_keys(keys)
        .expect("sign")
}

fn context(claim: ClaimState) -> HandoverFoldContext {
    HandoverFoldContext {
        channel_ref: CHANNEL.to_owned(),
        session_ref: SESSION.to_owned(),
        genesis_ref: GENESIS.to_owned(),
        founder_pubkey: fixed_keys(0x11).public_key().to_hex(),
        grants: vec![(fixed_keys(0x22).public_key().to_hex(), 1_800_000_000)],
        seats: vec![(
            fixed_keys(0x33).public_key().to_hex(),
            "builder".to_owned(),
            1_800_000_000,
        )],
        claim,
        claim_since: Some(1_800_000_500),
        retired: false,
    }
}

fn active_claim(claimant: &Keys, accepted_event_id: &str) -> ClaimState {
    ClaimState::Active(CurrentClaim {
        claimant: claimant.public_key().to_hex(),
        body_pubkey: fixed_keys(0xdd).public_key().to_hex(),
        accepted_event_id: accepted_event_id.to_owned(),
        seq: 2,
    })
}

#[test]
fn the_founder_a_live_operator_and_a_seat_all_have_standing_to_checkpoint() {
    let founder = fixed_keys(0x11);
    let operator = fixed_keys(0x22);
    let seat = fixed_keys(0x33);
    let events = vec![
        handover_event(
            &founder,
            "checkpoint",
            checkpoint_body("founder's next action", "partial"),
            1_800_000_100,
        ),
        handover_event(
            &operator,
            "checkpoint",
            checkpoint_body("operator's next action", "partial"),
            1_800_000_200,
        ),
        handover_event(
            &seat,
            "checkpoint",
            checkpoint_body("seat's next action", "partial"),
            1_800_000_300,
        ),
    ];
    let fold =
        fold_coding_session_handover(&events, &context(ClaimState::NoClaim)).expect("a clean fold");
    assert_eq!(fold.checkpoints.len(), 3);
    assert!(fold
        .checkpoints
        .iter()
        .all(|entry| entry.standing == HandoverStanding::Authorized));
    assert_eq!(
        fold.latest_authorized_checkpoint.as_deref(),
        Some(events[2].id.to_hex().as_str())
    );
    assert!(fold.excluded.is_empty());
}

/// Standing is judged at the record's own time: the grant that arrives after
/// the checkpoint does not reach back and authorize it.
#[test]
fn a_checkpoint_written_before_its_authors_grant_is_unauthorized_and_still_listed() {
    let operator = fixed_keys(0x22);
    let early = handover_event(
        &operator,
        "checkpoint",
        checkpoint_body("written before the grant", "partial"),
        1_799_999_000,
    );
    let fold =
        fold_coding_session_handover(std::slice::from_ref(&early), &context(ClaimState::NoClaim))
            .expect("a clean fold");
    assert_eq!(fold.checkpoints.len(), 1);
    assert_eq!(fold.checkpoints[0].standing, HandoverStanding::Unauthorized);
    assert_eq!(fold.latest_authorized_checkpoint, None);
    assert_eq!(fold.excluded.len(), 1);
    assert_eq!(fold.excluded[0].event_id, early.id.to_hex());
    assert!(fold.excluded[0]
        .reason
        .contains("not used for reconstruction"));
}

#[test]
fn a_stranger_checkpointing_is_unauthorized() {
    let stranger = fixed_keys(0x99);
    let event = handover_event(
        &stranger,
        "checkpoint",
        checkpoint_body("a stranger's plan", "partial"),
        1_800_000_400,
    );
    let fold = fold_coding_session_handover(&[event], &context(ClaimState::NoClaim))
        .expect("a clean fold");
    assert_eq!(fold.checkpoints[0].standing, HandoverStanding::Unauthorized);
    assert_eq!(fold.latest_authorized_checkpoint, None);
}

#[test]
fn the_claimants_continuation_of_the_claim_in_force_is_the_active_one() {
    let claimant = fixed_keys(0x22);
    let claim_ref = "aa".repeat(32);
    let event = handover_event(
        &claimant,
        "continuation",
        continuation_body(&claim_ref, "reconstructed", Some(&"22".repeat(32))),
        1_800_000_600,
    );
    let fold = fold_coding_session_handover(
        std::slice::from_ref(&event),
        &context(active_claim(&claimant, &claim_ref)),
    )
    .expect("a clean fold");
    assert_eq!(fold.continuations.len(), 1);
    assert_eq!(fold.continuations[0].standing, HandoverStanding::Authorized);
    assert_eq!(
        fold.continuations[0].mode,
        CodingSessionHandoverMode::Reconstructed
    );
    assert_eq!(
        fold.active_continuation.as_deref(),
        Some(event.id.to_hex().as_str())
    );
    assert_eq!(fold.claim_since, Some(1_800_000_500));
}

/// A continuation of an older claim is history, not an error: "continued by B
/// until …". It is listed, labelled `superseded`, and never the active one.
#[test]
fn a_continuation_of_a_superseded_claim_stays_historical() {
    let first = fixed_keys(0x22);
    let second = fixed_keys(0x33);
    let old_claim = "aa".repeat(32);
    let new_claim = "bb".repeat(32);
    let events = vec![
        handover_event(
            &first,
            "continuation",
            continuation_body(&old_claim, "native-resume", None),
            1_800_000_600,
        ),
        handover_event(
            &second,
            "continuation",
            continuation_body(&new_claim, "reconstructed", None),
            1_800_000_700,
        ),
    ];
    let fold = fold_coding_session_handover(&events, &context(active_claim(&second, &new_claim)))
        .expect("a clean fold");
    assert_eq!(fold.continuations[0].standing, HandoverStanding::Superseded);
    assert_eq!(fold.continuations[1].standing, HandoverStanding::Authorized);
    assert_eq!(
        fold.active_continuation.as_deref(),
        Some(events[1].id.to_hex().as_str())
    );
    assert!(fold
        .excluded
        .iter()
        .any(|exclusion| exclusion.reason.contains("not the claim in force")));
}

/// Somebody who is not the claimant cannot continue the claim, even naming it
/// correctly.
#[test]
fn a_continuation_by_anyone_but_the_claimant_is_unauthorized() {
    let claimant = fixed_keys(0x22);
    let impostor = fixed_keys(0x33);
    let claim_ref = "aa".repeat(32);
    let event = handover_event(
        &impostor,
        "continuation",
        continuation_body(&claim_ref, "native-resume", None),
        1_800_000_600,
    );
    let fold =
        fold_coding_session_handover(&[event], &context(active_claim(&claimant, &claim_ref)))
            .expect("a clean fold");
    assert_eq!(
        fold.continuations[0].standing,
        HandoverStanding::Unauthorized
    );
    assert_eq!(fold.active_continuation, None);
}

/// A voided claim leaves nobody continuing: the fence stays up until a fresh
/// accepted claim, and the fold must not fall back to the last claimant.
#[test]
fn a_voided_claim_has_no_active_continuation() {
    let claimant = fixed_keys(0x22);
    let claim_ref = "aa".repeat(32);
    let voided = ClaimState::Voided {
        last: CurrentClaim {
            claimant: claimant.public_key().to_hex(),
            body_pubkey: fixed_keys(0xdd).public_key().to_hex(),
            accepted_event_id: claim_ref.clone(),
            seq: 2,
        },
        voided_by: "cc".repeat(32),
        seq: 3,
    };
    let event = handover_event(
        &claimant,
        "continuation",
        continuation_body(&claim_ref, "native-resume", None),
        1_800_000_600,
    );
    let fold =
        fold_coding_session_handover(&[event], &context(voided.clone())).expect("a clean fold");
    assert_eq!(fold.continuations[0].standing, HandoverStanding::Superseded);
    assert_eq!(fold.active_continuation, None);
    assert_eq!(fold.claim, voided);
}

/// A retired umbrella lists everything and acts on nothing.
#[test]
fn a_retired_umbrella_lists_its_records_and_offers_nothing() {
    let founder = fixed_keys(0x11);
    let claimant = fixed_keys(0x22);
    let claim_ref = "aa".repeat(32);
    let events = vec![
        handover_event(
            &founder,
            "checkpoint",
            checkpoint_body("would have been next", "partial"),
            1_800_000_100,
        ),
        handover_event(
            &claimant,
            "continuation",
            continuation_body(&claim_ref, "reconstructed", None),
            1_800_000_600,
        ),
    ];
    let mut retired = context(active_claim(&claimant, &claim_ref));
    retired.retired = true;
    let fold = fold_coding_session_handover(&events, &retired).expect("a clean fold");
    assert!(fold.retired);
    assert_eq!(fold.checkpoints.len(), 1);
    assert_eq!(fold.continuations.len(), 1);
    assert_eq!(fold.latest_authorized_checkpoint, None);
    assert_eq!(fold.active_continuation, None);
    assert_eq!(fold.claim, ClaimState::NoClaim);
    assert_eq!(fold.claim_since, None);
    assert_eq!(fold.excluded.len(), 2);
    assert!(fold
        .excluded
        .iter()
        .all(|exclusion| exclusion.reason.contains("deleted")));
}

/// Order is `(created_at, id)` ascending regardless of how the caller supplied
/// the events, so two readers of the same page agree byte for byte.
#[test]
fn the_fold_is_deterministic_whatever_order_it_is_given() {
    let founder = fixed_keys(0x11);
    let events: Vec<nostr::Event> = (0..4)
        .map(|index| {
            handover_event(
                &founder,
                "checkpoint",
                checkpoint_body(&format!("step {index}"), "partial"),
                1_800_000_100 + index,
            )
        })
        .collect();
    let ascending =
        fold_coding_session_handover(&events, &context(ClaimState::NoClaim)).expect("a clean fold");
    let reversed: Vec<nostr::Event> = events.iter().rev().cloned().collect();
    let descending = fold_coding_session_handover(&reversed, &context(ClaimState::NoClaim))
        .expect("a clean fold");
    assert_eq!(ascending, descending);
    assert_eq!(
        ascending
            .checkpoints
            .iter()
            .map(|entry| entry.created_at)
            .collect::<Vec<_>>(),
        vec![1_800_000_100, 1_800_000_101, 1_800_000_102, 1_800_000_103]
    );
}

/// Two events with the same `created_at` still fold in one fixed order,
/// decided by the event id — a hash, not an arrival time.
#[test]
fn a_created_at_tie_breaks_on_the_event_id() {
    let founder = fixed_keys(0x11);
    let a = handover_event(
        &founder,
        "checkpoint",
        checkpoint_body("first written", "partial"),
        1_800_000_100,
    );
    let b = handover_event(
        &founder,
        "checkpoint",
        checkpoint_body("second written", "partial"),
        1_800_000_100,
    );
    let fold = fold_coding_session_handover(&[a.clone(), b.clone()], &context(ClaimState::NoClaim))
        .expect("a clean fold");
    let mut expected = [a.id.to_hex(), b.id.to_hex()];
    expected.sort();
    assert_eq!(
        fold.checkpoints
            .iter()
            .map(|entry| entry.event_id.clone())
            .collect::<Vec<_>>(),
        expected.to_vec()
    );
}

#[test]
fn a_tampered_signature_fails_the_whole_set() {
    let founder = fixed_keys(0x11);
    let mut tampered = handover_event(
        &founder,
        "checkpoint",
        checkpoint_body("a real plan", "partial"),
        1_800_000_100,
    );
    tampered.content.push(' ');
    let error = fold_coding_session_handover(&[tampered], &context(ClaimState::NoClaim))
        .expect_err("a whole-set failure");
    assert!(error.contains("invalid signature"), "{error}");
}

#[test]
fn a_record_from_another_session_channel_or_genesis_fails_the_whole_set() {
    let founder = fixed_keys(0x11);
    let good = handover_event(
        &founder,
        "checkpoint",
        checkpoint_body("a real plan", "partial"),
        1_800_000_100,
    );

    let mut elsewhere = context(ClaimState::NoClaim);
    elsewhere.session_ref = "0f1e2d3c-4b5a-4978-8796-a5b4c3d2e1f0".to_owned();
    let error = fold_coding_session_handover(std::slice::from_ref(&good), &elsewhere)
        .expect_err("a whole-set failure");
    assert!(error.contains("different session or genesis"), "{error}");

    let mut other_channel = context(ClaimState::NoClaim);
    other_channel.channel_ref = "11111111-2222-4333-8444-555555555555".to_owned();
    let error =
        fold_coding_session_handover(&[good], &other_channel).expect_err("a whole-set failure");
    assert!(error.contains("scoped to channel"), "{error}");
}

#[test]
fn a_duplicate_event_fails_the_whole_set_rather_than_counting_twice() {
    let founder = fixed_keys(0x11);
    let event = handover_event(
        &founder,
        "checkpoint",
        checkpoint_body("a real plan", "partial"),
        1_800_000_100,
    );
    let error =
        fold_coding_session_handover(&[event.clone(), event], &context(ClaimState::NoClaim))
            .expect_err("a whole-set failure");
    assert!(error.contains("duplicate"), "{error}");
}

#[test]
fn a_malformed_envelope_fails_the_whole_set() {
    let founder = fixed_keys(0x11);
    let bad = EventBuilder::new(
        Kind::Custom(KIND_CODING_SESSION_HANDOVER as u16),
        "not json".to_owned(),
    )
    .tags(vec![Tag::parse(["h", CHANNEL]).expect("h")])
    .custom_created_at(Timestamp::from_secs(1_800_000_100))
    .sign_with_keys(&founder)
    .expect("sign");
    let error = fold_coding_session_handover(&[bad], &context(ClaimState::NoClaim))
        .expect_err("a whole-set failure");
    assert!(error.contains("is invalid"), "{error}");
}

// ── Supersession: recency is stated, not measured (finding 3) ─────────────

/// The composition finding, reproduced: three checkpoints in **one second**,
/// ordered by `(created_at, id)` — so the position in the list is a hash — and
/// the newest *statement* still wins, because its author said what it replaces.
#[test]
fn the_newest_statement_wins_a_same_second_tie_whichever_way_the_ids_sort() {
    let founder = fixed_keys(0x11);
    let first = handover_event(
        &founder,
        "checkpoint",
        checkpoint_body("the oldest plan", "partial"),
        1_800_000_100,
    );
    let second = handover_event(
        &founder,
        "checkpoint",
        checkpoint_body_after(Some(&first.id.to_hex()), "the middle plan", "partial"),
        1_800_000_100,
    );
    let third = handover_event(
        &founder,
        "checkpoint",
        checkpoint_body_after(Some(&second.id.to_hex()), "the newest plan", "partial"),
        1_800_000_100,
    );
    let fold = fold_coding_session_handover(
        &[first.clone(), second.clone(), third.clone()],
        &context(ClaimState::NoClaim),
    )
    .expect("a clean fold");

    assert_eq!(
        fold.latest_authorized_checkpoint.as_deref(),
        Some(third.id.to_hex().as_str()),
        "the end of the author's own chain, not the largest id"
    );
    let standing = |event: &nostr::Event| {
        fold.checkpoints
            .iter()
            .find(|entry| entry.event_id == event.id.to_hex())
            .expect("a folded checkpoint")
            .standing
    };
    assert_eq!(standing(&first), HandoverStanding::Superseded);
    assert_eq!(standing(&second), HandoverStanding::Superseded);
    assert_eq!(standing(&third), HandoverStanding::Authorized);
    // Each superseded entry carries the id that replaced it, and says so once
    // in the disclosure list.
    let superseded_by = |event: &nostr::Event| {
        fold.checkpoints
            .iter()
            .find(|entry| entry.event_id == event.id.to_hex())
            .and_then(|entry| entry.superseded_by.clone())
    };
    assert_eq!(superseded_by(&first), Some(second.id.to_hex()));
    assert_eq!(superseded_by(&second), Some(third.id.to_hex()));
    assert_eq!(superseded_by(&third), None);
    assert_eq!(
        fold.excluded
            .iter()
            .filter(|exclusion| exclusion.reason.contains("replaced by"))
            .count(),
        2
    );
}

/// Nobody supersedes anybody else's statement. An operator naming the
/// founder's checkpoint leaves it exactly where it was — and the reference
/// stays on the entry for a reader to see.
#[test]
fn a_checkpoint_naming_another_authors_record_supersedes_nothing() {
    let founder = fixed_keys(0x11);
    let operator = fixed_keys(0x22);
    let theirs = handover_event(
        &founder,
        "checkpoint",
        checkpoint_body("the founder's plan", "partial"),
        1_800_000_100,
    );
    let mine = handover_event(
        &operator,
        "checkpoint",
        checkpoint_body_after(Some(&theirs.id.to_hex()), "my plan", "partial"),
        1_800_000_050,
    );
    let fold = fold_coding_session_handover(
        &[theirs.clone(), mine.clone()],
        &context(ClaimState::NoClaim),
    )
    .expect("a clean fold");

    let founders = fold
        .checkpoints
        .iter()
        .find(|entry| entry.event_id == theirs.id.to_hex())
        .expect("the founder's checkpoint");
    assert_eq!(founders.standing, HandoverStanding::Authorized);
    assert_eq!(founders.superseded_by, None);
    // The claim is still on the wire, verbatim, for a reader to judge.
    let operators = fold
        .checkpoints
        .iter()
        .find(|entry| entry.event_id == mine.id.to_hex())
        .expect("the operator's checkpoint");
    assert_eq!(
        operators.body.prev_checkpoint_ref.as_deref(),
        Some(theirs.id.to_hex().as_str())
    );
    // Both stand, so the clock decides between two different authors — which
    // is all it was ever able to do.
    assert_eq!(
        fold.latest_authorized_checkpoint.as_deref(),
        Some(theirs.id.to_hex().as_str())
    );
}

/// An unauthorized author's statement moves nothing, and a dangling or
/// self-reference is simply ignored.
#[test]
fn an_unauthorized_dangling_or_self_reference_supersedes_nothing() {
    let founder = fixed_keys(0x11);
    let stranger = fixed_keys(0x99);
    let real = handover_event(
        &founder,
        "checkpoint",
        checkpoint_body("the real plan", "partial"),
        1_800_000_100,
    );
    // A stranger cannot retire the founder's checkpoint by naming it.
    let forged = handover_event(
        &stranger,
        "checkpoint",
        checkpoint_body_after(Some(&real.id.to_hex()), "mine now", "partial"),
        1_800_000_200,
    );
    // A reference to an event nobody supplied changes nothing either.
    let dangling = handover_event(
        &founder,
        "checkpoint",
        checkpoint_body_after(Some(&"ee".repeat(32)), "names a ghost", "partial"),
        1_800_000_300,
    );
    let fold = fold_coding_session_handover(
        &[real.clone(), forged.clone(), dangling.clone()],
        &context(ClaimState::NoClaim),
    )
    .expect("a clean fold");

    let standing = |event: &nostr::Event| {
        fold.checkpoints
            .iter()
            .find(|entry| entry.event_id == event.id.to_hex())
            .expect("a folded checkpoint")
            .standing
    };
    assert_eq!(standing(&real), HandoverStanding::Authorized);
    assert_eq!(standing(&forged), HandoverStanding::Unauthorized);
    assert_eq!(standing(&dangling), HandoverStanding::Authorized);
    assert_eq!(
        fold.latest_authorized_checkpoint.as_deref(),
        Some(dangling.id.to_hex().as_str()),
        "the dangling reference is ignored, and the newest authorized record stands"
    );
    assert!(fold
        .checkpoints
        .iter()
        .all(|entry| entry.superseded_by.is_none()));
}

/// A chain of three, all in one second: only its end stands.
///
/// A true cycle is unconstructible — an id is a hash of the content that would
/// have to name it — so the shape worth pinning is the chain, and the rule that
/// makes it safe is that every named target is retired, however long the chain
/// and whatever order the events arrive in.
#[test]
fn a_chain_of_three_leaves_only_its_end_standing() {
    let founder = fixed_keys(0x11);
    let first = handover_event(
        &founder,
        "checkpoint",
        checkpoint_body("first", "partial"),
        1_800_000_100,
    );
    let second = handover_event(
        &founder,
        "checkpoint",
        checkpoint_body_after(Some(&first.id.to_hex()), "second", "partial"),
        1_800_000_100,
    );
    let cycle = handover_event(
        &founder,
        "checkpoint",
        checkpoint_body_after(Some(&second.id.to_hex()), "third", "partial"),
        1_800_000_100,
    );
    // Handed over newest-first, to prove the pass does not depend on the
    // order the caller supplies.
    let fold = fold_coding_session_handover(
        &[cycle.clone(), second, first],
        &context(ClaimState::NoClaim),
    )
    .expect("a clean fold");
    assert_eq!(
        fold.latest_authorized_checkpoint.as_deref(),
        Some(cycle.id.to_hex().as_str())
    );
}

/// SV-41: a gate start is kind 44246, and the handover fold reads 44247 only.
/// Handed one anyway, it refuses the set rather than counting it as anything.
#[test]
fn a_gate_start_is_never_a_handover_fact() {
    let start = crate::coding_session_observation::test_gate_start_event(
        &fixed_keys(0x44),
        CHANNEL,
        SESSION,
        GENESIS,
        "cargo test",
        1_759_572_120_000,
        None,
    );
    let error = fold_coding_session_handover(&[start], &context(ClaimState::NoClaim))
        .expect_err("a 44246 row is not a handover record");
    assert!(error.contains("is invalid"), "{error}");
}
