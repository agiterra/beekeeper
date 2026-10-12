//! Exact managed deletion tests using the actual scoped retention DB.
use super::inbound_tests::{deletion_event_with_keys, local_agent};
use super::*;

// Actual retention DB regressions for legal e-only managed-agent deletion.
fn retain_exact_delete_head(
    conn: &rusqlite::Connection,
    head: &nostr::Event,
    actor: &str,
    pending: bool,
) {
    use crate::managed_agents::retention::{retain_event, RetainedEvent};
    use nostr::JsonUtil;
    retain_event(
        conn,
        &RetainedEvent {
            kind: 30177,
            pubkey: head.pubkey.to_hex(),
            d_tag: actor.into(),
            content: head.content.clone(),
            created_at: head.created_at.as_secs() as i64,
            raw_event: head.as_json(),
            pending_sync: pending,
        },
    )
    .unwrap();
}
fn exact_managed_delete(owner: &nostr::Keys, _actor: &str, head: &nostr::Event) -> nostr::Event {
    nostr::EventBuilder::new(nostr::Kind::Custom(5), "")
        .tags([nostr::Tag::parse(["e", &head.id.to_hex()]).unwrap()])
        .custom_created_at(nostr::Timestamp::from(300))
        .sign_with_keys(owner)
        .unwrap()
}
#[test]
fn exact_managed_deletion_accepts_current_signed_head_without_touching_source_custody() {
    use crate::managed_agents::{agent_events::build_agent_event, retention::*};
    let dir = tempfile::tempdir().unwrap();
    let conn = open_retention_db(&dir.path().join("retention.sqlite")).unwrap();
    let owner = nostr::Keys::generate();
    let mut record = local_agent();
    record.pubkey = nostr::Keys::generate().public_key().to_hex();
    let head = build_agent_event(&record)
        .unwrap()
        .custom_created_at(nostr::Timestamp::from(100))
        .sign_with_keys(&owner)
        .unwrap();
    retain_exact_delete_head(&conn, &head, &record.pubkey, false);
    let deletion = exact_managed_delete(&owner, &record.pubkey, &head);
    assert_eq!(
        exact_managed_deletion::coordinate(&conn, &deletion).unwrap(),
        Some((30177, record.pubkey.clone()))
    );
    let source = serde_json::to_value(&record).unwrap();
    assert_eq!(
        exact_managed_deletion::retain_tombstone(
            &conn,
            &deletion,
            30177,
            &record.pubkey,
            &[record.clone()]
        )
        .unwrap(),
        InboundOutcome::Applied
    );
    assert!(get_retained_event(
        &conn,
        5,
        &owner.public_key().to_hex(),
        &tombstone_retention_d_tag(30177, &record.pubkey)
    )
    .unwrap()
    .is_some());
    assert_eq!(
        serde_json::to_value(record).unwrap(),
        source,
        "guard never rewrites source keys/settings"
    );
}
#[test]
fn exact_managed_deletion_preserves_new_head_local_edits_and_future_reconcile() {
    use crate::managed_agents::{agent_events::build_agent_event, retention::*};
    let dir = tempfile::tempdir().unwrap();
    let conn = open_retention_db(&dir.path().join("retention.sqlite")).unwrap();
    let owner = nostr::Keys::generate();
    let mut record = local_agent();
    record.pubkey = nostr::Keys::generate().public_key().to_hex();
    let old = build_agent_event(&record)
        .unwrap()
        .custom_created_at(nostr::Timestamp::from(100))
        .sign_with_keys(&owner)
        .unwrap();
    let deletion = exact_managed_delete(&owner, &record.pubkey, &old);
    record.name = "Concurrent new definition".into();
    let new = build_agent_event(&record)
        .unwrap()
        .custom_created_at(nostr::Timestamp::from(200))
        .sign_with_keys(&owner)
        .unwrap();
    retain_exact_delete_head(&conn, &new, &record.pubkey, false);
    let before = serde_json::to_value(&record).unwrap();
    assert_eq!(
        exact_managed_deletion::retain_tombstone(
            &conn,
            &deletion,
            30177,
            &record.pubkey,
            &[record.clone()]
        )
        .unwrap(),
        InboundOutcome::Skipped
    );
    let owner_hex = owner.public_key().to_hex();
    let tombstone = tombstone_retention_d_tag(30177, &record.pubkey);
    assert!(
        get_retained_event(&conn, 5, &owner_hex, &tombstone)
            .unwrap()
            .is_none(),
        "stale exact deletion is not remembered as coordinate-wide suppression"
    );
    assert_eq!(
        get_retained_event(&conn, 30177, &owner_hex, &record.pubkey)
            .unwrap()
            .unwrap()
            .raw_event,
        nostr::JsonUtil::as_json(&new)
    );
    let current_delete = exact_managed_delete(&owner, &record.pubkey, &new);
    let mut local_edit = record.clone();
    local_edit.respond_to = crate::managed_agents::RespondTo::Anyone;
    assert_eq!(
        exact_managed_deletion::retain_tombstone(
            &conn,
            &current_delete,
            30177,
            &record.pubkey,
            &[local_edit]
        )
        .unwrap(),
        InboundOutcome::Skipped
    );
    retain_exact_delete_head(&conn, &new, &record.pubkey, true);
    assert_eq!(
        exact_managed_deletion::retain_tombstone(
            &conn,
            &current_delete,
            30177,
            &record.pubkey,
            &[record.clone()]
        )
        .unwrap(),
        InboundOutcome::Skipped
    );
    assert!(get_retained_event(&conn, 5, &owner_hex, &tombstone)
        .unwrap()
        .is_none());
    assert_eq!(serde_json::to_value(&record).unwrap(), before);
    // Other coordinate-only kinds retain existing NIP09 routing/retention.
    let persona = deletion_event_with_keys(&format!("30175:{owner_hex}:persona"), &owner);
    assert!(!exact_managed_deletion::needs_head(&persona, 30175));
    assert_eq!(
        exact_managed_deletion::retain_tombstone(&conn, &persona, 30175, "persona", &[]).unwrap(),
        InboundOutcome::Applied
    );
    // The a parser stays unchanged; the signed cached-head resolver handles e-only separately.
    let e_only = nostr::EventBuilder::new(nostr::Kind::Custom(5), "")
        .tags([nostr::Tag::parse(["e", &new.id.to_hex()]).unwrap()])
        .sign_with_keys(&owner)
        .unwrap();
    assert!(parse_deletion_coordinate(&e_only).is_none());
}

