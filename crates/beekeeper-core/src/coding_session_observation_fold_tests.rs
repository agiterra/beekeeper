//! What the observer's projection says, and what it refuses to say.

use nostr::{EventBuilder, Keys, Kind, Tag};
use serde_json::{json, Value};

use super::*;

const SESSION: &str = "dc580cfb-6c80-4fc2-8f4e-dfc328acf222";
const CHANNEL: &str = "d3e440ea-89f8-4aee-8a02-17edc3e7272e";
const GENESIS: &str = "ce5d87ed1b9b4416bb0aa37ea0fb451f211289c54099882917f4ad538d51519b";

fn context(assignments: Vec<String>) -> CodingSessionObservationFoldContext {
    context_with_providers(assignments, None)
}

fn context_with_providers(
    assignments: Vec<String>,
    provider_pubkeys: Option<Vec<String>>,
) -> CodingSessionObservationFoldContext {
    CodingSessionObservationFoldContext {
        session_ref: SESSION.to_owned(),
        genesis_ref: GENESIS.to_owned(),
        known_assignment_refs: assignments,
        provider_pubkeys,
    }
}

fn id(byte: &str) -> String {
    byte.repeat(32)
}

/// Sign one observation with the exact envelope, for `keys`.
fn observation(
    keys: &Keys,
    observation_type: &str,
    assignment_ref: Option<&str>,
    body: Value,
) -> Event {
    signed_for(
        keys,
        observation_type,
        SESSION,
        GENESIS,
        assignment_ref,
        body,
    )
}

fn signed_for(
    keys: &Keys,
    observation_type: &str,
    session: &str,
    genesis: &str,
    assignment_ref: Option<&str>,
    body: Value,
) -> Event {
    signed_with_source(
        keys,
        observation_type,
        session,
        genesis,
        "declared",
        assignment_ref,
        body,
    )
}

#[allow(clippy::too_many_arguments)]
fn signed_with_source(
    keys: &Keys,
    observation_type: &str,
    session: &str,
    genesis: &str,
    source: &str,
    assignment_ref: Option<&str>,
    body: Value,
) -> Event {
    let content = json!({
        "schema": CODING_SESSION_OBSERVATION_SCHEMA,
        "sessionRef": session,
        "genesisRef": genesis,
        "type": observation_type,
        "source": source,
        "assignmentRef": assignment_ref.map_or(Value::Null, |value| json!(value)),
        "body": body,
    })
    .to_string();
    EventBuilder::new(
        Kind::Custom(KIND_CODING_SESSION_OBSERVATION as u16),
        content,
    )
    .tags(vec![
        Tag::parse(["h", CHANNEL]).expect("h"),
        Tag::parse(["d", session]).expect("d"),
        Tag::parse(["csob-v", CODING_SESSION_OBSERVATION_SCHEMA]).expect("csob-v"),
        Tag::parse(["csob-genesis", genesis]).expect("csob-genesis"),
        Tag::parse(["csob-type", observation_type]).expect("csob-type"),
    ])
    .sign_with_keys(keys)
    .expect("sign")
}

fn gate_row(gate: &str, outcome: &str) -> Value {
    json!({
        "gate": gate,
        "outcome": outcome,
        "command": "just ci",
        "summary": Value::Null,
        "durationMs": Value::Null,
    })
}

fn finding(finding_id: &str, disposition: &str) -> Value {
    json!({
        "findingId": finding_id,
        "title": "the waiting state does not fire",
        "disposition": disposition,
        "detail": Value::Null,
        "refs": [],
        "decisionRef": Value::Null,
    })
}

fn checkpoint(phase: &str) -> Value {
    json!({
        "phase": phase,
        "testsWritten": 2,
        "testsRed": 2,
        "testsGreen": 0,
        "lastCommand": Value::Null,
        "lastSummary": Value::Null,
        "note": Value::Null,
    })
}

fn phase(name: &str) -> Value {
    json!({
        "phase": name,
        "startedAtMs": 1_756_800_000_000u64,
        "endedAtMs": Value::Null,
        "durationMs": Value::Null,
    })
}

