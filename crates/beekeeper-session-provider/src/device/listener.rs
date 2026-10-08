//! The device engine: standing, once-only commands, and every record the
//! provider signs about a device.
//!
//! The engine is synchronous and owns all device state. It never blocks:
//! simctl, agent-device, image work and uploads run in spawned tasks
//! ([`super::work`]) that report back as [`WorkDone`], and capture runs in
//! [`super::capture`] tasks that report as [`CaptureOutput`]. Each handler
//! returns the events to publish; the connection loop in [`super`] sends
//! them. That keeps it testable without a relay or a simulator.
//!
//! # Standing (provider's decision, WIRE-C5 § 7)
//!
//! - `open`, `close`, `screenshot`: the generation's seat, or a session
//!   person (founder, the create's signer, a granted operator).
//! - `screenshot` also: a granted viewer.
//! - `action`: refused — typed controls are slice S3.
//!
//! Commands for a generation this provider does not serve are ignored, not
//! refused: another provider owns them. Every command this provider *does*
//! serve gets exactly one terminal 44255, made durable in the
//! [`Journal`] before it is sent and re-sent until the relay accepts it.

use std::collections::{BTreeMap, BTreeSet};

use nostr::{Event, Keys};
use tokio::sync::mpsc;

use super::capture::{
    default_cadence, now_ms, spawn_capture, CaptureHandle, CaptureOutput, CaptureTarget,
};
pub use super::journal::{Journal, JournalEntry};
use super::simctl::Simctl;
use super::slot::{remove_slot_file, DevicePaths};
use super::wire::{
    availability_content, decode_command, decode_watch, frame_tags, record_tags, refused_content,
    shot_content, sign, snapshot_tags, AgentDeviceAvailability, DeviceCommand, DeviceOp,
    PlatformAvailability, RecordScope, RecordType, RefusalCode, SnapshotFields, StateContent,
    WatchAction, KIND_SESSION_DEVICE_COMMAND, KIND_SESSION_DEVICE_RECORD, KIND_SURFACE_FRAME,
    KIND_SURFACE_SNAPSHOT, KIND_SURFACE_WATCH, SURFACE_SNAPSHOT_REQUEST_MIN_INTERVAL_MS,
};
use super::work::{OpenFailure, OpenedDevice, ShotRequest, WorkDone, Worker};

/// A command older than this when first seen is answered, not run: the
/// stored filter replays history, and a day-old `open` must not boot a
/// simulator today.
pub const COMMAND_MAX_AGE_SECS: u64 = 600;

/// What the provider tells the device service about one served generation.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeviceSessionView {
    /// Session channel UUID (lowercase text).
    pub channel: String,
    /// Provider session id: the directory the slot file lives in.
    pub session_id: String,
    /// `cs-target` key of the generation.
    pub cs_target: String,
    /// The lifecycle command that minted it (`generation_command_id`, else
    /// the create's `command_id`).
    pub csl_command: String,
    /// The seat this generation runs as.
    pub seat: Option<String>,
    /// Founder, the create's signer, granted operators.
    pub people: BTreeSet<String>,
    /// Granted viewers (snapshot only).
    pub viewers: BTreeSet<String>,
}

impl DeviceSessionView {
    fn scope(&self) -> RecordScope {
        RecordScope {
            channel: self.channel.clone(),
            cs_target: self.cs_target.clone(),
            csl_command: self.csl_command.clone(),
        }
    }

    fn may_drive(&self, author: &str) -> bool {
        self.seat.as_deref() == Some(author) || self.people.contains(author)
    }

    fn may_snapshot(&self, author: &str) -> bool {
        self.may_drive(author) || self.viewers.contains(author)
    }
}

/// One event to send.
#[derive(Debug, Clone)]
pub enum Outgoing {
    /// A stored event; `key` names the journal entry it answers, if any.
    Stored {
        /// Journal key, marked published when every event of it is accepted.
        key: Option<String>,
        /// The event.
        event: Event,
        /// Whether this is the last event of `key`.
        last: bool,
    },
    /// An ephemeral frame: sent once, never retried.
    Ephemeral(Event),
}

/// What this machine offers, probed once off the loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Availability {
    /// iOS Simulator.
    pub ios: PlatformAvailability,
    /// agent-device.
    pub agent: AgentDeviceAvailability,
}

