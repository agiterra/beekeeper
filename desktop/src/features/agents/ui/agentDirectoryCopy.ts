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
import type {
  AgentDirectoryInstallation,
  AgentDirectoryProjectAssociation,
  AgentDirectorySeat,
} from "@/features/agents/lib/agentDirectoryModel";

export const AGENT_FILTERS_TESTID = "agent-filters";

/**
 * The directory toolbar's one creation affordance. It opens the same catalog
 * dialog the old agents section opened (`PersonaCatalogDialog` with
 * `AgentDialog mode="definition"`) — one button, no new dialog.
 */
export const AGENT_DIRECTORY_ADD_TESTID = "agent-directory-add";
export const AGENT_DIRECTORY_ADD_LABEL = "Add agent";

/**
 * The project-roles bar above the directory. Three things this page keeps
 * apart, in plain words: a project's roles (shared instructions), installed
 * agents (identities on this computer), and workers taking part in a session.
 */
export const AGENTS_PROJECT_ROLES_TESTID = "agents-project-roles";
export const INSTALL_PROJECT_ROLES_TESTID = "install-project-roles";
export const AGENTS_PROJECT_ROLES_DESCRIPTION =
  "A project's roles are shared instructions. Installing them creates agents on this computer; an agent takes part in a session only when a lead picks it.";

/**
 * Saved teams, demoted below the directory. They are a convenience for adding
 * several agents to a channel at once — not how a project's roles work, and
 * not something anyone has to assemble before starting a lead.
 */
export const SAVED_AGENT_GROUPS_TESTID = "agents-saved-groups";
export const SAVED_AGENT_GROUPS_TITLE = "Saved agent groups";
export const SAVED_AGENT_GROUPS_DESCRIPTION =
  "Saved groups add several agents to a channel at once. They are not a project's roles: nobody needs a group to start a lead, and a lead picks workers by task.";

export const AGENT_FILTER_ALL_ROLES = "All roles";

export const AGENT_FILTER_ANY_STATUS = "Any status";
export const AGENT_FILTER_STATUS_RUNNING = "Running";
export const AGENT_FILTER_STATUS_STOPPED = "Stopped";
export const AGENT_FILTER_STATUS_SEATED = "Seated now";
export const AGENT_FILTER_STATUS_NOT_SEATED = "Not seated";
/**
 * Associated with a project — the agent's record says it belongs there. With
 * a project chosen it narrows to that project's agents; it never means
 * running, and a seat never satisfies it.
 */
export const AGENT_FILTER_STATUS_PROJECT_AGENT = "Belongs to a project";

export const AGENT_FILTER_ANY_PROJECT = "Any project";
/**
 * Under a project, the directory lists the agents associated with it (plus
 * any installed for it whose association is still missing, with a warning).
 * A seat in its sessions does not list an agent there.
 */
export const AGENT_FILTER_PROJECT_GROUP_LABEL =
  "agents associated with it; seats do not count";

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
 * `seated in {projectName} · {status} · {age}` for the freshest open seat;
 * `not seated` with none. A seat is where an agent is working, not which
 * project it belongs to. `seatUnknown` renders `seat unknown` instead, while
 * the seat read is partial (§A States — Partial).
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
  return `seated in ${project} · ${seat.status} · ${formatAge(seat.ageSeconds)}`;
}

export const AGENT_INSTALLED_HERE_NO = "not installed here";

/** Shown when the setup journals could not be read. */
export const AGENT_DIRECTORY_INSTALLATIONS_ERROR_TESTID =
  "agent-directory-installations-error";
export function installationsErrorText(message: string): string {
  return `Setup installations on this computer could not be read, so agents installed for a project but not yet associated with it are not flagged: ${message}`;
}

export const AGENT_NO_PROJECT = "No project";
export const AGENT_PROJECT_UNKNOWN = "Project not known on this computer";
export const AGENT_PROJECT_NOT_LISTED = "A project not listed here";

/**
 * `Tank Loop · builder` — the project this agent belongs to and its primary
 * role; `No project` when its record names none. A wire-only agent's record
 * is on another computer, so its project is said to be unknown rather than
 * none.
 */
export function agentProjectText(
  project: Pick<AgentDirectoryProjectAssociation, "projectName"> | null,
  homeRole: string | null,
  projectKnown: boolean,
): string {
  if (!projectKnown) return AGENT_PROJECT_UNKNOWN;
  if (!project) return AGENT_NO_PROJECT;
  const name = project.projectName ?? AGENT_PROJECT_NOT_LISTED;
  return homeRole ? `${name} · ${homeRole}` : name;
}

/**
 * `Installed for Tank Loop but not associated yet — the lead can't hire it.`
 * Said on the row until the record carries the association. More than one
 * installation adds `(+N more)`.
 */
export function unassociatedInstallationText(
  installations: readonly Pick<
    AgentDirectoryInstallation,
    "projectName" | "role"
  >[],
): string | null {
  const first = installations[0];
  if (!first) return null;
  const project = first.projectName ?? "a project not listed here";
  const more = installations.length - 1;
  const suffix = more > 0 ? ` (+${more} more)` : "";
  return `Installed for ${project}${suffix} but not associated yet — the lead can't hire it.`;
}

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
export const AGENT_ROW_PROJECT_TESTID = "agent-row-project";
export const AGENT_ROW_UNASSOCIATED_TESTID = "agent-row-unassociated";

/**
 * Inline rename on any row with a managed record on this computer. Renaming
 * changes the name only: the same identity (pubkey), the same role and the
 * same project association, with its relay profile republished — nothing is
 * minted and no role or instructions move.
 */
export const AGENT_ROW_RENAME_TESTID = "agent-row-rename";
export const AGENT_ROW_RENAME_FORM_TESTID = "agent-row-rename-form";
export const AGENT_ROW_RENAME_INPUT_TESTID = "agent-row-rename-input";
export const AGENT_ROW_RENAME_SAVE_TESTID = "agent-row-rename-save";
export const AGENT_ROW_RENAME_CANCEL_TESTID = "agent-row-rename-cancel";
export const AGENT_ROW_RENAME_ERROR_TESTID = "agent-row-rename-error";
export const AGENT_ROW_RENAME_LABEL = "Rename";
export const AGENT_ROW_RENAME_SAVE = "Save name";
export const AGENT_ROW_RENAME_CANCEL = "Cancel";
export const AGENT_ROW_RENAME_HINT =
  "Renames this agent in place — same identity, same role, same project.";
export const AGENT_ROW_RENAME_EMPTY = "Enter a name.";
export function renameAriaLabel(name: string): string {
  return `Rename ${name}`;
}
export function renameFailedText(message: string): string {
  return `The name was not changed: ${message}`;
}
export function renameUnpublishedText(message: string): string {
  return `Renamed on this computer, but the relay may still show the old name: ${message}`;
}

export const AGENT_FILTER_ROLE_TESTID = "agent-filter-role";
export const AGENT_FILTER_STATUS_TESTID = "agent-filter-status";
export const AGENT_FILTER_PROJECT_TESTID = "agent-filter-project";
export const AGENT_FILTER_INSTALLED_TESTID = "agent-filter-installed";
