/**
 * SV-35: the pending fold, and the wire lines it reads (WIRE-C3a).
 *
 * The fold must never show the request as the running model: pending until
 * the one terminal receipt, and after `model_applied` only a 44223 observed
 * after it names the model.
 */
import assert from "node:assert/strict";
import test from "node:test";

import {
  buildCodingSessionModelSetEvent,
  CODING_SESSION_MODEL_SET_ACTION_TYPE,
  MAX_CODING_SESSION_MODEL_SELECTION_BYTES,
} from "./codingSessionCommand.ts";
import {
  codingSessionReceiptSemanticKey,
  isCodingSessionTurnReceiptStatus,
  parseBeekeeperCodingSessionMetadata,
  parseCodingSessionLifecycleReceipt,
} from "./codingSessionIngressPayloads.ts";
import {
  foldCodingSessionModelSwitch,
  isCodingSessionModelSwitchInFlight,
} from "./codingSessionModelSwitch.ts";
import { parseCodingSessionProviderCatalog } from "./codingSessionProviderCatalog.ts";

const SESSION = {
  driver: "claude-agent-acp",
  instanceId: "instance-1",
  sessionId: "session-1",
  generation: 3,
};
const REQUEST = { commandId: "csc-model-1", selection: "sonnet[high]" };

function receipt(status, order, createdAt = 100, error = null) {
  return {
    eventId: `r-${order}`,
    commandId: REQUEST.commandId,
    status,
    error,
    createdAt,
    order,
  };
}

function meta(model, order, createdAt = 100) {
  return { eventId: `m-${order}`, model, createdAt, order };
}

test("no request is idle; a signed request with no answer is pending", () => {
  assert.deepEqual(
    foldCodingSessionModelSwitch({ request: null, receipts: [], metadata: [] }),
    { kind: "idle" },
  );
  const pending = foldCodingSessionModelSwitch({
    request: REQUEST,
    receipts: [],
    // Metadata alone never settles a switch: it may be the turn-end republish.
    metadata: [meta("sonnet[high]", 1)],
  });
  assert.deepEqual(pending, { kind: "pending", requested: "sonnet[high]" });
  assert.equal(isCodingSessionModelSwitchInFlight(pending), true);
});

test("receipts under another command id are ignored", () => {
  const state = foldCodingSessionModelSwitch({
    request: REQUEST,
    receipts: [{ ...receipt("model_applied", 1), commandId: "csc-other" }],
    metadata: [meta("sonnet[high]", 2)],
  });
  assert.equal(state.kind, "pending");
});

test("model_applied with no later 44223 is accepted, not applied", () => {
  const state = foldCodingSessionModelSwitch({
    request: REQUEST,
    receipts: [receipt("model_applied", 2)],
    // Observed before the receipt: the old model, never read as the new one.
    metadata: [meta("opus", 1)],
  });
  assert.deepEqual(state, { kind: "accepted", requested: "sonnet[high]" });
  assert.equal(isCodingSessionModelSwitchInFlight(state), true);
});

test("a 44223 signed before the receipt does not count even if seen after", () => {
  const state = foldCodingSessionModelSwitch({
    request: REQUEST,
    receipts: [receipt("model_applied", 1, 200)],
    metadata: [meta("opus", 2, 199)],
  });
  assert.equal(state.kind, "accepted");
});

test("item → receipt → metadata: the model is metadata's and matches", () => {
  const state = foldCodingSessionModelSwitch({
    request: REQUEST,
    receipts: [receipt("model_applied", 1)],
    metadata: [meta("sonnet[high]", 2)],
  });
  assert.deepEqual(state, {
    kind: "applied",
    requested: "sonnet[high]",
    running: "sonnet[high]",
    matches: true,
  });
  assert.equal(isCodingSessionModelSwitchInFlight(state), false);
});

test("asked X · running Y: metadata names another model", () => {
  const state = foldCodingSessionModelSwitch({
    request: REQUEST,
    receipts: [receipt("model_applied", 1)],
    // The adapter dropped an effort it does not offer for this model.
    metadata: [meta("sonnet", 2), meta("sonnet", 3)],
  });
  assert.equal(state.kind, "applied");
  assert.equal(state.running, "sonnet");
  assert.equal(state.matches, false);
});

for (const code of [
  "MODEL_SWITCH_UNSUPPORTED",
  "MODEL_NOT_OFFERED",
  "MODEL_SWITCH_FAILED",
  "UNAUTHORIZED_OPERATOR",
]) {
  test(`turn_refused ${code} reverts: refused, with the provider's code`, () => {
    const state = foldCodingSessionModelSwitch({
      request: REQUEST,
      receipts: [
        receipt("turn_refused", 1, 100, { code, message: "said why" }),
      ],
      metadata: [meta("opus", 2)],
    });
    assert.deepEqual(state, {
      kind: "refused",
      requested: "sonnet[high]",
      outcome: "refused",
      code,
      message: "said why",
    });
  });
}

