import type {
  CrewRoleInstallFailure,
  CrewRoleInstallFailureStage,
  InstallCrewRolePacksResponse,
} from "@/shared/api/tauriTeams";

/**
 * Copy and result derivations for the crew-role installer dialog.
 *
 * Kept out of the component so every sentence the operator reads is a value a
 * test can assert. The strings are verbatim from the front-door spec — a lane
 * that invents copy invents a comfortable version.
 */

export const INSTALL_CREW_ROLES_TITLE = "Install crew roles";

export const INSTALL_CREW_ROLES_BODY =
  "Pick a folder of role packs. Every pack whose persona declares a role becomes one agent on this computer — carrying that role and that pack — and they all join one team you can launch as a crew.";

export const INSTALL_CREW_ROLES_CHOOSE_FOLDER = "Choose folder…";

/**
 * What the installer will *try* to do, shown before it has done anything.
 *
 * Deliberately in the future tense and hedged: at this point nothing has been
 * scanned, so the roster is a plan, not a claim about seats. Once the install
 * returns, [`crewRolesSeatedNote`] replaces it with what actually happened.
 */
export const INSTALL_CREW_ROLES_ROSTER_PLAN =
  "Seats are filled from this roster, in order: lead, architect, builder, verifier, runner. A role whose pack is not in the folder holds no seat. The poker and designer packs install as agents but are never seated — the poker drives the built app, and the designer works before a crew launches.";

export const INSTALL_CREW_ROLES_REFRESH_NOTE =
  "already installed from this pack — role and pack link refreshed";

/**
 * Marks a row that installed but holds no seat.
 *
 * Without it, `poker` and `designer` — installed on purpose and seated on
 * purpose never — read exactly like the roster roles above them.
 */
export const INSTALL_CREW_ROLES_UNSEATED_NOTE =
  "installed, but not seated in the crew";

export const INSTALL_CREW_ROLES_NOTHING_FOUND =
  "No role packs in that folder. A role pack is a directory holding .plugin/plugin.json whose persona declares “role:” in its frontmatter.";

/** The menu entry that opens this dialog. */
export const INSTALL_CREW_ROLES_MENU_LABEL = "Install crew roles…";

/** A folder that could not be read, rendered verbatim with its cause. */
export function crewRolesUnreadableFolder(error: string): string {
  const message = error.trim();
  return message.startsWith("That folder could not be read")
    ? message
    : `That folder could not be read: ${message}`;
}

/** The sentence that belongs to each failure stage. */
const CREW_ROLE_FAILURE_SENTENCE: Record<CrewRoleInstallFailureStage, string> =
  {
    folder: "That folder could not be read",
    keys: "No agent key could be minted, so nothing was installed",
    store: "The packs were read, but this computer could not save them",
  };

/** `true` when `value` is the structured failure the backend returns. */
function isCrewRoleInstallFailure(
  value: unknown,
): value is CrewRoleInstallFailure {
  if (typeof value !== "object" || value === null) return false;
  const candidate = value as { failure?: unknown; detail?: unknown };
  return (
    typeof candidate.detail === "string" &&
    typeof candidate.failure === "string" &&
    candidate.failure in CREW_ROLE_FAILURE_SENTENCE
  );
}

/** The structured failure behind a thrown value, if there is one. */
function readCrewRoleInstallFailure(
  cause: unknown,
): CrewRoleInstallFailure | null {
  if (isCrewRoleInstallFailure(cause)) return cause;
  // `invokeTauri` wraps a non-Error rejection in a `TauriInvokeError` whose
  // `payload` is the value Rust serialised.
  const payload = (cause as { payload?: unknown } | null)?.payload;
  if (isCrewRoleInstallFailure(payload)) return payload;
  return null;
}

/** A thrown value's own words, with the `Error:` the runtime prepends removed. */
function crewRolesCauseDetail(cause: unknown): string {
  if (cause instanceof Error) return cause.message.trim();
  const text = String(cause).trim();
  return text.startsWith("Error:") ? text.slice("Error:".length).trim() : text;
}

/**
 * What to tell the operator about a failed install.
 *
 * The stage comes from the backend, never from the shape of the message: the
 * dialog used to wrap *every* failure in "That folder could not be read:",
 * which sent an operator whose keychain was locked to go and look at their
 * folder. A failure that names no stage is reported as a failure, and blamed
 * on nothing.
 */
export function crewRolesFailureMessage(cause: unknown): string {
  const failure = readCrewRoleInstallFailure(cause);
  if (!failure) return `The install failed: ${crewRolesCauseDetail(cause)}`;
  return `${CREW_ROLE_FAILURE_SENTENCE[failure.failure]}: ${failure.detail.trim()}`;
}

/**
 * The seats the install actually wrote.
 *
 * Never the roster: an install missing a roster pack writes a smaller crew,
 * and a constant here would name a seat the team does not hold.
 */
export function crewRolesSeatedNote(
  result: InstallCrewRolePacksResponse,
): string {
  if (result.seated.length === 0) return "No seats: this team holds no crew.";
  return `Seated: ${result.seated.join(", ")}.`;
}

/** One line per roster role that was dropped for want of a pack. */
export function crewRolesDroppedNotes(
  result: InstallCrewRolePacksResponse,
): string[] {
  return result.dropped.map(
    (role) => `${role}: no pack installed, so it holds no seat.`,
  );
}

/** Title-cased role for display: `lead` → `Lead`. */
export function crewRoleLabel(role: string): string {
  if (!role) return role;
  return role.charAt(0).toUpperCase() + role.slice(1);
}

/** The success toast. `{n}` is the count, `{roles}` the roles in seat order. */
export function crewRolesInstalledToast(
  result: InstallCrewRolePacksResponse,
): string {
  const roles = result.installed.map((row) => row.role).join(", ");
  return `Installed ${result.installed.length} crew roles into “${result.teamName}”: ${roles}.`;
}

/** One line of the result list. */
export type CrewRoleResultRow =
  | { kind: "installed"; text: string; note: string | null; seated: boolean }
  | { kind: "skipped"; text: string };

/**
 * The result list, in the order the dialog renders it: every installed role,
 * then every skipped path with its reason.
 *
 * A refreshed row says so — an operator who runs the installer twice must be
 * able to tell "nothing new happened" from "seven fresh agents appeared" — and
 * a row that installed without taking a seat says that too, so the two roles
 * that are deliberately unseated do not read like the five that are.
 */
export function crewRoleResultRows(
  result: InstallCrewRolePacksResponse,
): CrewRoleResultRow[] {
  const rows: CrewRoleResultRow[] = result.installed.map((row) => {
    const notes: string[] = [];
    if (row.refreshed) notes.push(INSTALL_CREW_ROLES_REFRESH_NOTE);
    if (!row.seated) notes.push(INSTALL_CREW_ROLES_UNSEATED_NOTE);
    return {
      kind: "installed" as const,
      text: `${crewRoleLabel(row.role)} — ${row.agentName}`,
      note: notes.length > 0 ? notes.join("; ") : null,
      seated: row.seated,
    };
  });
  for (const skipped of result.skipped) {
    rows.push({ kind: "skipped", text: `${skipped.path}: ${skipped.reason}` });
  }
  return rows;
}

/** `true` when the folder held no role pack at all. */
export function crewRolesFoundNothing(
  result: InstallCrewRolePacksResponse,
): boolean {
  return result.installed.length === 0;
}
