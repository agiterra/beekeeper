//! The pinned `agent-device` install and the Node runtime it needs.
//!
//! Driving a simulator is Callstack's `agent-device` CLI, pinned at
//! [`AGENT_DEVICE_VERSION`] — the version T3 pins
//! (`apps/server/src/device/DeviceToolchain.ts:30`) and the one the quick
//! start the seat reads was written for. It is npm-installed into
//! `<CSP state>/device/tools/agent-device/<version>` with T3's recipe: stage
//! into a temp sibling, write a sentinel only after npm exits 0 and the entry
//! exists, then rename into place. Never `npx`: an ephemeral cache would make
//! the first open after a reboot depend on the registry.
//!
//! Node is the one host requirement, and only for *driving*: snapshots and
//! frames are simctl alone. This Mac has Node only behind a mise shim, which
//! a launchd-started host may not see on its PATH, so [`resolve_node`] looks
//! in the well-known install roots too and reports a reason when nothing
//! qualifies, rather than failing an open.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use super::simctl::{CommandRunner, RunError, RunOutput};

/// The npm package that drives devices.
pub const AGENT_DEVICE_PACKAGE: &str = "agent-device";
/// The pinned version (T3 `DeviceToolchain.ts:30`; npm `engines: node >=22.12`).
pub const AGENT_DEVICE_VERSION: &str = "0.21.12";
/// The oldest Node `agent-device` 0.21.12 declares support for.
pub const MIN_NODE: (u64, u64, u64) = (22, 12, 0);
/// The sentinel file a completed install carries.
const SENTINEL: &str = ".install-complete";
/// How long `npm install` may take.
const INSTALL_TIMEOUT: Duration = Duration::from_secs(600);
/// How long `node --version` may take.
const VERSION_TIMEOUT: Duration = Duration::from_secs(10);

/// A Node runtime that qualifies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeRuntime {
    /// Absolute path of the `node` executable.
    pub path: PathBuf,
    /// Its version, e.g. `24.19.0`.
    pub version: String,
}

impl NodeRuntime {
    /// The `npm` beside this `node`, falling back to `npm` on PATH.
    pub fn npm(&self) -> PathBuf {
        self.path
            .parent()
            .map(|dir| dir.join("npm"))
            .filter(|npm| npm.exists())
            .unwrap_or_else(|| PathBuf::from("npm"))
    }
}

/// Parse `v24.19.0` (or `24.19.0`) into its three numbers.
pub fn parse_node_version(text: &str) -> Option<(u64, u64, u64)> {
    let trimmed = text.trim().trim_start_matches('v');
    let mut parts = trimmed.split('.').map(|part| {
        part.chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .parse::<u64>()
            .ok()
    });
    Some((
        parts.next()??,
        parts.next()??,
        parts.next().flatten().unwrap_or(0),
    ))
}

/// The places a `node` may live, most specific first: an explicit override,
/// the provider's PATH, then version-manager install roots under `home`
/// (mise, nvm) and the Homebrew prefixes. Version-manager roots list the
/// highest version first.
pub fn node_candidates(
    override_path: Option<&Path>,
    path_var: Option<&str>,
    home: Option<&Path>,
) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(path) = override_path {
        out.push(path.to_path_buf());
    }
    if let Some(path_var) = path_var {
        out.extend(std::env::split_paths(path_var).map(|dir| dir.join("node")));
    }
    if let Some(home) = home {
        for root in [
            home.join(".local/share/mise/installs/node"),
            home.join(".nvm/versions/node"),
        ] {
            let mut versions: Vec<PathBuf> = std::fs::read_dir(&root)
                .into_iter()
                .flatten()
                .flatten()
                .map(|entry| entry.path())
                .collect();
            versions.sort_by_key(|dir| {
                dir.file_name()
                    .and_then(|name| parse_node_version(&name.to_string_lossy()))
            });
            out.extend(versions.into_iter().rev().map(|dir| dir.join("bin/node")));
        }
    }
    out.push(PathBuf::from("/opt/homebrew/bin/node"));
    out.push(PathBuf::from("/usr/local/bin/node"));
    let mut seen = std::collections::BTreeSet::new();
    out.retain(|path| seen.insert(path.clone()));
    out
}

