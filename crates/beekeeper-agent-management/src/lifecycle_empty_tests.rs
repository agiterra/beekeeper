//! Invisible inventory is neither authority nor proof of termination.
use super::*;

struct VisibilityRelay {
    inner: DisappearingRelay,
    signer: Keys,
    provider: Keys,
    visible: bool,
    outcomes: Mutex<Vec<Event>>,
}
impl RelayTransport for VisibilityRelay {
    fn keys(&self) -> &Keys {
        &self.signer
    }
    fn relay_url(&self) -> &str {
        self.inner.relay_url()
    }
    fn verified_auth_tag_owner_hex(&self) -> Result<Option<String>, Error> {
        Ok(None)
    }
    fn scoped(&self, _: Keys, _: String) -> Result<Self, Error> {
        Err(Error::Other("unused fixture scope".into()))
    }
    async fn relay_identity(&self) -> Result<String, Error> {
        self.inner.relay_identity().await
    }
    async fn query_all(&self, filter: Value) -> Result<Vec<Value>, Error> {
        match filter["kinds"][0].as_u64() {
            Some(44223) => Ok(if self.visible {
                vec![serde_json::to_value(&self.inner.observation).expect("metadata")]
            } else {
                vec![]
            }),
            Some(44224) => Ok(self
                .outcomes
                .lock()
                .expect("outcomes")
                .iter()
                .map(|event| serde_json::to_value(event).expect("receipt"))
                .collect()),
            _ => self.inner.query_all(filter).await,
        }
    }
    async fn submit_event(&self, event: Event) -> Result<Value, Error> {
        assert!(self.visible, "invisible inventory must never publish Stop");
        let payload = beekeeper_core::coding_session_lifecycle_command::decode_coding_session_lifecycle_command(&event.content).expect("stop");
        let CodingSessionLifecycleAction::SessionStop { session, .. } = payload.action else {
            panic!("only Stop expected");
        };
        let receipt = beekeeper_core::coding_session_payload::LifecycleReceipt::stopped(
            &payload.command_id,
            &session,
        );
        let channel = event
            .tags
            .iter()
            .find_map(|tag| {
                let v = tag.as_slice();
                (v.first().map(String::as_str) == Some("h"))
                    .then(|| Uuid::parse_str(&v[1]).expect("channel"))
            })
            .expect("channel");
        let signed = beekeeper_sdk::build_coding_session_lifecycle_receipt(
            channel,
            &payload.command_id,
            &serde_json::to_string(&receipt).expect("receipt"),
        )
        .expect("builder")
        .sign_with_keys(&self.provider)
        .expect("signature");
        self.outcomes.lock().expect("outcomes").push(signed);
        self.inner.submit_event(event).await
    }
}
fn visibility_fixture(visible: bool, owner: bool) -> (VisibilityRelay, LifecycleRequest) {
    let (mut inner, request) = fixture();
    let provider = Keys::generate();
    inner
        .profiles
        .push(attested_profile(&inner.owner, &provider));
    let target = beekeeper_core::coding_session_payload::decode_coding_session_metadata(
        &inner.observation.content,
    )
    .expect("metadata")
    .session;
    inner.observation = beekeeper_sdk::build_coding_session_metadata(
        request.channels[0],
        &target,
        &inner.observation.content,
    )
    .expect("builder")
    .sign_with_keys(&provider)
    .expect("signature");
    let signer = if owner {
        inner.owner.clone()
    } else {
        Keys::generate()
    };
    (
        VisibilityRelay {
            inner,
            signer,
            provider,
            visible,
            outcomes: Mutex::new(vec![]),
        },
        request,
    )
}
fn context(home: &tempfile::TempDir) -> Context {
    Context {
        home: home.path().into(),
        instance: beekeeper_host_core::layout::Instance::Dev,
        native_home: None,
    }
}
#[tokio::test]
async fn ungranted_empty_inventory_refuses_new_and_retained_completed_operation_without_mutation() {
    let (client, request) = visibility_fixture(false, false);
    let home = tempfile::tempdir().expect("home");
    let context = context(&home);
    let path = local::operation_path(&context, request.operation_id).expect("path");
    assert!(matches!(
        stop(&request, &client, &context).await,
        Err(Error::Auth(_))
    ));
    assert!(!path.exists());
    let retained = json!({"request":{"type":"stop","actor":request.agent_pubkey,
        "owner":client.keys().public_key().to_hex(),"relay":client.relay_url(),"channels":request.channels},
        "events":[],"inventory":[],"completed":true,"result":{"terminal":true,"responsibilities":[],"custody":"retained"}});
    local::save(&path, &retained).expect("retained operation");
    let bytes = std::fs::read(&path).expect("bytes");
    for _ in 0..2 {
        assert!(matches!(
            stop(&request, &client, &context).await,
            Err(Error::Auth(_))
        ));
        assert_eq!(std::fs::read(&path).expect("unchanged"), bytes);
    }
    assert!(client.inner.published.lock().expect("published").is_empty());
}
#[tokio::test]
async fn owner_definition_without_responsibility_is_unconfirmed_not_quiescent() {
    let (client, request) = visibility_fixture(false, true);
    let home = tempfile::tempdir().expect("home");
    let context = context(&home);
    assert!(matches!(
        stop(&request, &client, &context).await,
        Err(Error::Unconfirmed(_))
    ));
    assert!(!local::operation_path(&context, request.operation_id)
        .expect("path")
        .exists());
    assert!(client.inner.published.lock().expect("published").is_empty());
}
#[tokio::test]
async fn visible_operator_is_not_owner_gated_and_requires_exact_provider_stopped_receipt() {
    let (client, request) = visibility_fixture(true, false);
    let home = tempfile::tempdir().expect("home");
    let result = stop(&request, &client, &context(&home))
        .await
        .expect("provider accepted Stop");
    assert_eq!(result["terminal"], true);
    assert_eq!(
        result["responsibilities"]
            .as_array()
            .expect("targets")
            .len(),
        1
    );
    let events = client.inner.published.lock().expect("published");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].pubkey, client.signer.public_key());
    assert_ne!(events[0].pubkey, client.inner.owner.public_key());
    assert_eq!(client.outcomes.lock().expect("receipts").len(), 1);
}
