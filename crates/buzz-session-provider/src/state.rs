//! Durable provider state: watermarks, command dedupe, session records,
//! per-generation sequence counters, and the catalog revision.
//!
//! Two files, both under `BUZZ_CSP_STATE_DIR`:
//!
//! - `state.json` — the whole mutable snapshot, replaced atomically
//!   (write to a temp file, fsync, rename). A torn write can therefore never
//!   produce a half-updated sequence counter.
//! - `commands.jsonl` — append-only record of consumed `commandId`s. Append is
//!   the right shape here because the only question ever asked is "have I
//!   already acted on this?", and an append cannot lose earlier answers.
//!
//! # The sequence-counter invariant
//!
//! `event_seq` is allocated **and durably persisted before** the event that
//! uses it is published. That ordering makes gaps possible — a crash between
//! reserving 5 and publishing it burns 5 — and duplicates impossible. That is
//! the correct trade: the consumer's `cst-key` dedupe treats two different
//! items at the same sequence as a conflict it must surface, while a gap is
//! simply a sequence it never sees.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use buzz_core::coding_session_command::CodingSessionTarget;

use crate::session::BootstrapTransport;

/// On-disk format version for `state.json`.
pub const STATE_VERSION: u32 = 1;

const STATE_FILE: &str = "state.json";
const COMMANDS_FILE: &str = "commands.jsonl";
/// Single-instance lock file. See [`acquire_state_dir_lock`].
pub const LOCK_FILE: &str = "provider.lock";

/// Exclusive single-instance lock on one provider state directory.
///
/// Held for the owning process's lifetime; the OS releases it when the file
/// handle closes, including on a crash, so a stale lock can never outlive its
/// process. Dropping this value releases the lock.
#[derive(Debug)]
pub struct StateDirLock {
    // Held only for its advisory lock; the handle itself is never read again.
    _file: File,
}

/// Take the exclusive advisory lock that makes this process the *only*
/// provider allowed to touch `dir`.
///
/// Two providers sharing one state directory consume each command up to twice
/// (one create → several sessions) and interleave appends into the same
/// ledger files, physically corrupting them. This is the same
/// at-most-one-live-instance rule the managed-agents tier already enforces
/// (docs/remote-agents.md, invariant I4), applied to coding-session providers.
///
/// On conflict the error names the directory and, when readable, the pid the
/// current owner recorded — callers must fail fast and exit nonzero rather
/// than proceed unlocked. On success the caller's pid is recorded in the lock
/// file so a supervisor can identify (and take over from) an orphaned owner.
pub fn acquire_state_dir_lock(dir: &Path) -> io::Result<StateDirLock> {
    fs::create_dir_all(dir)?;
    restrict_directory(dir)?;
    let path = dir.join(LOCK_FILE);
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    restrict_new_file(&mut options);
    let mut file = options.open(&path)?;
    restrict_file(&path)?;
    if let Err(error) = file.try_lock() {
        let owner = fs::read_to_string(&path).unwrap_or_default();
        let owner = owner.trim();
        return Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            format!(
                "another buzz-session-provider instance already owns {} (owner pid: {}): {error}",
                dir.display(),
                if owner.is_empty() { "unknown" } else { owner },
            ),
        ));
    }
    // Recorded only while the lock is held, so the pid is always the owner's.
    file.set_len(0)?;
    file.write_all(format!("{}\n", std::process::id()).as_bytes())?;
    file.sync_all()?;
    Ok(StateDirLock { _file: file })
}

