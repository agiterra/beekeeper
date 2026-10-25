//! The collector, driven end to end.
//!
//! This is the part of the work brief that does I/O, and the control run must
//! not be its first execution. Every test here drives
//! [`assemble_brief_text`] — the whole brief downstream of the verified
//! snapshot — against a **throwaway git repository under a temp directory**
//! standing in for this host's agents clone, and an in-memory record source.
//! Never a worktree of this repository: `git_tooling_tests_use_throwaway_clones`.
//!
//! The git side is real. `plan_blob` here delegates to the production
//! [`read_blob_at_commit`], so "the blob comes from the pinned commit", "that
//! commit is not in this host's clone" and "that path is not a plan path" are
//! answered by the code that will answer them live.

use std::collections::BTreeSet;
use std::path::Path;

use beekeeper_core::coding_session_team_transaction::CodingSessionTeamAssignment;
use beekeeper_core::project_work::{
    ProjectWorkAssignmentBound, ProjectWorkBody, ProjectWorkDeclared, ProjectWorkPayload,
    ProjectWorkRecordType,
};

use super::*;

/// The plan the contract is written against, reduced to two criteria so a
/// golden assertion can quote both.
const PLAN: &str = "---\n\
schema: beekeeper-plan/v1\n\
id: kettle-cli\n\
status: in-force\n\
title: Build and land kettle\n\
code_repository: pivot-test\n\
delivery_ref: refs/heads/main\n\
criteria:\n\
\x20 - id: cli-behaviour\n\
\x20   accept: Dependency-free Python 3 package kettle runs as python3 -m kettle.\n\
\x20   proof: {kind: review}\n\
\x20 - id: delivered-main\n\
\x20   accept: One coherent history on main contains the accepted implementation.\n\
\x20   proof: {kind: git-ref}\n\
retired_criteria: []\n\
---\n\
# Kettle\n";

const FOUNDER: &str = "11111111111111111111111111111111111111111111111111111111111111aa";
const CHANNEL: &str = "11111111-2222-3333-4444-555555555555";
const SESSION: &str = "66666666-7777-8888-9999-000000000000";
const GENESIS: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const PROJECT: &str =
    "30621:11111111111111111111111111111111111111111111111111111111111111aa:kettle";
const ASSIGNMENT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DECLARATION: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const BINDING: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const GOAL: &str = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";

/// One kind-44249 record in the shape the relay stores and the fold reads.
fn record(
    id: &str,
    created_at: u64,
    record_type: ProjectWorkRecordType,
    body: ProjectWorkBody,
) -> ProjectWorkEvent {
    let payload = ProjectWorkPayload {
        schema: beekeeper_core::project_work::PROJECT_WORK_SCHEMA.to_owned(),
        session_ref: SESSION.to_owned(),
        genesis_ref: GENESIS.to_owned(),
        project_ref: PROJECT.to_owned(),
        record_type,
        body,
    };
    ProjectWorkEvent {
        id: id.to_owned(),
        pubkey: FOUNDER.to_owned(),
        created_at,
        kind: KIND_PROJECT_WORK_RECORD,
        tags: payload
            .canonical_tags(CHANNEL)
            .into_iter()
            .map(|tag| tag.to_vec())
            .collect(),
        content: payload
            .canonical_content()
            .expect("a representable payload"),
    }
}

fn declared(commit: &str, path: &str) -> ProjectWorkEvent {
    record(
        DECLARATION,
        1_000,
        ProjectWorkRecordType::Declared,
        ProjectWorkBody::Declared(ProjectWorkDeclared {
            work_id: "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee".to_owned(),
            goal_ref: GOAL.to_owned(),
            decision_ref: None,
            responsible_actor: FOUNDER.to_owned(),
            plan_ref: ProjectWorkPlanRef {
                repository: "30617:11111111111111111111111111111111111111111111111111111111111111aa:kettle-beekeeper-agents".to_owned(),
                commit: commit.to_owned(),
                path: path.to_owned(),
            },
            supersedes: Vec::new(),
        }),
    )
}

