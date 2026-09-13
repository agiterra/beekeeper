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
    let decoded =
        buzz_core_pkg::project_pack_source::decode_project_pack_source(&event).expect("decode");
    assert_eq!(decoded.pin().as_sha(), Some(sha.as_str()));
    assert!(matches!(
        decoded.expectation(),
        buzz_core_pkg::project_pack_source::PackSourceExpectation::Expected(None)
    ));
}

#[test]
fn activation_keeps_one_recorded_channel_and_lead_identity_across_reopen() {
    let temp = tempfile::tempdir().expect("temp");
    let draft = draft(temp.path());
    let commit = "c".repeat(40);
    let lead_pubkey = "d".repeat(64);
    let channel_id = uuid::Uuid::new_v4().to_string();
    let journal = PublicationJournal {
        version: 1,
        setup_id: draft.setup_id.clone(),
        project_ref: draft.project_ref.clone(),
        owner_pubkey: draft.owner_pubkey.clone(),
        relay_url: draft.relay_url.clone(),
        publication_id: uuid::Uuid::new_v4().to_string(),
        request: request(&"e".repeat(64)),
        candidate_ref: "refs/heads/setup/test".to_string(),
        candidate_commit: Some(commit.clone()),
        source_event: None,
        installation: Some(InstallationJournal {
            team_id: uuid::Uuid::new_v4().to_string(),
            planned_roles: Vec::new(),
            channel: None,
            roles: vec![activation::ProjectTeamInstalledRole {
                role: "lead".to_string(),
                agent_pubkey: lead_pubkey.clone(),
                pack_ref: packs_cache::PackRef {
                    repo: format!("30617:{OWNER}:garden-packs"),
                    sha: commit.clone(),
                    role: "lead".to_string(),
                    path: format!("{DEFAULT_PACK_PATH}/lead"),
                },
            }],
        }),
        lead: Some(LeadJournal {
            channel_id: channel_id.clone(),
            lead_pubkey: lead_pubkey.clone(),
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
        }),
        status: PublicationStatus::Adopted,
        message: None,
    };

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
}
