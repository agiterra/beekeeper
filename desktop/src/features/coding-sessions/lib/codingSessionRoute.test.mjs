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

test("surface parsing is fail-closed to the main Beekeeper chrome", () => {
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

test("the founded route encodes both coordinates and never mints a generation id", async () => {
  const {
    CODING_SESSION_FOUNDED_ROUTE,
    buildFoundedCodingSessionPath,
    foundedCodingSessionRowId,
    parseFoundedCodingSessionRowId,
  } = await import("./codingSessionRoute.ts");
  assert.equal(
    CODING_SESSION_FOUNDED_ROUTE,
    "/coding-sessions/$channelId/founded/$sessionRef",
  );
  assert.equal(
    buildFoundedCodingSessionPath("channel/a", "ref|1"),
    "/coding-sessions/channel%2Fa/founded/ref%7C1",
  );
  const [, , encodedChannel, segment, encodedRef] =
    buildFoundedCodingSessionPath("channel%2Fa", "ref%7C1").split("/");
  assert.equal(segment, "founded");
  assert.equal(decodeURIComponent(encodedChannel), "channel%2Fa");
  assert.equal(decodeURIComponent(encodedRef), "ref%7C1");

  // The row id lives in the generation slot of a shelf row. It mirrors the
  // pending row's `pending:<commandId>` and can never be mistaken for a real
  // generation key, which carries the structured-key prefix.
  const rowId = foundedCodingSessionRowId(
    "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10",
  );
  assert.equal(rowId, "founded:5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10");
  assert.equal(rowId.startsWith("coding-session-transcript-generation"), false);
  assert.equal(rowId.startsWith("pending:"), false);
  assert.equal(
    parseFoundedCodingSessionRowId(rowId),
    "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10",
  );
  assert.equal(parseFoundedCodingSessionRowId("generation-1"), null);
  assert.equal(parseFoundedCodingSessionRowId("pending:csl-1"), null);
});
