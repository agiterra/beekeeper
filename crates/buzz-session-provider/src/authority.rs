//! Verified consumption of a session's authority chain (R21).
//!
//! The provider's operator set for a genesis-bearing session is derived from
//! exactly two kinds of fact, and nothing else:
//!
//! 1. **Relay-signed acceptance receipts** (kind 40099, type
//!    `coding_session_authority_transition_accepted`) whose content explicitly
//!    names the session's locally witnessed `genesisRef`. Each receipt is
//!    verified against the relay identity the provider witnessed at connect
//!    time (the NIP-11 `self` pubkey — the same key NIP-29/NIP-43 direct
//!    clients to verify relay-authored events against).
//! 2. **The accepted kind 44228 transition itself**, resolved by the explicit
//!    `acceptedEventId` reference carried inside the verified receipt — never
//!    by tag query — and checked for signature, payload linkage back to the
//!    same genesis and envelope shape. Legacy steering-grant links additionally
//!    bind the signer to the session owner. Additive seat links are authorized
//!    by the relay receipt and ignored semantically by this steering reader,
//!    while still advancing its contiguous accepted head.
//!
//! Unaccepted 44228s, tag-query projections, and ordering heuristics are
//! never inputs. Receipts fold strictly by `seq`: a grant applies only when it
//! extends the applied chain contiguously, and a gap triggers a backfill
//! rather than a guess. The functions here are pure decisions over
//! already-fetched events; the querying and state mutation live on
//! [`crate::Provider`].

use nostr::Event;
use serde::Deserialize;
use uuid::Uuid;

use buzz_core::coding_session_authority_transition::{
    decode_coding_session_authority_transition, CodingSessionAuthorityTransitionType,
    CODING_SESSION_AUTHORITY_TRANSITION_TAG_VERSION,
};
use buzz_core::kind::{KIND_CODING_SESSION_AUTHORITY_TRANSITION, KIND_SYSTEM_MESSAGE};

/// The system-message `type` the relay stamps on authority-transition
/// acceptance receipts (see `buzz-relay`'s
/// `handle_coding_session_authority_transition_accepted`).
pub const ACCEPTANCE_RECEIPT_TYPE: &str = "coding_session_authority_transition_accepted";

/// One verified acceptance receipt: the relay's signed statement that a
/// specific authority transition became the chain head at `seq`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptedTransition {
    /// Genesis event id the accepted chain is rooted at.
    pub genesis_ref: String,
    /// Event id of the accepted kind 44228 transition.
    pub accepted_event_id: String,
    /// Chain sequence number of the accepted transition (starts at 1).
    pub seq: u32,
    /// Which transition was accepted — decides how the fold applies it.
    pub transition_type: CodingSessionAuthorityTransitionType,
    /// Pubkey the transition targets: the grantee for `grant-*`, the pubkey
    /// losing its grant for `revoke`.
    pub grantee_pubkey: String,
}

/// Raw receipt content. Unknown extra fields are tolerated (the relay may
/// grow the receipt), but every field consumed here is validated strictly.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReceiptContent {
    #[serde(rename = "type")]
    receipt_type: String,
    genesis_ref: String,
    accepted_event_id: String,
    seq: u32,
    transition_type: CodingSessionAuthorityTransitionType,
    grantee_pubkey: String,
}

/// Whether a system message's content claims to be an authority-transition
/// acceptance receipt. Used to route without logging noise: 40099 carries
/// many unrelated system rows (joins, leaves, thread events).
pub fn looks_like_acceptance_receipt(content: &str) -> bool {
    #[derive(Deserialize)]
    struct TypeOnly {
        #[serde(rename = "type")]
        receipt_type: Option<String>,
    }
    serde_json::from_str::<TypeOnly>(content)
        .ok()
        .and_then(|c| c.receipt_type)
        .as_deref()
        == Some(ACCEPTANCE_RECEIPT_TYPE)
}

