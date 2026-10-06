//! Conditional publication through the real CLI client against a local HTTP stub.

use std::sync::{Arc, Mutex};

use axum::{http::StatusCode, routing::post, Router};
use clap::Parser;
use serde_json::{json, Value};

use super::{cmd_set_source_conditionally, PackSourceCondition, PackSourcePin};
use crate::{client::BeekeeperClient, error::CliError};

type Events = Arc<Mutex<Vec<nostr::Event>>>;

async fn relay(
    status: StatusCode,
    message: &str,
) -> (BeekeeperClient, Events, tokio::task::JoinHandle<()>) {
    let events = Events::default();
    let captured = events.clone();
    let message = message.to_owned();
    let app = Router::new().route(
        "/events",
        post(move |body: String| {
            let captured = captured.clone();
            let message = message.clone();
            async move {
                let event: nostr::Event = serde_json::from_str(&body).expect("signed event");
                event.verify().expect("valid signature");
                captured.lock().expect("events").push(event);
                (status, json!({"error": message}).to_string())
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let url = format!("http://{}", listener.local_addr().expect("address"));
    let task = tokio::spawn(async move { axum::serve(listener, app).await.expect("serve") });
    let client = BeekeeperClient::new(url, nostr::Keys::generate(), None, None).expect("client");
    (client, events, task)
}

fn coordinates(client: &BeekeeperClient) -> (String, String) {
    let owner = client.keys().public_key().to_hex();
    (
        format!("30621:{owner}:project"),
        format!("30617:{owner}:packs"),
    )
}

#[tokio::test]
async fn publication_sends_v1_or_explicit_v2_conditions_in_one_signed_event() {
    for (condition, expected) in [
        (PackSourceCondition::Unconditional, None),
        (PackSourceCondition::IfUnset, Some(Value::Null)),
        (
            PackSourceCondition::Expected(
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            ),
            Some(json!("a".repeat(64))),
        ),
    ] {
        let (client, events, task) = relay(StatusCode::OK, "").await;
        let (project, repo) = coordinates(&client);
        let mut argv = vec![
            "bee",
            "packs",
            "set-source",
            "--project",
            &project,
            "--repo",
            &repo,
            "--ref",
            "refs/heads/main",
            "--note",
            "checked",
        ];
        match condition {
            PackSourceCondition::Unconditional => {}
            PackSourceCondition::IfUnset => argv.push("--if-unset"),
            PackSourceCondition::Expected(id) => argv.extend(["--expected-source", id]),
        }
        let cli = crate::Cli::try_parse_from(argv).expect("arguments");
        let crate::Cmd::Packs(command) = cli.command else {
            panic!("packs command");
        };
        let result = crate::commands::packs_cli::dispatch(command, &client).await;
        task.abort();
        result.expect("published");
        let recorded = events.lock().expect("events");
        assert_eq!(recorded.len(), 1);
        let event = &recorded[0];
        assert_eq!(event.kind.as_u16(), 30624);
        let content: Value = serde_json::from_str(&event.content).expect("source content");
        assert_eq!(content["note"], "checked");
        match expected {
            None => {
                assert_eq!(content["schema"], "buzz-project-pack-source/v1");
                assert!(content.get("expectedSourceId").is_none());
            }
            Some(expected) => {
                assert_eq!(content["schema"], "buzz-project-pack-source/v2");
                assert_eq!(content["expectedSourceId"], expected);
            }
        }
        assert_eq!(
            event.tags.iter().next().expect("d tag").as_slice(),
            ["d", project.as_str()]
        );
    }
}

#[tokio::test]
async fn publication_conflicts_exit_five_without_retry_and_auth_stays_distinct() {
    for (status, message, code, conflict) in [
        (
            StatusCode::CONFLICT,
            "conflict: PACK_SOURCE_CONFLICT: expected source changed",
            5,
            true,
        ),
        (StatusCode::CONFLICT, "some other conflict", 2, false),
        (
            StatusCode::FORBIDDEN,
            "conflict: PACK_SOURCE_CONFLICT: unauthorized",
            3,
            false,
        ),
        (
            StatusCode::BAD_REQUEST,
            "unsupported pack source schema version",
            2,
            false,
        ),
    ] {
        let (client, events, task) = relay(status, message).await;
        let (project, repo) = coordinates(&client);
        let result = cmd_set_source_conditionally(
            &client,
            &project,
            &repo,
            &PackSourcePin {
                ref_name: Some("refs/heads/main"),
                sha: None,
            },
            None,
            None,
            PackSourceCondition::IfUnset,
        )
        .await;
        task.abort();
        let error = result.expect_err("refused");
        assert_eq!(crate::error::exit_code(&error), code);
        assert_eq!(matches!(&error, CliError::Conflict(_)), conflict);
        assert!(error.to_string().contains(message));
        assert!(!crate::error::is_retryable_error(&error));
        assert_eq!(
            events.lock().expect("events").len(),
            1,
            "no replacement or retry"
        );
    }
}

#[tokio::test]
async fn malformed_expected_source_is_refused_before_publication() {
    let (client, events, task) = relay(StatusCode::OK, "").await;
    let (project, repo) = coordinates(&client);
    let result = cmd_set_source_conditionally(
        &client,
        &project,
        &repo,
        &PackSourcePin {
            ref_name: Some("refs/heads/main"),
            sha: None,
        },
        None,
        None,
        PackSourceCondition::Expected("not-an-event-id"),
    )
    .await;
    task.abort();
    assert!(matches!(result, Err(CliError::Usage(_))));
    assert!(events.lock().expect("events").is_empty());
}
