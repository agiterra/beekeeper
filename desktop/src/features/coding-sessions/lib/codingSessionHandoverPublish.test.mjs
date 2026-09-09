import assert from "node:assert/strict";
import test from "node:test";

import {
  claimCodingSessionHandover,
  continueCodingSessionHandover,
  renderCodingSessionCheckpointTurn,
  resolveCheckpointPatch,
} from "./codingSessionHandoverPublish.ts";
import { decodeCodingSessionHandoverEvent } from "./codingSessionHandoverWire.ts";

const CHANNEL = "98610076-0cb7-4d5e-9f82-5f4ad7c5a723";
const SESSION = "683d55b3-d34e-410d-874a-55f9082d2631";
const GENESIS = "ab".repeat(32);
const VIEWER = "bb".repeat(32);
const BODY = "2b".repeat(32);
const CLAIM_EVENT = "c1".repeat(32);
const CHECKPOINT = "0c".repeat(32);

function checkpointBody(overrides = {}) {
  return {
    prevCheckpointRef: null,
    task: "Land the panel",
    assignmentRefs: [],
    decisions: [
      { eventId: "cd".repeat(32), summary: "Reconstruct, not native" },
    ],
    revision: {
      repoRef: null,
      baseSha: null,
      headSha: "9".repeat(40),
      branch: "work/handover",
      dirty: true,
      preserved: "partial",
      ...(overrides.revision ?? {}),
    },
    artifacts: [
      {
        kind: "wip-ref",
        repoRef: "30617:aa/beekeeper",
        ref: "refs/heads/wip/builder/abc12345",
        sha: "9".repeat(40),
      },
    ],
    tests: [{ name: "unit", command: "pnpm test", outcome: "failed" }],
    unresolved: ["Whether the fence covers siblings"],
    nextAction: "Mount the panel",
    missing: ["editor scratch buffer"],
    ...overrides,
  };
}

/** A signer that returns a well-formed event; ids differ per call. */
function fakeSigner() {
  let seq = 0;
  return async (input) => {
    seq += 1;
    return {
      id: seq.toString(16).padStart(2, "0").repeat(32),
      pubkey: VIEWER,
      created_at: 1_800_000_000 + seq,
      kind: input.kind,
      tags: input.tags,
      content: input.content,
      sig: "0".repeat(128),
    };
  };
}

function deps(overrides = {}) {
  const published = [];
  const base = {
    published,
    signer: fakeSigner(),
    publisher: {
      async publishEvent(event) {
        published.push(event);
        return event;
      },
    },
    publishTransition: async () => ({ id: CLAIM_EVENT }),
    fetchFold: async () => ({ acceptedHead: { eventId: CLAIM_EVENT, seq: 2 } }),
    prepareCheckout: async () => ({
      branch: "handover/683d55b3",
      checkedOutSha: "9".repeat(40),
      recovered: ["wip-ref refs/heads/wip/builder/abc12345 at 9999"],
      missing: [],
    }),
    publishCreate: async () => ({ eventId: "ee".repeat(32), kind: 44221 }),
    stageCreateHint: async () => ({}),
    clearCreateHint: async () => ({}),
    fetchEvents: async (filter) =>
      filter.kinds?.[0] === 44223 ? [metadataEvent()] : [createdReceipt()],
    wait: async () => {},
    settleAttempts: 2,
  };
  return { ...base, ...overrides, published };
}

let lastCommandId = null;

