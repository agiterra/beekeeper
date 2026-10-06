//! The registry bench: how a registry row earns a number instead of holding an
//! opinion.
//!
//! `team/model-registry.yaml` ships ten 1–5 priors per row under
//! `rating: { status: operational_opinion, confidence: low }`. Live run 3
//! (finding 25) is what those priors cost: the router disclosed that an
//! incumbent *"cleared the verifier gates (reasoning≥4.5, judgment≥4.5,
//! verification≥4.7) … nothing else cleared them"* while a Codex target sat on
//! the same bench — and every number in that sentence was somebody's guess.
//! **The rule this module implements: a row carries MEASURED scores or none.
//! Nothing here invents a number.**
//!
//! # Why the scoring is mechanical, and why that is the whole point
//!
//! A model judging a model puts the opinion straight back into the number the
//! bench exists to remove, and it makes two runs of the same artifacts
//! disagree. So a criterion's [`Check`] is one of a **closed** set read off a
//! run's artifacts alone, and [`score_run`] is a pure function of
//! [`RunArtifacts`]: same artifacts in, same decimal out, every time.
//!
//! Two of the seven checks cannot be answered by reading bytes — `command`
//! runs a script and `diffApplies` asks git — so the **harness** runs them and
//! records the answer in [`RunArtifacts::probes`] before scoring starts. A
//! criterion whose probe is missing **fails, by name**: it never passes on the
//! absence of evidence.
//!
//! # The arithmetic, in one place
//!
//! Per run, per trait — `raw` is the passed weight over the offered weight of
//! every criterion tagged with the trait, and
//! `score = round1(1.0 + 4.0 × raw)`, half-up, on the registry's 1–5 scale.
//!
//! A trait no criterion tags gets **no score** — never a zero. The row value is
//! the **median** over `--repeat` runs (default and minimum
//! [`BENCH_MIN_REPEAT`]), reported with `n`, `min` and `max`. The model is not
//! deterministic and this does not pretend otherwise: a spread of
//! [`BENCH_UNSTABLE_SPREAD`] or more on any trait marks the set **unstable**,
//! and a proposal built on it is refused.
//!
//! **No I/O here.** This module never reads a directory, spawns a process or
//! talks to a relay; it takes the bytes the CLI hands it, which is what lets
//! the scorer be tested without a model, a relay or a clock.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// Minimum — and default — number of runs behind a proposed row.
///
/// Fewer than three cannot produce a median that means anything, and `propose`
/// refuses a task with fewer.
pub const BENCH_MIN_REPEAT: u32 = 3;

/// A per-trait spread (`max − min`) at or above this marks the set unstable.
pub const BENCH_UNSTABLE_SPREAD: f64 = 1.0;

/// Wall-clock budget for one task, in seconds. Past it the task fails every
/// criterion: a timeout is a result, not a missing run.
pub const BENCH_TASK_TIMEOUT_SECS: u64 = 900;

/// Where the checked-in task sets live, relative to the repository root.
pub const REGISTRY_BENCH_RELATIVE_PATH: &str = "team/registry-bench";

/// Days a legacy row keeps routing after a bench exists for its class.
///
/// Brian's addendum of 2026-09-01: an unmeasured row routes until a bench
/// exists for its class; from that day it has thirty days, disclosed in every
/// routing record, and then `route` refuses it with the word `unmeasured`.
pub const LEGACY_ROW_GRACE_DAYS: i64 = 30;

/// Largest criterion weight the rubric may state.
pub const MAX_CRITERION_WEIGHT: u32 = 10;

// ── the checked-in bench ─────────────────────────────────────────────────────
/// `team/registry-bench/<role>/bench.yaml` — one role's ordered task set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BenchSet {
    /// The registry class this set measures, e.g. `verifier`.
    pub role: String,
    /// Bumped by hand on any change to the set. A proposal names the version
    /// its rows were produced under.
    pub bench_version: u32,
    /// Task ids, in the order they run. Empty means "no bench for this role
    /// yet", and `propose` refuses such a role rather than proposing a row
    /// measured by nothing.
    #[serde(default)]
    pub tasks: Vec<String>,
    /// One sentence on what this set claims to measure and what it does not.
    #[serde(default)]
    pub note: Option<String>,
}

