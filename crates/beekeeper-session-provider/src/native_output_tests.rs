//! The 2026-10-03 canary-codex-1 sequence, reproduced frame for frame.
//!
//! codex-acp 2.0.1 over Codex 0.159.3: one opening `tool_call` (title
//! `canary.sh`, a terminal), 14 `terminal_output_delta` updates carrying only
//! lines 8–22 of the script's 22 — Codex spawned the command, the first seven
//! probes printed, and only then did it subscribe the stream — and a
//! completion carrying `terminal_exit` alone. The session's own rollout
//! recorded every line as the command's `aggregated_output`.
use super::*;
use crate::transcript::{TranscriptTranslator, CONTENT_SOURCE_STREAMED};

const ID: &str = "exec-438a816d-de72-46cb-a2eb-1f3be9a67ff2";

const PROBES: [&str; 22] = [
    "sibling_run_file",
    "hidden_oracle_canary",
    "agents_repo_ledger",
    "operator_nostr_key",
    "operator_claude_history",
    "operator_codex_sessions",
    "lab_host_provider_key",
    "dev_provider_state",
    "hive_git_ls_remote",
    "direct_https_hive",
    "direct_https_model_host",
    "proxied_https_hive",
    "proxied_https_other",
    "loopback_other_port",
    "host_unix_socket",
    "ssh_agent_env",
    "relay_key_env",
    "git_credential_config",
    "bee_relay_query",
    "visible_tests_run",
    "local_commit",
    "private_native_home_set",
];

fn line(probe: &str) -> String {
    let expect = if matches!(
        probe,
        "visible_tests_run" | "local_commit" | "private_native_home_set"
    ) {
        "allowed"
    } else {
        "denied"
    };
    format!(
        "{{\"probe\":\"{probe}\",\"expect\":\"{expect}\",\"observed\":\"{expect}\",\"pass\":true}}\n"
    )
}

fn full_output() -> String {
    PROBES.iter().map(|probe| line(probe)).collect()
}

/// The 14 chunks codex-acp streamed: lines 8–22, the last two together.
fn streamed_chunks() -> Vec<String> {
    let mut chunks: Vec<String> = PROBES[7..20].iter().map(|probe| line(probe)).collect();
    chunks.push(line(PROBES[20]) + &line(PROBES[21]));
    chunks
}

fn start() -> Value {
    json!({"sessionUpdate":"tool_call","toolCallId":ID,"kind":"execute",
        "title":"canary.sh","status":"in_progress",
        "rawInput":{"command":"bash canary.sh","cwd":"."},
        "content":[{"type":"terminal","terminalId":ID}],
        "_meta":{"terminal_info":{"terminal_id":ID,"cwd":"."}}})
}

fn delta(text: &str) -> Value {
    json!({"sessionUpdate":"tool_call_update","toolCallId":ID,
        "_meta":{"terminal_output_delta":{"terminal_id":ID,"data":text}}})
}

fn done() -> Value {
    json!({"sessionUpdate":"tool_call_update","toolCallId":ID,"status":"completed",
        "_meta":{"terminal_exit":{"terminal_id":ID,"exit_code":0,"signal":null}}})
}

/// Run the observed frames through a translator; return the terminal frame's
/// items and the checks the translator queued.
fn translate(chunks: &[String]) -> (Vec<Value>, Vec<StreamCheck>) {
    let mut t = TranscriptTranslator::new(false);
    t.on_update(&start());
    for chunk in chunks {
        assert!(t.on_update(&delta(chunk)).is_empty());
    }
    let items = t.on_update(&done());
    (items, t.take_stream_checks())
}

fn result(items: &[Value]) -> &Value {
    items
        .iter()
        .find(|item| item["kind"] == "tool_result")
        .expect("tool result")
}

/// A private Codex home whose rollout recorded `aggregated` for the call, in
/// the shape Codex 0.159.3 writes (`event_msg` / `item_completed`).
fn codex_home_with(aggregated: Option<&str>) -> tempfile::TempDir {
    let home = tempfile::tempdir().expect("tempdir");
    let day = home.path().join("sessions/2026/10/03");
    std::fs::create_dir_all(&day).expect("sessions dir");
    if let Some(aggregated) = aggregated {
        write_rollout(&day, aggregated);
    }
    home
}

fn write_rollout(day: &Path, aggregated: &str) {
    let other = json!({"timestamp":"2026-10-03T21:26:25.556Z","type":"response_item",
        "payload":{"type":"custom_tool_call","call_id":"call_x","name":"exec"}});
    let record = json!({"timestamp":"2026-10-03T21:26:26.174Z","ordinal":14,"type":"event_msg",
        "payload":{"type":"item_completed","thread_id":"01a103a9","turn_id":"01a103a9-2744",
            "item":{"type":"CommandExecution","id":ID,"status":"completed",
                "aggregated_output":aggregated,"exit_code":0}}});
    let body = format!("{other}\n{record}\n");
    std::fs::write(day.join("rollout-2026-10-03T17-26-19-01a103a9.jsonl"), body).expect("rollout");
}

#[test]
fn the_observed_sequence_publishes_an_unverified_tail() {
    let (items, checks) = translate(&streamed_chunks());
    let result = result(&items);
    let content = result["content"].as_str().expect("content");
    assert!(content.starts_with("{\"probe\":\"dev_provider_state\""));
    assert_eq!(result["exitCode"], 0);
    assert_eq!(result["contentSource"], CONTENT_SOURCE_STREAMED);
    assert_eq!(result["outputComplete"], false);
    assert_eq!(result["outputGap"], json!({"streamedBytes": content.len()}));
    assert_eq!(checks.len(), 1);
    assert_eq!(checks[0].tool_id, ID);
}

