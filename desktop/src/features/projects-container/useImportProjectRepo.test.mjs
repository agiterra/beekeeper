import assert from "node:assert/strict";
import { test } from "node:test";

import { relayClient } from "@/shared/api/relayClient";

import { importProjectRepo } from "./useImportProjectRepo.ts";

const OWNER = "a".repeat(64);
const ACCESS_CHANNEL = "11111111-1111-4111-8111-111111111111";
const RELAY_ORIGIN = "https://relay.test";

const KIND_PROJECT = 30621;
const KIND_REPO_ANNOUNCEMENT = 30617;

function setupStubs({ repoEvents = [], projectEvents = [] } = {}) {
  const signedEvents = [];
  const importCalls = [];
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
      if (command === "import_project_local_repository") {
        importCalls.push(args.input);
        return {
          path: args.input.path,
          remote:
            args.input.remoteStrategy === "add-buzz-remote" ? "buzz" : "origin",
          branch: "main",
        };
      }
      if (command === "get_channels") {
        return [];
      }
      throw new Error(`Unexpected Tauri command: ${command}`);
    },
  };
  const originalFetchEvents = relayClient.fetchEvents;
  const originalFetchEventsBatch = relayClient.fetchEventsBatch;
  const originalPublishEvent = relayClient.publishEvent;
  relayClient.fetchEvents = async (filter) => {
    if (filter.kinds.includes(KIND_PROJECT)) return projectEvents;
    if (filter.kinds.includes(KIND_REPO_ANNOUNCEMENT)) return repoEvents;
    return [];
  };
  relayClient.fetchEventsBatch = async (filters) =>
    (
      await Promise.all(
        filters.map((filter) => relayClient.fetchEvents(filter)),
      )
    ).flat();
  relayClient.publishEvent = async () => {};
  return {
    signedEvents,
    importCalls,
    teardown: () => {
      delete globalThis.window.__TAURI_INTERNALS__;
      relayClient.fetchEvents = originalFetchEvents;
      relayClient.fetchEventsBatch = originalFetchEventsBatch;
      relayClient.publishEvent = originalPublishEvent;
    },
  };
}

function makeProject(overrides = {}) {
  return {
    id: `${OWNER}:skunkworks`,
    dtag: "skunkworks",
    owner: OWNER,
    name: "Skunkworks",
    description: "",
    createdAt: 1,
    address: `${KIND_PROJECT}:${OWNER}:skunkworks`,
    repoAddrs: [],
    agentAddrs: [],
    channelIds: [],
    ...overrides,
  };
}

function tagValues(event, name) {
  return event.tags.filter((tag) => tag[0] === name).map((tag) => tag[1]);
}

test("import announces one relay-hosted 30617 then imports the checkout", async () => {
  const stubs = setupStubs();
  try {
    const result = await importProjectRepo({
      project: makeProject(),
      name: "Widget Lib",
      accessChannelId: ACCESS_CHANNEL,
      path: "/tmp/checkouts/widget-lib",
      remoteStrategy: "set-origin",
      relayOrigin: RELAY_ORIGIN,
    });

    assert.equal(result.dtag, "widget-lib");
    assert.equal(result.branch, "main");

    const repoEvents = stubs.signedEvents.filter(
      (event) => event.kind === KIND_REPO_ANNOUNCEMENT,
    );
    assert.equal(repoEvents.length, 1);
    assert.deepEqual(tagValues(repoEvents[0], "project"), [
      `${KIND_PROJECT}:${OWNER}:skunkworks`,
    ]);
    assert.deepEqual(tagValues(repoEvents[0], "buzz-channel"), [
      ACCESS_CHANNEL,
    ]);
    // Relay-hosted: the announcement must not carry a clone tag; the import
    // uses the derived canonical URL instead.
    assert.deepEqual(tagValues(repoEvents[0], "clone"), []);

    assert.equal(stubs.importCalls.length, 1);
    assert.deepEqual(stubs.importCalls[0], {
      path: "/tmp/checkouts/widget-lib",
      cloneUrl: `${RELAY_ORIGIN}/git/${OWNER}/widget-lib`,
      owner: OWNER,
      dtag: "widget-lib",
      remoteStrategy: "set-origin",
    });
  } finally {
    stubs.teardown();
  }
});

test("import reuses my own announcement already pointing at the project", async () => {
  const stubs = setupStubs({
    repoEvents: [
      {
        id: "existing-repo",
        pubkey: OWNER,
        kind: KIND_REPO_ANNOUNCEMENT,
        created_at: 10,
        content: "",
        tags: [
          ["d", "widget-lib"],
          ["name", "widget-lib"],
          ["project", `${KIND_PROJECT}:${OWNER}:skunkworks`],
        ],
      },
    ],
  });
  try {
    const result = await importProjectRepo({
      project: makeProject(),
      name: "widget-lib",
      accessChannelId: ACCESS_CHANNEL,
      path: "/tmp/checkouts/widget-lib",
      remoteStrategy: "add-buzz-remote",
      relayOrigin: RELAY_ORIGIN,
    });

    // No second announcement was published; the import still ran.
    assert.equal(stubs.signedEvents.length, 0);
    assert.equal(stubs.importCalls.length, 1);
    assert.equal(stubs.importCalls[0].remoteStrategy, "add-buzz-remote");
    assert.equal(result.remote, "buzz");
    assert.equal(
      result.repoAddress,
      `${KIND_REPO_ANNOUNCEMENT}:${OWNER}:widget-lib`,
    );
  } finally {
    stubs.teardown();
  }
});

test("import keeps the duplicate error when the announcement belongs elsewhere", async () => {
  const stubs = setupStubs({
    repoEvents: [
      {
        id: "existing-repo",
        pubkey: OWNER,
        kind: KIND_REPO_ANNOUNCEMENT,
        created_at: 10,
        content: "",
        tags: [
          ["d", "widget-lib"],
          ["name", "widget-lib"],
          ["project", `${KIND_PROJECT}:${OWNER}:other-project`],
        ],
      },
    ],
  });
  try {
    await assert.rejects(
      importProjectRepo({
        project: makeProject(),
        name: "widget-lib",
        accessChannelId: ACCESS_CHANNEL,
        path: "/tmp/checkouts/widget-lib",
        remoteStrategy: "set-origin",
        relayOrigin: RELAY_ORIGIN,
      }),
      /already exists/,
    );
    assert.equal(stubs.importCalls.length, 0);
  } finally {
    stubs.teardown();
  }
});

test("import fails fast without a relay origin", async () => {
  const stubs = setupStubs();
  try {
    await assert.rejects(
      importProjectRepo({
        project: makeProject(),
        name: "widget-lib",
        accessChannelId: ACCESS_CHANNEL,
        path: "/tmp/checkouts/widget-lib",
        remoteStrategy: "set-origin",
        relayOrigin: null,
      }),
      /Relay origin unavailable/,
    );
    assert.equal(stubs.importCalls.length, 0);
  } finally {
    stubs.teardown();
  }
});
