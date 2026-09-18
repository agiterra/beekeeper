//! Crash-safe record of every host-executed action step this provider has
//! seen.
//!
//! A kind:46013 request is a promise the relay replays on every reconnect,
//! and a script is not assumed idempotent: nothing may run twice for one
//! request. So the first thing the provider does with a request is write it
//! here, and every later delivery of the same `requestedEventId` is answered
//! from this file rather than by running anything. The record then follows
//! the step through `claimed → running(pid) → exited → reported`, so a
//! restart can tell a run it lost (a `running` record whose pid is gone)
//! from one it never started, and report the former as `lost_on_restart`
//! (`docs/PROJECT_TEAMS_AND_ACTIONS_SPEC.md` § 5.6).
//!
//! The file is `action-steps.json` beside `state.json`, written through
//! [`crate::state::atomic_write`], in the style of
//! [`crate::ci_continuation_store`]: a version it does not read is an error,
//! never an empty start.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use buzz_core::host_step::HostStepResult;
use serde::{Deserialize, Serialize};

use crate::state::atomic_write;

/// Name of the store file inside the provider state directory.
pub const STORE_FILE: &str = "action-steps.json";
/// Schema version this build reads and writes.
pub const STORE_VERSION: u32 = 1;
/// Ceiling on retained terminal records, evicted oldest-first.
pub const MAX_RETAINED_TERMINALS: usize = 256;

/// Where one request is in its life.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum StepState {
    /// Written the moment the request was first seen, before anything else.
    Requested,
    /// The claim was published (or durably queued) and the command has not
    /// been spawned yet.
    Claimed {
        /// Event id of this host's kind:46022.
        claim_event_id: String,
    },
    /// The command is running.
    Running {
        /// Event id of this host's kind:46022.
        claim_event_id: String,
        /// OS pid, when the runtime reported one.
        pid: Option<u32>,
        /// Epoch seconds at spawn.
        started_at: u64,
    },
    /// The command ended and its result is known but not yet queued.
    Exited {
        /// The result as it will be published.
        result: Box<HostStepResult>,
    },
    /// The kind:46023 result is durably queued for publication.
    Reported {
        /// Event id of the signed result.
        result_event_id: String,
    },
    /// This host declined before claiming, and said so with a kind:46023.
    Refused {
        /// The refusal code that was published.
        code: String,
    },
    /// The relay recorded another host's claim first.
    LostClaim {
        /// Lowercase hex pubkey of the host whose claim won.
        winner: String,
    },
}

impl StepState {
    /// Whether nothing more will happen to this record.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Reported { .. } | Self::Refused { .. } | Self::LostClaim { .. }
        )
    }
}

/// One request this provider has seen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionStepRecord {
    /// Canonical lowercase UUID of the workflow run.
    pub run_id: String,
    /// The step's id.
    pub step_id: String,
    /// Event id of the kind:46013 — the record's key and the idempotency
    /// token for the operation ledger.
    pub requested_event_id: String,
    /// Full kind:30621 project coordinate the step runs for.
    pub project: String,
    /// The action's name in `actions.yml` (the agents repository's root).
    pub workflow_name: String,
    /// Canonical lowercase UUID of the channel every event about the step
    /// is tagged with.
    pub channel_id: String,
    /// Epoch seconds at which the request was first recorded here.
    pub created_at: u64,
    /// Lifecycle position.
    pub state: StepState,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Snapshot {
    version: u32,
    #[serde(default)]
    steps: Vec<ActionStepRecord>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotRef<'a> {
    version: u32,
    steps: &'a [ActionStepRecord],
}

/// Atomic, crash-safe store of action-step records.
#[derive(Debug)]
pub struct ActionStepStore {
    path: PathBuf,
    steps: Vec<ActionStepRecord>,
}

