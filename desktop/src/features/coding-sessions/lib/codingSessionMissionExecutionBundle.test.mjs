import assert from "node:assert/strict";
import test from "node:test";

import {
  isCodingSessionMissionExecutionItem,
  summarizeCodingSessionMissionExecution,
} from "./codingSessionMissionExecutionBundle.ts";

function tool(id, renderClass) {
  return {
    id,
    type: "tool",
    renderClass,
    descriptor: { renderClass, label: renderClass, preview: null },
    title: id,
    toolName: id,
    beekeeperToolName: null,
    status: "completed",
    args: {},
    result: "",
    isError: false,
    timestamp: "2026-09-01T00:00:00.000Z",
  };
}

function message(id) {
  return {
    id,
    type: "message",
    renderClass: "message",
    role: "assistant",
    title: "Assistant",
    text: id,
    timestamp: "2026-09-01T00:00:00.000Z",
  };
}

test("R2 C2: the breakdown is the classifier's verbs, in print order", () => {
  const summary = summarizeCodingSessionMissionExecution([
    message("prose"),
    tool("e1", "file-edit"),
    tool("s1", "shell"),
    tool("r1", "file-read"),
    tool("e2", "file-edit"),
    tool("s2", "shell"),
    tool("k1", "skill-read"),
  ]);
  assert.equal(summary.count, 6);
  assert.equal(summary.label, "6 execution events");
  assert.deepEqual(summary.breakdown, [
    { verb: "Terminal", count: 2 },
    { verb: "Read", count: 2 },
    { verb: "Edit", count: 2 },
  ]);
});

test("R2 C2: one event is singular", () => {
  const summary = summarizeCodingSessionMissionExecution([tool("s", "shell")]);
  assert.equal(summary.label, "1 execution event");
  assert.deepEqual(summary.breakdown, [{ verb: "Terminal", count: 1 }]);
});

test("R2 C2: a verb with no events is omitted, never printed as zero", () => {
  const summary = summarizeCodingSessionMissionExecution([
    tool("s", "shell"),
    tool("s2", "shell"),
  ]);
  assert.deepEqual(summary.breakdown, [{ verb: "Terminal", count: 2 }]);
  assert.equal(
    summary.breakdown.some((entry) => entry.count === 0),
    false,
  );
});

test("R2 C2: reversibility — the verbs always sum to the count", () => {
  const items = [
    message("prose"),
    tool("a", "shell"),
    tool("b", "generic"),
    tool("c", "raw-rail"),
    tool("d", "file-edit"),
  ];
  const summary = summarizeCodingSessionMissionExecution(items);
  assert.equal(
    summary.count,
    items.filter(isCodingSessionMissionExecutionItem).length,
  );
  assert.equal(
    summary.breakdown.reduce((total, entry) => total + entry.count, 0),
    summary.count,
  );
  // A call the classifier could not place keeps the classifier's own label
  // ("Tool", "Raw event") — never an invented word, and never dropped.
  assert.ok(summary.breakdown.some((entry) => entry.verb === "Tool"));
  assert.ok(summary.breakdown.some((entry) => entry.verb === "Raw event"));
});

test("R2 C2: a block with no tool items has no bundle", () => {
  const summary = summarizeCodingSessionMissionExecution([message("only")]);
  assert.equal(summary.count, 0);
  assert.deepEqual(summary.breakdown, []);
});

test("R2 C2: an unplaceable call keeps the classifier's own word", () => {
  // A vendor-prefixed MCP call with no descriptor, no `toolKind`, and a name
  // no rule matches: nothing on the wire says what it did, so the bundle says
  // `Tool` — the same word the expanded row uses. Amended in batch 2: a
  // *named* seat tool (`Read`, `Bash`, …) no longer lands here, because the
  // name is exactly the fact that was being thrown away (finding 9).
  const summary = summarizeCodingSessionMissionExecution([
    {
      id: "mcp-1",
      type: "tool",
      renderClass: "generic",
      title: "mcp__vendor__do_a_thing",
      toolName: "mcp__vendor__do_a_thing",
      beekeeperToolName: null,
      status: "completed",
      args: { path: "desktop/src/app/App.tsx" },
      result: "",
      isError: false,
      timestamp: "2026-09-01T00:00:00.000Z",
    },
  ]);
  assert.equal(summary.count, 1);
  assert.deepEqual(summary.breakdown, [{ verb: "Tool", count: 1 }]);
});

/**
 * A Claude-Code-style seat tool: the classifier has no rule for these names,
 * so `descriptor.renderClass` is `generic` — exactly what the live run
 * rendered as `Tool 49`.
 */
function agentTool(id, toolName, toolKind) {
  return {
    id,
    type: "tool",
    renderClass: "generic",
    descriptor: { renderClass: "generic", label: "Ran tool", preview: null },
    title: toolName,
    toolName,
    beekeeperToolName: null,
    toolKind: toolKind ?? null,
    status: "completed",
    args: {},
    result: "",
    isError: false,
    timestamp: "2026-09-01T00:00:00.000Z",
  };
}

test("A3.3: a seat's Bash/Read/Edit/Grep bundle reads Terminal · Read · Edit · Search", () => {
  const summary = summarizeCodingSessionMissionExecution([
    ...Array.from({ length: 4 }, (_, index) =>
      agentTool(`bash-${index}`, "Bash"),
    ),
    ...Array.from({ length: 2 }, (_, index) =>
      agentTool(`read-${index}`, "Read"),
    ),
    ...Array.from({ length: 3 }, (_, index) =>
      agentTool(`edit-${index}`, "Edit"),
    ),
    ...Array.from({ length: 2 }, (_, index) =>
      agentTool(`grep-${index}`, "Grep"),
    ),
  ]);
  assert.equal(summary.count, 11);
  assert.deepEqual(summary.breakdown, [
    { verb: "Terminal", count: 4 },
    { verb: "Read", count: 2 },
    { verb: "Edit", count: 3 },
    { verb: "Search", count: 2 },
  ]);
});

test("A3.3: ACP's own tool discriminant names a call the name rule misses", () => {
  const summary = summarizeCodingSessionMissionExecution([
    agentTool("a", "Preparing file…", "edit"),
    agentTool("b", "mcp__thing__lookup", "search"),
  ]);
  assert.deepEqual(summary.breakdown, [
    { verb: "Edit", count: 1 },
    { verb: "Search", count: 1 },
  ]);
});

test("A3.3: relay ops keep Relay and a genuinely unnamed call keeps Tool", () => {
  const summary = summarizeCodingSessionMissionExecution([
    tool("relay", "relay-op"),
    agentTool("mystery", "mcp__vendor__do_a_thing"),
  ]);
  assert.deepEqual(summary.breakdown, [
    { verb: "Relay", count: 1 },
    { verb: "Tool", count: 1 },
  ]);
});
