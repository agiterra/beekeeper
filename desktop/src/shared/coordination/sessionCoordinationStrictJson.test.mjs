import assert from "node:assert/strict";
import test from "node:test";

import {
  hasStrictLifecycleCommandJson,
  hasStrictLifecycleCommandValues,
  hasStrictLifecycleReceiptJson,
  hasStrictLifecycleReceiptValues,
  hasStrictMetadataJson,
  isStrictMetadataContent,
} from "./sessionCoordinationStrictJson.ts";

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
  capabilities: {
    threadTurnStart: true,
    threadTurnInterrupt: true,
    threadSteer: false,
    context: false,
    diff: false,
    plan: true,
  },
};

const FACTS = {
  observedCommit: "a".repeat(40),
  dirty: false,
  relayReachable: true,
  verifiedAt: 1_800_000_000,
};

/** Metadata content in the key order the Rust producer serializes. */
function metadata(overrides = {}) {
  return JSON.stringify({ ...BASE, ...overrides });
}

test("the sixteen shapes buzz-core accepts all decode", () => {
  const amendments = [
    { sessionRef: SESSION_REF },
    { agentRef: ACTOR, role: "builder" },
    { turnBudget: { used: 3, limit: 20 } },
    FACTS,
  ];
  let accepted = 0;
  for (let mask = 0; mask < 16; mask += 1) {
    const content = {};
    for (const [index, amendment] of amendments.entries()) {
      if (mask & (1 << index)) Object.assign(content, amendment);
    }
    // A budget describes an umbrella, so the two shapes that carry it always
    // carry a sessionRef too — that is the pairing buzz-core enforces, not an
    // extra shape.
    if (content.turnBudget) content.sessionRef = SESSION_REF;
    const source = metadata(content);
    assert.equal(
      isStrictMetadataContent(source),
      true,
      `shape ${mask} was rejected: ${source}`,
    );
    accepted += 1;
  }
  assert.equal(accepted, 16);
});

test("a turnBudget beside a sessionRef is accepted, not dropped", () => {
  const source = metadata({
    sessionRef: SESSION_REF,
    turnBudget: { used: 7, limit: 20 },
  });
  assert.equal(isStrictMetadataContent(source), true);
  assert.equal(hasStrictMetadataJson(source, JSON.parse(source)), true);
});

test("an explicit null role or turnBudget reads as absent, as Rust reads it", () => {
  assert.equal(isStrictMetadataContent(metadata({ role: null })), true);
  assert.equal(
    isStrictMetadataContent(
      metadata({ sessionRef: SESSION_REF, turnBudget: null }),
    ),
    true,
  );
});

test("a role without an actor is malformed, and so is a bad slug", () => {
  assert.equal(isStrictMetadataContent(metadata({ role: "builder" })), false);
  assert.equal(
    isStrictMetadataContent(metadata({ agentRef: ACTOR, role: "Builder" })),
    false,
  );
});

test("a turnBudget is refused without a sessionRef, and every malformed shape is refused", () => {
  assert.equal(
    isStrictMetadataContent(metadata({ turnBudget: { used: 1, limit: 20 } })),
    false,
  );
  const bad = [
    { used: 1 },
    { used: 1, limit: 0 },
    { used: -1, limit: 20 },
    { used: 1.5, limit: 20 },
    { used: 1, limit: 20, extra: true },
    { used: "1", limit: 20 },
    [1, 20],
  ];
  for (const turnBudget of bad) {
    assert.equal(
      isStrictMetadataContent(
        metadata({ sessionRef: SESSION_REF, turnBudget }),
      ),
      false,
      `accepted a malformed turnBudget: ${JSON.stringify(turnBudget)}`,
    );
  }
});

test("an unknown key is still a hard rejection", () => {
  assert.equal(isStrictMetadataContent(metadata({ nope: 1 })), false);
  assert.equal(
    isStrictMetadataContent(metadata({ observedCommit: "a".repeat(40) })),
    false,
  );
});