impl ActionStepStore {
    /// Open (or create) the store in `dir`.
    ///
    /// A file that exists but does not parse, or names a version this build
    /// does not read, is an error: every `running` record in it is a run
    /// whose loss must be reported, and starting empty would silently drop
    /// that duty.
    pub fn open(dir: &Path) -> io::Result<Self> {
        let path = dir.join(STORE_FILE);
        let steps = match fs::read(&path) {
            Ok(body) => {
                let snapshot: Snapshot = serde_json::from_slice(&body).map_err(|error| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("{STORE_FILE} is unreadable: {error}"),
                    )
                })?;
                if snapshot.version != STORE_VERSION {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!(
                            "{STORE_FILE} version {} is not supported (expected {STORE_VERSION})",
                            snapshot.version
                        ),
                    ));
                }
                snapshot.steps
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(error),
        };
        Ok(Self { path, steps })
    }

    /// Every record, in insertion order.
    pub fn records(&self) -> impl Iterator<Item = &ActionStepRecord> {
        self.steps.iter()
    }

    /// The record for one request, whatever state it is in.
    pub fn record(&self, requested_event_id: &str) -> Option<&ActionStepRecord> {
        self.steps
            .iter()
            .find(|record| record.requested_event_id == requested_event_id)
    }

    /// Every record whose command was spawned and whose end is not recorded.
    pub fn running(&self) -> impl Iterator<Item = &ActionStepRecord> {
        self.steps
            .iter()
            .filter(|record| matches!(record.state, StepState::Running { .. }))
    }

    /// Write one new record durably.
    ///
    /// Returns `false`, writing nothing, when a record with this
    /// `requestedEventId` already exists: a relay redelivery is the same
    /// request, and the existing record already says what became of it.
    pub fn insert(&mut self, record: ActionStepRecord) -> io::Result<bool> {
        if self.record(&record.requested_event_id).is_some() {
            return Ok(false);
        }
        self.mutate(|steps| steps.push(record))?;
        Ok(true)
    }

    /// Record that the claim for this request was published.
    pub fn mark_claimed(
        &mut self,
        requested_event_id: &str,
        claim_event_id: &str,
    ) -> io::Result<()> {
        self.set_state(
            requested_event_id,
            StepState::Claimed {
                claim_event_id: claim_event_id.to_owned(),
            },
        )
    }

    /// Record that the command was spawned. Keeps the claim id from the
    /// `claimed` record; a record not yet claimed is left untouched.
    pub fn mark_running(
        &mut self,
        requested_event_id: &str,
        pid: Option<u32>,
        started_at: u64,
    ) -> io::Result<()> {
        let claim_event_id = match self.record(requested_event_id).map(|record| &record.state) {
            Some(StepState::Claimed { claim_event_id }) => claim_event_id.clone(),
            _ => return Ok(()),
        };
        self.set_state(
            requested_event_id,
            StepState::Running {
                claim_event_id,
                pid,
                started_at,
            },
        )
    }

    /// Record how the command ended.
    pub fn mark_exited(
        &mut self,
        requested_event_id: &str,
        result: HostStepResult,
    ) -> io::Result<()> {
        self.set_state(
            requested_event_id,
            StepState::Exited {
                result: Box::new(result),
            },
        )
    }

    /// Record that the result is durably queued as `result_event_id`.
    pub fn mark_reported(
        &mut self,
        requested_event_id: &str,
        result_event_id: &str,
    ) -> io::Result<()> {
        self.set_state(
            requested_event_id,
            StepState::Reported {
                result_event_id: result_event_id.to_owned(),
            },
        )
    }

    /// Record that this host refused the request with `code`.
    pub fn mark_refused(&mut self, requested_event_id: &str, code: &str) -> io::Result<()> {
        self.set_state(
            requested_event_id,
            StepState::Refused {
                code: code.to_owned(),
            },
        )
    }

    /// Record that another host's claim was recorded first.
    pub fn mark_lost_claim(&mut self, requested_event_id: &str, winner: &str) -> io::Result<()> {
        self.set_state(
            requested_event_id,
            StepState::LostClaim {
                winner: winner.to_owned(),
            },
        )
    }

    /// Remove a record entirely. Used only when the request turned out not
    /// to be this provider's to answer (the operation ledger already names a
    /// different request for the same run and step).
    pub fn remove(&mut self, requested_event_id: &str) -> io::Result<()> {
        if self.record(requested_event_id).is_none() {
            return Ok(());
        }
        self.mutate(|steps| steps.retain(|record| record.requested_event_id != requested_event_id))
    }

    fn set_state(&mut self, requested_event_id: &str, state: StepState) -> io::Result<()> {
        if self.record(requested_event_id).is_none() {
            return Ok(());
        }
        self.mutate(|steps| {
            for record in steps.iter_mut() {
                if record.requested_event_id == requested_event_id {
                    record.state = state.clone();
                }
            }
        })
    }

    /// Apply `change`, persist, and roll the in-memory half back if the write
    /// failed — an undurable change is not a change.
    fn mutate(&mut self, change: impl FnOnce(&mut Vec<ActionStepRecord>)) -> io::Result<()> {
        let previous = self.steps.clone();
        change(&mut self.steps);
        prune_terminals(&mut self.steps);
        if let Err(error) = self.persist() {
            self.steps = previous;
            return Err(error);
        }
        Ok(())
    }

    fn persist(&self) -> io::Result<()> {
        let body = serde_json::to_vec_pretty(&SnapshotRef {
            version: STORE_VERSION,
            steps: &self.steps,
        })?;
        atomic_write(&self.path, &body)
    }
}

