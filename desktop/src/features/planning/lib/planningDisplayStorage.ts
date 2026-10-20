/**
 * A person's Planning display options (list or board, grouping, order,
 * whether closed issues show), remembered on this computer per project.
 *
 * Personal by design: two members looking at one project can group it
 * differently without either changing what the other sees. There is no
 * shared default view yet; a project-wide default would be a shared record,
 * which this slice does not add. Filters are not remembered — a filter
 * quietly hiding issues on the next visit reads as missing data.
 */
import {
  DEFAULT_PLANNING_DISPLAY,
  type PlanningDisplay,
  type PlanningGroupBy,
  type PlanningLayout,
  type PlanningOrderBy,
} from "./planningView.ts";

const STORAGE_PREFIX = "beekeeper.planning.display.v1";

export function planningDisplayKey(
  pubkey: string | null,
  projectAddress: string,
): string {
  return `${STORAGE_PREFIX}:${pubkey ?? "anonymous"}:${projectAddress}`;
}

const LAYOUTS: readonly PlanningLayout[] = ["list", "board"];
const GROUPS: readonly PlanningGroupBy[] = [
  "status",
  "assignee",
  "priority",
  "container",
  "none",
];
const ORDERS: readonly PlanningOrderBy[] = [
  "manual",
  "priority",
  "updated",
  "created",
];

/** Parse a stored value; anything unrecognised falls back field by field. */
export function parsePlanningDisplay(raw: string | null): PlanningDisplay {
  if (!raw) return DEFAULT_PLANNING_DISPLAY;
  let value: unknown;
  try {
    value = JSON.parse(raw);
  } catch {
    return DEFAULT_PLANNING_DISPLAY;
  }
  if (typeof value !== "object" || value === null) {
    return DEFAULT_PLANNING_DISPLAY;
  }
  const object = value as Record<string, unknown>;
  const pick = <T extends string>(
    allowed: readonly T[],
    candidate: unknown,
    fallback: T,
  ): T =>
    typeof candidate === "string" &&
    (allowed as readonly string[]).includes(candidate)
      ? (candidate as T)
      : fallback;
  return {
    layout: pick(LAYOUTS, object.layout, DEFAULT_PLANNING_DISPLAY.layout),
    groupBy: pick(GROUPS, object.groupBy, DEFAULT_PLANNING_DISPLAY.groupBy),
    orderBy: pick(ORDERS, object.orderBy, DEFAULT_PLANNING_DISPLAY.orderBy),
    showClosed:
      typeof object.showClosed === "boolean"
        ? object.showClosed
        : DEFAULT_PLANNING_DISPLAY.showClosed,
  };
}

export function readPlanningDisplay(key: string): PlanningDisplay {
  try {
    return parsePlanningDisplay(window.localStorage.getItem(key));
  } catch {
    return DEFAULT_PLANNING_DISPLAY;
  }
}

/** `false` when the browser refused the write (quota, private mode). */
export function writePlanningDisplay(
  key: string,
  display: PlanningDisplay,
): boolean {
  try {
    window.localStorage.setItem(key, JSON.stringify(display));
    return true;
  } catch {
    return false;
  }
}
