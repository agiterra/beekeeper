//! Receipt-backed projection of a session's accepted kind-44228 authority
//! chain.
//!
//! A child of `operations`, split out only to keep that file under 1,000 lines
//! (batch 2 lane B2). No behaviour change: these are the same items, and
//! `use super::*` gives them the parent's imports and private helpers exactly
//! as before.

use std::collections::BTreeMap;

use beekeeper_core::coding_session_authority_claim::{fold_current_claim, ClaimLink, ClaimState};
use beekeeper_core::coding_session_authority_transition::{
    decode_coding_session_authority_transition, CodingSessionAuthorityTransitionPayload,
    CodingSessionAuthorityTransitionType, CODING_SESSION_AUTHORITY_TRANSITION_TAG_VERSION,
};
use beekeeper_core::coding_session_policy::CodingSessionPolicyGrant;
use beekeeper_core::coding_session_team_transaction::{
    CodingSessionTeamActiveGrant, CodingSessionTeamActiveSeat,
};
use beekeeper_core::kind::{KIND_CODING_SESSION_AUTHORITY_TRANSITION, KIND_SYSTEM_MESSAGE};
use nostr::Event;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::client::BeekeeperClient;
use crate::error::CliError;

pub(super) const AUTHORITY_ACCEPTANCE_RECEIPT_TYPE: &str =
    "coding_session_authority_transition_accepted";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct AuthorityAcceptanceReceipt {
    #[serde(rename = "type")]
    receipt_type: String,
    genesis_ref: String,
    accepted_event_id: String,
    seq: u32,
    transition_type: CodingSessionAuthorityTransitionType,
    grantee_pubkey: String,
    #[serde(default)]
    role: Option<String>,
    /// The execution body a claim receipt names.
    ///
    /// **Absent here is what broke every chain read after the first
    /// takeover.** The relay stamps `bodyPubkey` onto takeover/transfer
    /// receipts (`beekeeper-relay/src/handlers/side_effects.rs`), and this struct
    /// is `deny_unknown_fields`, so the first accepted claim made every
    /// subsequent projection fail with "unknown field `bodyPubkey`" — which
    /// is to say all four handover verbs stopped working the moment one of
    /// them succeeded (composition run 5).
    #[serde(default)]
    body_pubkey: Option<String>,
    /// The project a `grant-project-actions`/`revoke-project-actions` receipt
    /// is scoped to.
    ///
    /// **Absent here broke every team session a project owner founded with
    /// "Use roles" on.** Lane 186 taught the relay to echo `projectRef`
    /// (`beekeeper-relay/src/handlers/side_effects.rs`) without teaching this
    /// `deny_unknown_fields` struct to read it, so from the moment the
    /// desktop signed that delegation at launch, every read of the chain —
    /// `sessions hire`, `seat-repair`, `report`, `operation get`, `pulse
    /// digest` — failed with "unknown field `projectRef`" and no seat could
    /// publish a report (ledger 204, found live 2026-09-20).
    #[serde(default)]
    project_ref: Option<String>,
}

