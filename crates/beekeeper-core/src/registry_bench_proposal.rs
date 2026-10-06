//! What a proposal is built from, and every reason it is refused.
//!
//! Split out of [`super`] only to keep both files under the 1,000-line rule;
//! everything here is re-exported from `registry_bench`, so no caller's path
//! changes. The division is real enough to be worth the file: [`super`] scores
//! **one run** from its artifacts, and this decides whether a set of signed
//! rows may become **one registry row**.

use std::collections::{BTreeMap, BTreeSet};

use super::{unstable_traits, MeasuredTrait, BENCH_UNSTABLE_SPREAD, REGISTRY_BENCH_RELATIVE_PATH};

// ── what `propose` refuses, and why ──────────────────────────────────────────
/// The gate rows read **back off the relay** for one proposal. `propose` never
/// reads the measuring run's own memory: signed events, or no proposal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BenchGateRow {
    /// The 44246 event id.
    pub event_id: String,
    /// 64-hex pubkey that signed it.
    pub signer: String,
    /// `registry-bench/<role>/<task-id>#<run>`.
    pub gate: String,
    /// The role parsed out of [`Self::gate`].
    pub role: String,
    /// The task id parsed out of [`Self::gate`].
    pub task_id: String,
    /// The run index parsed out of [`Self::gate`].
    pub run: u32,
    /// `passed` only when every criterion passed.
    pub outcome: String,
    /// The row's summary, `"<fraction> · failed: <criterion ids>"` — where a
    /// proposal gets its per-run detail. A `findingId` carries no run index
    /// (it is stable across runs), so run 2's failed set can only come from
    /// run 2's own gate row.
    pub summary: Option<String>,
}

impl BenchGateRow {
    /// The criteria this row says failed, or `None` when the row carries no
    /// summary this scorer can read.
    ///
    /// **`None` is not "nothing failed".** F2: a row with `summary: None` used
    /// to fold as a perfect run, so three `bee sessions observe gate` calls
    /// with no `--summary` minted a row clearing every ratified minimum. A row
    /// that cannot say what failed is not evidence, and
    /// [`gate_row_refusals`] refuses the proposal by row id.
    pub fn failed_criteria(&self) -> Option<Vec<String>> {
        let summary = self.summary.as_deref()?;
        if !summary.contains(BENCH_SUMMARY_MARKER) {
            return None;
        }
        Some(parse_bench_summary(summary))
    }

    /// `true` when this row's own `outcome` word says the task failed.
    ///
    /// F3: `outcome` was read off the wire and never used, so a row saying
    /// `failed` while its summary said `failed: none` folded as a perfect run.
    /// The signed word wins: every criterion this task tags scores as failed,
    /// whatever the summary claims.
    pub fn says_failed(&self) -> bool {
        self.outcome == "failed"
    }
}

/// Every reason a set of registry-bench gate rows is not evidence, in a fixed
/// order.
///
/// Separate from [`proposal_refusals`] because these are defects in the *rows*
/// rather than in the proposal built from them: a row that cannot say what
/// failed, a row whose two fields contradict each other, a row that never ran,
/// and two rows claiming the same task-run.
pub fn gate_row_refusals(rows: &[BenchGateRow]) -> Vec<String> {
    let mut refusals = Vec::new();

    for row in rows {
        // F2 — no parseable summary.
        let Some(failed) = row.failed_criteria() else {
            refusals.push(format!(
                "gate row {} has no parseable summary",
                row.event_id
            ));
            continue;
        };
        // F3 — the two fields on one signed row disagree.
        if row.outcome == "passed" && !failed.is_empty() {
            refusals.push(format!(
                "gate row {} says outcome passed and its summary names {} as failed: one signed \
                 row cannot say both",
                row.event_id,
                failed.join(", ")
            ));
        }
        if row.outcome == "failed" && failed.is_empty() {
            refusals.push(format!(
                "gate row {} says outcome failed and its summary names nothing as failed: one \
                 signed row cannot say both",
                row.event_id
            ));
        }
        // A row that did not run is not a run.
        if row.outcome != "passed" && row.outcome != "failed" {
            refusals.push(format!(
                "gate row {} has outcome {:?}, which is not a run: a measurement is built from \
                 tasks that ran",
                row.event_id, row.outcome
            ));
        }
    }

    // F8 — two measurements interleaved on one session. A gate name carries no
    // measurement id, so a second `measure --repeat 3` on the same session
    // republishes the same three names and the fold silently took whichever
    // sorted first.
    let mut seen: BTreeMap<(u32, String), Vec<&str>> = BTreeMap::new();
    for row in rows {
        seen.entry((row.run, row.task_id.clone()))
            .or_default()
            .push(row.event_id.as_str());
    }
    for ((run, task_id), ids) in seen {
        if ids.len() > 1 {
            refusals.push(format!(
                "{} gate rows claim {task_id}#{run} ({}): two measurements on one session, and \
                 nothing on the wire says which run produced which number — measure into a fresh \
                 --session-ref",
                ids.len(),
                ids.join(", ")
            ));
        }
    }

    refusals
}