/// One open simulator bound to one session.
#[derive(Debug)]
struct OpenSlot {
    cs_target: String,
    device: OpenedDevice,
    capture: CaptureHandle,
    capture_task: Option<tokio::task::JoinHandle<()>>,
    snapshot_requests: BTreeMap<String, u64>,
}

/// The device engine. See the module docs.
pub struct Engine {
    keys: Keys,
    me: String,
    paths: DevicePaths,
    simctl: Simctl,
    worker: Worker,
    capture_tx: mpsc::Sender<CaptureOutput>,
    journal: Journal,
    views: BTreeMap<String, DeviceSessionView>,
    slots: BTreeMap<String, OpenSlot>,
    busy: BTreeSet<String>,
    availability: Option<Availability>,
    awaiting_availability: BTreeSet<String>,
    cadence: std::time::Duration,
}

/// The key of a command's journal entry.
fn command_key(cs_target: &str, sdv_cmd: &str) -> String {
    format!("cmd|{cs_target}|{sdv_cmd}")
}

fn availability_key(cs_target: &str) -> String {
    format!("availability|{cs_target}")
}

fn android_unavailable() -> PlatformAvailability {
    PlatformAvailability {
        available: false,
        reason: Some("Android emulators are not offered by this provider yet".into()),
    }
}

impl Engine {
    /// An engine signing as `keys`, persisting to `journal`.
    pub fn new(
        keys: Keys,
        paths: DevicePaths,
        simctl: Simctl,
        worker: Worker,
        capture_tx: mpsc::Sender<CaptureOutput>,
        journal: Journal,
    ) -> Self {
        let me = keys.public_key().to_hex();
        Self {
            keys,
            me,
            paths,
            simctl,
            worker,
            capture_tx,
            journal,
            views: BTreeMap::new(),
            slots: BTreeMap::new(),
            busy: BTreeSet::new(),
            availability: None,
            awaiting_availability: BTreeSet::new(),
            cadence: default_cadence(),
        }
    }

    /// Shorten the capture cadence (tests).
    pub fn set_cadence(&mut self, cadence: std::time::Duration) {
        self.cadence = cadence;
    }

    /// This provider's pubkey.
    pub fn me(&self) -> &str {
        &self.me
    }

    /// The channels the subscription must cover.
    pub fn channels(&self) -> BTreeSet<String> {
        self.views
            .values()
            .map(|view| view.channel.clone())
            .collect()
    }

    /// The relay filters for the current views, or `None` when nothing is
    /// served.
    pub fn filters(&self) -> Option<serde_json::Value> {
        let channels: Vec<String> = self.channels().into_iter().collect();
        if channels.is_empty() {
            return None;
        }
        Some(serde_json::json!([
            {"kinds": [KIND_SESSION_DEVICE_COMMAND], "#h": channels},
            {"kinds": [KIND_SURFACE_WATCH], "#h": channels, "#p": [self.me]},
        ]))
    }

    /// Answer every command a previous process left running: the work it
    /// started cannot be trusted to have finished.
    pub fn recover(&mut self) -> Vec<Outgoing> {
        let mut out = Vec::new();
        for key in self.journal.running_keys() {
            let Some(entry) = self.journal.entry(&key).cloned() else {
                continue;
            };
            let scope = RecordScope {
                channel: entry.channel,
                cs_target: entry.cs_target,
                csl_command: entry.csl_command,
            };
            let refusal = self.sign_record(
                &scope,
                RecordType::Refused,
                None,
                entry.sdv_cmd.as_deref(),
                None,
                refused_content(
                    RefusalCode::Busy,
                    "the provider restarted before this command finished; send it again",
                ),
            );
            out.extend(self.answer(&key, refusal.into_iter().collect()));
        }
        out
    }