/// Host-local record of one session generation this provider owns.
///
/// `cwd` is deliberately here and nowhere else: it is machine-local execution
/// state, and signed content must never carry a filesystem path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRecord {
    /// Producer-minted session id (a UUID); half of the `cs-target`.
    pub session_id: String,
    /// Generation number. Starts at 1 and advances on each provider reattach.
    pub generation: u64,
    /// Channel every event for this session is published into.
    pub channel_id: Uuid,
    /// The create command that minted this session.
    pub command_id: String,
    /// The runtime instance that serves this session. Defaults to the claude
    /// ref for records written before the field existed.
    #[serde(default = "default_provider_instance_ref")]
    pub provider_instance_ref: String,
    /// Runtime slug behind the driver. Defaults for pre-field records.
    #[serde(default = "default_runtime_slug")]
    pub runtime: String,
    /// Driver slug minted into every `cs-target` for this generation. Persisted
    /// so the wire identity survives the descriptor disappearing (adapter
    /// uninstalled across a restart). Defaults to the claude driver for records
    /// written before the field existed — the only driver those can have used.
    #[serde(default = "default_driver")]
    pub driver: String,
    /// Working directory the agent runs in. Never serialized into an event.
    pub cwd: PathBuf,
    /// NIP-MP project coordinate, or `None` for a standalone session.
    pub project_ref: Option<String>,
    /// Repository coordinate, or `None`.
    pub repo_ref: Option<String>,
    /// Umbrella session reference from the create, echoed into every 44223
    /// for this session. Defaults to `None` for records written before the
    /// field existed — those creates could not have claimed an umbrella.
    #[serde(default)]
    pub session_ref: Option<String>,
    /// Explicit genesis event id for authority-aware sessions.
    #[serde(default)]
    pub genesis_ref: Option<String>,
    /// Founder pubkey resolved from genesis, or the locally witnessed create
    /// signer for legacy sessions. `None` is retained for pre-field records.
    #[serde(default)]
    pub founder_pubkey: Option<String>,
    /// Operator pubkeys granted steering authority by this session's accepted
    /// authority chain (kind 44228 `grant-operator` transitions, each applied
    /// only after its relay-signed kind 40099 acceptance receipt and the
    /// referenced transition event were verified — see `crate::authority`).
    ///
    /// A verified local cache in the R21 sense: every entry was witnessed and
    /// checked by this provider at the moment it was applied, and the set is
    /// re-extended by backfill on load. Meaningful only when `genesis_ref` is
    /// set — enforcement never consults it for legacy sessions. Defaults empty
    /// for records written before the field existed.
    #[serde(default)]
    pub granted_operators: BTreeSet<String>,
    /// Pubkeys holding a live `grant-viewer` — read-only session shares.
    /// Never consulted for steering; tracked for diagnostics and so a later
    /// `revoke` of a viewer folds cleanly. Defaults empty for pre-field
    /// records.
    #[serde(default)]
    pub granted_viewers: BTreeSet<String>,
    /// Highest accepted authority-chain `seq` whose grant has been applied to
    /// `granted_operators`. `0` means no transition has been applied. Grants
    /// apply strictly contiguously (`seq == authority_seq + 1`); a gap
    /// triggers a backfill rather than a guess. Defaults to 0 for pre-field
    /// records.
    #[serde(default)]
    pub authority_seq: u32,
    /// Requested model, or `None` to let the adapter decide.
    pub model: Option<String>,
    /// Opaque ACP session id used only to reattach this host's adapter.
    /// Never published or passed through the adapter environment.
    #[serde(default)]
    pub resume_cursor: Option<String>,
    /// Operator-facing title, or `None`.
    pub title: Option<String>,
    /// Creation time, milliseconds since the Unix epoch.
    pub created_at_ms: i64,
    /// Next `event_seq` to hand out. Sequences start at 1.
    pub next_seq: u64,
    /// How this generation's rehydration continuity bootstrap was delivered, or
    /// `None` when the open needed no bootstrap. Host-local diagnostic: never
    /// published, and defaults to `None` for records written before the field
    /// existed.
    #[serde(default)]
    pub bootstrap_transport: Option<BootstrapTransport>,
    /// The turn currently believed to be in flight, if any.
    pub open_turn: Option<OpenTurn>,
    /// Whether this generation has been retired (no further turns accepted).
    pub closed: bool,
}

fn default_provider_instance_ref() -> String {
    crate::config::PROVIDER_INSTANCE_REF.to_owned()
}

