use super::*;

const CAPABILITIES: &str = include_str!("../../capabilities/default.json");

/// Tauri window patterns are globs with `*`.
fn glob_matches(pattern: &str, label: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == label,
        Some((prefix, suffix)) => {
            label.len() >= prefix.len() + suffix.len()
                && label.starts_with(prefix)
                && label.ends_with(suffix)
        }
    }
}

#[test]
fn popout_label_matches_no_capability() {
    let capability: serde_json::Value =
        serde_json::from_str(CAPABILITIES).expect("capabilities/default.json is valid JSON");
    let windows = capability["windows"]
        .as_array()
        .expect("the capability names its windows");
    let label = popout_label("6f1c1c0e-6a8e-4c38-9d0f-1a2b3c4d5e6f");
    for pattern in windows {
        let pattern = pattern.as_str().expect("a window pattern is a string");
        assert!(
            !glob_matches(pattern, &label),
            "capability window pattern {pattern:?} matches the pop-out label {label:?}"
        );
    }
}

#[test]
fn channel_ids_are_normalized_uuids() {
    assert_eq!(
        normalize_channel_id(" 6F1C1C0E-6A8E-4C38-9D0F-1A2B3C4D5E6F "),
        Ok("6f1c1c0e-6a8e-4c38-9d0f-1a2b3c4d5e6f".to_string())
    );
    let refused = normalize_channel_id("../etc").expect_err("not a uuid");
    assert_eq!(refused.code, "preview_bad_request");
}

fn target(session: &str, generation: u64) -> CodingSessionTarget {
    CodingSessionTarget {
        driver: "claude".into(),
        instance_id: "inst-1".into(),
        session_id: session.into(),
        generation,
    }
}

#[test]
fn state_serializes_to_the_wire_shape() {
    let mut record = PreviewRecord::new("chan");
    record.status = PreviewStatus::Ready;
    record.url = Some("http://localhost:5173/".into());
    record.generation = 3;
    record.has_view = true;
    record.slot = Some(SlotRect {
        x: 0.0,
        y: 0.0,
        width: 100.0,
        height: 100.0,
    });
    record.slot_window = Some("main".into());
    record.binding = Binding::Agent {
        target: target("S", 2),
        execution_id: "exec-1".into(),
    };
    let value = serde_json::to_value(record.state()).expect("serialize");
    assert_eq!(value["channelId"], "chan");
    assert_eq!(value["status"], "ready");
    assert_eq!(value["placement"], "docked");
    assert_eq!(value["canGoBack"], false);
    assert_eq!(value["dataStore"], "per_session");
    assert_eq!(value["freezeFrame"], serde_json::Value::Null);
    assert_eq!(
        value["boundTo"],
        serde_json::json!({"kind": "agent", "executionId": "exec-1", "sessionId": "S", "generation": 2})
    );

    record.binding = Binding::Person {
        target: target("S", 4),
    };
    record.popped = true;
    record.status = PreviewStatus::ClosedByPerson;
    let value = serde_json::to_value(record.state()).expect("serialize");
    assert_eq!(
        value["boundTo"],
        serde_json::json!({"kind": "person", "sessionId": "S", "generation": 4})
    );
    assert_eq!(value["placement"], "popped_out");
    assert_eq!(value["status"], "closed_by_person");

    record.binding = Binding::None;
    record.has_view = false;
    let value = serde_json::to_value(record.state()).expect("serialize");
    assert_eq!(value["boundTo"], serde_json::json!({"kind": "none"}));
    assert_eq!(value["placement"], "none");
}

#[test]
fn unavailable_reasons_carry_the_wire_codes() {
    let value = serde_json::to_value(Unavailable::CONTENT_FILTER_FAILED).expect("serialize");
    assert_eq!(value["code"], "content_filter_failed");
    assert_eq!(Unavailable::NOT_MACOS.code, "not_macos");
    assert_eq!(Unavailable::WEBVIEW_FAILED.code, "webview_failed");
    let error = PreviewError::unavailable(&Unavailable::NOT_MACOS);
    assert_eq!(error.code, "preview_unavailable");
    assert_eq!(error.message, Unavailable::NOT_MACOS.sentence);
}
