//! Read-time observations shared by continuity transports.
use super::*;
use nostr::{Event, PublicKey, SecretKey};
use serde::Serialize;
use std::collections::BTreeSet;

/// Bounded reason codes; never include raw relay errors or decrypted text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UnknownReason {
    /// Transport did not establish a complete response.
    QueryFailed,
    /// Query reached its cap or transport reported partial coverage.
    Incomplete,
    /// At least one candidate could not be authenticated or decrypted.
    InvalidCandidate,
    /// Selected companion value is malformed, oversized or wrongly scoped.
    InvalidHead,
    /// No current claim with the requested identifier.
    NotFound,
    /// Visible divergent writes acknowledge the same base.
    Conflict,
    /// An assertion has no source reference.
    NoEvidence,
    /// Current authorized source read failed or returned no exact event.
    EvidenceUnavailable,
    /// Source signature/ID is invalid.
    EvidenceInvalid,
    /// Source ID, kind or channel differs from the reference.
    EvidenceScope,
    /// Source content digest differs.
    EvidenceHash,
    /// Retained excerpt is not an exact substring.
    EvidenceExcerpt,
}

/// Transport's coverage observation, not a guarantee of global convergence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryCoverage {
    /// Response completed below its limit on the single configured relay.
    Complete,
    /// Failed query; even an empty result is unknown.
    Failed,
    /// Truncated or otherwise partial response.
    Incomplete,
}

/// Validated head with exact stored bytes retained for readback/hash comparison.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Head {
    /// Selected NIP-AE event ID.
    pub event_id: String,
    /// Authenticated actor public key.
    pub author: String,
    /// Envelope timestamp; not claim observation time.
    pub created_at: u64,
    /// Exact stored value; keep this private.
    pub value: String,
    /// SHA-256 of exact stored value bytes.
    pub value_hash: String,
    /// Validated companion record.
    pub record: Record,
    /// Claims differing in visible same-base forks, including missing IDs.
    pub conflicts: BTreeSet<String>,
}

/// Outcome of an exact address read. No older fallback is possible on unknown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum HeadRead {
    /// Complete empty observation, not knowledge of global absence.
    AbsentAtRead,
    /// Latest signed envelope is a tombstone.
    WithdrawnRecord {
        /// Tombstone event ID (usable for explicit raw-memory reset only).
        event_id: String,
    },
    /// A current validated head, possibly containing visible claim conflicts.
    Present {
        /// Private record and observed metadata.
        head: Box<Head>,
    },
    /// Certainty could not be established.
    Unknown {
        /// Bounded reason.
        reason: UnknownReason,
    },
}

/// Cryptographic read identity, from the caller's verified custody context.
pub struct ReadIdentity<'a> {
    /// Expected signing agent.
    pub actor: &'a PublicKey,
    /// Expected owner in p tag.
    pub owner: &'a PublicKey,
    /// Agent or owner's secret key; never serialized by this module.
    pub reader_secret: &'a SecretKey,
    /// Other party's public key for NIP-44 decryption.
    pub peer: &'a PublicKey,
}

