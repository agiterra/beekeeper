//! Crash-safe storage for the two facts a host-result wake needs.
//!
//! Both halves are here because they are one question asked twice: *whose run
//! was this*, and *have I already said so*.
//!
//! 1. **The trigger index.** The relay-signed kind:46013 names the run's
//!    trigger author; the kind:46014 that finishes the run does not. The two
//!    can be minutes apart — 15 m 30 s apart in the run ledger 236(g)
//!    measures, most of it a person deciding whether to approve — and a
//!    provider restart inside that window is ordinary. So the author is
//!    written to disk when the request is seen, not held in memory and hoped
//!    for.
//!
//! 2. **The waked ledger.** One wake per result event id, forever, across
//!    restarts. The id is claimed and persisted **before** the wake is
//!    queued, which is the only order that makes "a restart must not re-wake"
//!    true: the other order loses at-most-once to a crash between the publish
//!    and the write, and a duplicate wake spends a lead's context on a fact it
//!    already has.
//!
//! The cost of claiming first is stated rather than hidden: a crash between
//! the claim and the enqueue loses that one wake. At-most-once is the
//! guarantee this store makes; at-least-once is not, and pretending otherwise
//! by writing afterwards would trade a visible rare loss for an invisible
//! common duplicate.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::state::atomic_write;

const STORE_FILE: &str = "host-result-wakes.json";
const STORE_SCHEMA: &str = "buzz-provider-host-result-wakes/v1";

/// How many runs the index remembers the trigger author of.
///
/// A bound rather than a horizon: a run whose entry has been evicted wakes
/// nobody and says so in the log, which is a visible miss. Unbounded growth
/// would instead be an invisible one — a state file that eventually fails to
/// write, taking every other wake with it.
const MAX_TRIGGERS: usize = 512;

/// How many answered results the ledger remembers.
///
/// Larger than the trigger index on purpose: this is the half that must not
/// forget. Eviction here can cost a duplicate wake, so it is set far above
/// any plausible replay window.
const MAX_WAKED: usize = 4_096;

/// How many results the automatic evidence binder remembers deciding.
///
/// The same bound as the waked ledger, for the same reason: forgetting one
/// costs at most a second decision, which finds its own records on the wire
/// and publishes nothing.
const MAX_EVIDENCE_CHECKED: usize = MAX_WAKED;

/// What the kind:46013 said about who started a run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TriggerFact {
    /// `<runId>:<stepId>` — the `d` tag every event about one host step
    /// carries, and the only identity the kind:46014 shares with the
    /// kind:46013.
    pub run_key: String,
    /// Lowercase hex pubkey the *relay* derived from the kind:46020's
    /// signature. Never a value the trigger's signer chose.
    pub author: String,
    /// The workflow's channel, from the request's `h` tag.
    pub channel_ref: Uuid,
    /// The project coordinate the action belongs to, for the log line.
    pub project: String,
    /// The action's name, for the log line.
    pub workflow_name: String,
    /// When this host first saw the request, in unix seconds.
    pub observed_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Snapshot {
    schema: String,
    #[serde(default)]
    triggers: Vec<TriggerFact>,
    #[serde(default)]
    waked: Vec<String>,
    /// Results the automatic evidence binder has taken custody of
    /// (`crate::auto_evidence`). Defaulted: a store written before it existed
    /// has decided nothing.
    #[serde(default)]
    evidence_checked: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotRef<'a> {
    schema: &'static str,
    triggers: &'a [TriggerFact],
    waked: &'a [String],
    evidence_checked: &'a [String],
}

/// Atomic crash-safe store for host-result wake routing and at-most-once.
#[derive(Debug)]
pub struct HostResultWakeStore {
    path: PathBuf,
    triggers: Vec<TriggerFact>,
    waked: Vec<String>,
    evidence_checked: Vec<String>,
}

