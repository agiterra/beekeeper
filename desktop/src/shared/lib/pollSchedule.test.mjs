import assert from "node:assert/strict";
import test from "node:test";

import { fnv1a32, phaseJitteredPeriodMs, phaseOffset } from "./pollSchedule.ts";

test("fnv1a32 matches the reference vectors", () => {
  assert.equal(fnv1a32(""), 0x811c9dc5);
  assert.equal(fnv1a32("a"), 0xe40c292c);
  assert.equal(fnv1a32("foobar"), 0xbf9cf968);
});

test("phaseOffset is deterministic for (pubkey, key) and lands inside the period", () => {
  const a = phaseOffset("presence", 30_000, "pubkey-a");
  assert.equal(phaseOffset("presence", 30_000, "pubkey-a"), a);
  assert.ok(a >= 0 && a < 30_000);
  assert.notEqual(
    a,
    phaseOffset("presence", 30_000, "pubkey-b"),
    "another identity gets another phase",
  );
  assert.notEqual(
    a,
    phaseOffset("terminals", 30_000, "pubkey-a"),
    "another poll gets another phase",
  );
  assert.equal(phaseOffset("presence", 0, "pubkey-a"), 0);
});

test("phaseJitteredPeriodMs stays within ±10 % of the nominal period", () => {
  for (const key of ["a", "b", "c", "presence", "terminals", "pulse"]) {
    const period = phaseJitteredPeriodMs(key, 30_000, "pubkey");
    assert.ok(period >= 27_000 && period <= 33_000, `${key}: ${period}`);
    assert.equal(phaseJitteredPeriodMs(key, 30_000, "pubkey"), period);
  }
});
