//! `bee sessions worktree` — what a session's worktrees hold, and what may go.
//!
//! The host that cuts a seat's worktree also records it. This reads that
//! record, gathers the facts the record cannot hold (is the session closed, is
//! the branch on the relay, how many uncommitted files are in the directory,
//! how much of it is rebuildable build output) and asks
//! [`beekeeper_core::worktree_lifecycle::classify_seat_worktree`] what they mean.
//!
//! Three things it will not do:
//!
//! * **Guess a path from a slug.** A tree the host never recorded is listed as
//!   `unrecorded` and removed by nothing here. The 65 trees that predate the
//!   record on the machine this was written for stay exactly where they are.
//! * **Remove uncommitted work.** `prune` refuses any tree that is not
//!   `prunable`, `git worktree remove` runs without `--force`, and a `held`
//!   row exits non-zero rather than quietly doing nothing.
//! * **Say more than it checked.** `tipOnRelay` comes from a relay-signed
//!   kind 30618, which is parameterized-replaceable: it says where a ref
//!   stands **now** and is never a push history. Every output that carries it
//!   carries that sentence too.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;
use serde_json::{json, Map, Value};

use beekeeper_core::kind::KIND_CODING_SESSION_CLOSURE;

/// Kind 5 — the NIP-09 deletion event a whole-session delete publishes.
const KIND_DELETION: u32 = 5;
use beekeeper_core::worktree_lifecycle::{
    build_output_reclaimable, classify_seat_worktree, render_reclaimable_bytes,
    SeatWorktreeDisposition, SeatWorktreeFacts,
};

use crate::commands::sandbox;

use crate::client::BeekeeperClient;
use crate::error::CliError;

/// Kind 30618 — the relay-signed NIP-34 repository state announcement.
const KIND_REPO_STATE: u32 = 30618;

/// The one sentence every surface repeats about `tipOnRelay`.
pub const TIP_ON_RELAY_LIMIT: &str =
    "kind 30618 is parameterized-replaceable: it says where a ref stands now, never a push history";

/// One worktree the desktop host recorded cutting.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordedSeatWorktree {
    /// Absolute path of the worktree directory.
    pub path: PathBuf,
    /// Branch it has checked out.
    pub branch: String,
    /// Repository it belongs to.
    pub repo_root: PathBuf,
    /// When the host cut it.
    pub created_at: String,
}

/// Oldest on-disk schema version this command still reads. Below it the file
/// predates any version tag this command understands.
const MIN_RECORD_VERSION: u32 = 1;

/// Newest on-disk schema version this command understands.
/// Mirrors `desktop/src-tauri/src/coding_sessions/workdir_store.rs`'s
/// `WORKDIR_STORE_VERSION` — a v1 file carries no `worktrees` map at all and
/// reads as empty, which is the honest answer for trees cut before the record
/// existed.
const MAX_RECORD_VERSION: u32 = 2;

/// The slice of `coding-session-workdirs.json` this command reads.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordedWorktreeStore {
    /// On-disk schema version. 1 predates the record and carries no map.
    #[serde(default)]
    pub version: u32,
    /// Keyed by `<sessionRef>/<seatLabel>`.
    #[serde(default)]
    pub worktrees: BTreeMap<String, RecordedSeatWorktree>,
}

/// Where the desktop host keeps its record, newest install identity first.
///
/// A closed list rather than a glob: the many `xyz.block.buzz.app.dev*`
/// directories on a long-lived machine are dead pre-rename installs, and
/// reading one of those would answer with somebody else's history.
const STORE_DIRS: &[&str] = &["io.agiterra.beekeeper.app.dev", "io.agiterra.beekeeper.app"];

/// Resolve the host record's path, or say where it looked.
/// A `git` invocation addressed at `cwd` and nothing else.
///
/// Every caller here names the repository by path, so the process environment
/// must not redirect it. Git exports `GIT_DIR` (and friends) to hooks, and the
/// pre-push gate runs this crate's tests inside one: with those inherited, a
/// `git init` in a temp dir wrote to the *pushing* repository's config and
/// raced its lock — "could not lock config file …/.git/config: File exists" —
/// and every worktree test failed for anyone pushing from a bare clone.
///
/// `pub(crate)` because `bee packs init` seeds a repository in a temp
/// directory the same way and must not be redirected either — one helper, one
/// list, so the two cannot drift.
pub(crate) fn git_command(cwd: &Path) -> Command {
    let mut command = Command::new("git");
    command.current_dir(cwd);
    for var in GIT_REPO_SELECTION_VARS {
        command.env_remove(var);
    }
    command
}

