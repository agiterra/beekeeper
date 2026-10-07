// Shared 44231 fixtures for the checkpoint tests. Signed with throwaway keys.
import {
  finalizeEvent,
  generateSecretKey,
  getPublicKey,
} from "nostr-tools/pure";

import { KIND_CODING_SESSION_CHECKPOINT } from "@/shared/constants/kinds.ts";
import { buildCodingSessionTargetKey } from "./codingSessionCommand.ts";
import {
  CODING_SESSION_CHECKPOINT_SCHEMA,
  CODING_SESSION_CHECKPOINT_TAG_VERSION,
  codingSessionCheckpointSemanticKey,
} from "./codingSessionCheckpoints.ts";

export const CHANNEL_ID = "1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9";
export const PROVIDER_SECRET = generateSecretKey();
export const PROVIDER_PUBKEY = getPublicKey(PROVIDER_SECRET);
export const STRANGER_SECRET = generateSecretKey();
export const STRANGER_PUBKEY = getPublicKey(STRANGER_SECRET);
export const TARGET = {
  driver: "claude-agent-acp",
  instanceId: "b1b2c3d4e5f60778",
  sessionId: "da8d6582-0000-4000-8000-000000000028",
  generation: 2,
};
export const TARGET_KEY = buildCodingSessionTargetKey(TARGET);
export const TREE_A = "a".repeat(40);
export const TREE_B = "b".repeat(40);
export const TREE_C = "c".repeat(40);
export const COMMIT = "d".repeat(40);
export const HEAD = "e".repeat(40);

export function scope(signers = [PROVIDER_PUBKEY]) {
  return {
    channelId: CHANNEL_ID,
    signersByTargetKey: new Map([[TARGET_KEY, new Set(signers)]]),
  };
}

/** A valid `turn` payload; `overrides` replace top-level keys. */
export function payload(overrides = {}) {
  return {
    schema: CODING_SESSION_CHECKPOINT_SCHEMA,
    session: TARGET,
    turnId: "turn-1",
    reason: "turn",
    coverage: { fromSeq: 41, throughSeq: 58 },
    git: gitFacts(),
    files: [
      {
        path: "src/main.rs",
        status: "modified",
        from: null,
        additions: 3,
        deletions: 1,
      },
    ],
    filesNotListed: 0,
    restorable: false,
    unavailable: null,
    summary: null,
    ...overrides,
  };
}

export function gitFacts(overrides = {}) {
  return {
    head: HEAD,
    branch: "main",
    baseTree: TREE_A,
    tree: TREE_B,
    commit: COMMIT,
    outsideTurn: false,
    complete: true,
    omitted: [],
    omittedNotListed: 0,
    ...overrides,
  };
}

export function unavailablePayload(code = "NOT_A_REPOSITORY", overrides = {}) {
  return payload({
    git: null,
    files: [],
    unavailable: {
      code,
      sentence: "The session's working directory is not a git repository.",
    },
    ...overrides,
  });
}

export function tagsFor(body, channelId = CHANNEL_ID) {
  return [
    ["h", channelId],
    ["csck-v", CODING_SESSION_CHECKPOINT_TAG_VERSION],
    ["cs-target", buildCodingSessionTargetKey(body.session)],
    ["csck-seq", String(body.coverage.throughSeq)],
    [
      "csck-key",
      codingSessionCheckpointSemanticKey(
        body.session,
        body.reason,
        body.coverage.throughSeq,
      ),
    ],
  ];
}

let clock = 1_800_600_000;

/** A signed 44231. `content` may be a string to send raw bytes. */
export function checkpointEvent(body = payload(), options = {}) {
  clock += 1;
  return finalizeEvent(
    {
      kind: options.kind ?? KIND_CODING_SESSION_CHECKPOINT,
      created_at: options.createdAt ?? clock,
      tags: options.tags ?? tagsFor(body),
      content:
        typeof options.content === "string"
          ? options.content
          : JSON.stringify(body),
    },
    options.secret ?? PROVIDER_SECRET,
  );
}
