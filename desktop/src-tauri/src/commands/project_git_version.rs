//! Which `git` this app may use to reach the relay, and what it says when
//! none will do.
//!
//! # The finding this module exists for
//!
//! Ledger 168. Every credentialed git operation the installed bundle ran
//! after a relaunch failed with `fatal: could not read Username for
//! 'https://hive.agiterra.org/git/…': terminal prompts disabled` — the
//! re-stage of 43 recorded seats, then two hires whose pack source pins a
//! branch and so must fetch. Nothing was wrong with the credential: the
//! helper (`crates/git-credential-nostr/src/lib.rs`) answers over git's
//! `authtype` credential protocol and prints nothing at all when git does
//! not announce `capability[]=authtype`, and that capability exists only
//! from git 2.46. A Finder-launched bundle has `PATH` =
//! `/usr/bin:/bin:/usr/sbin:/sbin`, so `resolve_command("git")` found
//! Apple's git 2.39.5, which never asks the helper for a credential and
//! falls straight through to the username prompt. `tauri dev` from a
//! terminal inherits a `PATH` with Homebrew's git on it and never sees this.
//!
//! So a remote operation does not take whatever `git` `PATH` happens to
//! offer. It takes the first git on a fixed candidate list that answers
//! `--version` with 2.46 or newer, and when none does it refuses in one
//! sentence naming the git it found, its version, the requirement and the
//! remedy — instead of letting git produce a username error about a
//! credential that was never the problem.
//!
//! Local-only operations are untouched: they need no helper, so any git will
//! do, and a machine with only Apple's git keeps working for everything that
//! does not talk to the relay.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::managed_agents::resolve_command;

/// The first git that implements the `authtype` credential protocol
/// `git-credential-nostr` answers over.
pub(crate) const MINIMUM_REMOTE_GIT: GitVersion = GitVersion {
    major: 2,
    minor: 46,
    patch: 0,
};

/// How long a single `git --version` may take before it is killed.
///
/// `/usr/bin/git` on a Mac without the Command Line Tools puts up an
/// installer dialog and waits, so this probe is never allowed to wait
/// forever on it.
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// What to tell someone whose git is too old, on this platform.
#[cfg(target_os = "macos")]
const INSTALL_HINT: &str = "Install it with `brew install git` and relaunch.";
#[cfg(not(target_os = "macos"))]
const INSTALL_HINT: &str = "Install git 2.46 or newer and relaunch.";

/// A three-number git version, ordered as git orders it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct GitVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl GitVersion {
    /// `major.minor`, the form a requirement is stated in.
    ///
    /// The capability landed in 2.46.0 and the patch number carries no
    /// meaning for it, so "git 2.46 or newer" is what a person is told and
    /// what the surface renders — one spelling, from one place.
    pub(crate) fn short(&self) -> String {
        format!("{}.{}", self.major, self.minor)
    }
}

impl std::fmt::Display for GitVersion {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Read a version out of whatever `git --version` printed.
///
/// Accepts the bare number and the full line, and every vendor's trailing
/// noise: Apple's `git version 2.39.5 (Apple Git-154)`, Git for Windows'
/// `2.47.0.windows.1`, a plain `2.55.0`. A line that carries no leading
/// number is `None` — an unparsable version is never read as a capable one.
pub(crate) fn parse_git_version(output: &str) -> Option<GitVersion> {
    let text = output.trim();
    let rest = text.strip_prefix("git version").unwrap_or(text).trim();
    let token = rest.split_whitespace().next()?;
    let mut parts = token.split('.');
    let major = parts.next()?.parse::<u32>().ok()?;
    // A missing component is zero; a present one that is not a number ends
    // the version, because `2.47.0.windows.1` means 2.47.0 and nothing else.
    let minor = parts.next().and_then(|part| part.parse::<u32>().ok());
    let patch = minor
        .and(parts.next())
        .and_then(|part| part.parse::<u32>().ok());
    Some(GitVersion {
        major,
        minor: minor.unwrap_or(0),
        patch: patch.unwrap_or(0),
    })
}

/// What a probe of every candidate found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct GitSelection {
    /// The first candidate at or above [`MINIMUM_REMOTE_GIT`].
    pub capable: Option<(PathBuf, GitVersion)>,
    /// The first candidate that answered `--version` at all, capable or not.
    ///
    /// This is what a refusal names: "git 2.39.5 at /usr/bin/git" tells the
    /// reader which git the app is actually reaching, which is the whole
    /// content of this bug.
    pub found: Option<(PathBuf, GitVersion)>,
}

/// Where to look for a git, in order.
///
/// The resolved `PATH` git first — on a terminal-launched build or a machine
/// whose `PATH` is set up, that is already the right answer and nothing is
/// spawned twice. Then the well-known installs a Finder-launched bundle
/// cannot see, then the repository's hermit `bin/git` for a workspace build.
/// A candidate that does not exist simply fails to answer and costs one
/// failed spawn; nothing here consults a login shell, which is how a
/// `PATH`-discovery hack would have been written and is exactly the thing
/// this module refuses to do.
pub(crate) fn candidate_gits(path_git: Option<PathBuf>, workspace: Option<&Path>) -> Vec<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    let mut push = |candidate: PathBuf| {
        if !candidates.contains(&candidate) {
            candidates.push(candidate);
        }
    };
    if let Some(path_git) = path_git {
        push(path_git);
    }
    for well_known in [
        "/opt/homebrew/bin/git",
        "/usr/local/bin/git",
        "/opt/homebrew/opt/git/bin/git",
    ] {
        push(PathBuf::from(well_known));
    }
    if let Some(workspace) = workspace {
        push(workspace.join("bin").join("git"));
    }
    candidates
}

