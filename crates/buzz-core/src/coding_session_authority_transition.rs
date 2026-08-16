//! NIP-CSAT (draft): coding-session authority-chain transitions.
//!
//! Events use [`crate::kind::KIND_CODING_SESSION_AUTHORITY_TRANSITION`] and
//! public JSON. A transition is one append-only link in one session's
//! authority chain — the sequence of decisions about who may steer a session
//! after its [`crate::coding_session_genesis`] founded it.
//!
//! # Exactly one transition type today
//!
//! Only `grant-operator` is implemented — see
//! [`CodingSessionAuthorityTransitionType`]. The type is carried as a string
//! enum precisely so `revoke`, `transfer`, and `takeover` are additive
//! later: adding a variant does not change the shape of an existing,
//! already-signed transition, and a relay that only understands
//! `grant-operator` correctly rejects any other value as unknown rather than
//! guessing at its meaning.
//!
//! # The chain, not just the link
//!
//! A transition's content is exactly five fields: `genesisRef` (the session's
//! genesis event id — never a `sessionRef` label, for the same reason genesis
//! itself is resolved only by id, see the module doc on
//! [`crate::coding_session_genesis`]), `prevAccepted` (the previous accepted
//! transition's event id, or `null` for the chain's first link), `seq` (a
//! sequence number starting at 1 and incrementing by exactly one per accepted
//! transition), `type`, and `granteePubkey`.
//!
//! This module validates a transition's *self-consistency* only — that its
//! fields are well-formed and that `seq`/`prevAccepted` agree with each other
//! structurally (`seq == 1` if and only if `prevAccepted` is `null`). It
//! cannot and does not validate that a transition actually *extends* the
//! chain: that requires knowing the current accepted head, which is state the
//! relay's storage transaction alone can answer atomically, exactly as
//! genesis's per-`sessionRef` uniqueness does. See
//! `buzz_db::event::insert_coding_session_authority_transition_event`.
//!
//! # Why `prevAccepted` is always present, never omitted
//!
//! Unlike genesis's optional `adopts` field, `prevAccepted` is a required key
//! whose *value* is nullable — it is never simply absent. A transition that
//! omitted the key would be ambiguous between "the first link" and "a
//! malformed submission missing a field", and the decoder is written to
//! reject that ambiguity rather than resolve it by treating a missing key the
//! same as an explicit `null` (see [`decode_coding_session_authority_transition`]).

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The version tag placed on each coding-session authority-transition event.
pub const CODING_SESSION_AUTHORITY_TRANSITION_TAG_VERSION: &str = "csat1-1";
/// Maximum UTF-8 byte length for the complete signed event content.
///
/// Content is five fixed fields, the largest realistic encoding (two 64-hex
/// event ids, a small integer, the longest transition type, and a 64-hex
/// pubkey) totalling well under 300 bytes — the same "no room to grow"
/// reasoning as genesis's content ceiling.
pub const MAX_AUTHORITY_TRANSITION_CONTENT_BYTES: usize = 512;

/// One transition type. Only [`Self::GrantOperator`] is implemented; the enum
/// exists so `revoke`, `transfer`, and `takeover` are additive variants in a
/// future revision rather than a breaking change to this one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CodingSessionAuthorityTransitionType {
    /// Grants a pubkey standing to steer the session as an operator, without
    /// moving ownership. The only transition type this build accepts.
    GrantOperator,
}

