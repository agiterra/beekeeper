//! Machine-readable schemas for the `bee sessions` typed bodies.
//!
//! Every `bee sessions` write verb that takes `--body` takes a complete JSON
//! object whose shape is a Rust type in
//! [`buzz_core::coding_session_team_transaction`]. Before this module the only
//! way to learn that shape was to publish a wrong body and read the relay's
//! refusal: ledger 178(c) counted 15 CLI `user_error` records in the kettle
//! run's lead, 11 in its builder and 26 in its verifier, all of them a body
//! being discovered one `missing field` at a time, and ledger 179(c) recorded
//! seven seats of Andy's run doing the same thing — one of which resorted to
//! grepping strings out of the `bee` binary. One of those probes (V:63)
//! published a placeholder verdict (`summary: "s"`, `findings: ["f"]`) that the
//! relay stored for good.
//!
//! So this module answers the question offline, in two ways:
//!
//! 1. `--example` prints a complete, valid, minimal body and exits 0 without
//!    reaching the relay. The examples are **serialized from the same Rust
//!    types that validate a body**, never hand-typed JSON, and
//!    `body_schema_tests.rs` round-trips every one of them through the
//!    publication validator. An example that stopped being valid fails the
//!    build, not a live mission.
//! 2. A body that does not decode is refused with **every** required key for
//!    that verb plus the `--example` command, in one message, so the second
//!    attempt succeeds. The serde error alone names one missing field per
//!    attempt, which is what produced the sequences above.
//!
//! It also owns `--verifies` (ledger 178(d)): the fence that establishes a
//! verifier's tree reads the assignment's `baseSha` and nothing else
//! (`crates/buzz-session-provider/src/verification_input.rs:142,265`), so an
//! assignment whose prose names one commit and whose `baseSha` names another
//! sends the seat to the wrong tree. `--verifies <report event id>` takes the
//! commit from the report being verified instead of from the author's memory.

use serde::de::DeserializeOwned;
use serde_json::Value;

use buzz_core::coding_session_team_transaction::{
    CodingSessionTeamAcknowledgement, CodingSessionTeamAcknowledgementStatus,
    CodingSessionTeamAssignment, CodingSessionTeamDispositionDecision,
    CodingSessionTeamMissionBlocked, CodingSessionTeamMissionCompleted,
    CodingSessionTeamRefutationDecision, CodingSessionTeamReport, CodingSessionTeamTransactionTest,
    CodingSessionTeamTransactionTestOutcome, CodingSessionTeamTransactionType,
    CodingSessionTeamVerdict, ROLES_REQUIRING_VERIFICATION_INPUT,
};
use buzz_core::kind::KIND_CODING_SESSION_TEAM_TRANSACTION;
use buzz_sdk::coding_session_team_transaction::parse_coding_session_team_transaction;
use nostr::Event;

use crate::client::BuzzClient;
use crate::error::CliError;
use crate::validate::validate_lower_hex64;
use crate::{SessionsCmd, TeamTransactionWriteArgs};

/// A 64-hex placeholder that is obviously a placeholder.
///
/// Sixteen hex digits repeated four times: it passes every `eventId` shape
/// rule, and nobody will mistake it for a real event id.
const PLACEHOLDER_EVENT_ID: &str =
    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
/// A second 64-hex placeholder, for the bodies that must name two events.
const PLACEHOLDER_EVENT_ID_2: &str =
    "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";
/// A 40-hex placeholder git object id.
const PLACEHOLDER_SHA: &str = "0123456789abcdef0123456789abcdef01234567";

/// One named example body for one write verb.
pub struct TeamBodyExample {
    /// What this example is an example *of*: a role for an assignment, a
    /// subtype for a verdict, and `default` where a verb has one shape.
    pub label: &'static str,
    /// One sentence saying when a caller wants this variant rather than
    /// another. Printed to stderr beside the JSON, never into it.
    pub note: &'static str,
    /// The complete body, serialized from the constructed Rust value.
    pub value: Value,
}

