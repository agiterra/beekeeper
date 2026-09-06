//! What moved, the policy line, the names, and the eight sibling keys.
//!
//! Split out of `pulse_mission_tests.rs` so no file here passes 1,000 lines.

use super::*;

// ── L9.4 what moved, from the relay's own ref state ──────────────────────────

#[test]
fn two_pushes_to_one_ref_leave_one_row_at_the_newer_sha() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    // 30618 is parameterized-replaceable: the relay holds where the ref stands
    // now and who moved it last, never a push history.
    let ref_state = vec![
        PulseRefState {
            ref_name: "refs/heads/wip/builder/1f2e3d4c".into(),
            sha: id("9a")[..40].to_owned(),
            pusher_pubkey: actor.public_key().to_hex(),
            as_of: Some(9_000),
        },
        PulseRefState {
            ref_name: "refs/heads/wip/builder/1f2e3d4c".into(),
            sha: id("9b")[..40].to_owned(),
            pusher_pubkey: actor.public_key().to_hex(),
            as_of: Some(9_640),
        },
    ];
    let facts = fold_pulse_mission_row(&sources(&context, &[], &[], &ref_state), 10_000);
    assert_eq!(facts.moved.len(), 1, "one ref, one row");
    let names = names(vec![(&actor.public_key().to_hex(), "Bob")], None);
    let row = render_pulse_mission_lines(&facts, &names, 10_000);
    assert_eq!(row.moved.len(), 1);
    assert_eq!(
        row.moved[0].sha,
        id("9b")[..40],
        "the newer SHA is where the ref stands"
    );
    for word in ["pushes", "2 commits", "history"] {
        assert!(
            !row.moved[0].lines[0].text.contains(word),
            "the row claims where the ref stands, never a push history: {:?}",
            row.moved[0].lines[0].text
        );
    }
    let wip = seat_lines(&row, &actor.public_key().to_hex())
        .into_iter()
        .find(|line| line.id == "wip")
        .expect("a wip line");
    assert!(
        wip.text
            .starts_with("Bob's local commits: 9b9b9b9b on wip/builder/1f2e3d4c"),
        "{:?}",
        wip.text
    );
}

#[test]
fn a_repo_with_no_ref_state_says_so_rather_than_showing_nothing() {
    let founder = Keys::generate();
    let context = context(&founder, Vec::new());
    let facts = fold_pulse_mission_row(&sources(&context, &[], &[], &[]), 10_000);
    let row = render_pulse_mission_lines(&facts, &PulseMissionNames::default(), 10_000);
    assert_eq!(
        line(&row, "ref-state").expect("ref-state line").text,
        "No ref state on the wire for this repo"
    );
}

#[test]
fn a_member_with_no_wip_ref_reads_not_shared_and_never_off() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    let facts = fold_pulse_mission_row(&sources(&context, &[], &[], &[]), 10_000);
    let names = names(vec![(&actor.public_key().to_hex(), "Ira")], None);
    let row = render_pulse_mission_lines(&facts, &names, 10_000);
    let wip = seat_lines(&row, &actor.public_key().to_hex())
        .into_iter()
        .find(|line| line.id == "wip")
        .expect("a wip line");
    // What the relay holds, not what that person's git config says.
    assert_eq!(wip.text, "Ira's local commits: not shared");
    assert!(!wip.text.contains("off"), "{:?}", wip.text);
}

#[test]
fn the_prune_window_is_disclosed_wherever_a_wip_ref_is_shown() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    let ref_state = vec![PulseRefState {
        ref_name: "refs/heads/wip/builder/1f2e3d4c".into(),
        sha: id("9a")[..40].to_owned(),
        pusher_pubkey: actor.public_key().to_hex(),
        as_of: Some(9_640),
    }];
    let facts = fold_pulse_mission_row(&sources(&context, &[], &[], &ref_state), 10_000);
    let row = render_pulse_mission_lines(&facts, &PulseMissionNames::default(), 10_000);
    assert_eq!(
        line(&row, "wip-window")
            .expect("the window is disclosed")
            .text,
        "Wip refs are pruned when their branch merges or after 30 days"
    );
}

