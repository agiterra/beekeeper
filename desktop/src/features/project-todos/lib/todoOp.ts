/**
 * The kind 44248 project to-do op — TypeScript twin of
 * `crates/beekeeper-core/src/project_todo.rs` and `docs/nips/NIP-TD.md`.
 *
 * Every op sets exactly one field. That is the whole concurrency story: two
 * people editing different fields of one item never race, and the same field
 * resolves by `(created_at, id)` in the fold. The content key set is exact per
 * op — absent is not null — and the `td-*` tags repeat the content's `op`,
 * `listId` and `itemId` so the relay can gate without parsing.
 *
 * No imports: this module is loaded by the conformance binder under plain
 * `node --test`. The kind integer is pinned against `KIND_PROJECT_TODO_OP`
 * in `todoOp.test.mjs`.
 */
import { rankError } from "./fractionalRank.ts";

/** Kind 44248. Pinned against `@/shared/constants/kinds` in tests. */
export const PROJECT_TODO_OP_KIND = 44248;
export const PROJECT_TODO_SCHEMA = "buzz-project-todo/v1";
export const PROJECT_TODO_TAG_VERSION = "td1-1";
export const MAX_PROJECT_TODO_CONTENT_BYTES = 4 * 1024;
export const MAX_PROJECT_TODO_TEXT_BYTES = 1024;

export type TodoVisibility = "project" | "personal";

export function isTodoVisibility(value: unknown): value is TodoVisibility {
  return value === "project" || value === "personal";
}

export type TodoOpKind =
  | "list.create"
  | "list.title"
  | "list.archived"
  | "list.pinned"
  | "item.add"
  | "item.text"
  | "item.done"
  | "item.assignee"
  | "item.due"
  | "item.rank"
  | "item.remove";

/**
 * One op's content, without the visibility every op also carries as its
 * `td-vis` tag (see {@link TodoOp}). `list.create` repeats the visibility in
 * content because it is the op that fixes it.
 */
export type TodoOpContent =
  | {
      op: "list.create";
      listId: string;
      title: string;
      visibility: TodoVisibility;
    }
  | { op: "list.title"; listId: string; title: string }
  | { op: "list.archived"; listId: string; archived: boolean }
  | { op: "list.pinned"; listId: string; pinned: boolean }
  | {
      op: "item.add";
      listId: string;
      itemId: string;
      text: string;
      rank: string;
    }
  | { op: "item.text"; listId: string; itemId: string; text: string }
  | { op: "item.done"; listId: string; itemId: string; done: boolean }
  | {
      op: "item.assignee";
      listId: string;
      itemId: string;
      assignee: string | null;
    }
  | { op: "item.due"; listId: string; itemId: string; due: string | null }
  | { op: "item.rank"; listId: string; itemId: string; rank: string }
  | { op: "item.remove"; listId: string; itemId: string };

/** An op with the list's visibility, which rides every event as `td-vis`. */
export type TodoOp = TodoOpContent & { visibility: TodoVisibility };

const CONTENT_KEYS: Record<TodoOpKind, readonly string[]> = {
  "list.create": ["schema", "op", "listId", "title", "visibility"],
  "list.title": ["schema", "op", "listId", "title"],
  "list.archived": ["schema", "op", "listId", "archived"],
  "list.pinned": ["schema", "op", "listId", "pinned"],
  "item.add": ["schema", "op", "listId", "itemId", "text", "rank"],
  "item.text": ["schema", "op", "listId", "itemId", "text"],
  "item.done": ["schema", "op", "listId", "itemId", "done"],
  "item.assignee": ["schema", "op", "listId", "itemId", "assignee"],
  "item.due": ["schema", "op", "listId", "itemId", "due"],
  "item.rank": ["schema", "op", "listId", "itemId", "rank"],
  "item.remove": ["schema", "op", "listId", "itemId"],
};

export const TODO_OP_KINDS = Object.keys(CONTENT_KEYS) as TodoOpKind[];

/** `true` for a 32-character lowercase hex id. */
export function isTodoId(value: unknown): value is string {
  return typeof value === "string" && /^[0-9a-f]{32}$/.test(value);
}

function isLowerHex64(value: string): boolean {
  return /^[0-9a-f]{64}$/.test(value);
}

