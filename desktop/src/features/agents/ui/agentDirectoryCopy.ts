/**
 * Every sentence the Agents directory shows, as values a test can assert.
 *
 * §0 of the design binds two rules across this whole surface: only "Seated ·
 * <status> · <age>" and "Past contributor"-shaped facts exist, and every
 * contributor list ends with the footnote that offers and eligibility are
 * not recorded. `formatAge`/`seatStatusText`/`shaText` are the Roles tab's
 * own words (`roles/ui/rolesCopy.ts`), reused rather than re-worded.
 */
import {
  formatAge,
  seatStatusText,
  shaText,
} from "@/features/roles/ui/rolesCopy";
import type { AgentDirectorySeat } from "@/features/agents/lib/agentDirectoryModel";

export const AGENT_FILTERS_TESTID = "agent-filters";

/**
 * The directory toolbar's one creation affordance. It opens the same catalog
 * dialog the old agents section opened (`PersonaCatalogDialog` with
 * `AgentDialog mode="definition"`) — one button, no new dialog.
 */
export const AGENT_DIRECTORY_ADD_TESTID = "agent-directory-add";
export const AGENT_DIRECTORY_ADD_LABEL = "Add agent";

export const AGENT_FILTER_ALL_ROLES = "All roles";

export const AGENT_FILTER_ANY_STATUS = "Any status";
export const AGENT_FILTER_STATUS_RUNNING = "Running";
export const AGENT_FILTER_STATUS_STOPPED = "Stopped";
export const AGENT_FILTER_STATUS_SEATED = "Seated now";
export const AGENT_FILTER_STATUS_NOT_SEATED = "Not seated";

export const AGENT_FILTER_ANY_PROJECT = "Any project";
export const AGENT_FILTER_PROJECT_GROUP_LABEL = "holds or held a seat there";

export const AGENT_FILTER_INSTALLED_LABEL = "Installed on this computer";
export const AGENT_FILTER_INSTALLED_HELPER =
  "Showing agents known only from the wire too; this computer cannot start them.";

export const AGENT_DIRECTORY_LOADING_TESTID = "agent-directory-loading";
export const AGENT_DIRECTORY_LOADING_ARIA = "Reading agents…";

export const AGENT_DIRECTORY_EMPTY_TESTID = "agent-directory-empty";
export const AGENT_DIRECTORY_EMPTY = "No agents on this computer yet.";

export const AGENT_DIRECTORY_FILTERED_EMPTY_TESTID =
  "agent-directory-filtered-empty";
export const AGENT_DIRECTORY_FILTERED_EMPTY = "No agent matches these filters.";

export const AGENT_DIRECTORY_ERROR_TESTID = "agent-directory-error";
export function agentDirectoryErrorText(message: string): string {
  return `Agents could not be read: ${message}`;
}

export const AGENT_DIRECTORY_SEAT_NOTICE_TESTID = "agent-directory-seat-notice";
export function seatNoticeText(message: string, detail: string): string {
  return `${message} — ${detail}`;
}

/** Shown for a row's current-seat cell while the seat read is partial. */
export const AGENT_SEAT_UNKNOWN = "seat unknown";
export const AGENT_NOT_SEATED = "not seated";
export const AGENT_UNPLACED_PROJECT = "unplaced";

/** `launches as Lead`; none → `launches as — not set`. */
export function launchesAsText(homeRole: string | null): string {
  if (!homeRole) return "launches as — not set";
  return `launches as ${homeRole.charAt(0).toUpperCase()}${homeRole.slice(1)}`;
}

/**
 * `{projectName} · {status} · {age}` for the freshest open seat; `not
 * seated` with none. `seatUnknown` renders `seat unknown` instead, while the
 * seat read is partial (§A States — Partial).
 */
export function currentSeatText(
  seat: Pick<
    AgentDirectorySeat,
    "projectName" | "status" | "ageSeconds"
  > | null,
  options?: { seatUnknown?: boolean },
): string {
  if (options?.seatUnknown) return AGENT_SEAT_UNKNOWN;
  if (!seat) return AGENT_NOT_SEATED;
  const project = seat.projectName ?? AGENT_UNPLACED_PROJECT;
  return `${project} · ${seat.status} · ${formatAge(seat.ageSeconds)}`;
}

export const AGENT_INSTALLED_HERE_NO = "not installed here";

/** Detail pane identity section. */
export const AGENT_DETAIL_TESTID = "agent-detail";
export const AGENT_DETAIL_CLOSE_TESTID = "agent-detail-close";
export const AGENT_DETAIL_PUBKEY_TESTID = "agent-detail-pubkey";
export const AGENT_DETAIL_INSTALLED_YES = "yes";
export const AGENT_DETAIL_INSTALLED_NO = "no — known from the wire only";

/** Detail pane seat history. */
export const AGENT_DETAIL_SEATS_TESTID = "agent-detail-seats";
export const AGENT_DETAIL_SEAT_ROW_TESTID = "agent-detail-seat-row";
export const AGENT_DETAIL_SEATS_EMPTY = "No seat recorded for this agent.";
export const AGENT_DETAIL_SEAT_NO_ROLE = "no role";
export const AGENT_DETAIL_SEAT_UNPLACED = "unplaced";
export const OFFERS_ELIGIBILITY_FOOTNOTE =
  "Offers and eligibility are not recorded yet.";

/** `running (5m)` — the Roles tab's own combined status+age word. */
export function seatDetailStatusText(seat: AgentDirectorySeat): string {
  return seatStatusText(seat.status, seat.ageSeconds);
}

/** The 8-char sha, full sha in the row's `title`. */
export function seatDetailPackShaText(seat: AgentDirectorySeat): string {
  return shaText(seat.packSha);
}

/** Detail pane controls. */
export const AGENT_DETAIL_CONTROLS_TESTID = "agent-detail-controls";
export const AGENT_DETAIL_START_TESTID = "agent-detail-start";
export const AGENT_DETAIL_STOP_TESTID = "agent-detail-stop";
export const AGENT_DETAIL_RESTART_TESTID = "agent-detail-restart";
export const AGENT_DETAIL_EDIT_TESTID = "agent-detail-edit";
export const AGENT_DETAIL_NOT_MANAGED =
  "This computer does not manage this agent, so it cannot start or stop it.";

export const AGENT_ROW_TESTID = "agent-row";
export const AGENT_ROW_NAME_TESTID = "agent-row-name";
export const AGENT_ROW_LAUNCHES_AS_TESTID = "agent-row-launches-as";
export const AGENT_ROW_ROLE_HISTORY_TESTID = "agent-row-role-history";
export const AGENT_ROW_SEAT_TESTID = "agent-row-seat";
export const AGENT_ROW_PACK_TESTID = "agent-row-pack";
export const AGENT_ROW_NOT_INSTALLED_TESTID = "agent-row-not-installed";

export const AGENT_FILTER_ROLE_TESTID = "agent-filter-role";
export const AGENT_FILTER_STATUS_TESTID = "agent-filter-status";
export const AGENT_FILTER_PROJECT_TESTID = "agent-filter-project";
export const AGENT_FILTER_INSTALLED_TESTID = "agent-filter-installed";