#[test]
fn a_landing_with_no_verdict_says_no_verdict_rather_than_nothing() {
    let founder = Keys::generate();
    let context = context(&founder, Vec::new());
    let ref_state = vec![PulseRefState {
        ref_name: "refs/heads/main".into(),
        sha: id("c1")[..40].to_owned(),
        pusher_pubkey: founder.public_key().to_hex(),
        as_of: Some(4_600),
    }];
    let facts = fold_pulse_mission_row(&sources(&context, &[], &[], &ref_state), 10_000);
    assert_eq!(facts.moved[0].kind, PulseMovedKind::Landing);
    let row = render_pulse_mission_lines(&facts, &PulseMissionNames::default(), 10_000);
    assert!(
        row.moved[0].lines[0]
            .text
            .ends_with(" · no verdict on the wire for this commit"),
        "{:?}",
        row.moved[0].lines[0].text
    );
}

#[test]
fn a_branch_named_like_a_wip_ref_is_not_one() {
    assert!(is_wip_ref("refs/heads/wip/builder/1f2e3d4c"));
    assert!(!is_wip_ref("refs/heads/feature/wip/builder"));
    assert!(!is_wip_ref("refs/heads/wip/"));
    assert!(!is_wip_ref("refs/heads/main"));
}

// ── L9.10 the policy line ────────────────────────────────────────────────────

#[test]
fn no_policy_record_reads_as_no_policy_rather_than_as_unknown() {
    let founder = Keys::generate();
    let context = context(&founder, Vec::new());
    let facts = fold_pulse_mission_row(&sources(&context, &[], &[], &[]), 10_000);
    let row = render_pulse_mission_lines(&facts, &PulseMissionNames::default(), 10_000);
    assert_eq!(
        line(&row, "policy").expect("policy line").text,
        "No policy set for this session"
    );
}

#[test]
fn a_record_setting_nothing_is_the_withdrawal_and_names_who_withdrew_it() {
    let mut facts = PulseMissionFacts {
        session_key: SESSION.into(),
        session_ref: Some(SESSION.into()),
        channel_id: CHANNEL.into(),
        name: None,
        latest_observation_at: None,
        state: PulseMissionState::Running,
        unreadable: None,
        waiting: None,
        terminal_event_id: None,
        verdict: None,
        excluded_completion: None,
        policy: PulseMissionPolicy::default(),
        seats: Vec::new(),
        moved: Vec::new(),
        timing: Vec::new(),
        seat_claims_refused: Vec::new(),
        ref_state_present: true,
        gate_provenance_checked: true,
    };
    facts.policy.author = Some(id("77"));
    facts.policy.withdrawn = true;
    let names = names(vec![(&id("77"), "Brian")], None);
    let row = render_pulse_mission_lines(&facts, &names, 10_000);
    assert_eq!(
        line(&row, "policy").expect("policy").text,
        "Policy withdrawn by Brian"
    );

    facts.policy.withdrawn = false;
    facts.policy.posture = Some("ship".into());
    facts.policy.budget_turns = Some(40);
    facts.policy.irreversible = vec!["push".into(), "deploy".into()];
    let row = render_pulse_mission_lines(&facts, &names, 10_000);
    assert_eq!(
        line(&row, "policy").expect("policy").text,
        "Policy by Brian: posture ship · budget 40 turns · irreversible push, deploy"
    );

    // An unset field is omitted, never defaulted.
    facts.policy.budget_turns = None;
    facts.policy.irreversible = Vec::new();
    let row = render_pulse_mission_lines(&facts, &names, 10_000);
    assert_eq!(
        line(&row, "policy").expect("policy").text,
        "Policy by Brian: posture ship"
    );
}

// ── Names, and the cost disclosure ───────────────────────────────────────────

#[test]
fn an_unmapped_name_renders_as_eight_hex_and_the_viewer_renders_as_you() {
    let viewer = id("88");
    let stranger = id("99");
    let names = names(vec![], Some(&viewer));
    assert_eq!(names.who(&viewer), "You");
    assert_eq!(names.who(&stranger), "99999999");
}

#[test]
fn the_cost_line_says_what_this_surface_cannot_show() {
    let founder = Keys::generate();
    let context = context(&founder, Vec::new());
    let facts = fold_pulse_mission_row(&sources(&context, &[], &[], &[]), 10_000);
    let row = render_pulse_mission_lines(&facts, &PulseMissionNames::default(), 10_000);
    let cost = row
        .timing
        .iter()
        .find(|line| line.id == "cost")
        .expect("the cost disclosure");
    assert_eq!(
        cost.text,
        "Token cost is not on this surface: Pulse reads no usage events"
    );
    assert_eq!(
        row.timing
            .iter()
            .find(|line| line.id == "timing-missing")
            .expect("timing")
            .text,
        "No phase timing on the wire for this session"
    );
}