impl BenchSet {
    /// `true` when this role ships as a stub with no tasks.
    pub fn is_stub(&self) -> bool {
        self.tasks.is_empty()
    }
}

/// `team/registry-bench/<role>/<task-id>/rubric.yaml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Rubric {
    /// Every criterion this task scores. Order is display order only.
    pub criteria: Vec<Criterion>,
}

/// One scored criterion: what it measures, how much it counts, and the one
/// mechanical question that decides it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Criterion {
    /// Unique within the task; it names the criterion in a failure and in the
    /// `findingId` published for it.
    pub id: String,
    /// Registry traits this is evidence for. A criterion may tag more than
    /// one; a trait no criterion tags gets no score at all.
    pub traits: Vec<String>,
    /// 1..=[`MAX_CRITERION_WEIGHT`], relative within a trait.
    pub weight: u32,
    /// The one mechanical question.
    pub check: Check,
}

/// The closed set of questions a criterion may ask of a run's artifacts.
///
/// **No model judges anything in v1.** A grader would put this lane's opinion
/// straight back into the number it exists to remove.
///
/// Externally tagged on purpose, which in YAML is a tag —
/// `check: !fileContains` followed by that variant's own fields. An unknown
/// tag is an unknown variant and serde refuses it **by name**, and every
/// variant's body is `deny_unknown_fields`, so a rubric can neither grow an
/// eighth kind of check nobody implemented nor carry a misspelt key that is
/// silently ignored.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Check {
    /// The adapter's own exit status.
    ExitCode(ExitCodeCheck),
    /// A regex over everything the run wrote to stdout.
    StdoutMatches(StdoutMatchesCheck),
    /// A file in the scratch directory contains a literal string.
    FileContains(FileContainsCheck),
    /// A file is **not** present. This is how "did not report a defect that is
    /// not there" is scored.
    FileAbsent(FileAbsentCheck),
    /// A JSON pointer into a file parses and equals a value.
    JsonPathEquals(JsonPathEqualsCheck),
    /// A file is a patch that applies to the fixture. Answered by the harness
    /// (a probe), because it needs git.
    DiffApplies(DiffAppliesCheck),
    /// A script in `fixture/` exits 0. Answered by the harness (a probe).
    Command(CommandCheck),
}

impl Check {
    /// The wire word for this kind of check, used in refusals and reports.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::ExitCode(_) => "exitCode",
            Self::StdoutMatches(_) => "stdoutMatches",
            Self::FileContains(_) => "fileContains",
            Self::FileAbsent(_) => "fileAbsent",
            Self::JsonPathEquals(_) => "jsonPathEquals",
            Self::DiffApplies(_) => "diffApplies",
            Self::Command(_) => "command",
        }
    }

    /// `true` when the harness must answer this check before scoring, because
    /// it cannot be decided by reading the artifacts.
    pub fn needs_probe(&self) -> bool {
        matches!(self, Self::DiffApplies(_) | Self::Command(_))
    }
}

/// See [`Check::ExitCode`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExitCodeCheck {
    /// The status the run must have exited with.
    pub equals: i32,
}

/// See [`Check::StdoutMatches`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StdoutMatchesCheck {
    /// A Rust `regex` pattern. One that does not compile **fails** the
    /// criterion and says so; it never passes and never panics.
    pub pattern: String,
}

/// See [`Check::FileContains`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FileContainsCheck {
    /// Path relative to the run's scratch directory.
    pub path: String,
    /// Literal substring, never a pattern.
    pub text: String,
}

/// See [`Check::FileAbsent`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FileAbsentCheck {
    /// Path relative to the run's scratch directory.
    pub path: String,
}

/// See [`Check::JsonPathEquals`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JsonPathEqualsCheck {
    /// Path relative to the run's scratch directory.
    pub path: String,
    /// RFC 6901 JSON pointer, e.g. `/defects/0/id`.
    pub pointer: String,
    /// The value that pointer must equal.
    pub equals: Value,
}