impl HostResultWakeStore {
    /// Open the store in `dir`, quarantining a body this build cannot read.
    ///
    /// A store it cannot parse is renamed rather than deleted — the operator
    /// keeps the evidence — and this process starts empty, which costs
    /// routing for runs already in flight and can cost one duplicate wake.
    /// Refusing to start instead would take every session on the host down
    /// with a wake ledger.
    pub fn open(dir: &Path) -> io::Result<Self> {
        let path = dir.join(STORE_FILE);
        let snapshot = match fs::read(&path) {
            Ok(body) => match serde_json::from_slice::<Snapshot>(&body) {
                Ok(snapshot) if Self::snapshot_valid(&snapshot) => snapshot,
                Ok(_) | Err(_) => {
                    Self::quarantine(&path)?;
                    tracing::error!(
                        target: "csp::host_result_wake",
                        path = %path.display(),
                        "quarantined a corrupt or unsupported host-result wake store; runs \
                         already in flight will wake nobody"
                    );
                    Self::empty()
                }
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => Self::empty(),
            Err(error) => return Err(error),
        };
        Ok(Self {
            path,
            triggers: snapshot.triggers,
            waked: snapshot.waked,
            evidence_checked: snapshot.evidence_checked,
        })
    }

    fn empty() -> Snapshot {
        Snapshot {
            schema: STORE_SCHEMA.to_owned(),
            triggers: Vec::new(),
            waked: Vec::new(),
            evidence_checked: Vec::new(),
        }
    }

    fn snapshot_valid(snapshot: &Snapshot) -> bool {
        if snapshot.schema != STORE_SCHEMA
            || snapshot.triggers.len() > MAX_TRIGGERS
            || snapshot.waked.len() > MAX_WAKED
            || snapshot.evidence_checked.len() > MAX_EVIDENCE_CHECKED
        {
            return false;
        }
        let mut run_keys = HashSet::new();
        let mut results = HashSet::new();
        snapshot
            .triggers
            .iter()
            .all(|fact| run_keys.insert(fact.run_key.as_str()))
            && snapshot.waked.iter().all(|id| results.insert(id.as_str()))
    }

    fn quarantine(path: &Path) -> io::Result<()> {
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs());
        let mut suffix = seconds.to_string();
        let mut target = path.with_file_name(format!("{STORE_FILE}.quarantined-{suffix}"));
        let mut ordinal = 0_u32;
        while target.exists() {
            ordinal = ordinal.saturating_add(1);
            suffix = format!("{seconds}-{ordinal}");
            target = path.with_file_name(format!("{STORE_FILE}.quarantined-{suffix}"));
        }
        fs::rename(path, target)
    }

    fn persist(&self) -> io::Result<()> {
        let body = serde_json::to_vec_pretty(&SnapshotRef {
            schema: STORE_SCHEMA,
            triggers: &self.triggers,
            waked: &self.waked,
            evidence_checked: &self.evidence_checked,
        })
        .map_err(io::Error::other)?;
        atomic_write(&self.path, &body)
    }

    /// Remember who triggered one run.
    ///
    /// Returns `false` when the run was already indexed, which is the common
    /// case: the channel subscription replays stored events on every
    /// reconnect and on every restart, so one kind:46013 is seen many times.
    /// A repeat writes nothing at all.
    pub fn note_trigger(&mut self, fact: TriggerFact) -> io::Result<bool> {
        if self
            .triggers
            .iter()
            .any(|known| known.run_key == fact.run_key)
        {
            return Ok(false);
        }
        self.triggers.push(fact);
        // Oldest first: a run whose request arrived long ago is the one whose
        // result is least likely still to come.
        while self.triggers.len() > MAX_TRIGGERS {
            self.triggers.remove(0);
        }
        self.persist()?;
        Ok(true)
    }

    /// What the kind:46013 said about this run, if this host still knows.
    pub fn trigger(&self, run_key: &str) -> Option<&TriggerFact> {
        self.triggers.iter().find(|known| known.run_key == run_key)
    }

    /// Whether this result has already been answered with a wake.
    pub fn already_waked(&self, result_event_id: &str) -> bool {
        self.waked.iter().any(|known| known == result_event_id)
    }

    /// Take at-most-once custody of one result event id.
    ///
    /// `Ok(true)` means this call is the first and the claim is **already on
    /// disk** — the caller may queue exactly one wake. `Ok(false)` means some
    /// earlier call, in this process or a previous one, already holds it.
    pub fn claim_result(&mut self, result_event_id: &str) -> io::Result<bool> {
        if self.already_waked(result_event_id) {
            return Ok(false);
        }
        self.waked.push(result_event_id.to_owned());
        while self.waked.len() > MAX_WAKED {
            self.waked.remove(0);
        }
        self.persist()?;
        Ok(true)
    }

    /// Whether the automatic evidence binder already decided this result.
    pub fn evidence_checked(&self, result_event_id: &str) -> bool {
        self.evidence_checked
            .iter()
            .any(|known| known == result_event_id)
    }

    /// Take at-most-once custody of one result for the automatic evidence
    /// binder, on disk before any read or publish — the same order, and the
    /// same stated cost, as [`Self::claim_result`].
    pub fn claim_evidence(&mut self, result_event_id: &str) -> io::Result<bool> {
        if self.evidence_checked(result_event_id) {
            return Ok(false);
        }
        self.evidence_checked.push(result_event_id.to_owned());
        while self.evidence_checked.len() > MAX_EVIDENCE_CHECKED {
            self.evidence_checked.remove(0);
        }
        self.persist()?;
        Ok(true)
    }

    /// How many results this ledger has answered. For tests and status.
    pub fn waked_len(&self) -> usize {
        self.waked.len()
    }

    /// How many runs this index can still route. For tests and status.
    pub fn trigger_len(&self) -> usize {
        self.triggers.len()
    }
}
