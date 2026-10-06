//! Tests for the coverage fold, bound to the five frozen sequences.
//!
//! Each sequence is `events.json` + `inputs.json` + `expected-fold.json`. The
//! expected file is the **contract's** statement of this fold's output, so
//! these tests compare the whole projection, not a field of it. On top of
//! that, every sequence is folded again under every permutation of its events
//! (or a large deterministic sample when the sequence is long) and under
//! duplication, because "order-independent" is a property, not a sentence.

use super::*;

use crate::coding_session_team_transaction::{
    fold_coding_session_team_transactions, CodingSessionTeamActiveSeat,
    CodingSessionTeamFoldContext,
};
use crate::kind::{is_project_a_scoped_kind, ALL_KINDS, KIND_CODING_SESSION_TEAM_TRANSACTION};

macro_rules! sequence {
    ($name:literal) => {
        Sequence {
            name: $name,
            events: include_str!(concat!(
                "../../../conformance/project-work/fixtures/sequences/",
                $name,
                "/events.json"
            )),
            inputs: include_str!(concat!(
                "../../../conformance/project-work/fixtures/sequences/",
                $name,
                "/inputs.json"
            )),
            expected: include_str!(concat!(
                "../../../conformance/project-work/fixtures/sequences/",
                $name,
                "/expected-fold.json"
            )),
        }
    };
}

struct Sequence {
    name: &'static str,
    events: &'static str,
    inputs: &'static str,
    expected: &'static str,
}

const KETTLE_PLAN: &str =
    include_str!("../../../conformance/project-work/fixtures/plans/valid/kettle.md");

const SEQUENCES: [Sequence; 37] = [
    sequence!("happy-path"),
    sequence!("amendment"),
    sequence!("fork"),
    sequence!("fork-descendant"),
    sequence!("fork-two-roots"),
    sequence!("superseded-observation"),
    sequence!("goal-changed"),
    sequence!("goal-ref-not-a-goal"),
    sequence!("evidence-refusals"),
    sequence!("action-hash-mismatch"),
    sequence!("action-failed"),
    sequence!("action-dirty"),
    sequence!("mixed-artifacts"),
    sequence!("wrong-assignee-report"),
    sequence!("superseded-disposition"),
    sequence!("same-action-two-commits"),
    sequence!("same-action-two-commits-reversed"),
    sequence!("plan-unavailable-before-bindings"),
    sequence!("action-dirty-after"),
    sequence!("action-no-host-result"),
    sequence!("action-not-compiled"),
    sequence!("action-wrong-echo-signer"),
    sequence!("action-wrong-step"),
    sequence!("canonical-exclusions"),
    sequence!("criterion-unassigned-report"),
    sequence!("plan-unreadable-after-bindings"),
    sequence!("projection-empty"),
    sequence!("relay-self-key-absent"),
    sequence!("report-absent-assignment"),
    sequence!("review-unresolved-and-unanswered"),
    sequence!("plan-drift-none"),
    sequence!("plan-drift-drifted"),
    sequence!("plan-drift-superseded"),
    sequence!("plan-drift-unknown"),
    sequence!("plan-drift-on-completed"),
    sequence!("ref-observation-rebound"),
    sequence!("ref-observation-wrong-ref"),
];

impl Sequence {
    fn events(&self) -> Vec<ProjectWorkEvent> {
        serde_json::from_str(self.events).expect("sequence events")
    }

