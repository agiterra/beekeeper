//! Coding-session genesis contract — the cryptographic origin of one umbrella
//! session.
//!
//! Events use [`crate::kind::KIND_CODING_SESSION_GENESIS`] and public JSON. The
//! signer is the session's **founder**: the human operator who brought the
//! umbrella into existence. Every later question of "who may steer this
//! session?" resolves back to this one signature, replacing the earlier
//! heuristic that inferred a founder from whichever `session.create` happened
//! to be joined to the earliest receipt.
//!
//! Genesis is deliberately the smallest event the fork defines. It carries a
//! `sessionRef` and a schema version and nothing else — no title, no project,
//! no provider. Anything mutable belongs in the lifecycle commands that follow
//! it; a founder record that could be argued about is not a founder record.
//!
//! # Why the reference also rides in a tag — and what the tag does *not* mean
//!
//! Unlike the lifecycle command's `sessionRef`, the genesis reference is
//! mirrored into a `csg-session` tag. Content is opaque to the relay's filter
//! and index layers, and the relay's storage transaction has to answer "is this
//! `(channel, sessionRef)` already founded?" atomically with the insert. That
//! probe is the tag's reason to exist: it is an **enforcement and diagnostic**
//! affordance for the relay and its operators. The tag is re-derived from the
//! decoded payload at ingest rather than trusted, so the two can never disagree.
//!
//! **Consumers must never select authority by this tag.** Authority resolves
//! one way only: through an explicit genesis **event id**, reached from the
//! receipt-joined create that names it. `{kinds:[44226], "#csg-session":[…]}`
//! is not a founder lookup and must not be written as one — it is a query whose
//! *correct* answer is "exactly one row", and a consumer that treats it as a
//! selection has already accepted that more than one answer is possible and
//! that it may pick among them. It may not. Two rows matching one
//! `(channel, sessionRef)` is diagnostic evidence that the uniqueness rule was
//! violated — a corruption to report, never an ambiguity to resolve by taking
//! the earliest, the newest, or the one with the most reachable signer.
//!
//! The distinction is the whole point of the kind: `sessionRef` is an umbrella
//! *label*, minted client-side and mirrorable anywhere, while canonical
//! identity is the genesis event id in its relay/community/channel context. The
//! same signed genesis mirrored to another relay is the same origin; an
//! independently signed genesis carrying the same UUID is a different session.
//! A consumer that resolves by label rather than by id cannot tell those apart.
//!
//! # Schema versioning
//!
//! `v` is an integer rather than the `schema` string used by 44220 and 44221.
//! The string form encodes a contract name that is already implied by the kind
//! number, and genesis has no room for a second interpretation of what it is.
//! Only the version can meaningfully change, so only the version is written.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::coding_session_lifecycle_command::validate_session_ref;

/// The currently supported coding-session genesis schema version.
pub const CODING_SESSION_GENESIS_SCHEMA_VERSION: u64 = 1;
/// The version tag placed on each coding-session genesis event.
pub const CODING_SESSION_GENESIS_TAG_VERSION: &str = "csg1-1";
/// Maximum UTF-8 byte length for the complete signed event content.
///
/// Genesis content is two fixed fields totalling well under 100 bytes. The
/// ceiling exists only so a malformed or padded submission is rejected before
/// it is parsed, not to leave room for growth — a field that needs room is a
/// field that does not belong here.
pub const MAX_GENESIS_CONTENT_BYTES: usize = 1024;

/// Durable coding-session genesis JSON payload.
///
/// The event's signer is the founder; the payload never restates it. A claimed
/// identity inside signed content is a second source of truth for something the
/// signature already settles, and the two would eventually disagree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionGenesisPayload {
    /// The umbrella session this event founds (canonical lowercase UUID).
    pub session_ref: String,
    /// Schema version — must equal [`CODING_SESSION_GENESIS_SCHEMA_VERSION`].
    pub v: u64,
}

impl CodingSessionGenesisPayload {
    /// Build a genesis payload at the current schema version.
    pub fn new(session_ref: impl Into<String>) -> Self {
        Self {
            session_ref: session_ref.into(),
            v: CODING_SESSION_GENESIS_SCHEMA_VERSION,
        }
    }

    /// Validate all payload fields before signing a genesis event.
    pub fn validate(&self) -> Result<(), String> {
        if self.v != CODING_SESSION_GENESIS_SCHEMA_VERSION {
            return Err(format!(
                "unsupported coding-session genesis schema version {}",
                self.v
            ));
        }
        // The canonical-form rule is shared with the lifecycle command's
        // `sessionRef` on purpose: the two references must be byte-comparable
        // for a lifecycle command to ever resolve to its founder. Only the
        // field name in the message differs.
        if validate_session_ref(&self.session_ref).is_err() {
            return Err(format!(
                "sessionRef must be a canonical lowercase hyphenated UUID (got {:?})",
                self.session_ref
            ));
        }
        Ok(())
    }
}