/// Every example body for one write verb, in the order `--example` offers them.
///
/// The first entry is the default: what `--example` with no value prints.
///
/// # Errors
/// [`CliError::Other`] if a constructed body does not serialize, which is a
/// bug in this file rather than anything a caller did.
pub fn team_body_examples(
    transaction_type: CodingSessionTeamTransactionType,
) -> Result<Vec<TeamBodyExample>, CliError> {
    let examples = match transaction_type {
        CodingSessionTeamTransactionType::Assignment => {
            // The roles whose example must carry `baseSha` are read from the
            // rule that requires it, so a role added to
            // `ROLES_REQUIRING_VERIFICATION_INPUT` gets its example for free
            // instead of silently getting the builder's.
            let mut rows = vec![(
                "builder",
                "work that produces a revision; `baseSha` is optional and may stay null",
                to_value(assignment_example("builder", None))?,
            )];
            for role in ROLES_REQUIRING_VERIFICATION_INPUT {
                rows.push((
                    role,
                    "answers about one exact revision: `baseSha` is REQUIRED, and it is the \
                     only field the provider's fence reads",
                    to_value(assignment_example(role, Some(PLACEHOLDER_SHA)))?,
                ));
            }
            rows
        }
        CodingSessionTeamTransactionType::Report => vec![(
            "default",
            "evidence against one assignment; `tests` is the structured list, and prose \
             elsewhere never populates it",
            to_value(report_example())?,
        )],
        CodingSessionTeamTransactionType::Verdict => vec![
            (
                "refutation",
                "an active verifier's attempt to refute one report. `assignmentRef` is the \
                 assignmentRef of the report you are judging (its `reportRef`), NOT your own \
                 assignment as verifier — publishing your own assignment id here silently \
                 excludes the verdict from the fold. The relay is asked for `reportRef` before \
                 publishing and the write is refused if `assignmentRef` disagrees with what it \
                 reports",
                to_value(refutation_example())?,
            ),
            (
                "disposition",
                "a founder's or active lead's ruling that governs one report. `assignmentRef` \
                 is the assignmentRef of the report you are judging (its `reportRef`), NOT the \
                 assignmentRef of any assignment of your own. An APPROVING disposition with \
                 `requiredAction: null` SETTLES the assignment on its own — no acknowledgement \
                 is owed and none will be asked for. Anything the assignee must still do goes \
                 in `requiredAction`, or into a new assignment; prose in `summary` or \
                 `findings` is never read as an ask",
                to_value(disposition_example())?,
            ),
            (
                "disposition-requiring-an-action",
                "the same ruling when the assignee still owes something: `requiredAction` is \
                 the only field that asks, and its presence is what keeps the assignment \
                 awaiting the assignee's acknowledgement. `assignmentRef` is the assignmentRef \
                 of the report you are judging, not your own",
                to_value(disposition_requiring_an_action_example())?,
            ),
        ],
        CodingSessionTeamTransactionType::Acknowledgement => vec![(
            "default",
            "receipt of one governing record. Owed only where the disposition ASKED — an \
             approving disposition with `requiredAction: null` settles its assignment \
             without one, and publishing a receipt for it costs a turn and adds no fact",
            to_value(acknowledgement_example())?,
        )],
        CodingSessionTeamTransactionType::MissionCompleted => vec![(
            "default",
            "the mission's terminal; `assignmentRefs` must name at least one assignment",
            to_value(completion_example())?,
        )],
        CodingSessionTeamTransactionType::MissionBlocked => vec![(
            "default",
            "the mission has actually stopped; reach for `bee sessions note` when it has not",
            to_value(blocked_example())?,
        )],
        // The three remaining verbs take explicit flags rather than a JSON
        // body (`bee sessions note`, `bee sessions decide request|answer`), so
        // `--help` already lists every field they need and no example is
        // reachable here.
        CodingSessionTeamTransactionType::Note
        | CodingSessionTeamTransactionType::DecisionRequest
        | CodingSessionTeamTransactionType::DecisionAnswer => Vec::new(),
    };
    Ok(examples
        .into_iter()
        .map(|(label, note, value)| TeamBodyExample { label, note, value })
        .collect())
}

