//! Plugin operations over HTTP: the relay side of `beekeeper:plugin@1`.
//!
//! - `GET /plugins` — what this relay runs: name, version, digest, the
//!   operation APIs each implements and its operations.
//! - `POST /plugins/{name}/invoke` — one operation, body
//!   `{"operation": "...", "scope": {"project": "30621:…"}, "input": …}`.
//!
//! Both are NIP-98 authenticated, tenant-bound by the request's host, and
//! pass the same admission, replay and relay-membership checks as `/query`.
//! The relay then decides, before any plugin code runs, whether the caller
//! may read or write the scope — with exactly the rule ingest applies to a
//! project-scoped event, so a plugin can never let anyone see or draft more
//! than they could by publishing and querying directly. The plugin reads only
//! through a [`RecordSource`] bound to that caller and scope, which applies
//! the reader visibility `/query` applies (personal to-do ops stay their
//! author's). Writes come back as unsigned drafts for the caller to sign;
//! this relay signs nothing on a plugin's behalf.
//!
//! Why HTTP and not an event kind: an invocation is a request with a reply
//! and no lasting fact — nothing to fan out, store or replay. The facts a
//! plugin produces are the member-signed events the caller publishes through
//! ordinary ingest afterwards.

use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use beekeeper_core::tenant::CommunityId;
use beekeeper_plugin_host::{
    Access, DraftValidators, Invocation, InvokeError, LoadError, OpEffect, OpError, PluginHost,
    RecordSource, Registry, Scope, SourceError, StoredRecord,
};
use serde::Deserialize;
use serde_json::{json, Value};

use super::bridge::{
    check_nip98_replay, enforce_http_admission, nip98_expected_url, verify_bridge_auth,
    VerifiedBridgeAuth,
};
use crate::state::AppState;
use beekeeper_core::tenant::TenantContext;

/// The environment variable naming the plugin registry.
pub const PLUGINS_FILE_ENV: &str = "BEEKEEPER_PLUGINS_FILE";

/// The kinds any plugin on this relay may be granted to read: the kinds that
/// are scoped to a project by their `a` tag, so the relay can bind a read to
/// the invocation's project. Today that is NIP-TD only.
const READABLE_KINDS: &[u32] = &[beekeeper_core::kind::KIND_PROJECT_TODO_OP];

/// The drafts this relay will hand a caller to sign, each with the same
/// validator ingest runs on it.
pub fn draft_validators() -> DraftValidators {
    let kind = beekeeper_core::kind::KIND_PROJECT_TODO_OP;
    DraftValidators::new().with(
        kind,
        Arc::new(move |tags, content| {
            beekeeper_core::project_todo::validate_project_todo_parts(kind, tags, content)
                .map(|_| ())
        }),
    )
}

/// Load the registry `BEEKEEPER_PLUGINS_FILE` names. Unset: no plugins, and
/// the relay says so at startup. Set but unloadable: an error the caller
/// treats as fatal — an operator who registered a plugin gets it or is told
/// why not.
pub fn load_plugins_from_env() -> Result<PluginHost, LoadError> {
    let Some(path) = std::env::var_os(PLUGINS_FILE_ENV).map(PathBuf::from) else {
        tracing::warn!(
            "{PLUGINS_FILE_ENV} is not set: this relay runs no plugins, so Planning is unavailable to every client"
        );
        return PluginHost::empty();
    };
    let registry = Registry::from_file(&path)?;
    for entry in &registry.entries {
        if let Some(kind) = entry
            .grants
            .read_kinds
            .iter()
            .find(|k| !READABLE_KINDS.contains(k))
        {
            return Err(LoadError::Plugin {
                plugin: entry.name.clone(),
                message: format!(
                    "is granted read of kind {kind}, which this relay cannot scope to a project"
                ),
            });
        }
    }
    let host = PluginHost::load(&registry, draft_validators())?;
    for plugin in host.plugins() {
        tracing::info!(
            plugin = %plugin.entry.name,
            version = %plugin.manifest.version,
            sha256 = %plugin.sha256,
            api = ?plugin.manifest.api,
            bytes = plugin.component_bytes,
            "plugin loaded"
        );
    }
    Ok(host)
}

