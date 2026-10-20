//! A plugin that misbehaves on request. Each operation is one fault the host
//! must contain; `echo` is the well-behaved control, and reports what the
//! host let it see so tests can check scoping and identity.

wit_bindgen::generate!({
    path: "../../wit/plugin",
    world: "operation-provider",
});

use beekeeper::plugin::{log, records, types};
use exports::beekeeper::plugin::operations::Guest;
use serde_json::json;

struct Faults;

const KIND: u32 = 44248;

const OPS: &[(&str, bool)] = &[
    ("echo", false),
    ("spin", false),
    ("panic", false),
    ("grow", false),
    ("read-ungranted-kind", false),
    ("query-flood", false),
    ("drafts-from-a-read", false),
    ("huge-result", false),
    ("not-json", false),
    ("draft-ungranted-kind", true),
    ("draft-other-scope", true),
    ("draft-invalid-op", true),
    ("draft-far-future", true),
    ("draft-ok", true),
];

fn draft(tags: Vec<Vec<String>>, content: &str, kind: u32, created_at: u64) -> types::EventDraft {
    types::EventDraft {
        kind,
        created_at,
        tags,
        content: content.to_owned(),
        group: "fault".to_owned(),
        if_refused: types::IfRefused::Abort,
        partial_summary: None,
    }
}

/// A valid NIP-TD `list.create` in `coordinate`.
fn list_create(coordinate: &str, now: u64) -> types::EventDraft {
    let list = "abcdefabcdefabcdefabcdefabcdefab";
    let content = json!({
        "schema": "buzz-project-todo/v1", "op": "list.create", "listId": list,
        "title": "From the fault plugin", "visibility": "project",
    })
    .to_string();
    let tags = vec![
        vec!["a".to_owned(), coordinate.to_owned()],
        vec!["td-v".to_owned(), "td1-1".to_owned()],
        vec!["td-op".to_owned(), "list.create".to_owned()],
        vec!["td-list".to_owned(), list.to_owned()],
        vec!["td-vis".to_owned(), "project".to_owned()],
    ];
    draft(tags, &content, KIND, now)
}

impl Guest for Faults {
    fn describe() -> types::Manifest {
        types::Manifest {
            name: "test-faults".to_owned(),
            version: "0.1.0-test".to_owned(),
            api: vec!["beekeeper.test-faults/1".to_owned()],
            operations: OPS
                .iter()
                .map(|(name, draft)| types::OperationDecl {
                    name: (*name).to_owned(),
                    effect: if *draft {
                        types::OpEffect::Draft
                    } else {
                        types::OpEffect::Read
                    },
                    summary: "fault".to_owned(),
                })
                .collect(),
            read_kinds: vec![KIND],
            draft_kinds: vec![KIND],
        }
    }

    fn invoke(inv: types::Invocation) -> Result<types::OpOutput, types::OpError> {
        let types::Scope::Project(coordinate) = inv.scope.clone();
        let ok = |result: serde_json::Value| {
            Ok(types::OpOutput {
                result_json: result.to_string(),
                drafts: vec![],
            })
        };
        match inv.operation.as_str() {
            "echo" => {
                log::log(log::Level::Info, "echo ran");
                let page = records::query(&types::RecordQuery {
                    kinds: vec![KIND],
                    until: None,
                    limit: 1000,
                })
                .map_err(types::OpError::Host)?;
                let mut coordinates: Vec<String> = page
                    .records
                    .iter()
                    .flat_map(|r| r.tags.iter())
                    .filter(|t| t.first().map(String::as_str) == Some("a"))
                    .filter_map(|t| t.get(1).cloned())
                    .collect();
                coordinates.sort();
                coordinates.dedup();
                let authors: Vec<String> = page.records.iter().map(|r| r.pubkey.clone()).collect();
                ok(json!({
                    "caller": inv.caller,
                    "scope": coordinate,
                    "access": matches!(inv.access, types::Access::Write),
                    "input": serde_json::from_str::<serde_json::Value>(&inv.input_json).ok(),
                    "records": page.records.len(),
                    "ids": page.records.iter().map(|r| r.id.clone()).collect::<Vec<_>>(),
                    "coordinates": coordinates,
                    "authors": authors,
                    "entropyBytes": inv.entropy.len(),
                }))
            }
            "spin" => {
                let mut n: u64 = 0;
                loop {
                    n = std::hint::black_box(n.wrapping_add(1));
                }
            }
            "panic" => panic!("the fault plugin panicked on purpose"),
            "grow" => {
                let mut hold: Vec<Vec<u8>> = Vec::new();
                loop {
                    hold.push(vec![7u8; 1 << 20]);
                    std::hint::black_box(&hold);
                }
            }
            "read-ungranted-kind" => {
                let error = records::query(&types::RecordQuery {
                    kinds: vec![1],
                    until: None,
                    limit: 10,
                })
                .err();
                ok(json!({ "denied": matches!(error, Some(types::HostError::Denied(_))) }))
            }
            "query-flood" => {
                let mut served = 0u32;
                loop {
                    match records::query(&types::RecordQuery {
                        kinds: vec![KIND],
                        until: None,
                        limit: 1,
                    }) {
                        Ok(_) => served += 1,
                        Err(types::HostError::BudgetExhausted(_)) => {
                            return ok(json!({ "served": served, "stopped": "budget" }))
                        }
                        Err(_) => return ok(json!({ "served": served, "stopped": "other" })),
                    }
                    if served > 100_000 {
                        return ok(json!({ "served": served, "stopped": "never" }));
                    }
                }
            }
            "drafts-from-a-read" => Ok(types::OpOutput {
                result_json: "{}".to_owned(),
                drafts: vec![list_create(&coordinate, inv.now)],
            }),
            "huge-result" => ok(json!({ "pad": "x".repeat(8 * 1024 * 1024) })),
            "not-json" => Ok(types::OpOutput {
                result_json: "not json".to_owned(),
                drafts: vec![],
            }),
            "draft-ungranted-kind" => {
                let mut d = list_create(&coordinate, inv.now);
                d.kind = 1;
                Ok(types::OpOutput {
                    result_json: "{}".to_owned(),
                    drafts: vec![d],
                })
            }
            "draft-other-scope" => Ok(types::OpOutput {
                result_json: "{}".to_owned(),
                drafts: vec![list_create(
                    "30621:9999999999999999999999999999999999999999999999999999999999999999:elsewhere",
                    inv.now,
                )],
            }),
            "draft-invalid-op" => {
                let mut d = list_create(&coordinate, inv.now);
                d.tags.push(vec!["h".to_owned(), "channel".to_owned()]);
                Ok(types::OpOutput {
                    result_json: "{}".to_owned(),
                    drafts: vec![d],
                })
            }
            "draft-far-future" => Ok(types::OpOutput {
                result_json: "{}".to_owned(),
                drafts: vec![list_create(&coordinate, inv.now + 3_600)],
            }),
            "draft-ok" => Ok(types::OpOutput {
                result_json: "{}".to_owned(),
                drafts: vec![list_create(&coordinate, inv.now)],
            }),
            other => Err(types::OpError::Internal(format!("no operation {other}"))),
        }
    }
}

export!(Faults);
