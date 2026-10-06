import assert from "node:assert/strict";
import test from "node:test";

import {
  codingSessionSubagentTitle,
  deriveCodingSessionSubagentPanel,
  formatCodingSessionSubagentGroupLabel,
  formatCodingSessionSubagentFooter,
  formatCodingSessionSubagentMeta,
  formatCodingSessionSubagentTokens,
  partitionCodingSessionSubagentItems,
  settleCodingSessionSubagentSpawns,
  settleCodingSessionSubagentStatus,
  summarizeCodingSessionSubagentStatuses,
  codingSessionSubagentModelName,
  withoutCodingSessionSubagentEchoedReport,
} from "./codingSessionSubagents.ts";
import { deriveCodingSessionTaskModel } from "./codingSessionTaskModel.ts";
import { deriveCodingSessionTranscriptModel } from "./codingSessionTranscriptModel.ts";
import { resolveCodingSessionTurnSettlement } from "./codingSessionTranscriptModelSettlement.ts";
import { projectCodingSessionTranscript } from "./codingSessionTranscriptProjection.ts";

const TARGET = {
  driver: "claude-agent-acp",
  instanceId: "0123456789abcdef",
  sessionId: "11111111-2222-3333-4444-555555555555",
  generation: 1,
};

/** Envelopes one second apart, all in one declared turn. */
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

function taskCall(toolId, description, extra = {}) {
  return {
    kind: "tool_call",
    tool: {
      toolName: description,
      toolKind: "think",
      toolId,
      input: { description, prompt: "Look around", subagent_type: "Explore" },
    },
    ...extra,
  };
}

function taskResult(toolId, extra = {}) {
  return {
    kind: "tool_result",
    toolId,
    toolName: "ignored",
    content: "Found three call sites.\nDetails follow.",
    isError: false,
    ...extra,
  };
}

const READ_CALL = (toolId, parentToolId) => ({
  kind: "tool_call",
  tool: { toolName: "Read", toolKind: "read", toolId, input: { path: "a.rs" } },
  parentToolId,
});
const READ_RESULT = (toolId, parentToolId) => ({
  kind: "tool_result",
  toolId,
  toolName: "Read",
  content: "fn main() {}",
  parentToolId,
});

function oneSubagentTurn(resultExtra = {}) {
  return project([
    { kind: "user_prompt", content: "Investigate" },
    taskCall("task-1", "Map the call sites"),
    { kind: "assistant_text", text: "Reading a.rs", parentToolId: "task-1" },
    READ_CALL("read-1", "task-1"),
    READ_RESULT("read-1", "task-1"),
    taskResult("task-1", resultExtra),
    { kind: "assistant_text", text: "The lead's own answer." },
  ]);
}

function turnEntries(transcript, isWorking = false) {
  const model = deriveCodingSessionTranscriptModel(transcript, { isWorking });
  const turn = model.blocks.find((block) => block.kind === "turn");
  assert.ok(turn, "expected one turn");
  return turn.entries;
}

test("projection stamps parentToolId, toolCallId and only the reported subagent fields", () => {
  const transcript = oneSubagentTurn({
    subagent: { type: "Explore", totalTokens: 48_210, durationMs: 9_000 },
  });
  const call = transcript.find((item) => item.toolCallId === "task-1");
  assert.equal(call.type, "tool");
  assert.equal(call.parentToolId, undefined);
  assert.deepEqual(call.subagent, {
    type: "Explore",
    totalTokens: 48_210,
    durationMs: 9_000,
  });
  const nested = transcript.filter((item) => item.parentToolId === "task-1");
  assert.equal(nested.length, 2, "subagent text and its paired Read card");
  const read = nested.find((item) => item.type === "tool");
  assert.equal(read.toolCallId, "read-1");
  assert.equal(read.status, "completed");
});

test("projection leaves pre-ledger-308 items byte-identical", () => {
  const [item] = project([{ kind: "assistant_text", text: "plain" }]);
  assert.equal("parentToolId" in item, false);
  assert.equal("subagent" in item, false);
});