fn bound_to_assignment() -> ProjectWorkEvent {
    record(
        BINDING,
        1_001,
        ProjectWorkRecordType::AssignmentBound,
        ProjectWorkBody::AssignmentBound(ProjectWorkAssignmentBound {
            declaration_ref: DECLARATION.to_owned(),
            criterion_ids: vec!["cli-behaviour".to_owned()],
            assignment_ref: ASSIGNMENT.to_owned(),
            replaces_binding: None,
        }),
    )
}

fn goal_record() -> ProjectWorkEvent {
    ProjectWorkEvent {
        id: GOAL.to_owned(),
        pubkey: FOUNDER.to_owned(),
        created_at: 900,
        kind: KIND_CODING_SESSION_GOAL,
        tags: vec![
            vec!["h".to_owned(), CHANNEL.to_owned()],
            vec!["d".to_owned(), SESSION.to_owned()],
            vec![
                "csgl-v".to_owned(),
                beekeeper_core::coding_session_goal::CODING_SESSION_GOAL_TAG_VERSION.to_owned(),
            ],
        ],
        content: "Ship the kettle CLI\nand nothing else".to_owned(),
    }
}

/// Records from memory, blobs from a real git repository.
struct Reads {
    records: Result<Vec<ProjectWorkEvent>, String>,
    goals: Vec<ProjectWorkEvent>,
    agents: Option<crate::agents_checkout::AgentsRepoRecord>,
}

impl WorkBriefReads for Reads {
    async fn records(&self, kind: u32) -> Result<Vec<ProjectWorkEvent>, String> {
        if kind == KIND_CODING_SESSION_GOAL {
            return Ok(self.goals.clone());
        }
        self.records.clone()
    }

    async fn plan_blob(&self, plan_ref: &ProjectWorkPlanRef) -> Result<String, String> {
        let Some(record) = self.agents.as_ref() else {
            return Err("this host has no clone of that repository recorded".to_owned());
        };
        read_blob_at_commit(record, &plan_ref.commit, &plan_ref.path)
            .await
            .map_err(|error| error.to_string())
    }
}

fn assignment() -> CodingSessionTeamAssignment {
    CodingSessionTeamAssignment {
        assignee_actor: "99999999999999999999999999999999999999999999999999999999999999bb"
            .to_owned(),
        assignee_role: "builder".to_owned(),
        objective: "Build the kettle CLI".to_owned(),
        brief: "Own kettle/ alone.".to_owned(),
        branch: Some("work/kettle".to_owned()),
        base_sha: None,
        file_ownership: vec!["kettle/".to_owned()],
        acceptance_steps: vec!["python3 -m unittest discover -s tests".to_owned()],
    }
}

fn context(state_dir: &Path) -> BriefContext<'static> {
    BriefContext {
        operation_id: ASSIGNMENT,
        decisions: Vec::new(),
        role: "builder",
        channel: CHANNEL.to_owned(),
        session_ref: SESSION.to_owned(),
        genesis_ref: GENESIS.to_owned(),
        project_ref: Some(PROJECT.to_owned()),
        founder_pubkey: FOUNDER.to_owned(),
        // No worktree: (e) then reports "no grant", which is what a host that
        // cut no sibling clone can actually witness.
        cwd: None,
        vocabulary: Default::default(),
        state_dir: state_dir.to_path_buf(),
        session_id: "77777777-8888-9999-aaaa-bbbbbbbbbbbb".to_owned(),
        runtime: RuntimeFacts {
            runtime: "claude".to_owned(),
            model: Some("opus-5".to_owned()),
            model_source: "the identity's own pin".to_owned(),
            compose_app_version: None,
            compose_digest: None,
        },
        binding: None,
    }
}

