//! Decoding, envelope validation, and the correction rules for kind 44244.
//!
//! A child of `coding_session_team_transaction`, split out only to keep every
//! file under 1,000 lines (FINAL-B §7). No behaviour change: these are the same
//! functions, and `use super::*` gives them the parent's types, constants and
//! private validators exactly as before.

use nostr::Event;
use serde_json::Value;

use super::*;

/// Strictly decode and validate signed kind 44244 content.
pub fn decode_coding_session_team_transaction(
    content: &str,
) -> Result<CodingSessionTeamTransactionPayload, String> {
    if content.len() > MAX_TEAM_TRANSACTION_CONTENT_BYTES {
        return Err(format!(
            "coding-session team-transaction content exceeds {MAX_TEAM_TRANSACTION_CONTENT_BYTES} bytes"
        ));
    }
    let value: Value = serde_json::from_str(content)
        .map_err(|_| "malformed coding-session team-transaction payload".to_owned())?;
    validate_exact_keys(
        value.as_object().ok_or_else(|| {
            "coding-session team-transaction payload must be an object".to_owned()
        })?,
        &[
            "schema",
            "sessionRef",
            "genesisRef",
            "type",
            "supersedes",
            "deliveryCommandId",
            "body",
        ],
        "team-transaction payload",
    )?;
    let transaction_type: CodingSessionTeamTransactionType = serde_json::from_value(
        value
            .get("type")
            .cloned()
            .ok_or_else(|| "team-transaction type is missing".to_owned())?,
    )
    .map_err(|_| "unsupported coding-session team-transaction type".to_owned())?;
    let body = value
        .get("body")
        .and_then(Value::as_object)
        .ok_or_else(|| "team-transaction body must be an object".to_owned())?;
    let expected_keys = if transaction_type == CodingSessionTeamTransactionType::Verdict {
        match body.get("subtype").and_then(Value::as_str) {
            Some("refutation") => &[
                "subtype",
                "assignmentRef",
                "reportRef",
                "decision",
                "summary",
                "findings",
                "requiredAction",
            ][..],
            Some("disposition") => &[
                "subtype",
                "assignmentRef",
                "reportRef",
                "refutationRef",
                "decision",
                "summary",
                "findings",
                "requiredAction",
            ][..],
            _ => return Err("unsupported team-transaction verdict subtype".to_owned()),
        }
    } else {
        expected_body_keys(transaction_type)
    };
    validate_exact_keys(body, expected_keys, "team-transaction body")?;
    if transaction_type == CodingSessionTeamTransactionType::Report {
        let tests = body
            .get("tests")
            .and_then(Value::as_array)
            .ok_or_else(|| "report tests must be an array".to_owned())?;
        for test in tests {
            validate_exact_keys(
                test.as_object()
                    .ok_or_else(|| "report test must be an object".to_owned())?,
                &["name", "command", "outcome", "evidence"],
                "report test",
            )?;
        }
    }

    // A second strict decode preserves serde's duplicate-field detection,
    // which the Value map above cannot represent.
    let payload: CodingSessionTeamTransactionPayload = serde_json::from_str(content)
        .map_err(|_| "malformed coding-session team-transaction payload".to_owned())?;
    payload.validate()?;
    Ok(payload)
}

/// Validate the exact ordered event envelope and return its decoded payload.
pub fn validate_coding_session_team_transaction_envelope(
    event: &Event,
) -> Result<CodingSessionTeamTransactionPayload, String> {
    if event.kind.as_u16() as u32 != KIND_CODING_SESSION_TEAM_TRANSACTION {
        return Err("coding-session team transaction has the wrong event kind".into());
    }
    let payload = decode_coding_session_team_transaction(&event.content)?;
    let tags: Vec<&[String]> = event.tags.iter().map(|tag| tag.as_slice()).collect();
    if tags.len() != 5 || tags.iter().any(|parts| parts.len() != 2) {
        return Err("coding-session team transaction requires exactly five two-field tags".into());
    }
    if tags[0][0] != "h" {
        return Err("team-transaction first tag must be h=channel UUID".into());
    }
    validate_canonical_uuid("h", &tags[0][1])?;
    if tags[1][0] != "d" || tags[1][1] != payload.session_ref {
        return Err("team-transaction d tag does not match payload sessionRef".into());
    }
    if tags[2][0] != "cstx-v" || tags[2][1] != CODING_SESSION_TEAM_TRANSACTION_SCHEMA {
        return Err("unsupported coding-session team-transaction tag version".into());
    }
    if tags[3][0] != "cstx-genesis" || tags[3][1] != payload.genesis_ref {
        return Err("team-transaction genesis tag does not match payload genesisRef".into());
    }
    if tags[4][0] != "cstx-type" || tags[4][1] != payload.transaction_type.as_str() {
        return Err("team-transaction type tag does not match payload type".into());
    }
    let event_id = event.id.to_hex();
    if payload.supersedes.as_deref() == Some(event_id.as_str())
        || payload
            .causal_references()
            .into_iter()
            .any(|reference| reference == event_id)
    {
        return Err("team transaction cannot reference its own event id".into());
    }
    Ok(payload)
}

