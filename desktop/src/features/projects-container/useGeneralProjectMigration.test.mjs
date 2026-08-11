import assert from "node:assert/strict";
import { test } from "node:test";

import { relayClient } from "@/shared/api/relayClient";

import { publishGeneralProject } from "./useGeneralProjectMigration.ts";

const OWNER = "a".repeat(64);
const OTHER = "b".repeat(64);

const KIND_PROJECT = 30621;
const KIND_REPO_ANNOUNCEMENT = 30617;

/**
 * Stub the Tauri bridge (`get_identity`, `sign_event`, `get_channels`) and
 * the relay round-trips (`fetchEvents` routed by kind, `publishEvent` as a
 * no-op), mirroring useCreateProjectContainer.test.mjs.
 */
function setupStubs({ channels = [], repoEvents = [], projectEvents = [] }) {
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
      if (command === "get_channels") {
        return channels;
      }
      throw new Error(`Unexpected Tauri command: ${command}`);
    },
  };
  const originalFetchEvents = relayClient.fetchEvents;
  const originalPublishEvent = relayClient.publishEvent;
  relayClient.fetchEvents = async (filter) => {
    if (filter.kinds.includes(KIND_PROJECT)) return projectEvents;
    if (filter.kinds.includes(KIND_REPO_ANNOUNCEMENT)) return repoEvents;
    return []; // deletions
  };
  relayClient.publishEvent = async () => {};
  return {
    signedEvents,
    teardown: () => {
      delete globalThis.window.__TAURI_INTERNALS__;
      relayClient.fetchEvents = originalFetchEvents;
      relayClient.publishEvent = originalPublishEvent;
    },
  };
}

function rawChannel({ id, channelType = "stream", projectRef = null }) {
  return {
    id,
    name: id,
    channel_type: channelType,
    visibility: "open",
    description: "",
    topic: null,
    purpose: null,
    member_count: 1,
    member_pubkeys: [],
    last_message_at: null,
    archived_at: null,
    participants: [],
    participant_pubkeys: [],
    ttl_seconds: null,
    ttl_deadline: null,
    project_ref: projectRef,
  };
}

function repoEvent({ dtag, projectRef = null }) {
  return {
    id: `repo-${dtag}`,
    pubkey: OWNER,
    kind: KIND_REPO_ANNOUNCEMENT,
    created_at: 10,
    content: "",
    tags: [
      ["d", dtag],
      ["name", dtag],
      ...(projectRef ? [["project", projectRef]] : []),
    ],
  };
}

function projectEvent({ dtag, owner = OTHER, tags = [] }) {
  return {
    id: `project-${dtag}`,
    pubkey: owner,
    kind: KIND_PROJECT,
    created_at: 20,
    content: "",
    tags: [["d", dtag], ["name", dtag], ...tags],
  };
}

function tagValues(event, name) {
  return event.tags.filter((tag) => tag[0] === name).map((tag) => tag[1]);
}

test("publishGeneralProject sweeps only items no real project owns", async () => {
  const tankloopAddress = `${KIND_PROJECT}:${OTHER}:tankloop`;
  const stubs = setupStubs({
    channels: [
      rawChannel({ id: "c-unclaimed" }),
      rawChannel({ id: "c-claimed" }), // forward-claimed by tankloop
      rawChannel({ id: "c-backref", projectRef: tankloopAddress }),
      rawChannel({ id: "c-dm", channelType: "dm" }),
    ],
    repoEvents: [
      repoEvent({ dtag: "r-unclaimed" }),
      repoEvent({ dtag: "r-claimed" }), // forward-claimed by tankloop
      repoEvent({ dtag: "r-backref", projectRef: tankloopAddress }),
    ],
    projectEvents: [
      projectEvent({
        dtag: "tankloop",
        tags: [
          ["channel", "c-claimed"],
          ["a", `${KIND_REPO_ANNOUNCEMENT}:${OWNER}:r-claimed`],
        ],
      }),
    ],
  });
  try {
    await publishGeneralProject();
    assert.equal(stubs.signedEvents.length, 1);
    const general = stubs.signedEvents[0];
    assert.deepEqual(tagValues(general, "channel"), ["c-unclaimed"]);
    assert.deepEqual(tagValues(general, "a"), [
      `${KIND_REPO_ANNOUNCEMENT}:${OWNER}:r-unclaimed`,
    ]);
  } finally {
    stubs.teardown();
  }
});

test("publishGeneralProject ignores claims from other General events", async () => {
  // A foreign general claiming a channel must not suppress the sweep — the
  // republished general is the fallback owner for anything no real project
  // claims.
  const stubs = setupStubs({
    channels: [rawChannel({ id: "c-general-claimed" })],
    projectEvents: [
      projectEvent({
        dtag: "general",
        tags: [["channel", "c-general-claimed"]],
      }),
    ],
  });
  try {
    await publishGeneralProject();
    assert.equal(stubs.signedEvents.length, 1);
    assert.deepEqual(tagValues(stubs.signedEvents[0], "channel"), [
      "c-general-claimed",
    ]);
  } finally {
    stubs.teardown();
  }
});
