//! The Git rights of a project execution: a verified private layout inside a
//! shared repository, and the Git configuration the execution runs with.
//!
//! A seat's worktree is a *linked* worktree: its commits land in the project
//! repository's shared object store and its branch lives in the shared refs.
//! Handing the seat the whole shared `.git` would let it move other seats'
//! branches, rewrite the repository configuration or hooks, or delete objects
//! everyone depends on. So the grants are narrow, and each was measured with
//! real `git` under the boundary (2026-09-26, `probes/git_layout_probe.sh`):
//!
//! | Path | Right | Why |
//! | --- | --- | --- |
//! | shared `.git` | read | history, config, hooks, other refs |
//! | `.git/worktrees/<this>` | read-write | this worktree's HEAD, index, logs |
//! | `.git/objects` | read-write | the project's ordinary shared object store: its own seats and the operator's checkout already share it by Git's design |
//! | `.git/lfs` | read-write | the project's Git LFS objects, which checkout and fetch write, same-project like `objects` |
//! | `.git/objects/info/alternates` | no write | a seat cannot make the project borrow another repository's objects |
//! | `refs/heads/<this branch>` (+ log, locks) | read-write | this worktree's branch |
//! | `refs/remotes/**` (+ logs) | read-write | fetch and push results — **shared**: a seat can move the project's local remote-tracking refs, which every fetch re-derives from the remote (disclosed) |
//!
//! Measured refusals: updating another branch, creating a new branch,
//! writing shared config, writing a hook, writing `packed-refs`. A repository
//! that borrows objects from another (`objects/info/alternates`) is refused
//! outright. The object store is ordinary same-project storage, not a
//! protected archive: a process of this project can damage this project's own
//! history, and repository maintenance that rewrites `packed-refs` or other
//! seats' refs (`git gc`, `git pack-refs`) does not complete from inside a
//! seat.

use std::path::{Path, PathBuf};

use buzz_acp::exec_boundary::{Access, Grant};

use crate::execution_scope::{canonical, refuse, EXECUTION_SCOPE_INVALID};
use crate::session::CreateFailure;

/// For a linked worktree, the Git administration it may use — verified to
/// belong to this worktree and (when known) to this project's repository.
/// `None` when the tree holds its own `.git` directory (the tree grant covers
/// it) or no repository at all.
/// `pinned_branch` is the branch the host recorded for this worktree when it
/// first prepared it (see [`crate::execution_scope::ExecutionBinding`]); when
/// present it — not the child-writable `HEAD` — decides which branch ref is
/// writable, so a seat cannot widen its own grant by re-pointing `HEAD`.
/// `host_branch` is a branch a host-run operation (never the child) decided
/// this worktree moves to — its ref is granted beside the current one.
pub(crate) fn linked_worktree_admin(
    tree: &Path,
    project_checkout: Option<&Path>,
    pinned_branch: Option<&str>,
    host_branch: Option<&str>,
) -> Result<Option<Vec<Grant>>, CreateFailure> {
    let dot_git = tree.join(".git");
    if dot_git.is_dir() {
        refuse_alternates(&dot_git)?;
        return Ok(None);
    }
    if !dot_git.exists() {
        return Ok(None);
    }
    let invalid = |what: &str| {
        refuse(
            EXECUTION_SCOPE_INVALID,
            format!("the working tree's Git administration cannot be verified: {what}"),
        )
    };
    let text = std::fs::read_to_string(&dot_git).map_err(|_| invalid("unreadable .git file"))?;
    let admin = text
        .trim()
        .strip_prefix("gitdir:")
        .map(str::trim)
        .map(|dir| {
            let dir = Path::new(dir);
            if dir.is_absolute() {
                dir.to_path_buf()
            } else {
                tree.join(dir)
            }
        })
        .and_then(|dir| canonical(&dir))
        .ok_or_else(|| invalid("the .git file names no admin directory"))?;
    let common = std::fs::read_to_string(admin.join("commondir"))
        .ok()
        .map(|dir| admin.join(dir.trim()))
        .and_then(|dir| canonical(&dir))
        .ok_or_else(|| invalid("no common directory"))?;
    if !admin.starts_with(common.join("worktrees")) {
        return Err(invalid(
            "the admin directory is not one of its repository's worktrees",
        ));
    }
    let back_link = std::fs::read_to_string(admin.join("gitdir"))
        .ok()
        .and_then(|link| canonical(Path::new(link.trim())));
    if back_link.as_deref() != canonical(&dot_git).as_deref() {
        return Err(invalid("the repository does not name this worktree back"));
    }
    if let Some(checkout) = project_checkout.and_then(canonical) {
        if canonical(&checkout.join(".git")).as_deref() != Some(common.as_path()) {
            return Err(refuse(
                EXECUTION_SCOPE_INVALID,
                "the working tree is a worktree of a repository other than this project's \
                 checkout",
            ));
        }
    }
    refuse_alternates(&common)?;
    let objects = common.join("objects");
    let mut grants = vec![
        Grant::tree(
            &common,
            Access::ReadOnly,
            "this project's repository (read)",
        ),
        Grant::tree(
            &admin,
            Access::ReadWrite,
            "this worktree's own Git administration",
        ),
        Grant::tree(
            &objects,
            Access::ReadWrite,
            "this project's shared Git object store",
        ),
        Grant::tree(
            common.join("lfs"),
            Access::ReadWrite,
            "this project's shared Git LFS object store",
        ),
        Grant::file(
            objects.join("info/alternates"),
            Access::NoWrite,
            "no borrowing of another repository's objects",
        ),
        Grant::tree(
            common.join("refs/remotes"),
            Access::ReadWrite,
            "remote-tracking refs (shared; re-derived by fetch)",
        ),
        Grant::tree(
            common.join("logs/refs/remotes"),
            Access::ReadWrite,
            "remote-tracking ref logs",
        ),
    ];
    let head_branch = pinned_branch
        .map(str::to_owned)
        .or_else(|| head_branch_of(&admin));
    for branch in head_branch
        .iter()
        .map(String::as_str)
        .chain(host_branch)
        .filter(|branch| valid_branch(branch))
    {
        grants.extend(branch_ref_grants(&common, branch));
    }
    Ok(Some(grants))
}