    /// Build the fold's input from the fixture, substituting the one plan
    /// fixture every sequence names. Nothing is invented here: the authority,
    /// the goal set, the compiled action definitions, the evidence facts and
    /// the ref states are the fixture's own, deserialized into the fold's
    /// types — the implementer must not write its own oracle.
    fn inputs(&self) -> WorkFoldInputs {
        let raw: serde_json::Value = serde_json::from_str(self.inputs).expect("sequence inputs");
        let mut plan_blobs = BTreeMap::new();
        for (key, value) in raw["planBlobs"].as_object().expect("planBlobs") {
            let path = value.as_str().expect("a plan blob names a fixture path");
            assert_eq!(
                path, "../../plans/valid/kettle.md",
                "{}: this harness knows one plan fixture; add the new one here",
                self.name
            );
            plan_blobs.insert(key.clone(), KETTLE_PLAN.to_owned());
        }
        let optional = |key: &str| -> serde_json::Value {
            raw.get(key)
                .filter(|value| !value.is_null())
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}))
        };
        WorkFoldInputs {
            events: self.events(),
            relay_self_key: raw["relaySelfKey"].as_str().map(str::to_owned),
            authority: serde_json::from_value(raw["authority"].clone()).expect("authority"),
            current_goal_ref: raw["currentGoalRef"].as_str().map(str::to_owned),
            goal_events: raw["goalEvents"]
                .as_array()
                .map(|ids| {
                    ids.iter()
                        .map(|id| id.as_str().expect("goal id").to_owned())
                        .collect()
                })
                .unwrap_or_default(),
            plan_blobs,
            action_definitions: serde_json::from_value(optional("actionDefinitions"))
                .expect("actionDefinitions"),
            // The canonical 44244 projection is an input the contract states
            // (A5): the fold judges canonicity, the assembler establishes it.
            team_projection: serde_json::from_value(optional("teamProjection"))
                .expect("teamProjection"),
            evidence: serde_json::from_value(optional("evidence")).expect("evidence"),
            ref_states: serde_json::from_value(raw["refStates"].clone()).expect("refStates"),
            session_ref: None,
            project_ref: None,
        }
    }

    fn expected(&self) -> serde_json::Value {
        serde_json::from_str(self.expected).expect("expected fold")
    }
}

fn as_json(projection: &WorkProjection) -> serde_json::Value {
    serde_json::to_value(projection).expect("the projection serializes")
}

#[test]
fn every_sequence_folds_to_exactly_its_expected_output() {
    for sequence in &SEQUENCES {
        let actual = as_json(&fold_work(&sequence.inputs()));
        assert_eq!(
            actual,
            sequence.expected(),
            "{}: the fold does not produce the contract's expected output\n{}",
            sequence.name,
            serde_json::to_string_pretty(&actual).unwrap_or_default()
        );
    }
}

/// Every permutation of a short sequence, or a deterministic sample of a long
/// one. No clock and no RNG: the sample is generated by a fixed linear
/// congruential shuffle so a failure reproduces exactly.
fn permutations(count: usize) -> Vec<Vec<usize>> {
    if count <= 6 {
        let mut all = Vec::new();
        let mut current: Vec<usize> = (0..count).collect();
        permute(&mut current, 0, &mut all);
        return all;
    }
    let mut samples = Vec::with_capacity(512);
    let mut state: u64 = 0x2026_0920;
    for _ in 0..512 {
        let mut order: Vec<usize> = (0..count).collect();
        for index in (1..count).rev() {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let pick = (state >> 33) as usize % (index + 1);
            order.swap(index, pick);
        }
        samples.push(order);
    }
    samples
}

fn permute(current: &mut Vec<usize>, start: usize, all: &mut Vec<Vec<usize>>) {
    if start == current.len() {
        all.push(current.clone());
        return;
    }
    for index in start..current.len() {
        current.swap(start, index);
        permute(current, start + 1, all);
        current.swap(start, index);
    }
}

#[test]
fn the_fold_is_order_independent_under_every_permutation() {
    for sequence in &SEQUENCES {
        let base = sequence.inputs();
        let expected = as_json(&fold_work(&base));
        for order in permutations(base.events.len()) {
            let mut permuted = base.clone();
            permuted.events = order
                .iter()
                .map(|index| base.events[*index].clone())
                .collect();
            assert_eq!(
                as_json(&fold_work(&permuted)),
                expected,
                "{}: order {order:?} folds differently",
                sequence.name
            );
        }
    }
}

#[test]
fn duplicate_deliveries_are_harmless() {
    for sequence in &SEQUENCES {
        let base = sequence.inputs();
        let expected = as_json(&fold_work(&base));
        let mut doubled = base.clone();
        doubled.events.extend(base.events.iter().cloned());
        doubled.events.extend(base.events.iter().rev().cloned());
        assert_eq!(
            as_json(&fold_work(&doubled)),
            expected,
            "{}: a repeated delivery changed the projection",
            sequence.name
        );
    }
}

#[test]
fn a_late_green_for_p_never_covers_p2() {
    // The amendment sequence is exactly this case, and it must hold under
    // every arrival order: the evidence binding naming P arrives *after* P2.
    let sequence = &SEQUENCES[1];
    let projection = fold_work(&sequence.inputs());
    let head = projection
        .declarations
        .iter()
        .find(|declaration| declaration.state == WorkDeclarationState::Head)
        .expect("the amendment has a head");
    assert!(!head.coverage_complete);
    for criterion in &head.criteria {
        assert_ne!(
            criterion.status,
            WorkCriterionStatus::Covered,
            "{} must not be covered at P2 by evidence bound to P",
            criterion.criterion_id
        );
    }
    assert_eq!(
        head.criteria
            .iter()
            .filter(|criterion| criterion.status == WorkCriterionStatus::Stale)
            .count(),
        2
    );
}