/// See [`Check::DiffApplies`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DiffAppliesCheck {
    /// Path to the patch, relative to the run's scratch directory.
    pub path: String,
}

/// See [`Check::Command`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommandCheck {
    /// Script path relative to the run's scratch directory. Exit 0 = pass.
    pub script: String,
}

/// Why a bench set or rubric was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BenchParseError {
    /// Not YAML, or not this schema's shape. Carries serde's own message.
    Malformed(String),
    /// A structural rule was broken; the string names which.
    NotCanonical(String),
}

impl std::fmt::Display for BenchParseError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Malformed(detail) => write!(formatter, "malformed bench file: {detail}"),
            Self::NotCanonical(detail) => {
                write!(formatter, "bench file is not canonical: {detail}")
            }
        }
    }
}

impl std::error::Error for BenchParseError {}

/// Parse a `bench.yaml`.
///
/// # Errors
///
/// [`BenchParseError`] when the file is not this shape, `role` is empty,
/// `benchVersion` is zero, or a task id repeats or is not a safe segment.
pub fn parse_bench_set(text: &str) -> Result<BenchSet, BenchParseError> {
    let set: BenchSet = serde_yaml::from_str(text)
        .map_err(|error| BenchParseError::Malformed(error.to_string()))?;
    if set.role.trim().is_empty() {
        return Err(BenchParseError::NotCanonical("role is empty".to_owned()));
    }
    if set.bench_version == 0 {
        return Err(BenchParseError::NotCanonical(
            "benchVersion 0 is not a version; a set starts at 1".to_owned(),
        ));
    }
    let mut seen = BTreeSet::new();
    for task in &set.tasks {
        if !is_safe_segment(task) {
            return Err(BenchParseError::NotCanonical(format!(
                "task id {task:?} is not a single safe path segment"
            )));
        }
        if !seen.insert(task.clone()) {
            return Err(BenchParseError::NotCanonical(format!(
                "task id {task:?} appears twice; the set is an ordered list of distinct tasks"
            )));
        }
    }
    Ok(set)
}

/// Parse a `rubric.yaml`.
///
/// # Errors
///
/// [`BenchParseError`] when the file is not this shape, scores nothing, a
/// criterion id repeats, a weight is outside `1..=`[`MAX_CRITERION_WEIGHT`],
/// or a criterion tags no trait.
pub fn parse_rubric(text: &str) -> Result<Rubric, BenchParseError> {
    let rubric: Rubric = serde_yaml::from_str(text)
        .map_err(|error| BenchParseError::Malformed(error.to_string()))?;
    if rubric.criteria.is_empty() {
        return Err(BenchParseError::NotCanonical(
            "a rubric with no criteria scores nothing; delete the task or write one".to_owned(),
        ));
    }
    let mut seen = BTreeSet::new();
    for criterion in &rubric.criteria {
        if criterion.id.trim().is_empty() {
            return Err(BenchParseError::NotCanonical(
                "a criterion has an empty id".to_owned(),
            ));
        }
        if !seen.insert(criterion.id.clone()) {
            return Err(BenchParseError::NotCanonical(format!(
                "criterion id {:?} appears twice",
                criterion.id
            )));
        }
        if criterion.weight == 0 || criterion.weight > MAX_CRITERION_WEIGHT {
            return Err(BenchParseError::NotCanonical(format!(
                "criterion {:?} has weight {}; the range is 1..={MAX_CRITERION_WEIGHT}",
                criterion.id, criterion.weight
            )));
        }
        if criterion.traits.is_empty() {
            return Err(BenchParseError::NotCanonical(format!(
                "criterion {:?} tags no trait, so nothing it proves would reach a row",
                criterion.id
            )));
        }
    }
    Ok(rubric)
}

/// Largest `findingId` the observation validator accepts, in bytes.
///
/// Mirrors `MAX_OBSERVATION_NAME_BYTES` in
/// [`crate::coding_session_observation`]. Duplicated as a constant rather than
/// imported so this module keeps its "no dependency on the wire types" shape;
/// [`registry_bench_tests`] asserts the two agree.
pub const MAX_BENCH_FINDING_ID_BYTES: usize = 64;

