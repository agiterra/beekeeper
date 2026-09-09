import assert from "node:assert/strict";
import { test } from "node:test";

import { relayClient } from "@/shared/api/relayClient";

import {
  LOCAL_GENERAL_ID,
  makeLocalGeneral,
} from "./lib/projectContainerModel.ts";
import { createProjectRepo } from "./useCreateProjectRepo.ts";

const OWNER = "a".repeat(64);
const OTHER_OWNER = "d".repeat(64);
const ACCESS_CHANNEL = "11111111-1111-4111-8111-111111111111";

const KIND_PROJECT = 30621;
const KIND_REPO_ANNOUNCEMENT = 30617;

/**
 * Stub the Tauri bridge and the relay round-trips, mirroring
 * useGeneralProjectMigration.test.mjs.
 */
function setupStubs({ repoEvents = [], projectEvents = [] } = {}) {
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
        return [];
      }
      throw new Error(`Unexpected Tauri command: ${command}`);
    },
  };
  const originalFetchEvents = relayClient.fetchEvents;
  const originalFetchEventsBatch = relayClient.fetchEventsBatch;
  const batchReads = [];
  const originalPublishEvent = relayClient.publishEvent;
  relayClient.fetchEvents = async (filter) => {
    if (filter.kinds.includes(KIND_PROJECT)) return projectEvents;
    if (filter.kinds.includes(KIND_REPO_ANNOUNCEMENT)) return repoEvents;
    return [];
  };
  relayClient.fetchEventsBatch = async (filters) => {
    batchReads.push(filters);
    return filters.flatMap((filter) =>
      filter.kinds.includes(KIND_REPO_ANNOUNCEMENT) ? repoEvents : [],
    );
  };
  relayClient.publishEvent = async () => {};
  return {
    signedEvents,
    batchReads,
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

test("createProjectRepo publishes one 30617 with the container back-ref and no per-repo 30621", async () => {
  const stubs = setupStubs();
  try {
    const result = await createProjectRepo({
      project: makeProject(),
      name: "Widget Lib",
      accessChannelId: ACCESS_CHANNEL,
    });

    assert.equal(result.dtag, "widget-lib");
    assert.equal(
      result.repoAddress,
      `${KIND_REPO_ANNOUNCEMENT}:${OWNER}:widget-lib`,
    );

    const repoEvents = stubs.signedEvents.filter(
      (event) => event.kind === KIND_REPO_ANNOUNCEMENT,
    );
    assert.equal(repoEvents.length, 1);
    assert.deepEqual(tagValues(repoEvents[0], "d"), ["widget-lib"]);
    assert.deepEqual(tagValues(repoEvents[0], "name"), ["Widget Lib"]);
    assert.deepEqual(tagValues(repoEvents[0], "buzz-channel"), [
      ACCESS_CHANNEL,
    ]);
    assert.deepEqual(tagValues(repoEvents[0], "project"), [
      `${KIND_PROJECT}:${OWNER}:skunkworks`,
    ]);

    // The regression this mutation exists to fix: creating a repo inside a
    // project must not spawn a sibling project container for the repo.
    const projectHeads = stubs.signedEvents.filter(
      (event) =>
        event.kind === KIND_PROJECT &&
        tagValues(event, "d").includes("widget-lib"),
    );
    assert.equal(projectHeads.length, 0);
  } finally {
    stubs.teardown();
  }
});

test("createProjectRepo adds an owner-curated forward ref on the owned container", async () => {
  const stubs = setupStubs();
  try {
    await createProjectRepo({
      project: makeProject({
        repoAddrs: [`${KIND_REPO_ANNOUNCEMENT}:${OWNER}:existing`],
        channelIds: [ACCESS_CHANNEL],
      }),
      name: "widget-lib",
      accessChannelId: ACCESS_CHANNEL,
    });

    const containerHeads = stubs.signedEvents.filter(
      (event) => event.kind === KIND_PROJECT,
    );
    assert.equal(containerHeads.length, 1);
    assert.deepEqual(tagValues(containerHeads[0], "d"), ["skunkworks"]);
    assert.deepEqual(tagValues(containerHeads[0], "a"), [
      `${KIND_REPO_ANNOUNCEMENT}:${OWNER}:existing`,
      `${KIND_REPO_ANNOUNCEMENT}:${OWNER}:widget-lib`,
    ]);
    assert.deepEqual(tagValues(containerHeads[0], "channel"), [ACCESS_CHANNEL]);
  } finally {
    stubs.teardown();
  }
});

test("createProjectRepo publishes only the 30617 when the container belongs to someone else", async () => {
  const stubs = setupStubs();
  try {
    await createProjectRepo({
      project: makeProject({
        id: `${OTHER_OWNER}:skunkworks`,
        owner: OTHER_OWNER,
        address: `${KIND_PROJECT}:${OTHER_OWNER}:skunkworks`,
      }),
      name: "widget-lib",
      accessChannelId: ACCESS_CHANNEL,
    });

    assert.equal(stubs.signedEvents.length, 1);
    assert.equal(stubs.signedEvents[0].kind, KIND_REPO_ANNOUNCEMENT);
    assert.deepEqual(tagValues(stubs.signedEvents[0], "project"), [
      `${KIND_PROJECT}:${OTHER_OWNER}:skunkworks`,
    ]);
  } finally {
    stubs.teardown();
  }
});

test("createProjectRepo rejects a duplicate repo coordinate before publishing", async () => {
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
        ],
      },
    ],
  });
  try {
    await assert.rejects(
      createProjectRepo({
        project: makeProject(),
        name: "widget-lib",
        accessChannelId: ACCESS_CHANNEL,
      }),
      /already exists/,
    );
    assert.equal(stubs.signedEvents.length, 0);
  } finally {
    stubs.teardown();
  }
});

