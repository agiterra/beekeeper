//! Paragraph flushing: the agent's prose reaches the record a paragraph at a
//! time, at a blank line outside an open code fence, and never shorter than
//! [`MIN_PARAGRAPH_FLUSH_BYTES`] unless a size, narrative or turn boundary
//! forces it.

use super::*;

/// A translator with paragraph flushing switched on, as the provider builds
/// it when `BEEKEEPER_CSP_TRANSCRIPT_PARAGRAPH_FLUSH` is set.
fn on(include_thoughts: bool) -> TranscriptTranslator {
    TranscriptTranslator::new(include_thoughts).with_paragraph_flush(true)
}

fn chunk(text: &str) -> Value {
    json!({
        "sessionUpdate": "agent_message_chunk",
        "content": { "type": "text", "text": text },
    })
}

fn thought(text: &str) -> Value {
    json!({
        "sessionUpdate": "agent_thought_chunk",
        "content": { "type": "text", "text": text },
    })
}

fn subagent_chunk(parent: &str, text: &str) -> Value {
    json!({
        "sessionUpdate": "agent_message_chunk",
        "content": { "type": "text", "text": text },
        "_meta": { "claudeCode": { "parentToolUseId": parent } },
    })
}

fn tool_call(id: &str, name: &str, input: Value) -> Value {
    json!({
        "sessionUpdate": "tool_call",
        "toolCallId": id,
        "title": name,
        "status": "in_progress",
        "rawInput": input,
    })
}

fn kinds(items: &[Value]) -> Vec<&str> {
    items
        .iter()
        .filter_map(|item| item.get("kind").and_then(Value::as_str))
        .collect()
}

fn texts(items: &[Value]) -> Vec<&str> {
    items
        .iter()
        .filter(|item| item["kind"] == "assistant_text")
        .filter_map(|item| item["text"].as_str())
        .collect()
}

/// A paragraph of ordinary prose at least `bytes` long, with no blank line.
fn paragraph(label: &str, bytes: usize) -> String {
    let mut text = format!("{label}:");
    while text.len() < bytes {
        text.push_str(" words of the answer");
    }
    text.push('.');
    text
}

/// Stream `text` in small token-sized pieces — small enough that every
/// `"\n\n"` is split across two chunks somewhere — collecting what each
/// chunk published.
fn stream(translator: &mut TranscriptTranslator, text: &str) -> Vec<Vec<Value>> {
    let mut published = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let mut cut = rest.len().min(5);
        while !rest.is_char_boundary(cut) {
            cut += 1;
        }
        let (piece, tail) = rest.split_at(cut);
        published.push(translator.on_update(&chunk(piece)));
        rest = tail;
    }
    published
}

fn all(published: Vec<Vec<Value>>) -> Vec<Value> {
    published.into_iter().flatten().collect()
}

/// A long answer arrives paragraph by paragraph, each one the moment its
/// closing blank line has streamed, and the items concatenate back to exactly
/// what the agent wrote: the split moves no byte.
#[test]
fn a_multi_paragraph_answer_flushes_paragraph_by_paragraph() {
    let one = paragraph("one", MIN_PARAGRAPH_FLUSH_BYTES);
    let two = paragraph("two", MIN_PARAGRAPH_FLUSH_BYTES);
    let three = paragraph("three", MIN_PARAGRAPH_FLUSH_BYTES);
    let answer = format!("{one}\n\n{two}\n\n{three}");

    let mut translator = on(false);
    let mut items = all(stream(&mut translator, &answer));
    assert_eq!(
        texts(&items),
        vec![format!("{one}\n\n"), format!("{two}\n\n")],
        "the first two paragraphs are out before the turn ends"
    );
    items.extend(translator.close_turn());

    assert_eq!(kinds(&items), vec!["assistant_text"; 3]);
    assert_eq!(texts(&items)[2], three);
    assert_eq!(texts(&items).concat(), answer);
}

/// A paragraph is published by the chunk that completes its blank line, not
/// one chunk later.
#[test]
fn the_chunk_that_completes_the_blank_line_flushes() {
    let one = paragraph("one", MIN_PARAGRAPH_FLUSH_BYTES);
    let mut translator = on(false);
    assert!(translator.on_update(&chunk(&one)).is_empty());
    assert!(translator.on_update(&chunk("\n")).is_empty());
    let items = translator.on_update(&chunk("\n"));
    assert_eq!(texts(&items), vec![format!("{one}\n\n")]);
    // A blank line of spaces is a blank line too.
    let mut spaced = on(false);
    let items = spaced.on_update(&chunk(&format!("{one}\n  \t\nnext")));
    assert_eq!(texts(&items), vec![format!("{one}\n  \t\n")]);
}

