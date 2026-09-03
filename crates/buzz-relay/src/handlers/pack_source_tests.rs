//! The kind:30624 write gate — the rule, and the production path that runs it.

use nostr::{EventBuilder, Keys, Kind, Tag};

use buzz_core::project_pack_source::{build_project_pack_source, PackPin};
use buzz_core::repository_founders::REPOSITORY_MAINTAINERS_TAG;

use super::*;

fn project_coordinate(owner: &nostr::PublicKey) -> String {
    format!("30621:{}:agiterra", owner.to_hex())
}

fn pack_source_event(keys: &Keys, project: &str, repo: &str) -> nostr::Event {
    let draft = build_project_pack_source(project, repo, &PackPin::Sha("a".repeat(40)), None, None)
        .expect("a valid draft");
    let tags: Vec<Tag> = draft
        .tags
        .iter()
        .map(|tag| Tag::parse(tag.clone()).expect("tag parses"))
        .collect();
    EventBuilder::new(
        Kind::Custom(buzz_core::kind::KIND_PROJECT_PACK_SOURCE as u16),
        draft.content,
    )
    .tags(tags)
    .sign_with_keys(keys)
    .expect("signs")
}

fn repo_announcement(
    keys: &Keys,
    repo_id: &str,
    project: Option<&str>,
    maintainers: &[&str],
) -> nostr::Event {
    let mut tags = vec![Tag::parse(["d", repo_id]).expect("d")];
    if let Some(project) = project {
        tags.push(Tag::parse(["project", project]).expect("project"));
    }
    if !maintainers.is_empty() {
        let mut values = vec![REPOSITORY_MAINTAINERS_TAG.to_string()];
        values.extend(maintainers.iter().map(|value| (*value).to_string()));
        tags.push(Tag::parse(values).expect("maintainers"));
    }
    EventBuilder::new(Kind::Custom(KIND_GIT_REPO_ANNOUNCEMENT as u16), "")
        .tags(tags)
        .sign_with_keys(keys)
        .expect("signs")
}

/// The creator of the project is always admitted, roster or no roster.
#[test]
fn the_project_creator_may_publish_a_pack_source() {
    let creator = Keys::generate();
    let coordinate = project_coordinate(&creator.public_key());
    let admitted = decide_pack_source_admission(
        &creator.public_key().to_hex(),
        &coordinate,
        &creator.public_key().to_hex(),
        &[],
        &[],
        false,
    )
    .expect("admitted");
    assert_eq!(admitted, PackSourceAdmission::ProjectCreator);
}

/// A roster Owner may, a collaborator and a viewer may not.
#[test]
fn only_a_roster_owner_is_admitted_from_the_roster() {
    let creator = Keys::generate();
    let owner = Keys::generate();
    let collaborator = Keys::generate();
    let viewer = Keys::generate();
    let coordinate = project_coordinate(&creator.public_key());
    let roster = vec![
        (owner.public_key().to_hex(), ProjectRole::Owner),
        (
            collaborator.public_key().to_hex(),
            ProjectRole::Collaborator,
        ),
        (viewer.public_key().to_hex(), ProjectRole::Viewer),
    ];

    assert_eq!(
        decide_pack_source_admission(
            &owner.public_key().to_hex(),
            &coordinate,
            &creator.public_key().to_hex(),
            &roster,
            &[],
            true,
        )
        .expect("owner admitted"),
        PackSourceAdmission::ProjectOwner
    );

    for stranger in [&collaborator, &viewer] {
        let refusal = decide_pack_source_admission(
            &stranger.public_key().to_hex(),
            &coordinate,
            &creator.public_key().to_hex(),
            &roster,
            &[],
            true,
        )
        .expect_err("refused");
        assert!(refusal.sentence().contains("Owner of that project"));
        assert!(refusal.roster_read);
    }
}

/// Finding 33's clause: a co-founder of one of the project's repositories may
/// set the pack source even though they neither created nor own the project.
#[test]
fn a_co_founder_of_a_project_repository_is_admitted() {
    let creator = Keys::generate();
    let signer = Keys::generate();
    let co_founder = Keys::generate();
    let stranger = Keys::generate();
    let coordinate = project_coordinate(&creator.public_key());
    let announcement = repo_announcement(
        &signer,
        "agiterra-beekeeper",
        Some(&coordinate),
        &[&co_founder.public_key().to_hex()],
    );

    for founder in [&signer, &co_founder] {
        let admitted = decide_pack_source_admission(
            &founder.public_key().to_hex(),
            &coordinate,
            &creator.public_key().to_hex(),
            &[],
            std::slice::from_ref(&announcement),
            true,
        )
        .expect("a founder is admitted");
        assert_eq!(
            admitted,
            PackSourceAdmission::RepositoryFounder {
                repo_coordinate: format!(
                    "30617:{}:agiterra-beekeeper",
                    signer.public_key().to_hex()
                ),
            }
        );
    }

    let refusal = decide_pack_source_admission(
        &stranger.public_key().to_hex(),
        &coordinate,
        &creator.public_key().to_hex(),
        &[],
        std::slice::from_ref(&announcement),
        true,
    )
    .expect_err("a stranger is refused");
    assert_eq!(refusal.repositories_searched, 1);
    assert!(
        refusal.sentence().contains("1 repositor"),
        "the refusal must say how much was searched: {}",
        refusal.sentence()
    );
}