/// Durable coding-session authority-transition JSON payload.
///
/// The event's signer is the party claiming to be authorized to extend the
/// chain — for `grant-operator`, the session's current owner. The payload
/// never restates the signer's identity; the signature already settles it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionAuthorityTransitionPayload {
    /// Event id (64-character lowercase hex) of the session's genesis. The
    /// chain is rooted here — never resolved by querying for a `sessionRef`.
    pub genesis_ref: String,
    /// Event id (64-character lowercase hex) of the previous accepted
    /// transition in this chain, or `None` for the chain's first link.
    pub prev_accepted: Option<String>,
    /// Sequence number: 1 for the first transition, incrementing by exactly 1
    /// per accepted transition thereafter.
    pub seq: u32,
    /// Which transition this is. Only [`CodingSessionAuthorityTransitionType::GrantOperator`]
    /// is accepted today.
    #[serde(rename = "type")]
    pub transition_type: CodingSessionAuthorityTransitionType,
    /// Pubkey (64-character lowercase hex) this transition grants operator
    /// standing to.
    pub grantee_pubkey: String,
}

impl CodingSessionAuthorityTransitionPayload {
    /// Build a `grant-operator` transition payload.
    pub fn new_grant_operator(
        genesis_ref: impl Into<String>,
        prev_accepted: Option<String>,
        seq: u32,
        grantee_pubkey: impl Into<String>,
    ) -> Self {
        Self {
            genesis_ref: genesis_ref.into(),
            prev_accepted,
            seq,
            transition_type: CodingSessionAuthorityTransitionType::GrantOperator,
            grantee_pubkey: grantee_pubkey.into(),
        }
    }

    /// Validate this payload's self-consistency before signing an event.
    ///
    /// This is a structural check only — it cannot know whether `seq` and
    /// `prevAccepted` actually agree with the chain's current head, since
    /// that is state only the relay's storage transaction can answer
    /// atomically. What it does enforce: every event id is well-formed hex,
    /// `seq` is never 0 (sequence numbers start at 1), and `seq == 1` if and
    /// only if `prevAccepted` is `None` — a transition cannot claim to be
    /// both "the first link" and "not the first link" at once.
    pub fn validate(&self) -> Result<(), String> {
        validate_event_id_hex("genesisRef", &self.genesis_ref)?;
        if let Some(prev) = &self.prev_accepted {
            validate_event_id_hex("prevAccepted", prev)?;
        }
        if self.seq == 0 {
            return Err("seq must start at 1 (0 is not a valid sequence number)".into());
        }
        if (self.seq == 1) != self.prev_accepted.is_none() {
            return Err("seq must be exactly 1 if and only if prevAccepted is null".into());
        }
        validate_event_id_hex("granteePubkey", &self.grantee_pubkey)?;
        Ok(())
    }
}

/// Check that `value` is a 64-character lowercase-hex Nostr id (event id or
/// pubkey — the two share an encoding).
fn validate_event_id_hex(field: &str, value: &str) -> Result<(), String> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(format!(
            "{field} must be a lowercase 64-hex value (got {value:?})"
        ));
    }
    Ok(())
}

