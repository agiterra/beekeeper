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
    let name = executable_basename("beekeeper-session-provider");
    let bundled = exe.parent().expect("parent").join(&name);
    executable_file(&bundled);
    for root in [&workspace, &cwd] {
        for profile in ["debug", "release"] {
            executable_file(&root.join("target").join(profile).join(&name));
        }
    }
    assert_eq!(
        resolve_workspace_command(
            "beekeeper-session-provider",
            &workspace,
            Some(&cwd),
            Some(&exe)
        ),
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
        .join(executable_basename("beekeeper-session-provider"));
    executable_file(&stale);
    assert_eq!(
        resolve_workspace_command("beekeeper-session-provider", temp.path(), None, Some(&exe)),
        None
    );
}

#[test]
fn unbundled_development_preserves_the_workspace_profile_preference() {
    let temp = tempfile::tempdir().expect("tempdir");
    let name = executable_basename("beekeeper-acp");
    let debug = temp.path().join("target/debug").join(&name);
    let release = temp.path().join("target/release").join(&name);
    executable_file(&debug);
    executable_file(&release);
    let exe = temp
        .path()
        .join("desktop/src-tauri/target/debug/beekeeper-desktop");
    assert_eq!(
        resolve_workspace_command("beekeeper-acp", temp.path(), None, Some(&exe)),
        Some(if cfg!(debug_assertions) {
            debug
        } else {
            release
        })
    );
}

#[test]
fn only_a_bundles_own_helpers_are_confined_to_it() {
    let bundled = Path::new("/x/Beekeeper Host.app/Contents/MacOS/beekeeper-host");
    let loose = Path::new("/x/target/debug/beekeeper-host");
    for helper in ["beekeeper-session-provider", "beekeeper-dev-mcp", "bee"] {
        assert!(must_come_from_bundle(helper, Some(bundled)), "{helper}");
        assert!(!must_come_from_bundle(helper, Some(loose)), "{helper}");
        assert!(!must_come_from_bundle(helper, None), "{helper}");
    }
    for external in ["claude-agent-acp", "claude", "node", "git", "sh"] {
        assert!(
            !must_come_from_bundle(external, Some(bundled)),
            "{external}"
        );
    }
}

#[cfg(unix)]
#[test]
fn a_symlink_into_a_bundle_reads_as_the_bundle_and_any_other_stays_as_given() {
    let temp = tempfile::tempdir().expect("tempdir");
    let real = temp
        .path()
        .canonicalize()
        .expect("canonical")
        .join("Host.app/Contents/MacOS/bee");
    executable_file(&real);
    let link = temp.path().join("bee");
    std::os::unix::fs::symlink(&real, &link).expect("symlink");
    assert_eq!(through_bundle_symlink(&link), real);

    let loose = temp.path().join("target/debug/bee");
    executable_file(&loose);
    let loose_link = temp.path().join("loose-bee");
    std::os::unix::fs::symlink(&loose, &loose_link).expect("symlink");
    assert_eq!(through_bundle_symlink(&loose_link), loose_link);
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
