//! Seeding a sandbox's build state inside a prepared project boundary.
//!
//! `buzz-core` owns the declaration, the decisions and the filesystem work, and
//! deliberately does not run git: *how* git may be run is this crate's
//! business. A checkout's own configuration can name smudge filters and an
//! fsmonitor program, and a coding session of the project can write that
//! configuration — so git runs inside the same prepared boundary the step or
//! the seat itself runs in.
//!
//! Both the desktop's worktree creation and an action step's detached tree hold
//! a [`HostLaunchPlan`] by the time they have a tree to seed, so both reach this
//! one implementation rather than each wrapping git themselves.

use std::path::Path;

use buzz_core::sandbox_manifest::{load_sandbox_manifest, SandboxRefusal};
use buzz_core::sandbox_seed::{
    apply, preflight, NodeKind, SeedDisposition, SeedInputs, SeedReceipt,
};
use buzz_core::sandbox_seed_fs::{ignored_from_patterns, SeedGit, StdSeedOps};

use crate::execution_scope_host::HostLaunchPlan;

/// Git, inside a prepared boundary.
pub struct PlanGit<'a> {
    plan: &'a HostLaunchPlan,
}

impl<'a> PlanGit<'a> {
    /// Answer the seeder's git questions through `plan`.
    #[must_use]
    pub const fn new(plan: &'a HostLaunchPlan) -> Self {
        Self { plan }
    }

    /// One git invocation's stdout, or `None` when it could not answer.
    fn git(&self, root: &Path, args: &[&str]) -> Option<String> {
        let mut command = self.plan.git_command(root, args);
        command.stdin(std::process::Stdio::null());
        let output = command.output().ok()?;
        if output.status.success() {
            Some(String::from_utf8_lossy(&output.stdout).into_owned())
        } else {
            None
        }
    }

    /// The pattern `git check-ignore -v` matched, or `None`.
    fn matching_pattern(&self, root: &Path, query: &str) -> Option<String> {
        let out = self.git(root, &["check-ignore", "-v", "--no-index", "--", query])?;
        // `<source>:<line>:<pattern>\t<pathname>`
        let first = out.lines().next()?;
        let pattern = first.split('\t').next()?.rsplit(':').next()?;
        Some(pattern.to_owned())
    }
}

impl SeedGit for PlanGit<'_> {
    fn tracked(&self, root: &Path, relative: &str) -> bool {
        // `ls-files` exits 0 and prints nothing when nothing matches, so a
        // failure means git could not answer — and an unanswered question here
        // must read as "tracked", or a seed could write over source.
        match self.git(root, &["ls-files", "--", relative]) {
            Some(out) => !out.trim().is_empty(),
            None => true,
        }
    }

    fn ignored(&self, root: &Path, relative: &str, kind: NodeKind) -> bool {
        // The rule lives in buzz-core; this only supplies git's answers.
        ignored_from_patterns(relative, kind, |query| self.matching_pattern(root, query))
    }
}

/// Filesystem work plus bounded git, for seeding one tree.
///
/// Sizes are left unmeasured: walking a 36 GB build directory is seconds, and
/// this runs on the path that creates a sandbox. An unmeasured size reads
/// `unknown` in the receipt, which is the honest answer for a number nobody
/// paid to find out.
#[must_use]
pub fn plan_seed_ops<'a>(plan: &'a HostLaunchPlan) -> StdSeedOps<PlanGit<'a>> {
    StdSeedOps::new(PlanGit::new(plan))
}

