/**
 * What a pending row is allowed to claim.
 *
 * These captions are the only thing standing between a person and a wrong
 * story about their own message: a turn the provider has parked behind an
 * hour of work must not read the same as one nobody ever picked up, and a
 * steer that was quietly downgraded to a boundary delivery must say so — the
 * whole point of asking to steer was to reach the turn that is running now.
 */
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

import { PENDING_CODING_SESSION_TURN_STALL_MS } from "../lib/codingSessionPendingTurns.ts";
import { describePendingCodingSessionTurn } from "./CodingSessionPendingTurns.tsx";

const STALLED = PENDING_CODING_SESSION_TURN_STALL_MS + 1;

test("an ordinary in-flight turn says nothing at all", () => {
  assert.equal(describePendingCodingSessionTurn("sending", 0), null);
  assert.equal(describePendingCodingSessionTurn("waiting", 0), null);
});

test("a turn nobody answered says so, and only that", () => {
  assert.equal(
    describePendingCodingSessionTurn("stalled", STALLED),
    "Not picked up yet",
  );
});

test("a queued turn ages out loud instead of expiring", () => {
  assert.equal(
    describePendingCodingSessionTurn("queued", 1_000),
    "Queued by the provider; it cannot be recalled",
  );
  assert.equal(
    describePendingCodingSessionTurn("queued", 4 * 60_000),
    "Queued by the provider, not started yet — 4m; it cannot be recalled",
  );
});

test("a held row says out loud that it cannot be taken back", () => {
  // The client-side queue this slice retired had a Cancel button beside the
  // words "Queued by the provider". They now mean something else: the command
  // is signed, published, and the provider's. Saying so only in a `title`
  // leaves every touch user, and most screen-reader users, reading the old
  // sentence with none of the new meaning.
  for (const state of ["queued", "degraded"]) {
    for (const age of [1_000, 4 * 60_000]) {
      assert.match(
        describePendingCodingSessionTurn(state, age),
        /cannot be recalled/,
        `${state} at ${age}ms`,
      );
    }
  }
  const source = readFileSync(
    new URL("./CodingSessionPendingTurns.tsx", import.meta.url),
    "utf8",
  );
  assert.doesNotMatch(
    source,
    /title=\{[\s\S]*?cannot be recalled/,
    "the sentence belongs on screen, not only in a tooltip",
  );
});

test("a degraded steer names the downgrade rather than reading as a queue", () => {
  assert.equal(
    describePendingCodingSessionTurn("degraded", 1_000),
    "Delivered at the next turn boundary — this provider cannot steer; it cannot be recalled",
  );
  assert.equal(
    describePendingCodingSessionTurn("degraded", 90_000),
    "Delivered at the next turn boundary — this provider cannot steer; not started yet — 1m; it cannot be recalled",
  );
});
