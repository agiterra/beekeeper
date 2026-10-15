//! Conformance vector for the pluggable runtime descriptor
//! (`plans/HEADLESS_PLUGGABLE_SLICE.md`, Step 1).
//!
//! A runtime this build has never heard of — `acme`, driven by a stub ACP
//! adapter named `acme-acp` — is commissioned through the descriptor alone,
//! started with file custody restricted to the variables the descriptor
//! names, and answers one turn through the provider. Nothing in the path
//! checks the runtime's name: the profile is the generic managed one, the
//! capabilities are the baseline vector, and the credential loader reads only
//! `ACME_TOKEN`. The second test is the refusal the same loader owes a
//! descriptor that names no variables.
//!
//! Deliberately a child of `runtime_credentials` rather than of the crate's
//! `tests` module, so it reuses only the harness pieces that module exports
//! (`provider`, `write_projects`, `test_operator_keys`, `fake_agent`) and
//! carries its own copies of the three small private helpers it needs.

use std::sync::Mutex;
use std::time::Duration;

use beekeeper_core::coding_session_command::CodingSessionTarget;
use beekeeper_core::coding_session_payload::Capabilities;
use beekeeper_core::coding_session_runtime::RuntimeDescriptor;
use beekeeper_core::kind::{
    KIND_CODING_SESSION_COMMAND, KIND_CODING_SESSION_LIFECYCLE_COMMAND,
    KIND_CODING_SESSION_LIFECYCLE_RECEIPT, KIND_CODING_SESSION_METADATA,
};
use nostr::{Event, EventBuilder, Kind, Tag};
use uuid::Uuid;

use crate::publish::EventSink;
use crate::session::testing::fake_agent;
use crate::session::{SessionEvent, TurnOutcome};
use crate::tests::{provider, test_operator_keys, write_projects};
use crate::Provider;

/// A stub ACP agent that answers `initialize` honestly — no session load, no
/// steering — opens one session, and answers one prompt. It records the
/// value of `ACME_TOKEN` it was started with into the session's own checkout
/// (the one tree the boundary lets it write), so the test can prove the
/// descriptor's `credentialEnv` reached the adapter without that value ever
/// appearing in a published event.
///
/// The log's path is baked in absolute (`ACME_ENV_LOG` is replaced by
/// [`rig`]) rather than written relative to the adapter's working directory:
/// this vector is about the credential reaching the adapter, and a relative
/// path would make it also a test of where the launch starts the child —
/// which `session::tests::the_legacy_launch_starts_the_adapter_in_the_session_checkout`
/// pins on its own (Woodpecker pipeline 322 on main 6122557e4 failed here
/// for exactly that reason, before the legacy launch set a directory at all).
const ACME_ACP: &str = r#"
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":2,"agentCapabilities":{"loadSession":false}}}\n' "$id" ;;
    *'"method":"session/new"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"sessionId":"acme-session-1"}}\n' "$id" ;;
    *'"method":"session/prompt"'*)
      printf '%s\n' "${ACME_TOKEN:-unset}" > 'ACME_ENV_LOG'
      printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"acme-session-1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"acme answered"}}}}\n'
      printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$id" ;;
  esac
done
"#;

const ACME_INSTANCE: &str = "acme-primary";
const ACME_DRIVER: &str = "acme-acp";
const ACME_RUNTIME: &str = "acme";
const ACME_SOURCE: &str = "acme-vault";
const ACME_SECRET: &str = "acme-secret-token";

#[derive(Default)]
struct CollectingSink {
    events: Mutex<Vec<Event>>,
}

impl EventSink for CollectingSink {
    async fn publish(&self, event: Event) -> Result<(), String> {
        self.events.lock().expect("lock").push(event);
        Ok(())
    }
}

impl CollectingSink {
    fn contents_of(&self, kind: u32) -> Vec<serde_json::Value> {
        self.events
            .lock()
            .expect("lock")
            .iter()
            .filter(|event| u32::from(event.kind.as_u16()) == kind)
            .map(|event| serde_json::from_str(&event.content).expect("json content"))
            .collect()
    }

    fn all(&self) -> Vec<Event> {
        self.events.lock().expect("lock").clone()
    }
}

