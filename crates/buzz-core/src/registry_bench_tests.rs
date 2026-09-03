//! Tests for [`crate::registry_bench`].
//!
//! The load-bearing one is [`the_same_artifacts_score_the_same_twice`]: the
//! whole claim a measured row makes is that the number came out of the run and
//! not out of the scorer's mood, and a scorer that is a pure function of
//! artifacts is the only way to say so.

use std::collections::BTreeMap;

use serde_json::json;

use super::*;

fn criterion(id: &str, traits: &[&str], weight: u32, check: Check) -> Criterion {
    Criterion {
        id: id.to_owned(),
        traits: traits.iter().map(|name| (*name).to_owned()).collect(),
        weight,
        check,
    }
}

fn artifacts() -> RunArtifacts {
    RunArtifacts {
        exit_code: Some(0),
        stdout: "planted defect: off-by-one in take_while\n".to_owned(),
        files: BTreeMap::from([
            (
                "report.json".to_owned(),
                json!({ "defects": [{ "id": "off-by-one" }] }).to_string(),
            ),
            ("notes.txt".to_owned(), "FOUND the boundary bug".to_owned()),
        ]),
        probes: BTreeMap::from([("script".to_owned(), true)]),
        timed_out: false,
        duration_ms: 4_200,
        timeout_secs: BENCH_TASK_TIMEOUT_SECS,
    }
}

// ── the closed check set, each kind scored to an asserted decimal ────────────

#[test]
fn every_check_kind_scores_a_fixture_to_the_decimal_it_should() {
    let cases: Vec<(Check, bool)> = vec![
        (Check::ExitCode(ExitCodeCheck { equals: 0 }), true),
        (Check::ExitCode(ExitCodeCheck { equals: 1 }), false),
        (
            Check::StdoutMatches(StdoutMatchesCheck {
                pattern: r"off-by-one".to_owned(),
            }),
            true,
        ),
        (
            Check::StdoutMatches(StdoutMatchesCheck {
                pattern: r"^nothing$".to_owned(),
            }),
            false,
        ),
        (
            Check::FileContains(FileContainsCheck {
                path: "notes.txt".to_owned(),
                text: "FOUND".to_owned(),
            }),
            true,
        ),
        (
            Check::FileContains(FileContainsCheck {
                path: "missing.txt".to_owned(),
                text: "FOUND".to_owned(),
            }),
            false,
        ),
        (
            Check::FileAbsent(FileAbsentCheck {
                path: "false-positive.txt".to_owned(),
            }),
            true,
        ),
        (
            Check::FileAbsent(FileAbsentCheck {
                path: "notes.txt".to_owned(),
            }),
            false,
        ),
        (
            Check::JsonPathEquals(JsonPathEqualsCheck {
                path: "report.json".to_owned(),
                pointer: "/defects/0/id".to_owned(),
                equals: json!("off-by-one"),
            }),
            true,
        ),
        (
            Check::JsonPathEquals(JsonPathEqualsCheck {
                path: "report.json".to_owned(),
                pointer: "/defects/1/id".to_owned(),
                equals: json!("off-by-one"),
            }),
            false,
        ),
    ];

    for (check, expected) in cases {
        let kind = check.kind().to_owned();
        let score = score_run(&[criterion("only", &["reasoning"], 1, check)], &artifacts());
        // One criterion, weight 1: raw is 1.0 or 0.0, so the trait is exactly
        // 5.0 or exactly 1.0. Nothing in between is reachable, which is the
        // point of asserting the decimal rather than the boolean.
        let expected_score = if expected { 5.0 } else { 1.0 };
        assert!(
            (score.traits["reasoning"] - expected_score).abs() < f64::EPSILON,
            "{kind}: expected {expected_score}, got {}",
            score.traits["reasoning"]
        );
    }
}

