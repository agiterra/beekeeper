//! The canonical projection of one umbrella's handover records (kind 44247).
//!
//! Every consumer — the CLI's `handover status`, the provider's fence
//! disclosure, the Desktop panel and its TypeScript twin — reads this one
//! fold, so "who holds this session, what is the checkpoint worth
//! reconstructing from, and who continued it" has exactly one answer.
//!
//! # Why an envelope failure fails the whole set
//!
//! Unlike kind 44246, whose fold lists a bad event and carries on, a defect
//! here is a **whole-set `Err`** — the same choice kind 44244's governance fold
//! makes, for the same reason. These records decide what somebody else
//! reconstructs work from and who is fenced out of a session; a fold that
//! quietly dropped one and answered anyway could show "no checkpoint" over a
//! checkpoint that exists, and a person would then reconstruct from an older
//! revision believing it was the newest. An observation can be dropped because
//! it settles nothing; a handover record cannot.
//!
//! # Standing is decided here, never at the relay
//!
//! The relay validates one event's structure and stops
//! (`docs/HANDOVER_IMPL.md` §2). Whether the author held standing is a question
//! about the accepted NIP-CSAT chain *at the record's own time*, which is why
//! [`HandoverFoldContext`] carries acceptance times: a checkpoint written
//! before its author was granted anything is `unauthorized`, and it stays
//! listed — the author did write it — but it is never used for reconstruction.
//!
//! # Recency is stated, not measured
//!
//! Within one author, "which checkpoint is the newest" is answered by that
//! author's own `prevCheckpointRef` chain and never by the clock: three
//! checkpoints published inside one second sort by `(created_at, id)`, and the
//! id is a hash, so the tie-break once handed a reconstruction a stale record
//! whose `missing` list was empty (`handover-composition-5.log`, finding 3).
//! A superseded checkpoint stays listed, carries the id that replaced it, and
//! is never [`HandoverFold::latest_authorized_checkpoint`].
//!
//! # Retirement beats everything
//!
//! A retired umbrella (an accepted whole-session deletion, §3.2) still lists
//! every record it holds, because the history is what somebody is reading, and
//! answers `None` for the latest authorized checkpoint, `None` for the active
//! continuation and [`ClaimState::NoClaim`] for the claim. Nothing is
//! reconstructed to make a deleted session resumable.

use std::collections::HashSet;

use nostr::Event;
use serde::Serialize;

use crate::coding_session_authority_claim::ClaimState;
use crate::coding_session_handover::{
    validate_coding_session_handover_envelope, CodingSessionHandoverBody,
    CodingSessionHandoverCheckpoint, CodingSessionHandoverContinuation, CodingSessionHandoverMode,
};

/// What the caller resolved about this umbrella before folding its handovers.
///
/// The authority half is supplied rather than derived: the accepted 44228
/// chain is verified differently by each caller (relay receipts for the
/// provider, a storage read for the relay, a decoded page for a client), and a
/// fold that re-derived it would be a second, disagreeing answer to a question
/// already settled.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HandoverFoldContext {
    /// Canonical lowercase UUID of the channel these records must be scoped to.
    pub channel_ref: String,
    /// Canonical lowercase UUID of the umbrella session.
    pub session_ref: String,
    /// Lowercase 64-hex event id of the session genesis.
    pub genesis_ref: String,
    /// The founder: the genesis signer, who always has standing to checkpoint.
    pub founder_pubkey: String,
    /// Live operator grants as `(pubkey, accepted_at)` — when each grant was
    /// accepted, in unix seconds.
    ///
    /// The time matters: standing is judged **at the record's own
    /// `created_at`**, so a grant accepted after a checkpoint was written does
    /// not retroactively authorize it.
    pub grants: Vec<(String, i64)>,
    /// Live seats as `(pubkey, role, accepted_at)`, under the same rule.
    pub seats: Vec<(String, String, i64)>,
    /// The claim state folded from the accepted chain
    /// ([`crate::coding_session_authority_claim::fold_current_claim`]).
    pub claim: ClaimState,
    /// When the accepted claim link was published, in unix seconds, when the
    /// caller resolved it.
    ///
    /// `None` is honest rather than convenient: a caller that folded the chain
    /// from receipts alone knows the claim without knowing its timestamp, and
    /// a surface that wants "took over 20 minutes ago" must be able to tell
    /// "unknown" from "just now".
    pub claim_since: Option<i64>,
    /// Whether an accepted whole-session deletion has been witnessed.
    pub retired: bool,
}

