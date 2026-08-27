import assert from "node:assert/strict";
import { test } from "node:test";
import {
  formatCodingSessionExecutionLabel,
  formatCodingSessionModelSummary,
  formatCodingSessionRuntimeLabel,
  splitCodingSessionModelId,
} from "./labels.ts";

test("an agentRef the provider bound wins over runtime and model (D9)", () => {
  assert.equal(
    formatCodingSessionExecutionLabel({
      runtime: "codex-acp",
      model: "gpt-5.4[high]",
      agentRef: "reviewer-bot",
      driver: "provider-a",
    }),
    "reviewer-bot",
  );
});

test("a blank agentRef is not a name, so runtime · model is printed", () => {
  for (const agentRef of [null, "", "   "]) {
    assert.equal(
      formatCodingSessionExecutionLabel({
        runtime: "codex-acp",
        model: "gpt-5.4[high]",
        agentRef,
        driver: "provider-a",
      }),
      "Codex · gpt-5.4 · High",
    );
  }
});

test("a runtime with no model prints the runtime alone", () => {
  assert.equal(
    formatCodingSessionExecutionLabel({
      runtime: "claude-agent-acp",
      model: null,
      agentRef: null,
      driver: "provider-a",
    }),
    "Claude Code",
  );
});

test("a null runtime falls back to the driver, never to an empty chip", () => {
  assert.equal(
    formatCodingSessionExecutionLabel({
      runtime: null,
      model: null,
      agentRef: null,
      driver: "codex-acp",
    }),
    "Codex",
  );
  assert.equal(
    formatCodingSessionExecutionLabel({
      runtime: null,
      model: null,
      agentRef: null,
      driver: null,
    }),
    "Coding Session",
  );
});

test("the two known runtimes have fixed names, and unknown ones are title-cased", () => {
  assert.equal(formatCodingSessionRuntimeLabel("codex-acp"), "Codex");
  assert.equal(
    formatCodingSessionRuntimeLabel("claude-agent-acp"),
    "Claude Code",
  );
  assert.equal(
    formatCodingSessionRuntimeLabel("claude_agent_acp"),
    "Claude Code",
  );
  assert.equal(formatCodingSessionRuntimeLabel("claude-code"), "Claude Code");
  assert.equal(formatCodingSessionRuntimeLabel("goose-acp"), "Goose Acp");
});

test("a model id is decoded bracket by bracket, never guessed at", () => {
  assert.deepEqual(splitCodingSessionModelId("gpt-5.4[high][1m]"), {
    model: "gpt-5.4",
    thinking: "high",
    context: "1m",
  });
  assert.deepEqual(splitCodingSessionModelId("opus[1m]"), {
    model: "opus",
    thinking: null,
    context: "1m",
  });
  assert.deepEqual(splitCodingSessionModelId("model-x[experimental]"), {
    model: "model-x[experimental]",
    thinking: null,
    context: null,
  });
});

test("a model summary composes the decoded dimensions, in order", () => {
  assert.equal(
    formatCodingSessionModelSummary("gpt-5.4[high][1m]"),
    "gpt-5.4 · High · 1M",
  );
  assert.equal(formatCodingSessionModelSummary("gpt-5.4"), "gpt-5.4");
});

test("the full execution chip composes runtime and every model dimension", () => {
  assert.equal(
    formatCodingSessionExecutionLabel({
      runtime: "claude-agent-acp",
      model: "opus[max][1m]",
      agentRef: null,
      driver: null,
    }),
    "Claude Code · opus · Max · 1M",
  );
});
