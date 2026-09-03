//! Tests for [`super::RepositoryFounders`] — finding 33's rule.

use super::*;
use nostr::{EventBuilder, Keys, Kind, Tag};

/// A kind:30617 announcement signed by `keys`, carrying `tags`.
fn announcement(keys: &Keys, tags: Vec<Vec<String>>) -> Event {
    let mut builder = EventBuilder::new(Kind::from(30617u16), "");
    for tag in tags {
        builder = builder.tag(Tag::parse(tag).expect("tag parses"));
    }
    builder.sign_with_keys(keys).expect("event signs")
}

fn hex(byte: u8) -> String {
    format!("{byte:02x}").repeat(32)
}

/// The signer alone founds a repository that declares no maintainers — the
/// pre-finding-33 behaviour, preserved exactly.
#[test]
fn a_bare_announcement_founds_only_its_signer() {
    let keys = Keys::generate();
    let founders = RepositoryFounders::from_announcement(&announcement(&keys, vec![]));
    assert_eq!(founders.pubkeys(), &[keys.public_key().to_hex()]);
    assert_eq!(founders.len(), 1);
    assert!(founders.contains(&keys.public_key().to_hex()));
    assert!(!founders.contains(&hex(0x11)));
    assert_eq!(founders.signer(), keys.public_key().to_hex());
}

/// The finding-33 shape: Andy signs, Brian is in `maintainers`, and both are
/// founders with the signer first.
#[test]
fn a_maintainers_tag_adds_a_founder() {
    let keys = Keys::generate();
    let brian = hex(0x3d);
    let founders = RepositoryFounders::from_announcement(&announcement(
        &keys,
        vec![vec!["maintainers".into(), brian.clone()]],
    ));
    assert_eq!(
        founders.pubkeys(),
        &[keys.public_key().to_hex(), brian.clone()]
    );
    assert!(founders.contains(&brian));
    assert!(
        founders.contains(&brian.to_uppercase()),
        "hex is case-folded"
    );
}

/// One multi-value tag, several tags, uppercase input and a repeat of the
/// signer all fold into one deduped, lower-hex, signer-first list.
#[test]
fn founders_are_deduped_lowercased_and_signer_first() {
    let keys = Keys::generate();
    let signer = keys.public_key().to_hex();
    let a = hex(0xa1);
    let b = hex(0xb2);
    let founders = RepositoryFounders::from_announcement(&announcement(
        &keys,
        vec![
            vec!["maintainers".into(), a.to_uppercase(), b.clone()],
            vec!["maintainers".into(), a.clone(), signer.to_uppercase()],
        ],
    ));
    assert_eq!(founders.pubkeys(), &[signer, a, b]);
    assert_eq!(founders.maintainers_declared(), 4);
    assert_eq!(founders.invalid_maintainers(), 0);
}

/// A maintainer value that is not 64-hex is ignored **and counted**: a typo in
/// a co-founder's key must not silently shrink the set to one.
#[test]
fn invalid_maintainer_values_are_ignored_and_counted() {
    let keys = Keys::generate();
    let good = hex(0xc3);
    let founders = RepositoryFounders::from_announcement(&announcement(
        &keys,
        vec![vec![
            "maintainers".into(),
            "not-hex".into(),
            good.clone(),
            "abc".repeat(3),
        ]],
    ));
    assert_eq!(founders.pubkeys(), &[keys.public_key().to_hex(), good]);
    assert_eq!(founders.maintainers_declared(), 3);
    assert_eq!(founders.invalid_maintainers(), 2);
    assert!(
        founders.rules_sentence().contains("2 maintainer value(s)"),
        "the sentence discloses ignored values: {}",
        founders.rules_sentence()
    );
}

/// A tag that is not `maintainers` never contributes a founder — including the
/// `buzz-protect` rows that share the announcement.
#[test]
fn other_tags_never_found_anyone() {
    let keys = Keys::generate();
    let founders = RepositoryFounders::from_announcement(&announcement(
        &keys,
        vec![
            vec!["d".into(), "agiterra-beekeeper".into()],
            vec!["buzz-protect".into(), "refs/heads/main".into()],
            vec!["p".into(), hex(0xee)],
            vec!["maintainer".into(), hex(0xef)],
        ],
    ));
    assert_eq!(founders.pubkeys(), &[keys.public_key().to_hex()]);
}

/// The coordinator's live case: the repository's project roster names a second
/// human `owner` who is neither signer nor maintainer. Andy's `a56ad5d01`
/// model already grants them git Owner; the founder set now says so.
#[test]
fn a_project_owner_on_the_roster_is_a_founder() {
    let keys = Keys::generate();
    let brian = hex(0x3d);
    let collaborator = hex(0x77);
    let viewer = hex(0x88);
    let founders = RepositoryFounders::from_announcement(&announcement(&keys, vec![]))
        .with_roster_roles(vec![
            (keys.public_key().to_hex(), ProjectRole::Owner),
            (brian.clone(), ProjectRole::Owner),
            (collaborator, ProjectRole::Collaborator),
            (viewer, ProjectRole::Viewer),
        ]);
    assert_eq!(
        founders.pubkeys(),
        &[keys.public_key().to_hex(), brian.clone()]
    );
    assert_eq!(
        founders.roster_owners_read(),
        Some(1),
        "the signer's own roster row adds nobody new"
    );
    assert!(founders.contains(&brian));
}

/// A roster that grants no Owner still counts as **read** — `Some(0)` and
/// `None` are different facts, and only the second earns the disclosure.
#[test]
fn an_empty_roster_read_is_not_an_unread_roster() {
    let keys = Keys::generate();
    let read = RepositoryFounders::from_announcement(&announcement(&keys, vec![]))
        .with_roster_roles(Vec::new());
    let unread = RepositoryFounders::from_announcement(&announcement(&keys, vec![]));
    assert_eq!(read.roster_owners_read(), Some(0));
    assert_eq!(unread.roster_owners_read(), None);
    assert!(!read.rules_sentence().contains("was not read here"));
    assert!(
        unread.rules_sentence().contains("was not read here"),
        "an unread roster is disclosed: {}",
        unread.rules_sentence()
    );
}

/// The sentence names the signer as the only key that may rewrite the rules —
/// the v1 residual, said out loud rather than discovered.
#[test]
fn the_rules_sentence_says_rules_are_signer_only() {
    let keys = Keys::generate();
    let brian = hex(0x3d);
    let sentence = RepositoryFounders::from_announcement(&announcement(
        &keys,
        vec![vec!["maintainers".into(), brian.clone()]],
    ))
    .with_roster_roles(Vec::new())
    .rules_sentence();
    assert!(
        sentence.contains(&format!(
            "rules are set by the announcement's signer {} and only that key can rewrite them",
            keys.public_key().to_hex()
        )),
        "sentence: {sentence}"
    );
    assert!(sentence.contains(&brian), "sentence: {sentence}");
    assert!(sentence.contains("(2)"), "sentence: {sentence}");
}
