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
//! `sessionRef`, a schema version, and — for the one case that needs it — an
//! explicit adoption reference. Anything else mutable belongs in the
//! lifecycle commands that follow it; a founder record that could be argued
//! about is not a founder record.
//!
//! # Two forms: fresh founding and explicit adoption
//!
//! A genesis event's content is exactly one of two shapes:
//!
//! - **Fresh founding** — `{"sessionRef", "v"}`. The signer founds a
//!   `sessionRef` no `session.create` history in the channel has ever used.
//! - **Legacy adoption** — `{"sessionRef", "v", "adopts": {"createEventId",
//!   "receiptEventId"}}`. The signer claims to be the founder of a
//!   `sessionRef` that predates genesis, and names the exact founding
//!   `session.create` (44221) and its joining lifecycle receipt (44224) as
//!   evidence. `adopts` carries exactly those two keys — nothing between,
//!   nothing beyond — and both values are 64-character lowercase hex event
//!   ids.
//!
//! Nothing else is accepted: not a third top-level key, not `adopts` present
//! but `null`, not an `adopts` object with a differently-named or additional
//! field. See [`decode_coding_session_genesis`].
//!
//! # Why adoption is a payload field, not a relay inference
//!
//! An earlier revision of this contract let the relay *infer* adoption: it
//! would compare a plain genesis's signer against a founder it projected
//! itself from local `session.create`/receipt history, and treat a match as
//! an implicit adoption. That was rejected. A signed event's meaning must
//! never depend on what one relay's local database happens to contain — the
//! same bytes would be "an adoption" on a relay that had replayed the old
//! history and "a fresh founding" (and thus a rejection, or worse, an
//! acceptance under a false premise) on one that had not. Multi-relay mirrors
//! and replicas could never agree, and no auditor could reconstruct a
//! genesis's justification from the event alone.
//!
//! So adoption is now a durable statement **inside the signed genesis
//! payload**. The relay still verifies the claim — it fetches the referenced
//! create and receipt, checks that the receipt genuinely joins the create,
//! that the create bears the same `sessionRef`, that the genesis signer is
//! the create's signer, and that both referenced events sit in the genesis's
//! own channel — but the *shape* of what is being claimed travels with the
//! event, not with whichever relay happens to be asked.
//!
//! A plain genesis (no `adopts`) over a `sessionRef` that `session.create`
//! history already uses in the channel is refused outright: the publisher
//! must resubmit with an explicit `adopts` reference rather than have the
//! relay guess at one.
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
//! `adopts` follows the identical discipline one level down: `createEventId`
//! and `receiptEventId` are event ids, not labels, and a relay resolves them
//! by direct lookup — never by scanning history for "a create that claims
//! this `sessionRef`". The genesis's own `sessionRef`/signer are then checked
//! *against* what those two specific events say, rather than derived from a
//! search over the channel's history.
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
/// Genesis content is two or three fixed fields, the largest form (adoption,
/// with its two 64-hex event ids) totalling well under 300 bytes. The ceiling
/// exists only so a malformed or padded submission is rejected before it is
/// parsed, not to leave room for growth — a field that needs room is a field
/// that does not belong here.
pub const MAX_GENESIS_CONTENT_BYTES: usize = 1024;

/// An explicit legacy-adoption reference: the founding `session.create`
/// (44221) and the lifecycle receipt (44224) that joins it, both by event id.
///
/// Carries exactly these two keys — nothing between, nothing beyond. See the
/// module doc for why adoption is a reference the relay verifies rather than
/// a claim it infers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionGenesisAdoption {
    /// Event id (64-character lowercase hex) of the founding `session.create`.
    pub create_event_id: String,
    /// Event id (64-character lowercase hex) of the lifecycle receipt that
    /// joins the founding create.
    pub receipt_event_id: String,
}

/// Durable coding-session genesis JSON payload.
///
/// The event's signer is the founder; the payload never restates it. A claimed
/// identity inside signed content is a second source of truth for something the
/// signature already settles, and the two would eventually disagree.
///
/// `adopts` is `None` for a fresh founding and `Some` for an explicit legacy
/// adoption — see the module doc's "Two forms" section. There is no third
/// state: [`decode_coding_session_genesis`] rejects an `adopts` key present
/// but `null`, since that is neither of the two accepted forms.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionGenesisPayload {
    /// The umbrella session this event founds (canonical lowercase UUID).
    pub session_ref: String,
    /// Schema version — must equal [`CODING_SESSION_GENESIS_SCHEMA_VERSION`].
    pub v: u64,
    /// Present only for an explicit legacy adoption.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adopts: Option<CodingSessionGenesisAdoption>,
}

