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
//! [`buzz_core_pkg::seat_git_hooks`], so this command and `lefthook.yml`'s
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

use buzz_core_pkg::seat_git_hooks::{plan_seat_git_hooks, SeatGitHookRequest};
use serde::{Deserialize, Serialize};

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
    for key in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    ] {
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
    if !worktree.is_dir() {
        return Err(format!(
            "{} is not a directory; no hooks were written",
            worktree.display()
        ));
    }
    let git_dir = PathBuf::from(
        git(&worktree, &["rev-parse", "--absolute-git-dir"])
            .map_err(|error| format!("{} is not a git worktree: {error}", worktree.display()))?,
    );
    // `rev-parse` walks *up* until it finds a repository, so a path that is
    // merely inside one answers happily — and arming the enclosing checkout
    // when the caller named a subdirectory is exactly the accident this
    // refuses. The path handed in must be the worktree's own root.
    let toplevel = PathBuf::from(git(&worktree, &["rev-parse", "--show-toplevel"])?);
    if toplevel.canonicalize().ok() != worktree.canonicalize().ok() {
        return Err(format!(
            "{} is inside the git worktree at {} but is not its root; no hooks were written",
            worktree.display(),
            toplevel.display()
        ));
    }
    let common_dir = PathBuf::from(git(
        &worktree,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?);

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
