/**
 * SV-116: a session's history is paged back to its first event, the pager
 * stops exactly when the relay runs out, it never hands the store an event
 * twice, and it says what it does not hold.
 *
 * The fake relay below implements the same contract beekeeper-relay serves for
 * these kinds: `created_at DESC, id ASC`, `until` inclusive, `since`
 * inclusive, `limit` clamped to the page ceiling.
 */
import assert from "node:assert/strict";
import test from "node:test";

import {
  advanceCodingSessionHistorySegment,
  CodingSessionHistoryBackfill,
  buildCodingSessionHistoryPageFilter,
} from "./codingSessionTrustedIngressPaging.ts";

const KIND = 44225;
const BASE = { kinds: [KIND], "#h": ["channel-1"], limit: 1000 };

function ev(id, createdAt) {
  return { id, created_at: createdAt, kind: KIND, tags: [], content: "" };
}

/** `count` events, `perSecond` to a second, newest at `top`. */
function history(count, { top = 1_000_000, perSecond = 1 } = {}) {
  return Array.from({ length: count }, (_, index) =>
    ev(
      `e${String(index).padStart(6, "0")}`,
      top - Math.floor(index / perSecond),
    ),
  );
}

function fakeRelay(events, pageCeiling) {
  const requests = [];
  const sorted = [...events].sort((a, b) =>
    b.created_at !== a.created_at
      ? b.created_at - a.created_at
      : a.id < b.id
        ? -1
        : 1,
  );
  return {
    requests,
    async fetchPage(filter) {
      requests.push(filter);
      const limit = Math.min(filter.limit, pageCeiling);
      return sorted
        .filter(
          (e) =>
            (filter.until === undefined || e.created_at <= filter.until) &&
            (filter.since === undefined || e.created_at >= filter.since),
        )
        .slice(0, limit);
    },
  };
}

async function backfillAll(events, { limit = 10, maxPages = 200 } = {}) {
  const relay = fakeRelay(events, limit);
  const backfill = new CodingSessionHistoryBackfill(true, limit, maxPages);
  const detach = backfill.attach();
  const ingested = [];
  const newest = await relay.fetchPage({ ...BASE, limit });
  ingested.push(...newest);
  backfill.noteNewestPage(KIND, { ...BASE, limit }, newest);
  await backfill.run({
    fetchPage: (filter) => relay.fetchPage(filter),
    ingest: (page) => ingested.push(...page),
  });
  detach();
  return { backfill, ingested, relay };
}

function assertNoDuplicates(events) {
  const ids = events.map((e) => e.id);
  assert.equal(new Set(ids).size, ids.length, "an event was ingested twice");
}

test("a short newest page is the whole history — no older page is requested", async () => {
  const { backfill, ingested, relay } = await backfillAll(history(7));
  assert.equal(relay.requests.length, 1);
  assert.equal(ingested.length, 7);
  assert.equal(backfill.completeness().state, "complete");
});

test("pages back to the first event, without duplicates, and stops on the short page", async () => {
  const events = history(43, { perSecond: 3 });
  const { backfill, ingested, relay } = await backfillAll(events);
  assertNoDuplicates(ingested);
  assert.deepEqual(
    new Set(ingested.map((e) => e.id)),
    new Set(events.map((e) => e.id)),
  );
  // Every page after the first asks from the previous page's oldest second.
  for (const filter of relay.requests.slice(1)) {
    assert.equal(typeof filter.until, "number");
    assert.equal(filter.since, undefined);
  }
  const last = relay.requests.at(-1);
  assert.ok(
    (await relay.fetchPage(last)).length < 10,
    "stopped on a short page",
  );
  const completeness = backfill.completeness();
  assert.equal(completeness.state, "complete");
  assert.equal(completeness.loadedEarlierCount, 43 - 10);
});

test("an exact multiple of the page size ends on the empty page after it", async () => {
  const events = history(30);
  const { backfill, ingested } = await backfillAll(events);
  assertNoDuplicates(ingested);
  assert.equal(ingested.length, 30);
  assert.equal(backfill.completeness().state, "complete");
});

test("a second holding more than a page is disclosed, not skipped silently", async () => {
  const crowded = [
    ...history(5, { top: 2000 }),
    ...Array.from({ length: 12 }, (_, i) => ev(`c${i}`, 1500)),
    ...history(4, { top: 1000 }).map((e) => ({ ...e, id: `o${e.id}` })),
  ];
  const { backfill, ingested } = await backfillAll(crowded);
  assertNoDuplicates(ingested);
  // Everything below the crowded second still arrives.
  for (const id of ["oe000000", "oe000003"]) {
    assert.ok(
      ingested.some((e) => e.id === id),
      `${id} was not paged`,
    );
  }
  const completeness = backfill.completeness();
  assert.equal(completeness.state, "incomplete");
  assert.equal(completeness.reason, "crowded-second");
});

test("a failing page stops paging and says so; the next run resumes from the same cursor", async () => {
  const events = history(25);
  const relay = fakeRelay(events, 10);
  const backfill = new CodingSessionHistoryBackfill(true, 10, 200);
  backfill.attach();
  const ingested = [];
  const newest = await relay.fetchPage({ ...BASE, limit: 10 });
  ingested.push(...newest);
  backfill.noteNewestPage(KIND, { ...BASE, limit: 10 }, newest);
  assert.equal(backfill.completeness().state, "loading-earlier");

  await backfill.run({
    fetchPage: async () => {
      throw new Error("relay went away");
    },
    ingest: (page) => ingested.push(...page),
  });
  let completeness = backfill.completeness();
  assert.equal(completeness.state, "incomplete");
  assert.equal(completeness.reason, "error");
  assert.equal(completeness.message, "relay went away");

  await backfill.run({
    fetchPage: (filter) => relay.fetchPage(filter),
    ingest: (page) => ingested.push(...page),
  });
  assertNoDuplicates(ingested);
  assert.equal(ingested.length, 25);
  completeness = backfill.completeness();
  assert.equal(completeness.state, "complete");
});

