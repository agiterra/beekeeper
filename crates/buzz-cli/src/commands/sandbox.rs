//! `bee sandbox` — seed a sandbox's build state from a checkout, and say what
//! this project calls build state.
//!
//! One declaration (`sandbox.yml`), one parser (`buzz_core::sandbox_manifest`),
//! one engine (`buzz_core::sandbox_seed`). This is the entry point a person and
//! the `Justfile` use; the Beekeeper launcher calls the same engine with the
//! boundary it holds. A second copy of any of it in shell is how the reclaim
//! list came to disagree with itself in four places.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::json;

use buzz_core::sandbox_manifest::{
    load_sandbox_manifest, reclaim_plan, reclaim_plan_for, sandbox_manifest_path, ReclaimAction,
    SandboxPlan, SANDBOX_YML,
};
use buzz_core::sandbox_seed::{
    apply, estimate_reclaimable, preflight, reclaim, render_reclaim_estimate, render_seed_bytes,
    NodeKind, SeedBoundary, SeedDisposition, SeedInputs, SeedProgram, SeedReceipt,
};
use buzz_core::sandbox_seed_fs::{ignored_from_patterns, NoGitAnswers, SeedGit, StdSeedOps};

use crate::error::CliError;
use crate::OutputFormat;

/// Git's answers, from git.
///
/// This runs `git` directly, which is right *here* and wrong in the product:
/// `bee sandbox` is a person acting on their own machine, while a seat's host
/// must run git inside the tree's prepared boundary. That is exactly why
/// `buzz-core` takes these answers through a trait instead of spawning git
/// itself.
pub struct GitAnswers;

impl GitAnswers {
    /// `git check-ignore -v`'s verdict for one path: the matching pattern, or
    /// `None` when nothing matches.
    fn matching_pattern(root: &Path, relative: &str) -> Option<String> {
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["check-ignore", "-v", "--no-index", "--"])
            .arg(relative)
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        // `<source>:<line>:<pattern>\t<pathname>`
        let line = String::from_utf8_lossy(&output.stdout);
        let first = line.lines().next()?;
        let before_tab = first.split('\t').next()?;
        let pattern = before_tab.rsplit(':').next()?;
        Some(pattern.to_owned())
    }
}

impl SeedGit for GitAnswers {
    fn tracked(&self, root: &Path, relative: &str) -> bool {
        // `ls-files` exits 0 and prints nothing when nothing matches, so a
        // non-zero exit means git could not answer — and an unanswered
        // question here must read as "tracked", or a seed could write over
        // source. Fail closed.
        let Ok(output) = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["ls-files", "--"])
            .arg(relative)
            .output()
        else {
            return true;
        };
        if !output.status.success() {
            return true;
        }
        !String::from_utf8_lossy(&output.stdout).trim().is_empty()
    }

    fn ignored(&self, root: &Path, relative: &str, kind: NodeKind) -> bool {
        // The rule itself lives in buzz-core, so this and the desktop's copy
        // cannot drift: all this supplies is git's answer for one query.
        ignored_from_patterns(relative, kind, |query| Self::matching_pattern(root, query))
    }
}

fn ops(measure: bool) -> StdSeedOps<GitAnswers> {
    StdSeedOps::new(GitAnswers).measuring(measure)
}

/// The checkout a relative path belongs to, from git.
fn toplevel_of(start: &Path) -> Result<PathBuf, CliError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(start)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .map_err(|error| CliError::Other(format!("could not run git: {error}")))?;
    if !output.status.success() {
        return Err(CliError::Usage(format!(
            "{} is not inside a git checkout",
            start.display()
        )));
    }
    Ok(PathBuf::from(
        String::from_utf8_lossy(&output.stdout).trim(),
    ))
}