/// A blank line inside a fenced code block is part of the code, not a
/// paragraph break: splitting there would publish half a fence as one item
/// and the rest as prose. The fence closes, then the next blank line flushes.
#[test]
fn a_blank_line_inside_a_fenced_code_block_does_not_flush() {
    let intro = paragraph("intro", MIN_PARAGRAPH_FLUSH_BYTES);
    for code in [
        "```rust\nfn a() {}\n\nfn b() {}\n```",
        "~~~\nfn a() {}\n\n```\n\nfn b() {}\n~~~",
        // A longer fence holds a shorter one, which must not close it.
        "````md\nfn a() {}\n\n```\n\nfn b() {}\n\n````",
    ] {
        let answer = format!("{intro}\n{code}\n\nAfter the code.");
        let mut translator = on(false);
        let items = all(stream(&mut translator, &answer));
        assert_eq!(
            texts(&items),
            vec![format!("{intro}\n{code}\n\n")],
            "{code}: only the blank line after the fence closed may flush"
        );
        assert_eq!(texts(&translator.close_turn()), vec!["After the code."]);
    }
}

/// An indented fence — the usual shape inside a list item — is still a fence.
#[test]
fn an_indented_fence_inside_a_list_item_does_not_flush() {
    let intro = paragraph("intro", MIN_PARAGRAPH_FLUSH_BYTES);
    let answer = format!("{intro}\n- step one\n  ```sh\n  ls\n\n  pwd\n  ```\n- step two");
    let mut translator = on(false);
    assert!(all(stream(&mut translator, &answer)).is_empty());
    assert_eq!(texts(&translator.close_turn()), vec![answer.as_str()]);
}

/// A blank line ends a paragraph before a list and ends a list before a
/// paragraph, and both are flush points. A blank line *between* two items of
/// one list, or before an indented continuation of an item, is not: the list
/// is one block, and splitting it would hand an old client — which renders
/// each item on its own — two lists, numbered from 1 twice.
#[test]
fn a_list_and_a_paragraph_split_but_a_loose_list_stays_whole() {
    let intro = paragraph("intro", MIN_PARAGRAPH_FLUSH_BYTES);
    let filler = paragraph("item", MIN_PARAGRAPH_FLUSH_BYTES);
    let list = format!(
        "1. {filler}\n\n2. second item\n\n   continued under the second item\n\n- a bullet\n* another\n+ and another"
    );
    let outro = "That is all.";
    let answer = format!("{intro}\n\n{list}\n\n{outro}");

    let mut translator = on(false);
    let mut items = all(stream(&mut translator, &answer));
    items.extend(translator.close_turn());
    assert_eq!(
        texts(&items),
        vec![
            format!("{intro}\n\n"),
            format!("{list}\n\n"),
            outro.to_owned()
        ]
    );
}

/// A line that only *looks* like it might start a list once more of it has
/// streamed ("-", "12") is not decided until it can be.
#[test]
fn a_list_marker_split_across_chunks_is_read_whole() {
    let item = paragraph("item", MIN_PARAGRAPH_FLUSH_BYTES);
    for (next, splits) in [
        ("- b", false),
        ("-b is prose", true),
        ("12. b", false),
        ("12 b", true),
    ] {
        let mut translator = on(false);
        assert!(translator
            .on_update(&chunk(&format!("- {item}\n\n")))
            .is_empty());
        let mut items = Vec::new();
        for ch in next.chars() {
            items.extend(translator.on_update(&chunk(&ch.to_string())));
        }
        assert_eq!(!items.is_empty(), splits, "next line {next:?}");
    }
}

/// The minimum: a short paragraph is not published on its own; it rides with
/// the next until the prefix up to a blank line reaches
/// [`MIN_PARAGRAPH_FLUSH_BYTES`], measured to the end of the blank line.
#[test]
fn short_paragraphs_coalesce_until_the_minimum() {
    let mut translator = on(false);
    let items = all(stream(&mut translator, "Sure.\n\nHere is the plan.\n\n"));
    assert!(items.is_empty(), "short paragraphs wait: {items:?}");

    // Exactly the minimum, counted through the blank line, flushes.
    let mut exact = on(false);
    let body = "x".repeat(MIN_PARAGRAPH_FLUSH_BYTES - 2);
    assert!(exact.on_update(&chunk(&body)).is_empty());
    assert!(exact.on_update(&chunk("\n")).is_empty());
    assert_eq!(
        texts(&exact.on_update(&chunk("\n"))),
        vec![format!("{body}\n\n")]
    );
    // One byte short does not.
    let mut short = on(false);
    let body = "x".repeat(MIN_PARAGRAPH_FLUSH_BYTES - 3);
    assert!(short.on_update(&chunk(&format!("{body}\n\n"))).is_empty());

    // The short paragraphs are not lost: they lead the next flush.
    let long = paragraph("long", MIN_PARAGRAPH_FLUSH_BYTES);
    let items = all(stream(&mut translator, &format!("{long}\n\nTail")));
    assert_eq!(
        texts(&items),
        vec![format!("Sure.\n\nHere is the plan.\n\n{long}\n\n")]
    );
}

