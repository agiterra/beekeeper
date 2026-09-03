//! Staging, probing, and the line between what the subject may touch and what
//! grades it.
//!
//! Split out of [`super`] only to keep both files under the 1,000-line rule and
//! re-exported whole, so no caller's path changes. The division is real: this
//! file is the harness's entire impure half — it copies trees, spawns probe
//! scripts and reads a disk — and everything after it
//! ([`buzz_core::registry_bench::score_run`]) is a pure function of what it
//! returns. That boundary is what makes two scorings of one run agree.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use buzz_core::registry_bench::{Check, Criterion, RunArtifacts};
use sha2::{Digest, Sha256};

use crate::error::CliError;

use super::RawRun;

/// Copy a task's `fixture/` into a fresh scratch directory.
///
/// The fixture is copied, never linked, so a run that scribbles on it cannot
/// change what the next run sees.
///
/// # Errors
///
/// [`CliError::Other`] naming the path that would not copy.
pub fn stage_fixture(fixture_dir: &Path, scratch: &Path) -> Result<(), CliError> {
    std::fs::create_dir_all(scratch).map_err(|error| {
        CliError::Other(format!("cannot create {}: {error}", scratch.display()))
    })?;
    if !fixture_dir.is_dir() {
        return Ok(());
    }
    // Flat, and **only** flat: these are the working files the task edits. The
    // graders used to be copied to `scratch/fixture/` as well, which put every
    // check script inside the directory the measured agent is handed as its
    // cwd — see [`stage_graders`].
    copy_tree(fixture_dir, scratch)
}

/// The check scripts, staged read-only **outside** the run's scratch dir, with
/// the hash the bench manifest says each of them should have.
///
/// F1, and it is the whole reason this type exists: the graders used to live in
/// `scratch/fixture/`, which is the directory the measured agent is given as
/// its cwd and writes its answer into. A stub adapter one line longer than the
/// honest one —
///
/// ```sh
/// printf '#!/bin/sh\nexit 0\n' > fixture/no-false-positives.sh
/// ```
///
/// — took `judgment` from **2.6 to 5.0**, `outcome: passed`, zero spread,
/// nothing unstable, and those rows would have been published as signed
/// evidence of a perfect run. The subject in a live run is an agent with shell
/// and file-edit tools; this was not theoretical.
///
/// Two defences, because one of them is a permission bit:
///
/// 1. the graders are copied to a sibling directory the subject is never told
///    about and never given as cwd, and every file in it is chmod'd read-only;
/// 2. **immediately before each script runs**, its bytes are hashed and
///    compared to the bench manifest. A mismatch refuses the whole run, naming
///    the file — it does not fail the criterion, because a tampered grader
///    means no number from this run can be trusted.
#[derive(Debug, Clone)]
pub struct GraderSet {
    /// The read-only staged copy, outside the scratch dir.
    pub dir: PathBuf,
    /// Grader-relative path → the sha256 the bench manifest says it has.
    pub hashes: BTreeMap<String, String>,
}

impl GraderSet {
    /// Resolve a rubric's `script` path and verify its bytes before it runs.
    ///
    /// # Errors
    ///
    /// [`CliError::Other`] naming the file when it is missing from the
    /// manifest, missing on disk, or hashes differently than the manifest says.
    pub fn verify(&self, script: &str) -> Result<PathBuf, CliError> {
        if script.contains("..") {
            return Err(CliError::Usage(format!(
                "grader {script:?} climbs out of its bench directory"
            )));
        }
        // Rubrics address their graders as `fixture/<name>`; the staged copy is
        // that `fixture/` directory itself.
        let relative = script.strip_prefix("fixture/").unwrap_or(script);
        let path = self.dir.join(relative);
        let Some(expected) = self.hashes.get(relative) else {
            return Err(CliError::Other(format!(
                "grader {script:?} is not in the bench manifest for this task: refusing the run \
                 rather than executing a script nothing vouches for"
            )));
        };
        let bytes = std::fs::read(&path).map_err(|error| {
            CliError::Other(format!("cannot read grader {}: {error}", path.display()))
        })?;
        let actual = hex::encode(Sha256::digest(&bytes));
        if actual != *expected {
            return Err(CliError::Other(format!(
                "grader {script:?} does not match the bench manifest ({} on disk, {} in the \
                 manifest): the run is refused — a measurement whose grader moved is not a \
                 measurement",
                &actual[..12.min(actual.len())],
                &expected[..12.min(expected.len())]
            )));
        }
        Ok(path)
    }
}

/// Copy a task's `fixture/` to a read-only staging directory outside `scratch`.
///
/// # Errors
///
/// [`CliError::Other`] naming the path that would not copy.
pub fn stage_graders(
    fixture_dir: &Path,
    graders_dir: &Path,
    manifest: &BTreeMap<String, Vec<u8>>,
    task_id: &str,
) -> Result<GraderSet, CliError> {
    // A stale staging dir from an earlier run would defeat the point.
    let _ = std::fs::remove_dir_all(graders_dir);
    let mut hashes = BTreeMap::new();
    for (path, bytes) in manifest {
        let Some(relative) = path
            .strip_prefix(&format!("{task_id}/fixture/"))
            .filter(|rest| !rest.is_empty())
        else {
            continue;
        };
        hashes.insert(relative.to_owned(), hex::encode(Sha256::digest(bytes)));
    }
    if fixture_dir.is_dir() {
        copy_tree(fixture_dir, graders_dir)?;
        make_read_only(graders_dir)?;
    }
    Ok(GraderSet {
        dir: graders_dir.to_path_buf(),
        hashes,
    })
}

