//! Serialize the shared workdir record and its provider view as one mutation.
//! A per-session launch lock cannot protect hints belonging to other sessions.

use std::path::Path;
use tauri::AppHandle;

/// Held from before reading the shared store until its provider view is written.
/// The OS lock also covers a second desktop process using the same app-data dir.
pub(crate) fn lock_workdir_store(app: &AppHandle) -> Result<std::fs::File, String> {
    lock_path(&super::workdir_store_path(app)?.with_extension("lock"))
}

fn lock_path(path: &Path) -> Result<std::fs::File, String> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if !meta.is_file() || meta.file_type().is_symlink() => {
            return Err("The workdir store lock must be a regular file.".into());
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(path).map_err(|error| error.to_string())?;
    if !file
        .metadata()
        .map_err(|error| error.to_string())?
        .is_file()
    {
        return Err("The workdir store lock must be a regular file.".into());
    }
    file.lock().map_err(|error| error.to_string())?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concurrent_hints_preserve_each_other_and_existing_project_defaults() {
        let root = tempfile::tempdir().expect("temp");
        let path = root.path().join("store.json");
        let mut store = super::super::CodingSessionWorkdirStore::default();
        store.stage_hint_for_project(
            "first",
            Some("project"),
            root.path().join("canonical"),
            None,
        );
        std::fs::write(&path, serde_json::to_vec(&store).expect("json")).expect("write");
        let workers: Vec<_> = (0..12)
            .map(|index| {
                let path = path.clone();
                std::thread::spawn(move || {
                    let _lock = lock_path(&path.with_extension("lock")).expect("lock");
                    let mut store: super::super::CodingSessionWorkdirStore =
                        serde_json::from_slice(&std::fs::read(&path).expect("read"))
                            .expect("store");
                    store.stage_hint(
                        &format!("request-{index}"),
                        path.with_file_name(format!("draft-{index}")),
                    );
                    std::thread::yield_now();
                    std::fs::write(&path, serde_json::to_vec(&store).expect("json"))
                        .expect("write");
                })
            })
            .collect();
        for worker in workers {
            worker.join().expect("worker");
        }
        let saved: super::super::CodingSessionWorkdirStore =
            serde_json::from_slice(&std::fs::read(path).expect("read")).expect("store");
        assert_eq!(saved.pending.len(), 13);
        assert_eq!(saved.by_project, store.by_project);
        assert_eq!(saved.mru, store.mru);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_lock_is_rejected_without_changing_its_target() {
        let root = tempfile::tempdir().expect("temp");
        let target = root.path().join("target");
        std::fs::write(&target, "keep").expect("write");
        let path = root.path().join("lock");
        std::os::unix::fs::symlink(&target, &path).expect("link");
        assert!(lock_path(&path).is_err());
        assert_eq!(std::fs::read_to_string(target).expect("read"), "keep");
    }
}
