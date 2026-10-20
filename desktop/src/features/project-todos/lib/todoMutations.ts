/**
 * Writing to-do ops: one signed kind 44248 event per field edit, stamped past
 * the latest op on the same target, appended to the cached read at once (so
 * the row moves before the relay echoes it) and reconciled by the
 * invalidation that follows.
 *
 * Every mutation computes ranks and timestamps from the read in the query
 * cache *at call time*, so the callbacks are reference-stable and a drag
 * handler holding one never acts on a stale closure.
 */
import * as React from "react";
import { type QueryClient, useQueryClient } from "@tanstack/react-query";

import { relayClient } from "@/shared/api/relayClient";
import { signRelayEvent } from "@/shared/api/tauri";
import type { RelayEvent } from "@/shared/api/types";
import { KIND_PROJECT_TODO_OP } from "@/shared/constants/kinds";

import { rankBetween } from "./fractionalRank";
import type { TodoItem } from "./todoFold";
import {
  type TodoOp,
  type TodoOpContent,
  type TodoOpKind,
  type TodoPriority,
  type TodoStatus,
  type TodoVisibility,
  encodeTodoOpContent,
  isClosedTodoStatus,
  newTodoId,
  todoOpItemId,
  todoOpTags,
} from "./todoOp";
import {
  type ProjectTodosRead,
  RELAY_TIMESTAMP_DRIFT_SECS,
  projectTodosQueryKey,
  readFromEvents,
  todoTargetKey,
} from "./todoQueries";

/** A bump that would land past this is refused here, not by the relay. */
const MAX_FUTURE_SKEW_SECS = RELAY_TIMESTAMP_DRIFT_SECS - 10;

/**
 * The `created_at` for a write against a target whose latest op was stamped
 * `headSecs`: never below `headSecs + 1`, never below now. The CLI's
 * `next_replaceable_created_at` rule.
 */
export function nextTodoCreatedAt(headSecs: number, nowSecs: number): number {
  return Math.max(headSecs + 1, nowSecs);
}

/** The rank that places an item at `index` among `open` (0 = first; `null` = last), excluding `movingId`. */
export function rankForIndex(
  open: readonly TodoItem[],
  index: number | null,
  movingId: string | null,
): string {
  const others = open.filter((item) => item.id !== movingId);
  const at =
    index === null
      ? others.length
      : Math.min(Math.max(index, 0), others.length);
  const after = at > 0 ? (others[at - 1]?.rank ?? null) : null;
  const before = others[at]?.rank ?? null;
  return rankBetween(after, before);
}

/** Sign and publish one op, then append it to the cached read. */
export async function publishTodoOp(
  queryClient: QueryClient,
  coordinate: string,
  op: TodoOp,
): Promise<RelayEvent> {
  const key = projectTodosQueryKey(coordinate);
  const read = queryClient.getQueryData<ProjectTodosRead>(key);
  const head =
    read?.latestByTarget[todoTargetKey(op.listId, todoOpItemId(op))] ?? 0;
  const now = Math.floor(Date.now() / 1_000);
  const createdAt = nextTodoCreatedAt(head, now);
  if (createdAt > now + MAX_FUTURE_SKEW_SECS) {
    throw new Error(
      `The latest change to this item is stamped ${head - now}s in the future; try again in a moment.`,
    );
  }
  const event = await signRelayEvent({
    kind: KIND_PROJECT_TODO_OP,
    content: encodeTodoOpContent(op),
    createdAt,
    tags: todoOpTags(coordinate, op),
  });
  const accepted = await relayClient.publishEvent(
    event,
    "Timed out saving the to-do change.",
    "The relay refused the to-do change.",
  );
  queryClient.setQueryData<ProjectTodosRead>(key, (previous) =>
    readFromEvents(
      coordinate,
      [...(previous?.events ?? []), accepted],
      previous?.truncated ?? false,
    ),
  );
  void queryClient.invalidateQueries({ queryKey: key });
  return accepted;
}

