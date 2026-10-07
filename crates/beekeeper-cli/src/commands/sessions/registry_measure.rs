//! `bee sessions registry measure` — run a role's bench and leave a signed row
//! for every task before any score is written down.
//!
//! Finding 25 in one sentence: the router disclosed that an incumbent *"cleared
//! the verifier gates (reasoning≥4.5, judgment≥4.5, verification≥4.7) …
//! nothing else cleared them"*, and every number in that sentence was an
//! opinion. This command is how a number stops being one.
//!
//! # Nothing here rests on a local file
//!
//! `--channel` and `--session-ref` are required because the bench runs **inside
//! a real coding session**. Per task, per run, this publishes through the
//! existing verbs (`bee sessions observe`):
//!
//! * one 44246 **gate** — `gate = "registry-bench/<role>/<task-id>#<run>"`,
//!   `command` = the argv actually spawned, `outcome` `passed` only when every
//!   criterion passed, `summary` = `"<fraction> · failed: <criterion ids>"`,
//!   `durationMs` measured;
//! * one 44246 **finding** per failed criterion, with
//!   `findingId = "bench:<benchVersion>:<role>:<task-id>:<criterion-id>"`;
//! * one 44246 **checkpoint** per run carrying the bench manifest.
//!
//! **The checkpoint is a disclosed deviation from LANE-L10.md.** The spec's
//! two row kinds have nowhere to carry `benchVersion` and `benchHash` — the
//! gate string is capped at 64 bytes and its `summary` format is frozen — and
//! `registry propose` refuses a proposal whose bench hash moved. So one
//! `checkpoint` per run, `phase: gates`, carries the manifest as JSON in its
//! `note`. Without it `propose` would have to trust the proposing process's own
//! memory for the one field that proves the task set did not change under the
//! measurement, which is exactly the thing this lane exists to stop.
//!
//! # `source: measured`
//!
//! The rows this command publishes carry `source: "measured"`. That token was
//! a wire widening of L5's closed `observed | declared` enum — core, CLI and
//! the TypeScript decoder in one commit — so the **relay must carry the new
//! core before any client writes one**, and the app must be relaunched on the
//! new `bee`. An older relay refuses a row carrying it.
//!
//! Residual, named not fixed: `observed|declared` says *who saw it* and
//! `measured` says *why it ran*. Two axes, one key. And nothing checks who
//! signed a `measured` row — see the note in
//! `beekeeper-core/src/coding_session_observation_fold.rs`.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use beekeeper_core::coding_session_observation::CodingSessionObservationSource;
use beekeeper_core::coding_session_routing::{Registry, RegistryTarget};
use beekeeper_core::coding_session_runtime::{parse_runtime_descriptors, RuntimeDescriptor};
use beekeeper_core::registry_bench::{
    accumulate_totals, aggregate_runs, bench_finding_id, bench_gate_name, bench_hash,
    compare_to_minimums, parse_bench_set, parse_rubric, score_run, traits_from_totals,
    validate_finding_id_lengths, BenchSet, Criterion, MeasuredTrait, Rubric, RunScore,
    BENCH_MIN_REPEAT, BENCH_TASK_TIMEOUT_SECS, REGISTRY_BENCH_RELATIVE_PATH,
};

use crate::client::BeekeeperClient;
use crate::error::CliError;
use crate::validate::validate_uuid;
use crate::{
    SessionObserveCheckpointArgs, SessionObserveCmd, SessionObserveFindingArgs,
    SessionObserveGateArgs,
};

use super::observations::cmd_observe_as;
use super::registry::load_registry;

/// Where the harness leaves what it measured, inside each run's scratch dir.
pub const BENCH_ARTIFACT_DIR: &str = ".bench";

// ── loading the checked-in bench ─────────────────────────────────────────────

/// One task, loaded off disk.
#[derive(Debug, Clone)]
pub struct BenchTaskSpec {
    /// The task id, which is also its directory name.
    pub id: String,
    /// `task.md`, verbatim — the prompt the seat is given.
    pub prompt: String,
    /// Every criterion, in rubric order.
    pub criteria: Vec<Criterion>,
    /// The `fixture/` directory copied into each run's scratch dir.
    pub fixture_dir: PathBuf,
}

