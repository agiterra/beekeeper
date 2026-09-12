/**
 * Exact-key discipline for the 44224 receipt decoder.
 *
 * A receipt is a signed claim about a turn this client sent, and the surfaces
 * that read one act on it — a row is relabelled, a draft comes back, a queued
 * turn stops claiming it started. So an object that is *nearly* a receipt is a
 * rejection, never a partial accept: one unexpected key means the producer and
 * this decoder disagree about the contract, and guessing which half is right
 * is how a wrong sentence ends up on screen under a signature.
 */
import assert from "node:assert/strict";
import test from "node:test";

import {
  CODING_SESSION_TURN_RECEIPT_STATUSES,
  codingSessionReceiptSemanticKey,
  isCodingSessionTurnReceiptStatus,
  parseBuzzCodingSessionMetadata,
  parseCodingSessionLifecycleReceipt,
} from "./codingSessionIngressPayloads.ts";

const SESSION = {
  driver: "claude-agent-acp",
  instanceId: "instance-1",
  sessionId: "session-1",
  generation: 2,
};

function receipt(overrides) {
  return JSON.stringify({
    schema: "buzz-coding-session-lifecycle-receipt/v1",
    commandId: "csc-1",
    session: SESSION,
    error: null,
    ...overrides,
  });
}

test("a steer the provider could not honour decodes as a five-key turn_degraded", () => {
  const parsed = parseCodingSessionLifecycleReceipt(
    receipt({
      status: "turn_degraded",
      error: {
        code: "STEER_UNSUPPORTED",
        message: "This runtime advertised no native steering.",
      },
    }),
  );
  assert.ok(parsed, "a well-formed turn_degraded must decode");
  assert.equal(parsed.status, "turn_degraded");
  assert.deepEqual(parsed.session, SESSION);
  assert.equal(parsed.error.code, "STEER_UNSUPPORTED");
  assert.deepEqual(Object.keys(parsed), [
    "schema",
    "commandId",
    "status",
    "session",
    "error",
  ]);
});

test("a six-key turn_degraded is rejected outright", () => {
  assert.equal(
    parseCodingSessionLifecycleReceipt(
      receipt({
        status: "turn_degraded",
        error: { code: "STEER_UNSUPPORTED", message: "no native steering" },
        turnId: "turn-9",
      }),
    ),
    null,
    "turnId belongs to turn_started and turn_injected alone",
  );
  assert.equal(
    parseCodingSessionLifecycleReceipt(
      receipt({ status: "turn_degraded", error: null }),
    ),
    null,
    "a degraded turn always says why it was degraded",
  );
});

test("an issued interrupt decodes as a five-key interrupt_delivered", () => {
  const parsed = parseCodingSessionLifecycleReceipt(
    receipt({ status: "interrupt_delivered" }),
  );
  assert.ok(parsed, "a well-formed interrupt_delivered must decode");
  assert.equal(parsed.error, null);
  assert.deepEqual(parsed.session, SESSION);
  assert.deepEqual(Object.keys(parsed), [
    "schema",
    "commandId",
    "status",
    "session",
    "error",
  ]);
  assert.equal(
    parseCodingSessionLifecycleReceipt(
      receipt({
        status: "interrupt_delivered",
        error: { code: "X", message: "y" },
      }),
    ),
    null,
    "a delivered interrupt is not an error",
  );
  assert.equal(
    parseCodingSessionLifecycleReceipt(
      receipt({ status: "interrupt_delivered", turnId: "turn-9" }),
    ),
    null,
  );
});

test("a turn refusal or drop takes any bounded code the provider grows", () => {
  for (const status of ["turn_refused", "turn_dropped"]) {
    const parsed = parseCodingSessionLifecycleReceipt(
      receipt({
        status,
        error: {
          code: "NO_LIVE_EXECUTION",
          message: "No execution is running for this session.",
        },
      }),
    );
    assert.ok(parsed, `${status} must accept a code this client never saw`);
    assert.equal(parsed.error.code, "NO_LIVE_EXECUTION");
  }
});

