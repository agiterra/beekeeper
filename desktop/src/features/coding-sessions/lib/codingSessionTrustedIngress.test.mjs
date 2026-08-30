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
import {
  buildCodingSessionIngressAuthorityIdentity,
  OPEN_CODING_SESSION_INGRESS_AUTHORITY,
  resolveCodingSessionIngressAuthority,
} from "./codingSessionIngressAuthority.ts";
import {
  BUZZ_CODING_SESSION_METADATA_SCHEMA,
  BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA,
  classifyTrustedCodingSessionIngressEvent,
  CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
  CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION,
  CODING_SESSION_METADATA_TAG_VERSION,
  CODING_SESSION_TRANSCRIPT_TAG_VERSION,
  CODING_SESSION_TURN_RECEIPT_STATUSES,
  codingSessionMetadataSemanticKey,
  codingSessionReceiptSemanticKey,
  codingSessionTranscriptSemanticKey,
  establishedCodingSessionTarget,
  isCodingSessionTurnReceipt,
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
    {
      state: "awaiting-metadata",
      commandId: "create-1",
      target: TARGET,
      malformedMetadataCount: 0,
    },
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
    {
      state: "awaiting-metadata",
      commandId: "create-1",
      target: TARGET,
      malformedMetadataCount: 0,
    },
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
      malformedMetadataCount: 0,
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

test("a resume that recovered no context resolves as its own fact, not a plain create", () => {
  const receipt = {
    schema: CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
    commandId: "create-1",
    status: "resumed_without_context",
    session: TARGET,
    error: {
      code: "CONTEXT_NOT_RECOVERED",
      message: "The previous conversation could not be replayed.",
    },
  };
  const store = new TrustedCodingSessionIngressStore();
  store.ingestRelayEvents([receiptEvent(receipt)], [CHANNEL_ID], AUTHORITY);
  // Before metadata the session is not yet openable, so the wait is the
  // ordinary one — the context loss is only reportable alongside a session.
  assert.deepEqual(
    store.resolveLifecycle(CHANNEL_ID, "create-1", PROVIDER_PUBKEY),
    {
      state: "awaiting-metadata",
      commandId: "create-1",
      target: TARGET,
      malformedMetadataCount: 0,
    },
  );

  store.ingestRelayEvents([metadataEvent()], [CHANNEL_ID], AUTHORITY);
  const resolved = store.resolveLifecycle(
    CHANNEL_ID,
    "create-1",
    PROVIDER_PUBKEY,
  );
  assert.deepEqual(resolved, {
    state: "resumed-without-context",
    commandId: "create-1",
    target: TARGET,
    metadata: metadata(),
    error: receipt.error,
  });
  // It established a session all the same: screens open it and must not offer
  // to retry the command that produced it.
  assert.deepEqual(establishedCodingSessionTarget(resolved), TARGET);
});

test("a plain resume keeps behaving exactly like a create", () => {
  const store = new TrustedCodingSessionIngressStore();
  store.ingestRelayEvents(
    [
      receiptEvent({
        schema: CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
        commandId: "create-1",
        status: "resumed",
        session: TARGET,
        error: null,
      }),
      metadataEvent(),
    ],
    [CHANNEL_ID],
    AUTHORITY,
  );
  assert.deepEqual(
    store.resolveLifecycle(CHANNEL_ID, "create-1", PROVIDER_PUBKEY),
    {
      state: "created",
      commandId: "create-1",
      target: TARGET,
      metadata: metadata(),
    },
  );
});

test("only resolutions that established a session name a target", () => {
  assert.equal(establishedCodingSessionTarget(null), null);
  assert.equal(establishedCodingSessionTarget(undefined), null);
  for (const state of ["pending", "awaiting-metadata", "failed", "conflict"]) {
    assert.equal(
      establishedCodingSessionTarget({
        state,
        commandId: "create-1",
        target: TARGET,
      }),
      null,
      state,
    );
  }
  for (const state of [
    "created",
    "created-with-failed-initial-turn",
    "resumed-without-context",
  ]) {
    assert.deepEqual(
      establishedCodingSessionTarget({
        state,
        commandId: "create-1",
        target: TARGET,
      }),
      TARGET,
      state,
    );
  }
});

test("a refused turn is readable, and only from the pinned provider authority", () => {
  const error = {
    code: "UNAUTHORIZED_OPERATOR",
    message:
      "only the session founder or a granted operator may steer this execution",
  };
  const refusal = (commandId, overrides = {}) => ({
    schema: CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
    commandId,
    status: "failed",
    session: null,
    error,
    ...overrides,
  });
  const otherPubkey = getPublicKey(OTHER_SECRET);
  const pluralAuthority = resolveCodingSessionIngressAuthority([
    { pubkey: PROVIDER_PUBKEY, label: "Selected provider" },
    { pubkey: otherPubkey, label: "Other trusted provider" },
  ]);
  const store = new TrustedCodingSessionIngressStore();
  store.ingestRelayEvents(
    [
      receiptEvent(refusal("turn-refused")),
      // A refusal signed by a provider this turn never addressed proves
      // nothing about this turn.
      receiptEvent(refusal("turn-foreign"), { secret: OTHER_SECRET }),
      // A turn that ran publishes no receipt; a create's success receipt is
      // not a refusal wearing a different status.
      receiptEvent(createdReceipt()),
    ],
    [CHANNEL_ID],
    pluralAuthority,
  );

  assert.deepEqual(
    store.resolveTurnRefusal(CHANNEL_ID, "turn-refused", PROVIDER_PUBKEY),
    error,
  );
  assert.equal(
    store.resolveTurnRefusal(CHANNEL_ID, "turn-foreign", PROVIDER_PUBKEY),
    null,
  );
  assert.equal(
    store.resolveTurnRefusal(CHANNEL_ID, "turn-silent", PROVIDER_PUBKEY),
    null,
  );
  assert.equal(
    store.resolveTurnRefusal(CHANNEL_ID, "create-1", PROVIDER_PUBKEY),
    null,
  );
  // The command id is channel-scoped, and a non-exact authority is no
  // authority at all.
  assert.equal(
    store.resolveTurnRefusal("channel-2", "turn-refused", PROVIDER_PUBKEY),
    null,
  );
  for (const invalid of [
    PROVIDER_PUBKEY.toUpperCase(),
    PROVIDER_PUBKEY.slice(2),
  ]) {
    assert.equal(
      store.resolveTurnRefusal(CHANNEL_ID, "turn-refused", invalid),
      null,
      invalid,
    );
  }
});

test("two refusals of one turn disagreeing is a conflict, not a refusal", () => {
  const store = new TrustedCodingSessionIngressStore();
  store.ingestRelayEvents(
    [
      receiptEvent({
        schema: CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
        commandId: "turn-1",
        status: "failed",
        session: null,
        error: { code: "UNAUTHORIZED_OPERATOR", message: "not granted" },
      }),
      receiptEvent(
        {
          schema: CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
          commandId: "turn-1",
          status: "failed",
          session: null,
          error: { code: "STALE_GENERATION", message: "wrong generation" },
        },
        { createdAt: 1_800_000_005 },
      ),
    ],
    [CHANNEL_ID],
    AUTHORITY,
  );
  assert.equal(
    store.resolveTurnRefusal(CHANNEL_ID, "turn-1", PROVIDER_PUBKEY),
    null,
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

test("metadata accepts an optional canonical sessionRef and rejects every other shape", () => {
  const sessionRef = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
  // Absent key: the pre-umbrella form decodes with no sessionRef property.
  const withoutKey = parseBuzzCodingSessionMetadata(JSON.stringify(metadata()));
  assert.equal("sessionRef" in withoutKey, false);
  // Present key: echoed only when the create claimed one.
  assert.equal(
    parseBuzzCodingSessionMetadata(JSON.stringify(metadata({ sessionRef })))
      ?.sessionRef,
    sessionRef,
  );
  // The echo is never an explicit null and never a looser string.
  for (const invalid of [
    null,
    "",
    "not-a-uuid",
    "5B7E1C2A-90D4-4B0E-A1F3-7C2D8E6F4A10",
    "5b7e1c2a90d44b0ea1f37c2d8e6f4a10",
    42,
  ]) {
    assert.equal(
      parseBuzzCodingSessionMetadata(
        JSON.stringify(metadata({ sessionRef: invalid })),
      ),
      null,
    );
  }
});

test("open authority admits any verified author; signatures and scoping still gate", () => {
  const allowed = new Set([CHANNEL_ID]);
  const otherPubkey = getPublicKey(OTHER_SECRET);
  const open = OPEN_CODING_SESSION_INGRESS_AUTHORITY;

  // The config authority rejects a signer outside the local allowlist…
  assert.equal(
    classifyTrustedCodingSessionIngressEvent(
      receiptEvent(createdReceipt(), { secret: OTHER_SECRET }),
      allowed,
      AUTHORITY,
    ).kind,
    "rejected-author",
  );
  // …the open authority admits them: channel membership is the authority,
  // and the relay only accepts these kinds from members.
  const classified = classifyTrustedCodingSessionIngressEvent(
    receiptEvent(createdReceipt(), { secret: OTHER_SECRET }),
    allowed,
    open,
  );
  assert.equal(classified.kind, "receipt");
  assert.equal(classified.signerPubkey, otherPubkey);

  // Open never widens what a signature or channel scope would reject.
  const forged = { ...receiptEvent(), pubkey: otherPubkey };
  assert.equal(
    classifyTrustedCodingSessionIngressEvent(forged, allowed, open).kind,
    "invalid-signature",
  );
  assert.equal(
    classifyTrustedCodingSessionIngressEvent(
      receiptEvent(createdReceipt(), { channelId: "not-subscribed" }),
      allowed,
      open,
    ).kind,
    "malformed",
  );

  // No authors constraint on the relay filter; a stable authority identity.
  const filter = buildTrustedCodingSessionIngressFilter([CHANNEL_ID], open, 10);
  assert.equal("authors" in filter, false);
  assert.equal(buildCodingSessionIngressAuthorityIdentity(open), "open");
});

// ── B1 code-coordinate facts (observedCommit/dirty/relayReachable/verifiedAt) ─

const FACTS = {
  observedCommit: "3b884e3562db861b07f078535f27074d2d44146f",
  dirty: true,
  relayReachable: true,
  verifiedAt: 1_800_000_123,
};
const NULL_FACTS = {
  observedCommit: null,
  dirty: null,
  relayReachable: null,
  verifiedAt: null,
};

test("metadata accepts exactly the four amendment dialects the provider emits", () => {
  const sessionRef = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
  // base, base+sessionRef, base+facts, base+sessionRef+facts — the Rust
  // producer's METADATA_FACT_FIELDS discipline, mirrored here.
  for (const shape of [
    metadata(),
    metadata({ sessionRef }),
    metadata({ ...FACTS }),
    metadata({ sessionRef, ...FACTS }),
    metadata({ ...NULL_FACTS }),
  ]) {
    const parsed = parseBuzzCodingSessionMetadata(JSON.stringify(shape));
    assert.notEqual(parsed, null, JSON.stringify(shape));
  }
  const parsed = parseBuzzCodingSessionMetadata(
    JSON.stringify(metadata({ ...FACTS })),
  );
  assert.equal(parsed.observedCommit, FACTS.observedCommit);
  assert.equal(parsed.dirty, true);
  assert.equal(parsed.relayReachable, true);
  assert.equal(parsed.verifiedAt, FACTS.verifiedAt);
});

test("a partial fact subset is corruption, not a dialect", () => {
  const keys = Object.keys(FACTS);
  for (let drop = 0; drop < keys.length; drop += 1) {
    const partial = { ...FACTS };
    delete partial[keys[drop]];
    assert.equal(
      parseBuzzCodingSessionMetadata(JSON.stringify(metadata(partial))),
      null,
      `dropping ${keys[drop]} must reject`,
    );
  }
  // A single stray fact key is equally partial.
  assert.equal(
    parseBuzzCodingSessionMetadata(JSON.stringify(metadata({ dirty: false }))),
    null,
  );
});

test("fact fields are typed and verifiedAt travels with relayReachable", () => {
  for (const bad of [
    { ...FACTS, observedCommit: 42 },
    { ...FACTS, dirty: "yes" },
    { ...FACTS, relayReachable: 1 },
    { ...FACTS, verifiedAt: 1.5 },
    { ...FACTS, verifiedAt: "soon" },
    // The probe invariant: both null or both set.
    { ...FACTS, relayReachable: null },
    { ...FACTS, verifiedAt: null },
  ]) {
    assert.equal(
      parseBuzzCodingSessionMetadata(JSON.stringify(metadata(bad))),
      null,
      JSON.stringify(bad),
    );
  }
});

test("a fact-bearing 44223 resolves the create that used to wedge on it", () => {
  // Regression for the schema-drift wedge: receipt accepted, fact-bearing
  // metadata rejected, session stuck at awaiting-metadata forever.
  const store = new TrustedCodingSessionIngressStore();
  store.ingestRelayEvents([receiptEvent()], [CHANNEL_ID], AUTHORITY);
  store.ingestRelayEvents(
    [metadataEvent(metadata({ ...FACTS }))],
    [CHANNEL_ID],
    AUTHORITY,
  );
  const resolved = store.resolveLifecycle(
    CHANNEL_ID,
    "create-1",
    PROVIDER_PUBKEY,
  );
  assert.equal(resolved.state, "created");
  assert.equal(resolved.metadata.observedCommit, FACTS.observedCommit);
});

test("unreadable metadata for the awaited target trips the drift counter", () => {
  const store = new TrustedCodingSessionIngressStore();
  store.ingestRelayEvents([receiptEvent()], [CHANNEL_ID], AUTHORITY);
  // A future amendment this build has never heard of: valid tags, valid
  // signer, undecodable payload.
  store.ingestRelayEvents(
    [
      metadataEvent(metadata(), {
        content: JSON.stringify(metadata({ futureField: "v2" })),
      }),
    ],
    [CHANNEL_ID],
    AUTHORITY,
  );
  const resolved = store.resolveLifecycle(
    CHANNEL_ID,
    "create-1",
    PROVIDER_PUBKEY,
  );
  assert.equal(resolved.state, "awaiting-metadata");
  assert.equal(resolved.malformedMetadataCount, 1);
  // A malformed payload for a DIFFERENT target must not trip this create.
  const other = new TrustedCodingSessionIngressStore();
  other.ingestRelayEvents([receiptEvent()], [CHANNEL_ID], AUTHORITY);
  other.ingestRelayEvents(
    [
      metadataEvent(metadata({ session: OTHER_TARGET }), {
        content: JSON.stringify(
          metadata({ session: OTHER_TARGET, futureField: "v2" }),
        ),
      }),
    ],
    [CHANNEL_ID],
    AUTHORITY,
  );
  assert.equal(
    other.resolveLifecycle(CHANNEL_ID, "create-1", PROVIDER_PUBKEY)
      .malformedMetadataCount,
    0,
  );
});

test("retainedShelfEvents selects only the newest accepted metadata per session", () => {
  const store = new TrustedCodingSessionIngressStore();
  const older = metadataEvent(metadata({ status: "running" }), {
    createdAt: 1_800_000_001,
  });
  const newer = metadataEvent(metadata({ status: "idle" }), {
    createdAt: 1_800_000_050,
  });
  const otherSession = metadataEvent(metadata({ session: OTHER_TARGET }), {
    createdAt: 1_800_000_002,
  });
  store.ingestRelayEvents(
    [
      receiptEvent(),
      older,
      newer,
      otherSession,
      transcriptEvent(transcript(), { createdAt: 1_800_000_003 }),
    ],
    [CHANNEL_ID],
    AUTHORITY,
  );

  const shelf = store.retainedShelfEvents();
  assert.deepEqual(
    shelf.map((event) => event.id).sort(),
    [newer.id, otherSession.id].sort(),
    "newest metadata per session only — no receipts, no transcripts, no stale metadata",
  );
});

test("a shelf cache rehydrates through the classifier and drops rejected authors", () => {
  const source = new TrustedCodingSessionIngressStore();
  source.ingestRelayEvents([metadataEvent()], [CHANNEL_ID], AUTHORITY);
  const cached = source.retainedShelfEvents();
  assert.equal(cached.length, 1);

  // Same authority: the cached signed bytes rebuild the same metadata view.
  const rehydrated = new TrustedCodingSessionIngressStore();
  rehydrated.ingestRelayEvents(cached, [CHANNEL_ID], AUTHORITY);
  const snapshot = rehydrated.snapshot([CHANNEL_ID]);
  assert.equal(snapshot.metadata.length, 1);
  assert.equal(snapshot.metadata[0].metadata.status, "running");

  // An authority that does not allow the signer rejects the cache instead of
  // trusting it — a stale or cross-identity cache cannot inject rows.
  const hostile = new TrustedCodingSessionIngressStore();
  hostile.ingestRelayEvents(
    cached,
    [CHANNEL_ID],
    resolveCodingSessionIngressAuthority([
      { pubkey: getPublicKey(OTHER_SECRET), label: "someone else" },
    ]),
  );
  const rejected = hostile.snapshot([CHANNEL_ID]);
  assert.equal(rejected.metadata.length, 0);
  assert.equal(rejected.rejectedAuthorCount, 1);
});

// ---------------------------------------------------------------------------
// Per-stage turn receipts (44224 `turn_*`)
// ---------------------------------------------------------------------------

const TURN_COMMAND_ID = "csc-turn-1";

function turnReceipt(status, overrides = {}) {
  const base = {
    schema: CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
    commandId: TURN_COMMAND_ID,
    status,
    session: TARGET,
    error: null,
  };
  if (status === "turn_started") base.turnId = "turn-abc";
  if (status === "turn_dropped") {
    base.error = { code: "QUEUE_FULL", message: "the session queue is full" };
  }
  if (status === "turn_refused") {
    base.error = {
      code: "UNAUTHORIZED_OPERATOR",
      message: "only the founder or a granted operator may steer this session",
    };
  }
  if (status === "turn_degraded") {
    base.error = {
      code: "STEER_UNSUPPORTED",
      message: "this runtime advertised no native steering",
    };
  }
  return { ...base, ...overrides };
}

function turnReceiptEvent(status, { value, ...options } = {}) {
  const receipt = value ?? turnReceipt(status);
  return receiptEvent(receipt, {
    tags: [
      ["h", options.channelId ?? CHANNEL_ID],
      ["cslr-v", CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION],
      ["csl-command", receipt.commandId],
      ["csl-key", codingSessionReceiptSemanticKey(receipt.commandId, status)],
    ],
    ...options,
  });
}

test("every turn stage decodes, with turnId only on turn_started", () => {
  for (const status of CODING_SESSION_TURN_RECEIPT_STATUSES) {
    const parsed = parseCodingSessionLifecycleReceipt(
      JSON.stringify(turnReceipt(status)),
    );
    assert.ok(parsed, status);
    assert.equal(parsed.status, status);
    assert.deepEqual(parsed.session, TARGET);
    assert.equal(isCodingSessionTurnReceipt(parsed), true);
  }
  const started = parseCodingSessionLifecycleReceipt(
    JSON.stringify(turnReceipt("turn_started")),
  );
  assert.equal(started.turnId, "turn-abc");
  assert.equal(started.error, null);
  const queued = parseCodingSessionLifecycleReceipt(
    JSON.stringify(turnReceipt("turn_queued")),
  );
  assert.equal("turnId" in queued, false);
});

test("a turn receipt with an unexpected key is rejected outright", () => {
  const cases = [
    // `turnId` belongs to exactly one status.
    turnReceipt("turn_queued", { turnId: "turn-abc" }),
    turnReceipt("turn_refused", { turnId: "turn-abc" }),
    // ... and turn_started cannot do without it.
    (() => {
      const value = turnReceipt("turn_started");
      delete value.turnId;
      return value;
    })(),
    turnReceipt("turn_started", { turnId: "" }),
    // An error where there must be none, and none where there must be one.
    turnReceipt("turn_queued", { error: { code: "X", message: "y" } }),
    turnReceipt("turn_dropped", { error: null }),
    turnReceipt("turn_refused", { error: { code: "X" } }),
    // A turn receipt always names the execution it was addressed to.
    turnReceipt("turn_queued", { session: null }),
    // Any extra key at all.
    turnReceipt("turn_queued", { deliver: "boundary" }),
    turnReceipt("turn_started", { deliver: "boundary" }),
  ];
  for (const value of cases) {
    assert.equal(
      parseCodingSessionLifecycleReceipt(JSON.stringify(value)),
      null,
      JSON.stringify(value),
    );
  }
});

test("each turn stage carries its own semantic key", () => {
  const keys = CODING_SESSION_TURN_RECEIPT_STATUSES.map((status) =>
    codingSessionReceiptSemanticKey(TURN_COMMAND_ID, status),
  );
  assert.equal(new Set(keys).size, keys.length);
  for (const key of keys) {
    assert.notEqual(key, lifecycleReceiptSemanticKey(TURN_COMMAND_ID));
  }
  // Lifecycle statuses keep the key the provider has always published.
  assert.equal(
    codingSessionReceiptSemanticKey("create-1", "created"),
    lifecycleReceiptSemanticKey("create-1"),
  );
});

test("a turn receipt keyed by command id alone is malformed", () => {
  const receipt = turnReceipt("turn_queued");
  const event = receiptEvent(receipt, {
    tags: [
      ["h", CHANNEL_ID],
      ["cslr-v", CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION],
      ["csl-command", receipt.commandId],
      ["csl-key", lifecycleReceiptSemanticKey(receipt.commandId)],
    ],
  });
  assert.equal(
    classifyTrustedCodingSessionIngressEvent(
      event,
      new Set([CHANNEL_ID]),
      AUTHORITY,
    ).kind,
    "malformed",
  );
});

test("queued then started is two facts about one turn, not a conflict", () => {
  const store = new TrustedCodingSessionIngressStore();
  store.ingestRelayEvents(
    [turnReceiptEvent("turn_queued")],
    [CHANNEL_ID],
    AUTHORITY,
  );
  assert.deepEqual(
    store.resolveTurnProgress(CHANNEL_ID, TURN_COMMAND_ID, PROVIDER_PUBKEY),
    { stage: "queued" },
  );
  store.ingestRelayEvents(
    [turnReceiptEvent("turn_started")],
    [CHANNEL_ID],
    AUTHORITY,
  );
  assert.deepEqual(
    store.resolveTurnProgress(CHANNEL_ID, TURN_COMMAND_ID, PROVIDER_PUBKEY),
    { stage: "started", turnId: "turn-abc" },
  );
  assert.equal(
    store.resolveTurnStartedAtMs(CHANNEL_ID, "turn-abc", PROVIDER_PUBKEY),
    1_800_000_000_000,
  );
  assert.equal(
    store.resolveTurnStartedAtMs(CHANNEL_ID, "other-turn", PROVIDER_PUBKEY),
    null,
  );
  assert.equal(
    store.resolveTurnRefusal(CHANNEL_ID, TURN_COMMAND_ID, PROVIDER_PUBKEY),
    null,
  );
});

test("a degraded steer is progress, not a failure, and outranks queued", () => {
  const store = new TrustedCodingSessionIngressStore();
  store.ingestRelayEvents(
    [turnReceiptEvent("turn_degraded"), turnReceiptEvent("turn_queued")],
    [CHANNEL_ID],
    AUTHORITY,
  );
  // The provider says both: it could not steer, and the turn is in the
  // mailbox. The later fact about the same turn is the one the row reads.
  assert.deepEqual(
    store.resolveTurnProgress(CHANNEL_ID, TURN_COMMAND_ID, PROVIDER_PUBKEY),
    { stage: "degraded" },
  );
  // A turn that will still run must never restore the draft.
  assert.equal(
    store.resolveTurnRefusal(CHANNEL_ID, TURN_COMMAND_ID, PROVIDER_PUBKEY),
    null,
  );
  store.ingestRelayEvents(
    [turnReceiptEvent("turn_started")],
    [CHANNEL_ID],
    AUTHORITY,
  );
  assert.deepEqual(
    store.resolveTurnProgress(CHANNEL_ID, TURN_COMMAND_ID, PROVIDER_PUBKEY),
    { stage: "started", turnId: "turn-abc" },
  );
});

test("an issued interrupt is neither progress on a turn nor a refusal", () => {
  const store = new TrustedCodingSessionIngressStore();
  store.ingestRelayEvents(
    [turnReceiptEvent("interrupt_delivered")],
    [CHANNEL_ID],
    AUTHORITY,
  );
  assert.equal(
    store.resolveTurnProgress(CHANNEL_ID, TURN_COMMAND_ID, PROVIDER_PUBKEY),
    null,
  );
  assert.equal(
    store.resolveTurnRefusal(CHANNEL_ID, TURN_COMMAND_ID, PROVIDER_PUBKEY),
    null,
  );
});

test("a refused turn and a dropped turn read back as different outcomes", () => {
  const refused = new TrustedCodingSessionIngressStore();
  refused.ingestRelayEvents(
    [turnReceiptEvent("turn_refused")],
    [CHANNEL_ID],
    AUTHORITY,
  );
  assert.deepEqual(
    refused.resolveTurnRefusal(CHANNEL_ID, TURN_COMMAND_ID, PROVIDER_PUBKEY),
    {
      code: "UNAUTHORIZED_OPERATOR",
      message: "only the founder or a granted operator may steer this session",
      outcome: "refused",
    },
  );

  const dropped = new TrustedCodingSessionIngressStore();
  dropped.ingestRelayEvents(
    [turnReceiptEvent("turn_queued"), turnReceiptEvent("turn_dropped")],
    [CHANNEL_ID],
    AUTHORITY,
  );
  assert.deepEqual(
    dropped.resolveTurnRefusal(CHANNEL_ID, TURN_COMMAND_ID, PROVIDER_PUBKEY),
    {
      code: "QUEUE_FULL",
      message: "the session queue is full",
      outcome: "dropped",
    },
  );
});

test("a turn receipt never decides a generation's lifecycle", () => {
  const store = new TrustedCodingSessionIngressStore();
  // The same command id on purpose: even then, a turn stage must not create,
  // confirm, or fail the generation the create is waiting on.
  store.ingestRelayEvents(
    [
      turnReceiptEvent("turn_refused", {
        value: turnReceipt("turn_refused", { commandId: "create-1" }),
      }),
      turnReceiptEvent("turn_queued", {
        value: turnReceipt("turn_queued", { commandId: "create-1" }),
      }),
    ],
    [CHANNEL_ID],
    AUTHORITY,
  );
  assert.deepEqual(
    store.resolveLifecycle(CHANNEL_ID, "create-1", PROVIDER_PUBKEY),
    { state: "pending", commandId: "create-1" },
  );
});

test("a turn receipt from another provider is not this turn's answer", () => {
  const store = new TrustedCodingSessionIngressStore();
  store.ingestRelayEvents(
    [turnReceiptEvent("turn_refused", { secret: OTHER_SECRET })],
    [CHANNEL_ID],
    OPEN_CODING_SESSION_INGRESS_AUTHORITY,
  );
  assert.equal(
    store.resolveTurnRefusal(CHANNEL_ID, TURN_COMMAND_ID, PROVIDER_PUBKEY),
    null,
  );
});