/// Strictly decode one kind:40099 authority acceptance receipt's content.
///
/// Split out of [`project_receipt_backed_authority_chain`] so the shared
/// `conformance/authority-chain` vectors can run against exactly the bytes
/// the live projection runs against, rather than against a second copy of
/// these rules.
pub(super) fn decode_authority_acceptance_receipt(
    value: Value,
) -> Result<AuthorityAcceptanceReceipt, String> {
    let role_key_present = value
        .as_object()
        .is_some_and(|object| object.contains_key("role"));
    // Keyed on the key's presence, not the parsed value, for the reason
    // the lifecycle receipt's `turnId` is: an explicit `"bodyPubkey": null`
    // is a receipt claiming the relay saw no body, which is a different
    // claim from a receipt that carries no such key.
    let body_key_present = value
        .as_object()
        .is_some_and(|object| object.contains_key("bodyPubkey"));
    let project_key_present = value
        .as_object()
        .is_some_and(|object| object.contains_key("projectRef"));
    let receipt: AuthorityAcceptanceReceipt = serde_json::from_value(value)
        .map_err(|error| format!("malformed authority acceptance receipt: {error}"))?;
    if receipt.receipt_type != AUTHORITY_ACCEPTANCE_RECEIPT_TYPE {
        return Err("authority receipt type mismatch".into());
    }
    let is_seat_receipt = matches!(
        receipt.transition_type,
        CodingSessionAuthorityTransitionType::GrantSeat
            | CodingSessionAuthorityTransitionType::RevokeSeat
    );
    if is_seat_receipt != role_key_present || is_seat_receipt != receipt.role.is_some() {
        return Err("authority receipt role presence does not match its transition type".into());
    }
    // A claim receipt names the body it fences to and every other receipt
    // names none: a grant receipt carrying one would be describing a fence
    // nobody raised, and a takeover receipt without one names a claim the
    // fold could not apply.
    let is_claim_receipt = receipt.transition_type.is_claim();
    if is_claim_receipt != body_key_present || is_claim_receipt != receipt.body_pubkey.is_some() {
        return Err(
            "authority receipt bodyPubkey presence does not match its transition type".into(),
        );
    }
    if let Some(body_pubkey) = &receipt.body_pubkey {
        if body_pubkey.len() != 64
            || !body_pubkey
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err("authority receipt bodyPubkey is not a lowercase 64-hex pubkey".into());
        }
    }
    // The same rule, exactly, for the delegation's scope: the two
    // project-action types carry `projectRef` and no other type may. A
    // delegation receipt that dropped its scope would say a delegation was
    // accepted without saying what it reaches, and a `revoke` carrying one
    // is a relay this build does not understand.
    let is_project_receipt = receipt.transition_type.is_project_actions();
    if is_project_receipt != project_key_present
        || is_project_receipt != receipt.project_ref.is_some()
    {
        return Err(
            "authority receipt projectRef presence does not match its transition type".into(),
        );
    }
    if let Some(project_ref) = &receipt.project_ref {
        beekeeper_core::coding_session_authority_transition::validate_project_ref(project_ref)
            .map_err(|error| format!("authority receipt {error}"))?;
    }
    Ok(receipt)
}

pub(super) async fn fetch_trusted_relay_self(client: &BeekeeperClient) -> Result<String, CliError> {
    let raw = client
        .get_public("/")
        .await
        .map_err(|error| CliError::Other(format!("failed to fetch relay info: {error}")))?;
    let value: Value = serde_json::from_str(&raw)
        .map_err(|error| CliError::Other(format!("relay info is not valid JSON: {error}")))?;
    let relay_self = value
        .get("self")
        .and_then(Value::as_str)
        .ok_or_else(|| CliError::Other("relay info is missing its trusted self pubkey".into()))?;
    if relay_self.len() != 64 || !relay_self.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(CliError::Other(
            "relay info self is not a valid 64-hex pubkey".into(),
        ));
    }
    Ok(relay_self.to_ascii_lowercase())
}

/// Read the relay-receipt-backed accepted authority chain for one genesis.
pub(super) async fn fetch_projected_authority(
    client: &BeekeeperClient,
    channel: &str,
    genesis: &str,
    founder: &str,
) -> Result<ProjectedAuthority, CliError> {
    let relay_self = fetch_trusted_relay_self(client).await?;
    let transitions = client
        .query_all(json!({
            "kinds": [KIND_CODING_SESSION_AUTHORITY_TRANSITION],
            "#h": [channel],
            "#csat-genesis": [genesis]
        }))
        .await?
        .into_iter()
        .map(serde_json::from_value)
        .collect::<Result<Vec<Event>, _>>()
        .map_err(|error| {
            CliError::Other(format!("relay returned malformed authority event: {error}"))
        })?;
    let receipts = client
        .query_all(json!({
            "kinds": [KIND_SYSTEM_MESSAGE],
            "#h": [channel],
            "authors": [relay_self]
        }))
        .await?
        .into_iter()
        .map(serde_json::from_value)
        .collect::<Result<Vec<Event>, _>>()
        .map_err(|error| {
            CliError::Other(format!(
                "relay returned malformed authority receipt: {error}"
            ))
        })?;
    project_receipt_backed_authority_chain(
        &transitions,
        &receipts,
        channel,
        genesis,
        founder,
        &relay_self,
    )
    .map_err(|error| CliError::Other(format!("invalid accepted authority chain: {error}")))
}