/// Whether a record's author held the standing its type requires.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum HandoverStanding {
    /// The author held standing: founder, a live operator or an active seat at
    /// the record's own time (checkpoint), or the claimant of the claim in
    /// force (continuation).
    Authorized,
    /// The author held no such standing. The record is still listed — it was
    /// written and signed — and is never used for reconstruction.
    Unauthorized,
    /// Replaced by a later statement, and therefore historical rather than
    /// wrong.
    ///
    /// Two records reach this: a continuation that acted on a claim which is no
    /// longer the one in force ("continued by B until …"), and a checkpoint its
    /// **own author** later named in a `prevCheckpointRef`. A superseded
    /// checkpoint was authorized — only an authorized one can be superseded —
    /// so this token means "was good, is old", never "was not allowed".
    Superseded,
}

/// One folded checkpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HandoverCheckpointEntry {
    /// Event id of the 44247 record.
    pub event_id: String,
    /// Lowercase 64-hex pubkey that signed it.
    pub author: String,
    /// The event's own `created_at`, in unix seconds.
    pub created_at: i64,
    /// Whether the author held standing at `created_at`, and whether a later
    /// checkpoint of theirs has replaced this one.
    pub standing: HandoverStanding,
    /// The event id of the **authorized** checkpoint that named this one in
    /// its `prevCheckpointRef`, when one did.
    ///
    /// Set together with [`HandoverStanding::Superseded`], and it is the link
    /// a surface follows to say "replaced by …". `None` on everything else,
    /// including a checkpoint whose author simply never wrote another.
    pub superseded_by: Option<String>,
    /// The checkpoint itself, exactly as signed.
    pub body: CodingSessionHandoverCheckpoint,
}

/// One folded continuation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HandoverContinuationEntry {
    /// Event id of the 44247 record.
    pub event_id: String,
    /// Lowercase 64-hex pubkey that signed it.
    pub author: String,
    /// The event's own `created_at`, in unix seconds.
    pub created_at: i64,
    /// The accepted claim link this continuation acted on.
    pub claim_ref: String,
    /// Native resume, or reconstruction. Never conflated.
    pub mode: CodingSessionHandoverMode,
    /// Whether this is the continuation of the claim in force.
    pub standing: HandoverStanding,
    /// The continuation itself, exactly as signed.
    pub body: CodingSessionHandoverContinuation,
}

/// One record this fold will not act on, and why — in a sentence a person can
/// read without knowing the rules.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HandoverExclusion {
    /// Event id of the excluded record.
    pub event_id: String,
    /// Why it is not used.
    pub reason: String,
}

/// Everything a surface needs to answer "who holds this session".
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HandoverFold {
    /// Every checkpoint, ascending by `(created_at, id)`.
    pub checkpoints: Vec<HandoverCheckpointEntry>,
    /// The newest authorized checkpoint's event id — what a reconstruction
    /// starts from. `None` on a retired umbrella, or when no author held
    /// standing.
    pub latest_authorized_checkpoint: Option<String>,
    /// Every continuation, ascending by `(created_at, id)`.
    pub continuations: Vec<HandoverContinuationEntry>,
    /// The claim state this fold was given, cleared to
    /// [`ClaimState::NoClaim`] on a retired umbrella.
    pub claim: ClaimState,
    /// When the claim link was published, when the caller knew it.
    pub claim_since: Option<i64>,
    /// The newest authorized continuation of the claim in force.
    pub active_continuation: Option<String>,
    /// Whether the umbrella has been retired.
    pub retired: bool,
    /// Records this fold lists but will not act on, each with its reason.
    pub excluded: Vec<HandoverExclusion>,
}

