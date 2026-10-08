//! SV-35 provider tests: `thread.model.set` admission, the item → receipt →
//! metadata publish order, refusals that leave metadata alone, and a resume
//! that reopens on the switched model.

use super::*;

/// A cooperative adapter offering `claude-sonnet-4-6` and `opus[1m]` plus an
/// effort select, logging every request to `__LOG__`.
const SWITCHABLE_AGENT: &str = r#"
OPTIONS='[{"value":"claude-sonnet-4-6"},{"value":"opus[1m]"}]'
EFFORT='{"id":"effort","category":"thought_level","type":"select","options":[{"value":"low"},{"value":"high"}]}'
while IFS= read -r line; do
  printf '%s\n' "$line" >> '__LOG__'
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":2}}\n' "$id" ;;
    *'"method":"session/new"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"sessionId":"acp-session-1","configOptions":[{"id":"model","category":"model","type":"select","currentValue":"claude-sonnet-4-6","options":%s},%s]}}\n' "$id" "$OPTIONS" "$EFFORT" ;;
    *'"method":"session/set_config_option"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{}}\n' "$id" ;;
    *'"method":"session/prompt"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$id" ;;
  esac
done
"#;

struct Fixture {
    _dir: tempfile::TempDir,
    provider: Provider,
    channel_id: Uuid,
    target: CodingSessionTarget,
    log: std::path::PathBuf,
}

async fn fixture(switchable: bool) -> Fixture {
    fixture_with(switchable, true).await
}

/// `enabled` is the host's `BUZZ_CSP_MODEL_SWITCH` (`Config::model_switch`).
async fn fixture_with(switchable: bool, enabled: bool) -> Fixture {
    let dir = tempfile::tempdir().expect("tempdir");
    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let channel_id = Uuid::new_v4();
    let projects = write_projects(dir.path(), channel_id, &cwd);
    let state_dir = dir.path().join("state");
    // Inside the checkout: the execution boundary lets the agent write there.
    let log = cwd.join("acp.log");
    let body = if switchable {
        SWITCHABLE_AGENT.replace("__LOG__", &log.to_string_lossy())
    } else {
        session::testing::GOOD_AGENT.to_owned()
    };
    let agent = fake_agent(dir.path(), "model-agent", &body);
    let mut runtime = claude_runtime(agent);
    runtime.allowed_models = vec!["claude-sonnet-4-6".into(), "opus[1m]".into()];
    let mut config =
        config_of_runtimes(Keys::generate(), &state_dir, Some(&projects), vec![runtime]);
    config.model_switch = enabled;
    let mut provider = Provider::new(config).expect("provider");
    provider
        .handle_command_event(channel_id, &create_event(&provider, channel_id, "create-1"))
        .await
        .expect("create");
    let target = provider
        .state()
        .sessions()
        .next()
        .expect("session")
        .target("instance-1");
    Fixture {
        _dir: dir,
        provider,
        channel_id,
        target,
        log,
    }
}

fn switch_event(
    channel_id: Uuid,
    command_id: &str,
    target: &CodingSessionTarget,
    selection: &str,
) -> Event {
    command_event(
        channel_id,
        command_id,
        target,
        serde_json::json!({"type": "thread.model.set", "selection": selection}),
    )
}

/// Every event's (kind, content) in outbox drain order.
fn drained(sink: &CollectingSink) -> Vec<(u32, serde_json::Value)> {
    sink.all()
        .into_iter()
        .map(|event| {
            (
                u32::from(event.kind.as_u16()),
                serde_json::from_str(&event.content).unwrap_or_default(),
            )
        })
        .collect()
}

fn latest_metadata(sink: &CollectingSink) -> serde_json::Value {
    sink.contents_of(KIND_CODING_SESSION_METADATA)
        .into_iter()
        .last()
        .expect("metadata")
}

async fn pump_until_switch(provider: &mut Provider) {
    pump_until(provider, |event| {
        matches!(event, session::SessionEvent::ModelSwitch { .. })
    })
    .await;
}

