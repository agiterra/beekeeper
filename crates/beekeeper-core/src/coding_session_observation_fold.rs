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
//!
//! One narrow exception, and it orders nothing: a gate start (SV-41) is paired
//! with its own closing row on `(author, gate, startedAtMs)`. That match can
//! only ever join an author's row to the same author's other row, so nobody
//! can move anyone else's record with it (`coding_session_observation_gate_start.rs`).
//!
//! **The contract, stated once: callers hand
//! [`fold_coding_session_observations`] a relay page oldest-first; a page read
//! newest-first must be reversed — finding 79.** The order comes from the
//! relay's storage, never from a number in the event, and the relay's
//! canonical page order is newest-first (`created_at DESC, id ASC`, the
//! `query_events` `ORDER BY` in `buzz-db`). Every reader that folds such a
//! page as read — the push gate, `bee sessions observations`, `bee git`'s
//! prediction, Pulse — was crowning the **oldest** row per `(author, gate)`:
//! a seat's first `cargo fmt` row, red and dirty at the base commit because
//! hermit was not on `PATH` yet, outranked four later green rows on the pushed
//! commit, and the relay refused every push with "gate `cargo fmt` has no
//! observed green row" (finding 79, Andy's machine, 2026-09-04).
//! [`fold_coding_session_observation_page`] is the entry point for a page in
//! that canonical order; it reverses before folding so nobody has to remember
//! to.
//!
//! # Provenance never merges
//!
//! Dedupe keys carry `source`, so an **observed** gate row and a **declared**
//! one about the same gate are two entries and neither supersedes the other.
//! In practice their authors already differ — the provider signs what it
//! watched, the seat signs what it says — but the rule is written into the key
//! rather than left to that coincidence, because the one thing this field
//! exists to prevent is a claim quietly taking the place of a measurement.

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
    /// Pubkeys whose `observed` claim this session honours: the provider
    /// instances running its executions.
    ///
    /// **`None` is not an empty set.** `None` means the caller could not
    /// resolve them, so nothing is checked and every claim stands as written,
    /// with [`CodingSessionObservationFold::provenance_checked`] `false` so a
    /// surface can say the check did not run. `Some(set)` means the caller
    /// knows, and a signer outside the set has its `observed` claim folded down
    /// to `declared` and listed in
    /// [`CodingSessionObservationFold::misclaimed_observed`] (REVIEW-L5 F2):
    /// the CLI refuses to mint such a row, but the wire does not, and a reader
    /// that ranked a self-asserted `observed` above a declared one — and
    /// printed "the record names the watcher" over it — would be repeating a
    /// claim as a measurement.
    pub provider_pubkeys: Option<Vec<String>>,
}

/// One folded checkpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodingSessionObservationCheckpointEntry {
    /// Event id of the observation.
    pub event_id: String,
    /// Canonical lowercase-hex pubkey that signed it.
    pub author_pubkey: String,
    /// Whether a mechanism watched this, or its subject claimed it.
    pub source: CodingSessionObservationSource,
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
    /// Whether a mechanism watched this row, or its subject claimed it.
    pub source: CodingSessionObservationSource,
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
    /// Whether a mechanism watched this, or its subject claimed it.
    pub source: CodingSessionObservationSource,
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
    /// Whether a mechanism watched this, or its subject claimed it.
    pub source: CodingSessionObservationSource,
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

