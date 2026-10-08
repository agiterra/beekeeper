//! The iOS Simulator through `xcrun simctl`, behind a command runner.
//!
//! Everything this module learns about the machine comes from JSON
//! (`simctl list -j`) or exit codes, never from parsing prose, because
//! Xcode 27's Device Hub already changed what the human-facing output looks
//! like (`justfile` `mobile-dev`) and will again. Tests drive it with a fake
//! [`CommandRunner`] and a fixture captured read-only from this Mac.
//!
//! Every call here blocks. Callers run it on `spawn_blocking`, never on the
//! provider's run loop or the device service's relay loop (SV-72/76).

use std::collections::BTreeMap;
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Deserialize;

use super::toolchain::{find_developer_dir, DeveloperDirs};

/// How long `simctl list` may take.
const LIST_TIMEOUT: Duration = Duration::from_secs(30);
/// How long a boot (including `bootstatus -b`) may take on a cold device.
const BOOT_TIMEOUT: Duration = Duration::from_secs(240);
/// How long one screenshot may take.
const SCREENSHOT_TIMEOUT: Duration = Duration::from_secs(20);
/// How long `open -a …` and `shutdown` may take.
const SHORT_TIMEOUT: Duration = Duration::from_secs(60);
/// The model opened when the command names none and nothing is booted: the
/// device the S1 acceptance names.
pub const DEFAULT_MODEL: &str = "iPhone 17";

/// What one finished command produced.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunOutput {
    /// Exit code, or `None` when a signal ended the process.
    pub status: Option<i32>,
    /// Captured standard output.
    pub stdout: Vec<u8>,
    /// Captured standard error.
    pub stderr: Vec<u8>,
}

impl RunOutput {
    /// Whether the process exited 0.
    pub fn success(&self) -> bool {
        self.status == Some(0)
    }

    /// Standard error as lossy text, trimmed, for a reason string.
    pub fn stderr_text(&self) -> String {
        String::from_utf8_lossy(&self.stderr).trim().to_owned()
    }
}

/// Why a command could not produce a [`RunOutput`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RunError {
    /// The program is not installed (or not on the provider's PATH).
    #[error("{0} not found")]
    NotFound(String),
    /// The program ran past its deadline and was killed.
    #[error("{0} timed out")]
    TimedOut(String),
    /// Any other spawn or pipe failure.
    #[error("{0}")]
    Io(String),
}

/// Runs one program to completion. The production runner spawns a process;
/// tests substitute a scripted fake.
pub trait CommandRunner: Send + Sync + 'static {
    /// Run `program` with `args` and extra `env`, killing it after `timeout`.
    fn run(
        &self,
        program: &str,
        args: &[String],
        env: &[(String, String)],
        timeout: Duration,
    ) -> Result<RunOutput, RunError>;
}

/// The production [`CommandRunner`]: a child process with piped output.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemRunner;

impl CommandRunner for SystemRunner {
    fn run(
        &self,
        program: &str,
        args: &[String],
        env: &[(String, String)],
        timeout: Duration,
    ) -> Result<RunOutput, RunError> {
        let mut command = Command::new(program);
        command
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, value) in env {
            command.env(key, value);
        }
        let mut child = command.spawn().map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                RunError::NotFound(program.to_owned())
            } else {
                RunError::Io(format!("{program}: {error}"))
            }
        })?;
        // Drain both pipes on their own threads: a screenshot is larger than
        // a pipe buffer, and a child blocked on a full pipe never exits.
        let stdout = child.stdout.take().map(drain);
        let stderr = child.stderr.take().map(drain);
        let deadline = Instant::now() + timeout;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() >= deadline => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(RunError::TimedOut(program.to_owned()));
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(20)),
                Err(error) => return Err(RunError::Io(format!("{program}: {error}"))),
            }
        };
        let join = |handle: Option<std::thread::JoinHandle<Vec<u8>>>| {
            handle
                .and_then(|handle| handle.join().ok())
                .unwrap_or_default()
        };
        Ok(RunOutput {
            status: status.code(),
            stdout: join(stdout),
            stderr: join(stderr),
        })
    }
}

fn drain<R: Read + Send + 'static>(mut pipe: R) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut out = Vec::new();
        let _ = pipe.read_to_end(&mut out);
        out
    })
}

/// One simulator as `simctl list -j devices available` describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimDevice {
    /// The device UDID. Host-local: it never leaves this machine.
    pub udid: String,
    /// The model name, e.g. `iPhone 17`.
    pub name: String,
    /// `Booted`, `Shutdown`, `Booting`, … as simctl reports it.
    pub state: String,
    /// The OS family the runtime is for, e.g. `iOS`.
    pub os: String,
    /// The runtime version, e.g. `27.0`.
    pub os_version: String,
}

