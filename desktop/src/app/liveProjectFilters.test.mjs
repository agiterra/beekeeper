import assert from "node:assert/strict";
import { test } from "node:test";

import {
  isWatchedProjectEvent,
  liveProjectFilters,
} from "./liveProjectFilters.ts";

const OWNER = "a".repeat(64);
const COORD = `30621:${OWNER}:platform`;
const OTHER = `30621:${OWNER}:other`;

test("the deletion filter is always scoped by #a", () => {
  const filters = liveProjectFilters([COORD]);
  const deletion = filters.find((filter) => filter.kinds.includes(5));
  assert.ok(deletion, "a known project must be watched for deletion");
  assert.deepEqual(deletion["#a"], [COORD]);
});

test("no coordinates means no deletion filter at all, never an unscoped one", () => {
  // The leak guard. A bare `{kinds:[5]}` would receive every message
  // deletion in the community, and — because the relay's global fan-out is
  // ungated — tombstones for private projects this viewer cannot read. An
  // empty watch set must produce no kind:5 filter rather than a broad one.
  const filters = liveProjectFilters([]);
  assert.equal(
    filters.filter((filter) => filter.kinds.includes(5)).length,
    0,
    "an empty coordinate set must not subscribe to kind:5 at all",
  );
  assert.deepEqual(filters, [{ kinds: [30621], limit: 0 }]);
});

test("the head filter is unscoped, which is what makes a create arrive", () => {
  // A project created on another client is a coordinate this one has never
  // seen, so it cannot be watched by `#a`.
  const head = liveProjectFilters([COORD]).find((filter) =>
    filter.kinds.includes(30621),
  );
  assert.ok(head);
  assert.equal(head["#a"], undefined);
});

test("coordinates are de-duplicated and ordered so the REQ is stable", () => {
  const filters = liveProjectFilters([OTHER, COORD, OTHER]);
  const deletion = filters.find((filter) => filter.kinds.includes(5));
  assert.deepEqual(deletion["#a"], [COORD, OTHER].sort());
});

test("live filters carry limit 0 — live only, history comes from the queries", () => {
  for (const filter of liveProjectFilters([COORD])) {
    assert.equal(filter.limit, 0);
  }
});

test("a project head is always watched, including one never seen before", () => {
  assert.equal(
    isWatchedProjectEvent(
      { kind: 30621, tags: [["d", "brand-new"]] },
      new Set(),
    ),
    true,
  );
});

test("a deletion counts only when it names a watched coordinate", () => {
  const watched = new Set([COORD]);
  assert.equal(
    isWatchedProjectEvent({ kind: 5, tags: [["a", COORD]] }, watched),
    true,
  );
  // The relay ORs both filters onto one REQ, so an unrelated tombstone can
  // land here; refetching the whole project list for it is waste.
  assert.equal(
    isWatchedProjectEvent({ kind: 5, tags: [["a", OTHER]] }, watched),
    false,
  );
});

test("a message deletion never triggers a project refetch", () => {
  assert.equal(
    isWatchedProjectEvent(
      { kind: 5, tags: [["e", "b".repeat(64)]] },
      new Set([COORD]),
    ),
    false,
  );
});

test("an unrelated kind is ignored", () => {
  assert.equal(
    isWatchedProjectEvent(
      { kind: 40002, tags: [["a", COORD]] },
      new Set([COORD]),
    ),
    false,
  );
});
