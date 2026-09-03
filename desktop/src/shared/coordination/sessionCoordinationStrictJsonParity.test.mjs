/**
 * Parity between the Pulse's strict gate and the session decoder.
 *
 * `isStrictMetadataContent` (this directory) and `parseBuzzCodingSessionMetadata`
 * (`features/coding-sessions/lib/codingSessionIngressPayloads.ts`) both read
 * kind 44223. They are allowed to disagree in one direction only: the gate may
 * be *more* open than the decoder (finding 34's fix makes `capabilities` and a
 * few routing tokens open sets the decoder still closes), but the gate must
 * never be *stricter* — never refuse a shape the decoder itself accepts. A
 * gate that is stricter than the decoder is exactly finding 34: real signed
 * events the rest of the app can read, quietly excluded from the Pulse.
 *
 * This is a black-box check across the same fixtures, not an import of one
 * decoder's internals into the other — the two are intentionally separate
 * implementations (this module stays dependency-free for the conformance
 * binder; the session decoder does not), so the only trustworthy comparison
 * is "same bytes in, does each accept them".
 */
import assert from "node:assert/strict";
import test from "node:test";

import { parseBuzzCodingSessionMetadata } from "../../features/coding-sessions/lib/codingSessionIngressPayloads.ts";
import { isStrictMetadataContent } from "./sessionCoordinationStrictJson.ts";

const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const ACTOR = "c".repeat(64);

const BASE = {
  schema: "buzz-coding-session-metadata/v1",
  session: {
    driver: "claude-agent-acp",
    instanceId: "0123456789abcdef",
    sessionId: "11111111-2222-3333-4444-555555555555",
    generation: 1,
  },
  projectRef: "30621:owner:agiterra",
  repoRef: null,
  title: "Advance coding sessions",
  agentRef: null,
  provider: "claude-primary",
  runtime: "claude",
  model: "claude-opus-5",
  status: "running",
  branch: null,
};

const FACTS = {
  observedCommit: "a".repeat(40),
  dirty: false,
  relayReachable: true,
  verifiedAt: 1_800_000_000,
};

const ROUTING = {
  class: "builder",
  tier: "standard",
  risk: { impact: 3, uncertainty: 3, irreversibility: 2, score: 18 },
  chosen: { provider: "claude-primary", model: "sonnet", effort: "medium" },
  runnerUp: null,
  reason: "cleared the builder gates and was the cheapest of them.",
  reviewRequired: false,
  reviewReasons: [],
  challengerSample: false,
  override: null,
  registryVersion: 1,
  catalogRevision: null,
};

const BEE_STAMP = {
  path: "/Applications/Beekeeper.app/Contents/Resources/bee",
  source: "bundled",
  version: "0.9.3",
  sha: "a1b2c3d",
  dirty: false,
};

const CAPABILITIES_SIX_KEY = {
  threadTurnStart: true,
  threadTurnInterrupt: true,
  threadSteer: false,
  context: false,
  diff: false,
  plan: true,
};

/** One real fixture per row: a name and its metadata content string. */
function fixtures() {
  const rows = [
    ["the six-key base shape", {}],
    ["sessionRef only", { sessionRef: SESSION_REF }],
    ["a seated create", { agentRef: ACTOR, role: "builder" }],
    [
      "a turnBudget beside its sessionRef",
      { sessionRef: SESSION_REF, turnBudget: { used: 3, limit: 20 } },
    ],
    ["the four B1 facts together", FACTS],
    ["a routing record", { routing: ROUTING }],
    // Finding 34's own case: a newer host's promptImage capability.
    [
      "capabilities carrying promptImage",
      { capabilities: { ...CAPABILITIES_SIX_KEY, promptImage: false } },
    ],
    // The sixth amendment this lane also found missing.
    ["a real beeStamp", { beeStamp: BEE_STAMP }],
    [
      "an unparsed beeStamp",
      {
        beeStamp: { ...BEE_STAMP, version: null, sha: null, dirty: null },
      },
    ],
    [
      "every amendment at once",
      {
        sessionRef: SESSION_REF,
        agentRef: ACTOR,
        role: "builder",
        turnBudget: { used: 3, limit: 20 },
        ...FACTS,
        routing: ROUTING,
        beeStamp: BEE_STAMP,
        capabilities: { ...CAPABILITIES_SIX_KEY, promptImage: true },
      },
    ],
  ];
  return rows.map(([name, overrides]) => [
    name,
    JSON.stringify({
      ...BASE,
      capabilities: CAPABILITIES_SIX_KEY,
      ...overrides,
    }),
  ]);
}

test("the strict gate never rejects a shape the session decoder accepts", () => {
  for (const [name, source] of fixtures()) {
    const decoded = parseBuzzCodingSessionMetadata(source);
    if (decoded === null) {
      // Not every fixture here is decoder-accepted (e.g. `futureThing` below
      // covers the direction that is *allowed* to diverge) — this loop only
      // asserts the direction that must never diverge.
      continue;
    }
    assert.equal(
      isStrictMetadataContent(source),
      true,
      `decoder accepted "${name}" but the strict gate refused it: ${source}`,
    );
  }
});

test("every real fixture is in fact decoder-accepted (the test proves something)", () => {
  // Guards the test above against a silent no-op: if every fixture failed to
  // decode, the loop above would pass trivially without checking anything.
  let decoded = 0;
  for (const [, source] of fixtures()) {
    if (parseBuzzCodingSessionMetadata(source) !== null) decoded += 1;
  }
  assert.ok(
    decoded >= fixtures().length - 1,
    `only ${decoded} fixtures decoded`,
  );
});

test("the gate may be more open than the decoder, never stricter — the one allowed divergence", () => {
  // `futureThing` is accepted by the fixed strict gate (open capabilities
  // map) but the session decoder's `decodeCapabilities` only knows
  // `promptImage` as an additional optional key, so it refuses this one.
  // That is the *allowed* direction: the gate being more forgiving than the
  // decoder never drops a real session, it only means the gate does not
  // itself reject something the decoder happens to be pickier about.
  const source = JSON.stringify({
    ...BASE,
    capabilities: { ...CAPABILITIES_SIX_KEY, futureThing: true },
  });
  assert.equal(parseBuzzCodingSessionMetadata(source), null);
  assert.equal(isStrictMetadataContent(source), true);
});
