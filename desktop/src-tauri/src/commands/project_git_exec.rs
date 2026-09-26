//! Shared git subprocess plumbing for the project commands.
//!
//! Runs the system `git` with an ephemeral, env-only auth configuration:
//! the identity nsec is handed to `git-credential-nostr` via environment
//! variables so nothing key-related ever touches disk or global git config.

use crate::commands::project_git_version::remote_capable_git;
use crate::{app_state::AppState, managed_agents::resolve_command};
use nostr::{Keys, ToBech32};
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use url::Url;

/// Wall-clock cap for a single git invocation. Remote operations talk to
/// relay-supplied clone URLs, so a slow or adversarial remote must not pin
/// `spawn_blocking` threads indefinitely.
const LOCAL_GIT_TIMEOUT: Duration = Duration::from_secs(60);
const REMOTE_GIT_TIMEOUT: Duration = Duration::from_secs(300);

/// The seven variables through which an ambient environment can pick git's
/// repository for it, overriding an explicit path/`-C`/`current_dir`.
///
/// Git exports `GIT_DIR` (and friends) into every hook it runs, so any spawn
/// reachable from inside one — the pre-push gate running this crate's or
/// `buzz-cli`'s tests, most concretely — inherits them and can silently
/// answer for the *hook's* repository instead of the one it was given. The
/// canonical list, shared by every caller in this crate
/// (`team_readiness_git.rs`'s checkout probe, `coding_session_seat_hooks.rs`'s
/// config writer, and `run_git` below) and mirrored by `buzz-cli`'s own
/// `git_command` (`sessions/worktree.rs`) for the same reason.
pub(crate) const GIT_REPO_SELECTION_VARS: [&str; 7] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_COMMON_DIR",
    "GIT_NAMESPACE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
];

fn git_subcommand<'a>(args: &'a [&str]) -> Option<&'a str> {
    let mut index = 0;
    while let Some(argument) = args.get(index).copied() {
        match argument {
            "-c" | "--config" | "-C" | "--git-dir" | "--work-tree" => index += 2,
            "--no-pager" | "--paginate" | "--end-of-options" => index += 1,
            argument
                if argument.starts_with("--config=")
                    || argument.starts_with("--git-dir=")
                    || argument.starts_with("--work-tree=") =>
            {
                index += 1;
            }
            argument if argument.starts_with('-') => index += 1,
            subcommand => return Some(subcommand),
        }
    }
    None
}

fn git_needs_credentials(args: &[&str]) -> bool {
    matches!(
        git_subcommand(args),
        Some("clone" | "fetch" | "push" | "pull" | "ls-remote" | "merge")
    )
}

#[derive(Clone)]
pub(crate) struct GitAuthConfig {
    git_path: std::path::PathBuf,
    /// The git a credentialed invocation runs, or the sentence it is refused
    /// with.
    ///
    /// **Ledger 168.** `git_path` is whatever `resolve_command("git")` found,
    /// which in a Finder-launched bundle is Apple's git 2.39.5 — a git with
    /// no `authtype` credential capability, so `git-credential-nostr` prints
    /// nothing and every fetch, clone and push against the relay dies on
    /// `could not read Username`. A remote operation therefore does not use
    /// `git_path`; it uses the first git ≥ 2.46 this computer has, and when
    /// there is none it fails with
    /// [`crate::commands::project_git_version::remote_git_refusal`] rather
    /// than with git's
    /// username error, which names the wrong problem.
    ///
    /// A config that carries no credential helper — a public GitHub clone,
    /// or a local-only config — keeps `git_path` here, because nothing it
    /// runs needs the helper and an old git serves it perfectly well.
    remote_git: Result<std::path::PathBuf, String>,
    credential_helper: Option<std::path::PathBuf>,
    nsec: String,
    allow_file_transport: bool,
    /// `user.name` / `user.email` for invocations that write a commit.
    ///
    /// Every invocation here runs with `GIT_CONFIG_GLOBAL=/dev/null` and
    /// `GIT_CONFIG_NOSYSTEM=1`, so git has no identity to fall back on except
    /// its hostname auto-detection — which fails outright on a host whose
    /// name has no dot ("unable to auto-detect email address"), and otherwise
    /// authors the commit as `user@hostname`. Callers that commit say who is
    /// committing; `None` leaves git's own behaviour untouched — still every
    /// caller here except the one below. **Finding 64**: the packs-seed
    /// tests set this rather than depending on whatever identity the machine
    /// running them happens to have. **Finding 66**: production's own packs
    /// seed commit (`packs_repo::project_packs_init`) depended on the same
    /// hostname auto-detection the tests were fixed to avoid — set via
    /// [`GitAuthConfig::set_commit_identity`] with the app's own identity,
    /// resolved from the kind:0 the host already has, never from git config.
    commit_identity: Option<(String, String)>,
}

