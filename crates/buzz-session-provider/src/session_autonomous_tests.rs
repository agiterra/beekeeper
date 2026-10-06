//! An unprompted turn, end to end through a real actor and a scripted
//! adapter over real pipes (SV-77, ledger 336).

use super::super::testing::{fake_agent, legacy_request};
use super::*;

/// An adapter that answers prompts at once and, after the first, starts a
/// cycle of its own — prose, a permission request, then the autonomous
/// `result` that closes it — `cycle_gap` seconds apart. It touches
/// `approved` when the permission reply it reads carries `allow_once`.
fn waking_agent(approved: &std::path::Path, cycle_gap: &str) -> String {
    format!(
        r#"n=0
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
  case "$line" in
    *'"optionId":"yes"'*)
      touch "{approved}" ;;
    *'"method":"initialize"'*)
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"protocolVersion":2}}}}\n' "$id" ;;
    *'"method":"session/new"'*)
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"sessionId":"acp-session-1"}}}}\n' "$id" ;;
    *'"method":"session/prompt"'*)
      n=$((n+1))
      printf '{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"acp-session-1","update":{{"sessionUpdate":"agent_message_chunk","content":{{"type":"text","text":"answer %s"}}}}}}}}\n' "$n"
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"stopReason":"end_turn"}}}}\n' "$id"
      if [ "$n" = 1 ]; then
        (
          sleep 0.4
          printf '%s\n' '{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"acp-session-1","update":{{"sessionUpdate":"agent_message_chunk","content":{{"type":"text","text":"woke on its own"}}}}}}}}'
          printf '%s\n' '{{"jsonrpc":"2.0","id":"perm-1","method":"session/request_permission","params":{{"sessionId":"acp-session-1","options":[{{"optionId":"no","kind":"reject_once"}},{{"optionId":"yes","kind":"allow_once"}}]}}}}'
          sleep {cycle_gap}
          printf '%s\n' '{{"jsonrpc":"2.0","method":"_claude/sdkMessage","params":{{"sessionId":"acp-session-1","message":{{"type":"result","num_turns":1,"origin":{{"kind":"task-notification"}}}}}}}}'
        ) &
      fi ;;
  esac
done
"#,
        approved = approved.display(),
    )
}

async fn next_event(rx: &mut mpsc::Receiver<SessionEvent>) -> SessionEvent {
    tokio::time::timeout(Duration::from_secs(15), rx.recv())
        .await
        .expect("event within timeout")
        .expect("channel open")
}

fn turn(command_id: &str) -> SessionCommand {
    SessionCommand::Turn {
        command_id: command_id.into(),
        attachments: Vec::new(),
        text: "go".into(),
        operator_pubkey: None,
        framing: None,
    }
}

/// Every event up to and including the next `TurnFinished`, with the text of
/// the items each turn id carried.
#[derive(Default)]
struct Seen {
    lifecycle: Vec<String>,
    text_by_turn: HashMap<String, String>,
    outcomes: Vec<(String, TurnOutcome)>,
}

impl Seen {
    async fn through_finish(&mut self, rx: &mut mpsc::Receiver<SessionEvent>) -> String {
        loop {
            match next_event(rx).await {
                SessionEvent::TurnStarted {
                    turn_id,
                    command_id,
                    ..
                } => self
                    .lifecycle
                    .push(format!("started {command_id} {turn_id}")),
                SessionEvent::AutonomousTurnStarted { turn_id, .. } => {
                    self.lifecycle.push(format!("autonomous {turn_id}"));
                }
                SessionEvent::TranscriptItems { turn_id, items, .. } => {
                    let text = self.text_by_turn.entry(turn_id).or_default();
                    for item in items {
                        text.push_str(&item.to_string());
                    }
                }
                SessionEvent::TurnFinished {
                    turn_id, outcome, ..
                } => {
                    self.lifecycle.push(format!("finished {turn_id}"));
                    self.outcomes.push((turn_id.clone(), outcome));
                    return turn_id;
                }
                _ => {}
            }
        }
    }
}

