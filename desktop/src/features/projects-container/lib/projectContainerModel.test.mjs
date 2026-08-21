import assert from "node:assert/strict";
import { test } from "node:test";

import {
  dedupProjectEvents,
  displayProjectsWithGeneral,
  eventToProjectContainer,
  GENERAL_PROJECT_DTAG,
  isProjectContainerDeleted,
  makeLocalGeneral,
  parseMemberRef,
  partitionByChannelProject,
  partitionByProject,
  projectContainerAddress,
  sortProjectContainers,
} from "./projectContainerModel.ts";

const OWNER = "a".repeat(64);
const OTHER = "b".repeat(64);

function makeProjectEvent({
  pubkey = OWNER,
  dtag = "platform",
  name = "Platform",
  createdAt = 100,
  tags = [],
  content = "",
} = {}) {
  return {
    id: `${pubkey}-${dtag}-${createdAt}`,
    pubkey,
    kind: 30621,
    created_at: createdAt,
    content,
    tags: [["d", dtag], ["name", name], ...tags],
  };
}

test("parseMemberRef parses valid coordinates", () => {
  assert.deepEqual(parseMemberRef(`30617:${OWNER}:my-repo`), {
    kind: 30617,
    owner: OWNER,
    dtag: "my-repo",
  });
  // dtag may itself contain colons
  assert.equal(parseMemberRef(`30175:${OWNER}:a:b`)?.dtag, "a:b");
});

test("parseMemberRef rejects malformed coordinates", () => {
  assert.equal(parseMemberRef("nope"), null);
  assert.equal(parseMemberRef(`x:${OWNER}:d`), null);
  assert.equal(parseMemberRef("30617:short:d"), null);
  assert.equal(parseMemberRef(`30617:${OWNER}:`), null);
});

test("eventToProjectContainer splits repo and agent refs", () => {
  const event = makeProjectEvent({
    tags: [
      ["a", `30617:${OWNER}:repo-1`],
      ["a", `30175:${OWNER}:helper`],
      ["a", `30177:${OWNER}:${OTHER}`],
      ["a", `1:${OWNER}:not-a-member-kind`],
      ["a", "garbage"],
      ["channel", "chan-1"],
      ["channel", "chan-1"],
      ["description", "Infra"],
    ],
  });
  const project = eventToProjectContainer(event);
  assert.equal(project.id, `${OWNER}:platform`);
  assert.equal(project.address, projectContainerAddress(OWNER, "platform"));
  assert.deepEqual(project.repoAddrs, [`30617:${OWNER}:repo-1`]);
  assert.deepEqual(project.agentAddrs, [
    `30175:${OWNER}:helper`,
    `30177:${OWNER}:${OTHER}`,
  ]);
  assert.deepEqual(project.channelIds, ["chan-1"]);
  assert.equal(project.description, "Infra");
});

test("eventToProjectContainer returns null without a d tag", () => {
  const event = makeProjectEvent();
  event.tags = event.tags.filter(([name]) => name !== "d");
  assert.equal(eventToProjectContainer(event), null);
});

test("dedupProjectEvents keeps newest head per (pubkey, d)", () => {
  const oldHead = makeProjectEvent({ createdAt: 100 });
  const newHead = makeProjectEvent({ createdAt: 200 });
  const otherOwner = makeProjectEvent({ pubkey: OTHER, createdAt: 50 });
  const deduped = dedupProjectEvents([oldHead, newHead, otherOwner]);
  assert.equal(deduped.length, 2);
  assert.ok(deduped.includes(newHead));
  assert.ok(deduped.includes(otherOwner));
});

test("isProjectContainerDeleted requires owner-signed deletion", () => {
  const project = eventToProjectContainer(makeProjectEvent());
  const ownerDeletion = {
    pubkey: OWNER,
    tags: [["a", project.address]],
  };
  const foreignDeletion = {
    pubkey: OTHER,
    tags: [["a", project.address]],
  };
  assert.equal(isProjectContainerDeleted(project, [foreignDeletion]), false);
  assert.equal(isProjectContainerDeleted(project, [ownerDeletion]), true);
});

test("partitionByProject unions forward refs and back refs", () => {
  const projectA = eventToProjectContainer(
    makeProjectEvent({ dtag: "a", tags: [["channel", "c1"]] }),
  );
  const projectB = eventToProjectContainer(makeProjectEvent({ dtag: "b" }));
  const items = [
    { id: "c1", ref: null }, // forward-ref'd by A
    { id: "c2", ref: projectB.address }, // back-refs B
    { id: "c3", ref: null }, // unclaimed
    { id: "c4", ref: "30621:unknown:nope" }, // dangling back-ref
  ];
  const { byProject, unclaimed } = partitionByProject(
    [projectA, projectB],
    items,
    (item) => item.id,
    (project) => project.channelIds,
    (item) => item.ref,
  );
  assert.deepEqual(
    byProject.get(projectA.id).map((i) => i.id),
    ["c1"],
  );
  assert.deepEqual(
    byProject.get(projectB.id).map((i) => i.id),
    ["c2"],
  );
  assert.deepEqual(
    unclaimed.map((i) => i.id),
    ["c3", "c4"],
  );
});

