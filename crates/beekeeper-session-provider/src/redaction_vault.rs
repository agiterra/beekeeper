//! The host's own record of what it redacted out of its transcripts.
//!
//! A published transcript replaces every host-private value with a
//! content-addressed marker (`[elided private context: 148 bytes, sha256:…]`).
//! That is right for the channel and wrong for the person sitting at the
//! machine that produced it: the thing most often hidden is a path in their own
//! home directory, and showing it back to them discloses nothing they do not
//! already know.
//!
//! So the provider keeps a private, local, expiring note of the recoverable
//! ones, keyed by the same digest the marker carries — no new wire field, and
//! nothing that leaves this machine.
//!
//! # What is never in here
//!
//! **Secrets.** [`beekeeper_core::coding_session_context::RedactionClass::is_recoverable`]
//! is the gate, and it is applied where redactions are *produced*, not here:
//! a credential never reaches this module in the first place, so no bug in this
//! file can write one to disk. Writing redacted credentials to plaintext files
//! would be a liability strictly worse than the readability problem the vault
//! exists to solve.
//!
//! # Expiry
//!
//! The vault is a debugging convenience, not an archive, and every entry is a
//! fact about the host. Three independent reapers, because each catches what
//! the others miss:
//!
//! 1. **Session-scoped** — [`remove_session`] on stop, the same way stopping an
//!    execution already removes its context-package directory. The common case.
//! 2. **Age-scoped** — [`sweep`] drops files older than the retention window at
//!    startup and daily after. Long enough to outlive a weekend plus a week of
//!    not looking at a transcript; short enough that an abandoned machine is not
//!    accumulating a map of its own filesystem forever.
//! 3. **Size-scoped** — a per-session cap and a whole-vault cap, oldest file
//!    first. A session that redacts thousands of paths must not be able to fill
//!    a disk.
//!
//! An expired lookup is indistinguishable from a lookup on another machine, and
//! both render the same. The reader is never told "expired" when what actually
//! happened cannot be proven.

use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write as _};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use beekeeper_core::coding_session_context::Redaction;
use serde::{Deserialize, Serialize};

const REDACTION_DIRECTORY: &str = "redactions";

/// Default retention window. Overridden by `BEEKEEPER_CSP_REDACTION_RETENTION_DAYS`;
/// `0` disables recording outright.
pub const DEFAULT_RETENTION_DAYS: u64 = 14;

/// Cap on one session's vault file.
pub const MAX_SESSION_VAULT_BYTES: u64 = 1024 * 1024;

/// Cap on the whole vault, oldest session file dropped first.
pub const MAX_VAULT_BYTES: u64 = 64 * 1024 * 1024;

/// One recorded redaction, as it sits on disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultEntry {
    /// SHA-256 of the value's JSON encoding — the join to the published marker.
    pub digest: String,
    /// Serialized JSON byte count, matching the marker.
    pub bytes: usize,
    /// The rule that caught it; always a recoverable class.
    pub class: String,
    /// The value before redaction.
    pub plaintext: String,
    /// Unix milliseconds, matching the transcript envelope's own clock.
    pub recorded_at: i64,
}

impl VaultEntry {
    fn from_redaction(redaction: &Redaction, recorded_at: i64) -> Self {
        Self {
            digest: redaction.digest.clone(),
            bytes: redaction.bytes,
            class: serde_json::to_value(redaction.class)
                .ok()
                .and_then(|value| value.as_str().map(str::to_owned))
                .unwrap_or_else(|| "unknown".to_owned()),
            plaintext: redaction.plaintext.clone(),
            recorded_at,
        }
    }
}

/// How long recorded redactions are kept, and whether they are kept at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetentionPolicy {
    days: u64,
}

impl RetentionPolicy {
    /// The environment variable that sets this policy.
    pub const ENV_VAR: &'static str = "BEEKEEPER_CSP_REDACTION_RETENTION_DAYS";

    /// Read the policy from a raw setting value.
    ///
    /// An unparseable value falls back to the default rather than failing
    /// startup: a typo in one convenience knob must not stop a provider from
    /// running sessions.
    pub fn from_setting(raw: Option<&str>) -> Self {
        let days = raw
            .and_then(|raw| raw.trim().parse::<u64>().ok())
            .unwrap_or(DEFAULT_RETENTION_DAYS);
        Self { days }
    }

    /// A policy that keeps nothing — no file is ever created.
    pub fn disabled() -> Self {
        Self { days: 0 }
    }

    /// Retain for exactly `days`; `0` disables recording.
    pub fn days(days: u64) -> Self {
        Self { days }
    }

