//! A replacement Planning provider, for tests only. It shares no code with
//! `plugins/planning` — not the NIP-TD crate, not the vocabulary — and is
//! registered, discovered and invoked through exactly the same registry,
//! endpoint, CLI and UI. It serves one fixed issue; `create-issue` drafts its
//! own hand-written NIP-TD ops (proving the draft path is not the first-party
//! plugin's alone); every other write is refused with an honest
//! `unsupported`.

wit_bindgen::generate!({
    path: "../../wit/plugin",
    world: "operation-provider",
});

use beekeeper::plugin::types;
use exports::beekeeper::plugin::operations::Guest;
use serde_json::json;

struct Fixture;

const OPS: &[(&str, bool)] = &[
    ("capabilities", false),
    ("read", false),
    ("activity", false),
    ("create-issue", true),
    ("set-title", true),
    ("set-description", true),
    ("set-status", true),
    ("set-priority", true),
    ("set-assignee", true),
];

impl Guest for Fixture {
    fn describe() -> types::Manifest {
        types::Manifest {
            name: "planning-fixture".to_owned(),
            version: "0.1.0-test".to_owned(),
            api: vec!["beekeeper.planning/1".to_owned()],
            operations: OPS
                .iter()
                .map(|(name, draft)| types::OperationDecl {
                    name: (*name).to_owned(),
                    effect: if *draft {
                        types::OpEffect::Draft
                    } else {
                        types::OpEffect::Read
                    },
                    summary: "fixture".to_owned(),
                })
                .collect(),
            read_kinds: vec![],
            draft_kinds: vec![44248],
        }
    }

    fn invoke(invocation: types::Invocation) -> Result<types::OpOutput, types::OpError> {
        let status = json!({ "id": "open", "label": "Open", "category": "unstarted" });
        let priority = json!({ "id": "p0", "label": "Whenever", "level": "none", "order": 4 });
        let result = match invocation.operation.as_str() {
            "capabilities" => json!({
                "statuses": [status],
                "priorities": [priority],
                "comments": false,
                "createsDefaultContainer": false,
                "defaultContainerTitle": null,
                "provider": {
                    "label": "Fixture",
                    "description": "A read-only test provider. It serves one fixed issue.",
                },
                "personIds": "beekeeper-pubkey",
                "limits": { "titleBytes": 64, "descriptionBytes": 64 },
            }),
            "read" => json!({
                "issues": [{
                    "key": "fixture/1",
                    "containerId": "fixture",
                    "issueId": "1",
                    "title": "Served by the fixture provider",
                    "description": null,
                    "status": status,
                    "priority": priority,
                    "assignee": null,
                    "due": null,
                    "rank": "a0",
                    "createdAt": invocation.now,
                    "createdBy": invocation.caller,
                    "updatedAt": invocation.now,
                    "completedAt": null,
                }],
                "containers": [{ "id": "fixture", "title": "Fixture" }],
                "truncated": false,
                "unreadable": 0,
                "people": [],
            }),
            "activity" => json!({ "entries": [] }),
            "create-issue" => return create(&invocation),
            _ => {
                return Err(types::OpError::Unsupported(
                    "The fixture provider is read-only.".to_owned(),
                ))
            }
        };
        Ok(types::OpOutput {
            result_json: result.to_string(),
            drafts: vec![],
        })
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A new list "Fixture inbox" and one item in it, as two NIP-TD ops written
/// by hand — no shared code with the first-party plugin.
fn create(invocation: &types::Invocation) -> Result<types::OpOutput, types::OpError> {
    let input: serde_json::Value = serde_json::from_str(&invocation.input_json)
        .map_err(|e| types::OpError::InvalidInput(e.to_string()))?;
    let title = input["title"]
        .as_str()
        .filter(|t| !t.trim().is_empty() && t.len() <= 64)
        .ok_or_else(|| types::OpError::InvalidInput("title: 1-64 bytes".to_owned()))?;
    let types::Scope::Project(coordinate) = &invocation.scope;
    let list = hex(&invocation.entropy[..16]);
    let item = hex(&invocation.entropy[16..32]);
    let tags = |op: &str, item_id: Option<&str>| {
        let mut tags = vec![
            vec!["a".to_owned(), coordinate.clone()],
            vec!["td-v".to_owned(), "td1-1".to_owned()],
            vec!["td-op".to_owned(), op.to_owned()],
            vec!["td-list".to_owned(), list.clone()],
            vec!["td-vis".to_owned(), "project".to_owned()],
        ];
        if let Some(id) = item_id {
            tags.push(vec!["td-item".to_owned(), id.to_owned()]);
        }
        tags
    };
    let draft =
        |op: &str, item_id: Option<&str>, content: serde_json::Value, at: u64| types::EventDraft {
            kind: 44248,
            created_at: at,
            tags: tags(op, item_id),
            content: content.to_string(),
            group: "issue".to_owned(),
            if_refused: types::IfRefused::Abort,
            partial_summary: None,
        };
    let now = invocation.now;
    Ok(types::OpOutput {
        result_json:
            json!({ "key": format!("{list}/{item}"), "containerId": list, "issueId": item })
                .to_string(),
        drafts: vec![
            draft(
                "list.create",
                None,
                json!({ "schema": "buzz-project-todo/v1", "op": "list.create", "listId": list,
                        "title": "Fixture inbox", "visibility": "project" }),
                now,
            ),
            draft(
                "item.add",
                Some(&item),
                json!({ "schema": "buzz-project-todo/v1", "op": "item.add", "listId": list,
                        "itemId": item, "text": title, "rank": "a0" }),
                now + 1,
            ),
        ],
    })
}

export!(Fixture);
