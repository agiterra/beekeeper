import assert from "node:assert/strict";
import { test } from "node:test";

import { relayClient } from "@/shared/api/relayClient";

import { updateProjectContainer } from "./projectOrganizeMutations.ts";

const OWNER = "a".repeat(64);
const OTHER_OWNER = "d".repeat(64);
const MEMBER_A = "b".repeat(64);

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

function makeProject(overrides = {}) {
  return {
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
    ...overrides,
  };
}

test("updateProjectContainer keeps the project's current visibility/members when omitted", async () => {
  const stubs = setupStubs();
  try {
    await updateProjectContainer({
      project: makeProject(),
      name: "Skunkworks Renamed",
    });
    const event = stubs.signedEvents.at(-1);
    assert.deepEqual(tagValues(event, "buzz-access"), ["private"]);
    assert.deepEqual(tagValues(event, "p"), [MEMBER_A]);
  } finally {
    stubs.teardown();
  }
});

test("updateProjectContainer clears members when switching to public", async () => {
  const stubs = setupStubs();
  try {
    await updateProjectContainer({
      project: makeProject(),
      name: "Skunkworks",
      visibility: "public",
      // A stale member list must not survive a public switch.
      memberPubkeys: [MEMBER_A],
    });
    const event = stubs.signedEvents.at(-1);
    assert.deepEqual(tagValues(event, "buzz-access"), []);
    assert.deepEqual(tagValues(event, "p"), []);
  } finally {
    stubs.teardown();
  }
});

test("updateProjectContainer rejects edits from a non-owner identity", async () => {
  const stubs = setupStubs();
  try {
    await assert.rejects(
      updateProjectContainer({
        project: makeProject({ owner: OTHER_OWNER }),
        name: "Skunkworks",
      }),
      /Only the project owner/,
    );
  } finally {
    stubs.teardown();
  }
});