export type TodoMutations = {
  createList: (
    title: string,
    visibility: TodoVisibility,
    extra?: { pinned?: boolean },
  ) => Promise<string>;
  renameList: (listId: string, title: string) => Promise<void>;
  setListArchived: (listId: string, archived: boolean) => Promise<void>;
  setListPinned: (listId: string, pinned: boolean) => Promise<void>;
  /**
   * One `item.add`, so the item exists exactly when this resolves. Fields
   * beyond text and position are separate ops the caller sends after, and
   * reports one by one: folding them in here would turn "created, but the
   * assignee was refused" into a rejection a caller would retry, minting a
   * duplicate.
   */
  addItem: (
    listId: string,
    text: string,
    extra?: { index?: number | null },
  ) => Promise<string>;
  setText: (listId: string, itemId: string, text: string) => Promise<void>;
  setDone: (listId: string, itemId: string, done: boolean) => Promise<void>;
  setAssignee: (
    listId: string,
    itemId: string,
    assignee: string | null,
  ) => Promise<void>;
  setDue: (listId: string, itemId: string, due: string | null) => Promise<void>;
  moveItem: (listId: string, itemId: string, index: number) => Promise<void>;
  removeItem: (listId: string, itemId: string) => Promise<void>;
  /**
   * Set the workflow status: `item.done`, then `item.status` (see
   * {@link statusOps}). Two events, not one: when the second is refused the
   * promise rejects with a {@link TodoPartialWriteError} saying what the
   * item now reads.
   */
  setStatus: (
    listId: string,
    itemId: string,
    status: TodoStatus,
  ) => Promise<void>;
  setPriority: (
    listId: string,
    itemId: string,
    priority: TodoPriority,
  ) => Promise<void>;
  /** `null` clears the description. */
  setDescription: (
    listId: string,
    itemId: string,
    description: string | null,
  ) => Promise<void>;
};

/**
 * The ops `setStatus` publishes, in order: `item.done` carrying the new
 * status's closed-ness, then `item.status`. Always both, never only the
 * status when this client's read says the closed boundary is not crossed:
 * that read can be stale (another writer may have reopened the item), and
 * done-ness is decided by `item.done` alone, by every reader of any age
 * (NIP-TD, fold rule 6). A status that disagrees with a later `item.done`
 * is simply not applied, so the pair can never split old and new readers.
 */
export function statusOps(
  listId: string,
  itemId: string,
  status: TodoStatus,
): TodoOpContent[] {
  return [
    { op: "item.done", listId, itemId, done: isClosedTodoStatus(status) },
    { op: "item.status", listId, itemId, status },
  ];
}

/**
 * A write of several ops of which only the first `saved` were accepted.
 * Ops are separate signed events; there is no transaction, so this says
 * exactly which part landed and what the item reads now, instead of a
 * generic failure that hides a half-applied change.
 */
export class TodoPartialWriteError extends Error {
  readonly saved: readonly TodoOpKind[];
  readonly refused: TodoOpKind;
  readonly reason: string;

  constructor(
    summary: string,
    saved: readonly TodoOpKind[],
    refused: TodoOpKind,
    reason: string,
  ) {
    super(`${summary} (${reason})`);
    this.name = "TodoPartialWriteError";
    this.saved = saved;
    this.refused = refused;
    this.reason = reason;
  }
}

function reasonOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/**
 * Publish {@link statusOps} through `publish`. A refusal of the first op
 * saved nothing and is rethrown as is; a refusal of the second leaves the
 * item reading `done` or `todo` from the saved `item.done`, and says so.
 */
