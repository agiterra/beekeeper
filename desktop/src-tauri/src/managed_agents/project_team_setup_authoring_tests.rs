use super::*;

fn fixture() -> (tempfile::TempDir, ProjectTeamSetupDraft, Keys) {
    let root = tempfile::tempdir().expect("temp");
    let owner = Keys::generate();
    let directory = root.path().join("draft");
    std::fs::create_dir(&directory).expect("draft");
    let draft = ProjectTeamSetupDraft {
        setup_id: Uuid::new_v4().to_string(),
        project_ref: format!("30621:{}:garden", owner.public_key().to_hex()),
        project_directory: root.path().join("project").to_string_lossy().into_owned(),
        draft_directory: directory.to_string_lossy().into_owned(),
        roles_directory: directory
            .join("personas/roles")
            .to_string_lossy()
            .into_owned(),
        status: super::super::SetupStatus::Draft,
        intent: "Build a garden".into(),
        owner_pubkey: owner.public_key().to_hex(),
        relay_url: "wss://garden.example".into(),
        roles: Vec::new(),
        expected_roles: Vec::new(),
        created_at: crate::util::now_iso(),
        latest_snapshot_id: None,
    };
    (root, draft, owner)
}

fn read_journal(draft: &ProjectTeamSetupDraft) -> AuthoringJournal {
    serde_json::from_slice(&std::fs::read(journal_path(draft).expect("path")).expect("read"))
        .expect("journal")
}

fn save_journal(draft: &ProjectTeamSetupDraft, journal: &AuthoringJournal) {
    std::fs::write(
        journal_path(draft).expect("path"),
        serde_json::to_vec(journal).expect("json"),
    )
    .expect("write");
}

#[test]
fn reservation_retries_preserve_exact_signed_genesis_and_all_ids() {
    let (_root, draft, owner) = fixture();
    let channel = Uuid::new_v4().to_string();
    assert!(read_reservation(&draft).expect("read").is_none());
    assert!(!journal_path(&draft).expect("path").exists());
    let first = reserve(&draft, &owner, &channel).expect("reserve");
    assert!(first
        .create_command_id
        .strip_prefix("csl-")
        .is_some_and(canonical_uuid));
    let before = std::fs::read(journal_path(&draft).expect("path")).expect("bytes");
    let second = reserve(&draft, &owner, &channel).expect("retry");
    assert_eq!(first, second);
    assert_eq!(read_reservation(&draft).expect("read"), Some(first));
    assert_eq!(
        before,
        std::fs::read(journal_path(&draft).expect("path")).expect("bytes")
    );
    assert!(std::fs::read_dir(&draft.draft_directory)
        .expect("draft")
        .next()
        .is_none());
}

#[test]
fn channel_and_owner_changes_cannot_retarget_existing_reservation() {
    let (_root, draft, owner) = fixture();
    let channel = Uuid::new_v4().to_string();
    let first = reserve(&draft, &owner, &channel).expect("reserve");
    assert_eq!(
        reserve(&draft, &owner, &Uuid::new_v4().to_string())
            .expect_err("channel")
            .code,
        "existing_authoring"
    );
    assert_eq!(
        reserve(&draft, &Keys::generate(), &channel)
            .expect_err("owner")
            .code,
        "scope_changed"
    );
    assert_eq!(read_reservation(&draft).expect("read"), Some(first));
}

#[test]
fn invalid_channel_has_no_effects() {
    let (root, draft, owner) = fixture();
    assert!(reserve(&draft, &owner, "not-a-channel").is_err());
    assert_eq!(
        std::fs::read_dir(root.path()).expect("directory").count(),
        1
    );
}

#[test]
fn journal_scope_mismatch_is_refused() {
    let (_root, draft, owner) = fixture();
    reserve(&draft, &owner, &Uuid::new_v4().to_string()).expect("reserve");
    for field in ["setup", "project", "owner", "community"] {
        let mut journal = read_journal(&draft);
        match field {
            "setup" => journal.setup_id = Uuid::new_v4().to_string(),
            "project" => journal.project_ref.push_str("-other"),
            "owner" => journal.owner_pubkey = Keys::generate().public_key().to_hex(),
            _ => journal.relay_url = "wss://other.example".into(),
        }
        assert!(validate_journal(&journal, &draft).is_err(), "{field}");
    }
}

