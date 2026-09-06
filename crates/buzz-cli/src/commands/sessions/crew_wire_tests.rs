//! Tests for the two 44220/44221 envelope rules `bee sessions` must honour:
//! the relay's exactly-three-tags validator, and the delivery class that is
//! omitted at its default because a relay predating the field refuses it.

use buzz_core::coding_session_command::{
    coding_session_target_key, CodingSessionAction, CodingSessionCommandPayload,
    CodingSessionDelivery, CodingSessionTarget, CODING_SESSION_COMMAND_SCHEMA,
};
use buzz_core::coding_session_payload::{
    LifecycleReceipt, ReceiptError, ReceiptStatus, LIFECYCLE_RECEIPT_SCHEMA,
};
use buzz_sdk::builders::build_coding_session_lifecycle_receipt;
use buzz_sdk::kind::KIND_CODING_SESSION_LIFECYCLE_RECEIPT;

use super::crew_cmds::{
    boundary_free_content, build_turn_command, classify_create_receipts, cmd_create,
    coding_session_navigation_url, refuse_unsupported_create_flags, CreateWaitOutcome,
};
use crate::client::BuzzClient;
use crate::error::CliError;

const CHANNEL: &str = "9a1657ac-f7aa-5db0-b632-d8bbeb6dfb50";
const ALICE: &str = "11";

fn target(session_id: &str, generation: u64) -> CodingSessionTarget {
    CodingSessionTarget {
        driver: "claude-agent-acp".into(),
        instance_id: "instance-1".into(),
        session_id: session_id.into(),
        generation,
    }
}

fn pk(seed: &str) -> String {
    seed.repeat(32)
}

fn usage_message(error: CliError) -> String {
    match error {
        CliError::Usage(message) | CliError::NotFound(message) => message,
        other => panic!("expected a usage/not-found error, got {other:?}"),
    }
}

fn receipt_event(
    keys: &nostr::Keys,
    command_id: &str,
    status: ReceiptStatus,
    target: Option<CodingSessionTarget>,
) -> serde_json::Value {
    let receipt = LifecycleReceipt {
        schema: LIFECYCLE_RECEIPT_SCHEMA.to_owned(),
        command_id: command_id.to_owned(),
        status,
        session: target,
        error: matches!(status, ReceiptStatus::Failed).then(|| ReceiptError {
            code: "PROVIDER_UNAVAILABLE".into(),
            message: "provider refused the create".into(),
        }),
        turn_id: None,
    };
    let event = build_coding_session_lifecycle_receipt(
        uuid::Uuid::parse_str(CHANNEL).expect("channel"),
        command_id,
        &serde_json::to_string(&receipt).expect("receipt"),
    )
    .expect("receipt builder")
    .sign_with_keys(keys)
    .expect("receipt sign");
    assert_eq!(
        u32::from(event.kind.as_u16()),
        KIND_CODING_SESSION_LIFECYCLE_RECEIPT
    );
    serde_json::to_value(event).expect("event json")
}

// ── wire shape ───────────────────────────────────────────────────────────────
fn payload(text: &str, deliver: CodingSessionDelivery) -> CodingSessionCommandPayload {
    CodingSessionCommandPayload {
        schema: CODING_SESSION_COMMAND_SCHEMA.to_owned(),
        command_id: "cmd-1".to_owned(),
        target: target("s-1", 2),
        action: CodingSessionAction::ThreadTurnStart {
            text: text.to_owned(),
            attachments: Vec::new(),
            deliver,
        },
    }
}

/// A relay built before `deliver` existed refuses any payload carrying it —
/// the payload is `deny_unknown_fields` — so the default class is omitted,
/// exactly as the desktop sender does.
#[test]
fn a_boundary_turn_omits_the_deliver_key() {
    let content =
        boundary_free_content(&payload("go", CodingSessionDelivery::Boundary)).expect("content");
    assert!(!content.contains("deliver"), "got {content}");
    let decoded: CodingSessionCommandPayload =
        serde_json::from_str(&content).expect("round-trips through the relay's own decoder");
    assert_eq!(decoded, payload("go", CodingSessionDelivery::Boundary));
}

/// The same omission rule covers attachments, and `boundary_free_content`'s
/// string surgery must survive them: it strips the one `deliver` key by
/// literal match, so a payload that now carries another optional key has to
/// still come out decodable.
#[test]
fn a_turn_without_images_carries_no_attachments_key() {
    let content =
        boundary_free_content(&payload("go", CodingSessionDelivery::Boundary)).expect("content");
    assert!(!content.contains("attachments"), "got {content}");
}

