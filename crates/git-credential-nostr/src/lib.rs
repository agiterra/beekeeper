//! git-credential-nostr — NIP-98 git credential helper for Beekeeper.
//!
//! Git calls this via the credential helper protocol (stdin/stdout).
//! We read the request, sign a kind:27235 event, and return the base64-encoded
//! event as the credential value.  Git then sends:
//!   Authorization: Nostr <credential>

use std::io::{self, BufRead, Read, Write};
use std::path::{Path, PathBuf};

use base64::Engine as _;
use nostr::nips::nip98::{HttpData, HttpMethod};
use nostr::types::Url;
use nostr::{EventBuilder, Keys, PublicKey, Tag};
use zeroize::Zeroize;

fn git_config(key: &str) -> Option<String> {
    let out = std::process::Command::new("git")
        .args(["config", "--get", key])
        .output()
        .ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        None
    }
}

#[cfg(unix)]
fn check_keyfile_permissions(path: &Path) -> Result<(), KeyError> {
    use std::os::unix::fs::PermissionsExt;
    let meta = std::fs::metadata(path)
        .map_err(|e| KeyError::Access(format!("cannot stat keyfile {}: {e}", path.display())))?;
    let mode = meta.permissions().mode() & 0o777;
    if mode & 0o177 != 0 {
        return Err(KeyError::Access(format!(
            "keyfile {} has insecure permissions (mode {:o}); git-credential-nostr requires 0600. \
             Run: chmod 600 {}",
            path.display(),
            mode,
            path.display()
        )));
    }
    Ok(())
}

#[cfg(not(unix))]
fn check_keyfile_permissions(path: &Path) -> Result<(), KeyError> {
    eprintln!(
        "warning: cannot check keyfile permissions on this platform ({})",
        path.display()
    );
    Ok(())
}

/// Max keyfile size — nsec1 is 63 bytes; hex keys are 64 bytes. 256 is generous.
const MAX_KEYFILE_BYTES: u64 = 256;

/// Why the key git would present could not be resolved.
///
/// Two variants, not one string, because callers keep different policies for
/// them: a key file with the wrong mode is an environment problem the user can
/// fix in place, while material that is not a secret key is a key problem. `bee`
/// maps the two onto different exit codes.
#[derive(Debug)]
pub enum KeyError {
    /// The key file exists but cannot be read as it stands — permissions, file
    /// type, or size.
    Access(String),
    /// What was found is not a usable Nostr secret key.
    Material(String),
}

impl std::fmt::Display for KeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Access(message) | Self::Material(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for KeyError {}

/// Where the key git will present came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeySource {
    /// `$NOSTR_PRIVATE_KEY` — what the ACP harness injects into a managed seat,
    /// and what therefore wins over anything on disk.
    Env,
    /// The key file, named by `git config nostr.keyfile` or by the caller.
    Keyfile(PathBuf),
}

/// The identity git will actually present, and the one it will not.
#[derive(Debug, Clone)]
pub struct ResolvedKey {
    /// The key the helper signs with.
    pub keys: Keys,
    /// Which of the two sources it came from.
    pub source: KeySource,
    /// A *different* identity sitting in the key file that git will not use.
    ///
    /// A seat runs with `NOSTR_PRIVATE_KEY` set to its own key while the
    /// operator's key file sits in the same shell. Both are real; only one is
    /// used, and any report that names the unused one is naming the wrong
    /// identity.
    pub shadowed: Option<(PathBuf, PublicKey)>,
}

/// `$NOSTR_PRIVATE_KEY`, or `None` when it is unset or empty.
pub fn env_key() -> Result<Option<Keys>, KeyError> {
    let Ok(mut raw) = std::env::var("NOSTR_PRIVATE_KEY") else {
        return Ok(None);
    };
    if raw.trim().is_empty() {
        raw.zeroize();
        return Ok(None);
    }
    let parsed = Keys::parse(raw.trim())
        .map_err(|e| KeyError::Material(format!("NOSTR_PRIVATE_KEY is not a key: {e}")));
    raw.zeroize();
    parsed.map(Some)
}

/// The key file `git config nostr.keyfile` names, if any.
pub fn configured_keyfile() -> Option<PathBuf> {
    git_config("nostr.keyfile")
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
}

