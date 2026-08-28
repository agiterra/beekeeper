import assert from "node:assert/strict";
import { test } from "node:test";
import {
  isCodingSessionTurnReceiptStatus,
  parseBuzzCodingSessionMetadata,
  parseCodingSessionLifecycleReceipt,
} from "./ingressPayloads.ts";
import { parseCodingSessionLifecycleCommand } from "./lifecycleCommand.ts";
import {
  parseCodingSessionClosure,
  parseCodingSessionGenesis,
  parseCodingSessionGoal,
  parseCodingSessionName,
} from "./sessionRecords.ts";
import { parseBuzzCodingSessionTranscript } from "./transcriptEnvelope.ts";
import {
  CHANNEL_ID,
  capabilities,
  closureEvent,
  createEvent,
  genesisEvent,
  nameEvent,
  newSigner,
  resign,
  SESSION_REF,
  target,
} from "./testFixtures.mjs";

const RECEIPT_SCHEMA = "buzz-coding-session-lifecycle-receipt/v1";

function receiptJson(overrides) {
  return JSON.stringify({
    schema: RECEIPT_SCHEMA,
    commandId: "cmd",
    status: "created",
    session: target(),
    error: null,
    ...overrides,
  });
}

test("a lifecycle receipt carries exactly five keys", () => {
  assert.ok(parseCodingSessionLifecycleReceipt(receiptJson({})));
  const withExtra = JSON.parse(receiptJson({}));
  withExtra.extra = 1;
  assert.equal(
    parseCodingSessionLifecycleReceipt(JSON.stringify(withExtra)),
    null,
  );
  const missing = JSON.parse(receiptJson({}));
  delete missing.error;
  assert.equal(
    parseCodingSessionLifecycleReceipt(JSON.stringify(missing)),
    null,
  );
});

test("turnId is legal only on turn_started", () => {
  assert.ok(
    parseCodingSessionLifecycleReceipt(
      receiptJson({ status: "turn_started", turnId: "t1" }),
    ),
  );
  assert.equal(
    parseCodingSessionLifecycleReceipt(receiptJson({ status: "turn_started" })),
    null,
    "turn_started without a turnId is malformed",
  );
  assert.equal(
    parseCodingSessionLifecycleReceipt(receiptJson({ turnId: "t1" })),
    null,
    "a lifecycle status may not carry a turnId",
  );
});

test("the six turn statuses are named exactly", () => {
  for (const status of [
    "turn_queued",
    "turn_started",
    "turn_degraded",
    "turn_dropped",
    "turn_refused",
    "interrupt_delivered",
  ]) {
    assert.equal(isCodingSessionTurnReceiptStatus(status), true, status);
  }
  for (const status of [
    "created",
    "created_with_failed_initial_turn",
    "failed",
    "resumed",
    "resumed_without_context",
    "stopped",
  ]) {
    assert.equal(isCodingSessionTurnReceiptStatus(status), false, status);
  }
});

test("an interrupt and a degraded steer decode, they are not malformed", () => {
  const interrupt = parseCodingSessionLifecycleReceipt(
    receiptJson({ status: "interrupt_delivered" }),
  );
  assert.ok(interrupt, "the provider publishes interrupt_delivered");
  assert.equal(interrupt.error, null, "an interrupt carries no error");
  assert.equal(
    parseCodingSessionLifecycleReceipt(
      receiptJson({
        status: "interrupt_delivered",
        error: { code: "NOPE", message: "no" },
      }),
    ),
    null,
    "and an interrupt with an error is malformed",
  );

  const degraded = parseCodingSessionLifecycleReceipt(
    receiptJson({
      status: "turn_degraded",
      error: { code: "STEER_UNSUPPORTED", message: "queued for the boundary" },
    }),
  );
  assert.ok(degraded, "a degraded steer is a relabelling, not a failure");
  assert.equal(degraded.error.code, "STEER_UNSUPPORTED");
  assert.equal(
    parseCodingSessionLifecycleReceipt(
      receiptJson({ status: "turn_degraded" }),
    ),
    null,
    "and a degraded steer without a reason is malformed",
  );
});