function utf8Length(value: string): number {
  return new TextEncoder().encode(value).length;
}

/** Why `value` is not a valid title/text, or `null`. */
export function todoTextError(field: string, value: string): string | null {
  if (value.trim().length === 0) return `todo ${field} must not be blank`;
  if (utf8Length(value) > MAX_PROJECT_TODO_TEXT_BYTES) {
    return `todo ${field} exceeds ${MAX_PROJECT_TODO_TEXT_BYTES} bytes`;
  }
  // biome-ignore lint/suspicious/noControlCharactersInRegex: the rule is about control characters
  if (/[\u0000-\u0008\u000B\u000C\u000E-\u001F\u007F-\u009F]/.test(value)) {
    return `todo ${field} must not contain control characters`;
  }
  return null;
}

/** Why `value` is not a `YYYY-MM-DD` calendar date in 1970–9999, or `null`. */
export function dueDateError(value: string): string | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value);
  if (!match) return "todo due date must be YYYY-MM-DD";
  const year = Number(match[1]);
  const month = Number(match[2]);
  const day = Number(match[3]);
  if (year < 1970 || year > 9999) return "todo due year must be 1970–9999";
  if (month < 1 || month > 12) return "todo due month must be 01–12";
  const leap = (year % 4 === 0 && year % 100 !== 0) || year % 400 === 0;
  const daysInMonth =
    [31, leap ? 29 : 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31][month - 1] ??
    0;
  if (day < 1 || day > daysInMonth) {
    return "todo due day is not a calendar day of that month";
  }
  return null;
}

/** The item an op names, or `null` for a list op. */
export function todoOpItemId(op: TodoOpContent): string | null {
  return "itemId" in op ? op.itemId : null;
}

/**
 * Strictly decode op content for a list of `visibility` (the event's
 * `td-vis` tag, already read by the caller). Returns the op or a reason; the
 * caller decides whether a reason is an `ignored` count (fold) or an error
 * (compose). A `list.create` whose content disagrees with the tag is an
 * error: the tag is what the relay gates on.
 */