/// Read a key file the way the helper reads it: 0600, a regular file, at most
/// [`MAX_KEYFILE_BYTES`], and an `npub1…` rejected by name.
///
/// `Ok(None)` means the file is not there — which is a state, not a failure:
/// `$NOSTR_PRIVATE_KEY` may still supply the key.
pub fn read_keyfile(path: &Path) -> Result<Option<Keys>, KeyError> {
    if !path.exists() {
        return Ok(None);
    }
    check_keyfile_permissions(path)?;
    let meta = std::fs::metadata(path)
        .map_err(|e| KeyError::Access(format!("cannot stat keyfile {}: {e}", path.display())))?;
    if !meta.is_file() {
        return Err(KeyError::Access(format!(
            "keyfile {} is not a regular file",
            path.display()
        )));
    }
    if meta.len() > MAX_KEYFILE_BYTES {
        return Err(KeyError::Access(format!(
            "keyfile {} exceeds {MAX_KEYFILE_BYTES}-byte size limit",
            path.display()
        )));
    }
    let mut raw = std::fs::read_to_string(path)
        .map_err(|e| KeyError::Access(format!("cannot read keyfile {}: {e}", path.display())))?;
    let trimmed = raw.trim().to_string();
    raw.zeroize();
    // A pasted `npub1…` is a public key where a secret was wanted. It is the one
    // shape of that mistake which is decidable locally, so it gets a real
    // message instead of a parse error further down.
    if trimmed.starts_with("npub1") {
        return Err(KeyError::Material(format!(
            "{} holds an npub, which is a *public* key. The key file needs the matching nsec.",
            path.display()
        )));
    }
    let parsed = Keys::parse(&trimmed).map_err(|e| {
        KeyError::Material(format!(
            "{} does not hold a usable key: {e}",
            path.display()
        ))
    });
    let mut trimmed = trimmed;
    trimmed.zeroize();
    parsed.map(Some)
}

/// Decide, from what each source holds, which key git will present.
///
/// Split from the IO so the precedence itself is testable without touching
/// process environment or the filesystem — the precedence is the part that has
/// been wrong.
pub fn choose_key(
    env: Option<Keys>,
    keyfile_path: &Path,
    keyfile_key: Option<Keys>,
) -> Option<ResolvedKey> {
    match (env, keyfile_key) {
        (Some(env), file) => {
            let shadowed = file
                .filter(|file| file.public_key() != env.public_key())
                .map(|file| (keyfile_path.to_path_buf(), file.public_key()));
            Some(ResolvedKey {
                keys: env,
                source: KeySource::Env,
                shadowed,
            })
        }
        (None, Some(file)) => Some(ResolvedKey {
            keys: file,
            source: KeySource::Keyfile(keyfile_path.to_path_buf()),
            shadowed: None,
        }),
        (None, None) => None,
    }
}

/// Resolve the key git will present: `$NOSTR_PRIVATE_KEY` first, then `keyfile`.
///
/// This is the resolution every Beekeeper caller must use — a check that read
/// `BEEKEEPER_PRIVATE_KEY` instead would test a different identity than the one git
/// presents, and could report success while every push failed.
///
/// `Ok(None)` means neither source holds a key. A key file that cannot be read
/// is *not* an error when the environment supplies a key: the helper would
/// never open it.
pub fn resolve_key(keyfile: &Path) -> Result<Option<ResolvedKey>, KeyError> {
    let env = env_key()?;
    let file = match read_keyfile(keyfile) {
        Ok(found) => found,
        Err(_) if env.is_some() => None,
        Err(error) => return Err(error),
    };
    Ok(choose_key(env, keyfile, file))
}

/// The key the *helper* signs with, resolved from the environment it runs in.
fn helper_key() -> Result<Keys, String> {
    if let Some(keys) = env_key().map_err(|e| e.to_string())? {
        return Ok(keys);
    }
    let path = configured_keyfile().ok_or_else(|| {
        "no nostr key configured. Set $NOSTR_PRIVATE_KEY or git config nostr.keyfile".to_string()
    })?;
    read_keyfile(&path)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("cannot stat keyfile {}: no such file", path.display()))
}

/// Strip git's service suffix so the signed URL is the repo root.
///
/// git invokes a credential helper once per challenge and reuses the header
/// across the `GET /info/refs` and the `POST /git-upload-pack`, so the NIP-98
/// `u` tag has to name the repo root. A request signed for the exact path is
/// rejected where real git succeeds — see docs/git-nip98-method-binding.md.
pub fn repo_root_url(request_url: &str) -> String {
    let without_query = request_url.split('?').next().unwrap_or(request_url);
    for suffix in ["/info/refs", "/git-upload-pack", "/git-receive-pack"] {
        if let Some(root) = without_query.strip_suffix(suffix) {
            return root.to_string();
        }
    }
    without_query.to_string()
}

