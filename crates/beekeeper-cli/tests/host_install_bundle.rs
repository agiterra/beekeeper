//! `bee host install`, from a bundle, against a recording `launchctl`.
//!
//! `PATH` holds nothing but the stub's directory, so the real `launchctl` cannot
//! be reached: `install` runs it by bare name. `HOME` and `BEEKEEPER_HOST_HOME`
//! are the same throwaway directory on purpose, which is the one way to take the
//! activation path that a person's own install takes (`home_is_ours`).
#![cfg(target_os = "macos")]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const MENU_BAR_LABEL: &str = "io.agiterra.beekeeper.menubar";
const HOST_LABEL: &str = "io.agiterra.beekeeper.host";

struct Rig {
    _temp: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
    bundle_dir: PathBuf,
    stub_dir: PathBuf,
    calls: PathBuf,
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
        let home = root.join("home");
        fs::create_dir_all(&home).expect("mkdir");

        let bundle_dir = root.join("Beekeeper Host.app/Contents/MacOS");
        fs::create_dir_all(&bundle_dir).expect("mkdir");
        fs::copy(env!("CARGO_BIN_EXE_bee"), bundle_dir.join("bee")).expect("copy the real bee");
        script(&bundle_dir.join("beekeeper-menubar"), "exit 0");
        script(&bundle_dir.join("beekeeper-host"), "exit 0");

        let stub_dir = root.join("stub");
        let calls = root.join("launchctl-calls");
        // Records each call; a bootstrap fails when the marker file is there.
        script(
            &stub_dir.join("launchctl"),
            &format!(
                "echo \"$*\" >>'{calls}'\n\
                 if [ \"$1\" = bootstrap ] && [ -e '{fail}' ]; then echo 'stub refused' >&2; exit 5; fi\n\
                 if [ \"$1\" = list ]; then exit 113; fi\n\
                 exit 0",
                calls = calls.display(),
                fail = root.join("fail-bootstrap").display(),
            ),
        );
        // Offered on PATH, and never acceptable from a bundle.
        script(&stub_dir.join("beekeeper-menubar"), "exit 0");
        script(&stub_dir.join("beekeeper-host"), "exit 0");