test("both new statuses are turn statuses, so their keys name the stage", () => {
  assert.deepEqual(
    [...CODING_SESSION_TURN_RECEIPT_STATUSES],
    [
      "turn_queued",
      "turn_started",
      "turn_injected",
      "turn_degraded",
      "turn_delivery_unknown",
      "turn_dropped",
      "turn_refused",
      "interrupt_delivered",
      "continuation_registered",
    ],
  );
  // Literal, not a call compared to itself: the earlier form was true for any
  // implementation, including one that returned a constant.
  const expectedKeys = {
    turn_degraded:
      "coding-session-lifecycle-receipt/v1|5:csc-113:turn_degraded",
    interrupt_delivered:
      "coding-session-lifecycle-receipt/v1|5:csc-119:interrupt_delivered",
  };
  for (const status of ["turn_degraded", "interrupt_delivered"]) {
    assert.equal(isCodingSessionTurnReceiptStatus(status), true);
    assert.equal(
      codingSessionReceiptSemanticKey("csc-1", status),
      expectedKeys[status],
    );
    assert.notEqual(
      codingSessionReceiptSemanticKey("csc-1", status),
      codingSessionReceiptSemanticKey("csc-1", "turn_queued"),
      "a stage that shares a key with another is fenced out of the relay",
    );
  }
  // A lifecycle receipt keeps the historical single-field key.
  assert.equal(
    codingSessionReceiptSemanticKey("csc-1", "created"),
    "coding-session-lifecycle-receipt/v1|5:csc-1",
  );
});

/**
 * The 44223 decoder has a twin in Rust (`decode_coding_session_metadata`,
 * `crates/buzz-core/src/coding_session_payload.rs`), and the two must accept
 * exactly the same signed events. Where they disagree, one half of the system
 * acts on a metadata the other half silently drops — the execution appears
 * seated in the desktop and unseated in the pulse fold, or the reverse — with
 * nothing anywhere saying the event was refused.
 */
function metadataContent(extra) {
  return JSON.stringify({
    schema: "buzz-coding-session-metadata/v1",
    session: SESSION,
    projectRef: null,
    repoRef: null,
    title: null,
    agentRef: null,
    provider: null,
    runtime: "claude",
    model: null,
    status: "idle",
    branch: null,
    capabilities: {
      threadTurnStart: true,
      threadTurnInterrupt: true,
      threadSteer: false,
      context: false,
      diff: false,
      plan: false,
    },
    ...extra,
  });
}

const SEAT_ACTOR = "ab".repeat(32);

test("an explicit null role decodes as an unseated execution, as Rust reads it", () => {
  // Rust's `(_, None) => {}` arm accepts the key with a null value and the
  // producer's own doc forbids emitting it, so the only way this shape reaches
  // a client is from a producer this one does not control. Dropping the whole
  // metadata over it loses the status, the model and the capabilities too.
  const parsed = parseBuzzCodingSessionMetadata(
    metadataContent({ role: null }),
  );
  assert.notEqual(parsed, null);
  assert.equal(Object.hasOwn(parsed, "role"), false);

  const seated = parseBuzzCodingSessionMetadata(
    metadataContent({ agentRef: SEAT_ACTOR, role: null }),
  );
  assert.equal(seated?.agentRef, SEAT_ACTOR);
  assert.equal(Object.hasOwn(seated, "role"), false);
});

const UMBRELLA = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";

// D9. The producer emits `turnBudget` only beside a `sessionRef` and only with
// a positive `limit`; a decoder that accepted either violation would render a
// crew allowance for a session that has no crew, or print "0 turns allowed"
// where the host meant "no budget".
test("a crew turn budget decodes only beside the umbrella it describes", () => {
  const parsed = parseBuzzCodingSessionMetadata(
    metadataContent({
      sessionRef: UMBRELLA,
      turnBudget: { used: 12, limit: 200 },
    }),
  );
  assert.deepEqual(parsed?.turnBudget, { used: 12, limit: 200 });

  // Over-spent is a real state — the founder is never refused — and is
  // reported rather than clamped.
  assert.deepEqual(
    parseBuzzCodingSessionMetadata(
      metadataContent({
        sessionRef: UMBRELLA,
        turnBudget: { used: 201, limit: 200 },
      }),
    )?.turnBudget,
    { used: 201, limit: 200 },
  );

  for (const [label, extra] of [
    ["no umbrella to bound", { turnBudget: { used: 1, limit: 200 } }],
    [
      "a zero limit",
      { sessionRef: UMBRELLA, turnBudget: { used: 0, limit: 0 } },
    ],
    [
      "a negative count",
      { sessionRef: UMBRELLA, turnBudget: { used: -1, limit: 200 } },
    ],
    [
      "an unknown key",
      {
        sessionRef: UMBRELLA,
        turnBudget: { used: 1, limit: 200, remaining: 199 },
      },
    ],
    ["a missing half", { sessionRef: UMBRELLA, turnBudget: { used: 1 } }],
  ]) {
    assert.equal(
      parseBuzzCodingSessionMetadata(metadataContent(extra)),
      null,
      `accepted a turnBudget with ${label}`,
    );
  }

  // An unbudgeted umbrella omits the key, and still decodes.
  const unbudgeted = parseBuzzCodingSessionMetadata(
    metadataContent({ sessionRef: UMBRELLA }),
  );
  assert.notEqual(unbudgeted, null);
  assert.equal(Object.hasOwn(unbudgeted, "turnBudget"), false);
});