/// The variables through which an ambient environment can pick git's
/// repository for it. The same list the desktop's team-readiness probe clears
/// (`team_readiness_git.rs`), for the same reason.
const GIT_REPO_SELECTION_VARS: [&str; 7] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_COMMON_DIR",
    "GIT_NAMESPACE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
];

pub fn default_store_path() -> Result<PathBuf, CliError> {
    let home = std::env::var("HOME").map_err(|_| {
        CliError::Usage("HOME is not set, so the host record cannot be found".into())
    })?;
    let base = Path::new(&home).join("Library/Application Support");
    for dir in STORE_DIRS {
        let candidate = base.join(dir).join("coding-session-workdirs.json");
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(CliError::NotFound(format!(
        "no host worktree record found under {} (looked in {})",
        base.display(),
        STORE_DIRS.join(", ")
    )))
}

/// Read the host record. A missing file is an empty record, not an error, when
/// the caller named the path itself.
///
/// Refuses a version outside `[MIN_RECORD_VERSION, MAX_RECORD_VERSION]` rather
/// than trusting a shape it does not understand: a too-old file predates any
/// version tag this build reads, and a too-new one may carry a shape this
/// build cannot classify correctly.
pub fn load_store(path: &Path) -> Result<RecordedWorktreeStore, CliError> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| CliError::Other(format!("failed to read {}: {error}", path.display())))?;
    let store: RecordedWorktreeStore = serde_json::from_str(&text)
        .map_err(|error| CliError::Other(format!("failed to parse {}: {error}", path.display())))?;
    if store.version < MIN_RECORD_VERSION || store.version > MAX_RECORD_VERSION {
        return Err(CliError::Other(format!(
            "{} has unsupported schema version {} (this build reads {}..={})",
            path.display(),
            store.version,
            MIN_RECORD_VERSION,
            MAX_RECORD_VERSION
        )));
    }
    Ok(store)
}

