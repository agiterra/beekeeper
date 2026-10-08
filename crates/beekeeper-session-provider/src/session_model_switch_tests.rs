//! SV-35 actor-side tests: the switch is applied at the boundary in mailbox
//! order, refusals keep the old model, and a required mode is re-asserted.

use std::time::Duration;

use beekeeper_acp::acp::AcpClient;
use tokio::sync::mpsc;

use super::super::testing::{fake_agent, legacy_request};
use super::super::{SessionCommand, SessionEvent, SessionManager};
use super::*;

/// An adapter with a model select (`sonnet`, `opus[1m]`, `haiku`, `broken`)
/// and an effort select. Every request is appended to `__LOG__`. Setting
/// `broken` answers an error; setting `haiku` is acknowledged but the fresh
/// response still reports `sonnet`; `__MODE__` decides how `session/set_mode`
/// answers. The first prompt is held for a second so a command can arrive
/// mid-turn.
const SWITCH_AGENT: &str = r#"
PROMPTS=0
OPTIONS='[{"value":"sonnet"},{"value":"opus[1m]"},{"value":"haiku"},{"value":"broken"}]'
EFFORT='{"id":"effort","category":"thought_level","type":"select","options":[{"value":"low"},{"value":"high"}]}'
while IFS= read -r line; do
  printf '%s\n' "$line" >> '__LOG__'
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":2}}\n' "$id" ;;
    *'"method":"session/new"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"sessionId":"acp-session-1","configOptions":[{"id":"model","category":"model","type":"select","currentValue":"sonnet","options":%s},%s]}}\n' "$id" "$OPTIONS" "$EFFORT" ;;
    *'"method":"session/set_config_option"'*)
      case "$line" in
        *'"value":"broken"'*)
          printf '{"jsonrpc":"2.0","id":%s,"error":{"code":-32000,"message":"model unavailable"}}\n' "$id" ;;
        *'"value":"haiku"'*)
          printf '{"jsonrpc":"2.0","id":%s,"result":{"configOptions":[{"id":"model","category":"model","type":"select","currentValue":"sonnet","options":%s},%s]}}\n' "$id" "$OPTIONS" "$EFFORT" ;;
        *'"value":"opus[1m]"'*)
          printf '{"jsonrpc":"2.0","id":%s,"result":{"configOptions":[{"id":"model","category":"model","type":"select","currentValue":"opus[1m]","options":%s},%s]}}\n' "$id" "$OPTIONS" "$EFFORT" ;;
        *)
          printf '{"jsonrpc":"2.0","id":%s,"result":{}}\n' "$id" ;;
      esac ;;
    *'"method":"session/set_mode"'*)
      __MODE__ ;;
    *'"method":"session/prompt"'*)
      PROMPTS=$((PROMPTS+1))
      if [ "$PROMPTS" -eq 1 ]; then sleep 1; fi
      printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$id" ;;
  esac
done
"#;

const MODE_OK: &str = r#"printf '{"jsonrpc":"2.0","id":%s,"result":{}}\n' "$id""#;
const MODE_ERR: &str = r#"printf '{"jsonrpc":"2.0","id":%s,"error":{"code":-32000,"message":"no such mode"}}\n' "$id""#;

fn switch_agent(dir: &std::path::Path, mode: &str) -> (String, std::path::PathBuf) {
    let log = dir.join("acp.log");
    let body = SWITCH_AGENT
        .replace("__LOG__", &log.to_string_lossy())
        .replace("__MODE__", mode);
    (fake_agent(dir, "switch-agent", &body), log)
}

/// Methods (and, for config sets, values) in the order the adapter saw them.
fn methods(log: &std::path::Path) -> Vec<String> {
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter_map(|request| {
            let method = request["method"].as_str()?.to_owned();
            Some(match request["params"]["value"].as_str() {
                Some(value) => format!("{method}={value}"),
                None => match request["params"]["modeId"].as_str() {
                    Some(mode) => format!("{method}={mode}"),
                    None => method,
                },
            })
        })
        .collect()
}

async fn opened(script: &str, cwd: &std::path::Path) -> (AcpClient, String, serde_json::Value) {
    let mut client = AcpClient::spawn("bash", &[script.to_owned()], &[], false)
        .await
        .expect("spawn");
    client.initialize().await.expect("initialize");
    let response = client
        .session_new_full(&cwd.to_string_lossy(), Vec::new(), None, None)
        .await
        .expect("session/new");
    (client, response.session_id, response.raw)
}