impl SimDevice {
    /// Whether simctl reports the device booted.
    pub fn is_booted(&self) -> bool {
        self.state == "Booted"
    }

    /// `iOS 27.0`, the label the surface header shows.
    pub fn os_label(&self) -> String {
        format!("{} {}", self.os, self.os_version)
    }
}

#[derive(Deserialize)]
struct ListJson {
    devices: BTreeMap<String, Vec<DeviceJson>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeviceJson {
    udid: String,
    name: String,
    state: String,
    #[serde(default = "default_true")]
    is_available: bool,
}

fn default_true() -> bool {
    true
}

/// `com.apple.CoreSimulator.SimRuntime.iOS-27-0` → `("iOS", "27.0")`.
pub fn parse_runtime_id(runtime: &str) -> Option<(String, String)> {
    let tail = runtime.rsplit('.').next()?;
    let (os, version) = tail.split_once('-')?;
    if os.is_empty() || version.is_empty() {
        return None;
    }
    Some((os.to_owned(), version.replace('-', ".")))
}

/// Parse `simctl list -j devices available`, newest runtime first, keeping
/// only available devices on runtimes whose id can be read.
pub fn parse_device_list(json: &[u8]) -> Result<Vec<SimDevice>, String> {
    let list: ListJson = serde_json::from_slice(json)
        .map_err(|error| format!("simctl device list is not the expected JSON: {error}"))?;
    let mut runtimes: Vec<(String, String, Vec<DeviceJson>)> = list
        .devices
        .into_iter()
        .filter_map(|(runtime, devices)| {
            parse_runtime_id(&runtime).map(|(os, version)| (os, version, devices))
        })
        .collect();
    runtimes.sort_by_key(|runtime| std::cmp::Reverse(version_key(&runtime.1)));
    Ok(runtimes
        .into_iter()
        .flat_map(|(os, version, devices)| {
            devices
                .into_iter()
                .filter(|device| device.is_available)
                .map(move |device| SimDevice {
                    udid: device.udid,
                    name: device.name,
                    state: device.state,
                    os: os.clone(),
                    os_version: version.clone(),
                })
        })
        .collect())
}

fn version_key(version: &str) -> Vec<u64> {
    version
        .split('.')
        .map(|part| part.parse().unwrap_or(0))
        .collect()
}

/// Pick the device an `open` should use.
///
/// A named model must match by name (case-insensitively), newest runtime
/// first. With no name: an iOS device that is already booted, else
/// [`DEFAULT_MODEL`], else the first iPhone on the newest runtime.
pub fn choose_device<'a>(devices: &'a [SimDevice], model: Option<&str>) -> Option<&'a SimDevice> {
    let ios = || devices.iter().filter(|device| device.os == "iOS");
    if let Some(model) = model {
        return ios().find(|device| device.name.eq_ignore_ascii_case(model.trim()));
    }
    ios()
        .find(|device| device.is_booted())
        .or_else(|| ios().find(|device| device.name == DEFAULT_MODEL))
        .or_else(|| ios().find(|device| device.name.starts_with("iPhone")))
}

/// Whether this machine can run simctl at all, and why not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum XcodeProbe {
    /// `xcrun --find simctl` answered.
    Found,
    /// It did not; the reason is the sentence the surface shows.
    Missing(String),
}

/// `xcrun simctl …` through a runner.
#[derive(Clone)]
pub struct Simctl {
    runner: Arc<dyn CommandRunner>,
    developer_dirs: DeveloperDirs,
}

impl std::fmt::Debug for Simctl {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Simctl")
    }
}

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

impl Simctl {
    /// Wrap a runner; [`Self::probe`] looks for this host's developer
    /// directory ([`DeveloperDirs::host`]).
    pub fn new(runner: Arc<dyn CommandRunner>) -> Self {
        Self::with_developer_dirs(runner, DeveloperDirs::host())
    }

    /// Wrap a runner, looking for a developer directory in `developer_dirs`.
    pub fn with_developer_dirs(
        runner: Arc<dyn CommandRunner>,
        developer_dirs: DeveloperDirs,
    ) -> Self {
        Self {
            runner,
            developer_dirs,
        }
    }

    fn simctl(&self, rest: &[&str], timeout: Duration) -> Result<RunOutput, String> {
        let mut all = vec!["simctl"];
        all.extend_from_slice(rest);
        self.runner
            .run("xcrun", &args(&all), &[], timeout)
            .map_err(|error| error.to_string())
    }

