//! Bounded private continuity claims over NIP-AE. No I/O, truth inference or CAS.
//!
//! Transport callers must query only the current community, retain query failure
//! and cap information, and re-read evidence under current actor access.

use crate::engram::Body;
use serde::{Deserialize, Deserializer, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

#[path = "continuity_read.rs"]
mod read;
pub use read::*;

/// Exact companion version.
pub const SCHEMA: &str = "beekeeper-continuity/1";
/// Maximum serialized outer NIP-AE body, including nested JSON escaping.
pub const MAX_BODY_BYTES: usize = 48 * 1024;
/// Maximum claims in one scope.
pub const MAX_CLAIMS: usize = 16;
/// Maximum evidence references per claim.
pub const MAX_EVIDENCE: usize = 4;
/// Maximum UTF-8 bytes in each text, prior text, reason or excerpt.
pub const MAX_TEXT_BYTES: usize = 2048;
/// Exact head query limit; reaching this is incomplete coverage.
pub const HEAD_QUERY_LIMIT: usize = 16;

/// Bounded errors never contain private input text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// JSON shape, missing/unknown/duplicate fields, or version is invalid.
    #[error("invalid continuity schema")]
    Schema,
    /// Scope is not canonical or does not match the requested address.
    #[error("invalid continuity scope")]
    Scope,
    /// An ID, correction or claim violates the wire contract.
    #[error("invalid continuity claim")]
    Claim,
    /// A text, array or serialized body exceeds its limit.
    #[error("continuity bound exceeded")]
    TooLarge,
    /// Preflight base differs from the immediately observed head.
    #[error("continuity base mismatch")]
    BaseMismatch,
    /// A changed/omitted claim does not preserve correction provenance.
    #[error("invalid continuity transition")]
    Transition,
    /// Writing would require a timestamp beyond wall clock plus 60 seconds.
    #[error("continuity head clock exceeds write allowance")]
    Clock,
}

// Unlike ordinary Option fields these fields must be present, even when null.
fn required_nullable<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::deserialize(deserializer)
}

/// Community and responsibility scope; actor/owner are bound by NIP-AE.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    /// Canonical HTTP(S) origin, without a trailing slash.
    pub community: String,
    /// Canonical lowercase hyphenated channel UUID.
    pub channel: String,
    /// Umbrella UUID, or explicit null for a standalone conversation.
    #[serde(deserialize_with = "required_nullable")]
    pub session: Option<String>,
}

impl Scope {
    /// Normalize an endpoint and UUIDs for address construction.
    pub fn new(community: &str, channel: &str, session: Option<&str>) -> Result<Self, Error> {
        Ok(Self {
            community: canonical_origin(community)?,
            channel: canonical_uuid(channel)?,
            session: session.map(canonical_uuid).transpose()?,
        })
    }

    /// Require the wire spelling to already be canonical.
    pub fn validate(&self) -> Result<(), Error> {
        if Self::new(&self.community, &self.channel, self.session.as_deref())? != *self {
            return Err(Error::Scope);
        }
        Ok(())
    }

    /// Derive the public slug; NIP-AE subsequently derives the private d tag.
    pub fn slug(&self) -> Result<String, Error> {
        self.validate()?;
        let bytes = serde_json::to_vec(&(&self.community, &self.channel, &self.session))
            .map_err(|_| Error::Schema)?;
        Ok(format!("mem/continuity/v1/{}", value_hash(&bytes)))
    }
}

/// Normalize a root HTTP(S)/WS(S) endpoint; refuse paths and ambient URL data.
pub fn canonical_origin(endpoint: &str) -> Result<String, Error> {
    // URL parsing normalizes dot segments and backslashes. Refuse non-root
    // input before that normalization can collapse distinct endpoints.
    let (_, authority_and_path) = endpoint.split_once("://").ok_or(Error::Scope)?;
    if endpoint.chars().any(|c| c.is_whitespace() || c == '\\')
        || authority_and_path
            .find('/')
            .is_some_and(|i| &authority_and_path[i..] != "/")
    {
        return Err(Error::Scope);
    }
    let mut url = url::Url::parse(endpoint).map_err(|_| Error::Scope)?;
    let scheme = match url.scheme() {
        "http" | "ws" => "http",
        "https" | "wss" => "https",
        _ => return Err(Error::Scope),
    };
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
        || url.host_str().is_none()
    {
        return Err(Error::Scope);
    }
    url.set_scheme(scheme).map_err(|_| Error::Scope)?;
    Ok(url.origin().ascii_serialization())
}

