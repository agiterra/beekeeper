//! The hand-written prose-join cases: attribution and delivery cases the
//! real translator cannot produce on its own (adjacent subagent pieces, a
//! second signer, two generations, provider `messageId`s, a lost `eventSeq`,
//! one event delivered twice). A sibling of
//! `transcript_prose_join_vectors_tests.rs`, which owns the translator cases,
//! the case list and the tests.

use serde_json::json;

use super::reference::reference_join;
use super::{
    messages_of, prose, result, subagent_prose, texts, tool_call_item, with_message_id, Case,
    Target, OTHER_SIGNER, SIGNER,
};

pub(super) fn own_prose_around_subagent_prose() -> Case {
    let name = "own-prose-around-subagent-prose";
    let mut at = Target::new(name, SIGNER, 1);
    at.publish(
        Some("turn-1"),
        vec![json!({ "kind": "user_prompt", "content": "Delegate it.", "steered": false })],
    );
    at.publish(
        Some("turn-1"),
        vec![
            prose("I will ask a subagent.\n\n"),
            prose("It knows the codebase."),
            subagent_prose("task-1", "Subagent: looking around."),
            prose("While it works, a note.\n\n"),
            prose("End of note."),
            subagent_prose("task-1", "Subagent: done."),
            result(),
        ],
    );
    let case = Case {
        name,
        source: "hand-written",
        description: "Adjacent pieces with different parentToolId never join. Own, subagent, own, subagent: four messages; the brief names the last of the agent's own, never the subagent's.",
        input: at.envelopes,
        session_status: Vec::new(),
        session_lease: Vec::new(),
    };
    let messages = messages_of(&case);
    assert_eq!(
        texts(&messages),
        vec![
            "I will ask a subagent.\n\nIt knows the codebase.",
            "Subagent: looking around.",
            "While it works, a note.\n\nEnd of note.",
            "Subagent: done.",
        ],
        "{name}"
    );
    assert_eq!(messages[1]["parentToolId"], "task-1", "{name}");
    let brief = reference_join(&case.input, &case.session_status, &case.session_lease).1;
    assert_eq!(
        brief[0]["latestAssistantFirstEventId"], messages[2]["firstEventId"],
        "{name}"
    );
    assert_eq!(
        brief[0]["latestAssistantEventId"], messages[2]["lastEventId"],
        "{name}"
    );
    case
}

pub(super) fn generations_share_turn_id() -> Case {
    let name = "two-generations-share-turn-id";
    let mut first = Target::new(name, SIGNER, 1);
    first.publish(
        Some("turn-1"),
        vec![
            prose("Generation one, cut off "),
            json!({ "kind": "interrupted" }),
        ],
    );
    let mut second = Target::new(name, SIGNER, 2);
    second.publish(
        Some("turn-1"),
        vec![prose("generation two starts fresh."), result()],
    );
    let mut input = first.envelopes;
    input.extend(second.envelopes);
    let case = Case {
        name,
        source: "hand-written",
        description: "Two generations that reuse a turnId are different targets: their prose never joins, though each is a lone piece with eventSeq 1.",
        input,
        session_status: Vec::new(),
        session_lease: Vec::new(),
    };
    assert_eq!(messages_of(&case).len(), 2, "{name}");
    case
}

pub(super) fn two_signers() -> Case {
    let name = "two-signers-one-target";
    let mut ours = Target::new(name, SIGNER, 1);
    ours.publish(Some("turn-1"), vec![prose("From the provider. ")]);
    let mut theirs = Target::new(name, OTHER_SIGNER, 1);
    theirs.lose_seq();
    theirs.publish(
        Some("turn-1"),
        vec![prose("From another signer."), result()],
    );
    ours.publish(Some("turn-1"), vec![result()]);
    let mut input = ours.envelopes;
    input.extend(theirs.envelopes);
    let case = Case {
        name,
        source: "hand-written",
        description: "The signer is part of the exact target: a piece from another signer on the same session never joins the provider's, even at the next eventSeq. Turn state (and so arriving) is read per exact target too.",
        input,
        session_status: Vec::new(),
        session_lease: Vec::new(),
    };
    let messages = messages_of(&case);
    assert_eq!(messages.len(), 2, "{name}");
    assert!(messages.iter().all(|m| m["arriving"] == false), "{name}");
    case
}