test("the page budget is disclosed rather than read as the beginning", async () => {
  const { backfill } = await backfillAll(history(100), { maxPages: 3 });
  const completeness = backfill.completeness();
  assert.equal(completeness.state, "incomplete");
  assert.equal(completeness.reason, "page-budget");
  // Each page re-reads its oldest second (`until` is inclusive): 3 × 9 new.
  assert.equal(completeness.loadedEarlierCount, 27);
});

test("a one-page scope with a full page says older history is not loaded", () => {
  const backfill = new CodingSessionHistoryBackfill(false, 10, 200);
  backfill.noteNewestPage(KIND, BASE, history(10));
  const completeness = backfill.completeness();
  assert.equal(completeness.state, "incomplete");
  assert.equal(completeness.reason, "not-paged");
  const short = new CodingSessionHistoryBackfill(false, 10, 200);
  short.noteNewestPage(KIND, BASE, history(3));
  assert.equal(short.completeness().state, "complete");
});

test("nothing is claimed before the newest page answers", () => {
  const backfill = new CodingSessionHistoryBackfill(true, 10, 200);
  assert.equal(backfill.completeness().state, "pending");
});

test("a reload that no longer reaches known history pages the gap, bounded by since", async () => {
  const old = history(15, { top: 1000 });
  const relayOld = fakeRelay(old, 10);
  const backfill = new CodingSessionHistoryBackfill(true, 10, 200);
  backfill.attach();
  const ingested = [];
  const first = await relayOld.fetchPage({ ...BASE, limit: 10 });
  ingested.push(...first);
  backfill.noteNewestPage(KIND, { ...BASE, limit: 10 }, first);
  await backfill.run({
    fetchPage: (f) => relayOld.fetchPage(f),
    ingest: (p) => ingested.push(...p),
  });
  assert.equal(backfill.completeness().state, "complete");

  // 25 events arrive while this client is away.
  const fresh = history(25, { top: 5000 }).map((e) => ({
    ...e,
    id: `n${e.id}`,
  }));
  const relay = fakeRelay([...old, ...fresh], 10);
  const reload = await relay.fetchPage({ ...BASE, limit: 10 });
  ingested.push(...reload);
  backfill.noteNewestPage(KIND, { ...BASE, limit: 10 }, reload);
  assert.equal(backfill.completeness().state, "loading-earlier");
  await backfill.run({
    fetchPage: (f) => relay.fetchPage(f),
    ingest: (p) => ingested.push(...p),
  });
  for (const filter of relay.requests.slice(1)) {
    assert.equal(filter.since, 1000, "the gap read stops at known history");
  }
  const ids = new Set(ingested.map((e) => e.id));
  for (const e of fresh)
    assert.ok(ids.has(e.id), `${e.id} missing after reload`);
  assert.equal(backfill.completeness().state, "complete");
});

test("paging pauses when the last reader detaches and keeps its cursor", async () => {
  const relay = fakeRelay(history(50), 10);
  const backfill = new CodingSessionHistoryBackfill(true, 10, 200);
  const detach = backfill.attach();
  const newest = await relay.fetchPage({ ...BASE, limit: 10 });
  backfill.noteNewestPage(KIND, { ...BASE, limit: 10 }, newest);
  const ingested = [...newest];
  await backfill.run({
    fetchPage: async (f) => {
      detach();
      return relay.fetchPage(f);
    },
    ingest: (p) => ingested.push(...p),
  });
  assert.equal(relay.requests.length, 2, "one in-flight page, then paused");
  assert.equal(backfill.completeness().state, "loading-earlier");
  backfill.attach();
  await backfill.run({
    fetchPage: (f) => relay.fetchPage(f),
    ingest: (p) => ingested.push(...p),
  });
  assertNoDuplicates(ingested);
  assert.equal(ingested.length, 50);
  assert.equal(backfill.completeness().state, "complete");
});

test("listeners hear progress so the hook can publish per frame", async () => {
  const relay = fakeRelay(history(35), 10);
  const backfill = new CodingSessionHistoryBackfill(true, 10, 200);
  backfill.attach();
  let notified = 0;
  backfill.subscribe(() => {
    notified += 1;
  });
  const newest = await relay.fetchPage({ ...BASE, limit: 10 });
  backfill.noteNewestPage(KIND, { ...BASE, limit: 10 }, newest);
  await backfill.run({ fetchPage: (f) => relay.fetchPage(f), ingest() {} });
  assert.ok(notified >= 4, `expected progress notifications, got ${notified}`);
});

test("advance: page filter carries until/since and the ceiling", () => {
  const filter = buildCodingSessionHistoryPageFilter(
    { ...BASE, since: 5 },
    { until: 99, seenAtUntil: new Set(), since: null },
    1000,
  );
  assert.equal(filter.until, 99);
  assert.equal(filter.since, undefined);
  assert.equal(filter.limit, 1000);
  const step = advanceCodingSessionHistorySegment(
    { until: 10, seenAtUntil: new Set(["a"]), since: null },
    [ev("a", 10), ev("b", 10), ev("c", 9)],
    3,
  );
  assert.deepEqual(
    step.fresh.map((e) => e.id),
    ["b", "c"],
  );
  assert.equal(step.next?.until, 9);
});
