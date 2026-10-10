/**
 * SV-29 rewind on the desktop: the 44221 builder, every strict reader's
 * handling of the optional receipt `rewind`, what the dialog may offer, the
 * outcome fold, and the join that names who rewound which turns.
 */
import assert from "node:assert/strict";
import test from "node:test";

import { foldSessionCoordination } from "@/shared/coordination/sessionCoordinationFold";
import {
  hasStrictLifecycleCommandJson,
  hasStrictLifecycleCommandValues,
  hasStrictLifecycleReceiptJson,
  hasStrictLifecycleReceiptValues,
} from "@/shared/coordination/sessionCoordinationStrictJson";
import { parseCodingSessionLifecycleReceipt } from "./codingSessionIngressPayloads.ts";
import { buildCodingSessionRewindEvent } from "./codingSessionLifecycleCommand.ts";
import {
  codingSessionRewindPrefill,
  codingSessionRewoundStatusRow,
  countCodingSessionRewoundTurns,
  findCodingSessionRewindReceipt,
  foldCodingSessionRewindOutcome,
  isCodingSessionRewoundRow,
  joinCodingSessionRewindRecord,
  resolveCodingSessionRewindAvailability,
} from "./codingSessionRewind.ts";
import { codingSessionRewindBlockText } from "./codingSessionRewindRows.ts";

const CHANNEL = "05ef0ecf-745f-5fb8-b7ff-f9cba21e01c2";
const PROVIDER = "d4".repeat(32);
const PERSON = "a1".repeat(32);
const CHECKPOINT = "c1".repeat(32);
const PRE_REWIND = "c2".repeat(32);
const HEAD = "e".repeat(40);

function target(generation) {
  return {
    driver: "acp",
    instanceId: "inst-1",
    sessionId: "sess-1",
    generation,
  };
}

const REWIND = {
  checkpoint: CHECKPOINT,
  cutGeneration: 1,
  cutAfterSeq: 4,
  previousGeneration: 1,
  files: "restored",
  preRewindCheckpoint: PRE_REWIND,
  head: HEAD,
};

function rewindCommand({
  id = "04".repeat(32),
  commandId = "rewind-1",
  signer = PERSON,
  files = "restore",
  checkpoint = CHECKPOINT,
  from = 1,
} = {}) {
  const built = buildCodingSessionRewindEvent({
    channelId: CHANNEL,
    commandId,
    target: target(from),
    providerAuthorityPubkey: PROVIDER,
    checkpoint,
    files,
  });
  return { id, pubkey: signer, created_at: 1_800_000_100, sig: "", ...built };
}

const NO_REWIND = Symbol("no rewind key");

function receipt({
  id = "05".repeat(32),
  commandId = "rewind-1",
  status = "resumed",
  session = target(2),
  error = null,
  rewind = REWIND,
  signer = PROVIDER,
  extra = {},
} = {}) {
  const content = {
    schema: "buzz-coding-session-lifecycle-receipt/v1",
    commandId,
    status,
    session,
    error,
    ...(rewind === NO_REWIND ? {} : { rewind }),
    ...extra,
  };
  return {
    id,
    pubkey: signer,
    created_at: 1_800_000_110,
    kind: 44224,
    sig: "",
    tags: [
      ["h", CHANNEL],
      ["cslr-v", "cslr1-1"],
      ["csl-command", commandId],
      [
        "csl-key",
        `coding-session-lifecycle-receipt/v1|${commandId.length}:${commandId}`,
      ],
    ],
    content: JSON.stringify(content),
  };
}

test("session.rewind builds exactly five action keys, in wire order", () => {
  const event = rewindCommand();
  assert.equal(event.kind, 44221);
  assert.deepEqual(event.tags, [
    ["h", CHANNEL],
    ["csl-v", "csl1-1"],
    ["csl-command", "rewind-1"],
  ]);
  assert.equal(
    event.content,
    `{"schema":"buzz-coding-session-lifecycle-command/v1","commandId":"rewind-1","action":{"type":"session.rewind","session":{"driver":"acp","instanceId":"inst-1","sessionId":"sess-1","generation":1},"providerAuthorityPubkey":"${PROVIDER}","checkpoint":"${CHECKPOINT}","files":"restore"}}`,
  );
  const content = JSON.parse(event.content);
  assert.equal(hasStrictLifecycleCommandJson(event.content, content), true);
  assert.equal(hasStrictLifecycleCommandValues(content), true);
});

