//! The records the device engine signs: state, refusals, availability and
//! the snapshot pair. A child module of [`super`] so it shares the engine's
//! private state without widening it.

use super::*;

impl Engine {
    pub(super) fn on_shot(
        &mut self,
        request: ShotRequest,
        result: Result<crate::artifact_upload::UploadedImage, String>,
    ) -> Vec<Outgoing> {
        let Some(view) = self.views.get(&request.cs_target).cloned() else {
            return request
                .key
                .map(|key| self.answer_orphan(&key, request.sdv_cmd.as_deref().unwrap_or_default()))
                .unwrap_or_default();
        };
        let image = match result {
            Ok(image) => image,
            Err(reason) => {
                tracing::warn!(target: "csp::device", slot = %request.device.slot, "device snapshot failed: {reason}");
                let Some(key) = request.key else {
                    return Vec::new();
                };
                let event = self.sign_record(
                    &view.scope(),
                    RecordType::Refused,
                    Some(&request.device.slot),
                    request.sdv_cmd.as_deref(),
                    None,
                    refused_content(
                        RefusalCode::CaptureFailed,
                        &format!(
                            "the snapshot could not be taken: {}",
                            crate::device::slot::scrub_host_details(&reason)
                        ),
                    ),
                );
                return self.answer(&key, event.into_iter().collect());
            }
        };
        let fields = SnapshotFields {
            channel: &view.channel,
            slot: &request.device.slot,
            sha256: &image.sha256,
            url: &image.url,
            mime: image.mime,
            width: image.width,
            height: image.height,
            taken_at_ms: now_ms(),
            provider: &self.me,
            requested_by: Some(&request.requested_by),
            command_event: request.command_event.as_deref(),
        };
        let alt = format!("{} ({})", request.device.model, request.device.os_label());
        let snapshot = match sign(
            &self.keys,
            KIND_SURFACE_SNAPSHOT,
            snapshot_tags(&fields),
            alt,
        ) {
            Ok(event) => event,
            Err(error) => {
                tracing::warn!(target: "csp::device", "snapshot not signed: {error}");
                return Vec::new();
            }
        };
        let Some(key) = request.key else {
            return vec![Outgoing::Stored {
                key: None,
                event: snapshot,
                last: true,
            }];
        };
        let snapshot_id = snapshot.id.to_hex();
        let shot = self.sign_record(
            &view.scope(),
            RecordType::Shot,
            Some(&request.device.slot),
            request.sdv_cmd.as_deref(),
            Some(&snapshot_id),
            shot_content(),
        );
        let mut events = vec![snapshot];
        events.extend(shot);
        self.answer(&key, events)
    }

    /// A command whose generation stopped being served while it ran still
    /// gets its terminal answer, scoped from the journal entry.
    pub(super) fn answer_orphan(&mut self, key: &str, sdv_cmd: &str) -> Vec<Outgoing> {
        let Some(entry) = self.journal.entry(key).cloned() else {
            return Vec::new();
        };
        let scope = RecordScope {
            channel: entry.channel,
            cs_target: entry.cs_target,
            csl_command: entry.csl_command,
        };
        let event = self.sign_record(
            &scope,
            RecordType::Refused,
            None,
            Some(sdv_cmd),
            None,
            refused_content(
                RefusalCode::NoDeviceOpen,
                "the session generation ended before the device answered",
            ),
        );
        self.answer(key, event.into_iter().collect())
    }

    pub(super) fn unbind(
        &mut self,
        view: &DeviceSessionView,
        slot: OpenSlot,
        reason: &str,
    ) -> Vec<Outgoing> {
        stop_capture(&slot);
        remove_slot_file(&self.paths, &view.session_id, &slot.device.slot);
        self.state_event(view, &slot.device, "closed", None, Some(reason))
            .map(|event| {
                vec![Outgoing::Stored {
                    key: None,
                    event,
                    last: true,
                }]
            })
            .unwrap_or_default()
    }

    pub(super) fn state_event(
        &self,
        view: &DeviceSessionView,
        device: &OpenedDevice,
        state: &'static str,
        sdv_cmd: Option<&str>,
        reason: Option<&str>,
    ) -> Option<Event> {
        let mut drivers = Vec::new();
        if device.agent_ready {
            drivers.push("agent");
        }
        drivers.push("host-owner");
        let content = StateContent {
            state,
            platform: "ios".into(),
            model: device.model.clone(),
            os_version: device.os_version.clone(),
            drivers,
            reason: reason.map(crate::device::slot::scrub_host_details),
        };
        self.sign_record(
            &view.scope(),
            RecordType::State,
            Some(&device.slot),
            sdv_cmd,
            None,
            content.to_json(),
        )
    }

    pub(super) fn sign_record(
        &self,
        scope: &RecordScope,
        kind: RecordType,
        slot: Option<&str>,
        sdv_cmd: Option<&str>,
        snapshot: Option<&str>,
        content: String,
    ) -> Option<Event> {
        sign(
            &self.keys,
            KIND_SESSION_DEVICE_RECORD,
            record_tags(scope, kind, slot, sdv_cmd, snapshot),
            content,
        )
        .map_err(|error| tracing::warn!(target: "csp::device", "device record not signed: {error}"))
        .ok()
    }

    pub(super) fn availability_event(
        &self,
        view: &DeviceSessionView,
        availability: &Availability,
    ) -> Option<Event> {
        self.sign_record(
            &view.scope(),
            RecordType::Availability,
            None,
            None,
            None,
            availability_content(
                &availability.ios,
                &android_unavailable(),
                &availability.agent,
            ),
        )
    }

    pub(super) fn agent_now_installed(&mut self, view: &DeviceSessionView) -> Vec<Outgoing> {
        let Some(availability) = self.availability.as_mut() else {
            return Vec::new();
        };
        availability.agent.installed = true;
        availability.agent.reason = None;
        let availability = availability.clone();
        self.availability_event(view, &availability)
            .map(|event| {
                vec![Outgoing::Stored {
                    key: None,
                    event,
                    last: true,
                }]
            })
            .unwrap_or_default()
    }
}
