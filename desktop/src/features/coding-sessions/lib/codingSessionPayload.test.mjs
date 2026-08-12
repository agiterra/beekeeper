import assert from "node:assert/strict";
import test from "node:test";

import {
  areCanonicalProjectionPayloadsDuplicate,
  isWithinProjectionDepth,
  MAX_PROJECTION_DEPTH,
} from "./codingSessionPayload.ts";

function nested(depth) {
  let value = "leaf";
  for (let index = 0; index < depth; index += 1) {
    value = { value };
  }
  return value;
}

test("projection depth probe accepts the ceiling and rejects over-deep payloads", () => {
  assert.equal(isWithinProjectionDepth(nested(MAX_PROJECTION_DEPTH)), true);
  assert.equal(
    isWithinProjectionDepth(nested(MAX_PROJECTION_DEPTH + 1)),
    false,
  );
});

test("null canonical payloads never classify as duplicates", () => {
  assert.equal(areCanonicalProjectionPayloadsDuplicate(null, null), false);
  assert.equal(areCanonicalProjectionPayloadsDuplicate(null, "{}"), false);
  assert.equal(areCanonicalProjectionPayloadsDuplicate("{}", null), false);
  assert.equal(areCanonicalProjectionPayloadsDuplicate("{}", "{}"), true);
});