/// Strip every write bit from a staged grader tree.
fn make_read_only(dir: &Path) -> Result<(), CliError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let entries = std::fs::read_dir(dir)
            .map_err(|error| CliError::Other(format!("cannot read {}: {error}", dir.display())))?;
        for entry in entries {
            let entry = entry.map_err(|error| {
                CliError::Other(format!("cannot read {}: {error}", dir.display()))
            })?;
            let path = entry.path();
            if path.is_dir() {
                make_read_only(&path)?;
                continue;
            }
            if let Ok(metadata) = std::fs::metadata(&path) {
                // Keep the execute bits, drop every write bit.
                let mode = metadata.permissions().mode() & 0o555;
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode));
            }
        }
    }
    #[cfg(not(unix))]
    let _ = dir;
    Ok(())
}

fn copy_tree(from: &Path, to: &Path) -> Result<(), CliError> {
    std::fs::create_dir_all(to)
        .map_err(|error| CliError::Other(format!("cannot create {}: {error}", to.display())))?;
    let entries = std::fs::read_dir(from)
        .map_err(|error| CliError::Other(format!("cannot read {}: {error}", from.display())))?;
    for entry in entries {
        let entry = entry
            .map_err(|error| CliError::Other(format!("cannot read {}: {error}", from.display())))?;
        let source = entry.path();
        let target = to.join(entry.file_name());
        if source.is_dir() {
            copy_tree(&source, &target)?;
            continue;
        }
        std::fs::copy(&source, &target).map_err(|error| {
            CliError::Other(format!("cannot copy {}: {error}", source.display()))
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            if let Ok(metadata) = std::fs::metadata(&source) {
                let _ = std::fs::set_permissions(
                    &target,
                    std::fs::Permissions::from_mode(metadata.permissions().mode()),
                );
            }
        }
    }
    Ok(())
}

/// Read the artifacts the criteria name, and answer the two checks that need a
/// process.
///
/// This is the whole of the harness's impure half. Everything after it —
/// [`score_run`] — is a pure function of what this returns, which is what makes
/// two scorings of one run agree.
pub fn collect_artifacts(
    scratch: &Path,
    graders: &GraderSet,
    raw: &RawRun,
    criteria: &[Criterion],
    timeout_secs: u64,
) -> Result<RunArtifacts, CliError> {
    let mut files = BTreeMap::new();
    let mut probes = BTreeMap::new();
    for criterion in criteria {
        match &criterion.check {
            Check::FileContains(check) => read_into(scratch, &check.path, &mut files),
            Check::FileAbsent(check) => read_into(scratch, &check.path, &mut files),
            Check::JsonPathEquals(check) => read_into(scratch, &check.path, &mut files),
            Check::Command(check) => {
                // Verified against the manifest, then run from the read-only
                // staged copy with the scratch dir as cwd. A mismatch is an
                // error, not a failed criterion: see [`GraderSet`].
                let script = graders.verify(&check.script)?;
                probes.insert(criterion.id.clone(), run_script(scratch, graders, &script));
            }
            Check::DiffApplies(check) => {
                // The patch is the SUBJECT'S artifact, so it is read from the
                // scratch dir on purpose; the grader here is `git apply`, which
                // the subject cannot rewrite.
                probes.insert(criterion.id.clone(), diff_applies(scratch, &check.path));
            }
            Check::ExitCode(_) | Check::StdoutMatches(_) => {}
        }
    }
    Ok(RunArtifacts {
        exit_code: raw.exit_code,
        stdout: raw.stdout.clone(),
        files,
        probes,
        timed_out: raw.timed_out,
        duration_ms: raw.duration_ms,
        timeout_secs,
    })
}

fn read_into(scratch: &Path, relative: &str, files: &mut BTreeMap<String, String>) {
    // A path that climbs out of the scratch dir reads as absent rather than as
    // a file: a rubric is checked-in code, but it is not a reason to read the
    // rest of the disk.
    if relative.contains("..") {
        return;
    }
    if let Ok(body) = std::fs::read_to_string(scratch.join(relative)) {
        files.insert(relative.to_owned(), body);
    }
}

fn run_script(scratch: &Path, graders: &GraderSet, script: &Path) -> bool {
    std::process::Command::new("/bin/sh")
        .arg(script)
        // cwd stays the scratch dir — the grader's job is to look at what the
        // subject produced — but the script itself comes from the read-only
        // staged copy, and `BENCH_FIXTURE` is how it reaches its own baselines
        // without a writable `fixture/` beside the answer.
        .current_dir(scratch)
        .env("BENCH_FIXTURE", &graders.dir)
        .status()
        .is_ok_and(|status| status.success())
}

fn diff_applies(scratch: &Path, patch: &str) -> bool {
    if patch.contains("..") {
        return false;
    }
    // Routed through the shared `git_command` helper (same one
    // `git_config_set` and `bee packs init` use) rather than a bare
    // `Command::new("git")`. Audited: `--check` alone reads the patch and the
    // files under `scratch` on disk and never consults the index or `HEAD`,
    // so an inherited `GIT_DIR` cannot presently misdirect this specific
    // call — verified by running it under a poisoned `GIT_DIR` pointed at an
    // unrelated repository and confirming the check result is unchanged.
    // Cleared anyway: `scratch` holds no `.git` of its own, and the moment a
    // future change adds `--cached`/`--index` here, an uncleared environment
    // would silently start reading a stranger's index.
    crate::commands::sessions::worktree::git_command(scratch)
        .args(["apply", "--check", patch])
        .status()
        .is_ok_and(|status| status.success())
}