#[test]
fn e_only_managed_deletion_resolves_verified_current_cache_and_never_guesses_evicted_id() {
    use crate::managed_agents::{agent_events::build_agent_event, retention::*};
    use nostr::JsonUtil;
    let dir = tempfile::tempdir().unwrap();
    let conn = open_retention_db(&dir.path().join("retention.sqlite")).unwrap();
    let owner = nostr::Keys::generate();
    let mut record = local_agent();
    record.pubkey = nostr::Keys::generate().public_key().to_hex();
    let old = build_agent_event(&record)
        .unwrap()
        .custom_created_at(nostr::Timestamp::from(100))
        .sign_with_keys(&owner)
        .unwrap();
    let deletion = |head: &nostr::Event, signer: &nostr::Keys| {
        nostr::EventBuilder::new(nostr::Kind::Custom(5), "")
            .tags([nostr::Tag::parse(["e", &head.id.to_hex()]).unwrap()])
            .custom_created_at(nostr::Timestamp::from(300))
            .sign_with_keys(signer)
            .unwrap()
    };
    retain_exact_delete_head(&conn, &old, &record.pubkey, false);
    let current = deletion(&old, &owner);
    assert_eq!(
        exact_managed_deletion::coordinate(&conn, &current).unwrap(),
        Some((30177, record.pubkey.clone()))
    );
    assert_eq!(
        exact_managed_deletion::retain_tombstone(
            &conn,
            &current,
            30177,
            &record.pubkey,
            &[record.clone()]
        )
        .unwrap(),
        InboundOutcome::Applied
    );
    // Independent fresh DB, with old ID genuinely replaced/evicted by newer head.
    let next = open_retention_db(&dir.path().join("next.sqlite")).unwrap();
    retain_exact_delete_head(&next, &old, &record.pubkey, false);
    record.name = "New owner revision survives".into();
    let newer = build_agent_event(&record)
        .unwrap()
        .custom_created_at(nostr::Timestamp::from(200))
        .sign_with_keys(&owner)
        .unwrap();
    retain_exact_delete_head(&next, &newer, &record.pubkey, false);
    let original = serde_json::to_value(&record).unwrap();
    assert_eq!(
        exact_managed_deletion::coordinate(&next, &current).unwrap(),
        None,
        "old e ID has no current signed locator; do not guess its actor"
    );
    assert!(get_retained_event(
        &next,
        5,
        &owner.public_key().to_hex(),
        &tombstone_retention_d_tag(30177, &record.pubkey)
    )
    .unwrap()
    .is_none());
    assert_eq!(
        get_retained_event(&next, 30177, &owner.public_key().to_hex(), &record.pubkey)
            .unwrap()
            .unwrap()
            .raw_event,
        newer.as_json()
    );
    let correct = deletion(&newer, &owner);
    let foreign = deletion(&newer, &nostr::Keys::generate());
    assert_eq!(
        exact_managed_deletion::coordinate(&next, &foreign).unwrap(),
        None
    );
    retain_exact_delete_head(&next, &newer, &record.pubkey, true);
    assert_eq!(
        exact_managed_deletion::coordinate(&next, &correct).unwrap(),
        None,
        "pending local state remains protected"
    );
    let mut forged = newer.clone();
    forged.content = "tampered".into();
    retain_exact_delete_head(&next, &forged, &record.pubkey, false);
    assert_eq!(
        exact_managed_deletion::coordinate(&next, &correct).unwrap(),
        None,
        "unsigned/tampered cache cannot locate an actor"
    );
    retain_exact_delete_head(&next, &newer, &"ab".repeat(32), false);
    assert!(
        exact_managed_deletion::coordinate(&next, &correct).is_err(),
        "wrong actor coordinate cannot become a locator"
    );
    assert_eq!(serde_json::to_value(&record).unwrap(), original);
    assert!(get_retained_event(
        &next,
        5,
        &owner.public_key().to_hex(),
        &tombstone_retention_d_tag(30177, &record.pubkey)
    )
    .unwrap()
    .is_none());
}