test("turn_dropped is a refusal of its own outcome", () => {
  const state = foldCodingSessionModelSwitch({
    request: REQUEST,
    receipts: [
      receipt("turn_dropped", 1, 100, {
        code: "QUEUE_FULL",
        message: "mailbox full",
      }),
    ],
    metadata: [],
  });
  assert.equal(state.kind, "refused");
  assert.equal(state.outcome, "dropped");
});

test("the 44220 is built like an interrupt with exactly type and selection", () => {
  const event = buildCodingSessionModelSetEvent({
    channelId: "channel-1",
    commandId: REQUEST.commandId,
    target: SESSION,
    selection: "opus[1m][high]",
  });
  assert.equal(event.kind, 44220);
  const payload = JSON.parse(event.content);
  assert.deepEqual(payload.action, {
    type: CODING_SESSION_MODEL_SET_ACTION_TYPE,
    selection: "opus[1m][high]",
  });
  assert.deepEqual(
    event.tags.map(([name]) => name),
    ["h", "cs-v", "cs-target"],
  );
});

test("the builder refuses what core's validate refuses", () => {
  const build = (selection) =>
    buildCodingSessionModelSetEvent({
      channelId: "channel-1",
      commandId: REQUEST.commandId,
      target: SESSION,
      selection,
    });
  assert.throws(() => build("   "), /must not be empty/);
  assert.throws(
    () => build("a".repeat(MAX_CODING_SESSION_MODEL_SELECTION_BYTES + 1)),
    /exceeds/,
  );
  assert.throws(() => build("opus\n[high]"), /control characters/);
  assert.throws(() => build("opus\u0085"), /control characters/);
  assert.doesNotThrow(() =>
    build("a".repeat(MAX_CODING_SESSION_MODEL_SELECTION_BYTES)),
  );
});

test("model_applied decodes as a five-key turn stage with a per-stage key", () => {
  assert.equal(isCodingSessionTurnReceiptStatus("model_applied"), true);
  const content = {
    schema: "buzz-coding-session-lifecycle-receipt/v1",
    commandId: REQUEST.commandId,
    status: "model_applied",
    session: SESSION,
    error: null,
  };
  const parsed = parseCodingSessionLifecycleReceipt(JSON.stringify(content));
  assert.deepEqual(parsed, content);
  assert.equal(
    parseCodingSessionLifecycleReceipt(
      JSON.stringify({ ...content, turnId: "t-1" }),
    ),
    null,
  );
  assert.equal(
    parseCodingSessionLifecycleReceipt(
      JSON.stringify({ ...content, error: { code: "X", message: "y" } }),
    ),
    null,
  );
  assert.notEqual(
    codingSessionReceiptSemanticKey(REQUEST.commandId, "model_applied"),
    codingSessionReceiptSemanticKey(REQUEST.commandId, "turn_refused"),
  );
});

function metadataContent(capabilities) {
  return JSON.stringify({
    schema: "buzz-coding-session-metadata/v1",
    session: SESSION,
    projectRef: null,
    repoRef: null,
    title: null,
    agentRef: null,
    provider: null,
    runtime: "claude",
    model: "opus",
    status: "idle",
    branch: null,
    capabilities: {
      threadTurnStart: true,
      threadTurnInterrupt: true,
      threadSteer: false,
      context: false,
      diff: false,
      plan: true,
      promptImage: false,
      ...capabilities,
    },
  });
}

test("44223 decodes with and without modelSwitch; absent reads as no switch", () => {
  const without = parseBeekeeperCodingSessionMetadata(metadataContent({}));
  assert.ok(without);
  assert.equal(Object.hasOwn(without.capabilities, "modelSwitch"), false);
  const withSwitch = parseBeekeeperCodingSessionMetadata(
    metadataContent({ modelSwitch: true }),
  );
  assert.equal(withSwitch?.capabilities.modelSwitch, true);
  const explicitFalse = parseBeekeeperCodingSessionMetadata(
    metadataContent({ modelSwitch: false }),
  );
  assert.equal(explicitFalse?.capabilities.modelSwitch, undefined);
  assert.equal(
    parseBeekeeperCodingSessionMetadata(
      metadataContent({ modelSwitch: "yes" }),
    ),
    null,
  );
});

function catalogContent(capabilities) {
  return JSON.stringify({
    schema: "buzz-coding-session-provider-catalog/v1",
    revision: 1,
    providers: [
      {
        providerInstanceRef: "claude",
        driver: "claude-agent-acp",
        runtime: "claude",
        defaultModel: "opus",
        allowedModels: ["opus", "sonnet"],
        capabilities: {
          threadTurnStart: true,
          threadTurnInterrupt: true,
          threadSteer: false,
          context: false,
          diff: false,
          plan: true,
          promptImage: false,
          ...capabilities,
        },
      },
    ],
  });
}

test("44222 still round-trips without modelSwitch and accepts it trailing", () => {
  assert.ok(parseCodingSessionProviderCatalog(catalogContent({})));
  const withSwitch = parseCodingSessionProviderCatalog(
    catalogContent({ modelSwitch: true }),
  );
  assert.equal(withSwitch?.providers[0].capabilities.modelSwitch, true);
});
