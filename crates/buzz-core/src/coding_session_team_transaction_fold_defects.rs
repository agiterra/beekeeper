//! Record-local defect detection for the NIP-CSTX fold.
//!
//! A child of `coding_session_team_transaction_fold`, split out only to keep
//! every file under 1,000 lines (REVIEW-B1b R6). No behaviour change: these
//! are the same functions, and `use super::*` gives them the parent's private
//! `Record` and helpers exactly as before.

use std::collections::HashMap;

use super::*;

/// A defect in one supplied record, judged without reference to the rest of the
/// graph, which excludes that record alone.
pub(super) struct RecordDefect {
    pub(super) code: CodingSessionTeamFoldExclusionCode,
    pub(super) reason: String,
}

/// Map each record with a record-local defect to the exclusion it earns.
///
/// Three defects, checked in this order because each can only be judged once
/// the one before it is ruled out: a reference naming an id nobody supplied
/// ([`CodingSessionTeamFoldExclusionCode::DanglingReference`]), then a
/// resolvable reference that cannot stand where the record puts it
/// ([`CodingSessionTeamFoldExclusionCode::WrongTypeReference`]), then an
/// invalid claim to correct a supplied record
/// ([`CodingSessionTeamFoldExclusionCode::InvalidCorrection`]). One record
/// earns at most one exclusion, and its `reason` carries the exact diagnostic.
///
/// Deterministic by construction: the record order is the supplied index order
/// and the reference order comes from the payload, never from a hash iteration.
pub(super) fn collect_record_defects(
    records: &[Record<'_>],
    by_id: &HashMap<String, usize>,
) -> HashMap<usize, RecordDefect> {
    let mut defects = HashMap::new();
    for (index, record) in records.iter().enumerate() {
        let mut references: Vec<&str> = record.payload.causal_references();
        if let Some(reference) = &record.payload.supersedes {
            references.push(reference);
        }
        if let Some(missing) = references
            .into_iter()
            .find(|reference| !by_id.contains_key(*reference))
        {
            defects.insert(
                index,
                RecordDefect {
                    code: CodingSessionTeamFoldExclusionCode::DanglingReference,
                    reason: format!(
                        "reference {missing} is absent from the supplied transaction set"
                    ),
                },
            );
            continue;
        }
        if let Err(detail) = validate_causal_types(record, records, by_id) {
            defects.insert(
                index,
                RecordDefect {
                    code: CodingSessionTeamFoldExclusionCode::WrongTypeReference,
                    reason: detail,
                },
            );
            continue;
        }
        if let Some(detail) = invalid_correction_detail(record, records, by_id) {
            defects.insert(
                index,
                RecordDefect {
                    code: CodingSessionTeamFoldExclusionCode::InvalidCorrection,
                    reason: detail,
                },
            );
        }
    }
    defects
}

/// Describe why this record's claim to correct another is invalid, or `None`
/// when it makes no such claim or the claim is sound.
///
/// The corrected record is only *read* here. A rejected correction leaves its
/// target exactly as the projection already found it — it can neither revive an
/// excluded record nor displace an included one.
fn invalid_correction_detail(
    record: &Record<'_>,
    records: &[Record<'_>],
    by_id: &HashMap<String, usize>,
) -> Option<String> {
    let previous = &records[*by_id.get(record.payload.supersedes.as_ref()?)?];
    let detail = if let Err(error) =
        validate_coding_session_team_transaction_supersession(record.event, previous.event)
    {
        error
    } else if verdict_subtype(&record.payload) != verdict_subtype(&previous.payload) {
        "a verdict correction must preserve its subtype".to_string()
    } else if logical_subject(&record.payload) != logical_subject(&previous.payload) {
        "a correction must preserve its logical subject".to_string()
    } else {
        return None;
    };
    Some(format!(
        "correction of {} is invalid: {detail}",
        previous.id
    ))
}

fn validate_causal_types(
    record: &Record<'_>,
    records: &[Record<'_>],
    by_id: &HashMap<String, usize>,
) -> Result<(), String> {
    // Absent references are excluded per-record as `DanglingReference` before
    // this runs, so this check simply has nothing to say about them. Its `Err`
    // is not a fold failure: `collect_record_defects` turns it into that one
    // record's `WrongTypeReference` exclusion.
    let get = |reference: &str| -> Option<&Record<'_>> {
        by_id.get(reference).map(|index| &records[*index])
    };
    match &record.payload.body {
        CodingSessionTeamTransactionBody::Assignment(_) => {}
        CodingSessionTeamTransactionBody::Report(body) => {
            if let Some(assignment) = get(&body.assignment_ref) {
                require_type(assignment, CodingSessionTeamTransactionType::Assignment)?;
            }
        }
        CodingSessionTeamTransactionBody::Verdict(verdict) => {
            if let Some(assignment) = get(verdict.assignment_ref()) {
                require_type(assignment, CodingSessionTeamTransactionType::Assignment)?;
            }
            if let Some(report) = get(verdict.report_ref()) {
                require_type(report, CodingSessionTeamTransactionType::Report)?;
                let CodingSessionTeamTransactionBody::Report(report_body) = &report.payload.body
                else {
                    return Err("verdict reportRef must name a report body".into());
                };
                if resolved_pointers_disagree(
                    by_id,
                    &report_body.assignment_ref,
                    verdict.assignment_ref(),
                ) {
                    return Err(
                        "verdict assignmentRef must match its report's assignmentRef".into(),
                    );
                }
            }
            if let CodingSessionTeamVerdict::Disposition {
                refutation_ref: Some(reference),
                ..
            } = verdict
            {
                if let Some(refutation) = get(reference) {
                    let CodingSessionTeamTransactionBody::Verdict(
                        CodingSessionTeamVerdict::Refutation {
                            assignment_ref,
                            report_ref,
                            ..
                        },
                    ) = &refutation.payload.body
                    else {
                        return Err(
                            "disposition refutationRef must name a refutation verdict".into()
                        );
                    };
                    if resolved_pointers_disagree(by_id, assignment_ref, verdict.assignment_ref())
                        || resolved_pointers_disagree(by_id, report_ref, verdict.report_ref())
                    {
                        return Err(
                            "disposition refutationRef must govern the same assignment/report pair"
                                .into(),
                        );
                    }
                }
            }
        }
        CodingSessionTeamTransactionBody::Acknowledgement(body) => {
            if let Some(acknowledged) = get(&body.acknowledged_event_ref) {
                let CodingSessionTeamTransactionBody::Verdict(verdict) = &acknowledged.payload.body
                else {
                    return Err("acknowledgement must name a disposition verdict".into());
                };
                if !matches!(verdict, CodingSessionTeamVerdict::Disposition { .. }) {
                    return Err("acknowledgement must name a disposition verdict".into());
                }
                // `is_authorized` follows this pointer a *second* hop out, to
                // the disposition's own assignment, to find the assigned actor.
                // Type it here so a wrong-type pointer two records away is this
                // record's own exclusion rather than a whole-set failure
                // (REVIEW-B1b F5). When it does not resolve at all the
                // disposition is dangling and already excluded, so this record
                // falls out as `DependentOnExcluded` instead.
                if let Some(assignment) = get(verdict.assignment_ref()) {
                    if assignment.payload.transaction_type
                        != CodingSessionTeamTransactionType::Assignment
                    {
                        return Err("acknowledgement's disposition names a non-assignment".into());
                    }
                }
            }
        }
        CodingSessionTeamTransactionBody::MissionCompleted(body) => {
            for reference in &body.assignment_refs {
                if let Some(assignment) = get(reference) {
                    require_type(assignment, CodingSessionTeamTransactionType::Assignment)?;
                }
            }
        }
        CodingSessionTeamTransactionBody::MissionBlocked(body) => {
            for reference in &body.assignment_refs {
                if let Some(assignment) = get(reference) {
                    require_type(assignment, CodingSessionTeamTransactionType::Assignment)?;
                }
            }
        }
        // A note declares no causal reference at all; its `refs` are pointers
        // and may name any event, including one outside the supplied set.
        CodingSessionTeamTransactionBody::Note(_) => {}
        // Neither does a request: `blocks` are pointers too (REVIEW-B1c F3), so
        // correcting the assignment a question is about must never delete the
        // question. Since finding 16 `blocks` no longer decides the waiting
        // state either — it is re-resolved against the current active set only
        // to answer which completions one open ruling holds up.
        CodingSessionTeamTransactionBody::DecisionRequest(_) => {}
        CodingSessionTeamTransactionBody::DecisionAnswer(body) => {
            if let Some(request) = get(&body.request_ref) {
                require_type(request, CodingSessionTeamTransactionType::DecisionRequest)?;
            }
        }
    }
    Ok(())
}

/// True only when two records' pointers to the *same* subject both resolve in
/// the supplied set and disagree.
///
/// Cross-record pointer agreement is a real structural contract, but it only
/// has canonical meaning when both sides name something the caller supplied. If
/// either id is absent, the record carrying it is already excluded as
/// [`CodingSessionTeamFoldExclusionCode::DanglingReference`] and the record
/// that depends on it falls out as
/// [`CodingSessionTeamFoldExclusionCode::DependentOnExcluded`] — so raising a
/// hard error here would deny the whole session over one seat's mistyped
/// pointer, which is the very denial of service this module was changed to
/// remove (REVIEW-B1b F1: 440 of 500 fuzzed single-reference mistakes took this
/// path).
fn resolved_pointers_disagree(by_id: &HashMap<String, usize>, left: &str, right: &str) -> bool {
    left != right && by_id.contains_key(left) && by_id.contains_key(right)
}

fn require_type(
    record: &Record<'_>,
    expected: CodingSessionTeamTransactionType,
) -> Result<(), String> {
    if record.payload.transaction_type != expected {
        return Err(format!(
            "team transaction {} has a wrong-type reference",
            record.id
        ));
    }
    Ok(())
}
