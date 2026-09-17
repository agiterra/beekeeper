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
  const ops = [
    { op: "list.create", listId, title: "Launch" },
    { op: "list.title", listId, title: "Launch v2" },
    { op: "list.archived", listId, archived: true },
    { op: "item.add", listId, itemId, text: "Write the NIP", rank: "a0" },
    { op: "item.text", listId, itemId, text: "Write\tthe NIP\nwith examples" },
    { op: "item.done", listId, itemId, done: true },
    { op: "item.assignee", listId, itemId, assignee: "b".repeat(64) },
    { op: "item.assignee", listId, itemId, assignee: null },
    { op: "item.due", listId, itemId, due: "2028-02-29" },
    { op: "item.due", listId, itemId, due: null },
    { op: "item.rank", listId, itemId, rank: "a0V" },
    { op: "item.remove", listId, itemId },
  ];
  for (const op of ops) {
    const content = encodeTodoOpContent(op);
    assert.deepEqual(decodeTodoOp(content), op, content);
    assert.equal(JSON.parse(content).schema, "buzz-project-todo/v1");
    const tags = todoOpTags(coordinate, op);
    assert.deepEqual(tags.slice(0, 4), [
      ["a", coordinate],
      ["td-v", "td1-1"],
      ["td-op", op.op],
      ["td-list", listId],
    ]);
    if (op.op.startsWith("item.")) {
      assert.deepEqual(tags[4], ["td-item", itemId]);
    } else {
      assert.equal(tags.length, 4);
    }
  }
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
  const bad = (patch) => decodeTodoOp(JSON.stringify({ ...base, ...patch }));
  assert.match(bad({ priority: "high" }).error, /unsupported field "priority"/);
  const { rank: _rank, ...missing } = base;
  assert.match(
    decodeTodoOp(JSON.stringify(missing)).error,
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