impl GitAuthConfig {
    /// The git to run for this invocation.
    ///
    /// `Err` only for a credentialed invocation on a computer whose git
    /// cannot authenticate at all; the string is the whole explanation.
    fn git_for(&self, needs_credentials: bool) -> Result<&std::path::Path, String> {
        if needs_credentials {
            self.remote_git.as_deref().map_err(std::clone::Clone::clone)
        } else {
            Ok(&self.git_path)
        }
    }

    /// Name the identity a subsequent `commit` in this config is authored as.
    ///
    /// See [`GitAuthConfig::commit_identity`] for why this exists: without
    /// it, a `git commit` run through this config depends on whatever
    /// identity the host machine happens to expose (or does not), which is
    /// never the app's own key. Every caller that writes a commit on the
    /// app's behalf — as opposed to on behalf of a person, which git prompts
    /// for — should call this before committing.
    pub(crate) fn set_commit_identity(
        &mut self,
        name: impl Into<String>,
        email: impl Into<String>,
    ) {
        self.commit_identity = Some((name.into(), email.into()));
    }
}

fn read_pipe_lossy(pipe: Option<impl Read>) -> String {
    let Some(mut pipe) = pipe else {
        return String::new();
    };
    let mut bytes = Vec::new();
    let _ = pipe.read_to_end(&mut bytes);
    String::from_utf8_lossy(&bytes).to_string()
}

/// One finished `git` invocation, whatever it exited with.
///
/// [`run_git`] folds a non-zero exit into an `Err`, which is right for the
/// callers that only want the output. It is wrong for the callers that
/// distinguish *git said no* from *git could not say*: `merge-base
/// --is-ancestor` answers "no" with exit 1 and "I failed" with 128, and a
/// reader that sees only `Err` reports a failure as an answer.
pub(crate) struct GitOutcome {
    /// The exit status, success or not.
    pub status: std::process::ExitStatus,
    /// Everything git wrote to stdout, lossily decoded.
    pub stdout: String,
    /// Everything git wrote to stderr, lossily decoded.
    pub stderr: String,
}

/// Run `git` and return what it did, without judging the exit status.
///
/// `Err` only when there was no invocation to judge: git could not be
/// spawned, it timed out, or waiting on it failed. Every other outcome —
/// including a fatal error from git itself — is `Ok` with the status and both
/// streams, for callers that need to tell git's "no" from git's "I cannot".
pub(crate) fn run_git_status(
    args: &[&str],
    cwd: Option<&std::path::Path>,
    auth: &GitAuthConfig,
) -> Result<GitOutcome, String> {
    let needs_credentials = git_needs_credentials(args);
    let mut command = Command::new(auth.git_for(needs_credentials)?);
    command.args(args);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let timeout = if needs_credentials {
        REMOTE_GIT_TIMEOUT
    } else {
        LOCAL_GIT_TIMEOUT
    };
    configure_git_auth(&mut command, auth, needs_credentials);
    command.stdin(Stdio::null());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    crate::util::configure_no_window(&mut command);

    let mut child = command
        .spawn()
        .map_err(|error| format!("failed to run git: {error}"))?;

    // Drain the pipes on background threads so a chatty git process can't
    // deadlock on a full pipe while we poll for exit below.
    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();
    let stdout_thread = std::thread::spawn(move || read_pipe_lossy(stdout_pipe));
    let stderr_thread = std::thread::spawn(move || read_pipe_lossy(stderr_pipe));

    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if started.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = stdout_thread.join();
                    let _ = stderr_thread.join();
                    return Err(format!("git timed out after {}s", timeout.as_secs()));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("failed to wait for git: {error}"));
            }
        }
    };

    Ok(GitOutcome {
        status,
        stdout: stdout_thread.join().unwrap_or_default(),
        stderr: stderr_thread.join().unwrap_or_default(),
    })
}