/// Strictly decode and validate signed authority-transition content.
///
/// Exactly one shape is accepted: `{genesisRef, prevAccepted, seq, type,
/// granteePubkey}` — nothing between, nothing beyond, and `prevAccepted`'s
/// key must be present even though its value may be `null`. A `type` value
/// other than `"grant-operator"` fails to decode into
/// [`CodingSessionAuthorityTransitionType`] and is rejected the same as any
/// other malformed field.
pub fn decode_coding_session_authority_transition(
    content: &str,
) -> Result<CodingSessionAuthorityTransitionPayload, String> {
    if content.len() > MAX_AUTHORITY_TRANSITION_CONTENT_BYTES {
        return Err(format!(
            "coding-session authority-transition content exceeds {MAX_AUTHORITY_TRANSITION_CONTENT_BYTES} bytes"
        ));
    }

    let value: Value = serde_json::from_str(content)
        .map_err(|_| "malformed coding-session authority-transition payload".to_string())?;
    let object = value.as_object().ok_or_else(|| {
        "coding-session authority-transition payload must be an object".to_string()
    })?;

    const EXPECTED: [&str; 5] = ["genesisRef", "prevAccepted", "seq", "type", "granteePubkey"];
    let complete = EXPECTED.iter().all(|key| object.contains_key(*key));
    let recognized = object.keys().all(|key| EXPECTED.contains(&key.as_str()));
    if !complete || !recognized {
        return Err(
            "coding-session authority-transition payload has missing or unsupported fields".into(),
        );
    }

    // Decode a second time into the strict serde type: this preserves
    // serde's duplicate-field detection, which the `Value` map above cannot
    // represent (a duplicate JSON key collapses to one entry there), and it
    // is what actually enforces `prevAccepted`'s value being either a
    // 64-hex string or JSON `null` — never any other type.
    let payload: CodingSessionAuthorityTransitionPayload = serde_json::from_str(content)
        .map_err(|_| "malformed coding-session authority-transition payload".to_string())?;
    payload.validate()?;
    Ok(payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event_id_hex(byte: &str) -> String {
        byte.repeat(32)
    }

    fn valid_json(genesis_ref: &str, prev: &str, seq: u32, grantee: &str) -> String {
        format!(
            r#"{{"genesisRef":"{genesis_ref}","prevAccepted":{prev},"seq":{seq},"type":"grant-operator","granteePubkey":"{grantee}"}}"#
        )
    }

    #[test]
    fn validates_and_strictly_decodes_the_first_transition() {
        let payload = CodingSessionAuthorityTransitionPayload::new_grant_operator(
            event_id_hex("ab"),
            None,
            1,
            event_id_hex("cd"),
        );
        assert!(payload.validate().is_ok());
        let content = serde_json::to_string(&payload).unwrap();
        assert_eq!(
            content,
            valid_json(&event_id_hex("ab"), "null", 1, &event_id_hex("cd")),
        );
        assert_eq!(
            decode_coding_session_authority_transition(&content).unwrap(),
            payload
        );
    }

    #[test]
    fn validates_and_strictly_decodes_a_chained_transition() {
        let payload = CodingSessionAuthorityTransitionPayload::new_grant_operator(
            event_id_hex("ab"),
            Some(event_id_hex("11")),
            2,
            event_id_hex("cd"),
        );
        assert!(payload.validate().is_ok());
        let content = serde_json::to_string(&payload).unwrap();
        assert_eq!(
            decode_coding_session_authority_transition(&content).unwrap(),
            payload
        );
    }

    /// The five keys are the whole contract — a sixth (smuggled) field, or a
    /// missing one, must be rejected outright.
    #[test]
    fn rejects_smuggled_and_missing_fields() {
        let base = serde_json::json!({
            "genesisRef": event_id_hex("ab"),
            "prevAccepted": null,
            "seq": 1,
            "type": "grant-operator",
            "granteePubkey": event_id_hex("cd"),
        });

        // Smuggled sixth field.
        let mut smuggled = base.clone();
        smuggled["note"] = serde_json::json!("trust me");
        assert!(decode_coding_session_authority_transition(&smuggled.to_string()).is_err());

        // Missing prevAccepted entirely — must not be treated the same as an
        // explicit null.
        let mut missing_prev = base.clone();
        missing_prev.as_object_mut().unwrap().remove("prevAccepted");
        assert!(decode_coding_session_authority_transition(&missing_prev.to_string()).is_err());

        for key in ["genesisRef", "seq", "type", "granteePubkey"] {
            let mut missing = base.clone();
            missing.as_object_mut().unwrap().remove(key);
            assert!(
                decode_coding_session_authority_transition(&missing.to_string()).is_err(),
                "should reject payload missing {key}"
            );
        }
    }

    /// A duplicate top-level key is caught by the strict second decode, not
    /// by the `Value`-based key-set check (which cannot see it).
    #[test]
    fn rejects_duplicate_fields() {
        let duplicated = format!(
            r#"{{"genesisRef":"{gr}","genesisRef":"{gr}","prevAccepted":null,"seq":1,"type":"grant-operator","granteePubkey":"{gp}"}}"#,
            gr = event_id_hex("ab"),
            gp = event_id_hex("cd"),
        );
        assert!(decode_coding_session_authority_transition(&duplicated).is_err());
    }

    #[test]
    fn rejects_malformed_hex_ids() {
        let ok_grantee = event_id_hex("cd");
        for genesis_ref in [
            event_id_hex("ab").to_uppercase(),
            event_id_hex("ab")[..63].to_owned(),
            String::new(),
            "not-hex-at-all-not-hex-at-all-not-hex-at-all-not-hex-at-all-gg".to_owned(),
        ] {
            let content = valid_json(&genesis_ref, "null", 1, &ok_grantee);
            assert!(
                decode_coding_session_authority_transition(&content).is_err(),
                "should reject genesisRef {genesis_ref:?}"
            );
        }

        let ok_genesis = event_id_hex("ab");
        for grantee in [
            event_id_hex("cd").to_uppercase(),
            event_id_hex("cd")[..10].to_owned(),
        ] {
            let content = valid_json(&ok_genesis, "null", 1, &grantee);
            assert!(
                decode_coding_session_authority_transition(&content).is_err(),
                "should reject granteePubkey {grantee:?}"
            );
        }

        // A malformed prevAccepted value.
        let content = valid_json(&ok_genesis, "\"not-hex\"", 2, &ok_grantee);
        assert!(decode_coding_session_authority_transition(&content).is_err());
    }

    /// `seq` must start at 1, and its nullness must agree with
    /// `prevAccepted` — both directions of the mismatch are rejected.
    #[test]
    fn rejects_bad_seq() {
        let genesis_ref = event_id_hex("ab");
        let grantee = event_id_hex("cd");
        let prev = event_id_hex("11");

        // seq = 0 is never valid.
        assert!(decode_coding_session_authority_transition(&valid_json(
            &genesis_ref,
            "null",
            0,
            &grantee
        ))
        .is_err());

        // seq = 1 with a non-null prevAccepted: claims to be first and not-first.
        assert!(decode_coding_session_authority_transition(&format!(
            r#"{{"genesisRef":"{genesis_ref}","prevAccepted":"{prev}","seq":1,"type":"grant-operator","granteePubkey":"{grantee}"}}"#
        ))
        .is_err());

        // seq = 2 with a null prevAccepted: claims to extend a chain with no predecessor.
        assert!(decode_coding_session_authority_transition(&valid_json(
            &genesis_ref,
            "null",
            2,
            &grantee
        ))
        .is_err());
    }

    /// Only `"grant-operator"` decodes; every other string, and every
    /// non-string, is rejected the same way an unrecognized enum value
    /// should be.
    #[test]
    fn rejects_unknown_transition_types() {
        let genesis_ref = event_id_hex("ab");
        let grantee = event_id_hex("cd");
        for bad_type in [
            "\"revoke\"",
            "\"transfer\"",
            "\"GRANT-OPERATOR\"",
            "\"grant_operator\"",
            "1",
            "null",
        ] {
            let content = format!(
                r#"{{"genesisRef":"{genesis_ref}","prevAccepted":null,"seq":1,"type":{bad_type},"granteePubkey":"{grantee}"}}"#
            );
            assert!(
                decode_coding_session_authority_transition(&content).is_err(),
                "should reject type {bad_type}"
            );
        }
    }

    #[test]
    fn rejects_content_over_the_byte_ceiling() {
        let content = " ".repeat(MAX_AUTHORITY_TRANSITION_CONTENT_BYTES + 1);
        assert!(decode_coding_session_authority_transition(&content).is_err());
    }

    #[test]
    fn rejects_non_object_and_malformed_json() {
        for rejected in ["not json", "[1,2,3]", "\"a string\"", "42", "null"] {
            assert!(decode_coding_session_authority_transition(rejected).is_err());
        }
    }
}
