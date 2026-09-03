//! `bee git setup` / `bee git status` — provision terminal git access to the
//! relay's own git hosting.
//!
//! The relay authenticates git over NIP-98 (`WWW-Authenticate: Nostr`), which
//! `git-credential-nostr` answers. The helper needs three things and has no way
//! to arrange any of them itself: to be reachable from git, to be told
//! `credential.useHttpPath=true`, and to find a key. Everything here writes
//! exactly those three and nothing else.
//!
//! **Config is URL-scoped, never global.** A bare `credential.helper nostr`
//! would be consulted for every remote including GitHub. The helper does decline
//! politely when the server never sends a `Nostr` challenge — but relying on
//! that makes correctness depend on a remote's behaviour rather than on local
//! configuration. Scoping to `<relay-origin>/git` is the form already used by
//! the desktop's managed agents (`managed_agents/runtime.rs`) and by
//! `scripts/sprig-entrypoint.sh`; this is the third caller of the same shape.
//!
//! **The key on disk is a deliberate, stated tradeoff.** `git-credential-nostr`
//! reads `$NOSTR_PRIVATE_KEY` or a file named by `git config nostr.keyfile` —
//! it cannot read an OS keyring. A terminal that has never run the desktop app
//! therefore has no way to reach a key held only in the keyring, so making
//! `git push` work at all means writing one. `--write-key` is opt-in, refuses to
//! overwrite a file holding a different identity, and enforces 0600 both when
//! writing and when reading back.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use nostr::{Keys, ToBech32};

use buzz_core::coding_session_verdict_admission::{
    active_seats_from_authority_transitions, evaluate_verdict_admission, fold_candidate_records,
    mission_gate_policy, mission_observations, mission_provider_pubkeys, mission_transactions,
    verdict_admission_fold_context, VerdictAdmission, VerdictAdmissionCandidate,
    VerdictAdmissionEvidence, VerdictAdmissionQuery, VerdictAdmissionRefusal,
    VERDICT_ADMISSION_MAX_AUTHORITY_TRANSITIONS, VERDICT_ADMISSION_MAX_OBSERVATIONS,
    VERDICT_ADMISSION_MAX_POLICIES, VERDICT_ADMISSION_MAX_PROVIDER_METADATA,
    VERDICT_ADMISSION_MAX_SESSIONS, VERDICT_ADMISSION_MAX_TRANSACTIONS,
};
use buzz_core::kind::{
    KIND_CODING_SESSION_AUTHORITY_TRANSITION, KIND_CODING_SESSION_GENESIS,
    KIND_CODING_SESSION_METADATA, KIND_CODING_SESSION_OBSERVATION, KIND_CODING_SESSION_POLICY,
    KIND_CODING_SESSION_TEAM_TRANSACTION,
};

use crate::error::CliError;

/// Git config scope to write into.
#[derive(Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum ConfigScope {
    /// `~/.gitconfig` — applies to every checkout (the useful default).
    Global,
    /// `.git/config` — this repository only.
    Local,
}

impl ConfigScope {
    fn flag(self) -> &'static str {
        match self {
            Self::Global => "--global",
            Self::Local => "--local",
        }
    }
}

/// The credential scope a relay's git hosting lives under.
///
/// Git matches `credential.<url>.<key>` by scheme, host and port, and — when
/// the configured URL carries a path — by path prefix. The relay serves git at
/// `/git/{owner}/{repo}`, so `<origin>/git` covers exactly the git transport
/// and nothing else on the same host.
pub fn credential_scope(relay_url: &str) -> Result<String, CliError> {
    let trimmed = relay_url.trim();
    if trimmed.is_empty() {
        return Err(CliError::Usage(
            "relay URL is empty (pass --relay-url or set BUZZ_RELAY_URL)".into(),
        ));
    }
    // Accept the ws:// forms the rest of the CLI takes, since a user will
    // paste whatever BUZZ_RELAY_URL holds.
    let normalized = match trimmed.split_once("://") {
        Some(("ws", rest)) => format!("http://{rest}"),
        Some(("wss", rest)) => format!("https://{rest}"),
        _ => trimmed.to_string(),
    };
    let parsed = url::Url::parse(&normalized)
        .map_err(|e| CliError::Usage(format!("relay URL {trimmed} is not a URL: {e}")))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(CliError::Usage(format!(
            "relay URL {trimmed} must be http(s) or ws(s), got scheme '{}'",
            parsed.scheme()
        )));
    }
    let host = parsed.host_str().ok_or_else(|| {
        CliError::Usage(format!(
            "relay URL {trimmed} has no host to scope credentials to"
        ))
    })?;
    let authority = match parsed.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_string(),
    };
    Ok(format!("{}://{}/git", parsed.scheme(), authority))
}

/// Locate `git-credential-nostr`.
///
/// Returns the value to write into `credential.<scope>.helper`: the bare
/// `nostr` shorthand when git can find the binary on `PATH` itself (which
/// survives a reinstall moving the file), or an absolute path when the caller
/// named one explicitly.
pub fn resolve_helper(explicit: Option<&Path>) -> Result<String, CliError> {
    if let Some(path) = explicit {
        if !path.is_file() {
            return Err(CliError::Usage(format!(
                "--helper {} is not a file",
                path.display()
            )));
        }
        // Absolute, because git resolves a relative helper against the current
        // directory at *use* time — a config entry written from the repo root
        // would stop working the moment you `cd` into a subdirectory.
        let absolute = path.canonicalize().map_err(|e| {
            CliError::Usage(format!("cannot resolve --helper {}: {e}", path.display()))
        })?;
        // Forward slashes work on every platform git supports; Git for Windows
        // invokes helpers through MinGW bash, which treats `\` as an escape.
        return Ok(absolute.to_string_lossy().replace('\\', "/"));
    }
    if which_on_path("git-credential-nostr").is_some() {
        return Ok("nostr".to_string());
    }
    Err(CliError::Usage(
        "git-credential-nostr was not found on PATH. Install it with \
         `cargo install --path crates/git-credential-nostr`, or pass \
         --helper <path> to point at a copy you already have."
            .into(),
    ))
}

fn which_on_path(command: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(command))
        .find(|candidate| is_executable_file(candidate))
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// Default key file location — the path `git-credential-nostr`'s README names.
pub fn default_keyfile() -> Result<PathBuf, CliError> {
    let home = dirs::home_dir().ok_or_else(|| {
        CliError::Usage("cannot resolve a home directory for the key file".into())
    })?;
    Ok(home.join(".nostr").join("key"))
}

/// The config entries that make terminal git access work.
///
/// Each key carries a *list* of values, because the helper entry needs two.
/// The empty first value resets the helper list for this URL scope: git
/// otherwise **appends** a scoped helper to whatever the system and global
/// configs already named. On this machine `/opt/homebrew/etc/gitconfig` sets
/// `credential.helper = osxkeychain`, so relay requests ran both helpers —
/// nostr answered correctly, then osxkeychain tried to `store` an ephemeral
/// credential and every successful operation printed
/// `fatal: failed to store: -1`. A "fatal" over a request that worked is worse
/// than noise; it sends you looking for a failure that did not happen.
///
/// `desktop/src-tauri/src/commands/project_git_exec.rs` does the same reset for
/// the same reason.
pub fn config_entries(scope: &str, helper: &str, keyfile: &Path) -> Vec<(String, Vec<String>)> {
    vec![
        (
            format!("credential.{scope}.helper"),
            vec![String::new(), helper.to_string()],
        ),
        (
            format!("credential.{scope}.useHttpPath"),
            vec!["true".to_string()],
        ),
        (
            "nostr.keyfile".to_string(),
            vec![keyfile.to_string_lossy().replace('\\', "/")],
        ),
    ]
}

/// Read a key file exactly as `git-credential-nostr` reads it.
///
/// Thin wrapper over [`git_credential_nostr::read_keyfile`] — the checks (0600,
/// regular file, size, `npub1…`) live in the helper crate so `bee` and the
/// helper cannot drift. Only the error *classification* is `bee`'s: an
/// unreadable file is a usage problem (exit 1), unusable material is a key
/// problem (exit 3).
fn read_keyfile(path: &Path) -> Result<Option<Keys>, CliError> {
    git_credential_nostr::read_keyfile(path).map_err(|error| match error {
        git_credential_nostr::KeyError::Access(message) => CliError::Usage(message),
        git_credential_nostr::KeyError::Material(message) => CliError::Key(message),
    })
}

/// Where `git-credential-nostr` found the key it will sign git with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyOrigin {
    /// `$NOSTR_PRIVATE_KEY` — what the ACP harness injects into a managed
    /// seat, and what therefore wins over anything on disk.
    Env,
    /// The file named by `git config nostr.keyfile`, or the default path.
    Keyfile(PathBuf),
}

impl KeyOrigin {
    /// How to name this source in output: "NOSTR_PRIVATE_KEY" or the path.
    pub fn label(&self) -> String {
        match self {
            Self::Env => "NOSTR_PRIVATE_KEY".to_string(),
            Self::Keyfile(path) => path.to_string_lossy().to_string(),
        }
    }
}

/// The identity git will actually present, and the one it will not.
#[derive(Clone, Debug)]
pub struct EffectiveKey {
    /// The key the helper resolves — the key git signs with.
    pub keys: Keys,
    /// Which of the two sources it came from.
    pub origin: KeyOrigin,
    /// The key file's identity, when a key file also exists and holds a
    /// *different* identity than the one git will use.
    ///
    /// A seat runs with `NOSTR_PRIVATE_KEY` set to its own key while the
    /// operator's key file sits in the same shell. Both are real; only one is
    /// used. Reporting the unused one as "the key" is the two-identities-in-one-
    /// shell lie this field exists to prevent.
    pub shadowed: Option<(PathBuf, nostr::PublicKey)>,
}

impl From<git_credential_nostr::ResolvedKey> for EffectiveKey {
    /// Adapt the helper's own resolution to the shape `bee` prints. The
    /// precedence lives in `git_credential_nostr::choose_key`, never here: a
    /// second copy of it is how a report comes to name an identity git does not
    /// use.
    fn from(resolved: git_credential_nostr::ResolvedKey) -> Self {
        Self {
            keys: resolved.keys,
            origin: match resolved.source {
                git_credential_nostr::KeySource::Env => KeyOrigin::Env,
                git_credential_nostr::KeySource::Keyfile(path) => KeyOrigin::Keyfile(path),
            },
            shadowed: resolved.shadowed,
        }
    }
}

impl EffectiveKey {
    /// The one-sentence disclosure when two identities are present, or `None`
    /// when there is nothing to disclose.
    pub fn disclosure(&self) -> Option<String> {
        let (path, other) = self.shadowed.as_ref()?;
        Some(format!(
            "Two keys are configured here: git signs with {} from {}, and {} from {} is not used.",
            short_pubkey(&self.keys.public_key()),
            self.origin.label(),
            short_pubkey(other),
            path.display()
        ))
    }
}

/// First 8 hex characters plus an ellipsis — enough to tell two keys apart
/// without printing a full identity into a log.
fn short_pubkey(pubkey: &nostr::PublicKey) -> String {
    let hex = pubkey.to_hex();
    format!("{}\u{2026}", &hex[..8.min(hex.len())])
}

/// The key file path the helper would read: `--keyfile`, then
/// `git config nostr.keyfile`, then the default.
fn effective_keyfile_path(keyfile: Option<&Path>) -> Result<PathBuf, CliError> {
    match keyfile {
        Some(path) => Ok(path.to_path_buf()),
        None => match git_config_get("nostr.keyfile") {
            Some(configured) => Ok(PathBuf::from(configured)),
            None => default_keyfile(),
        },
    }
}

/// Resolve the key git will present, exactly as `git-credential-nostr` does:
/// `$NOSTR_PRIVATE_KEY` first, then `git config nostr.keyfile`.
///
/// Reproducing the helper's precedence is the whole point. A check that read
/// `BUZZ_PRIVATE_KEY` instead would test a different identity than the one git
/// actually presents, and could report success while every push failed.
pub fn resolve_effective_key(keyfile: Option<&Path>) -> Result<EffectiveKey, CliError> {
    let path = effective_keyfile_path(keyfile)?;
    // The helper's own resolution, including its rule that a broken key file
    // must not mask a working `NOSTR_PRIVATE_KEY` — the helper would never open
    // the file in that case, so neither does this. `bee git status` still
    // reports the file's problem in its own field.
    let resolved = git_credential_nostr::resolve_key(&path).map_err(|error| match error {
        git_credential_nostr::KeyError::Access(message) => CliError::Usage(message),
        git_credential_nostr::KeyError::Material(message) => CliError::Key(message),
    })?;
    let resolved = resolved.ok_or_else(|| {
        CliError::Usage(format!(
            "no key: {} does not exist and NOSTR_PRIVATE_KEY is unset",
            path.display()
        ))
    })?;
    Ok(EffectiveKey::from(resolved))
}

