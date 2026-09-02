import assert from "node:assert/strict";
import { test } from "node:test";

import { relayClient } from "@/shared/api/relayClient";

import { deleteProjectContainerCascade } from "./projectOrganizeMutations.ts";

const OWNER = "a".repeat(64);
const OTHER_OWNER = "d".repeat(64);

/**
 * Stub the Tauri bridge and record every command in call order, so the test
 * can assert the project tombstone is signed *after* every child deletion.
 * `failing` names commands whose invocation should reject.
 */
function setupStubs({ failing = new Set(), roster = [] } = {}) {
  const calls = [];
  globalThis.window = globalThis.window ?? {};
  globalThis.window.__TAURI_INTERNALS__ = {
    invoke: async (command, args) => {
      calls.push({ command, args });
      if (failing.has(command)) throw new Error(`${command} failed`);
      if (command === "get_identity") {
        return { pubkey: OWNER, display_name: "Owner" };
      }
      if (command === "sign_event") {
        return JSON.stringify({
          id: `evt-${calls.length}`,
          sig: "sig",
          pubkey: OWNER,
          kind: args.kind,
          created_at: 1,
          content: args.content,
          tags: args.tags,
        });
      }
      if (command === "delete_channel" || command === "delete_workflow") {
        return null;
      }
      throw new Error(`Unexpected Tauri command: ${command}`);
    },
  };
  const originalPublishEvent = relayClient.publishEvent;
  relayClient.publishEvent = async () => {};
  // A non-creator delete resolves the project roster before it is allowed
  // through, so the kind:39010 read has to be answerable here. `roster` is
  // the projection's `p` tags; an empty list is a project whose only owner
  // is its creator.
  const originalFetchEvents = relayClient.fetchEvents;
  relayClient.fetchEvents = async () => [
    {
      id: "roster",
      pubkey: "f".repeat(64),
      kind: 39010,
      created_at: 10,
      content: "",
      tags: [
        ["d", `30621:${OWNER}:platform`],
        ...roster.map(({ pubkey, role }) => ["p", pubkey, "", role]),
      ],
    },
  ];
  return {
    calls,
    commands: () => calls.map((call) => call.command),
    teardown: () => {
      delete globalThis.window.__TAURI_INTERNALS__;
      relayClient.publishEvent = originalPublishEvent;
      relayClient.fetchEvents = originalFetchEvents;
    },
  };
}

function makeProject(overrides = {}) {
  return {
    id: `${OWNER}:platform`,
    dtag: "platform",
    owner: OWNER,
    name: "Platform",
    description: "",
    createdAt: 1,
    address: `30621:${OWNER}:platform`,
    repoAddrs: [],
    agentAddrs: [],
    channelIds: [],
    visibility: "private",
    members: [],
    ...overrides,
  };
}

const targets = {
  channels: [
    { id: "c-1", name: "general", channelType: "stream" },
    { id: "c-2", name: "sessions", channelType: "transport" },
  ],
  workflows: [
    { id: "w-1", name: "Nightly", channelId: "c-1", ownerPubkey: OWNER },
  ],
  foreignWorkflows: [],
  terminals: [],
};

test("cascade deletes channels and workflows, then the project LAST", async () => {
  const stubs = setupStubs();
  try {
    await deleteProjectContainerCascade({ project: makeProject(), targets });

    // get_identity, both channels, the workflow, then the tombstone
    // signature. Exactly *one* get_identity: the cascade authorizes at the
    // top and the final tombstone reuses that decision. The second one this
    // list used to carry re-resolved the viewer after every child was
    // already deleted, so its only possible new outcome was to refuse after
    // the destruction.
    assert.deepEqual(stubs.commands(), [
      "get_identity",
      "delete_channel",
      "delete_channel",
      "delete_workflow",
      "sign_event",
    ]);
    const last = stubs.calls.at(-1);
    assert.equal(last.command, "sign_event");
    assert.equal(last.args.kind, 5);
    assert.deepEqual(last.args.tags, [["a", `30621:${OWNER}:platform`]]);
  } finally {
    stubs.teardown();
  }
});

