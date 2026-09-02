//! The bounded projection every observer's screen reads (kind 44246).
//!
//! A child of `coding_session_observation`, split out only to keep every file
//! under 1,000 lines. `use super::*` gives it the parent's types and constants.
//!
//! # What this fold is not
//!
//! It is not the governance fold. It has no authority model, no supersession,
//! no correction validator and no exclusion codes, because an observation
//! **cannot deny anything**: it settles nothing, authorizes nothing, and
//! blocks nothing. The strongest thing that can happen to one here is that a
//! later statement by the same author about the same gate or the same finding
//! is shown instead — and the older event id is still listed, because it is
//! still on the wire and a reader may want it.
//!
//! # Order, and why no clock appears in it
//!
//! "Newest" means **last in the order the caller supplied**. No `created_at`
//! is read, and no `startedAtMs` or `durationMs` is either: every one of those
//! is the author's own measurement, disclosed as such, and using one for
//! ordering, discovery or dedupe would let an author reorder somebody else's
//! record by writing a number.

use std::collections::BTreeMap;

use nostr::Event;

use super::*;

/// The most entries any one collection in this fold will hold.
///
/// Observations are the highest-volume stream a session carries — every seat
/// writes many an hour — so the projection is bounded and says how much it
/// dropped rather than growing without limit or silently truncating.
pub const MAX_OBSERVATION_FOLD_ENTRIES: usize = 512;

/// The most event ids one folded gate row or finding will list.
///
/// A gate row and a finding are the two entries a republish appends to, and in
/// the one kind whose own justification is that every seat writes many an hour
/// (see this module's header) an unbounded list is the same defect §8 I10
/// exists to prevent — it was simply hiding inside an entry instead of at the
/// top level (REVIEW-L1 F2). The **newest** ids are kept: the newest statement
/// is the one shown, so its immediate neighbours are the ones a reader chasing
/// it wants, and what fell off the front is counted rather than hidden.
pub const MAX_OBSERVATION_ENTRY_EVENT_IDS: usize = 16;

/// What the caller can resolve an observation's `assignmentRef` against.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CodingSessionObservationFoldContext {
    /// Canonical lowercase UUID of the umbrella these observations belong to.
    pub session_ref: String,
    /// Lowercase 64-hex event id of the session genesis.
    pub genesis_ref: String,
    /// Event ids an `assignmentRef` may resolve to.
    ///
    /// Anything not in this set is disclosed as unresolved. Empty means the
    /// caller supplied no assignments, which makes every `assignmentRef`
    /// unresolved — the honest answer, and never an exclusion.
    pub known_assignment_refs: Vec<String>,
}

/// One folded checkpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodingSessionObservationCheckpointEntry {
    /// Event id of the observation.
    pub event_id: String,
    /// Canonical lowercase-hex pubkey that signed it.
    pub author_pubkey: String,
    /// The assignment it points at, as the author wrote it.
    pub assignment_ref: Option<String>,
    /// The checkpoint body, reproduced.
    pub body: CodingSessionObservationCheckpoint,
}

/// One folded gate row: the newest statement one author made about one gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodingSessionObservationGateEntry {
    /// Canonical lowercase-hex pubkey that signed it.
    pub author_pubkey: String,
    /// The row itself, from the newest observation naming this gate.
    pub row: CodingSessionObservationGateRow,
    /// The newest [`MAX_OBSERVATION_ENTRY_EVENT_IDS`] observations this author
    /// published naming this gate, in the caller's supplied order. The last is
    /// the one shown.
    pub event_ids: Vec<String>,
    /// How many older ids fell off the front of `event_ids`.
    pub dropped_event_ids: usize,
    /// The assignment the newest of them points at.
    pub assignment_ref: Option<String>,
}

/// One folded finding: the newest disposition one author gave one findingId.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodingSessionObservationFindingEntry {
    /// Canonical lowercase-hex pubkey that signed it.
    pub author_pubkey: String,
    /// The finding body from the newest observation carrying this id.
    pub body: CodingSessionObservationFinding,
    /// The newest [`MAX_OBSERVATION_ENTRY_EVENT_IDS`] observations this author
    /// published for this id, in the caller's supplied order.
    pub event_ids: Vec<String>,
    /// How many older ids fell off the front of `event_ids`.
    pub dropped_event_ids: usize,
    /// The assignment the newest of them points at.
    pub assignment_ref: Option<String>,
}

/// One folded phase timing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodingSessionObservationPhaseEntry {
    /// Event id of the observation.
    pub event_id: String,
    /// Canonical lowercase-hex pubkey that signed it.
    pub author_pubkey: String,
    /// The assignment it points at, as the author wrote it.
    pub assignment_ref: Option<String>,
    /// The timing body, reproduced. Every number in it is the author's claim.
    pub body: CodingSessionObservationPhaseTiming,
}

