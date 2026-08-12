import assert from "node:assert/strict";
import test from "node:test";
import {
  finalizeEvent,
  generateSecretKey,
  getPublicKey,
} from "nostr-tools/pure";

import {
  KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
  KIND_CODING_SESSION_METADATA,
  KIND_CODING_SESSION_TRANSCRIPT,
} from "@/shared/constants/kinds.ts";
import { buildCodingSessionTargetKey } from "./codingSessionCommand.ts";
import { resolveCodingSessionIngressAuthority } from "./codingSessionIngressAuthority.ts";
import {
  BUZZ_CODING_SESSION_METADATA_SCHEMA,
  BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA,
  classifyTrustedCodingSessionIngressEvent,
  CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
  CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION,
  CODING_SESSION_METADATA_TAG_VERSION,
  CODING_SESSION_TRANSCRIPT_TAG_VERSION,
  codingSessionMetadataSemanticKey,
  codingSessionTranscriptSemanticKey,
  lifecycleReceiptSemanticKey,
  MAX_RETAINED_RAW_EVENTS_PER_GENERATION,
  parseBuzzCodingSessionMetadata,
  parseBuzzCodingSessionTranscript,
  parseCodingSessionLifecycleReceipt,
  TrustedCodingSessionIngressStore,
} from "./codingSessionTrustedIngress.ts";
import {
  buildTrustedCodingSessionIngressFilter,
  buildTrustedCodingSessionIngressHistoryFilters,
  buildTrustedCodingSessionIngressLiveFilter,
} from "./useTrustedCodingSessionIngress.ts";

const CHANNEL_ID = "channel-1";
const PROVIDER_SECRET = generateSecretKey();
const OTHER_SECRET = generateSecretKey();
const PROVIDER_PUBKEY = getPublicKey(PROVIDER_SECRET);
const AUTHORITY = resolveCodingSessionIngressAuthority([
  { pubkey: PROVIDER_PUBKEY, label: "This computer (coding sessions)" },
]);
const TARGET = {
  driver: "claude-agent-acp",
  instanceId: "0123456789abcdef",
  sessionId: "11111111-2222-3333-4444-555555555555",
  generation: 3,
};
const OTHER_TARGET = {
  driver: "claude-agent-acp",
  instanceId: "0123456789abcdef",
  sessionId: "99999999-8888-7777-6666-555555555555",
  generation: 2,
};

function createdReceipt(overrides = {}) {
  return {
    schema: CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
    commandId: "create-1",
    status: "created",
    session: TARGET,
    error: null,
    ...overrides,
  };
}

function metadata(overrides = {}) {
  return {
    schema: BUZZ_CODING_SESSION_METADATA_SCHEMA,
    session: TARGET,
    projectRef: "30621:owner:agiterra",
    repoRef: null,
    title: "Advance Buzz coding sessions",
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
    ...overrides,
  };
}

function receiptEvent(
  value = createdReceipt(),
  {
    secret = PROVIDER_SECRET,
    channelId = CHANNEL_ID,
    tags,
    content,
    createdAt = 1_800_000_000,
    kind = KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
  } = {},
) {
  return finalizeEvent(
    {
      kind,
      created_at: createdAt,
      tags: tags ?? [
        ["h", channelId],
        ["cslr-v", CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION],
        ["csl-command", value.commandId],
        ["csl-key", lifecycleReceiptSemanticKey(value.commandId)],
      ],
      content: content ?? JSON.stringify(value),
    },
    secret,
  );
}

function metadataEvent(
  value = metadata(),
  {
    secret = PROVIDER_SECRET,
    channelId = CHANNEL_ID,
    tags,
    content,
    createdAt = 1_800_000_001,
    kind = KIND_CODING_SESSION_METADATA,
  } = {},
) {
  return finalizeEvent(
    {
      kind,
      created_at: createdAt,
      tags: tags ?? [
        ["h", channelId],
        ["csm-v", CODING_SESSION_METADATA_TAG_VERSION],
        ["cs-target", buildCodingSessionTargetKey(value.session)],
        ["csm-key", codingSessionMetadataSemanticKey(value.session)],
      ],
      content: content ?? JSON.stringify(value),
    },
    secret,
  );
}

