import assert from "node:assert/strict";
import { test } from "node:test";

import {
  dedupProjectEvents,
  displayProjectsWithGeneral,
  eventToProjectContainer,
  GENERAL_PROJECT_DTAG,
  isProjectContainerDeleted,
  isProjectMember,
  makeLocalGeneral,
  normalizeProjectColor,
  normalizeProjectMemberEntries,
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

test("eventToProjectContainer defaults to public when buzz-access is absent", () => {
  const project = eventToProjectContainer(makeProjectEvent());
  assert.equal(project.visibility, "public");
  assert.deepEqual(project.members, []);
});

test("eventToProjectContainer reads private visibility and p-tag members", () => {
  const event = makeProjectEvent({
    tags: [
      ["buzz-access", "private"],
      ["p", OTHER],
      ["p", OTHER.toUpperCase()], // dedupe is case-insensitive
      ["p", OWNER], // owner is implicit — excluded even if self-listed
      ["p", "not-a-pubkey"], // malformed — dropped
    ],
  });
  const project = eventToProjectContainer(event);
  assert.equal(project.visibility, "private");
  // Role-less legacy tags read as collaborator.
  assert.deepEqual(project.members, [{ pubkey: OTHER, role: "collaborator" }]);
});

test("eventToProjectContainer reads roles from arity-4 p tags", () => {
  const THIRD = "c".repeat(64);
  const project = eventToProjectContainer(
    makeProjectEvent({
      tags: [
        ["p", OTHER, "", "owner"],
        ["p", THIRD, "", "not-a-role"], // unknown roles fall back
      ],
    }),
  );
  assert.deepEqual(project.members, [
    { pubkey: OTHER, role: "owner" },
    { pubkey: THIRD, role: "collaborator" },
  ]);
});

test("eventToProjectContainer treats any non-private buzz-access value as public", () => {
  const project = eventToProjectContainer(
    makeProjectEvent({ tags: [["buzz-access", "public"]] }),
  );
  assert.equal(project.visibility, "public");
  const legacyValue = eventToProjectContainer(
    makeProjectEvent({ tags: [["buzz-access", "unlisted"]] }),
  );
  assert.equal(legacyValue.visibility, "public");
});

test("isProjectMember treats the owner as an implicit member", () => {
  const project = eventToProjectContainer(
    makeProjectEvent({
      tags: [
        ["buzz-access", "private"],
        ["p", OTHER],
      ],
    }),
  );
  assert.equal(isProjectMember(project, OWNER), true);
  assert.equal(isProjectMember(project, OWNER.toUpperCase()), true);
  assert.equal(isProjectMember(project, OTHER), true);
  assert.equal(isProjectMember(project, "c".repeat(64)), false);
});

test("normalizeProjectMemberEntries upgrades legacy snapshot shapes", () => {
  // Pre-roles snapshots stored bare pubkey strings.
  assert.deepEqual(normalizeProjectMemberEntries([OTHER.toUpperCase()]), [
    { pubkey: OTHER, role: "collaborator" },
  ]);
  // Current shape passes through; unknown roles fall back to collaborator.
  assert.deepEqual(
    normalizeProjectMemberEntries([
      { pubkey: OTHER, role: "viewer" },
      { pubkey: OWNER, role: "bogus" },
    ]),
    [
      { pubkey: OTHER, role: "viewer" },
      { pubkey: OWNER, role: "collaborator" },
    ],
  );
  // Garbage entries are dropped, not fatal.
  assert.deepEqual(normalizeProjectMemberEntries([null, 42, {}]), []);
  assert.deepEqual(normalizeProjectMemberEntries("nope"), []);
});

test("makeLocalGeneral is public with no members", () => {
  const general = makeLocalGeneral();
  assert.equal(general.dtag, GENERAL_PROJECT_DTAG);
  assert.equal(general.visibility, "public");
  assert.deepEqual(general.members, []);
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
    created_at: 100,
    tags: [["a", project.address]],
  };
  const foreignDeletion = {
    pubkey: OTHER,
    created_at: 100,
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

test("sortProjectContainers puts general first, then by name", () => {
  const general = eventToProjectContainer(
    makeProjectEvent({ dtag: "general", name: "General", createdAt: 500 }),
  );
  const zebra = eventToProjectContainer(
    makeProjectEvent({ dtag: "zebra", name: "Zebra", createdAt: 100 }),
  );
  const apple = eventToProjectContainer(
    makeProjectEvent({ dtag: "apple", name: "apple", createdAt: 300 }),
  );
  const sorted = sortProjectContainers([zebra, general, apple]);
  // Case-insensitive, and creation order (zebra oldest) is not consulted.
  assert.deepEqual(
    sorted.map((project) => project.dtag),
    ["general", "apple", "zebra"],
  );
});

test("sortProjectContainers order survives a republish of the head", () => {
  // The regression this sort exists for: adding a sub-item republishes the
  // kind:30621 head with a fresh created_at, which used to move the project.
  const build = (createdAt) => [
    eventToProjectContainer(
      makeProjectEvent({ dtag: "general", name: "General", createdAt: 1 }),
    ),
    eventToProjectContainer(
      makeProjectEvent({ dtag: "alpha", name: "Alpha", createdAt: 10 }),
    ),
    eventToProjectContainer(
      makeProjectEvent({ dtag: "beta", name: "Beta", createdAt }),
    ),
  ];
  const before = sortProjectContainers(build(20)).map((p) => p.dtag);
  const after = sortProjectContainers(build(9_999)).map((p) => p.dtag);
  assert.deepEqual(before, ["general", "alpha", "beta"]);
  assert.deepEqual(after, before);
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

test("eventToProjectContainer reads icon and normalized color tags", () => {
  const event = makeProjectEvent({
    tags: [
      ["icon", "🐝"],
      ["color", "#3B82F6"],
    ],
  });
  const project = eventToProjectContainer(event);
  assert.equal(project.icon, "🐝");
  // Hex is normalized to lowercase for stable comparisons.
  assert.equal(project.color, "#3b82f6");
});

test("eventToProjectContainer treats absent or malformed color as unset", () => {
  assert.equal(eventToProjectContainer(makeProjectEvent()).icon, null);
  assert.equal(eventToProjectContainer(makeProjectEvent()).color, null);
  for (const bad of ["red", "#fff", "#gggggg", "3b82f6", "#3b82f6ff"]) {
    const project = eventToProjectContainer(
      makeProjectEvent({ tags: [["color", bad]] }),
    );
    assert.equal(project.color, null, `expected ${bad} to read as unset`);
  }
});

test("normalizeProjectColor accepts only #rrggbb and lowercases it", () => {
  assert.equal(normalizeProjectColor("#AABBCC"), "#aabbcc");
  assert.equal(normalizeProjectColor("#aabbcc"), "#aabbcc");
  assert.equal(normalizeProjectColor("aabbcc"), null);
  assert.equal(normalizeProjectColor("#abc"), null);
  assert.equal(normalizeProjectColor(undefined), null);
  assert.equal(normalizeProjectColor(null), null);
});

test("makeLocalGeneral carries no icon or color", () => {
  const general = makeLocalGeneral();
  assert.equal(general.icon, null);
  assert.equal(general.color, null);
});

for (const [deletedAt, expected] of [
  [99, false],
  [100, true],
  [101, true],
]) {
  test(`project head at 100 is deleted by address tombstone at ${deletedAt}: ${expected}`, () => {
    const project = eventToProjectContainer(
      makeProjectEvent({ createdAt: 100 }),
    );
    assert.equal(
      isProjectContainerDeleted(project, [
        {
          pubkey: OWNER,
          created_at: deletedAt,
          tags: [["a", project.address]],
        },
      ]),
      expected,
    );
  });
}

test("deduped recreated project remains visible after an older address tombstone", () => {
  const heads = dedupProjectEvents([
    makeProjectEvent({ createdAt: 100 }),
    makeProjectEvent({ createdAt: 300 }),
  ]);
  const deletion = {
    pubkey: OWNER,
    created_at: 200,
    tags: [["a", `30621:${OWNER}:platform`]],
  };
  const visible = heads
    .map(eventToProjectContainer)
    .filter(
      (project) => project && !isProjectContainerDeleted(project, [deletion]),
    );
  assert.equal(visible.length, 1);
  assert.equal(visible[0].createdAt, 300);
});
