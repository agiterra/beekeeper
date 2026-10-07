import assert from "node:assert/strict";
import test from "node:test";

import {
  buildCodingSessionCheckpointFilter,
  buildCodingSessionCheckpointLiveFilter,
  codingSessionCheckpointGenerations,
  CODING_SESSION_CHECKPOINT_HISTORY_LIMIT,
  createCodingSessionCheckpointClassifier,
  mergeCodingSessionCheckpointEvent,
  readCodingSessionCheckpoints,
} from "./useCodingSessionCheckpoints.ts";
import { foldCodingSessionCheckpoints } from "../lib/codingSessionCheckpoints.ts";
import {
  CHANNEL_ID,
  checkpointEvent,
  payload,
  PROVIDER_PUBKEY,
  scope,
  TARGET,
  TARGET_KEY,
} from "../lib/codingSessionCheckpointFixtures.testFixtures.mjs";

function record(overrides = {}) {
  return {
    generationId: "gen-1",
    providerAuthorityPubkey: PROVIDER_PUBKEY,
    commandTarget: TARGET,
    transcript: [{ id: "item" }],
    ...overrides,
  };
}

test("only generations with transcript items lend their signer", () => {
  const generations = codingSessionCheckpointGenerations({
    executions: [
      {
        activeGeneration: record(),
        priorGenerations: [
          record({ generationId: "gen-0", transcript: [] }),
          record({ generationId: "gen-x", commandTarget: null }),
        ],
      },
    ],
  });
  assert.deepEqual(generations, [
    {
      generationId: "gen-1",
      targetKey: TARGET_KEY,
      signerPubkey: PROVIDER_PUBKEY,
    },
  ]);
  assert.deepEqual(codingSessionCheckpointGenerations(null), []);
});

test("filters carry explicit kinds, #h and authors; live is since-bounded", () => {
  assert.deepEqual(
    buildCodingSessionCheckpointFilter(CHANNEL_ID, [PROVIDER_PUBKEY]),
    {
      kinds: [44231],
      "#h": [CHANNEL_ID],
      authors: [PROVIDER_PUBKEY],
      limit: CODING_SESSION_CHECKPOINT_HISTORY_LIMIT,
    },
  );
  assert.deepEqual(
    buildCodingSessionCheckpointLiveFilter(CHANNEL_ID, [PROVIDER_PUBKEY], 99),
    {
      kinds: [44231],
      "#h": [CHANNEL_ID],
      authors: [PROVIDER_PUBKEY],
      since: 99,
      limit: 0,
    },
  );
});

test("a read keeps only 44231 and says when it hit its ceiling", async () => {
  const event = checkpointEvent();
  const calls = [];
  const data = await readCodingSessionCheckpoints({
    channelId: CHANNEL_ID,
    authors: [PROVIDER_PUBKEY],
    client: {
      async fetchEventsBatch(filters) {
        calls.push(filters);
        return [event, { ...event, id: "x", kind: 1 }];
      },
    },
  });
  assert.equal(calls[0][0].kinds[0], 44231);
  assert.deepEqual(data.events, [event]);
  assert.equal(data.capped, false);
});

test("a live event merges once; a duplicate leaves the data identical", () => {
  const first = checkpointEvent();
  const data = { events: [first], capped: false };
  assert.equal(mergeCodingSessionCheckpointEvent(data, first), data);
  const second = checkpointEvent(
    payload({ turnId: "turn-2", coverage: { fromSeq: 60, throughSeq: 70 } }),
  );
  const merged = mergeCodingSessionCheckpointEvent(data, second);
  assert.equal(merged.events.length, 2);
  assert.equal(mergeCodingSessionCheckpointEvent(undefined, second), undefined);
});

test("re-folding hands back the same entry objects (SV-100 identity)", () => {
  const classify = createCodingSessionCheckpointClassifier();
  const shared = scope();
  const first = checkpointEvent();
  const before = foldCodingSessionCheckpoints([first], shared, classify);
  const later = checkpointEvent(
    payload({ turnId: "turn-2", coverage: { fromSeq: 60, throughSeq: 70 } }),
  );
  const after = foldCodingSessionCheckpoints([first, later], shared, classify);
  const [b] = before.byScope.values();
  const [a] = after.byScope.values();
  assert.equal(a.byTurnId.get("turn-1"), b.byTurnId.get("turn-1"));
  // A new scope forgets the verdicts: a signer's standing may have changed.
  const fresh = foldCodingSessionCheckpoints([first], scope([]), classify);
  assert.equal(fresh.byScope.size, 0);
  assert.equal(fresh.rejected.unknownTarget, 1);
});