impl CodingSessionGenesisPayload {
    /// Build a fresh-founding genesis payload at the current schema version.
    pub fn new(session_ref: impl Into<String>) -> Self {
        Self {
            session_ref: session_ref.into(),
            v: CODING_SESSION_GENESIS_SCHEMA_VERSION,
            adopts: None,
        }
    }

    /// Build an explicit legacy-adoption genesis payload at the current
    /// schema version, referencing the founding create and its joining
    /// receipt by event id.
    pub fn new_adoption(
        session_ref: impl Into<String>,
        create_event_id: impl Into<String>,
        receipt_event_id: impl Into<String>,
    ) -> Self {
        Self {
            session_ref: session_ref.into(),
            v: CODING_SESSION_GENESIS_SCHEMA_VERSION,
            adopts: Some(CodingSessionGenesisAdoption {
                create_event_id: create_event_id.into(),
                receipt_event_id: receipt_event_id.into(),
            }),
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
        if let Some(adopts) = &self.adopts {
            validate_event_id_hex("adopts.createEventId", &adopts.create_event_id)?;
            validate_event_id_hex("adopts.receiptEventId", &adopts.receipt_event_id)?;
        }
        Ok(())
    }
}

/// Check that `value` is a 64-character lowercase-hex Nostr event id.
fn validate_event_id_hex(field: &str, value: &str) -> Result<(), String> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(format!(
            "{field} must be a lowercase 64-hex event id (got {value:?})"
        ));
    }
    Ok(())
}