test("a failed receipt must carry a null session and an error", () => {
  assert.ok(
    parseCodingSessionLifecycleReceipt(
      receiptJson({
        status: "failed",
        session: null,
        error: { code: "NOPE", message: "no" },
      }),
    ),
  );
  assert.equal(
    parseCodingSessionLifecycleReceipt(
      receiptJson({ status: "failed", session: null, error: null }),
    ),
    null,
  );
});

test("a target with a zero or fractional generation is refused", () => {
  for (const generation of [0, -1, 1.5, "1"]) {
    assert.equal(
      parseCodingSessionLifecycleReceipt(
        receiptJson({ session: target({ generation }) }),
      ),
      null,
      `generation ${generation}`,
    );
  }
});

function metadataJson(overrides) {
  return JSON.stringify({
    schema: "buzz-coding-session-metadata/v1",
    session: target(),
    projectRef: null,
    repoRef: null,
    title: "t",
    agentRef: null,
    provider: null,
    runtime: "codex-acp",
    model: "gpt-5.4",
    status: "idle",
    branch: null,
    capabilities: capabilities(),
    ...overrides,
  });
}

test("metadata accepts exactly the ten declared statuses", () => {
  for (const status of [
    "starting",
    "idle",
    "running",
    "waiting_for_input",
    "completed",
    "stopped",
    "failed",
    "interrupted",
    "disconnected",
    "unknown",
  ]) {
    assert.ok(parseBuzzCodingSessionMetadata(metadataJson({ status })), status);
  }
  assert.equal(
    parseBuzzCodingSessionMetadata(metadataJson({ status: "busy" })),
    null,
  );
});

test("a sessionRef echo must be the canonical UUID or the payload is refused", () => {
  assert.ok(
    parseBuzzCodingSessionMetadata(metadataJson({ sessionRef: SESSION_REF })),
  );
  assert.equal(
    parseBuzzCodingSessionMetadata(metadataJson({ sessionRef: "not-a-uuid" })),
    null,
  );
  assert.equal(
    parseBuzzCodingSessionMetadata(
      metadataJson({ sessionRef: "ABCDEF12-2222-4222-8222-222222222222" }),
    ),
    null,
    "uppercase hex is a different byte string, not a dialect",
  );
});

test("the four code-coordinate facts travel all-or-none", () => {
  assert.ok(
    parseBuzzCodingSessionMetadata(
      metadataJson({
        observedCommit: "abc",
        dirty: false,
        relayReachable: true,
        verifiedAt: 1,
      }),
    ),
  );
  assert.equal(
    parseBuzzCodingSessionMetadata(metadataJson({ observedCommit: "abc" })),
    null,
    "a partial subset is corruption, never a version skew",
  );
});

// One 33 KiB title trips the 2 KiB label bound long before the 32 KiB content
// bound is reached, so a single oversized-title case passes for the wrong
// reason and either bound could regress alone with the suite green. Each bound
// gets a payload only it can refuse.
test("an oversized title is refused by the label bound", () => {
  assert.equal(
    parseBuzzCodingSessionMetadata(
      metadataJson({ title: "x".repeat(3 * 1024) }),
    ),
    null,
    "3 KiB is well under the content bound, so only the label bound can refuse it",
  );
});

test("an oversized summary is refused by the summary bound", () => {
  assert.equal(
    parseBuzzCodingSessionMetadata(
      metadataJson({ contextSummary: "x".repeat(17 * 1024) }),
    ),
    null,
    "17 KiB is under the content bound, so only the summary bound can refuse it",
  );
});

