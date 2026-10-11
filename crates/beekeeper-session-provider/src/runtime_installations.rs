//! Host-declared runtime code roots, reread at every bounded adapter launch.
//! These are read grants only; model custody uses a separate source.
use crate::session::CreateFailure;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Installations {
    runtimes: BTreeMap<String, Vec<PathBuf>>,
}

fn refusal() -> CreateFailure {
    crate::execution_scope::refuse(
        crate::execution_scope::EXECUTION_BOUNDARY_UNAVAILABLE,
        "the host-declared runtime installation roots are unavailable or invalid; no adapter was started",
    )
}

pub(crate) fn roots(
    state: &Path,
    instance: &str,
    cwd: &Path,
) -> Result<Vec<PathBuf>, CreateFailure> {
    let file = state.join("runtime-installations.json");
    let bytes = match std::fs::read(&file) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(refusal()),
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if std::fs::metadata(&file)
            .map_err(|_| refusal())?
            .permissions()
            .mode()
            & 0o077
            != 0
        {
            return Err(refusal());
        }
    }
    let installations: Installations = serde_json::from_slice(&bytes).map_err(|_| refusal())?;
    let selected = installations
        .runtimes
        .get(instance)
        .cloned()
        .unwrap_or_default();
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .and_then(|path| path.canonicalize().ok());
    let state = state.canonicalize().map_err(|_| refusal())?;
    let cwd = cwd.canonicalize().map_err(|_| refusal())?;
    let service_home = beekeeper_host_core::layout::home_dir()
        .map_err(|_| refusal())?
        .canonicalize()
        .map_err(|_| refusal())?;
    validate_roots(selected, &state, &cwd, home.as_deref(), &service_home)
}

fn validate_roots(
    selected: Vec<PathBuf>,
    state: &Path,
    cwd: &Path,
    native_home: Option<&Path>,
    service_home: &Path,
) -> Result<Vec<PathBuf>, CreateFailure> {
    use beekeeper_host_core::layout::{host_dir, Instance};
    let mut custody = vec![state.to_path_buf(), cwd.to_path_buf()];
    custody.extend(
        [Instance::Production, Instance::Dev].map(|instance| host_dir(service_home, instance)),
    );
    // A dedicated service home contains actor and sibling-provider custody too.
    // Native HOME descendants may legitimately contain installed runtime packages.
    if native_home != Some(service_home) {
        custody.push(service_home.to_path_buf());
    }
    selected
        .into_iter()
        .map(|root| {
            if !root.is_absolute() {
                return Err(refusal());
            }
            let root = root.canonicalize().map_err(|_| refusal())?;
            if !root.is_dir()
                || custody
                    .iter()
                    .any(|path| path.starts_with(&root) || root.starts_with(path))
                || native_home.is_some_and(|home| home.starts_with(&root))
            {
                return Err(refusal());
            }
            Ok(root)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn runtime_installations_cannot_grant_service_or_sibling_custody() {
        let dir = tempfile::tempdir().expect("fixture");
        let base = dir.path().canonicalize().expect("canonical fixture");
        let home = base.join("native-home");
        let service = base.join("service");
        let state = service.join("provider-a");
        let cwd = base.join("workspace");
        let package = home.join("runtime-package");
        let sibling = service.join("provider-b/actors");
        let production = beekeeper_host_core::layout::host_dir(
            &home,
            beekeeper_host_core::layout::Instance::Production,
        );
        let dev = beekeeper_host_core::layout::host_dir(
            &home,
            beekeeper_host_core::layout::Instance::Dev,
        );
        for path in [&state, &cwd, &package, &sibling, &production, &dev] {
            std::fs::create_dir_all(path).expect("directory");
        }
        assert!(validate_roots(vec![package.clone()], &state, &cwd, Some(&home), &service).is_ok());
        for root in [&service, &sibling, base.as_path()] {
            assert!(validate_roots(
                vec![root.to_path_buf()],
                &state,
                &cwd,
                Some(&home),
                &service
            )
            .is_err());
        }
        for root in [&production, &dev, &home] {
            assert!(
                validate_roots(vec![root.to_path_buf()], &state, &cwd, Some(&home), &home).is_err()
            );
        }
        assert!(validate_roots(vec![package], &state, &cwd, Some(&home), &home).is_ok());
    }
    #[test]
    fn runtime_installations_require_explicit_narrow_existing_roots_and_reread() {
        let dir = tempfile::tempdir().expect("fixture");
        let state = dir.path().join("state");
        let cwd = dir.path().join("workspace");
        let package = dir.path().join("adapter");
        for path in [&state, &cwd, &package] {
            std::fs::create_dir(path).expect("directory");
        }
        let file = state.join("runtime-installations.json");
        assert!(roots(&state, "codex-primary", &cwd)
            .expect("legacy")
            .is_empty());
        let write = |root: &Path| {
            crate::state::atomic_write(
                &file,
                serde_json::to_vec(&serde_json::json!({"runtimes":{"codex-primary":[root]}}))
                    .expect("json")
                    .as_slice(),
            )
            .expect("record")
        };
        write(&package);
        assert_eq!(
            roots(&state, "codex-primary", &cwd).expect("package"),
            vec![package.canonicalize().expect("canonical")]
        );
        assert!(roots(&state, "claude-primary", &cwd)
            .expect("distinct runtime")
            .is_empty());
        write(dir.path());
        assert!(roots(&state, "codex-primary", &cwd).is_err());
        write(&cwd);
        assert!(roots(&state, "codex-primary", &cwd).is_err());
        write(Path::new("relative"));
        assert!(roots(&state, "codex-primary", &cwd).is_err());
        write(&package);
        std::fs::remove_dir(&package).expect("uninstall");
        assert!(roots(&state, "codex-primary", &cwd).is_err());
    }
}