test("createProjectRepo recovers when only the publish acknowledgement was lost", async () => {
  const stubs = setupStubs();
  const originalPublishEvent = relayClient.publishEvent;
  const originalFetchEventsBatch = relayClient.fetchEventsBatch;
  try {
    relayClient.publishEvent = async (event) => {
      if (event.kind === KIND_REPO_ANNOUNCEMENT) {
        throw new Error("mock lost acknowledgement");
      }
    };
    // The relay stored the event: the id-keyed recovery query finds it.
    relayClient.fetchEventsBatch = async ([filter]) => {
      if (filter.ids) return [stubs.signedEvents[0]];
      return [];
    };

    const result = await createProjectRepo({
      project: makeProject(),
      name: "widget-lib",
      accessChannelId: ACCESS_CHANNEL,
    });
    assert.equal(result.dtag, "widget-lib");
  } finally {
    relayClient.publishEvent = originalPublishEvent;
    relayClient.fetchEventsBatch = originalFetchEventsBatch;
    stubs.teardown();
  }
});

test("createProjectRepo surfaces a genuine publish rejection", async () => {
  const stubs = setupStubs();
  const originalPublishEvent = relayClient.publishEvent;
  try {
    relayClient.publishEvent = async (event) => {
      if (event.kind === KIND_REPO_ANNOUNCEMENT) {
        throw new Error("mock rejection");
      }
    };

    await assert.rejects(
      createProjectRepo({
        project: makeProject(),
        name: "widget-lib",
        accessChannelId: ACCESS_CHANNEL,
      }),
      /mock rejection/,
    );
  } finally {
    relayClient.publishEvent = originalPublishEvent;
    stubs.teardown();
  }
});

test("createProjectRepo publishes the real General first for the local placeholder", async () => {
  const stubs = setupStubs();
  try {
    const general = makeLocalGeneral();
    assert.equal(general.id, LOCAL_GENERAL_ID);
    await createProjectRepo({
      project: general,
      name: "widget-lib",
      accessChannelId: ACCESS_CHANNEL,
    });

    const generalHeads = stubs.signedEvents.filter(
      (event) =>
        event.kind === KIND_PROJECT &&
        tagValues(event, "d").includes("general"),
    );
    // Once to materialize the placeholder, once for the forward ref.
    assert.equal(generalHeads.length, 2);

    const repoEvents = stubs.signedEvents.filter(
      (event) => event.kind === KIND_REPO_ANNOUNCEMENT,
    );
    assert.equal(repoEvents.length, 1);
    assert.deepEqual(tagValues(repoEvents[0], "project"), [
      `${KIND_PROJECT}:${OWNER}:general`,
    ]);
    assert.ok(
      tagValues(generalHeads[1], "a").includes(
        `${KIND_REPO_ANNOUNCEMENT}:${OWNER}:widget-lib`,
      ),
    );
  } finally {
    stubs.teardown();
  }
});

test("repository creation checks its exact coordinate without waiting for discovery reads", async () => {
  const stubs = setupStubs();
  try {
    relayClient.fetchEvents = async () => {
      throw new Error("WebSocket discovery reads are unavailable");
    };
    const result = await createProjectRepo({
      project: makeProject(),
      name: "Widget Lib",
    });
    assert.equal(result.dtag, "widget-lib");
    assert.deepEqual(stubs.batchReads, [
      [
        {
          kinds: [KIND_REPO_ANNOUNCEMENT],
          authors: [OWNER],
          "#d": ["widget-lib"],
          limit: 1,
        },
      ],
    ]);
  } finally {
    stubs.teardown();
  }
});

test("repository creation does not treat a failed duplicate lookup as absence", async () => {
  const stubs = setupStubs();
  try {
    relayClient.fetchEventsBatch = async () => {
      throw new Error("Could not authenticate repository lookup");
    };
    await assert.rejects(
      createProjectRepo({ project: makeProject(), name: "widget-lib" }),
      /Could not authenticate repository lookup/,
    );
    assert.equal(stubs.signedEvents.length, 0);
  } finally {
    stubs.teardown();
  }
});