/// Acceptance: item → receipt → 44223, the same generation's `model` is the
/// applied selection, `modelSwitch` is published, and the record follows.
#[tokio::test]
async fn model_switch_applies_and_publishes_item_receipt_then_metadata() {
    let Fixture {
        _dir,
        mut provider,
        channel_id,
        target,
        log: _,
    } = fixture(true).await;
    let before = CollectingSink::new();
    provider.flush(&before).await.expect("flush");
    assert_eq!(
        latest_metadata(&before)["capabilities"]["modelSwitch"],
        serde_json::json!(true),
        "a switchable execution advertises it"
    );

    provider
        .handle_command_event(
            channel_id,
            &switch_event(channel_id, "model-1", &target, "opus[1m][high]"),
        )
        .await
        .expect("switch");
    pump_until_switch(&mut provider).await;
    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");

    let order = drained(&sink);
    let item = order
        .iter()
        .position(|(kind, content)| {
            *kind == KIND_CODING_SESSION_TRANSCRIPT && content["item"]["status"] == "model_switched"
        })
        .expect("model_switched item");
    let receipt = order
        .iter()
        .position(|(kind, content)| {
            *kind == KIND_CODING_SESSION_LIFECYCLE_RECEIPT && content["commandId"] == "model-1"
        })
        .expect("receipt");
    let metadata = order
        .iter()
        .rposition(|(kind, _)| *kind == KIND_CODING_SESSION_METADATA)
        .expect("metadata");
    assert!(item < receipt && receipt < metadata, "{order:?}");

    // This adapter's set answer carries no effort control, so `[high]` is
    // not applied and is not claimed: asked opus[1m][high], running opus[1m].
    assert_eq!(
        order[item].1["item"],
        serde_json::json!({"kind":"status","status":"model_switched","model":"opus[1m]","requested":"opus[1m][high]","commandId":"model-1"})
    );
    assert_eq!(
        receipt_stages(&sink, "model-1"),
        vec!["model_applied".to_owned()]
    );
    let meta = &order[metadata].1;
    assert_eq!(meta["model"], "opus[1m]");
    assert_eq!(
        meta["session"]["generation"], target.generation,
        "same generation"
    );
    let record = provider
        .state()
        .session(&target.session_id)
        .expect("record");
    assert_eq!(record.model.as_deref(), Some("opus[1m]"));
    assert_eq!(record.model_requested.as_deref(), Some("opus[1m][high]"));
}

/// Acceptance: a resume after a switch reopens on the switched model.
#[tokio::test]
async fn model_switch_survives_a_resume() {
    let Fixture {
        _dir,
        mut provider,
        channel_id,
        target,
        log,
    } = fixture(true).await;
    provider
        .handle_command_event(
            channel_id,
            &switch_event(channel_id, "model-1", &target, "opus[1m]"),
        )
        .await
        .expect("switch");
    pump_until_switch(&mut provider).await;
    provider.sessions.shutdown(&target.session_id);
    let resume =
        lifecycle_target_event(&provider, channel_id, "resume-1", "session.resume", &target);
    provider
        .handle_command_event(channel_id, &resume)
        .await
        .expect("resume");

    let requests: Vec<serde_json::Value> = std::fs::read_to_string(&log)
        .expect("log")
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    let opens: Vec<usize> = requests
        .iter()
        .enumerate()
        .filter(|(_, request)| request["method"] == "session/new")
        .map(|(index, _)| index)
        .collect();
    assert_eq!(opens.len(), 2, "create and resume each opened a session");
    assert!(
        requests[opens[1]..]
            .iter()
            .any(|request| request["method"] == "session/set_config_option"
                && request["params"]["value"] == "opus[1m]"),
        "the resumed process was put on the switched model"
    );
    let record = provider
        .state()
        .session(&target.session_id)
        .expect("record");
    assert_eq!(record.model.as_deref(), Some("opus[1m]"));
}

/// Acceptance: the catalog refusal and the no-control refusal each publish one
/// `turn_refused` and leave metadata's `model` alone; a non-steerer gets the
/// existing authority refusal.
#[tokio::test]
async fn model_switch_refusals_leave_the_model_alone() {
    let Fixture {
        _dir,
        mut provider,
        channel_id,
        target,
        log: _,
    } = fixture(true).await;
    provider
        .handle_command_event(
            channel_id,
            &switch_event(channel_id, "model-x", &target, "gpt-9[high]"),
        )
        .await
        .expect("switch");
    let stranger = Keys::generate();
    provider
        .handle_command_event(
            channel_id,
            &command_event_by(
                channel_id,
                "model-stranger",
                &target,
                serde_json::json!({"type": "thread.model.set", "selection": "opus[1m]"}),
                &stranger,
            ),
        )
        .await
        .expect("stranger");
    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    let code = |command_id: &str| {
        sink.contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
            .into_iter()
            .find(|receipt| receipt["commandId"] == command_id)
            .map(|receipt| (receipt["status"].clone(), receipt["error"]["code"].clone()))
            .expect("answered")
    };
    assert_eq!(
        code("model-x"),
        (
            serde_json::json!("turn_refused"),
            serde_json::json!(payload::MODEL_NOT_OFFERED)
        )
    );
    assert_eq!(
        code("model-stranger"),
        (
            serde_json::json!("turn_refused"),
            serde_json::json!(payload::UNAUTHORIZED_OPERATOR)
        )
    );
    assert_eq!(latest_metadata(&sink)["model"], "claude-sonnet-4-6");

    let Fixture {
        _dir: _bare_dir,
        provider: mut bare,
        channel_id,
        target,
        log: _,
    } = fixture(false).await;
    bare.handle_command_event(
        channel_id,
        &switch_event(channel_id, "model-u", &target, "opus[1m]"),
    )
    .await
    .expect("switch");
    let sink = CollectingSink::new();
    bare.flush(&sink).await.expect("flush");
    let receipt = sink
        .contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
        .into_iter()
        .find(|receipt| receipt["commandId"] == "model-u")
        .expect("answered");
    assert_eq!(receipt["error"]["code"], payload::MODEL_SWITCH_UNSUPPORTED);
    let metadata = latest_metadata(&sink);
    assert!(
        metadata["capabilities"].get("modelSwitch").is_none(),
        "an execution that cannot switch keeps its exact capability bytes: {metadata}"
    );
}