/// Refuse a rubric whose criterion ids would mint a `findingId` past the cap.
///
/// F14: the length was only discovered at publish time — **after** the model
/// had already run three times. A rubric that cannot report its own failures
/// is refused before anything is spawned.
///
/// # Errors
///
/// [`BenchParseError::NotCanonical`] naming the criterion and the length.
pub fn validate_finding_id_lengths(
    role: &str,
    task_id: &str,
    bench_version: u32,
    rubric: &Rubric,
) -> Result<(), BenchParseError> {
    for criterion in &rubric.criteria {
        let id = bench_finding_id(bench_version, role, task_id, &criterion.id);
        if id.len() > MAX_BENCH_FINDING_ID_BYTES {
            return Err(BenchParseError::NotCanonical(format!(
                "criterion {:?} mints findingId {id:?}, {} bytes against the \
                 {MAX_BENCH_FINDING_ID_BYTES}-byte cap: a rubric that cannot report its own \
                 failure is refused before a model runs, not after",
                criterion.id,
                id.len()
            )));
        }
    }
    Ok(())
}

fn is_safe_segment(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

// ── one run's artifacts ──────────────────────────────────────────────────────
/// Everything one task-run left behind, and the only thing the scorer reads.
/// The harness fills it; the scorer touches no disk and no process, which is
/// what makes two scorings of one run agree by construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunArtifacts {
    /// The adapter's exit status, or `None` when it was killed or reported
    /// none. `None` never satisfies an `exitCode` check.
    pub exit_code: Option<i32>,
    /// Everything the run wrote to stdout.
    pub stdout: String,
    /// Scratch-directory contents, keyed by relative path. A path absent from
    /// this map is a file that was not there.
    pub files: BTreeMap<String, String>,
    /// Answers the harness computed for the checks that need a process, keyed
    /// by **criterion id**. A missing probe fails its criterion, by name.
    pub probes: BTreeMap<String, bool>,
    /// `true` when the task passed its timeout. Every criterion then fails.
    pub timed_out: bool,
    /// Wall-clock duration the harness measured, in milliseconds.
    pub duration_ms: u64,
    /// The budget that was actually applied to this run, in seconds.
    ///
    /// `--task-timeout` is a flag, so the constant is not the number a reader
    /// needs: a finding that names a 900 s budget after a 60 s run names a
    /// budget that was never in force.
    pub timeout_secs: u64,
}

impl Default for RunArtifacts {
    fn default() -> Self {
        Self {
            exit_code: None,
            stdout: String::new(),
            files: BTreeMap::new(),
            probes: BTreeMap::new(),
            timed_out: false,
            duration_ms: 0,
            timeout_secs: BENCH_TASK_TIMEOUT_SECS,
        }
    }
}

/// One criterion's verdict and the sentence explaining it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CriterionOutcome {
    /// The criterion's id.
    pub id: String,
    /// `true` when it passed.
    pub passed: bool,
    /// Why — always populated, for a pass and for a failure alike.
    pub detail: String,
}

/// One task, scored.
#[derive(Debug, Clone, PartialEq)]
pub struct RunScore {
    /// Every criterion's verdict, in rubric order.
    pub outcomes: Vec<CriterionOutcome>,
    /// Trait → this run's 1–5 score. A trait nothing tagged is absent, never
    /// zero.
    pub traits: BTreeMap<String, f64>,
    /// Ids of the criteria that failed, in order.
    pub failed: Vec<String>,
    /// Passed weight, for the `<fraction>` in a gate row's summary.
    pub weight_passed: u32,
    /// Offered weight. See [`Self::weight_passed`].
    pub weight_total: u32,
}

impl RunScore {
    /// `"<passed>/<total>"` — the fraction a gate row's summary opens with.
    pub fn fraction(&self) -> String {
        format!("{}/{}", self.weight_passed, self.weight_total)
    }

    /// `true` when every criterion passed. The one thing that makes a gate
    /// row's `outcome` `passed`.
    pub fn all_passed(&self) -> bool {
        self.failed.is_empty()
    }
}

/// Round half-up to one decimal, on the registry's 1–5 scale.
pub fn round1(value: f64) -> f64 {
    (value * 10.0 + 0.5).floor() / 10.0
}

