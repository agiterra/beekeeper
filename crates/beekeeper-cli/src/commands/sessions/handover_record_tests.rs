//! Unit tests for the **records** `bee sessions handover` writes: the kind
//! 44247 envelope, the brief a reconstruction starts from, and which
//! checkpoint that is.
//!
//! A sibling of `handover_wire_tests.rs`, which covers the authority chain and
//! its receipts. Split by subject, and so that neither file passes 1,000
//! lines.

use beekeeper_core::coding_session_authority_claim::ClaimState;
use beekeeper_core::coding_session_command::CodingSessionTarget;
use beekeeper_core::coding_session_handover::{
    validate_coding_session_handover_envelope, CodingSessionHandoverArtifact,
    CodingSessionHandoverArtifactKind, CodingSessionHandoverBody, CodingSessionHandoverCheckpoint,
    CodingSessionHandoverContinuation, CodingSessionHandoverDecision, CodingSessionHandoverMode,
    CodingSessionHandoverPreserved, CodingSessionHandoverRevision, CodingSessionHandoverTest,
    CodingSessionHandoverTestOutcome,
};
use nostr::Keys;

use super::*;

const CHANNEL: &str = "e0d3f1b8-8c66-4c62-9ef1-3fa933b32f86";
const SESSION: &str = "1f2e3d4c-5b6a-4798-8765-43210fedcba9";
const GENESIS: &str = "abababababababababababababababababababababababababababababababab";

fn hex64(byte: &str) -> String {
    byte.repeat(32)
}

// ── the record's envelope ────────────────────────────────────────────────

fn test_client() -> BeekeeperClient {
    BeekeeperClient::new(
        "https://relay.invalid".to_owned(),
        Keys::generate(),
        None,
        None,
    )
    .expect("client")
}

fn sample_checkpoint() -> CodingSessionHandoverCheckpoint {
    CodingSessionHandoverCheckpoint {
        prev_checkpoint_ref: None,
        task: "finish the handover fold".to_owned(),
        assignment_refs: vec![hex64("44")],
        decisions: vec![CodingSessionHandoverDecision {
            event_id: hex64("1a"),
            summary: "capture through a temporary index".to_owned(),
        }],
        revision: CodingSessionHandoverRevision {
            repo_ref: Some("beekeeper".to_owned()),
            base_sha: Some("9".repeat(40)),
            head_sha: Some("a".repeat(40)),
            branch: Some("work/handover".to_owned()),
            dirty: true,
            preserved: CodingSessionHandoverPreserved::Partial,
        },
        artifacts: vec![CodingSessionHandoverArtifact {
            kind: CodingSessionHandoverArtifactKind::WipRef,
            repo_ref: "beekeeper".to_owned(),
            r#ref: Some("refs/heads/wip/owner/1f2e3d4c".to_owned()),
            sha: Some("a".repeat(40)),
            event_id: None,
            hash: None,
            base_sha: None,
            bytes: None,
        }],
        tests: vec![CodingSessionHandoverTest {
            name: "unit".to_owned(),
            command: "cargo test -p beekeeper-cli".to_owned(),
            outcome: CodingSessionHandoverTestOutcome::Failed,
        }],
        unresolved: vec!["whether the fence covers siblings".to_owned()],
        next_action: "run the gate and read its last line".to_owned(),
        missing: vec!["huge.bin: over the per-file capture bound".to_owned()],
    }
}

#[test]
fn a_checkpoint_event_carries_the_five_tags_in_the_order_the_validator_wants() {
    let client = test_client();
    let event = build_handover_event(
        &client,
        CHANNEL,
        SESSION,
        GENESIS,
        CodingSessionHandoverBody::Checkpoint(sample_checkpoint()),
    )
    .expect("build");
    let tags: Vec<Vec<String>> = event
        .tags
        .iter()
        .map(|tag| tag.as_slice().to_vec())
        .collect();
    assert_eq!(
        tags,
        vec![
            vec!["h".to_owned(), CHANNEL.to_owned()],
            vec!["d".to_owned(), SESSION.to_owned()],
            vec!["csh-v".to_owned(), "csh1".to_owned()],
            vec!["csh-genesis".to_owned(), GENESIS.to_owned()],
            vec!["csh-type".to_owned(), "checkpoint".to_owned()],
        ]
    );
    validate_coding_session_handover_envelope(&event)
        .expect("the relay's own validator accepts it");
}