/// The two checks that need a process are answered by the harness, and an
/// unanswered one **fails by name** — it never passes on missing evidence.
#[test]
fn a_probe_the_harness_did_not_answer_fails_and_says_so() {
    let answered = criterion(
        "script",
        &["verification"],
        3,
        Check::Command(CommandCheck {
            script: "check.sh".to_owned(),
        }),
    );
    let unanswered = criterion(
        "patch",
        &["verification"],
        3,
        Check::DiffApplies(DiffAppliesCheck {
            path: "fix.patch".to_owned(),
        }),
    );
    let score = score_run(&[answered, unanswered], &artifacts());
    assert_eq!(score.failed, vec!["patch".to_owned()]);
    let detail = &score
        .outcomes
        .iter()
        .find(|outcome| outcome.id == "patch")
        .expect("outcome")
        .detail;
    assert!(
        detail.contains("no answer") && detail.contains("never a pass"),
        "unexpected: {detail}"
    );
    // 3 of 6 weight passed → raw 0.5 → 1.0 + 4.0*0.5 = 3.0.
    assert!((score.traits["verification"] - 3.0).abs() < f64::EPSILON);
}

/// A pattern that does not compile is a broken rubric, not a free pass.
#[test]
fn a_pattern_that_does_not_compile_fails_the_criterion() {
    let score = score_run(
        &[criterion(
            "bad",
            &["reasoning"],
            1,
            Check::StdoutMatches(StdoutMatchesCheck {
                pattern: "([".to_owned(),
            }),
        )],
        &artifacts(),
    );
    assert_eq!(score.failed, vec!["bad".to_owned()]);
    assert!(score.outcomes[0].detail.contains("does not compile"));
}

/// A timeout is a result: every criterion fails, and the reason names the
/// budget rather than the check.
#[test]
fn a_task_past_its_budget_fails_every_criterion() {
    let mut timed_out = artifacts();
    timed_out.timed_out = true;
    let score = score_run(
        &[
            criterion(
                "a",
                &["reasoning"],
                5,
                Check::ExitCode(ExitCodeCheck { equals: 0 }),
            ),
            criterion(
                "b",
                &["verification"],
                5,
                Check::FileAbsent(FileAbsentCheck {
                    path: "nope".to_owned(),
                }),
            ),
        ],
        &timed_out,
    );
    assert_eq!(score.failed, vec!["a".to_owned(), "b".to_owned()]);
    assert!((score.traits["reasoning"] - 1.0).abs() < f64::EPSILON);
    assert!(score.outcomes[0].detail.contains("900s budget"));
}

/// A trait no criterion tags gets **no score**, never a zero.
#[test]
fn an_untagged_trait_is_absent_not_zero() {
    let score = score_run(
        &[criterion(
            "only",
            &["reasoning"],
            1,
            Check::ExitCode(ExitCodeCheck { equals: 0 }),
        )],
        &artifacts(),
    );
    assert!(score.traits.contains_key("reasoning"));
    assert!(
        !score.traits.contains_key("judgment"),
        "a trait nothing measured must not appear at all"
    );
}

/// The formula, weighted, to one asserted decimal.
#[test]
fn the_weighted_formula_is_one_plus_four_times_the_passed_weight_fraction() {
    let score = score_run(
        &[
            criterion(
                "names-the-defect",
                &["reasoning", "verification"],
                7,
                Check::StdoutMatches(StdoutMatchesCheck {
                    pattern: "off-by-one".to_owned(),
                }),
            ),
            criterion(
                "reports-no-absent-defect",
                &["verification"],
                3,
                Check::FileAbsent(FileAbsentCheck {
                    path: "notes.txt".to_owned(),
                }),
            ),
        ],
        &artifacts(),
    );
    // reasoning: 7/7 → 5.0. verification: 7/10 → 1.0 + 2.8 = 3.8.
    assert!((score.traits["reasoning"] - 5.0).abs() < f64::EPSILON);
    assert!((score.traits["verification"] - 3.8).abs() < f64::EPSILON);
    assert_eq!(score.fraction(), "7/10");
    assert!(!score.all_passed());
}

