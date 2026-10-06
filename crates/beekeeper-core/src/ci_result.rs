//! Relay-recorded terminal CI results.
//!
//! A CI result identifies one external run and attempt exactly. The identity's
//! canonical JSON digest is the event's correlation key; the remaining fields
//! are the immutable terminal fact recorded for that key. This module validates
//! only the wire representation. Event-id/signature verification and checking
//! that the signer is the trusted relay are separate consumer responsibilities.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use url::Url;
use uuid::Uuid;

use crate::kind::{event_kind_u32, normalize_project_coordinate, KIND_CI_RESULT};
use crate::project_pack_source::normalize_repository_coordinate;

/// Exact schema named by CI-result content and its `schema` tag.
pub const CI_RESULT_SCHEMA: &str = "buzz-ci-result/v1";
/// Maximum UTF-8 byte length of a check name.
pub const MAX_CI_CHECK_BYTES: usize = 128;
/// Maximum UTF-8 byte length of an external run identifier.
pub const MAX_CI_RUN_BYTES: usize = 256;
/// Maximum UTF-8 byte length of an evidence URL.
pub const MAX_CI_EVIDENCE_URL_BYTES: usize = 2048;
/// Maximum UTF-8 byte length of a result summary.
pub const MAX_CI_SUMMARY_BYTES: usize = 4096;

/// Which product boundary the external check observed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CiPhase {
    /// Compilation, tests, and other build validation.
    Build,
    /// Deployment of the identified commit.
    Deploy,
}

/// The closed terminal outcomes a CI result can record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CiConclusion {
    /// The exact check completed successfully.
    Success,
    /// The exact check completed unsuccessfully.
    Failure,
    /// The exact check was cancelled.
    Cancelled,
}

/// Exact identity of one external CI run attempt.
///
/// Field order is part of the v1 correlation digest. Do not reorder these
/// fields without defining a new schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CiResultIdentity {
    /// Full canonical kind:30621 project coordinate.
    pub project: String,
    /// Full canonical kind:30617 repository coordinate.
    pub repository: String,
    /// Full lowercase 40-hex Git commit.
    pub commit: String,
    /// Configured check name.
    pub check: String,
    /// External provider's run identifier.
    pub run: String,
    /// External provider's attempt number, starting at one.
    pub attempt: u32,
    /// Canonical UUID of the workflow that recorded the result.
    pub workflow: String,
    /// Whether the check observed build or deployment.
    pub phase: CiPhase,
}

/// Immutable terminal result for one [`CiResultIdentity`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CiResult {
    /// Must equal [`CI_RESULT_SCHEMA`].
    pub schema: String,
    /// Exact run identity and correlation input.
    pub identity: CiResultIdentity,
    /// Terminal result.
    pub conclusion: CiConclusion,
    /// Optional HTTP(S) evidence for the external run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_url: Option<String>,
    /// Optional bounded human-readable detail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

/// Validate the exact canonical fields used to correlate a CI result.
pub fn validate_identity(identity: &CiResultIdentity) -> Result<(), String> {
    let project = normalize_project_coordinate(&identity.project).ok_or_else(|| {
        "CI result project must be a full 30621:<64-hex>:<id> coordinate".to_string()
    })?;
    if project != identity.project {
        return Err("CI result project must use a lowercase canonical owner key".into());
    }

    let repository = normalize_repository_coordinate(&identity.repository).ok_or_else(|| {
        "CI result repository must be a full 30617:<64-hex>:<id> coordinate".to_string()
    })?;
    if repository != identity.repository {
        return Err("CI result repository must use a lowercase canonical owner key".into());
    }

    if identity.commit.len() != 40
        || !identity
            .commit
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("CI result commit must be lowercase 40-hex".into());
    }
    validate_nonempty_bounded("CI result check", &identity.check, MAX_CI_CHECK_BYTES)?;
    validate_nonempty_bounded("CI result run", &identity.run, MAX_CI_RUN_BYTES)?;
    if identity.attempt == 0 {
        return Err("CI result attempt must be greater than zero".into());
    }
    let workflow = Uuid::parse_str(&identity.workflow)
        .map_err(|_| "CI result workflow must be a UUID".to_string())?;
    if workflow.to_string() != identity.workflow {
        return Err("CI result workflow must be a lowercase canonical UUID".into());
    }
    Ok(())
}

