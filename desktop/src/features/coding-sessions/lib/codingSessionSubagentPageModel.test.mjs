import assert from "node:assert/strict";
import test from "node:test";

import {
  codingSessionSubagentBarElapsedMs,
  codingSessionSubagentPageRestingStatus,
  deriveCodingSessionSubagentBar,
  readCodingSessionSubagentPrompt,
  resolveCodingSessionSubagentLineage,
  resolveCodingSessionSubagentPage,
  selectCodingSessionSubagentItems,
} from "./codingSessionSubagentPageModel.ts";
import { deriveCodingSessionSubagentPanel } from "./codingSessionSubagents.ts";
import { projectCodingSessionTranscript } from "./codingSessionTranscriptProjection.ts";

const TARGET = {
  driver: "claude-agent-acp",
  instanceId: "0123456789abcdef",
  sessionId: "11111111-2222-3333-4444-555555555555",
  generation: 1,
};

function project(items) {
  return projectCodingSessionTranscript(
    items.map((item, index) => ({
      target: TARGET,
      eventSeq: index + 1,
      timestamp: 1_700_000_000_000 + index * 1_000,
      turnId: "turn-1",
      item,
    })),
    { channelId: "channel-1", generationId: "generation-1" },
  );
}

function taskCall(toolId, input) {
  return {
    kind: "tool_call",
    tool: { toolName: "Map the call sites", toolKind: "think", toolId, input },
  };
}

const INPUT = {
  description: "Map the call sites",
  prompt: "Find every caller of fit_item.",
  subagent_type: "Explore",
};

function spawnTranscript({ result = true, input = INPUT } = {}) {
  return project([
    { kind: "user_prompt", content: "Investigate" },
    taskCall("task-1", input),
    { kind: "assistant_text", text: "Reading a.rs", parentToolId: "task-1" },
    {
      kind: "tool_call",
      tool: {
        toolName: "Read",
        toolKind: "read",
        toolId: "read-1",
        input: { path: "a.rs" },
      },
      parentToolId: "task-1",
    },
    {
      kind: "tool_result",
      toolId: "read-1",
      toolName: "Read",
      content: "fn main() {}",
      parentToolId: "task-1",
    },
    {
      kind: "assistant_text",
      text: "Someone else's step",
      parentToolId: "task-2",
    },
    ...(result
      ? [
          {
            kind: "tool_result",
            toolId: "task-1",
            toolName: "ignored",
            content: "Found three call sites.",
            isError: false,
            subagent: {
              model: "claude-haiku",
              totalTokens: 48_210,
              durationMs: 9_000,
              toolUseCount: 1,
            },
          },
        ]
      : []),
    { kind: "assistant_text", text: "The lead's own answer." },
  ]);
}

test("the page holds only the items whose parentToolId is that call, in order", () => {
  const transcript = spawnTranscript();
  const items = selectCodingSessionSubagentItems(transcript, "task-1");
  assert.deepEqual(
    items.map((item) => item.type),
    ["message", "tool"],
  );
  assert.ok(items.every((item) => item.parentToolId === "task-1"));
  assert.equal(selectCodingSessionSubagentItems(transcript, "nope").length, 0);
});

test("a spawn's page carries its row, its items and the prompt it was given", () => {
  const transcript = spawnTranscript();
  const panel = deriveCodingSessionSubagentPanel([transcript]);
  const page = resolveCodingSessionSubagentPage({
    parentToolId: "task-1",
    panel,
    transcript,
  });
  assert.equal(page.kind, "spawn");
  assert.equal(page.items.length, 2);
  assert.equal(page.unattributed, false);
  assert.deepEqual(page.prompt, {
    kind: "given",
    text: "Find every caller of fit_item.",
    description: "Map the call sites",
  });
});

test("a page whose owning call is gone says so and keeps what was published", () => {
  const transcript = spawnTranscript();
  const panel = deriveCodingSessionSubagentPanel([transcript]);
  const page = resolveCodingSessionSubagentPage({
    parentToolId: "task-2",
    panel,
    transcript,
  });
  assert.equal(page.kind, "missing-call");
  assert.equal(page.items.length, 1);
});

test("the prompt is disclosed as truncated or absent rather than left out", () => {
  const call = (args) => ({ type: "tool", args });
  assert.deepEqual(
    readCodingSessionSubagentPrompt(
      call({
        truncated: true,
        byteCount: 20_000,
        preview: '{"prompt":"Find',
        contentDigest: "ab",
      }),
    ),
    {
      kind: "truncated",
      preview: '{"prompt":"Find',
      byteCount: 20_000,
      description: null,
    },
  );
  assert.deepEqual(
    readCodingSessionSubagentPrompt(call({ description: "x" })),
    {
      kind: "absent",
      description: "x",
    },
  );
  assert.equal(
    readCodingSessionSubagentPrompt(call({ prompt: "  " })).kind,
    "absent",
  );
});

test("a finished spawn's bar is settled: its reported duration, tokens and tools, no live timer", () => {
  const transcript = spawnTranscript();
  const [row] = deriveCodingSessionSubagentPanel([transcript]).rows;
  const bar = deriveCodingSessionSubagentBar(row);
  assert.equal(bar.status, "done");
  assert.equal(bar.statusLabel, "Completed");
  assert.equal(bar.live, false);
  assert.equal(bar.model, "claude-haiku");
  assert.equal(bar.modelName, "Claude Haiku");
  assert.equal(bar.tokens, "48.2k tok");
  assert.equal(bar.tools, "1 tool");
  assert.equal(codingSessionSubagentBarElapsedMs(bar, Date.now() + 1e9), 9_000);
});

