//! Tests for [`super`] — the harness, proven without a model.
//!
//! Everything here runs against a **stub runtime**: a [`BenchRunner`] that
//! writes fixed files into the scratch dir and returns a fixed exit code. That
//! is deliberate. A test that needed a real adapter would prove the adapter,
//! not the harness, and it could not assert that two runs of the same
//! artifacts score identically — which is the one property the word "measured"
//! rests on.

use std::sync::atomic::{AtomicU32, Ordering};

use super::*;

use buzz_core::coding_session_payload::Capabilities;
use buzz_core::coding_session_runtime::{CliEnvVar, RuntimeDescriptor};
use buzz_core::registry_bench::{Check, FileContainsCheck, RunArtifacts};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("repo root")
}

fn bench_root() -> PathBuf {
    repo_root().join(REGISTRY_BENCH_RELATIVE_PATH)
}

fn descriptor() -> RuntimeDescriptor {
    RuntimeDescriptor {
        steer_idle_guard: None,
        instance_ref: "claude-primary".to_owned(),
        driver: "claude".to_owned(),
        runtime: "claude".to_owned(),
        agent_command: "/usr/bin/true".to_owned(),
        agent_args: vec!["--acp".to_owned()],
        cli_env: Some(CliEnvVar {
            name: "CLAUDE_CODE_EXECUTABLE".to_owned(),
            value: "/opt/claude".to_owned(),
        }),
        default_model: "default".to_owned(),
        allowed_models: vec!["opus[1m]".to_owned(), "sonnet".to_owned()],
        discover_models: false,
        capabilities: Some(Capabilities::v1_for_runtime("claude")),
    }
}

/// Writes a fixed set of files and returns a fixed exit code. Optionally
/// counts its own invocations, so a `--repeat 3` can be asserted as three runs
/// per task rather than taken on trust.
struct StubRunner {
    files: Vec<(&'static str, &'static str)>,
    exit_code: Option<i32>,
    timed_out: bool,
    calls: AtomicU32,
    /// A file the stub writes into its own scratch dir, for the F1 attack.
    sabotage: Option<(&'static str, &'static str)>,
}

impl StubRunner {
    fn new(files: Vec<(&'static str, &'static str)>) -> Self {
        Self {
            files,
            exit_code: Some(0),
            timed_out: false,
            calls: AtomicU32::new(0),
            sabotage: None,
        }
    }
}

impl BenchRunner for StubRunner {
    fn run(
        &self,
        _plan: &SpawnPlan,
        _prompt: &str,
        scratch: &Path,
        _timeout: Duration,
    ) -> Result<RawRun, CliError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        std::fs::create_dir_all(scratch.join(BENCH_ARTIFACT_DIR)).expect("scratch");
        for (name, body) in &self.files {
            std::fs::write(scratch.join(name), body).expect("write");
        }
        // The harness's own measurements, which the rubric is allowed to read.
        std::fs::write(scratch.join(BENCH_ARTIFACT_DIR).join("duration-ms"), "4200")
            .expect("duration");
        if let Some((path, body)) = &self.sabotage {
            // Whatever the subject writes into its own scratch dir — including
            // an `exit 0` over a grader's name.
            if let Some(parent) = scratch.join(path).parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            std::fs::write(scratch.join(path), body).expect("sabotage");
        }
        Ok(RawRun {
            exit_code: self.exit_code,
            stdout: "stub runtime\n".to_owned(),
            timed_out: self.timed_out,
            duration_ms: 4_200,
        })
    }
}

// ── the checked-in bench parses and hashes ───────────────────────────────────

#[test]
fn every_shipped_bench_parses_and_the_stubs_say_they_are_stubs() {
    for role in [
        "verifier",
        "builder",
        "runner",
        "lead",
        "architect",
        "ui_designer",
        "researcher",
        "poker",
    ] {
        let (set, tasks) =
            load_role_bench(&bench_root(), role).unwrap_or_else(|error| panic!("{role}: {error}"));
        assert_eq!(set.role, role);
        assert_eq!(set.tasks.len(), tasks.len());
        if matches!(role, "verifier" | "builder" | "runner") {
            assert!(!set.is_stub(), "{role} ships a task set");
            assert!(tasks.iter().all(|task| !task.criteria.is_empty()));
        } else {
            assert!(set.is_stub(), "{role} ships as a zero-task stub");
        }
    }
}