/// Parse `registry-bench/<role>/<task-id>#<run>`.
///
/// # Errors
///
/// The string, when it is not that shape: a gate name this lane did not write
/// must never be counted as a run behind a row.
pub fn parse_bench_gate(gate: &str) -> Result<(String, String, u32), String> {
    let rest = gate
        .strip_prefix("registry-bench/")
        .ok_or_else(|| format!("{gate:?} is not a registry-bench gate"))?;
    let (path, run) = rest
        .rsplit_once('#')
        .ok_or_else(|| format!("{gate:?} has no #<run> suffix"))?;
    let (role, task) = path
        .split_once('/')
        .ok_or_else(|| format!("{gate:?} does not name <role>/<task-id>"))?;
    let run: u32 = run
        .parse()
        .map_err(|_| format!("{gate:?} has a non-numeric run index"))?;
    if role.is_empty() || task.is_empty() {
        return Err(format!("{gate:?} has an empty role or task id"));
    }
    Ok((role.to_owned(), task.to_owned(), run))
}

/// The marker every registry-bench gate row's summary must carry.
///
/// `parse_bench_summary` returns an empty list for `failed: none` **and** for a
/// summary it cannot read, and those are different facts. This constant is how
/// a caller tells them apart, which [`BenchGateRow::failed_criteria`] does.
pub const BENCH_SUMMARY_MARKER: &str = "failed:";