/// Where this machine keeps one pool per repository, when nobody named one.
///
/// A person's own cache directory, keyed on the source checkout, so several
/// sandboxes cut from one checkout share a cargo registry instead of each
/// downloading it. The product path does not use this: a seat's pools live
/// inside the project scope its executions already have.
fn default_pool_root(source: &Path) -> Option<PathBuf> {
    use sha2::{Digest as _, Sha256};
    let key = hex::encode(Sha256::digest(source.as_os_str().as_encoded_bytes()));
    let home = std::env::var_os("HOME")?;
    Some(
        PathBuf::from(home)
            .join("Library/Caches/beekeeper/sandbox-pools")
            .join(&key[..16]),
    )
}

fn resolved(path: &str) -> Result<PathBuf, CliError> {
    std::path::absolute(path).map_err(|error| CliError::Usage(format!("{path}: {error}")))
}

/// `bee sandbox plan` — what this project declares, and what reclaim would do.
///
/// Read-only in the strongest sense: it parses the manifest and prints the
/// plan. It neither writes nor measures.
///
/// # Errors
///
/// A manifest that is present and malformed.
pub fn plan(checkout: Option<&str>, format: OutputFormat) -> Result<(), CliError> {
    let root = match checkout {
        Some(path) => resolved(path)?,
        None => toplevel_of(Path::new("."))?,
    };
    let parsed =
        load_sandbox_manifest(&root).map_err(|refusal| CliError::Usage(format!("{refusal}")))?;
    let reclaim = reclaim_plan(parsed.as_ref());
    let body = match &parsed {
        None => json!({
            "checkout": root,
            "manifest": serde_json::Value::Null,
            "declares": false,
            "detail": format!(
                "{} has no {SANDBOX_YML}, so nothing is seeded and reclaim uses the built-in \
                 fallback list",
                root.display()
            ),
            "reclaim": reclaim,
        }),
        Some(parsed) => json!({
            "checkout": root,
            "manifest": sandbox_manifest_path(&root),
            "declares": true,
            "manifestSha256": parsed.manifest_sha256,
            "pool": parsed.pool,
            "entries": entry_rows(parsed),
            "warnings": parsed.warnings,
            "reclaim": reclaim,
        }),
    };
    print_json(&body, format);
    Ok(())
}

fn entry_rows(parsed: &SandboxPlan) -> Vec<serde_json::Value> {
    parsed
        .entries
        .iter()
        .map(|entry| {
            json!({
                "id": entry.id(),
                "kind": entry.kind().as_str(),
                "destination": entry.destination(),
                "seeds": entry.seeds(),
                "required": entry.required(),
            })
        })
        .collect()
}

/// `bee sandbox reclaim-paths` — what this project calls build state.
///
/// NUL-separated `path\0action` records, for a shell caller that must not
/// re-derive the list. A malformed manifest is reported on stderr and the
/// fallback list is printed, because refusing to free anything until a YAML
/// typo is fixed would be the worse answer.
///
/// # Errors
///
/// The checkout could not be resolved.
pub fn reclaim_paths(checkout: Option<&str>) -> Result<(), CliError> {
    let root = match checkout {
        Some(path) => resolved(path)?,
        None => toplevel_of(Path::new("."))?,
    };
    let (plan, refusal) = reclaim_plan_for(&root);
    if let Some(refusal) = refusal {
        eprintln!("warning: {SANDBOX_YML} was not used: {refusal}");
    }
    let mut out = String::new();
    for target in &plan.targets {
        if matches!(target.action, ReclaimAction::Keep) {
            continue;
        }
        let action = match target.action {
            ReclaimAction::DeleteDirectory => "delete",
            ReclaimAction::UnlinkOnly => "unlink",
            ReclaimAction::Keep => unreachable!("filtered above"),
        };
        out.push_str(&target.path);
        out.push('\0');
        out.push_str(action);
        out.push('\0');
    }
    print!("{out}");
    Ok(())
}

fn print_json(body: &serde_json::Value, format: OutputFormat) {
    let rendered = match format {
        OutputFormat::Compact => serde_json::to_string(body),
        OutputFormat::Json => serde_json::to_string_pretty(body),
    };
    println!(
        "{}",
        rendered.unwrap_or_else(|error| format!("{{\"error\":\"{error}\"}}"))
    );
}