#[test]
fn a_record_signed_by_a_non_lead_is_excluded_by_name_and_breaks_nothing() {
    let projection = fold_work(&SEQUENCES[0].inputs());
    assert_eq!(projection.excluded.len(), 1);
    assert_eq!(projection.excluded[0].code, "signer_not_may_lead");
    // The rest of the session still folds to complete coverage.
    assert!(projection.declarations[0].coverage_complete);
}

#[test]
fn an_unreadable_plan_blob_reads_unknown_and_never_open() {
    let mut inputs = SEQUENCES[0].inputs();
    inputs.plan_blobs.clear();
    let projection = fold_work(&inputs);
    let declaration = &projection.declarations[0];
    assert!(!declaration.plan_resolved);
    assert!(!declaration.coverage_complete);
    assert!(!declaration.criteria.is_empty());
    for criterion in &declaration.criteria {
        assert_eq!(criterion.status, WorkCriterionStatus::Unknown);
        assert!(criterion.reason.is_some());
    }
}

#[test]
fn an_unestablished_evidence_event_reads_unknown() {
    let mut inputs = SEQUENCES[0].inputs();
    inputs.evidence.clear();
    let projection = fold_work(&inputs);
    let declaration = &projection.declarations[0];
    assert!(!declaration.coverage_complete);
    assert!(declaration.criteria.iter().any(|criterion| {
        criterion.status == WorkCriterionStatus::Unknown
            && criterion.reason_code == Some(WorkReasonCode::EvidenceUnavailable)
    }));
}

#[test]
fn a_ref_observation_the_relay_did_not_sign_is_not_an_observation() {
    let mut inputs = SEQUENCES[0].inputs();
    for state in &mut inputs.ref_states {
        state.pubkey = "b0".repeat(32);
    }
    let projection = fold_work(&inputs);
    let delivered = projection.declarations[0]
        .criteria
        .iter()
        .find(|criterion| criterion.criterion_id == "delivered-main")
        .expect("delivered-main");
    assert_eq!(delivered.status, WorkCriterionStatus::Unknown);
    assert!(!projection.declarations[0].coverage_complete);
}

#[test]
fn the_fork_refuses_coverage_for_both_heads_and_names_them() {
    let projection = fold_work(&SEQUENCES[2].inputs());
    assert_eq!(projection.conflicts.len(), 1);
    assert_eq!(projection.conflicts[0].heads.len(), 2);
    for declaration in &projection.declarations {
        assert!(!declaration.coverage_complete);
    }
    // Bindings under a conflicted head are retained on the wire and simply
    // not projected: nothing is deleted.
    assert!(projection
        .declarations
        .iter()
        .all(|declaration| declaration.criteria.is_empty()));
    assert!(projection.excluded.is_empty());
}

// ── Compatibility (contract § (f)) ───────────────────────────────────────

