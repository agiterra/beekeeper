//! Live proof for SV-77 (ledger 336): a turn Claude Code starts on its own,
//! waking on a background task's notification, is read while it happens
//! rather than when the next prompt arrives.
//!
//! Ignored by default: it drives the real claude-agent-acp, which calls the
//! model with this machine's Claude credentials. Run it with
//!
//! ```text
//! BUZZ_LIVE_CLAUDE_ACP=<path to claude-agent-acp> \
//!   cargo test -p beekeeper-acp --test live_background_wake -- --ignored --nocapture
//! ```

use std::time::{Duration, Instant};

use beekeeper_acp::acp::{AcpClient, Unsolicited};

const SLEEP_SECS: u64 = 20;

fn short(msg: &serde_json::Value) -> String {
    let update = &msg["params"]["update"];
    let kind = update["sessionUpdate"]
        .as_str()
        .or_else(|| msg["method"].as_str())
        .unwrap_or("?");
    let text = update
        .pointer("/content/text")
        .and_then(serde_json::Value::as_str)
        .or_else(|| update["title"].as_str())
        .or_else(|| msg.pointer("/params/message/type").and_then(|v| v.as_str()))
        .unwrap_or("");
    let text: String = text.chars().take(160).collect();
    format!("{kind} {text:?}")
}

#[tokio::test]
#[ignore = "drives the real claude-agent-acp and model; see module docs"]
async fn an_autonomous_wake_is_read_while_it_happens() {
    let Ok(adapter) = std::env::var("BUZZ_LIVE_CLAUDE_ACP") else {
        panic!("set BUZZ_LIVE_CLAUDE_ACP to the claude-agent-acp binary");
    };
    let dir = std::env::temp_dir().join(format!("bgwake-live-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");

    let mut client = AcpClient::spawn(&adapter, &[], &[], false)
        .await
        .expect("spawn adapter");
    client.initialize().await.expect("initialize");
    client.set_emit_raw_sdk_frames(true);
    let session = client
        .session_new(&dir.to_string_lossy(), vec![], None, None)
        .await
        .expect("session/new");

    let prompt = format!(
        "Use the Bash tool with run_in_background set to true to run exactly: \
         python3 -c 'import time; time.sleep({SLEEP_SECS})'\n\
         Do not wait for it and do not check on it. Reply with the single word \
         STARTED and end your turn now. Later, when you are notified that the \
         background command completed, reply with the single word FINISHED."
    );
    let stop = client
        .session_prompt_with_idle_timeout(
            &session,
            &prompt,
            Duration::from_secs(120),
            Duration::from_secs(300),
        )
        .await
        .expect("prompt");
    let prompt_ended = Instant::now();
    eprintln!("[0.0s] prompted turn ended: {stop:?}");

    let mut first_wake = None;
    let mut finished_text = None;
    let mut cycle_end = None;
    let deadline = prompt_ended + Duration::from_secs(SLEEP_SECS + 150);
    while cycle_end.is_none() {
        let left = deadline.saturating_duration_since(Instant::now());
        let Ok(item) = tokio::time::timeout(left, client.next_unsolicited()).await else {
            break;
        };
        let at = prompt_ended.elapsed().as_secs_f32();
        if first_wake.is_none() && item.opens_turn() {
            first_wake = Some(at);
        }
        match &item {
            Unsolicited::Frame(msg) => {
                eprintln!("[{at:.1}s] {}", short(msg));
                if msg.to_string().contains("FINISHED") && finished_text.is_none() {
                    finished_text = Some(at);
                }
            }
            Unsolicited::CycleEnded { origin } => {
                eprintln!("[{at:.1}s] cycle ended, origin {origin}");
                cycle_end = Some(at);
            }
            Unsolicited::Exited => {
                eprintln!("[{at:.1}s] adapter exited");
                break;
            }
        }
    }
    client.shutdown().await;
    let _ = std::fs::remove_dir_all(&dir);

    eprintln!(
        "first wake {first_wake:?}s, FINISHED text {finished_text:?}s, cycle end {cycle_end:?}s \
         (background sleep {SLEEP_SECS}s, measured from the prompted turn's end)"
    );
    let wake = first_wake.expect("the autonomous turn was never read");
    assert!(
        wake < (SLEEP_SECS + 30) as f32,
        "the wake was read {wake}s after the prompt ended, long after the task finished"
    );
    assert!(
        finished_text.is_some(),
        "the autonomous reply was never read"
    );
    assert!(cycle_end.is_some(), "the cycle's end was never read");
}
