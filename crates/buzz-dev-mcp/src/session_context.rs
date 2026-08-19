//! Read-only MCP projection of one launcher-selected coding-session package.
//!
//! The package path is accepted only at process startup. Tool calls cannot
//! redirect the server to another file, fetch relay data, sign events, or
//! write provider-native state.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use buzz_core::coding_session_context::{
    coding_session_first_turn_brief, CodingSessionContextHistoryItem, CodingSessionContextPackage,
    CodingSessionContextRole, MAX_CONTEXT_HISTORY_CONTENT_BYTES, MAX_CONTEXT_PACKAGE_BYTES,
};
use rmcp::ErrorData;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};

/// Launcher-only path to one private, immutable context package.
pub const SESSION_CONTEXT_PACKAGE_ENV: &str = "BUZZ_SESSION_CONTEXT_PACKAGE";

const DEFAULT_HISTORY_LIMIT: usize = 100;
const MAX_HISTORY_LIMIT: usize = 200;
const DEFAULT_SEARCH_LIMIT: usize = 20;
const MAX_SEARCH_LIMIT: usize = 50;
const MAX_SEARCH_QUERY_BYTES: usize = 256;
const MAX_INLINE_CONTENT_BYTES: usize = MAX_CONTEXT_HISTORY_CONTENT_BYTES;
const MAX_SEARCH_SNIPPET_BYTES: usize = 2 * 1024;

/// Empty arguments for the session overview tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionOverviewParams {}

/// Pagination arguments for verified session history.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionHistoryParams {
    /// Zero-based history-item offset. Defaults to zero.
    #[serde(default)]
    pub offset: Option<usize>,
    /// Number of history items to return. Defaults to 100; maximum 200.
    #[serde(default)]
    pub limit: Option<usize>,
}

/// Search arguments for verified session history.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchSessionParams {
    /// Non-empty text query, at most 256 UTF-8 bytes.
    pub query: String,
    /// Zero-based offset into matching items. Defaults to zero.
    #[serde(default)]
    pub offset: Option<usize>,
    /// Number of matching items to return. Defaults to 20; maximum 50.
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Clone)]
pub(crate) struct SessionContextState {
    package: CodingSessionContextPackage,
}

impl SessionContextState {
    pub(crate) fn load_from_env() -> io::Result<Option<Self>> {
        let Some(raw) = std::env::var_os(SESSION_CONTEXT_PACKAGE_ENV) else {
            return Ok(None);
        };
        Self::load(Path::new(&raw)).map(Some)
    }