/// Score one run's artifacts against a set of criteria. Pure: the same inputs
/// always give the same [`RunScore`], which is the property that lets a row
/// claim a measurement at all.
pub fn score_run(criteria: &[Criterion], artifacts: &RunArtifacts) -> RunScore {
    let mut outcomes = Vec::with_capacity(criteria.len());
    let mut failed = Vec::new();
    // trait → (passed weight, total weight)
    let mut totals: BTreeMap<String, (u32, u32)> = BTreeMap::new();
    let mut weight_passed = 0_u32;
    let mut weight_total = 0_u32;

    for criterion in criteria {
        let (passed, detail) = if artifacts.timed_out {
            (
                false,
                format!(
                    "the task passed its {}s budget, so every criterion fails",
                    artifacts.timeout_secs
                ),
            )
        } else {
            evaluate(criterion, artifacts)
        };
        weight_total = weight_total.saturating_add(criterion.weight);
        if passed {
            weight_passed = weight_passed.saturating_add(criterion.weight);
        } else {
            failed.push(criterion.id.clone());
        }
        for trait_name in &criterion.traits {
            let entry = totals.entry(trait_name.clone()).or_insert((0, 0));
            entry.1 = entry.1.saturating_add(criterion.weight);
            if passed {
                entry.0 = entry.0.saturating_add(criterion.weight);
            }
        }
        outcomes.push(CriterionOutcome {
            id: criterion.id.clone(),
            passed,
            detail,
        });
    }

    let traits = traits_from_totals(&totals);

    RunScore {
        outcomes,
        traits,
        failed,
        weight_passed,
        weight_total,
    }
}

/// Turn per-trait `(passed weight, offered weight)` into 1–5 scores.
///
/// **The formula lives here and nowhere else** — [`score_run`] uses it for one
/// task and the harness reuses it to fold a run's tasks together, and two
/// implementations of `1.0 + 4.0 × raw` would be two answers waiting to
/// disagree. A trait whose offered weight is zero is **absent**, never zero.
pub fn traits_from_totals(totals: &BTreeMap<String, (u32, u32)>) -> BTreeMap<String, f64> {
    totals
        .iter()
        .filter(|(_, (_, total))| *total > 0)
        .map(|(name, (passed, total))| {
            let raw = f64::from(*passed) / f64::from(*total);
            (name.clone(), round1(4.0f64.mul_add(raw, 1.0)))
        })
        .collect()
}

/// Add one task's outcomes to a run-wide `(passed weight, offered weight)`
/// tally, so a run's trait score is taken across every task that tags it.
/// Averaging per-task scores would weight a one-criterion task like a
/// ten-criterion one.
pub fn accumulate_totals(
    totals: &mut BTreeMap<String, (u32, u32)>,
    criteria: &[Criterion],
    score: &RunScore,
) {
    for criterion in criteria {
        let passed = score
            .outcomes
            .iter()
            .find(|outcome| outcome.id == criterion.id)
            .is_some_and(|outcome| outcome.passed);
        for trait_name in &criterion.traits {
            let entry = totals.entry(trait_name.clone()).or_insert((0, 0));
            entry.1 = entry.1.saturating_add(criterion.weight);
            if passed {
                entry.0 = entry.0.saturating_add(criterion.weight);
            }
        }
    }
}