/// Validate a correction link when the superseded event is available.
///
/// Corrections must stay within one author, channel, session, genesis, and
/// operation type. Causal workflow links live in operation bodies and are not
/// accepted as substitutes for `supersedes`.
pub fn validate_coding_session_team_transaction_supersession(
    current: &Event,
    previous: &Event,
) -> Result<(), String> {
    let current_payload = validate_coding_session_team_transaction_envelope(current)?;
    let previous_payload = validate_coding_session_team_transaction_envelope(previous)?;
    if current_payload.supersedes.as_deref() != Some(previous.id.to_hex().as_str()) {
        return Err("supersedes does not name the supplied previous event".into());
    }
    if current.pubkey != previous.pubkey {
        return Err("a correction must have the same signed author".into());
    }
    if current_payload.session_ref != previous_payload.session_ref
        || current_payload.genesis_ref != previous_payload.genesis_ref
    {
        return Err("a correction cannot cross session or genesis".into());
    }
    if current_payload.transaction_type != previous_payload.transaction_type
        && !terminal_correction_is_allowed(
            previous_payload.transaction_type,
            current_payload.transaction_type,
        )
    {
        if previous_payload.transaction_type == CodingSessionTeamTransactionType::MissionCompleted
            && current_payload.transaction_type == CodingSessionTeamTransactionType::MissionBlocked
        {
            return Err(TERMINAL_COMPLETION_IS_NOT_REOPENED.into());
        }
        return Err("a correction must preserve the operation type".into());
    }
    if current.tags.as_slice()[0].as_slice() != previous.tags.as_slice()[0].as_slice() {
        return Err("a correction cannot cross channels".into());
    }
    // A terminal that only rewrites its own prose is the shape the lead reached
    // for four times on 2026-09-01. Editing the sentence is now a `note`, and
    // clearing the blocker is a `decision.answer`; a correction of a blocked
    // terminal has to actually change what is blocking.
    if let (
        CodingSessionTeamTransactionBody::MissionBlocked(current_body),
        CodingSessionTeamTransactionBody::MissionBlocked(previous_body),
    ) = (&current_payload.body, &previous_payload.body)
    {
        if same_blocker_set(&current_body.blockers, &previous_body.blockers) {
            return Err(TERMINAL_PROSE_EDIT_NEEDS_A_NOTE.into());
        }
    }
    Ok(())
}

/// Whether a correction may cross the operation type from `previous` to
/// `current`.
///
/// Exactly one crossing is legal: a `mission.blocked` corrected by a
/// `mission.completed` from the same author. Live run TeamRolesV1, finding 14:
/// the lead published `mission.blocked` at 21:57 and `mission.completed` at
/// 02:35, the vocabulary had no way to retract the first, and the fold could
/// only record a `terminal` **conflict** between them. A person reading the
/// Mission rail saw "Blocked" in red for 4 h 38 m over a mission that was
/// working, then "Completed" wearing a conflict badge — an honest projection of
/// a shape the vocabulary made impossible to say correctly.
///
/// A completion correcting a blocked is the *intended* shape and folds to one
/// corrected terminal. The reverse is not: a mission that has completed is not
/// reopened by a later record claiming it never did. Say that with a new
/// `mission.blocked` (which the fold weighs as a fresh terminal contender) or
/// with a `note`, never as a correction of the completion.
///
/// Public since REVIEW-B2 F5 so `bee sessions block|complete --supersedes` can
/// refuse the shape **before signing** through this exact rule. It is the
/// newest and least settled of the three correction rules, and it was the one
/// the CLI had copied.
pub const fn terminal_correction_is_allowed(
    previous: CodingSessionTeamTransactionType,
    current: CodingSessionTeamTransactionType,
) -> bool {
    matches!(
        (previous, current),
        (
            CodingSessionTeamTransactionType::MissionBlocked,
            CodingSessionTeamTransactionType::MissionCompleted,
        )
    )
}

/// Whether two blocker lists name the same set, ignoring order.
///
/// Order is prose: reordering the same blockers says nothing new about what is
/// holding the mission up.
///
/// Public since batch 2 lane B2 so `bee sessions block --supersedes` can refuse
/// a prose-only terminal edit **before signing** with this exact rule, rather
/// than growing a second copy of it in the CLI
/// (`crates/buzz-cli/src/commands/sessions/operations_precheck.rs`).
pub fn same_blocker_set(current: &[String], previous: &[String]) -> bool {
    if current.len() != previous.len() {
        return false;
    }
    let mut current: Vec<&str> = current.iter().map(String::as_str).collect();
    let mut previous: Vec<&str> = previous.iter().map(String::as_str).collect();
    current.sort_unstable();
    previous.sort_unstable();
    current == previous
}

fn expected_body_keys(
    transaction_type: CodingSessionTeamTransactionType,
) -> &'static [&'static str] {
    match transaction_type {
        CodingSessionTeamTransactionType::Assignment => &[
            "assigneeActor",
            "assigneeRole",
            "objective",
            "brief",
            "branch",
            "baseSha",
            "fileOwnership",
            "acceptanceSteps",
        ],
        CodingSessionTeamTransactionType::Report => &[
            "assignmentRef",
            "summary",
            "branch",
            "baseSha",
            "headSha",
            "files",
            "tests",
            "redBeforeGreen",
            "deviations",
            "residuals",
            "anomalies",
        ],
        // The decoder selects subtype-specific verdict keys before calling
        // this helper. An empty set remains fail-closed if a future caller
        // reaches this arm without doing so.
        CodingSessionTeamTransactionType::Verdict => &[],
        CodingSessionTeamTransactionType::Acknowledgement => {
            &["acknowledgedEventRef", "status", "note"]
        }
        CodingSessionTeamTransactionType::MissionCompleted => {
            &["assignmentRefs", "landedShas", "summary", "followUps"]
        }
        CodingSessionTeamTransactionType::MissionBlocked => &[
            "assignmentRefs",
            "summary",
            "blockers",
            "heldOn",
            "requiredAction",
        ],
        CodingSessionTeamTransactionType::Note => &["text", "refs"],
        CodingSessionTeamTransactionType::DecisionRequest => {
            &["question", "options", "heldOn", "blocks", "recommendation"]
        }
        CodingSessionTeamTransactionType::DecisionAnswer => &["requestRef", "choice", "note"],
    }
}