export function decodeTodoOp(
  content: string,
  visibility: TodoVisibility,
): TodoOp | { error: string } {
  if (utf8Length(content) > MAX_PROJECT_TODO_CONTENT_BYTES) {
    return {
      error: `todo op content exceeds ${MAX_PROJECT_TODO_CONTENT_BYTES} bytes`,
    };
  }
  let value: unknown;
  try {
    value = JSON.parse(content);
  } catch {
    return { error: "malformed todo op payload" };
  }
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    return { error: "todo op payload must be an object" };
  }
  const object = value as Record<string, unknown>;
  if (object.schema !== PROJECT_TODO_SCHEMA) {
    return {
      error: `todo op schema must be ${JSON.stringify(PROJECT_TODO_SCHEMA)}`,
    };
  }
  const op = object.op;
  if (typeof op !== "string" || !(op in CONTENT_KEYS)) {
    return { error: `unknown project todo op ${JSON.stringify(op)}` };
  }
  const kind = op as TodoOpKind;
  const expected = CONTENT_KEYS[kind];
  for (const key of Object.keys(object)) {
    if (!expected.includes(key)) {
      return {
        error: `todo op ${kind} has unsupported field ${JSON.stringify(key)}`,
      };
    }
  }
  for (const key of expected) {
    if (!(key in object)) {
      return {
        error: `todo op ${kind} is missing field ${JSON.stringify(key)}`,
      };
    }
  }
  const listId = object.listId;
  if (!isTodoId(listId))
    return { error: "todo listId must be 32 lowercase hex characters" };
  const isItemOp = kind.startsWith("item.");
  const itemId = isItemOp ? object.itemId : undefined;
  if (isItemOp && !isTodoId(itemId)) {
    return { error: "todo itemId must be 32 lowercase hex characters" };
  }
  const str = (key: string): string | { error: string } =>
    typeof object[key] === "string"
      ? (object[key] as string)
      : { error: `todo op ${key} must be a string` };
  const bool = (key: string): boolean | { error: string } =>
    typeof object[key] === "boolean"
      ? (object[key] as boolean)
      : { error: `todo op ${key} must be a boolean` };
  const nullableStr = (key: string): string | null | { error: string } =>
    object[key] === null
      ? null
      : typeof object[key] === "string"
        ? (object[key] as string)
        : { error: `todo op ${key} must be a string or null` };
  const bad = (v: unknown): v is { error: string } =>
    typeof v === "object" && v !== null && "error" in v;

  switch (kind) {
    case "list.create": {
      const title = str("title");
      if (bad(title)) return title;
      const err = todoTextError("title", title);
      if (err) return { error: err };
      const declared = object.visibility;
      if (!isTodoVisibility(declared)) {
        return { error: "todo visibility must be project or personal" };
      }
      if (declared !== visibility) {
        return {
          error: `todo list.create content visibility ${JSON.stringify(declared)} does not match its td-vis tag ${JSON.stringify(visibility)}`,
        };
      }
      return { op: kind, listId, title, visibility };
    }
    case "list.title": {
      const title = str("title");
      if (bad(title)) return title;
      const err = todoTextError("title", title);
      if (err) return { error: err };
      return { op: kind, listId, title, visibility };
    }
    case "list.archived": {
      const archived = bool("archived");
      if (bad(archived)) return archived;
      return { op: kind, listId, archived, visibility };
    }
    case "list.pinned": {
      const pinned = bool("pinned");
      if (bad(pinned)) return pinned;
      return { op: kind, listId, pinned, visibility };
    }
    case "item.add": {
      const text = str("text");
      if (bad(text)) return text;
      const textErr = todoTextError("text", text);
      if (textErr) return { error: textErr };
      const rank = str("rank");
      if (bad(rank)) return rank;
      const rankErr = rankError(rank);
      if (rankErr) return { error: rankErr };
      return {
        op: kind,
        listId,
        itemId: itemId as string,
        text,
        rank,
        visibility,
      };
    }
    case "item.text": {
      const text = str("text");
      if (bad(text)) return text;
      const err = todoTextError("text", text);
      if (err) return { error: err };
      return { op: kind, listId, itemId: itemId as string, text, visibility };
    }
    case "item.done": {
      const done = bool("done");
      if (bad(done)) return done;
      return { op: kind, listId, itemId: itemId as string, done, visibility };
    }
    case "item.assignee": {
      const assignee = nullableStr("assignee");
      if (bad(assignee)) return assignee;
      if (assignee !== null && !isLowerHex64(assignee)) {
        return {
          error: "todo assignee must be a 64-character lowercase hex pubkey",
        };
      }
      return {
        op: kind,
        listId,
        itemId: itemId as string,
        assignee,
        visibility,
      };
    }
    case "item.due": {
      const due = nullableStr("due");
      if (bad(due)) return due;
      if (due !== null) {
        const err = dueDateError(due);
        if (err) return { error: err };
      }
      return { op: kind, listId, itemId: itemId as string, due, visibility };
    }
    case "item.rank": {
      const rank = str("rank");
      if (bad(rank)) return rank;
      const err = rankError(rank);
      if (err) return { error: err };
      return { op: kind, listId, itemId: itemId as string, rank, visibility };
    }
    case "item.remove":
      return { op: kind, listId, itemId: itemId as string, visibility };
  }
}

/** Canonical content JSON for `op` (keys in contract order). */
export function encodeTodoOpContent(op: TodoOpContent): string {
  const ordered: Record<string, unknown> = { schema: PROJECT_TODO_SCHEMA };
  for (const key of CONTENT_KEYS[op.op]) {
    if (key === "schema") continue;
    ordered[key] = (op as unknown as Record<string, unknown>)[key];
  }
  return JSON.stringify(ordered);
}

/** The tags an op carries: `a`, `td-v`, `td-op`, `td-list`, `td-vis`, `[td-item]`. */
export function todoOpTags(coordinate: string, op: TodoOp): string[][] {
  const tags = [
    ["a", coordinate],
    ["td-v", PROJECT_TODO_TAG_VERSION],
    ["td-op", op.op],
    ["td-list", op.listId],
    ["td-vis", op.visibility],
  ];
  const itemId = todoOpItemId(op);
  if (itemId !== null) tags.push(["td-item", itemId]);
  return tags;
}

/** Mint a fresh 32-hex list or item id. */
export function newTodoId(): string {
  const bytes = new Uint8Array(16);
  crypto.getRandomValues(bytes);
  return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
}