/// `--example [<label>]`: print one complete valid body and write nothing.
///
/// The JSON alone goes to stdout, so `bee sessions report --example > body.json`
/// produces a file the very next command accepts. The labels, the note and the
/// edit instruction go to stderr.
///
/// # Errors
/// [`CliError::Usage`] for a label no variant carries, and for a verb that
/// takes no JSON body.
pub fn print_team_body_example(
    command: &str,
    transaction_type: CodingSessionTeamTransactionType,
    requested: &str,
) -> Result<(), CliError> {
    let examples = team_body_examples(transaction_type)?;
    let Some(default) = examples.first() else {
        return Err(CliError::Usage(format!(
            "`bee sessions {command}` takes no --body JSON, so it has no example body: run \
             `bee sessions {command} --help` for the flags it does take"
        )));
    };
    let chosen = if requested == DEFAULT_EXAMPLE_LABEL {
        default
    } else {
        examples
            .iter()
            .find(|example| example.label == requested)
            .ok_or_else(|| {
                CliError::Usage(format!(
                    "no {} example named {requested:?}: this verb offers {}",
                    transaction_type.as_str(),
                    label_list(&examples)
                ))
            })?
    };
    let rendered = serde_json::to_string_pretty(&chosen.value)
        .map_err(|error| CliError::Other(format!("example body did not render: {error}")))?;
    eprintln!(
        "{} example ({}): {}",
        transaction_type.as_str(),
        chosen.label,
        chosen.note
    );
    if examples.len() > 1 {
        eprintln!(
            "examples for this verb: {} — `bee sessions {command} --example <label>`",
            label_list(&examples)
        );
    }
    eprintln!(
        "every key above is required; replace the placeholder ids and text, keep the nulls you \
         have nothing to say for, then publish with `--body @<file>`"
    );
    println!("{rendered}");
    Ok(())
}

/// The value `--example` carries when the caller passed the flag on its own.
pub const DEFAULT_EXAMPLE_LABEL: &str = "default";

/// Every key one verb's body must carry, in one line.
///
/// Read off the default example rather than restated, so the list cannot drift
/// from the type that validates the body.
fn required_keys(examples: &[TeamBodyExample]) -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();
    for example in examples {
        if let Some(object) = example.value.as_object() {
            for key in object.keys() {
                if !keys.iter().any(|seen| seen == key) {
                    keys.push(key.clone());
                }
            }
        }
    }
    keys
}

/// `builder, verifier, runner`, for a message offering the choice.
fn label_list(examples: &[TeamBodyExample]) -> String {
    examples
        .iter()
        .map(|example| example.label)
        .collect::<Vec<_>>()
        .join(", ")
}

/// The refusal a body that does not decode gets: the serde error, **every**
/// required key, and the command that prints a valid body.
///
/// One message, because the failure mode being fixed here is a seat that
/// publishes N wrong bodies to learn N field names (ledger 178(c)).
fn shape_body_error(
    command: &str,
    transaction_type: CodingSessionTeamTransactionType,
    detail: &str,
) -> CliError {
    let label = transaction_type.as_str();
    let (keys, variants) = match team_body_examples(transaction_type) {
        Ok(examples) => {
            let variants = if examples.len() > 1 {
                format!(
                    " Variants: {} (`bee sessions {command} --example <label>`).",
                    label_list(&examples)
                )
            } else {
                String::new()
            };
            (required_keys(&examples), variants)
        }
        Err(error) => return error,
    };
    if keys.is_empty() {
        return CliError::Usage(format!("invalid {label} body: {detail}"));
    }
    let article = if label.starts_with('a') { "An" } else { "A" };
    CliError::Usage(format!(
        "invalid {label} body: {detail}. {article} {label} body must carry every one of these \
         keys, nullable ones as explicit JSON null: {}.{variants} Run `bee sessions {command} \
         --example` for a complete valid body and edit it — do not discover this shape by \
         publishing.",
        keys.join(", ")
    ))
}