    /// Replace the served generations. New ones get an availability record
    /// (once per generation); gone ones have their device unbound.
    pub fn serve(&mut self, views: Vec<DeviceSessionView>) -> Vec<Outgoing> {
        let mut out = Vec::new();
        let next: BTreeMap<String, DeviceSessionView> = views
            .into_iter()
            .map(|view| (view.cs_target.clone(), view))
            .collect();
        let gone: Vec<String> = self
            .slots
            .iter()
            .filter(|(_, slot)| !next.contains_key(&slot.cs_target))
            .map(|(session, _)| session.clone())
            .collect();
        for session in gone {
            let Some(slot) = self.slots.remove(&session) else {
                continue;
            };
            if let Some(view) = self.views.get(&slot.cs_target).cloned() {
                out.extend(self.unbind(&view, slot, "the session generation ended"));
            }
        }
        for view in next.values() {
            if !self.views.contains_key(&view.cs_target)
                && !self.slots.contains_key(&view.session_id)
            {
                self.restore_slot(view);
            }
        }
        self.views = next;
        let targets: Vec<String> = self.views.keys().cloned().collect();
        for cs_target in targets {
            if !self.journal.contains(&availability_key(&cs_target)) {
                self.awaiting_availability.insert(cs_target);
            }
        }
        if self.availability.is_some() {
            out.extend(self.publish_awaiting_availability());
        } else if !self.awaiting_availability.is_empty() {
            self.worker.probe();
        }
        out
    }

