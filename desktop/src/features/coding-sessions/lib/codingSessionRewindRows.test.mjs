import assert from "node:assert/strict";
import test from "node:test";

import {
  codingSessionRewindBlockText,
  codingSessionRewindFilesText,
  codingSessionRewindOutcomeRows,
  codingSessionRewoundRowLabel,
} from "./codingSessionRewindRows.ts";

const HEAD = "abcdef0123456789abcdef0123456789abcdef01";

test("the disabled reasons say exactly why, in the brief's words", () => {
  assert.equal(
    codingSessionRewindBlockText("provider-cannot-rewind"),
    "This provider build cannot rewind",
  );
  assert.equal(
    codingSessionRewindBlockText("no-git"),
    "No git checkpoint for this turn",
  );
  assert.equal(
    codingSessionRewindBlockText("outside-generation"),
    "Only this run's turns can be rewound",
  );
  assert.match(codingSessionRewindBlockText("turn-running"), /turn is running/);
  assert.match(
    codingSessionRewindBlockText("not-controller"),
    /control this session/,
  );
});

test("the files line names HEAD's unchanged sha", () => {
  assert.equal(
    codingSessionRewindFilesText("restored", HEAD),
    "Files restored to before this turn · HEAD unchanged at abcdef0",
  );
  assert.equal(
    codingSessionRewindFilesText("kept", null),
    "Files kept as they are",
  );
  assert.match(
    codingSessionRewindFilesText("restore_failed", HEAD),
    /failed partway/,
  );
});

test("no memory and still-remembers are distinct visible outcomes", () => {
  const none = codingSessionRewindOutcomeRows({
    kind: "rewound",
    target: null,
    memory: "none",
    rewind: null,
    message: "the projector could not run",
  });
  assert.equal(none.tone, "warning");
  assert.equal(none.lines[0], "Rewound, but restarted with no memory");
  assert.equal(none.lines[1], "the projector could not run");
  const still = codingSessionRewindOutcomeRows({
    kind: "not-restarted",
    files: "restored",
    message: "still remembers turns 2–3",
  });
  assert.deepEqual(still.lines, [
    "Not restarted: still remembers turns 2–3",
    "Files restored to before this turn",
  ]);
  const seeded = codingSessionRewindOutcomeRows({
    kind: "rewound",
    target: null,
    memory: "seeded",
    rewind: { files: "kept", head: HEAD },
    message: null,
  });
  assert.deepEqual(seeded.lines, [
    "Rewound — the new conversation is seeded from the record",
    "Files kept as they are · HEAD unchanged at abcdef0",
    "The earlier native session is detached, not erased.",
  ]);
  // Never a claim that the old session forgot.
  for (const rows of [none, still, seeded]) {
    for (const line of rows.lines)
      assert.doesNotMatch(line, /forgot|erased the/i);
  }
});

test("TREE_BUSY shows the seat the provider named", () => {
  const rows = codingSessionRewindOutcomeRows({
    kind: "refused",
    code: "TREE_BUSY",
    message:
      "Seat lead (sess-9) is mid-turn in this working tree; checked on this provider only.",
  });
  assert.equal(
    rows.lines[0],
    "Not rewound — the working tree is busy: Seat lead (sess-9) is mid-turn in this working tree; checked on this provider only.",
  );
  assert.equal(rows.lines[1], "Nothing was changed.");
});

test("the collapsed row: count, signer, files; a count it cannot establish is left out", () => {
  assert.equal(
    codingSessionRewoundRowLabel({
      count: 3,
      signer: "Brian",
      rewind: { files: "restored" },
      memory: "seeded",
    }),
    "3 turns rewound by Brian · files restored",
  );
  assert.equal(
    codingSessionRewoundRowLabel({
      count: 1,
      signer: "You",
      rewind: { files: "kept" },
      memory: "none",
    }),
    "1 turn rewound by You · files kept · restarted with no memory",
  );
  assert.equal(
    codingSessionRewoundRowLabel({
      count: null,
      signer: "Brian",
      rewind: { files: "kept" },
      memory: "seeded",
    }),
    "Turns rewound by Brian · files kept",
  );
});
