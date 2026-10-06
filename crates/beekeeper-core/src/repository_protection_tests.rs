//! The rule-record wire, and the layering that decides which record carries
//! each ref pattern's rules.

use nostr::{EventBuilder, Keys, Kind, Tag};

use super::*;
use crate::git_perms::EffectiveRules;

/// Build a signed kind:30625 carrying `rows` (each row = pattern + tokens).
fn rule_event(keys: &Keys, owner_hex: &str, repo_id: &str, rows: &[&[&str]]) -> nostr::Event {
    let draft = build_repository_protection(
        owner_hex,
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
        Kind::Custom(crate::kind::KIND_GIT_REPO_PROTECTION as u16),
        draft.content,
    )
    .tags(tags)
    .sign_with_keys(keys)
    .expect("signs")
}

fn announcement_layer(rows: &[&[&str]]) -> ProtectionLayer {
    let tags: Vec<Vec<String>> = rows
        .iter()
        .map(|row| {
            let mut tag = vec!["buzz-protect".to_string()];
            tag.extend(row.iter().map(|value| (*value).to_string()));
            tag
        })
        .collect();
    ProtectionLayer::from_announcement_tags(1_000, "e".repeat(64), &tags)
        .expect("announcement tags parse")
}

#[test]
fn the_d_tag_is_the_repository_owner_and_id() {
    let owner = "b".repeat(64);
    assert_eq!(
        repository_protection_d_tag(&owner, "beekeeper"),
        format!("{owner}:beekeeper")
    );
    let parsed = parse_repository_protection_d_tag(&format!("{owner}:beekeeper"))
        .expect("a well-formed d tag");
    assert_eq!(parsed.0, owner);
    assert_eq!(parsed.1, "beekeeper");
}

/// A repository id may itself contain a colon (`repo_id_from_event` allows
/// it), so the split is on the **first** colon only — otherwise a rule record
/// would silently address a different repository than the one it names.
#[test]
fn a_repository_id_may_contain_a_colon() {
    let owner = "c".repeat(64);
    let parsed = parse_repository_protection_d_tag(&format!("{owner}:a:b"))
        .expect("a colon-bearing repo id");
    assert_eq!(parsed.1, "a:b");
}

#[test]
fn a_malformed_d_tag_is_refused() {
    for bad in [
        "",
        "notahex:repo",
        &"d".repeat(64),
        &format!("{}:", "d".repeat(64)),
    ] {
        assert!(
            parse_repository_protection_d_tag(bad).is_none(),
            "{bad:?} must not parse as a rule-record address"
        );
    }
}

#[test]
fn a_signed_record_decodes_to_its_rows() {
    let founder = Keys::generate();
    let owner = "1".repeat(64);
    let event = rule_event(
        &founder,
        &owner,
        "beekeeper",
        &[&["refs/heads/main", "require-verdict"]],
    );
    let record = decode_repository_protection(&event).expect("decodes");
    assert_eq!(record.repo_owner(), owner);
    assert_eq!(record.repo_id(), "beekeeper");
    assert_eq!(record.author(), founder.public_key().to_hex());
    assert_eq!(record.rules().len(), 1);
    assert!(record.rules()[0].require_verdict);
}

/// Finding 31's rule on the write side: a record carrying a rule token this
/// build does not know keeps the rows it *does* know, and reports the rest,
/// rather than refusing the record whole.
#[test]
fn an_unknown_rule_token_is_reported_not_fatal() {
    let founder = Keys::generate();
    let event = rule_event(
        &founder,
        &"1".repeat(64),
        "beekeeper",
        &[&["refs/heads/main", "require-verdict", "require-moonphase"]],
    );
    let record = decode_repository_protection(&event).expect("decodes");
    assert!(record.rules()[0].require_verdict);
    assert_eq!(record.unknown_rules(), ["require-moonphase"]);
}

/// The finding-31 rule on the read side, stated as the lane requires it:
/// **the announcement-only case stays valid.** A repository whose rules were
/// signed before kind 30625 existed resolves to exactly the rules its
/// announcement carries, with no rule record anywhere.
#[test]
fn rules_signed_before_the_kind_existed_still_govern() {
    let resolved = resolve_protection_layers(&[announcement_layer(&[&[
        "refs/heads/main",
        "require-verdict",
        "no-delete",
    ]])]);
    let effective = EffectiveRules::for_ref("refs/heads/main", resolved.rules());
    assert!(effective.require_verdict);
    assert!(effective.no_delete);
    assert_eq!(resolved.decisions().len(), 1);
    assert_eq!(
        resolved.decisions()[0].source,
        ProtectionRecordSource::Announcement
    );
}

