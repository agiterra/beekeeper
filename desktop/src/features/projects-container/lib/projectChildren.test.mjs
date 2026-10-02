import assert from "node:assert/strict";
import { test } from "node:test";

import {
  buildProjectChildren,
  compareProjectChildren,
  PROJECT_CHILD_TYPE_RANK,
  projectChildKey,
  projectChildLabel,
} from "./projectChildren.ts";

const emptyInput = {
  codingSessions: [],
  remoteTerminals: [],
  streamChannels: [],
  forumChannels: [],
  shellSessions: [],
};

const makeChannel = (overrides = {}) => ({
  id: "channel-1",
  name: "general",
  channelType: "stream",
  ...overrides,
});

const makeShell = (overrides = {}) => ({
  sessionId: "shell-1",
  title: "zsh",
  running: true,
  ...overrides,
});

const makeRemote = (overrides = {}) => ({
  sessionId: "remote-1",
  ownerPubkey: "feedface".repeat(8),
  title: "build shell",
  projectRef: "30621:owner:alpha",
  dims: null,
  announcedAt: 1,
  ...overrides,
});

const makeSessionEntry = (overrides = {}) => ({
  placement: "project",
  projectId: "owner:alpha",
  placedBy: "channel",
  channelId: "channel-1",
  generationId: "generation-1",
  label: "Coding session · generation 1",
  sourceChannelLabel: null,
  runtimeLabel: "Claude Code",
  status: { kind: "idle", label: "Idle" },
  session: { lastEventAt: "2026-07-30T12:00:00.000Z" },
  ...overrides,
});

test("buildProjectChildren returns [] for empty input", () => {
  assert.deepEqual(buildProjectChildren(emptyInput), []);
});

test("the sidebar row model is channels, interactive work and what members pinned", () => {
  // Repositories, workflows, agents and Pulse belong to the project page;
  // the type table is the contract that keeps them out of the sidebar.
  assert.deepEqual(Object.keys(PROJECT_CHILD_TYPE_RANK).sort(), [
    "artifact",
    "channel",
    "coding-session",
    "forum",
    "remote-shell",
    "shell",
    "todo-list",
  ]);
});

test("pinned artifacts are rows after the to-do lists, in the project's own order", () => {
  const pin = (target, rank, targetKind = "file") => ({
    target,
    targetKind,
    pinned: true,
    rank,
    by: "a".repeat(64),
    updatedAt: 1,
  });
  const rows = buildProjectChildren({
    streamChannels: [makeChannel()],
    forumChannels: [],
    shellSessions: [],
    // Deliberately out of rank order and not alphabetical: the order the
    // sidebar shows is the project's, set by whoever reordered the pins, and
    // alphabetizing would silently undo every drag anyone made.
    artifactPins: [
      pin("docs/zebra.md", "a0"),
      pin("docs/mockups", "a1", "folder"),
      pin("plans/CURRENT_STATE.md", "a2"),
    ],
  });
  assert.deepEqual(
    rows.map((row) => row.type),
    ["channel", "artifact", "artifact", "artifact"],
  );
  assert.deepEqual(
    rows.slice(1).map((row) => projectChildLabel(row)),
    ["zebra", "mockups", "CURRENT_STATE"],
  );
  assert.equal(projectChildKey(rows[1]), "artifact:docs/zebra.md");
});

test("two pins at the same rank break on their target, never at random", () => {
  const pin = (target) => ({
    target,
    targetKind: "file",
    pinned: true,
    rank: "a0",
    by: "a".repeat(64),
    updatedAt: 1,
  });
  const rows = buildProjectChildren({
    streamChannels: [],
    forumChannels: [],
    shellSessions: [],
    artifactPins: [pin("docs/b.md"), pin("docs/a.md")],
  });
  assert.deepEqual(
    rows.map((row) => row.pin.target),
    ["docs/a.md", "docs/b.md"],
  );
});

test("a pinned to-do list is a row after the terminals, keyed and labelled by the list", () => {
  const list = {
    id: "1".repeat(32),
    title: "Launch",
    visibility: "project",
    archived: false,
    pinned: true,
    createdAt: 1,
    createdBy: "a".repeat(64),
    updatedAt: 1,
    open: [],
    completed: [],
  };
  const rows = buildProjectChildren({
    streamChannels: [makeChannel()],
    forumChannels: [],
    shellSessions: [makeShell()],
    todoLists: [list],
  });
  assert.deepEqual(
    rows.map((row) => row.type),
    ["channel", "shell", "todo-list"],
  );
  const row = rows[2];
  assert.equal(projectChildKey(row), `todo-list:${list.id}`);
  assert.equal(projectChildLabel(row), "Launch");
});