test("a subagent's items nest under its call and leave the lead's reading order", () => {
  const entries = turnEntries(oneSubagentTurn());
  assert.deepEqual(
    entries.map((entry) => entry.kind),
    ["item", "subagents", "item"],
  );
  const [prompt, spawnEntry, lead] = entries;
  assert.equal(prompt.item.role, "user");
  assert.equal(lead.item.text, "The lead's own answer.");
  assert.equal(spawnEntry.label, "Ran 1 subagent");
  assert.equal(
    codingSessionSubagentTitle(spawnEntry.spawns[0].call),
    "Map the call sites",
  );
  assert.equal(spawnEntry.spawns.length, 1);
  assert.deepEqual(
    spawnEntry.spawns[0].children.map((child) => child.type),
    ["message", "tool"],
  );
  const visibleTexts = entries.flatMap((entry) =>
    entry.kind === "item" && entry.item.type === "message"
      ? [entry.item.text]
      : [],
  );
  assert.equal(visibleTexts.includes("Reading a.rs"), false);
});

test("an item whose owning call is absent stays visible rather than vanishing", () => {
  const transcript = project([
    { kind: "user_prompt", content: "Go" },
    { kind: "assistant_text", text: "orphan", parentToolId: "missing" },
  ]);
  assert.equal(partitionCodingSessionSubagentItems(transcript).nested.size, 0);
  const entries = turnEntries(transcript);
  assert.equal(entries.at(-1).item.text, "orphan");
});

test("a running spawn reads as kicked off; consecutive spawns group", () => {
  const running = project([
    { kind: "user_prompt", content: "Go" },
    taskCall("task-1", "Survey tests"),
  ]);
  const runningEntry = turnEntries(running, true).at(-1);
  assert.equal(runningEntry.label, "Kicked off 1 subagent");
  assert.equal(
    summarizeCodingSessionSubagentStatuses(runningEntry.spawns),
    "1 working",
  );
  // The description left the row (SV-06) but stays one click away.
  assert.equal(
    codingSessionSubagentTitle(runningEntry.spawns[0].call),
    "Survey tests",
  );

  const several = project([
    { kind: "user_prompt", content: "Go" },
    taskCall("task-1", "One"),
    taskCall("task-2", "Two"),
    taskCall("task-3", "Three"),
    { kind: "assistant_text", text: "one", parentToolId: "task-2" },
  ]);
  const entries = turnEntries(several, true);
  assert.equal(entries.length, 2);
  assert.equal(entries[1].kind, "subagents");
  assert.equal(entries[1].label, "Kicked off 3 subagents");
  assert.deepEqual(
    entries[1].spawns.map((spawn) => spawn.children.length),
    [0, 1, 0],
  );
});

test("a spawn is recognized without the adapter's tool name", () => {
  // claude-agent-acp titles the call with its description; an undescribed
  // spawn falls back to the bare name.
  const transcript = project([
    { kind: "user_prompt", content: "Go" },
    { kind: "tool_call", tool: { toolName: "Task", toolId: "t-0" } },
  ]);
  assert.equal(turnEntries(transcript, true).at(-1).kind, "subagents");
});

test("panel rows report status, counts and only the facts that were sent", () => {
  const done = oneSubagentTurn({
    subagent: {
      type: "Explore",
      model: "claude-haiku-4-5",
      totalTokens: 48_210,
      durationMs: 385_000,
      toolUseCount: 7,
    },
  });
  const panel = deriveCodingSessionSubagentPanel([done]);
  assert.equal(panel.rows.length, 1);
  const [row] = panel.rows;
  assert.equal(row.status, "done");
  assert.equal(row.title, "Map the call sites");
  assert.equal(row.type, "Explore");
  assert.equal(row.durationMs, 385_000);
  // Counted from what was published, not the adapter's own figure.
  assert.equal(row.toolCount, 1);
  assert.equal(row.latest, "▸ Read");
  assert.equal(
    formatCodingSessionSubagentMeta(row),
    "Claude Haiku 4.5 · 48.2k tok · 1 tool",
  );
  // The raw id stays on the row: only the shown name is derived.
  assert.equal(row.model, "claude-haiku-4-5");

  const bare = deriveCodingSessionSubagentPanel([
    project([
      { kind: "user_prompt", content: "Go" },
      taskCall("task-9", "Quiet one"),
      taskResult("task-9", { isError: true, content: "Boom" }),
    ]),
  ]).rows[0];
  assert.equal(bare.status, "failed");
  assert.equal(bare.model, null);
  assert.equal(bare.totalTokens, null);
  assert.equal(bare.toolCount, null, "no nested tools and no report: unknown");
  assert.equal(bare.durationMs, 1_000, "falls back to the envelope timestamps");
  assert.equal(bare.latest, "Boom");
  assert.equal(formatCodingSessionSubagentMeta(bare), "");
});

