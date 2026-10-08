//! The agent-device daemon, its 0600 config, and the seat's PATH shim.
//!
//! The provider (outside the seat boundary) brings the daemon up in HTTP mode
//! on loopback — there is no `daemon start`; the first command in a state
//! dir spawns it and writes `daemon.json` (T3 `LocalDeviceHost.ts:505-540`)
//! — and writes the `--config` file holding its URL and token, mode 0600.
//!
//! The seat gets a per-session shim directory whose `agent-device` is the
//! only way in. It differs from T3's (`AgentDeviceShim.ts:30-37`) in one way
//! that matters here: the agent passes only `--session <slot>`, the opaque
//! slot id, and the shim injects `--config`, `--platform` and `--udid` from
//! the slot file. The agent's command lines are transcript rows on the relay,
//! so they must never carry a UDID or a host path (brief § 3). The shim reads
//! slot files from its *own* session directory only, so a seat can never
//! address another session's device (cross-session refusal by construction).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;

use super::simctl::CommandRunner;
use super::slot::{write_private, DevicePaths};
use super::toolchain::NodeRuntime;

/// How long bringing the daemon up may take (first use builds nothing; the
/// XCTest runner is built lazily on the first driving command).
const DAEMON_READY_TIMEOUT: Duration = Duration::from_secs(60);

/// The daemon's own record of where it listens.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DaemonFile {
    /// Loopback TCP port.
    pub http_port: u16,
    /// Bearer token every request must carry.
    pub token: String,
}

/// A running daemon this provider can hand a seat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonEndpoint {
    /// Loopback TCP port the seat's boundary must allow.
    pub port: u16,
    /// Bearer token.
    pub token: String,
}

/// The environment the daemon runs under: HTTP mode, owned lifetime, quiet.
pub fn daemon_env(paths: &DevicePaths, node: &NodeRuntime) -> Vec<(String, String)> {
    let node_dir = node
        .path
        .parent()
        .map(|dir| dir.to_string_lossy().into_owned())
        .unwrap_or_default();
    vec![
        (
            "AGENT_DEVICE_STATE_DIR".into(),
            paths
                .agent_device_state_dir()
                .to_string_lossy()
                .into_owned(),
        ),
        ("AGENT_DEVICE_DAEMON_SERVER_MODE".into(), "http".into()),
        // The daemon idles out after five minutes by default; the provider
        // owns its lifetime.
        ("AGENT_DEVICE_DAEMON_IDLE_TIMEOUT_MS".into(), "0".into()),
        ("AGENT_DEVICE_NO_UPDATE_NOTIFIER".into(), "1".into()),
        ("FORCE_COLOR".into(), "0".into()),
        ("NO_COLOR".into(), "1".into()),
        (
            "PATH".into(),
            format!("{node_dir}:/usr/bin:/bin:/usr/sbin:/sbin"),
        ),
    ]
}

/// Read `daemon.json` from agent-device's state dir.
pub fn read_daemon_file(paths: &DevicePaths) -> Option<DaemonFile> {
    let bytes = std::fs::read(paths.agent_device_state_dir().join("daemon.json")).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn port_answers(port: u16) -> bool {
    std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
        Duration::from_secs(2),
    )
    .is_ok()
}

/// Bring the daemon up (or reuse a live one). Blocking.
pub fn ensure_daemon(
    runner: &Arc<dyn CommandRunner>,
    paths: &DevicePaths,
    node: &NodeRuntime,
    entry: &Path,
) -> Result<DaemonEndpoint, String> {
    if let Some(file) = read_daemon_file(paths).filter(|file| port_answers(file.http_port)) {
        return Ok(DaemonEndpoint {
            port: file.http_port,
            token: file.token,
        });
    }
    let state = paths.agent_device_state_dir();
    std::fs::create_dir_all(&state)
        .map_err(|error| format!("preparing agent-device state failed: {error}"))?;
    let _ = std::fs::remove_file(state.join("daemon.json"));
    let args = vec![
        entry.to_string_lossy().into_owned(),
        "devices".to_owned(),
        "--json".to_owned(),
    ];
    // Its exit status is not the signal; daemon.json is.
    let _ = runner.run(
        &node.path.to_string_lossy(),
        &args,
        &daemon_env(paths, node),
        DAEMON_READY_TIMEOUT,
    );
    read_daemon_file(paths)
        .map(|file| DaemonEndpoint {
            port: file.http_port,
            token: file.token,
        })
        .ok_or_else(|| "the agent-device daemon did not start".to_owned())
}