fn canonical_uuid(value: &str) -> Result<String, Error> {
    uuid::Uuid::parse_str(value)
        .map(|id| id.hyphenated().to_string())
        .map_err(|_| Error::Scope)
}

/// An authored assertion is not established truth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimState {
    /// Eligible for current evidence checks.
    Asserted,
    /// Retained for attribution, never rendered as a current assertion.
    Withdrawn,
}

/// Exact currently-readable signed channel event reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    /// Explicit Nostr kind (the Nostr wire kind range).
    pub kind: u16,
    /// Lowercase 64-hex event ID.
    pub event_id: String,
    /// Canonical channel UUID from the source h tag.
    pub channel: String,
    /// SHA-256 of exact UTF-8 source content.
    pub sha256: String,
    /// Exact substring of source content; retained privately.
    pub excerpt: String,
}

/// Latest correction explanation, not an immutable history chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Correction {
    /// Immediately read head at the time of the change.
    pub prior_head: String,
    /// Exact previous claim text.
    pub prior_text: String,
    /// Nonempty explanation of the change.
    pub reason: String,
}

/// A bounded agent-authored claim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Claim {
    /// Unique scope-local [a-z0-9][a-z0-9_-]{0,63} identifier.
    pub id: String,
    /// Asserted or withdrawn.
    pub state: ClaimState,
    /// Authored text, not instructions or independently verified truth.
    pub text: String,
    /// Nonnegative Unix observation second, not a freshness guarantee.
    pub observed_at: u64,
    /// Zero to four current source references.
    pub evidence: Vec<Evidence>,
    /// Latest correction or explicit null for a new claim.
    #[serde(deserialize_with = "required_nullable")]
    pub correction: Option<Correction>,
}

/// Complete decoded companion value. Use `decode`/`validate` before consumption.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    /// Must equal SCHEMA.
    pub schema: String,
    /// Exact requested responsibility scope.
    pub scope: Scope,
    /// Immediately observed base, or explicit null on initial create.
    #[serde(deserialize_with = "required_nullable")]
    pub base_event_id: Option<String>,
    /// Complete claim set; structured writes cannot silently omit prior IDs.
    pub claims: Vec<Claim>,
}