/// `team/registry-bench`, resolved beside the registry file itself.
///
/// The bench is a sibling of the registry it writes into, so a `--registry`
/// pointed at a checkout finds that checkout's bench and not this process's
/// working directory.
pub fn bench_root_for(registry_path: &Path) -> PathBuf {
    registry_path.parent().and_then(Path::parent).map_or_else(
        || PathBuf::from(REGISTRY_BENCH_RELATIVE_PATH),
        |root| root.join(REGISTRY_BENCH_RELATIVE_PATH),
    )
}

/// Every file under one role's bench directory, sorted by relative path.
///
/// # Errors
///
/// [`CliError::NotFound`] when the role has no directory, [`CliError::Other`]
/// on any read failure, naming the path.
pub fn read_role_bench_files(
    bench_root: &Path,
    role: &str,
) -> Result<BTreeMap<String, Vec<u8>>, CliError> {
    let dir = bench_root.join(role);
    if !dir.is_dir() {
        return Err(CliError::NotFound(format!(
            "no bench for role {role:?} at {}",
            dir.display()
        )));
    }
    let mut files = BTreeMap::new();
    let mut stack = vec![dir.clone()];
    while let Some(current) = stack.pop() {
        let entries = std::fs::read_dir(&current).map_err(|error| {
            CliError::Other(format!("cannot read {}: {error}", current.display()))
        })?;
        for entry in entries {
            let entry = entry.map_err(|error| {
                CliError::Other(format!("cannot read {}: {error}", current.display()))
            })?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let relative = path
                .strip_prefix(&dir)
                .map_err(|_| CliError::Other(format!("{} escaped its bench", path.display())))?
                .to_string_lossy()
                .replace('\\', "/");
            let bytes = std::fs::read(&path).map_err(|error| {
                CliError::Other(format!("cannot read {}: {error}", path.display()))
            })?;
            files.insert(relative, bytes);
        }
    }
    Ok(files)
}

/// sha256 over one role's whole bench directory, sorted by path.
///
/// # Errors
///
/// As [`read_role_bench_files`].
pub fn hash_role_bench(bench_root: &Path, role: &str) -> Result<String, CliError> {
    Ok(bench_hash(&read_role_bench_files(bench_root, role)?))
}

/// Load `bench.yaml` and every task it names.
///
/// # Errors
///
/// [`CliError::NotFound`] for a missing file, [`CliError::Usage`] for one that
/// is not this schema — naming the file both times.
pub fn load_role_bench(
    bench_root: &Path,
    role: &str,
) -> Result<(BenchSet, Vec<BenchTaskSpec>), CliError> {
    let dir = bench_root.join(role);
    let manifest = dir.join("bench.yaml");
    let text = std::fs::read_to_string(&manifest).map_err(|error| {
        CliError::NotFound(format!("cannot read {}: {error}", manifest.display()))
    })?;
    let set = parse_bench_set(&text)
        .map_err(|error| CliError::Usage(format!("{}: {error}", manifest.display())))?;
    if set.role != role {
        return Err(CliError::Usage(format!(
            "{} says role {:?} and lives in {role}/",
            manifest.display(),
            set.role
        )));
    }

    let mut tasks = Vec::with_capacity(set.tasks.len());
    for id in &set.tasks {
        let task_dir = dir.join(id);
        let prompt_path = task_dir.join("task.md");
        let rubric_path = task_dir.join("rubric.yaml");
        let prompt = std::fs::read_to_string(&prompt_path).map_err(|error| {
            CliError::NotFound(format!("cannot read {}: {error}", prompt_path.display()))
        })?;
        let rubric_text = std::fs::read_to_string(&rubric_path).map_err(|error| {
            CliError::NotFound(format!("cannot read {}: {error}", rubric_path.display()))
        })?;
        let rubric = parse_rubric(&rubric_text)
            .map_err(|error| CliError::Usage(format!("{}: {error}", rubric_path.display())))?;
        tasks.push(BenchTaskSpec {
            id: id.clone(),
            prompt,
            criteria: rubric.criteria,
            fixture_dir: task_dir.join("fixture"),
        });
    }
    Ok((set, tasks))
}

// ── the real spawn path ──────────────────────────────────────────────────────