/// Probe `candidates` in order and report both the first capable git and the
/// first git that answered at all.
pub(crate) fn select_git(
    candidates: &[PathBuf],
    probe: impl Fn(&Path) -> Option<GitVersion>,
) -> GitSelection {
    let mut selection = GitSelection::default();
    for candidate in candidates {
        let Some(version) = probe(candidate) else {
            continue;
        };
        if selection.found.is_none() {
            selection.found = Some((candidate.clone(), version));
        }
        if version >= MINIMUM_REMOTE_GIT {
            selection.capable = Some((candidate.clone(), version));
            return selection;
        }
    }
    selection
}

/// The one sentence a remote operation fails with when no git qualifies.
///
/// Named after the git that *is* there, because "install git" to someone who
/// has git reads as nonsense; the version and the path are what make it
/// actionable. Kept pure, and given its hint, so the wording is testable
/// without a platform.
pub(crate) fn remote_git_refusal(found: Option<(&Path, GitVersion)>, install_hint: &str) -> String {
    match found {
        Some((path, version)) => format!(
            "git {version} at {} cannot authenticate to the relay: the Nostr credential \
             helper needs git {} or newer. {install_hint}",
            path.display(),
            MINIMUM_REMOTE_GIT.short()
        ),
        None => format!(
            "no git was found, so nothing can authenticate to the relay: the Nostr \
             credential helper needs git {} or newer. {install_hint}",
            MINIMUM_REMOTE_GIT.short()
        ),
    }
}

/// Ask one candidate what version it is.
///
/// Returns `None` for anything that is not a git that answered: a path that
/// does not exist, a non-executable, a non-zero exit, a hang, or output with
/// no version in it.
fn probe_git_version(path: &Path) -> Option<GitVersion> {
    let mut command = Command::new(path);
    command
        .arg("--version")
        // The same prompt suppression every other spawn here uses: a probe
        // must never be the thing that blocks on a terminal.
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    crate::util::configure_no_window(&mut command);
    let mut child = command.spawn().ok()?;
    let deadline = Instant::now() + PROBE_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(_)) | Err(_) => return None,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
        }
    }
    let mut output = String::new();
    {
        use std::io::Read as _;
        child.stdout.as_mut()?.read_to_string(&mut output).ok()?;
    }
    parse_git_version(&output)
}

/// The probe, run once for the life of the app.
///
/// Cached because every hire, every seat re-stage and every packs sync would
/// otherwise spawn the whole candidate list again, and because a git does not
/// change version under a running app. `resolve_command` caches its own half
/// of the answer for the same reason.
fn selection() -> &'static GitSelection {
    static SELECTION: OnceLock<GitSelection> = OnceLock::new();
    SELECTION.get_or_init(|| {
        let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        select_git(
            &candidate_gits(resolve_command("git"), Some(&workspace)),
            probe_git_version,
        )
    })
}

/// The git a credentialed operation may use, or the sentence it fails with.
pub(crate) fn remote_capable_git() -> Result<PathBuf, String> {
    let selection = selection();
    match &selection.capable {
        Some((path, _)) => Ok(path.clone()),
        None => Err(remote_git_refusal(
            selection
                .found
                .as_ref()
                .map(|(path, version)| (path.as_path(), *version)),
            INSTALL_HINT,
        )),
    }
}

/// What the app can say about the git it would reach the relay with.
///
/// Every field is a disclosed answer or a disclosed absence: a machine with
/// no git at all reports `null` path and version with `meetsMinimum: false`,
/// never an empty string and never a guess.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GitCapability {
    /// The chosen git when one qualifies, otherwise the git that was found.
    pub path: Option<String>,
    /// That git's version, `null` when nothing answered.
    pub version: Option<String>,
    /// Whether the reported git can authenticate to the relay.
    pub meets_minimum: bool,
    /// The requirement, so the surface never hard-codes a second copy.
    pub minimum: String,
}

/// Build the disclosure from a selection. Pure, so the surface's wording can
/// be tested against every case without a git on the machine.
pub(crate) fn describe_selection(selection: &GitSelection) -> GitCapability {
    let reported = selection.capable.as_ref().or(selection.found.as_ref());
    GitCapability {
        path: reported.map(|(path, _)| path.display().to_string()),
        version: reported.map(|(_, version)| version.to_string()),
        meets_minimum: selection.capable.is_some(),
        minimum: MINIMUM_REMOTE_GIT.short(),
    }
}

/// The git this app would use to reach the relay, for the About row.
#[tauri::command]
pub fn get_git_capability() -> GitCapability {
    describe_selection(selection())
}

#[cfg(test)]
#[path = "project_git_version_tests.rs"]
mod tests;