type ApiError = (StatusCode, Json<Value>);

fn error(status: StatusCode, code: &str, message: impl Into<String>) -> ApiError {
    (
        status,
        Json(json!({ "error": message.into(), "code": code })),
    )
}

/// Bind the tenant and authenticate exactly as `/query` does.
async fn authenticate(
    state: &Arc<AppState>,
    headers: &HeaderMap,
    method: &str,
    path: &str,
    body: Option<&[u8]>,
) -> Result<(TenantContext, nostr::PublicKey), ApiError> {
    let raw_host = headers
        .get(axum::http::header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let tenant = crate::tenant::bind_community(&state.db, raw_host)
        .await
        .map_err(|_| {
            error(
                StatusCode::NOT_FOUND,
                "unknown-host",
                "relay: no community is configured for this host",
            )
        })?;
    let url = nip98_expected_url(&state.config.relay_url, &tenant, path);
    let VerifiedBridgeAuth {
        pubkey,
        event_id_bytes,
        signed_created_at,
    } = verify_bridge_auth(headers, method, &url, body, state.config.require_auth_token)?;
    enforce_http_admission(state, &tenant, &pubkey).await?;
    check_nip98_replay(state, &tenant, event_id_bytes).await?;
    let auth_tag = super::relay_members::extract_auth_tag_header(headers);
    super::relay_members::enforce_relay_membership(
        state,
        tenant.community(),
        &pubkey.to_bytes(),
        auth_tag,
        signed_created_at,
    )
    .await?;
    Ok((tenant, pubkey))
}

/// `GET /plugins`.
pub async fn list_plugins(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    authenticate(&state, &headers, "GET", "/plugins", None).await?;
    let plugins: Vec<Value> = state
        .plugins
        .plugins()
        .map(|p| {
            json!({
                "name": p.entry.name,
                "version": p.manifest.version,
                "sha256": p.sha256,
                "api": p.manifest.api,
                "operations": p.manifest.operations,
                // What it reads and drafts, so a client can follow the
                // records live without knowing the plugin's internals.
                "readKinds": p.manifest.read_kinds,
                "draftKinds": p.manifest.draft_kinds,
            })
        })
        .collect();
    Ok(Json(json!({ "plugins": plugins })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScopeBody {
    project: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InvokeBody {
    operation: String,
    scope: ScopeBody,
    #[serde(default)]
    input: Value,
}

/// What the caller may do in a project, by the rule ingest applies to a
/// project-scoped write (`handlers::ingest::project_scoped_write_admitted`):
/// a private project admits its roster, a public one any relay member (whom
/// [`authenticate`] already required), an unknown coordinate nobody.
async fn project_access(
    state: &AppState,
    community: CommunityId,
    coordinate: &str,
    caller: &[u8],
) -> Result<Option<Access>, ApiError> {
    let gate = state
        .db
        .get_project_gate_by_coordinate(community, coordinate)
        .await
        .map_err(|e| {
            tracing::error!("plugin scope: project gate lookup failed: {e}");
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "host-failure",
                "project lookup failed",
            )
        })?;
    match gate {
        Some(gate) if gate.admits_write(caller) => Ok(Some(Access::Write)),
        Some(gate) if gate.admits_read(caller) => Ok(Some(Access::Read)),
        Some(_) => Ok(None),
        None => {
            let exists = state
                .db
                .project_exists_by_coordinate(community, coordinate)
                .await
                .map_err(|e| {
                    tracing::error!("plugin scope: project lookup failed: {e}");
                    error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "host-failure",
                        "project lookup failed",
                    )
                })?;
            if !exists {
                return Err(error(
                    StatusCode::NOT_FOUND,
                    "unknown-project",
                    "no project with that coordinate exists in this community",
                ));
            }
            Ok(Some(Access::Write))
        }
    }
}

/// Reads for one invocation: the caller's view of one project's records.
struct ProjectRecords {
    state: Arc<AppState>,
    runtime: tokio::runtime::Handle,
    community: CommunityId,
    coordinate: String,
    reader: Vec<u8>,
    hidden: Arc<beekeeper_db::git_repo::HiddenRepos>,
    /// The call's wall-clock deadline. Guest code is interrupted by the
    /// plugin host's epoch; a database read is bounded here, because no
    /// epoch reaches a thread blocked on it.
    deadline: std::time::Instant,
}

/// Database pages one plugin page may scan past rows the reader cannot see
/// before handing back a short page with a cursor.
const MAX_SCAN_PAGES: usize = 5;

/// `<created_at>:<id hex>` of the last row scanned — visible or not — so the
/// next page starts exactly after it, with the same composite order the
/// database uses (`created_at DESC, id ASC`).
fn encode_cursor(event: &nostr::Event) -> String {
    format!("{}:{}", event.created_at.as_secs(), event.id.to_hex())
}

fn decode_cursor(cursor: &str) -> Result<(chrono::DateTime<chrono::Utc>, Vec<u8>), SourceError> {
    let invalid = || SourceError::Invalid("not a cursor this relay issued".to_owned());
    let (secs, id) = cursor.split_once(':').ok_or_else(invalid)?;
    let secs: i64 = secs.parse().map_err(|_| invalid())?;
    let id = hex::decode(id).map_err(|_| invalid())?;
    if id.len() != 32 {
        return Err(invalid());
    }
    let at = chrono::DateTime::from_timestamp(secs, 0).ok_or_else(invalid)?;
    Ok((at, id))
}

impl RecordSource for ProjectRecords {
    fn query(
        &self,
        kinds: &[u32],
        until: Option<u64>,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<(Vec<StoredRecord>, Option<String>), SourceError> {
        let limit = limit as usize;
        let mut out = Vec::with_capacity(limit);
        let mut position = cursor.map(decode_cursor).transpose()?;
        let until = match until {
            Some(secs) => Some(
                chrono::DateTime::from_timestamp(i64::try_from(secs).unwrap_or(i64::MAX), 0)
                    .ok_or_else(|| SourceError::Invalid("until is out of range".to_owned()))?,
            ),
            None => None,
        };
        // One extra row says whether older rows exist. `max_limit` must allow
        // it: the database clamps every page to 1000 otherwise, and a
        // 1001-row probe would never see its 1001st row.
        let page_limit = limit as i64 + 1;
        for _ in 0..MAX_SCAN_PAGES {
            let query = beekeeper_db::EventQuery {
                kinds: Some(kinds.iter().map(|k| *k as i32).collect()),
                a_tags: Some(vec![self.coordinate.clone()]),
                global_only: true,
                until: position.as_ref().map(|(at, _)| *at).or(until),
                before_id: position.as_ref().map(|(_, id)| id.clone()),
                limit: Some(page_limit),
                max_limit: Some(page_limit),
                ..beekeeper_db::EventQuery::for_community(self.community)
            };
            let remaining = self
                .deadline
                .saturating_duration_since(std::time::Instant::now());
            let rows = self
                .runtime
                .block_on(tokio::time::timeout(
                    remaining,
                    self.state.db.query_events(&query),
                ))
                .map_err(|_| {
                    SourceError::Unavailable("record read ran past the call's deadline".to_owned())
                })?
                .map_err(|e| SourceError::Unavailable(format!("record read failed: {e}")))?;
            let db_more = rows.len() > limit;
            let scanned = &rows[..rows.len().min(limit)];
            for (index, row) in scanned.iter().enumerate() {
                let event = &row.event;
                if !crate::handlers::req::event_visible_to_reader(event, &self.reader, &self.hidden)
                {
                    continue;
                }
                out.push(StoredRecord {
                    id: event.id.to_hex(),
                    pubkey: event.pubkey.to_hex(),
                    created_at: event.created_at.as_secs(),
                    kind: beekeeper_core::kind::event_kind_u32(event),
                    tags: event.tags.iter().map(|t| t.as_slice().to_vec()).collect(),
                    content: event.content.clone(),
                });
                if out.len() == limit {
                    let at_end = index + 1 == scanned.len() && !db_more;
                    return Ok((out, (!at_end).then(|| encode_cursor(event))));
                }
            }
            let Some(last) = scanned.last() else {
                return Ok((out, None));
            };
            if !db_more {
                return Ok((out, None));
            }
            position = Some(decode_cursor(&encode_cursor(&last.event))?);
        }
        // Scanned its bound of rows the reader cannot see without filling the
        // page: hand back what it has and where to continue, never an end.
        let position = position.ok_or_else(|| {
            SourceError::Unavailable("record scan ended without a position".to_owned())
        })?;
        Ok((
            out,
            Some(format!(
                "{}:{}",
                position.0.timestamp(),
                hex::encode(position.1)
            )),
        ))
    }
}

fn invoke_error(error: &InvokeError) -> ApiError {
    let (status, code) = match error {
        InvokeError::NotRegistered(_) => (StatusCode::NOT_FOUND, "plugin-not-registered"),
        InvokeError::UnknownOperation { .. } => (StatusCode::NOT_FOUND, "unknown-operation"),
        InvokeError::WriteNotAllowed(_) => (StatusCode::FORBIDDEN, "write-not-allowed"),
        InvokeError::InputTooLarge { .. } => (StatusCode::PAYLOAD_TOO_LARGE, "input-too-large"),
        InvokeError::Busy(_) => (StatusCode::SERVICE_UNAVAILABLE, "busy"),
        InvokeError::Trapped { .. } => (StatusCode::INTERNAL_SERVER_ERROR, "plugin-trapped"),
        InvokeError::Operation(op) => match op {
            OpError::InvalidInput(_) => (StatusCode::UNPROCESSABLE_ENTITY, "invalid-input"),
            OpError::NotFound(_) => (StatusCode::NOT_FOUND, "not-found"),
            OpError::Conflict(_) => (StatusCode::CONFLICT, "conflict"),
            OpError::Unsupported(_) => (StatusCode::NOT_IMPLEMENTED, "unsupported"),
            OpError::Host(_) => (StatusCode::SERVICE_UNAVAILABLE, "host-refused"),
            OpError::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "plugin-internal"),
        },
        InvokeError::ContractViolation(_) => (StatusCode::BAD_GATEWAY, "contract-violation"),
        InvokeError::Host(_) => (StatusCode::INTERNAL_SERVER_ERROR, "host-failure"),
    };
    // A plugin's own message is the useful part; say it plainly.
    let message = match error {
        InvokeError::Operation(
            OpError::InvalidInput(m)
            | OpError::NotFound(m)
            | OpError::Conflict(m)
            | OpError::Unsupported(m)
            | OpError::Host(m)
            | OpError::Internal(m),
        ) => m.clone(),
        other => other.to_string(),
    };
    let mut body = json!({ "error": message, "code": code });
    if let InvokeError::Trapped { kind, .. } = error {
        body["trap"] = json!(kind);
    }
    (status, Json(body))
}

/// `POST /plugins/{name}/invoke`.
pub async fn invoke_plugin(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<Json<Value>, ApiError> {
    if !beekeeper_plugin_host::valid_plugin_name(&name) {
        return Err(error(
            StatusCode::NOT_FOUND,
            "plugin-not-registered",
            "no such plugin",
        ));
    }
    let path = format!("/plugins/{name}/invoke");
    let (tenant, caller) = authenticate(&state, &headers, "POST", &path, Some(&body)).await?;
    let request: InvokeBody = serde_json::from_slice(&body).map_err(|e| {
        error(
            StatusCode::BAD_REQUEST,
            "bad-request",
            format!("invalid invocation: {e}"),
        )
    })?;
    let coordinate = request.scope.project;
    if beekeeper_core::kind::normalize_project_coordinate(&coordinate).as_deref()
        != Some(coordinate.as_str())
    {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "bad-request",
            "scope.project must be a canonical 30621:<lowercase-hex>:<d> coordinate",
        ));
    }
    let effect = state
        .plugins
        .operation(&name, &request.operation)
        .map_err(|e| invoke_error(&e))?
        .effect;
    let caller_bytes = caller.to_bytes().to_vec();
    let Some(access) =
        project_access(&state, tenant.community(), &coordinate, &caller_bytes).await?
    else {
        return Err(error(
            StatusCode::FORBIDDEN,
            "scope-forbidden",
            "you cannot read this project",
        ));
    };
    if effect == OpEffect::Draft && access != Access::Write {
        return Err(error(
            StatusCode::FORBIDDEN,
            "write-not-allowed",
            "you can read this project but not change it",
        ));
    }
    let hidden = state
        .hidden_repos_cached(tenant.community(), &caller_bytes)
        .await
        .map_err(|e| {
            tracing::error!("plugin scope: visibility lookup failed: {e}");
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "host-failure",
                "visibility lookup failed",
            )
        })?;
    let mut entropy = [0u8; 32];
    rand::fill(&mut entropy);
    let invocation = Invocation {
        operation: request.operation.clone(),
        input: request.input,
        caller: caller.to_hex(),
        scope: Scope::Project(coordinate.clone()),
        access,
        now: nostr::Timestamp::now().as_secs(),
        entropy,
    };
    let deadline_ms = state
        .plugins
        .get(&name)
        .map_or(0, |p| p.entry.limits.deadline_ms);
    let source = Arc::new(ProjectRecords {
        state: state.clone(),
        runtime: tokio::runtime::Handle::current(),
        community: tenant.community(),
        coordinate,
        reader: caller_bytes,
        hidden,
        deadline: std::time::Instant::now() + std::time::Duration::from_millis(deadline_ms),
    });
    let started = std::time::Instant::now();
    let host = state.clone();
    let plugin_name = name.clone();
    let outcome =
        tokio::task::spawn_blocking(move || host.plugins.invoke(&plugin_name, invocation, source))
            .await
            .map_err(|e| {
                tracing::error!("plugin call panicked in the host: {e}");
                error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "host-failure",
                    "plugin host failure",
                )
            })?;
    let elapsed_ms = started.elapsed().as_millis() as u64;
    let outcome = match outcome {
        Ok(outcome) => outcome,
        Err(e) => {
            tracing::warn!(
                plugin = %name,
                operation = %request.operation,
                caller = %caller.to_hex(),
                elapsed_ms,
                error = %e,
                "plugin call failed"
            );
            return Err(invoke_error(&e));
        }
    };
    for (level, message) in &outcome.logs {
        tracing::debug!(plugin = %name, ?level, "{message}");
    }
    tracing::info!(
        plugin = %name,
        operation = %request.operation,
        caller = %caller.to_hex(),
        elapsed_ms,
        fuel = outcome.fuel_used,
        drafts = outcome.drafts.len(),
        "plugin call"
    );
    let plugin = state.plugins.get(&name).map(|p| {
        json!({
            "name": p.entry.name,
            "version": p.manifest.version,
            "sha256": p.sha256,
            "api": p.manifest.api,
        })
    });
    Ok(Json(json!({
        "plugin": plugin,
        "operation": request.operation,
        "access": access,
        "result": outcome.result,
        "drafts": outcome.drafts,
    })))
}
