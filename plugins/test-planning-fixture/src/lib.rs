//! A replacement Planning provider, for tests only. It shares no code with
//! `plugins/planning` — not the NIP-TD crate, not the vocabulary — and is
//! registered, discovered and invoked through exactly the same registry,
//! endpoint, CLI and UI. It serves one fixed issue and refuses every write
//! with an honest `unsupported`, which is what a read-only provider must do.

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
            draft_kinds: vec![],
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

export!(Fixture);
