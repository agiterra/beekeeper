import assert from "node:assert/strict";
import test from "node:test";

import {
  codingSessionDiscoveryRetryDelayMs,
  createCodingSessionDiscoveryController,
  isRetryableCodingSessionDiscoveryError,
} from "./codingSessionDiscoveryRetry.ts";

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((resolveValue, rejectValue) => {
    resolve = resolveValue;
    reject = rejectValue;
  });
  return { promise, reject, resolve };
}

async function flushMicrotasks() {
  await Promise.resolve();
  await Promise.resolve();
}

test("controller coalesces reconnect refreshes while one history request is active", async () => {
  const load = deferred();
  let loadCount = 0;
  const successes = [];
  const controller = createCodingSessionDiscoveryController({
    load() {
      loadCount += 1;
      return load.promise;
    },
    onAttemptStart() {},
    onSuccess: (value) => successes.push(value),
    onError() {},
    retrySeed: "channel-a",
  });

  assert.equal(controller.request(), true);
  assert.equal(controller.request(), false);
  assert.equal(loadCount, 1);

  load.resolve("ready");
  await flushMicrotasks();
  assert.deepEqual(successes, ["ready"]);
  assert.equal(loadCount, 1);
});

test("controller retries a failed history request and clears the retry attempt after success", async () => {
  const scheduled = [];
  const errors = [];
  const successes = [];
  let loadCount = 0;
  const controller = createCodingSessionDiscoveryController({
    async load() {
      loadCount += 1;
      if (loadCount === 1) {
        throw new Error("rate-limited: quota exceeded; retry in 5s");
      }
      return "recovered";
    },
    onAttemptStart() {},
    onSuccess: (value) => successes.push(value),
    onError: (error, retry) => errors.push({ error, retry }),
    retrySeed: "channel-b",
    getRateLimitRemainingMs: () => 0,
    schedule(callback, delayMs) {
      scheduled.push({ callback, delayMs });
      return scheduled.length;
    },
    clearSchedule() {},
  });

  controller.request();
  await flushMicrotasks();
  assert.equal(loadCount, 1);
  assert.equal(errors.length, 1);
  assert.equal(errors[0].retry.attempt, 1);
  assert.equal(errors[0].retry.willRetry, true);
  assert.ok(errors[0].retry.delayMs >= 5_000);
  assert.ok(errors[0].retry.delayMs < 5_750);
  assert.equal(controller.request(), false);

  scheduled.shift().callback();
  await flushMicrotasks();
  assert.equal(loadCount, 2);
  assert.deepEqual(successes, ["recovered"]);
});

test("controller stops after the bounded retry budget is exhausted", async () => {
  const scheduled = [];
  const errors = [];
  let loadCount = 0;
  const controller = createCodingSessionDiscoveryController({
    async load() {
      loadCount += 1;
      throw new Error("rate-limited: quota exceeded; retry in 5s");
    },
    onAttemptStart() {},
    onSuccess() {},
    onError: (_error, retry) => errors.push(retry),
    retrySeed: "bounded",
    maxRetries: 1,
    getRateLimitRemainingMs: () => 0,
    schedule(callback) {
      scheduled.push(callback);
      return scheduled.length;
    },
    clearSchedule() {},
  });

  controller.request();
  await flushMicrotasks();
  assert.equal(errors[0].willRetry, true);
  scheduled.shift()();
  await flushMicrotasks();
  assert.equal(loadCount, 2);
  assert.equal(errors[1].willRetry, false);
  assert.equal(scheduled.length, 0);
});

test("retry classifier excludes non-quota admission and authority failures", () => {
  assert.equal(
    isRetryableCodingSessionDiscoveryError(
      new Error("rate-limited: shared admission unavailable"),
    ),
    false,
  );
  assert.equal(
    isRetryableCodingSessionDiscoveryError(
      new Error("Native session projections disabled: invalid authority"),
    ),
    false,
  );
  assert.equal(
    isRetryableCodingSessionDiscoveryError(
      new Error("Timed out while loading channel history."),
    ),
    true,
  );
});

test("retry delay honors a longer active gate and caps hostile server hints", () => {
  const gateDelay = codingSessionDiscoveryRetryDelayMs(
    new Error("rate-limited: retry in 2s"),
    0,
    "gate",
    9_000,
  );
  assert.ok(gateDelay >= 9_000);
  assert.ok(gateDelay < 9_750);

  const capped = codingSessionDiscoveryRetryDelayMs(
    new Error("relay rate-limited: retry in 999999s"),
    40,
    "cap",
    0,
  );
  assert.equal(capped, 300_000);
});

test("cancel removes a scheduled retry and prevents later requests", async () => {
  const cleared = [];
  const scheduled = [];
  const controller = createCodingSessionDiscoveryController({
    async load() {
      throw new Error("socket closed");
    },
    onAttemptStart() {},
    onSuccess() {},
    onError() {},
    retrySeed: "cancel",
    getRateLimitRemainingMs: () => 0,
    schedule(callback, delayMs) {
      scheduled.push({ callback, delayMs });
      return 71;
    },
    clearSchedule: (timer) => cleared.push(timer),
  });

  controller.request();
  await flushMicrotasks();
  controller.cancel();
  assert.deepEqual(cleared, [71]);
  assert.equal(controller.request(), false);
});
