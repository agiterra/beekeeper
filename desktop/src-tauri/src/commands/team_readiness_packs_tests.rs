//! Ledger 135(c): readiness answers from the rung a hire actually stages from.
//!
//! Split from `team_readiness_tests.rs`, which sits at the repository's
//! 1,000-line ceiling.

use super::super::registry::tests::checkout_only_store;
use super::super::tests::CountingHost;
use super::super::*;
use super::*;
use crate::coding_sessions::workdir_store::CodingSessionWorkdirStore;
use crate::managed_agents::crew_roles::DiscoveredRolePack;

const PROJECT_REF: &str =
    "30621:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa:beekeeper";

// ── ledger 135(c): readiness reads the truth the hire uses ──────────────────

/// Facts for a launch that names `roles`, with nothing gathered yet.
fn gathered_for(roles: &[&str]) -> Gathered {
    Gathered {
        team: TeamReadinessTeam {
            selected_roles: roles.iter().map(|role| (*role).to_string()).collect(),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn codes(gathered: &Gathered) -> Vec<&str> {
    gathered
        .facts
        .iter()
        .map(|fact| fact.code.as_str())
        .collect()
}

#[test]
fn a_project_with_a_packs_repository_is_satisfied_by_it_not_by_the_checkout() {
    // Ledger 135(c): with "Use roles" ticked the founding form blocked on
    // ROLE_PACKS_MISSING inside a checkout with no personas/roles, while the
    // seated create staged every pack from the relay's kind:30624 regardless.
    let checkout = tempfile::tempdir().expect("tempdir");
    let mut gathered = gathered_for(&["builder", "verifier"]);
    collect_team(
        &CountingHost::default(),
        Some(checkout.path()),
        &ProjectPackSourceProbe::Available {
            repo: "30617:aa:agiterra-packs".into(),
            sha: Some("fb27ccf1234567890abcdef1234567890abcdef1".into()),
            roles: vec!["builder".into(), "lead".into(), "verifier".into()],
        },
        &mut gathered,
    );
    let fact = gathered
        .facts
        .iter()
        .find(|fact| fact.code == "ROLE_PACKS_FROM_PROJECT_SOURCE")
        .expect("the source that actually stages the packs is reported");
    assert_eq!(fact.state, TeamReadinessFactState::Ready);
    assert!(fact.summary.contains("30617:aa:agiterra-packs"));
    assert!(fact.summary.contains("fb27ccf1"), "{}", fact.summary);
    assert!(fact.summary.contains("builder, lead, verifier"));
    assert!(
        !codes(&gathered).contains(&"ROLE_PACKS_MISSING"),
        "the in-checkout layout is a fallback, not a requirement: {:?}",
        codes(&gathered)
    );
    assert_eq!(
        gathered.team.packs_revision.as_deref(),
        Some("fb27ccf1234567890abcdef1234567890abcdef1")
    );
}

#[test]
fn a_role_the_packs_repository_does_not_carry_still_blocks_the_launch() {
    let mut gathered = gathered_for(&["archivist"]);
    collect_team(
        &CountingHost::default(),
        None,
        &ProjectPackSourceProbe::Available {
            repo: "30617:aa:agiterra-packs".into(),
            sha: None,
            roles: vec!["builder".into()],
        },
        &mut gathered,
    );
    let fact = gathered
        .facts
        .iter()
        .find(|fact| fact.code == "SELECTED_ROLE_UNAVAILABLE")
        .expect("a role nothing can stage is still a blocker");
    assert_eq!(fact.state, TeamReadinessFactState::Blocked);
    assert!(fact.summary.contains("archivist"));
    assert!(fact.summary.contains("30617:aa:agiterra-packs"));
}

#[test]
fn a_packs_repository_this_computer_cannot_read_carries_the_hire_s_own_reason() {
    let mut gathered = gathered_for(&["builder"]);
    collect_team(
        &CountingHost::default(),
        None,
        &ProjectPackSourceProbe::Unavailable {
            reason: "could not fetch 30617:aa:agiterra-packs".into(),
        },
        &mut gathered,
    );
    let fact = gathered
        .facts
        .iter()
        .find(|fact| fact.code == "ROLE_PACKS_SOURCE_UNAVAILABLE")
        .expect("the refusal a hire would get is the fact readiness reports");
    assert_eq!(fact.state, TeamReadinessFactState::Blocked);
    assert!(fact.summary.contains("could not fetch"));

    // No seat named, no launch to block — a limit on what can be said.
    let mut unselected = gathered_for(&[]);
    collect_team(
        &CountingHost::default(),
        None,
        &ProjectPackSourceProbe::Unavailable {
            reason: "could not fetch".into(),
        },
        &mut unselected,
    );
    assert_eq!(
        unselected
            .facts
            .iter()
            .find(|fact| fact.code == "ROLE_PACKS_SOURCE_UNAVAILABLE")
            .map(|fact| fact.state),
        Some(TeamReadinessFactState::Limited)
    );
}

#[test]
fn a_project_with_no_packs_repository_still_falls_back_to_the_checkout() {
    let checkout = tempfile::tempdir().expect("tempdir");
    let mut gathered = gathered_for(&["builder"]);
    collect_team(
        &CountingHost::default(),
        Some(checkout.path()),
        &ProjectPackSourceProbe::Absent,
        &mut gathered,
    );
    let fact = gathered
        .facts
        .iter()
        .find(|fact| fact.code == "ROLE_PACKS_MISSING")
        .expect("with no source, the checkout folder is the answer and its absence blocks");
    assert_eq!(fact.state, TeamReadinessFactState::Blocked);
    assert!(
        fact.summary.contains("names no packs repository"),
        "the copy says which of the two is missing: {}",
        fact.summary
    );
}

#[test]
fn the_model_registry_is_a_limit_for_a_project_whose_packs_come_from_a_repository() {
    let checkout = tempfile::tempdir().expect("tempdir");
    let store = checkout_only_store(PROJECT_REF, checkout.path());
    let mut with_source = Gathered::default();
    collect_runtimes_and_registry(
        &CountingHost::default(),
        Some(&store),
        PROJECT_REF,
        true,
        &mut with_source,
    );
    let fact = with_source
        .facts
        .iter()
        .find(|fact| fact.code == "REGISTRY_UNREADABLE")
        .expect("the absent registry is still disclosed");
    assert_eq!(fact.state, TeamReadinessFactState::Limited);
    assert!(
        fact.summary.contains("each role pack"),
        "the remedy says what the registry is for: {}",
        fact.summary
    );

    let mut without_source = Gathered::default();
    collect_runtimes_and_registry(
        &CountingHost::default(),
        Some(&store),
        PROJECT_REF,
        false,
        &mut without_source,
    );
    assert_eq!(
        without_source
            .facts
            .iter()
            .find(|fact| fact.code == "REGISTRY_UNREADABLE")
            .map(|fact| fact.state),
        Some(TeamReadinessFactState::Blocked)
    );
}

#[test]
fn the_unrecorded_checkout_blocker_says_what_to_click() {
    let mut gathered = Gathered::default();
    let store = CodingSessionWorkdirStore::default();
    let checkout = collect_checkout(Ok(&store), PROJECT_REF, &mut gathered);
    assert!(checkout.is_none());
    let fact = gathered
        .facts
        .iter()
        .find(|fact| fact.code == "CHECKOUT_NOT_RECORDED")
        .expect("a cold host blocks on its missing checkout");
    assert!(fact.summary.contains("repository folder"));
    let remedy = fact.remedy.as_deref().unwrap_or_default();
    assert!(
        remedy.contains("founding form") && remedy.contains("Project settings"),
        "both ways to fix it are named: {remedy}"
    );
}

#[test]
fn selected_role_pack_state_distinguishes_dirty_and_wrong_project() {
    let pack = DiscoveredRolePack {
        dir: PathBuf::from("/project/personas/roles/builder"),
        persona_name: "builder".into(),
        display_name: "Builder".into(),
        role: "builder".into(),
        system_prompt: "Build carefully".into(),
        runtime: Some("codex".into()),
        model: None,
        provider: None,
        avatar_url: None,
    };
    let mut row = ManagedAgentReadinessMetadata {
        pubkey: "a".repeat(64),
        name: "Bob".into(),
        home_role: Some("builder".into()),
        persona_team_dir: Some(pack.dir.clone()),
        persona_name_in_team: Some(pack.persona_name.clone()),
        persona_source_version: None,
        auth_tag_present: true,
        auth_tag_owner: None,
        auth_tag_invalid: false,
        auth_tag_owner_mismatch: false,
    };
    assert_eq!(
        selected_role_pack_state(&[row.clone()], &pack),
        SelectedRolePackState::SourceUnknown
    );
    row.persona_source_version = Some("b".repeat(64));
    assert_eq!(
        selected_role_pack_state(&[row.clone()], &pack),
        SelectedRolePackState::Dirty
    );
    row.persona_source_version = Some(role_pack_source_version(&pack, &row.name));
    assert_eq!(
        selected_role_pack_state(&[row.clone()], &pack),
        SelectedRolePackState::Current
    );
    row.persona_team_dir = Some(PathBuf::from("/other/personas/roles/builder"));
    assert_eq!(
        selected_role_pack_state(&[row], &pack),
        SelectedRolePackState::WrongProject
    );
}
