import assert from "node:assert/strict";
import test from "node:test";

import { awaitingSentence } from "./CodingSessionMissionSettlement.tsx";

const ASSIGNMENT = "5c".repeat(32);
const ACTOR = "22".repeat(32);

test("a settled chain says so", () => {
  assert.equal(
    awaitingSentence({
      assignmentEventId: ASSIGNMENT,
      settled: true,
      awaiting: null,
    }),
    "settled",
  );
});

test("the missing link and its owner are named, never three nulls", () => {
  // Ledger 178(e): this is the sentence whose absence made a lead recall a
  // verifier for a turn it did not owe.
  assert.equal(
    awaitingSentence(
      {
        assignmentEventId: ASSIGNMENT,
        settled: false,
        awaiting: {
          link: "acknowledgement",
          owedByRole: "builder",
          owedByActor: ACTOR,
        },
      },
      () => "Kiln",
    ),
    "awaiting acknowledgement by builder Kiln",
  );
});

test("a disposition names no one actor, and says why", () => {
  const sentence = awaitingSentence({
    assignmentEventId: ASSIGNMENT,
    settled: false,
    awaiting: { link: "disposition", owedByRole: "lead", owedByActor: null },
  });
  assert.match(sentence, /awaiting disposition by lead/);
  assert.match(sentence, /any active lead seat or any steer-grant holder/);
});

test("an unsettled chain with no diagnosis discloses that, not silence", () => {
  assert.match(
    awaitingSentence({
      assignmentEventId: ASSIGNMENT,
      settled: false,
      awaiting: null,
    }),
    /did not say which link is missing/,
  );
});
