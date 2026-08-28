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

function pTags(event) {
  return event.tags.filter((tag) => tag[0] === "p");
}

test("publishProjectContainer emits buzz-access + role-carrying p tags when private", async () => {
  const stubs = setupStubs();
  try {
    await publishProjectContainer({
      name: "Skunkworks",
      dtag: "skunkworks",
      visibility: "private",
      members: [
        { pubkey: MEMBER_A, role: "collaborator" },
        { pubkey: MEMBER_B, role: "viewer" },
      ],
    });
    const event = stubs.signedEvents.at(-1);
    assert.deepEqual(tagValues(event, "buzz-access"), ["private"]);
    assert.deepEqual(pTags(event), [
      ["p", MEMBER_A, "", "collaborator"],
      ["p", MEMBER_B, "", "viewer"],
    ]);
  } finally {
    stubs.teardown();
  }
});

test("publishProjectContainer keeps member p tags on public projects", async () => {
  const stubs = setupStubs();
  try {
    // Members carry roles, not just a private ACL — going public keeps them.
    await publishProjectContainer({
      name: "Skunkworks",
      dtag: "skunkworks",
      visibility: "public",
      members: [{ pubkey: MEMBER_A, role: "owner" }],
    });
    const event = stubs.signedEvents.at(-1);
    assert.deepEqual(tagValues(event, "buzz-access"), []);
    assert.deepEqual(pTags(event), [["p", MEMBER_A, "", "owner"]]);
  } finally {
    stubs.teardown();
  }
});

test("publishProjectContainer omits p tags when no members are passed", async () => {
  const stubs = setupStubs();
  try {
    await publishProjectContainer({
      name: "Skunkworks",
      dtag: "skunkworks",
      visibility: "public",
    });
    const event = stubs.signedEvents.at(-1);
    assert.deepEqual(tagValues(event, "buzz-access"), []);
    assert.deepEqual(pTags(event), []);
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
      members: [
        { pubkey: MEMBER_A, role: "collaborator" },
        { pubkey: OWNER, role: "owner" },
        { pubkey: OWNER.toUpperCase(), role: "owner" },
      ],
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
      members: [{ pubkey: MEMBER_A, role: "collaborator" }],
    });
    const event = stubs.signedEvents.at(-1);
    assert.deepEqual(tagValues(event, "buzz-access"), []);
    // Members ride along (roles are visibility-independent) — only the
    // privatization is refused.
    assert.deepEqual(pTags(event), [["p", MEMBER_A, "", "collaborator"]]);
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
      members: [{ pubkey: MEMBER_A, role: "owner" }],
      icon: "🐝",
      color: "#3b82f6",
    };
    await addProjectMembers(project, { channelIds: ["chan-1"] });
    const event = stubs.signedEvents.at(-1);
    assert.deepEqual(tagValues(event, "buzz-access"), ["private"]);
    assert.deepEqual(pTags(event), [["p", MEMBER_A, "", "owner"]]);
    assert.deepEqual(tagValues(event, "channel"), ["chan-1"]);
    assert.deepEqual(tagValues(event, "icon"), ["🐝"]);
    assert.deepEqual(tagValues(event, "color"), ["#3b82f6"]);
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
      members: [
        { pubkey: MEMBER_A, role: "collaborator" },
        { pubkey: MEMBER_B, role: "viewer" },
      ],
      icon: "🚀",
      color: "#ef4444",
    };
    await removeProjectMembers(project, {
      repoAddrs: [`30617:${OWNER}:repo-1`],
    });
    const event = stubs.signedEvents.at(-1);
    assert.deepEqual(tagValues(event, "buzz-access"), ["private"]);
    assert.deepEqual(pTags(event), [
      ["p", MEMBER_A, "", "collaborator"],
      ["p", MEMBER_B, "", "viewer"],
    ]);
    assert.deepEqual(tagValues(event, "a"), []);
    assert.deepEqual(tagValues(event, "channel"), ["chan-1"]);
    assert.deepEqual(tagValues(event, "icon"), ["🚀"]);
    assert.deepEqual(tagValues(event, "color"), ["#ef4444"]);
  } finally {
    stubs.teardown();
  }
});

test("publishProjectContainer emits icon and color tags only when set and valid", async () => {
  const stubs = setupStubs();
  try {
    await publishProjectContainer({
      name: "Skunkworks",
      dtag: "skunkworks",
      icon: "🐝",
      color: "#3B82F6",
    });
    let event = stubs.signedEvents.at(-1);
    assert.deepEqual(tagValues(event, "icon"), ["🐝"]);
    // Color is normalized to lowercase on write.
    assert.deepEqual(tagValues(event, "color"), ["#3b82f6"]);

    await publishProjectContainer({
      name: "Skunkworks",
      dtag: "skunkworks",
      icon: null,
      color: null,
    });
    event = stubs.signedEvents.at(-1);
    assert.deepEqual(tagValues(event, "icon"), []);
    assert.deepEqual(tagValues(event, "color"), []);

    // Malformed color never reaches the event.
    await publishProjectContainer({
      name: "Skunkworks",
      dtag: "skunkworks",
      icon: "  ",
      color: "red",
    });
    event = stubs.signedEvents.at(-1);
    assert.deepEqual(tagValues(event, "icon"), []);
    assert.deepEqual(tagValues(event, "color"), []);
  } finally {
    stubs.teardown();
  }
});
