/**
 * C5 lane VB: the preview broadcast pump's watch filter (WIRE-C5 § 3).
 */
import assert from "node:assert/strict";
import test from "node:test";

import { parsePreviewWatchEvent } from "./previewBroadcastWatch.ts";

const ME = "a".repeat(64);
const WATCHER = "b".repeat(64);
const CHANNEL = "6f1c1c0e-6a8e-4c38-9d0f-1a2b3c4d5e6f";
const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";

function watch(overrides = {}) {
  return {
    pubkey: WATCHER,
    content: JSON.stringify({ action: "watch" }),
    tags: [
      ["h", CHANNEL],
      ["surface", "preview"],
      ["d", SESSION_REF],
      ["p", ME],
    ],
    ...overrides,
  };
}

test("a preview watch addressed to me is forwarded", () => {
  assert.deepEqual(parsePreviewWatchEvent(watch(), ME), {
    channelId: CHANNEL,
    sessionRef: SESSION_REF,
    watcherPubkey: WATCHER,
    action: "watch",
  });
  for (const action of ["stop", "resync", "snapshot"]) {
    const parsed = parsePreviewWatchEvent(
      watch({ content: JSON.stringify({ action }) }),
      ME,
    );
    assert.equal(parsed?.action, action);
  }
});

test("device watches, other producers and bad shapes are dropped", () => {
  const tags = watch().tags;
  const cases = [
    {
      tags: [
        tags[0],
        ["surface", "device"],
        ["d", "9f2c4e1a7b3d5c80"],
        tags[3],
      ],
    },
    { tags: [tags[0], tags[1], tags[2], ["p", "c".repeat(64)]] },
    // Wire order is exact.
    { tags: [tags[1], tags[0], tags[2], tags[3]] },
    { tags: [...tags, ["x", "1"]] },
    { tags: [["h", CHANNEL.toUpperCase()], tags[1], tags[2], tags[3]] },
    { tags: [tags[0], tags[1], ["d", "not-a-uuid"], tags[3]] },
    { tags: [tags[0], tags[1], tags[2], ["p", ME, "extra"]] },
    { content: '{"action":"watch","x":1}' },
    { content: '{"action":"poke"}' },
    { content: "not json" },
    { content: "[]" },
    { pubkey: ME },
    { pubkey: "short" },
  ];
  for (const overrides of cases) {
    assert.equal(
      parsePreviewWatchEvent(watch(overrides), ME),
      null,
      JSON.stringify(overrides),
    );
  }
});
