use super::*;
use nostr::{EventBuilder, JsonUtil, Keys, Timestamp};
use std::sync::Mutex;

struct Client {
    keys: Keys,
    url: String,
    delegated: bool,
    path: std::path::PathBuf,
    sent: Mutex<Vec<Event>>,
    rows: Mutex<Vec<Value>>,
    mode: u8, // 0 normal, 1 accepted/lost ACK, 2 lost before acceptance, 3 ACK without readback
}
impl RelayTransport for Client {
    fn keys(&self) -> &Keys {
        &self.keys
    }
    fn relay_url(&self) -> &str {
        &self.url
    }
    fn verified_auth_tag_owner_hex(&self) -> Result<Option<String>, Error> {
        Ok(self.delegated.then(|| "a".repeat(64)))
    }
    fn scoped(&self, _: Keys, _: String) -> Result<Self, Error> {
        unreachable!()
    }
    async fn query_all(&self, filter: Value) -> Result<Vec<Value>, Error> {
        assert_eq!(filter["kinds"], json!([44226]));
        assert!(filter["ids"].is_array() && filter["#h"].is_array());
        Ok(self.rows.lock().unwrap().clone())
    }
    async fn submit_event(&self, event: Event) -> Result<Value, Error> {
        let saved: Operation = serde_json::from_slice(&std::fs::read(&self.path).unwrap()).unwrap();
        assert_eq!(
            saved.event.as_json(),
            event.as_json(),
            "durable before publication"
        );
        self.sent.lock().unwrap().push(event.clone());
        if self.mode < 2 {
            *self.rows.lock().unwrap() = vec![serde_json::to_value(&event).unwrap()];
        }
        if self.mode == 1 || self.mode == 2 {
            return Err(Error::Other("lost ACK".into()));
        }
        if self.mode == 4 {
            return Ok(json!({"accepted":false,"message":"restricted: not a channel member"}));
        }
        if self.mode == 5 {
            return Ok(json!({"accepted":false,"message":"duplicate: already stored"}));
        }
        Ok(json!({"accepted":true,"event_id":event.id.to_hex()}))
    }
}
fn fixture(mode: u8) -> (tempfile::TempDir, Context, FoundRequest, Client) {
    let dir = tempfile::tempdir().unwrap();
    let context = Context {
        home: dir.path().into(),
        instance: beekeeper_host_core::layout::Instance::Production,
        native_home: None,
    };
    let args = FoundRequest {
        operation_id: Uuid::new_v4(),
        channel: Uuid::new_v4(),
        session_ref: Uuid::new_v4().to_string(),
    };
    let path = local::operation_path(&context, args.operation_id).unwrap();
    let client = Client {
        keys: Keys::generate(),
        url: "https://fixture.invalid".into(),
        delegated: false,
        path,
        sent: Mutex::new(vec![]),
        rows: Mutex::new(vec![]),
        mode,
    };
    (dir, context, args, client)
}
#[tokio::test]
async fn normal_found_is_canonical_persisted_and_read_back_without_execution() {
    let (_dir, context, args, client) = fixture(0);
    let output = found(&args, &client, &context).await.unwrap();
    assert_eq!(output["accepted"], true);
    assert_eq!(output["evidence"], "exact_signed_readback");
    let event: Event = serde_json::from_value(output["event"].clone()).unwrap();
    event.verify().unwrap();
    assert_eq!(
        event.content,
        format!(r#"{{"sessionRef":"{}","v":1}}"#, args.session_ref)
    );
    assert_eq!(
        serde_json::to_value(&event.tags).unwrap(),
        json!([
            ["h", args.channel.to_string()],
            ["csg-v", "csg1-1"],
            ["csg-session", args.session_ref]
        ])
    );
    let before = std::fs::read(&client.path).unwrap();
    assert_eq!(found(&args, &client, &context).await.unwrap(), output);
    assert_eq!(client.sent.lock().unwrap().len(), 1);
    assert_eq!(std::fs::read(&client.path).unwrap(), before);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&client.path)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}