    /// Whether Xcode's simctl is reachable. Never boots anything, and never
    /// runs `xcrun` on a Mac with no developer directory: there it is
    /// Apple's shim, which pops an install dialog at the person.
    pub fn probe(&self) -> XcodeProbe {
        if !self.developer_dirs.macos {
            return XcodeProbe::Missing("the iOS Simulator needs macOS".into());
        }
        if find_developer_dir(self.runner.as_ref(), &self.developer_dirs).is_none() {
            return XcodeProbe::Missing("Xcode not found".into());
        }
        match self
            .runner
            .run("xcrun", &args(&["--find", "simctl"]), &[], LIST_TIMEOUT)
        {
            Ok(output) if output.success() => XcodeProbe::Found,
            Ok(output) => XcodeProbe::Missing(format!(
                "Xcode not found ({})",
                first_line(&output.stderr_text())
            )),
            Err(RunError::NotFound(_)) => XcodeProbe::Missing("Xcode not found".into()),
            Err(error) => XcodeProbe::Missing(format!("Xcode not found ({error})")),
        }
    }

    /// Every available device, newest runtime first.
    pub fn list(&self) -> Result<Vec<SimDevice>, String> {
        let output = self.simctl(&["list", "-j", "devices", "available"], LIST_TIMEOUT)?;
        if !output.success() {
            return Err(format!(
                "simctl list failed: {}",
                first_line(&output.stderr_text())
            ));
        }
        parse_device_list(&output.stdout)
    }

    /// Boot `udid` and wait until it has finished booting. A device that is
    /// already booted is not an error.
    pub fn boot(&self, udid: &str) -> Result<(), String> {
        let output = self.simctl(&["bootstatus", udid, "-b"], BOOT_TIMEOUT)?;
        if output.success() {
            return Ok(());
        }
        Err(format!(
            "the simulator did not boot: {}",
            first_line(&output.stderr_text())
        ))
    }

    /// Show the booted device on this Mac's screen: Simulator.app where it
    /// exists, Xcode 27's Device Hub where it replaced it. Best effort — the
    /// device is usable headless, so a failure here is logged, not fatal.
    pub fn show(&self, udid: &str) -> Result<(), String> {
        let simulator = self.runner.run(
            "open",
            &args(&["-a", "Simulator", "--args", "-CurrentDeviceUDID", udid]),
            &[],
            SHORT_TIMEOUT,
        );
        if matches!(&simulator, Ok(output) if output.success()) {
            return Ok(());
        }
        match self
            .runner
            .run("open", &args(&["-a", "DeviceHub"]), &[], SHORT_TIMEOUT)
        {
            Ok(output) if output.success() => Ok(()),
            Ok(output) => Err(first_line(&output.stderr_text())),
            Err(error) => Err(error.to_string()),
        }
    }

    /// Shut `udid` down. A device that is already shut down is not an error.
    pub fn shutdown(&self, udid: &str) -> Result<(), String> {
        let output = self.simctl(&["shutdown", udid], SHORT_TIMEOUT)?;
        let stderr = output.stderr_text();
        if output.success() || stderr.contains("current state: Shutdown") {
            return Ok(());
        }
        Err(format!(
            "the simulator did not shut down: {}",
            first_line(&stderr)
        ))
    }

    /// One screenshot of `udid` as encoded bytes on stdout (`--type=jpeg` for
    /// frames, `png` for durable snapshots).
    pub fn screenshot(&self, udid: &str, format: ShotFormat) -> Result<Vec<u8>, String> {
        let kind = format!("--type={}", format.simctl_type());
        let output = self.simctl(&["io", udid, "screenshot", &kind, "-"], SCREENSHOT_TIMEOUT)?;
        if !output.success() || output.stdout.is_empty() {
            return Err(format!(
                "the simulator screenshot failed: {}",
                first_line(&output.stderr_text())
            ));
        }
        Ok(output.stdout)
    }
}

/// The encodings this module asks simctl for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShotFormat {
    /// Lossless, for a durable snapshot.
    Png,
    /// Lossy, for a watch frame.
    Jpeg,
}

impl ShotFormat {
    fn simctl_type(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpeg",
        }
    }
}

/// The first line of a reason, bounded, with any absolute path or UUID
/// removed so a simctl message can be published as a reason.
pub fn first_line(text: &str) -> String {
    let line = text.lines().next().unwrap_or("").trim();
    let line = if line.is_empty() { "no detail" } else { line };
    super::slot::scrub_host_details(&line.chars().take(200).collect::<String>())
}

#[cfg(test)]
#[path = "simctl_tests.rs"]
mod tests;
