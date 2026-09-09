import assert from "node:assert/strict";
import test from "node:test";
import { relayClient } from "@/shared/api/relayClient";
import { attachProjectRepository } from "./useAttachProjectRepository.ts";

const owner = "a".repeat(64);
const input = {
  project: { owner, dtag: "project", createdAt: 1 },
  repository: { repoAddress: `30617:${owner}:repo` },
};
test("attach uses an exact fresh head and refuses a concurrent advance before publishing", async () => {
  const original = relayClient.fetchEventsBatch;
  const reads = [];
  relayClient.fetchEventsBatch = async (filters) => {
    reads.push(filters);
    return [{ created_at: 2 }];
  };
  try {
    await assert.rejects(
      attachProjectRepository(input),
      /updated by another session/,
    );
    assert.deepEqual(reads, [
      [{ kinds: [30621], authors: [owner], "#d": ["project"], limit: 1 }],
    ]);
  } finally {
    relayClient.fetchEventsBatch = original;
  }
});
test("attach refuses unavailable or unauthenticated head reads", async () => {
  const original = relayClient.fetchEventsBatch;
  try {
    relayClient.fetchEventsBatch = async () => [];
    await assert.rejects(attachProjectRepository(input), /Could not find/);
    relayClient.fetchEventsBatch = async () => {
      throw new Error("Authentication refused");
    };
    await assert.rejects(
      attachProjectRepository(input),
      /Authentication refused/,
    );
  } finally {
    relayClient.fetchEventsBatch = original;
  }
});
