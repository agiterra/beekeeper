import assert from "node:assert/strict";
import test from "node:test";

import { CODING_SESSION_BOUNDARY_TITLE } from "../lib/codingSessionBoundaryStatus.ts";
import { CODING_SESSION_CONTINUITY_TITLE } from "../lib/codingSessionTranscriptItems.ts";
import { codingSessionSessionFacts } from "./CodingSessionWorkspaceSessionFacts.ts";

const boundary = {
  id: "b",
  type: "lifecycle",
  renderClass: "status",
  title: CODING_SESSION_BOUNDARY_TITLE,
  text: "x",
};
const continuity = {
  id: "c",
  type: "lifecycle",
  renderClass: "status",
  title: CODING_SESSION_CONTINUITY_TITLE,
  text: "y",
};
const message = (id) => ({ id, type: "message", role: "assistant", text: id });

test("only continuity and boundary rows are session facts, in order", () => {
  const facts = codingSessionSessionFacts(
    [continuity, message("m1"), boundary, message("m2")],
    null,
  );
  assert.deepEqual(
    facts.map((item) => item.id),
    ["c", "b"],
  );
});

test("a streamed item that is not a fact keeps the same facts array", () => {
  const first = codingSessionSessionFacts([continuity, boundary], null);
  const next = codingSessionSessionFacts(
    [continuity, boundary, message("m1")],
    first,
  );
  assert.equal(next, first);
  const changed = codingSessionSessionFacts(
    [continuity, boundary, { ...boundary, id: "b2" }],
    first,
  );
  assert.notEqual(changed, first);
  assert.equal(changed.length, 3);
});