/// Decode `value` as this verb's body, shaping any failure into the full-schema
/// refusal.
fn check_body(
    command: &str,
    transaction_type: CodingSessionTeamTransactionType,
    value: Value,
) -> Result<(), CliError> {
    fn decode<T: DeserializeOwned>(value: Value) -> Result<(), String> {
        serde_json::from_value::<T>(value)
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
    let decoded = match transaction_type {
        CodingSessionTeamTransactionType::Assignment => {
            decode::<CodingSessionTeamAssignment>(value)
        }
        CodingSessionTeamTransactionType::Report => decode::<CodingSessionTeamReport>(value),
        CodingSessionTeamTransactionType::Verdict => decode::<CodingSessionTeamVerdict>(value),
        CodingSessionTeamTransactionType::Acknowledgement => {
            decode::<CodingSessionTeamAcknowledgement>(value)
        }
        CodingSessionTeamTransactionType::MissionCompleted => {
            decode::<CodingSessionTeamMissionCompleted>(value)
        }
        CodingSessionTeamTransactionType::MissionBlocked => {
            decode::<CodingSessionTeamMissionBlocked>(value)
        }
        // Unreachable from the six verbs that route here; refused rather than
        // silently accepted if a future verb forgets to add its arm.
        CodingSessionTeamTransactionType::Note
        | CodingSessionTeamTransactionType::DecisionRequest
        | CodingSessionTeamTransactionType::DecisionAnswer => {
            Err("this verb takes explicit flags, not a JSON body".to_owned())
        }
    };
    decoded.map_err(|detail| shape_body_error(command, transaction_type, &detail))
}

/// Read `--body` from a literal, a `@path`, or `-` for stdin.
///
/// Deliberately a copy of the read in [`super::operations`] rather than a call
/// into it: this check runs *before* that module, and stdin can only be
/// consumed once, so the value read here is what is handed on.
fn read_body_argument(input: &str) -> Result<Value, CliError> {
    use std::io::Read;
    let raw = if input == "-" {
        let mut raw = String::new();
        std::io::stdin()
            .read_to_string(&mut raw)
            .map_err(|error| CliError::Other(format!("failed to read stdin: {error}")))?;
        raw
    } else if let Some(path) = input.strip_prefix('@') {
        std::fs::read_to_string(path)
            .map_err(|error| CliError::Usage(format!("failed to read body file {path}: {error}")))?
    } else {
        input.to_owned()
    };
    serde_json::from_str(&raw).map_err(|error| {
        CliError::Usage(format!(
            "invalid --body JSON: {error}. The value is a JSON object, `@path` to a file \
             holding one, or `-` for stdin."
        ))
    })
}

/// Fill an assignment's `baseSha` from the report it is verifying.
///
/// The provider's fence reads `baseSha` and only `baseSha`, so the commit a
/// verifier's tree is established at comes from this field and never from the
/// objective's prose (ledger 178(d)). Reading it off the report removes the one
/// step a human or a model can get wrong.
async fn apply_verifies(
    client: &BuzzClient,
    args: &TeamTransactionWriteArgs,
    report_ref: &str,
    body: &mut Value,
) -> Result<(), CliError> {
    validate_lower_hex64("--verifies", report_ref)?;
    let head_sha = fetch_report_head_sha(client, &args.channel, &args.session_ref, report_ref)
        .await?
        .ok_or_else(|| {
            CliError::Usage(format!(
                "report {report_ref} carries no headSha, so it does not name a revision to \
                 verify: ask its author for a corrected report, or set baseSha in --body \
                 yourself"
            ))
        })?;
    merge_base_sha(body, &head_sha, report_ref)
}

/// Write `head_sha` into the body's `baseSha`, or refuse a disagreement.
///
/// An equal value is left alone: `--verifies` and an explicit `baseSha` saying
/// the same thing is agreement, not a conflict. A *different* value is refused
/// rather than silently overwritten, because both spellings are somebody's
/// stated intent and only one of them can be the commit the seat judges.
fn merge_base_sha(body: &mut Value, head_sha: &str, report_ref: &str) -> Result<(), CliError> {
    let object = body.as_object_mut().ok_or_else(|| {
        CliError::Usage("--body must be a JSON object to fill baseSha into".to_owned())
    })?;
    match object.get("baseSha").and_then(Value::as_str) {
        Some(existing) if existing.eq_ignore_ascii_case(head_sha) => {}
        Some(existing) => {
            return Err(CliError::Usage(format!(
                "--body baseSha is {existing} but report {report_ref} reports headSha \
                 {head_sha}: the provider's fence establishes the verifier's tree at baseSha, so \
                 these disagreeing means the seat would judge a different commit. Drop baseSha \
                 from --body to take the report's, or drop --verifies to insist on yours."
            )));
        }
        None => {
            object.insert("baseSha".to_owned(), Value::String(head_sha.to_owned()));
        }
    }
    Ok(())
}

/// One stored report, by event id, inside this session.
///
/// Shared by `--verifies` (reads `headSha`) and the verdict `assignmentRef`
/// check (reads `assignmentRef`), so both read the same record the same way.
async fn fetch_report(
    client: &BuzzClient,
    channel: &str,
    session_ref: &str,
    report_ref: &str,
) -> Result<CodingSessionTeamReport, CliError> {
    let rows = client
        .query_all(serde_json::json!({
            "ids": [report_ref],
            "kinds": [KIND_CODING_SESSION_TEAM_TRANSACTION],
            "#h": [channel],
        }))
        .await?;
    let row = rows.first().ok_or_else(|| {
        CliError::NotFound(format!(
            "no kind:{KIND_CODING_SESSION_TEAM_TRANSACTION} record {report_ref} in channel \
             {channel}: check the id with `bee sessions operation list`"
        ))
    })?;
    let event: Event = serde_json::from_value(row.clone())
        .map_err(|error| CliError::Other(format!("relay returned malformed event: {error}")))?;
    let payload = parse_coding_session_team_transaction(&event)
        .map_err(|error| CliError::Usage(format!("{report_ref} is not a team record: {error}")))?;
    if payload.session_ref != session_ref {
        return Err(CliError::Usage(format!(
            "{report_ref} belongs to session {} rather than --session-ref {session_ref}",
            payload.session_ref
        )));
    }
    match payload.body {
        buzz_core::coding_session_team_transaction::CodingSessionTeamTransactionBody::Report(
            report,
        ) => Ok(report),
        other => Err(CliError::Usage(format!(
            "{report_ref} names a {} record; it must name the report being judged or verified",
            other.transaction_type().as_str()
        ))),
    }
}

/// The `headSha` of one stored report, by event id, inside this session.
async fn fetch_report_head_sha(
    client: &BuzzClient,
    channel: &str,
    session_ref: &str,
    report_ref: &str,
) -> Result<Option<String>, CliError> {
    Ok(fetch_report(client, channel, session_ref, report_ref)
        .await?
        .head_sha)
}

/// Refuse a verdict body whose `assignmentRef` disagrees with the
/// `assignmentRef` of the report it judges (`reportRef`).
///
/// A verdict names two events: the report under judgment and the assignment
/// that report was written against. They must be the SAME assignment the
/// report names — a verdict's own author has an assignment too (the verifier
/// is itself assigned to verify), and confusing the two silently excludes the
/// verdict from `validate_causal_types`
/// (`coding_session_team_transaction_fold_defects.rs`) as a
/// `WrongTypeReference`, so the verdict is folded out and never counted. This
/// is a pure comparison over already-fetched strings so it is testable
/// without a relay; [`verify_verdict_assignment_ref`] is the network-reaching
/// call site that fetches the report and hands its `assignmentRef` in.
///
/// # Errors
/// [`CliError::Usage`] naming the report's `assignmentRef` so the fix is one
/// copy-paste, when `verdict_assignment_ref` disagrees.
pub fn check_verdict_assignment_ref(
    verdict_assignment_ref: &str,
    report_ref: &str,
    report_assignment_ref: &str,
) -> Result<(), CliError> {
    if verdict_assignment_ref == report_assignment_ref {
        return Ok(());
    }
    Err(CliError::Usage(format!(
        "--body assignmentRef is {verdict_assignment_ref} but report {report_ref} (named by \
         --body reportRef) reports assignmentRef {report_assignment_ref}: a verdict's \
         assignmentRef must name the assignment the report you are judging was written \
         against, not your own assignment. Set --body assignmentRef to \
         {report_assignment_ref}."
    )))
}

/// Fetch the report a verdict body names and refuse a disagreeing
/// `assignmentRef` before publishing.
///
/// # Errors
/// Whatever [`fetch_report`] returns, or [`check_verdict_assignment_ref`]'s
/// refusal.
async fn verify_verdict_assignment_ref(
    client: &BuzzClient,
    args: &TeamTransactionWriteArgs,
    body: &Value,
) -> Result<(), CliError> {
    let assignment_ref = body_string(body, "assignmentRef")?;
    let report_ref = body_string(body, "reportRef")?;
    validate_lower_hex64("body reportRef", &report_ref)?;
    let report = fetch_report(client, &args.channel, &args.session_ref, &report_ref).await?;
    check_verdict_assignment_ref(&assignment_ref, &report_ref, &report.assignment_ref)
}

/// Read one required string field out of a checked `--body`.
///
/// Called only after [`check_body`] decoded the same value into its typed
/// shape, so the field is guaranteed present as a string; this stays a
/// `Result` rather than an `expect()` because that guarantee lives in another
/// function, not the type here.
fn body_string(body: &Value, key: &str) -> Result<String, CliError> {
    body.get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| CliError::Other(format!("checked --body is missing string field {key}")))
}

