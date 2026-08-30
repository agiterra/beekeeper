/**
 * The hire wire. Everything here is signed for real (nostr-tools) and read
 * back through the classifier the host will run, because the whole point of
 * the exact-key rule is that a payload cannot mean more on this screen than
 * it does at the relay that validates it with `deny_unknown_fields`.
 */
import assert from "node:assert/strict";
import test from "node:test";

import {
  finalizeEvent,
  generateSecretKey,
  getPublicKey,
} from "nostr-tools/pure";

import {
  buildCodingSessionHireEvent,
  classifyCodingSessionHireEvent,
  CODING_SESSION_HIRE_UNSUPPORTED_RELAY_MESSAGE,
  describeCodingSessionHireFailure,
  isCodingSessionHireUnsupportedRelayFailure,
  MAX_CODING_SESSION_HIRE_BRIEF_BYTES,
  publishCodingSessionHire,
  validateCodingSessionHireInput,
} from "./codingSessionHireWire.ts";
import { buildCodingSessionCreateEvent } from "./codingSessionLifecycleCommand.ts";

const CHANNEL_ID = "3d2a7b18-9b7a-4a41-9a86-6a52a1c0b7e1";
const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const GENESIS_REF = "a".repeat(64);
const LEAD_SECRET = generateSecretKey();
const LEAD_PUBKEY = getPublicKey(LEAD_SECRET);
const ALLOWED = new Set([CHANNEL_ID]);

function hireInput(overrides = {}) {
  return {
    channelId: CHANNEL_ID,
    commandId: "csl-hire-1",
    sessionRef: SESSION_REF,
    genesisRef: GENESIS_REF,
    role: "builder",
    providerInstanceRef: null,
    model: null,
    brief: "Fix the badge that points at a message nobody can find.",
    ...overrides,
  };
}

function signHire(overrides = {}) {
  return finalizeEvent(
    {
      created_at: 1_700_000_000,
      ...buildCodingSessionHireEvent(hireInput(overrides)),
    },
    LEAD_SECRET,
  );
}

test("a signed hire round-trips through the classifier", () => {
  const classified = classifyCodingSessionHireEvent(signHire(), ALLOWED);
  assert.equal(classified.kind, "hire");
  assert.equal(classified.channelId, CHANNEL_ID);
  assert.equal(classified.commandId, "csl-hire-1");
  assert.equal(classified.requesterPubkey, LEAD_PUBKEY);
  assert.deepEqual(classified.action, {
    type: "session.hire",
    sessionRef: SESSION_REF,
    genesisRef: GENESIS_REF,
    role: "builder",
    providerInstanceRef: null,
    model: null,
    brief: "Fix the badge that points at a message nobody can find.",
  });
});

test("the action carries exactly the seven contract keys, in order", () => {
  const built = buildCodingSessionHireEvent(hireInput());
  const action = JSON.parse(built.content).action;
  assert.deepEqual(Object.keys(action), [
    "type",
    "sessionRef",
    "genesisRef",
    "role",
    "providerInstanceRef",
    "model",
    "brief",
  ]);
  assert.deepEqual(
    built.tags.map((tag) => tag[0]),
    ["h", "csl-v", "csl-command"],
  );
});

test("an extra action key is malformed, not a permissive hire", () => {
  const built = buildCodingSessionHireEvent(hireInput());
  const payload = JSON.parse(built.content);
  payload.action.workdir = "/Users/brian/Projects/beekeeper/beekeeper";
  const event = finalizeEvent(
    { ...built, content: JSON.stringify(payload), created_at: 1_700_000_000 },
    LEAD_SECRET,
  );
  assert.equal(
    classifyCodingSessionHireEvent(event, ALLOWED).kind,
    "malformed",
  );
});

test("a create on the same kind is irrelevant, never a malformed hire", () => {
  const event = finalizeEvent(
    {
      created_at: 1_700_000_000,
      ...buildCodingSessionCreateEvent({
        channelId: CHANNEL_ID,
        commandId: "csl-create-1",
        projectRef: null,
        repoRef: null,
        sessionRef: SESSION_REF,
        genesisRef: GENESIS_REF,
        providerInstanceRef: "claude-primary",
        providerAuthorityPubkey: "b".repeat(64),
        model: null,
        title: null,
        initialTurn: null,
      }),
    },
    LEAD_SECRET,
  );
  assert.equal(
    classifyCodingSessionHireEvent(event, ALLOWED).kind,
    "irrelevant",
  );
});

test("a hire in a channel this host is not reading is malformed", () => {
  const classified = classifyCodingSessionHireEvent(
    signHire(),
    new Set(["other"]),
  );
  assert.equal(classified.kind, "malformed");
});

test("a tampered hire is invalid-signature, not a hire", () => {
  const event = signHire();
  const payload = JSON.parse(event.content);
  payload.action.role = "lead";
  const tampered = { ...event, content: JSON.stringify(payload) };
  assert.equal(
    classifyCodingSessionHireEvent(tampered, ALLOWED).kind,
    "invalid-signature",
  );
});