/// Every trait the seeded roles' class gates read must be evidenced, or the
/// bench cannot ever produce a proposable row. This is the check that catches
/// a rubric drifting away from the gate it is measured against.
#[test]
fn each_seeded_bench_evidences_every_trait_its_class_gate_reads() {
    let registry_text =
        std::fs::read_to_string(repo_root().join("team/model-registry.yaml")).expect("registry");
    let registry =
        buzz_core::coding_session_routing::parse_registry(&registry_text).expect("parses");
    for role in ["verifier", "builder", "runner"] {
        let (_, tasks) = load_role_bench(&bench_root(), role).expect("bench");
        let tagged: std::collections::BTreeSet<String> = tasks
            .iter()
            .flat_map(|task| task.criteria.iter())
            .flat_map(|criterion| criterion.traits.iter().cloned())
            .collect();
        let gate = &registry.classes[role].minimums;
        for trait_name in gate.keys() {
            assert!(
                tagged.contains(trait_name),
                "the {role} gate reads {trait_name} and its bench evidences nothing about it — \
                 propose would refuse this row forever"
            );
        }
    }
}

#[test]
fn the_bench_hash_changes_when_a_rubric_changes_and_not_otherwise() {
    let first = hash_role_bench(&bench_root(), "verifier").expect("hash");
    let second = hash_role_bench(&bench_root(), "verifier").expect("hash");
    assert_eq!(first, second, "the same tree hashes the same");
    let other = hash_role_bench(&bench_root(), "builder").expect("hash");
    assert_ne!(first, other);
}

#[test]
fn the_bench_root_is_a_sibling_of_the_registry_it_writes_into() {
    let registry = repo_root().join("team/model-registry.yaml");
    assert_eq!(
        bench_root_for(&registry),
        repo_root().join("team/registry-bench")
    );
}

// ── the spawn plan is the real one ───────────────────────────────────────────

/// The gate row's `command` is the argv actually spawned. Asserted here
/// because a row whose command is a paraphrase proves nothing — finding 26 is
/// exactly a prose claim about a command that was never run.
#[test]
fn the_plan_is_the_descriptors_own_argv_and_the_gate_row_records_it_verbatim() {
    let plan = plan_for(&descriptor(), "opus[1m]").expect("plan");
    assert_eq!(
        plan.argv,
        vec![
            "/usr/bin/true".to_owned(),
            "--acp".to_owned(),
            "--model".to_owned(),
            "opus[1m]".to_owned()
        ]
    );
    assert_eq!(plan.command_line(), "/usr/bin/true --acp --model opus[1m]");
    assert_eq!(
        plan.env,
        vec![(
            "CLAUDE_CODE_EXECUTABLE".to_owned(),
            "/opt/claude".to_owned()
        )]
    );

    let result = TaskRunResult {
        task_id: "planted-defect".to_owned(),
        run: 0,
        score: score_run(&[], &RunArtifacts::default()),
        command: plan.command_line(),
        duration_ms: 1,
    };
    assert_eq!(result.command, plan.command_line());
}

#[test]
fn a_model_the_runtime_does_not_offer_is_refused_by_name() {
    let error = plan_for(&descriptor(), "gpt-5.6-sol").expect_err("must refuse");
    let text = error.to_string();
    assert!(text.contains("gpt-5.6-sol"), "{text}");
    assert!(text.contains("opus[1m], sonnet"), "{text}");
}

// ── the harness end to end, on a stub runtime ────────────────────────────────

fn verifier_tasks() -> Vec<BenchTaskSpec> {
    load_role_bench(&bench_root(), "verifier").expect("bench").1
}

fn manifest() -> BTreeMap<String, Vec<u8>> {
    read_role_bench_files(&bench_root(), "verifier").expect("manifest")
}