    fn load(path: &Path) -> io::Result<Self> {
        if !path.is_absolute() {
            return Err(invalid_data(format!(
                "{SESSION_CONTEXT_PACKAGE_ENV} must name an absolute path"
            )));
        }
        let path_metadata = std::fs::symlink_metadata(path).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("cannot inspect session context package: {error}"),
            )
        })?;
        if path_metadata.file_type().is_symlink() {
            return Err(invalid_data(
                "session context package must not be a symbolic link",
            ));
        }

        let file = File::open(path).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("cannot open session context package: {error}"),
            )
        })?;
        let open_metadata = file.metadata().map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("cannot inspect open session context package: {error}"),
            )
        })?;
        if !open_metadata.is_file() {
            return Err(invalid_data(
                "session context package must be a regular file",
            ));
        }
        validate_private_permissions(&path_metadata, &open_metadata)?;
        if open_metadata.len() > MAX_CONTEXT_PACKAGE_BYTES as u64 {
            return Err(invalid_data(format!(
                "session context package exceeds {MAX_CONTEXT_PACKAGE_BYTES} bytes"
            )));
        }

        let mut bytes = Vec::with_capacity(open_metadata.len() as usize);
        file.take(MAX_CONTEXT_PACKAGE_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| {
                io::Error::new(
                    error.kind(),
                    format!("cannot read session context package: {error}"),
                )
            })?;
        if bytes.len() > MAX_CONTEXT_PACKAGE_BYTES {
            return Err(invalid_data(format!(
                "session context package grew past {MAX_CONTEXT_PACKAGE_BYTES} bytes while reading"
            )));
        }

        let raw: Value = serde_json::from_slice(&bytes).map_err(|error| {
            invalid_data(format!(
                "session context package is not valid JSON: {error}"
            ))
        })?;
        reject_secret_material(&raw)?;
        // Decode the original bytes, not the intermediate Value: a Value map
        // has already collapsed duplicate JSON keys, while the strict shared
        // struct must reject duplicate fields rather than silently taking one.
        let package: CodingSessionContextPackage =
            serde_json::from_slice(&bytes).map_err(|error| {
                invalid_data(format!(
                    "session context package has an invalid shape: {error}"
                ))
            })?;
        package.validate().map_err(|error| {
            invalid_data(format!(
                "session context package failed validation: {error}"
            ))
        })?;
        Ok(Self { package })
    }

    #[cfg(test)]
    fn from_package(package: CodingSessionContextPackage) -> Self {
        Self { package }
    }

    pub(crate) fn overview(&self, _params: SessionOverviewParams) -> Result<String, ErrorData> {
        let mut history_by_role = BTreeMap::<&'static str, usize>::new();
        for item in &self.package.history {
            *history_by_role.entry(role_label(item.role)).or_default() += 1;
        }
        render(json!({
            "packageVersion": self.package.v,
            "session": self.package.session,
            "provenance": self.package.provenance,
            "provenanceSemantics": provenance_semantics(),
            "firstTurnBrief": coding_session_first_turn_brief(&self.package),
            "availableHistoryItems": self.package.history.len(),
            "historyByRole": history_by_role,
        }))
    }

    pub(crate) fn history(&self, params: SessionHistoryParams) -> Result<String, ErrorData> {
        let offset = params.offset.unwrap_or(0);
        let limit = bounded_limit(
            params.limit,
            DEFAULT_HISTORY_LIMIT,
            MAX_HISTORY_LIMIT,
            "session_history",
        )?;
        let total = self.package.history.len();
        let start = offset.min(total);
        let end = start.saturating_add(limit).min(total);
        let items = self.package.history[start..end]
            .iter()
            .map(history_item_view)
            .collect::<Result<Vec<_>, _>>()?;
        render(json!({
            "sessionRef": self.package.session.session_ref,
            "provenance": self.package.provenance,
            "provenanceSemantics": provenance_semantics(),
            "offset": start,
            "limit": limit,
            "returned": items.len(),
            "availableHistoryItems": total,
            "nextOffset": (end < total).then_some(end),
            "items": items,
        }))
    }

    pub(crate) fn search(&self, params: SearchSessionParams) -> Result<String, ErrorData> {
        let query = params.query.trim();
        if query.is_empty() || query.len() > MAX_SEARCH_QUERY_BYTES {
            return Err(ErrorData::invalid_params(
                format!(
                    "search_session query must contain text and be at most {MAX_SEARCH_QUERY_BYTES} UTF-8 bytes"
                ),
                None,
            ));
        }
        let limit = bounded_limit(
            params.limit,
            DEFAULT_SEARCH_LIMIT,
            MAX_SEARCH_LIMIT,
            "search_session",
        )?;
        let offset = params.offset.unwrap_or(0);
        let folded_query = query.to_ascii_lowercase();
        let mut matches = Vec::new();
        for (history_offset, item) in self.package.history.iter().enumerate() {
            let searchable = serde_json::to_string(item).map_err(internal_serialization)?;
            let match_offset = if query.is_ascii() {
                searchable.to_ascii_lowercase().find(&folded_query)
            } else {
                searchable.find(query)
            };
            if let Some(match_offset) = match_offset {
                matches.push((history_offset, item, searchable, match_offset));
            }
        }
        let total_matches = matches.len();
        let start = offset.min(total_matches);
        let end = start.saturating_add(limit).min(total_matches);
        let results = matches[start..end]
            .iter()
            .map(|(history_offset, item, searchable, match_offset)| {
                json!({
                    "historyOffset": history_offset,
                    "eventId": item.event_id,
                    "createdAt": item.created_at,
                    "author": item.author,
                    "target": item.target,
                    "eventSeq": item.event_seq,
                    "turnId": item.turn_id,
                    "role": item.role,
                    "itemKind": item.item_kind,
                    "snippet": excerpt_around(searchable, *match_offset, MAX_SEARCH_SNIPPET_BYTES),
                    "snippetTruncatedByTool": searchable.len() > MAX_SEARCH_SNIPPET_BYTES,
                })
            })
            .collect::<Vec<_>>();
        render(json!({
            "sessionRef": self.package.session.session_ref,
            "provenance": self.package.provenance,
            "provenanceSemantics": provenance_semantics(),
            "query": query,
            "offset": start,
            "limit": limit,
            "returned": results.len(),
            "totalMatches": total_matches,
            "nextOffset": (end < total_matches).then_some(end),
            "results": results,
        }))
    }
}

