//! Focused R1 checks: exact addressing, lifecycle wire, and signed outcomes.
use beekeeper_core::coding_session_command::{coding_session_target_key, CodingSessionTarget};
use beekeeper_core::coding_session_payload::LifecycleReceipt;
use beekeeper_sdk::builders::{
    build_coding_session_lifecycle_command, build_coding_session_lifecycle_receipt,
};
use clap::Parser;
use nostr::Keys;
use serde_json::{json, Value};
use uuid::Uuid;

use super::crew_cmds::ReceiptWait;
use super::resume::{classify_resume_receipts, parse_target, resume_payload, resume_report};
use crate::error::CliError;

const CHANNEL: &str = "d3e440ea-89f8-4aee-8a02-17edc3e7272e";
const COMMAND: &str = "resume-test";
fn target(generation: u64) -> CodingSessionTarget {
    CodingSessionTarget {
        driver: "t3-acp".into(),
        instance_id: "test-provider".into(),
        session_id: "session-1".into(),
        generation,
    }
}
fn signed(keys: &Keys, channel: &str, receipt: &LifecycleReceipt) -> Value {
    let event = build_coding_session_lifecycle_receipt(
        Uuid::parse_str(channel).unwrap(),
        &receipt.command_id,
        &serde_json::to_string(receipt).unwrap(),
    )
    .unwrap()
    .sign_with_keys(keys)
    .unwrap();
    serde_json::to_value(event).unwrap()
}

#[test]
fn resume_arguments_and_exact_target() {
    let key = coding_session_target_key(&target(1));
    let args = crate::Cli::try_parse_from([
        "bee",
        "sessions",
        "resume",
        "--channel",
        CHANNEL,
        "--to",
        &key,
    ])
    .unwrap();
    assert!(matches!(
        args.command,
        crate::Cmd::Sessions(crate::SessionsCmd::Resume(_))
    ));
    assert_eq!(parse_target(&key).unwrap(), target(1));
    for bad in [
        "builder",
        "session-1",
        "",
        "coding-session/v1|1:x",
        &format!("{key} "),
    ] {
        assert!(parse_target(bad).is_err(), "{bad}");
    }
    assert!(
        crate::Cli::try_parse_from(["bee", "sessions", "resume", "--channel", CHANNEL]).is_err()
    );
    assert!(crate::Cli::try_parse_from([
        "bee",
        "sessions",
        "resume",
        "--channel",
        CHANNEL,
        "--to",
        &key,
        "--resume-cursor",
        "secret"
    ])
    .is_err());
}

#[test]
fn resume_wire_contains_only_the_existing_operator_contract() {
    let keys = Keys::generate();
    let provider = keys.public_key().to_hex();
    let payload = resume_payload(COMMAND, &target(1), &provider);
    let event = build_coding_session_lifecycle_command(Uuid::parse_str(CHANNEL).unwrap(), &payload)
        .unwrap()
        .sign_with_keys(&keys)
        .unwrap();
    assert_eq!(event.kind.as_u16(), 44221);
    assert_eq!(
        serde_json::from_str::<Value>(&event.content).unwrap(),
        json!({
            "schema": "buzz-coding-session-lifecycle-command/v1", "commandId": COMMAND,
            "action": { "type": "session.resume", "session": target(1), "providerAuthorityPubkey": provider }
        })
    );
    let tags: Vec<_> = event
        .tags
        .iter()
        .map(|tag| tag.as_slice().to_vec())
        .collect();
    assert_eq!(
        tags,
        vec![
            vec!["h", CHANNEL],
            vec!["csl-v", "csl1-1"],
            vec!["csl-command", COMMAND]
        ]
    );
}