test("an explicit null turnBudget decodes as unbudgeted, as Rust reads it", () => {
  // The mirror of the null-role rule two hundred lines above: Rust's
  // `Option<TurnBudget>` is serde-defaulted, so `contains_key` sees the key,
  // the shape check passes, and the value reads as `None`. Dropping the whole
  // metadata here would lose the status, the model and the capabilities over a
  // key that says nothing.
  const parsed = parseBuzzCodingSessionMetadata(
    metadataContent({ sessionRef: UMBRELLA, turnBudget: null }),
  );
  assert.notEqual(parsed, null);
  assert.equal(Object.hasOwn(parsed, "turnBudget"), false);

  // Even without an umbrella: a null budget makes no claim about one.
  const loose = parseBuzzCodingSessionMetadata(
    metadataContent({ turnBudget: null }),
  );
  assert.notEqual(loose, null);
  assert.equal(Object.hasOwn(loose, "turnBudget"), false);
});

test("an agentRef that is not a 64-hex pubkey is refused, as Rust refuses it", () => {
  // `validate_actor_pubkey` bounds this to lowercase 64-hex; a decoder that
  // only bounds the length hands a display name to `useUsersBatchQuery` and
  // renders a seat that no key can hold.
  for (const agentRef of [
    "not-a-pubkey",
    "AB".repeat(32),
    "ab".repeat(31),
    `${"ab".repeat(32)}f`,
  ]) {
    assert.equal(
      parseBuzzCodingSessionMetadata(metadataContent({ agentRef })),
      null,
      `accepted a non-pubkey agentRef: ${agentRef}`,
    );
  }
  assert.equal(
    parseBuzzCodingSessionMetadata(metadataContent({ agentRef: SEAT_ACTOR }))
      ?.agentRef,
    SEAT_ACTOR,
  );
});

// The 2026-08-30 routing amendment on 44223. The provider echoes the decision
// the create carried, so the seat's own metadata says which gates its model
// cleared and why it was the cheapest of them.
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

test("metadata carrying a routing record decodes it", () => {
  const parsed = parseBuzzCodingSessionMetadata(
    metadataContent({ routing: ROUTING_RECORD }),
  );
  assert.deepEqual(parsed?.routing, ROUTING_RECORD);
});

test("metadata omits absent routing and rejects an explicit null", () => {
  assert.equal(
    parseBuzzCodingSessionMetadata(metadataContent({})).routing,
    undefined,
  );
  assert.equal(
    parseBuzzCodingSessionMetadata(metadataContent({ routing: null })),
    null,
  );
});

test("a malformed routing record refuses the payload, it is not dropped", () => {
  for (const routing of [
    { class: "builder" },
    {
      ...ROUTING_RECORD,
      chosen: { ...ROUTING_RECORD.chosen, effort: "xhigh" },
    },
    { ...ROUTING_RECORD, risk: { ...ROUTING_RECORD.risk, score: 3 } },
  ]) {
    assert.equal(
      parseBuzzCodingSessionMetadata(metadataContent({ routing })),
      null,
      `decoded ${JSON.stringify(routing)} instead of refusing it`,
    );
  }
});

// LANE-L23's `packRef`: the seventh additive key on 44223, same
// omit-when-absent contract as `routing` and `beeStamp` (finding 31's rule —
// every new payload key on an existing kind is read-optional with a stated
// default, and the lane adding it ships a "signed before the key existed"
// decode test).
const PACK_REF = {
  repo: `30617:${SEAT_ACTOR}:agiterra-packs`,
  sha: "b".repeat(40),
  role: "builder",
  path: "personas/roles/builder",
};