/// Run a git command in `cwd`, answering `None` when git itself failed.
fn git(args: &[&str], cwd: &Path) -> Option<String> {
    let output = git_command(cwd).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Lines of `git status --porcelain`, which exclude ignored paths.
///
/// A directory git cannot read counts as one dirty file: an unreadable tree is
/// not a clean one, and the difference decides whether something is deleted.
pub fn count_dirty_files(path: &Path) -> u32 {
    match git(&["status", "--porcelain"], path) {
        Some(output) => output
            .lines()
            .filter(|line| !line.trim().is_empty())
            .count()
            .min(u32::MAX as usize) as u32,
        None => 1,
    }
}

/// Whether nothing may ever remove this tree.
///
/// Structural: the repository's own checkout, an ancestor of it, the process's
/// own working directory, or any path outside `<repo_root>.worktrees/`.
pub fn is_protected(repo_root: &Path, path: &Path) -> bool {
    if path == repo_root || repo_root.starts_with(path) {
        return true;
    }
    // Shared with the desktop host's record and prune guards, so `bee` and the
    // app can never disagree about which directories are manageable.
    let inside = beekeeper_core::worktree_placement::is_managed_worktree_path(repo_root, path, &[]);
    if !inside {
        return true;
    }
    match std::env::current_dir() {
        Ok(cwd) => cwd.starts_with(path),
        Err(_) => true,
    }
}

/// The repository id the relay files a worktree's repo under.
///
/// Read from the origin URL's last path segment, `.git` stripped. The remote
/// name is read from `git remote`, never hard-coded — two pre-push guards died
/// of hard-coding one the day the names moved.
pub fn repo_id_of(repo_root: &Path) -> Option<String> {
    let remotes = git(&["remote"], repo_root)?;
    let name = remotes
        .lines()
        .map(str::trim)
        .find(|line| *line == "origin")
        .or_else(|| remotes.lines().map(str::trim).find(|line| !line.is_empty()))?;
    let url = git(&["remote", "get-url", name], repo_root)?;
    let url = url.trim().trim_end_matches('/');
    let last = url.rsplit('/').next()?;
    Some(last.trim_end_matches(".git").to_string())
}

/// Every commit id a relay-signed kind 30618 currently names for `repo_id`.
pub async fn relay_ref_oids(
    client: &BeekeeperClient,
    repo_id: &str,
) -> Result<BTreeSet<String>, CliError> {
    let events = client
        .query_all(json!({ "kinds": [KIND_REPO_STATE], "#d": [repo_id] }))
        .await?;
    let mut oids = BTreeSet::new();
    for event in &events {
        let Some(tags) = event.get("tags").and_then(Value::as_array) else {
            continue;
        };
        for tag in tags {
            let Some(parts) = tag.as_array() else {
                continue;
            };
            let (Some(name), Some(value)) = (
                parts.first().and_then(Value::as_str),
                parts.get(1).and_then(Value::as_str),
            ) else {
                continue;
            };
            if name.starts_with("refs/")
                && value.len() == 40
                && value.chars().all(|c| c.is_ascii_hexdigit())
            {
                oids.insert(value.to_ascii_lowercase());
            }
        }
    }
    Ok(oids)
}

/// Whether the worktree's tip is named by, or an ancestor of, one of `oids`.
pub fn tip_on_relay(path: &Path, oids: &BTreeSet<String>) -> Option<bool> {
    let tip = git(&["rev-parse", "HEAD"], path)?
        .trim()
        .to_ascii_lowercase();
    if oids.is_empty() {
        return Some(false);
    }
    if oids.contains(&tip) {
        return Some(true);
    }
    Some(oids.iter().any(|oid| {
        git_command(path)
            .args(["merge-base", "--is-ancestor", &tip, oid])
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }))
}

/// How a session ended, as far as the relay can still say.
///
/// Two independent ways for a session's work to be over, and the second one
/// erases the evidence for the first: a whole-session deletion removes the
/// 44230 closures along with the genesis, so after one there is no closure
/// left to fold. Ledger 135(f) is exactly that — `bee sessions worktree
/// status` answered "the session is not closed, so nothing is removed" for
/// two trees whose session had been deleted an hour earlier, and no reaper
/// ever ran over them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionSettlement {
    /// A 44230 revision folds the session to `closed` or `archived`.
    pub settled: bool,
    /// An accepted whole-session deletion named this session.
    pub deleted: bool,
    /// Seconds since whichever of the two is newer.
    pub settled_for_secs: Option<u64>,
}

/// Whether an accepted whole-session deletion names this session, and when.
///
/// The deletion event is a kind:5 carrying `["d", <sessionRef>]` — a
/// single-letter tag precisely so this query is possible; the tombstone is
/// the only thing left on the relay once a session is gone, and without an
/// indexable marker on it "was this deleted?" has no answer at all.
///
/// A deletion published by a build older than this tag is invisible here, and
/// its trees keep reading `not-settled`. That is the honest answer: nothing
/// on the relay attributes such a tombstone to a session.
pub async fn session_deletion(
    client: &BeekeeperClient,
    session_ref: &str,
    now_secs: i64,
) -> Result<Option<u64>, CliError> {
    let events = client
        .query_all(json!({ "kinds": [KIND_DELETION], "#d": [session_ref] }))
        .await?;
    let newest = events
        .iter()
        .filter_map(|event| event.get("created_at").and_then(Value::as_i64))
        .max();
    Ok(newest
        .map(|created_at| u64::try_from(now_secs.saturating_sub(created_at)).unwrap_or(u64::MAX)))
}

