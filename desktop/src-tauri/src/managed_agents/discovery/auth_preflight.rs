//! Live credential pre-flight for the Claude runtime (finding 71).
//!
//! `claude auth status` reports `loggedIn: true` from the credential file
//! alone; it never exercises the OAuth refresh path. On 2026-09-04 every turn
//! of a claude seat failed with "Failed to authenticate: OAuth session expired
//! and could not be refreshed" while that status read stayed green. The only
//! check that tells the truth is a real call, so this module makes one:
//!
//! ```text
//! claude -p "reply with the single word ok" --max-turns 1 --output-format text
//! ```
//!
//! bounded to [`PREFLIGHT_TIMEOUT`], cached for [`PREFLIGHT_CACHE_TTL`], and
//! classified into exactly three states. The token itself is never read,
//! stored, or logged: the only bytes kept are a redacted, bounded excerpt of
//! the CLI's own error line.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// The one runtime whose readiness is decided by a live call today.
pub(crate) const CLAUDE_RUNTIME_ID: &str = "claude";
/// The CLI binary the pre-flight resolves and runs.
pub(crate) const CLAUDE_PREFLIGHT_BINARY: &str = "claude";
/// The exact arguments of the reproducing call from live run 6.
pub(crate) const CLAUDE_PREFLIGHT_ARGS: &[&str] = &[
    "-p",
    "reply with the single word ok",
    "--max-turns",
    "1",
    "--output-format",
    "text",
];
/// Hard bound on one pre-flight call; the child is killed past it.
pub(crate) const PREFLIGHT_TIMEOUT: Duration = Duration::from_secs(20);
/// How long a verdict is trusted before the call is made again.
pub(crate) const PREFLIGHT_CACHE_TTL: Duration = Duration::from_secs(10 * 60);
/// The remedy shown next to a dead credential, everywhere the runtime's
/// health is shown.
pub const CLAUDE_LOGIN_REMEDY: &str = "Run `claude auth login` in a terminal, then retry.";

/// What one live call proved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum AuthPreflightState {
    /// The call printed `ok`: the token was exercised and worked.
    VerifiedLive,
    /// The call failed with an authentication error; `sentence` is the CLI's
    /// own (redacted, bounded) error line.
    CredentialDead {
        /// The CLI's error line, redacted and bounded.
        sentence: String,
    },
    /// Timeout, spawn failure, or an answer that proves nothing either way.
    Unknown {
        /// Why no verdict could be reached.
        reason: String,
    },
}

/// A pre-flight verdict, with the timestamp it was reached at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthPreflightVerdict {
    /// The runtime the verdict is about.
    pub runtime_id: String,
    /// The three-way state.
    pub state: AuthPreflightState,
    /// When the call finished, unix milliseconds.
    pub checked_at_ms: u64,
    /// The command line that was run, for disclosure in the UI.
    pub command: String,
    /// What the operator should do; present only for a dead credential.
    pub remedy: Option<String>,
    /// True when the verdict was served from the cache rather than a fresh call.
    pub cached: bool,
}

/// What a finished pre-flight process left behind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PreflightOutput {
    /// Whether the process exited zero.
    pub exit_success: bool,
    /// Captured stdout.
    pub stdout: String,
    /// Captured stderr.
    pub stderr: String,
}

/// Why a pre-flight process produced no output to classify.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PreflightRunError {
    /// The process outlived the bound and was killed.
    TimedOut(Duration),
    /// The process could not be started.
    Spawn(String),
}

/// The seam the tests fake: something that runs a command under a bound.
pub(crate) trait PreflightRunner {
    /// Run `binary args…` and return what it printed, or why it could not.
    fn run(
        &self,
        binary: &Path,
        args: &[&str],
        timeout: Duration,
    ) -> Result<PreflightOutput, PreflightRunError>;
}

