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
  buildCodingSessionCreateObservationHistoryFilters,
  classifyCodingSessionGenesisEvent,
  classifyCodingSessionCreateEvent,
  CodingSessionCreateObservationStore,
} from "./codingSessionCreateObservations.ts";
import { buildCodingSessionGenesisEvent } from "./codingSessionGenesis.ts";
import { buildCodingSessionCreateEvent } from "./codingSessionLifecycleCommand.ts";
import {
  CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
  CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION,
  codingSessionReceiptSemanticKey,
  lifecycleReceiptSemanticKey,
} from "./codingSessionTrustedIngress.ts";
import { groupCodingSessionCatalog } from "./codingSessionUmbrellaModel.ts";
import {
  OPEN_CODING_SESSION_INGRESS_AUTHORITY,
  resolveCodingSessionIngressAuthority,
} from "./codingSessionIngressAuthority.ts";
import { KIND_CODING_SESSION_LIFECYCLE_RECEIPT } from "@/shared/constants/kinds.ts";

const CHANNEL_ID = "channel-1";
const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";

const PROVIDER_SECRET = generateSecretKey();
const PROVIDER_PUBKEY = getPublicKey(PROVIDER_SECRET);
const ROGUE_PROVIDER_SECRET = generateSecretKey();
const ROGUE_PROVIDER_PUBKEY = getPublicKey(ROGUE_PROVIDER_SECRET);
const FOUNDER_SECRET = generateSecretKey();
const FOUNDER_PUBKEY = getPublicKey(FOUNDER_SECRET);
const TEAMMATE_SECRET = generateSecretKey();
const TEAMMATE_PUBKEY = getPublicKey(TEAMMATE_SECRET);

/**
 * What the hook now hands the store: channel membership, exactly as the
 * display surfaces read. Which provider may answer a create is not a property
 * of this machine's run-permission list — it is written into the create — so
 * the fence lives in the join, and the store is deliberately given the widest
 * read authority to prove the join is doing the work.
 */
const AUTHORITY = OPEN_CODING_SESSION_INGRESS_AUTHORITY;