test("metadata over 32 KiB is refused rather than truncated", () => {
  const huge = metadataJson({
    contextSummary: "c".repeat(12 * 1024),
    diffSummary: "d".repeat(12 * 1024),
    planSummary: "p".repeat(12 * 1024),
  });
  assert.ok(huge.length > 32 * 1024, "the payload really is over the bound");
  assert.equal(
    parseBuzzCodingSessionMetadata(huge),
    null,
    "every field is within its own bound, so only the content bound refuses it",
  );
});

test("a transcript envelope has exactly six keys and a positive eventSeq", () => {
  const base = {
    schema: "buzz-coding-session-transcript/v1",
    session: target(),
    eventSeq: 1,
    timestamp: 1_700_000_000_000,
    turnId: null,
    item: { kind: "assistant_text", text: "x" },
  };
  assert.ok(parseBuzzCodingSessionTranscript(JSON.stringify(base)));
  assert.equal(
    parseBuzzCodingSessionTranscript(JSON.stringify({ ...base, eventSeq: 0 })),
    null,
  );
  assert.equal(
    parseBuzzCodingSessionTranscript(JSON.stringify({ ...base, extra: 1 })),
    null,
  );
  assert.equal(
    parseBuzzCodingSessionTranscript(
      JSON.stringify({ ...base, item: { kind: "" } }),
    ),
    null,
    "an item needs a nonempty kind",
  );
});

test("a transcript item deeper than 24 levels is refused", () => {
  let nested = { deep: true };
  for (let index = 0; index < 30; index += 1) nested = { nested };
  assert.equal(
    parseBuzzCodingSessionTranscript(
      JSON.stringify({
        schema: "buzz-coding-session-transcript/v1",
        session: target(),
        eventSeq: 1,
        timestamp: 1,
        turnId: null,
        item: { kind: "wat", payload: nested },
      }),
    ),
    null,
  );
});

test("a 44221 create names its provider authority in lowercase hex", () => {
  const operator = newSigner();
  const provider = newSigner();
  const create = parseCodingSessionLifecycleCommand(
    createEvent(operator, { providerAuthorityPubkey: provider.pubkey }),
  );
  assert.ok(create);
  assert.equal(create.action, "create");
  assert.equal(create.providerAuthorityPubkey, provider.pubkey);
  assert.equal(create.channelId, CHANNEL_ID);
  assert.equal(create.signerPubkey, operator.pubkey);
});

test("a create whose providerAuthorityPubkey is not 64 hex is refused", () => {
  const operator = newSigner();
  assert.equal(
    parseCodingSessionLifecycleCommand(
      createEvent(operator, { providerAuthorityPubkey: "nope" }),
    ),
    null,
  );
});

test("a genesisRef without a sessionRef is malformed, not legacy", () => {
  const operator = newSigner();
  const provider = newSigner();
  assert.equal(
    parseCodingSessionLifecycleCommand(
      createEvent(operator, {
        providerAuthorityPubkey: provider.pubkey,
        sessionRef: null,
        genesisRef: "a".repeat(64),
      }),
    ),
    null,
  );
});

test("a csl-command tag that disagrees with the payload is refused", () => {
  const operator = newSigner();
  const provider = newSigner();
  const event = createEvent(operator, {
    providerAuthorityPubkey: provider.pubkey,
  });
  const swapped = resign(operator, {
    ...event,
    tags: event.tags.map((tag) =>
      tag[0] === "csl-command" ? ["csl-command", "other"] : tag,
    ),
  });
  assert.equal(parseCodingSessionLifecycleCommand(swapped), null);
});

test("a genesis names its session and its signer is the founder", () => {
  const founder = newSigner();
  const genesis = parseCodingSessionGenesis(
    genesisEvent(founder, { sessionRef: SESSION_REF }),
  );
  assert.equal(genesis.founderPubkey, founder.pubkey);
  assert.equal(genesis.sessionRef, SESSION_REF);
});