/// `bee sandbox seed` — seed a sandbox's build state from a checkout.
///
/// Without `--confirm` it prints what it would do and writes nothing. The
/// manifest is read from the **tree being seeded**, so the declaration that
/// applies is the one belonging to the commit that tree is on.
///
/// # Errors
///
/// A malformed manifest, a tree that is not a directory, a source that is the
/// tree itself, or a seed that did not land every entry it declared — the
/// receipt is printed either way, and a non-zero exit is how a caller knows
/// the sandbox is colder than it asked to be.
pub fn seed(
    tree: &str,
    from: Option<&str>,
    pool_root: Option<&str>,
    run_recipes: bool,
    confirm: bool,
    format: OutputFormat,
) -> Result<(), CliError> {
    let tree = resolved(tree)?;
    if !tree.is_dir() {
        return Err(CliError::Usage(format!(
            "{} is not a directory",
            tree.display()
        )));
    }
    let source = match from {
        Some(path) => resolved(path)?,
        None => toplevel_of(Path::new("."))?,
    };
    if source == tree {
        return Err(CliError::Usage(format!(
            "{} is both the source and the sandbox; a checkout cannot be seeded from itself",
            tree.display()
        )));
    }
    let pool = match pool_root {
        Some(path) => Some(resolved(path)?),
        None => default_pool_root(&source),
    };
    let parsed =
        load_sandbox_manifest(&tree).map_err(|refusal| CliError::Usage(format!("{refusal}")))?;
    let ops = ops(true);
    let inputs = SeedInputs {
        source: Some(&source),
        dest: &tree,
        pool_root: pool.as_deref(),
    };
    let program = preflight(parsed.as_ref(), &inputs, &ops);
    if !confirm {
        print_json(
            &preview_body(&tree, &source, pool.as_deref(), &program),
            format,
        );
        return Ok(());
    }
    let mut receipt = apply(&program, &ops);
    if run_recipes {
        execute_runs(&tree, &mut receipt);
        receipt.set_boundary(SeedBoundary::NotEnforced {
            reason: "bee sandbox seed --run-recipes runs the project's own recipes directly, as \
                     the person who asked for it"
                .to_owned(),
        });
    } else if !receipt.pending_runs.is_empty() {
        receipt.set_boundary(SeedBoundary::NotEnforced {
            reason: "no recipes were run; pass --run-recipes to run them".to_owned(),
        });
    }
    print_json(&receipt_body(&receipt, pool.as_deref()), format);
    if receipt.complete {
        return Ok(());
    }
    let unmet: Vec<String> = receipt
        .unsatisfied()
        .iter()
        .map(|outcome| {
            format!(
                "{} ({})",
                outcome.id,
                outcome.disposition.code().unwrap_or("incomplete")
            )
        })
        .collect();
    let outstanding = receipt.pending_runs.len().saturating_sub(
        receipt
            .outcomes
            .iter()
            .filter(|outcome| outcome.used.is_some() && outcome.source.is_none())
            .count(),
    );
    let mut detail = if unmet.is_empty() {
        String::from("the sandbox is colder than it asked to be")
    } else {
        format!("these entries did not land: {}", unmet.join(", "))
    };
    if outstanding > 0 {
        detail.push_str(&format!(
            "; {outstanding} setup recipe(s) were not run — pass --run-recipes"
        ));
    }
    Err(CliError::Other(detail))
}

fn preview_body(
    tree: &Path,
    source: &Path,
    pool: Option<&Path>,
    program: &SeedProgram,
) -> serde_json::Value {
    json!({
        "dryRun": true,
        "tree": tree,
        "source": source,
        "poolRoot": pool,
        "entries": program.preview(),
        "pendingRuns": program.pending_runs(),
        "detail": "nothing was written; pass --confirm to carry this out",
    })
}