test("metadata carrying a packRef decodes it", () => {
  const parsed = parseBuzzCodingSessionMetadata(
    metadataContent({ packRef: PACK_REF }),
  );
  assert.deepEqual(parsed?.packRef, PACK_REF);
});

// The setup-lives-inside-the-app addendum (2026-09-03): a project with no
// 30624 source falls back to the app's own bundled packs, disclosed as a
// `packRef` naming `app:shipped` and a version instead of a commit.
test("metadata carrying a shipped-defaults packRef decodes it", () => {
  const shipped = {
    repo: "app:shipped",
    sha: "0.1.0",
    role: "builder",
    path: "personas/roles/builder",
  };
  const parsed = parseBuzzCodingSessionMetadata(
    metadataContent({ packRef: shipped }),
  );
  assert.deepEqual(parsed?.packRef, shipped);
});

test("a 44223 signed before packRef existed still decodes, with the key absent", () => {
  // No `packRef` key at all — the shape every host before this lane signed.
  const parsed = parseBuzzCodingSessionMetadata(metadataContent({}));
  assert.notEqual(parsed, null, "an older 44223 must still decode");
  assert.equal(Object.hasOwn(parsed, "packRef"), false);
});

test("metadata rejects an explicit null packRef", () => {
  assert.equal(
    parseBuzzCodingSessionMetadata(metadataContent({ packRef: null })),
    null,
  );
});

test("a malformed packRef refuses the payload, it is not dropped", () => {
  for (const packRef of [
    { ...PACK_REF, repo: "not-a-coordinate" },
    { ...PACK_REF, sha: "deadbeef" },
    { ...PACK_REF, role: "Not A Slug" },
    { class: "builder" },
  ]) {
    assert.equal(
      parseBuzzCodingSessionMetadata(metadataContent({ packRef })),
      null,
      `decoded ${JSON.stringify(packRef)} instead of refusing it`,
    );
  }
});

/**
 * A CI-continuation registration (`continuation_registered`) is a stage of one
 * 44220 and nothing more: the provider stored a pending registration and will
 * watch for one exact CI result. It must decode — a status this reader called
 * malformed would leave the sender's `bee ci continue` with no answer at all —
 * and it must decode as a *turn* stage, so no generation fold reads it as a
 * create, a confirmation, or an end.
 */
test("a continuation_registered receipt decodes as a non-mailbox turn stage", () => {
  const parsed = parseCodingSessionLifecycleReceipt(
    receipt({
      commandId: "cic-0123456789abcdef",
      status: "continuation_registered",
    }),
  );
  assert.ok(parsed, "a stored CI continuation must not decode as malformed");
  assert.equal(parsed.status, "continuation_registered");
  assert.equal(parsed.error, null);
  assert.deepEqual(parsed.session, SESSION);
  assert.equal(
    isCodingSessionTurnReceiptStatus("continuation_registered"),
    true,
  );
  // Its own semantic key, so it never fences out the `turn_queued` that may
  // follow under the same command id.
  assert.equal(
    codingSessionReceiptSemanticKey("cic-1", "continuation_registered"),
    "coding-session-lifecycle-receipt/v1|5:cic-123:continuation_registered",
  );
  assert.notEqual(
    codingSessionReceiptSemanticKey("cic-1", "continuation_registered"),
    codingSessionReceiptSemanticKey("cic-1", "turn_queued"),
  );
});

test("a continuation_registered receipt may claim neither a turn nor a failure", () => {
  // Nothing was queued and nothing has been refused yet: a registration that
  // named a turn id would claim a turn that does not exist, and one carrying
  // an error would report a refusal the provider has not made.
  assert.equal(
    parseCodingSessionLifecycleReceipt(
      receipt({
        status: "continuation_registered",
        error: { code: "CI_CONTINUATION_EXPIRED", message: "expired" },
      }),
    ),
    null,
  );
  assert.equal(
    parseCodingSessionLifecycleReceipt(
      JSON.stringify({
        schema: "buzz-coding-session-lifecycle-receipt/v1",
        commandId: "cic-1",
        status: "continuation_registered",
        session: SESSION,
        error: null,
        turnId: "turn-1",
      }),
    ),
    null,
  );
});

