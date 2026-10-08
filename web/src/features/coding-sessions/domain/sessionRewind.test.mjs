import assert from "node:assert/strict";
import { test } from "node:test";
import { parseCodingSessionLifecycleReceipt } from "./ingressPayloads.ts";
import { parseCodingSessionLifecycleCommand } from "./lifecycleCommand.ts";
import { CHANNEL_ID, newSigner, sign, target } from "./testFixtures.mjs";
import { buildNonToolItem } from "./transcriptItems.ts";

const CHECKPOINT = "c1".repeat(32);
const REWIND = {
  checkpoint: CHECKPOINT,
  cutGeneration: 1,
  cutAfterSeq: 4,
  previousGeneration: 1,
  files: "restored",
  preRewindCheckpoint: "c2".repeat(32),
  head: "e".repeat(40),
};

function receipt(overrides) {
  return JSON.stringify({
    schema: "buzz-coding-session-lifecycle-receipt/v1",
    commandId: "rewind-1",
    status: "resumed",
    session: target({ generation: 2 }),
    error: null,
    rewind: REWIND,
    ...overrides,
  });
}

test("a rewind's resumed receipt decodes with its seven-key rewind", () => {
  const decoded = parseCodingSessionLifecycleReceipt(receipt({}));
  assert.equal(decoded?.status, "resumed");
  assert.deepEqual(decoded?.rewind, REWIND);
  // Without the key it is an ordinary resume.
  const plain = JSON.parse(receipt({}));
  delete plain.rewind;
  assert.equal(
    parseCodingSessionLifecycleReceipt(JSON.stringify(plain))?.rewind,
    undefined,
  );
});

test("a rewind key is refused off its rule", () => {
  const refused = [
    // null is not absent; an unknown or missing key is not a rewind.
    { rewind: null },
    { rewind: { ...REWIND, extra: 1 } },
    { rewind: { ...REWIND, head: undefined } },
    // N+1 must follow previousGeneration.
    { session: target({ generation: 3 }) },
    // restore_failed rides only a failed receipt.
    { rewind: { ...REWIND, files: "restore_failed" } },
    // cutGeneration never exceeds previousGeneration.
    { rewind: { ...REWIND, cutGeneration: 2 } },
    // Only on resumed / resumed_without_context / failed REWIND_NOT_RESTARTED.
    { status: "stopped" },
    {
      status: "failed",
      session: null,
      error: { code: "SESSION_BUSY", message: "busy" },
    },
  ];
  for (const overrides of refused) {
    assert.equal(
      parseCodingSessionLifecycleReceipt(receipt(overrides)),
      null,
      JSON.stringify(overrides),
    );
  }
  const notRestarted = parseCodingSessionLifecycleReceipt(
    receipt({
      status: "failed",
      session: null,
      error: { code: "REWIND_NOT_RESTARTED", message: "needs a Restart" },
      rewind: { ...REWIND, files: "restore_failed" },
    }),
  );
  assert.equal(notRestarted?.rewind?.files, "restore_failed");
  const withoutContext = parseCodingSessionLifecycleReceipt(
    receipt({
      status: "resumed_without_context",
      error: {
        code: "CONTEXT_NOT_RECOVERED",
        message: "restarted with no memory",
      },
      rewind: { ...REWIND, files: "kept" },
    }),
  );
  assert.equal(withoutContext?.rewind?.files, "kept");
});

function command(action) {
  const signer = newSigner();
  return sign(signer, {
    kind: 44221,
    content: JSON.stringify({
      schema: "buzz-coding-session-lifecycle-command/v1",
      commandId: "rewind-1",
      action,
    }),
    tags: [
      ["h", CHANNEL_ID],
      ["csl-v", "csl1-1"],
      ["csl-command", "rewind-1"],
    ],
  });
}

test("session.rewind and session.restart decode beside session.resume", () => {
  const base = {
    session: target(),
    providerAuthorityPubkey: "d4".repeat(32),
  };
  const rewind = parseCodingSessionLifecycleCommand(
    command({
      type: "session.rewind",
      ...base,
      checkpoint: CHECKPOINT,
      files: "keep",
    }),
  );
  assert.equal(rewind?.action, "rewind");
  assert.deepEqual(rewind?.rewind, { checkpoint: CHECKPOINT, files: "keep" });
  assert.deepEqual(rewind?.previousTarget, target());
  assert.equal(
    parseCodingSessionLifecycleCommand(
      command({ type: "session.restart", ...base }),
    )?.action,
    "restart",
  );
  for (const bad of [
    { type: "session.rewind", ...base, checkpoint: CHECKPOINT },
    { type: "session.rewind", ...base, checkpoint: "AB", files: "keep" },
    { type: "session.rewind", ...base, checkpoint: CHECKPOINT, files: "all" },
    {
      type: "session.rewind",
      ...base,
      checkpoint: CHECKPOINT,
      files: "keep",
      extra: 1,
    },
  ]) {
    assert.equal(
      parseCodingSessionLifecycleCommand(command(bad)),
      null,
      JSON.stringify(bad),
    );
  }
});

test("the session_rewound item renders the provider's own words", () => {
  const identity = {
    id: "row-1",
    blockKey: "block-1",
    targetKey: "target",
    turnId: null,
    timestamp: 1,
    eventSeq: 1,
    eventId: "e".repeat(64),
  };
  const item = {
    kind: "status",
    status: "session_rewound",
    commandId: "rewind-1",
    checkpoint: CHECKPOINT,
    cutGeneration: 1,
    cutAfterSeq: 4,
    previousGeneration: 1,
    files: "restored",
    memory: "seeded",
  };
  const row = buildNonToolItem(item, identity);
  assert.equal(row.title, "Rewound");
  assert.equal(
    row.text,
    "Rewound to before this turn · files restored · new conversation seeded from the record",
  );
  assert.equal(
    buildNonToolItem({ ...item, files: "kept", memory: "none" }, identity).text,
    "Rewound to before this turn · files kept · restarted with no memory",
  );
});
