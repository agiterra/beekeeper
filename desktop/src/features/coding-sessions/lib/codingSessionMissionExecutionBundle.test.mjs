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
    buzzToolName: null,
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
  // The e2e fixture's `Read` tool arrives with no descriptor and a `generic`
  // class — the wire did not say enough to call it a file read, and neither
  // does the bundle. `Tool` is what the expanded row calls it too.
  const summary = summarizeCodingSessionMissionExecution([
    {
      id: "read-1",
      type: "tool",
      renderClass: "generic",
      title: "Read",
      toolName: "Read",
      buzzToolName: null,
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