/// Whether this `bee sessions` invocation is a `--example` request.
///
/// Returns the subcommand's name, the body type it publishes, and the label the
/// caller asked for. Read by `run` **before** the key gate: the command that
/// teaches a seat the wire must not itself require the wire (the same reason
/// `bee sessions explain` is dispatched there).
#[must_use]
pub fn example_request(
    command: &SessionsCmd,
) -> Option<(&'static str, CodingSessionTeamTransactionType, String)> {
    let (name, transaction_type, args) = match command {
        SessionsCmd::Assign(args) => ("assign", CodingSessionTeamTransactionType::Assignment, args),
        SessionsCmd::Report(args) => ("report", CodingSessionTeamTransactionType::Report, args),
        SessionsCmd::Verdict(args) => ("verdict", CodingSessionTeamTransactionType::Verdict, args),
        SessionsCmd::Acknowledge(args) => (
            "acknowledge",
            CodingSessionTeamTransactionType::Acknowledgement,
            args,
        ),
        SessionsCmd::Complete(args) => (
            "complete",
            CodingSessionTeamTransactionType::MissionCompleted,
            args,
        ),
        SessionsCmd::Block(args) => (
            "block",
            CodingSessionTeamTransactionType::MissionBlocked,
            args,
        ),
        _ => return None,
    };
    args.example
        .clone()
        .map(|requested| (name, transaction_type, requested))
}

