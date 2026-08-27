/**
 * Signed fixtures for the coding-session domain tests.
 *
 * These are REAL signatures, minted with nostr-tools, not stubs: the trust
 * gate's whole job is to verify them, so a fake signer would test the test.
 * Not a `.test.mjs`, so the runner never treats it as a suite.
 */
import {
  finalizeEvent,
  generateSecretKey,
  getPublicKey,
} from "nostr-tools/pure";
import {
  buildCodingSessionTargetKey,
  codingSessionMetadataSemanticKey,
  codingSessionReceiptSemanticKey,
  codingSessionTranscriptSemanticKey,
} from "./keys.ts";

export const CHANNEL_ID = "11111111-1111-4111-8111-111111111111";
export const SESSION_REF = "22222222-2222-4222-8222-222222222222";
export const OTHER_SESSION_REF = "33333333-3333-4333-8333-333333333333";

/** A fresh keypair. `pk` is 64 lowercase hex, as every decoder demands. */
export function newSigner() {
  const secretKey = generateSecretKey();
  return { secretKey, pubkey: getPublicKey(secretKey) };
}

export function sign(signer, template) {
  return finalizeEvent(
    {
      kind: template.kind,
      content: template.content,
      tags: template.tags,
      created_at: template.created_at ?? 1_700_000_000,
    },
    signer.secretKey,
  );
}

export function target(overrides = {}) {
  return {
    driver: "provider-a",
    instanceId: "instance-1",
    sessionId: "session-1",
    generation: 1,
    ...overrides,
  };
}

export function capabilities() {
  return {
    threadTurnStart: true,
    threadTurnInterrupt: true,
    threadSteer: false,
    context: false,
    diff: false,
    plan: false,
  };
}

export function metadataEvent(signer, options = {}) {
  const session = options.target ?? target();
  const content = {
    schema: "buzz-coding-session-metadata/v1",
    session,
    projectRef: options.projectRef ?? null,
    repoRef: options.repoRef ?? null,
    title: options.title ?? "A session",
    agentRef: options.agentRef ?? null,
    provider: options.provider ?? null,
    runtime: options.runtime ?? "codex-acp",
    model: options.model ?? "gpt-5.4[high]",
    status: options.status ?? "idle",
    branch: null,
    capabilities: capabilities(),
    ...(options.sessionRef ? { sessionRef: options.sessionRef } : {}),
  };
  return sign(signer, {
    kind: 44223,
    created_at: options.created_at ?? 1_700_000_000,
    content: JSON.stringify(content),
    tags: [
      ["h", options.channelId ?? CHANNEL_ID],
      ["csm-v", "csm1-1"],
      ["cs-target", buildCodingSessionTargetKey(session)],
      ["csm-key", codingSessionMetadataSemanticKey(session)],
    ],
  });
}

export function receiptEvent(signer, options = {}) {
  const session = options.target ?? target();
  const status = options.status ?? "created";
  const commandId = options.commandId ?? "command-1";
  const isTurn = [
    "turn_queued",
    "turn_started",
    "turn_degraded",
    "turn_dropped",
    "turn_refused",
    "interrupt_delivered",
  ].includes(status);
  const content = {
    schema: "buzz-coding-session-lifecycle-receipt/v1",
    commandId,
    status,
    session,
    error: options.error ?? null,
    ...(status === "turn_started"
      ? { turnId: options.turnId ?? "turn-1" }
      : {}),
  };
  return sign(signer, {
    kind: 44224,
    created_at: options.created_at ?? 1_700_000_000,
    content: JSON.stringify(content),
    tags: [
      ["h", options.channelId ?? CHANNEL_ID],
      ["cslr-v", "cslr1-1"],
      ["csl-command", commandId],
      ["csl-key", codingSessionReceiptSemanticKey(commandId, status, isTurn)],
    ],
  });
}

export function transcriptEvent(signer, options = {}) {
  const session = options.target ?? target();
  const eventSeq = options.eventSeq ?? 1;
  const content = {
    schema: "buzz-coding-session-transcript/v1",
    session,
    eventSeq,
    timestamp: options.timestamp ?? 1_700_000_000_000,
    turnId: options.turnId === undefined ? null : options.turnId,
    item: options.item ?? { kind: "assistant_text", text: "hello" },
  };
  return sign(signer, {
    kind: 44225,
    created_at: options.created_at ?? 1_700_000_000,
    content: JSON.stringify(content),
    tags: [
      ["h", options.channelId ?? CHANNEL_ID],
      ["cst-v", "cst1-1"],
      ["cs-target", buildCodingSessionTargetKey(session)],
      ["cst-seq", String(eventSeq)],
      ["cst-key", codingSessionTranscriptSemanticKey(session, eventSeq)],
    ],
  });
}

