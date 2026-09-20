//! Serialize the shared workdir record and its provider view as one mutation.
//! A per-session launch lock cannot protect hints belonging to other sessions.
//!
//! The protocol itself lives in
//! [`buzz_session_provider_pkg::assignment_inputs::lock_store_file`], because the
//! sidecar provider writes the same file: two implementations of one advisory
//! lock is two chances to disagree about which file it is and what may be
//! followed to get there.

use tauri::AppHandle;

/// Held from before reading the shared store until its provider view is written.
/// The OS lock also covers a second desktop process using the same app-data dir,
/// and the sidecar provider, which takes the same lock on the same path.
pub(crate) fn lock_workdir_store(app: &AppHandle) -> Result<std::fs::File, String> {
    buzz_session_provider_pkg::assignment_inputs::lock_store_file(&super::workdir_store_path(app)?)
}

#[cfg(test)]
mod tests {
    use buzz_session_provider_pkg::assignment_inputs::lock_store_file;

    /// The lock is taken on the store path's `.lock` sibling, and holding it
    /// serializes writers that each opened it for themselves — which is what
    /// the app and its sidecar provider are.
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
                    let _lock = lock_store_file(&path).expect("lock");
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
        // `lock_store_file` locks the `.lock` sibling of the path it is given.
        let store = root.path().join("store.json");
        std::os::unix::fs::symlink(&target, store.with_extension("lock")).expect("link");
        assert!(lock_store_file(&store).is_err());
        assert_eq!(std::fs::read_to_string(target).expect("read"), "keep");
    }
}
