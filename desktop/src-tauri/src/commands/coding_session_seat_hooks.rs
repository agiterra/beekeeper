//! Install the wip-share git hooks into a seat's freshly cut worktree.
//!
//! This is the install site for the ruling behind the lane: **nothing in
//! Pulse's data path may depend on anyone being asked to report.** The hire
//! host has just created the seat's worktree and already knows the seat's role,
//! pubkey, key file, assignment and session. It writes the hooks here; from
//! then on the agent does nothing but `git commit` and its commits appear on
//! `refs/heads/wip/<role>/<slug>`.
//!
//! Every byte written and every config key comes from
//! [`beekeeper_core_pkg::seat_git_hooks`], so this command and `lefthook.yml`'s
//! human path install the same script.
//!
//! ## Where the config goes, and why it is not always `--local`
//!
//! A seat worktree is a **linked** git worktree (`git worktree add`, see
//! `desktop/src-tauri/src/coding_sessions/worktree.rs`), and in a linked
//! worktree `git config --local` writes the *repository's shared* config file.
//! Arming a seat with `--local` would therefore turn `buzz.wipShare` on for the
//! person's own checkout too — a control that says "this seat" while enabling
//! everybody. So:
//!
//! * in the main worktree, `--local` really is that worktree's own config and
//!   is used directly;
//! * in a linked worktree, `extensions.worktreeConfig` is enabled and the lines
//!   are written with `--worktree`, which no other worktree can read.
//!
//! Enabling that extension is the single shared line this command writes, and
//! it is refused outright when the shared config sets `core.bare = true` or
//! `core.worktree`, because those two keys change meaning when the extension
//! turns on.
//!
//! ## When there is no key file
//!
//! A seat's secret key is never written to a file this host can name — it is
//! held in the OS keyring and injected into the seat process as
//! `$NOSTR_PRIVATE_KEY`. A request with `keyfile_path: None` therefore installs
//! the two hooks and every `buzz.*` line, and omits the five signing lines
//! entirely; the report says `signing: "unsigned"` so no caller has to assume.
//! Sharing still works: `git-credential-nostr` reads `$NOSTR_PRIVATE_KEY`
//! before any key file, so the seat's push authenticates as the seat.
//!
//! `--global` and `--system` are never written: every git process spawned here
//! runs with `GIT_CONFIG_GLOBAL=/dev/null` and `GIT_CONFIG_NOSYSTEM=1`, so
//! there is no global file for a write to land in.

use std::path::{Path, PathBuf};
use std::process::Command;

use beekeeper_core_pkg::seat_commit_identity::{seat_commit_identity, SeatCommitIdentity};
use beekeeper_core_pkg::seat_git_hooks::{plan_seat_git_hooks, SeatGitHookRequest};
use serde::{Deserialize, Serialize};

use crate::commands::project_git_exec::GIT_REPO_SELECTION_VARS;

/// What the hire host knows about a seat and the worktree it just cut.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallCodingSessionSeatHooksRequest {
    /// The seat's worktree, as `create_coding_session_worktree` returned it.
    pub worktree_path: String,
    /// The seat's role word.
    pub seat_role: String,
    /// The seat's pubkey, 64 lowercase hex.
    pub seat_pubkey: String,
    /// Path to the seat's Nostr key file, when this host can name one.
    ///
    /// `None` — and a blank string, which is the same thing said badly — means
    /// the seat's key exists only in the keyring and in the seat process's own
    /// `$NOSTR_PRIVATE_KEY`. See `seatKeyfilePath` in
    /// `desktop/src/features/coding-sessions/lib/codingSessionWorktreeSource.ts`
    /// for why that is the honest answer on this machine.
    pub keyfile_path: Option<String>,
    /// The signing program, normally `git-sign-nostr`.
    pub signer_program: String,
    /// The hire event this seat answers, when there is one.
    pub assignment_id: Option<String>,
    /// The coding session this seat belongs to.
    pub session_ref: Option<String>,
    /// The session genesis.
    pub genesis_ref: Option<String>,
    /// The channel a wip checkpoint would be published to.
    pub channel_id: Option<String>,
    /// The branch the worktree was cut on.
    pub branch: Option<String>,
    /// The project or session this seat was hired into, for the author name
    /// a person reads in `git log`. Absent is fine — the name falls back to
    /// `<role> seat` — but a person's name is never accepted here.
    #[serde(default)]
    pub project: Option<String>,
}