/// Lowercase SHA-256 over exact bytes, including stored value whitespace.
pub fn value_hash(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn hex_id(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Validate a requested claim identifier without echoing private input.
pub fn validate_claim_id(id: &str) -> Result<(), Error> {
    let alnum = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit();
    if id.is_empty()
        || id.len() > 64
        || !alnum(id.as_bytes()[0])
        || !id.bytes().all(|b| alnum(b) || b == b'_' || b == b'-')
    {
        return Err(Error::Claim);
    }
    Ok(())
}

impl Evidence {
    /// Validate reference syntax and bounds, without asserting availability.
    pub fn validate(&self) -> Result<(), Error> {
        if !hex_id(&self.event_id)
            || !hex_id(&self.sha256)
            || canonical_uuid(&self.channel)? != self.channel
        {
            return Err(Error::Claim);
        }
        if self.excerpt.len() > MAX_TEXT_BYTES {
            return Err(Error::TooLarge);
        }
        Ok(())
    }
}

impl Record {
    /// Parse directly from raw value bytes, rejecting duplicate keys at every level.
    pub fn decode(value: &str, scope: &Scope) -> Result<Self, Error> {
        if value.len() > MAX_BODY_BYTES {
            return Err(Error::TooLarge);
        }
        let record: Self = serde_json::from_str(value).map_err(|_| Error::Schema)?;
        record.validate(scope)?;
        check_body_size(&Body::Memory {
            slug: scope.slug()?,
            value: Some(value.to_owned()),
        })?;
        Ok(record)
    }

    /// Validate wire invariants; historical heads need not remain retrievable.
    pub fn validate(&self, scope: &Scope) -> Result<(), Error> {
        scope.validate()?;
        self.scope.validate()?;
        if &self.scope != scope {
            return Err(Error::Scope);
        }
        if self.schema != SCHEMA {
            return Err(Error::Schema);
        }
        if self.base_event_id.as_deref().is_some_and(|id| !hex_id(id)) {
            return Err(Error::Claim);
        }
        if self.claims.len() > MAX_CLAIMS {
            return Err(Error::TooLarge);
        }
        let mut ids = BTreeSet::new();
        for claim in &self.claims {
            validate_claim_id(&claim.id)?;
            if !ids.insert(&claim.id) {
                return Err(Error::Claim);
            }
            if claim.text.len() > MAX_TEXT_BYTES || claim.evidence.len() > MAX_EVIDENCE {
                return Err(Error::TooLarge);
            }
            for evidence in &claim.evidence {
                evidence.validate()?;
            }
            if let Some(c) = &claim.correction {
                if !hex_id(&c.prior_head)
                    || c.reason.trim().is_empty()
                    || self.base_event_id.is_none()
                {
                    return Err(Error::Claim);
                }
                if c.prior_text.len() > MAX_TEXT_BYTES || c.reason.len() > MAX_TEXT_BYTES {
                    return Err(Error::TooLarge);
                }
            }
            if claim.state == ClaimState::Withdrawn && claim.correction.is_none() {
                return Err(Error::Claim);
            }
        }
        Ok(())
    }

    /// Encode a write, sorting claims by ID and checking the escaped outer size.
    pub fn to_body(&self) -> Result<Body, Error> {
        self.validate(&self.scope)?;
        let mut record = self.clone();
        record.claims.sort_by(|a, b| a.id.cmp(&b.id));
        let value = serde_json::to_string(&record).map_err(|_| Error::Schema)?;
        let body = Body::Memory {
            slug: self.scope.slug()?,
            value: Some(value),
        };
        check_body_size(&body)?;
        Ok(body)
    }
}

fn check_body_size(body: &Body) -> Result<(), Error> {
    if body.to_json_bytes().len() > MAX_BODY_BYTES {
        return Err(Error::TooLarge);
    }
    Ok(())
}

/// Check a complete structured write against its observed predecessor.
/// `None` is allowed only after a complete empty read, never a tombstone/error.
/// This is preflight validation, not atomic compare-and-swap.
pub fn validate_transition(next: &Record, prior: Option<(&str, &Record)>) -> Result<(), Error> {
    next.to_body()?;
    if next.base_event_id.as_deref() != prior.map(|(id, _)| id) {
        return Err(Error::BaseMismatch);
    }
    if let Some((head_id, old)) = prior {
        if !hex_id(head_id) {
            return Err(Error::BaseMismatch);
        }
        old.validate(&next.scope)?;
        for previous in &old.claims {
            let current = next
                .claims
                .iter()
                .find(|c| c.id == previous.id)
                .ok_or(Error::Transition)?;
            if current == previous {
                continue;
            }
            let correction = current.correction.as_ref().ok_or(Error::Transition)?;
            if correction.prior_head != head_id
                || correction.prior_text != previous.text
                || (current.state == ClaimState::Withdrawn && current.text != previous.text)
            {
                return Err(Error::Transition);
            }
        }
    }
    for claim in &next.claims {
        let existed = prior.is_some_and(|(_, old)| old.claims.iter().any(|c| c.id == claim.id));
        if !existed && (claim.correction.is_some() || claim.state != ClaimState::Asserted) {
            return Err(Error::Transition);
        }
    }
    Ok(())
}

/// Existing monotonic timestamp rule, refusing future-poisoned heads.
pub fn write_created_at(now: u64, previous: Option<u64>) -> Result<u64, Error> {
    let timestamp = crate::engram::monotonic_created_at(now, previous);
    if previous == Some(u64::MAX) || timestamp > now.saturating_add(60) {
        return Err(Error::Clock);
    }
    Ok(timestamp)
}

#[cfg(test)]
#[path = "continuity_tests.rs"]
mod tests;