#[test]
fn the_four_facts_land_in_their_own_collections() {
    let seat = Keys::generate();
    let events = vec![
        observation(&seat, "checkpoint", None, checkpoint("red")),
        observation(
            &seat,
            "gate",
            None,
            json!({ "rows": [gate_row("just ci", "passed")] }),
        ),
        observation(&seat, "finding", None, finding("16", "fixed")),
        observation(&seat, "phase", None, phase("red")),
    ];
    let fold = fold_coding_session_observations(&events, &context(Vec::new()));
    assert_eq!(fold.checkpoints.len(), 1);
    assert_eq!(fold.gates.len(), 1);
    assert_eq!(fold.findings.len(), 1);
    assert_eq!(fold.phases.len(), 1);
    assert!(fold.unresolved.is_empty());
    assert!(fold.ignored.is_empty(), "{:?}", fold.ignored);
    assert!(!fold.truncated.any());
    assert_eq!(fold.gates[0].row.gate, "just ci");
    assert_eq!(fold.gates[0].author_pubkey, seat.public_key().to_hex());
    assert_eq!(fold.findings[0].body.finding_id, "16");
}

#[test]
fn two_findings_with_one_id_fold_to_the_later_disposition_and_list_both_ids() {
    let seat = Keys::generate();
    let first = observation(&seat, "finding", None, finding("16", "found"));
    let second = observation(&seat, "finding", None, finding("16", "fixed"));
    let (first_id, second_id) = (first.id.to_hex(), second.id.to_hex());

    let fold = fold_coding_session_observations(&[first, second], &context(Vec::new()));
    assert_eq!(fold.findings.len(), 1, "one findingId is one row");
    let entry = &fold.findings[0];
    assert_eq!(
        entry.body.disposition,
        CodingSessionObservationDisposition::Fixed,
        "the later statement is the one shown"
    );
    assert_eq!(
        entry.event_ids,
        vec![first_id, second_id],
        "both events are listed: there is no supersedes key, and the older one is still on \
         the wire"
    );
}

#[test]
fn a_second_author_saying_the_same_finding_id_is_a_second_row() {
    // The dedupe key is (author, findingId). Two seats numbering their own
    // findings 1, 2, 3 must not overwrite each other.
    let one = Keys::generate();
    let two = Keys::generate();
    let fold = fold_coding_session_observations(
        &[
            observation(&one, "finding", None, finding("1", "found")),
            observation(&two, "finding", None, finding("1", "wont-fix")),
        ],
        &context(Vec::new()),
    );
    assert_eq!(fold.findings.len(), 2);
    assert_ne!(
        fold.findings[0].author_pubkey,
        fold.findings[1].author_pubkey
    );
}

#[test]
fn a_later_gate_row_replaces_the_earlier_one_for_that_author_and_gate() {
    let seat = Keys::generate();
    let first = observation(
        &seat,
        "gate",
        None,
        json!({ "rows": [gate_row("just ci", "failed"), gate_row("cargo fmt", "passed")] }),
    );
    let second = observation(
        &seat,
        "gate",
        None,
        json!({ "rows": [gate_row("just ci", "passed")] }),
    );
    let (first_id, second_id) = (first.id.to_hex(), second.id.to_hex());

    let fold = fold_coding_session_observations(&[first, second], &context(Vec::new()));
    assert_eq!(fold.gates.len(), 2, "two gates, not three rows");
    let ci = fold
        .gates
        .iter()
        .find(|entry| entry.row.gate == "just ci")
        .expect("just ci");
    assert_eq!(ci.row.outcome, CodingSessionObservationGateOutcome::Passed);
    assert_eq!(ci.event_ids, vec![first_id.clone(), second_id]);
    let fmt = fold
        .gates
        .iter()
        .find(|entry| entry.row.gate == "cargo fmt")
        .expect("cargo fmt");
    assert_eq!(fmt.event_ids, vec![first_id]);
    assert_eq!(fmt.row.outcome, CodingSessionObservationGateOutcome::Passed);
}

