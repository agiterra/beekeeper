/**
 * H-08 — mapping Buzz `TranscriptItem`s into export-bundle messages.
 *
 * The corpus (`conformance/transcript-export/`) constrains the bundle
 * envelope and the user_prompt attachment participation rule; the per-kind
 * message shapes are Buzz-defined for the Buzz viewer. Buzz transcript items
 * carry no attachments today, so every user_prompt maps with an empty
 * attachments list — the attachment plane stays law-complete but dormant
 * (recorded in the ledger row).
 */
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";

import { projectCodingSessionTranscript } from "../codingSessionTranscriptProjection.ts";
import {
  mapCodingSessionTranscriptToExportMessages,
  mapTranscriptItemsToExportMessages,
} from "./transcriptExportMessages.ts";

const timestamp = "2026-04-23T12:00:00.000Z";

test("user and assistant messages map to user_prompt and assistant_text", () => {
  const mapped = mapTranscriptItemsToExportMessages([
    {
      id: "m-1",
      type: "message",
      renderClass: "message",
      role: "user",
      title: "Andy",
      text: "hello",
      timestamp,
    },
    {
      id: "m-2",
      type: "message",
      renderClass: "message",
      role: "assistant",
      title: "Assistant",
      text: "hi",
      timestamp,
    },
  ]);
  assert.deepEqual(mapped, [
    {
      kind: "user_prompt",
      id: "m-1",
      text: "hello",
      timestamp,
      attachments: [],
    },
    { kind: "assistant_text", id: "m-2", text: "hi", timestamp },
  ]);
});

test("thought, plan, and lifecycle items keep their kind, title, and text", () => {
  const mapped = mapTranscriptItemsToExportMessages([
    {
      id: "th-1",
      type: "thought",
      renderClass: "thought",
      title: "Thinking",
      text: "hmm",
      timestamp,
    },
    {
      id: "p-1",
      type: "plan",
      renderClass: "plan",
      title: "Plan",
      text: "1. do it",
      timestamp,
    },
    {
      id: "l-1",
      type: "lifecycle",
      renderClass: "status",
      title: "Turn result",
      text: "done",
      timestamp,
    },
  ]);
  assert.deepEqual(mapped, [
    { kind: "thought", id: "th-1", title: "Thinking", text: "hmm", timestamp },
    { kind: "plan", id: "p-1", title: "Plan", text: "1. do it", timestamp },
    {
      kind: "lifecycle",
      id: "l-1",
      title: "Turn result",
      text: "done",
      timestamp,
    },
  ]);
});

test("metadata items flatten their sections into one lifecycle-shaped note", () => {
  const mapped = mapTranscriptItemsToExportMessages([
    {
      id: "meta-1",
      type: "metadata",
      renderClass: "raw-rail",
      title: "Prompt",
      sections: [
        { title: "System", body: "be helpful" },
        { title: "Context", body: "repo buzz" },
      ],
      timestamp,
    },
  ]);
  assert.deepEqual(mapped, [
    {
      kind: "lifecycle",
      id: "meta-1",
      title: "Prompt",
      text: "System\nbe helpful\n\nContext\nrepo buzz",
      timestamp,
    },
  ]);
});

test("tool items map to tool_call with toolName, status, isError, and the result as text", () => {
  const mapped = mapTranscriptItemsToExportMessages([
    {
      id: "t-1",
      type: "tool",
      renderClass: "shell",
      descriptor: { renderClass: "shell", label: "Ran command", preview: "ls" },
      title: "Bash",
      toolName: "Bash",
      buzzToolName: null,
      status: "completed",
      args: { command: "ls" },
      result: "ok",
      isError: false,
      timestamp,
      startedAt: timestamp,
      completedAt: timestamp,
    },
  ]);
  assert.deepEqual(mapped, [
    {
      kind: "tool_call",
      id: "t-1",
      title: "Bash",
      toolName: "Bash",
      status: "completed",
      isError: false,
      text: "ok",
      timestamp,
    },
  ]);
});

// ── SV-36 S5: a coding session exports the messages the agent wrote ─────────

const proseJoinFixture = JSON.parse(
  readFileSync(
    resolve(
      process.cwd(),
      "../conformance/transcript-prose-join/fixtures/vectors.json",
    ),
    "utf8",
  ),
);

/** A vector's items as the session hands them to export: per stream, in order. */
function sessionTranscriptOf(vector) {
  const streams = new Map();
  for (const envelope of vector.input) {
    const target = envelope.content.session;
    const key = JSON.stringify([
      envelope.signer,
      target.driver,
      target.instanceId,
      target.sessionId,
      target.generation,
    ]);
    const stream = streams.get(key) ?? { key, envelope, byEventId: new Map() };
    stream.byEventId.set(envelope.eventId, {
      target,
      eventSeq: envelope.content.eventSeq,
      timestamp: envelope.content.timestamp,
      turnId: envelope.content.turnId,
      item: envelope.content.item,
      sourceEventId: envelope.eventId,
    });
    streams.set(key, stream);
  }
  return [...streams.values()]
    .sort((left, right) => (left.key < right.key ? -1 : 1))
    .flatMap((stream) =>
      projectCodingSessionTranscript(
        [...stream.byEventId.values()].sort(
          (left, right) => left.eventSeq - right.eventSeq,
        ),
        {
          generationId: stream.key,
          bridgeSource: { pubkey: stream.envelope.signer, label: "vector" },
        },
      ),
    );
}

for (const vector of proseJoinFixture.vectors) {
  test(`export joins prose like the contract: ${vector.name}`, () => {
    const items = sessionTranscriptOf(vector);
    const idByEvent = new Map(
      items.map((item) => [item.sourceEventId, item.id]),
    );
    const exported = mapCodingSessionTranscriptToExportMessages(items).filter(
      (message) =>
        message.kind === "assistant_text" || message.kind === "thought",
    );
    assert.deepEqual(
      exported.map((message) => [message.kind, message.id, message.text]),
      vector.expectedMessages.map((expected) => [
        expected.kind === "reasoning" ? "thought" : "assistant_text",
        idByEvent.get(expected.firstEventId),
        expected.text,
      ]),
      `${vector.name}: one export message per contract message`,
    );
  });
}

test("the raw mapper stays one to one", () => {
  const pieces = [
    {
      id: "p1",
      type: "message",
      renderClass: "message",
      role: "assistant",
      title: "Response",
      text: "One.\n\n",
      timestamp,
      turnId: "t",
    },
    {
      id: "p2",
      type: "message",
      renderClass: "message",
      role: "assistant",
      title: "Response",
      text: "Two.",
      timestamp,
      turnId: "t",
    },
  ];
  assert.equal(mapTranscriptItemsToExportMessages(pieces).length, 2);
  assert.deepEqual(
    mapCodingSessionTranscriptToExportMessages(pieces).map((m) => m.text),
    ["One.\n\nTwo."],
  );
});
