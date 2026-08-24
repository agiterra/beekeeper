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
    let keys = Keys::parse(raw.trim()).map_err(|e| {
        CliError::Key(format!(
            "{} does not hold a usable key: {e}",
            path.display()
        ))
    })?;
    Ok(Some(keys))
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

    let ready = helper_ok == Some(true)
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
        "ready": ready,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&report).unwrap_or_default()
    );
    Ok(())
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
