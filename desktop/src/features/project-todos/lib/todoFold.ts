/**
 * The project to-do fold — TypeScript twin of
 * `crates/buzz-core/src/project_todo_fold.rs`, bound to
 * `conformance/project-todo-fold/` (CONTRACT.md states the rules; the
 * vectors pin them byte for byte, key order included).
 *
 * Pure and total: any bag of events in, one digest out, the same digest from
 * every client. No imports outside this directory.
 */
import {
  PROJECT_TODO_OP_KIND,
  decodeTodoOp,
  isTodoVisibility,
  type TodoOp,
  type TodoVisibility,
} from "./todoOp.ts";

export const PROJECT_TODO_DIGEST_SCHEMA = "buzz-project-todo-digest/v1";

/** The subset of a relay event the fold reads. */
export type TodoFoldEvent = {
  id: string;
  pubkey: string;
  created_at: number;
  kind: number;
  tags: string[][];
  content: string;
};

export type TodoItem = {
  id: string;
  listId: string;
  text: string;
  done: boolean;
  rank: string;
  assignee: string | null;
  due: string | null;
  createdAt: number;
  createdBy: string;
  updatedAt: number;
  completedAt: number | null;
  completedBy: string | null;
};

export type TodoList = {
  id: string;
  title: string;
  /** Fixed at creation. */
  visibility: TodoVisibility;
  archived: boolean;
  /** Shown in the project sidebar; shared, though a personal list's pin is only ever seen by its owner. */
  pinned: boolean;
  createdAt: number;
  createdBy: string;
  updatedAt: number;
  open: TodoItem[];
  completed: TodoItem[];
};

export type ProjectTodoDigest = {
  schema: typeof PROJECT_TODO_DIGEST_SCHEMA;
  project: string;
  ignored: number;
  lists: TodoList[];
};

type OpKey = { createdAt: number; id: string };

function keyLess(a: OpKey, b: OpKey): boolean {
  return (
    a.createdAt < b.createdAt || (a.createdAt === b.createdAt && a.id < b.id)
  );
}

type Slot<T> = { key: OpKey; value: T };

function setField<T>(slot: Slot<T>, key: OpKey, value: T): void {
  if (keyLess(slot.key, key)) {
    slot.key = key;
    slot.value = value;
  }
}

type ListState = {
  title: Slot<string>;
  visibility: TodoVisibility;
  archived: Slot<boolean>;
  pinned: Slot<boolean>;
  createdAt: number;
  createdBy: string;
  updatedAt: number;
};

type ItemState = {
  text: Slot<string>;
  rank: Slot<string>;
  done: Slot<{ done: boolean; by: string | null }>;
  assignee: Slot<string | null>;
  due: Slot<string | null>;
  createdAt: number;
  createdBy: string;
  updatedAt: number;
};

type Decoded = { key: OpKey; pubkey: string; op: TodoOp };

/** Normalize a coordinate's hex case; `null` when it is not a 30621 coordinate. */
function normalizeCoordinate(value: string): string | null {
  const parts = value.split(":");
  if (parts.length < 3 || parts[0] !== "30621") return null;
  const hex = parts[1] ?? "";
  if (!/^[0-9a-fA-F]{64}$/.test(hex)) return null;
  const dtag = parts.slice(2).join(":");
  if (dtag.length === 0) return null;
  return `30621:${hex.toLowerCase()}:${dtag}`;
}

function decode(project: string, event: TodoFoldEvent): TodoOp | null {
  if (event.kind !== PROJECT_TODO_OP_KIND) return null;
  const aTags = event.tags.filter((t) => t[0] === "a");
  if (aTags.length !== 1) return null;
  const coordinate = aTags[0]?.[1];
  if (typeof coordinate !== "string") return null;
  if (normalizeCoordinate(coordinate) !== project) return null;
  const visTags = event.tags.filter((t) => t[0] === "td-vis");
  if (visTags.length !== 1) return null;
  const visibility = visTags[0]?.[1];
  if (!isTodoVisibility(visibility)) return null;
  const op = decodeTodoOp(event.content, visibility);
  return "error" in op ? null : op;
}

