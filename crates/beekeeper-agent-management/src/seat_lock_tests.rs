use super::*;
use std::sync::mpsc;

#[test]
fn stage_and_consume_share_one_lock_without_resurrecting_or_losing_entries() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("actor-seats.json");
    save(
        &path,
        &json!({"version":1,"pending":{"old":{"fixture":true}}}),
    )
    .unwrap();
    let (read_tx, read_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    std::thread::scope(|scope| {
        let stage_path = &path;
        let stage = scope.spawn(move || {
            update_actor_seats(stage_path, |pending| {
                read_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                pending.insert(
                    "new".into(),
                    json!({"packDir":"role","personaId":"builder"}),
                );
                Ok(())
            })
        });
        read_rx.recv().unwrap();
        let probe = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path.with_extension("lock"))
            .unwrap();
        let blocked = probe.try_lock().is_err();
        drop(probe);
        let consume = scope
            .spawn(|| beekeeper_session_provider::actor_seats::consume_seat(Some(&path), "old"));
        release_tx.send(()).unwrap();
        stage.join().unwrap().unwrap();
        consume.join().unwrap().unwrap();
        assert!(blocked, "stage must hold the provider's lock across RMW");
    });
    let value: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert!(value["pending"].get("old").is_none());
    assert_eq!(value["pending"]["new"]["personaId"], "builder");
    // Consumption first must also remain consumed when another stage arrives.
    beekeeper_session_provider::actor_seats::consume_seat(Some(&path), "new").unwrap();
    update_actor_seats(&path, |pending| {
        assert!(pending.is_empty());
        pending.insert("later".into(), json!({"packDir":"later"}));
        Ok(())
    })
    .unwrap();
    let value: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(value["pending"].as_object().unwrap().len(), 1);
    assert_eq!(value["pending"]["later"]["packDir"], "later");
}
