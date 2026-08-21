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
  remoteTerminals: [],
  streamChannels: [],
  forumChannels: [],
  repos: [],
  workflows: [],
  agents: [],
  shellSessions: [],
};

const makeChannel = (overrides = {}) => ({
  id: "channel-1",
  name: "general",
  channelType: "stream",
  ...overrides,
});

const makeRepo = (overrides = {}) => ({
  repoAddress: "30617:owner:buzz",
  name: "buzz",
  ...overrides,
});

const makeWorkflow = (overrides = {}) => ({
  id: "wf-1",
  name: "Nightly deploy",
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

test("buildProjectChildren returns [] for empty input", () => {
  assert.deepEqual(buildProjectChildren(emptyInput), []);
});

test("buildProjectChildren groups interleaved types by rank order", () => {
  const rows = buildProjectChildren({
    streamChannels: [makeChannel()],
    forumChannels: [
      makeChannel({ id: "forum-1", name: "ideas", channelType: "forum" }),
    ],
    repos: [makeRepo()],
    workflows: [makeWorkflow()],
    agents: [{ key: "30175:owner:helper", label: "Helper" }],
    shellSessions: [makeShell()],
    remoteTerminals: [makeRemote()],
  });
  assert.deepEqual(
    rows.map((row) => row.type),
    ["channel", "forum", "repo", "workflow", "agent", "shell", "remote-shell"],
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
    repos: [
      makeRepo({ repoAddress: "30617:owner:zeta", name: "zeta" }),
      makeRepo({ repoAddress: "30617:owner:alpha", name: "Alpha" }),
      makeRepo({ repoAddress: "30617:owner:mid", name: "mid" }),
    ],
  });
  assert.deepEqual(
    rows.map((row) => row.repo.name),
    ["Alpha", "mid", "zeta"],
  );
});

test("identical labels fall back to the key tie-break", () => {
  const rows = buildProjectChildren({
    ...emptyInput,
    workflows: [
      makeWorkflow({ id: "wf-b", name: "deploy" }),
      makeWorkflow({ id: "wf-a", name: "deploy" }),
    ],
  });
  assert.deepEqual(
    rows.map((row) => row.workflow.id),
    ["wf-a", "wf-b"],
  );
});

test("projectChildKey is unique across types with colliding ids", () => {
  const rows = buildProjectChildren({
    ...emptyInput,
    streamChannels: [makeChannel({ id: "same-id", name: "one" })],
    forumChannels: [
      makeChannel({ id: "same-id", name: "one", channelType: "forum" }),
    ],
    workflows: [makeWorkflow({ id: "same-id", name: "one" })],
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
      type: "agent",
      agent: { key: "k", label: "Helper" },
    }),
    "Helper",
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
  const agent = { type: "agent", agent: { key: "k", label: "aaa" } };
  const channel = { type: "channel", channel: makeChannel({ name: "zzz" }) };
  assert.ok(compareProjectChildren(channel, agent) < 0);
  assert.ok(compareProjectChildren(agent, channel) > 0);
});