test("the builder refuses a checkpoint that is not an event id, or an open files value", () => {
  assert.throws(
    () => rewindCommand({ checkpoint: "C1".repeat(32) }),
    /checkpoint/,
  );
  assert.throws(() => rewindCommand({ files: "all" }), /files/);
});

test("the strict command reader refuses a sixth key and an unknown files value", () => {
  const content = JSON.parse(rewindCommand().content);
  const extra = { ...content, action: { ...content.action, through: 4 } };
  assert.equal(
    hasStrictLifecycleCommandJson(JSON.stringify(extra), extra),
    false,
  );
  const open = { ...content, action: { ...content.action, files: "all" } };
  assert.equal(hasStrictLifecycleCommandJson(JSON.stringify(open), open), true);
  assert.equal(hasStrictLifecycleCommandValues(open), false);
  const missing = { ...content, action: { ...content.action } };
  delete missing.action.files;
  assert.equal(
    hasStrictLifecycleCommandJson(JSON.stringify(missing), missing),
    false,
  );
});

test("every receipt reader accepts the optional rewind exactly, and refuses it malformed", () => {
  const strict = (event) => {
    const content = JSON.parse(event.content);
    return (
      hasStrictLifecycleReceiptJson(event.content, content) &&
      hasStrictLifecycleReceiptValues(content)
    );
  };
  const ingress = (event) => parseCodingSessionLifecycleReceipt(event.content);

  // Without the key, exactly as before.
  assert.equal(strict(receipt({ rewind: NO_REWIND })), true);
  assert.equal(ingress(receipt({ rewind: NO_REWIND }))?.rewind, undefined);

  // With it: both accept, and the ingress decoder carries it.
  assert.equal(strict(receipt()), true);
  assert.deepEqual(ingress(receipt())?.rewind, REWIND);
  const failed = receipt({
    status: "failed",
    session: null,
    error: {
      code: "REWIND_NOT_RESTARTED",
      message: "still remembers turns 2–3",
    },
    rewind: { ...REWIND, files: "restore_failed", head: null },
  });
  assert.equal(strict(failed), true);
  assert.equal(ingress(failed)?.rewind?.files, "restore_failed");

  const bad = [
    receipt({ rewind: { ...REWIND, extra: 1 } }),
    receipt({ rewind: { ...REWIND, files: "kept-ish" } }),
    receipt({ rewind: { ...REWIND, cutAfterSeq: -1 } }),
    receipt({ rewind: { ...REWIND, cutGeneration: 0 } }),
    receipt({ rewind: { ...REWIND, head: "zz" } }),
    receipt({ rewind: { ...REWIND, preRewindCheckpoint: undefined } }),
    receipt({ rewind: null }),
    // A rewind rides only a resume-shaped success or a failure.
    receipt({ status: "stopped" }),
    // Coupled as buzz-core couples it (WIRE-C3b-FINAL).
    receipt({ session: target(3) }),
    receipt({ rewind: { ...REWIND, files: "restore_failed" } }),
    receipt({ rewind: { ...REWIND, cutGeneration: 2 } }),
    receipt({
      status: "failed",
      session: null,
      error: { code: "TREE_BUSY", message: "busy" },
    }),
    // Any other unknown key is still a rejection.
    receipt({ rewind: NO_REWIND, extra: { cut: 4 } }),
  ];
  for (const event of bad) {
    assert.equal(strict(event), false, event.content);
    assert.equal(ingress(event), null, event.content);
  }
});