/// Authenticate every candidate before selecting the NIP-AE LWW head.
/// Callers query kind 30174 + actor + owner + exact derived d with limit 16.
/// Any invalid candidate conservatively yields unknown. Companion-invalid
/// newest heads shadow older valid values. Query/read time stays with transport.
pub fn select_head(
    scope: &Scope,
    identity: &ReadIdentity<'_>,
    events: &[Event],
    coverage: QueryCoverage,
) -> HeadRead {
    let unknown = |reason| HeadRead::Unknown { reason };
    if coverage == QueryCoverage::Failed {
        return unknown(UnknownReason::QueryFailed);
    }
    if coverage == QueryCoverage::Incomplete || events.len() >= HEAD_QUERY_LIMIT {
        return unknown(UnknownReason::Incomplete);
    }
    let Ok(slug) = scope.slug() else {
        return unknown(UnknownReason::InvalidHead);
    };
    let mut candidates = Vec::new();
    for event in events {
        if event.verify().is_err() {
            return unknown(UnknownReason::InvalidCandidate);
        }
        let Ok(body) = crate::engram::validate_and_decrypt(
            event,
            identity.actor,
            identity.owner,
            identity.reader_secret,
            identity.peer,
        ) else {
            return unknown(UnknownReason::InvalidCandidate);
        };
        if body.slug() != slug {
            return unknown(UnknownReason::InvalidCandidate);
        }
        // Engram parsing intentionally tolerates envelope extensions. Measure
        // original plaintext too, so whitespace/unknown fields cannot evade cap.
        let Ok(plaintext) =
            nostr::nips::nip44::decrypt(identity.reader_secret, identity.peer, &event.content)
        else {
            return unknown(UnknownReason::InvalidCandidate);
        };
        let decoded = if plaintext.len() > MAX_BODY_BYTES {
            Err(Error::TooLarge)
        } else {
            match body {
                Body::Memory {
                    value: Some(value), ..
                } => Record::decode(&value, scope).map(|record| Some((value, record))),
                Body::Memory { value: None, .. } => Ok(None),
                _ => Err(Error::Schema),
            }
        };
        candidates.push((event, decoded));
    }
    let Some(winner) = crate::engram::select_head(events.iter().cloned()) else {
        return HeadRead::AbsentAtRead;
    };
    let Some((_, selected)) = candidates.iter().find(|(event, _)| event.id == winner.id) else {
        return unknown(UnknownReason::InvalidCandidate);
    };
    let (value, record) = match selected {
        Ok(Some(pair)) => pair,
        Ok(None) => {
            return HeadRead::WithdrawnRecord {
                event_id: winner.id.to_hex(),
            }
        }
        Err(_) => return unknown(UnknownReason::InvalidHead),
    };
    // Invalid older companion records can hide a visible same-base fork. Do
    // not silently ignore that uncertainty even when the LWW winner is valid.
    if candidates.iter().any(|(_, decoded)| decoded.is_err()) {
        return unknown(UnknownReason::InvalidHead);
    }
    // Check transitions when the exact predecessor happens to remain visible.
    // Its absence is normal for replaceable events and does not invent history.
    if let Some(base) = &record.base_event_id {
        if let Some((_, Ok(Some((_, previous))))) = candidates
            .iter()
            .find(|(event, _)| event.id.to_hex() == *base)
        {
            if validate_transition(record, Some((base, previous))).is_err() {
                return unknown(UnknownReason::InvalidHead);
            }
        }
    }
    let visible: Vec<_> = candidates
        .iter()
        .filter_map(|(event, decoded)| {
            decoded
                .as_ref()
                .ok()
                .and_then(|entry| entry.as_ref())
                .map(|(_, record)| (event.id.to_hex(), record))
        })
        .collect();
    let mut conflicts = BTreeSet::new();
    for (index, (left_id, left)) in visible.iter().enumerate() {
        for (right_id, right) in visible.iter().skip(index + 1) {
            if left_id == right_id || left.base_event_id != right.base_event_id {
                continue;
            }
            for claim in left.claims.iter().chain(right.claims.iter()) {
                let a = left.claims.iter().find(|c| c.id == claim.id);
                let b = right.claims.iter().find(|c| c.id == claim.id);
                if a != b && !explicitly_resolves(record, &claim.id, left_id, right_id, &visible) {
                    conflicts.insert(claim.id.clone());
                }
            }
        }
    }
    HeadRead::Present {
        head: Box::new(Head {
            event_id: winner.id.to_hex(),
            author: winner.pubkey.to_hex(),
            created_at: winner.created_at.as_secs(),
            value: value.clone(),
            value_hash: value_hash(value.as_bytes()),
            record: record.clone(),
            conflicts,
        }),
    }
}

// A later unrelated addition must not silently clear a visible fork. Follow
// retained correction references only while visible, bounded by the read cap.
// A missing intermediate explanation leaves conflict unknown conservatively.
fn explicitly_resolves(
    record: &Record,
    id: &str,
    left: &str,
    right: &str,
    visible: &[(String, &Record)],
) -> bool {
    let mut current = record;
    for _ in 0..HEAD_QUERY_LIMIT {
        let Some(correction) = current
            .claims
            .iter()
            .find(|c| c.id == id)
            .and_then(|c| c.correction.as_ref())
        else {
            return false;
        };
        let Some((_, previous)) = visible
            .iter()
            .find(|(event_id, _)| event_id == &correction.prior_head)
        else {
            return false;
        };
        let Some(prior) = previous.claims.iter().find(|c| c.id == id) else {
            return false;
        };
        if correction.prior_text != prior.text {
            return false;
        }
        if correction.prior_head == left || correction.prior_head == right {
            return true;
        }
        current = previous;
    }
    false
}