/// The relay-volume bound the minimum exists for: a 24 KiB answer written as
/// one-line paragraphs publishes no more than one event per minimum's worth of
/// prose.
#[test]
fn many_short_paragraphs_are_bounded_by_the_minimum() {
    let line = "A short paragraph of about fifty bytes, no more.\n\n";
    let answer = line.repeat(COALESCE_FLUSH_BYTES / line.len() - 1);
    let mut translator = on(false);
    let mut items = all(stream(&mut translator, &answer));
    items.extend(translator.close_turn());
    assert_eq!(texts(&items).concat(), answer);
    assert!(
        items.len() <= answer.len() / MIN_PARAGRAPH_FLUSH_BYTES + 1,
        "{} events for {} bytes",
        items.len(),
        answer.len()
    );
    assert!(items.len() > 1, "and it still streams");
}

/// Subagent prose keeps its own boundaries — its next item, the owning
/// call's result, the turn end, size — and a blank line in it is not one: the
/// change is to the agent's own narrative, the one a person reads live.
#[test]
fn subagent_buffers_do_not_flush_at_paragraphs() {
    let one = paragraph("one", MIN_PARAGRAPH_FLUSH_BYTES);
    let two = paragraph("two", MIN_PARAGRAPH_FLUSH_BYTES);
    let words = format!("{one}\n\n{two}\n\nthree");
    let mut translator = on(false);
    for piece in words.split_inclusive('\n') {
        assert!(translator
            .on_update(&subagent_chunk("spawn-1", piece))
            .is_empty());
    }
    let items = translator.close_turn();
    assert_eq!(kinds(&items), vec!["assistant_text"]);
    assert_eq!(items[0]["text"], words.as_str());
    assert_eq!(items[0]["parentToolId"], "spawn-1");
}

/// A subagent's paragraph break does not flush the agent's buffer either,
/// and the agent's does not flush the subagent's.
#[test]
fn an_agent_paragraph_leaves_a_subagents_buffer_alone() {
    let one = paragraph("one", MIN_PARAGRAPH_FLUSH_BYTES);
    let mut translator = on(false);
    translator.on_update(&subagent_chunk("spawn-1", &format!("{one}\n\nsub")));
    let items = translator.on_update(&chunk(&format!("{one}\n\nlead")));
    assert_eq!(texts(&items), vec![format!("{one}\n\n")]);
    assert!(items[0].get("parentToolId").is_none());
    let rest = translator.close_turn();
    assert_eq!(texts(&rest), vec!["lead", &format!("{one}\n\nsub")]);
    assert_eq!(rest[1]["parentToolId"], "spawn-1");
}

/// Thoughts keep their own buffer and are folded away by readers, so a
/// blank line in them is not a flush point. Thinking buffered before a
/// paragraph flush is published ahead of it, in the order it was written.
#[test]
fn thoughts_do_not_split_but_precede_the_paragraph_they_came_before() {
    let one = paragraph("one", MIN_PARAGRAPH_FLUSH_BYTES);
    let mut translator = on(true);
    let thinking = format!("{one}\n\nstill thinking\n\n");
    assert!(translator.on_update(&thought(&thinking)).is_empty());
    let items = translator.on_update(&chunk(&format!("{one}\n\nnext")));
    assert_eq!(kinds(&items), vec!["reasoning", "assistant_text"]);
    assert_eq!(items[0]["text"], thinking.as_str());
}

/// A call held for its arguments defers the paragraph rather than being
/// released, argument-less, ahead of it. Once the arguments land the call is
/// published with them, and the paragraph follows on the next chunk.
#[test]
fn a_held_call_defers_the_paragraph_until_its_arguments_land() {
    let one = paragraph("one", MIN_PARAGRAPH_FLUSH_BYTES);
    let mut translator = on(false);
    assert!(translator
        .on_update(&tool_call("t1", "Terminal", json!({})))
        .is_empty());
    assert!(
        translator
            .on_update(&chunk(&format!("{one}\n\nmore")))
            .is_empty(),
        "the paragraph waits for the held call"
    );
    let arguments = translator.on_update(&json!({
        "sessionUpdate": "tool_call_update",
        "toolCallId": "t1",
        "status": "in_progress",
        "rawInput": { "command": "ls" },
    }));
    assert_eq!(kinds(&arguments), vec!["tool_call"]);
    assert_eq!(arguments[0]["tool"]["input"]["command"], "ls");

    let items = translator.on_update(&chunk(" text"));
    assert_eq!(texts(&items), vec![format!("{one}\n\n")]);
    assert_eq!(texts(&translator.close_turn()), vec!["more text"]);
}