/// Write `keys` to `path` at mode 0600, refusing to replace a different identity.
///
/// Silently overwriting would be the worst outcome here: the previous key may be
/// the only copy of an identity, and the failure would be invisible until
/// something signed with the wrong one.
pub fn write_keyfile(path: &Path, keys: &Keys) -> Result<bool, CliError> {
    if let Some(existing) = read_keyfile(path)? {
        if existing.public_key() == keys.public_key() {
            return Ok(false);
        }
        return Err(CliError::Usage(format!(
            "{} already holds a different identity ({}). Refusing to overwrite it — \
             move it aside first if you really mean to replace it.",
            path.display(),
            existing.public_key().to_hex()
        )));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| CliError::Usage(format!("cannot create {}: {e}", parent.display())))?;
    }
    let nsec = keys
        .secret_key()
        .to_bech32()
        .map_err(|e| CliError::Key(format!("cannot encode the key: {e}")))?;

    // Create with 0600 from the outset. Writing first and chmod'ing after would
    // leave the key world-readable for the width of that window.
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|e| CliError::Usage(format!("cannot create {}: {e}", path.display())))?;
    writeln!(file, "{nsec}")
        .map_err(|e| CliError::Usage(format!("cannot write {}: {e}", path.display())))?;
    Ok(true)
}

/// Set `key` to exactly `values`, replacing whatever was there.
///
/// `--unset-all` first, then `--add` per value: plain `git config key value`
/// replaces only a single value and errors on a multi-valued key, so re-running
/// setup would either fail or silently append a second helper each time.
fn git_config_set(scope: ConfigScope, key: &str, values: &[String]) -> Result<(), CliError> {
    // Exit 5 is "nothing to unset", which is the normal first-run state.
    let unset = Command::new("git")
        .args(["config", scope.flag(), "--unset-all", key])
        .output()
        .map_err(|e| CliError::Usage(format!("cannot run git: {e}")))?;
    if !unset.status.success() && unset.status.code() != Some(5) {
        return Err(CliError::Usage(format!(
            "git config {} --unset-all {key} failed: {}",
            scope.flag(),
            String::from_utf8_lossy(&unset.stderr).trim()
        )));
    }
    for value in values {
        let output = Command::new("git")
            .args(["config", scope.flag(), "--add", key, value])
            .output()
            .map_err(|e| CliError::Usage(format!("cannot run git: {e}")))?;
        if !output.status.success() {
            return Err(CliError::Usage(format!(
                "git config {} --add {key} failed: {}",
                scope.flag(),
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
    }
    Ok(())
}

/// Last value of a possibly multi-valued key — the one git resolves to.
fn git_config_get_last(key: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["config", "--get-all", key])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .rfind(|line| !line.trim().is_empty())
        .map(str::to_string)
}

fn git_config_get(key: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["config", "--get", key])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!value.is_empty()).then_some(value)
}

/// Arguments for `bee git setup`, resolved from the CLI.
pub struct SetupRequest<'a> {
    pub relay_url: &'a str,
    pub helper: Option<PathBuf>,
    pub keyfile: Option<PathBuf>,
    pub scope: ConfigScope,
    /// Write the key to the key file. Requires `BUZZ_PRIVATE_KEY`.
    pub write_key: bool,
    /// Print the commands that would run; change nothing.
    pub print_only: bool,
    /// The identity, when one is available.
    pub keys: Option<Keys>,
}

pub fn cmd_setup(request: SetupRequest<'_>) -> Result<(), CliError> {
    let scope_url = credential_scope(request.relay_url)?;
    let helper = resolve_helper(request.helper.as_deref())?;
    let keyfile = match request.keyfile {
        Some(path) => path,
        None => default_keyfile()?,
    };
    let entries = config_entries(&scope_url, &helper, &keyfile);

    if request.print_only {
        for (key, values) in &entries {
            println!("git config {} --unset-all '{key}'", request.scope.flag());
            for value in values {
                println!(
                    "git config {} --add '{key}' '{value}'",
                    request.scope.flag()
                );
            }
        }
        if request.write_key {
            println!(
                "# plus: write your nsec to {} at mode 0600",
                keyfile.display()
            );
        }
        return Ok(());
    }

    if request.write_key {
        let keys = request.keys.as_ref().ok_or_else(|| {
            CliError::Auth(
                "--write-key needs an identity (set BUZZ_PRIVATE_KEY or pass --private-key)".into(),
            )
        })?;
        if write_keyfile(&keyfile, keys)? {
            println!("Wrote {} (mode 0600).", keyfile.display());
        } else {
            println!(
                "{} already holds this identity — left as it is.",
                keyfile.display()
            );
        }
    }

    for (key, values) in &entries {
        git_config_set(request.scope, key, values)?;
    }
    println!("Configured {} for {scope_url}.", request.scope.flag());

    // Report the gap rather than implying success. Setup that writes config over
    // a missing key file leaves a setup that cannot push, and the only symptom
    // is a 401 much later.
    match read_keyfile(&keyfile) {
        Ok(Some(keys)) => {
            println!(
                "Key file {} holds {}.",
                keyfile.display(),
                keys.public_key().to_hex()
            );
            // Not "ready" — see cmd_status. Local config being complete says
            // nothing about whether the relay accepts this key.
            println!("Local config is complete. Run `bee git check` to confirm");
            println!("the relay accepts this key.");
        }
        Ok(None) => {
            println!();
            println!(
                "No key file at {} yet — pushes will still fail.",
                keyfile.display()
            );
            println!("Write your nsec there at mode 0600, or re-run with --write-key.");
        }
        Err(error) => {
            println!();
            println!("Key file problem: {error}");
        }
    }
    Ok(())
}

pub fn cmd_status(relay_url: &str, keyfile: Option<PathBuf>) -> Result<(), CliError> {
    let scope_url = credential_scope(relay_url)?;
    // Multi-valued by design (empty reset, then the helper). The last entry is
    // the one git ends up using.
    let configured_helper = git_config_get_last(&format!("credential.{scope_url}.helper"));
    let configured_path = git_config_get(&format!("credential.{scope_url}.useHttpPath"));
    let configured_keyfile = git_config_get("nostr.keyfile");
    // Report the *effective* key file, falling back to the default the setup
    // path would use. Reporting null when the config is unset would push the
    // default into every caller, and a caller that guessed a different one
    // would tell the user about a file nothing reads.
    let keyfile_path = match keyfile {
        Some(path) => Some(path),
        None => match configured_keyfile.as_ref() {
            Some(configured) => Some(PathBuf::from(configured)),
            None => default_keyfile().ok(),
        },
    };

    let helper_ok = match configured_helper.as_deref() {
        None => None,
        Some("nostr") => Some(which_on_path("git-credential-nostr").is_some()),
        Some(path) => Some(is_executable_file(Path::new(path))),
    };
    let (key_present, key_pubkey, key_problem) = match keyfile_path.as_deref() {
        None => (false, None, None),
        Some(path) => match read_keyfile(path) {
            Ok(Some(keys)) => (true, Some(keys.public_key().to_hex()), None),
            Ok(None) => (false, None, None),
            Err(error) => (true, None, Some(error.to_string())),
        },
    };

    // The key file is not necessarily the key git uses. `$NOSTR_PRIVATE_KEY`
    // wins — that is the helper's precedence, and it is how the ACP harness
    // gives a seat its own identity — so report the key that will actually be
    // presented, name where it came from, and say plainly when a second,
    // unused identity is sitting in the same shell.
    let effective = resolve_effective_key(keyfile_path.as_deref());
    let (effective_pubkey, key_source, key_disclosure, effective_problem) = match &effective {
        Ok(resolved) => (
            Some(resolved.keys.public_key().to_hex()),
            Some(resolved.origin.label()),
            resolved.disclosure(),
            None,
        ),
        Err(error) => (None, None, None, Some(error.to_string())),
    };

    // NOT "ready". This says the three local pieces are in place — it cannot
    // say the relay accepts the key, and reporting `ready: true` over a 403 is
    // exactly the kind of comfortable guess this project treats as a bug.
    // `bee git check` is the one that asks.
    // A key git will present, from either source — not merely a key file.
    let configured = helper_ok == Some(true)
        && configured_path.as_deref() == Some("true")
        && effective_pubkey.is_some();

    let report = serde_json::json!({
        "scope": scope_url,
        "helper": configured_helper,
        // Distinguishes "configured but the binary is gone" from "never set up".
        // A status that reads ready over a helper path that no longer exists is
        // exactly the lie this field exists to prevent.
        "helper_resolvable": helper_ok,
        "use_http_path": configured_path,
        "keyfile": keyfile_path.as_ref().map(|p| p.to_string_lossy().to_string()),
        "keyfile_present": key_present,
        "keyfile_pubkey": key_pubkey,
        "keyfile_problem": key_problem,
        // The key git WILL sign with, and where the helper found it. Reporting
        // only `keyfile_pubkey` named the operator's key in a seat's shell,
        // where git actually signs as the seat.
        "effective_pubkey": effective_pubkey,
        "key_source": key_source,
        "key_problem": effective_problem,
        // One sentence, present only when two identities are configured and
        // they differ. Null is the honest value for "nothing to disclose".
        "key_disclosure": key_disclosure,
        "configured": configured,
        // Deliberately absent: anything named `ready`. Whether a push works is
        // a question for the relay — run `bee git check`.
        "next": if configured {
            "run `bee git check` to confirm the relay accepts this key"
        } else {
            "run `bee git setup`"
        },
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&report).unwrap_or_default()
    );
    Ok(())
}

/// Probe outcome for one repository.
pub struct RepoProbe {
    /// The `d` tag of the kind:30617 announcement.
    pub repo_id: String,
    /// The announcement author, which is the repo owner in the URL.
    pub owner: String,
    /// The HTTP status the git transport returned.
    pub status: u16,
    /// `read`, `denied`, `no-grant-or-missing`, `auth-rejected`, `unexpected`.
    pub access: &'static str,
    /// The relay's own words, when it said anything.
    pub detail: Option<String>,
}

/// Classify a git `info/refs` response.
///
/// The relay answers a denied read with **404, not 403** — deliberately, so
/// membership is not probeable by a stranger. That means "not found" and "you
/// have no grant" are the same wire response and must not be reported as if we
/// could tell them apart.
fn classify_probe(status: u16, body: &str) -> (&'static str, Option<String>) {
    match status {
        200 => ("read", None),
        401 => ("auth-rejected", Some(body.trim().to_string())),
        403 => (
            "denied",
            Some(if body.trim().is_empty() {
                "forbidden".to_string()
            } else {
                body.trim().to_string()
            }),
        ),
        404 => (
            "no-grant-or-missing",
            Some("the relay returns 404 for both; it will not distinguish them".to_string()),
        ),
        _ => (
            "unexpected",
            Some(format!("HTTP {status}: {}", body.trim())),
        ),
    }
}

/// The owner attestation a git request will carry, and what it is worth.
#[derive(Default)]
pub struct ProbeAttestation {
    /// The tag the helper would attach to the signed NIP-98 event. Sent as-is
    /// even when it does not verify — the helper does not check it either, and
    /// the point of the probe is to ask the relay the question git asks.
    pub tag: Option<nostr::Tag>,
    /// The owner, only once the attestation is proven to cover the key git
    /// signs with. Reporting an owner from an unverified tag would claim a
    /// relationship the relay is about to reject.
    pub owner: Option<String>,
    /// Why the attestation will not be honoured, when it will not be.
    pub warning: Option<String>,
}

/// Read the NIP-OA owner attestation the ACP harness injects, or the one
/// `git config nostr.authtag` holds, exactly as `git-credential-nostr` does.
///
/// A *malformed* attestation is an error, not a shrug: the helper fails closed
/// on it, so a probe that quietly dropped it would pass where every real push
/// fails. An attestation signed for a *different* key is not an error — it is
/// the shape of the two-identities-in-one-shell trap, and it is reported.
fn probe_attestation(keys: &Keys) -> Result<ProbeAttestation, CliError> {
    // The helper's own reader: same sources (`BUZZ_AUTH_TAG`, then
    // `git config nostr.authtag`), same fail-closed rule on a malformed tag.
    let Some(tag) = git_credential_nostr::resolve_auth_tag().map_err(CliError::Auth)? else {
        return Ok(ProbeAttestation::default());
    };
    let raw = serde_json::to_string(&tag.clone().to_vec())
        .map_err(|e| CliError::Auth(format!("cannot re-encode the attestation: {e}")))?;

    match buzz_sdk::nip_oa::verify_auth_tag(&raw, &keys.public_key()) {
        Ok(owner) => Ok(ProbeAttestation {
            tag: Some(tag),
            owner: Some(owner.to_hex()),
            warning: None,
        }),
        Err(e) => Ok(ProbeAttestation {
            tag: Some(tag),
            owner: None,
            warning: Some(format!(
                "BUZZ_AUTH_TAG is not signed for {}, the key git uses, so the relay will ignore it ({e})",
                short_pubkey(&keys.public_key())
            )),
        }),
    }
}

/// Sign the NIP-98 event `git-credential-nostr` signs, attestation included.
///
/// One signing path, in the helper crate: a check that signed its own variant
/// of the event would ask the relay a question git never asks.
fn sign_git_nip98(
    keys: &Keys,
    method: &str,
    url: &str,
    auth_tag: Option<nostr::Tag>,
) -> Result<String, CliError> {
    let method: nostr::nips::nip98::HttpMethod = method
        .parse()
        .map_err(|_| CliError::Other(format!("unsupported HTTP method {method}")))?;
    git_credential_nostr::authorization_header(keys, method, url, auth_tag).map_err(CliError::Other)
}

/// Sign the repo-root URL the credential helper signs.
///
/// `git-credential-nostr` strips `/info/refs`, `/git-upload-pack` and
/// `/git-receive-pack` and signs the repo root, because git invokes a helper
/// once per challenge and reuses the header across the GET and the POST. A
/// probe signed for the exact path would be rejected where real git succeeds —
/// see docs/git-nip98-method-binding.md.
fn repo_root_url(relay_origin: &str, owner: &str, repo: &str) -> String {
    format!("{relay_origin}/git/{owner}/{repo}")
}

