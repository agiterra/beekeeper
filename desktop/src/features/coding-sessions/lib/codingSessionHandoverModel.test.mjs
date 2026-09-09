import assert from "node:assert/strict";
import test from "node:test";

import {
  CODING_SESSION_HANDOVER_PARTIAL_DISCLOSURE,
  deriveCodingSessionHandoverModel,
} from "./codingSessionHandoverModel.ts";

const FOUNDER = "aa".repeat(32);
const B = "bb".repeat(32);
const C = "cc".repeat(32);
const BODY_A = "1a".repeat(32);
const BODY_B = "2b".repeat(32);
const CLAIM = "c1".repeat(32);
const CHECKPOINT = "cp".replace("cp", "0c").repeat(32);
const CONTINUATION = "0f".repeat(32);

function checkpointRow(overrides = {}) {
  return {
    eventId: overrides.eventId ?? CHECKPOINT,
    supersededBy: overrides.supersededBy ?? null,
    author: FOUNDER,
    createdAt: overrides.createdAt ?? 10,
    standing: overrides.standing ?? "authorized",
    body: {
      prevCheckpointRef: overrides.prevCheckpointRef ?? null,
      task: "task",
      assignmentRefs: [],
      decisions: [],
      revision: {
        repoRef: null,
        baseSha: null,
        headSha: "9".repeat(40),
        branch: "work/x",
        dirty: true,
        preserved: "all",
        ...(overrides.revision ?? {}),
      },
      artifacts: overrides.artifacts ?? [],
      tests: [],
      unresolved: [],
      nextAction: "next",
      missing: overrides.missing ?? [],
    },
  };
}

function continuationRow(overrides = {}) {
  return {
    eventId: CONTINUATION,
    author: B,
    createdAt: 20,
    claimRef: CLAIM,
    mode: "reconstructed",
    target: {
      driver: "acp",
      instanceId: "i",
      sessionId: "s",
      generation: 1,
    },
    checkpointRef: CHECKPOINT,
    recovered: ["wip-ref refs/heads/wip/builder/abc at 9999"],
    missing: [],
    note: null,
    standing: "authorized",
    ...overrides,
  };
}

function foldOf(overrides = {}) {
  return {
    checkpoints: overrides.checkpoints ?? [checkpointRow()],
    latestAuthorizedCheckpoint:
      overrides.latestAuthorizedCheckpoint === undefined
        ? CHECKPOINT
        : overrides.latestAuthorizedCheckpoint,
    continuations: overrides.continuations ?? [],
    activeContinuation: overrides.activeContinuation ?? null,
    claim: overrides.claim ?? { state: "no-claim" },
    claimSince: overrides.claimSince ?? null,
    claimVoidedAt: overrides.claimVoidedAt ?? null,
    retired: overrides.retired ?? false,
    excluded: [],
  };
}

function generation(overrides = {}) {
  return {
    handover: undefined,
    targetKey: "t",
    executionKey: "e",
    providerAuthorityPubkey: BODY_A,
    current: true,
    reachability: "unverified",
    status: "running",
    statusAt: 1,
    branch: null,
    observedCommit: null,
    dirty: null,
    relayReachable: null,
    verifiedAt: null,
    commitConfirmation: "Commit not checked",
    leaseState: null,
    leaseIssuedAt: null,
    leaseAcceptedAt: null,
    leaseExpiresAt: null,
    leaseSigner: null,
    leaseSourceEventId: null,
    leaseSequence: null,
    lifecycleCommandEventId: "l",
    lifecycleReceiptEventId: "r",
    sourceEventIds: [],
    ...overrides,
  };
}

function derive(overrides = {}) {
  return deriveCodingSessionHandoverModel({
    fold: overrides.fold ?? foldOf(),
    viewerPubkey: overrides.viewerPubkey ?? B,
    founderPubkey: overrides.founderPubkey ?? FOUNDER,
    operatorPubkeys: overrides.operatorPubkeys ?? [B],
    generations: overrides.generations ?? [generation()],
    thisExecutionProviderPubkey:
      overrides.thisExecutionProviderPubkey === undefined
        ? BODY_A
        : overrides.thisExecutionProviderPubkey,
    retiredAt: overrides.retiredAt ?? null,
  });
}

test("an operator may continue an unreachable session that carries a checkpoint", () => {
  const model = derive();
  assert.equal(model.viewerMayContinue, true);
  assert.equal(model.viewerIsClaimant, false);
  assert.equal(model.thisExecutionFenced, false);
  assert.equal(model.outcomeLabel, null);
});

test("a reachable execution is steered, not taken over", () => {
  const model = derive({
    generations: [generation({ reachability: "provider_reachable" })],
  });
  assert.equal(
    model.viewerMayContinue,
    false,
    "reachable work is continued from the composer, not reconstructed",
  );
});

