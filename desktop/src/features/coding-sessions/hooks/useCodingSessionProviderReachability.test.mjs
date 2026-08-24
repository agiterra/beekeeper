import assert from "node:assert/strict";
import test from "node:test";

import { buildCodingSessionTargetKey } from "../lib/codingSessionCommand.ts";
import { codingSessionReachabilityFromRead } from "./useCodingSessionProviderReachability.ts";

const TARGET = {
  driver: "claude-agent-acp",
  instanceId: "3728312ccac0f9fb",
  sessionId: "ca189986-8140-4479-9f48-fc98784c4380",
  generation: 1,
};
const OTHER_TARGET = { ...TARGET, generation: 2 };

function read(reachability, target = TARGET) {
  return {
    asOf: 1_787_538_000,
    complete: true,
    errors: [],
    ambiguities: [],
    channelsBySession: new Map(),
    sessions: [
      {
        sessionKey: "session-1",
        sessionRef: null,
        name: null,
        goal: null,
        lifecycle: "open",
        coordinationState: "open_unverified",
        latestObservationAt: null,
        observedAgeSeconds: null,
        sourceEventIds: [],
        generations: [
          { targetKey: buildCodingSessionTargetKey(target), reachability },
        ],
      },
    ],
  };
}

test("a live lease on this exact generation is the only thing that says reachable", () => {
  assert.deepEqual(
    codingSessionReachabilityFromRead(
      read("provider_reachable"),
      buildCodingSessionTargetKey(TARGET),
    ),
    { known: true, reachable: true },
  );
});

test("a proven generation with a lapsed lease is proven unreachable", () => {
  for (const reachability of ["unverified", "terminal"]) {
    assert.deepEqual(
      codingSessionReachabilityFromRead(
        read(reachability),
        buildCodingSessionTargetKey(TARGET),
      ),
      { known: true, reachable: false },
      `${reachability} must not read as reachable`,
    );
  }
});

test("no read, no target, and an unseen generation all claim nothing", () => {
  const key = buildCodingSessionTargetKey(TARGET);
  assert.deepEqual(codingSessionReachabilityFromRead(null, key), {
    known: false,
  });
  assert.deepEqual(
    codingSessionReachabilityFromRead(read("provider_reachable"), null),
    { known: false },
  );
  // The read proved a *different* generation of the same execution. Absence of
  // evidence about this one is not evidence that nobody answers for it.
  assert.deepEqual(
    codingSessionReachabilityFromRead(
      read("provider_reachable", OTHER_TARGET),
      key,
    ),
    { known: false },
  );
});

test("generation 2's lease never answers for generation 1", () => {
  assert.deepEqual(
    codingSessionReachabilityFromRead(
      read("provider_reachable", OTHER_TARGET),
      buildCodingSessionTargetKey(OTHER_TARGET),
    ),
    { known: true, reachable: true },
  );
});
