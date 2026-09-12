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

fn project_head(creator: &Keys, repositories: &[String]) -> nostr::Event {
    let mut tags = vec![Tag::parse(["d", "agiterra"]).expect("d")];
    for coordinate in repositories {
        tags.push(Tag::parse(["a", coordinate]).expect("a"));
    }
    EventBuilder::new(Kind::Custom(KIND_PROJECT as u16), r#"{"name":"agiterra"}"#)
        .tags(tags)
        .sign_with_keys(creator)
        .expect("signed project")
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
        None,
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
            None,
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
            None,
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

    let head = project_head(
        &creator,
        &[repository_coordinate(&announcement).expect("coordinate")],
    );
    for founder in [&signer, &co_founder] {
        let admitted = decide_pack_source_admission(
            &founder.public_key().to_hex(),
            &coordinate,
            &creator.public_key().to_hex(),
            &[],
            std::slice::from_ref(&announcement),
            true,
            Some(&head),
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
        Some(&head),
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
        None,
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
        None,
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

#[test]
fn public_project_self_backlink_cannot_create_pack_source_authority() {
    let creator = Keys::generate();
    let outsider = Keys::generate();
    let coordinate = project_coordinate(&creator.public_key());
    let head = project_head(&creator, &[]);
    let self_link = repo_announcement(&outsider, "self-endorsed", Some(&coordinate), &[]);
    assert!(decide_pack_source_admission(
        &outsider.public_key().to_hex(),
        &coordinate,
        &creator.public_key().to_hex(),
        &[],
        &[self_link],
        true,
        Some(&head),
    )
    .is_err());
}

#[test]
fn private_project_viewers_read_access_and_self_backlink_do_not_grant_source_writes() {
    let creator = Keys::generate();
    let viewer = Keys::generate();
    let coordinate = project_coordinate(&creator.public_key());
    let gate = buzz_db::project_acl::ProjectGate {
        owner: creator.public_key().to_bytes().to_vec(),
        members: vec![(viewer.public_key().to_bytes().to_vec(), ProjectRole::Viewer)],
    };
    // Repository backlink admission uses this read-level entitlement.
    assert!(gate.admits_read(viewer.public_key().as_bytes()));
    assert!(!gate.admits_write(viewer.public_key().as_bytes()));
    let head = project_head(&creator, &[]);
    let self_link = repo_announcement(&viewer, "viewer-repo", Some(&coordinate), &[]);
    assert!(decide_pack_source_admission(
        &viewer.public_key().to_hex(),
        &coordinate,
        &creator.public_key().to_hex(),
        &[(viewer.public_key().to_hex(), ProjectRole::Viewer)],
        &[self_link],
        true,
        Some(&head),
    )
    .is_err());
}

#[test]
fn only_the_exact_creator_signed_forward_roster_can_endorse_a_repository() {
    let creator = Keys::generate();
    let signer = Keys::generate();
    let coordinate = project_coordinate(&creator.public_key());
    let announcement = repo_announcement(&signer, "candidate", Some(&coordinate), &[]);
    let repo = repository_coordinate(&announcement).expect("repo");
    let genuine = project_head(&creator, std::slice::from_ref(&repo));
    let wrong_owner = project_head(&signer, std::slice::from_ref(&repo));
    let wrong_project = EventBuilder::new(Kind::Custom(KIND_PROJECT as u16), "{}")
        .tags([
            Tag::parse(["d", "other-project"]).expect("d"),
            Tag::parse(["a", &repo]).expect("a"),
        ])
        .sign_with_keys(&creator)
        .expect("sign");
    let wrong_repo = project_head(
        &creator,
        &[format!("30617:{}:candidate", creator.public_key().to_hex())],
    );
    let removed = project_head(&creator, &[]);
    let mut forged = genuine.clone();
    forged.content.push(' ');
    for head in [
        None,
        Some(&wrong_owner),
        Some(&wrong_project),
        Some(&wrong_repo),
        Some(&removed),
        Some(&forged),
    ] {
        assert!(decide_pack_source_admission(
            &signer.public_key().to_hex(),
            &coordinate,
            &creator.public_key().to_hex(),
            &[],
            std::slice::from_ref(&announcement),
            true,
            head,
        )
        .is_err());
    }
    assert!(decide_pack_source_admission(
        &signer.public_key().to_hex(),
        &coordinate,
        &creator.public_key().to_hex(),
        &[],
        &[announcement],
        true,
        Some(&genuine),
    )
    .is_ok());
}

/// Exercise the production head query, not merely its decision helper.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn founder_endorsement_uses_current_nondeleted_project_head_in_the_same_community() {
    let state = crate::api::git::policy::tests::policy_test_state().await;
    let mut communities = Vec::new();
    for label in ["unendorsed", "endorsed"] {
        communities.push(
            state
                .db
                .ensure_configured_community(&format!(
                    "packs-{label}-{}.example",
                    uuid::Uuid::new_v4().simple()
                ))
                .await
                .expect("test community")
                .id,
        );
    }
    let creator = Keys::generate();
    let signer = Keys::generate();
    let cofounder = Keys::generate();
    let project = project_coordinate(&creator.public_key());
    let announcement = repo_announcement(
        &signer,
        "endorsed-repo",
        Some(&project),
        &[&cofounder.public_key().to_hex()],
    );
    let repo = repository_coordinate(&announcement).expect("repo coordinate");
    let source = pack_source_event(&cofounder, &project, &repo);
    let head = |listed: bool, at: u64| {
        let mut tags = vec![Tag::parse(["d", "agiterra"]).expect("d")];
        if listed {
            tags.push(Tag::parse(["a", repo.as_str()]).expect("a"));
        }
        EventBuilder::new(Kind::Custom(KIND_PROJECT as u16), r#"{"name":"agiterra"}"#)
            .tags(tags)
            .custom_created_at(nostr::Timestamp::from(at))
            .sign_with_keys(&creator)
            .expect("signed head")
    };
    for community in &communities {
        state
            .db
            .replace_parameterized_event(*community, &announcement, "endorsed-repo", None)
            .await
            .expect("repo announcement");
    }
    let initial = head(true, 100);
    state
        .db
        .replace_parameterized_event(communities[1], &initial, "agiterra", None)
        .await
        .expect("other community endorsement");
    assert!(pack_source_write_admitted(&state, communities[0], &source)
        .await
        .expect("query")
        .is_err());
    assert!(matches!(
        pack_source_write_admitted(&state, communities[1], &source)
            .await
            .expect("query"),
        Ok(PackSourceAdmission::RepositoryFounder { .. })
    ));

    state
        .db
        .replace_parameterized_event(communities[0], &initial, "agiterra", None)
        .await
        .expect("local endorsement");
    assert!(pack_source_write_admitted(&state, communities[0], &source)
        .await
        .expect("query")
        .is_ok());
    state
        .db
        .replace_parameterized_event(communities[0], &head(false, 101), "agiterra", None)
        .await
        .expect("remove endorsement");
    assert!(pack_source_write_admitted(&state, communities[0], &source)
        .await
        .expect("query")
        .is_err());
    // Reposting an older signed endorsement cannot restore membership.
    state
        .db
        .replace_parameterized_event(communities[0], &initial, "agiterra", None)
        .await
        .expect("stale head");
    assert!(pack_source_write_admitted(&state, communities[0], &source)
        .await
        .expect("query")
        .is_err());
    assert!(pack_source_write_admitted(&state, communities[1], &source)
        .await
        .expect("other community unchanged")
        .is_ok());

    let restored = head(true, 102);
    state
        .db
        .replace_parameterized_event(communities[0], &restored, "agiterra", None)
        .await
        .expect("restore endorsement");
    assert!(pack_source_write_admitted(&state, communities[0], &source)
        .await
        .expect("query")
        .is_ok());
    state
        .db
        .soft_delete_event(communities[0], restored.id.as_bytes())
        .await
        .expect("delete current head");
    assert!(pack_source_write_admitted(&state, communities[0], &source)
        .await
        .expect("query")
        .is_err());
}
