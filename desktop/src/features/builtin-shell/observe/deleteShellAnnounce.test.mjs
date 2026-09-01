import assert from "node:assert/strict";
import { test } from "node:test";

import { relayClient } from "@/shared/api/relayClient";

import { deleteShellAnnounce } from "./deleteShellAnnounce.ts";

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
  const published = [];
  relayClient.publishEvent = async (event) => {
    published.push(event);
  };
  return {
    signed,
    published,
    teardown: () => {
      delete globalThis.window.__TAURI_INTERNALS__;
      relayClient.publishEvent = originalPublish;
    },
  };
}

test("the tombstone names the announce coordinate and nothing else", async () => {
  // The relay routes an addressable deletion entirely on this `a` tag, and
  // refuses any kind:5 carrying both an `a` and an `e` tag.
  const stubs = setupStubs();
  try {
    await deleteShellAnnounce({ ownerPubkey: OWNER, sessionId: "term-1" });
    assert.equal(stubs.signed.length, 1);
    assert.equal(stubs.signed[0].kind, 5);
    assert.deepEqual(stubs.signed[0].tags, [["a", `30623:${OWNER}:term-1`]]);
    assert.equal(stubs.published.length, 1);
  } finally {
    stubs.teardown();
  }
});

test("the owner component is lowercased", async () => {
  // project_acl and shell_session_acl store lowercase hex, and the relay
  // rebuilds the coordinate from decoded bytes; an uppercase key reaching
  // the wire would still resolve, but the two would stop being byte-equal
  // anywhere they are compared as strings.
  const stubs = setupStubs();
  try {
    await deleteShellAnnounce({
      ownerPubkey: OWNER.toUpperCase(),
      sessionId: "term-1",
    });
    assert.deepEqual(stubs.signed[0].tags, [["a", `30623:${OWNER}:term-1`]]);
  } finally {
    stubs.teardown();
  }
});
