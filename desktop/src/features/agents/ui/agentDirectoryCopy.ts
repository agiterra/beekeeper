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
 * Installed for a project by its setup — a fact about identities on this
 * computer, not about sessions. With a project chosen it narrows to that
 * project's installations; it never means running.
 */
export const AGENT_FILTER_STATUS_INSTALLED_FOR_PROJECT =
  "Installed for a project";

export const AGENT_FILTER_ANY_PROJECT = "Any project";
/**
 * Under a project, the directory lists agents installed here for it and
 * agents that hold or held a seat in its sessions — two facts, either enough.
 */
export const AGENT_FILTER_PROJECT_GROUP_LABEL =
  "installed for it, or holds or held a seat there";

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

/** Shown when the setup journals could not be read. */
export const AGENT_DIRECTORY_INSTALLATIONS_ERROR_TESTID =
  "agent-directory-installations-error";
export function installationsErrorText(message: string): string {
  return `Which projects these agents were installed for could not be read, so none is shown: ${message}`;
}

/**
 * `Installed for Tank Loop · lead` — the project's setup installed this
 * identity on this computer to carry that role. Deliberately not the seat
 * cell's shape (`Tank Loop · running · 5m`): installation is not taking part
 * in a session. A project this viewer cannot list is said so, not named.
 * More than one installation adds `(+N more)`.
 */
export function installedForText(
  installations: readonly Pick<
    AgentDirectoryInstallation,
    "projectName" | "role"
  >[],
): string | null {
  const first = installations[0];
  if (!first) return null;
  const project = first.projectName ?? "a project not listed here";
  const more = installations.length - 1;
  const base = `Installed for ${project} · ${first.role}`;
  return more > 0 ? `${base} (+${more} more)` : base;
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
export const AGENT_ROW_INSTALLED_FOR_TESTID = "agent-row-installed-for";

/**
 * Inline rename on an installed project agent's row. Renaming changes the
 * name only: the same identity (pubkey) and the same role, with its relay
 * profile republished — nothing is minted and no role or instructions move.
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
  "Renames this agent in place — same identity, same role.";
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