/// One observation whose `assignmentRef` names nothing the caller supplied.
///
/// A disclosure, never an exclusion: the observation itself is folded exactly
/// as it would be with no pointer at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodingSessionObservationUnresolvedRef {
    /// Event id of the observation.
    pub event_id: String,
    /// The `assignmentRef` that resolved to nothing, as the author wrote it.
    pub assignment_ref: String,
}

/// One event this fold could not read, and why.
///
/// Additive to the four collections and `unresolved`, and deliberately not
/// silent: an event nobody can read is a different fact from no event at all,
/// and a projection that hid it would be exactly the lie kind 44246 exists to
/// avoid. Nothing here denies anything either — a malformed observation costs
/// only itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodingSessionObservationIgnored {
    /// Event id of the ignored event.
    pub event_id: String,
    /// Bounded reason, suitable for a log line or a conformance test.
    pub reason: String,
}

/// How much each collection dropped at [`MAX_OBSERVATION_FOLD_ENTRIES`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CodingSessionObservationTruncation {
    /// Checkpoints not listed.
    pub checkpoints: usize,
    /// Gate rows not listed.
    pub gates: usize,
    /// Findings not listed.
    pub findings: usize,
    /// Phase timings not listed.
    pub phases: usize,
    /// Unresolved pointers not listed.
    pub unresolved: usize,
    /// Ignored events not listed.
    pub ignored: usize,
    /// Event ids dropped from the front of gate and finding entries, summed.
    pub entry_event_ids: usize,
}

impl CodingSessionObservationTruncation {
    /// Whether anything at all was dropped.
    pub const fn any(&self) -> bool {
        self.checkpoints > 0
            || self.gates > 0
            || self.findings > 0
            || self.phases > 0
            || self.unresolved > 0
            || self.ignored > 0
            || self.entry_event_ids > 0
    }
}

/// The four observation facts, plus what the fold could not resolve or read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CodingSessionObservationFold {
    /// Checkpoints, in the caller's supplied order.
    pub checkpoints: Vec<CodingSessionObservationCheckpointEntry>,
    /// Gate rows, one per `(author, gate)`, in first-seen order.
    pub gates: Vec<CodingSessionObservationGateEntry>,
    /// Findings, one per `(author, findingId)`, in first-seen order.
    pub findings: Vec<CodingSessionObservationFindingEntry>,
    /// Phase timings, in the caller's supplied order.
    pub phases: Vec<CodingSessionObservationPhaseEntry>,
    /// Pointers that resolved to nothing the caller supplied.
    pub unresolved: Vec<CodingSessionObservationUnresolvedRef>,
    /// Events this fold could not read at all.
    pub ignored: Vec<CodingSessionObservationIgnored>,
    /// What each bounded collection dropped.
    pub truncated: CodingSessionObservationTruncation,
}