/// What the install actually did — named, so a caller never has to assume.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodingSessionSeatHooksInstalled {
    /// The ref this seat's commits will land on.
    pub wip_ref: String,
    /// The directory the hook files were written into.
    pub hooks_dir: String,
    /// The hook file names written, in order.
    pub hooks_written: Vec<String>,
    /// `local` when the worktree owns its `--local` config, `worktree` when the
    /// lines went to a per-worktree `config.worktree`.
    pub config_scope: String,
    /// How git will find these hooks: `default` (they are already where git
    /// looks), `worktree` (`core.hooksPath` was pointed at them for this
    /// worktree alone), or `external` — another dispatcher owns
    /// `core.hooksPath`, so the files were written but that dispatcher decides
    /// what runs. `external` is disclosed rather than worked around: silently
    /// replacing somebody else's hook dispatch is how a repo loses its DCO
    /// trailer and its pre-push gates.
    pub dispatch: String,
    /// False when a second install found everything already in place.
    pub changed: bool,
    /// `keyfile` when the signing config was written, `unsigned` when it was
    /// left out because the caller could name no key file.
    ///
    /// `unsigned` is disclosed rather than papered over. The alternative —
    /// writing `commit.gpgsign = true` alongside a `nostr.keyfile` pointing at
    /// a path that does not exist — fails *every* commit the seat makes the
    /// moment `git-sign-nostr` is off its `PATH`, which is strictly worse than
    /// a seat whose shared commits are unsigned.
    pub signing: String,
    /// The `user.name` this worktree now commits under, e.g.
    /// `builder · kettle-control`.
    ///
    /// Reported rather than assumed: a seat that has to decide what to author
    /// as is a seat that stops and asks, which cost one control run 49 minutes
    /// of finished work (ledger 236(a), 239).
    pub commit_identity_name: String,
    /// The `user.email` this worktree now commits under, always
    /// `<pubkey8>@beekeeper.local` — the seat's own key, never a person's.
    pub commit_identity_email: String,
}

/// Prefix of every planned config line that arms *sharing* rather than signing.
///
/// The unsigned install keeps these and drops everything else. An allow-list
/// rather than a deny-list on purpose: a signing line added to the planner
/// later and missed here would turn `commit.gpgsign` on with no reachable key
/// and fail every commit the seat makes, whereas an unrecognised line merely
/// goes unwritten. The test
/// `the_only_lines_an_unsigned_seat_loses_are_the_signing_ones` pins which
/// lines that actually is, so nothing changes silently.
const SHARING_CONFIG_PREFIX: &str = "buzz.";

/// The lines an unsigned seat loses, as the planner emits them today.
///
/// Not used to filter — `SHARING_CONFIG_PREFIX` does that — but pinned by a
/// test, so a new line in the planner is a decision somebody makes rather than
/// a behaviour that changes on its own.
#[cfg(test)]
const SIGNING_CONFIG_KEYS: [&str; 5] = [
    "gpg.format",
    "gpg.x509.program",
    "commit.gpgsign",
    "user.signingkey",
    "nostr.keyfile",
];

