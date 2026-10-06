import assert from "node:assert/strict";
import test from "node:test";

import {
  annotateHiddenContext,
  hiddenContextDescription,
  hiddenContextLabel,
  isPathShapedContext,
} from "./hiddenContext.ts";
import { parseRedactionMarkers } from "./redactionMarker.ts";

const DIGEST =
  "eb7930a9a9209e69d829efa946d4ebea3f2e32c6b03f3317321c53bf33597e3f";
const MARKER = `[elided private context: 148 bytes, sha256:${DIGEST}]`;

function annotated(text, options) {
  return annotateHiddenContext(parseRedactionMarkers(text), options).filter(
    (segment) => segment.kind === "redaction",
  );
}

test("a separator touching the marker makes it a path", () => {
  assert.equal(isPathShapedContext("", "/src/main.rs"), true);
  assert.equal(isPathShapedContext("~/", ""), true);
  assert.equal(isPathShapedContext("C:\\Users\\", ""), true);
});

test("a path-naming key or a path-taking command makes it a path", () => {
  assert.equal(isPathShapedContext('{"cwd": "', '"}'), true);
  assert.equal(isPathShapedContext("working directory: ", ""), true);
  assert.equal(isPathShapedContext("cd ", " && ls"), true);
  assert.equal(isPathShapedContext("git -C ", " status"), true);
  assert.equal(isPathShapedContext("mkdir -p ", ""), true);
});

test("ordinary prose and secrets are only `hidden`", () => {
  assert.equal(isPathShapedContext("The token was ", "."), false);
  assert.equal(isPathShapedContext("Authorization: Bearer ", ""), false);
  assert.equal(isPathShapedContext("", ""), false);
  // A path key on an earlier line does not name this value.
  assert.equal(isPathShapedContext("cwd:\nthe answer is ", ""), false);
});

test("a marker alone on its line is a path only when the caller says so", () => {
  const text = `${MARKER}\n`;
  assert.equal(annotated(text)[0].pathShaped, false);
  assert.equal(
    annotated(text, { wholeLinesArePaths: true })[0].pathShaped,
    true,
  );
  // Not alone on its line: the flag does not apply.
  assert.equal(
    annotated(`see ${MARKER}`, { wholeLinesArePaths: true })[0].pathShaped,
    false,
  );
});

test("each marker is judged by its own neighbours", () => {
  const [first, second] = annotated(`cd ${MARKER} && echo ${MARKER}`);
  assert.equal(first.pathShaped, true);
  assert.equal(second.pathShaped, false);
});

test("labels and the description never carry the content", () => {
  assert.equal(hiddenContextLabel(true), "hidden path");
  assert.equal(hiddenContextLabel(false), "hidden");
  assert.equal(
    hiddenContextDescription({ bytes: 148, digest: DIGEST }),
    "Hidden before publishing — 148 bytes, sha256 eb7930a9a920…",
  );
  assert.equal(
    hiddenContextDescription({ bytes: 1, digest: DIGEST }),
    "Hidden before publishing — 1 byte, sha256 eb7930a9a920…",
  );
  assert.equal(
    hiddenContextDescription({ bytes: Number.NaN, digest: "" }),
    "Hidden before publishing — size unknown, no digest recorded",
  );
});