fn provenance_semantics() -> Value {
    json!({
        "complete": "Complete only for the source snapshot begun at provenance.completeAsOf; later concurrent activity may exist. A null completeAsOf is a legacy package with an unknown watermark",
        "sourceEventCount": "All signed facts retained in the verified proof graph, including identity, authority, lifecycle, metadata, and transcript events",
        "totalHistoryItems": "Verified transcript items only"
    })
}

fn history_item_view(item: &CodingSessionContextHistoryItem) -> Result<Value, ErrorData> {
    let content = serde_json::to_string(&item.content).map_err(internal_serialization)?;
    let (content_value, preview, content_truncated) = if content.len() <= MAX_INLINE_CONTENT_BYTES {
        (Some(item.content.clone()), None, false)
    } else {
        (
            None,
            Some(truncate_utf8(&content, MAX_INLINE_CONTENT_BYTES)),
            true,
        )
    };
    Ok(json!({
        "eventId": item.event_id,
        "createdAt": item.created_at,
        "author": item.author,
        "sourceKind": item.source_kind,
        "target": item.target,
        "eventSeq": item.event_seq,
        "turnId": item.turn_id,
        "role": item.role,
        "itemKind": item.item_kind,
        "content": content_value,
        "contentPreviewJson": preview,
        "contentTruncatedByTool": content_truncated,
    }))
}

fn bounded_limit(
    requested: Option<usize>,
    default: usize,
    maximum: usize,
    tool: &str,
) -> Result<usize, ErrorData> {
    let limit = requested.unwrap_or(default);
    if limit == 0 || limit > maximum {
        return Err(ErrorData::invalid_params(
            format!("{tool} limit must be between 1 and {maximum}"),
            None,
        ));
    }
    Ok(limit)
}

fn role_label(role: CodingSessionContextRole) -> &'static str {
    match role {
        CodingSessionContextRole::User => "user",
        CodingSessionContextRole::Assistant => "assistant",
        CodingSessionContextRole::Tool => "tool",
        CodingSessionContextRole::Reasoning => "reasoning",
        CodingSessionContextRole::Lifecycle => "lifecycle",
        CodingSessionContextRole::System => "system",
    }
}

fn excerpt_around(text: &str, match_offset: usize, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_owned();
    }
    let mut start = match_offset.saturating_sub(max_bytes / 3);
    while start < text.len() && !text.is_char_boundary(start) {
        start += 1;
    }
    let mut end = start.saturating_add(max_bytes).min(text.len());
    while end > start && !text.is_char_boundary(end) {
        end -= 1;
    }
    let prefix = if start > 0 { "…" } else { "" };
    let suffix = if end < text.len() { "…" } else { "" };
    format!("{prefix}{}{suffix}", &text[start..end])
}