test("the coordination fold follows a rewind to generation 2 like a resume", () => {
  const create = {
    id: "01".repeat(32),
    pubkey: PERSON,
    created_at: 1_800_000_000,
    kind: 44221,
    tags: [
      ["h", CHANNEL],
      ["csl-v", "csl1-1"],
      ["csl-command", "create-1"],
    ],
    content: JSON.stringify({
      schema: "buzz-coding-session-lifecycle-command/v1",
      commandId: "create-1",
      action: {
        type: "session.create",
        projectRef: null,
        repoRef: null,
        providerInstanceRef: "provider-1",
        providerAuthorityPubkey: PROVIDER,
        model: null,
        title: null,
        initialTurn: null,
      },
    }),
  };
  const created = receipt({
    id: "02".repeat(32),
    commandId: "create-1",
    status: "created",
    session: target(1),
    rewind: NO_REWIND,
  });
  const metadata = (id, generation, status, at) => ({
    id,
    pubkey: PROVIDER,
    created_at: at,
    kind: 44223,
    tags: [
      ["h", CHANNEL],
      ["csm-v", "csm1-1"],
      ["cs-target", `coding-session/v1|3:acp6:inst-16:sess-11:${generation}`],
      ["csm-key", "unused-by-this-reader"],
    ],
    content: JSON.stringify({
      schema: "buzz-coding-session-metadata/v1",
      session: target(generation),
      projectRef: null,
      repoRef: null,
      title: "Rewind",
      agentRef: null,
      provider: "claude-primary",
      runtime: "claude",
      model: null,
      status,
      branch: null,
      capabilities: {
        threadTurnStart: true,
        threadTurnInterrupt: true,
        threadSteer: true,
        context: false,
        diff: false,
        plan: true,
      },
    }),
  });
  const result = foldSessionCoordination({
    now: 1_800_000_200,
    events: [
      create,
      created,
      metadata("03".repeat(32), 1, "disconnected", 1_800_000_105),
      rewindCommand(),
      receipt(),
      metadata("06".repeat(32), 2, "idle", 1_800_000_120),
    ],
    commissioners: [PERSON],
  });
  assert.equal(result.sessions.length, 1, JSON.stringify(result.ambiguities));
  const generations = result.sessions[0].generations;
  const second = generations.find((g) => g.current);
  assert.equal(second?.lifecycleCommandEventId, "04".repeat(32));
});

const checkpoint = (git, restorable) => ({
  eventId: CHECKPOINT,
  payload: { reason: "turn", git, restorable },
});
const GIT = { baseTree: "1".repeat(40), tree: "2".repeat(40) };
const free = {
  turnRunning: false,
  mayControl: true,
  inCurrentGeneration: true,
};

test("availability: a restorable git checkpoint offers both choices", () => {
  const offered = resolveCodingSessionRewindAvailability({
    ...free,
    checkpoint: checkpoint(GIT, true),
  });
  assert.deepEqual(offered, {
    chat: { enabled: true },
    files: { enabled: true },
    checkpointEventId: CHECKPOINT,
  });
});

test("availability: no git or no baseTree is chat only (ledger 371)", () => {
  for (const git of [null, { baseTree: null, tree: "2".repeat(40) }]) {
    const chatOnly = resolveCodingSessionRewindAvailability({
      ...free,
      checkpoint: checkpoint(git, true),
    });
    assert.deepEqual(chatOnly.chat, { enabled: true }, JSON.stringify(git));
    assert.deepEqual(chatOnly.files, { enabled: false, block: "no-git" });
    assert.equal(
      codingSessionRewindBlockText(chatOnly.files.block),
      "No git checkpoint for this turn",
    );
  }
});

test("availability: restorable false refuses both, as the provider does", () => {
  const older = resolveCodingSessionRewindAvailability({
    ...free,
    checkpoint: checkpoint(GIT, false),
  });
  assert.deepEqual(older.chat, {
    enabled: false,
    block: "provider-cannot-rewind",
  });
  assert.deepEqual(older.files, {
    enabled: false,
    block: "provider-cannot-rewind",
  });
  // An earlier rewind-capable build tied restorable to the baseline: the
  // provider still refuses it, so neither choice is offered, and the reason
  // names the checkpoint rather than claiming the build cannot rewind.
  for (const git of [null, { baseTree: null, tree: "2".repeat(40) }]) {
    const unrestorable = resolveCodingSessionRewindAvailability({
      ...free,
      checkpoint: checkpoint(git, false),
    });
    const block = { enabled: false, block: "checkpoint-not-restorable" };
    assert.deepEqual(unrestorable.chat, block, JSON.stringify(git));
    assert.deepEqual(unrestorable.files, block);
  }
  const none = resolveCodingSessionRewindAvailability({
    ...free,
    checkpoint: null,
  });
  assert.deepEqual(none.chat, { enabled: false, block: "no-checkpoint" });
});