/// One row that claimed `observed` from a signer the caller does not know as a
/// provider instance.
///
/// A disclosure, never an exclusion: the row is folded exactly as a `declared`
/// one, because a bad claim about provenance costs only the claim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodingSessionObservationMisclaimedProvenance {
    /// Event id of the observation.
    pub event_id: String,
    /// The signer that claimed to be watching.
    pub author_pubkey: String,
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
    /// Misclaimed-provenance rows not listed.
    pub misclaimed_observed: usize,
    /// Event ids dropped from the front of gate and finding entries, summed.
    pub entry_event_ids: usize,
    /// Gate rows a later statement by the same author and provenance replaced.
    ///
    /// Newest-wins is right — a seat that re-runs a gate should show the newer
    /// result — but the displaced statement used to leave no trace except an
    /// id in a list, so a `failed` row replaced by a `passed` one read exactly
    /// like a gate that had only ever passed (REVIEW-L5 F1). The count makes
    /// the replacement a fact a surface can state.
    pub displaced_gates: usize,
    /// Findings a later disposition by the same author and provenance replaced.
    pub displaced_findings: usize,
    /// Gate starts not listed at [`MAX_OBSERVATION_GATE_STARTS`] (the oldest
    /// fall off; the running ones are what a reader came for).
    pub gate_starts: usize,
    /// Closing rows whose start this page does not hold.
    ///
    /// A disclosure, not a drop, so [`Self::any`] does not read it: the start
    /// fell off the page or was never published, and the close alone states
    /// nothing a reader is missing.
    pub gate_start_closes_unmatched: usize,
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
            || self.misclaimed_observed > 0
            || self.entry_event_ids > 0
            || self.displaced_gates > 0
            || self.displaced_findings > 0
            || self.gate_starts > 0
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
    ///
    /// Never a gate start: an observed `gate:` phase is routed to
    /// [`Self::gate_starts`] instead (SV-41). A declared or misclaimed one
    /// stays here, because it is its author's own words.
    pub phases: Vec<CodingSessionObservationPhaseEntry>,
    /// Gate starts the provider signed, open or closed, in the supplied order
    /// of their start rows, bounded at [`MAX_OBSERVATION_GATE_STARTS`].
    ///
    /// Not outcomes. No verdict, push, Pulse or bench path reads this; a start
    /// can never become a gate row.
    pub gate_starts: Vec<CodingSessionObservationGateStartEntry>,
    /// Pointers that resolved to nothing the caller supplied.
    pub unresolved: Vec<CodingSessionObservationUnresolvedRef>,
    /// Events this fold could not read at all.
    pub ignored: Vec<CodingSessionObservationIgnored>,
    /// Rows that claimed `observed` from a signer no provider instance backs.
    ///
    /// Each is folded as `declared`. Empty when nothing claimed falsely — or
    /// when nothing was checked, which `provenance_checked` distinguishes.
    pub misclaimed_observed: Vec<CodingSessionObservationMisclaimedProvenance>,
    /// Whether the caller supplied the provider set at all.
    ///
    /// `false` means no `observed` claim in this fold has been verified, which
    /// is a different fact from every claim checking out (§8 I9).
    pub provenance_checked: bool,
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
/// newest is **last in `events`** — so `events` is **oldest-first**. A page
/// read from the relay in its canonical order is newest-first and belongs to
/// [`fold_coding_session_observation_page`] instead; folding it here as read
/// crowns the oldest statement per key (finding 79, and this module's header).
/// Both event ids stay listed. Checkpoints and phase timings are never
/// deduped: each is a distinct moment its author recorded, and collapsing them
/// would delete the history the observer came for.
pub fn fold_coding_session_observations(
    events: &[Event],
    context: &CodingSessionObservationFoldContext,
) -> CodingSessionObservationFold {
    fold_in_order(events.iter(), context)
}

/// Fold one relay page of kind 44246 events, handed over **as the relay
/// returned it**: newest first, the canonical `created_at DESC, id ASC` order
/// `buzz-db`'s `query_events` produces and every `REQ`, `POST /query` and
/// `bee … query_all` page inherits.
///
/// This is [`fold_coding_session_observations`] over the page reversed, and it
/// exists so the reversal lives in one place with its reason attached rather
/// than at every reader (finding 79: the relay's push gate, `bee sessions
/// observations`, `bee git`'s prediction and Pulse each folded the page as
/// read and crowned the oldest row per `(author, gate)`). Nothing here reads
/// `created_at`: the page's order is storage's order, and reversing it is the
/// only thing done with it.
///
/// A caller that already holds events oldest-first — a live subscription
/// appended in arrival order, a fixture built in the order it was written —
/// wants the plain fold, not this.
pub fn fold_coding_session_observation_page(
    page_newest_first: &[Event],
    context: &CodingSessionObservationFoldContext,
) -> CodingSessionObservationFold {
    fold_in_order(page_newest_first.iter().rev(), context)
}

