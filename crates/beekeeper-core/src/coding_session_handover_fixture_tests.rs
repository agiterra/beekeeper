//! The one fixture the Desktop twin is pinned to.
//!
//! Generated **from this module's own types**, never hand-written: the twin
//! (`desktop/src/features/coding-sessions/lib/codingSessionHandoverFold.ts`)
//! reads the same bytes, so a field this fold renames and the decoder does not
//! is a failing Rust test rather than a silently empty panel (the 44244/44246
//! pattern).
//!
//! Regenerate with:
//!
//! ```text
//! BUZZ_UPDATE_FIXTURES=1 cargo test -p beekeeper-core --lib the_typescript_fixture_is_this_folds_real_output
//! ```
//!
//! # Byte stability, and the one thing that threatened it
//!
//! Fixed keys and a fixed `created_at` make every event **id** stable, but a
//! schnorr signature is randomized by default (`Keys::sign_schnorr` draws from
//! the OS), so signing the same event twice produced two different `sig`
//! values and the fixture would have drifted on every run for no reason at
//! all. [`FixedAuxRand`] supplies the BIP-340 auxiliary randomness instead, so
//! the signatures are reproducible **and real** — the Desktop twin can verify
//! them with `nostr-tools`, which a stripped or placeholder signature would
//! not survive.

use nostr::prelude::rand::{CryptoRng, Error as RandError, RngCore};
use nostr::{EventBuilder, Keys, Kind, Tag, Timestamp};
use serde_json::{json, Value};

use super::tests::{
    checkpoint_body, checkpoint_body_after, continuation_body, fixed_keys, handover_event, CHANNEL,
    GENESIS, SESSION,
};
use super::*;
use crate::coding_session_authority_claim::CurrentClaim;
use crate::coding_session_authority_transition::{
    CodingSessionAuthorityTransitionPayload, CODING_SESSION_AUTHORITY_TRANSITION_TAG_VERSION,
};
use crate::kind::KIND_CODING_SESSION_AUTHORITY_TRANSITION;

/// Path of the fixture the Desktop twin reads, relative to this crate.
const TS_TWIN_FIXTURE: &str =
    "../../desktop/src/features/coding-sessions/lib/codingSessionHandover.fixture.json";

/// A constant auxiliary-randomness source, so signing is reproducible.
///
/// BIP-340 accepts any 32 bytes of auxiliary randomness — including none —
/// and the signature it produces is valid either way. Test-only, and never
/// reachable from a signing path that matters: nothing in this crate's public
/// surface takes an RNG.
struct FixedAuxRand;

impl RngCore for FixedAuxRand {
    fn next_u32(&mut self) -> u32 {
        0x5a5a_5a5a
    }

    fn next_u64(&mut self) -> u64 {
        0x5a5a_5a5a_5a5a_5a5a
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        dest.fill(0x5a);
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), RandError> {
        self.fill_bytes(dest);
        Ok(())
    }
}

impl CryptoRng for FixedAuxRand {}

/// Sign one event reproducibly.
fn sign_fixed(builder: EventBuilder, keys: &Keys) -> nostr::Event {
    builder
        .build(keys.public_key())
        .sign_with_ctx(nostr::SECP256K1, &mut FixedAuxRand, keys)
        .expect("sign with fixed auxiliary randomness")
}

/// Sign one 44247 event reproducibly, with the exact five-tag envelope.
fn fixed_handover_event(
    keys: &Keys,
    record_type: &str,
    body: Value,
    created_at: u64,
) -> nostr::Event {
    // Built through the same helper the fold tests use, then re-signed with
    // fixed randomness: one envelope, one place.
    let template = handover_event(keys, record_type, body, created_at);
    sign_fixed(
        EventBuilder::new(template.kind, template.content.clone())
            .tags(template.tags.to_vec())
            .custom_created_at(template.created_at),
        keys,
    )
}