/** Fold `events` for `project` (a canonical coordinate). */
export function foldProjectTodos(
  project: string,
  events: readonly TodoFoldEvent[],
): ProjectTodoDigest {
  let ignored = 0;
  const seen = new Set<string>();
  const ops: Decoded[] = [];
  for (const event of events) {
    if (seen.has(event.id)) continue;
    seen.add(event.id);
    const op = decode(project, event);
    if (op === null) {
      ignored++;
      continue;
    }
    ops.push({
      key: { createdAt: event.created_at, id: event.id },
      pubkey: event.pubkey,
      op,
    });
  }
  ops.sort((a, b) =>
    keyLess(a.key, b.key) ? -1 : keyLess(b.key, a.key) ? 1 : 0,
  );

  const lists = new Map<string, ListState>();
  for (const d of ops) {
    if (d.op.op !== "list.create") continue;
    if (lists.has(d.op.listId)) {
      ignored++;
      continue;
    }
    lists.set(d.op.listId, {
      title: { key: d.key, value: d.op.title },
      visibility: d.op.visibility,
      archived: { key: d.key, value: false },
      pinned: { key: d.key, value: false },
      createdAt: d.key.createdAt,
      createdBy: d.pubkey,
      updatedAt: d.key.createdAt,
    });
  }
  // Every other op must agree with its list's visibility, and a personal
  // list takes ops from its creator only. Ops on a list that does not exist
  // are counted where they are handled below.
  const admitted: Decoded[] = [];
  for (const d of ops) {
    if (d.op.op === "list.create") {
      admitted.push(d);
      continue;
    }
    const list = lists.get(d.op.listId);
    if (!list) {
      admitted.push(d);
      continue;
    }
    const ok =
      d.op.visibility === list.visibility &&
      (list.visibility === "project" || d.pubkey === list.createdBy);
    if (ok) admitted.push(d);
    else ignored++;
  }
  ops.length = 0;
  ops.push(...admitted);

  const itemKey = (listId: string, itemId: string) => `${listId}/${itemId}`;
  const items = new Map<string, ItemState>();
  const removed = new Set<string>();
  for (const d of ops) {
    if (d.op.op !== "item.add") continue;
    const list = lists.get(d.op.listId);
    if (!list || items.has(itemKey(d.op.listId, d.op.itemId))) {
      ignored++;
      continue;
    }
    list.updatedAt = Math.max(list.updatedAt, d.key.createdAt);
    items.set(itemKey(d.op.listId, d.op.itemId), {
      text: { key: d.key, value: d.op.text },
      rank: { key: d.key, value: d.op.rank },
      done: { key: d.key, value: { done: false, by: null } },
      assignee: { key: d.key, value: null },
      due: { key: d.key, value: null },
      createdAt: d.key.createdAt,
      createdBy: d.pubkey,
      updatedAt: d.key.createdAt,
    });
  }
  for (const d of ops) {
    if (d.op.op !== "item.remove") continue;
    const target = itemKey(d.op.listId, d.op.itemId);
    if (items.has(target)) {
      removed.add(target);
      const list = lists.get(d.op.listId);
      if (list) list.updatedAt = Math.max(list.updatedAt, d.key.createdAt);
    } else {
      ignored++;
    }
  }

  for (const d of ops) {
    const list = lists.get(d.op.listId);
    if (!list) {
      if (
        d.op.op !== "list.create" &&
        d.op.op !== "item.add" &&
        d.op.op !== "item.remove"
      ) {
        ignored++;
      }
      continue;
    }
    const op = d.op;
    if (
      op.op === "list.create" ||
      op.op === "item.add" ||
      op.op === "item.remove"
    )
      continue;
    if (op.op === "list.title") {
      setField(list.title, d.key, op.title);
      list.updatedAt = Math.max(list.updatedAt, d.key.createdAt);
      continue;
    }
    if (op.op === "list.archived") {
      setField(list.archived, d.key, op.archived);
      list.updatedAt = Math.max(list.updatedAt, d.key.createdAt);
      continue;
    }
    if (op.op === "list.pinned") {
      setField(list.pinned, d.key, op.pinned);
      list.updatedAt = Math.max(list.updatedAt, d.key.createdAt);
      continue;
    }
    const target = itemKey(op.listId, op.itemId);
    if (removed.has(target)) continue;
    const item = items.get(target);
    if (!item) {
      ignored++;
      continue;
    }
    switch (op.op) {
      case "item.text":
        setField(item.text, d.key, op.text);
        break;
      case "item.done":
        setField(item.done, d.key, {
          done: op.done,
          by: op.done ? d.pubkey : null,
        });
        break;
      case "item.assignee":
        setField(item.assignee, d.key, op.assignee);
        break;
      case "item.due":
        setField(item.due, d.key, op.due);
        break;
      case "item.rank":
        setField(item.rank, d.key, op.rank);
        break;
    }
    item.updatedAt = Math.max(item.updatedAt, d.key.createdAt);
    list.updatedAt = Math.max(list.updatedAt, d.key.createdAt);
  }

  const outLists: TodoList[] = [];
  for (const [listId, state] of lists) {
    const open: { rank: string; row: TodoItem }[] = [];
    const completed: { key: OpKey; row: TodoItem }[] = [];
    for (const [key, item] of items) {
      const [owner, itemId] = key.split("/") as [string, string];
      if (owner !== listId || removed.has(key)) continue;
      const done = item.done.value.done;
      const row: TodoItem = {
        id: itemId,
        listId,
        text: item.text.value,
        done,
        rank: item.rank.value,
        assignee: item.assignee.value,
        due: item.due.value,
        createdAt: item.createdAt,
        createdBy: item.createdBy,
        updatedAt: item.updatedAt,
        completedAt: done ? item.done.key.createdAt : null,
        completedBy: item.done.value.by,
      };
      if (done) completed.push({ key: item.done.key, row });
      else open.push({ rank: item.rank.value, row });
    }
    open.sort((a, b) =>
      a.rank < b.rank
        ? -1
        : a.rank > b.rank
          ? 1
          : a.row.id < b.row.id
            ? -1
            : a.row.id > b.row.id
              ? 1
              : 0,
    );
    completed.sort((a, b) =>
      keyLess(a.key, b.key) ? 1 : keyLess(b.key, a.key) ? -1 : 0,
    );
    outLists.push({
      id: listId,
      title: state.title.value,
      visibility: state.visibility,
      archived: state.archived.value,
      pinned: state.pinned.value,
      createdAt: state.createdAt,
      createdBy: state.createdBy,
      updatedAt: state.updatedAt,
      open: open.map((o) => o.row),
      completed: completed.map((c) => c.row),
    });
  }
  outLists.sort((a, b) =>
    a.createdAt !== b.createdAt
      ? a.createdAt - b.createdAt
      : a.id < b.id
        ? -1
        : a.id > b.id
          ? 1
          : 0,
  );

  return {
    schema: PROJECT_TODO_DIGEST_SCHEMA,
    project,
    ignored,
    lists: outLists,
  };
}