test("availability: gates apply to both choices, in the provider's order", () => {
  const cp = checkpoint(GIT, true);
  const block = (input) =>
    resolveCodingSessionRewindAvailability({
      ...free,
      checkpoint: cp,
      ...input,
    }).chat;
  assert.deepEqual(block({ inCurrentGeneration: false, turnRunning: true }), {
    enabled: false,
    block: "outside-generation",
  });
  assert.deepEqual(block({ sessionClosed: true }), {
    enabled: false,
    block: "session-closed",
  });
  assert.deepEqual(block({ mayControl: null }), {
    enabled: false,
    block: "authority-unresolved",
  });
  assert.deepEqual(block({ mayControl: false }), {
    enabled: false,
    block: "not-controller",
  });
  assert.deepEqual(block({ turnRunning: true }), {
    enabled: false,
    block: "turn-running",
  });
});

test("outcome: refusals, the not-restarted cut, no memory, and seeded are distinct", () => {
  assert.deepEqual(
    foldCodingSessionRewindOutcome({ lifecycle: null, rewind: null }),
    { kind: "pending" },
  );
  assert.deepEqual(
    foldCodingSessionRewindOutcome({
      lifecycle: {
        state: "failed",
        commandId: "r",
        error: {
          code: "TREE_BUSY",
          message: "Seat lead is mid-turn in this tree (this provider only).",
        },
      },
      rewind: null,
    }),
    {
      kind: "refused",
      code: "TREE_BUSY",
      message: "Seat lead is mid-turn in this tree (this provider only).",
    },
  );
  assert.deepEqual(
    foldCodingSessionRewindOutcome({
      lifecycle: {
        state: "failed",
        commandId: "r",
        error: {
          code: "REWIND_NOT_RESTARTED",
          message: "still remembers turns 2–3",
        },
      },
      rewind: null,
      failedRewind: { ...REWIND, files: "restored" },
    }),
    {
      kind: "not-restarted",
      files: "restored",
      message: "still remembers turns 2–3",
    },
  );
  const none = foldCodingSessionRewindOutcome({
    lifecycle: {
      state: "resumed-without-context",
      commandId: "r",
      target: target(2),
      metadata: {},
      error: { code: "CONTEXT_NOT_RECOVERED", message: "projector failed" },
    },
    rewind: REWIND,
  });
  assert.equal(none.kind, "rewound");
  assert.equal(none.memory, "none");
  const seeded = foldCodingSessionRewindOutcome({
    lifecycle: {
      state: "awaiting-metadata",
      commandId: "r",
      target: target(2),
      malformedMetadataCount: 0,
    },
    rewind: REWIND,
  });
  assert.equal(seeded.kind, "rewound");
  assert.equal(seeded.memory, "seeded");
  assert.equal(seeded.rewind.head, HEAD);
});

test("the success receipt's rewind is found among a generation's raw events", () => {
  const events = [
    receipt({ signer: "ff".repeat(32), rewind: { ...REWIND, files: "kept" } }),
    receipt(),
  ];
  assert.deepEqual(
    findCodingSessionRewindReceipt(events, "rewind-1", PROVIDER),
    REWIND,
  );
  assert.equal(findCodingSessionRewindReceipt(events, "other", PROVIDER), null);
});

test("join: the provider's own success receipt names the signer of the rewind", () => {
  const record = joinCodingSessionRewindRecord({
    channelId: CHANNEL,
    providerAuthorityPubkey: PROVIDER,
    target: target(2),
    commands: [rewindCommand()],
    receipts: [receipt()],
  });
  assert.equal(record?.signerPubkey, PERSON);
  assert.equal(record?.requestedFiles, "restore");
  assert.equal(record?.memory, "seeded");
  assert.deepEqual(record?.rewind, REWIND);
});

