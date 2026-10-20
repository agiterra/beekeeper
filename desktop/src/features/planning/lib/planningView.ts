/**
 * Display options and the pure function that applies them: the list and the
 * board are two renderings of one filtered, grouped, ordered set of issues,
 * so switching between them never changes which records are shown.
 *
 * Display options are a person's own (stored on this computer, see
 * `planningDisplayStorage.ts`); they never change the issues themselves.
 */
import type {
  PlanningCapabilities,
  PlanningContainer,
  PlanningIssue,
  PlanningStatusCategory,
} from "./planningTypes.ts";

export type PlanningLayout = "list" | "board";
export type PlanningGroupBy =
  | "status"
  | "assignee"
  | "priority"
  | "container"
  | "none";
export type PlanningOrderBy = "manual" | "priority" | "updated" | "created";

/** The quick views across the top; each is a preset over the filters. */
export type PlanningScope = "all" | "mine" | "triage" | "active" | "backlog";

export type PlanningAssigneeFilter =
  | { kind: "any" }
  | { kind: "me" }
  | { kind: "unassigned" }
  | { kind: "person"; pubkey: string };

export type PlanningDisplay = {
  layout: PlanningLayout;
  groupBy: PlanningGroupBy;
  orderBy: PlanningOrderBy;
  /** Show issues in a closed status (completed, canceled) in the list. */
  showClosed: boolean;
};

export type PlanningFilters = {
  scope: PlanningScope;
  /** Empty = any. */
  statusIds: string[];
  /** Empty = any. */
  priorityIds: string[];
  assignee: PlanningAssigneeFilter;
  /** `null` = every container. */
  containerId: string | null;
  query: string;
};

export const DEFAULT_PLANNING_DISPLAY: PlanningDisplay = {
  layout: "list",
  groupBy: "status",
  orderBy: "manual",
  showClosed: true,
};

export const DEFAULT_PLANNING_FILTERS: PlanningFilters = {
  scope: "all",
  statusIds: [],
  priorityIds: [],
  assignee: { kind: "any" },
  containerId: null,
  query: "",
};

const CLOSED: ReadonlySet<PlanningStatusCategory> = new Set([
  "completed",
  "canceled",
]);

export function isClosedIssue(issue: PlanningIssue): boolean {
  return CLOSED.has(issue.status.category);
}

function scopeAdmits(
  scope: PlanningScope,
  issue: PlanningIssue,
  self: string | null,
): boolean {
  switch (scope) {
    case "all":
      return true;
    case "mine":
      return self !== null && issue.assignee === self;
    case "triage":
      return issue.status.category === "triage";
    case "active":
      return (
        issue.status.category === "unstarted" ||
        issue.status.category === "started"
      );
    case "backlog":
      return issue.status.category === "backlog";
  }
}

/** `true` when `issue` passes every filter. */
export function issueMatches(
  issue: PlanningIssue,
  filters: PlanningFilters,
  self: string | null,
): boolean {
  if (!scopeAdmits(filters.scope, issue, self)) return false;
  if (
    filters.statusIds.length > 0 &&
    !filters.statusIds.includes(issue.status.id)
  ) {
    return false;
  }
  if (
    filters.priorityIds.length > 0 &&
    !filters.priorityIds.includes(issue.priority.id)
  ) {
    return false;
  }
  switch (filters.assignee.kind) {
    case "me":
      if (self === null || issue.assignee !== self) return false;
      break;
    case "unassigned":
      if (issue.assignee !== null) return false;
      break;
    case "person":
      if (issue.assignee !== filters.assignee.pubkey) return false;
      break;
    case "any":
      break;
  }
  if (filters.containerId !== null && issue.containerId !== filters.containerId)
    return false;
  const query = filters.query.trim().toLowerCase();
  if (query.length > 0) {
    const haystack = `${issue.title}\n${issue.description ?? ""}`.toLowerCase();
    if (!haystack.includes(query)) return false;
  }
  return true;
}

/** `true` when any filter beyond the scope narrows the set. */
export function hasActiveFilters(filters: PlanningFilters): boolean {
  return (
    filters.statusIds.length > 0 ||
    filters.priorityIds.length > 0 ||
    filters.assignee.kind !== "any" ||
    filters.containerId !== null ||
    filters.query.trim().length > 0
  );
}

function compare(a: string | number, b: string | number): number {
  return a < b ? -1 : a > b ? 1 : 0;
}

