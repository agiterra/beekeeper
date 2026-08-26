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
    "Queued by the provider",
  );
  assert.equal(
    describePendingCodingSessionTurn("queued", 4 * 60_000),
    "Queued by the provider, not started yet — 4m",
  );
});

test("a degraded steer names the downgrade rather than reading as a queue", () => {
  assert.equal(
    describePendingCodingSessionTurn("degraded", 1_000),
    "Delivered at the next turn boundary — this provider cannot steer",
  );
  assert.equal(
    describePendingCodingSessionTurn("degraded", 90_000),
    "Delivered at the next turn boundary — this provider cannot steer; not started yet — 1m",
  );
});