#[test]
fn newest_is_supplied_order_and_no_authored_number_moves_it() {
    // I4: the author's own clock and the author's own measurements never
    // decide ordering. Both events here are signed at whatever `now` is, and
    // the *second* one claims to have started a year earlier and taken longer
    // — none of which changes which statement is shown.
    let seat = Keys::generate();
    let older_claim = json!({
        "phase": "gates",
        "startedAtMs": 1u64,
        "endedAtMs": 2u64,
        "durationMs": 1u64,
    });
    let first = observation(&seat, "phase", None, phase("gates"));
    let second = signed_for(&seat, "phase", SESSION, GENESIS, None, older_claim);
    let (first_id, second_id) = (first.id.to_hex(), second.id.to_hex());

    let fold = fold_coding_session_observations(&[first, second], &context(Vec::new()));
    assert_eq!(
        fold.phases
            .iter()
            .map(|entry| entry.event_id.clone())
            .collect::<Vec<_>>(),
        vec![first_id, second_id],
        "phases are listed in supplied order; a claimed start time reorders nothing"
    );
    assert_eq!(fold.phases[1].body.started_at_ms, 1);
}

#[test]
fn a_dangling_assignment_ref_is_disclosed_and_excludes_nothing() {
    let seat = Keys::generate();
    let known = id("ab");
    let missing = id("cd");
    let resolved = observation(&seat, "checkpoint", Some(&known), checkpoint("green"));
    let dangling = observation(&seat, "finding", Some(&missing), finding("7", "cross-lane"));
    let dangling_id = dangling.id.to_hex();

    let fold =
        fold_coding_session_observations(&[resolved, dangling], &context(vec![known.clone()]));
    assert_eq!(fold.unresolved.len(), 1);
    assert_eq!(fold.unresolved[0].event_id, dangling_id);
    assert_eq!(fold.unresolved[0].assignment_ref, missing);
    // And the observation itself is folded exactly as it would be with no
    // pointer at all: an observation cannot deny anything, including itself.
    assert_eq!(fold.findings.len(), 1);
    assert_eq!(fold.findings[0].event_ids, vec![dangling_id]);
    assert_eq!(fold.checkpoints.len(), 1);
    assert_eq!(
        fold.checkpoints[0].assignment_ref.as_deref(),
        Some(known.as_str())
    );
    assert!(fold.ignored.is_empty());
}

#[test]
fn an_unreadable_event_costs_only_itself() {
    // The whole reason this is not a 44244 subtype: on that kind a bad
    // envelope is a whole-set hard error and the session reads as a broken
    // mission. Here it is one listed line and every other fact survives.
    let seat = Keys::generate();
    let good = observation(
        &seat,
        "gate",
        None,
        json!({ "rows": [gate_row("just ci", "passed")] }),
    );
    let good_id = good.id.to_hex();
    let malformed = EventBuilder::new(
        Kind::Custom(KIND_CODING_SESSION_OBSERVATION as u16),
        "{\"schema\":\"nope\"}".to_owned(),
    )
    .sign_with_keys(&seat)
    .expect("sign");
    let malformed_id = malformed.id.to_hex();
    let other_session = signed_for(
        &seat,
        "checkpoint",
        "11111111-2222-4333-8444-555555555555",
        GENESIS,
        None,
        checkpoint("red"),
    );
    let other_session_id = other_session.id.to_hex();

    let fold =
        fold_coding_session_observations(&[malformed, good, other_session], &context(Vec::new()));
    assert_eq!(fold.gates.len(), 1);
    assert_eq!(fold.gates[0].event_ids, vec![good_id]);
    let ignored: Vec<&str> = fold
        .ignored
        .iter()
        .map(|entry| entry.event_id.as_str())
        .collect();
    assert!(ignored.contains(&malformed_id.as_str()), "{ignored:?}");
    assert!(ignored.contains(&other_session_id.as_str()), "{ignored:?}");
    assert!(
        fold.ignored
            .iter()
            .any(|entry| entry.reason.contains("different session or genesis")),
        "{:?}",
        fold.ignored
    );
}

#[test]
fn every_collection_is_bounded_and_says_what_it_dropped() {
    let seat = Keys::generate();
    let events: Vec<Event> = (0..MAX_OBSERVATION_FOLD_ENTRIES + 3)
        .map(|index| {
            observation(
                &seat,
                "phase",
                None,
                json!({
                    "phase": format!("phase-{index}"),
                    "startedAtMs": 1_756_800_000_000u64,
                    "endedAtMs": Value::Null,
                    "durationMs": Value::Null,
                }),
            )
        })
        .collect();
    let fold = fold_coding_session_observations(&events, &context(Vec::new()));
    assert_eq!(fold.phases.len(), MAX_OBSERVATION_FOLD_ENTRIES);
    assert_eq!(
        fold.truncated.phases, 3,
        "what did not fit is counted, not hidden"
    );
    assert!(fold.truncated.any());
}