        Self {
            _temp: temp,
            root,
            home,
            bundle_dir,
            stub_dir,
            calls,
        }
    }

    fn run(&self, bee: &Path, args: &[&str]) -> Output {
        Command::new(bee)
            .args(["host"])
            .args(args)
            .env_clear()
            .env("HOME", &self.home)
            .env("BEEKEEPER_HOST_HOME", &self.home)
            .env("PATH", &self.stub_dir)
            .current_dir(&self.root)
            .output()
            .expect("bee runs")
    }

    fn bee(&self, args: &[&str]) -> Output {
        self.run(&self.bundle_dir.join("bee"), args)
    }

    fn plist(&self, label: &str) -> PathBuf {
        self.home
            .join("Library/LaunchAgents")
            .join(format!("{label}.plist"))
    }

    fn calls(&self) -> Vec<String> {
        fs::read_to_string(&self.calls)
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn stdout_json(output: &Output) -> serde_json::Value {
    serde_json::from_slice(&output.stdout).expect("the answer is JSON")
}

#[test]
fn menu_bar_registers_this_bundles_menu_bar_and_leaves_the_host_alone() {
    let rig = Rig::new();
    let output = rig.bee(&["install", "--menu-bar"]);
    assert!(output.status.success(), "{}", stderr(&output));

    let program = rig.bundle_dir.join("beekeeper-menubar");
    let answer = stdout_json(&output);
    assert_eq!(answer["installed"], true);
    assert_eq!(answer["program"], program.display().to_string());

    let plist = fs::read_to_string(rig.plist(MENU_BAR_LABEL)).expect("the plist");
    assert!(plist.contains(&format!("<string>{}</string>", program.display())));
    assert!(!plist.contains("<string>run</string>"), "{plist}");
    assert!(plist.contains(&format!("<string>{}</string>", rig.home.display())));
    assert!(plist.contains("<string>production</string>"), "{plist}");

    // Replacing a loaded job, in that order, and only the menu bar's. A bootout
    // of the host here would end every session it runs.
    let calls = rig.calls();
    assert_eq!(calls.len(), 2, "{calls:?}");
    assert!(calls[0].starts_with("bootout gui/"), "{calls:?}");
    assert!(
        calls[0].ends_with(&format!("/{MENU_BAR_LABEL}")),
        "{calls:?}"
    );
    assert!(calls[1].starts_with("bootstrap gui/"), "{calls:?}");
    assert!(
        calls[1].contains(&format!("{MENU_BAR_LABEL}.plist")),
        "{calls:?}"
    );
    assert!(!rig.plist(HOST_LABEL).exists());

    // Saying the host is not registered is a fact a person needs, not a failure.
    assert!(
        stderr(&output).contains("bee host install"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn menu_bar_honours_the_dev_instance() {
    let rig = Rig::new();
    let output = rig.bee(&["install", "--menu-bar", "--dev"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let plist = fs::read_to_string(rig.plist(&format!("{MENU_BAR_LABEL}.dev"))).expect("plist");
    assert!(plist.contains("<string>dev</string>"), "{plist}");
    assert!(!rig.plist(MENU_BAR_LABEL).exists());
}

#[test]
fn menu_bar_through_a_symlink_still_names_the_bundle() {
    let rig = Rig::new();
    let link = rig.root.join("bee-link");
    std::os::unix::fs::symlink(rig.bundle_dir.join("bee"), &link).expect("symlink");
    let output = rig.run(&link, &["install", "--menu-bar"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        stdout_json(&output)["program"],
        rig.bundle_dir
            .join("beekeeper-menubar")
            .display()
            .to_string()
    );
}

#[test]
fn menu_bar_refuses_a_bundle_without_one_despite_the_one_on_path() {
    let rig = Rig::new();
    fs::remove_file(rig.bundle_dir.join("beekeeper-menubar")).expect("remove");
    let output = rig.bee(&["install", "--menu-bar"]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    assert!(
        stderr(&output).contains("no beekeeper-menubar in"),
        "{}",
        stderr(&output)
    );
    assert!(!rig.plist(MENU_BAR_LABEL).exists());
    assert!(rig.calls().is_empty(), "{:?}", rig.calls());
}

#[test]
fn menu_bar_refuses_a_bee_outside_any_bundle() {
    let rig = Rig::new();
    let loose = rig.root.join("loose/bee");
    fs::create_dir_all(loose.parent().expect("parent")).expect("mkdir");
    fs::copy(env!("CARGO_BIN_EXE_bee"), &loose).expect("copy");
    script(&rig.root.join("loose/beekeeper-menubar"), "exit 0");
    let output = rig.run(&loose, &["install", "--menu-bar"]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    assert!(
        stderr(&output).contains("not in an app bundle"),
        "{}",
        stderr(&output)
    );
    assert!(!rig.plist(MENU_BAR_LABEL).exists());
    assert!(rig.calls().is_empty(), "{:?}", rig.calls());
}

#[test]
fn menu_bar_conflicts_with_every_host_only_flag() {
    let rig = Rig::new();
    for flags in [
        vec!["--program", "/bin/sh"],
        vec!["--system", "--user", "persona"],
        vec!["--system", "--user", "persona", "--print"],
    ] {
        let mut args = vec!["install", "--menu-bar"];
        args.extend(&flags);
        let output = rig.bee(&args);
        assert!(!output.status.success(), "{flags:?}");
        assert!(
            stderr(&output).contains("cannot be used with"),
            "{}",
            stderr(&output)
        );
    }
    assert!(rig.calls().is_empty(), "{:?}", rig.calls());
}

#[test]
fn a_menu_bar_launchctl_refused_leaves_no_registration_behind() {
    let rig = Rig::new();
    fs::write(rig.root.join("fail-bootstrap"), "").expect("arm the stub");
    let output = rig.bee(&["install", "--menu-bar"]);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("stub refused"),
        "{}",
        stderr(&output)
    );
    assert!(!rig.plist(MENU_BAR_LABEL).exists());
}

/// Install both, read them back, uninstall both: the whole lifecycle a person
/// walks, and the host refusal surviving a menu bar install along the way.
#[test]
fn the_lifecycle_keeps_the_two_registrations_and_the_refusal_straight() {
    let rig = Rig::new();
    assert!(rig.bee(&["install"]).status.success());
    assert!(rig.bee(&["install", "--menu-bar"]).status.success());
    assert_eq!(
        stdout_json(&rig.bee(&["install"]))["program"],
        rig.bundle_dir.join("beekeeper-host").display().to_string()
    );

    let installed = stdout_json(&rig.bee(&["installed"]));
    assert_eq!(installed["agentHost"]["installed"], true);
    assert_eq!(installed["menuBar"]["installed"], true);
    assert_eq!(
        installed["menuBar"]["program"],
        rig.bundle_dir
            .join("beekeeper-menubar")
            .display()
            .to_string()
    );

    assert!(rig.bee(&["uninstall"]).status.success());
    assert!(!rig.plist(HOST_LABEL).exists());
    assert!(!rig.plist(MENU_BAR_LABEL).exists());
    let calls = rig.calls().join("\n");
    assert!(calls.contains(&format!("/{HOST_LABEL}")), "{calls}");
    assert!(calls.contains(&format!("/{MENU_BAR_LABEL}")), "{calls}");

    assert!(rig.bee(&["install", "--menu-bar"]).status.success());
    let installed = stdout_json(&rig.bee(&["installed"]));
    assert_eq!(installed["menuBar"]["installed"], true);
    assert_eq!(installed["agentHost"]["installed"], false);
    assert_eq!(installed["login"], "declined");
}

#[test]
fn the_host_install_refuses_a_bundle_without_one_despite_the_one_on_path() {
    let rig = Rig::new();
    fs::remove_file(rig.bundle_dir.join("beekeeper-host")).expect("remove");
    let output = rig.bee(&["install"]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    assert!(
        stderr(&output).contains("will not register one from PATH"),
        "{}",
        stderr(&output)
    );
    assert!(!rig.plist(HOST_LABEL).exists());
    assert!(rig.calls().is_empty(), "{:?}", rig.calls());

    // `--program` is still the operator's own statement.
    let named = rig.stub_dir.join("beekeeper-host");
    let output = rig.bee(&["install", "--program", named.to_str().expect("utf8")]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout_json(&output)["program"], named.display().to_string());
}

#[test]
fn a_bee_outside_a_bundle_keeps_looking_on_path() {
    let rig = Rig::new();
    let loose = rig.root.join("loose/bee");
    fs::create_dir_all(loose.parent().expect("parent")).expect("mkdir");
    fs::copy(env!("CARGO_BIN_EXE_bee"), &loose).expect("copy");

    let output = rig.run(&loose, &["install"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        stdout_json(&output)["program"],
        rig.stub_dir.join("beekeeper-host").display().to_string()
    );
}