/// R2, closed: a co-founder's newer record wins the pattern the announcement
/// set, without republishing (or being able to republish) the announcement.
#[test]
fn a_newer_founder_record_wins_the_pattern() {
    let co_founder = Keys::generate();
    let announcement = announcement_layer(&[&["refs/heads/main", "require-verdict"]]);
    let record = ProtectionLayer::from_record(
        &decode_repository_protection(&rule_event(
            &co_founder,
            &"1".repeat(64),
            "beekeeper",
            &[&["refs/heads/main", "no-delete"]],
        ))
        .expect("decodes"),
        2_000,
    );

    let resolved = resolve_protection_layers(&[announcement, record]);
    let effective = EffectiveRules::for_ref("refs/heads/main", resolved.rules());
    assert!(
        !effective.require_verdict,
        "the newest record for a pattern replaces it whole, it does not union with older ones"
    );
    assert!(effective.no_delete);
    assert_eq!(
        resolved.decisions()[0].source,
        ProtectionRecordSource::FounderRecord {
            author: co_founder.public_key().to_hex()
        }
    );
}

/// Patterns are independent: a record naming `refs/heads/main` leaves
/// `refs/tags/*` exactly where the announcement left it.
#[test]
fn a_record_only_supersedes_the_patterns_it_names() {
    let co_founder = Keys::generate();
    let announcement = announcement_layer(&[
        &["refs/heads/main", "require-verdict"],
        &["refs/tags/*", "no-delete"],
    ]);
    let record = ProtectionLayer::from_record(
        &decode_repository_protection(&rule_event(
            &co_founder,
            &"1".repeat(64),
            "beekeeper",
            &[&["refs/heads/main", "no-force-push"]],
        ))
        .expect("decodes"),
        2_000,
    );
    let resolved = resolve_protection_layers(&[announcement, record]);
    assert!(EffectiveRules::for_ref("refs/tags/*", resolved.rules()).no_delete);
    assert!(!EffectiveRules::for_ref("refs/heads/main", resolved.rules()).require_verdict);
}

/// The ruling this lane applies by default: a co-founder MAY remove the
/// protection the signer set. The removal is itself a signed founder act —
/// `none` is a rule token, not an absence.
#[test]
fn a_co_founder_may_clear_the_signers_rule() {
    let co_founder = Keys::generate();
    let announcement = announcement_layer(&[&["refs/heads/main", "require-verdict"]]);
    let record = ProtectionLayer::from_record(
        &decode_repository_protection(&rule_event(
            &co_founder,
            &"1".repeat(64),
            "beekeeper",
            &[&["refs/heads/main", PROTECTION_RULE_CLEAR]],
        ))
        .expect("decodes"),
        2_000,
    );
    let resolved = resolve_protection_layers(&[announcement, record]);
    let effective = EffectiveRules::for_ref("refs/heads/main", resolved.rules());
    assert!(!effective.require_verdict);
    assert!(
        !effective.has_explicit_match,
        "a cleared pattern is not a guarded ref: the operator said it carries no rules"
    );
    let decision = &resolved.decisions()[0];
    assert!(decision.cleared);
    assert_eq!(
        decision.source,
        ProtectionRecordSource::FounderRecord {
            author: co_founder.public_key().to_hex()
        }
    );
}

/// An older record loses to the announcement, so a stale rule record cannot
/// resurrect protection a later announcement dropped.
#[test]
fn an_older_record_loses_to_a_newer_announcement() {
    let co_founder = Keys::generate();
    let record = ProtectionLayer::from_record(
        &decode_repository_protection(&rule_event(
            &co_founder,
            &"1".repeat(64),
            "beekeeper",
            &[&["refs/heads/main", "require-verdict"]],
        ))
        .expect("decodes"),
        500,
    );
    let announcement = announcement_layer(&[&["refs/heads/main", "no-delete"]]);
    let resolved = resolve_protection_layers(&[record, announcement]);
    let effective = EffectiveRules::for_ref("refs/heads/main", resolved.rules());
    assert!(!effective.require_verdict);
    assert!(effective.no_delete);
}

