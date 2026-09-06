//! Where a gate row's word comes from, and what Pulse prints beside it.
//!
//! Findings 93 and 95 (2026-09-05). Every fixture here signs the top-level
//! `source` the schema actually carries — never a `body.rows[].source`, which
//! the strict decoder refuses — and every verdict is read back through the
//! real renderer, so the sentence under test is the sentence both surfaces
//! print. Split out of `pulse_mission_tests.rs` so no file here passes 1,000
//! lines.

use super::*;

/// Every `gate` line for one seat, rendered.
fn gate_texts(row: &PulseMissionRow, pubkey: &str) -> Vec<String> {
    seat_lines(row, pubkey)
        .into_iter()
        .filter(|line| line.id == "gate")
        .map(|line| line.text.clone())
        .collect()
}

/// A seat with an assignment, and a provider the caller may or may not name.
struct Stage {
    founder: Keys,
    actor: Keys,
    provider: Keys,
    context: CodingSessionTeamFoldContext,
    team: Vec<Event>,
    assignment_ref: String,
}

fn stage() -> Stage {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let provider = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    let assignment = signed(&assignment(&actor), &founder, 1);
    let assignment_ref = assignment.id.to_hex();
    Stage {
        founder,
        actor,
        provider,
        context,
        team: vec![assignment],
        assignment_ref,
    }
}

impl Stage {
    fn names(&self) -> PulseMissionNames {
        names(
            vec![
                (&self.actor.public_key().to_hex(), "Bob"),
                (&self.provider.public_key().to_hex(), "Prov"),
                (&self.founder.public_key().to_hex(), "Brian"),
            ],
            None,
        )
    }

    fn providers(&self) -> Vec<String> {
        vec![self.provider.public_key().to_hex()]
    }

    /// Fold and render with the provider set as given (`None` = unresolved).
    fn render(
        &self,
        observations: &[Event],
        providers: Option<&[String]>,
    ) -> (PulseMissionFacts, PulseMissionRow) {
        let mut sources = sources(&self.context, &self.team, observations, &[]);
        sources.provider_pubkeys = providers;
        let facts = fold_pulse_mission_row(&sources, 10_000);
        let row = render_pulse_mission_lines(&facts, &self.names(), 10_000);
        (facts, row)
    }
}

// ── Finding 93: the fold's effective source, not the event's JSON ────────────

#[test]
fn a_valid_observed_row_from_a_provider_renders_observed_and_names_the_seat() {
    let stage = stage();
    let observed = observation_with_source(
        &stage.provider,
        "gate",
        "observed",
        Some(&stage.assignment_ref),
        gate_body("cargo test", "passed", "cargo test -p buzz-core"),
        2,
    );
    let providers = stage.providers();
    let (facts, row) = stage.render(&[observed], Some(&providers));

    assert!(facts.gate_provenance_checked, "a provider set was supplied");
    assert_eq!(
        gate_texts(&row, &stage.actor.public_key().to_hex()),
        vec!["Bob · cargo test: passed (observed) · cargo test -p buzz-core"],
        "a top-level `source: observed` from a known provider reaches Pulse as observed — \
         the previous reader looked in `body.rows[].source` and folded every such row to \
         declared (finding 93)"
    );
    assert!(
        gate_texts(&row, &stage.provider.public_key().to_hex()).is_empty(),
        "an observed row is attributed to the seat its assignmentRef names, not its signer"
    );
}

#[test]
fn an_observed_claim_signed_outside_the_provider_set_renders_declared() {
    let stage = stage();
    // The seat says a mechanism watched it. Nobody the caller trusts did.
    let claimed = observation_with_source(
        &stage.actor,
        "gate",
        "observed",
        Some(&stage.assignment_ref),
        gate_body("cargo test", "passed", "cargo test -p buzz-core"),
        2,
    );
    let providers = stage.providers();
    let (facts, row) = stage.render(&[claimed], Some(&providers));

    assert!(facts.gate_provenance_checked);
    assert_eq!(
        gate_texts(&row, &stage.actor.public_key().to_hex()),
        vec!["Bob · cargo test: passed (declared) · cargo test -p buzz-core"],
        "the fold demoted the misclaimed row and Pulse consumed that verdict rather than \
         re-reading the token"
    );
}

#[test]
fn a_legacy_row_with_no_source_key_renders_declared() {
    let stage = stage();
    // `observation` signs the pre-2026-09-02 shape: no `source` at all.
    let legacy = observation(
        &stage.provider,
        "gate",
        Some(&stage.assignment_ref),
        gate_body("cargo fmt", "passed", "cargo fmt --check"),
        2,
    );
    let providers = stage.providers();
    let (_, row) = stage.render(&[legacy], Some(&providers));

    // Even from a provider: absence is a claim, never promoted — and the
    // signer is the author, because only an observed row is re-attributed.
    assert_eq!(
        gate_texts(&row, &stage.provider.public_key().to_hex()),
        vec!["Prov · cargo fmt: passed (declared) · cargo fmt --check"],
    );
}

// ── Finding 95: `measured` is checked here, since the fold leaves it alone ───

#[test]
fn a_measured_row_from_a_provider_renders_measured() {
    let stage = stage();
    let bench = observation_with_source(
        &stage.provider,
        "gate",
        "measured",
        None,
        gate_body("bench", "passed", "bee bench run --task-set v1"),
        2,
    );
    let providers = stage.providers();
    let (_, row) = stage.render(&[bench], Some(&providers));

    assert_eq!(
        gate_texts(&row, &stage.provider.public_key().to_hex()),
        vec!["Prov · bench: passed (measured) · bee bench run --task-set v1"],
        "a bench row signed by a known provider keeps the bench's word"
    );
}