/// The fold itself, over events in the order they are yielded: last is
/// newest. Both public entry points are this with an iterator chosen for them.
fn fold_in_order<'a>(
    events: impl Iterator<Item = &'a Event>,
    context: &CodingSessionObservationFoldContext,
) -> CodingSessionObservationFold {
    let mut fold = CodingSessionObservationFold {
        provenance_checked: context.provider_pubkeys.is_some(),
        ..CodingSessionObservationFold::default()
    };
    // Key: (author, source token, gate | findingId). `source` is in the key so
    // a declared row can never supersede an observed one, or the reverse.
    let mut gates: BTreeMap<(String, &'static str, String), CodingSessionObservationGateEntry> =
        BTreeMap::new();
    let mut gate_order: Vec<(String, &'static str, String)> = Vec::new();
    let mut findings: BTreeMap<
        (String, &'static str, String),
        CodingSessionObservationFindingEntry,
    > = BTreeMap::new();
    let mut finding_order: Vec<(String, &'static str, String)> = Vec::new();
    let mut gate_starts = super::gate_start::GateStartCollector::default();

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
        // REVIEW-L5 F2. `observed` is honoured only when the signer is one of
        // this session's provider instances; anything else is the subject
        // speaking about itself, which is exactly what `declared` means.
        //
        // `measured` is deliberately NOT under this check: a bench row is
        // signed by whoever ran the bench, and its subject is a routing target
        // rather than the signer's own work, so "the provider signed it" is not
        // the question to ask of it. The consequence is disclosed rather than
        // hidden: nothing here stops a seat signing `measured` about itself,
        // and a surface that ranks `measured` above `declared` is trusting the
        // signer. That is the same two-axes-one-key residual the widening was
        // named with, and it is owed a second key rather than a stricter fold.
        let source = match (payload.source, &context.provider_pubkeys) {
            (CodingSessionObservationSource::Observed, Some(providers))
                if !providers.contains(&author) =>
            {
                push_bounded(
                    &mut fold.misclaimed_observed,
                    &mut fold.truncated.misclaimed_observed,
                    CodingSessionObservationMisclaimedProvenance {
                        event_id: event_id.clone(),
                        author_pubkey: author.clone(),
                    },
                );
                CodingSessionObservationSource::Declared
            }
            (source, _) => source,
        };
        match payload.body {
            CodingSessionObservationBody::Checkpoint(body) => push_bounded(
                &mut fold.checkpoints,
                &mut fold.truncated.checkpoints,
                CodingSessionObservationCheckpointEntry {
                    event_id,
                    author_pubkey: author,
                    source,
                    assignment_ref: payload.assignment_ref,
                    body,
                },
            ),
            CodingSessionObservationBody::Phase(body) => {
                // SV-41: only the *effective* source counts, so a seat's own
                // `gate:` phase, and an `observed` claim folded down above,
                // stay ordinary phases.
                let routed = if source == CodingSessionObservationSource::Observed {
                    gate_starts.take(event_id, &author, payload.assignment_ref, body)
                } else {
                    Some((event_id, payload.assignment_ref, body))
                };
                if let Some((event_id, assignment_ref, body)) = routed {
                    push_bounded(
                        &mut fold.phases,
                        &mut fold.truncated.phases,
                        CodingSessionObservationPhaseEntry {
                            event_id,
                            author_pubkey: author,
                            source,
                            assignment_ref,
                            body,
                        },
                    );
                }
            }
            CodingSessionObservationBody::Gate(body) => {
                for row in body.rows {
                    let key = (author.clone(), source.as_str(), row.gate.clone());
                    match gates.get_mut(&key) {
                        Some(entry) => {
                            // The newer statement wins and the older one is
                            // counted: replacement is a fact, not a silence.
                            fold.truncated.displaced_gates += 1;
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
                                    source,
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
                let key = (author.clone(), source.as_str(), body.finding_id.clone());
                match findings.get_mut(&key) {
                    Some(entry) => {
                        fold.truncated.displaced_findings += 1;
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
                                source,
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
    gate_starts.finish(&mut fold);
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
