/**
 * Create observations: the only thing that makes founder/operator authority
 * real. Everything here is signed for real (nostr-tools) and verified for real
 * — the store's job is to bind a human signer to an execution *only* through a
 * provider-signed receipt, and to bind nothing at all when that is ambiguous.
 */
import assert from "node:assert/strict";
import test from "node:test";

import {
  finalizeEvent,
  generateSecretKey,
  getPublicKey,
} from "nostr-tools/pure";

import {
  buildCodingSessionCreateObservationFilter,
  classifyCodingSessionCreateEvent,
  CodingSessionCreateObservationStore,
} from "./codingSessionCreateObservations.ts";
import { buildCodingSessionCreateEvent } from "./codingSessionLifecycleCommand.ts";
import {
  CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
  CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION,
  lifecycleReceiptSemanticKey,
} from "./codingSessionTrustedIngress.ts";
import { groupCodingSessionCatalog } from "./codingSessionUmbrellaModel.ts";
import { resolveCodingSessionIngressAuthority } from "./codingSessionIngressAuthority.ts";
import { KIND_CODING_SESSION_LIFECYCLE_RECEIPT } from "@/shared/constants/kinds.ts";

const CHANNEL_ID = "channel-1";
const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";

const PROVIDER_SECRET = generateSecretKey();
const PROVIDER_PUBKEY = getPublicKey(PROVIDER_SECRET);
const ROGUE_PROVIDER_SECRET = generateSecretKey();
const FOUNDER_SECRET = generateSecretKey();
const FOUNDER_PUBKEY = getPublicKey(FOUNDER_SECRET);
const TEAMMATE_SECRET = generateSecretKey();
const TEAMMATE_PUBKEY = getPublicKey(TEAMMATE_SECRET);

const AUTHORITY = resolveCodingSessionIngressAuthority([
  { pubkey: PROVIDER_PUBKEY, label: "This computer" },
]);

const CLAUDE_TARGET = {
  driver: "claude-agent-acp",
  instanceId: "claude-instance",
  sessionId: "11111111-1111-1111-1111-111111111111",
  generation: 1,
};
const CODEX_TARGET = {
  driver: "codex-acp",
  instanceId: "codex-instance",
  sessionId: "22222222-2222-2222-2222-222222222222",
  generation: 1,
};

function createEvent({
  secret = FOUNDER_SECRET,
  commandId = "csl-1",
  sessionRef = SESSION_REF,
  omitSessionRef = false,
  createdAt = 1_800_000_000,
  channelId = CHANNEL_ID,
  overrideContent = null,
  overrideTags = null,
} = {}) {
  const built = buildCodingSessionCreateEvent({
    channelId,
    commandId,
    projectRef: null,
    repoRef: null,
    ...(omitSessionRef ? {} : { sessionRef }),
    providerInstanceRef: "claude-primary",
    providerAuthorityPubkey: PROVIDER_PUBKEY,
    model: null,
    title: "Advance Buzz live sessions",
    initialTurn: null,
  });
  return finalizeEvent(
    {
      kind: built.kind,
      created_at: createdAt,
      tags: overrideTags ?? built.tags,
      content: overrideContent ?? built.content,
    },
    secret,
  );
}

function receiptEvent({
  secret = PROVIDER_SECRET,
  commandId = "csl-1",
  target = CLAUDE_TARGET,
  createdAt = 1_800_000_005,
  channelId = CHANNEL_ID,
} = {}) {
  return finalizeEvent(
    {
      kind: KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
      created_at: createdAt,
      tags: [
        ["h", channelId],
        ["cslr-v", CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION],
        ["csl-command", commandId],
        ["csl-key", lifecycleReceiptSemanticKey(commandId)],
      ],
      content: JSON.stringify({
        schema: CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
        commandId,
        status: "created",
        session: target,
        error: null,
      }),
    },
    secret,
  );
}

function ingest(events) {
  const store = new CodingSessionCreateObservationStore();
  store.ingestRelayEvents(events, [CHANNEL_ID], AUTHORITY);
  return store;
}

