import assert from "node:assert/strict";
import test from "node:test";

import {
  codingSessionPatchLines,
  deriveCodingSessionTurnChanges,
  INCOMPLETE_CAPTURE_NOTE,
  OBSERVED_SOURCE_LABEL,
  OUTSIDE_TURN_NOTE,
  resolveCodingSessionDiffRange,
} from "./codingSessionCheckpointChanges.ts";
import { foldCodingSessionCheckpoints } from "./codingSessionCheckpoints.ts";
import {
  checkpointEvent,
  gitFacts,
  payload,
  scope,
  TREE_A,
  TREE_B,
  TREE_C,
  unavailablePayload,
} from "./codingSessionCheckpointFixtures.testFixtures.mjs";

function generationOf(...bodies) {
  const fold = foldCodingSessionCheckpoints(
    bodies.map((body) => checkpointEvent(body)),
    scope(),
  );
  return [...fold.byScope.values()][0];
}

function entryOf(body) {
  return generationOf(body).turns[0];
}

const observedFile = {
  path: "src/header.tsx",
  filename: "header.tsx",
  additions: 1,
  deletions: 1,
  diffs: [],
  editCount: 1,
};

test("no checkpoint: observed files are labelled may-be-incomplete; none is nothing", () => {
  assert.equal(
    deriveCodingSessionTurnChanges({ checkpoint: null, observedFiles: [] }),
    null,
  );
  const observed = deriveCodingSessionTurnChanges({
    checkpoint: null,
    observedFiles: [observedFile],
  });
  assert.equal(observed.source, "observed");
  assert.equal(observed.headline, "1 changed file");
  assert.equal(observed.sourceLabel, OBSERVED_SOURCE_LABEL);
  assert.equal(observed.showTotals, true);
});

test("git: N files changed · From git, binary files carry no counts", () => {
  const changes = deriveCodingSessionTurnChanges({
    checkpoint: entryOf(
      payload({
        files: [
          {
            path: "a.txt",
            status: "modified",
            from: null,
            additions: 2,
            deletions: 0,
          },
          {
            path: "img.png",
            status: "added",
            from: null,
            additions: null,
            deletions: null,
          },
        ],
        filesNotListed: 3,
      }),
    ),
    observedFiles: [observedFile],
  });
  assert.equal(changes.source, "git");
  assert.equal(changes.headline, "5 files changed");
  assert.equal(changes.sourceLabel, "From git");
  assert.equal(changes.files.length, 2);
  assert.equal(changes.files[1].additions, null);
  assert.equal(changes.showTotals, false, "a binary file means no totals");
  assert.ok(changes.notes.includes("3 more files not listed"));
});

test("complete: false with nothing omitted still says the capture is incomplete", () => {
  // The provider's overlapped capture (the next turn began before it
  // finished) is complete: false with no omission: never a full measurement.
  const changes = deriveCodingSessionTurnChanges({
    checkpoint: entryOf(payload({ git: gitFacts({ complete: false }) })),
    observedFiles: [],
  });
  assert.equal(changes.source, "git");
  assert.equal(changes.notCaptured, 0);
  assert.ok(
    changes.notes.includes(INCOMPLETE_CAPTURE_NOTE),
    `notes: ${JSON.stringify(changes.notes)}`,
  );
  const noChange = deriveCodingSessionTurnChanges({
    checkpoint: entryOf(
      payload({ git: gitFacts({ complete: false }), files: [] }),
    ),
    observedFiles: [],
  });
  assert.notEqual(noChange, null, "an incomplete no-change is not hidden");
  assert.ok(noChange.notes.includes(INCOMPLETE_CAPTURE_NOTE));
  const complete = deriveCodingSessionTurnChanges({
    checkpoint: entryOf(payload()),
    observedFiles: [],
  });
  assert.ok(!complete.notes.includes(INCOMPLETE_CAPTURE_NOTE));
});

test("a missing baseline never reads as 0 files", () => {
  const changes = deriveCodingSessionTurnChanges({
    checkpoint: entryOf(
      payload({ git: gitFacts({ baseTree: null }), files: [] }),
    ),
    observedFiles: [],
  });
  assert.equal(changes.headline, "Baseline not captured");
  assert.equal(changes.baselineMissing, true);
  assert.doesNotMatch(changes.headline, /0 files/);
  const withObserved = deriveCodingSessionTurnChanges({
    checkpoint: entryOf(
      payload({ git: gitFacts({ baseTree: null }), files: [] }),
    ),
    observedFiles: [observedFile],
  });
  assert.equal(withObserved.observedFallback, true);
  assert.equal(withObserved.files.length, 1);
  assert.ok(withObserved.notes.some((note) => /may be incomplete/.test(note)));
  // Files listed from the transcript never carry git's label.
  assert.equal(withObserved.headline, "Baseline not captured");
  assert.notEqual(withObserved.sourceLabel, "From git");
  assert.equal(withObserved.sourceLabel, OBSERVED_SOURCE_LABEL);
});

test("unavailable: says why, per code, and keeps observed files beside it", () => {
  const notRepo = deriveCodingSessionTurnChanges({
    checkpoint: entryOf(unavailablePayload("NOT_A_REPOSITORY")),
    observedFiles: [],
  });
  assert.equal(notRepo.source, "unavailable");
  assert.equal(notRepo.headline, "No checkpoint · not a git repository");
  assert.equal(notRepo.files.length, 0);
  const timedOut = deriveCodingSessionTurnChanges({
    checkpoint: entryOf(unavailablePayload("TIMED_OUT")),
    observedFiles: [observedFile],
  });
  assert.equal(timedOut.headline, "No checkpoint · capture timed out");
  assert.equal(timedOut.observedFallback, true);
  assert.match(timedOut.notes[0], /1 changed file · observed in transcript/);
});