#[test]
fn a_checkpoint_is_the_seats_liveness_and_its_test_counts() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    let observations = vec![observation(
        &actor,
        "checkpoint",
        None,
        checkpoint_body("green", 12, 12, 11),
        9_640,
    )];
    let facts = fold_pulse_mission_row(&sources(&context, &[], &observations, &[]), 10_000);
    let names = names(vec![(&actor.public_key().to_hex(), "Bob")], None);
    let row = render_pulse_mission_lines(&facts, &names, 10_000);
    let lines = seat_lines(&row, &actor.public_key().to_hex());
    assert_eq!(
        lines
            .iter()
            .find(|line| line.id == "live")
            .expect("live")
            .text,
        "Bob (builder) · green, checkpointed 6m ago"
    );
    assert_eq!(
        lines
            .iter()
            .find(|line| line.id == "checkpoint")
            .expect("checkpoint")
            .text,
        "Bob · tests 11/12 green, 12 seen red · green · 6m ago"
    );
}

#[test]
fn a_seat_claim_the_signed_chain_does_not_support_is_refused_and_disclosed() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let stranger = id("aa");
    let context = context(&founder, vec![(&actor, "builder")]);
    let mut sources = sources(&context, &[], &[], &[]);
    let claims = vec![stranger.clone(), actor.public_key().to_hex()];
    sources.claimed_seats = &claims;
    let facts = fold_pulse_mission_row(&sources, 10_000);
    assert_eq!(facts.seat_claims_refused, vec![stranger]);
    let row = render_pulse_mission_lines(&facts, &PulseMissionNames::default(), 10_000);
    assert_eq!(
        line(&row, "seat-claims-refused").expect("disclosure").text,
        "1 claimed seat refused: no accepted authority transition supports it"
    );
}

#[test]
fn an_unreported_assignment_is_what_that_seat_owes() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    let assignment = signed(&assignment(&actor), &founder, 9_000);
    let events = vec![assignment];
    let facts = fold_pulse_mission_row(&sources(&context, &events, &[], &[]), 10_000);
    let names = names(vec![(&actor.public_key().to_hex(), "Bob")], None);
    let row = render_pulse_mission_lines(&facts, &names, 10_000);
    let owed = seat_lines(&row, &actor.public_key().to_hex())
        .into_iter()
        .find(|line| line.id == "owed")
        .expect("an owed line");
    assert!(
        owed.text.starts_with("Bob owes a report on assignment "),
        "{:?}",
        owed.text
    );
    assert!(
        owed.text.ends_with(" · assigned 16m ago"),
        "{:?}",
        owed.text
    );
}

// ── L9.11 wire compatibility ─────────────────────────────────────────────────

#[test]
fn the_sibling_object_adds_exactly_eight_keys_and_nothing_else() {
    let rows = PulseMissionRows {
        missions_schema: PULSE_MISSION_ROWS_SCHEMA.into(),
        mission_scope: PULSE_MISSION_SCOPE.into(),
        missions: Vec::new(),
        mission_errors: Vec::new(),
        open_rulings: Vec::new(),
        rulings_waiting_on_viewer: Vec::new(),
        overlaps: Vec::new(),
        viewer_pubkey: None,
    };
    let value = serde_json::to_value(&rows).expect("serialize");
    let object = value.as_object().expect("an object");
    let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        vec![
            "missionErrors",
            "missionScope",
            "missions",
            "missionsSchema",
            "openRulings",
            "overlaps",
            "rulingsWaitingOnViewer",
            "viewerPubkey",
        ]
    );
    assert_eq!(rows.missions_schema, "buzz-pulse-mission-rows/v1");
    assert!(
        object.get("viewerPubkey").is_some_and(Value::is_null),
        "an optional value is JSON null, never an absent key"
    );
}

#[test]
fn kind_44240s_event_body_is_untouched_by_this_lane() {
    // The mission rows are a sibling object attached after the fold. Nothing
    // predating this lane refuses an entry, and an older digest reader ignores
    // eight unknown keys.
    let source = include_str!("pulse.rs");
    assert!(
        source.contains("pub struct PulseEntry"),
        "the 44240 body still lives where it did"
    );
    assert!(
        !source.contains("PulseMissionRow"),
        "no mission field was added to the 44240 body"
    );
    let fold = include_str!("pulse_fold.rs");
    assert!(
        !fold.contains("missionsSchema") && !fold.contains("PulseMissionRows"),
        "PulseDigest gains no field: the rows travel beside it"
    );
}