function transcript(overrides = {}) {
  return {
    schema: BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA,
    session: OTHER_TARGET,
    eventSeq: 1,
    timestamp: 1_800_000_000_000,
    turnId: "turn-1",
    item: { kind: "assistant_text", text: "Provider-neutral hello" },
    ...overrides,
  };
}

function transcriptEvent(
  value = transcript(),
  {
    secret = PROVIDER_SECRET,
    channelId = CHANNEL_ID,
    tags,
    content,
    createdAt = 1_800_000_004,
    kind = KIND_CODING_SESSION_TRANSCRIPT,
  } = {},
) {
  return finalizeEvent(
    {
      kind,
      created_at: createdAt,
      tags: tags ?? [
        ["h", channelId],
        ["cst-v", CODING_SESSION_TRANSCRIPT_TAG_VERSION],
        ["cs-target", buildCodingSessionTargetKey(value.session)],
        ["cst-seq", String(value.eventSeq)],
        [
          "cst-key",
          codingSessionTranscriptSemanticKey(value.session, value.eventSeq),
        ],
      ],
      content: content ?? JSON.stringify(value),
    },
    secret,
  );
}

test("strict codecs enforce exact receipt invariants and metadata shape", () => {
  assert.deepEqual(
    parseCodingSessionLifecycleReceipt(JSON.stringify(createdReceipt())),
    createdReceipt(),
  );
  const failed = {
    schema: CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
    commandId: "create-2",
    status: "failed",
    session: null,
    error: { code: "PROVIDER_AUTH_REQUIRED", message: "Provider is offline." },
  };
  assert.deepEqual(
    parseCodingSessionLifecycleReceipt(JSON.stringify(failed)),
    failed,
  );
  const createdWithFailedInitialTurn = {
    schema: CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
    commandId: "create-3",
    status: "created_with_failed_initial_turn",
    session: TARGET,
    error: {
      code: "INITIAL_TURN_FAILED",
      message: "The session exists, but its initial turn failed.",
    },
  };
  assert.deepEqual(
    parseCodingSessionLifecycleReceipt(
      JSON.stringify(createdWithFailedInitialTurn),
    ),
    createdWithFailedInitialTurn,
  );

  for (const invalid of [
    { ...createdReceipt(), error: { code: "bad", message: "bad" } },
    { ...failed, session: TARGET },
    { ...createdReceipt(), extra: true },
    {
      ...createdWithFailedInitialTurn,
      error: { code: "PROVIDER_UNAVAILABLE", message: "Wrong error code." },
    },
    { ...createdWithFailedInitialTurn, session: null },
  ]) {
    assert.equal(
      parseCodingSessionLifecycleReceipt(JSON.stringify(invalid)),
      null,
    );
  }

  assert.deepEqual(
    parseBuzzCodingSessionMetadata(JSON.stringify(metadata())),
    metadata(),
  );
  assert.equal(
    parseBuzzCodingSessionMetadata(
      JSON.stringify({
        ...metadata(),
        capabilities: { ...metadata().capabilities, terminal: true },
      }),
    ),
    null,
  );
});

test("a standalone session's null projectRef decodes; a missing key does not", () => {
  assert.equal(
    parseBuzzCodingSessionMetadata(
      JSON.stringify(metadata({ projectRef: null })),
    )?.projectRef,
    null,
  );
  const { projectRef: _dropped, ...withoutKey } = metadata();
  assert.equal(
    parseBuzzCodingSessionMetadata(JSON.stringify(withoutKey)),
    null,
  );
});

test("CST codec accepts only the exact provider-neutral transcript envelope", () => {
  assert.deepEqual(
    parseBuzzCodingSessionTranscript(JSON.stringify(transcript())),
    transcript(),
  );
  for (const invalid of [
    { ...transcript(), eventSeq: 0 },
    { ...transcript(), timestamp: Number.POSITIVE_INFINITY },
    { ...transcript(), turnId: 42 },
    { ...transcript(), item: { text: "missing kind" } },
    { ...transcript(), extra: true },
    { ...transcript(), session: { ...OTHER_TARGET, provider: "claude" } },
  ]) {
    assert.equal(
      parseBuzzCodingSessionTranscript(JSON.stringify(invalid)),
      null,
    );
  }
});