#[test]
fn resume_requires_signed_matching_receipt_and_exact_successor() {
    let keys = Keys::generate();
    let provider = keys.public_key().to_hex();
    let receipt = LifecycleReceipt::resumed(COMMAND, &target(2));
    let valid = signed(&keys, CHANNEL, &receipt);
    let classify = |events: &[Value]| {
        classify_resume_receipts(events, CHANNEL, COMMAND, &provider, &target(1))
    };
    assert!(classify(std::slice::from_ref(&valid)).unwrap().is_some());
    let mut tampered = valid;
    tampered["content"] = json!("tampered");
    for event in [
        tampered,
        signed(&Keys::generate(), CHANNEL, &receipt),
        signed(&keys, "00000000-0000-4000-8000-000000000001", &receipt),
        signed(
            &keys,
            CHANNEL,
            &LifecycleReceipt::resumed("other", &target(2)),
        ),
        signed(
            &keys,
            CHANNEL,
            &LifecycleReceipt::created(COMMAND, &target(2)),
        ),
    ] {
        assert!(classify(&[event]).unwrap().is_none());
    }
    for wrong in [
        target(1),
        target(3),
        CodingSessionTarget {
            session_id: "other".into(),
            ..target(2)
        },
    ] {
        assert!(classify(&[signed(
            &keys,
            CHANNEL,
            &LifecycleReceipt::resumed(COMMAND, &wrong)
        )])
        .is_err());
    }
    assert!(classify(&[
        signed(&keys, CHANNEL, &receipt),
        signed(
            &keys,
            CHANNEL,
            &LifecycleReceipt::failed(COMMAND, "STALE_GENERATION", "stale")
        ),
    ])
    .is_err());
}

#[test]
fn resume_preserves_runtime_context_disclosure_and_provider_status() {
    let keys = Keys::generate();
    let message = "Beekeeper restored no prior context for this generation, but this runtime keeps its own conversation across sessions and may still hold it";
    for receipt in [
        LifecycleReceipt::resumed(COMMAND, &target(2)),
        LifecycleReceipt::resumed_without_context(COMMAND, &target(2), message),
    ] {
        let answer = classify_resume_receipts(
            &[signed(&keys, CHANNEL, &receipt)],
            CHANNEL,
            COMMAND,
            &keys.public_key().to_hex(),
            &target(1),
        )
        .unwrap()
        .unwrap();
        let (report, result) = resume_report(ReceiptWait::Answered(answer), COMMAND, 30);
        assert!(result.is_ok());
        assert_eq!(report["outcome"], receipt.status.as_str());
        assert_eq!(report["session"], coding_session_target_key(&target(2)));
        assert_eq!(report["receipt"], serde_json::to_value(receipt).unwrap());
        assert!(report["summary"]
            .as_str()
            .unwrap()
            .contains("not proof of native context restoration"));
    }
}

#[test]
fn resume_refusals_and_timeout_never_claim_success() {
    let keys = Keys::generate();
    for code in [
        "SESSION_ALREADY_ATTACHED",
        "SESSION_STOPPED",
        "STALE_GENERATION",
        "MODEL_AUTH_FAILED",
        "SESSION_QUIESCENCE_UNCONFIRMED",
    ] {
        let receipt = LifecycleReceipt::failed(COMMAND, code, "provider reason");
        let answer = classify_resume_receipts(
            &[signed(&keys, CHANNEL, &receipt)],
            CHANNEL,
            COMMAND,
            &keys.public_key().to_hex(),
            &target(1),
        )
        .unwrap()
        .unwrap();
        let (report, result) = resume_report(ReceiptWait::Answered(answer), COMMAND, 30);
        assert!(matches!(result, Err(CliError::Refused(_))));
        assert_eq!(report["outcome"], "refused");
        assert_eq!(report["receipt"]["error"]["code"], code);
        assert!(report["session"].is_null());
    }
    for query_failed in [false, true] {
        let (report, result) = resume_report(ReceiptWait::Unconfirmed { query_failed }, COMMAND, 1);
        assert!(matches!(result, Err(CliError::Unconfirmed(_))));
        assert_eq!(report["outcome"], "unconfirmed");
        assert!(report["receipt"].is_null());
        assert!(report["session"].is_null());
        assert!(report["summary"].as_str().unwrap().contains(COMMAND));
    }
}

