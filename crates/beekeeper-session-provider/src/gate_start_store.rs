//! The gate starts this provider published and has not yet closed (SV-41 S5).
//!
//! A gate start (kind 44246 phase `gate:<gate>`, `endedAtMs: null`) reads
//! "gate running" until its own closing row arrives. The provider signs that
//! close when the call ends — but a provider that stops mid-gate takes the
//! agent and its command with it, and the observer that would have closed the
//! start was memory. Without this file the start would read "running" until
//! the 30-minute stale rule, for a gate whose process is already dead.
//!
//! So every start is written here **before** it is enqueued, and removed when
//! its close is enqueued. At boot, every entry still here is a gate whose
//! process died with the previous provider: it is closed with `endedAtMs` =
//! boot time and `durationMs: null` (the provider did not see it end, so it
//! claims no span), and the entry is cleared.
//!
//! Host-local and never on the wire: the session id lives here and nowhere in
//! the signed rows.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::state::atomic_write;

const STORE_FILE: &str = "gate-starts-open.json";
const STORE_SCHEMA: &str = "buzz-provider-gate-starts-open/v1";

/// How many open starts the store holds.
///
/// Far above anything real: a start is open only while its call runs, and the
/// observer keeps at most 32 pending calls per session. A start that would
/// exceed it is not published at all — no "running" claim is safer than one
/// a restart could not close.
pub(crate) const MAX_OPEN_GATE_STARTS: usize = 1_024;

/// One published start whose close has not been enqueued.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OpenGateStart {
    /// The provider's session id (host-local).
    pub session_id: String,
    /// The table's gate name, as the start's phase carries it.
    pub gate: String,
    /// The start's `startedAtMs`, which pairs the close with it.
    pub started_at_ms: i64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Snapshot {
    schema: String,
    #[serde(default)]
    open: Vec<OpenGateStart>,
}

/// Atomic, crash-safe set of open gate starts.
#[derive(Debug)]
pub(crate) struct GateStartStore {
    path: PathBuf,
    open: Vec<OpenGateStart>,
}

impl GateStartStore {
    /// Open the store in `dir`, quarantining a body this build cannot read.
    ///
    /// A quarantined store starts empty; the starts it held fall back to the
    /// 30-minute stale rule, which reads "no result observed" — never
    /// "running" — so the cost is delay, not an untrue state.
    pub(crate) fn open(dir: &Path) -> io::Result<Self> {
        let path = dir.join(STORE_FILE);
        let open = match fs::read(&path) {
            Ok(body) => match serde_json::from_slice::<Snapshot>(&body) {
                Ok(snapshot)
                    if snapshot.schema == STORE_SCHEMA
                        && snapshot.open.len() <= MAX_OPEN_GATE_STARTS =>
                {
                    snapshot.open
                }
                Ok(_) | Err(_) => {
                    quarantine(&path)?;
                    tracing::error!(
                        target: "csp",
                        path = %path.display(),
                        "quarantined a corrupt or unsupported open-gate-start store; starts it \
                         held close only by the stale rule"
                    );
                    Vec::new()
                }
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(error),
        };
        Ok(Self { path, open })
    }

    fn persist(&self) -> io::Result<()> {
        let body = serde_json::to_vec_pretty(&Snapshot {
            schema: STORE_SCHEMA.to_owned(),
            open: self.open.clone(),
        })
        .map_err(io::Error::other)?;
        atomic_write(&self.path, &body)
    }

    /// Record one start, on disk, before it is enqueued. A repeat is a no-op.
    ///
    /// # Errors
    /// The store is full, or the write failed. Either way the caller must not
    /// publish the start: a restart could not close it.
    pub(crate) fn record(&mut self, entry: OpenGateStart) -> io::Result<()> {
        if self.open.contains(&entry) {
            return Ok(());
        }
        if self.open.len() >= MAX_OPEN_GATE_STARTS {
            return Err(io::Error::other("the open-gate-start store is full"));
        }
        self.open.push(entry);
        if let Err(error) = self.persist() {
            self.open.pop();
            return Err(error);
        }
        Ok(())
    }

    /// Forget one start whose close is enqueued. Absent is a no-op.
    pub(crate) fn clear(
        &mut self,
        session_id: &str,
        gate: &str,
        started_at_ms: i64,
    ) -> io::Result<()> {
        let before = self.open.len();
        self.open.retain(|entry| {
            !(entry.session_id == session_id
                && entry.gate == gate
                && entry.started_at_ms == started_at_ms)
        });
        if self.open.len() == before {
            return Ok(());
        }
        self.persist()
    }

    /// Every start still open, oldest recorded first.
    pub(crate) fn entries(&self) -> &[OpenGateStart] {
        &self.open
    }
}

fn quarantine(path: &Path) -> io::Result<()> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    let mut target = path.with_file_name(format!("{STORE_FILE}.quarantined-{seconds}"));
    let mut ordinal = 0_u32;
    while target.exists() {
        ordinal = ordinal.saturating_add(1);
        target = path.with_file_name(format!("{STORE_FILE}.quarantined-{seconds}-{ordinal}"));
    }
    fs::rename(path, target)
}