// ── Fix round 1 ────────────────────────────────────────────────────────────

/// **REVIEW-L1 F1.** The module's own doc says the event signature is the sole
/// author, and `NIP-CSOB.md` says the same — but the fold read
/// `event.pubkey` and never checked that the key had signed anything. So a
/// rendered gate row could name a pubkey that never signed, and since the
/// dedupe key is `(author, gate)` / `(author, findingId)`, the forged author
/// also decided which statement was "newest" for that key.
///
/// The 44244 fold verifies as its first act
/// (`coding_session_team_transaction_fold.rs:344`). This one now does the same,
/// and routes the failure into `ignored` — which is exactly what this kind's
/// never-fails contract is for.
#[test]
fn an_event_whose_signature_does_not_check_is_ignored_not_attributed() {
    let seat = Keys::generate();
    let stranger = Keys::generate();
    let good = observation(
        &seat,
        "gate",
        None,
        json!({ "rows": [gate_row("just ci", "passed")] }),
    );
    let good_id = good.id.to_hex();

    // The forgery: a validly signed observation whose `pubkey` is rewritten to
    // somebody else's. Every byte else — id, signature, content, tags — is the
    // real one's, so only a signature check can catch it.
    let mut forged_json = serde_json::to_value(observation(
        &seat,
        "gate",
        None,
        json!({ "rows": [gate_row("cargo fmt", "failed")] }),
    ))
    .expect("event JSON");
    forged_json["pubkey"] = json!(stranger.public_key().to_hex());
    let forged: Event = serde_json::from_value(forged_json).expect("event");
    let forged_id = forged.id.to_hex();

    let fold = fold_coding_session_observations(&[good, forged], &context(Vec::new()));
    assert_eq!(
        fold.gates.len(),
        1,
        "only the observation that actually verifies is folded: {:?}",
        fold.gates
    );
    assert_eq!(fold.gates[0].event_ids, vec![good_id]);
    assert_eq!(fold.gates[0].author_pubkey, seat.public_key().to_hex());
    assert!(
        !fold
            .gates
            .iter()
            .any(|entry| entry.author_pubkey == stranger.public_key().to_hex()),
        "a key that never signed is never attributed an observation"
    );
    let ignored = fold
        .ignored
        .iter()
        .find(|entry| entry.event_id == forged_id)
        .expect("the forgery is listed, not silently dropped");
    assert!(
        ignored.reason.contains("signature"),
        "the reason names the signature: {}",
        ignored.reason
    );
}

/// **REVIEW-L1 F2.** `event_ids` grew one entry per republish with no bound and
/// no counter, in the one kind whose own justification is that every seat
/// writes many an hour — an unremarked exception to §8 I10. It is now bounded
/// like every other collection here: the newest
/// [`MAX_OBSERVATION_ENTRY_EVENT_IDS`] are kept, because the newest statement
/// is the one shown and its neighbours are the ones a reader is most likely to
/// want, and what fell off is counted.
#[test]
fn an_entrys_event_id_list_is_bounded_and_says_what_it_dropped() {
    let seat = Keys::generate();
    let republished = MAX_OBSERVATION_ENTRY_EVENT_IDS + 4;
    let mut ids = Vec::new();
    let mut events = Vec::new();
    for index in 0..republished {
        // A distinct `command` per event so no two ids collide.
        let event = observation(
            &seat,
            "gate",
            None,
            json!({ "rows": [{
                "gate": "just ci",
                "outcome": if index + 1 == republished { "passed" } else { "failed" },
                "command": format!("just ci # {index}"),
                "summary": Value::Null,
                "durationMs": Value::Null,
            }] }),
        );
        ids.push(event.id.to_hex());
        events.push(event);
    }

    let fold = fold_coding_session_observations(&events, &context(Vec::new()));
    assert_eq!(fold.gates.len(), 1, "one author, one gate, one row");
    let entry = &fold.gates[0];
    assert_eq!(
        entry.row.outcome,
        CodingSessionObservationGateOutcome::Passed,
        "the newest statement is still the one shown"
    );
    assert_eq!(entry.event_ids.len(), MAX_OBSERVATION_ENTRY_EVENT_IDS);
    assert_eq!(
        entry.event_ids.last(),
        ids.last(),
        "the newest id is kept; the oldest are the ones that fall off"
    );
    assert_eq!(
        entry.event_ids,
        ids[republished - MAX_OBSERVATION_ENTRY_EVENT_IDS..],
        "the kept ids are the newest window, in supplied order"
    );
    assert_eq!(
        entry.dropped_event_ids, 4,
        "what did not fit is counted, not hidden"
    );
    assert_eq!(fold.truncated.entry_event_ids, 4);
    assert!(fold.truncated.any());
}