test("the fork's amended item kinds decode through the CST codec", () => {
  for (const item of [
    { kind: "reasoning", text: "chain of thought" },
    {
      kind: "plan",
      entries: [{ content: "step", priority: "high", status: "pending" }],
      text: "- [ ] step",
    },
    {
      kind: "elided",
      reason: "oversize",
      byteCount: 41_235,
      contentDigest: "sha256:abc",
    },
  ]) {
    assert.deepEqual(
      parseBuzzCodingSessionTranscript(JSON.stringify(transcript({ item })))
        ?.item,
      item,
    );
  }
});

test("receipt command ids and metadata identities use the provider's exact UTF-8 bounds", () => {
  const commandAtLimit = "é".repeat(128);
  assert.notEqual(
    parseCodingSessionLifecycleReceipt(
      JSON.stringify(createdReceipt({ commandId: commandAtLimit })),
    ),
    null,
  );
  assert.equal(
    parseCodingSessionLifecycleReceipt(
      JSON.stringify(createdReceipt({ commandId: `${commandAtLimit}a` })),
    ),
    null,
  );

  const identityAtLimit = "é".repeat(256);
  assert.notEqual(
    parseBuzzCodingSessionMetadata(
      JSON.stringify(
        metadata({
          session: { ...TARGET, driver: identityAtLimit },
          projectRef: "é".repeat(1024),
        }),
      ),
    ),
    null,
  );
  assert.equal(
    parseBuzzCodingSessionMetadata(
      JSON.stringify(
        metadata({ session: { ...TARGET, driver: `${identityAtLimit}a` } }),
      ),
    ),
    null,
  );
  assert.equal(
    parseBuzzCodingSessionMetadata(
      JSON.stringify(metadata({ projectRef: `${"é".repeat(1024)}a` })),
    ),
    null,
  );
});

test("one native filter covers history and live, author-governed and channel-scoped", () => {
  const nativeFilter = {
    kinds: [
      KIND_CODING_SESSION_METADATA,
      KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
      KIND_CODING_SESSION_TRANSCRIPT,
    ],
    "#h": ["channel-1", "channel-2"],
    authors: [PROVIDER_PUBKEY],
    limit: 1000,
  };
  assert.deepEqual(
    buildTrustedCodingSessionIngressFilter(
      ["channel-1", "channel-2"],
      AUTHORITY,
      1000,
    ),
    nativeFilter,
  );
  assert.deepEqual(
    buildTrustedCodingSessionIngressHistoryFilters(
      ["channel-1", "channel-2"],
      AUTHORITY,
      1000,
    ),
    [nativeFilter],
    "no second broad filter survives: kind-9 is not a coding-session transport",
  );
  assert.deepEqual(
    buildTrustedCodingSessionIngressLiveFilter(["channel-1"], AUTHORITY),
    {
      kinds: [
        KIND_CODING_SESSION_METADATA,
        KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
        KIND_CODING_SESSION_TRANSCRIPT,
      ],
      "#h": ["channel-1"],
      authors: [PROVIDER_PUBKEY],
      limit: 0,
    },
  );
});

test("an unresolved authority produces a filter with no authors, and admits nothing", () => {
  const invalid = resolveCodingSessionIngressAuthority([]);
  const filter = buildTrustedCodingSessionIngressFilter(
    [CHANNEL_ID],
    invalid,
    10,
  );
  assert.equal("authors" in filter, false);
  assert.equal(
    classifyTrustedCodingSessionIngressEvent(
      receiptEvent(),
      new Set([CHANNEL_ID]),
      invalid,
    ).kind,
    "malformed",
  );
});

