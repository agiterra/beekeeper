use super::*;

const OWNER: &str = "6cbdf4451d3989c10c20d13240c665a9e11e3959a95488382193481692b68df2";

fn request(snapshot_id: &str) -> PublicationRequest {
    PublicationRequest {
        destination: PublicationDestination {
            repo_ref: format!("30617:{OWNER}:garden-packs"),
            pack_path: DEFAULT_PACK_PATH.to_string(),
            base_commit: None,
            create_announcement: None,
        },
        source_expectation: PublicationSourceExpectation::IfUnset,
        output: PublicationOutput::Snapshot {
            snapshot_id: snapshot_id.to_string(),
        },
    }
}

fn draft(root: &Path) -> ProjectTeamSetupDraft {
    let directory = root.join("scope");
    std::fs::create_dir_all(directory.join("draft/personas/roles")).expect("draft");
    ProjectTeamSetupDraft {
        setup_id: uuid::Uuid::new_v4().to_string(),
        project_ref: format!("30621:{OWNER}:garden"),
        project_directory: root.join("project").display().to_string(),
        draft_directory: directory.join("draft").display().to_string(),
        roles_directory: directory.join("draft/personas/roles").display().to_string(),
        status: super::super::SetupStatus::Draft,
        intent: "Grow food".to_string(),
        owner_pubkey: OWNER.to_string(),
        relay_url: "wss://example.test".to_string(),
        roles: vec!["lead".to_string()],
        expected_roles: Vec::new(),
        created_at: "2026-09-13T00:00:00Z".to_string(),
        latest_snapshot_id: None,
    }
}

#[test]
fn selected_snapshot_request_rejects_unpinned_or_nonportable_destination() {
    let snapshot = "a".repeat(64);
    assert!(validate_request(&request(&snapshot)).is_ok());

    let mut bad_snapshot = request("A");
    assert_eq!(
        validate_request(&bad_snapshot)
            .expect_err("bad snapshot")
            .code,
        "invalid_publication"
    );
    bad_snapshot = request(&snapshot);
    bad_snapshot.destination.pack_path = "../../roles".to_string();
    assert_eq!(
        validate_request(&bad_snapshot).expect_err("bad path").code,
        "invalid_publication"
    );
}

fn browser_request(source_expectation: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "destination": {
            "repoRef": format!("30617:{OWNER}:garden-packs"),
            "packPath": DEFAULT_PACK_PATH,
            "baseCommit": null
        },
        "sourceExpectation": source_expectation,
        "output": { "kind": "snapshot", "snapshotId": "a".repeat(64) }
    })
}

#[test]
fn publication_request_accepts_browser_camel_case_and_emits_it() {
    for source_expectation in [
        serde_json::json!({ "kind": "if_unset" }),
        serde_json::json!({ "kind": "expected", "eventId": "b".repeat(64) }),
    ] {
        let request: PublicationRequest =
            serde_json::from_value(browser_request(source_expectation)).expect("browser request");
        let emitted = serde_json::to_value(request).expect("emit request");
        assert!(emitted["output"].get("snapshotId").is_some());
        assert!(emitted["output"].get("snapshot_id").is_none());
        if emitted["sourceExpectation"]["kind"] == "expected" {
            assert!(emitted["sourceExpectation"].get("eventId").is_some());
            assert!(emitted["sourceExpectation"].get("event_id").is_none());
        }
    }
}