/// The owner attestation's state, as the relay will treat it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttestationState {
    /// No `BUZZ_AUTH_TAG` and no `nostr.authtag`: the key answers for itself.
    Absent,
    /// Verified to cover the key git signs with — the relay will honour it.
    Present {
        /// The owner whose relay membership this key inherits.
        owner: String,
    },
    /// Well formed, so the helper still sends it, but not signed for this key.
    /// The relay ignores it, so the key answers for itself after all.
    NotForThisKey {
        /// Why it will not be honoured.
        problem: String,
    },
    /// Unparseable. `git-credential-nostr` fails closed on it, so no git
    /// request can be made at all — not one that is denied, one that never
    /// leaves the machine.
    Malformed {
        /// What is wrong with it.
        problem: String,
    },
}

impl AttestationState {
    /// The one-word state for the report: present, absent, or invalid.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Absent => "absent",
            Self::Present { .. } => "present",
            Self::NotForThisKey { .. } | Self::Malformed { .. } => "invalid",
        }
    }
}

/// What the git transport answered, in the only terms that matter: whether
/// `git clone` and `git push` will work with this key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportVerdict {
    /// The relay authorized this key on the smart-HTTP transport.
    Accepted,
    /// The relay refused it. Git will fail the same way.
    Denied,
    /// The relay answered something that is neither — a 5xx, say. Nothing is
    /// known about the key, and saying either word would be a guess.
    Unavailable,
}

/// One `info/refs` probe: the request git makes, and what came back.
#[derive(Debug, Clone)]
pub struct TransportProbe {
    /// `git-upload-pack` (clone/fetch) or `git-receive-pack` (push).
    pub service: &'static str,
    /// The exact URL probed.
    pub url: String,
    /// The HTTP status the relay returned.
    pub status: u16,
    /// Accepted, denied, or unknown.
    pub verdict: TransportVerdict,
    /// What that status means here, in one sentence.
    pub detail: String,
}

/// Which repository the transport probe asked about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeTarget {
    /// A remote in this checkout that points at the relay — the repository
    /// this shell would actually push to.
    Remote {
        /// Repo owner pubkey, from the remote URL.
        owner: String,
        /// Repo id, from the remote URL.
        repo: String,
    },
    /// A repository that cannot exist. The relay checks membership *before* it
    /// resolves the repository, so a 404 here isolates the authorization gate
    /// from repository access.
    Gate {
        /// The key's own pubkey — any owner would do.
        owner: String,
        /// The impossible repo id.
        repo: String,
    },
}

impl ProbeTarget {
    fn owner(&self) -> &str {
        match self {
            Self::Remote { owner, .. } | Self::Gate { owner, .. } => owner,
        }
    }

    fn repo(&self) -> &str {
        match self {
            Self::Remote { repo, .. } | Self::Gate { repo, .. } => repo,
        }
    }

    /// True when the repository probed is a real one, which changes what a 404
    /// is allowed to mean.
    fn is_real_repo(&self) -> bool {
        matches!(self, Self::Remote { .. })
    }
}

/// What the relay's *HTTP* membership path said — a different gate from git's,
/// reported so a person can see the two differ.
#[derive(Debug, Clone)]
pub enum HttpMembership {
    /// The HTTP bridge answered; this many repo announcements were visible.
    Accepted {
        /// Count of kind:30617 announcements returned.
        announcements: usize,
    },
    /// The HTTP bridge refused, or could not be reached. Never the verdict.
    Refused {
        /// The relay's own words, with no invented remedy attached.
        detail: String,
    },
}

/// The repo id used to probe the authorization gate. It cannot exist: the name
/// is not a valid repo id anywhere, and no announcement can create it.
const GATE_PROBE_REPO: &str = "membership-probe-does-not-exist";

/// Map an `info/refs` status onto the verdict git itself will act on.
///
/// `real_repo` is load-bearing. The relay answers a denied read with **404, not
/// 403**, deliberately, so membership is not probeable by a stranger — which
/// makes 404 on a real repository ambiguous. On the gate probe, where the
/// repository cannot exist, the same 404 proves the opposite: the request got
/// past the membership gate and died at repository resolution.
fn transport_verdict(status: u16, real_repo: bool) -> (TransportVerdict, String) {
    transport_verdict_for("git-upload-pack", status, real_repo)
}

/// The exact sentence a push probe earns.
///
/// `git-receive-pack`'s advertisement is the READ gate and nothing else: the
/// pre-receive hook has not run, no ref update has been named, and no
/// protection rule has been consulted. Reporting a bare `Accepted` there read
/// as "your push will work", which is not what was tested — and, since batch 3,
/// is not even the last word on whether the relay will take the commit.
fn transport_verdict_for(
    service: &str,
    status: u16,
    real_repo: bool,
) -> (TransportVerdict, String) {
    if service == "git-receive-pack" && status == 200 {
        return (
            TransportVerdict::Accepted,
            "the relay advertised refs to a push client; this is the read gate only — the \
             pre-receive hook decides the push itself"
                .to_string(),
        );
    }
    match status {
        200 => (
            TransportVerdict::Accepted,
            "the relay served the ref advertisement".to_string(),
        ),
        401 => (
            TransportVerdict::Denied,
            "the relay rejected the NIP-98 credential itself".to_string(),
        ),
        403 => (
            TransportVerdict::Denied,
            "the relay does not say why; likely membership or attestation".to_string(),
        ),
        404 if real_repo => (
            TransportVerdict::Denied,
            "the relay answers 404 for both 'no grant on this repository' and \
             'no such repository', and will not distinguish them"
                .to_string(),
        ),
        404 => (
            TransportVerdict::Accepted,
            "the relay authorized this key at the git gate; the probe repository \
             does not exist, which is how the gate is told apart from repository access"
                .to_string(),
        ),
        other => (
            TransportVerdict::Unavailable,
            format!("HTTP {other}: the relay answered neither yes nor no"),
        ),
    }
}

/// What to do about a denial.
///
/// **Never "unset BUZZ_AUTH_TAG".** The remedy this replaces said exactly that
/// (ledger draft 92): the check denied a seat over the relay's HTTP membership
/// path while `git push` from the same key succeeded seconds later, and a seat
/// that followed the advice would have dropped the owner attestation its push
/// depends on.
fn remedy(
    verdict: TransportVerdict,
    attestation: &AttestationState,
    gate_accepted: bool,
) -> Option<String> {
    if verdict == TransportVerdict::Accepted {
        return None;
    }
    // The gate probe already proved the key itself is admitted, so nothing about
    // membership or the attestation is the problem here — saying otherwise would
    // send someone to fix a thing that is not broken.
    if gate_accepted {
        return Some(
            "This key is admitted by the relay; what it lacks is a grant on this repository. \
             Ask the operator for a role on the project the repository belongs to — or check \
             the remote, because a repository that does not exist answers identically."
                .to_string(),
        );
    }
    Some(match attestation {
        AttestationState::Present { owner } => format!(
            "Ask the operator to confirm this seat's owner ({}) is a member of this relay. \
             Keep the attestation: it is what carries the owner's grant.",
            short_hex(owner)
        ),
        AttestationState::Absent => "This key answers for itself alone — it carries no owner \
             attestation. Ask the operator to add it to the relay, or run this from a seat the \
             desktop attested."
            .to_string(),
        AttestationState::NotForThisKey { problem } => format!(
            "The owner attestation will not be honoured, so this key answers for itself: {problem}"
        ),
        AttestationState::Malformed { problem } => format!(
            "git-credential-nostr fails closed on this attestation, so no git request is made \
             at all: {problem}"
        ),
    })
}

/// Drop the CLI client's generic 403 hint from a line this command prints.
///
/// `client.rs` appends "(BUZZ_AUTH_TAG is set — it may be stale or revoked; try
/// unsetting it)" to every 403. On a *git* answer that advice is actively
/// harmful — the attestation in `BUZZ_AUTH_TAG` is what a seat's push depends
/// on — so it never rides along on this command's output.
fn strip_auth_tag_hint(message: &str) -> String {
    match message.split_once(" (BUZZ_AUTH_TAG is set") {
        Some((head, _)) => head.trim().to_string(),
        None => message.trim().to_string(),
    }
}

/// First 8 hex characters plus an ellipsis.
fn short_hex(hex: &str) -> String {
    format!("{}\u{2026}", &hex[..8.min(hex.len())])
}

/// Find a remote in this checkout that points at the relay's git hosting.
///
/// Parses `git remote -v` output. A shell sitting in a checkout of a relay repo
/// is the case that matters: probing the repository git would actually contact
/// answers the real question, where the gate probe only answers half of it.
fn parse_remote_target(origin: &str, remotes: &str) -> Option<(String, String)> {
    let prefix = format!("{origin}/git/");
    for line in remotes.lines() {
        let url = line.split_whitespace().nth(1)?;
        let Some(rest) = url.strip_prefix(&prefix) else {
            continue;
        };
        let rest = rest.trim_end_matches('/');
        let rest = rest.strip_suffix(".git").unwrap_or(rest);
        let mut parts = rest.splitn(2, '/');
        let (Some(owner), Some(repo)) = (parts.next(), parts.next()) else {
            continue;
        };
        if owner.len() == 64 && !repo.is_empty() && !repo.contains('/') {
            return Some((owner.to_string(), repo.to_string()));
        }
    }
    None
}

/// `git remote -v` in the current directory, or empty when there is no repo.
fn git_remotes() -> String {
    Command::new("git")
        .args(["remote", "-v"])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).to_string())
        .unwrap_or_default()
}

/// Everything `bee git check` learned, before any of it is rendered.
pub struct CheckReport {
    /// The relay origin probed.
    pub relay: String,
    /// The key git will present.
    pub pubkey: String,
    /// Where that key came from — `NOSTR_PRIVATE_KEY` or a path.
    pub key_source: String,
    /// The one-sentence disclosure when a second, unused identity is present.
    pub key_disclosure: Option<String>,
    /// The owner attestation's state.
    pub attestation: AttestationState,
    /// Which repository the transport probes asked about.
    pub target: ProbeTarget,
    /// The transport probes, in the order git makes them.
    pub transport: Vec<TransportProbe>,
    /// A follow-up probe of the authorization gate, made only when a *real*
    /// repository answered 404 — the one status that cannot say whether the key
    /// or the repository was the problem. Never changes the verdict: git still
    /// fails on this repository either way.
    pub gate: Option<TransportProbe>,
    /// The relay's HTTP membership path — secondary, never the verdict.
    pub http_membership: HttpMembership,
    /// Per-repository git probes, when the announcements were readable.
    pub repos: Vec<RepoProbe>,
    /// What `--ref` predicted, when it was asked for.
    pub prediction: Option<RefPrediction>,
}

/// The heading every prediction is printed under.
///
/// One sentence, and it says the only two things that matter: this is not the
/// decision, and the decision is made about the commit actually pushed.
pub const PREDICTION_HEADING: &str =
    "Prediction, not a promise. The relay's pre-receive hook decides at push time, on the \
     commit you actually send:";

/// What `bee git check --ref` worked out.
#[derive(Debug, Clone)]
pub struct RefPrediction {
    /// The ref asked about.
    pub ref_name: String,
    /// The commit the answer is about.
    pub sha: String,
    /// The answer.
    pub state: RefPredictionState,
    /// What the serving relay reports being ([`serving_relay_build`]). The
    /// prediction is the *local* build's rule; this says whose relay will
    /// actually decide.
    pub serving_relay: String,
    /// Who founds this repository, and who may rewrite its rules — finding 33.
    /// Empty until an announcement is read; the rule is never predicted
    /// without one.
    pub founders: String,
}

/// The prediction's answer, or why there is none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefPredictionState {
    /// No `require-verdict` rule matches this ref.
    Ungoverned,
    /// This checkout has no remote on the relay, so there is no repository to
    /// read rules from.
    NoRepository,
    /// Something the prediction needs could not be read. Never an answer.
    Unreadable {
        /// The relay's own words.
        detail: String,
    },
    /// The same rule the hook runs admits this commit for this key, by arm
    /// **(A)**: the pusher is a founder of the repository, and no mission was
    /// read at all.
    AdmittedAsFounder,
    /// Admitted by arm **(C)**: a lead's approving disposition over a report
    /// naming this commit, cleared by an independent verifier seat.
    AdmittedByVerdict {
        /// The umbrella whose ruling admits it.
        session_ref: String,
        /// The approving disposition that settled the assignment.
        disposition_event_id: String,
        /// The verifier's `not-refuted` refutation of the same report.
        refutation_event_id: String,
        /// The verifier seat that signed it.
        verifier_pubkey: String,
    },
    /// Admitted by arm **(B)**: every gate this mission requires was observed
    /// green on this exact commit, over a clean worktree, by the mission's own
    /// provider — and no verifier was required.
    AdmittedByObservedGates {
        /// The umbrella whose observations admit it.
        session_ref: String,
        /// The gates that had to be green, in the order they were required.
        gates: Vec<String>,
    },
    /// The same rule refuses it, with the sentence the hook would return.
    Refused {
        /// Verbatim §1j copy, shared with the relay.
        reason: String,
    },
}

impl CheckReport {
    /// Whether the follow-up gate probe proved the key itself is admitted.
    pub fn gate_accepted(&self) -> bool {
        self.gate
            .as_ref()
            .is_some_and(|gate| gate.verdict == TransportVerdict::Accepted)
    }

