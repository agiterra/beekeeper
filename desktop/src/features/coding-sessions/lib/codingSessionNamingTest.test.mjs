import assert from "node:assert/strict";
import test from "node:test";

import {
  codingSessionNamingTestSummary,
  formatNamingLatency,
} from "./codingSessionNamingTest.ts";

test("latency reads at the precision it deserves", () => {
  assert.equal(formatNamingLatency(0), "0ms");
  assert.equal(formatNamingLatency(840), "840ms");
  assert.equal(formatNamingLatency(1240), "1.2s");
  assert.equal(formatNamingLatency(9949), "9.9s");
  assert.equal(formatNamingLatency(11400), "11s");
  // A negative or fractional reading is still a string, not NaN.
  assert.equal(formatNamingLatency(-5), "0ms");
});

test("nothing tested yet says nothing", () => {
  const summary = codingSessionNamingTestSummary({
    error: null,
    isPending: false,
    result: null,
  });
  assert.equal(summary.tone, "none");
  assert.equal(summary.headline, null);
});

test("a successful test quotes the name it got back", () => {
  const summary = codingSessionNamingTestSummary({
    error: null,
    isPending: false,
    result: { name: "Fix the push timeout", elapsedMs: 1240 },
  });
  assert.equal(summary.tone, "ok");
  assert.match(summary.headline, /Fix the push timeout/);
  assert.match(summary.headline, /1\.2s/);
  // Comfortably inside the cadence: no warning to give.
  assert.equal(summary.detail, null);
});

test("a namer slower than the cadence that asks it says so", () => {
  const summary = codingSessionNamingTestSummary({
    error: null,
    isPending: false,
    result: { name: "Fix the push timeout", elapsedMs: 8000 },
  });
  assert.equal(summary.tone, "ok");
  assert.match(summary.detail, /slower than the every-five-seconds cadence/);
});

test("a failure is reported verbatim rather than as 'something went wrong'", () => {
  const summary = codingSessionNamingTestSummary({
    error: "The Anthropic API answered 401 Unauthorized",
    isPending: false,
    result: null,
  });
  assert.equal(summary.tone, "failed");
  assert.match(summary.headline, /401/);
});

test("a run in flight outranks the previous run's result", () => {
  const summary = codingSessionNamingTestSummary({
    error: "stale failure",
    isPending: true,
    result: { name: "stale name", elapsedMs: 100 },
  });
  assert.equal(summary.tone, "pending");
  assert.doesNotMatch(summary.headline, /stale/);
});
