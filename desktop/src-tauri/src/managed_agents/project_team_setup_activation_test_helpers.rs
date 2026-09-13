use super::*;
use crate::managed_agents::project_team_setup::SetupStatus;

#[test]
fn encrypted_role_plan_recovers_the_same_identity_after_the_persona_write_boundary() {
    let owner = nostr::Keys::generate();
    let planned = reserve_role_identity(&owner, "lead").expect("reserve lead");
    let journal_bytes = serde_json::to_vec(&planned).expect("journal bytes");
    let recovered = recover_role_identity(&owner, &planned).expect("recover lead");

    assert_eq!(recovered.pubkey, planned.pubkey);
    assert!(!journal_bytes
        .windows(recovered.private_key_nsec.len())
        .any(|window| window == recovered.private_key_nsec.as_bytes()));
    // Simulate the first of the three independent store writes succeeding: the
    // durable plan still has the same key when the agent/team writes have not.
    let partial = InstallationJournal {
        team_id: uuid::Uuid::new_v4().to_string(),
        planned_roles: vec![planned.clone()],
        channel: None,
        roles: Vec::new(),
    };
    let reopened: InstallationJournal =
        serde_json::from_slice(&serde_json::to_vec(&partial).expect("durable partial install"))
            .expect("reopen partial install");
    assert_eq!(
        recover_role_identity(&owner, &reopened.planned_roles[0])
            .expect("reopen recovery")
            .pubkey,
        recovered.pubkey
    );
}

#[test]
fn saved_channel_request_reopens_with_the_same_uuid_and_signed_bytes() {
    let owner = nostr::Keys::generate();
    let channel_id = uuid::Uuid::new_v4();
    let draft = ProjectTeamSetupDraft {
        setup_id: uuid::Uuid::new_v4().to_string(),
        project_ref: format!("30621:{}:garden", owner.public_key().to_hex()),
        project_directory: "/tmp/project".to_string(),
        draft_directory: "/tmp/draft".to_string(),
        roles_directory: "/tmp/draft/personas/roles".to_string(),
        status: SetupStatus::Draft,
        intent: "grow".to_string(),
        owner_pubkey: owner.public_key().to_hex(),
        relay_url: "wss://example.test".to_string(),
        roles: vec!["lead".to_string()],
        expected_roles: Vec::new(),
        created_at: "2026-09-13T00:00:00Z".to_string(),
        latest_snapshot_id: None,
    };
    let saved = ActivationChannelJournal {
        channel_id: channel_id.to_string(),
        create_event: channel::channel_event(&draft, channel_id, &owner).expect("sign create"),
    };
    let reopened: ActivationChannelJournal =
        serde_json::from_slice(&serde_json::to_vec(&saved).expect("persist reservation"))
            .expect("reopen reservation");
    assert_eq!(reopened.channel_id, saved.channel_id);
    assert_eq!(reopened.create_event.id, saved.create_event.id);
    assert_eq!(
        serde_json::to_value(&reopened.create_event).expect("reopened bytes"),
        serde_json::to_value(&saved.create_event).expect("saved bytes"),
    );
}

#[test]
fn lead_staging_requires_the_exact_adopted_repo_sha_role_and_path() {
    let source = ProjectTeamActivationSource {
        repo_ref: "30617:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa:packs"
            .to_string(),
        commit: "b".repeat(40),
        pack_path: "personas/roles".to_string(),
    };
    let input = exact_project_pack_source(&source);
    assert_eq!(input.repo, source.repo_ref);
    assert_eq!(input.sha.as_deref(), Some(source.commit.as_str()));
    assert_eq!(input.git_ref, None);
    assert_eq!(input.path.as_deref(), Some("personas/roles"));

    let expected = expected_pack_ref(&source, "lead");
    let staged = crate::managed_agents::actor_seats::StagedActorSeat {
        pack_staged: true,
        pack_ref: Some(expected.clone()),
    };
    require_exact_staged_pack(&staged, &expected).expect("exact pack");

    let wrong = crate::managed_agents::actor_seats::StagedActorSeat {
        pack_staged: true,
        pack_ref: Some(packs_cache::PackRef {
            sha: "c".repeat(40),
            ..expected
        }),
    };
    assert!(require_exact_staged_pack(&wrong, &expected_pack_ref(&source, "lead")).is_err());
}
