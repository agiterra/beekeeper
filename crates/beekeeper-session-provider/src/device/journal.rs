//! The device engine's durable once-only record.
//!
//! `<CSP state>/device/journal.json` (0600) holds one entry per command (and
//! per generation's availability record): written `running` before any work
//! starts, then the signed answer events, then `published` once the relay
//! accepted them. A replayed command finds its entry and is not run again; a
//! restart re-sends answered-but-unaccepted events and answers whatever was
//! left running ([`super::listener::Engine::recover`]).

use std::collections::BTreeMap;

use nostr::{Event, JsonUtil};
use serde::{Deserialize, Serialize};

use super::capture::now_ms;
use super::listener::DeviceSessionView;

/// Published journal entries kept for dedup.
const JOURNAL_KEEP: usize = 4096;

/// One journal entry: a command (or availability) and the events that
/// answer it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JournalEntry {
    /// Channel.
    pub channel: String,
    /// `cs-target`.
    pub cs_target: String,
    /// `csl-command`.
    pub csl_command: String,
    /// The command's `sdv-cmd`, when it is a command.
    #[serde(default)]
    pub sdv_cmd: Option<String>,
    /// The work is running; no answer yet.
    pub running: bool,
    /// Signed answer events (JSON), in publish order.
    #[serde(default)]
    pub events: Vec<String>,
    /// The relay accepted every answer event.
    pub published: bool,
    /// When the entry was made (unix ms).
    pub at_ms: u64,
}

/// The durable once-only record, `<CSP state>/device/journal.json` (0600).
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Journal {
    entries: BTreeMap<String, JournalEntry>,
    #[serde(skip)]
    path: Option<std::path::PathBuf>,
}

impl Journal {
    /// Load (or start) the journal at `path`. Entries left `running` by a
    /// previous process are answered by the caller ([`Engine::new`]).
    pub fn load(path: std::path::PathBuf) -> Self {
        let mut journal: Journal = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        journal.path = Some(path);
        journal
    }

    /// An in-memory journal (tests).
    pub fn in_memory() -> Self {
        Self::default()
    }

    fn save(&self) {
        let Some(path) = &self.path else { return };
        match serde_json::to_vec(self) {
            Ok(bytes) => {
                if let Err(error) = super::slot::write_private(path, &bytes) {
                    tracing::warn!(target: "csp::device", "device journal not saved: {error}");
                }
            }
            Err(error) => {
                tracing::warn!(target: "csp::device", "device journal not encoded: {error}")
            }
        }
    }

    /// Whether `key` was ever seen.
    pub fn contains(&self, key: &str) -> bool {
        self.entries.contains_key(key)
    }

    pub(super) fn begin(&mut self, key: &str, view: &DeviceSessionView, sdv_cmd: Option<&str>) {
        self.entries.insert(
            key.to_owned(),
            JournalEntry {
                channel: view.channel.clone(),
                cs_target: view.cs_target.clone(),
                csl_command: view.csl_command.clone(),
                sdv_cmd: sdv_cmd.map(str::to_owned),
                running: true,
                events: Vec::new(),
                published: false,
                at_ms: now_ms(),
            },
        );
        self.save();
    }

    pub(super) fn answer(&mut self, key: &str, events: &[Event]) {
        if let Some(entry) = self.entries.get_mut(key) {
            entry.running = false;
            entry.events = events.iter().map(JsonUtil::as_json).collect();
        }
        self.prune();
        self.save();
    }

    /// The relay accepted every event of `key`.
    pub fn mark_published(&mut self, key: &str) {
        if let Some(entry) = self.entries.get_mut(key) {
            entry.published = true;
        }
        self.save();
    }

    /// Answered but not yet accepted: what a (re)connect re-sends.
    pub fn unpublished(&self) -> Vec<(String, Vec<Event>)> {
        self.entries
            .iter()
            .filter(|(_, entry)| !entry.running && !entry.published)
            .map(|(key, entry)| {
                let events = entry
                    .events
                    .iter()
                    .filter_map(|json| Event::from_json(json).ok())
                    .collect();
                (key.clone(), events)
            })
            .collect()
    }

    pub(super) fn running_keys(&self) -> Vec<String> {
        self.entries
            .iter()
            .filter(|(_, entry)| entry.running)
            .map(|(key, _)| key.clone())
            .collect()
    }

    pub(super) fn entry(&self, key: &str) -> Option<&JournalEntry> {
        self.entries.get(key)
    }

    fn prune(&mut self) {
        if self.entries.len() <= JOURNAL_KEEP {
            return;
        }
        let mut published: Vec<(u64, String)> = self
            .entries
            .iter()
            .filter(|(_, entry)| entry.published)
            .map(|(key, entry)| (entry.at_ms, key.clone()))
            .collect();
        published.sort();
        let excess = self.entries.len() - JOURNAL_KEEP;
        for (_, key) in published.into_iter().take(excess) {
            self.entries.remove(&key);
        }
    }
}
