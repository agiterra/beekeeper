import assert from "node:assert/strict";
import { test } from "node:test";

import { relayClient } from "@/shared/api/relayClient";

import { deleteRepository } from "./deleteRepository.ts";

const OWNER = "a".repeat(64);

function setupStubs() {
  const signed = [];
  globalThis.window = globalThis.window ?? {};
  globalThis.window.__TAURI_INTERNALS__ = {
    invoke: async (command, args) => {
      if (command === "sign_event") {
        signed.push(args);
        return JSON.stringify({
          id: "evt",
          sig: "sig",
          pubkey: OWNER,
          kind: args.kind,
          created_at: 1,
          content: args.content,
          tags: args.tags,
        });
      }
      throw new Error(`Unexpected Tauri command: ${command}`);
    },
  };
  const originalPublish = relayClient.publishEvent;
  relayClient.publishEvent = async () => {};
  return {
    signed,
    teardown: () => {
      delete globalThis.window.__TAURI_INTERNALS__;
      relayClient.publishEvent = originalPublish;
    },
  };
}

test("the tombstone names the announcement coordinate and nothing else", async () => {
  const stubs = setupStubs();
  try {
    await deleteRepository({ ownerPubkey: OWNER, repoId: "myrepo" });
    assert.equal(stubs.signed[0].kind, 5);
    assert.deepEqual(stubs.signed[0].tags, [["a", `30617:${OWNER}:myrepo`]]);
  } finally {
    stubs.teardown();
  }
});

test("a project owner's delete addresses the repo owner, not the caller", async () => {
  // A tombstone addressed to the caller names a coordinate that does not
  // exist. The relay accepts it, logs "no live row matched", and returns
  // success — a delete that reports done and changes nothing.
  const stubs = setupStubs();
  const repoOwner = "b".repeat(64);
  try {
    await deleteRepository({ ownerPubkey: repoOwner, repoId: "myrepo" });
    assert.deepEqual(stubs.signed[0].tags, [
      ["a", `30617:${repoOwner}:myrepo`],
    ]);
  } finally {
    stubs.teardown();
  }
});

test("the owner component is lowercased", async () => {
  const stubs = setupStubs();
  try {
    await deleteRepository({
      ownerPubkey: OWNER.toUpperCase(),
      repoId: "myrepo",
    });
    assert.deepEqual(stubs.signed[0].tags, [["a", `30617:${OWNER}:myrepo`]]);
  } finally {
    stubs.teardown();
  }
});