/// Every `bee sessions` verb that takes `--body`, with its schema help wired in.
///
/// Handles `--example` and `--verifies`, refuses a body that does not decode
/// with the whole schema, and hands the checked body to
/// [`super::operations::cmd_write`] as a literal so nothing is read twice.
///
/// # Errors
/// [`CliError::Usage`] for a missing envelope flag, an undecodable body, or a
/// `--verifies` disagreement; otherwise whatever the publish path returns.
pub async fn dispatch_write(
    client: &BuzzClient,
    command: &str,
    mut args: TeamTransactionWriteArgs,
    transaction_type: CodingSessionTeamTransactionType,
) -> Result<(), CliError> {
    if let Some(requested) = args.example.clone() {
        return print_team_body_example(command, transaction_type, &requested);
    }
    require_envelope(command, &args)?;
    let mut body = read_body_argument(&args.body)?;
    if let Some(report_ref) = args.verifies.clone() {
        if transaction_type != CodingSessionTeamTransactionType::Assignment {
            // Ledger 213(e): six verbs share these args, so clap parses
            // `--verifies` on all of them. A flag that is accepted and
            // changes nothing is a lie in the interface — this names where
            // it applies and where the id the caller meant actually goes.
            return Err(CliError::Usage(format!(
                "--verifies applies to `sessions assign`; a report/verdict names its assignment \
                 in the body's `assignmentRef`. It fills an assignment's baseSha from a \
                 report's headSha, and `bee sessions {command}` publishes no baseSha"
            )));
        }
        apply_verifies(client, &args, &report_ref, &mut body).await?;
    }
    check_body(command, transaction_type, body.clone())?;
    if transaction_type == CodingSessionTeamTransactionType::Verdict {
        verify_verdict_assignment_ref(client, &args, &body).await?;
    }
    args.body = serde_json::to_string(&body)
        .map_err(|error| CliError::Other(format!("checked body did not render: {error}")))?;
    super::operations::cmd_write(client, args, transaction_type).await
}

