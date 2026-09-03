import assert from "node:assert/strict";
import test from "node:test";

import {
  buildPulseStaleBeeReading,
  codingSessionSeatBeeLine,
  PULSE_STALE_BEE_ROW_LIMIT,
  readSeatBeeStamp,
} from "./codingSessionSeatBee.ts";

const BUNDLED = {
  path: "/Applications/Beekeeper.app/Contents/MacOS/bee",
  source: "bundled",
  version: "0.1.0",
  sha: "23728227b",
  dirty: false,
};

const ON_PATH = {
  path: "/Users/brian/Projects/beekeeper/beekeeper/target/debug/bee",
  source: "path",
  version: "0.1.0",
  sha: "07c470be0",
  dirty: true,
};

const UNPARSED = {
  path: "/Applications/Beekeeper.app/Contents/MacOS/bee",
  source: "bundled",
  version: null,
  sha: null,
  dirty: null,
};

test("a stamp is read only when the wire carried all five keys", () => {
  assert.deepEqual(readSeatBeeStamp(BUNDLED), BUNDLED);
  assert.deepEqual(readSeatBeeStamp(ON_PATH), ON_PATH);
  assert.deepEqual(readSeatBeeStamp(UNPARSED), UNPARSED);
});

test("a missing key is absence, never a partially guessed stamp", () => {
  assert.equal(readSeatBeeStamp(undefined), null);
  assert.equal(readSeatBeeStamp(null), null);
  assert.equal(readSeatBeeStamp("bee 23728227b"), null);
  assert.equal(readSeatBeeStamp([BUNDLED]), null);
  // One key short.
  const { dirty: _dirty, ...missing } = BUNDLED;
  assert.equal(readSeatBeeStamp(missing), null);
  // One key too many: an unknown key means a shape this reader does not know.
  assert.equal(readSeatBeeStamp({ ...BUNDLED, extra: 1 }), null);
});

test("each field is refused on its own terms", () => {
  assert.equal(readSeatBeeStamp({ ...BUNDLED, path: "" }), null);
  assert.equal(readSeatBeeStamp({ ...BUNDLED, path: null }), null);
  assert.equal(readSeatBeeStamp({ ...BUNDLED, source: "sidecar" }), null);
  assert.equal(readSeatBeeStamp({ ...BUNDLED, source: null }), null);
  assert.equal(readSeatBeeStamp({ ...BUNDLED, version: 1 }), null);
  assert.equal(readSeatBeeStamp({ ...BUNDLED, dirty: "false" }), null);
  // sha: lowercase hex, 7..40, and never a `-dirty` suffix.
  assert.equal(readSeatBeeStamp({ ...BUNDLED, sha: "23728Z" }), null);
  assert.equal(readSeatBeeStamp({ ...BUNDLED, sha: "237282" }), null);
  assert.equal(readSeatBeeStamp({ ...BUNDLED, sha: "23728227B" }), null);
  assert.equal(readSeatBeeStamp({ ...BUNDLED, sha: "23728227b-dirty" }), null);
  assert.equal(readSeatBeeStamp({ ...BUNDLED, sha: "a".repeat(41) }), null);
  assert.deepEqual(readSeatBeeStamp({ ...BUNDLED, sha: "a".repeat(40) }), {
    ...BUNDLED,
    sha: "a".repeat(40),
  });
});

test("the seat card reads which build, in words", () => {
  assert.equal(codingSessionSeatBeeLine(BUNDLED), "bee 23728227b (bundled)");
  assert.equal(
    codingSessionSeatBeeLine(ON_PATH),
    "bee 07c470be0-dirty (found on PATH: /Users/brian/Projects/beekeeper/beekeeper/target/debug)",
  );
  assert.equal(codingSessionSeatBeeLine(UNPARSED), "bee build unknown");
});

test("no stamp on the wire renders nothing, not an invented unknown", () => {
  assert.equal(codingSessionSeatBeeLine(null), null);
});

test("a bare path name is its own directory", () => {
  assert.equal(
    codingSessionSeatBeeLine({ ...ON_PATH, path: "bee", dirty: false }),
    "bee 07c470be0 (found on PATH: bee)",
  );
});

const seat = (seatKey, label, stamp) => ({ seatKey, label, stamp });

test("only a host-decided `behind` makes a Pulse row", () => {
  const reading = buildPulseStaleBeeReading(
    [
      seat("a", "Bob · Builder", { ...BUNDLED, sha: "aaaaaaa1" }),
      seat("b", "Cleo · Verifier", { ...BUNDLED, sha: "bbbbbbb2" }),
    ],
    new Map([
      ["aaaaaaa1", { kind: "behind", commits: 3 }],
      ["bbbbbbb2", { kind: "on-main" }],
    ]),
  );
  assert.deepEqual(reading, {
    rows: [
      { seatKey: "a", label: "Bob · Builder", sha7: "aaaaaaa", behind: 3 },
    ],
    uncomparedCount: 0,
    truncatedCount: 0,
  });
});

test("everything the host could not compare is counted, never listed", () => {
  const reading = buildPulseStaleBeeReading(
    [
      seat("a", "No stamp", null),
      seat("b", "Unparsed", UNPARSED),
      seat("c", "No ancestry", { ...BUNDLED, sha: "ccccccc3" }),
      seat("d", "Explicitly unknown", { ...BUNDLED, sha: "ddddddd4" }),
    ],
    new Map([["ddddddd4", { kind: "unknown" }]]),
  );
  assert.deepEqual(reading, {
    rows: [],
    uncomparedCount: 4,
    truncatedCount: 0,
  });
});

test("rows fall by how far behind, ties by label byte order", () => {
  const stamps = [
    seat("a", "Zeta", { ...BUNDLED, sha: "1111111a" }),
    seat("b", "Alpha", { ...BUNDLED, sha: "2222222b" }),
    seat("c", "Mid", { ...BUNDLED, sha: "3333333c" }),
  ];
  const reading = buildPulseStaleBeeReading(
    stamps,
    new Map([
      ["1111111a", { kind: "behind", commits: 2 }],
      ["2222222b", { kind: "behind", commits: 2 }],
      ["3333333c", { kind: "behind", commits: 9 }],
    ]),
  );
  assert.deepEqual(
    reading.rows.map((row) => row.label),
    ["Mid", "Alpha", "Zeta"],
  );
});

test("the row list is bounded and says how many it dropped", () => {
  const seats = [];
  const ancestry = new Map();
  for (let index = 0; index < PULSE_STALE_BEE_ROW_LIMIT + 5; index += 1) {
    const sha = `${String(index).padStart(2, "0")}aaaaa`;
    seats.push(seat(`s${index}`, `Seat ${index}`, { ...BUNDLED, sha }));
    ancestry.set(sha, { kind: "behind", commits: index + 1 });
  }
  const reading = buildPulseStaleBeeReading(seats, ancestry);
  assert.equal(reading.rows.length, PULSE_STALE_BEE_ROW_LIMIT);
  assert.equal(reading.truncatedCount, 5);
  assert.equal(reading.uncomparedCount, 0);
});

test("no seats at all is an empty reading, not an unknown one", () => {
  assert.deepEqual(buildPulseStaleBeeReading([], new Map()), {
    rows: [],
    uncomparedCount: 0,
    truncatedCount: 0,
  });
});
