//! The device engine's slow work, each piece on its own task.
//!
//! Every simctl call, npm install, daemon start, image re-encode and upload
//! runs here — on `spawn_blocking` for the blocking parts — and reports back
//! to the engine as a [`WorkDone`] over a channel. Nothing here touches
//! engine state, so no simctl or relay await ever sits inside an event loop
//! (SV-72/76).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tokio::sync::mpsc;

use super::agent_device::{ensure_daemon, session_name, write_agent_device_config, write_shim};
use super::listener::Availability;
use super::simctl::{choose_device, CommandRunner, ShotFormat, Simctl, XcodeProbe};
use super::slot::{slot_id, write_slot_file, DevicePaths, SlotFile, SLOT_FILE_VERSION};
use super::toolchain::{
    agent_device_paths, ensure_agent_device, is_installed, resolve_node, DeveloperDirs,
    AGENT_DEVICE_VERSION,
};
use super::wire::{AgentDeviceAvailability, PlatformAvailability};
use crate::artifact_upload::{prepare_image, ArtifactUploader, UploadedImage};

/// A simulator bound to a session's slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenedDevice {
    /// Opaque slot id.
    pub slot: String,
    /// UDID (host-local).
    pub udid: String,
    /// Model name.
    pub model: String,
    /// `iOS`.
    pub os: String,
    /// `27.0`.
    pub os_version: String,
    /// agent-device is installed, its daemon is up, and the shim is written.
    pub agent_ready: bool,
}

impl OpenedDevice {
    /// `iOS 27.0`.
    pub fn os_label(&self) -> String {
        format!("{} {}", self.os, self.os_version)
    }
}

/// Why an open failed, with the device when one had been chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenFailure {
    /// The chosen device, when the failure came after choosing.
    pub device: Option<Box<OpenedDevice>>,
    /// Publishable reason.
    pub reason: String,
}

/// A durable snapshot to take.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShotRequest {
    /// Journal key, for a 44254 `screenshot`; `None` for a watcher's request.
    pub key: Option<String>,
    /// Generation.
    pub cs_target: String,
    /// Session (the slot file's directory).
    pub session_id: String,
    /// The command id, when a command asked.
    pub sdv_cmd: Option<String>,
    /// The 44254 event id, when a command asked.
    pub command_event: Option<String>,
    /// Who asked.
    pub requested_by: String,
    /// The device.
    pub device: OpenedDevice,
}

/// What finished work reports.
#[derive(Debug)]
pub enum WorkDone {
    /// The machine probe finished.
    Probed(Availability),
    /// An open chose its device and is booting it.
    Booting {
        /// Generation.
        cs_target: String,
        /// Command id.
        sdv_cmd: String,
        /// The device.
        device: OpenedDevice,
    },
    /// An open finished.
    Opened {
        /// Journal key.
        key: String,
        /// Generation.
        cs_target: String,
        /// Session.
        session_id: String,
        /// Command id.
        sdv_cmd: String,
        /// The device, or why not.
        result: Result<OpenedDevice, OpenFailure>,
    },
    /// A close with shutdown finished.
    Closed {
        /// Journal key.
        key: String,
        /// Generation.
        cs_target: String,
        /// Command id.
        sdv_cmd: String,
        /// The device.
        device: OpenedDevice,
        /// The shutdown's outcome.
        result: Result<(), String>,
    },
    /// A snapshot finished.
    Shot {
        /// What was asked.
        request: ShotRequest,
        /// Where it is served, or why not.
        result: Result<UploadedImage, String>,
    },
}

/// Host facts the worker needs that do not change.
#[derive(Clone)]
pub struct WorkerConfig {
    /// This provider's pubkey hex (the slot id's salt).
    pub me: String,
    /// Device paths.
    pub paths: DevicePaths,
    /// Where `node` may be (see [`super::toolchain::node_candidates`]).
    pub node_candidates: Vec<PathBuf>,
    /// `Some(reason)` when this host turned devices off.
    pub disabled: Option<String>,
    /// Where the probe looks for Xcode before it may run `xcrun`.
    pub developer_dirs: DeveloperDirs,
}

/// Spawns the work and reports it.
#[derive(Clone)]
pub struct Worker {
    config: WorkerConfig,
    simctl: Simctl,
    runner: Arc<dyn CommandRunner>,
    uploader: Option<ArtifactUploader>,
    tx: mpsc::Sender<WorkDone>,
    probing: Arc<AtomicBool>,
}