    /// Is recording on at all?
    pub fn enabled(self) -> bool {
        self.days > 0
    }

    fn window(self) -> Duration {
        Duration::from_secs(self.days * 24 * 60 * 60)
    }
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            days: DEFAULT_RETENTION_DAYS,
        }
    }
}

/// Append recoverable redactions for one session.
///
/// Never fails a publish: the transcript is the product and this is a local
/// convenience, so every error is returned for the caller to log and drop
/// rather than propagate. Entries whose digest this session has already
/// recorded are skipped by the caller (see `Provider`), which is what keeps a
/// repeated home directory from writing a line per item.
pub fn append(
    state_dir: &Path,
    session_id: &str,
    entries: &[Redaction],
    recorded_at: i64,
    retention: RetentionPolicy,
) -> Result<(), VaultError> {
    if !retention.enabled() || entries.is_empty() {
        return Ok(());
    }
    let path = session_path(state_dir, session_id)?;
    let root = vault_root(state_dir)?;
    std::fs::create_dir_all(&root).map_err(|error| VaultError::storage("create vault", error))?;
    reject_symlink(&root)?;
    set_private_directory_permissions(&root)?;
    reject_symlink(&path)?;

    // A session that redacts pathologically must not be able to fill a disk.
    // Refusing further appends is the honest failure: the reader sees an
    // unresolved pill, which is the same thing they would see on any other
    // machine, rather than a truncated file that silently loses older entries.
    if file_len(&path) >= MAX_SESSION_VAULT_BYTES {
        return Err(VaultError::Full);
    }

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|error| VaultError::storage("open vault file", error))?;
    set_private_file_permissions(&file)?;

    let mut buffer = String::new();
    for redaction in entries {
        // Belt and braces. The class gate already ran where the redaction was
        // produced; re-checking here means no future caller can hand this
        // module a secret by constructing a `Redaction` itself.
        if !redaction.class.is_recoverable() {
            continue;
        }
        let entry = VaultEntry::from_redaction(redaction, recorded_at);
        let Ok(line) = serde_json::to_string(&entry) else {
            continue;
        };
        buffer.push_str(&line);
        buffer.push('\n');
    }
    if buffer.is_empty() {
        return Ok(());
    }
    file.write_all(buffer.as_bytes())
        .map_err(|error| VaultError::storage("write vault file", error))?;
    Ok(())
}

/// Resolve digests recorded for one session.
///
/// A digest with no entry is simply absent from the map. The caller must not
/// turn that into a claim about *why* — expired, never recorded, and "this is
/// not the machine that produced it" are indistinguishable here.
pub fn resolve(
    state_dir: &Path,
    session_id: &str,
    digests: &[String],
) -> Result<HashMap<String, VaultEntry>, VaultError> {
    let wanted: std::collections::HashSet<&str> = digests.iter().map(String::as_str).collect();
    let path = session_path(state_dir, session_id)?;
    if wanted.is_empty() || !path.exists() {
        return Ok(HashMap::new());
    }
    reject_symlink(&path)?;
    let file = std::fs::File::open(&path)
        .map_err(|error| VaultError::storage("open vault file", error))?;

    let mut found = HashMap::new();
    for line in BufReader::new(file).lines() {
        let Ok(line) = line else { break };
        let Ok(entry) = serde_json::from_str::<VaultEntry>(&line) else {
            // A partial final line is what a crash mid-write leaves. Skip it
            // rather than refuse the whole file.
            continue;
        };
        if wanted.contains(entry.digest.as_str()) {
            found.insert(entry.digest.clone(), entry);
        }
    }
    Ok(found)
}

/// Drop one session's vault. Called when its execution stops.
pub fn remove_session(state_dir: &Path, session_id: &str) -> Result<(), VaultError> {
    let path = session_path(state_dir, session_id)?;
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(VaultError::storage("remove vault file", error)),
    }
}

/// Drop everything the vault holds.
pub fn remove_all(state_dir: &Path) -> Result<(), VaultError> {
    let root = vault_root(state_dir)?;
    match std::fs::remove_dir_all(&root) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(VaultError::storage("remove vault", error)),
    }
}

/// What one sweep removed, so the caller can say so rather than guess.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SweepOutcome {
    /// Files dropped for being older than the retention window.
    pub expired: usize,
    /// Files dropped to bring the vault back under its total cap.
    pub oversize: usize,
}

