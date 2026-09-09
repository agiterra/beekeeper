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
//!    by the relay receipt and folded separately from steering grants while
//!    still advancing the same contiguous accepted head.
//!
//! Unaccepted 44228s, tag-query projections, and ordering heuristics are
//! never inputs. Receipts fold strictly by `seq`: a grant applies only when it
//! extends the applied chain contiguously, and a gap triggers a backfill
//! rather than a guess. The functions here are pure decisions over
//! already-fetched events; the querying and state mutation live on
//! [`crate::Provider`].

use std::collections::{BTreeMap, HashSet};

use nostr::Event;
use serde::Deserialize;
use uuid::Uuid;

use buzz_core::coding_session_authority_claim::{fold_current_claim, ClaimLink, ClaimState};
use buzz_core::coding_session_authority_transition::{
    decode_coding_session_authority_transition, CodingSessionAuthorityTransitionType,
    CODING_SESSION_AUTHORITY_TRANSITION_TAG_VERSION,
};
use buzz_core::kind::{KIND_CODING_SESSION_AUTHORITY_TRANSITION, KIND_SYSTEM_MESSAGE};

/// The system-message `type` the relay stamps on authority-transition
/// acceptance receipts (see `buzz-relay`'s
/// `handle_coding_session_authority_transition_accepted`).
pub const ACCEPTANCE_RECEIPT_TYPE: &str = "coding_session_authority_transition_accepted";

/// The system-message `type` the relay stamps on a whole-session deletion it
/// applied (`buzz_relay::handlers::side_effects::CODING_SESSION_DELETION_RECEIPT_TYPE`).
///
/// A provider cannot verify a project owner's deletion by itself — the owner is
/// not the founder, and project roles are the relay's own state — so this
/// receipt is the only evidence that makes an owner deletion retirable
/// (`docs/HANDOVER_IMPL.md` §3.2). Founder-signed deletions remain verifiable
/// without it, which is what keeps deletions older than this receipt readable.
pub const DELETION_RECEIPT_TYPE: &str = "coding_session_deletion_accepted";

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
    /// Exact normalized role for seat transitions; absent for legacy grants.
    pub role: Option<String>,
    /// The execution body a `takeover`/`transfer` named; absent for every
    /// other type.
    ///
    /// The fence compares this against the provider's own pubkey, so a receipt
    /// that dropped it would leave a returning machine knowing the session was
    /// claimed and not whether *it* is the machine now carrying the work.
    pub body_pubkey: Option<String>,
}

/// A relay-signed statement that a whole-session deletion was applied.
///
/// Four facts and no prose: the immutable genesis the deletion retired, its
/// session reference, the kind 5 event that did it, and the channel it
/// happened in. A provider retires the record on the strength of this,
/// **or** of a founder-signed deletion it verified itself; nothing else is
/// retirement authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptedDeletion {
    /// Genesis event id (lowercase 64-hex) of the retired umbrella.
    pub genesis_ref: String,
    /// The umbrella's session reference, as the genesis declared it.
    pub session_ref: String,
    /// Event id of the kind 5 deletion the relay applied.
    pub deletion_event_id: String,
    /// The channel the deletion happened in.
    pub channel_id: Uuid,
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
    #[serde(default)]
    role: Option<String>,
    /// Present exactly on `takeover`/`transfer` receipts. `serde(default)`
    /// because every receipt signed before claims existed omits it, and a
    /// build that refused those would stall the contiguous fold on history it
    /// already accepted.
    #[serde(default)]
    body_pubkey: Option<String>,
}

/// Raw deletion-receipt content, held to the same strictness.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeletionReceiptContent {
    #[serde(rename = "type")]
    receipt_type: String,
    genesis_ref: String,
    session_ref: String,
    deletion_event_id: String,
    channel_id: String,
}