/// The one descriptor under test: an unknown runtime word, an unknown
/// driver slug, a stub adapter run through `bash`, and the two new fields.
fn acme_descriptor(agent_script: String, credential_env: Vec<String>) -> RuntimeDescriptor {
    RuntimeDescriptor {
        instance_ref: ACME_INSTANCE.into(),
        driver: ACME_DRIVER.into(),
        runtime: ACME_RUNTIME.into(),
        agent_command: "bash".into(),
        agent_args: vec![agent_script],
        cli_env: None,
        default_model: "acme-default".into(),
        allowed_models: vec!["acme-default".into()],
        discover_models: false,
        capabilities: None,
        steer_idle_guard: None,
        title_model: Some(None),
        credential_env,
        credential_source: Some(ACME_SOURCE.into()),
    }
}

struct Rig {
    _dir: tempfile::TempDir,
    channel_id: Uuid,
    env_log: std::path::PathBuf,
    provider: Provider,
}

/// A provider whose only runtime is `acme`, with a credential file holding
/// `ACME_TOKEN` for it, and the real driver-to-profile mapping (not the
/// fixtures' test double) so the launch path is the one production takes.
fn rig(credential_env: Vec<String>) -> Rig {
    let dir = tempfile::tempdir().expect("tempdir");
    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let channel_id = Uuid::new_v4();
    let projects = write_projects(dir.path(), channel_id, &cwd);
    let state = dir.path().join("state");
    let seed = provider(&state, Some(&projects));
    let mut config = seed.config.clone();
    drop(seed);

    let credentials = dir.path().join("acme-credentials.json");
    crate::state::atomic_write(
        &credentials,
        serde_json::json!({ "runtimes": { ACME_INSTANCE: { "ACME_TOKEN": ACME_SECRET } } })
            .to_string()
            .as_bytes(),
    )
    .expect("private credential file");
    crate::state::atomic_write(
        &state.join("runtime-credentials-source.json"),
        serde_json::json!({ "path": credentials })
            .to_string()
            .as_bytes(),
    )
    .expect("credential pointer");

    let env_log = cwd.join("acme-env.log");
    let script = fake_agent(
        dir.path(),
        ACME_DRIVER,
        &ACME_ACP.replace("ACME_ENV_LOG", &env_log.display().to_string()),
    );
    config.runtimes = vec![acme_descriptor(script, credential_env)];
    config.runtime_profile_override = None;
    let provider = Provider::new(config).expect("provider");
    Rig {
        _dir: dir,
        channel_id,
        env_log,
        provider,
    }
}

fn signed(channel_id: Uuid, kind: u32, content: String) -> Event {
    EventBuilder::new(Kind::Custom(kind as u16), content)
        .tags(vec![
            Tag::parse(["h", &channel_id.to_string()]).expect("tag")
        ])
        .sign_with_keys(test_operator_keys())
        .expect("sign")
}

fn create_event(provider: &Provider, channel_id: Uuid, command_id: &str) -> Event {
    let content = serde_json::json!({
        "schema": "buzz-coding-session-lifecycle-command/v1",
        "commandId": command_id,
        "action": {
            "type": "session.create",
            "projectRef": null,
            "repoRef": null,
            "providerInstanceRef": ACME_INSTANCE,
            "providerAuthorityPubkey": provider.config.pubkey_hex(),
            "model": null,
            "title": "Plugged in",
            "initialTurn": null,
        },
    })
    .to_string();
    signed(channel_id, KIND_CODING_SESSION_LIFECYCLE_COMMAND, content)
}

fn turn_event(channel_id: Uuid, command_id: &str, target: &CodingSessionTarget) -> Event {
    let content = serde_json::json!({
        "schema": "buzz-coding-session-command/v1",
        "commandId": command_id,
        "target": target,
        "action": { "type": "thread.turn.start", "text": "say hello" },
    })
    .to_string();
    signed(channel_id, KIND_CODING_SESSION_COMMAND, content)
}

/// Drain session reports into the provider until the turn ends, returning
/// how it ended.
async fn pump_until_turn_finished(provider: &mut Provider) -> TurnOutcome {
    loop {
        let event = tokio::time::timeout(Duration::from_secs(20), provider.next_session_event())
            .await
            .expect("session event within timeout")
            .expect("channel open");
        let finished = match &event {
            SessionEvent::TurnFinished { outcome, .. } => Some(outcome.clone()),
            _ => None,
        };
        provider.handle_session_event(event).expect("record");
        if let Some(outcome) = finished {
            return outcome;
        }
    }
}

