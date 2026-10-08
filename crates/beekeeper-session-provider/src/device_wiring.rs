//! C5 (SV-34): the provider's handle on [`crate::device::DeviceService`].
//!
//! Started once beside the host-step listener, on macOS only (elsewhere no
//! availability record is published, which the Device surface reads as "this
//! machine's provider does not offer devices" — the truth). Told on every
//! runtime tick which generations are *live* here ([`DeviceService::serve`]
//! is a no-op when nothing changed): only a running runtime has a seat that
//! can drive a device, and serving every stored record would publish one
//! availability record per historical session in a burst on the provider's
//! own key. With nothing served the service holds no relay subscription, and
//! capture runs only while someone watches.

use std::collections::BTreeSet;

use beekeeper_core::coding_session_command::coding_session_target_key;

use crate::device::{DeviceConfig, DeviceService, DeviceSessionView};
use crate::state::SessionRecord;
use crate::Provider;

impl Provider {
    /// Start the device service. Idempotent.
    pub fn start_device_service(&mut self) {
        // Not under the crate's own tests either: the provider-loop tests
        // would otherwise run the real simctl on the test machine (the
        // device module has its own fake-runner tests).
        if self.device_service.is_some() || !cfg!(target_os = "macos") || cfg!(test) {
            return;
        }
        self.device_service = Some(DeviceService::spawn(DeviceConfig {
            relay_url: self.config.relay_url.clone(),
            keys: self.config.keys.clone(),
            auth_tag: self.config.auth_tag.clone(),
            state_dir: self.config.state_dir.clone(),
            disabled: None,
        }));
        self.sync_device_service();
    }

    /// Tell the device service exactly which generations this host serves.
    pub(crate) fn sync_device_service(&self) {
        let Some(service) = &self.device_service else {
            return;
        };
        let live: BTreeSet<&str> = self.sessions.live_session_ids().collect();
        let views = self
            .state
            .sessions()
            .filter(|record| live.contains(record.session_id.as_str()))
            .filter(|record| !record.closed && !record.is_retired())
            .map(|record| device_session_view(record, &self.config.instance_id))
            .collect();
        service.serve(views);
    }
}

/// One served generation as the device service sees it: the seat drives;
/// the founder, the create's signer and granted operators drive; granted
/// viewers may only ask for a snapshot.
pub(crate) fn device_session_view(record: &SessionRecord, instance_id: &str) -> DeviceSessionView {
    let mut people: BTreeSet<String> = record.granted_operators.clone();
    people.extend(record.founder_pubkey.iter().cloned());
    people.extend(record.created_by.iter().cloned());
    DeviceSessionView {
        channel: record.channel_id.to_string(),
        session_id: record.session_id.clone(),
        cs_target: coding_session_target_key(&record.target(instance_id)),
        csl_command: record.generation_command_id().to_owned(),
        seat: record.actor.clone(),
        people,
        viewers: record.granted_viewers.clone(),
    }
}
