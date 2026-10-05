import assert from "node:assert/strict";
import { test } from "node:test";

import {
  CODING_SESSION_QUIET_AFTER_MS,
  codingSessionQuietMs,
  formatCodingSessionWorkingLabel,
} from "./CodingSessionTranscriptWorking.tsx";

const startedAt = "2026-10-05T15:30:00.000Z";
const start = Date.parse(startedAt);

test("a working turn whose provider is publishing reads as plain working", () => {
  const now = start + 322_000;
  assert.equal(
    formatCodingSessionWorkingLabel(startedAt, now, now - 5_000),
    "Working for 5m 22s",
  );
  // Exactly the threshold is not yet quiet.
  assert.equal(
    codingSessionQuietMs(now - CODING_SESSION_QUIET_AFTER_MS, now),
    null,
  );
});

test("over a minute with no transcript event discloses the silence", () => {
  // 2026-10-05: 4.5 minutes of nothing read "Working for 5m 22s".
  const now = start + 322_000;
  const lastEventAt = now - 270_000;
  assert.equal(
    formatCodingSessionWorkingLabel(startedAt, now, lastEventAt),
    "Working for 5m 22s · no update for 4m",
  );
  assert.equal(codingSessionQuietMs(lastEventAt, now), 240_000);
  // No known start still says so, without inventing a duration.
  assert.equal(
    formatCodingSessionWorkingLabel(null, now, now - 125_000),
    "Working · no update for 2m",
  );
});

test("the disclosure clears when events resume", () => {
  const now = start + 322_000;
  assert.match(
    formatCodingSessionWorkingLabel(startedAt, now, now - 200_000),
    /no update/,
  );
  assert.doesNotMatch(
    formatCodingSessionWorkingLabel(startedAt, now + 1_000, now + 500),
    /no update/,
  );
});

test("an unknown last event time claims nothing about silence", () => {
  const now = start + 600_000;
  assert.equal(codingSessionQuietMs(null, now), null);
  assert.equal(codingSessionQuietMs(Number.NaN, now), null);
  assert.equal(
    formatCodingSessionWorkingLabel(startedAt, now, null),
    "Working for 10m",
  );
  assert.equal(formatCodingSessionWorkingLabel(null, now), "Working…");
  // A provider clock ahead of ours is not silence either.
  assert.equal(codingSessionQuietMs(now + 30_000, now), null);
});
