import assert from "node:assert/strict";
import test from "node:test";

import {
  classifySendLane,
  LOCAL_BURST_CAPACITY,
  LOCAL_EVENTS_PER_MINUTE,
  RelaySendBudget,
  WRITE_RESERVE,
} from "./relaySendBudget.ts";

// ── Fake clock ────────────────────────────────────────────────────────────────

let fakeNow = 0;
const pendingTimers = new Map();
let nextTimerId = 1;

function fakeSetTimeout(fn, ms) {
  const id = nextTimerId++;
  pendingTimers.set(id, { fn, fireAt: fakeNow + ms });
  return id;
}

function fakeClearTimeout(id) {
  pendingTimers.delete(id);
}

function tickTo(ms) {
  fakeNow = ms;
  for (const [id, { fn, fireAt }] of Array.from(pendingTimers.entries())) {
    if (fireAt <= fakeNow) {
      pendingTimers.delete(id);
      fn();
    }
  }
}

function budget(overrides = {}) {
  fakeNow = 0;
  pendingTimers.clear();
  return new RelaySendBudget({
    now: () => fakeNow,
    setTimeoutFn: fakeSetTimeout,
    clearTimeoutFn: fakeClearTimeout,
    ...overrides,
  });
}

async function settle() {
  await new Promise((resolve) => setImmediate(resolve));
}

// ── Lane classification ───────────────────────────────────────────────────────

test("classifySendLane keys on payload[0] and the EVENT kind", () => {
  assert.equal(classifySendLane(["REQ", "sub", { kinds: [1] }]), "read");
  assert.equal(classifySendLane(["COUNT", "sub", { kinds: [1] }]), "read");
  assert.equal(classifySendLane(["EVENT", { kind: 40002 }]), "write");
  assert.equal(classifySendLane(["EVENT", { kind: 20002 }]), "ephemeral");
  assert.equal(classifySendLane(["EVENT", { kind: 29999 }]), "ephemeral");
  assert.equal(classifySendLane(["EVENT", { kind: 30000 }]), "write");
  assert.equal(classifySendLane(["AUTH", { kind: 22242 }]), "free");
  assert.equal(classifySendLane(["CLOSE", "sub"]), "free");
});

// ── Reserve and capacity ──────────────────────────────────────────────────────

test("reads stop at capacity minus the write reserve; writes may use the reserve", () => {
  const b = budget();
  const readCap = LOCAL_BURST_CAPACITY - WRITE_RESERVE;
  for (let i = 0; i < readCap; i++) {
    assert.equal(b.tryAcquire("read"), true, `read ${i} admitted`);
  }
  assert.equal(b.tryAcquire("read"), false, "read lane exhausted at reserve");
  assert.equal(
    b.tryAcquire("ephemeral"),
    false,
    "ephemerals respect the reserve",
  );
  for (let i = 0; i < WRITE_RESERVE; i++) {
    assert.equal(b.tryAcquire("write"), true, `write ${i} takes the reserve`);
  }
  assert.equal(b.tryAcquire("write"), false, "burst exhausted");
  assert.equal(b.framesInWindow(), LOCAL_BURST_CAPACITY);
  assert.equal(b.tryAcquire("free"), true, "AUTH/CLOSE are never charged");
  assert.equal(b.framesInWindow(), LOCAL_BURST_CAPACITY);
});

test("the burst window slides: frames older than 5 s free their slot", () => {
  const b = budget();
  for (let i = 0; i < LOCAL_BURST_CAPACITY; i++) {
    tickTo(i * 100);
    assert.equal(b.tryAcquire("write"), true);
  }
  assert.equal(b.tryAcquire("write"), false);
  // The first frame (t=0) leaves the window just after t=5000.
  tickTo(5_001);
  assert.equal(b.tryAcquire("write"), true, "one slot reopened");
  assert.equal(b.tryAcquire("write"), false, "only one");
});

test("the EVENT minute counter caps writes and ephemerals but not reads", () => {
  const b = budget({ capacity: 1_000, writeReserve: 0 });
  for (let i = 0; i < LOCAL_EVENTS_PER_MINUTE; i++) {
    assert.equal(b.tryAcquire("write"), true);
  }
  assert.equal(b.tryAcquire("write"), false, "minute counter exhausted");
  assert.equal(b.tryAcquire("ephemeral"), false);
  assert.equal(b.tryAcquire("read"), true, "reads are not EVENTs");
  tickTo(60_001);
  assert.equal(b.tryAcquire("write"), true, "minute window slid");
});

test("ephemerals leave a reserve on the minute counter for durable writes", () => {
  const b = budget({ capacity: 1_000, writeReserve: WRITE_RESERVE });
  let admitted = 0;
  while (b.tryAcquire("ephemeral")) admitted++;
  assert.equal(admitted, LOCAL_EVENTS_PER_MINUTE - WRITE_RESERVE);
  assert.equal(b.tryAcquire("write"), true, "a message still fits");
});

// ── acquire() waits ───────────────────────────────────────────────────────────

test("acquire resolves immediately while a slot is free and waits otherwise", async () => {
  const b = budget({ capacity: 2, writeReserve: 0 });
  const order = [];
  await b.acquire("read");
  await b.acquire("read");
  const third = b.acquire("read").then(() => order.push("third"));
  await settle();
  assert.deepEqual(order, [], "third read waits for the window");
  tickTo(5_001);
  await third;
  assert.deepEqual(order, ["third"]);
});

test("a waiting write is admitted ahead of a read waiting on the reserve", async () => {
  const b = budget({ capacity: 3, writeReserve: 1 });
  // Two reads fill everything a read may take (capacity − reserve = 2).
  assert.equal(b.tryAcquire("read"), true);
  tickTo(10);
  assert.equal(b.tryAcquire("read"), true);
  assert.equal(b.tryAcquire("read"), false);
  const order = [];
  const read = b.acquire("read").then(() => order.push("read"));
  const write = b.acquire("write").then(() => order.push("write"));
  await settle();
  assert.deepEqual(order, ["write"], "the write took the reserve at once");
  // The first read's frame leaves the window at t=5001, but the write took
  // the slot it freed against the reserve; the read gets in once the second
  // read's frame expires at t=5011.
  tickTo(5_001);
  await settle();
  assert.deepEqual(order, ["write"], "still one frame short of the reserve");
  tickTo(5_011);
  await read;
  await write;
  assert.deepEqual(order, ["write", "read"]);
});

test("reset releases every waiter and clears every charge", async () => {
  const b = budget({ capacity: 1, writeReserve: 0 });
  assert.equal(b.tryAcquire("write"), true);
  let released = false;
  const waiting = b.acquire("write").then(() => {
    released = true;
  });
  await settle();
  assert.equal(released, false);
  b.reset();
  await waiting;
  assert.equal(released, true, "waiters resume so session checks can run");
  assert.equal(b.framesInWindow(), 0);
  assert.equal(pendingTimers.size, 0, "the wake timer was cancelled");
});
