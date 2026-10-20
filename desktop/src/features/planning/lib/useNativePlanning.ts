/**
 * The native provider's binding: the project's to-do read and writes,
 * presented through the Planning contract. One read, one live
 * subscription — the same ones the To-Do tab mounts, sharing its React
 * Query cache, so the two tabs never disagree.
 */
import * as React from "react";

import type { ProjectTodosRead } from "@/features/project-todos/lib/todoQueries";
import { useProjectTodos } from "@/features/project-todos/lib/todoQueries";
import { useTodoMutations } from "@/features/project-todos/lib/todoMutations";

import { type ActivityEntry, nativeIssueActivity } from "./nativeActivity";
import {
  DEFAULT_NATIVE_CONTAINER_TITLE,
  isNativePriorityId,
  isNativeStatusId,
  nativeIssueKey,
  nativePlanningRead,
} from "./nativePlanning";
import type {
  PlanningIssue,
  PlanningMutations,
  PlanningRead,
  PlanningState,
} from "./planningTypes";

function readOf(read: ProjectTodosRead): PlanningRead {
  return nativePlanningRead(read.digest.lists, {
    truncated: read.truncated,
    // Ops older than `td-vis` are counted apart by the read; no deployed
    // relay ever held one, so they are not reported as unreadable.
    ignored: read.digest.ignored - read.legacy,
  });
}

function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export type NativePlanning = {
  state: PlanningState;
  mutations: PlanningMutations;
  activityFor: (issue: PlanningIssue) => ActivityEntry[];
};

/** Native planning for a project coordinate (`null`: nothing to read). */
export function useNativePlanning(coordinate: string | null): NativePlanning {
  const todos = useProjectTodos(coordinate);
  const todoMutations = useTodoMutations(coordinate);

  const read = todos.read;
  const planningRead = React.useMemo(
    () => (read ? readOf(read) : null),
    [read],
  );

  const state: PlanningState = React.useMemo(() => {
    if (todos.kind === "error") {
      return { kind: "error", read: planningRead, message: todos.message };
    }
    if (todos.kind === "loading" || !planningRead) return { kind: "loading" };
    return { kind: "ready", read: planningRead, refreshing: todos.refreshing };
  }, [planningRead, todos]);

  const mutations = React.useMemo<PlanningMutations>(
    () => ({
      createIssue: async (input) => {
        if (!isNativeStatusId(input.statusId)) {
          throw new Error(`Unknown status ${input.statusId}.`);
        }
        if (!isNativePriorityId(input.priorityId)) {
          throw new Error(`Unknown priority ${input.priorityId}.`);
        }
        const listId =
          input.containerId ??
          (await todoMutations.createList(
            DEFAULT_NATIVE_CONTAINER_TITLE,
            "project",
          ));
        const itemId = await todoMutations.addItem(listId, input.title, {
          assignee: input.assignee,
        });
        // The item exists from here on. Each further field is its own op;
        // one that fails is reported, never rolled back into a pretend
        // "nothing happened".
        const failed: { field: string; message: string }[] = [];
        const attempt = async (field: string, work: () => Promise<void>) => {
          try {
            await work();
          } catch (error) {
            failed.push({ field, message: message(error) });
          }
        };
        if (input.statusId !== "todo") {
          const statusId = input.statusId;
          await attempt("status", () =>
            todoMutations.setStatus(listId, itemId, statusId),
          );
        }
        if (input.priorityId !== "none") {
          const priorityId = input.priorityId;
          await attempt("priority", () =>
            todoMutations.setPriority(listId, itemId, priorityId),
          );
        }
        if (input.description !== null) {
          const description = input.description;
          await attempt("description", () =>
            todoMutations.setDescription(listId, itemId, description),
          );
        }
        return { key: nativeIssueKey(listId, itemId), failed };
      },
      setTitle: (issue, title) =>
        todoMutations.setText(issue.containerId, issue.issueId, title),
      setDescription: (issue, description) =>
        todoMutations.setDescription(
          issue.containerId,
          issue.issueId,
          description,
        ),
      setStatus: async (issue, statusId) => {
        if (!isNativeStatusId(statusId)) {
          throw new Error(`Unknown status ${statusId}.`);
        }
        await todoMutations.setStatus(
          issue.containerId,
          issue.issueId,
          statusId,
        );
      },
      setPriority: async (issue, priorityId) => {
        if (!isNativePriorityId(priorityId)) {
          throw new Error(`Unknown priority ${priorityId}.`);
        }
        await todoMutations.setPriority(
          issue.containerId,
          issue.issueId,
          priorityId,
        );
      },
      setAssignee: (issue, assignee) =>
        todoMutations.setAssignee(issue.containerId, issue.issueId, assignee),
    }),
    [todoMutations],
  );

  const events = read?.events;
  const activityFor = React.useCallback(
    (issue: PlanningIssue) =>
      nativeIssueActivity(events ?? [], issue.containerId, issue.issueId),
    [events],
  );

  return { state, mutations, activityFor };
}