fn default_runtime_slug() -> String {
    crate::config::RUNTIME.to_owned()
}

fn default_driver() -> String {
    crate::config::DRIVER.to_owned()
}

impl SessionRecord {
    /// The wire target naming this generation, minted with the persisted driver.
    pub fn target(&self, instance_id: &str) -> CodingSessionTarget {
        CodingSessionTarget {
            driver: self.driver.clone(),
            instance_id: instance_id.to_owned(),
            session_id: self.session_id.clone(),
            generation: self.generation,
        }
    }
}

/// A turn believed to be in flight, persisted so a crash can be repaired.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenTurn {
    /// Producer-minted turn id carried on every item of the turn.
    pub turn_id: String,
    /// The command that opened the turn, when one did.
    pub command_id: Option<String>,
    /// Turn start, milliseconds since the Unix epoch.
    pub started_at_ms: i64,
}

/// Persisted catalog advertisement state.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogState {
    /// Monotonic revision last advertised. `0` means nothing published yet.
    pub revision: u64,
    /// SHA-256 of the canonical bytes behind `revision`.
    pub content_digest: Option<String>,
    /// Channels the current revision has already been advertised into.
    #[serde(default)]
    pub advertised_channels: Vec<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Snapshot {
    version: u32,
    #[serde(default)]
    watermarks: BTreeMap<Uuid, u64>,
    #[serde(default)]
    sessions: BTreeMap<String, SessionRecord>,
    #[serde(default)]
    catalog: CatalogState,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            version: STATE_VERSION,
            watermarks: BTreeMap::new(),
            sessions: BTreeMap::new(),
            catalog: CatalogState::default(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommandRecord {
    command_id: String,
    at: u64,
}

/// Durable provider state rooted at one directory.
#[derive(Debug)]
pub struct StateStore {
    dir: PathBuf,
    snapshot: Snapshot,
    commands: HashSet<String>,
}

impl StateStore {
    /// Open (creating if absent) the state directory and load everything in it.
    ///
    /// `command_retention_secs` drops consumed command ids older than the
    /// freshness horizon: a command that old is ignored on its own merits, so
    /// remembering it buys nothing and the ledger would grow without bound.
    pub fn open(dir: &Path, command_retention_secs: u64) -> io::Result<Self> {
        fs::create_dir_all(dir)?;
        restrict_directory(dir)?;
        for file in [STATE_FILE, COMMANDS_FILE] {
            restrict_file_if_present(&dir.join(file))?;
        }
        let snapshot = load_snapshot(&dir.join(STATE_FILE))?;
        let mut store = Self {
            dir: dir.to_path_buf(),
            snapshot,
            commands: HashSet::new(),
        };
        store.load_commands(command_retention_secs)?;
        Ok(store)
    }

    /// Newest `created_at` already consumed in a channel, if any.
    pub fn watermark(&self, channel_id: Uuid) -> Option<u64> {
        self.snapshot.watermarks.get(&channel_id).copied()
    }

    /// Advance a channel watermark. Never moves backwards.
    pub fn record_watermark(&mut self, channel_id: Uuid, created_at: u64) -> io::Result<()> {
        let entry = self.snapshot.watermarks.entry(channel_id).or_insert(0);
        if created_at <= *entry {
            return Ok(());
        }
        *entry = created_at;
        self.persist()
    }

    /// Whether this command has already been acted on.
    pub fn is_command_consumed(&self, command_id: &str) -> bool {
        self.commands.contains(command_id)
    }

    /// Record a command as consumed, durably, before any side effect runs.
    pub fn consume_command(&mut self, command_id: &str, at: u64) -> io::Result<()> {
        if !self.commands.insert(command_id.to_owned()) {
            return Ok(());
        }
        let record = CommandRecord {
            command_id: command_id.to_owned(),
            at,
        };
        let mut options = OpenOptions::new();
        options.create(true).append(true);
        restrict_new_file(&mut options);
        let path = self.dir.join(COMMANDS_FILE);
        let mut file = options.open(&path)?;
        restrict_file(&path)?;
        // The whole line — payload and newline — goes down in a single
        // `write` call on an O_APPEND handle, so even a second writer (a bug
        // the state-dir lock exists to prevent) could not tear it in half.
        let mut line = serde_json::to_string(&record)?;
        line.push('\n');
        file.write_all(line.as_bytes())?;
        file.sync_all()
    }

    /// Every session record this provider has ever minted and not pruned.
    pub fn sessions(&self) -> impl Iterator<Item = &SessionRecord> {
        self.snapshot.sessions.values()
    }

    /// Number of sessions still accepting turns.
    pub fn live_session_count(&self) -> usize {
        self.snapshot
            .sessions
            .values()
            .filter(|session| !session.closed)
            .count()
    }

    /// Look up one session by its producer-minted id.
    pub fn session(&self, session_id: &str) -> Option<&SessionRecord> {
        self.snapshot.sessions.get(session_id)
    }

    /// Insert a freshly minted session record.
    pub fn insert_session(&mut self, record: SessionRecord) -> io::Result<()> {
        self.snapshot
            .sessions
            .insert(record.session_id.clone(), record);
        self.persist()
    }

    /// Mutate a session record in place and persist the result.
    ///
    /// Returns `false` when no such session exists, so callers can distinguish
    /// "updated" from "the session went away underneath me".
    pub fn update_session(
        &mut self,
        session_id: &str,
        mutate: impl FnOnce(&mut SessionRecord),
    ) -> io::Result<bool> {
        match self.snapshot.sessions.get_mut(session_id) {
            None => Ok(false),
            Some(record) => {
                mutate(record);
                self.persist()?;
                Ok(true)
            }
        }
    }

    /// Reserve the next `event_seq` for a generation and persist it *before*
    /// returning, so a crash can only ever burn the number, never reuse it.
    pub fn allocate_seq(&mut self, session_id: &str) -> io::Result<Option<u64>> {
        let Some(record) = self.snapshot.sessions.get_mut(session_id) else {
            return Ok(None);
        };
        let seq = record.next_seq.max(1);
        record.next_seq = seq + 1;
        self.persist()?;
        Ok(Some(seq))
    }

    /// Current catalog advertisement state.
    pub fn catalog(&self) -> &CatalogState {
        &self.snapshot.catalog
    }

    /// Replace the catalog advertisement state.
    pub fn set_catalog(&mut self, catalog: CatalogState) -> io::Result<()> {
        self.snapshot.catalog = catalog;
        self.persist()
    }

    fn load_commands(&mut self, retention_secs: u64) -> io::Result<()> {
        let path = self.dir.join(COMMANDS_FILE);
        let file = match File::open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        let cutoff = now_secs().saturating_sub(retention_secs);
        let mut kept: Vec<CommandRecord> = Vec::new();
        let mut dropped = false;
        for line in BufReader::new(file).lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<CommandRecord>(&line) {
                Ok(record) if record.at >= cutoff => {
                    if self.commands.insert(record.command_id.clone()) {
                        kept.push(record);
                    } else {
                        dropped = true;
                    }
                }
                Ok(_) => dropped = true,
                Err(error) => {
                    tracing::warn!(target: "csp::state", "dropping unreadable command ledger line: {error}");
                    dropped = true;
                }
            }
        }
        if dropped {
            let mut body = String::new();
            for record in &kept {
                body.push_str(&serde_json::to_string(record)?);
                body.push('\n');
            }
            atomic_write(&path, body.as_bytes())?;
        }
        Ok(())
    }

    fn persist(&self) -> io::Result<()> {
        let body = serde_json::to_vec_pretty(&self.snapshot)?;
        atomic_write(&self.dir.join(STATE_FILE), &body)
    }
}

fn load_snapshot(path: &Path) -> io::Result<Snapshot> {
    let body = match fs::read(path) {
        Ok(body) => body,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Snapshot::default()),
        Err(error) => return Err(error),
    };
    match serde_json::from_slice::<Snapshot>(&body) {
        Ok(snapshot) if snapshot.version == STATE_VERSION => Ok(snapshot),
        Ok(snapshot) => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "state.json version {} is not supported (expected {STATE_VERSION})",
                snapshot.version
            ),
        )),
        Err(error) => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("state.json is unreadable: {error}"),
        )),
    }
}