/// Run `git` and return its stdout, or its complaint.
///
/// A thin reading of [`run_git_status`]: a non-zero exit becomes an `Err`
/// carrying git's stderr, or `git exited with status <n>` when git said
/// nothing.
pub(crate) fn run_git(
    args: &[&str],
    cwd: Option<&std::path::Path>,
    auth: &GitAuthConfig,
) -> Result<String, String> {
    let outcome = run_git_status(args, cwd, auth)?;
    if !outcome.status.success() {
        let stderr = outcome.stderr.trim().to_string();
        return Err(if stderr.is_empty() {
            format!("git exited with status {}", outcome.status)
        } else {
            stderr
        });
    }
    Ok(outcome.stdout)
}

/// Run one hardened Git command with caller-owned bytes on stdin and return
/// stdout without a lossy UTF-8 conversion.  Snapshot publication uses this
/// for `hash-object` and `cat-file`: Git must receive and return the exact
/// bytes the snapshot verifier accepted, unaffected by attributes or a
/// worktree filter.
pub(crate) fn run_git_bytes(
    args: &[&str],
    cwd: Option<&std::path::Path>,
    auth: &GitAuthConfig,
    input: &[u8],
) -> Result<Vec<u8>, String> {
    let needs_credentials = git_needs_credentials(args);
    let mut command = Command::new(auth.git_for(needs_credentials)?);
    command.args(args);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let timeout = if needs_credentials {
        REMOTE_GIT_TIMEOUT
    } else {
        LOCAL_GIT_TIMEOUT
    };
    configure_git_auth(&mut command, auth, needs_credentials);
    command.stdin(Stdio::piped());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    crate::util::configure_no_window(&mut command);

    let mut child = command
        .spawn()
        .map_err(|error| format!("failed to run git: {error}"))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "git did not expose stdin".to_string())?;
    stdin
        .write_all(input)
        .map_err(|error| format!("write git stdin: {error}"))?;
    drop(stdin);

    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();
    let stdout_thread = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut pipe) = stdout_pipe {
            let _ = pipe.read_to_end(&mut bytes);
        }
        bytes
    });
    let stderr_thread = std::thread::spawn(move || read_pipe_lossy(stderr_pipe));
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_thread.join();
                let _ = stderr_thread.join();
                return Err(format!("git timed out after {}s", timeout.as_secs()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("failed to wait for git: {error}"));
            }
        }
    };
    let stdout = stdout_thread.join().unwrap_or_default();
    let stderr = stderr_thread.join().unwrap_or_default();
    if status.success() {
        Ok(stdout)
    } else if stderr.trim().is_empty() {
        Err(format!("git exited with status {status}"))
    } else {
        Err(stderr.trim().to_string())
    }
}

fn configure_git_auth(command: &mut Command, auth: &GitAuthConfig, needs_credentials: bool) {
    command.env("GIT_TERMINAL_PROMPT", "0");
    command.env("GIT_CONFIG_NOSYSTEM", "1");
    for key in GIT_REPO_SELECTION_VARS {
        command.env_remove(key);
    }
    // More this host clears for its own reasons: an inherited ssh command,
    // external diff or askpass program would run a program we did not choose.
    for key in [
        "GIT_SSH_COMMAND",
        "GIT_EXTERNAL_DIFF",
        "GIT_ASKPASS",
        "SSH_ASKPASS",
    ] {
        command.env_remove(key);
    }
    // Git for Windows maps `/dev/null` to `NUL` internally, so this value
    // disables the global config file on every platform.
    command.env("GIT_CONFIG_GLOBAL", "/dev/null");

    // Base entries: disable any inherited credential helper, and neutralize
    // repo-local hooks — every process git spawns inherits our environment
    // (including NOSTR_PRIVATE_KEY below), and a cloned repository's hooks
    // must never run with the identity key in reach.
    let mut entries: Vec<(&str, String)> = vec![
        ("credential.helper", String::new()),
        ("core.hooksPath", "/dev/null".to_string()),
        ("core.fsmonitor", "false".to_string()),
        // Two programs a repository's own configuration can name for a
        // fetch or ls-remote: the password prompt Git runs when no
        // helper answers a challenge (terminal prompts being off does not
        // stop it), and the command fetch negotiation runs to list an
        // alternate's refs. Empty is Git's own "none".
        ("core.askPass", String::new()),
        ("core.alternateRefsCommand", String::new()),
        // History reads (`log`, `show`) would otherwise verify any signed
        // commit with the repository's own `gpg.program`.
        ("log.showSignature", "false".to_string()),
        ("protocol.allow", "never".to_string()),
        ("protocol.http.allow", "always".to_string()),
        ("protocol.https.allow", "always".to_string()),
        ("protocol.ext.allow", "never".to_string()),
        (
            "protocol.file.allow",
            if auth.allow_file_transport {
                "always"
            } else {
                "never"
            }
            .to_string(),
        ),
    ];
    if let Some((name, email)) = &auth.commit_identity {
        entries.push(("user.name", name.clone()));
        entries.push(("user.email", email.clone()));
    }
    if needs_credentials {
        let Some(cred_helper) = &auth.credential_helper else {
            return apply_git_config(command, &entries);
        };
        command.env("NOSTR_PRIVATE_KEY", &auth.nsec);
        entries.push((
            "credential.helper",
            credential_helper_config_value(cred_helper),
        ));
        entries.push(("credential.useHttpPath", "true".to_string()));
    }
    apply_git_config(command, &entries);
}