/// Refuse a write whose envelope flags are absent.
///
/// `--channel`, `--session-ref`, `--genesis` and `--body` are required for every
/// write and optional only so that `--example` can run with none of them. An
/// empty value therefore means the caller omitted the flag, and saying which
/// one is better than the coordinate validator's "invalid UUID".
fn require_envelope(command: &str, args: &TeamTransactionWriteArgs) -> Result<(), CliError> {
    for (flag, value) in [
        ("--channel", &args.channel),
        ("--session-ref", &args.session_ref),
        ("--genesis", &args.genesis),
        ("--body", &args.body),
    ] {
        if value.trim().is_empty() {
            return Err(CliError::Usage(format!(
                "{flag} is required to publish: run `bee sessions {command} --help` for the \
                 envelope, or `bee sessions {command} --example` to see the body it takes"
            )));
        }
    }
    Ok(())
}

/// One constructed value as JSON, reporting a serialization bug rather than
/// panicking on it.
fn to_value<T: serde::Serialize>(value: T) -> Result<Value, CliError> {
    serde_json::to_value(value)
        .map_err(|error| CliError::Other(format!("example body did not serialize: {error}")))
}

/// The assignment example, parameterized by the one field whose requiredness
/// depends on the role.
fn assignment_example(role: &str, base_sha: Option<&str>) -> CodingSessionTeamAssignment {
    CodingSessionTeamAssignment {
        assignee_actor: PLACEHOLDER_EVENT_ID.to_owned(),
        assignee_role: role.to_owned(),
        objective: format!("one sentence naming the outcome this {role} owns"),
        brief: "the complete instructions: what to do, the constraints that bind it, and what \
                evidence the report must carry"
            .to_owned(),
        // The host allocates each seat's worktree branch; an assignment names
        // a branch only when the work must land on a particular remote name,
        // which the seat then pushes to with `HEAD:refs/heads/<name>`.
        branch: None,
        base_sha: base_sha.map(str::to_owned),
        file_ownership: vec!["crates/example-crate/src/".to_owned()],
        acceptance_steps: vec!["cargo test -p example-crate".to_owned()],
    }
}

/// The report example, with one structured test so the `tests` shape is visible.
fn report_example() -> CodingSessionTeamReport {
    CodingSessionTeamReport {
        assignment_ref: PLACEHOLDER_EVENT_ID.to_owned(),
        summary: "one sentence saying what the work produced".to_owned(),
        branch: Some("session-example-builder-1".to_owned()),
        base_sha: Some(PLACEHOLDER_SHA.to_owned()),
        head_sha: Some(PLACEHOLDER_SHA.to_owned()),
        files: vec!["crates/example-crate/src/lib.rs".to_owned()],
        tests: vec![CodingSessionTeamTransactionTest {
            name: "unit tests".to_owned(),
            command: "cargo test -p example-crate".to_owned(),
            outcome: CodingSessionTeamTransactionTestOutcome::Passed,
            evidence: Some("exit 0; 12 passed, 0 failed".to_owned()),
        }],
        red_before_green: Some(true),
        deviations: vec![],
        residuals: vec![],
        anomalies: vec![],
    }
}

