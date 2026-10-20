/**
 * The native provider's binding: the project's to-do read and writes,
 * presented through the Planning contract. One read, one live
 * subscription — the same ones the To-Do tab mounts, sharing its React
 * Query cache, so the two tabs never disagree.
 */
import * as React from "react";

import type { ProjectTodosRead } from "@/features/project-todos/lib/todoQueries";
import { useProjectTodos } from "@/features/project-todos/lib/todoQueries";
import {
  TodoPartialWriteError,
  useTodoMutations,
} from "@/features/project-todos/lib/todoMutations";

import { type ActivityEntry, nativeIssueActivity } from "./nativeActivity";
import { createNativeIssue } from "./nativeCreate";
import { PlanningPartialWriteError } from "./planningErrors";
import {
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
        const { listId, itemId, failed } = await createNativeIssue(
          todoMutations,
          { ...input, statusId: input.statusId, priorityId: input.priorityId },
        );
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
        try {
          await todoMutations.setStatus(
            issue.containerId,
            issue.issueId,
            statusId,
          );
        } catch (error) {
          if (error instanceof TodoPartialWriteError) {
            throw new PlanningPartialWriteError(error.message);
          }
          throw error;
        }
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