fn truncate_utf8(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_owned();
    }
    let mut end = max_bytes.saturating_sub('…'.len_utf8()).min(text.len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

fn render(value: Value) -> Result<String, ErrorData> {
    serde_json::to_string_pretty(&value).map_err(internal_serialization)
}

fn internal_serialization(error: serde_json::Error) -> ErrorData {
    ErrorData::internal_error(
        format!("failed to serialize session context: {error}"),
        None,
    )
}

fn reject_secret_material(value: &Value) -> io::Result<()> {
    match value {
        Value::Object(object) => {
            for (key, nested) in object {
                let normalized = key
                    .chars()
                    .filter(|character| character.is_ascii_alphanumeric())
                    .flat_map(char::to_lowercase)
                    .collect::<String>();
                if matches!(
                    normalized.as_str(),
                    "privatekey"
                        | "secretkey"
                        | "signingkey"
                        | "nostrprivatekey"
                        | "buzzprivatekey"
                        | "buzzauthtag"
                        | "relayauthtoken"
                ) {
                    return Err(invalid_data(format!(
                        "session context package contains forbidden secret field {key:?}"
                    )));
                }
                reject_secret_material(nested)?;
            }
        }
        Value::Array(values) => {
            for nested in values {
                reject_secret_material(nested)?;
            }
        }
        Value::String(text) if text.trim_start().starts_with("nsec1") => {
            return Err(invalid_data(
                "session context package contains forbidden Nostr secret material",
            ));
        }
        _ => {}
    }
    Ok(())
}

#[cfg(unix)]
fn validate_private_permissions(
    path_metadata: &std::fs::Metadata,
    open_metadata: &std::fs::Metadata,
) -> io::Result<()> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    if path_metadata.dev() != open_metadata.dev() || path_metadata.ino() != open_metadata.ino() {
        return Err(invalid_data(
            "session context package changed while it was being opened",
        ));
    }
    if open_metadata.permissions().mode() & 0o777 != 0o600 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "session context package must have exact Unix permissions 0600",
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn validate_private_permissions(
    _path_metadata: &std::fs::Metadata,
    _open_metadata: &std::fs::Metadata,
) -> io::Result<()> {
    Ok(())
}

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    fn package_json() -> Value {
        json!({
            "v": 1,
            "session": {
                "sessionRef": "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10",
                "genesisRef": "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd",
                "channelId": "00000000-0000-0000-0000-000000000000",
                "name": "Rehydrated context",
                "goal": "Continue from durable facts",
                "projectRef": null
            },
            "provenance": {
                "generatedAt": 1,
                "completeAsOf": null,
                "complete": false,
                "truncated": true,
                "sourceEventCount": 4,
                "includedHistoryItems": 2,
                "omittedHistoryItems": 3,
                "totalHistoryItems": null,
                "notes": ["Source query did not prove a complete history window"]
            },
            "history": [
                {
                    "eventId": format!("{:064x}", 1),
                    "createdAt": 10,
                    "author": "ab".repeat(32),
                    "sourceKind": 44225,
                    "target": {
                        "driver": "codex-acp",
                        "instanceId": "primary",
                        "sessionId": "provider-session",
                        "generation": 1
                    },
                    "eventSeq": 1,
                    "turnId": "turn-1",
                    "role": "user",
                    "itemKind": "user_prompt",
                    "content": {"kind": "user_prompt", "text": "Need the durable close/reopen split"}
                },
                {
                    "eventId": format!("{:064x}", 2),
                    "createdAt": 11,
                    "author": "ab".repeat(32),
                    "sourceKind": 44225,
                    "target": {
                        "driver": "codex-acp",
                        "instanceId": "primary",
                        "sessionId": "provider-session",
                        "generation": 1
                    },
                    "eventSeq": 2,
                    "turnId": "turn-1",
                    "role": "assistant",
                    "itemKind": "assistant_text",
                    "content": {"kind": "assistant_text", "text": "Closure is separate from stopping execution"}
                }
            ]
        })
    }

    fn package() -> CodingSessionContextPackage {
        serde_json::from_value(package_json()).expect("valid package fixture")
    }

    fn write_package(path: &Path, value: &Value) {
        std::fs::write(path, serde_json::to_vec(value).expect("encode package"))
            .expect("write package");
        make_private(path);
    }

    fn make_private(path: &Path) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
                .expect("chmod package");
        }
    }

    #[test]
    fn loads_one_private_absolute_package_and_keeps_provenance_honest() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("context.json");
        write_package(&path, &package_json());

        let state = SessionContextState::load(&path).expect("load package");
        let overview: Value = serde_json::from_str(
            &state
                .overview(SessionOverviewParams::default())
                .expect("overview"),
        )
        .expect("overview JSON");
        assert_eq!(overview["provenance"]["complete"], false);
        assert_eq!(overview["provenance"]["truncated"], true);
        assert_eq!(overview["provenance"]["omittedHistoryItems"], 3);
        assert_eq!(overview["availableHistoryItems"], 2);
    }

    #[test]
    fn history_is_bounded_paginated_and_repeats_provenance() {
        let state = SessionContextState::from_package(package());
        let first: Value = serde_json::from_str(
            &state
                .history(SessionHistoryParams {
                    offset: Some(0),
                    limit: Some(1),
                })
                .expect("history"),
        )
        .expect("history JSON");
        assert_eq!(first["returned"], 1);
        assert_eq!(first["nextOffset"], 1);
        assert_eq!(first["items"][0]["itemKind"], "user_prompt");
        assert_eq!(first["provenance"]["complete"], false);
        assert_eq!(first["provenance"]["truncated"], true);
        assert!(first["provenanceSemantics"]["sourceEventCount"]
            .as_str()
            .is_some_and(|text| text.contains("proof graph")));
        assert!(state
            .history(SessionHistoryParams {
                offset: None,
                limit: Some(MAX_HISTORY_LIMIT + 1),
            })
            .is_err());
    }

    #[test]
    fn one_history_call_can_retrieve_the_observed_101_item_session() {
        let mut package = package();
        let template = package.history[0].clone();
        package.history = (1..=101)
            .map(|seq| CodingSessionContextHistoryItem {
                event_id: format!("{seq:064x}"),
                created_at: seq,
                event_seq: seq,
                content: json!({
                    "kind": "user_prompt",
                    "content": format!("safe prompt {seq}"),
                    "steered": false
                }),
                ..template.clone()
            })
            .collect();
        package.provenance.included_history_items = 101;
        package.provenance.truncated = false;
        package.provenance.omitted_history_items = 0;
        package.provenance.total_history_items = None;
        package.validate().expect("101-item package");
        let state = SessionContextState::from_package(package);

        let page: Value = serde_json::from_str(
            &state
                .history(SessionHistoryParams {
                    offset: Some(0),
                    limit: Some(101),
                })
                .expect("101-item page"),
        )
        .expect("history JSON");

        assert_eq!(page["returned"], 101);
        assert!(page["nextOffset"].is_null());
    }

    #[test]
    fn search_is_bounded_paginated_and_repeats_provenance() {
        let state = SessionContextState::from_package(package());
        let result: Value = serde_json::from_str(
            &state
                .search(SearchSessionParams {
                    query: "CLOSURE".into(),
                    offset: None,
                    limit: Some(1),
                })
                .expect("search"),
        )
        .expect("search JSON");
        assert_eq!(result["totalMatches"], 1);
        assert_eq!(result["results"][0]["historyOffset"], 1);
        assert_eq!(result["provenance"]["complete"], false);
        assert_eq!(result["provenance"]["truncated"], true);
        assert!(state
            .search(SearchSessionParams {
                query: "x".repeat(MAX_SEARCH_QUERY_BYTES + 1),
                offset: None,
                limit: None,
            })
            .is_err());
    }

    #[test]
    fn rejects_relative_non_private_symlink_and_malformed_packages() {
        assert!(SessionContextState::load(Path::new("relative.json")).is_err());
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("context.json");
        write_package(&path, &package_json());

        #[cfg(unix)]
        {
            use std::os::unix::fs::{symlink, PermissionsExt};
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
                .expect("chmod public");
            assert!(SessionContextState::load(&path).is_err());
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
                .expect("chmod private");
            let link = dir.path().join("context-link.json");
            symlink(&path, &link).expect("create symlink");
            assert!(SessionContextState::load(&link).is_err());
        }

        let malformed = dir.path().join("malformed.json");
        write_package(&malformed, &json!({"v": 1, "privateKey": "secret"}));
        assert!(SessionContextState::load(&malformed).is_err());

        let duplicate = dir.path().join("duplicate.json");
        let encoded = serde_json::to_string(&package_json()).expect("encode duplicate fixture");
        std::fs::write(
            &duplicate,
            encoded.replacen("\"v\":1", "\"v\":1,\"v\":1", 1),
        )
        .expect("write duplicate fixture");
        make_private(&duplicate);
        assert!(SessionContextState::load(&duplicate).is_err());

        let oversized = dir.path().join("oversized.json");
        let file = File::create(&oversized).expect("create oversized fixture");
        file.set_len(MAX_CONTEXT_PACKAGE_BYTES as u64 + 1)
            .expect("size oversized fixture");
        make_private(&oversized);
        assert!(SessionContextState::load(&oversized).is_err());
    }

    #[test]
    fn rejects_secret_fields_and_nsec_values_inside_structured_history() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("context.json");
        let mut secret_field = package_json();
        secret_field["history"][0]["content"]["private_key"] = json!("aa");
        write_package(&path, &secret_field);
        assert!(SessionContextState::load(&path).is_err());

        let mut nsec = package_json();
        nsec["history"][0]["content"]["text"] = json!("nsec1forbidden");
        write_package(&path, &nsec);
        assert!(SessionContextState::load(&path).is_err());

        let mut host_path = package_json();
        host_path["history"][0]["content"]["content"] = json!("read /Users/alice/private/repo");
        write_package(&path, &host_path);
        assert!(SessionContextState::load(&path).is_err());
    }

    #[test]
    fn a_large_valid_item_is_returned_in_full() {
        let mut raw = package_json();
        raw["provenance"]["complete"] = json!(true);
        raw["provenance"]["completeAsOf"] = json!(1);
        raw["provenance"]["truncated"] = json!(false);
        raw["provenance"]["omittedHistoryItems"] = json!(0);
        raw["provenance"]["totalHistoryItems"] = json!(2);
        raw["history"][0]["content"]["text"] = json!("x".repeat(30 * 1024));
        let package: CodingSessionContextPackage =
            serde_json::from_value(raw).expect("large package fixture");
        package.validate().expect("large package valid");
        let state = SessionContextState::from_package(package);
        let history: Value = serde_json::from_str(
            &state
                .history(SessionHistoryParams {
                    offset: None,
                    limit: Some(1),
                })
                .expect("history"),
        )
        .expect("history JSON");
        assert_eq!(history["items"][0]["contentTruncatedByTool"], false);
        assert_eq!(
            history["items"][0]["content"]["text"]
                .as_str()
                .map(str::len),
            Some(30 * 1024)
        );
        assert!(history["items"][0]["contentPreviewJson"].is_null());
        assert_eq!(history["provenance"]["complete"], true);
        assert_eq!(history["provenance"]["truncated"], false);
    }
}
