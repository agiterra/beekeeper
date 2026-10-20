//! Component glue: `beekeeper:plugin@1` `operation-provider` over the logic
//! in the crate root. Nothing planning-specific lives here.

use beekeeper_nip_td::fold::TodoFoldEvent;
use beekeeper_nip_td::KIND_PROJECT_TODO_OP;

wit_bindgen::generate!({
    path: "../../wit/plugin",
    world: "operation-provider",
});

use beekeeper::plugin::{records, types};
use exports::beekeeper::plugin::operations::Guest;

struct Planning;

struct HostRecords;

impl crate::Records for HostRecords {
    fn page(
        &mut self,
        until: Option<u64>,
        limit: u32,
    ) -> Result<(Vec<TodoFoldEvent>, bool), crate::Error> {
        let page = records::query(&types::RecordQuery {
            kinds: vec![KIND_PROJECT_TODO_OP],
            until,
            limit,
        })
        .map_err(|error| crate::Error::Host(host_message(error)))?;
        Ok((
            page.records
                .into_iter()
                .map(|r| TodoFoldEvent {
                    id: r.id,
                    pubkey: r.pubkey,
                    created_at: r.created_at,
                    kind: r.kind,
                    tags: r.tags,
                    content: r.content,
                })
                .collect(),
            page.more,
        ))
    }
}

fn host_message(error: types::HostError) -> String {
    match error {
        types::HostError::Denied(m) => format!("denied: {m}"),
        types::HostError::BudgetExhausted(m) => format!("budget exhausted: {m}"),
        types::HostError::Unavailable(m) => format!("unavailable: {m}"),
        types::HostError::Invalid(m) => format!("invalid: {m}"),
    }
}

impl Guest for Planning {
    fn describe() -> types::Manifest {
        types::Manifest {
            name: crate::NAME.to_owned(),
            version: crate::VERSION.to_owned(),
            api: vec![crate::API.to_owned()],
            operations: crate::OPERATIONS
                .iter()
                .map(|op| types::OperationDecl {
                    name: op.name.to_owned(),
                    effect: if op.draft {
                        types::OpEffect::Draft
                    } else {
                        types::OpEffect::Read
                    },
                    summary: op.summary.to_owned(),
                })
                .collect(),
            read_kinds: vec![KIND_PROJECT_TODO_OP],
            draft_kinds: vec![KIND_PROJECT_TODO_OP],
        }
    }

    fn invoke(invocation: types::Invocation) -> Result<types::OpOutput, types::OpError> {
        let types::Scope::Project(coordinate) = invocation.scope;
        let mut entropy = [0u8; 32];
        if invocation.entropy.len() != 32 {
            return Err(types::OpError::Internal(
                "the host must supply 32 bytes of entropy".to_owned(),
            ));
        }
        entropy.copy_from_slice(&invocation.entropy);
        let input: serde_json::Value = serde_json::from_str(&invocation.input_json)
            .map_err(|e| types::OpError::InvalidInput(format!("input is not JSON: {e}")))?;
        let ctx = crate::Context {
            caller: invocation.caller,
            coordinate,
            now: invocation.now,
            entropy,
        };
        let output = crate::invoke(&invocation.operation, &input, &ctx, &mut HostRecords).map_err(
            |error| match error {
                crate::Error::InvalidInput(m) => types::OpError::InvalidInput(m),
                crate::Error::NotFound(m) => types::OpError::NotFound(m),
                crate::Error::Conflict(m) => types::OpError::Conflict(m),
                crate::Error::Unsupported(m) => types::OpError::Unsupported(m),
                crate::Error::Host(m) => types::OpError::Host(types::HostError::Unavailable(m)),
                crate::Error::Internal(m) => types::OpError::Internal(m),
            },
        )?;
        Ok(types::OpOutput {
            result_json: output.result.to_string(),
            drafts: output
                .drafts
                .into_iter()
                .map(|d| types::EventDraft {
                    kind: d.kind,
                    created_at: d.created_at,
                    tags: d.tags,
                    content: d.content,
                    group: d.group,
                    if_refused: match d.if_refused {
                        crate::IfRefused::Abort => types::IfRefused::Abort,
                        crate::IfRefused::SkipGroup => types::IfRefused::SkipGroup,
                    },
                    partial_summary: d.partial_summary,
                })
                .collect(),
        })
    }
}

export!(Planning);
