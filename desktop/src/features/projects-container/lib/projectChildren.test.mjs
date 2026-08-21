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
  streamChannels: [],
  forumChannels: [],
  repos: [],
  workflows: [],
  agents: [],
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
  });
  assert.deepEqual(
    rows.map((row) => row.type),
    ["channel", "forum", "repo", "workflow", "agent"],
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
});

test("compareProjectChildren orders by rank before label", () => {
  const agent = { type: "agent", agent: { key: "k", label: "aaa" } };
  const channel = { type: "channel", channel: makeChannel({ name: "zzz" }) };
  assert.ok(compareProjectChildren(channel, agent) < 0);
  assert.ok(compareProjectChildren(agent, channel) > 0);
});