/** The reconstructed execution's own first metadata: where it is running. */
function metadataEvent(overrides = {}) {
  return {
    id: "cc".repeat(32),
    pubkey: BODY,
    created_at: 1_800_000_950,
    kind: 44223,
    tags: [
      ["h", CHANNEL],
      ["csm-v", "csm1-1"],
      ["cs-target", "unused-by-this-reader"],
      ["csm-key", "unused-by-this-reader"],
    ],
    content: JSON.stringify({
      schema: "buzz-coding-session-metadata/v1",
      session: {
        driver: "acp",
        instanceId: "instance-b",
        sessionId: "session-b",
        generation: 1,
      },
      projectRef: null,
      repoRef: null,
      title: null,
      agentRef: null,
      provider: "acp",
      runtime: "acp",
      model: null,
      status: "running",
      branch: "handover/683d55b3",
      capabilities: {
        threadTurnStart: true,
        threadTurnInterrupt: true,
        threadSteer: true,
        context: false,
        diff: false,
        plan: true,
      },
      observedCommit: "9".repeat(40),
      dirty: true,
      relayReachable: null,
      verifiedAt: null,
      ...overrides,
    }),
    sig: "0".repeat(128),
  };
}

function createdReceipt() {
  return {
    id: "ff".repeat(32),
    pubkey: BODY,
    created_at: 1_800_000_900,
    kind: 44224,
    tags: [
      ["h", CHANNEL],
      ["cslr-v", "cslr1-1"],
      ["csl-command", lastCommandId],
      ["csl-key", "unused-by-this-reader"],
    ],
    content: JSON.stringify({
      schema: "buzz-coding-session-lifecycle-receipt/v1",
      commandId: lastCommandId,
      status: "created",
      session: {
        driver: "acp",
        instanceId: "instance-b",
        sessionId: "session-b",
        generation: 1,
      },
      error: null,
    }),
    sig: "0".repeat(128),
  };
}

function continueInput(overrides = {}) {
  return {
    channelId: CHANNEL,
    sessionRef: SESSION,
    genesisRef: GENESIS,
    viewerPubkey: VIEWER,
    bodyPubkey: BODY,
    providerInstanceRef: "instance-b",
    repoRef: "30617:aa/beekeeper",
    projectRef: null,
    title: "Handover",
    model: null,
    checkpoint: { body: checkpointBody(), authorLabel: "Ada" },
    checkpointRef: CHECKPOINT,
    checkout: {
      cwd: "/tmp/checkout",
      wipRef: "refs/heads/wip/builder/abc12345",
      sha: "9".repeat(40),
      sessionRef: SESSION,
    },
    declaredMissing: ["editor scratch buffer"],
    ...overrides,
  };
}

async function runContinue(dependencies, overrides = {}) {
  const captured = { ...dependencies };
  captured.publishCreate = async (input) => {
    lastCommandId = input.commandId;
    return dependencies.publishCreate(input);
  };
  return continueCodingSessionHandover(continueInput(overrides), captured);
}

test("the claim is published and settled against the relay's own head", async () => {
  const dependencies = deps();
  const claim = await claimCodingSessionHandover(
    {
      channelId: CHANNEL,
      genesisRef: GENESIS,
      claimantPubkey: VIEWER,
      bodyPubkey: BODY,
    },
    dependencies,
  );
  assert.equal(claim.acceptedEventId, CLAIM_EVENT);
});

test("a lost race says another link won, not that the relay went quiet", async () => {
  const dependencies = deps({
    fetchFold: async () => ({
      accepted: new Map(),
      acceptedHead: { eventId: "aa".repeat(32), seq: 3 },
    }),
  });
  await assert.rejects(
    claimCodingSessionHandover(
      {
        channelId: CHANNEL,
        genesisRef: GENESIS,
        claimantPubkey: VIEWER,
        bodyPubkey: BODY,
      },
      dependencies,
    ),
    (error) => {
      assert.match(error.message, /Another link won this chain first/);
      assert.match(error.message, new RegExp("aa".repeat(32)));
      assert.match(error.message, /seq 3/);
      assert.doesNotMatch(error.message, /accepted no receipt/);
      return true;
    },
  );
});

test("a head that never moved is the other refusal, in its own words", async () => {
  const dependencies = deps({
    fetchFold: async () => ({ accepted: new Map(), acceptedHead: null }),
  });
  await assert.rejects(
    claimCodingSessionHandover(
      {
        channelId: CHANNEL,
        genesisRef: GENESIS,
        claimantPubkey: VIEWER,
        bodyPubkey: BODY,
      },
      dependencies,
    ),
    /accepted no receipt/,
  );
});

