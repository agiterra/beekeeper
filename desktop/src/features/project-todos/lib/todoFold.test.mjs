import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import {
  isValidRank,
  rankBetween,
} from "@/features/project-todos/lib/fractionalRank";
import { foldProjectTodos } from "@/features/project-todos/lib/todoFold";
import {
  PROJECT_TODO_OP_KIND,
  decodeTodoOp,
  dueDateError,
  encodeTodoOpContent,
  newTodoId,
  todoOpTags,
  todoTextError,
} from "@/features/project-todos/lib/todoOp";
import { KIND_PROJECT_TODO_OP } from "@/shared/constants/kinds";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const FIXTURES = path.resolve(
  HERE,
  "../../../../../conformance/project-todo-fold/fixtures",
);
const folds = JSON.parse(
  readFileSync(path.join(FIXTURES, "fold-vectors.json"), "utf8"),
);
const ranks = JSON.parse(
  readFileSync(path.join(FIXTURES, "rank-vectors.json"), "utf8"),
);

test("the local kind pin matches the shared constant", () => {
  assert.equal(PROJECT_TODO_OP_KIND, KIND_PROJECT_TODO_OP);
  assert.equal(PROJECT_TODO_OP_KIND, 44248);
});

test("every fold vector folds byte-identically", () => {
  assert.ok(folds.cases.length > 0);
  for (const vector of folds.cases) {
    const digest = foldProjectTodos(vector.project, vector.events);
    assert.equal(
      JSON.stringify(digest, null, 2),
      JSON.stringify(vector.expected, null, 2),
      vector.name,
    );
  }
});

test("every rank vector mints the pinned key and refuses the invalid ones", () => {
  for (const [after, before, expected] of ranks.between) {
    assert.equal(rankBetween(after, before), expected, `${after}..${before}`);
  }
  for (const rank of ranks.invalid) {
    assert.equal(isValidRank(rank), false, rank);
  }
  assert.throws(() => rankBetween("a1", "a0"));
});

test("ops round-trip through content and carry the matching tags", () => {
  const listId = newTodoId();
  const itemId = newTodoId();
  const coordinate = `30621:${"a".repeat(64)}:tank-loop`;
  const project = "project";
  const ops = [
    { op: "list.create", listId, title: "Launch", visibility: project },
    { op: "list.title", listId, title: "Launch v2", visibility: project },
    { op: "list.archived", listId, archived: true, visibility: project },
    { op: "list.pinned", listId, pinned: true, visibility: project },
    {
      op: "list.create",
      listId,
      title: "Mine",
      visibility: "personal",
    },
    {
      op: "item.add",
      listId,
      itemId,
      text: "Write the NIP",
      rank: "a0",
      visibility: project,
    },
    {
      op: "item.text",
      listId,
      itemId,
      text: "Write\tthe NIP\nwith examples",
      visibility: project,
    },
    { op: "item.done", listId, itemId, done: true, visibility: project },
    {
      op: "item.assignee",
      listId,
      itemId,
      assignee: "b".repeat(64),
      visibility: project,
    },
    {
      op: "item.assignee",
      listId,
      itemId,
      assignee: null,
      visibility: "personal",
    },
    { op: "item.due", listId, itemId, due: "2028-02-29", visibility: project },
    { op: "item.due", listId, itemId, due: null, visibility: project },
    { op: "item.rank", listId, itemId, rank: "a0V", visibility: project },
    { op: "item.remove", listId, itemId, visibility: project },
  ];
  for (const op of ops) {
    const content = encodeTodoOpContent(op);
    const parsed = JSON.parse(content);
    assert.equal(parsed.schema, "buzz-project-todo/v1");
    // Only list.create carries visibility in content; every op carries it
    // as the td-vis tag.
    assert.equal("visibility" in parsed, op.op === "list.create");
    assert.deepEqual(decodeTodoOp(content, op.visibility), op, content);
    const tags = todoOpTags(coordinate, op);
    assert.deepEqual(tags.slice(0, 5), [
      ["a", coordinate],
      ["td-v", "td1-1"],
      ["td-op", op.op],
      ["td-list", listId],
      ["td-vis", op.visibility],
    ]);
    if (op.op.startsWith("item.")) {
      assert.deepEqual(tags[5], ["td-item", itemId]);
    } else {
      assert.equal(tags.length, 5);
    }
  }
  // A create whose content disagrees with its tag is refused.
  const personalCreate = encodeTodoOpContent(ops[4]);
  assert.match(
    decodeTodoOp(personalCreate, "project").error,
    /does not match its td-vis tag/,
  );
});