#[tokio::test]
async fn lost_ack_resolves_only_by_exact_signed_readback() {
    let (_dir, context, args, client) = fixture(1);
    assert!(found(&args, &client, &context).await.is_ok());
    assert_eq!(client.sent.lock().unwrap().len(), 1);
}
#[tokio::test]
async fn uncertain_retry_reuses_every_signed_byte_after_reopening_journal() {
    let (_dir, context, args, mut client) = fixture(2);
    assert!(matches!(
        found(&args, &client, &context).await,
        Err(Error::Unconfirmed(_))
    ));
    let first = client.sent.lock().unwrap()[0].as_json();
    client.mode = 0;
    assert!(found(&args, &client, &context).await.is_ok());
    let sent = client.sent.lock().unwrap();
    assert_eq!(sent.len(), 2);
    assert_eq!(first, sent[1].as_json());
}
#[tokio::test]
async fn ack_without_readback_is_unconfirmed() {
    let (_dir, context, args, client) = fixture(3);
    assert!(matches!(
        found(&args, &client, &context).await,
        Err(Error::Unconfirmed(_))
    ));
    let operation: Operation =
        serde_json::from_slice(&std::fs::read(&client.path).unwrap()).unwrap();
    assert!(!operation.observed);
}
#[tokio::test]
async fn conflicting_intent_signer_and_relay_preserve_original_without_publication() {
    let (_dir, context, args, mut client) = fixture(0);
    found(&args, &client, &context).await.unwrap();
    let before = std::fs::read(&client.path).unwrap();
    let mut changed = args.clone();
    changed.channel = Uuid::new_v4();
    assert!(matches!(
        found(&changed, &client, &context).await,
        Err(Error::Conflict(_))
    ));
    changed = args.clone();
    changed.session_ref = Uuid::new_v4().to_string();
    assert!(matches!(
        found(&changed, &client, &context).await,
        Err(Error::Conflict(_))
    ));
    client.url = "https://another.invalid".into();
    assert!(matches!(
        found(&args, &client, &context).await,
        Err(Error::Conflict(_))
    ));
    client.url = "https://fixture.invalid".into();
    client.keys = Keys::generate();
    assert!(matches!(
        found(&args, &client, &context).await,
        Err(Error::Conflict(_))
    ));
    assert_eq!(std::fs::read(&client.path).unwrap(), before);
    assert_eq!(client.sent.lock().unwrap().len(), 1);
}
#[tokio::test]
async fn delegated_and_malformed_inputs_refuse_before_journaling() {
    let (_dir, context, mut args, mut client) = fixture(0);
    client.delegated = true;
    assert!(matches!(
        found(&args, &client, &context).await,
        Err(Error::Auth(_))
    ));
    client.delegated = false;
    args.session_ref = "ABCD".into();
    assert!(matches!(
        found(&args, &client, &context).await,
        Err(Error::Usage(_))
    ));
    args.session_ref = Uuid::new_v4().to_string();
    args.operation_id = Uuid::nil();
    assert!(matches!(
        found(&args, &client, &context).await,
        Err(Error::Usage(_))
    ));
    assert!(!client.path.exists());
    assert!(client.sent.lock().unwrap().is_empty());
}
#[tokio::test]
async fn forged_readback_and_tampered_journal_cannot_authorize_success_or_republish() {
    let (_dir, context, args, client) = fixture(2);
    assert!(found(&args, &client, &context).await.is_err());
    let mut operation: Operation =
        serde_json::from_slice(&std::fs::read(&client.path).unwrap()).unwrap();
    let mut forged = serde_json::to_value(&operation.event).unwrap();
    forged["content"] = json!("{}");
    *client.rows.lock().unwrap() = vec![forged];
    assert!(matches!(
        found(&args, &client, &context).await,
        Err(Error::Unconfirmed(_))
    ));
    operation.event = beekeeper_sdk::build_coding_session_genesis(
        Uuid::new_v4(),
        &CodingSessionGenesisPayload::new(args.session_ref.clone()),
    )
    .unwrap()
    .sign_with_keys(&client.keys)
    .unwrap();
    local::save(&client.path, &operation).unwrap();
    assert!(matches!(
        found(&args, &client, &context).await,
        Err(Error::Auth(_))
    ));
    assert_eq!(client.sent.lock().unwrap().len(), 1);
}
#[test]
fn producer_bytes_match_existing_conformance_envelope() {
    let keys = Keys::generate();
    let channel = Uuid::parse_str("00000000-0000-4000-8000-000000000001").unwrap();
    let session = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
    let expected = EventBuilder::new(
        nostr::Kind::Custom(44226),
        format!(r#"{{"sessionRef":"{session}","v":1}}"#),
    )
    .tags(
        [
            ["h", &channel.to_string()],
            ["csg-v", "csg1-1"],
            ["csg-session", session],
        ]
        .map(|t| nostr::Tag::parse(t).unwrap()),
    )
    .custom_created_at(Timestamp::from(1793318400))
    .sign_with_keys(&keys)
    .unwrap();
    let actual = beekeeper_sdk::build_coding_session_genesis(
        channel,
        &CodingSessionGenesisPayload::new(session),
    )
    .unwrap()
    .custom_created_at(expected.created_at)
    .sign_with_keys(&keys)
    .unwrap();
    let vectors: Value = serde_json::from_str(include_str!(
        "../../../conformance/coding-session-records/44226-genesis/fixtures/vectors.json"
    ))
    .unwrap();
    let fresh = vectors["vectors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["name"] == "fresh-founding")
        .unwrap();
    assert_eq!(
        actual.content,
        serde_json::to_string(&fresh["content"]).unwrap()
    );
    assert_eq!(actual.id, expected.id);
    assert_eq!(actual.content.as_bytes(), expected.content.as_bytes());
    assert_eq!(actual.tags, expected.tags);
}

#[tokio::test]
async fn explicit_refusal_is_distinct_from_unverified_duplicate() {
    let (_dir, context, args, client) = fixture(4);
    assert!(matches!(
        found(&args, &client, &context).await,
        Err(Error::Refused(_))
    ));
    assert!(client.path.exists());
    let (_dir, context, args, client) = fixture(5);
    assert!(matches!(
        found(&args, &client, &context).await,
        Err(Error::Unconfirmed(_))
    ));
}

#[tokio::test]
async fn corrupt_or_symlink_journals_are_not_overwritten() {
    let (_dir, context, args, client) = fixture(0);
    std::fs::write(&client.path, b"{broken").unwrap();
    assert!(found(&args, &client, &context).await.is_err());
    assert_eq!(std::fs::read(&client.path).unwrap(), b"{broken");
    assert!(client.sent.lock().unwrap().is_empty());
    #[cfg(unix)]
    {
        let target = client.path.with_extension("target");
        std::fs::write(&target, b"keep").unwrap();
        std::fs::remove_file(&client.path).unwrap();
        std::os::unix::fs::symlink(&target, &client.path).unwrap();
        assert!(found(&args, &client, &context).await.is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"keep");
    }
}

#[tokio::test]
async fn copied_journal_cannot_satisfy_a_different_operation_uuid() {
    let (_dir, context, args, client) = fixture(0);
    found(&args, &client, &context).await.unwrap();
    let before = std::fs::read(&client.path).unwrap();
    let mut other = args.clone();
    other.operation_id = Uuid::new_v4();
    let copied = local::operation_path(&context, other.operation_id).unwrap();
    std::fs::copy(&client.path, &copied).unwrap();
    assert!(matches!(
        found(&other, &client, &context).await,
        Err(Error::Conflict(_))
    ));
    assert_eq!(std::fs::read(&copied).unwrap(), before);
    assert_eq!(std::fs::read(&client.path).unwrap(), before);
    assert_eq!(client.sent.lock().unwrap().len(), 1);
}
