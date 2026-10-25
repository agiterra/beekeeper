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
/// real `clap` definitions** in `crates/beekeeper-cli/tests/work_brief_flags.rs`,
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
/// the real definitions in `crates/beekeeper-cli/tests/work_brief_flags.rs`.
pub(crate) const FLAGS_THAT_DO_NOT_EXIST: [&str; 2] = ["--assignment", "--verifies"];

/// Every runnable line of a rendered brief.
fn command_lines(text: &str) -> Vec<&str> {
    text.lines()
        .map(str::trim)
        .filter(|line| line.starts_with("$BEE"))
        .collect()
}

fn builder_inputs() -> WorkBriefInputs {
    WorkBriefInputs {
        assignment: AssignmentFacts {
            assignment_ref: "a".repeat(64),
            role: "builder".into(),
            objective: "Teach the host to hand a seat its work".into(),
            brief: "Own work_brief.rs alone. Do not touch pending_completion.rs.".into(),
            branch: Some("work/lane-209".into()),
            assignment_branch: None,
            base_sha: Some("b".repeat(40)),
            file_ownership: vec!["crates/beekeeper-session-provider/src/work_brief.rs".into()],
            acceptance_steps: vec![
                "cargo test -p beekeeper-session-provider".into(),
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
            vocabulary: Default::default(),
            report_ref: None,
            verdict_assignment_ref: None,
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
    let text = assemble_work_brief(&builder_inputs()).render().unwrap();

    // (a)
    assert!(
        text.contains("Teach the host to hand a seat its work"),
        "{text}"
    );
    assert!(text.contains("Own work_brief.rs alone."), "{text}");
    assert!(
        text.contains("cargo test -p beekeeper-session-provider"),
        "{text}"
    );
    assert!(
        text.contains("crates/beekeeper-session-provider/src/work_brief.rs"),
        "{text}"
    );
    assert!(text.contains("Branch: `work/lane-209`"), "{text}");
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
    inputs.commands.verdict_assignment_ref = Some("e".repeat(64));
    inputs.assignment.establishment = InputEstablishment::Established {
        commit: "b".repeat(40),
    };
    let text = assemble_work_brief(&inputs).render().unwrap();

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
    let text = assemble_work_brief(&inputs).render().unwrap();
    assert!(
        text.contains("could not resolve which report this verification is about"),
        "{text}"
    );
    // No id is offered for the verdict: the seat's own assignment is named
    // only as the one its settlement report keeps.
    assert!(
        !text.contains(&format!(
            "verdict body's `assignmentRef` is {}",
            "a".repeat(64)
        )),
        "{text}"
    );
}

/// Run11 (ledger 272, defect 4): the brief told the verifier to put its own
/// assignment in the verdict. The CLI and the fold both refuse that; the
/// verdict must name the reviewed report's assignment.
#[test]
fn a_verdict_names_the_reviewed_reports_assignment_never_the_verifiers_own() {
    let mut inputs = builder_inputs();
    let own = "a".repeat(64);
    let reviewed = "e".repeat(64);
    inputs.assignment.role = "verifier".into();
    inputs.commands.role = "verifier".into();
    inputs.commands.report_ref = Some("9".repeat(64));
    inputs.commands.verdict_assignment_ref = Some(reviewed.clone());
    let text = assemble_work_brief(&inputs).render().unwrap();
    assert!(
        text.contains(&format!("its `assignmentRef` is {reviewed}")),
        "{text}"
    );
    assert!(
        !text.contains(&format!("verdict body's `assignmentRef` is {own}")),
        "{text}"
    );
    assert!(
        text.contains(&format!("The report body's `assignmentRef` is {own}")),
        "the settlement report keeps the verifier's own duty: {text}"
    );
}

/// Run11 (ledger 272, defect 1): the assignment named `work/lapbook-cli`,
/// the host had cut the builder's worktree on its own branch, and the brief
/// printed only the first. The brief names the branch the seat can commit to
/// and turns the assignment's name into a push target.
#[test]
fn the_brief_names_the_allocated_branch_and_pushes_to_the_assignments_name() {
    let mut inputs = builder_inputs();
    inputs.assignment.branch = Some("coding-session-builder-1".into());
    inputs.assignment.assignment_branch = Some("work/lapbook-cli".into());
    let text = assemble_work_brief(&inputs).render().unwrap();
    assert!(
        text.contains("Branch: `coding-session-builder-1`, allocated to this worktree by the host"),
        "{text}"
    );
    assert!(
        text.contains("git push origin HEAD:refs/heads/work/lapbook-cli"),
        "{text}"
    );
    assert!(text.contains("a new local branch is refused"), "{text}");
}

#[test]
fn a_legacy_session_with_no_declaration_still_gets_a_useful_brief() {
    let mut inputs = builder_inputs();
    inputs.contract = ContractFacts::None;
    inputs.decisions = DecisionFacts::default();
    let text = assemble_work_brief(&inputs).render().unwrap();

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
    let text = assemble_work_brief(&inputs).render().unwrap();

    assert!(
        text.contains("plan text unavailable (that commit is not in this host's clone)"),
        "{text}"
    );
    assert!(text.contains("cli-behaviour, gates-green"), "{text}");
    // Never invented text for a criterion whose bytes were not read.
    assert!(!text.contains("accept:"), "{text}");
}

#[test]
fn required_sections_are_never_dropped_or_truncated() {
    for section in [
        WorkBriefSection::Owed,
        WorkBriefSection::Commands,
        WorkBriefSection::Contract,
        WorkBriefSection::Decisions,
        WorkBriefSection::Permissions,
    ] {
        let mut brief = assemble_work_brief(&builder_inputs());
        brief
            .sections
            .iter_mut()
            .find(|s| s.section == section)
            .unwrap()
            .body = "義務".repeat(4096);
        assert_eq!(
            brief.render(),
            Err(WorkBriefRefusal::TooLarge),
            "{section:?}"
        );
    }
}

#[test]
fn only_runtime_can_be_omitted_with_disclosure_inside_the_byte_bound() {
    let mut inputs = builder_inputs();
    inputs.runtime.model_source = "🦀".repeat(8192);
    let text = assemble_work_brief(&inputs).render().unwrap();
    assert!(text.len() <= WORK_BRIEF_BUDGET_BYTES);
    assert!(text.contains("Dropped to fit the 8192-byte brief budget: what you are."));
    for heading in [
        "## WHAT YOU OWE",
        "## THE CONTRACT",
        "## DECISIONS IN FORCE",
        "## WHAT YOU MAY DO",
    ] {
        assert!(text.contains(heading));
    }
}

#[test]
fn field_thresholds_do_not_hide_constraints_in_prose_or_criteria() {
    let mut inputs = builder_inputs();
    inputs.assignment.brief = format!("{}\nNever publish private material.", "é".repeat(550));
    if let ContractFacts::Declared { criteria, .. } = &mut inputs.contract {
        criteria[0].accept = format!("{}\nOnly with the recorded grant.", "界".repeat(180));
    }
    let text = assemble_work_brief(&inputs).render().unwrap();
    assert!(text.contains(&inputs.assignment.brief));
    assert!(text.contains("Only with the recorded grant."));
    assert!(!text.contains("truncated"));
    inputs.assignment.brief = "é".repeat(8192);
    assert_eq!(
        assemble_work_brief(&inputs).render(),
        Err(WorkBriefRefusal::TooLarge)
    );
}

#[test]
fn refusal_messages_are_bounded_and_do_not_echo_required_content() {
    for refusal in [WorkBriefRefusal::TooLarge, WorkBriefRefusal::Unavailable] {
        assert!(refusal.to_string().len() < WORK_BRIEF_BUDGET_BYTES);
        assert!(refusal.to_string().contains("not submitted"));
    }
}
