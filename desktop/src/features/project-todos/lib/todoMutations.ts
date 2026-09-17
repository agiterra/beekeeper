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
  encodeTodoOpContent,
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
  createList: (title: string) => Promise<string>;
  renameList: (listId: string, title: string) => Promise<void>;
  setListArchived: (listId: string, archived: boolean) => Promise<void>;
  addItem: (
    listId: string,
    text: string,
    extra?: {
      assignee?: string | null;
      due?: string | null;
      index?: number | null;
    },
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
};

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
    const openItems = (listId: string): readonly TodoItem[] => {
      const read = queryClient.getQueryData<ProjectTodosRead>(
        projectTodosQueryKey(require()),
      );
      return read?.digest.lists.find((list) => list.id === listId)?.open ?? [];
    };
    const publish = (op: TodoOp) => publishTodoOp(queryClient, require(), op);
    return {
      createList: async (title) => {
        const listId = newTodoId();
        await publish({ op: "list.create", listId, title });
        return listId;
      },
      renameList: async (listId, title) => {
        await publish({ op: "list.title", listId, title });
      },
      setListArchived: async (listId, archived) => {
        await publish({ op: "list.archived", listId, archived });
      },
      addItem: async (listId, text, extra = {}) => {
        const itemId = newTodoId();
        const rank = rankForIndex(openItems(listId), extra.index ?? null, null);
        await publish({ op: "item.add", listId, itemId, text, rank });
        if (extra.assignee) {
          await publish({
            op: "item.assignee",
            listId,
            itemId,
            assignee: extra.assignee,
          });
        }
        if (extra.due) {
          await publish({ op: "item.due", listId, itemId, due: extra.due });
        }
        return itemId;
      },
      setText: async (listId, itemId, text) => {
        await publish({ op: "item.text", listId, itemId, text });
      },
      setDone: async (listId, itemId, done) => {
        await publish({ op: "item.done", listId, itemId, done });
      },
      setAssignee: async (listId, itemId, assignee) => {
        await publish({ op: "item.assignee", listId, itemId, assignee });
      },
      setDue: async (listId, itemId, due) => {
        await publish({ op: "item.due", listId, itemId, due });
      },
      moveItem: async (listId, itemId, index) => {
        const rank = rankForIndex(openItems(listId), index, itemId);
        await publish({ op: "item.rank", listId, itemId, rank });
      },
      removeItem: async (listId, itemId) => {
        await publish({ op: "item.remove", listId, itemId });
      },
    };
  }, [coordinate, queryClient]);
}