/// The argv one run spawns, and the descriptor it came out of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnPlan {
    /// The runtime descriptor this run uses.
    pub instance_ref: String,
    /// The model id, which must be one the descriptor offers.
    pub model: String,
    /// The full argv, exactly as spawned and exactly as recorded in the gate
    /// row's `command`.
    pub argv: Vec<String>,
    /// Env pairs the descriptor asks for.
    pub env: Vec<(String, String)>,
}

impl SpawnPlan {
    /// The one string the 44246 gate row carries as `command`.
    pub fn command_line(&self) -> String {
        self.argv.join(" ")
    }
}

/// Resolve `--runtime` and `--model` against `BEEKEEPER_CSP_RUNTIMES`.
///
/// The runtime list is the same environment variable the desktop host writes
/// and the provider sidecar reads, so a bench run is aimed at a real
/// [`RuntimeDescriptor`] rather than at a name this command invented.
///
/// # Errors
///
/// [`CliError::Usage`] when the variable is unset or unparseable, when the
/// instance ref is not in it, or when the model is not one the descriptor
/// offers — listing what is on offer each time.
pub fn resolve_spawn_plan(instance_ref: &str, model: &str) -> Result<SpawnPlan, CliError> {
    let raw = std::env::var("BEEKEEPER_CSP_RUNTIMES").map_err(|_| {
        CliError::Usage(
            "BEEKEEPER_CSP_RUNTIMES is not set: a bench run is aimed at a real runtime descriptor, \
             not at a name. Export the same JSON the desktop host writes."
                .to_owned(),
        )
    })?;
    let descriptors = parse_runtime_descriptors(&raw)
        .map_err(|error| CliError::Usage(format!("BEEKEEPER_CSP_RUNTIMES: {error}")))?;
    let descriptor = descriptors
        .iter()
        .find(|candidate| candidate.instance_ref == instance_ref)
        .ok_or_else(|| {
            CliError::Usage(format!(
                "no runtime {instance_ref:?} in BEEKEEPER_CSP_RUNTIMES; it offers: {}",
                descriptors
                    .iter()
                    .map(|d| d.instance_ref.clone())
                    .collect::<Vec<String>>()
                    .join(", ")
            ))
        })?;
    plan_for(descriptor, model)
}

/// Build the argv for one descriptor and model.
///
/// # Errors
///
/// [`CliError::Usage`] when the descriptor does not offer the model.
pub fn plan_for(descriptor: &RuntimeDescriptor, model: &str) -> Result<SpawnPlan, CliError> {
    let offered: Vec<&str> = if descriptor.allowed_models.is_empty() {
        vec![descriptor.default_model.as_str()]
    } else {
        descriptor
            .allowed_models
            .iter()
            .map(String::as_str)
            .collect()
    };
    if !offered.contains(&model) {
        return Err(CliError::Usage(format!(
            "runtime {:?} does not offer model {model:?}; it offers: {}",
            descriptor.instance_ref,
            offered.join(", ")
        )));
    }
    let mut argv = vec![descriptor.agent_command.clone()];
    argv.extend(descriptor.agent_args.iter().cloned());
    argv.push("--model".to_owned());
    argv.push(model.to_owned());
    let env = descriptor
        .cli_env
        .as_ref()
        .map(|pair| vec![(pair.name.clone(), pair.value.clone())])
        .unwrap_or_default();
    Ok(SpawnPlan {
        instance_ref: descriptor.instance_ref.clone(),
        model: model.to_owned(),
        argv,
        env,
    })
}

/// What one spawn returned, before any probe is run and before any scoring.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RawRun {
    /// The adapter's exit status, or `None` when it was killed.
    pub exit_code: Option<i32>,
    /// Everything it wrote to stdout.
    pub stdout: String,
    /// `true` when it passed `--task-timeout`.
    pub timed_out: bool,
    /// Wall-clock duration, in milliseconds.
    pub duration_ms: u64,
}

/// What actually runs a task.
///
/// A trait, and not a function, for one reason: the harness must be provable
/// without a model. Tests use a stub that returns fixed artifacts, and the
/// real implementation spawns the descriptor's own argv.
pub trait BenchRunner {
    /// Run one task in `scratch`, with `prompt` on stdin.
    ///
    /// # Errors
    ///
    /// [`CliError`] only for a failure to *start* — a task that runs and fails
    /// is a [`RawRun`], never an error, because a failure is a result.
    fn run(
        &self,
        plan: &SpawnPlan,
        prompt: &str,
        scratch: &Path,
        timeout: Duration,
    ) -> Result<RawRun, CliError>;
}