fn evaluate(criterion: &Criterion, artifacts: &RunArtifacts) -> (bool, String) {
    match &criterion.check {
        Check::ExitCode(check) => match artifacts.exit_code {
            Some(code) if code == check.equals => (true, format!("exit code {code}")),
            Some(code) => (
                false,
                format!("exit code {code}, expected {}", check.equals),
            ),
            None => (
                false,
                format!(
                    "the run reported no exit code, which is not {}",
                    check.equals
                ),
            ),
        },
        Check::StdoutMatches(check) => match regex::Regex::new(&check.pattern) {
            Ok(pattern) => {
                if pattern.is_match(&artifacts.stdout) {
                    (true, format!("stdout matches /{}/", check.pattern))
                } else {
                    (false, format!("stdout does not match /{}/", check.pattern))
                }
            }
            // A rubric with a broken pattern must not pass and must not panic.
            Err(error) => (
                false,
                format!("pattern /{}/ does not compile: {error}", check.pattern),
            ),
        },
        Check::FileContains(check) => match artifacts.files.get(&check.path) {
            Some(body) if body.contains(&check.text) => {
                (true, format!("{} contains {:?}", check.path, check.text))
            }
            Some(_) => (
                false,
                format!("{} does not contain {:?}", check.path, check.text),
            ),
            None => (false, format!("{} is not there at all", check.path)),
        },
        Check::FileAbsent(check) => {
            if artifacts.files.contains_key(&check.path) {
                (false, format!("{} is present and must not be", check.path))
            } else {
                (true, format!("{} is absent", check.path))
            }
        }
        Check::JsonPathEquals(check) => {
            let Some(body) = artifacts.files.get(&check.path) else {
                return (false, format!("{} is not there at all", check.path));
            };
            let Ok(json) = serde_json::from_str::<Value>(body) else {
                return (false, format!("{} is not JSON", check.path));
            };
            match json.pointer(&check.pointer) {
                Some(found) if *found == check.equals => (
                    true,
                    format!("{}{} == {}", check.path, check.pointer, check.equals),
                ),
                Some(found) => (
                    false,
                    format!(
                        "{}{} is {found}, expected {}",
                        check.path, check.pointer, check.equals
                    ),
                ),
                None => (
                    false,
                    format!("{} has nothing at {}", check.path, check.pointer),
                ),
            }
        }
        // The two the harness must answer. Absence of a probe is a failure
        // with a name, never a pass.
        Check::DiffApplies(check) => {
            probe(criterion, artifacts, &format!("{} applies", check.path))
        }
        Check::Command(check) => probe(criterion, artifacts, &format!("{} exited 0", check.script)),
    }
}

fn probe(criterion: &Criterion, artifacts: &RunArtifacts, sentence: &str) -> (bool, String) {
    match artifacts.probes.get(&criterion.id) {
        Some(true) => (true, sentence.to_owned()),
        Some(false) => (false, format!("not true: {sentence}")),
        None => (
            false,
            format!(
                "the harness recorded no answer for {:?} ({}), so it fails: an unanswered check \
                 is never a pass",
                criterion.id,
                criterion.check.kind()
            ),
        ),
    }
}

// ── across runs ──────────────────────────────────────────────────────────────
/// One trait across every run: the median, and the spread behind it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MeasuredTrait {
    /// The median of the per-run scores, one decimal.
    pub score: f64,
    /// How many runs are behind it.
    pub n: u32,
    /// The lowest per-run score.
    pub min: f64,
    /// The highest per-run score.
    pub max: f64,
}

impl MeasuredTrait {
    /// `max − min`. At or above [`BENCH_UNSTABLE_SPREAD`] the set is unstable
    /// and `propose` refuses it.
    pub fn spread(&self) -> f64 {
        round1(self.max - self.min)
    }

    /// `true` when this trait's runs disagreed too much to call it measured.
    pub fn is_unstable(&self) -> bool {
        self.spread() >= BENCH_UNSTABLE_SPREAD
    }
}

/// Median of a run's per-trait scores, with `n`, `min` and `max`.
///
/// A trait absent from a run is never read as zero: it aggregates only the
/// runs that scored it, and `n` says how many those were.
pub fn aggregate_runs(runs: &[BTreeMap<String, f64>]) -> BTreeMap<String, MeasuredTrait> {
    let mut per_trait: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    for run in runs {
        for (name, score) in run {
            per_trait.entry(name.clone()).or_default().push(*score);
        }
    }
    per_trait
        .into_iter()
        .filter(|(_, values)| !values.is_empty())
        .map(|(name, mut values)| {
            values.sort_by(|left, right| {
                left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal)
            });
            let n = values.len();
            let middle = n / 2;
            let median = if n % 2 == 1 {
                values[middle]
            } else {
                f64::midpoint(values[middle - 1], values[middle])
            };
            let measured = MeasuredTrait {
                score: round1(median),
                n: u32::try_from(n).unwrap_or(u32::MAX),
                min: values.first().copied().unwrap_or(median),
                max: values.last().copied().unwrap_or(median),
            };
            (name, measured)
        })
        .collect()
}

