//! A Host inside a `.app` runs that bundle's helpers or none, through the real
//! binary, past a `PATH` that offers every one of them.
//!
//! Everything is in a throwaway home with a nonsense key and a relay that does
//! not resolve. `HOME` and `BEEKEEPER_HOST_HOME` differ, and nothing here reaches
//! `install`, so no service manager is involved.
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

const PUBKEY: &str = "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";
const FAKE_KEY: &str = "0000000000000000000000000000000000000000000000000000000000000001";

/// Retries only `ExecutableFileBusy`, for up to five seconds. This binary is
/// copied in fresh by each test, and a sibling test's `fork` inherits the write
/// descriptor until that child's `execve`, so on Linux a run straight after the
/// copy can be refused though the copy is closed. Any other error stands.
fn past_text_busy<T>(mut attempt: impl FnMut() -> std::io::Result<T>) -> std::io::Result<T> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match attempt() {
            Err(error)
                if error.kind() == std::io::ErrorKind::ExecutableFileBusy
                    && Instant::now() < deadline =>
            {
                std::thread::sleep(Duration::from_millis(10));
            }
            other => return other,
        }
    }
}
const RELAY: &str = "wss://unreachable.invalid";
const HELPERS: [&str; 4] = [
    "beekeeper-session-provider",
    "beekeeper-dev-mcp",
    "bee",
    "git-credential-nostr",
];

struct Rig {
    _temp: tempfile::TempDir,
    root: PathBuf,
    bundle_dir: PathBuf,
    trap_dir: PathBuf,
    /// Written by the trap's provider if anything ever runs it.
    sprung: PathBuf,
}

fn script(path: &Path, body: &str) {
    fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    fs::write(path, format!("#!/bin/sh\n{body}\n")).expect("write");
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).expect("chmod");
}

impl Rig {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().canonicalize().expect("canonical");
        let bundle_dir = root.join("Beekeeper Host.app/Contents/MacOS");
        fs::create_dir_all(&bundle_dir).expect("mkdir");
        fs::copy(
            env!("CARGO_BIN_EXE_beekeeper-host"),
            bundle_dir.join("beekeeper-host"),
        )
        .expect("copy the real host");

        // `sleep`, so a provider the supervisor did start is still there to see.
        for helper in HELPERS {
            script(&bundle_dir.join(helper), "exec /bin/sleep 300");
        }

        let trap_dir = root.join("trap");
        let sprung = root.join("trap-sprung");
        for helper in HELPERS {
            script(
                &trap_dir.join(helper),
                &format!("echo {helper} >>'{}'", sprung.display()),
            );
        }

        let host_home = root.join("host-home");
        let host_dir = host_home.join(".local/state/buzz/host");
        let base_dir = host_home.join("app-data/session-provider");
        fs::create_dir_all(&host_dir).expect("mkdir");
        fs::create_dir_all(base_dir.join(PUBKEY)).expect("mkdir");
        fs::create_dir_all(base_dir.join("logs")).expect("mkdir");
        fs::create_dir_all(root.join("os-home")).expect("mkdir");
        fs::write(
            base_dir.join("coding-session-provider.json"),
            serde_json::json!({
                "version": 1,
                "providers": {RELAY: {
                    "providerPubkey": PUBKEY,
                    "instanceId": &PUBKEY[..16],
                    "createdAt": "2026-11-04T00:00:00Z",
                    "relayUrl": RELAY,
                }},
            })
            .to_string(),
        )
        .expect("write the record");
        fs::write(
            host_dir.join("host.json"),
            serde_json::json!({
                "version": 1,
                "instance": "production",
                "relayUrl": RELAY,
                "providerPubkey": PUBKEY,
                "sessionProviderBaseDir": base_dir,
                "providerStateDir": base_dir.join(PUBKEY),
                "runtimes": [],
                "writtenAt": "2026-11-04T00:00:00Z",
            })
            .to_string(),
        )
        .expect("write host.json");
        let key = host_dir.join("provider-key");
        fs::write(&key, format!("{FAKE_KEY}\n")).expect("write key");
        fs::set_permissions(&key, fs::Permissions::from_mode(0o600)).expect("chmod");