fn turn(command_id: &str) -> SessionCommand {
    SessionCommand::Turn {
        command_id: command_id.into(),
        text: format!("run {command_id}"),
        attachments: Vec::new(),
        operator_pubkey: None,
        framing: None,
    }
}

/// Acceptance: a switch sent while a turn runs applies after that turn and
/// before the next one, in mailbox order — and the adapter sees the set call
/// between the two prompts.
#[tokio::test]
async fn model_switch_sent_mid_turn_applies_after_that_turn_in_mailbox_order() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (agent, log) = switch_agent(dir.path(), MODE_OK);
    let (tx, mut rx) = mpsc::channel(64);
    let mut manager = SessionManager::new(tx);
    let startup = manager
        .create(legacy_request(&agent, dir.path(), "s1"))
        .await
        .expect("create");
    assert!(
        startup.model_switch_supported,
        "the adapter offered a model select"
    );
    let handle = manager.handle("s1").expect("live");
    handle.deliver(turn("t1")).expect("deliver t1");
    // Wait until t1 is running, then switch, then send t2.
    loop {
        let event = tokio::time::timeout(Duration::from_secs(15), rx.recv())
            .await
            .expect("event")
            .expect("open");
        if matches!(&event, SessionEvent::TurnStarted { command_id, .. } if command_id == "t1") {
            break;
        }
    }
    let handle = manager.handle("s1").expect("live");
    handle
        .deliver(SessionCommand::SetModel {
            command_id: "m1".into(),
            selection: "opus[1m][high]".into(),
        })
        .expect("deliver switch");
    handle.deliver(turn("t2")).expect("deliver t2");

    let mut seen = Vec::new();
    let mut outcome = None;
    while seen
        .iter()
        .filter(|tag: &&String| *tag == "finished")
        .count()
        < 2
    {
        let event = tokio::time::timeout(Duration::from_secs(15), rx.recv())
            .await
            .expect("event")
            .expect("open");
        match event {
            SessionEvent::TurnFinished { .. } => seen.push("finished".into()),
            SessionEvent::ModelSwitch {
                command_id,
                outcome: switched,
                ..
            } => {
                assert_eq!(command_id, "m1");
                outcome = Some(switched);
                seen.push("switch".into());
            }
            SessionEvent::TurnStarted { command_id, .. } => {
                seen.push(format!("started:{command_id}"));
            }
            _ => {}
        }
    }
    assert_eq!(
        seen,
        ["finished", "switch", "started:t2", "finished"],
        "{seen:?}"
    );
    assert_eq!(
        outcome,
        Some(ModelSwitchOutcome::Applied {
            requested: "opus[1m][high]".into(),
            applied: "opus[1m][high]".into(),
        })
    );
    let order: Vec<String> = methods(&log)
        .into_iter()
        .filter(|method| method.starts_with("session/prompt") || method.contains("set_config"))
        .collect();
    assert_eq!(
        order,
        [
            "session/prompt",
            "session/set_config_option=opus[1m]",
            "session/set_config_option=high",
            "session/prompt",
        ],
    );
    manager.shutdown("s1");
}

/// Acceptance: an adapter error keeps the previous model and the snapshot.
#[tokio::test]
async fn model_switch_adapter_error_keeps_the_old_model() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (agent, _log) = switch_agent(dir.path(), MODE_OK);
    let (mut client, session_id, raw) = opened(&agent, dir.path()).await;
    let mut controls = ModelControls::new(raw.clone(), None);
    let (outcome, fatal) = switch_on(&mut client, &session_id, &mut controls, "broken").await;
    assert!(!fatal);
    match outcome {
        ModelSwitchOutcome::Refused { code, message } => {
            assert_eq!(code, MODEL_SWITCH_FAILED);
            assert!(message.contains("keeps its previous model"), "{message}");
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert_eq!(
        controls.raw, raw,
        "a failed switch leaves the snapshot alone"
    );
    client.shutdown().await;
}

/// Acceptance: the adapter-side refusals — no model control at all, and a
/// model the live adapter does not offer — send nothing or refuse by name.
#[tokio::test]
async fn model_switch_refuses_unsupported_and_unoffered_without_switching() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (agent, log) = switch_agent(dir.path(), MODE_OK);
    let (mut client, session_id, raw) = opened(&agent, dir.path()).await;

    let mut bare = ModelControls::new(serde_json::json!({"sessionId": session_id}), None);
    let (outcome, _) = switch_on(&mut client, &session_id, &mut bare, "opus[1m]").await;
    assert!(
        matches!(outcome, ModelSwitchOutcome::Refused { code, .. } if code == MODEL_SWITCH_UNSUPPORTED)
    );

    let mut controls = ModelControls::new(raw, None);
    let (outcome, _) = switch_on(&mut client, &session_id, &mut controls, "gpt-9[high]").await;
    assert!(
        matches!(outcome, ModelSwitchOutcome::Refused { code, .. } if code == MODEL_NOT_OFFERED)
    );
    assert!(
        !methods(&log)
            .iter()
            .any(|method| method.contains("set_config")),
        "neither refusal reached the adapter: {:?}",
        methods(&log)
    );
    client.shutdown().await;
}