test("the whole flow claims, fetches, creates and then says what came across", async () => {
  const dependencies = deps();
  const result = await runContinue(dependencies);
  assert.equal(result.ok, true, result.ok ? "" : result.reason);
  assert.equal(result.progress.claimEventId, CLAIM_EVENT);
  assert.equal(result.progress.target.sessionId, "session-b");

  const continuation = dependencies.published.at(-1);
  const decoded = decodeCodingSessionHandoverEvent({
    event: continuation,
    channelRef: CHANNEL,
    sessionRef: SESSION,
    genesisRef: GENESIS,
  });
  assert.equal(decoded.ok, true, decoded.ok ? "" : decoded.reason);
  assert.equal(decoded.value.type, "continuation");
  assert.equal(decoded.value.body.claimRef, CLAIM_EVENT);
  assert.equal(
    decoded.value.body.mode,
    "reconstructed",
    "the desktop never labels a new execution as a native resume",
  );
  assert.equal(decoded.value.body.checkpointRef, CHECKPOINT);
  assert.deepEqual(decoded.value.body.recovered, [
    "wip-ref refs/heads/wip/builder/abc12345 at 9999",
  ]);
  assert.deepEqual(decoded.value.body.missing, ["editor scratch buffer"]);
});

test("a refused claim stops the flow before anything is fetched or created", async () => {
  let fetched = false;
  let created = false;
  const dependencies = deps({
    publishTransition: async () => {
      throw new Error("StaleHead: the chain moved");
    },
    prepareCheckout: async () => {
      fetched = true;
      throw new Error("unreachable");
    },
    publishCreate: async () => {
      created = true;
      return { eventId: "0", kind: 44221 };
    },
  });
  const result = await runContinue(dependencies);
  assert.equal(result.ok, false);
  assert.equal(result.step, "claim");
  assert.match(result.reason, /StaleHead/);
  assert.equal(fetched, false);
  assert.equal(created, false);
  assert.equal(result.progress.claimEventId, null);
});

test("a checkout that refused the patch is carried into the continuation", async () => {
  const dependencies = deps({
    prepareCheckout: async () => ({
      branch: "handover/683d55b3",
      checkedOutSha: "9".repeat(40),
      recovered: ["wip-ref refs/heads/wip/builder/abc12345 at 9999"],
      missing: ["the 4096-byte patch did not apply to keep.txt"],
    }),
  });
  const result = await runContinue(dependencies);
  assert.equal(result.ok, true);
  const decoded = decodeCodingSessionHandoverEvent({
    event: dependencies.published.at(-1),
    channelRef: CHANNEL,
    sessionRef: SESSION,
    genesisRef: GENESIS,
  });
  assert.deepEqual(decoded.value.body.missing, [
    "editor scratch buffer",
    "the 4096-byte patch did not apply to keep.txt",
  ]);
});

test("a create nobody answered is reported with the claim that did land", async () => {
  const dependencies = deps({ fetchEvents: async () => [] });
  const result = await runContinue(dependencies);
  assert.equal(result.ok, false);
  assert.equal(result.step, "created");
  assert.equal(
    result.progress.claimEventId,
    CLAIM_EVENT,
    "a claim that landed is a fact about the session even when the rest failed",
  );
  assert.equal(result.progress.continuationEventId, null);
});

test("a checkpoint with no artifacts still reconstructs, and says nothing came across", async () => {
  const dependencies = deps();
  const result = await runContinue(dependencies, { checkout: null });
  assert.equal(result.ok, true);
  const decoded = decodeCodingSessionHandoverEvent({
    event: dependencies.published.at(-1),
    channelRef: CHANNEL,
    sessionRef: SESSION,
    genesisRef: GENESIS,
  });
  assert.deepEqual(decoded.value.body.recovered, []);
  assert.ok(
    decoded.value.body.missing.includes(
      "the checkpoint named no artifact to recover",
    ),
    decoded.value.body.missing.join(" | "),
  );
});