/// Sign the accepted `takeover` link the claim in this fixture comes from.
fn fixed_takeover_event(keys: &Keys, body_pubkey: &str, created_at: u64) -> nostr::Event {
    let payload = CodingSessionAuthorityTransitionPayload::new_takeover(
        GENESIS.to_owned(),
        Some("11".repeat(32)),
        2,
        keys.public_key().to_hex(),
        body_pubkey.to_owned(),
    );
    payload.validate().expect("a well-formed takeover");
    let content = serde_json::to_string(&payload).expect("serialize takeover");
    sign_fixed(
        EventBuilder::new(
            Kind::Custom(KIND_CODING_SESSION_AUTHORITY_TRANSITION as u16),
            content,
        )
        .tags(vec![
            Tag::parse(["h", CHANNEL]).expect("h"),
            Tag::parse(["csat-v", CODING_SESSION_AUTHORITY_TRANSITION_TAG_VERSION])
                .expect("csat-v"),
            Tag::parse(["csat-genesis", GENESIS]).expect("csat-genesis"),
        ])
        .custom_created_at(Timestamp::from_secs(created_at)),
        keys,
    )
}

#[test]
fn the_typescript_fixture_is_this_folds_real_output() {
    let founder = fixed_keys(0x11);
    let claimant = fixed_keys(0x22);
    let stranger = fixed_keys(0x99);
    let body_pubkey = fixed_keys(0xdd).public_key().to_hex();

    // The accepted claim link, and the relay receipt shape a consumer folds it
    // from. The receipt is the *content object* the relay puts inside a kind
    // 40099 system message (`side_effects.rs::handle_coding_session_authority
    // _transition_accepted`), so the twin's receipt reader is pinned to the
    // same keys the relay writes.
    let takeover = fixed_takeover_event(&claimant, &body_pubkey, 1_800_000_500);
    let receipt = json!({
        "type": "coding_session_authority_transition_accepted",
        "genesisRef": GENESIS,
        "acceptedEventId": takeover.id.to_hex(),
        "seq": 2,
        "transitionType": "takeover",
        "granteePubkey": claimant.public_key().to_hex(),
        "bodyPubkey": body_pubkey,
    });

    let checkpoint = fixed_handover_event(
        &founder,
        "checkpoint",
        checkpoint_body("Fetch the wip ref and continue from 2b2b", "partial"),
        1_800_000_100,
    );
    // The finding-3 case, pinned: the founder's **second** checkpoint, written
    // in the same second as the first and naming it. Without
    // `prevCheckpointRef` the two would be ordered by the hash of their ids and
    // a reconstruction could start from either; with it, one supersedes the
    // other whichever way the ids sort.
    let superseding = fixed_handover_event(
        &founder,
        "checkpoint",
        checkpoint_body_after(
            Some(&checkpoint.id.to_hex()),
            "Apply the patch artifact before continuing",
            "partial",
        ),
        1_800_000_100,
    );
    // Written by somebody with no standing: listed, labelled, never used.
    let unauthorized = fixed_handover_event(
        &stranger,
        "checkpoint",
        checkpoint_body("A stranger's plan", "partial"),
        1_800_000_200,
    );
    let continuation = fixed_handover_event(
        &claimant,
        "continuation",
        continuation_body(
            &takeover.id.to_hex(),
            "reconstructed",
            Some(&superseding.id.to_hex()),
        ),
        1_800_000_600,
    );

    let context = HandoverFoldContext {
        channel_ref: CHANNEL.to_owned(),
        session_ref: SESSION.to_owned(),
        genesis_ref: GENESIS.to_owned(),
        founder_pubkey: founder.public_key().to_hex(),
        grants: vec![(claimant.public_key().to_hex(), 1_800_000_000)],
        seats: Vec::new(),
        claim: ClaimState::Active(CurrentClaim {
            claimant: claimant.public_key().to_hex(),
            body_pubkey: body_pubkey.clone(),
            accepted_event_id: takeover.id.to_hex(),
            seq: 2,
        }),
        claim_since: Some(1_800_000_500),
        retired: false,
    };
    let events = vec![
        checkpoint.clone(),
        superseding.clone(),
        unauthorized.clone(),
        continuation.clone(),
    ];
    let fold = fold_coding_session_handover(&events, &context).expect("the canonical fold");

    // The **same** records, read under a retired umbrella. Nothing about the
    // events changes; only the context does — and the twin diverged from this
    // fold precisely because the fixture had no such case to pin (review
    // follow-up). Note what the fold does *not* do: it judges the
    // continuation's standing against the claim it was given, uncleared, and
    // only then excludes everything as deleted. So `continuations[0].standing`
    // is still `authorized` while `activeContinuation` is null — the record is
    // real history, and the session is over.
    let retired_context = HandoverFoldContext {
        retired: true,
        ..context.clone()
    };
    let retired_events = vec![checkpoint.clone(), continuation.clone()];
    let retired_fold = fold_coding_session_handover(&retired_events, &retired_context)
        .expect("the canonical fold of a retired umbrella");

    let generated = serde_json::to_string_pretty(&json!({
        "events": [
            serde_json::to_value(&checkpoint).expect("checkpoint event"),
            serde_json::to_value(&superseding).expect("superseding checkpoint event"),
            serde_json::to_value(&continuation).expect("continuation event"),
            serde_json::to_value(&unauthorized).expect("unauthorized checkpoint event"),
            serde_json::to_value(&takeover).expect("takeover event"),
            receipt,
        ],
        "fold": serde_json::to_value(&fold).expect("fold"),
        // Additive: the two top-level keys above keep their exact meaning, so
        // a twin already pinned to them is untouched.
        "retired": {
            "events": [
                serde_json::to_value(&checkpoint).expect("checkpoint event"),
                serde_json::to_value(&continuation).expect("continuation event"),
            ],
            "fold": serde_json::to_value(&retired_fold).expect("retired fold"),
        },
    }))
    .expect("serialize the fixture")
        + "\n";

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(TS_TWIN_FIXTURE);
    if std::env::var("BUZZ_UPDATE_FIXTURES").is_ok() {
        std::fs::write(&path, &generated).expect("write the fixture");
    }
    let stored = std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!("read {}: {error}", path.display());
    });
    if stored != generated {
        // Written where a person can diff it rather than only described, the
        // way the Pulse fixtures do it.
        let scratch = std::env::temp_dir().join("codingSessionHandover.fixture.produced.json");
        let _ = std::fs::write(&scratch, &generated);
        panic!(
            "the Desktop twin fixture is stale; the current bytes were written to {} — \
             regenerate with BUZZ_UPDATE_FIXTURES=1",
            scratch.display()
        );
    }

    // The fixture must actually exercise every branch the twin reads, or it
    // pins nothing worth pinning.
    let wire: Value = serde_json::from_str(&generated).expect("fixture JSON");
    assert_eq!(wire["events"].as_array().map(Vec::len), Some(6));
    assert_eq!(
        wire["fold"]["checkpoints"].as_array().map(Vec::len),
        Some(3)
    );
    let entry = |event_id: String| -> Value {
        wire["fold"]["checkpoints"]
            .as_array()
            .expect("checkpoints")
            .iter()
            .find(|entry| entry["eventId"] == json!(event_id))
            .cloned()
            .expect("a folded checkpoint")
    };
    // The superseded checkpoint and the one that replaced it share a
    // `created_at`, so their order in this list is decided by the hash of their
    // ids and nothing else — which is exactly why the standing, never the
    // position, is what a reader uses.
    let superseded = entry(checkpoint.id.to_hex());
    let latest = entry(superseding.id.to_hex());
    assert_eq!(superseded["standing"], json!("superseded"));
    assert_eq!(superseded["supersededBy"], json!(superseding.id.to_hex()));
    assert_eq!(
        superseded["body"]["prevCheckpointRef"],
        json!(null),
        "an author's first checkpoint replaces nothing, and says so with a null"
    );
    assert_eq!(latest["standing"], json!("authorized"));
    assert_eq!(latest["supersededBy"], json!(null));
    assert_eq!(
        latest["body"]["prevCheckpointRef"],
        json!(checkpoint.id.to_hex())
    );
    assert_eq!(
        entry(unauthorized.id.to_hex())["standing"],
        json!("unauthorized")
    );
    assert_eq!(
        wire["fold"]["latestAuthorizedCheckpoint"],
        json!(superseding.id.to_hex()),
        "the newest statement, not the newest timestamp"
    );
    assert_eq!(
        wire["fold"]["activeContinuation"],
        json!(continuation.id.to_hex())
    );
    assert_eq!(wire["fold"]["claim"]["state"], json!("active"));
    assert_eq!(wire["fold"]["claim"]["bodyPubkey"], json!(body_pubkey));
    // Two disclosures: the stranger's checkpoint, and the superseded one.
    assert_eq!(wire["fold"]["excluded"].as_array().map(Vec::len), Some(2));
    assert!(
        wire["fold"]["excluded"]
            .as_array()
            .expect("exclusions")
            .iter()
            .any(
                |exclusion| exclusion["eventId"] == json!(checkpoint.id.to_hex())
                    && exclusion["reason"]
                        .as_str()
                        .is_some_and(|reason| reason.contains("replaced by"))
            ),
        "a superseded checkpoint says why it is not the one to reconstruct from"
    );
    assert_eq!(wire["fold"]["retired"], json!(false));

    // The retired scenario, pinned field by field: everything is listed, and
    // nothing is offered.
    let retired = &wire["retired"];
    assert_eq!(retired["events"].as_array().map(Vec::len), Some(2));
    assert_eq!(retired["fold"]["retired"], json!(true));
    assert_eq!(retired["fold"]["claim"], json!({ "state": "no-claim" }));
    assert_eq!(retired["fold"]["claimSince"], json!(null));
    assert_eq!(retired["fold"]["latestAuthorizedCheckpoint"], json!(null));
    assert_eq!(retired["fold"]["activeContinuation"], json!(null));
    assert_eq!(
        retired["fold"]["checkpoints"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(
        retired["fold"]["checkpoints"][0]["standing"],
        json!("authorized"),
        "standing is what the author held, and deletion does not rewrite that"
    );
    assert_eq!(
        retired["fold"]["continuations"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(
        retired["fold"]["continuations"][0]["standing"],
        json!("authorized"),
        "the continuation is judged against the uncleared claim, then excluded"
    );
    let reasons: Vec<&str> = retired["fold"]["excluded"]
        .as_array()
        .expect("exclusions")
        .iter()
        .filter_map(|exclusion| exclusion["reason"].as_str())
        .collect();
    assert_eq!(
        reasons,
        vec![
            "this session was deleted: nothing here is reconstructed or resumed",
            "this session was deleted: nothing here is reconstructed or resumed",
        ],
        "every record of a retired umbrella carries the same sentence"
    );
    assert_eq!(
        retired["fold"]["excluded"][0]["eventId"],
        json!(checkpoint.id.to_hex())
    );
    assert_eq!(
        retired["fold"]["excluded"][1]["eventId"],
        json!(continuation.id.to_hex())
    );
}

/// The signatures in the fixture are real, and reproducible: sign the same
/// event twice with the fixed auxiliary randomness and the bytes match, and
/// the result verifies.
#[test]
fn the_fixtures_signatures_are_both_reproducible_and_valid() {
    let founder = fixed_keys(0x11);
    let first = fixed_handover_event(
        &founder,
        "checkpoint",
        checkpoint_body("stability", "partial"),
        1_800_000_100,
    );
    let second = fixed_handover_event(
        &founder,
        "checkpoint",
        checkpoint_body("stability", "partial"),
        1_800_000_100,
    );
    assert_eq!(
        first.sig, second.sig,
        "a fixture that re-signs must not drift"
    );
    crate::verify_event(&first).expect("a real signature, not a placeholder");
}