/// Run git in `worktree` with a hermetic environment.
///
/// Global and system config are switched off, so nothing this command does can
/// reach `~/.gitconfig`; the inherited `GIT_DIR` family is cleared so a caller's
/// environment cannot redirect the write to another repository.
fn git(worktree: &Path, args: &[&str]) -> Result<String, String> {
    let mut command = Command::new("git");
    command.args(args);
    command.current_dir(worktree);
    command.env("GIT_TERMINAL_PROMPT", "0");
    command.env("GIT_CONFIG_NOSYSTEM", "1");
    // Git for Windows maps `/dev/null` to `NUL`, so this disables the global
    // file on every platform git supports.
    command.env("GIT_CONFIG_GLOBAL", "/dev/null");
    // The shared list (`project_git_exec::GIT_REPO_SELECTION_VARS`), not a
    // second, independently-drifting copy: this used to clear five of the
    // seven and missed `GIT_COMMON_DIR`, which alone — with no `GIT_DIR` set
    // at all — is enough for `git config --local` to resolve against another
    // repository's config file instead of this worktree's.
    for key in GIT_REPO_SELECTION_VARS {
        command.env_remove(key);
    }
    crate::util::configure_no_window(&mut command);
    let output = command
        .output()
        .map_err(|error| format!("failed to run git: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            format!("git {} failed", args.join(" "))
        } else {
            stderr
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// A `git config --get`-style read that treats "not set" as `None` rather than
/// an error, since git exits 1 for both.
/// `scope` of `None` reads whatever git would read, across every file it uses.
fn git_config_get(worktree: &Path, scope: Option<&str>, key: &str) -> Option<String> {
    let mut args = vec!["config"];
    if let Some(scope) = scope {
        args.push(scope);
    }
    args.extend_from_slice(&["--get", key]);
    git(worktree, &args).ok().filter(|value| !value.is_empty())
}

/// Which config file this worktree's own lines belong in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConfigScope {
    /// The worktree is the main one: `--local` is its own file.
    Local,
    /// A linked worktree: `--worktree` is the only file it does not share.
    Worktree,
}

impl ConfigScope {
    /// The git flag that selects this scope.
    fn flag(self) -> &'static str {
        match self {
            ConfigScope::Local => "--local",
            ConfigScope::Worktree => "--worktree",
        }
    }

    /// The word this scope is reported as.
    fn name(self) -> &'static str {
        match self {
            ConfigScope::Local => "local",
            ConfigScope::Worktree => "worktree",
        }
    }
}

/// Pick the scope, enabling `extensions.worktreeConfig` when a linked worktree
/// needs a config of its own.
///
/// Returns an error rather than enabling the extension when the shared config
/// sets `core.bare = true` or `core.worktree`: both keys become per-worktree
/// once the extension is on, and silently changing what they mean for the
/// person's own checkout is not this command's to do.
fn resolve_config_scope(
    worktree: &Path,
    git_dir: &Path,
    common_dir: &Path,
) -> Result<(ConfigScope, bool), String> {
    if git_dir == common_dir {
        return Ok((ConfigScope::Local, false));
    }
    if git_config_get(worktree, Some("--local"), "extensions.worktreeConfig").as_deref()
        == Some("true")
    {
        return Ok((ConfigScope::Worktree, false));
    }
    if git_config_get(worktree, Some("--local"), "core.bare").as_deref() == Some("true") {
        return Err(
            "this repository sets core.bare in its shared config; enabling per-worktree config \
             would change what that means for every worktree"
                .to_string(),
        );
    }
    if git_config_get(worktree, Some("--local"), "core.worktree").is_some() {
        return Err(
            "this repository sets core.worktree in its shared config; enabling per-worktree \
             config would change what that means for every worktree"
                .to_string(),
        );
    }
    git(
        worktree,
        &["config", "--local", "extensions.worktreeConfig", "true"],
    )?;
    Ok((ConfigScope::Worktree, true))
}