/// Whether a 44230 revision settles this session, and how long ago.
pub async fn session_settlement(
    client: &BeekeeperClient,
    session_ref: &str,
    now_secs: i64,
) -> Result<(bool, Option<u64>), CliError> {
    let events = client
        .query_all(json!({ "kinds": [KIND_CODING_SESSION_CLOSURE], "#d": [session_ref] }))
        .await?;
    let mut newest: Option<(i64, String, String)> = None;
    for event in &events {
        let created_at = event.get("created_at").and_then(Value::as_i64).unwrap_or(0);
        let id = event
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let action = event
            .get("content")
            .and_then(Value::as_str)
            .and_then(|content| serde_json::from_str::<Value>(content).ok())
            .and_then(|parsed| {
                parsed
                    .get("action")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            });
        let Some(action) = action else { continue };
        let candidate = (created_at, id, action);
        newest = match newest {
            Some(previous)
                if (previous.0, previous.1.clone()) >= (candidate.0, candidate.1.clone()) =>
            {
                Some(previous)
            }
            _ => Some(candidate),
        };
    }
    let Some((created_at, _, action)) = newest else {
        return Ok((false, None));
    };
    if action != "closed" && action != "archived" {
        return Ok((false, None));
    }
    let elapsed = now_secs.saturating_sub(created_at);
    Ok((true, u64::try_from(elapsed).ok()))
}

/// Both ends of a session, folded into one answer.
///
/// A closure and a deletion can both exist — closed first, deleted later —
/// and the grace window runs from whichever is newer, because that is the
/// moment the work last stopped being somebody's live work.
pub async fn session_end(
    client: &BeekeeperClient,
    session_ref: &str,
    now_secs: i64,
) -> Result<SessionSettlement, CliError> {
    let (settled, closed_for_secs) = session_settlement(client, session_ref, now_secs).await?;
    let deleted_for_secs = session_deletion(client, session_ref, now_secs).await?;
    // The *smaller* elapsed time is the newer event.
    let settled_for_secs = match (closed_for_secs, deleted_for_secs) {
        (Some(closed), Some(deleted)) => Some(closed.min(deleted)),
        (value, None) | (None, value) => value,
    };
    Ok(SessionSettlement {
        settled,
        deleted: deleted_for_secs.is_some(),
        settled_for_secs,
    })
}

/// Everything one row needs, already decided.
pub struct WorktreeRow {
    /// `<sessionRef>/<seatLabel>`.
    pub key: String,
    /// The record this row describes.
    pub record: RecordedSeatWorktree,
    /// What may be done with it.
    pub disposition: SeatWorktreeDisposition,
    /// `git status --porcelain` lines.
    pub dirty_files: u32,
    /// Rebuildable bytes, when measurable.
    pub reclaimable: Option<u64>,
    /// How that size should be read. A cloned build directory measures its
    /// full logical size while freeing it releases only what this sandbox
    /// wrote, so the number alone would overstate what reclaiming buys.
    pub reclaimable_label: String,
    /// Whether that build output may go right now.
    pub reclaimable_now: bool,
    /// Whether the relay's ref state was established at all.
    pub tip_known: bool,
    /// Whether the directory is still there.
    pub exists: bool,
    /// Whether an accepted whole-session deletion named this session.
    pub session_deleted: bool,
}

impl WorktreeRow {
    /// The row as the JSON every read of this command prints.
    pub fn to_json(&self) -> Value {
        let mut object = Map::new();
        object.insert("key".into(), json!(self.key));
        object.insert(
            "path".into(),
            json!(self.record.path.to_string_lossy().into_owned()),
        );
        object.insert("branch".into(), json!(self.record.branch));
        object.insert(
            "repoRoot".into(),
            json!(self.record.repo_root.to_string_lossy().into_owned()),
        );
        object.insert("createdAt".into(), json!(self.record.created_at));
        object.insert("disposition".into(), json!(self.disposition.token()));
        object.insert("dirtyFiles".into(), json!(self.dirty_files));
        object.insert("reclaimableBytes".into(), json!(self.reclaimable));
        object.insert("reclaimable".into(), json!(self.reclaimable_label));
        object.insert("reclaimableNow".into(), json!(self.reclaimable_now));
        object.insert("tipOnRelayKnown".into(), json!(self.tip_known));
        object.insert("tipOnRelayLimit".into(), json!(TIP_ON_RELAY_LIMIT));
        object.insert("exists".into(), json!(self.exists));
        object.insert("sessionDeleted".into(), json!(self.session_deleted));
        object.insert(
            "graceRemainingSecs".into(),
            match self.disposition {
                SeatWorktreeDisposition::WithinGrace { remaining_secs } => json!(remaining_secs),
                _ => Value::Null,
            },
        );
        object.insert("detail".into(), json!(self.detail()));
        Value::Object(object)
    }