export function createEvent(signer, options = {}) {
  const commandId = options.commandId ?? "command-1";
  const action = {
    type: "session.create",
    projectRef: options.projectRef ?? null,
    repoRef: null,
    ...(options.sessionRef === undefined
      ? {}
      : { sessionRef: options.sessionRef }),
    ...(options.genesisRef === undefined
      ? {}
      : { genesisRef: options.genesisRef }),
    providerInstanceRef: options.providerInstanceRef ?? "provider-a",
    providerAuthorityPubkey: options.providerAuthorityPubkey,
    model: options.model ?? null,
    title: options.title ?? null,
    initialTurn: null,
  };
  return sign(signer, {
    kind: 44221,
    created_at: options.created_at ?? 1_699_999_000,
    content: JSON.stringify({
      schema: "buzz-coding-session-lifecycle-command/v1",
      commandId,
      action,
    }),
    tags: [
      ["h", options.channelId ?? CHANNEL_ID],
      ["csl-v", "csl1-1"],
      ["csl-command", commandId],
    ],
  });
}

export function genesisEvent(signer, options = {}) {
  const sessionRef = options.sessionRef ?? SESSION_REF;
  return sign(signer, {
    kind: 44226,
    created_at: options.created_at ?? 1_699_998_000,
    content: JSON.stringify({ sessionRef, v: 1 }),
    tags: [
      ["h", options.channelId ?? CHANNEL_ID],
      ["csg-v", "csg1-1"],
      ["csg-session", sessionRef],
    ],
  });
}

export function nameEvent(signer, options = {}) {
  const sessionRef = options.sessionRef ?? SESSION_REF;
  return sign(signer, {
    kind: 44229,
    created_at: options.created_at ?? 1_700_000_500,
    content: options.name ?? "Named session",
    tags: [
      ["h", options.channelId ?? CHANNEL_ID],
      ["d", sessionRef],
      ["csnm-v", "csnm1-1"],
    ],
  });
}

export function closureEvent(signer, options = {}) {
  const sessionRef = options.sessionRef ?? SESSION_REF;
  const genesisRef = options.genesisRef ?? "a".repeat(64);
  return sign(signer, {
    kind: 44230,
    created_at: options.created_at ?? 1_700_000_600,
    content: JSON.stringify({
      action: options.action ?? "closed",
      genesisRef,
      sessionRef,
      v: 1,
    }),
    tags: [
      ["h", options.channelId ?? CHANNEL_ID],
      ["d", sessionRef],
      ["cscl-v", "cscl1-1"],
      ["cscl-genesis", genesisRef],
    ],
  });
}

export function leaseEvent(signer, options = {}) {
  const session = options.target ?? target();
  const leaseSequence = options.leaseSequence ?? 1;
  const commandId = options.commandId ?? "command-1";
  return sign(signer, {
    kind: 24223,
    created_at: options.created_at ?? 1_700_000_000,
    content: JSON.stringify({
      schema: "buzz-coding-session-lease/v1",
      target: session,
      state: options.state ?? "live",
      leaseSequence,
    }),
    tags: [
      ["h", options.channelId ?? CHANNEL_ID],
      ["cslease-v", "cslease1-1"],
      ["cs-target", buildCodingSessionTargetKey(session)],
      ["csl-command", commandId],
      ["cslease-seq", String(leaseSequence)],
    ],
  });
}

/** Re-sign a structurally mutated event, so the test exercises the structure. */
export function resign(signer, event) {
  return sign(signer, {
    kind: event.kind,
    content: event.content,
    tags: event.tags,
    created_at: event.created_at,
  });
}

/** Break a signature without touching anything else about the event. */
export function corruptSignature(event) {
  const flipped = event.sig.startsWith("0")
    ? `1${event.sig.slice(1)}`
    : `0${event.sig.slice(1)}`;
  return { ...event, sig: flipped };
}
