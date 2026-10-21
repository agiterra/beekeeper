//! Project to-do ops (kind 44248) — the codec and validator live in
//! [`beekeeper_nip_td::project_todo`], which builds for WebAssembly; this
//! module re-exports them unchanged and adds the one entry point that takes
//! a signed `nostr::Event` apart.

pub use beekeeper_nip_td::project_todo::*;
pub use beekeeper_nip_td::{
    check_draft_created_at, DRAFT_FUTURE_SECS, DRAFT_PAST_SECS, INGEST_TIMESTAMP_DRIFT_SECS,
};

use crate::kind::project_a_scoped_coordinate;

// What the tests below reached through the module's own imports before the
// codec moved out.
#[cfg(test)]
use crate::kind::KIND_PROJECT_TODO_OP;
#[cfg(test)]
use serde_json::{Map, Value};
#[cfg(test)]
use std::str::FromStr;

/// Validate a signed op end to end: kind, tag grammar, canonical project
/// coordinate, content envelope, and tag/content agreement.
///
/// This is the single validator ([`validate_project_todo_parts`] applied to
/// the event's parts). The relay calls it and defines no local copy; the
/// buzz-sdk builder delegates to it rather than repeating the rules.
pub fn validate_project_todo_envelope(event: &nostr::Event) -> Result<ProjectTodoOp, String> {
    let tags: Vec<Vec<String>> = event
        .tags
        .iter()
        .map(|tag| tag.as_slice().to_vec())
        .collect();
    let op =
        validate_project_todo_parts(crate::kind::event_kind_u32(event), &tags, &event.content)?;
    // The extractor and the parts check agree by construction; this keeps
    // the validator honest if either changes.
    let coordinate = tags
        .iter()
        .find(|tag| tag.first().map(String::as_str) == Some("a"))
        .and_then(|tag| tag.get(1))
        .cloned();
    if project_a_scoped_coordinate(event) != coordinate {
        return Err("todo op a tag did not resolve to its coordinate".to_owned());
    }
    Ok(op)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::{EventBuilder, Keys, Kind, Tag};

    const COORD: &str =
        "30621:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa:tank-loop";
    const LIST: &str = "0123456789abcdef0123456789abcdef";
    const ITEM: &str = "fedcba9876543210fedcba9876543210";

    fn add_op() -> ProjectTodoOp {
        ProjectTodoOp {
            list_id: LIST.to_owned(),
            visibility: TodoVisibility::Project,
            value: ProjectTodoOpValue::ItemAdd {
                item_id: ITEM.to_owned(),
                text: "Write the NIP".to_owned(),
                rank: "a0".to_owned(),
            },
        }
    }

    fn event_with(content: &str, tags: &[&[&str]]) -> nostr::Event {
        let keys = Keys::generate();
        let tag_vec: Vec<Tag> = tags
            .iter()
            .map(|parts| Tag::parse(parts.iter().copied()).expect("test tag parses"))
            .collect();
        EventBuilder::new(
            Kind::Custom(KIND_PROJECT_TODO_OP as u16),
            content.to_owned(),
        )
        .tags(tag_vec)
        .sign_with_keys(&keys)
        .expect("test event signs")
    }

    fn event_for(op: &ProjectTodoOp) -> nostr::Event {
        let tags = op.tags(COORD);
        let borrowed: Vec<Vec<&str>> = tags
            .iter()
            .map(|t| t.iter().map(String::as_str).collect())
            .collect();
        let slices: Vec<&[&str]> = borrowed.iter().map(Vec::as_slice).collect();
        event_with(&op.to_content(), &slices)
    }

    #[test]
    fn every_op_round_trips_through_content_and_envelope() {
        let ops = vec![
            ProjectTodoOp {
                list_id: LIST.to_owned(),
                visibility: TodoVisibility::Project,
                value: ProjectTodoOpValue::ListCreate {
                    title: "Launch".to_owned(),
                    visibility: TodoVisibility::Project,
                },
            },
            ProjectTodoOp {
                list_id: LIST.to_owned(),
                visibility: TodoVisibility::Project,
                value: ProjectTodoOpValue::ListTitle {
                    title: "Launch v2".to_owned(),
                },
            },
            ProjectTodoOp {
                list_id: LIST.to_owned(),
                visibility: TodoVisibility::Project,
                value: ProjectTodoOpValue::ListArchived { archived: true },
            },
            ProjectTodoOp {
                list_id: LIST.to_owned(),
                visibility: TodoVisibility::Project,
                value: ProjectTodoOpValue::ListPinned { pinned: true },
            },
            ProjectTodoOp {
                list_id: LIST.to_owned(),
                visibility: TodoVisibility::Personal,
                value: ProjectTodoOpValue::ListCreate {
                    title: "Mine".to_owned(),
                    visibility: TodoVisibility::Personal,
                },
            },
            add_op(),
            ProjectTodoOp {
                list_id: LIST.to_owned(),
                visibility: TodoVisibility::Project,
                value: ProjectTodoOpValue::ItemText {
                    item_id: ITEM.to_owned(),
                    text: "Write the NIP\nwith examples".to_owned(),
                },
            },
            ProjectTodoOp {
                list_id: LIST.to_owned(),
                visibility: TodoVisibility::Project,
                value: ProjectTodoOpValue::ItemDone {
                    item_id: ITEM.to_owned(),
                    done: true,
                },
            },
            ProjectTodoOp {
                list_id: LIST.to_owned(),
                visibility: TodoVisibility::Project,
                value: ProjectTodoOpValue::ItemAssignee {
                    item_id: ITEM.to_owned(),
                    assignee: Some("b".repeat(64)),
                },
            },
            ProjectTodoOp {
                list_id: LIST.to_owned(),
                visibility: TodoVisibility::Project,
                value: ProjectTodoOpValue::ItemAssignee {
                    item_id: ITEM.to_owned(),
                    assignee: None,
                },
            },
            ProjectTodoOp {
                list_id: LIST.to_owned(),
                visibility: TodoVisibility::Project,
                value: ProjectTodoOpValue::ItemDue {
                    item_id: ITEM.to_owned(),
                    due: Some("2028-02-29".to_owned()),
                },
            },
            ProjectTodoOp {
                list_id: LIST.to_owned(),
                visibility: TodoVisibility::Project,
                value: ProjectTodoOpValue::ItemDue {
                    item_id: ITEM.to_owned(),
                    due: None,
                },
            },
            ProjectTodoOp {
                list_id: LIST.to_owned(),
                visibility: TodoVisibility::Project,
                value: ProjectTodoOpValue::ItemRank {
                    item_id: ITEM.to_owned(),
                    rank: "a0V".to_owned(),
                },
            },
            ProjectTodoOp {
                list_id: LIST.to_owned(),
                visibility: TodoVisibility::Project,
                value: ProjectTodoOpValue::ItemRemove {
                    item_id: ITEM.to_owned(),
                },
            },
            ProjectTodoOp {
                list_id: LIST.to_owned(),
                visibility: TodoVisibility::Project,
                value: ProjectTodoOpValue::ItemStatus {
                    item_id: ITEM.to_owned(),
                    status: TodoStatus::InProgress,
                },
            },
            ProjectTodoOp {
                list_id: LIST.to_owned(),
                visibility: TodoVisibility::Project,
                value: ProjectTodoOpValue::ItemPriority {
                    item_id: ITEM.to_owned(),
                    priority: TodoPriority::Urgent,
                },
            },
            ProjectTodoOp {
                list_id: LIST.to_owned(),
                visibility: TodoVisibility::Project,
                value: ProjectTodoOpValue::ItemDescription {
                    item_id: ITEM.to_owned(),
                    description: Some("Steps:\n\t1. reproduce".to_owned()),
                },
            },
            ProjectTodoOp {
                list_id: LIST.to_owned(),
                visibility: TodoVisibility::Project,
                value: ProjectTodoOpValue::ItemDescription {
                    item_id: ITEM.to_owned(),
                    description: None,
                },
            },
        ];
        assert_eq!(ops.len(), ProjectTodoOpKind::ALL.len() + 4);
        for op in ops {
            let decoded =
                decode_project_todo_op(&op.to_content(), op.visibility).expect("content decodes");
            assert_eq!(decoded, op);
            let validated = validate_project_todo_envelope(&event_for(&op)).expect("envelope");
            assert_eq!(validated, op);
            assert_eq!(
                ProjectTodoOpKind::from_str(op.kind().as_str()),
                Ok(op.kind())
            );
        }
    }

    #[test]
    fn content_key_set_is_exact() {
        let content = add_op().to_content();
        let mut object: Map<String, Value> = serde_json::from_str(&content).unwrap();
        object.insert("priority".into(), Value::from("high"));
        let err = decode_project_todo_op(
            &Value::Object(object.clone()).to_string(),
            TodoVisibility::Project,
        )
        .unwrap_err();
        assert!(err.contains("unsupported field \"priority\""), "{err}");

        object.remove("priority");
        object.remove("rank");
        let err =
            decode_project_todo_op(&Value::Object(object).to_string(), TodoVisibility::Project)
                .unwrap_err();
        assert!(err.contains("missing field \"rank\""), "{err}");

        // Absent is not null: an assignee op must say null explicitly.
        let missing_assignee = format!(
            r#"{{"schema":"{PROJECT_TODO_SCHEMA}","op":"item.assignee","listId":"{LIST}","itemId":"{ITEM}"}}"#
        );
        assert!(
            decode_project_todo_op(&missing_assignee, TodoVisibility::Project)
                .unwrap_err()
                .contains("missing field \"assignee\"")
        );
    }

    #[test]
    fn rejects_bad_values() {
        let bad = |patch: &str| {
            let mut object: Map<String, Value> =
                serde_json::from_str(&add_op().to_content()).unwrap();
            let (k, v): (String, Value) = serde_json::from_str(patch).unwrap();
            object.insert(k, v);
            decode_project_todo_op(&Value::Object(object).to_string(), TodoVisibility::Project)
                .unwrap_err()
        };
        assert!(bad(r#"["listId","short"]"#).contains("listId"));
        assert!(bad(r#"["itemId","FEDCBA9876543210FEDCBA9876543210"]"#).contains("itemId"));
        assert!(bad(r#"["text","   "]"#).contains("blank"));
        assert!(bad("[\"text\",\"a\\u0007b\"]").contains("control"));
        assert!(bad(r#"["rank","a0V0"]"#).contains("rank"));
        assert!(bad(r#"["schema","buzz-project-todo/v0"]"#).contains("schema"));
        assert!(bad(r#"["op","item.rename"]"#).contains("unknown project todo op"));

        let long = "x".repeat(MAX_PROJECT_TODO_TEXT_BYTES + 1);
        assert!(validate_todo_text("text", &long).is_err());
        let oversize = format!(
            r#"{{"schema":"{PROJECT_TODO_SCHEMA}","op":"item.text","listId":"{LIST}","itemId":"{ITEM}","text":"{}"}}"#,
            "y".repeat(MAX_PROJECT_TODO_CONTENT_BYTES)
        );
        assert!(decode_project_todo_op(&oversize, TodoVisibility::Project)
            .unwrap_err()
            .contains("exceeds"));
    }

    #[test]
    fn due_dates_are_real_calendar_days() {
        for ok in [
            "1970-01-01",
            "2026-02-28",
            "2024-02-29",
            "2000-02-29",
            "9999-12-31",
        ] {
            validate_due_date(ok).unwrap_or_else(|e| panic!("{ok}: {e}"));
        }
        for bad in [
            "2026-2-28",
            "2026-02-30",
            "2026-13-01",
            "2100-02-29",
            "1969-12-31",
            "2026/02/28",
            "2026-02-28T00:00",
            "",
        ] {
            assert!(validate_due_date(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn assignee_must_be_a_lowercase_pubkey() {
        let content = |assignee: &str| {
            format!(
                r#"{{"schema":"{PROJECT_TODO_SCHEMA}","op":"item.assignee","listId":"{LIST}","itemId":"{ITEM}","assignee":{assignee}}}"#
            )
        };
        assert!(decode_project_todo_op(&content("null"), TodoVisibility::Project).is_ok());
        assert!(decode_project_todo_op(
            &content(&format!("\"{}\"", "c".repeat(64))),
            TodoVisibility::Project
        )
        .is_ok());
        assert!(decode_project_todo_op(
            &content(&format!("\"{}\"", "C".repeat(64))),
            TodoVisibility::Project
        )
        .is_err());
        assert!(decode_project_todo_op(&content("\"abc\""), TodoVisibility::Project).is_err());
        assert!(decode_project_todo_op(&content("7"), TodoVisibility::Project).is_err());
    }

    #[test]
    fn tag_grammar_is_closed_and_agrees_with_content() {
        let op = add_op();
        let content = op.to_content();
        let ok = validate_project_todo_envelope(&event_with(
            &content,
            &[
                &["td-item", ITEM],
                &["td-list", LIST],
                &["td-vis", "project"],
                &["td-op", "item.add"],
                &["td-v", PROJECT_TODO_TAG_VERSION],
                &["a", COORD],
            ],
        ));
        assert!(ok.is_ok(), "tag order is free: {ok:?}");

        let cases: Vec<(&str, Vec<&[&str]>)> = vec![
            ("requires one a tag", vec![&["td-v", "td1-1"], &["td-op", "item.add"], &["td-list", LIST], &["td-vis", "project"], &["td-item", ITEM]]),
            ("canonical", vec![&["a", "30621:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA:tank-loop"], &["td-v", "td1-1"], &["td-op", "item.add"], &["td-list", LIST], &["td-vis", "project"], &["td-item", ITEM]]),
            ("tag version", vec![&["a", COORD], &["td-v", "td1-2"], &["td-op", "item.add"], &["td-list", LIST], &["td-vis", "project"], &["td-item", ITEM]]),
            ("does not match content op", vec![&["a", COORD], &["td-v", "td1-1"], &["td-op", "item.text"], &["td-list", LIST], &["td-vis", "project"], &["td-item", ITEM]]),
            ("does not match content listId", vec![&["a", COORD], &["td-v", "td1-1"], &["td-op", "item.add"], &["td-list", ITEM], &["td-vis", "project"], &["td-item", ITEM]]),
            ("does not match content itemId", vec![&["a", COORD], &["td-v", "td1-1"], &["td-op", "item.add"], &["td-list", LIST], &["td-vis", "project"], &["td-item", LIST]]),
            ("requires one td-item tag", vec![&["a", COORD], &["td-v", "td1-1"], &["td-op", "item.add"], &["td-list", LIST], &["td-vis", "project"]]),
            ("requires one td-vis tag", vec![&["a", COORD], &["td-v", "td1-1"], &["td-op", "item.add"], &["td-list", LIST], &["td-item", ITEM]]),
            ("visibility must be project or personal", vec![&["a", COORD], &["td-v", "td1-1"], &["td-op", "item.add"], &["td-list", LIST], &["td-vis", "team"], &["td-item", ITEM]]),
            ("must not carry an h tag", vec![&["a", COORD], &["td-v", "td1-1"], &["td-op", "item.add"], &["td-list", LIST], &["td-vis", "project"], &["td-item", ITEM], &["h", "0c3b3f8e-1c1d-4f7e-9a2b-3c4d5e6f7a8b"]]),
            ("unsupported tag key", vec![&["a", COORD], &["td-v", "td1-1"], &["td-op", "item.add"], &["td-list", LIST], &["td-vis", "project"], &["td-item", ITEM], &["p", "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"]]),
            ("more than one a tag", vec![&["a", COORD], &["a", COORD], &["td-v", "td1-1"], &["td-op", "item.add"], &["td-list", LIST], &["td-vis", "project"], &["td-item", ITEM]]),
            ("exactly two fields", vec![&["a", COORD, "extra"], &["td-v", "td1-1"], &["td-op", "item.add"], &["td-list", LIST], &["td-vis", "project"], &["td-item", ITEM]]),
        ];
        for (needle, tags) in cases {
            let err = validate_project_todo_envelope(&event_with(&content, &tags)).unwrap_err();
            assert!(err.contains(needle), "expected {needle:?} in {err:?}");
        }

        // A list op must not carry td-item.
        let list_op = ProjectTodoOp {
            list_id: LIST.to_owned(),
            visibility: TodoVisibility::Project,
            value: ProjectTodoOpValue::ListArchived { archived: false },
        };
        let err = validate_project_todo_envelope(&event_with(
            &list_op.to_content(),
            &[
                &["a", COORD],
                &["td-v", "td1-1"],
                &["td-op", "list.archived"],
                &["td-list", LIST],
                &["td-vis", "project"],
                &["td-item", ITEM],
            ],
        ))
        .unwrap_err();
        assert!(err.contains("must not carry a td-item tag"), "{err}");
    }

    #[test]
    fn create_visibility_must_match_its_tag() {
        let content = format!(
            r#"{{"schema":"{PROJECT_TODO_SCHEMA}","op":"list.create","listId":"{LIST}","title":"Mine","visibility":"personal"}}"#
        );
        assert!(decode_project_todo_op(&content, TodoVisibility::Personal).is_ok());
        let err = decode_project_todo_op(&content, TodoVisibility::Project).unwrap_err();
        assert!(err.contains("does not match its td-vis tag"), "{err}");
        let err = validate_project_todo_envelope(&event_with(
            &content,
            &[
                &["a", COORD],
                &["td-v", "td1-1"],
                &["td-op", "list.create"],
                &["td-list", LIST],
                &["td-vis", "project"],
            ],
        ))
        .unwrap_err();
        assert!(err.contains("does not match its td-vis tag"), "{err}");
    }

    #[test]
    fn wrong_kind_is_rejected() {
        let keys = Keys::generate();
        let event = EventBuilder::new(Kind::Custom(44240), add_op().to_content())
            .sign_with_keys(&keys)
            .unwrap();
        assert!(validate_project_todo_envelope(&event)
            .unwrap_err()
            .contains("kind 44248"));
    }

    #[test]
    fn planning_fields_are_closed_vocabularies() {
        let op = |body: &str| {
            format!(
                r#"{{"schema":"{PROJECT_TODO_SCHEMA}","listId":"{LIST}","itemId":"{ITEM}",{body}}}"#
            )
        };
        let decode = |body: &str| decode_project_todo_op(&op(body), TodoVisibility::Project);

        for status in TodoStatus::ALL {
            let decoded = decode(&format!(r#""op":"item.status","status":"{status}""#)).unwrap();
            assert_eq!(
                decoded.value,
                ProjectTodoOpValue::ItemStatus {
                    item_id: ITEM.to_owned(),
                    status
                }
            );
        }
        assert!(decode(r#""op":"item.status","status":"started""#)
            .unwrap_err()
            .contains("todo status must be one of"));
        assert!(decode(r#""op":"item.status","status":null"#)
            .unwrap_err()
            .contains("must be a string"));
        assert!(TodoStatus::Done.is_closed() && TodoStatus::Canceled.is_closed());
        assert!(!TodoStatus::InProgress.is_closed());

        for priority in TodoPriority::ALL {
            decode(&format!(r#""op":"item.priority","priority":"{priority}""#)).unwrap();
        }
        // A number is never a priority, so no client's JSON reader has to
        // agree with another's about `1` versus `1.0`.
        assert!(decode(r#""op":"item.priority","priority":1"#)
            .unwrap_err()
            .contains("must be a string"));
        assert!(decode(r#""op":"item.priority","priority":"p1""#)
            .unwrap_err()
            .contains("todo priority must be one of"));

        assert!(decode(r#""op":"item.description""#)
            .unwrap_err()
            .contains("missing field \"description\""));
        assert!(decode(r#""op":"item.description","description":"  ""#)
            .unwrap_err()
            .contains("clear it with null"));
        assert!(
            decode("\"op\":\"item.description\",\"description\":\"a\\u0007b\"")
                .unwrap_err()
                .contains("control")
        );
        let long = "d".repeat(MAX_PROJECT_TODO_DESCRIPTION_BYTES + 1);
        assert!(decode(&format!(
            r#""op":"item.description","description":"{long}""#
        ))
        .unwrap_err()
        .contains("exceeds"));
        let fits = "d".repeat(MAX_PROJECT_TODO_DESCRIPTION_BYTES);
        decode(&format!(
            r#""op":"item.description","description":"{fits}""#
        ))
        .unwrap();
    }
}