/// Run the age and size reapers.
///
/// Safe to call at startup and on a timer. With recording disabled the whole
/// vault goes, so turning the knob off is not merely "stop writing" — it also
/// removes what was already written.
pub fn sweep(
    state_dir: &Path,
    retention: RetentionPolicy,
    now: SystemTime,
) -> Result<SweepOutcome, VaultError> {
    let root = vault_root(state_dir)?;
    if !root.exists() {
        return Ok(SweepOutcome::default());
    }
    if !retention.enabled() {
        remove_all(state_dir)?;
        return Ok(SweepOutcome::default());
    }
    reject_symlink(&root)?;

    let mut outcome = SweepOutcome::default();
    let mut surviving: Vec<(SystemTime, u64, PathBuf)> = Vec::new();
    let entries = std::fs::read_dir(&root)
        .map_err(|error| VaultError::storage("read vault directory", error))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }
        let modified = metadata.modified().unwrap_or(now);
        if now
            .duration_since(modified)
            .is_ok_and(|age| age > retention.window())
        {
            if std::fs::remove_file(&path).is_ok() {
                outcome.expired += 1;
            }
            continue;
        }
        surviving.push((modified, metadata.len(), path));
    }

    // Oldest first, so the cap eats the least useful files.
    surviving.sort_by_key(|(modified, _, _)| *modified);
    let mut total: u64 = surviving.iter().map(|(_, len, _)| *len).sum();
    for (_, len, path) in &surviving {
        if total <= MAX_VAULT_BYTES {
            break;
        }
        if std::fs::remove_file(path).is_ok() {
            outcome.oversize += 1;
            total = total.saturating_sub(*len);
        }
    }
    Ok(outcome)
}

/// Drop every vault file that does not belong to a currently live session.
///
/// The startup counterpart to [`remove_session`]: a provider killed mid-session
/// never ran the stop path, and its vault would otherwise survive until the age
/// reaper got to it.
pub fn sweep_orphans(state_dir: &Path, live_session_ids: &[String]) -> Result<usize, VaultError> {
    let root = vault_root(state_dir)?;
    if !root.exists() {
        return Ok(0);
    }
    reject_symlink(&root)?;
    let live: std::collections::HashSet<&str> =
        live_session_ids.iter().map(String::as_str).collect();

    let mut removed = 0;
    let entries = std::fs::read_dir(&root)
        .map_err(|error| VaultError::storage("read vault directory", error))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(session_id) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        if live.contains(session_id) {
            continue;
        }
        if std::fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}

/// Failure to record or read one session's redaction vault.
#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    /// The session id could not name a file inside the vault.
    #[error("redaction vault: unsafe session id")]
    UnsafeSessionId,
    /// A path the vault would use is a symlink, or the state path is unusable.
    #[error("redaction vault: {0}")]
    Storage(String),
    /// This session's vault is at its cap; nothing more is recorded for it.
    #[error("redaction vault: session vault is full")]
    Full,
}

impl VaultError {
    fn storage(what: &str, error: std::io::Error) -> Self {
        Self::Storage(format!("{what}: {error}"))
    }
}

/// `<state_dir>/redactions`.
fn vault_root(state_dir: &Path) -> Result<PathBuf, VaultError> {
    if state_dir.as_os_str().is_empty() {
        return Err(VaultError::Storage("state directory is empty".into()));
    }
    Ok(state_dir.join(REDACTION_DIRECTORY))
}

/// `<state_dir>/redactions/<session-id>.jsonl`, with the session id proven to
/// be a single safe path component.
fn session_path(state_dir: &Path, session_id: &str) -> Result<PathBuf, VaultError> {
    if !safe_session_id(session_id) {
        return Err(VaultError::UnsafeSessionId);
    }
    Ok(vault_root(state_dir)?.join(format!("{session_id}.jsonl")))
}

/// Session ids are provider-minted UUIDs. Anything that is not one cannot name
/// a file here — a traversal in a relay-supplied string must fail closed, not
/// resolve to some other directory's contents.
fn safe_session_id(session_id: &str) -> bool {
    !session_id.is_empty()
        && session_id.len() <= 128
        && session_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

fn file_len(path: &Path) -> u64 {
    std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0)
}

fn reject_symlink(path: &Path) -> Result<(), VaultError> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(VaultError::Storage(format!(
            "{} is a symlink",
            path.display()
        ))),
        _ => Ok(()),
    }
}

#[cfg(unix)]
fn set_private_directory_permissions(path: &Path) -> Result<(), VaultError> {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
        .map_err(|error| VaultError::storage("set vault directory permissions", error))
}

#[cfg(not(unix))]
fn set_private_directory_permissions(_path: &Path) -> Result<(), VaultError> {
    Ok(())
}

#[cfg(unix)]
fn set_private_file_permissions(file: &std::fs::File) -> Result<(), VaultError> {
    use std::os::unix::fs::PermissionsExt as _;
    file.set_permissions(std::fs::Permissions::from_mode(0o600))
        .map_err(|error| VaultError::storage("set vault file permissions", error))
}

