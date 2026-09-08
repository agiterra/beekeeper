import assert from "node:assert/strict";
import test from "node:test";

import {
  isMatchableFilter,
  matchesAnyFilter,
  matchesFilter,
} from "./relayFilterMatch.ts";

function event(overrides = {}) {
  return {
    id: "abcdef0123",
    pubkey: "alice",
    created_at: 1_000,
    kind: 9,
    tags: [
      ["h", "ch-1"],
      ["p", "bob"],
    ],
    content: "",
    sig: "sig",
    ...overrides,
  };
}

// Mirrors crates/buzz-core/src/filter.rs: fields AND, values OR, since/until
// inclusive, ids by prefix, `#x` any-of over tags whose first element is x.
const cases = [
  ["kind listed", { kinds: [9, 40002], limit: 1 }, event(), true],
  ["kind not listed", { kinds: [40002], limit: 1 }, event(), false],
  [
    "author listed",
    { kinds: [9], authors: ["alice"], limit: 1 },
    event(),
    true,
  ],
  [
    "author not listed",
    { kinds: [9], authors: ["bob"], limit: 1 },
    event(),
    false,
  ],
  ["since inclusive", { kinds: [9], since: 1_000, limit: 1 }, event(), true],
  [
    "since excludes older",
    { kinds: [9], since: 1_001, limit: 1 },
    event(),
    false,
  ],
  ["until inclusive", { kinds: [9], until: 1_000, limit: 1 }, event(), true],
  [
    "until excludes newer",
    { kinds: [9], until: 999, limit: 1 },
    event(),
    false,
  ],
  ["ids prefix", { kinds: [9], ids: ["abcd"], limit: 1 }, event(), true],
  ["ids no prefix", { kinds: [9], ids: ["ffff"], limit: 1 }, event(), false],
  ["#h single", { kinds: [9], "#h": ["ch-1"], limit: 1 }, event(), true],
  [
    "#h multi any-of",
    { kinds: [9], "#h": ["ch-0", "ch-1", "ch-2"], limit: 1 },
    event(),
    true,
  ],
  [
    "#h multi none",
    { kinds: [9], "#h": ["ch-0", "ch-2"], limit: 1 },
    event(),
    false,
  ],
  ["#p listed", { kinds: [9], "#p": ["bob"], limit: 1 }, event(), true],
  [
    "#p and #h both required",
    { kinds: [9], "#p": ["bob"], "#h": ["ch-2"], limit: 1 },
    event(),
    false,
  ],
  [
    "event with two h tags matches either",
    { kinds: [9], "#h": ["ch-9"], limit: 1 },
    event({
      tags: [
        ["h", "ch-1"],
        ["h", "ch-9"],
      ],
    }),
    true,
  ],
  [
    "#e absent on event",
    { kinds: [9], "#e": ["root"], limit: 1 },
    event(),
    false,
  ],
  [
    "h-less event fails #h strictly",
    { kinds: [7], "#h": ["ch-1"], limit: 1 },
    event({ kind: 7, tags: [["e", "target"]] }),
    false,
  ],
];

for (const [name, filter, ev, expected] of cases) {
  test(`matchesFilter — ${name}`, () => {
    assert.equal(matchesFilter(ev, filter), expected);
  });
}

test("permissive channel fallback passes an h-less event through #h only", () => {
  const reaction = event({ kind: 7, tags: [["e", "target"]] });
  const filter = { kinds: [7], "#h": ["ch-1"], limit: 1 };
  assert.equal(
    matchesFilter(reaction, filter, { channelFallback: "permissive" }),
    true,
  );
  // An event that HAS h tags, none matching, is still rejected.
  const other = event({ kind: 7, tags: [["h", "ch-2"]] });
  assert.equal(
    matchesFilter(other, filter, { channelFallback: "permissive" }),
    false,
  );
  // The fallback is for `h` only.
  const noP = { kinds: [7], "#p": ["bob"], limit: 1 };
  assert.equal(
    matchesFilter(reaction, noP, { channelFallback: "permissive" }),
    false,
  );
});

test("matchesAnyFilter ORs filters", () => {
  assert.equal(
    matchesAnyFilter(event(), [
      { kinds: [1], limit: 1 },
      { kinds: [9], "#h": ["ch-1"], limit: 1 },
    ]),
    true,
  );
  assert.equal(matchesAnyFilter(event(), [{ kinds: [1], limit: 1 }]), false);
});

test("isMatchableFilter refuses search and extension fields", () => {
  assert.equal(
    isMatchableFilter({
      kinds: [9],
      "#h": ["a"],
      limit: 5,
      since: 1,
      until: 2,
      ids: [],
      authors: [],
    }),
    true,
  );
  assert.equal(
    isMatchableFilter({ kinds: [9], limit: 5, search: "hello" }),
    false,
  );
  assert.equal(
    isMatchableFilter({ kinds: [9], limit: 5, before_id: "x" }),
    false,
  );
  // Any `#name` key is a tag clause; only a bare `#` is not.
  assert.equal(isMatchableFilter({ kinds: [9], limit: 5, "#hh": ["x"] }), true);
  assert.equal(isMatchableFilter({ kinds: [9], limit: 5, "#": ["x"] }), false);
});

test("a multi-letter tag key is matchable and matches by its full name", () => {
  const filter = { kinds: [44244], "#cstx-genesis": ["g1"] };
  assert.equal(isMatchableFilter(filter), true);
  const yes = { ...event(), kind: 44244, tags: [["cstx-genesis", "g1"]] };
  const no = { ...event(), kind: 44244, tags: [["cstx-genesis", "g2"]] };
  assert.equal(matchesFilter(yes, filter), true);
  assert.equal(matchesFilter(no, filter), false);
});