        Self {
            _temp: temp,
            root,
            bundle_dir,
            trap_dir,
            sprung,
        }
    }

    fn host(&self, args: &[&str]) -> Command {
        let mut command = Command::new(self.bundle_dir.join("beekeeper-host"));
        command
            .args(args)
            .env_clear()
            .env("HOME", self.root.join("os-home"))
            .env("BEEKEEPER_HOST_HOME", self.root.join("host-home"))
            .env("BEEKEEPER_HOST_SOCK", self.root.join("h.sock"))
            .env("PATH", format!("{}:/usr/bin:/bin", self.trap_dir.display()))
            .current_dir(&self.root);
        command
    }

    fn check(&self) -> String {
        let mut host = self.host(&["check"]);
        let output: Output = past_text_busy(|| host.output()).expect("check runs");
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    fn provider_line(&self) -> String {
        self.check()
            .lines()
            .find_map(|line| line.strip_prefix("provider bin:"))
            .expect("check names the provider")
            .trim()
            .to_string()
    }

    fn log_path(&self) -> PathBuf {
        let check = self.check();
        let line = check
            .lines()
            .find_map(|line| line.strip_prefix("provider log:"))
            .expect("check names the log");
        PathBuf::from(line.trim())
    }

    fn run_until(&self, done: impl Fn() -> bool) -> bool {
        let mut command = self.host(&["run"]);
        command.stdout(Stdio::null()).stderr(Stdio::null());
        let mut host = past_text_busy(|| command.spawn()).expect("the host starts");
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut reached = false;
        while Instant::now() < deadline {
            if done() {
                reached = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = Command::new("/bin/kill")
            .arg(host.id().to_string())
            .status();
        let _ = host.wait();
        reached
    }
}

#[test]
fn a_whole_bundle_uses_its_own_provider_past_a_path_that_offers_another() {
    let rig = Rig::new();
    assert_eq!(
        rig.provider_line(),
        rig.bundle_dir
            .join("beekeeper-session-provider")
            .display()
            .to_string()
    );
}

#[test]
fn each_missing_helper_refuses_by_name_and_never_takes_the_one_on_path() {
    for helper in HELPERS {
        let rig = Rig::new();
        fs::remove_file(rig.bundle_dir.join(helper)).expect("remove");
        let line = rig.provider_line();
        assert!(line.starts_with("<unresolved:"), "{helper}: {line}");
        assert!(line.contains(helper), "{helper}: {line}");
        assert!(
            !line.contains(&rig.trap_dir.display().to_string()),
            "{line}"
        );
    }
}

#[test]
fn a_helper_that_is_not_executable_counts_as_missing() {
    let rig = Rig::new();
    let path = rig.bundle_dir.join("beekeeper-session-provider");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).expect("chmod");
    assert!(rig.provider_line().contains("beekeeper-session-provider"));
}

#[test]
fn a_host_reached_through_a_symlink_still_reads_as_bundled() {
    let rig = Rig::new();
    fs::remove_file(rig.bundle_dir.join("beekeeper-session-provider")).expect("remove");
    let link = rig.root.join("beekeeper-host-link");
    std::os::unix::fs::symlink(rig.bundle_dir.join("beekeeper-host"), &link).expect("symlink");
    let output = rig.host(&["check"]);
    let mut linked = Command::new(&link);
    linked
        .args(output.get_args())
        .current_dir(&rig.root)
        .env_clear();
    for (key, value) in output.get_envs() {
        if let Some(value) = value {
            linked.env(key, value);
        }
    }
    let stdout = String::from_utf8_lossy(&past_text_busy(|| linked.output()).expect("runs").stdout)
        .into_owned();
    assert!(stdout.contains("<unresolved:"), "{stdout}");
}

#[test]
fn a_provider_named_in_host_json_is_still_the_operators() {
    let rig = Rig::new();
    let named = rig.root.join("override/beekeeper-session-provider");
    script(&named, "exit 0");
    fs::remove_file(rig.bundle_dir.join("bee")).expect("remove");

    let path = rig.root.join("host-home/.local/state/buzz/host/host.json");
    let mut config: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).expect("read")).expect("parse");
    config["providerCommand"] = serde_json::json!(named);
    fs::write(&path, config.to_string()).expect("write");

    assert_eq!(rig.provider_line(), named.display().to_string());
}

/// The supervisor takes the same answer: it logs the refusal, gives up, and has
/// run nothing, while the host stays up to report it.
#[test]
fn a_running_host_with_a_missing_helper_starts_nothing() {
    let rig = Rig::new();
    fs::remove_file(rig.bundle_dir.join("beekeeper-session-provider")).expect("remove");
    let log = rig.log_path();
    let refused = rig.run_until(|| {
        fs::read_to_string(&log)
            .is_ok_and(|logged| logged.contains("bundle is missing beekeeper-session-provider"))
    });
    assert!(refused, "no refusal in {}", log.display());
    assert!(!rig.sprung.exists(), "the host ran the provider on PATH");
}

/// The other half, so the test above cannot pass by nothing ever starting: with
/// the provider present the same rig reaches it, and the trap stays untouched.
#[test]
fn a_running_host_with_a_whole_bundle_starts_its_own_provider() {
    let rig = Rig::new();
    let marker = rig.root.join("provider-ran");
    script(
        &rig.bundle_dir.join("beekeeper-session-provider"),
        &format!(
            "echo \"mcp=$BEEKEEPER_CSP_CONTEXT_MCP_COMMAND\" >'{0}.tmp'\nmv '{0}.tmp' '{0}'\nexec /bin/sleep 300",
            marker.display()
        ),
    );
    let started = rig.run_until(|| marker.exists());
    assert!(started, "the provider never ran");
    let seen = fs::read_to_string(&marker).expect("marker");
    assert!(
        seen.contains(
            &rig.bundle_dir
                .join("beekeeper-dev-mcp")
                .display()
                .to_string()
        ),
        "{seen}"
    );
    assert!(!rig.sprung.exists());
}