#[tokio::test]
async fn codex_canary_result_is_recovered_whole_from_the_sessions_own_rollout() {
    let full = full_output();
    let home = codex_home_with(Some(&full));
    let (mut items, checks) = translate(&streamed_chunks());
    let source = NativeOutputSource::codex(home.path().to_owned());
    reconcile(&mut items, checks, Some(&source)).await;
    let result = result(&items);
    assert_eq!(result["content"], full.as_str(), "all 22 lines");
    assert_eq!(
        result["content"]
            .as_str()
            .map(str::lines)
            .map(Iterator::count),
        Some(22)
    );
    assert_eq!(result["contentSource"], CONTENT_SOURCE_NATIVE_ROLLOUT);
    assert_eq!(result["outputComplete"], true);
    assert!(result.get("outputGap").is_none());
    assert_eq!(result["exitCode"], 0);
}

#[tokio::test]
async fn a_record_the_stream_is_not_a_suffix_of_is_refused() {
    // Same length budget, different ending: the record is not this stream's.
    let mut other = full_output();
    other.push_str("{\"probe\":\"extra\"}\n");
    let home = codex_home_with(Some(&other));
    let (mut items, checks) = translate(&streamed_chunks());
    let streamed = result(&items)["content"].clone();
    let source = NativeOutputSource::codex(home.path().to_owned());
    reconcile(&mut items, checks, Some(&source)).await;
    let result = result(&items);
    assert_eq!(result["content"], streamed);
    assert_eq!(result["contentSource"], CONTENT_SOURCE_STREAMED);
    assert_eq!(result["outputComplete"], false);
    let streamed_bytes = streamed.as_str().map_or(0, str::len);
    assert_eq!(
        result["outputGap"],
        json!({"streamedBytes": streamed_bytes, "aggregatedBytes": other.len()})
    );
}

#[tokio::test]
async fn no_record_leaves_the_result_honestly_unverified() {
    let home = codex_home_with(None);
    let (mut items, checks) = translate(&streamed_chunks());
    let before = result(&items).clone();
    let source = NativeOutputSource::codex(home.path().to_owned());
    reconcile(&mut items, checks.clone(), Some(&source)).await;
    assert_eq!(result(&items), &before);
    assert_eq!(before["outputComplete"], false);
    // No private home at all (an unbounded execution): the same.
    reconcile(&mut items, checks, None).await;
    assert_eq!(result(&items), &before);
}

#[tokio::test]
async fn a_whole_stream_is_verified_complete_and_left_as_streamed() {
    let full = full_output();
    let home = codex_home_with(Some(&full));
    let (mut items, checks) = translate(std::slice::from_ref(&full));
    let source = NativeOutputSource::codex(home.path().to_owned());
    reconcile(&mut items, checks, Some(&source)).await;
    let result = result(&items);
    assert_eq!(result["content"], full.as_str());
    assert_eq!(result["contentSource"], CONTENT_SOURCE_STREAMED);
    assert_eq!(result["outputComplete"], true);
    assert!(result.get("outputGap").is_none());
}

#[tokio::test]
async fn a_record_flushed_after_the_frame_is_still_found() {
    let full = full_output();
    let home = codex_home_with(None);
    let day = home.path().join("sessions/2026/10/03");
    let writer = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(20)).await;
        write_rollout(&day, &full);
    });
    let (mut items, checks) = translate(&streamed_chunks());
    let source = NativeOutputSource::codex(home.path().to_owned());
    reconcile(&mut items, checks, Some(&source)).await;
    writer.await.expect("writer");
    assert_eq!(
        result(&items)["contentSource"],
        CONTENT_SOURCE_NATIVE_ROLLOUT
    );
}

#[tokio::test]
async fn a_recovered_output_over_the_cap_keeps_an_honest_digest() {
    let head = "h".repeat(MAX_TOOL_CONTENT_BYTES);
    let tail = "tail\n".to_owned();
    let full = format!("{head}{tail}");
    let home = codex_home_with(Some(&full));
    let (mut items, checks) = translate(&[tail]);
    let source = NativeOutputSource::codex(home.path().to_owned());
    reconcile(&mut items, checks, Some(&source)).await;
    let content = result(&items)["content"].as_str().expect("content");
    assert!(content.len() <= MAX_TOOL_CONTENT_BYTES);
    assert_eq!(content, bound_text(&full, MAX_TOOL_CONTENT_BYTES));
    assert!(content.contains("…[elided "));
    assert_eq!(result(&items)["outputComplete"], true);
}

#[test]
fn a_final_frame_result_carries_no_completeness_keys() {
    let mut t = TranscriptTranslator::new(false);
    t.on_update(&start());
    let mut end = done();
    end["content"] = json!([{"type":"content","content":{"type":"text","text":"whole"}}]);
    let items = t.on_update(&end);
    let result = result(&items);
    assert_eq!(result["content"], "whole");
    for key in ["contentSource", "outputComplete", "outputGap"] {
        assert!(result.get(key).is_none(), "{key}");
    }
    assert!(t.take_stream_checks().is_empty());
}

#[test]
fn only_a_completed_command_record_with_the_same_id_counts() {
    let home = tempfile::tempdir().expect("tempdir");
    let day = home.path().join("sessions/2026/10/03");
    std::fs::create_dir_all(&day).expect("dir");
    let started = json!({"type":"event_msg","payload":{"type":"item_started",
        "item":{"id":ID,"aggregated_output":"started only"}}});
    let other = json!({"type":"event_msg","payload":{"type":"item_completed",
        "item":{"id":format!("{ID}-other"),"aggregated_output":"another call"}}});
    std::fs::write(day.join("rollout-a.jsonl"), format!("{started}\n{other}\n")).expect("write");
    assert_eq!(aggregated_output(home.path(), ID), None);
}