#[test]
fn a_line_id_is_from_the_closed_set_the_surfaces_hang_testids_on() {
    const MISSION: [&str; 11] = [
        "waiting",
        "state",
        "verdict",
        "excluded-completion",
        "policy",
        "seat-claims-refused",
        "ref-state",
        "wip-window",
        "not-read",
        "unreadable",
        "landing",
    ];
    const SEAT: [&str; 7] = [
        "live",
        "gate",
        "gate-truncated",
        "gate-missing",
        "checkpoint",
        "owed",
        "wip",
    ];
    const TIMING: [&str; 3] = ["timing", "timing-missing", "cost"];

    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    let facts = fold_pulse_mission_row(&sources(&context, &[], &[], &[]), 10_000);
    let row = render_pulse_mission_lines(&facts, &PulseMissionNames::default(), 10_000);
    for line in &row.lines {
        assert!(MISSION.contains(&line.id.as_str()), "{:?}", line.id);
    }
    for line in row.seats.iter().flat_map(|seat| seat.lines.iter()) {
        assert!(SEAT.contains(&line.id.as_str()), "{:?}", line.id);
    }
    for line in &row.timing {
        assert!(TIMING.contains(&line.id.as_str()), "{:?}", line.id);
    }
}

#[test]
fn a_landing_never_claims_the_missions_verdict_covers_that_commit() {
    // 30618 says a commit is *on* a branch. Nothing on the wire ties a verdict
    // to a SHA, so the row discloses the mission's newest verdict and says
    // plainly that it is not a verdict over this commit.
    let facts = PulseMissionFacts {
        session_key: SESSION.into(),
        session_ref: Some(SESSION.into()),
        channel_id: CHANNEL.into(),
        name: None,
        latest_observation_at: None,
        state: PulseMissionState::Running,
        unreadable: None,
        waiting: None,
        terminal_event_id: None,
        verdict: Some(PulseMissionVerdict {
            event_id: id("5c"),
            author: id("11"),
            token: "approve".into(),
        }),
        excluded_completion: None,
        policy: PulseMissionPolicy::default(),
        seats: Vec::new(),
        moved: vec![PulseMissionMoved {
            kind: PulseMovedKind::Landing,
            sha: id("c1")[..40].to_owned(),
            ref_name: "refs/heads/main".into(),
            author_pubkey: id("11"),
            subject: None,
            age_seconds: Some(600),
            verdict: Some(PulseMissionVerdict {
                event_id: id("5c"),
                author: id("11"),
                token: "approve".into(),
            }),
        }],
        timing: Vec::new(),
        seat_claims_refused: Vec::new(),
        ref_state_present: true,
        gate_provenance_checked: true,
    };
    let row = render_pulse_mission_lines(&facts, &PulseMissionNames::default(), 10_000);
    let text = &row.moved[0].lines[0].text;
    assert!(
        text.ends_with("which is not a verdict over this commit"),
        "{text:?}"
    );
    assert!(
        !text.contains("verdict approve by"),
        "the row never states the verdict as if it governed this SHA: {text:?}"
    );
}

// ── Fix round 1 (REVIEW-L9) ──────────────────────────────────────────────────

#[test]
fn a_surface_with_no_identity_gets_the_sentence_from_the_model() {
    // REVIEW-L9 F4.1: the CLI appended this sentence itself and Desktop had no
    // line to render, so its own e2e test injected the missing one. It is now a
    // `not-read` line the model composes, which is what both consumers read.
    let founder = Keys::generate();
    let context = context(&founder, Vec::new());
    let facts = fold_pulse_mission_row(&sources(&context, &[], &[], &[]), 10_000);

    let anonymous = PulseMissionNames::default();
    let row = render_pulse_mission_lines(&facts, &anonymous, 10_000);
    let not_read = row
        .lines
        .iter()
        .find(|line| line.id == "not-read")
        .expect("a surface with no identity says so");
    assert_eq!(not_read.text, PULSE_NO_VIEWER_IDENTITY);

    // A surface that knows the viewer makes no such claim.
    let known = PulseMissionNames {
        names: BTreeMap::new(),
        viewer: Some(id("11")),
    };
    let row = render_pulse_mission_lines(&facts, &known, 10_000);
    assert!(
        !row.lines.iter().any(|line| line.id == "not-read"),
        "{:?}",
        row.lines
    );
}