pub(super) fn distinct_message_ids() -> Case {
    let name = "distinct-message-ids";
    let mut at = Target::new(name, SIGNER, 1);
    at.publish(
        Some("turn-1"),
        vec![
            with_message_id(prose("Done."), "m1"),
            with_message_id(prose("Next"), "m2"),
            prose(" part, no id,"),
            with_message_id(prose(" same id again."), "m2"),
            tool_call_item("call-1"),
            prose("No id yet, "),
            with_message_id(prose("then m3."), "m3"),
            with_message_id(prose("Then m4."), "m4"),
            result(),
        ],
    );
    let case = Case {
        name,
        source: "hand-written",
        description: "When a piece and the message so far both carry a messageId and they differ, the piece starts a new message, joined with nothing inserted. A piece without one joins; a message takes the first messageId any of its pieces carries.",
        input: at.envelopes,
        session_status: Vec::new(),
        session_lease: Vec::new(),
    };
    assert_eq!(
        texts(&messages_of(&case)),
        vec![
            "Done.",
            "Next part, no id, same id again.",
            "No id yet, then m3.",
            "Then m4."
        ],
        "{name}"
    );
    case
}

pub(super) fn reasoning_joins_like_prose() -> Case {
    let name = "reasoning-joins-like-prose";
    let mut at = Target::new(name, SIGNER, 1);
    let reasoning = |text: &str| json!({ "kind": "reasoning", "text": text });
    at.publish(
        Some("turn-1"),
        vec![
            reasoning("Thinking, part one; "),
            reasoning("part two."),
            prose("An answer."),
            reasoning("More thought."),
            prose("A second answer."),
            result(),
        ],
    );
    let case = Case {
        name,
        source: "hand-written",
        description: "Reasoning pieces join by the same key; a reasoning item between two prose pieces ends the prose message, and prose ends a reasoning one.",
        input: at.envelopes,
        session_status: Vec::new(),
        session_lease: Vec::new(),
    };
    assert_eq!(
        texts(&messages_of(&case)),
        vec![
            "Thinking, part one; part two.",
            "An answer.",
            "More thought.",
            "A second answer."
        ],
        "{name}"
    );
    case
}

pub(super) fn sequence_gap() -> Case {
    let name = "event-seq-gap-is-not-a-boundary";
    let mut at = Target::new(name, SIGNER, 1);
    at.publish(Some("turn-1"), vec![prose("Before the gap, ")]);
    at.lose_seq();
    at.publish(Some("turn-1"), vec![prose("after the gap."), result()]);
    let case = Case {
        name,
        source: "hand-written",
        description: "A lost sequence number (permitted by NIP-CST) is not an item: the pieces on either side are adjacent and join.",
        input: at.envelopes,
        session_status: Vec::new(),
        session_lease: Vec::new(),
    };
    assert_eq!(
        texts(&messages_of(&case)),
        vec!["Before the gap, after the gap."],
        "{name}"
    );
    case
}

pub(super) fn duplicate_delivery() -> Case {
    let name = "duplicate-delivery-is-one-piece";
    let mut at = Target::new(name, SIGNER, 1);
    let ids = at.publish(
        Some("turn-1"),
        vec![
            prose("Backfill and live both carry this paragraph.\n\n"),
            prose("And this one only once."),
            result(),
        ],
    );
    // The relay backfill returned every event; the live REQ then delivered
    // the first piece again, verbatim — same event id, same eventSeq.
    let repeated = at
        .envelopes
        .iter()
        .find(|e| e["eventId"] == ids[0])
        .cloned()
        .expect("first piece published");
    at.envelopes.push(repeated);
    let case = Case {
        name,
        source: "hand-written",
        description: "One signed piece delivered twice verbatim (relay backfill plus a live subscription): a piece is identified by its event id, so the second copy is the same piece, not a second one. Readers deduplicate by event id before joining; the paragraph appears once.",
        input: at.envelopes,
        session_status: Vec::new(),
        session_lease: Vec::new(),
    };
    assert_eq!(
        case.input.iter().filter(|e| e["eventId"] == ids[0]).count(),
        2,
        "{name}: the input carries the piece twice"
    );
    let messages = messages_of(&case);
    assert_eq!(
        texts(&messages),
        vec!["Backfill and live both carry this paragraph.\n\nAnd this one only once."],
        "{name}"
    );
    assert_eq!(messages[0]["firstEventId"], ids[0], "{name}");
    assert_eq!(messages[0]["lastEventId"], ids[1], "{name}");
    case
}
