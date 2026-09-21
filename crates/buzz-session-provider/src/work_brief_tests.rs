//! Golden renders for the work brief.
//!
//! Golden rather than "contains": the brief's whole value is that a seat may
//! act on it without checking, so a change to any sentence has to be a change
//! somebody made on purpose. Each test builds its inputs by hand, so nothing
//! here needs a relay, a git repository or a host store.

use super::*;

/// The flags every command line in section (b) spells.
///
/// Kept as data so a test can assert them, and **cross-checked against the
/// real `clap` definitions** in `crates/buzz-cli/tests/work_brief_flags.rs`,
/// which holds the identical list. The provider crate cannot depend on
/// `buzz-cli`, so one renamed flag has to fail on both sides of that pair
/// rather than be caught in neither.
pub(crate) const REPORT_FLAGS: [&str; 5] = [
    "--channel",
    "--session-ref",
    "--genesis",
    "--body",
    "--wake-to",
];

/// The same, for `bee sessions verdict`.
pub(crate) const VERDICT_FLAGS: [&str; 5] = REPORT_FLAGS;

/// Flags no command line in the brief may spell.
///
/// `--assignment` does not exist at all: the assignment id travels in the
/// report body, and a brief that printed a flag for it would teach every seat
/// a command that exits 1 on its first try. `--verifies` is worse than
/// missing — `report`, `verdict` and `assign` share one `clap` struct, so it
/// *parses* on a report and changes nothing. Both are cross-checked against
/// the real definitions in `crates/buzz-cli/tests/work_brief_flags.rs`.
pub(crate) const FLAGS_THAT_DO_NOT_EXIST: [&str; 2] = ["--assignment", "--verifies"];

/// Every runnable line of a rendered brief.
fn command_lines(text: &str) -> Vec<&str> {
    text.lines()
        .map(str::trim)
        .filter(|line| line.starts_with("$BEE"))
        .collect()
}

/// A brief whose optional sections are deliberately bulky, so a test can see
/// which of them the budget gives up first rather than guess from a total.
fn over_budget_inputs(criteria: usize, decisions: usize) -> WorkBriefInputs {
    let mut inputs = builder_inputs();
    inputs.runtime.model_source = format!(
        "routed by the project's model registry; {}",
        "y".repeat(760)
    );
    inputs.decisions.decisions = (0..decisions)
        .map(|index| DecisionSummary {
            event_id: format!("{index:064}"),
            summary: "z".repeat(90),
        })
        .collect();
    inputs.contract = ContractFacts::Declared {
        declaration_ref: "d".repeat(64),
        plan_provenance: "kettle-beekeeper-agents@0123456789ab:plans/kettle.md".into(),
        criteria: (0..criteria)
            .map(|index| CriterionExcerpt {
                id: format!("criterion-{index:02}"),
                accept: "x".repeat(600),
                provenance: format!(
                    "kettle-beekeeper-agents@0123456789ab:plans/kettle.md#criterion-{index:02}"
                ),
                proof: Some(PlanProof::Review),
            })
            .collect(),
    };
    inputs
}

fn builder_inputs() -> WorkBriefInputs {
    WorkBriefInputs {
        assignment: AssignmentFacts {
            assignment_ref: "a".repeat(64),
            role: "builder".into(),
            objective: "Teach the host to hand a seat its work".into(),
            brief: "Own work_brief.rs alone. Do not touch pending_completion.rs.".into(),
            branch: Some("work/lane-209".into()),
            base_sha: Some("b".repeat(40)),
            file_ownership: vec!["crates/buzz-session-provider/src/work_brief.rs".into()],
            acceptance_steps: vec![
                "cargo test -p buzz-session-provider".into(),
                "just file-size-check".into(),
            ],
            worktree: Some("/tmp/wt-lane-209".into()),
            establishment: InputEstablishment::NotRequired,
        },
        commands: CommandFacts {
            channel: "11111111-2222-3333-4444-555555555555".into(),
            session_ref: "66666666-7777-8888-9999-000000000000".into(),
            genesis_ref: "c".repeat(64),
            assignment_ref: "a".repeat(64),
            role: "builder".into(),
            report_ref: None,
        },
        contract: ContractFacts::Declared {
            declaration_ref: "d".repeat(64),
            plan_provenance: "kettle-beekeeper-agents@0123456789ab:plans/kettle.md".into(),
            criteria: vec![
                CriterionExcerpt {
                    id: "cli-behaviour".into(),
                    accept: "`bee kettle boil` prints the temperature and exits 0.".into(),
                    provenance:
                        "kettle-beekeeper-agents@0123456789ab:plans/kettle.md#cli-behaviour".into(),
                    proof: Some(PlanProof::Review),
                },
                CriterionExcerpt {
                    id: "gates-green".into(),
                    accept: "`just ci` is green on the delivered commit.".into(),
                    provenance: "kettle-beekeeper-agents@0123456789ab:plans/kettle.md#gates-green"
                        .into(),
                    proof: Some(PlanProof::Action {
                        name: "verify".into(),
                        step: "ci".into(),
                    }),
                },
            ],
        },
        decisions: DecisionFacts {
            goal_ref: Some("e".repeat(64)),
            goal_first_line: Some("Ship the kettle CLI".into()),
            decisions: vec![DecisionSummary {
                event_id: "f".repeat(64),
                summary: "Do we vendor the sensor crate? — answered: no".into(),
            }],
        },
        permissions: PermissionFacts {
            agents_access: "none".into(),
            agents_path: None,
            may_push: true,
        },
        runtime: RuntimeFacts {
            runtime: "claude".into(),
            model: Some("opus-5".into()),
            model_source: "routed by the project's model registry (version 3) for class builder \
                           at tier standard"
                .into(),
            compose_app_version: Some("0.1.0".into()),
            compose_digest: Some(format!("sha256:{}", "0".repeat(64))),
        },
        brief_path: Some("/tmp/seats/session/work-brief.md".into()),
    }
}