#[test]
fn an_observed_row_replacing_an_observed_row_keeps_the_disclosure() {
    // REVIEW-L9 F11: declared → observed → observed used to reset
    // `over_declared`, retiring the only evidence that a claim had been
    // displaced.
    //
    // Two *different* observers are what make this reachable: the observation
    // fold dedupes by `(author, gate)`, so one author's three rows collapse
    // before this module sees them. A provider signs an observed row under its
    // own key and it is attributed to the seat its `assignmentRef` names, so
    // two provider instances watching one seat produce exactly this sequence.
    let founder = Keys::generate();
    let actor = Keys::generate();
    let provider_a = Keys::generate();
    let provider_b = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    let assignment = signed(&assignment(&actor), &founder, 1);
    let assignment_ref = assignment.id.to_hex();

    let declared = observation_with_source(
        &actor,
        "gate",
        "declared",
        Some(&assignment_ref),
        gate_body("cargo test", "passed", "cargo test -p buzz-core"),
        2,
    );
    let observed_first = observation_with_source(
        &provider_a,
        "gate",
        "observed",
        Some(&assignment_ref),
        gate_body("cargo test", "failed", "cargo test -p buzz-core"),
        3,
    );
    let observed_second = observation_with_source(
        &provider_b,
        "gate",
        "observed",
        Some(&assignment_ref),
        gate_body("cargo test", "failed", "cargo test -p buzz-core --lib"),
        4,
    );
    let providers = vec![
        provider_a.public_key().to_hex(),
        provider_b.public_key().to_hex(),
    ];
    let team = vec![assignment];
    let observations = vec![declared, observed_first, observed_second];
    let mut sources = sources(&context, &team, &observations, &[]);
    sources.provider_pubkeys = Some(&providers);
    let facts = fold_pulse_mission_row(&sources, 10_000);

    let names = names(vec![(&actor.public_key().to_hex(), "Bob")], None);
    let row = render_pulse_mission_lines(&facts, &names, 10_000);
    let gates: Vec<&str> = row
        .seats
        .iter()
        .flat_map(|seat| seat.lines.iter())
        .filter(|line| line.id == "gate")
        .map(|line| line.text.as_str())
        .collect();
    assert_eq!(gates.len(), 1, "one seat, one gate: {gates:?}");
    assert_eq!(
        gates[0],
        "Bob · cargo test: failed (observed, over a declared row) · cargo test -p buzz-core --lib",
        "the newest observed row wins and still says it stands over a claim"
    );
}

#[test]
fn the_two_provenance_words_are_documented_as_what_they_are() {
    // REVIEW-L9 F7: the two doc comments were exactly inverted. Pin the meaning
    // to the tokens so a future swap fails here rather than misleading a reader
    // of a provenance type.
    assert_eq!(PulseGateSource::Declared.as_str(), "declared");
    assert_eq!(PulseGateSource::Observed.as_str(), "observed");
    assert_eq!(PulseGateSource::Measured.as_str(), "measured");
    assert_eq!(
        PulseGateSource::MeasuredSelfReported.as_str(),
        "self-reported measurement",
        "finding 95: a measured row nobody independent signed is rendered as the claim it is"
    );
    assert!(
        PulseGateSource::Declared.precedence() < PulseGateSource::Observed.precedence(),
        "a claim never outranks a measurement"
    );
    assert_eq!(
        PulseGateSource::MeasuredSelfReported.precedence(),
        PulseGateSource::Declared.precedence(),
        "a self-reported measurement ranks as the claim it is"
    );
    assert!(PulseGateSource::MeasuredSelfReported.is_claim());
    assert!(!PulseGateSource::Measured.is_claim());
    let source = include_str!("pulse_mission.rs");
    let declared_doc = source
        .split("pub enum PulseGateSource {")
        .nth(1)
        .expect("the enum")
        .split("Declared,")
        .next()
        .expect("the Declared arm's docs");
    assert!(
        declared_doc.contains("A claim"),
        "Declared must be documented as a claim: {declared_doc:?}"
    );
}
