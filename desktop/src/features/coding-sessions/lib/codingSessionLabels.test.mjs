import assert from "node:assert/strict";
import test from "node:test";

import {
  formatCodingSessionExecutionLabel,
  formatCodingSessionRoleLabel,
} from "./codingSessionLabels.ts";

const ACTOR =
  "aa11bb22cc33dd44ee55ff66aa77bb88cc99dd00ee11ff22aa33bb44cc55dd66";

test("an execution with no actor labels itself exactly as it always did", () => {
  assert.deepEqual(
    formatCodingSessionExecutionLabel({
      agentRef: null,
      role: null,
      runtime: "claude-agent-acp",
      model: "sonnet",
    }),
    { primary: "Claude Code · sonnet", secondary: null },
  );
  // A record projected before the seat keys existed carries neither.
  assert.deepEqual(
    formatCodingSessionExecutionLabel({
      agentRef: undefined,
      role: undefined,
      runtime: "codex-acp",
      model: null,
    }),
    { primary: "Codex", secondary: null },
  );
});

test("a seated execution leads with the agent and its role", () => {
  assert.deepEqual(
    formatCodingSessionExecutionLabel({
      agentRef: ACTOR,
      role: "builder",
      agentDisplayName: "Ada",
      runtime: "codex-acp",
      model: "gpt-5.6-sol",
    }),
    // Runtime and model are demoted, not dropped.
    { primary: "Ada · Builder", secondary: "Codex · gpt-5.6-sol" },
  );
});

test("an unresolved actor name falls back to the role, never to a guess", () => {
  const label = formatCodingSessionExecutionLabel({
    agentRef: ACTOR,
    role: "verifier",
    agentDisplayName: null,
    runtime: "claude-agent-acp",
    model: "opus",
  });
  assert.equal(label.primary, "Verifier");
  assert.equal(label.secondary, "Claude Code · opus");
  assert.ok(!label.primary.includes(ACTOR.slice(0, 8)));
});

test("half a seat is not a seat", () => {
  // An actor with no role, or a role with no actor, labels as unseated —
  // both are refused on the wire, and a projection must not invent one.
  assert.equal(
    formatCodingSessionExecutionLabel({
      agentRef: ACTOR,
      role: null,
      runtime: "codex-acp",
      model: null,
    }).primary,
    "Codex",
  );
  assert.equal(
    formatCodingSessionExecutionLabel({
      agentRef: null,
      role: "lead",
      runtime: "codex-acp",
      model: null,
    }).primary,
    "Codex",
  );
});

test("role slugs read as words", () => {
  assert.equal(formatCodingSessionRoleLabel("lead"), "Lead");
  assert.equal(formatCodingSessionRoleLabel("code-reviewer"), "Code Reviewer");
});
