use super::*;

const OWNER: &str = "6cbdf4451d3989c10c20d13240c665a9e11e3959a95488382193481692b68df2";

fn draft(root: &Path, project_ref: &str) -> ProjectTeamSetupDraft {
    let directory = root.join("scope");
    std::fs::create_dir_all(directory.join("draft/personas/roles")).expect("draft");
    ProjectTeamSetupDraft {
        setup_id: uuid::Uuid::new_v4().to_string(),
        project_ref: project_ref.to_string(),
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
fn absent_source_destination_is_project_qualified_not_slug_only() {
    let temp = tempfile::tempdir().expect("temp");
    let alpha = draft(temp.path(), &format!("30621:{OWNER}:tankloop"));
    let other_owner = "7cbdf4451d3989c10c20d13240c665a9e11e3959a95488382193481692b68df2";
    let beta = draft(temp.path(), &format!("30621:{other_owner}:tankloop"));
    let alpha_destination = host_destination(&alpha, None).expect("alpha destination");
    let beta_destination = host_destination(&beta, None).expect("beta destination");
    assert_ne!(alpha_destination.repo_ref, beta_destination.repo_ref);
    assert_eq!(alpha_destination.pack_path, DEFAULT_PACK_PATH);
    assert!(alpha_destination.create_announcement.is_some());
    assert!(alpha_destination
        .repo_ref
        .ends_with(&hex::encode(Sha256::digest(alpha.project_ref.as_bytes()))[..12]));
}

#[test]
fn candidate_blobs_ignore_gitattributes_and_recovery_reverifies_them() {
    let temp = tempfile::tempdir().expect("temp");
    let draft = draft(temp.path(), &format!("30621:{OWNER}:tankloop"));
    let destination = host_destination(&draft, None).expect("destination");
    let publication_id = uuid::Uuid::new_v4().to_string();
    let persona = b"line one\nline two\n".to_vec();
    let files = vec![
        (
            ".gitattributes".to_string(),
            b"*.md text eol=crlf\n".to_vec(),
        ),
        ("lead/personas/lead.persona.md".to_string(), persona.clone()),
    ];
    let keys = nostr::Keys::generate();
    let commit = git::create_candidate(&draft, &publication_id, &destination, &files, &keys)
        .expect("candidate");
    let checkout = candidate_dir(&draft, &publication_id).expect("candidate dir");
    let auth =
        crate::commands::project_git_exec::build_git_auth_config_for_keys(&keys).expect("git auth");
    let blob = crate::commands::project_git_exec::run_git_bytes(
        &[
            "show",
            &format!("{commit}:personas/roles/lead/personas/lead.persona.md"),
        ],
        Some(&checkout),
        &auth,
        &[],
    )
    .expect("committed blob");
    assert_eq!(blob, persona);
    assert_eq!(
        git::recover_candidate(&draft, &publication_id, &destination, &files, &keys)
            .expect("recovery")
            .as_deref(),
        Some(commit.as_str())
    );
}

#[test]
fn an_existing_journal_is_returned_before_a_fresh_source_observation() {
    let temp = tempfile::tempdir().expect("temp");
    let draft = draft(temp.path(), &format!("30621:{OWNER}:tankloop"));
    let request = PublicationRequest {
        destination: host_destination(&draft, None).expect("destination"),
        source_expectation: PublicationSourceExpectation::IfUnset,
        output: PublicationOutput::Snapshot {
            snapshot_id: "a".repeat(64),
        },
    };
    let journal = PublicationJournal {
        version: 1,
        setup_id: draft.setup_id.clone(),
        project_ref: draft.project_ref.clone(),
        owner_pubkey: draft.owner_pubkey.clone(),
        relay_url: draft.relay_url.clone(),
        publication_id: uuid::Uuid::new_v4().to_string(),
        request,
        candidate_ref: String::new(),
        candidate_commit: None,
        source_event: None,
        installation: None,
        lead: None,
        status: PublicationStatus::Adopted,
        message: None,
    };
    let mut journal = journal;
    journal.candidate_ref = format!("refs/heads/setup/{}", journal.publication_id);
    save_journal(&draft, &journal).expect("journal");
    assert_eq!(
        load_journal(&draft)
            .expect("load")
            .expect("saved")
            .publication_id,
        journal.publication_id
    );
}