/// Spawns the descriptor's own argv, with the prompt on stdin and stdout
/// redirected into `.bench/stdout`.
///
/// Redirected rather than piped on purpose: a piped child that outruns the
/// reader deadlocks, and the file is one of the artifacts the rubric is
/// allowed to read anyway.
#[derive(Debug, Clone, Copy, Default)]
pub struct AdapterRunner;

impl BenchRunner for AdapterRunner {
    fn run(
        &self,
        plan: &SpawnPlan,
        prompt: &str,
        scratch: &Path,
        timeout: Duration,
    ) -> Result<RawRun, CliError> {
        let artifacts = scratch.join(BENCH_ARTIFACT_DIR);
        std::fs::create_dir_all(&artifacts).map_err(|error| {
            CliError::Other(format!("cannot create {}: {error}", artifacts.display()))
        })?;
        let stdout_path = artifacts.join("stdout");
        let stdout_file = std::fs::File::create(&stdout_path).map_err(|error| {
            CliError::Other(format!("cannot create {}: {error}", stdout_path.display()))
        })?;
        let stderr_file = stdout_file
            .try_clone()
            .map_err(|error| CliError::Other(format!("cannot clone stdout handle: {error}")))?;

        let Some((command, args)) = plan.argv.split_first() else {
            return Err(CliError::Usage("the spawn plan has no argv".to_owned()));
        };
        let mut spawned = std::process::Command::new(command);
        spawned
            .args(args)
            .current_dir(scratch)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::from(stdout_file))
            .stderr(std::process::Stdio::from(stderr_file));
        for (key, value) in &plan.env {
            spawned.env(key, value);
        }
        let started = Instant::now();
        let mut child = spawned.spawn().map_err(|error| {
            CliError::Other(format!("cannot spawn {}: {error}", plan.command_line()))
        })?;
        if let Some(mut stdin) = child.stdin.take() {
            // A closed stdin on the far side is not this harness's failure to
            // report; the run's own result says what happened.
            let _ = stdin.write_all(prompt.as_bytes());
        }

        let mut timed_out = false;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) => {
                    if started.elapsed() >= timeout {
                        let _ = child.kill();
                        let _ = child.wait();
                        timed_out = true;
                        break None;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(error) => {
                    return Err(CliError::Other(format!(
                        "cannot wait on the adapter: {error}"
                    )))
                }
            }
        };

        let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let stdout = std::fs::read_to_string(&stdout_path).unwrap_or_default();
        let _ = std::fs::write(
            artifacts.join("exit-code"),
            status
                .and_then(|s| s.code())
                .map_or_else(|| "none".to_owned(), |code| code.to_string()),
        );
        let _ = std::fs::write(artifacts.join("duration-ms"), duration_ms.to_string());
        Ok(RawRun {
            exit_code: status.and_then(|status| status.code()),
            stdout,
            timed_out,
            duration_ms,
        })
    }
}

// ── staging, probing, and the pure/impure boundary ───────────────────────────

#[path = "registry_measure_graders.rs"]
mod graders;
pub use graders::*;

// ── the command ──────────────────────────────────────────────────────────────

/// Every task-run's row, and the per-run trait scores the medians are taken
/// over.
///
/// The two halves answer different questions and are deliberately not merged:
/// the first is what gets published as signed rows, the second is what gets
/// aggregated into a proposal.
pub type BenchOutcome = (Vec<TaskRunResult>, Vec<BTreeMap<String, f64>>);

/// One task-run, scored, with everything a row needs.
#[derive(Debug, Clone)]
pub struct TaskRunResult {
    /// Task id.
    pub task_id: String,
    /// Run index, 0-based.
    pub run: u32,
    /// The scored result.
    pub score: RunScore,
    /// The argv this run spawned, verbatim.
    pub command: String,
    /// The harness's own measurement.
    pub duration_ms: u64,
}

impl TaskRunResult {
    /// `passed` only when every criterion passed.
    pub fn outcome(&self) -> &'static str {
        if self.score.all_passed() {
            "passed"
        } else {
            "failed"
        }
    }

    /// `"<fraction> · failed: <criterion ids>"`, exactly as LANE-L10.md pins it.
    pub fn summary(&self) -> String {
        format!(
            "{} · failed: {}",
            self.score.fraction(),
            if self.score.failed.is_empty() {
                "none".to_owned()
            } else {
                self.score.failed.join(", ")
            }
        )
    }
}

