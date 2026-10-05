/**
 * The shared display-name vectors (`conformance/session-display-name/`), run
 * against the web resolver. The fixture is frozen: a reader change never
 * edits it to pass. Every vector is checked forwards and reversed, because
 * the rule must not depend on arrival order.
 */
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { test } from "node:test";

import {
  KIND_CODING_SESSION_GENERATED_TITLE,
  KIND_CODING_SESSION_NAME,
} from "../../../shared/lib/kinds.ts";
import {
  CODING_SESSION_TITLE_SCHEMA,
  CODING_SESSION_TITLE_TAG_VERSION,
  isValidCodingSessionTargetKey,
  MAX_CODING_SESSION_TITLE_CONTENT_BYTES,
  MAX_CODING_SESSION_TITLE_MODEL_BYTES,
  MAX_SESSION_DISPLAY_NAME_BYTES,
  parseCodingSessionTitleParts,
  resolveSessionDisplayName,
  UNTITLED_SESSION_NAME,
} from "./sessionTitle.ts";

const fixture = JSON.parse(
  readFileSync(
    resolve(
      process.cwd(),
      "../conformance/session-display-name/fixtures/vectors.json",
    ),
    "utf8",
  ),
);

test("the fixture is the schema and constants this build speaks", () => {
  assert.equal(fixture.schema, "buzz.conformance/session-display-name@1");
  assert.deepEqual(fixture.constants, {
    kindName: KIND_CODING_SESSION_NAME,
    kindGeneratedTitle: KIND_CODING_SESSION_GENERATED_TITLE,
    tagVersion: CODING_SESSION_TITLE_TAG_VERSION,
    payloadSchema: CODING_SESSION_TITLE_SCHEMA,
    maxTitleBytes: MAX_SESSION_DISPLAY_NAME_BYTES,
    maxContentBytes: MAX_CODING_SESSION_TITLE_CONTENT_BYTES,
    maxModelBytes: MAX_CODING_SESSION_TITLE_MODEL_BYTES,
    untitled: UNTITLED_SESSION_NAME,
  });
  assert.ok(fixture.envelopes.length > 0);
  assert.ok(fixture.vectors.length > 0);
});

for (const envelope of fixture.envelopes) {
  test(`envelope ${envelope.name}`, () => {
    const decoded = parseCodingSessionTitleParts(
      envelope.event.tags,
      envelope.event.content,
    );
    assert.equal(
      decoded !== null,
      envelope.valid,
      `${envelope.name}: ${envelope.description}`,
    );
  });
}

for (const vector of fixture.vectors) {
  for (const [direction, events] of [
    ["forwards", vector.events],
    ["reversed", [...vector.events].reverse()],
  ]) {
    test(`vector ${vector.name} (${direction})`, () => {
      assert.deepEqual(
        resolveSessionDisplayName(vector.scope, events),
        vector.expected,
        `${vector.name}: ${vector.description}`,
      );
    });
  }
}

test("a target key must re-encode to itself", () => {
  const good = "coding-session/v1|10:provider-a10:instance-19:session-11:1";
  assert.equal(isValidCodingSessionTargetKey(good), true);
  assert.equal(
    isValidCodingSessionTargetKey(good.replace("1:1", "2:01")),
    false,
  );
  assert.equal(
    isValidCodingSessionTargetKey(good.replace("1:1", "2:+1")),
    false,
  );
  assert.equal(
    isValidCodingSessionTargetKey(
      "coding-session/v1|10:provider-a10:instance-19:session-116:9007199254740992",
    ),
    false,
    "a generation past the safe-integer bound is refused",
  );
  // A multi-byte field is counted in UTF-8 bytes, not UTF-16 units.
  assert.equal(
    isValidCodingSessionTargetKey(
      "coding-session/v1|2:é10:instance-19:session-11:1",
    ),
    true,
  );
  assert.equal(
    isValidCodingSessionTargetKey(
      "coding-session/v1|1:é10:instance-19:session-11:1",
    ),
    false,
  );
});