test("the rendered turn carries the author's words and both disclosures", () => {
  const turn = renderCodingSessionCheckpointTurn({
    body: checkpointBody(),
    authorLabel: "Bea",
    recovered: ["wip-ref refs/heads/wip/builder/abc12345 at 9999"],
    missing: ["the patch did not apply"],
  });
  assert.match(turn, /handed over by Bea/);
  assert.match(turn, /Task: Land the panel/);
  assert.match(turn, /Reconstruct, not native/);
  assert.match(turn, /Uncommitted work preserved: partial/);
  assert.match(turn, /unit: failed/);
  assert.match(turn, /NOT recovered:/);
  assert.match(turn, /- Not all uncommitted work was preserved/);
  assert.match(turn, /- the patch did not apply/);
  assert.match(turn, /- editor scratch buffer/);
  assert.match(turn, /Next action: Mount the panel/);
});

test("a fully preserved checkpoint claims nothing it cannot support", () => {
  const turn = renderCodingSessionCheckpointTurn({
    body: checkpointBody({
      revision: { preserved: "all" },
      missing: [],
    }),
    authorLabel: "Bea",
  });
  assert.doesNotMatch(turn, /Not all uncommitted work was preserved/);
});

// ---------------------------------------------------------------------------
// The checkpoint's uncommitted bytes: fetched, verified, or disclosed.
// ---------------------------------------------------------------------------

const AUTHOR = "aa".repeat(32);
const PATCH_EVENT_ID = "ab".repeat(32);
const BASE_SHA = "1a".repeat(20);
const PATCH_TEXT =
  "diff --git a/keep.txt b/keep.txt\n--- a/keep.txt\n+++ b/keep.txt\n";

function patchArtifact(overrides = {}) {
  return {
    kind: "patch",
    repoRef: "30617:aa/beekeeper",
    eventId: PATCH_EVENT_ID,
    baseSha: BASE_SHA,
    bytes: PATCH_TEXT.length,
    ...overrides,
  };
}

function patchEvent(overrides = {}) {
  return {
    id: PATCH_EVENT_ID,
    pubkey: AUTHOR,
    created_at: 1_800_000_100,
    kind: 1617,
    tags: [["parent-commit", BASE_SHA]],
    content: PATCH_TEXT,
    sig: "0".repeat(128),
    ...overrides,
  };
}

test("the checkpoint author's own patch is fetched by id and applied", async () => {
  const filters = [];
  const patch = await resolveCheckpointPatch(
    { artifacts: [patchArtifact()], checkpointAuthor: AUTHOR },
    {
      fetchEvents: async (filter) => {
        filters.push(filter);
        return [patchEvent()];
      },
    },
  );
  assert.deepEqual(filters, [
    { ids: [PATCH_EVENT_ID], kinds: [1617], limit: 1 },
  ]);
  assert.equal(patch.patchText, PATCH_TEXT);
  assert.equal(patch.baseSha, BASE_SHA);
  assert.deepEqual(patch.missing, []);
});

test("a patch signed by anybody but the checkpoint's author is refused by name", async () => {
  const patch = await resolveCheckpointPatch(
    { artifacts: [patchArtifact()], checkpointAuthor: AUTHOR },
    { fetchEvents: async () => [patchEvent({ pubkey: "cc".repeat(32) })] },
  );
  assert.equal(patch.patchText, null);
  assert.equal(patch.missing.length, 1);
  assert.match(patch.missing[0], /was signed by cccc/);
  assert.match(patch.missing[0], /not applied/);
});

test("a patch naming a different base than the checkpoint is refused", async () => {
  const patch = await resolveCheckpointPatch(
    { artifacts: [patchArtifact()], checkpointAuthor: AUTHOR },
    {
      fetchEvents: async () => [
        patchEvent({ tags: [["parent-commit", "9".repeat(40)]] }),
      ],
    },
  );
  assert.equal(patch.patchText, null);
  assert.match(patch.missing[0], /names base 9{40}, but the checkpoint says/);
});