// ── Finding 34: a newer host's capabilities map ─────────────────────────────
//
// `promptImage` entered 44223's `capabilities` in 15bbe6158 (2026-09-01,
// prompt-image attachments). `capabilities` is an open map of booleans, not a
// closed set: a boolean-valued key this build has never heard of is a newer
// host describing a capability, not a malformed event. A non-boolean value is
// still refused — that is a shape violation no matter the key's name.

test("capabilities carrying promptImage (a real, newer key) decode", () => {
  const withPromptImage = metadata({
    capabilities: { ...BASE.capabilities, promptImage: false },
  });
  assert.equal(
    isStrictMetadataContent(withPromptImage),
    true,
    `a host that publishes promptImage was rejected: ${withPromptImage}`,
  );
});

test("capabilities carrying a never-seen boolean key still decode", () => {
  const withFutureKey = metadata({
    capabilities: { ...BASE.capabilities, futureThing: true },
  });
  assert.equal(
    isStrictMetadataContent(withFutureKey),
    true,
    `an unknown boolean capability was rejected: ${withFutureKey}`,
  );
});

test("a non-boolean value on any capability key, known or not, is refused", () => {
  assert.equal(
    isStrictMetadataContent(
      metadata({ capabilities: { ...BASE.capabilities, promptImage: "no" } }),
    ),
    false,
  );
  assert.equal(
    isStrictMetadataContent(
      metadata({ capabilities: { ...BASE.capabilities, futureThing: 1 } }),
    ),
    false,
  );
  assert.equal(
    isStrictMetadataContent(
      metadata({ capabilities: { ...BASE.capabilities, plan: "yes" } }),
    ),
    false,
  );
});

test("a seated create (actor + role) is a strict lifecycle command", () => {
  const create = (extra = {}) => ({
    schema: "buzz-coding-session-lifecycle-command/v1",
    commandId: "csl-1",
    action: {
      type: "session.create",
      projectRef: null,
      repoRef: null,
      sessionRef: SESSION_REF,
      genesisRef: "b".repeat(64),
      providerInstanceRef: "claude-primary",
      providerAuthorityPubkey: "a".repeat(64),
      model: null,
      title: "roletest",
      initialTurn: null,
      ...extra,
    },
  });
  const strict = (content) =>
    hasStrictLifecycleCommandJson(JSON.stringify(content), content) &&
    hasStrictLifecycleCommandValues(content);
  assert.equal(strict(create()), true);
  assert.equal(strict(create({ actor: ACTOR, role: "lead" })), true);
  assert.equal(strict(create({ role: "lead" })), false);
  assert.equal(strict(create({ actor: ACTOR })), false);
  assert.equal(strict(create({ actor: ACTOR, role: "Lead" })), false);
  assert.equal(strict(create({ actor: "C".repeat(64), role: "lead" })), false);
});

// ── Where per-turn usage lives, and where it does not ────────────────────────
//
// The provider reports token usage on the 44225 transcript `result` item, not
// on a 44224 receipt — no receipt status ends a turn (`turn_started` is the
// last stage a turn command produces; the turn's end is the `result` item).
// These two pin that boundary so a later change cannot quietly move usage onto
// the receipt: this gate is byte-exact on the receipt's key set and would drop
// every receipt on the floor.

test("the receipt gate still accepts the exact five-key lifecycle shape", () => {
  const receipt = {
    schema: "buzz-coding-session-lifecycle-receipt/v1",
    commandId: "cmd-1",
    status: "created",
    session: {
      driver: "claude-agent-acp",
      instanceId: "0123456789abcdef",
      sessionId: "11111111-2222-3333-4444-555555555555",
      generation: 1,
    },
    error: null,
  };
  const source = JSON.stringify(receipt);
  assert.equal(hasStrictLifecycleReceiptJson(source, receipt), true);
});