/// The same bound on a finding, and a fold that stayed inside it says zero
/// rather than nothing — empty is not "nothing dropped".
#[test]
fn a_finding_inside_the_bound_reports_zero_dropped() {
    let seat = Keys::generate();
    let events = vec![
        observation(&seat, "finding", None, finding("16", "found")),
        observation(&seat, "finding", None, finding("16", "fixed")),
    ];
    let fold = fold_coding_session_observations(&events, &context(Vec::new()));
    assert_eq!(fold.findings.len(), 1);
    assert_eq!(fold.findings[0].event_ids.len(), 2);
    assert_eq!(fold.findings[0].dropped_event_ids, 0);
    assert_eq!(fold.truncated.entry_event_ids, 0);
}

#[test]
fn an_observed_gate_row_and_a_declared_one_never_merge() {
    // Brian's 2026-09-02 ruling, in the fold: the mechanism's measurement and
    // the subject's claim about the same gate are two rows, and neither is
    // allowed to become the other. Same author on purpose — the realistic case
    // is two different keys, so keying on the author alone would pass this for
    // a reason that is only a coincidence.
    let author = Keys::generate();
    let declared = signed_with_source(
        &author,
        "gate",
        SESSION,
        GENESIS,
        "declared",
        None,
        json!({ "rows": [gate_row("cargo test -p beekeeper-cli", "passed")] }),
    );
    let observed = signed_with_source(
        &author,
        "gate",
        SESSION,
        GENESIS,
        "observed",
        None,
        json!({ "rows": [gate_row("cargo test -p beekeeper-cli", "failed")] }),
    );

    let fold = fold_coding_session_observations(&[declared, observed], &context(Vec::new()));
    assert_eq!(fold.gates.len(), 2, "{:?}", fold.gates);
    let observed_row = fold
        .gates
        .iter()
        .find(|entry| entry.source == CodingSessionObservationSource::Observed)
        .expect("the observed row survives");
    let declared_row = fold
        .gates
        .iter()
        .find(|entry| entry.source == CodingSessionObservationSource::Declared)
        .expect("the declared row survives");
    assert_eq!(
        observed_row.row.outcome,
        CodingSessionObservationGateOutcome::Failed
    );
    assert_eq!(
        declared_row.row.outcome,
        CodingSessionObservationGateOutcome::Passed
    );
}

#[test]
fn a_finding_keeps_its_provenance_apart_the_same_way() {
    let author = Keys::generate();
    let declared = signed_with_source(
        &author,
        "finding",
        SESSION,
        GENESIS,
        "declared",
        None,
        finding("F1", "wont-fix"),
    );
    let observed = signed_with_source(
        &author,
        "finding",
        SESSION,
        GENESIS,
        "observed",
        None,
        finding("F1", "found"),
    );
    let fold = fold_coding_session_observations(&[declared, observed], &context(Vec::new()));
    assert_eq!(fold.findings.len(), 2, "{:?}", fold.findings);
}