    /// The verdict the exit code must match: what git will do.
    ///
    /// Denied beats unavailable beats accepted — a push that is refused is
    /// refused however the fetch probe went.
    pub fn verdict(&self) -> TransportVerdict {
        if self.transport.is_empty() {
            return TransportVerdict::Denied;
        }
        if self
            .transport
            .iter()
            .any(|p| p.verdict == TransportVerdict::Denied)
        {
            return TransportVerdict::Denied;
        }
        if self
            .transport
            .iter()
            .any(|p| p.verdict == TransportVerdict::Unavailable)
        {
            return TransportVerdict::Unavailable;
        }
        TransportVerdict::Accepted
    }
}

/// What `bee git check` was asked to do.
pub struct CheckRequest {
    /// Relay URL, in any of the ws/wss/http/https forms.
    pub relay_url: String,
    /// Key file override; defaults to whatever `nostr.keyfile` names.
    pub keyfile: Option<PathBuf>,
    /// Also probe `git-receive-pack` — the request `git push` makes first.
    pub push: bool,
    /// Predict the pre-receive hook's answer for this ref, if given.
    pub ref_name: Option<String>,
    /// The commit the prediction is about; `HEAD` in this checkout by default.
    pub sha: Option<String>,
}

/// Ask the relay the same question git asks, and report what it answered.
///
/// The probe is a real `GET <repo>/info/refs?service=git-upload-pack` (plus
/// `git-receive-pack` when `push` is set) signed exactly as
/// `git-credential-nostr` signs it — same key resolution, same attestation,
/// same repo-root URL, same signing function. Anything less asks a different
/// question than the one the command is presented as answering.
pub async fn run_check(request: &CheckRequest) -> Result<CheckReport, CliError> {
    let scope = credential_scope(&request.relay_url)?;
    let origin = scope.strip_suffix("/git").unwrap_or(&scope).to_string();
    let effective = resolve_effective_key(request.keyfile.as_deref())?;
    let key_source = effective.origin.label();
    let key_disclosure = effective.disclosure();
    let keys = effective.keys.clone();
    let pubkey = keys.public_key().to_hex();

    // A managed seat's git requests carry its owner attestation *inside* the
    // signed NIP-98 event — git's credential protocol cannot add a header — so
    // a probe that omitted it would ask the relay a different question than git
    // asks, and could report a denial over a key the relay admits.
    let (attestation, auth_tag) = match probe_attestation(&keys) {
        Ok(probe) => {
            let state = match (&probe.owner, &probe.warning) {
                (Some(owner), _) => AttestationState::Present {
                    owner: owner.clone(),
                },
                (None, Some(problem)) => AttestationState::NotForThisKey {
                    problem: problem.clone(),
                },
                (None, None) => AttestationState::Absent,
            };
            (state, probe.tag)
        }
        Err(error) => (
            AttestationState::Malformed {
                problem: error.to_string(),
            },
            None,
        ),
    };

    let target = match parse_remote_target(&origin, &git_remotes()) {
        Some((owner, repo)) => ProbeTarget::Remote { owner, repo },
        None => ProbeTarget::Gate {
            owner: pubkey.clone(),
            repo: GATE_PROBE_REPO.to_string(),
        },
    };

    // A malformed attestation never reaches the wire: the helper refuses to
    // sign, so git fails before any request is made. Probing without it would
    // report on a request git will never send.
    if matches!(attestation, AttestationState::Malformed { .. }) {
        return Ok(CheckReport {
            relay: origin,
            pubkey,
            key_source,
            key_disclosure,
            attestation,
            target,
            transport: Vec::new(),
            gate: None,
            http_membership: HttpMembership::Refused {
                detail: "not asked: git-credential-nostr fails closed on the attestation"
                    .to_string(),
            },
            repos: Vec::new(),
            // Nothing was asked of the relay, so nothing is predicted. An
            // empty prediction would read as "ungoverned", which is a claim.
            prediction: None,
        });
    }

    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| CliError::Other(e.to_string()))?;

    let probe = |url: String, keys: Keys, http: reqwest::Client, tag: Option<nostr::Tag>| async move {
        let signed = repo_root_from_refs_url(&url);
        let auth = sign_git_nip98(&keys, "GET", &signed, tag)?;
        let response = http
            .get(&url)
            .header("Authorization", auth)
            .send()
            .await
            .map_err(CliError::Network)?;
        let status = response.status().as_u16();
        let body = response.text().await.unwrap_or_default();
        Ok::<_, CliError>((status, body))
    };

    let mut services: Vec<&'static str> = vec!["git-upload-pack"];
    if request.push {
        services.push("git-receive-pack");
    }
    let mut transport = Vec::new();
    for service in services {
        let url = format!(
            "{}/info/refs?service={service}",
            repo_root_url(&origin, target.owner(), target.repo())
        );
        let (status, _body) =
            probe(url.clone(), keys.clone(), http.clone(), auth_tag.clone()).await?;
        let (verdict, detail) = transport_verdict_for(service, status, target.is_real_repo());
        transport.push(TransportProbe {
            service,
            url,
            status,
            verdict,
            detail,
        });
    }

    // A real repository's 404 is the relay's single answer for "no grant here"
    // and "no such repo". It cannot say whether the *key* got through, so when
    // it happens the gate is asked separately — a repository that cannot exist
    // separates the two cleanly.
    let gate = if target.is_real_repo() && transport.iter().any(|p| p.status == 404) {
        let url = format!(
            "{}/info/refs?service=git-upload-pack",
            repo_root_url(&origin, &pubkey, GATE_PROBE_REPO)
        );
        let (status, _body) =
            probe(url.clone(), keys.clone(), http.clone(), auth_tag.clone()).await?;
        let (verdict, detail) = transport_verdict(status, false);
        Some(TransportProbe {
            service: "git-upload-pack",
            url,
            status,
            verdict,
            detail,
        })
    } else {
        None
    };

    // Secondary, and only ever secondary. This is the relay's HTTP membership
    // path — a different gate from the one git uses, on a different code path.
    // The live failure was letting it decide: it refused a seat whose `git push`
    // succeeded seconds later.
    let client =
        crate::client::BuzzClient::new(request.relay_url.clone(), keys.clone(), None, None)?;
    let announcements = client
        .query(&serde_json::json!({ "kinds": [30617], "limit": 500 }))
        .await;
    let (http_membership, events) = match announcements {
        Ok(raw) => {
            let events: Vec<serde_json::Value> = serde_json::from_str(&raw).unwrap_or_default();
            (
                HttpMembership::Accepted {
                    announcements: events.len(),
                },
                events,
            )
        }
        Err(error) => (
            HttpMembership::Refused {
                detail: strip_auth_tag_hint(&error.to_string()),
            },
            Vec::new(),
        ),
    };

    // Repository inventory. Visibility of the announcement and git read access
    // are separate gates, so each repo is still probed rather than assumed.
    let mut repos: Vec<RepoProbe> = Vec::new();
    if transport
        .iter()
        .all(|p| p.verdict == TransportVerdict::Accepted)
    {
        let mut seen = std::collections::BTreeSet::new();
        for event in events {
            let author = event
                .get("pubkey")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_lowercase();
            let dtag = event
                .get("tags")
                .and_then(|t| t.as_array())
                .and_then(|tags| {
                    tags.iter().find_map(|tag| {
                        let tag = tag.as_array()?;
                        (tag.first()?.as_str()? == "d").then(|| tag.get(1)?.as_str())?
                    })
                })
                .unwrap_or_default()
                .to_string();
            if author.len() != 64 || dtag.is_empty() || !seen.insert((author.clone(), dtag.clone()))
            {
                continue;
            }
            let url = format!(
                "{}/info/refs?service=git-upload-pack",
                repo_root_url(&origin, &author, &dtag)
            );
            let (status, body) = probe(url, keys.clone(), http.clone(), auth_tag.clone()).await?;
            let (access, detail) = classify_probe(status, &body);
            repos.push(RepoProbe {
                repo_id: dtag,
                owner: author,
                status,
                access,
                detail,
            });
        }
    }

    let prediction = match &request.ref_name {
        Some(ref_name) => {
            let mut prediction =
                predict_ref(&client, &target, ref_name, request.sha.as_deref(), &pubkey).await;
            prediction.serving_relay = serving_relay_build(&origin).await;
            Some(prediction)
        }
        None => None,
    };

    Ok(CheckReport {
        relay: origin,
        pubkey,
        key_source,
        key_disclosure,
        attestation,
        target,
        transport,
        gate,
        http_membership,
        repos,
        prediction,
    })
}

/// Resolve the commit a prediction is about: `--sha`, else this checkout's
/// `HEAD`.
fn resolve_sha(sha: Option<&str>) -> Option<String> {
    if let Some(sha) = sha {
        return Some(sha.trim().to_ascii_lowercase());
    }
    Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| {
            String::from_utf8_lossy(&out.stdout)
                .trim()
                .to_ascii_lowercase()
        })
        .filter(|sha| !sha.is_empty())
}

/// Run the relay's own admission rule against the fold this key can read.
///
/// Deliberately the **same** `buzz_core::coding_session_verdict_admission`
/// function the pre-receive hook runs, over candidates assembled by the same
/// shared helpers — one rule, so the two answers cannot drift into disagreeing
/// about the same commit. What differs is the *inputs*: this side reads the
/// authority chain off the wire (prediction-grade; see
/// `active_seats_from_authority_transitions`) where the relay reads its own
/// accepted projection. That, and the commit actually pushed, are why the
/// answer is printed as a prediction.
async fn predict_ref(
    client: &crate::client::BuzzClient,
    target: &ProbeTarget,
    ref_name: &str,
    sha: Option<&str>,
    pusher_pubkey: &str,
) -> RefPrediction {
    let sha_value = resolve_sha(sha).unwrap_or_default();
    let mut prediction = RefPrediction {
        ref_name: ref_name.to_string(),
        sha: sha_value.clone(),
        state: RefPredictionState::NoRepository,
        // Filled in by the caller, which knows the relay origin.
        serving_relay: "serving relay's version unknown".to_string(),
        founders: String::new(),
    };
    let ProbeTarget::Remote { owner, repo } = target else {
        return prediction;
    };
    if sha_value.is_empty() {
        prediction.state = RefPredictionState::Unreadable {
            detail: "no --sha was given and `git rev-parse HEAD` answered nothing here".to_string(),
        };
        return prediction;
    }

    let announcement = match fetch_repo_announcement(client, owner, repo).await {
        Ok(Some(event)) => event,
        Ok(None) => {
            prediction.state = RefPredictionState::Unreadable {
                detail: format!("the relay returned no kind:30617 announcement for {repo}"),
            };
            return prediction;
        }
        Err(error) => {
            prediction.state = RefPredictionState::Unreadable {
                detail: strip_auth_tag_hint(&error.to_string()),
            };
            return prediction;
        }
    };

    let tags: Vec<Vec<String>> = announcement
        .tags
        .iter()
        .map(|tag| tag.as_slice().to_vec())
        .collect();
    let rules = match buzz_core::git_perms::parse_protection_tags(&tags) {
        Ok(parsed) => parsed.rules,
        Err(error) => {
            prediction.state = RefPredictionState::Unreadable {
                detail: format!("the repository's protection rules do not parse: {error}"),
            };
            return prediction;
        }
    };
    if !buzz_core::git_perms::EffectiveRules::for_ref(ref_name, &rules).require_verdict {
        prediction.state = RefPredictionState::Ungoverned;
        return prediction;
    }

    // First `buzz-channel` tag wins, and a malformed one is not a binding —
    // the same fail-closed reading the relay's resolver applies.
    let channel = announcement
        .tags
        .iter()
        .find_map(|tag| match tag.as_slice() {
            [name, value] if name == "buzz-channel" => Some(value.clone()),
            _ => None,
        });
    let Some(channel) = channel.filter(|value| uuid::Uuid::parse_str(value).is_ok()) else {
        prediction.state = RefPredictionState::Refused {
            reason: VerdictAdmissionRefusal::RepositoryUnbound.reason(),
        };
        return prediction;
    };

    // Finding 33: a repository has founders, not an owner. The prediction
    // reads the same set the relay resolves — signer, NIP-34 `maintainers`,
    // and the project roster's Owners — and says in its own sentence when the
    // roster half was not readable from here.
    let founders = crate::commands::repos::repository_founders(client, &announcement).await;
    prediction.founders = founders.rules_sentence();
    let candidates = match fetch_verdict_candidates(client, &channel, &founders).await {
        Ok(candidates) => candidates,
        Err(error) => {
            prediction.state = RefPredictionState::Unreadable {
                detail: strip_auth_tag_hint(&error.to_string()),
            };
            return prediction;
        }
    };

    let query = VerdictAdmissionQuery {
        ref_name,
        new_oid: &sha_value,
        pusher_pubkey,
        repo_founders: founders.pubkeys(),
    };
    prediction.state = match evaluate_verdict_admission(&candidates, &query) {
        VerdictAdmission::Admitted(VerdictAdmissionEvidence::FounderPush { .. }) => {
            RefPredictionState::AdmittedAsFounder
        }
        VerdictAdmission::Admitted(VerdictAdmissionEvidence::VerifierVerdict {
            session_ref,
            disposition_event_id,
            refutation_event_id,
            verifier_pubkey,
            ..
        }) => RefPredictionState::AdmittedByVerdict {
            session_ref,
            disposition_event_id,
            refutation_event_id,
            verifier_pubkey,
        },
        VerdictAdmission::Admitted(VerdictAdmissionEvidence::ObservedGates {
            session_ref,
            gates,
            ..
        }) => RefPredictionState::AdmittedByObservedGates { session_ref, gates },
        VerdictAdmission::Refused(refusal) => RefPredictionState::Refused {
            reason: refusal.reason(),
        },
    };
    prediction
}