test("a genesis is refused past 1024 bytes of content", () => {
  // Padding is whitespace, so the payload stays exactly the two keys the
  // decoder demands and only the D4 content bound can refuse it.
  const founder = newSigner();
  const padded = (bytes) =>
    resign(founder, {
      kind: 44226,
      created_at: 1,
      content: `${JSON.stringify({ sessionRef: SESSION_REF, v: 1 })}${" ".repeat(bytes)}`,
      tags: [
        ["h", CHANNEL_ID],
        ["csg-v", "csg1-1"],
        ["csg-session", SESSION_REF],
      ],
    });
  assert.ok(parseCodingSessionGenesis(padded(0)));
  assert.ok(parseCodingSessionGenesis(padded(900)));
  assert.equal(parseCodingSessionGenesis(padded(1100)), null);
});

test("a closure is refused past 512 bytes of content", () => {
  const founder = newSigner();
  const genesisRef = "b".repeat(64);
  const padded = (bytes) => {
    const event = closureEvent(founder, { genesisRef });
    return resign(founder, {
      ...event,
      content: `${event.content}${" ".repeat(bytes)}`,
    });
  };
  assert.ok(parseCodingSessionClosure(padded(0)));
  assert.ok(parseCodingSessionClosure(padded(300)));
  assert.equal(parseCodingSessionClosure(padded(600)), null);
});

test("a session name must be one line within 256 bytes", () => {
  const founder = newSigner();
  assert.ok(parseCodingSessionName(nameEvent(founder, { name: "Fine" })));
  assert.equal(
    parseCodingSessionName(nameEvent(founder, { name: "two\nlines" })),
    null,
  );
  assert.equal(
    parseCodingSessionName(nameEvent(founder, { name: "x".repeat(257) })),
    null,
  );
  assert.equal(
    parseCodingSessionName(nameEvent(founder, { name: "   " })),
    null,
  );
});

test("a closure must echo its own genesis and session refs", () => {
  const founder = newSigner();
  const genesisRef = "b".repeat(64);
  assert.ok(parseCodingSessionClosure(closureEvent(founder, { genesisRef })));
  const event = closureEvent(founder, { genesisRef });
  const swapped = resign(founder, {
    ...event,
    tags: event.tags.map((tag) =>
      tag[0] === "cscl-genesis" ? ["cscl-genesis", "c".repeat(64)] : tag,
    ),
  });
  assert.equal(parseCodingSessionClosure(swapped), null);
});

test("a goal is refused past 4096 bytes", () => {
  const founder = newSigner();
  const goalEvent = (content) =>
    resign(founder, {
      kind: 44227,
      created_at: 1,
      content,
      tags: [
        ["h", CHANNEL_ID],
        ["d", SESSION_REF],
        ["csgl-v", "csgl1-1"],
      ],
    });
  assert.ok(parseCodingSessionGoal(goalEvent("ship it")));
  assert.equal(parseCodingSessionGoal(goalEvent("x".repeat(4097))), null);
});

test("a seated 44221 create (actor + role) parses; half a seat or a bad slug is refused", () => {
  const operator = newSigner();
  const provider = newSigner();
  const seated = parseCodingSessionLifecycleCommand(
    createEvent(operator, {
      providerAuthorityPubkey: provider.pubkey,
      actor: "d".repeat(64),
      role: "lead",
    }),
  );
  assert.ok(seated, "a seated create must parse");
  assert.equal(seated.action, "create");
  for (const bad of [
    { role: "lead" },
    { actor: "d".repeat(64) },
    { actor: "D".repeat(64), role: "lead" },
    { actor: "d".repeat(64), role: "Lead" },
  ]) {
    assert.equal(
      parseCodingSessionLifecycleCommand(
        createEvent(operator, {
          providerAuthorityPubkey: provider.pubkey,
          ...bad,
        }),
      ),
      null,
      JSON.stringify(bad),
    );
  }
});