#[test]
fn publication_request_accepts_legacy_journal_fields_but_rejects_unknown_ones() {
    let mut legacy = browser_request(serde_json::json!({
        "kind": "expected",
        "event_id": "b".repeat(64)
    }));
    let output = legacy["output"].as_object_mut().expect("output object");
    let snapshot = output.remove("snapshotId").expect("snapshot id");
    output.insert("snapshot_id".to_string(), snapshot);
    let request: PublicationRequest = serde_json::from_value(legacy).expect("legacy journal");
    assert!(matches!(
        request.source_expectation,
        PublicationSourceExpectation::Expected { ref event_id } if event_id == &"b".repeat(64)
    ));

    let mut unknown = browser_request(serde_json::json!({ "kind": "if_unset" }));
    unknown["output"]["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<PublicationRequest>(unknown).is_err());
}

#[test]
fn candidate_uses_owned_snapshot_bytes_and_conditional_source_pins_its_commit() {
    let temp = tempfile::tempdir().expect("temp");
    let draft = draft(temp.path());
    let publication_id = uuid::Uuid::new_v4().to_string();
    let request = request(&"b".repeat(64));
    let keys = nostr::Keys::generate();
    let sha = create_candidate(
        &draft,
        &publication_id,
        &request,
        vec![(
            "lead/personas/lead.persona.md".to_string(),
            b"owned bytes".to_vec(),
        )],
        &keys,
    )
    .expect("candidate");
    assert_eq!(sha.len(), 40);
    assert_eq!(
        std::fs::read_to_string(
            candidate_dir(&draft, &publication_id)
                .expect("candidate path")
                .join("personas/roles/lead/personas/lead.persona.md"),
        )
        .expect("candidate content"),
        "owned bytes"
    );
    let journal = PublicationJournal {
        version: 1,
        setup_id: draft.setup_id.clone(),
        project_ref: draft.project_ref.clone(),
        owner_pubkey: draft.owner_pubkey.clone(),
        relay_url: draft.relay_url.clone(),
        publication_id: publication_id.clone(),
        request,
        candidate_ref: format!("refs/heads/setup/{publication_id}"),
        candidate_commit: Some(sha.clone()),
        source_event: None,
        installation: None,
        lead: None,
        status: PublicationStatus::Pushed,
        message: None,
    };
    let event = source_event(&journal, &keys).expect("source event");
    let decoded = beekeeper_core_pkg::project_pack_source::decode_project_pack_source(&event)
        .expect("decode");
    assert_eq!(decoded.pin().as_sha(), Some(sha.as_str()));
    assert!(matches!(
        decoded.expectation(),
        beekeeper_core_pkg::project_pack_source::PackSourceExpectation::Expected(None)
    ));
}

#[tokio::test]
async fn reconciled_read_holds_the_journal_lock_through_live_source_observation() {
    let temp = tempfile::tempdir().expect("temp");
    let draft = draft(temp.path());
    let owner = nostr::Keys::generate();
    let mut journal = installed_journal(&draft, &"c".repeat(40), &[], None);
    journal.source_event = Some(source_event(&journal, &owner).expect("source event"));
    save_journal(&draft, &journal).expect("journal");

    let reader_draft = draft.clone();
    let (source_started, source_started_rx) = tokio::sync::oneshot::channel();
    let source_release = std::sync::Arc::new(tokio::sync::Notify::new());
    let reader_release = source_release.clone();
    let reader = tokio::spawn(async move {
        reconciled_publication_read(
            &reader_draft,
            async move {
                let _ = source_started.send(());
                reader_release.notified().await;
                Ok(None)
            },
            || Ok(()),
        )
        .await
        .expect("reconciled read")
    });
    source_started_rx.await.expect("source observation started");

    let writer_draft = draft.clone();
    let (writer_started, writer_started_rx) = tokio::sync::oneshot::channel();
    let (writer_finished, mut writer_finished_rx) = tokio::sync::oneshot::channel();
    let writer = tokio::spawn(async move {
        let _ = writer_started.send(());
        let _writer_guard = PUBLICATION_LOCK.lock().await;
        let mut latest = load_journal(&writer_draft)
            .expect("load latest")
            .expect("journal");
        latest.lead = Some(ready_lead(
            &uuid::Uuid::new_v4().to_string(),
            &"d".repeat(64),
        ));
        save_journal(&writer_draft, &latest).expect("save activation");
        let _ = writer_finished.send(());
    });
    writer_started_rx.await.expect("writer started");
    tokio::task::yield_now().await;
    assert!(
        writer_finished_rx.try_recv().is_err(),
        "activation writer must wait for the reconciling read"
    );

    source_release.notify_one();
    let (source, reconciled) = reader.await.expect("reader joined");
    assert!(source.is_none());
    assert_eq!(
        reconciled.expect("journal").status,
        PublicationStatus::SourceUnknown
    );
    writer.await.expect("writer joined");

    let final_journal = load_journal(&draft).expect("load final").expect("journal");
    assert_eq!(final_journal.status, PublicationStatus::SourceUnknown);
    assert!(
        final_journal.lead.is_some(),
        "later writer must not be lost"
    );
}

#[tokio::test]
async fn reconciled_read_context_refusal_leaves_the_journal_unchanged() {
    let temp = tempfile::tempdir().expect("temp");
    let draft = draft(temp.path());
    let owner = nostr::Keys::generate();
    let mut journal = installed_journal(&draft, &"c".repeat(40), &[], None);
    journal.source_event = Some(source_event(&journal, &owner).expect("source event"));
    save_journal(&draft, &journal).expect("journal");
    let path = journal_path(&draft).expect("journal path");
    let before = std::fs::read(&path).expect("journal bytes");

    let error = match reconciled_publication_read(&draft, async { Ok(None) }, || {
        Err(SetupError::new("scope_changed", "context changed"))
    })
    .await
    {
        Err(error) => error,
        Ok(_) => panic!("context refusal"),
    };

    assert_eq!(error.code, "scope_changed");
    assert_eq!(std::fs::read(path).expect("journal bytes"), before);
}

/// An adopted journal whose installation recorded `roles`, bound to `draft`.
pub(super) fn installed_journal(
    draft: &ProjectTeamSetupDraft,
    commit: &str,
    roles: &[(&str, &str)],
    lead: Option<LeadJournal>,
) -> PublicationJournal {
    let owner = &draft.owner_pubkey;
    let publication_id = uuid::Uuid::new_v4().to_string();
    let mut request = request(&"e".repeat(64));
    request.destination.repo_ref = format!("30617:{owner}:garden-packs");
    PublicationJournal {
        version: 1,
        setup_id: draft.setup_id.clone(),
        project_ref: draft.project_ref.clone(),
        owner_pubkey: owner.clone(),
        relay_url: draft.relay_url.clone(),
        candidate_ref: format!("refs/heads/setup/{publication_id}"),
        publication_id,
        request,
        candidate_commit: Some(commit.to_string()),
        source_event: None,
        installation: Some(InstallationJournal {
            team_id: uuid::Uuid::new_v4().to_string(),
            planned_roles: Vec::new(),
            channel: None,
            roles: roles
                .iter()
                .map(
                    |(role, agent_pubkey)| activation::ProjectTeamInstalledRole {
                        role: role.to_string(),
                        agent_pubkey: agent_pubkey.to_string(),
                        pack_ref: packs_cache::PackRef {
                            repo: format!("30617:{owner}:garden-packs"),
                            sha: commit.to_string(),
                            role: role.to_string(),
                            path: format!("{DEFAULT_PACK_PATH}/{role}"),
                        },
                    },
                )
                .collect(),
        }),
        lead,
        status: PublicationStatus::Adopted,
        message: None,
    }
}

pub(super) fn ready_lead(channel_id: &str, lead_pubkey: &str) -> LeadJournal {
    LeadJournal {
        channel_id: channel_id.to_string(),
        lead_pubkey: lead_pubkey.to_string(),
        session_ref: None,
        create_command_id: None,
        provider_pubkey: None,
        provider_instance_ref: None,
        runtime: None,
        driver: None,
        provider_host_instance_id: None,
        model: None,
        genesis_event: None,
        create_event: None,
        status: ProjectTeamLeadStatus::Ready,
        message: Some("saved".to_string()),
    }
}

#[test]
fn activation_keeps_one_recorded_channel_and_lead_identity_across_reopen() {
    let temp = tempfile::tempdir().expect("temp");
    let draft = draft(temp.path());
    let commit = "c".repeat(40);
    let lead_pubkey = "d".repeat(64);
    let channel_id = uuid::Uuid::new_v4().to_string();
    let journal = installed_journal(
        &draft,
        &commit,
        &[("lead", &lead_pubkey)],
        Some(ready_lead(&channel_id, &lead_pubkey)),
    );

    let activation = activation::activation(&journal);
    assert_eq!(activation.source.expect("source").commit, commit);
    assert_eq!(
        activation.installation.installed_roles[0].agent_pubkey,
        lead_pubkey
    );
    assert_eq!(activation.lead.status, ProjectTeamLeadStatus::Ready);
    assert_eq!(
        activation.lead.channel_id.as_deref(),
        Some(channel_id.as_str())
    );
    assert_eq!(activation.lead.session_ref, None);
    assert_eq!(
        activation.lead.lead_pubkey.as_deref(),
        Some(lead_pubkey.as_str())
    );
}

#[test]
fn activation_projects_lead_pubkey_from_lead_journal_then_installation_else_null() {
    let temp = tempfile::tempdir().expect("temp");
    let draft = draft(temp.path());
    let commit = "c".repeat(40);
    let installed_lead = "d".repeat(64);
    let worker = "f".repeat(64);
    let roles = [
        ("builder", worker.as_str()),
        ("lead", installed_lead.as_str()),
    ];

    let mut journal = installed_journal(&draft, &commit, &roles, None);
    let projected = activation::activation(&journal);
    assert_eq!(projected.lead.status, ProjectTeamLeadStatus::NeedsChannel);
    assert_eq!(
        projected.lead.lead_pubkey.as_deref(),
        Some(installed_lead.as_str())
    );
    let json = serde_json::to_value(&projected.lead).expect("lead json");
    assert_eq!(json["leadPubkey"], serde_json::json!(installed_lead));

    // The reserved lead journal's identity is the one the lead request uses.
    let reserved = "a".repeat(64);
    journal.lead = Some(ready_lead(&uuid::Uuid::new_v4().to_string(), &reserved));
    assert_eq!(
        activation::activation(&journal).lead.lead_pubkey.as_deref(),
        Some(reserved.as_str())
    );

    let not_installed = installed_journal(&draft, &commit, &[], None);
    let projected = activation::activation(&not_installed);
    assert_eq!(projected.lead.lead_pubkey, None);
    assert_eq!(
        serde_json::to_value(&projected.lead).expect("lead json")["leadPubkey"],
        serde_json::Value::Null
    );
}
