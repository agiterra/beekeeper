import assert from "node:assert/strict";
import { test } from "node:test";

import {
  cascadeTerminalsFromEvents,
  channelBelongsToProject,
  describeProjectCascade,
  projectCascadeChannels,
  projectCascadeCounts,
  projectCascadeExclusionNotes,
  projectCascadeRepos,
  projectCascadeWorkflows,
} from "./projectCascade.ts";

const OWNER = "a".repeat(64);
const OTHER = "b".repeat(64);
const ADDRESS = `30621:${OWNER}:platform`;

const project = (overrides = {}) => ({
  address: ADDRESS,
  channelIds: [],
  repoAddrs: [],
  ...overrides,
});

const repo = (overrides = {}) => ({
  repoAddress: `30617:${OWNER}:platform`,
  dtag: "platform",
  name: "platform",
  owner: OWNER,
  projectRef: null,
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
    terminals: [],
    repos: [],
    foreignRepos: [],
  });

  assert.deepEqual(counts, {
    channels: 1,
    forums: 1,
    transports: 1,
    workflows: 1,
    foreignWorkflows: 0,
    repos: 0,
    foreignRepos: 0,
    terminals: 0,
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
    terminals: [],
    repos: [],
    foreignRepos: [],
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
    terminals: [],
    repos: [],
    foreignRepos: [],
  });
  assert.equal(describeProjectCascade(counts), "1 channel, 1 workflow");
});

test("shared terminals are counted and named in the summary", () => {
  // Terminals used to be survivors the dialog had to disclose. They are
  // deleted now, so they belong in the count the checkbox arms, not in the
  // exclusion notes below it.
  const counts = projectCascadeCounts({
    channels: [channel({ id: "c-1" })],
    workflows: [],
    foreignWorkflows: [],
    repos: [],
    foreignRepos: [],
    terminals: [
      { sessionId: "t-1", ownerPubkey: "a".repeat(64), title: "build" },
      { sessionId: "t-2", ownerPubkey: "a".repeat(64), title: "" },
    ],
  });
  assert.equal(counts.terminals, 2);
  assert.equal(counts.total, 3);
  assert.equal(describeProjectCascade(counts), "1 channel, 2 shared terminals");
});