test("malformed refs, roles and briefs are all refused before signing", () => {
  for (const overrides of [
    { sessionRef: "not-a-uuid" },
    { genesisRef: "A".repeat(64) },
    { role: "Lead Seat" },
    { role: "x".repeat(65) },
    { brief: "   " },
    { brief: "x".repeat(MAX_CODING_SESSION_HIRE_BRIEF_BYTES + 1) },
    { providerInstanceRef: "" },
    { model: "" },
    { commandId: "" },
    { channelId: "  " },
  ]) {
    assert.throws(
      () => validateCodingSessionHireInput(hireInput(overrides)),
      /./,
      `expected ${JSON.stringify(overrides)} to be refused`,
    );
  }
});

test("a brief at the ceiling is accepted", () => {
  const brief = "x".repeat(MAX_CODING_SESSION_HIRE_BRIEF_BYTES);
  assert.doesNotThrow(() =>
    validateCodingSessionHireInput(hireInput({ brief })),
  );
});

test("the relay's malformed-payload rejection reads as a deployment fact", () => {
  const relayError = new Error(
    "malformed coding-session lifecycle command payload",
  );
  assert.equal(isCodingSessionHireUnsupportedRelayFailure(relayError), true);
  assert.equal(
    describeCodingSessionHireFailure(relayError),
    CODING_SESSION_HIRE_UNSUPPORTED_RELAY_MESSAGE,
  );
  assert.match(
    CODING_SESSION_HIRE_UNSUPPORTED_RELAY_MESSAGE,
    /This relay does not accept hire requests yet/,
  );
});

test("any other failure keeps its own words", () => {
  const offline = new Error("Timed out while sending the hire request.");
  assert.equal(isCodingSessionHireUnsupportedRelayFailure(offline), false);
  assert.equal(
    describeCodingSessionHireFailure(offline),
    "The hire request was not accepted. Timed out while sending the hire request.",
  );
});

test("publishing against a relay without session.hire rejects with the sentence", async () => {
  await assert.rejects(
    publishCodingSessionHire(hireInput(), {
      signer: async (input) =>
        finalizeEvent({ created_at: 1_700_000_000, ...input }, LEAD_SECRET),
      publisher: {
        publishEvent: async () => {
          throw new Error("malformed coding-session lifecycle command payload");
        },
      },
    }),
    (error) => error.message === CODING_SESSION_HIRE_UNSUPPORTED_RELAY_MESSAGE,
  );
});

test("publishing against a relay that accepts it hands back the command id", async () => {
  const published = await publishCodingSessionHire(hireInput(), {
    signer: async (input) =>
      finalizeEvent({ created_at: 1_700_000_000, ...input }, LEAD_SECRET),
    publisher: { publishEvent: async (event) => event },
  });
  assert.equal(published.commandId, "csl-hire-1");
  assert.match(published.eventId, /^[0-9a-f]{64}$/);
});

/**
 * The 2026-08-30 routing amendment.
 *
 * Two accepted key sets, not one growing set: a hire that predates the router
 * is still seven keys and still read, and a routed one carries `routing`
 * last. What a lead may put there is a *request* — a class and a risk — and
 * never a chosen model, because "the lead chooses the capability required;
 * the router chooses the execution target".
 */
const ROUTING_REQUEST = {
  class: "builder",
  risk: { impact: 3, uncertainty: 3, irreversibility: 2 },
};

test("an unrouted hire is byte-identical to the seven-key form", () => {
  const withKey = buildCodingSessionHireEvent(
    hireInput({ routing: undefined }),
  );
  const without = buildCodingSessionHireEvent(hireInput());
  assert.equal(withKey.content, without.content);
  assert.equal(JSON.parse(without.content).action.routing, undefined);
});

test("a routed hire round-trips with its routing request", () => {
  const classified = classifyCodingSessionHireEvent(
    signHire({ routing: ROUTING_REQUEST }),
    ALLOWED,
  );
  assert.equal(classified.kind, "hire");
  assert.deepEqual(classified.action.routing, ROUTING_REQUEST);
  // Trailing, so every earlier key keeps its position.
  assert.deepEqual(
    Object.keys(
      JSON.parse(signHire({ routing: ROUTING_REQUEST }).content).action,
    ),
    [
      "type",
      "sessionRef",
      "genesisRef",
      "role",
      "providerInstanceRef",
      "model",
      "brief",
      "routing",
    ],
  );
});

test("a routing request the relay would refuse is malformed here too", () => {
  for (const routing of [
    { class: "builder" },
    { class: "builder", risk: { impact: 3, uncertainty: 3 } },
    {
      class: "builder",
      risk: { impact: 0, uncertainty: 3, irreversibility: 2 },
    },
    {
      ...ROUTING_REQUEST,
      chosen: { provider: "p", model: "m", effort: "low" },
    },
    { ...ROUTING_REQUEST, override: { model: "sonnet" } },
    { ...ROUTING_REQUEST, tier: "ultra" },
  ]) {
    const event = signHire();
    const payload = JSON.parse(event.content);
    payload.action.routing = routing;
    assert.equal(
      classifyCodingSessionHireEvent(
        { ...event, content: JSON.stringify(payload) },
        ALLOWED,
      ).kind,
      "malformed",
      `accepted ${JSON.stringify(routing)}`,
    );
  }
});

test("the builder refuses a routing request rather than signing it", () => {
  assert.throws(
    () =>
      validateCodingSessionHireInput(
        hireInput({ routing: { class: "builder" } }),
      ),
    /action\.routing/,
  );
});
