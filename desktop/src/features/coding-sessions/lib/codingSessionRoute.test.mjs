import assert from "node:assert/strict";
import test from "node:test";

import {
  buildCodingSessionPath,
  buildCodingSessionPopoutUrl,
  buildCodingSessionWindowLabel,
  isCodingSessionPopoutLocation,
  parseCodingSessionSurface,
} from "./codingSessionRoute.ts";

test("coding-session route carries only channel and exact generation", () => {
  assert.equal(
    buildCodingSessionPath("channel/a", "generation|1"),
    "/coding-sessions/channel%2Fa/generation%7C1",
  );
  assert.equal(
    buildCodingSessionPopoutUrl("channel/a", "generation|1"),
    "/#/coding-sessions/channel%2Fa/generation%7C1?surface=popout",
  );
  const [, , encodedChannel, encodedGeneration] = buildCodingSessionPath(
    "channel%2Fa",
    "generation%7C1",
  ).split("/");
  assert.equal(decodeURIComponent(encodedChannel), "channel%2Fa");
  assert.equal(decodeURIComponent(encodedGeneration), "generation%7C1");
});

test("surface parsing is fail-closed to the main Buzz chrome", () => {
  assert.equal(parseCodingSessionSurface("popout"), "popout");
  assert.equal(parseCodingSessionSurface("provider"), "main");
  assert.equal(parseCodingSessionSurface(undefined), "main");
  assert.equal(
    isCodingSessionPopoutLocation({
      pathname: "/coding-sessions/channel/generation",
      search: { surface: "popout" },
    }),
    true,
  );
  assert.equal(
    isCodingSessionPopoutLocation({
      pathname: "/channels/channel",
      search: { surface: "popout" },
    }),
    false,
  );
});

test("popout label is deterministic, route-specific, and Tauri-safe", () => {
  const label = buildCodingSessionWindowLabel("channel/a", "generation|1");
  assert.equal(
    label,
    buildCodingSessionWindowLabel("channel/a", "generation|1"),
  );
  assert.notEqual(
    label,
    buildCodingSessionWindowLabel("channel/a", "generation|2"),
  );
  assert.match(label, /^coding-session-[a-z0-9-]+$/);
});