#[test]
fn the_provenance_word_is_a_closed_set_refused_by_name() {
    let author = Keys::generate();
    let invented = signed_with_source(
        &author,
        "gate",
        SESSION,
        GENESIS,
        "inferred",
        None,
        json!({ "rows": [gate_row("just ci", "passed")] }),
    );
    let event_id = invented.id.to_hex();
    let fold = fold_coding_session_observations(&[invented], &context(Vec::new()));
    assert!(fold.gates.is_empty());
    let ignored = fold
        .ignored
        .iter()
        .find(|entry| entry.event_id == event_id)
        .expect("an unknown provenance word is listed, never guessed at");
    assert!(ignored.reason.contains("\"source\""), "{}", ignored.reason);
    assert!(
        ignored.reason.contains("\"observed\""),
        "{}",
        ignored.reason
    );
    assert!(
        ignored.reason.contains("\"declared\""),
        "{}",
        ignored.reason
    );
}

/// REVIEW-L5 **F1**, second half: a newer statement wins, but never silently.
///
/// The key is `(author, source, gate)` and newest-wins, which is right — a seat
/// that re-runs a gate should show the newer result. What was wrong is that the
/// displaced statement left no trace but an event id in a list: a `failed` row
/// replaced by a `passed` row read exactly like a gate that had only ever
/// passed. The count is now in `truncated`, and every surface says it.
#[test]
fn a_replaced_gate_row_is_counted_rather_than_silently_dropped() {
    let author = Keys::generate();
    let failed = signed_with_source(
        &author,
        "gate",
        SESSION,
        GENESIS,
        "observed",
        None,
        json!({ "rows": [gate_row("cargo test", "failed")] }),
    );
    let passed = signed_with_source(
        &author,
        "gate",
        SESSION,
        GENESIS,
        "observed",
        None,
        json!({ "rows": [gate_row("cargo test", "passed")] }),
    );

    let fold = fold_coding_session_observations(&[failed, passed], &context(Vec::new()));
    assert_eq!(fold.gates.len(), 1, "newest still wins");
    assert_eq!(
        fold.gates[0].row.outcome,
        CodingSessionObservationGateOutcome::Passed
    );
    assert_eq!(
        fold.truncated.displaced_gates, 1,
        "the statement it replaced is counted, not hidden"
    );
    assert!(fold.truncated.any());
}

/// The same rule for findings: a disposition that replaced another says so.
#[test]
fn a_replaced_finding_is_counted_too() {
    let author = Keys::generate();
    let found = signed_with_source(
        &author,
        "finding",
        SESSION,
        GENESIS,
        "declared",
        None,
        finding("F1", "found"),
    );
    let wont_fix = signed_with_source(
        &author,
        "finding",
        SESSION,
        GENESIS,
        "declared",
        None,
        finding("F1", "wont-fix"),
    );
    let fold = fold_coding_session_observations(&[found, wont_fix], &context(Vec::new()));
    assert_eq!(fold.findings.len(), 1);
    assert_eq!(fold.truncated.displaced_findings, 1);
}

/// A first statement displaces nothing, and says zero rather than nothing.
#[test]
fn a_single_statement_displaces_nothing() {
    let author = Keys::generate();
    let only = signed_with_source(
        &author,
        "gate",
        SESSION,
        GENESIS,
        "observed",
        None,
        json!({ "rows": [gate_row("cargo test", "failed")] }),
    );
    let fold = fold_coding_session_observations(&[only], &context(Vec::new()));
    assert_eq!(fold.truncated.displaced_gates, 0);
    assert_eq!(fold.truncated.displaced_findings, 0);
    assert!(!fold.truncated.any());
}

// -- REVIEW-L5 F2: `observed` is a claim until the signer is checked ---------