    /// The one sentence a person reads for this row.
    ///
    /// A deleted session never reads "the session is not closed": a deletion
    /// settles the tree, so that arm is unreachable, and every other sentence
    /// says which of the two ends this was. "Not closed" over a session that
    /// was deleted is the kind of true-about-the-wrong-thing answer that sent
    /// ledger 135(f)'s worktrees to be removed by hand.
    pub fn detail(&self) -> String {
        let sentence = self.disposition_sentence();
        if self.session_deleted {
            return format!("{sentence} (the session was deleted, not closed)");
        }
        sentence
    }

    fn disposition_sentence(&self) -> String {
        let path = self.record.path.to_string_lossy();
        match self.disposition {
            SeatWorktreeDisposition::Prunable => format!("{path}: clean, will be removed"),
            SeatWorktreeDisposition::Held { dirty_files } => {
                format!("held: {dirty_files} uncommitted files")
            }
            SeatWorktreeDisposition::NotSettled => {
                format!("{path}: the session is not closed, so nothing is removed")
            }
            SeatWorktreeDisposition::TipNotOnRelay if !self.tip_known => format!(
                "{path}: this host could not confirm the branch is on the relay, so nothing is removed"
            ),
            SeatWorktreeDisposition::TipNotOnRelay => format!(
                "{path}: the relay's current ref state does not hold this branch, so nothing is removed"
            ),
            SeatWorktreeDisposition::ExecutionLive => {
                format!("{path}: an execution is still running here")
            }
            SeatWorktreeDisposition::Unrecorded => {
                format!("{path}: this host never recorded cutting it, so it never removes it")
            }
            SeatWorktreeDisposition::Protected => format!("{path}: protected, never removed"),
            SeatWorktreeDisposition::WithinGrace { remaining_secs } => {
                let days = remaining_secs.div_ceil(24 * 60 * 60);
                format!("{path}: clean and pushed, kept {days} more days")
            }
        }
    }
}

/// Facts the caller established outside this module.
#[derive(Debug, Clone, Copy)]
pub struct SessionFacts {
    /// A 44230 revision folds the session to `closed` or `archived`.
    pub settled: bool,
    /// An accepted whole-session deletion named this session.
    pub deleted: bool,
    /// Seconds since that revision.
    pub settled_for_secs: Option<u64>,
    /// An execution is still running.
    ///
    /// Defaults to `false` and is raised by `--execution-live`: a settled
    /// session's executions are stopped by the host that owns them (that is
    /// what the close dialog promises), so a settled umbrella with a live
    /// execution is an operator's observation, not something this command can
    /// derive from the record.
    pub execution_live: bool,
}

/// Classify one recorded worktree.
pub fn row_for(
    key: &str,
    record: &RecordedSeatWorktree,
    facts: SessionFacts,
    tip: Option<bool>,
) -> WorktreeRow {
    let exists = record.path.is_dir();
    let protected = is_protected(&record.repo_root, &record.path);
    let dirty_files = if exists && !protected {
        count_dirty_files(&record.path)
    } else {
        0
    };
    let seat_facts = SeatWorktreeFacts {
        session_settled: facts.settled,
        session_deleted: facts.deleted,
        execution_live: facts.execution_live,
        tip_on_relay: tip.unwrap_or(false),
        dirty_files,
        recorded: true,
        is_protected: protected,
        settled_for_secs: facts.settled_for_secs,
    };
    // What this project calls build state, from its own sandbox.yml, with the
    // built-in list as the fallback for a project that declares none.
    let reclaimable = if exists {
        sandbox::reclaimable(&record.path)
    } else {
        (Some(0), render_reclaimable_bytes(Some(0)))
    };
    WorktreeRow {
        key: key.to_string(),
        record: record.clone(),
        disposition: classify_seat_worktree(&seat_facts),
        dirty_files,
        reclaimable: reclaimable.0,
        reclaimable_label: reclaimable.1,
        reclaimable_now: build_output_reclaimable(&seat_facts),
        tip_known: tip.is_some(),
        exists,
        session_deleted: facts.deleted,
    }
}