/// Drop the oldest terminal records beyond [`MAX_RETAINED_TERMINALS`].
///
/// Terminal records are kept at all so a relay redelivery of a request this
/// provider already answered cannot make it run; only the oldest are let go.
fn prune_terminals(steps: &mut Vec<ActionStepRecord>) {
    let terminal = steps
        .iter()
        .filter(|record| record.state.is_terminal())
        .count();
    if terminal <= MAX_RETAINED_TERMINALS {
        return;
    }
    let mut excess = terminal - MAX_RETAINED_TERMINALS;
    let mut order: Vec<(u64, String)> = steps
        .iter()
        .filter(|record| record.state.is_terminal())
        .map(|record| (record.created_at, record.requested_event_id.clone()))
        .collect();
    order.sort();
    let doomed: Vec<String> = order.into_iter().take(excess).map(|(_, id)| id).collect();
    steps.retain(|record| {
        if excess > 0 && doomed.contains(&record.requested_event_id) {
            excess -= 1;
            false
        } else {
            true
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use buzz_core::host_step::{HostStepDisposition, HOST_STEP_SCHEMA};

    fn record(id: u8, created_at: u64) -> ActionStepRecord {
        ActionStepRecord {
            run_id: "00000000-0000-0000-0000-000000000001".into(),
            step_id: "build".into(),
            requested_event_id: hex::encode([id; 32]),
            project: format!("30621:{}:pulse", "11".repeat(32)),
            workflow_name: "nightly".into(),
            channel_id: "00000000-0000-0000-0000-000000000009".into(),
            created_at,
            state: StepState::Requested,
        }
    }

    fn result(record: &ActionStepRecord) -> HostStepResult {
        HostStepResult {
            schema: HOST_STEP_SCHEMA.into(),
            run_id: record.run_id.clone(),
            step_id: record.step_id.clone(),
            requested_event_id: record.requested_event_id.clone(),
            claim_event_id: Some(hex::encode([0x66; 32])),
            channel_id: record.channel_id.clone(),
            disposition: HostStepDisposition::Exited,
            exit_code: Some(0),
            refusal: None,
            timed_out: false,
            duration_ms: Some(10),
            head_sha: None,
            dirty: None,
            stdout_tail: String::new(),
            stderr_tail: String::new(),
            truncated: false,
            artifact_path: None,
            routed: None,
            artifacts: Vec::new(),
        }
    }

    #[test]
    fn a_record_round_trips_through_every_state_and_a_reopen() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut store = ActionStepStore::open(dir.path()).expect("open");
        let first = record(1, 100);
        let id = first.requested_event_id.clone();
        assert!(store.insert(first.clone()).expect("insert"));
        assert!(!store.insert(first).expect("duplicate insert is a no-op"));
        assert_eq!(store.records().count(), 1);

        store.mark_claimed(&id, &"66".repeat(32)).expect("claimed");
        store.mark_running(&id, Some(4242), 101).expect("running");
        assert_eq!(store.running().count(), 1);
        let reopened = ActionStepStore::open(dir.path()).expect("reopen");
        assert_eq!(
            reopened.record(&id).map(|record| &record.state),
            Some(&StepState::Running {
                claim_event_id: "66".repeat(32),
                pid: Some(4242),
                started_at: 101,
            })
        );

        let result = result(reopened.record(&id).expect("record"));
        store.mark_exited(&id, result.clone()).expect("exited");
        assert_eq!(store.running().count(), 0);
        store
            .mark_reported(&id, &"77".repeat(32))
            .expect("reported");
        let reopened = ActionStepStore::open(dir.path()).expect("reopen");
        assert_eq!(
            reopened.record(&id).map(|record| &record.state),
            Some(&StepState::Reported {
                result_event_id: "77".repeat(32)
            })
        );
        assert!(reopened.record(&id).expect("record").state.is_terminal());
    }

    #[test]
    fn running_is_only_reachable_from_claimed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut store = ActionStepStore::open(dir.path()).expect("open");
        let first = record(2, 100);
        let id = first.requested_event_id.clone();
        store.insert(first).expect("insert");
        store.mark_running(&id, Some(1), 1).expect("ignored");
        assert_eq!(
            store.record(&id).map(|record| &record.state),
            Some(&StepState::Requested)
        );
        store
            .mark_refused(&id, "ACTION_DEFINITION_DRIFT")
            .expect("refused");
        store
            .mark_lost_claim(&"ab".repeat(32), "cd")
            .expect("unknown id is a no-op");
        store.remove(&id).expect("remove");
        assert_eq!(store.records().count(), 0);
    }

    #[test]
    fn an_unreadable_or_foreign_version_file_refuses_to_open() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join(STORE_FILE),
            b"{\"version\": 99, \"steps\": []}",
        )
        .expect("write");
        assert_eq!(
            ActionStepStore::open(dir.path()).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        std::fs::write(dir.path().join(STORE_FILE), b"not json").expect("write");
        assert!(ActionStepStore::open(dir.path()).is_err());
    }

    #[test]
    fn only_the_oldest_terminal_records_are_pruned() {
        let mut steps: Vec<ActionStepRecord> = (0..(MAX_RETAINED_TERMINALS + 3))
            .map(|index| {
                let mut record = record((index % 250) as u8, index as u64);
                record.requested_event_id = format!("{index:0>64}");
                record.state = StepState::Refused { code: "X".into() };
                record
            })
            .collect();
        let mut live = record(9, 0);
        live.requested_event_id = "live".into();
        steps.push(live);
        prune_terminals(&mut steps);
        assert_eq!(steps.len(), MAX_RETAINED_TERMINALS + 1);
        assert!(steps
            .iter()
            .any(|record| record.requested_event_id == "live"));
        assert!(!steps
            .iter()
            .any(|record| record.state.is_terminal() && record.created_at < 3));
    }
}