test("the content key set is exact and values are validated", () => {
  const listId = newTodoId();
  const itemId = newTodoId();
  const base = {
    schema: "buzz-project-todo/v1",
    op: "item.add",
    listId,
    itemId,
    text: "t",
    rank: "a0",
  };
  const bad = (patch) =>
    decodeTodoOp(JSON.stringify({ ...base, ...patch }), "project");
  assert.match(bad({ priority: "high" }).error, /unsupported field "priority"/);
  const { rank: _rank, ...missing } = base;
  assert.match(
    decodeTodoOp(JSON.stringify(missing), "project").error,
    /missing field "rank"/,
  );
  assert.match(bad({ listId: "short" }).error, /listId/);
  assert.match(bad({ text: "   " }).error, /blank/);
  assert.match(bad({ text: `a${String.fromCharCode(7)}b` }).error, /control/);
  assert.match(bad({ rank: "a0V0" }).error, /rank/);
  assert.match(bad({ schema: "buzz-project-todo/v0" }).error, /schema/);
  assert.match(bad({ op: "item.rename" }).error, /unknown project todo op/);
  assert.match(
    decodeTodoOp(
      JSON.stringify({
        schema: "buzz-project-todo/v1",
        op: "item.assignee",
        listId,
        itemId,
      }),
      "project",
    ).error,
    /missing field "assignee"/,
  );
  assert.match(
    decodeTodoOp(
      JSON.stringify({
        schema: "buzz-project-todo/v1",
        op: "item.assignee",
        listId,
        itemId,
        assignee: "ABC",
      }),
      "project",
    ).error,
    /assignee/,
  );
  assert.equal(
    todoTextError("text", "x".repeat(1025)),
    "todo text exceeds 1024 bytes",
  );
  assert.equal(todoTextError("text", "fine"), null);
});

test("due dates are real calendar days", () => {
  for (const ok of [
    "1970-01-01",
    "2026-02-28",
    "2024-02-29",
    "2000-02-29",
    "9999-12-31",
  ]) {
    assert.equal(dueDateError(ok), null, ok);
  }
  for (const bad of [
    "2026-2-28",
    "2026-02-30",
    "2026-13-01",
    "2100-02-29",
    "1969-12-31",
    "2026/02/28",
    "",
  ]) {
    assert.notEqual(dueDateError(bad), null, bad);
  }
});

// Mixed-version agreement: a reader that predates `item.status` decodes it
// as an unknown op and drops it. Whatever order `item.done` and
// `item.status` arrive in — a pair split by another writer, a pair whose
// second op was refused, a status written from a stale read — the two
// readers must agree on whether the item is done, when and by whom.
const MV_PROJECT = `30621:${"a".repeat(64)}:p`;
const MV_LIST = "1".repeat(32);
const MV_ITEM = "2".repeat(32);
const MV_STATUSES = [
  "triage",
  "backlog",
  "todo",
  "in_progress",
  "done",
  "canceled",
];

function mvEvent(n, author, createdAt, op) {
  const full = { listId: MV_LIST, visibility: "project", ...op };
  return {
    id: n.toString(16).padStart(64, "0"),
    pubkey: author.repeat(64),
    created_at: createdAt,
    kind: 44248,
    tags: todoOpTags(MV_PROJECT, full),
    content: encodeTodoOpContent(full),
  };
}

function mvSeed() {
  return [
    mvEvent(1, "a", 1, { op: "list.create", title: "L" }),
    mvEvent(2, "a", 1, {
      op: "item.add",
      itemId: MV_ITEM,
      text: "t",
      rank: "a0",
    }),
  ];
}

const mvDone = (done) => ({ op: "item.done", itemId: MV_ITEM, done });
const mvStatus = (status) => ({ op: "item.status", itemId: MV_ITEM, status });

function mvItem(events) {
  const list = foldProjectTodos(MV_PROJECT, events).lists[0];
  return [...list.open, ...list.completed][0];
}

function mvOldReader(events) {
  return mvItem(
    events.filter((event) => !event.content.includes('"op":"item.status"')),
  );
}

test("readers of different ages agree on done under any interleaving", () => {
  let state = 0x2545f491;
  const next = (bound) => {
    state ^= state << 13;
    state ^= state >>> 17;
    state ^= state << 5;
    return (state >>> 0) % bound;
  };
  for (let round = 0; round < 2000; round += 1) {
    const events = mvSeed();
    const count = 1 + next(8);
    for (let n = 0; n < count; n += 1) {
      const author = next(2) === 0 ? "b" : "c";
      const createdAt = 1 + next(6);
      const op =
        next(3) === 0 ? mvDone(next(2) === 0) : mvStatus(MV_STATUSES[next(6)]);
      events.push(mvEvent(100 + n * 7 + next(5), author, createdAt, op));
    }
    const fresh = mvItem(events);
    const old = mvOldReader(events);
    const why = JSON.stringify(events.slice(2));
    assert.equal(fresh.done, old.done, why);
    assert.equal(fresh.completedAt, old.completedAt, why);
    assert.equal(fresh.completedBy, old.completedBy, why);
    assert.equal(
      fresh.status === "done" || fresh.status === "canceled",
      fresh.done,
      why,
    );
  }
});

test("a stale-read Canceled published as a pair wins for every reader; alone it changes nothing", () => {
  // B read Done; A reopened; B, still reading Done, sets Canceled.
  const base = [
    ...mvSeed(),
    mvEvent(10, "b", 2, mvDone(true)),
    mvEvent(11, "b", 3, mvStatus("done")),
    mvEvent(12, "a", 4, mvDone(false)),
  ];
  const paired = [
    ...base,
    mvEvent(13, "b", 5, mvDone(true)),
    mvEvent(14, "b", 6, mvStatus("canceled")),
  ];
  assert.equal(mvItem(paired).status, "canceled");
  assert.equal(mvOldReader(paired).done, true);
  const statusOnly = [...base, mvEvent(13, "b", 5, mvStatus("canceled"))];
  assert.equal(mvItem(statusOnly).status, "todo");
  assert.equal(mvItem(statusOnly).done, false);
  assert.equal(mvOldReader(statusOnly).done, false);
});
