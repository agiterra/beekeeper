//! The long-lived stdout reader against a scripted adapter over real pipes
//! (SV-77, ledger 336).

use super::*;
use std::time::Duration;

async fn spawn_script(script: &str) -> AcpClient {
    AcpClient::spawn("bash", &["-c".into(), script.into()], &[], false)
        .await
        .expect("spawn scripted adapter")
}

/// Pulls the numeric JSON-RPC id out of the line the script just read.
const READ_ID: &str = r#"id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')"#;

const CHUNK: &str = r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"woke"}}}}"#;

const PERMISSION: &str = r#"{"jsonrpc":"2.0","id":"perm-1","method":"session/request_permission","params":{"sessionId":"s","options":[{"optionId":"no","kind":"reject_once"},{"optionId":"yes","kind":"allow_once"}]}}"#;

async fn next_within(client: &mut AcpClient, limit: Duration) -> Unsolicited {
    tokio::time::timeout(limit, client.next_unsolicited())
        .await
        .expect("unsolicited frame within the limit")
}

/// (a) Output with no prompt in flight reaches the owner live — not at the
/// next prompt.
#[tokio::test]
async fn an_update_with_no_prompt_in_flight_is_delivered_within_a_second() {
    let mut client = spawn_script(&format!("sleep 0.3; echo '{CHUNK}'; sleep 10")).await;
    let started = std::time::Instant::now();
    let item = next_within(&mut client, Duration::from_secs(1)).await;
    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(item.opens_turn(), "agent prose opens a turn: {item:?}");
    let Unsolicited::Frame(msg) = item else {
        panic!("expected a frame, got {item:?}");
    };
    assert_eq!(msg["params"]["update"]["content"]["text"], "woke");
    client.shutdown().await;
}

/// (b) A permission request between turns is answered at once with the
/// prompted turn's policy (allow_once by kind), and the owner hears of it.
#[tokio::test]
async fn a_permission_request_between_turns_is_answered_with_the_turn_policy() {
    let script = format!(
        r#"sleep 0.2; echo '{PERMISSION}'
read -r reply
printf '{{"jsonrpc":"2.0","method":"test/echo","params":%s}}\n' "$reply"
sleep 10"#
    );
    let mut client = spawn_script(&script).await;
    let request = next_within(&mut client, Duration::from_secs(2)).await;
    assert!(
        request.opens_turn(),
        "a permission request is the agent working"
    );
    let echo = next_within(&mut client, Duration::from_secs(2)).await;
    let Unsolicited::Frame(echo) = echo else {
        panic!("expected the echoed reply, got {echo:?}");
    };
    assert_eq!(
        echo["params"]["id"], "perm-1",
        "the reply keeps the request's string id"
    );
    assert_eq!(
        echo["params"]["result"]["outcome"],
        serde_json::json!({"outcome": "selected", "optionId": "yes"})
    );
    client.shutdown().await;
}

/// (b, legacy owner) A client whose owner never asks for unsolicited frames —
/// the managed-agent pool — still has the request answered at once; only the
/// echo, a notification, waits in the inbox for its next read loop.
#[tokio::test]
async fn a_permission_request_is_answered_even_when_nobody_reads_between_turns() {
    let script = format!(
        r#"echo '{PERMISSION}'
read -r reply
printf '{{"jsonrpc":"2.0","method":"test/echo","params":%s}}\n' "$reply"
sleep 10"#
    );
    let mut client = spawn_script(&script).await;
    let echoed = tokio::time::timeout(Duration::from_secs(2), client.inbox.recv())
        .await
        .expect("answered without any request in flight");
    let Some(Inbound::Frame { msg, .. }) = echoed else {
        panic!("expected the echo frame, got {echoed:?}");
    };
    assert_eq!(msg["params"]["result"]["outcome"]["optionId"], "yes");
    client.shutdown().await;
}

/// (c) Responses match their request by id while stray responses, agent
/// requests and notifications interleave — for a plain request and for a
/// prompt.
#[tokio::test]
async fn responses_match_by_id_under_interleaving() {
    let script = format!(
        r#"while IFS= read -r line; do
  {READ_ID}
  case "$line" in
    *'"method":"initialize"'*)
      echo '{{"jsonrpc":"2.0","id":77,"result":{{"stray":true}}}}'
      echo '{CHUNK}'
      echo '{{"jsonrpc":"2.0","id":5,"method":"x/unknown","params":{{}}}}'
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"protocolVersion":2,"mine":true}}}}\n' "$id" ;;
    *'"method":"session/prompt"'*)
      echo '{CHUNK}'
      echo '{{"jsonrpc":"2.0","id":4242,"result":{{"stray":true}}}}'
      echo '{PERMISSION}'
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"stopReason":"end_turn"}}}}\n' "$id" ;;
  esac
