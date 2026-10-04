/**
 * The three item kinds this fork added to the transcript contract, carried all
 * the way from a 44225 envelope through the projector into the turn-first
 * transcript model and the task rail. The donor's model suites cover the
 * shared derivation; these cover the seams the amendments opened.
 */
import assert from "node:assert/strict";
import test from "node:test";

import { deriveCodingSessionTaskModel } from "./codingSessionTaskModel.ts";
import { CODING_SESSION_CONTINUITY_TITLE } from "./codingSessionTranscriptItems.ts";
import { deriveCodingSessionTranscriptModel } from "./codingSessionTranscriptModel.ts";
import { projectCodingSessionTranscript } from "./codingSessionTranscriptProjection.ts";

const TARGET = {
  driver: "claude-agent-acp",
  instanceId: "0123456789abcdef",
  sessionId: "11111111-2222-3333-4444-555555555555",
  generation: 1,
};

function envelope(eventSeq, item, turnId = "turn-1") {
  return {
    target: TARGET,
    eventSeq,
    timestamp: 1_800_000_000_000 + eventSeq,
    turnId,
    item,
  };
}

function project(items) {
  return projectCodingSessionTranscript(
    items.map((item, index) => envelope(index + 1, item)),
    { channelId: "channel-1", generationId: "generation-1" },
  );
}

function turnOf(transcript) {
  const model = deriveCodingSessionTranscriptModel(transcript, {
    isWorking: false,
  });
  const turn = model.blocks.find((block) => block.kind === "turn");
  assert.ok(turn, "expected the items to derive one turn");
  return { model, turn };
}

test("a plan item drives the task rail through the producer's checklist", () => {
  const transcript = project([
    { kind: "user_prompt", content: "go" },
    {
      kind: "plan",
      entries: [
        { content: "read the code", priority: "high", status: "completed" },
        {
          content: "write the test",
          priority: "medium",
          status: "in_progress",
        },
        { content: "run the gate", priority: "low", status: "pending" },
      ],
      text: "- [x] read the code\n- [ ] write the test (in progress)\n- [ ] run the gate",
    },
  ]);
  const tasks = deriveCodingSessionTaskModel(transcript);
  assert.ok(tasks, "a plan item must produce a task snapshot");
  assert.deepEqual(
    tasks.tasks.map((task) => [task.text, task.status]),
    [
      ["read the code", "completed"],
      ["write the test", "in_progress"],
      ["run the gate", "pending"],
    ],
  );
  assert.equal(tasks.completedCount, 1);
  assert.equal(tasks.state, "active");
  assert.equal(
    tasks.sourceItemId,
    transcript.find((item) => item.type === "plan").id,
  );
});

test("a later plan replaces the earlier one — plans are snapshots, not a log", () => {
  const tasks = deriveCodingSessionTaskModel(
    project([
      { kind: "plan", text: "- [ ] first attempt" },
      { kind: "plan", text: "- [x] first attempt\n- [x] second attempt" },
    ]),
  );
  assert.deepEqual(
    tasks.tasks.map((task) => task.text),
    ["first attempt", "second attempt"],
  );
  assert.equal(tasks.completedCount, 2);
  assert.equal(tasks.state, "complete");
});

test("a plan carrying no checklist leaves the rail alone rather than emptying it", () => {
  const withPlanOnly = deriveCodingSessionTaskModel(
    project([{ kind: "plan", text: "prose with no checkboxes" }]),
  );
  assert.equal(withPlanOnly, null);

  const preserved = deriveCodingSessionTaskModel(
    project([
      { kind: "plan", text: "- [ ] real task" },
      { kind: "plan", text: "prose with no checkboxes" },
    ]),
  );
  assert.deepEqual(
    preserved.tasks.map((task) => task.text),
    ["real task"],
  );
});

test("a reasoning item reaches the turn as a thought, never dropped or diagnosed away", () => {
  const transcript = project([
    { kind: "user_prompt", content: "go" },
    { kind: "reasoning", text: "I should read the file first." },
    { kind: "assistant_text", text: "Reading it." },
  ]);
  const thought = transcript.find((item) => item.type === "thought");
  assert.ok(thought);
  assert.equal(thought.text, "I should read the file first.");

  const { model, turn } = turnOf(transcript);
  assert.deepEqual(model.diagnostics, []);
  assert.ok(
    turn.entries.some(
      (entry) => entry.kind === "item" && entry.item.type === "thought",
    ),
    "the thought stays in reading order",
  );
});