/// Format a path for git `credential.helper`.
///
/// Git for Windows invokes helpers via MinGW bash, which treats `\` as
/// escapes. Forward slashes work on every platform git supports. Git evaluates
/// the value as shell code: use an explicit shell helper and single-quote the
/// complete path so spaces and shell metacharacters remain literal. Without
/// `!`, Git would prefix a quoted path with `git credential-`.
fn credential_helper_config_value(path: &std::path::Path) -> String {
    let normalized = path.to_string_lossy().replace('\\', "/");
    format!("!'{}'", normalized.replace('\'', "'\\''"))
}

fn apply_git_config(command: &mut Command, entries: &[(&str, String)]) {
    command.env("GIT_CONFIG_COUNT", entries.len().to_string());
    for (index, (key, value)) in entries.iter().enumerate() {
        command.env(format!("GIT_CONFIG_KEY_{index}"), key);
        command.env(format!("GIT_CONFIG_VALUE_{index}"), value);
    }
}

pub(crate) fn build_git_auth_config(state: &AppState) -> Result<GitAuthConfig, String> {
    let keys = state.signing_keys()?;
    build_git_auth_config_for_keys(&keys)
}

pub(crate) fn build_git_clone_auth_config(
    clone_url: &str,
    state: &AppState,
) -> Result<GitAuthConfig, String> {
    if validate_github_clone_url(clone_url).is_ok() {
        // A public GitHub clone presents no credential, so the helper — and
        // with it the 2.46 requirement — is not in play.
        let git_path =
            resolve_command("git").ok_or_else(|| "git was not found on PATH".to_string())?;
        return Ok(GitAuthConfig {
            remote_git: Ok(git_path.clone()),
            git_path,
            credential_helper: None,
            nsec: String::new(),
            allow_file_transport: false,
            commit_identity: None,
        });
    }
    build_git_auth_config(state)
}

pub(crate) fn build_git_auth_config_for_keys(keys: &Keys) -> Result<GitAuthConfig, String> {
    let git_path = resolve_command("git").ok_or_else(|| "git was not found on PATH".to_string())?;
    let credential_helper = resolve_command("git-credential-nostr");
    let nsec = keys
        .secret_key()
        .to_bech32()
        .map_err(|error| format!("encode identity key: {error}"))?;
    Ok(GitAuthConfig {
        git_path,
        // Probed once per app run and cached; a machine with no capable git
        // carries the refusal here and produces it at the first remote
        // operation, never at a local one.
        remote_git: remote_capable_git(),
        credential_helper,
        nsec,
        allow_file_transport: false,
        commit_identity: None,
    })
}

/// A git configuration for operations that never touch a remote.
///
/// `worktree add`, `rev-parse`, and friends need the same hardening every
/// other invocation gets — no inherited global config, no repo-local hooks —
/// but they must not carry the identity nsec: there is nothing for a
/// credential helper to authenticate, and an env-borne key is worth removing
/// wherever it is not needed.
pub(crate) fn build_local_git_auth_config() -> Result<GitAuthConfig, String> {
    let git_path = resolve_command("git").ok_or_else(|| "git was not found on PATH".to_string())?;
    Ok(GitAuthConfig {
        // No helper, nothing to authenticate: whatever git this computer has
        // is the right one, including Apple's.
        remote_git: Ok(git_path.clone()),
        git_path,
        credential_helper: None,
        nsec: String::new(),
        allow_file_transport: false,
        commit_identity: None,
    })
}