/// Current facts derived from a complete, verified accepted authority chain.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CurrentAuthority {
    /// Active steering grants, actor pubkey to accepted grant event id.
    pub operator_grants: BTreeMap<String, String>,
    /// Active seats, actor pubkey to `(role, accepted grant event id)`.
    pub seats: BTreeMap<String, (String, String)>,
    /// Who holds this session and on which body — the fence's whole input.
    ///
    /// Folded by the one canonical rule
    /// ([`fold_current_claim`]) rather than by a second implementation here,
    /// so the relay's acceptance rules, this provider's fence and the Desktop
    /// twin cannot disagree about who took a session over. Three states, and
    /// consumers must keep them apart: `NoClaim` is the ordinary session,
    /// `Voided` keeps the fence up for everybody.
    pub claim: ClaimState,
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
    let is_seat = matches!(
        content.transition_type,
        CodingSessionAuthorityTransitionType::GrantSeat
            | CodingSessionAuthorityTransitionType::RevokeSeat
    );
    if is_seat != content.role.is_some() {
        return Err(if is_seat {
            "seat acceptance receipt must carry role".into()
        } else {
            "non-seat acceptance receipt must not carry role".into()
        });
    }
    if let Some(role) = content.role.as_deref() {
        buzz_core::coding_session_lifecycle_command::validate_role_slug(role)
            .map_err(|error| error.replace("action.role", "receipt.role"))?;
    }
    // A claim receipt must name the body, and nothing else may. A claim whose
    // body the receipt dropped could not be folded into a fence at all, and a
    // grant receipt carrying one is a relay this build does not understand —
    // both fail closed rather than fold to something plausible.
    let is_claim = content.transition_type.is_claim();
    match (is_claim, content.body_pubkey.as_deref()) {
        (true, Some(body)) => {
            if body.len() != 64
                || !body
                    .chars()
                    .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
            {
                return Err("acceptance receipt bodyPubkey is not a lowercase 64-hex id".to_owned());
            }
        }
        (true, None) => {
            return Err("claim acceptance receipt must carry bodyPubkey".into());
        }
        (false, Some(_)) => {
            return Err("non-claim acceptance receipt must not carry bodyPubkey".into());
        }
        (false, None) => {}
    }
    Ok(AcceptedTransition {
        genesis_ref: content.genesis_ref,
        accepted_event_id: content.accepted_event_id,
        seq: content.seq,
        transition_type: content.transition_type,
        grantee_pubkey: content.grantee_pubkey,
        role: content.role,
        body_pubkey: content.body_pubkey,
    })
}

/// Whether a system message's content claims to be a session-deletion receipt.
pub fn looks_like_deletion_receipt(content: &str) -> bool {
    #[derive(Deserialize)]
    struct TypeOnly {
        #[serde(rename = "type")]
        receipt_type: Option<String>,
    }
    serde_json::from_str::<TypeOnly>(content)
        .ok()
        .and_then(|c| c.receipt_type)
        .as_deref()
        == Some(DELETION_RECEIPT_TYPE)
}

