import assert from "node:assert/strict";
import test from "node:test";

import { summarizeCodingSessionTools } from "./codingSessionTranscriptModel.ts";

// The fold sentence's tool counting (SV-03). Split out of
// `codingSessionTranscriptModel.test.mjs` for the 1000-line ceiling.

const timestamp = "2026-07-30T12:00:00.000Z";

test("SV-03: Claude Bash and ACP execute calls count as commands, failed ones too", () => {
  const call = (id, extra) => ({
    id,
    type: "tool",
    renderClass: "generic",
    descriptor: { renderClass: "generic", label: "Ran tool", preview: null },
    title: id,
    toolName: id,
    buzzToolName: null,
    status: "completed",
    args: {},
    result: "",
    isError: false,
    timestamp,
    startedAt: timestamp,
    completedAt: timestamp,
    ...extra,
  });
  assert.equal(
    summarizeCodingSessionTools([
      // claude-agent-acp titles a Bash call with its command; only the
      // discriminant says what it was.
      call("`ls -la`", { toolKind: "execute" }),
      call("`cargo test`", { toolKind: "execute" }),
      // A failed Bash carries only an `error` descriptor.
      call("Bash", {
        renderClass: "error",
        descriptor: { renderClass: "error", label: "Ran tool failed" },
        args: { command: "false" },
        status: "failed",
        isError: true,
      }),
    ]),
    "Ran 3 commands",
  );
  assert.equal(
    summarizeCodingSessionTools([call("mystery"), call("other")]),
    "Used 2 tools",
  );
  // Claude's bare tool names, with no discriminant, read as T3 reads them.
  assert.equal(
    summarizeCodingSessionTools([call("Bash"), call("Bash", { id: "b2" })]),
    "Ran 2 commands",
  );
});

function call(id, extra = {}) {
  return {
    id,
    type: "tool",
    renderClass: "generic",
    descriptor: { renderClass: "generic", label: "Ran tool", preview: null },
    title: id,
    toolName: id,
    buzzToolName: null,
    status: "completed",
    args: {},
    result: "",
    isError: false,
    timestamp,
    startedAt: timestamp,
    completedAt: timestamp,
    ...extra,
  };
}

const times = (count, make) =>
  Array.from({ length: count }, (_, index) => make(index));

test("T3 wording: one clause per kind, its count and plural", () => {
  const bash = (i) => call(`bash-${i}`, { toolKind: "execute" });
  const edit = (i) =>
    call(`edit-${i}`, { toolKind: "edit", args: { file_path: `f${i}.ts` } });
  assert.equal(summarizeCodingSessionTools([bash(0)]), "Ran 1 command");
  assert.equal(summarizeCodingSessionTools(times(5, bash)), "Ran 5 commands");
  assert.equal(summarizeCodingSessionTools([edit(0)]), "Changed 1 file");
  assert.equal(
    summarizeCodingSessionTools([
      call("g1", { toolKind: "search" }),
      call("g2", { toolName: "Grep", title: "Grep" }),
    ]),
    "Searched code 2 times",
  );
  assert.equal(
    summarizeCodingSessionTools([call("WebSearch")]),
    "Searched the web 1 time",
  );
  assert.equal(
    summarizeCodingSessionTools([
      call("r1", { toolKind: "read", args: { file_path: "a.ts" } }),
    ]),
    "Read 1 file",
  );
  assert.equal(summarizeCodingSessionTools([call("x")]), "Used 1 tool");
});

test("T3 wording: two kinds join with 'and'; more name two and count the rest", () => {
  const bash = (i) => call(`bash-${i}`, { toolKind: "execute" });
  const edit = (i) =>
    call(`edit-${i}`, { toolKind: "edit", args: { file_path: `f${i}.ts` } });
  const read = (i) =>
    call(`read-${i}`, { toolKind: "read", args: { file_path: `r${i}.ts` } });
  assert.equal(
    summarizeCodingSessionTools([edit(0), edit(1), ...times(5, bash)]),
    "Changed 2 files and ran 5 commands",
  );
  // Commands + edits + reads (the SV-03 mixed fold): commands and edits win
  // the two places in first-seen order; the read is still counted.
  assert.equal(
    summarizeCodingSessionTools([edit(0), read(0), ...times(5, bash), edit(1)]),
    "Changed 2 files, ran 5 commands, and performed 1 other action",
  );
  // Reads outrank other tools; two left over are "actions".
  assert.equal(
    summarizeCodingSessionTools([
      call("x"),
      read(0),
      bash(0),
      call("y"),
      call("WebSearch"),
    ]),
    "Read 1 file, ran 1 command, and performed 3 other actions",
  );
});

test("files are counted once each, a call without a path as its own (deliberate difference from T3)", () => {
  const read = (id, path) =>
    call(id, { toolKind: "read", args: path ? { file_path: path } : {} });
  assert.equal(
    summarizeCodingSessionTools([
      read("r1", "a.ts"),
      read("r2", "a.ts"),
      read("r3", "a.ts"),
    ]),
    "Read 1 file",
  );
  assert.equal(
    summarizeCodingSessionTools([read("r1", "a.ts"), read("r2"), read("r3")]),
    "Read 3 files",
  );
});

test("a server-qualified tool name is someone else's tool, not a local read", () => {
  assert.equal(
    summarizeCodingSessionTools([
      call("mcp__github__read_file"),
      call("github.read"),
    ]),
    "Used 2 tools",
  );
});

test("an MCP call's title never stands in for its server-qualified name", () => {
  assert.equal(
    summarizeCodingSessionTools([
      call("m1", { toolName: "mcp__github__read_file", title: "Read file" }),
    ]),
    "Used 1 tool",
  );
  assert.equal(
    summarizeCodingSessionTools([
      call("m2", { toolName: "mcp__shell__run", title: "Bash" }),
    ]),
    "Used 1 tool",
  );
  // With no tool name at all, the title is what the provider said it was.
  assert.equal(
    summarizeCodingSessionTools([call("t1", { toolName: "", title: "Bash" })]),
    "Ran 1 command",
  );
});