/// The first candidate whose `--version` is at least [`MIN_NODE`], or the
/// sentence the availability record carries when none is.
pub fn resolve_node(
    runner: &dyn CommandRunner,
    candidates: &[PathBuf],
) -> Result<NodeRuntime, String> {
    let mut too_old: Option<String> = None;
    for candidate in candidates {
        let program = candidate.to_string_lossy();
        let output = match runner.run(&program, &["--version".to_owned()], &[], VERSION_TIMEOUT) {
            Ok(output) if output.success() => output,
            _ => continue,
        };
        let text = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        match parse_node_version(&text) {
            Some(version) if version >= MIN_NODE => {
                return Ok(NodeRuntime {
                    path: candidate.clone(),
                    version: format!("{}.{}.{}", version.0, version.1, version.2),
                })
            }
            Some(_) => {
                too_old.get_or_insert(text);
            }
            None => {}
        }
    }
    let (major, minor, _) = MIN_NODE;
    Err(match too_old {
        Some(found) => format!("Node {found} is older than the {major}.{minor} agent-device needs"),
        None => format!("Node {major}.{minor} or newer not found"),
    })
}

/// `xcode-select`, by absolute path: it is a real binary, not one of the
/// `/usr/bin` shims (`xcrun`, `xcodebuild`, `simctl`) that pop Apple's
/// "install the command line developer tools" dialog when no developer
/// directory exists.
pub const XCODE_SELECT: &str = "/usr/bin/xcode-select";
/// How long `xcode-select -p` may take.
const XCODE_SELECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Where a developer directory (Xcode or the Command Line Tools) may be.
/// [`find_developer_dir`] checks these before anything runs `xcrun`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeveloperDirs {
    /// `DEVELOPER_DIR`, which `xcrun` honours over `xcode-select`.
    pub env: Option<PathBuf>,
    /// Known install locations, checked when `xcode-select -p` names none
    /// that exists.
    pub fallbacks: Vec<PathBuf>,
}

impl DeveloperDirs {
    /// This process's `DEVELOPER_DIR`, the Command Line Tools, and every
    /// `/Applications/Xcode*.app` developer directory.
    pub fn host() -> Self {
        let mut fallbacks = vec![PathBuf::from("/Library/Developer/CommandLineTools")];
        let mut xcodes: Vec<PathBuf> = std::fs::read_dir("/Applications")
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .map(|name| name.to_string_lossy())
                    .is_some_and(|name| name.starts_with("Xcode") && name.ends_with(".app"))
            })
            .map(|app| app.join("Contents/Developer"))
            .collect();
        xcodes.sort();
        fallbacks.extend(xcodes);
        Self {
            env: std::env::var_os("DEVELOPER_DIR")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from),
            fallbacks,
        }
    }
}

/// The developer directory `xcrun` would use, found without running
/// `xcrun` (or anything else behind Apple's install shim): `DEVELOPER_DIR`,
/// then `xcode-select -p`, then the known install locations. `None` means
/// no Xcode and no Command Line Tools: `xcrun` must not be called at all.
pub fn find_developer_dir(runner: &dyn CommandRunner, dirs: &DeveloperDirs) -> Option<PathBuf> {
    if let Some(env) = dirs.env.as_ref().filter(|dir| dir.is_dir()) {
        return Some(env.clone());
    }
    let selected = runner
        .run(XCODE_SELECT, &["-p".to_owned()], &[], XCODE_SELECT_TIMEOUT)
        .ok()
        .filter(RunOutput::success)
        .map(|output| PathBuf::from(String::from_utf8_lossy(&output.stdout).trim()))
        .filter(|dir| dir.is_absolute() && dir.is_dir());
    selected.or_else(|| dirs.fallbacks.iter().find(|dir| dir.is_dir()).cloned())
}

/// Where one pinned install lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolPaths {
    /// `<tools>/agent-device/<version>`.
    pub install_dir: PathBuf,
    /// The entry script run with Node.
    pub entry: PathBuf,
    /// The completion sentinel.
    pub sentinel: PathBuf,
}

/// The pinned agent-device paths under `tools_dir`.
pub fn agent_device_paths(tools_dir: &Path) -> ToolPaths {
    let install_dir = tools_dir
        .join(AGENT_DEVICE_PACKAGE)
        .join(AGENT_DEVICE_VERSION);
    ToolPaths {
        entry: install_dir
            .join("node_modules")
            .join(AGENT_DEVICE_PACKAGE)
            .join("bin")
            .join("agent-device.mjs"),
        sentinel: install_dir.join(SENTINEL),
        install_dir,
    }
}

