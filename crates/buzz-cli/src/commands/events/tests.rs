//! Tests for `bee events query`'s filter construction and compact rendering.
//!
//! Every case here runs without a relay: the point of this command is that the
//! filter it sends is exactly the filter the caller asked for, and that a
//! malformed event still prints. Both are decided before or after the network,
//! never during it.

use serde_json::json;

use super::*;
use crate::error::exit_code;

fn args<'a>(kinds: &'a str) -> EventsQueryArgs<'a> {
    EventsQueryArgs {
        kinds: Some(kinds),
        ..EventsQueryArgs::default()
    }
}

fn usage_message(error: CliError) -> String {
    match error {
        CliError::Usage(message) => message,
        other => panic!("expected a usage error, got {other:?}"),
    }
}

/// The relay's p-gate answers 403 to a kindless filter, so the CLI refuses it
/// here — with that reason, and with the CLI's bad-input exit code.
#[test]
fn events_query_requires_kinds() {
    for kinds in [None, Some(""), Some("  "), Some(",")] {
        let error = prepare_query(&EventsQueryArgs {
            kinds,
            ..EventsQueryArgs::default()
        })
        .expect_err("a filter with no kinds is refused");
        assert_eq!(
            error.to_string(),
            "--kinds is required: a filter with no kinds is refused by the relay with 403.",
            "kinds = {kinds:?}"
        );
        assert_eq!(exit_code(&error), 1, "kinds = {kinds:?}");
    }
}

/// `--kinds` is documented as required, and its help says why.
#[test]
fn events_query_help_states_why_kinds_is_required() {
    use clap::CommandFactory;
    let mut command = crate::Cli::command();
    let rendered = command.render_long_help().to_string();
    assert!(
        rendered.contains("events"),
        "events is a top-level subcommand"
    );
    let query = crate::Cli::command()
        .find_subcommand("events")
        .expect("events subcommand")
        .find_subcommand("query")
        .expect("query subcommand")
        .clone();
    let about = query
        .get_about()
        .map(ToString::to_string)
        .unwrap_or_default();
    assert_eq!(
        about,
        "Run a raw authenticated REQ against the relay. --kinds is required; \
         the relay's p-gate refuses a filter without it."
    );
}

/// Every flag lands on the exact filter key the relay reads, and `--limit`
/// travels beside the filter rather than inside it — the pager owns that key.
#[test]
fn events_query_builds_the_filter_it_was_asked_for() {
    let author = "a".repeat(64);
    let other = "b".repeat(64);
    let id = "c".repeat(64);
    let prepared = prepare_query(&EventsQueryArgs {
        kinds: Some("44223, 44225"),
        channel: Some("9a1657ac-f7aa-5db0-b632-d8bbeb6dfb50"),
        h: None,
        authors: Some(&format!("{author},{other}")),
        ids: Some(&id),
        since: Some("2026-08-27T00:00:00Z"),
        until: Some("1787875200"),
        limit: Some(20),
    })
    .expect("prepares");

    assert_eq!(
        prepared.filter,
        json!({
            "kinds": [44223, 44225],
            "#h": ["9a1657ac-f7aa-5db0-b632-d8bbeb6dfb50"],
            "authors": [author, other],
            "ids": [id],
            "since": 1_787_788_800,
            "until": 1_787_875_200,
        })
    );
    assert_eq!(prepared.limit, Some(20));
}

/// `--h` writes the same key as `--channel`, for an h-scope that is not a
/// channel UUID; absent `--limit` means "page it all".
#[test]
fn a_raw_h_value_writes_the_same_filter_key_as_a_channel() {
    let prepared = prepare_query(&EventsQueryArgs {
        kinds: Some("44226"),
        h: Some("not-a-uuid-scope"),
        ..EventsQueryArgs::default()
    })
    .expect("prepares");
    assert_eq!(
        prepared.filter,
        json!({ "kinds": [44226], "#h": ["not-a-uuid-scope"] })
    );
    assert_eq!(prepared.limit, None);
}

/// `--channel` is a UUID, checked before the request rather than after a 200
/// with zero rows — an unparseable channel silently matches nothing.
#[test]
fn events_query_refuses_a_malformed_channel_before_any_request() {
    let error = prepare_query(&EventsQueryArgs {
        channel: Some("general"),
        ..args("44223")
    })
    .expect_err("refuses");
    assert!(
        usage_message(error).contains("general"),
        "the refusal names the value it refused"
    );
}

