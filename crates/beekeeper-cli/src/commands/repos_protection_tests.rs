//! The pure half of `bee repos protect` against a co-founded repository.

use nostr::{EventBuilder, Keys, Kind, Tag};

use super::*;

fn record_with_rows(keys: &Keys, rows: &[&[&str]]) -> Event {
    let mut tags = vec![Tag::parse(["d", &format!("{}:beekeeper", "1".repeat(64))]).unwrap()];
    for row in rows {
        let mut values = vec!["buzz-protect".to_string()];
        values.extend(row.iter().map(|value| (*value).to_string()));
        tags.push(Tag::parse(values).unwrap());
    }
    EventBuilder::new(
        Kind::Custom(KIND_GIT_REPO_PROTECTION as u16),
        r#"{"schema":"buzz-repo-protection/v1"}"#,
    )
    .tags(tags)
    .sign_with_keys(keys)
    .expect("signs")
}

/// Setting a rule replaces only that pattern's row in the caller's own
/// record; every other row they hold is carried through.
#[test]
fn setting_a_rule_keeps_the_founders_other_rows() {
    let keys = Keys::generate();
    let current = record_with_rows(
        &keys,
        &[
            &["refs/heads/main", "require-verdict"],
            &["refs/tags/*", "no-delete"],
        ],
    );
    let rows = next_rule_record_rows(
        Some(&current),
        "refs/heads/main",
        Some(vec!["refs/heads/main".into(), "no-force-push".into()]),
    );
    assert!(rows.contains(&vec!["refs/tags/*".to_string(), "no-delete".to_string()]));
    assert!(rows.contains(&vec![
        "refs/heads/main".to_string(),
        "no-force-push".to_string()
    ]));
    assert_eq!(rows.len(), 2);
}

/// Removing writes the clear token, not an absence: an absent row would fall
/// back to the announcement's rule, which is the opposite of "remove it".
#[test]
fn removing_a_rule_writes_the_clear_token() {
    let keys = Keys::generate();
    let current = record_with_rows(&keys, &[&["refs/heads/main", "require-verdict"]]);
    let rows = next_rule_record_rows(Some(&current), "refs/heads/main", None);
    assert_eq!(
        rows,
        vec![vec![
            "refs/heads/main".to_string(),
            PROTECTION_RULE_CLEAR.to_string()
        ]]
    );
}

/// A founder with no record yet writes one carrying exactly the one row.
#[test]
fn a_founder_with_no_record_writes_their_first_row() {
    let rows = next_rule_record_rows(
        None,
        "refs/heads/main",
        Some(vec!["refs/heads/main".into(), "require-verdict".into()]),
    );
    assert_eq!(
        rows,
        vec![vec![
            "refs/heads/main".to_string(),
            "require-verdict".to_string()
        ]]
    );
}

/// The record a caller may write is the announcement when they signed it and
/// their own rule record otherwise — the whole of finding 33 R2's fix, in one
/// function.
#[test]
fn the_signer_writes_the_announcement_and_a_co_founder_writes_a_record() {
    let signer = Keys::generate();
    let co_founder = Keys::generate();
    let announcement = EventBuilder::new(Kind::Custom(KIND_GIT_REPO_ANNOUNCEMENT as u16), "")
        .tags(vec![
            Tag::parse(["d", "beekeeper"]).unwrap(),
            Tag::parse(["maintainers", &co_founder.public_key().to_hex()]).unwrap(),
        ])
        .sign_with_keys(&signer)
        .expect("signs");
    let founders = RepositoryFounders::from_announcement(&announcement).with_roster_owners(vec![]);
    let rules = RepositoryRules {
        announcement,
        founders,
        resolved: ResolvedProtection::default(),
        records_read: 0,
        records_from_non_founders: 0,
    };

    assert!(rules.may_set_rules(&signer.public_key().to_hex()));
    assert!(rules.may_set_rules(&co_founder.public_key().to_hex()));
    assert!(!rules.may_set_rules(&Keys::generate().public_key().to_hex()));
    assert_eq!(
        rules.writable_record(&signer.public_key().to_hex()),
        WritableRecord::Announcement
    );
    assert_eq!(
        rules.writable_record(&co_founder.public_key().to_hex()),
        WritableRecord::RuleRecord
    );
}