// ---------------------------------------------------------------------------
// Native steering: `turn_injected` (six keys) and `turn_delivery_unknown`
// ---------------------------------------------------------------------------

test("an injected steer decodes as the second six-key status, naming the running turn", () => {
  const parsed = parseCodingSessionLifecycleReceipt(
    receipt({ status: "turn_injected", turnId: "turn-running-7" }),
  );
  assert.ok(parsed, "a well-formed turn_injected must decode");
  assert.equal(parsed.status, "turn_injected");
  assert.equal(parsed.turnId, "turn-running-7");
  assert.equal(parsed.error, null);
  assert.deepEqual(parsed.session, SESSION);
  assert.deepEqual(Object.keys(parsed), [
    "schema",
    "commandId",
    "status",
    "session",
    "error",
    "turnId",
  ]);
  assert.equal(isCodingSessionTurnReceiptStatus("turn_injected"), true);
  assert.equal(
    codingSessionReceiptSemanticKey("csc-1", "turn_injected"),
    "coding-session-lifecycle-receipt/v1|5:csc-113:turn_injected",
  );
});

test("an injected steer without the turn it joined, or with an error, is rejected", () => {
  // Same rule as turn_started: the id is the whole claim.
  assert.equal(
    parseCodingSessionLifecycleReceipt(receipt({ status: "turn_injected" })),
    null,
    "no turnId names no turn",
  );
  assert.equal(
    parseCodingSessionLifecycleReceipt(
      receipt({ status: "turn_injected", turnId: "   " }),
    ),
    null,
    "a blank turnId is the same absence",
  );
  assert.equal(
    parseCodingSessionLifecycleReceipt(
      receipt({ status: "turn_injected", turnId: null }),
    ),
    null,
    "an explicit null is a six-key claim of nothing",
  );
  assert.equal(
    parseCodingSessionLifecycleReceipt(
      receipt({
        status: "turn_injected",
        turnId: "turn-running-7",
        error: { code: "STEER_ACK_LOST", message: "x" },
      }),
    ),
    null,
    "an injected input was delivered; it carries no error",
  );
  assert.equal(
    parseCodingSessionLifecycleReceipt(
      receipt({ status: "turn_injected", turnId: "x".repeat(257) }),
    ),
    null,
    "the turn id is bounded like the command id",
  );
});

test("a delivery-unknown answer decodes as five keys with a well-formed code", () => {
  for (const code of [
    "STEER_WRITE_FAILED",
    "STEER_ACK_LOST",
    "STEER_ACK_TIMEOUT",
    "STEER_ACK_UNRECOGNIZED",
    "STEER_UNRESOLVED_AT_RESTART",
    "STEER_UNOBSERVED_NEW_TURN",
  ]) {
    const parsed = parseCodingSessionLifecycleReceipt(
      receipt({
        status: "turn_delivery_unknown",
        error: { code, message: "the prompt ended before the ack arrived" },
      }),
    );
    assert.ok(parsed, code);
    assert.equal(parsed.status, "turn_delivery_unknown");
    assert.equal(parsed.error.code, code);
    assert.deepEqual(Object.keys(parsed), [
      "schema",
      "commandId",
      "status",
      "session",
      "error",
    ]);
  }
  assert.equal(
    codingSessionReceiptSemanticKey("csc-1", "turn_delivery_unknown"),
    "coding-session-lifecycle-receipt/v1|5:csc-121:turn_delivery_unknown",
  );
});

test("a delivery-unknown answer must say why, and may not claim a turn", () => {
  assert.equal(
    parseCodingSessionLifecycleReceipt(
      receipt({ status: "turn_delivery_unknown", error: null }),
    ),
    null,
    "no error is a claim of delivery, not of not knowing",
  );
  assert.equal(
    parseCodingSessionLifecycleReceipt(
      receipt({
        status: "turn_delivery_unknown",
        error: { code: "  ", message: "lost" },
      }),
    ),
    null,
    "a blank code is not a code",
  );
  assert.equal(
    parseCodingSessionLifecycleReceipt(
      receipt({
        status: "turn_delivery_unknown",
        error: { code: "STEER_ACK_LOST", message: "lost" },
        turnId: "turn-running-7",
      }),
    ),
    null,
    "a turnId would claim the input joined a turn, which this status cannot say",
  );
});