#[test]
fn resume_authority_lookup_keeps_the_observed_generation_and_refuses_ambiguity() {
    use super::crew::build_executions;
    use super::decode_receipts;
    use super::resume::resolve_exact;
    let keys = Keys::generate();
    let other = Keys::generate();
    let mut events = vec![
        signed(
            &keys,
            CHANNEL,
            &LifecycleReceipt::created("create", &target(1)),
        ),
        signed(
            &keys,
            CHANNEL,
            &LifecycleReceipt::resumed("resume-previous", &target(2)),
        ),
    ];
    let key = coding_session_target_key(&target(1));
    let (receipts, _) = decode_receipts(&events);
    let rows = build_executions(&[], &receipts, &[], &Default::default(), 100);
    let resolved = resolve_exact(&rows, &key).unwrap();
    assert_eq!(resolved.target, target(1));
    assert_eq!(resolved.signer, keys.public_key().to_hex());
    assert!(resolve_exact(&rows, &coding_session_target_key(&target(3))).is_err());
    events.push(signed(
        &other,
        CHANNEL,
        &LifecycleReceipt::created("other", &target(1)),
    ));
    let (receipts, _) = decode_receipts(&events);
    let rows = build_executions(&[], &receipts, &[], &Default::default(), 100);
    assert!(resolve_exact(&rows, &key).is_err());
}

/// An in-memory transport: no server, credentials, or external processes.
struct SilentRelay {
    keys: Keys,
    fail_query: bool,
}
impl beekeeper_agent_management::RelayTransport for SilentRelay {
    fn keys(&self) -> &Keys {
        &self.keys
    }
    fn relay_url(&self) -> &str {
        "http://unused.invalid"
    }
    fn verified_auth_tag_owner_hex(
        &self,
    ) -> Result<Option<String>, beekeeper_agent_management::Error> {
        Ok(None)
    }
    fn scoped(&self, _: Keys, _: String) -> Result<Self, beekeeper_agent_management::Error> {
        panic!("resume receipt wait must not scope another identity")
    }
    async fn query_all(
        &self,
        filter: Value,
    ) -> Result<Vec<Value>, beekeeper_agent_management::Error> {
        assert_eq!(
            filter,
            json!({ "kinds": [44224], "#h": [CHANNEL], "#csl-command": [COMMAND] })
        );
        if self.fail_query {
            Err(beekeeper_agent_management::Error::Other("offline".into()))
        } else {
            Ok(vec![])
        }
    }
    async fn submit_event(
        &self,
        _: nostr::Event,
    ) -> Result<Value, beekeeper_agent_management::Error> {
        panic!("receipt wait must never resubmit")
    }
}

#[tokio::test]
async fn resume_receipt_deadline_is_unconfirmed_without_resubmitting() {
    use super::crew_cmds::await_command_receipt;
    for fail_query in [false, true] {
        let relay = SilentRelay {
            keys: Keys::generate(),
            fail_query,
        };
        let wait = await_command_receipt(&relay, CHANNEL, COMMAND, 1, |events| {
            classify_resume_receipts(
                events,
                CHANNEL,
                COMMAND,
                &relay.keys.public_key().to_hex(),
                &target(1),
            )
        })
        .await;
        assert!(
            matches!(wait, ReceiptWait::Unconfirmed { query_failed } if query_failed == fail_query)
        );
        let (_, result) = resume_report(wait, COMMAND, 1);
        assert!(matches!(result, Err(CliError::Unconfirmed(_))));
    }
    let (report, result) = resume_report(
        ReceiptWait::Conflicted("contradictory provider answers".into()),
        COMMAND,
        1,
    );
    assert_eq!(report["outcome"], "conflicted");
    assert!(report["session"].is_null());
    assert!(matches!(result, Err(CliError::Refused(_))));
}