/// The adapter's fresh report wins over the request when it names a different
/// base model, and A → B → A works against the updated snapshot.
#[tokio::test]
async fn model_switch_publishes_what_the_adapter_reports_and_switches_back() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (agent, _log) = switch_agent(dir.path(), MODE_OK);
    let (mut client, session_id, raw) = opened(&agent, dir.path()).await;
    let mut controls = ModelControls::new(raw, None);

    let (outcome, _) = switch_on(&mut client, &session_id, &mut controls, "haiku").await;
    assert_eq!(
        outcome,
        ModelSwitchOutcome::Applied {
            requested: "haiku".into(),
            applied: "sonnet".into(),
        },
        "the adapter still reports sonnet, so sonnet is what is published"
    );
    let (outcome, _) = switch_on(&mut client, &session_id, &mut controls, "opus[1m]").await;
    assert!(
        matches!(outcome, ModelSwitchOutcome::Applied { ref applied, .. } if applied == "opus[1m]")
    );
    assert_eq!(
        beekeeper_acp::acp::reported_model(&controls.raw).as_deref(),
        Some("opus[1m]"),
        "the snapshot follows the adapter"
    );
    let (outcome, _) = switch_on(&mut client, &session_id, &mut controls, "sonnet").await;
    assert!(
        matches!(outcome, ModelSwitchOutcome::Applied { ref applied, .. } if applied == "sonnet")
    );
    client.shutdown().await;
}

/// Acceptance: the Codex bounded mode is re-asserted after every switch, and
/// a switch that cannot re-assert it ends the execution.
#[tokio::test]
async fn model_switch_reasserts_the_bounded_mode_and_fails_closed() {
    let mode = crate::execution_scope::CODEX_BOUNDED_MODE;
    let dir = tempfile::tempdir().expect("tempdir");
    let (agent, log) = switch_agent(dir.path(), MODE_OK);
    let (mut client, session_id, raw) = opened(&agent, dir.path()).await;
    let mut controls = ModelControls::new(raw.clone(), Some(mode));
    let (outcome, fatal) = switch_on(&mut client, &session_id, &mut controls, "opus[1m]").await;
    assert!(
        matches!(outcome, ModelSwitchOutcome::Applied { .. }),
        "{outcome:?}"
    );
    assert!(!fatal);
    let tail: Vec<String> = methods(&log).into_iter().rev().take(2).collect();
    assert_eq!(
        tail,
        [
            format!("session/set_mode={mode}"),
            "session/set_config_option=opus[1m]".to_owned(),
        ],
        "the mode is set again after the model"
    );
    client.shutdown().await;

    let failing = tempfile::tempdir().expect("tempdir");
    let (agent, _log) = switch_agent(failing.path(), MODE_ERR);
    let (mut client, session_id, raw) = opened(&agent, failing.path()).await;
    let mut controls = ModelControls::new(raw, Some(mode));
    let (outcome, fatal) = switch_on(&mut client, &session_id, &mut controls, "opus[1m]").await;
    assert!(fatal, "an unassertable boundary mode ends the execution");
    assert!(
        matches!(outcome, ModelSwitchOutcome::Refused { code, .. } if code == MODEL_SWITCH_FAILED)
    );
    client.shutdown().await;
}

#[test]
fn model_switch_reconcile_keeps_effort_when_the_base_agrees() {
    assert_eq!(
        reconcile_applied("opus[1m][high]".into(), Some("opus[1m]".into())),
        "opus[1m][high]"
    );
    assert_eq!(
        reconcile_applied("haiku".into(), Some("sonnet".into())),
        "sonnet"
    );
    assert_eq!(reconcile_applied("haiku".into(), None), "haiku");
}