fn team_transaction_events() -> (Vec<nostr::Event>, CodingSessionTeamFoldContext, nostr::Keys) {
    use crate::coding_session_team_transaction::{
        CodingSessionTeamAssignment, CodingSessionTeamTransactionBody,
        CodingSessionTeamTransactionPayload, CodingSessionTeamTransactionType,
        CODING_SESSION_TEAM_TRANSACTION_SCHEMA,
    };
    use nostr::{EventBuilder, Keys, Kind, Tag};

    let keys = Keys::generate();
    let channel = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";
    let session = "11111111-2222-4333-8444-555555555555";
    let genesis = "9e".repeat(32);
    let payload = CodingSessionTeamTransactionPayload {
        schema: CODING_SESSION_TEAM_TRANSACTION_SCHEMA.to_owned(),
        session_ref: session.to_owned(),
        genesis_ref: genesis.clone(),
        transaction_type: CodingSessionTeamTransactionType::Assignment,
        supersedes: None,
        delivery_command_id: None,
        body: CodingSessionTeamTransactionBody::Assignment(CodingSessionTeamAssignment {
            assignee_actor: keys.public_key().to_hex(),
            assignee_role: "builder".to_owned(),
            objective: "Build the kettle CLI".to_owned(),
            brief: "Build it.".to_owned(),
            branch: None,
            base_sha: None,
            file_ownership: vec!["kettle/".to_owned()],
            acceptance_steps: vec!["python3 -m unittest".to_owned()],
        }),
    };
    let content = serde_json::to_string(&payload).expect("payload");
    let tags = [
        ["h", channel],
        ["d", session],
        ["cstx-v", CODING_SESSION_TEAM_TRANSACTION_SCHEMA],
        ["cstx-genesis", genesis.as_str()],
        ["cstx-type", "assignment"],
    ]
    .into_iter()
    .map(|parts| Tag::parse(parts).expect("tag"))
    .collect::<Vec<_>>();
    let event = EventBuilder::new(
        Kind::Custom(KIND_CODING_SESSION_TEAM_TRANSACTION as u16),
        content,
    )
    .tags(tags)
    .sign_with_keys(&keys)
    .expect("sign");
    let context = CodingSessionTeamFoldContext {
        channel_ref: channel.to_owned(),
        session_ref: session.to_owned(),
        genesis_ref: genesis,
        founder_pubkey: keys.public_key().to_hex(),
        active_seats: vec![CodingSessionTeamActiveSeat {
            actor_pubkey: keys.public_key().to_hex(),
            role: "lead".to_owned(),
        }],
        active_grants: Vec::new(),
        verifier_required: false,
    };
    (vec![event], context, keys)
}

#[test]
fn the_44244_fold_is_unaffected_by_44249_events_in_the_same_channel() {
    let (events, context, _keys) = team_transaction_events();
    let without = fold_coding_session_team_transactions(&events, &context).expect("folds");

    // The mixed stream as a reader actually sees it: a REQ names its kinds,
    // so the 44249 records never enter this fold. Selecting by kind is the
    // whole of the compatibility claim, and it is checked here rather than
    // asserted in prose.
    let work_events = SEQUENCES[0].events();
    let mut mixed: Vec<nostr::Event> = events.clone();
    assert!(!work_events.is_empty());
    let selected: Vec<nostr::Event> = mixed
        .drain(..)
        .filter(|event| crate::kind::event_kind_u32(event) == KIND_CODING_SESSION_TEAM_TRANSACTION)
        .collect();
    let with = fold_coding_session_team_transactions(&selected, &context).expect("folds");
    assert_eq!(
        format!("{without:?}"),
        format!("{with:?}"),
        "the same 44244 events fold byte-identically whether or not 44249 records exist"
    );

    // And the 44244 reader refuses a 44249 event outright rather than
    // misreading it — the finding-13 cliff, which is exactly why work
    // records are a sibling kind rather than three more 44244 subtypes.
    let work = &work_events[0];
    assert_eq!(work.kind, KIND_PROJECT_WORK_RECORD);
    assert!(
        crate::coding_session_team_transaction::decode_recorded_coding_session_team_transaction(
            &work.content
        )
        .is_err()
    );
}

#[test]
fn the_work_kind_is_registered_but_is_not_project_a_scoped() {
    assert!(ALL_KINDS.contains(&KIND_PROJECT_WORK_RECORD));
    // The `a` tag is a selector; `h` is the gate. Claiming project-membership
    // gating this kind does not have would be a lie about what is enforced.
    assert!(!is_project_a_scoped_kind(KIND_PROJECT_WORK_RECORD));
}

#[test]
fn a_foreign_kind_in_the_stream_is_ignored_not_excluded() {
    let mut inputs = SEQUENCES[0].inputs();
    let expected = as_json(&fold_work(&inputs));
    let mut stray = inputs.events[0].clone();
    stray.id = "fe".repeat(32);
    stray.kind = 40002;
    inputs.events.push(stray);
    assert_eq!(
        as_json(&fold_work(&inputs)),
        expected,
        "a kind this fold does not own is not its business"
    );
}

#[test]
fn authority_is_the_44244_predicate_including_steer_grantees() {
    // `bind6`'s signer holds a live may_steer grant and no lead seat, and its
    // record is admitted; `bind9`'s holds neither, and its record is not.
    // The predicate is the 44244 fold's own, not a second copy.
    let inputs = SEQUENCES[0].inputs();
    let grantee = &inputs.authority.active_grants[0].actor_pubkey;
    assert!(inputs.authority.may_lead(grantee));
    assert!(inputs.authority.may_lead(&inputs.authority.founder_pubkey));
    assert!(!inputs.authority.may_lead(&"b0".repeat(32)));

    let mut without_grant = inputs.clone();
    without_grant.authority.active_grants.clear();
    let projection = fold_work(&without_grant);
    assert!(
        projection.excluded.len() > 1,
        "withdrawing the steer grant must exclude the records it admitted"
    );
}