#[test]
fn forged_and_wrongly_bound_genesis_events_are_refused_on_load() {
    let (_root, draft, owner) = fixture();
    let channel = Uuid::new_v4().to_string();
    reserve(&draft, &owner, &channel).expect("reserve");
    let mut forged = read_journal(&draft);
    forged.reservation.genesis_event.content = "forged".into();
    seal_journal(&mut forged, &owner).expect("seal forged event for validation");
    save_journal(&draft, &forged);
    assert!(read_reservation(&draft).is_err());

    for mismatch in ["kind", "channel", "session", "signer"] {
        let mut journal = read_journal(&draft);
        let session = if mismatch == "session" {
            Uuid::new_v4().to_string()
        } else {
            journal.reservation.session_ref.clone()
        };
        let event_channel = if mismatch == "channel" {
            Uuid::new_v4()
        } else {
            Uuid::parse_str(&channel).expect("channel")
        };
        let event_keys = if mismatch == "signer" {
            Keys::generate()
        } else {
            owner.clone()
        };
        let valid = beekeeper_sdk_pkg::build_coding_session_genesis(
            event_channel,
            &CodingSessionGenesisPayload::new(session),
        )
        .expect("build")
        .sign_with_keys(&event_keys)
        .expect("sign");
        let event = if mismatch == "kind" {
            nostr::EventBuilder::new(nostr::Kind::Custom(1), valid.content.clone())
                .tags(valid.tags.clone())
                .sign_with_keys(&owner)
                .expect("sign wrong kind")
        } else {
            valid
        };
        journal.reservation.genesis_event_id = event.id.to_hex();
        journal.reservation.genesis_event = event;
        seal_journal(&mut journal, &owner).expect("seal mismatched event for validation");
        save_journal(&draft, &journal);
        assert!(read_reservation(&draft).is_err(), "{mismatch}");
    }
}

#[test]
fn edited_retry_identifiers_are_rejected_without_replacing_the_journal() {
    let (_root, draft, owner) = fixture();
    let channel = Uuid::new_v4().to_string();
    reserve(&draft, &owner, &channel).expect("reserve");
    let original = read_journal(&draft);
    for field in ["command", "authoring"] {
        let mut journal: AuthoringJournal =
            serde_json::from_value(serde_json::to_value(&original).expect("value")).expect("copy");
        if field == "command" {
            journal.reservation.create_command_id = format!("csl-{}", Uuid::new_v4());
        } else {
            journal.reservation.authoring_id = Uuid::new_v4().to_string();
        }
        save_journal(&draft, &journal);
        let path = journal_path(&draft).expect("path");
        let before = std::fs::read(&path).expect("bytes");
        assert!(read_reservation(&draft)
            .expect_err("edited identifier")
            .message
            .contains("owner seal"));
        assert!(reserve(&draft, &owner, &channel).is_err());
        assert_eq!(std::fs::read(path).expect("preserved"), before);
    }
}

#[test]
fn same_owner_reservation_cannot_be_transplanted_by_rewriting_scope_metadata() {
    let (_root, draft, owner) = fixture();
    reserve(&draft, &owner, &Uuid::new_v4().to_string()).expect("reserve");
    for field in ["setup", "project", "community"] {
        let mut other = draft.clone();
        let mut journal = read_journal(&draft);
        match field {
            "setup" => other.setup_id = Uuid::new_v4().to_string(),
            "project" => other.project_ref.push_str("-other"),
            _ => other.relay_url = "wss://other.example".into(),
        }
        journal.setup_id = other.setup_id.clone();
        journal.project_ref = other.project_ref.clone();
        journal.relay_url = other.relay_url.clone();
        assert!(
            validate_journal(&journal, &other)
                .expect_err("transplanted scope")
                .message
                .contains("owner seal"),
            "{field}"
        );
    }
}

#[test]
fn concurrent_reservations_share_one_durable_event() {
    let (_root, draft, owner) = fixture();
    let channel = Uuid::new_v4().to_string();
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let draft = draft.clone();
            let owner = owner.clone();
            let channel = channel.clone();
            std::thread::spawn(move || reserve(&draft, &owner, &channel).expect("reserve"))
        })
        .collect();
    let results: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().expect("thread"))
        .collect();
    assert!(results.iter().all(|result| result == &results[0]));
}

