use super::*;

fn scope() -> BackupScope {
    BackupScope {
        actor_pubkey: nostr::Keys::generate().public_key().to_hex(),
        relay_url: "ws://127.0.0.1:3037/".into(),
        operation_id: uuid::Uuid::new_v4().to_string(),
    }
}

#[test]
fn partial_capture_resumes_and_completed_custody_cannot_be_replaced() {
    let dir = tempfile::tempdir().unwrap();
    let backup = dir.path().join("backup");
    atomic_write::create_dir_all_restricted(&backup).unwrap();
    atomic_write::atomic_write_json_restricted(&backup.join("key"), b"scoped-key").unwrap();
    let files = [
        BackupFile {
            name: "key",
            bytes: b"scoped-key",
        },
        BackupFile {
            name: "record.json",
            bytes: b"preserved-config",
        },
    ];
    let identity = scope();
    let first = capture(&backup, identity.clone(), &files).unwrap();
    assert!(capture(&backup, identity.clone(), &files).unwrap() == first);
    let changed = [BackupFile {
        name: "key",
        bytes: b"replacement",
    }];
    assert!(capture(&backup, identity, &changed).is_err());
    assert_eq!(std::fs::read(backup.join("key")).unwrap(), b"scoped-key");
    assert!(verify(&backup).unwrap() == first);
}

#[test]
fn tampered_context_and_path_escape_block_adoption() {
    let dir = tempfile::tempdir().unwrap();
    let files = [BackupFile {
        name: "native-context.json",
        bytes: b"retained-cursor",
    }];
    let backup = dir.path().join("backup");
    capture(&backup, scope(), &files).unwrap();
    std::fs::write(backup.join("native-context.json"), b"different-cursor").unwrap();
    assert!(verify(&backup).is_err());
    let escaped = [BackupFile {
        name: "../key",
        bytes: b"key",
    }];
    assert!(capture(dir.path(), scope(), &escaped).is_err());
}

#[cfg(unix)]
#[test]
fn symlinked_custody_directory_is_refused_without_writing_external_files() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let backup = dir.path().join("backup");
    atomic_write::create_dir_all_restricted(&backup).unwrap();
    std::os::unix::fs::symlink(outside.path(), backup.join("nested")).unwrap();
    let files = [BackupFile {
        name: "nested/key",
        bytes: b"custody",
    }];
    assert!(capture(&backup, scope(), &files).is_err());
    assert!(!outside.path().join("key").exists());
    assert!(!backup.join("manifest.json").exists());
}
