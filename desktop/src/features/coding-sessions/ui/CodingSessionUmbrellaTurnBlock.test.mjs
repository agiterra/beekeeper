import assert from "node:assert/strict";
import test from "node:test";

import {
  hidesCodingSessionRehydrationClaim,
  resolveCodingSessionMissionBlockItems,
} from "./CodingSessionUmbrellaTurnBlock.tsx";
import { CODING_SESSION_CONTINUITY_STATUSES } from "../lib/codingSessionTranscriptItems.ts";

function continuity(slug) {
  return {
    id: `continuity-${slug}`,
    type: "lifecycle",
    renderClass: "status",
    title: "Session continuity",
    text: CODING_SESSION_CONTINUITY_STATUSES.get(slug),
    timestamp: "2026-09-01T21:34:22.000Z",
  };
}

test("A3.3: a fresh seat's rehydration claim is omitted in Mission", () => {
  assert.equal(
    hidesCodingSessionRehydrationClaim({
      hasPriorGeneration: false,
      item: continuity("session_rehydrated"),
      mission: true,
    }),
    true,
  );
});

test("A3.3: the same claim stands when this umbrella holds a prior generation", () => {
  assert.equal(
    hidesCodingSessionRehydrationClaim({
      hasPriorGeneration: true,
      item: continuity("session_rehydrated"),
      mission: true,
    }),
    false,
  );
});

test("A3.3: Conversation is untouched — the wire keeps its claim", () => {
  assert.equal(
    hidesCodingSessionRehydrationClaim({
      hasPriorGeneration: false,
      item: continuity("session_rehydrated"),
      mission: false,
    }),
    false,
  );
});

test("A3.3: the other continuity slugs a fresh seat can honestly say are kept", () => {
  for (const slug of [
    "session_fresh",
    "session_resumed",
    "session_loaded",
    "session_restarted_without_context",
  ]) {
    assert.equal(
      hidesCodingSessionRehydrationClaim({
        hasPriorGeneration: false,
        item: continuity(slug),
        mission: true,
      }),
      false,
      slug,
    );
  }
});

/**
 * F8: reference stability. `block.items` feeds a `useMemo` downstream, so the
 * filter must not mint a new array for every Mission block of a fresh seat —
 * only for the one block that actually carries the row.
 */
test("A3.3/F8: a block with no rehydration row keeps its own items array", () => {
  const items = [continuity("session_fresh")];
  assert.equal(
    resolveCodingSessionMissionBlockItems({
      hasPriorGeneration: false,
      items,
      mission: true,
    }),
    items,
  );
  assert.equal(
    resolveCodingSessionMissionBlockItems({
      hasPriorGeneration: true,
      items: [continuity("session_rehydrated")],
      mission: true,
    }).length,
    1,
  );
});

test("A3.3/F8: the block that does carry the row gets it filtered out", () => {
  const items = [continuity("session_rehydrated"), continuity("session_fresh")];
  const resolved = resolveCodingSessionMissionBlockItems({
    hasPriorGeneration: false,
    items,
    mission: true,
  });
  assert.notEqual(resolved, items);
  assert.deepEqual(
    resolved.map((item) => item.text),
    [CODING_SESSION_CONTINUITY_STATUSES.get("session_fresh")],
  );
});

test("A3.3/F8: Conversation always gets the very same array back", () => {
  const items = [continuity("session_rehydrated")];
  assert.equal(
    resolveCodingSessionMissionBlockItems({
      hasPriorGeneration: false,
      items,
      mission: false,
    }),
    items,
  );
});
