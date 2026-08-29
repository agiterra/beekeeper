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

/// Read a key file, enforcing the same 0600 rule the helper enforces.
///
/// A file the helper will refuse is worse than no file: setup would report
/// success and every push would still fail, with the reason surfacing only in
/// git's stderr.
fn read_keyfile(path: &Path) -> Result<Option<Keys>, CliError> {
    if !path.exists() {
        return Ok(None);
    }
    let metadata = std::fs::metadata(path)
        .map_err(|e| CliError::Usage(format!("cannot stat {}: {e}", path.display())))?;
    if !metadata.is_file() {
        return Err(CliError::Usage(format!(
            "{} exists but is not a regular file",
            path.display()
        )));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = metadata.permissions().mode();
        if mode & 0o177 != 0 {
            return Err(CliError::Usage(format!(
                "{} is mode {:o}; git-credential-nostr requires 0600. Run: chmod 600 {}",
                path.display(),
                mode & 0o777,
                path.display()
            )));
        }
    }
    let raw = std::fs::read_to_string(path)
        .map_err(|e| CliError::Usage(format!("cannot read {}: {e}", path.display())))?;
    let trimmed = raw.trim();
    // A pasted `npub1...` is a public key where a secret was wanted. It is the
    // one shape of that mistake which is decidable, so it gets a real message
    // instead of a parse error. The 64-hex form of the same mistake is NOT
    // decidable — every 32-byte value is a plausible secret key — which is why
    // `bee git check` exists to ask the relay instead of guessing.
    if trimmed.starts_with("npub1") {
        return Err(CliError::Key(format!(
            "{} holds an npub, which is a *public* key. The key file needs the \
             matching nsec.",
            path.display()
        )));
    }
    let keys = Keys::parse(trimmed).map_err(|e| {
        CliError::Key(format!(
            "{} does not hold a usable key: {e}",
            path.display()
        ))
    })?;
    Ok(Some(keys))
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

/// Decide, from what each source holds, which key git will present.
///
/// Split out from the IO so the precedence itself is testable without touching
/// process environment or the filesystem — the precedence is the part that has
/// been wrong.
fn choose_effective_key(
    env_key: Option<Keys>,
    keyfile_path: &Path,
    keyfile_key: Option<Keys>,
) -> Option<EffectiveKey> {
    match (env_key, keyfile_key) {
        (Some(env), file) => {
            let shadowed = file
                .filter(|file| file.public_key() != env.public_key())
                .map(|file| (keyfile_path.to_path_buf(), file.public_key()));
            Some(EffectiveKey {
                keys: env,
                origin: KeyOrigin::Env,
                shadowed,
            })
        }
        (None, Some(file)) => Some(EffectiveKey {
            keys: file,
            origin: KeyOrigin::Keyfile(keyfile_path.to_path_buf()),
            shadowed: None,
        }),
        (None, None) => None,
    }
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

/// Parse `$NOSTR_PRIVATE_KEY`, or `None` when it is unset or empty.
fn env_key() -> Result<Option<Keys>, CliError> {
    let Ok(raw) = std::env::var("NOSTR_PRIVATE_KEY") else {
        return Ok(None);
    };
    if raw.trim().is_empty() {
        return Ok(None);
    }
    Keys::parse(raw.trim())
        .map(Some)
        .map_err(|e| CliError::Key(format!("NOSTR_PRIVATE_KEY is not a key: {e}")))
}

/// Resolve the key git will present, exactly as `git-credential-nostr` does:
/// `$NOSTR_PRIVATE_KEY` first, then `git config nostr.keyfile`.
///
/// Reproducing the helper's precedence is the whole point. A check that read
/// `BUZZ_PRIVATE_KEY` instead would test a different identity than the one git
/// actually presents, and could report success while every push failed.
pub fn resolve_effective_key(keyfile: Option<&Path>) -> Result<EffectiveKey, CliError> {
    let path = effective_keyfile_path(keyfile)?;
    let env = env_key()?;
    // A broken key file must not mask a working env key: the helper would never
    // read it, so neither does this.
    let file = match read_keyfile(&path) {
        Ok(found) => found,
        // The helper never reads the file when the env key is set, so a broken
        // file must not fail a resolution that will succeed. `bee git status`
        // still reports the file's problem in its own field.
        Err(_) if env.is_some() => None,
        Err(error) => return Err(error),
    };
    choose_effective_key(env, &path, file).ok_or_else(|| {
        CliError::Usage(format!(
            "no key: {} does not exist and NOSTR_PRIVATE_KEY is unset",
            path.display()
        ))
    })
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
struct RepoProbe {
    repo_id: String,
    owner: String,
    status: u16,
    access: &'static str,
    detail: Option<String>,
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
    let raw = std::env::var("BUZZ_AUTH_TAG")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| git_config_get("nostr.authtag"));
    let Some(raw) = raw else {
        return Ok(ProbeAttestation::default());
    };
    let parts: Vec<String> = serde_json::from_str(&raw)
        .map_err(|e| CliError::Auth(format!("BUZZ_AUTH_TAG is not valid JSON: {e}")))?;
    if parts.len() != 4 || parts.first().map(String::as_str) != Some("auth") {
        return Err(CliError::Auth(
            "BUZZ_AUTH_TAG must be [auth, owner, conditions, signature]".to_string(),
        ));
    }
    let tag = nostr::Tag::parse(parts)
        .map_err(|e| CliError::Auth(format!("BUZZ_AUTH_TAG is not a usable tag: {e}")))?;

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
/// Deliberately not `client::sign_nip98`: that one cannot carry the NIP-OA
/// tag, and the whole value of `bee git check` is that it asks the relay the
/// same question git asks.
fn sign_git_nip98(
    keys: &Keys,
    method: &str,
    url: &str,
    auth_tag: Option<nostr::Tag>,
) -> Result<String, CliError> {
    use base64::Engine as _;
    use nostr::JsonUtil as _;

    let mut tags = vec![
        nostr::Tag::parse(["u", url]).map_err(|e| CliError::Other(format!("tag error: {e}")))?,
        nostr::Tag::parse(["method", method])
            .map_err(|e| CliError::Other(format!("tag error: {e}")))?,
        nostr::Tag::parse(["nonce", &uuid::Uuid::new_v4().to_string()])
            .map_err(|e| CliError::Other(format!("tag error: {e}")))?,
    ];
    tags.extend(auth_tag);
    let event = nostr::EventBuilder::new(nostr::Kind::Custom(27235), "")
        .tags(tags)
        .sign_with_keys(keys)
        .map_err(|e| CliError::Other(format!("NIP-98 signing failed: {e}")))?;
    Ok(format!(
        "Nostr {}",
        base64::engine::general_purpose::STANDARD.encode(event.as_json().as_bytes())
    ))
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

/// Ask the relay what this key can actually do.
pub async fn cmd_check(
    relay_url: &str,
    keyfile: Option<PathBuf>,
    compact: bool,
) -> Result<(), CliError> {
    let scope = credential_scope(relay_url)?;
    let origin = scope.strip_suffix("/git").unwrap_or(&scope).to_string();
    let effective = resolve_effective_key(keyfile.as_deref())?;
    let key_source = effective.origin.label();
    let key_disclosure = effective.disclosure();
    let keys = effective.keys.clone();
    let pubkey = keys.public_key().to_hex();
    // A managed seat's git requests carry its owner attestation *inside* the
    // signed NIP-98 event — git's credential protocol cannot add a header — so
    // a probe that omitted it would ask the relay a different question than
    // git asks, and could report "member NO" over a key the relay admits.
    let attestation = probe_attestation(&keys)?;
    let auth_tag = attestation.tag.clone();
    let attested_owner = attestation.owner.clone();
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

    // Membership oracle. The relay checks NIP-98 and relay membership in the
    // request extractor, *before* it resolves the repository — so a repo that
    // cannot exist still separates the two answers cleanly:
    //   403 -> the key is not a relay member
    //   404 -> the key is a member; this repo just isn't there
    // That makes membership answerable without needing any repo to exist.
    let sentinel = format!(
        "{}/info/refs?service=git-upload-pack",
        repo_root_url(&origin, &pubkey, "membership-probe-does-not-exist")
    );
    let (sentinel_status, sentinel_body) =
        probe(sentinel, keys.clone(), http.clone(), auth_tag.clone()).await?;
    let is_member = match sentinel_status {
        403 => false,
        404 | 200 => true,
        _ => {
            return Err(CliError::Other(format!(
                "membership probe returned an unexpected HTTP {sentinel_status}: {}",
                sentinel_body.trim()
            )))
        }
    };

    // Repository inventory. Announcements the key cannot read simply do not come
    // back, so this is already "repos visible to this key" — but visibility of
    // the announcement and git read access are separate gates, so each one is
    // still probed rather than assumed.
    let mut probes: Vec<RepoProbe> = Vec::new();
    if is_member {
        let client =
            crate::client::BuzzClient::new(relay_url.to_string(), keys.clone(), None, None)?;
        let raw = client
            .query(&serde_json::json!({ "kinds": [30617], "limit": 500 }))
            .await?;
        let events: Vec<serde_json::Value> = serde_json::from_str(&raw).unwrap_or_default();
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
            probes.push(RepoProbe {
                repo_id: dtag,
                owner: author,
                status,
                access,
                detail,
            });
        }
    }

    let readable = probes.iter().filter(|p| p.access == "read").count();
    let report = serde_json::json!({
        "relay": origin,
        "pubkey": pubkey,
        "key_source": key_source,
        "key_disclosure": key_disclosure,
        // The owner this key is attested to, when it runs as a managed seat and
        // the attestation actually covers this key. Null means the key answers
        // for itself alone.
        "attested_owner": attested_owner,
        "attestation_problem": attestation.warning,
        "relay_member": is_member,
        "repos_probed": probes.len(),
        "repos_readable": readable,
        "repos": probes.iter().map(|p| serde_json::json!({
            "repo_id": p.repo_id,
            "owner": p.owner,
            "access": p.access,
            "http_status": p.status,
            "detail": p.detail,
        })).collect::<Vec<_>>(),
    });

    if compact {
        println!("{}", serde_json::to_string(&report).unwrap_or_default());
        return Ok(());
    }

    println!("relay   {origin}");
    println!("key     {pubkey}  (from {key_source})");
    if let Some(owner) = &attested_owner {
        println!("owner   {owner}  (this key acts for its owner's grant)");
    }
    if let Some(warning) = &attestation.warning {
        println!("owner   none — {warning}");
    }
    if let Some(disclosure) = &key_disclosure {
        println!("note    {disclosure}");
    }
    if !is_member {
        println!(
            "member  NO — the relay rejects this key: {}",
            sentinel_body.trim()
        );
        println!();
        println!("Nothing below can work until this key is a relay member.");
        println!("Every git request is gated on it, clone included.");
        return Ok(());
    }
    println!("member  yes");
    println!();
    if probes.is_empty() {
        println!("No repository announcements are visible to this key.");
        return Ok(());
    }
    println!("{:<34} {:<20} OWNER", "REPO", "ACCESS");
    for entry in &probes {
        println!(
            "{:<34} {:<20} {}",
            entry.repo_id,
            entry.access,
            &entry.owner[..16.min(entry.owner.len())]
        );
    }
    println!();
    println!("{readable} of {} readable over git.", probes.len());
    println!("`no-grant-or-missing` is the relay's single answer for both — it does");
    println!("not distinguish them, so neither does this.");
    Ok(())
}

/// Strip the git service suffix so the signed URL matches the helper's.
fn repo_root_from_refs_url(url: &str) -> String {
    let without_query = url.split('?').next().unwrap_or(url);
    for suffix in ["/info/refs", "/git-upload-pack", "/git-receive-pack"] {
        if let Some(root) = without_query.strip_suffix(suffix) {
            return root.to_string();
        }
    }
    without_query.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine as _;

    /// Serialises the tests that read or write process environment.
    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|e| e.into_inner())
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
        // helper prefers it (`git-credential-nostr` lib.rs `load_key`). A
        // report that named the key *file* would name the operator's identity
        // in a shell where git signs as the seat.
        let env = Keys::generate();
        let resolved = choose_effective_key(Some(env.clone()), Path::new("/tmp/key"), None)
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
            choose_effective_key(None, Path::new("/home/a/.nostr/key"), Some(file.clone()))
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
        let resolved = choose_effective_key(
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
        let resolved = choose_effective_key(
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
        assert!(choose_effective_key(None, Path::new("/home/a/.nostr/key"), None).is_none());
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