/// Two scorings of one artifact set agree, byte for byte. This is the property
/// the word "measured" rests on.
#[test]
fn the_same_artifacts_score_the_same_twice() {
    let criteria = vec![
        criterion(
            "exit",
            &["discipline"],
            2,
            Check::ExitCode(ExitCodeCheck { equals: 0 }),
        ),
        criterion(
            "names",
            &["reasoning", "verification"],
            7,
            Check::StdoutMatches(StdoutMatchesCheck {
                pattern: "off-by-one".to_owned(),
            }),
        ),
        criterion(
            "quiet",
            &["judgment"],
            4,
            Check::FileAbsent(FileAbsentCheck {
                path: "extra-defects.txt".to_owned(),
            }),
        ),
        criterion(
            "probe",
            &["verification"],
            3,
            Check::Command(CommandCheck {
                script: "check.sh".to_owned(),
            }),
        ),
    ];
    let first = score_run(&criteria, &artifacts());
    let second = score_run(&criteria, &artifacts());
    assert_eq!(first, second);
}

// ── across runs ──────────────────────────────────────────────────────────────

fn run(pairs: &[(&str, f64)]) -> BTreeMap<String, f64> {
    pairs
        .iter()
        .map(|(name, score)| ((*name).to_owned(), *score))
        .collect()
}

#[test]
fn the_row_value_is_the_median_with_n_min_and_max() {
    let runs = vec![
        run(&[("verification", 4.6)]),
        run(&[("verification", 4.2)]),
        run(&[("verification", 4.4)]),
    ];
    let aggregate = aggregate_runs(&runs);
    let measured = aggregate["verification"];
    assert!((measured.score - 4.4).abs() < f64::EPSILON);
    assert_eq!(measured.n, 3);
    assert!((measured.min - 4.2).abs() < f64::EPSILON);
    assert!((measured.max - 4.6).abs() < f64::EPSILON);
    assert!(!measured.is_unstable(), "spread 0.4 is stable");
}

#[test]
fn an_even_number_of_runs_takes_the_midpoint_of_the_two_middles() {
    let runs = vec![
        run(&[("coding", 4.0)]),
        run(&[("coding", 4.2)]),
        run(&[("coding", 4.4)]),
        run(&[("coding", 4.6)]),
    ];
    assert!((aggregate_runs(&runs)["coding"].score - 4.3).abs() < f64::EPSILON);
}

#[test]
fn a_spread_of_one_or_more_marks_the_trait_unstable() {
    let runs = vec![
        run(&[("judgment", 5.0)]),
        run(&[("judgment", 4.0)]),
        run(&[("judgment", 4.5)]),
    ];
    let aggregate = aggregate_runs(&runs);
    assert!((aggregate["judgment"].spread() - 1.0).abs() < f64::EPSILON);
    assert_eq!(unstable_traits(&aggregate), vec!["judgment".to_owned()]);
}

#[test]
fn confidence_is_derived_from_n_and_never_chosen() {
    assert_eq!(derived_confidence(2), "low");
    assert_eq!(derived_confidence(3), "medium");
    assert_eq!(derived_confidence(8), "medium");
    assert_eq!(derived_confidence(9), "high");
}

// ── the bench hash ───────────────────────────────────────────────────────────

#[test]
fn moving_bytes_between_two_files_changes_the_bench_hash() {
    let left = BTreeMap::from([
        ("bench.yaml".to_owned(), b"ab".to_vec()),
        ("task/rubric.yaml".to_owned(), b"cd".to_vec()),
    ]);
    let right = BTreeMap::from([
        ("bench.yaml".to_owned(), b"abc".to_vec()),
        ("task/rubric.yaml".to_owned(), b"d".to_vec()),
    ]);
    assert_ne!(bench_hash(&left), bench_hash(&right));
    assert_eq!(bench_hash(&left), bench_hash(&left.clone()));
}

// ── parsing ──────────────────────────────────────────────────────────────────

#[test]
fn a_rubric_with_an_unknown_check_kind_is_refused_by_name() {
    let error = parse_rubric(
        "criteria:\n  - id: a\n    traits: [reasoning]\n    weight: 3\n    check: !llmJudge\n      \
         prompt: is it good\n",
    )
    .expect_err("an unknown check kind must be refused");
    let text = error.to_string();
    assert!(text.contains("llmJudge"), "unexpected: {text}");
}

