import assert from "node:assert/strict";
import test from "node:test";

import { readPendingLaneDEvidence } from "./useCodingSessionMissionSurface.tsx";

test("U-F8: absent Lane D evidence keeps a stable identity across reads", () => {
  const first = readPendingLaneDEvidence({});
  const second = readPendingLaneDEvidence({ goal: { kind: "absent" } });
  // Fresh `[]` literals here churn the hook's result identity on every
  // evidence change and defeat every downstream memo.
  assert.equal(first.transactions, second.transactions);
  assert.equal(first.unseatedReportEventIds, second.unseatedReportEventIds);
  assert.deepEqual(first.transactions, []);
  assert.deepEqual(first.unseatedReportEventIds, []);
});

test("U-F8: supplied Lane D evidence is passed through by reference", () => {
  const transactions = [];
  const unseatedReportEventIds = ["a".repeat(64)];
  const read = readPendingLaneDEvidence({
    transactions,
    unseatedReportEventIds,
  });
  assert.equal(read.transactions, transactions);
  assert.equal(read.unseatedReportEventIds, unseatedReportEventIds);
});
