import assert from "node:assert/strict";
import { test } from "node:test";

import {
  finalizeEvent,
  generateSecretKey,
  getPublicKey,
} from "nostr-tools/pure";

import { KIND_CODING_SESSION_GOAL } from "../../../shared/constants/kinds.ts";
import {
  MAX_ONE_LINE_GOAL_CHARS,
  autoSummarizeCodingSessionGoal,
  codingSessionAutoGoalSentence,
  codingSessionGoalNeedsSummary,
} from "./codingSessionAutoGoal.ts";
import { buildCodingSessionGoalEvent } from "./codingSessionGoal.ts";

const CHANNEL_ID = "3d2a7b18-9b7a-4a41-9a86-6a52a1c0b7e1";
const SESSION_REF = "22222222-2222-4222-8222-222222222222";
const FOUNDER_SECRET = generateSecretKey();
const FOUNDER = getPublicKey(FOUNDER_SECRET);
const ON = {
  provider: "anthropic",
  baseUrl: "",
  model: "claude-haiku-4-5",
  hasApiKey: true,
};
const LONG_PROMPT = [
  "Move the create dialog onto the founded page, keep Solo and Team as a switch at the top,",
  "publish the name and the prompt when each field is left, and make Start flush both before it",
  "runs the create. Then rebuild the app and clean up the worktrees.",
].join("\n");

function signedGoal(content, createdAt = 1_700_000_000) {
  const built = buildCodingSessionGoalEvent({
    channelId: CHANNEL_ID,
    content,
    sessionRef: SESSION_REF,
  });
  return finalizeEvent(
    {
      kind: KIND_CODING_SESSION_GOAL,
      created_at: createdAt,
      tags: built.tags,
      content: built.content,
    },
    FOUNDER_SECRET,
  );
}

function deps({
  settings = ON,
  generated = "Move session setup onto the founded page.",
  goals = [signedGoal(LONG_PROMPT)],
  publishFails = null,
} = {}) {
  const calls = [];
  return {
    calls,
    deps: {
      getSettings: async () => settings,
      generate: async (firstMessage) => {
        calls.push(["generate", firstMessage]);
        if (generated instanceof Error) throw generated;
        return generated;
      },
      readGoals: async (filter) => {
        calls.push(["readGoals", filter]);
        return goals;
      },
      publishGoal: async (input) => {
        calls.push(["publish", input]);
        if (publishFails) throw publishFails;
        return {};
      },
    },
  };
}

const INPUT = {
  channelId: CHANNEL_ID,
  sessionRef: SESSION_REF,
  founderPubkey: FOUNDER,
  firstMessage: LONG_PROMPT,
};

test("a long Solo prompt becomes a one-line goal after Start", async () => {
  const d = deps();
  assert.deepEqual(await autoSummarizeCodingSessionGoal(INPUT, d.deps), {
    kind: "published",
    goal: "Move session setup onto the founded page.",
  });
  assert.deepEqual(
    d.calls.map(([name]) => name),
    ["generate", "readGoals", "publish"],
  );
  assert.deepEqual(d.calls[2][1], {
    channelId: CHANNEL_ID,
    content: "Move session setup onto the founded page.",
    sessionRef: SESSION_REF,
  });
  // The read is scoped to this session's founder-signed goals.
  assert.deepEqual(d.calls[1][1]["#d"], [SESSION_REF]);
  assert.deepEqual(d.calls[1][1].authors, [FOUNDER]);
});

test("no namer, or a prompt that already fits one line, changes nothing", async () => {
  for (const settings of [null, { ...ON, provider: "off" }]) {
    const d = deps({ settings });
    assert.deepEqual(await autoSummarizeCodingSessionGoal(INPUT, d.deps), {
      kind: "off",
    });
    assert.deepEqual(d.calls, []);
  }
  const short = deps();
  assert.deepEqual(
    await autoSummarizeCodingSessionGoal(
      { ...INPUT, firstMessage: "Fix the reconnect bug." },
      short.deps,
    ),
    { kind: "already-short" },
  );
  assert.deepEqual(short.calls, []);
});

test("a goal somebody set meanwhile is kept; the summary is not published over it", async () => {
  const d = deps({ goals: [signedGoal("Typed on the phone", 1_700_000_100)] });
  assert.deepEqual(await autoSummarizeCodingSessionGoal(INPUT, d.deps), {
    kind: "changed-meanwhile",
  });
  assert.equal(
    d.calls.some(([name]) => name === "publish"),
    false,
  );
  // No goal on the wire at all (the pre-Start publish was refused): nothing is invented.
  const none = deps({ goals: [] });
  assert.deepEqual(await autoSummarizeCodingSessionGoal(INPUT, none.deps), {
    kind: "changed-meanwhile",
  });
});

test("an empty answer, a model refusal and a relay refusal are stated, and the prompt stays the goal", async () => {
  assert.deepEqual(
    await autoSummarizeCodingSessionGoal(INPUT, deps({ generated: " " }).deps),
    { kind: "empty" },
  );
  assert.deepEqual(
    await autoSummarizeCodingSessionGoal(
      INPUT,
      deps({ generated: new Error("401 from the namer") }).deps,
    ),
    { kind: "failed", reason: "401 from the namer" },
  );
  assert.deepEqual(
    await autoSummarizeCodingSessionGoal(
      INPUT,
      deps({ publishFails: new Error("relay: blocked") }).deps,
    ),
    { kind: "failed", reason: "relay: blocked" },
  );
});

test("what counts as needing a summary, and the sentence under the prompt", () => {
  assert.equal(codingSessionGoalNeedsSummary("one line, short"), false);
  assert.equal(codingSessionGoalNeedsSummary("two\nlines"), true);
  assert.equal(
    codingSessionGoalNeedsSummary("x".repeat(MAX_ONE_LINE_GOAL_CHARS + 1)),
    true,
  );
  assert.equal(codingSessionAutoGoalSentence(null), null);
  assert.equal(codingSessionAutoGoalSentence({ ...ON, provider: "off" }), null);
  assert.match(
    codingSessionAutoGoalSentence(ON),
    /one-line summary of this prompt \(claude-haiku-4-5\)/,
  );
});