test("a claimed body that is still reachable offers no reconstruction", () => {
  const model = derive({
    fold: foldOf({
      claim: {
        state: "active",
        claimant: C,
        bodyPubkey: BODY_B,
        acceptedEventId: CLAIM,
        seq: 2,
      },
    }),
    generations: [
      generation({
        providerAuthorityPubkey: BODY_B,
        reachability: "provider_reachable",
      }),
    ],
  });
  assert.equal(model.claimedBodyReachable, true);
  assert.equal(model.viewerMayContinue, false);
  assert.equal(
    model.viewerMayTakeBack,
    true,
    "take-back does not depend on reachability",
  );
});

test("the claimant is never offered the session they already hold", () => {
  const model = derive({
    fold: foldOf({
      claim: {
        state: "active",
        claimant: B,
        bodyPubkey: BODY_B,
        acceptedEventId: CLAIM,
        seq: 2,
      },
    }),
  });
  assert.equal(model.viewerIsClaimant, true);
  assert.equal(model.viewerMayContinue, false);
  assert.equal(model.viewerMayTakeBack, false);
});

test("a viewer with no standing is offered nothing", () => {
  const model = derive({ viewerPubkey: C, operatorPubkeys: [B] });
  assert.equal(model.viewerMayContinue, false);
  assert.equal(model.viewerMayTakeBack, false);
});

test("an execution that is not the claimed body reads fenced", () => {
  const model = derive({
    fold: foldOf({
      claim: {
        state: "active",
        claimant: C,
        bodyPubkey: BODY_B,
        acceptedEventId: CLAIM,
        seq: 2,
      },
    }),
    thisExecutionProviderPubkey: BODY_A,
  });
  assert.equal(model.thisExecutionFenced, true);
  assert.equal(model.activeBody, BODY_B);
});

test("a voided claim fences every body and still offers a fresh takeover", () => {
  const model = derive({
    fold: foldOf({
      claim: {
        state: "voided",
        last: {
          claimant: C,
          bodyPubkey: BODY_B,
          acceptedEventId: CLAIM,
          seq: 2,
        },
        voidedBy: "de".repeat(32),
        seq: 3,
      },
      claimVoidedAt: 900,
    }),
    thisExecutionProviderPubkey: BODY_B,
  });
  assert.equal(
    model.thisExecutionFenced,
    true,
    "even the last claimed body stays fenced once the claim is void",
  );
  assert.equal(model.viewerMayTakeBack, true);
  assert.equal(model.viewerMayContinue, true);
  assert.deepEqual(
    model.evidenceLinks.map((link) => link.label),
    ["Voided claim", "Voided by", "Checkpoint"],
  );
});

test("`partial` preservation always says so, patch artifact or not", () => {
  const model = derive({
    fold: foldOf({
      checkpoints: [
        checkpointRow({
          revision: { preserved: "partial" },
          artifacts: [
            {
              kind: "patch",
              repoRef: "repo",
              eventId: "ab".repeat(32),
              baseSha: "9".repeat(40),
              bytes: 10,
            },
          ],
          missing: ["editor scratch buffer"],
        }),
      ],
    }),
  });
  assert.equal(model.missing[0], CODING_SESSION_HANDOVER_PARTIAL_DISCLOSURE);
  assert.deepEqual(model.missing, [
    CODING_SESSION_HANDOVER_PARTIAL_DISCLOSURE,
    "editor scratch buffer",
  ]);
  assert.ok(
    model.evidenceLinks.some((link) => link.label === "Patch"),
    "the patch is still linked — it is evidence, not a guarantee",
  );
});

test("`all` preservation adds no disclosure it cannot support", () => {
  const model = derive();
  assert.deepEqual(model.missing, []);
});

test("`none` preservation discloses as loudly as `partial`", () => {
  const model = derive({
    fold: foldOf({
      checkpoints: [checkpointRow({ revision: { preserved: "none" } })],
    }),
  });
  assert.deepEqual(model.missing, [CODING_SESSION_HANDOVER_PARTIAL_DISCLOSURE]);
});

test("the two outcomes are labelled apart, never conflated", () => {
  const reconstructed = derive({
    fold: foldOf({
      claim: {
        state: "active",
        claimant: C,
        bodyPubkey: BODY_B,
        acceptedEventId: CLAIM,
        seq: 2,
        since: 5,
      },
      continuations: [continuationRow()],
      activeContinuation: CONTINUATION,
    }),
  });
  assert.equal(reconstructed.outcomeLabel, "Reconstructed");
  assert.deepEqual(reconstructed.recovered, [
    "wip-ref refs/heads/wip/builder/abc at 9999",
  ]);

  const native = derive({
    fold: foldOf({
      claim: {
        state: "active",
        claimant: C,
        bodyPubkey: BODY_B,
        acceptedEventId: CLAIM,
        seq: 2,
        since: 5,
      },
      continuations: [continuationRow({ mode: "native-resume" })],
      activeContinuation: CONTINUATION,
    }),
  });
  assert.equal(native.outcomeLabel, "Native continuation");
});