#[test]
fn a_continuation_event_is_labelled_by_its_mode_and_validates() {
    let client = test_client();
    let event = build_handover_event(
        &client,
        CHANNEL,
        SESSION,
        GENESIS,
        CodingSessionHandoverBody::Continuation(CodingSessionHandoverContinuation {
            claim_ref: hex64("aa"),
            mode: CodingSessionHandoverMode::Reconstructed,
            checkpoint_ref: Some(hex64("cd")),
            target: CodingSessionTarget {
                driver: "claude-agent-acp".to_owned(),
                instance_id: "inst".to_owned(),
                session_id: "sess".to_owned(),
                generation: 1,
            },
            recovered: vec!["wip-ref refs/heads/wip/owner/1f2e3d4c at aaaa".to_owned()],
            missing: vec!["uncommitted bytes over the bound".to_owned()],
            note: None,
        }),
    )
    .expect("build");
    let payload = validate_coding_session_handover_envelope(&event).expect("validate");
    assert_eq!(payload.handover_type.as_str(), "continuation");
}

#[test]
fn an_out_of_bounds_checkpoint_is_refused_before_it_is_signed() {
    let client = test_client();
    let mut body = sample_checkpoint();
    body.task = "x".repeat(8 * 1024);
    let error = build_handover_event(
        &client,
        CHANNEL,
        SESSION,
        GENESIS,
        CodingSessionHandoverBody::Checkpoint(body),
    )
    .expect_err("must refuse");
    assert!(matches!(error, CliError::Usage(_)), "got {error}");
}

#[test]
fn a_checkpoint_that_preserves_nothing_may_not_claim_a_patch_artifact() {
    // The one cross-field honesty rule 44247 enforces on its own, asserted
    // here so this command can never be the producer that trips it.
    let client = test_client();
    let mut body = sample_checkpoint();
    body.revision.preserved = CodingSessionHandoverPreserved::None;
    body.artifacts = vec![CodingSessionHandoverArtifact {
        kind: CodingSessionHandoverArtifactKind::Patch,
        repo_ref: "beekeeper".to_owned(),
        r#ref: None,
        sha: None,
        event_id: Some(hex64("ef")),
        hash: None,
        base_sha: Some("9".repeat(40)),
        bytes: Some(12),
    }];
    assert!(build_handover_event(
        &client,
        CHANNEL,
        SESSION,
        GENESIS,
        CodingSessionHandoverBody::Checkpoint(body)
    )
    .is_err());
}

// ── the reconstruction brief ─────────────────────────────────────────────

#[test]
fn the_brief_carries_every_section_including_what_did_not_travel() {
    let body = sample_checkpoint();
    let text = super::super::handover_render::render_initial_turn(
        &body,
        Some(&hex64("cd")),
        &hex64("0a"),
        SESSION,
    );
    for expected in [
        "## Task",
        "finish the handover fold",
        "## Decisions already taken",
        "capture through a temporary index",
        "## Revision",
        "preserved: partial",
        "## Artifacts",
        "refs/heads/wip/owner/1f2e3d4c",
        "## Tests",
        "unit: failed",
        "## Unresolved",
        "## Next action",
        "run the gate and read its last line",
        "## Not preserved",
        "huge.bin",
    ] {
        assert!(text.contains(expected), "the brief must carry {expected:?}");
    }
}

#[test]
fn the_brief_says_the_native_context_did_not_travel() {
    let text = super::super::handover_render::render_initial_turn(
        &sample_checkpoint(),
        None,
        &hex64("0a"),
        SESSION,
    );
    assert!(text.contains(super::super::handover_render::RECONSTRUCTION_LIMIT));
    assert!(
        text.contains("no authorized checkpoint existed"),
        "a brief with no checkpoint reference says so rather than printing an empty id"
    );
}