test("a patch event with no base tag is still applied — git checks it", async () => {
  const patch = await resolveCheckpointPatch(
    { artifacts: [patchArtifact()], checkpointAuthor: AUTHOR },
    { fetchEvents: async () => [patchEvent({ tags: [] })] },
  );
  assert.equal(patch.patchText, PATCH_TEXT);
  assert.deepEqual(patch.missing, []);
});

test("a patch the relay does not hold, or a read that failed, is disclosed", async () => {
  const absent = await resolveCheckpointPatch(
    { artifacts: [patchArtifact()], checkpointAuthor: AUTHOR },
    { fetchEvents: async () => [] },
  );
  assert.equal(absent.patchText, null);
  assert.match(absent.missing[0], /was not on the relay/);

  const failed = await resolveCheckpointPatch(
    { artifacts: [patchArtifact()], checkpointAuthor: AUTHOR },
    {
      fetchEvents: async () => {
        throw new Error("relay refused the read");
      },
    },
  );
  assert.equal(failed.patchText, null);
  assert.match(failed.missing[0], /relay refused the read/);
});

test("a blob artifact names its hash rather than pretending it came across", async () => {
  const patch = await resolveCheckpointPatch(
    {
      artifacts: [
        {
          kind: "blob",
          repoRef: "30617:aa/beekeeper",
          hash: "b8".repeat(32),
          baseSha: BASE_SHA,
          bytes: 131_072,
        },
      ],
      checkpointAuthor: AUTHOR,
    },
    {
      fetchEvents: async () => {
        throw new Error("no fetch should happen for a blob");
      },
    },
  );
  assert.equal(patch.patchText, null);
  assert.equal(patch.missing.length, 1);
  assert.match(patch.missing[0], new RegExp(`blob ${"b8".repeat(32)}`));
  assert.match(patch.missing[0], /bee sessions handover continue/);
});

test("the continue flow hands the resolved patch to the native checkout", async () => {
  const requests = [];
  const dependencies = deps({
    fetchEvents: async (filter) =>
      filter.kinds?.[0] === 1617 ? [patchEvent()] : [createdReceipt()],
    prepareCheckout: async (request) => {
      requests.push(request);
      return {
        branch: "handover/683d55b3",
        checkedOutSha: "9".repeat(40),
        recovered: [
          "wip-ref refs/heads/wip/builder/abc12345 at 9999",
          "patch applied (70 bytes)",
        ],
        missing: [],
      };
    },
  });
  const result = await runContinue(dependencies, {
    artifacts: [patchArtifact()],
    checkpointAuthor: AUTHOR,
  });
  assert.equal(result.ok, true, result.ok ? "" : result.reason);
  assert.equal(requests.length, 1);
  assert.equal(requests[0].patchText, PATCH_TEXT);
  assert.equal(requests[0].baseSha, BASE_SHA);

  const decoded = decodeCodingSessionHandoverEvent({
    event: dependencies.published.at(-1),
    channelRef: CHANNEL,
    sessionRef: SESSION,
    genesisRef: GENESIS,
  });
  assert.ok(
    decoded.value.body.recovered.includes("patch applied (70 bytes)"),
    "what the checkout reported is what the continuation says",
  );
});

test("an unresolvable patch still reconstructs, and the continuation says why", async () => {
  const dependencies = deps({
    fetchEvents: async (filter) =>
      filter.kinds?.[0] === 1617 ? [] : [createdReceipt()],
  });
  const result = await runContinue(dependencies, {
    artifacts: [patchArtifact()],
    checkpointAuthor: AUTHOR,
  });
  assert.equal(result.ok, true, result.ok ? "" : result.reason);
  const decoded = decodeCodingSessionHandoverEvent({
    event: dependencies.published.at(-1),
    channelRef: CHANNEL,
    sessionRef: SESSION,
    genesisRef: GENESIS,
  });
  assert.ok(
    decoded.value.body.missing.some((line) =>
      /was not on the relay/.test(line),
    ),
    decoded.value.body.missing.join(" | "),
  );
});