test("footer totals settled, running and only reported tokens", () => {
  const panel = deriveCodingSessionSubagentPanel([
    oneSubagentTurn({ subagent: { totalTokens: 1_500 } }),
    project([
      { kind: "user_prompt", content: "Go" },
      taskCall("task-2", "Unreported"),
      taskResult("task-2"),
      taskCall("task-3", "Still going"),
    ]),
  ]);
  assert.equal(panel.rows.length, 3);
  assert.equal(panel.settled, 2);
  assert.equal(panel.running, 1);
  assert.equal(panel.totalTokens, 1_500);
  assert.equal(
    formatCodingSessionSubagentFooter(panel),
    "2 settled · Σ 1.5k tok · 1 running",
  );
  const unreported = deriveCodingSessionSubagentPanel([
    project([taskCall("t", "x"), taskResult("t")]),
  ]);
  assert.equal(formatCodingSessionSubagentFooter(unreported), "1 settled");
});

test("token figures stay compact", () => {
  assert.equal(formatCodingSessionSubagentTokens(812), "812");
  assert.equal(formatCodingSessionSubagentTokens(48_210), "48.2k");
  assert.equal(formatCodingSessionSubagentTokens(312_000), "312k");
  assert.equal(formatCodingSessionSubagentTokens(1_300_000), "1.3M");
});

test("a subagent's todo list does not become the session's plan", () => {
  const transcript = project([
    { kind: "user_prompt", content: "Go" },
    taskCall("task-1", "Plan inside"),
    {
      kind: "plan",
      entries: [{ content: "subagent step", status: "pending" }],
      parentToolId: "task-1",
    },
  ]);
  assert.equal(deriveCodingSessionTaskModel(transcript), null);
});

/** Envelopes with an explicit turn per item; `null` declares "no turn". */
function projectTurns(pairs) {
  return projectCodingSessionTranscript(
    pairs.map(([turnId, item], index) => ({
      target: TARGET,
      eventSeq: index + 1,
      timestamp: 1_700_000_000_000 + index * 1_000,
      turnId,
      item,
    })),
    { channelId: "channel-1", generationId: "generation-1" },
  );
}

const TURN_RESULT = { kind: "result", subtype: "success", result: "ok" };

test("an open spawn is stopped once its own turn ends", () => {
  const transcript = projectTurns([
    ["turn-1", { kind: "user_prompt", content: "Go" }],
    ["turn-1", taskCall("task-1", "Never answered")],
    ["turn-1", TURN_RESULT],
  ]);
  const [row] = deriveCodingSessionSubagentPanel([transcript]).rows;
  assert.equal(row.status, "stopped");
  assert.equal(row.latest, null);
  const entry = turnEntries(transcript).find((e) => e.kind === "subagents");
  assert.equal(entry.label, "Ran 1 subagent");
  assert.equal(entry.spawns[0].status, "stopped");
});

test("an open spawn is stopped when a later turn begins, and counts as settled", () => {
  const transcript = projectTurns([
    ["turn-1", { kind: "user_prompt", content: "Go" }],
    ["turn-1", taskCall("task-1", "Abandoned")],
    ["turn-2", { kind: "user_prompt", content: "Next" }],
    ["turn-2", taskCall("task-2", "Current")],
  ]);
  const panel = deriveCodingSessionSubagentPanel([transcript]);
  assert.deepEqual(
    panel.rows.map((row) => row.status),
    ["stopped", "running"],
  );
  assert.equal(
    formatCodingSessionSubagentFooter(panel),
    "1 settled · 1 running",
  );
});