test("classifier requires trusted signer, exact channel, exact tags, valid signature, and semantic keys", () => {
  const allowed = new Set([CHANNEL_ID]);
  const classifiedReceipt = classifyTrustedCodingSessionIngressEvent(
    receiptEvent(),
    allowed,
    AUTHORITY,
  );
  assert.equal(classifiedReceipt.kind, "receipt");
  assert.equal(classifiedReceipt.signerPubkey, PROVIDER_PUBKEY);
  assert.equal(
    classifyTrustedCodingSessionIngressEvent(
      metadataEvent(),
      allowed,
      AUTHORITY,
    ).kind,
    "metadata",
  );
  const classifiedTranscript = classifyTrustedCodingSessionIngressEvent(
    transcriptEvent(),
    allowed,
    AUTHORITY,
  );
  assert.equal(classifiedTranscript.kind, "transcript");
  assert.equal(classifiedTranscript.signerPubkey, PROVIDER_PUBKEY);

  assert.equal(
    classifyTrustedCodingSessionIngressEvent(
      receiptEvent(createdReceipt(), { kind: KIND_CODING_SESSION_METADATA }),
      allowed,
      AUTHORITY,
    ).kind,
    "malformed",
    "a receipt tag on the metadata kind is a broken producer, not a stray event",
  );

  assert.equal(
    classifyTrustedCodingSessionIngressEvent(
      receiptEvent(createdReceipt(), { secret: OTHER_SECRET }),
      allowed,
      AUTHORITY,
    ).kind,
    "rejected-author",
  );
  assert.equal(
    classifyTrustedCodingSessionIngressEvent(
      receiptEvent(createdReceipt(), { channelId: "other-channel" }),
      allowed,
      AUTHORITY,
    ).kind,
    "malformed",
  );
  assert.equal(
    classifyTrustedCodingSessionIngressEvent(
      { ...receiptEvent(), sig: "bad" },
      allowed,
      AUTHORITY,
    ).kind,
    "invalid-signature",
  );

  for (const event of [
    receiptEvent(createdReceipt(), {
      tags: [
        ["h", CHANNEL_ID],
        ["cslr-v", CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION],
        ["csl-key", lifecycleReceiptSemanticKey("create-1")],
        ["csl-command", "create-1"],
      ],
    }),
    receiptEvent(createdReceipt(), {
      tags: [
        ["h", CHANNEL_ID],
        ["cslr-v", CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION],
        ["csl-command", "create-1"],
        ["csl-key", "wrong"],
      ],
    }),
    metadataEvent(metadata(), {
      tags: [
        ["h", CHANNEL_ID],
        ["csm-v", CODING_SESSION_METADATA_TAG_VERSION],
        ["cs-target", buildCodingSessionTargetKey(TARGET)],
        ["csm-key", "wrong"],
      ],
    }),
    transcriptEvent(transcript(), {
      tags: [
        ["h", CHANNEL_ID],
        ["cst-v", CODING_SESSION_TRANSCRIPT_TAG_VERSION],
        ["cst-seq", "1"],
        ["cs-target", buildCodingSessionTargetKey(OTHER_TARGET)],
        ["cst-key", codingSessionTranscriptSemanticKey(OTHER_TARGET, 1)],
      ],
    }),
    transcriptEvent(transcript(), {
      tags: [
        ["h", CHANNEL_ID],
        ["cst-v", CODING_SESSION_TRANSCRIPT_TAG_VERSION],
        ["cs-target", buildCodingSessionTargetKey(OTHER_TARGET)],
        ["cst-seq", "2"],
        ["cst-key", codingSessionTranscriptSemanticKey(OTHER_TARGET, 1)],
      ],
    }),
  ]) {
    assert.equal(
      classifyTrustedCodingSessionIngressEvent(event, allowed, AUTHORITY).kind,
      "malformed",
    );
  }
});

test("kind-9 is not a coding-session transport, even wearing the right tags", () => {
  const allowed = new Set([CHANNEL_ID]);
  for (const event of [
    finalizeEvent(
      {
        kind: 9,
        created_at: 1_800_000_000,
        tags: [["h", CHANNEL_ID]],
        content: "hello",
      },
      PROVIDER_SECRET,
    ),
    receiptEvent(createdReceipt(), { kind: 9 }),
    metadataEvent(metadata(), { kind: 9 }),
    transcriptEvent(transcript(), { kind: 9 }),
  ]) {
    assert.equal(
      classifyTrustedCodingSessionIngressEvent(event, allowed, AUTHORITY).kind,
      "irrelevant",
    );
  }

  const store = new TrustedCodingSessionIngressStore();
  store.ingestRelayEvents(
    [receiptEvent(createdReceipt(), { kind: 9 })],
    [CHANNEL_ID],
    AUTHORITY,
  );
  const snapshot = store.snapshot([CHANNEL_ID]);
  assert.equal(snapshot.malformedCount, 0);
  assert.deepEqual(
    store.resolveLifecycle(CHANNEL_ID, "create-1", PROVIDER_PUBKEY),
    { state: "pending", commandId: "create-1" },
  );
});