#[test]
fn every_handover_surface_says_it_hands_over_the_whole_session() {
    let disclosure = super::super::handover_render::WHOLE_SESSION_DISCLOSURE;
    assert!(disclosure.contains("whole session"));
    assert!(
        disclosure.contains("not one slice of the work"),
        "the sentence must rule the narrow reading out: {disclosure}"
    );
    let text = super::super::handover_render::render_initial_turn(
        &sample_checkpoint(),
        None,
        &hex64("0a"),
        SESSION,
    );
    assert!(
        text.contains(disclosure),
        "the reconstruction brief repeats it too"
    );
}

#[test]
fn the_brief_is_bounded_and_says_when_it_was_cut() {
    let mut body = sample_checkpoint();
    body.unresolved = (0..32)
        .map(|index| format!("{index}: {}", "q".repeat(500)))
        .collect();
    let text =
        super::super::handover_render::render_initial_turn(&body, None, &hex64("0a"), SESSION);
    assert!(
        text.len() <= super::super::handover_render::MAX_INITIAL_TURN_BYTES,
        "the brief travels as an initialTurn and must fit its ceiling; it was {} bytes",
        text.len()
    );
    assert!(
        text.contains("[truncated:"),
        "a cut brief must say it was cut rather than read complete"
    );
}

#[test]
fn bounding_text_never_splits_a_character() {
    let text = "é".repeat(200);
    let bounded = super::super::handover_render::bounded_text(&text, 120);
    assert!(bounded.len() <= 120 || bounded.contains("[truncated:"));
    // The assertion that matters: the result is valid UTF-8 by construction,
    // and `bounded_text` returning at all means it never split a code point.
    assert!(bounded.chars().count() > 0);
}

#[test]
fn verification_notes_always_print_both_lists() {
    let empty = super::super::handover_render::VerificationNotes::default();
    let rendered = empty.render();
    assert!(rendered.contains("verified:") && rendered.contains("(nothing)"));
    assert!(
        rendered.contains("not verified:") && rendered.contains("(nothing outstanding)"),
        "an empty 'not verified' list is printed rather than omitted, so a reader is never left \
         to infer that everything was checked: {rendered}"
    );
}

// ── which checkpoint a reconstruction starts from ────────────────────────

/// Sign one kind 44247 checkpoint whose body says what it replaces.
fn checkpoint_event(
    signer: &Keys,
    created_at: u64,
    prev: Option<&str>,
    task: &str,
) -> nostr::Event {
    let payload = beekeeper_core::coding_session_handover::CodingSessionHandoverPayload {
        schema: beekeeper_core::coding_session_handover::CODING_SESSION_HANDOVER_SCHEMA.to_owned(),
        session_ref: SESSION.to_owned(),
        genesis_ref: GENESIS.to_owned(),
        handover_type:
            beekeeper_core::coding_session_handover::CodingSessionHandoverType::Checkpoint,
        body: CodingSessionHandoverBody::Checkpoint(CodingSessionHandoverCheckpoint {
            prev_checkpoint_ref: prev.map(str::to_owned),
            task: task.to_owned(),
            assignment_refs: Vec::new(),
            decisions: Vec::new(),
            revision: CodingSessionHandoverRevision {
                repo_ref: None,
                base_sha: None,
                head_sha: None,
                branch: None,
                dirty: false,
                preserved: CodingSessionHandoverPreserved::All,
            },
            artifacts: Vec::new(),
            tests: Vec::new(),
            unresolved: Vec::new(),
            next_action: "carry on".to_owned(),
            missing: Vec::new(),
        }),
    };
    beekeeper_sdk::coding_session_handover::build_coding_session_handover(CHANNEL, payload)
        .expect("builder")
        .custom_created_at(nostr::Timestamp::from(created_at))
        .sign_with_keys(signer)
        .expect("sign")
}