#[test]
fn a_decision_id_in_goal_ref_is_refused_by_the_fold_not_the_envelope() {
    let sequence = &SEQUENCES[7];
    assert_eq!(sequence.name, "goal-ref-not-a-goal");
    let inputs = sequence.inputs();
    // The envelope accepts it: both are 64-hex and one event cannot tell them
    // apart. The fold is the only layer holding the goal set.
    assert!(validate_project_work_envelope(&inputs.events[0]).is_ok());
    let projection = fold_work(&inputs);
    assert!(projection.declarations.is_empty());
    assert_eq!(projection.excluded[0].code, "goal_ref_not_a_goal");
    assert!(projection.excluded[0].message.contains("decisionRef"));

    // With no goal set established, the fold judges nobody on that question
    // rather than guessing.
    let mut unestablished = inputs.clone();
    unestablished.goal_events.clear();
    assert_eq!(fold_work(&unestablished).declarations.len(), 1);
}

#[test]
fn coverage_is_about_one_revision_not_a_per_criterion_scoreboard() {
    let sequence = &SEQUENCES[12];
    assert_eq!(sequence.name, "mixed-artifacts");
    let projection = fold_work(&sequence.inputs());
    let declaration = &projection.declarations[0];
    assert!(declaration
        .criteria
        .iter()
        .all(|criterion| criterion.status == WorkCriterionStatus::Covered));
    assert!(
        !declaration.coverage_complete,
        "five criteria green at three commits verify nothing at the delivered commit"
    );
    assert_eq!(
        declaration.coverage_reason_code,
        Some(WorkCoverageReasonCode::MixedArtifacts)
    );
    assert_eq!(declaration.artifact_commits.len(), 3);
}

#[test]
fn evidence_that_resolved_and_failed_is_open_with_its_reason_named() {
    for (index, expected) in [
        (8, WorkReasonCode::WrongSigner),
        (9, WorkReasonCode::WrongRunOrHash),
        (10, WorkReasonCode::ActionFailed),
        (11, WorkReasonCode::DirtyRevision),
    ] {
        let projection = fold_work(&SEQUENCES[index].inputs());
        let declaration = &projection.declarations[0];
        assert!(
            declaration.criteria.iter().any(|criterion| {
                criterion.reason_code == Some(expected)
                    && criterion.status == WorkCriterionStatus::Open
                    && criterion.reason.is_some()
            }),
            "{}: expected a criterion open with {}",
            SEQUENCES[index].name,
            expected.as_str()
        );
        // A failed claim is never coverage, and never silently empty.
        assert!(!declaration.coverage_complete);
        assert!(declaration.candidate_artifact.is_none());
    }
}

#[test]
fn a_fork_is_defined_over_maximal_declarations_including_two_roots() {
    // P forks to A and B, then A2 supersedes only A: A2 and B share no
    // immediate predecessor but both are maximal, so the work is still in
    // conflict. A same-predecessor rule would clear it while B stands.
    let descendant = fold_work(&SEQUENCES[3].inputs());
    assert_eq!(descendant.conflicts.len(), 1);
    assert_eq!(descendant.conflicts[0].heads.len(), 2);

    // Two declarations of one workId with empty `supersedes` are two heads:
    // an empty list is not a claim to be first.
    let roots = fold_work(&SEQUENCES[4].inputs());
    assert_eq!(roots.conflicts.len(), 1);
    assert_eq!(roots.conflicts[0].heads.len(), 2);
    assert!(roots
        .declarations
        .iter()
        .all(|declaration| declaration.state == WorkDeclarationState::Conflict));
}