/// The branch a worktree administration's `HEAD` names, if it names one.
fn head_branch_of(admin: &Path) -> Option<String> {
    std::fs::read_to_string(admin.join("HEAD"))
        .ok()
        .and_then(|head| {
            head.trim()
                .strip_prefix("ref: refs/heads/")
                .map(str::to_owned)
        })
        .filter(|branch| valid_branch(branch))
}

/// The branch a working tree is on: its linked administration's `HEAD`, or
/// its own `.git/HEAD`. Read by the host when it first prepares the tree —
/// before any child has run in it — and then pinned.
pub(crate) fn worktree_branch(tree: &Path) -> Option<String> {
    let dot_git = tree.join(".git");
    if dot_git.is_dir() {
        return head_branch_of(&dot_git);
    }
    let text = std::fs::read_to_string(&dot_git).ok()?;
    let dir = Path::new(text.trim().strip_prefix("gitdir:")?.trim());
    let admin = if dir.is_absolute() {
        dir.to_path_buf()
    } else {
        tree.join(dir)
    };
    head_branch_of(&admin)
}

/// The ref, reflog and lock files of one branch, and the directories a
/// hierarchical branch name needs.
fn branch_ref_grants(common: &Path, branch: &str) -> Vec<Grant> {
    let mut grants = Vec::new();
    for base in [common.join("refs/heads"), common.join("logs/refs/heads")] {
        let file = base.join(branch);
        let mut parent = file.parent().map(Path::to_path_buf);
        while let Some(dir) = parent.filter(|dir| dir != &base && dir.starts_with(&base)) {
            grants.push(Grant::file(
                &dir,
                Access::ReadWrite,
                "directory of this worktree's branch ref",
            ));
            parent = dir.parent().map(Path::to_path_buf);
        }
        grants.push(Grant::file(
            &file,
            Access::ReadWrite,
            "this worktree's branch ref",
        ));
        grants.push(Grant::file(
            PathBuf::from(format!("{}.lock", file.display())),
            Access::ReadWrite,
            "this worktree's branch ref lock",
        ));
    }
    grants
}

fn valid_branch(branch: &str) -> bool {
    !branch.is_empty()
        && !branch.starts_with('/')
        && !branch
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
}

/// A repository that borrows another repository's objects cannot be kept
/// apart from it.
fn refuse_alternates(git_dir: &Path) -> Result<(), CreateFailure> {
    let alternates = git_dir.join("objects/info/alternates");
    match std::fs::read_to_string(&alternates) {
        Ok(text) if text.lines().any(|line| !line.trim().is_empty()) => Err(refuse(
            EXECUTION_SCOPE_INVALID,
            "this repository borrows objects from another repository (objects/info/alternates), \
             so a project execution in it cannot be kept apart from that repository",
        )),
        _ => Ok(()),
    }
}