#[test]
fn a_measured_row_signed_outside_the_provider_set_is_a_self_reported_measurement() {
    let stage = stage();
    // The seat scores itself and calls it a measurement.
    let self_scored = observation_with_source(
        &stage.actor,
        "gate",
        "measured",
        Some(&stage.assignment_ref),
        gate_body("bench", "passed", "bee bench run --task-set v1"),
        2,
    );
    let providers = stage.providers();
    let (facts, row) = stage.render(&[self_scored], Some(&providers));

    assert!(facts.gate_provenance_checked);
    assert_eq!(
        gate_texts(&row, &stage.actor.public_key().to_hex()),
        vec!["Bob · bench: passed (self-reported measurement) · bee bench run --task-set v1"],
        "finding 95: with the provider set known, a seat's own `measured` row is rendered \
         as the claim it is, not as an independent score"
    );
    let gate = &facts.seats[0].gates[0];
    assert_eq!(gate.source, PulseGateSource::MeasuredSelfReported);
}

#[test]
fn a_self_reported_measurement_never_stands_over_a_declared_row() {
    let stage = stage();
    let declared = observation_with_source(
        &stage.actor,
        "gate",
        "declared",
        Some(&stage.assignment_ref),
        gate_body("bench", "failed", "bee bench run --task-set v1"),
        2,
    );
    let self_scored = observation_with_source(
        &stage.actor,
        "gate",
        "measured",
        Some(&stage.assignment_ref),
        gate_body("bench", "passed", "bee bench run --task-set v1"),
        3,
    );
    let providers = stage.providers();
    let (_, row) = stage.render(&[declared, self_scored], Some(&providers));

    // Both are the seat's own words, so the newer one is shown and nothing
    // says it displaced a claim: "over a declared row" is reserved for a
    // mechanism's row standing over the subject's.
    assert_eq!(
        gate_texts(&row, &stage.actor.public_key().to_hex()),
        vec!["Bob · bench: passed (self-reported measurement) · bee bench run --task-set v1"],
    );
}

#[test]
fn an_observed_row_standing_over_a_self_reported_measurement_says_so() {
    let stage = stage();
    let self_scored = observation_with_source(
        &stage.actor,
        "gate",
        "measured",
        Some(&stage.assignment_ref),
        gate_body("cargo test", "passed", "cargo test -p buzz-core"),
        2,
    );
    let observed = observation_with_source(
        &stage.provider,
        "gate",
        "observed",
        Some(&stage.assignment_ref),
        gate_body("cargo test", "failed", "cargo test -p buzz-core"),
        3,
    );
    let providers = stage.providers();
    let (_, row) = stage.render(&[self_scored, observed], Some(&providers));

    assert_eq!(
        gate_texts(&row, &stage.actor.public_key().to_hex()),
        vec!["Bob · cargo test: failed (observed, over a declared row) · cargo test -p buzz-core"],
        "a self-reported measurement is a claim, and a mechanism's row displacing it is \
         disclosed exactly as it would be over a declared row"
    );
}

// ── Unknown is not false: no provider set, nothing checked, said so ──────────

#[test]
fn no_provider_set_leaves_every_word_standing_and_prints_unverified_beside_it() {
    let stage = stage();
    let claimed_observed = observation_with_source(
        &stage.actor,
        "gate",
        "observed",
        Some(&stage.assignment_ref),
        gate_body("cargo test", "passed", "cargo test -p buzz-core"),
        2,
    );
    let bench = observation_with_source(
        &stage.provider,
        "gate",
        "measured",
        None,
        gate_body("bench", "passed", "bee bench run --task-set v1"),
        3,
    );
    let declared = observation_with_source(
        &stage.actor,
        "gate",
        "declared",
        Some(&stage.assignment_ref),
        gate_body("cargo fmt", "passed", "cargo fmt --check"),
        4,
    );
    let (facts, row) = stage.render(&[claimed_observed, bench, declared], None);

    assert!(
        !facts.gate_provenance_checked,
        "`None` is not an empty set: the fold checked nobody and the row carries that"
    );
    // Unchanged classification — the seat's `observed` claim is not demoted,
    // because nothing contradicted it — but every mechanism's word says nobody
    // verified it. A claim needs no such note.
    assert_eq!(
        gate_texts(&row, &stage.actor.public_key().to_hex()),
        vec![
            "Bob · cargo fmt: passed (declared) · cargo fmt --check",
            "Bob · cargo test: passed (observed, unverified) · cargo test -p buzz-core",
        ],
    );
    assert_eq!(
        gate_texts(&row, &stage.provider.public_key().to_hex()),
        vec!["Prov · bench: passed (measured, unverified) · bee bench run --task-set v1"],
        "with no provider set a measured row is neither trusted nor called self-reported"
    );
}

#[test]
fn an_empty_provider_set_is_an_answer_and_verifies_nobody() {
    let stage = stage();
    let bench = observation_with_source(
        &stage.provider,
        "gate",
        "measured",
        None,
        gate_body("bench", "passed", "bee bench run --task-set v1"),
        2,
    );
    let none: Vec<String> = Vec::new();
    let (facts, row) = stage.render(&[bench], Some(&none));

    assert!(facts.gate_provenance_checked);
    assert_eq!(
        gate_texts(&row, &stage.provider.public_key().to_hex()),
        vec!["Prov · bench: passed (self-reported measurement) · bee bench run --task-set v1"],
    );
}
