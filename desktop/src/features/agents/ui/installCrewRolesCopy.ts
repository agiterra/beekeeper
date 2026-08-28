import type { InstallCrewRolePacksResponse } from "@/shared/api/tauriTeams";

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

export const INSTALL_CREW_ROLES_ROSTER_NOTE =
  "Seated by default: lead, architect, builder, verifier, runner. The poker and designer packs are installed as agents but not seated — the poker drives the built app, and the designer works before a crew launches.";

export const INSTALL_CREW_ROLES_REFRESH_NOTE =
  "already installed from this pack — role and pack link refreshed";

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
 * able to tell "nothing new happened" from "seven fresh agents appeared".
 */
export function crewRoleResultRows(
  result: InstallCrewRolePacksResponse,
): CrewRoleResultRow[] {
  const rows: CrewRoleResultRow[] = result.installed.map((row) => ({
    kind: "installed" as const,
    text: `${crewRoleLabel(row.role)} — ${row.agentName}`,
    note: row.refreshed ? INSTALL_CREW_ROLES_REFRESH_NOTE : null,
    seated: row.seated,
  }));
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