async fn sh(dir: &Path, args: &[&str]) -> String {
    let out = tokio::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .await
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// A throwaway agents repository with the plan at one commit and a rewritten
/// plan at the next, so "read at the pinned commit" is a claim with teeth.
///
/// Under a temp dir, never a worktree of this repository.
async fn agents_repo(root: &Path) -> (crate::agents_checkout::AgentsRepoRecord, String) {
    let repo = root.join("kettle-beekeeper-agents");
    std::fs::create_dir_all(repo.join("plans")).unwrap();
    sh(&repo, &["init", "--quiet", "--initial-branch", "main"]).await;
    std::fs::write(repo.join("plans/kettle.md"), PLAN).unwrap();
    sh(&repo, &["add", "--all"]).await;
    sh(&repo, &["commit", "--quiet", "-m", "adopt"]).await;
    let pinned = sh(&repo, &["rev-parse", "HEAD"]).await;
    std::fs::write(
        repo.join("plans/kettle.md"),
        PLAN.replace("Dependency-free Python 3", "SOMETHING ELSE ENTIRELY"),
    )
    .unwrap();
    sh(&repo, &["commit", "--quiet", "-am", "amend"]).await;
    (
        crate::agents_checkout::AgentsRepoRecord {
            path: repo,
            ref_name: "refs/heads/main".into(),
            url: None,
        },
        pinned,
    )
}

#[tokio::test]
async fn the_collector_excerpts_the_criterion_from_the_pinned_commit_and_leaves_a_copy_in_the_bundle(
) {
    let tmp = tempfile::tempdir().unwrap();
    let (agents, pinned) = agents_repo(tmp.path()).await;
    let state_dir = tmp.path().join("state");
    let reads = Reads {
        records: Ok(vec![
            declared(&pinned, "plans/kettle.md"),
            bound_to_assignment(),
        ]),
        goals: vec![goal_record()],
        agents: Some(agents),
    };
    let text = assemble_brief_text(
        &context(&state_dir),
        &assignment(),
        &[],
        &BTreeSet::new(),
        &reads,
    )
    .await
    .unwrap();

    // (c) from the pinned commit, not the tip and not the working copy.
    assert!(
        text.contains("Dependency-free Python 3 package kettle runs as python3 -m kettle."),
        "{text}"
    );
    assert!(!text.contains("SOMETHING ELSE ENTIRELY"), "{text}");
    assert!(
        text.contains(&format!(
            "kettle-beekeeper-agents@{}:plans/kettle.md#cli-behaviour",
            &pinned[..12]
        )),
        "{text}"
    );
    assert!(text.contains("proved by review"), "{text}");
    // Only the bound criterion is quoted; the plan's other one is not.
    assert!(!text.contains("delivered-main"), "{text}");
    // (d) came through the same assembled input set.
    assert!(text.contains("Ship the kettle CLI"), "{text}");
    assert!(text.contains("and nothing else"), "{text}");

    // The seat's own bundle holds the same bytes, and the brief names it last.
    let bundle =
        crate::session::seat_bundle_dir(&state_dir, "77777777-8888-9999-aaaa-bbbbbbbbbbbb")
            .join(WORK_BRIEF_FILE_NAME);
    assert!(bundle.is_file(), "{} was not written", bundle.display());
    assert_eq!(std::fs::read_to_string(&bundle).unwrap(), text);
    let last = text.trim_end().lines().last().unwrap();
    assert!(
        last.contains(&bundle.display().to_string()),
        "the last line must name the copy: {last}"
    );
}

#[tokio::test]
async fn a_commit_this_host_does_not_have_names_the_criterion_ids_and_why() {
    let tmp = tempfile::tempdir().unwrap();
    let (agents, _pinned) = agents_repo(tmp.path()).await;
    let reads = Reads {
        records: Ok(vec![
            declared(&"a".repeat(40), "plans/kettle.md"),
            bound_to_assignment(),
        ]),
        goals: vec![goal_record()],
        agents: Some(agents),
    };
    let text = assemble_brief_text(
        &context(&tmp.path().join("state")),
        &assignment(),
        &[],
        &BTreeSet::new(),
        &reads,
    )
    .await;
    assert_eq!(text, Err(WorkBriefRefusal::Unavailable));
}

#[tokio::test]
async fn a_plan_path_that_could_escape_the_repository_is_refused_by_name() {
    let tmp = tempfile::tempdir().unwrap();
    let (agents, pinned) = agents_repo(tmp.path()).await;
    let reads = Reads {
        records: Ok(vec![
            declared(&pinned, "../../etc/passwd"),
            bound_to_assignment(),
        ]),
        goals: vec![goal_record()],
        agents: Some(agents),
    };
    let text = assemble_brief_text(
        &context(&tmp.path().join("state")),
        &assignment(),
        &[],
        &BTreeSet::new(),
        &reads,
    )
    .await;
    assert_eq!(text, Err(WorkBriefRefusal::Unavailable));
}

#[tokio::test]
async fn a_failed_record_read_refuses_instead_of_claiming_an_absent_contract() {
    let tmp = tempfile::tempdir().unwrap();
    let reads = Reads {
        records: Err("relay kind-44249 partition could not be exhausted".to_owned()),
        goals: Vec::new(),
        agents: None,
    };
    let text = assemble_brief_text(
        &context(&tmp.path().join("state")),
        &assignment(),
        &[],
        &BTreeSet::new(),
        &reads,
    )
    .await;
    assert_eq!(text, Err(WorkBriefRefusal::Unavailable));
}

#[tokio::test]
async fn a_session_with_no_declaration_gets_the_legacy_line() {
    let tmp = tempfile::tempdir().unwrap();
    let reads = Reads {
        records: Ok(Vec::new()),
        goals: vec![goal_record()],
        agents: None,
    };
    let text = assemble_brief_text(
        &context(&tmp.path().join("state")),
        &assignment(),
        &[],
        &BTreeSet::new(),
        &reads,
    )
    .await
    .unwrap();

    assert!(text.contains("No adopted plan for this session"), "{text}");
    assert!(text.contains("Ship the kettle CLI"), "{text}");
}

#[tokio::test]
async fn a_bundle_copy_that_cannot_be_written_never_costs_the_turn_its_brief() {
    let tmp = tempfile::tempdir().unwrap();
    // A *file* where the state directory should be: `create_dir_all` below it
    // fails, which is the whole point — the brief is still returned.
    let blocked = tmp.path().join("state-is-a-file");
    std::fs::write(&blocked, b"not a directory").unwrap();
    let reads = Reads {
        records: Ok(Vec::new()),
        goals: Vec::new(),
        agents: None,
    };
    let text = assemble_brief_text(
        &context(&blocked),
        &assignment(),
        &[],
        &BTreeSet::new(),
        &reads,
    )
    .await
    .unwrap();

    assert!(text.contains("Build the kettle CLI"), "{text}");
    assert!(!text.contains("This brief is also at"));
    assert!(text.len() <= crate::work_brief::WORK_BRIEF_BUDGET_BYTES);
    assert!(blocked.is_file(), "the blocking file was not disturbed");
}

/// Run11 (ledger 272, defect 4): the verdict's `assignmentRef` is the
/// reviewed report's own assignment, read from that report — the report
/// whose `headSha` is the verifier's base — never the verifier's duty.
#[test]
fn the_reviewed_reports_assignment_is_read_from_the_report_itself() {
    use beekeeper_core::coding_session_team_transaction::{
        CodingSessionTeamReport, CodingSessionTeamTransactionBody,
    };
    use beekeeper_sdk::coding_session_team_transaction::{
        build_coding_session_team_transaction, coding_session_team_transaction_payload,
    };
    let builder_assignment = "b".repeat(64);
    let head = "c".repeat(40);
    let payload = coding_session_team_transaction_payload(
        SESSION.to_owned(),
        GENESIS.to_owned(),
        None,
        None,
        CodingSessionTeamTransactionBody::Report(CodingSessionTeamReport {
            assignment_ref: builder_assignment.clone(),
            summary: "Built".into(),
            branch: Some("coding-session-builder-1".into()),
            base_sha: None,
            head_sha: Some(head.clone()),
            files: Vec::new(),
            tests: Vec::new(),
            red_before_green: None,
            deviations: Vec::new(),
            residuals: Vec::new(),
            anomalies: Vec::new(),
        }),
    );
    let report = build_coding_session_team_transaction(CHANNEL, payload)
        .expect("the record builds")
        .sign_with_keys(&nostr::Keys::generate())
        .expect("the record signs");
    let included = std::iter::once(report.id.to_hex()).collect();
    let default = beekeeper_core::role_vocabulary::RoleVocabulary::default();
    let reviewed = super::report_under_review(
        std::slice::from_ref(&report),
        &included,
        Some(&head),
        "verifier",
        &default,
    );
    assert_eq!(
        reviewed,
        Some((report.id.to_hex(), Some(builder_assignment))),
        "the verdict names the builder's assignment, from the report"
    );
    assert_eq!(
        super::report_under_review(
            std::slice::from_ref(&report),
            &included,
            Some(&head),
            "builder",
            &default
        ),
        None,
        "only a verifier rules on a report"
    );
    // Under a project whose judging role is `qa`, `qa` resolves the report
    // and the foundation's `verifier` does not.
    let qa = beekeeper_core::role_vocabulary::RoleVocabulary::from_team(
        Some("captain"),
        Some(&["qa".to_owned()]),
    )
    .expect("a vocabulary");
    assert!(
        super::report_under_review(
            std::slice::from_ref(&report),
            &included,
            Some(&head),
            "qa",
            &qa
        )
        .is_some(),
        "qa is this project's judging role"
    );
    assert_eq!(
        super::report_under_review(&[report], &included, Some(&head), "verifier", &qa),
        None,
        "verifier is not a role of this project"
    );
}

/// Collector -> provider admission, with no relay service or model process.
/// This is deliberately downstream of complete canonical reads and upstream
/// of SessionCommand construction, exactly the production admission seam.
#[tokio::test]
async fn required_overflow_cannot_cross_collector_provider_admission() {
    use crate::work_brief::WORK_BRIEF_BUDGET_BYTES;
    struct CompleteReads {
        plan: String,
        extra: bool,
    }
    impl WorkBriefReads for CompleteReads {
        async fn records(&self, kind: u32) -> Result<Vec<ProjectWorkEvent>, String> {
            Ok(if kind == KIND_CODING_SESSION_GOAL {
                vec![goal_record()]
            } else {
                vec![
                    declared(&"a".repeat(40), "plans/kettle.md"),
                    if self.extra {
                        record(
                            BINDING,
                            1_001,
                            ProjectWorkRecordType::AssignmentBound,
                            ProjectWorkBody::AssignmentBound(ProjectWorkAssignmentBound {
                                declaration_ref: DECLARATION.into(),
                                criterion_ids: (0..10).map(|i| format!("criterion-{i}")).collect(),
                                assignment_ref: ASSIGNMENT.into(),
                                replaces_binding: None,
                            }),
                        )
                    } else {
                        bound_to_assignment()
                    },
                ]
            })
        }
        async fn plan_blob(&self, _: &ProjectWorkPlanRef) -> Result<String, String> {
            Ok(self.plan.clone())
        }
    }
    for field in ["objective", "brief", "criteria", "permissions"] {
        let dir = tempfile::tempdir().unwrap();
        let mut ctx = context(dir.path());
        let mut assignment = assignment();
        let mut reads = CompleteReads {
            plan: PLAN.to_owned(),
            extra: false,
        };
        let long = "界".repeat(4096);
        match field {
            "objective" => assignment.objective = long,
            "brief" => assignment.brief = long,
            "criteria" => {
                let criteria = (0..10)
                    .map(|i| {
                        format!(
                            " - id: criterion-{i}\n   accept: {}\n   proof: {{kind: review}}\n",
                            "界".repeat(300)
                        )
                    })
                    .collect::<String>();
                let start = PLAN.find("criteria:\n").unwrap() + "criteria:\n".len();
                let end = PLAN.find("retired_criteria:").unwrap();
                reads.plan = format!("{}{}{}", &PLAN[..start], criteria, &PLAN[end..]);
                reads.extra = true;
            }
            "permissions" => {
                ctx.binding = Some(crate::execution_scope::ExecutionBinding {
                    scope_digest: "digest".into(),
                    project_ref: None,
                    actor: None,
                    tree: None,
                    agents: Some(PathBuf::from(long)),
                    branch: None,
                    agents_writable: false,
                })
            }
            _ => unreachable!(),
        }
        let prepared = assemble_brief_text(&ctx, &assignment, &[], &BTreeSet::new(), &reads).await;
        assert_eq!(prepared, Err(WorkBriefRefusal::TooLarge), "{field}");
        assert!(
            !crate::session::seat_bundle_dir(dir.path(), &ctx.session_id)
                .join(WORK_BRIEF_FILE_NAME)
                .exists()
        );
        let mut provider = crate::tests::provider(&dir.path().join("provider"), None);
        let target = crate::CodingSessionTarget {
            driver: "claude-agent-acp".into(),
            instance_id: provider.config.instance_id.clone(),
            session_id: ctx.session_id.clone(),
            generation: 1,
        };
        let admitted = provider
            .admit_work_brief(
                uuid::Uuid::parse_str(CHANNEL).unwrap(),
                "oversize",
                &target,
                prepared.map(Some),
            )
            .unwrap();
        assert!(
            matches!(admitted, Err(crate::TurnDisposition::Answered(ref code)) if code == "WORK_BRIEF_TOO_LARGE")
        );
        assert!(provider.state.is_command_refused("oversize"));
        assert!(!provider.state.is_command_consumed("oversize"));
        assert!(provider
            .state
            .terminal_dispositions()
            .iter()
            .all(|(_, row)| row.event.content.len() < WORK_BRIEF_BUDGET_BYTES));
    }
}

#[tokio::test]
async fn decision_questions_notes_and_scope_are_complete_or_the_turn_is_refused() {
    use beekeeper_core::coding_session_team_transaction::*;
    use beekeeper_sdk::coding_session_team_transaction::*;
    let keys = nostr::Keys::generate();
    let sign = |body| {
        build_coding_session_team_transaction(
            CHANNEL,
            coding_session_team_transaction_payload(SESSION, GENESIS, None, None, body),
        )
        .unwrap()
        .sign_with_keys(&keys)
        .unwrap()
    };
    let question = format!("{}\nDo not touch production.", "é".repeat(110));
    let request = sign(CodingSessionTeamTransactionBody::DecisionRequest(
        CodingSessionTeamDecisionRequest {
            question: question.clone(),
            options: vec!["local only".into()],
            held_on: "founder".into(),
            blocks: vec![ASSIGNMENT.into()],
            recommendation: None,
        },
    ));
    let answer = sign(CodingSessionTeamTransactionBody::DecisionAnswer(
        CodingSessionTeamDecisionAnswer {
            request_ref: request.id.to_hex(),
            choice: CodingSessionTeamDecisionChoice::Index(0),
            note: Some("Never publish credentials.".into()),
            condition: Some("Only on an isolated checkout.".into()),
        },
    ));
    let events = vec![request, answer];
    let dir = tempfile::tempdir().unwrap();
    let reads = Reads {
        records: Ok(vec![]),
        goals: vec![],
        agents: None,
    };
    let mut ctx = context(dir.path());
    // Renderer fixture only; the relay-backed provider tests prove authority admission.
    let projected = |events: &[Event]| {
        events
            .chunks_exact(2)
            .map(|pair| CodingSessionTeamFoldDecision {
                request_event_id: pair[0].id.to_hex(),
                held_on: "founder".into(),
                blocks: vec![ASSIGNMENT.into()],
                answered_by: Some(keys.public_key().to_hex()),
                answer_event_id: Some(pair[1].id.to_hex()),
            })
            .collect()
    };
    ctx.decisions = projected(&events);
    let text = assemble_brief_text(&ctx, &assignment(), &events, &BTreeSet::new(), &reads)
        .await
        .unwrap();
    for required in [
        &question,
        "Never publish credentials.",
        "Only on an isolated checkout.",
    ] {
        assert!(text.contains(required));
    }
    // Many distinct valid decisions, rather than invalid oversize wire fields.
    let mut many = Vec::new();
    for n in 0..40 {
        let request = sign(CodingSessionTeamTransactionBody::DecisionRequest(
            CodingSessionTeamDecisionRequest {
                question: format!("{n}: {question}"),
                options: vec!["local only".into()],
                held_on: "founder".into(),
                blocks: vec![ASSIGNMENT.into()],
                recommendation: None,
            },
        ));
        let answer = sign(CodingSessionTeamTransactionBody::DecisionAnswer(
            CodingSessionTeamDecisionAnswer {
                request_ref: request.id.to_hex(),
                choice: CodingSessionTeamDecisionChoice::Index(0),
                note: None,
                condition: None,
            },
        ));
        many.extend([request, answer]);
    }
    ctx.decisions = projected(&many);
    let prepared = assemble_brief_text(&ctx, &assignment(), &many, &BTreeSet::new(), &reads).await;
    assert_eq!(prepared, Err(WorkBriefRefusal::TooLarge));
    let mut provider = crate::tests::provider(&dir.path().join("provider"), None);
    let target = crate::CodingSessionTarget {
        driver: "claude-agent-acp".into(),
        instance_id: provider.config.instance_id.clone(),
        session_id: ctx.session_id.clone(),
        generation: 1,
    };
    assert!(provider
        .admit_work_brief(
            uuid::Uuid::parse_str(CHANNEL).unwrap(),
            "decisions",
            &target,
            prepared.map(Some)
        )
        .unwrap()
        .is_err());
    assert!(provider.state.is_command_refused("decisions"));
}

#[tokio::test]
async fn rendering_changed_facts_does_not_reuse_cached_partial_content() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = context(dir.path());
    let reads = Reads {
        records: Ok(vec![]),
        goals: vec![],
        agents: None,
    };
    let mut assigned = assignment();
    assigned.brief = "é".repeat(8192);
    assert_eq!(
        assemble_brief_text(&ctx, &assigned, &[], &BTreeSet::new(), &reads).await,
        Err(WorkBriefRefusal::TooLarge)
    );
    assigned.brief = "Corrected complete instruction. Never edit outside kettle/.".into();
    let text = assemble_brief_text(&ctx, &assigned, &[], &BTreeSet::new(), &reads)
        .await
        .unwrap();
    assert!(text.contains(&assigned.brief));
    assert_eq!(
        std::fs::read_to_string(
            crate::session::seat_bundle_dir(dir.path(), &ctx.session_id).join(WORK_BRIEF_FILE_NAME)
        )
        .unwrap(),
        text
    );
}