/// Verify one freshly fetched source; signatures attest authorship, not truth.
/// The caller must establish current access in this community, never feed a
/// cached event as evidence of present readability.
pub fn verify_evidence(reference: &Evidence, event: &Event) -> Result<(), UnknownReason> {
    reference
        .validate()
        .map_err(|_| UnknownReason::EvidenceInvalid)?;
    event.verify().map_err(|_| UnknownReason::EvidenceInvalid)?;
    let channels: Vec<_> = event
        .tags
        .iter()
        .filter(|tag| tag.kind().to_string() == "h")
        .collect();
    if event.id.to_hex() != reference.event_id
        || event.kind.as_u16() != reference.kind
        || channels.len() != 1
        || channels[0].content() != Some(reference.channel.as_str())
    {
        return Err(UnknownReason::EvidenceScope);
    }
    if value_hash(event.content.as_bytes()) != reference.sha256 {
        return Err(UnknownReason::EvidenceHash);
    }
    if !event.content.contains(&reference.excerpt) {
        return Err(UnknownReason::EvidenceExcerpt);
    }
    Ok(())
}

/// Read-time disposition for one claim; all data is private to actor/owner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ClaimOutcome {
    /// Source signatures and exact content resolved under current access.
    /// This attests neither semantic entailment nor independent truth.
    AssertionWithReadableEvidence {
        /// Agent that authored the claim.
        author: String,
        /// Selected memory head.
        head_event_id: String,
        /// Text, observation time, source IDs and retained correction.
        claim: Claim,
    },
    /// Explicit retirement; never display its text as current.
    Withdrawn {
        /// Retired identifier.
        id: String,
    },
    /// Not eligible as a current evidence-backed assertion.
    Unknown {
        /// Requested/selected identifier.
        id: String,
        /// One or more bounded codes.
        reasons: Vec<UnknownReason>,
        /// Exact source IDs that did not resolve, at most four.
        unresolved_ids: Vec<String>,
    },
}

/// Resolve requested IDs (empty means all, including visible fork-only IDs).
/// The callback must freshly query each exact ID+kind+h on the current relay
/// as the actor. It maps missing/revoked/deleted/failed reads to a bounded code.
/// No evidence lookup occurs for withdrawn/conflicting claims.
pub fn evaluate_claims<F>(
    head: &Head,
    requested: &[String],
    mut fetch: F,
) -> Result<Vec<ClaimOutcome>, Error>
where
    F: FnMut(&Evidence) -> Result<Event, UnknownReason>,
{
    head.record.validate(&head.record.scope)?;
    if requested.len() > MAX_CLAIMS {
        return Err(Error::TooLarge);
    }
    let ids: BTreeSet<String> = if requested.is_empty() {
        head.record
            .claims
            .iter()
            .map(|c| c.id.clone())
            .chain(head.conflicts.iter().cloned())
            .collect()
    } else {
        for id in requested {
            validate_claim_id(id)?;
        }
        requested.iter().cloned().collect()
    };
    let mut outcomes = Vec::new();
    for id in ids {
        let unknown = |reason| ClaimOutcome::Unknown {
            id: id.clone(),
            reasons: vec![reason],
            unresolved_ids: vec![],
        };
        if head.conflicts.contains(&id) {
            outcomes.push(unknown(UnknownReason::Conflict));
            continue;
        }
        let Some(claim) = head.record.claims.iter().find(|c| c.id == id) else {
            outcomes.push(unknown(UnknownReason::NotFound));
            continue;
        };
        if claim.state == ClaimState::Withdrawn {
            outcomes.push(ClaimOutcome::Withdrawn { id });
            continue;
        }
        if claim.evidence.is_empty() {
            outcomes.push(unknown(UnknownReason::NoEvidence));
            continue;
        }
        let mut reasons = Vec::new();
        let mut unresolved_ids = Vec::new();
        for evidence in &claim.evidence {
            if let Err(reason) = fetch(evidence).and_then(|event| verify_evidence(evidence, &event))
            {
                if !reasons.contains(&reason) {
                    reasons.push(reason);
                }
                if !unresolved_ids.contains(&evidence.event_id) {
                    unresolved_ids.push(evidence.event_id.clone());
                }
            }
        }
        outcomes.push(if reasons.is_empty() {
            ClaimOutcome::AssertionWithReadableEvidence {
                author: head.author.clone(),
                head_event_id: head.event_id.clone(),
                claim: claim.clone(),
            }
        } else {
            ClaimOutcome::Unknown {
                id,
                reasons,
                unresolved_ids,
            }
        });
    }
    Ok(outcomes)
}