/// Replace a file's contents atomically: write a sibling temp file, fsync it,
/// then rename over the target. A crash leaves either the old file or the new
/// one, never a truncated mixture.
pub(crate) fn atomic_write(path: &Path, body: &[u8]) -> io::Result<()> {
    let parent = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let temp = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("state"),
        Uuid::new_v4()
    ));
    {
        let mut options = OpenOptions::new();
        options.create_new(true).write(true);
        restrict_new_file(&mut options);
        let mut file = options.open(&temp)?;
        file.write_all(body)?;
        file.sync_all()?;
    }
    fs::rename(&temp, path)?;
    restrict_file(path)
}

#[cfg(unix)]
fn restrict_new_file(options: &mut OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt;
    options.mode(0o600);
}

#[cfg(not(unix))]
fn restrict_new_file(_options: &mut OpenOptions) {}

#[cfg(unix)]
fn restrict_directory(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn restrict_directory(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn restrict_file(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

fn restrict_file_if_present(path: &Path) -> io::Result<()> {
    match restrict_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(not(unix))]
fn restrict_file(_path: &Path) -> io::Result<()> {
    Ok(())
}

/// Seconds since the Unix epoch.
pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}

/// Milliseconds since the Unix epoch.
pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Index live session records by target for generation fencing.
pub fn index_by_target(
    sessions: impl Iterator<Item = SessionRecord>,
) -> HashMap<String, SessionRecord> {
    sessions
        .map(|record| (record.session_id.clone(), record))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(session_id: &str) -> SessionRecord {
        SessionRecord {
            session_id: session_id.to_owned(),
            generation: 1,
            channel_id: Uuid::nil(),
            command_id: format!("create-{session_id}"),
            provider_instance_ref: "claude-primary".into(),
            runtime: "claude".into(),
            driver: "claude-agent-acp".into(),
            cwd: PathBuf::from("/Users/operator/checkout"),
            project_ref: None,
            repo_ref: None,
            session_ref: None,
            genesis_ref: None,
            founder_pubkey: Some("ab".repeat(32)),
            granted_operators: BTreeSet::new(),
            granted_viewers: BTreeSet::new(),
            authority_seq: 0,
            model: None,
            resume_cursor: None,
            title: None,
            created_at_ms: 1_700_000_000_000,
            next_seq: 1,
            bootstrap_transport: None,
            open_turn: None,
            closed: false,
        }
    }

    #[test]
    fn round_trips_watermarks_sessions_and_catalog_across_a_restart() {
        let dir = tempfile::tempdir().expect("tempdir");
        let channel = Uuid::new_v4();
        {
            let mut store = StateStore::open(dir.path(), 3600).expect("open");
            store.record_watermark(channel, 100).expect("watermark");
            store.record_watermark(channel, 50).expect("watermark");
            let mut session = record("s1");
            session.resume_cursor = Some("host-private-acp-session".into());
            store.insert_session(session).expect("insert");
            store
                .set_catalog(CatalogState {
                    revision: 4,
                    content_digest: Some("abc".into()),
                    advertised_channels: vec![channel],
                })
                .expect("catalog");
        }
        let store = StateStore::open(dir.path(), 3600).expect("reopen");
        assert_eq!(store.watermark(channel), Some(100));
        assert_eq!(
            store.session("s1").map(|s| s.cwd.clone()),
            Some("/Users/operator/checkout".into())
        );
        assert_eq!(store.catalog().revision, 4);
        assert_eq!(store.catalog().advertised_channels, vec![channel]);
        assert_eq!(
            store
                .session("s1")
                .and_then(|session| session.resume_cursor.as_deref()),
            Some("host-private-acp-session")
        );
    }

    #[cfg(unix)]
    #[test]
    fn private_state_uses_owner_only_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let mut store = StateStore::open(dir.path(), 3600).expect("open");
        store.insert_session(record("s1")).expect("insert");
        store
            .consume_command("create-s1", now_secs())
            .expect("consume");

        assert_eq!(
            fs::metadata(dir.path())
                .expect("directory metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        for file in [STATE_FILE, COMMANDS_FILE] {
            assert_eq!(
                fs::metadata(dir.path().join(file))
                    .expect("file metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o600,
                "{file} must not expose the ACP resume cursor or command ledger"
            );
        }
    }

    /// The whole point of allocate-before-publish: a process that dies between
    /// reserving a sequence and publishing it must resume *past* the reserved
    /// number. A gap is legal; handing 2 out twice is not.
    #[test]
    fn a_crash_after_allocating_burns_the_sequence_rather_than_reusing_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let allocated = {
            let mut store = StateStore::open(dir.path(), 3600).expect("open");
            store.insert_session(record("s1")).expect("insert");
            let first = store
                .allocate_seq("s1")
                .expect("allocate")
                .expect("session");
            let second = store
                .allocate_seq("s1")
                .expect("allocate")
                .expect("session");
            assert_eq!((first, second), (1, 2));
            // Simulate the crash here: `second` was persisted but never published.
            second
        };

        let mut store = StateStore::open(dir.path(), 3600).expect("reopen");
        let after_restart = store
            .allocate_seq("s1")
            .expect("allocate")
            .expect("session");
        assert_eq!(
            after_restart,
            allocated + 1,
            "restart must resume past the reserved sequence, leaving a gap"
        );
    }

    #[test]
    fn consumed_commands_survive_restart_and_expire_past_retention() {
        let dir = tempfile::tempdir().expect("tempdir");
        let now = now_secs();
        {
            let mut store = StateStore::open(dir.path(), 3600).expect("open");
            store.consume_command("fresh", now).expect("consume");
            store
                .consume_command("stale", now.saturating_sub(10_000))
                .expect("consume");
            // Duplicate consumption is a no-op, not a second ledger line.
            store.consume_command("fresh", now).expect("consume");
        }
        let store = StateStore::open(dir.path(), 3600).expect("reopen");
        assert!(store.is_command_consumed("fresh"));
        assert!(
            !store.is_command_consumed("stale"),
            "entries older than the horizon are pruned, since they are ignored anyway"
        );

        let ledger = fs::read_to_string(dir.path().join(COMMANDS_FILE)).expect("read ledger");
        assert_eq!(
            ledger.lines().filter(|line| !line.is_empty()).count(),
            1,
            "pruning rewrites the ledger rather than growing it"
        );
    }

    #[test]
    fn live_session_count_excludes_closed_generations() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut store = StateStore::open(dir.path(), 3600).expect("open");
        store.insert_session(record("s1")).expect("insert");
        let mut closed = record("s2");
        closed.closed = true;
        store.insert_session(closed).expect("insert");
        assert_eq!(store.live_session_count(), 1);
        assert_eq!(store.sessions().count(), 2);
    }

    #[test]
    fn updating_a_missing_session_reports_rather_than_creating_one() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut store = StateStore::open(dir.path(), 3600).expect("open");
        assert!(!store
            .update_session("ghost", |record| record.closed = true)
            .expect("update"));
        assert!(store.allocate_seq("ghost").expect("allocate").is_none());
    }

    /// Records written before the umbrella and authority fields existed still
    /// load as ungoverned legacy executions; no authority is inferred.
    #[test]
    fn pre_authority_records_load_ungoverned_without_inference() {
        let mut value = serde_json::to_value(record("s1")).expect("serialize");
        let object = value.as_object_mut().expect("object");
        for field in [
            "sessionRef",
            "genesisRef",
            "founderPubkey",
            "grantedOperators",
            "authoritySeq",
        ] {
            object
                .remove(field)
                .expect("field present in current records");
        }
        let loaded: SessionRecord = serde_json::from_value(value).expect("deserialize");
        assert!(loaded.session_ref.is_none());
        assert!(loaded.genesis_ref.is_none());
        assert!(loaded.founder_pubkey.is_none());
        assert!(loaded.granted_operators.is_empty());
        assert_eq!(loaded.authority_seq, 0);
    }

    /// Records written before the runtime fields existed must still load, and
    /// they load as claude — the only runtime that could have minted them.
    #[test]
    fn pre_runtime_session_records_default_to_claude() {
        let mut value = serde_json::to_value(record("s1")).expect("serialize");
        let object = value.as_object_mut().expect("object");
        object.remove("providerInstanceRef");
        object.remove("runtime");
        object.remove("driver");
        let loaded: SessionRecord = serde_json::from_value(value).expect("deserialize");
        assert_eq!(loaded.provider_instance_ref, "claude-primary");
        assert_eq!(loaded.runtime, "claude");
        assert_eq!(loaded.driver, "claude-agent-acp");
    }

    /// The bootstrap transport is a diagnostic added after `STATE_VERSION` was
    /// pinned, so a record written without it must still load — as "no
    /// bootstrap recorded", which is exactly what those records mean.
    #[test]
    fn pre_bootstrap_transport_records_load_without_one() {
        let mut written = record("s1");
        written.bootstrap_transport = Some(BootstrapTransport::SystemPrompt);
        let mut value = serde_json::to_value(&written).expect("serialize");
        let object = value.as_object_mut().expect("object");
        assert_eq!(
            object.remove("bootstrapTransport"),
            Some(serde_json::json!("systemPrompt")),
            "the field is persisted under its camelCase name"
        );
        let loaded: SessionRecord = serde_json::from_value(value).expect("deserialize");
        assert_eq!(loaded.bootstrap_transport, None);
    }

    /// The persisted driver — not the live descriptor table — names the wire
    /// target, so a session's identity cannot mutate mid-life when its
    /// runtime's adapter is uninstalled across a restart.
    #[test]
    fn the_target_is_minted_from_the_persisted_driver() {
        let mut codex = record("s1");
        codex.driver = "codex-acp".into();
        let target = codex.target("instance-1");
        assert_eq!(target.driver, "codex-acp");
        assert_eq!(target.instance_id, "instance-1");
        assert_eq!(target.session_id, "s1");
        assert_eq!(target.generation, 1);
    }

    /// One state directory, one provider: the second lock attempt must fail
    /// while the first is held, and succeed once it is released. (flock
    /// conflicts apply across open file descriptions, so two handles in one
    /// process exercise the same contention two processes would.)
    #[test]
    fn the_state_dir_lock_admits_exactly_one_holder() {
        let dir = tempfile::tempdir().expect("tempdir");
        let first = acquire_state_dir_lock(dir.path()).expect("first lock");
        let second = acquire_state_dir_lock(dir.path());
        let error = second.expect_err("a second holder must be refused");
        assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
        assert!(
            error.to_string().contains(&std::process::id().to_string()),
            "the refusal names the owning pid: {error}"
        );
        drop(first);
        acquire_state_dir_lock(dir.path()).expect("lock is free after release");
    }

    /// The lock file records the owner's pid so a supervisor can identify an
    /// orphaned provider to take over from.
    #[test]
    fn the_lock_file_records_the_owner_pid() {
        let dir = tempfile::tempdir().expect("tempdir");
        let _lock = acquire_state_dir_lock(dir.path()).expect("lock");
        let recorded = fs::read_to_string(dir.path().join(LOCK_FILE)).expect("read lock file");
        assert_eq!(recorded.trim(), std::process::id().to_string());
    }

    #[test]
    fn a_corrupt_snapshot_is_an_error_rather_than_a_silent_reset() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join(STATE_FILE), b"{ not json").expect("write");
        let error = StateStore::open(dir.path(), 3600).expect_err("should refuse to start");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }
}