/// The verifier's half of a verdict.
fn refutation_example() -> CodingSessionTeamVerdict {
    CodingSessionTeamVerdict::Refutation {
        assignment_ref: PLACEHOLDER_EVENT_ID.to_owned(),
        report_ref: PLACEHOLDER_EVENT_ID_2.to_owned(),
        decision: CodingSessionTeamRefutationDecision::NotRefuted,
        summary: "one sentence saying what was attempted and what it showed".to_owned(),
        findings: vec!["one finding, with the file:line or the run that proves it".to_owned()],
        required_action: None,
    }
}

/// The lead's or founder's half of a verdict.
fn disposition_example() -> CodingSessionTeamVerdict {
    CodingSessionTeamVerdict::Disposition {
        assignment_ref: PLACEHOLDER_EVENT_ID.to_owned(),
        report_ref: PLACEHOLDER_EVENT_ID_2.to_owned(),
        refutation_ref: None,
        decision: CodingSessionTeamDispositionDecision::Approve,
        summary: "one sentence saying what is being ruled and why".to_owned(),
        findings: vec!["one finding, with the file:line or the run that proves it".to_owned()],
        required_action: None,
    }
}

/// The same ruling with the one field that asks the assignee for something.
///
/// `requiredAction` is read **mechanically**: present and non-blank is an ask,
/// absent is not, and no prose anywhere else in the body changes that
/// (`buzz_core::coding_session_team_transaction::approving_disposition_asks_nothing`).
/// A lead who writes "approved; please push" in `summary` and leaves this
/// field null has asked for nothing under the contract, and the assignment
/// settles — which is why the example exists beside the plain one.
fn disposition_requiring_an_action_example() -> CodingSessionTeamVerdict {
    CodingSessionTeamVerdict::Disposition {
        assignment_ref: PLACEHOLDER_EVENT_ID.to_owned(),
        report_ref: PLACEHOLDER_EVENT_ID_2.to_owned(),
        refutation_ref: None,
        decision: CodingSessionTeamDispositionDecision::ApproveWithNotes,
        summary: "one sentence saying what is being ruled and why".to_owned(),
        findings: vec!["one finding, with the file:line or the run that proves it".to_owned()],
        required_action: Some(
            "the one bounded thing the assignee must still do, or answer, before this              assignment settles"
                .to_owned(),
        ),
    }
}

/// The acknowledgement example.
fn acknowledgement_example() -> CodingSessionTeamAcknowledgement {
    CodingSessionTeamAcknowledgement {
        acknowledged_event_ref: PLACEHOLDER_EVENT_ID.to_owned(),
        status: CodingSessionTeamAcknowledgementStatus::Received,
        note: None,
    }
}

/// The `mission.completed` example.
fn completion_example() -> CodingSessionTeamMissionCompleted {
    CodingSessionTeamMissionCompleted {
        assignment_refs: vec![PLACEHOLDER_EVENT_ID.to_owned()],
        landed_shas: vec![PLACEHOLDER_SHA.to_owned()],
        summary: "one sentence saying what the mission delivered".to_owned(),
        follow_ups: vec![],
    }
}

/// The `mission.blocked` example.
fn blocked_example() -> CodingSessionTeamMissionBlocked {
    CodingSessionTeamMissionBlocked {
        assignment_refs: vec![PLACEHOLDER_EVENT_ID.to_owned()],
        summary: "one sentence saying what stopped".to_owned(),
        blockers: vec!["the exact thing that is missing".to_owned()],
        held_on: None,
        required_action: "the one bounded action that moves the mission again".to_owned(),
    }
}

#[cfg(test)]
#[path = "body_schema_tests.rs"]
mod tests;