#[test]
fn a_criterion_that_tags_no_trait_is_refused() {
    let error = parse_rubric(
        "criteria:\n  - id: a\n    traits: []\n    weight: 3\n    check: !exitCode\n      \
         equals: 0\n",
    )
    .expect_err("must be refused");
    assert!(error.to_string().contains("tags no trait"));
}

#[test]
fn a_weight_outside_the_range_is_refused_by_name() {
    let error = parse_rubric(
        "criteria:\n  - id: a\n    traits: [reasoning]\n    weight: 11\n    check: !exitCode\n      \
         equals: 0\n",
    )
    .expect_err("must be refused");
    assert!(error.to_string().contains("weight 11"));
}

#[test]
fn a_stub_bench_parses_and_says_it_is_a_stub() {
    let set = parse_bench_set("role: lead\nbenchVersion: 1\ntasks: []\n").expect("parse");
    assert!(set.is_stub());
}

// ── the gate name is a round trip ────────────────────────────────────────────

#[test]
fn a_summary_names_the_criteria_that_failed_and_says_none_when_they_did_not() {
    assert_eq!(
        parse_bench_summary("7/10 · failed: names-the-defect, exited-clean"),
        vec!["names-the-defect".to_owned(), "exited-clean".to_owned()]
    );
    assert!(parse_bench_summary("10/10 · failed: none").is_empty());
    assert!(parse_bench_summary("something else entirely").is_empty());
}

#[test]
fn the_gate_name_round_trips_and_refuses_anything_else() {
    let gate = bench_gate_name("verifier", "planted-defect", 2);
    assert_eq!(gate, "registry-bench/verifier/planted-defect#2");
    assert_eq!(
        parse_bench_gate(&gate).expect("parse"),
        ("verifier".to_owned(), "planted-defect".to_owned(), 2)
    );
    assert!(parse_bench_gate("clippy").is_err());
    assert!(parse_bench_gate("registry-bench/verifier/a").is_err());
    assert_eq!(
        bench_finding_id(1, "verifier", "planted-defect", "names-the-defect"),
        "bench:1:verifier:planted-defect:names-the-defect"
    );
}

// ── what `propose` refuses ───────────────────────────────────────────────────

fn gate_row(signer: &str, role: &str, task: &str, run: u32) -> BenchGateRow {
    BenchGateRow {
        event_id: format!("{role}-{task}-{run}"),
        signer: signer.to_owned(),
        gate: bench_gate_name(role, task, run),
        role: role.to_owned(),
        task_id: task.to_owned(),
        run,
        outcome: "passed".to_owned(),
        summary: Some("10/10 · failed: none".to_owned()),
    }
}

fn measured(score: f64, n: u32, min: f64, max: f64) -> MeasuredTrait {
    MeasuredTrait { score, n, min, max }
}

fn verifier_minimums() -> BTreeMap<String, f64> {
    BTreeMap::from([
        ("reasoning".to_owned(), 4.5),
        ("judgment".to_owned(), 4.5),
        ("verification".to_owned(), 4.7),
    ])
}

#[test]
fn a_clean_proposal_is_refused_for_nothing() {
    let rows: Vec<BenchGateRow> = (0..3)
        .map(|run| gate_row("aa", "verifier", "planted-defect", run))
        .collect();
    let traits = BTreeMap::from([
        ("reasoning".to_owned(), measured(4.6, 3, 4.4, 4.8)),
        ("judgment".to_owned(), measured(4.5, 3, 4.4, 4.6)),
        ("verification".to_owned(), measured(4.8, 3, 4.6, 5.0)),
    ]);
    let tasks = vec!["planted-defect".to_owned()];
    let refusals = proposal_refusals(&ProposalInput {
        role: "verifier",
        bench_version: 1,
        tree_bench_version: 1,
        bench_hash: "abc",
        tree_bench_hash: "abc",
        repeat: 3,
        tasks: &tasks,
        gate_rows: &rows,
        minimums: &verifier_minimums(),
        traits: &traits,
    });
    assert!(refusals.is_empty(), "{refusals:?}");
}