/// Every reason string the fold prints comes from the README's table.
///
/// A6 § "The exact reason strings" fixes one wording per code so two
/// implementations say the same thing about the same fact. This loads that
/// table out of the contract itself and checks each code's template against
/// the strings the sequences actually produced: the table and the fold cannot
/// drift apart without this failing.
#[test]
fn every_reason_string_matches_the_contracts_table() {
    const README: &str = include_str!("../../../conformance/project-work/README.md");
    // One *branch*, one wording: three columns, and a code with four ways to
    // fail has four rows (A7.4).
    let mut table: Vec<(String, String, String)> = Vec::new();
    let mut in_table = false;
    for line in README.lines() {
        if line.starts_with("| `reasonCode` | branch | `reason` |") {
            in_table = true;
            continue;
        }
        if in_table {
            if !line.starts_with("| `") {
                if line.starts_with('|') {
                    continue; // the header rule
                }
                break;
            }
            let cells: Vec<&str> = line.trim_matches('|').split(" | ").collect();
            assert_eq!(cells.len(), 3, "a template row has three columns: {line}");
            table.push((
                cells[0].trim().trim_matches('`').to_owned(),
                cells[1].trim().to_owned(),
                cells[2].trim().trim_matches('`').to_owned(),
            ));
        }
    }
    assert_eq!(
        table.len(),
        31,
        "the contract's reason table should hold 31 branch rows, found {}",
        table.len()
    );

    // A template becomes an exact matcher: every placeholder carries the
    // shape the contract documents for it, so two branches of one code can
    // never both claim the same sentence.
    let matches_template = |template: &str, produced: &str| -> bool {
        let mut rest = produced;
        let mut parts = template.split(['<', '>']);
        let mut literal = true;
        loop {
            let Some(part) = parts.next() else {
                break rest.is_empty();
            };
            if literal {
                if !rest.starts_with(part) {
                    break false;
                }
                rest = &rest[part.len()..];
            } else {
                // Placeholders: `<id>` is 8 hex and an ellipsis, `<sha>` 12,
                // `<code>` a signed integer, the rest a slug or a name.
                let taken = match part {
                    "id" => rest
                        .starts_with(|c: char| c.is_ascii_hexdigit())
                        .then(|| rest.char_indices().nth(8).map(|(at, _)| at))
                        .flatten()
                        .filter(|at| rest[*at..].starts_with('\u{2026}'))
                        .map(|at| at + '\u{2026}'.len_utf8()),
                    "sha" => rest
                        .starts_with(|c: char| c.is_ascii_hexdigit())
                        .then(|| rest.char_indices().nth(12).map(|(at, _)| at))
                        .flatten()
                        .filter(|at| rest[*at..].starts_with('\u{2026}'))
                        .map(|at| at + '\u{2026}'.len_utf8()),
                    "code" => Some(
                        rest.find(|c: char| !c.is_ascii_digit() && c != '-')
                            .unwrap_or(rest.len()),
                    )
                    .filter(|at| *at > 0),
                    // A slug or a repository name: everything up to the next
                    // literal the template names.
                    _ => {
                        let next = parts.clone().next().unwrap_or("");
                        if next.is_empty() {
                            Some(rest.len())
                        } else {
                            rest.find(next).filter(|at| *at > 0)
                        }
                    }
                };
                match taken {
                    Some(at) => rest = &rest[at..],
                    None => break false,
                }
            }
            literal = !literal;
        }
    };

    let mut exercised: BTreeSet<usize> = BTreeSet::new();
    for sequence in &SEQUENCES {
        let projection = fold_work(&sequence.inputs());
        for declaration in &projection.declarations {
            for criterion in &declaration.criteria {
                let (Some(code), Some(reason)) =
                    (criterion.reason_code, criterion.reason.as_deref())
                else {
                    continue;
                };
                let row = table.iter().position(|(row_code, _, template)| {
                    row_code == code.as_str() && matches_template(template, reason)
                });
                let Some(row) = row else {
                    panic!(
                        "{}: {} printed {reason:?}, which follows no {} row of the contract's \
                         table",
                        sequence.name,
                        code.as_str(),
                        code.as_str()
                    )
                };
                exercised.insert(row);
            }
        }
    }

    // Every documented sentence is produced by something. A row nothing
    // exercises is a promise, not a contract — there is no allowance here.
    let unexercised: Vec<&str> = table
        .iter()
        .enumerate()
        .filter(|(index, _)| !exercised.contains(index))
        .map(|(_, (code, branch, _))| format!("{code}/{branch}"))
        .collect::<Vec<_>>()
        .iter()
        .map(String::as_str)
        .map(|owned| Box::leak(owned.to_owned().into_boxed_str()) as &str)
        .collect();
    assert!(
        unexercised.is_empty(),
        "no sequence produces these documented sentences: {unexercised:?}"
    );
}