/// Write the agent-device `--config` file for one slot (0600).
pub fn write_agent_device_config(
    paths: &DevicePaths,
    session_id: &str,
    slot: &str,
    endpoint: &DaemonEndpoint,
) -> std::io::Result<PathBuf> {
    let path = paths.agent_device_config(session_id, slot);
    let json = serde_json::json!({
        "daemonBaseUrl": format!("http://127.0.0.1:{}", endpoint.port),
        "daemonAuthToken": endpoint.token,
    });
    write_private(&path, json.to_string().as_bytes())?;
    Ok(path)
}

/// The agent-device session name for a slot.
pub fn session_name(slot: &str) -> String {
    format!("bk-{slot}")
}

fn js(value: &str) -> String {
    serde_json::Value::String(value.to_owned()).to_string()
}

fn sh_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

/// The Node launcher the shim runs. Kept a pure function of its inputs so
/// the refusal rules are testable as text and, where Node exists, by running.
pub fn launcher_source(slot_dir: &Path, node: &Path, entry: &Path) -> String {
    format!(
        r#"import {{ spawn }} from "node:child_process";
import {{ readFileSync }} from "node:fs";
const SLOT_DIR = {slot_dir};
const NODE = {node};
const ENTRY = {entry};
const args = process.argv.slice(2);
const informational = args.length === 1 && ["help", "--help", "-h", "--version", "version"].includes(args[0]);
const fail = (message) => {{ console.error(message); process.exit(2); }};
if (!informational) {{
  const at = args.indexOf("--session");
  const slotId = at >= 0 ? args[at + 1] : undefined;
  if (!slotId || !/^[0-9a-f]{{16}}$/.test(slotId)) fail("Run `bee device open` first and pass its --session <slot> on every agent-device command.");
  for (const pinned of ["--config", "--udid", "--serial", "--platform"]) {{
    if (args.includes(pinned)) fail(`This agent-device is pinned to the session's device; drop ${{pinned}}.`);
  }}
  let slot;
  try {{ slot = JSON.parse(readFileSync(`${{SLOT_DIR}}/${{slotId}}.json`, "utf8")); }}
  catch {{ fail(`No device is open as ${{slotId}} in this session.`); }}
  if (!slot.agentDeviceConfig) fail("agent-device is unavailable on this machine; `bee device screenshot` still works.");
  args.splice(at, 2, "--session", slot.sessionName, "--config", slot.agentDeviceConfig, "--platform", slot.platform, "--udid", slot.udid);
}}
const env = {{ ...process.env }};
delete env.AGENT_DEVICE_DAEMON_BASE_URL;
delete env.AGENT_DEVICE_DAEMON_AUTH_TOKEN;
delete env.AGENT_DEVICE_CONFIG;
const child = spawn(NODE, [ENTRY, ...args], {{ stdio: "inherit", env }});
child.on("error", (error) => {{ console.error(error.message); process.exitCode = 1; }});
child.on("exit", (code) => {{ process.exitCode = code ?? 1; }});
"#,
        slot_dir = js(&slot_dir.to_string_lossy()),
        node = js(&node.to_string_lossy()),
        entry = js(&entry.to_string_lossy()),
    )
}

/// Write the session's shim directory and return it, for the seat's PATH.
pub fn write_shim(
    paths: &DevicePaths,
    session_id: &str,
    node: &NodeRuntime,
    entry: &Path,
) -> std::io::Result<PathBuf> {
    let dir = paths.shim_dir(session_id);
    std::fs::create_dir_all(&dir)?;
    let launcher = dir.join("agent-device-launcher.mjs");
    std::fs::write(
        &launcher,
        launcher_source(&paths.session_dir(session_id), &node.path, entry),
    )?;
    let script = format!(
        "#!/bin/sh\nexec {} {} \"$@\"\n",
        sh_quote(&node.path.to_string_lossy()),
        sh_quote(&launcher.to_string_lossy())
    );
    let shim = dir.join("agent-device");
    std::fs::write(&shim, script)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(dir)
}

