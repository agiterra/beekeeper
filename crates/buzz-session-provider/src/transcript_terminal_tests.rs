//! Frames from installed @agentclientprotocol/codex-acp 2.0.1's
//! AcpToolCallRenderer / CommandReporter (dist/index.js:27537–27580,
//! 27712–27749, 36529–36532): output is an append-only metadata channel;
//! terminal completion deliberately carries no content/rawOutput.
use super::*;

fn start(id: &str) -> Value {
    json!({"sessionUpdate":"tool_call", "toolCallId":id, "kind":"execute",
        "title":"python3 tools/event_probe.py failure", "status":"in_progress",
        "rawInput":{"command":"python3 tools/event_probe.py failure","cwd":"."},
        "content":[{"type":"terminal","terminalId":id}],
        "_meta":{"terminal_info":{"terminal_id":id,"cwd":"."}}})
}
fn delta(id: &str, text: &str) -> Value {
    json!({"sessionUpdate":"tool_call_update", "toolCallId":id,
        "_meta":{"terminal_output_delta":{"terminal_id":id,"data":text}}})
}
fn done(id: &str, code: i64) -> Value {
    json!({"sessionUpdate":"tool_call_update", "toolCallId":id,
        "status":if code == 0 {"completed"} else {"failed"},
        "_meta":{"terminal_exit":{"terminal_id":id,"exit_code":code,"signal":null}}})
}
fn result(items: &[Value]) -> &Value {
    items
        .iter()
        .find(|item| item["kind"] == "tool_result")
        .expect("tool result")
}

#[test]
fn codex_streamed_output_and_observed_exit_survive_terminal_only_completion() {
    let mut t = TranscriptTranslator::new(false);
    t.on_update(&start("exec-one"));
    assert!(t.on_update(&delta("exec-one", "synthetic ")).is_empty());
    assert!(t
        .on_update(&delta("exec-one", "failure probe\n"))
        .is_empty());
    let items = t.on_update(&done("exec-one", 7));
    assert_eq!(result(&items)["content"], "synthetic failure probe\n");
    assert_eq!(result(&items)["exitCode"], 7);
    assert_eq!(result(&items)["isError"], true);
}

#[test]
fn codex_output_sent_on_completion_is_consumed_once() {
    let mut t = TranscriptTranslator::new(false);
    t.on_update(&start("exec-one"));
    let mut end = done("exec-one", 0);
    end["_meta"]["terminal_output_delta"] = json!({"terminal_id":"exec-one","data":"once\n"});
    let items = t.on_update(&end);
    assert_eq!(result(&items)["content"], "once\n");
    let duplicate = t.on_update(&done("exec-one", 0));
    assert_eq!(result(&duplicate)["content"], "");
}

#[test]
fn terminal_identity_mismatch_and_anonymous_frames_do_not_leak_output_or_exit() {
    let mut t = TranscriptTranslator::new(false);
    t.on_update(&start("a"));
    let mut wrong = delta("a", "other call secret");
    wrong["_meta"]["terminal_output_delta"]["terminal_id"] = json!("b");
    t.on_update(&wrong);
    let mut end = done("a", 7);
    end["_meta"]["terminal_exit"]["terminal_id"] = json!("b");
    let items = t.on_update(&end);
    assert_eq!(result(&items)["content"], "");
    assert!(result(&items).get("exitCode").is_none());
    let mut anonymous = done("", 7);
    anonymous["_meta"]["terminal_output_delta"] = json!({"terminal_id":"","data":"unattributable"});
    let items = t.on_update(&anonymous);
    assert_eq!(result(&items)["content"], "");
    assert!(result(&items).get("exitCode").is_none());
}

#[test]
fn interleaved_calls_and_reused_ids_do_not_share_output() {
    let mut t = TranscriptTranslator::new(false);
    t.on_update(&start("a"));
    t.on_update(&start("b"));
    t.on_update(&delta("a", "alpha"));
    t.on_update(&delta("b", "beta"));
    assert_eq!(result(&t.on_update(&done("b", 0)))["content"], "beta");
    assert_eq!(result(&t.on_update(&done("a", 0)))["content"], "alpha");
    t.on_update(&delta("a", "late"));
    assert_eq!(result(&t.on_update(&done("a", 0)))["content"], "");
    t.on_update(&start("a"));
    t.on_update(&delta("a", "new"));
    assert_eq!(result(&t.on_update(&done("a", 0)))["content"], "new");
}