/// Set the git identity a seat's worktree commits under, at its own scope.
///
/// # Why the host does this at all
///
/// A linked worktree inherits no identity: `git config user.email` in a tree
/// the host just cut is empty at local, worktree *and* (because every git
/// process here runs with `GIT_CONFIG_GLOBAL=/dev/null`) global scope. A seat
/// that finds it empty has to decide what to author as, and in the control run
/// of 2026-09-22 a builder seat did the only safe thing its staged text
/// allowed — it stopped and asked a founder — parking finished code for 49
/// minutes over a fact the host already knew (ledger 236(a), 239).
///
/// So the identity is *derived and written*, never asked for: `user.name` is
/// the seat's role and project, `user.email` is the seat's own key at
/// `@beekeeper.local`. It goes to the same scope the sharing lines do — the
/// worktree's own file — so configuring one seat never re-authors the person's
/// own checkout.
///
/// Idempotent, and it never writes the operator's identity: the values come
/// from [`beekeeper_core_pkg::seat_commit_identity`], which can only ever produce
/// an address derived from the key it was handed.
///
/// # Errors
///
/// Returns the refusal when `worktree` is not a git worktree root, when the
/// scope cannot be resolved, or when the seat's pubkey is not 64 lowercase hex.
///
/// On success answers the identity and the name of the scope it was written
/// to (`local` or `worktree`), so a caller can record both.
pub(crate) fn ensure_seat_commit_identity(
    worktree: &Path,
    seat_pubkey: &str,
    seat_role: &str,
    project: Option<&str>,
) -> Result<(SeatCommitIdentity, &'static str), String> {
    let dirs = resolve_worktree_dirs(worktree)?;
    let (scope, _) = resolve_config_scope(worktree, &dirs.git_dir, &dirs.common_dir)?;
    let (identity, _) =
        write_seat_commit_identity(worktree, scope, seat_pubkey, seat_role, project)?;
    Ok((identity, scope.name()))
}

/// The identity write itself, given a scope the caller has already resolved.
///
/// Answers whether anything changed, so an installer that reports `changed`
/// tells the truth about this write too.
fn write_seat_commit_identity(
    worktree: &Path,
    scope: ConfigScope,
    seat_pubkey: &str,
    seat_role: &str,
    project: Option<&str>,
) -> Result<(SeatCommitIdentity, bool), String> {
    let identity = seat_commit_identity(seat_pubkey, seat_role, project)?;
    let mut changed = false;
    for (key, value) in [
        ("user.name", identity.name.as_str()),
        ("user.email", identity.email.as_str()),
    ] {
        if git_config_get(worktree, Some(scope.flag()), key).as_deref() == Some(value) {
            continue;
        }
        git(worktree, &["config", scope.flag(), key, value])?;
        changed = true;
    }
    Ok((identity, changed))
}

/// The two directories the config scope decision needs.
struct WorktreeDirs {
    /// This worktree's own git directory.
    git_dir: PathBuf,
    /// The repository's shared git directory — equal to `git_dir` in the main
    /// worktree, different in a linked one.
    common_dir: PathBuf,
}

/// Resolve a worktree's own and shared git directories, refusing a path that
/// is inside a repository but is not that worktree's root.
///
/// The root check is not ceremony: `rev-parse` walks *up* until it finds a
/// repository, so a subdirectory answers happily, and configuring the
/// enclosing checkout when the caller named a subdirectory is exactly the
/// accident this refuses.
fn resolve_worktree_dirs(worktree: &Path) -> Result<WorktreeDirs, String> {
    if !worktree.is_dir() {
        return Err(format!("{} is not a directory", worktree.display()));
    }
    let git_dir = PathBuf::from(
        git(worktree, &["rev-parse", "--absolute-git-dir"])
            .map_err(|error| format!("{} is not a git worktree: {error}", worktree.display()))?,
    );
    let toplevel = PathBuf::from(git(worktree, &["rev-parse", "--show-toplevel"])?);
    if toplevel.canonicalize().ok() != worktree.canonicalize().ok() {
        return Err(format!(
            "{} is inside the git worktree at {} but is not its root",
            worktree.display(),
            toplevel.display()
        ));
    }
    let common_dir = PathBuf::from(git(
        worktree,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?);
    Ok(WorktreeDirs {
        git_dir,
        common_dir,
    })
}

/// Write `contents` to `path` at mode 0755, reporting whether anything changed.
fn write_hook(path: &Path, contents: &str) -> Result<bool, String> {
    let existing = std::fs::read_to_string(path).ok();
    let already_executable = is_executable(path);
    if existing.as_deref() == Some(contents) && already_executable {
        return Ok(false);
    }
    std::fs::write(path, contents).map_err(|error| format!("write {}: {error}", path.display()))?;
    set_executable(path)?;
    Ok(true)
}

#[cfg(unix)]
/// Whether the file already carries an owner-execute bit.
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|meta| meta.permissions().mode() & 0o100 != 0)
}