test("join: a stranger's receipt, another checkpoint, or two rewinds prove nothing", () => {
  const join = (commands, receipts) =>
    joinCodingSessionRewindRecord({
      channelId: CHANNEL,
      providerAuthorityPubkey: PROVIDER,
      target: target(2),
      commands,
      receipts,
    });
  assert.equal(
    join([rewindCommand()], [receipt({ signer: "ee".repeat(32) })]),
    null,
  );
  assert.equal(
    join(
      [rewindCommand()],
      [receipt({ rewind: { ...REWIND, checkpoint: "c3".repeat(32) } })],
    ),
    null,
  );
  assert.equal(
    join(
      [rewindCommand()],
      [
        receipt({
          status: "failed",
          session: null,
          error: { code: "REWIND_NOT_RESTARTED", message: "x" },
        }),
      ],
    ),
    null,
  );
  assert.equal(
    join(
      [
        rewindCommand(),
        rewindCommand({ id: "06".repeat(32), commandId: "rewind-2" }),
      ],
      [receipt(), receipt({ id: "07".repeat(32), commandId: "rewind-2" })],
    ),
    null,
  );
});

const prompt = (seq, extra = {}) => ({
  id: `p${seq}`,
  type: "message",
  role: "user",
  text: "x",
  title: "",
  timestamp: "",
  sourceEventSeq: seq,
  ...extra,
});

test("count: prompts after the cut across the rewound generations, steers excluded", () => {
  const generations = [
    {
      generation: 1,
      items: [prompt(1), prompt(5), prompt(9, { steered: true }), prompt(12)],
    },
    { generation: 2, items: [prompt(1), prompt(3)] },
  ];
  assert.equal(
    countCodingSessionRewoundTurns({
      rewind: { cutGeneration: 1, cutAfterSeq: 4, previousGeneration: 2 },
      generations,
    }),
    4,
  );
  // A generation inside the cut that this view does not hold: no number.
  assert.equal(
    countCodingSessionRewoundTurns({
      rewind: { cutGeneration: 1, cutAfterSeq: 4, previousGeneration: 3 },
      generations,
    }),
    null,
  );
});

test("the dedicated session_rewound item becomes a Rewound row from its closed fields", () => {
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
  const row = codingSessionRewoundStatusRow(item);
  assert.deepEqual(row, {
    title: "Rewound",
    text: "Rewound to before this turn · files restored · new conversation seeded from the record",
  });
  assert.equal(
    codingSessionRewoundStatusRow({ ...item, files: "kept", memory: "none" })
      ?.text,
    "Rewound to before this turn · files kept · restarted with no memory",
  );
  // A malformed item is not read as a rewind.
  assert.equal(
    codingSessionRewoundStatusRow({ ...item, extra: 1 }),
    undefined,
    "an unknown key",
  );
  const { checkpoint: _checkpoint, ...missing } = item;
  assert.equal(
    codingSessionRewoundStatusRow(missing),
    undefined,
    "a key absent",
  );
  assert.equal(
    codingSessionRewoundStatusRow({ ...item, reason: "no_umbrella_context" }),
    undefined,
    "a reason beside seeded memory",
  );
  assert.ok(
    codingSessionRewoundStatusRow({
      ...item,
      memory: "none",
      reason: "no_umbrella_context",
    }),
  );
  assert.equal(
    codingSessionRewoundStatusRow({ ...item, memory: undefined }),
    undefined,
  );
  assert.equal(
    codingSessionRewoundStatusRow({ ...item, files: "restore_failed" }),
    undefined,
  );
  assert.equal(
    codingSessionRewoundStatusRow({
      kind: "status",
      status: "session_resumed",
    }),
    undefined,
  );
  const lifecycle = {
    id: "i",
    type: "lifecycle",
    renderClass: "status",
    timestamp: "",
    ...row,
  };
  assert.equal(isCodingSessionRewoundRow(lifecycle), true);
});

test("the prefill targets the new generation's composer, once per command", () => {
  const prefill = codingSessionRewindPrefill({
    commandId: "rewind-1",
    channelId: CHANNEL,
    target: target(2),
    text: "Try again",
  });
  assert.equal(prefill.id, "rewind-prefill:rewind-1");
  assert.equal(prefill.targetKey, "coding-session/v1|3:acp6:inst-16:sess-11:2");
  assert.equal(prefill.text, "Try again");
});
