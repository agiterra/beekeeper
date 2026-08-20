import assert from "node:assert/strict";
import { test } from "node:test";

import {
  channelBelongsToProject,
  describeProjectCascade,
  projectCascadeChannels,
  projectCascadeCounts,
  projectCascadeExclusionNotes,
  projectCascadeWorkflows,
} from "./projectCascade.ts";

const OWNER = "a".repeat(64);
const OTHER = "b".repeat(64);
const ADDRESS = `30621:${OWNER}:platform`;

const project = (overrides = {}) => ({
  address: ADDRESS,
  channelIds: [],
  ...overrides,
});

const channel = (overrides = {}) => ({
  id: "c-1",
  name: "general",
  channelType: "stream",
  projectRef: null,
  ...overrides,
});

const workflow = (overrides = {}) => ({
  id: "w-1",
  name: "Nightly",
  channelId: "c-1",
  ownerPubkey: OWNER,
  ...overrides,
});

test("a channel is claimed by its forward ref or its back-reference", () => {
  const byForwardRef = channelBelongsToProject(
    channel({ id: "c-1" }),
    project({ channelIds: ["c-1"] }),
  );
  const byBackRef = channelBelongsToProject(
    channel({ id: "c-2", projectRef: ADDRESS }),
    project(),
  );
  const unrelated = channelBelongsToProject(channel({ id: "c-3" }), project());

  assert.equal(byForwardRef, true);
  assert.equal(byBackRef, true);
  assert.equal(unrelated, false);
});

test("cascade channels include transports and exclude DMs", () => {
  const channels = [
    channel({ id: "c-1", projectRef: ADDRESS }),
    channel({ id: "c-2", channelType: "forum", projectRef: ADDRESS }),
    channel({ id: "c-3", channelType: "transport", projectRef: ADDRESS }),
    channel({ id: "c-4", channelType: "dm", projectRef: ADDRESS }),
    channel({ id: "c-5" }),
  ];

  const picked = projectCascadeChannels(project(), channels);

  assert.deepEqual(
    picked.map((c) => c.id),
    ["c-1", "c-2", "c-3"],
    "transport must be included; DM and unrelated channels must not",
  );
});

test("cascade workflows are limited to the project's channels", () => {
  const workflows = [
    workflow({ id: "w-1", channelId: "c-1" }),
    workflow({ id: "w-2", channelId: "elsewhere" }),
    workflow({ id: "w-3", channelId: null }),
  ];

  const { mine, foreign } = projectCascadeWorkflows(["c-1"], workflows, OWNER);

  assert.deepEqual(
    mine.map((w) => w.id),
    ["w-1"],
  );
  assert.deepEqual(foreign, []);
});

test("a workflow authored by someone else is never queued for deletion", () => {
  const workflows = [
    workflow({ id: "w-mine", channelId: "c-1", ownerPubkey: OWNER }),
    workflow({ id: "w-theirs", channelId: "c-1", ownerPubkey: OTHER }),
    workflow({ id: "w-theirs-2", channelId: "c-1", ownerPubkey: OTHER }),
  ];

  const { mine, foreign } = projectCascadeWorkflows(["c-1"], workflows, OWNER);

  assert.deepEqual(
    mine.map((w) => w.id),
    ["w-mine"],
    "a kind:5 tombstone only deletes the signer's own events",
  );
  assert.deepEqual(
    foreign.map((w) => w.id),
    ["w-theirs", "w-theirs-2"],
    "foreign workflows must be reported, not silently dropped",
  );
});

test("workflow ownership compares case-insensitively", () => {
  const { mine } = projectCascadeWorkflows(
    ["c-1"],
    [workflow({ ownerPubkey: OWNER.toUpperCase() })],
    OWNER,
  );
  assert.equal(mine.length, 1);
});

test("an unresolved identity classifies every workflow as foreign", () => {
  const { mine, foreign } = projectCascadeWorkflows(
    ["c-1"],
    [workflow()],
    null,
  );
  assert.deepEqual(mine, []);
  assert.equal(foreign.length, 1);
});

test("counts break transports out from ordinary channels", () => {
  const counts = projectCascadeCounts({
    channels: [
      channel({ id: "c-1" }),
      channel({ id: "c-2", channelType: "forum" }),
      channel({ id: "c-3", channelType: "transport" }),
    ],
    workflows: [workflow()],
    foreignWorkflows: [],
  });

  assert.deepEqual(counts, {
    channels: 1,
    forums: 1,
    transports: 1,
    workflows: 1,
    foreignWorkflows: 0,
    total: 4,
  });
});

test("foreign workflows are counted but excluded from the delete total", () => {
  const counts = projectCascadeCounts({
    channels: [channel({ id: "c-1" })],
    workflows: [workflow({ id: "w-1" })],
    foreignWorkflows: [
      workflow({ id: "w-2", ownerPubkey: OTHER }),
      workflow({ id: "w-3", ownerPubkey: OTHER }),
    ],
  });

  assert.equal(counts.workflows, 1);
  assert.equal(counts.foreignWorkflows, 2);
  assert.equal(
    counts.total,
    2,
    "the total must only cover what the cascade actually deletes",
  );
});

test("the summary counts only the workflows that get deleted", () => {
  const counts = projectCascadeCounts({
    channels: [channel({ id: "c-1" })],
    workflows: [workflow({ id: "w-1" })],
    foreignWorkflows: [workflow({ id: "w-2", ownerPubkey: OTHER })],
  });
  assert.equal(describeProjectCascade(counts), "1 channel, 1 workflow");
});

test("exclusion notes name the foreign workflows the cascade skips", () => {
  const notes = projectCascadeExclusionNotes(
    {
      channels: 1,
      forums: 0,
      transports: 0,
      workflows: 1,
      foreignWorkflows: 2,
      total: 2,
    },
    false,
  );
  assert.equal(notes.length, 1);
  assert.match(notes[0], /2 workflows .* created by someone else/);
});

test("exclusion notes say so when workflows cannot be enumerated at all", () => {
  const notes = projectCascadeExclusionNotes(
    {
      channels: 1,
      forums: 0,
      transports: 0,
      workflows: 0,
      foreignWorkflows: 0,
      total: 1,
    },
    true,
  );
  assert.equal(notes.length, 1);
  assert.match(notes[0], /can't be listed in this build/);
});

test("a cascade with nothing skipped produces no exclusion notes", () => {
  assert.deepEqual(
    projectCascadeExclusionNotes(
      {
        channels: 1,
        forums: 0,
        transports: 0,
        workflows: 1,
        foreignWorkflows: 0,
        total: 2,
      },
      false,
    ),
    [],
  );
});

test("the summary pluralizes and omits empty child types", () => {
  assert.equal(
    describeProjectCascade({
      channels: 2,
      forums: 0,
      transports: 1,
      workflows: 1,
      foreignWorkflows: 0,
      total: 4,
    }),
    "2 channels, 1 session transport, 1 workflow",
  );
});

test("an empty plan summarizes as the empty string", () => {
  assert.equal(
    describeProjectCascade({
      channels: 0,
      forums: 0,
      transports: 0,
      workflows: 0,
      foreignWorkflows: 0,
      total: 0,
    }),
    "",
  );
});
