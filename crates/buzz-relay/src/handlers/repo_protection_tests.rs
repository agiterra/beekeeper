//! The kind:30625 write gate — the rule, and the production path that runs it.

use nostr::{EventBuilder, Keys, Kind, Tag};

use buzz_core::repository_founders::REPOSITORY_MAINTAINERS_TAG;
use buzz_core::repository_protection::build_repository_protection;

use super::*;

fn rule_event(keys: &Keys, repo_owner_hex: &str, repo_id: &str, rows: &[&[&str]]) -> nostr::Event {
    let draft = build_repository_protection(
        repo_owner_hex,
        repo_id,
        &rows
            .iter()
            .map(|row| row.iter().map(|value| (*value).to_string()).collect())
            .collect::<Vec<Vec<String>>>(),
    )
    .expect("a valid draft");
    let tags: Vec<Tag> = draft
        .tags
        .iter()
        .map(|tag| Tag::parse(tag.clone()).expect("tag parses"))
        .collect();
    EventBuilder::new(
        Kind::Custom(buzz_core::kind::KIND_GIT_REPO_PROTECTION as u16),
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
    EventBuilder::new(
        Kind::Custom(buzz_core::kind::KIND_GIT_REPO_ANNOUNCEMENT as u16),
        "",
    )
    .tags(tags)
    .sign_with_keys(keys)
    .expect("signs")
}

/// The announcement's own signer is a founder and may always set rules — the
/// behaviour that existed before this kind, now reached through it.
#[test]
fn the_announcement_signer_is_admitted() {
    let signer = Keys::generate();
    let announcement = repo_announcement(&signer, "beekeeper", None, &[]);
    let admitted = decide_repo_protection_admission(
        &signer.public_key().to_hex(),
        &signer.public_key().to_hex(),
        "beekeeper",
        Some(&announcement),
        &[],
        false,
    )
    .expect("admitted");
    assert_eq!(admitted, RepoProtectionAdmission::AnnouncementSigner);
}

/// R2 closed: a NIP-34 maintainer may set rules on a repository they did not
/// announce.
#[test]
fn a_maintainer_is_admitted() {
    let signer = Keys::generate();
    let co_founder = Keys::generate();
    let announcement = repo_announcement(
        &signer,
        "beekeeper",
        None,
        &[&co_founder.public_key().to_hex()],
    );
    let admitted = decide_repo_protection_admission(
        &co_founder.public_key().to_hex(),
        &signer.public_key().to_hex(),
        "beekeeper",
        Some(&announcement),
        &[],
        false,
    )
    .expect("admitted");
    assert_eq!(admitted, RepoProtectionAdmission::RepositoryFounder);
}

/// Finding 33's own case: the roster Owner of the project the repository
/// belongs to, who is neither its signer nor a listed maintainer.
#[test]
fn a_project_roster_owner_is_admitted() {
    use buzz_core::channel::ProjectRole;

    let signer = Keys::generate();
    let roster_owner = Keys::generate();
    let coordinate = format!("30621:{}:agiterra", signer.public_key().to_hex());
    let announcement = repo_announcement(&signer, "beekeeper", Some(&coordinate), &[]);
    let admitted = decide_repo_protection_admission(
        &roster_owner.public_key().to_hex(),
        &signer.public_key().to_hex(),
        "beekeeper",
        Some(&announcement),
        &[(roster_owner.public_key().to_hex(), ProjectRole::Owner)],
        true,
    )
    .expect("admitted");
    assert_eq!(admitted, RepoProtectionAdmission::RepositoryFounder);
}

/// A collaborator is not a founder, and the refusal says what was checked
/// rather than reporting a partial answer as a whole one.
#[test]
fn a_collaborator_is_refused_and_told_what_was_checked() {
    use buzz_core::channel::ProjectRole;

    let signer = Keys::generate();
    let collaborator = Keys::generate();
    let coordinate = format!("30621:{}:agiterra", signer.public_key().to_hex());
    let announcement = repo_announcement(&signer, "beekeeper", Some(&coordinate), &[]);
    let refusal = decide_repo_protection_admission(
        &collaborator.public_key().to_hex(),
        &signer.public_key().to_hex(),
        "beekeeper",
        Some(&announcement),
        &[(
            collaborator.public_key().to_hex(),
            ProjectRole::Collaborator,
        )],
        true,
    )
    .expect_err("refused");
    let sentence = refusal.sentence();
    assert!(
        sentence.contains("founder of that repository"),
        "{sentence}"
    );
    assert!(sentence.contains("roster was read"), "{sentence}");
}

/// Fail closed on a repository nobody announced: an unknown repository has no
/// founders, so it has nobody who may write rules for it. Saying "not a
/// founder" without saying the announcement was missing would send the author
/// looking for the wrong problem.
#[test]
fn a_record_for_an_unannounced_repository_is_refused() {
    let author = Keys::generate();
    let refusal = decide_repo_protection_admission(
        &author.public_key().to_hex(),
        &"1".repeat(64),
        "beekeeper",
        None,
        &[],
        false,
    )
    .expect_err("refused");
    assert!(
        refusal.sentence().contains("no repository"),
        "{}",
        refusal.sentence()
    );
}

/// The production path, end to end: a stranger's rule record is refused by
/// `ingest_event` itself, and a co-founder's is stored.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn ingest_refuses_a_rule_record_from_a_stranger_and_stores_a_co_founders() {
    use crate::handlers::ingest::{ingest_event, IngestAuth, IngestError};

    let state = crate::api::git::policy::tests::policy_test_state().await;
    let host = format!("protect-{}.example", uuid::Uuid::new_v4().simple());
    let community = state
        .db
        .ensure_configured_community(&host)
        .await
        .expect("community")
        .id;
    let tenant = buzz_core::tenant::TenantContext::resolved(community, host.clone());

    let signer = Keys::generate();
    let co_founder = Keys::generate();
    let stranger = Keys::generate();
    let repo_id = format!("repo-{}", uuid::Uuid::new_v4().simple());

    let auth = |keys: &Keys| IngestAuth::Http {
        pubkey: keys.public_key(),
        scopes: vec![
            buzz_auth::Scope::ReposWrite,
            buzz_auth::Scope::MessagesWrite,
            buzz_auth::Scope::ChannelsWrite,
        ],
        auth_method: crate::handlers::ingest::HttpAuthMethod::Nip98,
    };

    ingest_event(
        &state,
        &tenant,
        repo_announcement(
            &signer,
            &repo_id,
            None,
            &[&co_founder.public_key().to_hex()],
        ),
        auth(&signer),
    )
    .await
    .expect("the announcement stores");

    let owner_hex = signer.public_key().to_hex();
    let refused = ingest_event(
        &state,
        &tenant,
        rule_event(
            &stranger,
            &owner_hex,
            &repo_id,
            &[&["refs/heads/main", "require-verdict"]],
        ),
        auth(&stranger),
    )
    .await;
    match refused {
        Err(IngestError::AuthFailed(message)) => assert!(
            message.contains("founder of that repository"),
            "the refusal must say why: {message}"
        ),
        Err(other) => {
            panic!("a stranger's rule record must be refused for the right reason: {other:?}")
        }
        Ok(result) => panic!(
            "a stranger's rule record must be refused, accepted={}",
            result.accepted
        ),
    }

    let accepted = ingest_event(
        &state,
        &tenant,
        rule_event(
            &co_founder,
            &owner_hex,
            &repo_id,
            &[&["refs/heads/main", "require-verdict"]],
        ),
        auth(&co_founder),
    )
    .await
    .expect("the co-founder's rule record stores");
    assert!(
        accepted.accepted,
        "a maintainer's rule record must be accepted"
    );
}