/// The shipped default: with `BUZZ_CSP_MODEL_SWITCH` unset (the host's
/// `Config::model_switch` false), even an adapter that offers a model control
/// keeps its exact capability bytes, and a switch is refused
/// `MODEL_SWITCH_UNSUPPORTED` without reaching the adapter.
#[tokio::test]
async fn model_switch_host_switch_off_hides_the_bit_and_refuses() {
    let Fixture {
        _dir,
        mut provider,
        channel_id,
        target,
        log,
    } = fixture_with(true, false).await;
    provider
        .handle_command_event(
            channel_id,
            &switch_event(channel_id, "model-off", &target, "opus[1m][high]"),
        )
        .await
        .expect("switch");
    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    let receipt = sink
        .contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
        .into_iter()
        .find(|receipt| receipt["commandId"] == "model-off")
        .expect("answered");
    assert_eq!(receipt["status"], "turn_refused");
    assert_eq!(receipt["error"]["code"], payload::MODEL_SWITCH_UNSUPPORTED);
    let metadata = latest_metadata(&sink);
    assert!(
        metadata["capabilities"].get("modelSwitch").is_none(),
        "switch off keeps the exact capability bytes: {metadata}"
    );
    assert_eq!(metadata["model"], "claude-sonnet-4-6");
    let calls = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(
        !calls
            .lines()
            .any(|line| line.contains("session/set_config_option") && line.contains("opus[1m]")),
        "the refused switch never reaches the adapter: {calls}"
    );
}

/// The emission switch, in both states whatever value this build ships: off,
/// the capability is never advertised and every switch is refused
/// `MODEL_SWITCH_UNSUPPORTED` before the catalog is even consulted.
#[test]
fn model_switch_emission_const_gates_both_the_bit_and_admission() {
    let allowed = vec!["opus[1m]".to_owned()];
    assert!(model_switch::advertised(true, true));
    assert!(!model_switch::advertised(false, true));
    assert!(!model_switch::advertised(true, false));
    assert!(!serde_json::to_string(
        &Capabilities::v1_claude().with_model_switch(model_switch::advertised(false, true))
    )
    .expect("serialize")
    .contains("modelSwitch"));
    assert_eq!(
        model_switch::admission_refusal(true, "opus[1m][high]", &allowed),
        None
    );
    assert_eq!(
        model_switch::admission_refusal(false, "opus[1m][high]", &allowed).map(|(code, _)| code),
        Some(payload::MODEL_SWITCH_UNSUPPORTED)
    );
    assert_eq!(
        model_switch::admission_refusal(true, "gpt-9", &allowed).map(|(code, _)| code),
        Some(payload::MODEL_NOT_OFFERED)
    );
}

#[test]
fn model_switch_offered_base_takes_the_longest_listed_prefix() {
    let allowed = vec![
        "opus[1m]".to_owned(),
        "gpt-5.6-sol[high]".to_owned(),
        "haiku".to_owned(),
    ];
    assert_eq!(
        model_switch::offered_base("opus[1m][high][fast]", &allowed),
        Some("opus[1m]")
    );
    assert_eq!(
        model_switch::offered_base("gpt-5.6-sol[high]", &allowed),
        Some("gpt-5.6-sol[high]")
    );
    assert_eq!(
        model_switch::offered_base("haiku[low]", &allowed),
        Some("haiku")
    );
    assert_eq!(model_switch::offered_base("opus[high]", &allowed), None);
    assert_eq!(model_switch::offered_base("sonnet", &allowed), None);
}