#[tokio::test]
async fn failed_goal_read_is_unknown_not_no_goal() {
    struct MissingGoal;
    impl WorkBriefReads for MissingGoal {
        async fn records(&self, kind: u32) -> Result<Vec<ProjectWorkEvent>, String> {
            if kind == KIND_CODING_SESSION_GOAL {
                Err("unavailable".into())
            } else {
                Ok(vec![])
            }
        }
        async fn plan_blob(&self, _: &ProjectWorkPlanRef) -> Result<String, String> {
            unreachable!()
        }
    }
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(
        assemble_brief_text(
            &context(dir.path()),
            &assignment(),
            &[],
            &BTreeSet::new(),
            &MissingGoal
        )
        .await,
        Err(WorkBriefRefusal::Unavailable)
    );
}

#[tokio::test]
async fn a_partially_resolved_binding_does_not_silently_lose_its_missing_criterion() {
    let dir = tempfile::tempdir().unwrap();
    let (agents, pinned) = agents_repo(dir.path()).await;
    let binding = record(
        BINDING,
        1_001,
        ProjectWorkRecordType::AssignmentBound,
        ProjectWorkBody::AssignmentBound(ProjectWorkAssignmentBound {
            declaration_ref: DECLARATION.into(),
            criterion_ids: vec!["cli-behaviour".into(), "missing-constraint".into()],
            assignment_ref: ASSIGNMENT.into(),
            replaces_binding: None,
        }),
    );
    let reads = Reads {
        records: Ok(vec![declared(&pinned, "plans/kettle.md"), binding.clone()]),
        goals: vec![goal_record()],
        agents: Some(agents),
    };
    assert_eq!(
        assemble_brief_text(
            &context(dir.path()),
            &assignment(),
            &[],
            &BTreeSet::new(),
            &reads
        )
        .await,
        Err(WorkBriefRefusal::Unavailable)
    );
    // A record excluded by the authority fold must not manufacture a
    // mandatory constraint or denial for this assignment.
    let mut foreign = binding;
    foreign.id = "8".repeat(64);
    foreign.pubkey = "9".repeat(64);
    let reads = Reads {
        records: Ok(vec![
            declared(&pinned, "plans/kettle.md"),
            bound_to_assignment(),
            foreign,
        ]),
        goals: vec![goal_record()],
        agents: reads.agents,
    };
    assert!(assemble_brief_text(
        &context(dir.path()),
        &assignment(),
        &[],
        &BTreeSet::new(),
        &reads
    )
    .await
    .is_ok());
}
