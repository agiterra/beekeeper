//! Crash-safe schema-v3 storage for provider-owned team wakes.
//!
//! A structural channel refusal may be removed manually only while the
//! provider is stopped: edit `team-wake-intents.json`, remove that channel's
//! `refusal` object, then restart the provider. Live editing is unsupported;
//! the in-memory store owns the authoritative snapshot and its next atomic
//! write can overwrite an on-disk edit made while the provider is running.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use beekeeper_core::coding_session_command::CodingSessionTarget;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::state::atomic_write;

use super::{WakeIntent, WakeScope, WakeSource};

const STORE_FILE: &str = "team-wake-intents.json";
const STORE_SCHEMA: &str = "buzz-provider-team-wake-intents/v3";
const MAX_DISCOVERY_CHANNELS: usize = 1_024;
const MAX_ADMITTED_PER_CHANNEL: usize = 64;
const MAX_TERMINALS_PER_CHANNEL: usize = 256;
// This is not a second discovery limit. It is the existing complete-control
// partition envelope (32 pages of 1,000 events) expressed at the durable
// ledger boundary, where exceeding it would otherwise let live traffic create
// a partial truth the complete scan correctly refuses.
const MAX_RESOLVED_PER_CHANNEL: usize = 32 * 1_000;

/// Result of attempting to admit a relay-backed report source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoveryCapture {
    Admitted,
    Duplicate,
    /// The source remains recoverable by a later complete partition scan.
    Saturated,
    /// The channel cannot be represented truthfully by the complete scan.
    /// The caller must make this explicit and stop treating the channel as
    /// successfully discovered.
    ResolvedLedgerFull,
    /// This channel is durably wake-dead at the verification envelope.
    Refused(ChannelRefusalCode),
}

/// Durable reason one channel cannot be verified exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelRefusalCode {
    PartitionSaturated,
    ResolvedLedgerFull,
}