test("a project whose only children are terminals still has children", () => {
  // Before terminals were deletable this was "nothing to delete" and the
  // checkbox stayed disabled over a project that plainly had something in it.
  const counts = projectCascadeCounts({
    channels: [],
    workflows: [],
    foreignWorkflows: [],
    repos: [],
    foreignRepos: [],
    terminals: [
      { sessionId: "t-1", ownerPubkey: "a".repeat(64), title: "build" },
    ],
  });
  assert.equal(counts.total, 1);
  assert.equal(describeProjectCascade(counts), "1 shared terminal");
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

test("terminals read their coordinate off the announce", () => {
  // The relay's `#a` filter already scopes the fetch to this project, so what
  // this reads is the coordinate each tombstone will name.
  const events = [
    {
      id: "e1",
      kind: 30623,
      pubkey: OWNER,
      created_at: 1,
      content: "",
      tags: [
        ["d", "term-1"],
        ["a", `30621:${OWNER}:platform`],
        ["title", "build"],
      ],
    },
  ];
  assert.deepEqual(cascadeTerminalsFromEvents(events), [
    { sessionId: "term-1", ownerPubkey: OWNER, title: "build" },
  ]);
});

test("terminals are ordered, de-duplicated and lowercased", () => {
  // The count under the checkbox and the deletion that follows it must
  // describe the same set, in the same order.
  const other = "b".repeat(64);
  const announce = (pubkey, sessionId) => ({
    id: `${pubkey}:${sessionId}`,
    kind: 30623,
    pubkey,
    created_at: 1,
    content: "",
    tags: [["d", sessionId]],
  });
  const terminals = cascadeTerminalsFromEvents([
    announce(other.toUpperCase(), "term-2"),
    announce(OWNER, "term-1"),
    announce(OWNER, "term-1"),
  ]);
  assert.equal(terminals.length, 2);
  assert.equal(terminals[0].ownerPubkey, OWNER);
  assert.equal(terminals[1].ownerPubkey, other);
});

test("an announce with no session id is skipped", () => {
  // Otherwise it would become a tombstone for `30623:<owner>:` — a
  // coordinate that names nothing.
  const events = [
    {
      id: "e1",
      kind: 30623,
      pubkey: OWNER,
      created_at: 1,
      content: "",
      tags: [],
    },
  ];
  assert.deepEqual(cascadeTerminalsFromEvents(events), []);
});

// ── Repositories ────────────────────────────────────────────────────────────
//
// The rule these pin is not "events I signed". The relay's
// `project_owner_admits_deletion` lets a project Owner delete a kind:30617
// they did not sign — but it resolves the governing project through the
// repo's *back-reference*, so a forward-ref-only repo falls back to the
// authorship arm. A delete issued there is accepted, matches no live row,
// and reports success having changed nothing.

test("a repository is claimed by its forward ref or its back-reference", () => {
  const forwardOnly = repo({
    repoAddress: `30617:${OTHER}:fwd`,
    dtag: "fwd",
    owner: OTHER,
  });
  const backOnly = repo({
    repoAddress: `30617:${OTHER}:back`,
    dtag: "back",
    owner: OTHER,
    projectRef: ADDRESS,
  });
  const unrelated = repo({
    repoAddress: `30617:${OTHER}:other`,
    dtag: "other",
    owner: OTHER,
  });
  const { mine, foreign } = projectCascadeRepos(
    project({ repoAddrs: [forwardOnly.repoAddress] }),
    [forwardOnly, backOnly, unrelated],
    OWNER,
    true,
  );
  const claimed = [...mine, ...foreign].map((entry) => entry.repoId).sort();
  assert.deepEqual(claimed, ["back", "fwd"]);
});

test("an owner may delete a teammate's repository only when it carries the back-reference", () => {
  const backReferenced = repo({
    repoAddress: `30617:${OTHER}:back`,
    dtag: "back",
    owner: OTHER,
    projectRef: ADDRESS,
  });
  const forwardOnly = repo({
    repoAddress: `30617:${OTHER}:fwd`,
    dtag: "fwd",
    owner: OTHER,
  });
  const { mine, foreign } = projectCascadeRepos(
    project({ repoAddrs: [forwardOnly.repoAddress] }),
    [backReferenced, forwardOnly],
    OWNER,
    true,
  );
  assert.deepEqual(
    mine.map((entry) => entry.repoId),
    ["back"],
  );
  // The one a naive "owners may delete anything in their project" rule gets
  // wrong: the relay would accept this tombstone and change nothing.
  assert.deepEqual(
    foreign.map((entry) => entry.repoId),
    ["fwd"],
  );
});

test("a non-owner deletes only what they signed, back-reference or not", () => {
  const ownRepo = repo({ projectRef: ADDRESS });
  const theirs = repo({
    repoAddress: `30617:${OTHER}:theirs`,
    dtag: "theirs",
    owner: OTHER,
    projectRef: ADDRESS,
  });
  const { mine, foreign } = projectCascadeRepos(
    project(),
    [ownRepo, theirs],
    OWNER,
    false,
  );
  assert.deepEqual(
    mine.map((entry) => entry.repoId),
    ["platform"],
  );
  assert.deepEqual(
    foreign.map((entry) => entry.repoId),
    ["theirs"],
  );
});

test("an unresolved identity classifies every repository as foreign", () => {
  const { mine, foreign } = projectCascadeRepos(
    project(),
    [repo({ projectRef: ADDRESS })],
    null,
    false,
  );
  assert.deepEqual(mine, []);
  assert.equal(foreign.length, 1);
});

test("repositories stay out of the main cascade total", () => {
  const counts = projectCascadeCounts({
    channels: [],
    workflows: [],
    foreignWorkflows: [],
    terminals: [],
    repos: [repo()],
    foreignRepos: [],
  });
  assert.equal(counts.repos, 1);
  // A project whose only child is a repository must still read as "nothing
  // to delete" under the first checkbox, or that box arms itself over work
  // it does not do.
  assert.equal(counts.total, 0);
});

test("a repository somebody else owns is named as a survivor", () => {
  const notes = projectCascadeExclusionNotes(
    projectCascadeCounts({
      channels: [],
      workflows: [],
      foreignWorkflows: [],
      terminals: [],
      repos: [],
      foreignRepos: [repo({ owner: OTHER })],
    }),
    false,
  );
  assert.equal(notes.length, 1);
  assert.match(notes[0], /1 repository in this project cannot be deleted/);
});