#[test]
fn propose_names_each_reason_it_refuses() {
    let mut rows: Vec<BenchGateRow> = (0..2)
        .map(|run| gate_row("aa", "verifier", "planted-defect", run))
        .collect();
    rows.push(gate_row("bb", "verifier", "planted-defect", 2));
    // `verification` measured nothing; `judgment` is unstable.
    let traits = BTreeMap::from([
        ("reasoning".to_owned(), measured(4.6, 3, 4.4, 4.8)),
        ("judgment".to_owned(), measured(4.5, 3, 4.0, 5.0)),
    ]);
    let tasks = vec!["planted-defect".to_owned(), "second".to_owned()];
    let refusals = proposal_refusals(&ProposalInput {
        role: "verifier",
        bench_version: 1,
        tree_bench_version: 1,
        bench_hash: "abcdef012345",
        tree_bench_hash: "999999999999",
        repeat: 3,
        tasks: &tasks,
        gate_rows: &rows,
        minimums: &verifier_minimums(),
        traits: &traits,
    });
    let joined = refusals.join(" | ");
    assert!(joined.contains("changed under the measurement"), "{joined}");
    assert!(joined.contains("2 signing keys"), "{joined}");
    assert!(joined.contains("task second has 0 run(s)"), "{joined}");
    assert!(
        joined.contains("reads verification and this bench evidences nothing"),
        "{joined}"
    );
    assert!(joined.contains("UNSTABLE on judgment"), "{joined}");
}

#[test]
fn a_stub_role_is_refused_because_nothing_measured_it() {
    let refusals = proposal_refusals(&ProposalInput {
        role: "poker",
        bench_version: 1,
        tree_bench_version: 1,
        bench_hash: "abc",
        tree_bench_hash: "abc",
        repeat: 3,
        tasks: &[],
        gate_rows: &[],
        minimums: &BTreeMap::new(),
        traits: &BTreeMap::new(),
    });
    assert_eq!(refusals.len(), 1);
    assert!(refusals[0].contains("no tasks"));
}

/// The addendum's ratified minimums, reported beside the measurement — a
/// measured score below a ratified bar is **not** a pass, and `None` never is.
#[test]
fn the_minimum_table_reports_the_bar_and_the_number_beside_it() {
    let traits = BTreeMap::from([
        ("reasoning".to_owned(), measured(4.6, 3, 4.4, 4.8)),
        ("judgment".to_owned(), measured(4.2, 3, 4.1, 4.3)),
    ]);
    // Trait order is the gate's own (BTreeMap): judgment, reasoning, verification.
    let table = compare_to_minimums(&verifier_minimums(), &traits);
    assert_eq!(table.len(), 3);

    assert_eq!(table[0].trait_name, "judgment");
    assert_eq!(table[0].measured, Some(4.2));
    assert!(
        !table[0].clears(),
        "4.2 does not clear the ratified 4.5 — the bar is the bar"
    );

    assert_eq!(table[1].trait_name, "reasoning");
    assert!(table[1].clears());

    assert_eq!(table[2].trait_name, "verification");
    assert_eq!(table[2].measured, None);
    assert!(
        !table[2].clears(),
        "a trait the bench did not measure never clears its bar"
    );
}

#[test]
fn a_measured_block_derives_its_confidence_from_its_thinnest_trait() {
    let block = MeasuredBlock {
        role: "verifier".to_owned(),
        bench_version: 1,
        bench_hash: "abc".to_owned(),
        measured_at: "2026-09-02".to_owned(),
        measured_by: "a".repeat(64),
        runs: vec!["e1".to_owned()],
        traits: BTreeMap::from([
            ("reasoning".to_owned(), measured(4.6, 12, 4.4, 4.8)),
            ("verification".to_owned(), measured(4.8, 3, 4.6, 5.0)),
        ]),
    };
    assert_eq!(block.samples(), 3);
    assert_eq!(block.confidence(), "medium");
    assert_eq!(
        block.measured_trait_names(),
        vec!["reasoning".to_owned(), "verification".to_owned()]
    );
}

