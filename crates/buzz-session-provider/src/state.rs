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

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use buzz_core::coding_session_command::CodingSessionTarget;

/// On-disk format version for `state.json`.
pub const STATE_VERSION: u32 = 1;

const STATE_FILE: &str = "state.json";
const COMMANDS_FILE: &str = "commands.jsonl";

/// Host-local record of one session generation this provider owns.
///
/// `cwd` is deliberately here and nowhere else: it is machine-local execution
/// state, and signed content must never carry a filesystem path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRecord {
    /// Producer-minted session id (a UUID); half of the `cs-target`.
    pub session_id: String,
    /// Generation number. Always 1 in v1 — rebinding is a future feature.
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
    /// Requested model, or `None` to let the adapter decide.
    pub model: Option<String>,
    /// Operator-facing title, or `None`.
    pub title: Option<String>,
    /// Creation time, milliseconds since the Unix epoch.
    pub created_at_ms: i64,
    /// Next `event_seq` to hand out. Sequences start at 1.
    pub next_seq: u64,
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
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.dir.join(COMMANDS_FILE))?;
        writeln!(file, "{}", serde_json::to_string(&record)?)?;
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
        let mut file = File::create(&temp)?;
        file.write_all(body)?;
        file.sync_all()?;
    }
    fs::rename(&temp, path)
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
            model: None,
            title: None,
            created_at_ms: 1_700_000_000_000,
            next_seq: 1,
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
            store.insert_session(record("s1")).expect("insert");
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

    /// Records written before `sessionRef` existed must still load, and they
    /// load with no umbrella — the only claim those creates could have made.
    #[test]
    fn pre_session_ref_records_load_with_no_umbrella() {
        let mut value = serde_json::to_value(record("s1")).expect("serialize");
        value
            .as_object_mut()
            .expect("object")
            .remove("sessionRef")
            .expect("field present in current records");
        let loaded: SessionRecord = serde_json::from_value(value).expect("deserialize");
        assert!(loaded.session_ref.is_none());
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

    #[test]
    fn a_corrupt_snapshot_is_an_error_rather_than_a_silent_reset() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join(STATE_FILE), b"{ not json").expect("write");
        let error = StateStore::open(dir.path(), 3600).expect_err("should refuse to start");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }
}
