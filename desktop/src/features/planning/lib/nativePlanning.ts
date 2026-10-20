/**
 * Native planning: a project's issues are the items of its shared NIP-TD
 * lists (kind 44248). Nothing here stores anything; it reads the same fold
 * the To-Do tab reads and maps it onto the Planning contract.
 *
 * What is and is not an issue:
 * - every item of a **project**-visibility list that is not archived;
 * - not a **personal** list's items: those are one person's to-dos, which
 *   the relay withholds from everyone else, and a shared board must not
 *   show something only its viewer can see as if it were shared;
 * - not an archived list's items, the way an archived Linear team drops out.
 *
 * Not NIP-34 repository issues (kind 1621). Those belong to a repository and
 * its surfaces; Planning neither reads nor converts them, and whether a
 * project should have one issue record instead of two is an open product
 * decision (NIP-TD § Limitations), not something this module settles.
 */
import type { TodoItem, TodoList } from "../../project-todos/lib/todoFold.ts";
import {
  MAX_PROJECT_TODO_DESCRIPTION_BYTES,
  MAX_PROJECT_TODO_TEXT_BYTES,
  type TodoPriority,
  type TodoStatus,
} from "../../project-todos/lib/todoOp.ts";
import type {
  PlanningCapabilities,
  PlanningContainer,
  PlanningIssue,
  PlanningPriority,
  PlanningRead,
  PlanningStatus,
} from "./planningTypes.ts";

/** The title of the shared list a first issue creates when none exists. */
export const DEFAULT_NATIVE_CONTAINER_TITLE = "Issues";

export const NATIVE_STATUSES: readonly PlanningStatus[] = [
  { id: "triage", label: "Triage", category: "triage" },
  { id: "backlog", label: "Backlog", category: "backlog" },
  { id: "todo", label: "Todo", category: "unstarted" },
  { id: "in_progress", label: "In Progress", category: "started" },
  { id: "done", label: "Done", category: "completed" },
  { id: "canceled", label: "Canceled", category: "canceled" },
];

export const NATIVE_PRIORITIES: readonly PlanningPriority[] = [
  { id: "urgent", label: "Urgent", level: "urgent", order: 0 },
  { id: "high", label: "High", level: "high", order: 1 },
  { id: "medium", label: "Medium", level: "medium", order: 2 },
  { id: "low", label: "Low", level: "low", order: 3 },
  { id: "none", label: "No priority", level: "none", order: 4 },
];

export const NATIVE_CAPABILITIES: PlanningCapabilities = {
  statuses: NATIVE_STATUSES,
  priorities: NATIVE_PRIORITIES,
  // NIP-TD has no comment op and Beekeeper has no generic comment kind for a
  // project item; the detail says so instead of drawing a box that cannot save.
  comments: false,
  createsDefaultContainer: true,
  personIds: "beekeeper-pubkey",
  limits: {
    titleBytes: MAX_PROJECT_TODO_TEXT_BYTES,
    descriptionBytes: MAX_PROJECT_TODO_DESCRIPTION_BYTES,
  },
};

const STATUS_BY_ID = new Map(NATIVE_STATUSES.map((s) => [s.id, s]));
const PRIORITY_BY_ID = new Map(NATIVE_PRIORITIES.map((p) => [p.id, p]));

export function nativeStatus(id: TodoStatus): PlanningStatus {
  return STATUS_BY_ID.get(id) ?? (NATIVE_STATUSES[2] as PlanningStatus);
}

export function nativePriority(id: TodoPriority): PlanningPriority {
  return PRIORITY_BY_ID.get(id) ?? (NATIVE_PRIORITIES[4] as PlanningPriority);
}

export function isNativeStatusId(id: string): id is TodoStatus {
  return STATUS_BY_ID.has(id);
}

export function isNativePriorityId(id: string): id is TodoPriority {
  return PRIORITY_BY_ID.has(id);
}

export function nativeIssueKey(listId: string, itemId: string): string {
  return `${listId}/${itemId}`;
}

/** `true` for a list whose items Planning shows. */
export function isPlanningList(list: TodoList): boolean {
  return list.visibility === "project" && !list.archived;
}

export function nativeIssue(item: TodoItem): PlanningIssue {
  return {
    provider: "native",
    key: nativeIssueKey(item.listId, item.id),
    containerId: item.listId,
    issueId: item.id,
    title: item.text,
    description: item.description,
    status: nativeStatus(item.status),
    priority: nativePriority(item.priority),
    assignee: item.assignee,
    due: item.due,
    rank: item.rank,
    createdAt: item.createdAt,
    createdBy: item.createdBy,
    updatedAt: item.updatedAt,
    completedAt: item.completedAt,
  };
}

/** Map a to-do digest onto the Planning read. */
export function nativePlanningRead(
  lists: readonly TodoList[],
  meta: { truncated: boolean; ignored: number },
): PlanningRead {
  const containers: PlanningContainer[] = [];
  const issues: PlanningIssue[] = [];
  for (const list of lists) {
    if (!isPlanningList(list)) continue;
    containers.push({ id: list.id, title: list.title });
    for (const item of [...list.open, ...list.completed]) {
      issues.push(nativeIssue(item));
    }
  }
  return {
    provider: "native",
    issues,
    containers,
    truncated: meta.truncated,
    unreadable: meta.ignored,
    people: [],
  };
}