test("a checkpoint that named artifacts never signs `named no artifact`", async () => {
  // B3: with no checkout prepared, the honest sentence names what was not
  // fetched. Signing "the checkpoint named no artifact" over a checkpoint that
  // named a wip ref and a patch would be a false statement about A's work.
  const dependencies = deps({
    fetchEvents: async (filter) =>
      filter.kinds?.[0] === 1617 ? [patchEvent()] : [createdReceipt()],
  });
  const result = await runContinue(dependencies, {
    checkout: null,
    artifacts: [
      {
        kind: "wip-ref",
        repoRef: "30617:aa/beekeeper",
        ref: "refs/heads/wip/builder/abc12345",
        sha: "9".repeat(40),
      },
      patchArtifact(),
    ],
    checkpointAuthor: AUTHOR,
  });
  assert.equal(result.ok, true, result.ok ? "" : result.reason);
  const missing = decodeCodingSessionHandoverEvent({
    event: dependencies.published.at(-1),
    channelRef: CHANNEL,
    sessionRef: SESSION,
    genesisRef: GENESIS,
  }).value.body.missing;
  assert.ok(
    missing.some((line) =>
      /2 artifact\(s\) named by the checkpoint/.test(line),
    ),
    missing.join(" | "),
  );
  assert.ok(
    missing.some((line) => /patch was read but not applied/.test(line)),
    missing.join(" | "),
  );
  assert.ok(
    !missing.some((line) => /named no artifact/.test(line)),
    "a checkpoint that named artifacts must never be described as naming none",
  );
});

test("a checkpoint that truly named nothing says exactly that", async () => {
  const dependencies = deps();
  const result = await runContinue(dependencies, {
    checkout: null,
    artifacts: [],
    checkpointAuthor: AUTHOR,
  });
  assert.equal(result.ok, true);
  const missing = decodeCodingSessionHandoverEvent({
    event: dependencies.published.at(-1),
    channelRef: CHANNEL,
    sessionRef: SESSION,
    genesisRef: GENESIS,
  }).value.body.missing;
  assert.ok(
    missing.includes("the checkpoint named no artifact to recover"),
    missing.join(" | "),
  );
});

test("a patch is resolved even when the checkpoint named no wip ref", async () => {
  const requests = [];
  const dependencies = deps({
    fetchEvents: async (filter) =>
      filter.kinds?.[0] === 1617 ? [patchEvent()] : [createdReceipt()],
    prepareCheckout: async (request) => {
      requests.push(request);
      return {
        branch: "handover/683d55b3",
        checkedOutSha: "9".repeat(40),
        recovered: ["patch applied (70 bytes)"],
        missing: [],
      };
    },
  });
  const result = await runContinue(dependencies, {
    artifacts: [patchArtifact()],
    checkpointAuthor: AUTHOR,
  });
  assert.equal(result.ok, true, result.ok ? "" : result.reason);
  assert.equal(requests[0].patchText, PATCH_TEXT);
});

// ---------------------------------------------------------------------------
// The create must run where the work landed, and must prove it did.
// ---------------------------------------------------------------------------

test("the recovered directory is staged for this exact create, before it goes out", async () => {
  const order = [];
  const staged = [];
  const dependencies = deps({
    stageCreateHint: async (input) => {
      order.push("stage");
      staged.push(input);
      return {};
    },
    publishCreate: async (input) => {
      order.push("create");
      lastCommandId = input.commandId;
      return { eventId: "ee".repeat(32), kind: 44221 };
    },
  });
  const result = await continueCodingSessionHandover(
    continueInput({ artifacts: [], checkpointAuthor: AUTHOR }),
    dependencies,
  );
  assert.equal(result.ok, true, result.ok ? "" : result.reason);
  assert.deepEqual(order, ["stage", "create"], "the hint precedes the create");
  assert.equal(staged.length, 1);
  assert.equal(
    staged[0].path,
    "/tmp/checkout",
    "the directory the person chose, not a project default",
  );
  assert.equal(staged[0].commandId, result.progress.createCommandId);
});