    /// A provider restart forgets open devices, but their slot files remain:
    /// re-adopt one so a screenshot after the restart still answers.
    fn restore_slot(&mut self, view: &DeviceSessionView) {
        let dir = self.paths.session_dir(&view.session_id);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(slot) = name
                .strip_suffix(".json")
                .filter(|slot| super::wire::valid_slot(slot))
            else {
                continue;
            };
            let Ok(file) = super::slot::read_slot_file(&self.paths, &view.session_id, slot) else {
                continue;
            };
            let device = OpenedDevice {
                slot: file.slot,
                udid: file.udid,
                model: file.model,
                os: "iOS".into(),
                os_version: file.os_version.trim_start_matches("iOS ").to_owned(),
                agent_ready: file.agent_device_config.is_some(),
            };
            tracing::info!(target: "csp::device", slot = %device.slot, "re-adopted an open device after restart");
            self.slots.insert(
                view.session_id.clone(),
                OpenSlot {
                    cs_target: view.cs_target.clone(),
                    device,
                    capture: CaptureHandle::default(),
                    capture_task: None,
                    snapshot_requests: BTreeMap::new(),
                },
            );
            return;
        }
    }

    fn publish_awaiting_availability(&mut self) -> Vec<Outgoing> {
        let Some(availability) = self.availability.clone() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for cs_target in std::mem::take(&mut self.awaiting_availability) {
            let Some(view) = self.views.get(&cs_target).cloned() else {
                continue;
            };
            let key = availability_key(&cs_target);
            if self.journal.contains(&key) {
                continue;
            }
            self.journal.begin(&key, &view, None);
            let event = self.availability_event(&view, &availability);
            out.extend(self.answer(&key, event.into_iter().collect()));
        }
        out
    }

    /// Handle one relay event (44254 or 24320).
    pub fn on_event(&mut self, event: &Event) -> Vec<Outgoing> {
        let kind = event.kind.as_u16();
        if kind == KIND_SESSION_DEVICE_COMMAND {
            return match decode_command(event) {
                Ok(command) => self.on_command(command, event.created_at.as_secs()),
                Err(reason) => {
                    tracing::debug!(target: "csp::device", event_id = %event.id, "device command skipped: {reason}");
                    Vec::new()
                }
            };
        }
        if kind == KIND_SURFACE_WATCH {
            return match decode_watch(event, &self.me) {
                Ok(request) => self.on_watch(request),
                Err(reason) => {
                    tracing::debug!(target: "csp::device", "surface watch skipped: {reason}");
                    Vec::new()
                }
            };
        }
        Vec::new()
    }

    fn refuse(
        &mut self,
        key: &str,
        view: &DeviceSessionView,
        command: &DeviceCommand,
        code: RefusalCode,
        reason: &str,
    ) -> Vec<Outgoing> {
        let event = self.sign_record(
            &view.scope(),
            RecordType::Refused,
            command.slot.as_deref(),
            Some(&command.sdv_cmd),
            None,
            refused_content(code, reason),
        );
        self.answer(key, event.into_iter().collect())
    }

    fn on_command(&mut self, command: DeviceCommand, created_at: u64) -> Vec<Outgoing> {
        let Some(view) = self.views.get(&command.cs_target).cloned() else {
            return Vec::new();
        };
        if view.channel != command.channel {
            return Vec::new();
        }
        let key = command_key(&command.cs_target, &command.sdv_cmd);
        if self.journal.contains(&key) {
            return Vec::new();
        }
        self.journal.begin(&key, &view, Some(&command.sdv_cmd));
        let now = now_ms() / 1000;
        if now.saturating_sub(created_at) > COMMAND_MAX_AGE_SECS {
            return self.refuse(
                &key,
                &view,
                &command,
                RefusalCode::InvalidCommand,
                "this command is too old to run; send it again",
            );
        }
        let allowed = match command.op {
            DeviceOp::Screenshot => view.may_snapshot(&command.author),
            _ => view.may_drive(&command.author),
        };
        if !allowed {
            return self.refuse(
                &key,
                &view,
                &command,
                RefusalCode::NoStanding,
                "Only a seat of this session or its person may drive the device",
            );
        }
        match command.op.clone() {
            DeviceOp::Open { platform, model } => self.open(key, view, command, &platform, model),
            DeviceOp::Close { shutdown } => self.close(key, view, command, shutdown),
            DeviceOp::Screenshot => self.screenshot(key, view, command),
            DeviceOp::Action { .. } => self.refuse(
                &key,
                &view,
                &command,
                RefusalCode::InvalidCommand,
                "device controls are not offered by this provider yet",
            ),
        }
    }

    fn open(
        &mut self,
        key: String,
        view: DeviceSessionView,
        command: DeviceCommand,
        platform: &str,
        model: Option<String>,
    ) -> Vec<Outgoing> {
        if platform != "ios" {
            let reason = android_unavailable().reason.unwrap_or_default();
            return self.refuse(
                &key,
                &view,
                &command,
                RefusalCode::PlatformUnavailable,
                &reason,
            );
        }
        if let Some(Availability {
            ios:
                PlatformAvailability {
                    available: false,
                    reason,
                },
            ..
        }) = &self.availability
        {
            let reason = reason
                .clone()
                .unwrap_or_else(|| "the iOS Simulator is not available".into());
            return self.refuse(
                &key,
                &view,
                &command,
                RefusalCode::PlatformUnavailable,
                &reason,
            );
        }
        if let Some(slot) = self.slots.get(&view.session_id) {
            let same = model
                .as_deref()
                .is_none_or(|model| model.eq_ignore_ascii_case(&slot.device.model));
            if !same {
                return self.refuse(
                    &key,
                    &view,
                    &command,
                    RefusalCode::Busy,
                    "a device is already open in this session; close it first",
                );
            }
            let device = slot.device.clone();
            let event = self.state_event(&view, &device, "open", Some(&command.sdv_cmd), None);
            return self.answer(&key, event.into_iter().collect());
        }
        if !self.busy.insert(view.session_id.clone()) {
            return self.refuse(
                &key,
                &view,
                &command,
                RefusalCode::Busy,
                "another device command for this session is still running",
            );
        }
        self.worker.open(
            key,
            view.cs_target.clone(),
            view.session_id.clone(),
            command.sdv_cmd.clone(),
            model,
        );
        Vec::new()
    }

    fn close(
        &mut self,
        key: String,
        view: DeviceSessionView,
        command: DeviceCommand,
        shutdown: bool,
    ) -> Vec<Outgoing> {
        let matches = self
            .slots
            .get(&view.session_id)
            .is_some_and(|slot| Some(&slot.device.slot) == command.slot.as_ref());
        if !matches {
            return self.refuse(
                &key,
                &view,
                &command,
                RefusalCode::NoDeviceOpen,
                "no device is open in this slot",
            );
        }
        let Some(slot) = self.slots.remove(&view.session_id) else {
            return Vec::new();
        };
        stop_capture(&slot);
        remove_slot_file(&self.paths, &view.session_id, &slot.device.slot);
        if shutdown {
            self.busy.insert(view.session_id.clone());
            self.worker.close(
                key,
                view.cs_target.clone(),
                command.sdv_cmd.clone(),
                slot.device,
            );
            return Vec::new();
        }
        let event = self.state_event(&view, &slot.device, "closed", Some(&command.sdv_cmd), None);
        self.answer(&key, event.into_iter().collect())
    }

    fn screenshot(
        &mut self,
        key: String,
        view: DeviceSessionView,
        command: DeviceCommand,
    ) -> Vec<Outgoing> {
        let Some(slot) = self
            .slots
            .get(&view.session_id)
            .filter(|slot| Some(&slot.device.slot) == command.slot.as_ref())
        else {
            return self.refuse(
                &key,
                &view,
                &command,
                RefusalCode::NoDeviceOpen,
                "no device is open in this slot",
            );
        };
        self.worker.shot(ShotRequest {
            key: Some(key),
            cs_target: view.cs_target.clone(),
            session_id: view.session_id.clone(),
            sdv_cmd: Some(command.sdv_cmd.clone()),
            command_event: Some(command.event_id.clone()),
            requested_by: command.author.clone(),
            device: slot.device.clone(),
        });
        Vec::new()
    }

    fn on_watch(&mut self, request: super::wire::WatchRequest) -> Vec<Outgoing> {
        let Some((session, view)) = self.slots.iter().find_map(|(session, slot)| {
            let view = self.views.get(&slot.cs_target)?;
            (slot.device.slot == request.slot && view.channel == request.channel)
                .then(|| (session.clone(), view.clone()))
        }) else {
            return Vec::new();
        };
        let now = now_ms();
        let simctl = self.simctl.clone();
        let cadence = self.cadence;
        let capture_tx = self.capture_tx.clone();
        let Some(slot) = self.slots.get_mut(&session) else {
            return Vec::new();
        };
        match request.action {
            WatchAction::Stop => {
                if let Ok(mut watchers) = slot.capture.watchers.lock() {
                    watchers.stop(&request.watcher);
                }
                slot.capture.wake.notify_one();
                return Vec::new();
            }
            WatchAction::Snapshot => {
                let last = slot.snapshot_requests.get(&request.watcher).copied();
                if last.is_some_and(|at| {
                    now.saturating_sub(at) < SURFACE_SNAPSHOT_REQUEST_MIN_INTERVAL_MS
                }) {
                    return Vec::new();
                }
                slot.snapshot_requests.insert(request.watcher.clone(), now);
                let device = slot.device.clone();
                self.worker.shot(ShotRequest {
                    key: None,
                    cs_target: view.cs_target.clone(),
                    session_id: view.session_id.clone(),
                    sdv_cmd: None,
                    command_event: None,
                    requested_by: request.watcher,
                    device,
                });
                return Vec::new();
            }
            WatchAction::Resync => {
                slot.capture
                    .resync
                    .store(true, std::sync::atomic::Ordering::SeqCst);
            }
            WatchAction::Watch => {}
        }
        if let Ok(mut watchers) = slot.capture.watchers.lock() {
            watchers.touch(&request.watcher, now);
        }
        let running = slot
            .capture_task
            .as_ref()
            .is_some_and(|task| !task.is_finished());
        if running {
            slot.capture.wake.notify_one();
        } else {
            slot.capture = CaptureHandle {
                watchers: slot.capture.watchers.clone(),
                ..CaptureHandle::default()
            };
            let target = CaptureTarget {
                channel: view.channel.clone(),
                slot: slot.device.slot.clone(),
                udid: slot.device.udid.clone(),
            };
            slot.capture_task = Some(spawn_capture(
                simctl,
                target,
                slot.capture.clone(),
                capture_tx,
                cadence,
            ));
        }
        Vec::new()
    }

    /// Handle a capture task's output.
    pub fn on_capture(&mut self, output: CaptureOutput) -> Vec<Outgoing> {
        match output {
            CaptureOutput::Frame(header, kind, content) => sign(
                &self.keys,
                KIND_SURFACE_FRAME,
                frame_tags(&header, kind),
                content,
            )
            .map_err(|error| tracing::warn!(target: "csp::device", "frame not signed: {error}"))
            .map(|event| vec![Outgoing::Ephemeral(event)])
            .unwrap_or_default(),
            CaptureOutput::Ended { .. } => Vec::new(),
        }
    }

    /// Handle finished work.
    pub fn on_work(&mut self, done: WorkDone) -> Vec<Outgoing> {
        match done {
            WorkDone::Probed(availability) => {
                self.availability = Some(availability);
                self.publish_awaiting_availability()
            }
            WorkDone::Booting {
                cs_target,
                sdv_cmd,
                device,
            } => {
                let Some(view) = self.views.get(&cs_target).cloned() else {
                    return Vec::new();
                };
                self.state_event(&view, &device, "booting", Some(&sdv_cmd), None)
                    .map(|event| {
                        vec![Outgoing::Stored {
                            key: None,
                            event,
                            last: true,
                        }]
                    })
                    .unwrap_or_default()
            }
            WorkDone::Opened {
                key,
                cs_target,
                session_id,
                sdv_cmd,
                result,
            } => {
                self.busy.remove(&session_id);
                let Some(view) = self.views.get(&cs_target).cloned() else {
                    return self.answer_orphan(&key, &sdv_cmd);
                };
                match result {
                    Ok(device) => {
                        let mut out = Vec::new();
                        if device.agent_ready
                            && self
                                .availability
                                .as_ref()
                                .is_some_and(|a| !a.agent.installed)
                        {
                            out.extend(self.agent_now_installed(&view));
                        }
                        let event = self.state_event(&view, &device, "open", Some(&sdv_cmd), None);
                        self.slots.insert(
                            session_id,
                            OpenSlot {
                                cs_target,
                                device,
                                capture: CaptureHandle::default(),
                                capture_task: None,
                                snapshot_requests: BTreeMap::new(),
                            },
                        );
                        out.extend(self.answer(&key, event.into_iter().collect()));
                        out
                    }
                    Err(OpenFailure {
                        device: Some(device),
                        reason,
                    }) => {
                        let event = self.state_event(
                            &view,
                            &device,
                            "failed",
                            Some(&sdv_cmd),
                            Some(&reason),
                        );
                        self.answer(&key, event.into_iter().collect())
                    }
                    Err(OpenFailure {
                        device: None,
                        reason,
                    }) => {
                        let event = self.sign_record(
                            &view.scope(),
                            RecordType::Refused,
                            None,
                            Some(&sdv_cmd),
                            None,
                            refused_content(RefusalCode::PlatformUnavailable, &reason),
                        );
                        self.answer(&key, event.into_iter().collect())
                    }
                }
            }
            WorkDone::Closed {
                key,
                cs_target,
                sdv_cmd,
                device,
                result,
            } => {
                self.busy.retain(|session| self.slots.contains_key(session));
                let Some(view) = self.views.get(&cs_target).cloned() else {
                    return self.answer_orphan(&key, &sdv_cmd);
                };
                let reason = result.err();
                let event =
                    self.state_event(&view, &device, "closed", Some(&sdv_cmd), reason.as_deref());
                self.answer(&key, event.into_iter().collect())
            }
            WorkDone::Shot { request, result } => self.on_shot(request, result),
        }
    }

    fn answer(&mut self, key: &str, events: Vec<Event>) -> Vec<Outgoing> {
        self.journal.answer(key, &events);
        let count = events.len();
        events
            .into_iter()
            .enumerate()
            .map(|(index, event)| Outgoing::Stored {
                key: Some(key.to_owned()),
                event,
                last: index + 1 == count,
            })
            .collect()
    }

    /// The relay accepted every event of `key`.
    pub fn mark_published(&mut self, key: &str) {
        self.journal.mark_published(key);
    }

    /// Answered-but-unaccepted events, for a (re)connect.
    pub fn unpublished(&self) -> Vec<Outgoing> {
        self.journal
            .unpublished()
            .into_iter()
            .flat_map(|(key, events)| {
                let count = events.len();
                events
                    .into_iter()
                    .enumerate()
                    .map(move |(index, event)| Outgoing::Stored {
                        key: Some(key.clone()),
                        event,
                        last: index + 1 == count,
                    })
            })
            .collect()
    }

    /// Stop every capture task (shutdown).
    pub fn stop_all(&mut self) {
        for slot in self.slots.values() {
            stop_capture(slot);
        }
    }

    /// The shim directory for a session's seat PATH, when a device is open.
    pub fn slot_of(&self, session_id: &str) -> Option<&OpenedDevice> {
        self.slots.get(session_id).map(|slot| &slot.device)
    }
}

fn stop_capture(slot: &OpenSlot) {
    slot.capture
        .closed
        .store(true, std::sync::atomic::Ordering::SeqCst);
    slot.capture.wake.notify_one();
}

#[path = "listener_records.rs"]
mod records;

#[cfg(test)]
#[path = "listener_tests.rs"]
mod tests;