/// The whole of SV-77's fix as the provider sees it: the agent's own cycle is
/// published live as its own turn — not filed under the next prompt — its
/// permission request is answered with nobody prompting, and the next user
/// turn is a third, distinct turn.
#[tokio::test]
async fn an_unprompted_cycle_is_published_live_as_its_own_turn() {
    let dir = tempfile::tempdir().expect("tempdir");
    let approved = dir.path().join("approved");
    let agent = fake_agent(dir.path(), "waking-agent", &waking_agent(&approved, "0.3"));
    let (tx, mut rx) = mpsc::channel(64);
    let mut manager = SessionManager::new(tx);
    manager
        .create(legacy_request(&agent, dir.path(), "s1"))
        .await
        .expect("create");
    let handle = manager.handle("s1").expect("handle");

    handle.deliver(turn("turn-1")).expect("deliver");
    let mut seen = Seen::default();
    let first = seen.through_finish(&mut rx).await;

    let woke_at = std::time::Instant::now();
    let autonomous = seen.through_finish(&mut rx).await;
    assert!(
        woke_at.elapsed() < Duration::from_secs(5),
        "the unprompted turn arrived live, not at the next prompt"
    );
    assert_ne!(autonomous, first);
    assert_eq!(
        seen.lifecycle[2],
        format!("autonomous {autonomous}"),
        "{:?}",
        seen.lifecycle
    );
    let text = &seen.text_by_turn[&autonomous];
    assert!(text.contains("woke on its own"), "{text}");
    assert!(
        text.contains("autonomous_turn_started"),
        "the turn says up front that nobody prompted it: {text}"
    );
    assert!(
        text.contains("the agent woke on task-notification"),
        "the turn names what woke it: {text}"
    );
    assert!(
        !text.contains("user_prompt"),
        "nobody prompted this turn, and its record must not say anyone did: {text}"
    );
    assert_eq!(
        seen.outcomes[1].1,
        TurnOutcome::Completed {
            stop_reason: StopReason::EndTurn
        }
    );
    assert!(
        approved.exists(),
        "the between-turn permission request was answered allow_once"
    );

    handle.deliver(turn("turn-2")).expect("deliver");
    let third = seen.through_finish(&mut rx).await;
    assert!(
        third != first && third != autonomous,
        "{:?}",
        seen.lifecycle
    );
    let third_text = &seen.text_by_turn[&third];
    assert!(third_text.contains("answer 2"), "{third_text}");
    assert!(
        !third_text.contains("woke on its own"),
        "autonomous work was glued onto the next prompt: {third_text}"
    );
    manager.shutdown("s1");
}

/// A prompt that arrives while an unprompted turn runs waits for it: the
/// autonomous turn finishes before the prompted one starts, so neither
/// turn's output is filed under the other.
#[tokio::test]
async fn a_prompt_during_an_unprompted_turn_runs_after_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let approved = dir.path().join("approved");
    let agent = fake_agent(
        dir.path(),
        "slow-waking-agent",
        &waking_agent(&approved, "1.5"),
    );
    let (tx, mut rx) = mpsc::channel(64);
    let mut manager = SessionManager::new(tx);
    manager
        .create(legacy_request(&agent, dir.path(), "s1"))
        .await
        .expect("create");
    let handle = manager.handle("s1").expect("handle");

    handle.deliver(turn("turn-1")).expect("deliver");
    let mut seen = Seen::default();
    seen.through_finish(&mut rx).await;

    // Wait for the unprompted turn to open, then prompt into it.
    loop {
        match next_event(&mut rx).await {
            SessionEvent::AutonomousTurnStarted { turn_id, .. } => {
                seen.lifecycle.push(format!("autonomous {turn_id}"));
                break;
            }
            SessionEvent::TurnStarted { .. } | SessionEvent::TurnFinished { .. } => {
                panic!(
                    "a turn moved before the unprompted one opened: {:?}",
                    seen.lifecycle
                )
            }
            _ => {}
        }
    }
    handle.deliver(turn("turn-2")).expect("deliver");

    let autonomous = seen.through_finish(&mut rx).await;
    let prompted = seen.through_finish(&mut rx).await;
    assert_ne!(autonomous, prompted);
    let order: Vec<&str> = seen
        .lifecycle
        .iter()
        .map(|entry| entry.split(' ').next().unwrap_or(""))
        .collect();
    assert_eq!(
        order,
        [
            "started",
            "finished",
            "autonomous",
            "finished",
            "started",
            "finished"
        ],
        "{:?}",
        seen.lifecycle
    );
    assert!(seen.lifecycle[4].starts_with("started turn-2"));
    assert!(seen.text_by_turn[&prompted].contains("answer 2"));
    manager.shutdown("s1");
}