/// A local configuration for a clone whose *remote* is a path on this disk.
///
/// Every other configuration built here sets `protocol.file.allow=never` on
/// purpose: a relay-supplied clone URL must never be able to name a local
/// path. A cache-to-sibling clone is the opposite case — the "remote" is a
/// directory this host wrote itself, there is nothing to authenticate, and
/// the nsec and the credential helper stay out of the environment. Widening
/// [`build_git_auth_config`] instead would hand the file transport to every
/// remote operation, so this is its own constructor (ledger 169: the seat's
/// agents clone, spec § 4.11, ran with the remote config and git refused it
/// with `fatal: transport 'file' not allowed`).
pub(crate) fn build_local_clone_git_auth_config() -> Result<GitAuthConfig, String> {
    let mut auth = build_local_git_auth_config()?;
    auth.allow_file_transport = true;
    Ok(auth)
}

#[cfg(test)]
pub(crate) fn build_test_git_auth_config() -> Result<GitAuthConfig, String> {
    let mut auth = build_git_auth_config_for_keys(&Keys::generate())?;
    auth.allow_file_transport = true;
    // A test clones and fetches over `file://` with no relay and no helper
    // behind it, so the 2.46 requirement — which exists only for the Nostr
    // credential protocol — must not decide whether the suite can run. The
    // machine's own git answers for these, as it did before ledger 168.
    auth.remote_git = Ok(auth.git_path.clone());
    // Finding 64: a test that commits names its own author. Without this the
    // packs-seed tests fail on any machine whose hostname git cannot turn
    // into an email, and pass elsewhere by authoring commits as whoever
    // happens to be logged in — a test whose result depends on the operator.
    auth.commit_identity = Some((
        "Beekeeper Test".to_string(),
        "test@example.invalid".to_string(),
    ));
    Ok(auth)
}

/// Normalizes and validates a relay-supplied branch name. Strips a
/// `refs/heads/` prefix, then rejects anything outside a conservative
/// character allowlist, path traversal (`..`), leading/trailing `/`, and
/// flag-shaped values (leading `-`) so a branch can never reach git as an
/// option instead of a positional argument.
pub(crate) fn clean_branch(value: Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.trim_start_matches("refs/heads/"))
        .filter(|value| {
            !value.is_empty()
                && !value.starts_with('-')
                && !value.contains("..")
                && !value.starts_with('/')
                && !value.ends_with('/')
                && value
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '.' | '-'))
        })
        .map(ToString::to_string)
}

pub(crate) fn clean_target_ref(value: Option<String>) -> Option<String> {
    let value = value?.trim().to_string();
    for prefix in ["refs/tags/", "refs/nostr/"] {
        if let Some(name) = value.strip_prefix(prefix) {
            let clean_name = clean_branch(Some(name.to_string()))?;
            return (clean_name == name).then_some(format!("{prefix}{clean_name}"));
        }
    }
    None
}

