/**
 * Wiring guard for the community-switch reset contract (AGENTS.md "Community
 * Switching"): every module-level, community-scoped cache must be torn down in
 * `resetCommunityState`. `useCommunityInit` itself cannot be imported here (it
 * pulls the Tauri bridge), so this asserts on the source of that one function
 * — enough to catch a new registry being added without a reset.
 */

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import {
  isCodingSessionLaneMessageHiddenFromChannel,
  observeCodingSessionLaneRefs,
  publishCodingSessionLaneRenderableRefs,
  resetCodingSessionLaneVisibility,
  codingSessionLaneObservedChannelIds,
} from "@/features/messages/lib/codingSessionLaneVisibility";

const here = path.dirname(fileURLToPath(import.meta.url));

function resetCommunityStateSource() {
  const source = readFileSync(path.join(here, "useCommunityInit.ts"), "utf8");
  const declared = source.indexOf("function resetCommunityState(");
  assert.ok(
    declared >= 0,
    "resetCommunityState must exist in useCommunityInit",
  );
  // The signature destructures across lines, so anchor the body on its first
  // statement rather than on the opening brace.
  const start = source.indexOf("relayClient.disconnect();", declared);
  assert.ok(start >= 0, "resetCommunityState must still disconnect the relay");
  const end = source.indexOf("\n}\n", start);
  return source.slice(start, end);
}

test("resetCommunityState drops the relay-self identity query cache", () => {
  // `relaySelfQueryKey` is a GLOBAL React Query key (call sites read it with an
  // exact-key getQueryData, so it cannot be relay-scoped) and the QueryClient
  // outlives the community remount. Without an explicit removal here, community
  // B would classify relay-signed state against community A's relay identity.
  assert.match(
    resetCommunityStateSource(),
    /queryClient\.removeQueries\(\{\s*queryKey:\s*relaySelfQueryKey\s*\}\)/,
  );
});

test("resetCommunityState tears down the coding-session lane registry", () => {
  assert.match(
    resetCommunityStateSource(),
    /resetCodingSessionLaneVisibility\(\);/,
  );
});

test("resetCodingSessionLaneVisibility clears observed and renderable refs", () => {
  const channelId = "9f1c0b1a-2d3e-4f50-8a61-7b2c3d4e5f60";
  const sessionRef = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
  const laneEvent = {
    kind: 9,
    tags: [
      ["h", channelId],
      ["cs-session", sessionRef],
    ],
  };

  observeCodingSessionLaneRefs(channelId, [laneEvent]);
  publishCodingSessionLaneRenderableRefs(channelId, new Set([sessionRef]));
  assert.equal(
    isCodingSessionLaneMessageHiddenFromChannel(channelId, laneEvent),
    true,
  );

  resetCodingSessionLaneVisibility();

  assert.deepEqual(codingSessionLaneObservedChannelIds(), []);
  // Post-reset the ref is unknown again, so the message is ordinary chat —
  // never hidden with no lane to render it.
  assert.equal(
    isCodingSessionLaneMessageHiddenFromChannel(channelId, laneEvent),
    false,
  );
});