fn perfect_stub() -> StubRunner {
    StubRunner::new(vec![
        (
            "report.md",
            "- `last_page` returns one too many pages at an exact multiple\n",
        ),
        ("verdict.json", r#"{"verdict": "changes-requested"}"#),
    ])
}

#[test]
fn a_perfect_run_scores_five_on_every_trait_the_bench_evidences() {
    let scratch = tempfile::tempdir().expect("tempdir");
    let (results, per_run) = run_bench(
        &perfect_stub(),
        &plan_for(&descriptor(), "opus[1m]").expect("plan"),
        &verifier_tasks(),
        3,
        scratch.path(),
        Duration::from_secs(30),
        &manifest(),
    )
    .expect("runs");
    assert_eq!(results.len(), 3, "one task, three runs");
    for result in &results {
        assert_eq!(result.outcome(), "passed", "{:?}", result.score.failed);
        assert_eq!(result.summary(), "22/22 · failed: none");
    }
    let traits = aggregate_runs(&per_run);
    for name in ["reasoning", "judgment", "verification", "discipline"] {
        let measured = traits
            .get(name)
            .unwrap_or_else(|| panic!("{name} must be measured"));
        assert!((measured.score - 5.0).abs() < f64::EPSILON, "{name}");
        assert_eq!(measured.n, 3);
        assert!(!measured.is_unstable());
    }
    // Nothing tagged these, so they are absent rather than zero.
    for name in ["taste", "velocity", "costEfficiency"] {
        assert!(!traits.contains_key(name), "{name} must not appear");
    }
}

/// The verifier bench's whole point: naming defects that are not there costs
/// judgment and discipline, and the numbers say so.
#[test]
fn a_run_that_reports_an_absent_defect_loses_judgment_and_discipline() {
    let scratch = tempfile::tempdir().expect("tempdir");
    let runner = StubRunner::new(vec![
        (
            "report.md",
            "- `last_page` is off by one\n- `page_starts` might overflow\n",
        ),
        ("verdict.json", r#"{"verdict": "changes-requested"}"#),
    ]);
    let (results, per_run) = run_bench(
        &runner,
        &plan_for(&descriptor(), "opus[1m]").expect("plan"),
        &verifier_tasks(),
        3,
        scratch.path(),
        Duration::from_secs(30),
        &manifest(),
    )
    .expect("runs");
    assert_eq!(results[0].outcome(), "failed");
    assert_eq!(
        results[0].score.failed,
        vec!["reports-no-absent-defect".to_owned()]
    );
    assert_eq!(
        results[0].summary(),
        "16/22 · failed: reports-no-absent-defect"
    );

    // Partial credit, and the weights compose. judgment is tagged by
    // reports-no-absent-defect (6, failed) and verdict-is-changes-requested
    // (4, passed): 4 of 10 → 1.0 + 4.0×0.4 = 2.6. discipline is tagged by
    // reports-no-absent-defect (6, failed), wrote-nothing-else (2) and
    // exited-clean (2): 4 of 10 → 2.6. Nothing the run got right is thrown
    // away because something else went wrong.
    let traits = aggregate_runs(&per_run);
    assert!(
        (traits["judgment"].score - 2.6).abs() < f64::EPSILON,
        "{:?}",
        traits["judgment"]
    );
    assert!((traits["discipline"].score - 2.6).abs() < f64::EPSILON);
    assert!((traits["reasoning"].score - 5.0).abs() < f64::EPSILON);
    assert!((traits["verification"].score - 5.0).abs() < f64::EPSILON);
}

/// Two runs of the same harness over the same stub produce the same decimals.
/// This is the repeated-run proof: nothing in the scoring path is a clock, a
/// hash-map order, or a model.
#[test]
fn two_passes_of_the_harness_over_one_stub_agree_exactly() {
    let plan = plan_for(&descriptor(), "opus[1m]").expect("plan");
    let first_dir = tempfile::tempdir().expect("tempdir");
    let second_dir = tempfile::tempdir().expect("tempdir");
    let (first_results, first_runs) = run_bench(
        &perfect_stub(),
        &plan,
        &verifier_tasks(),
        3,
        first_dir.path(),
        Duration::from_secs(30),
        &manifest(),
    )
    .expect("runs");
    let (second_results, second_runs) = run_bench(
        &perfect_stub(),
        &plan,
        &verifier_tasks(),
        3,
        second_dir.path(),
        Duration::from_secs(30),
        &manifest(),
    )
    .expect("runs");
    assert_eq!(first_runs, second_runs);
    assert_eq!(
        first_results
            .iter()
            .map(TaskRunResult::summary)
            .collect::<Vec<String>>(),
        second_results
            .iter()
            .map(TaskRunResult::summary)
            .collect::<Vec<String>>()
    );
    assert_eq!(
        aggregate_runs(&first_runs),
        aggregate_runs(&second_runs),
        "the same artifacts must score the same, or `measured` means nothing"
    );
}

/// A task that outran its budget fails every criterion, and the failure names
/// the budget rather than the check.
#[test]
fn a_timed_out_run_fails_everything_and_says_why() {
    let mut runner = perfect_stub();
    runner.timed_out = true;
    let scratch = tempfile::tempdir().expect("tempdir");
    let (results, _) = run_bench(
        &runner,
        &plan_for(&descriptor(), "opus[1m]").expect("plan"),
        &verifier_tasks(),
        3,
        scratch.path(),
        Duration::from_secs(30),
        &manifest(),
    )
    .expect("runs");
    assert_eq!(results[0].outcome(), "failed");
    assert_eq!(results[0].score.weight_passed, 0);
    assert!(results[0].score.outcomes[0].detail.contains("30s budget"));
}

/// `--repeat` is honoured literally: three runs is three spawns per task, and
/// nothing is reused between them.
#[test]
fn repeat_spawns_the_runtime_once_per_task_per_run() {
    let runner = perfect_stub();
    let scratch = tempfile::tempdir().expect("tempdir");
    run_bench(
        &runner,
        &plan_for(&descriptor(), "opus[1m]").expect("plan"),
        &verifier_tasks(),
        4,
        scratch.path(),
        Duration::from_secs(30),
        &manifest(),
    )
    .expect("runs");
    assert_eq!(runner.calls.load(Ordering::SeqCst), 4);
}

/// A fixture is copied, never shared: run 1 cannot see what run 0 wrote.
#[test]
fn each_run_gets_its_own_scratch_copy_of_the_fixture() {
    let scratch = tempfile::tempdir().expect("tempdir");
    run_bench(
        &perfect_stub(),
        &plan_for(&descriptor(), "opus[1m]").expect("plan"),
        &verifier_tasks(),
        3,
        scratch.path(),
        Duration::from_secs(30),
        &manifest(),
    )
    .expect("runs");
    for run in 0..3 {
        let dir = scratch.path().join(format!("planted-defect-{run}"));
        assert!(
            dir.join("pager.rs").is_file(),
            "run {run} staged the fixture"
        );
        assert!(
            dir.join("report.md").is_file(),
            "run {run} has its own output"
        );
    }
}

/// The manifest is the only thing that carries `benchHash` onto the wire, and
/// `propose` refuses without it. Asserted so a rename cannot silently break the
/// one field that proves the task set did not move under the measurement.
#[test]
fn the_run_manifest_names_the_bench_version_and_hash() {
    let (set, _) = load_role_bench(&bench_root(), "verifier").expect("bench");
    let manifest = run_manifest(
        "verifier",
        &set,
        "abc123",
        &plan_for(&descriptor(), "opus[1m]").expect("plan"),
        3,
        1,
    );
    assert_eq!(manifest["kind"], "registry-bench-manifest");
    assert_eq!(manifest["role"], "verifier");
    assert_eq!(manifest["benchVersion"], set.bench_version);
    assert_eq!(manifest["benchHash"], "abc123");
    assert_eq!(manifest["run"], 1);
    assert_eq!(manifest["model"], "opus[1m]");
}

/// A path that climbs out of the scratch dir is not read. A rubric is
/// checked-in code, but it is not a reason to read the rest of the disk.
#[test]
fn a_criterion_cannot_read_outside_its_scratch_directory() {
    let scratch = tempfile::tempdir().expect("tempdir");
    std::fs::write(scratch.path().join("inside"), "here").expect("write");
    let criteria = vec![Criterion {
        id: "escape".to_owned(),
        traits: vec!["discipline".to_owned()],
        weight: 1,
        check: Check::FileContains(FileContainsCheck {
            path: "../../etc/hosts".to_owned(),
            text: "localhost".to_owned(),
        }),
    }];
    let graders = stage_graders(
        &bench_root().join("verifier/planted-defect/fixture"),
        &scratch.path().join(".graders"),
        &manifest(),
        "planted-defect",
    )
    .expect("graders");
    let artifacts = collect_artifacts(scratch.path(), &graders, &RawRun::default(), &criteria, 900)
        .expect("collect");
    assert!(artifacts.files.is_empty());
    let score = score_run(&criteria, &artifacts);
    assert_eq!(score.failed, vec!["escape".to_owned()]);
}

// ── fix round 1 ──────────────────────────────────────────────────────────────

/// **F1, the attack that made this fix round.** A stub adapter one line longer
/// than the honest one overwrites its own grader with `exit 0`. Before the fix
/// that took `judgment` from 2.6 to 5.0 with `outcome: passed` and zero spread.
/// The graders now live outside the directory the subject is handed, so the
/// overwrite scores **nothing**.
#[test]
fn a_subject_that_overwrites_its_own_grader_scores_nothing_by_it() {
    let mut runner = StubRunner::new(vec![
        (
            "report.md",
            "- `last_page` is off by one\n- `page_starts` looks wrong too\n",
        ),
        ("verdict.json", r#"{"verdict": "changes-requested"}"#),
    ]);
    // The exact cheat from REVIEW-L10 §5 F1.
    runner.sabotage = Some(("fixture/no-false-positives.sh", "#!/bin/sh\nexit 0\n"));

    let scratch = tempfile::tempdir().expect("tempdir");
    let (results, per_run) = run_bench(
        &runner,
        &plan_for(&descriptor(), "opus[1m]").expect("plan"),
        &verifier_tasks(),
        3,
        scratch.path(),
        Duration::from_secs(30),
        &manifest(),
    )
    .expect("runs");

    assert_eq!(results[0].outcome(), "failed", "the cheat must not pass");
    assert!(
        results[0]
            .score
            .failed
            .contains(&"reports-no-absent-defect".to_owned()),
        "the real grader must still run: {:?}",
        results[0].score.failed
    );
    let traits = aggregate_runs(&per_run);
    // The cheat scores exactly what an honest sloppy run scores — 2.6, not the
    // 5.0 the overwrite used to buy. `discipline` drops further (1.8) because
    // the `fixture/` directory the cheat had to create is itself a stray file.
    assert!(
        (traits["judgment"].score - 2.6).abs() < f64::EPSILON,
        "the cheat must not buy a 5.0: {:?}",
        traits["judgment"]
    );
    assert!(
        (traits["discipline"].score - 1.8).abs() < f64::EPSILON,
        "{:?}",
        traits["discipline"]
    );
}

/// The subject cannot reach the graders at all: they are staged outside its
/// scratch dir and every file in the staged copy is read-only.
#[test]
fn the_graders_are_staged_outside_the_scratch_dir_and_are_not_writable() {
    let scratch = tempfile::tempdir().expect("tempdir");
    let fixture = bench_root().join("verifier/planted-defect/fixture");
    let graders_dir = scratch.path().join(".graders/planted-defect-0");
    let answer_dir = scratch.path().join("planted-defect-0");
    stage_fixture(&fixture, &answer_dir).expect("stage");
    let graders =
        stage_graders(&fixture, &graders_dir, &manifest(), "planted-defect").expect("graders");

    // The answer directory holds the working files and NO `fixture/` copy.
    assert!(answer_dir.join("pager.rs").is_file());
    assert!(
        !answer_dir.join("fixture").exists(),
        "the graders must not be copied beside the answer"
    );
    // And the staged copy is not under the scratch dir the subject is handed.
    assert!(!graders.dir.starts_with(&answer_dir));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(graders.dir.join("no-false-positives.sh"))
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o222, 0, "no write bit anywhere: {mode:o}");
    }
}

/// The second defence, because the first one is a permission bit: every grader
/// is hashed against the bench manifest immediately before it runs, and a
/// mismatch refuses the RUN naming the file — it does not merely fail the
/// criterion, because a tampered grader means no number from the run is safe.
#[test]
fn a_grader_that_does_not_match_the_manifest_refuses_the_run_by_name() {
    let scratch = tempfile::tempdir().expect("tempdir");
    let fixture = bench_root().join("verifier/planted-defect/fixture");
    let graders_dir = scratch.path().join("graders");
    let graders =
        stage_graders(&fixture, &graders_dir, &manifest(), "planted-defect").expect("graders");

    // Clean, before anything moves.
    graders
        .verify("fixture/no-false-positives.sh")
        .expect("the shipped grader matches its manifest entry");

    // Now move it, the way a compromised host or a bad merge would.
    let target = graders_dir.join("no-false-positives.sh");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o644)).expect("chmod");
    }
    std::fs::write(&target, "#!/bin/sh\nexit 0\n").expect("tamper");

    let error = graders
        .verify("fixture/no-false-positives.sh")
        .expect_err("a moved grader must refuse the run");
    let text = error.to_string();
    assert!(text.contains("no-false-positives.sh"), "{text}");
    assert!(text.contains("does not match the bench manifest"), "{text}");

    // And a script the manifest has never heard of is refused too.
    let unknown = graders
        .verify("fixture/whatever.sh")
        .expect_err("must refuse");
    assert!(
        unknown.to_string().contains("not in the bench manifest"),
        "{unknown}"
    );
}