/// The criterion ids a gate row's summary names as failed, from
/// `"<fraction> · failed: a, b"`. `failed: none` and an unparseable summary
/// both give an empty list — different facts, so a caller that must tell them
/// apart checks for the marker itself.
pub fn parse_bench_summary(summary: &str) -> Vec<String> {
    summary
        .split_once("failed:")
        .map(|(_, tail)| tail.trim())
        .filter(|tail| *tail != "none")
        .map(|tail| {
            tail.split(',')
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// Build the gate string for one task-run.
pub fn bench_gate_name(role: &str, task_id: &str, run: u32) -> String {
    format!("registry-bench/{role}/{task_id}#{run}")
}

/// Build the `findingId` for one failed criterion.
pub fn bench_finding_id(bench_version: u32, role: &str, task_id: &str, criterion: &str) -> String {
    format!("bench:{bench_version}:{role}:{task_id}:{criterion}")
}

/// Everything a proposal is checked against.
#[derive(Debug, Clone)]
pub struct ProposalInput<'a> {
    /// The class being proposed for.
    pub role: &'a str,
    /// The bench version the rows were produced under.
    pub bench_version: u32,
    /// The `benchVersion` in the tree's `bench.yaml` right now.
    ///
    /// F12: `bench_version` was documented as a refusal input and read by
    /// nothing. Version drift was caught only incidentally by the hash, and a
    /// documented input that refuses nothing invites the next person to lean
    /// on it.
    pub tree_bench_version: u32,
    /// The bench hash the rows were produced under.
    pub bench_hash: &'a str,
    /// The hash of the bench directory **as it is on disk right now**.
    pub tree_bench_hash: &'a str,
    /// Runs demanded per task.
    pub repeat: u32,
    /// The bench's ordered task list.
    pub tasks: &'a [String],
    /// The gate rows read back off the relay.
    pub gate_rows: &'a [BenchGateRow],
    /// The class gate these rows are read by; every trait in it must be
    /// evidenced.
    pub minimums: &'a BTreeMap<String, f64>,
    /// The aggregated per-trait medians.
    pub traits: &'a BTreeMap<String, MeasuredTrait>,
}

/// Every reason this proposal is refused, in a fixed order, or an empty vector.
/// Exit 4 with these sentences is the whole of `propose`'s judgment: it writes
/// a row or names what stopped it, and never writes a partial one.
pub fn proposal_refusals(input: &ProposalInput<'_>) -> Vec<String> {
    let mut refusals = Vec::new();

    if input.tasks.is_empty() {
        refusals.push(format!(
            "the bench for {} has no tasks: a row measured by nothing is not a measured row",
            input.role
        ));
    }

    if input.bench_version != input.tree_bench_version {
        refusals.push(format!(
            "the rows were measured under {}/{} benchVersion {} and the tree says {} today",
            REGISTRY_BENCH_RELATIVE_PATH, input.role, input.bench_version, input.tree_bench_version
        ));
    }

    if input.bench_hash != input.tree_bench_hash {
        refusals.push(format!(
            "the rows were measured against bench hash {} and {}/{} hashes {} today: the task set \
             changed under the measurement",
            short_hash(input.bench_hash),
            REGISTRY_BENCH_RELATIVE_PATH,
            input.role,
            short_hash(input.tree_bench_hash)
        ));
    }

    let mine: Vec<&BenchGateRow> = input
        .gate_rows
        .iter()
        .filter(|row| row.role == input.role)
        .collect();

    let signers: BTreeSet<&str> = mine.iter().map(|row| row.signer.as_str()).collect();
    if signers.len() > 1 {
        refusals.push(format!(
            "{} signing keys wrote these rows ({}): one proposal, one signer",
            signers.len(),
            signers.into_iter().collect::<Vec<&str>>().join(", ")
        ));
    }

    for task in input.tasks {
        let runs: BTreeSet<u32> = mine
            .iter()
            .filter(|row| row.task_id == *task)
            .map(|row| row.run)
            .collect();
        if (runs.len() as u32) < input.repeat {
            refusals.push(format!(
                "task {task} has {} run(s) on the relay and {} were asked for",
                runs.len(),
                input.repeat
            ));
        }
    }

    for trait_name in input.minimums.keys() {
        if !input.traits.contains_key(trait_name) {
            refusals.push(format!(
                "the {} gate reads {trait_name} and this bench evidences nothing about it: a \
                 {} row measuring nothing about {trait_name} is not a {} row",
                input.role, input.role, input.role
            ));
        }
    }

    let unstable = unstable_traits(input.traits);
    if !unstable.is_empty() {
        let detail: Vec<String> = unstable
            .iter()
            .filter_map(|name| {
                input
                    .traits
                    .get(name)
                    .map(|measured| format!("{name} {}–{}", measured.min, measured.max))
            })
            .collect();
        refusals.push(format!(
            "the set is UNSTABLE on {}: a spread of {BENCH_UNSTABLE_SPREAD} or more means the \
             runs disagreed too much to call any of it measured ({})",
            unstable.join(", "),
            detail.join("; ")
        ));
    }

    refusals
}

fn short_hash(hash: &str) -> &str {
    hash.get(..12).unwrap_or(hash)
}

/// One incumbent's measured score beside the minimum its class asks for.
///
/// Brian's addendum of 2026-09-01 ratified the class minimums as they stand
/// and asked for exactly this table — the bar, the number, the gap — so they
/// can be re-derived later with numbers in hand rather than beside priors.
#[derive(Debug, Clone, PartialEq)]
pub struct MinimumComparison {
    /// The trait.
    pub trait_name: String,
    /// What the class gate asks for.
    pub minimum: f64,
    /// What this bench measured, or `None` when the bench evidenced nothing.
    pub measured: Option<f64>,
}

impl MinimumComparison {
    /// `true` when a measured score clears the ratified minimum. `None` — the
    /// bench measured nothing — is never a pass.
    pub fn clears(&self) -> bool {
        self.measured.is_some_and(|score| score >= self.minimum)
    }
}

/// The comparison table for one class, in trait order.
pub fn compare_to_minimums(
    minimums: &BTreeMap<String, f64>,
    traits: &BTreeMap<String, MeasuredTrait>,
) -> Vec<MinimumComparison> {
    minimums
        .iter()
        .map(|(trait_name, minimum)| MinimumComparison {
            trait_name: trait_name.clone(),
            minimum: *minimum,
            measured: traits.get(trait_name).map(|measured| measured.score),
        })
        .collect()
}
