//! Field validators shared by every NIP-CSTX body.
//!
//! A child of `coding_session_team_transaction`, split out only to keep every
//! file under 1,000 lines (FINAL-B §7). No behaviour change: these are the same
//! functions, and `use super::*` gives them the parent's constants exactly as
//! before.

use serde_json::Value;

use super::*;

pub(super) fn validate_exact_keys(
    object: &serde_json::Map<String, Value>,
    expected: &[&str],
    label: &str,
) -> Result<(), String> {
    if object.len() != expected.len()
        || !expected.iter().all(|key| object.contains_key(*key))
        || !object.keys().all(|key| expected.contains(&key.as_str()))
    {
        return Err(format!("{label} has missing or unsupported fields"));
    }
    Ok(())
}

pub(super) fn validate_canonical_uuid(field: &str, value: &str) -> Result<(), String> {
    let parsed = Uuid::parse_str(value).map_err(|_| format!("{field} must be a UUID"))?;
    if parsed.to_string() != value {
        return Err(format!("{field} must be a lowercase canonical UUID"));
    }
    Ok(())
}

pub(super) fn validate_event_id(field: &str, value: &str) -> Result<(), String> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(format!("{field} must be a lowercase 64-hex event id"));
    }
    Ok(())
}

pub(super) fn validate_event_ids(
    field: &str,
    values: &[String],
    require_nonempty: bool,
) -> Result<(), String> {
    validate_collection_size(field, values.len(), require_nonempty)?;
    for value in values {
        validate_event_id(field, value)?;
    }
    validate_unique(field, values)
}

/// Validate a bounded, unique list of event ids with a per-field cap tighter
/// than [`MAX_TEAM_TRANSACTION_ITEMS`].
pub(super) fn validate_bounded_event_ids(
    field: &str,
    values: &[String],
    max: usize,
) -> Result<(), String> {
    if values.len() > max {
        return Err(format!("{field} exceeds {max} entries"));
    }
    for value in values {
        validate_event_id(field, value)?;
    }
    validate_unique(field, values)
}

pub(super) fn validate_git_sha(field: &str, value: &str) -> Result<(), String> {
    if !matches!(value.len(), 40 | 64)
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(format!(
            "{field} must be a lowercase 40- or 64-hex git object id"
        ));
    }
    Ok(())
}

pub(super) fn validate_optional_git_sha(field: &str, value: Option<&str>) -> Result<(), String> {
    if let Some(value) = value {
        validate_git_sha(field, value)?;
    }
    Ok(())
}

pub(super) fn validate_git_shas(field: &str, values: &[String]) -> Result<(), String> {
    validate_collection_size(field, values.len(), false)?;
    for value in values {
        validate_git_sha(field, value)?;
    }
    validate_unique(field, values)
}

pub(super) fn validate_role(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err("assigneeRole must be a lowercase [a-z0-9-] slug of at most 64 bytes".into());
    }
    Ok(())
}

pub(super) fn validate_delivery_command_id(value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err("deliveryCommandId must not be blank".into());
    }
    if value.len() > MAX_TEAM_TRANSACTION_DELIVERY_COMMAND_ID_BYTES {
        return Err(format!(
            "deliveryCommandId exceeds {MAX_TEAM_TRANSACTION_DELIVERY_COMMAND_ID_BYTES} bytes"
        ));
    }
    if value.chars().any(char::is_control) {
        return Err("deliveryCommandId must not contain control characters".into());
    }
    Ok(())
}

pub(super) fn validate_text(field: &str, value: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{field} must not be blank"));
    }
    if value.len() > max {
        return Err(format!("{field} exceeds {max} bytes"));
    }
    if value.contains('\0') {
        return Err(format!("{field} must not contain NUL"));
    }
    Ok(())
}

pub(super) fn validate_optional_text(
    field: &str,
    value: Option<&str>,
    max: usize,
) -> Result<(), String> {
    if let Some(value) = value {
        validate_text(field, value, max)?;
    }
    Ok(())
}

pub(super) fn validate_texts(
    field: &str,
    values: &[String],
    require_nonempty: bool,
) -> Result<(), String> {
    validate_collection_size(field, values.len(), require_nonempty)?;
    for value in values {
        validate_text(field, value, MAX_TEAM_TRANSACTION_TEXT_BYTES)?;
    }
    Ok(())
}

pub(super) fn validate_paths(field: &str, values: &[String]) -> Result<(), String> {
    validate_collection_size(field, values.len(), false)?;
    for value in values {
        validate_text(field, value, MAX_TEAM_TRANSACTION_PATH_BYTES)?;
    }
    validate_unique(field, values)
}

pub(super) fn validate_collection_size(
    field: &str,
    len: usize,
    require_nonempty: bool,
) -> Result<(), String> {
    if require_nonempty && len == 0 {
        return Err(format!("{field} must not be empty"));
    }
    if len > MAX_TEAM_TRANSACTION_ITEMS {
        return Err(format!(
            "{field} exceeds {MAX_TEAM_TRANSACTION_ITEMS} entries"
        ));
    }
    Ok(())
}

pub(super) fn validate_unique(field: &str, values: &[String]) -> Result<(), String> {
    for (index, value) in values.iter().enumerate() {
        if values[..index].contains(value) {
            return Err(format!("{field} must not contain duplicates"));
        }
    }
    Ok(())
}
