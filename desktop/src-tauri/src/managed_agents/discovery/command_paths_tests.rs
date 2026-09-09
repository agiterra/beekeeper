use super::*;

fn executable_file(path: &Path) {
    std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    std::fs::write(path, "#!/bin/sh\nexit 0\n").expect("write executable");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }
}

#[test]
fn installed_debug_bundle_uses_its_provider_over_both_source_checkouts() {
    let temp = tempfile::tempdir().expect("tempdir");
    let workspace = temp.path().join("compiled-checkout");
    let cwd = temp.path().join("launch-checkout");
    let exe = temp
        .path()
        .join("Beekeeper Dev.app/Contents/MacOS/beekeeper-desktop");
    let name = executable_basename("buzz-session-provider");
    let bundled = exe.parent().expect("parent").join(&name);
    executable_file(&bundled);
    for root in [&workspace, &cwd] {
        for profile in ["debug", "release"] {
            executable_file(&root.join("target").join(profile).join(&name));
        }
    }
    assert_eq!(
        resolve_workspace_command("buzz-session-provider", &workspace, Some(&cwd), Some(&exe)),
        Some(bundled)
    );
}

#[test]
fn missing_bundle_sidecar_does_not_select_an_old_workspace_binary() {
    let temp = tempfile::tempdir().expect("tempdir");
    let exe = temp
        .path()
        .join("Beekeeper.app/Contents/MacOS/beekeeper-desktop");
    let stale = temp
        .path()
        .join("target/debug")
        .join(executable_basename("buzz-session-provider"));
    executable_file(&stale);
    assert_eq!(
        resolve_workspace_command("buzz-session-provider", temp.path(), None, Some(&exe)),
        None
    );
}

#[test]
fn unbundled_development_preserves_the_workspace_profile_preference() {
    let temp = tempfile::tempdir().expect("tempdir");
    let name = executable_basename("buzz-acp");
    let debug = temp.path().join("target/debug").join(&name);
    let release = temp.path().join("target/release").join(&name);
    executable_file(&debug);
    executable_file(&release);
    let exe = temp
        .path()
        .join("desktop/src-tauri/target/debug/beekeeper-desktop");
    assert_eq!(
        resolve_workspace_command("buzz-acp", temp.path(), None, Some(&exe)),
        Some(if cfg!(debug_assertions) {
            debug
        } else {
            release
        })
    );
}

#[test]
fn explicit_command_override_remains_explicit_in_a_bundle() {
    let temp = tempfile::tempdir().expect("tempdir");
    let custom = temp.path().join(executable_basename("custom-provider"));
    executable_file(&custom);
    let exe = temp
        .path()
        .join("Beekeeper.app/Contents/MacOS/beekeeper-desktop");
    assert_eq!(
        resolve_workspace_command(
            custom.to_str().expect("utf8"),
            temp.path(),
            None,
            Some(&exe)
        ),
        Some(custom)
    );
}
