//! SV-29 rewind cuts: what a rewound generation's context package forgets.
//!
//! A `session.rewind` with cut `(g, s)` detaches generation N and opens N+1
//! as a new conversation seeded from the record — the record *as it was
//! before the rewound turns*. So every package projected from then on drops:
//!
//! - the items of generation `g` of that execution with `eventSeq > s`;
//! - the kind:44220 commands, and the turn-stage kind:44224 receipts, whose
//!   `commandId` is on a dropped `user_prompt` item — they carry the rewound
//!   prompts' text, which would otherwise come back through the inbox.
//!
//! Nothing is deleted: the relay keeps every fact, and the rewound turns stay
//! readable to people. Cuts come from two places: the chain's own rewind
//! receipts (so later refreshes and sibling seats see them), and the request
//! (the generation a rewind is opening has no receipt yet).
//!
//! Iteration 1 only rewinds turns of the generation being rewound, so a cut
//! never names an earlier generation and no generation leaves the lineage
//! entirely; a cut naming one would still apply to that generation alone.

use std::collections::HashSet;

use beekeeper_core::coding_session_command::CodingSessionTarget;
use beekeeper_core::coding_session_lifecycle_command::{
    decode_coding_session_lifecycle_command, CodingSessionLifecycleAction,
};
use beekeeper_core::coding_session_payload::{
    decode_coding_session_lifecycle_receipt, TranscriptEnvelope,
};
use nostr::Event;
use serde_json::Value;

use super::{record_note, ContextExecutionFacts};

/// One rewind cut: keep `target`'s generation only through `after_seq`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextCut {
    /// The execution and the generation the cut falls in.
    pub target: CodingSessionTarget,
    /// The last `eventSeq` of that generation kept.
    pub after_seq: u64,
}

fn same_execution(left: &CodingSessionTarget, right: &CodingSessionTarget) -> bool {
    left.driver == right.driver
        && left.instance_id == right.instance_id
        && left.session_id == right.session_id
}

/// The cuts the chain's own rewind receipts prove.
fn proved_cuts(executions: &[ContextExecutionFacts]) -> Vec<ContextCut> {
    let mut cuts = Vec::new();
    for generation in executions
        .iter()
        .flat_map(|execution| &execution.generations)
    {
        let rewinds =
            decode_coding_session_lifecycle_command(&generation.lifecycle_command.content)
                .is_ok_and(|command| {
                    matches!(
                        command.action,
                        CodingSessionLifecycleAction::SessionRewind { .. }
                    )
                });
        if !rewinds {
            continue;
        }
        let Ok(receipt) = decode_coding_session_lifecycle_receipt(&generation.receipt.content)
        else {
            continue;
        };
        let (Some(rewind), Some(session)) = (receipt.rewind, receipt.session) else {
            continue;
        };
        cuts.push(ContextCut {
            target: CodingSessionTarget {
                generation: rewind.cut_generation,
                ..session
            },
            after_seq: rewind.cut_after_seq,
        });
    }
    cuts
}

/// The `commandId` of a 44220 or a 44224, and the target it names.
fn command_and_target(event: &Event) -> Option<(String, CodingSessionTarget)> {
    let value: Value = serde_json::from_str(&event.content).ok()?;
    let command_id = value.get("commandId")?.as_str()?.to_owned();
    let target = value
        .get("target")
        .or_else(|| value.get("session"))
        .cloned()
        .and_then(|target| serde_json::from_value(target).ok())?;
    Some((command_id, target))
}

/// Apply every cut (module docs). Returns nothing; what was dropped is
/// disclosed in `notes`.
pub(super) fn apply_rewind_cuts(
    requested: &[ContextCut],
    executions: &mut [ContextExecutionFacts],
    turn_commands: &mut Vec<Event>,
    turn_receipts: &mut Vec<Event>,
    notes: &mut Vec<String>,
) {
    let mut cuts = proved_cuts(executions);
    cuts.extend(requested.iter().cloned());
    if cuts.is_empty() {
        return;
    }
    // (execution, commandId) pairs of rewound prompts.
    let mut dropped_prompts: Vec<(CodingSessionTarget, String)> = Vec::new();
    let mut dropped_items = 0usize;
    for cut in &cuts {
        for execution in executions.iter_mut() {
            for generation in &mut execution.generations {
                let mut kept = Vec::with_capacity(generation.transcript.len());
                for event in generation.transcript.drain(..) {
                    let Ok(envelope) = serde_json::from_str::<TranscriptEnvelope>(&event.content)
                    else {
                        kept.push(event);
                        continue;
                    };
                    let in_cut = same_execution(&envelope.session, &cut.target)
                        && envelope.session.generation == cut.target.generation;
                    if !in_cut || envelope.event_seq <= cut.after_seq {
                        kept.push(event);
                        continue;
                    }
                    dropped_items += 1;
                    if envelope.item.get("kind").and_then(Value::as_str) == Some("user_prompt") {
                        if let Some(command_id) =
                            envelope.item.get("commandId").and_then(Value::as_str)
                        {
                            dropped_prompts.push((envelope.session.clone(), command_id.to_owned()));
                        }
                    }
                }
                generation.transcript = kept;
            }
        }
    }
    let dropped: HashSet<(String, String, String, String)> = dropped_prompts
        .iter()
        .map(|(target, command_id)| {
            (
                target.driver.clone(),
                target.instance_id.clone(),
                target.session_id.clone(),
                command_id.clone(),
            )
        })
        .collect();
    let rewound = |event: &Event| {
        command_and_target(event).is_some_and(|(command_id, target)| {
            dropped.contains(&(
                target.driver,
                target.instance_id,
                target.session_id,
                command_id,
            ))
        })
    };
    let commands_before = turn_commands.len();
    turn_commands.retain(|event| !rewound(event));
    let receipts_before = turn_receipts.len();
    turn_receipts.retain(|event| !rewound(event));
    let dropped_traffic =
        (commands_before - turn_commands.len()) + (receipts_before - turn_receipts.len());
    if dropped_items > 0 || dropped_traffic > 0 {
        record_note(
            notes,
            format!(
                "Rewound: {dropped_items} transcript items and {dropped_traffic} turn commands \
                 and receipts after the rewind cut are not in this package"
            ),
        );
    }
}