/// The real runner: a child process with piped stdio, drained on threads,
/// polled against the deadline and killed past it.
pub(crate) struct ProcessRunner {
    /// `PATH` for the child, so `claude` can find `node` the way the login
    /// probe already does.
    pub path_env: Option<String>,
    /// Working directory for the child; the home directory, so no project's
    /// `.claude/` settings colour the check.
    pub cwd: Option<PathBuf>,
}

impl PreflightRunner for ProcessRunner {
    fn run(
        &self,
        binary: &Path,
        args: &[&str],
        timeout: Duration,
    ) -> Result<PreflightOutput, PreflightRunError> {
        let mut command = Command::new(binary);
        command.args(args);
        if let Some(path) = &self.path_env {
            command.env("PATH", path);
        }
        if let Some(cwd) = &self.cwd {
            command.current_dir(cwd);
        }
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        crate::util::configure_no_window(&mut command);

        let mut child = command
            .spawn()
            .map_err(|error| PreflightRunError::Spawn(error.to_string()))?;

        let stdout_pipe = child.stdout.take();
        let stderr_pipe = child.stderr.take();
        let stdout_thread = std::thread::spawn(move || drain(stdout_pipe));
        let stderr_thread = std::thread::spawn(move || drain(stderr_pipe));

        let deadline = Instant::now() + timeout;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) => {
                    if Instant::now() >= deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        break None;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(_) => break None,
            }
        };
        let stdout = stdout_thread.join().unwrap_or_default();
        let stderr = stderr_thread.join().unwrap_or_default();
        match status {
            Some(status) => Ok(PreflightOutput {
                exit_success: status.success(),
                stdout,
                stderr,
            }),
            None => Err(PreflightRunError::TimedOut(timeout)),
        }
    }
}

fn drain<R: Read>(pipe: Option<R>) -> String {
    let mut buf = Vec::new();
    if let Some(mut pipe) = pipe {
        let _ = pipe.read_to_end(&mut buf);
    }
    String::from_utf8_lossy(&buf).into_owned()
}

