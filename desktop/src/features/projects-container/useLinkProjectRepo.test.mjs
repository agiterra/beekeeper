import assert from "node:assert/strict";
import { test } from "node:test";

import { linkProjectRepo } from "./useLinkProjectRepo.ts";

const OWNER = "d".repeat(64);
const RELAY_ORIGIN = "https://relay.test";

function setupStubs() {
  const linkCalls = [];
  const signedEvents = [];
  globalThis.window = globalThis.window ?? {};
  globalThis.window.__TAURI_INTERNALS__ = {
    invoke: async (command, args) => {
      if (command === "link_project_local_repository") {
        linkCalls.push(args.input);
        return { path: args.input.path, remote: "origin" };
      }
      if (command === "sign_event") {
        signedEvents.push(args);
        return JSON.stringify({ id: "evt", tags: args.tags });
      }
      throw new Error(`Unexpected Tauri command: ${command}`);
    },
  };
  return {
    linkCalls,
    signedEvents,
    teardown: () => {
      delete globalThis.window.__TAURI_INTERNALS__;
    },
  };
}

function makeRepo(overrides = {}) {
  return {
    id: "widget",
    dtag: "widget",
    name: "Widget",
    owner: OWNER.toUpperCase(),
    repoAddress: `30617:${OWNER}:widget`,
    // Fork-style announcement: the clone tag names the external upstream.
    cloneUrls: ["https://github.com/example/widget.git"],
    ...overrides,
  };
}

test("link always targets the relay-hosted URL, not the announced clone tag", async () => {
  const stubs = setupStubs();
  try {
    const result = await linkProjectRepo({
      repo: makeRepo(),
      path: "/tmp/checkouts/widget",
      remoteStrategy: "add-buzz-remote",
      relayOrigin: RELAY_ORIGIN,
    });

    assert.equal(stubs.linkCalls.length, 1);
    assert.deepEqual(stubs.linkCalls[0], {
      path: "/tmp/checkouts/widget",
      // Derived relay URL — the GitHub clone tag must never reach the
      // workspace clone-URL gate.
      cloneUrl: `${RELAY_ORIGIN}/git/${OWNER}/widget`,
      owner: OWNER,
      dtag: "widget",
      remoteStrategy: "add-buzz-remote",
    });
    assert.equal(result.name, "Widget");
    // Linking never signs or publishes events.
    assert.equal(stubs.signedEvents.length, 0);
  } finally {
    stubs.teardown();
  }
});

test("link fails without a relay origin", async () => {
  const stubs = setupStubs();
  try {
    await assert.rejects(
      linkProjectRepo({
        repo: makeRepo(),
        path: "/tmp/checkouts/widget",
        remoteStrategy: "set-origin",
        relayOrigin: null,
      }),
      /Relay origin unavailable/,
    );
    assert.equal(stubs.linkCalls.length, 0);
  } finally {
    stubs.teardown();
  }
});