/// Global Git settings a project execution keeps: who commits, how pushes
/// authenticate, and the filter drivers checkout needs to write correct
/// content (Git LFS installs its driver here). Everything else in the
/// operator's global Git configuration (aliases, include paths, `url.*`
/// rewrites, other projects' settings) stays out.
const STAGED_GIT_KEYS: &str = r"^(user\.(name|email)|credential\..*|filter\..*)$";

/// The execution's staged Git configuration and what running it needs.
#[derive(Debug)]
pub(crate) struct StagedGit {
    /// The file `GIT_CONFIG_GLOBAL` names, in the execution's control
    /// directory (read-only to the child).
    pub config: PathBuf,
    /// Credential-helper and filter-driver programs the staged file runs, by
    /// absolute path.
    pub helpers: Vec<PathBuf>,
    /// The operator's Git signing key file an unseated execution pushes
    /// with (`nostr.keyfile`); never for a seat, which carries its own key.
    pub keyfile: Option<PathBuf>,
}

/// Write the execution's own global Git configuration into `control`.
///
/// Read host-side from the operator's system and global configuration, in
/// that order (Git's own), limited to [`STAGED_GIT_KEYS`]; the execution
/// never reads the operator's files, and runs with system configuration off
/// (`GIT_CONFIG_NOSYSTEM`) so the staged file is the whole selection — a
/// system-only helper the operator relies on is carried over, not dropped,
/// and nothing implicit is added. Each
/// credential helper is resolved on the host to the absolute program it runs
/// — a bare `nostr` becomes the host's `git-credential-nostr` — so which
/// helper runs never depends on the child's `PATH`, and a same-named tool in a
/// project cannot stand in for it. For an unseated execution the configured
/// `nostr.keyfile` is staged too: that is how the operator's own Git transport
/// authenticates when no seat key is in the environment.
///
/// `tool_dir` is where the host's own `bee` lives: the app bundle ships
/// `git-credential-nostr` beside it.
pub(crate) fn stage_git_config(
    control: &Path,
    unseated: bool,
    tool_dir: Option<&Path>,
) -> Result<StagedGit, CreateFailure> {
    let config = control.join("gitconfig");
    let unavailable = |error: std::io::Error| {
        refuse(
            crate::execution_scope::EXECUTION_BOUNDARY_UNAVAILABLE,
            format!("could not stage the execution's Git configuration: {error}"),
        )
    };
    // Built under a name no other preparation uses and renamed into place:
    // a Git already running in this execution reads the old file or the new
    // one, never a truncated one, and two preparations never share Git's
    // config lock.
    static STAGING: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let building = control.join(format!(
        ".gitconfig-{}-{}",
        std::process::id(),
        STAGING.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    // Repository maintenance is the host's, not an execution's: the detached
    // `git maintenance run --auto` a commit starts would try to repack the
    // project's shared refs, which the boundary refuses, and outlive the turn.
    crate::execution_scope::write_host_file(&building, STAGED_GIT_BASE, false)?;
    let staged = fill_git_config(&building, unseated, tool_dir).and_then(|staged| {
        std::fs::rename(&building, &config)
            .map(|()| staged)
            .map_err(unavailable)
    });
    if staged.is_err() {
        let _ = std::fs::remove_file(&building);
    }
    staged.map(|staged| StagedGit { config, ..staged })
}

/// What every staged Git configuration starts with, before the operator's
/// selected entries are added.
const STAGED_GIT_BASE: &[u8] = b"[maintenance]\n\tauto = false\n[gc]\n\tauto = 0\n";

fn fill_git_config(
    config: &Path,
    unseated: bool,
    tool_dir: Option<&Path>,
) -> Result<StagedGit, CreateFailure> {
    let unavailable = |error: std::io::Error| {
        refuse(
            crate::execution_scope::EXECUTION_BOUNDARY_UNAVAILABLE,
            format!("could not stage the execution's Git configuration: {error}"),
        )
    };
    let mut staged = StagedGit {
        config: config.to_path_buf(),
        helpers: Vec::new(),
        keyfile: None,
    };
    let mut entries: Vec<(String, String)> =
        operator_git_config(&["--get-regexp", STAGED_GIT_KEYS]);

    if unseated {
        let keyfile = operator_git_config(&["--get", "nostr.keyfile"])
            .into_iter()
            .next()
            .map(|(_, value)| expand_home(&value))
            .and_then(|path| canonical(&path))
            .filter(|path| path.is_file());
        if let Some(keyfile) = keyfile {
            entries.push(("nostr.keyfile".to_owned(), keyfile.display().to_string()));
            staged.keyfile = Some(keyfile);
        }
    }
    // Where Git and the host look for a helper named by its short name: Git's
    // own helper directory, the host's PATH, and beside the host's `bee` (the
    // app bundle ships `git-credential-nostr` there).
    let helper_dirs: Vec<PathBuf> = git_exec_path()
        .into_iter()
        .chain(
            std::env::var_os("PATH")
                .iter()
                .flat_map(std::env::split_paths),
        )
        .chain(tool_dir.map(Path::to_path_buf))
        .collect();
    for (key, value) in entries {
        let value = if is_helper_key(&key) {
            let (value, program) = resolve_helper(&value, &helper_dirs);
            staged.helpers.extend(program);
            value
        } else if is_filter_command_key(&key) {
            let (value, program) = resolve_command_program(&value, &helper_dirs);
            staged.helpers.extend(program);
            value
        } else {
            value
        };
        let status = std::process::Command::new("git")
            .args(["config", "--file"])
            .arg(config)
            .args(["--add", &key, &value])
            .status()
            .map_err(unavailable)?;
        if !status.success() {
            return Err(refuse(
                crate::execution_scope::EXECUTION_BOUNDARY_UNAVAILABLE,
                format!("could not stage the Git setting {key}"),
            ));
        }
    }
    Ok(staged)
}

fn is_filter_command_key(key: &str) -> bool {
    key.starts_with("filter.")
        && [".clean", ".smudge", ".process"]
            .iter()
            .any(|suffix| key.ends_with(suffix))
}

/// A filter command with its program named absolutely, and that program.
fn resolve_command_program(value: &str, dirs: &[PathBuf]) -> (String, Option<PathBuf>) {
    let Some((program, rest)) = leading_word(value.trim()) else {
        return (value.to_owned(), None);
    };
    let resolved = if Path::new(&program).is_absolute() {
        canonical(Path::new(&program)).map(|_| PathBuf::from(&program))
    } else if program.is_empty() || program.contains('/') {
        None
    } else {
        dirs.iter()
            .map(|dir| dir.join(&program))
            .find(|path| path.is_file())
    };
    let Some(resolved) = resolved else {
        return (value.to_owned(), None);
    };
    (with_rest(shell_quoted(&resolved), rest), Some(resolved))
}

/// The program word at the start of a command Git runs through the shell,
/// and what follows it: a bare word, one in `"…"` with nothing to expand, or
/// one in `'…'` — including the `'\''` joins [`shell_quoted`] (and the
/// desktop host) write for an apostrophe. Anything more elaborate is left as
/// the operator wrote it.
fn leading_word(command: &str) -> Option<(String, &str)> {
    let separated = |rest: &str| rest.is_empty() || rest.starts_with(char::is_whitespace);
    match command.chars().next() {
        Some('\'') => {
            let mut word = String::new();
            let mut rest = command;
            loop {
                let body = rest.strip_prefix('\'')?;
                let close = body.find('\'')?;
                word.push_str(&body[..close]);
                rest = &body[close + 1..];
                match rest.strip_prefix("\\'") {
                    Some(joined) if joined.starts_with('\'') => {
                        word.push('\'');
                        rest = joined;
                    }
                    _ => break,
                }
            }
            separated(rest).then_some((word, rest))
        }
        Some('"') => {
            let close = command[1..].find('"')? + 1;
            let word = &command[1..close];
            let rest = &command[close + 1..];
            (!word.contains(['\\', '$', '`']) && separated(rest)).then(|| (word.to_owned(), rest))
        }
        Some(_) => {
            let (word, rest) = command
                .split_once(char::is_whitespace)
                .unwrap_or((command, ""));
            (!word.contains(['\'', '"', '\\'])).then(|| (word.to_owned(), rest))
        }
        None => None,
    }
}

/// A path as one shell word, in the grammar the desktop host already writes
/// for its own helper (`credential_helper_config_value`): single-quoted, so
/// spaces and shell characters in an install path stay literal.
pub(crate) fn shell_quoted(path: &Path) -> String {
    let normalized = path.to_string_lossy().replace('\\', "/");
    format!("'{}'", normalized.replace('\'', "'\\''"))
}

fn with_rest(mut command: String, rest: &str) -> String {
    let rest = rest.trim();
    if !rest.is_empty() {
        command.push(' ');
        command.push_str(rest);
    }
    command
}

fn is_helper_key(key: &str) -> bool {
    key == "credential.helper" || (key.starts_with("credential.") && key.ends_with(".helper"))
}

/// `(key, value)` pairs from the operator's system, then global, Git
/// configuration (a test's disposable file stands in for both).
fn operator_git_config(query: &[&str]) -> Vec<(String, String)> {
    let scopes = operator_config_scopes();
    let single = query.first() == Some(&"--get");
    let mut entries = Vec::new();
    for scope in scopes {
        entries.extend(read_git_config(&scope, query));
    }
    if single {
        // A later scope wins for a single value, as in Git.
        entries.into_iter().last().into_iter().collect()
    } else {
        entries
    }
}

fn read_git_config(scope: &[std::ffi::OsString], query: &[&str]) -> Vec<(String, String)> {
    let mut read = std::process::Command::new("git");
    read.arg("config").args(scope).arg("--null").args(query);
    for var in crate::git_probe::GIT_REPO_SELECTION_VARS {
        read.env_remove(var);
    }
    read.stdin(std::process::Stdio::null());
    let Ok(output) = read.output() else {
        return Vec::new();
    };
    // Exit 1 means no matching keys.
    let single = query.first() == Some(&"--get");
    output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
        .filter_map(|entry| {
            let entry = String::from_utf8_lossy(entry);
            if single {
                Some((
                    query.get(1).copied().unwrap_or_default().to_owned(),
                    entry.into_owned(),
                ))
            } else {
                entry
                    .split_once('\n')
                    .map(|(key, value)| (key.to_owned(), value.to_owned()))
            }
        })
        .collect()
}

#[cfg(test)]
thread_local! {
    /// A disposable file standing in for the operator's system and global
    /// Git configuration, for tests on this thread only. Unset, a test sees
    /// none.
    pub(crate) static TEST_OPERATOR_CONFIG: std::cell::RefCell<Option<PathBuf>> =
        const { std::cell::RefCell::new(None) };
}

fn operator_config_scopes() -> Vec<Vec<std::ffi::OsString>> {
    // Under test the person's own system and global configuration is never
    // read — its helpers (a Keychain helper among them) must not be reached
    // by a fixture; a test that needs one supplies a disposable file.
    #[cfg(test)]
    return TEST_OPERATOR_CONFIG
        .with(|file| file.borrow().clone())
        .map(|file| vec!["--file".into(), file.into_os_string()])
        .into_iter()
        .collect();
    #[cfg(not(test))]
    vec![vec!["--system".into()], vec!["--global".into()]]
}

fn expand_home(value: &str) -> PathBuf {
    match (value.strip_prefix("~/"), crate::execution_scope::home_dir()) {
        (Some(rest), Some(home)) => home.join(rest),
        _ => PathBuf::from(value),
    }
}

/// Git's own helper directory (`git --exec-path`).
fn git_exec_path() -> Option<PathBuf> {
    let output = std::process::Command::new("git")
        .arg("--exec-path")
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| PathBuf::from(String::from_utf8_lossy(&output.stdout).trim()))
}

/// A credential helper value with its program named absolutely, and that
/// program. An empty value (a reset) is kept as is; a program the host cannot
/// find is kept as written, so Git reports it missing rather than something
/// else answering.
fn resolve_helper(value: &str, dirs: &[PathBuf]) -> (String, Option<PathBuf>) {
    let trimmed = value.trim();
    let (shell, body) = match trimmed.strip_prefix('!') {
        Some(body) => (true, body.trim_start()),
        None => (false, trimmed),
    };
    let Some((program, rest)) = leading_word(body) else {
        return (value.to_owned(), None);
    };
    let name = if shell {
        program.clone()
    } else {
        format!("git-credential-{program}")
    };
    let resolved = if Path::new(&program).is_absolute() {
        canonical(Path::new(&program)).map(|_| PathBuf::from(&program))
    } else if program.is_empty() || program.contains('/') {
        None
    } else {
        dirs.iter()
            .map(|dir| dir.join(&name))
            .find(|path| path.is_file())
    };
    let Some(resolved) = resolved else {
        return (value.to_owned(), None);
    };
    // Always the explicit shell form: Git would prefix a quoted path
    // without `!` with `git credential-`.
    (
        with_rest(format!("!{}", shell_quoted(&resolved)), rest),
        Some(resolved),
    )
}

#[cfg(test)]
#[path = "execution_scope_git_tests.rs"]
mod tests;