test("outside-turn and not-captured disclosures", () => {
  const changes = deriveCodingSessionTurnChanges({
    checkpoint: entryOf(
      payload({
        git: gitFacts({
          outsideTurn: true,
          complete: false,
          omitted: [{ path: "big.bin", reason: "too_large" }],
          omittedNotListed: 2,
        }),
      }),
    ),
    observedFiles: [],
  });
  assert.equal(changes.outsideTurn, true);
  assert.equal(changes.notCaptured, 3);
  assert.ok(changes.notes.includes(OUTSIDE_TURN_NOTE));
  assert.ok(
    changes.notes.includes("3 files not captured (too large or unreadable)"),
  );
  const tooLarge = deriveCodingSessionTurnChanges({
    checkpoint: entryOf(
      payload({
        git: gitFacts({
          complete: false,
          omitted: [{ path: "big.bin", reason: "too_large" }],
        }),
      }),
    ),
    observedFiles: [],
  });
  assert.ok(tooLarge.notes.includes("1 file not captured (too large)"));
});

test("a complete measurement of no change says nothing unless contradicted", () => {
  const quiet = entryOf(payload({ files: [] }));
  assert.equal(
    deriveCodingSessionTurnChanges({ checkpoint: quiet, observedFiles: [] }),
    null,
  );
  const contradicted = deriveCodingSessionTurnChanges({
    checkpoint: quiet,
    observedFiles: [observedFile],
  });
  assert.equal(contradicted.headline, "No files changed");
  assert.equal(contradicted.source, "git");
});

test("turn range: base→tree, previous tree for a missing baseline, else none", () => {
  const generation = generationOf(
    payload({ turnId: "t1", coverage: { fromSeq: 1, throughSeq: 5 } }),
    payload({
      turnId: "t2",
      coverage: { fromSeq: 6, throughSeq: 9 },
      git: gitFacts({ baseTree: null, tree: TREE_C }),
      files: [],
    }),
  );
  const first = resolveCodingSessionDiffRange({
    generation,
    scope: "turn",
    turn: generation.byTurnId.get("t1"),
  });
  assert.equal(first.kind, "range");
  assert.equal(first.fromTree, TREE_A);
  assert.equal(first.toTree, TREE_B);
  assert.equal(first.note, null);
  const second = resolveCodingSessionDiffRange({
    generation,
    scope: "turn",
    turn: null,
  });
  assert.equal(second.kind, "range");
  assert.equal(second.fromTree, TREE_B, "the previous checkpoint's tree");
  assert.equal(second.toTree, TREE_C);
  assert.match(second.note, /Baseline not captured/);

  const lone = generationOf(
    payload({ git: gitFacts({ baseTree: null }), files: [] }),
  );
  const none = resolveCodingSessionDiffRange({
    generation: lone,
    scope: "turn",
    turn: null,
  });
  assert.equal(none.kind, "none");
  assert.match(none.reason, /Baseline not captured/);

  const notRepo = generationOf(unavailablePayload());
  const unavailable = resolveCodingSessionDiffRange({
    generation: notRepo,
    scope: "turn",
    turn: null,
  });
  assert.equal(unavailable.kind, "none");
  assert.match(unavailable.reason, /not a git repository/);
});

test("session range: first baseline → latest tree, union of listed files", () => {
  const generation = generationOf(
    payload({ turnId: "t1", coverage: { fromSeq: 1, throughSeq: 5 } }),
    unavailablePayload("TIMED_OUT", {
      turnId: "t2",
      coverage: { fromSeq: 6, throughSeq: 7 },
    }),
    payload({
      turnId: "t3",
      coverage: { fromSeq: 8, throughSeq: 9 },
      git: gitFacts({ baseTree: TREE_B, tree: TREE_C }),
      files: [
        {
          path: "src/main.rs",
          status: "modified",
          from: null,
          additions: 1,
          deletions: 1,
        },
        {
          path: "z.txt",
          status: "added",
          from: null,
          additions: 1,
          deletions: 0,
        },
      ],
    }),
  );
  const range = resolveCodingSessionDiffRange({
    generation,
    scope: "session",
    turn: null,
  });
  assert.equal(range.kind, "range");
  assert.equal(range.fromTree, TREE_A);
  assert.equal(range.toTree, TREE_C);
  assert.deepEqual(
    range.files.map((file) => file.path),
    ["src/main.rs", "z.txt"],
  );
  assert.equal(range.files[0].additions, null, "per-turn counts do not sum");
  assert.match(range.note, /1 turn has no git checkpoint/);

  const noFirstBase = generationOf(
    payload({
      turnId: "t1",
      coverage: { fromSeq: 1, throughSeq: 5 },
      git: gitFacts({ baseTree: null }),
      files: [],
    }),
    payload({
      turnId: "t2",
      coverage: { fromSeq: 6, throughSeq: 9 },
      git: gitFacts({ tree: TREE_C }),
    }),
  );
  const shifted = resolveCodingSessionDiffRange({
    generation: noFirstBase,
    scope: "session",
    turn: null,
  });
  assert.equal(shifted.fromTree, TREE_B);
  assert.match(shifted.note, /first turn's baseline was not captured/);
});

test("patch text becomes add/remove/context lines with headers dropped", () => {
  const lines = codingSessionPatchLines(
    [
      "diff --git a/x b/x",
      "index 1..2 100644",
      "--- a/x",
      "+++ b/x",
      "@@ -1,2 +1,2 @@",
      " keep",
      "-old",
      "+new",
      "\\ No newline at end of file",
    ].join("\n"),
  );
  assert.deepEqual(
    lines.map((line) => line.kind),
    ["meta", "context", "remove", "add", "meta"],
  );
  assert.equal(lines[3].text, "new");
});