/// Strictly decode and validate signed genesis content.
///
/// Exactly two shapes are accepted: `{sessionRef, v}` or `{sessionRef, v,
/// adopts: {createEventId, receiptEventId}}` — nothing between, nothing
/// beyond. `adopts` present but not an object (including `null`) is rejected,
/// as is an `adopts` object missing either key or carrying an extra one.
/// Unlike the lifecycle command there is no *historical* form to
/// accommodate: genesis ships with the authority chain, so every genesis
/// event that will ever exist is written against one of these two shapes.
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

    // The presence of the `adopts` key (any value, including `null`) selects
    // which of the two field sets this payload must match exactly. A form
    // that omits `adopts` cannot smuggle it in as `null` here: `null` is a
    // present key, so it is checked against the adoption shape below, where
    // a non-object value fails the object check and the payload is rejected.
    let base_form = ["sessionRef", "v"];
    let adoption_form = ["sessionRef", "v", "adopts"];
    let has_adopts = object.contains_key("adopts");
    let expected: &[&str] = if has_adopts {
        &adoption_form
    } else {
        &base_form
    };
    let complete = expected.iter().all(|key| object.contains_key(*key));
    let recognized = object.keys().all(|key| expected.contains(&key.as_str()));
    if !complete || !recognized {
        return Err("coding-session genesis payload has missing or unsupported fields".into());
    }

    if has_adopts {
        let adopts_object = object
            .get("adopts")
            .and_then(Value::as_object)
            .ok_or_else(|| "coding-session genesis adopts must be an object".to_string())?;
        let adopts_expected = ["createEventId", "receiptEventId"];
        let adopts_complete = adopts_expected
            .iter()
            .all(|key| adopts_object.contains_key(*key));
        let adopts_recognized = adopts_object
            .keys()
            .all(|key| adopts_expected.contains(&key.as_str()));
        if !adopts_complete || !adopts_recognized {
            return Err("coding-session genesis adopts has missing or unsupported fields".into());
        }
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

    /// A syntactically valid 64-hex event id, distinct per repeated byte so
    /// two calls never collide.
    fn event_id_hex(byte: &str) -> String {
        byte.repeat(32)
    }

    fn genesis_content(session_ref_json: &str, v_json: &str) -> String {
        format!(r#"{{"sessionRef":{session_ref_json},"v":{v_json}}}"#)
    }

    #[test]
    fn validates_and_strictly_decodes_the_exact_contract() {
        let payload = CodingSessionGenesisPayload::new(session_reference());
        assert_eq!(payload.v, CODING_SESSION_GENESIS_SCHEMA_VERSION);
        assert!(payload.adopts.is_none());
        let content = serde_json::to_string(&payload).unwrap();
        assert_eq!(
            content,
            genesis_content("\"5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10\"", "1"),
            "the fresh-founding form must serialize without an `adopts` key at all"
        );
        assert_eq!(decode_coding_session_genesis(&content).unwrap(), payload);
    }

    /// The second accepted form: an explicit legacy adoption, carrying the
    /// founding create and its joining receipt by event id.
    #[test]
    fn validates_and_strictly_decodes_the_adoption_form() {
        let payload = CodingSessionGenesisPayload::new_adoption(
            session_reference(),
            event_id_hex("ab"),
            event_id_hex("cd"),
        );
        assert_eq!(payload.v, CODING_SESSION_GENESIS_SCHEMA_VERSION);
        let content = serde_json::to_string(&payload).unwrap();
        assert_eq!(
            content,
            format!(
                r#"{{"sessionRef":"{}","v":1,"adopts":{{"createEventId":"{}","receiptEventId":"{}"}}}}"#,
                session_reference(),
                event_id_hex("ab"),
                event_id_hex("cd"),
            )
        );
        assert_eq!(decode_coding_session_genesis(&content).unwrap(), payload);
    }

    /// Exactly two payload shapes exist. Anything with an `adopts` key that is
    /// present-but-`null`, incomplete, or carrying an extra field is neither
    /// the fresh-founding form nor the adoption form, and must be rejected —
    /// mirroring the lifecycle command's
    /// `rejects_action_shapes_between_and_beyond_the_two_forms` precedent.
    #[test]
    fn rejects_adoption_shapes_between_and_beyond_the_two_forms() {
        let create = event_id_hex("ab");
        let receipt = event_id_hex("cd");
        for rejected in [
            // `adopts` present but `null` is neither form: the fresh form
            // omits the key entirely, and the adoption form's value must be
            // an object.
            format!(
                r#"{{"sessionRef":"{}","v":1,"adopts":null}}"#,
                session_reference()
            ),
            // Adoption object missing `receiptEventId`.
            format!(
                r#"{{"sessionRef":"{}","v":1,"adopts":{{"createEventId":"{create}"}}}}"#,
                session_reference()
            ),
            // Adoption object missing `createEventId`.
            format!(
                r#"{{"sessionRef":"{}","v":1,"adopts":{{"receiptEventId":"{receipt}"}}}}"#,
                session_reference()
            ),
            // Adoption object with a third, smuggled field.
            format!(
                r#"{{"sessionRef":"{}","v":1,"adopts":{{"createEventId":"{create}","receiptEventId":"{receipt}","note":"trust me"}}}}"#,
                session_reference()
            ),
            // Adoption object that is an array, not an object.
            format!(
                r#"{{"sessionRef":"{}","v":1,"adopts":["{create}","{receipt}"]}}"#,
                session_reference()
            ),
            // Adoption object that is a bare string.
            format!(
                r#"{{"sessionRef":"{}","v":1,"adopts":"{create}"}}"#,
                session_reference()
            ),
            // A fourth top-level key alongside a well-formed adopts object.
            format!(
                r#"{{"sessionRef":"{}","v":1,"adopts":{{"createEventId":"{create}","receiptEventId":"{receipt}"}},"founder":"{create}"}}"#,
                session_reference()
            ),
            // `v` missing while `adopts` is present.
            format!(
                r#"{{"sessionRef":"{}","adopts":{{"createEventId":"{create}","receiptEventId":"{receipt}"}}}}"#,
                session_reference()
            ),
        ] {
            assert!(
                decode_coding_session_genesis(&rejected).is_err(),
                "should reject {rejected:?}"
            );
        }
    }

    /// The two adoption references must themselves be well-formed event ids —
    /// a malformed reference cannot be resolved by any relay, adoption or not.
    #[test]
    fn rejects_malformed_adoption_event_ids() {
        for (create, receipt) in [
            (event_id_hex("ab").to_uppercase(), event_id_hex("cd")),
            (event_id_hex("ab"), event_id_hex("cd").to_uppercase()),
            (event_id_hex("ab")[..63].to_owned(), event_id_hex("cd")),
            (event_id_hex("ab"), String::new()),
            (
                "not-hex-at-all-not-hex-at-all-not-hex-at-all-not-hex-at-all-gg".to_owned(),
                event_id_hex("cd"),
            ),
        ] {
            let payload =
                CodingSessionGenesisPayload::new_adoption(session_reference(), create, receipt);
            assert!(payload.validate().is_err());
            let content = serde_json::to_string(&payload).unwrap();
            assert!(
                decode_coding_session_genesis(&content).is_err(),
                "decode should reject malformed adoption ids in {content:?}"
            );
        }
    }

    /// Genesis has no historical form to tolerate, so "exactly two fields" (or
    /// exactly three, with `adopts`) means exactly that: nothing missing,
    /// nothing extra, nothing duplicated.
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