/// A subagent's held call is not the agent's: it does not defer the agent's
/// paragraph, and the paragraph does not release it.
#[test]
fn a_subagents_held_call_does_not_defer_the_agents_paragraph() {
    let one = paragraph("one", MIN_PARAGRAPH_FLUSH_BYTES);
    let mut translator = on(false);
    let mut held = tool_call("child-1", "Terminal", json!({}));
    held["_meta"]["claudeCode"]["parentToolUseId"] = json!("spawn-1");
    assert!(translator.on_update(&held).is_empty());
    let items = translator.on_update(&chunk(&format!("{one}\n\nmore")));
    assert_eq!(kinds(&items), vec!["assistant_text"]);
}

/// Off — the default — the translator publishes exactly what it did before
/// the paragraph boundary existed: one item for the whole answer, at the
/// narrative or turn boundary, however many blank lines it holds.
#[test]
fn paragraph_flushing_is_off_by_default() {
    let one = paragraph("one", MIN_PARAGRAPH_FLUSH_BYTES);
    let two = paragraph("two", MIN_PARAGRAPH_FLUSH_BYTES);
    let answer = format!("{one}\n\n{two}\n\nthree");
    let mut translator = TranscriptTranslator::new(true);
    translator.on_update(&thought("first, think"));
    assert!(all(stream(&mut translator, &answer)).is_empty());
    let items = translator.close_turn();
    assert_eq!(kinds(&items), vec!["assistant_text", "reasoning"]);
    assert_eq!(items[0]["text"], answer.as_str());

    let mut explicit = TranscriptTranslator::new(true).with_paragraph_flush(false);
    assert!(all(stream(&mut explicit, &answer)).is_empty());
}

/// A tool call is a fresh start: a fence the prose before it left open does
/// not swallow the paragraph breaks of the prose after it.
#[test]
fn a_narrative_boundary_resets_the_fence() {
    let one = paragraph("one", MIN_PARAGRAPH_FLUSH_BYTES);
    let mut translator = on(false);
    translator.on_update(&chunk("```\nunclosed"));
    let items = translator.on_update(&tool_call("t1", "Bash", json!({ "command": "ls" })));
    assert_eq!(kinds(&items), vec!["assistant_text", "tool_call"]);
    let items = translator.on_update(&chunk(&format!("{one}\n\nnext")));
    assert_eq!(texts(&items), vec![format!("{one}\n\n")]);
}

/// The size boundary cuts wherever it lands, even inside a fence — and the
/// fence is still open for the prose that continues it, so a blank line in
/// the rest of the code still does not flush.
#[test]
fn a_size_flush_inside_a_fence_keeps_the_fence_open() {
    let mut translator = on(false);
    translator.on_update(&chunk("```\n"));
    let items = translator.on_update(&chunk(&"x".repeat(COALESCE_FLUSH_BYTES)));
    assert_eq!(kinds(&items), vec!["assistant_text"]);
    let code = format!(
        "{}\n\nmore code\n```",
        "y".repeat(MIN_PARAGRAPH_FLUSH_BYTES)
    );
    assert!(translator.on_update(&chunk(&code)).is_empty());
    let items = translator.on_update(&chunk("\n\nafter"));
    assert_eq!(texts(&items), vec![format!("{code}\n\n")]);
}

/// A new turn starts outside any fence.
#[test]
fn a_new_turn_resets_the_fence() {
    let one = paragraph("one", MIN_PARAGRAPH_FLUSH_BYTES);
    let mut translator = on(false);
    translator.on_update(&chunk("```\nunclosed"));
    let _ = translator.begin_turn("again", None, None, None, 0);
    let items = translator.on_update(&chunk(&format!("{one}\n\nnext")));
    assert_eq!(texts(&items), vec![format!("{one}\n\n")]);
}

/// Multibyte prose splits only at the newline, so no item ever ends inside a
/// character.
#[test]
fn multibyte_prose_splits_cleanly() {
    let one = "é".repeat(MIN_PARAGRAPH_FLUSH_BYTES);
    let answer = format!("{one}\r\n\r\nzweiter Absatz…");
    let mut translator = on(false);
    let mut items = all(stream(&mut translator, &answer));
    assert_eq!(texts(&items), vec![format!("{one}\r\n\r\n")]);
    items.extend(translator.close_turn());
    assert_eq!(texts(&items).concat(), answer);
}
