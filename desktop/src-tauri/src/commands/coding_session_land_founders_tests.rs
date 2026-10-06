//! Land-boundary tests about **who governs and who founds** a repository.
//!
//! Split out of `coding_session_land_tests.rs` on 2026-09-03, when three
//! lanes' cases pushed that file past the repository's 1,000-line ceiling.
//! The seam is the subject, not the calendar: everything here is about the
//! rule's *source* — an announcement's own rows, a founder's kind-30625 rule
//! record, the `maintainers` set, the project roster — rather than about the
//! verdict evidence the sibling file exercises.
//!
//! A child module of the tests module, so it shares that file's fixtures
//! (`protect`, `mission`, `request`, …) through `use super::*` rather than
//! growing a second copy of them.

use super::*;

/// Finding 31's mandatory case on this reader: an announcement whose rules
/// were signed before kind 30625 existed governs exactly as it always did,
/// with no record anywhere and `rule_records: None` on the wire.
#[test]
fn rules_signed_before_the_rule_record_kind_existed_still_govern() {
    assert!(ref_requires_verdict(
        &protect(&["require-verdict"]),
        None,
        &no_founders(),
        "refs/heads/main"
    ));
    assert!(ref_requires_verdict(
        &protect(&["require-verdict"]),
        Some(&Vec::new()),
        &no_founders(),
        "refs/heads/main"
    ));
}

/// A co-founder's record governs a ref the announcement says nothing about —
/// and a stranger's record, or an unverified one, governs nothing.
#[test]
fn only_a_founders_rule_record_governs() {
    use beekeeper_core_pkg::repository_protection::build_repository_protection;
    use nostr::{EventBuilder, Keys, Kind, Tag};

    let signer = Keys::generate();
    let co_founder = Keys::generate();
    let stranger = Keys::generate();
    let owner_hex = signer.public_key().to_hex();
    let announcement_tags = vec![
        vec!["d".to_owned(), "beekeeper".to_owned()],
        vec!["maintainers".to_owned(), co_founder.public_key().to_hex()],
    ];
    let founders = beekeeper_core_pkg::repository_founders::RepositoryFounders::from_parts(
        &owner_hex,
        &announcement_tags,
    )
    .with_roster_owners(Vec::new());

    let record = |keys: &Keys| {
        let draft = build_repository_protection(
            &owner_hex,
            "beekeeper",
            &[vec![
                "refs/heads/main".to_owned(),
                "require-verdict".to_owned(),
            ]],
        )
        .expect("a valid draft");
        let tags: Vec<Tag> = draft
            .tags
            .iter()
            .map(|tag| Tag::parse(tag.clone()).expect("tag"))
            .collect();
        serde_json::to_value(
            EventBuilder::new(
                Kind::Custom(beekeeper_core_pkg::kind::KIND_GIT_REPO_PROTECTION as u16),
                draft.content.clone(),
            )
            .tags(tags)
            .sign_with_keys(keys)
            .expect("signs"),
        )
        .expect("serializes")
    };

    assert!(
        ref_requires_verdict(
            &announcement_tags,
            Some(&vec![record(&co_founder)]),
            &founders,
            "refs/heads/main"
        ),
        "a co-founder's record governs a ref the announcement never mentioned"
    );
    assert!(
        !ref_requires_verdict(
            &announcement_tags,
            Some(&vec![record(&stranger)]),
            &founders,
            "refs/heads/main"
        ),
        "a stranger's record governs nothing, exactly as at the gate"
    );
    assert!(
        !ref_requires_verdict(
            &announcement_tags,
            Some(&vec![serde_json::json!({"not":"an event"})]),
            &founders,
            "refs/heads/main"
        ),
        "an undecodable record is skipped, never fatal"
    );
}

// ── finding 33: the Land control names the founders ─────────────────────

/// A viewer who is a `maintainers` co-founder — but not the announcement's
/// signer and not the mission's founder — may land the ruling, and the answer
/// names both founders so the screen can say who they are.
#[test]
fn a_maintainer_may_land_and_the_answer_names_both_founders() {
    let mission = mission("approve", Some(HEAD_SHA), None);
    let maintainer = Keys::generate().public_key().to_hex();
    let mut tags = protect(&["require-verdict"]);
    tags.push(vec!["maintainers".to_owned(), maintainer.clone()]);
    let mut request = request(&mission, Some(tags));
    request.pusher_pubkey = maintainer.clone();

    let answer = land_adapter(request).expect("the boundary answers");
    assert!(answer.admitted, "an equal owner lands the other's ruling");
    assert!(answer.viewer_is_founder);
    assert_eq!(
        answer.founders,
        vec![mission.founder.public_key().to_hex(), maintainer]
    );
    assert!(
        answer.founders_note.contains("(2)"),
        "the sentence names both: {}",
        answer.founders_note
    );
}

/// The roster half, which no tag carries: the caller passes the project's
/// Owners, and the same viewer becomes a founder. Passing `None` instead is
/// "not read", and the sentence says so.
#[test]
fn project_roster_owners_join_the_founder_set_and_null_is_disclosed() {
    let mission = mission("approve", Some(HEAD_SHA), None);
    let roster_owner = Keys::generate().public_key().to_hex();

    let mut read = request(&mission, Some(protect(&["require-verdict"])));
    read.pusher_pubkey = roster_owner.clone();
    read.project_owner_pubkeys = Some(vec![roster_owner.clone()]);
    let answer = land_adapter(read).expect("the boundary answers");
    assert!(answer.admitted, "a project Owner founds the repository too");
    assert!(answer.viewer_is_founder);
    assert!(
        !answer.founders_note.contains("was not read here"),
        "a read roster is not disclosed as unread: {}",
        answer.founders_note
    );

    let mut unread = request(&mission, Some(protect(&["require-verdict"])));
    unread.pusher_pubkey = roster_owner.clone();
    unread.project_owner_pubkeys = None;
    let answer = land_adapter(unread).expect("the boundary answers");
    assert!(!answer.admitted, "an unread roster grants nobody");
    assert!(!answer.viewer_is_founder);
    assert!(
        answer.founders_note.contains("was not read here"),
        "an unread roster is disclosed: {}",
        answer.founders_note
    );
}

/// With no repository record the founder line is a fact about the *read*, not
/// a claim that the repository has no founders.
#[test]
fn no_repository_record_names_no_founders_and_says_why() {
    let mission = mission("approve", Some(HEAD_SHA), None);
    let answer = land_adapter(request(&mission, None)).expect("the boundary answers");
    assert!(!answer.repository_known);
    assert!(answer.founders.is_empty());
    assert_eq!(answer.founders_note, NO_REPOSITORY_FOUNDERS_NOTE);
    assert!(!answer.viewer_is_founder);
}
