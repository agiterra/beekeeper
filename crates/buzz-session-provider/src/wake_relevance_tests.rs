//! The four drop rules of [`still_owed`], one at a time, plus the pointer
//! readers the dequeue side depends on.

use super::*;

fn fact(id: &str, author: Option<&str>, created_at: Option<u64>, reopens: bool) -> WakeFact {
    WakeFact {
        fact_id: id.to_owned(),
        author: author.map(str::to_owned),
        created_at,
        reopens_work: reopens,
    }
}

fn view(recipient: &str) -> RelevanceView {
    RelevanceView {
        recipient: recipient.to_owned(),
        ..RelevanceView::default()
    }
}

#[test]
fn a_foreign_unconsumed_fact_on_an_open_umbrella_is_owed() {
    assert_eq!(
        still_owed(&fact("aa", Some("builder"), Some(10), false), &view("lead")),
        Ok(())
    );
}

#[test]
fn the_recipients_own_fact_is_self_authored() {
    assert_eq!(
        still_owed(&fact("aa", Some("lead"), Some(10), true), &view("lead")),
        Err(WakeDropReason::SelfAuthored)
    );
}

#[test]
fn a_fact_the_recipient_cites_is_already_consumed() {
    let mut view = view("lead");
    view.cited_by_recipient.insert("aa".into());
    assert_eq!(
        still_owed(&fact("aa", Some("host"), None, true), &view),
        Err(WakeDropReason::AlreadyConsumed)
    );
}

#[test]
fn a_delivered_fact_is_a_duplicate() {
    let mut view = view("lead");
    view.delivered.insert("aa".into());
    assert_eq!(
        still_owed(&fact("aa", Some("builder"), Some(1), true), &view),
        Err(WakeDropReason::Duplicate)
    );
}

#[test]
fn after_a_terminal_only_newer_contesting_facts_are_owed() {
    let mut view = view("lead");
    view.terminal = Some(TerminalMark {
        event_id: "tt".into(),
        created_at: 100,
    });
    // Bookkeeping, whenever it was signed.
    assert_eq!(
        still_owed(&fact("r", Some("builder"), Some(200), false), &view),
        Err(WakeDropReason::PostTerminal)
    );
    // Contesting, but already known when the lead completed.
    assert_eq!(
        still_owed(&fact("d", Some("verifier"), Some(99), true), &view),
        Err(WakeDropReason::PostTerminal)
    );
    // Contesting and newer, or of unprovable order: owed.
    assert_eq!(
        still_owed(&fact("d", Some("verifier"), Some(101), true), &view),
        Ok(())
    );
    assert_eq!(
        still_owed(&fact("d", Some("verifier"), Some(100), true), &view),
        Ok(())
    );
    assert_eq!(still_owed(&fact("h", None, None, true), &view), Ok(()));
}

#[test]
fn cited_by_reads_every_event_id_the_author_signed_and_nothing_else() {
    let lead = nostr::Keys::generate();
    let other = nostr::Keys::generate();
    let cited = "ab".repeat(32);
    let foreign = "cd".repeat(32);
    let own = nostr::EventBuilder::new(
        nostr::Kind::Custom(1),
        serde_json::json!({"body": {"evidenceRefs": [{"eventId": cited}]}}).to_string(),
    )
    .sign_with_keys(&lead)
    .expect("sign");
    let theirs = nostr::EventBuilder::new(
        nostr::Kind::Custom(1),
        serde_json::json!({"reportRef": foreign}).to_string(),
    )
    .sign_with_keys(&other)
    .expect("sign");
    let ids = cited_by(&[own, theirs], &lead.public_key().to_hex());
    assert!(ids.contains(&cited));
    assert!(
        !ids.contains(&foreign),
        "another author's citations are not the lead's"
    );
}

#[test]
fn a_host_result_pointer_reopens_only_when_it_did_not_exit_clean() {
    let pointer = |disposition: &str, exit: i64| {
        serde_json::json!({
            "schema": crate::host_result_wake::HOST_RESULT_WAKE_SCHEMA,
            "type": crate::host_result_wake::HOST_RESULT_WAKE_TYPE,
            "runId": "run-1",
            "stepId": "verify",
            "disposition": disposition,
            "resultEventId": "ef".repeat(32),
            "exitCode": exit,
        })
        .to_string()
    };
    let clean = fact_of_pointer(&pointer("exited", 0), &[]).expect("a pointer");
    assert_eq!(clean.fact_id, "ef".repeat(32));
    assert!(!clean.reopens_work);
    let failed = fact_of_pointer(&pointer("exited", 1), &[]).expect("a pointer");
    assert!(failed.reopens_work);
    assert!(fact_of_pointer("prose", &[]).is_none());
}

#[test]
fn every_drop_reason_has_its_stable_slug() {
    assert_eq!(
        [
            WakeDropReason::AlreadyConsumed,
            WakeDropReason::SelfAuthored,
            WakeDropReason::PostTerminal,
            WakeDropReason::Duplicate,
        ]
        .map(WakeDropReason::as_str),
        [
            "already_consumed",
            "self_authored",
            "post_terminal",
            "duplicate"
        ]
    );
}
