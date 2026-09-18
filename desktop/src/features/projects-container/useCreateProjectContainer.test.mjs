import assert from "node:assert/strict";
import { test } from "node:test";

import { relayClient } from "@/shared/api/relayClient";

import {
  addProjectMembers,
  createProjectContainer,
  initProjectRepositories,
  projectRepositoryIds,
  publishProjectContainer,
  removeProjectMembers,
  repositoryIdTakenRefusal,
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

function existingProject(overrides = {}) {
  return {
    id: "existing-head",
    pubkey: OWNER,
    kind: 30621,
    created_at: 1,
    tags: [
      ["d", "skunkworks"],
      ["name", "Skunkworks"],
    ],
    content: "",
    sig: "sig",
    ...overrides,
  };
}

function projectDeletion(overrides = {}) {
  return existingProject({
    id: "deletion",
    kind: 5,
    tags: [["a", `30621:${OWNER}:skunkworks`]],
    ...overrides,
  });
}

async function withCreationRead(eventsOrError, run) {
  const stubs = setupStubs();
  const originalBatch = relayClient.fetchEventsBatch;
  const filters = [];
  relayClient.fetchEventsBatch = async (requested) => {
    filters.push(requested);
    if (eventsOrError instanceof Error) throw eventsOrError;
    return eventsOrError;
  };
  try {
    await run({ ...stubs, filters });
  } finally {
    relayClient.fetchEventsBatch = originalBatch;
    stubs.teardown();
  }
}

test("create checks the exact owned head, the address tombstone and both repository ids in one bounded batch", async () => {
  await withCreationRead([], async ({ filters, signedEvents }) => {
    await createProjectContainer({ name: "  Skunkworks  " });
    assert.deepEqual(filters, [
      [
        { kinds: [30621], authors: [OWNER], "#d": ["skunkworks"], limit: 1 },
        {
          kinds: [5],
          authors: [OWNER],
          "#a": [`30621:${OWNER}:skunkworks`],
          limit: 1,
        },
        {
          kinds: [30617],
          "#d": ["skunkworks", "skunkworks-beekeeper-agents"],
          limit: 16,
        },
      ],
    ]);
    assert.equal(signedEvents.length, 1);
    assert.deepEqual(tagValues(signedEvents[0], "name"), ["Skunkworks"]);
  });
});

test("create refuses a repository id another key already announced, before signing anything", async () => {
  const taken = {
    id: "x".repeat(64),
    kind: 30617,
    pubkey: MEMBER_A,
    tags: [["d", "skunkworks-beekeeper-agents"]],
    content: "",
    created_at: 1,
    sig: "sig",
  };
  await withCreationRead([taken], async ({ signedEvents }) => {
    await assert.rejects(
      createProjectContainer({ name: "Skunkworks" }),
      /Repository id "skunkworks-beekeeper-agents" is already taken in this community by bbbbbbbb/,
    );
    assert.equal(signedEvents.length, 0, "nothing was signed");
  });
  // The owner's own announcement is not a collision: it is reused.
  assert.equal(
    repositoryIdTakenRefusal(
      [{ pubkey: OWNER, tags: [["d", "skunkworks"]] }],
      OWNER,
      ["skunkworks", "skunkworks-beekeeper-agents"],
    ),
    null,
  );
  assert.deepEqual(projectRepositoryIds("skunkworks"), {
    code: "skunkworks",
    agents: "skunkworks-beekeeper-agents",
  });
});

test("initProjectRepositories runs the host command and adds forward refs for the repositories that exist", async () => {
  const stubs = setupStubs();
  const previousInvoke = globalThis.window.__TAURI_INTERNALS__.invoke;
  const calls = [];
  globalThis.window.__TAURI_INTERNALS__.invoke = async (command, args) => {
    if (command === "project_agents_init") {
      calls.push(args);
      return {
        projectRef: args.projectRef,
        codeRepoRef: `30617:${OWNER}:skunkworks`,
        codeRepoId: "skunkworks",
        codeAnnouncementEventId: "c".repeat(64),
        codeRepoExisted: false,
        agentsRepoRef: `30617:${OWNER}:skunkworks-beekeeper-agents`,
        agentsRepoId: "skunkworks-beekeeper-agents",
        agentsCloneUrl:
          "https://hive.example/git/x/skunkworks-beekeeper-agents",
        agentsAnnouncementEventId: "d".repeat(64),
        agentsRepoExisted: false,
        branch: "main",
        roles: ["builder", "lead"],
        seedCommitSha: "e".repeat(40),
        seedError: null,
        seedSkipped: false,
        pushed: true,
        pushError: null,
        pushRecordEventId: null,
        sourceEventId: "f".repeat(64),
        sourceExisted: false,
        publicationError: null,
        commitIdentityName: "Owner",
        commitIdentityEmail: "aaaaaaaa@beekeeper.local",
        agentsAnnouncementWithdrawnEventId: null,
        agentsAnnouncementWithdrawalError: null,
        complete: true,
        gap: null,
      };
    }
    return previousInvoke(command, args);
  };
  try {
    const project = {
      id: "p",
      dtag: "skunkworks",
      owner: OWNER,
      name: "Skunkworks",
      description: "",
      createdAt: 1,
      address: `30621:${OWNER}:skunkworks`,
      repoAddrs: [],
      agentAddrs: [],
      channelIds: [],
      visibility: "public",
      members: [],
      icon: null,
      color: null,
    };
    const result = await initProjectRepositories(project);
    assert.equal(result.complete, true);
    assert.deepEqual(calls, [{ projectRef: project.address }]);
    const republished = stubs.signedEvents.at(-1);
    assert.equal(republished.kind, 30621);
    assert.deepEqual(tagValues(republished, "a"), [
      `30617:${OWNER}:skunkworks`,
      `30617:${OWNER}:skunkworks-beekeeper-agents`,
    ]);
  } finally {
    stubs.teardown();
  }
});

test("create refuses an old owned head without signing an overwrite", async () => {
  // Exact discovery has no community-wide 200-head cutoff: this target may
  // precede any number of unrelated projects and still occupies its address.
  await withCreationRead(
    [existingProject({ created_at: 1 })],
    async ({ signedEvents }) => {
      await assert.rejects(
        createProjectContainer({ name: "Skunkworks" }),
        /already have a project/,
      );
      assert.equal(signedEvents.length, 0);
    },
  );
});

test("another owner's same slug and our other slug do not occupy this address", async () => {
  await withCreationRead(
    [
      existingProject({ pubkey: MEMBER_A }),
      existingProject({ tags: [["d", "other-project"]] }),
    ],
    async ({ signedEvents }) => {
      await createProjectContainer({ name: "Skunkworks" });
      assert.equal(signedEvents.length, 1);
    },
  );
});

test("owner address tombstone retains existing project recreation semantics", async () => {
  await withCreationRead(
    [existingProject(), projectDeletion()],
    async ({ signedEvents }) => {
      await createProjectContainer({ name: "Skunkworks" });
      assert.equal(signedEvents.length, 1);
    },
  );
});

for (const [label, deletion] of [
  ["foreign author", projectDeletion({ pubkey: MEMBER_A })],
  ["other address", projectDeletion({ tags: [["a", `30621:${OWNER}:other`]] })],
  ["event-only reference", projectDeletion({ tags: [["e", "existing-head"]] })],
  ["non-deletion event", projectDeletion({ kind: 1 })],
]) {
  test(`create does not treat ${label} as an address tombstone`, async () => {
    await withCreationRead(
      [existingProject(), deletion],
      async ({ signedEvents }) => {
        await assert.rejects(
          createProjectContainer({ name: "Skunkworks" }),
          /already have a project/,
        );
        assert.equal(signedEvents.length, 0);
      },
    );
  });
}

for (const tags of [
  [
    ["d", ""],
    ["d", "skunkworks"],
  ],
  [
    ["d", "other"],
    ["d", "skunkworks"],
  ],
]) {
  test(`malformed matching head ${JSON.stringify(tags)} is not absence`, async () => {
    await withCreationRead(
      [existingProject({ tags }), projectDeletion()],
      async ({ signedEvents }) => {
        await assert.rejects(
          createProjectContainer({ name: "Skunkworks" }),
          /Could not verify/,
        );
        assert.equal(signedEvents.length, 0);
      },
    );
  });
}

for (const reason of ["Query refused", "Query timed out"]) {
  test(`create signs nothing when ${reason.toLowerCase()}`, async () => {
    await withCreationRead(new Error(reason), async ({ signedEvents }) => {
      await assert.rejects(
        createProjectContainer({ name: "Skunkworks" }),
        new RegExp(reason),
      );
      assert.equal(signedEvents.length, 0);
    });
  });
}

for (const [label, head] of [
  ["conflict", existingProject()],
  ["absence", null],
]) {
  test(`create retains ${label} checking through the existing HTTP-to-WS fallback`, async () => {
    const stubs = setupStubs();
    const originalHistory = relayClient.fetchHistory;
    const fallbacks = [];
    // The real batch API invokes query_relay_filters. The unsupported-command
    // error from setupStubs exercises its real WS fallback, not a fake batch.
    relayClient.fetchHistory = async (filter) => {
      fallbacks.push(filter);
      return filter.kinds.includes(30621) && head ? [head] : [];
    };
    try {
      const create = createProjectContainer({ name: "Skunkworks" });
      if (head) await assert.rejects(create, /already have a project/);
      else await create;
      assert.equal(fallbacks.length, 3);
      assert.deepEqual(fallbacks[0], {
        kinds: [30621],
        authors: [OWNER],
        "#d": ["skunkworks"],
        limit: 1,
      });
      assert.deepEqual(fallbacks[1], {
        kinds: [5],
        authors: [OWNER],
        "#a": [`30621:${OWNER}:skunkworks`],
        limit: 1,
      });
      assert.deepEqual(fallbacks[2], {
        kinds: [30617],
        "#d": ["skunkworks", "skunkworks-beekeeper-agents"],
        limit: 16,
      });
      assert.equal(stubs.signedEvents.length, head ? 0 : 1);
    } finally {
      relayClient.fetchHistory = originalHistory;
      stubs.teardown();
    }
  });
}

test("an older address tombstone cannot clear a newer recreated project head", async () => {
  await withCreationRead(
    [existingProject({ created_at: 20 }), projectDeletion({ created_at: 10 })],
    async ({ signedEvents }) => {
      await assert.rejects(
        createProjectContainer({ name: "Skunkworks" }),
        /already have a project/,
      );
      assert.equal(signedEvents.length, 0);
    },
  );
});

for (const deletionTime of [20, 21]) {
  test(`address tombstone at ${deletionTime} covers the project head at 20`, async () => {
    await withCreationRead(
      [
        existingProject({ created_at: 20 }),
        projectDeletion({ created_at: deletionTime }),
      ],
      async ({ signedEvents }) => {
        await createProjectContainer({ name: "Skunkworks" });
        assert.equal(signedEvents.length, 1);
      },
    );
  });
}