/// Fold signed kind 44246 events into the observer's four collections.
///
/// **The signature is the author**, and it is verified before any pubkey is
/// attributed an observation: `author_pubkey`, and with it the `(author, gate)`
/// and `(author, findingId)` dedupe keys, would otherwise be whatever an
/// unverified event claimed (REVIEW-L1 F1).
///
/// **Never fails.** There is no whole-set hard error and no exclusion: an event
/// whose signature does not check, or that is malformed, cross-context, or not
/// an observation at all, is listed in
/// [`CodingSessionObservationFold::ignored`] and costs nothing but itself. That
/// is the whole reason these four facts are not 44244 subtypes — on that kind
/// one bad envelope reads a whole session as a broken mission.
///
/// Dedupe is by `(author, gate)` and `(author, findingId)`, newest-wins, where
/// newest is **last in `events`**. Both event ids stay listed. Checkpoints and
/// phase timings are never deduped: each is a distinct moment its author
/// recorded, and collapsing them would delete the history the observer came
/// for.
pub fn fold_coding_session_observations(
    events: &[Event],
    context: &CodingSessionObservationFoldContext,
) -> CodingSessionObservationFold {
    let mut fold = CodingSessionObservationFold::default();
    let mut gates: BTreeMap<(String, String), CodingSessionObservationGateEntry> = BTreeMap::new();
    let mut gate_order: Vec<(String, String)> = Vec::new();
    let mut findings: BTreeMap<(String, String), CodingSessionObservationFindingEntry> =
        BTreeMap::new();
    let mut finding_order: Vec<(String, String)> = Vec::new();

    for event in events {
        let event_id = event.id.to_hex();
        // The signature is the author, so it is checked before anybody is
        // named one (REVIEW-L1 F1). The 44244 fold verifies as its first act
        // for the same reason; the difference is only what a failure costs —
        // there it is a whole-set `Err`, here it is one listed line.
        if let Err(error) = crate::verify_event(event) {
            push_bounded(
                &mut fold.ignored,
                &mut fold.truncated.ignored,
                CodingSessionObservationIgnored {
                    event_id,
                    reason: format!("invalid observation signature: {error}"),
                },
            );
            continue;
        }
        let author = event.pubkey.to_hex();
        let payload = match validate_coding_session_observation_envelope(event) {
            Ok(payload) => payload,
            Err(reason) => {
                push_bounded(
                    &mut fold.ignored,
                    &mut fold.truncated.ignored,
                    CodingSessionObservationIgnored { event_id, reason },
                );
                continue;
            }
        };
        if payload.session_ref != context.session_ref || payload.genesis_ref != context.genesis_ref
        {
            push_bounded(
                &mut fold.ignored,
                &mut fold.truncated.ignored,
                CodingSessionObservationIgnored {
                    event_id,
                    reason: "observation names a different session or genesis than the fold's \
                             context"
                        .to_owned(),
                },
            );
            continue;
        }
        if let Some(reference) = &payload.assignment_ref {
            if !context.known_assignment_refs.contains(reference) {
                push_bounded(
                    &mut fold.unresolved,
                    &mut fold.truncated.unresolved,
                    CodingSessionObservationUnresolvedRef {
                        event_id: event_id.clone(),
                        assignment_ref: reference.clone(),
                    },
                );
            }
        }
        match payload.body {
            CodingSessionObservationBody::Checkpoint(body) => push_bounded(
                &mut fold.checkpoints,
                &mut fold.truncated.checkpoints,
                CodingSessionObservationCheckpointEntry {
                    event_id,
                    author_pubkey: author,
                    assignment_ref: payload.assignment_ref,
                    body,
                },
            ),
            CodingSessionObservationBody::Phase(body) => push_bounded(
                &mut fold.phases,
                &mut fold.truncated.phases,
                CodingSessionObservationPhaseEntry {
                    event_id,
                    author_pubkey: author,
                    assignment_ref: payload.assignment_ref,
                    body,
                },
            ),
            CodingSessionObservationBody::Gate(body) => {
                for row in body.rows {
                    let key = (author.clone(), row.gate.clone());
                    match gates.get_mut(&key) {
                        Some(entry) => {
                            entry.row = row;
                            entry.assignment_ref = payload.assignment_ref.clone();
                            push_newest_event_id(
                                &mut entry.event_ids,
                                &mut entry.dropped_event_ids,
                                event_id.clone(),
                            );
                        }
                        None => {
                            gate_order.push(key.clone());
                            gates.insert(
                                key,
                                CodingSessionObservationGateEntry {
                                    author_pubkey: author.clone(),
                                    row,
                                    event_ids: vec![event_id.clone()],
                                    dropped_event_ids: 0,
                                    assignment_ref: payload.assignment_ref.clone(),
                                },
                            );
                        }
                    }
                }
            }
            CodingSessionObservationBody::Finding(body) => {
                let key = (author.clone(), body.finding_id.clone());
                match findings.get_mut(&key) {
                    Some(entry) => {
                        entry.body = body;
                        entry.assignment_ref = payload.assignment_ref.clone();
                        push_newest_event_id(
                            &mut entry.event_ids,
                            &mut entry.dropped_event_ids,
                            event_id.clone(),
                        );
                    }
                    None => {
                        finding_order.push(key.clone());
                        findings.insert(
                            key,
                            CodingSessionObservationFindingEntry {
                                author_pubkey: author.clone(),
                                body,
                                event_ids: vec![event_id.clone()],
                                dropped_event_ids: 0,
                                assignment_ref: payload.assignment_ref.clone(),
                            },
                        );
                    }
                }
            }
        }
    }

    for key in gate_order {
        if let Some(entry) = gates.remove(&key) {
            fold.truncated.entry_event_ids += entry.dropped_event_ids;
            push_bounded(&mut fold.gates, &mut fold.truncated.gates, entry);
        }
    }
    for key in finding_order {
        if let Some(entry) = findings.remove(&key) {
            fold.truncated.entry_event_ids += entry.dropped_event_ids;
            push_bounded(&mut fold.findings, &mut fold.truncated.findings, entry);
        }
    }
    fold
}

/// Append one id to an entry's bounded list, dropping the oldest when full.
///
/// The window slides forward rather than closing: the newest statement is the
/// one the entry shows, so keeping the newest ids beside it is what a reader
/// following the entry actually needs (REVIEW-L1 F2).
fn push_newest_event_id(into: &mut Vec<String>, dropped: &mut usize, event_id: String) {
    into.push(event_id);
    if into.len() > MAX_OBSERVATION_ENTRY_EVENT_IDS {
        into.remove(0);
        *dropped += 1;
    }
}

/// Push into a collection bounded at [`MAX_OBSERVATION_FOLD_ENTRIES`],
/// counting what did not fit so the caller can say so.
fn push_bounded<T>(into: &mut Vec<T>, dropped: &mut usize, entry: T) {
    if into.len() < MAX_OBSERVATION_FOLD_ENTRIES {
        into.push(entry);
    } else {
        *dropped += 1;
    }
}

#[cfg(test)]
#[path = "coding_session_observation_fold_tests.rs"]
mod tests;