export async function publishStatusOps(
  publish: (op: TodoOpContent) => Promise<unknown>,
  listId: string,
  itemId: string,
  status: TodoStatus,
): Promise<void> {
  const [doneOp, statusOp] = statusOps(listId, itemId, status) as [
    TodoOpContent,
    TodoOpContent,
  ];
  await publish(doneOp);
  try {
    await publish(statusOp);
  } catch (error) {
    const reads = isClosedTodoStatus(status) ? "done" : "todo";
    throw new TodoPartialWriteError(
      `Only part of the change saved: the item now reads “${reads}” for everyone, but the relay refused the status “${status}”`,
      ["item.done"],
      "item.status",
      reasonOf(error),
    );
  }
}

/** Reference-stable mutation callbacks for one project coordinate. */
export function useTodoMutations(coordinate: string | null): TodoMutations {
  const queryClient = useQueryClient();
  return React.useMemo(() => {
    const require = (): string => {
      if (coordinate === null) {
        throw new Error("This project has no coordinate to write to yet.");
      }
      return coordinate;
    };
    const cachedList = (listId: string) =>
      queryClient
        .getQueryData<ProjectTodosRead>(projectTodosQueryKey(require()))
        ?.digest.lists.find((list) => list.id === listId);
    const openItems = (listId: string): readonly TodoItem[] =>
      cachedList(listId)?.open ?? [];
    // Every op repeats its list's visibility as the `td-vis` tag; a list the
    // cache does not know cannot be written to honestly, so say so.
    const visibilityOf = (listId: string): TodoVisibility => {
      const list = cachedList(listId);
      if (!list) {
        throw new Error(
          "This list is not in the current read; refresh and try again.",
        );
      }
      return list.visibility;
    };
    const publish = (op: TodoOp) => publishTodoOp(queryClient, require(), op);
    const publishOn = (content: TodoOpContent) =>
      publish({ ...content, visibility: visibilityOf(content.listId) });
    return {
      createList: async (title, visibility, extra = {}) => {
        const listId = newTodoId();
        await publish({ op: "list.create", listId, title, visibility });
        if (extra.pinned) {
          await publish({
            op: "list.pinned",
            listId,
            pinned: true,
            visibility,
          });
        }
        return listId;
      },
      renameList: async (listId, title) => {
        await publishOn({ op: "list.title", listId, title });
      },
      setListArchived: async (listId, archived) => {
        await publishOn({ op: "list.archived", listId, archived });
      },
      setListPinned: async (listId, pinned) => {
        await publishOn({ op: "list.pinned", listId, pinned });
      },
      addItem: async (listId, text, extra = {}) => {
        const itemId = newTodoId();
        const rank = rankForIndex(openItems(listId), extra.index ?? null, null);
        await publishOn({ op: "item.add", listId, itemId, text, rank });
        return itemId;
      },
      setText: async (listId, itemId, text) => {
        await publishOn({ op: "item.text", listId, itemId, text });
      },
      setDone: async (listId, itemId, done) => {
        await publishOn({ op: "item.done", listId, itemId, done });
      },
      setAssignee: async (listId, itemId, assignee) => {
        await publishOn({ op: "item.assignee", listId, itemId, assignee });
      },
      setDue: async (listId, itemId, due) => {
        await publishOn({ op: "item.due", listId, itemId, due });
      },
      moveItem: async (listId, itemId, index) => {
        const rank = rankForIndex(openItems(listId), index, itemId);
        await publishOn({ op: "item.rank", listId, itemId, rank });
      },
      removeItem: async (listId, itemId) => {
        await publishOn({ op: "item.remove", listId, itemId });
      },
      setStatus: async (listId, itemId, status) => {
        await publishStatusOps(publishOn, listId, itemId, status);
      },
      setPriority: async (listId, itemId, priority) => {
        await publishOn({ op: "item.priority", listId, itemId, priority });
      },
      setDescription: async (listId, itemId, description) => {
        await publishOn({
          op: "item.description",
          listId,
          itemId,
          description,
        });
      },
    };
  }, [coordinate, queryClient]);
}