/// Fold verified kind 44247 events for one umbrella.
///
/// `events` may arrive in any order: the fold sorts by `(created_at, id)`
/// ascending, so two callers holding the same set — a relay page read
/// newest-first and a subscription appended in arrival order — produce byte-
/// identical output. Ties on `created_at` break on the event id, which is a
/// hash and therefore stable, rather than on arrival, which is not.
///
/// # Errors
///
/// The whole set fails, naming the offending event, when: an event's signature
/// or envelope is invalid; it names a different session, genesis or channel
/// than the context; or two events share an id. See the module header for why
/// this is an error rather than a listed line.
pub fn fold_coding_session_handover(
    events: &[Event],
    context: &HandoverFoldContext,
) -> Result<HandoverFold, String> {
    let mut ordered: Vec<(&Event, String)> = Vec::with_capacity(events.len());
    let mut seen: HashSet<String> = HashSet::with_capacity(events.len());
    for event in events {
        let event_id = event.id.to_hex();
        if !seen.insert(event_id.clone()) {
            return Err(format!(
                "duplicate coding-session handover event {event_id}: the same record was supplied \
                 twice, and a fold that counted it twice would double a continuation"
            ));
        }
        ordered.push((event, event_id));
    }
    ordered.sort_by(|(left, left_id), (right, right_id)| {
        left.created_at
            .as_secs()
            .cmp(&right.created_at.as_secs())
            .then_with(|| left_id.cmp(right_id))
    });

    let mut fold = HandoverFold {
        claim: if context.retired {
            ClaimState::NoClaim
        } else {
            context.claim.clone()
        },
        claim_since: if context.retired {
            None
        } else {
            context.claim_since
        },
        retired: context.retired,
        ..HandoverFold::default()
    };

    for (event, event_id) in ordered {
        // The signature is the author, so it is checked before anybody is
        // named one.
        crate::verify_event(event).map_err(|error| {
            format!("coding-session handover {event_id} has an invalid signature: {error}")
        })?;
        let payload = validate_coding_session_handover_envelope(event)
            .map_err(|error| format!("coding-session handover {event_id} is invalid: {error}"))?;
        if payload.session_ref != context.session_ref || payload.genesis_ref != context.genesis_ref
        {
            return Err(format!(
                "coding-session handover {event_id} names a different session or genesis than the \
                 fold's context"
            ));
        }
        let channel = event
            .tags
            .iter()
            .find_map(|tag| match tag.as_slice() {
                [name, value, ..] if name == "h" => Some(value.clone()),
                _ => None,
            })
            .unwrap_or_default();
        if channel != context.channel_ref {
            return Err(format!(
                "coding-session handover {event_id} is scoped to channel {channel}, not the \
                 fold's {}",
                context.channel_ref
            ));
        }

        let author = event.pubkey.to_hex();
        let created_at = event.created_at.as_secs() as i64;
        match payload.body {
            CodingSessionHandoverBody::Checkpoint(body) => {
                let standing = if checkpoint_standing(context, &author, created_at) {
                    HandoverStanding::Authorized
                } else {
                    fold.excluded.push(HandoverExclusion {
                        event_id: event_id.clone(),
                        reason: format!(
                            "checkpoint by {author} is not used for reconstruction: that pubkey \
                             was neither the founder nor a live operator nor an active seat of \
                             this umbrella when it was written"
                        ),
                    });
                    HandoverStanding::Unauthorized
                };
                fold.checkpoints.push(HandoverCheckpointEntry {
                    event_id,
                    author,
                    created_at,
                    standing,
                    superseded_by: None,
                    body,
                });
            }
            CodingSessionHandoverBody::Continuation(body) => {
                let claim_ref = body.claim_ref.clone();
                let mode = body.mode;
                let standing = match context.claim.active() {
                    Some(claim) if claim.accepted_event_id == claim_ref => {
                        if claim.claimant == author {
                            HandoverStanding::Authorized
                        } else {
                            fold.excluded.push(HandoverExclusion {
                                event_id: event_id.clone(),
                                reason: format!(
                                    "continuation by {author} names the claim in force, but that \
                                     claim is held by {}",
                                    claim.claimant
                                ),
                            });
                            HandoverStanding::Unauthorized
                        }
                    }
                    _ => {
                        fold.excluded.push(HandoverExclusion {
                            event_id: event_id.clone(),
                            reason: format!(
                                "continuation acted on claim {claim_ref}, which is not the claim \
                                 in force: it is history, not the current state of this session"
                            ),
                        });
                        HandoverStanding::Superseded
                    }
                };
                fold.continuations.push(HandoverContinuationEntry {
                    event_id,
                    author,
                    created_at,
                    claim_ref,
                    mode,
                    standing,
                    body,
                });
            }
        }
    }

    apply_checkpoint_supersession(&mut fold);

    if context.retired {
        // Listed, never acted on. Every record of a retired umbrella carries
        // its reason so a surface says "deleted" rather than showing an empty
        // panel over records that plainly exist.
        for event_id in fold
            .checkpoints
            .iter()
            .map(|entry| entry.event_id.clone())
            .chain(
                fold.continuations
                    .iter()
                    .map(|entry| entry.event_id.clone()),
            )
            .collect::<Vec<_>>()
        {
            if !fold
                .excluded
                .iter()
                .any(|exclusion| exclusion.event_id == event_id)
            {
                fold.excluded.push(HandoverExclusion {
                    event_id,
                    reason: "this session was deleted: nothing here is reconstructed or resumed"
                        .to_owned(),
                });
            }
        }
        return Ok(fold);
    }

    // Newest among the authorized checkpoints **nobody has replaced**. A
    // superseded entry is no longer `Authorized`, so it cannot be chosen here
    // however its id happens to hash (finding 3).
    fold.latest_authorized_checkpoint = fold
        .checkpoints
        .iter()
        .rev()
        .find(|entry| entry.standing == HandoverStanding::Authorized)
        .map(|entry| entry.event_id.clone());
    fold.active_continuation = fold
        .continuations
        .iter()
        .rev()
        .find(|entry| entry.standing == HandoverStanding::Authorized)
        .map(|entry| entry.event_id.clone());
    Ok(fold)
}