/// Build the `Authorization: Nostr <base64 kind:27235>` value git sends.
///
/// `auth_tag` is the NIP-OA owner attestation, which must ride *inside* the
/// signed event: git's credential protocol can return an Authorization value
/// but cannot add a second header. Any caller that asks the relay what git can
/// do has to sign the same event, attestation included, or it is asking a
/// different question.
pub fn authorization_header(
    keys: &Keys,
    method: HttpMethod,
    repo_root: &str,
    auth_tag: Option<Tag>,
) -> Result<String, String> {
    let url = Url::parse(repo_root).map_err(|e| format!("invalid URL {repo_root:?}: {e}"))?;
    let builder = EventBuilder::http_auth(HttpData::new(url, method));
    let builder = match auth_tag {
        Some(tag) => builder.tag(tag),
        None => builder,
    };
    let event = builder
        .sign_with_keys(keys)
        .map_err(|e| format!("failed to sign NIP-98 event: {e}"))?;
    let json =
        serde_json::to_string(&event).map_err(|e| format!("failed to serialize event: {e}"))?;
    Ok(format!(
        "Nostr {}",
        base64::engine::general_purpose::STANDARD.encode(json.as_bytes())
    ))
}

/// Load the NIP-OA owner attestation injected by Beekeeper Desktop/ACP.
///
/// The tag must be part of the signed NIP-98 event: Git's credential protocol
/// can return an Authorization value, but it cannot add a separate HTTP header.
///
/// A malformed tag is an error, not a shrug — the helper fails closed on it, so
/// every git request fails, and a caller that quietly dropped it would report a
/// success git will never have.
pub fn resolve_auth_tag() -> Result<Option<Tag>, String> {
    let raw = std::env::var("BEEKEEPER_AUTH_TAG")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| git_config("nostr.authtag"));

    raw.map(|value| {
        let parts: Vec<String> =
            serde_json::from_str(&value).map_err(|e| format!("invalid NIP-OA auth tag: {e}"))?;
        if parts.len() != 4 || parts.first().map(String::as_str) != Some("auth") {
            return Err(
                "invalid NIP-OA auth tag: expected [auth, owner, conditions, signature]"
                    .to_string(),
            );
        }
        Tag::parse(parts).map_err(|e| format!("invalid NIP-OA auth tag: {e}"))
    })
    .transpose()
}

#[derive(Default)]
struct CredRequest {
    has_authtype_capability: bool,
    protocol: Option<String>,
    host: Option<String>,
    path: Option<String>,
    wwwauth: Option<String>,
}

fn parse_stdin() -> CredRequest {
    let stdin = io::stdin();
    let mut req = CredRequest::default();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        if line.is_empty() {
            break;
        }
        if line == "capability[]=authtype" {
            req.has_authtype_capability = true;
        } else if let Some(v) = line.strip_prefix("protocol=") {
            req.protocol = Some(v.to_string());
        } else if let Some(v) = line.strip_prefix("host=") {
            req.host = Some(v.to_string());
        } else if let Some(v) = line.strip_prefix("path=") {
            req.path = Some(v.to_string());
        } else if let Some(v) = line.strip_prefix("wwwauth[]=") {
            if v.starts_with("Nostr ") && req.wwwauth.is_none() {
                req.wwwauth = Some(v.to_string());
            }
        }
    }
    req
}

fn parse_method(wwwauth: &str) -> Option<HttpMethod> {
    // Strip the scheme prefix ("Nostr ") if present, then split on commas.
    // Handles variations: `Nostr method="GET", realm="buzz"` and
    // `Nostr method="GET",realm="buzz"` (with or without space after comma).
    let params = wwwauth.strip_prefix("Nostr ").unwrap_or(wwwauth);
    for param in params.split(',') {
        let param = param.trim();
        if let Some(rest) = param.strip_prefix("method=\"") {
            let end = rest.find('"')?;
            return rest[..end].parse().ok();
        }
    }
    None
}

/// Short prefix of a pubkey used to name a key without printing it whole.
const PUBKEY_PREFIX_LEN: usize = 8;