fn assert_complete_elision(original: &str, bounded: &str) {
    let marker_at = bounded.find("…[elided ").expect("marker");
    let retained = &bounded[..marker_at];
    assert!(original.starts_with(retained));
    let omitted = &original[retained.len()..];
    assert_eq!(&bounded[marker_at..], elision_marker(omitted));
}

#[test]
fn bound_text_marker_accounts_for_every_displaced_byte_including_marker_room() {
    for original in ["x".repeat(47_360), "🐝".repeat(4096), "a".repeat(1000)] {
        for limit in [100, 300, 8192] {
            if original.len() <= limit {
                continue;
            }
            let bounded = bound_text(&original, limit);
            assert!(bounded.len() <= limit);
            assert_complete_elision(&original, &bounded);
        }
    }
}

#[test]
fn streamed_large_utf8_output_has_complete_digest_and_bounded_storage() {
    for chunks in [
        vec!["🐝".repeat(2000), "z".repeat(20_000), "🐝".repeat(3000)],
        vec!["x".repeat(8190), "🐝".repeat(100), "tail".into()],
        vec!["🐝".repeat(10_000), "tail".into()],
    ] {
        let mut t = TranscriptTranslator::new(false);
        t.on_update(&start("a"));
        for chunk in &chunks {
            t.on_update(&delta("a", chunk));
            let output = t.tools["a"].terminal_output.as_ref().expect("buffer");
            assert!(output.prefix.len() <= MAX_TOOL_CONTENT_BYTES);
        }
        let items = t.on_update(&done("a", 0));
        let content = result(&items)["content"].as_str().expect("content");
        assert!(content.len() <= MAX_TOOL_CONTENT_BYTES);
        assert_complete_elision(&chunks.concat(), content);
        assert!(t.tools["a"].terminal_output.is_none());
    }
}

#[test]
fn streamed_private_paths_are_redacted_by_normal_publish_seam() {
    let mut t = TranscriptTranslator::new(false);
    t.on_update(&start("a"));
    t.on_update(&delta("a", "failed at /Users/example/"));
    t.on_update(&delta("a", "private/secret.txt\n"));
    let items = t.on_update(&done("a", 1));
    let fitted = fit_item(result(&items).clone(), 512, 32 * 1024);
    let text = fitted["content"].as_str().expect("text");
    assert!(!text.contains("/Users/example"));
    assert!(text.contains("[elided private context:"));
    assert_eq!(fitted["exitCode"], 1);
}

#[test]
fn nonterminal_command_raw_exit_is_observed_not_parsed_from_text() {
    let mut t = TranscriptTranslator::new(false);
    t.on_update(&start("a"));
    let end = json!({"sessionUpdate":"tool_call_update","toolCallId":"a",
        "status":"failed","rawOutput":{"exit_code":7}});
    assert_eq!(result(&t.on_update(&end))["exitCode"], 7);
    t.on_update(&start("b"));
    let end = json!({"sessionUpdate":"tool_call_update","toolCallId":"b",
        "status":"failed","content":[{"type":"content","content":{"type":"text","text":"Exit code 7"}}]});
    assert!(result(&t.on_update(&end)).get("exitCode").is_none());
}

#[test]
fn terminal_snapshot_replaces_stream_instead_of_duplicating_it() {
    let mut t = TranscriptTranslator::new(false);
    t.on_update(&start("a"));
    t.on_update(&delta("a", "same output"));
    let mut end = done("a", 0);
    end["content"] = json!([{"type":"content","content":{"type":"text","text":"same output"}}]);
    assert_eq!(result(&t.on_update(&end))["content"], "same output");
}

#[test]
fn terminal_output_alias_appends_and_dual_keys_do_not_double_count() {
    let mut t = TranscriptTranslator::new(false);
    t.on_update(&start("a"));
    let mut frame = delta("a", "first");
    frame["_meta"]["terminal_output"] = frame["_meta"]["terminal_output_delta"].clone();
    t.on_update(&frame);
    let mut frame = delta("a", "second");
    frame["_meta"]["terminal_output"] = frame["_meta"]
        .as_object_mut()
        .expect("meta")
        .remove("terminal_output_delta")
        .expect("delta");
    t.on_update(&frame);
    assert_eq!(
        result(&t.on_update(&done("a", 0)))["content"],
        "firstsecond"
    );
}