test("partitionByProject lists multi-claimed items under each project", () => {
  const projectA = eventToProjectContainer(
    makeProjectEvent({ dtag: "a", tags: [["channel", "c1"]] }),
  );
  const projectB = eventToProjectContainer(
    makeProjectEvent({ dtag: "b", tags: [["channel", "c1"]] }),
  );
  const { byProject, unclaimed } = partitionByProject(
    [projectA, projectB],
    [{ id: "c1" }],
    (item) => item.id,
    (project) => project.channelIds,
  );
  assert.equal(byProject.get(projectA.id).length, 1);
  assert.equal(byProject.get(projectB.id).length, 1);
  assert.equal(unclaimed.length, 0);
});

test("partitionByProject drops General's claim when a real project also claims the item", () => {
  const general = eventToProjectContainer(
    makeProjectEvent({
      dtag: GENERAL_PROJECT_DTAG,
      name: "General",
      tags: [
        ["channel", "c1"],
        ["channel", "c2"],
      ],
    }),
  );
  const project = eventToProjectContainer(
    makeProjectEvent({ dtag: "tankloop", tags: [["channel", "c1"]] }),
  );
  const { byProject, unclaimed } = partitionByProject(
    [general, project],
    [{ id: "c1" }, { id: "c2" }],
    (item) => item.id,
    (p) => p.channelIds,
  );
  // c1 belongs to the real project only; c2 stays with General.
  assert.deepEqual(
    byProject.get(project.id).map((i) => i.id),
    ["c1"],
  );
  assert.deepEqual(
    byProject.get(general.id).map((i) => i.id),
    ["c2"],
  );
  assert.equal(unclaimed.length, 0);
});

test("partitionByProject drops General's claim when the item back-refs a real project", () => {
  const general = eventToProjectContainer(
    makeProjectEvent({
      dtag: GENERAL_PROJECT_DTAG,
      name: "General",
      tags: [["channel", "c1"]],
    }),
  );
  const project = eventToProjectContainer(makeProjectEvent({ dtag: "amas" }));
  const { byProject } = partitionByProject(
    [general, project],
    [{ id: "c1", ref: project.address }],
    (item) => item.id,
    (p) => p.channelIds,
    (item) => item.ref,
  );
  assert.equal(byProject.get(general.id).length, 0);
  assert.equal(byProject.get(project.id).length, 1);
});

test("partitionByChannelProject groups by the channel's owning project", () => {
  const channelToProject = new Map([
    ["chan-a", "proj-1"],
    ["chan-b", "proj-2"],
  ]);
  const workflows = [
    { id: "w1", channelId: "chan-a" },
    { id: "w2", channelId: "chan-b" },
    { id: "w3", channelId: "chan-a" },
    { id: "w4", channelId: "chan-unclaimed" },
    { id: "w5", channelId: null },
  ];
  const { byProject, unclaimed } = partitionByChannelProject(
    workflows,
    channelToProject,
  );
  assert.deepEqual(
    byProject.get("proj-1").map((w) => w.id),
    ["w1", "w3"],
  );
  assert.deepEqual(
    byProject.get("proj-2").map((w) => w.id),
    ["w2"],
  );
  assert.deepEqual(
    unclaimed.map((w) => w.id),
    ["w4", "w5"],
  );
});

test("sortProjectContainers puts general first, then by age", () => {
  const general = eventToProjectContainer(
    makeProjectEvent({ dtag: "general", createdAt: 500 }),
  );
  const older = eventToProjectContainer(
    makeProjectEvent({ dtag: "older", createdAt: 100 }),
  );
  const newer = eventToProjectContainer(
    makeProjectEvent({ dtag: "newer", createdAt: 300 }),
  );
  const sorted = sortProjectContainers([newer, general, older]);
  assert.deepEqual(
    sorted.map((project) => project.dtag),
    ["general", "older", "newer"],
  );
});

test("displayProjectsWithGeneral prepends the local placeholder without a real general", () => {
  const project = eventToProjectContainer(makeProjectEvent({ dtag: "alpha" }));
  const shown = displayProjectsWithGeneral([project]);
  assert.equal(shown.length, 2);
  assert.equal(shown[0].dtag, GENERAL_PROJECT_DTAG);
  assert.equal(shown[0].id, makeLocalGeneral().id);
  assert.equal(shown[1], project);
});

test("displayProjectsWithGeneral passes through when a real general exists", () => {
  const general = eventToProjectContainer(
    makeProjectEvent({ dtag: "general" }),
  );
  const project = eventToProjectContainer(makeProjectEvent({ dtag: "alpha" }));
  const projects = [general, project];
  assert.equal(displayProjectsWithGeneral(projects), projects);
});