/// A seat that signs its own row `observed` is folded as **declared**.
///
/// The CLI refuses to mint one (`observations.rs`, deliberately no flag), but
/// the wire does not, and the reader ranked observed rows first and printed
/// "the record names the watcher" over them. Provenance a reader trusts has to
/// be *verified*, not self-asserted.
#[test]
fn an_observed_row_signed_by_a_non_provider_is_folded_as_declared_and_disclosed() {
    let provider = Keys::generate();
    let seat = Keys::generate();
    let forged = signed_with_source(
        &seat,
        "gate",
        SESSION,
        GENESIS,
        "observed",
        None,
        json!({ "rows": [gate_row("cargo test", "passed")] }),
    );
    let forged_id = forged.id.to_hex();
    let genuine = signed_with_source(
        &provider,
        "gate",
        SESSION,
        GENESIS,
        "observed",
        None,
        json!({ "rows": [gate_row("cargo clippy", "failed")] }),
    );

    let fold = fold_coding_session_observations(
        &[forged, genuine],
        &context_with_providers(Vec::new(), Some(vec![provider.public_key().to_hex()])),
    );

    let seat_row = fold
        .gates
        .iter()
        .find(|entry| entry.row.gate == "cargo test")
        .expect("the row is still folded — a bad claim costs only its claim");
    assert_eq!(
        seat_row.source,
        CodingSessionObservationSource::Declared,
        "a claim nobody can verify is a claim"
    );
    let provider_row = fold
        .gates
        .iter()
        .find(|entry| entry.row.gate == "cargo clippy")
        .expect("the provider's row");
    assert_eq!(
        provider_row.source,
        CodingSessionObservationSource::Observed
    );

    assert_eq!(fold.misclaimed_observed.len(), 1);
    assert_eq!(fold.misclaimed_observed[0].event_id, forged_id);
    assert_eq!(
        fold.misclaimed_observed[0].author_pubkey,
        seat.public_key().to_hex()
    );
    assert!(fold.provenance_checked);
}

/// A caller that supplied no provider set has not verified anything, and the
/// fold says so rather than downgrading every row (unknown is not false).
#[test]
fn no_provider_set_leaves_the_claim_standing_and_says_it_was_not_checked() {
    let seat = Keys::generate();
    let claimed = signed_with_source(
        &seat,
        "gate",
        SESSION,
        GENESIS,
        "observed",
        None,
        json!({ "rows": [gate_row("cargo test", "passed")] }),
    );
    let fold = fold_coding_session_observations(&[claimed], &context(Vec::new()));
    assert_eq!(
        fold.gates[0].source,
        CodingSessionObservationSource::Observed,
        "nothing was checked, so nothing is contradicted"
    );
    assert!(fold.misclaimed_observed.is_empty());
    assert!(
        !fold.provenance_checked,
        "and the surface must be able to say the check did not run"
    );
}

/// An empty provider set is a real answer — this session has no provider whose
/// word counts — so every `observed` claim in it is unverified.
#[test]
fn an_empty_provider_set_verifies_nobody() {
    let seat = Keys::generate();
    let claimed = signed_with_source(
        &seat,
        "gate",
        SESSION,
        GENESIS,
        "observed",
        None,
        json!({ "rows": [gate_row("cargo test", "passed")] }),
    );
    let fold = fold_coding_session_observations(
        &[claimed],
        &context_with_providers(Vec::new(), Some(Vec::new())),
    );
    assert_eq!(
        fold.gates[0].source,
        CodingSessionObservationSource::Declared
    );
    assert_eq!(fold.misclaimed_observed.len(), 1);
    assert!(fold.provenance_checked);
}

/// A *declared* row from the provider stays declared — the check only ever
/// removes an unearned claim, never adds one.
#[test]
fn the_check_never_promotes_a_declared_row() {
    let provider = Keys::generate();
    let declared = signed_with_source(
        &provider,
        "gate",
        SESSION,
        GENESIS,
        "declared",
        None,
        json!({ "rows": [gate_row("cargo test", "passed")] }),
    );
    let fold = fold_coding_session_observations(
        &[declared],
        &context_with_providers(Vec::new(), Some(vec![provider.public_key().to_hex()])),
    );
    assert_eq!(
        fold.gates[0].source,
        CodingSessionObservationSource::Declared
    );
    assert!(fold.misclaimed_observed.is_empty());
}

// ── Finding 79: the relay's page is newest-first ───────────────────────────

