import assert from "node:assert/strict";
import test from "node:test";

import { deriveSeatBeeStamps } from "./codingSessionSeatBee.ts";

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

const ON_PATH_OLDER = {
  path: "/Users/brian/Projects/beekeeper/beekeeper/target/debug/bee",
  source: "path",
  version: "0.1.0",
  sha: "aaaaaaa11",
  dirty: false,
};

/** A minimal generation, defaulting to no stamp. */
function generation(overrides = {}) {
  return { beeStamp: null, ...overrides };
}

test("a seat's active generation stamp wins when it has one", () => {
  const stamps = deriveSeatBeeStamps([
    {
      executionKey: "builder-1",
      activeGeneration: generation({ beeStamp: BUNDLED }),
      priorGenerations: [generation({ beeStamp: ON_PATH_OLDER })],
    },
  ]);
  assert.deepEqual(stamps.get("builder-1"), BUNDLED);
});

test("a fresh resume with no stamp yet falls back to the newest prior generation's", () => {
  const stamps = deriveSeatBeeStamps([
    {
      executionKey: "builder-1",
      // The active generation just resumed and has not republished metadata.
      activeGeneration: generation({ beeStamp: null }),
      // Ascending: index 0 is oldest, the last entry is the newest prior.
      priorGenerations: [
        generation({ beeStamp: ON_PATH_OLDER }),
        generation({ beeStamp: ON_PATH }),
      ],
    },
  ]);
  assert.deepEqual(
    stamps.get("builder-1"),
    ON_PATH,
    "the most recent prior generation's stamp, not an older one",
  );
});

test("a seat none of whose generations ever carried a stamp maps to null", () => {
  const stamps = deriveSeatBeeStamps([
    {
      executionKey: "refuter-1",
      activeGeneration: generation(),
      priorGenerations: [generation(), generation()],
    },
  ]);
  assert.equal(stamps.get("refuter-1"), null);
  assert.ok(
    stamps.has("refuter-1"),
    "an explicit null, not an absent map entry — both render nothing on the chip, but the map is not silently incomplete",
  );
});

test("every seat in the umbrella gets its own entry, unmixed with another seat's stamp", () => {
  const stamps = deriveSeatBeeStamps([
    {
      executionKey: "builder-1",
      activeGeneration: generation({ beeStamp: BUNDLED }),
      priorGenerations: [],
    },
    {
      executionKey: "refuter-1",
      activeGeneration: generation({ beeStamp: ON_PATH }),
      priorGenerations: [],
    },
  ]);
  assert.deepEqual(stamps.get("builder-1"), BUNDLED);
  assert.deepEqual(stamps.get("refuter-1"), ON_PATH);
  assert.equal(stamps.size, 2);
});

test("an execution with no prior generations at all is not an error", () => {
  const stamps = deriveSeatBeeStamps([
    {
      executionKey: "builder-1",
      activeGeneration: generation(),
      priorGenerations: [],
    },
  ]);
  assert.equal(stamps.get("builder-1"), null);
});