test("a running spawn's bar ticks from its start; a settled-stopped one never does", () => {
  const transcript = spawnTranscript({ result: false });
  const [row] = deriveCodingSessionSubagentPanel([transcript]).rows;
  const bar = deriveCodingSessionSubagentBar(row);
  assert.equal(bar.status, "running");
  assert.equal(bar.live, true);
  assert.equal(
    codingSessionSubagentBarElapsedMs(bar, bar.startedAtMs + 65_000),
    65_000,
  );

  // The same open call read through a settled turn: stopped, no timer.
  const [stoppedRow] = deriveCodingSessionSubagentPanel(
    [transcript],
    () => "settled",
  ).rows;
  const stopped = deriveCodingSessionSubagentBar(stoppedRow);
  assert.equal(stopped.status, "stopped");
  assert.equal(stopped.statusLabel, "Stopped");
  assert.equal(stopped.live, false);
  assert.equal(codingSessionSubagentBarElapsedMs(stopped, Date.now()), null);

  const [unknownRow] = deriveCodingSessionSubagentPanel(
    [transcript],
    () => "unknown",
  ).rows;
  const unknown = deriveCodingSessionSubagentBar(unknownRow);
  assert.equal(unknown.statusLabel, "Status unknown");
  assert.equal(unknown.live, false);
});

test("a failed spawn's bar says Failed", () => {
  const transcript = project([
    { kind: "user_prompt", content: "Investigate" },
    taskCall("task-1", INPUT),
    {
      kind: "tool_result",
      toolId: "task-1",
      toolName: "x",
      content: "boom",
      isError: true,
    },
  ]);
  const [row] = deriveCodingSessionSubagentPanel([transcript]).rows;
  const bar = deriveCodingSessionSubagentBar(row);
  assert.equal(bar.status, "failed");
  assert.equal(bar.statusLabel, "Failed");
  assert.equal(bar.live, false);
});

test("the page's transcript reads unended steps through the subagent's status", () => {
  assert.equal(codingSessionSubagentPageRestingStatus("running"), "running");
  assert.equal(codingSessionSubagentPageRestingStatus("stopped"), "stopped");
  assert.equal(codingSessionSubagentPageRestingStatus("failed"), "stopped");
  assert.equal(codingSessionSubagentPageRestingStatus("unknown"), "unknown");
  assert.equal(codingSessionSubagentPageRestingStatus(null), "unknown");
});

test("SV-97: the page drops the subagent's last prose when it is the returned result", () => {
  const transcript = project([
    { kind: "user_prompt", content: "Investigate" },
    taskCall("task-1", INPUT),
    { kind: "assistant_text", text: "Reading a.rs", parentToolId: "task-1" },
    {
      kind: "assistant_text",
      text: "Found three call sites.",
      parentToolId: "task-1",
    },
    {
      kind: "tool_result",
      toolId: "task-1",
      toolName: "Task",
      content: "Found three call sites.",
      isError: false,
    },
  ]);
  const page = resolveCodingSessionSubagentPage({
    parentToolId: "task-1",
    panel: deriveCodingSessionSubagentPanel([transcript]),
    transcript,
  });
  assert.equal(page.kind, "spawn");
  assert.deepEqual(
    page.items.map((item) => item.text),
    ["Reading a.rs"],
  );
});

test("SV-98: the lineage names the execution holding the call, with its shown status", () => {
  const record = (generationId, title, ids) => ({
    generationId,
    title,
    transcript: ids.map((id) => ({ id })),
  });
  const lead = {
    executionKey: "lead",
    priorGenerations: [],
    activeGeneration: record("g-lead", "Lead seat", ["x"]),
  };
  const seat = {
    executionKey: "seat",
    priorGenerations: [record("g-seat-1", "Old", ["call-1"])],
    activeGeneration: record("g-seat-2", "Reviewer seat", []),
  };
  const working = { kind: "working", label: "Working" };
  const idle = { kind: "idle", label: "Idle" };
  const executions = [
    { execution: lead, status: idle },
    { execution: seat, status: working },
  ];
  const base = {
    layout: "umbrella",
    sessionTitle: "Session title",
    focusedExecution: lead,
    focusedRecord: lead.activeGeneration,
    executions,
  };
  assert.deepEqual(
    resolveCodingSessionSubagentLineage({ ...base, callId: "call-1" }),
    { title: "Reviewer seat", status: working, generationId: "g-seat-1" },
  );
  // A call no execution holds falls back to the focused one; so does the
  // single layout, which titles the parent by the session.
  assert.deepEqual(
    resolveCodingSessionSubagentLineage({ ...base, callId: "nope" }),
    { title: "Session title", status: idle, generationId: "g-lead" },
  );
  assert.deepEqual(
    resolveCodingSessionSubagentLineage({
      ...base,
      layout: "single",
      callId: "call-1",
    }),
    { title: "Session title", status: idle, generationId: "g-lead" },
  );
  // Nothing in view to name: no status, never a guessed one.
  assert.equal(
    resolveCodingSessionSubagentLineage({
      ...base,
      focusedExecution: null,
      focusedRecord: null,
      callId: null,
    }).status,
    null,
  );
});