export function compareIssues(
  orderBy: PlanningOrderBy,
): (a: PlanningIssue, b: PlanningIssue) => number {
  const tie = (a: PlanningIssue, b: PlanningIssue) => compare(a.key, b.key);
  switch (orderBy) {
    case "manual":
      return (a, b) =>
        compare(a.containerId, b.containerId) ||
        compare(a.rank, b.rank) ||
        tie(a, b);
    case "priority":
      return (a, b) =>
        a.priority.order - b.priority.order ||
        b.updatedAt - a.updatedAt ||
        tie(a, b);
    case "updated":
      return (a, b) => b.updatedAt - a.updatedAt || tie(a, b);
    case "created":
      return (a, b) => b.createdAt - a.createdAt || tie(a, b);
  }
}

export type PlanningGroup = {
  /** Stable id: a status/priority id, a pubkey, a container id, or `unassigned` / `all`. */
  id: string;
  label: string;
  /** For a status group, its category (the icon and colour). */
  category?: PlanningStatusCategory;
  /** For an assignee group, the pubkey (`null` = unassigned). */
  assignee?: string | null;
  issues: PlanningIssue[];
};

/**
 * Group `issues` (already filtered) for display. Status and priority groups
 * come in the provider's own order and include empty groups, so a board has
 * a column to drop into; assignee and container groups list only those with
 * issues, apart from the containers the provider already has.
 */
export function groupIssues(
  issues: readonly PlanningIssue[],
  display: Pick<PlanningDisplay, "groupBy" | "orderBy">,
  capabilities: Pick<PlanningCapabilities, "statuses" | "priorities">,
  containers: readonly PlanningContainer[],
  nameOf: (pubkey: string) => string,
): PlanningGroup[] {
  const sorted = [...issues].sort(compareIssues(display.orderBy));
  switch (display.groupBy) {
    case "none":
      return [{ id: "all", label: "All issues", issues: sorted }];
    case "status":
      return capabilities.statuses.map((status) => ({
        id: status.id,
        label: status.label,
        category: status.category,
        issues: sorted.filter((issue) => issue.status.id === status.id),
      }));
    case "priority":
      return capabilities.priorities.map((priority) => ({
        id: priority.id,
        label: priority.label,
        issues: sorted.filter((issue) => issue.priority.id === priority.id),
      }));
    case "container":
      return containers.map((container) => ({
        id: container.id,
        label: container.title,
        issues: sorted.filter((issue) => issue.containerId === container.id),
      }));
    case "assignee": {
      const byAssignee = new Map<string | null, PlanningIssue[]>();
      for (const issue of sorted) {
        const bucket = byAssignee.get(issue.assignee) ?? [];
        bucket.push(issue);
        byAssignee.set(issue.assignee, bucket);
      }
      const people = [...byAssignee.keys()]
        .filter((pubkey): pubkey is string => pubkey !== null)
        .map((pubkey) => ({ pubkey, name: nameOf(pubkey) }))
        .sort((a, b) => compare(a.name.toLowerCase(), b.name.toLowerCase()));
      const groups: PlanningGroup[] = people.map(({ pubkey, name }) => ({
        id: pubkey,
        label: name,
        assignee: pubkey,
        issues: byAssignee.get(pubkey) ?? [],
      }));
      groups.push({
        id: "unassigned",
        label: "No assignee",
        assignee: null,
        issues: byAssignee.get(null) ?? [],
      });
      return groups;
    }
  }
}

/**
 * The issues a view shows, in display order: filtered, grouped, and with
 * closed issues dropped from the list when the viewer hid them. The board
 * always shows every status column, closed ones included.
 */
export function planningView(
  issues: readonly PlanningIssue[],
  filters: PlanningFilters,
  display: PlanningDisplay,
  capabilities: Pick<PlanningCapabilities, "statuses" | "priorities">,
  containers: readonly PlanningContainer[],
  self: string | null,
  nameOf: (pubkey: string) => string,
): { groups: PlanningGroup[]; matched: number; total: number } {
  const matched = issues.filter((issue) => {
    if (!issueMatches(issue, filters, self)) return false;
    if (display.layout === "list" && !display.showClosed) {
      return !isClosedIssue(issue);
    }
    return true;
  });
  return {
    groups: groupIssues(matched, display, capabilities, containers, nameOf),
    matched: matched.length,
    total: issues.length,
  };
}

/** The flat order a keyboard walks: groups top to bottom, issues within. */
export function flattenGroups(groups: readonly PlanningGroup[]): string[] {
  return groups.flatMap((group) => group.issues.map((issue) => issue.key));
}

/** The next selection after a j/k (or arrow) step from `current`. */
export function stepSelection(
  order: readonly string[],
  current: string | null,
  step: 1 | -1,
): string | null {
  if (order.length === 0) return null;
  const at = current === null ? -1 : order.indexOf(current);
  if (at === -1)
    return step === 1 ? (order[0] ?? null) : (order.at(-1) ?? null);
  const next = Math.min(Math.max(at + step, 0), order.length - 1);
  return order[next] ?? null;
}