done"#
    );
    let mut client = spawn_script(&script).await;
    let init = client.initialize().await.expect("initialize answered");
    assert_eq!(init["mine"], true, "the stray id-77 response was not taken");

    let stop = client
        .session_prompt_with_idle_timeout(
            "s",
            "go",
            Duration::from_secs(5),
            Duration::from_secs(10),
        )
        .await
        .expect("prompt answered");
    assert_eq!(stop, StopReason::EndTurn);
    assert!(!client.has_in_flight_prompt());
    client.shutdown().await;
}

/// (d) The adapter exiting fails the request waiting on it at once rather
/// than at its 60s timeout, fails every later request immediately, and tells
/// the owner.
#[tokio::test]
async fn adapter_exit_fails_pending_and_later_requests() {
    let mut client = spawn_script("read -r line; exit 0").await;
    // Delivery on: the owner must hear about the exit.
    assert!(client.try_next_unsolicited().is_none());
    let started = std::time::Instant::now();
    let pending = client.initialize().await;
    assert!(
        matches!(pending, Err(AcpError::AgentExited)),
        "got {pending:?}"
    );
    assert!(started.elapsed() < Duration::from_secs(5));
    let later = client.authenticate("x").await;
    assert!(
        matches!(later, Err(AcpError::AgentExited) | Err(AcpError::Io(_))),
        "got {later:?}"
    );
    let item = next_within(&mut client, Duration::from_secs(1)).await;
    assert!(matches!(item, Unsolicited::Exited), "got {item:?}");
    client.shutdown().await;
}

/// (d, prompt) A prompt in flight when the adapter dies fails with
/// `AgentExited`, not an idle timeout.
#[tokio::test]
async fn adapter_exit_fails_the_prompt_in_flight() {
    let mut client = spawn_script("read -r line; exit 0").await;
    let result = client
        .session_prompt_with_idle_timeout(
            "s",
            "go",
            Duration::from_secs(20),
            Duration::from_secs(30),
        )
        .await;
    assert!(
        matches!(result, Err(AcpError::AgentExited)),
        "got {result:?}"
    );
    client.shutdown().await;
}

/// Output that trails a prompt's answer is not left for the next prompt: it
/// is delivered as between-turn output, and the next prompt sees only its
/// own frames.
#[tokio::test]
async fn output_after_the_prompt_answer_is_not_glued_onto_the_next_prompt() {
    let script = format!(
        r#"n=0
while IFS= read -r line; do
  {READ_ID}
  case "$line" in
    *'"method":"session/prompt"'*)
      n=$((n+1))
      if [ "$n" = 1 ]; then
        printf '{{"jsonrpc":"2.0","id":%s,"result":{{"stopReason":"end_turn"}}}}\n{CHUNK}\n' "$id"
      else
        printf '{{"jsonrpc":"2.0","id":%s,"result":{{"stopReason":"end_turn"}}}}\n' "$id"
      fi ;;
  esac
done"#
    );
    let mut client = spawn_script(&script).await;
    // The owner asks for between-turn output before the first prompt.
    assert!(client.try_next_unsolicited().is_none());
    let observer = ObserverHandle::in_process();
    client.set_observer(Some(observer.clone()), 0);
    let mut frames = observer.subscribe();
    let stop = client
        .session_prompt_with_idle_timeout(
            "s",
            "one",
            Duration::from_secs(5),
            Duration::from_secs(10),
        )
        .await
        .expect("first prompt");
    assert_eq!(stop, StopReason::EndTurn);

    let item = next_within(&mut client, Duration::from_secs(1)).await;
    assert!(
        item.opens_turn(),
        "the trailing chunk arrives upward: {item:?}"
    );

    client
        .session_prompt_with_idle_timeout(
            "s",
            "two",
            Duration::from_secs(5),
            Duration::from_secs(10),
        )
        .await
        .expect("second prompt");
    // The trailing chunk was never observed as a turn's `acp_read`.
    let mut turn_updates = 0;
    while let Ok(event) = frames.try_recv() {
        if event.kind == "acp_read" && event.payload["method"] == "session/update" {
            turn_updates += 1;
        }
    }
    assert_eq!(
        turn_updates, 0,
        "between-turn output was filed under a turn"
    );
    client.shutdown().await;
}

/// The end of an autonomous cycle is recognised from the adapter's raw
/// `result`; a `num_turns: 0` placeholder and a user-lane result are not it.
#[tokio::test]
async fn an_autonomous_result_ends_the_cycle_and_a_placeholder_does_not() {
    let result = |origin: &str, turns: u64| {
        format!(
            r#"{{"jsonrpc":"2.0","method":"_claude/sdkMessage","params":{{"sessionId":"s","message":{{"type":"result","num_turns":{turns},"origin":{{"kind":"{origin}"}}}}}}}}"#
        )
    };
    let script = format!(
        "sleep 0.2; echo '{}'; echo '{}'; echo '{}'; sleep 10",
        result("task-notification", 0),
        result("human", 3),
        result("task-notification", 2),
    );
    let mut client = spawn_script(&script).await;
    let item = next_within(&mut client, Duration::from_secs(2)).await;
    assert!(
        matches!(&item, Unsolicited::CycleEnded { origin } if origin == "task-notification"),
        "got {item:?}"
    );
    client.shutdown().await;
}