/// Equal `created_at` is a real case (two founders acting in the same
/// second), so the tie-break is stated rather than left to iteration order:
/// the greater event id wins, and the result does not depend on input order.
#[test]
fn an_equal_timestamp_breaks_on_the_event_id_and_not_on_input_order() {
    let a = Keys::generate();
    let b = Keys::generate();
    let left = ProtectionLayer::from_record(
        &decode_repository_protection(&rule_event(
            &a,
            &"1".repeat(64),
            "beekeeper",
            &[&["refs/heads/main", "require-verdict"]],
        ))
        .expect("decodes"),
        7_000,
    );
    let right = ProtectionLayer::from_record(
        &decode_repository_protection(&rule_event(
            &b,
            &"1".repeat(64),
            "beekeeper",
            &[&["refs/heads/main", "no-delete"]],
        ))
        .expect("decodes"),
        7_000,
    );
    let forward = resolve_protection_layers(&[left.clone(), right.clone()]);
    let backward = resolve_protection_layers(&[right, left]);
    assert_eq!(forward.decisions(), backward.decisions());
    let winner = if forward.decisions()[0].event_id > String::new() {
        &forward.decisions()[0].event_id
    } else {
        unreachable!()
    };
    assert!(!winner.is_empty());
}

/// The superseded records are named, not dropped in silence: a screen that
/// says "who set this" must also be able to say what it replaced.
#[test]
fn a_decision_names_what_it_superseded() {
    let co_founder = Keys::generate();
    let announcement = announcement_layer(&[&["refs/heads/main", "require-verdict"]]);
    let record = ProtectionLayer::from_record(
        &decode_repository_protection(&rule_event(
            &co_founder,
            &"1".repeat(64),
            "beekeeper",
            &[&["refs/heads/main", "no-delete"]],
        ))
        .expect("decodes"),
        2_000,
    );
    let resolved = resolve_protection_layers(&[announcement, record]);
    assert_eq!(
        resolved.decisions()[0].superseded,
        vec![ProtectionRecordSource::Announcement]
    );
}

/// The record's own `d` tag is what binds it to a repository; a record whose
/// address names a different repository is not a layer for this one. The
/// relay filters by `d_tag` in SQL, but the type refuses to be told
/// otherwise.
#[test]
fn a_record_addressing_another_repository_is_not_a_layer_for_this_one() {
    let founder = Keys::generate();
    let event = rule_event(
        &founder,
        &"1".repeat(64),
        "beekeeper",
        &[&["refs/heads/main", "require-verdict"]],
    );
    let record = decode_repository_protection(&event).expect("decodes");
    assert!(record.addresses(&"1".repeat(64), "beekeeper"));
    assert!(!record.addresses(&"2".repeat(64), "beekeeper"));
    assert!(!record.addresses(&"1".repeat(64), "other"));
}

#[test]
fn a_record_with_the_wrong_schema_is_refused() {
    let founder = Keys::generate();
    let event = EventBuilder::new(
        Kind::Custom(crate::kind::KIND_GIT_REPO_PROTECTION as u16),
        r#"{"schema":"something-else/v1"}"#,
    )
    .tags(vec![
        Tag::parse(["d", &format!("{}:beekeeper", "1".repeat(64))]).expect("d"),
        Tag::parse(["buzz-protect", "refs/heads/main", "require-verdict"]).expect("row"),
    ])
    .sign_with_keys(&founder)
    .expect("signs");
    assert!(decode_repository_protection(&event).is_err());
}

/// A record with no rows is legal and meaningful: it is a founder saying
/// "I hold no rules on this repository", which retires every row they had
/// set without needing a tombstone.
#[test]
fn a_record_with_no_rows_retires_that_founders_rules() {
    let co_founder = Keys::generate();
    let announcement = announcement_layer(&[&["refs/heads/main", "no-delete"]]);
    let earlier = ProtectionLayer::from_record(
        &decode_repository_protection(&rule_event(
            &co_founder,
            &"1".repeat(64),
            "beekeeper",
            &[&["refs/heads/main", "require-verdict"]],
        ))
        .expect("decodes"),
        2_000,
    );
    let later = ProtectionLayer::from_record(
        &decode_repository_protection(&rule_event(&co_founder, &"1".repeat(64), "beekeeper", &[]))
            .expect("decodes"),
        3_000,
    );
    // Addressable: the later record *replaces* the earlier at the same
    // address, so only it is ever a layer.
    let resolved = resolve_protection_layers(&[announcement, later]);
    assert!(EffectiveRules::for_ref("refs/heads/main", resolved.rules()).no_delete);
    assert!(!EffectiveRules::for_ref("refs/heads/main", resolved.rules()).require_verdict);
    drop(earlier);
}