/// The repository's current kind:30617 announcement, as its owner published it.
async fn fetch_repo_announcement(
    client: &crate::client::BuzzClient,
    owner: &str,
    repo: &str,
) -> Result<Option<nostr::Event>, CliError> {
    let rows = client
        .query_all(serde_json::json!({
            "kinds": [30617],
            "authors": [owner],
            "#d": [repo],
            "limit": 1,
        }))
        .await?;
    let Some(row) = rows.into_iter().next() else {
        return Ok(None);
    };
    let event: nostr::Event = serde_json::from_value(row)
        .map_err(|error| CliError::Other(format!("relay returned a malformed 30617: {error}")))?;
    Ok(Some(event))
}

/// Assemble the missions on `channel` whose founder is `owner`, folded.
async fn fetch_verdict_candidates(
    client: &crate::client::BuzzClient,
    channel: &str,
    founders: &buzz_core::repository_founders::RepositoryFounders,
) -> Result<Vec<VerdictAdmissionCandidate>, CliError> {
    let decode = |rows: Vec<serde_json::Value>| -> Result<Vec<nostr::Event>, CliError> {
        rows.into_iter()
            .map(|row| {
                serde_json::from_value(row).map_err(|error| {
                    CliError::Other(format!("relay returned a malformed event: {error}"))
                })
            })
            .collect()
    };
    let geneses = decode(
        client
            .query_paginated(
                serde_json::json!({
                    // Every founder's missions, not only the signer's.
                    "kinds": [KIND_CODING_SESSION_GENESIS],
                    "#h": [channel],
                    "authors": founders.pubkeys(),
                }),
                VERDICT_ADMISSION_MAX_SESSIONS as u32,
            )
            .await?,
    )?;
    let transactions = decode(
        client
            .query_paginated(
                serde_json::json!({
                    "kinds": [KIND_CODING_SESSION_TEAM_TRANSACTION],
                    "#h": [channel],
                }),
                VERDICT_ADMISSION_MAX_TRANSACTIONS as u32,
            )
            .await?,
    )?;
    // Bounded like the two reads either side of it. Reading fewer transitions
    // than exist can only shrink the seat list, which can only turn an
    // admission into a refusal — never the other way round.
    let authority = decode(
        client
            .query_paginated(
                serde_json::json!({
                    "kinds": [KIND_CODING_SESSION_AUTHORITY_TRANSITION],
                    "#h": [channel],
                }),
                VERDICT_ADMISSION_MAX_AUTHORITY_TRANSITIONS as u32,
            )
            .await?,
    )?;

    // Arm (B)'s three inputs, bounded like everything either side of them.
    // Reading fewer of any of them can only turn an admission into a refusal,
    // which is the direction a *prediction* must also fail in: `bee git check`
    // may under-promise and must never over-promise.
    let observations = decode(
        client
            .query_paginated(
                serde_json::json!({
                    "kinds": [KIND_CODING_SESSION_OBSERVATION],
                    "#h": [channel],
                }),
                VERDICT_ADMISSION_MAX_OBSERVATIONS as u32,
            )
            .await?,
    )?;
    let policies = decode(
        client
            .query_paginated(
                serde_json::json!({
                    "kinds": [KIND_CODING_SESSION_POLICY],
                    "#h": [channel],
                    "authors": founders.pubkeys(),
                }),
                VERDICT_ADMISSION_MAX_POLICIES as u32,
            )
            .await?,
    )?;
    let session_metadata = decode(
        client
            .query_paginated(
                serde_json::json!({
                    "kinds": [KIND_CODING_SESSION_METADATA],
                    "#h": [channel],
                }),
                VERDICT_ADMISSION_MAX_PROVIDER_METADATA as u32,
            )
            .await?,
    )?;

    let mut candidates = Vec::new();
    for genesis in &geneses {
        let genesis_ref = genesis.id.to_hex();
        let Ok(payload) =
            buzz_core::coding_session_genesis::decode_coding_session_genesis(&genesis.content)
        else {
            continue;
        };
        let events: Vec<nostr::Event> =
            mission_transactions(&payload.session_ref, &genesis_ref, &transactions)
                .into_iter()
                .cloned()
                .collect();
        let seats = active_seats_from_authority_transitions(&authority, &genesis_ref);
        // The mission's founder is whoever signed this genesis — with a
        // founder set that is no longer always the announcement's signer.
        let founder_pubkey = genesis.pubkey.to_hex();
        let context = verdict_admission_fold_context(
            channel,
            payload.session_ref.clone(),
            genesis_ref.clone(),
            founder_pubkey.clone(),
            seats.clone(),
        );
        // A mission that does not fold admits nothing; it must not make the
        // whole prediction unavailable.
        let canonical = fold_candidate_records(&events, &context).unwrap_or_default();
        let observed_gates =
            buzz_core::coding_session_observation::fold_coding_session_observations(
                &mission_observations(&payload.session_ref, &genesis_ref, &observations)
                    .into_iter()
                    .cloned()
                    .collect::<Vec<nostr::Event>>(),
                &buzz_core::coding_session_observation::CodingSessionObservationFoldContext {
                    session_ref: payload.session_ref.clone(),
                    genesis_ref: genesis_ref.clone(),
                    known_assignment_refs: Vec::new(),
                    provider_pubkeys: Some(mission_provider_pubkeys(
                        &payload.session_ref,
                        &session_metadata,
                    )),
                },
            )
            .gates;
        let gate_policy = mission_gate_policy(&payload.session_ref, &genesis_ref, &policies);
        candidates.push(VerdictAdmissionCandidate {
            session_ref: payload.session_ref,
            genesis_ref,
            founder_pubkey,
            canonical,
            active_seats: seats,
            observed_gates,
            gate_policy,
        });
    }
    Ok(candidates)
}

/// What the serving relay says about itself, for the enforcement disclosure.
///
/// **Why this exists.** `bee repos protect list` and `bee git check --ref` both
/// answer from the *local* build's rule table: the local build knows
/// `require-verdict`, so `unknown_rules` comes back empty and the prediction
/// comes back confident — whatever the relay actually serving the repository
/// does. A relay predating the rule parses the token into its own unknown list
/// and ignores it, and a person reading either command would never learn that.
/// Neither command can fix it; both can stop hiding it.
///
/// NIP-11 carries `software` and `version` but advertises no capability for
/// this rule, so the honest line is "the local build evaluated this; here is
/// what the relay reports being". A capability advertisement is the real
/// answer and is a separate lane.
pub const ENFORCEMENT_DISCLOSURE: &str =
    "This answer is the rule as THIS build evaluates it. The relay serving the \
     repository is what enforces it, and a relay predating require-verdict parses \
     the token and ignores it.";

/// Ask the relay's NIP-11 document what build it is.
///
/// Returns the sentence to print. Never an error: a relay that does not answer
/// is disclosed as unknown, because "unknown" is the true answer and an empty
/// line would read as agreement.
pub async fn serving_relay_build(origin: &str) -> String {
    let unknown = "serving relay's version unknown".to_string();
    let Ok(http) = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
    else {
        return unknown;
    };
    let Ok(response) = http
        .get(origin)
        .header("Accept", "application/nostr+json")
        .send()
        .await
    else {
        return unknown;
    };
    if !response.status().is_success() {
        return unknown;
    }
    let Ok(doc) = response.json::<serde_json::Value>().await else {
        return unknown;
    };
    let software = doc.get("software").and_then(|v| v.as_str());
    let version = doc.get("version").and_then(|v| v.as_str());
    match (software, version) {
        (Some(software), Some(version)) => {
            format!("serving relay reports {software} {version}")
        }
        (None, Some(version)) => format!("serving relay reports version {version}"),
        _ => unknown,
    }
}

/// The compact (`--format compact`) form of a report.
pub fn render_json(report: &CheckReport) -> serde_json::Value {
    serde_json::json!({
        "prediction": report.prediction.as_ref().map(|prediction| serde_json::json!({
            "heading": PREDICTION_HEADING,
            "evaluated_by": ENFORCEMENT_DISCLOSURE,
            "serving_relay": prediction.serving_relay,
            "founders": prediction.founders,
            "ref": prediction.ref_name,
            "sha": prediction.sha,
            "arm": match &prediction.state {
                RefPredictionState::AdmittedAsFounder => Some("founder"),
                RefPredictionState::AdmittedByVerdict { .. } => Some("verifier-verdict"),
                RefPredictionState::AdmittedByObservedGates { .. } => Some("observed-gates"),
                _ => None,
            },
            "answer": match &prediction.state {
                RefPredictionState::Ungoverned => "ungoverned",
                RefPredictionState::NoRepository => "no_repository",
                RefPredictionState::Unreadable { .. } => "unreadable",
                RefPredictionState::AdmittedAsFounder => "admitted",
                RefPredictionState::AdmittedByVerdict { .. } => "admitted",
                RefPredictionState::AdmittedByObservedGates { .. } => "admitted",
                RefPredictionState::Refused { .. } => "refused",
            },
            "detail": match &prediction.state {
                RefPredictionState::Ungoverned =>
                    Some("no require-verdict rule governs this ref".to_string()),
                RefPredictionState::NoRepository =>
                    Some("no relay remote in this checkout".to_string()),
                RefPredictionState::Unreadable { detail } => Some(detail.clone()),
                RefPredictionState::Refused { reason } => Some(reason.clone()),
                RefPredictionState::AdmittedAsFounder => Some(
                    "you are a founder of this repository; a founder's push needs no verdict"
                        .to_string()
                ),
                RefPredictionState::AdmittedByVerdict {
                    session_ref, disposition_event_id, refutation_event_id, verifier_pubkey,
                } => Some(format!(
                    "mission {session_ref} approved it (disposition {disposition_event_id}) and \
                     verifier {verifier_pubkey} did not refute it (refutation \
                     {refutation_event_id})"
                )),
                RefPredictionState::AdmittedByObservedGates { session_ref, gates } => {
                    Some(format!(
                        "mission {session_ref} observed {} green on this commit, over a clean \
                         worktree, and requires no verifier",
                        gates.join(", ")
                    ))
                }
            },
        })),
        "relay": report.relay,
        "pubkey": report.pubkey,
        "key_source": report.key_source,
        "key_disclosure": report.key_disclosure,
        "attestation": report.attestation.label(),
        "attested_owner": match &report.attestation {
            AttestationState::Present { owner } => Some(owner.clone()),
            _ => None,
        },
        "attestation_problem": match &report.attestation {
            AttestationState::NotForThisKey { problem }
            | AttestationState::Malformed { problem } => Some(problem.clone()),
            _ => None,
        },
        // The verdict, and the only field an exit code is derived from.
        "git_transport": match report.verdict() {
            TransportVerdict::Accepted => "accepted",
            TransportVerdict::Denied => "denied",
            TransportVerdict::Unavailable => "unavailable",
        },
        "git_probes": report.transport.iter().map(|p| serde_json::json!({
            "service": p.service,
            "url": p.url,
            "http_status": p.status,
            "verdict": match p.verdict {
                TransportVerdict::Accepted => "accepted",
                TransportVerdict::Denied => "denied",
                TransportVerdict::Unavailable => "unavailable",
            },
            "detail": p.detail,
        })).collect::<Vec<_>>(),
        "gate_probe": report.gate.as_ref().map(|gate| serde_json::json!({
            "http_status": gate.status,
            "verdict": match gate.verdict {
                TransportVerdict::Accepted => "accepted",
                TransportVerdict::Denied => "denied",
                TransportVerdict::Unavailable => "unavailable",
            },
            "detail": gate.detail,
        })),
        "probe_target": match &report.target {
            ProbeTarget::Remote { owner, repo } => serde_json::json!({
                "kind": "remote", "owner": owner, "repo": repo,
            }),
            ProbeTarget::Gate { owner, repo } => serde_json::json!({
                "kind": "authorization-gate", "owner": owner, "repo": repo,
            }),
        },
        // A different gate from git's, reported so the two can be seen to differ.
        "relay_http_membership": match &report.http_membership {
            HttpMembership::Accepted { announcements } => serde_json::json!({
                "ok": true, "announcements": announcements,
            }),
            HttpMembership::Refused { detail } => serde_json::json!({
                "ok": false, "detail": detail,
            }),
        },
        "remedy": remedy(report.verdict(), &report.attestation, report.gate_accepted()),
        "repos_probed": report.repos.len(),
        "repos_readable": report.repos.iter().filter(|p| p.access == "read").count(),
        "repos": report.repos.iter().map(|p| serde_json::json!({
            "repo_id": p.repo_id,
            "owner": p.owner,
            "access": p.access,
            "http_status": p.status,
            "detail": p.detail,
        })).collect::<Vec<_>>(),
    })
}