test("a malformed native event is counted once, no matter how often it arrives", () => {
  const malformed = receiptEvent(createdReceipt(), {
    tags: [
      ["h", CHANNEL_ID],
      ["cslr-v", CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION],
    ],
  });
  const store = new TrustedCodingSessionIngressStore();
  store.ingestRelayEvents([malformed, malformed], [CHANNEL_ID], AUTHORITY);
  const snapshot = store.snapshot([CHANNEL_ID]);
  assert.equal(snapshot.malformedCount, 1);
  assert.equal(snapshot.rejectedAuthorCount, 0);
  assert.equal(snapshot.invalidSignatureCount, 0);
});

test("CST storage is immutable by exact target, signer, and sequence", () => {
  const otherPubkey = getPublicKey(OTHER_SECRET);
  const pluralAuthority = resolveCodingSessionIngressAuthority([
    { pubkey: PROVIDER_PUBKEY, label: "This computer" },
    { pubkey: otherPubkey, label: "Other provider" },
  ]);
  const store = new TrustedCodingSessionIngressStore();
  store.ingestRelayEvents(
    [
      transcriptEvent(),
      transcriptEvent(transcript(), { createdAt: 1_800_000_005 }),
      transcriptEvent(
        transcript({ item: { kind: "assistant_text", text: "Conflict" } }),
        { createdAt: 1_800_000_006 },
      ),
      transcriptEvent(transcript(), {
        secret: OTHER_SECRET,
        createdAt: 1_800_000_007,
      }),
    ],
    [CHANNEL_ID],
    pluralAuthority,
  );

  const snapshot = store.snapshot([CHANNEL_ID]);
  assert.equal(snapshot.transcripts.length, 2);
  assert.equal(
    snapshot.transcripts.find((entry) => entry.signerPubkey === PROVIDER_PUBKEY)
      ?.conflictCount,
    1,
  );
  assert.equal(
    snapshot.transcripts.find((entry) => entry.signerPubkey === otherPubkey)
      ?.conflictCount,
    0,
  );
  assert.equal(snapshot.malformedCount, 0);
});

test("receipt resolves only its exact target metadata and never falls forward", () => {
  const store = new TrustedCodingSessionIngressStore();
  store.ingestRelayEvents(
    [
      receiptEvent(),
      metadataEvent(metadata({ session: { ...TARGET, generation: 4 } }), {
        createdAt: 1_800_000_002,
      }),
    ],
    [CHANNEL_ID],
    AUTHORITY,
  );
  assert.deepEqual(
    store.resolveLifecycle(CHANNEL_ID, "create-1", PROVIDER_PUBKEY),
    { state: "awaiting-metadata", commandId: "create-1", target: TARGET },
  );

  store.ingestRelayEvents([metadataEvent()], [CHANNEL_ID], AUTHORITY);
  const resolved = store.resolveLifecycle(
    CHANNEL_ID,
    "create-1",
    PROVIDER_PUBKEY,
  );
  assert.equal(resolved.state, "created");
  assert.deepEqual(resolved.target, TARGET);
  assert.equal(resolved.metadata.session.generation, 3);
  assert.equal(resolved.metadata.projectRef, "30621:owner:agiterra");
});

test("lifecycle resolution is fenced to the durable transaction provider authority", () => {
  const otherPubkey = getPublicKey(OTHER_SECRET);
  const pluralAuthority = resolveCodingSessionIngressAuthority([
    { pubkey: PROVIDER_PUBKEY, label: "Selected provider" },
    { pubkey: otherPubkey, label: "Other trusted provider" },
  ]);
  const store = new TrustedCodingSessionIngressStore();

  store.ingestRelayEvents(
    [
      receiptEvent(createdReceipt(), {
        secret: OTHER_SECRET,
        createdAt: 1_800_000_000,
      }),
      metadataEvent(metadata(), {
        secret: OTHER_SECRET,
        createdAt: 1_800_000_001,
      }),
    ],
    [CHANNEL_ID],
    pluralAuthority,
  );
  assert.deepEqual(
    store.resolveLifecycle(CHANNEL_ID, "create-1", PROVIDER_PUBKEY),
    { state: "pending", commandId: "create-1" },
  );
  const diagnosticSnapshot = store.snapshot([CHANNEL_ID]);
  assert.equal(diagnosticSnapshot.metadata.length, 1);
  assert.equal(diagnosticSnapshot.metadata[0].signerPubkey, otherPubkey);

  store.ingestRelayEvents(
    [receiptEvent(createdReceipt(), { createdAt: 1_800_000_002 })],
    [CHANNEL_ID],
    pluralAuthority,
  );
  assert.deepEqual(
    store.resolveLifecycle(CHANNEL_ID, "create-1", PROVIDER_PUBKEY),
    { state: "awaiting-metadata", commandId: "create-1", target: TARGET },
  );

  store.ingestRelayEvents(
    [metadataEvent(metadata(), { createdAt: 1_800_000_003 })],
    [CHANNEL_ID],
    pluralAuthority,
  );
  assert.equal(
    store.resolveLifecycle(CHANNEL_ID, "create-1", PROVIDER_PUBKEY).state,
    "created",
  );
  assert.equal(store.snapshot([CHANNEL_ID]).metadata.length, 2);
});