#[derive(Debug)]
pub(super) struct ProjectedAuthority {
    pub(super) grants: Vec<CodingSessionTeamActiveGrant>,
    pub(super) seats: Vec<CodingSessionTeamActiveSeat>,
    pub(super) seat_grant_refs: BTreeMap<String, String>,
    pub(super) head_event_id: Option<String>,
    pub(super) head_seq: u32,
    /// The grant/revoke history in accepted order, with the relay acceptance
    /// receipt's `created_at` on each.
    ///
    /// `grants` above is the projection's *current* answer, which cannot say
    /// whether a grant stood at some earlier moment. Kind 44245 needs exactly
    /// that: a policy is judged by whether its signer could steer **when it was
    /// published**, so a later revoke must not retroactively invalidate a
    /// record, and a later grant must not retroactively bless one
    /// (REVIEW-B2 F1).
    pub(super) policy_grants: Vec<CodingSessionPolicyGrant>,
    /// Who holds this whole session, folded from the same accepted chain by
    /// [`fold_current_claim`].
    ///
    /// Deliberately beside `grants` rather than derived from it: a grant says
    /// who may steer, a claim says who took the session over, and the two
    /// answer different questions at the fence. `Voided` is not `NoClaim`
    /// (`docs/HANDOVER_IMPL.md` §1).
    pub(crate) claim: ClaimState,
    /// `created_at` of the relay receipt that accepted the claim link in
    /// force, in unix seconds, or `None` when there is no claim.
    ///
    /// The receipt's time rather than the transition's: acceptance is what put
    /// the claim in force, and it is the only one of the two this projection
    /// verified against the trusted relay key.
    pub(crate) claim_since: Option<u64>,
    /// When each still-live grant was accepted, in unix seconds.
    ///
    /// `grants` says who may steer *now*; a handover record's standing is
    /// judged at the record's own `created_at`, so the reader needs the
    /// moment the grant landed as well as the fact of it
    /// (`HandoverFoldContext::grants`).
    pub(crate) grant_accepted_at: BTreeMap<String, u64>,
    /// When each still-live seat was accepted, in unix seconds, under the same
    /// rule as [`Self::grant_accepted_at`].
    pub(crate) seat_accepted_at: BTreeMap<String, u64>,
}

fn validate_accepted_authority_transition(
    event: &Event,
    channel: &str,
    genesis: &str,
) -> Result<(String, CodingSessionAuthorityTransitionPayload), String> {
    beekeeper_core::verify_event(event)
        .map_err(|error| format!("invalid accepted authority-transition signature: {error}"))?;
    if event.kind.as_u16() as u32 != KIND_CODING_SESSION_AUTHORITY_TRANSITION {
        return Err("accepted authority transition has the wrong kind".into());
    }
    let payload = decode_coding_session_authority_transition(&event.content)?;
    if payload.genesis_ref != genesis {
        return Err("accepted authority transition crosses the supplied genesis".into());
    }
    let tags: Vec<&[String]> = event.tags.iter().map(|tag| tag.as_slice()).collect();
    if tags.len() != 3
        || tags.iter().any(|tag| tag.len() != 2)
        || tags[0] != ["h", channel]
        || tags[1] != ["csat-v", CODING_SESSION_AUTHORITY_TRANSITION_TAG_VERSION]
        || tags[2] != ["csat-genesis", genesis]
    {
        return Err("accepted authority transition has an invalid envelope".into());
    }
    Ok((event.pubkey.to_hex(), payload))
}