/// Strictly decode and validate signed genesis content.
///
/// Both fields are required and no others are tolerated. Unlike the lifecycle
/// command there is no historical form to accommodate: genesis ships with the
/// authority chain, so every genesis event that will ever exist is written
/// against this exact shape.
pub fn decode_coding_session_genesis(content: &str) -> Result<CodingSessionGenesisPayload, String> {
    if content.len() > MAX_GENESIS_CONTENT_BYTES {
        return Err(format!(
            "coding-session genesis content exceeds {MAX_GENESIS_CONTENT_BYTES} bytes"
        ));
    }

    let value: Value = serde_json::from_str(content)
        .map_err(|_| "malformed coding-session genesis payload".to_string())?;
    let object = value
        .as_object()
        .ok_or_else(|| "coding-session genesis payload must be an object".to_string())?;
    let expected = ["sessionRef", "v"];
    let complete = expected.iter().all(|key| object.contains_key(*key));
    let recognized = object.keys().all(|key| expected.contains(&key.as_str()));
    if !complete || !recognized {
        return Err("coding-session genesis payload has missing or unsupported fields".into());
    }

    // Decode a second time into the strict serde type. This preserves serde's
    // duplicate-field detection, which a Value alone cannot represent.
    let payload: CodingSessionGenesisPayload = serde_json::from_str(content)
        .map_err(|_| "malformed coding-session genesis payload".to_string())?;
    payload.validate()?;
    Ok(payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A canonical lowercase umbrella session reference.
    fn session_reference() -> String {
        "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10".to_owned()
    }

    fn genesis_content(session_ref_json: &str, v_json: &str) -> String {
        format!(r#"{{"sessionRef":{session_ref_json},"v":{v_json}}}"#)
    }

    #[test]
    fn validates_and_strictly_decodes_the_exact_contract() {
        let payload = CodingSessionGenesisPayload::new(session_reference());
        assert_eq!(payload.v, CODING_SESSION_GENESIS_SCHEMA_VERSION);
        let content = serde_json::to_string(&payload).unwrap();
        assert_eq!(
            content,
            genesis_content("\"5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10\"", "1")
        );
        assert_eq!(decode_coding_session_genesis(&content).unwrap(), payload);
    }

    /// Genesis has no historical form to tolerate, so "exactly two fields"
    /// means exactly two: nothing missing, nothing extra, nothing duplicated.
    #[test]
    fn rejects_missing_unknown_and_duplicate_fields() {
        for rejected in [
            r#"{"sessionRef":"5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10"}"#.to_owned(),
            r#"{"v":1}"#.to_owned(),
            r#"{}"#.to_owned(),
            // A founder identity restated in content: the signature already
            // settles this, and a second source of truth would drift.
            format!(
                r#"{{"sessionRef":"{}","v":1,"founder":"{}"}}"#,
                session_reference(),
                "ab".repeat(32)
            ),
            // A host path smuggled alongside the reference.
            format!(
                r#"{{"sessionRef":"{}","v":1,"cwd":"/tmp"}}"#,
                session_reference()
            ),
            format!(
                r#"{{"sessionRef":"{}","sessionRef":"{}","v":1}}"#,
                session_reference(),
                session_reference()
            ),
            r#"["5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10",1]"#.to_owned(),
            "not json".to_owned(),
        ] {
            assert!(
                decode_coding_session_genesis(&rejected).is_err(),
                "should reject {rejected:?}"
            );
        }
    }

    /// The reference is the founder record's whole subject, and it is compared
    /// byte-exactly against a lifecycle command's `sessionRef`. A tolerated
    /// spelling here would found an umbrella nothing could ever join.
    #[test]
    fn rejects_a_non_canonical_session_ref() {
        for rejected in [
            session_reference().to_uppercase(),
            session_reference().replace('-', ""),
            format!("{{{}}}", session_reference()),
            format!("urn:uuid:{}", session_reference()),
            session_reference()[..35].to_owned(),
            "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a1g".to_owned(),
            String::new(),
        ] {
            let mut payload = CodingSessionGenesisPayload::new(rejected.clone());
            assert!(payload.validate().is_err(), "should reject {rejected:?}");

            payload.session_ref = rejected.clone();
            let content = serde_json::to_string(&payload).unwrap();
            assert!(
                decode_coding_session_genesis(&content).is_err(),
                "decode should reject sessionRef {rejected:?}"
            );
        }
    }

    /// A version the relay does not understand is a rejection, never a
    /// best-effort read: a misread founder record is worse than none.
    #[test]
    fn rejects_unsupported_schema_versions() {
        for rejected in ["0", "2", "99"] {
            let content = genesis_content(&format!("\"{}\"", session_reference()), rejected);
            assert!(
                decode_coding_session_genesis(&content).is_err(),
                "should reject v={rejected}"
            );
        }
        // Non-integer versions never reach the version check — they fail to
        // decode into the strict type first, which is the same verdict.
        for rejected in ["\"1\"", "1.5", "null", "-1"] {
            let content = genesis_content(&format!("\"{}\"", session_reference()), rejected);
            assert!(
                decode_coding_session_genesis(&content).is_err(),
                "should reject v={rejected}"
            );
        }
    }

    #[test]
    fn rejects_content_over_the_byte_ceiling() {
        let content = " ".repeat(MAX_GENESIS_CONTENT_BYTES + 1);
        assert!(decode_coding_session_genesis(&content).is_err());
    }
}