test("lifecycle resolution rejects non-exact provider authority pubkeys", () => {
  const store = new TrustedCodingSessionIngressStore();
  store.ingestRelayEvents(
    [receiptEvent(), metadataEvent()],
    [CHANNEL_ID],
    AUTHORITY,
  );
  for (const invalid of [
    PROVIDER_PUBKEY.toUpperCase(),
    PROVIDER_PUBKEY.slice(2),
  ]) {
    assert.deepEqual(store.resolveLifecycle(CHANNEL_ID, "create-1", invalid), {
      state: "conflict",
      commandId: "create-1",
    });
  }
});

test("failed receipt is terminal and never waits for metadata", () => {
  const store = new TrustedCodingSessionIngressStore();
  const error = {
    code: "PROJECT_CWD_UNRESOLVED",
    message: "No working directory is configured for this project.",
  };
  store.ingestRelayEvents(
    [
      receiptEvent({
        schema: CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
        commandId: "create-1",
        status: "failed",
        session: null,
        error,
      }),
    ],
    [CHANNEL_ID],
    AUTHORITY,
  );
  assert.deepEqual(
    store.resolveLifecycle(CHANNEL_ID, "create-1", PROVIDER_PUBKEY),
    { state: "failed", commandId: "create-1", error },
  );
});

test("a failed initial turn still opens the session it established", () => {
  const receipt = {
    schema: CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
    commandId: "create-1",
    status: "created_with_failed_initial_turn",
    session: TARGET,
    error: {
      code: "INITIAL_TURN_FAILED",
      message: "The session exists; its first turn never reached the agent.",
    },
  };
  const store = new TrustedCodingSessionIngressStore();
  store.ingestRelayEvents([receiptEvent(receipt)], [CHANNEL_ID], AUTHORITY);
  assert.deepEqual(
    store.resolveLifecycle(CHANNEL_ID, "create-1", PROVIDER_PUBKEY),
    {
      state: "awaiting-metadata-after-failed-initial-turn",
      commandId: "create-1",
      target: TARGET,
      error: receipt.error,
    },
  );

  store.ingestRelayEvents([metadataEvent()], [CHANNEL_ID], AUTHORITY);
  assert.deepEqual(
    store.resolveLifecycle(CHANNEL_ID, "create-1", PROVIDER_PUBKEY),
    {
      state: "created-with-failed-initial-turn",
      commandId: "create-1",
      target: TARGET,
      metadata: metadata(),
      error: receipt.error,
    },
  );
});

test("conflicting immutable receipts still fail closed", () => {
  const store = new TrustedCodingSessionIngressStore();
  store.ingestRelayEvents(
    [
      receiptEvent(),
      receiptEvent({
        ...createdReceipt(),
        status: "failed",
        session: null,
        error: { code: "failed", message: "No session." },
      }),
    ],
    [CHANNEL_ID],
    AUTHORITY,
  );
  assert.deepEqual(
    store.resolveLifecycle(CHANNEL_ID, "create-1", PROVIDER_PUBKEY),
    { state: "conflict", commandId: "create-1" },
  );
});