#[cfg(not(unix))]
/// Windows has no execute bit; existence is all git needs.
fn is_executable(path: &Path) -> bool {
    path.exists()
}

#[cfg(unix)]
/// Give the hook mode 0755 so git will run it.
fn set_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .map_err(|error| format!("chmod {}: {error}", path.display()))
}

#[cfg(not(unix))]
/// No-op: Windows git runs hooks without an execute bit.
fn set_executable(_path: &Path) -> Result<(), String> {
    Ok(())
}

/// Link every hook git would otherwise have run into `hooks_dir`.
///
/// `core.hooksPath` replaces the directory outright rather than adding to it,
/// so pointing a seat at its own hooks directory would otherwise drop the
/// repository's `commit-msg` sign-off and its pre-push gates. Anything already
/// in the seat's directory — including the two hooks just written — is left
/// alone.
fn mirror_existing_hooks(from: &Path, hooks_dir: &Path) -> Result<bool, String> {
    let Ok(entries) = std::fs::read_dir(from) else {
        return Ok(false);
    };
    let mut changed = false;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let target = hooks_dir.join(&name);
        if target.exists() || target.symlink_metadata().is_ok() {
            continue;
        }
        if entry.path().is_dir() {
            continue;
        }
        link_or_copy(&entry.path(), &target)?;
        changed = true;
    }
    Ok(changed)
}

#[cfg(unix)]
/// Symlink, so the mirrored hook stays whatever the repository updates it to.
fn link_or_copy(source: &Path, target: &Path) -> Result<(), String> {
    std::os::unix::fs::symlink(source, target)
        .map_err(|error| format!("link {}: {error}", target.display()))
}

#[cfg(not(unix))]
/// Copy, since a symlink needs a privilege Windows does not grant by default.
fn link_or_copy(source: &Path, target: &Path) -> Result<(), String> {
    std::fs::copy(source, target)
        .map(|_| ())
        .map_err(|error| format!("copy {}: {error}", target.display()))
}

