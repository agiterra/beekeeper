import assert from "node:assert/strict";
import test from "node:test";

import {
  CODING_SESSION_UNKNOWN_ACTOR,
  buildCodingSessionTurnByline,
} from "./codingSessionTurnByline.ts";

const KEYSTONE = "ede63017".repeat(8);

test("a seated block names the actor and its role, never the provider key", () => {
  const byline = buildCodingSessionTurnByline({
    agentDisplayName: "Keystone",
    agentRef: KEYSTONE,
    generation: 1,
    label: "Keystone · Lead",
    model: "claude-opus-5",
    providerInstanceRef: "claude-cc-1",
    role: "lead",
    runtime: "claude-agent-acp",
  });

  assert.equal(byline.name, "Keystone · Lead");
  assert.equal(byline.detail, null);
  assert.equal(byline.via, "via claude-cc-1");
  assert.equal(
    byline.screenReader,
    "Response from Keystone, Lead, generation 1, via claude-cc-1.",
  );
});

test("a seat whose profile has not been read falls back to role and runtime", () => {
  const byline = buildCodingSessionTurnByline({
    agentDisplayName: null,
    agentRef: KEYSTONE,
    generation: 2,
    label: "Lead",
    model: "claude-opus-5",
    providerInstanceRef: null,
    role: "lead",
    runtime: "claude-agent-acp",
  });

  assert.equal(byline.name, "Lead");
  assert.equal(byline.detail, "Claude Code · claude-opus-5");
  assert.equal(byline.via, null);
  assert.equal(byline.screenReader, "Response from Lead, generation 2.");
});

test("an unseated execution keeps naming its runtime and model", () => {
  const byline = buildCodingSessionTurnByline({
    agentRef: null,
    generation: 1,
    label: "Codex · gpt-5.6-sol",
    model: "gpt-5.6-sol",
    role: null,
    runtime: "codex-acp",
  });

  assert.equal(byline.name, "Codex · gpt-5.6-sol");
  assert.equal(byline.detail, null);
  assert.equal(
    byline.screenReader,
    "Response from Codex, gpt-5.6-sol, generation 1.",
  );
});

test("nothing on the wire names the actor, so the byline says so", () => {
  const byline = buildCodingSessionTurnByline({
    agentRef: null,
    generation: 1,
    label: null,
    model: null,
    role: null,
    runtime: null,
  });

  assert.equal(byline.name, CODING_SESSION_UNKNOWN_ACTOR);
  assert.equal(byline.name, "unknown actor");
  assert.equal(byline.detail, null);
  assert.equal(
    byline.screenReader,
    "Response from unknown actor, generation 1.",
  );
});

test("a 64-hex label is a signer key, not a name, and is never the headline", () => {
  const byline = buildCodingSessionTurnByline({
    agentRef: null,
    generation: 1,
    label: "8b830553…fbc0",
    model: null,
    role: null,
    runtime: null,
  });

  assert.equal(byline.name, CODING_SESSION_UNKNOWN_ACTOR);
  assert.doesNotMatch(byline.name, /8b830553/);
  assert.doesNotMatch(byline.screenReader, /8b830553/);
});
