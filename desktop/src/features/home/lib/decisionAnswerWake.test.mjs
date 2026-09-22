import assert from "node:assert/strict";
import { test } from "node:test";

import {
  resolveAskerTarget,
  wakeAskerForAnswer,
} from "@/features/home/lib/decisionAnswerWake";

const ASKER = "a".repeat(64);
const SESSION = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const target = (generation) => ({
  driver: "claude-agent-acp",
  instanceId: "lead",
  sessionId: "s",
  generation,
});
const entry = (over) => ({
  sessionRef: SESSION,
  agentRef: ASKER,
  role: "lead",
  commandTarget: target(1),
  ...over,
});

test("the asker's newest generation in this session is the wake target", () => {
  const resolved = resolveAskerTarget(
    [
      entry({}),
      entry({ commandTarget: target(2) }),
      entry({ sessionRef: "other", commandTarget: target(9) }),
      entry({ agentRef: "b".repeat(64), commandTarget: target(9) }),
    ],
    SESSION,
    ASKER,
  );
  assert.equal(resolved.target.generation, 2);
  assert.equal(resolveAskerTarget([], SESSION, ASKER), null);
});

test("one boundary 44220 names the answer; no seat and refusal are said", async () => {
  const calls = [];
  const sent = await wakeAskerForAnswer({
    channelRef: "c",
    answerEventId: "e".repeat(64),
    asker: { target: target(1), role: "lead" },
    askerLabel: "aaaa",
    publish: async (input) => {
      calls.push(input);
      return {
        eventId: "f".repeat(64),
        kind: 44220,
        commandId: input.commandId,
      };
    },
  });
  assert.equal(sent.status, "sent");
  assert.equal(calls.length, 1);
  assert.equal(calls[0].deliver, "boundary");
  assert.deepEqual(JSON.parse(calls[0].text), {
    operationId: "e".repeat(64),
    type: "decision.answer",
  });
  assert.match(calls[0].commandId, /^team-wake-v1:e{64}:[0-9a-f]{24}$/);

  const none = await wakeAskerForAnswer({
    channelRef: "c",
    answerEventId: "e".repeat(64),
    asker: null,
    askerLabel: "aaaa",
  });
  assert.equal(none.status, "no-seat");
  const failed = await wakeAskerForAnswer({
    channelRef: "c",
    answerEventId: "e".repeat(64),
    asker: { target: target(1), role: "lead" },
    askerLabel: "aaaa",
    publish: async () => {
      throw new Error("not a member");
    },
  });
  assert.equal(failed.status, "failed");
  assert.match(failed.message, /answer was published.*not a member/);
});