/// Install the hooks and the config, and say what happened.
///
/// Refuses a path that is not a git worktree without writing anything, so a
/// mistyped working directory cannot leave hook files loose on disk.
fn install(
    request: &InstallCodingSessionSeatHooksRequest,
) -> Result<CodingSessionSeatHooksInstalled, String> {
    let worktree = PathBuf::from(&request.worktree_path);
    // Shared with `ensure_seat_commit_identity`, which resolves the same two
    // directories when it is called on its own at the moment of the cut: one
    // definition of "is this a worktree root", so the two cannot disagree.
    let WorktreeDirs {
        git_dir,
        common_dir,
    } = resolve_worktree_dirs(&worktree)
        .map_err(|error| format!("{error}; no hooks were written"))?;

    let keyfile_path = request
        .keyfile_path
        .as_deref()
        .map(str::trim)
        .filter(|path| !path.is_empty());
    let mut plan = plan_seat_git_hooks(&SeatGitHookRequest {
        seat_role: request.seat_role.clone(),
        seat_pubkey: request.seat_pubkey.clone(),
        keyfile_path: keyfile_path.map(str::to_string),
        signer_program: request.signer_program.clone(),
        assignment_id: request.assignment_id.clone(),
        session_ref: request.session_ref.clone(),
        genesis_ref: request.genesis_ref.clone(),
        channel_id: request.channel_id.clone(),
        branch: request.branch.clone(),
    })?;
    // No key file, no signing identity. `plan_seat_git_hooks` already omits the
    // five signing lines when it is handed no path, so this filter is
    // belt-and-braces — an allow-list, so a signing line added to the planner
    // later cannot slip through here and break every commit the seat makes. The
    // test at `coding_session_seat_hooks_tests.rs` pins today's dropped set.
    let signing = if keyfile_path.is_some() {
        "keyfile"
    } else {
        plan.config
            .retain(|line| line.key.starts_with(SHARING_CONFIG_PREFIX));
        "unsigned"
    };

    let hooks_dir = git_dir.join("hooks");
    std::fs::create_dir_all(&hooks_dir)
        .map_err(|error| format!("create {}: {error}", hooks_dir.display()))?;

    let mut changed = false;
    let mut hooks_written = Vec::with_capacity(plan.hooks.len());
    for hook in &plan.hooks {
        changed |= write_hook(&hooks_dir.join(hook.name), hook.contents)?;
        hooks_written.push(hook.name.to_string());
    }

    let (scope, scope_changed) = resolve_config_scope(&worktree, &git_dir, &common_dir)?;
    changed |= scope_changed;
    // The commit identity is written *unconditionally*, and deliberately not
    // through the plan: it is not a sharing line and not a signing line, so
    // neither the `buzz.` allow-list above nor the presence of a key file may
    // decide whether this seat can author a commit at all (ledger 239).
    let (identity, identity_changed) = write_seat_commit_identity(
        &worktree,
        scope,
        &request.seat_pubkey,
        &request.seat_role,
        request.project.as_deref(),
    )?;
    changed |= identity_changed;
    for line in &plan.config {
        if git_config_get(&worktree, Some(scope.flag()), &line.key).as_deref()
            == Some(line.value.as_str())
        {
            continue;
        }
        git(&worktree, &["config", scope.flag(), &line.key, &line.value])?;
        changed = true;
    }

    // Where git will look. `core.hooksPath` set by anything else is left
    // alone and disclosed.
    let configured_hooks_path = git_config_get(&worktree, None, "core.hooksPath");
    let default_hooks_dir = common_dir.join("hooks");
    let dispatch = match configured_hooks_path {
        Some(path) if Path::new(&path) == hooks_dir => "worktree",
        Some(_) => "external",
        None if default_hooks_dir == hooks_dir => "default",
        None => {
            mirror_existing_hooks(&default_hooks_dir, &hooks_dir)?;
            let hooks_dir_arg = hooks_dir.to_string_lossy().to_string();
            git(
                &worktree,
                &["config", scope.flag(), "core.hooksPath", &hooks_dir_arg],
            )?;
            changed = true;
            "worktree"
        }
    };

    let wip_ref = plan
        .config
        .iter()
        .find(|line| line.key == "buzz.wipRef")
        .map(|line| line.value.clone())
        .unwrap_or_default();

    Ok(CodingSessionSeatHooksInstalled {
        wip_ref,
        hooks_dir: hooks_dir.to_string_lossy().to_string(),
        hooks_written,
        config_scope: scope.name().to_string(),
        dispatch: dispatch.to_string(),
        changed,
        signing: signing.to_string(),
        commit_identity_name: identity.name,
        commit_identity_email: identity.email,
    })
}

/// Install the wip-share hooks into a seat's worktree.
///
/// Idempotent: a second call with the same request writes nothing and answers
/// `changed: false`.
#[tauri::command]
pub async fn install_coding_session_seat_hooks(
    request: InstallCodingSessionSeatHooksRequest,
) -> Result<CodingSessionSeatHooksInstalled, String> {
    tauri::async_runtime::spawn_blocking(move || install(&request))
        .await
        .map_err(|error| format!("seat hook install task failed: {error}"))?
}

#[cfg(test)]
#[path = "coding_session_seat_hooks_tests.rs"]
mod tests;
