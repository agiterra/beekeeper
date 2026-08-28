import type { ProjectCodingSessionShelfEntry } from "./projectCodingSessionShelf";
import { projectSessionObservationTitle } from "./projectSessionObservation";

/**
 * The sidebar's one-glance session state, painted as a coloured dot.
 *
 * Closure facts (kind 44230) outrank provider metadata: an archived or closed
 * session is orange/red whatever its provider last said. Below that, the dot
 * reads the provider's last *reported* status — green only while it reported
 * `working`; everything else, including a fresh session with no turn yet and
 * a status nobody has read, is the calm blue. Liveness is not claimed: the
 * hover carries the same "last reported, not a live lease" caveat the text
 * status used to.
 */
export type ProjectSessionIndicatorState =
  | "starting"
  | "idle"
  | "running"
  | "closed"
  | "archived";

export type ProjectSessionIndicator = {
  state: ProjectSessionIndicatorState;
  /** Short state name for the hover. */
  label: string;
  /** Full hover text: the state name, plus provenance where it applies. */
  title: string;
  /** Tailwind background class for the dot. */
  colorClass: string;
};

const LABELS: Record<ProjectSessionIndicatorState, string> = {
  starting: "Starting",
  idle: "Idle",
  running: "Running",
  closed: "Closed",
  archived: "Archived",
};

const COLORS: Record<ProjectSessionIndicatorState, string> = {
  starting: "bg-blue-500",
  idle: "bg-blue-500",
  running: "bg-emerald-500",
  closed: "bg-orange-500",
  archived: "bg-red-500",
};

export function projectSessionIndicatorState(
  entry: Pick<
    ProjectCodingSessionShelfEntry,
    "isArchived" | "isClosed" | "pending" | "status"
  >,
): ProjectSessionIndicatorState {
  if (entry.isArchived) return "archived";
  if (entry.isClosed) return "closed";
  if (entry.pending === true) return "starting";
  return entry.status.kind === "working" ? "running" : "idle";
}

export function projectSessionIndicator(
  entry: Pick<
    ProjectCodingSessionShelfEntry,
    "isArchived" | "isClosed" | "pending" | "status"
  >,
): ProjectSessionIndicator {
  const state = projectSessionIndicatorState(entry);
  const label = LABELS[state];
  let title = label;
  if (state === "starting") {
    title = `${label} — waiting for the session provider`;
  } else if (state === "running") {
    // Green is the one colour that claims activity, so it alone carries the
    // caveat: last reported, not a live lease. Idle is just "Idle".
    title = `${label} — ${projectSessionObservationTitle(entry.status)}`;
  }
  return { state, label, title, colorClass: COLORS[state] };
}