/// Return the lowercase SHA-256 correlation digest of canonical identity JSON.
pub fn correlation_id(identity: &CiResultIdentity) -> Result<String, String> {
    validate_identity(identity)?;
    let encoded = serde_json::to_vec(identity)
        .map_err(|error| format!("CI result identity could not be encoded: {error}"))?;
    Ok(hex::encode(Sha256::digest(encoded)))
}

/// Build the exact tags and canonical JSON content for a relay-signed result.
pub fn build_ci_result(result: &CiResult) -> Result<(Vec<Vec<String>>, String), String> {
    validate_result(result)?;
    let digest = correlation_id(&result.identity)?;
    let tags = vec![
        vec!["d".into(), digest],
        vec!["a".into(), result.identity.repository.clone()],
        vec!["project".into(), result.identity.project.clone()],
        vec!["workflow".into(), result.identity.workflow.clone()],
        vec!["schema".into(), CI_RESULT_SCHEMA.into()],
    ];
    let content = serde_json::to_string(result)
        .map_err(|error| format!("CI result content could not be encoded: {error}"))?;
    Ok((tags, content))
}

/// Decode and validate a kind:46008 result, including exact tag agreement.
///
/// This deliberately does not verify the event id, Schnorr signature, or
/// trusted-relay provenance. Consumers must perform those checks separately.
pub fn decode_ci_result(event: &nostr::Event) -> Result<CiResult, String> {
    if event_kind_u32(event) != KIND_CI_RESULT {
        return Err(format!("CI result must be kind {KIND_CI_RESULT}"));
    }
    let result: CiResult = serde_json::from_str(&event.content)
        .map_err(|error| format!("malformed CI result content: {error}"))?;
    validate_result(&result)?;
    let (expected_tags, _) = build_ci_result(&result)?;
    if event.tags.len() != expected_tags.len() {
        return Err("CI result requires exactly five tags".into());
    }
    for expected in expected_tags {
        let matches = event
            .tags
            .iter()
            .filter(|tag| tag.as_slice() == expected.as_slice())
            .count();
        if matches != 1 {
            return Err("CI result tags must exactly match its validated content".into());
        }
    }
    Ok(result)
}

fn validate_result(result: &CiResult) -> Result<(), String> {
    if result.schema != CI_RESULT_SCHEMA {
        return Err(format!("CI result schema must be {CI_RESULT_SCHEMA:?}"));
    }
    validate_identity(&result.identity)?;
    if let Some(evidence_url) = &result.evidence_url {
        if evidence_url.len() > MAX_CI_EVIDENCE_URL_BYTES {
            return Err(format!(
                "CI result evidence_url exceeds {MAX_CI_EVIDENCE_URL_BYTES} bytes"
            ));
        }
        let parsed = Url::parse(evidence_url)
            .map_err(|_| "CI result evidence_url must be an HTTP(S) URL".to_string())?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err("CI result evidence_url must be an HTTP(S) URL".into());
        }
    }
    if result
        .summary
        .as_ref()
        .is_some_and(|summary| summary.len() > MAX_CI_SUMMARY_BYTES)
    {
        return Err(format!(
            "CI result summary exceeds {MAX_CI_SUMMARY_BYTES} bytes"
        ));
    }
    Ok(())
}

fn validate_nonempty_bounded(field: &str, value: &str, max: usize) -> Result<(), String> {
    if value.is_empty() {
        return Err(format!("{field} must not be empty"));
    }
    if value.len() > max {
        return Err(format!("{field} exceeds {max} bytes"));
    }
    Ok(())
}

#[cfg(test)]
#[path = "ci_result_tests.rs"]
mod tests;
