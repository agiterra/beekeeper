import assert from "node:assert/strict";
import test from "node:test";

import { parseNewCodingSessionRequest } from "./newCodingSessionDialogStore.ts";

// What survives a reload is what this parses. The route this dialog replaced
// pinned its channel into the URL so a mid-create refresh re-attached to the
// durable transaction; the stored request is what does that job now, so a
// shape it silently mis-reads is a create that quietly loses its dialog.

test("a channel request round-trips, with or without a channel", () => {
  assert.deepEqual(
    parseNewCodingSessionRequest('{"kind":"channel","channelId":"abc"}'),
    { kind: "channel", channelId: "abc" },
  );
  assert.deepEqual(
    parseNewCodingSessionRequest('{"kind":"channel","channelId":null}'),
    { kind: "channel", channelId: null },
  );
});

test("a project request round-trips", () => {
  assert.deepEqual(
    parseNewCodingSessionRequest('{"kind":"project","projectId":"p1"}'),
    { kind: "project", projectId: "p1" },
  );
});

test("nothing stored means no dialog", () => {
  assert.equal(parseNewCodingSessionRequest(null), null);
});

test("anything that is not a request is refused rather than half-read", () => {
  for (const raw of [
    "not json",
    "null",
    "[]",
    '"channel"',
    "{}",
    '{"kind":"channel"}',
    '{"kind":"channel","channelId":7}',
    '{"kind":"project"}',
    '{"kind":"project","projectId":""}',
    '{"kind":"project","projectId":42}',
    '{"kind":"elsewhere","channelId":"abc"}',
  ]) {
    assert.equal(parseNewCodingSessionRequest(raw), null, raw);
  }
});