/// The just-in-time guidance for a seat that opened a device (adapted from
/// T3 `handlers.ts:34-67`). Carries only the opaque slot id; Lane V's
/// `bee device open` prints the same text.
pub fn quick_start(slot: &str, model: &str, os_label: &str) -> String {
    let target = format!("--session {slot}");
    [
        format!("The session is watching {model} ({os_label}) in the Device surface."),
        format!("Drive it with agent-device. Always pass {target}; the shim adds the device and daemon flags."),
        "Typical loop:".to_owned(),
        format!("  agent-device open <bundle-id> {target}"),
        format!("  agent-device snapshot -i {target}        # accessibility tree with @eN refs"),
        format!("  agent-device click @e3 {target}"),
        format!("  agent-device fill @e5 \"text\" {target}"),
        "  bee device screenshot                      # a dated snapshot everyone in the session sees".to_owned(),
        "Prefer snapshot refs over coordinates. The Flutter debug bundle id is per worktree: read it from `flutter run` output.".to_owned(),
        "First use builds an XCTest runner and can take a couple of minutes; later commands are fast.".to_owned(),
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::super::test_support::FakeRunner;
    use super::*;

    fn node() -> NodeRuntime {
        NodeRuntime {
            path: PathBuf::from("/opt/node/bin/node"),
            version: "24.19.0".into(),
        }
    }

    #[test]
    fn the_daemon_is_started_in_http_mode_and_read_back_from_daemon_json() {
        let dir = tempfile::tempdir().expect("state");
        let paths = DevicePaths::new(dir.path());
        let state = paths.agent_device_state_dir();
        // The fake "daemon" answers by writing daemon.json, as the real one
        // does; port 1 has no listener, so a stale file is never reused.
        std::fs::create_dir_all(&state).expect("mkdir");
        std::fs::write(
            state.join("daemon.json"),
            r#"{"httpPort":1,"token":"stale"}"#,
        )
        .expect("stale");
        let written = state.clone();
        let runner = Arc::new(FakeRunner::new().on_fn("node", "devices --json", move || {
            std::fs::write(
                written.join("daemon.json"),
                r#"{"httpPort":1,"token":"t0k"}"#,
            )
            .expect("write");
            super::super::test_support::ok(b"[]")
        }));
        let runner_dyn: Arc<dyn CommandRunner> = runner.clone();
        let endpoint = ensure_daemon(
            &runner_dyn,
            &paths,
            &node(),
            Path::new("/tools/agent-device.mjs"),
        )
        .expect("started");
        assert_eq!(
            endpoint,
            DaemonEndpoint {
                port: 1,
                token: "t0k".into()
            }
        );
        let env = runner.env_of("devices --json").expect("ran");
        assert!(env.contains(&("AGENT_DEVICE_DAEMON_SERVER_MODE".into(), "http".into())));
        assert!(env.contains(&("AGENT_DEVICE_DAEMON_IDLE_TIMEOUT_MS".into(), "0".into())));
        assert!(env
            .iter()
            .any(|(k, v)| k == "PATH" && v.starts_with("/opt/node/bin:")));
    }

    #[test]
    fn the_config_file_is_private_and_names_loopback() {
        let dir = tempfile::tempdir().expect("state");
        let paths = DevicePaths::new(dir.path());
        let path = write_agent_device_config(
            &paths,
            "sess",
            "0123456789abcdef",
            &DaemonEndpoint {
                port: 4123,
                token: "secret".into(),
            },
        )
        .expect("write");
        let json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).expect("read")).expect("json");
        assert_eq!(json["daemonBaseUrl"], "http://127.0.0.1:4123");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).expect("meta").permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn the_shim_refuses_without_a_slot_and_pins_the_device_itself() {
        let source = launcher_source(
            Path::new("/s/dev/sess"),
            Path::new("/n/node"),
            Path::new("/t/a.mjs"),
        );
        assert!(source.contains("const SLOT_DIR = \"/s/dev/sess\";"));
        assert!(source.contains("/^[0-9a-f]{16}$/"));
        assert!(source.contains("\"--config\", \"--udid\", \"--serial\", \"--platform\""));
        assert!(source.contains("\"--udid\", slot.udid"));
        let dir = tempfile::tempdir().expect("state");
        let paths = DevicePaths::new(dir.path());
        let shim = write_shim(&paths, "sess", &node(), Path::new("/t/a.mjs")).expect("shim");
        let script = std::fs::read_to_string(shim.join("agent-device")).expect("script");
        assert!(script.starts_with("#!/bin/sh\nexec '/opt/node/bin/node' '"));
        assert_eq!(shim, paths.shim_dir("sess"));
    }

    #[test]
    fn the_quick_start_names_only_the_opaque_slot() {
        let text = quick_start("0123456789abcdef", "iPhone 17", "iOS 27.0");
        assert!(text.contains("--session 0123456789abcdef"));
        assert!(!super::super::slot::contains_uuid_shape(&text));
        assert!(!super::super::slot::contains_host_path(&text));
    }
}
