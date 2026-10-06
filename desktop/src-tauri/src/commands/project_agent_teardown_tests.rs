use super::execute_impl::open_project_sessions;

fn snapshot(sessions: serde_json::Value) -> serde_json::Value {
    serde_json::json!({ "sessions": sessions })
}

/// The shape the provider actually writes: a map keyed by session id, with
/// the id repeated on the record as `sessionId`.
fn session(id: &str, extra: serde_json::Value) -> (String, serde_json::Value) {
    let mut value = serde_json::json!({ "sessionId": id });
    if let (Some(object), Some(more)) = (value.as_object_mut(), extra.as_object()) {
        for (key, entry) in more {
            object.insert(key.clone(), entry.clone());
        }
    }
    (id.to_string(), value)
}

fn sessions(entries: Vec<(String, serde_json::Value)>) -> serde_json::Value {
    serde_json::Value::Object(entries.into_iter().collect())
}

#[test]
fn an_open_session_for_this_project_is_named() {
    let snap = snapshot(sessions(vec![session(
        "s-1",
        serde_json::json!({ "projectRef": "30621:aa:tank-loop", "role": "lead" }),
    )]));
    assert_eq!(
        open_project_sessions(&snap, "30621:aa:tank-loop"),
        vec!["s-1 (lead)".to_string()],
    );
}

#[test]
fn closed_and_retired_sessions_are_not_open() {
    let snap = snapshot(sessions(vec![
        session(
            "s-1",
            serde_json::json!({ "projectRef": "30621:aa:tank-loop", "closed": true }),
        ),
        session(
            "s-2",
            serde_json::json!({
                "projectRef": "30621:aa:tank-loop",
                "retired": { "at": "now" }
            }),
        ),
        session(
            "s-3",
            serde_json::json!({ "projectRef": "30621:aa:tank-loop" }),
        ),
    ]));
    assert_eq!(
        open_project_sessions(&snap, "30621:aa:tank-loop"),
        vec!["s-3".to_string()],
    );
}

#[test]
fn another_projects_session_is_never_named() {
    let snap = snapshot(sessions(vec![session(
        "s-1",
        serde_json::json!({ "projectRef": "30621:aa:other" }),
    )]));
    assert!(open_project_sessions(&snap, "30621:aa:tank-loop").is_empty());
}

#[test]
fn a_snapshot_without_sessions_reads_as_none_open() {
    // A provider that has never run is the ordinary case, not an error.
    assert!(open_project_sessions(&serde_json::json!({}), "30621:aa:x").is_empty());
}

#[test]
fn the_keys_this_reads_are_the_ones_the_provider_writes() {
    // Pinned against the real `SessionRecord` so a rename upstream fails
    // here rather than silently answering "none open" forever — the same
    // guard `seat_agents_clone` keeps over the same file.
    let fixture = serde_json::json!({
        "sessionId": "74495ca8-2aeb-4f25-9f40-3124be1c476f",
        "generation": 1,
        "channelId": "85b8db75-4b60-4741-bcfa-7f75cc238ff0",
        "commandId": "csl-86e283cf-dd2c-4d6c-a0ee-b23cd1f9a8c1",
        "cwd": "/tmp/checkout",
        "projectRef": "30621:aa:tank-loop",
        "repoRef": null,
        "createdBy": "3d3b7169a13a8311b480bdfce85b4a0c7ff9b185832cbc6e547db7bbcf96c05e",
        "authoritySeq": 7,
        "createdAtMs": 1789903633887u64,
        "nextSeq": 291,
        "closed": false
    });
    let record: beekeeper_session_provider_pkg::state::SessionRecord =
        serde_json::from_value(fixture).expect("the provider's own type must accept this shape");
    let written = serde_json::to_value(&record).expect("serialize");

    // The reader agrees with the type, on the type's own output — so a rename
    // upstream fails here instead of turning this into a permanent
    // "no sessions open" and stranding somebody's running seat silently.
    let snapshot = serde_json::json!({ "sessions": { "s": written } });
    assert_eq!(
        open_project_sessions(&snapshot, "30621:aa:tank-loop"),
        vec!["74495ca8-2aeb-4f25-9f40-3124be1c476f".to_string()],
    );
}