/// Tell the user which key the relay just refused — and nothing else.
///
/// `erase` is the only denial signal git's credential protocol hands a helper:
/// git calls it when it rejects the credential the previous `get` supplied.
/// Without this line a seat sees git's generic failure with no hint that the
/// *key* is what was refused, and a shell where `NOSTR_PRIVATE_KEY` and the
/// configured keyfile disagree presents an identity the user never chose.
///
/// The line names the key and points at `bee git check`. It deliberately does
/// not state a reason: the relay answers every git denial identically so
/// membership cannot be probed, so any reason printed here would be invented.
fn report_denial() {
    // Drain stdin first — git writes the credential fields and expects the
    // helper to consume them.
    let mut discarded = String::new();
    let _ = io::stdin().lock().read_to_string(&mut discarded);

    let Ok(keys) = helper_key() else {
        // No key was ever presented, so nothing of ours was denied.
        return;
    };
    let pubkey = keys.public_key().to_hex();
    let short = &pubkey[..PUBKEY_PREFIX_LEN.min(pubkey.len())];
    eprintln!("relay denied this key ({short}\u{2026}); run `bee git check` to see why");
}

/// The first git that implements the `authtype` credential protocol this
/// helper answers over, stated the way a person is told it.
pub const MINIMUM_GIT_VERSION: &str = "2.46";

/// What to tell someone whose git is too old, on this platform.
#[cfg(target_os = "macos")]
const INSTALL_HINT: &str = "Install it with `brew install git` and retry.";
#[cfg(not(target_os = "macos"))]
const INSTALL_HINT: &str = "Install git 2.46 or newer and retry.";

/// The one sentence printed when git cannot carry a Nostr credential.
///
/// The same explanation the desktop app refuses a remote operation with
/// (`desktop/src-tauri/src/commands/project_git_version.rs`), in the one
/// place a terminal user meets it. Pure, and given its hint, so the wording
/// is testable without a platform.
pub fn authtype_unsupported_message(install_hint: &str) -> String {
    format!(
        "git-credential-nostr: this git cannot authenticate to the relay — it does not \
         announce the authtype credential capability, which needs git \
         {MINIMUM_GIT_VERSION} or newer. {install_hint}"
    )
}

