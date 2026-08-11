import assert from "node:assert/strict";
import { test } from "node:test";

import { relayClient } from "@/shared/api/relayClient";

import {
  addProjectMembers,
  publishProjectContainer,
  removeProjectMembers,
} from "./useCreateProjectContainer.ts";

const OWNER = "a".repeat(64);
const MEMBER_A = "b".repeat(64);
const MEMBER_B = "c".repeat(64);

// The test-loader transpiles TS imports. `signRelayEvent` (tauri.ts) invokes
// `window.__TAURI_INTERNALS__.invoke("sign_event", ...)`; `getIdentity`
// invokes `get_identity`. Stub both, and stub relayClient.publishEvent (a
// real WebSocket round-trip in production) as a no-op that just resolves.
function setupStubs() {
  const signedEvents = [];
  globalThis.window = globalThis.window ?? {};
  globalThis.window.__TAURI_INTERNALS__ = {
    invoke: async (command, args) => {
      if (command === "get_identity") {
        return { pubkey: OWNER, display_name: "Owner" };
      }
      if (command === "sign_event") {
        const event = {
          id: `evt-${signedEvents.length}`,
          sig: "sig",
          pubkey: OWNER,
          kind: args.kind,
          created_at: args.createdAt ?? 1,
          content: args.content,
          tags: args.tags,
        };
        signedEvents.push(event);
        return JSON.stringify(event);
      }
      throw new Error(`Unexpected Tauri command: ${command}`);
    },
  };
  const originalPublishEvent = relayClient.publishEvent;
  relayClient.publishEvent = async () => {};
  return {
    signedEvents,
    teardown: () => {
      delete globalThis.window.__TAURI_INTERNALS__;
      relayClient.publishEvent = originalPublishEvent;
    },
  };
}

function tagValues(event, name) {
  return event.tags.filter((tag) => tag[0] === name).map((tag) => tag[1]);
}

test("publishProjectContainer emits buzz-access + p tags when private", async () => {
  const stubs = setupStubs();
  try {
    await publishProjectContainer({
      name: "Skunkworks",
      dtag: "skunkworks",
      visibility: "private",
      memberPubkeys: [MEMBER_A, MEMBER_B],
    });
    const event = stubs.signedEvents.at(-1);
    assert.deepEqual(tagValues(event, "buzz-access"), ["private"]);
    assert.deepEqual(tagValues(event, "p").sort(), [MEMBER_A, MEMBER_B].sort());
  } finally {
    stubs.teardown();
  }
});

test("publishProjectContainer omits buzz-access/p tags when public", async () => {
  const stubs = setupStubs();
  try {
    await publishProjectContainer({
      name: "Skunkworks",
      dtag: "skunkworks",
      visibility: "public",
    });
    const event = stubs.signedEvents.at(-1);
    assert.deepEqual(tagValues(event, "buzz-access"), []);
    assert.deepEqual(tagValues(event, "p"), []);
  } finally {
    stubs.teardown();
  }
});

test("publishProjectContainer excludes the owner from p tags even if passed", async () => {
  const stubs = setupStubs();
  try {
    await publishProjectContainer({
      name: "Skunkworks",
      dtag: "skunkworks",
      visibility: "private",
      memberPubkeys: [MEMBER_A, OWNER, OWNER.toUpperCase()],
    });
    const event = stubs.signedEvents.at(-1);
    assert.deepEqual(tagValues(event, "p"), [MEMBER_A]);
  } finally {
    stubs.teardown();
  }
});

test("publishProjectContainer forces the general project public regardless of input", async () => {
  const stubs = setupStubs();
  try {
    await publishProjectContainer({
      name: "General",
      dtag: "general",
      visibility: "private",
      memberPubkeys: [MEMBER_A],
    });
    const event = stubs.signedEvents.at(-1);
    assert.deepEqual(tagValues(event, "buzz-access"), []);
    assert.deepEqual(tagValues(event, "p"), []);
  } finally {
    stubs.teardown();
  }
});

// Regression: addProjectMembers/removeProjectMembers rebuild the whole event
// from the parsed project fields — a missed visibility/members passthrough
// would silently republish a private project as public.
test("addProjectMembers preserves visibility and members through a republish", async () => {
  const stubs = setupStubs();
  try {
    const project = {
      id: `${OWNER}:skunkworks`,
      dtag: "skunkworks",
      owner: OWNER,
      name: "Skunkworks",
      description: "",
      createdAt: 1,
      address: `30621:${OWNER}:skunkworks`,
      repoAddrs: [],
      agentAddrs: [],
      channelIds: [],
      visibility: "private",
      members: [MEMBER_A],
    };
    await addProjectMembers(project, { channelIds: ["chan-1"] });
    const event = stubs.signedEvents.at(-1);
    assert.deepEqual(tagValues(event, "buzz-access"), ["private"]);
    assert.deepEqual(tagValues(event, "p"), [MEMBER_A]);
    assert.deepEqual(tagValues(event, "channel"), ["chan-1"]);
  } finally {
    stubs.teardown();
  }
});

test("removeProjectMembers preserves visibility and members through a republish", async () => {
  const stubs = setupStubs();
  try {
    const project = {
      id: `${OWNER}:skunkworks`,
      dtag: "skunkworks",
      owner: OWNER,
      name: "Skunkworks",
      description: "",
      createdAt: 1,
      address: `30621:${OWNER}:skunkworks`,
      repoAddrs: [`30617:${OWNER}:repo-1`],
      agentAddrs: [],
      channelIds: ["chan-1"],
      visibility: "private",
      members: [MEMBER_A, MEMBER_B],
    };
    await removeProjectMembers(project, {
      repoAddrs: [`30617:${OWNER}:repo-1`],
    });
    const event = stubs.signedEvents.at(-1);
    assert.deepEqual(tagValues(event, "buzz-access"), ["private"]);
    assert.deepEqual(tagValues(event, "p").sort(), [MEMBER_A, MEMBER_B].sort());
    assert.deepEqual(tagValues(event, "a"), []);
    assert.deepEqual(tagValues(event, "channel"), ["chan-1"]);
  } finally {
    stubs.teardown();
  }
});
