//! Prose join for the first-turn brief.
//!
//! A streamed answer reaches the wire as several kind-44225 `assistant_text`
//! pieces (size cuts today, paragraph cuts with
//! `BEEKEEPER_CSP_TRANSCRIPT_PARAGRAPH_FLUSH`). NIP-CST amendment 3, paragraph
//! **Join key**, says every reader puts them back into the one message the
//! agent wrote; `conformance/transcript-prose-join/CONTRACT.md` is that rule in
//! executable form. The brief reads it to name a turn's latest own message by
//! both ends, and to spend one evidence slot per message rather than one per
//! paragraph.

use std::collections::{BTreeMap, HashSet};

use serde_json::Value;

use super::{BriefTurn, CodingSessionContextHistoryItem};
use crate::coding_session_command::coding_session_target_key;

/// Where one history item sits in a joined prose run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BriefProsePiece {
    /// Not `assistant_text` or `reasoning`.
    NotProse,
    /// A second delivery of an event id already seen earlier in the history
    /// (contract rule 0). The brief observes nothing from it.
    Repeat,
    /// The first piece of a message: its identity.
    First,
    /// A later piece of the message whose first piece is at this history
    /// offset.
    Continues(usize),
}

struct OpenRun<'a> {
    first: usize,
    kind: &'a str,
    turn_id: Option<&'a str>,
    parent_tool_id: Option<&'a str>,
    message_id: Option<&'a str>,
}

/// Classify every history item against the join key.
///
/// Within one exact target (author + driver + instanceId + sessionId +
/// generation), items are walked in `eventSeq` order, never history order.
/// Two adjacent prose items of the same kind join when they share `turnId`
/// and `parentToolId` and do not carry differing `messageId`s; any other item
/// between them ends the message. A missing sequence number is not an item.
pub(super) fn brief_prose_pieces(
    history: &[CodingSessionContextHistoryItem],
) -> Vec<BriefProsePiece> {
    let mut pieces = vec![BriefProsePiece::NotProse; history.len()];
    let mut seen = HashSet::with_capacity(history.len());
    let mut targets = BTreeMap::<(&str, String), Vec<usize>>::new();
    for (offset, item) in history.iter().enumerate() {
        if !seen.insert(item.event_id.as_str()) {
            pieces[offset] = BriefProsePiece::Repeat;
            continue;
        }
        targets
            .entry((
                item.author.as_str(),
                coding_session_target_key(&item.target),
            ))
            .or_default()
            .push(offset);
    }
    for offsets in targets.values_mut() {
        offsets.sort_by(|&left, &right| {
            history[left]
                .event_seq
                .cmp(&history[right].event_seq)
                .then_with(|| history[left].event_id.cmp(&history[right].event_id))
        });
        let mut open: Option<OpenRun<'_>> = None;
        for &offset in offsets.iter() {
            let item = &history[offset];
            let kind = item.item_kind.as_str();
            if !matches!(kind, "assistant_text" | "reasoning") {
                open = None;
                continue;
            }
            let turn_id = item.turn_id.as_deref();
            let parent_tool_id = content_str(&item.content, "parentToolId");
            let message_id = content_str(&item.content, "messageId");
            match open.as_mut() {
                Some(run)
                    if run.kind == kind
                        && run.turn_id == turn_id
                        && run.parent_tool_id == parent_tool_id
                        && !matches!(
                            (run.message_id, message_id),
                            (Some(current), Some(next)) if current != next
                        ) =>
                {
                    // Rule 5: a message's `messageId` is the first one any of
                    // its pieces carries.
                    if run.message_id.is_none() {
                        run.message_id = message_id;
                    }
                    pieces[offset] = BriefProsePiece::Continues(run.first);
                }
                _ => {
                    pieces[offset] = BriefProsePiece::First;
                    open = Some(OpenRun {
                        first: offset,
                        kind,
                        turn_id,
                        parent_tool_id,
                        message_id,
                    });
                }
            }
        }
    }
    pieces
}

/// `parentToolId` of a prose item, or `None` for the agent's own prose.
pub(super) fn brief_parent_tool_id(item: &CodingSessionContextHistoryItem) -> Option<&str> {
    content_str(&item.content, "parentToolId")
}

fn content_str<'a>(content: &'a Value, key: &str) -> Option<&'a str> {
    content.get(key).and_then(Value::as_str)
}

/// The latest own prose message of one brief turn, named by both ends.
pub(super) struct BriefAssistantMessage {
    event_seq: u64,
    pub(super) last_event_id: String,
    pub(super) first_event_id: String,
}

impl BriefTurn {
    /// Keep the own prose piece with the highest `eventSeq`: it ends the
    /// turn's latest own message, whose first piece is `run_first_event_id`.
    pub(super) fn observe_own_prose(
        &mut self,
        item: &CodingSessionContextHistoryItem,
        run_first_event_id: Option<&str>,
    ) {
        if self
            .latest_assistant
            .as_ref()
            .is_some_and(|latest| latest.event_seq >= item.event_seq)
        {
            return;
        }
        self.latest_assistant = Some(BriefAssistantMessage {
            event_seq: item.event_seq,
            last_event_id: item.event_id.clone(),
            first_event_id: run_first_event_id
                .unwrap_or(item.event_id.as_str())
                .to_owned(),
        });
    }
}