/// The human form of a report.
pub fn render_human(report: &CheckReport) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let _ = writeln!(out, "relay   {}", report.relay);
    let _ = writeln!(
        out,
        "key     {}  (from {})",
        report.pubkey, report.key_source
    );
    match &report.attestation {
        AttestationState::Present { owner } => {
            let _ = writeln!(
                out,
                "owner   {owner}  (attestation present; this key acts for its owner's grant)"
            );
        }
        AttestationState::Absent => {
            let _ = writeln!(
                out,
                "owner   none  (no attestation; this key answers for itself)"
            );
        }
        AttestationState::NotForThisKey { problem } | AttestationState::Malformed { problem } => {
            let _ = writeln!(out, "owner   attestation invalid — {problem}");
        }
    }
    if let Some(disclosure) = &report.key_disclosure {
        let _ = writeln!(out, "note    {disclosure}");
    }

    match &report.target {
        ProbeTarget::Remote { owner, repo } => {
            let _ = writeln!(
                out,
                "repo    {}/{repo}  (a remote in this checkout)",
                short_hex(owner)
            );
        }
        ProbeTarget::Gate { .. } => {
            let _ = writeln!(
                out,
                "repo    none here — probing the authorization gate with a repository that cannot exist"
            );
        }
    }

    for probe in &report.transport {
        let word = match probe.verdict {
            TransportVerdict::Accepted => "accepted",
            TransportVerdict::Denied => "denied",
            TransportVerdict::Unavailable => "unavailable",
        };
        let _ = writeln!(
            out,
            "git     {word} — {} → HTTP {}",
            probe.service, probe.status
        );
        let _ = writeln!(out, "        {}", probe.detail);
    }
    if report.transport.is_empty() {
        let _ = writeln!(
            out,
            "git     denied — no request was made; the credential helper refuses to sign"
        );
    }
    if let Some(gate) = &report.gate {
        let _ = writeln!(
            out,
            "gate    {} — the same request against a repository that cannot exist → HTTP {}",
            match gate.verdict {
                TransportVerdict::Accepted => "accepted",
                TransportVerdict::Denied => "denied",
                TransportVerdict::Unavailable => "unavailable",
            },
            gate.status
        );
        let _ = writeln!(
            out,
            "        {}",
            match gate.verdict {
                TransportVerdict::Accepted =>
                    "so the key itself is admitted; the denial above is about this repository",
                _ => "so the key itself is refused, on this repository and every other",
            }
        );
    }

    // Secondary. Labelled, so a person can see the two gates differ.
    match &report.http_membership {
        HttpMembership::Accepted { announcements } => {
            let _ = writeln!(
                out,
                "relay HTTP membership: accepted ({announcements} repository announcements visible)"
            );
        }
        HttpMembership::Refused { detail } => {
            let _ = writeln!(out, "relay HTTP membership: refused — {detail}");
            let _ = writeln!(
                out,
                "        That is a different gate from git's. The git line above is the one"
            );
            let _ = writeln!(out, "        that governs clone and push.");
        }
    }

    if let Some(remedy) = remedy(
        report.verdict(),
        &report.attestation,
        report.gate_accepted(),
    ) {
        let _ = writeln!(out);
        let _ = writeln!(out, "{remedy}");
    }

    if let Some(prediction) = &report.prediction {
        let _ = writeln!(out);
        let _ = writeln!(out, "{PREDICTION_HEADING}");
        let _ = writeln!(
            out,
            "  {} @ {}",
            prediction.ref_name,
            short_hex(&prediction.sha)
        );
        let line = match &prediction.state {
            RefPredictionState::Ungoverned => {
                "  no require-verdict rule governs this ref; nothing here reads a mission verdict"
                    .to_string()
            }
            RefPredictionState::NoRepository => {
                "  no relay remote in this checkout, so no repository rules to read".to_string()
            }
            RefPredictionState::Unreadable { detail } => format!("  not predicted — {detail}"),
            RefPredictionState::AdmittedAsFounder => {
                "  admitted by arm (A) — you are a founder of this repository, and a founder's \
                 push needs no verdict"
                    .to_string()
            }
            RefPredictionState::AdmittedByVerdict {
                session_ref,
                disposition_event_id,
                refutation_event_id,
                verifier_pubkey,
            } => format!(
                "  admitted by arm (C) — mission {session_ref} approved it (disposition {}), and \
                 verifier {} did not refute it (refutation {})",
                short_hex(disposition_event_id),
                short_hex(verifier_pubkey),
                short_hex(refutation_event_id)
            ),
            RefPredictionState::AdmittedByObservedGates { session_ref, gates } => format!(
                "  admitted by arm (B) — mission {session_ref} observed {} green on this exact \
                 commit over a clean worktree, and requires no verifier",
                gates.join(", ")
            ),
            RefPredictionState::Refused { reason } => format!("  refused — {reason}"),
        };
        let _ = writeln!(out, "{line}");
        let _ = writeln!(out, "  {ENFORCEMENT_DISCLOSURE}");
        let _ = writeln!(out, "  {}", prediction.serving_relay);
        if !prediction.founders.is_empty() {
            let _ = writeln!(out, "  {}", prediction.founders);
        }
    }

    if !report.repos.is_empty() {
        let _ = writeln!(out);
        let _ = writeln!(out, "{:<34} {:<20} OWNER", "REPO", "ACCESS");
        for entry in &report.repos {
            let _ = writeln!(
                out,
                "{:<34} {:<20} {}",
                entry.repo_id,
                entry.access,
                &entry.owner[..16.min(entry.owner.len())]
            );
        }
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "{} of {} readable over git.",
            report.repos.iter().filter(|p| p.access == "read").count(),
            report.repos.len()
        );
        let _ = writeln!(
            out,
            "`no-grant-or-missing` is the relay's single answer for both — it does"
        );
        let _ = writeln!(out, "not distinguish them, so neither does this.");
    }
    out
}

/// Ask the relay what git can actually do with this key, print it, and exit
/// with the code that matches: 0 when the transport accepts, 3 when it denies.
pub async fn cmd_check(
    relay_url: &str,
    keyfile: Option<PathBuf>,
    push: bool,
    ref_name: Option<String>,
    sha: Option<String>,
    compact: bool,
) -> Result<(), CliError> {
    let report = run_check(&CheckRequest {
        relay_url: relay_url.to_string(),
        keyfile,
        push,
        ref_name,
        sha,
    })
    .await?;

    if compact {
        println!(
            "{}",
            serde_json::to_string(&render_json(&report)).unwrap_or_default()
        );
    } else {
        print!("{}", render_human(&report));
    }

    // The exit code is the transport's verdict and nothing else — it has to
    // match what git will do, or a script that trusts it is misled.
    match report.verdict() {
        TransportVerdict::Accepted => Ok(()),
        TransportVerdict::Denied if report.gate_accepted() => Err(CliError::Auth(format!(
            "the relay's git transport denied this repository to {} (the key itself is admitted)",
            short_hex(&report.pubkey)
        ))),
        TransportVerdict::Denied => Err(CliError::Auth(format!(
            "the relay's git transport denied this key ({})",
            short_hex(&report.pubkey)
        ))),
        TransportVerdict::Unavailable => Err(CliError::Relay {
            status: report
                .transport
                .iter()
                .find(|p| p.verdict == TransportVerdict::Unavailable)
                .map(|p| p.status)
                .unwrap_or(0),
            body: "the relay's git transport answered neither yes nor no".to_string(),
        }),
    }
}

