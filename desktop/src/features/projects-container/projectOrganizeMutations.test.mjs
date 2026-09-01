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

function pTags(event) {
  return event.tags.filter((tag) => tag[0] === "p");
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
    members: [{ pubkey: MEMBER_A, role: "viewer" }],
    icon: "🐝",
    color: "#3b82f6",
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
    assert.deepEqual(pTags(event), [["p", MEMBER_A, "", "viewer"]]);
  } finally {
    stubs.teardown();
  }
});

test("updateProjectContainer keeps members when switching to public", async () => {
  const stubs = setupStubs();
  try {
    await updateProjectContainer({
      project: makeProject(),
      name: "Skunkworks",
      visibility: "public",
    });
    const event = stubs.signedEvents.at(-1);
    assert.deepEqual(tagValues(event, "buzz-access"), []);
    // Members are roles now, not just a private ACL — a public flip must not
    // drop them from the head event.
    assert.deepEqual(pTags(event), [["p", MEMBER_A, "", "viewer"]]);
  } finally {
    stubs.teardown();
  }
});

test("updateProjectContainer keeps icon and color when omitted", async () => {
  const stubs = setupStubs();
  try {
    await updateProjectContainer({
      project: makeProject(),
      name: "Skunkworks Renamed",
    });
    const event = stubs.signedEvents.at(-1);
    // Omitted (undefined) means "keep" — only an explicit null clears.
    assert.deepEqual(tagValues(event, "icon"), ["🐝"]);
    assert.deepEqual(tagValues(event, "color"), ["#3b82f6"]);
  } finally {
    stubs.teardown();
  }
});

test("updateProjectContainer clears icon and color on explicit null", async () => {
  const stubs = setupStubs();
  try {
    await updateProjectContainer({
      project: makeProject(),
      name: "Skunkworks",
      icon: null,
      color: null,
    });
    const event = stubs.signedEvents.at(-1);
    assert.deepEqual(tagValues(event, "icon"), []);
    assert.deepEqual(tagValues(event, "color"), []);
  } finally {
    stubs.teardown();
  }
});

test("updateProjectContainer stays creator-only, and says why", async () => {
  // Head edits are the one capability a roster Owner does NOT share with the
  // creator, and the refusal must name the real reason: NIP-01 addresses a
  // replaceable event by (kind, pubkey, d), so this republish signed by
  // anyone else would create a second project rather than edit this one.
  // "Only the project owner can edit it" was the old wording and it read as
  // a lie to somebody who *is* an owner.
  const stubs = setupStubs();
  try {
    await assert.rejects(
      updateProjectContainer({
        project: makeProject({ owner: OTHER_OWNER }),
        name: "Skunkworks",
      }),
      /Only the key that created this project/,
    );
  } finally {
    stubs.teardown();
  }
});