function record({
  target = CLAUDE_TARGET,
  signerPubkey = PROVIDER_PUBKEY,
  sessionRef = SESSION_REF,
  lastEventAt = "2026-08-12T10:00:00.000Z",
} = {}) {
  return {
    generationId: `gen-${target.driver}-${target.generation}`,
    label: `${target.driver} · generation ${target.generation}`,
    title: "Advance Buzz live sessions",
    providerAuthorityPubkey: signerPubkey,
    metadataAuthorityPubkey: signerPubkey,
    lastEventAt,
    status: "running",
    transcript: [],
    conflictCount: 0,
    commandTarget: target,
    projectRef: null,
    repoRef: null,
    sessionRef,
    provider: "claude-primary",
    runtime: target.driver,
    model: null,
    capabilities: null,
  };
}

test("the subscription reads creates and receipts, unfiltered by author", () => {
  const filter = buildCodingSessionCreateObservationFilter([CHANNEL_ID], 500);
  assert.deepEqual(filter.kinds, [44221, 44224]);
  assert.deepEqual(filter["#h"], [CHANNEL_ID]);
  assert.equal(filter.limit, 500);
  // Any member may found a session, so there is no allowlist to scope by —
  // and a p-gated relay still requires the explicit kinds above.
  assert.equal("authors" in filter, false);
});

test("a signed create joins its execution through the provider's receipt", () => {
  const store = ingest([createEvent(), receiptEvent()]);
  const observations = store.snapshot([CHANNEL_ID]);
  assert.equal(observations.length, 1);
  assert.equal(observations[0].signerPubkey, FOUNDER_PUBKEY);
  assert.equal(observations[0].sessionRef, SESSION_REF);
  assert.deepEqual(observations[0].target, CLAUDE_TARGET);
  assert.equal(observations[0].createdAt, 1_800_000_000);
});

test("a create nobody's provider acted on binds nothing", () => {
  // No receipt: the create minted no execution, so it is no evidence of
  // operating one. This is what stops a backdated create bearing someone
  // else's sessionRef from stealing foundership.
  assert.deepEqual(ingest([createEvent()]).snapshot([CHANNEL_ID]), []);
  // ...and a receipt with no observed create binds nothing either.
  assert.deepEqual(ingest([receiptEvent()]).snapshot([CHANNEL_ID]), []);
});

test("only receipts from the configured provider authority can join", () => {
  const store = ingest([
    createEvent(),
    receiptEvent({ secret: ROGUE_PROVIDER_SECRET }),
  ]);
  assert.deepEqual(store.snapshot([CHANNEL_ID]), []);
});

test("an unsigned or tampered create is refused outright", () => {
  const tampered = { ...createEvent(), sig: "0".repeat(128) };
  const store = ingest([tampered, receiptEvent()]);
  assert.deepEqual(store.snapshot([CHANNEL_ID]), []);
  assert.equal(store.counts().invalidSignatureCount, 1);
});

test("a disputed commandId resolves to no observation rather than a guess", () => {
  const store = ingest([
    createEvent({ secret: FOUNDER_SECRET }),
    createEvent({ secret: TEAMMATE_SECRET, createdAt: 1_800_000_001 }),
    receiptEvent(),
  ]);
  assert.deepEqual(store.snapshot([CHANNEL_ID]), []);

  // Two providers naming different targets for one command is the same kind
  // of disagreement: resolve nothing, side with nobody.
  const disagreeing = new CodingSessionCreateObservationStore();
  disagreeing.ingestRelayEvents(
    [
      createEvent(),
      receiptEvent(),
      receiptEvent({ target: CODEX_TARGET, createdAt: 1_800_000_006 }),
    ],
    [CHANNEL_ID],
    AUTHORITY,
  );
  assert.deepEqual(disagreeing.snapshot([CHANNEL_ID]), []);
});