/// **Finding 79.** The fold defines "newest" as last in the order supplied,
/// and the relay's push gate fed it a page read in storage's canonical order
/// — `created_at DESC, id ASC`, newest **first** — so the fold crowned the
/// *oldest* row per `(author, gate)`. Measured on Andy's machine: a seat's
/// first `cargo fmt` row was `failed`/dirty at the base commit (exit 127,
/// hermit not on `PATH` yet); four later rows were green on the pushed
/// commit; the relay refused every push with "gate `cargo fmt` has no
/// observed green row".
///
/// Two halves. The plain fold's contract is unchanged and is proven here on
/// purpose — the same newest-first slice handed to it still crowns the red
/// row, which is exactly why the page entry point exists. The page fold
/// reverses before folding: green wins, the red row's id stays listed and
/// first, and the displacement is counted.
#[test]
fn a_relay_page_read_newest_first_crowns_the_newest_row_through_the_page_fold() {
    let provider = Keys::generate();
    let red_at_base = signed_with_source(
        &provider,
        "gate",
        SESSION,
        GENESIS,
        "observed",
        None,
        json!({ "rows": [{
            "gate": "cargo fmt",
            "outcome": "failed",
            "command": "cargo fmt --all --check",
            "summary": "exit 127: cargo: command not found",
            "durationMs": 12,
            "headSha": "1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b",
            "dirty": true,
        }] }),
    );
    let green_on_head = signed_with_source(
        &provider,
        "gate",
        SESSION,
        GENESIS,
        "observed",
        None,
        json!({ "rows": [{
            "gate": "cargo fmt",
            "outcome": "passed",
            "command": "cargo fmt --all --check",
            "summary": Value::Null,
            "durationMs": 900,
            "headSha": "2f73f558f2f73f558f2f73f558f2f73f558f2f73",
            "dirty": false,
        }] }),
    );
    // The relay's page: newest first.
    let page = vec![green_on_head.clone(), red_at_base.clone()];
    let context = context_with_providers(Vec::new(), Some(vec![provider.public_key().to_hex()]));

    // The plain fold's contract, unchanged: last in the slice is newest, so
    // a page handed to it as read crowns the base-commit row. This is the
    // bug, kept as the statement of why the page fold exists.
    let as_read = fold_coding_session_observations(&page, &context);
    assert_eq!(as_read.gates.len(), 1);
    assert_eq!(
        as_read.gates[0].row.outcome,
        CodingSessionObservationGateOutcome::Failed,
        "the plain fold over a newest-first page crowns the oldest row — finding 79's shape"
    );

    // The page fold reverses first, so the newest statement wins.
    let fold = fold_coding_session_observation_page(&page, &context);
    assert_eq!(fold.gates.len(), 1, "one author, one gate: one entry");
    let entry = &fold.gates[0];
    assert_eq!(
        entry.row.outcome,
        CodingSessionObservationGateOutcome::Passed,
        "the newest row is the one shown"
    );
    assert_eq!(
        entry.row.head_sha.as_deref(),
        Some("2f73f558f2f73f558f2f73f558f2f73f558f2f73")
    );
    assert_eq!(entry.row.dirty, Some(false));
    assert_eq!(
        entry.event_ids,
        vec![red_at_base.id.to_hex(), green_on_head.id.to_hex()],
        "both ids stay listed, oldest first, newest last"
    );
    assert_eq!(
        fold.truncated.displaced_gates, 1,
        "the red row was displaced, and the count says so"
    );
    assert!(fold.provenance_checked);
    assert!(fold.misclaimed_observed.is_empty());
}

/// The page fold changes only the order; everything else the fold says about
/// a page is the same whichever entry point read it, and an empty page folds
/// to nothing either way.
#[test]
fn the_page_fold_and_the_plain_fold_agree_on_everything_but_order() {
    let seat = Keys::generate();
    let oldest_first = vec![
        observation(&seat, "checkpoint", None, checkpoint("red")),
        observation(&seat, "finding", None, finding("16", "found")),
        observation(&seat, "finding", None, finding("16", "fixed")),
        observation(&seat, "phase", None, phase("red")),
    ];
    let newest_first: Vec<Event> = oldest_first.iter().rev().cloned().collect();
    let plain = fold_coding_session_observations(&oldest_first, &context(Vec::new()));
    let page = fold_coding_session_observation_page(&newest_first, &context(Vec::new()));
    assert_eq!(plain, page);
    assert_eq!(page.findings.len(), 1);
    assert_eq!(
        page.findings[0].body.disposition,
        CodingSessionObservationDisposition::Fixed
    );
    assert_eq!(
        fold_coding_session_observation_page(&[], &context(Vec::new())),
        fold_coding_session_observations(&[], &context(Vec::new()))
    );
}