/// The pre-flight command for a runtime, when it has one.
pub(crate) fn preflight_command_for(
    runtime_id: &str,
) -> Option<(&'static str, &'static [&'static str])> {
    (runtime_id == CLAUDE_RUNTIME_ID).then_some((CLAUDE_PREFLIGHT_BINARY, CLAUDE_PREFLIGHT_ARGS))
}

/// The disclosed command line, quoted the way a shell would want it.
pub(crate) fn preflight_command_line(binary: &str, args: &[&str]) -> String {
    let mut parts = vec![binary.to_string()];
    parts.extend(args.iter().map(|arg| {
        if arg.contains(' ') {
            format!("\"{arg}\"")
        } else {
            arg.to_string()
        }
    }));
    parts.join(" ")
}

/// Longest excerpt of a CLI error line the verdict keeps.
const SENTENCE_MAX_CHARS: usize = 240;

/// Words that mark a failure as the credential's, not the network's or the
/// CLI's. Matched case-insensitively against stderr and stdout together.
const AUTH_FAILURE_MARKERS: &[&str] = &[
    "failed to authenticate",
    "oauth",
    "not logged in",
    "not authenticated",
    "authentication_error",
    "authentication error",
    "please run /login",
    "please log in",
    "invalid api key",
    "unauthorized",
];

/// Decide the three-way state from what the call did.
pub(crate) fn classify_preflight(
    result: Result<PreflightOutput, PreflightRunError>,
) -> AuthPreflightState {
    let output = match result {
        Ok(output) => output,
        Err(PreflightRunError::TimedOut(bound)) => {
            return AuthPreflightState::Unknown {
                reason: format!("the check did not finish within {}s", bound.as_secs()),
            }
        }
        Err(PreflightRunError::Spawn(error)) => {
            return AuthPreflightState::Unknown {
                reason: format!("the check could not start: {}", redact(&error)),
            }
        }
    };
    if output.exit_success {
        let printed_ok = output
            .stdout
            .split(|c: char| !c.is_ascii_alphanumeric())
            .any(|word| word.eq_ignore_ascii_case("ok"));
        return if printed_ok {
            AuthPreflightState::VerifiedLive
        } else {
            AuthPreflightState::Unknown {
                reason: "the check succeeded but did not print ok".to_string(),
            }
        };
    }
    let combined = format!("{}\n{}", output.stderr, output.stdout);
    let lower = combined.to_ascii_lowercase();
    let sentence = first_meaningful_line(&combined);
    if AUTH_FAILURE_MARKERS
        .iter()
        .any(|marker| lower.contains(marker))
    {
        AuthPreflightState::CredentialDead { sentence }
    } else {
        AuthPreflightState::Unknown {
            reason: if sentence.is_empty() {
                "the check failed without a message".to_string()
            } else {
                sentence
            },
        }
    }
}

fn first_meaningful_line(text: &str) -> String {
    let line = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("");
    let redacted = redact(line);
    if redacted.chars().count() > SENTENCE_MAX_CHARS {
        let cut: String = redacted.chars().take(SENTENCE_MAX_CHARS).collect();
        format!("{cut}…")
    } else {
        redacted
    }
}

/// Blank out anything that could be a credential: `sk-…` keys, bearer
/// values, and any long unbroken token-shaped run. The pre-flight never reads
/// the credential file, so this is belt-and-braces over the CLI's own words.
fn redact(text: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut after_bearer = false;
    for word in text.split_whitespace() {
        let looks_like_secret = after_bearer
            || word.starts_with("sk-")
            || (word.len() >= 40
                && word
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')));
        after_bearer = word.eq_ignore_ascii_case("bearer");
        out.push(if looks_like_secret {
            "[redacted]".to_string()
        } else {
            word.to_string()
        });
    }
    out.join(" ")
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Run one pre-flight through `runner` and wrap its state as a verdict.
pub(crate) fn preflight_verdict(
    runner: &dyn PreflightRunner,
    runtime_id: &str,
    binary: &Path,
    binary_label: &str,
    args: &[&str],
) -> AuthPreflightVerdict {
    let state = classify_preflight(runner.run(binary, args, PREFLIGHT_TIMEOUT));
    verdict_from_state(
        runtime_id,
        state,
        preflight_command_line(binary_label, args),
    )
}

fn verdict_from_state(
    runtime_id: &str,
    state: AuthPreflightState,
    command: String,
) -> AuthPreflightVerdict {
    let remedy = matches!(state, AuthPreflightState::CredentialDead { .. })
        .then(|| CLAUDE_LOGIN_REMEDY.to_string());
    AuthPreflightVerdict {
        runtime_id: runtime_id.to_string(),
        state,
        checked_at_ms: now_ms(),
        command,
        remedy,
        cached: false,
    }
}

struct CachedVerdict {
    at: Instant,
    verdict: AuthPreflightVerdict,
}

fn cache() -> &'static Mutex<HashMap<String, CachedVerdict>> {
    static CACHE: OnceLock<Mutex<HashMap<String, CachedVerdict>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cached_within(
    map: &HashMap<String, CachedVerdict>,
    runtime_id: &str,
    now: Instant,
    ttl: Duration,
) -> Option<AuthPreflightVerdict> {
    let entry = map.get(runtime_id)?;
    (now.saturating_duration_since(entry.at) < ttl).then(|| AuthPreflightVerdict {
        cached: true,
        ..entry.verdict.clone()
    })
}

/// The verdict reached within the last [`PREFLIGHT_CACHE_TTL`], if any.
pub(crate) fn cached_preflight_verdict(runtime_id: &str) -> Option<AuthPreflightVerdict> {
    let map = cache().lock().ok()?;
    cached_within(&map, runtime_id, Instant::now(), PREFLIGHT_CACHE_TTL)
}

/// Keep a verdict so the next [`PREFLIGHT_CACHE_TTL`] of reads need no call.
pub(crate) fn remember_preflight_verdict(verdict: &AuthPreflightVerdict) {
    if let Ok(mut map) = cache().lock() {
        map.insert(
            verdict.runtime_id.clone(),
            CachedVerdict {
                at: Instant::now(),
                verdict: AuthPreflightVerdict {
                    cached: false,
                    ..verdict.clone()
                },
            },
        );
    }
}

/// The login hint the catalog carries when the live check found the
/// credential dead: the CLI's own line, then the remedy.
pub(crate) fn credential_dead_login_hint(sentence: &str) -> String {
    if sentence.is_empty() {
        CLAUDE_LOGIN_REMEDY.to_string()
    } else {
        format!("{sentence} — {CLAUDE_LOGIN_REMEDY}")
    }
}

/// Run the runtime's pre-flight, serving a cached verdict unless `force`.
///
/// Returns `None` for a runtime that has no pre-flight. A runtime whose CLI
/// cannot be resolved gets an `Unknown` verdict rather than an error: that is
/// a fact about the machine the UI should show, not a fault in the caller.
pub(crate) fn run_runtime_auth_preflight(
    runtime_id: &str,
    force: bool,
) -> Option<AuthPreflightVerdict> {
    let (binary_label, args) = preflight_command_for(runtime_id)?;
    if !force {
        if let Some(cached) = cached_preflight_verdict(runtime_id) {
            return Some(cached);
        }
    }
    // One live call at a time: a catalog read warming the cache and a UI
    // health check landing together must not spawn two `claude -p` processes.
    // The second caller waits on the first and then reads its verdict.
    static RUN_LOCK: Mutex<()> = Mutex::new(());
    let _running = RUN_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if !force {
        if let Some(cached) = cached_preflight_verdict(runtime_id) {
            return Some(cached);
        }
    }
    let command = preflight_command_line(binary_label, args);
    let Some(binary) = super::resolve_command(binary_label) else {
        let verdict = verdict_from_state(
            runtime_id,
            AuthPreflightState::Unknown {
                reason: format!("the `{binary_label}` CLI could not be found on this computer"),
            },
            command,
        );
        remember_preflight_verdict(&verdict);
        return Some(verdict);
    };
    let runner = ProcessRunner {
        path_env: crate::managed_agents::readiness::cli_probe::augmented_path(),
        cwd: dirs::home_dir(),
    };
    let verdict = preflight_verdict(&runner, runtime_id, &binary, binary_label, args);
    match &verdict.state {
        AuthPreflightState::VerifiedLive => {
            tracing::info!("auth pre-flight for {runtime_id}: verified live")
        }
        AuthPreflightState::CredentialDead { sentence } => {
            tracing::warn!("auth pre-flight for {runtime_id}: credential dead — {sentence}")
        }
        AuthPreflightState::Unknown { reason } => {
            tracing::warn!("auth pre-flight for {runtime_id}: unknown — {reason}")
        }
    }
    remember_preflight_verdict(&verdict);
    Some(verdict)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// A runner that answers with a scripted result and records the call.
    struct FakeRunner {
        result: Result<PreflightOutput, PreflightRunError>,
        calls: RefCell<Vec<(PathBuf, Vec<String>, Duration)>>,
    }

    impl FakeRunner {
        fn new(result: Result<PreflightOutput, PreflightRunError>) -> Self {
            Self {
                result,
                calls: RefCell::new(Vec::new()),
            }
        }
    }

    impl PreflightRunner for FakeRunner {
        fn run(
            &self,
            binary: &Path,
            args: &[&str],
            timeout: Duration,
        ) -> Result<PreflightOutput, PreflightRunError> {
            self.calls.borrow_mut().push((
                binary.to_path_buf(),
                args.iter().map(|a| a.to_string()).collect(),
                timeout,
            ));
            self.result.clone()
        }
    }

    fn output(exit_success: bool, stdout: &str, stderr: &str) -> PreflightOutput {
        PreflightOutput {
            exit_success,
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
        }
    }

    const ADAPTER_ERROR: &str =
        "Failed to authenticate: OAuth session expired and could not be refreshed";

    #[test]
    fn a_call_that_prints_ok_is_verified_live() {
        let runner = FakeRunner::new(Ok(output(true, "ok\n", "")));
        let verdict = preflight_verdict(
            &runner,
            CLAUDE_RUNTIME_ID,
            Path::new("/usr/local/bin/claude"),
            CLAUDE_PREFLIGHT_BINARY,
            CLAUDE_PREFLIGHT_ARGS,
        );
        assert_eq!(verdict.state, AuthPreflightState::VerifiedLive);
        assert_eq!(verdict.remedy, None);
        assert!(!verdict.cached);
        assert_eq!(
            verdict.command,
            "claude -p \"reply with the single word ok\" --max-turns 1 --output-format text"
        );
        // The exact reproducing call from live run 6, under the 20s bound.
        let calls = runner.calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, PathBuf::from("/usr/local/bin/claude"));
        assert_eq!(
            calls[0].1,
            vec![
                "-p",
                "reply with the single word ok",
                "--max-turns",
                "1",
                "--output-format",
                "text"
            ]
        );
        assert_eq!(calls[0].2, Duration::from_secs(20));
    }

    #[test]
    fn a_polite_ok_still_counts() {
        assert_eq!(
            classify_preflight(Ok(output(true, "OK.\n", ""))),
            AuthPreflightState::VerifiedLive
        );
    }

    #[test]
    fn the_verbatim_adapter_error_is_credential_dead_with_its_sentence_and_the_remedy() {
        let runner = FakeRunner::new(Ok(output(false, "", &format!("{ADAPTER_ERROR}\n"))));
        let verdict = preflight_verdict(
            &runner,
            CLAUDE_RUNTIME_ID,
            Path::new("/usr/local/bin/claude"),
            CLAUDE_PREFLIGHT_BINARY,
            CLAUDE_PREFLIGHT_ARGS,
        );
        assert_eq!(
            verdict.state,
            AuthPreflightState::CredentialDead {
                sentence: ADAPTER_ERROR.to_string()
            }
        );
        assert_eq!(
            verdict.remedy.as_deref(),
            Some("Run `claude auth login` in a terminal, then retry.")
        );
    }

    #[test]
    fn the_clis_login_prompt_is_credential_dead_too() {
        assert_eq!(
            classify_preflight(Ok(output(false, "Not logged in · Please run /login\n", ""))),
            AuthPreflightState::CredentialDead {
                sentence: "Not logged in · Please run /login".to_string()
            }
        );
    }

    #[test]
    fn a_timeout_is_unknown_and_says_so() {
        let runner = FakeRunner::new(Err(PreflightRunError::TimedOut(PREFLIGHT_TIMEOUT)));
        let verdict = preflight_verdict(
            &runner,
            CLAUDE_RUNTIME_ID,
            Path::new("/usr/local/bin/claude"),
            CLAUDE_PREFLIGHT_BINARY,
            CLAUDE_PREFLIGHT_ARGS,
        );
        assert_eq!(
            verdict.state,
            AuthPreflightState::Unknown {
                reason: "the check did not finish within 20s".to_string()
            }
        );
        assert_eq!(verdict.remedy, None);
    }

    #[test]
    fn a_failure_that_is_not_about_auth_is_unknown_not_dead() {
        assert_eq!(
            classify_preflight(Ok(output(
                false,
                "",
                "Error: fetch failed: getaddrinfo ENOTFOUND api.anthropic.com\n"
            ))),
            AuthPreflightState::Unknown {
                reason: "Error: fetch failed: getaddrinfo ENOTFOUND api.anthropic.com".to_string()
            }
        );
        assert_eq!(
            classify_preflight(Err(PreflightRunError::Spawn("permission denied".into()))),
            AuthPreflightState::Unknown {
                reason: "the check could not start: permission denied".to_string()
            }
        );
    }

    #[test]
    fn a_success_that_did_not_say_ok_proves_nothing() {
        assert_eq!(
            classify_preflight(Ok(output(true, "I cannot help with that.\n", ""))),
            AuthPreflightState::Unknown {
                reason: "the check succeeded but did not print ok".to_string()
            }
        );
    }

    #[test]
    fn a_token_in_the_error_line_never_reaches_the_verdict() {
        let stderr = "Failed to authenticate with sk-ant-oat01-abcdefghijklmnopqrstuvwxyz0123456789 and Bearer eyJhbGciOi\n";
        let state = classify_preflight(Ok(output(false, "", stderr)));
        let AuthPreflightState::CredentialDead { sentence } = state else {
            panic!("expected credential dead");
        };
        assert!(!sentence.contains("sk-ant"), "{sentence}");
        assert!(!sentence.contains("eyJ"), "{sentence}");
        assert_eq!(
            sentence,
            "Failed to authenticate with [redacted] and Bearer [redacted]"
        );
    }

    #[test]
    fn the_sentence_is_bounded() {
        let long = format!("Failed to authenticate: {}", "x ".repeat(400));
        let state = classify_preflight(Ok(output(false, "", &long)));
        let AuthPreflightState::CredentialDead { sentence } = state else {
            panic!("expected credential dead");
        };
        assert!(
            sentence.chars().count() <= SENTENCE_MAX_CHARS + 1,
            "{sentence}"
        );
        assert!(sentence.ends_with('…'));
    }

    #[test]
    fn a_cached_verdict_is_served_inside_the_ttl_and_not_after() {
        let verdict = verdict_from_state(
            CLAUDE_RUNTIME_ID,
            AuthPreflightState::VerifiedLive,
            "claude".into(),
        );
        let mut map = HashMap::new();
        let at = Instant::now();
        map.insert(
            CLAUDE_RUNTIME_ID.to_string(),
            CachedVerdict {
                at,
                verdict: verdict.clone(),
            },
        );
        let fresh = cached_within(
            &map,
            CLAUDE_RUNTIME_ID,
            at + Duration::from_secs(60),
            PREFLIGHT_CACHE_TTL,
        )
        .expect("inside the ttl");
        assert!(fresh.cached);
        assert_eq!(fresh.state, AuthPreflightState::VerifiedLive);
        assert!(
            cached_within(
                &map,
                CLAUDE_RUNTIME_ID,
                at + PREFLIGHT_CACHE_TTL,
                PREFLIGHT_CACHE_TTL
            )
            .is_none(),
            "a verdict as old as the ttl is stale"
        );
        assert!(cached_within(&map, "codex", at, PREFLIGHT_CACHE_TTL).is_none());
    }

    #[test]
    fn only_claude_has_a_preflight_today() {
        assert!(preflight_command_for("claude").is_some());
        assert!(preflight_command_for("codex").is_none());
        assert!(preflight_command_for("goose").is_none());
        assert!(run_runtime_auth_preflight("goose", true).is_none());
    }

    #[test]
    fn the_login_hint_carries_the_clis_line_then_the_remedy() {
        assert_eq!(
            credential_dead_login_hint(ADAPTER_ERROR),
            format!("{ADAPTER_ERROR} — Run `claude auth login` in a terminal, then retry.")
        );
        assert_eq!(
            credential_dead_login_hint(""),
            "Run `claude auth login` in a terminal, then retry."
        );
    }

    #[test]
    fn the_verdict_serializes_the_way_the_ui_reads_it() {
        let verdict = verdict_from_state(
            CLAUDE_RUNTIME_ID,
            AuthPreflightState::CredentialDead {
                sentence: ADAPTER_ERROR.to_string(),
            },
            "claude -p x".into(),
        );
        let json = serde_json::to_value(&verdict).expect("serializes");
        assert_eq!(json["runtimeId"], "claude");
        assert_eq!(json["state"]["state"], "credential_dead");
        assert_eq!(json["state"]["sentence"], ADAPTER_ERROR);
        assert_eq!(
            json["remedy"],
            "Run `claude auth login` in a terminal, then retry."
        );
        assert_eq!(json["cached"], false);
        assert!(json["checkedAtMs"].is_u64());
    }
}