test("envelope and payload are read exactly; a bad claim is refused", () => {
  const allowed = new Set([CHANNEL_ID]);
  // Wrong channel, extra tag, wrong tag version, mismatched commandId.
  assert.equal(
    classifyCodingSessionCreateEvent(createEvent(), new Set(["other"])).kind,
    "malformed",
  );
  const extraTag = createEvent({
    overrideTags: [
      ["h", CHANNEL_ID],
      ["csl-v", "csl1-1"],
      ["csl-command", "csl-1"],
      ["p", "a".repeat(64)],
    ],
  });
  assert.equal(
    classifyCodingSessionCreateEvent(extraTag, allowed).kind,
    "malformed",
  );
  const mismatchedCommand = createEvent({
    overrideTags: [
      ["h", CHANNEL_ID],
      ["csl-v", "csl1-1"],
      ["csl-command", "csl-other"],
    ],
  });
  assert.equal(
    classifyCodingSessionCreateEvent(mismatchedCommand, allowed).kind,
    "malformed",
  );
  const badSessionRef = createEvent({
    overrideContent: JSON.stringify({
      schema: "buzz-coding-session-lifecycle-command/v1",
      commandId: "csl-1",
      action: {
        type: "session.create",
        projectRef: null,
        repoRef: null,
        sessionRef: "NOT-A-UUID",
        providerInstanceRef: "claude-primary",
        providerAuthorityPubkey: PROVIDER_PUBKEY,
        model: null,
        title: null,
        initialTurn: null,
      },
    }),
  });
  assert.equal(
    classifyCodingSessionCreateEvent(badSessionRef, allowed).kind,
    "malformed",
  );
});

test("a historical create with no sessionRef key observes an implicit umbrella", () => {
  // The 8-key v1 form: absent key means "no umbrella claimed", not truncation
  // this consumer may reject — the operator fact is still exact.
  const historical = createEvent({ omitSessionRef: true });
  assert.equal(JSON.parse(historical.content).action.sessionRef, undefined);
  const [observation] = ingest([historical, receiptEvent()]).snapshot([
    CHANNEL_ID,
  ]);
  assert.equal(observation.sessionRef, null);
  assert.equal(observation.signerPubkey, FOUNDER_PUBKEY);
});

test("collected observations make founder, operator, and foreign attachments real", () => {
  const store = ingest([
    // The founder creates first...
    createEvent({ commandId: "csl-1", createdAt: 1_800_000_000 }),
    receiptEvent({ commandId: "csl-1", target: CLAUDE_TARGET }),
    // ...a teammate attaches their own execution to the same umbrella later.
    createEvent({
      secret: TEAMMATE_SECRET,
      commandId: "csl-2",
      createdAt: 1_800_000_100,
    }),
    receiptEvent({
      commandId: "csl-2",
      target: CODEX_TARGET,
      createdAt: 1_800_000_105,
    }),
  ]);
  const creates = store.snapshot([CHANNEL_ID]);
  assert.equal(creates.length, 2);

  const [umbrella] = groupCodingSessionCatalog(
    [record({ target: CLAUDE_TARGET }), record({ target: CODEX_TARGET })],
    creates,
  );
  assert.equal(umbrella.founderPubkey, FOUNDER_PUBKEY);
  assert.equal(umbrella.foreignAttachmentCount, 1);
  const operators = new Map(
    umbrella.executions.map((execution) => [
      execution.activeGeneration.commandTarget.driver,
      execution.operatorPubkey,
    ]),
  );
  assert.equal(operators.get(CLAUDE_TARGET.driver), FOUNDER_PUBKEY);
  assert.equal(operators.get(CODEX_TARGET.driver), TEAMMATE_PUBKEY);

  // Same catalog, no observations: authority stays unknown and nothing is
  // flagged — the permissive fallback, never a wrong accusation.
  const [ungrouped] = groupCodingSessionCatalog([
    record({ target: CLAUDE_TARGET }),
    record({ target: CODEX_TARGET }),
  ]);
  assert.equal(ungrouped.founderPubkey, null);
  assert.equal(ungrouped.foreignAttachmentCount, 0);
});

test("observations are scoped to the channels the caller asked for", () => {
  const store = new CodingSessionCreateObservationStore();
  store.ingestRelayEvents(
    [
      createEvent({ channelId: "channel-2" }),
      receiptEvent({ channelId: "channel-2" }),
    ],
    ["channel-2"],
    AUTHORITY,
  );
  assert.equal(store.snapshot(["channel-2"]).length, 1);
  assert.deepEqual(store.snapshot([CHANNEL_ID]), []);
});