test("a retired umbrella offers nothing at all", () => {
  const model = derive({
    fold: foldOf({ retired: true, claim: { state: "no-claim" } }),
    retiredAt: 1_700_000_000,
  });
  assert.equal(model.retired, true);
  assert.equal(model.retiredAt, 1_700_000_000);
  assert.equal(model.viewerMayContinue, false);
  assert.equal(model.viewerMayTakeBack, false);
  assert.equal(model.thisExecutionFenced, false);
});

test("no authorized checkpoint means nothing to reconstruct from", () => {
  const model = derive({
    fold: foldOf({ checkpoints: [], latestAuthorizedCheckpoint: null }),
  });
  assert.equal(model.latestCheckpoint, null);
  assert.equal(model.viewerMayContinue, false);
});

// ---------------------------------------------------------------------------
// The fence, told apart by the fact that causes it.
// ---------------------------------------------------------------------------

const ACTIVE_ON_BODY_A = {
  state: "active",
  claimant: C,
  bodyPubkey: BODY_A,
  acceptedEventId: CLAIM,
  seq: 2,
};

test("a non-claimant on the claimed body is fenced, not merely idle", () => {
  // The provider refuses `operator != claim.claimant` on the claimed body too
  // (§3). A body check alone would have called this "not fenced" while every
  // turn came back refused.
  const model = derive({
    fold: foldOf({ claim: ACTIVE_ON_BODY_A }),
    thisExecutionProviderPubkey: BODY_A,
    viewerPubkey: B,
  });
  assert.equal(model.thisExecutionFenced, true);
  assert.equal(model.fenceReason, "not-claimant");
});

test("the claimant on their own body is not fenced", () => {
  const model = derive({
    fold: foldOf({ claim: ACTIVE_ON_BODY_A }),
    thisExecutionProviderPubkey: BODY_A,
    viewerPubkey: C,
    operatorPubkeys: [B, C],
  });
  assert.equal(model.thisExecutionFenced, false);
  assert.equal(model.fenceReason, null);
});

test("an unknown body is `unknown`, never `not fenced`", () => {
  const model = derive({
    fold: foldOf({ claim: ACTIVE_ON_BODY_A }),
    thisExecutionProviderPubkey: null,
  });
  assert.equal(model.fenceReason, "unknown");
  assert.equal(
    model.thisExecutionFenced,
    true,
    "absence of evidence about the body is not evidence that it may act",
  );
});

test("a provider's own 44223 fence is believed while the chain read catches up", () => {
  const disclosed = {
    state: "active",
    claimant: C,
    bodyPubkey: BODY_B,
    acceptedEventId: CLAIM,
  };
  const model = derive({
    fold: foldOf({ claim: { state: "no-claim" } }),
    generations: [generation({ handover: disclosed })],
    thisExecutionProviderPubkey: BODY_A,
  });
  assert.deepEqual(model.metadataFence, disclosed);
  assert.equal(model.fenceReason, "other-body");
  assert.equal(
    model.claim.state,
    "no-claim",
    "the chain stays the primary source",
  );
});

test("a session nobody disclosed a fence for stays unfenced", () => {
  const model = derive({ generations: [generation()] });
  assert.equal(model.metadataFence, null);
  assert.equal(model.fenceReason, null);
});

test("a superseded continuation stays listed as history", () => {
  const older = continuationRow({
    eventId: "1f".repeat(32),
    claimRef: "aa".repeat(32),
    standing: "superseded",
    createdAt: 5,
  });
  const model = derive({
    fold: foldOf({
      claim: ACTIVE_ON_BODY_A,
      continuations: [older, continuationRow()],
      activeContinuation: CONTINUATION,
    }),
  });
  assert.equal(model.continuation.eventId, CONTINUATION);
  assert.equal(model.priorContinuation.eventId, older.eventId);
});

test("a provider that discloses a voided claim fences every body, its own included", () => {
  const model = derive({
    fold: foldOf({ claim: { state: "no-claim" } }),
    generations: [
      generation({
        handover: {
          state: "voided",
          claimant: C,
          bodyPubkey: BODY_A,
          acceptedEventId: CLAIM,
        },
      }),
    ],
    // The viewer is on the very body the voided claim named: still fenced.
    thisExecutionProviderPubkey: BODY_A,
  });
  assert.equal(model.fenceReason, "voided");
  assert.equal(model.thisExecutionFenced, true);
});

test("a superseded checkpoint is history, never the one reconstruction reads", () => {
  const replaced = checkpointRow({
    eventId: "0e".repeat(32),
    standing: "superseded",
    supersededBy: CHECKPOINT,
    missing: ["an older statement's missing list"],
  });
  const model = derive({
    fold: foldOf({
      checkpoints: [replaced, checkpointRow()],
      latestAuthorizedCheckpoint: CHECKPOINT,
    }),
  });
  assert.equal(model.latestCheckpoint.eventId, CHECKPOINT);
  assert.deepEqual(
    model.supersededCheckpoints.map((row) => row.eventId),
    [replaced.eventId],
  );
  assert.deepEqual(
    model.missing,
    [],
    "the replaced statement's own disclosures are not read as the current ones",
  );
});