/// Verify one kind 40099 event as an authority-transition acceptance receipt.
///
/// Checks, in order: kind, author is exactly the witnessed relay identity,
/// signature, channel scope (`h` tag), and content shape. Only the pinned
/// transition types pass — an acceptance for a type this build does not
/// understand fails to decode, which stalls the contiguous fold and thereby
/// blocks *later* grants too: fail closed, never skip a link whose meaning
/// is unknown.
pub fn verify_acceptance_receipt(
    event: &Event,
    relay_self_hex: &str,
    channel_id: Uuid,
) -> Result<AcceptedTransition, String> {
    if u32::from(event.kind.as_u16()) != KIND_SYSTEM_MESSAGE {
        return Err(format!(
            "acceptance receipt has kind {}, expected {KIND_SYSTEM_MESSAGE}",
            event.kind.as_u16()
        ));
    }
    if event.pubkey.to_hex() != relay_self_hex {
        return Err(format!(
            "acceptance receipt signer {} is not the relay identity {relay_self_hex}",
            event.pubkey.to_hex()
        ));
    }
    event
        .verify()
        .map_err(|error| format!("acceptance receipt signature is invalid: {error}"))?;
    let channel = channel_id.to_string();
    if !event.tags.iter().any(|tag| {
        let tag = tag.as_slice();
        tag.len() == 2 && tag[0] == "h" && tag[1] == channel
    }) {
        return Err("acceptance receipt is not scoped to the session channel".into());
    }
    let content: ReceiptContent = serde_json::from_str(&event.content)
        .map_err(|error| format!("acceptance receipt content is malformed: {error}"))?;
    if content.receipt_type != ACCEPTANCE_RECEIPT_TYPE {
        return Err(format!(
            "system message type {:?} is not an acceptance receipt",
            content.receipt_type
        ));
    }
    for (field, value) in [
        ("genesisRef", &content.genesis_ref),
        ("acceptedEventId", &content.accepted_event_id),
        ("granteePubkey", &content.grantee_pubkey),
    ] {
        if value.len() != 64
            || !value
                .chars()
                .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
        {
            return Err(format!(
                "acceptance receipt {field} is not a lowercase 64-hex id"
            ));
        }
    }
    if content.seq == 0 {
        return Err("acceptance receipt seq must start at 1".into());
    }
    Ok(AcceptedTransition {
        genesis_ref: content.genesis_ref,
        accepted_event_id: content.accepted_event_id,
        seq: content.seq,
        transition_type: content.transition_type,
        grantee_pubkey: content.grantee_pubkey,
    })
}