/// Apply each author's own supersession statements to their checkpoints.
///
/// One rule, deliberately narrow: an **authorized** checkpoint that names
/// another checkpoint in `prevCheckpointRef` supersedes that one when the
/// target exists in this fold, was written by the **same author**, and was
/// itself authorized. Anything else — an unknown id, another author's
/// checkpoint, a continuation's id, an unauthorized target, a self-reference —
/// has no effect at all, and the reference stays on the entry verbatim for a
/// reader to make of what it will. Nobody supersedes anybody else's statement,
/// and nothing an unauthorized author writes moves an authorized record.
///
/// **No clock is consulted.** The reference *is* the ordering: that is the
/// whole point of the field (finding 3, where three checkpoints in one second
/// were ordered by a hash). A chain — C names B, B names A — leaves only C
/// standing, and a pair that names each other leaves neither, which is the
/// safe reading of two statements that both claim to replace the other.
fn apply_checkpoint_supersession(fold: &mut HandoverFold) {
    // (target id → the authorized checkpoint that named it). Built in one pass
    // over the entries so a chain of any length resolves without recursion.
    let mut superseded_by: Vec<(String, String)> = Vec::new();
    for namer in &fold.checkpoints {
        if namer.standing != HandoverStanding::Authorized {
            continue;
        }
        let Some(target_id) = namer.body.prev_checkpoint_ref.as_deref() else {
            continue;
        };
        if target_id == namer.event_id {
            continue;
        }
        let names_an_authorized_checkpoint_of_the_same_author =
            fold.checkpoints.iter().any(|target| {
                target.event_id == target_id
                    && target.author == namer.author
                    && target.standing == HandoverStanding::Authorized
            });
        if names_an_authorized_checkpoint_of_the_same_author {
            superseded_by.push((target_id.to_owned(), namer.event_id.clone()));
        }
    }
    for (target_id, namer_id) in superseded_by {
        if let Some(target) = fold
            .checkpoints
            .iter_mut()
            .find(|entry| entry.event_id == target_id)
        {
            target.standing = HandoverStanding::Superseded;
            target.superseded_by = Some(namer_id.clone());
            fold.excluded.push(HandoverExclusion {
                event_id: target_id,
                reason: format!(
                    "checkpoint replaced by {namer_id}, which its own author wrote as the next \
                     one: a reconstruction starts from the newest statement, not the newest \
                     timestamp"
                ),
            });
        }
    }
}

/// Whether `author` held standing to checkpoint at `created_at`.
///
/// The founder always does — the umbrella is theirs. Everyone else needs a
/// grant or a seat whose acceptance is **not later than** the record: a person
/// granted at 12:05 did not have standing at 12:00, and pretending otherwise
/// would let a record be authorized retroactively by an act it predates.
fn checkpoint_standing(context: &HandoverFoldContext, author: &str, created_at: i64) -> bool {
    if author == context.founder_pubkey {
        return true;
    }
    context
        .grants
        .iter()
        .any(|(pubkey, accepted_at)| pubkey == author && *accepted_at <= created_at)
        || context
            .seats
            .iter()
            .any(|(pubkey, _role, accepted_at)| pubkey == author && *accepted_at <= created_at)
}

#[cfg(test)]
#[path = "coding_session_handover_fold_tests.rs"]
mod tests;

// The one fixture the Desktop twin is pinned to, in its own file so neither
// this module nor its tests passes 1,000 lines.
#[cfg(test)]
#[path = "coding_session_handover_fixture_tests.rs"]
mod fixture_tests;