// ── fix round 1 ──────────────────────────────────────────────────────────────

/// F9 — the finding names the budget that was actually applied, not the
/// constant. A run under `--task-timeout 60` used to publish a finding blaming
/// a 900 s budget that was never in force.
#[test]
fn a_timeout_detail_names_the_budget_that_was_applied() {
    let mut artifacts = artifacts();
    artifacts.timed_out = true;
    artifacts.timeout_secs = 60;
    let score = score_run(
        &[criterion(
            "a",
            &["reasoning"],
            1,
            Check::ExitCode(ExitCodeCheck { equals: 0 }),
        )],
        &artifacts,
    );
    assert!(
        score.outcomes[0].detail.contains("60s budget"),
        "unexpected: {}",
        score.outcomes[0].detail
    );
    assert!(!score.outcomes[0].detail.contains("900s"));
}

/// F2 — a row that cannot say what failed is not evidence. It used to fold as
/// a perfect run, so three `observe gate` calls with no `--summary` minted a
/// row clearing every ratified minimum.
#[test]
fn a_gate_row_with_no_parseable_summary_is_refused_by_id() {
    let mut row = gate_row("aa", "verifier", "planted-defect", 0);
    row.summary = None;
    assert_eq!(row.failed_criteria(), None);
    let refusals = gate_row_refusals(&[row]);
    assert_eq!(
        refusals,
        vec!["gate row verifier-planted-defect-0 has no parseable summary".to_owned()]
    );

    let mut unparseable = gate_row("aa", "verifier", "planted-defect", 0);
    unparseable.summary = Some("looked fine to me".to_owned());
    assert_eq!(unparseable.failed_criteria(), None);
    assert!(gate_row_refusals(&[unparseable])[0].contains("no parseable summary"));
}

/// And `failed: none` is still a perfectly good answer — the two cases are
/// different facts and stay different.
#[test]
fn a_summary_saying_failed_none_is_evidence_and_is_not_refused() {
    let row = gate_row("aa", "verifier", "planted-defect", 0);
    assert_eq!(row.failed_criteria(), Some(Vec::new()));
    assert!(gate_row_refusals(&[row]).is_empty());
}

/// F3 — the signed `outcome` word is consulted, and two fields on one row are
/// never allowed to contradict each other in silence. Neither field wins: the
/// row is refused by name rather than coerced into agreement with one of them
/// (Brian's ruling, fix round 1).
#[test]
fn outcome_and_summary_may_not_disagree_and_neither_field_wins() {
    let mut failed_row = gate_row("aa", "verifier", "planted-defect", 0);
    failed_row.outcome = "failed".to_owned();
    assert!(failed_row.says_failed());
    // `outcome: failed` + `failed: none` is a contradiction, refused by id.
    assert!(gate_row_refusals(&[failed_row.clone()])[0].contains("cannot say both"));

    let mut passed_row = gate_row("aa", "verifier", "planted-defect", 0);
    passed_row.summary = Some("7/10 · failed: names-the-defect".to_owned());
    assert!(gate_row_refusals(&[passed_row])[0].contains("cannot say both"));

    let mut not_run = gate_row("aa", "verifier", "planted-defect", 0);
    not_run.outcome = "not-run".to_owned();
    assert!(gate_row_refusals(&[not_run])[0].contains("which is not a run"));
}

/// F8 — two measurements on one session used to interleave silently: the fold
/// took whichever row sorted first, and `runs` listed six ids for an `n` of 3.
#[test]
fn two_rows_claiming_one_task_run_refuse_the_proposal() {
    let first = gate_row("aa", "verifier", "planted-defect", 0);
    let mut second = gate_row("aa", "verifier", "planted-defect", 0);
    second.event_id = "second".to_owned();
    let refusals = gate_row_refusals(&[first, second]);
    assert_eq!(refusals.len(), 1, "{refusals:?}");
    assert!(
        refusals[0].contains("2 gate rows claim planted-defect#0"),
        "{refusals:?}"
    );
    assert!(refusals[0].contains("fresh --session-ref"), "{refusals:?}");
}