/// F9 — the applied `--task-timeout` reaches the scorer, so a finding names the
/// budget that was actually in force.
#[test]
fn the_applied_timeout_reaches_the_artifacts() {
    let mut runner = perfect_stub();
    runner.timed_out = true;
    let scratch = tempfile::tempdir().expect("tempdir");
    let (results, _) = run_bench(
        &runner,
        &plan_for(&descriptor(), "opus[1m]").expect("plan"),
        &verifier_tasks(),
        3,
        scratch.path(),
        Duration::from_secs(60),
        &manifest(),
    )
    .expect("runs");
    assert!(
        results[0].score.outcomes[0].detail.contains("60s budget"),
        "unexpected: {}",
        results[0].score.outcomes[0].detail
    );
}

/// F10/F11 — the two rubric criteria the review called out are now graders that
/// test the property they claim, not one hardcoded filename or one substring.
#[test]
fn a_report_that_clears_the_defect_does_not_pass_names_the_defect() {
    // Mentions `last_page` — and clears it. The old `fileContains` passed.
    let runner = StubRunner::new(vec![
        (
            "report.md",
            "- I checked `last_page` and it looks correct; no defects found.\n",
        ),
        ("verdict.json", r#"{"verdict": "approved"}"#),
    ]);
    let scratch = tempfile::tempdir().expect("tempdir");
    let (results, _) = run_bench(
        &runner,
        &plan_for(&descriptor(), "opus[1m]").expect("plan"),
        &verifier_tasks(),
        3,
        scratch.path(),
        Duration::from_secs(30),
        &manifest(),
    )
    .expect("runs");
    assert!(
        results[0]
            .score
            .failed
            .contains(&"names-the-defect".to_owned()),
        "a report that clears the code must not score the indictment: {:?}",
        results[0].score.failed
    );
}

#[test]
fn a_stray_file_of_any_name_fails_wrote_nothing_else() {
    let mut runner = perfect_stub();
    // Not `notes.md` — the old rubric's one hardcoded name.
    runner.sabotage = Some(("scratchpad.txt", "thinking out loud\n"));
    let scratch = tempfile::tempdir().expect("tempdir");
    let (results, _) = run_bench(
        &runner,
        &plan_for(&descriptor(), "opus[1m]").expect("plan"),
        &verifier_tasks(),
        3,
        scratch.path(),
        Duration::from_secs(30),
        &manifest(),
    )
    .expect("runs");
    assert!(
        results[0]
            .score
            .failed
            .contains(&"wrote-nothing-else".to_owned()),
        "any stray file counts, not one name: {:?}",
        results[0].score.failed
    );
}