/// Traits whose runs disagreed by [`BENCH_UNSTABLE_SPREAD`] or more, sorted.
pub fn unstable_traits(traits: &BTreeMap<String, MeasuredTrait>) -> Vec<String> {
    traits
        .iter()
        .filter(|(_, measured)| measured.is_unstable())
        .map(|(name, _)| name.clone())
        .collect()
}

/// `confidence` for a measured row — **derived, never chosen**: `medium` for
/// `3 ≤ n < 9`, `high` above, `low` below the minimum (a row that cannot be
/// proposed at all, kept honest here rather than special-cased at the caller).
pub fn derived_confidence(n: u32) -> &'static str {
    match n {
        0..=2 => "low",
        3..=8 => "medium",
        _ => "high",
    }
}

// ── the measured block a row gains ───────────────────────────────────────────
/// The optional block `registry propose` writes onto one registry row.
///
/// Traits the bench did not evidence are **absent** from [`Self::traits`] and
/// stay opinions in the row's `scores`; the disclosure says which are which. A
/// half-measured row reading as measured is the same lie as a badge with no
/// event behind it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MeasuredBlock {
    /// The class whose bench produced these numbers.
    pub role: String,
    /// The `benchVersion` of the set that ran.
    pub bench_version: u32,
    /// sha256 over `team/registry-bench/<role>/**`, sorted by path.
    pub bench_hash: String,
    /// ISO date the runs were made.
    pub measured_at: String,
    /// 64-hex pubkey that signed every observation behind this block.
    pub measured_by: String,
    /// Every 44246 `gate` event id, in the order the runs happened.
    pub runs: Vec<String>,
    /// Trait → the median and its spread. Absent traits stay opinions.
    pub traits: BTreeMap<String, MeasuredTrait>,
}

impl MeasuredBlock {
    /// The traits this block measured, sorted.
    pub fn measured_trait_names(&self) -> Vec<String> {
        self.traits.keys().cloned().collect()
    }

    /// The smallest `n` any trait has: a block is only as measured as its
    /// thinnest trait.
    pub fn samples(&self) -> u32 {
        self.traits
            .values()
            .map(|measured| measured.n)
            .min()
            .unwrap_or(0)
    }

    /// `confidence` for the row's `rating`, derived from [`Self::samples`].
    pub fn confidence(&self) -> &'static str {
        derived_confidence(self.samples())
    }

    /// `confidence` once somebody has tried to resolve [`Self::runs`] against
    /// the relay.
    ///
    /// F4b: `runs` is a list of event ids and nothing resolved them, so a
    /// hand-edited block with a fabricated id and `n: 99` read as
    /// `confidence: high`. A row whose runs cannot be found is capped at
    /// `medium` however large its `n` — **`high` is a claim about evidence
    /// somebody has actually seen.**
    pub fn resolved_confidence(&self, runs_resolved: bool) -> &'static str {
        let derived = self.confidence();
        if runs_resolved || derived != "high" {
            derived
        } else {
            "medium"
        }
    }
}

// ── the bench hash ───────────────────────────────────────────────────────────
/// sha256 over a role's whole bench directory, sorted by path.
///
/// Length-prefixed per entry, so moving bytes between two files cannot leave
/// the digest unchanged. The CLI walks the directory; this hashes what it is
/// handed, so the digest is testable without a filesystem.
pub fn bench_hash(files: &BTreeMap<String, Vec<u8>>) -> String {
    let mut hasher = Sha256::new();
    for (path, bytes) in files {
        hasher.update(path.as_bytes());
        hasher.update([0]);
        hasher.update(bytes.len().to_le_bytes());
        hasher.update(bytes);
    }
    hex::encode(hasher.finalize())
}

#[path = "registry_bench_proposal.rs"]
mod proposal;
pub use proposal::*;

#[cfg(test)]
#[path = "registry_bench_tests.rs"]
mod registry_bench_tests;