pub(super) fn project_receipt_backed_authority_chain(
    transition_events: &[Event],
    receipt_events: &[Event],
    channel: &str,
    genesis: &str,
    founder: &str,
    relay_self: &str,
) -> Result<ProjectedAuthority, String> {
    let mut transitions = BTreeMap::new();
    for event in transition_events {
        let event_id = event.id.to_hex();
        transitions.entry(event_id).or_insert(event);
    }

    let mut receipt_ids = std::collections::BTreeSet::new();
    let mut accepted_by_seq = BTreeMap::new();
    let mut accepted_ids = std::collections::BTreeSet::new();
    for event in receipt_events {
        let value: Value = match serde_json::from_str(&event.content) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if value.get("type").and_then(Value::as_str) != Some(AUTHORITY_ACCEPTANCE_RECEIPT_TYPE) {
            continue;
        }
        if value.get("genesisRef").and_then(Value::as_str) != Some(genesis) {
            continue;
        }
        if !receipt_ids.insert(event.id.to_hex()) {
            continue;
        }
        if u32::from(event.kind.as_u16()) != KIND_SYSTEM_MESSAGE {
            return Err("authority acceptance receipt has the wrong kind".into());
        }
        beekeeper_core::verify_event(event)
            .map_err(|error| format!("invalid authority-receipt signature: {error}"))?;
        if event.pubkey.to_hex() != relay_self {
            return Err("authority acceptance receipt is not signed by the trusted relay".into());
        }
        if !event
            .tags
            .iter()
            .any(|tag| tag.as_slice() == ["h", channel])
        {
            return Err("authority acceptance receipt crosses the requested channel".into());
        }
        let receipt = decode_authority_acceptance_receipt(value)?;
        let transition = transitions
            .get(&receipt.accepted_event_id)
            .ok_or_else(|| "authority receipt references a missing transition".to_owned())?;
        let (signer, payload) =
            validate_accepted_authority_transition(transition, channel, genesis)?;
        if payload.genesis_ref != receipt.genesis_ref
            || payload.seq != receipt.seq
            || payload.transition_type != receipt.transition_type
            || payload.grantee_pubkey != receipt.grantee_pubkey
            || payload.role != receipt.role
            // The receipt is the relay's statement about the transition, so a
            // body they disagree about means one of the two is not describing
            // this link — and the fence would be raised on whichever the
            // reader happened to trust.
            || payload.body_pubkey != receipt.body_pubkey
            // And the scope, for the same reason: a delegation is only as
            // narrow as the project it names, so a receipt and a link that
            // disagree about `projectRef` are not describing one delegation.
            || payload.project_ref != receipt.project_ref
        {
            return Err("authority receipt facts do not match the accepted transition".into());
        }
        if !accepted_ids.insert(receipt.accepted_event_id.clone()) {
            return Err("duplicate authority receipts name the same accepted transition".into());
        }
        let accepted = (
            receipt.accepted_event_id.clone(),
            signer,
            payload,
            event.created_at.as_secs(),
        );
        if accepted_by_seq.insert(receipt.seq, accepted).is_some() {
            return Err("conflicting authority receipts claim the same sequence".into());
        }
    }

    let links: Vec<_> = accepted_by_seq.into_values().collect();
    let mut expected_prev: Option<&str> = None;
    let mut grants = BTreeMap::new();
    let mut seats = BTreeMap::new();
    let mut seat_grant_refs = BTreeMap::new();
    let mut policy_grants = Vec::with_capacity(links.len());
    // Every accepted link, in seq order, reduced to what the canonical claim
    // fold reads. Kept as links rather than as a running state so the answer
    // comes from `fold_current_claim` and not from a second copy of its rules.
    let mut claim_links: Vec<ClaimLink> = Vec::new();
    let mut claim_accepted_at: BTreeMap<String, u64> = BTreeMap::new();
    let mut grant_accepted_at: BTreeMap<String, u64> = BTreeMap::new();
    let mut seat_accepted_at: BTreeMap<String, u64> = BTreeMap::new();
    for (offset, (event_id, signer, payload, accepted_at)) in links.iter().enumerate() {
        let expected_seq = u32::try_from(offset + 1)
            .map_err(|_| "authority chain exceeds u32 sequence space".to_owned())?;
        if payload.seq != expected_seq || payload.prev_accepted.as_deref() != expected_prev {
            return Err(format!(
                "authority transition {} does not extend the canonical chain",
                event_id
            ));
        }
        let signer_is_founder = signer == founder;
        let signer_is_operator = grants
            .get(signer)
            .is_some_and(|grant: &CodingSessionTeamActiveGrant| grant.may_steer);
        let signer_is_lead = seats
            .get(signer)
            .is_some_and(|seat: &CodingSessionTeamActiveSeat| seat.role == "lead");
        // The claim as it stands *before* this link, which is what a
        // `transfer`'s signer is judged against.
        let claim_before = fold_current_claim(claim_links.iter().cloned());
        let signer_is_authorized = match payload.transition_type {
            // A seat may be granted by the founder, a live operator, or an
            // active lead.
            CodingSessionAuthorityTransitionType::GrantSeat
            | CodingSessionAuthorityTransitionType::RevokeSeat => {
                signer_is_founder || signer_is_operator || signer_is_lead
            }
            // §1: a takeover is a self-claim by the founder or a live
            // operator. `signer_is_operator` is read from `grants` as it
            // stands *before* this link, which is the "grants_before" the
            // relay's own acceptance rule names.
            CodingSessionAuthorityTransitionType::Takeover => {
                signer_is_founder || signer_is_operator
            }
            // §1: a transfer is signed by the current claimant or the founder,
            // and there must be a claim in force to transfer.
            //
            // Both halves, spelled the same way the relay spells them
            // (`beekeeper-db/src/event.rs`, `NoActiveClaim` before
            // `SignerNotAuthorized`): the way back from a voided claim is a
            // fresh takeover, never a transfer, so a founder-signed transfer
            // from `NoClaim` or `Voided` is refused here exactly as the relay
            // refuses it. Unreachable against a relay that already enforces
            // it — which is the point: a projection that accepted a link the
            // relay would not is a second, disagreeing answer to one question.
            CodingSessionAuthorityTransitionType::Transfer => claim_before
                .active()
                .is_some_and(|claim| signer_is_founder || claim.claimant == *signer),
            _ => signer_is_founder,
        };
        if !signer_is_authorized {
            return Err(format!(
                "authority transition {event_id} has an unauthorized signer"
            ));
        }
        if payload.transition_type == CodingSessionAuthorityTransitionType::Takeover
            && payload.grantee_pubkey != *signer
        {
            // The inverse of `SelfNomination`: a takeover names its own signer
            // as claimant, so a link that claims the session for somebody else
            // is a transfer that skipped the transfer rules.
            return Err(format!(
                "authority transition {event_id} is a takeover whose claimant is not its signer"
            ));
        }
        if payload.transition_type.is_claim() {
            claim_accepted_at.insert(event_id.clone(), *accepted_at);
        }
        claim_links.push(ClaimLink {
            seq: payload.seq,
            accepted_event_id: event_id.clone(),
            transition_type: payload.transition_type,
            grantee_pubkey: payload.grantee_pubkey.clone(),
            body_pubkey: payload.body_pubkey.clone(),
        });
        if payload.transition_type == CodingSessionAuthorityTransitionType::GrantSeat
            && payload.grantee_pubkey == *signer
        {
            return Err("seat grant cannot nominate its own signer".into());
        }
        if signer_is_lead
            && !signer_is_founder
            && !signer_is_operator
            && payload.role.as_deref() == Some("lead")
        {
            return Err("active lead cannot grant or revoke lead authority".into());
        }
        policy_grants.push(CodingSessionPolicyGrant {
            grantee: payload.grantee_pubkey.clone(),
            accepted_at: *accepted_at,
            transition_type: payload.transition_type,
        });
        match payload.transition_type {
            CodingSessionAuthorityTransitionType::GrantOperator => {
                grants.insert(
                    payload.grantee_pubkey.clone(),
                    CodingSessionTeamActiveGrant {
                        actor_pubkey: payload.grantee_pubkey.clone(),
                        grant_event_ref: event_id.clone(),
                        may_steer: true,
                    },
                );
                grant_accepted_at.insert(payload.grantee_pubkey.clone(), *accepted_at);
            }
            CodingSessionAuthorityTransitionType::GrantViewer => {
                grants.insert(
                    payload.grantee_pubkey.clone(),
                    CodingSessionTeamActiveGrant {
                        actor_pubkey: payload.grantee_pubkey.clone(),
                        grant_event_ref: event_id.clone(),
                        may_steer: false,
                    },
                );
                grant_accepted_at.remove(&payload.grantee_pubkey);
            }
            CodingSessionAuthorityTransitionType::Revoke => {
                if grants.remove(&payload.grantee_pubkey).is_none() {
                    return Err("revoke names a pubkey with no active grant".into());
                }
                grant_accepted_at.remove(&payload.grantee_pubkey);
            }
            CodingSessionAuthorityTransitionType::GrantSeat => {
                let role = payload
                    .role
                    .clone()
                    .ok_or_else(|| "grant-seat requires role".to_owned())?;
                seats.insert(
                    payload.grantee_pubkey.clone(),
                    CodingSessionTeamActiveSeat {
                        actor_pubkey: payload.grantee_pubkey.clone(),
                        role,
                    },
                );
                seat_grant_refs.insert(payload.grantee_pubkey.clone(), event_id.clone());
                seat_accepted_at.insert(payload.grantee_pubkey.clone(), *accepted_at);
            }
            CodingSessionAuthorityTransitionType::RevokeSeat => {
                let expected_role = payload
                    .role
                    .as_deref()
                    .ok_or_else(|| "revoke-seat requires role".to_owned())?;
                match seats.get(&payload.grantee_pubkey) {
                    Some(seat) if seat.role == expected_role => {
                        seats.remove(&payload.grantee_pubkey);
                        seat_grant_refs.remove(&payload.grantee_pubkey);
                        seat_accepted_at.remove(&payload.grantee_pubkey);
                    }
                    Some(_) => return Err("revoke-seat role does not match active seat".into()),
                    None => return Err("revoke-seat names no active seat".into()),
                }
            }
            // A claim moves who holds the whole session; it grants and revokes
            // nothing, so the grant and seat maps are untouched. The claim
            // itself is folded from `claim_links` below.
            // A project-actions delegation grants standing over one project's
            // kind:30620/46020 events and no session grant or seat (ledger
            // 186), so these maps are untouched; it is folded by
            // `beekeeper_core::coding_session_project_action_grant`.
            CodingSessionAuthorityTransitionType::Takeover
            | CodingSessionAuthorityTransitionType::Transfer
            | CodingSessionAuthorityTransitionType::GrantProjectActions
            | CodingSessionAuthorityTransitionType::RevokeProjectActions => {}
        }
        expected_prev = Some(event_id);
    }
    let claim = fold_current_claim(claim_links.into_iter());
    let claim_since = claim
        .active()
        .and_then(|current| claim_accepted_at.get(&current.accepted_event_id).copied());
    Ok(ProjectedAuthority {
        grants: grants.into_values().collect(),
        seats: seats.into_values().collect(),
        seat_grant_refs,
        head_event_id: expected_prev.map(str::to_owned),
        head_seq: u32::try_from(links.len())
            .map_err(|_| "authority chain exceeds u32 sequence space".to_owned())?,
        policy_grants,
        claim,
        claim_since,
        grant_accepted_at,
        seat_accepted_at,
    })
}