/// Run every task `repeat` times and fold the per-run trait scores.
///
/// Returns the per-run results in order and the per-run trait maps the
/// aggregate is taken over.
///
/// # Errors
///
/// Only a failure to stage or start — a task that runs and fails is a result.
#[allow(clippy::too_many_arguments)]
pub fn run_bench(
    runner: &dyn BenchRunner,
    plan: &SpawnPlan,
    tasks: &[BenchTaskSpec],
    repeat: u32,
    scratch_root: &Path,
    timeout: Duration,
    manifest: &BTreeMap<String, Vec<u8>>,
) -> Result<BenchOutcome, CliError> {
    let mut results = Vec::new();
    let mut per_run: Vec<BTreeMap<String, f64>> = Vec::new();
    for run in 0..repeat {
        // One run is one pass over the whole set: a trait's raw fraction is
        // taken across every criterion in the set that tags it, so a
        // one-criterion task cannot weigh as much as a ten-criterion one.
        let mut totals: BTreeMap<String, (u32, u32)> = BTreeMap::new();
        for task in tasks {
            let scratch = scratch_root.join(format!("{}-{run}", task.id));
            // Two directories, and the separation is the point: the subject
            // gets `scratch` as its cwd, and the graders live in `graders`,
            // which it is never handed and cannot write.
            let graders_dir = scratch_root.join(format!(".graders/{}-{run}", task.id));
            stage_fixture(&task.fixture_dir, &scratch)?;
            let graders = stage_graders(&task.fixture_dir, &graders_dir, manifest, &task.id)?;
            let raw = runner.run(plan, &task.prompt, &scratch, timeout)?;
            let artifacts =
                collect_artifacts(&scratch, &graders, &raw, &task.criteria, timeout.as_secs())?;
            let score = score_run(&task.criteria, &artifacts);
            // Partial credit, and the weights compose within a task as well as
            // across tasks (Brian's ruling, fix round 1). `measure` and
            // `propose` read the SAME number from the same rows because both
            // go through this one scorer and its summary: a row whose
            // `outcome` and summary contradict each other is refused by name
            // (`gate_row_refusals`), never coerced into agreement.
            accumulate_totals(&mut totals, &task.criteria, &score);
            results.push(TaskRunResult {
                task_id: task.id.clone(),
                run,
                score,
                command: plan.command_line(),
                duration_ms: raw.duration_ms,
            });
        }
        per_run.push(traits_from_totals(&totals));
    }
    Ok((results, per_run))
}

/// The manifest one run publishes so `propose` can check the bench did not move.
pub fn run_manifest(
    role: &str,
    set: &BenchSet,
    bench_hash: &str,
    plan: &SpawnPlan,
    repeat: u32,
    run: u32,
) -> Value {
    json!({
        "kind": "registry-bench-manifest",
        "role": role,
        "benchVersion": set.bench_version,
        "benchHash": bench_hash,
        "runtime": plan.instance_ref,
        "model": plan.model,
        "repeat": repeat,
        "run": run,
        "taskTimeoutSecs": BENCH_TASK_TIMEOUT_SECS,
    })
}