/// An adapter whose unprompted cycle is the audit's (BK-AUDIT-1006) final
/// answer: two prose chunks with a paragraph break between them, then the
/// autonomous `result` at once — no tool call or permission to flush the
/// prose first.
const ANSWERING_AGENT: &str = r#"n=0
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":2}}\n' "$id" ;;
    *'"method":"session/new"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"sessionId":"acp-session-1"}}\n' "$id" ;;
    *'"method":"session/prompt"'*)
      n=$((n+1))
      printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$id"
      if [ "$n" = 1 ]; then
        (
          sleep 0.3
          printf '%s\n' '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"acp-session-1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"All nine steps are done.\n\n"}}}}'
          printf '%s\n' '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"acp-session-1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"**Failed commands:** only the one that failed on purpose."}}}}'
          printf '%s\n' '{"jsonrpc":"2.0","method":"_claude/sdkMessage","params":{"sessionId":"acp-session-1","message":{"type":"result","num_turns":1,"origin":{"kind":"task-notification"}}}}'
        ) &
      fi ;;
  esac
done
"#;

/// SV-93 (wire half): the row naming what woke the agent is published after
/// the prose the agent had already said, never between two parts of its
/// answer. The audit's seq 52 landed between seq 51 and 53, splitting it.
#[tokio::test]
async fn the_wake_row_never_splits_the_final_answer() {
    let dir = tempfile::tempdir().expect("tempdir");
    let agent = fake_agent(dir.path(), "answering-agent", ANSWERING_AGENT);
    let (tx, mut rx) = mpsc::channel(64);
    let mut manager = SessionManager::new(tx);
    manager
        .create(legacy_request(&agent, dir.path(), "s1"))
        .await
        .expect("create");
    let handle = manager.handle("s1").expect("handle");
    handle.deliver(turn("turn-1")).expect("deliver");

    // Collect the autonomous turn's items in publication order.
    let mut autonomous: Option<String> = None;
    let mut items: Vec<serde_json::Value> = Vec::new();
    loop {
        match next_event(&mut rx).await {
            SessionEvent::AutonomousTurnStarted { turn_id, .. } => autonomous = Some(turn_id),
            SessionEvent::TranscriptItems {
                turn_id,
                items: batch,
                ..
            } if autonomous.as_deref() == Some(turn_id.as_str()) => items.extend(batch),
            SessionEvent::TurnFinished { turn_id, .. }
                if autonomous.as_deref() == Some(turn_id.as_str()) =>
            {
                break
            }
            _ => {}
        }
    }
    let wake_row = items
        .iter()
        .position(|item| {
            item["status"]
                .as_str()
                .is_some_and(|status| status.starts_with("autonomous_turn: "))
        })
        .expect("the wake row is published");
    let prose: Vec<usize> = items
        .iter()
        .enumerate()
        .filter(|(_, item)| item["kind"] == "assistant_text")
        .map(|(at, _)| at)
        .collect();
    let text: String = prose
        .iter()
        .filter_map(|at| items[*at]["text"].as_str())
        .collect();
    assert!(
        text.contains("All nine steps") && text.contains("Failed commands"),
        "{items:?}"
    );
    assert!(
        prose.iter().all(|at| *at < wake_row),
        "the wake row interrupted the answer: {items:?}"
    );
    manager.shutdown("s1");
}
