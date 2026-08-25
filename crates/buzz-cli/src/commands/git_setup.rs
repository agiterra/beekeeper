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

/// The three config entries that make terminal git access work.
pub fn config_entries(scope: &str, helper: &str, keyfile: &Path) -> Vec<(String, String)> {
    vec![
        (format!("credential.{scope}.helper"), helper.to_string()),
        (
            format!("credential.{scope}.useHttpPath"),
            "true".to_string(),
        ),
        (
            "nostr.keyfile".to_string(),
            keyfile.to_string_lossy().replace('\\', "/"),
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

/// Load the key exactly as `git-credential-nostr` does: `$NOSTR_PRIVATE_KEY`
/// first, then `git config nostr.keyfile`.
///
/// Reproducing the helper's precedence is the whole point. A check that read
/// `BUZZ_PRIVATE_KEY` instead would test a different identity than the one git
/// actually presents, and could report success while every push failed.
fn helper_effective_keys(keyfile: Option<&Path>) -> Result<(Keys, String), CliError> {
    if let Ok(env_key) = std::env::var("NOSTR_PRIVATE_KEY") {
        if !env_key.trim().is_empty() {
            let keys = Keys::parse(env_key.trim())
                .map_err(|e| CliError::Key(format!("NOSTR_PRIVATE_KEY is not a key: {e}")))?;
            return Ok((keys, "NOSTR_PRIVATE_KEY".to_string()));
        }
    }
    let path = match keyfile {
        Some(path) => path.to_path_buf(),
        None => match git_config_get("nostr.keyfile") {
            Some(configured) => PathBuf::from(configured),
            None => default_keyfile()?,
        },
    };
    match read_keyfile(&path)? {
        Some(keys) => Ok((keys, path.to_string_lossy().to_string())),
        None => Err(CliError::Usage(format!(
            "no key: {} does not exist and NOSTR_PRIVATE_KEY is unset",
            path.display()
        ))),
    }
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

fn git_config_set(scope: ConfigScope, key: &str, value: &str) -> Result<(), CliError> {
    let output = Command::new("git")
        .args(["config", scope.flag(), key, value])
        .output()
        .map_err(|e| CliError::Usage(format!("cannot run git: {e}")))?;
    if !output.status.success() {
        return Err(CliError::Usage(format!(
            "git config {} {key} failed: {}",
            scope.flag(),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(())
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
        for (key, value) in &entries {
            println!("git config {} '{key}' '{value}'", request.scope.flag());
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

    for (key, value) in &entries {
        git_config_set(request.scope, key, value)?;
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
            println!("Terminal git access is ready.");
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
    let configured_helper = git_config_get(&format!("credential.{scope_url}.helper"));
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

    // NOT "ready". This says the three local pieces are in place — it cannot
    // say the relay accepts the key, and reporting `ready: true` over a 403 is
    // exactly the kind of comfortable guess this project treats as a bug.
    // `bee git check` is the one that asks.
    let configured = helper_ok == Some(true)
        && configured_path.as_deref() == Some("true")
        && key_pubkey.is_some();

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
    let (keys, key_source) = helper_effective_keys(keyfile.as_deref())?;
    let pubkey = keys.public_key().to_hex();
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| CliError::Other(e.to_string()))?;

    let probe = |url: String, keys: Keys, http: reqwest::Client| async move {
        let signed = repo_root_from_refs_url(&url);
        let auth = crate::client::sign_nip98(&keys, "GET", &signed, None)?;
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
    let (sentinel_status, sentinel_body) = probe(sentinel, keys.clone(), http.clone()).await?;
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
            let (status, body) = probe(url, keys.clone(), http.clone()).await?;
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