/** A viewer who runs no providers at all: the ordinary non-founder member. */
const EMPTY_LOCAL_ALLOWLIST = resolveCodingSessionIngressAuthority([]);

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
  genesisRef,
  providerAuthorityPubkey = PROVIDER_PUBKEY,
} = {}) {
  const built = buildCodingSessionCreateEvent({
    channelId,
    commandId,
    projectRef: null,
    repoRef: null,
    ...(omitSessionRef ? {} : { sessionRef }),
    ...(genesisRef ? { genesisRef } : {}),
    providerInstanceRef: "claude-primary",
    providerAuthorityPubkey,
    model: null,
    title: "Advance Beekeeper live sessions",
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

function genesisEvent({
  secret = FOUNDER_SECRET,
  sessionRef = SESSION_REF,
  channelId = CHANNEL_ID,
  createdAt = 1_799_999_999,
  overrideContent = null,
  overrideTags = null,
} = {}) {
  const built = buildCodingSessionGenesisEvent({ channelId, sessionRef });
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

/**
 * A per-stage turn receipt for the same command id a create used. Nothing on
 * the wire stops one, and it names a real target, so the fold has to refuse it
 * on its status rather than on its shape.
 */
function turnReceiptEvent({
  secret = PROVIDER_SECRET,
  commandId = "csl-1",
  status = "turn_refused",
  target = CLAUDE_TARGET,
  createdAt = 1_800_000_006,
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
        ["csl-key", codingSessionReceiptSemanticKey(commandId, status)],
      ],
      content: JSON.stringify({
        schema: CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
        commandId,
        status,
        session: target,
        error:
          status === "turn_refused"
            ? { code: "STALE_GENERATION", message: "that generation moved on" }
            : null,
      }),
    },
    secret,
  );
}

function ingest(events, authority = AUTHORITY) {
  const store = new CodingSessionCreateObservationStore();
  store.ingestRelayEvents(events, [CHANNEL_ID], authority);
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
    title: "Advance Beekeeper live sessions",
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
  assert.deepEqual(filter.kinds, [44221, 44224, 44226]);
  assert.deepEqual(filter["#h"], [CHANNEL_ID]);
  assert.equal(filter.limit, 500);
  // Any member may found a session, so there is no allowlist to scope by —
  // and a p-gated relay still requires the explicit kinds above.
  assert.equal("authors" in filter, false);
});

test("the backfill gives every kind its own budget, so turn receipts cannot evict creates", () => {
  const filters = buildCodingSessionCreateObservationHistoryFilters(
    [CHANNEL_ID],
    1000,
  );
  // One filter per kind. A relay returns its newest `limit` rows across every
  // kind a filter names, and a turn now publishes two or more 44224 receipts,
  // so a shared budget is eventually all receipts and no creates at all.
  assert.deepEqual(
    filters.map((filter) => filter.kinds),
    [[44221], [44224], [44226]],
  );
  for (const filter of filters) {
    assert.equal(filter.limit, 1000);
    assert.deepEqual(filter["#h"], [CHANNEL_ID]);
    assert.equal("authors" in filter, false);
  }
  // Every kind the subscription reads must also be backfilled, or a fact that
  // arrived before this store existed is never seen at all.
  assert.deepEqual(
    filters.flatMap((filter) => filter.kinds),
    buildCodingSessionCreateObservationFilter([CHANNEL_ID], 0).kinds,
  );
});

test("founder resolves only through the create's exact genesis event id", () => {
  const genesis = genesisEvent();
  const create = createEvent({ genesisRef: genesis.id });
  const store = ingest([create, receiptEvent(), genesis]);
  const [observation] = store.snapshot([CHANNEL_ID]);
  assert.equal(observation.genesisRef, genesis.id);
  assert.equal(observation.genesisFounderPubkey, FOUNDER_PUBKEY);
  const [umbrella] = groupCodingSessionCatalog([record()], [observation]);
  assert.equal(umbrella.genesisRef, genesis.id);
  assert.equal(umbrella.founderPubkey, FOUNDER_PUBKEY);

  // A different valid genesis with the same tag is not a candidate: the
  // create's event-id link remains the sole resolution path.
  const unrelated = genesisEvent({ secret: TEAMMATE_SECRET });
  const unresolved = ingest([create, receiptEvent(), unrelated]).snapshot([
    CHANNEL_ID,
  ])[0];
  assert.equal(unresolved.genesisRef, genesis.id);
  assert.equal(unresolved.genesisFounderPubkey, null);
  assert.equal(
    groupCodingSessionCatalog([record()], [unresolved])[0].founderPubkey,
    null,
  );
});

test("genesis classification rejects a session tag/content mismatch", () => {
  const event = genesisEvent({
    overrideTags: [
      ["h", CHANNEL_ID],
      ["csg-v", "csg1-1"],
      ["csg-session", "11111111-2222-3333-4444-555555555555"],
    ],
  });
  assert.equal(
    classifyCodingSessionGenesisEvent(event, new Set([CHANNEL_ID])).kind,
    "malformed",
  );
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

test("only receipts from the provider the create itself named can join", () => {
  // The self-fence, and the reason it is stronger than an allowlist: the
  // rogue's receipt is refused because *this create* did not address it, not
  // because this machine happens not to run it.
  const store = ingest([
    createEvent(),
    receiptEvent({ secret: ROGUE_PROVIDER_SECRET }),
  ]);
  assert.deepEqual(store.snapshot([CHANNEL_ID]), []);

  // Same rogue signer, now the provider the create actually names: it joins.
  const addressed = ingest([
    createEvent({ providerAuthorityPubkey: ROGUE_PROVIDER_PUBKEY }),
    receiptEvent({ secret: ROGUE_PROVIDER_SECRET }),
  ]);
  assert.equal(addressed.snapshot([CHANNEL_ID]).length, 1);
});

test("a member who runs no providers still resolves the join", () => {
  // The foreign-member case. `allowed-bridge-pubkeys` is empty on this
  // machine — it governs what may run here, and this person founded nothing —
  // so consulting it used to erase the founder of every session they joined:
  // no join, `founderPubkey: null`, and a session the composer then called
  // "ungoverned" while the provider refused every control it had enabled.
  assert.equal(EMPTY_LOCAL_ALLOWLIST.state, "invalid");
  const genesis = genesisEvent();
  const create = createEvent({ genesisRef: genesis.id });
  const [observation] = ingest([create, receiptEvent(), genesis]).snapshot([
    CHANNEL_ID,
  ]);
  assert.equal(observation.signerPubkey, FOUNDER_PUBKEY);
  assert.equal(observation.genesisRef, genesis.id);
  assert.equal(observation.genesisFounderPubkey, FOUNDER_PUBKEY);
});

test("a create naming no readable provider is malformed, never joinable", () => {
  const allowed = new Set([CHANNEL_ID]);
  const withPin = JSON.parse(createEvent().content);
  const pinless = createEvent({
    overrideContent: JSON.stringify({
      ...withPin,
      action: (() => {
        const { providerAuthorityPubkey: _dropped, ...rest } = withPin.action;
        return rest;
      })(),
    }),
  });
  assert.equal(
    classifyCodingSessionCreateEvent(pinless, allowed).kind,
    "malformed",
  );
  const nonHexPin = createEvent({
    overrideContent: JSON.stringify({
      ...withPin,
      action: { ...withPin.action, providerAuthorityPubkey: "not-a-pubkey" },
    }),
  });
  assert.equal(
    classifyCodingSessionCreateEvent(nonHexPin, allowed).kind,
    "malformed",
  );
  // ...and neither one binds anything, however trusted the receipt's signer.
  assert.deepEqual(
    ingest([pinless, receiptEvent()]).snapshot([CHANNEL_ID]),
    [],
  );
  assert.deepEqual(
    ingest([nonHexPin, receiptEvent()]).snapshot([CHANNEL_ID]),
    [],
  );
});

test("creates disagreeing about which provider they addressed are disputed", () => {
  // Two signed creates for one commandId naming different providers is a
  // disagreement about who may answer. Resolving it first-wins would let the
  // earlier create silently choose the joining provider for the later one.
  const store = ingest([
    createEvent(),
    createEvent({
      createdAt: 1_800_000_001,
      providerAuthorityPubkey: ROGUE_PROVIDER_PUBKEY,
    }),
    receiptEvent(),
    receiptEvent({ secret: ROGUE_PROVIDER_SECRET, createdAt: 1_800_000_006 }),
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
  const validContent = JSON.parse(createEvent().content);
  const smuggledAction = createEvent({
    overrideContent: JSON.stringify({
      ...validContent,
      action: { ...validContent.action, founderPubkey: TEAMMATE_PUBKEY },
    }),
  });
  assert.equal(
    classifyCodingSessionCreateEvent(smuggledAction, allowed).kind,
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

test("a turn receipt never joins a create to an execution", () => {
  // A turn is an event inside a generation, never a change to one. If a
  // `turn_queued` could join, a create the provider never answered would
  // still bind an execution the moment somebody sent a turn.
  assert.deepEqual(
    ingest([
      createEvent(),
      turnReceiptEvent({ status: "turn_queued" }),
    ]).snapshot([CHANNEL_ID]),
    [],
  );
  assert.deepEqual(
    ingest([
      createEvent(),
      turnReceiptEvent({ status: "turn_refused" }),
    ]).snapshot([CHANNEL_ID]),
    [],
  );
});

test("a turn receipt does not dispute the create's real answer", () => {
  // Both receipts name the same command and the same target; only one of them
  // is an answer to the create. Reading both would make the join ambiguous
  // and silently drop a session that was created perfectly well.
  const observations = ingest([
    createEvent(),
    receiptEvent(),
    turnReceiptEvent({ status: "turn_refused" }),
  ]).snapshot([CHANNEL_ID]);
  assert.equal(observations.length, 1);
  assert.deepEqual(observations[0].target, CLAUDE_TARGET);
});

test("a seated create (actor + role) is a create, and joins like any other", () => {
  const allowed = new Set([CHANNEL_ID]);
  const seat = (extra) => {
    const content = JSON.parse(createEvent().content);
    return createEvent({
      overrideContent: JSON.stringify({
        ...content,
        action: { ...content.action, ...extra },
      }),
    });
  };
  const seated = seat({ actor: "d".repeat(64), role: "lead" });
  assert.equal(
    classifyCodingSessionCreateEvent(seated, allowed).kind,
    "create",
  );
  const store = ingest([seated, receiptEvent()]);
  assert.equal(store.snapshot([CHANNEL_ID]).length, 1);
  assert.equal(store.counts().malformedCount, 0);
  // Half a seat, an uppercase actor, or a bad slug is malformed — the same
  // rule buzz-core enforces on ingest.
  for (const bad of [
    { role: "lead" },
    { actor: "d".repeat(64) },
    { actor: "D".repeat(64), role: "lead" },
    { actor: "d".repeat(64), role: "Lead" },
    { actor: "d".repeat(64), role: "" },
  ]) {
    assert.equal(
      classifyCodingSessionCreateEvent(seat(bad), allowed).kind,
      "malformed",
      JSON.stringify(bad),
    );
  }
});

/**
 * The routing amendment, on the authority stream.
 *
 * A routed create must bind authority exactly as an unrouted one does, and a
 * `routing` that is not the closed record must be malformed rather than
 * ignored — the desktop must not give a smuggled payload a more permissive
 * meaning than the relay that validates it with `deny_unknown_fields`.
 */
const ROUTING_RECORD = {
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

function routedCreateEvent(routing) {
  const event = createEvent();
  const payload = JSON.parse(event.content);
  payload.action.routing = routing;
  return finalizeEvent(
    {
      kind: event.kind,
      created_at: event.created_at,
      tags: event.tags,
      content: JSON.stringify(payload),
    },
    FOUNDER_SECRET,
  );
}

test("a routed create still classifies as a create", () => {
  const classified = classifyCodingSessionCreateEvent(
    routedCreateEvent(ROUTING_RECORD),
    new Set([CHANNEL_ID]),
  );
  assert.equal(classified.kind, "create");
  assert.equal(classified.signerPubkey, FOUNDER_PUBKEY);
});

test("a routing object that is not the closed record is malformed", () => {
  for (const routing of [
    null,
    { class: "builder" },
    {
      ...ROUTING_RECORD,
      chosen: { ...ROUTING_RECORD.chosen, effort: "ultra" },
    },
    { ...ROUTING_RECORD, smuggled: true },
  ]) {
    assert.equal(
      classifyCodingSessionCreateEvent(
        routedCreateEvent(routing),
        new Set([CHANNEL_ID]),
      ).kind,
      "malformed",
      `accepted ${JSON.stringify(routing)}`,
    );
  }
});

test("a create answering a hire classifies as a create; its hireRef binds nothing and must be lowercase hex", () => {
  const allowed = new Set([CHANNEL_ID]);
  const base = JSON.parse(createEvent().content);
  const hired = createEvent({
    overrideContent: JSON.stringify({
      ...base,
      action: { ...base.action, hireRef: "ab".repeat(32) },
    }),
  });
  const classified = classifyCodingSessionCreateEvent(hired, allowed);
  assert.equal(classified.kind, "create");
  assert.equal(classified.providerAuthorityPubkey, PROVIDER_PUBKEY);
  assert.equal(classified.sessionRef, SESSION_REF);
  assert.equal(
    Object.hasOwn(classified, "hireRef"),
    false,
    "attribution never becomes an authority fact on the observation",
  );
  const shouted = createEvent({
    overrideContent: JSON.stringify({
      ...base,
      action: { ...base.action, hireRef: "AB".repeat(32) },
    }),
  });
  assert.equal(
    classifyCodingSessionCreateEvent(shouted, allowed).kind,
    "malformed",
  );
  const notAnId = createEvent({
    overrideContent: JSON.stringify({
      ...base,
      action: { ...base.action, hireRef: 7 },
    }),
  });
  assert.equal(
    classifyCodingSessionCreateEvent(notAnId, allowed).kind,
    "malformed",
  );
});

test("ingestion signals only new relevant evidence, preserving duplicate and unrelated no-ops", () => {
  const store = new CodingSessionCreateObservationStore();
  const event = createEvent();
  const ingest = (events) =>
    store.ingestRelayEvents(events, [CHANNEL_ID], AUTHORITY);
  assert.equal(ingest([]), false);
  assert.equal(ingest([{ ...event, id: "unrelated", kind: 1 }]), false);
  assert.equal(ingest([event]), true);
  const before = store.snapshot([CHANNEL_ID]);
  assert.equal(ingest([event, event]), false);
  assert.deepEqual(store.snapshot([CHANNEL_ID]), before);
  const invalid = {
    ...event,
    id: "new-invalid-evidence",
    sig: "0".repeat(128),
  };
  assert.equal(
    ingest([invalid]),
    true,
    "new validation failures must still notify readers",
  );
  assert.equal(ingest([invalid]), false);
});

// ---------------------------------------------------------------------------
// Founded umbrellas: a genesis nothing has started is still a founding fact.
// ---------------------------------------------------------------------------

test("a genesis with no create is a founded umbrella, and the classification carries its createdAt", () => {
  const genesis = genesisEvent({ createdAt: 1_799_999_000 });
  const classified = classifyCodingSessionGenesisEvent(
    genesis,
    new Set([CHANNEL_ID]),
  );
  assert.equal(classified.kind, "genesis");
  assert.equal(classified.createdAt, 1_799_999_000);

  const store = ingest([genesis]);
  assert.deepEqual(store.snapshot([CHANNEL_ID]), []);
  assert.deepEqual(store.foundedSnapshot([CHANNEL_ID]), [
    {
      channelId: CHANNEL_ID,
      sessionRef: SESSION_REF,
      genesisRef: genesis.id,
      founderPubkey: FOUNDER_PUBKEY,
      foundedAt: 1_799_999_000,
    },
  ]);
  assert.equal(store.counts().disputedGenesisCount, 0);
});

test("a genesis stays in the founded snapshot once its create is joined — subtracting is the projection's job", () => {
  const genesis = genesisEvent();
  const store = ingest([
    genesis,
    createEvent({ genesisRef: genesis.id }),
    receiptEvent(),
  ]);
  assert.equal(store.snapshot([CHANNEL_ID]).length, 1);
  assert.equal(store.foundedSnapshot([CHANNEL_ID]).length, 1);
});

test("two geneses for one ref are disputed: neither is founded, and the count says so", () => {
  const first = genesisEvent({ createdAt: 1_799_999_000 });
  const second = genesisEvent({
    secret: TEAMMATE_SECRET,
    createdAt: 1_799_999_001,
  });
  const store = ingest([first, second]);
  assert.deepEqual(store.foundedSnapshot([CHANNEL_ID]), []);
  assert.equal(store.counts().disputedGenesisCount, 1);

  // A different ref in the same channel is untouched by the dispute.
  const otherRef = "6c8f2d3b-01e5-4c1f-b2a4-8d3e9f7a5b21";
  const other = genesisEvent({ sessionRef: otherRef });
  const mixed = ingest([first, second, other]);
  assert.deepEqual(
    mixed.foundedSnapshot([CHANNEL_ID]).map((entry) => entry.sessionRef),
    [otherRef],
  );
  assert.equal(mixed.counts().disputedGenesisCount, 1);
});

test("founded snapshots are ordered oldest first and scoped to the channels asked for", () => {
  const older = genesisEvent({
    sessionRef: "6c8f2d3b-01e5-4c1f-b2a4-8d3e9f7a5b21",
    createdAt: 1_799_999_000,
  });
  const newer = genesisEvent({ createdAt: 1_799_999_500 });
  const elsewhere = genesisEvent({
    channelId: "channel-2",
    sessionRef: "7d9a3e4c-12f6-4d2a-c3b5-9e4f0a8b6c32",
  });
  const store = new CodingSessionCreateObservationStore();
  store.ingestRelayEvents(
    [newer, elsewhere, older],
    [CHANNEL_ID, "channel-2"],
    AUTHORITY,
  );
  assert.deepEqual(
    store.foundedSnapshot([CHANNEL_ID]).map((entry) => entry.genesisRef),
    [older.id, newer.id],
  );
  assert.deepEqual(
    store.foundedSnapshot(["channel-2"]).map((entry) => entry.channelId),
    ["channel-2"],
  );
  assert.deepEqual(store.foundedSnapshot(["channel-3"]), []);
  // A genesis in a channel the store was never asked to read is irrelevant,
  // not founded: it was never accepted.
  const scoped = ingest([elsewhere]);
  assert.deepEqual(scoped.foundedSnapshot(["channel-2"]), []);
});

test("an unsigned or tampered genesis founds nothing", () => {
  const genesis = genesisEvent();
  const tampered = { ...genesis, content: genesis.content.replace("v", "V") };
  const unsigned = { ...genesis, sig: "0".repeat(128) };
  const store = ingest([tampered, unsigned]);
  assert.deepEqual(store.foundedSnapshot([CHANNEL_ID]), []);
  assert.equal(store.counts().invalidSignatureCount >= 1, true);
});

test("a forgotten session leaves the founded snapshot, and a stale echo of its genesis is not re-admitted", () => {
  const genesis = genesisEvent();
  const store = ingest([genesis]);
  assert.equal(store.foundedSnapshot([CHANNEL_ID]).length, 1);
  assert.equal(
    store.forgetCodingSession({
      channelId: CHANNEL_ID,
      sessionRef: SESSION_REF,
      genesisRef: genesis.id,
    }),
    true,
  );
  assert.deepEqual(store.foundedSnapshot([CHANNEL_ID]), []);
  // The relay's soft-delete is final; a late copy of the same bytes (a
  // post-fence catch-up that raced the delete) changes nothing.
  assert.equal(
    store.ingestRelayEvents([genesis], [CHANNEL_ID], AUTHORITY),
    false,
  );
  assert.deepEqual(store.foundedSnapshot([CHANNEL_ID]), []);
  // Forgetting what was never held is a no-op, said so.
  assert.equal(
    store.forgetCodingSession({
      channelId: CHANNEL_ID,
      sessionRef: "33333333-3333-4333-8333-333333333333",
      genesisRef: null,
    }),
    false,
  );
});

test("forgetting by ref alone (no genesis id known) still drops the session's genesis", () => {
  const genesis = genesisEvent();
  const store = ingest([genesis]);
  assert.equal(
    store.forgetCodingSession({
      channelId: CHANNEL_ID,
      sessionRef: SESSION_REF,
      genesisRef: null,
    }),
    true,
  );
  assert.deepEqual(store.foundedSnapshot([CHANNEL_ID]), []);
});