/// Operator-visible structural refusal retained until a successful restart
/// probe or manual deletion of this object from the store file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelRefusal {
    pub code: ChannelRefusalCode,
    pub at_unix: u64,
    pub refused_live_reports: u32,
    pub last_refused_event_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SourceRef {
    scope: WakeScope,
    source: WakeSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChannelState {
    channel_ref: Uuid,
    #[serde(default)]
    resolved: Vec<String>,
    #[serde(default)]
    admitted: Vec<SourceRef>,
    #[serde(default)]
    in_flight: Option<WakeIntent>,
    #[serde(default)]
    terminals: Vec<WakeIntent>,
    #[serde(default)]
    refusal: Option<ChannelRefusal>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Snapshot {
    schema: String,
    #[serde(default)]
    channels: Vec<ChannelState>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotRef<'a> {
    schema: &'static str,
    channels: &'a [ChannelState],
}

/// Atomic crash-safe store for unresolved provider wake intents.
#[derive(Debug)]
pub struct WakeIntentStore {
    path: PathBuf,
    rr_channel: Option<Uuid>,
    channels: Vec<ChannelState>,
    poisoned: bool,
}

impl WakeIntentStore {
    pub fn open(dir: &Path) -> io::Result<Self> {
        let path = dir.join(STORE_FILE);
        let snapshot = match fs::read(&path) {
            Ok(body) => match serde_json::from_slice::<Snapshot>(&body) {
                Ok(snapshot) if Self::snapshot_valid(&snapshot) => snapshot,
                Ok(_) | Err(_) => {
                    Self::quarantine(&path)?;
                    tracing::error!(
                        target: "csp::team_wake",
                        path = %path.display(),
                        "quarantined corrupt or unsupported team-wake store; restarting at-least-once discovery"
                    );
                    Self::empty_snapshot()
                }
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => Self::empty_snapshot(),
            Err(error) => return Err(error),
        };
        Ok(Self {
            path,
            rr_channel: None,
            channels: snapshot.channels,
            poisoned: false,
        })
    }

    fn empty_snapshot() -> Snapshot {
        Snapshot {
            schema: STORE_SCHEMA.to_owned(),
            channels: Vec::new(),
        }
    }

    fn snapshot_valid(snapshot: &Snapshot) -> bool {
        if snapshot.schema != STORE_SCHEMA || snapshot.channels.len() > MAX_DISCOVERY_CHANNELS {
            return false;
        }
        let mut channels = HashSet::new();
        snapshot.channels.iter().all(|channel| {
            channels.insert(channel.channel_ref)
                && channel.admitted.len() <= MAX_ADMITTED_PER_CHANNEL
                && channel.terminals.len() <= MAX_TERMINALS_PER_CHANNEL
                && channel.resolved.len() <= MAX_RESOLVED_PER_CHANNEL
                && channel.admitted.iter().all(|source| {
                    source.scope.channel_ref == channel.channel_ref
                        && matches!(
                            source.source,
                            WakeSource::Report { .. } | WakeSource::Disposition { .. }
                        )
                })
                && channel.in_flight.as_ref().is_none_or(|intent| {
                    intent.scope.channel_ref == channel.channel_ref
                        && valid_reason_detail(intent.last_reason_detail.as_deref())
                })
                && channel.terminals.iter().all(|intent| {
                    intent.scope.channel_ref == channel.channel_ref
                        && matches!(intent.source, WakeSource::Terminal { .. })
                        && valid_reason_detail(intent.last_reason_detail.as_deref())
                })
                && Self::report_count(channel) <= MAX_RESOLVED_PER_CHANNEL
                && Self::channel_unique(channel)
        })
    }

    fn channel_unique(channel: &ChannelState) -> bool {
        let mut ids = HashSet::new();
        channel.resolved.iter().all(|id| ids.insert(id.as_str()))
            && channel
                .admitted
                .iter()
                .all(|source| ids.insert(source.source.event_id()))
            && channel
                .in_flight
                .as_ref()
                .is_none_or(|intent| ids.insert(intent.source.event_id()))
    }

    fn quarantine(path: &Path) -> io::Result<()> {
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_secs());
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

    fn ensure_healthy(&self) -> io::Result<()> {
        if self.poisoned {
            return Err(io::Error::other(
                "provider team-wake store is poisoned after a failed durable write",
            ));
        }
        Ok(())
    }

    fn persist(&mut self) -> io::Result<()> {
        self.ensure_healthy()?;
        let body = serde_json::to_vec(&SnapshotRef {
            schema: STORE_SCHEMA,
            channels: &self.channels,
        })?;
        if let Err(error) = atomic_write(&self.path, &body) {
            self.poisoned = true;
            return Err(error);
        }
        Ok(())
    }

    fn channel_index(&self, channel_ref: Uuid) -> Option<usize> {
        self.channels
            .iter()
            .position(|channel| channel.channel_ref == channel_ref)
    }

    fn channel_index_or_insert(&mut self, channel_ref: Uuid) -> io::Result<usize> {
        self.ensure_healthy()?;
        if let Some(index) = self.channel_index(channel_ref) {
            return Ok(index);
        }
        if self.channels.len() >= MAX_DISCOVERY_CHANNELS {
            tracing::error!(
                target: "csp::team_wake",
                %channel_ref,
                "refusing team-wake channel: durable channel bound reached"
            );
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "provider team-wake channel bound reached",
            ));
        }
        self.channels.push(ChannelState {
            channel_ref,
            resolved: Vec::new(),
            admitted: Vec::new(),
            in_flight: None,
            terminals: Vec::new(),
            refusal: None,
        });
        Ok(self.channels.len() - 1)
    }

    fn intent(scope: WakeScope, source: WakeSource) -> WakeIntent {
        WakeIntent {
            scope,
            source,
            target: None,
            command_id: None,
            signed_event: None,
            relay_accepted_at: None,
            attempt: 0,
            last_reason: None,
            last_reason_detail: None,
        }
    }

    fn report_known(channel: &ChannelState, source_id: &str) -> bool {
        channel.resolved.iter().any(|id| id == source_id)
            || channel
                .admitted
                .iter()
                .any(|source| source.source.event_id() == source_id)
            || channel
                .in_flight
                .as_ref()
                .is_some_and(|intent| intent.source.event_id() == source_id)
    }

    fn report_count(channel: &ChannelState) -> usize {
        channel.resolved.len()
            + channel.admitted.len()
            + usize::from(channel.in_flight.as_ref().is_some_and(|intent| {
                matches!(
                    intent.source,
                    WakeSource::Report { .. } | WakeSource::Disposition { .. }
                )
            }))
    }

    fn report_capacity_available(channel: &ChannelState) -> bool {
        Self::report_count(channel) < MAX_RESOLVED_PER_CHANNEL
    }

    fn capture_report_inner(
        &mut self,
        scope: WakeScope,
        source: WakeSource,
        live: bool,
    ) -> io::Result<DiscoveryCapture> {
        debug_assert!(matches!(
            source,
            WakeSource::Report { .. } | WakeSource::Disposition { .. }
        ));
        let index = self.channel_index_or_insert(scope.channel_ref)?;
        if let Some(code) = self.channels[index]
            .refusal
            .as_ref()
            .map(|refusal| refusal.code)
        {
            if live {
                let refusal = self.channels[index]
                    .refusal
                    .as_mut()
                    .ok_or_else(|| io::Error::other("team-wake refusal disappeared"))?;
                refusal.refused_live_reports = refusal.refused_live_reports.saturating_add(1);
                refusal.last_refused_event_id = Some(source.event_id().to_owned());
                tracing::error!(
                    target: "csp::team_wake",
                    channel_ref = %scope.channel_ref,
                    source_id = %source.event_id(),
                    code = ?code,
                    "refusing live team-wake report on structurally refused channel"
                );
                self.persist()?;
            }
            return Ok(DiscoveryCapture::Refused(code));
        }
        let channel = &mut self.channels[index];
        if Self::report_known(channel, source.event_id()) {
            return Ok(DiscoveryCapture::Duplicate);
        }
        if !Self::report_capacity_available(channel) {
            channel.refusal = Some(ChannelRefusal {
                code: ChannelRefusalCode::ResolvedLedgerFull,
                at_unix: unix_secs(),
                refused_live_reports: u32::from(live),
                last_refused_event_id: live.then(|| source.event_id().to_owned()),
            });
            park_terminals(channel);
            if live {
                tracing::error!(
                    target: "csp::team_wake",
                    channel_ref = %scope.channel_ref,
                    source_id = %source.event_id(),
                    code = ?ChannelRefusalCode::ResolvedLedgerFull,
                    "refusing live team-wake report at the verification envelope"
                );
            }
            self.persist()?;
            return Ok(DiscoveryCapture::ResolvedLedgerFull);
        }
        if channel.admitted.len() >= MAX_ADMITTED_PER_CHANNEL {
            return Ok(DiscoveryCapture::Saturated);
        }
        channel.admitted.push(SourceRef { scope, source });
        self.persist()?;
        Ok(DiscoveryCapture::Admitted)
    }

    pub fn capture_report(
        &mut self,
        scope: WakeScope,
        source: WakeSource,
    ) -> io::Result<DiscoveryCapture> {
        self.capture_report_inner(scope, source, false)
    }

    /// Capture a live relay report, durably counting explicit loss while the
    /// channel is structurally refused.
    pub fn capture_live_report(
        &mut self,
        scope: WakeScope,
        source: WakeSource,
    ) -> io::Result<DiscoveryCapture> {
        self.capture_report_inner(scope, source, true)
    }

    /// Durably capture a provider-local terminal fact without relying on relay re-query.
    pub fn capture_terminal(&mut self, scope: WakeScope, source: WakeSource) -> io::Result<bool> {
        debug_assert!(matches!(source, WakeSource::Terminal { .. }));
        let index = self.channel_index_or_insert(scope.channel_ref)?;
        let channel = &mut self.channels[index];
        if channel
            .terminals
            .iter()
            .any(|intent| terminal_key(&intent.source) == terminal_key(&source))
            || channel
                .in_flight
                .as_ref()
                .is_some_and(|intent| terminal_key(&intent.source) == terminal_key(&source))
        {
            return Ok(false);
        }
        if channel.terminals.len() >= MAX_TERMINALS_PER_CHANNEL {
            tracing::error!(
                target: "csp::team_wake",
                channel_ref = %scope.channel_ref,
                source_id = %source.event_id(),
                "refusing terminal wake: provider-controlled terminal sanity cap reached"
            );
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "provider team-wake terminal sanity cap reached",
            ));
        }
        let mut intent = Self::intent(scope, source);
        if channel.refusal.is_some() {
            intent.last_reason = Some("channel_refused".into());
        }
        channel.terminals.push(intent);
        self.persist()?;
        Ok(true)
    }

    pub fn has_terminal_command(
        &self,
        command_id: &str,
        source_target: &CodingSessionTarget,
    ) -> bool {
        self.channels.iter().any(|channel| {
            channel
                .terminals
                .iter()
                .chain(channel.in_flight.iter())
                .any(|intent| {
                    matches!(
                        &intent.source,
                        WakeSource::Terminal {
                            caused_by_command_id,
                            source_target: stored_target,
                            ..
                        } if caused_by_command_id == command_id && stored_target == source_target
                    )
                })
        })
    }

    /// Advance the one memory-only scheduler cursor once for a runtime tick.
    ///
    /// The caller must use the returned channel for both discovery and
    /// processing. Advancing separately for those phases recreates global
    /// starvation when a blocked channel and a saturated channel alternate.
    pub fn next_tick_channel(
        &mut self,
        channels: impl IntoIterator<Item = Uuid>,
    ) -> io::Result<Option<Uuid>> {
        self.ensure_healthy()?;
        let selected = select_after(self.rr_channel, channels);
        if let Some(channel_ref) = selected {
            self.rr_channel = Some(channel_ref);
        }
        Ok(selected)
    }

    /// Promote at most one terminal/report for the channel selected by this
    /// tick. This deliberately does not advance the round-robin cursor.
    pub fn pending_for_channel(&mut self, channel_ref: Uuid) -> io::Result<Option<WakeIntent>> {
        self.ensure_healthy()?;
        let Some(index) = self.channel_index(channel_ref) else {
            return Ok(None);
        };
        let channel = &mut self.channels[index];
        if channel.refusal.is_some() {
            return Ok(None);
        }
        if channel.in_flight.is_none()
            && channel.terminals.is_empty()
            && channel.admitted.is_empty()
        {
            return Ok(None);
        }
        let promoted = channel.in_flight.is_none();
        if channel.in_flight.is_none() {
            let intent = if !channel.admitted.is_empty() {
                let source = channel.admitted.remove(0);
                Self::intent(source.scope, source.source)
            } else {
                channel.terminals.remove(0)
            };
            channel.in_flight = Some(intent);
        }
        let intent = channel.in_flight.clone();
        if promoted {
            self.persist()?;
        }
        Ok(intent)
    }

    /// All durable channels with work are scheduling candidates even if a
    /// socket resubscription has not yet repopulated the live channel set.
    pub fn work_channels(&self) -> impl Iterator<Item = Uuid> + '_ {
        self.channels.iter().filter_map(|channel| {
            (channel.refusal.is_none()
                && (channel.in_flight.is_some()
                    || !channel.terminals.is_empty()
                    || !channel.admitted.is_empty()))
            .then_some(channel.channel_ref)
        })
    }

    pub fn has_work(&self, channel_ref: Uuid) -> bool {
        self.channel_index(channel_ref).is_some_and(|index| {
            let channel = &self.channels[index];
            channel.in_flight.is_some()
                || !channel.terminals.is_empty()
                || !channel.admitted.is_empty()
        })
    }

    pub fn is_refused(&self, channel_ref: Uuid) -> bool {
        self.refusal(channel_ref).is_some()
    }

    pub fn replace_in_flight(&mut self, channel_ref: Uuid, intent: WakeIntent) -> io::Result<()> {
        let index = self.channel_index_required(channel_ref)?;
        if intent.scope.channel_ref != channel_ref {
            return Err(io::Error::other("team-wake intent channel mismatch"));
        }
        self.channels[index].in_flight = Some(intent);
        self.persist()
    }

    pub fn defer_in_flight(&mut self, channel_ref: Uuid, intent: WakeIntent) -> io::Result<()> {
        self.replace_in_flight(channel_ref, intent)
    }

    /// Move an unattempted terminal behind already-admitted durable work.
    ///
    /// This is the migration path for lifecycle terminals captured by the
    /// pre-gating provider. Their initiating 44221 is deliberately absent
    /// from the 44220 inbox, so they can never prove a report requirement.
    /// They remain durable evidence, but may not head-of-line block a report
    /// or a later terminal whose initiating turn can still be verified.
    pub fn park_unattempted_terminal_behind_work(
        &mut self,
        channel_ref: Uuid,
        intent: WakeIntent,
    ) -> io::Result<bool> {
        let index = self.channel_index_required(channel_ref)?;
        let channel = &mut self.channels[index];
        if (channel.admitted.is_empty() && channel.terminals.is_empty())
            || intent.signed_event.is_some()
            || intent.relay_accepted_at.is_some()
            || !matches!(intent.source, WakeSource::Terminal { .. })
        {
            return Ok(false);
        }
        let Some(current) = channel.in_flight.as_ref() else {
            return Ok(false);
        };
        if current.source.event_id() != intent.source.event_id() {
            return Ok(false);
        }
        channel.in_flight = None;
        channel.terminals.push(intent);
        self.persist()?;
        Ok(true)
    }

    /// Resolve the selected source. Report ids enter the permanent ledger; terminals do not.
    pub fn retire_in_flight(&mut self, channel_ref: Uuid) -> io::Result<()> {
        let index = self.channel_index_required(channel_ref)?;
        let Some(intent) = self.channels[index].in_flight.take() else {
            return Ok(());
        };
        if matches!(
            intent.source,
            WakeSource::Report { .. } | WakeSource::Disposition { .. }
        ) {
            let id = intent.source.event_id().to_owned();
            if !self.channels[index].resolved.iter().any(|item| item == &id) {
                self.channels[index].resolved.push(id);
            }
        }
        self.persist()
    }

    fn channel_index_required(&self, channel_ref: Uuid) -> io::Result<usize> {
        self.channel_index(channel_ref)
            .ok_or_else(|| io::Error::other("team-wake channel disappeared"))
    }

    pub fn refusal(&self, channel_ref: Uuid) -> Option<&ChannelRefusal> {
        self.channel_index(channel_ref)
            .and_then(|index| self.channels[index].refusal.as_ref())
    }

    /// Durably refuse a structurally unqueryable channel. Returns true only
    /// for the first transition so the caller can log once.
    pub fn refuse_channel(
        &mut self,
        channel_ref: Uuid,
        code: ChannelRefusalCode,
    ) -> io::Result<bool> {
        let index = self.channel_index_or_insert(channel_ref)?;
        if self.channels[index]
            .refusal
            .as_ref()
            .is_some_and(|r| r.code == code)
        {
            return Ok(false);
        }
        let (refused_live_reports, last_refused_event_id) = self.channels[index]
            .refusal
            .as_ref()
            .map_or((0, None), |refusal| {
                (
                    refusal.refused_live_reports,
                    refusal.last_refused_event_id.clone(),
                )
            });
        self.channels[index].refusal = Some(ChannelRefusal {
            code,
            at_unix: unix_secs(),
            refused_live_reports,
            last_refused_event_id,
        });
        park_terminals(&mut self.channels[index]);
        self.persist()?;
        Ok(true)
    }

    pub fn clear_refusal(&mut self, channel_ref: Uuid) -> io::Result<bool> {
        let Some(index) = self.channel_index(channel_ref) else {
            return Ok(false);
        };
        if self.channels[index].refusal.take().is_none() {
            return Ok(false);
        }
        self.persist()?;
        Ok(true)
    }

    pub fn refused_channels(&self) -> impl Iterator<Item = Uuid> + '_ {
        self.channels
            .iter()
            .filter_map(|channel| channel.refusal.as_ref().map(|_| channel.channel_ref))
    }

    #[cfg(test)]
    pub fn channel_counts(&self, channel_ref: Uuid) -> (usize, usize, usize, usize) {
        self.channel_index(channel_ref)
            .map_or((0, 0, 0, 0), |index| {
                let channel = &self.channels[index];
                (
                    channel.resolved.len(),
                    channel.admitted.len(),
                    usize::from(channel.in_flight.is_some()),
                    channel.terminals.len(),
                )
            })
    }
}