/// `bee sessions registry measure`.
///
/// # Errors
///
/// [`CliError::Usage`] for a bad flag, an unknown runtime or a stub role;
/// [`CliError::NotFound`] for a missing bench; [`CliError::Other`] for a run
/// that could not be staged or started; [`CliError::Auth`] when `client` is
/// `None` and this is not a dry run.
///
/// `client` is `None` exactly on the keyless `--dry-run` path: scoring a bench
/// signs nothing, so it must not demand an identity.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub async fn cmd_registry_measure(
    client: Option<&BeekeeperClient>,
    role: &str,
    runtime: &str,
    model: &str,
    channel: &str,
    session_ref: &str,
    repeat: u32,
    registry_path: Option<&str>,
    task_timeout_secs: u64,
    dry_run: bool,
    format: &crate::OutputFormat,
) -> Result<(), CliError> {
    validate_uuid(channel)?;
    validate_uuid(session_ref)?;
    if repeat < BENCH_MIN_REPEAT {
        return Err(CliError::Usage(format!(
            "--repeat {repeat} is below the minimum of {BENCH_MIN_REPEAT}: fewer than three runs \
             cannot produce a median that means anything"
        )));
    }
    let (path, registry) = load_registry(registry_path)?;
    let bench_root = bench_root_for(&path);
    let (set, tasks) = load_role_bench(&bench_root, role)?;
    if set.is_stub() {
        return Err(CliError::Usage(format!(
            "the bench for {role} has no tasks: there is nothing to measure, and a row measured \
             by nothing is not a measured row"
        )));
    }
    let hash = hash_role_bench(&bench_root, role)?;
    let plan = resolve_spawn_plan(runtime, model)?;
    // F14 — a rubric whose criterion ids would mint an over-length `findingId`
    // is refused BEFORE a model runs, not after three runs have been paid for.
    for task in &tasks {
        validate_finding_id_lengths(
            role,
            &task.id,
            set.bench_version,
            &Rubric {
                criteria: task.criteria.clone(),
            },
        )
        .map_err(|error| CliError::Usage(format!("{}/{}: {error}", role, task.id)))?;
    }

    let scratch_root =
        std::env::temp_dir().join(format!("buzz-registry-bench-{role}-{}", std::process::id()));
    let manifest = read_role_bench_files(&bench_root, role)?;
    let (results, per_run) = run_bench(
        &AdapterRunner,
        &plan,
        &tasks,
        repeat,
        &scratch_root,
        Duration::from_secs(task_timeout_secs),
        &manifest,
    )?;
    let traits = aggregate_runs(&per_run);

    // The class gate this row will be read by, with the measurement beside each
    // ratified minimum. Brian's addendum: the bar is the bar, and this is the
    // table it can be re-derived from later with numbers in hand.
    let minimums = registry
        .classes
        .get(role)
        .map(|class| class.minimums.clone())
        .unwrap_or_default();
    let comparison: Vec<Value> = compare_to_minimums(&minimums, &traits)
        .into_iter()
        .map(|row| {
            json!({
                "trait": row.trait_name,
                "minimum": row.minimum,
                "measured": row.measured,
                "clears": row.clears(),
            })
        })
        .collect();

    let report = json!({
        "role": role,
        "benchVersion": set.bench_version,
        "benchHash": hash,
        "runtime": plan.instance_ref,
        "model": plan.model,
        "command": plan.command_line(),
        "repeat": repeat,
        "dryRun": dry_run,
        "scratch": scratch_root.display().to_string(),
        "runs": results
            .iter()
            .map(|result| json!({
                "gate": bench_gate_name(role, &result.task_id, result.run),
                "outcome": result.outcome(),
                "summary": result.summary(),
                "durationMs": result.duration_ms,
                "failed": result.score.failed,
            }))
            .collect::<Vec<Value>>(),
        "traits": traits_json(&traits),
        // Ratified minimums, and what this run actually measured beside them.
        "againstClassMinimums": comparison,
        "incumbentScores": incumbent_scores(&registry, role, &plan),
        "note": if dry_run {
            "DRY RUN: nothing was published. The argv above is the real spawn plan."
        } else {
            "every row below was published as a signed 44246 observation before this table \
             was printed"
        },
    });

    if !dry_run {
        // `--dry-run` publishes nothing, so it needs no identity — and needing
        // one was the reason a bench could not be sanity-checked on a machine
        // without a key (F16). Every other path signs, and says so plainly
        // rather than failing later inside the publish loop.
        let client = client.ok_or_else(|| {
            CliError::Auth(
                "publishing bench rows requires BEEKEEPER_PRIVATE_KEY (use --dry-run to score without \
                 publishing)"
                    .into(),
            )
        })?;
        publish_rows(
            client,
            channel,
            session_ref,
            role,
            &set,
            &hash,
            &plan,
            repeat,
            &results,
        )
        .await?;
    }

    match format {
        crate::OutputFormat::Compact => println!(
            "{}",
            json!({
                "role": role,
                "benchVersion": set.bench_version,
                "traits": traits_json(&traits),
                "dryRun": dry_run,
            })
        ),
        crate::OutputFormat::Json => println!("{report}"),
    }
    Ok(())
}