test("cascade deletes the session transport channel", async () => {
  const stubs = setupStubs();
  try {
    await deleteProjectContainerCascade({ project: makeProject(), targets });
    const deleted = stubs.calls
      .filter((call) => call.command === "delete_channel")
      .map((call) => call.args.channelId);
    assert.deepEqual(deleted, ["c-1", "c-2"]);
  } finally {
    stubs.teardown();
  }
});

test("a child failure aborts before the project tombstone", async () => {
  const stubs = setupStubs({ failing: new Set(["delete_workflow"]) });
  try {
    await assert.rejects(
      deleteProjectContainerCascade({ project: makeProject(), targets }),
      /was NOT deleted/,
    );
    assert.ok(
      !stubs.commands().includes("sign_event"),
      "the project tombstone must not be published after a child failure",
    );
  } finally {
    stubs.teardown();
  }
});

test("cascade never signs a delete for a workflow it does not own", async () => {
  const stubs = setupStubs();
  try {
    await deleteProjectContainerCascade({
      project: makeProject(),
      targets: {
        channels: [],
        // A foreign workflow that leaked into the deletable list must still
        // not be issued a kind:5: the tombstone would address
        // `30620:<caller>:w-theirs`, which does not exist, and the relay would
        // return accepted:true while the workflow stayed alive.
        workflows: [
          { id: "w-mine", name: "Mine", channelId: "c-1", ownerPubkey: OWNER },
          {
            id: "w-theirs",
            name: "Theirs",
            channelId: "c-1",
            ownerPubkey: OTHER_OWNER,
          },
        ],
        foreignWorkflows: [],
        terminals: [],
      },
    });

    const deletedWorkflows = stubs.calls
      .filter((call) => call.command === "delete_workflow")
      .map((call) => call.args.workflowId);
    assert.deepEqual(deletedWorkflows, ["w-mine"]);
  } finally {
    stubs.teardown();
  }
});

test("cascade still refuses an identity with no owner role, before deleting anything", async () => {
  // The stub identity is OWNER; this project belongs to somebody else and
  // its roster names nobody, so OWNER holds no role in it at all.
  const stubs = setupStubs();
  try {
    await assert.rejects(
      deleteProjectContainerCascade({
        project: makeProject({ owner: OTHER_OWNER }),
        targets,
      }),
      /Only a project owner can delete it/,
    );
    // The refusal lands before any child is touched — a cascade that deleted
    // half a project and then failed authorization would be unrecoverable.
    assert.deepEqual(stubs.commands(), ["get_identity"]);
  } finally {
    stubs.teardown();
  }
});

test("cascade admits a roster owner who did not create the project", async () => {
  // The capability this whole change exists for: the head is addressed to
  // OTHER_OWNER's key, which OWNER can never sign for, and OWNER deletes it
  // anyway on the strength of an `owner` seat. The relay agrees —
  // `project_owner_admits_deletion` resolves the same role from the ACL.
  const stubs = setupStubs({ roster: [{ pubkey: OWNER, role: "owner" }] });
  try {
    await deleteProjectContainerCascade({
      project: makeProject({ owner: OTHER_OWNER }),
      targets,
    });
    assert.deepEqual(stubs.commands(), [
      "get_identity",
      "delete_channel",
      "delete_channel",
      "delete_workflow",
      "sign_event",
    ]);
  } finally {
    stubs.teardown();
  }
});

test("cascade refuses a collaborator seat", async () => {
  // Write access into a project is not authority over the project.
  const stubs = setupStubs({
    roster: [{ pubkey: OWNER, role: "collaborator" }],
  });
  try {
    await assert.rejects(
      deleteProjectContainerCascade({
        project: makeProject({ owner: OTHER_OWNER }),
        targets,
      }),
      /Only a project owner can delete it/,
    );
    assert.deepEqual(stubs.commands(), ["get_identity"]);
  } finally {
    stubs.teardown();
  }
});