/// Whether the pinned version is installed and complete.
pub fn is_installed(paths: &ToolPaths) -> bool {
    paths.entry.exists()
        && std::fs::read_to_string(&paths.sentinel)
            .is_ok_and(|text| text.trim() == AGENT_DEVICE_VERSION)
}

/// Install the pinned agent-device with `node`'s npm, unless it already is.
/// Blocking; run it on `spawn_blocking`.
pub fn ensure_agent_device(
    runner: &Arc<dyn CommandRunner>,
    node: &NodeRuntime,
    tools_dir: &Path,
) -> Result<ToolPaths, String> {
    let paths = agent_device_paths(tools_dir);
    if is_installed(&paths) {
        return Ok(paths);
    }
    let parent = paths
        .install_dir
        .parent()
        .ok_or("the agent-device install directory has no parent")?
        .to_path_buf();
    let _ = std::fs::remove_dir_all(&paths.install_dir);
    std::fs::create_dir_all(&parent)
        .map_err(|error| format!("preparing the agent-device install failed: {error}"))?;
    let staging = parent.join(format!(".staging-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging)
        .map_err(|error| format!("preparing the agent-device install failed: {error}"))?;
    let result = install_into(runner, node, &staging, &paths);
    let _ = std::fs::remove_dir_all(&staging);
    result.map(|()| paths)
}

fn install_into(
    runner: &Arc<dyn CommandRunner>,
    node: &NodeRuntime,
    staging: &Path,
    paths: &ToolPaths,
) -> Result<(), String> {
    let args = vec![
        "install".to_owned(),
        "--prefix".to_owned(),
        staging.to_string_lossy().into_owned(),
        "--no-fund".to_owned(),
        "--no-audit".to_owned(),
        format!("{AGENT_DEVICE_PACKAGE}@{AGENT_DEVICE_VERSION}"),
    ];
    // npm is a Node script: its own `#!/usr/bin/env node` must find the node
    // that qualified, not whatever a launchd PATH holds.
    let node_dir = node
        .path
        .parent()
        .map(|dir| dir.to_string_lossy().into_owned())
        .unwrap_or_default();
    let path = format!("{node_dir}:/usr/bin:/bin:/usr/sbin:/sbin");
    let env = vec![("PATH".to_owned(), path)];
    let npm = node.npm();
    let output = runner
        .run(&npm.to_string_lossy(), &args, &env, INSTALL_TIMEOUT)
        .map_err(|error| match error {
            RunError::NotFound(_) => "npm not found beside Node".to_owned(),
            other => format!("npm install of agent-device failed: {other}"),
        })?;
    if !output.success() {
        return Err(format!(
            "npm install of agent-device failed: {}",
            super::simctl::first_line(&output.stderr_text())
        ));
    }
    let staged_entry = staging
        .join("node_modules")
        .join(AGENT_DEVICE_PACKAGE)
        .join("bin")
        .join("agent-device.mjs");
    if !staged_entry.exists() {
        return Err("npm finished but agent-device's entry script is missing".into());
    }
    std::fs::write(staging.join(SENTINEL), format!("{AGENT_DEVICE_VERSION}\n"))
        .map_err(|error| format!("recording the agent-device install failed: {error}"))?;
    if let Err(error) = std::fs::rename(staging, &paths.install_dir) {
        // A concurrent provider may have published the same version first.
        if !is_installed(paths) {
            return Err(format!(
                "publishing the agent-device install failed: {error}"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{ok, FakeRunner};
    use super::*;

    #[test]
    fn node_versions_parse_and_compare() {
        assert_eq!(parse_node_version("v24.19.0\n"), Some((24, 19, 0)));
        assert_eq!(parse_node_version("22.12"), Some((22, 12, 0)));
        assert_eq!(parse_node_version("nope"), None);
        assert!(parse_node_version("v22.11.9").is_some_and(|v| v < MIN_NODE));
        assert!(parse_node_version("v22.12.0").is_some_and(|v| v >= MIN_NODE));
    }

    #[test]
    fn the_first_qualifying_node_wins_and_an_old_one_is_named() {
        let runner = FakeRunner::new()
            .on("/old/node", "--version", ok(b"v20.11.1\n"))
            .on("/new/node", "--version", ok(b"v24.19.0\n"));
        let candidates = [
            PathBuf::from("/missing/node"),
            PathBuf::from("/old/node"),
            PathBuf::from("/new/node"),
        ];
        let node = resolve_node(&runner, &candidates).expect("qualifies");
        assert_eq!(node.path, PathBuf::from("/new/node"));
        assert_eq!(node.version, "24.19.0");
        let reason = resolve_node(&runner, &candidates[..2]).expect_err("too old");
        assert!(
            reason.contains("v20.11.1") && reason.contains("22.12"),
            "{reason}"
        );
        let reason = resolve_node(&FakeRunner::new(), &candidates).expect_err("none");
        assert_eq!(reason, "Node 22.12 or newer not found");
    }

    #[test]
    fn mise_installs_are_searched_highest_first() {
        let home = tempfile::tempdir().expect("home");
        for version in ["20.1.0", "24.19.0", "22.12.0"] {
            std::fs::create_dir_all(
                home.path()
                    .join(".local/share/mise/installs/node")
                    .join(version)
                    .join("bin"),
            )
            .expect("mkdir");
        }
        let candidates = node_candidates(None, None, Some(home.path()));
        let mise: Vec<String> = candidates
            .iter()
            .filter(|p| p.to_string_lossy().contains("mise"))
            .map(|p| {
                p.parent()
                    .and_then(Path::parent)
                    .map(|d| {
                        d.file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned()
                    })
                    .unwrap_or_default()
            })
            .collect();
        assert_eq!(mise, ["24.19.0", "22.12.0", "20.1.0"]);
    }

    /// A runner whose npm "installs" by creating the entry in the staging
    /// prefix, so the stage-sentinel-rename recipe is exercised without a
    /// network or a real npm.
    struct FakeNpm;
    impl CommandRunner for FakeNpm {
        fn run(
            &self,
            _program: &str,
            args: &[String],
            _env: &[(String, String)],
            _t: Duration,
        ) -> Result<RunOutput, RunError> {
            let prefix = args
                .iter()
                .position(|a| a == "--prefix")
                .and_then(|i| args.get(i + 1))
                .expect("prefix");
            let bin = Path::new(prefix).join("node_modules/agent-device/bin");
            std::fs::create_dir_all(&bin).expect("mkdir");
            std::fs::write(bin.join("agent-device.mjs"), "// fake").expect("write");
            assert!(args.contains(&format!("agent-device@{AGENT_DEVICE_VERSION}")));
            ok(b"")
        }
    }

    #[test]
    fn install_stages_writes_the_sentinel_and_is_idempotent() {
        let tools = tempfile::tempdir().expect("tools");
        let node = NodeRuntime {
            path: PathBuf::from("/nonexistent/node"),
            version: "24.19.0".into(),
        };
        let runner: Arc<dyn CommandRunner> = Arc::new(FakeNpm);
        let paths = ensure_agent_device(&runner, &node, tools.path()).expect("install");
        assert!(is_installed(&paths));
        assert!(paths.install_dir.ends_with("agent-device/0.21.12"));
        let leftovers: Vec<_> = std::fs::read_dir(paths.install_dir.parent().expect("parent"))
            .expect("read")
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with(".staging"))
            .collect();
        assert!(leftovers.is_empty());
        // A second call is a no-op: a runner that would fail is never called.
        let refusing: Arc<dyn CommandRunner> = Arc::new(FakeRunner::new());
        ensure_agent_device(&refusing, &node, tools.path()).expect("already installed");
    }

    #[test]
    fn a_failed_npm_leaves_no_install_and_a_scrubbed_reason() {
        let tools = tempfile::tempdir().expect("tools");
        let node = NodeRuntime {
            path: PathBuf::from("/nonexistent/node"),
            version: "24.19.0".into(),
        };
        let runner: Arc<dyn CommandRunner> = Arc::new(FakeRunner::new().on(
            "npm",
            "install",
            super::super::test_support::fail(1, "npm ERR! network /Users/brian/.npm/_logs/x.log"),
        ));
        let reason = ensure_agent_device(&runner, &node, tools.path()).expect_err("fails");
        assert!(
            reason.starts_with("npm install of agent-device failed"),
            "{reason}"
        );
        assert!(!reason.contains("/Users/"), "{reason}");
        assert!(!is_installed(&agent_device_paths(tools.path())));
    }
}