#[tokio::test]
async fn resume_lost_responses_retain_reconciliation_identity() {
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let keys = Keys::generate();
    let records = json!([signed(
        &keys,
        CHANNEL,
        &LifecycleReceipt::created("create", &target(1)),
    )])
    .to_string();
    let bodies = Arc::new(Mutex::new(Vec::<Vec<u8>>::new()));
    let captured = bodies.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let (header_end, length) = loop {
                let mut chunk = [0; 4096];
                let count = stream.read(&mut chunk).await.unwrap();
                assert_ne!(count, 0);
                request.extend_from_slice(&chunk[..count]);
                if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&request[..end]);
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().unwrap())
                        })
                        .unwrap();
                    break (end + 4, length);
                }
            };
            while request.len() < header_end + length {
                let mut chunk = [0; 4096];
                let count = stream.read(&mut chunk).await.unwrap();
                assert_ne!(count, 0);
                request.extend_from_slice(&chunk[..count]);
            }
            let response = if request.starts_with(b"POST /query ") {
                format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{records}",
                    records.len()
                )
            } else {
                assert!(request.starts_with(b"POST /events "));
                captured
                    .lock()
                    .unwrap()
                    .push(request[header_end..header_end + length].to_vec());
                // The event reached storage, but the success response body is lost.
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 100\r\nconnection: close\r\n\r\n{\"partial\":".into()
            };
            stream.write_all(response.as_bytes()).await.unwrap();
            stream.shutdown().await.unwrap();
        }
    });
    let client = crate::client::BeekeeperClient::new(url, keys, None, None).unwrap();
    let exact_target = coding_session_target_key(&target(1));
    let args = crate::SessionsResumeArgs {
        channel: CHANNEL.into(),
        to: exact_target.clone(),
        timeout_secs: Some(1),
    };
    let error = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        super::resume::cmd_resume(&client, &args),
    )
    .await
    .unwrap()
    .unwrap_err();
    server.abort();
    assert!(matches!(error, CliError::DeliveryUnknown(_)), "{error:?}");
    assert_eq!(crate::error::exit_code(&error), 2);
    assert!(!crate::error::is_retryable_error(&error));
    let message = error.to_string();
    assert!(message.contains("resume acceptance is unknown"));
    assert!(message.contains("do not retry with a new commandId"));
    let (_, result) = message.split_once("; result: ").unwrap();
    let result: Value = serde_json::from_str(result).unwrap();
    let bodies = bodies.lock().unwrap();
    assert_eq!(
        bodies.len(),
        3,
        "only the three identical-byte transport attempts"
    );
    assert!(bodies.iter().all(|body| body == &bodies[0]));
    let event: nostr::Event = serde_json::from_slice(&bodies[0]).unwrap();
    event.verify().unwrap();
    let payload: Value = serde_json::from_str(&event.content).unwrap();
    assert_eq!(payload["action"]["type"], "session.resume");
    assert_eq!(payload["action"]["session"], json!(target(1)));
    assert_eq!(result["commandId"], payload["commandId"]);
    assert!(result["commandId"].as_str().unwrap().starts_with("resume-"));
    assert_eq!(result["target"], exact_target);
    assert_eq!(result["event_id"], event.id.to_hex());
    assert_eq!(result["outcome"], "delivery_unknown");
    assert!(result["accepted"].is_null());
}

#[test]
fn resume_submission_preserves_other_errors() {
    use super::resume::submission_error;
    for error in [
        CliError::Auth("denied".into()),
        CliError::Conflict("superseded".into()),
        CliError::Relay {
            status: 429,
            body: "busy".into(),
        },
        CliError::Other("invalid response".into()),
    ] {
        let before = (
            format!("{error:?}"),
            crate::error::exit_code(&error),
            crate::error::is_retryable_error(&error),
        );
        let after = submission_error(error, COMMAND, "target", "event");
        assert_eq!(
            before,
            (
                format!("{after:?}"),
                crate::error::exit_code(&after),
                crate::error::is_retryable_error(&after)
            )
        );
    }
}
