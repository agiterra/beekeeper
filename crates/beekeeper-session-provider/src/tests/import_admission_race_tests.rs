use super::*;
use std::path::PathBuf;

#[tokio::test]
#[ignore = "isolated process fixture invoked only by the pair-lock reservation regression"]
async fn isolated_reserved_writer_fixture() {
    let root = PathBuf::from(
        std::env::var_os("BEEKEEPER_IMPORT_ADMISSION_FIXTURE").expect("isolated fixture"),
    );
    let command = std::env::var("BEEKEEPER_IMPORT_OPERATION").expect("operation");
    let actor = "cd".repeat(32);
    let channel = Uuid::new_v4();
    let work = root.join("work");
    std::fs::create_dir_all(&work).expect("work");
    let projects = write_projects(&root, channel, &work);
    write_actor_seats(&root, &command, &actor);
    let mut provider = provider(&root.join("state"), Some(&projects));
    let event = seated_create_event(&provider, channel, &command, &actor, "lead");
    provider
        .handle_command_event(channel, &event)
        .await
        .expect("bounded busy outcome");
    assert!(
        provider.state.lifecycle_start(&command).is_none(),
        "no reservation escapes the held pair fence"
    );
    assert!(!provider.state.is_command_consumed(&command));
    assert_eq!(provider.sessions.live_count(), 0);
    assert_eq!(provider.state.sessions().count(), 0);
    assert!(provider.outbox.pending_events().any(|event| {
        payload::decode_coding_session_lifecycle_receipt(&event.content).is_ok_and(|receipt| {
            receipt.command_id == command
                && receipt
                    .error
                    .is_some_and(|failure| failure.code == "ACTOR_OWNERSHIP_BUSY")
        })
    }));
}

#[tokio::test]
async fn held_import_pair_fences_actual_create_before_reservation_or_consumption() {
    use beekeeper_host_core::{layout::Instance, ownership};
    let root = tempfile::tempdir().expect("fixture home");
    let actor = "cd".repeat(32);
    let command = Uuid::new_v4().to_string();
    let mut expected = ownership::Transfer {
        operation_id: command.clone(),
        actor_pubkey: actor.clone(),
        relay_url: "ws://localhost:3000".into(),
        legacy_nonce: "legacy-generation".into(),
        definition_event_id: "ab".repeat(32),
        target: None,
        phase: ownership::Phase::Prepared,
    };
    ownership::advance(root.path(), Instance::Dev, expected.clone()).expect("prepared");
    expected.phase = ownership::Phase::Quiesced;
    ownership::advance(root.path(), Instance::Dev, expected.clone()).expect("quiesced");
    let _pair = ownership::admission_lock(root.path(), Instance::Dev, &actor, &expected.relay_url)
        .expect("import fence");
    let output = tokio::process::Command::new(std::env::current_exe().expect("test binary"))
        .args([
            "--exact",
            "tests::import_admission_race_tests::isolated_reserved_writer_fixture",
            "--ignored",
        ])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env(beekeeper_host_core::layout::HOME_VAR, root.path())
        .env(beekeeper_host_core::layout::INSTANCE_VAR, "dev")
        .env("BEEKEEPER_IMPORT_ADMISSION_FIXTURE", root.path())
        .env("BEEKEEPER_IMPORT_OPERATION", &command)
        .output()
        .await
        .expect("owned subprocess");
    assert!(
        output.status.success(),
        "isolated writer assertion failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let state = StateStore::open(&root.path().join("state"), 3600)
        .expect("read existing actual child fixture state");
    assert!(state.lifecycle_start(&command).is_none());
    assert!(!state.is_command_consumed(&command));
    assert_eq!(state.sessions().count(), 0);
    assert_eq!(
        ownership::for_pair(root.path(), Instance::Dev, &actor, &expected.relay_url)
            .expect("ownership")
            .expect("transfer")
            .definition_event_id,
        expected.definition_event_id
    );
}