#[test]
fn a_turn_with_images_keeps_them_through_the_boundary_rewrite() {
    let mut with_images = payload("look at this", CodingSessionDelivery::Boundary);
    with_images.action = CodingSessionAction::ThreadTurnStart {
        text: "look at this".to_owned(),
        attachments: vec![buzz_core::coding_session_command::TurnAttachment {
            sha256: "aa0011223344556677889900aabbccddeeff00112233445566778899aabbccdd".to_owned(),
            mime: "image/png".to_owned(),
            size: 2048,
            dim: Some("800x600".to_owned()),
            filename: Some("shot.png".to_owned()),
        }],
        deliver: CodingSessionDelivery::Boundary,
    };
    let content = boundary_free_content(&with_images).expect("content");
    assert!(!content.contains("deliver"), "got {content}");
    let decoded: CodingSessionCommandPayload =
        serde_json::from_str(&content).expect("round-trips through the relay's own decoder");
    assert_eq!(decoded, with_images);
}

#[test]
fn a_steer_turn_keeps_the_deliver_key() {
    let content =
        boundary_free_content(&payload("go", CodingSessionDelivery::Steer)).expect("content");
    assert!(content.contains(r#""deliver":"steer""#), "got {content}");
}

/// The removal is a string edit, so prove it cannot be fooled by turn text
/// that spells the needle: JSON escapes every quote inside a string value.
#[test]
fn turn_text_that_spells_the_deliver_key_is_left_intact() {
    let hostile = r#"look for ,"deliver":"boundary" in the payload"#;
    let content =
        boundary_free_content(&payload(hostile, CodingSessionDelivery::Boundary)).expect("content");
    let decoded: CodingSessionCommandPayload = serde_json::from_str(&content).expect("decodes");
    match decoded.action {
        CodingSessionAction::ThreadTurnStart { text, deliver, .. } => {
            assert_eq!(text, hostile);
            assert_eq!(deliver, CodingSessionDelivery::Boundary);
        }
        other => panic!("expected a turn start, got {other:?}"),
    }
}

/// The relay's 44220 envelope validator accepts exactly one `h`, `cs-v`, and
/// `cs-target` tag; the boundary path rebuilds them, so pin that it rebuilds
/// them identically to the SDK.
#[test]
fn the_boundary_envelope_carries_the_same_tags_as_the_sdk_builder() {
    let channel = uuid::Uuid::parse_str(CHANNEL).expect("uuid");
    let keys = nostr::Keys::generate();
    let ours = build_turn_command(channel, &payload("go", CodingSessionDelivery::Boundary))
        .expect("builder")
        .sign_with_keys(&keys)
        .expect("sign");
    let sdk = buzz_sdk::builders::build_coding_session_command(
        channel,
        &payload("go", CodingSessionDelivery::Boundary),
    )
    .expect("sdk builder")
    .sign_with_keys(&keys)
    .expect("sign");

    let tags = |event: &nostr::Event| -> Vec<Vec<String>> {
        event
            .tags
            .iter()
            .map(|tag| tag.as_slice().to_vec())
            .collect()
    };
    assert_eq!(tags(&ours), tags(&sdk));
    assert_eq!(ours.kind, sdk.kind);
    // The one intended difference, and nothing else.
    assert_eq!(
        ours.content,
        sdk.content.replace(r#","deliver":"boundary""#, "")
    );
}

#[test]
fn a_steer_turn_is_built_by_the_sdk_unchanged() {
    let channel = uuid::Uuid::parse_str(CHANNEL).expect("uuid");
    let keys = nostr::Keys::generate();
    let ours = build_turn_command(channel, &payload("go", CodingSessionDelivery::Steer))
        .expect("builder")
        .sign_with_keys(&keys)
        .expect("sign");
    let sdk = buzz_sdk::builders::build_coding_session_command(
        channel,
        &payload("go", CodingSessionDelivery::Steer),
    )
    .expect("sdk builder")
    .sign_with_keys(&keys)
    .expect("sign");
    assert_eq!(ours.content, sdk.content);
    assert_eq!(ours.id, sdk.id);
}

// ── create refusals ──────────────────────────────────────────────────────────

#[test]
fn create_refuses_an_actor_and_says_where_seats_come_from() {
    let message =
        usage_message(refuse_unsupported_create_flags(Some(&pk(ALICE)), None, None).unwrap_err());
    assert!(message.contains("ACTOR_UNAVAILABLE"), "got {message}");
    assert!(message.contains("desktop"), "got {message}");
}

#[test]
fn create_refuses_a_role_because_a_role_is_half_of_a_pair() {
    let message =
        usage_message(refuse_unsupported_create_flags(None, Some("builder"), None).unwrap_err());
    assert!(message.contains("ACTOR_ROLE_PAIR"), "got {message}");
}

#[test]
fn create_refuses_a_driver_because_the_provider_mints_it() {
    let message =
        usage_message(refuse_unsupported_create_flags(None, None, Some("codex-acp")).unwrap_err());
    assert!(message.contains("--provider-instance"), "got {message}");
}

#[test]
fn create_accepts_the_unseated_flag_set() {
    assert!(refuse_unsupported_create_flags(None, None, None).is_ok());
}

#[test]
fn create_wait_keeps_same_channel_roots_on_distinct_targets() {
    let provider = nostr::Keys::generate();
    let authority = provider.public_key().to_hex();
    let first = receipt_event(
        &provider,
        "create-a",
        ReceiptStatus::Created,
        Some(target("session-a", 1)),
    );
    let second = receipt_event(
        &provider,
        "create-b",
        ReceiptStatus::Created,
        Some(target("session-b", 1)),
    );
    let first_answer = classify_create_receipts(
        &[first.clone(), second.clone()],
        CHANNEL,
        "create-a",
        &authority,
    )
    .expect("first receipt should classify")
    .expect("first receipt should be present");
    let second_answer = classify_create_receipts(&[first, second], CHANNEL, "create-b", &authority)
        .expect("second receipt should classify")
        .expect("second receipt should be present");
    assert!(matches!(
        first_answer,
        CreateWaitOutcome::Confirmed { target, .. } if target.session_id == "session-a"
    ));
    assert!(matches!(
        second_answer,
        CreateWaitOutcome::Confirmed { target, .. } if target.session_id == "session-b"
    ));
}

#[test]
fn create_wait_ignores_wrong_signer_and_unrelated_receipts() {
    let provider = nostr::Keys::generate();
    let stranger = nostr::Keys::generate();
    let authority = provider.public_key().to_hex();
    let valid = receipt_event(
        &provider,
        "create-a",
        ReceiptStatus::Created,
        Some(target("session-a", 1)),
    );
    let wrong_signer = receipt_event(
        &stranger,
        "create-a",
        ReceiptStatus::Created,
        Some(target("stranger", 1)),
    );
    let unrelated = receipt_event(
        &provider,
        "other-command",
        ReceiptStatus::Created,
        Some(target("other", 1)),
    );
    let result = classify_create_receipts(
        &[wrong_signer, unrelated, valid.clone(), valid],
        CHANNEL,
        "create-a",
        &authority,
    )
    .expect("unrelated evidence must not fail the wait")
    .expect("valid provider evidence should remain");
    assert!(matches!(
        result,
        CreateWaitOutcome::Confirmed { target, .. } if target.session_id == "session-a"
    ));
}

#[test]
fn create_wait_surfaces_provider_refusal_and_no_confirmation() {
    let provider = nostr::Keys::generate();
    let authority = provider.public_key().to_hex();
    let refused = receipt_event(&provider, "create-a", ReceiptStatus::Failed, None);
    let result = classify_create_receipts(&[refused], CHANNEL, "create-a", &authority)
        .expect("refusal should classify")
        .expect("refusal should be present");
    assert!(matches!(result, CreateWaitOutcome::Refused { .. }));
    assert_eq!(
        classify_create_receipts(&[], CHANNEL, "create-a", &authority)
            .expect("empty evidence is not an error"),
        None
    );
}

#[test]
fn create_wait_refuses_conflicting_targets_in_same_command() {
    let provider = nostr::Keys::generate();
    let authority = provider.public_key().to_hex();
    let first = receipt_event(
        &provider,
        "create-a",
        ReceiptStatus::Created,
        Some(target("session-a", 1)),
    );
    let second = receipt_event(
        &provider,
        "create-a",
        ReceiptStatus::Created,
        Some(target("session-b", 1)),
    );
    let error = classify_create_receipts(&[first, second], CHANNEL, "create-a", &authority)
        .expect_err("two target answers must not be guessed between");
    assert!(error.contains("conflicting lifecycle receipts"), "{error}");
}

#[tokio::test]
async fn create_wait_uses_one_publish_and_exact_receipt_query() {
    use axum::body::Bytes;
    use axum::extract::State;
    use axum::response::Json;
    use axum::routing::post;
    use axum::Router;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct Records {
        published: Vec<nostr::Event>,
        queries: Vec<serde_json::Value>,
        query_attempts: HashMap<String, u32>,
    }

    #[derive(Clone)]
    struct Relay {
        provider: nostr::Keys,
        records: Arc<Mutex<Records>>,
    }

    let relay = Relay {
        provider: nostr::Keys::generate(),
        records: Arc::new(Mutex::new(Records::default())),
    };
    let provider_authority = relay.provider.public_key().to_hex();
    let app = Router::new()
        .route(
            "/events",
            post(|State(relay): State<Relay>, body: Bytes| async move {
                let event: nostr::Event = serde_json::from_slice(&body).expect("event JSON");
                let event_id = event.id.to_hex();
                relay.records.lock().expect("records").published.push(event);
                Json(serde_json::json!({
                    "event_id": event_id,
                    "accepted": true,
                    "message": ""
                }))
            }),
        )
        .route(
            "/query",
            post(|State(relay): State<Relay>, body: Bytes| async move {
                let filters: Vec<serde_json::Value> =
                    serde_json::from_slice(&body).expect("filter JSON");
                let filter = filters.first().expect("one filter").clone();
                let command_id = filter["#csl-command"][0]
                    .as_str()
                    .expect("exact command tag")
                    .to_owned();
                let (attempt, publication_index) = {
                    let mut records = relay.records.lock().expect("records");
                    records.queries.push(filter);
                    let attempt = {
                        let entry = records
                            .query_attempts
                            .entry(command_id.clone())
                            .or_default();
                        *entry += 1;
                        *entry
                    };
                    let publication_index = records
                        .published
                        .iter()
                        .position(|event| {
                            event.tags.iter().any(|tag| {
                                tag.as_slice().len() == 2
                                    && tag.as_slice()[0] == "csl-command"
                                    && tag.as_slice()[1] == command_id
                            })
                        })
                        .expect("query follows its publish");
                    (attempt, publication_index)
                };

                // First create is immediate, second is delayed by one empty
                // catch-up query, and third never gets a receipt.
                let should_answer =
                    publication_index == 0 || (publication_index == 1 && attempt >= 2);
                let events = if should_answer {
                    vec![receipt_event(
                        &relay.provider,
                        &command_id,
                        ReceiptStatus::Created,
                        Some(target(&format!("recorded-{publication_index}"), 1)),
                    )]
                } else {
                    Vec::new()
                };
                Json(serde_json::json!(events))
            }),
        )
        .with_state(relay.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let url = format!("http://{}", listener.local_addr().expect("address"));
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("recording relay");
    });
    let client = BuzzClient::new(url, nostr::Keys::generate(), None, None).expect("client");

    async fn create(client: &BuzzClient, provider_authority: &str) -> Result<(), CliError> {
        cmd_create(
            client,
            &CHANNEL.to_ascii_uppercase(),
            None,
            None,
            "provider-primary",
            provider_authority,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            true,
            Some(1),
        )
        .await
    }

    assert!(
        create(&client, &provider_authority).await.is_ok(),
        "immediate receipt should confirm"
    );
    assert!(
        create(&client, &provider_authority).await.is_ok(),
        "delayed receipt should confirm"
    );
    let timeout = create(&client, &provider_authority)
        .await
        .expect_err("third create must time out");
    assert!(matches!(timeout, CliError::Unconfirmed(message) if
        message.contains("receipt queries failed or did not complete")));

    let records = relay.records.lock().expect("records");
    assert_eq!(
        records.published.len(),
        3,
        "wait must never publish a retry"
    );
    assert!(
        records.queries.len() >= 4,
        "immediate + delayed + timeout queries"
    );
    let command_ids: std::collections::HashSet<_> = records
        .published
        .iter()
        .map(|event| {
            event
                .tags
                .iter()
                .find(|tag| tag.as_slice().first().map(String::as_str) == Some("csl-command"))
                .expect("command tag")
                .as_slice()[1]
                .clone()
        })
        .collect();
    for filter in &records.queries {
        assert_eq!(filter["kinds"], serde_json::json!([44224]));
        assert_eq!(filter["#h"], serde_json::json!([CHANNEL]));
        let queried = filter["#csl-command"][0]
            .as_str()
            .expect("command filter value");
        assert!(
            command_ids.contains(queried),
            "query must target its create"
        );
        assert!(
            filter.get("since").is_none(),
            "command tag closes the race without clock bounds"
        );
    }
    let first_target = coding_session_target_key(&target("recorded-0", 1));
    let session_url = coding_session_navigation_url(CHANNEL, &provider_authority, &first_target);
    let parsed = url::Url::parse(&session_url).expect("session navigation URL");
    let query: std::collections::HashMap<_, _> = parsed.query_pairs().into_owned().collect();
    assert_eq!(query.get("channel"), Some(&CHANNEL.to_owned()));
    assert_eq!(query.get("provider"), Some(&provider_authority));
    assert_eq!(query.get("target"), Some(&first_target));
    assert!(session_url.contains("target=coding-session%2Fv1%7C"));
    drop(records);
    server.abort();
}