fn valid_reason_detail(detail: Option<&str>) -> bool {
    detail.is_none_or(|value| value.len() <= 1_024 && !value.chars().any(char::is_control))
}

fn select_after(cursor: Option<Uuid>, channels: impl IntoIterator<Item = Uuid>) -> Option<Uuid> {
    let mut channels: Vec<Uuid> = channels.into_iter().collect();
    channels.sort_unstable();
    channels.dedup();
    cursor
        .and_then(|cursor| channels.iter().find(|channel| **channel > cursor).copied())
        .or_else(|| channels.first().copied())
}

fn terminal_key(source: &WakeSource) -> Option<(&str, &CodingSessionTarget)> {
    match source {
        WakeSource::Terminal {
            caused_by_command_id,
            source_target,
            ..
        } => Some((caused_by_command_id, source_target)),
        WakeSource::Report { .. } | WakeSource::Disposition { .. } => None,
    }
}

fn unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

fn park_terminals(channel: &mut ChannelState) {
    if let Some(intent) = &mut channel.in_flight {
        if matches!(intent.source, WakeSource::Terminal { .. }) {
            intent.last_reason = Some("channel_refused".into());
        }
    }
    for intent in &mut channel.terminals {
        intent.last_reason = Some("channel_refused".into());
    }
}