/// The vector: commissioned by descriptor, started under file custody
/// restricted to `credentialEnv`, one turn answered, baseline capabilities
/// advertised, and no name check anywhere in the path.
#[tokio::test]
async fn an_unknown_runtime_with_a_stub_acp_adapter_is_started_and_answers_a_turn() {
    let Rig {
        _dir,
        channel_id,
        env_log,
        mut provider,
    } = rig(vec!["ACME_TOKEN".into()]);

    provider
        .handle_command_event(
            channel_id,
            &create_event(&provider, channel_id, "acme-create-1"),
        )
        .await
        .expect("handle create");
    let sink = CollectingSink::default();
    provider.flush(&sink).await.expect("flush");

    let receipts = sink.contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT);
    assert_eq!(receipts.len(), 1, "{receipts:?}");
    assert_eq!(
        receipts[0]["status"], "created",
        "the create was refused: {}",
        receipts[0]
    );
    assert_eq!(receipts[0]["session"]["driver"], ACME_DRIVER);

    let metadata = sink.contents_of(KIND_CODING_SESSION_METADATA);
    assert_eq!(metadata.len(), 1, "{metadata:?}");
    assert_eq!(metadata[0]["provider"], ACME_INSTANCE);
    assert_eq!(metadata[0]["runtime"], ACME_RUNTIME);
    assert_eq!(metadata[0]["model"], "acme-default");
    // Baseline capabilities for a runtime no table knows: start and
    // interrupt, nothing claimed that was never proven for it.
    let baseline = serde_json::to_value(Capabilities::v1_baseline()).expect("baseline");
    assert_eq!(metadata[0]["capabilities"], baseline);

    let record = provider.state().sessions().next().expect("session record");
    assert_eq!(record.provider_instance_ref, ACME_INSTANCE);
    assert_eq!(record.runtime, ACME_RUNTIME);
    let target = record.target("instance-1");

    provider
        .handle_command_event(channel_id, &turn_event(channel_id, "acme-turn-1", &target))
        .await
        .expect("handle turn");
    let outcome = pump_until_turn_finished(&mut provider).await;
    assert!(
        matches!(outcome, TurnOutcome::Completed { .. }),
        "the stub adapter's turn did not complete: {outcome:?}"
    );

    // The adapter was started with exactly the credential the descriptor
    // named, read from the file at start...
    let seen = std::fs::read_to_string(&env_log).expect("the adapter recorded its environment");
    assert_eq!(seen.trim(), ACME_SECRET);
    // ...and that value never crossed the wire.
    provider.flush(&sink).await.expect("flush");
    for event in sink.all() {
        assert!(
            !event.content.contains(ACME_SECRET),
            "a credential value leaked into a signed event: {}",
            event.content
        );
    }

    // The operator label travels with the descriptor the session ran under
    // and is echoed into the 44223 metadata by `Provider::session_metadata`.
    assert_eq!(
        provider
            .config
            .runtime(ACME_INSTANCE)
            .and_then(|descriptor| descriptor.credential_source.as_deref()),
        Some(ACME_SOURCE)
    );
    assert_eq!(
        metadata[0]["credentialSource"], ACME_SOURCE,
        "the descriptor's credentialSource must reach the signed metadata: {}",
        metadata[0]
    );
}

/// The refusal side of the plug: file custody is configured, the runtime
/// word has no built-in variable list, and the descriptor names none. The
/// create fails with the existing code and a message that names the fix —
/// it is never started against an ambient login.
#[tokio::test]
async fn an_unknown_runtime_that_names_no_credential_variables_is_refused_by_name_of_the_fix() {
    let Rig {
        _dir,
        channel_id,
        env_log,
        mut provider,
    } = rig(Vec::new());

    provider
        .handle_command_event(
            channel_id,
            &create_event(&provider, channel_id, "acme-create-2"),
        )
        .await
        .expect("handle create");
    let sink = CollectingSink::default();
    provider.flush(&sink).await.expect("flush");

    let receipts = sink.contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT);
    assert_eq!(receipts.len(), 1, "{receipts:?}");
    assert_eq!(receipts[0]["status"], "failed", "{}", receipts[0]);
    assert_eq!(receipts[0]["error"]["code"], "MODEL_AUTH_UNAVAILABLE");
    let message = receipts[0]["error"]["message"].as_str().expect("message");
    assert!(message.contains("credentialEnv"), "{message}");
    assert!(message.contains("--credential-env"), "{message}");
    assert_eq!(provider.state().sessions().count(), 0);
    assert!(!env_log.exists(), "no adapter may have been started");
}