#[test]
fn subagent_terminal_output_keeps_parent_attribution() {
    let mut t = TranscriptTranslator::new(false);
    for mut frame in [start("a"), delta("a", "worker output")] {
        frame["_meta"]["claudeCode"] = json!({"parentToolUseId":"parent"});
        t.on_update(&frame);
    }
    let mut end = done("a", 0);
    end["_meta"]["claudeCode"] = json!({"parentToolUseId":"parent"});
    let items = t.on_update(&end);
    assert_eq!(result(&items)["content"], "worker output");
    assert_eq!(result(&items)["parentToolId"], "parent");
}

#[test]
fn malformed_and_mcp_raw_exit_fields_are_not_shell_exit_observations() {
    for raw in [
        json!({"exit_code":"7"}),
        json!({"exit_code":7,"business":"payload"}),
    ] {
        let mut t = TranscriptTranslator::new(false);
        t.on_update(&start("a"));
        let end = json!({"sessionUpdate":"tool_call_update","toolCallId":"a","status":"failed","rawOutput":raw});
        assert!(result(&t.on_update(&end)).get("exitCode").is_none());
    }
    let mut t = TranscriptTranslator::new(false);
    t.on_update(&start("a"));
    let end = json!({"sessionUpdate":"tool_call_update","toolCallId":"a","status":"completed",
        "rawOutput":{"exit_code":7},"_meta":{"is_mcp_tool_call":true}});
    assert!(result(&t.on_update(&end)).get("exitCode").is_none());
}

#[test]
fn bound_text_tiny_budgets_never_emit_a_partial_or_oversized_marker() {
    for original in ["🐝".repeat(100), "x".repeat(10_000)] {
        for limit in 0..=110 {
            let bounded = bound_text(&original, limit);
            assert!(bounded.len() <= limit, "budget {limit}");
            if bounded.is_empty() {
                assert!(elision_marker(&original).len() > limit);
            } else {
                assert_complete_elision(&original, &bounded);
            }
        }
    }
    assert_eq!(bound_text("🐝", 4), "🐝");
    assert_eq!(bound_text("", 0), "");
}

#[test]
fn declared_terminal_identity_is_bound_to_one_call_and_cannot_be_rebound() {
    let mut t = TranscriptTranslator::new(false);
    let mut opening = start("call");
    opening["_meta"]["terminal_info"]["terminal_id"] = json!("terminal");
    t.on_update(&opening);
    let mut chunk = delta("call", "mapped output");
    chunk["_meta"]["terminal_output_delta"]["terminal_id"] = json!("terminal");
    t.on_update(&chunk);
    // Neither the call-id fallback nor a later declaration can override the
    // exact terminal association observed on the opening frame.
    let mut wrong = delta("call", "wrong output");
    wrong["_meta"]["terminal_info"] = json!({"terminal_id":"call"});
    t.on_update(&wrong);
    let mut end = done("call", 7);
    end["_meta"]["terminal_exit"]["terminal_id"] = json!("terminal");
    let items = t.on_update(&end);
    assert_eq!(result(&items)["content"], "mapped output");
    assert_eq!(result(&items)["exitCode"], 7);

    // Reusing a call id starts a fresh association. Its previous terminal
    // cannot contribute either output or a numeric exit to the new call.
    t.on_update(&start("call"));
    t.on_update(&chunk);
    let items = t.on_update(&end);
    assert_eq!(result(&items)["content"], "");
    assert!(result(&items).get("exitCode").is_none());
}

#[test]
fn absent_terminal_declaration_uses_only_the_observed_call_id() {
    let mut t = TranscriptTranslator::new(false);
    let mut opening = start("call");
    opening.as_object_mut().expect("opening").remove("_meta");
    t.on_update(&opening);
    t.on_update(&delta("call", "fallback output"));
    let items = t.on_update(&done("call", 0));
    assert_eq!(result(&items)["content"], "fallback output");
    assert_eq!(result(&items)["exitCode"], 0);
}