test("a refused create drops the hint it staged", async () => {
  const cleared = [];
  const dependencies = deps({
    stageCreateHint: async () => ({}),
    clearCreateHint: async (commandId) => {
      cleared.push(commandId);
      return {};
    },
    publishCreate: async () => {
      throw new Error("relay refused the create");
    },
  });
  const result = await continueCodingSessionHandover(
    continueInput({ artifacts: [], checkpointAuthor: AUTHOR }),
    dependencies,
  );
  assert.equal(result.ok, false);
  assert.equal(result.step, "create");
  assert.deepEqual(cleared, [result.progress.createCommandId]);
});

test("an execution running somewhere else is said, not called recovered", async () => {
  const dependencies = deps({
    fetchEvents: async (filter) =>
      filter.kinds?.[0] === 44223
        ? [metadataEvent({ branch: "main", observedCommit: "a".repeat(40) })]
        : [createdReceipt()],
  });
  const result = await runContinue(dependencies);
  assert.equal(result.ok, true, result.ok ? "" : result.reason);
  assert.equal(result.progress.checkoutConfirmed, false);
  const missing = decodeCodingSessionHandoverEvent({
    event: dependencies.published.at(-1),
    channelRef: CHANNEL,
    sessionRef: SESSION,
    genesisRef: GENESIS,
  }).value.body.missing;
  assert.ok(
    missing.some((line) =>
      /the execution reports branch main at a{40}, not the recovered checkout — it is running somewhere else/.test(
        line,
      ),
    ),
    missing.join(" | "),
  );
});

test("an execution that published nothing is unconfirmed, and says so", async () => {
  const dependencies = deps({
    fetchEvents: async (filter) =>
      filter.kinds?.[0] === 44223 ? [] : [createdReceipt()],
  });
  const result = await runContinue(dependencies);
  assert.equal(result.progress.checkoutConfirmed, false);
  const missing = decodeCodingSessionHandoverEvent({
    event: dependencies.published.at(-1),
    channelRef: CHANNEL,
    sessionRef: SESSION,
    genesisRef: GENESIS,
  }).value.body.missing;
  assert.ok(
    missing.some((line) => /published no metadata/.test(line)),
    missing.join(" | "),
  );
});

test("a confirmed checkout adds no missing line at all", async () => {
  const dependencies = deps();
  const result = await runContinue(dependencies);
  assert.equal(result.progress.checkoutConfirmed, true);
});

test("the prompt is written after the recovery, and says the same thing the wire does", async () => {
  const prompts = [];
  const dependencies = deps({
    prepareCheckout: async () => ({
      branch: "handover/683d55b3",
      checkedOutSha: "9".repeat(40),
      recovered: ["wip-ref refs/heads/wip/builder/abc12345 at 9999"],
      missing: ["the 812-byte patch did not apply to keep.txt"],
    }),
    publishCreate: async (input) => {
      prompts.push(input.initialTurn);
      lastCommandId = input.commandId;
      return { eventId: "ee".repeat(32), kind: 44221 };
    },
  });
  const result = await continueCodingSessionHandover(
    continueInput({ artifacts: [], checkpointAuthor: AUTHOR }),
    dependencies,
  );
  assert.equal(result.ok, true, result.ok ? "" : result.reason);
  const prompt = prompts[0];
  // The branch and sha that actually landed.
  assert.match(prompt, /Checked out here: handover\/683d55b3 at 9{40}/);
  assert.match(prompt, /- wip-ref refs\/heads\/wip\/builder\/abc12345 at 9999/);
  // The one list, in both places.
  const missing = decodeCodingSessionHandoverEvent({
    event: dependencies.published.at(-1),
    channelRef: CHANNEL,
    sessionRef: SESSION,
    genesisRef: GENESIS,
  }).value.body.missing;
  assert.ok(missing.includes("the 812-byte patch did not apply to keep.txt"));
  for (const line of missing) {
    assert.ok(
      prompt.includes(line),
      `the agent must be told what the wire says is missing: ${line}`,
    );
  }
});