/// F12 — `bench_version` was a documented refusal input that refused nothing.
#[test]
fn a_bench_version_that_moved_under_the_measurement_is_refused() {
    let rows: Vec<BenchGateRow> = (0..3)
        .map(|run| gate_row("aa", "verifier", "planted-defect", run))
        .collect();
    let traits = BTreeMap::from([
        ("reasoning".to_owned(), measured(4.6, 3, 4.4, 4.8)),
        ("judgment".to_owned(), measured(4.5, 3, 4.4, 4.6)),
        ("verification".to_owned(), measured(4.8, 3, 4.6, 5.0)),
    ]);
    let tasks = vec!["planted-defect".to_owned()];
    let refusals = proposal_refusals(&ProposalInput {
        role: "verifier",
        bench_version: 1,
        tree_bench_version: 2,
        bench_hash: "abc",
        tree_bench_hash: "abc",
        repeat: 3,
        tasks: &tasks,
        gate_rows: &rows,
        minimums: &verifier_minimums(),
        traits: &traits,
    });
    assert_eq!(refusals.len(), 1, "{refusals:?}");
    assert!(
        refusals[0].contains("benchVersion 1 and the tree says 2"),
        "{refusals:?}"
    );
}

/// F4b — `high` is a claim about evidence somebody has actually seen. A block
/// whose run ids do not resolve is capped at `medium` however large its `n`.
#[test]
fn confidence_is_capped_at_medium_when_the_runs_do_not_resolve() {
    let block = MeasuredBlock {
        role: "verifier".to_owned(),
        bench_version: 1,
        bench_hash: "abc".to_owned(),
        measured_at: "2026-09-02".to_owned(),
        measured_by: "a".repeat(64),
        runs: vec!["deadbeef".to_owned()],
        traits: BTreeMap::from([("reasoning".to_owned(), measured(5.0, 99, 5.0, 5.0))]),
    };
    assert_eq!(block.confidence(), "high");
    assert_eq!(block.resolved_confidence(true), "high");
    assert_eq!(block.resolved_confidence(false), "medium");

    // A block that only ever earned `medium` is not promoted by resolving.
    let thin = MeasuredBlock {
        traits: BTreeMap::from([("reasoning".to_owned(), measured(5.0, 3, 5.0, 5.0))]),
        ..block
    };
    assert_eq!(thin.resolved_confidence(true), "medium");
    assert_eq!(thin.resolved_confidence(false), "medium");
}

/// F14 — a rubric that cannot report its own failure is refused before a model
/// runs, not after three runs have been paid for.
#[test]
fn a_rubric_whose_finding_id_would_not_fit_the_name_cap_is_refused_at_parse_time() {
    assert_eq!(
        MAX_BENCH_FINDING_ID_BYTES,
        crate::coding_session_observation::MAX_OBSERVATION_NAME_BYTES,
        "the bench's cap must be the wire's cap"
    );
    let long = "x".repeat(40);
    let rubric = Rubric {
        criteria: vec![criterion(
            &long,
            &["reasoning"],
            1,
            Check::ExitCode(ExitCodeCheck { equals: 0 }),
        )],
    };
    let error = validate_finding_id_lengths("verifier", "planted-defect", 1, &rubric)
        .expect_err("must be refused");
    let text = error.to_string();
    assert!(text.contains("64-byte cap"), "unexpected: {text}");
    assert!(text.contains("before a model runs"), "unexpected: {text}");

    // The shipped ids fit.
    let ok = Rubric {
        criteria: vec![criterion(
            "verdict-is-changes-requested",
            &["judgment"],
            1,
            Check::ExitCode(ExitCodeCheck { equals: 0 }),
        )],
    };
    assert!(validate_finding_id_lengths("verifier", "planted-defect", 1, &ok).is_ok());
}
