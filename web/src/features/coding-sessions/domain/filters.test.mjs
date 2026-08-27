import assert from "node:assert/strict";
import { test } from "node:test";
import {
  CODING_SESSION_HISTORY_LIMIT,
  codingSessionCreatesFilters,
  codingSessionFactsFilter,
  codingSessionFactsLiveFilter,
  codingSessionHistoryFilters,
  codingSessionLeasesFilter,
  codingSessionRosterFilters,
  isTruncatedHistoryPage,
} from "./filters.ts";

const CHANNEL = "11111111-1111-4111-8111-111111111111";

test("every filter carries explicit kinds and an h scope", () => {
  for (const filter of codingSessionHistoryFilters(CHANNEL)) {
    assert.ok(
      Array.isArray(filter.kinds) && filter.kinds.length > 0,
      "a filter without kinds trips the relay p-gate and returns 403",
    );
    assert.deepEqual(filter["#h"], [CHANNEL]);
  }
});

test("no filter narrows by authors: authority is the read-side trust gate", () => {
  for (const filter of codingSessionHistoryFilters(CHANNEL)) {
    assert.equal("authors" in filter, false);
  }
});

test("the fact filter asks for exactly the three provider streams", () => {
  assert.deepEqual(
    codingSessionFactsFilter(CHANNEL).kinds,
    [44223, 44224, 44225],
  );
  assert.equal(
    codingSessionFactsFilter(CHANNEL).limit,
    CODING_SESSION_HISTORY_LIMIT,
  );
});

test("the live filter replays nothing", () => {
  const live = codingSessionFactsLiveFilter(CHANNEL);
  assert.equal(live.limit, 0);
  assert.deepEqual(live.kinds, [44223, 44224, 44225]);
});

test("the create reads are one filter PER KIND", () => {
  const filters = codingSessionCreatesFilters(CHANNEL);
  assert.equal(filters.length, 3);
  assert.deepEqual(
    filters.map((filter) => filter.kinds),
    [[44221], [44224], [44226]],
  );
  for (const filter of filters) {
    assert.equal(filter.limit, CODING_SESSION_HISTORY_LIMIT);
  }
});

test("leases read whole at the history limit, never paginated", () => {
  assert.deepEqual(codingSessionLeasesFilter(CHANNEL).kinds, [24223]);
  assert.equal(
    codingSessionLeasesFilter(CHANNEL).limit,
    CODING_SESSION_HISTORY_LIMIT,
  );
});

test("the roster reads use the smaller 500 limit", () => {
  const filters = codingSessionRosterFilters(CHANNEL);
  assert.deepEqual(
    filters.map((filter) => filter.kinds),
    [[44228], [40099]],
  );
  for (const filter of filters) assert.equal(filter.limit, 500);
});

test("a full page is reported as truncated, never as complete", () => {
  const filter = codingSessionFactsFilter(CHANNEL);
  assert.equal(isTruncatedHistoryPage(filter, 1000), true);
  assert.equal(isTruncatedHistoryPage(filter, 999), false);
  assert.equal(
    isTruncatedHistoryPage(codingSessionFactsLiveFilter(CHANNEL), 5000),
    false,
    "a live subscription has no page to truncate",
  );
});