/// Verify one kind 40099 event as a whole-session deletion receipt.
///
/// Checks, in order: kind, author is exactly the witnessed relay identity,
/// signature, and content shape. The channel is **read from the content and
/// returned** rather than compared against a caller's expectation, because the
/// caller's question is "was this record's genesis retired", and the record
/// knows its own channel; a receipt for another channel simply names another
/// genesis, which the caller's comparison already rejects.
///
/// Nothing here decides whether the deletion *should* have been allowed. The
/// relay authorized it (founder or project owner) and applied it; this
/// function proves only that the relay said so, under the key the provider
/// witnessed at connect time — the same standard the authority receipts are
/// held to.
///
/// # Errors
/// A sentence naming the check that failed.
pub fn verify_deletion_receipt(
    event: &Event,
    relay_self_hex: &str,
) -> Result<AcceptedDeletion, String> {
    if u32::from(event.kind.as_u16()) != KIND_SYSTEM_MESSAGE {
        return Err(format!(
            "deletion receipt has kind {}, expected {KIND_SYSTEM_MESSAGE}",
            event.kind.as_u16()
        ));
    }
    if event.pubkey.to_hex() != relay_self_hex {
        return Err(format!(
            "deletion receipt signer {} is not the relay identity {relay_self_hex}",
            event.pubkey.to_hex()
        ));
    }
    event
        .verify()
        .map_err(|error| format!("deletion receipt signature is invalid: {error}"))?;
    let content: DeletionReceiptContent = serde_json::from_str(&event.content)
        .map_err(|error| format!("deletion receipt content is malformed: {error}"))?;
    if content.receipt_type != DELETION_RECEIPT_TYPE {
        return Err(format!(
            "system message type {:?} is not a deletion receipt",
            content.receipt_type
        ));
    }
    for (field, value) in [
        ("genesisRef", &content.genesis_ref),
        ("deletionEventId", &content.deletion_event_id),
    ] {
        if value.len() != 64
            || !value
                .chars()
                .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
        {
            return Err(format!(
                "deletion receipt {field} is not a lowercase 64-hex id"
            ));
        }
    }
    let session_ref = Uuid::parse_str(&content.session_ref)
        .map_err(|_| "deletion receipt sessionRef is not a UUID".to_owned())?;
    if session_ref.to_string() != content.session_ref {
        return Err("deletion receipt sessionRef is not a canonical lowercase UUID".into());
    }
    let channel_id = Uuid::parse_str(&content.channel_id)
        .map_err(|_| "deletion receipt channelId is not a UUID".to_owned())?;
    // Defense in depth against a receipt that names one channel in its content
    // and was published in another: the scope a reader subscribed to is the `h`
    // tag, and a mismatch means the two halves disagree about where this
    // happened.
    let scoped_here = event.tags.iter().any(|tag| {
        let tag = tag.as_slice();
        tag.len() == 2 && tag[0] == "h" && tag[1] == channel_id.to_string()
    });
    if !scoped_here {
        return Err("deletion receipt is not scoped to the channel it names".into());
    }
    Ok(AcceptedDeletion {
        genesis_ref: content.genesis_ref,
        session_ref: content.session_ref,
        deletion_event_id: content.deletion_event_id,
        channel_id,
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
/// folded separately from steering grants while still advancing the same
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
    event
        .verify()
        .map_err(|error| format!("accepted transition signature is invalid: {error}"))?;
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
    if payload.role != accepted.role {
        return Err("accepted transition role does not match its receipt".into());
    }
    // The body is the fence's whole input, so the receipt and the signed link
    // must agree about it exactly — a provider that trusted the receipt alone
    // could be fenced onto a machine the claimant never named.
    if payload.body_pubkey != accepted.body_pubkey {
        return Err("accepted transition bodyPubkey does not match its receipt".into());
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

/// Fold a complete verified accepted chain into current steering grants and seats.
///
/// Each tuple must contain a receipt already checked by
/// [`verify_acceptance_receipt`] and its exact transition already checked by
/// [`verify_accepted_transition`]. This function additionally proves chain
/// contiguity and `prevAccepted` linkage before exposing any authority.
pub fn fold_current_authority(
    links: &[(AcceptedTransition, Event)],
) -> Result<CurrentAuthority, String> {
    let mut ordered = Vec::with_capacity(links.len());
    for (accepted, event) in links {
        let payload = decode_coding_session_authority_transition(&event.content)
            .map_err(|error| format!("accepted transition payload is invalid: {error}"))?;
        ordered.push((accepted, payload));
    }
    ordered.sort_by_key(|(accepted, _)| accepted.seq);
    let mut previous: Option<String> = None;
    let mut state = CurrentAuthority::default();
    let mut seen_seq = HashSet::new();
    // Collected in chain order and folded once at the end, by the shared rule.
    let mut claim_links: Vec<ClaimLink> = Vec::with_capacity(links.len());
    for (index, (accepted, payload)) in ordered.into_iter().enumerate() {
        let expected_seq = (index + 1) as u32;
        if accepted.seq != expected_seq || !seen_seq.insert(accepted.seq) {
            return Err(format!(
                "accepted authority chain is not contiguous at seq {expected_seq}"
            ));
        }
        if payload.prev_accepted != previous {
            return Err(format!(
                "accepted transition {} does not extend the previous head",
                accepted.accepted_event_id
            ));
        }
        claim_links.push(ClaimLink {
            seq: accepted.seq,
            accepted_event_id: accepted.accepted_event_id.clone(),
            transition_type: accepted.transition_type,
            grantee_pubkey: accepted.grantee_pubkey.clone(),
            body_pubkey: accepted.body_pubkey.clone(),
        });
        match accepted.transition_type {
            CodingSessionAuthorityTransitionType::GrantOperator => {
                state.operator_grants.insert(
                    accepted.grantee_pubkey.clone(),
                    accepted.accepted_event_id.clone(),
                );
            }
            CodingSessionAuthorityTransitionType::GrantViewer
            | CodingSessionAuthorityTransitionType::Revoke => {
                state.operator_grants.remove(&accepted.grantee_pubkey);
            }
            CodingSessionAuthorityTransitionType::GrantSeat => {
                let role = accepted
                    .role
                    .clone()
                    .ok_or_else(|| "verified grant-seat receipt has no role".to_owned())?;
                state.seats.insert(
                    accepted.grantee_pubkey.clone(),
                    (role, accepted.accepted_event_id.clone()),
                );
            }
            // A claim moves who is carrying the session, never who may steer
            // it or who holds a seat: those are the grant and seat sets above,
            // and the claim is folded separately below.
            CodingSessionAuthorityTransitionType::Takeover
            | CodingSessionAuthorityTransitionType::Transfer => {}
            CodingSessionAuthorityTransitionType::RevokeSeat => {
                let role = accepted
                    .role
                    .as_deref()
                    .ok_or_else(|| "verified revoke-seat receipt has no role".to_owned())?;
                if state
                    .seats
                    .get(&accepted.grantee_pubkey)
                    .is_some_and(|(active, _)| active == role)
                {
                    state.seats.remove(&accepted.grantee_pubkey);
                } else {
                    return Err("revoke-seat does not match the active accepted seat".into());
                }
            }
        }
        previous = Some(accepted.accepted_event_id.clone());
    }
    state.claim = fold_current_claim(claim_links.into_iter());
    Ok(state)
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

    #[allow(clippy::too_many_arguments)]
    fn seat_transition_event(
        keys: &Keys,
        channel_id: Uuid,
        genesis_ref: &str,
        prev_accepted: Option<String>,
        seq: u32,
        grantee: &str,
        role: &str,
        revoke: bool,
    ) -> Event {
        let payload = if revoke {
            buzz_core::coding_session_authority_transition::CodingSessionAuthorityTransitionPayload::new_revoke_seat(
                genesis_ref.to_owned(),
                prev_accepted,
                seq,
                grantee.to_owned(),
                role.to_owned(),
            )
        } else {
            buzz_core::coding_session_authority_transition::CodingSessionAuthorityTransitionPayload::new_grant_seat(
                genesis_ref.to_owned(),
                prev_accepted,
                seq,
                grantee.to_owned(),
                role.to_owned(),
            )
        };
        buzz_sdk::builders::build_coding_session_authority_transition(channel_id, &payload)
            .expect("builder")
            .sign_with_keys(keys)
            .expect("sign seat transition")
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
        // (`takeover` and `transfer` are pinned since the handover work; a
        // type this build has never heard of still stalls the fold.)
        let unknown_transition = receipt_event(
            &relay,
            channel_id,
            serde_json::json!({
                "type": ACCEPTANCE_RECEIPT_TYPE,
                "genesisRef": "ab".repeat(32),
                "acceptedEventId": "cd".repeat(32),
                "seq": 1,
                "transitionType": "handover",
                "granteePubkey": "ef".repeat(32),
            })
            .to_string(),
        );
        assert!(verify_acceptance_receipt(&unknown_transition, &relay_hex, channel_id).is_err());

        // A claim receipt with no body cannot be folded into a fence.
        let bodyless_claim = receipt_event(
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
        assert!(verify_acceptance_receipt(&bodyless_claim, &relay_hex, channel_id).is_err());

        // And a grant receipt must not carry one.
        let grant_with_body = receipt_event(
            &relay,
            channel_id,
            serde_json::json!({
                "type": ACCEPTANCE_RECEIPT_TYPE,
                "genesisRef": "ab".repeat(32),
                "acceptedEventId": "cd".repeat(32),
                "seq": 1,
                "transitionType": "grant-operator",
                "granteePubkey": "ef".repeat(32),
                "bodyPubkey": "dd".repeat(32),
            })
            .to_string(),
        );
        assert!(verify_acceptance_receipt(&grant_with_body, &relay_hex, channel_id).is_err());

        let missing_seat_role = receipt_event(
            &relay,
            channel_id,
            serde_json::json!({
                "type": ACCEPTANCE_RECEIPT_TYPE,
                "genesisRef": "ab".repeat(32),
                "acceptedEventId": "cd".repeat(32),
                "seq": 1,
                "transitionType": "grant-seat",
                "granteePubkey": "ef".repeat(32),
            })
            .to_string(),
        );
        assert!(verify_acceptance_receipt(&missing_seat_role, &relay_hex, channel_id).is_err());

        let legacy_role = receipt_event(
            &relay,
            channel_id,
            serde_json::json!({
                "type": ACCEPTANCE_RECEIPT_TYPE,
                "genesisRef": "ab".repeat(32),
                "acceptedEventId": "cd".repeat(32),
                "seq": 1,
                "transitionType": "grant-operator",
                "granteePubkey": "ef".repeat(32),
                "role": "lead",
            })
            .to_string(),
        );
        assert!(verify_acceptance_receipt(&legacy_role, &relay_hex, channel_id).is_err());

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
            role: None,
            body_pubkey: None,
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
            role: None,
            body_pubkey: None,
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

        let mut invalid_signature = transition.clone();
        invalid_signature.content.push(' ');
        assert!(
            verify_accepted_transition(&invalid_signature, &accepted, channel_id, &owner_hex)
                .expect_err("tampered transition must fail")
                .contains("signature is invalid")
        );
    }

    #[test]
    fn accepted_seat_chain_folds_role_parity_and_revocation() {
        let founder = Keys::generate();
        let channel_id = Uuid::new_v4();
        let genesis_ref = "ab".repeat(32);
        let actor = "ef".repeat(32);
        let grant = seat_transition_event(
            &founder,
            channel_id,
            &genesis_ref,
            None,
            1,
            &actor,
            "builder",
            false,
        );
        let revoke = seat_transition_event(
            &founder,
            channel_id,
            &genesis_ref,
            Some(grant.id.to_hex()),
            2,
            &actor,
            "builder",
            true,
        );
        let links = vec![
            (
                AcceptedTransition {
                    genesis_ref: genesis_ref.clone(),
                    accepted_event_id: grant.id.to_hex(),
                    seq: 1,
                    transition_type: CodingSessionAuthorityTransitionType::GrantSeat,
                    grantee_pubkey: actor.clone(),
                    role: Some("builder".into()),
                    body_pubkey: None,
                },
                grant,
            ),
            (
                AcceptedTransition {
                    genesis_ref,
                    accepted_event_id: revoke.id.to_hex(),
                    seq: 2,
                    transition_type: CodingSessionAuthorityTransitionType::RevokeSeat,
                    grantee_pubkey: actor,
                    role: Some("builder".into()),
                    body_pubkey: None,
                },
                revoke,
            ),
        ];
        assert!(fold_current_authority(&links)
            .expect("contiguous accepted chain")
            .seats
            .is_empty());
    }

    // ── Claims and retirement (docs/HANDOVER_IMPL.md §1, §3.2) ─────────────

    fn claim_receipt_content(
        genesis_ref: &str,
        accepted_id: &str,
        seq: u32,
        grantee: &str,
        body: Option<&str>,
        transition_type: &str,
    ) -> String {
        let mut content = serde_json::json!({
            "type": ACCEPTANCE_RECEIPT_TYPE,
            "genesisRef": genesis_ref,
            "acceptedEventId": accepted_id,
            "seq": seq,
            "transitionType": transition_type,
            "granteePubkey": grantee,
        });
        if let (Some(object), Some(body)) = (content.as_object_mut(), body) {
            object.insert("bodyPubkey".into(), serde_json::json!(body));
        }
        content.to_string()
    }

    #[allow(clippy::too_many_arguments)]
    fn claim_transition_event(
        keys: &Keys,
        channel_id: Uuid,
        genesis_ref: &str,
        prev_accepted: Option<String>,
        seq: u32,
        claimant: &str,
        body_pubkey: &str,
        transfer: bool,
    ) -> Event {
        let payload = if transfer {
            buzz_core::coding_session_authority_transition::CodingSessionAuthorityTransitionPayload::new_transfer(
                genesis_ref.to_owned(),
                prev_accepted,
                seq,
                claimant.to_owned(),
                body_pubkey.to_owned(),
            )
        } else {
            buzz_core::coding_session_authority_transition::CodingSessionAuthorityTransitionPayload::new_takeover(
                genesis_ref.to_owned(),
                prev_accepted,
                seq,
                claimant.to_owned(),
                body_pubkey.to_owned(),
            )
        };
        buzz_sdk::builders::build_coding_session_authority_transition(channel_id, &payload)
            .expect("builder")
            .sign_with_keys(keys)
            .expect("sign claim transition")
    }

    #[test]
    fn a_claim_receipt_verifies_and_carries_its_body() {
        let relay = relay_keys();
        let channel_id = Uuid::new_v4();
        let body = "dd".repeat(32);
        let event = receipt_event(
            &relay,
            channel_id,
            claim_receipt_content(
                &"ab".repeat(32),
                &"cd".repeat(32),
                2,
                &"ef".repeat(32),
                Some(&body),
                "takeover",
            ),
        );
        let accepted = verify_acceptance_receipt(&event, &relay.public_key().to_hex(), channel_id)
            .expect("verifies");
        assert_eq!(
            accepted.transition_type,
            CodingSessionAuthorityTransitionType::Takeover
        );
        assert_eq!(accepted.body_pubkey.as_deref(), Some(body.as_str()));
    }

    /// The receipt and the signed link must agree about the body: the fence
    /// compares it against this provider's own key, so a disagreement is a
    /// machine being told it is or is not the one carrying the work.
    #[test]
    fn a_transition_whose_body_disagrees_with_its_receipt_is_rejected() {
        let claimant = Keys::generate();
        let channel_id = Uuid::new_v4();
        let genesis_ref = "ab".repeat(32);
        let body = "dd".repeat(32);
        let transition = claim_transition_event(
            &claimant,
            channel_id,
            &genesis_ref,
            None,
            1,
            &claimant.public_key().to_hex(),
            &body,
            false,
        );
        let accepted = AcceptedTransition {
            genesis_ref: genesis_ref.clone(),
            accepted_event_id: transition.id.to_hex(),
            seq: 1,
            transition_type: CodingSessionAuthorityTransitionType::Takeover,
            grantee_pubkey: claimant.public_key().to_hex(),
            role: None,
            body_pubkey: Some(body),
        };
        // The owner check does not apply to a claim: the relay's receipt is
        // what authorized it, exactly as for a seat link.
        verify_accepted_transition(
            &transition,
            &accepted,
            channel_id,
            &Keys::generate().public_key().to_hex(),
        )
        .expect("a claim is authorized by its receipt, not by the owner rule");

        let wrong_body = AcceptedTransition {
            body_pubkey: Some("99".repeat(32)),
            ..accepted.clone()
        };
        assert!(
            verify_accepted_transition(&transition, &wrong_body, channel_id, &"11".repeat(32))
                .expect_err("must reject")
                .contains("bodyPubkey does not match")
        );

        let no_body = AcceptedTransition {
            body_pubkey: None,
            ..accepted
        };
        assert!(
            verify_accepted_transition(&transition, &no_body, channel_id, &"11".repeat(32))
                .is_err()
        );
    }

    #[test]
    fn the_accepted_chain_folds_a_claim_and_a_regrant_does_not_restore_it() {
        let founder = Keys::generate();
        let claimant = Keys::generate();
        let channel_id = Uuid::new_v4();
        let genesis_ref = "ab".repeat(32);
        let body = "dd".repeat(32);
        let claimant_hex = claimant.public_key().to_hex();

        let grant = transition_event(&founder, channel_id, &genesis_ref, 1, &claimant_hex);
        let takeover = claim_transition_event(
            &claimant,
            channel_id,
            &genesis_ref,
            Some(grant.id.to_hex()),
            2,
            &claimant_hex,
            &body,
            false,
        );
        let links = vec![
            (
                AcceptedTransition {
                    genesis_ref: genesis_ref.clone(),
                    accepted_event_id: grant.id.to_hex(),
                    seq: 1,
                    transition_type: CodingSessionAuthorityTransitionType::GrantOperator,
                    grantee_pubkey: claimant_hex.clone(),
                    role: None,
                    body_pubkey: None,
                },
                grant.clone(),
            ),
            (
                AcceptedTransition {
                    genesis_ref: genesis_ref.clone(),
                    accepted_event_id: takeover.id.to_hex(),
                    seq: 2,
                    transition_type: CodingSessionAuthorityTransitionType::Takeover,
                    grantee_pubkey: claimant_hex.clone(),
                    role: None,
                    body_pubkey: Some(body.clone()),
                },
                takeover.clone(),
            ),
        ];
        let authority = fold_current_authority(&links).expect("contiguous chain");
        let claim = authority.claim.active().expect("an active claim");
        assert_eq!(claim.claimant, claimant_hex);
        assert_eq!(claim.body_pubkey, body);
        assert_eq!(claim.accepted_event_id, takeover.id.to_hex());
        // A claim does not add a steering grant of its own; the one the
        // founder gave is still the only one.
        assert_eq!(authority.operator_grants.len(), 1);

        // Revoke the claimant, then regrant the same pubkey: standing to
        // steer comes back, the claim does not.
        let revoke_payload = buzz_core::coding_session_authority_transition::CodingSessionAuthorityTransitionPayload::new(
            CodingSessionAuthorityTransitionType::Revoke,
            genesis_ref.clone(),
            Some(takeover.id.to_hex()),
            3,
            claimant_hex.clone(),
        );
        let revoke = buzz_sdk::builders::build_coding_session_authority_transition(
            channel_id,
            &revoke_payload,
        )
        .expect("builder")
        .sign_with_keys(&founder)
        .expect("sign revoke");
        let regrant_payload = buzz_core::coding_session_authority_transition::CodingSessionAuthorityTransitionPayload::new_grant_operator(
            genesis_ref.clone(),
            Some(revoke.id.to_hex()),
            4,
            claimant_hex.clone(),
        );
        let regrant = buzz_sdk::builders::build_coding_session_authority_transition(
            channel_id,
            &regrant_payload,
        )
        .expect("builder")
        .sign_with_keys(&founder)
        .expect("sign regrant");

        let mut voided_links = links;
        voided_links.push((
            AcceptedTransition {
                genesis_ref: genesis_ref.clone(),
                accepted_event_id: revoke.id.to_hex(),
                seq: 3,
                transition_type: CodingSessionAuthorityTransitionType::Revoke,
                grantee_pubkey: claimant_hex.clone(),
                role: None,
                body_pubkey: None,
            },
            revoke.clone(),
        ));
        voided_links.push((
            AcceptedTransition {
                genesis_ref,
                accepted_event_id: regrant.id.to_hex(),
                seq: 4,
                transition_type: CodingSessionAuthorityTransitionType::GrantOperator,
                grantee_pubkey: claimant_hex.clone(),
                role: None,
                body_pubkey: None,
            },
            regrant,
        ));
        let authority = fold_current_authority(&voided_links).expect("contiguous chain");
        assert!(
            matches!(&authority.claim, ClaimState::Voided { last, .. } if last.claimant == claimant_hex),
            "a regrant must not resurrect a voided claim: {:?}",
            authority.claim
        );
        assert!(authority.claim.active().is_none());
        // …and the regrant did restore the steering grant, which is the
        // distinction the three-state fold exists to keep.
        assert!(authority.operator_grants.contains_key(&claimant_hex));
    }

    #[test]
    fn a_chain_with_no_claim_folds_to_no_claim() {
        let founder = Keys::generate();
        let channel_id = Uuid::new_v4();
        let genesis_ref = "ab".repeat(32);
        let grantee = "ef".repeat(32);
        let grant = transition_event(&founder, channel_id, &genesis_ref, 1, &grantee);
        let links = vec![(
            AcceptedTransition {
                genesis_ref,
                accepted_event_id: grant.id.to_hex(),
                seq: 1,
                transition_type: CodingSessionAuthorityTransitionType::GrantOperator,
                grantee_pubkey: grantee,
                role: None,
                body_pubkey: None,
            },
            grant,
        )];
        assert_eq!(
            fold_current_authority(&links).expect("chain").claim,
            ClaimState::NoClaim
        );
    }

    fn deletion_receipt_event(keys: &Keys, channel_id: Uuid, content: serde_json::Value) -> Event {
        EventBuilder::new(
            Kind::Custom(KIND_SYSTEM_MESSAGE as u16),
            content.to_string(),
        )
        .tags(vec![
            Tag::parse(["h", &channel_id.to_string()]).expect("tag")
        ])
        .sign_with_keys(keys)
        .expect("sign deletion receipt")
    }

    fn deletion_receipt_content(
        genesis_ref: &str,
        session_ref: &str,
        deletion_event_id: &str,
        channel_id: Uuid,
    ) -> serde_json::Value {
        serde_json::json!({
            "type": DELETION_RECEIPT_TYPE,
            "genesisRef": genesis_ref,
            "sessionRef": session_ref,
            "deletionEventId": deletion_event_id,
            "channelId": channel_id.to_string(),
        })
    }

    #[test]
    fn a_relay_signed_deletion_receipt_verifies_and_names_the_retired_genesis() {
        let relay = relay_keys();
        let channel_id = Uuid::new_v4();
        let session_ref = Uuid::new_v4().to_string();
        let genesis_ref = "ab".repeat(32);
        let deletion_event_id = "cd".repeat(32);
        let event = deletion_receipt_event(
            &relay,
            channel_id,
            deletion_receipt_content(&genesis_ref, &session_ref, &deletion_event_id, channel_id),
        );
        assert!(looks_like_deletion_receipt(&event.content));
        let accepted =
            verify_deletion_receipt(&event, &relay.public_key().to_hex()).expect("verifies");
        assert_eq!(accepted.genesis_ref, genesis_ref);
        assert_eq!(accepted.session_ref, session_ref);
        assert_eq!(accepted.deletion_event_id, deletion_event_id);
        assert_eq!(accepted.channel_id, channel_id);
    }

    /// Retirement is terminal, so its evidence is held to the same standard as
    /// an authority receipt: the relay's own key, a valid signature, a
    /// receipt type this build knows, well-formed ids, and a scope that agrees
    /// with what the content claims.
    #[test]
    fn a_deletion_receipt_that_is_not_the_relays_own_word_is_refused() {
        let relay = relay_keys();
        let relay_hex = relay.public_key().to_hex();
        let channel_id = Uuid::new_v4();
        let session_ref = Uuid::new_v4().to_string();
        let good =
            deletion_receipt_content(&"ab".repeat(32), &session_ref, &"cd".repeat(32), channel_id);

        // Signed by somebody else.
        let impostor = deletion_receipt_event(&Keys::generate(), channel_id, good.clone());
        assert!(verify_deletion_receipt(&impostor, &relay_hex)
            .expect_err("must reject")
            .contains("not the relay identity"));

        // Tampered after signing.
        let mut tampered = deletion_receipt_event(&relay, channel_id, good.clone());
        tampered.content.push(' ');
        assert!(verify_deletion_receipt(&tampered, &relay_hex).is_err());

        // A different system-message type is not a deletion receipt at all.
        let other = deletion_receipt_event(
            &relay,
            channel_id,
            serde_json::json!({"type": "member_joined"}),
        );
        assert!(!looks_like_deletion_receipt(&other.content));
        assert!(verify_deletion_receipt(&other, &relay_hex).is_err());

        // Malformed ids and references.
        for bad in [
            deletion_receipt_content("not-hex", &session_ref, &"cd".repeat(32), channel_id),
            deletion_receipt_content(&"AB".repeat(32), &session_ref, &"cd".repeat(32), channel_id),
            deletion_receipt_content(&"ab".repeat(32), "not-a-uuid", &"cd".repeat(32), channel_id),
            deletion_receipt_content(&"ab".repeat(32), &session_ref, "short", channel_id),
        ] {
            let event = deletion_receipt_event(&relay, channel_id, bad);
            assert!(verify_deletion_receipt(&event, &relay_hex).is_err());
        }

        // Published in one channel, naming another.
        let elsewhere = deletion_receipt_event(
            &relay,
            Uuid::new_v4(),
            deletion_receipt_content(&"ab".repeat(32), &session_ref, &"cd".repeat(32), channel_id),
        );
        assert!(verify_deletion_receipt(&elsewhere, &relay_hex)
            .expect_err("must reject")
            .contains("not scoped to the channel"));
    }

    /// The one string that binds this verifier to the relay that writes it.
    /// The relay's own constant is `CODING_SESSION_DELETION_RECEIPT_TYPE`;
    /// change one and this test names the other.
    #[test]
    fn the_deletion_receipt_type_is_the_relays_own_word() {
        assert_eq!(DELETION_RECEIPT_TYPE, "coding_session_deletion_accepted");
    }
}