test("SV-44: a later turn that began while the spawn's turn still publishes does not stop it", () => {
  const transcript = projectTurns([
    ["turn-1", { kind: "user_prompt", content: "Go" }],
    ["turn-1", taskCall("task-1", "Still going")],
    // A queued prompt with its own turn id arrives mid-run...
    ["turn-2", { kind: "user_prompt", content: "Queued" }],
    // ...and turn 1 keeps publishing after it.
    ["turn-1", { kind: "assistant_text", text: "Still working on it." }],
  ]);
  assert.equal(
    deriveCodingSessionSubagentPanel([transcript]).rows[0].status,
    "running",
  );
});

test("a spawn whose turn settles without a result reads as its tool rows do (SV-44)", () => {
  // Turn A starts a Task; prompt B arrives; one more child of A's Task lands
  // after B's prompt; A never reports a result; B completes; session idle.
  const transcript = projectTurns([
    ["turn-a", { kind: "user_prompt", content: "Go" }],
    ["turn-a", taskCall("task-1", "Abandoned")],
    ["turn-b", { kind: "user_prompt", content: "Next" }],
    [
      "turn-a",
      { kind: "assistant_text", text: "late child", parentToolId: "task-1" },
    ],
    ["turn-b", { kind: "assistant_text", text: "Done." }],
    ["turn-b", TURN_RESULT],
  ]);
  const model = deriveCodingSessionTranscriptModel(transcript, {
    isWorking: false,
  });
  const turnA = model.blocks.find(
    (block) => block.kind === "turn" && block.id === "turn-a",
  );
  assert.ok(turnA, "turn A is in the model");
  assert.equal(turnA.superseded, false, "B's prompt came before A went quiet");
  const entry = turnA.entries.find((e) => e.kind === "subagents");
  // From the transcript alone the spawn still reads running...
  assert.equal(entry.spawns[0].status, "running");

  // ...but the session is idle, so A's settlement is `settled`, and the
  // spawn reads stopped like A's other unended tool rows.
  const idle = resolveCodingSessionTurnSettlement(turnA, "stopped");
  assert.equal(idle, "settled");
  const settled = settleCodingSessionSubagentSpawns(entry.spawns, idle);
  assert.deepEqual(
    settled.map((spawn) => spawn.status),
    ["stopped"],
  );
  assert.equal(
    formatCodingSessionSubagentGroupLabel(settled),
    "Ran 1 subagent",
  );
  assert.equal(summarizeCodingSessionSubagentStatuses(settled), "1 stopped");

  // A session nobody can vouch for: no spinner and no verdict.
  const unsure = resolveCodingSessionTurnSettlement(turnA, "unknown");
  assert.equal(unsure, "unknown");
  assert.deepEqual(
    settleCodingSessionSubagentSpawns(entry.spawns, unsure).map(
      (spawn) => spawn.status,
    ),
    ["unknown"],
  );
});

test("settlement never rewrites a spawn that has its result, nor a live one", () => {
  for (const status of ["done", "failed", "stopped"]) {
    for (const settlement of ["live", "settled", "unknown"]) {
      assert.equal(
        settleCodingSessionSubagentStatus(status, settlement),
        status,
      );
    }
  }
  assert.equal(settleCodingSessionSubagentStatus("running", "live"), "running");
  const spawns = [{ call: {}, children: [], status: "running" }];
  assert.equal(settleCodingSessionSubagentSpawns(spawns, "live"), spawns);
});

test("a steered prompt inside the same turn does not stop a spawn", () => {
  const transcript = projectTurns([
    ["turn-1", { kind: "user_prompt", content: "Go" }],
    ["turn-1", taskCall("task-1", "Still going")],
    ["turn-1", { kind: "user_prompt", content: "also", steered: true }],
  ]);
  assert.equal(
    deriveCodingSessionSubagentPanel([transcript]).rows[0].status,
    "running",
  );
});