// ── A `failed` create names no session, and `stopped` was never wired ───────
//
// `LifecycleReceipt::failed` (coding_session_payload.rs:338) always ships
// `session: null` — a create that never reached a session names none — and
// `LifecycleReceipt::stopped` (:384) is a plain terminal receipt like
// `created`/`resumed`. Neither status is new; both were always in
// `ReceiptStatus`. A strict reader that never learned either silently drops
// every failed create and every explicit stop from session coordination.

test("a failed create (null session, an error) is a strict lifecycle receipt", () => {
  const receipt = {
    schema: "buzz-coding-session-lifecycle-receipt/v1",
    commandId: "cmd-1",
    status: "failed",
    session: null,
    error: { code: "PROVIDER_UNAVAILABLE", message: "no capacity" },
  };
  const source = JSON.stringify(receipt);
  assert.equal(
    hasStrictLifecycleReceiptJson(source, receipt) &&
      hasStrictLifecycleReceiptValues(receipt),
    true,
    `a real 'failed' shape (coding_session_payload.rs:338) was rejected`,
  );
});

test("a stopped execution (a session, no error) is a strict lifecycle receipt", () => {
  const receipt = {
    schema: "buzz-coding-session-lifecycle-receipt/v1",
    commandId: "cmd-1",
    status: "stopped",
    session: {
      driver: "claude-agent-acp",
      instanceId: "0123456789abcdef",
      sessionId: "11111111-2222-3333-4444-555555555555",
      generation: 1,
    },
    error: null,
  };
  const source = JSON.stringify(receipt);
  assert.equal(
    hasStrictLifecycleReceiptJson(source, receipt) &&
      hasStrictLifecycleReceiptValues(receipt),
    true,
    `a real 'stopped' shape (coding_session_payload.rs:384) was rejected`,
  );
});

test("a receipt that tries to carry a usage block is refused", () => {
  const receipt = {
    schema: "buzz-coding-session-lifecycle-receipt/v1",
    commandId: "cmd-1",
    status: "created",
    session: {
      driver: "claude-agent-acp",
      instanceId: "0123456789abcdef",
      sessionId: "11111111-2222-3333-4444-555555555555",
      generation: 1,
    },
    error: null,
    usage: { inputTokens: 1200, contextWindow: 1000000 },
  };
  const source = JSON.stringify(receipt);
  assert.equal(
    hasStrictLifecycleReceiptJson(source, receipt),
    false,
    "usage belongs on the transcript result item, not on a receipt",
  );
});

// ── The 2026-08-30 routing amendment ─────────────────────────────────────────