#[test]
fn a_builder_under_a_declaration_gets_both_criteria_verbatim_with_provenance() {
    let text = assemble_work_brief(&builder_inputs()).render();

    // (a)
    assert!(
        text.contains("Teach the host to hand a seat its work"),
        "{text}"
    );
    assert!(text.contains("Own work_brief.rs alone."), "{text}");
    assert!(
        text.contains("cargo test -p buzz-session-provider"),
        "{text}"
    );
    assert!(
        text.contains("crates/buzz-session-provider/src/work_brief.rs"),
        "{text}"
    );
    assert!(text.contains("Branch: work/lane-209"), "{text}");
    // (b) — real flags only, and never one the CLI does not define.
    for flag in REPORT_FLAGS {
        assert!(text.contains(flag), "section (b) must spell {flag}: {text}");
    }
    // Every command line, and only the command lines: the brief also *names*
    // `--assignment` in a sentence saying the flag does not exist, which is
    // the opposite of teaching it.
    for line in command_lines(&text) {
        for absent in FLAGS_THAT_DO_NOT_EXIST {
            assert!(
                !line.contains(absent),
                "a command line must not spell {absent}, which the CLI does not define: {line}"
            );
        }
        assert!(line.starts_with("$BEE sessions "), "{line}");
    }
    assert!(text.contains("$BEE sessions report --example"), "{text}");
    assert!(
        !text.contains("\n  bee sessions"),
        "never a bare bee: {text}"
    );
    // A builder is told nothing about verdicts.
    assert!(!text.contains("sessions verdict"), "{text}");
    // (c)
    assert!(
        text.contains("kettle-beekeeper-agents@0123456789ab:plans/kettle.md#cli-behaviour"),
        "{text}"
    );
    assert!(
        text.contains("`bee kettle boil` prints the temperature and exits 0."),
        "{text}"
    );
    assert!(text.contains("proved by review"), "{text}");
    assert!(
        text.contains("proved by action: a host run of `verify` step `ci`"),
        "{text}"
    );
    // (d), (e), (f)
    assert!(text.contains("Ship the kettle CLI"), "{text}");
    assert!(text.contains("Agents repository: no grant"), "{text}");
    assert!(text.contains("Runtime: claude. Model: opus-5."), "{text}");
    assert!(text.contains("/tmp/seats/session/work-brief.md"), "{text}");
    // No role prose: the seat bundle already carries it.
    assert!(!text.contains("role pack"), "{text}");
    assert!(
        text.len() <= WORK_BRIEF_BUDGET_BYTES,
        "{} bytes",
        text.len()
    );
}

#[test]
fn a_verifier_is_given_its_verdict_line_its_report_and_the_established_commit() {
    let mut inputs = builder_inputs();
    inputs.assignment.role = "verifier".into();
    inputs.commands.role = "verifier".into();
    inputs.commands.report_ref = Some("9".repeat(64));
    inputs.assignment.establishment = InputEstablishment::Established {
        commit: "b".repeat(40),
    };
    let text = assemble_work_brief(&inputs).render();

    assert!(
        text.contains("this host has already put this worktree on"),
        "{text}"
    );
    assert!(
        text.contains("$BEE sessions verdict --example refutation"),
        "{text}"
    );
    for flag in VERDICT_FLAGS {
        assert!(text.contains(flag), "{flag}: {text}");
    }
    assert!(
        text.contains(&format!("`reportRef` is {}", "9".repeat(64))),
        "{text}"
    );
    assert!(!text.contains("--verifies"), "{text}");
}