test("same-second metadata resolves deterministically instead of wedging the session", () => {
  // The provider legitimately walks a session through several states inside
  // one second, and `created_at` has no sub-second resolution to order them
  // by. The donor called that a conflict, which left a real session stuck.
  const events = [
    metadataEvent(metadata({ status: "starting" }), {
      createdAt: 1_800_000_010,
    }),
    metadataEvent(metadata({ status: "idle" }), { createdAt: 1_800_000_010 }),
    metadataEvent(metadata(), { createdAt: 1_800_000_010 }),
  ];
  const forward = new TrustedCodingSessionIngressStore();
  forward.ingestRelayEvents(
    [receiptEvent(), ...events],
    [CHANNEL_ID],
    AUTHORITY,
  );
  const reversed = new TrustedCodingSessionIngressStore();
  reversed.ingestRelayEvents(
    [receiptEvent(), ...[...events].reverse()],
    [CHANNEL_ID],
    AUTHORITY,
  );

  const forwardResolution = forward.resolveLifecycle(
    CHANNEL_ID,
    "create-1",
    PROVIDER_PUBKEY,
  );
  const reversedResolution = reversed.resolveLifecycle(
    CHANNEL_ID,
    "create-1",
    PROVIDER_PUBKEY,
  );
  assert.equal(forwardResolution.state, "created");
  assert.deepEqual(forwardResolution.metadata, reversedResolution.metadata);

  // The ambiguity is still reported, it just is not fatal.
  assert.equal(forward.snapshot([CHANNEL_ID]).metadata[0].conflictCount, 2);
});

test("verified raw events are retained per generation for pop-out bootstrap", () => {
  const store = new TrustedCodingSessionIngressStore();
  const receipt = receiptEvent();
  const meta = metadataEvent();
  const item = transcriptEvent();
  store.ingestRelayEvents([receipt, meta, item], [CHANNEL_ID], AUTHORITY);

  const sessionScope = {
    channelId: CHANNEL_ID,
    targetKey: buildCodingSessionTargetKey(TARGET),
    signerPubkey: PROVIDER_PUBKEY,
  };
  assert.deepEqual(
    store.retainedRawEvents(sessionScope).map((event) => event.id),
    [receipt.id, meta.id],
    "the receipt and metadata for this generation, oldest first",
  );
  assert.deepEqual(
    store
      .retainedRawEvents({
        ...sessionScope,
        targetKey: buildCodingSessionTargetKey(OTHER_TARGET),
      })
      .map((event) => event.id),
    [item.id],
  );
  assert.deepEqual(
    store.retainedRawEvents({ ...sessionScope, channelId: "other-channel" }),
    [],
  );
  assert.deepEqual(
    store.retainedRawEvents({ ...sessionScope, signerPubkey: "f".repeat(64) }),
    [],
  );
});

test("raw retention is bounded, dropping the oldest events first", () => {
  assert.equal(MAX_RETAINED_RAW_EVENTS_PER_GENERATION, 2_000);
  const bound = 4;
  const overflow = 3;
  const store = new TrustedCodingSessionIngressStore(bound);
  const events = Array.from({ length: bound + overflow }, (_, index) =>
    transcriptEvent(transcript({ eventSeq: index + 1 }), {
      createdAt: 1_800_000_000 + index,
    }),
  );
  store.ingestRelayEvents(events, [CHANNEL_ID], AUTHORITY);

  const retained = store.retainedRawEvents({
    channelId: CHANNEL_ID,
    targetKey: buildCodingSessionTargetKey(OTHER_TARGET),
    signerPubkey: PROVIDER_PUBKEY,
  });
  assert.equal(retained.length, bound);
  assert.deepEqual(
    retained.map((event) => event.id),
    events.slice(overflow).map((event) => event.id),
  );
});

test("a rejected or unverifiable event is never retained", () => {
  const store = new TrustedCodingSessionIngressStore();
  store.ingestRelayEvents(
    [
      metadataEvent(metadata(), { secret: OTHER_SECRET }),
      {
        ...metadataEvent(metadata(), { createdAt: 1_800_000_009 }),
        sig: "bad",
      },
    ],
    [CHANNEL_ID],
    AUTHORITY,
  );
  assert.deepEqual(
    store.retainedRawEvents({
      channelId: CHANNEL_ID,
      targetKey: buildCodingSessionTargetKey(TARGET),
      signerPubkey: PROVIDER_PUBKEY,
    }),
    [],
  );
  const snapshot = store.snapshot([CHANNEL_ID]);
  assert.equal(snapshot.rejectedAuthorCount, 1);
  assert.equal(snapshot.invalidSignatureCount, 1);
});