#[cfg(not(unix))]
fn set_private_file_permissions(_file: &std::fs::File) -> Result<(), VaultError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use beekeeper_core::coding_session_context::RedactionClass;
    use tempfile::TempDir;

    use super::*;

    fn host_path(plaintext: &str, digest: &str) -> Redaction {
        Redaction {
            digest: digest.to_owned(),
            bytes: plaintext.len() + 2,
            class: RedactionClass::HostPath,
            plaintext: plaintext.to_owned(),
        }
    }

    fn secret(plaintext: &str, digest: &str) -> Redaction {
        Redaction {
            digest: digest.to_owned(),
            bytes: plaintext.len() + 2,
            class: RedactionClass::ShapedSecret,
            plaintext: plaintext.to_owned(),
        }
    }

    #[test]
    fn a_recorded_path_resolves_by_the_digest_its_marker_carries() {
        let dir = TempDir::new().unwrap();
        append(
            dir.path(),
            "session-1",
            &[host_path("/Users/andy/Code/thing.rs", "aa")],
            1_000,
            RetentionPolicy::default(),
        )
        .unwrap();

        let found = resolve(dir.path(), "session-1", &["aa".to_owned()]).unwrap();
        assert_eq!(found["aa"].plaintext, "/Users/andy/Code/thing.rs");
        assert_eq!(found["aa"].class, "host-path");
    }

    /// The gate runs where redactions are produced, and again here. A caller
    /// that hand-builds a secret `Redaction` still cannot get it onto disk.
    #[test]
    fn a_secret_handed_to_the_vault_directly_is_still_not_written() {
        let dir = TempDir::new().unwrap();
        append(
            dir.path(),
            "session-1",
            &[secret("ghp_aaaaaaaaaaaaaaaaaaaa", "bb")],
            1_000,
            RetentionPolicy::default(),
        )
        .unwrap();

        let found = resolve(dir.path(), "session-1", &["bb".to_owned()]).unwrap();
        assert!(found.is_empty(), "{found:?}");
        let raw = std::fs::read_to_string(dir.path().join("redactions/session-1.jsonl"))
            .unwrap_or_default();
        assert!(!raw.contains("ghp_"), "{raw}");
    }

    #[test]
    fn a_digest_that_was_never_recorded_is_simply_absent() {
        let dir = TempDir::new().unwrap();
        append(
            dir.path(),
            "session-1",
            &[host_path("/Users/andy", "aa")],
            1_000,
            RetentionPolicy::default(),
        )
        .unwrap();

        let found = resolve(dir.path(), "session-1", &["zz".to_owned()]).unwrap();
        assert!(found.is_empty());
    }

    #[test]
    fn one_session_never_resolves_another_sessions_digest() {
        let dir = TempDir::new().unwrap();
        append(
            dir.path(),
            "session-1",
            &[host_path("/Users/andy", "aa")],
            1_000,
            RetentionPolicy::default(),
        )
        .unwrap();

        let found = resolve(dir.path(), "session-2", &["aa".to_owned()]).unwrap();
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn disabled_retention_records_nothing_and_removes_what_exists() {
        let dir = TempDir::new().unwrap();
        append(
            dir.path(),
            "session-1",
            &[host_path("/Users/andy", "aa")],
            1_000,
            RetentionPolicy::default(),
        )
        .unwrap();
        assert!(dir.path().join("redactions/session-1.jsonl").exists());

        // Turning the knob off is not merely "stop writing".
        sweep(dir.path(), RetentionPolicy::disabled(), SystemTime::now()).unwrap();
        assert!(!dir.path().join("redactions").exists());

        append(
            dir.path(),
            "session-1",
            &[host_path("/Users/andy", "aa")],
            1_000,
            RetentionPolicy::disabled(),
        )
        .unwrap();
        assert!(!dir.path().join("redactions").exists());
    }

    #[test]
    fn a_file_older_than_the_window_is_swept_and_a_fresh_one_survives() {
        let dir = TempDir::new().unwrap();
        let policy = RetentionPolicy::days(14);
        append(dir.path(), "old", &[host_path("/a", "aa")], 1, policy).unwrap();
        append(dir.path(), "new", &[host_path("/b", "bb")], 1, policy).unwrap();

        // Both files were just written, so age has to be staged: backdate one.
        let old_path = dir.path().join("redactions/old.jsonl");
        let ancient = SystemTime::now() - Duration::from_secs(20 * 24 * 60 * 60);
        OpenOptions::new()
            .write(true)
            .open(&old_path)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(ancient))
            .unwrap();

        let outcome = sweep(dir.path(), policy, SystemTime::now()).unwrap();
        assert_eq!(outcome.expired, 1, "{outcome:?}");
        assert!(!old_path.exists());
        assert!(dir.path().join("redactions/new.jsonl").exists());
    }

    #[test]
    fn a_session_vault_at_its_cap_refuses_rather_than_losing_older_entries() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("redactions/session-1.jsonl");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, vec![b'x'; MAX_SESSION_VAULT_BYTES as usize + 1]).unwrap();

        let error = append(
            dir.path(),
            "session-1",
            &[host_path("/Users/andy", "aa")],
            1_000,
            RetentionPolicy::default(),
        )
        .unwrap_err();
        assert!(matches!(error, VaultError::Full), "{error:?}");
    }

    #[test]
    fn stopping_a_session_removes_its_vault_and_leaves_the_others() {
        let dir = TempDir::new().unwrap();
        let policy = RetentionPolicy::default();
        append(dir.path(), "one", &[host_path("/a", "aa")], 1, policy).unwrap();
        append(dir.path(), "two", &[host_path("/b", "bb")], 1, policy).unwrap();

        remove_session(dir.path(), "one").unwrap();
        assert!(!dir.path().join("redactions/one.jsonl").exists());
        assert!(dir.path().join("redactions/two.jsonl").exists());
        // Idempotent: a stop that arrives twice is not an error.
        remove_session(dir.path(), "one").unwrap();
    }

    #[test]
    fn a_startup_sweep_removes_vaults_no_live_session_owns() {
        let dir = TempDir::new().unwrap();
        let policy = RetentionPolicy::default();
        append(dir.path(), "live", &[host_path("/a", "aa")], 1, policy).unwrap();
        append(dir.path(), "dead", &[host_path("/b", "bb")], 1, policy).unwrap();

        let removed = sweep_orphans(dir.path(), &["live".to_owned()]).unwrap();
        assert_eq!(removed, 1);
        assert!(dir.path().join("redactions/live.jsonl").exists());
        assert!(!dir.path().join("redactions/dead.jsonl").exists());
    }

    /// A relay-supplied string must never name a file outside the vault.
    #[test]
    fn a_traversing_session_id_fails_closed() {
        let dir = TempDir::new().unwrap();
        for id in ["../escape", "a/b", "", "..", "with space", "a\0b"] {
            let error = append(
                dir.path(),
                id,
                &[host_path("/a", "aa")],
                1,
                RetentionPolicy::default(),
            )
            .unwrap_err();
            assert!(matches!(error, VaultError::UnsafeSessionId), "{id:?}");
            assert!(
                resolve(dir.path(), id, &["aa".to_owned()]).is_err(),
                "{id:?}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn the_vault_directory_is_0700_and_every_file_is_0600() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = TempDir::new().unwrap();
        append(
            dir.path(),
            "session-1",
            &[host_path("/Users/andy", "aa")],
            1_000,
            RetentionPolicy::default(),
        )
        .unwrap();

        let root = std::fs::metadata(dir.path().join("redactions")).unwrap();
        assert_eq!(root.permissions().mode() & 0o777, 0o700);
        let file = std::fs::metadata(dir.path().join("redactions/session-1.jsonl")).unwrap();
        assert_eq!(file.permissions().mode() & 0o777, 0o600);
    }

    #[test]
    fn a_partial_final_line_does_not_hide_the_entries_before_it() {
        let dir = TempDir::new().unwrap();
        append(
            dir.path(),
            "session-1",
            &[host_path("/Users/andy", "aa")],
            1_000,
            RetentionPolicy::default(),
        )
        .unwrap();
        // What a crash mid-write leaves behind.
        let path = dir.path().join("redactions/session-1.jsonl");
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(b"{\"digest\":\"bb\",\"byt").unwrap();

        let found = resolve(dir.path(), "session-1", &["aa".to_owned()]).unwrap();
        assert_eq!(found["aa"].plaintext, "/Users/andy");
    }

    #[test]
    fn the_retention_knob_reads_from_the_environment_and_survives_a_typo() {
        assert_eq!(
            RetentionPolicy::from_setting(Some("3")),
            RetentionPolicy::days(3)
        );
        assert!(!RetentionPolicy::from_setting(Some("0")).enabled());

        // A typo in one convenience knob must not stop a provider from running.
        assert_eq!(
            RetentionPolicy::from_setting(Some("fourteen")),
            RetentionPolicy::default()
        );
        assert_eq!(
            RetentionPolicy::from_setting(None),
            RetentionPolicy::default()
        );
    }
}