/// Gather every row for one session, or for every recorded session.
async fn rows_for(
    client: &BeekeeperClient,
    store: &RecordedWorktreeStore,
    session_ref: Option<&str>,
    seat: Option<&str>,
    execution_live: bool,
) -> Result<Vec<WorktreeRow>, CliError> {
    let now = chrono::Utc::now().timestamp();
    let mut settlement: BTreeMap<String, SessionSettlement> = BTreeMap::new();
    let mut oids_by_repo: BTreeMap<PathBuf, BTreeSet<String>> = BTreeMap::new();
    let mut rows = Vec::new();

    for (key, record) in &store.worktrees {
        let Some((session, label)) = key.split_once('/') else {
            continue;
        };
        if session_ref.is_some_and(|wanted| wanted != session) {
            continue;
        }
        if seat.is_some_and(|wanted| wanted != label) {
            continue;
        }
        if !settlement.contains_key(session) {
            settlement.insert(
                session.to_string(),
                session_end(client, session, now).await?,
            );
        }
        let end = settlement[session];
        if !oids_by_repo.contains_key(&record.repo_root) {
            let oids = match repo_id_of(&record.repo_root) {
                Some(repo_id) => relay_ref_oids(client, &repo_id).await?,
                None => BTreeSet::new(),
            };
            oids_by_repo.insert(record.repo_root.clone(), oids);
        }
        let oids = &oids_by_repo[&record.repo_root];
        // No relay ref state at all means the answer was never established —
        // `None`, which reads as "could not confirm", never as "not pushed".
        let tip = if oids.is_empty() {
            None
        } else {
            tip_on_relay(&record.path, oids)
        };
        rows.push(row_for(
            key,
            record,
            SessionFacts {
                settled: end.settled,
                deleted: end.deleted,
                settled_for_secs: end.settled_for_secs,
                execution_live,
            },
            tip,
        ));
    }
    Ok(rows)
}

/// Print rows as the JSON array every read of this command answers with, or
/// one line each under `--format compact`.
pub fn print_rows(rows: &[WorktreeRow], format: &crate::OutputFormat) {
    match format {
        crate::OutputFormat::Compact => {
            for row in rows {
                println!("{}\t{}", row.disposition.token(), row.detail());
            }
        }
        _ => {
            let values: Vec<Value> = rows.iter().map(WorktreeRow::to_json).collect();
            println!("{}", Value::Array(values));
        }
    }
}

/// `bee sessions worktree status` — every recorded tree and its disposition.
pub async fn cmd_status(
    client: &BeekeeperClient,
    session_ref: Option<&str>,
    all: bool,
    store_path: Option<&str>,
    execution_live: bool,
    format: &crate::OutputFormat,
) -> Result<(), CliError> {
    if session_ref.is_none() && !all {
        return Err(CliError::Usage(
            "name a session with --session, or pass --all".into(),
        ));
    }
    let path = match store_path {
        Some(path) => PathBuf::from(path),
        None => default_store_path()?,
    };
    let store = load_store(&path)?;
    let rows = rows_for(client, &store, session_ref, None, execution_live).await?;
    print_rows(&rows, format);
    Ok(())
}

/// `bee sessions worktree prune` — remove what is prunable, on `--confirm`.
///
/// Without `--confirm` it prints dispositions and removes nothing. With it, a
/// row that is not `prunable` is refused rather than skipped: a sweep that
/// silently passed over held work is exactly how work gets lost.
pub async fn cmd_prune(
    client: &BeekeeperClient,
    session_ref: &str,
    seat: Option<&str>,
    confirm: bool,
    store_path: Option<&str>,
    execution_live: bool,
    format: &crate::OutputFormat,
) -> Result<(), CliError> {
    let path = match store_path {
        Some(path) => PathBuf::from(path),
        None => default_store_path()?,
    };
    let store = load_store(&path)?;
    let rows = rows_for(client, &store, Some(session_ref), seat, execution_live).await?;
    if !confirm {
        print_rows(&rows, format);
        return Ok(());
    }
    if let Some(blocked) = rows.iter().find(|row| !row.disposition.is_host_prunable()) {
        return Err(CliError::Other(format!(
            "refusing to prune: {} — {}",
            blocked.key,
            blocked.detail()
        )));
    }
    let mut removed = Vec::new();
    for row in &rows {
        remove_worktree(&row.record.repo_root, &row.record.path)?;
        removed.push(json!({
            "key": row.key,
            "path": row.record.path.to_string_lossy().into_owned(),
            "removed": true,
        }));
    }
    println!(
        "{}",
        json!({
            "removed": removed.len(),
            "worktrees": removed,
            "recordNote": "the host's own record is updated by the desktop app; this removes directories only",
        })
    );
    Ok(())
}