/// Run one `run` entry inside `plan`, in `tree`.
///
/// A closed shape rather than a free command line: the project's own pinned
/// `just` recipe, or one of its own scripts. Stated plainly because the honesty
/// rule applies to comments too — that shape is legibility, not the boundary.
/// What enforces is that this goes through [`HostLaunchPlan::program_command`].
pub fn run_entry_in(
    plan: &HostLaunchPlan,
    tree: &Path,
    run: &buzz_core::sandbox_seed::PendingRun,
    shared_env: &std::collections::BTreeMap<String, String>,
) -> (SeedDisposition, String) {
    let (program, args) = match (run.recipe.as_deref(), run.script.as_deref()) {
        (Some(recipe), None) => (
            tree.join("bin/just").to_string_lossy().into_owned(),
            vec![recipe.to_owned()],
        ),
        (None, Some(script)) => (
            "/bin/bash".to_owned(),
            vec![tree.join(script).to_string_lossy().into_owned()],
        ),
        _ => {
            return (
                SeedDisposition::Refused {
                    code: "SANDBOX_SEED_RUN_SHAPE",
                },
                format!("{} names neither a recipe nor a script", run.id),
            )
        }
    };
    let argv: Vec<&str> = args.iter().map(String::as_str).collect();
    let mut command = plan.program_command(tree, &program, &argv);
    for (name, value) in shared_env.iter().chain(run.env.iter()) {
        command.env(name, value);
    }
    for name in &run.env_from_host {
        match std::env::var_os(name) {
            Some(value) => {
                command.env(name, value);
            }
            None => {
                return (
                    SeedDisposition::Refused {
                        code: "SANDBOX_SEED_ENV_MISSING",
                    },
                    format!("{} needs {name} from the host, which is not set", run.id),
                )
            }
        }
    }
    command.stdin(std::process::Stdio::null());
    match command.output() {
        Err(error) => (
            SeedDisposition::Failed {
                code: "SANDBOX_SEED_RUN_FAILED",
            },
            format!("{} could not be started: {error}", run.id),
        ),
        Ok(output) if output.status.success() => {
            (SeedDisposition::Seeded, format!("{} ran", run.id))
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let tail = stderr.trim().lines().next_back().unwrap_or("no output");
            (
                SeedDisposition::Failed {
                    code: "SANDBOX_SEED_RUN_FAILED",
                },
                format!("{} exited {}: {tail}", run.id, output.status),
            )
        }
    }
}

/// Seed one tree's build state as the project declares it, inside `boundary`.
///
/// The whole sequence, in one place, because both paths that cut a sandbox do
/// exactly this: read the declaration *from the tree* (so the declaration that
/// applies is the one belonging to the commit the tree is on), carry out the
/// filesystem entries, then run the project's own setup recipes inside the same
/// boundary and record how they were confined.
///
/// `Ok(None)` means the project declares no `sandbox.yml` — nothing was asked
/// for, which is not the same as a seed that failed. Any other outcome answers
/// with a receipt, including one where nothing landed: a cold sandbox nobody was
/// told about is what the receipt exists to prevent.
///
/// # Errors
///
/// The manifest is present and malformed. Returned rather than swallowed so the
/// caller can disclose it; no caller should let it fail the tree, because
/// `sandbox.yml` is a tracked file an agent may edit.
pub fn seed_tree(
    boundary: &HostLaunchPlan,
    source: &Path,
    tree: &Path,
    pool_root: Option<&Path>,
) -> Result<Option<SeedReceipt>, SandboxRefusal> {
    let Some(declared) = load_sandbox_manifest(tree)? else {
        return Ok(None);
    };
    let ops = plan_seed_ops(boundary);
    let program = preflight(
        Some(&declared),
        &SeedInputs {
            source: Some(source),
            dest: tree,
            pool_root,
        },
        &ops,
    );
    let mut receipt = apply(&program, &ops);
    if !receipt.pending_runs.is_empty() {
        let runs = receipt.pending_runs.clone();
        let shared_env = receipt.env.clone();
        for run in &runs {
            let (disposition, detail) = run_entry_in(boundary, tree, run, &shared_env);
            receipt.record_run(run, disposition, detail);
        }
        receipt.set_boundary(boundary_of(boundary));
    }
    Ok(Some(receipt))
}

/// How the boundary a seed's recipes ran in should be disclosed.
#[must_use]
pub fn boundary_of(plan: &HostLaunchPlan) -> buzz_core::sandbox_seed::SeedBoundary {
    match plan {
        HostLaunchPlan::Bounded(_) => buzz_core::sandbox_seed::SeedBoundary::Enforced {
            backend: "the project boundary prepared for this host command".to_owned(),
        },
        HostLaunchPlan::Unenforced { reason } => {
            buzz_core::sandbox_seed::SeedBoundary::NotEnforced {
                reason: (*reason).to_owned(),
            }
        }
    }
}

/// One line a person can read, for a step tail or a log.
#[must_use]
pub fn summarize(receipt: &SeedReceipt) -> String {
    let unmet = receipt.unsatisfied();
    if unmet.is_empty() {
        return format!(
            "sandbox: seeded {} entr{} from the project's sandbox.yml",
            receipt.outcomes.len(),
            if receipt.outcomes.len() == 1 {
                "y"
            } else {
                "ies"
            }
        );
    }
    let named: Vec<String> = unmet
        .iter()
        .map(|outcome| {
            format!(
                "{} ({})",
                outcome.id,
                outcome.disposition.code().unwrap_or("incomplete")
            )
        })
        .collect();
    format!(
        "sandbox: this tree is colder than it asked to be — {} did not land",
        named.join(", ")
    )
}