pub(crate) fn validate_clone_url(clone_url: &str) -> Result<(), String> {
    let parsed = Url::parse(clone_url).map_err(|error| format!("invalid clone URL: {error}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("clone URL must be http or https".into());
    }
    // Buzz git remotes are served at `…/git/<owner-pubkey>/<repo-id>` — a
    // literal `git` segment followed by the 64-hex owner pubkey and a
    // non-empty repository id (the relay may live under a path prefix).
    let segments = parsed
        .path_segments()
        .map(|segments| segments.filter(|s| !s.is_empty()).collect::<Vec<_>>())
        .unwrap_or_default();
    let is_buzz_repo_path = segments
        .iter()
        .rposition(|segment| *segment == "git")
        .filter(|index| segments.len() == index + 3)
        .map(|index| {
            segments[index + 1].len() == 64
                && segments[index + 1].chars().all(|c| c.is_ascii_hexdigit())
                && !segments[index + 2].is_empty()
        })
        .unwrap_or(false);
    if !is_buzz_repo_path {
        return Err("clone URL must point at a Buzz git repository".into());
    }
    Ok(())
}

fn validate_github_clone_url(clone_url: &str) -> Result<(), String> {
    let parsed = Url::parse(clone_url).map_err(|error| format!("invalid clone URL: {error}"))?;
    if parsed.scheme() != "https"
        || parsed.host_str() != Some("github.com")
        || parsed.port().is_some()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err("GitHub clone URL must use public https://github.com/owner/repository".into());
    }
    let segments = parsed
        .path_segments()
        .map(|segments| {
            segments
                .filter(|segment| !segment.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let valid_segment = |segment: &&str| {
        !segment.starts_with('-')
            && !segment.contains("..")
            && segment.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-')
            })
    };
    if segments.len() != 2 || !segments.iter().all(valid_segment) {
        return Err("GitHub clone URL must name one owner and repository".into());
    }
    Ok(())
}

pub(crate) fn validate_local_clone_url(clone_url: &str) -> Result<(), String> {
    if validate_clone_url(clone_url).is_ok() || validate_github_clone_url(clone_url).is_ok() {
        return Ok(());
    }
    Err("clone URL must point at a Buzz repository or public GitHub repository".into())
}

pub(crate) fn validate_local_clone_url_for_workspace(
    clone_url: &str,
    state: &AppState,
) -> Result<(), String> {
    if validate_github_clone_url(clone_url).is_ok() {
        return Ok(());
    }
    validate_workspace_clone_url(clone_url, state)
}

pub(crate) fn clone_url_owner(clone_url: &str) -> Option<String> {
    let parsed = Url::parse(clone_url).ok()?;
    let segments = parsed
        .path_segments()?
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    let index = segments.iter().rposition(|segment| *segment == "git")?;
    (segments.len() == index + 3).then(|| segments[index + 1].to_ascii_lowercase())
}

pub(crate) fn validate_workspace_clone_url(
    clone_url: &str,
    state: &AppState,
) -> Result<(), String> {
    let relay_base = crate::relay::relay_api_base_url_with_override(state);
    validate_clone_url_against_relay(clone_url, &relay_base)
}

fn validate_clone_url_against_relay(clone_url: &str, relay_base: &str) -> Result<(), String> {
    validate_clone_url(clone_url)?;
    let clone = Url::parse(clone_url).map_err(|error| format!("invalid clone URL: {error}"))?;
    let relay = Url::parse(relay_base)
        .map_err(|error| format!("configured relay URL is invalid: {error}"))?;
    if clone.scheme() != relay.scheme()
        || clone.host_str() != relay.host_str()
        || clone.port_or_known_default() != relay.port_or_known_default()
    {
        return Err("clone URL must use the active workspace relay".into());
    }
    let relay_path = relay.path().trim_end_matches('/');
    if !relay_path.is_empty() && !clone.path().starts_with(&format!("{relay_path}/")) {
        return Err("clone URL must use the active workspace relay path".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        clean_branch, clean_target_ref, credential_helper_config_value, git_needs_credentials,
        git_subcommand, validate_clone_url, validate_clone_url_against_relay,
        validate_local_clone_url,
    };

    #[test]
    fn credential_helper_config_value_uses_forward_slashes() {
        let path =
            std::path::PathBuf::from(r"C:\Users\x\AppData\Local\Buzz\git-credential-nostr.exe");
        assert_eq!(
            credential_helper_config_value(&path),
            "!'C:/Users/x/AppData/Local/Buzz/git-credential-nostr.exe'",
        );
    }

    #[cfg(unix)]
    #[test]
    fn credential_helper_runs_from_bundle_path_with_shell_characters() {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;
        use std::process::{Command, Stdio};

        let root = tempfile::tempdir().unwrap();
        let bundle = root.path().join("Beekeeper Dev's $HOME `literal`.app");
        std::fs::create_dir(&bundle).unwrap();
        let helper = bundle.join("git-credential-nostr");
        std::fs::write(
            &helper,
            "#!/bin/sh\n[ \"$1\" = get ] || exit 1\ncat >/dev/null\nprintf 'username=test-user\\npassword=test-token\\n'\n",
        )
        .unwrap();
        std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut child = Command::new("git")
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_TERMINAL_PROMPT", "0")
            .current_dir(root.path())
            .args(["-c", "credential.helper=", "-c"])
            .arg(format!(
                "credential.helper={}",
                credential_helper_config_value(&helper)
            ))
            .args(["credential", "fill"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(b"protocol=https\nhost=example.invalid\n\n")
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("password=test-token"));
    }

    #[test]
    fn git_subcommand_skips_global_config_options() {
        assert_eq!(
            git_subcommand(&[
                "-c",
                "user.name=Buzz User",
                "-c",
                "user.email=user@example.com",
                "merge",
                "HEAD",
            ]),
            Some("merge")
        );
        assert_eq!(
            git_subcommand(&["--config=credential.useHttpPath=true", "fetch", "origin"]),
            Some("fetch")
        );
    }

    #[test]
    fn remote_and_promisor_operations_receive_credentials() {
        assert!(git_needs_credentials(&["fetch", "origin"]));
        assert!(git_needs_credentials(&[
            "-c",
            "user.name=Buzz User",
            "merge",
            "HEAD"
        ]));
        assert!(!git_needs_credentials(&["rev-parse", "HEAD"]));
    }

    #[test]
    fn clean_branch_accepts_plain_and_prefixed_names() {
        assert_eq!(
            clean_branch(Some("refs/heads/feature/x-1".into())),
            Some("feature/x-1".to_string())
        );
        assert_eq!(
            clean_branch(Some(" main ".into())),
            Some("main".to_string())
        );
    }

    #[test]
    fn clean_branch_rejects_flag_shaped_and_traversal_values() {
        assert_eq!(clean_branch(Some("--upload-pack=/tmp/evil".into())), None);
        assert_eq!(clean_branch(Some("-x".into())), None);
        assert_eq!(clean_branch(Some("a/../b".into())), None);
        assert_eq!(clean_branch(Some("/leading".into())), None);
        assert_eq!(clean_branch(Some("trailing/".into())), None);
        assert_eq!(clean_branch(Some("bad name".into())), None);
        assert_eq!(clean_branch(None), None);
    }

    #[test]
    fn clean_target_ref_accepts_only_tags_and_pull_request_refs() {
        assert_eq!(
            clean_target_ref(Some("refs/tags/v1.0.0".into())),
            Some("refs/tags/v1.0.0".to_string())
        );
        assert_eq!(
            clean_target_ref(Some("refs/nostr/abc123".into())),
            Some("refs/nostr/abc123".to_string())
        );
        assert_eq!(clean_target_ref(Some("refs/heads/main".into())), None);
        assert_eq!(clean_target_ref(Some("refs/tags/../main".into())), None);
    }

    #[test]
    fn validate_clone_url_requires_buzz_repo_shape() {
        let owner = "a".repeat(64);
        assert!(validate_clone_url(&format!("https://relay.example/git/{owner}/repo")).is_ok());
        assert!(
            validate_clone_url(&format!("https://relay.example/prefix/git/{owner}/repo")).is_ok()
        );
        assert!(validate_clone_url("https://relay.example/git/short/repo").is_err());
        assert!(validate_clone_url("https://evil.example/has/git/inpath").is_err());
        assert!(validate_clone_url(&format!("ssh://relay.example/git/{owner}/repo")).is_err());
        assert!(validate_clone_url(&format!(
            "https://relay.example/git/{owner}/repo/unexpected"
        ))
        .is_err());
    }

    #[test]
    fn workspace_clone_url_requires_exact_relay_origin_and_prefix() {
        let owner = "a".repeat(64);
        let valid = format!("https://relay.example/prefix/git/{owner}/repo");
        assert!(validate_clone_url_against_relay(&valid, "https://relay.example/prefix").is_ok());
        assert!(validate_clone_url_against_relay(&valid, "http://relay.example/prefix").is_err());
        assert!(
            validate_clone_url_against_relay(&valid, "https://relay.example:8443/prefix").is_err()
        );
        assert!(validate_clone_url_against_relay(&valid, "https://relay.example/other").is_err());
        assert!(validate_clone_url_against_relay(
            &format!("https://evil.example/prefix/git/{owner}/repo"),
            "https://relay.example/prefix",
        )
        .is_err());
    }

    #[test]
    fn local_clone_url_allows_only_public_github_https_urls() {
        assert!(validate_local_clone_url("https://github.com/block/buzz").is_ok());
        assert!(validate_local_clone_url("https://github.com/block/buzz.git").is_ok());
        assert!(validate_local_clone_url("http://github.com/block/buzz").is_err());
        assert!(validate_local_clone_url("https://github.com/block/buzz/issues").is_err());
        assert!(validate_local_clone_url("https://user@github.com/block/buzz").is_err());
        assert!(validate_local_clone_url("https://github.com.evil.test/block/buzz").is_err());
        assert!(validate_local_clone_url("https://gitlab.com/block/buzz").is_err());
    }
}

#[cfg(all(test, unix))]
#[path = "project_git_exec_hardening_tests.rs"]
mod hardening_tests;