/// Run the credential helper. Returns exit code.
/// Reads from stdin, writes to stdout. Errors go to stderr only.
pub fn run() -> i32 {
    match std::env::args().nth(1).as_deref() {
        Some("get") | None => {}
        Some("erase") => {
            report_denial();
            return 0;
        }
        Some(_) => return 0, // store or unknown → silent exit 0
    }

    let req = parse_stdin();

    if !req.has_authtype_capability {
        // Ledger 168: git older than 2.46 does not announce `authtype`, so
        // this helper has no way to hand git a NIP-98 credential and every
        // authenticated operation dies on git's own
        // `could not read Username … terminal prompts disabled`, which names
        // the wrong problem entirely. The empty answer still goes to stdout,
        // because git's protocol expects one; the explanation goes to stderr,
        // where a terminal user reads it.
        eprintln!("{}", authtype_unsupported_message(INSTALL_HINT));
        println!();
        let _ = io::stdout().flush();
        return 0;
    }

    macro_rules! require {
        ($opt:expr, $msg:expr) => {
            match $opt {
                Some(v) => v,
                None => {
                    eprintln!("error: {}", $msg);
                    return 1;
                }
            }
        };
    }

    // No Nostr challenge from the server — this isn't a Beekeeper remote.
    // Exit silently so git falls through to the next credential helper.
    // This check comes FIRST so non-Beekeeper remotes never hit validation errors.
    let wwwauth = match req.wwwauth.as_deref() {
        Some(v) => v,
        None => return 0,
    };
    let method = match parse_method(wwwauth) {
        Some(m) => m,
        None => return 0,
    };

    let protocol = require!(
        req.protocol.as_deref(),
        "missing protocol in credential request"
    );
    let host = require!(req.host.as_deref(), "missing host in credential request");
    let path = require!(
        req.path.as_deref(),
        "credential.useHttpPath must be true for NIP-98 auth"
    );

    let url = repo_root_url(&format!("{protocol}://{host}/{path}"));

    let keys = match helper_key() {
        Ok(k) => k,
        Err(e) => {
            eprintln!("error: {e}");
            return 1;
        }
    };

    let auth_tag = match resolve_auth_tag() {
        Ok(tag) => tag,
        Err(e) => {
            eprintln!("error: {e}");
            return 1;
        }
    };

    // One signing path, shared with `bee git check`: the check has to sign the
    // event git sends, or it answers a different question than the one it is
    // presented as answering.
    let header = match authorization_header(&keys, method, &url, auth_tag) {
        Ok(header) => header,
        Err(e) => {
            eprintln!("error: {e}");
            return 1;
        }
    };
    let credential = header.strip_prefix("Nostr ").unwrap_or(&header);

    println!("capability[]=authtype");
    println!("authtype=Nostr");
    println!("credential={credential}");
    println!("ephemeral=true");
    println!("quit=true");
    println!();
    let _ = io::stdout().flush();
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::JsonUtil as _;

    #[test]
    fn the_signed_url_is_always_the_repo_root() {
        // git invokes a helper once per challenge and reuses the header across
        // the GET and the POST, so the `u` tag must name the repo root. Every
        // caller — the helper and `bee git check` — goes through this one
        // function so neither can sign a URL the other would not.
        let root = "https://hive.agiterra.org/git/abc/repo";
        for url in [
            format!("{root}/info/refs?service=git-upload-pack"),
            format!("{root}/info/refs?service=git-receive-pack"),
            format!("{root}/git-upload-pack"),
            format!("{root}/git-receive-pack"),
            root.to_string(),
        ] {
            assert_eq!(repo_root_url(&url), root, "for {url}");
        }
    }

    #[test]
    fn the_authorization_header_carries_the_attestation_inside_the_signature() {
        let seat = Keys::generate();
        let owner = Keys::generate();
        let tag = Tag::parse([
            "auth".to_string(),
            owner.public_key().to_hex(),
            String::new(),
            "00".repeat(64),
        ])
        .expect("tag");

        let header = authorization_header(
            &seat,
            HttpMethod::GET,
            "https://relay.example/git/abc/repo",
            Some(tag.clone()),
        )
        .expect("header");

        let encoded = header.strip_prefix("Nostr ").expect("Nostr scheme");
        let json = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .expect("base64");
        let event = nostr::Event::from_json(&json).expect("event");

        assert!(
            event.verify().is_ok(),
            "the attestation must be covered by the signature — git cannot send it as a header"
        );
        assert_eq!(event.pubkey, seat.public_key(), "a seat signs as itself");
        assert!(event.tags.iter().any(|t| t.as_slice() == tag.as_slice()));
        assert!(event
            .tags
            .iter()
            .any(|t| t.as_slice() == ["u", "https://relay.example/git/abc/repo"]));
    }

    #[test]
    fn the_env_key_wins_over_the_key_file() {
        // The ACP harness injects `NOSTR_PRIVATE_KEY` into a seat; the file on
        // disk is usually the operator's. Naming the file's identity in a seat's
        // shell names the wrong key.
        let env = Keys::generate();
        let file = Keys::generate();
        let resolved = choose_key(
            Some(env.clone()),
            Path::new("/home/a/.nostr/key"),
            Some(file.clone()),
        )
        .expect("a key");
        assert_eq!(resolved.keys.public_key(), env.public_key());
        assert_eq!(resolved.source, KeySource::Env);
        assert_eq!(
            resolved.shadowed.map(|(_, key)| key),
            Some(file.public_key()),
            "the identity git will NOT use is still reported"
        );
    }

    #[test]
    fn a_missing_key_file_is_a_state_not_a_failure() {
        let missing = Path::new("/nonexistent/beekeeper/key");
        assert!(matches!(read_keyfile(missing), Ok(None)));
    }

    /// Ledger 168. Before this, a git with no `authtype` capability got an
    /// empty answer and silence, and the user got git's username prompt
    /// error — a sentence about a credential, for a problem that is the git.
    #[test]
    fn a_git_without_authtype_is_told_why_it_cannot_authenticate() {
        let message = authtype_unsupported_message("Install it with `brew install git` and retry.");
        assert_eq!(
            message,
            "git-credential-nostr: this git cannot authenticate to the relay — it does not \
             announce the authtype credential capability, which needs git 2.46 or newer. \
             Install it with `brew install git` and retry."
        );
    }

    /// The request parser must keep telling the capability apart from the
    /// rest of the request, because that is the fact the message rests on.
    #[test]
    fn the_capability_line_is_the_only_thing_that_announces_authtype() {
        assert!(!CredRequest::default().has_authtype_capability);
    }
}