fn receipt_body(receipt: &SeedReceipt, pool: Option<&Path>) -> serde_json::Value {
    let lines: Vec<serde_json::Value> = receipt
        .outcomes
        .iter()
        .map(|outcome| {
            json!({
                "id": outcome.id,
                "declared": outcome.declared,
                "used": outcome.used,
                "state": outcome.disposition,
                "size": render_seed_bytes(outcome.bytes),
                "detail": outcome.detail,
                "notes": outcome.notes,
            })
        })
        .collect();
    json!({
        "tree": receipt.tree,
        "source": receipt.source,
        "poolRoot": pool,
        "manifestSha256": receipt.manifest_sha256,
        "complete": receipt.complete,
        "boundary": receipt.boundary,
        "env": receipt.env,
        "entries": lines,
    })
}

/// Run the project's own setup recipes, in the tree, as the person who asked.
///
/// Unconfined, and the receipt says so: this is `bee` on somebody's own
/// machine. The product path runs the same entries inside the new tree's
/// prepared boundary instead.
fn execute_runs(tree: &Path, receipt: &mut SeedReceipt) {
    let runs = receipt.pending_runs.clone();
    let shared_env = receipt.env.clone();
    for run in &runs {
        match build_command(tree, run, &shared_env) {
            Err(detail) => receipt.record_run(
                run,
                SeedDisposition::Refused {
                    code: "SANDBOX_SEED_RUN_SHAPE",
                },
                detail,
            ),
            Ok(mut command) => match command.output() {
                Err(error) => receipt.record_run(
                    run,
                    SeedDisposition::Failed {
                        code: "SANDBOX_SEED_RUN_FAILED",
                    },
                    format!("{} could not be started: {error}", run.id),
                ),
                Ok(output) if output.status.success() => {
                    receipt.record_run(run, SeedDisposition::Seeded, format!("{} ran", run.id));
                }
                Ok(output) => {
                    let tail = String::from_utf8_lossy(&output.stderr);
                    let tail = tail.trim().lines().next_back().unwrap_or("no output");
                    receipt.record_run(
                        run,
                        SeedDisposition::Failed {
                            code: "SANDBOX_SEED_RUN_FAILED",
                        },
                        format!("{} exited {}: {tail}", run.id, output.status),
                    );
                }
            },
        }
    }
}

fn build_command(
    tree: &Path,
    run: &buzz_core::sandbox_seed::PendingRun,
    shared_env: &BTreeMap<String, String>,
) -> Result<Command, String> {
    let mut command = match (run.recipe.as_deref(), run.script.as_deref()) {
        // The project's own pinned `just`, from the tree, rather than whatever
        // is on PATH.
        (Some(recipe), None) => {
            let mut command = Command::new(tree.join("bin/just"));
            command.arg(recipe);
            command
        }
        (None, Some(script)) => {
            let mut command = Command::new("/bin/bash");
            command.arg(tree.join(script));
            command
        }
        _ => return Err(format!("{} names neither a recipe nor a script", run.id)),
    };
    command.current_dir(tree);
    for (name, value) in shared_env.iter().chain(run.env.iter()) {
        command.env(name, value);
    }
    for name in &run.env_from_host {
        match std::env::var_os(name) {
            Some(value) => {
                command.env(name, value);
            }
            None => {
                return Err(format!(
                    "{} needs {name} from the host, which is not set here",
                    run.id
                ))
            }
        }
    }
    Ok(command)
}

/// Measure and free what a checkout calls build state.
///
/// The one implementation behind `bee sessions worktree reclaim` and the
/// desktop's own prune, so the set of directories cannot differ between them.
/// Reclaim asks git nothing — it reads shapes off the disk — so it carries no
/// git of its own.
#[must_use]
pub fn reclaim_build_output(tree: &Path) -> buzz_core::sandbox_seed::ReclaimReceipt {
    let (plan, _) = reclaim_plan_for(tree);
    reclaim(&plan, tree, &StdSeedOps::new(NoGitAnswers).measuring(true))
}

/// What reclaiming one checkout would free, and how to say it.
#[must_use]
pub fn reclaimable(tree: &Path) -> (Option<u64>, String) {
    let (plan, _) = reclaim_plan_for(tree);
    let estimate =
        estimate_reclaimable(&plan, tree, &StdSeedOps::new(NoGitAnswers).measuring(true));
    (estimate.bytes, render_reclaim_estimate(estimate))
}