/// A pubkey that is not 64 lowercase hex characters is refused locally.
#[test]
fn events_query_refuses_a_malformed_author_before_any_request() {
    let good = "a".repeat(64);
    for bad in [
        "deadbeef".to_owned(),
        "A".repeat(64),
        format!("{good}0"),
        String::new(),
    ] {
        let authors = format!("{good},{bad}");
        let error = prepare_query(&EventsQueryArgs {
            authors: Some(&authors),
            ..args("44223")
        })
        .expect_err("refuses");
        let message = usage_message(error);
        assert!(
            message.contains("--authors"),
            "the refusal names the flag: {message}"
        );
    }
}

/// The same rule for `--ids`.
#[test]
fn events_query_refuses_a_malformed_id_before_any_request() {
    let error = prepare_query(&EventsQueryArgs {
        ids: Some("not-an-event-id"),
        ..args("44223")
    })
    .expect_err("refuses");
    assert!(usage_message(error).contains("--ids"));
}

/// A kind that is not an integer is a caller mistake, not a relay one.
#[test]
fn events_query_refuses_a_non_numeric_kind() {
    let error = prepare_query(&args("44223,messages")).expect_err("refuses");
    assert!(usage_message(error).contains("messages"));
}

/// `--since`/`--until` take RFC 3339 or Unix seconds; anything else is refused
/// by name.
#[test]
fn events_query_refuses_an_unparseable_timestamp() {
    let error = prepare_query(&EventsQueryArgs {
        since: Some("yesterday"),
        ..args("44223")
    })
    .expect_err("refuses");
    let message = usage_message(error);
    assert!(message.contains("--since"), "{message}");
    assert!(message.contains("yesterday"), "{message}");
}

/// Newest first, broken on id when two events share a second.
#[test]
fn events_are_sorted_newest_first_by_created_at_then_id() {
    let mut events = vec![
        json!({ "id": "aa", "created_at": 100 }),
        json!({ "id": "cc", "created_at": 300 }),
        json!({ "id": "bb", "created_at": 300 }),
        json!({ "id": "dd", "created_at": 200 }),
    ];
    sort_newest_first(&mut events);
    let order: Vec<&str> = events
        .iter()
        .map(|event| event["id"].as_str().expect("id"))
        .collect();
    assert_eq!(order, ["cc", "bb", "dd", "aa"]);
}

/// The compact row is built from the raw event fields only. An event whose
/// content is not JSON — the exact event a debugging query is looking for —
/// still prints a row, with its content as the summary.
#[test]
fn compact_rows_survive_an_event_whose_content_is_not_json() {
    let event = json!({
        "id": "e".repeat(64),
        "pubkey": "f".repeat(64),
        "kind": 44225,
        "created_at": 1_787_788_800_i64,
        "tags": [["h", "9a1657ac-f7aa-5db0-b632-d8bbeb6dfb50"], ["cst-seq", "4"]],
        "content": "{ this is not json\nand never was",
        "sig": "0".repeat(128),
    });
    let row = compact_row(&event);
    assert_eq!(row["id"], json!("e".repeat(64)));
    assert_eq!(row["kind"], json!(44225));
    assert_eq!(row["pubkey"], json!("f".repeat(64)));
    assert_eq!(row["h"], json!("9a1657ac-f7aa-5db0-b632-d8bbeb6dfb50"));
    assert_eq!(row["summary"], json!("{ this is not json and never was"));
    assert_eq!(row["createdAt"], json!("2026-08-27T00:00:00+00:00"));
}

/// An event carrying no `h` tag reports `h: null` rather than being dropped or
/// given a borrowed scope.
#[test]
fn a_compact_row_with_no_h_tag_says_null() {
    let row = compact_row(&json!({
        "id": "1".repeat(64),
        "pubkey": "2".repeat(64),
        "kind": 1,
        "created_at": 10,
        "tags": [],
        "content": "hello",
    }));
    assert_eq!(row["h"], Value::Null);
    assert_eq!(row["summary"], json!("hello"));
}

/// The summary is the first 120 *characters*, with newlines collapsed to
/// single spaces — never bytes, so a multi-byte character cannot split.
#[test]
fn a_summary_is_120_characters_with_newlines_collapsed() {
    assert_eq!(summarize("one\ntwo\r\n\nthree"), "one two three");
    assert_eq!(summarize("  padded \n "), "padded");
    let long = "é".repeat(400);
    let summary = summarize(&long);
    assert_eq!(summary.chars().count(), SUMMARY_CHARS);
    assert_eq!(summary, "é".repeat(SUMMARY_CHARS));
}

/// A compact row over an event with no readable content says so with an empty
/// summary rather than inventing one.
#[test]
fn a_compact_row_without_content_has_a_null_summary() {
    let row = compact_row(&json!({
        "id": "3".repeat(64),
        "pubkey": "4".repeat(64),
        "kind": 1,
        "created_at": 10,
        "tags": [],
    }));
    assert_eq!(row["summary"], Value::Null);
}