test("an elided item stays visible in reading order, not folded into diagnostics", () => {
  const transcript = project([
    { kind: "user_prompt", content: "go" },
    {
      kind: "elided",
      reason: "oversize",
      byteCount: 41_235,
      contentDigest: "sha256:deadbeef",
    },
  ]);
  const { model, turn } = turnOf(transcript);
  assert.deepEqual(model.diagnostics, []);
  const elided = turn.entries.find(
    (entry) => entry.kind === "item" && entry.item.title === "Content dropped",
  );
  assert.ok(elided, "a dropped item must be visible where it was dropped");
  assert.equal(elided.item.elision.bytes, 41_235);
  assert.deepEqual(turn.diagnostics, []);
});

test("routine continuity leaves the transcript for Details; a restart without context stays in it", () => {
  const expected = new Map([
    ["session_fresh", "Started fresh — no prior session context"],
    [
      "session_rehydrated",
      "Rehydrated — verified session history is available to this agent",
    ],
    [
      "session_resumed",
      "Resumed — reconnected to the provider's native session",
    ],
    [
      "session_loaded",
      "Loaded — the provider replayed its native session history",
    ],
    ["session_restarted_without_context", "Restarted without prior context"],
  ]);

  for (const [slug, prose] of expected) {
    const { model, turn } = turnOf(
      project([
        { kind: "user_prompt", content: "go" },
        { kind: "status", status: slug },
        { kind: "assistant_text", text: "done" },
      ]),
    );
    assert.deepEqual(model.diagnostics, []);
    assert.deepEqual(
      turn.diagnostics,
      [],
      `${slug} must not be buried in Turn details`,
    );
    const row = turn.entries.find(
      (entry) =>
        entry.kind === "item" &&
        entry.item.title === CODING_SESSION_CONTINUITY_TITLE,
    );
    // SV-16 (plans/SESSION_VIEW_PARITY_PLAN.md, D3): routine continuity is
    // kept in model.sessionFacts for Details; a loss of context is news
    // about the next turn and stays in the reading order.
    const fact = model.sessionFacts.find(
      (item) => item.title === CODING_SESSION_CONTINUITY_TITLE,
    );
    assert.ok(fact, `${slug} must be kept in the session facts`);
    assert.equal(fact.text, prose);
    assert.equal(fact.type, "lifecycle");
    if (slug === "session_restarted_without_context") {
      assert.ok(row, `${slug} must render in the reading order`);
      assert.equal(row.item.text, prose);
      assert.equal(row.item.type, "lifecycle");
      assert.equal(row.item.renderClass, "status");
    } else {
      assert.equal(row, undefined, `${slug} must leave the transcript`);
    }
  }
});

test("an unknown status slug keeps its generic title and its diagnostics routing", () => {
  const { turn } = turnOf(
    project([
      { kind: "user_prompt", content: "go" },
      { kind: "status", status: "session_teleported" },
      { kind: "assistant_text", text: "done" },
    ]),
  );
  // Additive slugs are the provider's to invent; prose for one this build has
  // never seen would be a guess about the session's history.
  assert.deepEqual(
    turn.diagnostics.map((item) => [item.title, item.text]),
    [["Status", "session_teleported"]],
  );
});

test("status rows still divert to the diagnostics rail, and ceremony is dropped", () => {
  const { model, turn } = turnOf(
    project([
      { kind: "user_prompt", content: "go" },
      { kind: "status", status: "thinking" },
      { kind: "context_window_updated", usage: { usedTokens: 10 } },
      { kind: "assistant_text", text: "done" },
    ]),
  );
  assert.deepEqual(model.diagnostics, []);
  assert.deepEqual(
    turn.diagnostics.map((item) => item.title),
    ["Status"],
    "context-window telemetry is ceremonial and drops out entirely",
  );
  assert.deepEqual(
    turn.entries.map((entry) =>
      entry.kind === "item" ? entry.item.type : "tool-group",
    ),
    ["message", "message"],
  );
});