fn founder_fold_context(
    founder: &Keys,
) -> beekeeper_core::coding_session_handover_fold::HandoverFoldContext {
    beekeeper_core::coding_session_handover_fold::HandoverFoldContext {
        channel_ref: CHANNEL.to_owned(),
        session_ref: SESSION.to_owned(),
        genesis_ref: GENESIS.to_owned(),
        founder_pubkey: founder.public_key().to_hex(),
        grants: Vec::new(),
        seats: Vec::new(),
        claim: ClaimState::NoClaim,
        claim_since: None,
        retired: false,
    }
}

#[test]
fn two_same_second_checkpoints_resolve_to_the_one_that_names_the_other() {
    use beekeeper_core::coding_session_handover_fold::{
        fold_coding_session_handover, HandoverStanding,
    };

    // Both published in the same second. `(created_at, id)` ordering would
    // therefore be decided by a **hash**, and in the composition run that made
    // a reconstruction start from an older statement whose `missing` list was
    // empty. The author's own `prevCheckpointRef` is what decides instead.
    let founder = Keys::generate();
    let older = checkpoint_event(&founder, 1_700_000_000, None, "the first statement");
    let newer = checkpoint_event(
        &founder,
        1_700_000_000,
        Some(&older.id.to_hex()),
        "what actually happened since",
    );
    assert_eq!(
        older.created_at, newer.created_at,
        "the fixture must actually collide in one second"
    );

    // Fed newest-first, the order a relay page arrives in.
    let fold = fold_coding_session_handover(
        &[newer.clone(), older.clone()],
        &founder_fold_context(&founder),
    )
    .expect("fold");

    assert_eq!(
        fold.latest_authorized_checkpoint.as_deref(),
        Some(newer.id.to_hex().as_str()),
        "the checkpoint that names the other is the one a reconstruction starts from"
    );
    let replaced = fold
        .checkpoints
        .iter()
        .find(|entry| entry.event_id == older.id.to_hex())
        .expect("the replaced checkpoint is still listed");
    assert_eq!(
        replaced.standing,
        HandoverStanding::Superseded,
        "history, not a deletion: it was written and signed"
    );
    assert_eq!(
        replaced.superseded_by.as_deref(),
        Some(newer.id.to_hex().as_str()),
        "and it names what replaced it, so a surface can say so"
    );
}

#[test]
fn a_new_checkpoint_names_this_authors_own_latest_and_not_somebody_elses() {
    use beekeeper_core::coding_session_handover_fold::fold_coding_session_handover;

    // `handover checkpoint` fills `prevCheckpointRef` from
    // `HandoverState::latest_checkpoint_by(caller)`. This asserts the rule it
    // applies: this author's own newest still-standing checkpoint, never the
    // umbrella's, which may belong to somebody else.
    let founder = Keys::generate();
    let other = Keys::generate();
    let mine_first = checkpoint_event(&founder, 1_700_000_000, None, "mine, first");
    let mine_second = checkpoint_event(
        &founder,
        1_700_000_010,
        Some(&mine_first.id.to_hex()),
        "mine, second",
    );
    // The founder's fold context makes only the founder authorized, so a
    // second author's record is listed and never chosen — which is the point:
    // the per-author answer must not reach for it either.
    let theirs = checkpoint_event(&other, 1_700_000_020, None, "theirs, newest");

    let fold = fold_coding_session_handover(
        &[mine_first.clone(), mine_second.clone(), theirs],
        &founder_fold_context(&founder),
    )
    .expect("fold");

    let mine_latest = fold
        .checkpoints
        .iter()
        .rfind(|entry| {
            entry.author == founder.public_key().to_hex()
                && entry.standing
                    == beekeeper_core::coding_session_handover_fold::HandoverStanding::Authorized
        })
        .expect("this author has a standing checkpoint");
    assert_eq!(
        mine_latest.event_id,
        mine_second.id.to_hex(),
        "the next checkpoint by this author replaces this author's latest, and the superseded \
         first one is skipped"
    );
}
