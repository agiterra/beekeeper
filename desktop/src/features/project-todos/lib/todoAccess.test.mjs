import assert from "node:assert/strict";
import { test } from "node:test";

import { todoWriteAccess } from "@/features/project-todos/lib/todoAccess";
import {
  nextTodoCreatedAt,
  rankForIndex,
} from "@/features/project-todos/lib/todoMutations";
import {
  formatDue,
  isOverdue,
  todoPerson,
} from "@/features/project-todos/lib/todoPeople";
import {
  readFromEvents,
  todoOpsFilter,
  todoTargetKey,
} from "@/features/project-todos/lib/todoQueries";
import { LOCAL_GENERAL_ID } from "@/features/projects-container/lib/projectContainerModel";

const OWNER = "1".repeat(64);
const COLLAB = "2".repeat(64);
const VIEWER = "3".repeat(64);
const STRANGER = "4".repeat(64);

function project(overrides = {}) {
  return {
    id: `${OWNER}:tank-loop`,
    dtag: "tank-loop",
    owner: OWNER,
    name: "Tank Loop",
    address: `30621:${OWNER}:tank-loop`,
    visibility: "private",
    members: [],
    ...overrides,
  };
}

const ROSTER = [
  { pubkey: COLLAB, role: "collaborator" },
  { pubkey: VIEWER, role: "viewer" },
];
const READY = { isOwner: false, isLoading: false };

test("write access mirrors the relay's project-scoped admission rule", () => {
  assert.deepEqual(
    todoWriteAccess(OWNER, project(), ROSTER, { ...READY, isOwner: true }),
    { kind: "writable" },
  );
  assert.deepEqual(todoWriteAccess(COLLAB, project(), ROSTER, READY), {
    kind: "writable",
  });
  assert.equal(
    todoWriteAccess(VIEWER, project(), ROSTER, READY).kind,
    "read-only",
  );
  assert.match(
    todoWriteAccess(VIEWER, project(), ROSTER, READY).reason,
    /viewer/,
  );
  assert.match(
    todoWriteAccess(STRANGER, project(), ROSTER, READY).reason,
    /not a member/,
  );
  // A public project admits any community member.
  assert.deepEqual(
    todoWriteAccess(STRANGER, project({ visibility: "public" }), ROSTER, READY),
    { kind: "writable" },
  );
  // Roster roles are compared case-insensitively.
  assert.deepEqual(
    todoWriteAccess(COLLAB.toUpperCase(), project(), ROSTER, READY),
    { kind: "writable" },
  );
});

test("no coordinate and still-loading are their own states, never a guess", () => {
  assert.deepEqual(
    todoWriteAccess(
      OWNER,
      project({ id: LOCAL_GENERAL_ID, owner: "" }),
      [],
      READY,
    ),
    { kind: "no-coordinate" },
  );
  assert.deepEqual(todoWriteAccess(OWNER, null, [], READY), {
    kind: "no-coordinate",
  });
  assert.deepEqual(
    todoWriteAccess(OWNER, project(), ROSTER, { ...READY, isLoading: true }),
    { kind: "loading" },
  );
  assert.deepEqual(todoWriteAccess(null, project(), ROSTER, READY), {
    kind: "loading",
  });
});

function item(id, rank) {
  return {
    id,
    listId: "l".repeat(32),
    text: id,
    done: false,
    rank,
    assignee: null,
    due: null,
    createdAt: 1,
    createdBy: OWNER,
    updatedAt: 1,
    completedAt: null,
    completedBy: null,
  };
}

test("rankForIndex places between neighbours and excludes the mover", () => {
  const a = "a".repeat(32);
  const b = "b".repeat(32);
  const c = "c".repeat(32);
  const open = [item(a, "a0"), item(b, "a1"), item(c, "a2")];
  assert.equal(rankForIndex(open, null, null), "a3");
  assert.equal(rankForIndex(open, 0, null), "Zz");
  assert.equal(rankForIndex(open, 1, null), "a0V");
  assert.equal(rankForIndex(open, 99, null), "a3");
  // The dnd index is the `over` item's position in the current list; the
  // mover is excluded so that index reads the same going up or down.
  assert.equal(rankForIndex(open, 0, c), "Zz");
  assert.equal(rankForIndex(open, 2, a), "a3");
  assert.equal(rankForIndex(open, 1, a), "a1V");
  assert.equal(rankForIndex([], 0, null), "a0");
});

test("the created_at bump never sorts below the latest op on its target", () => {
  assert.equal(nextTodoCreatedAt(0, 1_000), 1_000);
  assert.equal(nextTodoCreatedAt(999, 1_000), 1_000);
  assert.equal(nextTodoCreatedAt(1_000, 1_000), 1_001);
  assert.equal(nextTodoCreatedAt(1_500, 1_000), 1_501);
});

test("readFromEvents dedupes by id and tracks the latest stamp per target", () => {
  const coordinate = `30621:${OWNER}:tank-loop`;
  const listId = "1".repeat(32);
  const itemId = "a".repeat(32);
  const event = (id, created_at, tags, content) => ({
    id,
    pubkey: OWNER,
    created_at,
    kind: 44248,
    tags: [
      ["a", coordinate],
      ["td-v", "td1-1"],
      ["td-vis", "project"],
      ...tags,
    ],
    content,
    sig: "",
  });
  const create = event(
    "e1".padStart(64, "0"),
    100,
    [
      ["td-op", "list.create"],
      ["td-list", listId],
    ],
    JSON.stringify({
      schema: "buzz-project-todo/v1",
      op: "list.create",
      listId,
      title: "L",
      visibility: "project",
    }),
  );
  const add = event(
    "e2".padStart(64, "0"),
    120,
    [
      ["td-op", "item.add"],
      ["td-list", listId],
      ["td-item", itemId],
    ],
    JSON.stringify({
      schema: "buzz-project-todo/v1",
      op: "item.add",
      listId,
      itemId,
      text: "one",
      rank: "a0",
    }),
  );
  const read = readFromEvents(coordinate, [create, add, add], false);
  assert.equal(read.events.length, 2);
  assert.equal(read.digest.lists[0].open[0].text, "one");
  assert.equal(read.latestByTarget[todoTargetKey(listId, itemId)], 120);
  assert.equal(read.latestByTarget[todoTargetKey(listId, null)], 120);
  assert.equal(read.truncated, false);
  assert.deepEqual(todoOpsFilter(coordinate), {
    kinds: [44248],
    "#a": [coordinate],
    limit: 1000,
  });
});

test("people resolve to a name, an avatar and the agent flag", () => {
  const agent = "5".repeat(64);
  const profiles = {
    [COLLAB]: { displayName: "Ada", avatarUrl: "u", ownerPubkey: null },
    [agent]: { displayName: "Kiln", avatarUrl: null, ownerPubkey: OWNER },
  };
  assert.deepEqual(todoPerson(COLLAB, profiles), {
    pubkey: COLLAB,
    name: "Ada",
    avatarUrl: "u",
    isAgent: false,
  });
  assert.equal(todoPerson(agent, profiles).isAgent, true);
  assert.match(todoPerson(STRANGER, profiles).name, /^4{8}…4{4}$/);
});

test("due dates read against the viewer's local today", () => {
  assert.equal(isOverdue("2026-09-16", "2026-09-17"), true);
  assert.equal(isOverdue("2026-09-17", "2026-09-17"), false);
  assert.match(formatDue("2026-10-01", "2026-09-17"), /Oct 1/);
  assert.match(formatDue("2027-10-01", "2026-09-17"), /2027/);
});
