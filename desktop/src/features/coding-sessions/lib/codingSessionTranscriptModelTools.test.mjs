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
    "Ran 2 tool calls",
  );
});