#[test]
fn a_verifier_whose_report_could_not_be_resolved_is_told_so_rather_than_given_a_guess() {
    let mut inputs = builder_inputs();
    inputs.assignment.role = "verifier".into();
    inputs.commands.role = "verifier".into();
    inputs.commands.report_ref = None;
    let text = assemble_work_brief(&inputs).render();
    assert!(
        text.contains("could not resolve which report it is about"),
        "{text}"
    );
}

#[test]
fn a_legacy_session_with_no_declaration_still_gets_a_useful_brief() {
    let mut inputs = builder_inputs();
    inputs.contract = ContractFacts::None;
    inputs.decisions = DecisionFacts::default();
    let text = assemble_work_brief(&inputs).render();

    assert!(text.contains("No adopted plan for this session"), "{text}");
    assert!(
        text.contains("This session has no current goal record."),
        "{text}"
    );
    // Everything a seat needs to work and to answer is still there.
    assert!(
        text.contains("Teach the host to hand a seat its work"),
        "{text}"
    );
    assert!(text.contains("$BEE sessions report --channel"), "{text}");
    assert!(
        !text.contains("Dropped to fit"),
        "nothing was dropped: {text}"
    );
}

#[test]
fn an_unreadable_plan_blob_names_the_criterion_ids_and_says_why() {
    let mut inputs = builder_inputs();
    inputs.contract = ContractFacts::PlanUnreadable {
        declaration_ref: "d".repeat(64),
        reason: "that commit is not in this host's clone".into(),
        criterion_ids: vec!["cli-behaviour".into(), "gates-green".into()],
    };
    let text = assemble_work_brief(&inputs).render();

    assert!(
        text.contains("plan text unavailable (that commit is not in this host's clone)"),
        "{text}"
    );
    assert!(text.contains("cli-behaviour, gates-green"), "{text}");
    // Never invented text for a criterion whose bytes were not read.
    assert!(!text.contains("accept:"), "{text}");
}

#[test]
fn an_over_budget_brief_drops_what_you_are_then_decisions_and_says_which() {
    let text = assemble_work_brief(&over_budget_inputs(8, 7)).render();

    assert!(
        text.len() <= WORK_BRIEF_BUDGET_BYTES,
        "{} bytes",
        text.len()
    );
    assert!(
        text.contains(
            "Dropped to fit the 8192-byte brief budget: what you are, decisions in force."
        ),
        "{text}"
    );
    assert!(!text.contains("## WHAT YOU ARE"), "{text}");
    assert!(!text.contains("## DECISIONS IN FORCE"), "{text}");
    // (a), (b) and the contract survive: they are what the turn is for.
    assert!(text.contains("## WHAT YOU OWE"), "{text}");
    assert!(text.contains("## HOW TO ANSWER (ready to run)"), "{text}");
    assert!(text.contains("## THE CONTRACT"), "{text}");
    // Each excerpt was cut with its own provenance named, never mid-nothing.
    assert!(
        text.contains("…truncated; the whole of it is at kettle-beekeeper-agents@0123456789ab:plans/kettle.md#criterion-00"),
        "{text}"
    );
    // No identifier was cut: the assignment id the seat must quote is whole.
    assert!(text.contains(&"a".repeat(64)), "{text}");
}

#[test]
fn what_you_are_is_given_up_before_decisions_are() {
    let text = assemble_work_brief(&over_budget_inputs(8, 1)).render();
    assert!(
        text.len() <= WORK_BRIEF_BUDGET_BYTES,
        "{} bytes",
        text.len()
    );
    assert!(
        text.contains("Dropped to fit the 8192-byte brief budget: what you are."),
        "{text}"
    );
    assert!(!text.contains("## WHAT YOU ARE"), "{text}");
    assert!(text.contains("## DECISIONS IN FORCE"), "{text}");
}

#[test]
fn the_drop_order_never_gives_up_what_you_owe_or_how_to_answer() {
    assert!(!DROP_ORDER.contains(&WorkBriefSection::Owed));
    assert!(!DROP_ORDER.contains(&WorkBriefSection::Commands));
    assert_eq!(DROP_ORDER[0], WorkBriefSection::Runtime);
    assert_eq!(DROP_ORDER[1], WorkBriefSection::Decisions);
}

#[test]
fn an_excerpt_is_cut_on_a_character_boundary_and_names_where_the_rest_is() {
    let whole = "repo@abc123456789:plans/p.md#c";
    let text = bounded(&"é".repeat(400), MAX_CRITERION_EXCERPT_BYTES, whole);
    assert!(text.len() <= MAX_CRITERION_EXCERPT_BYTES, "{}", text.len());
    assert!(text.contains(whole), "{text}");
    // Valid UTF-8 by construction: a panicking slice would have failed above.
    assert!(text.starts_with('é'));
}
