//! `beekeeper-plugin-run` — run one plugin operation through the real host
//! runtime, outside a relay. For plugin authors, and for the desktop's mock
//! end-to-end tests, which drive the real Planning component this way.
//!
//! Reads one JSON request on stdin:
//!
//! ```json
//! { "registry": "target/plugins/registry.toml", "plugin": "planning",
//!   "operation": "read", "input": {}, "caller": "<hex>",
//!   "project": "30621:<hex>:<d>", "access": "write",
//!   "now": 1760000000, "records": [ { "id": …, "pubkey": …, … } ] }
//! ```
//!
//! and writes `{"ok": {...}}` or `{"error": {...}}` on stdout. The record
//! file stands in for the relay's store: the source returns records of the
//! asked kinds whose single `a` tag is the project, newest first. It does not
//! apply the relay's visibility rules — the caller passes only what that
//! reader could see.

use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;

use beekeeper_plugin_host::{
    Access, DraftValidators, Invocation, InvokeError, PluginHost, RecordSource, Registry, Scope,
    SourceError, StoredRecord,
};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    registry: PathBuf,
    plugin: String,
    operation: String,
    #[serde(default)]
    input: Value,
    caller: String,
    project: String,
    access: Access,
    now: Option<u64>,
    #[serde(default)]
    records: Vec<StoredRecord>,
}

struct FileSource {
    project: String,
    records: Vec<StoredRecord>,
}

impl RecordSource for FileSource {
    fn query(
        &self,
        kinds: &[u32],
        until: Option<u64>,
        limit: u32,
    ) -> Result<(Vec<StoredRecord>, bool), SourceError> {
        let mut matching: Vec<&StoredRecord> = self
            .records
            .iter()
            .filter(|r| kinds.contains(&r.kind))
            .filter(|r| until.is_none_or(|u| r.created_at <= u))
            .filter(|r| {
                let a: Vec<&Vec<String>> = r
                    .tags
                    .iter()
                    .filter(|t| t.first().map(String::as_str) == Some("a"))
                    .collect();
                a.len() == 1 && a[0].get(1) == Some(&self.project)
            })
            .collect();
        matching.sort_by(|x, y| (y.created_at, &y.id).cmp(&(x.created_at, &x.id)));
        let more = matching.len() > limit as usize;
        Ok((
            matching.into_iter().take(limit as usize).cloned().collect(),
            more,
        ))
    }
}

/// The validators a relay installs. Kept in step with
/// `beekeeper-relay`'s plugin wiring: one entry per draftable kind.
fn validators() -> DraftValidators {
    let kind = beekeeper_nip_td::KIND_PROJECT_TODO_OP;
    DraftValidators::new().with(
        kind,
        Arc::new(move |tags, content| {
            beekeeper_nip_td::project_todo::validate_project_todo_parts(kind, tags, content)
                .map(|_| ())
        }),
    )
}

fn error_json(error: &InvokeError) -> Value {
    let code = match error {
        InvokeError::NotRegistered(_) => "plugin-not-registered",
        InvokeError::UnknownOperation { .. } => "unknown-operation",
        InvokeError::WriteNotAllowed(_) => "write-not-allowed",
        InvokeError::InputTooLarge { .. } => "input-too-large",
        InvokeError::Busy(_) => "busy",
        InvokeError::Trapped { .. } => "plugin-trapped",
        InvokeError::Operation(_) => "operation-failed",
        InvokeError::ContractViolation(_) => "contract-violation",
        InvokeError::Host(_) => "host-failure",
    };
    let mut body = json!({ "code": code, "message": error.to_string() });
    if let InvokeError::Operation(op) = error {
        body["operation"] = json!(op);
    }
    if let InvokeError::Trapped { kind, .. } = error {
        body["trap"] = json!(kind);
    }
    body
}

fn main() {
    let mut text = String::new();
    let out = (|| -> Result<Value, Value> {
        std::io::stdin()
            .read_to_string(&mut text)
            .map_err(|e| json!({ "code": "bad-request", "message": e.to_string() }))?;
        let request: Request = serde_json::from_str(&text)
            .map_err(|e| json!({ "code": "bad-request", "message": e.to_string() }))?;
        let registry = Registry::from_file(&request.registry)
            .map_err(|e| json!({ "code": "registry", "message": e.to_string() }))?;
        let host = PluginHost::load(&registry, validators())
            .map_err(|e| json!({ "code": "load", "message": e.to_string() }))?;
        let now = request.now.unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
        });
        let mut entropy = [0u8; 32];
        // Not secret, only unique: the clock and pid are enough for test ids.
        let seed = format!("{now}:{}:{}", std::process::id(), text.len());
        entropy
            .copy_from_slice(&hex_bytes(&beekeeper_plugin_host::sha256_hex(seed.as_bytes()))[..32]);
        let source = Arc::new(FileSource {
            project: request.project.clone(),
            records: request.records,
        });
        let loaded = host.get(&request.plugin).map(|p| {
            json!({ "name": p.manifest.name, "version": p.manifest.version, "sha256": p.sha256 })
        });
        let outcome = host
            .invoke(
                &request.plugin,
                Invocation {
                    operation: request.operation,
                    input: request.input,
                    caller: request.caller,
                    scope: Scope::Project(request.project),
                    access: request.access,
                    now,
                    entropy,
                },
                source,
            )
            .map_err(|e| error_json(&e))?;
        Ok(json!({
            "plugin": loaded,
            "result": outcome.result,
            "drafts": outcome.drafts,
            "fuelUsed": outcome.fuel_used,
        }))
    })();
    let body = match out {
        Ok(ok) => json!({ "ok": ok }),
        Err(error) => json!({ "error": error }),
    };
    println!("{body}");
}

fn hex_bytes(hex: &str) -> Vec<u8> {
    (0..hex.len())
        .step_by(2)
        .filter_map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
        .collect()
}