/// Read-optional, on the CLI's own reader: a repository with no rule record
/// lists exactly the rules its announcement carries, and says the
/// announcement is what carries them.
#[test]
fn rules_signed_before_the_kind_existed_still_list() {
    let signer = Keys::generate();
    let announcement = EventBuilder::new(Kind::Custom(KIND_GIT_REPO_ANNOUNCEMENT as u16), "")
        .tags(vec![
            Tag::parse(["d", "beekeeper"]).unwrap(),
            Tag::parse(["buzz-protect", "refs/heads/main", "require-verdict"]).unwrap(),
        ])
        .sign_with_keys(&signer)
        .expect("signs");
    let tags: Vec<Vec<String>> = announcement
        .tags
        .iter()
        .map(|tag| tag.as_slice().to_vec())
        .collect();
    let layer = ProtectionLayer::from_announcement_tags(
        announcement.created_at.as_secs(),
        announcement.id.to_hex(),
        &tags,
    )
    .expect("parses");
    let rules = RepositoryRules {
        founders: RepositoryFounders::from_announcement(&announcement).with_roster_owners(vec![]),
        announcement,
        resolved: resolve_protection_layers(&[layer]),
        records_read: 0,
        records_from_non_founders: 0,
    };
    let decision = rules
        .resolved
        .decision_for("refs/heads/main")
        .expect("the announcement's own rule");
    let json = decision_json(&rules, decision);
    assert_eq!(json["record"], "announcement");
    assert_eq!(json["rules"][0], "require-verdict");
    assert_eq!(json["signed_by"], rules.announcement.pubkey.to_hex());
    assert_eq!(json["cleared"], false);
}

/// A listing says which founder's record carries a rule and what it replaced,
/// so a rule nobody can find is not a state this command can produce.
#[test]
fn a_listing_names_the_record_and_its_signer() {
    let signer = Keys::generate();
    let co_founder = Keys::generate();
    let announcement = EventBuilder::new(Kind::Custom(KIND_GIT_REPO_ANNOUNCEMENT as u16), "")
        .tags(vec![
            Tag::parse(["d", "beekeeper"]).unwrap(),
            Tag::parse(["buzz-protect", "refs/heads/main", "require-verdict"]).unwrap(),
        ])
        .sign_with_keys(&signer)
        .expect("signs");
    let tags: Vec<Vec<String>> = announcement
        .tags
        .iter()
        .map(|tag| tag.as_slice().to_vec())
        .collect();
    let base = ProtectionLayer::from_announcement_tags(
        announcement.created_at.as_secs(),
        announcement.id.to_hex(),
        &tags,
    )
    .expect("parses");
    let record = record_with_rows(&co_founder, &[&["refs/heads/main", "no-delete"]]);
    let layer = ProtectionLayer::from_record(
        &decode_repository_protection(&record).expect("decodes"),
        announcement.created_at.as_secs() + 10,
    );
    let rules = RepositoryRules {
        founders: RepositoryFounders::from_announcement(&announcement).with_roster_owners(vec![]),
        announcement,
        resolved: resolve_protection_layers(&[base, layer]),
        records_read: 1,
        records_from_non_founders: 0,
    };
    let json = decision_json(
        &rules,
        rules
            .resolved
            .decision_for("refs/heads/main")
            .expect("rule"),
    );
    assert_eq!(json["record"], "rule-record");
    assert_eq!(json["signed_by"], co_founder.public_key().to_hex());
    assert_eq!(json["record_event_id"], record.id.to_hex());
    assert_eq!(json["superseded"][0], "announcement");
}