test("buildProjectChildren groups interleaved types by rank order", () => {
  const rows = buildProjectChildren({
    codingSessions: [makeSessionEntry()],
    streamChannels: [makeChannel()],
    forumChannels: [
      makeChannel({ id: "forum-1", name: "ideas", channelType: "forum" }),
    ],
    shellSessions: [makeShell()],
    remoteTerminals: [makeRemote()],
  });
  assert.deepEqual(
    rows.map((row) => row.type),
    ["coding-session", "channel", "forum", "shell", "remote-shell"],
  );
  const ranks = rows.map((row) => PROJECT_CHILD_TYPE_RANK[row.type]);
  assert.deepEqual(
    ranks,
    [...ranks].sort((a, b) => a - b),
  );
});

test("sorts case-insensitively by label within a type", () => {
  const rows = buildProjectChildren({
    ...emptyInput,
    streamChannels: [
      makeChannel({ id: "c-zeta", name: "zeta" }),
      makeChannel({ id: "c-alpha", name: "Alpha" }),
      makeChannel({ id: "c-mid", name: "mid" }),
    ],
  });
  assert.deepEqual(
    rows.map((row) => row.channel.name),
    ["Alpha", "mid", "zeta"],
  );
});

test("identical labels fall back to the key tie-break", () => {
  const rows = buildProjectChildren({
    ...emptyInput,
    streamChannels: [
      makeChannel({ id: "c-b", name: "deploy" }),
      makeChannel({ id: "c-a", name: "deploy" }),
    ],
  });
  assert.deepEqual(
    rows.map((row) => row.channel.id),
    ["c-a", "c-b"],
  );
});

test("coding sessions keep shelf activity order instead of alphabetizing", () => {
  const rows = buildProjectChildren({
    ...emptyInput,
    codingSessions: [
      makeSessionEntry({
        generationId: "zzz-idle",
        label: "Aaa idle session",
        status: { kind: "idle", label: "Idle" },
      }),
      makeSessionEntry({
        generationId: "aaa-working",
        label: "Zzz working session",
        status: { kind: "working", label: "Working" },
      }),
    ],
  });
  assert.deepEqual(
    rows.map((row) => row.entry.generationId),
    ["aaa-working", "zzz-idle"],
  );
});

test("a coding-session key is its exact channel and generation coordinates", () => {
  assert.equal(
    projectChildKey({
      type: "coding-session",
      entry: makeSessionEntry({
        channelId: "channel/a",
        generationId: "generation|1",
      }),
    }),
    "session:channel/a:generation|1",
  );
});

test("projectChildKey is unique across types with colliding ids", () => {
  const rows = buildProjectChildren({
    ...emptyInput,
    streamChannels: [makeChannel({ id: "same-id", name: "one" })],
    forumChannels: [
      makeChannel({ id: "same-id", name: "one", channelType: "forum" }),
    ],
    shellSessions: [makeShell({ sessionId: "same-id", title: "one" })],
  });
  const keys = rows.map(projectChildKey);
  assert.equal(new Set(keys).size, keys.length);
});

test("projectChildLabel reads the per-type display name", () => {
  assert.equal(
    projectChildLabel({ type: "channel", channel: makeChannel() }),
    "general",
  );
  assert.equal(
    projectChildLabel({
      type: "coding-session",
      entry: makeSessionEntry({ label: "Fix the build" }),
    }),
    "Fix the build",
  );
  assert.equal(
    projectChildLabel({ type: "shell", session: makeShell() }),
    "zsh",
  );
});

test("shell rows sort last and key by session id", () => {
  const rows = buildProjectChildren({
    ...emptyInput,
    streamChannels: [makeChannel()],
    shellSessions: [
      makeShell({ sessionId: "shell-b", title: "zsh" }),
      makeShell({ sessionId: "shell-a", title: "zsh" }),
    ],
  });
  assert.deepEqual(
    rows.map((row) => row.type),
    ["channel", "shell", "shell"],
  );
  assert.deepEqual(
    rows.slice(1).map((row) => row.session.sessionId),
    ["shell-a", "shell-b"],
  );
  assert.equal(projectChildKey(rows[1]), "shell:shell-a");
});

test("remote terminals key by owner+session and label by title", () => {
  const rows = buildProjectChildren({
    ...emptyInput,
    remoteTerminals: [makeRemote()],
  });
  assert.equal(rows.length, 1);
  assert.equal(
    projectChildKey(rows[0]),
    `remote-shell:${"feedface".repeat(8)}:remote-1`,
  );
  assert.equal(projectChildLabel(rows[0]), "build shell");
});

test("compareProjectChildren orders by rank before label", () => {
  const shell = { type: "shell", session: makeShell({ title: "aaa" }) };
  const channel = { type: "channel", channel: makeChannel({ name: "zzz" }) };
  assert.ok(compareProjectChildren(channel, shell) < 0);
  assert.ok(compareProjectChildren(shell, channel) > 0);
});