impl Worker {
    /// A worker reporting on `tx`.
    pub fn new(
        config: WorkerConfig,
        runner: Arc<dyn CommandRunner>,
        uploader: Option<ArtifactUploader>,
        tx: mpsc::Sender<WorkDone>,
    ) -> Self {
        let simctl = Simctl::with_developer_dirs(runner.clone(), config.developer_dirs.clone());
        Self {
            config,
            simctl,
            runner,
            uploader,
            tx,
            probing: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Probe what this machine offers (once at a time).
    pub fn probe(&self) {
        if self.probing.swap(true, Ordering::SeqCst) {
            return;
        }
        let this = self.clone();
        tokio::spawn(async move {
            let probe = this.clone();
            let availability = tokio::task::spawn_blocking(move || probe.probe_blocking())
                .await
                .unwrap_or_else(|error| Availability {
                    ios: PlatformAvailability {
                        available: false,
                        reason: Some(format!("the device probe failed: {error}")),
                    },
                    agent: AgentDeviceAvailability {
                        installed: false,
                        version: None,
                        reason: Some("the device probe failed".into()),
                    },
                });
            this.probing.store(false, Ordering::SeqCst);
            let _ = this.tx.send(WorkDone::Probed(availability)).await;
        });
    }

    /// The probe itself. Blocking; never boots or installs anything.
    pub fn probe_blocking(&self) -> Availability {
        if let Some(reason) = &self.config.disabled {
            return Availability {
                ios: PlatformAvailability {
                    available: false,
                    reason: Some(reason.clone()),
                },
                agent: AgentDeviceAvailability {
                    installed: false,
                    version: None,
                    reason: Some(reason.clone()),
                },
            };
        }
        let ios = match self.simctl.probe() {
            XcodeProbe::Found => PlatformAvailability {
                available: true,
                reason: None,
            },
            XcodeProbe::Missing(reason) => PlatformAvailability {
                available: false,
                reason: Some(reason),
            },
        };
        let agent = match resolve_node(self.runner.as_ref(), &self.config.node_candidates) {
            Ok(_) => AgentDeviceAvailability {
                installed: is_installed(&agent_device_paths(&self.config.paths.tools_dir())),
                version: Some(AGENT_DEVICE_VERSION.into()),
                reason: None,
            },
            Err(reason) => AgentDeviceAvailability {
                installed: false,
                version: None,
                reason: Some(reason),
            },
        };
        Availability { ios, agent }
    }

    /// Open a device for `session_id`.
    pub fn open(
        &self,
        key: String,
        cs_target: String,
        session_id: String,
        sdv_cmd: String,
        model: Option<String>,
    ) {
        let this = self.clone();
        tokio::spawn(async move {
            let worker = this.clone();
            let (target, session, cmd) = (cs_target.clone(), session_id.clone(), sdv_cmd.clone());
            let result = tokio::task::spawn_blocking(move || {
                worker.open_blocking(&target, &session, &cmd, model.as_deref())
            })
            .await
            .unwrap_or_else(|error| {
                Err(OpenFailure {
                    device: None,
                    reason: format!("the open did not finish: {error}"),
                })
            });
            let _ = this
                .tx
                .send(WorkDone::Opened {
                    key,
                    cs_target,
                    session_id,
                    sdv_cmd,
                    result,
                })
                .await;
        });
    }

    /// The open itself. Blocking.
    pub fn open_blocking(
        &self,
        cs_target: &str,
        session_id: &str,
        sdv_cmd: &str,
        model: Option<&str>,
    ) -> Result<OpenedDevice, OpenFailure> {
        let fail = |device: Option<&OpenedDevice>, reason: String| OpenFailure {
            device: device.cloned().map(Box::new),
            reason,
        };
        let devices = self.simctl.list().map_err(|reason| fail(None, reason))?;
        let chosen = choose_device(&devices, model).ok_or_else(|| {
            fail(
                None,
                match model {
                    Some(model) => format!("no available simulator named {model} on this machine"),
                    None => "no available iPhone simulator on this machine".into(),
                },
            )
        })?;
        let mut device = OpenedDevice {
            slot: slot_id(&self.config.me, &chosen.udid),
            udid: chosen.udid.clone(),
            model: chosen.name.clone(),
            os: chosen.os.clone(),
            os_version: chosen.os_version.clone(),
            agent_ready: false,
        };
        if !chosen.is_booted() {
            let _ = self.tx.blocking_send(WorkDone::Booting {
                cs_target: cs_target.to_owned(),
                sdv_cmd: sdv_cmd.to_owned(),
                device: device.clone(),
            });
            self.simctl
                .boot(&device.udid)
                .map_err(|reason| fail(Some(&device), reason))?;
        }
        if let Err(reason) = self.simctl.show(&device.udid) {
            tracing::info!(target: "csp::device", slot = %device.slot, "the simulator window did not open (headless is fine): {reason}");
        }
        let paths = &self.config.paths;
        let agent = self.prepare_agent(session_id, &device.slot);
        let (config, port, node) = match agent {
            Ok((config, port, node)) => (Some(config), Some(port), Some(node)),
            Err(reason) => {
                tracing::warn!(target: "csp::device", slot = %device.slot, "agent-device unavailable: {reason}");
                (None, None, None)
            }
        };
        device.agent_ready = config.is_some();
        let file = SlotFile {
            version: SLOT_FILE_VERSION,
            slot: device.slot.clone(),
            udid: device.udid.clone(),
            platform: "ios".into(),
            model: device.model.clone(),
            os_version: device.os_label(),
            session_name: session_name(&device.slot),
            agent_device_config: config,
            daemon_port: port,
            node,
            shot_dir: paths.shot_dir(session_id, &device.slot),
        };
        write_slot_file(paths, session_id, &file).map_err(|error| {
            fail(
                Some(&device),
                format!("the slot file could not be written: {error}"),
            )
        })?;
        Ok(device)
    }

    fn prepare_agent(
        &self,
        session_id: &str,
        slot: &str,
    ) -> Result<(PathBuf, u16, PathBuf), String> {
        if self.config.disabled.is_some() {
            return Err("devices are turned off on this machine".into());
        }
        let paths = &self.config.paths;
        let node = resolve_node(self.runner.as_ref(), &self.config.node_candidates)?;
        let tool = ensure_agent_device(&self.runner, &node, &paths.tools_dir())?;
        let endpoint = ensure_daemon(&self.runner, paths, &node, &tool.entry)?;
        let config = write_agent_device_config(paths, session_id, slot, &endpoint)
            .map_err(|error| format!("the agent-device config could not be written: {error}"))?;
        write_shim(paths, session_id, &node, &tool.entry)
            .map_err(|error| format!("the agent-device shim could not be written: {error}"))?;
        Ok((config, endpoint.port, node.path))
    }

    /// Shut a closed device down.
    pub fn close(&self, key: String, cs_target: String, sdv_cmd: String, device: OpenedDevice) {
        let this = self.clone();
        tokio::spawn(async move {
            let simctl = this.simctl.clone();
            let udid = device.udid.clone();
            let result = tokio::task::spawn_blocking(move || simctl.shutdown(&udid))
                .await
                .unwrap_or_else(|error| Err(error.to_string()));
            let _ = this
                .tx
                .send(WorkDone::Closed {
                    key,
                    cs_target,
                    sdv_cmd,
                    device,
                    result,
                })
                .await;
        });
    }

    /// Take, store and upload one durable snapshot.
    pub fn shot(&self, request: ShotRequest) {
        let this = self.clone();
        tokio::spawn(async move {
            let result = this.shot_inner(&request).await;
            let _ = this.tx.send(WorkDone::Shot { request, result }).await;
        });
    }

    async fn shot_inner(&self, request: &ShotRequest) -> Result<UploadedImage, String> {
        let uploader = self
            .uploader
            .clone()
            .ok_or("this provider has no media endpoint to upload to")?;
        let simctl = self.simctl.clone();
        let udid = request.device.udid.clone();
        let dir = self
            .config
            .paths
            .shot_dir(&request.session_id, &request.device.slot);
        let prepared = tokio::task::spawn_blocking(move || {
            let png = simctl.screenshot(&udid, ShotFormat::Png)?;
            let prepared = prepare_image(&png, "image/png")?;
            // Beside the slot file, where `bee device screenshot` reads it.
            std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
            std::fs::write(
                dir.join(format!("{}.png", prepared.sha256)),
                &prepared.bytes,
            )
            .map_err(|error| error.to_string())?;
            Ok::<_, String>(prepared)
        })
        .await
        .map_err(|error| error.to_string())??;
        uploader.upload_prepared("device snapshot", &prepared).await
    }
}