/// Defense in depth: verify the resolved kind 44228 transition against the
/// receipt that accepted it.
///
/// The caller resolved `event` by the receipt's explicit `acceptedEventId`
/// through `query_event_by_id`, which already verified the signature and the
/// id/kind match. This check binds the two artifacts together: the payload
/// restates exactly the facts the receipt asserted, the envelope is the
/// three-tag shape the relay validated at ingest. Legacy grant transition
/// signers are additionally bound to the session owner; additive seat links
/// rely on the relay-signed acceptance receipt for chain authorization and are
/// ignored semantically by this steering-ACL reader while still advancing its
/// accepted head.
pub fn verify_accepted_transition(
    event: &Event,
    accepted: &AcceptedTransition,
    channel_id: Uuid,
    owner_pubkey: &str,
) -> Result<(), String> {
    if u32::from(event.kind.as_u16()) != KIND_CODING_SESSION_AUTHORITY_TRANSITION {
        return Err(format!(
            "accepted transition has kind {}, expected {KIND_CODING_SESSION_AUTHORITY_TRANSITION}",
            event.kind.as_u16()
        ));
    }
    if event.id.to_hex() != accepted.accepted_event_id {
        return Err("resolved transition id does not match the receipt".into());
    }
    let payload = decode_coding_session_authority_transition(&event.content)
        .map_err(|error| format!("accepted transition payload is invalid: {error}"))?;
    let legacy_owner_only = matches!(
        payload.transition_type,
        CodingSessionAuthorityTransitionType::GrantOperator
            | CodingSessionAuthorityTransitionType::GrantViewer
            | CodingSessionAuthorityTransitionType::Revoke
    );
    if legacy_owner_only && event.pubkey.to_hex() != owner_pubkey {
        return Err(format!(
            "accepted transition signer {} is not the session owner {owner_pubkey}",
            event.pubkey.to_hex()
        ));
    }
    if payload.genesis_ref != accepted.genesis_ref {
        return Err("accepted transition names a different genesis than its receipt".into());
    }
    if payload.seq != accepted.seq {
        return Err("accepted transition seq does not match its receipt".into());
    }
    if payload.grantee_pubkey != accepted.grantee_pubkey {
        return Err("accepted transition grantee does not match its receipt".into());
    }
    if payload.transition_type != accepted.transition_type {
        return Err("accepted transition type does not match its receipt".into());
    }
    let tags: Vec<&[String]> = event.tags.iter().map(|tag| tag.as_slice()).collect();
    if tags.len() != 3 || tags.iter().any(|tag| tag.len() != 2) {
        return Err("accepted transition must carry exactly three two-field tags".into());
    }
    if tags[0][0] != "h" || tags[0][1] != channel_id.to_string() {
        return Err("accepted transition is not scoped to the session channel".into());
    }
    if tags[1][0] != "csat-v" || tags[1][1] != CODING_SESSION_AUTHORITY_TRANSITION_TAG_VERSION {
        return Err("accepted transition has an unsupported csat-v envelope".into());
    }
    if tags[2][0] != "csat-genesis" || tags[2][1] != accepted.genesis_ref {
        return Err("accepted transition csat-genesis does not match its receipt".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::{EventBuilder, Keys, Kind, Tag};

    fn relay_keys() -> Keys {
        Keys::generate()
    }

    fn receipt_content(genesis_ref: &str, accepted_id: &str, seq: u32, grantee: &str) -> String {
        serde_json::json!({
            "type": ACCEPTANCE_RECEIPT_TYPE,
            "genesisRef": genesis_ref,
            "acceptedEventId": accepted_id,
            "seq": seq,
            "transitionType": "grant-operator",
            "granteePubkey": grantee,
        })
        .to_string()
    }

    fn receipt_event(keys: &Keys, channel_id: Uuid, content: String) -> Event {
        EventBuilder::new(Kind::Custom(KIND_SYSTEM_MESSAGE as u16), content)
            .tags(vec![
                Tag::parse(["h", &channel_id.to_string()]).expect("tag")
            ])
            .sign_with_keys(keys)
            .expect("sign receipt")
    }

    fn transition_event(
        keys: &Keys,
        channel_id: Uuid,
        genesis_ref: &str,
        seq: u32,
        grantee: &str,
    ) -> Event {
        let payload =
            buzz_core::coding_session_authority_transition::CodingSessionAuthorityTransitionPayload::new_grant_operator(
                genesis_ref.to_owned(),
                (seq > 1).then(|| "22".repeat(32)),
                seq,
                grantee.to_owned(),
            );
        buzz_sdk::builders::build_coding_session_authority_transition(channel_id, &payload)
            .expect("builder")
            .sign_with_keys(keys)
            .expect("sign transition")
    }

    #[test]
    fn a_relay_signed_receipt_verifies_and_decodes() {
        let relay = relay_keys();
        let channel_id = Uuid::new_v4();
        let genesis_ref = "ab".repeat(32);
        let accepted_id = "cd".repeat(32);
        let grantee = "ef".repeat(32);
        let event = receipt_event(
            &relay,
            channel_id,
            receipt_content(&genesis_ref, &accepted_id, 1, &grantee),
        );
        let accepted = verify_acceptance_receipt(&event, &relay.public_key().to_hex(), channel_id)
            .expect("verifies");
        assert_eq!(accepted.genesis_ref, genesis_ref);
        assert_eq!(accepted.accepted_event_id, accepted_id);
        assert_eq!(accepted.seq, 1);
        assert_eq!(accepted.grantee_pubkey, grantee);
        assert!(looks_like_acceptance_receipt(&event.content));
    }

    #[test]
    fn a_receipt_signed_by_anything_but_the_relay_identity_is_rejected() {
        let relay = relay_keys();
        let impostor = Keys::generate();
        let channel_id = Uuid::new_v4();
        let content = receipt_content(&"ab".repeat(32), &"cd".repeat(32), 1, &"ef".repeat(32));
        let event = receipt_event(&impostor, channel_id, content);
        let error = verify_acceptance_receipt(&event, &relay.public_key().to_hex(), channel_id)
            .expect_err("must reject");
        assert!(error.contains("not the relay identity"), "{error}");
    }

    #[test]
    fn receipts_with_wrong_scope_type_or_fields_are_rejected() {
        let relay = relay_keys();
        let relay_hex = relay.public_key().to_hex();
        let channel_id = Uuid::new_v4();
        let good = receipt_content(&"ab".repeat(32), &"cd".repeat(32), 1, &"ef".repeat(32));

        // Wrong channel scope.
        let elsewhere = receipt_event(&relay, Uuid::new_v4(), good.clone());
        assert!(verify_acceptance_receipt(&elsewhere, &relay_hex, channel_id).is_err());

        // A different system-message type is not a receipt at all.
        let other_type = receipt_event(
            &relay,
            channel_id,
            serde_json::json!({"type": "member_joined"}).to_string(),
        );
        assert!(!looks_like_acceptance_receipt(&other_type.content));
        assert!(verify_acceptance_receipt(&other_type, &relay_hex, channel_id).is_err());

        // An unknown transition type must fail verification, not be skipped.
        // (`takeover` is reserved but not pinned; `revoke` decodes now.)
        let unknown_transition = receipt_event(
            &relay,
            channel_id,
            serde_json::json!({
                "type": ACCEPTANCE_RECEIPT_TYPE,
                "genesisRef": "ab".repeat(32),
                "acceptedEventId": "cd".repeat(32),
                "seq": 1,
                "transitionType": "takeover",
                "granteePubkey": "ef".repeat(32),
            })
            .to_string(),
        );
        assert!(verify_acceptance_receipt(&unknown_transition, &relay_hex, channel_id).is_err());

        // Malformed ids and a zero seq.
        for content in [
            receipt_content("not-hex", &"cd".repeat(32), 1, &"ef".repeat(32)),
            receipt_content(&"ab".repeat(32), &"CD".repeat(32), 1, &"ef".repeat(32)),
            receipt_content(&"ab".repeat(32), &"cd".repeat(32), 0, &"ef".repeat(32)),
        ] {
            let event = receipt_event(&relay, channel_id, content);
            assert!(verify_acceptance_receipt(&event, &relay_hex, channel_id).is_err());
        }
    }

    #[test]
    fn an_accepted_transition_verifies_against_its_receipt() {
        let owner = Keys::generate();
        let channel_id = Uuid::new_v4();
        let genesis_ref = "ab".repeat(32);
        let grantee = "ef".repeat(32);
        let transition = transition_event(&owner, channel_id, &genesis_ref, 1, &grantee);
        let accepted = AcceptedTransition {
            genesis_ref: genesis_ref.clone(),
            accepted_event_id: transition.id.to_hex(),
            seq: 1,
            transition_type: CodingSessionAuthorityTransitionType::GrantOperator,
            grantee_pubkey: grantee.clone(),
        };
        verify_accepted_transition(
            &transition,
            &accepted,
            channel_id,
            &owner.public_key().to_hex(),
        )
        .expect("verifies");
    }

    #[test]
    fn transitions_that_disagree_with_their_receipt_are_rejected() {
        let owner = Keys::generate();
        let owner_hex = owner.public_key().to_hex();
        let channel_id = Uuid::new_v4();
        let genesis_ref = "ab".repeat(32);
        let grantee = "ef".repeat(32);
        let transition = transition_event(&owner, channel_id, &genesis_ref, 1, &grantee);
        let accepted = AcceptedTransition {
            genesis_ref: genesis_ref.clone(),
            accepted_event_id: transition.id.to_hex(),
            seq: 1,
            transition_type: CodingSessionAuthorityTransitionType::GrantOperator,
            grantee_pubkey: grantee.clone(),
        };

        // Signed by someone other than the owner.
        let forged = transition_event(&Keys::generate(), channel_id, &genesis_ref, 1, &grantee);
        let forged_accepted = AcceptedTransition {
            accepted_event_id: forged.id.to_hex(),
            ..accepted.clone()
        };
        assert!(
            verify_accepted_transition(&forged, &forged_accepted, channel_id, &owner_hex)
                .expect_err("must reject")
                .contains("not the session owner")
        );

        // Receipt facts that disagree with the payload.
        for wrong in [
            AcceptedTransition {
                genesis_ref: "99".repeat(32),
                ..accepted.clone()
            },
            AcceptedTransition {
                seq: 2,
                ..accepted.clone()
            },
            AcceptedTransition {
                grantee_pubkey: "77".repeat(32),
                ..accepted.clone()
            },
            AcceptedTransition {
                accepted_event_id: "55".repeat(32),
                ..accepted.clone()
            },
        ] {
            assert!(
                verify_accepted_transition(&transition, &wrong, channel_id, &owner_hex).is_err()
            );
        }

        // Wrong channel scope on the transition envelope.
        assert!(
            verify_accepted_transition(&transition, &accepted, Uuid::new_v4(), &owner_hex).is_err()
        );
    }
}