test("without turn identity a later prompt or terminal stops the spawn", () => {
  const byPrompt = projectTurns([
    [null, taskCall("task-1", "Orphaned")],
    [null, { kind: "user_prompt", content: "Next" }],
  ]);
  assert.equal(byPrompt[0].turnId, undefined, "fixture carries no turn");
  assert.equal(
    deriveCodingSessionSubagentPanel([byPrompt]).rows[0].status,
    "stopped",
  );
  const byTerminal = projectTurns([
    [null, taskCall("task-1", "Cut off")],
    [null, { kind: "interrupted" }],
  ]);
  assert.equal(
    deriveCodingSessionSubagentPanel([byTerminal]).rows[0].status,
    "stopped",
  );
  const stillOpen = projectTurns([[null, taskCall("task-1", "Live")]]);
  assert.equal(
    deriveCodingSessionSubagentPanel([stillOpen]).rows[0].status,
    "running",
  );
});

test("the group label is T3's; the in-flight count is the status line (SV-06)", () => {
  const spawn = (status) => ({ call: {}, children: [], status });
  assert.equal(
    formatCodingSessionSubagentGroupLabel([spawn("done")]),
    "Ran 1 subagent",
  );
  assert.equal(
    formatCodingSessionSubagentGroupLabel(
      ["done", "failed", "stopped", "done"].map(spawn),
    ),
    "Ran 4 subagents",
  );
  assert.equal(
    formatCodingSessionSubagentGroupLabel(["running", "running"].map(spawn)),
    "Kicked off 2 subagents",
  );
  assert.equal(
    formatCodingSessionSubagentGroupLabel(
      ["running", "failed", "failed"].map(spawn),
    ),
    "Kicked off 3 subagents",
  );
  // How many still work, and the failures, are the line beside the label.
  assert.equal(
    summarizeCodingSessionSubagentStatuses(
      ["running", "failed", "failed"].map(spawn),
    ),
    "1 working · 2 failed",
  );
});

test("the status line counts every spawn once, live work first (SV-06)", () => {
  const spawn = (status) => ({ call: {}, children: [], status });
  assert.equal(
    summarizeCodingSessionSubagentStatuses(
      ["done", "running", "failed", "running"].map(spawn),
    ),
    "2 working · 1 done · 1 failed",
  );
  assert.equal(
    summarizeCodingSessionSubagentStatuses(["done", "done"].map(spawn)),
    "2 done",
  );
  assert.equal(
    summarizeCodingSessionSubagentStatuses(["stopped", "done"].map(spawn)),
    "1 done · 1 stopped",
  );
  assert.equal(summarizeCodingSessionSubagentStatuses([]), "");
});

test("SV-97: only a final assistant message that repeats the report is dropped", () => {
  const msg = (id, text, role = "assistant") => ({
    id,
    type: "message",
    role,
    text,
  });
  const tool = { id: "t", type: "tool" };
  const children = [msg("a", "Looking"), tool, msg("b", " Done: 3 sites. ")];
  assert.deepEqual(
    withoutCodingSessionSubagentEchoedReport(children, "Done: 3 sites.").map(
      (c) => c.id,
    ),
    ["a", "t"],
  );
  // A later tool after the prose does not hide that the prose is the echo.
  assert.deepEqual(
    withoutCodingSessionSubagentEchoedReport(
      [msg("b", "Done"), tool],
      "Done",
    ).map((c) => c.id),
    ["t"],
  );
  // An earlier identical message is not the final one: nothing is dropped.
  const earlier = [msg("a", "Done"), msg("b", "Then more")];
  assert.equal(
    withoutCodingSessionSubagentEchoedReport(earlier, "Done"),
    earlier,
  );
  // No report shown, nothing dropped; a user message is never the echo.
  assert.equal(
    withoutCodingSessionSubagentEchoedReport(children, "  "),
    children,
  );
  const user = [msg("u", "Done", "user")];
  assert.equal(withoutCodingSessionSubagentEchoedReport(user, "Done"), user);
});

test("SV-98: a subagent's model reads by one human name, and unknown stays unknown", () => {
  assert.equal(
    codingSessionSubagentModelName("claude-opus-5-5"),
    "Claude Opus 5.5",
  );
  assert.equal(codingSessionSubagentModelName("opus"), "Opus");
  assert.equal(codingSessionSubagentModelName(null), null);
});