/// Remove one worktree directory. Never `--force`.
pub fn remove_worktree(repo_root: &Path, path: &Path) -> Result<(), CliError> {
    let output = git_command(repo_root)
        .args(["worktree", "remove"])
        .arg(path)
        .output()
        .map_err(|error| CliError::Other(format!("failed to run git worktree remove: {error}")))?;
    if !output.status.success() {
        return Err(CliError::Other(format!(
            "git refused to remove {}: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let _ = git_command(repo_root).args(["worktree", "prune"]).output();
    Ok(())
}

/// `bee sessions worktree reclaim` — take back the rebuildable directories.
///
/// Independent of `held`: nothing in `target/` or `desktop/node_modules` is a
/// commit, and both are ignored by git, which is why neither ever appears in
/// `dirtyFiles`.
pub async fn cmd_reclaim(
    client: &BeekeeperClient,
    session_ref: &str,
    seat: Option<&str>,
    confirm: bool,
    store_path: Option<&str>,
    execution_live: bool,
    format: &crate::OutputFormat,
) -> Result<(), CliError> {
    let path = match store_path {
        Some(path) => PathBuf::from(path),
        None => default_store_path()?,
    };
    let store = load_store(&path)?;
    let rows = rows_for(client, &store, Some(session_ref), seat, execution_live).await?;
    if !confirm {
        print_rows(&rows, format);
        return Ok(());
    }
    let mut results = Vec::new();
    for row in &rows {
        if !row.reclaimable_now {
            return Err(CliError::Other(format!(
                "refusing to reclaim: {} — {}",
                row.key,
                row.detail()
            )));
        }
        let receipt = sandbox::reclaim_build_output(&row.record.path);
        let (freed, any_unmeasurable) = receipt.freed();
        results.push(json!({
            "key": row.key,
            "declaration": receipt.source,
            "paths": receipt.outcomes,
            "freedBytes": freed,
            "freed": if any_unmeasurable {
                format!(
                    "at least {} — some of what was removed was cloned from the source \
                     checkout, so its exclusive share could not be measured",
                    render_reclaimable_bytes(Some(freed))
                )
            } else {
                render_reclaimable_bytes(Some(freed))
            },
        }));
    }
    println!(
        "{}",
        json!({ "reclaimed": results.len(), "worktrees": results })
    );
    Ok(())
}

/// Route `bee sessions worktree <verb>` to its command function.
///
/// Kept here rather than inline in `sessions.rs`'s dispatch — that file's
/// ownership is module lines only, and every verb this subcommand gains stays
/// off it.
pub async fn dispatch(
    client: &BeekeeperClient,
    cmd: crate::SessionWorktreeCmd,
    format: &crate::OutputFormat,
) -> Result<(), CliError> {
    match cmd {
        crate::SessionWorktreeCmd::Status {
            session,
            all,
            store,
            execution_live,
        } => {
            cmd_status(
                client,
                session.as_deref(),
                all,
                store.as_deref(),
                execution_live,
                format,
            )
            .await
        }
        crate::SessionWorktreeCmd::Prune {
            session,
            seat,
            confirm,
            store,
            execution_live,
        } => {
            cmd_prune(
                client,
                &session,
                seat.as_deref(),
                confirm,
                store.as_deref(),
                execution_live,
                format,
            )
            .await
        }
        crate::SessionWorktreeCmd::Reclaim {
            session,
            seat,
            confirm,
            store,
            execution_live,
        } => {
            cmd_reclaim(
                client,
                &session,
                seat.as_deref(),
                confirm,
                store.as_deref(),
                execution_live,
                format,
            )
            .await
        }
    }
}

#[cfg(test)]
#[path = "worktree_tests.rs"]
mod tests;