/// Strip the git service suffix so the signed URL matches the helper's.
///
/// Delegates to [`git_credential_nostr::repo_root_url`], the same function the
/// helper uses on the URL git hands it.
fn repo_root_from_refs_url(url: &str) -> String {
    git_credential_nostr::repo_root_url(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine as _;

    /// The helper's own precedence, adapted to what `bee` prints — the same two
    /// steps `resolve_effective_key` takes, without touching the filesystem.
    fn choose_key_for_test(
        env: Option<Keys>,
        keyfile_path: &Path,
        keyfile_key: Option<Keys>,
    ) -> Option<EffectiveKey> {
        git_credential_nostr::choose_key(env, keyfile_path, keyfile_key).map(EffectiveKey::from)
    }

    /// Serialises the tests that read or write process environment.
    ///
    /// One lock for the sync and the async tests alike — two locks let a sync
    /// test overwrite `BUZZ_AUTH_TAG` while an async probe was mid-flight, which
    /// failed only in the full-suite run.
    fn env_lock() -> tokio::sync::MutexGuard<'static, ()> {
        async_env_lock().blocking_lock()
    }

    #[test]
    fn credential_scope_covers_only_the_git_path() {
        assert_eq!(
            credential_scope("https://hive.agiterra.org").unwrap(),
            "https://hive.agiterra.org/git"
        );
        // A trailing path on the relay URL must not leak into the scope.
        assert_eq!(
            credential_scope("https://hive.agiterra.org/").unwrap(),
            "https://hive.agiterra.org/git"
        );
    }

    #[test]
    fn credential_scope_accepts_the_websocket_forms() {
        assert_eq!(
            credential_scope("wss://hive.agiterra.org").unwrap(),
            "https://hive.agiterra.org/git"
        );
        assert_eq!(
            credential_scope("ws://localhost:3000").unwrap(),
            "http://localhost:3000/git"
        );
    }

    #[test]
    fn credential_scope_keeps_a_non_default_port() {
        assert_eq!(
            credential_scope("https://relay.example:8443").unwrap(),
            "https://relay.example:8443/git"
        );
    }

    #[test]
    fn credential_scope_rejects_junk() {
        assert!(credential_scope("").is_err());
        assert!(credential_scope("   ").is_err());
        assert!(credential_scope("file:///etc/passwd").is_err());
        assert!(credential_scope("not a url").is_err());
    }

    #[test]
    fn the_helper_entry_resets_the_scoped_list_before_adding_itself() {
        // Without the empty first value git APPENDS to the system/global
        // helper list, so osxkeychain also runs and its `store` failure prints
        // `fatal: failed to store: -1` over a request that actually succeeded.
        let entries = config_entries(
            "https://hive.agiterra.org/git",
            "nostr",
            Path::new("/home/a/.nostr/key"),
        );
        let (_, helper_values) = entries
            .iter()
            .find(|(key, _)| key.ends_with(".helper"))
            .expect("a helper entry");
        assert_eq!(helper_values, &vec![String::new(), "nostr".to_string()]);
    }

    #[test]
    fn config_entries_never_touch_the_global_helper() {
        let entries = config_entries(
            "https://hive.agiterra.org/git",
            "nostr",
            Path::new("/home/a/.nostr/key"),
        );
        let keys: Vec<&str> = entries.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(
            keys,
            vec![
                "credential.https://hive.agiterra.org/git.helper",
                "credential.https://hive.agiterra.org/git.useHttpPath",
                "nostr.keyfile",
            ]
        );
        // The unscoped key would answer for github.com too.
        assert!(!keys.contains(&"credential.helper"));
        assert!(!keys.contains(&"credential.useHttpPath"));
    }

    #[test]
    fn resolve_helper_rejects_a_missing_explicit_path() {
        let error = resolve_helper(Some(Path::new("/nonexistent/git-credential-nostr")))
            .expect_err("a missing helper must not be accepted");
        assert!(error.to_string().contains("is not a file"));
    }

    #[test]
    fn probe_signs_the_repo_root_like_the_credential_helper_does() {
        // The helper strips the service suffix and signs the repo root, because
        // git reuses one Authorization header across the GET and the POST. A
        // probe that signed the exact path would be rejected where real git
        // succeeds. See docs/git-nip98-method-binding.md.
        let root = "https://hive.agiterra.org/git/abc/repo";
        for url in [
            format!("{root}/info/refs?service=git-upload-pack"),
            format!("{root}/info/refs?service=git-receive-pack"),
            format!("{root}/git-upload-pack"),
            format!("{root}/git-receive-pack"),
        ] {
            assert_eq!(repo_root_from_refs_url(&url), root, "for {url}");
        }
    }

    #[test]
    fn a_denied_read_is_never_reported_as_a_missing_repo() {
        // The relay answers both with 404 on purpose, so that membership cannot
        // be probed by a stranger. Reporting either one alone would be a guess.
        let (access, detail) = classify_probe(404, "repository not found");
        assert_eq!(access, "no-grant-or-missing");
        assert!(detail.unwrap().contains("will not distinguish"));
    }

    #[test]
    fn probe_classification_separates_membership_from_access() {
        assert_eq!(classify_probe(200, "").0, "read");
        assert_eq!(
            classify_probe(403, "restricted: not a relay member").0,
            "denied"
        );
        assert_eq!(
            classify_probe(401, "missing Authorization header").0,
            "auth-rejected"
        );
        assert_eq!(classify_probe(500, "boom").0, "unexpected");
    }

    // ── which key git will actually present ──────────────────────────────

    #[test]
    fn the_env_key_wins_and_says_so() {
        // The ACP harness injects `NOSTR_PRIVATE_KEY` into a seat, and the
        // helper prefers it (`git-credential-nostr` lib.rs `choose_key`). A
        // report that named the key *file* would name the operator's identity
        // in a shell where git signs as the seat.
        let env = Keys::generate();
        let resolved = choose_key_for_test(Some(env.clone()), Path::new("/tmp/key"), None)
            .expect("an env key is a key");
        assert_eq!(resolved.keys.public_key(), env.public_key());
        assert_eq!(resolved.origin, KeyOrigin::Env);
        assert_eq!(resolved.origin.label(), "NOSTR_PRIVATE_KEY");
        assert!(resolved.disclosure().is_none(), "nothing is being shadowed");
    }

    #[test]
    fn the_key_file_is_named_by_path_when_it_is_what_git_uses() {
        let file = Keys::generate();
        let resolved =
            choose_key_for_test(None, Path::new("/home/a/.nostr/key"), Some(file.clone()))
                .expect("a key file is a key");
        assert_eq!(resolved.keys.public_key(), file.public_key());
        assert_eq!(resolved.origin.label(), "/home/a/.nostr/key");
        assert!(resolved.disclosure().is_none());
    }

    #[test]
    fn two_different_identities_in_one_shell_are_disclosed_in_one_sentence() {
        // The live finding: a seat signs git as itself while the operator's
        // key file sits in the same shell, and every local report named the
        // file. Both keys are real; only one is used, and the difference has
        // to be said out loud.
        let env = Keys::generate();
        let file = Keys::generate();
        let resolved = choose_key_for_test(
            Some(env.clone()),
            Path::new("/home/a/.nostr/key"),
            Some(file.clone()),
        )
        .expect("resolved");
        assert_eq!(resolved.keys.public_key(), env.public_key());

        let disclosure = resolved
            .disclosure()
            .expect("two identities must be disclosed");
        assert_eq!(disclosure.lines().count(), 1, "one sentence, one line");
        assert!(
            disclosure.contains(&env.public_key().to_hex()[..8]),
            "must name the key git signs with: {disclosure}"
        );
        assert!(
            disclosure.contains("NOSTR_PRIVATE_KEY"),
            "must name where it came from: {disclosure}"
        );
        assert!(
            disclosure.contains(&file.public_key().to_hex()[..8])
                && disclosure.contains("/home/a/.nostr/key"),
            "must name the identity that is NOT used: {disclosure}"
        );
        assert!(
            !disclosure.contains(&env.public_key().to_hex())
                && !disclosure.contains(&file.public_key().to_hex()),
            "a short prefix is enough; do not print whole identities: {disclosure}"
        );
    }

    #[test]
    fn the_same_identity_in_both_places_is_not_a_conflict() {
        let same = Keys::generate();
        let resolved = choose_key_for_test(
            Some(same.clone()),
            Path::new("/home/a/.nostr/key"),
            Some(same),
        )
        .expect("resolved");
        assert!(
            resolved.disclosure().is_none(),
            "one identity written twice is not two identities"
        );
    }

    #[test]
    fn no_key_anywhere_resolves_to_nothing() {
        assert!(choose_key_for_test(None, Path::new("/home/a/.nostr/key"), None).is_none());
    }

    #[test]
    fn the_probe_signs_the_owner_attestation_into_the_event() {
        // Git's credential protocol cannot add a header, so a managed seat's
        // attestation rides inside the signed NIP-98 event. A probe that
        // omitted it would ask the relay a different question than git asks.
        use nostr::JsonUtil as _;

        let seat = Keys::generate();
        let owner = Keys::generate();
        let tag = nostr::Tag::parse([
            "auth".to_string(),
            owner.public_key().to_hex(),
            String::new(),
            "00".repeat(64),
        ])
        .unwrap();

        let header = sign_git_nip98(
            &seat,
            "GET",
            "https://relay.example/git/abc/repo",
            Some(tag.clone()),
        )
        .expect("sign");
        let encoded = header.strip_prefix("Nostr ").expect("Nostr scheme");
        let json = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .expect("base64");
        let event = nostr::Event::from_json(&json).expect("event");

        assert!(
            event.verify().is_ok(),
            "the tag must be covered by the signature"
        );
        assert_eq!(event.pubkey, seat.public_key(), "a seat signs as itself");
        assert!(
            event.tags.iter().any(|t| t.as_slice() == tag.as_slice()),
            "the attestation must be in the signed event"
        );
        assert_eq!(
            tag.as_slice().get(1).map(String::as_str),
            Some(owner.public_key().to_hex().as_str())
        );
    }

    #[test]
    fn an_attestation_for_another_key_is_reported_not_claimed() {
        // `BUZZ_AUTH_TAG` is attested to `BUZZ_PRIVATE_KEY`, which need not be
        // the key git signs with. Reporting an owner from a tag that does not
        // cover the git key would claim a relationship the relay is about to
        // reject. Serialised because it reads process environment.
        let _guard = env_lock();
        let git_key = Keys::generate();
        let other_key = Keys::generate();
        let owner = Keys::generate();

        let good = buzz_sdk::nip_oa::compute_auth_tag(&owner, &git_key.public_key(), "")
            .expect("auth tag");
        // SAFETY-EQUIVALENT: single-threaded section guarded by `env_lock`.
        std::env::set_var("BUZZ_AUTH_TAG", &good);
        let resolved = probe_attestation(&git_key).expect("verified tag");
        assert_eq!(
            resolved.owner.as_deref(),
            Some(owner.public_key().to_hex().as_str())
        );
        assert!(resolved.warning.is_none());
        assert!(resolved.tag.is_some(), "the tag still rides on the request");

        let mismatched = buzz_sdk::nip_oa::compute_auth_tag(&owner, &other_key.public_key(), "")
            .expect("auth tag");
        std::env::set_var("BUZZ_AUTH_TAG", &mismatched);
        let resolved = probe_attestation(&git_key).expect("a mismatch is a report, not an error");
        assert!(
            resolved.owner.is_none(),
            "an attestation that does not cover the git key names no owner"
        );
        let warning = resolved.warning.expect("the mismatch must be disclosed");
        assert!(
            warning.contains(&git_key.public_key().to_hex()[..8]),
            "must name the key git uses: {warning}"
        );
        assert!(
            resolved.tag.is_some(),
            "the helper sends it regardless, so the probe must too"
        );

        std::env::set_var("BUZZ_AUTH_TAG", "not-json");
        assert!(
            probe_attestation(&git_key).is_err(),
            "a malformed attestation fails closed, exactly as the helper does"
        );

        std::env::remove_var("BUZZ_AUTH_TAG");
    }

    // ── the check answers the question it is presented as answering ──────

    /// A stub relay: `info/refs` answers `refs_status`, `POST /query` answers
    /// `query_status` with `query_body`. Returns the base URL.
    async fn stub_relay(
        refs_status: u16,
        refs_body: &'static str,
        query_status: u16,
        query_body: &'static str,
    ) -> String {
        use axum::body::Body;
        use axum::http::Response;
        use axum::Router;

        let app = Router::new()
            .route(
                "/query",
                axum::routing::post(move || async move {
                    Response::builder()
                        .status(query_status)
                        .header("content-type", "application/json")
                        .body(Body::from(query_body))
                        .expect("response")
                }),
            )
            .route(
                "/{*path}",
                axum::routing::get(move || async move {
                    Response::builder()
                        .status(refs_status)
                        .body(Body::from(refs_body))
                        .expect("response")
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        format!("http://{addr}")
    }

    /// Serialises the async tests that set process environment. `tokio`'s mutex,
    /// not `std`'s: the environment has to stay set across the probe's `await`.
    fn async_env_lock() -> &'static tokio::sync::Mutex<()> {
        static LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
        LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
    }

    /// Set the seat's key and attestation for the duration of one test, and
    /// hand back the report a check produces against `relay`.
    async fn check_against(relay: &str, keys: &Keys, tag: Option<&str>, push: bool) -> CheckReport {
        // SAFETY-EQUIVALENT: single-threaded section guarded by `env_lock`.
        std::env::set_var("NOSTR_PRIVATE_KEY", keys.secret_key().to_secret_hex());
        match tag {
            Some(tag) => std::env::set_var("BUZZ_AUTH_TAG", tag),
            None => std::env::remove_var("BUZZ_AUTH_TAG"),
        }
        let report = run_check(&CheckRequest {
            relay_url: relay.to_string(),
            keyfile: None,
            push,
            ref_name: None,
            sha: None,
        })
        .await;
        std::env::remove_var("NOSTR_PRIVATE_KEY");
        std::env::remove_var("BUZZ_AUTH_TAG");
        report.expect("a probe against the stub relay")
    }

    #[test]
    fn the_verdict_is_what_git_will_do_with_the_same_request() {
        // 200: the relay served the ref advertisement.
        assert_eq!(
            transport_verdict(200, true).0,
            TransportVerdict::Accepted,
            "a served ref advertisement is a clone that works"
        );
        // 404 on a repo that cannot exist: the request got PAST the membership
        // gate and died at repository resolution. That is the gate answering.
        assert_eq!(transport_verdict(404, false).0, TransportVerdict::Accepted);
        // 404 on a real repo: the relay answers 404 for "no grant" and for "no
        // such repo" alike, so acceptance cannot be read out of it.
        let (verdict, detail) = transport_verdict(404, true);
        assert_eq!(verdict, TransportVerdict::Denied);
        assert!(
            detail.contains("will not distinguish"),
            "the ambiguity has to be stated, not resolved by guessing: {detail}"
        );
        assert_eq!(transport_verdict(403, false).0, TransportVerdict::Denied);
        assert_eq!(transport_verdict(401, true).0, TransportVerdict::Denied);
        // A 5xx is neither. Calling it "denied" would invent a decision.
        assert_eq!(
            transport_verdict(503, true).0,
            TransportVerdict::Unavailable
        );
    }

    /// `--push` probes `git-receive-pack`'s advertisement — the READ gate. A
    /// bare "Accepted" there read as "your push will work", which was never
    /// what the probe tested and, since the verdict gate, is not even the last
    /// word on whether the relay takes the commit.
    #[test]
    fn the_push_probe_says_it_is_the_read_gate_only() {
        let (verdict, detail) = transport_verdict_for("git-receive-pack", 200, true);
        assert_eq!(verdict, TransportVerdict::Accepted);
        assert_eq!(
            detail,
            "the relay advertised refs to a push client; this is the read gate only — the \
             pre-receive hook decides the push itself"
        );
        // The fetch probe's sentence is unchanged.
        assert_eq!(
            transport_verdict_for("git-upload-pack", 200, true).1,
            "the relay served the ref advertisement"
        );
        // Every other status keeps its existing reading on both services.
        for status in [401, 403, 404, 503] {
            assert_eq!(
                transport_verdict_for("git-receive-pack", status, true),
                transport_verdict(status, true),
                "only a 200 push advertisement changes wording"
            );
        }
    }

    fn report_with(prediction: RefPrediction) -> CheckReport {
        CheckReport {
            relay: "https://relay.example".to_string(),
            pubkey: "ab".repeat(32),
            key_source: "NOSTR_PRIVATE_KEY".to_string(),
            key_disclosure: None,
            attestation: AttestationState::Absent,
            target: ProbeTarget::Remote {
                owner: "ab".repeat(32),
                repo: "beekeeper".to_string(),
            },
            transport: Vec::new(),
            gate: None,
            http_membership: HttpMembership::Accepted { announcements: 0 },
            repos: Vec::new(),
            prediction: Some(prediction),
        }
    }

    /// The prediction is printed as a prediction, and the refusal it prints is
    /// the relay's own sentence — produced by the same
    /// `buzz_core::coding_session_verdict_admission` rule, not a second copy of
    /// the words.
    #[test]
    fn a_prediction_prints_the_relays_own_sentence_under_a_prediction_heading() {
        let sha = "07c470be007c470be007c470be007c470be007c4";
        let expected = evaluate_verdict_admission(
            &[],
            &VerdictAdmissionQuery {
                ref_name: "refs/heads/main",
                new_oid: sha,
                pusher_pubkey: &"cd".repeat(32),
                repo_founders: &["ab".repeat(32)],
            },
        );
        let VerdictAdmission::Refused(refusal) = expected else {
            panic!("no candidates admits nothing");
        };
        let rendered = render_human(&report_with(RefPrediction {
            ref_name: "refs/heads/main".to_string(),
            sha: sha.to_string(),
            state: RefPredictionState::Refused {
                reason: refusal.reason(),
            },
            serving_relay: "serving relay reports buzz-relay 0.2.1".to_string(),
            founders: String::new(),
        }));
        assert!(
            rendered.contains(PREDICTION_HEADING),
            "the heading says it is not a promise:\n{rendered}"
        );
        assert!(
            rendered.contains(&refusal.reason()),
            "the CLI prints the relay's own refusal verbatim:\n{rendered}"
        );
        assert!(
            !rendered.contains("will be accepted") && !rendered.contains("your push will"),
            "nothing here may promise an outcome:\n{rendered}"
        );
    }

    /// The prediction never claims the serving relay agrees with it.
    ///
    /// `unknown_rules` and the prediction are both computed by the LOCAL
    /// build, which knows `require-verdict`; a relay predating the rule parses
    /// the token and ignores it, and nothing a person saw used to say so.
    /// Neither command can fix that — both can stop hiding it (fix round 1,
    /// F5).
    #[test]
    fn a_prediction_says_whose_build_evaluated_it() {
        let rendered = render_human(&report_with(RefPrediction {
            ref_name: "refs/heads/main".to_string(),
            sha: "2".repeat(40),
            state: RefPredictionState::Ungoverned,
            serving_relay: "serving relay's version unknown".to_string(),
            founders: String::new(),
        }));
        assert!(
            rendered.contains("the rule as THIS build evaluates it"),
            "the answer names whose rule table produced it:\n{rendered}"
        );
        assert!(
            rendered.contains("a relay predating require-verdict parses the token and ignores it"),
            "and says what that means when they disagree:\n{rendered}"
        );
        assert!(
            rendered.contains("serving relay's version unknown"),
            "an unanswering relay is disclosed as unknown, never as agreement:\n{rendered}"
        );
    }

    /// An ungoverned ref says exactly that, rather than implying a verdict was
    /// searched for and missing.
    #[test]
    fn an_ungoverned_ref_says_no_rule_governs_it() {
        let rendered = render_human(&report_with(RefPrediction {
            ref_name: "refs/heads/topic".to_string(),
            sha: "2".repeat(40),
            state: RefPredictionState::Ungoverned,
            serving_relay: "serving relay reports buzz-relay 0.2.1".to_string(),
            founders: String::new(),
        }));
        assert!(
            rendered.contains("no require-verdict rule governs this ref"),
            "{rendered}"
        );
        assert!(
            !rendered.contains("no approved report names"),
            "an ungoverned ref must not borrow the refusal copy:\n{rendered}"
        );
    }

    /// A read the prediction needed and did not get is disclosed as unknown —
    /// never rendered as "ungoverned", which is a claim about the repository.
    #[test]
    fn an_unreadable_prediction_is_unknown_not_permissive() {
        let rendered = render_human(&report_with(RefPrediction {
            ref_name: "refs/heads/main".to_string(),
            sha: "2".repeat(40),
            state: RefPredictionState::Unreadable {
                detail: "the relay refused the query".to_string(),
            },
            serving_relay: "serving relay reports buzz-relay 0.2.1".to_string(),
            founders: String::new(),
        }));
        assert!(
            rendered.contains("not predicted — the relay refused the query"),
            "{rendered}"
        );
    }

    #[test]
    fn no_remedy_ever_advises_dropping_the_owner_attestation() {
        // The remedy this replaces said "BUZZ_AUTH_TAG is set — it may be stale
        // or revoked; try unsetting it" over a key whose `git push` worked. A
        // seat that followed it would lose the access it had.
        let states = [
            AttestationState::Absent,
            AttestationState::Present {
                owner: "ab".repeat(32),
            },
            AttestationState::NotForThisKey {
                problem: "signed for another key".to_string(),
            },
            AttestationState::Malformed {
                problem: "not JSON".to_string(),
            },
        ];
        for state in &states {
            for verdict in [TransportVerdict::Denied, TransportVerdict::Unavailable] {
                let text = remedy(verdict, state, false).expect("a denial explains itself");
                assert!(
                    !text.contains("unset"),
                    "never advise unsetting the attestation: {text}"
                );
                assert!(
                    !text.contains("stale or revoked"),
                    "never guess at the reason: {text}"
                );
            }
        }
        assert!(
            remedy(TransportVerdict::Accepted, &states[1], false).is_none(),
            "an accepted transport has nothing to remedy"
        );
        let attested = remedy(
            TransportVerdict::Denied,
            &AttestationState::Present {
                owner: "ab".repeat(32),
            },
            false,
        )
        .expect("remedy");
        // A gate that admitted the key must not be told to go fix membership.
        let repo_only =
            remedy(TransportVerdict::Denied, &AttestationState::Absent, true).expect("remedy");
        assert!(
            repo_only.contains("grant on this repository") && !repo_only.contains("unset"),
            "a repository-level denial is not a membership problem: {repo_only}"
        );
        assert!(
            attested.contains("owner") && attested.contains("member of this relay"),
            "an attested seat is told whose membership to check: {attested}"
        );
        assert!(
            attested.contains("Keep the attestation"),
            "and told to keep the thing its push depends on: {attested}"
        );
    }

    #[test]
    fn the_clients_generic_403_hint_never_rides_along() {
        let raw = "relay error 403: relay_membership_required (BUZZ_AUTH_TAG is set \u{2014} it may be stale or revoked; try unsetting it)";
        let stripped = strip_auth_tag_hint(raw);
        assert_eq!(stripped, "relay error 403: relay_membership_required");
        assert!(!stripped.contains("unsetting"));
        // Anything without the hint passes through untouched.
        assert_eq!(
            strip_auth_tag_hint("relay error 500: boom"),
            "relay error 500: boom"
        );
    }

    #[test]
    fn a_relay_remote_in_this_checkout_is_the_repository_probed() {
        let owner = "ab".repeat(32);
        let remotes = format!(
            "github\thttps://github.com/block/buzz.git (fetch)\n\
             origin\thttps://hive.agiterra.org/git/{owner}/beekeeper.git (fetch)\n\
             origin\thttps://hive.agiterra.org/git/{owner}/beekeeper.git (push)\n"
        );
        assert_eq!(
            parse_remote_target("https://hive.agiterra.org", &remotes),
            Some((owner.clone(), "beekeeper".to_string())),
            "the repository git would contact is the one worth probing"
        );
        // A checkout with no relay remote leaves the gate probe as the target.
        assert_eq!(
            parse_remote_target(
                "https://hive.agiterra.org",
                "origin\tgit@github.com:block/buzz.git (fetch)\n"
            ),
            None
        );
    }

    /// A real repository answering 404 cannot say whether the key or the repo
    /// was the problem, so the gate is asked separately and both are reported.
    #[tokio::test]
    async fn a_404_on_a_real_repository_still_says_whether_the_key_got_through() {
        let _guard = async_env_lock().lock().await;
        let seat = Keys::generate();
        let relay = stub_relay(404, "repository not found", 200, "[]").await;
        let owner = "ab".repeat(32);
        let target = ProbeTarget::Remote {
            owner: owner.clone(),
            repo: "beekeeper".to_string(),
        };
        assert!(target.is_real_repo());

        // No remote here points at the stub, so drive the mapping directly and
        // then prove the gate follow-up through the rendered report.
        let report = CheckReport {
            relay: relay.clone(),
            pubkey: seat.public_key().to_hex(),
            key_source: "NOSTR_PRIVATE_KEY".to_string(),
            key_disclosure: None,
            attestation: AttestationState::Absent,
            target,
            transport: vec![TransportProbe {
                service: "git-upload-pack",
                url: format!("{relay}/git/{owner}/beekeeper/info/refs?service=git-upload-pack"),
                status: 404,
                verdict: TransportVerdict::Denied,
                detail: transport_verdict(404, true).1,
            }],
            gate: Some(TransportProbe {
                service: "git-upload-pack",
                url: format!("{relay}/git/{owner}/{GATE_PROBE_REPO}/info/refs"),
                status: 404,
                verdict: TransportVerdict::Accepted,
                detail: transport_verdict(404, false).1,
            }),
            http_membership: HttpMembership::Accepted { announcements: 0 },
            repos: Vec::new(),
            prediction: None,
        };

        assert_eq!(
            report.verdict(),
            TransportVerdict::Denied,
            "git still fails on this repository, so the exit code must too"
        );
        let rendered = render_human(&report);
        assert!(
            rendered.contains("gate    accepted"),
            "the key's own standing must not be lost in the repo's 404: {rendered}"
        );
        assert!(
            rendered.contains("the denial above is about this repository"),
            "{rendered}"
        );
        assert_eq!(render_json(&report)["gate_probe"]["verdict"], "accepted");
    }

    /// 200 on `info/refs` is git working. The check must say so, exit 0, and
    /// disclose which key and which attestation produced that answer.
    #[tokio::test]
    async fn an_accepted_transport_is_reported_accepted_with_its_disclosures() {
        let _guard = async_env_lock().lock().await;
        let seat = Keys::generate();
        let owner = Keys::generate();
        let tag =
            buzz_sdk::nip_oa::compute_auth_tag(&owner, &seat.public_key(), "").expect("auth tag");
        let relay = stub_relay(200, "001e# service=git-upload-pack", 200, "[]").await;

        let report = check_against(&relay, &seat, Some(&tag), true).await;

        assert_eq!(report.verdict(), TransportVerdict::Accepted);
        assert_eq!(
            report.transport.len(),
            2,
            "--push probes git-receive-pack as well as git-upload-pack"
        );
        assert_eq!(report.transport[0].service, "git-upload-pack");
        assert_eq!(report.transport[1].service, "git-receive-pack");
        assert_eq!(
            report.attestation,
            AttestationState::Present {
                owner: owner.public_key().to_hex()
            }
        );

        let rendered = render_human(&report);
        assert!(
            rendered.contains(&seat.public_key().to_hex()),
            "the key it used must be named: {rendered}"
        );
        assert!(
            rendered.contains("NOSTR_PRIVATE_KEY"),
            "and where that key came from: {rendered}"
        );
        assert!(
            rendered.contains(&owner.public_key().to_hex()),
            "and the owner the attestation covers: {rendered}"
        );
        assert!(
            rendered.contains("git     accepted"),
            "the transport verdict is the headline: {rendered}"
        );
        assert!(
            !rendered.contains("unsetting"),
            "no advice to drop the attestation on a success either: {rendered}"
        );
        let json = render_json(&report);
        assert_eq!(json["git_transport"], "accepted");
        assert_eq!(json["attestation"], "present");
        assert!(json["remedy"].is_null());
    }

    /// 401 is the relay refusing the credential outright. Git fails; so does
    /// the check, with exit 3 and no invented reason.
    #[tokio::test]
    async fn a_denied_transport_is_reported_denied_and_exits_three() {
        let _guard = async_env_lock().lock().await;
        let seat = Keys::generate();
        let relay = stub_relay(401, "missing Authorization header", 200, "[]").await;

        // SAFETY-EQUIVALENT: single-threaded section guarded by `env_lock`.
        std::env::set_var("NOSTR_PRIVATE_KEY", seat.secret_key().to_secret_hex());
        std::env::remove_var("BUZZ_AUTH_TAG");
        let result = cmd_check(&relay, None, false, None, None, true).await;
        std::env::remove_var("NOSTR_PRIVATE_KEY");

        let error = result.expect_err("a refused credential is not a success");
        assert_eq!(
            crate::error::exit_code(&error),
            3,
            "the exit code has to match what git will do: {error}"
        );
        assert!(
            !error.to_string().contains("unsetting"),
            "no dangerous remedy on the error line either: {error}"
        );
    }

    /// 403 on the gate probe: the relay refuses this key on the git transport.
    /// With an attestation present, the remedy names the owner to check.
    #[tokio::test]
    async fn a_gate_denial_names_the_owner_to_ask_about() {
        let _guard = async_env_lock().lock().await;
        let seat = Keys::generate();
        let owner = Keys::generate();
        let tag =
            buzz_sdk::nip_oa::compute_auth_tag(&owner, &seat.public_key(), "").expect("auth tag");
        let relay = stub_relay(403, "relay_membership_required", 200, "[]").await;

        let report = check_against(&relay, &seat, Some(&tag), false).await;

        assert_eq!(report.verdict(), TransportVerdict::Denied);
        let rendered = render_human(&report);
        assert!(rendered.contains("git     denied"), "{rendered}");
        assert!(
            rendered.contains(&owner.public_key().to_hex()[..8]),
            "the owner whose membership to confirm must be named: {rendered}"
        );
        assert!(!rendered.contains("unsetting"), "{rendered}");
    }

    /// The live failure (ledger draft 92, 2026-08-29 18:51): `bee git check`
    /// exited 3 over `relay_membership_required` while `git push` from the same
    /// key succeeded seconds later. The git transport is the gate that governs
    /// push and clone; the relay's HTTP membership path is a different gate, and
    /// letting it decide made the check answer a different question than the one
    /// it is presented as answering.
    #[tokio::test]
    async fn the_git_transport_governs_not_the_relay_http_membership_path() {
        let _guard = async_env_lock().lock().await;
        let seat = Keys::generate();
        let owner = Keys::generate();
        let tag =
            buzz_sdk::nip_oa::compute_auth_tag(&owner, &seat.public_key(), "").expect("auth tag");
        // SAFETY-EQUIVALENT: single-threaded section guarded by `env_lock`.
        std::env::set_var("NOSTR_PRIVATE_KEY", seat.secret_key().to_secret_hex());
        std::env::set_var("BUZZ_AUTH_TAG", &tag);

        // 404 from the git transport on a repo that cannot exist = the
        // authorization gate let this key through. 403 from POST /query = the
        // relay's HTTP membership path refuses it.
        let relay = stub_relay(
            404,
            "repository not found",
            403,
            r#"{"error":"relay_membership_required"}"#,
        )
        .await;

        let result = cmd_check(&relay, None, false, None, None, true).await;

        std::env::remove_var("NOSTR_PRIVATE_KEY");
        std::env::remove_var("BUZZ_AUTH_TAG");

        let error = result.as_ref().err().map(ToString::to_string);
        assert!(
            result.is_ok(),
            "the git transport accepted this key, so the check must too: {error:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn an_npub_in_the_key_file_is_named_as_the_mistake_it_is() {
        // Keys::parse accepts any 64 hex chars as a secret, so pasting a public
        // key yields a valid-looking, completely wrong identity that every
        // local check reports as fine. The bech32 form is the one case that can
        // be caught locally — so it is.
        let dir = std::env::temp_dir().join(format!("bee-npub-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("key");
        std::fs::write(
            &path,
            "npub1zxrgz5aeerdm7ku2x4j7j0gkj47a88nd9kvdfs8r26z0hwq493ussvhnev\n",
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();

        let error = read_keyfile(&path).expect_err("an npub is not a secret key");
        assert!(error.to_string().contains("public"), "got: {error}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn write_keyfile_creates_at_0600_and_is_idempotent() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("bee-git-setup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("nested").join("key");
        let keys = Keys::generate();

        assert!(write_keyfile(&path, &keys).unwrap(), "first write creates");
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "key file must not be readable by anyone else");

        assert!(
            !write_keyfile(&path, &keys).unwrap(),
            "re-running with the same identity must be a no-op, not an error"
        );

        let other = Keys::generate();
        let error = write_keyfile(&path, &other)
            .expect_err("a different identity must never be silently overwritten");
        assert!(error.to_string().contains("different identity"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn read_keyfile_refuses_a_group_readable_key() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("bee-git-setup-mode-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("key");
        let keys = Keys::generate();
        write_keyfile(&path, &keys).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();

        let error = read_keyfile(&path).expect_err("the helper would reject this file too");
        assert!(error.to_string().contains("requires 0600"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