#[cfg(unix)]
#[test]
fn private_journal_rejects_links_and_preserves_the_target() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let (root, draft, owner) = fixture();
    let channel = Uuid::new_v4().to_string();
    reserve(&draft, &owner, &channel).expect("reserve");
    let path = journal_path(&draft).expect("path");
    assert_eq!(
        std::fs::metadata(&path)
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    let saved = root.path().join("saved.json");
    std::fs::rename(&path, &saved).expect("move");
    let bytes = std::fs::read(&saved).expect("bytes");
    symlink(&saved, &path).expect("link");
    assert!(read_reservation(&draft).is_err());
    assert!(reserve(&draft, &owner, &channel).is_err());
    assert_eq!(std::fs::read(saved).expect("bytes"), bytes);
}

#[test]
fn supplied_setup_id_must_match_the_scoped_draft() {
    let (root, mut draft, _owner) = fixture();
    let scope =
        SetupScope::new(&draft.project_ref, &draft.owner_pubkey, &draft.relay_url).expect("scope");
    let storage = root.path().join("storage");
    let directory = scope.directory(&storage);
    std::fs::create_dir_all(directory.join("draft")).expect("storage");
    let directory = std::fs::canonicalize(directory).expect("canonical");
    draft.draft_directory = directory.join("draft").to_string_lossy().into_owned();
    draft.roles_directory = directory
        .join("draft/personas/roles")
        .to_string_lossy()
        .into_owned();
    std::fs::write(
        directory.join("record.json"),
        serde_json::to_vec(&draft).expect("json"),
    )
    .expect("record");
    assert!(bound_draft(&storage, &scope, &draft.setup_id).is_ok());
    assert!(bound_draft(&storage, &scope, &Uuid::new_v4().to_string()).is_err());
}

#[test]
fn oversized_journal_is_refused_without_replacing_it() {
    let (_root, draft, owner) = fixture();
    let path = journal_path(&draft).expect("path");
    let bytes = vec![b'x'; JOURNAL_LIMIT as usize + 1];
    std::fs::write(&path, &bytes).expect("oversized");
    assert!(read_reservation(&draft).is_err());
    assert!(reserve(&draft, &owner, &Uuid::new_v4().to_string()).is_err());
    assert_eq!(std::fs::read(path).expect("preserved"), bytes);
}

#[test]
fn local_brief_accepts_full_intent_and_keeps_writes_scoped_to_draft() {
    let (_root, mut draft, _) = fixture();
    draft.intent = "x".repeat(16 * 1024);
    let brief = authoring_prompt(&draft).expect("full supported intent");
    assert!(brief.contains(&draft.intent));
    assert!(brief.contains(&draft.project_directory));
    assert!(brief.contains(&draft.roles_directory));
    assert!(brief.contains("Ordinary solo sessions must remain possible"));
    assert!(brief.contains("Do not publish packs"));
    draft.project_directory = "x".repeat(32 * 1024);
    assert!(authoring_prompt(&draft).is_err());
}

#[test]
fn brief_ipc_returns_saved_bytes_renders_when_absent_and_refuses_another_scope() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("storage");
    let owner = Keys::generate().public_key().to_hex();
    let draft =
        super::super::tests::write_bound_draft(&root, &owner, "wss://garden.example", "garden");
    let root = std::fs::canonicalize(&root).expect("canonical root");
    let scope = SetupScope::new(&draft.project_ref, &owner, &draft.relay_url).expect("scope");
    let saved = Path::new(&draft.draft_directory).join("PROJECT_TEAM_SETUP.md");

    let rendered = read_brief(&root, &scope, &draft.setup_id).expect("rendered brief");
    assert_eq!(rendered.text, authoring_prompt(&draft).expect("prompt"));
    assert!(!saved.exists(), "reading the brief must not write it");

    let bytes = "Saved brief — edited by an earlier build.\n\nKeep exact bytes.\r\n";
    std::fs::write(&saved, bytes).expect("saved brief");
    let brief = read_brief(&root, &scope, &draft.setup_id).expect("saved brief");
    assert_eq!(brief.text.as_bytes(), bytes.as_bytes());
    assert_eq!(
        serde_json::to_value(&brief).expect("json"),
        serde_json::json!({ "text": bytes })
    );

    let other_relay =
        SetupScope::new(&draft.project_ref, &owner, "wss://orchard.example").expect("scope");
    assert!(read_brief(&root, &other_relay, &draft.setup_id).is_err());
    let other_owner = Keys::generate().public_key().to_hex();
    let other_owner =
        SetupScope::new(&draft.project_ref, &other_owner, &draft.relay_url).expect("scope");
    assert!(read_brief(&root, &other_owner, &draft.setup_id).is_err());
    assert!(read_brief(&root, &scope, &Uuid::new_v4().to_string()).is_err());
}