/// A repository that belongs to *another* project admits nobody here. The
/// filter is the caller's, so this pins that the decision never widens it.
#[test]
fn a_foreign_repository_is_never_offered_to_this_decision() {
    let creator = Keys::generate();
    let signer = Keys::generate();
    let coordinate = project_coordinate(&creator.public_key());
    // The caller filters by `project` back-reference; here the decision is
    // handed an empty list, which is what that filter produces.
    let refusal = decide_pack_source_admission(
        &signer.public_key().to_hex(),
        &coordinate,
        &creator.public_key().to_hex(),
        &[],
        &[],
        true,
    )
    .expect_err("refused");
    assert_eq!(refusal.repositories_searched, 0);
}

/// A refusal for a project nothing stored says so rather than implying a
/// roster was consulted.
#[test]
fn an_unknown_project_says_no_roster_was_read() {
    let creator = Keys::generate();
    let stranger = Keys::generate();
    let coordinate = project_coordinate(&creator.public_key());
    let refusal = decide_pack_source_admission(
        &stranger.public_key().to_hex(),
        &coordinate,
        &creator.public_key().to_hex(),
        &[],
        &[],
        false,
    )
    .expect_err("refused");
    assert!(
        refusal.sentence().contains("no project by that coordinate"),
        "{}",
        refusal.sentence()
    );
}

/// The production path, end to end: a stranger's 30624 is refused by
/// `ingest_event` itself, and the project's creator's is stored.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn ingest_refuses_a_pack_source_from_a_stranger_and_stores_the_creators() {
    use crate::handlers::ingest::{ingest_event, IngestAuth, IngestError};

    let state = crate::api::git::policy::tests::policy_test_state().await;
    let host = format!("packs-{}.example", uuid::Uuid::new_v4().simple());
    let community = state
        .db
        .ensure_configured_community(&host)
        .await
        .expect("community")
        .id;
    let tenant = buzz_core::tenant::TenantContext::resolved(community, host.clone());

    let creator = Keys::generate();
    let stranger = Keys::generate();
    let coordinate = project_coordinate(&creator.public_key());
    let repo = format!("30617:{}:agiterra-packs", creator.public_key().to_hex());

    let auth = |keys: &Keys| IngestAuth::Http {
        pubkey: keys.public_key(),
        scopes: vec![
            buzz_auth::Scope::ReposWrite,
            buzz_auth::Scope::MessagesWrite,
            buzz_auth::Scope::ChannelsWrite,
        ],
        auth_method: crate::handlers::ingest::HttpAuthMethod::Nip98,
    };

    // The project head, so the coordinate resolves to something stored.
    let project_head = EventBuilder::new(
        Kind::Custom(buzz_core::kind::KIND_PROJECT as u16),
        r#"{"name":"agiterra"}"#,
    )
    .tags(vec![Tag::parse(["d", "agiterra"]).expect("d")])
    .sign_with_keys(&creator)
    .expect("signs");
    ingest_event(&state, &tenant, project_head, auth(&creator))
        .await
        .expect("the project head stores");

    let refused = ingest_event(
        &state,
        &tenant,
        pack_source_event(&stranger, &coordinate, &repo),
        auth(&stranger),
    )
    .await;
    match refused {
        Err(IngestError::AuthFailed(message)) => {
            assert!(
                message.contains("pack source") || message.contains("Owner of that project"),
                "the refusal must say why: {message}"
            );
        }
        Err(other) => {
            panic!("a stranger's pack source must be refused for the right reason, got {other:?}")
        }
        Ok(result) => panic!(
            "a stranger's pack source must be refused, it was accepted={}",
            result.accepted
        ),
    }

    let accepted = ingest_event(
        &state,
        &tenant,
        pack_source_event(&creator, &coordinate, &repo),
        auth(&creator),
    )
    .await
    .expect("the creator's pack source stores");
    assert!(accepted.accepted, "the creator's record must be accepted");
}
