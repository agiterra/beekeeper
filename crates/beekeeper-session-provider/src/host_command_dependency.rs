//! Host-installed executable prerequisite checks, without running code.
//!
//! Relative paths are artifacts of a potentially not-yet-materialized bound
//! checkout, not host dependencies. They remain checked by actual bounded
//! materialization/spawn rather than incorrectly probing the old checkout.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use super::{PrepareRefusal, ACTION_HOST_EXECUTABLE_MISSING, ACTION_STEP_INVALID};

fn executable(path: &Path) -> bool {
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn candidates(path: PathBuf, extensions: &str) -> Vec<PathBuf> {
    let mut paths = vec![path.clone()];
    if cfg!(windows) && path.extension().is_none() {
        paths.extend(
            extensions
                .split(';')
                .filter(|s| !s.is_empty())
                .map(|suffix| {
                    let mut name = path.as_os_str().to_owned();
                    name.push(suffix);
                    PathBuf::from(name)
                }),
        );
    }
    paths
}

pub(super) fn check(
    command: &[String],
    cwd: &Path,
    overlay: &BTreeMap<String, String>,
    host_env: &HashMap<String, String>,
) -> Result<(), PrepareRefusal> {
    let Some(program) = command.first().filter(|program| !program.is_empty()) else {
        return Err(PrepareRefusal {
            code: ACTION_STEP_INVALID.into(),
            message: "run_on_host command is empty".into(),
        });
    };
    let path = Path::new(program);
    if !path.is_absolute() && path.components().count() > 1 {
        // A relative artifact may exist only at the bound trigger revision.
        return Ok(());
    }
    let effective = |name: &str| {
        overlay
            .get(name)
            .or_else(|| host_env.get(name))
            .cloned()
            .or_else(|| std::env::var(name).ok())
    };
    let extensions = effective("PATHEXT").unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".into());
    let available = if path.is_absolute() {
        candidates(path.to_path_buf(), &extensions)
            .iter()
            .any(|candidate| executable(candidate))
    } else {
        effective("PATH").is_some_and(|search| {
            std::env::split_paths(&search).any(|directory| {
                let directory = if directory.is_absolute() {
                    directory
                } else {
                    cwd.join(directory)
                };
                candidates(directory.join(program), &extensions)
                    .iter()
                    .any(|candidate| executable(candidate))
            })
        })
    };
    if !available {
        return Err(PrepareRefusal {
            code: ACTION_HOST_EXECUTABLE_MISSING.into(),
            message: format!(
                "host executable {program:?} is absent or not executable in the configured PATH; \
                 install the prerequisite on this executor before a new invocation"
            ),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_host_executable_refuses_before_a_claim_can_be_built() {
        let tree = tempfile::tempdir().expect("tree");
        let env = HashMap::from([("PATH".into(), tree.path().display().to_string())]);
        let refusal = check(
            &["missing-hook-runtime".into()],
            tree.path(),
            &BTreeMap::new(),
            &env,
        )
        .expect_err("uninstalled runtime must refuse");
        assert_eq!(refusal.code, ACTION_HOST_EXECUTABLE_MISSING);
    }

    #[cfg(unix)]
    #[test]
    fn configured_path_and_execute_permission_are_checked_without_running_code() {
        use std::os::unix::fs::PermissionsExt;
        let tree = tempfile::tempdir().expect("tree");
        let program = tree.path().join("hook-runtime");
        std::fs::write(&program, "must not run").expect("fixture");
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o600))
            .expect("permissions");
        let env = HashMap::from([("PATH".into(), tree.path().display().to_string())]);
        let command = vec!["hook-runtime".into()];
        assert!(check(&command, tree.path(), &BTreeMap::new(), &env).is_err());
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700))
            .expect("permissions");
        assert!(check(&command, tree.path(), &BTreeMap::new(), &env).is_ok());
        let overlay = BTreeMap::from([(
            "PATH".into(),
            tree.path().join("missing").display().to_string(),
        )]);
        assert!(check(&command, tree.path(), &overlay, &env).is_err());
    }

    #[test]
    fn relative_program_is_left_to_materialization_of_its_bound_artifact() {
        let tree = tempfile::tempdir().expect("tree");
        assert!(check(
            &["./hooks/new-at-trigger-commit".into()],
            tree.path(),
            &BTreeMap::new(),
            &HashMap::new()
        )
        .is_ok());
    }
}