fn traits_json(traits: &BTreeMap<String, MeasuredTrait>) -> Value {
    traits
        .iter()
        .map(|(name, measured)| {
            (
                name.clone(),
                json!({
                    "score": measured.score,
                    "n": measured.n,
                    "min": measured.min,
                    "max": measured.max,
                    "spread": measured.spread(),
                    "unstable": measured.is_unstable(),
                }),
            )
        })
        .collect::<serde_json::Map<String, Value>>()
        .into()
}

/// The row this bench is measuring, as the registry describes it today — so a
/// reader sees the opinion and the measurement side by side rather than one
/// replacing the other silently.
fn incumbent_scores(registry: &Registry, role: &str, plan: &SpawnPlan) -> Value {
    let row: Option<&RegistryTarget> = registry.targets.iter().find(|target| {
        target.provider == plan.instance_ref
            && beekeeper_core::coding_session_routing::base_id(&target.model)
                == beekeeper_core::coding_session_routing::base_id(&plan.model)
    });
    row.map_or(Value::Null, |row| {
        json!({
            "target": row.label(),
            "standingFor": row.status.get(role),
            "priors": row.scores,
            "rating": {
                "status": row.rating.status,
                "confidence": row.rating.confidence,
                "author": row.rating.author,
                "date": row.rating.date,
            },
            "alreadyMeasured": row.is_measured(),
        })
    })
}

#[allow(clippy::too_many_arguments)]
async fn publish_rows(
    client: &BeekeeperClient,
    channel: &str,
    session_ref: &str,
    role: &str,
    set: &BenchSet,
    hash: &str,
    plan: &SpawnPlan,
    repeat: u32,
    results: &[TaskRunResult],
) -> Result<(), CliError> {
    let mut manifests_published: Vec<u32> = Vec::new();
    for result in results {
        if !manifests_published.contains(&result.run) {
            manifests_published.push(result.run);
            cmd_observe_as(
                client,
                SessionObserveCmd::Checkpoint(SessionObserveCheckpointArgs {
                    channel: channel.to_owned(),
                    session_ref: session_ref.to_owned(),
                    genesis: None,
                    assignment: None,
                    phase: "gates".to_owned(),
                    tests_written: 0,
                    tests_red: 0,
                    tests_green: 0,
                    last_command: Some(plan.command_line()),
                    last_summary: Some(format!(
                        "registry-bench/{role} v{} run {}",
                        set.bench_version, result.run
                    )),
                    note: Some(run_manifest(role, set, hash, plan, repeat, result.run).to_string()),
                }),
                CodingSessionObservationSource::Measured,
            )
            .await?;
        }

        cmd_observe_as(
            client,
            SessionObserveCmd::Gate(SessionObserveGateArgs {
                channel: channel.to_owned(),
                session_ref: session_ref.to_owned(),
                genesis: None,
                assignment: None,
                gate: vec![format!(
                    "{}:{}:{}",
                    bench_gate_name(role, &result.task_id, result.run),
                    result.outcome(),
                    result.command
                )],
                summary: vec![result.summary()],
                duration_ms: vec![result.duration_ms],
                // The bench scores a fixed task set, not a checkout: there is
                // no commit these rows are about, and naming one would be an
                // invention.
                head_sha: Vec::new(),
            }),
            CodingSessionObservationSource::Measured,
        )
        .await?;

        for criterion in &result.score.failed {
            cmd_observe_as(
                client,
                SessionObserveCmd::Finding(SessionObserveFindingArgs {
                    channel: channel.to_owned(),
                    session_ref: session_ref.to_owned(),
                    genesis: None,
                    assignment: None,
                    finding_id: bench_finding_id(
                        set.bench_version,
                        role,
                        &result.task_id,
                        criterion,
                    ),
                    title: format!(
                        "{} failed {criterion} on run {}",
                        result.task_id, result.run
                    ),
                    disposition: "found".to_owned(),
                    detail: Some(
                        result
                            .score
                            .outcomes
                            .iter()
                            .find(|outcome| outcome.id == *criterion)
                            .map_or_else(String::new, |outcome| outcome.detail.clone()),
                    ),
                    reference: Vec::new(),
                    decision: None,
                }),
                CodingSessionObservationSource::Measured,
            )
            .await?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "registry_measure_tests.rs"]
mod registry_measure_tests;