const ROUTING = {
  class: "builder",
  tier: "standard",
  risk: { impact: 3, uncertainty: 3, irreversibility: 2, score: 18 },
  chosen: { provider: "claude-primary", model: "sonnet", effort: "medium" },
  runnerUp: {
    provider: "codex-primary",
    model: "gpt-5.6-luna[medium]",
    effort: "medium",
  },
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

test("the sixty-four metadata shapes buzz-core accepts all decode", () => {
  const amendments = [
    { sessionRef: SESSION_REF },
    { agentRef: ACTOR, role: "builder" },
    { turnBudget: { used: 3, limit: 20 } },
    FACTS,
    { routing: ROUTING },
    { beeStamp: BEE_STAMP },
  ];
  let accepted = 0;
  for (let mask = 0; mask < 64; mask += 1) {
    const content = {};
    for (const [index, amendment] of amendments.entries()) {
      if (mask & (1 << index)) Object.assign(content, amendment);
    }
    if (content.turnBudget) content.sessionRef = SESSION_REF;
    if (isStrictMetadataContent(metadata(content))) accepted += 1;
  }
  assert.equal(accepted, 64);
});

// ── The sixth amendment: `beeStamp` ──────────────────────────────────────────
//
// `SessionMetadata.bee_stamp` (coding_session_payload.rs:989) is documented
// in Rust's own words as "the sixth independent additive key" — the shape
// comment right above it says the accepted shape count is sixty-four, not
// thirty-two. This checker's own `metadataFieldForms` doc said "five... amendments
// ... thirty-two" and never grew a sixth bit, so every 44223 carrying a real
// `beeStamp` (docs/SESSION_STATE.md item 103) was rejected outright.

test("a real beeStamp decodes", () => {
  const source = metadata({ beeStamp: BEE_STAMP });
  assert.equal(
    isStrictMetadataContent(source),
    true,
    `a real beeStamp shape (coding_session_payload.rs:989) was rejected: ${source}`,
  );
});

test("an explicit null beeStamp is refused, naming the key", () => {
  // Unlike `role`/`turnBudget`, an absent stamp is never written as an
  // explicit null (`decode_coding_session_metadata`: "beeStamp must not be
  // null").
  assert.equal(isStrictMetadataContent(metadata({ beeStamp: null })), false);
});

test("an unparsed beeStamp (host looked, could not parse) still decodes", () => {
  const source = metadata({
    beeStamp: { ...BEE_STAMP, version: null, sha: null, dirty: null },
  });
  assert.equal(isStrictMetadataContent(source), true);
});

test("a malformed beeStamp is refused", () => {
  const bad = [
    { ...BEE_STAMP, path: "" },
    { ...BEE_STAMP, source: "downloaded" },
    { ...BEE_STAMP, sha: "not-hex" },
    { ...BEE_STAMP, sha: "abc" },
    { ...BEE_STAMP, dirty: "false" },
    { ...BEE_STAMP, extra: true },
  ];
  for (const beeStamp of bad) {
    assert.equal(
      isStrictMetadataContent(metadata({ beeStamp })),
      false,
      `accepted a malformed beeStamp: ${JSON.stringify(beeStamp)}`,
    );
  }
});

test("a routing record that disagrees with its own risk is refused", () => {
  const wrongScore = {
    ...ROUTING,
    risk: { ...ROUTING.risk, score: 19 },
  };
  assert.equal(
    isStrictMetadataContent(metadata({ routing: wrongScore })),
    false,
  );
});

test("the wire never carries a routed effort the router may not buy", () => {
  for (const effort of ["xhigh", "max", "ultra", ""]) {
    assert.equal(
      isStrictMetadataContent(
        metadata({
          routing: { ...ROUTING, chosen: { ...ROUTING.chosen, effort } },
        }),
      ),
      false,
      `accepted effort ${effort}`,
    );
  }
});

test("reviewRequired must agree with the triggers it lists", () => {
  assert.equal(
    isStrictMetadataContent(
      metadata({ routing: { ...ROUTING, reviewReasons: ["risk 80 >= 40"] } }),
    ),
    false,
    "reviewRequired false while a trigger is listed is a contradiction",
  );
  assert.equal(
    isStrictMetadataContent(
      metadata({
        routing: {
          ...ROUTING,
          reviewRequired: true,
          // Exactly what the canonical router emits, value and all
          // (crates/buzz-core/src/coding_session_routing.rs:1555-1564).
          reviewReasons: ["risk 80 >= 40", "irreversibility 4 >= 4"],
        },
      }),
    ),
    true,
  );
  assert.equal(
    isStrictMetadataContent(
      metadata({
        routing: {
          ...ROUTING,
          reviewRequired: true,
          reviewReasons: ["securityBoundary", "leadRequests"],
        },
      }),
    ),
    true,
    "the six flag triggers keep the names buzz-core gives them",
  );
});

test("a review reason is a bounded token, not a member of a closed set", () => {
  // The vocabulary is open because two of the §6 triggers carry the number
  // that fired them. What the observer still refuses is a reason that is not
  // a reason: blank, oversized, or a list longer than buzz-core will sign.
  const reasons = (list) =>
    isStrictMetadataContent(
      metadata({
        routing: { ...ROUTING, reviewRequired: true, reviewReasons: list },
      }),
    );
  assert.equal(reasons(["risk 80 >= 40"]), true);
  assert.equal(reasons(["   "]), false, "blank");
  assert.equal(reasons(["x".repeat(257)]), false, "over 256 bytes");
  assert.equal(reasons([42]), false, "not a string");
  assert.equal(reasons(Array(16).fill("leadRequests")), true, "16 is the cap");
  assert.equal(reasons(Array(17).fill("leadRequests")), false, "17 is not");
});

test("a routed create is a strict lifecycle command, unrouted or not", () => {
  const create = (extra = {}) => ({
    schema: "buzz-coding-session-lifecycle-command/v1",
    commandId: "csl-1",
    action: {
      type: "session.create",
      projectRef: null,
      repoRef: null,
      sessionRef: SESSION_REF,
      genesisRef: "b".repeat(64),
      providerInstanceRef: "claude-primary",
      providerAuthorityPubkey: "a".repeat(64),
      model: null,
      title: "roletest",
      initialTurn: null,
      ...extra,
    },
  });
  const strict = (content) =>
    hasStrictLifecycleCommandJson(JSON.stringify(content), content) &&
    hasStrictLifecycleCommandValues(content);
  assert.equal(strict(create({ routing: ROUTING })), true);
  assert.equal(
    strict(create({ actor: ACTOR, role: "builder", routing: ROUTING })),
    true,
  );
  assert.equal(strict(create({ routing: { class: "builder" } })), false);
  assert.equal(strict(create({ routing: null })), false);
  assert.equal(
    strict(create({ routing: { ...ROUTING, profile: { verification: 4.7 } } })),
    true,
  );
  // `routing.profile` keys are `bounded_token`, not a closed trait vocabulary
  // (`validate_profile`, coding_session_routing.rs:1233-1250) — a lead may ask
  // for a minimum on any trait name it likes, including one this build has
  // never named. Only the shape (bounded, 1..=5) is enforced.
  assert.equal(
    strict(create({ routing: { ...ROUTING, profile: { nonsense: 4 } } })),
    true,
  );
  assert.equal(
    strict(create({ routing: { ...ROUTING, profile: { nonsense: 6 } } })),
    false,
    "the 1..=5 bound still applies to an unnamed trait",
  );
});

// ── `tier` and `override.effort` are bounded tokens, not closed sets ────────
//
// `RoutingRecord.tier: String` is `bounded_token`-checked (coding_session_
// routing.rs:1316), derived from the registry's own `tiers: BTreeMap<String,
// TierPolicy>` — a registry config, not a Rust enum. `RoutingOverride.effort`
// is bounded the same way (:1279) *by design*: `xhigh`/`max`/`ultra` are
// exactly the values a human override is for, per the router's own comment
// (:118-121). Only `RoutingTarget.effort` — the router's own purchase,
// `chosen`/`runnerUp` — stays the closed `low`/`medium`/`high` set
// (`validate_routing_target`, :1252-1262): that one *is* the protocol
// boundary the relay enforces (the autonomous ceiling), which is why the
// existing "never carries a routed effort the router may not buy" test above
// is unaffected by this change.

test("routing.tier is a bounded token, not the three known names only", () => {
  assert.equal(
    isStrictMetadataContent(
      metadata({ routing: { ...ROUTING, tier: "urgent" } }),
    ),
    true,
    "a registry-defined tier this build has never named was rejected",
  );
  assert.equal(
    isStrictMetadataContent(metadata({ routing: { ...ROUTING, tier: "" } })),
    false,
    "blank is still refused",
  );
});

test("routing.override.effort may be a human-only value like xhigh", () => {
  const withOverride = (effort) =>
    isStrictMetadataContent(
      metadata({
        routing: {
          ...ROUTING,
          override: { model: "claude-opus-5", effort, because: "asked" },
        },
      }),
    );
  assert.equal(withOverride("xhigh"), true);
  assert.equal(withOverride("max"), true);
  assert.equal(withOverride("ultra"), true);
  assert.equal(withOverride(""), false, "blank is still refused");
});
